import { describe, it, expect } from 'vitest';
import {
    FILE_TOOL_RE, NATIVE_TOOL_RE,
    hasToolResponse, needsAgentLoop, isMultiStepResponse, detectToolKind,
} from './agent-tools';

describe('agent-tools / FILE_TOOL_RE', () => {
    const fileTools = [
        'readfile', 'readlines', 'writefile', 'listdir', 'searchfiles', 'editfile',
        'locate_file', 'start_indexer', 'analyze_code', 'mcp_query', 'graphify',
        'memoria_guardar', 'memoria_buscar', 'memoria_eliminar', 'memoria_consolidar',
        'memory_core_set', 'memory_core_delete', 'fork_task', 'wait_task', 'cd',
        'pdf_search', 'principle_set', 'principle_delete', 'schedule_create', 'schedule_list',
    ];
    it('matches every declared file tool', () => {
        for (const t of fileTools) {
            expect(FILE_TOOL_RE.test(`<TOOL>${t}:arg</TOOL>`), t).toBe(true);
        }
    });
    it('does not match native-only tools', () => {
        expect(FILE_TOOL_RE.test('<TOOL>sysinfo</TOOL>')).toBe(false);
    });
});

describe('agent-tools / NATIVE_TOOL_RE — incl. the entries llm-stream had dropped', () => {
    // These were MISSING from llm-stream.ts's stale copy — the divergence bug.
    // Locking them in here means the canonical list can never silently regress.
    const recovered = [
        'state_diff:1', 'process_lineage:x', 'process_ancestry:42', 'diagnose_spike',
        'healing_find:x', 'threat_scan', 'obj_query:x', 'runbook_scan', 'daily_patterns',
        'sandbox_preview:x', 'kg_neighbors:x', 'kg_recent', 'kg_ext_summary', 'incident_detective',
    ];
    it('matches the previously-divergent native tools', () => {
        for (const t of recovered) {
            expect(NATIVE_TOOL_RE.test(`<TOOL>${t}</TOOL>`), t).toBe(true);
        }
    });
    it('still matches the original native tools', () => {
        for (const t of ['sysinfo', 'netconn', 'tasklist', 'eventlog:System:1001', 'search_web:dns', 'fetch:https://x']) {
            expect(NATIVE_TOOL_RE.test(`<TOOL>${t}</TOOL>`), t).toBe(true);
        }
    });
});

describe('agent-tools / predicates', () => {
    it('hasToolResponse', () => {
        expect(hasToolResponse('<TOOL>sysinfo</TOOL>')).toBe(true);
        expect(hasToolResponse('<EXECUTE>Get-Process</EXECUTE>')).toBe(true);
        expect(hasToolResponse('<THOUGHT>thinking</THOUGHT>')).toBe(true);
        expect(hasToolResponse('just a plain answer')).toBe(false);
    });
    it('needsAgentLoop covers file, native, and THOUGHT', () => {
        expect(needsAgentLoop('<TOOL>readfile:/x</TOOL>')).toBe(true);
        expect(needsAgentLoop('<TOOL>threat_scan</TOOL>')).toBe(true);   // was broken in llm-stream
        expect(needsAgentLoop('<THOUGHT>...</THOUGHT>')).toBe(true);
        expect(needsAgentLoop('plain text')).toBe(false);
    });
    it('isMultiStepResponse', () => {
        expect(isMultiStepResponse('<THOUGHT>x</THOUGHT>')).toBe(true);
        expect(isMultiStepResponse('<TOOL>readfile:/x</TOOL> then <EXECUTE>y</EXECUTE>')).toBe(true);
        expect(isMultiStepResponse('<TOOL>readfile:/x</TOOL>')).toBe(false); // TOOL without EXECUTE, no THOUGHT
        expect(isMultiStepResponse('plain')).toBe(false);
    });
});

describe('agent-tools / detectToolKind', () => {
    it('returns the file tool name (stripped of trailing colon)', () => {
        expect(detectToolKind('<TOOL>writefile:/a.txt|hi</TOOL>')).toBe('writefile');
        expect(detectToolKind('<TOOL>readfile:/x</TOOL>')).toBe('readfile');
    });
    it('returns the native tool name', () => {
        expect(detectToolKind('<TOOL>process_lineage:123</TOOL>')).toBe('process_lineage');
        expect(detectToolKind('<TOOL>sysinfo</TOOL>')).toBe('sysinfo');
    });
    it('prefers file tools when both appear (matches loop order)', () => {
        expect(detectToolKind('<TOOL>readfile:/x</TOOL> <TOOL>sysinfo</TOOL>')).toBe('readfile');
    });
    it('returns null when there is no tool tag', () => {
        expect(detectToolKind('<THOUGHT>just thinking</THOUGHT>')).toBe(null);
        expect(detectToolKind('plain answer')).toBe(null);
    });
});
