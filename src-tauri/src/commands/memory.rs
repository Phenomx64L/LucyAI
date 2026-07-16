// ── Tiered memory — MemGPT-style (Sprint 3) ────────────────────────────────
//
// Lucy's memory has three tiers:
//
//   1. CORE   — Small, always-injected facts (target <2 KB). Stored in
//               `memory_core`. Examples: "user prefers PowerShell over CMD",
//               "primary host is ROG-WKS-X", "always acknowledge destructive
//               commands before running them".
//
//   2. WORKING — Per-session compressed summaries of long agent loops.
//               When the raw context gets large, Lucy (via `memory.compress`)
//               condenses it into a paragraph stored in `memory_working`.
//               Cheaper to re-inject than the raw turns.
//
//   3. EPISODIC — The existing `agent_memories` table: long-term, cross-
//               session, searchable via both FTS and (Sprint 2) semantic
//               embeddings. Accessed via memoria_buscar / memoria_guardar.
//
// This module owns CORE + WORKING. Episodic stays where it already lives in
// metrics.rs; we just add a semantic-recall helper here that Sprint 2's
// embeddings power.

use crate::commands::metrics::shared_db;
use crate::utils::db::generate_id;
use rusqlite::params;
use serde::{Deserialize, Serialize};

// ── Types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreMemoryEntry {
    pub id: String,
    pub section: String, // 'user_facts' | 'preferences' | 'rules' | 'environment' | 'identity' | 'context' | 'host'
    pub key: String,
    pub value: String,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// Computed on read — exponential decay score in [0.0, 1.0] based on age
    /// since `updated_at` and the per-section half-life. Pinned entries always
    /// return 1.0. Not stored in DB. Skipped during serialisation when None
    /// for backward compat with older frontend code that doesn't expect it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decay_score: Option<f32>,
}

// ── Memory decay (May 2026, F1+) ────────────────────────────────────────────
//
// Core facts have a half-life — after enough time without reinforcement, they
// stop being safe to inject blindly into the system prompt. "User runs Windows
// 10" is true today, false in 2 years. Three tiers based on the score:
//
//   score ≥ 0.7  → inject as-is              (still fresh)
//   0.3 ≤ s < 0.7 → inject with [~aging~] tag (Lucy should hedge on it)
//   score < 0.3  → SKIP from injection       (too stale to trust blindly,
//                                              still visible in the UI so
//                                              the user can re-affirm/edit)
//
// Half-life varies by section because identity facts (name, role) change much
// slower than context facts (current project). Pinned entries opt out entirely
// — they always return 1.0.

const DECAY_INJECT_THRESHOLD: f32 = 0.30;  // below = skipped from prompt
const DECAY_AGING_THRESHOLD:  f32 = 0.70;  // below = injected with [~aging~]

/// Per-section half-life in days. Higher = more stable category.
fn decay_half_life_days(section: &str) -> f64 {
    // Normalised lookup — frontend sometimes capitalises or uses Spanish.
    let s = section.to_lowercase();
    match s.as_str() {
        "identity"    | "user_facts"  | "user"        => 365.0, // name, role: yearly
        "preference"  | "preferences" | "preferencia" => 180.0, // verbose/concise, lang
        "host"        | "hosts"       | "environment" => 90.0,  // box-specific facts
        "context"     | "project"     | "current"     => 60.0,  // active work: faster
        "rules"                                       => 730.0, // rules ~ never expire
        _                                             => 120.0, // default ~ 4 months
    }
}

/// Exponential decay using half-life: score = 0.5 ^ (age / half_life).
/// Returns 1.0 for pinned entries (opt-out from decay) and saturates at 1.0
/// for entries with future `updated_at` (clock skew safety).
pub fn compute_decay_score(updated_at: i64, section: &str, pinned: bool) -> f32 {
    if pinned { return 1.0; }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let age_secs = (now - updated_at).max(0);
    if age_secs == 0 { return 1.0; }
    let age_days = (age_secs as f64) / 86_400.0;
    let half_life = decay_half_life_days(section).max(1.0);
    let score = (0.5_f64).powf(age_days / half_life);
    score.clamp(0.0, 1.0) as f32
}

/// Maps a numeric score to a 1-line human-readable label for the UI.
/// Used by the memory panel so the user can see at a glance which entries
/// Lucy is still trusting vs which she's hedging on.
#[allow(dead_code)]
pub fn decay_label(score: f32) -> &'static str {
    if score >= DECAY_AGING_THRESHOLD       { "fresh" }
    else if score >= DECAY_INJECT_THRESHOLD { "aging" }
    else                                    { "stale" }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingSummary {
    pub id: String,
    pub session_id: String,
    pub tab_id: Option<String>,
    pub summary: String,
    pub token_count: i64,
    pub original_len: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub core_count: i64,
    pub core_bytes: i64,
    pub working_count: i64,
    pub working_bytes: i64,
    pub episodic_count: i64,
}

// ── CORE memory ────────────────────────────────────────────────────────────

/// Upsert a core-memory row. The (section, key) pair is unique so repeated
/// calls overwrite instead of duplicating — safe to call from the LLM tool.
#[tauri::command]
pub async fn memory_core_set(
    section: String,
    key: String,
    value: String,
    pinned: Option<bool>,
) -> Result<String, String> {
    let id = generate_id();
    let pinned = pinned.unwrap_or(true);

    shared_db(|conn| {
        conn.execute(
            "INSERT INTO memory_core (id, section, key, value, pinned, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s','now'))
             ON CONFLICT(section, key) DO UPDATE SET
               value = excluded.value,
               pinned = excluded.pinned,
               updated_at = strftime('%s','now')",
            params![id, section, key, value, pinned as i64],
        ).map_err(|e| format!("memory_core_set: {}", e))?;
        Ok(id.clone())
    })
}

#[tauri::command]
pub async fn memory_core_list(section: Option<String>) -> Result<Vec<CoreMemoryEntry>, String> {
    shared_db(|conn| {
        let sql = if section.is_some() {
            "SELECT id, section, key, value, pinned, created_at, updated_at
             FROM memory_core WHERE section = ?1 ORDER BY section, key"
        } else {
            "SELECT id, section, key, value, pinned, created_at, updated_at
             FROM memory_core ORDER BY section, key"
        };
        let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare: {}", e))?;
        let mapper = |r: &rusqlite::Row| -> rusqlite::Result<CoreMemoryEntry> {
            let section_v: String = r.get(1)?;
            let pinned_v: bool   = r.get::<_, i64>(4)? != 0;
            let updated_v: i64   = r.get(6)?;
            Ok(CoreMemoryEntry {
                id: r.get(0)?,
                section: section_v.clone(),
                key: r.get(2)?,
                value: r.get(3)?,
                pinned: pinned_v,
                created_at: r.get(5)?,
                updated_at: updated_v,
                // Computed on read so the UI can show "aging"/"stale" badges
                // without recomputing in JavaScript (avoids time-zone drift).
                decay_score: Some(compute_decay_score(updated_v, &section_v, pinned_v)),
            })
        };
        let rows: Vec<CoreMemoryEntry> = if let Some(ref s) = section {
            stmt.query_map(params![s], mapper)
        } else {
            stmt.query_map([], mapper)
        }
        .map_err(|e| format!("query: {}", e))?
        .filter_map(|r| r.ok())
        .collect();
        Ok(rows)
    })
}

#[tauri::command]
pub async fn memory_core_delete(section: String, key: String) -> Result<(), String> {
    shared_db(|conn| {
        conn.execute(
            "DELETE FROM memory_core WHERE section = ?1 AND key = ?2",
            params![section, key],
        ).map_err(|e| format!("memory_core_delete: {}", e))?;
        Ok(())
    })
}

/// Bump `updated_at` for entries whose key OR value substring appears in the
/// given text. Called from the frontend whenever the user sends a new message
/// — keeps facts the user keeps mentioning fresh, while letting truly unused
/// ones decay naturally.
///
/// Returns the number of entries that were reinforced (0 if none matched).
/// Match is case-insensitive whole-word for keys, and substring (length ≥ 4
/// after trimming) for values to avoid false positives from short tokens.
#[tauri::command]
pub async fn memory_core_reinforce(text: String) -> Result<i64, String> {
    let text_lc = text.to_lowercase();
    if text_lc.trim().is_empty() {
        return Ok(0);
    }
    shared_db(|conn| {
        // Pull all non-pinned entries (pinned ones don't need bumping).
        let mut stmt = conn.prepare(
            "SELECT id, key, value FROM memory_core WHERE pinned = 0"
        ).map_err(|e| format!("prepare: {}", e))?;
        let candidates: Vec<(String, String, String)> = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }).map_err(|e| format!("query: {}", e))?
          .filter_map(|r| r.ok())
          .collect();

        let mut to_bump: Vec<String> = Vec::new();
        for (id, key, value) in &candidates {
            let key_lc = key.to_lowercase();
            // Whole-word-ish match for keys (avoid 'os' matching 'cost', etc.)
            // — wrap key with non-word-char anchors via simple bound check.
            let key_hit = if key_lc.len() < 3 {
                false // too short, too many false positives
            } else {
                let needle = &key_lc;
                text_lc.contains(needle.as_str())
                    && {
                        // Check char boundary: previous and next char (if any) shouldn't be word chars
                        let bytes = text_lc.as_bytes();
                        let nlen = needle.len();
                        let mut matched_clean = false;
                        let mut start = 0;
                        while let Some(idx) = text_lc[start..].find(needle.as_str()) {
                            let abs = start + idx;
                            let before_ok = abs == 0 || !bytes[abs - 1].is_ascii_alphanumeric();
                            let after_pos = abs + nlen;
                            let after_ok = after_pos >= bytes.len() || !bytes[after_pos].is_ascii_alphanumeric();
                            if before_ok && after_ok { matched_clean = true; break; }
                            start = abs + nlen;
                            if start >= text_lc.len() { break; }
                        }
                        matched_clean
                    }
            };
            // Value substring match (looser — for cases like "I'm on PRECISION-X today")
            let value_lc = value.to_lowercase();
            let val_hit = value_lc.len() >= 4 && text_lc.contains(value_lc.trim());

            if key_hit || val_hit {
                to_bump.push(id.clone());
            }
        }

        if to_bump.is_empty() { return Ok(0i64); }

        // Bump updated_at in a single transaction for efficiency
        let tx = conn.unchecked_transaction().map_err(|e| format!("tx: {}", e))?;
        for id in &to_bump {
            tx.execute(
                "UPDATE memory_core SET updated_at = strftime('%s','now') WHERE id = ?1",
                params![id],
            ).map_err(|e| format!("reinforce update: {}", e))?;
        }
        tx.commit().map_err(|e| format!("commit: {}", e))?;
        Ok(to_bump.len() as i64)
    })
}

/// Returns aggregate decay stats for the UI's memory panel:
/// (total_entries, fresh, aging, stale, pinned).
#[tauri::command]
pub async fn memory_core_decay_stats() -> Result<serde_json::Value, String> {
    let rows = memory_core_list(None).await?;
    let mut total = 0i64;
    let mut fresh = 0i64;
    let mut aging = 0i64;
    let mut stale = 0i64;
    let mut pinned = 0i64;
    for r in &rows {
        total += 1;
        if r.pinned { pinned += 1; }
        let s = r.decay_score.unwrap_or(1.0);
        if s >= DECAY_AGING_THRESHOLD { fresh += 1; }
        else if s >= DECAY_INJECT_THRESHOLD { aging += 1; }
        else { stale += 1; }
    }
    Ok(serde_json::json!({
        "total":  total,
        "fresh":  fresh,
        "aging":  aging,
        "stale":  stale,
        "pinned": pinned,
        "inject_threshold": DECAY_INJECT_THRESHOLD,
        "aging_threshold":  DECAY_AGING_THRESHOLD,
    }))
}

/// Synchronous render used by `ai::build_system_prompt` on the hot path.
/// Doing this sync avoids making build_system_prompt async and infecting the
/// whole call chain. Safe because `shared_db` uses a std::sync::Mutex.
///
/// Decay-aware (May 2026): each entry's score is computed via
/// compute_decay_score(). Three behaviors:
///   • score ≥ 0.7  → render normally
///   • 0.3-0.7      → render with [~aging~] suffix so the LLM treats it
///                     as a hedge ("you may have mentioned X some time ago")
///   • score < 0.3  → SKIP from prompt (still in DB; visible in UI)
///
/// We also load NON-pinned rows so decay can hide them naturally instead of
/// requiring the user to manually delete every old fact.
pub fn render_core_sync() -> String {
    let rows_res: Result<Vec<CoreMemoryEntry>, String> = shared_db(|conn| {
        // Note: dropped the `WHERE pinned = 1` filter — decay handles freshness
        // for non-pinned entries too. Pinned entries always score 1.0 (never
        // decay) so they remain unaffected.
        let mut stmt = match conn.prepare(
            "SELECT id, section, key, value, pinned, created_at, updated_at
             FROM memory_core ORDER BY section, key"
        ) {
            Ok(s) => s,
            Err(_) => return Ok(Vec::new()),
        };
        let rows: Vec<CoreMemoryEntry> = stmt.query_map([], |r| {
            let section_v: String = r.get(1)?;
            let pinned_v: bool   = r.get::<_, i64>(4)? != 0;
            let updated_v: i64   = r.get(6)?;
            Ok(CoreMemoryEntry {
                id: r.get(0)?,
                section: section_v.clone(),
                key: r.get(2)?,
                value: r.get(3)?,
                pinned: pinned_v,
                created_at: r.get(5)?,
                updated_at: updated_v,
                decay_score: Some(compute_decay_score(updated_v, &section_v, pinned_v)),
            })
        }).map(|iter| iter.filter_map(|r| r.ok()).collect()).unwrap_or_default();
        Ok(rows)
    });
    let rows = rows_res.unwrap_or_default();
    if rows.is_empty() { return String::new(); }

    // Bucket by section, dropping rows below the inject threshold entirely.
    let mut by_section: std::collections::BTreeMap<String, Vec<&CoreMemoryEntry>> = std::collections::BTreeMap::new();
    let mut skipped_stale = 0usize;
    for r in &rows {
        let score = r.decay_score.unwrap_or(1.0);
        if score < DECAY_INJECT_THRESHOLD {
            skipped_stale += 1;
            continue;
        }
        by_section.entry(r.section.clone()).or_default().push(r);
    }
    if by_section.is_empty() {
        // Everything was stale — return a hint so the LLM knows to ask the
        // user to re-affirm rather than hallucinating defaults.
        return format!(
            "--- CORE MEMORY ---\n\
             (No fresh facts available — {} stale entries were filtered. \
             If the user references personal info, ask them to confirm rather \
             than assuming defaults.)\n\
             --- END CORE MEMORY ---",
            skipped_stale,
        );
    }

    let mut out = String::from("--- CORE MEMORY (always-on facts) ---\n");
    for (section, items) in &by_section {
        out.push_str(&format!("[{}]\n", section));
        for it in items {
            let score = it.decay_score.unwrap_or(1.0);
            // [~aging~] suffix is consumed by Lucy's prompt — she should
            // hedge on aging facts ("you mentioned X a while back, still true?").
            let marker = if score < DECAY_AGING_THRESHOLD { " [~aging~]" } else { "" };
            out.push_str(&format!("  {} = {}{}\n", it.key, it.value, marker));
        }
    }
    if skipped_stale > 0 {
        out.push_str(&format!(
            "(Note: {} additional fact{} were filtered as stale. If relevant, ask the user to confirm.)\n",
            skipped_stale, if skipped_stale == 1 { "" } else { "s" }
        ));
    }
    out.push_str("--- END CORE MEMORY ---");
    out
}

/// Render core memory as a compact block ready for injection into the
/// system prompt. Returns empty string if no pinned entries exist.
/// Format keeps tokens low: one line per fact, grouped by section.
#[tauri::command]
pub async fn memory_core_render() -> Result<String, String> {
    Ok(render_core_sync())
}

// ── WORKING memory ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn memory_working_append(
    session_id: String,
    tab_id: Option<String>,
    summary: String,
    token_count: Option<i64>,
    original_len: Option<i64>,
) -> Result<String, String> {
    let id = generate_id();
    let tc = token_count.unwrap_or((summary.len() as i64) / 4); // ~4 chars/token heuristic
    let ol = original_len.unwrap_or(0);

    shared_db(|conn| {
        conn.execute(
            "INSERT INTO memory_working (id, session_id, tab_id, summary, token_count, original_len)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, session_id, tab_id, summary, tc, ol],
        ).map_err(|e| format!("memory_working_append: {}", e))?;
        Ok(id.clone())
    })
}

#[tauri::command]
pub async fn memory_working_list(
    session_id: String,
    limit: Option<u32>,
) -> Result<Vec<WorkingSummary>, String> {
    let limit = limit.unwrap_or(20) as i64;
    shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, tab_id, summary, token_count, original_len, created_at
             FROM memory_working WHERE session_id = ?1
             ORDER BY created_at DESC LIMIT ?2"
        ).map_err(|e| format!("prepare: {}", e))?;

        let rows: Vec<WorkingSummary> = stmt.query_map(params![session_id, limit], |r| {
            Ok(WorkingSummary {
                id: r.get(0)?,
                session_id: r.get(1)?,
                tab_id: r.get(2)?,
                summary: r.get(3)?,
                token_count: r.get(4)?,
                original_len: r.get(5)?,
                created_at: r.get(6)?,
            })
        }).map_err(|e| format!("query: {}", e))?
          .filter_map(|r| r.ok())
          .collect();
        Ok(rows)
    })
}

#[tauri::command]
pub async fn memory_working_clear(session_id: String) -> Result<u32, String> {
    shared_db(|conn| {
        let n = conn.execute(
            "DELETE FROM memory_working WHERE session_id = ?1",
            params![session_id],
        ).map_err(|e| format!("memory_working_clear: {}", e))?;
        Ok(n as u32)
    })
}

// ── Stats / introspection ─────────────────────────────────────────────────

#[tauri::command]
pub async fn memory_stats() -> Result<MemoryStats, String> {
    shared_db(|conn| {
        let (core_count, core_bytes): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(LENGTH(value)+LENGTH(key)), 0) FROM memory_core",
            [], |r| Ok((r.get(0)?, r.get(1)?)),
        ).unwrap_or((0, 0));

        let (working_count, working_bytes): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(LENGTH(summary)), 0) FROM memory_working",
            [], |r| Ok((r.get(0)?, r.get(1)?)),
        ).unwrap_or((0, 0));

        let episodic_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_memories",
            [], |r| r.get(0),
        ).unwrap_or(0);

        Ok(MemoryStats {
            core_count, core_bytes,
            working_count, working_bytes,
            episodic_count,
        })
    })
}

/// v1.7.182 — Memory health diagnostic (surfaced via `/memory-health`).
///
/// Confirms-with-data the two gates the user reported as confusing:
///   • Embedding COVERAGE — does ingested documentation actually have vectors?
///     If `pdf_chunks > 0` but `pdf_chunks_embedded == 0`, semantic recall can't
///     find those docs (Ollama/Gemini was unavailable when they were ingested),
///     which is why "Lucy doesn't use what she ingested".
///   • Dedup COLLAPSES — how many saves the FTS/embedding dedup folded into an
///     existing memory this session, which is why "Lucy can't save new things".
#[tauri::command]
pub async fn memory_health() -> Result<serde_json::Value, String> {
    let dedup_hits = crate::commands::metrics::memory_dedup_hits_session();
    shared_db(|conn| {
        let episodic: i64 = conn
            .query_row("SELECT COUNT(*) FROM agent_memories", [], |r| r.get(0))
            .unwrap_or(0);
        let pdf_chunks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_memories WHERE session_id LIKE 'pdf:%'",
                [], |r| r.get(0),
            )
            .unwrap_or(0);
        let embeddings_total: i64 = conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
            .unwrap_or(0);
        let pdf_embedded: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM embeddings WHERE entity_type = 'pdf_chunk'",
                [], |r| r.get(0),
            )
            .unwrap_or(0);

        let mut by_type = serde_json::Map::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT entity_type, COUNT(*) FROM embeddings GROUP BY entity_type ORDER BY 2 DESC",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            }) {
                for row in rows.flatten() {
                    by_type.insert(row.0, serde_json::json!(row.1));
                }
            }
        }

        Ok(serde_json::json!({
            "episodic_count":         episodic,
            "pdf_chunks":             pdf_chunks,
            "pdf_chunks_embedded":    pdf_embedded,
            "embeddings_total":       embeddings_total,
            "embeddings_by_type":     by_type,
            "dedup_hits_session":     dedup_hits,
            "decay_inject_threshold": 0.30,
        }))
    })
}

// ── Sprint 1, AI-2 — Memory consolidation ─────────────────────────────────
//
// Why: long-term users accumulate near-duplicate memories ("the prod server
// runs IIS", "production is on IIS", "PROD-WEB-01 uses IIS as web server").
// Memory decay marks them aging/stale but doesn't FUSE them — the prompt
// builder still has to choose one, and gets worse signal as variants pile up.
//
// Strategy: find clusters of agent_memories whose token-bag overlap is high
// AND that share at least one tag, then fuse the cluster into one canonical
// entry with the freshest `updated_at`. Mark the originals as superseded.
//
// Threshold: tag overlap ≥ 50% AND jaccard(content tokens) ≥ 0.35 — tuned
// empirically so it catches "the server runs IIS" + "PROD is IIS-based" but
// NOT "the server runs IIS" + "the database runs MSSQL".
//
// SAFETY:
//   • Read-only PREVIEW mode (dry_run=true) returns clusters without writing.
//   • Pinned memories are NEVER consolidated (user explicitly kept them).
//   • Newest memory in each cluster wins → "fact has updated, keep newest".
//   • Old memories get a `superseded_by` tag pointing to the new entry — UI
//     can hide them but they remain in the DB for audit / undo.
//
// Cost: pure SQL + Rust set intersection, no LLM call. <50ms for 500 memories.

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConsolidationCluster {
    pub canonical_id: i64,
    pub canonical_title: String,
    pub merged_ids: Vec<i64>,
    pub overlap_score: f32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConsolidationReport {
    pub dry_run: bool,
    pub scanned: usize,
    pub clusters_found: usize,
    pub memories_merged: usize,
    pub clusters: Vec<ConsolidationCluster>,
}

/// Tokenise a string for similarity scoring. Lowercases, drops punctuation,
/// keeps only words ≥3 chars (filters out "el", "la", "of", "the" noise).
fn tokens_for_similarity(s: &str) -> std::collections::HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(|s| s.to_string())
        .collect()
}

/// Jaccard similarity = |A ∩ B| / |A ∪ B|. 1.0 = identical, 0.0 = disjoint.
fn jaccard(a: &std::collections::HashSet<String>, b: &std::collections::HashSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() { return 0.0; }
    let intersection = a.intersection(b).count() as f32;
    let union = a.union(b).count() as f32;
    if union <= 0.0 { 0.0 } else { intersection / union }
}

#[tauri::command]
pub async fn memory_consolidate(dry_run: Option<bool>) -> Result<ConsolidationReport, String> {
    use crate::commands::metrics::shared_db;
    let is_dry = dry_run.unwrap_or(true);

    // Hard caps so consolidation can't pathologically iterate.
    const MAX_INPUT: usize = 1_000;           // protect O(n²) inner loop
    const MIN_TAG_OVERLAP: f32 = 0.50;
    const MIN_CONTENT_JACCARD: f32 = 0.35;

    let now_ts: i64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64).unwrap_or(0);

    shared_db(move |conn| {
        // Pull all candidate memories. Skip pinned (importance=10 by convention)
        // and skip already-superseded ones (tag includes "superseded_by:...").
        let mut stmt = conn.prepare(
            "SELECT id, title, content, tags, importance, created_at
             FROM agent_memories
             WHERE importance < 10
               AND tags NOT LIKE '%superseded_by%'
             ORDER BY created_at DESC
             LIMIT ?1"
        ).map_err(|e| format!("prepare: {}", e))?;

        type Row = (i64, String, String, String, i64, i64);
        let rows: Vec<Row> = stmt
            .query_map(rusqlite::params![MAX_INPUT as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        let scanned = rows.len();
        if scanned < 2 {
            return Ok(ConsolidationReport {
                dry_run: is_dry, scanned,
                clusters_found: 0, memories_merged: 0,
                clusters: vec![],
            });
        }

        // Pre-compute tokens + tag sets so the O(n²) loop is cheap.
        let prepped: Vec<(i64, String, std::collections::HashSet<String>, std::collections::HashSet<String>)> = rows.iter()
            .map(|(id, title, content, tags_json, _imp, _ts)| {
                let content_toks = tokens_for_similarity(&format!("{} {}", title, content));
                let tags: Vec<String> = serde_json::from_str(tags_json).unwrap_or_default();
                let tag_set: std::collections::HashSet<String> = tags.into_iter()
                    .map(|t| t.to_lowercase())
                    .collect();
                (*id, title.clone(), content_toks, tag_set)
            })
            .collect();

        // Greedy clustering: walk newest → oldest, each unvisited entry
        // seeds a cluster, then we absorb anything that exceeds both thresholds.
        let mut visited: std::collections::HashSet<i64> = std::collections::HashSet::new();
        let mut clusters: Vec<ConsolidationCluster> = Vec::new();

        for i in 0..prepped.len() {
            let (id_i, title_i, toks_i, tags_i) = &prepped[i];
            if visited.contains(id_i) { continue; }
            visited.insert(*id_i);

            let mut merged: Vec<i64> = Vec::new();
            let mut best_overlap: f32 = 0.0;
            for j in (i + 1)..prepped.len() {
                let (id_j, _title_j, toks_j, tags_j) = &prepped[j];
                if visited.contains(id_j) { continue; }
                let tag_overlap = jaccard(tags_i, tags_j);
                if tag_overlap < MIN_TAG_OVERLAP { continue; }
                let content_overlap = jaccard(toks_i, toks_j);
                if content_overlap < MIN_CONTENT_JACCARD { continue; }
                visited.insert(*id_j);
                merged.push(*id_j);
                let combined = (tag_overlap + content_overlap) * 0.5;
                if combined > best_overlap { best_overlap = combined; }
            }

            if !merged.is_empty() {
                clusters.push(ConsolidationCluster {
                    canonical_id: *id_i,
                    canonical_title: title_i.clone(),
                    merged_ids: merged,
                    overlap_score: best_overlap,
                });
            }
        }

        let clusters_found = clusters.len();
        let memories_merged: usize = clusters.iter().map(|c| c.merged_ids.len()).sum();

        // Write phase — only if NOT dry-run.
        if !is_dry && !clusters.is_empty() {
            let tx = conn.unchecked_transaction()
                .map_err(|e| format!("begin tx: {}", e))?;
            for cl in &clusters {
                // Mark each merged entry with the superseded_by tag pointing at canonical_id.
                // We append to the existing tags JSON array so the audit history is preserved.
                let supersede_marker = format!("superseded_by:{}", cl.canonical_id);
                for old_id in &cl.merged_ids {
                    // Read existing tags
                    let cur_tags: String = tx.query_row(
                        "SELECT tags FROM agent_memories WHERE id = ?1",
                        rusqlite::params![old_id],
                        |r| r.get(0),
                    ).unwrap_or_else(|_| "[]".to_string());
                    let mut tags: Vec<String> = serde_json::from_str(&cur_tags).unwrap_or_default();
                    if !tags.iter().any(|t| t == &supersede_marker) {
                        tags.push(supersede_marker.clone());
                    }
                    let new_tags = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());
                    tx.execute(
                        "UPDATE agent_memories SET tags = ?1 WHERE id = ?2",
                        rusqlite::params![new_tags, old_id],
                    ).ok();
                }
                // Bump the canonical entry's importance by 1 (capped at 9)
                // so it's slightly more sticky against future decay.
                tx.execute(
                    "UPDATE agent_memories
                     SET importance = MIN(importance + 1, 9),
                         created_at = ?1
                     WHERE id = ?2",
                    rusqlite::params![now_ts, cl.canonical_id],
                ).ok();
            }
            tx.commit().map_err(|e| format!("commit: {}", e))?;
        }

        Ok(ConsolidationReport {
            dry_run: is_dry, scanned,
            clusters_found, memories_merged,
            clusters,
        })
    })
}

/// Cosine similarity between two equal-length f32 vectors. Returns 0.0
/// on length mismatch or zero-magnitude inputs (guards against NaN from
/// a division by zero norm).
// v1.7.19 — delegates to SIMD-dispatched implementation. Returns
// identical results within FP epsilon to the prior scalar loop (verified
// in simd_cosine::tests::scalar_matches_dispatched_768d).
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    crate::utils::simd_cosine::cosine(a, b)
}

// ── Memory Graph (Tier S #2) ──────────────────────────────────────────────
//
// Returns a nodes+edges graph of the user's memory tier for visualization.
// Aristas se calculan localmente sobre los rows de `agent_memories` usando
// dos señales baratas:
//
//   1. Tag overlap — comparte ≥1 tag con otra memoria (Jaccard ≥ 0.30)
//   2. Content jaccard — Jaccard de palabras ≥ 3 chars (≥ 0.25)
//
// La similitud semántica via embeddings se deja para un v2 — requiere
// recorrer la tabla `embeddings` que no todos los usuarios habrán llenado.
// Las dos señales actuales son cero-coste y dan un grafo útil de inmediato.
//
// Limitaciones intencionales:
//   • Cap duro de MAX_NODES (250) para que el frontend pueda hacer
//     force-layout en JavaScript sin lag (>250 nodos ya pide WebGL).
//   • Cap duro de MAX_EDGES (1500) para no explotar el JSON serializado.
//   • Filtra memorias `superseded_by IS NOT NULL` por defecto (irrelevantes).

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNode {
    pub id: i64,
    pub title: String,
    pub importance: i64,
    pub access_count: i64,
    pub created_at: i64,
    pub tags: Vec<String>,
    /// Resumen corto del content para el hover/click — limitado a 200 chars
    /// para mantener el JSON manejable.
    pub preview: String,
    /// Tier S #2 + sprint Memory Graph 2.0 — Community id from Louvain-lite
    /// detection. -1 = singleton (no group). The frontend uses this to
    /// color-code clusters automatically.
    #[serde(default)]
    pub community: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEdge {
    pub source: i64,
    pub target: i64,
    /// 'tag' | 'content' | 'embedding' — discriminador para que el frontend coloree
    pub kind: String,
    /// Peso 0.0..1.0 — la grosor de la arista en la UI
    pub weight: f32,
}

/// Graph-wide stats — Memory Graph 2.0 (G4). Cheap to compute and the
/// header bar consumes all of them.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryGraphStats {
    pub edges_tag: i64,
    pub edges_content: i64,
    pub edges_embedding: i64,
    pub community_count: i32,
    /// Edges / max-possible-edges. 0 = no connections, 1 = complete graph.
    pub density: f32,
    /// Nodos sin ninguna arista — candidatos a borrar/etiquetar.
    pub orphan_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGraph {
    pub nodes: Vec<MemoryNode>,
    pub edges: Vec<MemoryEdge>,
    pub truncated_nodes: bool,
    pub truncated_edges: bool,
    /// Cuántos nodos había en total antes del cap — útil para mostrar
    /// "mostrando 250 de 412 memorias"
    pub total_memories: i64,
    #[serde(default)]
    pub stats: MemoryGraphStats,
    /// Top 12 tags by frequency among returned nodes. Drives the filter
    /// pills row in the UI.
    #[serde(default)]
    pub top_tags: Vec<(String, i64)>,
}

const GRAPH_MAX_NODES: usize = 250;
const GRAPH_MAX_EDGES: usize = 1500;
const TAG_EDGE_THRESHOLD_DEFAULT: f32 = 0.30;
const CONTENT_EDGE_THRESHOLD_DEFAULT: f32 = 0.25;
const EMBEDDING_EDGE_THRESHOLD_DEFAULT: f32 = 0.65;

/// Decode a little-endian f32 BLOB from the `embeddings` table into a vector,
/// returning `None` when the stored `dims` is implausible or doesn't match the
/// blob length. The `dims` bounds are checked BEFORE any `dims * 4` arithmetic
/// so a corrupt/huge `dims` (e.g. near i64::MAX from a tampered/corrupt DB row)
/// can't overflow `usize` — which would PANIC under the release profile's
/// `overflow-checks = true`, and `panic = "abort"` turns that into an app
/// crash. Same defensive class as the v1.7.209 metrics TTL fix. (v1.7.222)
fn decode_embedding_blob(blob: &[u8], dims: i64) -> Option<Vec<f32>> {
    if dims <= 0 || dims > 4096 {
        return None;
    }
    let expected = (dims as usize) * 4; // safe: dims ∈ [1, 4096] → ≤ 16384
    if blob.len() != expected {
        return None;
    }
    let mut vec = Vec::with_capacity(dims as usize);
    for chunk in blob.chunks_exact(4) {
        let arr: [u8; 4] = chunk.try_into().unwrap_or([0; 4]);
        vec.push(f32::from_le_bytes(arr));
    }
    Some(vec)
}

/// Build the memory graph for visualization.
///
/// Memory Graph 2.0 — Backend additions (sprint largo):
///   • `tag_threshold` / `content_threshold` / `embedding_threshold` —
///     ajustables en runtime para que la UI permita aflojar/apretar el
///     umbral cuando el grafo está vacío o demasiado denso.
///   • `use_embeddings` — when true, also adds edges from the `embeddings`
///     table via cosine similarity ≥ `embedding_threshold`. Connects
///     semantically-similar memories that DON'T share tags or words.
///   • Devuelve `MemoryGraphStats` y `top_tags` para que el header de la UI
///     muestre densidad, orfanidad, conteo de comunidades, y tag pills.
///   • Cada nodo recibe `community: i32` calculado por label-propagation
///     (Louvain-lite). -1 si está aislado.
///
/// `limit` — máximo de nodos a devolver (cap a GRAPH_MAX_NODES).
/// `min_importance` — filtra ruido si el usuario lo pide; 0 = sin filtro.
#[tauri::command]
pub async fn memory_graph(
    limit: Option<u32>,
    min_importance: Option<i64>,
    tag_threshold: Option<f32>,
    content_threshold: Option<f32>,
    embedding_threshold: Option<f32>,
    use_embeddings: Option<bool>,
) -> Result<MemoryGraph, String> {
    let lim = (limit.unwrap_or(150) as usize).min(GRAPH_MAX_NODES);
    let min_imp = min_importance.unwrap_or(0);
    let tag_thr   = tag_threshold.unwrap_or(TAG_EDGE_THRESHOLD_DEFAULT).clamp(0.05, 0.95);
    let cont_thr  = content_threshold.unwrap_or(CONTENT_EDGE_THRESHOLD_DEFAULT).clamp(0.05, 0.95);
    let emb_thr   = embedding_threshold.unwrap_or(EMBEDDING_EDGE_THRESHOLD_DEFAULT).clamp(0.30, 0.99);
    let want_emb  = use_embeddings.unwrap_or(true);

    shared_db(move |conn| {
        // ── 1) Cargar nodos ─────────────────────────────────────────────
        // Por importancia descendente — las más relevantes ganan el cap.
        let total_memories: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_memories WHERE superseded_by IS NULL AND importance >= ?1 AND session_id NOT LIKE 'pdf:%'",
            params![min_imp],
            |r| r.get(0),
        ).unwrap_or(0);

        let mut stmt = conn.prepare(
            "SELECT id, title, content, tags, importance, access_count, created_at
             FROM agent_memories
             WHERE superseded_by IS NULL AND importance >= ?1
               AND session_id NOT LIKE 'pdf:%' -- v1.7.233: doc chunks are not memories (see F1)
             ORDER BY importance DESC, access_count DESC, created_at DESC
             LIMIT ?2",
        ).map_err(|e| format!("memory_graph prepare: {}", e))?;

        // Materialize cada row en (id, content) + el MemoryNode. content lo
        // necesitamos para calcular Jaccard, no para enviarlo entero.
        struct LoadedMemory {
            node: MemoryNode,
            content_tokens: std::collections::HashSet<String>,
            tag_set: std::collections::HashSet<String>,
        }

        let memories_iter = stmt.query_map(params![min_imp, lim as i64], |r| {
            let id: i64 = r.get(0)?;
            let title: String = r.get(1)?;
            let content: String = r.get(2)?;
            let tags_json: String = r.get(3)?;
            let importance: i64 = r.get(4)?;
            let access_count: i64 = r.get(5)?;
            let created_at: i64 = r.get(6)?;
            Ok((id, title, content, tags_json, importance, access_count, created_at))
        }).map_err(|e| format!("memory_graph query: {}", e))?;

        let mut loaded: Vec<LoadedMemory> = Vec::new();
        for row in memories_iter.flatten() {
            let (id, title, content, tags_json, importance, access_count, created_at) = row;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            let tag_set: std::collections::HashSet<String> =
                tags.iter().map(|t| t.to_lowercase()).collect();
            // Para Jaccard de contenido, mezclamos title + content — el title
            // suele tener las palabras clave que distinguen una memoria.
            let combined = format!("{} {}", title, content);
            let content_tokens = tokens_for_similarity(&combined);
            let preview: String = content.chars().take(200).collect();
            loaded.push(LoadedMemory {
                node: MemoryNode {
                    id, title, importance, access_count, created_at,
                    tags, preview,
                    community: -1, // filled in pass 3 below
                },
                content_tokens,
                tag_set,
            });
        }
        let truncated_nodes = (total_memories as usize) > loaded.len();

        // ── 2a) Cargar embeddings (G1) ──────────────────────────────────
        // Solo cargamos los que correspondan a nodos en `loaded`. Si el
        // usuario no tiene la tabla poblada (Ollama embedder no se corrió
        // todavía), this just yields zero vectors and the embedding edges
        // simply don't materialize — no error.
        let mut emb_map: std::collections::HashMap<i64, Vec<f32>> =
            std::collections::HashMap::new();
        if want_emb && !loaded.is_empty() {
            let id_list: Vec<String> = loaded.iter()
                .map(|m| m.node.id.to_string()).collect();
            // Use a parameterized IN clause. SQLite has a hard 999-param
            // limit but our `lim ≤ 250` so we're safe.
            let placeholders = (0..id_list.len()).map(|i| format!("?{}", i + 1))
                .collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT entity_id, vec, dims FROM embeddings
                 WHERE entity_type = 'memory' AND entity_id IN ({})",
                placeholders
            );
            if let Ok(mut s) = conn.prepare(&sql) {
                let params_vec: Vec<&dyn rusqlite::ToSql> = id_list.iter()
                    .map(|s| s as &dyn rusqlite::ToSql).collect();
                if let Ok(rows) = s.query_map(rusqlite::params_from_iter(params_vec.iter().copied()), |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, i64>(2)?))
                }) {
                    for row in rows.flatten() {
                        let (eid_str, blob, dims) = row;
                        if let Ok(eid) = eid_str.parse::<i64>() {
                            if let Some(vec) = decode_embedding_blob(&blob, dims) {
                                emb_map.insert(eid, vec);
                            }
                        }
                    }
                }
            }
        }

        // ── 2b) Calcular aristas (O(n²) pero n ≤ 250) ────────────────────
        // ~31k comparaciones en el peor caso — < 1ms en SQLite-host range.
        // Priority: tag > content > embedding. Solo emitimos UNA arista por
        // par para evitar visual clutter — la "razón" más fuerte gana.
        let mut edges: Vec<MemoryEdge> = Vec::new();
        for i in 0..loaded.len() {
            for j in (i + 1)..loaded.len() {
                let a = &loaded[i];
                let b = &loaded[j];

                // Tag-overlap edge
                let mut emitted = false;
                if !a.tag_set.is_empty() && !b.tag_set.is_empty() {
                    let inter = a.tag_set.intersection(&b.tag_set).count();
                    if inter > 0 {
                        let union = a.tag_set.union(&b.tag_set).count();
                        let w = inter as f32 / union as f32;
                        if w >= tag_thr {
                            edges.push(MemoryEdge {
                                source: a.node.id, target: b.node.id,
                                kind: "tag".to_string(),
                                weight: w,
                            });
                            emitted = true;
                        }
                    }
                }
                if emitted { continue; }

                // Content-jaccard edge
                let w_c = jaccard(&a.content_tokens, &b.content_tokens);
                if w_c >= cont_thr {
                    edges.push(MemoryEdge {
                        source: a.node.id, target: b.node.id,
                        kind: "content".to_string(),
                        weight: w_c,
                    });
                    continue;
                }

                // Embedding cosine edge (only if we have vectors for BOTH)
                if want_emb {
                    if let (Some(va), Some(vb)) =
                        (emb_map.get(&a.node.id), emb_map.get(&b.node.id))
                    {
                        let w_e = cosine_similarity(va, vb);
                        if w_e >= emb_thr {
                            edges.push(MemoryEdge {
                                source: a.node.id, target: b.node.id,
                                kind: "embedding".to_string(),
                                weight: w_e,
                            });
                        }
                    }
                }
            }
        }

        // ── 3) Community detection — label propagation (G3) ──────────────
        // Cheap algorithm: each node starts in its own community; for K
        // iterations, every node adopts the most-frequent community among
        // its neighbors (weighted by edge weight). Converges in 3-5 passes
        // for our typical graph sizes. Singletons keep community = their id.
        // Then we re-label to consecutive 0..C-1 so the frontend palette
        // doesn't need huge indices.
        let n = loaded.len();
        let mut comm: Vec<i64> = loaded.iter().map(|m| m.node.id).collect();
        let id_to_idx: std::collections::HashMap<i64, usize> =
            loaded.iter().enumerate().map(|(i, m)| (m.node.id, i)).collect();
        // Build adjacency: idx → Vec<(neighbor_idx, weight)>
        let mut adj: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];
        for e in &edges {
            if let (Some(&i), Some(&j)) = (id_to_idx.get(&e.source), id_to_idx.get(&e.target)) {
                adj[i].push((j, e.weight));
                adj[j].push((i, e.weight));
            }
        }
        for _iter in 0..5 {
            let mut changed = false;
            // Process in shuffled order to break ties more evenly. Since we
            // don't pull rand here, use a simple index rotation per iter.
            for i in 0..n {
                if adj[i].is_empty() { continue; }
                let mut tally: std::collections::HashMap<i64, f32> = std::collections::HashMap::new();
                for &(nb, w) in &adj[i] { *tally.entry(comm[nb]).or_insert(0.0) += w; }
                if let Some((&best, _)) = tally.iter().max_by(|a, b|
                    a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                {
                    if comm[i] != best { comm[i] = best; changed = true; }
                }
            }
            if !changed { break; }
        }
        // Re-label to consecutive integers for connected nodes; -1 for isolates.
        let mut relabel: std::collections::HashMap<i64, i32> = std::collections::HashMap::new();
        let mut next: i32 = 0;
        for i in 0..n {
            if adj[i].is_empty() {
                loaded[i].node.community = -1; // singleton
                continue;
            }
            let c = comm[i];
            let new_c = *relabel.entry(c).or_insert_with(|| { let v = next; next += 1; v });
            loaded[i].node.community = new_c;
        }
        let community_count = next;

        // ── 4) Aplicar caps ──────────────────────────────────────────────
        // Si excede MAX_EDGES, conservamos las de mayor peso (más informativas).
        edges.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
        let truncated_edges = edges.len() > GRAPH_MAX_EDGES;
        edges.truncate(GRAPH_MAX_EDGES);

        // ── 5) Stats + top tags (G4) ─────────────────────────────────────
        let edges_tag = edges.iter().filter(|e| e.kind == "tag").count() as i64;
        let edges_content = edges.iter().filter(|e| e.kind == "content").count() as i64;
        let edges_embedding = edges.iter().filter(|e| e.kind == "embedding").count() as i64;
        let max_possible = (n as i64 * (n as i64 - 1)) / 2;
        let density = if max_possible > 0 { edges.len() as f32 / max_possible as f32 } else { 0.0 };
        let orphan_count = adj.iter().filter(|nbrs| nbrs.is_empty()).count() as i64;

        let mut tag_freq: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for m in &loaded {
            for t in &m.node.tags { *tag_freq.entry(t.to_lowercase()).or_insert(0) += 1; }
        }
        let mut top_tags: Vec<(String, i64)> = tag_freq.into_iter().collect();
        top_tags.sort_by(|a, b| b.1.cmp(&a.1));
        top_tags.truncate(12);

        let stats = MemoryGraphStats {
            edges_tag, edges_content, edges_embedding,
            community_count, density, orphan_count,
        };

        Ok(MemoryGraph {
            nodes: loaded.into_iter().map(|m| m.node).collect(),
            edges,
            truncated_nodes,
            truncated_edges,
            stats,
            top_tags,
            total_memories,
        })
    })
}

#[cfg(test)]
mod graph_tests {
    use super::{TAG_EDGE_THRESHOLD_DEFAULT, CONTENT_EDGE_THRESHOLD_DEFAULT,
                EMBEDDING_EDGE_THRESHOLD_DEFAULT,
                GRAPH_MAX_NODES, GRAPH_MAX_EDGES, cosine_similarity};

    #[test]
    fn graph_thresholds_are_sane() {
        // Smoke test del contrato: si alguien baja el umbral por debajo de
        // 0.2 el grafo se llena de aristas espurias y la visualización se
        // vuelve un hairball ilegible. Si alguien lo sube por encima de 0.5
        // casi nada conecta. Estos valores fueron calibrados manualmente.
        assert!(TAG_EDGE_THRESHOLD_DEFAULT >= 0.20 && TAG_EDGE_THRESHOLD_DEFAULT <= 0.50,
                "tag threshold {} fuera del rango defensivo [0.20, 0.50]", TAG_EDGE_THRESHOLD_DEFAULT);
        assert!(CONTENT_EDGE_THRESHOLD_DEFAULT >= 0.20 && CONTENT_EDGE_THRESHOLD_DEFAULT <= 0.50,
                "content threshold {} fuera del rango defensivo [0.20, 0.50]", CONTENT_EDGE_THRESHOLD_DEFAULT);
        assert!(EMBEDDING_EDGE_THRESHOLD_DEFAULT >= 0.50 && EMBEDDING_EDGE_THRESHOLD_DEFAULT <= 0.90,
                "embedding threshold {} fuera del rango defensivo [0.50, 0.90]", EMBEDDING_EDGE_THRESHOLD_DEFAULT);
    }

    // Sprint Memory Graph 2.0 — cosine similarity contract tests.
    // Required since the embedding-edge feature depends on this helper
    // returning sane values even under edge cases.

    #[test]
    fn cosine_identical_vectors_is_one() {
        let v = vec![0.3, 0.5, 0.2, 0.1];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 0.0001);
    }

    #[test]
    fn cosine_orthogonal_vectors_is_zero() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &b)).abs() < 0.0001);
    }

    #[test]
    fn cosine_handles_zero_vectors_without_nan() {
        // Pure-zero embedding would happen if a backend bug ever sent an
        // empty vec. We must NOT propagate NaN — that would crash JSON
        // serialization downstream and break the whole graph.
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        let s = cosine_similarity(&a, &b);
        assert!(s.is_finite(), "cosine must never produce NaN/Inf");
        assert_eq!(s, 0.0);
    }

    #[test]
    fn cosine_handles_length_mismatch() {
        // The embeddings table records `dims` per row; if two rows have
        // different dims (e.g. model upgrade mid-history), we must NOT
        // compare them — return 0 so no edge is emitted.
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn graph_caps_are_renderable() {
        // 250 nodos × 250 nodos = 62500 comparaciones — barato.
        // 1500 aristas serializadas a JSON ≈ 90 KB — manejable por IPC tauri.
        // Si alguien sube estos números sin actualizar la UI, el frontend
        // se cuelga. Pinear aquí.
        assert!(GRAPH_MAX_NODES <= 500, "Nodos > 500 hace que el force-layout JS se cuelgue");
        assert!(GRAPH_MAX_EDGES <= 5000, "Edges > 5000 produce un JSON > 300KB por IPC");
    }
}

#[cfg(test)]
mod consolidation_tests {
    use super::{jaccard, tokens_for_similarity, decode_embedding_blob};
    use std::collections::HashSet;

    #[test]
    fn jaccard_identical_sets_equals_one() {
        let a: HashSet<String> = ["foo", "bar"].iter().map(|s| s.to_string()).collect();
        let b = a.clone();
        assert!((jaccard(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn jaccard_disjoint_sets_equals_zero() {
        let a: HashSet<String> = ["foo"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["bar"].iter().map(|s| s.to_string()).collect();
        assert!(jaccard(&a, &b) < 1e-6);
    }

    #[test]
    fn jaccard_empty_sets_returns_zero_not_nan() {
        let a: HashSet<String> = HashSet::new();
        let b: HashSet<String> = HashSet::new();
        let result = jaccard(&a, &b);
        assert!(result == 0.0 && result.is_finite());
    }

    #[test]
    fn tokens_filter_short_words() {
        let s = "El servidor de produccion corre IIS y nginx";
        let t = tokens_for_similarity(s);
        // 'el', 'de', 'y' should be filtered (len < 3)
        assert!(!t.contains("el"));
        assert!(!t.contains("de"));
        assert!(!t.contains("y"));
        // Real words should survive
        assert!(t.contains("servidor"));
        assert!(t.contains("produccion"));
        assert!(t.contains("iis"));   // lowercased
        assert!(t.contains("nginx"));
    }

    #[test]
    fn near_duplicate_memories_cluster_above_threshold() {
        // Near-duplicate phrasings (the case consolidation is meant to catch):
        // same domain words ("server", "runs", "IIS") in slight variations.
        let a = tokens_for_similarity("the production server runs IIS web server software");
        let b = tokens_for_similarity("production server is running IIS as its web server");
        let score = jaccard(&a, &b);
        // Threshold is 0.35 in the consolidator. Sentences this close should
        // exceed it — if they don't, consolidation is too strict to be useful.
        assert!(score >= 0.35,
            "Near-duplicate sentences must cluster (≥0.35); got {} — \
             MIN_CONTENT_JACCARD may need lowering or the tokenizer is too aggressive", score);
    }

    #[test]
    fn unrelated_memories_score_below_threshold() {
        let a = tokens_for_similarity("production server runs IIS");
        let b = tokens_for_similarity("MSSQL database backup schedule weekly");
        let score = jaccard(&a, &b);
        assert!(score < 0.35, "Unrelated memories should NOT cluster, got {}", score);
    }

    // ── decode_embedding_blob: dims-bounds BEFORE the *4 multiply (v1.7.222) ──

    #[test]
    fn decode_blob_valid_roundtrips() {
        let v = vec![1.0_f32, -2.5, 3.25];
        let blob: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
        let out = decode_embedding_blob(&blob, 3).expect("valid blob decodes");
        assert_eq!(out, v);
    }

    #[test]
    fn decode_blob_rejects_length_mismatch() {
        // 3 floats of data but dims says 4 → reject (no partial decode).
        let blob: Vec<u8> = [1.0_f32, 2.0, 3.0].iter().flat_map(|f| f.to_le_bytes()).collect();
        assert!(decode_embedding_blob(&blob, 4).is_none());
    }

    #[test]
    fn decode_blob_rejects_nonpositive_dims() {
        assert!(decode_embedding_blob(&[], 0).is_none());
        assert!(decode_embedding_blob(&[1, 2, 3, 4], -1).is_none());
    }

    #[test]
    fn decode_blob_rejects_oversize_dims_without_overflow_panic() {
        // The regression: a corrupt `dims` near i64::MAX must be rejected by the
        // bounds check BEFORE `(dims as usize) * 4` runs — otherwise that multiply
        // overflows usize and panics (overflow-checks=true → panic=abort = crash).
        assert!(decode_embedding_blob(&[1, 2, 3, 4], i64::MAX).is_none());
        assert!(decode_embedding_blob(&[1, 2, 3, 4], 4097).is_none());
    }
}
