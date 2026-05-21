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
            // May 2026 lineup — Opus 4.7 / Sonnet 4.6 / Haiku 4.5 (1M ctx on Opus & Sonnet)
            //
            // Effort levels (from platform.claude.com/docs/en/build-with-claude/effort):
            //   • Opus 4.7    supports low/medium/high/xhigh/max  (xhigh is exclusive)
            //   • Sonnet 4.6  supports low/medium/high/max        (docs recommend medium default)
            //   • Haiku 4.5   does NOT support effort (lightweight tier)
            //   • Sonnet 4.5  legacy — uses manual thinking, no effort param
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
            // ── Opus 4.7 — start with xhigh for coding/agentic (per docs) ──
            { id: "claude-opus-4-7::xhigh",  icon: "◆", nameEn: "Claude Opus 4.7 — Extra High (coding/agentic)", nameEs: "Claude Opus 4.7 — Extra Alto (coding/agéntico)" },
            { id: "claude-opus-4-7::high",   icon: "◆", nameEn: "Claude Opus 4.7 — High (default)",              nameEs: "Claude Opus 4.7 — Alto (predeterminado)" },
            { id: "claude-opus-4-7::medium", icon: "◆", nameEn: "Claude Opus 4.7 — Medium (cost-sensitive)",     nameEs: "Claude Opus 4.7 — Medio (sensible al costo)" },
            { id: "claude-opus-4-7::max",    icon: "◆", nameEn: "Claude Opus 4.7 — Max (frontier problems)",     nameEs: "Claude Opus 4.7 — Max (problemas frontera)" },
            // ── Sonnet 4.6 — docs recommend medium as default ──
            { id: "claude-sonnet-4-6::medium", icon: "◇", nameEn: "Claude Sonnet 4.6 — Medium (recommended default)", nameEs: "Claude Sonnet 4.6 — Medio (predeterminado recomendado)" },
            { id: "claude-sonnet-4-6::low",    icon: "◇", nameEn: "Claude Sonnet 4.6 — Low (speed-sensitive)",        nameEs: "Claude Sonnet 4.6 — Bajo (sensible a latencia)" },
            { id: "claude-sonnet-4-6::high",   icon: "◇", nameEn: "Claude Sonnet 4.6 — High (quality first)",         nameEs: "Claude Sonnet 4.6 — Alto (calidad primero)" },
            { id: "claude-sonnet-4-6::max",    icon: "◇", nameEn: "Claude Sonnet 4.6 — Max (deepest analysis)",       nameEs: "Claude Sonnet 4.6 — Max (análisis más profundo)" },
            // ── Haiku 4.5 — no effort param available ──
            { id: "claude-haiku-4-5",  icon: "▸", nameEn: "Claude Haiku 4.5 — Fast & Efficient",         nameEs: "Claude Haiku 4.5 — Rápido y Eficiente" },
            // Legacy — kept for backward compat with existing chats / runbooks
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
            // Notes:
            //   • gemini-3.1-pro-preview is still in PREVIEW (1M ctx · 65K out · cutoff Jan 2025).
            //   • gemini-3.5-flash is GENERALLY AVAILABLE (1M ctx · 65K out · cutoff Jan 2025).
            //   • gemini-3.1-flash-lite is GA (1M ctx · 65K out · cutoff Jan 2025).
            { id: "gemini-3.1-pro-preview::high",   icon: "◆", nameEn: "Gemini 3.1 Pro — High Effort (deep reasoning)", nameEs: "Gemini 3.1 Pro — Esfuerzo Alto (razonamiento profundo)" },
            { id: "gemini-3.1-pro-preview::medium", icon: "◆", nameEn: "Gemini 3.1 Pro — Medium Effort (balanced)",     nameEs: "Gemini 3.1 Pro — Esfuerzo Medio (balanceado)" },
            { id: "gemini-3.5-flash",               icon: "◐", nameEn: "Gemini 3.5 Flash — Frontier-class at lower cost", nameEs: "Gemini 3.5 Flash — Frontera a menor costo" },
            { id: "gemini-3.1-flash-lite",          icon: "◯", nameEn: "Gemini 3.1 Flash-Lite — High-volume workhorse",   nameEs: "Gemini 3.1 Flash-Lite — Caballo de batalla de alto volumen" },
            { id: "gemini-3.1-flash-lite-preview",  icon: "◎", nameEn: "Gemini 3.1 Flash-Lite Preview",                   nameEs: "Gemini 3.1 Flash-Lite Vista Previa" },
        ]
    },
    {
        label: "── OpenAI GPT-5 Vision ──",
        provider: "openai",
        credential_key: "openai_api_key",
        options: [
            // ── Lucy's geometric icon system ──
            //   ◆ = frontier (top-tier reasoning)
            //   ▸ = instant / fast tier
            //   ◯ = mini (small but capable)
            //   ◌ = nano (smallest)
            //   ⌬ = codex (hex = code-specialized)
            //   ▫ = legacy (kept for backward compat)
            { id: "gpt-5.5",         icon: "◆",  nameEn: "GPT-5.5 — Frontier Reasoning & Coding", nameEs: "GPT-5.5 — Razonamiento y Código de Frontera" },
            { id: "gpt-5.5-instant", icon: "▸",  nameEn: "GPT-5.5 Instant — Low-Latency Default", nameEs: "GPT-5.5 Instant — Baja Latencia por Defecto" },
            { id: "gpt-5.4-mini",    icon: "◯",  nameEn: "GPT-5.4 Mini — Fast & Cost Effective",  nameEs: "GPT-5.4 Mini — Rápido y Económico" },
            { id: "gpt-5.4-nano",    icon: "◌",  nameEn: "GPT-5.4 Nano — Cheapest Reasoning",     nameEs: "GPT-5.4 Nano — Razonamiento Más Barato" },
            { id: "gpt-5.3-codex",   icon: "⌬",  nameEn: "GPT-5.3 Codex — Agentic Coding",        nameEs: "GPT-5.3 Codex — Codificación Agéntica" },
            // Legacy
            { id: "gpt-4o",      icon: "▫", nameEn: "GPT-4o — Legacy Multimodal",    nameEs: "GPT-4o — Legado Multimodal" },
            { id: "gpt-4o-mini", icon: "▫", nameEn: "GPT-4o Mini — Legacy Fast",     nameEs: "GPT-4o Mini — Legado Rápido" },
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
