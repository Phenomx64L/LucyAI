// ── guardrails/mod.rs — Security guardrails for Lucy ────────────────────────
//
// Pattern-based scanner inspired by Meta's LlamaFirewall, tuned to the
// concrete vulnerabilities surfaced in Lucy's May-2026 security audit:
//
//   S1  — PowerShell injection via stored host password (utils/shell.rs)
//   S2  — cmd /C blocklist bypass (commands/local.rs::execute_cmd)
//   S5  — SSRF via fetch_url_content + redirect chains (commands/ai.rs)
//   S10 — UAC elevation injection from file content (commands/shell.rs)
//
// Design choices
// --------------
// • Pure Rust, zero new dependencies (uses `regex` already in the tree).
// • All patterns are compiled once via `once_cell::sync::Lazy`.
// • Scanner is role-aware: the same regex bank is applied differently
//   depending on whether the text comes from the user, the LLM, a tool
//   output, or a stored secret. This mirrors LlamaFirewall's Role taxonomy.
// • Three-state decision (`Allow` / `HumanInTheLoop` / `Block`) maps cleanly
//   onto Lucy's existing bypass-token flow for dangerous commands.
//
// What this is NOT
// ----------------
// • NOT a replacement for the targeted fixes (S1 still needs `-EncodedCommand`,
//   S2 still needs argv-exec). Guardrails are *defense in depth* — they catch
//   the patterns one layer earlier, but the real fix in the host code stays.
// • NOT only regex anymore (May 2026): the `prompt_guard` module ships an
//   ONNX-based ML classifier behind the `ml-guard` cargo feature. When the
//   feature is built AND the model is installed, ambiguous inputs that the
//   regex bank flagged as HumanInTheLoop get a second pass with PromptGuard
//   2. If ML confirms high jailbreak probability (>=0.85), the decision is
//   PROMOTED to Block. ML can only make the guardrail STRICTER, never lets
//   bad inputs through that regex would have blocked.

mod patterns;
mod scanner;
pub mod prompt_guard;

pub use scanner::{scan, scan_url, ScanDecision, ScanResult, Role};

// ── Tauri command — frontend pre-check ─────────────────────────────────
// Exposed to the Svelte side so the UI can pre-scan user input before
// sending it to the LLM. Returns the full ScanResult (decision + reason +
// matched audit IDs) so the UI can render a meaningful red bubble.
//
// Usage from frontend:
//   const r = await invoke('guardrail_scan', { text, role: 'user' });
//   if (r.decision === 'block') { ... }
//   if (r.decision === 'human_in_the_loop') { ... }

#[tauri::command]
pub fn guardrail_scan(text: String, role: String) -> ScanResult {
    let r = match role.as_str() {
        "user"            => Role::User,
        "tool"            => Role::Tool,
        "assistant"       => Role::Assistant,
        "secret_material" => Role::SecretMaterial,
        _                 => Role::User,
    };
    scan(&text, r)
}

#[tauri::command]
pub fn guardrail_scan_url(url: String) -> ScanResult {
    scan_url(&url)
}

/// Frontend status probe for the ML-guard layer. Returns Active /
/// ModelMissing / RuntimeMissing / FeatureDisabled so the UI can render
/// an actionable badge.
#[tauri::command]
pub fn prompt_guard_status() -> prompt_guard::StatusInfo {
    prompt_guard::status_info()
}
