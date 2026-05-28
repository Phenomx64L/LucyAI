// ── theme-loader.ts — Tier B #3 (JSON theming system) ─────────────────────
//
// Lets users define + share themes as JSON, no CSS editing required.
//
// Why JSON instead of full CSS: themes only need to change a small set of
// CSS variables (--bg-top, --bg-mid, --bg-bottom, --sidebar-overlay, accent
// colors). Restricting the surface to JSON means:
//   • Themes are inert data — can't inject malicious selectors or @import
//   • Easy to validate, easy to share via copy-paste
//   • The whole "theme exporer" surface is ~150 LOC instead of 1500
//
// Storage: localStorage('lucy_custom_themes') as a JSON array of CustomTheme.
// Injected into the DOM via a single <style id="lucy-custom-themes"> tag
// that's regenerated when the registry changes.

import { safeGetLS, safeSetLSString } from '$lib/safe-ls';

/** Allowed CSS variables a theme can override. Whitelisted so themes
 *  can't smuggle in arbitrary CSS that would alter layout. */
const ALLOWED_VARS = [
    '--bg-top', '--bg-mid', '--bg-bottom',
    '--sidebar-overlay',
    '--accent', '--accent-dim', '--accent-border',
    '--text-main', '--text-muted',
] as const;
type AllowedVar = typeof ALLOWED_VARS[number];

/** Hex / rgb / rgba / hsl(a) — anything else is rejected. */
const COLOR_RE = /^(#[0-9a-fA-F]{3,8}|rgba?\([^)]+\)|hsla?\([^)]+\))\s*$/;

export interface CustomTheme {
    /** Unique slug, used as data-theme="custom-<id>". Lowercase + dashes only. */
    id: string;
    /** Human-readable name shown in the picker. */
    name: string;
    /** Subset of ALLOWED_VARS → color value. Missing vars inherit defaults. */
    vars: Partial<Record<AllowedVar, string>>;
}

const STORAGE_KEY = 'lucy_custom_themes';
const STYLE_ID    = 'lucy-custom-themes';

/** Read the registry from localStorage. Defensive — never throws. */
export function listCustomThemes(): CustomTheme[] {
    const raw = safeGetLS(STORAGE_KEY, '[]');
    try {
        const parsed = JSON.parse(raw);
        if (!Array.isArray(parsed)) return [];
        return parsed.filter(isValidTheme);
    } catch {
        return [];
    }
}

/** Persist the registry and re-inject the <style> tag. */
export function saveCustomThemes(themes: CustomTheme[]): void {
    safeSetLSString(STORAGE_KEY, JSON.stringify(themes));
    injectStyleTag(themes);
}

/**
 * Add or replace a theme by id. Returns the resulting registry.
 *
 * Validation:
 *   • id matches /^[a-z0-9-]+$/ (slug-safe)
 *   • name 1..60 chars
 *   • each var is in ALLOWED_VARS
 *   • each value matches COLOR_RE
 * Throws on invalid input — the UI should catch and surface the error.
 */
export function upsertCustomTheme(theme: CustomTheme): CustomTheme[] {
    const sanitized = validateOrThrow(theme);
    const all = listCustomThemes();
    const idx = all.findIndex(t => t.id === sanitized.id);
    if (idx >= 0) all[idx] = sanitized;
    else all.push(sanitized);
    saveCustomThemes(all);
    return all;
}

export function deleteCustomTheme(id: string): CustomTheme[] {
    const all = listCustomThemes().filter(t => t.id !== id);
    saveCustomThemes(all);
    return all;
}

/** Inject (or update) the <style> tag that holds all custom themes.
 *  Called on every save and once at app start. */
export function injectStyleTag(themes?: CustomTheme[]): void {
    if (typeof document === 'undefined') return; // SSR / vitest node env
    const list = themes ?? listCustomThemes();
    const css = list.map(themeToCss).join('\n');
    let el = document.getElementById(STYLE_ID) as HTMLStyleElement | null;
    if (!el) {
        el = document.createElement('style');
        el.id = STYLE_ID;
        document.head.appendChild(el);
    }
    el.textContent = css;
}

/** Convert one CustomTheme to a `[data-theme="custom-<id>"] { ... }` block. */
function themeToCss(t: CustomTheme): string {
    const lines: string[] = [];
    for (const [k, v] of Object.entries(t.vars)) {
        if (!ALLOWED_VARS.includes(k as AllowedVar)) continue;
        if (typeof v !== 'string' || !COLOR_RE.test(v)) continue;
        lines.push(`  ${k}: ${v};`);
    }
    return `[data-theme="custom-${t.id}"] {\n${lines.join('\n')}\n}`;
}

/** Bootstrap from localStorage at app start. Call once. */
export function bootCustomThemes(): void {
    injectStyleTag();
}

// ── Validation ────────────────────────────────────────────────────────────
function isValidTheme(x: unknown): x is CustomTheme {
    try { validateOrThrow(x as CustomTheme); return true; }
    catch { return false; }
}

export function validateOrThrow(t: CustomTheme): CustomTheme {
    if (!t || typeof t !== 'object') throw new Error('theme must be an object');
    if (typeof t.id !== 'string' || !/^[a-z0-9-]{1,40}$/.test(t.id)) {
        throw new Error('theme.id must match /^[a-z0-9-]{1,40}$/');
    }
    if (typeof t.name !== 'string' || t.name.length === 0 || t.name.length > 60) {
        throw new Error('theme.name must be 1..60 chars');
    }
    if (!t.vars || typeof t.vars !== 'object') {
        throw new Error('theme.vars must be an object');
    }
    const cleanedVars: Partial<Record<AllowedVar, string>> = {};
    for (const [k, v] of Object.entries(t.vars)) {
        if (!ALLOWED_VARS.includes(k as AllowedVar)) {
            throw new Error(`theme.vars contains disallowed variable: ${k}`);
        }
        if (typeof v !== 'string' || !COLOR_RE.test(v)) {
            throw new Error(`theme.vars.${k} is not a valid color (hex/rgb/rgba/hsl)`);
        }
        cleanedVars[k as AllowedVar] = v;
    }
    return { id: t.id, name: t.name, vars: cleanedVars };
}

/** Convert one theme to a downloadable JSON string. */
export function exportThemeJson(t: CustomTheme): string {
    return JSON.stringify(t, null, 2);
}

/** Parse a JSON string into a validated theme. Throws on invalid. */
export function importThemeJson(json: string): CustomTheme {
    const parsed = JSON.parse(json);
    return validateOrThrow(parsed);
}
