// ── replay.rs — Tier S #1 (Deterministic Replay Mode) ────────────────────
//
// Lets the user capture any LLM turn — the COMPLETE input that produced an
// answer — and re-run it later byte-for-byte. Critical for:
//
//   • Debugging "why did Lucy say X" — reproduce + tweak
//   • Forensic audit of past responses
//   • Comparing models on identical input (run snapshot through Sonnet,
//     then Opus, then Gemini — compare outputs)
//   • Prompt-engineering work: edit the system prompt, replay, see drift
//
// Why this is a non-replicable differentiator: no conventional AI tool
// (Cursor, Cline, Hermes, OpenInterpreter) stores the FULL serialized turn
// context. They store the chat transcript but the actual prompt sent to
// the provider (which mixes system rules + memories + working dir + history)
// is lost. Lucy captures it whole.
//
// Provider determinism caveats:
//   • Anthropic Claude — no seed parameter; temperature=0 is "best effort"
//   • OpenAI GPT       — `seed` + `system_fingerprint` give true determinism
//   • Gemini 2.0+      — `seed` (best-effort, version dependent)
//   • Ollama / local   — `seed` deterministic
//
// The capture pins `temperature=0.0` and stores `seed` if one was used.

use crate::commands::metrics::shared_db;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySnapshot {
    pub id: i64,
    pub created_at: i64,
    pub label: String,
    pub task_id: String,
    pub tab_id: String,
    pub model: String,
    pub effort: String,
    pub system_prompt: String,
    pub user_prompt: String,
    pub context_block: String,
    pub images_b64: String,
    pub original_response: String,
    pub original_tokens_in: i64,
    pub original_tokens_out: i64,
    pub original_latency_ms: i64,
    pub temperature: f64,
    pub seed: Option<i64>,
    pub replays_run: i64,
}

/// Slim metadata for list views — omits the huge `system_prompt` /
/// `context_block` columns to keep the JSON payload small.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayMeta {
    pub id: i64,
    pub created_at: i64,
    pub label: String,
    pub tab_id: String,
    pub model: String,
    pub effort: String,
    /// First 160 chars of user_prompt — enough for the row to be searchable
    /// without ferrying the whole thing across the IPC boundary.
    pub prompt_preview: String,
    pub original_tokens_in: i64,
    pub original_tokens_out: i64,
    pub original_latency_ms: i64,
    pub replays_run: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct ReplaySaveArgs {
    pub label: Option<String>,
    pub task_id: Option<String>,
    pub tab_id: Option<String>,
    pub model: String,
    pub effort: Option<String>,
    pub system_prompt: String,
    pub user_prompt: String,
    pub context_block: Option<String>,
    pub images_b64: Option<String>,
    pub original_response: String,
    pub original_tokens_in: Option<i64>,
    pub original_tokens_out: Option<i64>,
    pub original_latency_ms: Option<i64>,
    pub temperature: Option<f64>,
    pub seed: Option<i64>,
}

/// Save a complete snapshot of an LLM turn.
///
/// Returns the new row id so the frontend can immediately reference it
/// (link in the chat bubble, etc).
#[tauri::command]
pub async fn replay_save(args: ReplaySaveArgs) -> Result<i64, String> {
    shared_db(move |conn| {
        conn.execute(
            "INSERT INTO replay_snapshots (
                label, task_id, tab_id, model, effort,
                system_prompt, user_prompt, context_block, images_b64,
                original_response, original_tokens_in, original_tokens_out,
                original_latency_ms, temperature, seed
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                &args.label.unwrap_or_default(),
                &args.task_id.unwrap_or_default(),
                &args.tab_id.unwrap_or_default(),
                &args.model,
                &args.effort.unwrap_or_default(),
                &args.system_prompt,
                &args.user_prompt,
                &args.context_block.unwrap_or_default(),
                &args.images_b64.unwrap_or_else(|| "[]".to_string()),
                &args.original_response,
                args.original_tokens_in.unwrap_or(0),
                args.original_tokens_out.unwrap_or(0),
                args.original_latency_ms.unwrap_or(0),
                args.temperature.unwrap_or(0.0),
                args.seed,
            ],
        ).map_err(|e| format!("replay_save insert: {}", e))?;
        Ok(conn.last_insert_rowid())
    })
}

/// List recent snapshots — newest first. `limit` defaults to 50, capped at 500.
#[tauri::command]
pub async fn replay_list(
    limit: Option<u32>,
    tab_id: Option<String>,
) -> Result<Vec<ReplayMeta>, String> {
    let lim = limit.unwrap_or(50).min(500) as i64;
    shared_db(move |conn| {
        fn read_meta(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReplayMeta> {
            let user_prompt: String = row.get(5)?;
            Ok(ReplayMeta {
                id: row.get(0)?,
                created_at: row.get(1)?,
                label: row.get(2)?,
                tab_id: row.get(3)?,
                model: row.get(4)?,
                effort: "".to_string(), // filled below
                prompt_preview: user_prompt.chars().take(160).collect(),
                original_tokens_in: row.get(6)?,
                original_tokens_out: row.get(7)?,
                original_latency_ms: row.get(8)?,
                replays_run: row.get(9)?,
            })
        }
        // We split effort into a second column read to keep the SELECT tidy.
        // Materializamos `rows` DENTRO de cada bloque para que `stmt` viva lo
        // suficiente (el query_map devuelve un iterator atado a la vida del
        // statement, así que el .collect() debe pasar antes del `}`).
        let rows: Vec<ReplayMeta> = if let Some(ref tid) = tab_id {
            let mut stmt = conn.prepare(
                "SELECT id, created_at, label, tab_id, model, user_prompt,
                        original_tokens_in, original_tokens_out, original_latency_ms,
                        replays_run, effort
                 FROM replay_snapshots
                 WHERE tab_id = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2"
            ).map_err(|e| format!("replay_list prepare (tab): {}", e))?;
            let v: Vec<ReplayMeta> = stmt.query_map(params![tid, lim], |row| {
                let mut m = read_meta(row)?;
                m.effort = row.get::<_, String>(10).unwrap_or_default();
                Ok(m)
            }).map_err(|e| format!("replay_list query (tab): {}", e))?
              .filter_map(|r| r.ok()).collect();
            v
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, created_at, label, tab_id, model, user_prompt,
                        original_tokens_in, original_tokens_out, original_latency_ms,
                        replays_run, effort
                 FROM replay_snapshots
                 ORDER BY created_at DESC
                 LIMIT ?1"
            ).map_err(|e| format!("replay_list prepare: {}", e))?;
            let v: Vec<ReplayMeta> = stmt.query_map(params![lim], |row| {
                let mut m = read_meta(row)?;
                m.effort = row.get::<_, String>(10).unwrap_or_default();
                Ok(m)
            }).map_err(|e| format!("replay_list query: {}", e))?
              .filter_map(|r| r.ok()).collect();
            v
        };
        Ok(rows)
    })
}

/// Fetch a full snapshot — including the heavy `system_prompt`,
/// `context_block`, and `original_response` columns the list view omits.
#[tauri::command]
pub async fn replay_get(id: i64) -> Result<Option<ReplaySnapshot>, String> {
    shared_db(move |conn| {
        let row = conn.query_row(
            "SELECT id, created_at, label, task_id, tab_id, model, effort,
                    system_prompt, user_prompt, context_block, images_b64,
                    original_response, original_tokens_in, original_tokens_out,
                    original_latency_ms, temperature, seed, replays_run
             FROM replay_snapshots WHERE id = ?1",
            params![id],
            |r| Ok(ReplaySnapshot {
                id: r.get(0)?,
                created_at: r.get(1)?,
                label: r.get(2)?,
                task_id: r.get(3)?,
                tab_id: r.get(4)?,
                model: r.get(5)?,
                effort: r.get(6)?,
                system_prompt: r.get(7)?,
                user_prompt: r.get(8)?,
                context_block: r.get(9)?,
                images_b64: r.get(10)?,
                original_response: r.get(11)?,
                original_tokens_in: r.get(12)?,
                original_tokens_out: r.get(13)?,
                original_latency_ms: r.get(14)?,
                temperature: r.get(15)?,
                seed: r.get(16)?,
                replays_run: r.get(17)?,
            }),
        ).ok();
        Ok(row)
    })
}

/// Bump the replays_run counter — called by the frontend after a successful
/// re-execution so the list view can show "ran 3×" badges.
#[tauri::command]
pub async fn replay_bump_count(id: i64) -> Result<(), String> {
    shared_db(move |conn| {
        conn.execute(
            "UPDATE replay_snapshots SET replays_run = replays_run + 1 WHERE id = ?1",
            params![id],
        ).map_err(|e| format!("replay_bump_count: {}", e))?;
        Ok(())
    })
}

/// Update the label of a snapshot. Lets the user annotate "this was the
/// turn where Lucy broke X".
#[tauri::command]
pub async fn replay_relabel(id: i64, label: String) -> Result<(), String> {
    shared_db(move |conn| {
        conn.execute(
            "UPDATE replay_snapshots SET label = ?1 WHERE id = ?2",
            params![label, id],
        ).map_err(|e| format!("replay_relabel: {}", e))?;
        Ok(())
    })
}

#[tauri::command]
pub async fn replay_delete(id: i64) -> Result<(), String> {
    shared_db(move |conn| {
        conn.execute(
            "DELETE FROM replay_snapshots WHERE id = ?1",
            params![id],
        ).map_err(|e| format!("replay_delete: {}", e))?;
        Ok(())
    })
}

/// Housekeeping: prune snapshots older than `days` (default 30).
/// Returns count of deleted rows.
#[tauri::command]
pub async fn replay_clear_old(days: Option<u32>) -> Result<i64, String> {
    let d = days.unwrap_or(30) as i64;
    shared_db(move |conn| {
        let n = conn.execute(
            "DELETE FROM replay_snapshots
             WHERE created_at < strftime('%s','now') - ?1 * 86400",
            params![d],
        ).map_err(|e| format!("replay_clear_old: {}", e))?;
        Ok(n as i64)
    })
}

/// Compute a normalized drift score between original and replay outputs.
///
/// Returns a struct with multiple lenses so the UI can show whatever the
/// user finds intuitive:
///   • char_jaccard      — set similarity over 4-char shingles (0..1)
///   • length_delta_pct  — |new_len - orig_len| / max(orig_len, 1)
///   • is_identical      — bytes-equal
///
/// Why shingled char Jaccard over Levenshtein: Levenshtein on 5KB strings
/// is O(n*m) ≈ 25M ops. Shingled Jaccard is O(n) and good enough to spot
/// "essentially the same" vs "substantially different" at the UX level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftScore {
    pub char_jaccard: f32,
    pub length_delta_pct: f32,
    pub is_identical: bool,
}

#[tauri::command]
pub fn replay_drift(original: String, replay: String) -> DriftScore {
    let is_identical = original == replay;
    let orig_len = original.chars().count();
    let new_len = replay.chars().count();
    let length_delta_pct = if orig_len == 0 { 0.0 } else {
        (new_len as f32 - orig_len as f32).abs() / orig_len as f32
    };
    let char_jaccard = shingle_jaccard(&original, &replay, 4);
    DriftScore { char_jaccard, length_delta_pct, is_identical }
}

fn shingle_jaccard(a: &str, b: &str, k: usize) -> f32 {
    use std::collections::HashSet;
    fn shingles(s: &str, k: usize) -> HashSet<String> {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() < k { return HashSet::new(); }
        let mut out: HashSet<String> = HashSet::new();
        for i in 0..=(chars.len() - k) {
            out.insert(chars[i..i + k].iter().collect());
        }
        out
    }
    let sa = shingles(a, k);
    let sb = shingles(b, k);
    if sa.is_empty() && sb.is_empty() { return 1.0; }
    if sa.is_empty() || sb.is_empty() { return 0.0; }
    let inter = sa.intersection(&sb).count() as f32;
    let union = sa.union(&sb).count() as f32;
    inter / union
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_identical_strings_score_1() {
        let d = replay_drift("hello world".into(), "hello world".into());
        assert!(d.is_identical);
        assert!((d.char_jaccard - 1.0).abs() < 0.001);
        assert!((d.length_delta_pct - 0.0).abs() < 0.001);
    }

    #[test]
    fn drift_unrelated_strings_score_near_0() {
        let d = replay_drift("the quick brown fox".into(),
                             "lorem ipsum dolor sit amet".into());
        assert!(!d.is_identical);
        assert!(d.char_jaccard < 0.20,
                "Expected jaccard < 0.20 for unrelated text, got {}", d.char_jaccard);
    }

    #[test]
    fn drift_minor_edits_keep_jaccard_high() {
        // A typo-fix-style edit should leave the bulk of 4-char shingles
        // identical. We pin "high" as ≥ 0.6 so the UX layer can confidently
        // call this "essentially the same".
        let a = "Lucy executed Get-Process and returned 134 entries.";
        let b = "Lucy executed Get-Process and returned 135 entries.";
        let d = replay_drift(a.into(), b.into());
        assert!(d.char_jaccard >= 0.6,
                "Single-char edit should keep jaccard high, got {}", d.char_jaccard);
        assert!(!d.is_identical);
    }

    #[test]
    fn drift_length_delta_normalizes() {
        let d = replay_drift("aaaa".into(), "aaaaaaaa".into());
        // 8 vs 4 chars → delta 4/4 = 1.0 (100%)
        assert!((d.length_delta_pct - 1.0).abs() < 0.001,
                "8 vs 4 chars should be 100% delta, got {}", d.length_delta_pct);
    }

    #[test]
    fn drift_empty_inputs_dont_panic() {
        // Edge case: an empty original (rare but possible — e.g. a turn that
        // got cancelled before any output). Must return defined values.
        let d = replay_drift(String::new(), "hello".into());
        assert!(!d.is_identical);
        assert!(d.char_jaccard >= 0.0 && d.char_jaccard <= 1.0);
    }
}
