// ── principles.rs — behavioral rules injected into Lucy's system prompt
//
// Adapted from the Maestro project's "principles" concept. Each Principle
// is a short instruction the agent reads BEFORE generating a turn, e.g.:
//
//   "Always run a Get-* validation before any Set-* operation."
//   "Prefer PowerShell 7 over 5 when both are present."
//   "On host PROD-AD-01: NEVER restart W3SVC during business hours."
//
// Stored in the existing metrics SQLite. CRUD via Tauri commands. The
// runtime renderer (render_principles_block) is called once per turn and
// produces a compact text block the prompt builder can paste.

use serde::{Deserialize, Serialize};
use crate::commands::metrics::shared_db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principle {
    pub id:         i64,
    pub name:       String,
    pub rule:       String,
    pub scope:      Option<String>,   // None = global, else host id / project tag
    pub priority:   i64,
    pub enabled:    bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Save a new principle. Returns the new id.
#[tauri::command]
pub fn save_principle(
    name:     String,
    rule:     String,
    scope:    Option<String>,
    priority: Option<i64>,
) -> Result<i64, String> {
    if name.trim().is_empty() || rule.trim().is_empty() {
        return Err("save_principle: name and rule are required".to_string());
    }
    if name.len() > 120 || rule.len() > 1000 {
        return Err("save_principle: name <= 120 chars, rule <= 1000 chars".to_string());
    }
    let pri = priority.unwrap_or(100).max(1).min(1000);
    shared_db(|conn| {
        conn.execute(
            "INSERT INTO principles (name, rule, scope, priority)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![name, rule, scope, pri],
        ).map_err(|e| format!("save_principle: {}", e))?;
        Ok(conn.last_insert_rowid())
    })
}

/// Update an existing principle. Any field omitted keeps its prior value.
#[tauri::command]
pub fn update_principle(
    id:       i64,
    name:     Option<String>,
    rule:     Option<String>,
    scope:    Option<Option<String>>,    // double-Option: None = leave alone, Some(None) = clear, Some(Some(s)) = set
    priority: Option<i64>,
    enabled:  Option<bool>,
) -> Result<usize, String> {
    if id <= 0 { return Err("update_principle: invalid id".to_string()); }
    shared_db(|conn| {
        // Build a single UPDATE with COALESCE(?,col) so callers can patch any subset.
        // The double-Option for scope can't go through COALESCE cleanly; if scope
        // is provided we issue a separate UPDATE for it.
        let n = conn.execute(
            "UPDATE principles SET
                 name       = COALESCE(?2, name),
                 rule       = COALESCE(?3, rule),
                 priority   = COALESCE(?4, priority),
                 enabled    = COALESCE(?5, enabled),
                 updated_at = strftime('%s','now')
             WHERE id = ?1",
            rusqlite::params![
                id,
                name,
                rule,
                priority,
                enabled.map(|b| if b { 1 } else { 0 }),
            ],
        ).map_err(|e| format!("update_principle: {}", e))?;
        if let Some(scope_outer) = scope {
            conn.execute(
                "UPDATE principles SET scope = ?2, updated_at = strftime('%s','now') WHERE id = ?1",
                rusqlite::params![id, scope_outer],
            ).map_err(|e| format!("update_principle scope: {}", e))?;
        }
        Ok(n)
    })
}

/// Delete a principle by id.
#[tauri::command]
pub fn delete_principle(id: i64) -> Result<usize, String> {
    if id <= 0 { return Err("delete_principle: invalid id".to_string()); }
    shared_db(|conn| {
        let n = conn.execute(
            "DELETE FROM principles WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| format!("delete_principle: {}", e))?;
        Ok(n)
    })
}

/// List all principles, optionally filtered by scope.
/// scope = None  → returns ALL (both global and scoped) so the UI can browse.
/// scope = Some  → returns global + matching scope (what the prompt builder uses).
#[tauri::command]
pub fn list_principles(scope: Option<String>) -> Result<Vec<Principle>, String> {
    shared_db(|conn| {
        let (sql, params): (&str, Vec<rusqlite::types::Value>) = match scope {
            Some(s) => (
                "SELECT id, name, rule, scope, priority, enabled, created_at, updated_at
                 FROM principles
                 WHERE enabled = 1 AND (scope IS NULL OR scope = ?1)
                 ORDER BY priority ASC, id ASC",
                vec![rusqlite::types::Value::from(s)],
            ),
            None => (
                "SELECT id, name, rule, scope, priority, enabled, created_at, updated_at
                 FROM principles
                 ORDER BY enabled DESC, priority ASC, id ASC",
                vec![],
            ),
        };
        let mut stmt = conn.prepare(sql).map_err(|e| format!("list_principles prepare: {}", e))?;
        let params_ref: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            Ok(Principle {
                id:         row.get(0)?,
                name:       row.get(1)?,
                rule:       row.get(2)?,
                scope:      row.get(3)?,
                priority:   row.get(4)?,
                enabled:    row.get::<_, i64>(5)? != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        }).map_err(|e| format!("list_principles query: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("list_principles row: {}", e))?);
        }
        Ok(out)
    })
}

/// Render the active principles as a compact prompt block. Used by ai.rs
/// to inject into every turn. Empty string if nothing applies.
///
///   --- ACTIVE PRINCIPLES (4 rules) ---
///   [P1] Validate with Get-* before any Set-*.
///   [P2] Prefer PowerShell 7 over 5 when both are present.
///   ...
///
/// Sync (no async) so it can be called from the synchronous prompt builder.
pub fn render_principles_block(scope: Option<&str>) -> String {
    let scope_owned = scope.map(|s| s.to_string());
    let principles = match list_principles(scope_owned) {
        Ok(p) => p,
        Err(_) => return String::new(),
    };
    let active: Vec<&Principle> = principles.iter().filter(|p| p.enabled).collect();
    if active.is_empty() {
        return String::new();
    }
    let mut out = format!("\n\n--- ACTIVE PRINCIPLES ({} rule{}) ---\n",
        active.len(),
        if active.len() == 1 { "" } else { "s" });
    for (idx, p) in active.iter().enumerate() {
        let scope_tag = match &p.scope {
            Some(s) if !s.is_empty() => format!(" (scope: {})", s),
            _ => String::new(),
        };
        out.push_str(&format!("[P{}]{} {}\n", idx + 1, scope_tag, p.rule.trim()));
    }
    out.push_str("(These principles override defaults when they apply. Follow them silently — don't restate them in your answer.)\n");
    out
}
