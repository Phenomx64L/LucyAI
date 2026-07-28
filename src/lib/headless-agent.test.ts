import { describe, it, expect, vi } from 'vitest';
import {
    runHeadlessAgent,
    stripToolTags,
    findMutatingTag,
    bindDepsHandlers,
    SUBAGENT_DEPS_TOOLS,
    type HeadlessAgentOptions,
} from './headless-agent';
import { NATIVE_READONLY_HANDLERS_DEPS } from './agent-tools-native';
import type { NativeHandler } from './agent-tools-native';

/** Fake read-only handler so the loop tests never touch Tauri. */
function fakeHandler(kind: string, result: string, onCall?: () => void): NativeHandler {
    return {
        kind,
        matchRe: new RegExp(`<TOOL>${kind}</TOOL>`, 'i'),
        stripRe: new RegExp(`<TOOL>${kind}</TOOL>`, 'gi'),
        build: () => ({
            label: `[${kind}]`,
            fn: async () => { onCall?.(); return result; },
        }),
    };
}

/** Scripts a sequence of model replies. */
function scriptedAskLucy(replies: string[]) {
    const calls: Array<{ prompt: string; context: string }> = [];
    const fn = vi.fn(async (prompt: string, context: string) => {
        calls.push({ prompt, context });
        return replies[Math.min(calls.length - 1, replies.length - 1)];
    });
    return { fn, calls };
}

function opts(askLucy: HeadlessAgentOptions['askLucy'], extra: Partial<HeadlessAgentOptions> = {}): HeadlessAgentOptions {
    return { askLucy, handlers: [], ...extra };
}

describe('stripToolTags', () => {
    it('removes TOOL, THOUGHT and EXECUTE blocks', () => {
        const raw = 'Antes <TOOL>sysinfo</TOOL> medio <THOUGHT>pensando</THOUGHT> despues';
        expect(stripToolTags(raw)).toBe('Antes  medio  despues');
    });

    it('collapses the blank lines left behind', () => {
        expect(stripToolTags('a\n\n\n\n<TOOL>x</TOOL>\n\n\n\nb')).toBe('a\n\nb');
    });

    it('leaves plain prose untouched', () => {
        expect(stripToolTags('  informe normal  ')).toBe('informe normal');
    });
});

describe('findMutatingTag', () => {
    it.each([
        ['<EXECUTE_CMD>rm -rf /</EXECUTE_CMD>'],
        ['<TOOL>writefile:C:\\x.txt</TOOL>'],
        ['<TOOL>editfile:a.rs</TOOL>'],
        ['<TOOL>panic_kill:1234</TOOL>'],
        ['<TOOL>schedule_create:x</TOOL>'],
    ])('flags %s', (text) => {
        expect(findMutatingTag(text)).not.toBeNull();
    });

    it.each([
        ['<TOOL>sysinfo</TOOL>'],
        ['<TOOL>tasklist</TOOL>'],
        ['<TOOL>eventlog:System:50</TOOL>'],
        ['respuesta sin herramientas'],
    ])('allows %s', (text) => {
        expect(findMutatingTag(text)).toBeNull();
    });
});

describe('SUBAGENT_DEPS_TOOLS — the allow-list', () => {
    const deps: any = {
        retryWithBackoff: async (fn: any) => fn(),
        cachedFetch: async (_c: string, _q: string, p: any) => p(),
        mcpServers: [], mcpSecrets: {}, loadMcpServers: async () => [],
        runbooksDir: '', tabId: 'fork:test',
    };

    it('every allowed name exists in the deps table', () => {
        // A rename upstream must fail here, not silently shrink what a
        // sub-agent can do while the prompt keeps advertising the old name.
        const known = NATIVE_READONLY_HANDLERS_DEPS.map((h) => h.kind);
        for (const k of SUBAGENT_DEPS_TOOLS) expect(known).toContain(k);
    });

    it('EXCLUDES the deps tools a background agent must not have', () => {
        // The point of the list. `start_indexer` kicks off a background job,
        // `obj_query` needs a live PowerShell session bound to a real tab, and
        // the rest reach the network. None belong to an unattended agent.
        for (const forbidden of ['start_indexer', 'obj_query', 'fetch', 'search_web', 'mcp_discover']) {
            expect(SUBAGENT_DEPS_TOOLS as readonly string[]).not.toContain(forbidden);
        }
    });

    it('binds only the allowed handlers', () => {
        const bound = bindDepsHandlers(SUBAGENT_DEPS_TOOLS, deps);
        expect(bound.map((h) => h.kind).sort()).toEqual([...SUBAGENT_DEPS_TOOLS].sort());
    });

    it('drops unknown names instead of throwing', () => {
        // A tool disappearing upstream should narrow the sub-agent, never crash
        // the turn that launched it.
        const bound = bindDepsHandlers(['sysinfo', 'herramienta_inexistente'], deps);
        expect(bound.map((h) => h.kind)).toEqual(['sysinfo']);
    });

    it('produces handlers the loop can actually drive', () => {
        const bound = bindDepsHandlers(['sysinfo'], deps);
        const m = '<TOOL>sysinfo</TOOL>'.match(bound[0].matchRe);
        expect(m).not.toBeNull();
        const task = bound[0].build(m!);
        expect(task.label).toBeTruthy();
        expect(typeof task.fn).toBe('function');
    });

    it('a bound handler still refuses a mutating request at the loop level', async () => {
        // Belt and braces: even with real tools bound, the mutation gate is what
        // keeps an unattended sub-agent read-only.
        const { fn } = scriptedAskLucy(['<TOOL>writefile:C:\\x.txt</TOOL>']);
        const r = await runHeadlessAgent('escribe', {
            askLucy: fn,
            handlers: bindDepsHandlers(SUBAGENT_DEPS_TOOLS, deps),
        });
        expect(r.status).toBe('blocked');
    });
});

describe('runHeadlessAgent', () => {
    it('returns ok and the prose when the model answers without tools', async () => {
        const { fn } = scriptedAskLucy(['La CPU esta al 12%.']);
        const r = await runHeadlessAgent('estado?', opts(fn));

        expect(r.status).toBe('ok');
        expect(r.text).toBe('La CPU esta al 12%.');
        expect(r.steps).toEqual([]);
        expect(r.iterations).toBe(1);
        expect(fn).toHaveBeenCalledTimes(1);
    });

    it('executes a read-only tool and feeds its output into the next turn', async () => {
        const { fn, calls } = scriptedAskLucy([
            '<TOOL>sysinfo</TOOL>',
            'Informe final: todo correcto.',
        ]);
        const r = await runHeadlessAgent('reporte', opts(fn, {
            handlers: [fakeHandler('sysinfo', '[SYSINFO] cpu=12%')],
        }));

        expect(r.status).toBe('ok');
        expect(r.text).toBe('Informe final: todo correcto.');
        expect(r.steps).toEqual(['[sysinfo]']);
        expect(r.iterations).toBe(2);
        // The tool result must reach the model, otherwise the loop is decorative.
        expect(calls[1].context).toContain('[SYSINFO] cpu=12%');
    });

    it('runs every read-only tool present in one reply', async () => {
        const { fn } = scriptedAskLucy([
            '<TOOL>sysinfo</TOOL> y <TOOL>tasklist</TOOL>',
            'listo',
        ]);
        const r = await runHeadlessAgent('reporte', opts(fn, {
            handlers: [fakeHandler('sysinfo', 'A'), fakeHandler('tasklist', 'B')],
        }));

        expect(r.steps).toEqual(['[sysinfo]', '[tasklist]']);
        expect(r.status).toBe('ok');
    });

    it('BLOCKS a mutating tool instead of executing it unattended', async () => {
        const ran = vi.fn();
        const { fn } = scriptedAskLucy(['<TOOL>writefile:C:\\tmp\\x.txt</TOOL>']);
        const r = await runHeadlessAgent('escribe algo', opts(fn, {
            handlers: [fakeHandler('writefile', 'escrito', ran)],
        }));

        expect(r.status).toBe('blocked');
        expect(r.blockedBy).toContain('writefile');
        expect(ran).not.toHaveBeenCalled();
        expect(fn).toHaveBeenCalledTimes(1);
    });

    it('BLOCKS an <EXECUTE> shell request', async () => {
        const { fn } = scriptedAskLucy(['<EXECUTE_CMD>Stop-Service W32Time</EXECUTE_CMD>']);
        const r = await runHeadlessAgent('para el servicio', opts(fn));

        expect(r.status).toBe('blocked');
        expect(r.blockedBy).toContain('EXECUTE');
    });

    it('blocks — never silently continues — on a tool with no handler', async () => {
        const { fn } = scriptedAskLucy(['<TOOL>herramienta_desconocida:x</TOOL>']);
        const r = await runHeadlessAgent('haz algo', opts(fn));

        expect(r.status).toBe('blocked');
        expect(r.steps).toEqual([]);
    });

    it('stops at the iteration ceiling when the model keeps asking for tools', async () => {
        const { fn } = scriptedAskLucy(['<TOOL>sysinfo</TOOL>']); // never resolves
        const r = await runHeadlessAgent('bucle', opts(fn, {
            handlers: [fakeHandler('sysinfo', 'x')],
            maxIterations: 3,
        }));

        expect(r.status).toBe('max_iterations');
        expect(r.iterations).toBe(3);
        expect(fn).toHaveBeenCalledTimes(3);
    });

    it('treats a failing tool as data, not as a crash', async () => {
        const boom: NativeHandler = {
            kind: 'eventlog',
            matchRe: /<TOOL>eventlog<\/TOOL>/i,
            stripRe: /<TOOL>eventlog<\/TOOL>/gi,
            build: () => ({ label: '[eventlog]', fn: async () => { throw new Error('acceso denegado'); } }),
        };
        const { fn, calls } = scriptedAskLucy(['<TOOL>eventlog</TOOL>', 'No pude leer el log.']);
        const r = await runHeadlessAgent('logs', opts(fn, { handlers: [boom] }));

        expect(r.status).toBe('ok');
        expect(calls[1].context).toContain('acceso denegado');
    });

    it('reports progress through onStep', async () => {
        const seen: string[] = [];
        const { fn } = scriptedAskLucy(['<TOOL>sysinfo</TOOL>', 'fin']);
        await runHeadlessAgent('x', opts(fn, {
            handlers: [fakeHandler('sysinfo', 'ok')],
            onStep: (l) => seen.push(l),
        }));

        expect(seen).toEqual(['[sysinfo]']);
    });
});
