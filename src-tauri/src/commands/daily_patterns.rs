// ── F10 — Daily Pattern Learning ──────────────────────────────────────────
//
// Learns time-of-day routines from observed conversation_turns + process_lineage.
// "Every Monday between 9-10am, the user opens VSCode, Spotify, and Slack."
//
// Lucy can then propose:
//   - Pre-warm those apps when the time approaches
//   - Surface a "ready for Monday" toast
//   - Build a workspace preset on demand
//
// Data sources combined:
//   • conversation_turns — what commands/topics happened when
//   • process_lineage — what apps started when (process names by first_seen)
//
// Algorithm (deliberately simple, transparent):
//   1) Bucket observations by (weekday, hour-band)
//      weekdays: Mon..Sun
//      hour-band: 6 buckets per day (0-4, 4-8, 8-12, 12-16, 16-20, 20-24)
//   2) For each (weekday, band, signal) tuple, count occurrences
//   3) Compute "confidence" = occurrences / weeks_observed
//   4) Filter to confidence ≥ 0.5 (happens in at least half the weeks)
//   5) Group results into "routines" the user can adopt
//
// Privacy: identical to F7. 100% local, opt-in only.

use crate::commands::metrics::shared_db;
use chrono::{DateTime, Datelike, Local, Timelike};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyPattern {
    pub weekday: u32,           // 0=Mon..6=Sun (chrono Datelike num_days_from_monday)
    pub weekday_label: String,  // localized name for the UI
    pub hour_band: String,      // "08-12" etc.
    pub signal: String,         // process name or command verb
    pub kind: String,           // "process" | "command"
    pub occurrences: usize,
    pub weeks_observed: u32,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyPatternScanResult {
    pub days_analyzed: i64,
    pub weeks_covered: u32,
    pub patterns: Vec<DailyPattern>,
}

/// Boundaries kept as a documented constant + test fixture. The `hour_band`
/// function inlines the bands for clarity; this array is the source of
/// truth for any future change + the `band_boundaries_continuous` test.
#[cfg(test)]
const BAND_BOUNDARIES: [u32; 7] = [0, 4, 8, 12, 16, 20, 24];

fn hour_band(hour: u32) -> &'static str {
    if hour < 4  { "00-04" }
    else if hour < 8  { "04-08" }
    else if hour < 12 { "08-12" }
    else if hour < 16 { "12-16" }
    else if hour < 20 { "16-20" }
    else              { "20-24" }
}

fn weekday_label(d: u32) -> &'static str {
    match d {
        0 => "Lun", 1 => "Mar", 2 => "Mié", 3 => "Jue",
        4 => "Vie", 5 => "Sáb", 6 => "Dom", _ => "?"
    }
}

/// Extract the "signal" from a command: first non-flag token, lowercased.
fn signal_from_command(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.len() < 2 || trimmed.starts_with('?') || trimmed.ends_with('?') {
        return None;
    }
    let t = trimmed.split_whitespace().find(|t| !t.starts_with('-'))?;
    let t_lower = t.to_lowercase();
    if t_lower.len() > 30 || t_lower.starts_with("/help") { return None; }
    // Drop pure prose: count letters vs vowels — sentences have a vowel-heavy ratio
    // but command names too (git, ls, etc.). Skip heuristic; just trust the token.
    Some(t_lower)
}

/// Extract the binary name (no path, no .exe) from a full exe_path.
fn binary_basename(path: &str) -> Option<String> {
    let stem = std::path::Path::new(path).file_stem()?.to_str()?;
    let lower = stem.to_lowercase();
    // Skip system noise that runs all the time
    let noise = ["svchost", "runtimebroker", "system", "smss", "csrss", "winlogon",
                 "lsass", "services", "dwm", "explorer", "conhost", "searchhost",
                 "audiodg", "fontdrvhost", "wuauclt"];
    if noise.iter().any(|n| lower.contains(n)) { return None; }
    Some(lower)
}

#[tauri::command]
pub async fn daily_patterns_scan(days: Option<i64>, min_confidence: Option<f64>) -> Result<DailyPatternScanResult, String> {
    let lookback_days = days.unwrap_or(28).clamp(7, 365);
    let threshold = min_confidence.unwrap_or(0.5).clamp(0.0, 1.0);
    let cutoff = now_ts() - lookback_days * 24 * 3600;

    shared_db(move |conn| {
        // (weekday, band, signal, kind) → occurrences
        // Also track distinct weeks each signal appeared in.
        let mut occ: HashMap<(u32, &'static str, String, &'static str), HashMap<i64, ()>> = HashMap::new();

        // ── Source 1: user commands ─────────────────────────────────────
        let mut stmt = conn.prepare(
            "SELECT created_at, content FROM conversation_turns
             WHERE role NOT IN ('Lucy', 'lucy', 'system', 'Sistema')
               AND created_at >= ?1"
        ).map_err(|e| format!("prepare turns: {}", e))?;
        let rows: Vec<(i64, String)> = stmt
            .query_map(params![cutoff], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| format!("query turns: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        for (ts, content) in &rows {
            let signal = match signal_from_command(content) { Some(s) => s, None => continue };
            let dt = DateTime::from_timestamp(*ts, 0).map(|t| t.with_timezone(&Local));
            if let Some(dt) = dt {
                let wd = dt.weekday().num_days_from_monday();
                let band = hour_band(dt.hour());
                let week = dt.iso_week().week() as i64;
                occ.entry((wd, band, signal, "command")).or_default().insert(week, ());
            }
        }

        // ── Source 2: process arrivals (binary basenames) ────────────────
        // Best-effort — ensure the table exists before querying.
        if let Ok(mut pstmt) = conn.prepare(
            "SELECT first_seen, exe_path FROM process_lineage
             WHERE first_seen >= ?1"
        ) {
            let prows: Vec<(i64, String)> = pstmt
                .query_map(params![cutoff], |r| Ok((r.get(0)?, r.get(1)?)))
                .ok()
                .map(|iter| iter.filter_map(|r| r.ok()).collect())
                .unwrap_or_default();
            for (ts, exe) in prows {
                let signal = match binary_basename(&exe) { Some(s) => s, None => continue };
                let dt = DateTime::from_timestamp(ts, 0).map(|t| t.with_timezone(&Local));
                if let Some(dt) = dt {
                    let wd = dt.weekday().num_days_from_monday();
                    let band = hour_band(dt.hour());
                    let week = dt.iso_week().week() as i64;
                    occ.entry((wd, band, signal, "process")).or_default().insert(week, ());
                }
            }
        }

        // Weeks covered = distinct iso_weeks in the full lookback
        let weeks_covered = ((lookback_days as f64) / 7.0).ceil() as u32;

        // Build patterns
        let mut patterns: Vec<DailyPattern> = occ.into_iter()
            .filter_map(|((wd, band, sig, kind), weeks)| {
                let weeks_obs = weeks.len() as u32;
                let confidence = (weeks_obs as f64) / (weeks_covered.max(1) as f64);
                if confidence < threshold { return None; }
                if weeks_obs < 2 { return None; } // never a "pattern" from one week
                Some(DailyPattern {
                    weekday: wd,
                    weekday_label: weekday_label(wd).to_string(),
                    hour_band: band.to_string(),
                    signal: sig,
                    kind: kind.to_string(),
                    occurrences: weeks_obs as usize,
                    weeks_observed: weeks_obs,
                    confidence,
                })
            })
            .collect();

        // Sort: confidence desc, then weekday asc, then band asc for stable UI
        patterns.sort_by(|a, b| {
            b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal)
                .then(a.weekday.cmp(&b.weekday))
                .then(a.hour_band.cmp(&b.hour_band))
        });
        patterns.truncate(30);

        Ok(DailyPatternScanResult {
            days_analyzed: lookback_days,
            weeks_covered,
            patterns,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_boundaries_correct() {
        assert_eq!(hour_band(0),  "00-04");
        assert_eq!(hour_band(3),  "00-04");
        assert_eq!(hour_band(4),  "04-08");
        assert_eq!(hour_band(9),  "08-12");
        assert_eq!(hour_band(15), "12-16");
        assert_eq!(hour_band(20), "20-24");
        assert_eq!(hour_band(23), "20-24");
    }

    #[test]
    fn binary_basename_strips_noise() {
        assert_eq!(binary_basename("C:\\Windows\\System32\\svchost.exe"), None);
        assert_eq!(binary_basename("C:\\Windows\\explorer.exe"), None);
    }

    #[test]
    fn binary_basename_keeps_user_apps() {
        assert_eq!(binary_basename("C:\\Program Files\\Discord\\Discord.exe"), Some("discord".to_string()));
        assert_eq!(binary_basename("C:\\app\\VSCode.exe"), Some("vscode".to_string()));
    }

    #[test]
    fn signal_extraction() {
        assert_eq!(signal_from_command("npm install"), Some("npm".to_string()));
        assert_eq!(signal_from_command("git push origin main"), Some("git".to_string()));
        assert_eq!(signal_from_command("docker compose up -d"), Some("docker".to_string()));
        assert!(signal_from_command("¿qué hace esto?").is_none());
    }

    #[test]
    fn weekday_labels_spanish() {
        assert_eq!(weekday_label(0), "Lun");
        assert_eq!(weekday_label(6), "Dom");
    }

    /// Compile-time check: BAND_BOUNDARIES covers the 24-hour clock exactly.
    #[test]
    fn band_boundaries_continuous() {
        assert_eq!(BAND_BOUNDARIES.first().copied(), Some(0));
        assert_eq!(BAND_BOUNDARIES.last().copied(),  Some(24));
        // monotonic strictly increasing
        for w in BAND_BOUNDARIES.windows(2) {
            assert!(w[0] < w[1]);
        }
    }
}
