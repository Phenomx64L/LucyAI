// ── agent-host.test.ts — the test double that later phases assert against ───
//
// agent-host.ts is types + doubles only, so there is no production behaviour to
// test here. What IS worth pinning is the double itself: every phase migrated in
// Phase 2+ will be verified through `createRecordingHost`, so if the recorder
// silently drops calls, those migrations get a false green.

import { describe, it, expect } from 'vitest';
import { createRecordingHost, seedTab } from './agent-host';

describe('createRecordingHost', () => {
    it('records calls in order, across methods', () => {
        const host = createRecordingHost();
        host.addThinking('t1');
        host.refresh();
        host.fin('t1');
        expect(host.calls.map((c) => c.method)).toEqual(['addThinking', 'refresh', 'fin']);
    });

    it('records arguments verbatim', () => {
        const host = createRecordingHost();
        host.logTaskEvent('provider_fallback', 'empty_response', null, { from: 'a', to: 'b' }, 't1');
        expect(host.callsTo('logTaskEvent')[0].args).toEqual([
            'provider_fallback', 'empty_response', null, { from: 'a', to: 'b' }, 't1',
        ]);
    });

    it('exposes addMsg payloads through messages()', () => {
        const host = createRecordingHost();
        host.addMsg('t1', { role: 'lucy', html: '<b>hi</b>' });
        host.addMsg('t1', { role: 'user', rawContent: 'hola' });
        expect(host.messages()).toEqual([
            { role: 'lucy', html: '<b>hi</b>' },
            { role: 'user', rawContent: 'hola' },
        ]);
    });

    it('appends to a seeded tab so message-order assertions work', () => {
        const host = createRecordingHost();
        const tab = seedTab(host, { id: 't1' });
        host.addMsg('t1', { role: 'lucy', html: 'a' });
        host.addMsg('t1', { role: 'lucy', html: 'b' });
        expect(tab.messages.map((m) => m.html)).toEqual(['a', 'b']);
        expect(host.getTab('t1')).toBe(tab);
        expect(host.getTab('nope')).toBeUndefined();
    });

    it('records toast calls under a namespaced method name', () => {
        const host = createRecordingHost();
        host.toast.error('boom');
        host.toast.success('ok');
        expect(host.calls.map((c) => c.method)).toEqual(['toast.error', 'toast.success']);
        expect(host.callsTo('toast.error')[0].args).toEqual(['boom']);
    });

    it('records the HITL confirmations that halt a turn', () => {
        const host = createRecordingHost();
        host.confirmRunAs({ cmd: 'Remove-Item C:\\x', ctx: '', doSpeak: false, tabId: 't1', isDestructive: true });
        expect(host.callsTo('confirmRunAs')[0].args[0].isDestructive).toBe(true);
    });

    it('records the learn confirmation — the third halt (Phase 4)', () => {
        // Phase 1 named confirmRunAs and confirmSecurityBlock but missed this
        // one: it kept assigning pendingLearn* and raising the modal inline, so
        // a headless caller reaching that branch would have written component
        // variables that, for it, do not exist.
        const host = createRecordingHost();
        host.confirmLearn({
            claves: ['reiniciar spooler', 'spooler colgado'],
            script: 'Restart-Service Spooler',
            respuesta: 'Reinicio el servicio de cola de impresión.',
            tabId: 't1',
            doSpeak: false,
        });

        const req = host.callsTo('confirmLearn')[0].args[0];
        expect(req.claves).toHaveLength(2);
        expect(req.script).toBe('Restart-Service Spooler');
        expect(req.tabId).toBe('t1');
        expect(req.doSpeak).toBe(false);
    });

    it('all three HITL halts share the same shape: recorded, and nothing else fires', () => {
        // The contract each one relies on: the caller fin()s and returns. If a
        // halt ever started doing work of its own, this would catch it.
        const host = createRecordingHost();
        host.confirmRunAs({ cmd: 'x', ctx: '', doSpeak: false, tabId: 't1' });
        host.confirmSecurityBlock({ tabId: 't1', cmd: 'x', ctx: '', doSpeak: false, blockWord: 'format', displayCmd: 'x', execType: 'ps', token: null });
        host.confirmLearn({ claves: ['a'], script: 'b', respuesta: 'c', tabId: 't1', doSpeak: false });

        expect(host.calls.map((c) => c.method)).toEqual([
            'confirmRunAs', 'confirmSecurityBlock', 'confirmLearn',
        ]);
    });

    it('invoke resolves undefined by default', async () => {
        const host = createRecordingHost();
        await expect(host.invoke('get_system_health')).resolves.toBeUndefined();
        expect(host.callsTo('invoke')[0].args).toEqual(['get_system_health', undefined]);
    });

    it('scripted overrides still get recorded — the call log stays complete', () => {
        // Without the override-wrapping in createRecordingHost, a test that
        // scripts `invoke` would lose the log for the very call it is about.
        const host = createRecordingHost({
            invoke: async (cmd: string) => (cmd === 'get_system_health' ? 'RAM: 32GB' : null),
        });
        return host.invoke('get_system_health').then((out) => {
            expect(out).toBe('RAM: 32GB');
            expect(host.callsTo('invoke')).toHaveLength(1);
            expect(host.callsTo('invoke')[0].args[0]).toBe('get_system_health');
        });
    });

    it('non-function overrides (toast) replace wholesale without breaking the log', () => {
        const seen: string[] = [];
        const host = createRecordingHost({
            toast: {
                success: (m) => seen.push('s:' + m),
                error: (m) => seen.push('e:' + m),
                info: (m) => seen.push('i:' + m),
                warning: (m) => seen.push('w:' + m),
            },
        });
        host.toast.error('x');
        host.refresh();
        expect(seen).toEqual(['e:x']);
        // The custom toast bypasses the recorder by design; other calls still log.
        expect(host.calls.map((c) => c.method)).toEqual(['refresh']);
    });
});
