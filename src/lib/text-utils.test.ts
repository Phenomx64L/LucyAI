import { describe, it, expect } from 'vitest';
import { fmtBytes, truncateWithHint } from './text-utils';

describe('fmtBytes', () => {
    it('formats byte counts across unit boundaries', () => {
        expect(fmtBytes(512)).toBe('512 B');
        expect(fmtBytes(1536)).toBe('1.5 KB');
        expect(fmtBytes(5 * 1024 * 1024)).toBe('5.0 MB');
        expect(fmtBytes(3 * 1024 * 1024 * 1024)).toBe('3.00 GB');
    });
    it('returns an em-dash for null/undefined', () => {
        expect(fmtBytes(null)).toBe('—');
        expect(fmtBytes(undefined)).toBe('—');
    });
});

describe('truncateWithHint', () => {
    it('returns text unchanged when within max', () => {
        expect(truncateWithHint('short', 10)).toBe('short');
    });
    it('appends a +N chars hint span when truncated', () => {
        const out = truncateWithHint('abcdefghij', 4);
        expect(out.startsWith('abcd')).toBe(true);
        expect(out).toContain('+6 chars');
        expect(out).toContain('trunc-hint');
    });
    it('handles empty input', () => {
        expect(truncateWithHint('', 5)).toBe('');
    });
});
