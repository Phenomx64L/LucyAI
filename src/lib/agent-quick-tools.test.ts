// ── agent-quick-tools.test.ts ───────────────────────────────────────────────
//
// First phase migrated behind the host port, and the first part of runAI() that
// can be executed end-to-end in a test: the recording host stands in for the
// chat UI and the Tauri backend, so these exercise the REAL code path rather
// than a re-implementation of it.

import { describe, it, expect } from 'vitest';
import { createRecordingHost } from './agent-host';
import { tryQuickNativeTool } from './agent-quick-tools';

const ctx = (host: any, doSpeak = false) => ({ tabId: 't1', doSpeak, host });

describe('tryQuickNativeTool — dispatch', () => {
    it('returns false and touches nothing when no quick tool matches', async () => {
        const host = createRecordingHost();
        expect(await tryQuickNativeTool('Tienes 32 GB de RAM.', ctx(host))).toBe(false);
        expect(host.calls).toEqual([]);
    });

    it('does not fire on a tool it does not own', async () => {
        const host = createRecordingHost();
        expect(await tryQuickNativeTool('<TOOL>threat_scan</TOOL>', ctx(host))).toBe(false);
        expect(host.calls).toEqual([]);
    });

    it('checks tools in order — sysinfo wins over a later tag', async () => {
        const host = createRecordingHost({ invoke: async () => 'RAM: 32GB' });
        await tryQuickNativeTool('<TOOL>sysinfo</TOOL><TOOL>tasklist</TOOL>', ctx(host));
        expect(host.callsTo('invoke').map((c) => c.args[0])).toEqual(['get_system_health']);
    });
});

describe('tryQuickNativeTool — sysinfo', () => {
    it('invokes the backend, renders the result and ends the turn', async () => {
        const host = createRecordingHost({ invoke: async () => 'RAM: 32GB' });
        expect(await tryQuickNativeTool('<TOOL>sysinfo</TOOL>', ctx(host))).toBe(true);
        expect(host.calls.map((c) => c.method)).toEqual(['invoke', 'addMsg', 'fin']);
        const msg = host.messages()[0];
        expect(msg.html).toContain('Lucy (Hardware)');
        expect(msg.html).toContain('RAM: 32GB');
        expect(msg.rawContent).toBe('RAM: 32GB');
    });

    it('speaks only when the turn was voice-initiated, and before fin()', async () => {
        const quiet = createRecordingHost({ invoke: async () => 'x' });
        await tryQuickNativeTool('<TOOL>sysinfo</TOOL>', ctx(quiet, false));
        expect(quiet.callsTo('speak')).toHaveLength(0);

        const loud = createRecordingHost({ invoke: async () => 'x' });
        await tryQuickNativeTool('<TOOL>sysinfo</TOOL>', ctx(loud, true));
        expect(loud.calls.map((c) => c.method)).toEqual(['invoke', 'addMsg', 'speak', 'fin']);
    });
});

describe('tryQuickNativeTool — netconn', () => {
    it('formats connection rows and caps them at 30', async () => {
        const conns = Array.from({ length: 42 }, (_, i) => ({
            protocol: 'TCP', local_addr: '127.0.0.1', local_port: 1000 + i,
            remote_addr: '10.0.0.1', remote_port: 443, state: 'ESTABLISHED', pid: i,
        }));
        const host = createRecordingHost({ invoke: async () => conns });
        expect(await tryQuickNativeTool('<TOOL>netconn</TOOL>', ctx(host))).toBe(true);
        expect(String(host.messages()[0].rawContent).split('\n')).toHaveLength(30);
    });

    it('renders a placeholder when there are no connections', async () => {
        const host = createRecordingHost({ invoke: async () => [] });
        await tryQuickNativeTool('<TOOL>netconn</TOOL>', ctx(host));
        expect(host.messages()[0].html).toContain('Sin conexiones activas.');
    });

    it('handles a missing remote address without printing "undefined"', async () => {
        const host = createRecordingHost({
            invoke: async () => [{ protocol: 'TCP', local_addr: '0.0.0.0', local_port: 445, remote_addr: null, remote_port: null, state: 'LISTENING', pid: null }],
        });
        await tryQuickNativeTool('<TOOL>netconn</TOOL>', ctx(host));
        const row = host.messages()[0].rawContent;
        expect(row).not.toContain('undefined');
        expect(row).toContain('(PID -)'); // pid ?? '-'
    });

    it('reports a backend failure as a message and STILL ends the turn', async () => {
        // Load-bearing: without fin() the tab would stay stuck in isProcessing
        // after a failed tool, with no way for the user to send another prompt.
        const host = createRecordingHost({ invoke: async () => { throw new Error('IPC down'); } });
        expect(await tryQuickNativeTool('<TOOL>netconn</TOOL>', ctx(host))).toBe(true);
        expect(host.calls.map((c) => c.method)).toEqual(['invoke', 'addMsg', 'fin']);
        expect(host.messages()[0].html).toContain('! Red');
        expect(host.messages()[0].style).toContain('#ef4444');
    });
});

describe('tryQuickNativeTool — tasklist', () => {
    it('formats process rows, caps at 25 and converts KB to MB', async () => {
        const tasks = Array.from({ length: 40 }, (_, i) => ({ name: `p${i}.exe`, pid: i, mem_kb: 2048 }));
        const host = createRecordingHost({ invoke: async () => tasks });
        await tryQuickNativeTool('<TOOL>tasklist</TOOL>', ctx(host));
        const rows = String(host.messages()[0].rawContent).split('\n');
        expect(rows).toHaveLength(25);
        expect(rows[0]).toContain('2.0 MB');
    });

    it('reports a backend failure and ends the turn', async () => {
        const host = createRecordingHost({ invoke: async () => { throw new Error('nope'); } });
        await tryQuickNativeTool('<TOOL>tasklist</TOOL>', ctx(host));
        expect(host.messages()[0].html).toContain('! Tasklist');
        expect(host.callsTo('fin')).toHaveLength(1);
    });
});

describe('tryQuickNativeTool — eventlog', () => {
    it('parses the tag and passes log name, count and level through', async () => {
        const host = createRecordingHost({ invoke: async () => [] });
        expect(await tryQuickNativeTool('<TOOL>eventlog:System:50:Error</TOOL>', ctx(host))).toBe(true);
        expect(host.callsTo('invoke')[0].args).toEqual([
            'get_event_log', { logName: 'System', count: 50, level: 'Error' },
        ]);
    });

    it('defaults the level to null when the tag omits it', async () => {
        const host = createRecordingHost({ invoke: async () => [] });
        await tryQuickNativeTool('<TOOL>eventlog:Application:10</TOOL>', ctx(host));
        expect(host.callsTo('invoke')[0].args[1]).toEqual({ logName: 'Application', count: 10, level: null });
    });

    it('clamps the requested count to 500', async () => {
        // The model routinely asks for far more events than the bubble can show,
        // and an unbounded fetch stalls the turn.
        const host = createRecordingHost({ invoke: async () => [] });
        await tryQuickNativeTool('<TOOL>eventlog:System:99999</TOOL>', ctx(host));
        expect((host.callsTo('invoke')[0].args[1] as any).count).toBe(500);
    });

    it('renders a placeholder when the log is empty', async () => {
        const host = createRecordingHost({ invoke: async () => [] });
        await tryQuickNativeTool('<TOOL>eventlog:System:10</TOOL>', ctx(host));
        expect(host.messages()[0].html).toContain('Sin eventos.');
    });
});

describe('tryQuickNativeTool — registry', () => {
    it('reads a value and renders it', async () => {
        const host = createRecordingHost({ invoke: async () => 'Windows 11 Pro' });
        expect(await tryQuickNativeTool('<TOOL>registry:HKLM|SOFTWARE\\Microsoft|ProductName</TOOL>', ctx(host))).toBe(true);
        expect(host.callsTo('invoke')[0].args).toEqual([
            'read_registry_value', { hive: 'HKLM', keyPath: 'SOFTWARE\\Microsoft', valueName: 'ProductName' },
        ]);
        expect(host.messages()[0].rawContent).toBe('Windows 11 Pro');
    });

    it('labels an empty value name as (Default)', async () => {
        const host = createRecordingHost({ invoke: async () => 'v' });
        await tryQuickNativeTool('<TOOL>registry:HKCU|Software\\Foo|</TOOL>', ctx(host));
        expect(host.callsTo('invoke')[0].args[1]).toEqual({ hive: 'HKCU', keyPath: 'Software\\Foo', valueName: '' });
        expect(host.messages()[0].html).toContain('(Default)');
    });

    it('SECURITY: refuses a sensitive hive WITHOUT calling the backend', async () => {
        // The refusal must come before the read — a blocked path is never touched.
        for (const key of ['SAM', 'SECURITY', 'SYSTEM']) {
            const host = createRecordingHost({ invoke: async () => 'leaked' });
            expect(await tryQuickNativeTool(`<TOOL>registry:HKLM|${key}|x</TOOL>`, ctx(host))).toBe(true);
            expect(host.callsTo('invoke')).toHaveLength(0);
            expect(host.messages()[0].html).toContain('Acceso denegado');
            expect(host.calls.map((c) => c.method)).toEqual(['addMsg', 'fin']);
        }
    });

    it('SECURITY: refuses paths naming a password or credential store', async () => {
        const host = createRecordingHost({ invoke: async () => 'leaked' });
        await tryQuickNativeTool('<TOOL>registry:HKCU|Software\\MyApp\\Passwords|x</TOOL>', ctx(host));
        expect(host.callsTo('invoke')).toHaveLength(0);
        expect(host.messages()[0].html).toContain('Acceso denegado');
    });

    it('reports a read failure and ends the turn', async () => {
        const host = createRecordingHost({ invoke: async () => { throw new Error('no such key'); } });
        await tryQuickNativeTool('<TOOL>registry:HKLM|Software\\Nope|x</TOOL>', ctx(host));
        expect(host.messages()[0].html).toContain('! Registro');
        expect(host.callsTo('fin')).toHaveLength(1);
    });
});
