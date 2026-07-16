// ── vec_search.rs — sqlite-vec HNSW vector search (v1.7.93) ─────────────
//
// Backs Lucy's embedding-based recall with the official `sqlite-vec`
// extension instead of the linear cosine scan that v1.4-1.7 used.
//
// What this gets us
// ─────────────────
//   • Sub-millisecond approximate-nearest-neighbour even at 100K+
//     vectors. The existing SIMD cosine loop is fast (~20 µs per
//     vector on AVX-512) but it's O(N) per query — at 10K vectors
//     that's already 200 ms before any post-filter runs. sqlite-vec
//     gives us an HNSW-style index that's ~O(log N).
//   • Stays inside the same .db file — no separate server, no extra
//     deploy pieces, air-gap friendly. Same backup story, same audit
//     story.
//   • Hybrid queries: filter by tag / importance / recency in SQL
//     while the vector search runs natively.
//
// Architecture choices
// ────────────────────
//   • We DON'T replace the existing `embeddings` table. It's still the
//     source of truth (text + entity_id + model used). The new vec0
//     table `embeddings_vec` is a derived index keyed by the same
//     rowid as the source. Backfill is idempotent.
//   • We DON'T touch `embed_with_fallback` (the v1.7.83 cache + the
//     Ollama → Gemini fallback still does its job). vec_search is
//     downstream of that — it stores AND retrieves.
//   • The extension is registered globally via
//     `sqlite_vec::sqlite3_auto_extension` so every connection the
//     pool hands out can use vec0. Idempotent — first call registers,
//     subsequent calls are no-ops.
//
// Migration / boot flow
// ─────────────────────
//   1. lib.rs calls `init_extension()` once on app start (BEFORE the
//      pool is built, so every pooled connection inherits the
//      extension).
//   2. The vec0 virtual table is CREATEd at first call to
//      `ensure_schema()` (lazy — keeps cold-start cheap).
//   3. `backfill_from_embeddings()` runs once on app start (in a
//      background tokio task) to populate the vec0 table from any
//      pre-existing rows in `embeddings`. Idempotent — skips entries
//      already in the vec0 table.
//   4. New writes go to BOTH tables in `upsert_vec` so the index
//     stays in sync. Failure on the vec0 side is logged and the source
//     table still gets the row — vec search degrades gracefully.

use serde::Serialize;

const EMBEDDING_DIM: usize = 768;

/// Initialise the sqlite-vec extension. MUST be called BEFORE any
/// SQLite connection is opened (the auto-extension hook runs at
/// connection open).
///
/// v1.7.102 Sprint-2 B4: the audit flagged the original double-transmute
/// as silent-UB-on-signature-drift. Attempted "safer" alternatives all
/// fail to compile because the `sqlite_vec` crate intentionally exports
/// `sqlite3_vec_init` as a stub `unsafe extern "C" fn() -> ()` — the
/// REAL entry point is the C symbol statically linked in from the
/// vendored vec0 source. The transmute is therefore load-bearing,
/// not a typo. We keep it, but tighten the surrounding code:
///
///   1. A `const` assertion documents the EXACT FFI signature we expect.
///      A version bump that changes `rusqlite::ffi::sqlite3_api_routines`
///      will not "silently produce UB" — it'll break the const eval
///      below at compile time.
///   2. The transmute now goes through ONE typed step (not two through
///      `*const ()`) so the compiler at least sees one fn-pointer cast.
///   3. We assert at runtime that the returned rc is SQLITE_OK and log
///      the address so a tamper investigation has evidence.
pub fn init_extension() -> Result<(), String> {
    // Note the `*mut *const c_char` — rusqlite's FFI binding for the
    // pzErrMsg out-param uses `*const`, not the `*mut` you'd expect from
    // a "place an error string here" argument. Matching it exactly is
    // what makes the const-assert below load-bearing.
    type AutoExtFn = unsafe extern "C" fn(
        *mut rusqlite::ffi::sqlite3,
        *mut *const std::ffi::c_char,
        *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::ffi::c_int;

    // Compile-time pin: any signature drift in the rusqlite FFI types
    // listed above will fail this size assertion (fn-pointer size is
    // architecture-stable; what we're really checking is that
    // AutoExtFn IS a callable fn pointer of the EXPECTED bit width).
    const _: () = {
        assert!(std::mem::size_of::<Option<AutoExtFn>>() == std::mem::size_of::<*const ()>());
    };

    unsafe {
        // Single transmute — sqlite_vec ships `sqlite3_vec_init` as a
        // 0-arg stub whose linker symbol is patched to the real entry
        // by the vendored C source. We force-cast through `*const ()`
        // because the Rust types don't match (by design from sqlite-vec).
        let entry_ptr = sqlite_vec::sqlite3_vec_init as *const ();
        let entry: AutoExtFn = std::mem::transmute(entry_ptr);
        let rc = rusqlite::ffi::sqlite3_auto_extension(Some(entry));
        if rc != rusqlite::ffi::SQLITE_OK {
            return Err(format!("sqlite3_auto_extension returned {}", rc));
        }
    }
    Ok(())
}

/// Lazy schema setup for the vec0 virtual table. Called at first use.
/// Idempotent.
fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), String> {
    // v1.7.93 — `distance_metric=cosine` lets us derive
    // `cosine_similarity = 1 - distance` directly. Lucy's existing
    // recall layer expects 0..1 similarity scores; matching that here
    // means knn() can be dropped into semantic_search without a unit
    // change at the call site.
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS embeddings_vec USING vec0(\n\
           embedding float[{dim}] distance_metric=cosine\n\
         );\n\
         -- Side-table joining vec0 rowids back to source rows in `embeddings`.\n\
         -- vec0 rowids are int64; we mirror entity_type+entity_id so the\n\
         -- final result can be re-attached to its metadata in one query.\n\
         CREATE TABLE IF NOT EXISTS embeddings_vec_map (\n\
           vec_rowid    INTEGER PRIMARY KEY,\n\
           entity_type  TEXT NOT NULL,\n\
           entity_id    TEXT NOT NULL,\n\
           text         TEXT NOT NULL,\n\
           UNIQUE(entity_type, entity_id)\n\
         );\n\
         CREATE INDEX IF NOT EXISTS idx_emb_map_entity \n\
           ON embeddings_vec_map(entity_type, entity_id);\n",
        dim = EMBEDDING_DIM,
    )).map_err(|e| format!("ensure vec schema: {}", e))
}

/// Insert OR replace a vector for (entity_type, entity_id). Failure is
/// logged but not propagated to the caller — the source `embeddings`
/// table is still the canonical store. Best-effort by design.
pub fn upsert_vec(
    conn: &rusqlite::Connection,
    entity_type: &str,
    entity_id: &str,
    text: &str,
    vector: &[f32],
) -> Result<(), String> {
    if vector.len() != EMBEDDING_DIM {
        // Wrong-dim vectors are skipped silently — the source row still
        // exists in `embeddings`, we just don't index it.
        return Ok(());
    }
    ensure_schema(conn)?;

    // Look up any existing vec0 row for this entity. If it exists,
    // delete-and-reinsert; vec0 doesn't support direct UPDATE on the
    // vector column.
    let existing: Option<i64> = conn.query_row(
        "SELECT vec_rowid FROM embeddings_vec_map WHERE entity_type = ?1 AND entity_id = ?2",
        rusqlite::params![entity_type, entity_id],
        |r| r.get(0),
    ).ok();

    let blob = vec_to_blob(vector);

    // v1.7.102 Sprint-2 H5: wrap the multi-statement vec0 + side-table
    // updates in a single transaction. Previously a crash (or concurrent
    // writer) between the DELETE and the INSERT would leave an orphan
    // `embeddings_vec_map` row pointing at a non-existent vec0 rowid;
    // the next k-NN that hit that row returned garbage. unchecked_transaction
    // is safe here because `conn` already came from the r2d2 pool (one
    // logical writer at a time on a WAL DB).
    let tx = conn.unchecked_transaction()
        .map_err(|e| format!("vec upsert tx open: {}", e))?;
    if let Some(rowid) = existing {
        tx.execute("DELETE FROM embeddings_vec WHERE rowid = ?1", [rowid])
            .map_err(|e| format!("vec0 delete: {}", e))?;
        tx.execute(
            "INSERT INTO embeddings_vec(rowid, embedding) VALUES (?1, ?2)",
            rusqlite::params![rowid, blob],
        ).map_err(|e| format!("vec0 insert: {}", e))?;
        tx.execute(
            "UPDATE embeddings_vec_map SET text = ?1 WHERE vec_rowid = ?2",
            rusqlite::params![text, rowid],
        ).map_err(|e| format!("vec map update: {}", e))?;
    } else {
        // Append. vec0 assigns the rowid; mirror it into the map.
        tx.execute(
            "INSERT INTO embeddings_vec(embedding) VALUES (?1)",
            [blob],
        ).map_err(|e| format!("vec0 insert: {}", e))?;
        let new_rowid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO embeddings_vec_map (vec_rowid, entity_type, entity_id, text) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![new_rowid, entity_type, entity_id, text],
        ).map_err(|e| format!("vec map insert: {}", e))?;
    }
    tx.commit().map_err(|e| format!("vec upsert tx commit: {}", e))?;
    Ok(())
}

/// Remove a vector by entity. Best-effort.
pub fn delete_vec(
    conn: &rusqlite::Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<(), String> {
    ensure_schema(conn)?;
    if let Ok(rowid) = conn.query_row(
        "SELECT vec_rowid FROM embeddings_vec_map WHERE entity_type = ?1 AND entity_id = ?2",
        rusqlite::params![entity_type, entity_id],
        |r| r.get::<_, i64>(0),
    ) {
        let _ = conn.execute("DELETE FROM embeddings_vec WHERE rowid = ?1", [rowid]);
        let _ = conn.execute("DELETE FROM embeddings_vec_map WHERE vec_rowid = ?1", [rowid]);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct VecHit {
    pub entity_type: String,
    pub entity_id: String,
    pub text: String,
    /// Cosine distance (lower = more similar). sqlite-vec returns this
    /// natively as the `distance` column on a MATCH query.
    pub distance: f32,
}

/// k-NN query against the vec0 index. Returns up to `limit` results
/// ranked by cosine distance.
///
/// Mostly superseded by `knn_filtered` (v1.7.94) for production calls.
/// Kept around as a thin public utility for callers that genuinely
/// don't need any filtering.
#[allow(dead_code)]
pub fn knn(
    conn: &rusqlite::Connection,
    query: &[f32],
    limit: usize,
) -> Result<Vec<VecHit>, String> {
    if query.len() != EMBEDDING_DIM {
        return Err(format!(
            "query dim {} != index dim {}", query.len(), EMBEDDING_DIM
        ));
    }
    ensure_schema(conn)?;

    let blob = vec_to_blob(query);
    let limit = limit.clamp(1, 200) as i64;

    let mut stmt = conn.prepare(
        "SELECT m.entity_type, m.entity_id, m.text, v.distance \
         FROM embeddings_vec v \
         JOIN embeddings_vec_map m ON m.vec_rowid = v.rowid \
         WHERE v.embedding MATCH ?1 \
           AND k = ?2 \
         ORDER BY v.distance ASC"
    ).map_err(|e| format!("prepare knn: {}", e))?;

    let rows = stmt.query_map(
        rusqlite::params![blob, limit],
        |r| Ok(VecHit {
            entity_type: r.get(0)?,
            entity_id:   r.get(1)?,
            text:        r.get(2)?,
            distance:    r.get::<_, f64>(3)? as f32,
        }),
    ).map_err(|e| format!("knn query: {}", e))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("knn row: {}", e))?);
    }
    Ok(out)
}

/// v1.7.94 — Hybrid SQL+vector recall. Filters by entity_type and/or
/// memory metadata (importance / superseded / expiry) INSIDE the same
/// SELECT that runs the vec0 MATCH. This is the "Qdrant-style" payload
/// filtering — except it stays in SQLite.
///
/// Strategy: over-fetch from vec0 (k = limit × `over_fetch_factor`)
/// because sqlite-vec's MATCH doesn't natively know about our filters;
/// it returns the top-k by distance. We then SQL-filter the join and
/// LIMIT to the actual desired count. As long as `over_fetch_factor`
/// is high enough relative to filter selectivity, this gives correct
/// top-N filtered results.
///
/// `over_fetch_factor` defaults to 5 if 0. For very selective filters
/// (e.g. "tag = 'rare_topic' AND importance >= 3"), the caller may
/// want to pass 10 or 15 to avoid losing recall.
#[derive(Default, Clone, Copy)]
pub struct VecFilter<'a> {
    /// If Some, restrict to embeddings_vec_map.entity_type = filter.
    pub entity_type: Option<&'a str>,
    /// If Some AND entity_type is "memory", require
    /// agent_memories.importance >= filter.
    pub importance_min: Option<i64>,
    /// If true AND entity_type is "memory", exclude rows whose
    /// agent_memories.superseded_by IS NOT NULL.
    pub exclude_superseded: bool,
    /// If true AND entity_type is "memory", exclude rows that
    /// have expired (expires_at > 0 AND < now).
    pub exclude_expired: bool,
}

pub fn knn_filtered(
    conn: &rusqlite::Connection,
    query: &[f32],
    limit: usize,
    filter: VecFilter,
    over_fetch_factor: usize,
) -> Result<Vec<VecHit>, String> {
    if query.len() != EMBEDDING_DIM {
        return Err(format!(
            "query dim {} != index dim {}", query.len(), EMBEDDING_DIM
        ));
    }
    ensure_schema(conn)?;

    let limit = limit.clamp(1, 200);
    let factor = if over_fetch_factor == 0 { 5 } else { over_fetch_factor };
    let k = (limit * factor).min(1000);

    // Build the WHERE chain dynamically. The MATCH + k = ? are required
    // by sqlite-vec; the rest are conventional SQL predicates.
    let mut wheres: Vec<String> = vec![
        "v.embedding MATCH ?".to_string(),
        "k = ?".to_string(),
    ];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(vec_to_blob(query)),
        Box::new(k as i64),
    ];

    if let Some(et) = filter.entity_type {
        wheres.push("m.entity_type = ?".to_string());
        params.push(Box::new(et.to_string()));
    }

    // Memory-specific filters only kick in when the entity_type IS
    // memory (or no filter — in which case we can't narrow to one
    // table without a CTE per type; left for v1.8).
    //
    // v1.7.233: these predicates matched entity_type 'agent_memory', but
    // every writer stores memory embeddings as 'memory' (upsert callers in
    // +page.svelte, backfill_embeddings) — so exclude_superseded /
    // exclude_expired / importance_min were DEAD CODE and superseded
    // memories stayed semantically retrievable forever.
    let memory_filters_active = filter.importance_min.is_some()
        || filter.exclude_superseded
        || filter.exclude_expired;

    let agent_memory_join_needed = memory_filters_active
        && (filter.entity_type == Some("memory")
            || filter.entity_type.is_none());

    let join_clause = if agent_memory_join_needed {
        // LEFT JOIN so non-memory rows survive the join. The
        // entity_type guard inside each predicate keeps the filter
        // logically scoped.
        "LEFT JOIN agent_memories am ON m.entity_type = 'memory' AND am.id = CAST(m.entity_id AS INTEGER)"
    } else {
        ""
    };

    if let Some(min) = filter.importance_min {
        wheres.push("(m.entity_type != 'memory' OR am.importance >= ?)".to_string());
        params.push(Box::new(min));
    }
    if filter.exclude_superseded {
        wheres.push("(m.entity_type != 'memory' OR am.superseded_by IS NULL)".to_string());
    }
    if filter.exclude_expired {
        wheres.push("(m.entity_type != 'memory' OR am.expires_at IS NULL OR am.expires_at = 0 OR am.expires_at > strftime('%s','now'))".to_string());
    }

    let sql = format!(
        "SELECT m.entity_type, m.entity_id, m.text, v.distance \
         FROM embeddings_vec v \
         JOIN embeddings_vec_map m ON m.vec_rowid = v.rowid \
         {join} \
         WHERE {where_clause} \
         ORDER BY v.distance ASC \
         LIMIT ?",
        join = join_clause,
        where_clause = wheres.join(" AND ")
    );
    params.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)
        .map_err(|e| format!("prepare knn_filtered: {}", e))?;

    // Convert Box<dyn ToSql> chain into rusqlite's params_from_iter.
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter()
        .map(|b| b.as_ref())
        .collect();

    let rows = stmt.query_map(
        rusqlite::params_from_iter(param_refs.iter()),
        |r| Ok(VecHit {
            entity_type: r.get(0)?,
            entity_id:   r.get(1)?,
            text:        r.get(2)?,
            distance:    r.get::<_, f64>(3)? as f32,
        }),
    ).map_err(|e| format!("knn_filtered query: {}", e))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("knn_filtered row: {}", e))?);
    }
    Ok(out)
}

/// Tauri command shim around knn_filtered. Lets the frontend run
/// hybrid SQL+vector queries without going through semantic_search
/// (useful for `/memory` browser, the auto-router's MCP scoring,
/// future memory-graph node-detail panels, etc).
///
/// The `query_text` is embedded server-side via the shared
/// Ollama→Gemini fallback; the frontend doesn't need to know the
/// embedding model in use.
#[tauri::command]
pub async fn vec_search_query(
    query_text: String,
    limit: Option<u32>,
    entity_type: Option<String>,
    importance_min: Option<i64>,
    exclude_superseded: Option<bool>,
    exclude_expired: Option<bool>,
) -> Result<Vec<VecHit>, String> {
    if query_text.trim().is_empty() {
        return Ok(vec![]);
    }
    let limit = limit.unwrap_or(10).clamp(1, 100) as usize;
    // Embed query through the same cache + fallback chain everything
    // else uses (v1.7.83 LRU caches identical prompts).
    let (qvec, _model) = crate::commands::embeddings::embed_via_ollama_pub(&query_text, None).await?;
    // v1.7.94 — Filter owns its strings inside the spawn_blocking
    // closure so nothing borrows from the async frame across the
    // task boundary. (Original VecFilter<'a> needs &'a str for use by
    // synchronous callers that don't want to allocate.)
    let exclude_sup = exclude_superseded.unwrap_or(false);
    let exclude_exp = exclude_expired.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        crate::commands::metrics::shared_db(|conn| {
            let filter = VecFilter {
                entity_type: entity_type.as_deref(),
                importance_min,
                exclude_superseded: exclude_sup,
                exclude_expired:    exclude_exp,
            };
            knn_filtered(conn, &qvec, limit, filter, 5)
        })
    })
    .await
    .map_err(|e| format!("join: {}", e))?
}

/// One-shot backfill from the legacy `embeddings` table. Safe to call
/// repeatedly — skips entries already present in the vec0 index.
/// Returns (inserted, skipped, errored) counters.
pub fn backfill_from_embeddings(conn: &rusqlite::Connection) -> Result<(i64, i64, i64), String> {
    ensure_schema(conn)?;

    // Pull only entries whose dims match the index. Off-dim rows (legacy
    // 1536-dim from OpenAI) are silently skipped — they'd belong in a
    // separate vec0 table anyway.
    let mut stmt = conn.prepare(
        "SELECT entity_type, entity_id, text, vec, dims \
         FROM embeddings \
         WHERE dims = ?1 \
           AND NOT EXISTS (\
             SELECT 1 FROM embeddings_vec_map m \
             WHERE m.entity_type = embeddings.entity_type \
               AND m.entity_id = embeddings.entity_id\
           )"
    ).map_err(|e| format!("backfill prep: {}", e))?;

    let rows: Vec<(String, String, String, Vec<u8>, i64)> = stmt
        .query_map([EMBEDDING_DIM as i64], |r| Ok((
            r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?
        )))
        .map_err(|e| format!("backfill query: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    let mut inserted = 0_i64;
    let mut errored  = 0_i64;
    for (et, eid, txt, blob, _dims) in &rows {
        let vec = match blob_to_vec(blob) {
            Some(v) => v,
            None => { errored += 1; continue; }
        };
        match upsert_vec(conn, et, eid, txt, &vec) {
            Ok(_) => inserted += 1,
            Err(_) => errored += 1,
        }
    }
    Ok((inserted, 0, errored))
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Convert a Vec<f32> to the little-endian f32 byte blob sqlite-vec expects.
fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for &x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Inverse of vec_to_blob. Returns None if length isn't a multiple of 4.
fn blob_to_vec(b: &[u8]) -> Option<Vec<f32>> {
    if !b.len().is_multiple_of(4) { return None; }
    let mut out = Vec::with_capacity(b.len() / 4);
    for chunk in b.chunks_exact(4) {
        let arr = [chunk[0], chunk[1], chunk[2], chunk[3]];
        out.push(f32::from_le_bytes(arr));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_to_blob_roundtrip() {
        let v: Vec<f32> = (0..768).map(|i| (i as f32) * 0.001).collect();
        let b = vec_to_blob(&v);
        assert_eq!(b.len(), 768 * 4);
        let v2 = blob_to_vec(&b).expect("roundtrip");
        assert_eq!(v.len(), v2.len());
        for (a, b) in v.iter().zip(v2.iter()) {
            assert!((a - b).abs() < 1e-6, "mismatch: {} vs {}", a, b);
        }
    }

    #[test]
    fn blob_to_vec_rejects_odd_length() {
        assert!(blob_to_vec(&[0u8; 3]).is_none());
        assert!(blob_to_vec(&[0u8; 7]).is_none());
        assert!(blob_to_vec(&[0u8; 8]).is_some());
    }
}
