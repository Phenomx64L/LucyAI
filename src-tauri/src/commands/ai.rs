// ── AI — Integración con Gemini (ask_lucy + ask_lucy_stream) ────────────────────

use keyring::Entry;
use serde_json::json;
use tauri::Emitter;
use futures_util::StreamExt;
use std::sync::Mutex;
use std::sync::OnceLock;
use crate::state::{HTTP_CLIENT, ALLOWED_MODELS};
use crate::commands::metrics::log_usage_internal;

// v1.7.110 audit H3 — bounded JSON parse for untrusted LLM / MCP response
// bodies.
//
// serde_json already enforces a 128-level recursion limit by default, so a
// deeply-NESTED "depth bomb" returns an Err rather than overflowing the
// stack — that dimension was never actually exploitable here. The remaining
// risk is the WIDTH / total-size dimension: a malicious or MITM'd provider
// streaming a multi-gigabyte body that `res.text()` buffers fully into RAM
// before we ever parse it. This helper caps the body length and documents
// the recursion behaviour so the bound is explicit at every call site.
//
// 24 MB ceiling: a 64k-token completion with heavily JSON-escaped content
// plus usage metadata tops out around 3-5 MB in practice; 24 MB is generous
// headroom for the largest legitimate response while still rejecting a
// runaway stream by ~3 orders of magnitude.
const MAX_LLM_JSON_BYTES: usize = 24 * 1024 * 1024;

fn parse_json_capped(body: &str) -> Result<serde_json::Value, String> {
    if body.len() > MAX_LLM_JSON_BYTES {
        return Err(format!(
            "respuesta JSON excede el límite de {} MB ({} bytes) — posible payload malicioso, abortando parse",
            MAX_LLM_JSON_BYTES / (1024 * 1024),
            body.len()
        ));
    }
    serde_json::from_str(body).map_err(|e| format!("Error parseando JSON: {}", e))
}

// ── Sprint 4, UI-7 — Process-wide prompt cache telemetry ──────────────────
//
// Anthropic returns `cache_creation_input_tokens` (write) and
// `cache_read_input_tokens` (hit) on every response when cache_control is
// attached. We accumulate them here so a single tauri command can read the
// running totals for the footer indicator. Volatile by design — restarting
// the app resets the counters, which is fine: the footer shows "this session".
//
// Locked behind a Mutex (not RwLock) because writes happen on every API call
// and reads happen on a 5s timer — contention is negligible.
#[derive(Default, Clone, serde::Serialize)]
pub struct CacheStats {
    pub input_tokens_total: u64,       // sum of normal input tokens (uncached)
    pub cache_creation_total: u64,     // tokens written into the cache
    pub cache_read_total: u64,         // tokens served from cache
    pub calls_with_cache_activity: u64,// how many requests had cache fields populated
    pub calls_total_anthropic: u64,    // every anthropic response we processed
}
static CACHE_STATS: OnceLock<Mutex<CacheStats>> = OnceLock::new();
fn cache_stats() -> &'static Mutex<CacheStats> {
    CACHE_STATS.get_or_init(|| Mutex::new(CacheStats::default()))
}

/// Tauri command — read the per-session cache telemetry for the footer.
#[tauri::command]
pub fn get_cache_stats() -> CacheStats {
    cache_stats().lock().map(|g| g.clone()).unwrap_or_default()
}

/// Adaptive `num_ctx` for Ollama. We used to hardcode 32768 which is fine for
/// log-paste / analysis but blows VRAM on 7B vision models like qwen2.5vl
/// (KV-cache at 32K easily takes 6-10 GB extra → "model runner unexpectedly
/// stopped" 500s).
///
/// Strategy: pick the smallest power-of-2 window that fits `prompt_chars / 3`
/// (rough char→token ratio for mixed ES/EN), clamped to [2048, 32768].
/// Ollama's runner only allocates KV-cache for the requested window, so
/// smaller prompts → smaller cache → no OOM crash.
fn adaptive_num_ctx(prompt_chars: usize) -> u32 {
    let est_tokens = (prompt_chars / 3).saturating_add(512); // +512 for response headroom
    if est_tokens <= 2048  { 2048 }
    else if est_tokens <= 4096  { 4096 }
    else if est_tokens <= 8192  { 8192 }
    else if est_tokens <= 16384 { 16384 }
    else                        { 32768 }
}

// ── LIST LOCAL MODELS (Ollama /api/tags) ─────────────────────────────────────
/// Pregunta a Ollama (o endpoint compatible) qué modelos hay instalados.
/// Lee la URL del chat endpoint guardada en keyring (`local_api_key`),
/// deriva la base y consulta `{base}/api/tags`.
#[tauri::command]
pub async fn list_local_models() -> Result<Vec<String>, String> {
    let entry = Entry::new("LucySysAdmin", "local_api_key").map_err(|e| e.to_string())?;
    let stored = entry.get_password().map_err(|_| "Endpoint local no configurado".to_string())?;

    // Derivar base URL: quitar /v1/chat/completions, /v1, /api/chat, etc.
    let base = stored
        .trim_end_matches('/')
        .trim_end_matches("/v1/chat/completions")
        .trim_end_matches("/api/chat")
        .trim_end_matches("/v1")
        .trim_end_matches("/api")
        .trim_end_matches('/')
        .to_string();

    let tags_url = format!("{}/api/tags", base);
    let resp = HTTP_CLIENT
        .get(&tags_url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("No se pudo conectar a Ollama en {}: {}", tags_url, e))?;

    if !resp.status().is_success() {
        return Err(format!("Ollama respondió {}: verifica que esté corriendo", resp.status()));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| format!("JSON inválido: {}", e))?;
    let models = json["models"].as_array().ok_or("Respuesta sin campo 'models'")?;

    let names: Vec<String> = models
        .iter()
        .filter_map(|m| m["name"].as_str().map(String::from))
        .collect();

    Ok(names)
}

// ── TOKEN EXTRACTION from API responses ──────────────────────────────────────

/// Safely convert u64 to u32, saturating at u32::MAX instead of silently truncating.
#[inline]
fn safe_u64_to_u32(v: u64) -> u32 {
    v.min(u32::MAX as u64) as u32
}

// ── HTTP RETRY HELPER (Tier 1) ───────────────────────────────────────────────
//
// Wraps a reqwest::RequestBuilder with exponential backoff for transient
// failures. Retries up to 3 times total (initial + 2 retries) for:
//   • Network/connection errors (DNS, connect, EOF mid-handshake)
//   • Timeouts (reqwest::Error::is_timeout)
//   • HTTP 429 (rate limited — common with Gemini Flash free tier)
//   • HTTP 5xx (provider-side server error)
//
// Non-retryable (returned immediately):
//   • HTTP 2xx (success)
//   • HTTP 3xx (redirects — handled by reqwest internally)
//   • HTTP 4xx other than 429 (auth, malformed payload — retrying won't help)
//
// Backoff schedule: 1000ms → 2000ms → 4000ms (capped). Total worst-case wait
// added is ~7s before final failure, which is well within the 25s per-request
// timeout most callers set.
//
// Implementation note: `RequestBuilder::try_clone()` returns None for
// requests with a streaming body (e.g. multipart file upload). For Lucy's
// JSON-only LLM calls this always returns Some, but we degrade to a single
// attempt for the None case rather than failing.
pub(crate) async fn send_with_retry(
    req: reqwest::RequestBuilder,
) -> Result<reqwest::Response, String> {
    const MAX_ATTEMPTS: u32 = 3;
    const INITIAL_DELAY_MS: u64 = 1000;
    const MAX_DELAY_MS: u64 = 4000;

    // If the request body can't be cloned (rare for JSON), send once with no retry.
    let Some(_test_clone) = req.try_clone() else {
        return req.send().await.map_err(|e| format!("Error de red: {}", e));
    };
    drop(_test_clone); // we'll re-clone inside the loop

    let mut delay_ms = INITIAL_DELAY_MS;
    let mut last_err_text: Option<String> = None;

    for attempt in 1..=MAX_ATTEMPTS {
        // try_clone is cheap (Arc bumps on inner state); fail-safe to single shot
        let this_attempt = match req.try_clone() {
            Some(c) => c,
            None => return req.send().await.map_err(|e| format!("Error de red: {}", e)),
        };

        match this_attempt.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                // Retryable status codes: 429 (rate limit) and 5xx (server error)
                let retryable = status == 429 || (500..=599).contains(&status);
                if retryable && attempt < MAX_ATTEMPTS {
                    last_err_text = Some(format!("HTTP {}", status));
                    crate::utils::logging::write_app_log(
                        "WARNING",
                        &format!("send_with_retry: HTTP {} on attempt {}/{}, retrying in {}ms",
                            status, attempt, MAX_ATTEMPTS, delay_ms),
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms * 2).min(MAX_DELAY_MS);
                    continue;
                }
                // Either success, non-retryable status, or final attempt — return as-is.
                return Ok(resp);
            }
            Err(e) => {
                let is_transient = e.is_timeout() || e.is_connect() || e.is_request();
                if is_transient && attempt < MAX_ATTEMPTS {
                    last_err_text = Some(e.to_string());
                    crate::utils::logging::write_app_log(
                        "WARNING",
                        &format!("send_with_retry: network error on attempt {}/{} ({}), retrying in {}ms",
                            attempt, MAX_ATTEMPTS, e, delay_ms),
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms * 2).min(MAX_DELAY_MS);
                    continue;
                }
                // Non-transient or final attempt failed.
                return Err(format!("Error de red: {} (tras {} intento{})",
                    e, attempt, if attempt == 1 { "" } else { "s" }));
            }
        }
    }

    // Unreachable in practice (loop always returns), but satisfies the compiler.
    Err(format!("send_with_retry exhausted. Last error: {}",
        last_err_text.unwrap_or_else(|| "unknown".to_string())))
}

/// Extract input and output tokens from Anthropic API response.
///
/// Sprint 3, AI-5 — additionally logs cache hit telemetry. Anthropic's
/// `usage` block exposes `cache_creation_input_tokens` (tokens written into
/// the ephemeral cache, billed 1.25×) and `cache_read_input_tokens` (tokens
/// served from cache, billed 0.1×). Surfacing them is the only way to verify
/// AI-1 (prompt caching) is actually paying off in production — a silent
/// cache miss looks identical to no caching at all in normal logs.
///
/// We fold cache_read into `input_tokens` for the cost calculator (which
/// already applies the per-vendor mid-tier price; the 0.1× discount is then
/// accounted for in the savings line we eprintln below). The cost field in
/// llm_usage stays approximate — exact cache-aware accounting belongs in a
/// future migration that adds dedicated columns.
fn extract_tokens_anthropic(json: &serde_json::Value) -> Option<(u32, u32)> {
    let usage = &json["usage"];
    let input = safe_u64_to_u32(usage["input_tokens"].as_u64()?);
    let output = safe_u64_to_u32(usage["output_tokens"].as_u64()?);
    // Optional fields — only present when cache_control was attached.
    let cache_create = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
    let cache_read   = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
    // Sprint 4, UI-7 — Update process-wide cache telemetry. Every anthropic
    // call increments calls_total_anthropic; only calls that actually exercised
    // the cache (read or write) bump calls_with_cache_activity. This lets the
    // footer show "cache active on N of M last calls" not just total tokens.
    if let Ok(mut g) = cache_stats().lock() {
        g.calls_total_anthropic += 1;
        g.input_tokens_total += input as u64;
        if cache_create > 0 || cache_read > 0 {
            g.cache_creation_total += cache_create;
            g.cache_read_total += cache_read;
            g.calls_with_cache_activity += 1;
        }
    }
    if cache_create > 0 || cache_read > 0 {
        let total_input = input as u64 + cache_create + cache_read;
        let hit_pct = if total_input > 0 {
            (cache_read as f64 / total_input as f64) * 100.0
        } else { 0.0 };
        // Savings = cache_read tokens * 0.9 (we pay 0.1× instead of 1×).
        // Stated as a fraction of what the full uncached call would have cost.
        let savings_pct = if total_input > 0 {
            (cache_read as f64 * 0.9 / total_input as f64) * 100.0
        } else { 0.0 };
        eprintln!(
            "[ai-cache] input={} cache_write={} cache_read={} hit={:.1}% saved≈{:.1}%",
            input, cache_create, cache_read, hit_pct, savings_pct
        );
    }
    Some((input, output))
}

/// Extract input and output tokens from OpenAI API response
fn extract_tokens_openai(json: &serde_json::Value) -> Option<(u32, u32)> {
    let usage = &json["usage"];
    let input = safe_u64_to_u32(usage["prompt_tokens"].as_u64()?);
    let output = safe_u64_to_u32(usage["completion_tokens"].as_u64()?);
    Some((input, output))
}

/// Extract input and output tokens from Google Gemini API response
fn extract_tokens_gemini(json: &serde_json::Value) -> Option<(u32, u32)> {
    let usage = &json["usageMetadata"];
    let input = safe_u64_to_u32(usage["promptTokenCount"].as_u64()?);
    let output = safe_u64_to_u32(usage["candidatesTokenCount"].as_u64()?);
    Some((input, output))
}

/// Resolve a Gemini model selection into:
///   1. The REAL model id Google's API expects (no "::effort" suffix)
///   2. An optional `generationConfig` JSON object containing the right
///      `thinkingConfig.thinkingLevel` value
///
/// Lucy exposes Gemini 3.x Pro as two dropdown entries — "::high" and
/// "::medium" — so the user picks how much reasoning budget to spend BEFORE
/// sending the prompt. This helper strips the suffix and produces the
/// matching generationConfig.
///
/// For non-Pro models, or Pro without an explicit suffix, returns the model
/// id unchanged and `None` (Google then picks its default thinking budget).
///
/// Also handles the legacy alias `gemini-3-flash-preview` → `gemini-3.5-flash`
/// so old saved chats keep working after the May 2026 lineup refresh.
fn resolve_gemini_model(raw_model: &str) -> (String, Option<serde_json::Value>) {
    // Legacy alias — silently upgrade old chats to the GA model.
    if raw_model == "gemini-3-flash-preview" {
        return ("gemini-3.5-flash".to_string(), None);
    }

    // Effort suffix: "<id>::low" | "<id>::medium" | "<id>::high"
    if let Some((base, effort)) = raw_model.split_once("::") {
        let level = match effort.trim().to_lowercase().as_str() {
            "low" | "bajo" => Some("low"),
            "medium" | "med" | "medio" | "balanced" => Some("medium"),
            "high" | "alto" | "deep" => Some("high"),
            _ => None,
        };
        if let Some(lvl) = level {
            let cfg = serde_json::json!({
                "thinkingConfig": { "thinkingLevel": lvl }
            });
            return (base.to_string(), Some(cfg));
        }
        // Unrecognized suffix — strip it but don't add a config (safer than
        // sending an unknown thinkingLevel to Google).
        return (base.to_string(), None);
    }

    (raw_model.to_string(), None)
}

/// Merge an optional generationConfig into the given Gemini payload.
/// If `cfg` is None, the payload is left untouched. If the payload already
/// has a generationConfig, the keys are merged (cfg wins on conflicts).
fn apply_gemini_generation_config(payload: &mut serde_json::Value, cfg: Option<serde_json::Value>) {
    let Some(extra) = cfg else { return };
    let Some(extra_obj) = extra.as_object().cloned() else { return };
    let map = payload.as_object_mut();
    let Some(map) = map else { return };
    let entry = map.entry("generationConfig".to_string()).or_insert_with(|| serde_json::json!({}));
    if let Some(existing) = entry.as_object_mut() {
        for (k, v) in extra_obj { existing.insert(k, v); }
    }
}

/// Resolve a Claude (Anthropic) model selection into:
///   1. The REAL model id Anthropic's API expects (no "::effort" suffix)
///   2. The effort string to send as `output_config.effort` — None when the
///      model doesn't support effort, the suffix is missing, or it's invalid
///
/// Per platform.claude.com/docs/en/build-with-claude/effort:
///   • Opus 4.7    accepts: low | medium | high | xhigh | max
///   • Sonnet 4.6  accepts: low | medium | high | max     (no xhigh)
///   • Opus 4.5    accepts: low | medium | high | max
///   • Mythos      accepts: low | medium | high | max
///   • Haiku 4.5   does NOT support effort
///
/// We accept multilingual aliases (alto/medio/bajo) for parity with the
/// Gemini resolver, and silently strip unsupported suffixes (e.g.
/// `claude-haiku-4-5::high` → just `claude-haiku-4-5` with no effort).
fn resolve_anthropic_model(raw_model: &str) -> (String, Option<&'static str>) {
    let Some((base, effort_raw)) = raw_model.split_once("::") else {
        return (raw_model.to_string(), None);
    };
    let effort = match effort_raw.trim().to_lowercase().as_str() {
        "low"   | "bajo"                       => Some("low"),
        "medium"| "med"  | "medio" | "balanced"=> Some("medium"),
        "high"  | "alto"                       => Some("high"),
        "xhigh" | "x-high" | "extra-alto" | "extra-high" | "extra"
                                               => Some("xhigh"),
        "max"   | "maximo" | "máximo"          => Some("max"),
        _ => None,
    };
    // Per-model whitelist of accepted effort values.
    let supported: &[&str] = match base {
        "claude-opus-4-7"   => &["low", "medium", "high", "xhigh", "max"],
        "claude-sonnet-4-6" => &["low", "medium", "high",          "max"],
        "claude-opus-4-5"   => &["low", "medium", "high",          "max"],
        // Haiku / older models: no effort param at all.
        _ => &[],
    };
    let final_effort = match effort {
        Some(e) if supported.contains(&e) => Some(e),
        _ => None, // unsupported combination — strip the suffix defensively
    };
    (base.to_string(), final_effort)
}

/// Merge `output_config.effort` into a Claude (Anthropic) payload.
/// No-op when `effort` is None or the payload isn't an object.
fn apply_anthropic_output_config(payload: &mut serde_json::Value, effort: Option<&str>) {
    let Some(level) = effort else { return };
    let Some(map) = payload.as_object_mut() else { return };
    let entry = map.entry("output_config".to_string()).or_insert_with(|| serde_json::json!({}));
    if let Some(existing) = entry.as_object_mut() {
        existing.insert("effort".to_string(), serde_json::Value::String(level.to_string()));
    }
}

/// Sprint 1, AI-1 — Build Anthropic payload with prompt caching applied.
///
/// Splits `final_prompt` on the `LUCY_CACHE_BOUNDARY` marker (inserted by
/// `build_composable_prompt`):
///   • stable_half → goes into `system` array with `cache_control: ephemeral`
///   • dynamic_half → goes into `messages[0].content` as user input
///
/// If the marker is absent (e.g. local-model prompt path, or future caller
/// that hasn't migrated), fall back to the original single-message shape so
/// no behavior changes.
///
/// Economics: Anthropic charges cache *writes* at 1.25× and *hits* at 0.1×.
/// Break-even is the second use. Lucy's stable prompt is ~3-5K tokens, so a
/// long session (50+ turns) saves 85% of input cost.
fn build_anthropic_payload_with_cache(
    model: &str,
    max_tokens: u32,
    final_prompt: &str,
    stream: bool,
) -> serde_json::Value {
    use crate::commands::prompt_sections::LUCY_CACHE_BOUNDARY;

    // Find the boundary. split() returns 1 elem if absent — handle that case.
    if let Some(idx) = final_prompt.find(LUCY_CACHE_BOUNDARY) {
        let stable = final_prompt[..idx].trim_end();
        let dynamic_start = idx + LUCY_CACHE_BOUNDARY.len();
        let dynamic = final_prompt[dynamic_start..].trim_start();

        // Defensive: only cache when the stable half is substantial.
        // Caching <1024 tokens (≈4KB) wastes the write multiplier — Anthropic
        // documents a 1024-token minimum to actually engage the cache.
        // Approximate: 1 token ≈ 4 chars for English/Spanish prose.
        const MIN_CACHE_CHARS: usize = 4096;
        if stable.len() >= MIN_CACHE_CHARS {
            return serde_json::json!({
                "model": model,
                "max_tokens": max_tokens,
                "stream": stream,
                "system": [
                    {
                        "type": "text",
                        "text": stable,
                        "cache_control": { "type": "ephemeral" }
                    }
                ],
                "messages": [
                    { "role": "user", "content": dynamic }
                ]
            });
        }
        // Stable section too short to be worth caching — flat message but
        // still split into system + user so structure is correct.
        return serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "stream": stream,
            "system": stable,
            "messages": [
                { "role": "user", "content": dynamic }
            ]
        });
    }

    // No boundary marker — legacy path. Drop everything into the user message,
    // same as the pre-AI-1 behavior.
    serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "stream": stream,
        "messages": [
            { "role": "user", "content": final_prompt }
        ]
    })
}

// ── MAX TOKENS por modelo ────────────────────────────────────────────────────
/// Devuelve el max_tokens óptimo para cada modelo de Anthropic.
/// Si se pasa un override > 0, lo usa directamente (escalación por truncamiento).
fn get_max_tokens(model: &str, override_val: Option<u32>) -> u32 {
    if let Some(v) = override_val {
        if v > 0 { return v; }
    }
    if model.contains("sonnet-4") || model.contains("opus-4") {
        16384
    } else if model.contains("3-7") || model.contains("3.7") {
        16384
    } else if model.contains("3-5") || model.contains("3.5") {
        8192
    } else {
        8192 // default seguro
    }
}

// ── URL CONTENT FETCHER ───────────────────────────────────────────────────────

/// Strips HTML tags from a string, returning readable plain text.
fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag   = false;
    let mut in_script = false;
    let mut in_style  = false;

    for (i, c) in html.char_indices() {
        if c == '<' {
            in_tag = true;
            let remain = &html[i..];
            if remain.get(..7).map_or(false, |s| s.eq_ignore_ascii_case("<script")) {
                in_script = true;
            } else if remain.get(..8).map_or(false, |s| s.eq_ignore_ascii_case("</script")) {
                in_script = false;
            } else if remain.get(..6).map_or(false, |s| s.eq_ignore_ascii_case("<style")) {
                in_style = true;
            } else if remain.get(..7).map_or(false, |s| s.eq_ignore_ascii_case("</style")) {
                in_style = false;
            }
        } else if c == '>' {
            in_tag = false;
            if !in_script && !in_style { out.push(' '); }
        } else if !in_tag && !in_script && !in_style {
            out.push(c);
        }
    }

    // Decode common HTML entities & collapse whitespace
    out.replace("&amp;",  "&")
       .replace("&lt;",   "<")
       .replace("&gt;",   ">")
       .replace("&quot;", "\"")
       .replace("&#39;",  "'")
       .replace("&nbsp;", " ")
}

/// Fetches a URL and returns up to 12 000 chars of readable plain text.
///
/// Guardrail (audit S5): rejects SSRF targets — loopback, RFC1918,
/// link-local, cloud-metadata IPs — and any scheme other than http(s).
/// Without this, an LLM-emitted `<TOOL>fetch_url:http://169.254.169.254/...</TOOL>`
/// would exfiltrate cloud IAM credentials via the response body.
#[tauri::command]
pub async fn fetch_url_content(url: String) -> Result<String, String> {
    let scan = crate::guardrails::scan_url(&url);
    if !matches!(scan.decision, crate::guardrails::ScanDecision::Allow) {
        return Err(format!("URL bloqueada por guardrail [{}]", scan.reason));
    }
    // v1.7.110 audit H1 — DNS-rebinding guard. scan_url only checked the URL
    // string; this resolves the host and rejects if it points at an internal
    // IP (the hostname-resolves-to-127.0.0.1 SSRF the string regex can't see).
    // Also hardens against octal/hex/decimal IP obfuscation since the OS
    // resolver normalizes those. Blocking resolver → spawn_blocking so we
    // don't stall the tokio worker.
    {
        let url_for_resolve = url.clone();
        let resolve_check = tauri::async_runtime::spawn_blocking(move || {
            crate::guardrails::host_resolves_to_internal(&url_for_resolve)
        })
        .await
        .map_err(|e| format!("Error interno verificando host: {}", e))?;
        if let Err(reason) = resolve_check {
            return Err(format!("URL bloqueada por SSRF guard: {}", reason));
        }
    }
    let res = HTTP_CLIENT
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Lucy/1.0")
        .header("Accept", "text/html,application/xhtml+xml,text/plain;q=0.9")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Error de red al obtener URL: {}", e))?;

    let status = res.status().as_u16();
    if status >= 400 {
        return Err(format!("La URL devolvió HTTP {}", status));
    }

    let body = res.text().await
        .map_err(|e| format!("Error al leer cuerpo: {}", e))?;

    let plain = strip_html_tags(&body);
    let clean: String = plain.split_whitespace().collect::<Vec<&str>>().join(" ");
    let truncated = crate::utils::safe_truncate(&clean, 6_000);

    Ok(truncated.to_string())
}

// ── PROMPT HELPERS ────────────────────────────────────────────────────────────

fn lang_instruction(lang: &str) -> &'static str {
    match lang {
        l if l.starts_with("es") => "LANGUAGE RULE: Always respond in Spanish.",
        l if l.starts_with("en") => "LANGUAGE RULE: Always respond in English.",
        l if l.starts_with("pt") => "LANGUAGE RULE: Always respond in Portuguese.",
        l if l.starts_with("fr") => "LANGUAGE RULE: Always respond in French.",
        l if l.starts_with("de") => "LANGUAGE RULE: Always respond in German.",
        l if l.starts_with("it") => "LANGUAGE RULE: Always respond in Italian.",
        l if l.starts_with("ja") => "LANGUAGE RULE: Always respond in Japanese.",
        l if l.starts_with("zh") => "LANGUAGE RULE: Always respond in Chinese.",
        _                        => "LANGUAGE RULE: Detect the user language and respond in the same language.",
    }
}

fn build_hosts_context(hosts_json: Option<&str>) -> String {
    let Some(hj) = hosts_json else { return String::new(); };
    let Ok(hosts) = serde_json::from_str::<serde_json::Value>(hj) else { return String::new(); };
    let Some(arr) = hosts.as_array() else { return String::new(); };
    if arr.is_empty() { return String::new(); }

    // CRITICAL: we expose `id` so Lucy can fill target="..." in EXECUTE_REMOTE.
    // We intentionally do NOT suggest raw Invoke-Command/ssh here because
    // RULE 14 forbids them for configured hosts — the previous version of
    // this helper contradicted RULE 14 and caused Lucy to emit bare
    // markdown commands that the frontend never executed.
    let mut lines = String::from("\n--- CONFIGURED REMOTE HOSTS ---\n");
    lines.push_str("When user mentions any of these by name, you MUST wrap the command in:\n");
    lines.push_str("<EXECUTE_REMOTE target=\"HOST_ID\">YOUR_COMMAND</EXECUTE_REMOTE>\n");
    lines.push_str("using the exact `id` field below as HOST_ID. NEVER use Invoke-Command, PSCredential,\n");
    lines.push_str("or raw `ssh user@ip` — credentials and transport are handled by the system.\n\n");

    for h in arr {
        let id      = h["id"].as_str().unwrap_or("?");
        let name    = h["name"].as_str().unwrap_or("?");
        let htype   = h["type"].as_str().unwrap_or("windows");
        let host_ip = h["host"].as_str().unwrap_or("?");
        let uname   = h["username"].as_str().unwrap_or("?");
        let port    = h["port"].as_u64().unwrap_or(if htype == "linux" { 22 } else { 5985 });
        let proto   = if htype == "linux" { "SSH" } else { "WinRM" };
        lines.push_str(&format!(
            "- id=\"{id}\" name=\"{name}\" type={htype} ({proto}) ip={host_ip} user={uname} port={port}\n"
        ));
    }
    // Concrete example using a real id from the list — if arr is non-empty,
    // show one example so Lucy has a pattern to copy rather than guessing.
    if let Some(first) = arr.first() {
        let id   = first["id"].as_str().unwrap_or("?");
        let name = first["name"].as_str().unwrap_or("?");
        let htype = first["type"].as_str().unwrap_or("windows");
        let example_cmd = if htype == "linux" {
            "top -b -n 1 -o %CPU | head -n 20"
        } else {
            "Get-Process | Sort-Object CPU -Descending | Select -First 10 Name, CPU, Id"
        };
        lines.push_str(&format!(
            "\nEXAMPLE — user says \"investigar CPU en {name}\" → you emit:\n\
             <EXECUTE_REMOTE target=\"{id}\">{example_cmd}</EXECUTE_REMOTE>\n"
        ));
    }
    lines.push_str("--- END HOSTS ---\n");
    lines
}

// MED-3 / RUST-1 FIX: all disk I/O moved into spawn_blocking to avoid stalling
// the tokio executor. Reading many runbook files (potentially hundreds of MB)
// was previously done directly on the async executor thread.
#[tauri::command]
pub async fn search_runbooks(dir_path: Option<String>, query: String) -> Result<String, String> {
    let Some(path) = dir_path else { return Err("No runbooks directory configured.".to_string()) };

    tokio::task::spawn_blocking(move || {
        use simsearch::SimSearch;

        let path_obj = std::path::Path::new(&path);
        if !path_obj.exists() || !path_obj.is_dir() {
            return Err("Runbooks directory not found.".to_string());
        }

        let mut engine: SimSearch<String> = SimSearch::new();
        let mut files_metadata = std::collections::HashMap::new();

        if let Ok(entries) = std::fs::read_dir(path_obj) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Some(ext) = p.extension() {
                        let ext_str = ext.to_string_lossy().to_lowercase();
                        if ext_str == "md" || ext_str == "txt" {
                            if let Ok(text) = std::fs::read_to_string(&p) {
                                let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                                engine.insert(name.clone(), &text);
                                files_metadata.insert(name, text);
                            }
                        }
                    }
                }
            }
        }

        let results = engine.search(&query);
        if results.is_empty() {
            return Ok(format!("[Sin resultados SEMÁNTICOS estrictos para '{}'", query));
        }

        let mut out = String::new();
        for r in results.into_iter().take(2) {
            if let Some(content) = files_metadata.get(&r) {
                let trunc = crate::utils::safe_truncate(content, 12000);
                out.push_str(&format!("--- RUNBOOK FILE: {} ---\n{}\n\n", r, trunc));
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn change_agent_dir(path: String) -> Result<String, String> {
    // SEC-2 FIX: reject path traversal (..) and sensitive directories.
    if path.contains("..") {
        return Err("Path traversal bloqueado: '..' no permitido en change_agent_dir.".to_string());
    }
    let p = std::path::Path::new(&path);
    if !p.exists() || !p.is_dir() {
        return Err(format!("Directorio no encontrado o no válido: {}", path));
    }
    // Canonicalize and validate against sensitive directory blocklist.
    let canonical = p.canonicalize()
        .map_err(|e| format!("No se pudo resolver el directorio: {}", e))?;
    let canon_lower = canonical.to_string_lossy().to_ascii_lowercase();
    let blocked_dirs: &[&str] = &[
        r"c:\windows", r"c:\program files", r"c:\program files (x86)",
        r"c:\programdata\microsoft", r"c:\$recycle.bin",
        r"c:\system volume information",
    ];
    for bd in blocked_dirs {
        if canon_lower.starts_with(bd) {
            return Err(format!("Directorio bloqueado por política de seguridad: {}", canonical.display()));
        }
    }
    if let Ok(mut cwd) = crate::state::GLOBAL_CWD.write() {
        *cwd = canonical.to_string_lossy().to_string();
        // NOTE: std::env::set_current_dir removed — it's a process-global side effect
        // that races with concurrent async tasks. GLOBAL_CWD is the single source of truth.
        Ok(format!("Directorio de trabajo cambiado a: {}", canonical.display()))
    } else {
        Err("Fallo al bloquear GLOBAL_CWD".into())
    }
}

/// build_system_prompt
/// 
/// ARCHITECTURAL DECISION & INTELLECTUAL PROPERTY: 
/// Las "RULES" (21 + custom) de este agente no son prompts genéricos.
/// Son la propiedad intelectual central del proyecto, derivadas de 10+ años de 
/// experiencia en Administración de Sistemas por parte de Iván Eduardo Luna (@Phenomx64L).
/// 
/// Cada RULE resuelve problemas específicos y protege al sistema operativo:
/// - RULE 14: HOST ROUTING previene confusión multi-host destructiva.
/// - RULE 18.5: AUTONOMOUS CODING capacita al agente sin requerir proxy de usuario.
/// - RULE 23: ReAct SELF-CORRECTION previene bucles infinitos en PowerShell.
/// - RULE 25: TIERED MEMORY dirige el enrutamiento semántico.
/// 
/// PROTECTED BY GNU GPLv3: Distribuir o alterar esta lógica clave exige
/// mantener el código fuente abierto y otorgar crédito explícito al autor original.
/// Ver: https://github.com/Phenomx64L/LucyAI
/// build_system_prompt — now delegates to the composable prompt_sections module.
/// Kept as a thin wrapper to preserve the same call signature for ask_lucy/ask_lucy_stream.
fn build_system_prompt(
    lang: &str,
    context: &str,
    hosts_context: &str,
    user_name: &str,
    prompt: &str,
    working_dir: &str,
    runbooks_dir: Option<&str>,
) -> String {
    crate::commands::prompt_sections::build_system_prompt_v2(
        lang, context, hosts_context, user_name, prompt, working_dir, runbooks_dir,
    )
}

/// Legacy monolithic build — preserved for reference and rollback.
/// Remove after v1.3.0 ships stable.
#[allow(dead_code)]
fn build_system_prompt_legacy(
    lang: &str,
    context: &str,
    hosts_context: &str,
    user_name: &str,
    prompt: &str,
    working_dir: &str,
    runbooks_dir: Option<&str>,
) -> String {
    let cwd = working_dir;
    let runbooks_info = if let Some(rf) = runbooks_dir {
        format!("Runbooks Directory Configured: {rf}\nUse <TOOL>search_runbooks:YOUR_QUERY</TOOL> to fetch specific runbook files using Semantic Search TF-IDF. Strongly consider doing this BEFORE executing commands if the user is asking context-heavy infrastructure questions.")
    } else { "".to_string() };

    let user_profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".to_string());
    let core_mem_block = crate::commands::memory::render_core_sync();
    let principles_block = crate::commands::principles::render_principles_block(None);
    format!(
        "You are Lucy, an expert Windows SysAdmin AI assistant with autonomous code analysis and modification capabilities.\n\
        {lang}\n\
        CURRENT USER: {user_name} (Profile: {user_profile})\n\
        WORKING DIRECTORY: {cwd}\n\
        {rb}\n\
        When the user references project files without a full path, resolve them relative to this directory.\n\
        RULE 0 — INTENT DETECTION (apply BEFORE anything else):\n\
        STEP 1: Classify the message into one of these categories:\n\
          A) CONVERSATIONAL — general questions -> respond with normal text.\n\
          B) FILE OPERATION — user asks to create, edit, or read a local file -> You MUST generate a markdown PowerShell block (e.g., ```powershell New-Item ... ```) to natively execute the file operation. DO NOT explicitly ask for permission. ACTUALLY create or edit the file autonomously.\n\
          C) SYSTEM ACTION — user asks to execute on the system -> Use <EXECUTE> tags or native markdown powershell blocks autonomously.\n\
          D) CODE GENERATION — user EXPLICITLY asks to just SEE code without running it -> Provide standard markdown code blocks without executing.\n\
          RULE 1: For trivial tasks (like simple file creation, basic commands), COMPLETELY BYPASS <THOUGHT> tags and output the markdown codeblock or <EXECUTE> tags NATIVELY to save tokens and answer instantaneously. Do not pause to ask for permission. Just do it.\n\
        RULE 2: If a command requires admin elevation, DO NOT auto-generate Start-Process RunAs. Instead: explain what requires elevation, show the command the user should run, and ask 'Do you want me to execute this with admin privileges?'. Only generate the RunAs <EXECUTE> after user explicitly confirms.\n\
        RULE 3: NEVER print raw HTML. Use Markdown for formatting responses.\n\
        RULE 4: ONLY if a command you already executed in THIS conversation returned an error, analyze the error and ask how to proceed WITHOUT generating <EXECUTE>. Do NOT apply this rule to new independent instructions.\n\
        RULE 5: Silently correct phonetically mistranscribed words.\n\
        RULE 6: If the user teaches you a command, respond ONLY with <LEARN>key1,key2|powershell_command|response</LEARN>.\n\
        RULE 6b — PERSONAL MEMORY: When the user reveals stable personal facts, preferences, or environment info worth remembering across sessions, silently emit a <REMEMBER> tag ALONGSIDE your normal response (not instead of it). The tag is stripped from display and persisted to the user profile. Format: <REMEMBER category=\"identity|preference|context|host\">key: value</REMEMBER>. Valid categories: 'identity' (name, role, org), 'preference' (verbose/concise, shell, language), 'context' (projects, responsibilities), 'host' (info tied to a specific server). Only remember FACTS — not conversational filler. Do NOT re-remember facts already shown in the '--- PERFIL DEL USUARIO ---' section. Examples: <REMEMBER category=\"preference\">preferred_shell: PowerShell 7</REMEMBER>, <REMEMBER category=\"context\">main_project: Lucy Tauri assistant</REMEMBER>.\n\
        RULE 7 — PDF GENERATION: To create PDFs use Edge Headless. CRITICAL: 'msedge' is NOT in the system PATH on most installations. NEVER call it as a bare command. Use ONE of these patterns: (a) Quote the full path with the call operator: & 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe' --headless --disable-gpu --print-to-pdf=\"OUT.pdf\" \"file:///INPUT.html\"; (b) Discover first: emit <TOOL>locate_file:msedge.exe</TOOL>, then use the returned path quoted with &; (c) Fallback to Chrome: & 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe' with the same flags. If a previous turn already produced 'msedge no se reconoce' / 'CommandNotFoundException', do NOT retry the bare command — switch to a quoted full path immediately.\n\
        RULE 8: For Linux use native ssh. For Windows Server use Invoke-Command -ComputerName. EXCEPTION: if the context says \"ACTIVE REMOTE SHELL\", the session is already established — generate RAW commands only, NO Invoke-Command, NO -ComputerName, NO -Credential wrappers.\n\
        RULE 9: <TOOL>sysinfo</TOOL> is ONLY for LOCAL machine hardware queries: CPU usage, RAM, disk, system health, uptime. NEVER use sysinfo for: code analysis, file review, bug detection, logic verification, architecture analysis, or ANY question about code/files/projects. For REMOTE hosts, use Invoke-Command or SSH with the host details. EXCEPTION: if context says \"ACTIVE REMOTE SHELL\", generate raw commands — the WinRM/SSH tunnel is already open.\n\
        RULE 10: To keep the machine awake use PowerToys Awake.\n\
        RULE 11: For cleaning system logs, ALWAYS use RULE 2 elevation.\n\
        RULE 12: If asked about quick actions or the sidebar, tell them to use the + button in the side panel.\n\
        RULE 13: Each user message is INDEPENDENT unless explicitly referencing a previous result. Do NOT mix outputs or reports from previous tasks into new responses.\n\
        RULE 14 — HOST ROUTING (CRITICAL — DO NOT SKIP): If the user's message mentions ANY host name, alias, or ID listed in the CONFIGURED REMOTE HOSTS block below, you MUST emit the command wrapped in <EXECUTE_REMOTE target=\"<id>\">...</EXECUTE_REMOTE> using the exact `id` field from that block. Do NOT describe what you would do. Do NOT show the command as markdown. Do NOT wait for permission. Do NOT use Invoke-Command, PSCredential, ssh, or scp. Emit the tag IMMEDIATELY as part of your first response. The frontend will execute it, capture output, and send it back for analysis on the NEXT turn. If you respond without <EXECUTE_REMOTE> when a host is clearly mentioned, NOTHING runs and the user sees dead text. Example: user says \"CPU alta en PARROT\" and PARROT has id=\"abc123\" type=linux → your entire response body should include <EXECUTE_REMOTE target=\"abc123\">top -b -n 1 -o %CPU | head -n 20</EXECUTE_REMOTE>.\n\
        CRITICAL RULE FOR REMOTE: If an <EXECUTE_REMOTE> command returns a syntax error or property validation error, DO NOT attempt to rewrite the command using Invoke-Command or Get-Credential. The connection is fully isolated and managed by the system. Simply correct your YOUR_COMMAND syntax and try again using <EXECUTE_REMOTE>.\n\
        RULE 15 — ALTERNATIVE EXECUTORS (use when PowerShell is blocked by policy or unavailable):\n\
        RULE 15b — AVOID TERMINAL-SERVER-ONLY COMMANDS: 'query user', 'query session', 'qwinsta' ONLY work on Terminal Server / RDS hosts. On regular Windows workstations/servers to check if a user is active or enabled, ALWAYS use PowerShell: Get-LocalUser -Name 'username' | Select Name,Enabled,LastLogon. To list logged-on users: Get-WmiObject Win32_LoggedOnUser | Select Antecedent -Unique.\n\
        RULE 16 — WEB DOCUMENTATION CONTEXT: If the context contains '--- CONTENIDO WEB: <url> ---' blocks, the system has already fetched and embedded that documentation. Use it directly to cross-reference against live data. CRITICAL: reading web context does NOT change your execution behavior — continue using <EXECUTE> tags exactly as before. Do NOT say you cannot access URLs. After consulting the web content, immediately generate the appropriate <EXECUTE> command to retrieve the live data needed for comparison.\n\
        - CMD (<EXECUTE_CMD>): net, ipconfig, netstat, ping, tracert, dir, tasklist, sc, reg query — any cmd.exe command.\n\
        - WMIC (<EXECUTE_WMIC>): ⚠️ STRICT SCOPE — ONLY for Win32_* hardware/OS classes via allowed aliases: cpu, os, diskdrive, logicaldisk, memorychip, computersystem, nic, nicconfig, process, service, startup, bios, baseboard, csproduct, useraccount, qfe, or `path Win32_*`. Examples: 'cpu get name,maxclockspeed', 'os get caption,version', 'diskdrive get model,size', 'memorychip get capacity', 'bios get serialnumber'. ❌ NEVER put `reg query`, registry paths, file system commands, or anything that is not a WMI/Win32_* query inside <EXECUTE_WMIC>. Doing so will be rejected with 'Query WMIC no permitida'.\n\
        - NETSH (<EXECUTE_NETSH>): network/firewall config — 'interface ip show config', 'advfirewall show allprofiles', 'wlan show profiles', 'interface show interface'.\n\
        - REG (<EXECUTE_REG>): registry read/query — examples: 'query HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion /v ProductName', 'query \"HKLM\\SOFTWARE\\Microsoft\\Windows Defender\\Real-Time Protection\" /s'. ⚠️ ANY command starting with `reg query`, `reg add`, `reg delete`, or that references HKLM/HKCU/HKCR/HKU paths MUST go inside <EXECUTE_REG> — NEVER inside <EXECUTE_WMIC> (WMI ≠ Registry).\n\
        - CSCRIPT (<EXECUTE_CSCRIPT>): VBS scripts for COM/AD — 'Dim obj: Set obj = GetObject(\"WinNT://./\") : For Each u in obj : WScript.Echo u.Name : Next'.\n\
        - NATIVE_REGISTRY (<TOOL>registry:HKLM|SOFTWARE\\...|ValueName</TOOL>): reads registry directly from Rust, works even when reg.exe is blocked.\n\
        - NATIVE_NETSTAT (<TOOL>netconn</TOOL>): returns active network connections from native Rust.\n\
        - NATIVE_TASKLIST (<TOOL>tasklist</TOOL>): returns running processes via native sysinfo.\n\
        - EVENT_LOG (<TOOL>eventlog:System:50:error</TOOL>): reads Windows Event Log entries. Format: log_name:count:level (level optional: critical|error|warn|info).\n\
        When the user asks for network info, processes, registry values, or hardware info and PowerShell might be restricted, prefer these native alternatives.\n\
        RULE 17 — FILE & CODE TOOLS (ALWAYS prefer over PowerShell — you are an AI agent with tool chaining):\n\
        These tools execute natively in Rust. The system will automatically feed results back to you so you can chain multiple operations.\n\
        ⚠️ CRITICAL SYNTAX RULE: You MUST ALWAYS wrap tool invocations inside <TOOL>...</TOOL> tags VERBATIM. NEVER write a tool name as plain text (e.g. NEVER write 'system_diff:tasks' alone — it MUST be '<TOOL>system_diff:tasks</TOOL>'). The system parser only recognizes tools wrapped in literal <TOOL> tags. Plain text tool names will be IGNORED and the user will see nothing happen.\n\
        Available tools:\n\
        - READ FILE: <TOOL>readfile:/path/to/file</TOOL> — reads file content (max 512KB). For large files use readlines.\n\
        - READ LINES: <TOOL>readlines:/path/to/file:START:COUNT</TOOL> — reads specific lines (1-based). Example: <TOOL>readlines:C:\\config.txt:1:50</TOOL>\n\
        - WRITE FILE: <TOOL>writefile:/path/to/file</TOOL> followed by <FILECONTENT>full content</FILECONTENT> — overwrites entire file. ⚠️ TEXT ONLY: writefile writes UTF-8 text. It CANNOT create binary files (.ico, .png, .jpg, .exe, .dll, .wasm, .onnx, etc.). For binary assets use <EXECUTE> with PowerShell: `Invoke-WebRequest -Uri URL -OutFile path` to download, or `cargo tauri icon input.png` to generate app icons, or `[System.IO.File]::WriteAllBytes(path, bytes)` for raw bytes.\n\
        - EDIT FILE: <TOOL>editfile:/path/to/file|||exact text to find|||replacement text</TOOL> — surgical find-and-replace WITHOUT rewriting the whole file. PREFERRED for modifications.\n\
        - LIST DIR: <TOOL>listdir:/path/to/dir</TOOL> — lists directory contents with sizes and dates.\n\
        - LOCATE FILE: <TOOL>locate_file:name</TOOL> — Searches the entire local drive for a filename instantaneously (O(log n)) using the SQLite indexer.\n\
        - START INDEXER: <TOOL>start_indexer:C:\\</TOOL> — Rebuilds the global SQLite file index for a given path. Use this if locate_file cannot find something you suspect exists.\n\
        - CHANGE DIR: <TOOL>cd:/nueva/ruta</TOOL> — Changes your logical working directory. Use this when the user asks you to switch paths or create a project in a specific directory. ⚠️ CRITICAL: NEVER use `<EXECUTE>cd path</EXECUTE>` — that spawns a subprocess that exits immediately and changes NOTHING. ALWAYS use `<TOOL>cd:path</TOOL>` which persists the change for all future commands.\n\
        RULE 22 — WEB KNOWLEDGE: NEVER guess release dates, software versions, or information post-2024. Use <TOOL>search_web:query</TOOL> IMMEDIATELY and autonomously — do NOT ask the user for permission. If a snippet is too short or lacks exact data, follow up with <TOOL>fetch:URL</TOOL> on the result URL before answering.\n\
        - SEARCH WEB: <TOOL>search_web:query</TOOL> — Tavily API (preferred, AI-summarized) or DuckDuckGo fallback. Use for documentation, current events, software versions, or system requirements.\n\
        - FETCH WEB: <TOOL>fetch:URL</TOOL> — Fetches full text of a webpage. Use when search snippets are insufficient.\n\
        - SYSTEM DIFF: <TOOL>system_diff:tasks</TOOL> or <TOOL>system_diff:network</TOOL> — Takes a snapshot of system processes or ports. Call it again later to get a DIFFERENCE (who died/closed, who was born/opened). Perfect for verifying if your commands worked.\n\
        - SEARCH RUNBOOKS: <TOOL>search_runbooks:query</TOOL> — uses TF-IDF Semantic similarity to fetch the top 2 company runbooks that match your technical issue query.\n\
        - SEMANTIC SEARCH: <TOOL>semantic:natural language query</TOOL> — vector search (cosine over Ollama embeddings) across the user's saved skills and persistent memories. USE THIS FIRST when the user's phrasing may not match exact trigger words — e.g. user says \"the server won't respond\" → semantic can surface a 'restart service' skill even if the trigger is 'reiniciar servicio'. Returns top hits with similarity scores; if it returns [SEMANTIC SEARCH UNAVAILABLE], fall back to search_runbooks or search_web.\n\
        - SEARCH FILES: <TOOL>searchfiles:/directory|||pattern</TOOL> — searches text across all files. For multi-pattern search, separate words with '|' (e.g. ERROR|CRITICAL|PANIC), this uses Aho-Corasick for blazing speed.\n\
        - ANALYZE CODE: <TOOL>analyze_code:/path</TOOL> — uses Tree-Sitter to extract the Abstract Syntax Tree (AST) summary of Rust or JavaScript. Use this BEFORE reading the whole file if you only want to explore existing functions/classes.\n\
        - SUB-AGENTS (Parallel Forking): Use these to investigate multiple things simultaneously, saving iterations.\n\
          - FORK: <TOOL>fork_task:UniqueID|||Single-shot instruction for the sub-agent (no tools available)</TOOL> — Launches a fast background LLM agent. Returns immediately with [FORK LAUNCHED]. The sub-agent runs in parallel while you continue with other actions.\n\
          - WAIT: <TOOL>wait_task:UniqueID</TOOL> — Blocks until the forked sub-agent finishes and returns its result. Use in a LATER step after fork_task. Example pattern: fork ResearchA + fork ResearchB → do other work → wait ResearchA → wait ResearchB → synthesize.\n\
          - RULE: UniqueID must be a short snake_case string (e.g. research_deps, check_errors). Never reuse the same ID in one task.\n\
        - MCP DISCOVER: <TOOL>mcp_discover:server_cmd</TOOL> — Interrogates an MCP server (e.g. npx -y @modelcontextprotocol/server-sqlite) to learn what tools it offers. YOU MUST ALWAYS EXECUTE THIS FIRST before using mcp_query on an unknown MCP.\n\
        - MCP QUERY: <TOOL>mcp_query:server_cmd|||tool_name|||json_args</TOOL> — Spawns a local MCP server, asks for a tool, and returns result. (E.g. <TOOL>mcp_query:npx -y @modelcontextprotocol/server-sqlite|||query|||{{\"query\":\"SELECT * FROM foo\"}}</TOOL>).\n\
        MCP SERVERS AVAILABLE (no install needed — npx auto-downloads):\n\
          • Git/version-control: uvx mcp-server-git — tools: git_log, git_diff, git_status, git_commit, git_branch\n\
          • SQLite DB: npx -y @modelcontextprotocol/server-sqlite -- /path/to/db.sqlite — tools: query, list-tables, describe-table\n\
          • Filesystem (ACL): npx -y @modelcontextprotocol/server-filesystem /allowed/path — tools: read_file, write_file, list_directory\n\
          • Memory (persistent KV): npx -y @modelcontextprotocol/server-memory — tools: create_entities, search_nodes, read_graph\n\
          • Shodan (recon): npx -y @burtthecoder/mcp-shodan — requires SHODAN_API_KEY in MCP secrets — tools: search, host_info, dns_lookup\n\
          • VirusTotal (malware): npx -y @burtthecoder/mcp-virustotal — requires VIRUSTOTAL_API_KEY — tools: file_report, url_report, ip_report\n\
          WORKFLOW: 1) mcp_discover the server 2) learn its tools 3) mcp_query with correct tool_name and json_args.\n\
        PERSISTENT MEMORY — Cross-session knowledge store. Use these to remember important discoveries:\n\
        - SAVE MEMORY: <TOOL>memoria_guardar:Short title|||Detailed content|||tag1,tag2</TOOL> — Persists a fact, decision, or discovery to your long-term memory DB. Use after: finding a key config, understanding project architecture, fixing a recurring error pattern, learning the user's environment specifics.\n\
        - SEARCH MEMORY: <TOOL>memoria_buscar:query</TOOL> — Full-text searches your memory DB. Use at the START of a task to recall relevant past knowledge before acting. ⚠️ CRITICAL HABIT: when the user asks you to consolidate, unify, merge or clean up memories about a topic, you MUST call this FIRST to get the actual ids — never assume the ids without verifying. Each search hit includes its `id` field; you'll need those for the next two tools.\n\
        - DELETE MEMORY: <TOOL>memoria_eliminar:42</TOOL> or <TOOL>memoria_eliminar:10,11,12</TOOL> — Removes one or several memories by id. Use when a memory is wrong, obsolete, or has been superseded by a newer/better entry. The id comes from a prior memoria_buscar result. Without this tool you can only ADD memories — leading to the bug where 13 partial duplicates accumulated for a single topic.\n\
        - CONSOLIDATE MEMORIES (atomic): <TOOL>memoria_consolidar:id1,id2,id3,id4|||New unified title|||Full unified content covering all the points|||tag1,tag2</TOOL> — Atomically deletes the listed ids AND inserts a new memory in ONE database transaction. Either everything succeeds or nothing changes (rollback). USE THIS — never \"call memoria_guardar then forget to delete the originals\". When the user asks to unify/consolidate/merge memories on a topic: (1) memoria_buscar to get the ids, (2) ONE memoria_consolidar with the full id list and the synthesized content. After consolidation, double-check by running memoria_buscar again and confirming the count to the user.\n\
        - SET PRINCIPLE: <TOOL>principle_set:Short Name|||Full rule text|||scope?|||priority?</TOOL> — Persists a behavioral rule that gets injected into your own future prompts. Use when the user says \"always do X\", \"never do Y\", \"in production, always Z\". scope: 'global' or empty for everywhere; otherwise a host id or project tag. priority: 1-1000 (lower = higher priority, default 100). Example: <TOOL>principle_set:No prod restart|||Never restart any service on PROD-* hosts during 8am-6pm weekdays|||PROD-AD-01|||10</TOOL>. Confirm to the user briefly after saving so they know the rule is now active.\n\
        - DELETE PRINCIPLE: <TOOL>principle_delete:42</TOOL> — Removes a previously-saved rule by id. Use when the user says \"forget about that rule\" or \"don't follow X anymore\".\n\
        - SCHEDULE TASK: <TOOL>schedule_create:Name|||Prompt body to run|||cron_expr|||next_run_iso_or_epoch</TOOL> — Creates a recurring (cron) or one-shot scheduled task that runs unattended. cron_expr is the standard 5-field POSIX format ('0 9 * * 1-5' = 9 AM weekdays); leave empty/'' for one-shot. next_run can be unix epoch SECONDS or ISO 8601. Example for daily morning health check: <TOOL>schedule_create:Daily AM Health|||Run a system health report on PROD-AD-01 and summarize errors from the past 24h|||0 9 * * 1-5|||2026-04-29T09:00:00Z</TOOL>. ALWAYS confirm with the user before persisting if the task looks expensive or runs frequently — they pay the tokens.\n\
        - LIST SCHEDULES: <TOOL>schedule_list</TOOL> — Returns all scheduled tasks with their next/last run + status. Use when the user asks \"what tasks do I have scheduled?\" or \"show my crons\".\n\
        - IMPORTANCE: Include importance:1 (routine), importance:2 (useful), or importance:3 (critical) in the content to prioritize. Default: 1.\n\
        - RULE: Save memories proactively. If you learned something that would help in a future session (server names, project structure, user preferences, working solutions), ALWAYS save it.\n\
        TOOL CHAINING: You are AUTHORIZED to use MULTIPLE tools in a single response to speed up your work. Simply output consecutive <TOOL>...</TOOL> tags. Use this to: search → read → analyze → edit → verify in parallel.\n\
        EDITING FILES: For modifications, ALWAYS prefer <TOOL>editfile</TOOL> over <TOOL>writefile</TOOL>. editfile does surgical find-and-replace — you only need to specify the exact block to change. Use writefile ONLY for creating new files or complete rewrites.\n\
        UX RULE (FILES MODIFIED): Never manually format a list of files you modified. The system interface will automatically group and display 'Files Modified' badges for the user when you use writefile or editfile.\n\
        CRITICAL: NEVER use PowerShell for file I/O. NEVER use Get-Content/Set-Content/Out-File. ALWAYS use these native tools.\n\
        RULE 18 — CODE ANALYSIS & MODIFICATION WORKFLOW:\n\
        When asked to analyze, review, fix, or modify code:\n\
        Step 1: <TOOL>listdir:/path</TOOL> to understand the project.\n\
        Step 2: <TOOL>searchfiles:/path|||keyword</TOOL> to find relevant code.\n\
        Step 3: <TOOL>readfile:/path</TOOL> or <TOOL>readlines:/path:START:COUNT</TOOL> to read the specific file.\n\
        Step 4: Analyze and explain findings wrapped in <THOUGHT>...</THOUGHT>. (Skip THOUGHT entirely if the logic is trivial to save latency).\n\
        Step 5: If asked to fix, use <TOOL>editfile:/path|||OLD_TEXT|||NEW_TEXT</TOOL> to patch the code.\n\
        Step 6: Optionally read back the modified file to verify the change.\n\
        NEVER respond with <TOOL>sysinfo</TOOL> when asked about code. NEVER use <EXECUTE> to read/write files.\n\
        When the user asks 'ves algún problema', 'revisa el código', 'analiza este archivo' → this is CODE ANALYSIS, not system health.\n\
        RULE 18.5 — AUTONOMOUS CODING AGENT CAPABILITIES:\n\
        You are an advanced agentic programmer. You can autonomously write, test, and debug code.\n\
        When tasked with software development (e.g., \"build this feature\", \"fix tests\", \"create a project\"):\n\
        1. Explore the codebase first using your file tracking tools (searchfiles, readfile).\n\
        2. Implement the requested code correctly using editfile or writefile.\n\
        3. ALWAYS verify your changes by executing the relevant build/test commands (e.g., 'cargo check', 'npm test') via <EXECUTE> or <EXECUTE_CMD>. Commands automatically run in the GLOBAL WORKING DIRECTORY (set with <TOOL>cd:path</TOOL>).\n\
        ⚠️ POWERSHELL SYNTAX: NEVER chain commands with `&&` — it fails on PowerShell 5.x. Use `;` to chain (e.g., `Set-Location dir; cargo check`) or better yet, change directory first with <TOOL>cd:path</TOOL> then run <EXECUTE>cargo check</EXECUTE> separately.\n\
        ⚠️ BUILD COMMANDS: Use `--manifest-path` for Cargo instead of cd: `cargo check --manifest-path X:\\path\\Cargo.toml`. For npm: `npm run build --prefix X:\\path`.\n\
        4. If a command fails, autonomously read the error, reason about the fix in <THOUGHT>, edit the file, and run the command again. Repeat this verify-fix loop until successful.\n\
        DO NOT ask the user for permission to fix your own compilation errors. Work autonomously as a senior developer.\n\
        RULE 19 — SELF-AWARENESS & ANTI-HALLUCINATION:\n\
        - Your rules and configuration are embedded in this system prompt. You do NOT have a config file on disk. If asked about your rules, logic, or how to improve your behavior, answer from what you know here — do NOT try to read files.\n\
        - NEVER invent or guess file paths. Use the WORKING DIRECTORY above as your base. When a user mentions a filename without full path, use <TOOL>searchfiles:{cwd}|||filename</TOOL> to locate it FIRST.\n\
        - If a TOOL returns an error (e.g. 'os error 3' = file not found), do NOT retry with a different guessed path. Instead, tell the user the file was not found and ask for the correct path.\n\
        - When asked about yourself, your logic, or how to improve: explain based on your rules above. Suggest improvements as text — do NOT try to modify your own code.\n\
        RULE 20 — LARGE FILE STRATEGY:\n\
        - You possess a massive context window. You are AUTHORIZED to use <TOOL>readfile:/path</TOOL> for any file up to 500KB (including massive files like +page.svelte) to gain full structural understanding.\n\
        - Only for files EXCEEDING 512KB, use <TOOL>searchfiles:/path|keyword</TOOL> followed by <TOOL>readlines:/path:START:COUNT</TOOL>.\n\
        RULE 21: When using the file editing tool, NEVER attempt to replace a single line of code, as duplicate lines may exist and the system will block the operation. Always include at least 2 preceding lines and 2 succeeding lines in your search string context to ensure the match is 100% unique across the entire file.\n\
        RULE 23 — REACT SELF-CORRECTION (MANDATORY on failure): Tool results arrive tagged with [EXIT_CODE: N]. Interpret them as follows: 0 = success (proceed), 1 = soft stderr/warning (inspect, then proceed or adjust), 2 = hard failure (MUST reflect before retrying). If you see `[TOOL FAILURE DETECTED — step N | exit=X]` in your tool results, your NEXT response MUST begin with a <THOUGHT> block (≤80 words) stating: (a) the probable root cause in one sentence, (b) whether the command itself was wrong (syntax, missing dependency, permission, wrong host) or the environment was unexpected, (c) a DIFFERENT next action — NEVER retry the identical command without a concrete change. If you have already failed the same command twice in a row with the same cause, STOP executing and surface a clear summary to the user asking for guidance. This reflection is silent telemetry: keep it inside <THOUGHT> — do not apologize to the user.\n\
        RULE 24 — PLAN/ACT/VERIFY for DESTRUCTIVE actions (MANDATORY): Before executing ANY potentially destructive command, you MUST emit a <PLAN> block instead of a raw <EXECUTE>. Destructive = anything that stops/restarts services, deletes files/keys/users, modifies firewall/network state, kills processes, reboots, uninstalls, or changes persistent configuration. Trigger words (any of these in your intended command): Stop-Service, Restart-Service, Restart-Computer, Remove-*, Disable-*, Set-Service, Set-ItemProperty, Invoke-WmiMethod, shutdown, reboot, reg delete, reg add (HKLM/HKCR), netsh set, sc delete, sc stop, taskkill, kill -9, rm -rf, dd, mkfs, fdisk, format, systemctl stop/disable/mask, iptables -F, Disable-NetAdapter, Reset-*. Format: <PLAN risk=\"high|med|low\" target=\"local|<host_id>\" engine=\"powershell|shell\"><DESC>One-line human description</DESC><CMD>the exact command to run</CMD><VERIFY>a short read-only command that confirms success after CMD runs</VERIFY><ROLLBACK>optional — command that undoes CMD if needed</ROLLBACK></PLAN>. The UI will render this as an interactive card with [Execute] [Dry-Run] [Edit] [Cancel] buttons. Do NOT emit a separate <EXECUTE> alongside <PLAN> — the user clicks Execute to run it. READ-ONLY commands (Get-*, Select-*, ps, ls, df, netstat, grep) do NOT need <PLAN> — keep using <EXECUTE> / <EXECUTE_REMOTE> for those. Example: <PLAN risk=\"high\" target=\"local\" engine=\"powershell\"><DESC>Stop IIS World Wide Web service</DESC><CMD>Stop-Service -Name W3SVC -Force</CMD><VERIFY>Get-Service W3SVC | Select Name,Status</VERIFY><ROLLBACK>Start-Service W3SVC</ROLLBACK></PLAN>.\n\
        RULE 25 — TIERED MEMORY (MemGPT-style): Your memory has THREE tiers. Use them correctly:\n\
        • CORE — Small, always-injected facts shown in the '--- CORE MEMORY (always-on facts) ---' block below (if present). These facts are ALWAYS in your context — do NOT search for them. To ADD a stable fact (env info, hard preferences, critical rules), emit <TOOL>memory_core_set:section|||key|||value</TOOL>. Valid sections: 'user_facts', 'preferences', 'rules', 'environment'. Keep values short (<200 chars). Only promote to CORE facts that are truly always-relevant — everyday findings belong in episodic memory (memoria_guardar). To remove a core fact use <TOOL>memory_core_delete:section|||key</TOOL>.\n\
        • WORKING — Per-session compressed summaries of long agent loops. You do NOT write these directly; the UI may compress raw context into <TOOL>memory_working_append</TOOL> automatically.\n\
        • EPISODIC — Long-term searchable knowledge (memoria_guardar / memoria_buscar / memoria_eliminar / memoria_consolidar / semantic). This is where general discoveries go.\n\
        DECISION GUIDE — WRITE: Is this fact true across ALL future sessions AND short enough to always carry? → CORE. Is it a useful but situational fact? → memoria_guardar. Is it just session scratch? → don't persist.\n\
        DECISION GUIDE — CONSOLIDATE: BEFORE saving any new memory, search first (memoria_buscar) to check if the topic already has 2+ entries. If it does, prefer memoria_consolidar over memoria_guardar — fold the existing entries plus the new info into ONE comprehensive entry, atomically. Goal: at most 1-2 memories per distinct topic. If the user says \"unifica las memorias de X\", \"consolida\", \"limpia memorias duplicadas\", \"reduce a una sola\" → ALWAYS use memoria_consolidar with the full id list (never call memoria_guardar followed by 'I will delete the others later'). After consolidating, briefly state to the user: \"Consolidé N memorias en una nueva (id X). Quedan Y entradas sobre este tema.\" so they can verify.\n\
        RULE 26 — PDF INTELLIGENCE: Users can ingest PDF manuals/documentation using the PDF panel (sidebar). When ingested, content is stored as episodic memories (session_id = 'pdf:{{doc_id}}') AND as semantic vectors. When the user asks about content that may be in a manual or document: (1) Try FTS search first: <TOOL>memoria_buscar:exact terms from likely section</TOOL>. (2) For semantic/conceptual search: <TOOL>pdf_search:natural language question</TOOL>. Each result shows the source filename and the relevant passage. Always cite the document name and section when using PDF content. If no results found, tell the user no PDF has been ingested yet and suggest dragging the file to the PDF panel in the sidebar.\n\
        {core_mem}\n\
        {principles}\n\
        {ctx}
        {hosts}
        The user's name is {uname}. Always address them by name.\nINSTRUCTION: {prompt}",
        lang = lang,
        ctx = context,
        hosts = hosts_context,
        uname = user_name,
        rb = runbooks_info,
        core_mem = core_mem_block,
        principles = principles_block,
        prompt = prompt
    )
}
// ── ASK LUCY (respuesta única) ────────────────────────────────────────────────

#[tauri::command]
pub async fn ask_lucy(
    prompt: String,
    context: Option<String>,
    user_name: String,
    model: String,
    images: Option<Vec<serde_json::Value>>,
    lang: Option<String>,
    hosts_json: Option<String>,
    runbooks_dir: Option<String>,
    max_tokens_override: Option<u32>,
) -> Result<String, String> {
    // NVIDIA NIM models use "owner/model-name" format (e.g. "meta/llama-3.1-70b-instruct").
    // UUID-only strings (8-4-4-4-12 hex) are internal function IDs — never valid model names.
    let is_uuid = {
        let p: Vec<&str> = model.split('-').collect();
        p.len() == 5 && p[0].len() == 8 && p[1].len() == 4 && p[2].len() == 4
            && p[3].len() == 4 && p[4].len() == 12
            && p.iter().all(|s| s.chars().all(|c| c.is_ascii_hexdigit()))
    };
    let is_allowed = !is_uuid && (
        ALLOWED_MODELS.contains(&model.as_str())
        || model.starts_with("local-")
        || (model.contains('/') && !model.contains("..") && model.len() < 120)
    );
    if !is_allowed {
        return Err(format!(
            "Modelo '{}' no permitido. {}",
            model,
            if is_uuid { "Los IDs UUID internos de NVIDIA no son nombres de modelo válidos. Selecciona un modelo del catálogo." }
            else       { "Selecciona un modelo válido desde el selector." }
        ));
    }

    let provider = if model.starts_with("gpt-")    { "openai" }
                   else if model.starts_with("claude-") { "anthropic" }
                   else if model.starts_with("local-")  { "local" }
                   else if model.contains('/')          { "nvidia" }
                   else                                 { "gemini" };

    let entry = Entry::new("LucySysAdmin", &format!("{}_api_key", provider)).map_err(|e| e.to_string())?;
    let api_key = entry.get_password().map_err(|_| format!("API Key para {} no configurada. Configúrala en Ajustes.", provider))?;

    let cwd = crate::state::GLOBAL_CWD.read().map(|c| c.clone()).unwrap_or_else(|_| "C:\\".to_string());
    let user_lang = lang.as_deref().unwrap_or("es-MX");
    let hosts_context = build_hosts_context(hosts_json.as_deref());
    // Cloud models (Gemini/Claude/OpenAI/NVIDIA) get the full v2 prompt with
    // all rules + tools. Local Ollama models get a slim version (≤800 tokens)
    // because small 7-14B models hallucinate when overwhelmed with rules.
    let final_prompt = if provider == "local" {
        crate::commands::prompt_sections::build_local_system_prompt(
            lang_instruction(user_lang),
            context.as_deref().unwrap_or_default(),
            &hosts_context,
            &user_name,
            &prompt,
            &cwd,
        )
    } else {
        build_system_prompt(
            lang_instruction(user_lang),
            context.as_deref().unwrap_or_default(),
            &hosts_context,
            &user_name,
            &prompt,
            &cwd,
            runbooks_dir.as_deref(),
        )
    };

    let req = match provider {
        "openai" => {
            let payload = json!({ "model": model, "messages": [{"role": "user", "content": final_prompt}] });
            HTTP_CLIENT.post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&payload)
        },
        "nvidia" => {
            // NVIDIA NIM — OpenAI-compatible endpoint (build.nvidia.com)
            let payload = json!({
                "model": model,
                "messages": [{"role": "user", "content": final_prompt}],
                "max_tokens": 4096,
                "temperature": 0.2,
                "top_p": 0.9
            });
            HTTP_CLIENT.post("https://nim.api.nvidia.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&payload)
        },
        "local" => {
            // Strip "local-" prefix. num_ctx is adaptive (see adaptive_num_ctx
            // doc): hardcoded 32K used to crash 7B vision models on consumer GPUs.
            // Temperature 0.1 (not 0.2): small local models drift into nonsense
            // quickly above 0.15 — keep deterministic for code-gen quality.
            let actual_model = model.replace("local-", "");
            let ctx_size = adaptive_num_ctx(final_prompt.len());
            let payload = json!({
                "model": actual_model,
                "messages": [{"role": "user", "content": final_prompt}],
                "options": {
                    "temperature": 0.1,
                    "num_ctx": ctx_size,
                    "top_p": 0.9,
                    "repeat_penalty": 1.1
                }
            });
            HTTP_CLIENT.post(&api_key).json(&payload)
        },
        "anthropic" => {
            // Resolve "<id>::effort" → (clean_id, effort). The suffix only
            // affects Opus 4.7 / Sonnet 4.6 / Opus 4.5; on Haiku/Sonnet 4.5
            // the effort is stripped silently (model doesn't accept it).
            let (clean_model, effort) = resolve_anthropic_model(&model);
            let max_tok = get_max_tokens(&clean_model, max_tokens_override);
            // Sprint 1, AI-1 — Split on the cache boundary so the stable half
            // (rules + tools + identity) lands in `system` with cache_control,
            // and the dynamic half (memories + working dir + user prompt)
            // lands in `messages`. Anthropic charges cache writes 1.25× and
            // hits 0.1× — break-even at 2nd use, big savings on long sessions.
            let mut payload = build_anthropic_payload_with_cache(&clean_model, max_tok, &final_prompt, false);
            apply_anthropic_output_config(&mut payload, effort);
            HTTP_CLIENT.post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&payload)
        },
        _ => {
            let mut parts = vec![json!({"text": final_prompt})];
            if let Some(imgs) = images {
                for img in imgs { parts.push(json!({ "inlineData": { "mimeType": img["mimeType"], "data": img["data"] } })); }
            }
            // Resolve "<id>::effort" → (clean_id, generationConfig)
            let (clean_model, gen_cfg) = resolve_gemini_model(&model);
            let mut payload = json!({ "contents": [{ "parts": parts }] });
            apply_gemini_generation_config(&mut payload, gen_cfg);
            // SECURITY: use x-goog-api-key header instead of ?key= query param
            let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent", clean_model);
            HTTP_CLIENT.post(&url).header("x-goog-api-key", &*api_key).json(&payload)
        }
    };

    // Tier-1: send_with_retry handles 429/5xx + transient network errors with
    // exponential backoff (1s → 2s → 4s). Critical for Gemini Flash free tier
    // which rate-limits aggressively, and for Anthropic 529 overload events.
    let res = send_with_retry(req).await?;

    if !res.status().is_success() {
        let status = res.status();
        let err_text = res.text().await.unwrap_or_default();
        return Err(format!("Error API HTTP {}: {}", status, err_text));
    }

    let body_text = res.text().await.map_err(|e| format!("Error al leer body: {}", e))?;
    let v: serde_json::Value = parse_json_capped(&body_text)?;

    if provider == "anthropic" {
        if let Some(reason) = v["stop_reason"].as_str() {
            if reason == "max_tokens" {
                let text = v["content"].get(0).and_then(|c| c["text"].as_str()).unwrap_or("");
                return Ok(format!("{}\n__TRUNCATED__", text));
            }
        }
    }

    let text_result = match provider {
        "openai" | "local" | "nvidia" => v["choices"].get(0).and_then(|c| c["message"]["content"].as_str()),
        "anthropic" => v["content"].get(0).and_then(|c| c["text"].as_str()),
        _ => v["candidates"].get(0).and_then(|c| c["content"]["parts"][0]["text"].as_str())
    };

    if let Some(t) = text_result {
        if let Some((input_tokens, output_tokens)) = match provider {
            "openai" | "local" | "nvidia" => extract_tokens_openai(&v),
            "anthropic" => extract_tokens_anthropic(&v),
            _ => extract_tokens_gemini(&v),
        } {
            let _ = log_usage_internal(&model, input_tokens, output_tokens, "ask_lucy", &user_name).await;
        }
        Ok(t.to_string())
    } else {
        Err(format!("Respuesta API ({}): {}", provider, body_text))
    }
}

// ── ASK LUCY STREAMING (SSE) ──────────────────────────────────────────────────

#[tauri::command]
pub async fn ask_lucy_stream(
    window: tauri::Window,
    request_id: String,
    prompt: String,
    context: Option<String>,
    user_name: String,
    model: String,
    images: Option<Vec<serde_json::Value>>,
    lang: Option<String>,
    hosts_json: Option<String>,
    runbooks_dir: Option<String>,
    max_tokens_override: Option<u32>,
) -> Result<String, String> {
    let is_uuid = {
        let p: Vec<&str> = model.split('-').collect();
        p.len() == 5 && p[0].len() == 8 && p[1].len() == 4 && p[2].len() == 4
            && p[3].len() == 4 && p[4].len() == 12
            && p.iter().all(|s| s.chars().all(|c| c.is_ascii_hexdigit()))
    };
    let is_allowed = !is_uuid && (
        ALLOWED_MODELS.contains(&model.as_str())
        || model.starts_with("local-")
        || (model.contains('/') && !model.contains("..") && model.len() < 120)
    );
    if !is_allowed {
        return Err(format!(
            "Modelo '{}' no permitido. {}",
            model,
            if is_uuid { "ID UUID interno de NVIDIA — selecciona un modelo del catálogo." }
            else       { "Selecciona un modelo válido desde el selector." }
        ));
    }

    let provider = if model.starts_with("gpt-")        { "openai" }
                   else if model.starts_with("claude-") { "anthropic" }
                   else if model.starts_with("local-")  { "local" }
                   else if model.contains('/')          { "nvidia" }
                   else                                 { "gemini" };

    let entry = Entry::new("LucySysAdmin", &format!("{}_api_key", provider)).map_err(|e| e.to_string())?;
    let api_key = entry.get_password().map_err(|_| format!("API Key para {} no configurada. Configúrala en Ajustes.", provider))?;

    let cwd = crate::state::GLOBAL_CWD.read().map(|c| c.clone()).unwrap_or_else(|_| "C:\\".to_string());
    let user_lang = lang.as_deref().unwrap_or("es-MX");
    let hosts_context = build_hosts_context(hosts_json.as_deref());
    // Same provider-aware prompt selection as ask_lucy — see comment there.
    let final_prompt = if provider == "local" {
        crate::commands::prompt_sections::build_local_system_prompt(
            lang_instruction(user_lang),
            context.as_deref().unwrap_or_default(),
            &hosts_context,
            &user_name,
            &prompt,
            &cwd,
        )
    } else {
        build_system_prompt(
            lang_instruction(user_lang),
            context.as_deref().unwrap_or_default(),
            &hosts_context,
            &user_name,
            &prompt,
            &cwd,
            runbooks_dir.as_deref(),
        )
    };

    let req = match provider {
        "openai" => {
            let payload = json!({ "model": model, "messages": [{"role": "user", "content": final_prompt}], "stream": true });
            HTTP_CLIENT.post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&payload)
        },
        "nvidia" => {
            // NVIDIA NIM streaming — OpenAI SSE-compatible
            let payload = json!({
                "model": model,
                "messages": [{"role": "user", "content": final_prompt}],
                "stream": true,
                "max_tokens": 4096,
                "temperature": 0.2,
                "top_p": 0.9
            });
            HTTP_CLIENT.post("https://nim.api.nvidia.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&payload)
        },
        "local" => {
            // Strip "local-" prefix. num_ctx adaptive — see adaptive_num_ctx doc.
            // Temperature 0.1 + repeat_penalty 1.1 — small local models drift
            // into hallucination above 0.15 and loop on identical phrases.
            let actual_model = model.replace("local-", "");
            let ctx_size = adaptive_num_ctx(final_prompt.len());
            let payload = json!({
                "model": actual_model,
                "messages": [{"role": "user", "content": final_prompt}],
                "stream": true,
                "options": {
                    "temperature": 0.1,
                    "num_ctx": ctx_size,
                    "top_p": 0.9,
                    "repeat_penalty": 1.1
                }
            });
            HTTP_CLIENT.post(&api_key).json(&payload)
        },
        "anthropic" => {
            let (clean_model, effort) = resolve_anthropic_model(&model);
            let max_tok = get_max_tokens(&clean_model, max_tokens_override);
            // Sprint 1, AI-1 — Same cache-boundary split for streaming.
            // The "stream": true field is added inside the helper.
            let mut payload = build_anthropic_payload_with_cache(&clean_model, max_tok, &final_prompt, true);
            apply_anthropic_output_config(&mut payload, effort);
            HTTP_CLIENT.post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&payload)
        },
        _ => {
            let mut parts = vec![json!({"text": final_prompt})];
            if let Some(imgs) = images {
                for img in imgs { parts.push(json!({ "inlineData": { "mimeType": img["mimeType"], "data": img["data"] } })); }
            }
            // Resolve "<id>::effort" → (clean_id, generationConfig). The
            // effort suffix only affects Pro (3.1+) where it maps to
            // thinkingConfig.thinkingLevel; other models ignore it.
            let (clean_model, gen_cfg) = resolve_gemini_model(&model);
            let mut payload = json!({ "contents": [{ "parts": parts }] });
            apply_gemini_generation_config(&mut payload, gen_cfg);
            // SECURITY: API key in header, not query string
            let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse", clean_model);
            HTTP_CLIENT.post(&url).header("x-goog-api-key", &*api_key).json(&payload)
        }
    };

    // Tier-1: retry on 429/5xx before opening the byte stream. The streaming
    // happens at the response level (bytes_stream), so retrying the initial
    // send() is safe — we haven't started consuming the body yet.
    let res = send_with_retry(req).await?;

    if !res.status().is_success() {
        let status = res.status();
        let err_text = res.text().await.unwrap_or_default();
        return Err(format!("Error API HTTP {}: {}", status, err_text));
    }

    let mut byte_stream = res.bytes_stream();
    let mut full_text = String::new();
    let mut line_buffer = String::new();
    let chunk_event = format!("lucy-chunk-{}", request_id);
    let mut was_truncated = false;
    let mut input_tokens: u32 = 0;
    let mut output_tokens: u32 = 0;
    let start_time = std::time::Instant::now();
    // Local models (Ollama) need much more time for TTFT on large prompts
    let stream_timeout = if provider == "local" {
        std::time::Duration::from_secs(300) // 5 min for local models
    } else {
        std::time::Duration::from_secs(120) // 2 min for cloud APIs
    };

    // BUG FIX: `stream_done` flag ensures both loops exit when the stream
    // ends. Previously `break` only exited the inner line-parsing loop,
    // leaving the outer byte-stream loop blocked on `.next().await`
    // indefinitely — causing the "Procesando..." indicator to never stop.
    let mut stream_done = false;

    while let Some(chunk) = byte_stream.next().await {
        if stream_done { break; }

        // Check for timeout
        if start_time.elapsed() > stream_timeout {
            eprintln!("[ask_lucy_stream] Timeout after {} seconds waiting for stream", stream_timeout.as_secs());
            let timeout_msg = "\n__STREAM_TIMEOUT__";
            full_text.push_str(timeout_msg);
            let _ = window.emit(&chunk_event, timeout_msg); // Emit timeout marker
            break;
        }

        let bytes = chunk.map_err(|e| format!("Error de stream: {}", e))?;
        line_buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(newline_pos) = line_buffer.find('\n') {
            let line = line_buffer[..newline_pos].trim().to_string();
            line_buffer = line_buffer[newline_pos + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" { stream_done = true; break; }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                    // Check for end-of-stream indicators from various providers
                    let mut stream_ended = false;

                    // Anthropic: check delta.stop_reason
                    if let Some(reason) = v["delta"]["stop_reason"].as_str() {
                        if reason == "max_tokens" {
                            was_truncated = true;
                        }
                        stream_ended = reason == "end_turn" || reason == "stop_sequence" || reason == "max_tokens";
                    }

                    // OpenAI/Local: check choices[0].finish_reason
                    if let Some(reason) = v["choices"].get(0).and_then(|c| c["finish_reason"].as_str()) {
                        if reason == "max_tokens" || reason == "length" {
                            was_truncated = true;
                        }
                        stream_ended = reason == "stop" || reason == "max_tokens" || reason == "length";
                    }

                    // Gemini: check finishReason in candidates
                    if let Some(reason) = v["candidates"].get(0).and_then(|c| c["finishReason"].as_str()) {
                        stream_ended = reason == "STOP" || reason == "MAX_TOKENS";
                        if reason == "MAX_TOKENS" {
                            was_truncated = true;
                        }
                    }

                    if input_tokens == 0 && output_tokens == 0 {
                        if let Some((in_t, out_t)) = match provider {
                            "openai" | "local" | "nvidia" => extract_tokens_openai(&v),
                            "anthropic" => extract_tokens_anthropic(&v),
                            _ => extract_tokens_gemini(&v),
                        } {
                            input_tokens = in_t;
                            output_tokens = out_t;
                        }
                    }

                    let text_chunk = match provider {
                        "openai" | "local" | "nvidia" => v["choices"].get(0).and_then(|c| c["delta"]["content"].as_str()),
                        "anthropic" => v["delta"]["text"].as_str(),
                        _ => v["candidates"].get(0).and_then(|c| c["content"]["parts"][0]["text"].as_str())
                    };

                    if let Some(t) = text_chunk {
                        full_text.push_str(t);
                        let _ = window.emit(&chunk_event, t);
                    }

                    if stream_ended {
                        stream_done = true;
                        break;
                    }
                }
            }
        }
    }

    if was_truncated {
        full_text.push_str("\n__TRUNCATED__");
    }

    if input_tokens > 0 || output_tokens > 0 {
        let _ = log_usage_internal(&model, input_tokens, output_tokens, "ask_lucy_stream", &user_name).await;
    }

    eprintln!("[ask_lucy_stream] Completado: {} bytes, modelo: {}, tokens: in={} out={}", full_text.len(), model, input_tokens, output_tokens);
    Ok(full_text)
}

#[tauri::command]
pub fn log_agent_loop(message: String) {
    use std::io::Write;
    // Rotate at 10 MB, keep 3 historical files (~40 MB max disk footprint).
    crate::utils::logging::rotate_log("lucy_agent_loop.log", 10 * 1024 * 1024, 3);
    let path = crate::utils::logging::get_logs_dir().join("lucy_agent_loop.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), message);
    }
}

// ==========================================
// GENERADOR DIRECTO DE SKILLS (Sin overhead de agente)
// ==========================================
#[tauri::command]
pub async fn generate_skill_template(idea: String, model: String) -> Result<String, String> {
    let provider = if model.starts_with("gpt-")        { "openai" }
                   else if model.starts_with("claude-") { "anthropic" }
                   else if model.starts_with("local-")  { "local" }
                   else if model.contains('/')          { "nvidia" }
                   else                                 { "gemini" };

    let entry = keyring::Entry::new("LucySysAdmin", &format!("{}_api_key", provider)).map_err(|e| e.to_string())?;
    let api_key = entry.get_password().map_err(|_| format!("API Key para {} no configurada.", provider))?;

    let sys_prompt = format!(r#"Eres un generador estricto de JSON. Crea una "skill" de automatización de Windows/Powershell para la siguiente idea: {}.
Responde ÚNICAMENTE con un JSON válido, sin markdown ni backticks, respetando esta estructura:
{{
  "name": "Nombre corto descriptivo (max 40 chars, único, sin espacios al inicio/fin)",
  "category": "quick_cmd | runbook | macro",
  "description": "Qué hace la skill (1-2 líneas claras)",
  "script": "El script PowerShell completo. Usa {{{{paramName}}}} para variables que el usuario debe proporcionar.",
  "parameters": [{{"name":"paramName","type":"string","required":true,"description":"Para qué es"}}],
  "triggers": ["frase natural 1", "frase 2", "alias corto"],
  "tags": ["tag1", "tag2"]
}}
REGLAS:
- "category" SOLO acepta uno de: quick_cmd, runbook, macro. Usa "quick_cmd" para comandos de una línea, "runbook" para procedimientos multi-paso, "macro" para automatizaciones complejas con condicionales.
- Si la idea no requiere parámetros, devuelve "parameters": [].
- "triggers" debe tener al menos 2 frases en español natural que un SysAdmin diría para invocar esta skill.
- NO uses markdown, NO uses backticks, SOLO el objeto JSON crudo."#, idea);

    let req = match provider {
        "openai" => {
            let payload = serde_json::json!({ "model": model, "messages": [{"role": "user", "content": sys_prompt}] });
            crate::state::HTTP_CLIENT.post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&payload)
        },
        "nvidia" => {
            let payload = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": sys_prompt}],
                "max_tokens": 1024,
                "temperature": 0.1
            });
            crate::state::HTTP_CLIENT.post("https://nim.api.nvidia.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&payload)
        },
        "anthropic" => {
            let (clean_model, effort) = resolve_anthropic_model(&model);
            let mut payload = serde_json::json!({ "model": clean_model, "max_tokens": 1024, "messages": [{"role": "user", "content": sys_prompt}] });
            apply_anthropic_output_config(&mut payload, effort);
            crate::state::HTTP_CLIENT.post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&payload)
        },
        _ => { // gemini — SECURITY: key in header, not URL
            let (clean_model, gen_cfg) = resolve_gemini_model(&model);
            let mut payload = serde_json::json!({ "contents": [{ "parts": [{"text": sys_prompt}] }] });
            apply_gemini_generation_config(&mut payload, gen_cfg);
            let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent", clean_model);
            crate::state::HTTP_CLIENT.post(&url).header("x-goog-api-key", &*api_key).json(&payload)
        }
    };

    // Hard timeout so the spinner never hangs forever — fail-fast with a useful message.
    // send_with_retry adds up to ~7s of backoff on transient failures (429/5xx/network)
    // before giving up, well within the 25s per-attempt budget.
    let req = req.timeout(std::time::Duration::from_secs(25));
    let res = send_with_retry(req).await.map_err(|e| {
        // send_with_retry already includes attempt count; just add provider context.
        format!("Proveedor '{}': {}", provider, e)
    })?;
    let status = res.status();
    let body_text = res.text().await.map_err(|e| e.to_string())?;

    if status.is_success() {
        if let Ok(v) = parse_json_capped(&body_text) {
            let text = match provider {
                "openai" | "local" | "nvidia" => v["choices"].get(0).and_then(|c| c["message"]["content"].as_str()),
                "anthropic" => v["content"].get(0).and_then(|c| c["text"].as_str()),
                _ => v["candidates"].get(0).and_then(|c| c["content"]["parts"][0]["text"].as_str())
            };
            if let Some(t) = text {
                let trimmed = t.trim();
                if trimmed.is_empty() {
                    return Err(format!("El proveedor '{}' devolvió respuesta vacía. Intenta de nuevo o cambia de modelo.", provider));
                }
                return Ok(trimmed.to_string());
            }
        }
        // Couldn't extract content — return the raw body so the user can see what came back.
        Err(format!("Respuesta inesperada de '{}' (sin contenido reconocible): {}", provider, &body_text[..body_text.len().min(400)]))
    } else {
        // Surface common API errors with actionable hints
        let hint = if status.as_u16() == 401 || status.as_u16() == 403 {
            format!(" → API Key inválida o sin permisos. Configúrala en Settings → API Key.")
        } else if status.as_u16() == 429 {
            " → Rate limit alcanzado. Espera un momento o cambia de modelo.".to_string()
        } else if status.as_u16() >= 500 {
            format!(" → El servidor de '{}' está caído. Intenta otro proveedor.", provider)
        } else {
            String::new()
        };
        Err(format!("API '{}' respondió {} {}{}", provider, status.as_u16(), &body_text[..body_text.len().min(300)], hint))
    }
}

// ── NVIDIA NIM — listar modelos disponibles ───────────────────────────────────

/// Consulta el catálogo de modelos de NVIDIA NIM (build.nvidia.com).
/// Requiere que la NVIDIA API key esté configurada en Ajustes.
#[tauri::command]
pub async fn list_nvidia_models() -> Result<Vec<String>, String> {
    let entry = keyring::Entry::new("LucySysAdmin", "nvidia_api_key")
        .map_err(|e| e.to_string())?;
    let api_key = entry.get_password()
        .map_err(|_| "NVIDIA API Key no configurada. Consíguela gratis en build.nvidia.com".to_string())?;

    let res = crate::state::HTTP_CLIENT
        .get("https://nim.api.nvidia.com/v1/models")
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Error de red al consultar NVIDIA NIM: {}", e))?;

    if !res.status().is_success() {
        let code = res.status().as_u16();
        return Err(match code {
            401 => "NVIDIA API Key inválida o sin permisos. Verifica en build.nvidia.com".to_string(),
            429 => "Rate limit de NVIDIA NIM alcanzado. Intenta más tarde.".to_string(),
            _   => format!("NVIDIA NIM respondió HTTP {}", code),
        });
    }

    let json: serde_json::Value = res.json().await
        .map_err(|e| format!("Respuesta JSON inválida de NVIDIA NIM: {}", e))?;

    let models = json["data"].as_array()
        .ok_or_else(|| "Respuesta NVIDIA sin campo 'data'".to_string())?;

    // NVIDIA NIM /v1/models returns objects where:
    //   - "id"   may be an internal UUID (e.g. "b0fcd392-e905-4ab4-8eb9-...")
    //   - "name" (or "root") contains the real model path (e.g. "meta/llama-3.1-70b-instruct")
    // Strategy: prefer "name", then "root", then "id" — but only if it contains '/'
    // which is the canonical format for NIM model identifiers.
    let mut names: Vec<String> = models
        .iter()
        .filter_map(|m| {
            // Try name → root → id (only if it looks like "owner/model")
            m["name"].as_str()
                .filter(|s| s.contains('/'))
                .or_else(|| m["root"].as_str().filter(|s| s.contains('/')))
                .or_else(|| m["id"].as_str().filter(|s| s.contains('/')))
                .map(String::from)
        })
        .collect();

    // Deduplicate and sort
    names.sort();
    names.dedup();

    if names.is_empty() {
        // Fallback: return the raw IDs so the user can see what the API sent
        // (helps diagnose unexpected response formats)
        eprintln!("[nvidia] /v1/models devolvió 0 IDs con formato owner/model. Respuesta raw: {}",
            serde_json::to_string(&json).unwrap_or_default().chars().take(500).collect::<String>());
        return Err(
            "NVIDIA NIM no devolvió modelos en formato esperado (owner/model). \
             Verifica que tu API key tenga acceso a modelos en build.nvidia.com".to_string()
        );
    }

    Ok(names)
}

#[cfg(test)]
mod gemini_resolver_tests {
    use super::{apply_gemini_generation_config, resolve_gemini_model};

    #[test]
    fn legacy_3_flash_preview_upgrades_to_3_5_flash() {
        let (id, cfg) = resolve_gemini_model("gemini-3-flash-preview");
        assert_eq!(id, "gemini-3.5-flash");
        assert!(cfg.is_none());
    }

    #[test]
    fn pro_high_strips_suffix_and_emits_high_thinking_level() {
        let (id, cfg) = resolve_gemini_model("gemini-3.1-pro-preview::high");
        assert_eq!(id, "gemini-3.1-pro-preview");
        let cfg = cfg.expect("expected generationConfig for ::high");
        assert_eq!(cfg["thinkingConfig"]["thinkingLevel"], "high");
    }

    #[test]
    fn pro_medium_strips_suffix_and_emits_medium_thinking_level() {
        let (id, cfg) = resolve_gemini_model("gemini-3.1-pro-preview::medium");
        assert_eq!(id, "gemini-3.1-pro-preview");
        let cfg = cfg.expect("expected generationConfig for ::medium");
        assert_eq!(cfg["thinkingConfig"]["thinkingLevel"], "medium");
    }

    #[test]
    fn pro_with_spanish_alto_alias_works() {
        let (id, cfg) = resolve_gemini_model("gemini-3.1-pro-preview::alto");
        assert_eq!(id, "gemini-3.1-pro-preview");
        assert_eq!(cfg.expect("cfg")["thinkingConfig"]["thinkingLevel"], "high");
    }

    #[test]
    fn unknown_effort_strips_suffix_but_no_cfg() {
        let (id, cfg) = resolve_gemini_model("gemini-3.1-pro-preview::ludicrous");
        assert_eq!(id, "gemini-3.1-pro-preview");
        assert!(cfg.is_none(), "unknown effort should NOT emit a thinkingLevel");
    }

    #[test]
    fn non_pro_models_pass_through_unchanged() {
        let (id, cfg) = resolve_gemini_model("gemini-3.5-flash");
        assert_eq!(id, "gemini-3.5-flash");
        assert!(cfg.is_none());

        let (id2, cfg2) = resolve_gemini_model("gemini-3.1-flash-lite");
        assert_eq!(id2, "gemini-3.1-flash-lite");
        assert!(cfg2.is_none());
    }

    #[test]
    fn apply_config_merges_into_existing_payload() {
        let mut payload = serde_json::json!({ "contents": [{"parts": [{"text": "hi"}]}] });
        let cfg = Some(serde_json::json!({ "thinkingConfig": { "thinkingLevel": "high" } }));
        apply_gemini_generation_config(&mut payload, cfg);
        assert_eq!(payload["generationConfig"]["thinkingConfig"]["thinkingLevel"], "high");
        // Original contents are preserved
        assert_eq!(payload["contents"][0]["parts"][0]["text"], "hi");
    }

    #[test]
    fn apply_config_with_none_is_noop() {
        let mut payload = serde_json::json!({ "contents": [] });
        let before = payload.clone();
        apply_gemini_generation_config(&mut payload, None);
        assert_eq!(payload, before);
    }
}

#[cfg(test)]
mod anthropic_resolver_tests {
    use super::{
        apply_anthropic_output_config,
        resolve_anthropic_model,
        get_cache_stats,
        extract_tokens_anthropic,
    };

    #[test]
    fn opus_47_xhigh_is_accepted() {
        let (id, eff) = resolve_anthropic_model("claude-opus-4-7::xhigh");
        assert_eq!(id, "claude-opus-4-7");
        assert_eq!(eff, Some("xhigh"));
    }

    #[test]
    fn opus_47_all_five_levels_accepted() {
        for level in ["low", "medium", "high", "xhigh", "max"] {
            let (id, eff) = resolve_anthropic_model(&format!("claude-opus-4-7::{}", level));
            assert_eq!(id, "claude-opus-4-7");
            assert_eq!(eff, Some(level), "expected effort {} to pass for Opus 4.7", level);
        }
    }

    #[test]
    fn sonnet_46_rejects_xhigh_but_accepts_max() {
        // Per docs Sonnet 4.6 has no xhigh tier; strip silently.
        let (id, eff) = resolve_anthropic_model("claude-sonnet-4-6::xhigh");
        assert_eq!(id, "claude-sonnet-4-6");
        assert_eq!(eff, None);

        let (_, eff2) = resolve_anthropic_model("claude-sonnet-4-6::max");
        assert_eq!(eff2, Some("max"));
    }

    #[test]
    fn haiku_does_not_support_effort() {
        // Even with a valid level, Haiku gets stripped because the API doesn't accept it.
        let (id, eff) = resolve_anthropic_model("claude-haiku-4-5::high");
        assert_eq!(id, "claude-haiku-4-5");
        assert_eq!(eff, None);
    }

    #[test]
    fn spanish_aliases_work() {
        let (_, e1) = resolve_anthropic_model("claude-opus-4-7::alto");
        assert_eq!(e1, Some("high"));
        let (_, e2) = resolve_anthropic_model("claude-opus-4-7::medio");
        assert_eq!(e2, Some("medium"));
        let (_, e3) = resolve_anthropic_model("claude-opus-4-7::bajo");
        assert_eq!(e3, Some("low"));
        let (_, e4) = resolve_anthropic_model("claude-opus-4-7::extra-alto");
        assert_eq!(e4, Some("xhigh"));
    }

    #[test]
    fn no_suffix_passes_through_unchanged() {
        let (id, eff) = resolve_anthropic_model("claude-opus-4-7");
        assert_eq!(id, "claude-opus-4-7");
        assert_eq!(eff, None);
    }

    #[test]
    fn apply_output_config_injects_effort() {
        let mut payload = serde_json::json!({
            "model": "claude-opus-4-7",
            "max_tokens": 4096,
            "messages": []
        });
        apply_anthropic_output_config(&mut payload, Some("medium"));
        assert_eq!(payload["output_config"]["effort"], "medium");
        // Original fields preserved
        assert_eq!(payload["model"], "claude-opus-4-7");
        assert_eq!(payload["max_tokens"], 4096);
    }

    #[test]
    fn apply_output_config_none_is_noop() {
        let mut payload = serde_json::json!({ "model": "claude-opus-4-7" });
        let before = payload.clone();
        apply_anthropic_output_config(&mut payload, None);
        assert_eq!(payload, before);
    }

    #[test]
    fn unknown_effort_strips_to_none() {
        let (id, eff) = resolve_anthropic_model("claude-opus-4-7::ludicrous");
        assert_eq!(id, "claude-opus-4-7");
        assert_eq!(eff, None);
    }

    // ── Sprint 5, TEST-3 — get_cache_stats (UI-7) ───────────────────────
    // The accumulator is a process-wide Mutex<CacheStats>. We can't reset
    // it between tests, but we CAN make assertions that hold regardless of
    // what other tests ran first — e.g. "calling extract_tokens_anthropic
    // with cache fields strictly increases the cache_read_total".

    /// CONTRACT: get_cache_stats returns a usable struct, even before any
    /// anthropic call has been made (avoids panic on first read).
    #[test]
    fn get_cache_stats_default_is_zeroed_at_least_once() {
        let s = get_cache_stats();
        // We can't assert ALL fields are 0 because earlier tests may have
        // bumped the counters. We just assert the read itself succeeds and
        // returns sane (non-negative-by-type) values. Counters are u64 so
        // negativity is impossible — the real check is "doesn't panic".
        let _ = s.calls_total_anthropic;
        let _ = s.cache_read_total;
        let _ = s.cache_creation_total;
    }

    /// CONTRACT: a response WITHOUT cache fields still increments
    /// calls_total_anthropic but NOT cache_creation/read. This separation
    /// is what lets the footer say "N of M calls used the cache" — if every
    /// call counted as cache activity the ratio would be useless.
    #[test]
    fn extract_tokens_no_cache_fields_bumps_total_only() {
        let before = get_cache_stats();
        let body = serde_json::json!({
            "usage": { "input_tokens": 100, "output_tokens": 50 }
        });
        let _ = extract_tokens_anthropic(&body);
        let after = get_cache_stats();
        assert_eq!(after.calls_total_anthropic, before.calls_total_anthropic + 1,
            "any anthropic call should bump calls_total");
        assert_eq!(after.calls_with_cache_activity, before.calls_with_cache_activity,
            "no cache fields → calls_with_cache_activity unchanged");
        assert_eq!(after.cache_creation_total, before.cache_creation_total,
            "no cache fields → cache_creation_total unchanged");
        assert_eq!(after.cache_read_total, before.cache_read_total,
            "no cache fields → cache_read_total unchanged");
    }

    /// CONTRACT: a response WITH cache_read fields bumps both counters and
    /// the activity counter. This is the happy path AI-1 was built for.
    #[test]
    fn extract_tokens_with_cache_read_bumps_cache_counters() {
        let before = get_cache_stats();
        let body = serde_json::json!({
            "usage": {
                "input_tokens": 80,
                "output_tokens": 40,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 4000
            }
        });
        let _ = extract_tokens_anthropic(&body);
        let after = get_cache_stats();
        assert_eq!(after.calls_with_cache_activity,
                   before.calls_with_cache_activity + 1,
            "cache_read > 0 must bump calls_with_cache_activity");
        assert_eq!(after.cache_read_total, before.cache_read_total + 4000,
            "cache_read_total should accumulate the 4000 reported");
    }

    /// CONTRACT: missing input_tokens (malformed response) returns None
    /// rather than panicking. The Anthropic message_start SSE event has
    /// `output_tokens: 1` but a malformed cache event might miss input.
    #[test]
    fn extract_tokens_missing_input_returns_none() {
        let body = serde_json::json!({ "usage": { "output_tokens": 50 } });
        let r = extract_tokens_anthropic(&body);
        assert!(r.is_none(), "missing input_tokens must return None, not panic");
    }
}
