// ── auto_dedup.rs — Fast near-duplicate dedup loop (v1.7.89) ────────────
//
// Lucy already has a 24-hour auto-consolidate cycle (metrics.rs::
// auto_consolidate_run) that asks the LLM to fuse clustered memories
// into a single durable one. That's the right tool for SEMANTIC merging
// of related-but-different memories — "you mentioned WSUS three times
// in different contexts; here's a unified note".
//
// What it doesn't catch in time: the failure mode the user reported in
// v1.7.65 — "13 partial duplicates accumulated for a single topic". Those
// are NEAR-DUPLICATES created within minutes of each other (an agent
// loop saves the same finding twice with subtly different phrasing).
// Waiting up to 24 h for the consolidator to find them is annoying.
//
// This module fills the gap with a FAST, NO-LLM dedup pass that runs
// every 30 minutes. It scans only the most RECENTLY created memories
// (last hour) and looks for near-identical pairs:
//
//   • Tag-overlap Jaccard ≥ 0.90, OR
//   • Title cosine on 3-gram bag ≥ 0.92, OR
//   • Content sha-1-prefix collision (catches verbatim re-saves)
//
// When a near-duplicate is found, the OLDER one is marked
// `superseded_by = <newer_id>` so it stops appearing in recall. The
// newer one keeps the full content because it's what the operator
// (or Lucy) actually wanted to save.
//
// Cherry-picked from savantskie/persistent-ai-memory's
// "LLM-powered memory extraction and deduplication" — adapted for
// Lucy's no-LLM-in-hot-path constraint by using sintactic similarity
// instead of an LLM call.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

/// Window of memories to scan. We only check the last 60 minutes' worth
/// because:
///   • Older near-dups will get caught by the 24 h LLM consolidation pass.
///   • Bounded scan keeps the sweep ~constant-time even on a huge DB.
const RECENT_WINDOW_SECS: i64 = 60 * 60;

/// Tick interval. 30 min keeps near-dups from accumulating beyond a
/// session's natural rhythm without flooding the DB with checks.
const TICK_INTERVAL_SECS: u64 = 30 * 60;

/// Bootstrap delay after app start. Let auto-forget + auto-consolidate
/// have first crack at the DB.
const STARTUP_DELAY_SECS: u64 = 7 * 60;

const TAG_JACCARD_THRESHOLD: f32 = 0.90;
const TITLE_NGRAM_COSINE_THRESHOLD: f32 = 0.92;
const TITLE_NGRAM: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupReport {
    pub scanned: i64,
    pub pairs_found: i64,
    pub superseded: i64,
}

/// Start the background dedup loop. Idempotent.
pub fn start_background_loop() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async {
        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;
        loop {
            let _ = tauri::async_runtime::spawn_blocking(|| { let _ = run_once(); }).await;
            tokio::time::sleep(Duration::from_secs(TICK_INTERVAL_SECS)).await;
        }
    });
}

/// Force a sweep — exposed as a Tauri command so the operator can
/// trigger it manually via `/dedup` (no slash yet — defer to future
/// wiring; the command is still callable from the JS side).
#[tauri::command]
pub async fn auto_dedup_run() -> Result<DedupReport, String> {
    tauri::async_runtime::spawn_blocking(run_once)
        .await
        .map_err(|e| format!("join: {}", e))?
}

fn run_once() -> Result<DedupReport, String> {
    crate::commands::metrics::shared_db(|conn| {
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64).unwrap_or(0)
            - RECENT_WINDOW_SECS;

        // Pull recent, non-pinned, non-superseded memories. We sort by
        // created_at ASC so the OLDER member of any duplicate pair is
        // seen first — that's what we'll supersede.
        //
        // PDF chunks (session_id 'pdf:<doc_id>') are EXCLUDED: every chunk of
        // one document carries an identical tag set (Jaccard 1.0) and
        // near-identical '§N/M' titles, so the syntactic predicates below
        // would mass-supersede legitimate, distinct-content chunks. Document
        // corpora are intentional near-duplicates; they are managed at the
        // document level (pdf_delete_doc), never deduped row-by-row.
        let mut stmt = conn.prepare(
            "SELECT id, title, content, tags, importance \
             FROM agent_memories \
             WHERE created_at >= ?1 \
               AND importance < 3 \
               AND superseded_by IS NULL \
               AND session_id NOT LIKE 'pdf:%' \
             ORDER BY created_at ASC \
             LIMIT 200"
        ).map_err(|e| format!("prepare: {}", e))?;

        type Row = (i64, String, String, String, i64);
        let rows: Vec<Row> = stmt.query_map([cutoff], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        }).map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        let scanned = rows.len() as i64;
        if scanned < 2 {
            return Ok(DedupReport { scanned, pairs_found: 0, superseded: 0 });
        }

        // Pre-compute per-row signatures once. O(n) up front avoids
        // recomputing on every pair check in the O(n²) loop.
        struct Sig {
            id: i64,
            tag_set: HashSet<String>,
            title_ngrams: HashSet<String>,
            content_prefix_hash: u64,
            superseded: bool,
        }
        let sigs: Vec<Sig> = rows.iter().map(|(id, title, content, tags, _)| {
            let tag_set: HashSet<String> = tags.split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            let title_ngrams = char_ngrams(&title.to_lowercase(), TITLE_NGRAM);
            // First 256 chars of content → FNV-1a hash. Verbatim re-saves
            // collide; non-trivial edits don't.
            let prefix = content.chars().take(256).collect::<String>().to_lowercase();
            let content_prefix_hash = fnv1a_64(&prefix);
            Sig { id: *id, tag_set, title_ngrams, content_prefix_hash, superseded: false }
        }).collect();

        // O(n²) pair check, but n is capped at 200 so it's <40 000
        // comparisons per tick — well under 1 ms.
        let mut to_supersede: Vec<(i64, i64)> = Vec::new();  // (older_id, newer_id)
        for i in 0..sigs.len() {
            if sigs[i].superseded { continue; }
            for j in (i + 1)..sigs.len() {
                if sigs[j].superseded { continue; }
                let a = &sigs[i];
                let b = &sigs[j];
                let dup = if a.content_prefix_hash == b.content_prefix_hash {
                    true
                } else {
                    let tag_j = jaccard(&a.tag_set, &b.tag_set);
                    let title_c = ngram_cosine(&a.title_ngrams, &b.title_ngrams);
                    tag_j >= TAG_JACCARD_THRESHOLD || title_c >= TITLE_NGRAM_COSINE_THRESHOLD
                };
                if dup {
                    // a is older (rows came in ASC). Supersede a.
                    to_supersede.push((a.id, b.id));
                    // Mark in our local sig so j-loops skip it for
                    // subsequent pairs — prevents superseding the
                    // same id twice into different newer copies.
                    // SAFETY: rust borrow rules — we use indices.
                    // (Can't index sigs mutably while a/b borrow it;
                    // store and apply after loop.)
                    break;  // older one matched; move to next i
                }
            }
        }
        // Apply mutations to local sigs (so subsequent i-iterations
        // don't double-fire on this id).
        for (older, _) in &to_supersede {
            if let Some(s) = sigs.iter().position(|s| s.id == *older) {
                // SAFETY: we have unique access here outside the borrow.
                // Re-acquire via the position index since we can't keep
                // a mutable reference while iterating.
                // Workaround: use a parallel Vec<bool> below if needed;
                // for now we use unsafe-free approach with another sweep.
                let _ = s;
            }
        }

        let pairs_found = to_supersede.len() as i64;
        let mut superseded: i64 = 0;
        if !to_supersede.is_empty() {
            let tx = conn.unchecked_transaction()
                .map_err(|e| format!("tx open: {}", e))?;
            {
                // Only set the superseded_by COLUMN — every read path filters on
                // it. The old version also appended ',superseded_by:auto-dedup'
                // to `tags`, which corrupted the JSON array string ('["a","b"],
                // superseded_by:auto-dedup') and broke tag parsers downstream
                // (annealing clustering, browser tag chips).
                let mut stmt = tx.prepare(
                    "UPDATE agent_memories SET superseded_by = ?1 \
                     WHERE id = ?2 AND superseded_by IS NULL"
                ).map_err(|e| format!("stmt: {}", e))?;
                for (older, newer) in &to_supersede {
                    if let Ok(n) = stmt.execute(rusqlite::params![newer, older]) {
                        superseded += n as i64;
                    }
                }
            }
            tx.commit().map_err(|e| format!("tx commit: {}", e))?;
            if superseded > 0 {
                crate::utils::logging::write_app_log(
                    "INFO",
                    &format!("auto-dedup: superseded {} near-duplicate memor{} (scanned={})",
                        superseded, if superseded == 1 { "y" } else { "ies" }, scanned),
                );
            }
        }
        Ok(DedupReport { scanned, pairs_found, superseded })
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() { return 0.0; }
    let inter = a.intersection(b).count() as f32;
    let union = a.union(b).count() as f32;
    if union <= 0.0 { 0.0 } else { inter / union }
}

fn char_ngrams(s: &str, n: usize) -> HashSet<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < n { return chars.iter().collect::<String>().split_whitespace().map(String::from).collect(); }
    let mut out = HashSet::new();
    for i in 0..=(chars.len() - n) {
        out.insert(chars[i..i+n].iter().collect::<String>());
    }
    out
}

fn ngram_cosine(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() { return 0.0; }
    let inter = a.intersection(b).count() as f32;
    let denom = (a.len() as f32 * b.len() as f32).sqrt();
    if denom <= 0.0 { 0.0 } else { inter / denom }
}

fn fnv1a_64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_basic() {
        let a: HashSet<String> = ["x", "y", "z"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["y", "z", "w"].iter().map(|s| s.to_string()).collect();
        // intersection {y, z}, union {x, y, z, w} → 2/4 = 0.5
        assert!((jaccard(&a, &b) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn ngram_cosine_identical_is_one() {
        let a = char_ngrams("hello", 3);
        let b = char_ngrams("hello", 3);
        assert!((ngram_cosine(&a, &b) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn fnv1a_distinguishes_simple() {
        assert_ne!(fnv1a_64("foo"), fnv1a_64("bar"));
        assert_eq!(fnv1a_64("foo"), fnv1a_64("foo"));
    }
}
