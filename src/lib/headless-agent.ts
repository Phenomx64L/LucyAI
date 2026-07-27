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

import { NATIVE_READONLY_HANDLERS, type NativeHandler } from './agent-tools-native';

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
}

export interface HeadlessAgentResult {
    status: HeadlessStatus;
    /** Final model text with tool tags stripped. */
    text: string;
    /** Labels of the tools actually executed, in order. */
    steps: string[];
    /** Populated when status === 'blocked': the tag that ended the run. */
    blockedBy: string | null;
    /** Model round-trips consumed. */
    iterations: number;
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

    const steps: string[] = [];
    let context = '';
    let lastText = '';
    let iterations = 0;

    while (iterations < maxIterations) {
        iterations++;
        const resp = await opts.askLucy(prompt, context);
        lastText = String(resp ?? '');

        // A mutating request ends the run — there is no human here to confirm it.
        const mutating = findMutatingTag(lastText);
        if (mutating) {
            return {
                status: 'blocked',
                text: stripToolTags(lastText),
                steps,
                blockedBy: mutating,
                iterations,
            };
        }

        // Nothing actionable left: this reply IS the answer.
        if (!ANY_TOOL_RE.test(lastText)) {
            return { status: 'ok', text: stripToolTags(lastText), steps, blockedBy: null, iterations };
        }

        // Execute every read-only tool the reply asked for.
        const results: string[] = [];
        for (const h of handlers) {
            const m = lastText.match(h.matchRe);
            if (!m) continue;
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
            // Tags present but none matched a read-only handler — the reply wants
            // something this path cannot provide. Report it rather than looping.
            return {
                status: 'blocked',
                text: stripToolTags(lastText),
                steps,
                blockedBy: (lastText.match(/<TOOL>[^<]{0,60}/i) || ['<TOOL>'])[0],
                iterations,
            };
        }

        context = `${context}\n\n${results.join('\n\n')}`.trim();
    }

    return { status: 'max_iterations', text: stripToolTags(lastText), steps, blockedBy: null, iterations };
}
