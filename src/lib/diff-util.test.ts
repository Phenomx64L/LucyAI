import { describe, it, expect } from 'vitest';
import { diffLines } from './diff-util';

describe('diffLines', () => {
    it('returns [] for identical input', () => {
        expect(diffLines('a\nb\nc', 'a\nb\nc')).toEqual([]);
        expect(diffLines('', '')).toEqual([]);
    });

    it('marks a changed middle line as rem then add, with context', () => {
        const d = diffLines('a\nb\nc', 'a\nB\nc');
        // 1 line context above (a), the change (b→B), 1 line below (c)
        expect(d).toEqual([
            { type: 'eq', text: 'a', n: 1 },
            { type: 'rem', text: 'b' },
            { type: 'add', text: 'B' },
            { type: 'eq', text: 'c', n: 3 },
        ]);
    });

    it('handles pure additions (empty → content)', () => {
        const d = diffLines('', 'line1\nline2');
        expect(d.filter(x => x.type === 'rem').map(x => x.text)).toEqual(['']);
        expect(d.filter(x => x.type === 'add').map(x => x.text)).toEqual(['line1', 'line2']);
    });

    it('handles appends (keeps prefix as context, adds the tail)', () => {
        const d = diffLines('a\nb', 'a\nb\nc');
        expect(d.some(x => x.type === 'add' && x.text === 'c')).toBe(true);
        // no removals when purely appending
        expect(d.filter(x => x.type === 'rem')).toEqual([]);
    });

    it('handles removals', () => {
        const d = diffLines('a\nb\nc', 'a\nc');
        expect(d.some(x => x.type === 'rem' && x.text === 'b')).toBe(true);
    });

    it('tolerates null/undefined', () => {
        expect(() => diffLines(null, undefined)).not.toThrow();
        expect(diffLines(null, undefined)).toEqual([]);
        expect(diffLines(null, 'x').some(d => d.type === 'add' && d.text === 'x')).toBe(true);
    });
});
