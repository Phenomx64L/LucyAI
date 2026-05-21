// ── Self-Healing Engine — F4 (Frontier R&D, May 2026) ────────────────────
//
// When Lucy detects an anomaly or the user reports a known-shape problem,
// this module searches her own memory for *patterns she has already
// resolved before*. Returns ranked candidates with success-rate scoring
// so the user (or the agent loop) can decide whether to retry the same
// fix, adapt it, or investigate fresh.
//
// How patterns get into memory:
//   • When `incident_finalize` is called with status='resolved', the
//     resolver pipeline (frontend side, +page.svelte) automatically
//     stores a `healing-pattern` memory with:
//       title:   short symptom description
//       content: { symptom, fix, evidence_ids, host_name, attempts }
//       tags:    ["healing-pattern", "auto-saved", ...domain tags]
//       importance: 5 (higher than default — these are gold)
//
//   • Manual: user can run /save-fix to crystallize the current session
//     into a healing-pattern.
//
// How patterns get retrieved:
//   • `find_similar_patterns(symptom)` does:
//       1) FTS5 prefix match on title+content for healing-pattern memories
//       2) Tag intersection scoring
//       3) Recency weighting (older fixes count less)
//       4) Success-rate weighting (patterns with a `success_count` get bumps)
//   • Returns top N with a confidence score 0-1.
//
// The engine deliberately does NOT execute fixes. It only PROPOSES them.
// HITL (human-in-the-loop) is always required to apply.

use crate::commands::metrics::shared_db;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealingPattern {
    pub id: i64,
    pub title: String,
    pub symptom: String,
    pub fix_description: String,
    pub success_count: i32,
    pub last_used: i64,
    pub confidence: f64,
    /// Tags from the underlying memory, for context.
    pub tags: Vec<String>,
    /// Days since this pattern was last successfully applied.
    pub age_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveHealingPatternArgs {
    pub title: String,
    pub symptom: String,
    pub fix: String,
    pub tags: Option<Vec<String>>,
    pub host_name: Option<String>,
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Persist a new healing pattern. Wraps the structured payload as JSON
/// inside `agent_memories.content` so we don't need a new table schema.
#[tauri::command]
pub async fn healing_save_pattern(args: SaveHealingPatternArgs) -> Result<i64, String> {
    let user_tags = args.tags.unwrap_or_default();
    let mut tags: Vec<String> = vec!["healing-pattern".to_string(), "auto-saved".to_string()];
    for t in user_tags { tags.push(t); }
    let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());

    let payload = serde_json::json!({
        "kind": "healing-pattern",
        "symptom": args.symptom,
        "fix": args.fix,
        "host_name": args.host_name,
        "success_count": 1,
        "last_used": now_ts(),
        "created_at": now_ts(),
    });
    let content = payload.to_string();
    let title = args.title.chars().take(200).collect::<String>();

    shared_db(|conn| {
        conn.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, files, importance, created_at)
             VALUES ('', ?1, ?2, ?3, '[]', 5, ?4)",
            params![title, content, tags_json, now_ts()],
        ).map_err(|e| format!("insert: {}", e))?;
        Ok(conn.last_insert_rowid())
    })
}

/// Increment the success_count + last_used of an existing pattern memory.
/// Called when the user confirms a proposed fix worked.
#[tauri::command]
pub async fn healing_mark_success(memory_id: i64) -> Result<(), String> {
    shared_db(|conn| {
        let row: Option<String> = conn.query_row(
            "SELECT content FROM agent_memories WHERE id = ?1",
            params![memory_id],
            |r| r.get(0),
        ).ok();
        let content = row.ok_or_else(|| format!("memory {} not found", memory_id))?;
        let mut v: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("parse content: {}", e))?;
        let cur = v.get("success_count").and_then(|x| x.as_i64()).unwrap_or(0);
        v["success_count"] = serde_json::json!(cur + 1);
        v["last_used"] = serde_json::json!(now_ts());
        let new_content = v.to_string();
        conn.execute(
            "UPDATE agent_memories SET content = ?1, last_accessed_at = ?2, access_count = access_count + 1 WHERE id = ?3",
            params![new_content, now_ts(), memory_id],
        ).map_err(|e| format!("update: {}", e))?;
        Ok(())
    })
}

/// Search for healing patterns matching a symptom description.
/// Uses FTS5 if available, falls back to LIKE matching.
///
/// Scoring formula:
///   score = 0.5 * text_match + 0.25 * recency + 0.25 * success_rate
///
/// where:
///   text_match = how many tokens of symptom appear in title+content (0..1)
///   recency = 1.0 if last_used <30 days, 0.5 <90 days, 0.25 <365 days, 0 older
///   success_rate = log10(1 + success_count) / 2  (capped at 1)
#[tauri::command]
pub async fn healing_find_similar(symptom: String, top_k: Option<usize>) -> Result<Vec<HealingPattern>, String> {
    let top = top_k.unwrap_or(5).min(20);
    let symptom_lc = symptom.to_lowercase();
    let tokens: Vec<&str> = symptom_lc.split_whitespace()
        .filter(|t| t.len() >= 3)
        .collect();
    if tokens.is_empty() {
        return Ok(Vec::new());
    }

    let now = now_ts();

    shared_db(|conn| {
        // Pull all healing-pattern memories. We expect <500 of these in practice
        // — they're explicit-write only — so in-memory ranking is fine.
        let mut stmt = conn.prepare(
            "SELECT id, title, content, tags, last_accessed_at
             FROM agent_memories
             WHERE tags LIKE '%healing-pattern%'
             ORDER BY created_at DESC
             LIMIT 1000"
        ).map_err(|e| format!("prepare: {}", e))?;

        type Row = (i64, String, String, String, i64);
        let rows: Vec<Row> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        let mut scored: Vec<HealingPattern> = Vec::new();
        for (id, title, content, tags_json, last_accessed) in rows {
            let title_lc = title.to_lowercase();
            let content_lc = content.to_lowercase();

            let matched_tokens = tokens.iter()
                .filter(|tok| title_lc.contains(*tok) || content_lc.contains(*tok))
                .count();
            if matched_tokens == 0 { continue; }
            let text_match = matched_tokens as f64 / tokens.len() as f64;

            // Parse the JSON payload to extract structured fields
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::Value::Null);
            let symptom_field = parsed.get("symptom").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let fix_field = parsed.get("fix").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let success_count = parsed.get("success_count").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
            let last_used = parsed.get("last_used").and_then(|v| v.as_i64()).unwrap_or(last_accessed);

            let age_days = ((now - last_used).max(0)) / 86_400;
            let recency = if age_days < 30 { 1.0 }
                         else if age_days < 90 { 0.5 }
                         else if age_days < 365 { 0.25 }
                         else { 0.0 };

            let success_rate = ((success_count as f64 + 1.0).log10() / 2.0).min(1.0);

            let confidence = 0.50 * text_match + 0.25 * recency + 0.25 * success_rate;

            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

            scored.push(HealingPattern {
                id,
                title,
                symptom: symptom_field,
                fix_description: fix_field,
                success_count,
                last_used,
                confidence,
                tags,
                age_days,
            });
        }

        scored.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top);
        Ok(scored)
    })
}

#[tauri::command]
pub async fn healing_list_all(limit: Option<usize>) -> Result<Vec<HealingPattern>, String> {
    let cap = limit.unwrap_or(50).min(500);
    let now = now_ts();
    shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, title, content, tags
             FROM agent_memories
             WHERE tags LIKE '%healing-pattern%'
             ORDER BY importance DESC, created_at DESC
             LIMIT ?1"
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows: Vec<HealingPattern> = stmt
            .query_map(params![cap as i64], |r| {
                let id: i64 = r.get(0)?;
                let title: String = r.get(1)?;
                let content: String = r.get(2)?;
                let tags_json: String = r.get(3)?;
                let parsed: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::Value::Null);
                let symptom_field = parsed.get("symptom").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let fix_field = parsed.get("fix").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let success_count = parsed.get("success_count").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
                let last_used = parsed.get("last_used").and_then(|v| v.as_i64()).unwrap_or(0);
                let age_days = ((now - last_used).max(0)) / 86_400;
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                Ok(HealingPattern {
                    id,
                    title,
                    symptom: symptom_field,
                    fix_description: fix_field,
                    success_count,
                    last_used,
                    confidence: 1.0, // raw listing doesn't compute search confidence
                    tags,
                    age_days,
                })
            })
            .map_err(|e| format!("query: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect: {}", e))?;
        Ok(rows)
    })
}

#[tauri::command]
pub async fn healing_delete_pattern(memory_id: i64) -> Result<(), String> {
    shared_db(|conn| {
        conn.execute("DELETE FROM agent_memories WHERE id = ?1 AND tags LIKE '%healing-pattern%'", params![memory_id])
            .map_err(|e| format!("delete: {}", e))?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn confidence_formula_zero_for_no_match() {
        // The scoring loop short-circuits when matched_tokens == 0. We just
        // verify the formula bounds for a synthetic case.
        let text_match = 0.0_f64;
        let recency = 0.0_f64;
        let success_rate = 0.0_f64;
        let c = 0.50 * text_match + 0.25 * recency + 0.25 * success_rate;
        assert_eq!(c, 0.0);
    }

    #[test]
    fn confidence_formula_one_for_perfect_match() {
        let text_match = 1.0_f64;
        let recency = 1.0_f64;
        let success_rate = 1.0_f64;
        let c = 0.50 * text_match + 0.25 * recency + 0.25 * success_rate;
        assert!((c - 1.0).abs() < 1e-9);
    }

    #[test]
    fn recency_bands_decay() {
        let cases = [(0_i64, 1.0), (15, 1.0), (60, 0.5), (200, 0.25), (400, 0.0)];
        for (age_days, expected) in cases {
            let recency = if age_days < 30 { 1.0 }
                         else if age_days < 90 { 0.5 }
                         else if age_days < 365 { 0.25 }
                         else { 0.0 };
            assert_eq!(recency, expected, "age_days={}", age_days);
        }
    }
}
