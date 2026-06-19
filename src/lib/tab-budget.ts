// ── tab-budget.ts — token-aware message budgeting (pure core) ────────────────
//
// Extracted from +page.svelte `pruneTabForBudget` (refactor, v1.7.198). The
// component kept the side effects (mutating tab.messages, marking _needsDigest,
// logging); this module owns the PURE decision of WHICH messages to keep, so it
// can be unit-tested without a Svelte tab object.

export interface BudgetableMsg {
    /** estimated token cost of this message; missing/0 treated as 0. */
    tokens?: number;
}

export interface BudgetResult<M> {
    /** messages to keep, in original order (older-kept ++ recent). */
    kept: M[];
    /** how many OLDER messages were dropped. */
    droppedCount: number;
    /** total tokens before pruning. */
    totalTokens: number;
    /** approximate tokens after pruning. */
    keptTokens: number;
    /** true when even the recent block alone exceeded the budget. */
    overflowedRecent: boolean;
}

/**
 * Decide which messages survive a soft token budget.
 *
 * Returns `null` when NO change is needed (already under budget, or nothing
 * older could be dropped) — the caller should then leave the tab untouched.
 * Otherwise returns the kept set + metadata.
 *
 * Mirrors the original semantics exactly:
 *   • the most recent `keepRecent` messages are always kept verbatim;
 *   • older messages are kept newest→oldest until the remaining budget runs out;
 *   • if even the recent block exceeds the budget, only the recent block is kept
 *     (`overflowedRecent = true`), even if that drops 0 older messages.
 */
export function selectMessagesWithinBudget<M extends BudgetableMsg>(
    messages: M[],
    maxTokens: number,
    keepRecent: number,
): BudgetResult<M> | null {
    if (!messages || messages.length === 0) return null;

    let total = 0;
    for (const m of messages) total += m.tokens || 0;
    if (total <= maxTokens) return null;

    const recent = messages.slice(-keepRecent);
    let recentTokens = 0;
    for (const m of recent) recentTokens += m.tokens || 0;

    const olderBudget = maxTokens - recentTokens;
    if (olderBudget <= 0) {
        // Pathological: even the recent block exceeds budget. Keep just it.
        return {
            kept: recent,
            droppedCount: messages.length - recent.length,
            totalTokens: total,
            keptTokens: recentTokens,
            overflowedRecent: true,
        };
    }

    // Walk older messages newest→oldest, keeping until budget runs out.
    const older = messages.slice(0, -keepRecent);
    const keptOlder: M[] = [];
    let used = 0;
    for (let i = older.length - 1; i >= 0; i--) {
        const t = older[i].tokens || 0;
        if (used + t > olderBudget) break;
        used += t;
        keptOlder.unshift(older[i]);
    }
    const droppedCount = older.length - keptOlder.length;
    if (droppedCount === 0) return null; // nothing actually dropped → no change

    return {
        kept: [...keptOlder, ...recent],
        droppedCount,
        totalTokens: total,
        keptTokens: used + recentTokens,
        overflowedRecent: false,
    };
}
