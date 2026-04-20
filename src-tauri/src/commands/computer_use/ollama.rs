// ── Local Ollama Computer Use Provider ────────────────────────────────────────
//
// Uses local Ollama models (llava, etc.) with vision capabilities.
// Communicates via the Ollama HTTP API (default: localhost:11434).

use async_trait::async_trait;
use serde_json::{json, Value};
use crate::state::HTTP_CLIENT;
use crate::commands::computer_use::types::{ComputerAction, ComputeConfig};
use crate::commands::computer_use::traits::ComputerUseProvider;

pub struct OllamaProvider;

impl OllamaProvider {
    pub fn new() -> Self {
        Self
    }

    fn get_endpoint() -> String {
        std::env::var("OLLAMA_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:11434".into())
    }
}

/// Normalize common LLaVA/Qwen hallucinations into the strict schema.
/// Returns None if the item can't be salvaged.
fn normalize_hallucinated_action(item: &Value) -> Option<Value> {
    let obj = item.as_object()?;
    let action = obj.get("action").and_then(|v| v.as_str())?.to_lowercase();

    // Coordinate may come as "coordinate", "coordinates", "coord", "position", "xy"
    let coord = obj.get("coordinate")
        .or_else(|| obj.get("coordinates"))
        .or_else(|| obj.get("coord"))
        .or_else(|| obj.get("position"))
        .or_else(|| obj.get("xy"))
        .cloned();

    // Text may come as "text", "term", "query", "value", "input"
    let text = obj.get("text")
        .or_else(|| obj.get("term"))
        .or_else(|| obj.get("query"))
        .or_else(|| obj.get("value"))
        .or_else(|| obj.get("input"))
        .and_then(|v| v.as_str())
        .map(String::from);

    match action.as_str() {
        "click" | "press" | "tap" | "select" => {
            // Only salvageable if a coordinate was provided.
            coord.map(|c| json!({"action": "left_click", "coordinate": c}))
        }
        "double_click" | "doubleclick" | "dblclick" => {
            coord.map(|c| json!({"action": "double_click", "coordinate": c}))
        }
        "right_click" | "rightclick" | "context_menu" => {
            coord.map(|c| json!({"action": "right_click", "coordinate": c}))
        }
        "move" | "hover" => {
            coord.map(|c| json!({"action": "mouse_move", "coordinate": c}))
        }
        "write" | "input" | "enter_text" => {
            text.map(|t| json!({"action": "type", "text": t}))
        }
        "keypress" | "hotkey" | "shortcut" => {
            text.map(|t| json!({"action": "key", "text": t}))
        }
        // "search" is a composite — best we can do: type the term (user will still need a click first)
        "search" | "find" | "lookup" => {
            text.map(|t| json!({"action": "type", "text": t}))
        }
        _ => None,
    }
}

#[async_trait]
impl ComputerUseProvider for OllamaProvider {
    fn name(&self) -> &str {
        "Local Ollama (Vision)"
    }

    /// Override default parser: try strict first, then salvage hallucinations,
    /// and strip markdown code fences that small models love to add.
    fn parse_actions(&self, response: &str) -> Result<Vec<ComputerAction>, String> {
        // Strip Qwen/Llama chat-template tokens that sometimes leak through Ollama.
        let mut cleaned = response.to_string();
        for tok in &[
            "<|im_start|>", "<|im_end|>", "<|endoftext|>",
            "<|eot_id|>", "<|start_header_id|>", "<|end_header_id|>",
            "assistant\n", "user\n", "system\n",
        ] {
            cleaned = cleaned.replace(tok, "");
        }
        // Strip ```json ... ``` fences.
        let cleaned = cleaned.trim();
        let cleaned = cleaned.trim_start_matches("```json").trim_start_matches("```");
        let cleaned = cleaned.trim_end_matches("```").trim();

        // Try to extract the first JSON array anywhere in the text.
        let json_slice = if let (Some(start), Some(end)) = (cleaned.find('['), cleaned.rfind(']')) {
            if end > start { &cleaned[start..=end] } else { cleaned }
        } else {
            cleaned
        };

        let parsed: Value = serde_json::from_str(json_slice)
            .map_err(|e| format!("Ollama JSON parse failed: {} — raw: {}", e, cleaned))?;

        let arr = parsed.as_array()
            .ok_or_else(|| "Ollama response is not a JSON array".to_string())?;

        let mut actions = Vec::new();
        for item in arr {
            // Fast path: already-valid strict schema
            if let Some(a) = ComputerAction::from_json(item) {
                actions.push(a);
                continue;
            }
            // Salvage path: rewrite hallucinated fields, then retry
            if let Some(fixed) = normalize_hallucinated_action(item) {
                if let Some(a) = ComputerAction::from_json(&fixed) {
                    actions.push(a);
                }
            }
        }

        if actions.is_empty() {
            return Err(format!(
                "Ollama returned no usable actions. Model likely lacks GUI grounding. Raw: {}",
                cleaned
            ));
        }
        Ok(actions)
    }

    fn get_model(&self) -> String {
        "llava".into()
    }

    async fn check_credentials(&self) -> Result<(), String> {
        let endpoint = Self::get_endpoint();
        let resp = HTTP_CLIENT
            .get(&format!("{}/api/tags", endpoint))
            .send()
            .await;

        match resp {
            Ok(_) => Ok(()),
            Err(_) => Err(format!(
                "Ollama not running at {}. Start Ollama or set OLLAMA_ENDPOINT env var.",
                endpoint
            )),
        }
    }

    async fn query_llm(
        &self,
        config: &ComputeConfig,
        screenshot_b64: &str,
        messages: &[Value],
    ) -> Result<(Vec<ComputerAction>, String, bool), String> {
        let endpoint = Self::get_endpoint();

        // Build message history for Ollama format
        let mut ollama_messages = Vec::new();

        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let mut content_text = String::new();
            let mut images = Vec::new();

            if let Some(msg_content) = msg.get("content") {
                if let Some(arr) = msg_content.as_array() {
                    for item in arr {
                        match item.get("type").and_then(|t| t.as_str()) {
                            Some("text") => {
                                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                    content_text = text.to_string();
                                }
                            }
                            Some("image") => {
                                if let Some(data) = item.get("source")
                                    .and_then(|s| s.get("data"))
                                    .and_then(|d| d.as_str())
                                {
                                    images.push(data.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            if !content_text.is_empty() || !images.is_empty() {
                let mut msg_obj = json!({
                    "role": role,
                    "content": content_text
                });
                if !images.is_empty() {
                    msg_obj["images"] = json!(images);
                }
                ollama_messages.push(msg_obj);
            }
        }

        // Add current screenshot — strict schema prompt, no room for invented fields.
        let prompt = format!(
            "You are controlling a Windows Remote Desktop. Screen size: {width}x{height} pixels \
             (top-left = [0,0], bottom-right = [{width},{height}]).\n\
             \n\
             TASK: {task}\n\
             \n\
             Look at the screenshot and output ONE JSON array with the NEXT actions to execute. \
             Return ONLY valid JSON. No markdown, no prose, no code fences.\n\
             \n\
             ALLOWED ACTIONS (strict schema — any other field name will be rejected):\n\
               {{\"action\":\"left_click\",\"coordinate\":[X,Y]}}      — single click at pixel X,Y\n\
               {{\"action\":\"double_click\",\"coordinate\":[X,Y]}}    — double click\n\
               {{\"action\":\"right_click\",\"coordinate\":[X,Y]}}     — right click\n\
               {{\"action\":\"mouse_move\",\"coordinate\":[X,Y]}}      — move cursor only\n\
               {{\"action\":\"type\",\"text\":\"hello\"}}              — type text at current focus\n\
               {{\"action\":\"key\",\"text\":\"Return\"}}              — press a key (Return, Tab, ctrl+a, alt+F4…)\n\
               {{\"action\":\"screenshot\"}}                           — request another screenshot\n\
             \n\
             RULES:\n\
              - coordinate MUST be exact pixel numbers [X,Y] within 0..{width} x 0..{height}.\n\
              - NEVER use fields like \"target\", \"term\", \"element\", \"label\", \"description\".\n\
              - NEVER use actions like \"search\", \"click\" (use left_click), \"press\", \"open\".\n\
              - To open an app: left_click its icon, then type its name, then key Return.\n\
              - To search Google: left_click the address bar, type the query, key Return.\n\
              - If the task is already done, return [] (empty array).\n\
             \n\
             EXAMPLE OUTPUT:\n\
               [{{\"action\":\"left_click\",\"coordinate\":[412,738]}},{{\"action\":\"type\",\"text\":\"edge\"}},{{\"action\":\"key\",\"text\":\"Return\"}}]",
            width = config.window_width,
            height = config.window_height,
            task = config.task
        );

        ollama_messages.push(json!({
            "role": "user",
            "content": prompt,
            "images": [screenshot_b64]
        }));

        // Strip "local-" UI prefix so Ollama receives the real tag (e.g. "llava").
        let ollama_model = config.model.strip_prefix("local-").unwrap_or(&config.model);

        let body = json!({
            "model": ollama_model,
            "messages": ollama_messages,
            "stream": false,
            "options": {
                // Lower temperature → more deterministic JSON, fewer hallucinated fields.
                "temperature": 0.2,
                "top_p": 0.8,
                // Stop as soon as the model tries to start a new chat turn — this is the
                // root cause of the "<|im_start|>" leak seen with Qwen2.5-VL on Ollama.
                // NOTE: do NOT include "```" here — Qwen often opens with ```json
                // and that stop token would kill generation before any JSON is emitted.
                // The parser already strips code fences after the fact.
                "stop": [
                    "<|im_start|>", "<|im_end|>", "<|endoftext|>",
                    "<|eot_id|>", "<|start_header_id|>", "<|end_header_id|>"
                ],
                // Cap output so we don't wait forever if the model rambles.
                "num_predict": 512,
            }
        });

        // ── First attempt ─────────────────────────────────────────────────────
        async fn call_ollama(endpoint: &str, body: &Value) -> Result<String, String> {
            let resp = HTTP_CLIENT
                .post(&format!("{}/api/chat", endpoint))
                .header("content-type", "application/json")
                .json(body)
                .send()
                .await
                .map_err(|e| format!("Ollama connection error: {}", e))?;
            let resp_json: Value = resp.json().await
                .map_err(|e| format!("Ollama JSON parse: {}", e))?;
            Ok(resp_json.get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string())
        }

        let mut text_response = call_ollama(&endpoint, &body).await?;

        // ── Retry once with a stricter nudge if the first response is empty
        //    or can't be parsed. Helps when Qwen got cut by a stop token or
        //    emitted prose instead of JSON.
        if text_response.trim().is_empty() || self.parse_actions(&text_response).is_err() {
            eprintln!("[ollama] first response unparseable ({} chars) → retrying with nudge", text_response.len());
            let nudge = json!({
                "role": "user",
                "content": "Your previous response could not be parsed. Output ONLY a raw JSON \
                            array of actions. No markdown, no prose, no code fences, no chat tokens. \
                            Just: [{\"action\":\"...\",\"coordinate\":[X,Y]}]. \
                            If there's nothing to do, output []."
            });
            let mut retry_messages = ollama_messages.clone();
            retry_messages.push(json!({
                "role": "assistant",
                "content": if text_response.trim().is_empty() { "(empty)" } else { text_response.as_str() },
            }));
            retry_messages.push(nudge);

            let retry_body = json!({
                "model": ollama_model,
                "messages": retry_messages,
                "stream": false,
                "options": {
                    "temperature": 0.1,
                    "top_p": 0.7,
                    "stop": ["<|im_start|>", "<|im_end|>", "<|endoftext|>", "<|eot_id|>"],
                    "num_predict": 384,
                }
            });

            match call_ollama(&endpoint, &retry_body).await {
                Ok(retry_text) if !retry_text.trim().is_empty() => {
                    text_response = retry_text;
                }
                _ => { /* keep original, parser will decide below */ }
            }
        }

        // Parse actions from response. Distinguish "model said done" from "parser failed".
        let (actions, should_stop, effective_text) = match self.parse_actions(&text_response) {
            Ok(a) => {
                let done = a.is_empty() || text_response.to_lowercase().contains("task complete");
                (a, done, text_response)
            }
            Err(parse_err) => {
                // Parse failed → don't silently quit. Emit a diagnostic and let the
                // agent loop continue with a fresh screenshot (should_stop = false
                // with empty actions means the outer loop will end; so we instead
                // give up gracefully with an informative message).
                eprintln!("[ollama] parse_actions failed: {}", parse_err);
                (
                    Vec::new(),
                    true, // end the loop but with a clear explanation
                    format!(
                        "⚠️ Local model returned unparseable output (chat-template leak or non-JSON). \
                         Raw response:\n{}\n\nTry a stronger grounded model (qwen2.5vl:7b) \
                         or switch to Claude for this task.",
                        text_response
                    ),
                )
            }
        };

        Ok((actions, effective_text, should_stop))
    }
}
