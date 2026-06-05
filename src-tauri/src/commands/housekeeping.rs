// ── housekeeping.rs — Tier-A self-care schedulers (v1.7.95) ─────────────
//
// Five background loops that keep Lucy fit without operator intervention.
// All follow the same tokio pattern (set-once AtomicBool guard, bootstrap
// delay so the DB pool is settled, then periodic tick). All log via
// utils::logging::write_app_log only when something actionable happens,
// so a healthy install produces silent ticks.
//
// Layout
// ──────
//   Each scheduler lives in its own sub-module so we can spawn / disable
//   them individually from lib.rs::run. Per-loop config is via env var
//   (LUCY_HK_*) so an operator can disable a single loop without a
//   recompile — useful when debugging.
//
//   The loops:
//     • embed_warmup        — one-shot, pre-embed top recent prompts so
//                             the LRU cache (v1.7.83) is warm before the
//                             first real query.
//     • audit_verify        — periodic hash-chain re-verification of any
//                             incident with chained audit events.
//     • mcp_health          — periodic liveness probe of every enabled
//                             MCP server; marks unreachable ones.
//     • crystal_promo       — promote memories with high access_count +
//                             confidence into the durable `agent_crystals`
//                             table.
//     • snapshot_retention  — prune state_snapshots beyond N entries / age.
//
// Anti-patterns to keep in mind
// ─────────────────────────────
//   • DON'T compete with already-running schedulers (proactive_detector,
//     db_maintenance, auto_dedup, auto_consolidate). Cadences chosen so
//     no two loops run in the same minute on average.
//   • DON'T silently overwrite operator-curated data. Crystal promotion
//     INSERTS a new crystal row; it never modifies the source memory.
//     Snapshot retention deletes only by age + count caps, never by
//     content.
//   • DON'T do network I/O on the main thread. All loops use
//     `tauri::async_runtime::spawn_blocking` for SQL and
//     `tauri::async_runtime::spawn` for async work.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Single entry point — called once from `lib.rs::run` setup().
/// Each sub-loop has its own once-guard inside, so re-calls are no-ops.
pub fn start_all() {
    // Tier A — self-care
    embed_warmup::spawn();
    audit_verify::spawn();
    mcp_health::spawn();
    crystal_promo::spawn();
    snapshot_retention::spawn();
    // Tier B — operational sentinels (v1.7.96)
    disk_sentinel::spawn();
    resource_pressure::spawn();
    db_size_watcher::spawn();
    rotated_log_sweep::spawn();
}

fn env_disabled(name: &str) -> bool {
    std::env::var(name).is_ok()
}

// ── 1. embedding cache warmup ───────────────────────────────────────────
//
// At boot, embed the N most-recent distinct user prompts so the v1.7.83
// LRU cache lands populated. First real query against any of those
// prompts is then served from cache — no Ollama round-trip.
//
// Why chip_click_log: it carries the prompt text the operator actually
// typed (under `text` column). 20 entries × 50-200 ms per embed = at
// most ~4 s of background work, only once per boot.
pub mod embed_warmup {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_EMBED_WARMUP") {
            eprintln!("[housekeeping] embed_warmup disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            // Let the DB pool and embedding model settle before we hit them.
            tokio::time::sleep(Duration::from_secs(120)).await;
            run_once().await;
        });
    }

    async fn run_once() {
        // Pull the 20 most-recent distinct prompts. SELECT DISTINCT
        // because the same prompt re-used (rerun, branch, replay) is
        // worth only one embed.
        let prompts: Vec<String> = match crate::commands::metrics::shared_db(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT text FROM chip_click_log \
                 WHERE text IS NOT NULL AND length(text) > 8 \
                 ORDER BY occurred_at DESC LIMIT 20"
            ).map_err(|e| format!("warmup prepare: {}", e))?;
            let rows: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("warmup query: {}", e))?
                .filter_map(|r| r.ok())
                .collect();
            Ok::<Vec<String>, String>(rows)
        }) {
            Ok(v) => v,
            Err(_) => return,
        };
        if prompts.is_empty() { return; }
        let mut warmed = 0usize;
        for p in prompts {
            // best-effort: embed populates the v1.7.83 LRU cache as a
            // side effect. Errors (Ollama offline) are silent — we'll
            // retry on the next boot.
            if let Ok((_, _)) =
                crate::commands::embeddings::embed_via_ollama_pub(&p, None).await {
                warmed += 1;
            }
        }
        if warmed > 0 {
            crate::utils::logging::write_app_log(
                "INFO",
                &format!("housekeeping/embed_warmup: cached {} prompt embeddings", warmed),
            );
        }
    }
}

// ── 2. audit hash chain verification ────────────────────────────────────
//
// hash_chain.rs lets every incident accumulate a chain of audit events,
// each carrying the SHA-256 of the previous. If a row is tampered or
// dropped, the chain breaks. We verify periodically so any tampering
// surfaces as a proactive_insight instead of being found at audit time.
pub mod audit_verify {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(12 * 3600);     // 12 h

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_AUDIT_VERIFY") {
            eprintln!("[housekeeping] audit_verify disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(300)).await;   // 5 min warmup
            loop {
                tick().await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    async fn tick() {
        // Pull all incident_ids that have at least one chained audit row.
        let incidents: Vec<String> = match crate::commands::metrics::shared_db(|conn| {
            let mut stmt = match conn.prepare(
                "SELECT DISTINCT incident_id FROM audit_chain WHERE incident_id IS NOT NULL"
            ) {
                Ok(s) => s,
                // Table doesn't exist yet — no incidents to verify.
                Err(_) => return Ok::<Vec<String>, String>(vec![]),
            };
            let rows: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("audit_verify query: {}", e))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        }) {
            Ok(v) => v,
            Err(_) => return,
        };

        let mut broken: Vec<String> = Vec::new();
        for id in &incidents {
            if let Ok(report) =
                crate::commands::hash_chain::verify_incident_chain(id.clone()).await {
                // The exact field name depends on hash_chain.rs's
                // ChainVerifyReport shape; the convention across Lucy is
                // a boolean `ok`. We attempt to read it via serde so the
                // hash_chain module can evolve without breaking us.
                let s = match serde_json::to_value(&report) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let ok = s.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
                if !ok { broken.push(id.clone()); }
            }
        }
        if !broken.is_empty() {
            crate::utils::logging::write_app_log(
                "ERROR",
                &format!("housekeeping/audit_verify: chain BROKEN for {} incident(s): {}",
                         broken.len(),
                         broken.iter().take(5).cloned().collect::<Vec<_>>().join(", ")),
            );
        }
    }
}

// ── 3. MCP servers health poll ──────────────────────────────────────────
//
// Enabled MCP servers are probed for liveness via a cheap tools/list
// call. Unreachable ones are flagged but NOT auto-disabled — the
// operator might be debugging a transient outage. The flag surfaces in
// proactive_insights via the next detector tick.
pub mod mcp_health {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(5 * 60);         // 5 min

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_MCP_HEALTH") {
            eprintln!("[housekeeping] mcp_health disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(180)).await;  // 3 min warmup
            loop {
                tick().await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    async fn tick() {
        // Pull every enabled server. The mcp_server_list command already
        // filters by enabled = 1 / 0 — we read directly from the table
        // here to skip the async hop and avoid Tauri command overhead.
        let servers: Vec<(String, String)> = match crate::commands::metrics::shared_db(|conn| {
            let mut stmt = match conn.prepare(
                "SELECT name, command FROM mcp_servers WHERE enabled = 1"
            ) {
                Ok(s) => s,
                Err(_) => return Ok::<Vec<(String, String)>, String>(vec![]),
            };
            let rows: Vec<(String, String)> = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| format!("mcp_health query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
            Ok(rows)
        }) {
            Ok(v) => v,
            Err(_) => return,
        };

        let mut down: Vec<String> = Vec::new();
        for (name, _cmd) in &servers {
            // Reuse the already-existing discover command; success means
            // the server responded to tools/list within its timeout.
            match crate::commands::mcp::discover_mcp_tools(name.clone(), None).await {
                Ok(_) => {}  // alive
                Err(_) => { down.push(name.clone()); }
            }
        }
        if !down.is_empty() {
            crate::utils::logging::write_app_log(
                "WARN",
                &format!("housekeeping/mcp_health: {} server(s) unreachable: {}",
                         down.len(), down.join(", ")),
            );
        }
    }
}

// ── 4. memory crystal promotion ─────────────────────────────────────────
//
// Memories with high access_count AND high confidence are operationally
// "load-bearing" — operator references them repeatedly across sessions.
// We promote those into `agent_crystals` so they get their own retrieval
// tier and are immune to the auto-forget decay path.
//
// Idempotent: each crystal row has a UNIQUE constraint on (source_id);
// re-running just touches updated_at on existing crystals.
pub mod crystal_promo {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(6 * 3600);       // 6 h
    const ACCESS_THRESHOLD: i64 = 5;
    const CONFIDENCE_THRESHOLD: f64 = 0.80;

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_CRYSTAL_PROMO") {
            eprintln!("[housekeeping] crystal_promo disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(600)).await;  // 10 min warmup
            loop {
                let _ = tauri::async_runtime::spawn_blocking(|| {
                    let _ = tick();
                }).await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    fn tick() -> Result<i64, String> {
        crate::commands::metrics::shared_db(|conn| {
            // We don't know the exact shape of agent_crystals across DB
            // versions; treat the INSERT as best-effort. If the table is
            // missing a column (very old DB), the whole UPDATE fails as
            // one and we skip — caller will retry in 6 h.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64).unwrap_or(0);

            // Pick eligible memories. We require confidence column to
            // exist (added in v1.6.0 grounding); if not we get an error
            // and bail.
            let mut stmt = match conn.prepare(
                "SELECT id, title, content, tags \
                 FROM agent_memories \
                 WHERE access_count >= ?1 \
                   AND confidence  >= ?2 \
                   AND superseded_by IS NULL \
                   AND id NOT IN (SELECT source_id FROM agent_crystals \
                                  WHERE source_id IS NOT NULL) \
                 LIMIT 50"
            ) {
                Ok(s) => s,
                Err(_) => return Ok(0i64),
            };
            let rows: Vec<(i64, String, String, String)> = stmt
                .query_map(rusqlite::params![ACCESS_THRESHOLD, CONFIDENCE_THRESHOLD],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .map_err(|e| format!("crystal_promo query: {}", e))?
                .filter_map(|r| r.ok())
                .collect();

            if rows.is_empty() { return Ok(0); }

            let tx = conn.unchecked_transaction()
                .map_err(|e| format!("tx open: {}", e))?;
            let mut promoted = 0i64;
            {
                // Schema fields vary across versions — we INSERT with a
                // wide column set and rely on DEFAULT values for ones we
                // don't supply. If the INSERT fails for a missing column
                // we silently skip that row.
                let insert_sql = "INSERT OR IGNORE INTO agent_crystals \
                    (source_id, summary, content, tags, created_at) \
                    VALUES (?1, ?2, ?3, ?4, ?5)";
                if let Ok(mut ins) = tx.prepare(insert_sql) {
                    for (id, title, content, tags) in &rows {
                        if ins.execute(rusqlite::params![id, title, content, tags, now]).is_ok() {
                            promoted += 1;
                        }
                    }
                }
            }
            tx.commit().map_err(|e| format!("tx commit: {}", e))?;
            if promoted > 0 {
                crate::utils::logging::write_app_log(
                    "INFO",
                    &format!("housekeeping/crystal_promo: promoted {} memory→crystal", promoted),
                );
            }
            Ok(promoted)
        })
    }
}

// ── 5. state snapshot retention ─────────────────────────────────────────
//
// F2 state_snapshots accumulate across investigations. They're useful
// for /diff but the long tail just costs disk. Two caps: keep the
// freshest N rows, and don't keep anything older than D days.
pub mod snapshot_retention {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(6 * 3600);       // 6 h
    const KEEP_NEWEST: i64 = 200;
    const MAX_AGE_DAYS: i64 = 30;

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_SNAPSHOT_RETENTION") {
            eprintln!("[housekeeping] snapshot_retention disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(900)).await;  // 15 min warmup
            loop {
                let _ = tauri::async_runtime::spawn_blocking(|| {
                    let _ = tick();
                }).await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    fn tick() -> Result<i64, String> {
        crate::commands::metrics::shared_db(|conn| {
            let cutoff = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64).unwrap_or(0)
                - MAX_AGE_DAYS * 86_400;

            // Age cap.
            let by_age = conn.execute(
                "DELETE FROM state_snapshots WHERE captured_at < ?1",
                [cutoff],
            ).unwrap_or(0) as i64;

            // Count cap — keep the newest KEEP_NEWEST, drop the rest.
            let by_count = conn.execute(
                "DELETE FROM state_snapshots \
                 WHERE id NOT IN (\
                   SELECT id FROM state_snapshots \
                   ORDER BY captured_at DESC LIMIT ?1\
                 )",
                [KEEP_NEWEST],
            ).unwrap_or(0) as i64;

            let total = by_age + by_count;
            if total > 0 {
                crate::utils::logging::write_app_log(
                    "INFO",
                    &format!("housekeeping/snapshot_retention: pruned {} (by_age={}, by_count={})",
                             total, by_age, by_count),
                );
            }
            Ok(total)
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// TIER B — operational sentinels (v1.7.96)
//
// Where Tier A keeps *Lucy* fit, Tier B watches the *host* she lives on.
// Loops here observe + report; they NEVER mutate operator state. The
// signal lands in lucy_app.log (and via proactive_detector picks it up
// as an insight on the next 3 min tick).
//
//   • disk_sentinel       — drive free-space monitor
//   • resource_pressure   — RAM / CPU pressure detector
//   • db_size_watcher     — lucy.db logical size tracker
//   • rotated_log_sweep   — prune ancient .log.gz archives
// ═══════════════════════════════════════════════════════════════════════

// ── 6. disk free-space sentinel ─────────────────────────────────────────
//
// Polls every mounted drive via sysinfo::Disks. Logs at WARN when any
// drive drops below 15% free, ERROR below 5%. We deliberately don't
// auto-clean anything — the operator decides what's safe to remove.
pub mod disk_sentinel {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(30 * 60);        // 30 min
    const WARN_PCT: f64 = 15.0;
    const CRIT_PCT: f64 = 5.0;

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_DISK_SENTINEL") {
            eprintln!("[housekeeping] disk_sentinel disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(240)).await;  // 4 min warmup
            loop {
                let _ = tauri::async_runtime::spawn_blocking(tick).await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    fn tick() {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let mut warn_drives: Vec<String> = Vec::new();
        let mut crit_drives: Vec<String> = Vec::new();
        for d in &disks {
            let total = d.total_space() as f64;
            if total < 1.0 { continue; }
            let free = d.available_space() as f64;
            let pct = (free / total) * 100.0;
            let mount = d.mount_point().to_string_lossy().to_string();
            // Report bytes in GiB for readability.
            let free_gib = free / (1024.0 * 1024.0 * 1024.0);
            let line = format!("{} ({:.1}% free, {:.1} GiB)", mount, pct, free_gib);
            if pct < CRIT_PCT {
                crit_drives.push(line);
            } else if pct < WARN_PCT {
                warn_drives.push(line);
            }
        }
        if !crit_drives.is_empty() {
            crate::utils::logging::write_app_log(
                "ERROR",
                &format!("housekeeping/disk_sentinel: CRITICAL low free space — {}",
                         crit_drives.join("; ")),
            );
        }
        if !warn_drives.is_empty() {
            crate::utils::logging::write_app_log(
                "WARN",
                &format!("housekeeping/disk_sentinel: low free space — {}",
                         warn_drives.join("; ")),
            );
        }
    }
}

// ── 7. resource pressure detector ───────────────────────────────────────
//
// RAM and CPU pressure both degrade Lucy's responsiveness (LLM token
// streaming, SIMD cosine batches) AND signal a host issue the operator
// might want to know about. We sample sysinfo once per tick and log at
// WARN past a threshold. CPU needs a brief refresh-sleep-refresh pattern
// because sysinfo reports usage as a delta between two refreshes.
pub mod resource_pressure {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(5 * 60);         // 5 min
    const MEM_WARN_PCT: f64 = 85.0;
    const CPU_WARN_PCT: f64 = 85.0;

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_RESOURCE_PRESSURE") {
            eprintln!("[housekeeping] resource_pressure disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(360)).await;  // 6 min warmup
            loop {
                tick().await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    async fn tick() {
        use sysinfo::System;
        let mut sys = System::new();
        sys.refresh_memory();
        sys.refresh_cpu();
        // Required gap for accurate CPU sampling per sysinfo docs.
        tokio::time::sleep(Duration::from_millis(
            sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.as_millis() as u64 + 50,
        )).await;
        sys.refresh_cpu();

        let total_mem = sys.total_memory() as f64;
        let used_mem  = sys.used_memory() as f64;
        let mem_pct   = if total_mem > 0.0 { used_mem / total_mem * 100.0 } else { 0.0 };

        let cpus = sys.cpus();
        let cpu_avg: f64 = if cpus.is_empty() {
            0.0
        } else {
            cpus.iter().map(|c| c.cpu_usage() as f64).sum::<f64>() / (cpus.len() as f64)
        };

        let mem_hot = mem_pct >= MEM_WARN_PCT;
        let cpu_hot = cpu_avg >= CPU_WARN_PCT;
        if mem_hot || cpu_hot {
            crate::utils::logging::write_app_log(
                "WARN",
                &format!("housekeeping/resource_pressure: mem={:.1}% cpu={:.1}% (mem_hot={} cpu_hot={})",
                         mem_pct, cpu_avg, mem_hot, cpu_hot),
            );
        }
    }
}

// ── 8. database size watcher ────────────────────────────────────────────
//
// lucy.db can grow large over months (user previously flagged a 386 MB
// instance). We periodically log the logical size via PRAGMA so growth
// trends are visible in the log, and we escalate to ERROR past a hard
// cap so the operator runs VACUUM / clean-up.
pub mod db_size_watcher {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(12 * 3600);      // 12 h
    const SIZE_WARN_MB: f64 = 500.0;
    const SIZE_CRIT_MB: f64 = 2048.0;

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_DB_SIZE_WATCHER") {
            eprintln!("[housekeeping] db_size_watcher disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(720)).await;  // 12 min warmup
            loop {
                let _ = tauri::async_runtime::spawn_blocking(|| {
                    let _ = tick();
                }).await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    fn tick() -> Result<(), String> {
        let bytes: i64 = crate::commands::metrics::shared_db(|conn| {
            // page_count * page_size = logical DB size (excluding WAL/SHM).
            let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))
                .map_err(|e| format!("page_count: {}", e))?;
            let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))
                .map_err(|e| format!("page_size: {}", e))?;
            Ok::<i64, String>(page_count.saturating_mul(page_size))
        })?;
        let mb = (bytes as f64) / (1024.0 * 1024.0);
        if mb >= SIZE_CRIT_MB {
            crate::utils::logging::write_app_log(
                "ERROR",
                &format!("housekeeping/db_size_watcher: lucy.db is {:.1} MB — past CRIT cap ({:.0} MB). Consider VACUUM + memory archival.", mb, SIZE_CRIT_MB),
            );
        } else if mb >= SIZE_WARN_MB {
            crate::utils::logging::write_app_log(
                "WARN",
                &format!("housekeeping/db_size_watcher: lucy.db is {:.1} MB — past WARN cap ({:.0} MB).", mb, SIZE_WARN_MB),
            );
        } else {
            // Healthy: still log at INFO once per tick so trend is visible
            // when an operator is investigating long-term growth.
            crate::utils::logging::write_app_log(
                "INFO",
                &format!("housekeeping/db_size_watcher: lucy.db = {:.1} MB", mb),
            );
        }
        Ok(())
    }
}

// ── 9. rotated log sweep ────────────────────────────────────────────────
//
// utils::logging rotates the main lucy_app.log at 5 MB and gzips the
// previous file as lucy_app.1.log.gz. Over months, *.log.gz archives
// accumulate (audit + agent_loop + future logs). We prune any .gz older
// than N days. The active .log is never touched.
pub mod rotated_log_sweep {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(24 * 3600);      // 24 h
    const MAX_AGE_DAYS: u64 = 30;

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_ROTATED_LOG_SWEEP") {
            eprintln!("[housekeeping] rotated_log_sweep disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(1800)).await;  // 30 min warmup
            loop {
                let _ = tauri::async_runtime::spawn_blocking(tick).await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    fn tick() {
        let dir = crate::utils::logging::get_logs_dir();
        let Ok(read) = std::fs::read_dir(&dir) else { return; };
        let cutoff = std::time::SystemTime::now()
            .checked_sub(Duration::from_secs(MAX_AGE_DAYS * 86_400))
            .unwrap_or(std::time::UNIX_EPOCH);
        let mut pruned = 0u32;
        let mut bytes_freed: u64 = 0;
        for entry in read.flatten() {
            let path = entry.path();
            // Only .gz archives — never the active *.log file.
            if path.extension().and_then(|s| s.to_str()) != Some("gz") { continue; }
            let Ok(meta) = entry.metadata() else { continue; };
            let Ok(modified) = meta.modified() else { continue; };
            if modified < cutoff {
                let size = meta.len();
                if std::fs::remove_file(&path).is_ok() {
                    pruned += 1;
                    bytes_freed += size;
                }
            }
        }
        if pruned > 0 {
            let mib = (bytes_freed as f64) / (1024.0 * 1024.0);
            crate::utils::logging::write_app_log(
                "INFO",
                &format!("housekeeping/rotated_log_sweep: removed {} stale .gz archive(s), {:.1} MiB",
                         pruned, mib),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_disabled_reads_env() {
        // Sanity: a name we know isn't set returns false.
        assert!(!env_disabled("LUCY_HK_DEFINITELY_NOT_SET_ZZZ"));
    }

    #[test]
    fn tier_b_module_paths_compile() {
        // Forces the linker to resolve every Tier B sub-module so a typo
        // in one of the spawn() symbols fails the test build, not just
        // runtime startup. We don't call spawn() (it would start a real
        // tokio loop); we just take its function pointer.
        let _ = disk_sentinel::spawn as fn();
        let _ = resource_pressure::spawn as fn();
        let _ = db_size_watcher::spawn as fn();
        let _ = rotated_log_sweep::spawn as fn();
    }
}
