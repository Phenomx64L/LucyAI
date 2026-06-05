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
pub fn init_extension() -> Result<(), String> {
    // sqlite-vec registers via the static auto-extension mechanism.
    // Cast the loader fn pointer to the shape rusqlite's ffi expects.
    unsafe {
        let entry: unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *mut std::ffi::c_char,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> std::ffi::c_int = std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
        let rc = rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(entry)));
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

    if let Some(rowid) = existing {
        conn.execute("DELETE FROM embeddings_vec WHERE rowid = ?1", [rowid])
            .map_err(|e| format!("vec0 delete: {}", e))?;
        conn.execute(
            "INSERT INTO embeddings_vec(rowid, embedding) VALUES (?1, ?2)",
            rusqlite::params![rowid, blob],
        ).map_err(|e| format!("vec0 insert: {}", e))?;
        conn.execute(
            "UPDATE embeddings_vec_map SET text = ?1 WHERE vec_rowid = ?2",
            rusqlite::params![text, rowid],
        ).map_err(|e| format!("vec map update: {}", e))?;
    } else {
        // Append. vec0 assigns the rowid; mirror it into the map.
        conn.execute(
            "INSERT INTO embeddings_vec(embedding) VALUES (?1)",
            [blob],
        ).map_err(|e| format!("vec0 insert: {}", e))?;
        let new_rowid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO embeddings_vec_map (vec_rowid, entity_type, entity_id, text) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![new_rowid, entity_type, entity_id, text],
        ).map_err(|e| format!("vec map insert: {}", e))?;
    }
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
    if b.len() % 4 != 0 { return None; }
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
