// ── Google Gemini Vision Computer Use Provider ────────────────────────────────
//
// Uses Gemini's vision API with structured JSON prompts to simulate Computer Use.
// Since Gemini doesn't have a native Computer Use tool, we instruct it to generate
// JSON-formatted action sequences.

use async_trait::async_trait;
use serde_json::{json, Value};
use keyring::Entry;
use crate::state::HTTP_CLIENT;
use crate::commands::computer_use::types::{ComputerAction, ComputeConfig};
use crate::commands::computer_use::traits::ComputerUseProvider;

pub struct GeminiProvider;

impl GeminiProvider {
    pub fn new() -> Self {
        Self
    }
}

/// Resolve the caller-selected model id into the real Gemini API model name.
/// Strips any "::effort" suffix, upgrades the legacy alias, and falls back to a
/// known vision-capable model if the selection isn't a Gemini id (so we never
/// POST to a non-existent `:generateContent` endpoint and hang/error).
fn resolve_gemini_model(raw: &str) -> String {
    let base = raw.split("::").next().unwrap_or(raw).trim();
    let base = if base == "gemini-3-flash-preview" { "gemini-3.5-flash" } else { base };
    if base.starts_with("gemini") && !base.is_empty() {
        base.to_string()
    } else {
        "gemini-2.5-flash".to_string()
    }
}

#[async_trait]
impl ComputerUseProvider for GeminiProvider {
    fn name(&self) -> &str {
        "Google Gemini Vision"
    }

    fn get_model(&self) -> String {
        "gemini-2.0-flash".into()
    }

    async fn check_credentials(&self) -> Result<(), String> {
        Entry::new("LucySysAdmin", "gemini_api_key")
            .and_then(|e| e.get_password())
            .map_err(|_| "Gemini API key not configured (Settings → Providers)".to_string())
            .map(|_| ())
    }

    async fn query_llm(
        &self,
        config: &ComputeConfig,
        screenshot_b64: &str,
        messages: &[Value],
    ) -> Result<(Vec<ComputerAction>, String, bool), String> {
        let api_key = Entry::new("LucySysAdmin", "gemini_api_key")
            .and_then(|e| e.get_password())
            .map_err(|_| "Gemini API key not configured".to_string())?;

        // Build Gemini request with vision content
        let mut contents = Vec::new();

        // Convert message history to Gemini format
        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let gemini_role = if role == "assistant" { "model" } else { "user" };

            if let Some(content_arr) = msg.get("content").and_then(|c| c.as_array()) {
                let mut parts = Vec::new();

                for item in content_arr {
                    match item.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                parts.push(json!({
                                    "text": text
                                }));
                            }
                        }
                        Some("image") => {
                            if let Some(data) = item.get("source")
                                .and_then(|s| s.get("data"))
                                .and_then(|d| d.as_str())
                            {
                                parts.push(json!({
                                    "inline_data": {
                                        "mime_type": "image/png",
                                        "data": data
                                    }
                                }));
                            }
                        }
                        _ => {}
                    }
                }

                if !parts.is_empty() {
                    contents.push(json!({
                        "role": gemini_role,
                        "parts": parts
                    }));
                }
            }
        }

        // Add current screenshot if not already in last message
        if let Some(last) = contents.last() {
            if last.get("role").and_then(|r| r.as_str()) != Some("user") {
                contents.push(json!({
                    "role": "user",
                    "parts": [{
                        "inline_data": {
                            "mime_type": "image/png",
                            "data": screenshot_b64
                        }
                    }]
                }));
            }
        }

        let system_prompt = format!(
            "{}\n\n\
            Display size: {}x{} pixels.\n\
            Task: {}\n\n\
            Analyze the screenshot and generate a JSON array of actions to execute. \
            Return ONLY valid JSON, no markdown or extra text.",
            self.system_prompt(),
            config.window_width,
            config.window_height,
            config.task
        );

        let body = json!({
            "contents": contents,
            "system_instruction": {
                "parts": [{
                    "text": system_prompt
                }]
            },
            "generation_config": {
                "temperature": 0.7,
                "max_output_tokens": 1024,
                "response_mime_type": "application/json"
            }
        });

        let api_model = resolve_gemini_model(&config.model);
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            api_model, api_key
        );

        let resp = HTTP_CLIENT
            .post(&url)
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
                return Err(format!("Gemini API error: {}", msg));
            }
        }

        // Extract response content
        let mut text_response = String::new();
        let mut actions = Vec::new();

        if let Some(candidates) = resp_json.get("candidates").and_then(|c| c.as_array()) {
            if let Some(first) = candidates.first() {
                if let Some(content) = first.get("content") {
                    if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                        for part in parts {
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                text_response = text.to_string();

                                // Parse JSON actions
                                if let Ok(parsed_actions) = self.parse_actions(text) {
                                    actions = parsed_actions;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Stop only when there's genuinely nothing left to do. The old
        // substring check on `text_response` ("done"/"complete") was unsafe
        // here: with response_mime_type=application/json the text IS the action
        // JSON, so coordinates/keys could trip a false stop. Empty actions =
        // the model has nothing more to perform → end the turn.
        let should_stop = actions.is_empty();

        Ok((actions, text_response, should_stop))
    }
}
