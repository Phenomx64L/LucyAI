// ── agent-workspace.test.ts ──────────────────────────────────────────────────
//
// These pin the BOUNDS on the cockpit's workspace lanes.
//
// Two of the four lanes shipped unbounded. `execPush` stores raw command
// output — the same string that is clipped to 4 000 chars for the conversation
// bubble and truncated to 16 000 for the model on the very same line of
// +page.svelte — and `tracePush` is the mirror of `liveTrace`, whose own store
// caps at 2 000 entries with a comment costing out the memory. Mirroring into
// an unbounded array defeats that ring buffer silently: the source trims, the
// copy grows forever, and `resetWorkspace()` only fires at the START of a task,
// so a single 60-turn run keeps every byte it ever saw.
//
// A cap is the kind of thing a later refactor drops without noticing, because
// nothing downstream fails — the app just gets slower and heavier over a long
// session. Hence tests on the bound itself rather than on behaviour around it.

import { describe, it, expect, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import {
    agentExec, agentTrace, agentArtifacts, agentConvo,
    execPush, tracePush, artifactPush, convoPush,
    resetWorkspace, convoReset,
} from './agent-workspace';

beforeEach(() => { resetWorkspace(); convoReset(); });

describe('execPush — the lane that carries raw command output', () => {
    it('keeps only the most recent entries', () => {
        for (let i = 0; i < 260; i++) execPush({ cmd: `cmd-${i}`, output: 'x', ok: true });
        const l = get(agentExec);
        expect(l.length).toBe(200);
        // Newest kept, oldest dropped — a trace you can still act on.
        expect(l[l.length - 1].cmd).toBe('cmd-259');
        expect(l[0].cmd).toBe('cmd-60');
    });

    it('clips oversized output instead of storing it whole', () => {
        // A recursive listing or a log dump is megabytes. The UI never shows
        // more than a few lines of it and the model never receives more than
        // 16 000 chars, so storing the rest buys nothing.
        execPush({ cmd: 'Get-ChildItem -Recurse', output: 'y'.repeat(50_000), ok: true });
        const [e] = get(agentExec);
        expect(e.output.length).toBeLessThan(50_000);
        expect(e.output).toMatch(/truncado/);
    });

    it('leaves output that fits completely untouched', () => {
        execPush({ cmd: 'whoami', output: 'WORKSTATION\\ivan', ok: true });
        expect(get(agentExec)[0].output).toBe('WORKSTATION\\ivan');
    });
});

describe('tracePush — the mirror that was defeating liveTrace ring buffer', () => {
    it('caps at the same 2000 as the store it mirrors', () => {
        // Matching liveTrace's MAX_ENTRIES is deliberate: a mirror that holds
        // more than its source is a copy nobody budgeted for.
        for (let i = 0; i < 2_100; i++) tracePush({ phase: 'info', label: `t-${i}` });
        const l = get(agentTrace);
        expect(l.length).toBe(2_000);
        expect(l[l.length - 1].label).toBe('t-2099');
    });

    it('clips oversized detail', () => {
        // `detail` carries stderr / stdout excerpts, so it is unbounded at the
        // source too.
        tracePush({ phase: 'exec.end', label: 'ran', detail: 'z'.repeat(20_000) });
        const [t] = get(agentTrace);
        expect(t.detail!.length).toBeLessThan(20_000);
        expect(t.detail).toMatch(/truncado/);
    });

    it('leaves an entry with no detail as undefined, not an empty string', () => {
        tracePush({ phase: 'think', label: 'planning' });
        expect(get(agentTrace)[0].detail).toBeUndefined();
    });
});

describe('the lanes that were already bounded stay bounded', () => {
    it('artifactPush keeps 60 and clips both sides of the diff', () => {
        for (let i = 0; i < 70; i++) {
            artifactPush({ kind: 'edit', path: `f${i}.ts`, before: 'a'.repeat(9_000), after: 'b'.repeat(9_000) });
        }
        const l = get(agentArtifacts);
        expect(l.length).toBe(60);
        expect(l[0].before!.length).toBeLessThan(9_000);
        expect(l[0].after!.length).toBeLessThan(9_000);
    });

    it('convoPush keeps 200', () => {
        for (let i = 0; i < 240; i++) convoPush({ role: 'user', text: `m${i}` });
        const l = get(agentConvo);
        expect(l.length).toBe(200);
        expect(l[l.length - 1].text).toBe('m239');
    });
});

describe('attachment mirror contract', () => {
    it('carries kind and chars through, not just name and previewUrl', () => {
        // The declared type used to stop at `{ name, previewUrl }` — the
        // pre-v1.8.1 shape, when the mirror dropped documents. CockpitShell
        // branches on `kind` to pick a thumbnail vs a document chip and reads
        // `chars` for the size line; both must survive the push.
        convoPush({
            role: 'user',
            text: 'mira este informe',
            atts: [
                { name: 'shot.png', kind: 'image', previewUrl: 'data:image/png;base64,AAA' },
                { name: 'guide.pdf', kind: 'pdf', chars: 12_345 },
            ],
        });
        const [m] = get(agentConvo);
        expect(m.atts).toHaveLength(2);
        expect(m.atts![0].kind).toBe('image');
        expect(m.atts![1].kind).toBe('pdf');
        expect(m.atts![1].chars).toBe(12_345);
        // A document has no preview — that is what made the old mirror drop it.
        expect(m.atts![1].previewUrl).toBeUndefined();
    });
});
