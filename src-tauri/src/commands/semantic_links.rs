// ── semantic_links.rs — Typed memory relationships (v1.7.87) ────────────
//
// Inspired by the `memory-graph/memory-graph` MCP server's relationship
// taxonomy. Lucy's existing graph edges encode SIMILARITY (by tag, content,
// or embedding) — three flavours of "these two memories overlap textually."
// That makes a good visualization but it can't represent REASONING:
//   "A solved B"
//   "B contradicts the assumption in A"
//   "B is a refinement of A"
//   "A caused B"
//
// This module adds a SECOND edge type alongside the similarity edges: an
// explicit, typed semantic link the operator (or Lucy herself) creates to
// encode that a memory has a SPECIFIC operational meaning relative to
// another memory.
//
// Design choices
// ──────────────
//   • Separate table, not a column on the existing similarity edges.
//     Similarity edges are recomputed from scratch on graph load (their
//     "truth" lives in the embeddings). Semantic links are persistent
//     facts the operator authored — they need their own storage.
//   • Six relationship kinds chosen from the most operationally
//     useful ones in the memory-graph taxonomy. The list is closed
//     (rusty enum) so the graph render can colour them consistently.
//   • Bidirectional vs directed: directed (source → target). All six
//     kinds have an arrow direction operators care about.
//   • Confidence on each link [0.0, 1.0]. Defaults to 1.0 for
//     operator-authored links; lower values let Lucy add weak inferred
//     links without dominating the visualization.

use serde::{Deserialize, Serialize};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS memory_semantic_links (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id   INTEGER NOT NULL,
    target_id   INTEGER NOT NULL,
    kind        TEXT    NOT NULL,
    confidence  REAL    NOT NULL DEFAULT 1.0,
    note        TEXT,
    created_at  INTEGER NOT NULL,
    UNIQUE(source_id, target_id, kind)
);
CREATE INDEX IF NOT EXISTS idx_semlinks_source ON memory_semantic_links(source_id);
CREATE INDEX IF NOT EXISTS idx_semlinks_target ON memory_semantic_links(target_id);
"#;

/// The six relationship kinds. Closed enum so the renderer can colour
/// them consistently and the slash command can validate input.
const ALLOWED_KINDS: &[&str] = &[
    "causal",        // A caused B   (A → B)
    "resolves",      // A solved B   (A → B); arrow points to the problem
    "derives_from",  // A was built on B  (A → B)
    "references",    // A points at B (A → B)
    "contradicts",   // A contradicts B (A ↔ B but arrow shows who flagged it)
    "refines",       // A is a refined version of B (A → B)
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticLink {
    pub id: i64,
    pub source_id: i64,
    pub target_id: i64,
    pub kind: String,
    pub confidence: f64,
    pub note: Option<String>,
    pub created_at: i64,
}

fn _ensure(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute_batch(SCHEMA).map_err(|e| format!("schema: {}", e))
}

fn _now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Create a typed link between two memories. Idempotent on (source, target, kind).
#[tauri::command]
pub fn memory_link_add(
    source_id: i64,
    target_id: i64,
    kind: String,
    confidence: Option<f64>,
    note: Option<String>,
) -> Result<i64, String> {
    let kind_norm = kind.to_lowercase();
    if !ALLOWED_KINDS.contains(&kind_norm.as_str()) {
        return Err(format!(
            "Unknown link kind '{}'. Allowed: {}",
            kind, ALLOWED_KINDS.join(", ")
        ));
    }
    if source_id == target_id {
        return Err("source_id and target_id must differ".into());
    }
    let confidence = confidence.unwrap_or(1.0).clamp(0.0, 1.0);
    crate::commands::metrics::shared_db(|conn| {
        _ensure(conn)?;
        // Verify both memories exist — silent integrity check.
        let exists = |id: i64| -> bool {
            conn.query_row(
                "SELECT 1 FROM agent_memories WHERE id = ?1",
                [id],
                |_| Ok(()),
            ).is_ok()
        };
        if !exists(source_id) {
            return Err(format!("source memory id {} not found", source_id));
        }
        if !exists(target_id) {
            return Err(format!("target memory id {} not found", target_id));
        }
        conn.execute(
            "INSERT INTO memory_semantic_links (source_id, target_id, kind, confidence, note, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(source_id, target_id, kind) DO UPDATE SET \
               confidence = excluded.confidence, \
               note = excluded.note, \
               created_at = excluded.created_at",
            rusqlite::params![source_id, target_id, kind_norm, confidence, note, _now()],
        ).map_err(|e| format!("insert: {}", e))?;
        Ok(conn.last_insert_rowid())
    })
}

/// List all semantic links currently stored. Frontend renders them as a
/// second edge layer in the graph.
#[tauri::command]
pub fn memory_link_list() -> Result<Vec<SemanticLink>, String> {
    crate::commands::metrics::shared_db(|conn| {
        _ensure(conn)?;
        let mut stmt = conn.prepare(
            "SELECT id, source_id, target_id, kind, confidence, note, created_at \
             FROM memory_semantic_links ORDER BY created_at DESC"
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt.query_map([], |r| {
            Ok(SemanticLink {
                id:         r.get(0)?,
                source_id:  r.get(1)?,
                target_id:  r.get(2)?,
                kind:       r.get(3)?,
                confidence: r.get(4)?,
                note:       r.get(5)?,
                created_at: r.get(6)?,
            })
        }).map_err(|e| format!("query: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| format!("row: {}", e))?);
        }
        Ok(out)
    })
}

/// Remove a link by id. Returns true if a row was deleted.
#[tauri::command]
pub fn memory_link_remove(id: i64) -> Result<bool, String> {
    crate::commands::metrics::shared_db(|conn| {
        _ensure(conn)?;
        let n = conn.execute(
            "DELETE FROM memory_semantic_links WHERE id = ?1", [id]
        ).map_err(|e| format!("delete: {}", e))?;
        Ok(n > 0)
    })
}

/// Returns the closed list of allowed kinds so the frontend can render a
/// picker / dropdown without hard-coding the taxonomy.
#[tauri::command]
pub fn memory_link_kinds() -> Vec<String> {
    ALLOWED_KINDS.iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_kinds_is_closed() {
        // If we ever expand the taxonomy, the frontend palette must be
        // expanded too. This test prevents silent drift.
        assert_eq!(ALLOWED_KINDS.len(), 6);
        for k in ALLOWED_KINDS {
            assert!(k.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "kind '{}' must be lowercase snake_case", k);
        }
    }
}
