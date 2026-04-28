// ── Computer Use — Multi-Provider Architecture ─────────────────────────────────
//
// Supports multiple LLM providers for GUI automation:
//   - Anthropic Claude (native computer_20250124 tool)
//   - Google Gemini Vision (structured JSON prompts)
//   - OpenAI GPT-4V/4o (structured JSON prompts)
//   - Local Ollama models with vision (structured JSON prompts)
//
// All providers execute the same action sequence:
//   screenshot → send_to_llm → parse_actions → execute → screenshot → repeat
//

pub mod traits;
pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod ollama;
pub mod types;

pub use traits::ComputerUseProvider;

/// Factory function to create the appropriate provider based on model name
pub fn create_provider(model: &str) -> Box<dyn ComputerUseProvider> {
    if model.contains("claude") {
        Box::new(anthropic::AnthropicProvider::new())
    } else if model.contains("gemini") {
        Box::new(gemini::GeminiProvider::new())
    } else if model.contains("gpt-4") || model.contains("o1-") {
        Box::new(openai::OpenAiProvider::new())
    } else {
        // Default to local Ollama
        Box::new(ollama::OllamaProvider::new())
    }
}
