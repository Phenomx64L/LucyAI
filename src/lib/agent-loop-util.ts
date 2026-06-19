// ── agent-loop-util.ts — small pure primitives for the agent loop ────────────
//
// Extracted from +page.svelte `runAI` (Phase-3 refactor, v1.7.199). The agent
// loop is the hot path where recent intermittent bugs lived, so Phase 3 moves
// out ONLY provably-pure leaf helpers (no closure/DOM/Svelte deps) — never the
// control flow — and locks each with tests.

/**
 * djb2 string hash → unsigned 32-bit int. Used by the skip-stuck detector to
 * cheaply tell whether two consecutive agent-turn responses are byte-identical
 * (the model grinding out the same output). Trims first so trailing-whitespace
 * churn between turns doesn't change the hash.
 *
 * Must stay bit-for-bit identical to the original inline version — the streak
 * counter compares hashes across turns, so any drift would silently change when
 * the loop bails.
 */
export function hashResp(s: string | null | undefined): number {
    let h = 5381;
    const str = String(s || '').trim();
    for (let i = 0; i < str.length; i++) h = ((h << 5) + h + str.charCodeAt(i)) | 0;
    return h >>> 0;
}
