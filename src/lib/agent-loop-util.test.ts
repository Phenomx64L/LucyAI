import { describe, it, expect } from 'vitest';
import { hashResp } from './agent-loop-util';

// Reference implementation = the original inline closure from runAI. These
// assertions pin the EXACT djb2 output so the skip-stuck comparison can never
// silently drift.
function reference(s: string): number {
    let h = 5381;
    const str = String(s || '').trim();
    for (let i = 0; i < str.length; i++) h = ((h << 5) + h + str.charCodeAt(i)) | 0;
    return h >>> 0;
}

describe('hashResp', () => {
    it('returns an unsigned 32-bit int', () => {
        const h = hashResp('hello world');
        expect(Number.isInteger(h)).toBe(true);
        expect(h).toBeGreaterThanOrEqual(0);
        expect(h).toBeLessThanOrEqual(0xffffffff);
    });

    it('matches the original djb2 implementation exactly', () => {
        for (const s of ['', 'a', 'hello', '<THOUGHT>plan</THOUGHT>', 'voy a editar el archivo', 'áéíóú ñ 🚀']) {
            expect(hashResp(s)).toBe(reference(s));
        }
    });

    it('ignores leading/trailing whitespace (trim)', () => {
        expect(hashResp('  same  ')).toBe(hashResp('same'));
    });

    it('is stable and collision-free for distinct short inputs', () => {
        expect(hashResp('abc')).toBe(hashResp('abc'));
        expect(hashResp('abc')).not.toBe(hashResp('abd'));
    });

    it('handles null/undefined without throwing', () => {
        expect(() => hashResp(null)).not.toThrow();
        expect(hashResp(null)).toBe(hashResp(''));
        expect(hashResp(undefined)).toBe(hashResp(''));
    });
});
