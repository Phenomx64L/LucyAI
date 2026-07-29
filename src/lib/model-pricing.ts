// ── model-pricing.ts — Per-model cost table (single source of truth) ──────
//
// Lucy used to have ONE price per provider (anthropic/openai/gemini), which
// over-charged Sonnet by 5× and under-charged Opus by 5×. This module gives
// each model its real per-1K rate so the footer cost indicator and the
// pre-flight estimator both match what the user will actually be billed.
//
// Conventions:
//   • inputPer1K  / outputPer1K  → USD per 1 000 tokens
//   • iterFactor  → conservative multiplier for OUTPUT token prediction.
//                   Used ONLY by cost-predictor.ts to project how many
//                   tokens an agent loop will spend across iterations.
//   • Effort suffix "<id>::low|medium|high|xhigh|max" affects iterFactor
//     (more thinking ⇒ more output tokens) but NEVER the per-token price.
//
// Prices last refreshed: May 2026, aligned with the lineup in models.js.
// When you bump a model, also update src-tauri/src/utils/model_pricing.rs
// so Rust's calculate_cost() returns the same number.

export interface ModelPricing {
    /** USD per 1 000 input tokens. */
    inputPer1K: number;
    /** USD per 1 000 output tokens (incl. thinking tokens on adaptive-thinking models). */
    outputPer1K: number;
    /** Conservative multiplier on predicted output tokens (default for non-effort variants). */
    iterFactor: number;
    /** Provider tag, used for grouped UI hints. */
    provider: 'anthropic' | 'openai' | 'gemini' | 'nvidia' | 'local';
}

// Effort multipliers — applied to iterFactor when the model id ends in "::<level>".
// Calibration: max emits ~3× more thinking tokens than high for Claude Opus 4.7;
// Gemini xhigh / Pro high emit ~2× more than medium. These are conservative
// upper bounds — better to over-quote than surprise the user.
const EFFORT_FACTOR: Record<string, number> = {
    low: 0.5,
    medium: 1.0,
    high: 1.5,
    xhigh: 3.0,
    max: 5.0,
    // Spanish aliases (mirror the ones accepted in ai.rs resolvers)
    bajo: 0.5,
    medio: 1.0,
    alto: 1.5,
    'extra-alto': 3.0,
    maximo: 5.0,
};

// ── Per-model table (May 2026 lineup) ────────────────────────────────────
// All values per-1K-tokens in USD. Sources: public price pages as of May 2026.
// When in doubt err HIGH so the cost indicator doesn't surprise the user.
export const MODEL_PRICING: Record<string, ModelPricing> = {
    // ── Anthropic Claude ──────────────────────────────────────────────
    // CORRECTED: Opus 4.6/4.7/4.8 were carrying Opus 4.5's $15/$75 per 1M —
    // 3× the real price — so the footer indicator and the pre-flight estimate
    // both over-quoted every Opus turn by 3×. The Opus tier came down to
    // $5/$25 with 4.6 and has stayed there through Opus 5. Opus 4.5 keeps the
    // old numbers because for 4.5 they were correct.
    'claude-opus-5':     { inputPer1K: 0.005,  outputPer1K: 0.025,  iterFactor: 4, provider: 'anthropic' },
    'claude-opus-4-8':   { inputPer1K: 0.005,  outputPer1K: 0.025,  iterFactor: 4, provider: 'anthropic' },
    'claude-opus-4-7':   { inputPer1K: 0.005,  outputPer1K: 0.025,  iterFactor: 4, provider: 'anthropic' },
    'claude-opus-4-6':   { inputPer1K: 0.005,  outputPer1K: 0.025,  iterFactor: 4, provider: 'anthropic' },
    'claude-opus-4-5':   { inputPer1K: 0.015,  outputPer1K: 0.075,  iterFactor: 4, provider: 'anthropic' },
    // Sonnet 5 has an introductory $2/$10 running to 2026-08-31. We quote the
    // standard $3/$15 on purpose — this table's rule is to err HIGH, and a
    // promotional rate that lapses would silently start under-quoting.
    'claude-sonnet-5':   { inputPer1K: 0.003,  outputPer1K: 0.015,  iterFactor: 4, provider: 'anthropic' },
    'claude-sonnet-4-6': { inputPer1K: 0.003,  outputPer1K: 0.015,  iterFactor: 4, provider: 'anthropic' },
    'claude-sonnet-4-5': { inputPer1K: 0.003,  outputPer1K: 0.015,  iterFactor: 4, provider: 'anthropic' },
    // Fable 5 is the only model above the Opus tier: $10/$50 per 1M, double
    // Opus 5. Its thinking is always on and always billed, hence the higher
    // iterFactor — an under-quote here is the most expensive one in the table.
    'claude-fable-5':    { inputPer1K: 0.010,  outputPer1K: 0.050,  iterFactor: 5, provider: 'anthropic' },
    'claude-haiku-4-5':  { inputPer1K: 0.001,  outputPer1K: 0.005,  iterFactor: 3, provider: 'anthropic' },

    // ── Google Gemini ─────────────────────────────────────────────────
    // CORRECTED 2026-07-29 against ai.google.dev/gemini-api/docs/pricing. The
    // previous numbers were "Flash-class is cheap" guesses and ran badly UNDER:
    // gemini-3.5-flash was quoted at $0.30/$2.50 per 1M against a real
    // $1.50/$9.00 — 5× under on input, 3.6× on output — so the footer showed a
    // fraction of the real spend for the default Gemini model.
    //
    // 3.1 Pro is tier-priced by prompt size ($2/$12 at ≤200k, $4/$18 above).
    // We quote the ≤200k tier: Lucy's own compaction keeps prompts under that,
    // and the alternative under-quotes nothing while over-quoting long ones.
    'gemini-3.1-pro-preview':        { inputPer1K: 0.0020,  outputPer1K: 0.012,   iterFactor: 4, provider: 'gemini' },
    'gemini-3.6-flash':              { inputPer1K: 0.0015,  outputPer1K: 0.0075,  iterFactor: 3, provider: 'gemini' },
    'gemini-3.5-flash':              { inputPer1K: 0.0015,  outputPer1K: 0.0090,  iterFactor: 3, provider: 'gemini' },
    'gemini-3.5-flash-lite':         { inputPer1K: 0.00030, outputPer1K: 0.0025,  iterFactor: 3, provider: 'gemini' },
    'gemini-3.1-flash-lite':         { inputPer1K: 0.00025, outputPer1K: 0.0015,  iterFactor: 3, provider: 'gemini' },
    'gemini-3.1-flash-lite-preview': { inputPer1K: 0.00025, outputPer1K: 0.0015,  iterFactor: 3, provider: 'gemini' },
    'gemini-3-flash-preview':        { inputPer1K: 0.0015,  outputPer1K: 0.0090,  iterFactor: 3, provider: 'gemini' }, // legacy → 3.5-flash
    // Legacy Gemini 2.5 (kept for old saved chats)
    'gemini-2.5-pro':                { inputPer1K: 0.00125, outputPer1K: 0.010,   iterFactor: 4, provider: 'gemini' },
    'gemini-2.5-flash':              { inputPer1K: 0.00030, outputPer1K: 0.0025,  iterFactor: 3, provider: 'gemini' },
    'gemini-2.5-flash-lite-preview': { inputPer1K: 0.00010, outputPer1K: 0.0004,  iterFactor: 3, provider: 'gemini' },

    // ── OpenAI ────────────────────────────────────────────────────────
    // Verified 2026-07-29 against developers.openai.com/api/docs/pricing.
    // Standard tier — Batch/Flex halve these, Priority doubles them; Lucy uses
    // neither. The 5.5/5.4/5.3 entries below are legacy and unverifiable now
    // that OpenAI no longer lists them; left at their last-known values so a
    // saved chat still gets a cost estimate rather than the generic fallback.
    'gpt-5.6-sol':     { inputPer1K: 0.005,   outputPer1K: 0.030,  iterFactor: 4, provider: 'openai' },
    'gpt-5.6-terra':   { inputPer1K: 0.0025,  outputPer1K: 0.015,  iterFactor: 4, provider: 'openai' },
    'gpt-5.6-luna':    { inputPer1K: 0.001,   outputPer1K: 0.006,  iterFactor: 3, provider: 'openai' },
    // Legacy — last-known rates, no longer published
    'gpt-5.5':         { inputPer1K: 0.005,   outputPer1K: 0.020,  iterFactor: 4, provider: 'openai' },
    'gpt-5.5-instant': { inputPer1K: 0.002,   outputPer1K: 0.008,  iterFactor: 3, provider: 'openai' },
    'gpt-5.4-mini':    { inputPer1K: 0.00020, outputPer1K: 0.00080, iterFactor: 3, provider: 'openai' },
    'gpt-5.4-nano':    { inputPer1K: 0.00010, outputPer1K: 0.00040, iterFactor: 3, provider: 'openai' },
    'gpt-5.3-codex':   { inputPer1K: 0.005,   outputPer1K: 0.020,  iterFactor: 5, provider: 'openai' },
    // Legacy
    'gpt-4o':          { inputPer1K: 0.0025,  outputPer1K: 0.010,  iterFactor: 4, provider: 'openai' },
    'gpt-4o-mini':     { inputPer1K: 0.00015, outputPer1K: 0.0006, iterFactor: 3, provider: 'openai' },
};

const ZERO_PRICING: ModelPricing = {
    inputPer1K: 0, outputPer1K: 0, iterFactor: 3, provider: 'local',
};
const FALLBACK_PRICING: ModelPricing = {
    inputPer1K: 0.001, outputPer1K: 0.003, iterFactor: 4, provider: 'openai',
};

/**
 * Strip an effort suffix and return the base model id + the multiplier.
 * Returns multiplier = 1.0 when no recognized suffix is present.
 */
function splitEffort(rawModel: string): { base: string; multiplier: number; effort?: string } {
    if (!rawModel) return { base: '', multiplier: 1 };
    const idx = rawModel.indexOf('::');
    if (idx < 0) return { base: rawModel, multiplier: 1 };
    const base = rawModel.slice(0, idx);
    const effort = rawModel.slice(idx + 2).toLowerCase();
    const multiplier = EFFORT_FACTOR[effort];
    if (multiplier == null) return { base, multiplier: 1 }; // unknown suffix — treat as default
    return { base, multiplier, effort };
}

/**
 * Resolve a model id (with optional ::effort) to its pricing.
 *
 *   getPricing("claude-opus-4-7::xhigh")
 *     → { inputPer1K: 0.015, outputPer1K: 0.075, iterFactor: 12, provider: 'anthropic' }
 *
 *   getPricing("gemini-3.5-flash")
 *     → { inputPer1K: 0.00030, outputPer1K: 0.0025, iterFactor: 3, provider: 'gemini' }
 *
 *   getPricing("local-qwen2.5:7b")     → { inputPer1K: 0, outputPer1K: 0, ... }
 *   getPricing("microsoft/phi-3-mini") → fallback NVIDIA price (averaged)
 */
export function getPricing(model: string | null | undefined): ModelPricing & { effort?: string } {
    const m = (model || '').trim();
    if (!m) return FALLBACK_PRICING;

    // Local Ollama models
    if (m.startsWith('local-')) return ZERO_PRICING;

    // NVIDIA NIM models (owner/model-name)
    if (m.includes('/')) {
        return { inputPer1K: 0.0008, outputPer1K: 0.0024, iterFactor: 4, provider: 'nvidia' };
    }

    const { base, multiplier, effort } = splitEffort(m);
    const base_l = base.toLowerCase();
    let pricing = MODEL_PRICING[base_l] || MODEL_PRICING[base];

    // Prefix-based fallback — protects against model rename churn (e.g.
    // claude-3-5-sonnet → claude-sonnet-4-5 → claude-sonnet-4-6). Without this
    // an unknown claude-* falls through to FALLBACK_PRICING which is tagged
    // 'openai', and the cost-predictor reports the wrong provider tier.
    // Picks a conservative mid-tier price for each vendor — exact pricing
    // requires the model to be in MODEL_PRICING above.
    if (!pricing) {
        if (base_l.startsWith('claude-')) {
            // Mid-tier (Sonnet) anthropic pricing — better than 'openai' mislabel
            pricing = { inputPer1K: 0.003, outputPer1K: 0.015, iterFactor: 4, provider: 'anthropic' };
        } else if (base_l.startsWith('gemini-')) {
            // Mid-tier Gemini Flash pricing
            pricing = { inputPer1K: 0.00030, outputPer1K: 0.0025, iterFactor: 3, provider: 'gemini' };
        } else if (base_l.startsWith('gpt-')) {
            // Mid-tier GPT pricing
            pricing = { inputPer1K: 0.005, outputPer1K: 0.020, iterFactor: 4, provider: 'openai' };
        } else {
            pricing = FALLBACK_PRICING;
        }
    }
    return {
        inputPer1K: pricing.inputPer1K,
        outputPer1K: pricing.outputPer1K,
        iterFactor: pricing.iterFactor * multiplier,
        provider: pricing.provider,
        effort,
    };
}

/**
 * Compute the USD cost of a single API call given the model + observed token counts.
 * This is the authoritative function: footer indicator and accumulated cost both
 * use it to stay consistent with each other and with the Rust side.
 */
export function computeCost(model: string, inputTokens: number, outputTokens: number): number {
    const p = getPricing(model);
    const inCost = (inputTokens / 1000) * p.inputPer1K;
    const outCost = (outputTokens / 1000) * p.outputPer1K;
    return inCost + outCost;
}

/**
 * Human-readable price label for the active model — used in the dropdown
 * and the footer hover tooltip. Examples:
 *   "Opus 4.7 (xhigh): $15.00 in · $75.00 out per 1M tokens"
 *   "Flash-Lite: $0.10 in · $0.40 out per 1M tokens · Free local fallback"
 */
export function pricingLabel(model: string): string {
    const p = getPricing(model);
    if (p.inputPer1K === 0 && p.outputPer1K === 0) return 'Free (local)';
    const inUsd = (p.inputPer1K * 1000).toFixed(p.inputPer1K < 0.001 ? 4 : 3);
    const outUsd = (p.outputPer1K * 1000).toFixed(p.outputPer1K < 0.001 ? 4 : 3);
    return `$${inUsd} in · $${outUsd} out / 1M tokens`;
}
