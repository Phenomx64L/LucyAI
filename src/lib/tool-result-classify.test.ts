import { describe, it, expect } from 'vitest';
import { classifyToolResults, stripFraming } from './tool-result-classify';

describe('stripFraming', () => {
    it('removes bracket headers, rule lines and PS prompts', () => {
        const s = '[EXECUTION RESULT — step 2]\n--- TOOL RESULTS ---\nPS > Get-Service\nactual body line';
        expect(stripFraming(s)).toBe('actual body line');
    });
    it('returns empty string for null/blank', () => {
        expect(stripFraming(null)).toBe('');
        expect(stripFraming('   ')).toBe('');
    });
});

describe('classifyToolResults', () => {
    it('counts blank / very short results as empty', () => {
        const r = classifyToolResults(['', '   ', 'tiny']);
        expect(r.totalToolCalls).toBe(3);
        expect(r.emptyCount).toBe(3);
        expect(r.errorCount).toBe(0);
    });

    it('counts results that are only framing as empty', () => {
        const r = classifyToolResults(['[EXECUTION RESULT — step 1]\nPS > whoami\n']);
        expect(r.emptyCount).toBe(1);
    });

    it('matches empty markers even inside verbose wrappers', () => {
        const r = classifyToolResults(['[RESULT] the command returned no output for this query okay']);
        expect(r.emptyCount).toBe(1);
    });

    it('counts ERROR-marked results as errors', () => {
        const r = classifyToolResults([
            '[POWERSHELL ERROR] CommandNotFoundException: Get-Foo is not recognized as a cmdlet',
        ]);
        expect(r.errorCount).toBe(1);
        expect(r.emptyCount).toBe(0);
    });

    it('tallies PowerShell parse errors as a subset of errors', () => {
        const r = classifyToolResults([
            '[POWERSHELL ERROR] ParserError: Unexpected token in expression or statement somewhere',
        ]);
        expect(r.errorCount).toBe(1);
        expect(r.psParseErrorCount).toBe(1);
    });

    it('counts a substantial good result as neither empty nor error', () => {
        const r = classifyToolResults([
            'Status   Name      DisplayName\nRunning  Spooler   Print Spooler service is active here',
        ]);
        expect(r.emptyCount).toBe(0);
        expect(r.errorCount).toBe(0);
    });

    it('handles empty / non-array input', () => {
        expect(classifyToolResults([])).toEqual({ totalToolCalls: 0, emptyCount: 0, errorCount: 0, psParseErrorCount: 0 });
        // @ts-expect-error — defensive: non-array
        expect(classifyToolResults(null).totalToolCalls).toBe(0);
    });
});
