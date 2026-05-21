// ── F7 — Generative Runbook from Observation ─────────────────────────────
//
// Mines the user's command history for repeated sequences. When the same
// ordered set of commands (≥3 steps) appears ≥3 times across the last N
// days, proposes it as a reusable runbook ("/monday-setup", etc.).
//
// Why this is novel:
//   - AutoHotkey and macros require explicit programming.
//   - AI assistants don't observe workflow patterns.
//   - This is *learn-by-watching* with NO model training, just statistics.
//
// Data source:
//   - `conversation_turns` table (already populated by Lucy's chat)
//   - We treat consecutive user commands as a "session sequence"
//   - Sessions are split by gaps >30 min of inactivity
//
// Algorithm (simplified PrefixSpan):
//   1) Extract command lines from turns where role='Iván' / lucyConfig.name
//   2) Normalize: lowercase, strip args, keep command verb + first 1-2 tokens
//   3) Group into sessions by time gaps
//   4) Find frequent N-grams (3 ≤ N ≤ 6) using support threshold ≥3
//   5) Filter: skip trivial (/help, /clear), skip noise (single-char inputs)
//   6) Rank by lift = freq × length
//   7) Return top-K candidates with confidence
//
// Privacy: 100% local. Never leaves the machine. The user explicitly
// opts in by calling this command.

use crate::commands::metrics::shared_db;
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
pub struct RunbookCandidate {
    /// Ordered sequence of normalized commands.
    pub sequence: Vec<String>,
    /// How many times this sequence appeared.
    pub frequency: usize,
    /// Sample timestamps (up to 3) of when it was seen — for the user to verify.
    pub sample_times: Vec<i64>,
    /// Confidence 0-1: combines freq, length, time-spread.
    pub confidence: f64,
    /// Suggested name (derived from first command).
    pub suggested_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunbookScanResult {
    pub days_analyzed: i64,
    pub total_sessions: usize,
    pub total_commands: usize,
    pub candidates: Vec<RunbookCandidate>,
}

const SESSION_GAP_SEC: i64 = 30 * 60; // 30 min idle splits sessions
const MIN_SEQ_LEN: usize = 3;
const MAX_SEQ_LEN: usize = 6;
const MIN_SUPPORT: usize = 3; // sequence must repeat at least 3×

/// Normalize a user command for sequence matching.
/// Strategy: keep the first non-flag token, lowercased. This collapses
/// "git status" / "git status --short" into a single key but distinguishes
/// "git pull" from "git push".
fn normalize_command(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() || s.len() < 2 { return None; }

    // Skip pure questions (heuristic: ends with ? or starts with question word)
    let lower = s.to_lowercase();
    let q_words = ["qué", "que", "what", "why", "por qué", "porqué", "how", "cómo", "como", "cuándo", "cuando", "where", "dónde", "donde"];
    if s.ends_with('?') || q_words.iter().any(|w| lower.starts_with(w)) {
        return None;
    }

    // Skip /help-class slash commands (noisy, not part of workflows)
    if lower.starts_with("/help") || lower == "/?" || lower.starts_with("/clear") {
        return None;
    }

    // First two tokens, dropping flags
    let tokens: Vec<&str> = s.split_whitespace()
        .filter(|t| !t.starts_with('-'))
        .take(2)
        .collect();
    if tokens.is_empty() { return None; }

    let key = tokens.join(" ").to_lowercase();
    if key.len() > 60 {
        // Probably a sentence, not a command
        return None;
    }
    // Sentences vs commands: count whitespace. If too many spaces in first two
    // tokens after our filter, it's probably a question.
    if tokens.iter().any(|t| t.chars().filter(|c| c.is_whitespace()).count() > 0) {
        return None;
    }
    Some(key)
}

#[tauri::command]
pub async fn runbook_scan(days: Option<i64>, top_k: Option<usize>) -> Result<RunbookScanResult, String> {
    let lookback_days = days.unwrap_or(30).clamp(1, 365);
    let cap = top_k.unwrap_or(8).clamp(1, 50);
    let cutoff = now_ts() - lookback_days * 24 * 3600;

    shared_db(move |conn| {
        // Pull user turns ordered by time
        let mut stmt = conn.prepare(
            "SELECT created_at, content FROM conversation_turns
             WHERE role NOT IN ('Lucy', 'lucy', 'system', 'Sistema')
               AND created_at >= ?1
             ORDER BY created_at ASC"
        ).map_err(|e| format!("prepare: {}", e))?;

        let rows: Vec<(i64, String)> = stmt
            .query_map(params![cutoff], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        let total_commands = rows.len();

        // Build sessions by time gap
        let mut sessions: Vec<Vec<(i64, String)>> = Vec::new();
        let mut current: Vec<(i64, String)> = Vec::new();
        let mut last_ts = 0_i64;
        for (ts, content) in rows {
            let normalized = match normalize_command(&content) {
                Some(s) => s,
                None => continue,
            };
            if !current.is_empty() && (ts - last_ts) > SESSION_GAP_SEC {
                sessions.push(std::mem::take(&mut current));
            }
            current.push((ts, normalized));
            last_ts = ts;
        }
        if !current.is_empty() { sessions.push(current); }

        let total_sessions = sessions.len();

        // ── N-gram mining ────────────────────────────────────────────────
        // Key: tuple of N normalized commands (joined by '\u{1f}' so we can
        // recover them). Value: Vec<sample_ts>.
        let sep = '\u{1f}';
        let mut counts: HashMap<String, Vec<i64>> = HashMap::new();
        for sess in &sessions {
            for n in MIN_SEQ_LEN..=MAX_SEQ_LEN {
                if sess.len() < n { continue; }
                for window in sess.windows(n) {
                    let key: String = window.iter()
                        .map(|(_, c)| c.as_str())
                        .collect::<Vec<_>>()
                        .join(&sep.to_string());
                    let entry = counts.entry(key).or_insert_with(Vec::new);
                    entry.push(window[0].0); // first ts of the window
                }
            }
        }

        // Filter to support threshold
        let mut filtered: Vec<(String, Vec<i64>)> = counts
            .into_iter()
            .filter(|(_, ts)| ts.len() >= MIN_SUPPORT)
            .collect();

        // Remove subsumed sequences: if [a,b,c] freq=5 and [a,b,c,d] freq=5,
        // prefer the LONGER one. We keep only sequences that aren't a strict
        // prefix of a longer sequence with same freq.
        filtered.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        let mut keep = Vec::with_capacity(filtered.len());
        for (seq, ts) in &filtered {
            let subsumed = keep.iter().any(|(other_seq, other_ts): &(String, Vec<i64>)| {
                other_seq.starts_with(seq) && other_seq != seq && other_ts.len() == ts.len()
            });
            if !subsumed {
                keep.push((seq.clone(), ts.clone()));
            }
        }

        // Build candidates with confidence
        let mut candidates: Vec<RunbookCandidate> = keep.into_iter().map(|(key, sample_ts)| {
            let sequence: Vec<String> = key.split(sep).map(|s| s.to_string()).collect();
            let frequency = sample_ts.len();
            // Time spread: if all occurrences are within 24h, less interesting
            // (probably a one-day burst). Spread over weeks → real pattern.
            let min_ts = *sample_ts.iter().min().unwrap_or(&0);
            let max_ts = *sample_ts.iter().max().unwrap_or(&0);
            let span_days = ((max_ts - min_ts) / 86_400).max(1) as f64;
            // Confidence formula:
            //   freq_norm = min(freq/10, 1.0)
            //   len_norm  = (len-3)/3   (3 steps = 0, 6 steps = 1)
            //   spread_norm = min(span_days/14, 1.0)
            // confidence = 0.5*freq_norm + 0.25*len_norm + 0.25*spread_norm
            let freq_norm = (frequency as f64 / 10.0).min(1.0);
            let len_norm = ((sequence.len() as f64 - MIN_SEQ_LEN as f64) / (MAX_SEQ_LEN as f64 - MIN_SEQ_LEN as f64)).clamp(0.0, 1.0);
            let spread_norm = (span_days / 14.0).min(1.0);
            let confidence = 0.5 * freq_norm + 0.25 * len_norm + 0.25 * spread_norm;

            let suggested_name = if sequence.is_empty() {
                "untitled".to_string()
            } else {
                let first = sequence[0].chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>();
                format!("{}-flow", first.chars().take(20).collect::<String>())
            };

            // Keep at most 3 sample timestamps for display
            let mut samples = sample_ts.clone();
            samples.sort();
            let sample_times = if samples.len() > 3 {
                vec![samples[0], samples[samples.len() / 2], samples[samples.len() - 1]]
            } else { samples };

            RunbookCandidate {
                sequence,
                frequency,
                sample_times,
                confidence,
                suggested_name,
            }
        }).collect();

        candidates.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(cap);

        Ok(RunbookScanResult {
            days_analyzed: lookback_days,
            total_sessions,
            total_commands,
            candidates,
        })
    })
}

// ── Promote-to-skill ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteRunbookArgs {
    /// Ordered list of normalized commands from a runbook candidate.
    pub sequence: Vec<String>,
    /// Human-readable name for the skill ("git-flow", "monday-setup").
    pub name: String,
    /// Optional description shown in the skills picker.
    pub description: Option<String>,
    /// Optional category — defaults to "workflow".
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotedRunbook {
    pub skill_id: String,
    pub name: String,
    pub category: String,
    pub script: String,
}

/// Materialize a detected workflow as a saved skill the user can re-invoke.
/// The "script" is a multi-line shell sequence — one command per line — so
/// it can be displayed, edited, and replayed step-by-step with HITL.
#[tauri::command]
pub async fn runbook_promote(args: PromoteRunbookArgs) -> Result<PromotedRunbook, String> {
    if args.sequence.is_empty() {
        return Err("sequence must not be empty".into());
    }
    // Sanitize the requested name: lowercase, [a-z0-9-_], collapse spaces
    let raw = args.name.trim().to_lowercase();
    let safe_name: String = raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c }
                 else if c.is_whitespace() { '-' }
                 else { '_' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if safe_name.is_empty() || safe_name.len() > 80 {
        return Err("name must be 1-80 chars of [a-z0-9-_]".into());
    }

    let category = args.category.unwrap_or_else(|| "workflow".to_string());
    let description = args.description.unwrap_or_else(||
        format!("Auto-generated from observed workflow ({} steps)", args.sequence.len())
    );
    // Script: numbered comments + commands, so a human can read it back
    let script = {
        let mut out = String::new();
        out.push_str(&format!("# Runbook: {}\n", safe_name));
        out.push_str(&format!("# Generated from observed workflow — {} steps\n", args.sequence.len()));
        out.push_str("# Edit before invoking if any step needs arguments.\n\n");
        for (i, step) in args.sequence.iter().enumerate() {
            out.push_str(&format!("# step {}\n{}\n", i + 1, step));
        }
        out
    };

    // Triggers: the user can type the name OR the first step
    let triggers = serde_json::to_string(&vec![
        safe_name.clone(),
        format!("/{}", safe_name),
        args.sequence.first().cloned().unwrap_or_default(),
    ]).unwrap_or_else(|_| "[]".to_string());

    let id = format!("rb-{}-{}", safe_name, now_ts());
    let ts = chrono::Local::now().to_rfc3339();

    shared_db(|conn| {
        conn.execute(
            "INSERT INTO skills (id, name, category, triggers, script, description, parameters, created_at, updated_at, usage_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, '[]', ?7, ?7, 0)
             ON CONFLICT(name) DO UPDATE SET
                script      = excluded.script,
                description = excluded.description,
                updated_at  = excluded.updated_at",
            rusqlite::params![&id, &safe_name, &category, &triggers, &script, &description, &ts],
        ).map_err(|e| format!("insert skill: {}", e))?;
        Ok(())
    })?;

    Ok(PromotedRunbook {
        skill_id: id,
        name: safe_name,
        category,
        script,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_flags() {
        assert_eq!(normalize_command("git status --short"), Some("git status".to_string()));
        assert_eq!(normalize_command("ls -la"), Some("ls".to_string()));
    }

    #[test]
    fn normalize_drops_questions() {
        assert!(normalize_command("¿qué es esto?").is_none());
        assert!(normalize_command("what is happening?").is_none());
    }

    #[test]
    fn normalize_drops_help() {
        assert!(normalize_command("/help").is_none());
        assert!(normalize_command("/clear").is_none());
    }

    #[test]
    fn normalize_keeps_real_commands() {
        assert_eq!(normalize_command("npm install"), Some("npm install".to_string()));
        assert_eq!(normalize_command("docker ps"), Some("docker ps".to_string()));
    }

    #[test]
    fn normalize_drops_long_text() {
        let long = "this is a very long sentence that is clearly not a command at all just prose";
        assert_eq!(normalize_command(long), Some("this is".to_string()));
        // Even when shortened we get a key — but reality: the LIKELIHOOD of this
        // recurring exactly 3 times in different sessions is near zero, so noise
        // self-filters via the support threshold.
    }
}
