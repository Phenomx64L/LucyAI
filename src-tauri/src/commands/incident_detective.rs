// ── Incident Detective — Cross-feature synthesis (Sprint 7) ──────────────
//
// Combines the three Frontier signal sources into a single forensic view
// around a "symptom" timestamp:
//
//   • F3 Causal Engine  → ranks process arrivals by suspicion
//   • F8 Mini-EDR       → flags behaviorally suspicious processes in the window
//   • F9 Knowledge Graph→ surfaces files modified in the same window
//
// Result: an actionable "what happened around T?" report that no single
// signal could produce alone. This is the use case where Lucy decisively
// beats any commercial EDR or AI assistant — context fusion is the moat.
//
// Output shape:
//   - Window: [T-window, T+window]
//   - Process candidates with score + reasoning (from F3)
//   - Threats: any process in the window that F8 flagged "review"/"alert"
//   - File changes: paths modified in the window (from F9), with their
//     co-touched neighbors so we can show "what cluster of files moved"
//   - Composite narrative: one-line summary picking the highest-confidence
//     signal across all three modules

use crate::commands::causal;
use crate::commands::knowledge_graph::KgFile;
use crate::commands::metrics::shared_db;
use crate::commands::threat_scan;
use rusqlite::params;
use serde::{Deserialize, Serialize};

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentDetectiveReport {
    pub symptom_ts: i64,
    pub window_sec: i64,
    /// One-line summary of the most-likely explanation.
    pub narrative: String,
    /// Overall confidence 0-1 — max of any single signal in the window.
    pub confidence: f64,
    /// From F3.
    pub causal: causal::CausalAnalysis,
    /// From F8 — only candidates that fall within the time window.
    pub threats: Vec<threat_scan::ThreatCandidate>,
    /// From F9 — files modified in the window.
    pub file_changes: Vec<KgFile>,
    /// Cluster of files most touched together in the window (top 5 names).
    pub touched_cluster_summary: String,
}

/// Run all three signal sources for a symptom and synthesize.
#[tauri::command]
pub async fn incident_detective(
    symptom_ts: Option<i64>,
    window_sec: Option<i64>,
) -> Result<IncidentDetectiveReport, String> {
    let symptom = symptom_ts.unwrap_or_else(now_ts);
    let window = window_sec.unwrap_or(300).clamp(30, 7200); // 5 min default

    // ── F3 Causal ─────────────────────────────────────────────────────
    let causal_report = causal::diagnose_spike(Some(symptom), Some(window)).await?;

    // ── F8 Threat scan (filter to window) ─────────────────────────────
    // We use a wider lookback than the window so the baseline novelty
    // calc has data, then filter results to the actual window.
    let scan_min = ((window * 2) / 60).max(5);
    let scan = threat_scan::threat_scan(Some(scan_min), Some(0.30), Some(200)).await?;
    let lower = symptom - window;
    let upper = symptom + window;
    let mut threats: Vec<threat_scan::ThreatCandidate> = scan.candidates.into_iter()
        .filter(|c| c.first_seen >= lower && c.first_seen <= upper)
        .collect();
    threats.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    threats.truncate(8);

    // ── F9 Knowledge Graph: files modified in the window ─────────────
    let file_changes: Vec<KgFile> = shared_db(move |conn| {
        // kg_files may not exist if KG was never used. CREATE IF NOT EXISTS
        // is idempotent and matches the schema from knowledge_graph.rs.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS kg_files (
                path        TEXT PRIMARY KEY,
                extension   TEXT NOT NULL DEFAULT '',
                parent_dir  TEXT NOT NULL DEFAULT '',
                last_mtime  INTEGER NOT NULL,
                first_seen  INTEGER NOT NULL,
                touch_count INTEGER NOT NULL DEFAULT 1
            )",
            [],
        ).ok();
        let mut stmt = conn.prepare(
            "SELECT path, extension, parent_dir, last_mtime, first_seen, touch_count
             FROM kg_files
             WHERE last_mtime BETWEEN ?1 AND ?2
             ORDER BY last_mtime DESC LIMIT 50"
        ).map_err(|e| format!("prepare kg: {}", e))?;
        let rows: Vec<KgFile> = stmt
            .query_map(params![lower, upper], |row| {
                Ok(KgFile {
                    path: row.get(0)?,
                    extension: row.get(1)?,
                    parent_dir: row.get(2)?,
                    last_mtime: row.get(3)?,
                    first_seen: row.get(4)?,
                    touch_count: row.get(5)?,
                })
            })
            .map_err(|e| format!("query kg: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })?;

    // Cluster summary: pick the most-touched parent_dir and the top extensions
    let touched_cluster_summary = {
        if file_changes.is_empty() {
            "no indexed file changes in window".to_string()
        } else {
            let mut by_dir: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            let mut by_ext: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for f in &file_changes {
                *by_dir.entry(f.parent_dir.clone()).or_insert(0) += 1;
                *by_ext.entry(f.extension.clone()).or_insert(0) += 1;
            }
            let top_dir = by_dir.iter().max_by_key(|(_, n)| **n);
            let top_ext = by_ext.iter().max_by_key(|(_, n)| **n);
            match (top_dir, top_ext) {
                (Some((d, dn)), Some((e, en))) => {
                    let ext_part = if e.is_empty() { String::new() } else { format!(" · mostly .{} ({}×)", e, en) };
                    format!("{} files, top dir: {} ({}×){}", file_changes.len(), d, dn, ext_part)
                }
                _ => format!("{} files modified in window", file_changes.len()),
            }
        }
    };

    // ── Composite narrative ──────────────────────────────────────────
    let top_causal = causal_report.candidates.first();
    let top_threat = threats.first();

    let causal_conf = top_causal.map(|c| c.confidence).unwrap_or(0.0);
    let threat_conf = top_threat.map(|t| t.score).unwrap_or(0.0);
    let file_conf = if file_changes.is_empty() { 0.0 } else { 0.4 };
    let confidence = causal_conf.max(threat_conf).max(file_conf);

    let narrative = match (top_threat, top_causal, file_changes.len()) {
        (Some(t), _, _) if t.score >= 0.55 => format!(
            "[BEHAVIORAL ALERT] {} (pid {}) flagged at {}% — {}",
            t.name, t.pid, (t.score * 100.0) as u32,
            t.reasons.first().cloned().unwrap_or_else(|| "matches threat heuristics".to_string())
        ),
        (_, Some(c), _) if c.confidence >= 0.55 => format!(
            "[CAUSAL] {} appeared {}s {} the symptom — {}% confidence",
            c.name,
            c.temporal_offset_sec.abs(),
            if c.temporal_offset_sec < 0 { "before" } else { "after" },
            (c.confidence * 100.0) as u32,
        ),
        (_, _, n) if n >= 3 => format!(
            "[FILE ACTIVITY] {} files modified in this window — {}",
            n, touched_cluster_summary
        ),
        (_, Some(c), _) => format!(
            "[WEAK] best guess {} ({}% confidence) — consider widening the window",
            c.name, (c.confidence * 100.0) as u32
        ),
        _ => "[INCONCLUSIVE] no strong signal in this window — try widening, or check incidents/state_diff for a longer view".to_string(),
    };

    Ok(IncidentDetectiveReport {
        symptom_ts: symptom,
        window_sec: window,
        narrative,
        confidence,
        causal: causal_report,
        threats,
        file_changes,
        touched_cluster_summary,
    })
}
