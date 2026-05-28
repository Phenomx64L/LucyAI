// ── dashboard_integrations.rs — Lightweight Dashboard data fetchers (Sprint C) ─
//
// Three thin commands the Dashboard polls every few seconds. Each is a
// single fast SQL query — cheaper than reusing full `incident_list` /
// `process_lineage_list` which return the entire payload.
//
// Why a new module: we want < 1 ms per call so the Dashboard's 5-10s
// refresh loop doesn't sweat. Reusing the chunky list commands would
// mean ferrying KBs across IPC every tick for data that distills to a
// single integer in the UI.

use rusqlite::params;
use serde::{Serialize, Deserialize};
use crate::commands::metrics::shared_db;

// ── D15 — Open incidents count per host ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenIncidentsBrief {
    /// Number of incidents currently in status='open' for this host.
    pub open_count: i64,
    /// Title of the most recently-updated open incident (or empty when 0).
    pub latest_title: String,
    /// ID of that incident, so the UI can deep-link.
    pub latest_id: String,
}

/// Lightweight count + headline for the Dashboard's "open incidents" banner.
///
/// Filters by `host_name` to scope to the currently-selected host.
/// Returns `open_count = 0` when there are no open incidents — the UI
/// uses this to hide the banner without a separate "exists" check.
#[tauri::command]
pub async fn dashboard_open_incidents(host_name: String) -> Result<OpenIncidentsBrief, String> {
    shared_db(move |conn| {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM incidents
             WHERE status = 'open' AND (host_name = ?1 OR ?1 = '')",
            params![host_name],
            |r| r.get(0),
        ).unwrap_or(0);

        let mut brief = OpenIncidentsBrief {
            open_count: count,
            latest_title: String::new(),
            latest_id: String::new(),
        };

        if count > 0 {
            // Fetch only the most-recent open incident headline. Keeps the
            // payload tiny — the user clicks through to Incidents view for
            // the full list.
            let headline = conn.query_row(
                "SELECT id, title FROM incidents
                 WHERE status = 'open' AND (host_name = ?1 OR ?1 = '')
                 ORDER BY updated_at DESC LIMIT 1",
                params![host_name],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            ).ok();
            if let Some((id, title)) = headline {
                brief.latest_id = id;
                brief.latest_title = title;
            }
        }
        Ok(brief)
    })
}

// ── D17 — Failed logins (Security event log) ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FailedLoginsBrief {
    /// True if we successfully queried the Security log. False when access
    /// was denied (typical for non-admin Lucy) or on non-Windows hosts.
    pub available: bool,
    /// Count of Event ID 4625 (failed logon attempts) in the last 24h.
    pub count_24h: i64,
    /// Short explanation when unavailable (e.g. "Requires admin").
    pub note: String,
}

/// Failed-logon count from the Windows Security event log (Event ID 4625)
/// in the last 24 hours. Local-host only — for remote hosts the user
/// should run this through NexShell with appropriate credentials.
///
/// Why a separate backend command rather than a Rust syscall: the
/// Security log requires admin on Windows. PowerShell's Get-WinEvent
/// handles the UAC-elevation negotiation correctly and gives us a clean
/// "AccessDenied" we can translate. A raw Windows API call would mean
/// re-implementing all that for marginal speed gain.
///
/// On non-Windows hosts returns `available: false` immediately.
#[tauri::command]
pub async fn dashboard_failed_logins_24h() -> Result<FailedLoginsBrief, String> {
    // Non-Windows: not applicable. We could parse /var/log/auth.log on
    // Linux but that's out of scope for this iteration — the Dashboard
    // is primarily a Windows SRE tool today.
    if !cfg!(target_os = "windows") {
        return Ok(FailedLoginsBrief {
            available: false,
            count_24h: 0,
            note: "Only available on Windows".to_string(),
        });
    }

    tokio::task::spawn_blocking(|| {
        // Get-WinEvent is the modern API (vs the deprecated Get-EventLog).
        // -MaxEvents caps at 500 so even on a server under attack the query
        // returns quickly. We only want the COUNT — pipe through Measure-Object.
        let script = "try {
            $events = Get-WinEvent -FilterHashtable @{LogName='Security'; ID=4625; StartTime=(Get-Date).AddHours(-24)} -MaxEvents 500 -ErrorAction Stop
            ($events | Measure-Object).Count
        } catch [System.UnauthorizedAccessException] {
            'ACCESS_DENIED'
        } catch {
            if ($_.Exception.Message -like '*No events*' -or $_.Exception.Message -like '*returned no results*') { '0' }
            else { 'ERROR:' + $_.Exception.Message }
        }";
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output();
        let raw = match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(e) => return Ok(FailedLoginsBrief {
                available: false,
                count_24h: 0,
                note: format!("PowerShell spawn failed: {}", e),
            }),
        };
        // Take only the last non-empty line — PowerShell can prepend warnings
        // about profile loading even with -NoProfile (rare).
        let last = raw.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
        if last == "ACCESS_DENIED" {
            return Ok(FailedLoginsBrief {
                available: false,
                count_24h: 0,
                note: "Requires admin to read Security log".to_string(),
            });
        }
        if let Some(err_msg) = last.strip_prefix("ERROR:") {
            return Ok(FailedLoginsBrief {
                available: false,
                count_24h: 0,
                note: err_msg.chars().take(120).collect(),
            });
        }
        // Plain integer expected.
        match last.parse::<i64>() {
            Ok(n) => Ok(FailedLoginsBrief {
                available: true,
                count_24h: n,
                note: String::new(),
            }),
            Err(_) => Ok(FailedLoginsBrief {
                available: false,
                count_24h: 0,
                note: format!("Unexpected output: {}", last.chars().take(60).collect::<String>()),
            }),
        }
    })
    .await
    .map_err(|e| format!("Task join: {}", e))?
}

// ── D18 — Process lineage badges: "new in last N hours" ──────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcessLineageBriefRow {
    /// PID we matched against process_lineage. Same PID can re-appear
    /// across boots; the table dedupes by `chain_hash`, but the badge
    /// only cares "is this process less than 24h old in our records?".
    pub pid: i64,
    /// Unix seconds when process_lineage first saw this exe path.
    pub first_seen: i64,
    /// True when first_seen >= now - 24h. Drives the green "new" badge.
    pub is_new_24h: bool,
}

/// Given a list of PIDs the Dashboard is about to display, return a
/// per-PID summary from process_lineage. Lets the Dashboard show a
/// "● new" badge next to processes that appeared in the last 24h.
///
/// Why not do it row-by-row inside top_processes JSON: the sysinfo dump
/// doesn't know the lineage history. This command is the bridge.
/// One IN(...) query for N rows < 1ms.
#[tauri::command]
pub async fn dashboard_process_lineage_brief(
    pids: Vec<i64>,
) -> Result<Vec<ProcessLineageBriefRow>, String> {
    if pids.is_empty() { return Ok(Vec::new()); }
    // SQLite limit is 999 params per query; we cap at 50 for the Dashboard
    // (top_processes is small). Anything beyond suggests a caller bug.
    let pid_slice: Vec<i64> = pids.iter().take(50).copied().collect();

    shared_db(move |conn| {
        // Build a parameterized IN clause. We need one ? placeholder per
        // pid since rusqlite doesn't expand a Vec into placeholders for us.
        let placeholders: String = (0..pid_slice.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT pid, MIN(first_seen) AS first_seen
             FROM process_lineage
             WHERE pid IN ({})
             GROUP BY pid",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)
            .map_err(|e| format!("prepare: {}", e))?;
        let params_vec: Vec<&dyn rusqlite::ToSql> = pid_slice.iter()
            .map(|p| p as &dyn rusqlite::ToSql)
            .collect();
        let now = chrono::Local::now().timestamp();
        let cutoff = now - 24 * 3600;
        let rows: Vec<ProcessLineageBriefRow> = stmt
            .query_map(
                rusqlite::params_from_iter(params_vec.iter().copied()),
                |r| {
                    let pid: i64 = r.get(0)?;
                    let first_seen: i64 = r.get(1)?;
                    Ok(ProcessLineageBriefRow {
                        pid,
                        first_seen,
                        is_new_24h: first_seen >= cutoff,
                    })
                },
            )
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}
