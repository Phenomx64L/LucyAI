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
            const grp = LLM_GROUPS.find(g => g.label.includes('Locales'));
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

export const LLM_GROUPS = [
    {
        label: "── Anthropic Claude (Native Computer Use) ──",
        provider: "anthropic",
        credential_key: "anthropic_api_key",
        options: [
            { id: "claude-opus-4-5", icon: "◉", nameEn: "Claude Opus 4.5 — Best Intelligence", nameEs: "Claude Opus 4.5 — Máxima Inteligencia" },
            { id: "claude-sonnet-4-6", icon: "◉", nameEn: "Claude Sonnet 4.6 — Advanced Analysis & Code", nameEs: "Claude Sonnet 4.6 — Análisis Avanzado y Código" },
            { id: "claude-sonnet-4-5", icon: "✦", nameEn: "Claude Sonnet 4.5 — Fast & Efficient", nameEs: "Claude Sonnet 4.5 — Rápido y Eficiente" },
            { id: "claude-3-5-sonnet-latest", icon: "▸", nameEn: "Claude 3.5 Sonnet — Balanced Performance", nameEs: "Claude 3.5 Sonnet — Rendimiento Equilibrado" },
        ]
    },
    {
        label: "── Google Gemini Vision ──",
        provider: "gemini",
        credential_key: "gemini_api_key",
        options: [
            { id: "gemini-3.1-pro-preview", icon: "◆", nameEn: "Gemini 3.1 Pro — Ultimate Analysis", nameEs: "Gemini 3.1 Pro — Máxima Inteligencia" },
            { id: "gemini-3-flash-preview", icon: "▸", nameEn: "Gemini 3 Flash — Fast & Balanced", nameEs: "Gemini 3 Flash — Rápido y Equilibrado" },
            { id: "gemini-2.5-pro", icon: "◆", nameEn: "Gemini 2.5 Pro — Deep Analysis", nameEs: "Gemini 2.5 Pro — Análisis Profundo" },
            { id: "gemini-2.5-flash", icon: "▸", nameEn: "Gemini 2.5 Flash — SysAdmin Balanced", nameEs: "Gemini 2.5 Flash — SysAdmin Equilibrado" },
        ]
    },
    {
        label: "── OpenAI GPT-4 Vision ──",
        provider: "openai",
        credential_key: "openai_api_key",
        options: [
            { id: "gpt-4o", icon: "✦", nameEn: "GPT-4o — Multimodal Intelligence", nameEs: "GPT-4o — Inteligencia Multimodal" },
            { id: "gpt-4-turbo", icon: "▸", nameEn: "GPT-4 Turbo — Fast & Capable", nameEs: "GPT-4 Turbo — Rápido y Capaz" },
            { id: "gpt-4o-mini", icon: "▸", nameEn: "GPT-4o Mini — Fast & Cost Effective", nameEs: "GPT-4o Mini — Rápido y Económico" },
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
