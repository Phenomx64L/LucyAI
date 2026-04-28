// ── Shared Types for all Computer Use Providers ───────────────────────────────

use serde::{Deserialize, Serialize};

/// Unified action format — all providers emit these after parsing their responses
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum ComputerAction {
    #[serde(rename = "screenshot")]
    Screenshot,

    #[serde(rename = "left_click")]
    LeftClick { coordinate: [i32; 2] },

    #[serde(rename = "right_click")]
    RightClick { coordinate: [i32; 2] },

    #[serde(rename = "double_click")]
    DoubleClick { coordinate: [i32; 2] },

    #[serde(rename = "middle_click")]
    MiddleClick { coordinate: [i32; 2] },

    #[serde(rename = "mouse_move")]
    MouseMove { coordinate: [i32; 2] },

    #[serde(rename = "left_click_drag")]
    LeftClickDrag {
        start_coordinate: [i32; 2],
        end_coordinate: [i32; 2],
    },

    #[serde(rename = "type")]
    Type { text: String },

    #[serde(rename = "key")]
    Key { text: String },

    #[serde(rename = "cursor_position")]
    CursorPosition,
}

impl ComputerAction {
    /// Parse from JSON (for providers that use JSON schema)
    pub fn from_json(json: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(json.clone()).ok()
    }

    /// Convert to JSON
    #[allow(dead_code)]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::json!({}))
    }
}

/// Result from executing an action
#[allow(dead_code)]   // reserved for upcoming computer_use providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    /// Base64-encoded PNG screenshot (if applicable)
    pub screenshot: Option<String>,
    /// Text result or error message
    pub message: String,
    /// Whether the action succeeded
    pub success: bool,
}

/// Configuration for a Computer Use session
#[derive(Debug, Clone)]
pub struct ComputeConfig {
    pub model: String,
    #[allow(dead_code)]
    pub api_key: String,
    pub task: String,
    #[allow(dead_code)]
    pub max_steps: u32,
    pub window_width: i32,
    pub window_height: i32,
}

/// Message turn in the conversation
#[allow(dead_code)]   // reserved for upcoming computer_use providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTurn {
    pub role: String, // "user" or "assistant"
    pub content: serde_json::Value,
}
