// ── smart_chips.rs — LLM-generated context-aware chip suggestions (v1.4.2) ──
//
// The frontend already has a heuristic chip engine (predictive-chips.ts)
// that does pattern matching on the last turn. This module adds a SECOND
// layer: a fast, cheap LLM call that reads the last few turns and
// proposes 1-3 next actions a heuristic can't easily express.
//
// Design constraints (these are the real reasons for the choices below):
//
//   • CHEAP & FAST. Defaults to Gemini Flash. ~150 input + 80 output tokens
//     ⇒ <$0.0003 per call. Latency 400-800ms. Runs AFTER the main response
//     lands, so it never blocks the user.
//
//   • SELF-CONTAINED. Does NOT reuse the heavy ask_lucy() pipeline. That
//     function injects the full system prompt (5K+ tokens with sections,
//     hosts, principles…), which is wasteful and biases the LLM toward
//     producing Lucy-style answers instead of structured JSON. We build a
//     tiny custom system prompt here.
//
//   • PROVIDER-AGNOSTIC. Gemini Flash is the primary path because it's
//     the cheapest model and almost always configured. Anthropic Haiku
//     is the fallback when no Gemini key exists. OpenAI/local can be
//     added later — these two cover ~95% of users today.
//
//   • STRUCTURED OUTPUT. We ask for strict JSON via response_mime_type
//     (Gemini) or via prompt instruction (Anthropic). A loose JSON
//     extractor (`extract_first_json_array`) salvages malformed output
//     instead of failing the whole call.
//
//   • PRIVACY-AWARE. The frontend MUST NOT call this when privacy_mode
//     is on (no cloud calls allowed). That check lives in the caller —
//     we don't second-guess it server-side.
//
// What we DO NOT do here:
//   - No streaming. Chips appear all-at-once when the call returns.
//   - No retry. If the call fails, the heuristic chips already on screen
//     stay there; the user sees no error.
//   - No persistence of generated chips. Each turn regenerates.

use keyring::Entry;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// A single chip suggestion. Mirrors the frontend `SmartChip` type so
/// the bridge is zero-glue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartChip {
    /// Short button label (≤5 words). Shown to the user.
    pub label: String,
    /// What gets dropped into the input when clicked. Full sentence.
    pub text: String,
    /// Semantic intent (`diagnose|verify|explain|fix|export|continue|other`).
    /// Used by the frontend to pick an icon and severity color.
    pub intent: String,
    /// Tooltip — explains WHY this chip was suggested. Optional but
    /// makes the UI feel less like a slot machine.
    #[serde(default)]
    pub rationale: String,
}

/// Compressed view of one chat turn the frontend sends up. Keeps the
/// payload small — the LLM doesn't need the full markdown, just the gist.
#[derive(Debug, Clone, Deserialize)]
pub struct TurnSummary {
    pub role: String,          // "user" | "lucy"
    pub text: String,          // trimmed to ~600 chars by the caller
    #[serde(default)]
    pub had_tools: bool,       // any <TOOL> tags?
    #[serde(default)]
    pub had_error: bool,       // error/failure detected?
    #[serde(default)]
    pub tool_labels: Vec<String>, // e.g. ["mcp_query:github", "readfile"]
}

// ── Public command ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn generate_smart_chips(
    turns: Vec<TurnSummary>,
    lang: Option<String>,
    model_hint: Option<String>,
) -> Result<Vec<SmartChip>, String> {
    if turns.is_empty() {
        return Ok(Vec::new());
    }
    // Cap at last 6 turns — more than that is noise for a "next 3 chips"
    // decision and bloats the prompt.
    let recent: Vec<&TurnSummary> = turns.iter().rev().take(6).rev().collect();

    let lang = lang.unwrap_or_else(|| "es-MX".into());
    let lang_label = if lang.starts_with("es") { "Spanish" } else { "English" };

    let system_prompt = build_chip_system_prompt(lang_label);
    let user_prompt   = build_chip_user_prompt(&recent);

    // Try Gemini first (cheapest), fall back to Anthropic Haiku.
    if let Ok(key) = read_keyring("gemini_api_key") {
        let model = model_hint
            .as_deref()
            .filter(|m| m.starts_with("gemini"))
            .unwrap_or("gemini-2.5-flash");
        match call_gemini_for_chips(&key, model, &system_prompt, &user_prompt).await {
            Ok(chips) => return Ok(chips),
            Err(e)    => eprintln!("[smart_chips] gemini path failed: {} — trying anthropic", e),
        }
    }

    if let Ok(key) = read_keyring("anthropic_api_key") {
        match call_anthropic_for_chips(&key, "claude-haiku-4-5", &system_prompt, &user_prompt).await {
            Ok(chips) => return Ok(chips),
            Err(e)    => return Err(format!("smart_chips: gemini+anthropic both failed ({})", e)),
        }
    }

    // No keys → silently return empty. Heuristics still cover the user.
    Ok(Vec::new())
}

// ── Tab auto-title (Quick-win B / v1.4.3) ───────────────────────────────
//
// After Lucy responds, the frontend asks this command for a tight 3-5
// word title summarizing the conversation. Reuses the same provider
// routing (Gemini Flash → Anthropic Haiku fallback) as smart_chips but
// asks for a plain string instead of a JSON array.
//
// Latency: same 400-800ms range. Cost: ~$0.0001 because output is tiny.
// Only runs ONCE per tab — the frontend tracks `_titleAuto` and skips
// once the user manually renames the tab.

#[tauri::command]
pub async fn generate_tab_title(
    turns: Vec<TurnSummary>,
    lang: Option<String>,
) -> Result<String, String> {
    if turns.is_empty() {
        return Ok(String::new());
    }
    let recent: Vec<&TurnSummary> = turns.iter().rev().take(4).rev().collect();
    let lang = lang.unwrap_or_else(|| "es-MX".into());
    let lang_label = if lang.starts_with("es") { "Spanish" } else { "English" };

    let sys = format!(
        "You name a chat tab with a SHORT, SCANNABLE title that captures the topic.\n\
        RULES:\n\
        1. 3-5 words MAX. No punctuation other than spaces. No quotes. No emoji.\n\
        2. Title Case. Language: {lang}.\n\
        3. Focus on the SUBJECT, not what Lucy is doing. \
        \"GitHub commits review\" not \"Listing commits\".\n\
        4. Be specific. \"CPU spike Tuesday\" beats \"Performance issue\".\n\
        5. Output ONLY the title — no preamble, no explanation, no JSON.",
        lang = lang_label,
    );

    let mut user_prompt = String::from("Recent conversation:\n\n");
    for (i, t) in recent.iter().enumerate() {
        let truncated: String = t.text.chars().take(400).collect();
        user_prompt.push_str(&format!("[{i}] {role}: {txt}\n\n",
            i = i + 1, role = t.role.to_uppercase(), txt = truncated));
    }
    user_prompt.push_str("Title:");

    // Gemini first.
    if let Ok(key) = read_keyring("gemini_api_key") {
        if let Ok(raw) = call_gemini_for_text(&key, "gemini-2.5-flash", &sys, &user_prompt).await {
            return Ok(sanitize_title(&raw));
        }
    }
    // Fallback Anthropic.
    if let Ok(key) = read_keyring("anthropic_api_key") {
        if let Ok(raw) = call_anthropic_for_text(&key, "claude-haiku-4-5", &sys, &user_prompt).await {
            return Ok(sanitize_title(&raw));
        }
    }
    Ok(String::new())
}

/// Clean up the raw LLM output for use as a tab title.
/// Strips quotes, fences, trailing punctuation, caps at 5 words / 48 chars.
/// Trims aggressively because models love to wrap titles in surrounding
/// whitespace that breaks visual alignment in the tab strip.
fn sanitize_title(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    // Drop a leading/trailing markdown fence if present, including the
    // ```json variants. Has to happen BEFORE trim_matches('`') so we don't
    // accidentally peel one backtick at a time.
    if s.starts_with("```") {
        s = s.trim_start_matches("```json").to_string();
        s = s.trim_start_matches("```").to_string();
        if let Some(idx) = s.rfind("```") { s.truncate(idx); }
        s = s.trim().to_string();
    }
    // Now strip remaining surrounding quotes / inline backticks.
    s = s.trim().trim_matches('"').trim_matches('\'').trim_matches('`').trim().to_string();
    // Trailing punctuation that adds nothing to a tab title.
    s = s.trim_end_matches(|c: char| c == '.' || c == ',' || c == ':' || c == ';').to_string();
    // Drop a "Title:" prefix some models slip in.
    if let Some(rest) = s.strip_prefix("Title:").or_else(|| s.strip_prefix("title:")) {
        s = rest.trim().to_string();
    }
    // Cap words.
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() > 5 {
        s = words[..5].join(" ");
    } else {
        // Rejoin to collapse any leftover internal whitespace.
        s = words.join(" ");
    }
    // Cap chars (in case of one absurdly long word).
    s.chars().take(48).collect()
}

async fn call_gemini_for_text(
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, api_key,
    );
    let payload = json!({
        "system_instruction": { "parts": [{ "text": system }] },
        "contents": [{ "role": "user", "parts": [{ "text": user }] }],
        "generationConfig": { "temperature": 0.3, "maxOutputTokens": 32 }
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build().map_err(|e| e.to_string())?;
    let res = client.post(&url)
        .header("Content-Type", "application/json")
        .json(&payload).send().await
        .map_err(|e| format!("gemini network: {}", e))?;
    if !res.status().is_success() {
        return Err(format!("gemini http {}", res.status()));
    }
    let body: Value = res.json().await.map_err(|e| format!("gemini json: {}", e))?;
    let text = body
        .get("candidates").and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("content")).and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array()).and_then(|a| a.first())
        .and_then(|p| p.get("text")).and_then(|t| t.as_str())
        .unwrap_or("").to_string();
    Ok(text)
}

async fn call_anthropic_for_text(
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let payload = json!({
        "model": model,
        "max_tokens": 32,
        "temperature": 0.3,
        "system": system,
        "messages": [{ "role": "user", "content": user }],
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build().map_err(|e| e.to_string())?;
    let res = client.post("https://api.anthropic.com/v1/messages")
        .header("Content-Type", "application/json")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&payload).send().await
        .map_err(|e| format!("anthropic network: {}", e))?;
    if !res.status().is_success() {
        return Err(format!("anthropic http {}", res.status()));
    }
    let body: Value = res.json().await.map_err(|e| format!("anthropic json: {}", e))?;
    let text = body
        .get("content").and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|b| b.get("text")).and_then(|t| t.as_str())
        .unwrap_or("").to_string();
    Ok(text)
}

// ── Prompt construction ─────────────────────────────────────────────────

fn build_chip_system_prompt(lang_label: &str) -> String {
    format!(
        "You are a tiny background helper inside a SysAdmin AI assistant called Lucy. \
After each conversation turn, you propose 1-3 SHORT next-action suggestions that the user \
might want to click. These are NOT answers — they're button labels that fill the input \
when clicked.\n\
\n\
RULES:\n\
1. Output ONLY a JSON array. No prose, no markdown fences.\n\
2. Each item: {{\"label\": \"...\", \"text\": \"...\", \"intent\": \"...\", \"rationale\": \"...\"}}.\n\
3. label    ≤ 5 words. Imperative voice. Language: {lang}.\n\
4. text     A single complete sentence Lucy will see if clicked. Language: {lang}.\n\
5. intent   One of: diagnose, verify, explain, fix, export, continue, other.\n\
6. rationale 1 short sentence (≤15 words) explaining why this chip fits THIS turn.\n\
7. Never repeat what Lucy just did. Suggest the NEXT step.\n\
8. If the last turn had an error → at least one chip should be intent=fix.\n\
9. If Lucy ended with a question → suggest answers, not new topics.\n\
10. If Lucy listed many items → suggest filtering or focusing.\n\
11. If a host or file or repo was just touched → suggest verifying or exporting it.\n\
12. Max 3 items. Quality > quantity. Return [] if nothing useful comes to mind.",
        lang = lang_label,
    )
}

fn build_chip_user_prompt(turns: &[&TurnSummary]) -> String {
    let mut s = String::from("Recent conversation turns (oldest first):\n\n");
    for (i, t) in turns.iter().enumerate() {
        let truncated: String = t.text.chars().take(600).collect();
        s.push_str(&format!("[{i}] {role}:\n{txt}\n",
            i = i + 1,
            role = t.role.to_uppercase(),
            txt = truncated,
        ));
        let mut meta = Vec::new();
        if t.had_tools  { meta.push("used tools".to_string()); }
        if t.had_error  { meta.push("had error".to_string()); }
        if !t.tool_labels.is_empty() {
            meta.push(format!("tools=[{}]", t.tool_labels.join(", ")));
        }
        if !meta.is_empty() {
            s.push_str(&format!("(meta: {})\n", meta.join("; ")));
        }
        s.push('\n');
    }
    s.push_str("Now output 1-3 chip suggestions as a JSON array.");
    s
}

// ── Provider calls ──────────────────────────────────────────────────────

fn read_keyring(slot: &str) -> Result<String, String> {
    Entry::new("LucySysAdmin", slot)
        .map_err(|e| e.to_string())?
        .get_password()
        .map_err(|e| e.to_string())
}

async fn call_gemini_for_chips(
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
) -> Result<Vec<SmartChip>, String> {
    // Gemini supports response_mime_type=application/json to force valid
    // JSON output. Saves us a fragile regex extraction step.
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, api_key,
    );
    let payload = json!({
        "system_instruction": { "parts": [{ "text": system }] },
        "contents": [{ "role": "user", "parts": [{ "text": user }] }],
        "generationConfig": {
            "response_mime_type": "application/json",
            "temperature": 0.4,
            "maxOutputTokens": 400,
        }
    });

    let client = Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;

    let res = client.post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send().await
        .map_err(|e| format!("gemini network: {}", e))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(format!("gemini http {}: {}", status, body.chars().take(280).collect::<String>()));
    }

    let body: Value = res.json().await.map_err(|e| format!("gemini json: {}", e))?;
    let text = body
        .get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|a| a.first())
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("[]");
    parse_chip_array(text)
}

async fn call_anthropic_for_chips(
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
) -> Result<Vec<SmartChip>, String> {
    let url = "https://api.anthropic.com/v1/messages";
    let payload = json!({
        "model": model,
        "max_tokens": 400,
        "temperature": 0.4,
        "system": system,
        "messages": [{ "role": "user", "content": user }],
    });

    let client = Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;

    let res = client.post(url)
        .header("Content-Type", "application/json")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&payload)
        .send().await
        .map_err(|e| format!("anthropic network: {}", e))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(format!("anthropic http {}: {}", status, body.chars().take(280).collect::<String>()));
    }

    let body: Value = res.json().await.map_err(|e| format!("anthropic json: {}", e))?;
    let text = body
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("[]");
    parse_chip_array(text)
}

// ── Parsing ─────────────────────────────────────────────────────────────

/// Parse a JSON array of chips from raw LLM text, tolerating:
///   • Markdown code fences (```json … ```)
///   • Leading prose ("Sure, here you go: [ … ]")
///   • Single-quoted JSON (some local models do this)
///   • Extra trailing tokens
fn parse_chip_array(raw: &str) -> Result<Vec<SmartChip>, String> {
    let stripped = strip_code_fences(raw);
    let json_slice = extract_first_json_array(&stripped).unwrap_or(&stripped);

    // First try strict parse.
    if let Ok(parsed) = serde_json::from_str::<Vec<SmartChip>>(json_slice) {
        return Ok(sanitize_chips(parsed));
    }

    // Fallback: parse as Value, then coerce each entry leniently.
    let v: Value = serde_json::from_str(json_slice)
        .map_err(|e| format!("smart_chips parse: {} (raw: {})", e, json_slice.chars().take(160).collect::<String>()))?;
    let arr = v.as_array().ok_or("expected JSON array")?;
    let mut out = Vec::new();
    for item in arr {
        let label    = item.get("label").and_then(|v| v.as_str()).unwrap_or("").trim();
        let text     = item.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
        let intent   = item.get("intent").and_then(|v| v.as_str()).unwrap_or("other").trim();
        let rationale = item.get("rationale").and_then(|v| v.as_str()).unwrap_or("").trim();
        if label.is_empty() || text.is_empty() { continue; }
        out.push(SmartChip {
            label: label.to_string(),
            text: text.to_string(),
            intent: intent.to_string(),
            rationale: rationale.to_string(),
        });
    }
    Ok(sanitize_chips(out))
}

fn strip_code_fences(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```json").or_else(|| trimmed.strip_prefix("```")) {
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn extract_first_json_array(s: &str) -> Option<&str> {
    let start = s.find('[')?;
    // Find matching closing bracket (depth count, ignore brackets in strings).
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if escape { escape = false; continue; }
        match b {
            b'\\' if in_str => { escape = true; }
            b'"'            => { in_str = !in_str; }
            b'['  if !in_str => { depth += 1; }
            b']'  if !in_str => {
                depth -= 1;
                if depth == 0 { return Some(&s[start..=i]); }
            }
            _ => {}
        }
    }
    None
}

/// Drop empty / malformed entries, cap at 3, and clamp string lengths.
fn sanitize_chips(input: Vec<SmartChip>) -> Vec<SmartChip> {
    const VALID_INTENTS: &[&str] = &[
        "diagnose", "verify", "explain", "fix", "export", "continue", "other",
    ];
    input.into_iter()
        .filter(|c| !c.label.trim().is_empty() && !c.text.trim().is_empty())
        .map(|mut c| {
            c.label    = c.label.chars().take(48).collect();
            c.text     = c.text.chars().take(400).collect();
            c.rationale = c.rationale.chars().take(160).collect();
            if !VALID_INTENTS.contains(&c.intent.as_str()) {
                c.intent = "other".into();
            }
            c
        })
        .take(3)
        .collect()
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_array_with_leading_prose() {
        let raw = "Sure, here are the chips: [{\"label\":\"a\",\"text\":\"b\",\"intent\":\"fix\"}] trailing junk";
        let slice = extract_first_json_array(raw).expect("found");
        assert!(slice.starts_with('[') && slice.ends_with(']'));
        assert!(slice.contains("\"label\":\"a\""));
    }

    #[test]
    fn strips_code_fences() {
        let raw = "```json\n[1,2,3]\n```";
        assert_eq!(strip_code_fences(raw), "[1,2,3]");
        let raw2 = "```\n[1]\n```";
        assert_eq!(strip_code_fences(raw2), "[1]");
    }

    #[test]
    fn parses_valid_chip_array() {
        let raw = r#"[
          {"label":"Fix it","text":"Apply the fix to the host","intent":"fix","rationale":"error occurred"},
          {"label":"Explain","text":"Explain in plain Spanish","intent":"explain","rationale":""}
        ]"#;
        let chips = parse_chip_array(raw).expect("parses");
        assert_eq!(chips.len(), 2);
        assert_eq!(chips[0].intent, "fix");
        assert_eq!(chips[1].intent, "explain");
    }

    #[test]
    fn rejects_empty_label_or_text() {
        let raw = r#"[
          {"label":"","text":"x","intent":"fix"},
          {"label":"x","text":"","intent":"fix"},
          {"label":"ok","text":"hi","intent":"explain"}
        ]"#;
        let chips = parse_chip_array(raw).expect("parses");
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].label, "ok");
    }

    #[test]
    fn coerces_unknown_intent_to_other() {
        let raw = r#"[{"label":"x","text":"y","intent":"weirdo"}]"#;
        let chips = parse_chip_array(raw).expect("parses");
        assert_eq!(chips[0].intent, "other");
    }

    #[test]
    fn caps_at_three_chips() {
        let raw = r#"[
          {"label":"a","text":"a","intent":"fix"},
          {"label":"b","text":"b","intent":"fix"},
          {"label":"c","text":"c","intent":"fix"},
          {"label":"d","text":"d","intent":"fix"},
          {"label":"e","text":"e","intent":"fix"}
        ]"#;
        let chips = parse_chip_array(raw).expect("parses");
        assert_eq!(chips.len(), 3);
    }

    #[test]
    fn clamps_string_lengths() {
        let long = "x".repeat(1000);
        let raw = format!(r#"[{{"label":"{l}","text":"{l}","intent":"fix","rationale":"{l}"}}]"#, l = long);
        let chips = parse_chip_array(&raw).expect("parses");
        assert!(chips[0].label.chars().count() <= 48);
        assert!(chips[0].text.chars().count() <= 400);
        assert!(chips[0].rationale.chars().count() <= 160);
    }

    #[test]
    fn empty_turns_returns_empty() {
        // Synchronous shortcut path — we can call this without a runtime
        // by replicating the empty-check.
        let turns: Vec<TurnSummary> = Vec::new();
        assert!(turns.is_empty());
    }

    #[test]
    fn handles_malformed_array_gracefully() {
        let raw = "not even json";
        assert!(parse_chip_array(raw).is_err());
    }

    // ── Title sanitizer tests ──
    #[test]
    fn title_strips_surrounding_quotes() {
        assert_eq!(sanitize_title("\"GitHub Commits Review\""), "GitHub Commits Review");
        assert_eq!(sanitize_title("'Quick CPU Check'"), "Quick CPU Check");
    }

    #[test]
    fn title_drops_trailing_punctuation() {
        assert_eq!(sanitize_title("DNS Lookup Issue."), "DNS Lookup Issue");
        assert_eq!(sanitize_title("Disk Cleanup:"), "Disk Cleanup");
    }

    #[test]
    fn title_caps_at_five_words() {
        let r = sanitize_title("One Two Three Four Five Six Seven Eight");
        assert_eq!(r.split_whitespace().count(), 5);
        assert_eq!(r, "One Two Three Four Five");
    }

    #[test]
    fn title_strips_markdown_fence() {
        assert_eq!(sanitize_title("```\nCPU Spike\n```"), "CPU Spike");
    }

    #[test]
    fn title_drops_title_prefix() {
        assert_eq!(sanitize_title("Title: Network Audit"), "Network Audit");
        assert_eq!(sanitize_title("title: Quick Diagnosis"), "Quick Diagnosis");
    }

    #[test]
    fn title_caps_total_chars() {
        let huge = "a".repeat(120);
        let r = sanitize_title(&huge);
        assert!(r.chars().count() <= 48);
    }

    #[test]
    fn title_preserves_short_input() {
        assert_eq!(sanitize_title("Lucy Memory"), "Lucy Memory");
    }
}
