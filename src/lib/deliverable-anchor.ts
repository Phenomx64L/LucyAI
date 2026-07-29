// ── deliverable-anchor.ts — keep the last delivered artifact reachable ───────
//
// v1.8.1. Extracted from `+page.svelte`'s context builder so the decision is
// unit-testable (the component keeps the side effects: reading the tab and
// concatenating the prompt).
//
// THE PROBLEM
// The conversation history sent to the model is rebuilt from `tab.messages`
// under two independent cuts: the `compaction.keepFrom` verbatim window and the
// `contextMax` character walk. A long agent run inflates the tab enough to
// trigger both — and a generated report is the single largest message, so it is
// the FIRST thing evicted. The user then asks Lucy to act on the report she just
// wrote ("export this to PDF") and gets "I have no report loaded in the context
// of our conversation": literally true, indistinguishable from amnesia.
//
// THE FIX
// The agent stores its last substantial output on the tab, outside `agentCtx`,
// where neither the rolling window nor the tab compaction can reach it. This
// module decides whether it still needs re-injecting and formats the block.

export interface Deliverable {
    /** The delivered text (report, summary, generated document). */
    text: string;
    /** When it was produced — informational only. */
    ts?: number;
    /** The user request that produced it, for re-grounding the model. */
    goal?: string;
}

/** Chars of the deliverable used to detect it is still in verbatim history. */
export const PROBE_LEN = 200;

/** Default cap — generous enough to re-emit or export a full report. */
export const DEFAULT_ANCHOR_MAX = 24_000;

/**
 * Build the anchor block to prepend to the prompt context.
 *
 * Returns `''` (nothing to inject) when there is no deliverable, when its text
 * is blank, or when it is STILL present verbatim in the supplied history — in
 * that last case re-injecting would just pay for the same text twice.
 *
 * `recentContents` is the raw content of the messages that survived the cuts.
 */
export function buildDeliverableAnchor(
    deliverable: Deliverable | null | undefined,
    recentContents: readonly string[],
    maxChars: number = DEFAULT_ANCHOR_MAX,
): string {
    const text = String(deliverable?.text ?? '').trim();
    if (!text) return '';

    const probe = text.slice(0, PROBE_LEN);
    for (const c of recentContents) {
        if (String(c ?? '').includes(probe)) return '';
    }

    // A cap of 0 or less would produce a header with no body — treat any
    // non-positive cap as "do not inject" rather than emitting a useless stub.
    if (maxChars <= 0) return '';

    const truncated = text.length > maxChars;
    const body = truncated ? text.slice(0, maxChars) : text;
    const goal = String(deliverable?.goal ?? '').trim();

    return (
        '--- ÚLTIMO ENTREGABLE (lo produjiste tú; sigue vigente aunque ya no esté en el historial) ---\n' +
        (goal ? `Petición original: "${goal}"\n\n` : '') +
        body +
        (truncated
            ? `\n\n[… truncado: el entregable completo tenía ${text.length.toLocaleString()} caracteres]`
            : '') +
        '\n--- FIN ÚLTIMO ENTREGABLE ---\n\n'
    );
}
