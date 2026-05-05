import { describe, it, expect, beforeEach } from 'vitest';
import {
    observe,
    getProposals,
    markAccepted,
    dismissProposal,
    resetForTab,
} from './skill-factory';

// In Node, localStorage isn't defined by default. The skill-factory uses
// safe-ls, which falls back gracefully — but for these tests we want a
// real per-test backing store so the module's persistence behavior is
// exercised. Provide a minimal in-memory shim.
class MemStorage {
    map = new Map<string, string>();
    get length() { return this.map.size; }
    key(i: number) { return Array.from(this.map.keys())[i] ?? null; }
    getItem(k: string) { return this.map.has(k) ? this.map.get(k)! : null; }
    setItem(k: string, v: string) { this.map.set(k, String(v)); }
    removeItem(k: string) { this.map.delete(k); }
    clear() { this.map.clear(); }
}
(globalThis as any).localStorage = new MemStorage();

const TAB = 'tab-test';

beforeEach(() => {
    (globalThis as any).localStorage.clear();
    resetForTab(TAB);
});

describe('skill-factory / observe + getProposals', () => {
    it('returns nothing on a fresh tab', () => {
        expect(getProposals(TAB)).toEqual([]);
    });

    it('proposes a SEQUENCE after just 2 occurrences', () => {
        const seq = ['Get-Service IIS', 'Restart-Service IIS', 'Get-Service IIS'];
        // Two full sequences in the buffer.
        for (let i = 0; i < 2; i++) {
            for (const cmd of seq) {
                observe(TAB, { cmd, target: 'local', engine: 'powershell', ts: Date.now(), ok: true });
            }
        }
        const props = getProposals(TAB);
        expect(props.length).toBeGreaterThan(0);
        expect(props[0].kind).toBe('sequence');
        expect(props[0].occurrences).toBeGreaterThanOrEqual(2);
        expect(props[0].suggestedScript.split('\n').length).toBeGreaterThanOrEqual(2);
    });

    it('proposes a SINGLE only after 3+ occurrences', () => {
        for (let i = 0; i < 4; i++) {
            observe(TAB, { cmd: 'Test-Connection 8.8.8.8', target: 'local', engine: 'powershell', ts: Date.now(), ok: true });
        }
        const props = getProposals(TAB);
        // Single may co-exist with sequence; verify at least one is single.
        const single = props.find(p => p.kind === 'single');
        expect(single).toBeDefined();
        expect(single!.occurrences).toBeGreaterThanOrEqual(3);
    });

    it('rejects ok=false observations entirely', () => {
        for (let i = 0; i < 5; i++) {
            observe(TAB, { cmd: 'Get-FailingThing', target: 'local', engine: 'powershell', ts: Date.now(), ok: false });
        }
        expect(getProposals(TAB)).toEqual([]);
    });

    it('does not re-propose immediately after dismiss', () => {
        for (let i = 0; i < 4; i++) {
            observe(TAB, { cmd: 'Get-Process explorer', target: 'local', engine: 'powershell', ts: Date.now(), ok: true });
        }
        const first = getProposals(TAB);
        expect(first.length).toBeGreaterThan(0);
        dismissProposal(TAB, first[0].fingerprint);
        const second = getProposals(TAB);
        // The dismissed fingerprint must not reappear.
        expect(second.find(p => p.fingerprint === first[0].fingerprint)).toBeUndefined();
    });

    it('respects cooldown after markAccepted', () => {
        for (let i = 0; i < 4; i++) {
            observe(TAB, { cmd: 'Get-Disk', target: 'local', engine: 'powershell', ts: Date.now(), ok: true });
        }
        const list = getProposals(TAB);
        expect(list.length).toBeGreaterThan(0);
        markAccepted(TAB, list[0].fingerprint);
        // Within the cooldown the same fingerprint shouldn't reappear.
        const list2 = getProposals(TAB);
        expect(list2.find(p => p.fingerprint === list[0].fingerprint)).toBeUndefined();
    });

    it('produces a kebab-case suggested name', () => {
        for (let i = 0; i < 4; i++) {
            observe(TAB, { cmd: 'Get-EventLog -LogName System', target: 'local', engine: 'powershell', ts: Date.now(), ok: true });
        }
        const list = getProposals(TAB);
        const single = list.find(p => p.kind === 'single')!;
        expect(single.suggestedName).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
    });
});
