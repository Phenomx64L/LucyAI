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

-- Create indexes for fast queries
CREATE INDEX IF NOT EXISTS idx_token_usage_timestamp ON token_usage(timestamp);
CREATE INDEX IF NOT EXISTS idx_token_usage_model ON token_usage(model);
CREATE INDEX IF NOT EXISTS idx_permission_rules_enabled ON permission_rules(enabled, priority);
CREATE INDEX IF NOT EXISTS idx_skills_name ON skills(name);
CREATE INDEX IF NOT EXISTS idx_skills_category ON skills(category);
"#;

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
