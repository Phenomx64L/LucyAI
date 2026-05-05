import { describe, it, expect } from 'vitest';
import {
    estimateTokens,
    predictCost,
    compareModels,
    formatEstimate,
} from './cost-predictor';

describe('cost-predictor / estimateTokens', () => {
    it('returns 0 for empty input', () => {
        expect(estimateTokens('')).toBe(0);
    });

    it('scales roughly 1 token per 4 prose chars', () => {
        const text = 'a'.repeat(400);
        const t = estimateTokens(text);
        // Range allows the code-heuristic to deviate.
        expect(t).toBeGreaterThan(80);
        expect(t).toBeLessThan(150);
    });

    it('estimates differently when code fences appear', () => {
        const prose = 'word '.repeat(200);                 // 1000 chars
        const code  = '```js\n' + 'x;\n'.repeat(330) + '```';  // similar size, code
        expect(estimateTokens(prose)).toBeGreaterThan(0);
        expect(estimateTokens(code)).toBeGreaterThan(0);
    });
});

describe('cost-predictor / predictCost', () => {
    it('returns free for local- prefixed models', () => {
        const r = predictCost('hello', 0, 'local-llama');
        expect(r.usd).toBe(0);
        expect(r.provider).toBe('local');
        expect(r.label).toMatch(/free/i);
    });

    it('returns gemini-tier prices for unprefixed model ids', () => {
        const r = predictCost('hello world test prompt', 0, 'gemini-2.5-flash');
        expect(r.provider).toBe('gemini');
        expect(r.usd).toBeGreaterThanOrEqual(0);
    });

    it('returns anthropic-tier for claude-* models', () => {
        const r = predictCost('hello world', 0, 'claude-3-5-sonnet-latest');
        expect(r.provider).toBe('anthropic');
    });

    it('reports low confidence with empty history', () => {
        const r = predictCost('hello', 0, 'gemini-2.5-flash', []);
        expect(r.confidence).toBe('low');
    });

    it('reports high confidence with ≥10 history samples', () => {
        const history = Array.from({ length: 12 }, () => ({ tokens_in: 100, tokens_out: 200 }));
        const r = predictCost('hello', 0, 'gemini-2.5-flash', history);
        expect(r.confidence).toBe('high');
    });

    it('has monotonic input scaling', () => {
        const a = predictCost('a'.repeat(200), 0, 'gemini-2.5-flash');
        const b = predictCost('a'.repeat(2000), 0, 'gemini-2.5-flash');
        expect(b.inputTokens).toBeGreaterThan(a.inputTokens);
        expect(b.usd).toBeGreaterThan(a.usd);
    });
});

describe('cost-predictor / compareModels', () => {
    it('sorts by USD ascending, cheapest first', () => {
        const list = compareModels('hello world', 0, [
            'claude-3-5-sonnet-latest',
            'gemini-2.5-flash',
            'local-llama',
        ]);
        expect(list[0].usd).toBeLessThanOrEqual(list[list.length - 1].usd);
    });
});

describe('cost-predictor / formatEstimate', () => {
    it('produces a human readable label', () => {
        const r = predictCost('hello', 0, 'gemini-2.5-flash');
        const label = formatEstimate(r);
        expect(label).toMatch(/in/);
        expect(label).toMatch(/out/);
    });
});
