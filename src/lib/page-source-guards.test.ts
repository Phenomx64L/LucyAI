// ── Source-level guards for src/routes/+page.svelte ──────────────────────────
//
// The monolith cannot be imported into a test: it is a Svelte component with a
// 14k-line script that touches Tauri, the DOM and localStorage at module scope.
// A whole class of defect in it is therefore unreachable by ordinary unit
// tests, and this file exists for the ones that are still mechanically
// checkable from the source text. Precedent: `utils/shell.rs` and
// `utils/db.rs::catalog_contract` do the same in Rust with `include_str!`.
//
// (It lives in src/lib because vitest.config only includes `src/lib/**`.)
//
// This is a narrow tool. It catches "the code says something it must not say",
// never "the code does the wrong thing". Prefer a real test whenever one is
// possible.

import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const SRC = readFileSync(
    fileURLToPath(new URL('../routes/+page.svelte', import.meta.url)),
    'utf8',
);
const read = (rel: string) => readFileSync(fileURLToPath(new URL(rel, import.meta.url)), 'utf8');

describe('+page.svelte source guards', () => {
    it('finds the file it is supposed to be guarding', () => {
        // Without this, a moved or renamed file turns every guard below into a
        // vacuous pass over an empty string.
        expect(SRC.length).toBeGreaterThan(100_000);
    });

    it('chatInput() reads the DOM instead of calling itself', () => {
        // The bug this pins actually shipped. Phase 2b replaced all 19 verbatim
        // occurrences of `document.querySelector('.chat-wrap.on .ibox')` with
        // `chatInput()` in a single pass — and the pass also rewrote the body of
        // the accessor it had just written, leaving:
        //
        //     function chatInput() { return chatInput(); }
        //
        // Every focus of the composer was an immediate stack overflow. Nothing
        // caught it: `check`, `check:js`, 561 tests and the build were all
        // green, because unbounded recursion is neither a type error nor
        // reachable from any test that can import this file.
        const body = SRC.match(/function chatInput\(\)\s*\{([\s\S]*?)\n {4}\}/);
        expect(body, 'chatInput() not found — was it renamed?').not.toBeNull();

        const code = (body![1] ?? '').replace(/\/\/[^\n]*/g, '');
        expect(code, 'chatInput() must not call itself').not.toMatch(/\bchatInput\s*\(/);
        expect(code).toMatch(/document\.querySelector/);
    });

    it('keeps the composer selector in exactly one place', () => {
        // The point of the accessor. Two copies is how the pair drifts apart,
        // and a selector that matches nothing fails silently — the input simply
        // never takes focus, with no error anywhere.
        const hits = SRC.split("'.chat-wrap.on .ibox'").length - 1;
        expect(hits, 'the selector should appear once, inside chatInput()').toBe(1);
    });
});

describe('LiveTracePanel covers every TracePhase', () => {
    // Adding a phase means editing FOUR places: the union in liveTrace.ts, and
    // `enabledPhases`, `phaseIcon` and `phaseColor` in the panel. Three of them
    // are hand-maintained lists with no link to the first.
    //
    // `warn` had already drifted: five call sites emitted it, the union did not
    // declare it, and the panel filters on
    // `if (!enabledPhases.has(e.phase)) continue`. So those entries — three of
    // them "✗ fork_… failed" — were written to the ring buffer and dropped
    // before rendering. A trace panel silently discarding the failures it
    // exists to show is the worst version of this bug, because the panel still
    // looks like it is working.
    //
    // `phaseIcon`/`phaseColor` are component-internal, so this reads the source
    // rather than calling them.
    const union = read('./liveTrace.ts')
        .match(/export type TracePhase[\s\S]*?;/)?.[0] ?? '';
    const panel = read('./LiveTracePanel.svelte');
    const phases = [...union.matchAll(/'([a-z.]+)'/g)].map((m) => m[1]);

    it('found the phase union to check against', () => {
        expect(phases.length).toBeGreaterThan(5);
        expect(phases).toContain('warn');
    });

    it.each(['enabledPhases', 'phaseIcon', 'phaseColor'])(
        'every phase appears in %s',
        (region) => {
            // Crude but sufficient: each phase name must appear somewhere in the
            // panel's source for each of the three places that must list it.
            // A phase missing from any one of them is either invisible or
            // renders with no icon and no colour.
            const body =
                region === 'enabledPhases'
                    ? panel.match(/enabledPhases = new Set<TracePhase>\(\[[\s\S]*?\]\)/)?.[0] ?? ''
                    : panel.match(new RegExp(`function ${region}[\\s\\S]*?\\n {4}\\}`))?.[0] ?? '';
            expect(body, `${region} not found in LiveTracePanel`).not.toBe('');
            for (const p of phases) {
                expect(body, `'${p}' missing from ${region}`).toContain(`'${p}'`);
            }
        },
    );
});
