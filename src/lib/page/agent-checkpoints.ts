// ── page/agent-checkpoints.ts ────────────────────────────────────────────
//
// Agent-loop checkpointing extracted from `+page.svelte` (Sprint D).
//
// When Lucy's agent loop runs a multi-step task (think → execute → reflect
// → repeat), each intermediate state is snapshotted to localStorage so a
// crash, reload, or accidental close doesn't silently erase progress. We
// don't auto-resume — that's a UX decision the surface layer makes after
// inspecting `listStaleCheckpoints()`. This module is just the storage
// engine: write, clear, enumerate.
//
// Storage format: one key per tab (`lucy_agent_ckpt_<tabId>`) holding a
// trimmed JSON snapshot. Context tail is bounded to 30K chars; HTML to 8K;
// goal to 2K. That keeps total localStorage usage under a quota even with
// many open agent loops.

import { safeSetLS, safeRemoveLS, safeParseLS } from '$lib/safe-ls';

const CKPT_PREFIX  = 'lucy_agent_ckpt_';
const CKPT_MAX_CTX = 30_000;

export interface CheckpointInput {
    loop_i?: number;
    goal?: string;
    stepsHtml?: string;
    agentCtx?: string;
    editCountsByPath?: Map<string, number>;
    toolCallCounts?:   Map<string, number>;
    filesMod?: Iterable<string>;
    agentToolCards?: Array<{
        icon?: string;
        label?: string;
        kind?: string;
        status?: string;
        duration?: number;
    }>;
    model?: string;
    title?: string;
}

export interface Checkpoint {
    ts: number;
    loop_i: number;
    goal: string;
    stepsHtml: string;
    agentCtxTail: string;
    editCounts: Array<[string, number]>;
    toolCounts: Array<[string, number]>;
    filesMod: string[];
    toolCardsMeta: Array<{
        icon?: string;
        label?: string;
        kind?: string;
        status?: string;
        duration?: number;
    }>;
    model: string;
    title: string;
}

/** Persist a checkpoint snapshot for the given tab. Trims oversized fields. */
export function saveCheckpoint(tabId: string | number, data: CheckpointInput): void {
    try {
        const snap: Checkpoint = {
            ts:           Date.now(),
            loop_i:       data.loop_i ?? 0,
            goal:         (data.goal || '').slice(0, 2000),
            stepsHtml:    (data.stepsHtml || '').slice(0, 8000),
            agentCtxTail: (data.agentCtx || '').slice(-CKPT_MAX_CTX),
            editCounts:   Array.from((data.editCountsByPath || new Map()).entries()),
            toolCounts:   Array.from((data.toolCallCounts   || new Map()).entries()),
            filesMod:     Array.from(data.filesMod || []),
            toolCardsMeta: (data.agentToolCards || []).map(c => ({
                icon: c.icon, label: c.label, kind: c.kind,
                status: c.status, duration: c.duration,
            })),
            model: data.model || '',
            title: data.title || '',
        };
        safeSetLS(CKPT_PREFIX + tabId, snap);
    } catch (e) {
        // Quota exceeded or tab closed — non-fatal
        // eslint-disable-next-line no-console
        console.warn('[checkpoint] save failed:', e);
    }
}

/** Drop the checkpoint for a tab (e.g. after successful completion). */
export function clearCheckpoint(tabId: string | number): void {
    safeRemoveLS(CKPT_PREFIX + tabId);
}

/** Enumerate every checkpoint currently in localStorage. */
export interface CheckpointEntry {
    key: string;
    tabId: string;
    snap: Checkpoint;
}
export function listStaleCheckpoints(): CheckpointEntry[] {
    const out: CheckpointEntry[] = [];
    try {
        for (let i = 0; i < localStorage.length; i++) {
            const k = localStorage.key(i);
            if (!k || !k.startsWith(CKPT_PREFIX)) continue;
            try {
                const snap = safeParseLS(k, {} as Checkpoint);
                out.push({ key: k, tabId: k.slice(CKPT_PREFIX.length), snap });
            } catch { /* skip corrupt entries */ }
        }
    } catch { /* localStorage unavailable (private mode, etc.) */ }
    return out;
}

// ── Misc small utilities co-located here because they're 1-liners that
//    didn't warrant their own file. Group them into a `helpers` namespace
//    to keep the import surface tidy.

/** Match Windows registry hives/paths that must never be edited from chat. */
export const isSensitiveRegistry = (keyPath: string): boolean => {
    if (/^(SAM|SECURITY|SYSTEM|CurrentUser\\Identities|\.DEFAULT\\Volatile)$/i.test(keyPath)) return true;
    const lc = keyPath.toLowerCase();
    return lc.includes('password') || lc.includes('credential');
};
