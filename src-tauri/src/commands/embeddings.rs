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

/// v1.7.108 audit M1 — batched embedding via Ollama's newer /api/embed
/// endpoint (available in Ollama 0.1.40+). Takes an `input: [str, str, ...]`
/// array and returns `embeddings: [[..], [..], ...]` in the same order.
///
/// On a 50-memory backfill this is 1 HTTP round-trip instead of 50 — wallclock
/// drops from 2.5-10s to 200-500ms (the bulk of latency is per-request TCP +
/// JSON overhead, not the GPU work). The GPU forward-pass itself parallelizes
/// well within a single request because nomic-embed-text is small enough.
///
/// Returns `Err` on ANY failure (network, HTTP non-2xx, malformed JSON,
/// length mismatch) — the caller falls back to per-text sequential embed_via_ollama.
async fn embed_batch_via_ollama(
    texts: &[&str],
    model: Option<&str>,
) -> Result<(Vec<Vec<f32>>, String), String> {
    if texts.is_empty() {
        return Ok((vec![], DEFAULT_EMBED_MODEL.to_string()));
    }
    let m = model.unwrap_or(DEFAULT_EMBED_MODEL).to_string();
    let base = ollama_base();
    let url = format!("{}/api/embed", base);

    let body = serde_json::json!({ "model": m, "input": texts });
    let resp = HTTP_CLIENT
        .post(&url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("Ollama batch embed request failed ({}): {}", url, e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Ollama batch returned {} — falling back to per-text",
            resp.status()
        ));
    }

    let json: serde_json::Value = resp.json().await
        .map_err(|e| format!("Ollama batch invalid JSON: {}", e))?;
    let arr = json["embeddings"].as_array()
        .ok_or("Ollama batch response missing 'embeddings' array")?;

    // Per-row vector decode.
    let mut out: Vec<Vec<f32>> = Vec::with_capacity(arr.len());
    for row in arr {
        let inner = row.as_array()
            .ok_or("Ollama batch row is not an array")?;
        let v: Vec<f32> = inner.iter()
            .filter_map(|x| x.as_f64().map(|f| f as f32))
            .collect();
        if v.is_empty() {
            return Err("Ollama batch returned an empty vector".to_string());
        }
        out.push(v);
    }

    if out.len() != texts.len() {
        return Err(format!(
            "Ollama batch length mismatch: requested {} got {}",
            texts.len(), out.len()
        ));
    }

    Ok((out, m))
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
        // v1.7.233 — mirror into sqlite-vec like upsert_embedding does.
        // Without this, freshly ingested pdf_chunk vectors only reached the
        // durable vec0 index at the NEXT boot's backfill; a pdf_search that
        // found stale non-empty hits returned early and never saw new chunks.
        let _ = super::vec_search::upsert_vec(conn, &entity_type, &entity_id, &text, &v);
        Ok(())
    })?;
    vec_index::invalidate();
    Ok(())
}

/// v1.7.233 — Batch variant of `embed_and_store` for bulk ingestion (PDF
/// chunks). Embeds `items` = (entity_id, text) pairs 16-per-HTTP-call via
/// Ollama's /api/embed (M1); when a batch call fails (older Ollama,
/// Gemini-only setup) it falls back to the sequential per-text path so
/// ingestion still completes. Returns how many embeddings were stored.
pub(crate) async fn embed_and_store_batch(
    entity_type: &str,
    items: &[(String, String)],
    model: Option<String>,
) -> Result<u32, String> {
    const BATCH: usize = 16;
    let mut stored: u32 = 0;
    for chunk in items.chunks(BATCH) {
        let texts: Vec<&str> = chunk.iter().map(|(_, t)| t.as_str()).collect();
        match embed_batch_via_ollama(&texts, model.as_deref()).await {
            Ok((vecs, used_model)) => {
                shared_db(|conn| {
                    for ((eid, text), v) in chunk.iter().zip(vecs.iter()) {
                        let dims = v.len() as i64;
                        let blob = vec_to_blob(v);
                        let id = generate_id();
                        conn.execute(
                            "INSERT INTO embeddings (id, entity_type, entity_id, text, vec, dims, model)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                             ON CONFLICT(entity_type, entity_id) DO UPDATE SET
                               text  = excluded.text,
                               vec   = excluded.vec,
                               dims  = excluded.dims,
                               model = excluded.model,
                               created_at = strftime('%s','now')",
                            params![id, entity_type, eid, text, blob, dims, used_model],
                        ).map_err(|e| format!("embed_and_store_batch: {}", e))?;
                        let _ = super::vec_search::upsert_vec(conn, entity_type, eid, text, v);
                    }
                    Ok(())
                })?;
                stored += chunk.len() as u32;
            }
            Err(_) => {
                // Batch endpoint unavailable — per-text fallback (best-effort,
                // same semantics as the old sequential ingest loop).
                for (eid, text) in chunk {
                    if embed_and_store(entity_type.to_string(), eid.clone(), text.clone(), model.clone()).await.is_ok() {
                        stored += 1;
                    }
                }
            }
        }
    }
    vec_index::invalidate();
    Ok(stored)
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
/// Build the one-line WARN emitted when `semantic_search`'s linear scan
/// skips stored vectors whose dimension differs from the query. Returns
/// `None` when nothing was skipped. Pure, so the threshold logic is unit
/// tested without a DB. (v1.7.220)
fn dim_mismatch_warning(skipped: usize, total: usize, query_dim: usize) -> Option<String> {
    if skipped == 0 {
        return None;
    }
    Some(format!(
        "semantic_search: {}/{} stored embeddings have a dimension ≠ {} \
         (embedding model changed?). They are invisible to recall until \
         re-embedded — run backfill_embeddings.",
        skipped, total, query_dim
    ))
}

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
        // v1.7.233: memory-scoped searches (the pre-loop recall, memoria
        // tools) now exclude superseded/expired rows — the vec_search
        // predicates were dead code before (entity_type naming mismatch),
        // so rows hidden by dedup/consolidation stayed semantically
        // retrievable forever. Other entity types keep defaults.
        let is_memory = entity_type.as_deref() == Some("memory");
        let filter = super::vec_search::VecFilter {
            entity_type: entity_type.as_deref(),
            exclude_superseded: is_memory,
            exclude_expired: is_memory,
            ..Default::default()
        };
        // Over-fetch 10× for memory queries: with 1000+ pdf_chunk vectors in
        // the same vec0 table, a 5× pool can be exhausted by chunks before
        // the entity_type filter runs, silently starving memory recall.
        let over_fetch = if is_memory { 10 } else { 5 };
        let knn_res = shared_db(|conn| {
            super::vec_search::knn_filtered(conn, &qvec, limit, filter, over_fetch)
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
    // v1.7.220 — honor the stored `dims`. A vector whose dimension differs
    // from the query lives in a DIFFERENT embedding space (the user switched
    // embedding models), so cosine() silently returns 0.0 and the row would
    // vanish from recall with no signal — the documented "no usa lo
    // ingestado" failure. Skip those explicitly (also saves the blob decode
    // + cosine) and surface one diagnostic so the operator knows to re-embed.
    let qdim = qvec.len();
    let total_rows = rows.len();
    let mut dim_skips = 0usize;
    let mut scored: Vec<SemanticHit> = rows.into_iter()
        .filter_map(|(et, eid, text, blob, dims)| {
            if dims as usize != qdim {
                dim_skips += 1;
                return None;
            }
            let v = blob_to_vec(&blob);
            let s = cosine(&qvec, &v);
            if s >= min_score {
                Some(SemanticHit { entity_type: et, entity_id: eid, text, score: s })
            } else { None }
        })
        .collect();
    if let Some(msg) = dim_mismatch_warning(dim_skips, total_rows, qdim) {
        crate::utils::logging::write_app_log("WARN", &msg);
    }

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
            // v1.7.233: exclude PDF chunks — they are already embedded as
            // entity_type 'pdf_chunk' by pdf_ingest. Without this filter one
            // maintenance backfill would re-embed every chunk as 'memory',
            // letting the manual invade the pre-loop semantic recall that is
            // deliberately scoped to entityType:'memory'.
            "memory" => "SELECT CAST(id AS TEXT), (title || ' — ' || content) FROM agent_memories WHERE session_id NOT LIKE 'pdf:%'",
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

    // 3. Filter the pending list once.
    let pending: Vec<(String, String)> = pairs.into_iter()
        .filter(|(id, text)| !existing_ids.contains(id) && !text.trim().is_empty())
        .collect();

    // v1.7.108 audit M1 — batch embedding via Ollama's /api/embed.
    //
    // The old loop made one HTTP call per memory; on a 50-row backfill that
    // was 50 sequential request/response cycles. Even with the same GPU work,
    // per-request TCP + JSON overhead dominated wallclock (~50-200 ms each).
    // Batching collapses the network cost to one round-trip per BATCH_CHUNK.
    //
    // CHUNK size 16 chosen because:
    //   • Most Ollama deployments are single-GPU with ~8GB VRAM — 16 nomic
    //     texts fit comfortably in one forward pass.
    //   • Keeps payload < 100 KB even for long-text memories, so we don't
    //     trip the default 1 MB HTTP body limit some proxies impose.
    //   • Limits blast radius if the batch fails — we re-try at most 16 rows
    //     via the per-text fallback before moving on.
    //
    // Fallback: if /api/embed errors (older Ollama < 0.1.40 returns 404,
    // network blip, model not loaded), we drop back to embed_via_ollama
    // per text for THAT chunk only. The next chunk re-tries the batch path.
    const BATCH_CHUNK: usize = 16;
    let mut new_count = 0u32;

    for chunk in pending.chunks(BATCH_CHUNK) {
        let texts_ref: Vec<&str> = chunk.iter().map(|(_, t)| t.as_str()).collect();

        let batch_result = embed_batch_via_ollama(&texts_ref, model.as_deref()).await;

        match batch_result {
            Ok((vecs, used_model)) => {
                // Bulk-insert under a single tx for the chunk — fewer DB locks.
                let chunk_owned: Vec<(String, String, Vec<f32>)> = chunk.iter()
                    .zip(vecs)
                    .map(|((id, text), v)| (id.clone(), text.clone(), v))
                    .collect();
                let used_model_clone = used_model.clone();
                let entity_type_inner = entity_type.clone();
                let inserted = shared_db(move |conn| {
                    let tx = conn.unchecked_transaction().map_err(|e| format!("tx: {}", e))?;
                    let mut n = 0u32;
                    {
                        let mut stmt = tx.prepare(
                            "INSERT INTO embeddings (id, entity_type, entity_id, text, vec, dims, model)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                             ON CONFLICT(entity_type, entity_id) DO NOTHING"
                        ).map_err(|e| format!("prepare: {}", e))?;
                        for (id, text, v) in &chunk_owned {
                            let blob = vec_to_blob(v);
                            let dims = v.len() as i64;
                            let row_id = generate_id();
                            // execute() returns rows changed: 0 when ON CONFLICT
                            // DO NOTHING skips a row already present (TOCTOU vs a
                            // concurrent writer), 1 on a real insert. Count actual
                            // inserts so the returned total isn't inflated. (v1.7.220)
                            n += stmt.execute(params![row_id, entity_type_inner, id, text, blob, dims, used_model_clone])
                                .map_err(|e| format!("insert: {}", e))? as u32;
                        }
                    }
                    tx.commit().map_err(|e| format!("commit: {}", e))?;
                    Ok(n)
                })?;
                new_count += inserted;
            }
            Err(batch_err) => {
                // Per-text fallback for this chunk. Telemetry so we know how
                // often we're hitting older Ollama / batch failures.
                eprintln!("[embeddings] batch failed, per-text fallback: {}", batch_err);
                for (id, text) in chunk {
                    match embed_via_ollama(text, model.clone()).await {
                        Ok((v, used_model)) => {
                            let blob = vec_to_blob(&v);
                            let dims = v.len() as i64;
                            let row_id = generate_id();
                            // Count actual inserts (0 on conflict) and stop
                            // swallowing a DB error silently. (v1.7.220)
                            let inserted = shared_db(|conn| {
                                let changed = conn.execute(
                                    "INSERT INTO embeddings (id, entity_type, entity_id, text, vec, dims, model)
                                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                                     ON CONFLICT(entity_type, entity_id) DO NOTHING",
                                    params![row_id, &entity_type, id, text, blob, dims, used_model],
                                ).map_err(|e| format!("insert: {}", e))?;
                                Ok::<usize, String>(changed)
                            }).unwrap_or(0);
                            new_count += inserted as u32;
                        }
                        Err(e) => {
                            eprintln!("[embeddings] backfill skip {} / {}: {}", entity_type, id, e);
                            // Stop early if Ollama is down — no point hammering
                            // it for the rest of the corpus on the same failure.
                            return Err(format!("Backfill aborted after {} embeddings: {}", new_count, e));
                        }
                    }
                }
            }
        }
    }
    Ok(new_count)
}

#[cfg(test)]
mod embed_tests {
    use super::*;

    #[test]
    fn dim_warning_none_when_no_skips() {
        assert!(dim_mismatch_warning(0, 100, 768).is_none());
    }

    #[test]
    fn dim_warning_reports_counts() {
        let msg = dim_mismatch_warning(7, 100, 768).expect("should warn");
        assert!(msg.contains("7/100"), "msg = {msg}");
        assert!(msg.contains("768"), "msg = {msg}");
        assert!(msg.contains("backfill_embeddings"), "msg = {msg}");
    }

    #[test]
    fn blob_vec_roundtrip() {
        let v = vec![0.0_f32, 1.5, -2.25, 3.125];
        let blob = vec_to_blob(&v);
        assert_eq!(blob.len(), v.len() * 4);
        assert_eq!(blob_to_vec(&blob), v);
    }

    #[test]
    fn blob_to_vec_drops_partial_trailing_bytes() {
        // chunks_exact(4) ignores a non-multiple-of-4 tail instead of panicking.
        let mut blob = vec_to_blob(&[1.0_f32, 2.0]);
        blob.push(0xAB); // stray byte → not a full f32
        assert_eq!(blob_to_vec(&blob), vec![1.0, 2.0]);
    }

    #[test]
    fn embed_key_is_deterministic_and_model_sensitive() {
        let a = _embed_key("hello world", Some("nomic-embed-text"));
        let b = _embed_key("hello world", Some("nomic-embed-text"));
        let c = _embed_key("hello world", Some("text-embedding-004"));
        let d = _embed_key("hello world", None);
        assert_eq!(a, b, "same (text, model) must hash identically");
        assert_ne!(a, c, "different model must change the key");
        assert_ne!(a, d, "Some(model) vs None must differ");
    }
}
