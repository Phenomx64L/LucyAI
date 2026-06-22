import { describe, it, expect } from 'vitest';
import { NATIVE_READONLY_HANDLERS } from './agent-tools-native';

// One representative sample per handler kind. The parity tests assert that each
// handler's matchRe detects its tag, its stripRe removes it, and build() yields
// a well-formed { label, fn } task — catching any regex transcription typo from
// the v1.7.213 extraction without needing to run the (Tauri-invoking) fn.
const SAMPLES: Record<string, string> = {
    state_diff:          '<TOOL>state_diff:60</TOOL>',
    process_lineage:     '<TOOL>process_lineage:chrome</TOOL>',
    process_ancestry:    '<TOOL>process_ancestry:1234</TOOL>',
    diagnose_spike:      '<TOOL>diagnose_spike:120</TOOL>',
    threat_scan:         '<TOOL>threat_scan</TOOL>',
    runbook_scan:        '<TOOL>runbook_scan</TOOL>',
    daily_patterns:      '<TOOL>daily_patterns</TOOL>',
    sandbox_preview:     '<TOOL>sandbox_preview:del C:\\x</TOOL>',
    kg_recent:           '<TOOL>kg_recent</TOOL>',
    kg_neighbors:        '<TOOL>kg_neighbors:/a/b.ts</TOOL>',
    kg_ext_summary:      '<TOOL>kg_ext_summary</TOOL>',
    incident_detective:  '<TOOL>incident_detective</TOOL>',
};

describe('agent-tools-native / registry shape', () => {
    it('exports exactly the 12 extracted handlers, each with a unique kind', () => {
        expect(NATIVE_READONLY_HANDLERS).toHaveLength(12);
        const kinds = NATIVE_READONLY_HANDLERS.map(h => h.kind);
        expect(new Set(kinds).size).toBe(12);
        expect(new Set(kinds)).toEqual(new Set(Object.keys(SAMPLES)));
    });
});

describe('agent-tools-native / per-handler parity (match + strip + build)', () => {
    for (const h of NATIVE_READONLY_HANDLERS) {
        it(`${h.kind}: matches its tag, strips it, and builds a task`, () => {
            const sample = SAMPLES[h.kind];
            expect(sample, `no sample for ${h.kind}`).toBeTruthy();

            // 1. matchRe detects the tag inside a larger response.
            const m = (`bla bla ${sample} tail`).match(h.matchRe);
            expect(m, `${h.kind} matchRe did not match`).not.toBeNull();

            // 2. stripRe removes the tag completely (global flag).
            const stripped = `before ${sample} after`.replace(h.stripRe, '');
            expect(stripped.includes('<TOOL>'), `${h.kind} stripRe left a <TOOL> tag`).toBe(false);
            expect(stripped).toBe('before  after');

            // 3. build() yields a { label:string, fn:()=>Promise } task.
            const task = h.build(m as RegExpMatchArray);
            expect(typeof task.label).toBe('string');
            expect(task.label.length).toBeGreaterThan(0);
            expect(typeof task.fn).toBe('function');
        });
    }

    it('optional-arg handlers also match WITHOUT an argument', () => {
        // diagnose_spike / threat_scan / runbook_scan / daily_patterns / kg_recent
        // / kg_ext_summary / incident_detective accept a bare <TOOL>name</TOOL>.
        const optional = ['diagnose_spike', 'threat_scan', 'runbook_scan', 'daily_patterns', 'kg_recent', 'kg_ext_summary', 'incident_detective'];
        for (const kind of optional) {
            const h = NATIVE_READONLY_HANDLERS.find(x => x.kind === kind)!;
            expect(`<TOOL>${kind}</TOOL>`.match(h.matchRe), `${kind} should match bare form`).not.toBeNull();
        }
    });
});
