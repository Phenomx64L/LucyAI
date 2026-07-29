import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

// Reactive store: starts with one fallback "custom" entry, gets replaced
// when refreshLocalModels() succeeds.
export const localModels = writable([
    { id: "local-custom", icon: "", nameEn: "Local LLM — Custom Endpoint", nameEs: "LLM Local — Endpoint Personalizado" }
]);

// 🟢 / 🔴 status of local Ollama endpoint
export const ollamaOnline = writable(false);

/** Pick a friendly icon based on model family. */
// Pick a Lucy-style geometric glyph for an auto-detected Ollama model.
// Heuristic: largest family models get ◆, mid-tier ◇, code-specialized ⌬,
// preview/tiny ◌, generic local fallback ⌂. Keeps the dropdown coherent
// with the cloud-model icons in LLM_GROUPS above.
function pickIcon(name) {
    const n = name.toLowerCase();
    if (n.includes('codellama') || n.includes('coder') || n.includes('codestral')) return '⌬';
    if (n.includes('nemotron') || n.includes('405b') || n.includes('340b')) return '◆';
    if (n.includes('70b') || n.includes('72b') || n.includes('120b'))         return '◆';
    if (n.includes('mini') || n.includes('1.5b') || n.includes('3b'))         return '◌';
    if (n.includes('llama')    || n.includes('qwen')   || n.includes('deepseek') ||
        n.includes('mistral')  || n.includes('mixtral')|| n.includes('phi')    ||
        n.includes('gemma'))                                                  return '◇';
    return '⌂'; // generic local
}

/** Query Ollama for installed models and update the store. */
export async function refreshLocalModels() {
    try {
        const names = await invoke('list_local_models');
        ollamaOnline.set(Array.isArray(names));
        if (Array.isArray(names) && names.length > 0) {
            const opts = names.map(n => ({
                id: 'local-' + n,
                icon: pickIcon(n),
                nameEn: `${n} — Local (Ollama)`,
                nameEs: `${n} — Local (Ollama)`
            }));
            // Always keep "custom" as a manual fallback
            opts.push({ id: "local-custom", icon: "⌂", nameEn: "Custom Endpoint", nameEs: "Endpoint Personalizado" });
            localModels.set(opts);
            // Sync into LLM_GROUPS so existing consumers (getModelDescription, etc.) see them too.
            // BUG FIX: was matching by label.includes('Locales') (Spanish word) but the
            // actual group label is "── Local Ollama (Self-Hosted) ──" (English). The find
            // returned undefined and the dropdown never received the detected models.
            // Now matches by the stable `provider` field, same pattern as refreshNvidiaModels.
            const grp = LLM_GROUPS.find(g => g.provider === 'ollama');
            if (grp) grp.options = opts;
            return opts;
        }
    } catch (e) {
        // Ollama not running or endpoint not configured — keep fallback.
        ollamaOnline.set(false);
        console.warn('[localModels] refresh failed:', e);
    }
    return get(localModels);
}

// NVIDIA NIM writable store — populated dynamically by refreshNvidiaModels()
export const nvidiaModels = writable([]);
export const nvidiaConfigured = writable(false);

/** UUID regex — NVIDIA /v1/models returns internal function IDs we must discard. */
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** Fetch available NVIDIA NIM models and update the store + LLM_GROUPS. */
export async function refreshNvidiaModels() {
    try {
        const names = await invoke('list_nvidia_models');
        nvidiaConfigured.set(true);
        if (Array.isArray(names) && names.length > 0) {
            // Filter out any UUID-style IDs that slipped through
            const valid = names.filter(n => n.includes('/') && !UUID_RE.test(n));
            const opts = (valid.length > 0 ? valid : names).map(n => {
                const parts = n.split('/');
                const shortName = parts[parts.length - 1] || n;
                return {
                    id: n,
                    icon: pickIcon(n),
                    nameEn: `${shortName} — NVIDIA NIM`,
                    nameEs: `${shortName} — NVIDIA NIM`
                };
            });
            nvidiaModels.set(opts);
            const grp = LLM_GROUPS.find(g => g.provider === 'nvidia');
            if (grp) grp.options = opts;
            return opts;
        }
    } catch (_) {
        nvidiaConfigured.set(false);
    }
    return [];
}

export const LLM_GROUPS = [
    {
        label: "── Anthropic Claude (Native Computer Use) ──",
        provider: "anthropic",
        credential_key: "anthropic_api_key",
        options: [
            // Claude 5 lineup — Opus 5 (flagship) / Sonnet 5 / Fable 5 (top tier).
            // Opus 4.8 and Sonnet 4.6 stay as the previous generation. All 1M ctx
            // except Haiku 4.5 (200K).
            //
            // Effort levels (from platform.claude.com/docs/en/build-with-claude/effort):
            //   • Opus 5      supports low/medium/high/xhigh/max
            //   • Sonnet 5    supports low/medium/high/xhigh/max  (first Sonnet with xhigh)
            //   • Fable 5     supports low/medium/high/xhigh/max
            //   • Opus 4.8    supports low/medium/high/xhigh/max
            //   • Sonnet 4.6  supports low/medium/high/max        (no xhigh)
            //   • Opus 4.5    supports low/medium/high            (no xhigh, no max)
            //   • Haiku 4.5   does NOT support effort (lightweight tier)
            //   • Sonnet 4.5  legacy — uses manual thinking, no effort param
            //
            // ── Two API facts that bite if you forget them ──
            // 1. Opus 5 THINKS BY DEFAULT. Omitting the `thinking` parameter runs
            //    adaptive thinking (Opus 4.8 ran without). `max_tokens` caps
            //    thinking + response together, so a budget sized for 4.8's answer
            //    can truncate here. Disabling thinking is accepted only at effort
            //    `high` or below — pairing it with xhigh/max returns a 400.
            // 2. Fable 5 thinks ALWAYS — any explicit `thinking` config is a 400 —
            //    and requires 30-day data retention. An org on zero data retention
            //    gets a 400 on EVERY Fable request, with nothing wrong in the body.
            //
            // Lucy sends neither `thinking` nor `temperature`/`top_p` to Anthropic
            // (see build_anthropic_payload_with_cache), so both are safe today. If
            // anyone adds them, read the two notes above first: those parameters
            // are rejected outright on Opus 5 / 4.8 / 4.7 / Fable 5.
            //
            // We surface effort as a "<id>::<level>" suffix; the backend resolver
            // strips it and adds `output_config.effort` to the Anthropic payload.
            // See ai.rs → resolve_anthropic_model() + apply_anthropic_output_config().
            //
            // ── Lucy's geometric icon system (replaces generic emoji) ──
            // The whole UI uses Tabler-style strokes + Unicode geometric
            // shapes (◆ ◇ ◐ ◯ ◎ ▸ ⌬ ▫). Within each provider:
            //   ◆ = flagship / max-intelligence tier
            //   ◇ = mid-tier / balanced
            //   ▸ = fast / instant tier
            //   ▫ = legacy (kept for backward-compat with old chats)
            // Effort-suffix variants share the same shape — the tier ratio
            // is what matters at a glance, the effort level is in the text.
            //
            // ── Opus 5 (flagship) — $5/$25 per 1M. Start at xhigh for coding and
            //    agentic work, high for everything else. Note low/medium punch far
            //    above their weight on this model — worth trying before paying for
            //    xhigh on routine work.
            { id: "claude-opus-5::xhigh",  icon: "◆", nameEn: "Claude Opus 5 — Extra High (coding/agentic)", nameEs: "Claude Opus 5 — Extra Alto (coding/agéntico)" },
            { id: "claude-opus-5::high",   icon: "◆", nameEn: "Claude Opus 5 — High (default)",              nameEs: "Claude Opus 5 — Alto (predeterminado)" },
            { id: "claude-opus-5::medium", icon: "◆", nameEn: "Claude Opus 5 — Medium (cost-sensitive)",     nameEs: "Claude Opus 5 — Medio (sensible al costo)" },
            { id: "claude-opus-5::max",    icon: "◆", nameEn: "Claude Opus 5 — Max (frontier problems)",     nameEs: "Claude Opus 5 — Max (problemas frontera)" },
            // ── Sonnet 5 — near-Opus quality on coding/agentic at Sonnet cost
            //    ($3/$15 per 1M). Default effort is high; xhigh for the hardest work.
            { id: "claude-sonnet-5::high",   icon: "◇", nameEn: "Claude Sonnet 5 — High (default)",              nameEs: "Claude Sonnet 5 — Alto (predeterminado)" },
            { id: "claude-sonnet-5::xhigh",  icon: "◇", nameEn: "Claude Sonnet 5 — Extra High (hardest tasks)",  nameEs: "Claude Sonnet 5 — Extra Alto (tareas más duras)" },
            { id: "claude-sonnet-5::medium", icon: "◇", nameEn: "Claude Sonnet 5 — Medium (cost-saving)",        nameEs: "Claude Sonnet 5 — Medio (ahorro de costo)" },
            { id: "claude-sonnet-5::low",    icon: "◇", nameEn: "Claude Sonnet 5 — Low (speed-sensitive)",       nameEs: "Claude Sonnet 5 — Bajo (sensible a latencia)" },
            // ── Fable 5 — most capable, and the most expensive: $10/$50 per 1M,
            //    DOUBLE Opus 5. Thinking is always on and cannot be configured, and
            //    the org must allow 30-day data retention or every request 400s.
            { id: "claude-fable-5::xhigh", icon: "◆", nameEn: "Claude Fable 5 — Extra High (2× Opus 5 cost)", nameEs: "Claude Fable 5 — Extra Alto (2× el costo de Opus 5)" },
            { id: "claude-fable-5::high",  icon: "◆", nameEn: "Claude Fable 5 — High (2× Opus 5 cost)",       nameEs: "Claude Fable 5 — Alto (2× el costo de Opus 5)" },
            // ── Previous generation — kept for pinning and for existing chats ──
            { id: "claude-opus-4-8::high",     icon: "◇", nameEn: "Claude Opus 4.8 — High (previous gen)",     nameEs: "Claude Opus 4.8 — Alto (generación anterior)" },
            { id: "claude-opus-4-8::xhigh",    icon: "◇", nameEn: "Claude Opus 4.8 — Extra High (previous gen)", nameEs: "Claude Opus 4.8 — Extra Alto (generación anterior)" },
            { id: "claude-sonnet-4-6::medium", icon: "◇", nameEn: "Claude Sonnet 4.6 — Medium (previous gen)", nameEs: "Claude Sonnet 4.6 — Medio (generación anterior)" },
            { id: "claude-sonnet-4-6::high",   icon: "◇", nameEn: "Claude Sonnet 4.6 — High (previous gen)",   nameEs: "Claude Sonnet 4.6 — Alto (generación anterior)" },
            // ── Haiku 4.5 — no effort param available ──
            { id: "claude-haiku-4-5",  icon: "▸", nameEn: "Claude Haiku 4.5 — Fast & Efficient",         nameEs: "Claude Haiku 4.5 — Rápido y Eficiente" },
            // Legacy — kept for backward compat with existing chats / runbooks
            { id: "claude-opus-4-7::high", icon: "▫", nameEn: "Claude Opus 4.7 — Legacy",                nameEs: "Claude Opus 4.7 — Legado" },
            { id: "claude-sonnet-4-5", icon: "▫", nameEn: "Claude Sonnet 4.5 — Legacy",                  nameEs: "Claude Sonnet 4.5 — Legado" },
        ]
    },
    {
        label: "── Google Gemini Vision ──",
        provider: "gemini",
        credential_key: "gemini_api_key",
        options: [
            // Gemini 3.x family — refreshed May 2026 from ai.google.dev/gemini-api/docs.
            //
            // ── Lucy's geometric icon system ──
            //   ◆ = flagship (Pro tier)
            //   ◐ = balanced fast (Flash GA)
            //   ◯ = lite (lowest cost workhorse)
            //   ◎ = preview / experimental (concentric "target")
            //
            // Reasoning effort: Gemini 3.x Pro accepts a `thinkingConfig.thinkingLevel`
            // hint (low|medium|high). We expose Pro as TWO entries — Medium (default,
            // faster) and High (deeper, slower) — so the user picks the budget upfront.
            // The "::high" / "::medium" suffix is stripped server-side and translated
            // to the right thinkingConfig payload. See ai.rs → resolve_gemini_model().
            //
            // Verified 2026-07-29 against ai.google.dev/gemini-api/docs/models
            // and .../pricing. Prices are per 1M tokens, paid tier:
            //   • gemini-3.6-flash        GA      $1.50 / $7.50   ← newest
            //   • gemini-3.5-flash        GA      $1.50 / $9.00
            //   • gemini-3.5-flash-lite   GA      $0.30 / $2.50
            //   • gemini-3.1-flash-lite   GA      $0.25 / $1.50
            //   • gemini-3.1-pro-preview  PREVIEW $2.00 / $12.00  (≤200k prompt;
            //     $4/$18 above that — model-pricing.ts quotes the higher-volume
            //     tier only implicitly, see the note there)
            { id: "gemini-3.6-flash",               icon: "◐", nameEn: "Gemini 3.6 Flash — Agentic & multimodal (newest)", nameEs: "Gemini 3.6 Flash — Agéntico y multimodal (más reciente)" },
            { id: "gemini-3.5-flash",               icon: "◐", nameEn: "Gemini 3.5 Flash — Sustained frontier performance", nameEs: "Gemini 3.5 Flash — Rendimiento de frontera sostenido" },
            { id: "gemini-3.1-pro-preview::high",   icon: "◆", nameEn: "Gemini 3.1 Pro — High Effort (deep reasoning)", nameEs: "Gemini 3.1 Pro — Esfuerzo Alto (razonamiento profundo)" },
            { id: "gemini-3.1-pro-preview::medium", icon: "◆", nameEn: "Gemini 3.1 Pro — Medium Effort (balanced)",     nameEs: "Gemini 3.1 Pro — Esfuerzo Medio (balanceado)" },
            { id: "gemini-3.5-flash-lite",          icon: "◯", nameEn: "Gemini 3.5 Flash-Lite — High-throughput",         nameEs: "Gemini 3.5 Flash-Lite — Alto rendimiento" },
            { id: "gemini-3.1-flash-lite",          icon: "◯", nameEn: "Gemini 3.1 Flash-Lite — Cheapest workhorse",      nameEs: "Gemini 3.1 Flash-Lite — El más barato" },
            { id: "gemini-3.1-flash-lite-preview",  icon: "◎", nameEn: "Gemini 3.1 Flash-Lite Preview",                   nameEs: "Gemini 3.1 Flash-Lite Vista Previa" },
        ]
    },
    {
        label: "── OpenAI GPT Vision ──",
        provider: "openai",
        credential_key: "openai_api_key",
        options: [
            // ── Lucy's geometric icon system ──
            //   ◆ = frontier (top-tier reasoning)
            //   ◇ = balanced mid-tier
            //   ◯ = budget tier
            //   ▫ = legacy (kept for backward compat with saved chats)
            //
            // Verified 2026-07-29 against developers.openai.com/api/docs/models
            // and .../pricing. The GPT-5.6 family replaced the 5.5/5.4/5.3
            // lineup wholesale — three sizes of one model rather than separate
            // instant/mini/nano/codex variants. All three are 1.05M context and
            // 128K max output. Prices per 1M tokens, standard tier:
            //   • gpt-5.6-sol    $5.00 / $30.00
            //   • gpt-5.6-terra  $2.50 / $15.00
            //   • gpt-5.6-luna   $1.00 / $6.00
            // (Batch and Flex halve those; Priority doubles them. Regional
            // data-residency endpoints add 10% for models released after
            // 2026-03-05 — Lucy does not use either, so the standard rate is
            // what model-pricing.ts quotes.)
            { id: "gpt-5.6-sol",   icon: "◆", nameEn: "GPT-5.6 Sol — Frontier (complex professional work)", nameEs: "GPT-5.6 Sol — Frontera (trabajo profesional complejo)" },
            { id: "gpt-5.6-terra", icon: "◇", nameEn: "GPT-5.6 Terra — Balanced intelligence/cost",         nameEs: "GPT-5.6 Terra — Equilibrio inteligencia/costo" },
            { id: "gpt-5.6-luna",  icon: "◯", nameEn: "GPT-5.6 Luna — Cost-sensitive workloads",            nameEs: "GPT-5.6 Luna — Cargas sensibles al costo" },
            // Legacy — no longer listed by OpenAI as current. Kept selectable so
            // saved chats and runbooks pinned to them still resolve; expect them
            // to 404 once OpenAI retires the endpoints.
            { id: "gpt-5.5",         icon: "▫", nameEn: "GPT-5.5 — Legacy",         nameEs: "GPT-5.5 — Legado" },
            { id: "gpt-5.5-instant", icon: "▫", nameEn: "GPT-5.5 Instant — Legacy", nameEs: "GPT-5.5 Instant — Legado" },
            { id: "gpt-5.4-mini",    icon: "▫", nameEn: "GPT-5.4 Mini — Legacy",    nameEs: "GPT-5.4 Mini — Legado" },
            { id: "gpt-5.4-nano",    icon: "▫", nameEn: "GPT-5.4 Nano — Legacy",    nameEs: "GPT-5.4 Nano — Legado" },
            { id: "gpt-5.3-codex",   icon: "▫", nameEn: "GPT-5.3 Codex — Legacy",   nameEs: "GPT-5.3 Codex — Legado" },
            { id: "gpt-4o",          icon: "▫", nameEn: "GPT-4o — Legacy",          nameEs: "GPT-4o — Legado" },
            { id: "gpt-4o-mini",     icon: "▫", nameEn: "GPT-4o Mini — Legacy",     nameEs: "GPT-4o Mini — Legado" },
        ]
    },
    {
        label: "── xAI Grok ──",
        provider: "xai",
        credential_key: "xai_api_key",
        options: [
            // Verified 2026-07-29 against docs.x.ai. OpenAI-compatible at
            // https://api.x.ai/v1/chat/completions — Lucy reuses the OpenAI
            // request/parse path unchanged (see ai.rs openai_compatible_endpoint).
            //
            // Prices are TIERED BY PROMPT LENGTH: the lower rate applies under
            // 200k tokens, roughly double at or above it. model-pricing.ts quotes
            // the lower tier, which is where Lucy's compaction keeps every turn.
            //   • grok-4.5  500k ctx  $2.00 / $6.00 per 1M  (under 200k prompt)
            //   • grok-4.3    1M ctx  $1.25 / $2.50 per 1M
            { id: "grok-4.5", icon: "◆", nameEn: "Grok 4.5 — Flagship reasoning",        nameEs: "Grok 4.5 — Razonamiento insignia" },
            { id: "grok-4.3", icon: "◇", nameEn: "Grok 4.3 — 1M context, lower cost",    nameEs: "Grok 4.3 — 1M de contexto, menor costo" },
        ]
    },
    {
        label: "── DeepSeek ──",
        provider: "deepseek",
        credential_key: "deepseek_api_key",
        options: [
            // Verified 2026-07-29 against api-docs.deepseek.com. OpenAI-compatible
            // at https://api.deepseek.com/chat/completions.
            //
            // By far the cheapest cloud tier Lucy offers — v4-flash is ~$0.14/$0.28
            // per 1M against Gemini Flash-Lite's $0.25/$1.50, with a 1M context and
            // 384K max output. Worth considering for long agent loops where token
            // volume, not per-token quality, is what costs money.
            //   • deepseek-v4-flash  1M ctx  $0.14  / $0.28 per 1M
            //   • deepseek-v4-pro    1M ctx  $0.435 / $0.87 per 1M
            { id: "deepseek-v4-flash", icon: "◇", nameEn: "DeepSeek V4 Flash — Cheapest cloud tier", nameEs: "DeepSeek V4 Flash — La nube más barata" },
            { id: "deepseek-v4-pro",   icon: "◆", nameEn: "DeepSeek V4 Pro — Stronger reasoning",    nameEs: "DeepSeek V4 Pro — Razonamiento más fuerte" },
        ]
    },
    {
        label: "── NVIDIA NIM (build.nvidia.com) ──",
        provider: "nvidia",
        credential_key: "nvidia_api_key",
        options: [
            // ── Lucy's geometric icon system for NIM-hosted models ──
            //   ◆ = flagship / largest model from a family
            //   ◇ = mid-tier balanced
            //   ◯ = small / fast
            //   ⌬ = code-specialized
            //   ✎ = custom model (user types their own owner/model id)
            { id: "meta/llama-3.1-70b-instruct",        icon: "◇", nameEn: "Llama 3.1 70B — Balanced Power",           nameEs: "Llama 3.1 70B — Potencia Equilibrada" },
            { id: "meta/llama-3.3-70b-instruct",        icon: "◇", nameEn: "Llama 3.3 70B — Latest Llama",            nameEs: "Llama 3.3 70B — Llama más Reciente" },
            { id: "meta/llama-3.1-405b-instruct",       icon: "◆", nameEn: "Llama 3.1 405B — Max Intelligence",       nameEs: "Llama 3.1 405B — Máxima Inteligencia" },
            { id: "nvidia/nemotron-3-super-120b-a12b",  icon: "◆", nameEn: "Nemotron 3 Super 120B — NVIDIA Flagship", nameEs: "Nemotron 3 Super 120B — NVIDIA Flagship" },
            { id: "nvidia/nemotron-4-340b-instruct",    icon: "◆", nameEn: "Nemotron 4 340B — NVIDIA Max",            nameEs: "Nemotron 4 340B — NVIDIA Máximo" },
            { id: "mistralai/mistral-large-2-instruct", icon: "◇", nameEn: "Mistral Large 2 — Code & Reasoning",     nameEs: "Mistral Large 2 — Código y Razonamiento" },
            { id: "mistralai/mistral-7b-instruct-v0.3", icon: "◯", nameEn: "Mistral 7B — Fast & Lightweight",        nameEs: "Mistral 7B — Rápido y Ligero" },
            { id: "google/gemma-4-31b-it",              icon: "◇", nameEn: "Gemma 4 31B (NIM) — Google via NVIDIA",  nameEs: "Gemma 4 31B (NIM) — Google vía NVIDIA" },
            { id: "microsoft/phi-3.5-mini-instruct",    icon: "◯", nameEn: "Phi-3.5 Mini — Fast & Efficient",        nameEs: "Phi-3.5 Mini — Rápido y Eficiente" },
            { id: "nvidia-custom",                      icon: "✎", nameEn: "Custom NVIDIA Model — type owner/model", nameEs: "Modelo NVIDIA Personalizado — escribe owner/model" },
        ]
    },
    {
        label: "── Local Ollama (Self-Hosted) ──",
        provider: "ollama",
        credential_key: "ollama_endpoint",
        endpoint_default: "http://localhost:11434",
        options: [
            // ⌂ = local/self-hosted ("house" — matches the side-bar "Local DNS" + "Lock Equipment" icons aesthetic)
            { id: "local-custom", icon: "⌂", nameEn: "Custom Local Model — ollama pull <model>", nameEs: "Modelo Local Personalizado — ollama pull <model>" }
        ]
    }
];

export function getModelDescription(id, isEN) {
    for (const group of LLM_GROUPS) {
        const opt = group.options.find(o => o.id === id);
        if (opt) return isEN ? opt.nameEn : opt.nameEs;
    }
    return id;
}

export function getModelIcon(id) {
    for (const group of LLM_GROUPS) {
        const opt = group.options.find(o => o.id === id);
        if (opt) return opt.icon;
    }
    return "◉";  // Fallback to neutral circle symbol
}
