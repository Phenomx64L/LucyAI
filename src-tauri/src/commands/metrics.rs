// ── Cost tracking, permission rules, and skill management commands ──
// Uses rusqlite directly for simpler API access.
//
// Connection model: a single shared Connection lives in a OnceCell<Mutex<_>>,
// initialized at app startup via `init(app)`. All commands borrow it through
// `with_db()`. This avoids opening a new file handle on every Tauri call,
// gives us PRAGMA WAL once, and serializes writes safely.

use crate::utils::db::{TokenUsage, PermissionRule, Skill, generate_id, calculate_cost, INIT_SQL};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use once_cell::sync::OnceCell;
use tauri::{AppHandle, Manager};

static DB: OnceCell<Mutex<rusqlite::Connection>> = OnceCell::new();

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

/// Initialize the shared connection + schema. Call once from the Tauri setup hook.
pub fn init(app: &AppHandle) -> Result<(), String> {
    if DB.get().is_some() {
        return Ok(());
    }
    let path = db_path(app)?;
    let conn = rusqlite::Connection::open(&path)
        .map_err(|e| format!("Failed to open database at {}: {}", path.display(), e))?;

    // Tuning: WAL allows concurrent reads, NORMAL sync trades a tiny crash window
    // for ~10x write throughput, busy_timeout avoids SQLITE_BUSY under contention.
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;\
         PRAGMA synchronous=NORMAL;\
         PRAGMA temp_store=MEMORY;\
         PRAGMA busy_timeout=5000;",
    ).map_err(|e| format!("Failed to set pragmas: {}", e))?;

    // Schema is idempotent (CREATE IF NOT EXISTS) — execute_batch handles
    // multiple statements + line comments natively, no manual splitting needed.
    conn.execute_batch(INIT_SQL)
        .map_err(|e| format!("Failed to initialize schema: {}", e))?;

    DB.set(Mutex::new(conn))
        .map_err(|_| "DB already initialized".to_string())?;
    Ok(())
}

/// Borrow the shared connection. Returns an error if `init()` was not called.
fn with_db<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&rusqlite::Connection) -> Result<R, String>,
{
    let cell = DB.get().ok_or_else(|| "Metrics DB not initialized".to_string())?;
    let guard = cell.lock().map_err(|e| format!("DB mutex poisoned: {}", e))?;
    f(&*guard)
}

/// Back-compat shim: frontend may still invoke this. Schema is already created
/// at startup, so this is a no-op success.
#[tauri::command]
pub async fn init_metrics_db() -> Result<(), String> {
    if DB.get().is_some() { Ok(()) } else { Err("DB not initialized at startup".to_string()) }
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
