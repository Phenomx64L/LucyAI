// ── design-md — parse + lint a DESIGN.md spec for the visual identity
//
// Implements (a subset of) the @google/design.md spec used to teach a
// coding agent the visual identity of a project: design tokens in YAML
// front-matter + human prose explaining the *why*.
//
//   ---
//   name: Heritage
//   colors:
//     primary:   "#1A1C1E"
//     secondary: "#6C7278"
//   typography:
//     h1: { fontFamily: Public Sans, fontSize: 3rem }
//   rounded: { sm: 4px, md: 8px }
//   spacing: { sm: 8px, md: 16px }
//   ---
//
//   ## Overview
//   ...prose...
//
// Lucy uses this in two complementary ways:
//   • Lucy HAS its own DESIGN.md (committed at repo root) so the agent
//     respects its own identity when generating Svelte/CSS.
//   • Lucy READS the user project's DESIGN.md when working in their
//     repo, and injects the tokens into the system prompt so generated
//     code uses their colors / fonts / spacing — not invented ones.

export interface DesignTokens {
    name?: string;
    colors?:     Record<string, string>;
    typography?: Record<string, Record<string, string | number>>;
    rounded?:    Record<string, string>;
    spacing?:    Record<string, string>;
    /** Anything else we don't model strongly — passed through. */
    [key: string]: any;
}

export interface ParsedDesign {
    /** YAML front-matter, parsed to an object. Empty if no front-matter. */
    tokens: DesignTokens;
    /** Markdown prose (everything AFTER the closing `---`). */
    prose:  string;
    /** Hard parse errors (unknown YAML lines, malformed structure). */
    errors: string[];
    /** Soft warnings (e.g. duplicate keys, missing recommended sections). */
    warnings: string[];
}

const FRONT_MATTER_RE = /^---\s*\n([\s\S]*?)\n---\s*\n?/;

/**
 * Parse a DESIGN.md source string. Best-effort — never throws.
 * Returns an empty tokens block + prose=raw text if no front-matter found.
 *
 * NOTE: this is a deliberately small YAML reader covering the subset the
 * spec uses (scalars, simple key:value, nested 1-level objects, inline
 * `{}` flow maps). It does NOT pull a full YAML dependency — keeps Lucy's
 * bundle lean.
 */
export function parseDesignMd(source: string): ParsedDesign {
    const errors:   string[] = [];
    const warnings: string[] = [];
    if (!source || typeof source !== 'string') {
        return { tokens: {}, prose: '', errors: ['empty source'], warnings: [] };
    }
    const m = source.match(FRONT_MATTER_RE);
    if (!m) {
        return { tokens: {}, prose: source, errors: [], warnings: ['No YAML front-matter found'] };
    }
    const yaml  = m[1];
    const prose = source.slice(m[0].length).trimStart();
    const tokens = _parseSimpleYaml(yaml, errors, warnings);
    if (!tokens.name)   warnings.push('Missing recommended `name` field');
    if (!tokens.colors) warnings.push('Missing recommended `colors` block');
    return { tokens, prose, errors, warnings };
}

/**
 * Tiny indent-aware YAML reader. Supports:
 *   key: value
 *   key: { a: 1, b: "x" }
 *   key:
 *     subkey: value
 * Strings may be bare, single- or double-quoted. Numbers parsed when bare.
 */
function _parseSimpleYaml(src: string, errors: string[], warnings: string[]): DesignTokens {
    const out: DesignTokens = {};
    const lines = src.split(/\r?\n/);
    let currentKey: string | null = null;
    let currentObj: Record<string, any> | null = null;

    for (let lineNo = 0; lineNo < lines.length; lineNo++) {
        const raw = lines[lineNo];
        if (!raw.trim() || raw.trim().startsWith('#')) continue;
        const indent = raw.match(/^( *)/)![1].length;

        // Top-level key
        if (indent === 0) {
            currentKey = null; currentObj = null;
            const idx = raw.indexOf(':');
            if (idx === -1) { errors.push(`Line ${lineNo + 1}: missing ':'`); continue; }
            const key = raw.slice(0, idx).trim();
            const val = raw.slice(idx + 1).trim();
            if (val === '' ) {
                // Block follows
                currentKey = key;
                currentObj = {};
                out[key] = currentObj;
            } else if (val.startsWith('{') && val.endsWith('}')) {
                out[key] = _parseFlowMap(val, lineNo + 1, errors);
            } else {
                out[key] = _coerce(val);
            }
            continue;
        }
        // Nested under current top-level key
        if (currentObj && indent >= 2) {
            const stripped = raw.trim();
            const idx = stripped.indexOf(':');
            if (idx === -1) { errors.push(`Line ${lineNo + 1}: missing ':' under ${currentKey}`); continue; }
            const key = stripped.slice(0, idx).trim();
            const val = stripped.slice(idx + 1).trim();
            if (val === '') {
                // 2-level nested block
                const sub: Record<string, any> = {};
                currentObj[key] = sub;
                // Look ahead for indented children
                while (lineNo + 1 < lines.length) {
                    const next = lines[lineNo + 1];
                    if (!next.trim() || next.trim().startsWith('#')) { lineNo++; continue; }
                    const nIndent = next.match(/^( *)/)![1].length;
                    if (nIndent <= indent) break;
                    const nStripped = next.trim();
                    const nIdx = nStripped.indexOf(':');
                    if (nIdx === -1) { errors.push(`Line ${lineNo + 2}: malformed nested entry`); lineNo++; continue; }
                    const nKey = nStripped.slice(0, nIdx).trim();
                    const nVal = nStripped.slice(nIdx + 1).trim();
                    sub[nKey] = nVal.startsWith('{') && nVal.endsWith('}')
                        ? _parseFlowMap(nVal, lineNo + 2, errors)
                        : _coerce(nVal);
                    lineNo++;
                }
            } else if (val.startsWith('{') && val.endsWith('}')) {
                currentObj[key] = _parseFlowMap(val, lineNo + 1, errors);
            } else {
                currentObj[key] = _coerce(val);
            }
            continue;
        }
        warnings.push(`Line ${lineNo + 1}: skipped (unexpected indent)`);
    }
    return out;
}

function _parseFlowMap(s: string, lineNo: number, errors: string[]): Record<string, any> {
    const inside = s.slice(1, -1).trim();
    const out: Record<string, any> = {};
    if (!inside) return out;
    // Naive split on top-level commas — good enough for design tokens, which
    // don't nest flow maps. Quoted strings are respected.
    const parts: string[] = [];
    let buf = '';
    let inQ: string | null = null;
    for (const ch of inside) {
        if (inQ) {
            buf += ch;
            if (ch === inQ) inQ = null;
        } else if (ch === '"' || ch === "'") {
            inQ = ch; buf += ch;
        } else if (ch === ',') {
            parts.push(buf); buf = '';
        } else {
            buf += ch;
        }
    }
    if (buf.trim()) parts.push(buf);
    for (const p of parts) {
        const idx = p.indexOf(':');
        if (idx === -1) { errors.push(`Line ${lineNo}: malformed flow entry "${p.trim()}"`); continue; }
        const k = p.slice(0, idx).trim();
        const v = p.slice(idx + 1).trim();
        out[k] = _coerce(v);
    }
    return out;
}

function _coerce(raw: string): string | number | boolean | null {
    const v = raw.trim();
    if (!v) return '';
    // Quoted string
    if ((v.startsWith('"') && v.endsWith('"')) || (v.startsWith("'") && v.endsWith("'"))) {
        return v.slice(1, -1);
    }
    if (v === 'null') return null;
    if (v === 'true') return true;
    if (v === 'false') return false;
    if (/^-?\d+(\.\d+)?$/.test(v)) return Number(v);
    return v;
}

// ── Lint pass ────────────────────────────────────────────────────────────

export interface LintFinding {
    severity: 'error' | 'warning' | 'info';
    code: string;
    message: string;
}

/**
 * Quick sanity lint over parsed tokens. Returns a list of findings the UI
 * (or the agent) can render. Doesn't compute WCAG contrast yet (would
 * require the canvas API — punt to a future pass).
 */
export function lintDesignTokens(parsed: ParsedDesign): LintFinding[] {
    const out: LintFinding[] = [];
    for (const e of parsed.errors)   out.push({ severity: 'error',   code: 'parse',   message: e });
    for (const w of parsed.warnings) out.push({ severity: 'warning', code: 'schema',  message: w });

    const colors = parsed.tokens.colors;
    if (colors && typeof colors === 'object') {
        for (const [name, hex] of Object.entries(colors)) {
            if (typeof hex !== 'string' || !/^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(hex)) {
                out.push({ severity: 'error', code: 'color', message: `colors.${name}: not a hex color: ${JSON.stringify(hex)}` });
            }
        }
    }
    const spacing = parsed.tokens.spacing;
    if (spacing && typeof spacing === 'object') {
        for (const [name, val] of Object.entries(spacing)) {
            if (typeof val !== 'string' || !/^\d+(px|rem|em|%)$/.test(val)) {
                out.push({ severity: 'warning', code: 'spacing', message: `spacing.${name}: prefer "Npx" or "Nrem", got: ${JSON.stringify(val)}` });
            }
        }
    }
    if (!parsed.prose || parsed.prose.length < 40) {
        out.push({ severity: 'info', code: 'prose', message: 'Front-matter is fine but the prose section is short — agents benefit from "why" rationale.' });
    }
    return out;
}

// ── Agent-prompt formatter ──────────────────────────────────────────────

/**
 * Render a compact agent-friendly summary of the parsed tokens. This is what
 * gets injected into Lucy's system prompt when DESIGN.md is detected so the
 * generated code respects the project's identity.
 *
 * Output format:
 *   --- DESIGN TOKENS (DESIGN.md detected) ---
 *   name: Heritage
 *   colors:
 *     - primary: #1A1C1E
 *     - secondary: #6C7278
 *   typography:
 *     - h1: Public Sans / 3rem
 *   spacing: sm=8px, md=16px
 *   rationale: <first 400 chars of prose>
 */
export function formatTokensForPrompt(parsed: ParsedDesign): string {
    if (!parsed.tokens || Object.keys(parsed.tokens).length === 0) return '';
    const lines: string[] = ['--- DESIGN TOKENS (DESIGN.md detected) ---'];
    if (parsed.tokens.name) lines.push(`name: ${parsed.tokens.name}`);
    const colors = parsed.tokens.colors;
    if (colors && typeof colors === 'object') {
        lines.push('colors:');
        for (const [k, v] of Object.entries(colors)) lines.push(`  - ${k}: ${v}`);
    }
    const typo = parsed.tokens.typography;
    if (typo && typeof typo === 'object') {
        lines.push('typography:');
        for (const [k, v] of Object.entries(typo)) {
            if (v && typeof v === 'object') {
                const f = (v as any).fontFamily ?? '?';
                const s = (v as any).fontSize   ?? '?';
                lines.push(`  - ${k}: ${f} / ${s}`);
            }
        }
    }
    const spacing = parsed.tokens.spacing;
    if (spacing && typeof spacing === 'object') {
        const pairs = Object.entries(spacing).map(([k, v]) => `${k}=${v}`).join(', ');
        lines.push(`spacing: ${pairs}`);
    }
    const rounded = parsed.tokens.rounded;
    if (rounded && typeof rounded === 'object') {
        const pairs = Object.entries(rounded).map(([k, v]) => `${k}=${v}`).join(', ');
        lines.push(`rounded: ${pairs}`);
    }
    if (parsed.prose && parsed.prose.length > 0) {
        const rationale = parsed.prose.slice(0, 400).replace(/\s+/g, ' ').trim();
        lines.push(`rationale: ${rationale}${parsed.prose.length > 400 ? '…' : ''}`);
    }
    lines.push('(When generating UI/CSS code, USE these exact tokens. Do not invent colors, fonts or spacing.)');
    return lines.join('\n');
}
