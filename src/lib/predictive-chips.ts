// ── predictive-chips.ts — U5 contextual next-action suggestions ──────────
//
// Looks at the LAST Lucy turn (message + tool cards + errors) and proposes
// 1-3 short chips representing likely next actions. Sits between the
// streaming layer and the input bar.
//
// Design:
//   • Pure heuristic — no LLM call. Fast (< 1ms), predictable, debuggable.
//   • Pattern library: each pattern is a `ChipRule` with a matcher fn
//     and a chip-generator fn. New patterns can be added without touching
//     core logic.
//   • Output is plain data — UI renders it.
//   • Dismissed chips don't reappear for the same turn (sticky dismiss).
//
// Activation:
//   • Called on each new Lucy message.
//   • Returns [] if no patterns match (input strip hides).
//   • Chips fade in with stagger animation; click executes or fills input.

export interface PredictiveChip {
    /** Stable id (used for dismiss-sticky and key in #each). */
    id: string;
    /** Short label shown on the chip. */
    label: string;
    /** Longer tooltip explaining what will happen. */
    tooltip: string;
    /** Icon glyph (single char or emoji). */
    icon: string;
    /** What to do on click. */
    action:
        | { kind: 'fill_input'; text: string }
        | { kind: 'run_command'; cmd: string }
        | { kind: 'slash'; command: string };
    /** Severity hint for styling: info / suggest / caution. */
    severity?: 'info' | 'suggest' | 'caution';
}

/** Inputs available to chip rules. */
export interface ChipContext {
    /** Last Lucy reply, raw text (the rawContent of the message). */
    lastLucyText: string;
    /** Last user prompt (rawContent of the previous user message). */
    lastUserText: string;
    /** True if the last response touched tools (executed). */
    hadTools: boolean;
    /** Last tool-card labels, lowercased. */
    toolLabels: string[];
    /** True if any tool errored. */
    hadError: boolean;
    /** True if Lucy mentioned an open question / unresolved status. */
    hasOpenQuestion: boolean;
    /** Current working directory if known. */
    cwd?: string;
}

type ChipRule = {
    name: string;
    matches: (ctx: ChipContext) => boolean;
    build:  (ctx: ChipContext) => PredictiveChip | null;
};

// ── Pattern library ─────────────────────────────────────────────────────

const RULES: ChipRule[] = [
    // After a successful read of code → suggest analyze
    {
        name: 'analyze-after-read',
        matches: (c) => c.hadTools && c.toolLabels.some(l => /readfile|read file|abrir|open/.test(l)),
        build: (_c) => ({
            id: 'chip-analyze',
            label: 'Analiza este código',
            tooltip: 'Pide a Lucy un resumen técnico + posibles bugs del último archivo abierto',
            icon: '⌬',
            action: { kind: 'fill_input', text: 'Analiza este código que acabamos de leer: ¿qué hace, qué riesgos tiene, qué optimizarías?' },
            severity: 'info',
        }),
    },

    // After detecting an error in tool output → suggest healing
    {
        name: 'healing-after-error',
        matches: (c) => c.hadError || /error|failed|no such|denied|exception/i.test(c.lastLucyText),
        build: (c) => {
            const m = c.lastLucyText.match(/error[:\s]+([^.\n]{8,80})/i);
            const symptom = m ? m[1].trim() : 'error reciente';
            return {
                id: 'chip-healing',
                label: '¿Lo he resuelto antes?',
                tooltip: 'Busca en la memoria de Lucy fixes previos a un error parecido',
                icon: '💊',
                action: { kind: 'fill_input', text: `Busca en tu memoria si ya he resuelto algo parecido a: "${symptom}". Usa <TOOL>healing_find:${symptom}</TOOL>.` },
                severity: 'caution',
            };
        },
    },

    // After Lucy describes system state → suggest snapshot
    {
        name: 'snapshot-after-status',
        matches: (c) => /CPU|RAM|memoria|process|servicio|service|disco|disk/i.test(c.lastLucyText)
                     && !c.toolLabels.some(l => l.includes('snapshot')),
        build: (_c) => ({
            id: 'chip-snapshot',
            label: 'Capturar snapshot',
            tooltip: 'Guarda el estado actual del sistema para comparar más tarde (/snapshot)',
            icon: '◫',
            action: { kind: 'slash', command: '/snapshot' },
            severity: 'suggest',
        }),
    },

    // After a slow operation → suggest diagnose_spike
    {
        name: 'diagnose-after-slow',
        matches: (c) => /lent[oa]|slow|tard[óo]|spike|congestion|alto|high/i.test(c.lastLucyText),
        build: (_c) => ({
            id: 'chip-diagnose',
            label: '¿Por qué se puso lenta?',
            tooltip: 'Correlaciona procesos recientes con el síntoma (F3 Causal Engine)',
            icon: '△',
            action: { kind: 'fill_input', text: '¿Por qué se puso lenta mi máquina hace un momento? Usa <TOOL>diagnose_spike:300</TOOL> para investigar.' },
            severity: 'suggest',
        }),
    },

    // After git operations → suggest commit
    {
        name: 'commit-after-git',
        matches: (c) => /git\s+status|git\s+diff|sin commitear|uncommitted/i.test(c.lastLucyText),
        build: (_c) => ({
            id: 'chip-git-commit',
            label: 'Generar commit',
            tooltip: 'Crea un commit message convencional a partir del diff actual',
            icon: '⌥',
            action: { kind: 'fill_input', text: 'Genera un mensaje de commit convencional para los cambios actuales y muéstrame el comando.' },
            severity: 'info',
        }),
    },

    // After Lucy proposes a plan → confirm
    {
        name: 'confirm-plan',
        matches: (c) => /<PLAN>|plan propuesto|paso 1|step 1|primero,/i.test(c.lastLucyText)
                     && !c.lastLucyText.includes('CONFIRMADO'),
        build: (_c) => ({
            id: 'chip-confirm',
            label: 'Procede con el plan',
            tooltip: 'Confirma el plan y deja que Lucy ejecute los pasos',
            icon: '▶',
            action: { kind: 'fill_input', text: 'Confirmado, procede con el plan.' },
            severity: 'suggest',
        }),
    },

    // After incident detected → suggest state diff to investigate
    {
        name: 'investigate-incident',
        matches: (c) => /incidente|incident|anomal/i.test(c.lastLucyText) && c.hasOpenQuestion,
        build: (_c) => ({
            id: 'chip-state-diff',
            label: '¿Qué cambió?',
            tooltip: 'Compara el estado actual con el snapshot de hace 30 min',
            icon: '◐',
            action: { kind: 'fill_input', text: '¿Qué cambió en mi máquina en los últimos 30 minutos? Usa <TOOL>state_diff:30</TOOL>.' },
            severity: 'caution',
        }),
    },

    // After a verbose output → summarize
    {
        name: 'summarize-long',
        matches: (c) => c.lastLucyText.length > 1600,
        build: (_c) => ({
            id: 'chip-summary',
            label: 'Resúmeme esto',
            tooltip: 'Pide un TL;DR de 3 líneas de la última respuesta',
            icon: '☱',
            action: { kind: 'fill_input', text: 'Resúmeme la respuesta anterior en 3 líneas máximo, sólo lo esencial.' },
            severity: 'info',
        }),
    },
];

/** Generate predictive chips for the current context. */
export function predictChips(ctx: ChipContext): PredictiveChip[] {
    const out: PredictiveChip[] = [];
    const seen = new Set<string>();
    for (const rule of RULES) {
        try {
            if (!rule.matches(ctx)) continue;
            const chip = rule.build(ctx);
            if (!chip) continue;
            if (seen.has(chip.id)) continue;
            seen.add(chip.id);
            out.push(chip);
            if (out.length >= 3) break;
        } catch {
            // Defensive: a bad rule shouldn't break the strip
            continue;
        }
    }
    return out;
}

// ── Dismiss memory (per-turn sticky) ─────────────────────────────────────

const _dismissed = new Set<string>();

export function dismissChip(id: string): void { _dismissed.add(id); }
export function isDismissed(id: string): boolean { return _dismissed.has(id); }
export function resetDismissed(): void { _dismissed.clear(); }
