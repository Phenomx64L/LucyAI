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
function pickIcon(name) {
    const n = name.toLowerCase();
    if (n.includes('nemotron')) return '⚡';
    if (n.includes('qwen')) return '🐉';
    if (n.includes('llama')) return '🦙';
    if (n.includes('deepseek')) return '🐋';
    if (n.includes('mistral') || n.includes('mixtral')) return '🌬️';
    if (n.includes('phi')) return '🔷';
    if (n.includes('gemma')) return '💎';
    if (n.includes('codellama') || n.includes('coder')) return '👨‍💻';
    return '🖥️';
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
            opts.push({ id: "local-custom", icon: "🖥️", nameEn: "Custom Endpoint", nameEs: "Endpoint Personalizado" });
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
            // Iconography:
            //   🧠 reasoning tier   ✨ balanced tier   ⚡ fast tier   🕰 legacy
            { id: "claude-opus-4-7",   icon: "🧠", nameEn: "Claude Opus 4.7 — Best Intelligence",         nameEs: "Claude Opus 4.7 — Máxima Inteligencia" },
            { id: "claude-sonnet-4-6", icon: "✨", nameEn: "Claude Sonnet 4.6 — Advanced Analysis & Code", nameEs: "Claude Sonnet 4.6 — Análisis Avanzado y Código" },
            { id: "claude-haiku-4-5",  icon: "⚡", nameEn: "Claude Haiku 4.5 — Fast & Efficient",         nameEs: "Claude Haiku 4.5 — Rápido y Eficiente" },
            // Legacy — kept for backward compat with existing chats / runbooks
            { id: "claude-sonnet-4-5", icon: "🕰", nameEn: "Claude Sonnet 4.5 — Legacy",                  nameEs: "Claude Sonnet 4.5 — Legado" },
        ]
    },
    {
        label: "── Google Gemini Vision ──",
        provider: "gemini",
        credential_key: "gemini_api_key",
        options: [
            // Gemini 3.1 family (May 2026). 2.5 entries removed from the dropdown
            // (still in ALLOWED_MODELS for backward compat with old saved chats).
            //   💎 frontier   🌟 balanced   🪶 lite/cheapest   🧪 preview
            { id: "gemini-3.1-pro-preview",        icon: "💎", nameEn: "Gemini 3.1 Pro — Ultimate Analysis",            nameEs: "Gemini 3.1 Pro — Máxima Inteligencia" },
            { id: "gemini-3-flash-preview",        icon: "🌟", nameEn: "Gemini 3 Flash — Fast & Balanced",              nameEs: "Gemini 3 Flash — Rápido y Equilibrado" },
            { id: "gemini-3.1-flash-lite",         icon: "🪶", nameEn: "Gemini 3.1 Flash-Lite — Frontier at Low Cost", nameEs: "Gemini 3.1 Flash-Lite — Frontier a Bajo Costo" },
            { id: "gemini-3.1-flash-lite-preview", icon: "🧪", nameEn: "Gemini 3.1 Flash-Lite Preview",                 nameEs: "Gemini 3.1 Flash-Lite Vista Previa" },
        ]
    },
    {
        label: "── OpenAI GPT-5 Vision ──",
        provider: "openai",
        credential_key: "openai_api_key",
        options: [
            // GPT-5.5 family (April-May 2026).
            //   🚀 frontier   ⚡ instant/fast   🌱 mini   🍃 nano   👨‍💻 codex   🕰 legacy
            { id: "gpt-5.5",         icon: "🚀",     nameEn: "GPT-5.5 — Frontier Reasoning & Coding", nameEs: "GPT-5.5 — Razonamiento y Código de Frontera" },
            { id: "gpt-5.5-instant", icon: "⚡",     nameEn: "GPT-5.5 Instant — Low-Latency Default", nameEs: "GPT-5.5 Instant — Baja Latencia por Defecto" },
            { id: "gpt-5.4-mini",    icon: "🌱",     nameEn: "GPT-5.4 Mini — Fast & Cost Effective",  nameEs: "GPT-5.4 Mini — Rápido y Económico" },
            { id: "gpt-5.4-nano",    icon: "🍃",     nameEn: "GPT-5.4 Nano — Cheapest Reasoning",     nameEs: "GPT-5.4 Nano — Razonamiento Más Barato" },
            { id: "gpt-5.3-codex",   icon: "👨‍💻", nameEn: "GPT-5.3 Codex — Agentic Coding",        nameEs: "GPT-5.3 Codex — Codificación Agéntica" },
            // Legacy
            { id: "gpt-4o",      icon: "🕰", nameEn: "GPT-4o — Legacy Multimodal",    nameEs: "GPT-4o — Legado Multimodal" },
            { id: "gpt-4o-mini", icon: "🕰", nameEn: "GPT-4o Mini — Legacy Fast",     nameEs: "GPT-4o Mini — Legado Rápido" },
        ]
    },
    {
        label: "── NVIDIA NIM (build.nvidia.com) ──",
        provider: "nvidia",
        credential_key: "nvidia_api_key",
        options: [
            { id: "meta/llama-3.1-70b-instruct",        icon: "🦙", nameEn: "Llama 3.1 70B — Balanced Power",           nameEs: "Llama 3.1 70B — Potencia Equilibrada" },
            { id: "meta/llama-3.3-70b-instruct",        icon: "🦙", nameEn: "Llama 3.3 70B — Latest Llama",            nameEs: "Llama 3.3 70B — Llama más Reciente" },
            { id: "meta/llama-3.1-405b-instruct",       icon: "🦙", nameEn: "Llama 3.1 405B — Max Intelligence",       nameEs: "Llama 3.1 405B — Máxima Inteligencia" },
            { id: "nvidia/nemotron-3-super-120b-a12b",  icon: "⚡", nameEn: "Nemotron 3 Super 120B — NVIDIA Flagship", nameEs: "Nemotron 3 Super 120B — NVIDIA Flagship" },
            { id: "nvidia/nemotron-4-340b-instruct",    icon: "⚡", nameEn: "Nemotron 4 340B — NVIDIA Max",            nameEs: "Nemotron 4 340B — NVIDIA Máximo" },
            { id: "mistralai/mistral-large-2-instruct", icon: "🌬️", nameEn: "Mistral Large 2 — Code & Reasoning",     nameEs: "Mistral Large 2 — Código y Razonamiento" },
            { id: "mistralai/mistral-7b-instruct-v0.3", icon: "🌬️", nameEn: "Mistral 7B — Fast & Lightweight",        nameEs: "Mistral 7B — Rápido y Ligero" },
            { id: "google/gemma-4-31b-it",              icon: "💎", nameEn: "Gemma 4 31B (NIM) — Google via NVIDIA",  nameEs: "Gemma 4 31B (NIM) — Google vía NVIDIA" },
            { id: "microsoft/phi-3.5-mini-instruct",    icon: "🔷", nameEn: "Phi-3.5 Mini — Fast & Efficient",        nameEs: "Phi-3.5 Mini — Rápido y Eficiente" },
            { id: "nvidia-custom",                      icon: "✏️", nameEn: "Custom NVIDIA Model — type owner/model", nameEs: "Modelo NVIDIA Personalizado — escribe owner/model" },
        ]
    },
    {
        label: "── Local Ollama (Self-Hosted) ──",
        provider: "ollama",
        credential_key: "ollama_endpoint",
        endpoint_default: "http://localhost:11434",
        options: [
            { id: "local-custom", icon: "🖥️", nameEn: "Custom Local Model — ollama pull <model>", nameEs: "Modelo Local Personalizado — ollama pull <model>" }
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
