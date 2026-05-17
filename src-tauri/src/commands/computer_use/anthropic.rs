// ── Anthropic Claude Computer Use Provider ────────────────────────────────────
//
// Implements the native Anthropic Computer Use API with tool_use format.
// Reference: https://docs.anthropic.com/en/docs/build-a-tool-use-agent

use async_trait::async_trait;
use serde_json::{json, Value};
use keyring::Entry;
use crate::state::HTTP_CLIENT;
use crate::commands::computer_use::types::{ComputerAction, ComputeConfig};
use crate::commands::computer_use::traits::ComputerUseProvider;

pub struct AnthropicProvider {
    #[allow(dead_code)]   // surfaced via trait::get_model()
    model: String,
}

impl AnthropicProvider {
    pub fn new() -> Self {
        Self {
            model: "claude-opus-4-7".into(),
        }
    }
}

#[async_trait]
impl ComputerUseProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "Anthropic Claude"
    }

    fn get_model(&self) -> String {
        self.model.clone()
    }

    async fn check_credentials(&self) -> Result<(), String> {
        Entry::new("LucySysAdmin", "anthropic_api_key")
            .and_then(|e| e.get_password())
            .map_err(|_| "Anthropic API key not configured".to_string())
            .map(|_| ())
    }

    async fn query_llm(
        &self,
        config: &ComputeConfig,
        _screenshot_b64: &str,
        messages: &[Value],
    ) -> Result<(Vec<ComputerAction>, String, bool), String> {
        let api_key = Entry::new("LucySysAdmin", "anthropic_api_key")
            .and_then(|e| e.get_password())
            .map_err(|_| "Anthropic API key not configured".to_string())?;

        // Determine tool type and beta header based on model version
        let (tool_type, beta_hdr) = if config.model.contains("3-5") || config.model.contains("3.5") {
            ("computer_20241022", Some("computer-use-2024-10-22"))
        } else if config.model.contains("3-7") || config.model.contains("3.7") {
            ("computer_20250124", Some("computer-use-2025-01-24"))
        } else {
            ("computer_20250124", Some("computer-use-2025-01-24"))
        };

        let tools = json!([{
            "type": tool_type,
            "name": "computer",
            "display_width_px":  config.window_width,
            "display_height_px": config.window_height,
            "display_number": 1
        }]);

        let body = json!({
            "model":      config.model,
            "max_tokens": 4096,
            "tools":      tools,
            "messages":   messages,
        });

        let mut req = HTTP_CLIENT
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key",           &api_key)
            .header("anthropic-version",   "2023-06-01")
            .header("content-type",        "application/json")
            .json(&body);

        if let Some(bh) = beta_hdr {
            req = req.header("anthropic-beta", bh);
        }

        let resp = req.send().await
            .map_err(|e| format!("HTTP error: {}", e))?;

        let resp_json: Value = resp.json().await
            .map_err(|e| format!("JSON parse: {}", e))?;

        // Check for API errors
        if let Some(err_msg) = resp_json["error"]["message"].as_str() {
            return Err(format!("Anthropic API error: {}", err_msg));
        }

        let stop_reason = resp_json["stop_reason"].as_str().unwrap_or("");
        let content = resp_json["content"].as_array().cloned().unwrap_or_default();

        // Extract text response
        let mut text_response = String::new();
        for block in &content {
            if block["type"] == "text" {
                if let Some(txt) = block["text"].as_str() {
                    text_response = txt.trim().to_string();
                    break;
                }
            }
        }

        // Extract tool_use blocks and convert to ComputerAction
        let tool_uses: Vec<&Value> = content.iter()
            .filter(|b| b["type"] == "tool_use")
            .collect();

        let has_tool_uses = !tool_uses.is_empty();
        let mut actions = Vec::new();
        for tu in tool_uses {
            if let Some(input) = tu.get("input") {
                // Convert Anthropic tool_use input to ComputerAction
                if let Some(action_str) = input.get("action").and_then(|a| a.as_str()) {
                    let action = match action_str {
                        "screenshot" => ComputerAction::Screenshot,
                        "left_click" => {
                            let coord = Self::extract_coord(input);
                            ComputerAction::LeftClick { coordinate: coord }
                        }
                        "right_click" => {
                            let coord = Self::extract_coord(input);
                            ComputerAction::RightClick { coordinate: coord }
                        }
                        "double_click" => {
                            let coord = Self::extract_coord(input);
                            ComputerAction::DoubleClick { coordinate: coord }
                        }
                        "left_click_drag" => {
                            let start = Self::extract_array_coord(input, "start_coordinate");
                            let end = Self::extract_array_coord(input, "end_coordinate");
                            ComputerAction::LeftClickDrag {
                                start_coordinate: start,
                                end_coordinate: end,
                            }
                        }
                        "type" => {
                            let text = input.get("text")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .to_string();
                            ComputerAction::Type { text }
                        }
                        "key" => {
                            let text = input.get("text")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .to_string();
                            ComputerAction::Key { text }
                        }
                        _ => continue,
                    };
                    actions.push(action);
                }
            }
        }

        let should_stop = stop_reason == "end_turn" || !has_tool_uses;

        Ok((actions, text_response, should_stop))
    }
}

impl AnthropicProvider {
    fn extract_coord(input: &Value) -> [i32; 2] {
        let coord = input.get("coordinate")
            .and_then(|c| c.as_array())
            .map(|arr| [
                arr.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                arr.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            ])
            .unwrap_or([0, 0]);
        coord
    }

    fn extract_array_coord(input: &Value, key: &str) -> [i32; 2] {
        input.get(key)
            .and_then(|c| c.as_array())
            .map(|arr| [
                arr.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                arr.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            ])
            .unwrap_or([0, 0])
    }
}
