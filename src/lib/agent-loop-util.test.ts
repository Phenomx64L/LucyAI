import { describe, it, expect } from 'vitest';
import { hashResp, normalizeAgentResp, pickStrongerInFamily } from './agent-loop-util';

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

describe('normalizeAgentResp (near-identical grind detection)', () => {
    const H = (s: string) => hashResp(normalizeAgentResp(s));

    it('collapses a reworded <THOUGHT> around the SAME action to one signature', () => {
        const a = `<THOUGHT>Let me read the config file to check the port.</THOUGHT>\n<TOOL>readfile:C:\\app\\config.toml</TOOL>`;
        const b = `<THOUGHT>I'll open the config now — need to verify the listening port setting.</THOUGHT>\n<TOOL>readfile:C:\\app\\config.toml</TOOL>`;
        // Byte-identical detector would MISS this (different THOUGHT text)…
        expect(hashResp(a)).not.toBe(hashResp(b));
        // …but the normalized detector treats them as the same grinding turn.
        expect(H(a)).toBe(H(b));
    });

    it('ignores casing and whitespace/line-wrap churn', () => {
        expect(H('<TOOL>listdir:/etc</TOOL>')).toBe(H('  <tool>LISTDIR:/etc</tool>  '));
        expect(H('a\n\n  b   c')).toBe(H('A B C'));
    });

    it('does NOT collapse turns acting on DIFFERENT targets (no false grind)', () => {
        // Paging / multi-file investigation is real progress — must stay distinct.
        expect(H('<TOOL>readfile:A.txt</TOOL>')).not.toBe(H('<TOOL>readfile:B.txt</TOOL>'));
        expect(H('<TOOL>readlines:big.log|0|100</TOOL>')).not.toBe(H('<TOOL>readlines:big.log|100|100</TOOL>'));
    });

    it('keeps distinct prose distinct (only cosmetic churn collapses)', () => {
        expect(H('Ethernet is down; restarting the adapter.')).not.toBe(H('DNS is misconfigured; flushing the cache.'));
    });

    it('drops an unclosed trailing THOUGHT (truncated turn)', () => {
        const closed = `<TOOL>sysinfo</TOOL><THOUGHT>done</THOUGHT>`;
        const unclosed = `<TOOL>sysinfo</TOOL><THOUGHT>done and then some more musing that never closed`;
        expect(H(closed)).toBe(H(unclosed));
    });

    it('handles null/undefined without throwing', () => {
        expect(() => normalizeAgentResp(null)).not.toThrow();
        expect(normalizeAgentResp(null)).toBe('');
        expect(normalizeAgentResp(undefined)).toBe('');
    });
});

describe('pickStrongerInFamily (self-heal escalation)', () => {
    // Mirrors the real LLM_GROUPS shape (provider + options[{id,icon}]).
    const GROUPS = [
        { provider: 'google', options: [
            { id: 'gemini-3.1-pro-preview::high',   icon: '◆' },
            { id: 'gemini-3.1-pro-preview::medium', icon: '◆' },
            { id: 'gemini-3.5-flash',               icon: '◐' },
            { id: 'gemini-3.1-flash-lite',          icon: '◯' },
        ] },
        { provider: 'anthropic', options: [
            { id: 'claude-opus-4-8::max', icon: '◆' },
            { id: 'claude-haiku-4-5',     icon: '▸' },
        ] },
        { provider: 'local', options: [
            { id: 'local-qwen2.5-coder:7b', icon: '⌬' },
        ] },
    ];

    it('escalates Gemini Flash to the BALANCED Pro flagship (not the ::high tier)', () => {
        expect(pickStrongerInFamily('gemini-3.5-flash', GROUPS)).toBe('gemini-3.1-pro-preview::medium');
    });

    it('escalates within the same provider family only', () => {
        // Haiku → the Anthropic flagship (same API key), never a cross-provider jump.
        expect(pickStrongerInFamily('claude-haiku-4-5', GROUPS)).toBe('claude-opus-4-8::max');
    });

    it('returns null when the model is ALREADY the flagship', () => {
        expect(pickStrongerInFamily('gemini-3.1-pro-preview::high', GROUPS)).toBeNull();
        expect(pickStrongerInFamily('gemini-3.1-pro-preview::medium', GROUPS)).toBeNull();
        expect(pickStrongerInFamily('claude-opus-4-8::max', GROUPS)).toBeNull();
    });

    it('never escalates a local model (no cloud key implied)', () => {
        expect(pickStrongerInFamily('local-qwen2.5-coder:7b', GROUPS)).toBeNull();
    });

    it('returns null for unknown / empty models and empty catalog', () => {
        expect(pickStrongerInFamily('some-unlisted-model', GROUPS)).toBeNull();
        expect(pickStrongerInFamily('', GROUPS)).toBeNull();
        expect(pickStrongerInFamily(null, GROUPS)).toBeNull();
        expect(pickStrongerInFamily('gemini-3.5-flash', [])).toBeNull();
    });
});
