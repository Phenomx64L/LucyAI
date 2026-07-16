// ── liveTrace.ts — transparent telemetry store for the agent loop ────────────
// Ring buffer of recent trace events so the user can see, in real time, what
// Lucy is thinking, executing, and observing. Cheap, ephemeral (no persistence).

import { writable, get } from 'svelte/store';
// ── Lucy 2.0 cockpit: mirror every trace event into the cockpit store.
//    v1.7.234 COCKPIT GA — ya no es dev-only; mismo kill-switch que +page
//    (localStorage.lucy_ui_v2 = '0'). ──
import { tracePush as _cockpitTrace } from './cockpit/agent-workspace';
const COCKPIT = (() => {
    try { return typeof localStorage !== 'undefined' && localStorage.getItem('lucy_ui_v2') !== '0'; }
    catch { return true; }
})();

export type TracePhase =
    | 'thought'       // LLM reasoning streaming
    | 'tool.start'    // a tool is about to run
    | 'tool.end'      // tool finished (success or failure)
    | 'exec.start'    // a shell/PS command is about to run
    | 'exec.end'      // command finished
    | 'llm.turn'      // a new LLM turn begins
    | 'react.reflect' // ReAct self-correction triggered
    | 'plan'          // a <PLAN> was parsed
    | 'info';         // generic info line

export interface TraceEntry {
    id: number;
    ts: number;           // Date.now()
    phase: TracePhase;
    label: string;        // short one-line summary
    detail?: string;      // optional longer text (stderr, stdout excerpt, etc.)
    ok?: boolean;         // only meaningful for .end phases
    exitCode?: number | null;
    elapsedMs?: number;
    step?: number;        // agent loop step index when applicable
    tabId?: string;
}

// Raised to 2000 — LiveTracePanel now uses virtual scrolling so it only
// renders the viewport slice.  The full buffer is kept in memory for
// scrollback and debugging.  2000 entries × ~200 bytes ≈ 400KB — well
// within budget for a desktop app.
const MAX_ENTRIES = 2000;
let _seq = 0;

export const liveTrace = writable<TraceEntry[]>([]);

/** Push a new trace entry. O(1) amortised, trims to MAX_ENTRIES. */
export function pushTrace(e: Omit<TraceEntry, 'id' | 'ts'> & { ts?: number }) {
    const entry: TraceEntry = {
        id: ++_seq,
        ts: e.ts ?? Date.now(),
        ...e,
    };
    liveTrace.update(list => {
        const next = list.length >= MAX_ENTRIES ? list.slice(-MAX_ENTRIES + 1) : list.slice();
        next.push(entry);
        return next;
    });
    if (COCKPIT) _cockpitTrace({ phase: entry.phase, label: entry.label, detail: entry.detail, step: entry.step }); // cockpit mirror
    return entry.id;
}

/** Convenience helper to measure duration between start/end pairs. */
export function traceStart(phase: 'tool.start' | 'exec.start', label: string, step?: number, tabId?: string) {
    const id = pushTrace({ phase, label, step, tabId });
    const t0 = Date.now();
    return {
        end: (ok: boolean, detail?: string, exitCode?: number | null) => {
            pushTrace({
                phase: phase === 'tool.start' ? 'tool.end' : 'exec.end',
                label,
                ok,
                detail,
                exitCode: exitCode ?? null,
                elapsedMs: Date.now() - t0,
                step,
                tabId,
            });
            return id;
        },
    };
}

export function clearTrace() { liveTrace.set([]); }

/** Parse command output to guess an exit-code signal for ReAct.
 *  Lucy's PS wrapper appends "[stderr warnings]" when stderr is non-empty, and
 *  error output often carries "CategoryInfo:", "FullyQualifiedErrorId", or a
 *  leading "ERROR:" / "Exception" marker. We return:
 *   - 0      → looks clean
 *   - 1      → stderr / exception present but command may have run
 *   - 2      → clear failure signal (Exception / FullyQualifiedErrorId / ParserError)
 *   - null   → unknown (empty or non-parseable output) */
export function inferExitCode(output: string | null | undefined): number | null {
    if (output == null) return null;
    const s = String(output);
    if (!s.trim()) return null;
    // Hard failure signals
    if (/FullyQualifiedErrorId|ParserError|CommandNotFoundException|System\.Management\.Automation\./i.test(s)) return 2;
    if (/^\s*(ERROR|Exception|Fatal)[:\s]/im.test(s)) return 2;
    // Soft stderr signals
    if (/\[stderr warnings\]/i.test(s)) return 1;
    if (/\bCategoryInfo\s*:/i.test(s)) return 1;
    if (/\b(failed|error|exception)\b/i.test(s) && /at line:|line \d+ char \d+/i.test(s)) return 2;
    return 0;
}

/** Extract a short stderr-ish excerpt (first ~3 non-empty lines mentioning error). */
export function extractErrorExcerpt(output: string, maxLines = 3): string {
    if (!output) return '';
    const lines = output.split(/\r?\n/).filter(l => l.trim());
    const errLines: string[] = [];
    for (const l of lines) {
        if (/error|exception|fail|cannot|denied|unknown|not found/i.test(l)) {
            errLines.push(l.trim());
            if (errLines.length >= maxLines) break;
        }
    }
    return errLines.join('\n');
}

/** Build the ReAct failure marker injected back into toolResults for the LLM. */
export function buildReactMarker(step: number, exitCode: number, excerpt: string, cmd: string): string {
    const head = `[TOOL FAILURE DETECTED — step ${step} | exit=${exitCode}]`;
    const body = [
        `CMD: ${cmd.length > 160 ? cmd.slice(0, 160) + '…' : cmd}`,
        excerpt ? `STDERR: ${excerpt}` : null,
        `SELF-CHECK: Before your next action, briefly state in <THOUGHT> what caused this failure and how you will adjust. If the command is inherently wrong (syntax, missing dependency, permission), switch strategy instead of retrying the same thing.`,
    ].filter(Boolean).join('\n');
    return `${head}\n${body}`;
}

/** Read-only helper. */
export function getTrace(): TraceEntry[] { return get(liveTrace); }
