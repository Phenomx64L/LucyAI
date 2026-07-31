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
    pub chain_hash: Option<String>, // SHA-256 hash-chain for tamper-evidence
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
    /// Tier A #1 — task_id of the fork that spawned this one. '' = root.
    #[serde(default)]
    pub parent_task_id: String,
    /// Tier A #1 — token & cost ledger (only meaningful for status='done').
    #[serde(default)]
    pub tokens_in:  i64,
    #[serde(default)]
    pub tokens_out: i64,
    #[serde(default)]
    pub cost_usd:   f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../src/lib/types/")]
pub struct AgentMemory {
    pub id:         i64,
    pub session_id: String,
    pub title:      String,
    pub content:    String,
    pub tags:       String,  // JSON array (kept as string for SQLite compat)
    pub files:      String,  // JSON array
    pub importance: i64,     // 1-3
    pub created_at: i64,     // unix epoch seconds
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

-- Mem0-inspired columns (added May 2026 — schema migration). Idempotent
-- via `ALTER TABLE ADD COLUMN`; SQLite returns "duplicate column" silently
-- on second run, which we catch in metrics::init().
-- • last_accessed_at — bumped on every successful search hit. Drives
--   recency-weighted ranking so frequently-used memories surface first
--   while stale ones decay out of the top-K.
-- • access_count — running tally of search hits. Used as a tie-breaker
--   for recency + signal that this memory matters to the user.
-- • superseded_by — when a newer memory contradicts/replaces an old one,
--   we keep both rows but link them so audit trails stay intact. The
--   search query filters out superseded rows from the default result set.

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
-- chain_hash: SHA-256( prev_chain_hash || id || command || executed_at )
-- Forms a tamper-evident linked chain per incident. First action uses "GENESIS".
CREATE TABLE IF NOT EXISTS incident_action (
    id                    TEXT PRIMARY KEY,
    incident_id           TEXT NOT NULL,
    phase                 TEXT NOT NULL,
    rationale             TEXT NOT NULL DEFAULT '',
    command               TEXT NOT NULL DEFAULT '',
    output_evidence_id    TEXT,
    executed_at           INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    chain_hash            TEXT NOT NULL DEFAULT '',
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

-- ── Replay Deterministic Mode (Tier S #1) ──────────────────────────────
-- Snapshot of a single LLM turn so it can be re-run later byte-for-byte.
-- Every field needed to reproduce the EXACT call is captured:
--   • model + effort suffix
--   • full system_prompt (the composable identity + rules block)
--   • the user_prompt verbatim
--   • the context_block that was injected (memories + working dir + history)
--   • images (b64 JSON array) if any
--   • original response + tokens + latency for comparison
--
-- Why store full text instead of references: the underlying memories /
-- working dir / history MUTATE over time. A reference would replay against
-- *today's* context, not the original. The whole point of replay is to ask
-- "given this EXACT input back then, what does the model say NOW?".
--
-- Storage cost: ~5-30KB per snapshot. At 100 snapshots per day that's ~3MB
-- per day, ~1GB per year — acceptable for a local SQLite DB. We add a TTL
-- prune command (replay_clear) for housekeeping.
CREATE TABLE IF NOT EXISTS replay_snapshots (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    label           TEXT    NOT NULL DEFAULT '',
    task_id         TEXT    NOT NULL DEFAULT '',
    tab_id          TEXT    NOT NULL DEFAULT '',
    model           TEXT    NOT NULL,
    effort          TEXT    NOT NULL DEFAULT '',
    system_prompt   TEXT    NOT NULL DEFAULT '',
    user_prompt     TEXT    NOT NULL,
    context_block   TEXT    NOT NULL DEFAULT '',
    images_b64      TEXT    NOT NULL DEFAULT '[]',   -- JSON array
    original_response TEXT  NOT NULL DEFAULT '',
    original_tokens_in  INTEGER NOT NULL DEFAULT 0,
    original_tokens_out INTEGER NOT NULL DEFAULT 0,
    original_latency_ms INTEGER NOT NULL DEFAULT 0,
    temperature     REAL    NOT NULL DEFAULT 0.0,
    seed            INTEGER NULL,                    -- only OpenAI/Gemini/Ollama
    replays_run     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_replay_created ON replay_snapshots(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_replay_label   ON replay_snapshots(label);
CREATE INDEX IF NOT EXISTS idx_replay_tab     ON replay_snapshots(tab_id, created_at DESC);

-- ── NexShell Session Recording (Tier S #3) ────────────────────────────
-- Persistent timeline of every command + output of a remote shell session.
-- Two-table split:
--   • shell_recordings        — one row per recording (the "tape")
--   • shell_recording_events  — many rows per recording (cmd/out/err/exit)
--
-- Why split: a single shell session can emit thousands of chunks. Storing
-- each chunk as a separate row keeps writes cheap (no read-modify-write
-- on a giant blob) and lets the player paginate by time range during
-- playback instead of loading the entire tape into memory.
--
-- t_ms = milliseconds since recording start, NOT wall-clock. The recording
-- is replayable independently of when it was made — useful for sharing or
-- for replaying at a different speed.
--
-- Events are append-only. Once a recording is finished (ended_at set), the
-- tape is immutable except for delete.

CREATE TABLE IF NOT EXISTS shell_recordings (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT    NOT NULL,
    host_id     TEXT    NOT NULL DEFAULT '',
    host_name   TEXT    NOT NULL DEFAULT '',
    host_type   TEXT    NOT NULL DEFAULT '',   -- 'windows' | 'linux' | 'rdp'
    title       TEXT    NOT NULL DEFAULT '',
    started_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    ended_at    INTEGER NULL,
    event_count INTEGER NOT NULL DEFAULT 0,
    byte_count  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_shellrec_host    ON shell_recordings(host_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_shellrec_session ON shell_recordings(session_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_shellrec_started ON shell_recordings(started_at DESC);

CREATE TABLE IF NOT EXISTS shell_recording_events (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    recording_id  INTEGER NOT NULL,
    t_ms          INTEGER NOT NULL,            -- ms since recording started
    kind          TEXT    NOT NULL,            -- 'cmd' | 'out' | 'err' | 'meta' | 'exit'
    data          TEXT    NOT NULL DEFAULT '',
    FOREIGN KEY (recording_id) REFERENCES shell_recordings(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_shellrecev_rid_t ON shell_recording_events(recording_id, t_ms);

-- ── Inventory Drift Detection (Sprint B #2) ────────────────────────────
-- Stores a per-host "baseline" snapshot taken at a known-good moment.
-- Future scans diff against this baseline to surface drift: software
-- installed/removed, services changed, ports opened, scheduled tasks
-- added, etc.
--
-- Why a single row per host (not history): drift is "since the baseline
-- you trusted". A growing snapshot history would just become noise — if
-- you want point-in-time comparisons, you already have state_snapshots
-- for system-level drift. This table is specifically for inventory diff
-- against the operator-chosen reference.
--
-- snapshot_json shape mirrors the InventoryView snapshot: { ports[],
-- services[], software[], certs[], scheduled[] }. Stored as TEXT JSON
-- because the structure evolves over time and rigid columns would mean
-- ALTER TABLE on every Inventory schema bump.

CREATE TABLE IF NOT EXISTS inventory_baselines (
    host_id        TEXT    PRIMARY KEY,
    label          TEXT    NOT NULL DEFAULT '',
    snapshot_json  TEXT    NOT NULL,
    created_at     INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at     INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_inv_baseline_updated ON inventory_baselines(updated_at DESC);

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
    id           TEXT    PRIMARY KEY,
    filename     TEXT    NOT NULL,
    path         TEXT    NOT NULL,
    page_count   INTEGER NOT NULL DEFAULT 0,
    chunk_count  INTEGER NOT NULL DEFAULT 0,
    ingested_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    status       TEXT    NOT NULL DEFAULT 'ingesting',  -- 'ingesting'|'done'|'error'
    content_hash TEXT    NOT NULL DEFAULT '',           -- SHA-256 hex of extracted text (v1.7.233 idempotent re-ingest)
    synth_status TEXT    NOT NULL DEFAULT ''            -- '' pendiente | 'working' | 'done' | 'error:…' (v1.7.233 M2 síntesis)
);
CREATE INDEX IF NOT EXISTS idx_pdf_docs_ingested ON pdf_documents(ingested_at DESC);
-- NOTE: idx_pdf_docs_hash (on content_hash) is created in the metrics.rs
-- migrations array, NOT here. On an EXISTING database this INIT_SQL runs
-- BEFORE the ALTER that adds content_hash — an index referencing the column
-- here aborts the whole schema init ("Metrics DB not initialized").

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

-- ── Audit Trail (P0 Feature 1) ──────────────────────────────────────────
-- Persistent audit log of every command executed (local + remote).
-- Replaces the localStorage-backed persistedWritable<AuditEntry[]> in
-- stores.ts with a durable, queryable, size-capped SQLite table.
-- Auto-prune: frontend or periodic task deletes rows older than 90 days.
CREATE TABLE IF NOT EXISTS audit_trail (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   TEXT    NOT NULL,            -- ISO 8601
    host_id     TEXT    NOT NULL DEFAULT '', -- '' = local
    host_name   TEXT    NOT NULL DEFAULT 'local',
    command     TEXT    NOT NULL,
    source      TEXT    NOT NULL DEFAULT 'manual', -- 'manual'|'runbook'|'compliance'|'ai'|'broadcast'
    exit_code   INTEGER,                    -- NULL if unknown
    duration_ms INTEGER,                    -- NULL if not measured
    output_preview TEXT NOT NULL DEFAULT '', -- first ~500 chars of output
    user        TEXT    NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_audit_trail_created  ON audit_trail(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_trail_host     ON audit_trail(host_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_trail_source   ON audit_trail(source, created_at DESC);

-- ── Capacity Metrics (P0 Feature 3) ─────────────────────────────────────
-- Historical system metrics samples for trend projection. Sampled every
-- 5 min by a frontend interval or backend tick. Downsampled hourly
-- averages kept for 90 days; raw samples kept for 7 days.
CREATE TABLE IF NOT EXISTS metrics_samples (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id     TEXT    NOT NULL DEFAULT '', -- '' = local
    ts          INTEGER NOT NULL,           -- unix epoch seconds
    cpu         REAL    NOT NULL DEFAULT 0.0,
    ram         REAL    NOT NULL DEFAULT 0.0,  -- percent
    disk        REAL    NOT NULL DEFAULT 0.0,  -- percent (primary disk)
    ram_mb      INTEGER NOT NULL DEFAULT 0,    -- absolute MB used
    disk_gb     INTEGER NOT NULL DEFAULT 0,    -- absolute GB used
    disk_total_gb INTEGER NOT NULL DEFAULT 0,  -- total disk GB
    ram_total_mb INTEGER NOT NULL DEFAULT 0,   -- total RAM MB
    is_hourly   INTEGER NOT NULL DEFAULT 0     -- 1 = downsampled hourly avg
);
CREATE INDEX IF NOT EXISTS idx_metrics_samples_ts   ON metrics_samples(host_id, ts DESC);
CREATE INDEX IF NOT EXISTS idx_metrics_samples_hourly ON metrics_samples(is_hourly, ts DESC);

-- ── MCP Servers Registry (v1.4.2) ────────────────────────────────────────
-- First-class registry of MCP (Model Context Protocol) servers, similar
-- to claude_desktop_config.json / Cursor / Cline. Persists the spawn
-- command, which secret keys to inject as env vars, and a CACHE of the
-- last discovered tools so Lucy can include them in her system prompt
-- without re-spawning the process each turn.
--
-- env_keys is a JSON array of key names that map to entries already
-- stored in Windows Credential Manager via the existing MCP Secrets
-- mechanism. Splitting "which secrets exist" (Keyring) from "which
-- secrets THIS server needs" (env_keys) lets one secret bag serve
-- many servers without leaking unrelated tokens into every subprocess.
--
-- tools_cache is the verbatim JSON array returned by tools/list on the
-- last successful discover. Lucy embeds a compact form in the system
-- prompt so the LLM knows what's available without an extra round-trip.
CREATE TABLE IF NOT EXISTS mcp_servers (
    id              TEXT    PRIMARY KEY,
    name            TEXT    NOT NULL UNIQUE,
    command         TEXT    NOT NULL,                      -- full shell-style cmd: "npx -y @mcp/server-fs C:/path"
    env_keys        TEXT    NOT NULL DEFAULT '[]',         -- JSON array of secret key names
    enabled         INTEGER NOT NULL DEFAULT 1,
    tools_cache     TEXT    NOT NULL DEFAULT '[]',         -- last tools/list result (JSON array)
    last_discovered INTEGER,                                -- unix epoch
    last_status     TEXT    NOT NULL DEFAULT 'pending',    -- 'ok'|'error'|'pending'
    last_error      TEXT,
    last_latency_ms INTEGER,
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at      INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled ON mcp_servers(enabled, updated_at DESC);

-- ── Chip click memory (v1.4.2, Layer 3 of smart chips) ────────────────────
-- Logs every user interaction with a predictive chip so that, over time,
-- Lucy learns what suggestions ACTUALLY get used in each conversation
-- context. The Layer-3 retriever queries this table at chip-generation
-- time and surfaces past clicks whose context signature matches the
-- current turn — those appear in the strip with the ◊ "memory" badge.
--
-- Signature columns are stored as JSON arrays so the matcher can do
-- intersection checks without a separate junction table. Total rows
-- grow ~O(clicks/day) — for a heavy user that's <100/day, so even
-- 3 years of history fits in <10 MB. No vacuum schedule needed.
--
-- event_kind splits the table into clicks (positive signal) and
-- dismisses (negative signal) so the scorer can penalize chips the
-- user explicitly rejected in a similar context.
CREATE TABLE IF NOT EXISTS chip_click_log (
    id            TEXT    PRIMARY KEY,
    label         TEXT    NOT NULL,
    text          TEXT    NOT NULL,
    intent        TEXT    NOT NULL DEFAULT 'other',
    domains       TEXT    NOT NULL DEFAULT '[]',   -- JSON array of domain strings
    tool_labels   TEXT    NOT NULL DEFAULT '[]',   -- JSON array
    had_error     INTEGER NOT NULL DEFAULT 0,
    lang          TEXT    NOT NULL DEFAULT 'es-MX',
    event_kind    TEXT    NOT NULL DEFAULT 'click', -- 'click' | 'dismiss'
    occurred_at   INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX IF NOT EXISTS idx_chip_log_recent ON chip_click_log(occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_chip_log_kind   ON chip_click_log(event_kind, occurred_at DESC);
"#;

// ── Audit Trail (P0 Feature 1) ────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id:             i64,
    pub timestamp:      String,
    pub host_id:        String,
    pub host_name:      String,
    pub command:        String,
    pub source:         String,
    pub exit_code:      Option<i32>,
    pub duration_ms:    Option<i64>,
    pub output_preview: String,
    pub user:           String,
    pub created_at:     i64,
}

// ── Capacity Metrics (P0 Feature 3) ───────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]  // fields read via serde deserialization in capacity.rs
pub struct MetricsSample {
    pub id:            i64,
    pub host_id:       String,
    pub ts:            i64,
    pub cpu:           f64,
    pub ram:           f64,
    pub disk:          f64,
    pub ram_mb:        i64,
    pub disk_gb:       i64,
    pub disk_total_gb: i64,
    pub ram_total_mb:  i64,
    pub is_hourly:     bool,
}

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
    // v1.7.233 — how many of this doc's chunks have a pdf_chunk embedding.
    // `status='done'` only covers the chunk-SAVING phase; embeddings run in a
    // background task with no other visibility. embedded_count == chunk_count
    // means the document is 100% semantically searchable.
    #[serde(default)]
    pub embedded_count: i64,
}

// ── Per-model pricing (May 2026) ──────────────────────────────────────────
// MUST be kept in sync with src/lib/model-pricing.ts (frontend source of truth).
// Prices in USD per 1K tokens. Lookup is by base model id — the "::effort"
// suffix (e.g. "::xhigh", "::max") is stripped before lookup since effort
// changes the token COUNT, not the per-token rate.
//
// If a model id is not recognized, we conservatively fall back to a
// generic "cloud-mid" price so the user still gets a non-zero estimate.

/// Returns (input_per_1k, output_per_1k) USD prices for the given model.
fn model_prices(model: &str) -> (f64, f64) {
    // Strip "::effort" suffix — effort affects token count, not price.
    let base = match model.find("::") {
        Some(i) => &model[..i],
        None => model,
    };

    // Local Ollama: free
    if base.starts_with("local-") {
        return (0.0, 0.0);
    }

    // NVIDIA NIM (format "owner/model"): approximate average
    if base.contains('/') {
        return (0.0008, 0.0024);
    }

    match base {
        // ── Anthropic Claude ──
        // Keep in step with src/lib/model-pricing.ts — the two are read by
        // different halves of the UI and a drift between them shows up as the
        // footer and the pre-flight estimate disagreeing about the same turn.
        //
        // CORRECTED: 4.6/4.7/4.8 were carrying Opus 4.5's $15/$75 per 1M, 3×
        // the real price. The Opus tier dropped to $5/$25 at 4.6 and has held
        // there through Opus 5. Opus 4.5 keeps the old numbers — for 4.5 they
        // were right.
        "claude-opus-5"     |
        "claude-opus-4-8"   |
        "claude-opus-4-7"   |
        "claude-opus-4-6"     => (0.005,   0.025),
        "claude-opus-4-5"     => (0.015,   0.075),
        // Fable 5 — the only tier above Opus, at double its price.
        "claude-fable-5"      => (0.010,   0.050),
        // Sonnet 5 runs an introductory $2/$10 to 2026-08-31; we bill the
        // standard $3/$15 so a lapsed promo cannot start under-quoting.
        "claude-sonnet-5"   |
        "claude-sonnet-4-6" |
        "claude-sonnet-4-5"   => (0.003,   0.015),
        "claude-haiku-4-5"    => (0.001,   0.005),

        // ── Google Gemini ──
        // CORRECTED 2026-07-29 from ai.google.dev/gemini-api/docs/pricing. The
        // old numbers ran badly UNDER — gemini-3.5-flash was billed at
        // $0.30/$2.50 per 1M against a real $1.50/$9.00 (5× / 3.6× under), and
        // it is the default Gemini model, so the reported spend was a fraction
        // of the real one. 3.1 Pro is tier-priced by prompt size; we quote the
        // ≤200k tier ($2/$12) rather than the >200k one ($4/$18).
        "gemini-3.1-pro-preview"        => (0.0020,  0.012),
        "gemini-3.6-flash"              => (0.0015,  0.0075),
        "gemini-3.5-flash"              => (0.0015,  0.0090),
        "gemini-3-flash-preview"        => (0.0015,  0.0090), // legacy alias
        "gemini-3.5-flash-lite"         => (0.00030, 0.0025),
        "gemini-3.1-flash-lite"         |
        "gemini-3.1-flash-lite-preview" => (0.00025, 0.0015),
        // Legacy 2.5
        "gemini-2.5-pro"                => (0.00125, 0.010),
        "gemini-2.5-flash"              => (0.00030, 0.0025),
        "gemini-2.5-flash-lite-preview" => (0.00010, 0.0004),

        // ── OpenAI GPT ──
        // Verified 2026-07-29 from developers.openai.com/api/docs/pricing.
        // Standard tier. The 5.5/5.4/5.3 rows are legacy last-known values —
        // OpenAI no longer publishes them.
        "gpt-5.6-sol"     => (0.005,   0.030),
        "gpt-5.6-terra"   => (0.0025,  0.015),
        "gpt-5.6-luna"    => (0.001,   0.006),
        "gpt-5.5"         => (0.005,   0.020),
        "gpt-5.5-instant" => (0.002,   0.008),
        "gpt-5.4-mini"    => (0.00020, 0.00080),
        "gpt-5.4-nano"    => (0.00010, 0.00040),
        "gpt-5.3-codex"   => (0.005,   0.020),
        // Legacy
        "gpt-4o"          => (0.0025,  0.010),
        "gpt-4o-mini"     => (0.00015, 0.0006),

        // ── xAI Grok ── (docs.x.ai, 2026-07-29)
        // Tiered by prompt length; these are the under-200k rates, which is
        // where Lucy's compaction keeps every turn.
        "grok-4.5"        => (0.0020,  0.0060),
        "grok-4.3"        => (0.00125, 0.0025),

        // ── DeepSeek ── (api-docs.deepseek.com, 2026-07-29)
        // Cache-MISS input rates. The published cache-hit rate is ~50× lower,
        // but a hit is not predictable per request, so quoting it would
        // under-report every uncached turn.
        "deepseek-v4-flash" => (0.00014,  0.00028),
        "deepseek-v4-pro"   => (0.000435, 0.00087),

        // Fallback for unknown cloud model — conservative mid-tier estimate.
        _ => (0.001, 0.003),
    }
}

// Legacy constants kept for backward compatibility with any external callers.
// `#[allow(dead_code)]` because nothing in-tree references them — they exist
// only so an external consumer (e.g. a future dashboard plugin) could read the
// rough per-vendor mid-tier price without going through `model_prices()`.
#[allow(dead_code)] pub const ANTHROPIC_INPUT_PRICE_PER_1K: f64 = 0.003;
#[allow(dead_code)] pub const ANTHROPIC_OUTPUT_PRICE_PER_1K: f64 = 0.015;
#[allow(dead_code)] pub const OPENAI_GPT4_INPUT_PRICE_PER_1K: f64 = 0.005;
#[allow(dead_code)] pub const OPENAI_GPT4_OUTPUT_PRICE_PER_1K: f64 = 0.020;
#[allow(dead_code)] pub const GOOGLE_GEMINI_INPUT_PRICE_PER_1K: f64 = 0.00030;
#[allow(dead_code)] pub const GOOGLE_GEMINI_OUTPUT_PRICE_PER_1K: f64 = 0.0025;

/// Calculate token cost based on model and token counts.
/// Uses the per-model table; falls back to a mid-tier estimate for unknown ids.
pub fn calculate_cost(model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
    let (in_price, out_price) = model_prices(model);
    let input_cost = (input_tokens as f64 / 1000.0) * in_price;
    let output_cost = (output_tokens as f64 / 1000.0) * out_price;
    input_cost + output_cost
}

// ── The model catalog contract ───────────────────────────────────────────────
//
// Adding a cloud model to Lucy means editing FOUR places that nothing ties
// together:
//
//   1. src/lib/models.js        — LLM_GROUPS: which ids the dropdown offers,
//                                 and which provider each group belongs to
//   2. src/lib/model-pricing.ts — MODEL_PRICING: the frontend rate table
//   3. this file                — model_prices(): the backend rate table
//   4. commands/ai.rs           — provider_for_model(): where requests go
//
// The comment above model_prices() says it "MUST be kept in sync with
// src/lib/model-pricing.ts". Saying MUST is not a mechanism, and the drift it
// was meant to prevent had already happened twice by the time anyone measured
// it: every Opus from 4.6 on was billed at 4.5's $15/$75 (3× real), and
// gemini-3.5-flash — the DEFAULT Gemini model — at $0.30/$2.50 against a real
// $1.50/$9.00. Neither is visible from inside either file. Both look like a
// plausible number sitting next to other plausible numbers, and the only
// symptom is a cost report that is quietly wrong, which is the one number a
// user has no independent way to check.
//
// Every failure mode here is silent by construction:
//   • A missing price does not error — it returns a "conservative mid-tier"
//     guess and the estimate is simply wrong.
//   • A missing provider branch does not error either. provider_for_model()
//     ends in `else { "gemini" }`, so an unrouted id is not rejected, it is
//     sent to Google.
//
// So the catalog gets read from the JS/TS sources at compile time and checked
// against the Rust. Precedent: utils/shell.rs does the same thing to keep
// migrated callers from hand-rolling a PowerShell spawn.
#[cfg(test)]
mod catalog_contract {
    use super::model_prices;
    use crate::commands::ai::{provider_for_model, resolve_anthropic_model, resolve_gemini_model};

    const MODELS_JS: &str = include_str!("../../../src/lib/models.js");
    const PRICING_TS: &str = include_str!("../../../src/lib/model-pricing.ts");
    const LLM_TS: &str = include_str!("../../../src/lib/llm-models.ts");

    /// Every single-quoted string between `start` and `end` in a TS source.
    ///
    /// Comment lines are skipped first, and that is not a nicety: the very
    /// first prose comment added to one of these maps said "this file's own
    /// rule", and the apostrophe in "file's" shifted the quote pairing for
    /// everything after it. A scanner over source that cannot survive an
    /// apostrophe in a comment will break on the next sentence someone writes.
    fn quoted_strings_after(src: &str, start: &str, end: &str) -> Vec<String> {
        let body = src
            .split_once(start)
            .unwrap_or_else(|| panic!("'{start}' disappeared from its source file"))
            .1;
        let body = body.split_once(end).map_or(body, |(b, _)| b);

        let mut out = Vec::new();
        for line in body.lines() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
                continue;
            }
            let mut rest = t;
            while let Some(i) = rest.find('\'') {
                rest = &rest[i + 1..];
                let Some(j) = rest.find('\'') else { break };
                out.push(rest[..j].to_string());
                rest = &rest[j + 1..];
            }
        }
        out
    }

    /// `(group provider, model id)` for every entry the dropdown offers.
    ///
    /// Deliberately a dumb line scanner rather than a JS parse: the point is to
    /// fail when someone adds a model, and a scanner that only understands the
    /// file's actual formatting fails loudly on a reformat instead of quietly
    /// matching nothing. The empty-result assertion below is what guards that.
    fn catalog() -> Vec<(String, String)> {
        let body = MODELS_JS
            .split_once("export const LLM_GROUPS")
            .expect("LLM_GROUPS disappeared from models.js")
            .1;

        let mut provider = String::new();
        let mut out = Vec::new();
        for line in body.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("provider: \"") {
                provider = rest.split('"').next().unwrap_or_default().to_string();
            } else if let Some(rest) = t.strip_prefix("{ id: \"") {
                if let Some(id) = rest.split('"').next() {
                    out.push((provider.clone(), id.to_string()));
                }
            }
        }
        out
    }

    /// `id -> (inputPer1K, outputPer1K)` from the frontend rate table.
    fn frontend_prices() -> Vec<(String, f64, f64)> {
        let body = PRICING_TS
            .split_once("MODEL_PRICING")
            .expect("MODEL_PRICING disappeared from model-pricing.ts")
            .1;

        let num_after = |line: &str, key: &str| -> Option<f64> {
            let rest = line.split_once(key)?.1;
            let end = rest.find(',').unwrap_or(rest.len());
            rest[..end].trim().parse().ok()
        };

        let mut out = Vec::new();
        for line in body.lines() {
            let t = line.trim();
            // A rate row is `'id': { inputPer1K: …, outputPer1K: … }`. Requiring
            // both the quoted key AND inputPer1K skips ZERO_PRICING /
            // FALLBACK_PRICING and the prefix fallbacks inside getPricing(),
            // which are unkeyed literals.
            if !t.starts_with('\'') || !t.contains("inputPer1K") {
                continue;
            }
            let id = t[1..].split('\'').next().unwrap_or_default().to_string();
            if let (Some(i), Some(o)) = (num_after(t, "inputPer1K:"), num_after(t, "outputPer1K:")) {
                out.push((id, i, o));
            }
        }
        out
    }

    /// Strip the `::effort` suffix. Effort changes how many tokens a turn
    /// spends, not the per-token rate or the provider.
    fn base(id: &str) -> &str {
        id.split_once("::").map_or(id, |(b, _)| b)
    }

    #[test]
    fn the_source_scanners_still_understand_their_files() {
        // Without this, a reformat of either file turns every test below into a
        // vacuous pass over an empty list — the failure mode where a guard
        // reports green precisely because it has stopped looking.
        let cat = catalog();
        assert!(cat.len() > 30, "catalog scan found only {} entries", cat.len());
        assert!(
            cat.iter().any(|(p, _)| p == "anthropic") && cat.iter().any(|(p, _)| p == "deepseek"),
            "catalog scan lost the provider grouping"
        );
        assert!(
            frontend_prices().len() > 20,
            "pricing scan found only {} rows",
            frontend_prices().len()
        );
        assert!(
            quoted_strings_after(LLM_TS, "const CONTEXT_WINDOWS", "\n};").len() > 20,
            "context-window scan lost its map"
        );
        assert_eq!(
            quoted_strings_after(LLM_TS, "export const LLM = Object.freeze({", "} as const)").len(),
            5,
            "the internal tier list changed shape"
        );
    }

    #[test]
    fn every_offered_model_routes_to_the_provider_its_group_claims() {
        for (group, id) in catalog() {
            // The group is named after the daemon, the router after the id
            // prefix. Same destination, different vocabulary.
            let want = if group == "ollama" { "local" } else { group.as_str() };
            let got = provider_for_model(base(&id));
            assert_eq!(
                got, want,
                "'{id}' is offered under provider '{group}' but routes to '{got}'"
            );
        }
    }

    #[test]
    fn the_two_price_tables_agree_on_every_model() {
        for (id, want_in, want_out) in frontend_prices() {
            let (got_in, got_out) = model_prices(&id);
            assert_eq!(
                (got_in, got_out),
                (want_in, want_out),
                "'{id}': model-pricing.ts says {want_in}/{want_out} per 1K, db.rs says {got_in}/{got_out}"
            );
        }
    }

    #[test]
    fn every_offered_model_has_a_real_price_somewhere() {
        // Checking the TS table by NAME is what makes this exact. Checking the
        // Rust by value cannot be: an id missing from model_prices() returns the
        // mid-tier guess, which is a valid-looking pair of numbers. But a model
        // present here and absent there fails the agreement test above, since no
        // real rate equals the (0.001, 0.003) fallback — so name-checking one
        // side pins both.
        let priced: Vec<String> = frontend_prices().into_iter().map(|(id, _, _)| id).collect();

        for (_, id) in catalog() {
            let b = base(&id);
            // NVIDIA NIM is `owner/model`: hundreds of third-party models behind
            // one endpoint, priced by an average in both tables on purpose.
            // The two `*-custom` rows are placeholders the user overwrites with
            // their own id, so there is nothing to price until they do.
            if b.contains('/') || b == "nvidia-custom" || b.starts_with("local-") {
                continue;
            }
            assert!(
                priced.iter().any(|p| p == b),
                "'{id}' is offered in the dropdown but has no entry in model-pricing.ts, \
                 so every turn on it is billed at the generic fallback rate"
            );
        }
    }

    #[test]
    fn every_offered_model_is_on_the_backend_whitelist() {
        // `ask_lucy` and `ask_lucy_stream` reject anything not in
        // ALLOWED_MODELS, matching the WHOLE string — `::effort` suffix
        // included, so every suffix needs its own row. A model in the dropdown
        // and not on that list is not degraded, it is dead: "Modelo 'x' no
        // permitido. Selecciona un modelo válido desde el selector", from the
        // selector.
        //
        // llm-models.ts already carries the warning — "keep these two files in
        // sync — adding a new id here without registering it there will produce
        // 'unauthorized model' errors at runtime" — which is exactly what
        // happened three commits in a row anyway. A warning addressed to
        // whoever edits the file next cannot be read by the person who edits a
        // different file.
        for (_, id) in catalog() {
            // Admitted by shape rather than by name: NVIDIA NIM is
            // `owner/model` for hundreds of third-party models, and Ollama ids
            // depend on what the user has pulled locally.
            if id.contains('/') || id.starts_with("local-") {
                continue;
            }
            // The one deliberate exception: the NVIDIA placeholder is meant to
            // be refused until the user types a real `owner/model` over it.
            if id == "nvidia-custom" {
                continue;
            }
            assert!(
                crate::state::ALLOWED_MODELS.contains(&id.as_str()),
                "'{id}' is in the dropdown but not in ALLOWED_MODELS — picking it \
                 returns 'Modelo no permitido' and the entry is unusable"
            );
        }
    }

    #[test]
    fn the_internal_tiers_are_models_the_backend_accepts() {
        // llm-models.ts fixes the models Lucy picks for ITSELF — autotag, the
        // smart filter, script verification, memory summarisation. The user
        // never selects these, so a broken tier does not show up as "that model
        // is broken"; it shows up as a feature that stopped working, weeks
        // later, in a view nobody connects to a model id.
        //
        // This is the exact sync that file's header has warned about since
        // v1.7.0. It is now checked instead of requested.
        for id in quoted_strings_after(LLM_TS, "export const LLM = Object.freeze({", "} as const)") {
            assert!(
                crate::state::ALLOWED_MODELS.contains(&id.as_str()),
                "llm-models.ts routes an internal tier to '{id}', which ALLOWED_MODELS rejects"
            );
        }
        for id in quoted_strings_after(LLM_TS, "KNOWN_GEMINI_IDS", "]);") {
            assert!(
                crate::state::ALLOWED_MODELS.contains(&id.as_str()),
                "KNOWN_GEMINI_IDS lists '{id}', which ALLOWED_MODELS rejects"
            );
        }
    }

    #[test]
    fn every_offered_model_has_a_context_window() {
        // CONTEXT_WINDOWS feeds the `used/max` chip in the Context Strip and
        // falls back to 128k. Its own comment spells out the consequence: on a
        // 1M-context model that fallback makes a normal session look nearly
        // full, so the chip turns red and the user starts compacting a
        // conversation that was using 3% of its budget.
        let mapped = quoted_strings_after(LLM_TS, "const CONTEXT_WINDOWS", "\n};");
        for (_, id) in catalog() {
            // Same two shape-based exclusions as everywhere else: NIM is
            // `owner/model` and Ollama depends on what the user has pulled.
            if id.contains('/') || id.starts_with("local-") || id == "nvidia-custom" {
                continue;
            }
            let b = base(&id);
            assert!(
                mapped.iter().any(|m| m == b || m == &id),
                "'{id}' is offered in the dropdown but missing from CONTEXT_WINDOWS, \
                 so its token chip is denominated by the 128k fallback"
            );
        }
    }

    #[test]
    fn every_offered_effort_level_survives_its_resolver() {
        // Both resolvers strip an effort they do not recognise instead of
        // erroring, which is the right call for a stray suffix in a saved chat
        // from two lineups ago. It is the wrong outcome for an entry Lucy puts
        // in its own dropdown: the request goes out at the model's default, the
        // answer comes back looking fine, and the only trace is a bill for
        // reasoning that never happened. Nothing distinguishes the two cases at
        // runtime — the dropdown has to be right up front.
        //
        // Anthropic's whitelist is per-model and genuinely irregular: Sonnet 4.6
        // takes max but not xhigh, Opus 4.5 takes neither, Haiku takes no effort
        // at all. That is the shape of thing that gets one row wrong.
        for (_, id) in catalog() {
            if !id.contains("::") {
                continue;
            }
            let b = base(&id);
            if b.starts_with("claude-") {
                let (got_base, effort) = resolve_anthropic_model(&id);
                assert_eq!(got_base, b, "'{id}' resolved to the wrong base model");
                assert!(
                    effort.is_some(),
                    "the dropdown offers '{id}', but resolve_anthropic_model() drops that \
                     effort for this model — it would run at the default and bill as if not"
                );
            } else if b.starts_with("gemini-") {
                let (got_base, cfg) = resolve_gemini_model(&id);
                assert_eq!(got_base, b, "'{id}' resolved to the wrong base model");
                assert!(
                    cfg.is_some(),
                    "the dropdown offers '{id}', but resolve_gemini_model() drops that \
                     thinkingLevel — Google would pick its own budget instead"
                );
            } else {
                panic!("'{id}' carries an ::effort suffix but no resolver handles its provider");
            }
        }
    }
}

#[cfg(test)]
mod pricing_tests {
    use super::{calculate_cost, model_prices};

    // The previous test here asserted "opus is 5× sonnet" and passed for as
    // long as the Opus rate sat at Opus 4.5's $15/$75 — 3× what 4.6 onward
    // actually cost. A ratio cannot catch that: both sides can drift together
    // and the assertion still holds. These pin ABSOLUTE per-1K rates, so a
    // wrong number fails on the number that is wrong.
    #[test]
    fn anthropic_rates_match_the_published_price_list() {
        // USD per 1K tokens = published per-1M price / 1000.
        for (model, want_in, want_out) in [
            ("claude-fable-5",    0.010,  0.050),   // $10 / $50 per 1M
            ("claude-opus-5",     0.005,  0.025),   // $5  / $25
            ("claude-opus-4-8",   0.005,  0.025),
            ("claude-opus-4-5",   0.015,  0.075),   // legacy tier — correct for 4.5
            ("claude-sonnet-5",   0.003,  0.015),   // $3  / $15 (intro $2/$10 not quoted)
            ("claude-sonnet-4-6", 0.003,  0.015),
            ("claude-haiku-4-5",  0.001,  0.005),   // $1  / $5
        ] {
            let (got_in, got_out) = model_prices(model);
            assert!((got_in - want_in).abs() < 1e-9,
                "{} input: got {} want {}", model, got_in, want_in);
            assert!((got_out - want_out).abs() < 1e-9,
                "{} output: got {} want {}", model, got_out, want_out);
        }
    }

    #[test]
    fn opus_5_costs_less_than_the_4_5_tier_it_replaced() {
        // Guards the direction of the correction: the Opus tier got CHEAPER at
        // 4.6, and re-inheriting 4.5's numbers is the regression to catch.
        let now = calculate_cost("claude-opus-5", 1000, 1000);
        let old_tier = calculate_cost("claude-opus-4-5", 1000, 1000);
        assert!(now < old_tier, "opus-5={} opus-4-5={}", now, old_tier);
    }

    #[test]
    fn effort_suffix_is_stripped_for_pricing() {
        let plain  = calculate_cost("claude-opus-4-7",         1000, 1000);
        let xhigh  = calculate_cost("claude-opus-4-7::xhigh",  1000, 1000);
        let max    = calculate_cost("claude-opus-4-7::max",    1000, 1000);
        // Same per-token price regardless of effort — effort affects token COUNT,
        // not the rate. The frontend predictor multiplies by iterFactor separately.
        assert!((plain - xhigh).abs() < 1e-9);
        assert!((plain - max).abs() < 1e-9);
    }

    // This used to assert "Flash costs ≤1/3 of Pro". That stopped being true:
    // the Flash tier moved upmarket — gemini-3.5-flash is described by Google
    // as its most intelligent model for sustained frontier work and is priced
    // at $1.50/$9.00 against Pro's $2.00/$12.00, roughly three quarters of it.
    // The genuinely cheap tier is now Flash-LITE. Naming matters here: someone
    // reaching for "flash" expecting the budget option is picking a model that
    // costs 6× what they think it does.
    #[test]
    fn flash_lite_is_the_cheap_tier_not_flash() {
        let pro        = model_prices("gemini-3.1-pro-preview");
        let flash      = model_prices("gemini-3.5-flash");
        let flash_lite = model_prices("gemini-3.1-flash-lite");

        // Flash-Lite is the budget tier — comfortably under a third of Pro.
        assert!(pro.0 > flash_lite.0 * 3.0, "Pro input should cost >3× Flash-Lite");
        assert!(pro.1 > flash_lite.1 * 3.0, "Pro output should cost >3× Flash-Lite");

        // Flash sits just below Pro, NOT near Flash-Lite. If this ever starts
        // failing because Flash got cheap again, that is a real repricing —
        // check the tier ordering in models.js at the same time.
        assert!(flash.0 < pro.0,             "Flash should still be under Pro");
        assert!(flash.0 > flash_lite.0 * 3.0, "Flash is NOT the budget tier");
    }

    #[test]
    fn local_and_unknown_models_handled() {
        assert_eq!(model_prices("local-qwen2.5:7b"), (0.0, 0.0));
        let nv = model_prices("microsoft/phi-3-mini-4k-instruct");
        assert!(nv.0 > 0.0 && nv.1 > 0.0);
        let unknown = model_prices("totally-fake-model-9000");
        // Fallback to mid-tier, not zero — so we still log a cost estimate.
        assert!(unknown.0 > 0.0 && unknown.1 > 0.0);
    }

    #[test]
    fn legacy_gemini_3_flash_preview_uses_3_5_flash_price() {
        // The id stays "gemini-3-flash-preview" in old logs, but charges 3.5 rate.
        let legacy = model_prices("gemini-3-flash-preview");
        let current = model_prices("gemini-3.5-flash");
        assert_eq!(legacy, current);
    }
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
