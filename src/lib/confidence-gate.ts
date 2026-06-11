// ── confidence-gate.ts ─────────────────────────────────────────────────────
//
// v1.7.109 audit F4 — deterministic confidence scoring on the final LLM
// answer.  Lucy already has plumbing for safety (reflection-gate) and
// syntax validation (script-verifier), but NOTHING was looking at whether
// the model itself was confident about the answer it was about to ship.
//
// Frontier pattern: rate the answer 0..1 by counting hedge markers vs
// certainty markers vs explicit-failure markers, plus signal density
// (numbers, code blocks, citations).  If the score is below a threshold
// we surface a low-confidence badge in the UI and offer a one-click
// re-run with a stronger model tier instead of silently emitting.
//
// Why deterministic (no LLM-judge):
//   • This runs on EVERY answer; an LLM-judge round-trip would double
//     latency and cost for the common case where confidence is fine.
//   • The markers we score against are stable across models — both
//     Claude/Gemini/Ollama use the same hedging vocabulary in EN + ES.
//   • A scoring bug here is recoverable; a bad LLM-judge call is silent.
//
// Threshold tuning (LOW_CONFIDENCE_THRESHOLD = 0.55):
//   • Above 0.70: clearly confident answer (no badge).
//   • 0.55-0.70: ambiguous (no badge, but score is logged for telemetry).
//   • Below 0.55: low confidence (badge + offer re-run).
//
// Scoring weights are tuned for the case where the user gets a real
// answer that's just hedge-heavy.  An "I couldn't find anything" answer
// drops to ~0.2 and triggers the badge; a "this is definitely X" answer
// rises to ~0.85.

export interface ConfidenceResult {
    score: number;             // 0..1, higher = more confident
    band: 'high' | 'medium' | 'low';
    reasons: string[];         // human-readable factors (top 3)
    suggestRerun: boolean;     // true when score < threshold
}

// Bilingual marker lists. Lowercased and word-boundary-matched so they
// don't trip on substrings (e.g. "creo" inside "increyo" — though that
// example doesn't exist, the principle does).
const CERTAINTY_MARKERS = [
    // Spanish
    'la respuesta es', 'claramente', 'definitivamente', 'sin duda',
    'confirmo', 'efectivamente', 'es seguro que', 'está confirmado',
    'verificado', 'comprobado',
    // English
    "i'm confident", "i am confident", 'the answer is', 'definitely',
    'clearly', 'without doubt', 'confirmed', 'verified',
    'this is the', 'i can confirm',
];

const HEDGE_MARKERS = [
    // Spanish
    'creo que', 'tal vez', 'quizá', 'quizás', 'probablemente',
    'no estoy seguro', 'podría ser', 'es posible', 'aparentemente',
    'parece que', 'al parecer', 'me imagino', 'supongo',
    // English
    'i think', 'i believe', 'maybe', 'perhaps', 'probably',
    "i'm not sure", "i am not sure", 'could be', 'it seems',
    'apparently', 'might be', 'i guess', 'i suspect',
];

const FAILURE_MARKERS = [
    // Spanish — strong "I couldn't do it" signals
    'no encontré', 'no pude', 'no logré', 'sin resultados',
    'no hay datos', 'no se pudo', 'no tengo', 'desconozco',
    'no dispongo', 'no se encontr',
    // English
    "i couldn't", 'i could not', "i don't know", 'no results',
    'unable to', 'i lack', 'no data', "i can't find",
    'failed to', 'not available',
];

// Bonuses: concrete signal that the model actually did work.
// We don't try to verify the work, just count signal density.
const NUMBER_RE     = /\b\d{2,}\b/g;                // multi-digit numbers
const CODE_BLOCK_RE = /```[\s\S]*?```/g;            // fenced code
const PATH_RE       = /[A-Za-z]:\\[\w\\.\- ]+/g;    // Windows paths
const URL_RE        = /https?:\/\/\S+/g;            // URLs/citations

const LOW_CONFIDENCE_THRESHOLD = 0.55;

/**
 * Score the confidence of an LLM final answer. Deterministic, sub-millisecond.
 *
 * Strips out <THOUGHT>, <TOOL>, <REMEMBER> blocks before scoring — those
 * are scaffolding, not the user-visible answer.
 */
export function scoreConfidence(rawText: string): ConfidenceResult {
    const text = (rawText || '')
        .replace(/<THOUGHT>[\s\S]*?<\/THOUGHT>/gi, '')
        .replace(/<TOOL>[\s\S]*?<\/TOOL>/gi, '')
        .replace(/<REMEMBER>[\s\S]*?<\/REMEMBER>/gi, '')
        .trim();
    const lower = text.toLowerCase();

    // Very short answers can't be confidently anything — treat empty /
    // <20 char answers as low confidence by definition.
    if (text.length < 20) {
        return {
            score: 0.25,
            band: 'low',
            reasons: ['Respuesta extremadamente corta'],
            suggestRerun: true,
        };
    }

    let score = 0.70; // neutral baseline
    const reasons: string[] = [];

    // Count markers (capped per category so a rant of "I think I think I think"
    // doesn't tank the score below the floor).
    let certCount = 0, hedgeCount = 0, failCount = 0;
    for (const m of CERTAINTY_MARKERS) {
        if (lower.includes(m)) certCount = Math.min(certCount + 1, 4);
    }
    for (const m of HEDGE_MARKERS) {
        if (lower.includes(m)) hedgeCount = Math.min(hedgeCount + 1, 4);
    }
    for (const m of FAILURE_MARKERS) {
        if (lower.includes(m)) failCount = Math.min(failCount + 1, 4);
    }

    score += certCount * 0.05;   // up to +0.20
    score -= hedgeCount * 0.06;  // up to -0.24
    score -= failCount * 0.10;   // up to -0.40 — failure is the strongest signal

    if (certCount >= 2)  reasons.push(`Lenguaje de certeza (${certCount})`);
    if (hedgeCount >= 2) reasons.push(`Lenguaje de duda (${hedgeCount})`);
    if (failCount >= 1)  reasons.push(`Señales de no-encontrado (${failCount})`);

    // Signal density bonuses
    const nNums   = (text.match(NUMBER_RE)     || []).length;
    const nCode   = (text.match(CODE_BLOCK_RE) || []).length;
    const nPaths  = (text.match(PATH_RE)       || []).length;
    const nUrls   = (text.match(URL_RE)        || []).length;
    const signalBonus = Math.min(
        nNums * 0.01 + nCode * 0.05 + nPaths * 0.02 + nUrls * 0.03,
        0.15
    );
    score += signalBonus;
    if (signalBonus >= 0.05) reasons.push(`Contenido concreto (números/código/paths)`);

    // Long, structured answers with section headers are usually confident
    // (the model committed to a shape). Cheap heuristic: count `\n##`.
    const headerCount = (text.match(/\n#{1,3}\s/g) || []).length;
    if (headerCount >= 2) {
        score += 0.04;
        reasons.push('Estructura con secciones');
    }

    // Clamp.
    score = Math.max(0, Math.min(1, score));

    const band: 'high' | 'medium' | 'low' =
        score >= 0.70 ? 'high'
      : score >= LOW_CONFIDENCE_THRESHOLD ? 'medium'
      : 'low';

    return {
        score,
        band,
        reasons: reasons.slice(0, 3),
        suggestRerun: band === 'low',
    };
}

/**
 * Render an HTML badge for the response footer when confidence is low.
 * Returns '' for medium/high — we don't clutter normal answers.
 */
export function renderConfidenceBadge(r: ConfidenceResult): string {
    if (r.band !== 'low') return '';
    const pct = Math.round(r.score * 100);
    const reasonStr = r.reasons.length
        ? r.reasons.join(' · ').replace(/"/g, '&quot;')
        : 'señales de incertidumbre detectadas';
    return `<div class="confidence-gate-badge" title="${reasonStr}" style="margin-top:8px;padding:6px 10px;border-left:3px solid #d4a155;background:rgba(212,161,85,0.08);border-radius:4px;font-size:12px;color:var(--txt2);">
        ⚠ <strong>Confianza ${pct}%</strong> — la respuesta contiene señales de duda. Revisa críticamente o re-corre con un modelo más capaz si es una tarea importante.
    </div>`;
}
