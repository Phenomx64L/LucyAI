// ── graph_layout_cache.rs — Memory Graph positions cache (v1.7.85) ───────
//
// Saves the (x, y) layout produced by d3-force in MemoryGraphView so the
// next time the operator opens the graph, nodes load already-positioned
// instead of going through the 300-tick pre-warm.
//
// Why it matters
// ──────────────
// Opening the graph today (v1.7.72) runs the full d3-force pre-warm:
// 300 sequential ticks × ~3 ms each = ~900 ms of stutter before the
// first paint. For a 250-node graph that's the worst single source of
// "Lucy feels slow" complaints after the streaming pipeline.
//
// Strategy
// ────────
//   • One row per (node_id) — replace on conflict.
//   • Frontend calls `graph_layout_save_bulk` when the user closes the
//     overlay AFTER fitToView has settled, OR when alpha drops below
//     alphaMin. Single bulk write inside one transaction → 1 fsync
//     for the whole graph.
//   • On next open, frontend calls `graph_layout_load` to get any
//     cached positions, then seeds simNodes with them. If a node has
//     no entry (newly created since the cache was written) it falls
//     back to the community-seeded position the existing code computes.
//   • Cache is best-effort: rows expire after RETENTION_DAYS and are
//     pruned on each load. If the cache is empty or stale, the graph
//     behaves exactly as today.
//
// The schema is unconditional — calling `graph_layout_load` always
// returns rows (possibly empty) so the frontend never branches on
// "cache available?" — it just merges whatever it gets.

use serde::{Deserialize, Serialize};

const RETENTION_DAYS: i64 = 30;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS memory_graph_layout (
    node_id    INTEGER PRIMARY KEY,
    x          REAL NOT NULL,
    y          REAL NOT NULL,
    pinned     INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
);
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphLayoutEntry {
    pub node_id: i64,
    pub x: f64,
    pub y: f64,
    pub pinned: i64,
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

/// Pull every cached layout entry that hasn't expired. Frontend calls
/// this once on graph open and merges the results into simNodes.
#[tauri::command]
pub fn graph_layout_load() -> Result<Vec<GraphLayoutEntry>, String> {
    crate::commands::metrics::shared_db(|conn| {
        _ensure(conn)?;
        let cutoff = _now() - RETENTION_DAYS * 86400;
        // Prune stale rows lazily so the table doesn't grow unbounded.
        let _ = conn.execute(
            "DELETE FROM memory_graph_layout WHERE updated_at < ?1",
            [cutoff],
        );
        let mut stmt = conn.prepare(
            "SELECT node_id, x, y, pinned FROM memory_graph_layout"
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt.query_map([], |r| {
            Ok(GraphLayoutEntry {
                node_id: r.get(0)?,
                x:       r.get(1)?,
                y:       r.get(2)?,
                pinned:  r.get(3)?,
            })
        }).map_err(|e| format!("query: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| format!("row: {}", e))?);
        }
        Ok(out)
    })
}

/// Replace the cached layout in one transaction (one fsync regardless
/// of how many nodes the graph has). Called when the operator closes
/// the overlay or the sim hits alphaMin.
#[tauri::command]
pub fn graph_layout_save_bulk(entries: Vec<GraphLayoutEntry>) -> Result<(), String> {
    if entries.is_empty() { return Ok(()); }
    crate::commands::metrics::shared_db(|conn| {
        _ensure(conn)?;
        let tx = conn.unchecked_transaction()
            .map_err(|e| format!("tx open: {}", e))?;
        let now = _now();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO memory_graph_layout (node_id, x, y, pinned, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(node_id) DO UPDATE SET \
                   x = excluded.x, \
                   y = excluded.y, \
                   pinned = excluded.pinned, \
                   updated_at = excluded.updated_at"
            ).map_err(|e| format!("prepare: {}", e))?;
            for e in &entries {
                // NaN/Inf guard — d3-force has been known to produce them
                // on degenerate edge cases. Skip silently rather than
                // commit garbage that would crash the next reload.
                if !e.x.is_finite() || !e.y.is_finite() { continue; }
                let _ = stmt.execute(rusqlite::params![
                    e.node_id, e.x, e.y, e.pinned, now
                ]);
            }
        }
        tx.commit().map_err(|e| format!("tx commit: {}", e))?;
        Ok(())
    })
}

/// Drop the entire cache. Exposed for operator-triggered reset (e.g. via
/// a slash command in a future build) and for tests.
#[tauri::command]
pub fn graph_layout_clear() -> Result<(), String> {
    crate::commands::metrics::shared_db(|conn| {
        _ensure(conn)?;
        conn.execute("DELETE FROM memory_graph_layout", [])
            .map(|_| ())
            .map_err(|e| format!("clear: {}", e))
    })
}
