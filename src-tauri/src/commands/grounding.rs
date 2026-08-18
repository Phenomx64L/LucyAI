// ── grounding.rs — Probabilistic truth convergence (v1.6.0) ──────────────
//
// Implements ADR-044 (Probabilistic Truth Convergence Through Contradiction
// Resolution) from the Kappa Graph research mirror in
// docs/research/kappa-graph/adrs/ADR-044-probabilistic-truth-convergence.md
//
// Core formula (ADR-044):
//     grounding_strength = support_weight / (support_weight + contradict_weight)
//
// Computed at QUERY TIME, never cached. Caching edge counts is explicitly
// forbidden by the ADR because stale values break the truth convergence
// guarantee: "Cached values become stale between writes, breaking truth
// convergence. Grounding must reflect current graph state; query overhead
// is the intentional price of correctness."
//
// Default filter threshold: 0.20 (configurable in Settings → Memory).
// Memories below threshold are NOT deleted — they remain in the graph
// for audit and re-evaluation. They simply drop out of the default
// inject-into-prompt path.
//
// Data model (new in v1.6.0):
//
//   agent_memories.confidence FLOAT DEFAULT 0.5
//     Per-row prior confidence (the LLM/user's stated belief at ingest).
//     Used as a tie-breaker when a memory has zero support/contradict
//     evidence yet.
//
//   memory_core.confidence FLOAT DEFAULT 0.5
//     Same as above, for the small core-tier facts.
//
//   memory_evidence (memory_kind, memory_id, kind, weight, source_ref, created_at)
//     One row per support/contradict signal. `weight` defaults to 1.0;
//     callers can pass higher confidence numbers (e.g. 2.0 for an explicit
//     👍 from the user, 0.4 for a low-confidence chip hit).
//
//     memory_kind: 'agent' | 'core'
//     kind:        'support' | 'contradict'
//
//   memory_instances (memory_kind, memory_id, quote_text, source_kind,
//                     source_ref, offset_start, offset_end, created_at)
//     Provenance chain per ADR-068 / ADR-051. Each row anchors a memory to
//     the literal text + source it was extracted from. FTS5 over quote_text
//     so the user can ask "where did Lucy learn X" and get verbatim hits.

use crate::commands::metrics::shared_db;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

// ── Migration ──────────────────────────────────────────────────────────────

/// Run all v1.6.0 grounding-related schema migrations idempotently.
/// Called from metrics::init() during DB pool bring-up.
///
/// Strategy: ALTER TABLE ADD COLUMN is run individually and the
/// "duplicate column" error is swallowed (SQLite < 3.35 isn't idempotent
/// for ADD COLUMN). The CREATE TABLE statements are `IF NOT EXISTS`.
pub fn migrate(conn: &rusqlite::Connection) -> Result<(), String> {
    let column_migrations = [
        "ALTER TABLE agent_memories ADD COLUMN confidence REAL NOT NULL DEFAULT 0.5",
        "ALTER TABLE memory_core    ADD COLUMN confidence REAL NOT NULL DEFAULT 0.5",
    ];
    for sql in column_migrations {
        let _ = conn.execute(sql, []); // swallow duplicate-column errors
    }

    let table_migrations = [
        // Evidence log — drives the grounding-score numerator + denominator.
        // memory_kind lets the same table back both `agent_memories` and
        // `memory_core` without polymorphic FK headaches in SQLite.
        "CREATE TABLE IF NOT EXISTS memory_evidence (\
            id           INTEGER PRIMARY KEY AUTOINCREMENT,\
            memory_kind  TEXT NOT NULL CHECK(memory_kind IN ('agent','core')),\
            memory_id    INTEGER NOT NULL,\
            kind         TEXT NOT NULL CHECK(kind IN ('support','contradict')),\
            weight       REAL NOT NULL DEFAULT 1.0,\
            source_ref   TEXT NOT NULL DEFAULT '',\
            created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now'))\
        )",
        "CREATE INDEX IF NOT EXISTS idx_memory_evidence_lookup \
            ON memory_evidence(memory_kind, memory_id, kind)",
        "CREATE INDEX IF NOT EXISTS idx_memory_evidence_created \
            ON memory_evidence(created_at DESC)",

        // Instance / provenance — links a memory back to literal source text.
        // memory_id is INTEGER for `agent` (matches agent_memories.id), but
        // memory_core uses TEXT primary keys, so we keep memory_id_text
        // for core. Either is populated, never both.
        "CREATE TABLE IF NOT EXISTS memory_instances (\
            id            INTEGER PRIMARY KEY AUTOINCREMENT,\
            memory_kind   TEXT NOT NULL CHECK(memory_kind IN ('agent','core')),\
            memory_id     INTEGER NULL,\
            memory_id_text TEXT NULL,\
            quote_text    TEXT NOT NULL,\
            source_kind   TEXT NOT NULL DEFAULT 'unknown',\
            source_ref    TEXT NOT NULL DEFAULT '',\
            offset_start  INTEGER NOT NULL DEFAULT 0,\
            offset_end    INTEGER NOT NULL DEFAULT 0,\
            created_at    INTEGER NOT NULL DEFAULT (strftime('%s','now'))\
        )",
        "CREATE INDEX IF NOT EXISTS idx_memory_instances_lookup \
            ON memory_instances(memory_kind, memory_id, memory_id_text)",

        // FTS5 over the quote text — enables \"where did Lucy learn X\"
        // queries that hit verbatim source snippets, not summaries.
        "CREATE VIRTUAL TABLE IF NOT EXISTS memory_instances_fts \
            USING fts5(quote_text, source_ref)",
        "CREATE TRIGGER IF NOT EXISTS memory_instances_ai \
            AFTER INSERT ON memory_instances BEGIN \
                INSERT INTO memory_instances_fts(rowid, quote_text, source_ref) \
                VALUES (new.id, new.quote_text, new.source_ref); \
            END",
        "CREATE TRIGGER IF NOT EXISTS memory_instances_ad \
            AFTER DELETE ON memory_instances BEGIN \
                DELETE FROM memory_instances_fts WHERE rowid = old.id; \
            END",
    ];
    for sql in table_migrations {
        conn.execute_batch(sql)
            .map_err(|e| format!("grounding migration failed: {} — {}", sql, e))?;
    }
    Ok(())
}

// ── Core scoring (ADR-044) ─────────────────────────────────────────────────

/// Result of a grounding query. Always computed at query time per ADR-044.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingScore {
    /// `support_w / (support_w + contradict_w)` — in [0.0, 1.0].
    /// Falls back to `confidence` (the row's prior) when there's no
    /// evidence yet, so a freshly-saved memory isn't auto-filtered.
    pub strength:        f32,
    pub support_weight:  f32,
    pub contradict_weight: f32,
    pub support_count:   i64,
    pub contradict_count: i64,
    /// `true` when strength came from the prior `confidence`, not from
    /// observed evidence. The UI can label these "no evidence yet".
    pub from_prior:      bool,
}

/// Compute the grounding score for a single memory. Reads evidence rows
/// fresh on every call (no caching — see ADR-044's explicit ban).
///
/// `memory_kind` must be 'agent' or 'core'. `memory_id_str` accepts either
/// the integer ID for agent_memories or the TEXT id for memory_core.
fn compute_grounding_inner(
    conn: &rusqlite::Connection,
    memory_kind: &str,
    memory_id_str: &str,
) -> Result<GroundingScore, String> {
    let mid: Option<i64> = memory_id_str.parse().ok();

    let pair = |stmt: &str, p_int: i64, p_str: &str| -> (f32, i64) {
        if memory_kind == "agent" {
            conn.query_row(stmt, params![p_int], |r| {
                Ok((r.get::<_, f32>(0)?, r.get::<_, i64>(1)?))
            }).unwrap_or((0.0, 0))
        } else {
            conn.query_row(stmt, params![p_str], |r| {
                Ok((r.get::<_, f32>(0)?, r.get::<_, i64>(1)?))
            }).unwrap_or((0.0, 0))
        }
    };

    let (sum_support, count_support, sum_contradict, count_contradict): (f32, i64, f32, i64) = {
        // Agent path keys on memory_id INTEGER; core path matches source_ref TEXT.
        let support_sql = if memory_kind == "agent" {
            "SELECT COALESCE(SUM(weight),0.0), COUNT(*) FROM memory_evidence \
             WHERE memory_kind='agent' AND memory_id=?1 AND kind='support'"
        } else {
            "SELECT COALESCE(SUM(weight),0.0), COUNT(*) FROM memory_evidence \
             WHERE memory_kind='core' AND source_ref=?1 AND kind='support'"
        };
        let contradict_sql = if memory_kind == "agent" {
            "SELECT COALESCE(SUM(weight),0.0), COUNT(*) FROM memory_evidence \
             WHERE memory_kind='agent' AND memory_id=?1 AND kind='contradict'"
        } else {
            "SELECT COALESCE(SUM(weight),0.0), COUNT(*) FROM memory_evidence \
             WHERE memory_kind='core' AND source_ref=?1 AND kind='contradict'"
        };
        let int_id = mid.unwrap_or(0);
        let (ss, sc) = pair(support_sql,    int_id, memory_id_str);
        let (cs, cc) = pair(contradict_sql, int_id, memory_id_str);
        (ss, sc, cs, cc)
    };

    let total = sum_support + sum_contradict;
    let (strength, from_prior) = if total > 0.0 {
        (sum_support / total, false)
    } else {
        // Fall back to the row's prior confidence column so brand-new
        // memories don't get filtered out before they've been observed.
        let prior: f32 = if memory_kind == "agent" {
            if let Some(id) = mid {
                conn.query_row(
                    "SELECT confidence FROM agent_memories WHERE id=?1",
                    params![id],
                    |r| r.get::<_, f32>(0),
                ).optional().map_err(|e| e.to_string())?.unwrap_or(0.5)
            } else { 0.5 }
        } else {
            conn.query_row(
                "SELECT confidence FROM memory_core WHERE id=?1",
                params![memory_id_str],
                |r| r.get::<_, f32>(0),
            ).optional().map_err(|e| e.to_string())?.unwrap_or(0.5)
        };
        (prior, true)
    };

    Ok(GroundingScore {
        strength,
        support_weight:    sum_support,
        contradict_weight: sum_contradict,
        support_count:     count_support,
        contradict_count:  count_contradict,
        from_prior,
    })
}

// ── Tauri commands ─────────────────────────────────────────────────────────

/// Computes the live grounding score for a single memory. The frontend
/// calls this when it wants to render the `◉ 87% confianza` chip or
/// when the prompt-injection path needs to decide whether to include
/// the memory.
#[tauri::command]
pub async fn memory_grounding(
    memory_kind: String,
    memory_id: String,
) -> Result<GroundingScore, String> {
    let mk = memory_kind.clone();
    let mid = memory_id.clone();
    shared_db(move |conn| compute_grounding_inner(conn, &mk, &mid))
        .map_err(|e| e.to_string())
}

/// Add a support or contradict signal for a memory. `weight` defaults to
/// 1.0; callers SHOULD pass a higher value (e.g. 2.0) for explicit user
/// feedback and a lower value (e.g. 0.4) for low-confidence automated
/// signals. `source_ref` is free-form provenance text — for `core`
/// memories it MUST be the memory's TEXT id (see compute_grounding_inner).
#[derive(Debug, Clone, Deserialize)]
pub struct EvidenceInput {
    pub memory_kind: String,
    /// For 'agent' memories: parsable as i64.
    /// For 'core' memories: the TEXT id (also passed in `source_ref`).
    pub memory_id: String,
    pub kind: String,
    #[serde(default = "default_weight")]
    pub weight: f32,
    #[serde(default)]
    pub source_ref: String,
}
fn default_weight() -> f32 { 1.0 }

#[tauri::command]
pub async fn memory_evidence_log(event: EvidenceInput) -> Result<i64, String> {
    if event.kind != "support" && event.kind != "contradict" {
        return Err(format!("invalid kind '{}', expected 'support' or 'contradict'", event.kind));
    }
    if event.memory_kind != "agent" && event.memory_kind != "core" {
        return Err(format!("invalid memory_kind '{}', expected 'agent' or 'core'", event.memory_kind));
    }

    let mid_int: i64 = if event.memory_kind == "agent" {
        event.memory_id.parse()
            .map_err(|_| format!("agent memory_id '{}' is not an integer", event.memory_id))?
    } else {
        0 // core uses source_ref for lookup
    };
    let source_ref = if event.memory_kind == "core" && event.source_ref.is_empty() {
        event.memory_id.clone() // canonical source_ref for core = the TEXT id
    } else {
        event.source_ref.clone()
    };

    let id = shared_db(move |conn| {
        conn.execute(
            "INSERT INTO memory_evidence (memory_kind, memory_id, kind, weight, source_ref) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![event.memory_kind, mid_int, event.kind, event.weight, source_ref],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    })?;

    Ok(id)
}

// ── Instances / provenance ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInstance {
    pub id:           i64,
    pub memory_kind:  String,
    pub quote_text:   String,
    pub source_kind:  String,
    pub source_ref:   String,
    pub offset_start: i64,
    pub offset_end:   i64,
    pub created_at:   i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstanceInput {
    pub memory_kind: String,
    pub memory_id: String,
    pub quote_text: String,
    #[serde(default = "default_source_kind")]
    pub source_kind: String,
    #[serde(default)]
    pub source_ref: String,
    #[serde(default)]
    pub offset_start: i64,
    #[serde(default)]
    pub offset_end: i64,
}
fn default_source_kind() -> String { "chat".to_string() }

#[tauri::command]
pub async fn memory_instance_save(inst: InstanceInput) -> Result<i64, String> {
    if inst.memory_kind != "agent" && inst.memory_kind != "core" {
        return Err(format!("invalid memory_kind '{}'", inst.memory_kind));
    }

    let mid_int: Option<i64> = if inst.memory_kind == "agent" {
        Some(inst.memory_id.parse()
            .map_err(|_| format!("agent memory_id '{}' is not an integer", inst.memory_id))?)
    } else {
        None
    };
    let mid_text: Option<String> = if inst.memory_kind == "core" {
        Some(inst.memory_id.clone())
    } else {
        None
    };

    let id = shared_db(move |conn| {
        conn.execute(
            "INSERT INTO memory_instances \
             (memory_kind, memory_id, memory_id_text, quote_text, source_kind, source_ref, offset_start, offset_end) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                inst.memory_kind, mid_int, mid_text,
                inst.quote_text, inst.source_kind, inst.source_ref,
                inst.offset_start, inst.offset_end,
            ],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    })?;

    Ok(id)
}

#[tauri::command]
pub async fn memory_instances_for(
    memory_kind: String,
    memory_id: String,
    limit: Option<i64>,
) -> Result<Vec<MemoryInstance>, String> {
    let cap = limit.unwrap_or(20).clamp(1, 200);
    let mid_int: Option<i64> = memory_id.parse().ok();

    shared_db(move |conn| {
        let rows: Vec<MemoryInstance> = if memory_kind == "agent" {
            if let Some(id) = mid_int {
                let mut stmt = conn.prepare(
                    "SELECT id, memory_kind, quote_text, source_kind, source_ref, \
                            offset_start, offset_end, created_at \
                     FROM memory_instances \
                     WHERE memory_kind='agent' AND memory_id=?1 \
                     ORDER BY created_at DESC LIMIT ?2"
                ).map_err(|e| e.to_string())?;
                let it = stmt.query_map(params![id, cap], |r| Ok(MemoryInstance {
                    id:          r.get(0)?,
                    memory_kind: r.get(1)?,
                    quote_text:  r.get(2)?,
                    source_kind: r.get(3)?,
                    source_ref:  r.get(4)?,
                    offset_start: r.get(5)?,
                    offset_end:  r.get(6)?,
                    created_at:  r.get(7)?,
                })).map_err(|e| e.to_string())?;
                it.filter_map(|r| r.ok()).collect()
            } else { Vec::new() }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, memory_kind, quote_text, source_kind, source_ref, \
                        offset_start, offset_end, created_at \
                 FROM memory_instances \
                 WHERE memory_kind='core' AND memory_id_text=?1 \
                 ORDER BY created_at DESC LIMIT ?2"
            ).map_err(|e| e.to_string())?;
            let it = stmt.query_map(params![memory_id, cap], |r| Ok(MemoryInstance {
                id:          r.get(0)?,
                memory_kind: r.get(1)?,
                quote_text:  r.get(2)?,
                source_kind: r.get(3)?,
                source_ref:  r.get(4)?,
                offset_start: r.get(5)?,
                offset_end:  r.get(6)?,
                created_at:  r.get(7)?,
            })).map_err(|e| e.to_string())?;
            it.filter_map(|r| r.ok()).collect()
        };
        Ok(rows)
    })
}

/// Free-text search across the FTS5 instance index. Useful for the
/// "where did Lucy learn X" UI flow — the user types a query and gets
/// back the literal quote snippets that triggered relevant memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceHit {
    pub instance:     MemoryInstance,
    /// The memory_id this instance belongs to (as string — caller
    /// disambiguates by memory_kind).
    pub memory_id:    String,
}

#[tauri::command]
pub async fn memory_instances_search(
    query: String,
    limit: Option<i64>,
) -> Result<Vec<InstanceHit>, String> {
    let cap = limit.unwrap_or(10).clamp(1, 50);
    shared_db(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT mi.id, mi.memory_kind, mi.memory_id, mi.memory_id_text, \
                    mi.quote_text, mi.source_kind, mi.source_ref, \
                    mi.offset_start, mi.offset_end, mi.created_at \
             FROM memory_instances_fts f \
             JOIN memory_instances mi ON mi.id = f.rowid \
             WHERE memory_instances_fts MATCH ?1 \
             ORDER BY rank \
             LIMIT ?2"
        ).map_err(|e| e.to_string())?;
        let it = stmt.query_map(params![query, cap], |r| {
            let mem_id_int: Option<i64> = r.get(2)?;
            let mem_id_text: Option<String> = r.get(3)?;
            let mem_id_str = mem_id_int.map(|x| x.to_string())
                .or(mem_id_text)
                .unwrap_or_default();
            Ok(InstanceHit {
                instance: MemoryInstance {
                    id:           r.get(0)?,
                    memory_kind:  r.get(1)?,
                    quote_text:   r.get(4)?,
                    source_kind:  r.get(5)?,
                    source_ref:   r.get(6)?,
                    offset_start: r.get(7)?,
                    offset_end:   r.get(8)?,
                    created_at:   r.get(9)?,
                },
                memory_id: mem_id_str,
            })
        }).map_err(|e| e.to_string())?;
        let rows: Vec<InstanceHit> = it.filter_map(|r| r.ok()).collect();
        Ok(rows)
    })
}
