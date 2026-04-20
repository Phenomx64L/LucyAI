// ── ComputerUseProvider Trait — Common Interface ────────────────────────────────

use async_trait::async_trait;
use serde_json::Value;
use crate::commands::computer_use::types::{ComputerAction, ActionResult, ComputeConfig};

/// Common interface for all Computer Use providers
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
    fn parse_actions(&self, response: &str) -> Result<Vec<ComputerAction>, String> {
        // Try to parse as JSON array of actions
        if let Ok(json) = serde_json::from_str::<Value>(response) {
            if let Some(arr) = json.as_array() {
                let mut actions = Vec::new();
                for item in arr {
                    if let Some(action) = ComputerAction::from_json(item) {
                        actions.push(action);
                    }
                }
                if !actions.is_empty() {
                    return Ok(actions);
                }
            }
        }
        Err("Could not parse actions from response".into())
    }

    /// Get instructions to prepend to the first prompt
    fn system_prompt(&self) -> String {
        r#"You are controlling a Windows Remote Desktop session.
You can perform GUI actions like clicking, typing, and taking screenshots.
Respond with JSON array of actions to execute next.
Example: [{"action": "left_click", "coordinate": [100, 200]}]"#
            .to_string()
    }

    /// Check if API credentials are configured
    async fn check_credentials(&self) -> Result<(), String>;

    /// Optional: Custom logic to build the API request
    async fn build_request(
        &self,
        config: &ComputeConfig,
        messages: &[Value],
    ) -> Result<(String, Value), String> {
        Err("Not implemented".into())
    }
}
