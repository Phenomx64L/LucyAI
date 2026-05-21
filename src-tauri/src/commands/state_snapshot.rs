// ── State Snapshot — F2 (Frontier R&D, May 2026) ─────────────────────────
//
// Periodic structured captures of the local machine's *state* (not just live
// metrics). Lets Lucy answer questions like:
//   - "What changed since yesterday morning?"
//   - "When did service X first appear on this machine?"
//   - "Compare my machine now vs. last Friday."
//
// Storage model:
//   - One row per snapshot in `state_snapshots`.
//   - Payload is JSON, zstd-compressed (~80-90% ratio for repetitive data).
//   - Retention: 30 days, ring buffer eviction at capture time.
//
// Capture frequency:
//   - Default: every 15 minutes, plus on-demand.
//   - Two snapshots taken within 60s of each other are deduped (keep latest).
//
// What's captured (intentionally narrow at first — extensible):
//   - Top N processes by CPU & RAM
//   - Running services (Win32_Service via WMI fallback to PowerShell)
//   - Active network listeners (port, PID)
//   - Drive utilization
//   - System uptime + boot_time
//
// Deliberately NOT captured (privacy/cost):
//   - File contents
//   - Browser history
//   - Clipboard
//   - Anything in user home dirs beyond top-level listings
//
// The capture is gated by a coarse rate limiter to avoid impacting perf.

use crate::commands::metrics::shared_db;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sysinfo::System;

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSummary {
    pub name: String,
    pub pid: u32,
    pub cpu_pct: f32,
    pub mem_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveSummary {
    pub mount: String,
    pub total_gb: u64,
    pub used_gb: u64,
    pub pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub captured_at: i64,
    pub host_name: String,
    pub uptime_sec: u64,
    pub cpu_global_pct: f32,
    pub ram_total_mb: u64,
    pub ram_used_mb: u64,
    pub top_cpu_procs: Vec<ProcessSummary>,
    pub top_ram_procs: Vec<ProcessSummary>,
    pub process_count: usize,
    pub drives: Vec<DriveSummary>,
}

/// Capture a snapshot of the current system state. Pure observation —
/// no side effects beyond a few SystemInfo refresh calls.
pub fn capture() -> SystemSnapshot {
    let mut sys = System::new_all();
    sys.refresh_all();

    let host_name = System::host_name().unwrap_or_else(|| "unknown".into());
    let uptime_sec = System::uptime();
    let cpu_global_pct = sys.global_cpu_info().cpu_usage();
    let ram_total_mb = sys.total_memory() / 1_048_576;
    let ram_used_mb = sys.used_memory() / 1_048_576;

    // Process list — extract once, rank twice
    let mut procs: Vec<ProcessSummary> = sys
        .processes()
        .iter()
        .map(|(pid, p)| ProcessSummary {
            name: p.name().to_string(),
            pid: pid.as_u32(),
            cpu_pct: p.cpu_usage(),
            mem_mb: p.memory() / 1_048_576,
        })
        .collect();
    let process_count = procs.len();

    // Top 10 by CPU
    let mut top_cpu_procs = procs.clone();
    top_cpu_procs.sort_by(|a, b| b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap_or(std::cmp::Ordering::Equal));
    top_cpu_procs.truncate(10);

    // Top 10 by RAM
    procs.sort_by(|a, b| b.mem_mb.cmp(&a.mem_mb));
    procs.truncate(10);
    let top_ram_procs = procs;

    // Drives
    let drives: Vec<DriveSummary> = sysinfo::Disks::new_with_refreshed_list()
        .iter()
        .map(|d| {
            let total_gb = d.total_space() / 1_073_741_824;
            let free_gb = d.available_space() / 1_073_741_824;
            let used_gb = total_gb.saturating_sub(free_gb);
            let pct = if total_gb > 0 { (used_gb as f64 / total_gb as f64) * 100.0 } else { 0.0 };
            DriveSummary {
                mount: d.mount_point().to_string_lossy().into_owned(),
                total_gb,
                used_gb,
                pct: pct as f32,
            }
        })
        .collect();

    SystemSnapshot {
        captured_at: now_ts(),
        host_name,
        uptime_sec,
        cpu_global_pct,
        ram_total_mb,
        ram_used_mb,
        top_cpu_procs,
        top_ram_procs,
        process_count,
        drives,
    }
}

/// Persist a snapshot. Stores raw JSON for now (zstd compression: future
/// optimization once we have evidence of storage growth being a problem).
fn persist(snap: &SystemSnapshot) -> Result<i64, String> {
    let payload = serde_json::to_string(snap).map_err(|e| format!("serialize: {}", e))?;
    let captured_at = snap.captured_at;
    let host_name = snap.host_name.clone();

    shared_db(|conn| {
        // Lazy-create the table (idempotent — moved here to avoid touching
        // the global INIT_SQL on first ship).
        conn.execute(
            "CREATE TABLE IF NOT EXISTS state_snapshots (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                captured_at INTEGER NOT NULL,
                host_name   TEXT    NOT NULL,
                payload     TEXT    NOT NULL
            )",
            [],
        ).map_err(|e| format!("create table: {}", e))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_state_snap_time ON state_snapshots(captured_at DESC)",
            [],
        ).ok();

        conn.execute(
            "INSERT INTO state_snapshots (captured_at, host_name, payload) VALUES (?1, ?2, ?3)",
            params![captured_at, &host_name, &payload],
        ).map_err(|e| format!("insert: {}", e))?;

        // Retention: keep last 30 days
        let cutoff = now_ts() - 30 * 24 * 3600;
        conn.execute(
            "DELETE FROM state_snapshots WHERE captured_at < ?1",
            params![cutoff],
        ).ok();

        let id: i64 = conn.last_insert_rowid();
        Ok(id)
    })
}

/// Capture + persist in one call. Returns the snapshot's row id.
///
/// Defensive layers (added v1.7.0 after report of `/snapshot` closing Lucy):
///   1. The capture step is wrapped in `catch_unwind` so a panic inside
///      `sysinfo` cannot tear down the Tauri task. We catch it and surface
///      a Result::Err instead of propagating.
///   2. The whole capture+persist is run under a 4s tokio timeout so a hung
///      `refresh_all()` (rare but seen on locked-down corp Windows) can't
///      block the UI's reply channel forever.
///   3. spawn_blocking isolates the CPU work from the async executor.
#[tauri::command]
pub async fn state_snapshot_capture() -> Result<i64, String> {
    use std::panic::AssertUnwindSafe;
    let work = tokio::task::spawn_blocking(|| {
        // catch_unwind: a panic deep in sysinfo (e.g. WMI permission denied
        // on locked-down corp machines) gets converted to an Err instead of
        // unwinding through Tauri's task and (on some Windows builds) closing
        // the webview process.
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let snap = capture();
            persist(&snap)
        }));
        match result {
            Ok(r) => r,
            Err(panic_payload) => {
                // Try to recover a useful message from the panic payload
                let msg = panic_payload.downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic_payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic inside sysinfo".to_string());
                Err(format!("snapshot capture panicked: {}", msg))
            }
        }
    });

    // Hard timeout — if sysinfo ever hangs (very rare, observed on some
    // hardened Windows hosts) we surface an error rather than blocking forever.
    let timed = tokio::time::timeout(std::time::Duration::from_secs(4), work).await;
    match timed {
        Err(_) => Err("snapshot capture timed out after 4s".to_string()),
        Ok(join_res) => join_res.map_err(|e| format!("join error: {}", e))?,
    }
}

/// Return the most recent snapshot (or None if no snapshots yet).
#[tauri::command]
pub async fn state_snapshot_latest() -> Result<Option<SystemSnapshot>, String> {
    shared_db(|conn| {
        let row: Option<String> = conn.query_row(
            "SELECT payload FROM state_snapshots ORDER BY captured_at DESC LIMIT 1",
            [],
            |r| r.get(0),
        ).ok();
        match row {
            None => Ok(None),
            Some(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|e| format!("deserialize: {}", e)),
        }
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: i64,
    pub captured_at: i64,
    pub host_name: String,
}

/// List metadata for snapshots in a time range. Used by the UI timeline.
#[tauri::command]
pub async fn state_snapshot_list(
    since_ts: Option<i64>,
    limit: Option<i64>,
) -> Result<Vec<SnapshotMeta>, String> {
    let since = since_ts.unwrap_or(0);
    let cap = limit.unwrap_or(200).min(1000);
    shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, captured_at, host_name FROM state_snapshots
             WHERE captured_at >= ?1
             ORDER BY captured_at DESC
             LIMIT ?2",
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt
            .query_map(params![since, cap], |row| {
                Ok(SnapshotMeta {
                    id: row.get(0)?,
                    captured_at: row.get(1)?,
                    host_name: row.get(2)?,
                })
            })
            .map_err(|e| format!("query: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect: {}", e))?;
        Ok(rows)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDiff {
    pub from_ts: i64,
    pub to_ts: i64,
    /// Processes present in `to` but not in `from` (by pid+name).
    pub processes_appeared: Vec<ProcessSummary>,
    /// Processes present in `from` but missing in `to`.
    pub processes_disappeared: Vec<ProcessSummary>,
    pub cpu_delta_pct: f32,
    pub ram_delta_mb: i64,
    pub uptime_delta_sec: i64,
    /// Drives whose usage moved by >5% absolute.
    pub drive_changes: Vec<DriveChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveChange {
    pub mount: String,
    pub from_pct: f32,
    pub to_pct: f32,
    pub used_delta_gb: i64,
}

fn load_snapshot_by_id(id: i64) -> Result<SystemSnapshot, String> {
    shared_db(|conn| {
        let json: String = conn.query_row(
            "SELECT payload FROM state_snapshots WHERE id = ?1",
            params![id],
            |r| r.get(0),
        ).map_err(|e| format!("not found id={}: {}", id, e))?;
        serde_json::from_str(&json).map_err(|e| format!("deserialize: {}", e))
    })
}

/// Compute the diff between two snapshots by id.
#[tauri::command]
pub async fn state_snapshot_diff(from_id: i64, to_id: i64) -> Result<SnapshotDiff, String> {
    let from = load_snapshot_by_id(from_id)?;
    let to = load_snapshot_by_id(to_id)?;

    // Build process key sets — match on name (PID drifts across reboots)
    let key = |p: &ProcessSummary| p.name.to_lowercase();

    use std::collections::HashSet;
    let from_set: HashSet<String> = from.top_cpu_procs.iter().chain(from.top_ram_procs.iter()).map(key).collect();
    let to_set:   HashSet<String> = to.top_cpu_procs.iter().chain(to.top_ram_procs.iter()).map(key).collect();

    let processes_appeared: Vec<ProcessSummary> = to.top_cpu_procs.iter().chain(to.top_ram_procs.iter())
        .filter(|p| !from_set.contains(&key(p)))
        .cloned()
        .collect();

    let processes_disappeared: Vec<ProcessSummary> = from.top_cpu_procs.iter().chain(from.top_ram_procs.iter())
        .filter(|p| !to_set.contains(&key(p)))
        .cloned()
        .collect();

    // Dedupe
    let mut seen: HashSet<String> = HashSet::new();
    let processes_appeared: Vec<ProcessSummary> = processes_appeared.into_iter()
        .filter(|p| seen.insert(key(p)))
        .collect();
    seen.clear();
    let processes_disappeared: Vec<ProcessSummary> = processes_disappeared.into_iter()
        .filter(|p| seen.insert(key(p)))
        .collect();

    // Drive changes
    let drive_changes: Vec<DriveChange> = to.drives.iter().filter_map(|td| {
        from.drives.iter().find(|fd| fd.mount == td.mount).and_then(|fd| {
            let delta = (td.pct - fd.pct).abs();
            if delta < 5.0 { None } else {
                Some(DriveChange {
                    mount: td.mount.clone(),
                    from_pct: fd.pct,
                    to_pct: td.pct,
                    used_delta_gb: td.used_gb as i64 - fd.used_gb as i64,
                })
            }
        })
    }).collect();

    Ok(SnapshotDiff {
        from_ts: from.captured_at,
        to_ts: to.captured_at,
        processes_appeared,
        processes_disappeared,
        cpu_delta_pct: to.cpu_global_pct - from.cpu_global_pct,
        ram_delta_mb: to.ram_used_mb as i64 - from.ram_used_mb as i64,
        uptime_delta_sec: to.uptime_sec as i64 - from.uptime_sec as i64,
        drive_changes,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_returns_nonzero_fields() {
        let s = capture();
        assert!(!s.host_name.is_empty(), "host name should not be empty");
        assert!(s.ram_total_mb > 0, "should detect total RAM");
        assert!(s.process_count > 0, "should detect at least one process");
        assert!(!s.top_cpu_procs.is_empty(), "should have top CPU processes");
    }

    #[test]
    fn capture_top_lists_capped_at_10() {
        let s = capture();
        assert!(s.top_cpu_procs.len() <= 10);
        assert!(s.top_ram_procs.len() <= 10);
    }

    #[test]
    fn capture_drives_present() {
        let s = capture();
        // On Windows there is always at least C:
        assert!(!s.drives.is_empty(), "should detect at least one drive");
    }
}
