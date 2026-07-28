// ── plan-seed.ts — what counts as a PLAN STEP in the cockpit ────────────────
//
// Extracted from +page.svelte so the rule can be tested. It was an inline
// heuristic with no coverage, and it showed.
//
// What it does
// ------------
// The cockpit's Plan panel has no protocol behind it: there is no `<STEPS>` tag
// and none is being added here, because prompt surface is expensive and the
// model already writes its intentions in prose. (`<PLAN>` is NOT this — that is
// the destructive-action confirmation card with risk/CMD/VERIFY/ROLLBACK, a
// different thing entirely.) So the panel reads the first reasoning block and
// recognises an enumeration.
//
// Why it needed tightening
// ------------------------
// The old rule accepted ANY bulleted or numbered line of 4–120 chars, anywhere
// in the text, and showed the first two it found. Observed live, that produced:
//
//   · "Destino: Chat (no se especificó archivo)" / "Formato: Markdown"
//        — output-format metadata rendered as the plan for the whole turn
//   · "Leer el estado de los servicios es una acción de solo lectura…"
//        repeated verbatim four times — reasoning, listed twice over
//
// A panel labelled "Plan" that sometimes shows a plan and sometimes shows
// formatting metadata teaches the operator to stop reading it. Showing NOTHING
// is better than showing something untrue: with no plan the panel falls back to
// its post-hoc command log, which is always honest.
//
// Three rules, each earned from one of those failures.

/** `Clave: valor` — metadata, not an action. Kills "Destino:", "Formato:". */
const METADATA_RE = /^[\p{Lu}][\p{L}\s]{0,24}:\s/u;

/**
 * A numbered ("1." / "1)" / "1-") or bulleted ("-" / "•" / "*") item.
 *
 * The separator must be ADJACENT to the number: "1 - Revisar" does NOT match.
 * The original comment claimed it did — it never has. Left narrow on purpose
 * rather than corrected upward: `\d{1,2}\s*[.)\-]` would also swallow ranges
 * like "10 - 15 GB de RAM" as a step, and this rule exists to produce fewer
 * false steps, not more.
 */
const ITEM_RE = /^(?:\d{1,2}[.)\-]\s+|[-•*]\s+)(.{4,120})$/;

const MAX_STEPS = 8;
const MIN_STEPS = 2;

function clean(label: string): string {
    return label.trim().replace(/\*\*/g, '').replace(/`/g, '').replace(/[.:;\s]+$/, '');
}

/**
 * Extracts a forward-looking step list from a reasoning block, or `[]` when the
 * text does not contain one.
 *
 * Returning `[]` is a real answer, not a failure: most turns have no plan, and
 * the panel is correct to stay empty for them.
 */
export function extractPlanSteps(text: string | null | undefined): string[] {
    if (!text) return [];

    // RULE 1 — the list must be CONTIGUOUS.
    // A plan is written as a block. Bullets scattered across sections are a
    // report's structure, not a sequence of steps, and stitching them together
    // invents an order the model never expressed. Keep the longest run.
    let best: string[] = [];
    let run: string[] = [];
    for (const raw of String(text).split('\n')) {
        const line = raw.trim();
        const m = line.match(ITEM_RE);
        if (m) {
            const label = clean(m[1]);
            // RULE 2 — drop `Clave: valor` metadata. It is the shape that put
            // "Formato: Markdown" on screen as a step of the task.
            if (label && !/^https?:/i.test(label) && !METADATA_RE.test(label)) run.push(label);
            continue;
        }
        // A blank line does not break the run — lists are often loosely spaced.
        if (line === '') continue;
        if (run.length > best.length) best = run;
        run = [];
    }
    if (run.length > best.length) best = run;

    // RULE 3 — a repeated step is not a step.
    // The stalled turn emitted its two lines twice; showing four rows made a
    // loop look like progress. Dedupe case-insensitively, and if that leaves
    // fewer than two DISTINCT items there was no plan to begin with.
    const seen = new Set<string>();
    const unique: string[] = [];
    for (const s of best) {
        const key = s.toLowerCase();
        if (seen.has(key)) continue;
        seen.add(key);
        unique.push(s);
        if (unique.length >= MAX_STEPS) break;
    }

    return unique.length >= MIN_STEPS ? unique : [];
}
