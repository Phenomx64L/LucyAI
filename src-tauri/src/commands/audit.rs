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

// v1.7.83 — Audit-trail write batching.
//
// Background:
//   Pre-v1.7.83 each save_audit_entry call ran its own INSERT in its
//   own implicit transaction. With WAL + synchronous=NORMAL that's
//   still ONE fsync to disk per entry — fine in isolation, but during
//   burst sessions (agent loops, multi-host parallel commands, replay
//   re-runs) audit writes pile up at ~10-30 per second. Each fsync is
//   ~1-3 ms on SSD, ~10-30 ms on HDD; the latency stacks visibly.
//
// Strategy:
//   • save_audit_entry returns IMMEDIATELY with a synthetic id
//     (Unix-nanos timestamp). Frontend uses the id only as a key in
//     its reactive store; no caller compares it to a real DB rowid.
//   • Entries land in PENDING (a Mutex<Vec<_>>).
//   • A background flusher (spawned on first call via OnceCell) drains
//     PENDING every 500 ms inside ONE transaction. N entries → 1 fsync
//     instead of N fsyncs.
//   • On shutdown the flusher is a daemon thread — entries pending at
//     exit are lost. Acceptable trade-off for an audit *trail*: the
//     command itself ran, and the worst case is a missing log row
//     within the last 500 ms of the session. The real audit-of-record
//     for compliance lives in hash_chain.rs which is sync.

#[derive(Clone)]
struct PendingAuditEntry {
    timestamp: String,
    host_id: String,
    host_name: String,
    command: String,
    source: String,
    exit_code: Option<i32>,
    duration_ms: Option<i64>,
    output_preview: String,
    user: String,
}

static PENDING_AUDIT: once_cell::sync::Lazy<std::sync::Mutex<Vec<PendingAuditEntry>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(Vec::with_capacity(64)));
static FLUSHER_STARTED: once_cell::sync::OnceCell<()> = once_cell::sync::OnceCell::new();

fn _start_audit_flusher() {
    FLUSHER_STARTED.get_or_init(|| {
        tauri::async_runtime::spawn(async {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                _drain_audit_buffer().await;
            }
        });
    });
}

async fn _drain_audit_buffer() {
    // Take ownership of the current buffer in one swap, release the lock
    // immediately so producers can keep enqueueing while we write.
    let batch: Vec<PendingAuditEntry> = {
        let mut g = match PENDING_AUDIT.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if g.is_empty() { return; }
        std::mem::take(&mut *g)
    };
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let _ = shared_db(|conn| -> Result<(), String> {
            let tx = conn.unchecked_transaction()
                .map_err(|e| format!("audit batch tx open: {}", e))?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO audit_trail (timestamp, host_id, host_name, command, source, exit_code, duration_ms, output_preview, user) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
                ).map_err(|e| format!("audit batch prep: {}", e))?;
                for e in &batch {
                    let _ = stmt.execute(rusqlite::params![
                        e.timestamp, e.host_id, e.host_name, e.command, e.source,
                        e.exit_code, e.duration_ms, e.output_preview, e.user,
                    ]);
                }
            }
            tx.commit().map_err(|e| format!("audit batch commit: {}", e))?;
            Ok(())
        });
    }).await;
}

/// Persist a single audit entry. Called from the frontend after every command
/// execution (local::execute_cmd, hosts::execute_remote_*, shell::execute_powershell, etc.).
///
/// v1.7.83 — fire-and-forget batched write. Returns a synthetic Unix-nanos
/// id immediately; the entry is queued and flushed by the background
/// drainer at most 500 ms later. Frontend uses the id only as a reactive-
/// store key, not a foreign reference.
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
    _start_audit_flusher();
    let synthetic_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    if let Ok(mut g) = PENDING_AUDIT.lock() {
        g.push(PendingAuditEntry {
            timestamp, host_id, host_name, command, source,
            exit_code, duration_ms, output_preview: preview, user,
        });
        // Hard backstop: if a single burst pushes the buffer above 512,
        // flush eagerly so memory doesn't grow without bound during a
        // network outage that's keeping the flusher's spawn_blocking
        // waiting on a busy DB lock.
        if g.len() >= 512 {
            drop(g);
            // Fire a one-shot drain. The periodic flusher continues normally.
            tauri::async_runtime::spawn(async { _drain_audit_buffer().await; });
        }
    }
    Ok(synthetic_id)
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
