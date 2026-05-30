import { describe, it, expect } from 'vitest';
import { fuzzyScore, fuzzyFilter } from './fuzzy-match';

describe('fuzzyScore', () => {
    it('empty query returns score 0', () => {
        const r = fuzzyScore('', 'anything');
        expect(r.score).toBe(0);
        expect(r.indices).toEqual([]);
    });

    it('exact prefix scores high', () => {
        const a = fuzzyScore('data', 'Database Schema').score;
        const b = fuzzyScore('data', 'Update Documentation').score;
        expect(a).toBeGreaterThan(b);
    });

    it('word-boundary match beats mid-word', () => {
        // 'sc' against 'Schema Tool' → boundary match on S
        const a = fuzzyScore('sc', 'Schema Tool').score;
        // 'sc' against 'Discount' → mid-word, no boundary
        const b = fuzzyScore('sc', 'Discount').score;
        expect(a).toBeGreaterThan(b);
    });

    it('contiguous substring scores highest', () => {
        const a = fuzzyScore('mcp', 'mcp-server registry').score;
        const b = fuzzyScore('mcp', 'mark cherry pop').score; // scattered m,c,p
        expect(a).toBeGreaterThan(b);
    });

    it('rejects when query chars not all present in order', () => {
        const r = fuzzyScore('abc', 'cba');
        expect(r.score).toBe(-Infinity);
        expect(r.indices).toEqual([]);
    });

    it('returns indices of matched chars', () => {
        const r = fuzzyScore('db', 'database');
        // d at 0, b at 4
        expect(r.indices[0]).toBe(0);
        expect(r.indices[1]).toBeGreaterThan(0);
    });

    it('smart-case respects uppercase in query', () => {
        // 'Sc' should match 'Schema' but not 'discount'
        const r1 = fuzzyScore('Sc', 'Schema');
        const r2 = fuzzyScore('Sc', 'discount');
        expect(r1.score).toBeGreaterThan(0);
        expect(r2.score).toBe(-Infinity);
    });

    it('lowercase query is case-insensitive', () => {
        const r1 = fuzzyScore('sc', 'Schema');
        const r2 = fuzzyScore('sc', 'schema');
        expect(r1.score).toBeGreaterThan(0);
        expect(r2.score).toBeGreaterThan(0);
    });

    it('CamelCase boundary detected', () => {
        // 'rb' against 'RunBook' matches R at 0 (start) AND B at 3 via
        // CamelCase boundary. Should beat 'rb' against a scattered
        // candidate without contiguous substring.
        const r = fuzzyScore('rb', 'RunBook');
        expect(r.score).toBeGreaterThan(0);
        const scattered = fuzzyScore('rb', 'random sub').score;
        expect(r.score).toBeGreaterThan(scattered);
    });

    it('rewards consecutive matches', () => {
        const tight   = fuzzyScore('mcp', 'mcp servers').score;
        const sparse  = fuzzyScore('mcp', 'mxcxp').score;
        expect(tight).toBeGreaterThan(sparse);
    });
});

describe('fuzzyFilter', () => {
    it('returns all items when query is empty', () => {
        const items = [{ label: 'a' }, { label: 'b' }];
        const out = fuzzyFilter(items, '', (i) => i.label);
        expect(out).toEqual(items);
    });

    it('sorts by score desc', () => {
        const items = [
            { label: 'Update Documentation' },
            { label: 'Database Schema' },
            { label: 'Display' },
        ];
        const out = fuzzyFilter(items, 'data', (i) => i.label);
        // Database starts with 'data' → should win
        expect(out[0].label).toBe('Database Schema');
    });

    it('drops non-matching items', () => {
        const items = [
            { label: 'something' },
            { label: 'irrelevant' },
            { label: 'database' },
        ];
        const out = fuzzyFilter(items, 'xyz', (i) => i.label);
        expect(out).toHaveLength(0);
    });

    it('stable within ties', () => {
        const items = [
            { id: 1, label: 'abcd' },
            { id: 2, label: 'abcd' },
        ];
        const out = fuzzyFilter(items, 'ab', (i) => i.label);
        expect(out[0].id).toBe(1);
        expect(out[1].id).toBe(2);
    });
});
