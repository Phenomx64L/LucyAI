// ── Database initialization & schema management for Nivel 2 features ──
// Cost tracking, permission rules, and skills/runbooks storage

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub id: String,
    pub task_id: String,
    pub timestamp: String,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_cost: f64,
    pub user: String,
    pub request_type: String, // 'quick_command', 'agent_loop', 'stream'
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub id: String,
    pub pattern: String,
    pub action: String, // 'allow', 'block', 'ask'
    pub description: String,
    pub priority: u32,
    pub applies_to: String, // 'command', 'file_path', 'registry'
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub category: String, // 'quick_cmd', 'runbook', 'macro'
    pub triggers: String, // JSON array of trigger phrases
    pub script: String,
    pub description: String,
    pub parameters: String, // JSON array of parameter definitions
    pub created_at: String,
    pub updated_at: String,
    pub usage_count: u32,
    pub last_executed: Option<String>,
    pub enabled: bool,
    pub tags: String, // JSON array of tags
}

// ── SRE / Incident Response (Nivel 4) ──────────────────────────────────────
// Inspired by OpenSRE's state-machine + evidence-backed reasoning.
// An Incident is a structured troubleshooting session. Its phases follow
// extract → plan → investigate → diagnose → report → done. Evidence is
// tagged automatically from command outputs, hypotheses reference evidence
// IDs, and a validity score is computed before the final report is shown.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub id: String,
    pub shell_id: String,       // session / terminal this incident belongs to
    pub host_name: String,      // human label for the target host
    pub title: String,          // short alert / task description
    pub description: String,    // richer user context
    pub phase: String,          // 'extract'|'plan'|'investigate'|'diagnose'|'report'|'done'
    pub status: String,         // 'open'|'resolved'|'abandoned'
    pub validity_score: f64,    // 0..1 — how well hypotheses are backed by evidence
    pub loop_count: u32,        // investigation loops used
    pub max_loops: u32,         // ceiling before auto-finalize
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
    pub summary: Option<String>,// final resolution narrative
    pub root_cause: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentEvidence {
    pub id: String,
    pub incident_id: String,
    pub kind: String,           // 'command_output'|'log'|'metric'|'user_input'|'observation'|'hypothesis_test'
    pub source: String,         // e.g. 'powershell:Get-Service', 'user', 'event_log'
    pub content: String,        // raw payload (output text, JSON, etc.)
    pub tags: String,           // JSON array of labels: ["cpu","high"]
    pub timestamp: i64,
    pub phase: String,          // phase active when captured
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentHypothesis {
    pub id: String,
    pub incident_id: String,
    pub claim: String,                  // "Service X is crashing due to OOM"
    pub supporting_evidence_ids: String,// JSON array of evidence IDs
    pub contradicting_evidence_ids: String,// JSON array
    pub status: String,                 // 'proposed'|'validated'|'refuted'|'inconclusive'
    pub confidence: f64,                // 0..1 — self-reported by LLM, gated by score
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentAction {
    pub id: String,
    pub incident_id: String,
    pub phase: String,
    pub rationale: String,      // why the LLM chose this action
    pub command: String,        // what was executed (if applicable)
    pub output_evidence_id: Option<String>, // link to captured evidence
    pub executed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkResult {
    pub id:          String,
    pub task_id:     String,
    pub tab_id:      String,
    pub session_id:  String,
    pub model:       String,
    pub instruction: String,
    pub status:      String,   // 'running' | 'done' | 'error'
    pub result:      Option<String>,
    pub error_msg:   Option<String>,
    pub created_at:  i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemory {
    pub id:         i64,
    pub session_id: String,
    pub title:      String,
    pub content:    String,
    pub tags:       String,  // JSON array
    pub files:      String,  // JSON array
    pub importance: i64,     // 1-3
    pub created_at: i64,     // unix epoch
}

/// Create tables for cost tracking, permission rules, and skills
pub const INIT_SQL: &str = r#"
-- Token usage tracking for cost calculation
CREATE TABLE IF NOT EXISTS token_usage (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost REAL NOT NULL DEFAULT 0.0,
    user TEXT NOT NULL,
    request_type TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Daily cost summaries
CREATE TABLE IF NOT EXISTS daily_summary (
    date TEXT NOT NULL,
    model TEXT NOT NULL,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost REAL NOT NULL DEFAULT 0.0,
    request_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, model)
);

-- Permission rules for command/file/registry access control
CREATE TABLE IF NOT EXISTS permission_rules (
    id TEXT PRIMARY KEY,
    pattern TEXT NOT NULL,
    action TEXT NOT NULL,
    description TEXT,
    priority INTEGER NOT NULL DEFAULT 0,
    applies_to TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Persistent skills/runbooks storage
CREATE TABLE IF NOT EXISTS skills (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL,
    triggers TEXT NOT NULL,
    script TEXT NOT NULL,
    description TEXT,
    parameters TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    usage_count INTEGER NOT NULL DEFAULT 0,
    last_executed TEXT,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    tags TEXT
);

-- Agent cross-session memory (observations discovered during tasks)
CREATE TABLE IF NOT EXISTS agent_memories (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT    NOT NULL DEFAULT '',
    title      TEXT    NOT NULL,
    content    TEXT    NOT NULL,
    tags       TEXT    NOT NULL DEFAULT '[]',
    files      TEXT    NOT NULL DEFAULT '[]',
    importance INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_agent_memories_created    ON agent_memories(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_memories_importance ON agent_memories(importance DESC);

-- FTS5 full-text search for memories
CREATE VIRTUAL TABLE IF NOT EXISTS agent_memories_fts
    USING fts5(title, content, tags);

-- Keep FTS in sync automatically
CREATE TRIGGER IF NOT EXISTS agent_memories_ai
    AFTER INSERT ON agent_memories BEGIN
        INSERT INTO agent_memories_fts(rowid, title, content, tags)
        VALUES (new.id, new.title, new.content, new.tags);
    END;
-- NOTE: agent_memories_fts is a regular (own-content) FTS5 table, so the
-- 'delete' command form (only valid for contentless / external-content FTS5)
-- is NOT allowed here. Use a normal DELETE instead.
CREATE TRIGGER IF NOT EXISTS agent_memories_ad
    AFTER DELETE ON agent_memories BEGIN
        DELETE FROM agent_memories_fts WHERE rowid = old.id;
    END;
CREATE TRIGGER IF NOT EXISTS agent_memories_au
    AFTER UPDATE ON agent_memories BEGIN
        DELETE FROM agent_memories_fts WHERE rowid = old.id;
        INSERT INTO agent_memories_fts(rowid, title, content, tags)
            VALUES (new.id, new.title, new.content, new.tags);
    END;

-- ── SRE / Incident Response (Nivel 4) ────────────────────────────────────
-- Incident = structured troubleshooting session with phases, evidence, hypotheses.
CREATE TABLE IF NOT EXISTS incidents (
    id             TEXT PRIMARY KEY,
    shell_id       TEXT NOT NULL,
    host_name      TEXT NOT NULL DEFAULT '',
    title          TEXT NOT NULL,
    description    TEXT NOT NULL DEFAULT '',
    phase          TEXT NOT NULL DEFAULT 'extract',
    status         TEXT NOT NULL DEFAULT 'open',
    validity_score REAL NOT NULL DEFAULT 0.0,
    loop_count     INTEGER NOT NULL DEFAULT 0,
    max_loops      INTEGER NOT NULL DEFAULT 5,
    created_at     INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at     INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    resolved_at    INTEGER,
    summary        TEXT,
    root_cause     TEXT
);

-- Evidence collected during an incident. Each piece is addressable by id so
-- hypotheses can reference specific proofs. Kind is an enum for filtering.
CREATE TABLE IF NOT EXISTS incident_evidence (
    id           TEXT PRIMARY KEY,
    incident_id  TEXT NOT NULL,
    kind         TEXT NOT NULL,
    source       TEXT NOT NULL DEFAULT '',
    content      TEXT NOT NULL,
    tags         TEXT NOT NULL DEFAULT '[]',
    timestamp    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    phase        TEXT NOT NULL DEFAULT 'investigate',
    FOREIGN KEY (incident_id) REFERENCES incidents(id) ON DELETE CASCADE
);

-- Hypotheses the LLM proposes; each links back to supporting evidence ids.
CREATE TABLE IF NOT EXISTS incident_hypothesis (
    id                           TEXT PRIMARY KEY,
    incident_id                  TEXT NOT NULL,
    claim                        TEXT NOT NULL,
    supporting_evidence_ids      TEXT NOT NULL DEFAULT '[]',
    contradicting_evidence_ids   TEXT NOT NULL DEFAULT '[]',
    status                       TEXT NOT NULL DEFAULT 'proposed',
    confidence                   REAL NOT NULL DEFAULT 0.0,
    created_at                   INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    FOREIGN KEY (incident_id) REFERENCES incidents(id) ON DELETE CASCADE
);

-- Actions executed during the investigation (audit trail of the agent).
CREATE TABLE IF NOT EXISTS incident_action (
    id                    TEXT PRIMARY KEY,
    incident_id           TEXT NOT NULL,
    phase                 TEXT NOT NULL,
    rationale             TEXT NOT NULL DEFAULT '',
    command               TEXT NOT NULL DEFAULT '',
    output_evidence_id    TEXT,
    executed_at           INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    FOREIGN KEY (incident_id) REFERENCES incidents(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_incidents_shell     ON incidents(shell_id, status);
CREATE INDEX IF NOT EXISTS idx_incidents_status    ON incidents(status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_evidence_incident   ON incident_evidence(incident_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_hypothesis_incident ON incident_hypothesis(incident_id);
CREATE INDEX IF NOT EXISTS idx_action_incident     ON incident_action(incident_id, executed_at);

-- ── Conversation History (Hermes-inspired /recall) ──────────────────────
-- Persist every visible turn so the user can later search past sessions.
-- Separate from `agent_memories` (which is curated, importance-ranked):
-- this is raw history — high volume, low signal, but full-text searchable.
CREATE TABLE IF NOT EXISTS conversation_turns (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    tab_id     TEXT    NOT NULL DEFAULT '',
    tab_title  TEXT    NOT NULL DEFAULT '',
    role       TEXT    NOT NULL,            -- 'user' | 'lucy' | 'system'
    content    TEXT    NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_conv_turns_created ON conversation_turns(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_conv_turns_tab     ON conversation_turns(tab_id, created_at);

-- ── Fork Results (Sprint 4 — Persistent Parallel Agents) ──────────────────
-- Stores sub-agent fork results across sessions. In-memory forkedTasks{} is
-- ephemeral; this table survives tab switches, reloads, and app restarts.
-- Results are automatically pruned after 7 days.
CREATE TABLE IF NOT EXISTS fork_results (
    id          TEXT PRIMARY KEY,
    task_id     TEXT NOT NULL,       -- unique name given by LLM (e.g. 'check_cpu')
    tab_id      TEXT NOT NULL DEFAULT '',
    session_id  TEXT NOT NULL DEFAULT '',
    model       TEXT NOT NULL DEFAULT '',
    instruction TEXT NOT NULL,       -- original prompt sent to sub-agent
    status      TEXT NOT NULL DEFAULT 'running', -- 'running'|'done'|'error'
    result      TEXT,                -- sub-agent response (NULL while running)
    error_msg   TEXT,                -- error detail if status='error'
    created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    finished_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_fork_tab     ON fork_results(tab_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_fork_status  ON fork_results(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_fork_task_id ON fork_results(task_id);

CREATE VIRTUAL TABLE IF NOT EXISTS conversation_turns_fts
    USING fts5(content, tab_title, role, content='conversation_turns', content_rowid='id');

CREATE TRIGGER IF NOT EXISTS conv_turns_ai
    AFTER INSERT ON conversation_turns BEGIN
        INSERT INTO conversation_turns_fts(rowid, content, tab_title, role)
        VALUES (new.id, new.content, new.tab_title, new.role);
    END;
CREATE TRIGGER IF NOT EXISTS conv_turns_ad
    AFTER DELETE ON conversation_turns BEGIN
        INSERT INTO conversation_turns_fts(conversation_turns_fts, rowid, content, tab_title, role)
        VALUES ('delete', old.id, old.content, old.tab_title, old.role);
    END;

-- ── User Profile (Hermes-inspired persistent user memory) ────────────────
-- Flexible key-value store for user identity, preferences, frequent hosts,
-- and free-form context the AI picks up over time. Injected into system
-- prompt so Lucy doesn't re-ask the user for things she already knows.
CREATE TABLE IF NOT EXISTS user_profile (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    category   TEXT NOT NULL DEFAULT 'general',  -- 'identity'|'preference'|'context'|'host'|'general'
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_user_profile_category ON user_profile(category);

-- ── Quality Telemetry (opus-4-7 Tier 2.A) ─────────────────────────────────
-- Lightweight event log for measuring first-try success, retry counts,
-- confidence distribution, plan usage, batch efficiency. One row per event.
-- Intentionally denormalized — analytical queries aggregate on demand.
CREATE TABLE IF NOT EXISTS task_events (
    id         TEXT PRIMARY KEY,
    timestamp  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    tab_id     TEXT,
    event_type TEXT NOT NULL,   -- 'exec_success'|'exec_failure'|'retry'|'confidence'|'plan_execute'|'plan_dryrun'|'plan_cancel'|'batch'|'first_try_success'
    subtype    TEXT,            -- e.g. 'high'|'med'|'low' for confidence; 'local'|'remote' for exec
    elapsed_ms INTEGER,
    metadata   TEXT             -- free-form JSON
);
CREATE INDEX IF NOT EXISTS idx_task_events_timestamp ON task_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_task_events_type ON task_events(event_type);

-- MemGPT-style tiered memory (Sprint 3)
-- ── Core memory: small set of always-on facts & rules (injected into every
--    system prompt). Kept tight — target <2 KB total. Examples: user's OS,
--    preferred shell, critical host nicknames, policy rules.
CREATE TABLE IF NOT EXISTS memory_core (
    id         TEXT PRIMARY KEY,
    section    TEXT NOT NULL,      -- 'user_facts'|'preferences'|'rules'|'environment'
    key        TEXT NOT NULL,      -- short label, shown to LLM
    value      TEXT NOT NULL,      -- the fact itself
    pinned     INTEGER NOT NULL DEFAULT 1,  -- 1 = always inject; 0 = soft-pin
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_core_section_key ON memory_core(section, key);

-- ── Working memory: per-session compressed summaries. When the agent loop
--    context gets long, Lucy (or the reactive compactor) writes a summary
--    here and drops detail. At recall time these summaries are cheaper than
--    the raw turns they replaced.
CREATE TABLE IF NOT EXISTS memory_working (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL,
    tab_id      TEXT,
    summary     TEXT NOT NULL,
    token_count INTEGER NOT NULL DEFAULT 0,  -- approx input tokens this summary replaces
    original_len INTEGER NOT NULL DEFAULT 0, -- original char length compressed
    created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_memory_working_session ON memory_working(session_id, created_at);

-- Semantic embeddings (Sprint 2 — vector search on top of SQLite)
-- vec: little-endian f32 array stored as BLOB; cosine is computed in Rust
-- after loading rows by entity_type (bucket size expected <10k).
CREATE TABLE IF NOT EXISTS embeddings (
    id           TEXT PRIMARY KEY,
    entity_type  TEXT NOT NULL,  -- 'skill'|'runbook'|'memory'|'log'|'incident'
    entity_id    TEXT NOT NULL,
    text         TEXT NOT NULL,  -- the original text that was embedded (for dedupe/debug)
    vec          BLOB NOT NULL,  -- raw f32 little-endian
    dims         INTEGER NOT NULL,
    model        TEXT NOT NULL,  -- e.g. 'nomic-embed-text' or 'mxbai-embed-large'
    created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_embeddings_entity ON embeddings(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_embeddings_type ON embeddings(entity_type);

-- ── PDF Intelligence (Sprint 4 Pillar 4) ────────────────────────────────────
-- Tracks every ingested PDF. Chunks live in agent_memories
-- (session_id = 'pdf:{id}') and embeddings table (entity_type = 'pdf_chunk').
CREATE TABLE IF NOT EXISTS pdf_documents (
    id          TEXT    PRIMARY KEY,
    filename    TEXT    NOT NULL,
    path        TEXT    NOT NULL,
    page_count  INTEGER NOT NULL DEFAULT 0,
    chunk_count INTEGER NOT NULL DEFAULT 0,
    ingested_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    status      TEXT    NOT NULL DEFAULT 'ingesting'  -- 'ingesting'|'done'|'error'
);
CREATE INDEX IF NOT EXISTS idx_pdf_docs_ingested ON pdf_documents(ingested_at DESC);

-- Create indexes for fast queries
CREATE INDEX IF NOT EXISTS idx_token_usage_timestamp ON token_usage(timestamp);
CREATE INDEX IF NOT EXISTS idx_token_usage_model ON token_usage(model);
CREATE INDEX IF NOT EXISTS idx_permission_rules_enabled ON permission_rules(enabled, priority);
CREATE INDEX IF NOT EXISTS idx_skills_name ON skills(name);
CREATE INDEX IF NOT EXISTS idx_skills_category ON skills(category);

-- ── Principles (Maestro-inspired) ─────────────────────────────────────────
-- Behavioral rules injected into the system prompt before each turn.
-- Lucy reads these and adjusts her approach (e.g. "always validate with
-- Get-* before Set-*", "prefer PowerShell 7 over 5"). Per-host scoping
-- supported via 'scope' (NULL = global, host_id otherwise).
CREATE TABLE IF NOT EXISTS principles (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    rule        TEXT NOT NULL,                       -- the actual instruction
    scope       TEXT,                                -- NULL = global, else host_id or project tag
    priority    INTEGER NOT NULL DEFAULT 100,        -- lower = applied first / shown first
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_principles_enabled ON principles(enabled, priority);
CREATE INDEX IF NOT EXISTS idx_principles_scope   ON principles(scope);

-- ── Scheduled tasks (Hermes-inspired natural-language cron) ───────────────
-- Each row is a recurring or one-shot task. The tauri startup spawns a
-- ticker that wakes every 60s, queries `next_run <= now()`, and dispatches
-- the prompt to a fresh agent run via the gateway. Last run + outcome
-- are recorded for the Insights view.
CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL,
    prompt       TEXT NOT NULL,                       -- natural-language task body
    cron_expr    TEXT,                                -- '0 9 * * *' style; NULL for one-shot
    next_run     INTEGER NOT NULL,                    -- unix epoch
    last_run     INTEGER,                             -- unix epoch of most recent run
    last_status  TEXT,                                -- 'ok' | 'error' | 'skipped'
    last_output  TEXT,                                -- truncated tail
    enabled      INTEGER NOT NULL DEFAULT 1,
    created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at   INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_scheduled_next ON scheduled_tasks(enabled, next_run);
"#;

// ── PDF Intelligence (Sprint 4 Pillar 4) ──────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfDocument {
    pub id:          String,
    pub filename:    String,
    pub path:        String,
    pub page_count:  i64,   // 0 until extraction (pdf-extract doesn't report pages)
    pub chunk_count: i64,
    pub ingested_at: i64,
    pub status:      String,  // 'ingesting' | 'done' | 'error'
}

/// Price constants per 1K tokens (as of 2026)
pub const ANTHROPIC_INPUT_PRICE_PER_1K: f64 = 0.003;
pub const ANTHROPIC_OUTPUT_PRICE_PER_1K: f64 = 0.015;

pub const OPENAI_GPT4_INPUT_PRICE_PER_1K: f64 = 0.03;
pub const OPENAI_GPT4_OUTPUT_PRICE_PER_1K: f64 = 0.06;

pub const GOOGLE_GEMINI_INPUT_PRICE_PER_1K: f64 = 0.0005;
pub const GOOGLE_GEMINI_OUTPUT_PRICE_PER_1K: f64 = 0.0015;

/// Calculate token cost based on model and token counts
pub fn calculate_cost(model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
    let (in_price, out_price) = match model {
        m if m.contains("claude") => (ANTHROPIC_INPUT_PRICE_PER_1K, ANTHROPIC_OUTPUT_PRICE_PER_1K),
        m if m.contains("gpt") => (OPENAI_GPT4_INPUT_PRICE_PER_1K, OPENAI_GPT4_OUTPUT_PRICE_PER_1K),
        m if m.contains("gemini") => (GOOGLE_GEMINI_INPUT_PRICE_PER_1K, GOOGLE_GEMINI_OUTPUT_PRICE_PER_1K),
        _ => (0.0, 0.0), // Unknown model, no cost
    };

    let input_cost = (input_tokens as f64 / 1000.0) * in_price;
    let output_cost = (output_tokens as f64 / 1000.0) * out_price;
    input_cost + output_cost
}

/// Generate unique ID — nanosecond timestamp + monotonic counter.
/// Safe under concurrency: even two calls in the same nanosecond get distinct IDs.
pub fn generate_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}_{:x}", now_ns, n)
}
