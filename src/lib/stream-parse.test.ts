// ── stream-parse.test.ts ─────────────────────────────────────────────────────
// Locks the streamed-<THOUGHT> parser behaviour that used to live (untested)
// nested inside +page.svelte.

import { describe, it, expect } from 'vitest';
import { makeThoughtStreamer } from './stream-parse';

describe('makeThoughtStreamer', () => {
    it('emits only the newly-revealed reasoning delta as the accumulator grows', () => {
        const out: string[] = [];
        const feed = makeThoughtStreamer((d) => out.push(d));
        feed('<THOUGHT>Hel');
        feed('<THOUGHT>Hello');
        feed('<THOUGHT>Hello wor');
        expect(out).toEqual(['Hel', 'lo', ' wor']);
    });

    it('emits nothing until the open tag appears', () => {
        const out: string[] = [];
        const feed = makeThoughtStreamer((d) => out.push(d));
        feed('no tag yet');
        feed('still ');
        expect(out).toEqual([]);
        feed('<THOUGHT>x');
        expect(out).toEqual(['x']);
    });

    it('stops permanently once </THOUGHT> closes — later chunks are ignored', () => {
        const out: string[] = [];
        const feed = makeThoughtStreamer((d) => out.push(d));
        feed('<THOUGHT>done</THOUGHT>');
        feed('<THOUGHT>done</THOUGHT> and then the ANSWER body keeps streaming');
        expect(out).toEqual(['done']);
    });

    it('handles the open tag being followed by the close in one shot', () => {
        const out: string[] = [];
        const feed = makeThoughtStreamer((d) => out.push(d));
        feed('prefix <THOUGHT>reasoning here</THOUGHT> suffix');
        expect(out).toEqual(['reasoning here']);
    });

    it('is case-insensitive on the open tag', () => {
        const out: string[] = [];
        const feed = makeThoughtStreamer((d) => out.push(d));
        feed('<thought>lower');
        expect(out).toEqual(['lower']);
    });

    it('never throws on empty / falsy input', () => {
        const feed = makeThoughtStreamer(() => {});
        expect(() => { feed(''); feed(undefined as unknown as string); }).not.toThrow();
    });
});
