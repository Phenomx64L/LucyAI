// ── tool-result-classify.ts — anti-hallucination tool-result triage ──────────
//
// Extracted from +page.svelte `runAI` (Phase-3 refactor, v1.7.200). Pure: given
// a turn's tool-result strings, count how many were empty / errored / hit a
// PowerShell parse error. The component uses the counts to decide whether to
// inject the "NO USABLE DATA — don't fabricate" guard. Pulling the counting out
// makes the classification (which had real "Lucy invents results when output is
// empty" bug history) unit-testable.

// All regexes are non-global → `.test()` is stateless → safe as module consts.
const EMPTY_MARKERS: RegExp[] = [
    /\(sin salida\)/i, /\(no output\)/i, /\bempty\b/i, /no se encontraron/i,
    /not found/i, /no results/i, /returned no output/i, /no se devolvieron/i,
];
const ERROR_MARKERS: RegExp[] = [
    /^\[.*ERROR\]/im, /\[stderr warnings\]/i, /Reg Error/i, /ParserError/i,
    /CommandNotFoundException/i, /FullyQualifiedErrorId/i,
    /Acceso denegado|Access is denied/i, /POWERSHELL ERROR/i,
];
const PS_PARSE_RE = /El literal de hash estaba incompleto|hash literal was incomplete|Token .* inesperado|Unexpected token|Falta un bloque (Catch|Finally)|Missing (Catch|Finally) block|sintaxis no es válida|is not a valid (script|syntax)/i;

export interface ToolResultClassification {
    totalToolCalls: number;
    emptyCount: number;
    errorCount: number;
    psParseErrorCount: number;
}

/**
 * Strip the boilerplate framing that wraps tool outputs ("[EXECUTION RESULT —
 * step N]", warpBlock header rows, PS prompts) so the BODY can be measured for
 * actual content vs just headers.
 */
export function stripFraming(s: string | null | undefined): string {
    return String(s || '')
        .replace(/^\[[^\]]+\]\s*\n?/gm, '')   // bracket-headers
        .replace(/^---[^\n]*---\s*\n?/gm, '')  // markdown rules
        .replace(/^PS\s*>\s*[^\n]*\n?/gm, '')  // PS prompt lines
        .trim();
}

/**
 * Classify each tool-result string for the anti-hallucination guard. Order of
 * checks matches the original inline loop exactly:
 *   1. blank / < 30 chars            → empty
 *   2. body (framing-stripped) < 25  → empty
 *   3. matches an EMPTY marker        → empty
 *   4. matches an ERROR marker        → error (+ PS-parse tally)
 */
export function classifyToolResults(toolResults: unknown[]): ToolResultClassification {
    const totalToolCalls = Array.isArray(toolResults) ? toolResults.length : 0;
    let emptyCount = 0;
    let errorCount = 0;
    let psParseErrorCount = 0;

    for (const r of (toolResults || [])) {
        const raw = String(r ?? '').trim();
        if (!raw || raw.length < 30) { emptyCount++; continue; }
        const body = stripFraming(raw);
        if (!body || body.length < 25) { emptyCount++; continue; }
        if (EMPTY_MARKERS.some((re) => re.test(raw))) { emptyCount++; continue; }
        if (ERROR_MARKERS.some((re) => re.test(raw))) {
            errorCount++;
            if (PS_PARSE_RE.test(raw)) psParseErrorCount++;
            continue;
        }
    }
    return { totalToolCalls, emptyCount, errorCount, psParseErrorCount };
}
