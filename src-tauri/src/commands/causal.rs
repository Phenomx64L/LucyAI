// ── Causal Engine — F3 MVP (Frontier R&D, May 2026) ──────────────────────
//
// Answers "why did my machine get slower N minutes ago?" by correlating:
//
//   1. Current vs. historical metrics (the *symptom*)
//   2. Process lineage entries that appeared within the suspect window
//      (the *culprits*)
//   3. State snapshots taken around the suspect window (the *change set*)
//
// The output is a ranked list of `CausalCandidate`s with:
//   - confidence: 0-1
//   - reasoning: human-readable why-this-is-suspect
//
// This is intentionally NOT a deep-learning model — it's a transparent
// heuristic with explainable weights. Better to be predictable and slightly
// off than confidently wrong with a black box.
//
// Scoring weights (sum to 1.0):
//   • temporal_proximity (0.35): closer to symptom time → higher
//   • resource_intensity (0.30): proc CPU/RAM at observation > 50%
//   • novelty           (0.20): didn't exist in prior snapshot → suspicious
//   • parent_chain_oddness (0.15): unusual parent (orphan / temp dir) → suspicious

use crate::commands::metrics::shared_db;
use crate::commands::process_lineage::ProcessLineageRow;
use rusqlite::params;
use serde::{Deserialize, Serialize};

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalCandidate {
    pub pid: u32,
    pub name: String,
    pub exe_path: String,
    pub parent_chain_summary: String,
    pub first_seen: i64,
    /// Seconds between this candidate's first_seen and the symptom timestamp.
    pub temporal_offset_sec: i64,
    /// Composite score 0-1.
    pub confidence: f64,
    /// Bullet list of why this was flagged.
    pub reasoning: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalAnalysis {
    pub symptom_ts: i64,
    pub window_sec: i64,
    pub candidates: Vec<CausalCandidate>,
    pub state_diff_summary: Option<String>,
    pub metric_context: String,
}

/// Heuristic check: is this exe path suspicious by location?
fn path_oddness(exe: &str) -> (f64, Option<String>) {
    let lower = exe.to_lowercase();
    if lower.is_empty() {
        return (0.5, Some("missing exe path".to_string()));
    }
    if lower.contains("\\temp\\") || lower.contains("/tmp/") || lower.contains("appdata\\local\\temp") {
        return (1.0, Some("runs from a temp directory".to_string()));
    }
    if lower.contains("\\downloads\\") {
        return (0.85, Some("runs from Downloads folder".to_string()));
    }
    if lower.contains("\\appdata\\") && !lower.contains("microsoft") && !lower.contains("google") {
        return (0.55, Some("runs from AppData".to_string()));
    }
    if lower.contains("\\program files") || lower.contains("\\windows\\system32") {
        return (0.0, None); // expected location
    }
    (0.2, None) // unknown but not red flag
}

/// Diagnose a performance spike. Looks at process_lineage entries that
/// first appeared within ±window_sec of `symptom_ts` and ranks them.
///
/// Arguments:
///   - symptom_ts: unix seconds. If 0/None, defaults to now.
///   - window_sec: how wide the suspect window is on each side. Default 120s.
#[tauri::command]
pub async fn diagnose_spike(
    symptom_ts: Option<i64>,
    window_sec: Option<i64>,
) -> Result<CausalAnalysis, String> {
    let symptom = symptom_ts.unwrap_or_else(now_ts);
    let window = window_sec.unwrap_or(120).clamp(15, 1800);

    let lower = symptom - window;
    let upper = symptom + window;

    shared_db(move |conn| {
        // ── Pull candidates from process_lineage in window ────────────────
        let mut stmt = conn.prepare(
            "SELECT id, pid, ppid, parent_chain, exe_path, cmdline, first_seen,
                    vanished_at, observed_count, chain_hash
             FROM process_lineage
             WHERE first_seen BETWEEN ?1 AND ?2
             ORDER BY first_seen ASC
             LIMIT 200"
        ).map_err(|e| format!("prepare lineage: {}", e))?;

        let rows: Vec<ProcessLineageRow> = stmt
            .query_map(params![lower, upper], |row| {
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
            .map_err(|e| format!("query lineage: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        // ── Find the closest two snapshots bracketing the symptom for novelty ─
        let snap_before: Option<(i64, String)> = conn.query_row(
            "SELECT id, payload FROM state_snapshots
             WHERE captured_at < ?1 ORDER BY captured_at DESC LIMIT 1",
            params![symptom],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();

        // ── Rank candidates ───────────────────────────────────────────────
        let mut candidates: Vec<CausalCandidate> = Vec::with_capacity(rows.len());
        for r in &rows {
            let temporal_offset_sec = (r.first_seen - symptom).abs();
            // Linear decay over the window — closer to symptom = higher score
            let temporal_proximity = 1.0 - (temporal_offset_sec as f64 / window as f64).clamp(0.0, 1.0);

            // We don't have live CPU/RAM in lineage row, so use observed_count
            // as a stand-in for "process is still around and using time"
            let resource_intensity = (r.observed_count as f64 / 5.0).min(1.0);

            // Novelty: was this binary in the prior snapshot?
            let mut novelty = 0.5; // unknown by default
            if let Some((_, snap_json)) = &snap_before {
                let lower_exe = r.exe_path.to_lowercase();
                let proc_name = std::path::Path::new(&r.exe_path)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned().to_lowercase())
                    .unwrap_or_default();
                if !proc_name.is_empty() && snap_json.to_lowercase().contains(&proc_name) {
                    novelty = 0.1; // already existed
                } else if !lower_exe.is_empty() {
                    novelty = 1.0; // truly new
                }
            }

            let (oddness_score, oddness_reason) = path_oddness(&r.exe_path);

            let confidence = 0.35 * temporal_proximity
                           + 0.30 * resource_intensity
                           + 0.20 * novelty
                           + 0.15 * oddness_score;

            // Build human reasoning
            let mut reasoning = Vec::new();
            if temporal_proximity > 0.7 {
                reasoning.push(format!("appeared {}s {} the symptom",
                    temporal_offset_sec,
                    if r.first_seen <= symptom { "before" } else { "after" }));
            }
            if novelty > 0.8 {
                reasoning.push("was not present in the prior snapshot (new arrival)".to_string());
            }
            if let Some(odd) = oddness_reason { reasoning.push(odd); }
            if resource_intensity > 0.6 {
                reasoning.push(format!("observed across {} polling cycles (sustained)", r.observed_count));
            }
            if r.vanished_at.is_some() {
                reasoning.push("has already vanished (short-lived)".to_string());
            }
            if reasoning.is_empty() {
                reasoning.push("matches window heuristics".to_string());
            }

            // Parent chain summary
            let chain: serde_json::Value = serde_json::from_str(&r.parent_chain).unwrap_or(serde_json::Value::Array(vec![]));
            let chain_str = chain.as_array()
                .map(|arr| arr.iter()
                    .filter_map(|n| n.get("name").and_then(|v| v.as_str()))
                    .take(4)
                    .collect::<Vec<_>>()
                    .join(" ← "))
                .unwrap_or_default();

            let proc_name = std::path::Path::new(&r.exe_path)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("pid-{}", r.pid));

            candidates.push(CausalCandidate {
                pid: r.pid,
                name: proc_name,
                exe_path: r.exe_path.clone(),
                parent_chain_summary: chain_str,
                first_seen: r.first_seen,
                temporal_offset_sec: r.first_seen - symptom,
                confidence,
                reasoning,
            });
        }

        candidates.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(10);

        // ── State diff context for the prompt ─────────────────────────────
        let state_diff_summary: Option<String> = snap_before.as_ref().map(|(id, _)| {
            format!("baseline snapshot id={} available for diff", id)
        });

        let metric_context = format!(
            "Analyzed window: {}s before/after symptom_ts={}. Found {} lineage events.",
            window, symptom, rows.len()
        );

        Ok(CausalAnalysis {
            symptom_ts: symptom,
            window_sec: window,
            candidates,
            state_diff_summary,
            metric_context,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_oddness_flags_temp_dirs() {
        let (s, r) = path_oddness("C:\\Users\\u\\AppData\\Local\\Temp\\evil.exe");
        assert_eq!(s, 1.0);
        assert!(r.is_some());
    }

    #[test]
    fn path_oddness_passes_program_files() {
        let (s, r) = path_oddness("C:\\Program Files\\Google\\Chrome\\chrome.exe");
        assert_eq!(s, 0.0);
        assert!(r.is_none());
    }

    #[test]
    fn path_oddness_flags_downloads() {
        let (s, _) = path_oddness("C:\\Users\\u\\Downloads\\installer.exe");
        assert!(s >= 0.8);
    }

    #[test]
    fn path_oddness_handles_empty() {
        let (s, r) = path_oddness("");
        assert!(s > 0.0);
        assert!(r.is_some());
    }
}
