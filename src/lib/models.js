export const LLM_GROUPS = [
    {
        label: "── Anthropic Claude ──",
        options: [
            { id: "claude-sonnet-4-6", icon: "🧠", nameEn: "Claude 4.6 Sonnet — Advanced Coding & Analysis", nameEs: "Claude 4.6 Sonnet — Análisis y Programación Avanzada" },
            { id: "claude-sonnet-4-5", icon: "✨", nameEn: "Claude 4.5 Sonnet — Fast & Efficient", nameEs: "Claude 4.5 Sonnet — Rápido y Eficiente" }
        ]
    },
    {
        label: "── OpenAI GPT ──",
        options: [
            { id: "gpt-5.4", icon: "✨", nameEn: "GPT-5.4 — Scale Intelligence", nameEs: "GPT-5.4 — Inteligencia a Escala" },
            { id: "gpt-5.4-mini", icon: "⚡", nameEn: "GPT-5.4 Mini — Fast Code & Agents", nameEs: "GPT-5.4 Mini — Rápido para Código y Agentes" },
            { id: "gpt-5.4-nano", icon: "💨", nameEn: "GPT-5.4 Nano — High-Volume Tasks", nameEs: "GPT-5.4 Nano — Tareas de Alto Volumen" }
        ]
    },
    {
        label: "── Gemini 3 (Next-Gen Preview) ──",
        options: [
            { id: "gemini-3.1-pro-preview", icon: "💎", nameEn: "Pro 3.1 — Complex logic & deep thinking", nameEs: "Pro 3.1 — Análisis Arquitectónico Profundo" },
            { id: "gemini-3-flash-preview", icon: "🦅", nameEn: "Flash 3 — Expansive context, rapid tasks", nameEs: "Flash 3 — Contexto Masivo & Refactorización ágil" },
            { id: "gemini-3.1-flash-lite-preview", icon: "⚡", nameEn: "Lite 3.1 — Max efficiency for simple ops", nameEs: "Lite 3.1 — Scripts Rápidos" }
        ]
    },
    {
        label: "── Gemini 2.5 (Estable) ──",
        options: [
            { id: "gemini-2.5-flash", icon: "⚡", nameEn: "Flash 2.5 — Balanced SysAdmin (Recommended)", nameEs: "Flash 2.5 — SysAdmin Equilibrado" },
            { id: "gemini-2.5-pro", icon: "🧠", nameEn: "Pro 2.5 — Critical code & debugging", nameEs: "Pro 2.5 — Depuración Crítica" }
        ]
    },
    {
        label: "── Modelos Locales ──",
        options: [
            { id: "local-custom", icon: "🖥️", nameEn: "Local LLM — Custom Endpoint (Ollama/API)", nameEs: "Local LLM — Endpoint Personalizado (Ollama/API)" }
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
    return "🤖";
}
