// ── OpenAI GPT-4V/4o Computer Use Provider ────────────────────────────────────
//
// Uses GPT-4 Vision with structured JSON prompts to simulate Computer Use.
// Similar to Gemini, we generate JSON-formatted action sequences.

use async_trait::async_trait;
use serde_json::{json, Value};
use keyring::Entry;
use crate::state::HTTP_CLIENT;
use crate::commands::computer_use::types::{ComputerAction, ComputeConfig};
use crate::commands::computer_use::traits::ComputerUseProvider;

pub struct OpenAiProvider;

impl OpenAiProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ComputerUseProvider for OpenAiProvider {
    fn name(&self) -> &str {
        "OpenAI GPT-4 Vision"
    }

    fn get_model(&self) -> String {
        "gpt-4o".into()
    }

    async fn check_credentials(&self) -> Result<(), String> {
        Entry::new("LucySysAdmin", "openai_api_key")
            .and_then(|e| e.get_password())
            .map_err(|_| "OpenAI API key not configured (Settings → Providers)".to_string())
            .map(|_| ())
    }

    async fn query_llm(
        &self,
        config: &ComputeConfig,
        screenshot_b64: &str,
        messages: &[Value],
    ) -> Result<(Vec<ComputerAction>, String, bool), String> {
        let api_key = Entry::new("LucySysAdmin", "openai_api_key")
            .and_then(|e| e.get_password())
            .map_err(|_| "OpenAI API key not configured".to_string())?;

        // Build OpenAI request with vision content
        let mut api_messages = Vec::new();

        // Convert message history to OpenAI format
        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let mut content = Vec::new();

            if let Some(msg_content) = msg.get("content") {
                if let Some(arr) = msg_content.as_array() {
                    for item in arr {
                        match item.get("type").and_then(|t| t.as_str()) {
                            Some("text") => {
                                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                    content.push(json!({
                                        "type": "text",
                                        "text": text
                                    }));
                                }
                            }
                            Some("image") => {
                                if let Some(data) = item.get("source")
                                    .and_then(|s| s.get("data"))
                                    .and_then(|d| d.as_str())
                                {
                                    content.push(json!({
                                        "type": "image_url",
                                        "image_url": {
                                            "url": format!("data:image/png;base64,{}", data)
                                        }
                                    }));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            if !content.is_empty() {
                api_messages.push(json!({
                    "role": role,
                    "content": content
                }));
            }
        }

        // Add current screenshot as user message
        let user_content = json!([
            {
                "type": "image_url",
                "image_url": {
                    "url": format!("data:image/png;base64,{}", screenshot_b64)
                }
            },
            {
                "type": "text",
                "text": format!(
                    "Display size: {}x{} pixels.\n\
                     Task: {}\n\n\
                     Analyze this screenshot and generate a JSON array of actions to execute next. \
                     Return ONLY valid JSON array, no markdown or extra text.",
                    config.window_width,
                    config.window_height,
                    config.task
                )
            }
        ]);

        api_messages.push(json!({
            "role": "user",
            "content": user_content
        }));

        let body = json!({
            "model": config.model,
            "messages": api_messages,
            "max_tokens": 1024,
            "temperature": 0.7,
        });

        let resp = HTTP_CLIENT
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?;

        let resp_json: Value = resp.json().await
            .map_err(|e| format!("JSON parse: {}", e))?;

        // Check for API errors
        if let Some(error) = resp_json.get("error") {
            if let Some(msg) = error.get("message").and_then(|m| m.as_str()) {
                return Err(format!("OpenAI API error: {}", msg));
            }
        }

        // Extract response content
        let mut text_response = String::new();
        let mut actions = Vec::new();

        if let Some(choices) = resp_json.get("choices").and_then(|c| c.as_array()) {
            if let Some(first) = choices.first() {
                if let Some(msg) = first.get("message") {
                    if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                        text_response = content.to_string();

                        // Try to parse JSON actions
                        if let Ok(parsed_actions) = self.parse_actions(content) {
                            actions = parsed_actions;
                        } else {
                            // Try to extract JSON from markdown code blocks
                            if let Some(json_str) = Self::extract_json_from_markdown(content) {
                                if let Ok(parsed_actions) = self.parse_actions(&json_str) {
                                    actions = parsed_actions;
                                }
                            }
                        }
                    }
                }
            }
        }

        let should_stop = actions.is_empty() || text_response.contains("complete") || text_response.contains("done");

        Ok((actions, text_response, should_stop))
    }
}

impl OpenAiProvider {
    /// Extract JSON from markdown code blocks (```json ... ```)
    fn extract_json_from_markdown(text: &str) -> Option<String> {
        if let Some(start) = text.find("```json") {
            let start = start + 7;
            if let Some(end) = text[start..].find("```") {
                let json_str = text[start..start + end].trim();
                return Some(json_str.to_string());
            }
        }
        if let Some(start) = text.find("```") {
            let start = start + 3;
            if let Some(end) = text[start..].find("```") {
                let json_str = text[start..start + end].trim();
                return Some(json_str.to_string());
            }
        }
        None
    }
}
