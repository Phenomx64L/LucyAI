// ── cost-predictor — pre-flight token/cost estimation ──────────────────────
//
// Goal: tell the user what an agent run will *probably* cost before they
// start it, so they can choose a cheaper model when the task is exploratory.
//
// Approach:
//   1. Estimate input tokens from the prompt + (recent) context length.
//      Heuristic: ~4 chars per token for English/Spanish prose, ~3 for code.
//   2. Multiply input by an "iteration factor" derived from past runs of the
//      same model (read from token_usage table). Falls back to a conservative
//      4× when no history exists.
//   3. Look up provider pricing (per 1k tokens) and compute USD.
//
// The numbers are *estimates*, not contracts. UI should label them as such.

import { COST_PER_1K } from './constants';
import { formatTokens } from './text-utils';
import { getPricing } from './model-pricing';

export interface CostEstimate {
    /** Estimated input tokens for the first turn. */
    inputTokens: number;
    /** Estimated output tokens across all expected iterations. */
    outputTokens: number;
    /** Estimated total cost in USD. May be 0 for local/Ollama models. */
    usd: number;
    /** Pricing tier resolved (e.g. 'gemini', 'anthropic', 'local'). */
    provider: string;
    /** Human-readable cents/USD label, e.g. "~$0.012" or "<$0.01". */
    label: string;
    /** Confidence: 'high' (≥10 historical samples), 'med' (1-9), 'low' (none). */
    confidence: 'high' | 'med' | 'low';
}

/** Empirically-derived iteration factors per task style.
 *  These are conservative — better to over-quote than surprise the user. */
const DEFAULT_ITER_FACTOR = 4;

/**
 * Map a model id to its pricing — backed by the per-model table in
 * $lib/model-pricing.ts so the predictor matches the footer indicator
 * and the Rust calculate_cost(). Handles "::effort" suffixes.
 *
 * (`COST_PER_1K` from constants.ts is kept only for legacy callers in
 * other files; new code should consume getPricing() directly.)
 */
function pricingFor(model: string): { in: number; out: number; provider: string; iterFactor: number } {
    const p = getPricing(model);
    return { in: p.inputPer1K, out: p.outputPer1K, provider: p.provider, iterFactor: p.iterFactor };
}

/**
 * Estimate tokens from a string. Char-based heuristic that works for both
 * EN/ES prose AND code (which has fewer tokens per char than prose).
 *
 * NOT exact (the real tokenizer is in tiktoken_rs on the backend), but
 * accurate enough for ±15% UI predictions.
 */
export function estimateTokens(text: string): number {
    if (!text) return 0;
    const codeFences = (text.match(/```/g)?.length ?? 0) / 2;
    const codeChars  = codeFences > 0 ? text.length * 0.4 : 0;
    const proseChars = text.length - codeChars;
    return Math.ceil(proseChars / 4 + codeChars / 3);
}

/**
 * Build a full pre-flight estimate.
 *
 * @param prompt        the user's input
 * @param contextChars  approximate length of the context window currently in use
 * @param model         model id Lucy will invoke
 * @param history       optional list of prior runs `{tokens_in, tokens_out}` from
 *                      the metrics DB — used to refine iter factor
 */
export function predictCost(
    prompt: string,
    contextChars: number,
    model: string,
    history: Array<{ tokens_in: number; tokens_out: number }> = []
): CostEstimate {
    const px = pricingFor(model);
    const inputTokens = estimateTokens(prompt) + Math.ceil(contextChars / 4);

    // Iteration factor: prefer historical (out / in) ratio for THIS model
    // when we have data — otherwise fall back to the per-model table value
    // (which already encodes the "::effort" multiplier, so Opus 4.7::max
    // predicts 20× the output tokens of Sonnet 4.6::low, as it should).
    // Capped to [1.5, 20] so degenerate history can't blow up the estimate.
    let iterFactor = px.iterFactor || DEFAULT_ITER_FACTOR;
    let confidence: CostEstimate['confidence'] = 'low';
    if (history.length > 0) {
        const ratios = history
            .map(h => h.tokens_in > 0 ? h.tokens_out / h.tokens_in : null)
            .filter((r): r is number => r !== null && Number.isFinite(r));
        if (ratios.length > 0) {
            const avg = ratios.reduce((a, b) => a + b, 0) / ratios.length;
            iterFactor = Math.max(1.5, Math.min(20, avg));
            confidence = ratios.length >= 10 ? 'high' : 'med';
        }
    }
    const outputTokens = Math.ceil(inputTokens * iterFactor);

    const usd =
        (inputTokens  / 1000) * px.in +
        (outputTokens / 1000) * px.out;

    let label: string;
    if (px.provider === 'local')      label = 'free (local)';
    else if (usd < 0.01)              label = '<$0.01';
    else if (usd < 1)                 label = `~$${usd.toFixed(3)}`;
    else                              label = `~$${usd.toFixed(2)}`;

    return {
        inputTokens,
        outputTokens,
        usd,
        provider: px.provider,
        label,
        confidence,
    };
}

/**
 * Compare estimates across multiple models. Useful for the "you'd save X
 * by using Gemini Flash instead" UI hint.
 *
 * Returns models sorted by usd ascending, cheapest first.
 */
export function compareModels(
    prompt: string,
    contextChars: number,
    candidates: string[],
    historyByModel: Record<string, Array<{ tokens_in: number; tokens_out: number }>> = {}
): Array<CostEstimate & { model: string }> {
    return candidates
        .map(m => ({
            model: m,
            ...predictCost(prompt, contextChars, m, historyByModel[m] || []),
        }))
        .sort((a, b) => a.usd - b.usd);
}

/**
 * Pretty-format an estimate for inline display in chat input UI.
 * E.g. "~12k in / ~48k out · ~$0.012 (med)".
 */
export function formatEstimate(est: CostEstimate): string {
    const cnf = est.confidence === 'low' ? ' (low conf.)' : '';
    return `${formatTokens(est.inputTokens)} in / ${formatTokens(est.outputTokens)} out · ${est.label}${cnf}`;
}
