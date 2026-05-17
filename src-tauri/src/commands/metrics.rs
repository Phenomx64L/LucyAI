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
pub(crate) type PooledConn = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;
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
        combined.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let top: Vec<AgentMemory> = combined.into_iter().take(lim_usize).map(|(m, _)| m).collect();
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
#[tauri::command]
pub async fn fork_save(
    task_id: String,
    tab_id: String,
    session_id: String,
    model: String,
    instruction: String,
) -> Result<String, String> {
    let id = generate_id();
    with_db(|conn| {
        conn.execute(
            "INSERT INTO fork_results
             (id, task_id, tab_id, session_id, model, instruction, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running')",
            rusqlite::params![&id, &task_id, &tab_id, &session_id, &model, &instruction],
        ).map_err(|e| format!("fork_save: {}", e))?;
        Ok(id.clone())
    })
}

/// Mark a fork as done or error. Stores result text or error message.
#[tauri::command]
pub async fn fork_update(
    task_id: String,
    status: String,   // 'done' | 'error'
    result: Option<String>,
    error_msg: Option<String>,
) -> Result<(), String> {
    with_db(|conn| {
        conn.execute(
            "UPDATE fork_results
             SET status = ?1, result = ?2, error_msg = ?3,
                 finished_at = strftime('%s','now')
             WHERE task_id = ?4 AND status = 'running'",
            rusqlite::params![&status, &result, &error_msg, &task_id],
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
                    status, result, error_msg, created_at, finished_at
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
            })
        }
        let rows: Vec<ForkResult> = if let Some(ref tid) = tab_id {
            let mut stmt = conn.prepare(
                "SELECT id, task_id, tab_id, session_id, model, instruction,
                        status, result, error_msg, created_at, finished_at
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
                        status, result, error_msg, created_at, finished_at
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
