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
    pub section: String, // 'user_facts' | 'preferences' | 'rules' | 'environment'
    pub key: String,
    pub value: String,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
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
            Ok(CoreMemoryEntry {
                id: r.get(0)?,
                section: r.get(1)?,
                key: r.get(2)?,
                value: r.get(3)?,
                pinned: r.get::<_, i64>(4)? != 0,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
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

/// Synchronous render used by `ai::build_system_prompt` on the hot path.
/// Doing this sync avoids making build_system_prompt async and infecting the
/// whole call chain. Safe because `shared_db` uses a std::sync::Mutex.
pub fn render_core_sync() -> String {
    let rows_res: Result<Vec<CoreMemoryEntry>, String> = shared_db(|conn| {
        let mut stmt = match conn.prepare(
            "SELECT id, section, key, value, pinned, created_at, updated_at
             FROM memory_core WHERE pinned = 1 ORDER BY section, key"
        ) {
            Ok(s) => s,
            Err(_) => return Ok(Vec::new()),
        };
        let rows: Vec<CoreMemoryEntry> = stmt.query_map([], |r| {
            Ok(CoreMemoryEntry {
                id: r.get(0)?,
                section: r.get(1)?,
                key: r.get(2)?,
                value: r.get(3)?,
                pinned: r.get::<_, i64>(4)? != 0,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
            })
        }).map(|iter| iter.filter_map(|r| r.ok()).collect()).unwrap_or_default();
        Ok(rows)
    });
    let rows = rows_res.unwrap_or_default();
    if rows.is_empty() { return String::new(); }

    let mut by_section: std::collections::BTreeMap<String, Vec<&CoreMemoryEntry>> = std::collections::BTreeMap::new();
    for r in &rows {
        by_section.entry(r.section.clone()).or_default().push(r);
    }

    let mut out = String::from("--- CORE MEMORY (always-on facts) ---\n");
    for (section, items) in &by_section {
        out.push_str(&format!("[{}]\n", section));
        for it in items {
            out.push_str(&format!("  {} = {}\n", it.key, it.value));
        }
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
