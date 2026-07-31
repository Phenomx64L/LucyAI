import { describe, it, expect } from 'vitest';
import {
    NATIVE_READONLY_HANDLERS,
    NATIVE_READONLY_HANDLERS_DEPS,
    type NativeHandlerDeps,
} from './agent-tools-native';

// One representative sample per handler kind. The parity tests assert that each
// handler's matchRe detects its tag, its stripRe removes it, and build() yields
// a well-formed { label, fn } task — catching any regex transcription typo from
// the v1.7.213 / v1.7.214 extraction without running the (Tauri-invoking) fn.
const SAMPLES: Record<string, string> = {
    // pure (v1.7.213 + healing_find/semantic from 2b)
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
    healing_find:        '<TOOL>healing_find:winrm cert error</TOOL>',
    semantic:            '<TOOL>semantic:dns failures</TOOL>',
};

// Closure-coupled (v1.7.214 Batch 2b + v1.7.215 Batch 3 read-only file tools).
const DEPS_SAMPLES: Record<string, string> = {
    system_diff:      '<TOOL>system_diff:services</TOOL>',
    obj_query:        '<TOOL>obj_query:$_.Name -eq "x"</TOOL>',
    mcp_discover:     '<TOOL>mcp_discover:filesystem</TOOL>',
    fetch:            '<TOOL>fetch:https://example.com</TOOL>',
    search_web:       '<TOOL>search_web:powershell winrm</TOOL>',
    // v1.8 — reads a URL INTO memory, unlike `fetch` which reads it into the
    // current turn and forgets it.
    web_ingest:       '<TOOL>web_ingest:https://learn.microsoft.com/winrm</TOOL>',
    search_runbooks:  '<TOOL>search_runbooks:dns reset</TOOL>',
    searchfiles:      '<TOOL>searchfiles:C:\\proj|||*.rs</TOOL>',
    locate_file:      '<TOOL>locate_file:msedge.exe</TOOL>',
    start_indexer:    '<TOOL>start_indexer:C:\\proj</TOOL>',
    // Batch 4 (v1.7.216): remaining read-only file-read + basic native tools.
    readlines:        '<TOOL>readlines:/a/b.ts:10:20</TOOL>',
    listdir:          '<TOOL>listdir:C:\\proj</TOOL>',
    analyze_code:     '<TOOL>analyze_code:/a/b.rs</TOOL>',
    sysinfo:          '<TOOL>sysinfo</TOOL>',
    netconn:          '<TOOL>netconn</TOOL>',
    tasklist:         '<TOOL>tasklist</TOOL>',
    eventlog:         '<TOOL>eventlog:System:100</TOOL>',
    registry:         '<TOOL>registry:HKLM|Software\\X|Value</TOOL>',
};

const mockDeps: NativeHandlerDeps = {
    retryWithBackoff: (fn) => fn(),
    cachedFetch: (_c, _q, producer) => producer(),
    mcpServers: [],
    mcpSecrets: {},
    loadMcpServers: async () => {},
    runbooksDir: '',
    tabId: 'test-tab',
};

describe('agent-tools-native / pure registry shape', () => {
    it('exports the 14 pure handlers, each with a unique kind', () => {
        expect(NATIVE_READONLY_HANDLERS).toHaveLength(14);
        const kinds = NATIVE_READONLY_HANDLERS.map(h => h.kind);
        expect(new Set(kinds).size).toBe(14);
        expect(new Set(kinds)).toEqual(new Set(Object.keys(SAMPLES)));
    });
});

describe('agent-tools-native / deps registry shape', () => {
    it('exports the 18 closure-coupled handlers, each with a unique kind', () => {
        expect(NATIVE_READONLY_HANDLERS_DEPS).toHaveLength(18);
        const kinds = NATIVE_READONLY_HANDLERS_DEPS.map(h => h.kind);
        expect(new Set(kinds).size).toBe(18);
        expect(new Set(kinds)).toEqual(new Set(Object.keys(DEPS_SAMPLES)));
    });
    it('pure and deps kinds do not overlap', () => {
        const pure = new Set(NATIVE_READONLY_HANDLERS.map(h => h.kind));
        for (const h of NATIVE_READONLY_HANDLERS_DEPS) {
            expect(pure.has(h.kind), `${h.kind} is in both registries`).toBe(false);
        }
    });
});

describe('agent-tools-native / pure per-handler parity', () => {
    for (const h of NATIVE_READONLY_HANDLERS) {
        it(`${h.kind}: matches, strips, builds`, () => {
            const sample = SAMPLES[h.kind];
            expect(sample, `no sample for ${h.kind}`).toBeTruthy();
            const m = (`bla ${sample} tail`).match(h.matchRe);
            expect(m, `${h.kind} matchRe failed`).not.toBeNull();
            const stripped = `before ${sample} after`.replace(h.stripRe, '');
            expect(stripped.includes('<TOOL>'), `${h.kind} stripRe left a tag`).toBe(false);
            const task = h.build(m as RegExpMatchArray);
            expect(typeof task.label).toBe('string');
            expect(task.label.length).toBeGreaterThan(0);
            expect(typeof task.fn).toBe('function');
        });
    }
});

describe('agent-tools-native / deps per-handler parity', () => {
    for (const h of NATIVE_READONLY_HANDLERS_DEPS) {
        it(`${h.kind}: matches, strips, builds with deps`, () => {
            const sample = DEPS_SAMPLES[h.kind];
            expect(sample, `no sample for ${h.kind}`).toBeTruthy();
            const m = (`bla ${sample} tail`).match(h.matchRe);
            expect(m, `${h.kind} matchRe failed`).not.toBeNull();
            const stripped = `before ${sample} after`.replace(h.stripRe, '');
            expect(stripped.includes('<TOOL>'), `${h.kind} stripRe left a tag`).toBe(false);
            const task = h.build(m as RegExpMatchArray, mockDeps);
            expect(typeof task.label).toBe('string');
            expect(task.label.length).toBeGreaterThan(0);
            expect(typeof task.fn).toBe('function');
        });
    }

    it('search_runbooks returns the "not configured" guidance task when runbooksDir is empty', async () => {
        const h = NATIVE_READONLY_HANDLERS_DEPS.find(x => x.kind === 'search_runbooks')!;
        const m = '<TOOL>search_runbooks:x</TOOL>'.match(h.matchRe)!;
        const task = h.build(m, { ...mockDeps, runbooksDir: '' });
        const out = await task.fn();
        expect(out).toContain('No runbooks directory is configured');
    });
});
