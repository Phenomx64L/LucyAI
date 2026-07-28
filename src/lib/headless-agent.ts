// ── headless-agent.ts — the agent loop, without a UI ────────────────────────
//
// Continues the de-monolithing effort that `agent-tools.ts` started at v1.7.212.
//
// WHY THIS EXISTS
// ---------------
// The real agent loop lives inside `runAI()` in `+page.svelte` — 4,842 lines
// welded to component state (addMsg, refresh, tab bookkeeping). Anything that
// is not a chat tab therefore had NO way to run tools. The scheduler hit that
// wall head-on: it called the single-shot `ask_lucy` command, got back a reply
// full of `<TOOL>` tags, executed none of them, and stored the raw tags while
// marking the row `ok`. A "daily health check" reported success every morning
// having never looked at the machine.
//
// This module is the smallest honest fix: a bounded loop that drives the
// ALREADY-EXTRACTED, already-tested handler table from `agent-tools-native.ts`.
// It is not a port of runAI — it deliberately does far less.
//
// THE SAFETY BOUNDARY (read this before widening it)
// --------------------------------------------------
// Only `NATIVE_READONLY_HANDLERS` are executed. No shell, no `<EXECUTE>`, no
// writefile/editfile, no remote hosts. That is not an oversight — it is the
// whole reason this is safe to run unattended.
//
// Lucy's central safety invariant is that mutating commands are PREFILLED for a
// human, never auto-executed. A scheduled task runs at 03:00 with nobody
// watching, so there is no human in that loop to confirm anything. Read-only
// tools keep the invariant intact: the worst case is a stale report, not a
// stopped service.
//
// If a response asks for a mutating tool, the run STOPS and reports
// `status: 'blocked'`. It does not silently drop the tag and carry on — that
// would recreate the same "looks fine, did nothing" failure in a new place.
//
// Pure except for the handlers it is given: `askLucy` and `handlers` are both
// injected, so the loop unit-tests without Tauri.

import {
    NATIVE_READONLY_HANDLERS,
    NATIVE_READONLY_HANDLERS_DEPS,
    type NativeHandler,
    type NativeHandlerDeps,
} from './agent-tools-native';

/**
 * The deps-coupled tools a background sub-agent may use.
 *
 * An EXPLICIT ALLOW-LIST, not "everything in NATIVE_READONLY_HANDLERS_DEPS",
 * and the difference matters: that table's name has drifted from its contents.
 * It also holds `start_indexer` (kicks off a background indexing job — a write
 * by any honest reading), `obj_query` (needs a live PowerShell session bound to
 * a real tab, which a fork does not have), and the network-egress tools
 * (`fetch`, `search_web`, `mcp_discover`). Inheriting that list wholesale would
 * hand an unattended agent more than "look at the machine".
 *
 * What is here is exactly machine inspection: the questions a background
 * diagnostic sub-agent needs to answer and nothing else. Widening this list is
 * a security decision — make it deliberately, not by adding to the table above.
 */
export const SUBAGENT_DEPS_TOOLS = [
    'sysinfo', 'netconn', 'tasklist', 'eventlog', 'registry', 'system_diff',
] as const;

/**
 * Binds deps-coupled handlers into the plain `NativeHandler` shape the loop
 * drives, so a caller can hand `runHeadlessAgent` one flat array.
 *
 * Unknown names are dropped silently rather than throwing: the allow-list is a
 * policy statement, and a tool being renamed in the table should narrow what a
 * sub-agent can do, never crash the turn that launched it.
 */
export function bindDepsHandlers(kinds: readonly string[], deps: NativeHandlerDeps): NativeHandler[] {
    return NATIVE_READONLY_HANDLERS_DEPS
        .filter((h) => kinds.includes(h.kind))
        .map((h) => ({
            kind: h.kind,
            matchRe: h.matchRe,
            stripRe: h.stripRe,
            build: (m: RegExpMatchArray) => h.build(m, deps),
        }));
}

/** Tags that mean "change the machine". Their presence ends an unattended run. */
const MUTATING_RE = /<EXECUTE|<TOOL>(writefile|editfile|panic_kill|cd_change|cd|fork_task|memoria_eliminar|memory_core_delete|principle_set|principle_delete|schedule_create|start_indexer):/i;

/** Any actionable tag at all — used to tell "answered" from "wanted a tool". */
const ANY_TOOL_RE = /<TOOL>|<EXECUTE/i;

export type HeadlessStatus = 'ok' | 'blocked' | 'max_iterations';

export interface HeadlessAgentOptions {
    /** Calls the model. `context` carries accumulated tool output. */
    askLucy: (prompt: string, context: string) => Promise<string>;
    /** Defaults to the read-only native table. Override only in tests. */
    handlers?: NativeHandler[];
    /** Hard ceiling on model round-trips. Unattended runs must terminate. */
    maxIterations?: number;
    /** Optional progress sink (logging, live trace). */
    onStep?: (label: string) => void;
    /**
     * Final synthesis pass when the loop cannot continue. ON by default.
     *
     * Without it the loop ABANDONS: the first live run produced four
     * sub-agents that between them called real tools and returned nothing
     * usable — two asked for a console command and were refused, two re-ran the
     * same read four times until the ceiling. Every one of them was holding
     * tool output it never turned into an answer.
     *
     * Set false only when the caller wants the raw failure (tests).
     */
    synthesize?: boolean;
}

export interface HeadlessAgentResult {
    status: HeadlessStatus;
    /** Final model text with tool tags stripped. */
    text: string;
    /** Labels of the tools actually executed, in order. */
    steps: string[];
    /** Populated when status === 'blocked': the tag that ended the run. */
    blockedBy: string | null;
    /** Model round-trips consumed (the synthesis pass included). */
    iterations: number;
    /** True when `text` came from the forced synthesis rather than a clean finish. */
    synthesized?: boolean;
    /** Why the loop had to stop early — carried so the caller can say so. */
    note?: string;
}

/** Removes every tool/thought tag so stored output reads as prose. */
export function stripToolTags(text: string): string {
    return text
        .replace(/<TOOL>[\s\S]*?<\/TOOL>/gi, '')
        .replace(/<THOUGHT>[\s\S]*?<\/THOUGHT>/gi, '')
        .replace(/<EXECUTE[^>]*>[\s\S]*?<\/EXECUTE[^>]*>/gi, '')
        .replace(/\n{3,}/g, '\n\n')
        .trim();
}

/** First mutating tag in `text`, or null. Used for the blocked report. */
export function findMutatingTag(text: string): string | null {
    const m = MUTATING_RE.exec(text);
    return m ? m[0] : null;
}

/**
 * Runs a bounded, read-only agent loop.
 *
 * Terminates on the first of: a reply with no tool tags (`ok`), a reply asking
 * for a mutating tool (`blocked`), or the iteration ceiling (`max_iterations`).
 */
export async function runHeadlessAgent(
    prompt: string,
    opts: HeadlessAgentOptions,
): Promise<HeadlessAgentResult> {
    const handlers = opts.handlers ?? NATIVE_READONLY_HANDLERS;
    const maxIterations = Math.max(1, opts.maxIterations ?? 4);
    const wantSynthesis = opts.synthesize !== false;

    const steps: string[] = [];
    const ranKinds = new Set<string>();
    let context = '';
    let lastText = '';
    let iterations = 0;

    /**
     * One last call with tools off, answering from what was already gathered.
     *
     * This is the difference between a sub-agent that returns "[BLOQUEADO]" and
     * one that returns the event-log summary it had already fetched. Whatever
     * stopped the loop, the collected output is still worth an answer — the
     * caller can judge it, and a partial answer beats none.
     */
    async function synthesize(status: HeadlessStatus, note: string, blockedBy: string | null): Promise<HeadlessAgentResult> {
        const bail = (): HeadlessAgentResult => ({
            status, text: stripToolTags(lastText), steps, blockedBy, iterations, note,
        });
        if (!wantSynthesis || !context.trim()) return bail();
        iterations++;
        try {
            const finalResp = await opts.askLucy(
                `${prompt}\n\n[CIERRE — ${note}]\nNo dispones de más herramientas en este paso. Responde AHORA con los datos ya recogidos que aparecen en el contexto. No emitas ninguna etiqueta de herramienta. Si los datos son parciales, dilo y resume lo que sí tengas.`,
                context,
            );
            const text = stripToolTags(String(finalResp ?? ''));
            // A synthesis that came back empty (or as pure tool tags again) is
            // worse than the raw reply — fall back rather than return nothing.
            if (!text) return bail();
            return { status, text, steps, blockedBy, iterations, synthesized: true, note };
        } catch {
            return bail();
        }
    }

    while (iterations < maxIterations) {
        iterations++;
        const resp = await opts.askLucy(prompt, context);
        lastText = String(resp ?? '');

        // A mutating request ends the LOOP — there is no human here to confirm
        // it — but not the turn: whatever was gathered still gets synthesized.
        // Observed live: sub-agents reach for <EXECUTE> even when told they have
        // no shell, because the full system prompt advertises it. Refusing and
        // then discarding their tool output wasted the whole fork.
        const mutating = findMutatingTag(lastText);
        if (mutating) return synthesize('blocked', `pidió una acción no permitida (${mutating})`, mutating);

        // Nothing actionable left: this reply IS the answer.
        if (!ANY_TOOL_RE.test(lastText)) {
            return { status: 'ok', text: stripToolTags(lastText), steps, blockedBy: null, iterations };
        }

        // Execute every read-only tool the reply asked for — skipping any kind
        // already run. Live runs showed models re-emitting the SAME tag every
        // iteration until the ceiling, burning four round-trips on one read: the
        // tool output is in the context, but the model does not recognise its own
        // work as done. Re-running it cannot add information, so it is skipped
        // and the repetition itself becomes the signal to go synthesize.
        const results: string[] = [];
        let repeated = 0;
        for (const h of handlers) {
            const m = lastText.match(h.matchRe);
            if (!m) continue;
            if (ranKinds.has(h.kind)) { repeated++; continue; }
            ranKinds.add(h.kind);
            const task = h.build(m);
            steps.push(task.label);
            opts.onStep?.(task.label);
            try {
                results.push(await task.fn());
            } catch (e) {
                // A failed tool is data for the next turn, not a crash: the model
                // can say "the event log was unreadable" instead of the run dying.
                results.push(`[${h.kind} ERROR] ${String(e).slice(0, 300)}`);
            }
        }

        if (results.length === 0) {
            // Asked only for tools already run: no new information is coming, so
            // looping again would just spend the ceiling. Answer with what we have.
            if (repeated > 0) return synthesize('ok', 'repitió herramientas ya ejecutadas', null);
            // Tags present but none matched an available handler.
            return synthesize(
                'blocked',
                'pidió una herramienta no disponible',
                (lastText.match(/<TOOL>[^<]{0,60}/i) || ['<TOOL>'])[0],
            );
        }

        context = `${context}\n\n${results.join('\n\n')}`.trim();
    }

    if (wantSynthesis) return synthesize('max_iterations', `agotó ${iterations} pasos`, null);

    // Only reachable with synthesis disabled.
    return { status: 'max_iterations', text: stripToolTags(lastText), steps, blockedBy: null, iterations };
}
