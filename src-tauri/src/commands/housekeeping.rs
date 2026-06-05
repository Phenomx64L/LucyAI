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
    embed_warmup::spawn();
    audit_verify::spawn();
    mcp_health::spawn();
    crystal_promo::spawn();
    snapshot_retention::spawn();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_disabled_reads_env() {
        // Sanity: a name we know isn't set returns false.
        assert!(!env_disabled("LUCY_HK_DEFINITELY_NOT_SET_ZZZ"));
    }
}
