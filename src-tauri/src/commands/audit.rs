// ── AUDIT TRAIL — Persistent command audit log (P0 Feature 1) ────────────────
//
// Replaces the localStorage-backed auditTrail store with durable SQLite storage.
// Every command executed through Lucy (local, remote, AI, runbook, broadcast) is
// logged here with timestamp, host, exit code, duration, and output preview.
//
// Query supports filtering by host, source, date range, and full-text search.

use serde::{Deserialize, Serialize};
use crate::commands::metrics::shared_db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditQueryResult {
    pub entries: Vec<crate::utils::db::AuditEntry>,
    pub total_count: i64,
    pub has_more: bool,
}

/// Persist a single audit entry. Called from the frontend after every command
/// execution (local::execute_cmd, hosts::execute_remote_*, shell::execute_powershell, etc.)
#[tauri::command]
pub async fn save_audit_entry(
    timestamp: String,
    host_id: String,
    host_name: String,
    command: String,
    source: String,
    exit_code: Option<i32>,
    duration_ms: Option<i64>,
    output_preview: String,
    user: String,
) -> Result<i64, String> {
    // Truncate output_preview to 500 chars max to prevent bloat
    let preview = if output_preview.len() > 500 {
        format!("{}…", &output_preview[..499])
    } else {
        output_preview
    };

    tokio::task::spawn_blocking(move || {
        shared_db(|conn| {
            conn.execute(
                "INSERT INTO audit_trail (timestamp, host_id, host_name, command, source, exit_code, duration_ms, output_preview, user)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![timestamp, host_id, host_name, command, source, exit_code, duration_ms, preview, user],
            )
            .map_err(|e| format!("Failed to insert audit entry: {}", e))?;
            Ok(conn.last_insert_rowid())
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Query audit trail with optional filters: host, source, date range, search term.
/// Returns paginated results (limit + offset). Default limit = 100, max = 1000.
#[tauri::command]
pub async fn query_audit_trail(
    host_id: Option<String>,
    source: Option<String>,
    search: Option<String>,
    from_ts: Option<i64>,
    to_ts: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<AuditQueryResult, String> {
    tokio::task::spawn_blocking(move || {
        shared_db(|conn| {
            let lim = limit.unwrap_or(100).min(1000);
            let off = offset.unwrap_or(0).max(0);

            // Build dynamic WHERE clause
            let mut conditions: Vec<String> = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(ref h) = host_id {
                conditions.push(format!("host_id = ?{}", params.len() + 1));
                params.push(Box::new(h.clone()));
            }
            if let Some(ref s) = source {
                conditions.push(format!("source = ?{}", params.len() + 1));
                params.push(Box::new(s.clone()));
            }
            if let Some(ts) = from_ts {
                conditions.push(format!("created_at >= ?{}", params.len() + 1));
                params.push(Box::new(ts));
            }
            if let Some(ts) = to_ts {
                conditions.push(format!("created_at <= ?{}", params.len() + 1));
                params.push(Box::new(ts));
            }
            if let Some(ref q) = search {
                conditions.push(format!(
                    "(command LIKE ?{} OR output_preview LIKE ?{})",
                    params.len() + 1,
                    params.len() + 2
                ));
                let like = format!("%{}%", q);
                params.push(Box::new(like.clone()));
                params.push(Box::new(like));
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            // Count total matching
            let count_sql = format!("SELECT COUNT(*) FROM audit_trail {}", where_clause);
            let total_count: i64 = conn
                .query_row(
                    &count_sql,
                    rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                    |row| row.get(0),
                )
                .map_err(|e| format!("Count query failed: {}", e))?;

            // Fetch rows
            let select_sql = format!(
                "SELECT id, timestamp, host_id, host_name, command, source, exit_code, duration_ms, output_preview, user, created_at \
                 FROM audit_trail {} ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
                where_clause,
                params.len() + 1,
                params.len() + 2
            );
            params.push(Box::new(lim));
            params.push(Box::new(off));

            let mut stmt = conn.prepare(&select_sql)
                .map_err(|e| format!("Prepare failed: {}", e))?;
            let entries: Vec<crate::utils::db::AuditEntry> = stmt
                .query_map(
                    rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                    |row| {
                        Ok(crate::utils::db::AuditEntry {
                            id:             row.get(0)?,
                            timestamp:      row.get(1)?,
                            host_id:        row.get(2)?,
                            host_name:      row.get(3)?,
                            command:        row.get(4)?,
                            source:         row.get(5)?,
                            exit_code:      row.get(6)?,
                            duration_ms:    row.get(7)?,
                            output_preview: row.get(8)?,
                            user:           row.get(9)?,
                            created_at:     row.get(10)?,
                        })
                    },
                )
                .map_err(|e| format!("Query failed: {}", e))?
                .filter_map(|r| r.ok())
                .collect();

            let has_more = (off + lim) < total_count;

            Ok(AuditQueryResult {
                entries,
                total_count,
                has_more,
            })
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Delete audit entries older than `days` days. Returns count of deleted rows.
/// Default: 90 days. Called by periodic janitor or manually from settings.
#[tauri::command]
pub async fn prune_audit_trail(days: Option<i64>) -> Result<i64, String> {
    let cutoff_days = days.unwrap_or(90).max(1);
    tokio::task::spawn_blocking(move || {
        shared_db(|conn| {
            let deleted = conn
                .execute(
                    "DELETE FROM audit_trail WHERE created_at < (strftime('%s','now') - ?1 * 86400)",
                    rusqlite::params![cutoff_days],
                )
                .map_err(|e| format!("Prune failed: {}", e))?;
            Ok(deleted as i64)
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Get audit statistics: total entries, entries per source, entries per host.
#[tauri::command]
pub async fn audit_stats() -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(|| {
        shared_db(|conn| {
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM audit_trail", [], |r| r.get(0))
                .unwrap_or(0);

            let mut by_source = serde_json::Map::new();
            {
                let mut stmt = conn
                    .prepare("SELECT source, COUNT(*) FROM audit_trail GROUP BY source ORDER BY COUNT(*) DESC")
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                    .map_err(|e| e.to_string())?;
                for row in rows.flatten() {
                    by_source.insert(row.0, serde_json::Value::Number(row.1.into()));
                }
            }

            let mut by_host = serde_json::Map::new();
            {
                let mut stmt = conn
                    .prepare("SELECT host_name, COUNT(*) FROM audit_trail GROUP BY host_name ORDER BY COUNT(*) DESC LIMIT 20")
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                    .map_err(|e| e.to_string())?;
                for row in rows.flatten() {
                    by_host.insert(row.0, serde_json::Value::Number(row.1.into()));
                }
            }

            Ok(serde_json::json!({
                "total": total,
                "by_source": by_source,
                "by_host": by_host,
            }))
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}
