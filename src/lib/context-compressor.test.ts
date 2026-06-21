import { describe, it, expect } from 'vitest';
import {
    dedupToolResults,
    smartCollapseOldResults,
    shouldCompact,
    recordCompactionRatio,
    compressToolResults,
    localDedupAgentContext,
    type ToolResult,
} from './context-compressor';

const longA = '[readfile] /etc/long-config-a\n' + 'line\n'.repeat(80);
const longB = '[searchfiles] pattern="x"\n' + 'match\n'.repeat(80);

describe('context-compressor / dedupToolResults', () => {
    it('replaces older duplicate, keeps newest', () => {
        const r: ToolResult[] = [
            { kind: 'readfile', text: longA },
            { kind: 'readfile', text: longB },
            { kind: 'readfile', text: longA },          // duplicate of [0]
        ];
        const out = dedupToolResults(r);
        expect(out[0].text.startsWith('[Duplicate')).toBe(true);
        expect(out[1].text).toBe(longB);
        expect(out[2].text).toBe(longA);                // newest survives
    });

    it('skips dedup for outputs shorter than the threshold', () => {
        const r: ToolResult[] = [
            { kind: 'readfile', text: 'tiny' },
            { kind: 'readfile', text: 'tiny' },
        ];
        const out = dedupToolResults(r);
        expect(out[0].text).toBe('tiny');
        expect(out[1].text).toBe('tiny');
    });

    it('handles empty input', () => {
        expect(dedupToolResults([])).toEqual([]);
    });
});

describe('context-compressor / smartCollapseOldResults', () => {
    it('keeps the last N verbatim', () => {
        const r: ToolResult[] = Array.from({ length: 5 }, (_, i) => ({
            kind: 'readfile', text: 'X'.repeat(5000) + `_${i}`,
        }));
        const out = smartCollapseOldResults(r, 3);
        // First 2 collapsed, last 3 verbatim.
        expect(out[0].text).toMatch(/collapsed/);
        expect(out[1].text).toMatch(/collapsed/);
        expect(out[2].text.endsWith('_2')).toBe(true);
        expect(out[3].text.endsWith('_3')).toBe(true);
        expect(out[4].text.endsWith('_4')).toBe(true);
    });

    it('produces tool-aware summaries for known kinds', () => {
        // Body must be ≥ COLLAPSE_BLOCK_LEN/4 (= 2000 chars) for the
        // collapse rule to trigger — anything shorter is already cheap
        // enough to keep verbatim.
        const fat = 'line of content here\n'.repeat(200);   // ~4 KB
        const r: ToolResult[] = [
            { kind: 'readfile',  args: '/x', text: fat },
            { kind: 'readfile',  args: '/y', text: fat },
            { kind: 'readfile',  args: '/z', text: 'now' },
            { kind: 'readfile',  args: '/w', text: 'now2' },
        ];
        const out = smartCollapseOldResults(r, 1);
        expect(out[0].text).toMatch(/\[readfile\]/);
        expect(out[0].text).toMatch(/\d+ chars, \d+ lines/);
    });

    it('does not collapse when length under threshold', () => {
        const r: ToolResult[] = Array.from({ length: 5 }, (_, i) => ({
            kind: 'readfile', text: 'short',
        }));
        const out = smartCollapseOldResults(r, 3);
        // Short content stays verbatim even on the older slice.
        expect(out.every(o => o.text === 'short')).toBe(true);
    });
});

describe('context-compressor / shouldCompact (anti-thrashing)', () => {
    it('returns true on empty/missing memory', () => {
        expect(shouldCompact(null)).toBe(true);
        expect(shouldCompact({})).toBe(true);
        expect(shouldCompact({ _compRatios: [] })).toBe(true);
    });

    it('returns true while history is short', () => {
        expect(shouldCompact({ _compRatios: [0.05] })).toBe(true);
    });

    it('returns false when last 2 ratios are both <10%', () => {
        expect(shouldCompact({ _compRatios: [0.05, 0.03] })).toBe(false);
    });

    it('returns true if at least one of last 2 is meaningful', () => {
        expect(shouldCompact({ _compRatios: [0.05, 0.20] })).toBe(true);
        expect(shouldCompact({ _compRatios: [0.20, 0.05] })).toBe(true);
    });
});

describe('context-compressor / recordCompactionRatio', () => {
    it('appends and clamps history to last 4', () => {
        const wm: any = {};
        for (let i = 0; i < 6; i++) recordCompactionRatio(wm, 1000, 700);
        expect(wm._compRatios.length).toBe(4);
        expect(wm._compRatios.every((r: number) => Math.abs(r - 0.3) < 0.01)).toBe(true);
    });

    it('handles before=0 gracefully', () => {
        const wm: any = {};
        recordCompactionRatio(wm, 0, 0);
        expect(wm._compRatios).toBeUndefined();
    });
});

describe('context-compressor / compressToolResults end-to-end', () => {
    it('reports realistic before/after/ratio', () => {
        const r: ToolResult[] = [
            { kind: 'readfile', text: longA },
            { kind: 'readfile', text: longA },          // duplicate
            { kind: 'readfile', text: longB },          // recent, kept verbatim
        ];
        const out = compressToolResults(r);
        expect(out.before).toBeGreaterThan(0);
        expect(out.after).toBeLessThanOrEqual(out.before);
        expect(out.ratio).toBeGreaterThanOrEqual(0);
        expect(out.ratio).toBeLessThanOrEqual(1);
    });
});

describe('context-compressor / localDedupAgentContext (raw-string Phase 1)', () => {
    it('returns input unchanged below the size/loop gate', () => {
        expect(localDedupAgentContext('small ctx', 5)).toBe('small ctx');
        const big = 'x'.repeat(9000);
        expect(localDedupAgentContext(big, 1)).toBe(big); // loop_i < 2 → off
    });

    it('collapses duplicate >=200-char lines (keeps the first)', () => {
        const dup = 'A'.repeat(250);
        const ctx = `${dup}\n${dup}\n` + 'z'.repeat(8000);
        const out = localDedupAgentContext(ctx, 2); // gate on; keepRecent=1 → no step-trim
        expect(out).toContain('[... duplicate block omitted]');
        expect(out.split(dup).length - 1).toBe(1);   // only one full copy remains
    });

    it('trims old TOOL RESULTS step blocks beyond 600 chars', () => {
        const body = 'B'.repeat(800);
        const ctx =
            `--- TOOL RESULTS (step 1) ---\n${body}\n` +
            `--- TOOL RESULTS (step 2) ---\nshort recent body\n` +
            'p'.repeat(8000);
        const out = localDedupAgentContext(ctx, 4); // keepRecent=2 → trims step 1
        expect(out).toContain('--- TOOL RESULTS (step 1, trimmed) ---');
        expect(out).toMatch(/\d+ chars omitted/);
        expect(out).not.toContain(body);            // full 800-char body gone
    });

    it('v1.7.204 — truncates oversized [EXECUTION RESULT] bodies (was a dead no-op)', () => {
        const ctx =
            '[EXECUTION RESULT]\n' + 'x'.repeat(5000) +
            '\n\n--- TOOL RESULTS (step 9) ---\n' + 'p'.repeat(8000);
        const out = localDedupAgentContext(ctx, 3);
        expect(out).toContain('[... output truncated for context compression]');
        expect(out).not.toContain('x'.repeat(5000)); // full 5000-char body gone (capped to 4000)
    });

    it('leaves a short [EXECUTION RESULT] body untouched (< 4000 chars to end/next block)', () => {
        // Padding lives BEFORE the result so the body has < 4000 chars to the
        // string end → the {4000,}? minimum can't be met → no truncation.
        const ctx =
            'q'.repeat(8000) + '\n\n--- TOOL RESULTS (step 1) ---\n' +
            '[EXECUTION RESULT]\n' + 'y'.repeat(120) + '\n\n--- end ---';
        const out = localDedupAgentContext(ctx, 3);
        expect(out).toContain('y'.repeat(120));
        expect(out).not.toContain('output truncated for context compression');
    });

    it('does not throw on empty input', () => {
        expect(localDedupAgentContext('', 5)).toBe('');
    });
});
