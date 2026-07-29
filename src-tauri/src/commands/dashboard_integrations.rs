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

// ── Shared PowerShell runner for the Dashboard probes ────────────────────

/// Run a PowerShell script and return its stdout, correctly decoded.
///
/// Centralises the two things every Dashboard probe needs and one of them
/// silently got wrong for a long time.
///
/// **Encoding.** Lucy is a GUI process with no console, so a PowerShell it
/// spawns writes to the pipe in the system OEM code page — CP-850 on a
/// Spanish install, where `ó` is the single byte 0xA2. That is not valid
/// UTF-8, so `String::from_utf8_lossy` replaced it with U+FFFD and the
/// Dashboard displayed "selecci<?>n especificados". Forcing
/// `[Console]::OutputEncoding` before the payload runs is the same fix
/// `shell.rs` already applies to the main execution engine (its wrapper does
/// this at line ~359); the Dashboard's own spawns never inherited it.
///
/// The assignment must come BEFORE the payload: PowerShell fixes a stream's
/// encoding when it first writes to it, so setting it afterwards is too late.
///
/// **CREATE_NO_WINDOW.** Without it every refresh flashes a console window.
fn run_powershell_utf8(script: &str) -> Result<String, String> {
    crate::utils::shell::run_powershell_utf8(script).map(|(stdout, _, _)| stdout)
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
///
/// Get-WinEvent is the modern API (vs the deprecated Get-EventLog). -MaxEvents
/// caps at 500 so even a server under attack answers quickly; we only want the
/// COUNT, hence Measure-Object.
///
/// THE TRAP: `Get-WinEvent` THROWS when a filter matches nothing — it does not
/// return 0. So the "everything is fine" case arrives as an exception and has
/// to be told apart from a real failure. This used to be done by matching the
/// exception TEXT against the English 'No events' / 'returned no results'.
/// Windows localises that message, so on a Spanish install it reads "No se
/// encontraron eventos que coincidan con los criterios de selección
/// especificados", matched neither pattern, and fell through to the error
/// branch. The result: a machine with a perfectly readable log and ZERO failed
/// logons — the healthiest possible outcome — reported "Registro de seguridad
/// no legible". The bug was invisible in English and guaranteed everywhere else.
///
/// `FullyQualifiedErrorId` is the locale-independent discriminator and is what
/// we match on now. Never match on `$_.Exception.Message`.
///
/// Note also that reading the Security log does NOT always require elevation:
/// verified on a non-elevated shell that a user with the right group membership
/// reads it fine. Treat ACCESS_DENIED as one possible answer, not the default.
const FAILED_LOGINS_COUNT_SCRIPT: &str = r#"try {
    $events = Get-WinEvent -FilterHashtable @{LogName='Security'; ID=4625; StartTime=(Get-Date).AddHours(-24)} -MaxEvents 500 -ErrorAction Stop
    ($events | Measure-Object).Count
} catch {
    if ($_.FullyQualifiedErrorId -like 'NoMatchingEventsFound*') { '0' }
    elseif ($_.Exception -is [System.UnauthorizedAccessException] -or $_.FullyQualifiedErrorId -like '*UnauthorizedAccess*') { 'ACCESS_DENIED' }
    else { 'ERROR:' + $_.Exception.Message }
}"#;

/// Turn the script's single-token output into a brief.
///
/// Split out from the spawn so the mapping is unit-testable without a Windows
/// host: the shapes it must handle are exactly the tokens the script emits.
fn classify_failed_logins_output(raw: &str) -> FailedLoginsBrief {
    // Last non-empty line only — PowerShell can prepend profile warnings even
    // with -NoProfile (rare, but it has happened).
    let last = raw
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();

    if last == "ACCESS_DENIED" {
        return FailedLoginsBrief {
            available: false,
            count_24h: 0,
            note: "Requires admin to read Security log".to_string(),
        };
    }
    if let Some(err_msg) = last.strip_prefix("ERROR:") {
        return FailedLoginsBrief {
            available: false,
            count_24h: 0,
            note: err_msg.chars().take(120).collect(),
        };
    }
    match last.parse::<i64>() {
        Ok(n) => FailedLoginsBrief { available: true, count_24h: n, note: String::new() },
        Err(_) => FailedLoginsBrief {
            available: false,
            count_24h: 0,
            note: format!("Unexpected output: {}", last.chars().take(60).collect::<String>()),
        },
    }
}

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
        let raw = match run_powershell_utf8(FAILED_LOGINS_COUNT_SCRIPT) {
            Ok(s) => s,
            Err(e) => return Ok(FailedLoginsBrief {
                available: false,
                count_24h: 0,
                note: e,
            }),
        };
        Ok(classify_failed_logins_output(&raw))
    })
    .await
    .map_err(|e| format!("Task join: {}", e))?
}

// ── Failed-logins DRILL-DOWN — the actual 4625 events for threat hunting ──

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FailedLoginEvent {
    pub time:        String,
    pub user:        String,
    pub source_ip:   String,
    pub workstation: String,
    pub logon_type:  String,
}

/// Return the last (up to 50) Security 4625 events in the past 24h with the
/// fields a SysAdmin actually wants: when, which account was targeted, the
/// source IP and workstation, and the logon type. Windows-only; empty list
/// elsewhere or on access-denied.
#[tauri::command]
pub async fn dashboard_failed_logins_detail() -> Result<Vec<FailedLoginEvent>, String> {
    if !cfg!(target_os = "windows") {
        return Ok(vec![]);
    }
    tokio::task::spawn_blocking(|| {
        // Parse each event's XML so we can pull TargetUserName / IpAddress /
        // WorkstationName / LogonType, then emit compact JSON for Rust to read.
        let script = r#"try {
            $ev = Get-WinEvent -FilterHashtable @{LogName='Security'; ID=4625; StartTime=(Get-Date).AddHours(-24)} -MaxEvents 50 -ErrorAction Stop
            $rows = foreach ($e in $ev) {
                $x = [xml]$e.ToXml(); $d = @{}
                foreach ($n in $x.Event.EventData.Data) { $d[$n.Name] = $n.'#text' }
                [pscustomobject]@{
                    time        = $e.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss')
                    user        = [string]$d['TargetUserName']
                    source_ip   = [string]$d['IpAddress']
                    workstation = [string]$d['WorkstationName']
                    logon_type  = [string]$d['LogonType']
                }
            }
            if ($rows) { $rows | ConvertTo-Json -Compress } else { '[]' }
        } catch { '[]' }"#;
        // Same UTF-8 runner as the count probe. It matters more here than it
        // looks: these rows carry account and workstation NAMES, which on a
        // Spanish-language domain routinely contain accents and ñ. Decoded as
        // UTF-8 from an OEM code page they would arrive full of U+FFFD — still
        // valid JSON, so nothing would fail, and the operator would just see a
        // corrupted username while threat hunting.
        let raw = run_powershell_utf8(script)?;
        let txt = raw.trim();
        if txt.is_empty() || txt == "[]" {
            return Ok(vec![]);
        }
        // ConvertTo-Json emits a bare object (not an array) when there's exactly
        // one row — handle both shapes.
        let val: serde_json::Value = serde_json::from_str(txt)
            .map_err(|e| format!("JSON parse: {}", e))?;
        let arr = match val {
            serde_json::Value::Array(a) => a,
            v @ serde_json::Value::Object(_) => vec![v],
            _ => vec![],
        };
        let pick = |v: &serde_json::Value, k: &str| {
            v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
        };
        let events: Vec<FailedLoginEvent> = arr.iter().map(|v| FailedLoginEvent {
            time:        pick(v, "time"),
            user:        pick(v, "user"),
            source_ip:   pick(v, "source_ip"),
            workstation: pick(v, "workstation"),
            logon_type:  pick(v, "logon_type"),
        }).collect();
        Ok(events)
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── The locale trap ─────────────────────────────────────────────────────
    //
    // The defect these guard was invisible on an English Windows and certain
    // everywhere else, which is the worst combination: it survives every
    // developer machine and fails on every user's. Get-WinEvent THROWS when a
    // filter matches nothing, so "all clear" arrives as an exception, and
    // telling it apart from a real failure by reading the exception TEXT means
    // reading a string Windows translates.
    //
    // A unit test cannot run Spanish PowerShell, so it guards the decision
    // instead: match on the stable id, never on the prose.

    #[test]
    fn failed_logins_script_discriminates_on_error_id_not_message_text() {
        assert!(
            FAILED_LOGINS_COUNT_SCRIPT.contains("FullyQualifiedErrorId"),
            "the no-events case must be detected by its locale-independent id",
        );
        assert!(
            FAILED_LOGINS_COUNT_SCRIPT.contains("NoMatchingEventsFound"),
            "NoMatchingEventsFound is the id Get-WinEvent raises for an empty match",
        );
        // The exact shape of the original bug: an English-only text match.
        assert!(
            !FAILED_LOGINS_COUNT_SCRIPT.contains("Exception.Message -like"),
            "matching the exception TEXT is locale-dependent — it reported a \
             healthy zero as 'Registro de seguridad no legible' on es-ES",
        );
        for english in ["No events", "returned no results"] {
            assert!(
                !FAILED_LOGINS_COUNT_SCRIPT.contains(english),
                "'{}' is an English message fragment; Windows localises it",
                english,
            );
        }
    }

    // ── Output classification ───────────────────────────────────────────────

    #[test]
    fn a_real_zero_is_available_not_an_error() {
        // THE case that regressed. Zero failed logons is the healthiest
        // possible answer and must be reported as data, not as a fault —
        // the UI deliberately distinguishes "a real zero" from "a zero
        // because we could not look".
        let b = classify_failed_logins_output("0");
        assert!(b.available);
        assert_eq!(b.count_24h, 0);
        assert!(b.note.is_empty());
    }

    #[test]
    fn a_positive_count_is_parsed() {
        let b = classify_failed_logins_output("17");
        assert!(b.available);
        assert_eq!(b.count_24h, 17);
    }

    #[test]
    fn access_denied_stays_unavailable() {
        // Distinct from a real zero on purpose: conflating them would tell an
        // operator "no failed logons" when the truth is "I could not read it".
        let b = classify_failed_logins_output("ACCESS_DENIED");
        assert!(!b.available);
        assert_eq!(b.count_24h, 0);
        assert!(b.note.to_lowercase().contains("admin"));
    }

    #[test]
    fn a_real_error_keeps_its_message() {
        let b = classify_failed_logins_output("ERROR:El servicio de registro de eventos no responde");
        assert!(!b.available);
        assert!(b.note.contains("no responde"));
        assert!(!b.note.starts_with("ERROR:"), "the prefix is a protocol token, not prose");
    }

    #[test]
    fn only_the_last_non_empty_line_is_read() {
        // -NoProfile does not always stop PowerShell prepending a warning.
        let b = classify_failed_logins_output("WARNING: perfil omitido\n\n3\n\n");
        assert!(b.available);
        assert_eq!(b.count_24h, 3);
    }

    #[test]
    fn unparseable_output_does_not_become_a_silent_zero() {
        let b = classify_failed_logins_output("¿qué?");
        assert!(!b.available, "garbage must not be reported as zero failed logons");
        assert!(b.note.contains("Unexpected"));
    }

    // Encoding is no longer tested here. The preamble moved to
    // `utils::shell::ps_utf8` during the 2026-07-28 audit and is covered by
    // `preamble_sets_both_encoding_handles_before_the_payload` there; the local
    // wrapper this test drove became a pass-through with no caller outside the
    // test itself, which is exactly the dead code `cargo` then warned about.
}
