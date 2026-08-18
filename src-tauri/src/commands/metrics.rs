// ── Cost tracking, permission rules, and skill management commands ──
// Uses rusqlite directly for simpler API access.
//
// Connection model: a single shared Connection lives in a OnceCell<Mutex<_>>,
// initialized at app startup via `init(app)`. All commands borrow it through
// `with_db()`. This avoids opening a new file handle on every Tauri call,
// gives us PRAGMA WAL once, and serializes writes safely.

use crate::utils::db::{TokenUsage, PermissionRule, Skill, AgentMemory, ForkResult, generate_id, calculate_cost, INIT_SQL};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use once_cell::sync::OnceCell;
use tauri::{AppHandle, Manager};

// P11 audit (May 2026): r2d2 connection pool instead of a single
// Mutex<Connection>. SQLite WAL mode allows concurrent readers, but with a
// single mutex they all serialized. With a pool of N connections (each set
// to WAL via PRAGMA on open), readers proceed in parallel and a write
// blocks only the briefest critical section.
//
// Pool size 8 is generous for Lucy's workload — typical concurrent demand
// is 2-3 (UI dashboard refresh + LLM streaming + audit log writes). Headroom
// for bursts (e.g. multi-host scan + log persist + cost summary refresh).
pub(crate) type DbPool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
static POOL: OnceCell<DbPool> = OnceCell::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSummary {
    pub total_cost: f64,
    pub total_tokens: u32,
    pub request_count: u32,
    pub per_model: Vec<ModelCost>,
    pub period: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCost {
    pub model: String,
    pub cost: f64,
    pub tokens: u32,
    pub requests: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionAction {
    pub action: String,
    pub reason: String,
    pub rule_id: Option<String>,
}

/// Resolve the canonical Lucy DB path. Single file shared across metrics + indexer.
pub fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("app_data_dir: {}", e))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create_dir_all({}): {}", dir.display(), e))?;
    Ok(dir.join("lucy.db"))
}

/// Initialize the shared pool + schema (fail-fast variant). The Tauri boot path
/// now uses `init_resilient` (retry + corruption recovery), so this simple
/// variant is no longer called internally; kept as public API for tests and a
/// future lucy-core adoption of the shared DB layer.
#[allow(dead_code)]
pub fn init(app: &AppHandle) -> Result<(), String> {
    if POOL.get().is_some() {
        return Ok(());
    }
    init_with_path(db_path(app)?)
}

/// v1.7.238 — ¿el error de SQLite huele a corrupción del archivo (irrecuperable
/// por reintento)? Se usa para decidir cuarentena+recreación vs backoff.
pub(crate) fn looks_like_corruption(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("malformed")
        || e.contains("not a database")
        || e.contains("disk image is malformed")
        || e.contains("database corruption")
        || e.contains("file is encrypted")
}

/// v1.7.238 — Arranque RESILIENTE para operación DESATENDIDA (nadie mira a las
/// 3am). El `init` normal falla-rápido (lo usan tests + la costura egui); este
/// wrapper añade la capa "sin supervisor":
///   • REINTENTO con backoff para locks transitorios (un antivirus/backup
///     corporativo teniendo lucy.db abierto justo tras el reboot).
///   • RECUPERACIÓN de corrupción: si la DB está corrupta, se pone en cuarentena
///     (`lucy.db.corrupt-<ts>`) y se recrea fresca → Lucy vuelve DEGRADADA pero
///     VIVA, en vez de quedar zombie permanente donde TODO comando de DB
///     devuelve "not initialized" (que además fail-cierra toda ejecución).
///   • Todo queda en `lucy_app.log` (archivo, sin depender de la DB) para
///     reconstruir una noche fallida — antes iba a `eprintln!`, invisible en un
///     binario GUI de Windows release.
pub fn init_resilient(app: &AppHandle) -> Result<(), String> {
    if POOL.get().is_some() {
        return Ok(());
    }
    let path = db_path(app)?;
    init_path_resilient(path)
}

/// Núcleo de `init_resilient` sobre una ruta concreta (Tauri-free, testeable).
pub fn init_path_resilient(path: std::path::PathBuf) -> Result<(), String> {
    if POOL.get().is_some() {
        return Ok(());
    }
    const ATTEMPTS: u32 = 3;
    let mut last_err = String::new();
    for attempt in 1..=ATTEMPTS {
        match init_with_path(path.clone()) {
            Ok(()) => {
                if attempt > 1 {
                    crate::utils::logging::write_app_log(
                        "INFO",
                        &format!("metrics DB init OK en intento {}/{}", attempt, ATTEMPTS),
                    );
                }
                return Ok(());
            }
            Err(e) => {
                last_err = e.clone();
                if looks_like_corruption(&e) {
                    // Cuarentena del archivo corrupto + recreación fresca. Se
                    // limpian los sidecars WAL/SHM para que la DB nueva arranque
                    // limpia. Degradado (se pierde la memoria curada) pero VIVO;
                    // el backup automático permite restaurar después.
                    let ts = chrono::Utc::now().timestamp();
                    let quarantine = path.with_extension(format!("db.corrupt-{}", ts));
                    let renamed = std::fs::rename(&path, &quarantine).is_ok();
                    for ext in ["db-wal", "db-shm"] {
                        let _ = std::fs::remove_file(path.with_extension(ext));
                    }
                    crate::utils::logging::write_app_log(
                        "FATAL",
                        &format!(
                            "lucy.db CORRUPTA ({}). Cuarentena → {:?} ({}); recreando esquema fresco.",
                            e,
                            quarantine,
                            if renamed { "movida" } else { "MOVER FALLÓ" }
                        ),
                    );
                    return match init_with_path(path.clone()) {
                        Ok(()) => {
                            crate::utils::logging::write_app_log(
                                "WARNING",
                                "metrics DB recreada fresca tras corrupción — memoria/historial perdidos; restaura desde backup si aplica.",
                            );
                            Ok(())
                        }
                        Err(e2) => {
                            let msg = format!("recrear tras corrupción falló: {}", e2);
                            crate::utils::logging::write_app_log("FATAL", &msg);
                            Err(msg)
                        }
                    };
                }
                // Transitorio (lock): backoff creciente y reintento.
                crate::utils::logging::write_app_log(
                    "WARNING",
                    &format!(
                        "metrics DB init intento {}/{} falló: {} — reintentando",
                        attempt, ATTEMPTS, e
                    ),
                );
                if attempt < ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(1500 * attempt as u64));
                }
            }
        }
    }
    crate::utils::logging::write_app_log(
        "FATAL",
        &format!(
            "metrics DB init FALLÓ tras {} intentos: {} — Lucy corre SIN persistencia (comandos que usan la DB fallarán fail-closed).",
            ATTEMPTS, last_err
        ),
    );
    Err(last_err)
}

/// v1.7.236 — Tauri-free seam. Initialize the shared pool + schema from a plain
/// filesystem PATH. `init(app)` resolves the app-data dir then delegates here, so
/// a NON-Tauri host (e.g. the native egui shell in the WebView-migration
/// prototype) can reuse the EXACT same DB layer + every command function by
/// calling this directly — no Tauri AppHandle, no IPC. Behavior for the Tauri app
/// is unchanged: `init` still short-circuits before touching the path when the
/// pool already exists, and this fn re-checks the same guard.
pub fn init_with_path(path: std::path::PathBuf) -> Result<(), String> {
    if POOL.get().is_some() {
        return Ok(());
    }

    // Pool manager: every connection it hands out runs the same PRAGMAs.
    // WAL mode is per-DATABASE (set once persists), but synchronous,
    // temp_store and busy_timeout are per-connection and must be set on
    // each handle the pool spawns.
    //
    // v1.7.82 — performance tuning. Adds three high-impact PRAGMAs that
    // address Lucy's real workload (15-20 SELECT queries per LLM turn
    // against agent_memories / memory_core / chip_click_log):
    //
    //   • cache_size = -64000  → 64 MB page cache per connection.
    //     Default is 2000 pages (~8 MB) which is undersized for a
    //     ~200 MB DB with hot tables. Bumping to 64 MB lets the working
    //     set live in memory; observed 2-3× speedup on memory recall
    //     queries against the v1.4+ chip_click_log.
    //
    //   • mmap_size = 268435456 → 256 MB memory-mapped I/O.
    //     Lets SQLite read pages directly from the OS page cache without
    //     the syscall round-trip. Especially helpful on WSL2 / VMs where
    //     read() syscalls are expensive. Zero downside on Windows/Linux
    //     native — the kernel decides whether to back it.
    //
    //   • wal_autocheckpoint = 1000 → checkpoint when WAL hits 1000 pages
    //     (~4 MB at 4 KB pages) instead of the default 1000.
    //     Default is fine; setting explicitly so the value is auditable.
    //
    // The existing tuning (WAL + NORMAL + temp_store=MEMORY + 5 s busy)
    // stays unchanged. These additions are additive and cannot regress
    // correctness — they only adjust how aggressively SQLite caches.
    let manager = r2d2_sqlite::SqliteConnectionManager::file(&path)
        .with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode=WAL;\
                 PRAGMA synchronous=NORMAL;\
                 PRAGMA temp_store=MEMORY;\
                 PRAGMA busy_timeout=5000;\
                 PRAGMA cache_size=-64000;\
                 PRAGMA mmap_size=268435456;\
                 PRAGMA wal_autocheckpoint=1000;\
                 PRAGMA foreign_keys=ON;"
            )
        });

    let pool = r2d2::Pool::builder()
        .max_size(8)
        .min_idle(Some(1))
        .build(manager)
        .map_err(|e| format!("Failed to build SQLite pool at {}: {}", path.display(), e))?;

    // Initial schema setup — borrow one connection from the pool.
    let conn = pool.get()
        .map_err(|e| format!("Failed to acquire init connection: {}", e))?;

    run_schema_init(&conn)?;

    // Release the init connection back to the pool, then publish.
    drop(conn);
    // v1.8 — hand the SAME pool to lucy-core, the Tauri-free half that the
    // native shell also links. Not a second pool: one file, one set of PRAGMAs,
    // one connection budget. `init_with_pool` is idempotent, and a failure here
    // must not stop the app — it only means the shared-heart functions fall
    // back to being unavailable, not that the DB is broken.
    let _ = lucy_core::init_with_pool(pool.clone());
    POOL.set(pool)
        .map_err(|_| "DB pool already initialized".to_string())?;
    Ok(())
}

/// v1.7.234 — Full schema init (legacy-trigger drop + INIT_SQL + migrations
/// array + grounding) extracted VERBATIM from the boot path so tests can run
/// the EXACT real sequence against a fresh connection.
pub(crate) fn run_schema_init(conn: &rusqlite::Connection) -> Result<(), String> {
    // Migration: drop legacy broken FTS5 triggers from older DB versions.
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS agent_memories_ad;\
         DROP TRIGGER IF EXISTS agent_memories_au;",
    ).map_err(|e| format!("Failed to drop legacy triggers: {}", e))?;

    // Schema is idempotent (CREATE IF NOT EXISTS).
    conn.execute_batch(INIT_SQL)
        .map_err(|e| format!("Failed to initialize schema: {}", e))?;

    // ── Mem0-inspired additive migrations (May 2026) ──
    // ALTER TABLE ADD COLUMN isn't idempotent before SQLite 3.35 — we just
    // swallow the "duplicate column" error to keep startup happy on existing
    // databases. New installs run these as no-ops because INIT_SQL already
    // documents the intent.
    let migrations = [
        "ALTER TABLE agent_memories ADD COLUMN last_accessed_at INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE agent_memories ADD COLUMN access_count     INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE agent_memories ADD COLUMN superseded_by    INTEGER NULL",
        "CREATE INDEX IF NOT EXISTS idx_agent_memories_superseded \
         ON agent_memories(superseded_by) WHERE superseded_by IS NULL",
        // agentmemory-inspired auto-forget (Tier 1 #1): expires_at is a
        // Unix epoch seconds value. 0 means "never expires" — the default,
        // preserves the prior behaviour. auto_forget_run() deletes rows
        // where expires_at > 0 AND expires_at < now().
        "ALTER TABLE agent_memories ADD COLUMN expires_at INTEGER NOT NULL DEFAULT 0",
        "CREATE INDEX IF NOT EXISTS idx_agent_memories_expires \
         ON agent_memories(expires_at) WHERE expires_at > 0",
        // ── v1.7.236 R5 — memorias fijadas (pin del operador) ─────────────
        // pinned=1 → la memoria se inyecta SIEMPRE al contexto (bloque
        // MEMORIAS FIJADAS), sin depender del score semántico. Pensado para
        // instrucciones operativas en uso desatendido ("si preguntan por X,
        // el procedimiento es Y"). Cap de inyección en get_pinned_memories.
        "ALTER TABLE agent_memories ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
        // ── v1.7.233 — idempotent PDF re-ingest ─────────────────────────
        // SHA-256 hex of the extracted text; pdf_ingest refuses to create a
        // second full chunk set for a document already ingested.
        "ALTER TABLE pdf_documents ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''",
        // v1.7.233 M2 — estado de la síntesis jerárquica por documento.
        "ALTER TABLE pdf_documents ADD COLUMN synth_status TEXT NOT NULL DEFAULT ''",
        // ── v1.8 — web ingestion shares this table ───────────────────────
        // `pdf_documents` is the ingested-DOCUMENTS table; the name is
        // historical. Web pages live here too so they inherit the documents
        // list, the embedded/total counter, deletion, the coverage diagnostic
        // and the re-embed repair instead of getting a second copy of each.
        // `kind` is what keeps that honest — DEFAULT 'pdf' so every existing
        // row is already correct without a backfill.
        "ALTER TABLE pdf_documents ADD COLUMN kind TEXT NOT NULL DEFAULT 'pdf'",
        "CREATE INDEX IF NOT EXISTS idx_pdf_docs_hash \
         ON pdf_documents(content_hash) WHERE content_hash != ''",
        // ── v1.7.104 Sprint-4 perf — D3 sparkline index ─────────────────
        // `recent_model_latencies` (v1.7.99) selects with
        // `WHERE json_extract(metadata,'$.model') IS NOT NULL ORDER BY
        // timestamp DESC`. With no index on the json path, every poll
        // (every 30s while the UI is open) does a json-parse of `metadata`
        // for each candidate row. As task_events grows past ~5k rows the
        // poll measurably stalls.
        //
        // A SQLite **expression index** on the json_extract result lets
        // the planner skip the parse for the predicate AND lets it use
        // an index-only scan when ordering on timestamp. Filtered
        // (partial) so we don't index rows missing both fields — most
        // task_events carry no model.
        "CREATE INDEX IF NOT EXISTS idx_task_events_model_ts \
         ON task_events(timestamp DESC) \
         WHERE elapsed_ms IS NOT NULL \
           AND elapsed_ms > 0 \
           AND json_extract(metadata, '$.model') IS NOT NULL",
        // ── v1.7.101 — Sprint #1 B1 fix ───────────────────────────────────
        // Tier-A `crystal_promo` housekeeping loop was shipping an INSERT
        // against `agent_crystals(source_id, summary, content, tags, …)` —
        // but the live schema only carries `narrative, key_outcomes,
        // files_affected, lessons, source_chars`. The prepare() failed
        // every 6h since v1.7.95 and the loop silently returned Ok(0).
        //
        // Adding `source_id` (nullable, indexed unique) closes the
        // idempotency gap: the promoter's WHERE id NOT IN (SELECT
        // source_id …) can now actually find promoted rows, and INSERT
        // OR IGNORE on a unique source_id prevents double-promotion.
        // Existing crystals get source_id=NULL — they're not eligible
        // for "already promoted" lookups but otherwise unaffected.
        // ── Tier A #1 — Hierarchical forks + cost ledger ─────────────────
        // parent_task_id: the task_id of the fork that spawned THIS one.
        // Empty string = top-level fork (no parent). Enables tree rendering
        // in ForksMonitorPanel and lineage analysis ("who spawned who").
        //
        // Cost fields: each ask_lucy call inside a fork reports its token
        // counts and we sum them here. cost_usd is denormalized so the UI
        // can sort/filter by spend without recomputing from a price table.
        "ALTER TABLE fork_results ADD COLUMN parent_task_id TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE fork_results ADD COLUMN tokens_in      INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE fork_results ADD COLUMN tokens_out     INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE fork_results ADD COLUMN cost_usd       REAL    NOT NULL DEFAULT 0.0",
        "CREATE INDEX IF NOT EXISTS idx_fork_parent ON fork_results(parent_task_id)",
        // agentmemory-inspired crystals (Tier 2 #4). Each row is the LLM-
        // distilled digest of a completed agent session: 1-2 sentence
        // narrative + key outcomes + files affected + lessons learned.
        // Surfaced via list_crystals / get_crystal commands.
        "CREATE TABLE IF NOT EXISTS agent_crystals (\
            id              INTEGER PRIMARY KEY AUTOINCREMENT,\
            session_id      TEXT    NOT NULL DEFAULT '',\
            project         TEXT    NOT NULL DEFAULT '',\
            narrative       TEXT    NOT NULL,\
            key_outcomes    TEXT    NOT NULL DEFAULT '[]',\
            files_affected  TEXT    NOT NULL DEFAULT '[]',\
            lessons         TEXT    NOT NULL DEFAULT '[]',\
            source_chars    INTEGER NOT NULL DEFAULT 0,\
            created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now'))\
        )",
        // v1.7.234 fresh-install fix: este ALTER (v1.7.101) iba ANTES del
        // CREATE TABLE de arriba -> en BD nueva fallaba con "no such table"
        // (no tolerado) y abortaba TODO el init ("Metrics DB not initialized").
        "ALTER TABLE agent_crystals ADD COLUMN source_id INTEGER NULL",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_crystals_source_id \
         ON agent_crystals(source_id) WHERE source_id IS NOT NULL",
        "CREATE INDEX IF NOT EXISTS idx_agent_crystals_session \
         ON agent_crystals(session_id, created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_agent_crystals_created \
         ON agent_crystals(created_at DESC)",
        // agentmemory-inspired insights (Tier 3 #8). Each row is a higher-
        // level META-observation derived from a cluster of memories. Unlike
        // crystals (one-shot session digests) and unlike consolidated
        // memories (which supersede their sources), insights are RECURRENT:
        // every reflect_run looks for matching patterns and either creates
        // a new insight or REINFORCES an existing one — confidence grows
        // toward 1.0 each time the pattern re-appears, and reinforcements
        // counts the recurrences.
        //
        // fingerprint is a stable hash over the lowercased, whitespace-
        // collapsed content — same wording across runs deduplicates into
        // reinforcement instead of creating noise.
        "CREATE TABLE IF NOT EXISTS agent_insights (\
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,\
            content             TEXT    NOT NULL,\
            fingerprint         TEXT    NOT NULL UNIQUE,\
            confidence          REAL    NOT NULL DEFAULT 0.5,\
            reinforcements      INTEGER NOT NULL DEFAULT 1,\
            concepts            TEXT    NOT NULL DEFAULT '[]',\
            source_count        INTEGER NOT NULL DEFAULT 0,\
            last_reinforced_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),\
            created_at          INTEGER NOT NULL DEFAULT (strftime('%s','now')),\
            updated_at          INTEGER NOT NULL DEFAULT (strftime('%s','now'))\
        )",
        "CREATE INDEX IF NOT EXISTS idx_agent_insights_confidence \
         ON agent_insights(confidence DESC, reinforcements DESC)",
        "CREATE INDEX IF NOT EXISTS idx_agent_insights_updated \
         ON agent_insights(updated_at DESC)",
        // agentmemory-inspired graph (Tier 3 #9). Each row is an undirected
        // edge between two memories — we materialise it as TWO directed
        // rows (a→b and b→a) so a simple SELECT on source_id is enough for
        // BFS without UNION trickery.
        //
        // edge_type captures the WHY:
        //   • 'shares_concept' — overlapping tags (weight = Jaccard sim)
        //   • 'shares_file'    — overlapping file refs (weight = capped)
        //   • 'same_session'   — both came from one agent session
        //
        // Edges are built in batch by graph_rebuild_edges_run (idempotent)
        // rather than per-save to keep the hot save path fast.
        "CREATE TABLE IF NOT EXISTS agent_memory_edges (\
            source_id    INTEGER NOT NULL,\
            target_id    INTEGER NOT NULL,\
            edge_type    TEXT    NOT NULL,\
            weight       REAL    NOT NULL DEFAULT 1.0,\
            created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now')),\
            PRIMARY KEY (source_id, target_id, edge_type)\
        )",
        "CREATE INDEX IF NOT EXISTS idx_memory_edges_source \
         ON agent_memory_edges(source_id, weight DESC)",
        "CREATE INDEX IF NOT EXISTS idx_memory_edges_target \
         ON agent_memory_edges(target_id)",
        // Persistent session summaries (the user's "Lucy pierde el hilo en
        // sesiones grandes" complaint). Each row is a structured YAML/text
        // summary of a tab's older messages, regenerated by the frontend's
        // regenerateSmartDigest() function every N turns. Persisted here so
        // it survives app restarts — closing Lucy mid-session and reopening
        // doesn't blow away the compacted context.
        //
        // tab_id is the Svelte-side tab identifier (UUID-ish string).
        // anchor_msg_index is the position in tab.messages BEYOND which
        // summarization applies — messages at index > anchor are still
        // verbatim; index <= anchor is collapsed into `summary`.
        "CREATE TABLE IF NOT EXISTS agent_session_summaries (\
            id                INTEGER PRIMARY KEY AUTOINCREMENT,\
            tab_id            TEXT    NOT NULL,\
            anchor_msg_index  INTEGER NOT NULL DEFAULT 0,\
            summary           TEXT    NOT NULL,\
            model_used        TEXT    NOT NULL DEFAULT '',\
            tokens_in         INTEGER NOT NULL DEFAULT 0,\
            tokens_out        INTEGER NOT NULL DEFAULT 0,\
            created_at        INTEGER NOT NULL DEFAULT (strftime('%s','now')),\
            updated_at        INTEGER NOT NULL DEFAULT (strftime('%s','now'))\
        )",
        // ONE summary per tab (replace-on-write semantics handled in the
        // command). Index on (tab_id, updated_at DESC) so the latest summary
        // surfaces instantly when the tab reopens.
        "CREATE INDEX IF NOT EXISTS idx_session_summaries_tab \
         ON agent_session_summaries(tab_id, updated_at DESC)",
    ];
    for stmt in &migrations {
        if let Err(e) = conn.execute(stmt, []) {
            let msg = e.to_string();
            // Tolerate idempotent re-runs
            if !msg.contains("duplicate column") && !msg.contains("already exists") {
                return Err(format!("Migration failed [{}]: {}", stmt, msg));
            }
        }
    }

    // v1.6.0 — Kappa-Graph-style grounding migrations (ADR-044).
    // Runs after the main migration block so it can add `confidence`
    // columns to agent_memories and memory_core, plus the
    // memory_evidence and memory_instances tables. Failure here is
    // fatal: grounding columns are referenced by query-time scoring.
    super::grounding::migrate(conn)
        .map_err(|e| format!("grounding migration failed: {}", e))?;
    Ok(())
}

/// Borrow a pooled connection. Returns an error if `init()` was not called
/// or the pool is exhausted (shouldn't happen at max_size=8).
fn with_db<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&rusqlite::Connection) -> Result<R, String>,
{
    let pool = POOL.get().ok_or_else(|| "Metrics DB not initialized".to_string())?;
    let conn = pool.get().map_err(|e| format!("DB pool exhausted: {}", e))?;
    f(&conn)
}

/// Crate-visible alias so sibling modules (e.g. incident.rs) can reuse the
/// same pool without re-opening the file.
pub(crate) fn shared_db<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&rusqlite::Connection) -> Result<R, String>,
{
    with_db(f)
}

/// Back-compat shim: frontend may still invoke this. Schema is already created
/// at startup, so this is a no-op success.
#[tauri::command]
pub async fn init_metrics_db() -> Result<(), String> {
    if POOL.get().is_some() { Ok(()) } else { Err("DB not initialized at startup".to_string()) }
}

// ── Test-only DB initialization ─────────────────────────────────────────────
// Spins up an in-memory SQLite pool with the full schema so tests that
// transitively hit `with_db()` (via execute_powershell → check_permission,
// for example) can run without needing a Tauri AppHandle.
//
// Idempotent: a second call after the pool is set is a no-op. Each test
// process gets one shared pool — that's fine because:
//   1. SQLite in-memory shared cache is process-local.
//   2. permission_rules starts empty → check_permission returns "allow" for
//      everything, which is exactly what we want for exec-contract tests.
//
// Gated behind `#[cfg(test)]` so it never ships in release binaries.
#[cfg(test)]
pub fn init_in_memory_for_tests() -> Result<(), String> {
    if POOL.get().is_some() {
        return Ok(());
    }
    // `SqliteConnectionManager::memory()` opens a fresh `:memory:` DB per
    // connection by default. To share state across pool handles we'd need
    // `file::memory:?cache=shared` — but for our tests, ONE connection in
    // the pool is enough (max_size=1). check_permission borrows briefly,
    // returns, and the same handle services the next call.
    let manager = r2d2_sqlite::SqliteConnectionManager::memory()
        .with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode=MEMORY;\
                 PRAGMA synchronous=OFF;\
                 PRAGMA foreign_keys=ON;"
            )
        });
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .min_idle(Some(1))
        .build(manager)
        .map_err(|e| format!("test pool build: {}", e))?;
    let conn = pool.get().map_err(|e| format!("test pool get: {}", e))?;
    conn.execute_batch(INIT_SQL)
        .map_err(|e| format!("test schema init: {}", e))?;
    drop(conn);
    // Race tolerance: cargo test runs tests in parallel. Two of them may
    // both see `POOL.get().is_some() == false` and both build a pool.
    // Whichever calls .set() first wins — the loser silently drops its
    // pool. Either pool is functionally equivalent (both back an empty
    // permission_rules table), so we don't care which one survives.
    let _ = POOL.set(pool);
    Ok(())
}

/// Internal function to log token usage (called from both Tauri command and AI instrumentation)
pub async fn log_usage_internal(
    model: &str,
    input_tokens: u32,
    output_tokens: u32,
    request_type: &str,
    user: &str,
) -> Result<(), String> {
    let id = generate_id();
    let task_id = format!("task_{}", chrono::Local::now().timestamp_millis());
    let timestamp = chrono::Local::now().to_rfc3339();
    let total_cost = calculate_cost(model, input_tokens, output_tokens);
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let total_tokens = input_tokens + output_tokens;

    with_db(|conn| {
        // v1.7.102 Sprint-2 H8: wrap the two writes in a single transaction.
        // Previously they were independent execute() calls — a crash (or
        // process kill) between them left token_usage carrying a row that
        // daily_summary didn't reflect, and the cost dashboard diverged
        // from the row log without any way to reconcile. Atomic now.
        let tx = conn.unchecked_transaction()
            .map_err(|e| format!("log_usage tx open: {}", e))?;
        tx.execute(
            "INSERT INTO token_usage (id, task_id, timestamp, model, input_tokens, output_tokens, total_cost, user, request_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![&id, &task_id, &timestamp, model, input_tokens, output_tokens, total_cost, user, request_type],
        ).map_err(|e| format!("Failed to insert token_usage: {}", e))?;

        // Atomic UPSERT — replaces the prior check-then-insert race.
        tx.execute(
            "INSERT INTO daily_summary (date, model, total_tokens, total_cost, request_count)
             VALUES (?1, ?2, ?3, ?4, 1)
             ON CONFLICT(date, model) DO UPDATE SET
                 total_tokens = total_tokens + excluded.total_tokens,
                 total_cost = total_cost + excluded.total_cost,
                 request_count = request_count + 1",
            rusqlite::params![&date, model, total_tokens, total_cost],
        ).map_err(|e| format!("Failed to upsert daily_summary: {}", e))?;
        tx.commit().map_err(|e| format!("log_usage tx commit: {}", e))?;
        Ok(())
    })
}

/// Log token usage and calculate cost (Tauri command wrapper)
#[tauri::command]
pub async fn log_token_usage(
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    request_type: String,
    user: String,
) -> Result<String, String> {
    log_usage_internal(&model, input_tokens, output_tokens, &request_type, &user).await?;
    Ok(generate_id())
}

// ── v1.7.31 — Cost by day (for the StatusBar sparkline) ─────────────────
//
// Returns an array of (date, total_cost_usd) entries for the last N days.
// Days with no spend are explicitly emitted as `0.0` so the frontend
// sparkline doesn't have to do gap-filling. Keeps the chart visually
// continuous and removes a class of "sparkline collapses to a single
// dot" bugs when usage is sporadic.

#[derive(serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../src/lib/types/")]
pub struct CostDayPoint {
    pub date: String,   // ISO YYYY-MM-DD
    pub cost: f64,      // USD, rounded server-side to 6 decimals
}

#[tauri::command]
pub async fn get_cost_by_day(days: Option<u32>) -> Result<Vec<CostDayPoint>, String> {
    let days = days.unwrap_or(7).clamp(1, 90) as i64;
    // Build the requested date range INCLUSIVE of today, oldest first.
    // Generating client-side dates AND sql-grouping means even when
    // `daily_summary` has no rows for some dates we still emit zeros.
    let today = chrono::Local::now().date_naive();
    let mut points: Vec<CostDayPoint> = (0..days).rev().map(|offset| {
        let d = today - chrono::Duration::days(offset);
        CostDayPoint { date: d.format("%Y-%m-%d").to_string(), cost: 0.0 }
    }).collect();

    with_db(|conn| {
        // Pull all rows in the window in a single sweep, then merge into
        // the pre-allocated vec. Avoids per-day round-trips.
        let oldest = today - chrono::Duration::days(days - 1);
        let oldest_str = oldest.format("%Y-%m-%d").to_string();
        let mut stmt = conn.prepare(
            "SELECT date, COALESCE(SUM(total_cost), 0) FROM daily_summary
             WHERE date >= ?1 GROUP BY date"
        ).map_err(|e| format!("prepare failed: {}", e))?;
        let rows = stmt.query_map(rusqlite::params![oldest_str], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        }).map_err(|e| format!("query failed: {}", e))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|e| format!("collect failed: {}", e))?;
        for (date, cost) in rows {
            if let Some(p) = points.iter_mut().find(|p| p.date == date) {
                p.cost = (cost * 1_000_000.0).round() / 1_000_000.0;
            }
        }
        Ok(())
    })?;
    Ok(points)
}

/// Get cost summary for a period (day, month, all)
#[tauri::command]
pub async fn get_cost_summary(period: String) -> Result<CostSummary, String> {
    // Bind the date filter as a parameter rather than format!()-ing it
    // into the SQL — same query plan, no possibility of injection drift
    // if this code grows.
    let (where_sql, bind_value): (&str, Option<String>) = match period.as_str() {
        "day"   => ("WHERE date = ?1",       Some(chrono::Local::now().format("%Y-%m-%d").to_string())),
        "month" => ("WHERE date LIKE ?1",    Some(format!("{}%", chrono::Local::now().format("%Y-%m")))),
        _       => ("",                      None),
    };

    with_db(|conn| {
        let totals_sql = format!(
            "SELECT COALESCE(SUM(total_cost), 0), COALESCE(SUM(total_tokens), 0), COALESCE(SUM(request_count), 0) FROM daily_summary {}",
            where_sql
        );
        let (total_cost, total_tokens, request_count) = if let Some(ref v) = bind_value {
            conn.query_row(&totals_sql, rusqlite::params![v], |row| {
                Ok((row.get::<_, f64>(0)?, row.get::<_, u32>(1)?, row.get::<_, u32>(2)?))
            })
        } else {
            conn.query_row(&totals_sql, [], |row| {
                Ok((row.get::<_, f64>(0)?, row.get::<_, u32>(1)?, row.get::<_, u32>(2)?))
            })
        }.map_err(|e| format!("Failed to query totals: {}", e))?;

        let model_sql = format!(
            "SELECT model, COALESCE(SUM(total_cost), 0), COALESCE(SUM(total_tokens), 0), COALESCE(SUM(request_count), 0)
             FROM daily_summary {} GROUP BY model ORDER BY total_cost DESC",
            where_sql
        );
        let mut stmt = conn.prepare(&model_sql)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<ModelCost> {
            Ok(ModelCost {
                model: row.get(0)?,
                cost: row.get(1)?,
                tokens: row.get(2)?,
                requests: row.get(3)?,
            })
        };
        let per_model: Vec<ModelCost> = if let Some(ref v) = bind_value {
            stmt.query_map(rusqlite::params![v], map_row)
        } else {
            stmt.query_map([], map_row)
        }.map_err(|e| format!("Failed to query models: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect results: {}", e))?;

        Ok(CostSummary {
            total_cost,
            total_tokens,
            request_count,
            per_model,
            period: period.clone(),
        })
    })
}

// ── Cost history reset ────────────────────────────────────────────────────
//
// Lets the user wipe accumulated cost / token totals from the Cost Dashboard
// without touching anything else (memories, skills, hosts, conversations).
// This is the "fresh start" the user asked for so they don't have to
// uninstall or hand-edit the DB.
//
// Scopes:
//   • "day"   — only today's rows (LIKE 'YYYY-MM-DD%' for token_usage)
//   • "month" — current calendar month
//   • "all"   — every row in token_usage + daily_summary
//
// SAFETY:
//   • Affects ONLY token_usage and daily_summary tables.
//   • Audit trail is NOT touched — compliance / forensics stay intact.
//   • Wrapped in a single transaction so the two tables stay consistent.
//   • Returns the (deleted_rows, deleted_summary_rows) tuple so the UI
//     can show the user exactly how much was wiped.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CostResetResult {
    pub scope: String,
    pub deleted_token_rows: u64,
    pub deleted_summary_rows: u64,
}

#[tauri::command]
pub async fn reset_cost_history(scope: String) -> Result<CostResetResult, String> {
    let scope_norm = scope.to_lowercase();
    if !matches!(scope_norm.as_str(), "day" | "month" | "all") {
        return Err(format!("Invalid scope '{}'. Expected: day | month | all", scope));
    }

    with_db(|conn| {
        let tx = conn.unchecked_transaction()
            .map_err(|e| format!("begin tx: {}", e))?;

        let (deleted_token_rows, deleted_summary_rows) = match scope_norm.as_str() {
            "day" => {
                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                // token_usage.timestamp is ISO 8601 (e.g. "2026-05-20T14:33:01");
                // a LIKE 'YYYY-MM-DD%' match captures every row from today.
                let n_tu = tx.execute(
                    "DELETE FROM token_usage WHERE timestamp LIKE ?1",
                    rusqlite::params![format!("{}%", today)],
                ).map_err(|e| format!("delete token_usage (day): {}", e))? as u64;
                let n_ds = tx.execute(
                    "DELETE FROM daily_summary WHERE date = ?1",
                    rusqlite::params![&today],
                ).map_err(|e| format!("delete daily_summary (day): {}", e))? as u64;
                (n_tu, n_ds)
            }
            "month" => {
                let prefix = chrono::Local::now().format("%Y-%m").to_string();
                let n_tu = tx.execute(
                    "DELETE FROM token_usage WHERE timestamp LIKE ?1",
                    rusqlite::params![format!("{}%", prefix)],
                ).map_err(|e| format!("delete token_usage (month): {}", e))? as u64;
                let n_ds = tx.execute(
                    "DELETE FROM daily_summary WHERE date LIKE ?1",
                    rusqlite::params![format!("{}%", prefix)],
                ).map_err(|e| format!("delete daily_summary (month): {}", e))? as u64;
                (n_tu, n_ds)
            }
            _ /* "all" */ => {
                let n_tu = tx.execute("DELETE FROM token_usage", [])
                    .map_err(|e| format!("delete token_usage (all): {}", e))? as u64;
                let n_ds = tx.execute("DELETE FROM daily_summary", [])
                    .map_err(|e| format!("delete daily_summary (all): {}", e))? as u64;
                (n_tu, n_ds)
            }
        };

        tx.commit().map_err(|e| format!("commit reset: {}", e))?;

        Ok(CostResetResult {
            scope: scope_norm,
            deleted_token_rows,
            deleted_summary_rows,
        })
    })
}

/// Get token usage history (last N records)
#[tauri::command]
pub async fn get_token_history(limit: u32) -> Result<Vec<TokenUsage>, String> {
    with_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, task_id, timestamp, model, input_tokens, output_tokens, total_cost, user, request_type
             FROM token_usage ORDER BY timestamp DESC LIMIT ?1"
        ).map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            Ok(TokenUsage {
                id: row.get(0)?,
                task_id: row.get(1)?,
                timestamp: row.get(2)?,
                model: row.get(3)?,
                input_tokens: row.get(4)?,
                output_tokens: row.get(5)?,
                total_cost: row.get(6)?,
                user: row.get(7)?,
                request_type: row.get(8)?,
            })
        }).map_err(|e| format!("Failed to query history: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect results: {}", e))?;

        Ok(rows)
    })
}

/// Check if command/path matches permission rules
#[tauri::command]
pub async fn check_permission(
    cmd: String,
    rule_type: String,
) -> Result<PermissionAction, String> {
    with_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, pattern, action FROM permission_rules
             WHERE applies_to = ?1 AND enabled = 1 ORDER BY priority ASC"
        ).map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rules = stmt.query_map(rusqlite::params![&rule_type], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        }).map_err(|e| format!("Failed to query rules: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect results: {}", e))?;

        // Patterns are now validated at save_permission_rule time, so any failure
        // here is corruption — fail closed (block) rather than silently allow.
        for (rule_id, pattern, action) in rules {
            let re = regex::Regex::new(&pattern)
                .map_err(|e| format!("Corrupt rule {}: invalid regex {:?}: {}", rule_id, pattern, e))?;
            if re.is_match(&cmd) {
                return Ok(PermissionAction {
                    action: action.clone(),
                    reason: format!("Matched rule: {}", pattern),
                    rule_id: Some(rule_id),
                });
            }
        }

        Ok(PermissionAction {
            action: "allow".to_string(),
            reason: "No matching rules".to_string(),
            rule_id: None,
        })
    })
}

/// Save a permission rule
#[tauri::command]
pub async fn save_permission_rule(rule: PermissionRule) -> Result<String, String> {
    // Validate the regex up-front. A bad pattern used to be silently dropped
    // by check_permission, which could turn a "block" rule into "allow".
    regex::Regex::new(&rule.pattern)
        .map_err(|e| format!("Invalid regex pattern {:?}: {}", rule.pattern, e))?;

    // Validate action enum.
    match rule.action.as_str() {
        "allow" | "block" | "ask" => {},
        other => return Err(format!("Invalid action {:?}: must be allow|block|ask", other)),
    }

    let id = if rule.id.is_empty() { generate_id() } else { rule.id.clone() };
    let created_at = if rule.created_at.is_empty() {
        chrono::Local::now().to_rfc3339()
    } else {
        rule.created_at.clone()
    };

    with_db(|conn| {
        conn.execute(
            "INSERT OR REPLACE INTO permission_rules (id, pattern, action, description, priority, applies_to, enabled, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![&id, &rule.pattern, &rule.action, &rule.description,
                             rule.priority, &rule.applies_to, rule.enabled as u8, &created_at],
        ).map_err(|e| format!("Failed to save rule: {}", e))?;
        Ok(id.clone())
    })
}

/// List permission rules
#[tauri::command]
pub async fn list_permission_rules(applies_to: Option<String>) -> Result<Vec<PermissionRule>, String> {
    with_db(|conn| {
        let query = if applies_to.is_some() {
            "SELECT id, pattern, action, description, priority, applies_to, enabled, created_at
             FROM permission_rules WHERE applies_to = ?1 ORDER BY priority ASC"
        } else {
            "SELECT id, pattern, action, description, priority, applies_to, enabled, created_at
             FROM permission_rules ORDER BY priority ASC"
        };

        let mut stmt = conn.prepare(query)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows: Vec<PermissionRule> = if let Some(ref filter) = applies_to {
            stmt.query_map(rusqlite::params![filter], parse_permission_rule)
        } else {
            stmt.query_map([], parse_permission_rule)
        }.map_err(|e| format!("Failed to query rules: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect results: {}", e))?;

        Ok(rows)
    })
}

/// Delete permission rule
#[tauri::command]
pub async fn delete_permission_rule(rule_id: String) -> Result<(), String> {
    with_db(|conn| {
        conn.execute(
            "DELETE FROM permission_rules WHERE id = ?1",
            rusqlite::params![&rule_id],
        ).map_err(|e| format!("Failed to delete rule: {}", e))?;
        Ok(())
    })
}

// ── Skills commands ──

/// Save a skill
#[tauri::command]
pub async fn save_skill(skill: Skill) -> Result<String, String> {
    let id = if skill.id.is_empty() { generate_id() } else { skill.id.clone() };
    let now = chrono::Local::now().to_rfc3339();

    with_db(|conn| {
        conn.execute(
            "INSERT OR REPLACE INTO skills (id, name, category, triggers, script, description, parameters, created_at, updated_at, usage_count, enabled, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![&id, &skill.name, &skill.category, &skill.triggers, &skill.script,
                             &skill.description, &skill.parameters, &skill.created_at, &now,
                             skill.usage_count, skill.enabled as u8, &skill.tags],
        ).map_err(|e| format!("Failed to save skill: {}", e))?;
        Ok(id.clone())
    })
}

/// List skills
#[tauri::command]
pub async fn list_skills(category: Option<String>) -> Result<Vec<Skill>, String> {
    with_db(|conn| {
        let query = if category.is_some() {
            "SELECT id, name, category, triggers, script, description, parameters, created_at, updated_at, usage_count, last_executed, enabled, tags
             FROM skills WHERE category = ?1 ORDER BY usage_count DESC"
        } else {
            "SELECT id, name, category, triggers, script, description, parameters, created_at, updated_at, usage_count, last_executed, enabled, tags
             FROM skills ORDER BY usage_count DESC"
        };

        let mut stmt = conn.prepare(query)
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows: Vec<Skill> = if let Some(ref cat) = category {
            stmt.query_map(rusqlite::params![cat], parse_skill)
        } else {
            stmt.query_map([], parse_skill)
        }.map_err(|e| format!("Failed to query skills: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect results: {}", e))?;

        Ok(rows)
    })
}

/// Delete skill
#[tauri::command]
pub async fn delete_skill(skill_id: String) -> Result<(), String> {
    with_db(|conn| {
        conn.execute(
            "DELETE FROM skills WHERE id = ?1",
            rusqlite::params![&skill_id],
        ).map_err(|e| format!("Failed to delete skill: {}", e))?;
        Ok(())
    })
}

/// Increment skill usage
#[tauri::command]
pub async fn increment_skill_usage(skill_id: String) -> Result<(), String> {
    let now = chrono::Local::now().to_rfc3339();
    with_db(|conn| {
        conn.execute(
            "UPDATE skills SET usage_count = usage_count + 1, last_executed = ?1 WHERE id = ?2",
            rusqlite::params![&now, &skill_id],
        ).map_err(|e| format!("Failed to update skill: {}", e))?;
        Ok(())
    })
}

fn parse_permission_rule(row: &rusqlite::Row) -> rusqlite::Result<PermissionRule> {
    Ok(PermissionRule {
        id: row.get(0)?,
        pattern: row.get(1)?,
        action: row.get(2)?,
        description: row.get(3)?,
        priority: row.get(4)?,
        applies_to: row.get(5)?,
        enabled: row.get::<_, u8>(6)? != 0,
        created_at: row.get(7)?,
    })
}

fn parse_skill(row: &rusqlite::Row) -> rusqlite::Result<Skill> {
    Ok(Skill {
        id: row.get(0)?,
        name: row.get(1)?,
        category: row.get(2)?,
        triggers: row.get(3)?,
        script: row.get(4)?,
        description: row.get(5)?,
        parameters: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        usage_count: row.get(9)?,
        last_executed: row.get(10).ok(),
        enabled: row.get::<_, u8>(11)? != 0,
        tags: row.get(12)?,
    })
}

// ══════════════════════════════════════════════════════════════════════════════
// AGENT MEMORY — persistent cross-session knowledge store
// ══════════════════════════════════════════════════════════════════════════════

/// Result of `save_agent_memory` — communicates whether we actually
/// inserted a new row or merged into an existing duplicate.
#[derive(Debug, Serialize, Deserialize)]
pub struct SaveMemoryResult {
    /// Numeric id of the affected row (either newly inserted or existing dup).
    pub id: i64,
    /// `"inserted"` for a fresh row, `"duplicate"` when the incoming memory
    /// matched an existing one (>=85% FTS similarity) and we kept the older
    /// row but bumped its `access_count` + refreshed `last_accessed_at`.
    pub action: String,
    /// Human-readable reason — useful for the agent to surface in chat.
    pub reason: String,
}

/// Save a memory discovered during an agent task — with **two-stage
/// automatic deduplication** (Mem0-inspired, May 2026).
///
/// **Stage 1 — FTS5 bm25 probe** (cheap, ~1ms). Catches duplicates where
/// the new content uses similar wording to an existing one. Threshold
/// `bm25 < -8.0` catches ~90% of true dups with our content shape.
///
/// **Stage 2 — Embedding cosine similarity** (Mem0 deep, May 2026).
/// Runs ONLY if stage 1 didn't catch a dup AND Ollama embeddings are
/// available. Embeds the new content + searches `vec_index` for any
/// `entity_type=memory` row with cosine ≥ 0.92. Catches **paraphrased**
/// duplicates that bm25 misses (e.g. "server X uses PostgreSQL 16" vs
/// "PROD-DB-01 runs Postgres 16.x" — bm25 sees different words, but
/// the embedding sees the same fact).
///
/// If either stage matches: skip INSERT, bump the existing row's access
/// counters, return `{ action: "duplicate", id: existing }`.
///
/// `tags`  — JSON array string, e.g. `["rust","cargo","fix"]`
/// `files` — JSON array string of related file paths
// v1.7.112 audit C4 — process-wide serialization of the semantic-dedup
// critical section.
//
// THE RACE (before this lock): two save_agent_memory calls for
// semantically-similar-but-FTS-different content (paraphrases — "el
// servidor X cayó" vs "X server went down") could interleave:
//   T1: stage1 (FTS no match) → stage2 embed probe (no dup, nothing inserted yet)
//   T2: stage1 (FTS no match) → stage2 embed probe (still no dup)
//   T1: tx re-probe stage1 (FTS no match) → INSERT
//   T2: tx re-probe stage1 (FTS no match) → INSERT
//   → two near-duplicate rows survive. The in-tx re-probe only re-runs
//     stage1 (FTS), which never matched, so it can't catch this.
//
// THE FIX: hold an async mutex across the stage2-probe → insert window so
// the second caller's probe runs AFTER the first caller's row + embedding
// are committed, and therefore finds the now-existing semantic dup.
// save_agent_memory only fires when the LLM emits a <REMEMBER> tag (rare,
// human-paced) so full serialization here costs nothing measurable while
// eliminating the race outright. tokio::sync::Mutex (not std) because we
// hold the guard across the `.await` of stage2_embedding_dedup.
static SAVE_MEMORY_DEDUP_LOCK: once_cell::sync::Lazy<tokio::sync::Mutex<()>> =
    once_cell::sync::Lazy::new(|| tokio::sync::Mutex::new(()));

// v1.7.182 — observability. Counts saves that the dedup gates collapsed into an
// existing memory this process session, so `/memory-health` can show whether
// "Lucy can't save new things" is really the dedup folding distinct facts
// together (vs the agent never emitting <REMEMBER>).
pub(crate) static MEMORY_DEDUP_HITS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
/// Saves collapsed by Stage-1/2 dedup since process start.
pub fn memory_dedup_hits_session() -> u64 {
    MEMORY_DEDUP_HITS.load(std::sync::atomic::Ordering::Relaxed)
}
fn _note_dedup(stage: &str, id: i64, title: &str) {
    MEMORY_DEDUP_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let t: String = title.chars().take(60).collect();
    eprintln!("[memory] dedup {} → new save collapsed into existing #{} (title='{}')", stage, id, t);
}

#[tauri::command]
pub async fn save_agent_memory(
    title:      String,
    content:    String,
    tags:       Option<String>,
    files:      Option<String>,
    session_id: Option<String>,
    importance: Option<i64>,
    // agentmemory-inspired TTL (Tier 1 #1). Days from now until the memory
    // is eligible for auto-deletion via `auto_forget_run`. `None` (or 0)
    // keeps the memory forever — same as pre-TTL behaviour.
    ttl_days:   Option<i64>,
) -> Result<SaveMemoryResult, String> {
    // v1.7.109 audit C3 — PII / secret scrub at the persistence boundary.
    //
    // Without this, memories like "Conexión: postgres://admin:Sup3r!@db.prod"
    // — that the agent loop happily extracts from a `<REMEMBER>` tag — land
    // verbatim in agent_memories. From there they leak via:
    //   • DB backups (db_backup.rs serializes rows verbatim)
    //   • Memory Browser UI (renders the row content)
    //   • Export commands and snapshot replays
    //   • Future LLM prompts that recall the memory (the secret crosses the
    //     network boundary again on every recall)
    //
    // We reuse the same scrub_for_audit pass used by the audit logger
    // (utils/secret_scrubber.rs). It already has 11 unit tests and short-
    // circuits cleanly on non-secret text, so the typical mundane memory
    // costs one marker scan.
    //
    // Dedup notes: stage1 (FTS) and stage2 (embeddings) below operate on
    // the SCRUBBED text. That means two memories that differed only by
    // secret value (e.g. two API tokens for the same service) get correctly
    // dedup'd — which is what we want, since they encode the same fact.
    let title   = crate::utils::secret_scrubber::scrub_for_audit(&title);
    let content = crate::utils::secret_scrubber::scrub_for_audit(&content);

    // Fast-path Stage 1 probe OUTSIDE the lock — FTS dedup is atomic on its
    // own (single SELECT) and catches the common exact/near-exact repeat
    // without any serialization cost. Most duplicate saves are caught here.
    let stage1_result = with_db(|conn| stage1_fts_dedup(conn, &title, &content))?;
    if let Some(dup) = stage1_result {
        _note_dedup("S1(FTS)", dup.id, &title);
        return Ok(dup);
    }

    // v1.7.112 C4 — everything from the semantic probe through the INSERT is
    // the race-critical section. Serialize it so a concurrent paraphrase
    // save can't slip a near-duplicate past Stage 2's blind spot. Held
    // across the async embed probe, hence the tokio mutex.
    let _dedup_guard = SAVE_MEMORY_DEDUP_LOCK.lock().await;

    // Stage 2 — try embedding-based dedup. If Ollama is offline or any
    // step fails, fall through to insert without semantic dedup. Stage 2
    // is best-effort; we never block a save on it. With the lock held, a
    // concurrent save that started just before us has already committed its
    // row + embedding, so our probe now sees it and dedups correctly.
    let stage2_result = stage2_embedding_dedup(&title, &content).await;
    if let Ok(Some(dup)) = stage2_result {
        _note_dedup("S2(embedding)", dup.id, &title);
        return Ok(dup);
    }

    // v1.7.236 R4 — Stage 3: not a duplicate, but is it a CONTRADICTION of an
    // existing fact (same subject, different value)? Probe now — still under
    // the dedup lock so the old row can't be concurrently superseded — and
    // apply after the insert succeeds. Best-effort: None on Ollama-down /
    // timeout / no band candidate, and the save proceeds unchanged.
    let contra_old_id: Option<i64> = stage3_contradiction_check(&title, &content).await;

    // v1.7.103 Sprint-3 H7: re-probe stage 1 INSIDE the final tx so a
    // concurrent writer that snuck in during the async stage 2 window
    // cannot land a duplicate. Stage 2 (Ollama embed probe) is async
    // and intrinsically can't be in the same tx — so we accept that
    // narrow race for semantic dedup and tighten the FTS path which
    // we CAN gate atomically.
    //
    // The closure returns SaveMemoryResult directly (either the
    // re-probe hit OR the freshly inserted row), so the caller can't
    // tell whether the dedup landed in the first probe or the
    // re-probe — by design.
    let result = with_db(|conn| {
        let tx = conn.unchecked_transaction()
            .map_err(|e| format!("save_agent_memory tx open: {}", e))?;

        // Defensive re-probe under tx. If anyone slipped in between our
        // first stage1 call and this tx, surface as duplicate.
        if let Some(dup) = stage1_fts_dedup(&tx, &title, &content)? {
            // stage1_fts_dedup already incremented access_count via its
            // own UPDATE. Commit so that counter persists.
            _note_dedup("S1-reprobe", dup.id, &title);
            tx.commit().map_err(|e| format!("save_agent_memory tx commit (dup): {}", e))?;
            return Ok(dup);
        }

        let imp  = importance.unwrap_or(1).clamp(1, 3);
        // v1.7.236 (audit): tags and files are agent-controlled and were the ONLY
        // fields persisted UNSCRUBBED — a secret smuggled into a tag/file entry
        // (e.g. a token pasted as a "tag") bypassed the title/content scrub above.
        // Scrub the raw JSON strings at the persistence boundary, mirroring the
        // title/content scrub. Redaction stays inside the string values, so the
        // JSON array shape is preserved.
        let tags  = crate::utils::secret_scrubber::scrub_for_audit(
            &tags.unwrap_or_else(|| "[]".to_string()));
        let files = crate::utils::secret_scrubber::scrub_for_audit(
            &files.unwrap_or_else(|| "[]".to_string()));
        let sid   = session_id.unwrap_or_default();
        // TTL → absolute expires_at epoch seconds. ttl_days == None or 0
        // → keep forever (expires_at = 0, the default).
        let expires_at: i64 = match ttl_days.unwrap_or(0) {
            d if d > 0 => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                // ttl_days is agent-controlled (e.g. a <REMEMBER ttl=…> tag).
                // Clamp to ~100 years and use saturating math: release builds
                // are panic=abort + overflow-checks=on, so a raw `d * 86_400`
                // on an absurd value would overflow → panic → abort the whole
                // app. Saturating keeps it a (very far) future timestamp.
                let days = d.min(36_500);
                now.saturating_add(days.saturating_mul(86_400))
            }
            _ => 0,
        };
        tx.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, files, importance, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![sid, title, content, tags, files, imp, expires_at],
        ).map_err(|e| format!("save_agent_memory: {}", e))?;
        let new_id = tx.last_insert_rowid();
        tx.commit().map_err(|e| format!("save_agent_memory tx commit: {}", e))?;

        Ok(SaveMemoryResult {
            id: new_id,
            action: "inserted".to_string(),
            reason: "New memory stored".to_string(),
        })
    })?;

    // v1.7.236 R4 — apply the contradiction verdict: the OLD row is marked
    // superseded by the row we just inserted. Guarded by action == "inserted"
    // (the re-probe may have returned a duplicate instead) and by the WHERE
    // (`superseded_by IS NULL`) so an already-superseded row is never
    // re-pointed. Failure here never un-does the save.
    let mut result = result;
    if result.action == "inserted" {
        if let Some(old_id) = contra_old_id {
            if old_id != result.id {
                let n = with_db(|conn| {
                    conn.execute(
                        "UPDATE agent_memories SET superseded_by = ?2
                          WHERE id = ?1 AND superseded_by IS NULL",
                        rusqlite::params![old_id, result.id],
                    )
                    .map_err(|e| format!("stage3 supersede: {}", e))
                })
                .unwrap_or(0);
                if n == 1 {
                    result.action = "inserted_superseding".to_string();
                    result.reason = format!(
                        "Contradice a la memoria {} — la versión nueva la reemplaza (superseded)",
                        old_id
                    );
                }
            }
        }
    }

    Ok(result)
}

/// Stage 1 — FTS5 bm25 dedup. Returns Some(dup-result) when a strong
/// match is found, None otherwise. Synchronous (DB-only).
fn stage1_fts_dedup(
    conn: &rusqlite::Connection,
    title: &str,
    content: &str,
) -> Result<Option<SaveMemoryResult>, String> {
    let probe = format!("{} {}", title, content.chars().take(200).collect::<String>());
    let safe_q = probe
        .split_whitespace()
        .filter(|w| w.len() > 2 && !w.chars().any(|c| c.is_control()))
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .take(20)
        .collect::<Vec<_>>()
        .join(" OR ");

    if safe_q.is_empty() {
        return Ok(None);
    }

    let best: rusqlite::Result<(i64, f64)> = conn.query_row(
        // v1.7.237 — exclude ALL document-derived rows from the dedup candidate
        // set: 'pdf:%' chunks AND 'pdf-doc:%' ingest-summaries. Both live as
        // agent_memories rows in the FTS index (pdf.rs) but are "not memories"
        // for user-memory dedup. Without this, a user-authored capsule saturated
        // with a document's vocabulary (e.g. "GoAnywhere MFT - Resumen Técnico
        // Core") bm25-matches a raw chunk OR the doc's ingest-summary, returns
        // action:"duplicate", and is NEVER inserted — the "I saved it /
        // deduplicated automatically, but the capsule is invisible" bug. CONFIRMED
        // in the field: Lucy dedup'd the capsule against the installation_guide /
        // architecture_guide pdf-doc summaries. Dedup collapses only real memories.
        "SELECT am.id, bm25(agent_memories_fts) AS score
         FROM agent_memories am
         JOIN agent_memories_fts fts ON am.id = fts.rowid
         WHERE agent_memories_fts MATCH ?1
           AND am.superseded_by IS NULL
           AND am.session_id NOT LIKE 'pdf:%'
           AND am.session_id NOT LIKE 'pdf-doc:%'
         ORDER BY score ASC
         LIMIT 1",
        rusqlite::params![safe_q],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );

    if let Ok((dup_id, score)) = best {
        if score < -8.0 {
            let _ = conn.execute(
                "UPDATE agent_memories
                 SET access_count = access_count + 1,
                     last_accessed_at = strftime('%s','now')
                 WHERE id = ?1",
                rusqlite::params![dup_id],
            );
            return Ok(Some(SaveMemoryResult {
                id: dup_id,
                action: "duplicate".to_string(),
                reason: format!("FTS bm25 score {:.2} matches memory {}", score, dup_id),
            }));
        }
    }
    Ok(None)
}

/// Stage 2 — embedding cosine dedup (Mem0 deep). Best-effort: requires
/// Ollama embeddings; returns Ok(None) if anything goes wrong so the
/// caller falls back to a normal insert.
async fn stage2_embedding_dedup(
    title: &str,
    content: &str,
) -> Result<Option<SaveMemoryResult>, String> {
    // Build the same probe as stage 1 — title + first chunk of content
    let probe = format!("{}\n{}", title, content.chars().take(800).collect::<String>());

    // Embed via Ollama. If embeddings aren't available we silently skip.
    let embed_res = crate::commands::embeddings::embed_via_ollama_pub(&probe, None).await;
    let (query_vec, query_model) = match embed_res {
        Ok(pair) => pair,
        Err(_) => return Ok(None),  // Ollama offline / model missing — skip stage 2
    };

    // Search the in-memory vec index for high-similarity memories
    let hits = crate::commands::vec_index::search(&query_vec, &query_model, 5, 0.92);
    let best_memory = hits.iter()
        .find(|(etype, _id, _text, _score)| etype == "memory");
    let Some((_etype, entity_id, _text, score)) = best_memory else {
        return Ok(None);
    };

    let dup_id: i64 = match entity_id.parse() {
        Ok(i) => i,
        Err(_) => return Ok(None),  // entity_id wasn't a memory row id — skip
    };

    // Touch the existing row — and use the UPDATE's row count as a LIVENESS
    // CHECK. The in-memory vec_index (vec_index.rs) exposes only search(); it
    // is never pruned when a memory is deleted or superseded. So a hit here
    // can point at a row that no longer qualifies as a dedup target. The
    // WHERE clause (`id = ?1 AND superseded_by IS NULL`) only matches a live,
    // non-superseded row, so:
    //   • touched == 1 → genuine duplicate, collapse into it.
    //   • touched == 0 → the matched row was deleted or superseded (stale
    //     index entry). Treating that as a duplicate would DROP the new
    //     memory into a dangling id (the "memory isn't saved" symptom), so we
    //     fall through to a normal insert instead.
    // A DB error also yields 0 → fall through and insert (never lose a save).
    let touched = with_db(|conn| {
        // v1.7.237 — also exclude document-derived rows (pdf: chunks and
        // pdf-doc: summaries). A pdf-doc summary is embedded as entity_type
        // 'memory', so the vec_index hit above CAN point at one; the session_id
        // guard makes touched==0 for it → the code below falls through to a
        // normal insert instead of collapsing a real user memory into a
        // document stub. Mirrors the stage-1 FTS exclusion.
        conn.execute(
            "UPDATE agent_memories
             SET access_count = access_count + 1,
                 last_accessed_at = strftime('%s','now')
             WHERE id = ?1 AND superseded_by IS NULL
               AND session_id NOT LIKE 'pdf:%'
               AND session_id NOT LIKE 'pdf-doc:%'",
            rusqlite::params![dup_id],
        ).map_err(|e| format!("touch failed: {}", e))
    }).unwrap_or(0);

    if touched == 0 {
        return Ok(None);
    }

    Ok(Some(SaveMemoryResult {
        id: dup_id,
        action: "duplicate".to_string(),
        reason: format!("Embedding cosine {:.3} matches memory {} (semantic paraphrase)", score, dup_id),
    }))
}

// ── v1.7.236 R4 — Stage 3: contradiction detection at save ───────────────────
// Dedup (stages 1-2) catches "same fact, same value". This catches "same
// subject, DIFFERENT value" — "el timeout de GoAnywhere es 30s" vs "…es 60s".
// Without it both memories coexist and recall can inject the stale one; for
// unattended users that trust Lucy blindly, serving obsolete config is the
// worst failure mode. Flow: embeddings put the pair in the similarity band
// [0.78, 0.92) (above = dup already caught, below = unrelated), then a local
// LLM gives a strict yes/no verdict; on YES the OLD row is marked
// superseded_by = new_id (existing machinery — every read path already
// excludes superseded rows). Best-effort at every step: any failure means
// "no contradiction", never a lost save.

const CONTRA_BAND_LOW: f32 = 0.78;
const CONTRA_BAND_HIGH: f32 = 0.92; // == stage-2 dup threshold

/// Pure verdict parser (unit-tested): strips qwen3 <think> blocks, then the
/// first token decides. Anything that isn't a clear SI/YES is a NO — a false
/// "contradiction" would silently hide a valid memory, so we bias hard toward
/// keeping both.
fn parse_contradiction_verdict(raw: &str) -> bool {
    let mut s = raw.trim();
    if let Some(p) = s.find("</think>") {
        s = s[p + 8..].trim();
    }
    let first = s
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_uppercase();
    matches!(first.as_str(), "SI" | "SÍ" | "YES")
}

/// Strict yes/no verdict from the local LLM (same model + base as the M3
/// query expansion). Bounded by the client timeout; None on any failure.
async fn ollama_contradiction_verdict(old_text: &str, new_text: &str) -> Option<bool> {
    let base = crate::commands::embeddings::ollama_base();
    let sys = "Eres un verificador de hechos para la memoria de un asistente SysAdmin. \
Te doy un hecho VIEJO y un hecho NUEVO sobre un tema similar. Responde EXACTAMENTE una palabra: \
SI — si afirman valores/estados INCOMPATIBLES sobre el mismo asunto (el nuevo reemplaza al viejo), \
NO — si son compatibles, complementarios o hablan de asuntos distintos. \
Ante la duda responde NO.";
    let user = format!(
        "VIEJO:\n{}\n\nNUEVO:\n{}",
        old_text.chars().take(700).collect::<String>(),
        new_text.chars().take(700).collect::<String>()
    );
    let body = serde_json::json!({
        "model": "qwen3:4b",
        "messages": [
            { "role": "system", "content": sys },
            { "role": "user",   "content": user }
        ],
        "stream": false,
        "options": { "temperature": 0.0, "num_predict": 60 }
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;
    let resp = client.post(format!("{}/api/chat", base)).json(&body).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let out = json
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    if out.trim().is_empty() {
        return None;
    }
    Some(parse_contradiction_verdict(out))
}

/// Returns Some(old_id) when the new memory contradicts a live, non-pdf row in
/// the similarity band. The probe embed is LRU-cached (same text stage 2 just
/// embedded), so this costs one extra Ollama round-trip only when a band
/// candidate actually exists.
async fn stage3_contradiction_check(title: &str, content: &str) -> Option<i64> {
    let probe = format!("{}\n{}", title, content.chars().take(800).collect::<String>());
    let (query_vec, query_model) = crate::commands::embeddings::embed_via_ollama_pub(&probe, None)
        .await
        .ok()?;
    let hits = crate::commands::vec_index::search(&query_vec, &query_model, 5, CONTRA_BAND_LOW);
    let cand = hits
        .iter()
        .find(|(etype, _id, _t, score)| etype == "memory" && *score < CONTRA_BAND_HIGH)?;
    let old_id: i64 = cand.1.parse().ok()?;
    // Liveness + scope: only live, non-superseded, non-document rows can be
    // contradicted (pdf chunks are reference material, not mutable facts).
    let old = with_db(|conn| {
        conn.query_row(
            "SELECT title, content FROM agent_memories
              WHERE id = ?1 AND superseded_by IS NULL AND session_id NOT LIKE 'pdf:%'",
            rusqlite::params![old_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .map_err(|e| format!("stage3 fetch: {}", e))
    })
    .ok()?;
    let old_text = format!("{}\n{}", old.0, old.1);
    let new_text = format!("{}\n{}", title, content);
    match ollama_contradiction_verdict(&old_text, &new_text).await {
        Some(true) => Some(old_id),
        _ => None,
    }
}

/// v1.7.233 — Diversity cap for ranked memory results: at most 2 chunks per
/// ingested document (session_id 'pdf:<doc_id>') in one result set. Same-doc
/// chunks are near-identical (shared heading prefix + 200-char overlap), so
/// letting one manual fill all 8-10 slots crowds out both real memories and
/// other documents. Non-pdf rows pass through untouched.
fn cap_per_document(ranked: Vec<(AgentMemory, f64)>, limit: usize) -> Vec<AgentMemory> {
    const PER_DOC: usize = 2;
    let mut per_doc: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut out: Vec<AgentMemory> = Vec::with_capacity(limit);
    for (m, _) in ranked {
        if out.len() >= limit { break; }
        if m.session_id.starts_with("pdf:") {
            let c = per_doc.entry(m.session_id.clone()).or_insert(0usize);
            if *c >= PER_DOC { continue; }
            *c += 1;
        }
        out.push(m);
    }
    out
}

/// Hybrid retrieval over memories — **RRF fusion** (BM25 + embedding
/// cosine) with Mem0-inspired recency/access decay (May 2026).
///
/// Pipeline (inspired by rohitg00/agentmemory v0.11):
///   1. Run BM25 search to get top-N (lexical match)
///   2. If Ollama embeddings are available, run cosine search via the
///      in-memory vec_index, filtered to `entity_type=memory`
///   3. Fuse both ranked lists with Reciprocal Rank Fusion (k=60):
///        rrf_score(id) = Σ 1 / (k + rank_in_stream_i)
///      A memory in BOTH lists scores higher than the same memory in
///      only one — caught by lexical AND semantic signals.
///   4. Apply decay/access/importance bonus as a multiplicative post-rank
///      (so frequently-used / fresh memories outrank stale ones with
///      similar fusion score).
///   5. Touch the matched rows so future searches see them as "hot".
///
/// Graceful degradation: when Ollama is offline or the vec_index isn't
/// ready, the BM25 stream alone drives ranking — identical to the
/// previous v1.4.0 behavior.
///
/// Superseded memories (`superseded_by IS NOT NULL`) are excluded by
/// default — they remain in the table for audit but never surface in
/// normal retrieval.
#[tauri::command]
pub async fn search_agent_memories(
    query: String,
    limit: Option<i64>,
    // v1.7.237 — Some(true) excluye filas derivadas de documentos (pdf:%/pdf-doc:%)
    // para que la busqueda del Explorador (que quiere MEMORIAS) no muestre chunks
    // ni resumenes de PDF como si fueran memorias. None/false = comportamiento
    // actual (incluye documentos): lo usan otras rutas como el fallback-keyword
    // del recall, que SI quiere documentacion. Gemelo del fix de dedup v1.7.237.
    exclude_documents: Option<bool>,
) -> Result<Vec<AgentMemory>, String> {
    let lim = limit.unwrap_or(10).clamp(1, 50);
    if query.trim().is_empty() {
        return with_db(|_conn| get_recent_memories(Some(lim)));
    }
    // Fragmentos SQL opcionales de exclusion de documentos: el stream BM25 aliasa
    // la tabla (am.), el fetch final no.
    let bm25_doc_filter = if exclude_documents.unwrap_or(false) {
        " AND am.session_id NOT LIKE 'pdf:%' AND am.session_id NOT LIKE 'pdf-doc:%'"
    } else { "" };
    let fetch_doc_filter = if exclude_documents.unwrap_or(false) {
        " AND session_id NOT LIKE 'pdf:%' AND session_id NOT LIKE 'pdf-doc:%'"
    } else { "" };
    // Cast to usize for ranking math; FETCH_N is the pool we draw from in
    // each stream before fusion — wider than `lim` so the fusion has
    // headroom to interleave the two ranked lists.
    const FETCH_N: usize = 20;
    const RRF_K: f64 = 60.0;
    let lim_usize = lim as usize;

    // ── Stream 1: BM25 (lexical) ──────────────────────────────────────
    // Returns up to FETCH_N ids ranked by bm25. We DON'T apply decay
    // here — that's a post-fusion multiplier so it doesn't distort the
    // RRF rank ordering between streams.
    let bm25_ids: Vec<i64> = with_db(|conn| {
        // Tier 1 #3: SysAdmin-domain synonym expansion (ps↔process, gpo↔
        // group-policy, dns↔name-resolution, etc.) — see commands/synonyms.rs
        let safe_q = crate::commands::synonyms::expand_query(&query);
        if safe_q.is_empty() { return Ok(Vec::new()); }
        let sql = format!(
            "SELECT am.id
                   FROM agent_memories am
                   JOIN agent_memories_fts fts ON am.id = fts.rowid
                   WHERE agent_memories_fts MATCH ?1
                     AND am.superseded_by IS NULL{}
                   ORDER BY bm25(agent_memories_fts) ASC
                   LIMIT ?2",
            bm25_doc_filter
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("bm25 prepare: {}", e))?;
        let n = FETCH_N as i64;
        let rows = stmt.query_map(rusqlite::params![safe_q, n], |r| r.get::<_, i64>(0))
            .map_err(|e| format!("bm25 query: {}", e))?;
        let mut ids = Vec::with_capacity(FETCH_N);
        for id in rows.flatten() { ids.push(id); }
        Ok(ids)
    })?;

    // ── Stream 2: embedding cosine (best-effort, async) ──────────────
    // Skip silently if Ollama embeddings aren't available — RRF over a
    // single stream is still valid, it just reduces to bm25 ranking.
    let cosine_ids: Vec<i64> = match crate::commands::embeddings::embed_via_ollama_pub(&query, None).await {
        Ok((qvec, qmodel)) => {
            crate::commands::vec_index::search(&qvec, &qmodel, FETCH_N, 0.50)
                .into_iter()
                .filter(|(etype, _, _, _)| etype == "memory")
                .filter_map(|(_, eid, _, _)| eid.parse::<i64>().ok())
                .collect()
        }
        Err(_) => Vec::new(),
    };

    // ── RRF fusion ────────────────────────────────────────────────────
    // For each id seen in either stream, sum 1/(k + rank). k=60 is the
    // canonical RRF constant from Cormack et al. — high enough that the
    // top of one stream doesn't dominate the other.
    use std::collections::HashMap;
    let mut fused: HashMap<i64, f64> = HashMap::with_capacity(FETCH_N * 2);
    for (rank, id) in bm25_ids.iter().enumerate() {
        *fused.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + (rank as f64 + 1.0));
    }
    for (rank, id) in cosine_ids.iter().enumerate() {
        *fused.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + (rank as f64 + 1.0));
    }

    if fused.is_empty() {
        return Ok(Vec::new());
    }

    // ── Fetch full rows + apply decay/access/importance multiplier ────
    let ids_vec: Vec<i64> = fused.keys().copied().collect();
    let rows = with_db(|conn| {
        let in_clause = ids_vec.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        // Safe: ids come from i64 results of SELECT, can't be injected.
        // fetch_doc_filter (v1.7.237) excluye pdf:%/pdf-doc: cuando
        // exclude_documents=Some(true) — necesario porque el stream coseno puede
        // traer resumenes pdf-doc: (embebidos como entity_type 'memory').
        let sql = format!(
            "SELECT id, session_id, title, content, tags, files, importance, created_at,
                    access_count, last_accessed_at
             FROM agent_memories
             WHERE id IN ({})
               AND superseded_by IS NULL{}",
            in_clause, fetch_doc_filter
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("fetch prepare: {}", e))?;
        let mapped = stmt.query_map([], |row| {
            Ok((
                AgentMemory {
                    id:          row.get(0)?,
                    session_id:  row.get(1)?,
                    title:       row.get(2)?,
                    content:     row.get(3)?,
                    tags:        row.get(4)?,
                    files:       row.get(5)?,
                    importance:  row.get(6)?,
                    created_at:  row.get(7)?,
                },
                row.get::<_, i64>(8).unwrap_or(0) as f64,  // access_count
                row.get::<_, i64>(9).unwrap_or(0) as f64,  // last_accessed_at
            ))
        }).map_err(|e| format!("fetch query: {}", e))?;
        let now = chrono::Utc::now().timestamp() as f64;
        let mut combined: Vec<(AgentMemory, f64)> = Vec::new();
        for r in mapped {
            let Ok((m, access, last_acc)) = r else { continue };
            let rrf = fused.get(&m.id).copied().unwrap_or(0.0);
            // Decay/access/importance multiplier — keep the RRF rank as
            // primary signal but bias toward fresh + frequently-used.
            // Range roughly [0.5..2.5] given our v1.4 thresholds.
            let imp_bonus    = (m.importance as f64) * 0.10;
            let access_bonus = ((access + 1.0).ln() / 2f64.ln()) * 0.05;
            let age_sec      = (now - last_acc).max(0.0);
            let recency_bonus = (-age_sec / 86400.0).exp() * 0.30;
            let multiplier = 1.0 + imp_bonus + access_bonus + recency_bonus;
            combined.push((m, rrf * multiplier));
        }
        combined.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        // v1.7.233: per-document diversity cap (≤2 chunks per ingested doc).
        let top: Vec<AgentMemory> = cap_per_document(combined, lim_usize);

        // Touch matched rows so frequently-retrieved memories surface higher
        // in future searches. Fire-and-forget — if the UPDATE fails we still
        // return results.
        if !top.is_empty() {
            let id_list = top.iter().map(|m| m.id.to_string()).collect::<Vec<_>>().join(",");
            let touch_sql = format!(
                "UPDATE agent_memories
                 SET access_count = access_count + 1,
                     last_accessed_at = strftime('%s','now')
                 WHERE id IN ({})",
                id_list
            );
            let _ = conn.execute(&touch_sql, []);
        }
        Ok(top)
    })?;

    Ok(rows)
}

// ── Query expansion (Tier 1 #2, agentmemory-inspired) ────────────────────
// Ask the local LLM for 2-3 reformulations of the user's query, then run
// BM25 + cosine on each, fuse everything via RRF. Massive recall win on
// vague queries ("DNS issues" → ["DNS resolution failures", "name lookup
// problems", "AD DNS errors"]).
//
// Always best-effort: if Ollama is offline / slow / returns nonsense, we
// fall back to the original query and the result is identical to plain
// search_agent_memories. Cached in-memory so a repeated search is free.

use std::sync::Mutex;
use once_cell::sync::Lazy;

// Tiny LRU-ish cache (size-capped, no time-based eviction). 5-min freshness
// is plenty for an interactive session; restart clears it.
static EXPAND_CACHE: Lazy<Mutex<std::collections::HashMap<String, Vec<String>>>> =
    Lazy::new(|| Mutex::new(std::collections::HashMap::with_capacity(64)));

const EXPAND_CACHE_MAX: usize = 64;

/// Ask Ollama for N reformulations of `query`. Returns an empty vec on
/// any failure (network, parse, timeout). The original query is NEVER
/// included — callers add it explicitly so they control rank ordering.
// v1.7.233 (M3): pub(crate) — pdf.rs reusa la expansión en pdf_search.
pub(crate) async fn expand_query_via_ollama(query: &str) -> Vec<String> {
    // Trivially short queries don't benefit from expansion — single keyword
    // queries are already maximally lexical-friendly.
    let trimmed = query.trim();
    if trimmed.len() < 8 || trimmed.split_whitespace().count() < 2 {
        return Vec::new();
    }

    // Cache hit?
    if let Ok(cache) = EXPAND_CACHE.lock() {
        if let Some(hit) = cache.get(trimmed) {
            return hit.clone();
        }
    }

    let base = crate::commands::embeddings::ollama_base();
    let sys = "You are a query expansion engine for an autonomous SysAdmin assistant's memory search. \
Given a user query, output 3 alternative phrasings that capture the SAME intent with DIFFERENT vocabulary \
(synonyms, paraphrases, domain-specific restatements). Output EXACTLY 3 lines, one phrasing per line, \
no numbering, no quotes, no explanations, no XML, no JSON. Keep each line under 100 characters.";

    let body = serde_json::json!({
        "model": "qwen3:4b",   // small + fast; falls back via try-with-fallback below
        "messages": [
            { "role": "system", "content": sys },
            { "role": "user",   "content": format!("Query: {}", trimmed) },
        ],
        "stream": false,
        "options": { "temperature": 0.4, "num_predict": 200 },
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))  // hard cap — never block search > 8s
        .build()
        .ok();
    let client = match client { Some(c) => c, None => return Vec::new() };

    let resp = client
        .post(format!("{}/api/chat", base))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await;
    let resp = match resp { Ok(r) => r, Err(_) => return Vec::new() };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j, Err(_) => return Vec::new(),
    };

    let text = json
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    let reformulations: Vec<String> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && l.len() <= 200)
        // Strip common LLM artefacts (numbering, quotes, dashes)
        .map(|l| l.trim_start_matches(|c: char| c.is_ascii_digit() || matches!(c, '.' | ')' | '-' | '*' | ' '))
                  .trim_matches(|c| matches!(c, '"' | '\'' | '`'))
                  .trim()
                  .to_string())
        .filter(|l| !l.is_empty() && l.to_lowercase() != trimmed.to_lowercase())
        .take(3)
        .collect();

    // Cache (with simple cap eviction — drop a random key when full)
    if let Ok(mut cache) = EXPAND_CACHE.lock() {
        if cache.len() >= EXPAND_CACHE_MAX {
            if let Some(k) = cache.keys().next().cloned() {
                cache.remove(&k);
            }
        }
        cache.insert(trimmed.to_string(), reformulations.clone());
    }

    reformulations
}

/// Run BM25 against `agent_memories_fts` for a single query string,
/// returning up to `limit` ids ranked best-first. Shared between
/// search_agent_memories and search_agent_memories_expanded.
fn bm25_search_one(query: &str, limit: usize) -> Result<Vec<i64>, String> {
    with_db(|conn| {
        // Tier 1 #3: expand each token with its SysAdmin-domain synonyms
        // before handing to FTS5 — "ps" matches "process" rows, "gpo"
        // matches "group policy", etc. Unknown tokens pass through.
        let safe_q = crate::commands::synonyms::expand_query(query);
        if safe_q.is_empty() { return Ok(Vec::new()); }
        // v1.7.237 — exclude document-derived rows ('pdf:%' chunks + 'pdf-doc:%'
        // ingest-summaries). bm25_search_one is used ONLY by
        // search_agent_memories_expanded (= the `memoria_buscar` agent tool),
        // which searches MEMORIES; documents are retrieved via the separate
        // pdf_search + the proactive PDF list in the system prompt. Without this,
        // the FTS index (hundreds of pdf rows) floods `memoria_buscar` with
        // document fragments so the model never finds the memory it wants — the
        // "hunt for the capsule ID that loops to MAX_LOOPS" bug. The base
        // search_agent_memories (its own inline bm25) is intentionally untouched
        // (browser/lessons keep their current behavior).
        let sql = "SELECT am.id
                   FROM agent_memories am
                   JOIN agent_memories_fts fts ON am.id = fts.rowid
                   WHERE agent_memories_fts MATCH ?1
                     AND am.superseded_by IS NULL
                     AND am.session_id NOT LIKE 'pdf:%'
                     AND am.session_id NOT LIKE 'pdf-doc:%'
                   ORDER BY bm25(agent_memories_fts) ASC
                   LIMIT ?2";
        let mut stmt = conn.prepare(sql).map_err(|e| format!("bm25 prepare: {}", e))?;
        let n = limit as i64;
        let rows = stmt.query_map(rusqlite::params![safe_q, n], |r| r.get::<_, i64>(0))
            .map_err(|e| format!("bm25 query: {}", e))?;
        let mut ids = Vec::with_capacity(limit);
        for id in rows.flatten() { ids.push(id); }
        Ok(ids)
    })
}

/// Multi-query variant of search_agent_memories — opt-in via this command.
/// Generates 3 LLM reformulations of `query`, runs BM25 + cosine for each
/// (4 query strings × 2 streams = up to 8 ranked lists), fuses them all
/// via RRF, then applies the same decay/access/importance multiplier as
/// the standard search.
///
/// Use this for **agent-driven** memory recall where 1-3s extra latency
/// is acceptable for ~15-25% better recall on vague queries. UI live
/// search should stay on the fast path (search_agent_memories).
///
/// Cached: a repeated query hits the in-memory expansion cache instantly.
#[tauri::command]
pub async fn search_agent_memories_expanded(
    query: String,
    limit: Option<i64>,
) -> Result<Vec<AgentMemory>, String> {
    let lim = limit.unwrap_or(10).clamp(1, 50);
    if query.trim().is_empty() {
        return get_recent_memories(Some(lim));
    }
    const FETCH_N: usize = 20;
    const RRF_K: f64 = 60.0;
    let lim_usize = lim as usize;

    // ── Expand query (best-effort, ≤8s) ──────────────────────────────
    let mut all_queries: Vec<String> = vec![query.clone()];
    let reformulations = expand_query_via_ollama(&query).await;
    all_queries.extend(reformulations);

    // ── Run BM25 + cosine for each query string in parallel ──────────
    // BM25 is sync (DB), cosine needs an embedding call per query — run
    // those concurrently to keep wall-clock close to a single-query search.
    let bm25_lists: Vec<Vec<i64>> = all_queries
        .iter()
        .map(|q| bm25_search_one(q, FETCH_N).unwrap_or_default())
        .collect();

    // Cosine: embed each query, search vec_index, collect ids
    let mut cosine_lists: Vec<Vec<i64>> = Vec::with_capacity(all_queries.len());
    for q in &all_queries {
        let ids = match crate::commands::embeddings::embed_via_ollama_pub(q, None).await {
            Ok((qvec, qmodel)) => crate::commands::vec_index::search(&qvec, &qmodel, FETCH_N, 0.50)
                .into_iter()
                .filter(|(etype, _, _, _)| etype == "memory")
                .filter_map(|(_, eid, _, _)| eid.parse::<i64>().ok())
                .collect(),
            Err(_) => Vec::new(),
        };
        cosine_lists.push(ids);
    }

    // ── RRF over all streams ─────────────────────────────────────────
    use std::collections::HashMap;
    let mut fused: HashMap<i64, f64> = HashMap::with_capacity(FETCH_N * 4);
    for list in bm25_lists.iter().chain(cosine_lists.iter()) {
        for (rank, id) in list.iter().enumerate() {
            *fused.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + (rank as f64 + 1.0));
        }
    }
    if fused.is_empty() {
        return Ok(Vec::new());
    }

    // ── Same fetch + multiplier path as the standard search ─────────
    let ids_vec: Vec<i64> = fused.keys().copied().collect();
    let rows = with_db(|conn| {
        let in_clause = ids_vec.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, session_id, title, content, tags, files, importance, created_at,
                    access_count, last_accessed_at
             FROM agent_memories
             WHERE id IN ({})
               AND superseded_by IS NULL",
            in_clause
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("fetch prepare: {}", e))?;
        let mapped = stmt.query_map([], |row| {
            Ok((
                AgentMemory {
                    id:          row.get(0)?,
                    session_id:  row.get(1)?,
                    title:       row.get(2)?,
                    content:     row.get(3)?,
                    tags:        row.get(4)?,
                    files:       row.get(5)?,
                    importance:  row.get(6)?,
                    created_at:  row.get(7)?,
                },
                row.get::<_, i64>(8).unwrap_or(0) as f64,
                row.get::<_, i64>(9).unwrap_or(0) as f64,
            ))
        }).map_err(|e| format!("fetch query: {}", e))?;
        let now = chrono::Utc::now().timestamp() as f64;
        let mut combined: Vec<(AgentMemory, f64)> = Vec::new();
        for r in mapped {
            let Ok((m, access, last_acc)) = r else { continue };
            let rrf = fused.get(&m.id).copied().unwrap_or(0.0);
            let imp_bonus    = (m.importance as f64) * 0.10;
            let access_bonus = ((access + 1.0).ln() / 2f64.ln()) * 0.05;
            let age_sec      = (now - last_acc).max(0.0);
            let recency_bonus = (-age_sec / 86400.0).exp() * 0.30;
            let multiplier = 1.0 + imp_bonus + access_bonus + recency_bonus;
            combined.push((m, rrf * multiplier));
        }
        // Initial sort by RRF*multiplier — we may overwrite this below
        // with the cross-encoder ranking, but we still cap at FETCH_N=20
        // candidates first so reranker cost stays bounded.
        combined.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(combined)
    })?;

    // ── Cross-encoder reranker (Tier 3 #7, feature-gated) ────────────
    // If the `ml-reranker` feature is built AND the ONNX model is
    // available, re-score the top-FETCH_N candidates against the
    // ORIGINAL query (not reformulations — the cross-encoder needs the
    // user's actual intent). On any failure (model missing, runtime
    // missing, inference error) we degrade gracefully to RRF ranking.
    let mut candidates = rows;
    candidates.truncate(FETCH_N);  // bound reranker cost regardless of stream sizes

    let reranked: Option<Vec<(AgentMemory, f64)>> = (|| {
        if candidates.is_empty() { return None; }
        let passages: Vec<&str> = candidates.iter().map(|(m, _)| m.content.as_str()).collect();
        let scores = crate::commands::reranker::rerank(&query, &passages)?;
        if scores.len() != candidates.len() { return None; }
        // Pair (memory, score) — drop the old RRF*mult ranking, use the
        // cross-encoder logit directly. Higher = more relevant.
        let mut paired: Vec<(AgentMemory, f64)> = candidates.iter()
            .zip(scores.iter())
            .map(|((m, _), s)| (m.clone(), *s as f64))
            .collect();
        paired.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Some(paired)
    })();

    let final_ranked = reranked.unwrap_or(candidates);
    // v1.7.233: per-document diversity cap (≤2 chunks per ingested doc).
    let top: Vec<AgentMemory> = cap_per_document(final_ranked, lim_usize);

    // ── Touch returned rows for next-search ranking boost ───────────
    if !top.is_empty() {
        let id_list = top.iter().map(|m| m.id.to_string()).collect::<Vec<_>>().join(",");
        with_db(|conn| {
            let touch_sql = format!(
                "UPDATE agent_memories
                 SET access_count = access_count + 1,
                     last_accessed_at = strftime('%s','now')
                 WHERE id IN ({})",
                id_list
            );
            let _ = conn.execute(&touch_sql, []);
            Ok(())
        }).ok();
    }

    Ok(top)
}

/// Mark a memory as superseded by a newer one (Mem0-style conflict
/// resolution). The old row stays in the table for audit but is excluded
/// from default search results.
///
/// Use case: agent learns "user prefers vim" then later "user prefers
/// neovim". Calling supersede(old_id, new_id) keeps both rows but only
/// the new one surfaces in retrieval. Audit views can still show the
/// full chain.
#[tauri::command]
pub fn supersede_memory(old_id: i64, new_id: i64) -> Result<usize, String> {
    if old_id <= 0 || new_id <= 0 || old_id == new_id {
        return Err("supersede_memory: invalid ids".to_string());
    }
    with_db(|conn| {
        let n = conn.execute(
            "UPDATE agent_memories SET superseded_by = ?2 WHERE id = ?1",
            rusqlite::params![old_id, new_id],
        ).map_err(|e| format!("supersede_memory: {}", e))?;
        Ok(n)
    })
}

/// Delete a single agent memory by id. Cascades through the FTS trigger
/// already declared in db.rs (agent_memories_ad).
///
/// Returns the number of rows deleted (0 if id not found, 1 on success).
/// User-facing rationale: Lucy reported "I consolidated 13 memories into 1"
/// but the 13 originals stayed alive because no delete tool existed. Now she
/// can actually clean up after herself.
#[tauri::command]
pub fn delete_agent_memory(id: i64) -> Result<usize, String> {
    if id <= 0 {
        return Err("delete_agent_memory: invalid id".to_string());
    }
    with_db(|conn| {
        let n = conn.execute(
            "DELETE FROM agent_memories WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| format!("delete_agent_memory: {}", e))?;
        Ok(n)
    })
}

// ── Auto-forget (Tier 1 #1, agentmemory-inspired) ────────────────────────
// Periodic cleanup that keeps the memory store from growing unbounded.
// Three eviction policies, all conservative (importance=3 is NEVER evicted):
//
//   1. TTL expired   — rows with expires_at > 0 AND expires_at < now()
//   2. Low-value old — rows never accessed (access_count=0) older than
//                      LOW_VALUE_AGE_DAYS, importance==1, no TTL set
//   3. (future) contradiction detection via embedding cosine > 0.9 across
//      memories — flag for manual review rather than auto-delete
//
// Always preserves: superseded memories (audit trail) and importance==3
// rows (user explicitly pinned them). Dry-run mode reports what WOULD be
// deleted without touching the DB — for UI confirmation flows.

const LOW_VALUE_AGE_DAYS: i64 = 30;

#[derive(serde::Serialize)]
pub struct AutoForgetReport {
    pub dry_run: bool,
    pub ttl_expired: i64,
    pub low_value: i64,
    pub total_deleted: i64,
    pub now_epoch: i64,
}

/// Run the auto-forget sweep. `dry_run = true` reports counts without
/// deleting; `false` actually removes rows (cascades through the FTS
/// delete trigger). Idempotent — safe to call on every app startup
/// and via a UI button.
#[tauri::command]
pub fn auto_forget_run(dry_run: Option<bool>) -> Result<AutoForgetReport, String> {
    let dry = dry_run.unwrap_or(false);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let low_value_cutoff = now - (LOW_VALUE_AGE_DAYS * 86_400);

    with_db(|conn| {
        // ── 1. Count + (optionally) delete TTL-expired rows ────────────
        let ttl_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_memories
             WHERE expires_at > 0 AND expires_at < ?1
               AND importance < 3",
            rusqlite::params![now],
            |r| r.get(0),
        ).unwrap_or(0);

        let ttl_deleted = if !dry && ttl_count > 0 {
            conn.execute(
                "DELETE FROM agent_memories
                 WHERE expires_at > 0 AND expires_at < ?1
                   AND importance < 3",
                rusqlite::params![now],
            ).map_err(|e| format!("auto_forget_run TTL: {}", e))? as i64
        } else { 0 };

        // ── 2. Count + (optionally) delete low-value old rows ──────────
        // Conservative: only importance==1 + access_count==0 + no TTL set
        // (TTL=0 means user/code didn't explicitly opt in to keep forever
        // — they just didn't set a TTL, so we treat that as eligible).
        // v1.7.233: PDF chunks excluded — they are importance-1 reference
        // material managed at the DOCUMENT level (pdf_delete_doc), and an
        // unqueried manual section is not "low value", it just hasn't been
        // needed yet. Without this, the janitor would silently erode an
        // ingested manual 30 days after ingest.
        let low_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_memories
             WHERE access_count = 0
               AND importance = 1
               AND expires_at = 0
               AND created_at < ?1
               AND superseded_by IS NULL
               AND session_id NOT LIKE 'pdf:%'",
            rusqlite::params![low_value_cutoff],
            |r| r.get(0),
        ).unwrap_or(0);

        let low_deleted = if !dry && low_count > 0 {
            conn.execute(
                "DELETE FROM agent_memories
                 WHERE access_count = 0
                   AND importance = 1
                   AND expires_at = 0
                   AND created_at < ?1
                   AND superseded_by IS NULL
                   AND session_id NOT LIKE 'pdf:%'",
                rusqlite::params![low_value_cutoff],
            ).map_err(|e| format!("auto_forget_run low-value: {}", e))? as i64
        } else { 0 };

        Ok(AutoForgetReport {
            dry_run: dry,
            ttl_expired: if dry { ttl_count } else { ttl_deleted },
            low_value:   if dry { low_count } else { low_deleted },
            total_deleted: if dry { ttl_count + low_count } else { ttl_deleted + low_deleted },
            now_epoch: now,
        })
    })
}

/// Atomic consolidation: delete a list of memories AND insert a new one in
/// the same transaction. Either all succeed or nothing changes — Lucy can
/// fold N memories into 1 without ever leaving the DB in a half-state.
///
/// `delete_ids` — comma-separated list of ids to drop (e.g. "10,11,12,13")
/// `new_title` / `new_content` / `new_tags` — payload for the replacement.
/// Returns the new id.
#[tauri::command]
pub fn consolidate_agent_memories(
    delete_ids:  String,
    new_title:   String,
    new_content: String,
    new_tags:    Option<String>,
    importance:  Option<i64>,
) -> Result<i64, String> {
    // Parse + validate the id list before touching the DB.
    let ids: Vec<i64> = delete_ids
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .filter(|&n| n > 0)
        .collect();
    if ids.is_empty() {
        return Err("consolidate_agent_memories: no valid ids in delete_ids".to_string());
    }
    if new_title.trim().is_empty() || new_content.trim().is_empty() {
        return Err("consolidate_agent_memories: new_title and new_content required".to_string());
    }
    let imp = importance.unwrap_or(2).clamp(1, 3);
    let tags = new_tags.unwrap_or_else(|| "[\"consolidated\"]".to_string());

    with_db(|conn| {
        // We can't use rusqlite::Transaction here because with_db borrows
        // &Connection (not &mut). Manual BEGIN/COMMIT/ROLLBACK keeps the
        // same atomicity guarantees while staying within the existing
        // shared-connection contract.
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| format!("consolidate begin: {}", e))?;

        let inner = || -> Result<(usize, i64), String> {
            // Build a parameterized IN clause — never interpolate ids directly.
            let placeholders = (1..=ids.len()).map(|i| format!("?{}", i)).collect::<Vec<_>>().join(",");
            let del_sql = format!("DELETE FROM agent_memories WHERE id IN ({})", placeholders);
            let params_vec: Vec<rusqlite::types::Value> =
                ids.iter().map(|&i| rusqlite::types::Value::from(i)).collect();
            let params: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
            let deleted = conn.execute(&del_sql, params.as_slice())
                .map_err(|e| format!("consolidate delete: {}", e))?;

            conn.execute(
                "INSERT INTO agent_memories (session_id, title, content, tags, files, importance)
                 VALUES ('', ?1, ?2, ?3, '[]', ?4)",
                rusqlite::params![new_title, new_content, tags, imp],
            ).map_err(|e| format!("consolidate insert: {}", e))?;
            Ok((deleted, conn.last_insert_rowid()))
        };

        match inner() {
            Ok((deleted, new_id)) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| format!("consolidate commit: {}", e))?;
                crate::utils::logging::write_app_log(
                    "INFO",
                    &format!("consolidate_agent_memories: dropped {} ids → new memory id {}", deleted, new_id),
                );
                Ok(new_id)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    })
}

/// v1.7.233 — Real corpus-level counts for the Memory Browser stat cards.
/// Before this, both memory UIs computed "totals" over the ≤50 fetched rows,
/// so after a big PDF ingest every card lied ("50 memorias / 0 alta").
#[derive(Debug, Clone, Serialize)]
pub struct MemoryBrowserStats {
    pub total: i64,       // live real memories (non-pdf, non-superseded)
    pub high: i64,        // importance >= 3
    pub tagged: i64,      // tags present (not '[]' / '')
    pub week: i64,        // created in the last 7 days
    pub superseded: i64,  // rows hidden by dedup/consolidation
    pub pdf_chunks: i64,  // document chunk rows (session_id 'pdf:%')
    pub pdf_docs: i64,    // ingested documents
}

// Named memory_browser_stats — memory.rs already owns `memory_stats`
// (core/working-memory tiers introspection, a different subsystem).
#[tauri::command]
pub fn memory_browser_stats() -> Result<MemoryBrowserStats, String> {
    with_db(|conn| {
        let one = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> i64 {
            conn.query_row(sql, params, |r| r.get::<_, i64>(0)).unwrap_or(0)
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64).unwrap_or(0);
        let week_cutoff = now - 7 * 86_400;
        const LIVE: &str = "(superseded_by IS NULL OR superseded_by = '') AND session_id NOT LIKE 'pdf:%'";
        Ok(MemoryBrowserStats {
            total:      one(&format!("SELECT COUNT(*) FROM agent_memories WHERE {LIVE}"), &[]),
            high:       one(&format!("SELECT COUNT(*) FROM agent_memories WHERE {LIVE} AND importance >= 3"), &[]),
            tagged:     one(&format!("SELECT COUNT(*) FROM agent_memories WHERE {LIVE} AND tags != '' AND tags != '[]'"), &[]),
            week:       one(&format!("SELECT COUNT(*) FROM agent_memories WHERE {LIVE} AND created_at >= {week_cutoff}"), &[]),
            superseded: one("SELECT COUNT(*) FROM agent_memories WHERE superseded_by IS NOT NULL AND superseded_by != ''", &[]),
            pdf_chunks: one("SELECT COUNT(*) FROM agent_memories WHERE session_id LIKE 'pdf:%'", &[]),
            pdf_docs:   one("SELECT COUNT(*) FROM pdf_documents", &[]),
        })
    })
}

/// Return the most recent memories ordered by importance then date.
/// v1.7.233: optional `importance` pushes the filter into SQL — the browser
/// used to filter client-side AFTER the 50-row fetch, silently hiding rows.
#[tauri::command]
pub fn get_recent_memories_filtered(limit: Option<i64>, importance: Option<i64>) -> Result<Vec<AgentMemory>, String> {
    with_db(|conn| {
        let lim = limit.unwrap_or(15).clamp(1, 50);
        let imp = importance.unwrap_or(0);
        let sql = format!(
            "SELECT id, session_id, title, content, tags, files, importance, created_at
             FROM agent_memories
             WHERE (superseded_by IS NULL OR superseded_by = '')
               AND session_id NOT LIKE 'pdf:%'
               {}
             ORDER BY importance DESC, created_at DESC
             LIMIT ?1",
            if imp > 0 { format!("AND importance = {}", imp.clamp(1, 3)) } else { String::new() }
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("get_recent_filtered prepare: {}", e))?;
        map_memory_rows(&mut stmt, rusqlite::params![lim])
    })
}

/// Return the most recent memories ordered by importance then date.
///
/// v1.8 — the body now lives in `lucy-core`, the Tauri-free crate the native
/// shell also links, so both read the SAME rows through the SAME query.
///
/// It was already a copy that had drifted: the core capped the limit at 300
/// where this capped at 50, and the core's doc comment claimed they were "la
/// MISMA consulta". Nothing had caught it because nothing called the core's
/// version yet — the divergence was sitting there waiting for the first person
/// to trust it. The two exclusions in that WHERE (superseded rows, PDF chunks)
/// each cost a real bug to discover; having them written twice is how one copy
/// silently loses one.
#[tauri::command]
pub fn get_recent_memories(limit: Option<i64>) -> Result<Vec<AgentMemory>, String> {
    lucy_core::get_recent_memories(limit)
}

/// v1.7.236 R2 — refuerzo por citas. Tras cada respuesta, el frontend parsea
/// los marcadores [§id] que Lucy CITÓ de verdad y bombea sus contadores de
/// acceso. El decay estilo Mem0 de search_agent_memories hace el resto: lo
/// citado sube, lo inyectado-e-ignorado decae con el tiempo. Cap 50 ids.
#[tauri::command]
pub fn touch_memories_by_ids(ids: Vec<i64>) -> Result<usize, String> {
    if ids.is_empty() {
        return Ok(0);
    }
    let ids: Vec<i64> = ids.into_iter().take(50).collect();
    with_db(move |conn| {
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "UPDATE agent_memories
                SET access_count = access_count + 1,
                    last_accessed_at = strftime('%s','now')
              WHERE id IN ({placeholders})"
        );
        conn.execute(&sql, rusqlite::params_from_iter(ids.iter()))
            .map_err(|e| format!("touch by ids: {}", e))
    })
}

/// v1.7.236 R5 — fija/desfija una memoria (pin del operador).
#[tauri::command]
pub fn set_memory_pinned(id: i64, pinned: bool) -> Result<(), String> {
    with_db(move |conn| {
        conn.execute(
            "UPDATE agent_memories SET pinned = ?1 WHERE id = ?2",
            rusqlite::params![pinned as i64, id],
        )
        .map(|_| ())
        .map_err(|e| format!("set pinned: {}", e))
    })
}

/// v1.7.236 R5 — memorias fijadas para inyección incondicional. Mismo shape
/// que get_recent_memories (reusa map_memory_rows); cap duro de 10 para que
/// un exceso de pines no infle el contexto — el frontend inyecta ≤5.
#[tauri::command]
pub fn get_pinned_memories(limit: Option<i64>) -> Result<Vec<AgentMemory>, String> {
    with_db(|conn| {
        let lim = limit.unwrap_or(5).clamp(1, 10);
        let sql = "SELECT id, session_id, title, content, tags, files, importance, created_at
                   FROM agent_memories
                   WHERE pinned = 1
                     AND (superseded_by IS NULL OR superseded_by = '')
                     AND session_id NOT LIKE 'pdf:%'
                   ORDER BY importance DESC, created_at DESC
                   LIMIT ?1";
        let mut stmt = conn.prepare(sql).map_err(|e| format!("pinned prepare: {}", e))?;
        map_memory_rows(&mut stmt, rusqlite::params![lim])
    })
}

fn map_memory_rows(
    stmt: &mut rusqlite::Statement<'_>,
    params: impl rusqlite::Params,
) -> Result<Vec<AgentMemory>, String> {
    let rows = stmt.query_map(params, |row| {
        Ok(AgentMemory {
            id:         row.get(0)?,
            session_id: row.get(1)?,
            title:      row.get(2)?,
            content:    row.get(3)?,
            tags:       row.get(4)?,
            files:      row.get(5)?,
            importance: row.get(6)?,
            created_at: row.get(7)?,
        })
    }).map_err(|e| format!("query_map: {}", e))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("collect: {}", e))
}

// ══════════════════════════════════════════════════════════════════════════════
// USER PROFILE — persistent facts about the user (Hermes-style)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub key: String,
    pub value: String,
    pub category: String,
    pub updated_at: i64,
}

/// Upsert a profile key. Category defaults to "general".
/// Keys are user-supplied but should be slug-like (e.g. "preferred_shell",
/// "default_domain", "host:prod-db:role"). We don't enforce a schema because
/// the AI may discover new fact types we haven't anticipated.
#[tauri::command]
pub fn set_user_profile(key: String, value: String, category: Option<String>) -> Result<(), String> {
    let cat = category.unwrap_or_else(|| "general".to_string());
    with_db(|conn| {
        conn.execute(
            "INSERT INTO user_profile (key, value, category, updated_at)
             VALUES (?1, ?2, ?3, strftime('%s','now'))
             ON CONFLICT(key) DO UPDATE SET
                 value      = excluded.value,
                 category   = excluded.category,
                 updated_at = excluded.updated_at",
            rusqlite::params![key, value, cat],
        ).map_err(|e| format!("set_user_profile: {}", e))?;
        Ok(())
    })
}

#[tauri::command]
pub fn get_user_profile() -> Result<Vec<ProfileEntry>, String> {
    with_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT key, value, category, updated_at FROM user_profile
             ORDER BY category, key"
        ).map_err(|e| format!("profile prepare: {}", e))?;
        let rows = stmt.query_map([], |row| {
            Ok(ProfileEntry {
                key: row.get(0)?,
                value: row.get(1)?,
                category: row.get(2)?,
                updated_at: row.get(3)?,
            })
        }).map_err(|e| format!("profile query: {}", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("profile collect: {}", e))
    })
}

#[tauri::command]
pub fn delete_user_profile(key: String) -> Result<(), String> {
    with_db(|conn| {
        conn.execute(
            "DELETE FROM user_profile WHERE key = ?1",
            rusqlite::params![key],
        ).map_err(|e| format!("delete_user_profile: {}", e))?;
        Ok(())
    })
}

/// Build a compact context block ready to be concatenated into the system
/// prompt. Format is intentionally terse — Lucy pays for every token in
/// this block on every turn. Groups entries by category; skips stale facts
/// older than 180 days (profile info becomes lies otherwise).
///
/// Also appends the top 5 high-importance memories for continuity.
/// Returns an empty string when profile + memories are both empty so the
/// caller can safely `.concat()` without special-casing.
#[tauri::command]
pub fn build_profile_context() -> Result<String, String> {
    const STALE_SECS: i64 = 180 * 24 * 3600;
    let profile = get_user_profile()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64).unwrap_or(0);

    let mut out = String::new();
    if !profile.is_empty() {
        out.push_str("## USER PROFILE (facts Lucy has learned about this user)\n");
        let mut last_cat = String::new();
        for p in &profile {
            if now - p.updated_at > STALE_SECS { continue; }
            if p.category != last_cat {
                out.push_str(&format!("\n### {}\n", p.category));
                last_cat = p.category.clone();
            }
            out.push_str(&format!("- {}: {}\n", p.key, p.value));
        }
    }

    // Append top memories (importance DESC, recent first)
    let mems = get_recent_memories(Some(5)).unwrap_or_default();
    if !mems.is_empty() {
        out.push_str("\n## RELEVANT MEMORIES FROM PAST SESSIONS\n");
        for m in mems {
            // Trim each memory content to 200 chars — avoids bloating the prompt
            let trimmed = if m.content.len() > 200 {
                format!("{}…", crate::utils::safe_truncate(&m.content, 200))
            } else {
                m.content.clone()
            };
            out.push_str(&format!("- [{}] {}: {}\n", m.importance, m.title, trimmed));
        }
    }

    Ok(out)
}

// ══════════════════════════════════════════════════════════════════════════════
// CONVERSATION HISTORY — /recall (Hermes-inspired cross-session search)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub id: i64,
    pub tab_id: String,
    pub tab_title: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

/// Persist a visible conversation turn. Called fire-and-forget from the
/// frontend on every user/lucy/system message. Content is truncated to
/// 32KB to protect the DB from giant tool outputs occasionally appearing
/// in chat. Silently drops empty content.
#[tauri::command]
pub fn save_conversation_turn(
    tab_id: String,
    tab_title: String,
    role: String,
    content: String,
) -> Result<(), String> {
    const MAX_CONTENT: usize = 32 * 1024;
    let trimmed = content.trim();
    if trimmed.is_empty() { return Ok(()); }
    let clipped: String = if trimmed.len() > MAX_CONTENT {
        format!("{}…[truncated]", crate::utils::safe_truncate(trimmed, MAX_CONTENT))
    } else {
        trimmed.to_string()
    };
    with_db(|conn| {
        conn.execute(
            "INSERT INTO conversation_turns (tab_id, tab_title, role, content)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![tab_id, tab_title, role, clipped],
        ).map_err(|e| format!("save_conversation_turn: {}", e))?;
        Ok(())
    })
}

/// Full-text search across conversation history. Each whitespace-separated
/// term becomes a prefix match (`word*`) OR'd together — this matches
/// Hermes' FTS behavior and tolerates sysadmin-style queries ("iis reset
/// prod"). Returns up to `limit` turns, newest first when ranks tie.
#[tauri::command]
pub fn recall_conversations(query: String, limit: Option<i64>) -> Result<Vec<ConversationTurn>, String> {
    let lim = limit.unwrap_or(15).clamp(1, 50);
    let safe_q: String = query
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ");

    with_db(|conn| {
        if safe_q.is_empty() {
            // Empty query → most recent turns
            let mut stmt = conn.prepare(
                "SELECT id, tab_id, tab_title, role, content, created_at
                 FROM conversation_turns ORDER BY created_at DESC LIMIT ?1"
            ).map_err(|e| format!("recall prepare: {}", e))?;
            map_turn_rows(&mut stmt, rusqlite::params![lim])
        } else {
            let mut stmt = conn.prepare(
                "SELECT ct.id, ct.tab_id, ct.tab_title, ct.role, ct.content, ct.created_at
                 FROM conversation_turns ct
                 JOIN conversation_turns_fts fts ON ct.id = fts.rowid
                 WHERE conversation_turns_fts MATCH ?1
                 ORDER BY rank, ct.created_at DESC
                 LIMIT ?2"
            ).map_err(|e| format!("recall FTS prepare: {}", e))?;
            map_turn_rows(&mut stmt, rusqlite::params![safe_q, lim])
        }
    })
}

fn map_turn_rows(
    stmt: &mut rusqlite::Statement<'_>,
    params: impl rusqlite::Params,
) -> Result<Vec<ConversationTurn>, String> {
    let rows = stmt.query_map(params, |row| {
        Ok(ConversationTurn {
            id:         row.get(0)?,
            tab_id:     row.get(1)?,
            tab_title:  row.get(2)?,
            role:       row.get(3)?,
            content:    row.get(4)?,
            created_at: row.get(5)?,
        })
    }).map_err(|e| format!("recall query: {}", e))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("recall collect: {}", e))
}

// ── Quality Telemetry (opus-4-7 Tier 2.A — raw event log only) ────────────
// Raw event logger. No pre-baked summary command — queries against
// task_events are done ad-hoc once real data accumulates, so we avoid
// shipping metrics that conflate distinct failure modes or that rely on
// self-reported signals. Kept intentionally minimal.

/// Log a quality telemetry event. Non-blocking best-effort — failure is swallowed.
#[tauri::command]
pub async fn log_task_event(
    event_type: String,
    subtype: Option<String>,
    elapsed_ms: Option<i64>,
    metadata: Option<String>,
    tab_id: Option<String>,
) -> Result<(), String> {
    let id = generate_id();
    with_db(|conn| {
        conn.execute(
            "INSERT INTO task_events (id, tab_id, event_type, subtype, elapsed_ms, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![&id, &tab_id, &event_type, &subtype, &elapsed_ms, &metadata],
        ).map_err(|e| format!("insert task_event: {}", e))?;
        Ok(())
    })
}

// ── Task Telemetry Queries (Phase 4 — Quality Dashboard) ────────────────────

/// Summary of task events for the telemetry dashboard.
/// Groups events by type, returns counts and avg elapsed time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySummary {
    pub event_type: String,
    pub count: i64,
    pub avg_elapsed_ms: Option<f64>,
    pub last_ts: i64,
}

/// Distribution bucket for confidence levels or other categorical breakdowns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryBucket {
    pub label: String,
    pub count: i64,
}

/// One row of agent-loop-block stats: which model triggered which kind of
/// block, and how often, in the requested time window.
///
/// Used by `/loop-stats` and the quality dashboard to answer questions like
/// "is Gemini Flash hitting target-loops more than Gemini Pro?". Drives
/// model-selection decisions: if a model trips the safety nets repeatedly
/// on a particular task type, the user knows to switch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopBlockStat {
    pub model:         String,
    pub event_subtype: String,   // 'tool_loop' | 'target_loop' | 'error_repeat' | 'max_loops'
    pub count:         i64,
    pub last_ts:       i64,
}

/// Aggregated loop-block events grouped by (model, subtype). Filters to the
/// last `days` of data (default 30, max 365). Returns rows sorted so the
/// "loudest" model+kind combos surface first.
///
/// Relies on the existing task_events table — agent_loop_event_log writes
/// rows with event_type='agent_loop_block' and metadata.model populated.
#[tauri::command]
pub async fn loop_block_stats(days: Option<i64>) -> Result<Vec<LoopBlockStat>, String> {
    let days = days.unwrap_or(30).clamp(1, 365);
    let cutoff = chrono::Utc::now().timestamp() - days * 86400;
    with_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT
                COALESCE(json_extract(metadata, '$.model'), 'unknown') AS model,
                COALESCE(subtype, 'unknown')                          AS event_subtype,
                COUNT(*)                                              AS count,
                MAX(timestamp)                                        AS last_ts
             FROM task_events
             WHERE event_type = 'agent_loop_block'
               AND timestamp >= ?1
             GROUP BY model, event_subtype
             ORDER BY count DESC, model ASC"
        ).map_err(|e| format!("loop_block_stats prepare: {}", e))?;
        let rows: Vec<LoopBlockStat> = stmt
            .query_map(rusqlite::params![cutoff], |r| {
                Ok(LoopBlockStat {
                    model:         r.get(0)?,
                    event_subtype: r.get(1)?,
                    count:         r.get(2)?,
                    last_ts:       r.get(3)?,
                })
            })
            .map_err(|e| format!("loop_block_stats query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

// ── v1.7.99 — D3: per-model latency time series ─────────────────────────
//
// Powers the LatencySparkline component in the StatusBar. Returns the
// last N task_events rows that carry both a non-null elapsed_ms AND a
// `model` field in metadata, ordered newest-first. Frontend bins them
// per-model and draws a sparkline.
//
// We deliberately don't filter by event_type — Lucy logs latency on
// several events (plan_dryrun, plan_execute, batch, rollback_*) and
// all of them contribute meaningful "how fast is the model right
// now" signal. The sparkline reads as "recent throughput", not "chat
// turn duration".

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelLatencyPoint {
    pub model:      String,
    pub elapsed_ms: i64,
    pub ts:         i64,
}

/// Pull the most recent latency samples per model. `limit` caps the
/// total row count (default 200, max 1000) — frontend further trims
/// per-model to a window of ~30 points for the sparkline.
#[tauri::command]
pub async fn recent_model_latencies(
    limit: Option<i64>,
) -> Result<Vec<ModelLatencyPoint>, String> {
    let limit = limit.unwrap_or(200).clamp(1, 1000);
    with_db(|conn| {
        // v1.7.101 Sprint-1 B2 fix: `task_events.timestamp` is already an
        // INTEGER epoch column (see init schema). Wrapping it in
        // `strftime('%s', timestamp)` treats the INT as a Julian Day
        // number, so the v1.7.99 sparkline was plotting garbage timestamps.
        // Returning the column directly is both correct AND lets the query
        // planner use the existing index on `timestamp`.
        let mut stmt = conn.prepare(
            "SELECT
                COALESCE(json_extract(metadata, '$.model'), 'unknown') AS model,
                elapsed_ms,
                timestamp                                              AS ts
             FROM task_events
             WHERE elapsed_ms IS NOT NULL
               AND elapsed_ms > 0
               AND json_extract(metadata, '$.model') IS NOT NULL
             ORDER BY timestamp DESC
             LIMIT ?1"
        ).map_err(|e| format!("recent_model_latencies prepare: {}", e))?;
        let rows: Vec<ModelLatencyPoint> = stmt
            .query_map(rusqlite::params![limit], |r| {
                Ok(ModelLatencyPoint {
                    model:      r.get(0)?,
                    elapsed_ms: r.get(1)?,
                    ts:         r.get(2)?,
                })
            })
            .map_err(|e| format!("recent_model_latencies query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

/// Query aggregated telemetry data for the quality dashboard.
/// Returns event summaries grouped by type for the given period.
#[tauri::command]
pub async fn get_task_telemetry(
    period: Option<String>,
) -> Result<Vec<TelemetrySummary>, String> {
    // v1.7.101 Sprint-1 B3 fix: `task_events.timestamp` is INTEGER epoch.
    // The original `strftime('%s', datetime('now','-1 day'))` returns a
    // TEXT result which SQLite then coerces lexicographically when
    // compared with the INTEGER column — a real correctness bug, not just
    // a perf nit. Use `unixepoch(...)` so we get an INTEGER directly and
    // the comparison stays in the right type domain.
    let since_unix: &str = match period.as_deref() {
        Some("day")  => "unixepoch('now', '-1 day')",
        Some("week") => "unixepoch('now', '-7 days')",
        Some("all")  => "0",
        _            => "unixepoch('now', '-30 days')", // default: month
    };

    with_db(|conn| {
        let sql = format!(
            "SELECT event_type,
                    COUNT(*) as cnt,
                    AVG(elapsed_ms) as avg_ms,
                    MAX(timestamp) as last_ts
             FROM task_events
             WHERE timestamp >= {}
             GROUP BY event_type
             ORDER BY cnt DESC",
            since_unix
        );
        let mut stmt = conn.prepare(&sql)
            .map_err(|e| format!("prepare: {}", e))?;

        let rows = stmt.query_map([], |r| {
            Ok(TelemetrySummary {
                event_type: r.get(0)?,
                count: r.get(1)?,
                avg_elapsed_ms: r.get(2)?,
                last_ts: r.get::<_, i64>(3).unwrap_or(0),
            })
        }).map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();

        Ok(rows)
    })
}

/// Get confidence distribution breakdown (how many high/med/low confidence events).
#[tauri::command]
pub async fn get_confidence_distribution(
    period: Option<String>,
) -> Result<Vec<TelemetryBucket>, String> {
    let since = match period.as_deref() {
        Some("day") => "datetime('now', '-1 day')",
        Some("week") => "datetime('now', '-7 days')",
        Some("all") => "datetime('1970-01-01')",
        _ => "datetime('now', '-30 days')",
    };

    with_db(|conn| {
        let sql = format!(
            "SELECT COALESCE(subtype, 'unknown') as label,
                    COUNT(*) as cnt
             FROM task_events
             WHERE event_type = 'confidence'
               AND timestamp >= strftime('%s', {})
             GROUP BY subtype
             ORDER BY cnt DESC",
            since
        );
        let mut stmt = conn.prepare(&sql)
            .map_err(|e| format!("prepare: {}", e))?;

        let rows = stmt.query_map([], |r| {
            Ok(TelemetryBucket {
                label: r.get(0)?,
                count: r.get(1)?,
            })
        }).map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();

        Ok(rows)
    })
}

// ── Fork Results (Sprint 4 — Persistent Parallel Agents) ──────────────────
// Four commands cover the full lifecycle:
//   fork_save   — called immediately when a fork is launched (status='running')
//   fork_update — called when fork finishes or errors (sets result / error_msg)
//   fork_get    — retrieve a single fork by task_id (for wait_task resolution)
//   fork_list   — list all forks for a tab (for the ForksMonitorPanel)
//   fork_clear  — prune rows older than N days (housekeeping)

/// Persist a newly launched fork as 'running'. Returns the row id.
///
/// Tier A #1 — `parent_task_id` defaults to '' (root fork). Pass the
/// task_id of the spawning fork to build a tree.
#[tauri::command]
pub async fn fork_save(
    task_id: String,
    tab_id: String,
    session_id: String,
    model: String,
    instruction: String,
    parent_task_id: Option<String>,
) -> Result<String, String> {
    let id = generate_id();
    let parent = parent_task_id.unwrap_or_default();
    with_db(|conn| {
        conn.execute(
            "INSERT INTO fork_results
             (id, task_id, tab_id, session_id, model, instruction, status, parent_task_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running', ?7)",
            rusqlite::params![&id, &task_id, &tab_id, &session_id, &model, &instruction, &parent],
        ).map_err(|e| format!("fork_save: {}", e))?;
        Ok(id.clone())
    })
}

/// Mark a fork as done or error. Stores result text or error message.
///
/// Tier A #1 — also accepts optional `tokens_in` / `tokens_out`; the cost
/// is computed server-side via `calculate_cost(model, in, out)` so the
/// per-vendor pricing table stays the single source of truth.
#[tauri::command]
pub async fn fork_update(
    task_id: String,
    status: String,   // 'done' | 'error'
    result: Option<String>,
    error_msg: Option<String>,
    tokens_in: Option<i64>,
    tokens_out: Option<i64>,
) -> Result<(), String> {
    let t_in  = tokens_in.unwrap_or(0).max(0);
    let t_out = tokens_out.unwrap_or(0).max(0);
    with_db(|conn| {
        // Resolve the fork's model so calculate_cost picks the right tier.
        let model: String = conn.query_row(
            "SELECT model FROM fork_results WHERE task_id = ?1 ORDER BY created_at DESC LIMIT 1",
            rusqlite::params![&task_id],
            |r| r.get(0),
        ).unwrap_or_else(|_| String::new());
        let cost = if t_in > 0 || t_out > 0 {
            calculate_cost(&model, t_in.try_into().unwrap_or(0), t_out.try_into().unwrap_or(0))
        } else { 0.0 };
        conn.execute(
            "UPDATE fork_results
             SET status = ?1, result = ?2, error_msg = ?3,
                 tokens_in = ?4, tokens_out = ?5, cost_usd = ?6,
                 finished_at = strftime('%s','now')
             WHERE task_id = ?7 AND status = 'running'",
            rusqlite::params![&status, &result, &error_msg, t_in, t_out, cost, &task_id],
        ).map_err(|e| format!("fork_update: {}", e))?;
        Ok(())
    })
}

/// Retrieve a single fork by task_id (most recent first).
#[tauri::command]
pub async fn fork_get(task_id: String) -> Result<Option<ForkResult>, String> {
    with_db(|conn| {
        let r = conn.query_row(
            "SELECT id, task_id, tab_id, session_id, model, instruction,
                    status, result, error_msg, created_at, finished_at,
                    parent_task_id, tokens_in, tokens_out, cost_usd
             FROM fork_results WHERE task_id = ?1
             ORDER BY created_at DESC LIMIT 1",
            rusqlite::params![&task_id],
            |row| Ok(ForkResult {
                id:          row.get(0)?,
                task_id:     row.get(1)?,
                tab_id:      row.get(2)?,
                session_id:  row.get(3)?,
                model:       row.get(4)?,
                instruction: row.get(5)?,
                status:      row.get(6)?,
                result:      row.get(7)?,
                error_msg:   row.get(8)?,
                created_at:  row.get(9)?,
                finished_at: row.get(10)?,
                parent_task_id: row.get(11)?,
                tokens_in:   row.get(12)?,
                tokens_out:  row.get(13)?,
                cost_usd:    row.get(14)?,
            }),
        ).ok();
        Ok(r)
    })
}

/// List all forks for a given tab (newest first, max 100).
#[tauri::command]
pub async fn fork_list(
    tab_id: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<ForkResult>, String> {
    let lim = limit.unwrap_or(50) as i64;
    with_db(|conn| {
        // Inline mapper per branch — rusqlite's query_map consumes the FnMut
        // so we cannot share one closure across two branches.
        fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ForkResult> {
            Ok(ForkResult {
                id:          row.get(0)?,
                task_id:     row.get(1)?,
                tab_id:      row.get(2)?,
                session_id:  row.get(3)?,
                model:       row.get(4)?,
                instruction: row.get(5)?,
                status:      row.get(6)?,
                result:      row.get(7)?,
                error_msg:   row.get(8)?,
                created_at:  row.get(9)?,
                finished_at: row.get(10)?,
                parent_task_id: row.get(11)?,
                tokens_in:   row.get(12)?,
                tokens_out:  row.get(13)?,
                cost_usd:    row.get(14)?,
            })
        }
        let rows: Vec<ForkResult> = if let Some(ref tid) = tab_id {
            let mut stmt = conn.prepare(
                "SELECT id, task_id, tab_id, session_id, model, instruction,
                        status, result, error_msg, created_at, finished_at,
                        parent_task_id, tokens_in, tokens_out, cost_usd
                 FROM fork_results WHERE tab_id = ?1
                 ORDER BY created_at DESC LIMIT ?2"
            ).map_err(|e| format!("fork_list prepare (tab): {}", e))?;
            let v: Vec<ForkResult> = stmt.query_map(rusqlite::params![tid, lim], read_row)
                .map_err(|e| format!("fork_list query (tab): {}", e))?
                .filter_map(|r| r.ok()).collect();
            v
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, task_id, tab_id, session_id, model, instruction,
                        status, result, error_msg, created_at, finished_at,
                        parent_task_id, tokens_in, tokens_out, cost_usd
                 FROM fork_results
                 ORDER BY created_at DESC LIMIT ?1"
            ).map_err(|e| format!("fork_list prepare (all): {}", e))?;
            let v: Vec<ForkResult> = stmt.query_map(rusqlite::params![lim], read_row)
                .map_err(|e| format!("fork_list query (all): {}", e))?
                .filter_map(|r| r.ok()).collect();
            v
        };
        Ok(rows)
    })
}

/// Prune finished forks older than `days` (default 7). Returns deleted count.
#[tauri::command]
pub async fn fork_clear(days: Option<u32>) -> Result<u32, String> {
    let cutoff_days = days.unwrap_or(7) as i64;
    with_db(|conn| {
        let n = conn.execute(
            "DELETE FROM fork_results
             WHERE status != 'running'
               AND created_at < strftime('%s','now') - ?1 * 86400",
            rusqlite::params![cutoff_days],
        ).map_err(|e| format!("fork_clear: {}", e))?;
        Ok(n as u32)
    })
}

// ── Crystals (Tier 2 #4, agentmemory-inspired) ───────────────────────────
// A crystal is the LLM-distilled digest of a completed agent session:
//   • narrative      — 1-2 sentence summary of what was accomplished
//   • key_outcomes   — bullet-list of decisions/results
//   • files_affected — paths touched during the session
//   • lessons        — patterns or insights worth remembering for next time
//
// Crystals are recall gold for "how did I fix the WinRM cert issue three
// weeks ago?" — they survive normal memory eviction (importance is implicit:
// the user/agent explicitly chose to crystallize this session) and stay
// browsable in a dedicated UI panel.

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct AgentCrystal {
    pub id: i64,
    pub session_id: String,
    pub project: String,
    pub narrative: String,
    /// JSON array string of bullet outcomes
    pub key_outcomes: String,
    /// JSON array string of file paths
    pub files_affected: String,
    /// JSON array string of lesson strings
    pub lessons: String,
    pub source_chars: i64,
    pub created_at: i64,
}

/// XML the LLM must produce. Stricter than free-form JSON: we get
/// deterministic parsing and clean failure modes if the model emits prose.
const CRYSTALLIZE_SYSTEM: &str = "You are a memory crystallization engine for an autonomous SysAdmin assistant. \
Given a transcript of a completed agent session, distill it into a single durable memory.

Output EXACTLY this XML (no markdown, no commentary, no code fences):
<crystal>
  <narrative>1-2 sentence summary of what was accomplished, in past tense.</narrative>
  <outcomes>
    <outcome>Concrete result or decision (under 100 chars each)</outcome>
  </outcomes>
  <files>
    <file>path/to/file</file>
  </files>
  <lessons>
    <lesson>Reusable pattern or insight under 120 chars</lesson>
  </lessons>
</crystal>

Rules:
- 2-5 outcomes, 0-10 files, 1-4 lessons.
- Lessons must be generalisable (\"PowerShell ExecutionPolicy=Bypass needed for unsigned scripts\"), not session-specific (\"ran command at 3pm\").
- If files were edited or commands targeted specific paths, include them.
- Skip XML elements entirely if empty — DO NOT emit <outcome></outcome>.
- Output ONLY the <crystal> block. No preamble.";

/// Tiny XML helper — extracts the inner text of every `<tag>...</tag>`
/// match in `xml`. Handles nested whitespace, trims results, drops empties.
fn xml_collect(xml: &str, tag: &str) -> Vec<String> {
    let open  = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(s) = xml[cursor..].find(&open) {
        let start = cursor + s + open.len();
        let Some(e) = xml[start..].find(&close) else { break };
        let inner = xml[start..start + e].trim().to_string();
        if !inner.is_empty() {
            out.push(inner);
        }
        cursor = start + e + close.len();
    }
    out
}

fn xml_single(xml: &str, tag: &str) -> Option<String> {
    xml_collect(xml, tag).into_iter().next()
}

/// Crystallize a chunk of session transcript into a stored AgentCrystal.
/// The frontend assembles `transcript` from the live tab messages; we
/// don't try to read messages from disk (Lucy keeps them in-memory).
///
/// Returns the new crystal id. Fails fast if Ollama is unreachable or
/// emits unparseable output — crystallization is opt-in and rare, so
/// silent fallback isn't useful here (unlike search expansion).
#[tauri::command]
pub async fn crystallize_session(
    session_id: String,
    project:    Option<String>,
    transcript: String,
) -> Result<i64, String> {
    let t = transcript.trim();
    if t.is_empty() {
        return Err("crystallize_session: empty transcript".to_string());
    }
    // Cap input to a reasonable size — Ollama context is finite and
    // crystallization is a one-shot summarisation, not a deep read.
    const MAX_CHARS: usize = 24_000;
    let chunk = if t.chars().count() > MAX_CHARS {
        let truncated: String = t.chars().take(MAX_CHARS).collect();
        format!("{}\n\n[transcript truncated at {} chars]", truncated, MAX_CHARS)
    } else {
        t.to_string()
    };
    let source_chars = chunk.chars().count() as i64;

    // ── Call Ollama for the digest ──────────────────────────────────
    let base = crate::commands::embeddings::ollama_base();
    let body = serde_json::json!({
        "model": "qwen3:4b",
        "messages": [
            { "role": "system", "content": CRYSTALLIZE_SYSTEM },
            { "role": "user",   "content": format!("Session transcript:\n\n{}", chunk) },
        ],
        "stream": false,
        "options": { "temperature": 0.3, "num_predict": 1200 },
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .map_err(|e| format!("http client: {}", e))?;
    let resp = client
        .post(format!("{}/api/chat", base))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ollama unreachable: {}", e))?;
    let json: serde_json::Value = resp.json().await
        .map_err(|e| format!("Ollama JSON parse: {}", e))?;
    let xml = json
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    // ── Parse XML ────────────────────────────────────────────────────
    let narrative = xml_single(&xml, "narrative")
        .ok_or_else(|| format!("crystallize: missing <narrative> in LLM output: {}",
                               xml.chars().take(200).collect::<String>()))?;
    let outcomes = xml_collect(&xml, "outcome");
    let files    = xml_collect(&xml, "file");
    let lessons  = xml_collect(&xml, "lesson");

    let key_outcomes_json   = serde_json::to_string(&outcomes).unwrap_or_else(|_| "[]".to_string());
    let files_affected_json = serde_json::to_string(&files).unwrap_or_else(|_| "[]".to_string());
    let lessons_json        = serde_json::to_string(&lessons).unwrap_or_else(|_| "[]".to_string());
    let project_str         = project.unwrap_or_default();

    // ── Persist ──────────────────────────────────────────────────────
    let new_id = with_db(|conn| {
        conn.execute(
            "INSERT INTO agent_crystals \
             (session_id, project, narrative, key_outcomes, files_affected, lessons, source_chars) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                session_id, project_str, narrative,
                key_outcomes_json, files_affected_json, lessons_json,
                source_chars
            ],
        ).map_err(|e| format!("crystallize insert: {}", e))?;
        Ok(conn.last_insert_rowid())
    })?;

    // ── Spawn-off: also save each lesson as a normal agent_memory ───
    // Crystallization promotes lessons to first-class memories so they
    // surface in the regular search pipeline. Fire-and-forget — failures
    // here don't invalidate the crystal itself.
    //
    // v1.7.236 — CAP the promotion at MAX_PROMOTED_LESSONS. A runaway local
    // model can emit dozens of <lesson> tags; without a cap, one auto-
    // crystallized session could flood memory (the GoAnywhere failure mode).
    // A genuinely useful session distils a handful of durable lessons, not 40.
    const MAX_PROMOTED_LESSONS: usize = 6;
    for lesson in lessons.iter().take(MAX_PROMOTED_LESSONS) {
        let title = format!("Lesson: {}",
            lesson.chars().take(60).collect::<String>());
        let tags  = serde_json::to_string(&vec!["crystal", "lesson"])
            .unwrap_or_else(|_| "[]".to_string());
        let _ = save_agent_memory(
            title,
            lesson.clone(),
            Some(tags),
            None,
            Some(session_id.clone()),
            Some(2),       // importance bump — distilled lessons matter
            None,          // no TTL — keep crystallized wisdom forever
        ).await;
    }

    // v1.7.236 (#2 — crystals into the recall pipeline) — also promote the
    // crystal NARRATIVE as a searchable memory. Lessons capture the actionable
    // "how"; the narrative captures the "what we did / why", which is what
    // answers "have I dealt with this before?". Until now the narrative lived
    // only in `agent_crystals` (the cockpit viewer) and never reached recall.
    // Importance 1 so it's findable but never outranks a hard fact; deduped by
    // save_agent_memory's FTS pipeline; carries the same files list so a
    // file-scoped recall can surface it.
    {
        let ntitle = format!("Sesión: {}", narrative.chars().take(60).collect::<String>());
        let ntags = serde_json::to_string(&vec!["crystal", "session-summary"])
            .unwrap_or_else(|_| "[]".to_string());
        let _ = save_agent_memory(
            ntitle,
            narrative.clone(),
            Some(ntags),
            Some(files_affected_json.clone()),
            Some(session_id.clone()),
            Some(1),       // low importance — session context, not a hard fact
            None,          // keep — crystallized context is durable
        ).await;
    }

    Ok(new_id)
}

/// Outcome of an auto-crystallization attempt (returned to the frontend so it can
/// flash a subtle "sesión cristalizada" toast only when `fired`).
#[derive(Debug, Serialize, Deserialize)]
pub struct AutoCrystalResult {
    pub fired: bool,
    pub crystal_id: Option<i64>,
    pub reason: String,
}

/// Auto-crystallize a session at its end — the AUTONOMOUS counterpart to the
/// manual `/crystallize`. Lucy distils her own lessons without being asked, so
/// the user need not re-teach her (the point-4 autonomy goal).
///
/// STRICT GATING against the memory-flood failure mode (the GoAnywhere incident,
/// [[memory-system-overhaul]]). Every gate below is CHEAP and runs BEFORE any
/// Ollama call, so a thin or already-distilled session costs nothing:
///   • MIN_TURNS / MIN_TOOL_CALLS  — only sessions where real work happened.
///   • MIN_TRANSCRIPT_CHARS        — there must be substance to distil.
///   • one crystal per session     — idempotent; re-invoking each turn is a no-op.
/// The lesson promotion inside `crystallize_session` is itself capped
/// (MAX_PROMOTED_LESSONS) and deduped (save_agent_memory's FTS pipeline).
///
/// NEVER errors the caller: a failed gate or an Ollama-down box just returns
/// `fired = false` with a reason, so the agent loop is never disturbed.
#[tauri::command]
pub async fn maybe_auto_crystallize_session(
    session_id: String,
    project: Option<String>,
    transcript: String,
    turn_count: i64,
    tool_call_count: i64,
) -> Result<AutoCrystalResult, String> {
    const MIN_TURNS: i64 = 4;
    const MIN_TOOL_CALLS: i64 = 3;
    const MIN_TRANSCRIPT_CHARS: usize = 800;

    let skip = |reason: &str| {
        Ok(AutoCrystalResult { fired: false, crystal_id: None, reason: reason.to_string() })
    };

    if session_id.trim().is_empty() { return skip("no session id"); }
    if turn_count < MIN_TURNS { return skip("session too short (turns)"); }
    if tool_call_count < MIN_TOOL_CALLS { return skip("not enough tool activity"); }
    if transcript.trim().chars().count() < MIN_TRANSCRIPT_CHARS { return skip("transcript too thin"); }

    // Idempotent: at most ONE auto-crystal per session. Cheap DB check before we
    // ever wake Ollama, so the agent loop can call this on every turn-end freely.
    let already = with_db(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM agent_crystals WHERE session_id = ?1",
            rusqlite::params![session_id],
            |r| r.get::<_, i64>(0),
        ).map_err(|e| format!("auto-crystal existence check: {}", e))
    }).unwrap_or(0);
    if already > 0 { return skip("session already crystallized"); }

    // Gates passed — distil. crystallize_session is fail-fast on Ollama; we
    // downgrade its error to a non-firing result so the turn is never broken.
    match crystallize_session(session_id, project, transcript).await {
        Ok(id) => Ok(AutoCrystalResult { fired: true, crystal_id: Some(id), reason: "crystallized".to_string() }),
        Err(e) => skip(&format!("crystallize skipped: {}", e)),
    }
}

/// List recent crystals, newest first. Optional filters by session or project.
#[tauri::command]
pub fn list_crystals(
    session_id: Option<String>,
    project:    Option<String>,
    limit:      Option<i64>,
) -> Result<Vec<AgentCrystal>, String> {
    let lim = limit.unwrap_or(20).clamp(1, 100);
    with_db(|conn| {
        let (sql, params): (&str, Vec<Box<dyn rusqlite::ToSql>>) = match (session_id.as_deref(), project.as_deref()) {
            (Some(s), Some(p)) => (
                "SELECT id, session_id, project, narrative, key_outcomes, \
                        files_affected, lessons, source_chars, created_at \
                 FROM agent_crystals WHERE session_id = ?1 AND project = ?2 \
                 ORDER BY created_at DESC LIMIT ?3",
                vec![Box::new(s.to_string()), Box::new(p.to_string()), Box::new(lim)],
            ),
            (Some(s), None) => (
                "SELECT id, session_id, project, narrative, key_outcomes, \
                        files_affected, lessons, source_chars, created_at \
                 FROM agent_crystals WHERE session_id = ?1 \
                 ORDER BY created_at DESC LIMIT ?2",
                vec![Box::new(s.to_string()), Box::new(lim)],
            ),
            (None, Some(p)) => (
                "SELECT id, session_id, project, narrative, key_outcomes, \
                        files_affected, lessons, source_chars, created_at \
                 FROM agent_crystals WHERE project = ?1 \
                 ORDER BY created_at DESC LIMIT ?2",
                vec![Box::new(p.to_string()), Box::new(lim)],
            ),
            (None, None) => (
                "SELECT id, session_id, project, narrative, key_outcomes, \
                        files_affected, lessons, source_chars, created_at \
                 FROM agent_crystals \
                 ORDER BY created_at DESC LIMIT ?1",
                vec![Box::new(lim)],
            ),
        };
        let mut stmt = conn.prepare(sql).map_err(|e| format!("list_crystals prepare: {}", e))?;
        let p_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| &**b).collect();
        let rows = stmt.query_map(p_refs.as_slice(), |row| Ok(AgentCrystal {
            id:             row.get(0)?,
            session_id:     row.get(1)?,
            project:        row.get(2)?,
            narrative:      row.get(3)?,
            key_outcomes:   row.get(4)?,
            files_affected: row.get(5)?,
            lessons:        row.get(6)?,
            source_chars:   row.get(7)?,
            created_at:     row.get(8)?,
        })).map_err(|e| format!("list_crystals query: {}", e))?;
        let mut out = Vec::new();
        for c in rows.flatten() { out.push(c); }
        Ok(out)
    })
}

/// Fetch one crystal by id, or None if not found.
#[tauri::command]
pub fn get_crystal(id: i64) -> Result<Option<AgentCrystal>, String> {
    with_db(|conn| {
        let r: rusqlite::Result<AgentCrystal> = conn.query_row(
            "SELECT id, session_id, project, narrative, key_outcomes, \
                    files_affected, lessons, source_chars, created_at \
             FROM agent_crystals WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok(AgentCrystal {
                id:             row.get(0)?,
                session_id:     row.get(1)?,
                project:        row.get(2)?,
                narrative:      row.get(3)?,
                key_outcomes:   row.get(4)?,
                files_affected: row.get(5)?,
                lessons:        row.get(6)?,
                source_chars:   row.get(7)?,
                created_at:     row.get(8)?,
            }),
        );
        match r {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("get_crystal: {}", e)),
        }
    })
}

/// Delete a crystal by id. The associated lesson memories (saved by
/// crystallize_session) are NOT cascaded — they're independent rows in
/// agent_memories tagged with "crystal,lesson" and survive crystal deletion.
#[tauri::command]
pub fn delete_crystal(id: i64) -> Result<usize, String> {
    if id <= 0 { return Err("delete_crystal: invalid id".to_string()); }
    with_db(|conn| {
        let n = conn.execute(
            "DELETE FROM agent_crystals WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| format!("delete_crystal: {}", e))?;
        Ok(n)
    })
}

// ── Auto-consolidation (Tier 2 #5, agentmemory-inspired) ─────────────────
// Periodic background pass that fuses related agent_memories into a single
// higher-quality memory. The originals are kept (audit trail) but marked
// superseded — they no longer surface in default search results.
//
// Pipeline:
//   1. Pull eligible memories: importance == 1, NOT superseded, ≥ 7 days
//      old (give them time to be accessed before fusion), has ≥ 2 tags.
//   2. Cluster by Jaccard similarity on the tag set — two memories cluster
//      together if they share ≥ MIN_SHARED_TAGS tags. Connected components
//      give us groups; we keep clusters of size [MIN_SIZE..MAX_SIZE].
//   3. For each cluster, call Ollama with a strict XML prompt asking for a
//      single fused memory (title/content/tags/type). Insert the result.
//   4. Mark every source as superseded_by the new id.
//
// Conservative defaults — importance≥2 is never auto-consolidated (the
// user/agent explicitly flagged it as important); pinned & recent rows
// stay untouched. Dry-run mode reports cluster proposals without writing.

const CONSOLIDATE_MIN_AGE_DAYS:     i64   = 7;
const CONSOLIDATE_MIN_SHARED_TAGS:  usize = 2;
const CONSOLIDATE_MIN_CLUSTER:      usize = 3;
const CONSOLIDATE_MAX_CLUSTER:      usize = 12;
const CONSOLIDATE_MAX_CLUSTERS_RUN: usize = 5;   // Cap LLM calls per run
const CONSOLIDATE_OLLAMA_TIMEOUT_S: u64   = 30;

#[derive(serde::Serialize, Clone)]
pub struct ConsolidationCluster {
    pub source_ids: Vec<i64>,
    pub shared_tags: Vec<String>,
    /// Only populated when dry_run=false AND the LLM call succeeded.
    pub new_memory_id: Option<i64>,
    pub new_title: Option<String>,
}

#[derive(serde::Serialize)]
pub struct AutoConsolidateReport {
    pub dry_run: bool,
    pub eligible_memories: i64,
    pub clusters_found: i64,
    pub clusters_processed: i64,
    pub memories_superseded: i64,
    pub new_memories: i64,
    pub clusters: Vec<ConsolidationCluster>,
}

/// Parse the JSON tag array stored in agent_memories.tags. Returns an
/// empty Vec on any malformed/empty input.
fn parse_tags(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw)
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !t.trim().is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

/// Union-Find for cluster building. Tiny, all-in-one, no external dep.
struct UnionFind { parent: Vec<usize> }
impl UnionFind {
    fn new(n: usize) -> Self { UnionFind { parent: (0..n).collect() } }
    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }
    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb { self.parent[ra] = rb; }
    }
}

const CONSOLIDATE_SYSTEM: &str = "You are a memory consolidation engine for an autonomous SysAdmin assistant. \
Given a set of related observations, distill them into ONE durable memory.

Output EXACTLY this XML (no markdown, no preamble):
<memory>
  <type>pattern|preference|architecture|bug|workflow|fact</type>
  <title>Concise title (max 80 chars)</title>
  <content>2-4 sentence synthesis. Capture the COMMON insight; drop one-off details.</content>
  <tags>
    <tag>lowercase-keyword</tag>
  </tags>
</memory>

Rules:
- Pick the SHARED essence — don't list every observation. If they share a root cause, name it.
- 2-6 tags. Reuse the inputs' tags where they fit; add 1-2 new abstract ones if useful.
- type=fact for static knowledge, pattern for recurring behaviour, workflow for procedures.
- Output ONLY the <memory> block.";

/// XML helpers (shared with crystallize).
fn xml_single_local(xml: &str, tag: &str) -> Option<String> {
    let open  = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let s = xml.find(&open)? + open.len();
    let e = xml[s..].find(&close)?;
    Some(xml[s..s + e].trim().to_string())
}
fn xml_collect_local(xml: &str, tag: &str) -> Vec<String> {
    let open  = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(s) = xml[cursor..].find(&open) {
        let start = cursor + s + open.len();
        let Some(e) = xml[start..].find(&close) else { break };
        let inner = xml[start..start + e].trim().to_string();
        if !inner.is_empty() { out.push(inner); }
        cursor = start + e + close.len();
    }
    out
}

/// Run a consolidation sweep. `dry_run = true` reports cluster proposals
/// without calling Ollama or writing anything. Idempotent — safe to call
/// repeatedly and via a 24h background scheduler.
#[tauri::command]
pub async fn auto_consolidate_run(
    dry_run: Option<bool>,
) -> Result<AutoConsolidateReport, String> {
    let dry = dry_run.unwrap_or(false);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let age_cutoff = now - (CONSOLIDATE_MIN_AGE_DAYS * 86_400);

    // ── 1. Pull eligible memories (read-only) ────────────────────────
    #[derive(Clone)]
    struct Eligible { id: i64, title: String, content: String, tags: Vec<String> }
    let eligibles: Vec<Eligible> = with_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, title, content, tags FROM agent_memories
             WHERE importance = 1
               AND superseded_by IS NULL
               AND expires_at = 0
               AND created_at < ?1
             ORDER BY created_at ASC
             LIMIT 500"  // cap; very old DBs would otherwise stall the sweep
        ).map_err(|e| format!("consolidate prepare: {}", e))?;
        let rows = stmt.query_map(rusqlite::params![age_cutoff], |row| {
            Ok(Eligible {
                id:      row.get(0)?,
                title:   row.get(1)?,
                content: row.get(2)?,
                tags:    parse_tags(&row.get::<_, String>(3)?),
            })
        }).map_err(|e| format!("consolidate query: {}", e))?;
        let mut out = Vec::new();
        for r in rows { if let Ok(e) = r {
            // Drop memories without tags — can't cluster them
            if e.tags.len() >= 2 { out.push(e); }
        }}
        Ok(out)
    })?;

    let eligible_count = eligibles.len() as i64;
    if eligibles.len() < CONSOLIDATE_MIN_CLUSTER {
        return Ok(AutoConsolidateReport {
            dry_run: dry,
            eligible_memories: eligible_count,
            clusters_found: 0,
            clusters_processed: 0,
            memories_superseded: 0,
            new_memories: 0,
            clusters: Vec::new(),
        });
    }

    // ── 2. Cluster by shared-tag overlap (Union-Find) ────────────────
    // O(N²) pairwise — acceptable up to ~500 memories, our hard cap.
    let n = eligibles.len();
    let mut uf = UnionFind::new(n);
    use std::collections::HashSet;
    let tag_sets: Vec<HashSet<&str>> = eligibles.iter()
        .map(|e| e.tags.iter().map(|s| s.as_str()).collect())
        .collect();
    for i in 0..n {
        for j in (i + 1)..n {
            let shared = tag_sets[i].intersection(&tag_sets[j]).count();
            if shared >= CONSOLIDATE_MIN_SHARED_TAGS {
                uf.union(i, j);
            }
        }
    }
    // Materialize clusters
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        let root = uf.find(i);
        groups.entry(root).or_default().push(i);
    }
    // Keep clusters within size bounds, sort by size DESC (largest first)
    let mut viable: Vec<Vec<usize>> = groups.into_values()
        .filter(|g| g.len() >= CONSOLIDATE_MIN_CLUSTER && g.len() <= CONSOLIDATE_MAX_CLUSTER)
        .collect();
    viable.sort_by(|a, b| b.len().cmp(&a.len()));
    let clusters_found = viable.len() as i64;

    // Cap total Ollama calls per run
    viable.truncate(CONSOLIDATE_MAX_CLUSTERS_RUN);

    // ── 3. (dry-run shortcut) Just report proposals ──────────────────
    if dry {
        let clusters: Vec<ConsolidationCluster> = viable.iter().map(|idxs| {
            let source_ids: Vec<i64> = idxs.iter().map(|&i| eligibles[i].id).collect();
            // Shared tags = intersection across the cluster
            let mut shared: HashSet<&str> = tag_sets[idxs[0]].clone();
            for &i in idxs.iter().skip(1) {
                shared = shared.intersection(&tag_sets[i]).copied().collect();
            }
            ConsolidationCluster {
                source_ids,
                shared_tags: shared.into_iter().map(|s| s.to_string()).collect(),
                new_memory_id: None,
                new_title: None,
            }
        }).collect();
        return Ok(AutoConsolidateReport {
            dry_run: true,
            eligible_memories: eligible_count,
            clusters_found,
            clusters_processed: viable.len() as i64,
            memories_superseded: 0,
            new_memories: 0,
            clusters,
        });
    }

    // ── 4. Real run: per-cluster LLM + insert + supersede ────────────
    let base = crate::commands::embeddings::ollama_base();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(CONSOLIDATE_OLLAMA_TIMEOUT_S))
        .build()
        .map_err(|e| format!("http client: {}", e))?;

    let mut total_superseded = 0i64;
    let mut total_new = 0i64;
    let mut report_clusters: Vec<ConsolidationCluster> = Vec::with_capacity(viable.len());
    let mut processed = 0i64;

    for idxs in &viable {
        let cluster_members: Vec<&Eligible> = idxs.iter().map(|&i| &eligibles[i]).collect();
        let source_ids: Vec<i64> = cluster_members.iter().map(|e| e.id).collect();
        let mut shared: HashSet<&str> = tag_sets[idxs[0]].clone();
        for &i in idxs.iter().skip(1) {
            shared = shared.intersection(&tag_sets[i]).copied().collect();
        }
        let shared_tags_vec: Vec<String> = shared.iter().map(|s| s.to_string()).collect();

        // Build the user prompt: each memory as a numbered block
        let mut user_prompt = String::with_capacity(2048);
        user_prompt.push_str("Observations:\n\n");
        for (i, m) in cluster_members.iter().enumerate() {
            let snippet: String = m.content.chars().take(400).collect();
            user_prompt.push_str(&format!(
                "{}. {} — {}\n   tags: {}\n\n",
                i + 1,
                m.title.chars().take(80).collect::<String>(),
                snippet,
                m.tags.join(", ")
            ));
        }

        let body = serde_json::json!({
            "model": "qwen3:4b",
            "messages": [
                { "role": "system", "content": CONSOLIDATE_SYSTEM },
                { "role": "user",   "content": user_prompt },
            ],
            "stream": false,
            "options": { "temperature": 0.3, "num_predict": 600 },
        });

        let resp = match client
            .post(format!("{}/api/chat", base))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => {
                // Network failure for THIS cluster — log a placeholder and move on
                report_clusters.push(ConsolidationCluster {
                    source_ids: source_ids.clone(),
                    shared_tags: shared_tags_vec.clone(),
                    new_memory_id: None,
                    new_title: Some("(Ollama unreachable)".to_string()),
                });
                continue;
            }
        };
        let json: serde_json::Value = match resp.json().await {
            Ok(j) => j, Err(_) => continue,
        };
        let xml = json
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let title = match xml_single_local(&xml, "title") {
            Some(t) => t, None => continue,  // skip unparseable
        };
        let content = match xml_single_local(&xml, "content") {
            Some(c) => c, None => continue,
        };
        let mem_type = xml_single_local(&xml, "type").unwrap_or_else(|| "pattern".to_string());
        let mut tags = xml_collect_local(&xml, "tag");
        if tags.is_empty() {
            tags = shared_tags_vec.clone();
        }
        // Always tag the result so consolidated rows are filterable
        tags.push("consolidated".to_string());
        tags.push(format!("type:{}", mem_type));

        let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());

        // Insert + supersede in a single transaction
        let result_id: Result<i64, String> = with_db(|conn| {
            let tx = conn.unchecked_transaction()
                .map_err(|e| format!("consolidate tx: {}", e))?;
            tx.execute(
                "INSERT INTO agent_memories
                 (session_id, title, content, tags, files, importance, expires_at)
                 VALUES ('', ?1, ?2, ?3, '[]', 2, 0)",
                rusqlite::params![title, content, tags_json],
            ).map_err(|e| format!("consolidate insert: {}", e))?;
            let new_id = tx.last_insert_rowid();
            for sid in &source_ids {
                tx.execute(
                    "UPDATE agent_memories SET superseded_by = ?1 WHERE id = ?2",
                    rusqlite::params![new_id, sid],
                ).map_err(|e| format!("consolidate supersede: {}", e))?;
            }
            tx.commit().map_err(|e| format!("consolidate commit: {}", e))?;
            Ok(new_id)
        });

        match result_id {
            Ok(new_id) => {
                total_new += 1;
                total_superseded += source_ids.len() as i64;
                processed += 1;
                report_clusters.push(ConsolidationCluster {
                    source_ids,
                    shared_tags: shared_tags_vec,
                    new_memory_id: Some(new_id),
                    new_title: Some(title),
                });
            }
            Err(_) => {
                report_clusters.push(ConsolidationCluster {
                    source_ids,
                    shared_tags: shared_tags_vec,
                    new_memory_id: None,
                    new_title: Some("(insert failed)".to_string()),
                });
            }
        }
    }

    Ok(AutoConsolidateReport {
        dry_run: false,
        eligible_memories: eligible_count,
        clusters_found,
        clusters_processed: processed,
        memories_superseded: total_superseded,
        new_memories: total_new,
        clusters: report_clusters,
    })
}

// ── Reflection (Tier 3 #8, agentmemory-inspired) ─────────────────────────
// Periodic meta-observation pass. Unlike consolidate (which fuses sources
// into one replacement memory) and unlike crystallize (one-shot session
// digest), reflect produces RECURRENT insights:
//
//   "I've noticed the user prefers PowerShell over cmd for filesystem tasks"
//   "Most DNS issues on this estate trace back to AD replication lag"
//   "WinRM cert errors usually mean clock skew between client and server"
//
// Each run looks for clusters of related memories, generates a meta-
// insight via Ollama, then either:
//   • Creates a new agent_insights row (first occurrence), OR
//   • REINFORCES an existing insight whose fingerprint matches —
//     confidence += 0.1*(1-confidence) (asymptotic toward 1.0),
//     reinforcements += 1, last_reinforced_at = now.
//
// Fingerprint is a stable hash over lowercased+whitespace-collapsed
// content. Two runs that produce semantically-equivalent insights with
// slight wording differences will MISS each other and create duplicate
// rows — that's acceptable noise; the high-confidence ones surface first
// in /insights via the (confidence DESC, reinforcements DESC) index.

const REFLECT_MIN_AGE_DAYS:    i64   = 5;
const REFLECT_MIN_CLUSTER:     usize = 4;     // Higher bar than consolidate
const REFLECT_MAX_CLUSTER:     usize = 20;
const REFLECT_MIN_SHARED_TAGS: usize = 2;
const REFLECT_MAX_CLUSTERS:    usize = 4;
const REFLECT_OLLAMA_TIMEOUT_S: u64  = 35;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct AgentInsight {
    pub id: i64,
    pub content: String,
    pub fingerprint: String,
    pub confidence: f64,
    pub reinforcements: i64,
    /// JSON-encoded Vec<String> of concept keywords
    pub concepts: String,
    pub source_count: i64,
    pub last_reinforced_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(serde::Serialize)]
pub struct ReflectReport {
    pub dry_run: bool,
    pub eligible_memories: i64,
    pub clusters_processed: i64,
    pub insights_created: i64,
    pub insights_reinforced: i64,
}

const REFLECT_SYSTEM: &str = "You are a reflection engine for an autonomous SysAdmin assistant. \
Given a cluster of related observations the agent has accumulated, identify ONE generalisable META-insight \
— a pattern, preference, or rule of thumb that holds ACROSS the observations.

Output EXACTLY this XML (no markdown, no preamble):
<insight>
  <content>One concise sentence (under 200 chars) stating the meta-pattern.</content>
  <concepts>
    <concept>lowercase-keyword</concept>
  </concepts>
</insight>

Rules:
- The content MUST generalise — don't restate a single observation. If the
  observations don't share a generalisable insight, output an empty <content/>.
- 1-4 concepts that the insight is ABOUT (not literal tags from the inputs;
  abstract them: 'PowerShell' not 'pwsh', 'authentication' not 'auth-fail').
- Content style: assertive present-tense observation. \"Lucy prefers X over Y for Z.\"
  Or \"X errors usually trace to Y.\" Or \"X requires Y when Z.\"";

/// Stable fingerprint: lowercase + collapse whitespace + sha256 hex prefix.
/// Two insights with identical wording (modulo whitespace/case) collapse
/// to the same fingerprint and reinforce instead of duplicating.
fn insight_fingerprint(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let normalized: String = content
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut h = Sha256::new();
    h.update(normalized.as_bytes());
    let bytes = h.finalize();
    let mut s = String::with_capacity(32);
    for b in bytes.iter().take(16) {  // 16 bytes / 32 hex chars is plenty for dedup
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[tauri::command]
pub async fn reflect_run(dry_run: Option<bool>) -> Result<ReflectReport, String> {
    let dry = dry_run.unwrap_or(false);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let age_cutoff = now - (REFLECT_MIN_AGE_DAYS * 86_400);

    // 1. Pull eligible memories (read-only).
    #[derive(Clone)]
    struct Eligible { title: String, content: String, tags: Vec<String> }
    let eligibles: Vec<Eligible> = with_db(|conn| {
        // v1.7.233: exclude PDF chunks — after a large ingest the 400-newest
        // window was 100% manual chunks (one giant same-tag cluster that the
        // 4..20 size filter drops), starving insight generation for real
        // memories indefinitely.
        let mut stmt = conn.prepare(
            "SELECT title, content, tags FROM agent_memories
             WHERE superseded_by IS NULL
               AND expires_at = 0
               AND created_at < ?1
               AND session_id NOT LIKE 'pdf:%'
             ORDER BY created_at DESC
             LIMIT 400"
        ).map_err(|e| format!("reflect prepare: {}", e))?;
        let rows = stmt.query_map(rusqlite::params![age_cutoff], |row| {
            Ok(Eligible {
                title:   row.get(0)?,
                content: row.get(1)?,
                tags:    parse_tags(&row.get::<_, String>(2)?),
            })
        }).map_err(|e| format!("reflect query: {}", e))?;
        let mut out = Vec::new();
        for e in rows.flatten() {
            if e.tags.len() >= 2 { out.push(e); }
        }
        Ok(out)
    })?;

    let eligible_count = eligibles.len() as i64;
    if eligibles.len() < REFLECT_MIN_CLUSTER {
        return Ok(ReflectReport {
            dry_run: dry,
            eligible_memories: eligible_count,
            clusters_processed: 0,
            insights_created: 0,
            insights_reinforced: 0,
        });
    }

    // 2. Cluster by shared-tag overlap (same Union-Find pattern as consolidate).
    let n = eligibles.len();
    let mut uf = UnionFind::new(n);
    use std::collections::HashSet;
    let tag_sets: Vec<HashSet<&str>> = eligibles.iter()
        .map(|e| e.tags.iter().map(|s| s.as_str()).collect())
        .collect();
    for i in 0..n {
        for j in (i + 1)..n {
            let shared = tag_sets[i].intersection(&tag_sets[j]).count();
            if shared >= REFLECT_MIN_SHARED_TAGS {
                uf.union(i, j);
            }
        }
    }
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        let root = uf.find(i);
        groups.entry(root).or_default().push(i);
    }
    let mut viable: Vec<Vec<usize>> = groups.into_values()
        .filter(|g| g.len() >= REFLECT_MIN_CLUSTER && g.len() <= REFLECT_MAX_CLUSTER)
        .collect();
    viable.sort_by(|a, b| b.len().cmp(&a.len()));
    viable.truncate(REFLECT_MAX_CLUSTERS);

    if dry {
        return Ok(ReflectReport {
            dry_run: true,
            eligible_memories: eligible_count,
            clusters_processed: viable.len() as i64,
            insights_created: 0,
            insights_reinforced: 0,
        });
    }

    // 3. For each cluster: LLM → fingerprint match → create or reinforce.
    let base = crate::commands::embeddings::ollama_base();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(REFLECT_OLLAMA_TIMEOUT_S))
        .build()
        .map_err(|e| format!("http client: {}", e))?;

    let mut created = 0i64;
    let mut reinforced = 0i64;
    let mut processed = 0i64;

    for idxs in &viable {
        // Build user prompt
        let mut user_prompt = String::with_capacity(2048);
        user_prompt.push_str("Cluster of related observations:\n\n");
        for (i, &mi) in idxs.iter().enumerate() {
            let m = &eligibles[mi];
            let snippet: String = m.content.chars().take(280).collect();
            user_prompt.push_str(&format!(
                "{}. {} — {}\n   tags: {}\n\n",
                i + 1,
                m.title.chars().take(80).collect::<String>(),
                snippet,
                m.tags.join(", ")
            ));
        }

        let body = serde_json::json!({
            "model": "qwen3:4b",
            "messages": [
                { "role": "system", "content": REFLECT_SYSTEM },
                { "role": "user",   "content": user_prompt },
            ],
            "stream": false,
            "options": { "temperature": 0.4, "num_predict": 400 },
        });

        let resp = match client
            .post(format!("{}/api/chat", base))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };
        let json: serde_json::Value = match resp.json().await { Ok(j) => j, Err(_) => continue };
        let xml = json
            .get("message").and_then(|m| m.get("content")).and_then(|c| c.as_str())
            .unwrap_or("").to_string();

        let content = match xml_single_local(&xml, "content") {
            Some(c) if !c.trim().is_empty() && c.len() >= 20 => c,
            _ => continue,  // model emitted empty/too-short content → skip
        };
        let concepts: Vec<String> = xml_collect_local(&xml, "concept")
            .into_iter()
            .filter(|c| !c.trim().is_empty())
            .take(6)
            .collect();
        let concepts_json = serde_json::to_string(&concepts).unwrap_or_else(|_| "[]".to_string());

        let fp = insight_fingerprint(&content);
        let cluster_size = idxs.len() as i64;
        processed += 1;

        // 4. Upsert: try INSERT; on UNIQUE conflict, run reinforcement UPDATE.
        let action: Result<&'static str, String> = with_db(|conn| {
            let res = conn.execute(
                "INSERT INTO agent_insights \
                 (content, fingerprint, confidence, reinforcements, concepts, \
                  source_count, last_reinforced_at, created_at, updated_at) \
                 VALUES (?1, ?2, 0.5, 1, ?3, ?4, ?5, ?5, ?5)",
                rusqlite::params![content, fp, concepts_json, cluster_size, now],
            );
            match res {
                Ok(_) => Ok("created"),
                Err(rusqlite::Error::SqliteFailure(_, ref msg))
                    if msg.as_deref().unwrap_or("").contains("UNIQUE") =>
                {
                    // Reinforce: confidence += 0.1 * (1 - confidence)
                    conn.execute(
                        "UPDATE agent_insights SET \
                            confidence = confidence + 0.10 * (1.0 - confidence), \
                            reinforcements = reinforcements + 1, \
                            source_count = source_count + ?2, \
                            last_reinforced_at = ?3, \
                            updated_at = ?3 \
                         WHERE fingerprint = ?1",
                        rusqlite::params![fp, cluster_size, now],
                    ).map_err(|e| format!("reflect reinforce: {}", e))?;
                    Ok("reinforced")
                }
                Err(e) => Err(format!("reflect insert: {}", e)),
            }
        });

        match action {
            Ok("created")    => created += 1,
            Ok("reinforced") => reinforced += 1,
            _ => {}  // already counted in `processed`, no-op on insert failure
        }
    }

    Ok(ReflectReport {
        dry_run: false,
        eligible_memories: eligible_count,
        clusters_processed: processed,
        insights_created: created,
        insights_reinforced: reinforced,
    })
}

/// List insights, ordered by (confidence DESC, reinforcements DESC) — the
/// most-confident, most-recurrent insights surface first.
#[tauri::command]
pub fn list_insights(limit: Option<i64>) -> Result<Vec<AgentInsight>, String> {
    let lim = limit.unwrap_or(20).clamp(1, 100);
    with_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, content, fingerprint, confidence, reinforcements, \
                    concepts, source_count, last_reinforced_at, created_at, updated_at \
             FROM agent_insights \
             ORDER BY confidence DESC, reinforcements DESC \
             LIMIT ?1"
        ).map_err(|e| format!("list_insights prepare: {}", e))?;
        let rows = stmt.query_map(rusqlite::params![lim], |row| Ok(AgentInsight {
            id:                 row.get(0)?,
            content:            row.get(1)?,
            fingerprint:        row.get(2)?,
            confidence:         row.get(3)?,
            reinforcements:     row.get(4)?,
            concepts:           row.get(5)?,
            source_count:       row.get(6)?,
            last_reinforced_at: row.get(7)?,
            created_at:         row.get(8)?,
            updated_at:         row.get(9)?,
        })).map_err(|e| format!("list_insights query: {}", e))?;
        let mut out = Vec::new();
        for i in rows.flatten() { out.push(i); }
        Ok(out)
    })
}

#[tauri::command]
pub fn delete_insight(id: i64) -> Result<usize, String> {
    if id <= 0 { return Err("delete_insight: invalid id".to_string()); }
    with_db(|conn| {
        let n = conn.execute(
            "DELETE FROM agent_insights WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| format!("delete_insight: {}", e))?;
        Ok(n)
    })
}

// ── Memory graph (Tier 3 #9, agentmemory-inspired) ───────────────────────
// Multi-hop graph over agent_memories. Edges encode WHY two memories are
// related — shared concepts (tags), shared file references, or same
// agent session. BFS traversal with per-hop decay surfaces memories that
// are 2-3 jumps away from a seed, useful for queries like "what else
// relates to this DNS incident?" or "memories adjacent to the patches I
// applied last week".
//
// Edges are built in batch (graph_rebuild_edges_run) rather than on every
// save — the hot save path stays fast. The scheduler runs the rebuild
// every 24 h, AFTER auto-consolidate so consolidated/superseded rows are
// already settled.

const GRAPH_MIN_SHARED_TAGS:    usize = 2;
const GRAPH_MAX_EDGES_PER_MEM:  usize = 12;   // Cap fan-out per source
const GRAPH_SESSION_GROUP_LIMIT: usize = 20;  // Same-session edges per group

#[derive(serde::Serialize)]
pub struct GraphRebuildReport {
    pub eligible_memories: i64,
    pub concept_edges:     i64,
    pub file_edges:        i64,
    pub session_edges:     i64,
    pub total_directed_edges: i64,
}

/// Rebuild the entire graph from scratch (truncates agent_memory_edges
/// first). Idempotent — safe to call any time. Cheap on modest DBs
/// (≤500 memories runs in <1s); the cap on eligible rows keeps it bounded
/// regardless of how large the memory store grows over years.
#[tauri::command]
pub fn graph_rebuild_edges_run() -> Result<GraphRebuildReport, String> {
    // 1. Pull all non-superseded memories with their tag/file/session.
    #[derive(Clone)]
    struct Node {
        id:         i64,
        session_id: String,
        tags:       Vec<String>,
        files:      Vec<String>,
    }
    let nodes: Vec<Node> = with_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, tags, files FROM agent_memories
             WHERE superseded_by IS NULL
             ORDER BY id ASC
             LIMIT 2000"
        ).map_err(|e| format!("graph fetch prepare: {}", e))?;
        let rows = stmt.query_map([], |row| {
            let tags_json:  String = row.get(2)?;
            let files_json: String = row.get(3)?;
            let tags  = parse_tags(&tags_json);
            let files: Vec<String> = serde_json::from_str(&files_json).unwrap_or_default();
            Ok(Node {
                id:         row.get(0)?,
                session_id: row.get(1)?,
                tags,
                files,
            })
        }).map_err(|e| format!("graph fetch query: {}", e))?;
        let mut out = Vec::new();
        for n in rows.flatten() { out.push(n); }
        Ok(out)
    })?;

    let total = nodes.len();
    if total < 2 {
        return Ok(GraphRebuildReport {
            eligible_memories: total as i64,
            concept_edges: 0, file_edges: 0, session_edges: 0,
            total_directed_edges: 0,
        });
    }

    // 2. Build edge candidates in memory before writing.
    // Vec<(src, tgt, type, weight)> — we'll dedupe by primary key on insert.
    let mut edges: Vec<(i64, i64, &'static str, f64)> = Vec::with_capacity(total * 8);
    use std::collections::{HashMap, HashSet};

    // Pre-compute tag-sets and file-sets for fast intersection.
    let tag_sets: Vec<HashSet<&str>> = nodes.iter()
        .map(|n| n.tags.iter().map(|s| s.as_str()).collect())
        .collect();
    let file_sets: Vec<HashSet<&str>> = nodes.iter()
        .map(|n| n.files.iter().map(|s| s.as_str()).collect())
        .collect();

    // Track best K outgoing edges per source to enforce the cap.
    // Map<src_id, Vec<(tgt_id, type, weight)>> after building all, we'll
    // keep top-K by weight per source per type.

    // Concept edges — pairwise Jaccard on tags
    let mut concept_count = 0i64;
    for i in 0..total {
        if tag_sets[i].is_empty() { continue; }
        for j in (i + 1)..total {
            let shared = tag_sets[i].intersection(&tag_sets[j]).count();
            if shared >= GRAPH_MIN_SHARED_TAGS {
                let union = tag_sets[i].union(&tag_sets[j]).count();
                let weight = if union > 0 { shared as f64 / union as f64 } else { 0.0 };
                edges.push((nodes[i].id, nodes[j].id, "shares_concept", weight));
                edges.push((nodes[j].id, nodes[i].id, "shares_concept", weight));
                concept_count += 2;
            }
        }
    }

    // File edges — any shared file path is enough (rare = high signal)
    let mut file_count = 0i64;
    for i in 0..total {
        if file_sets[i].is_empty() { continue; }
        for j in (i + 1)..total {
            let shared = file_sets[i].intersection(&file_sets[j]).count();
            if shared > 0 {
                let weight = ((shared as f64) / 3.0).min(1.0);  // cap at 1.0
                edges.push((nodes[i].id, nodes[j].id, "shares_file", weight));
                edges.push((nodes[j].id, nodes[i].id, "shares_file", weight));
                file_count += 2;
            }
        }
    }

    // Session edges — group by session_id, fully connect within group up
    // to GRAPH_SESSION_GROUP_LIMIT members. Skip empty session ids.
    let mut session_groups: HashMap<&str, Vec<i64>> = HashMap::new();
    for n in &nodes {
        if n.session_id.is_empty() { continue; }
        session_groups.entry(n.session_id.as_str()).or_default().push(n.id);
    }
    let mut session_count = 0i64;
    for members in session_groups.values() {
        if members.len() < 2 { continue; }
        let m: Vec<i64> = members.iter().take(GRAPH_SESSION_GROUP_LIMIT).copied().collect();
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                edges.push((m[i], m[j], "same_session", 0.5));
                edges.push((m[j], m[i], "same_session", 0.5));
                session_count += 2;
            }
        }
    }

    // 3. Cap fan-out per (source, type). Sort by weight DESC and keep top-K.
    edges.sort_by(|a, b|
        a.0.cmp(&b.0)
            .then_with(|| a.2.cmp(b.2))
            .then_with(|| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal))
    );
    // Now per (source, type) the strongest weights come first; walk and
    // keep only the first K of each group.
    let mut kept: Vec<(i64, i64, &'static str, f64)> = Vec::with_capacity(edges.len());
    let mut current_src: i64 = -1;
    let mut current_type: &'static str = "";
    let mut taken: usize = 0;
    for e in edges {
        if e.0 != current_src || e.2 != current_type {
            current_src = e.0;
            current_type = e.2;
            taken = 0;
        }
        if taken < GRAPH_MAX_EDGES_PER_MEM {
            kept.push(e);
            taken += 1;
        }
    }
    let total_kept = kept.len() as i64;

    // 4. Write — truncate first, then bulk INSERT in a transaction.
    with_db(|conn| {
        let tx = conn.unchecked_transaction()
            .map_err(|e| format!("graph tx: {}", e))?;
        tx.execute("DELETE FROM agent_memory_edges", [])
            .map_err(|e| format!("graph truncate: {}", e))?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO agent_memory_edges \
                 (source_id, target_id, edge_type, weight) VALUES (?1, ?2, ?3, ?4)"
            ).map_err(|e| format!("graph insert prepare: {}", e))?;
            for (src, tgt, et, w) in &kept {
                stmt.execute(rusqlite::params![src, tgt, et, w])
                    .map_err(|e| format!("graph insert: {}", e))?;
            }
        }
        tx.commit().map_err(|e| format!("graph commit: {}", e))?;
        Ok(())
    })?;

    Ok(GraphRebuildReport {
        eligible_memories: total as i64,
        concept_edges: concept_count,
        file_edges: file_count,
        session_edges: session_count,
        total_directed_edges: total_kept,
    })
}

#[derive(serde::Serialize)]
pub struct GraphNeighbor {
    pub memory_id: i64,
    pub hops:      i64,
    pub score:     f64,
    /// Pipe-joined list of edge types that connected this node — debugging
    /// help: "shares_concept|same_session" means we got here via both.
    pub edge_types: String,
    pub memory:    AgentMemory,
}

/// BFS over agent_memory_edges starting from `seed_id`. Each hop multiplies
/// the cumulative weight by `hop_decay` (default 0.5). Up to `max_hops`
/// levels deep, returning the top-`limit` nodes by score (excluding the
/// seed itself).
///
/// Multi-path nodes keep their BEST score across all reachable paths —
/// the BFS keeps frontiers fresh but updates already-seen nodes if a new
/// path delivers a higher score.
#[tauri::command]
pub fn graph_neighbors(
    seed_id:  i64,
    max_hops: Option<i64>,
    limit:    Option<i64>,
) -> Result<Vec<GraphNeighbor>, String> {
    if seed_id <= 0 { return Err("graph_neighbors: invalid seed_id".to_string()); }
    let hops = max_hops.unwrap_or(2).clamp(1, 4);
    let lim  = limit.unwrap_or(15).clamp(1, 50);
    const HOP_DECAY: f64 = 0.5;

    // BFS with score + path-type tracking
    use std::collections::HashMap;
    // node -> (best_score, hop_at_best, edge_types_concat)
    let mut best: HashMap<i64, (f64, i64, String)> = HashMap::new();
    let mut frontier: Vec<(i64, f64, i64)> = vec![(seed_id, 1.0, 0)];

    for current_hop in 0..hops {
        if frontier.is_empty() { break; }
        let mut next: Vec<(i64, f64, i64)> = Vec::new();
        // Bulk-fetch all outgoing edges for the current frontier in ONE query
        let src_list: Vec<String> = frontier.iter()
            .map(|(s, _, _)| s.to_string()).collect();
        if src_list.is_empty() { break; }
        let in_clause = src_list.join(",");
        // safe: ids are i64
        let sql = format!(
            "SELECT source_id, target_id, edge_type, weight FROM agent_memory_edges \
             WHERE source_id IN ({}) ORDER BY weight DESC", in_clause
        );

        let edges: Vec<(i64, i64, String, f64)> = with_db(|conn| {
            let mut stmt = conn.prepare(&sql).map_err(|e| format!("graph BFS prepare: {}", e))?;
            let rows = stmt.query_map([], |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
            ))).map_err(|e| format!("graph BFS query: {}", e))?;
            let mut out = Vec::new();
            for e in rows.flatten() { out.push(e); }
            Ok(out)
        })?;

        // Index frontier by source for O(1) lookup of parent score
        let frontier_scores: HashMap<i64, f64> = frontier.iter()
            .map(|(s, sc, _)| (*s, *sc)).collect();

        for (src, tgt, et, w) in edges {
            if tgt == seed_id { continue; }
            let parent_score = frontier_scores.get(&src).copied().unwrap_or(0.0);
            let new_score = parent_score * w * (HOP_DECAY.powi(current_hop as i32));
            let new_hop = current_hop + 1;

            let mut should_expand = true;
            best.entry(tgt)
                .and_modify(|(s, h, types)| {
                    if new_score > *s {
                        *s = new_score; *h = new_hop;
                    }
                    if !types.contains(&et) {
                        if !types.is_empty() { types.push('|'); }
                        types.push_str(&et);
                    } else {
                        should_expand = false;
                    }
                })
                .or_insert_with(|| (new_score, new_hop, et.clone()));
            if should_expand {
                next.push((tgt, new_score, new_hop));
            }
        }
        // De-dupe next frontier: one entry per id with best score
        let mut next_map: HashMap<i64, (f64, i64)> = HashMap::new();
        for (id, sc, h) in next {
            next_map.entry(id)
                .and_modify(|x| { if sc > x.0 { x.0 = sc; x.1 = h; } })
                .or_insert((sc, h));
        }
        frontier = next_map.into_iter().map(|(id, (sc, h))| (id, sc, h)).collect();
    }

    if best.is_empty() {
        return Ok(Vec::new());
    }

    // Take top-`lim` ids by score
    let mut ranked: Vec<(i64, f64, i64, String)> = best.into_iter()
        .map(|(id, (sc, h, types))| (id, sc, h, types))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(lim as usize);

    if ranked.is_empty() { return Ok(Vec::new()); }

    // Fetch full memory rows in one query
    let id_list = ranked.iter().map(|(id, _, _, _)| id.to_string()).collect::<Vec<_>>().join(",");
    let mem_rows: HashMap<i64, AgentMemory> = with_db(|conn| {
        let sql = format!(
            "SELECT id, session_id, title, content, tags, files, importance, created_at \
             FROM agent_memories WHERE id IN ({}) AND superseded_by IS NULL", id_list
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("graph mem prepare: {}", e))?;
        let rows = stmt.query_map([], |row| Ok(AgentMemory {
            id:         row.get(0)?,
            session_id: row.get(1)?,
            title:      row.get(2)?,
            content:    row.get(3)?,
            tags:       row.get(4)?,
            files:      row.get(5)?,
            importance: row.get(6)?,
            created_at: row.get(7)?,
        })).map_err(|e| format!("graph mem query: {}", e))?;
        let mut m = HashMap::new();
        for am in rows.flatten() { m.insert(am.id, am); }
        Ok(m)
    })?;

    let neighbors: Vec<GraphNeighbor> = ranked.into_iter()
        .filter_map(|(id, score, hop, types)| {
            mem_rows.get(&id).map(|m| GraphNeighbor {
                memory_id: id,
                hops: hop,
                score,
                edge_types: types,
                memory: m.clone(),
            })
        })
        .collect();

    Ok(neighbors)
}

// ── Persistent session summaries (anti-thread-loss) ──────────────────────
// Companion to the frontend's `regenerateSmartDigest()` — that function
// builds the YAML/text summary by asking a cheap LLM to compact the older
// half of a tab's messages, but until now stored the result only in
// in-memory `tab.workingMemory.compactedDigest`. That was lost on close.
//
// These commands let the frontend persist + retrieve the summary so a
// session survives a Lucy restart without losing the thread.

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SessionSummary {
    pub id: i64,
    pub tab_id: String,
    pub anchor_msg_index: i64,
    pub summary: String,
    pub model_used: String,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert a session summary. We delete any prior row for this tab_id first
/// (one summary per tab), then insert the fresh one. This is the path the
/// frontend takes every time regenerateSmartDigest succeeds.
#[tauri::command]
pub fn save_session_summary(
    tab_id:           String,
    anchor_msg_index: i64,
    summary:          String,
    model_used:       Option<String>,
    tokens_in:        Option<i64>,
    tokens_out:       Option<i64>,
) -> Result<i64, String> {
    if tab_id.trim().is_empty() {
        return Err("save_session_summary: empty tab_id".to_string());
    }
    if summary.trim().is_empty() {
        return Err("save_session_summary: empty summary".to_string());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    with_db(|conn| {
        let tx = conn.unchecked_transaction()
            .map_err(|e| format!("session-summary tx: {}", e))?;
        // Replace previous summary for this tab (one per tab)
        tx.execute(
            "DELETE FROM agent_session_summaries WHERE tab_id = ?1",
            rusqlite::params![tab_id],
        ).map_err(|e| format!("session-summary delete: {}", e))?;
        tx.execute(
            "INSERT INTO agent_session_summaries \
             (tab_id, anchor_msg_index, summary, model_used, tokens_in, tokens_out, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            rusqlite::params![
                tab_id, anchor_msg_index, summary,
                model_used.unwrap_or_default(),
                tokens_in.unwrap_or(0),
                tokens_out.unwrap_or(0),
                now
            ],
        ).map_err(|e| format!("session-summary insert: {}", e))?;
        let id = tx.last_insert_rowid();
        tx.commit().map_err(|e| format!("session-summary commit: {}", e))?;
        Ok(id)
    })
}

/// Fetch the latest summary for a tab, or None.
#[tauri::command]
pub fn get_session_summary(tab_id: String) -> Result<Option<SessionSummary>, String> {
    if tab_id.trim().is_empty() { return Ok(None); }
    with_db(|conn| {
        let r: rusqlite::Result<SessionSummary> = conn.query_row(
            "SELECT id, tab_id, anchor_msg_index, summary, model_used, \
                    tokens_in, tokens_out, created_at, updated_at \
             FROM agent_session_summaries WHERE tab_id = ?1 \
             ORDER BY updated_at DESC LIMIT 1",
            rusqlite::params![tab_id],
            |row| Ok(SessionSummary {
                id:               row.get(0)?,
                tab_id:           row.get(1)?,
                anchor_msg_index: row.get(2)?,
                summary:          row.get(3)?,
                model_used:       row.get(4)?,
                tokens_in:        row.get(5)?,
                tokens_out:       row.get(6)?,
                created_at:       row.get(7)?,
                updated_at:       row.get(8)?,
            }),
        );
        match r {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("get_session_summary: {}", e)),
        }
    })
}

/// Delete all summaries for a tab (called when the user closes the tab
/// or explicitly clears its conversation).
#[tauri::command]
pub fn delete_session_summary(tab_id: String) -> Result<usize, String> {
    if tab_id.trim().is_empty() { return Ok(0); }
    with_db(|conn| {
        let n = conn.execute(
            "DELETE FROM agent_session_summaries WHERE tab_id = ?1",
            rusqlite::params![tab_id],
        ).map_err(|e| format!("delete_session_summary: {}", e))?;
        Ok(n)
    })
}

// ── v1.6.11 — Memory Browser Auto-tag UPDATE path ─────────────────────────
//
// MemoryBrowserView's "Accept AI tags" flow was calling
// `save_agent_memory_full`, a command that never existed. Result: hotfix
// v1.6.10 unblocked the LLM call but Accept still failed with
// "Command save_agent_memory_full not found".
//
// The actual operation is small — UPDATE one row's tags (JSON-encoded).
// We could overload save_agent_memory but its INSERT/dedup path is wrong
// for "replace tags on an existing row". A dedicated command keeps the
// concerns separate.

#[tauri::command]
pub async fn update_agent_memory_tags(
    id:   i64,
    tags: Vec<String>,
) -> Result<(), String> {
    let tags_json = serde_json::to_string(&tags)
        .map_err(|e| format!("tags JSON encode: {}", e))?;
    with_db(|conn| {
        let n = conn.execute(
            "UPDATE agent_memories SET tags = ?1 WHERE id = ?2",
            rusqlite::params![tags_json, id],
        ).map_err(|e| format!("update_agent_memory_tags: {}", e))?;
        if n == 0 {
            return Err(format!("no memory found with id={}", id));
        }
        Ok(())
    })
}

// ── v1.7.120 — Memory Browser bulk-promote UPDATE path ────────────────────
//
// MemoryBrowserView's "bulk promote" flow (importance +1) was ALSO calling
// the non-existent `save_agent_memory_full`, so the button silently did
// nothing (the per-row catch swallowed "command not found"). Same class of
// hole as the tags path above. A dedicated importance UPDATE keeps it small
// and avoids touching save_agent_memory's INSERT/dedup path.
// v1.7.233: clamped 1..=3 — the SAVE path clamps 1..=3, every janitor policy
// reasons in the 1..=3 scale ("3 is never evicted", "1 is consolidation-
// eligible"), and the browser renders exactly 3 diamonds. The old 1..=10 cap
// let bulk-promote mint importance values no other subsystem understood.
#[tauri::command]
pub async fn update_agent_memory_importance(
    id:         i64,
    importance: i64,
) -> Result<(), String> {
    let imp = importance.clamp(1, 3);
    with_db(|conn| {
        let n = conn.execute(
            "UPDATE agent_memories SET importance = ?1 WHERE id = ?2",
            rusqlite::params![imp, id],
        ).map_err(|e| format!("update_agent_memory_importance: {}", e))?;
        if n == 0 {
            return Err(format!("no memory found with id={}", id));
        }
        Ok(())
    })
}

#[cfg(test)]
mod schema_tests {
    use super::*;

    /// v1.7.234 regression — the REAL full init path on a FRESH database
    /// (first-time install). Must never fail.
    #[test]
    fn fresh_db_full_real_init_succeeds() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        run_schema_init(&conn).expect("real init path must succeed on a FRESH database");
    }

    /// v1.7.238 — el clasificador de corrupción decide cuarentena vs reintento.
    /// Debe reconocer los mensajes reales de SQLite de archivo dañado y NO
    /// confundir un lock transitorio (que sí se reintenta) con corrupción.
    #[test]
    fn corruption_classifier_distinguishes_corruption_from_transient() {
        assert!(looks_like_corruption("database disk image is malformed"));
        assert!(looks_like_corruption("file is not a database"));
        assert!(looks_like_corruption("Failed to initialize schema: file is encrypted or is not a database"));
        // Locks / IO transitorios NO son corrupción → se reintentan, no se borra la DB.
        assert!(!looks_like_corruption("database is locked"));
        assert!(!looks_like_corruption("Failed to acquire init connection: timed out"));
        assert!(!looks_like_corruption("unable to open database file"));
    }

    /// v1.7.236 R5 — the pinned column must exist after the real init chain
    /// (guards the migration-ordering class of bugs) and be updatable.
    #[test]
    fn pinned_column_exists_after_real_init() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        run_schema_init(&conn).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('agent_memories') WHERE name='pinned'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "pinned column must exist after migrations");
        conn.execute("UPDATE agent_memories SET pinned = 1 WHERE 1=0", [])
            .expect("pinned column must be updatable");
    }

    /// v1.7.236 R4 — the verdict parser must be strict: only a clear SI/YES
    /// counts as contradiction; everything ambiguous keeps both memories.
    #[test]
    fn contradiction_verdict_parser_is_strict() {
        use super::parse_contradiction_verdict as p;
        assert!(p("SI"));
        assert!(p("sí"));
        assert!(p("YES"));
        assert!(p("  SI.  el valor cambió"));
        assert!(p("<think>hmm 30 vs 60 son incompatibles</think>\nSI"));
        assert!(!p("NO"));
        assert!(!p("no, son complementarios"));
        assert!(!p("<think>duda</think>\nNO"));
        assert!(!p("SINCERAMENTE no lo sé"));   // "SINCERAMENTE" != token SI
        assert!(!p(""));
        assert!(!p("quizás"));
        assert!(!p("Los hechos SI se contradicen")); // el veredicto va PRIMERO o no vale
    }

    /// v1.7.236 — auto-crystallization CHEAP gates must reject thin sessions
    /// BEFORE any Ollama/DB work (flood protection). These early-return paths
    /// never touch the DB, so no init is needed.
    #[tokio::test]
    async fn auto_crystallize_gates_reject_thin_sessions() {
        let big = "x".repeat(2000);
        // too few turns
        let r = maybe_auto_crystallize_session("s1".into(), None, big.clone(), 2, 9).await.unwrap();
        assert!(!r.fired, "2 turns must not fire");
        assert_eq!(r.reason, "session too short (turns)");
        // enough turns, too few tool calls
        let r = maybe_auto_crystallize_session("s1".into(), None, big.clone(), 8, 1).await.unwrap();
        assert!(!r.fired, "1 tool call must not fire");
        assert_eq!(r.reason, "not enough tool activity");
        // enough turns+tools, transcript too thin
        let r = maybe_auto_crystallize_session("s1".into(), None, "short".into(), 8, 9).await.unwrap();
        assert!(!r.fired, "thin transcript must not fire");
        assert_eq!(r.reason, "transcript too thin");
        // empty session id
        let r = maybe_auto_crystallize_session("".into(), None, big, 8, 9).await.unwrap();
        assert!(!r.fired, "no session id must not fire");
        assert_eq!(r.reason, "no session id");
    }

    /// v1.7.233 regression — the GoAnywhere incident hotfix-of-the-hotfix.
    /// INIT_SQL once contained `CREATE INDEX ... ON pdf_documents(content_hash)`,
    /// but on an EXISTING database INIT_SQL runs BEFORE the migration that adds
    /// the column → "no such column: content_hash" aborted the whole schema
    /// init ("Metrics DB not initialized", every memory feature dead). Fresh
    /// test DBs never caught it because CREATE TABLE ships the column.
    /// This test boots against a LEGACY pdf_documents (no content_hash).
    #[test]
    fn init_sql_and_pdf_hash_migration_succeed_on_legacy_db() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        // Simulate a pre-v1.7.233 database: pdf_documents WITHOUT content_hash.
        conn.execute_batch(
            "CREATE TABLE pdf_documents (
                id          TEXT    PRIMARY KEY,
                filename    TEXT    NOT NULL,
                path        TEXT    NOT NULL,
                page_count  INTEGER NOT NULL DEFAULT 0,
                chunk_count INTEGER NOT NULL DEFAULT 0,
                ingested_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                status      TEXT    NOT NULL DEFAULT 'ingesting'
            );",
        ).unwrap();

        // Boot step 1: INIT_SQL must NOT reference content_hash anywhere
        // (CREATE TABLE IF NOT EXISTS is a no-op here, so the old shape stays).
        conn.execute_batch(INIT_SQL)
            .expect("INIT_SQL must succeed on a legacy DB without content_hash");

        // Boot step 2: the migration pair (same statements as the migrations
        // array, same order — ALTER first, index second).
        conn.execute(
            "ALTER TABLE pdf_documents ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''",
            [],
        ).expect("content_hash ALTER must succeed on a legacy DB");
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_pdf_docs_hash \
             ON pdf_documents(content_hash) WHERE content_hash != ''",
            [],
        ).expect("content_hash index must succeed after the ALTER");

        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('pdf_documents') WHERE name = 'content_hash'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(n, 1, "content_hash column missing after migration");
    }

    /// Fresh-install path: INIT_SQL ships content_hash in CREATE TABLE, the
    /// ALTER then fails with "duplicate column" (swallowed by the migration
    /// runner) and the index still applies cleanly.
    #[test]
    fn fresh_db_migration_pair_is_idempotent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(INIT_SQL).expect("INIT_SQL on fresh DB");
        let err = conn.execute(
            "ALTER TABLE pdf_documents ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''",
            [],
        ).unwrap_err().to_string();
        assert!(err.contains("duplicate column"), "expected duplicate-column error, got: {}", err);
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_pdf_docs_hash \
             ON pdf_documents(content_hash) WHERE content_hash != ''",
            [],
        ).expect("index on fresh DB");
    }
}
