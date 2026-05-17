// ── Semantic embeddings via Ollama + in-memory cosine search ──────────────
//
// Sprint 2: Replace pure FTS5/TF-IDF matching with semantic similarity. We
// call Ollama's /api/embeddings locally (no cost, no cloud) to turn skills,
// runbooks and memories into vectors, store them as BLOBs in SQLite, and
// compute cosine similarity in Rust on search.
//
// Why not sqlite-vec?  It would be faster once corpus > 10k vectors, but it
// adds a C dependency + build-system changes. For Lucy's scale (dozens of
// skills, hundreds of runbooks/memories), a linear scan on a Vec<f32> is
// <5 ms and zero build-time pain. We can swap it later without touching the
// call sites since this module owns the storage schema.

use crate::commands::metrics::shared_db;
use crate::commands::vec_index;
use crate::state::HTTP_CLIENT;
use crate::utils::db::generate_id;
use keyring::Entry;
use rusqlite::params;
use serde::{Deserialize, Serialize};

const DEFAULT_EMBED_MODEL: &str = "nomic-embed-text";

// ── Types exposed to the frontend ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticHit {
    pub entity_type: String,
    pub entity_id: String,
    pub text: String,
    pub score: f32,
}

// ── Internal helpers ───────────────────────────────────────────────────────

/// Resolve the Ollama base URL from the keyring-stored endpoint.
/// Falls back to http://localhost:11434 if not configured.
pub(crate) fn ollama_base() -> String {
    if let Ok(entry) = Entry::new("LucySysAdmin", "local_api_key") {
        if let Ok(stored) = entry.get_password() {
            return stored
                .trim_end_matches('/')
                .trim_end_matches("/v1/chat/completions")
                .trim_end_matches("/api/chat")
                .trim_end_matches("/v1")
                .trim_end_matches("/api")
                .trim_end_matches('/')
                .to_string();
        }
    }
    "http://localhost:11434".to_string()
}

/// Convert a Vec<f32> into the little-endian byte representation we store
/// in the `embeddings.vec` BLOB column.
fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// Inverse of vec_to_blob.
fn blob_to_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Cosine similarity. Expects equal-length vectors; returns 0.0 if either
/// is zero-magnitude (avoids NaN).
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut na = 0.0_f32;
    let mut nb = 0.0_f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = (na.sqrt()) * (nb.sqrt());
    if denom == 0.0 { 0.0 } else { dot / denom }
}

/// Call Ollama's /api/embeddings endpoint for a single text.
/// Returns (vec, model_used).
async fn embed_via_ollama(text: &str, model: Option<String>) -> Result<(Vec<f32>, String), String> {
    let m = model.unwrap_or_else(|| DEFAULT_EMBED_MODEL.to_string());
    let base = ollama_base();
    let url = format!("{}/api/embeddings", base);

    let body = serde_json::json!({ "model": m, "prompt": text });
    let resp = HTTP_CLIENT
        .post(&url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Ollama embeddings request failed ({}): {}. Tip: run `ollama pull {}` first.", url, e, m))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text_body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Ollama returned {} for /api/embeddings. Response: {}. Tip: `ollama pull {}`.",
            status, text_body, m
        ));
    }

    let json: serde_json::Value = resp.json().await
        .map_err(|e| format!("Ollama embeddings returned invalid JSON: {}", e))?;
    let arr = json["embedding"].as_array()
        .ok_or("Ollama response missing 'embedding' field")?;
    let v: Vec<f32> = arr.iter()
        .filter_map(|x| x.as_f64().map(|f| f as f32))
        .collect();
    if v.is_empty() {
        return Err("Ollama returned an empty embedding vector".to_string());
    }
    Ok((v, m))
}

// ── Internal helper (used by pdf.rs and other sibling modules) ────────────

/// Compute and upsert an embedding without the text-dedup check.
/// Skips silently if Ollama is unavailable — embeddings are best-effort.
pub(crate) async fn embed_and_store(
    entity_type: String,
    entity_id: String,
    text: String,
    model: Option<String>,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let (v, used_model) = embed_via_ollama(&text, model).await?;
    let dims = v.len() as i64;
    let blob = vec_to_blob(&v);
    let id = generate_id();
    shared_db(|conn| {
        conn.execute(
            "INSERT INTO embeddings (id, entity_type, entity_id, text, vec, dims, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(entity_type, entity_id) DO UPDATE SET
               text  = excluded.text,
               vec   = excluded.vec,
               dims  = excluded.dims,
               model = excluded.model,
               created_at = strftime('%s','now')",
            params![id, entity_type, entity_id, text, blob, dims, used_model],
        ).map_err(|e| format!("embed_and_store: {}", e))?;
        Ok(())
    })
}

// ── Tauri commands ─────────────────────────────────────────────────────────

/// Compute an embedding for arbitrary text and return the raw vector.
/// Useful for frontend search queries; storage is the caller's responsibility.
#[tauri::command]
pub async fn embed_text(text: String, model: Option<String>) -> Result<Vec<f32>, String> {
    let (v, _) = embed_via_ollama(&text, model).await?;
    Ok(v)
}

/// Crate-visible wrapper around the private `embed_via_ollama` so sibling
/// modules (e.g. `metrics::save_agent_memory` Stage 2 dedup) can embed
/// content without going through the Tauri command boundary. Returns
/// (vector, actual_model_used).
pub(crate) async fn embed_via_ollama_pub(text: &str, model: Option<String>) -> Result<(Vec<f32>, String), String> {
    embed_via_ollama(text, model).await
}

/// Check whether the embeddings system is available (Ollama reachable + model
/// installed). Runs a 1-token embed as smoke test. Returns `true` on success.
#[tauri::command]
pub async fn embeddings_available(model: Option<String>) -> Result<bool, String> {
    match embed_via_ollama("ok", model).await {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Upsert an embedding row. If a row with (entity_type, entity_id) already
/// exists AND the text hasn't changed, this is a no-op (we skip the network
/// call). Otherwise we call Ollama and replace the row.
#[tauri::command]
pub async fn upsert_embedding(
    entity_type: String,
    entity_id: String,
    text: String,
    model: Option<String>,
) -> Result<(), String> {
    // Fast path: if the text is identical we can skip the whole embed round-trip.
    let existing: Option<String> = shared_db(|conn| {
        Ok(conn.query_row(
            "SELECT text FROM embeddings WHERE entity_type = ?1 AND entity_id = ?2",
            params![&entity_type, &entity_id],
            |r| r.get::<_, String>(0),
        ).ok())
    }).unwrap_or(None);

    if let Some(prev) = existing {
        if prev == text {
            return Ok(());
        }
    }

    let (v, used_model) = embed_via_ollama(&text, model).await?;
    let dims = v.len() as i64;
    let blob = vec_to_blob(&v);
    let id = generate_id();

    shared_db(|conn| {
        conn.execute(
            "INSERT INTO embeddings (id, entity_type, entity_id, text, vec, dims, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(entity_type, entity_id) DO UPDATE SET
               text  = excluded.text,
               vec   = excluded.vec,
               dims  = excluded.dims,
               model = excluded.model,
               created_at = strftime('%s','now')",
            params![id, entity_type, entity_id, text, blob, dims, used_model],
        ).map_err(|e| format!("Failed to upsert embedding: {}", e))?;
        Ok(())
    })?;
    // Invalidate HNSW index so it rebuilds on next unfiltered search
    vec_index::invalidate();
    Ok(())
}

/// Delete embeddings for an entity. Called when the underlying skill/memory
/// is deleted so we don't leak orphan vectors.
#[tauri::command]
pub async fn delete_embedding(entity_type: String, entity_id: String) -> Result<(), String> {
    shared_db(|conn| {
        conn.execute(
            "DELETE FROM embeddings WHERE entity_type = ?1 AND entity_id = ?2",
            params![entity_type, entity_id],
        ).map_err(|e| format!("Failed to delete embedding: {}", e))?;
        Ok(())
    })?;
    // Invalidate HNSW index
    vec_index::invalidate();
    Ok(())
}

/// Semantic search. Embeds `query` and returns up to `limit` hits ranked by
/// cosine similarity, optionally filtered by entity_type. Results below
/// `min_score` (default 0.25) are discarded to avoid garbage matches.
///
/// Uses HNSW index (vec_index) when corpus > 500 rows for O(log n) search.
/// Falls back to linear scan for smaller corpora or filtered queries.
#[tauri::command]
pub async fn semantic_search(
    query: String,
    entity_type: Option<String>,
    limit: Option<u32>,
    min_score: Option<f32>,
    model: Option<String>,
) -> Result<Vec<SemanticHit>, String> {
    let (qvec, _) = embed_via_ollama(&query, model).await?;
    let limit = limit.unwrap_or(5).max(1) as usize;
    let min_score = min_score.unwrap_or(0.25);

    // ── Fast path: use HNSW index if available and no type filter ────────
    // The index doesn't support per-type filtering (it indexes ALL vectors).
    // For filtered queries or when index is stale, fall through to linear scan.
    if entity_type.is_none() && vec_index::is_ready() {
        let results = vec_index::search(&qvec, limit, min_score);
        if !results.is_empty() {
            let hits: Vec<SemanticHit> = results.into_iter()
                .map(|(et, eid, text, score)| SemanticHit { entity_type: et, entity_id: eid, text, score })
                .collect();
            return Ok(hits);
        }
    }

    // ── Linear scan: pull all candidate rows from SQLite ─────────────────
    let rows: Vec<(String, String, String, Vec<u8>, i64)> = shared_db(|conn| {
        let sql = if entity_type.is_some() {
            "SELECT entity_type, entity_id, text, vec, dims
             FROM embeddings WHERE entity_type = ?1"
        } else {
            "SELECT entity_type, entity_id, text, vec, dims FROM embeddings"
        };
        let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare: {}", e))?;
        let mapper = |r: &rusqlite::Row| -> rusqlite::Result<(String, String, String, Vec<u8>, i64)> {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        };
        let iter: Vec<_> = if let Some(et) = &entity_type {
            stmt.query_map(params![et], mapper)
                .map_err(|e| format!("query: {}", e))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map([], mapper)
                .map_err(|e| format!("query: {}", e))?
                .filter_map(|r| r.ok())
                .collect()
        };
        Ok(iter)
    })?;

    // ── Opportunistic index build: if corpus crossed threshold, populate index ──
    if entity_type.is_none() && rows.len() >= vec_index::INDEX_THRESHOLD && !vec_index::is_ready() {
        let entries: Vec<vec_index::IndexEntry> = rows.iter()
            .map(|(et, eid, text, blob, _)| vec_index::IndexEntry {
                entity_type: et.clone(),
                entity_id: eid.clone(),
                text: text.clone(),
                vec: blob_to_vec(blob),
            })
            .collect();
        // Build in background — this search still uses linear scan
        std::thread::spawn(move || vec_index::reload(entries));
    }

    // Score every row, partial-sort by cosine desc, filter by threshold.
    let mut scored: Vec<SemanticHit> = rows.into_iter()
        .filter_map(|(et, eid, text, blob, _dims)| {
            let v = blob_to_vec(&blob);
            let s = cosine(&qvec, &v);
            if s >= min_score {
                Some(SemanticHit { entity_type: et, entity_id: eid, text, score: s })
            } else { None }
        })
        .collect();

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    Ok(scored)
}

/// Backfill: iterate over a table (skills or agent_memories) and ensure every
/// row has a corresponding embedding. Idempotent; safe to call on startup.
/// Returns the number of NEW embeddings created.
#[tauri::command]
pub async fn backfill_embeddings(
    entity_type: String,
    model: Option<String>,
) -> Result<u32, String> {
    // 1. Pull every (id, text-to-embed) pair from the source table.
    let pairs: Vec<(String, String)> = shared_db(|conn| {
        let sql = match entity_type.as_str() {
            "skill" => "SELECT id, (name || ' — ' || COALESCE(description,'') || ' — ' || COALESCE(triggers,'')) FROM skills WHERE enabled = 1",
            "memory" => "SELECT CAST(id AS TEXT), (title || ' — ' || content) FROM agent_memories",
            other => return Err(format!("Unknown entity_type '{}' for backfill", other)),
        };
        let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare: {}", e))?;
        let rows: Vec<_> = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })?;

    // 2. Pull existing entity_ids so we can skip already-embedded rows.
    let existing_ids: std::collections::HashSet<String> = shared_db(|conn| {
        let mut stmt = conn.prepare("SELECT entity_id FROM embeddings WHERE entity_type = ?1")
            .map_err(|e| format!("prepare: {}", e))?;
        let rows: Vec<String> = stmt.query_map(params![&entity_type], |r| r.get::<_, String>(0))
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows.into_iter().collect())
    })?;

    // 3. For each missing row, call Ollama + insert. We do this sequentially
    //    on purpose — Ollama's embedding endpoint is single-model single-GPU
    //    on most user setups; parallel requests just queue internally.
    let mut new_count = 0u32;
    for (id, text) in pairs {
        if existing_ids.contains(&id) || text.trim().is_empty() {
            continue;
        }
        match embed_via_ollama(&text, model.clone()).await {
            Ok((v, used_model)) => {
                let blob = vec_to_blob(&v);
                let dims = v.len() as i64;
                let row_id = generate_id();
                let _ = shared_db(|conn| {
                    conn.execute(
                        "INSERT INTO embeddings (id, entity_type, entity_id, text, vec, dims, model)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                         ON CONFLICT(entity_type, entity_id) DO NOTHING",
                        params![row_id, entity_type, id, text, blob, dims, used_model],
                    ).map_err(|e| format!("insert: {}", e))?;
                    Ok(())
                });
                new_count += 1;
            }
            Err(e) => {
                eprintln!("[embeddings] backfill skip {} / {}: {}", entity_type, id, e);
                // Stop early if Ollama is down — no point hammering it for the
                // rest of the corpus on the same failure.
                return Err(format!("Backfill aborted after {} embeddings: {}", new_count, e));
            }
        }
    }
    Ok(new_count)
}
