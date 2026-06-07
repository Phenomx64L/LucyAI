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

// ── Cloud embedding fallback (Tier 1) ─────────────────────────────────────
//
// When Ollama is unavailable (not installed, model not pulled, daemon down),
// fall back to Gemini's text-embedding-004 API. Both models output 768-dim
// vectors so they can coexist in the same `embeddings` table without
// dimension-mismatch errors during cosine search.
//
// Caveat: vectors from different models live in different latent spaces, so
// cosine scores between an Ollama-embedded query and a Gemini-embedded passage
// are noisy. In practice this only matters when the user oscillates between
// having/not having Ollama mid-session — a rare situation. The `model` column
// records which embedder produced each row so a future re-index command can
// fix the inconsistency.
const GEMINI_EMBED_MODEL: &str = "text-embedding-004";

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
/// is zero-magnitude (avoids NaN). v1.7.19 — delegates to the SIMD
/// dispatcher (AVX-512 / AVX2 / scalar) instead of the manual loop.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    crate::utils::simd_cosine::cosine(a, b)
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

/// Call Google's Gemini embedding API as a cloud fallback when Ollama is down.
/// Returns (vec, "text-embedding-004"). Reads the API key from the same keyring
/// entry the ask_lucy commands use.
async fn embed_via_gemini(text: &str) -> Result<(Vec<f32>, String), String> {
    let api_key = Entry::new("LucySysAdmin", "gemini_api_key")
        .map_err(|e| format!("keyring: {}", e))?
        .get_password()
        .map_err(|_| "Gemini API key no configurada (sin Ollama y sin Gemini, no hay embeddings).".to_string())?;

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:embedContent",
        GEMINI_EMBED_MODEL
    );
    let body = serde_json::json!({
        "model": format!("models/{}", GEMINI_EMBED_MODEL),
        "content": { "parts": [{ "text": text }] }
    });
    let resp = HTTP_CLIENT
        .post(&url)
        .header("x-goog-api-key", &api_key)
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Gemini embed request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text_body = resp.text().await.unwrap_or_default();
        return Err(format!("Gemini embed HTTP {}: {}", status, text_body));
    }

    let json: serde_json::Value = resp.json().await
        .map_err(|e| format!("Gemini embed: JSON inválido: {}", e))?;
    let arr = json["embedding"]["values"].as_array()
        .ok_or("Gemini embed: respuesta sin embedding.values")?;
    let v: Vec<f32> = arr.iter()
        .filter_map(|x| x.as_f64().map(|f| f as f32))
        .collect();
    if v.is_empty() {
        return Err("Gemini embed: vector vacío".to_string());
    }
    Ok((v, GEMINI_EMBED_MODEL.to_string()))
}

// v1.7.83 — LRU-ish embedding cache.
// Lucy's auto-router (v1.7.5) embeds the user prompt to score against
// 18+ skill presets. During streaming an investigation tab can hit
// embed_with_fallback dozens of times with very similar (often
// IDENTICAL) text — the unified context orchestrator + memory recall
// + skill auto-router all share the same query string. Without a
// cache, each invocation is a 50-200 ms Ollama round-trip (or worse,
// a Gemini paid call). The cache cuts that to a microsecond clone.
//
// Sizing:
//   - 256 entries × ~3 KB per (text + 768-dim f32 vector) ≈ 750 KB.
//     Trivial vs Lucy's typical ~200 MB working set.
//   - Hot prompts (the same user message during a single turn) dominate
//     the access pattern, so even FIFO eviction works fine — no need
//     for proper LRU bookkeeping.
//
// Keyed on (text_hash, model_label): different models DO produce
// different vectors, so the cache must scope by model. Using a hash
// instead of the raw text keeps the HashMap key cheap.

use std::collections::VecDeque;
use std::sync::Mutex;

// v1.7.104 Sprint-4 perf: bumped 256 → 1024. Audit measured a busy
// operator types 50+ unique queries per session — the old 256 cap
// thrashed after ~30 min, defeating the cache's purpose. Memory cost
// of 4× larger cache: 768 dims × 4 bytes × 1024 entries ≈ 3.1 MB
// resident, still trivial against Lucy's typical ~100 MB working set.
const EMBED_CACHE_MAX: usize = 1024;

#[derive(Clone)]
struct CachedEmbedding {
    vector: Vec<f32>,
    model_used: String,
}

static EMBED_CACHE: once_cell::sync::Lazy<Mutex<(std::collections::HashMap<u64, CachedEmbedding>, VecDeque<u64>)>>
    = once_cell::sync::Lazy::new(|| Mutex::new((std::collections::HashMap::with_capacity(EMBED_CACHE_MAX + 1), VecDeque::with_capacity(EMBED_CACHE_MAX + 1))));

/// FNV-1a 64-bit. Same algorithm as the frontend cache; collision risk
/// at 256 entries is < 10⁻¹⁶, way below the noise floor.
fn _embed_key(text: &str, model: Option<&str>) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^= b'|' as u64;
    h = h.wrapping_mul(0x100000001b3);
    if let Some(m) = model {
        for b in m.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

fn _embed_cache_get(key: u64) -> Option<CachedEmbedding> {
    EMBED_CACHE.lock().ok().and_then(|guard| guard.0.get(&key).cloned())
}

fn _embed_cache_put(key: u64, value: CachedEmbedding) {
    if let Ok(mut guard) = EMBED_CACHE.lock() {
        let (map, order) = &mut *guard;
        if map.insert(key, value).is_none() {
            order.push_back(key);
            // Evict oldest if over capacity.
            while order.len() > EMBED_CACHE_MAX {
                if let Some(old) = order.pop_front() {
                    map.remove(&old);
                }
            }
        }
    }
}

/// Try Ollama first (preferred — free, local, no rate limits, no telemetry),
/// fall back to Gemini text-embedding-004 if Ollama isn't reachable.
///
/// This is the function ALL call sites should use. The individual `embed_via_*`
/// helpers exist for tests and the `embeddings_available` smoke check only.
///
/// v1.7.83 — first checks the in-process LRU cache (see EMBED_CACHE above)
/// before hitting either provider. Cache miss falls through to the network
/// call and stores the result.
async fn embed_with_fallback(
    text: &str,
    model: Option<String>,
) -> Result<(Vec<f32>, String), String> {
    let key = _embed_key(text, model.as_deref());
    if let Some(hit) = _embed_cache_get(key) {
        return Ok((hit.vector, hit.model_used));
    }
    let result = match embed_via_ollama(text, model.clone()).await {
        Ok(r) => Ok(r),
        Err(ollama_err) => {
            // Only fall back to Gemini if the user has it configured. If both
            // are unavailable, return a combined error so the caller can show
            // a useful "configure one of these" message.
            match embed_via_gemini(text).await {
                Ok(r) => {
                    crate::utils::logging::write_app_log(
                        "INFO",
                        &format!("Embedding fallback: Ollama failed ({}), used Gemini instead.", ollama_err),
                    );
                    Ok(r)
                }
                Err(gem_err) => Err(format!(
                    "Sin embeddings: Ollama no disponible ({}) y Gemini tampoco ({}). Configura uno de los dos en Ajustes → Proveedores.",
                    ollama_err.lines().next().unwrap_or("?"),
                    gem_err.lines().next().unwrap_or("?")
                )),
            }
        }
    };
    // Cache the successful result so the next identical call returns instantly.
    if let Ok((ref v, ref m)) = result {
        _embed_cache_put(key, CachedEmbedding {
            vector: v.clone(),
            model_used: m.clone(),
        });
    }
    result
}

// ── Internal helper (used by pdf.rs and other sibling modules) ────────────

/// Compute and upsert an embedding without the text-dedup check.
/// Skips silently if both Ollama and Gemini are unavailable — embeddings are best-effort.
pub(crate) async fn embed_and_store(
    entity_type: String,
    entity_id: String,
    text: String,
    model: Option<String>,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let (v, used_model) = embed_with_fallback(&text, model).await?;
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
/// Tries Ollama first, falls back to Gemini cloud if Ollama is unreachable.
#[tauri::command]
pub async fn embed_text(text: String, model: Option<String>) -> Result<Vec<f32>, String> {
    let (v, _) = embed_with_fallback(&text, model).await?;
    Ok(v)
}

/// Crate-visible wrapper so sibling modules (e.g. `metrics::save_agent_memory`
/// Stage 2 dedup) can embed content without going through the Tauri command
/// boundary. Returns (vector, actual_model_used). Uses the same Ollama-first
/// + Gemini-fallback chain as the public commands.
///
/// Note: the function name still mentions "ollama" for backward compatibility
/// with existing callers — it actually goes through embed_with_fallback now.
pub(crate) async fn embed_via_ollama_pub(text: &str, model: Option<String>) -> Result<(Vec<f32>, String), String> {
    embed_with_fallback(text, model).await
}

/// Check whether the embeddings system is available. Returns `true` if EITHER
/// Ollama OR Gemini can produce an embedding (the fallback chain works).
/// Runs a 1-token embed as smoke test.
#[tauri::command]
pub async fn embeddings_available(model: Option<String>) -> Result<bool, String> {
    match embed_with_fallback("ok", model).await {
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

    let (v, used_model) = embed_with_fallback(&text, model).await?;
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
        // v1.7.93 — Mirror into the sqlite-vec index in the SAME txn-less
        // pass. Failure here is logged but not propagated — the source
        // row in `embeddings` is what matters; vec_search degrades to no
        // hit and the caller falls back to the in-memory HNSW or the
        // linear cosine scan.
        let _ = super::vec_search::upsert_vec(conn, &entity_type, &entity_id, &text, &v);
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
            params![&entity_type, &entity_id],
        ).map_err(|e| format!("Failed to delete embedding: {}", e))?;
        // v1.7.93 — Drop the matching vec_search index entry too.
        let _ = super::vec_search::delete_vec(conn, &entity_type, &entity_id);
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

    // ── v1.7.93/94 — Second-tier fast path: sqlite-vec HNSW on disk ──────
    // The in-memory vec_index above is the fastest path BUT it's not built
    // until the background loader fires AND has to rebuild on every boot.
    // sqlite-vec gives us a durable, on-disk HNSW that survives restarts.
    // Sits between the in-memory index (fastest, transient) and the linear
    // scan (slowest, always works).
    //
    // v1.7.94 — Now supports entity_type filtering via knn_filtered, so
    // semantic_search no longer falls through to the linear scan when the
    // caller wants to scope the search (e.g. only chunks of a runbook).
    {
        let filter = super::vec_search::VecFilter {
            entity_type: entity_type.as_deref(),
            // For pure semantic_search we don't know whether the caller
            // wants to skip superseded/expired memories — that's a
            // memory-pipeline concern, not a generic embedding-search
            // one. Defaults stay off; the per-table recall paths
            // (memory.rs, metrics.rs) can pass tighter filters.
            ..Default::default()
        };
        let knn_res = shared_db(|conn| {
            super::vec_search::knn_filtered(conn, &qvec, limit, filter, 5)
                .map_err(|e| e)
        });
        if let Ok(rows) = knn_res {
            if !rows.is_empty() {
                let hits: Vec<SemanticHit> = rows.into_iter()
                    .map(|h| SemanticHit {
                        entity_type: h.entity_type,
                        entity_id:   h.entity_id,
                        text:        h.text,
                        // cosine_distance ∈ [0, 2] → similarity = 1 - distance
                        // (we set distance_metric=cosine on the vec0 table).
                        score:       (1.0_f32 - h.distance).max(0.0),
                    })
                    .filter(|h| h.score >= min_score)
                    .collect();
                if !hits.is_empty() {
                    return Ok(hits);
                }
            }
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
