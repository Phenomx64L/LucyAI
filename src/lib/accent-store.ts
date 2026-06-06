// accent-store.ts — v1.7.98 (Option D5)
//
// Lightweight overlay on top of the existing 11-theme warp system.
// Where a "warp theme" (default, ocean, hacker, sunset, …) controls the
// gradient backdrop, an "accent" controls just the primary action hue:
// --accent + --accent-dim + --accent-border + --accent-glow. The two
// dimensions are orthogonal so an operator can keep their preferred
// dark backdrop AND express a personal accent.
//
// Persistence: localStorage 'lucy_accent_id'. Default is 'emerald'
// (matches the historical green identity).
//
// Design rules:
//   • Six presets only. Custom hue picker would add a settings surface
//     we don't need right now and the named swatches read better.
//   • Each preset writes the four --accent* vars on document.documentElement
//     inline. That cascades over the :root defaults in app.css without
//     needing a CSS rebuild.
//   • Hardcoded rgba(16,185,129,…) literals scattered through the
//     codebase will NOT shift — they keep their green identity by
//     design. The accent system targets the "primary brand" surface
//     (input border, send button, sidebar active row, etc.).

export type AccentId = 'emerald' | 'cyan' | 'violet' | 'amber' | 'pink' | 'sky';

export interface AccentDef {
    id: AccentId;
    label: string;
    /** Base accent color — HEX so cite-chip etc. that read it as a string keep working. */
    accent: string;
    /** Slight transparency variant for background tints. */
    dim: string;
    /** Stronger transparency for hairline borders. */
    border: string;
    /** Multi-stop box-shadow string for "glow" treatments. */
    glow: string;
}

export const ACCENT_PRESETS: AccentDef[] = [
    {
        id: 'emerald',
        label: 'Emerald',
        accent: '#10b981',
        dim:    'rgba(16, 185, 129, 0.10)',
        border: 'rgba(16, 185, 129, 0.18)',
        glow:   '0 0 8px rgba(16, 185, 129, 0.25), 0 0 20px rgba(16, 185, 129, 0.08)',
    },
    {
        id: 'cyan',
        label: 'Cyan',
        accent: '#22d3ee',
        dim:    'rgba(34, 211, 238, 0.10)',
        border: 'rgba(34, 211, 238, 0.20)',
        glow:   '0 0 8px rgba(34, 211, 238, 0.28), 0 0 20px rgba(34, 211, 238, 0.10)',
    },
    {
        id: 'violet',
        label: 'Violet',
        accent: '#a78bfa',
        dim:    'rgba(167, 139, 250, 0.12)',
        border: 'rgba(167, 139, 250, 0.22)',
        glow:   '0 0 8px rgba(167, 139, 250, 0.30), 0 0 20px rgba(167, 139, 250, 0.10)',
    },
    {
        id: 'amber',
        label: 'Amber',
        accent: '#f59e0b',
        dim:    'rgba(245, 158, 11, 0.12)',
        border: 'rgba(245, 158, 11, 0.22)',
        glow:   '0 0 8px rgba(245, 158, 11, 0.30), 0 0 20px rgba(245, 158, 11, 0.10)',
    },
    {
        id: 'pink',
        label: 'Pink',
        accent: '#f472b6',
        dim:    'rgba(244, 114, 182, 0.12)',
        border: 'rgba(244, 114, 182, 0.22)',
        glow:   '0 0 8px rgba(244, 114, 182, 0.30), 0 0 20px rgba(244, 114, 182, 0.10)',
    },
    {
        id: 'sky',
        label: 'Sky',
        accent: '#3b9eff',
        dim:    'rgba(59, 158, 255, 0.12)',
        border: 'rgba(59, 158, 255, 0.22)',
        glow:   '0 0 8px rgba(59, 158, 255, 0.30), 0 0 20px rgba(59, 158, 255, 0.10)',
    },
];

const LS_KEY = 'lucy_accent_id';
const DEFAULT_ID: AccentId = 'emerald';

/** Resolve a stored id to an AccentDef. Falls back to emerald for any
 *  unknown id (forward-compat: if we add an id and the user downgrades
 *  the build, the deserialized id is ignored gracefully). */
export function getAccent(id: AccentId): AccentDef {
    return ACCENT_PRESETS.find(a => a.id === id) || ACCENT_PRESETS[0];
}

/** Read the persisted accent id from localStorage. Safe under SSR /
 *  no-localStorage environments — returns the default in that case. */
export function readAccentId(): AccentId {
    try {
        const raw = localStorage.getItem(LS_KEY);
        const found = ACCENT_PRESETS.find(a => a.id === raw);
        return found ? found.id : DEFAULT_ID;
    } catch {
        return DEFAULT_ID;
    }
}

/** Write the four --accent* CSS variables onto <html> and persist the
 *  id. Calling this with the same id twice is idempotent. */
export function applyAccent(id: AccentId): AccentDef {
    const def = getAccent(id);
    if (typeof document !== 'undefined') {
        const root = document.documentElement;
        root.style.setProperty('--accent',        def.accent);
        root.style.setProperty('--accent-dim',    def.dim);
        root.style.setProperty('--accent-border', def.border);
        root.style.setProperty('--accent-glow',   def.glow);
        root.setAttribute('data-accent', def.id);
    }
    try { localStorage.setItem(LS_KEY, id); } catch { /* persistence is best-effort */ }
    return def;
}

/** Initialize accent on app boot. Call once from onMount in +page.svelte. */
export function initAccent(): AccentId {
    const id = readAccentId();
    applyAccent(id);
    return id;
}
