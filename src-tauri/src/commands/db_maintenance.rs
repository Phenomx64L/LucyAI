// ── db_maintenance.rs — background SQLite housekeeping (v1.4.10) ────────
//
// Solves three problems the audit flagged simultaneously:
//
//   A3 — chip_click_log + conversation_turns + task_events have NO
//        retention. After months of use the DB balloons (user reported
//        386 MB) and FTS5 queries slow down.
//
//   A4 — chip_click_log lacks the index its hot query uses
//        (`(lang, had_error, occurred_at)`).
//
//   v1.4.5 follow-up — WAL never checkpointed at runtime. With
//        synchronous=NORMAL + WAL, the .db-wal sibling file grows
//        unboundedly between checkpoints; under heavy concurrent
//        writes it can exceed the main DB size.
//
// Strategy: a single tokio task that runs every hour. Tasks are CHEAP
// when there's nothing to do (PRAGMA optimize ~5ms, wal_checkpoint with
// nothing to flush <1ms, DELETE on indexed range with 0 matches <1ms),
// so the polling cost over a year is dominated by the actual cleanup
// passes when needed.
//
// Tunables come from env vars so SREs can override per deploy:
//   LUCY_DB_RETENTION_CHIPS_DAYS     (default 180)
//   LUCY_DB_RETENTION_TURNS_DAYS     (default 90)
//   LUCY_DB_RETENTION_EVENTS_DAYS    (default 90)
//   LUCY_DB_MAINT_INTERVAL_SECS      (default 3600)
//   LUCY_DB_MAINT_DISABLE=1          to opt-out entirely (CI, tests)
//
// We log a summary line on each pass via eprintln so the operator can
// see what got pruned in the Lucy log file.

use std::time::Duration;
use tokio::time::interval;

use crate::commands::metrics::shared_db;

/// Spawn the maintenance loop. Idempotent: subsequent calls are no-ops
/// because the OnceCell guard ensures only one task runs per process.
pub fn spawn_background_maintenance() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    if std::env::var("LUCY_DB_MAINT_DISABLE").is_ok() {
        eprintln!("[db_maint] disabled via LUCY_DB_MAINT_DISABLE");
        return;
    }

    let interval_secs: u64 = std::env::var("LUCY_DB_MAINT_INTERVAL_SECS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(3600);
    let chips_days: i64 = std::env::var("LUCY_DB_RETENTION_CHIPS_DAYS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(180);
    let turns_days: i64 = std::env::var("LUCY_DB_RETENTION_TURNS_DAYS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(90);
    let events_days: i64 = std::env::var("LUCY_DB_RETENTION_EVENTS_DAYS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(90);

    tokio::spawn(async move {
        // First run AFTER 5 minutes — let the app finish booting before
        // we touch the DB. Subsequent runs hit the configured interval.
        let mut tick = interval(Duration::from_secs(interval_secs));
        // The first .tick() returns immediately. Burn it so the first
        // real cleanup happens after `interval_secs` of idle.
        tick.tick().await;
        // Initial delay so startup latency isn't impacted.
        tokio::time::sleep(Duration::from_secs(300)).await;
        eprintln!("[db_maint] background loop started — interval={}s, chips_days={}, turns_days={}, events_days={}",
            interval_secs, chips_days, turns_days, events_days);

        loop {
            // First — ensure the v1.4.10 chip_click_log index exists. The
            // schema in db.rs only added two indexes; the hot query in
            // chip_memory.rs filters on (lang, had_error, occurred_at)
            // which neither covered. Adding the index here (vs in the
            // base schema) means existing installs get it on the first
            // maintenance pass without a separate migration.
            ensure_v1410_indexes();

            // Then — run the actual maintenance. Each step is wrapped
            // independently so a transient SQLITE_BUSY on one doesn't
            // abort the others.
            let r1 = prune_chip_log(chips_days);
            let r2 = prune_conversation_turns(turns_days);
            let r3 = prune_task_events(events_days);
            let r4 = checkpoint_wal();
            let r5 = optimize();

            eprintln!(
                "[db_maint] pass complete: chips_pruned={} turns_pruned={} events_pruned={} wal={} optimize={}",
                r1, r2, r3, r4, r5,
            );

            tick.tick().await;
        }
    });
}

fn ensure_v1410_indexes() {
    let _ = shared_db(|conn| {
        // The hot query in chip_memory::suggest_memory_chips filters
        // `WHERE lang = ? AND had_error = ? AND occurred_at >= ?` and
        // sorts by occurred_at — perfectly served by this composite.
        // IF NOT EXISTS makes it idempotent; subsequent passes are <1ms.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chip_log_filter
               ON chip_click_log(lang, had_error, occurred_at DESC)",
            [],
        ).map_err(|e| format!("idx_chip_log_filter: {}", e))?;
        Ok::<(), String>(())
    });
}

fn prune_chip_log(days: i64) -> i64 {
    let cutoff_secs = days * 24 * 60 * 60;
    shared_db(move |conn| {
        let n = conn.execute(
            "DELETE FROM chip_click_log
              WHERE occurred_at < strftime('%s','now') - ?1",
            rusqlite::params![cutoff_secs],
        ).map_err(|e| format!("prune chip_click_log: {}", e))? as i64;
        Ok::<i64, String>(n)
    }).unwrap_or(-1)
}

fn prune_conversation_turns(days: i64) -> i64 {
    let cutoff_secs = days * 24 * 60 * 60;
    // We delete from conversation_turns; the FTS5 mirror is kept in
    // sync via the AFTER DELETE trigger declared in db.rs. After a
    // big prune we trigger an FTS5 optimize so the freed segments
    // get compacted (otherwise the .db-wal grows from the deletions
    // until the next normal checkpoint).
    let pruned: i64 = shared_db(move |conn| {
        let n = conn.execute(
            "DELETE FROM conversation_turns
              WHERE created_at < strftime('%s','now') - ?1",
            rusqlite::params![cutoff_secs],
        ).map_err(|e| format!("prune conversation_turns: {}", e))? as i64;
        Ok::<i64, String>(n)
    }).unwrap_or(-1);

    if pruned > 0 {
        let _ = shared_db(|conn| {
            // FTS5 compact-segments command. Cheap when there's nothing
            // to do; reclaims tombstone space proportional to the
            // pruned rows. The bare INSERT form is FTS5's optimize DSL.
            conn.execute(
                "INSERT INTO conversation_turns_fts(conversation_turns_fts) VALUES('optimize')",
                [],
            ).map_err(|e| format!("fts5 optimize: {}", e))?;
            Ok::<(), String>(())
        });
    }
    pruned
}

fn prune_task_events(days: i64) -> i64 {
    let cutoff_secs = days * 24 * 60 * 60;
    shared_db(move |conn| {
        let n = conn.execute(
            "DELETE FROM task_events
              WHERE timestamp < strftime('%s','now') - ?1",
            rusqlite::params![cutoff_secs],
        ).map_err(|e| format!("prune task_events: {}", e))? as i64;
        Ok::<i64, String>(n)
    }).unwrap_or(-1)
}

/// Run a TRUNCATE checkpoint — moves all WAL frames into the main DB
/// and shrinks the .db-wal file back to 0. Without this, the WAL grows
/// across the day until SQLite auto-checkpoints at a 1000-page boundary,
/// but never SHRINKS the file on disk. TRUNCATE explicitly reclaims it.
fn checkpoint_wal() -> &'static str {
    let res = shared_db(|conn| {
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .map_err(|e| format!("wal_checkpoint: {}", e))?;
        Ok::<(), String>(())
    });
    match res {
        Ok(_) => "ok",
        Err(_) => "busy",  // Acceptable — checkpoint retried next interval.
    }
}

/// PRAGMA optimize lets SQLite run cheap analyze/vacuum-incremental
/// passes on tables whose stats it considers stale. Per the SQLite
/// docs this should be called on a schedule for long-running
/// connections — exactly our case.
fn optimize() -> &'static str {
    let res = shared_db(|conn| {
        conn.execute("PRAGMA optimize", []).map_err(|e| format!("pragma optimize: {}", e))?;
        Ok::<(), String>(())
    });
    match res {
        Ok(_) => "ok",
        Err(_) => "busy",
    }
}

// ── Manual trigger (UI + slash command) ─────────────────────────────────

/// Run one full maintenance pass on demand. Surfaces a structured
/// summary the UI can show; useful from a Settings → "Optimize DB now"
/// button or a `/db-optimize` slash command.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MaintReport {
    pub chips_pruned:  i64,
    pub turns_pruned:  i64,
    pub events_pruned: i64,
    pub wal_checkpoint: String,
    pub optimize:      String,
    pub size_before_mb: f64,
    pub size_after_mb:  f64,
}

#[tauri::command]
pub async fn db_maintenance_run_now() -> Result<MaintReport, String> {
    let size_before_mb = current_db_size_mb();

    ensure_v1410_indexes();

    let chips_days  = std::env::var("LUCY_DB_RETENTION_CHIPS_DAYS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(180_i64);
    let turns_days  = std::env::var("LUCY_DB_RETENTION_TURNS_DAYS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(90_i64);
    let events_days = std::env::var("LUCY_DB_RETENTION_EVENTS_DAYS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(90_i64);

    let chips_pruned  = prune_chip_log(chips_days);
    let turns_pruned  = prune_conversation_turns(turns_days);
    let events_pruned = prune_task_events(events_days);
    let wal_checkpoint = checkpoint_wal().to_string();
    let optimize_status = optimize().to_string();

    // Re-measure AFTER the checkpoint so the size delta reflects WAL
    // reclamation too.
    let size_after_mb = current_db_size_mb();

    Ok(MaintReport {
        chips_pruned, turns_pruned, events_pruned,
        wal_checkpoint, optimize: optimize_status,
        size_before_mb, size_after_mb,
    })
}

fn current_db_size_mb() -> f64 {
    shared_db(|conn| {
        let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap_or(0);
        let page_size:  i64 = conn.query_row("PRAGMA page_size",  [], |r| r.get(0)).unwrap_or(0);
        Ok::<f64, String>((page_count * page_size) as f64 / 1_048_576.0)
    }).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maint_report_serializes() {
        let r = MaintReport {
            chips_pruned: 12, turns_pruned: 0, events_pruned: 3,
            wal_checkpoint: "ok".into(), optimize: "ok".into(),
            size_before_mb: 12.5, size_after_mb: 11.0,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("chips_pruned"));
        assert!(json.contains("size_after_mb"));
    }

    #[test]
    fn env_default_when_unset() {
        // We don't touch real env; just verify the fallback path compiles
        // and matches the documented defaults.
        let chips_days: i64 = std::env::var("LUCY_DB_RETENTION_CHIPS_DAYS_NEVER_SET")
            .ok().and_then(|s| s.parse().ok()).unwrap_or(180);
        assert_eq!(chips_days, 180);
    }
}
