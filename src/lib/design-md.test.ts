import { describe, it, expect } from 'vitest';
import {
    parseDesignMd,
    lintDesignTokens,
    formatTokensForPrompt,
} from './design-md';

const SAMPLE = `---
name: Heritage
colors:
  primary: "#1A1C1E"
  secondary: "#6C7278"
typography:
  h1: { fontFamily: Public Sans, fontSize: 3rem }
  body-md: { fontFamily: Public Sans, fontSize: 1rem }
spacing: { sm: 8px, md: 16px }
rounded: { md: 8px }
---

## Overview
Heritage is a premium architectural minimalism palette inspired by
high-end broadsheets and contemporary galleries. The single accent
color drives interaction across the entire UI.
`;

describe('design-md / parseDesignMd', () => {
    it('returns empty tokens + raw prose when no front-matter', () => {
        const r = parseDesignMd('# just markdown');
        expect(r.tokens).toEqual({});
        expect(r.prose).toBe('# just markdown');
    });

    it('parses scalars, nested objects, and inline flow maps', () => {
        const r = parseDesignMd(SAMPLE);
        expect(r.tokens.name).toBe('Heritage');
        expect((r.tokens.colors as any).primary).toBe('#1A1C1E');
        expect((r.tokens.typography as any).h1.fontFamily).toBe('Public Sans');
        expect((r.tokens.spacing as any).sm).toBe('8px');
        expect((r.tokens.rounded as any).md).toBe('8px');
    });

    it('extracts prose section after front-matter', () => {
        const r = parseDesignMd(SAMPLE);
        expect(r.prose).toMatch(/^## Overview/);
    });

    it('warns on missing recommended keys', () => {
        const r = parseDesignMd('---\nname: x\n---\nbody');
        const ws = r.warnings.join(' ');
        expect(ws).toMatch(/colors/i);
    });

    it('does not throw on malformed front-matter', () => {
        const r = parseDesignMd('---\nthis line has no colon\n---\nprose');
        expect(Array.isArray(r.errors)).toBe(true);
        expect(r.tokens).toBeDefined();
    });

    it('handles empty input gracefully', () => {
        const r = parseDesignMd('');
        expect(r.tokens).toEqual({});
        expect(r.prose).toBe('');
        expect(r.errors.length).toBeGreaterThan(0);
    });
});

describe('design-md / lintDesignTokens', () => {
    it('flags non-hex color values', () => {
        const r = parseDesignMd(`---
name: Bad
colors:
  primary: red
---
prose prose prose prose prose prose prose prose prose prose prose
`);
        const findings = lintDesignTokens(r);
        const colorErr = findings.find(f => f.code === 'color' && f.severity === 'error');
        expect(colorErr).toBeDefined();
    });

    it('warns on spacing without unit suffix', () => {
        const r = parseDesignMd(`---
name: Bad
colors:
  primary: "#000000"
spacing: { sm: 8 }
---
prose prose prose prose prose prose prose prose prose prose prose
`);
        const findings = lintDesignTokens(r);
        expect(findings.find(f => f.code === 'spacing')).toBeDefined();
    });
});

describe('design-md / formatTokensForPrompt', () => {
    it('returns empty string for empty tokens', () => {
        expect(formatTokensForPrompt({ tokens: {}, prose: '', errors: [], warnings: [] })).toBe('');
    });

    it('renders a compact agent-friendly block', () => {
        const r = parseDesignMd(SAMPLE);
        const out = formatTokensForPrompt(r);
        expect(out).toMatch(/DESIGN TOKENS/);
        expect(out).toMatch(/Heritage/);
        expect(out).toMatch(/primary/);
        expect(out).toMatch(/Public Sans/);
        expect(out).toMatch(/USE these exact tokens/);
    });
});
