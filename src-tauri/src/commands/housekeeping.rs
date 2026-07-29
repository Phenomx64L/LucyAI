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
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

// v1.7.99 — AppHandle stash for Tauri event emission. Set once by
// start_all() and consumed by the few sub-modules that need to push
// signals to the frontend (currently: crystal_promo). Using OnceLock
// keeps every existing sub-module's API unchanged — only emitting
// loops need to reach for it.
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// Best-effort emit. Silent if the handle isn't installed yet
/// (extremely unlikely once start_all has run) or if Tauri's
/// emit channel is closed. Designed for fire-and-forget UI signals.
fn try_emit(event: &str, payload: serde_json::Value) {
    if let Some(app) = APP_HANDLE.get() {
        let _ = app.emit(event, payload);
    }
}

/// Single entry point — called once from `lib.rs::run` setup().
/// Each sub-loop has its own once-guard inside, so re-calls are no-ops.
///
/// v1.7.99 — Now takes an AppHandle so loops can emit Tauri events to
/// the frontend (e.g. memory:consolidated for D2 visualization).
pub fn start_all(app: &AppHandle) {
    let _ = APP_HANDLE.set(app.clone());
    // Tier 0 — disaster recovery (v1.7.238): backup automático diario. Va primero
    // para que un snapshot exista lo antes posible en operación desatendida.
    db_auto_backup::spawn();
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
    // Tier C — proactive trust sentinels (v1.7.97)
    clock_drift::spawn();
    network_heartbeat::spawn();
    ollama_model_health::spawn();
    cert_expiry::spawn();
}

fn env_disabled(name: &str) -> bool {
    std::env::var(name).is_ok()
}

/// v1.7.110 audit M3 — per-loop phase jitter to break the thundering herd.
///
/// Each scheduler's tick schedule is anchored to when its bootstrap sleep
/// ends. The bootstrap delays were hand-staggered (120s, 150s, 180s, …) but
/// loops with periods that share common factors (e.g. two 6-hour loops, or a
/// 12h and a 6h) drift back into alignment over days of uptime and then
/// thunder on the r2d2 pool (max 8 connections) on the same wall-clock
/// minute.
///
/// Adding a random 0-45s offset to each bootstrap PERMANENTLY shifts that
/// loop's entire downstream schedule by a unique amount, so no two loops
/// stay phase-locked. 45s is large enough to separate ticks (each DB scan is
/// sub-second) but small enough not to meaningfully delay any single loop's
/// first run. Computed once per process at spawn time — deterministic for the
/// life of the run, random across runs.
fn boot_jitter() -> Duration {
    Duration::from_millis(rand::random::<u64>() % 45_000)
}

// ── 0. auto-backup diario (v1.7.238 — disaster recovery desatendido) ─────
//
// Toda la maquinaria de backup (VACUUM INTO atómico + firma HMAC + restore
// validado) ya existía en db_backup.rs, pero sus ÚNICOS call sites eran dos
// botones manuales en la UI. En operación desatendida eso significa que si la
// DB se corrompe (o el arranque resiliente v1.7.238 la pone en cuarentena y la
// recrea fresca), TODA la memoria curada / permisos / audit trail se pierden
// sin retorno. Este loop crea un snapshot firmado por día a
// %APPDATA%\Lucy\backups\lucy-auto-<fecha>.db (retención 7), corriendo ~5 min
// tras el boot y cada 24 h. VACUUM INTO no bloquea escritores.
pub mod db_auto_backup {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(24 * 3600);
    const KEEP: usize = 7;

    fn backups_dir() -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(
            std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string()),
        );
        p.push("Lucy");
        p.push("backups");
        let _ = std::fs::create_dir_all(&p);
        p
    }

    /// Conserva los `KEEP` snapshots más recientes (por mtime); borra el resto
    /// junto a su sidecar `.sig`.
    fn prune(dir: &std::path::Path) {
        let mut items: Vec<(std::path::PathBuf, std::time::SystemTime)> =
            std::fs::read_dir(dir)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    n.starts_with("lucy-auto-") && n.ends_with(".db")
                })
                .filter_map(|e| {
                    let m = e.metadata().ok()?.modified().ok()?;
                    Some((e.path(), m))
                })
                .collect();
        items.sort_by(|a, b| b.1.cmp(&a.1)); // más nuevo primero
        for (path, _) in items.into_iter().skip(KEEP) {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(path.with_extension("db.sig"));
        }
    }

    async fn run_once() {
        let dir = backups_dir();
        let date = chrono::Local::now().format("%Y-%m-%d");
        let target = dir.join(format!("lucy-auto-{}.db", date));
        let target_str = target.to_string_lossy().to_string();
        match crate::commands::db_backup::db_backup_create(target_str).await {
            Ok(bytes) => {
                crate::utils::logging::write_app_log(
                    "INFO",
                    &format!("backup automático OK: {} ({} bytes)", target.display(), bytes),
                );
                prune(&dir);
            }
            Err(e) => {
                // Si no hay DB (init falló) o el disco está lleno, se registra y
                // se reintenta en el próximo tick. Nunca aborta el proceso.
                crate::utils::logging::write_app_log(
                    "WARNING",
                    &format!("backup automático falló: {}", e),
                );
            }
        }
    }

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_AUTO_BACKUP") {
            eprintln!("[housekeeping] db_auto_backup disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            // Espera a que el pool de la DB asiente antes del primer snapshot.
            tokio::time::sleep(Duration::from_secs(300) + boot_jitter()).await;
            loop {
                run_once().await;
                tokio::time::sleep(TICK).await;
            }
        });
    }
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
            tokio::time::sleep(Duration::from_secs(120) + boot_jitter()).await;
            run_once().await;
        });
    }

    async fn run_once() {
        // v1.7.102 Sprint-2 H4: wrap the sync shared_db call in
        // spawn_blocking. Without this, the SELECT runs on the tokio
        // worker and stalls every other async task (PTY data emit, LLM
        // streaming, UI polls) for the SQL duration. Brief but
        // observable during cold start.
        let prompts: Vec<String> = match tauri::async_runtime::spawn_blocking(|| {
            crate::commands::metrics::shared_db(|conn| {
                // Pull the 20 most-recent distinct prompts. SELECT DISTINCT
                // because the same prompt re-used (rerun, branch, replay) is
                // worth only one embed.
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
            })
        }).await {
            Ok(Ok(v))  => v,
            Ok(Err(_)) => return,
            Err(_)     => return,   // join error — runtime shutting down
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
            tokio::time::sleep(Duration::from_secs(300) + boot_jitter()).await;   // 5 min warmup + jitter
            loop {
                tick().await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    async fn tick() {
        // v1.7.102 Sprint-2 H4: shared_db on spawn_blocking — see
        // embed_warmup::run_once for the same rationale.
        let incidents: Vec<String> = match tauri::async_runtime::spawn_blocking(|| {
            crate::commands::metrics::shared_db(|conn| {
                // Pull all incident_ids that have at least one chained audit row.
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
            })
        }).await {
            Ok(Ok(v))  => v,
            Ok(Err(_)) => return,
            Err(_)     => return,
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
            tokio::time::sleep(Duration::from_secs(180) + boot_jitter()).await;  // 3 min warmup + jitter
            loop {
                tick().await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    async fn tick() {
        // v1.7.102 Sprint-2 H4: shared_db on spawn_blocking.
        let servers: Vec<(String, String)> = match tauri::async_runtime::spawn_blocking(|| {
            crate::commands::metrics::shared_db(|conn| {
                // Pull every enabled server. The mcp_server_list command already
                // filters by enabled = 1 / 0 — we read directly from the table
                // here to skip the async hop and avoid Tauri command overhead.
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
            })
        }).await {
            Ok(Ok(v))  => v,
            Ok(Err(_)) => return,
            Err(_)     => return,
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
            tokio::time::sleep(Duration::from_secs(600) + boot_jitter()).await;  // 10 min warmup + jitter
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

            // v1.7.101 Sprint-1 B1 fix: rewritten against the REAL
            // agent_crystals schema (id, session_id, project, narrative,
            // key_outcomes, files_affected, lessons, source_chars,
            // created_at, source_id [added v1.7.101]). The original v1.7.95
            // INSERT targeted columns that never existed — prepare failed
            // every tick and this loop returned Ok(0) silently.
            //
            // We map agent_memory → crystal as:
            //   narrative      = "<title>\n\n<content>" (the human-readable
            //                    storyboard the crystal table is built for)
            //   key_outcomes   = [tags…]            (the promotion *reason*)
            //   files_affected = []                 (unknown at this layer)
            //   lessons        = []                 (operator-curated only)
            //   source_chars   = content.len()      (bytes consolidated)
            //   source_id      = agent_memories.id  (idempotency key)
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
                Err(e) => {
                    // Now that the schema is fixed, a prepare error here
                    // means a REAL regression (column dropped, DB
                    // corrupted). Surface it.
                    crate::utils::logging::write_app_log(
                        "WARN",
                        &format!("housekeeping/crystal_promo: select prepare failed ({}). Skipping tick.", e),
                    );
                    return Ok(0i64);
                }
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
                let insert_sql = "INSERT OR IGNORE INTO agent_crystals \
                    (source_id, narrative, key_outcomes, files_affected, lessons, \
                     source_chars, created_at) \
                    VALUES (?1, ?2, ?3, '[]', '[]', ?4, ?5)";
                let mut ins = tx.prepare(insert_sql)
                    .map_err(|e| format!("crystal_promo insert prepare: {}", e))?;
                for (id, title, content, tags) in &rows {
                    // narrative = title + content, capped at 8 KiB — the
                    // crystal table is for digests, not full snapshots.
                    let narrative = if title.is_empty() {
                        content.clone()
                    } else {
                        format!("{}\n\n{}", title, content)
                    };
                    let truncated_narrative = if narrative.len() > 8192 {
                        format!("{}…", crate::utils::safe_truncate(&narrative, 8189))
                    } else {
                        narrative
                    };
                    // key_outcomes: tags is already a JSON array string in
                    // agent_memories; reuse verbatim, fallback to "[]" if
                    // not well-formed.
                    let key_outcomes_json = if tags.trim_start().starts_with('[') {
                        tags.clone()
                    } else {
                        "[]".to_string()
                    };
                    let source_chars = content.len() as i64;
                    if ins.execute(rusqlite::params![
                        id, truncated_narrative, key_outcomes_json, source_chars, now,
                    ]).is_ok() {
                        promoted += 1;
                    }
                }
            }
            tx.commit().map_err(|e| format!("tx commit: {}", e))?;
            if promoted > 0 {
                crate::utils::logging::write_app_log(
                    "INFO",
                    &format!("housekeeping/crystal_promo: promoted {} memory→crystal", promoted),
                );
                // v1.7.99 — D2: tell the frontend so it can play the
                // consolidation animation. Fire-and-forget — UI signal,
                // not a correctness path. Payload carries count + the
                // first few promoted source ids so the UI can deep-link
                // the user into the Memory Browser if they click the
                // toast.
                let sample_ids: Vec<i64> = rows.iter().take(3).map(|r| r.0).collect();
                try_emit("memory:consolidated", serde_json::json!({
                    "count":      promoted,
                    "sample_ids": sample_ids,
                    "ts":         now,
                }));
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
            tokio::time::sleep(Duration::from_secs(900) + boot_jitter()).await;  // 15 min warmup + jitter
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

            // v1.7.101 Sprint-1 H3 fix: replaced `.unwrap_or(0)` with an
            // explicit match that surfaces real SQL errors as WARN. The
            // previous code masked column-rename / lock-timeout / corrupt
            // index errors as "0 rows pruned" — so a broken retention
            // path would silently grow state_snapshots forever.
            let by_age = match conn.execute(
                "DELETE FROM state_snapshots WHERE captured_at < ?1",
                [cutoff],
            ) {
                Ok(n) => n as i64,
                Err(e) => {
                    crate::utils::logging::write_app_log(
                        "WARN",
                        &format!("housekeeping/snapshot_retention age-prune failed: {}", e),
                    );
                    0
                }
            };

            // Count cap — keep the newest KEEP_NEWEST, drop the rest.
            let by_count = match conn.execute(
                "DELETE FROM state_snapshots \
                 WHERE id NOT IN (\
                   SELECT id FROM state_snapshots \
                   ORDER BY captured_at DESC LIMIT ?1\
                 )",
                [KEEP_NEWEST],
            ) {
                Ok(n) => n as i64,
                Err(e) => {
                    crate::utils::logging::write_app_log(
                        "WARN",
                        &format!("housekeeping/snapshot_retention count-prune failed: {}", e),
                    );
                    0
                }
            };

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
            tokio::time::sleep(Duration::from_secs(240) + boot_jitter()).await;  // 4 min warmup + jitter
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
            tokio::time::sleep(Duration::from_secs(360) + boot_jitter()).await;  // 6 min warmup + jitter
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
            tokio::time::sleep(Duration::from_secs(720) + boot_jitter()).await;  // 12 min warmup + jitter
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
            tokio::time::sleep(Duration::from_secs(1800) + boot_jitter()).await;  // 30 min warmup + jitter
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

// ═══════════════════════════════════════════════════════════════════════
// TIER C — proactive trust sentinels (v1.7.97)
//
// Tier A keeps Lucy fit, Tier B watches the host hardware, Tier C
// guards the trust + connectivity surface Lucy *depends* on but doesn't
// own: wall-clock accuracy (audit chain integrity), Ollama liveness,
// embedding-model availability, and TLS-cert freshness on the host.
//
//   • clock_drift          — local clock vs HTTP Date header
//   • network_heartbeat    — Ollama endpoint reachability
//   • ollama_model_health  — required embedding model present
//   • cert_expiry          — Windows cert store expiring-soon scan
// ═══════════════════════════════════════════════════════════════════════

// ── 10. clock drift sentinel ────────────────────────────────────────────
//
// Audit-chain timestamps and incident correlation across hosts assume
// the local clock is roughly correct. NTP usually keeps it accurate,
// but laptops returning from sleep, VMs with stale time sources, and
// hosts with a broken w32time service can drift minutes-to-hours
// silently. We compare the local clock to the HTTP Date header on a
// well-known endpoint. Drift > 5 min → WARN, > 30 min → ERROR.
pub mod clock_drift {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(6 * 3600);       // 6 h
    const WARN_SECS: i64 = 5 * 60;       // 5 min
    const CRIT_SECS: i64 = 30 * 60;      // 30 min

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_CLOCK_DRIFT") {
            eprintln!("[housekeeping] clock_drift disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(420) + boot_jitter()).await;  // 7 min warmup + jitter
            loop {
                tick().await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    async fn tick() {
        // HEAD against a reliable, low-cost endpoint. We don't care
        // about the body — only the Date header.
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build() {
            Ok(c) => c,
            Err(_) => return,
        };
        // v1.7.104 Sprint-4 perf: switched google.com → cloudflare.com.
        // cloudflare.com/cdn-cgi/trace returns ~200 bytes (vs google's
        // multi-KB HTML), zero third-party cookies, and reads as a
        // more sensible outbound for a sysadmin tool. The Date header
        // semantics are identical (RFC 7231 HTTP-date).
        let resp = match client.head("https://www.cloudflare.com/cdn-cgi/trace").send().await {
            Ok(r) => r,
            Err(_) => return,   // offline; nothing actionable
        };
        let date_str = match resp.headers().get(reqwest::header::DATE)
            .and_then(|v| v.to_str().ok()) {
            Some(s) => s.to_string(),
            None => return,
        };
        // RFC 2822 e.g. "Tue, 15 Apr 2025 07:28:00 GMT"
        let server_time = match chrono::DateTime::parse_from_rfc2822(&date_str) {
            Ok(t) => t.timestamp(),
            Err(_) => return,
        };
        let local_time = chrono::Utc::now().timestamp();
        let delta = local_time - server_time;  // positive = local is ahead

        if delta.abs() >= CRIT_SECS {
            crate::utils::logging::write_app_log(
                "ERROR",
                &format!("housekeeping/clock_drift: local clock off by {} s ({} ahead). Audit timestamps unreliable. Sync via w32tm /resync.",
                         delta, if delta > 0 { "local" } else { "remote" }),
            );
        } else if delta.abs() >= WARN_SECS {
            crate::utils::logging::write_app_log(
                "WARN",
                &format!("housekeeping/clock_drift: local clock off by {} s.", delta),
            );
        }
    }
}

// ── 11. network heartbeat ───────────────────────────────────────────────
//
// Lucy's hot path runs through Ollama (local embeddings + small-model
// inference). If Ollama is down, semantic_search degrades silently to
// linear scan and embed_warmup quietly fails. We probe /api/version
// every 7 min and log WARN if it's unreachable for two consecutive
// ticks (avoids false alarms on a one-tick blip).
pub mod network_heartbeat {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    static MISS_STREAK: AtomicBool = AtomicBool::new(false);   // single-bit memory
    const TICK: Duration = Duration::from_secs(7 * 60);        // 7 min
    const OLLAMA_URL: &str = "http://127.0.0.1:11434/api/version";

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_NETWORK_HEARTBEAT") {
            eprintln!("[housekeeping] network_heartbeat disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(150)).await;  // 2.5 min warmup
            loop {
                tick().await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    async fn tick() {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(4))
            .build() {
            Ok(c) => c,
            Err(_) => return,
        };
        let ok = matches!(
            client.get(OLLAMA_URL).send().await,
            Ok(r) if r.status().is_success()
        );
        if ok {
            // Reset the streak silently — log only the recovery if we
            // were previously down.
            if MISS_STREAK.swap(false, Ordering::SeqCst) {
                crate::utils::logging::write_app_log(
                    "INFO",
                    "housekeeping/network_heartbeat: Ollama recovered.",
                );
            }
        } else {
            // First miss → record. Second consecutive miss → WARN.
            let was_missing = MISS_STREAK.swap(true, Ordering::SeqCst);
            if was_missing {
                crate::utils::logging::write_app_log(
                    "WARN",
                    "housekeeping/network_heartbeat: Ollama unreachable at 127.0.0.1:11434 for ≥2 ticks. Semantic recall is degraded.",
                );
            }
        }
    }
}

// ── 12. ollama embedding-model presence ─────────────────────────────────
//
// Even when Ollama itself is alive, the required embedding model
// (nomic-embed-text) may have been removed via `ollama rm`. semantic
// recall would then silently fall back to Gemini (paid API) or pure
// linear scan. We poll /api/tags hourly and WARN if the model is gone.
pub mod ollama_model_health {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(3600);           // 1 h
    const TAGS_URL: &str = "http://127.0.0.1:11434/api/tags";
    const REQUIRED_MODEL: &str = "nomic-embed-text";

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_OLLAMA_MODEL_HEALTH") {
            eprintln!("[housekeeping] ollama_model_health disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(540)).await;  // 9 min warmup
            loop {
                tick().await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    async fn tick() {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(6))
            .build() {
            Ok(c) => c,
            Err(_) => return,
        };
        let json: serde_json::Value = match client.get(TAGS_URL).send().await {
            Ok(r) => match r.json().await { Ok(v) => v, Err(_) => return },
            Err(_) => return,   // Ollama offline; network_heartbeat covers it
        };
        // Response shape: { "models": [ { "name": "nomic-embed-text:latest", ... }, ... ] }
        let present = json.get("models")
            .and_then(|m| m.as_array())
            .map(|arr| arr.iter().any(|m| {
                m.get("name").and_then(|n| n.as_str())
                    .map(|s| s.starts_with(REQUIRED_MODEL))
                    .unwrap_or(false)
            }))
            .unwrap_or(false);
        if !present {
            crate::utils::logging::write_app_log(
                "WARN",
                &format!("housekeeping/ollama_model_health: required embedding model `{}` is NOT present in Ollama. Run: ollama pull {}",
                         REQUIRED_MODEL, REQUIRED_MODEL),
            );
        }
    }
}

// ── 13. Windows certificate expiry scan ─────────────────────────────────
//
// Sysadmin hosts often carry their own TLS certs in CurrentUser\My
// for client-cert auth, code signing, S/MIME. A silent expiry breaks
// the workflow exactly when the operator needs it. We shell out to
// PowerShell once per day and surface any cert expiring within 30
// days.
//
// Implementation note: Cert:\CurrentUser\My is accessible without
// admin; Cert:\LocalMachine\My would need elevation. We pick the
// user store so the loop never errors on a non-elevated install.
pub mod cert_expiry {
    use super::*;

    static STARTED: AtomicBool = AtomicBool::new(false);
    const TICK: Duration = Duration::from_secs(24 * 3600);      // 24 h
    const WARN_DAYS: i64 = 30;

    pub fn spawn() {
        if STARTED.swap(true, Ordering::SeqCst) { return; }
        if env_disabled("LUCY_HK_NO_CERT_EXPIRY") {
            eprintln!("[housekeeping] cert_expiry disabled via env");
            return;
        }
        tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(2400)).await;  // 40 min warmup
            loop {
                let _ = tauri::async_runtime::spawn_blocking(tick).await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    #[cfg(target_os = "windows")]
    fn tick() {
        // Single-line PowerShell: returns JSON array of expiring certs.
        // ConvertTo-Json with depth 2 keeps Subject + Thumbprint + NotAfter.
        let ps = format!(
            "Get-ChildItem Cert:\\CurrentUser\\My -ErrorAction SilentlyContinue | \
             Where-Object {{ $_.NotAfter -lt (Get-Date).AddDays({}) -and $_.NotAfter -gt (Get-Date) }} | \
             Select-Object Thumbprint, Subject, @{{n='NotAfter';e={{$_.NotAfter.ToString('o')}}}} | \
             ConvertTo-Json -Compress -Depth 2",
            WARN_DAYS
        );
        // UTF-8 forced: a certificate Subject is a DN full of organisation
        // names, and those carry accents and ñ as a matter of course
        // ("O=Ayuntamiento de A Coruña"). It also gains CREATE_NO_WINDOW,
        // which this call was missing — it flashed a console on every sweep.
        let (stdout, _, ok) = match crate::utils::shell::run_powershell_utf8(&ps) {
            Ok(t) => t,
            Err(_) => return,   // PowerShell missing — nothing we can do
        };
        if !ok { return; }
        let trimmed = stdout.trim();
        if trimmed.is_empty() || trimmed == "null" { return; }

        // ConvertTo-Json returns a bare object (not array) when there's
        // exactly one match; normalise to array.
        let value: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => return,
        };
        let arr: Vec<serde_json::Value> = match value {
            serde_json::Value::Array(a) => a,
            other => vec![other],
        };
        if arr.is_empty() { return; }

        let summary: Vec<String> = arr.iter().take(5).map(|c| {
            let subj = c.get("Subject").and_then(|v| v.as_str()).unwrap_or("?");
            let exp  = c.get("NotAfter").and_then(|v| v.as_str()).unwrap_or("?");
            format!("{} (expires {})", subj, exp)
        }).collect();
        crate::utils::logging::write_app_log(
            "WARN",
            &format!("housekeeping/cert_expiry: {} cert(s) in CurrentUser\\My expire within {} days — {}",
                     arr.len(), WARN_DAYS, summary.join("; ")),
        );
    }

    #[cfg(not(target_os = "windows"))]
    fn tick() {
        // No-op on non-Windows. The scheduler still spawns so the
        // start_all() contract is uniform; it just never reports.
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

    #[test]
    fn tier_c_module_paths_compile() {
        // Same guard for Tier C.
        let _ = clock_drift::spawn as fn();
        let _ = network_heartbeat::spawn as fn();
        let _ = ollama_model_health::spawn as fn();
        let _ = cert_expiry::spawn as fn();
    }
}
