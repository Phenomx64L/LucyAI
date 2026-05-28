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

/// Initialize the shared pool + schema. Call once from the Tauri setup hook.
pub fn init(app: &AppHandle) -> Result<(), String> {
    if POOL.get().is_some() {
        return Ok(());
    }
    let path = db_path(app)?;

    // Pool manager: every connection it hands out runs the same PRAGMAs.
    // WAL mode is per-DATABASE (set once persists), but synchronous,
    // temp_store and busy_timeout are per-connection and must be set on
    // each handle the pool spawns.
    let manager = r2d2_sqlite::SqliteConnectionManager::file(&path)
        .with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode=WAL;\
                 PRAGMA synchronous=NORMAL;\
                 PRAGMA temp_store=MEMORY;\
                 PRAGMA busy_timeout=5000;\
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

    // Release the init connection back to the pool, then publish.
    drop(conn);
    POOL.set(pool)
        .map_err(|_| "DB pool already initialized".to_string())?;
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
    f(&*conn)
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
        conn.execute(
            "INSERT INTO token_usage (id, task_id, timestamp, model, input_tokens, output_tokens, total_cost, user, request_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![&id, &task_id, &timestamp, model, input_tokens, output_tokens, total_cost, user, request_type],
        ).map_err(|e| format!("Failed to insert token_usage: {}", e))?;

        // Atomic UPSERT — replaces the prior check-then-insert race.
        conn.execute(
            "INSERT INTO daily_summary (date, model, total_tokens, total_cost, request_count)
             VALUES (?1, ?2, ?3, ?4, 1)
             ON CONFLICT(date, model) DO UPDATE SET
                 total_tokens = total_tokens + excluded.total_tokens,
                 total_cost = total_cost + excluded.total_cost,
                 request_count = request_count + 1",
            rusqlite::params![&date, model, total_tokens, total_cost],
        ).map_err(|e| format!("Failed to upsert daily_summary: {}", e))?;
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
    // Stage 1 + INSERT happen synchronously in the DB closure. Stage 2
    // (embedding probe) is async, so we run it BEFORE the DB closure
    // when bm25 didn't catch a dup. The closure handles the insert + dup
    // dance for stages 1 and 2 atomically.
    let stage1_result = with_db(|conn| stage1_fts_dedup(conn, &title, &content))?;
    if let Some(dup) = stage1_result {
        return Ok(dup);
    }

    // Stage 2 — try embedding-based dedup. If Ollama is offline or any
    // step fails, fall through to insert without semantic dedup. Stage 2
    // is best-effort; we never block a save on it.
    let stage2_result = stage2_embedding_dedup(&title, &content).await;
    if let Ok(Some(dup)) = stage2_result {
        return Ok(dup);
    }

    // No dups found — insert the fresh row.
    let new_id = with_db(|conn| {
        let imp  = importance.unwrap_or(1).max(1).min(3);
        let tags  = tags.unwrap_or_else(|| "[]".to_string());
        let files = files.unwrap_or_else(|| "[]".to_string());
        let sid   = session_id.unwrap_or_default();
        // TTL → absolute expires_at epoch seconds. ttl_days == None or 0
        // → keep forever (expires_at = 0, the default).
        let expires_at: i64 = match ttl_days.unwrap_or(0) {
            d if d > 0 => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                now + d * 86_400
            }
            _ => 0,
        };
        conn.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, files, importance, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![sid, title, content, tags, files, imp, expires_at],
        ).map_err(|e| format!("save_agent_memory: {}", e))?;
        Ok(conn.last_insert_rowid())
    })?;

    Ok(SaveMemoryResult {
        id: new_id,
        action: "inserted".to_string(),
        reason: "New memory stored".to_string(),
    })
}

/// Stage 1 — FTS5 bm25 dedup. Returns Some(dup-result) when a strong
/// match is found, None otherwise. Synchronous (DB-only).
fn stage1_fts_dedup(
    conn: &rusqlite::Connection,
    title: &str,
    content: &str,
) -> Result<Option<SaveMemoryResult>, String> {
    let probe = format!("{} {}", title, &content.chars().take(200).collect::<String>());
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
        "SELECT am.id, bm25(agent_memories_fts) AS score
         FROM agent_memories am
         JOIN agent_memories_fts fts ON am.id = fts.rowid
         WHERE agent_memories_fts MATCH ?1
           AND am.superseded_by IS NULL
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
    let probe = format!("{}\n{}", title, &content.chars().take(800).collect::<String>());

    // Embed via Ollama. If embeddings aren't available we silently skip.
    let embed_res = crate::commands::embeddings::embed_via_ollama_pub(&probe, None).await;
    let query_vec = match embed_res {
        Ok((v, _)) => v,
        Err(_) => return Ok(None),  // Ollama offline / model missing — skip stage 2
    };

    // Search the in-memory vec index for high-similarity memories
    let hits = crate::commands::vec_index::search(&query_vec, 5, 0.92);
    let best_memory = hits.iter()
        .find(|(etype, _id, _text, _score)| etype == "memory");
    let Some((_etype, entity_id, _text, score)) = best_memory else {
        return Ok(None);
    };

    let dup_id: i64 = match entity_id.parse() {
        Ok(i) => i,
        Err(_) => return Ok(None),  // entity_id wasn't a memory row id — skip
    };

    // Touch the existing row, then return the dup result.
    let _ = with_db(|conn| {
        conn.execute(
            "UPDATE agent_memories
             SET access_count = access_count + 1,
                 last_accessed_at = strftime('%s','now')
             WHERE id = ?1 AND superseded_by IS NULL",
            rusqlite::params![dup_id],
        ).map_err(|e| format!("touch failed: {}", e))?;
        Ok::<(), String>(())
    });

    Ok(Some(SaveMemoryResult {
        id: dup_id,
        action: "duplicate".to_string(),
        reason: format!("Embedding cosine {:.3} matches memory {} (semantic paraphrase)", score, dup_id),
    }))
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
pub async fn search_agent_memories(query: String, limit: Option<i64>) -> Result<Vec<AgentMemory>, String> {
    let lim = limit.unwrap_or(10).max(1).min(50);
    if query.trim().is_empty() {
        return with_db(|_conn| get_recent_memories(Some(lim)));
    }
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
        let sql = "SELECT am.id
                   FROM agent_memories am
                   JOIN agent_memories_fts fts ON am.id = fts.rowid
                   WHERE agent_memories_fts MATCH ?1
                     AND am.superseded_by IS NULL
                   ORDER BY bm25(agent_memories_fts) ASC
                   LIMIT ?2";
        let mut stmt = conn.prepare(sql).map_err(|e| format!("bm25 prepare: {}", e))?;
        let n = FETCH_N as i64;
        let rows = stmt.query_map(rusqlite::params![safe_q, n], |r| r.get::<_, i64>(0))
            .map_err(|e| format!("bm25 query: {}", e))?;
        let mut ids = Vec::with_capacity(FETCH_N);
        for r in rows { if let Ok(id) = r { ids.push(id); } }
        Ok(ids)
    })?;

    // ── Stream 2: embedding cosine (best-effort, async) ──────────────
    // Skip silently if Ollama embeddings aren't available — RRF over a
    // single stream is still valid, it just reduces to bm25 ranking.
    let cosine_ids: Vec<i64> = match crate::commands::embeddings::embed_via_ollama_pub(&query, None).await {
        Ok((qvec, _)) => {
            crate::commands::vec_index::search(&qvec, FETCH_N, 0.50)
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
        let top: Vec<AgentMemory> = combined.into_iter().take(lim_usize).map(|(m, _)| m).collect();

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
async fn expand_query_via_ollama(query: &str) -> Vec<String> {
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
        let sql = "SELECT am.id
                   FROM agent_memories am
                   JOIN agent_memories_fts fts ON am.id = fts.rowid
                   WHERE agent_memories_fts MATCH ?1
                     AND am.superseded_by IS NULL
                   ORDER BY bm25(agent_memories_fts) ASC
                   LIMIT ?2";
        let mut stmt = conn.prepare(sql).map_err(|e| format!("bm25 prepare: {}", e))?;
        let n = limit as i64;
        let rows = stmt.query_map(rusqlite::params![safe_q, n], |r| r.get::<_, i64>(0))
            .map_err(|e| format!("bm25 query: {}", e))?;
        let mut ids = Vec::with_capacity(limit);
        for r in rows { if let Ok(id) = r { ids.push(id); } }
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
    let lim = limit.unwrap_or(10).max(1).min(50);
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
            Ok((qvec, _)) => crate::commands::vec_index::search(&qvec, FETCH_N, 0.50)
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
    let top: Vec<AgentMemory> = final_ranked.into_iter()
        .take(lim_usize)
        .map(|(m, _)| m)
        .collect();

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
        let low_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_memories
             WHERE access_count = 0
               AND importance = 1
               AND expires_at = 0
               AND created_at < ?1
               AND superseded_by IS NULL",
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
                   AND superseded_by IS NULL",
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
    let imp = importance.unwrap_or(2).max(1).min(3);
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

/// Return the most recent memories ordered by importance then date.
#[tauri::command]
pub fn get_recent_memories(limit: Option<i64>) -> Result<Vec<AgentMemory>, String> {
    with_db(|conn| {
        let lim = limit.unwrap_or(15).max(1).min(50);
        let sql = "SELECT id, session_id, title, content, tags, files, importance, created_at
                   FROM agent_memories
                   ORDER BY importance DESC, created_at DESC
                   LIMIT ?1";
        let mut stmt = conn.prepare(sql).map_err(|e| format!("get_recent prepare: {}", e))?;
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
    let lim = limit.unwrap_or(15).max(1).min(50);
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

/// Query aggregated telemetry data for the quality dashboard.
/// Returns event summaries grouped by type for the given period.
#[tauri::command]
pub async fn get_task_telemetry(
    period: Option<String>,
) -> Result<Vec<TelemetrySummary>, String> {
    let since = match period.as_deref() {
        Some("day") => "datetime('now', '-1 day')",
        Some("week") => "datetime('now', '-7 days')",
        Some("all") => "datetime('1970-01-01')",
        _ => "datetime('now', '-30 days')", // default: month
    };

    with_db(|conn| {
        let sql = format!(
            "SELECT event_type,
                    COUNT(*) as cnt,
                    AVG(elapsed_ms) as avg_ms,
                    MAX(timestamp) as last_ts
             FROM task_events
             WHERE timestamp >= strftime('%s', {})
             GROUP BY event_type
             ORDER BY cnt DESC",
            since
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
    for lesson in &lessons {
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

    Ok(new_id)
}

/// List recent crystals, newest first. Optional filters by session or project.
#[tauri::command]
pub fn list_crystals(
    session_id: Option<String>,
    project:    Option<String>,
    limit:      Option<i64>,
) -> Result<Vec<AgentCrystal>, String> {
    let lim = limit.unwrap_or(20).max(1).min(100);
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
        for r in rows { if let Ok(c) = r { out.push(c); } }
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
        let mut stmt = conn.prepare(
            "SELECT title, content, tags FROM agent_memories
             WHERE superseded_by IS NULL
               AND expires_at = 0
               AND created_at < ?1
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
        for r in rows { if let Ok(e) = r {
            if e.tags.len() >= 2 { out.push(e); }
        }}
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
    let lim = limit.unwrap_or(20).max(1).min(100);
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
        for r in rows { if let Ok(i) = r { out.push(i); } }
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
        for r in rows { if let Ok(n) = r { out.push(n); } }
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
    for (_, members) in session_groups.iter() {
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
    let hops = max_hops.unwrap_or(2).max(1).min(4);
    let lim  = limit.unwrap_or(15).max(1).min(50);
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
            for r in rows { if let Ok(e) = r { out.push(e); } }
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
        for r in rows { if let Ok(am) = r { m.insert(am.id, am); } }
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
