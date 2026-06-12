// ── ComputerUseProvider Trait — Common Interface ────────────────────────────────

use async_trait::async_trait;
use serde_json::Value;
use crate::commands::computer_use::types::{ComputerAction, ComputeConfig};

/// Common interface for all Computer Use providers
#[allow(dead_code)]   // optional methods reserved for future provider routing
#[async_trait]
pub trait ComputerUseProvider: Send + Sync {
    /// Get the provider name (for logging/debugging)
    fn name(&self) -> &str;

    /// Get the model ID this provider handles
    fn get_model(&self) -> String {
        "unknown".into()
    }

    /// Send a request to the LLM and get back a list of actions to execute.
    ///
    /// Receives:
    ///   - screenshot (base64 PNG)
    ///   - task description
    ///   - conversation history (messages so far)
    ///   - window dimensions
    ///
    /// Returns:
    ///   - List of ComputerAction to execute
    ///   - Assistant's text response (for logging)
    ///   - Whether to stop (end_turn equivalent)
    async fn query_llm(
        &self,
        config: &ComputeConfig,
        screenshot_b64: &str,
        messages: &[Value],
    ) -> Result<(Vec<ComputerAction>, String, bool), String>;

    /// Parse the LLM response and extract actions.
    /// Default implementation for JSON-based providers.
    ///
    /// Tolerant by design — vision models wrap their output in inconsistent
    /// shapes. We accept a bare array `[ {..}, {..} ]`, an object wrapper
    /// `{"actions":[ .. ]}`, a single action object `{ "action": ".." }`, and
    /// strip Markdown ```json fences. A strict parser here silently produced
    /// zero actions (→ the agent "did nothing") whenever the model deviated.
    fn parse_actions(&self, response: &str) -> Result<Vec<ComputerAction>, String> {
        let cleaned = strip_code_fences(response);
        let json: Value = serde_json::from_str(cleaned.trim())
            .map_err(|e| format!("Respuesta no es JSON válido: {}", e))?;

        let items: Vec<Value> = if let Some(arr) = json.as_array() {
            arr.clone()
        } else if let Some(arr) = json.get("actions").and_then(|a| a.as_array()) {
            arr.clone()
        } else if json.is_object() {
            vec![json.clone()]
        } else {
            return Err("La respuesta JSON no contiene acciones".into());
        };

        let mut actions = Vec::new();
        for item in &items {
            if let Some(action) = ComputerAction::from_json(item) {
                actions.push(action);
            }
        }
        if actions.is_empty() {
            return Err("No se reconoció ninguna acción en la respuesta".into());
        }
        Ok(actions)
    }

    /// Get instructions to prepend to the first prompt
    fn system_prompt(&self) -> String {
        r#"You control a Windows desktop. Look at the screenshot and decide the next GUI actions.
Reply with ONLY a raw JSON array of actions — no prose, no Markdown, no code fences.
Coordinates are pixels in the screenshot you were shown ([x, y], origin top-left).
Allowed actions:
  {"action":"left_click","coordinate":[x,y]}
  {"action":"right_click","coordinate":[x,y]}
  {"action":"double_click","coordinate":[x,y]}
  {"action":"mouse_move","coordinate":[x,y]}
  {"action":"left_click_drag","start_coordinate":[x,y],"end_coordinate":[x,y]}
  {"action":"type","text":"hello"}
  {"action":"key","text":"ctrl+s"}
Return an empty array [] only when the task is fully complete.
Example: [{"action":"left_click","coordinate":[100,200]},{"action":"type","text":"hola"}]"#
            .to_string()
    }

    /// Check if API credentials are configured
    async fn check_credentials(&self) -> Result<(), String>;

    /// Optional: Custom logic to build the API request
    async fn build_request(
        &self,
        _config: &ComputeConfig,
        _messages: &[Value],
    ) -> Result<(String, Value), String> {
        Err("Not implemented".into())
    }
}

/// Strip a leading/trailing Markdown code fence (```json … ```), if present,
/// so JSON parsing survives models that ignore "no Markdown" instructions.
fn strip_code_fences(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // Drop the optional language tag on the first line (e.g. ```json).
        let after_lang = rest.splitn(2, '\n').nth(1).unwrap_or(rest);
        let body = after_lang.strip_suffix("```").unwrap_or(after_lang);
        return body.trim().to_string();
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy;
    #[async_trait]
    impl ComputerUseProvider for Dummy {
        fn name(&self) -> &str { "dummy" }
        async fn query_llm(&self, _c: &ComputeConfig, _s: &str, _m: &[Value])
            -> Result<(Vec<ComputerAction>, String, bool), String> { Ok((vec![], String::new(), true)) }
        async fn check_credentials(&self) -> Result<(), String> { Ok(()) }
    }

    #[test]
    fn parses_bare_array() {
        let a = Dummy.parse_actions(r#"[{"action":"left_click","coordinate":[10,20]}]"#).unwrap();
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn parses_actions_wrapper_object() {
        let a = Dummy.parse_actions(r#"{"actions":[{"action":"type","text":"hi"}]}"#).unwrap();
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn parses_single_action_object() {
        let a = Dummy.parse_actions(r#"{"action":"left_click","coordinate":[1,2]}"#).unwrap();
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn parses_through_code_fence() {
        let a = Dummy.parse_actions("```json\n[{\"action\":\"key\",\"text\":\"enter\"}]\n```").unwrap();
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn empty_array_is_no_actions() {
        assert!(Dummy.parse_actions("[]").is_err());
    }
}
