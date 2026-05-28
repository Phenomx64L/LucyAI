// ── model-pricing.test.ts ─────────────────────────────────────────────────
//
// Regression guard for the prefix-based fallback added in Sprint 4 (BUG-3).
//
// Background: before the fix, any model id NOT in MODEL_PRICING fell through
// to FALLBACK_PRICING which is tagged provider:'openai'. That meant a future
// rename like claude-sonnet-4-7 (not yet in the table) would silently report
// the wrong provider tier in cost-predictor, the StatusBar rate pill, etc.
//
// The fix routes unknown `claude-*`, `gpt-*`, `gemini-*` ids to the correct
// vendor with a conservative mid-tier price. These tests pin the contract.

import { describe, it, expect } from 'vitest';
import { getPricing, computeCost } from './model-pricing';

describe('getPricing / known models', () => {
    it('returns anthropic tier for explicit claude-opus-4-7', () => {
        const p = getPricing('claude-opus-4-7');
        expect(p.provider).toBe('anthropic');
        expect(p.inputPer1K).toBe(0.015);
    });

    it('returns gemini tier for explicit gemini-3-pro', () => {
        // gemini-3-pro exists in MODEL_PRICING per the table
        const p = getPricing('gemini-3-pro');
        expect(p.provider).toBe('gemini');
    });

    it('returns local tier for any local- prefix', () => {
        const p = getPricing('local-qwen2.5:7b');
        expect(p.provider).toBe('local');
        expect(p.inputPer1K).toBe(0);
        expect(p.outputPer1K).toBe(0);
    });

    it('returns nvidia tier for owner/model NIM ids', () => {
        const p = getPricing('microsoft/phi-3-mini');
        expect(p.provider).toBe('nvidia');
    });
});

describe('getPricing / prefix-based fallback (Sprint 4 BUG-3)', () => {
    it('returns anthropic provider for UNREGISTERED claude-* model', () => {
        // This was the original failing case — claude-3-5-sonnet-latest is
        // not in MODEL_PRICING and used to fall through to FALLBACK_PRICING
        // (provider:'openai'). The prefix fallback now catches it.
        const p = getPricing('claude-3-5-sonnet-latest');
        expect(p.provider).toBe('anthropic');
        expect(p.inputPer1K).toBeGreaterThan(0);
        expect(p.outputPer1K).toBeGreaterThan(p.inputPer1K); // sanity: output > input
    });

    it('returns anthropic for a hypothetical future claude rename', () => {
        // Models get renamed every cycle (3-5 → 4-5 → 4-6 → 4-7). The test
        // protects against the next rename: ANY claude-* should be anthropic.
        const p = getPricing('claude-opus-5-0-future');
        expect(p.provider).toBe('anthropic');
    });

    it('returns gemini for UNREGISTERED gemini-* model', () => {
        const p = getPricing('gemini-99-experimental');
        expect(p.provider).toBe('gemini');
    });

    it('returns openai for UNREGISTERED gpt-* model', () => {
        const p = getPricing('gpt-99-future');
        expect(p.provider).toBe('openai');
    });

    it('falls back to FALLBACK_PRICING for truly unknown vendor', () => {
        // No vendor prefix → conservative fallback (provider:'openai' is the
        // legacy fallback label, intentional, not the same as gpt-* routing).
        const p = getPricing('unknown-vendor-x-1');
        expect(p.provider).toBe('openai');
    });

    it('handles null/undefined/empty input without throwing', () => {
        expect(() => getPricing(null)).not.toThrow();
        expect(() => getPricing(undefined)).not.toThrow();
        expect(() => getPricing('')).not.toThrow();
        // Sanity: degraded but defined
        expect(getPricing('').inputPer1K).toBeGreaterThanOrEqual(0);
    });
});

describe('getPricing / effort suffix', () => {
    it('strips the ::effort suffix before lookup', () => {
        const base = getPricing('claude-opus-4-7');
        const withEffort = getPricing('claude-opus-4-7::high');
        expect(base.inputPer1K).toBe(withEffort.inputPer1K);
        expect(base.outputPer1K).toBe(withEffort.outputPer1K);
        expect(withEffort.effort).toBe('high');
    });

    it('applies effort multiplier to iterFactor only', () => {
        const base = getPricing('claude-opus-4-7');
        const xhigh = getPricing('claude-opus-4-7::xhigh');
        // iterFactor scales with effort, prices do not
        expect(xhigh.iterFactor).toBeGreaterThan(base.iterFactor);
        expect(xhigh.inputPer1K).toBe(base.inputPer1K);
    });

    it('also strips effort for the prefix fallback path', () => {
        // Unregistered model + effort suffix — both behaviours combine
        const p = getPricing('claude-9-9-future::medium');
        expect(p.provider).toBe('anthropic');
        expect(p.effort).toBe('medium');
    });
});

describe('computeCost', () => {
    it('returns zero for local models regardless of token count', () => {
        expect(computeCost('local-anything', 10_000, 10_000)).toBe(0);
    });

    it('scales linearly with token counts', () => {
        const a = computeCost('claude-opus-4-7', 1000, 1000);
        const b = computeCost('claude-opus-4-7', 2000, 2000);
        // Allow tiny floating-point slack
        expect(b).toBeCloseTo(a * 2, 6);
    });

    it('output tokens are more expensive than input for cloud models', () => {
        const inputOnly  = computeCost('claude-opus-4-7', 1000, 0);
        const outputOnly = computeCost('claude-opus-4-7', 0, 1000);
        expect(outputOnly).toBeGreaterThan(inputOnly);
    });
});
