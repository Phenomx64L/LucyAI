// @vitest-environment jsdom
// ── theme-loader.test.ts ──────────────────────────────────────────────────
//
// Tier B #3 — Regression guards for the JSON theming subsystem.
// Pins:
//   • Validation rejects unknown vars and malformed color strings
//   • upsert is idempotent (replaces by id, not duplicates)
//   • The injected <style> tag only contains whitelisted properties
//   • Import round-trips with export

import { describe, it, expect, beforeEach } from 'vitest';
import {
    validateOrThrow,
    upsertCustomTheme,
    deleteCustomTheme,
    listCustomThemes,
    injectStyleTag,
    exportThemeJson,
    importThemeJson,
    type CustomTheme,
} from './theme-loader';

function freshTheme(over: Partial<CustomTheme> = {}): CustomTheme {
    return {
        id: 'mocha-dark',
        name: 'Mocha Dark',
        vars: {
            '--bg-top':    '#4a3b2b',
            '--bg-mid':    '#241b12',
            '--bg-bottom': '#0f0806',
        },
        ...over,
    };
}

describe('theme-loader / validateOrThrow', () => {
    beforeEach(() => localStorage.clear());

    it('accepts a well-formed theme', () => {
        expect(() => validateOrThrow(freshTheme())).not.toThrow();
    });

    it('rejects an id with disallowed characters', () => {
        expect(() => validateOrThrow(freshTheme({ id: 'Mocha Dark!' })))
            .toThrowError(/id must match/);
    });

    it('rejects a name longer than 60 chars', () => {
        expect(() => validateOrThrow(freshTheme({ name: 'x'.repeat(61) })))
            .toThrowError(/name must be 1\.\.60/);
    });

    it('rejects unknown CSS variable keys', () => {
        // Attempt to smuggle an unrelated variable.
        const bad = freshTheme();
        (bad.vars as Record<string, string>)['--evil-property'] = '#ffffff';
        expect(() => validateOrThrow(bad)).toThrowError(/disallowed variable/);
    });

    it('rejects malformed color values', () => {
        const bad = freshTheme({ vars: { '--bg-top': 'not-a-color' as string } });
        expect(() => validateOrThrow(bad)).toThrowError(/not a valid color/);
    });

    it('accepts hex / rgb / rgba / hsl color forms', () => {
        const colors = ['#0af', '#10b981', 'rgb(10,20,30)', 'rgba(10,20,30,0.5)', 'hsl(120,50%,40%)'];
        for (const c of colors) {
            expect(() => validateOrThrow(freshTheme({ vars: { '--bg-top': c } })))
                .not.toThrow();
        }
    });
});

describe('theme-loader / upsert + delete', () => {
    beforeEach(() => localStorage.clear());

    it('adds a new theme to the registry', () => {
        upsertCustomTheme(freshTheme());
        const all = listCustomThemes();
        expect(all).toHaveLength(1);
        expect(all[0].id).toBe('mocha-dark');
    });

    it('replaces by id rather than duplicating', () => {
        upsertCustomTheme(freshTheme());
        upsertCustomTheme(freshTheme({ name: 'Mocha Dark v2' }));
        const all = listCustomThemes();
        expect(all).toHaveLength(1);
        expect(all[0].name).toBe('Mocha Dark v2');
    });

    it('preserves order for distinct ids', () => {
        upsertCustomTheme(freshTheme({ id: 'first',  name: 'First'  }));
        upsertCustomTheme(freshTheme({ id: 'second', name: 'Second' }));
        const ids = listCustomThemes().map(t => t.id);
        expect(ids).toEqual(['first', 'second']);
    });

    it('removes by id', () => {
        upsertCustomTheme(freshTheme());
        deleteCustomTheme('mocha-dark');
        expect(listCustomThemes()).toHaveLength(0);
    });
});

describe('theme-loader / injectStyleTag', () => {
    beforeEach(() => {
        localStorage.clear();
        const el = document.getElementById('lucy-custom-themes');
        if (el) el.remove();
    });

    it('creates the <style> tag if missing', () => {
        injectStyleTag([freshTheme()]);
        const el = document.getElementById('lucy-custom-themes');
        expect(el).not.toBeNull();
        expect(el?.tagName).toBe('STYLE');
    });

    it('emits CSS scoped to data-theme="custom-<id>"', () => {
        injectStyleTag([freshTheme()]);
        const css = document.getElementById('lucy-custom-themes')?.textContent || '';
        expect(css).toMatch(/\[data-theme="custom-mocha-dark"\]/);
        expect(css).toMatch(/--bg-top:\s*#4a3b2b/);
    });

    it('silently drops malformed colors from the emitted CSS', () => {
        // Even if a bad theme somehow lands in storage (older Lucy version),
        // the renderer must not inject the bad rule. validateOrThrow catches
        // upsert; this is defence in depth.
        const sneaky: CustomTheme = {
            id: 'bad',
            name: 'Bad',
            vars: {
                '--bg-top': '#10b981',
                // @ts-expect-error testing runtime defensive behaviour
                '--evil': 'red',
            },
        };
        injectStyleTag([sneaky]);
        const css = document.getElementById('lucy-custom-themes')?.textContent || '';
        expect(css).not.toMatch(/--evil/);
        expect(css).toMatch(/--bg-top:\s*#10b981/);
    });
});

describe('theme-loader / export + import round-trip', () => {
    it('importThemeJson(exportThemeJson(t)) yields an equivalent theme', () => {
        const t = freshTheme();
        const json = exportThemeJson(t);
        const round = importThemeJson(json);
        expect(round).toEqual(t);
    });

    it('importThemeJson throws on invalid JSON', () => {
        expect(() => importThemeJson('{not json')).toThrow();
    });

    it('importThemeJson throws on schema violation', () => {
        expect(() => importThemeJson('{"id":"x","name":"","vars":{}}'))
            .toThrowError(/name must be 1\.\.60/);
    });
});
