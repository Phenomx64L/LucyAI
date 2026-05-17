// ── intent-classifier — pre-flight prompt classification
//
// Classifies a user prompt into one of four buckets BEFORE the main agent
// loop fires. The motivation: Lucy's first turn does too much work for
// trivial questions ("what's PowerShell?") because the full system prompt
// + agent loop overhead pays the same cost regardless of complexity.
//
// Buckets:
//   trivial      — pure conversation, no tools, no execution
//                  → bypass agent loop, single short response
//   tool_use     — needs file reads, searches, info gathering
//                  → standard agent loop
//   planning     — multi-step work, ≥3 steps with dependencies
//                  → force <PLAN> emission, gate execution behind user click
//   destructive  — mutates system state (Stop/Restart/Remove/Set/etc.)
//                  → require <PLAN> + verify + rollback (existing flow)
//
// Two backends:
//   • LOCAL  — pure regex/keyword heuristics. Free, sub-millisecond,
//              ~75% accuracy. Always runs first.
//   • LLM    — optional cheap-model second opinion (Gemini Flash). Used
//              ONLY when local confidence < 0.7 to avoid round-tripping
//              for the obvious cases. Caller decides if to invoke.
//
// The module is pure logic — no fetch, no Tauri. The caller wires the
// LLM call when (and only when) it's worth it.

export type IntentKind = 'trivial' | 'tool_use' | 'planning' | 'destructive';

export interface IntentClassification {
    kind:        IntentKind;
    /** 0..1 — heuristic-derived. >0.8 = act on it; <0.7 = consider LLM second-opinion. */
    confidence:  number;
    /** Short human-readable explanation of why this bucket fired. */
    reasoning:   string;
    /** Specific signals that contributed (for debugging). */
    signals:     string[];
}

// ── Lexical features ────────────────────────────────────────────────────

// Words that strongly suggest the user wants Lucy to MUTATE the system.
// Curated from Windows + Linux + container ops vocabulary.
const DESTRUCTIVE_PATTERNS: { re: RegExp; weight: number; reason: string }[] = [
    { re: /\b(restart|reinici|reboot|reinicia)\w*\b/i,            weight: 0.30, reason: 'restart-verb' },
    { re: /\b(stop|detener|detén|kill|matar|terminate)\w*\b/i,    weight: 0.30, reason: 'stop-verb' },
    { re: /\b(remove|delete|borra|elimina|destruye|drop)\w*\b/i,  weight: 0.30, reason: 'delete-verb' },
    { re: /\b(format|formatear|wipe|reset)\w*\b/i,                weight: 0.40, reason: 'format-verb' },
    { re: /\b(disable|desactiv|deshabilita|uninstall|desinstal)\w*\b/i, weight: 0.30, reason: 'disable-verb' },
    { re: /\b(set-itemproperty|set-service|invoke-wmimethod|set-acl)\b/i, weight: 0.40, reason: 'powershell-mutator' },
    { re: /\b(rm\s+-rf|dd\s+if=|mkfs|fdisk\s+\/d)/i,              weight: 0.50, reason: 'shell-destructive' },
    { re: /\b(shutdown|apaga|halt|poweroff)\b/i,                  weight: 0.40, reason: 'shutdown-verb' },
    { re: /\b(production|prod|live)\b/i,                          weight: 0.10, reason: 'prod-context' },
];

// Words that suggest Lucy needs to USE TOOLS to gather information.
const TOOL_USE_PATTERNS: { re: RegExp; weight: number; reason: string }[] = [
    { re: /\b(check|verify|verifica|revisa|inspect|inspecciona)\w*\b/i, weight: 0.20, reason: 'inspect-verb' },
    { re: /\b(read|lee|show|muéstr|display|abre|open)\w*\b/i,           weight: 0.20, reason: 'read-verb' },
    { re: /\b(search|busca|find|encuentra|locate|locali|grep)\w*\b/i,   weight: 0.25, reason: 'search-verb' },
    { re: /\b(list|lista|enumera|cat|ls|dir|tree)\b/i,                  weight: 0.20, reason: 'list-verb' },
    { re: /\b(get-|select-|measure-|where-|test-)\w+/i,                 weight: 0.30, reason: 'powershell-getter' },
    { re: /\b(file|archivo|directory|directorio|folder|carpeta|log)\b/i, weight: 0.10, reason: 'fs-noun' },
    { re: /\b(host|server|servidor|machine|máquina|node|cluster)\b/i,   weight: 0.15, reason: 'host-noun' },
    { re: /\b(error|warning|status|estado|health|salud)\b/i,            weight: 0.15, reason: 'diagnostics-noun' },
    { re: /\.(ps1|sh|md|json|yaml|yml|toml|svelte|ts|rs|py)\b/i,        weight: 0.30, reason: 'file-extension' },
    { re: /[\\/](?:home|users|etc|var|tmp|opt|root)[\\/]/i,             weight: 0.30, reason: 'unix-path' },
    { re: /[A-Z]:\\/,                                                    weight: 0.30, reason: 'windows-path' },
];

// Words that suggest a multi-step plan is needed.
const PLANNING_PATTERNS: { re: RegExp; weight: number; reason: string }[] = [
    { re: /\b(then|luego|después|after|tras|next|seguido)\b/i,    weight: 0.20, reason: 'sequence-marker' },
    { re: /\bfirst\b.*\bthen\b/i,                                  weight: 0.30, reason: 'first-then' },
    { re: /\b(primero|al final|finally|por último)\b/i,           weight: 0.20, reason: 'order-marker' },
    { re: /\b(migrate|migración|deploy|deployment|rollout|release)\b/i, weight: 0.25, reason: 'deployment-noun' },
    { re: /\b(setup|configura todo|build pipeline|orchestr)\w*\b/i, weight: 0.25, reason: 'setup-verb' },
    { re: /\b(varios|multiple|several|many|todos los)\b.*\b(servidor|host|paso|step)/i, weight: 0.30, reason: 'multi-target' },
    { re: /\b(plan|planea|planifica|esquema)\b/i,                 weight: 0.30, reason: 'plan-explicit' },
    { re: /\?.*\?/,                                                weight: 0.10, reason: 'compound-question' },   // asks 2+ things
];

// Words that suggest TRIVIAL conversational intent (no system effect).
const TRIVIAL_PATTERNS: { re: RegExp; weight: number; reason: string }[] = [
    { re: /\b(qu[eé] es|what is|define|explica|explain|how does)\b/i, weight: 0.30, reason: 'definition-question' },
    { re: /\b(hola|hello|hi|buenos|buenas|good morning|qué tal)\b/i,   weight: 0.40, reason: 'greeting' },
    { re: /\b(gracias|thanks|thank you|ok|listo|perfecto)\b/i,         weight: 0.40, reason: 'acknowledgement' },
    { re: /^[A-Za-zñÑáéíóúÁÉÍÓÚ\s,.?!¿¡]{1,40}$/,                      weight: 0.10, reason: 'short-prose-only' },
    { re: /\b(joke|chiste|why is|por qué)\b/i,                          weight: 0.20, reason: 'curiosity-question' },
    { re: /\b(your|tu) (name|nombre|version|capabilities|capacidades)\b/i, weight: 0.30, reason: 'self-question' },
];

// ── Public API ─────────────────────────────────────────────────────────

/**
 * Pure-local classifier. Walks each pattern set, accumulates weighted
 * scores, picks the bucket with the highest score, normalizes the gap
 * between top-1 and top-2 into a confidence value.
 *
 * Caller is expected to invoke a cheap LLM second opinion only when
 * confidence < 0.7. For trivia the local pass is plenty.
 */
export function classifyIntent(prompt: string): IntentClassification {
    const text = (prompt || '').trim();
    if (!text || text.length < 2) {
        return {
            kind: 'trivial',
            confidence: 0.99,
            reasoning: 'empty or near-empty prompt',
            signals: ['empty-input'],
        };
    }

    const scores: Record<IntentKind, number> = {
        trivial: 0, tool_use: 0, planning: 0, destructive: 0,
    };
    const signals: string[] = [];

    const score = (kind: IntentKind, list: typeof DESTRUCTIVE_PATTERNS) => {
        for (const p of list) {
            if (p.re.test(text)) {
                scores[kind] += p.weight;
                signals.push(`${kind}:${p.reason}`);
            }
        }
    };
    score('destructive', DESTRUCTIVE_PATTERNS);
    score('tool_use',    TOOL_USE_PATTERNS);
    score('planning',    PLANNING_PATTERNS);
    score('trivial',     TRIVIAL_PATTERNS);

    // Length heuristic: very long prompts are rarely trivial.
    if (text.length > 200) {
        scores.trivial *= 0.5;
        scores.tool_use += 0.05;
    }
    // Question-mark heuristic: a single ? leans informational.
    const qMarks = (text.match(/\?/g) || []).length;
    if (qMarks === 1) scores.tool_use += 0.05;
    if (qMarks >= 2)  scores.planning += 0.10;

    // Destructive trumps planning (a destructive plan is still destructive).
    if (scores.destructive > 0.20 && scores.planning > 0.20) {
        scores.destructive += 0.10;
    }

    // Pick the winner.
    const ranked = (Object.entries(scores) as [IntentKind, number][])
        .sort((a, b) => b[1] - a[1]);
    const [topKind, topScore] = ranked[0];
    const [, second] = ranked[1];

    // Confidence = normalized gap between top-1 and top-2, capped to 0..1.
    // If the top score itself is very low (<0.20) the prompt didn't trigger
    // any of our heuristics — fall back to tool_use with low confidence
    // because the standard agent loop is the safe default.
    let kind:       IntentKind = topKind;
    let confidence: number;
    let reasoning:  string;

    if (topScore < 0.15) {
        kind = 'tool_use';
        confidence = 0.40;
        reasoning = 'no strong signals; defaulting to tool_use (safe default)';
    } else {
        const gap = topScore - second;
        confidence = Math.min(1, 0.50 + gap * 1.4);
        reasoning  = `top score ${topScore.toFixed(2)} vs ${second.toFixed(2)} for "${kind}"`;
    }

    return { kind, confidence, reasoning, signals };
}

/**
 * Build the prompt for the optional LLM second opinion. Caller hands this
 * to a cheap model (e.g. Gemini Flash) when the local classifier returns
 * confidence < 0.7. The LLM should reply ONLY with one of the four bucket
 * names — anything else falls back to the local guess.
 */
export function buildLlmClassifierPrompt(userText: string): string {
    return [
        'Classify the SysAdmin prompt below into ONE bucket. Reply with JUST the bucket name, no prose.',
        '',
        'Buckets:',
        '  trivial     — small talk, definitions, what-is questions, greetings, "show me your version"',
        '  tool_use    — needs to read files, search, list, check status, gather info (no mutation)',
        '  planning    — multi-step work with sequencing or 3+ dependencies',
        '  destructive — mutates system state (restart/delete/disable/format/install/uninstall)',
        '',
        '=== PROMPT ===',
        userText.slice(0, 1200),
        '=== END ===',
        '',
        'Reply with ONE WORD (trivial / tool_use / planning / destructive):',
    ].join('\n');
}

/** Validate + normalize the LLM's reply to a bucket. Defensive — the model
 *  occasionally returns "tool use" or "the answer is tool_use". */
export function parseLlmIntentReply(raw: string): IntentKind | null {
    const s = String(raw || '').toLowerCase();
    if (/destructive/.test(s)) return 'destructive';
    if (/planning|multi[ -]?step/.test(s)) return 'planning';
    if (/trivial|conversation|greeting/.test(s)) return 'trivial';
    if (/tool[ _-]?use|tool|use[ _-]?tool|gather|inspect/.test(s)) return 'tool_use';
    return null;
}

/**
 * Combine local + LLM signals into a final classification. If both agree,
 * confidence climbs; if they disagree, take the LLM's bucket but keep
 * confidence modest so the caller treats it as a soft hint.
 */
export function reconcileIntent(local: IntentClassification, llm: IntentKind | null): IntentClassification {
    if (!llm || llm === local.kind) {
        return llm
            ? { ...local, confidence: Math.min(1, local.confidence + 0.20), reasoning: local.reasoning + ' (+ LLM agrees)' }
            : local;
    }
    return {
        kind: llm,
        confidence: Math.max(local.confidence, 0.65),
        reasoning: `local guessed "${local.kind}", LLM overrode to "${llm}"`,
        signals: [...local.signals, `llm-override:${llm}`],
    };
}
