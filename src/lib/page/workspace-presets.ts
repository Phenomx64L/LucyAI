// ── page/workspace-presets.ts ────────────────────────────────────────────
//
// Workspace preset helpers extracted from `+page.svelte` (Sprint D refactor).
// A "preset" is a quick-restore snapshot of the user's UI state — model in
// the active tab, theme, density, personality, view, sidebar/focus toggles,
// and a thin per-tab snapshot (title + model only — never message history,
// which would explode localStorage).
//
// All persistence goes through `safeSetLS` so corrupted/quota-exceeded
// localStorage failures degrade silently instead of crashing the page.

import { safeSetLS, safeSetLSString } from '$lib/safe-ls';

/** Persistence schema (v2). v1 lacked the `view/sidebarCollapsed/focusMode/
 *  tabs/lang` fields — applyPreset gracefully accepts older shapes. */
export interface WorkspacePreset {
    v: number;
    name: string;
    model: string;
    theme: string;
    density: 'comfortable' | 'compact' | string;
    personality?: string;
    view?: string;
    sidebarCollapsed?: boolean;
    focusMode?: boolean;
    tabs?: Array<{ title: string; model: string }>;
    lang?: string;
    ts: number;
    lastApplied: number | null;
}

/** Minimal subset of the page's reactive surface that the preset helpers
 *  need to read/write. `setView` is the page's view-switch fn (kept opaque). */
export interface PresetContext {
    presets: WorkspacePreset[];
    activeModel: string;
    theme: string;
    density: string;
    personality: string;
    view: string;
    sidebarCollapsed: boolean;
    focusMode: boolean;
    userLang: string;
    tabsSnapshot: Array<{ title: string; model: string }>;
}

/** Build a new preset payload from the current UI state. */
export function buildPreset(name: string, ctx: PresetContext): WorkspacePreset {
    return {
        v: 2,
        name: name.trim(),
        model: ctx.activeModel || 'gemini-3.1-flash-lite',
        theme: ctx.theme,
        density: ctx.density,
        personality: ctx.personality,
        view: ctx.view,
        sidebarCollapsed: !!ctx.sidebarCollapsed,
        focusMode: !!ctx.focusMode,
        tabs: ctx.tabsSnapshot,
        lang: ctx.userLang,
        ts: Date.now(),
        lastApplied: null,
    };
}

/** Upsert a preset by name (replaces if `name` already exists). */
export function upsertPreset(
    presets: WorkspacePreset[],
    preset: WorkspacePreset,
): WorkspacePreset[] {
    const next = [...presets.filter(p => p.name !== preset.name), preset];
    safeSetLS('lucy_presets', next);
    return next;
}

/** Remove a preset by name. */
export function deletePreset(
    presets: WorkspacePreset[],
    name: string,
): WorkspacePreset[] {
    const next = presets.filter(p => p.name !== name);
    safeSetLS('lucy_presets', next);
    return next;
}

/** Stamp `lastApplied` on the named preset and persist. */
export function stampApplied(
    presets: WorkspacePreset[],
    name: string,
): WorkspacePreset[] {
    const now = Date.now();
    const next = presets.map(x => x.name === name ? { ...x, lastApplied: now } : x);
    safeSetLS('lucy_presets', next);
    return next;
}

/** Apply preset side-effects that DON'T touch reactive Svelte state.
 *  Returns the patches the caller must apply to its own state. */
export function presetPatches(p: WorkspacePreset, currentLang: string) {
    return {
        theme:     p.theme,
        density:   p.density || 'comfortable',
        personality: p.personality,
        view:      p.v >= 2 ? p.view : undefined,
        sidebarCollapsed: p.v >= 2 ? p.sidebarCollapsed : undefined,
        focusMode: p.v >= 2 ? p.focusMode : undefined,
        lang:      (p.v >= 2 && p.lang && p.lang !== currentLang) ? p.lang : undefined,
    };
}

/** Persist scalar fields after `presetPatches` is applied. */
export function persistPresetScalars(theme: string, density: string, personality?: string, lang?: string): void {
    safeSetLSString('lucy_warp_theme', theme);
    safeSetLSString('lucy_density', density);
    if (personality) safeSetLSString('lucy_personality', personality);
    if (lang)        safeSetLSString('lucy_user_lang', lang);
}

/** Pretty relative-time formatter for preset cards. */
export function ageString(ts: number | null | undefined, isEN: boolean, userLang: string): string {
    if (!ts) return '';
    const diff = Date.now() - ts;
    const m = Math.round(diff / 60000);
    if (m < 1)  return isEN ? 'just now' : 'ahora';
    if (m < 60) return `${m}${isEN ? 'm ago' : 'm'}`;
    const h = Math.round(m / 60);
    if (h < 24) return `${h}${isEN ? 'h ago' : 'h'}`;
    const d = Math.round(h / 24);
    if (d < 30) return `${d}${isEN ? 'd ago' : 'd'}`;
    return new Date(ts).toLocaleDateString(userLang);
}
