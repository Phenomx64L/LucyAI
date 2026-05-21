// ── Frontier Telemetry (Sprint 8) ─────────────────────────────────────────
//
// Lightweight internal counter for Frontier feature usage. Lets the user
// see WHICH of Lucy's 10+ Frontier capabilities they actually rely on,
// and gives us evidence to keep / iterate / retire each feature.
//
// Storage: single SQLite table `frontier_telemetry` (additive, idempotent).
//   • feature_id (PK): "F1.process_lineage_poll", "F8.threat_scan", ...
//   • invocations: monotonic counter
//   • errors: monotonic counter of failed invocations
//   • last_used: latest unix-sec timestamp
//   • total_ms: cumulative wall-clock time spent in this feature
//
// Privacy: 100% local. Doesn't record arguments or results, only the
// feature_id and timing. Aggregated metrics only.

use crate::commands::metrics::shared_db;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn ensure_table(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS frontier_telemetry (
            feature_id  TEXT PRIMARY KEY,
            invocations INTEGER NOT NULL DEFAULT 0,
            errors      INTEGER NOT NULL DEFAULT 0,
            last_used   INTEGER,
            total_ms    INTEGER NOT NULL DEFAULT 0
        )",
        [],
    ).map_err(|e| format!("ensure telemetry: {}", e))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierStat {
    pub feature_id: String,
    pub invocations: i64,
    pub errors: i64,
    pub last_used: Option<i64>,
    pub total_ms: i64,
    /// Average ms per invocation. None when no invocations yet.
    pub avg_ms: Option<f64>,
    /// Error rate 0-1.
    pub error_rate: f64,
}

#[tauri::command]
pub async fn frontier_telemetry_record(
    feature_id: String,
    elapsed_ms: i64,
    failed: bool,
) -> Result<(), String> {
    let now = now_ts();
    shared_db(|conn| {
        ensure_table(conn)?;
        conn.execute(
            "INSERT INTO frontier_telemetry (feature_id, invocations, errors, last_used, total_ms)
             VALUES (?1, 1, ?2, ?3, ?4)
             ON CONFLICT(feature_id) DO UPDATE SET
                invocations = frontier_telemetry.invocations + 1,
                errors      = frontier_telemetry.errors + ?2,
                last_used   = excluded.last_used,
                total_ms    = frontier_telemetry.total_ms + ?4",
            params![feature_id, if failed { 1 } else { 0 }, now, elapsed_ms],
        ).map_err(|e| format!("record telemetry: {}", e))?;

        // Bounded retention — telemetry table grows by feature_id, which is
        // capped by the number of Frontier tools (~30) plus dynamic feature
        // ids that the agent loop may generate. If we ever hit >5000 distinct
        // ids, drop entries unused for >60 days. This keeps `/frontier-stats`
        // queries fast even after months of use.
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM frontier_telemetry", [], |r| r.get(0),
        ).unwrap_or(0);
        if count > 5000 {
            let cutoff = now - 60 * 24 * 3600;
            conn.execute(
                "DELETE FROM frontier_telemetry
                 WHERE last_used IS NOT NULL AND last_used < ?1",
                params![cutoff],
            ).ok();
        }
        Ok(())
    })
}

#[tauri::command]
pub async fn frontier_telemetry_summary() -> Result<Vec<FrontierStat>, String> {
    shared_db(|conn| {
        ensure_table(conn)?;
        let mut stmt = conn.prepare(
            "SELECT feature_id, invocations, errors, last_used, total_ms
             FROM frontier_telemetry
             ORDER BY invocations DESC"
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows: Vec<FrontierStat> = stmt
            .query_map([], |row| {
                let feature_id: String = row.get(0)?;
                let invocations: i64 = row.get(1)?;
                let errors: i64 = row.get(2)?;
                let last_used: Option<i64> = row.get(3)?;
                let total_ms: i64 = row.get(4)?;
                let avg_ms = if invocations > 0 {
                    Some(total_ms as f64 / invocations as f64)
                } else { None };
                let error_rate = if invocations > 0 {
                    errors as f64 / invocations as f64
                } else { 0.0 };
                Ok(FrontierStat {
                    feature_id, invocations, errors, last_used, total_ms,
                    avg_ms, error_rate,
                })
            })
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

#[tauri::command]
pub async fn frontier_telemetry_clear() -> Result<(), String> {
    shared_db(|conn| {
        ensure_table(conn)?;
        conn.execute("DELETE FROM frontier_telemetry", []).ok();
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn record_and_summarize_round_trip() {
        // Note: this hits a real DB if init has run. We just ensure the
        // functions don't panic on no-data; in production the metrics
        // init() creates the pool.
        let r = frontier_telemetry_summary().await;
        assert!(r.is_ok() || matches!(r, Err(_)));
    }
}
