// ── density-mode.ts — Workspace density controller ───────────────────────
//
// Drives `data-density` on <body>. Three intent-based modes:
//   • focus    — reading mode, sidebars dimmed, chat-centered
//   • explore  — default, everything visible
//   • war-room — dashboard prominent, chat compacted, anomalies enlarged
//
// Modes are switched via:
//   - Manual: Ctrl+1 / Ctrl+2 / Ctrl+3
//   - Auto-suggest: anomaly detected → suggest war-room (user can accept/dismiss)
//   - Persisted in localStorage so the user's choice survives reload
//
// CSS in app.css consumes [data-density="..."] on body.

import { writable } from 'svelte/store';
import { safeGetLS, safeSetLSString } from '$lib/safe-ls';

export type DensityMode = 'focus' | 'explore' | 'war-room';

const LS_KEY = 'lucy_density_mode';

/** Reactive store, primarily for the StatusBar pill. */
export const densityMode = writable<DensityMode>('explore');

// ── v1.4.16 — Fine-grained density slider ─────────────────────────────────
// On top of the three intent modes, we expose a 0..1 fine-tune that the CSS
// reads via `--density-fine`. 0 = ultra-tight (war-room-ish padding),
// 1 = ultra-spacious (focus-ish padding). It's orthogonal to the mode:
// a war-room user can still pump the slider to give themselves more
// breathing room without losing their dashboard layout.
const LS_FINE_KEY = 'lucy_density_fine';
export const densityFine = writable<number>(0.5);
function applyFine(v: number): void {
    try {
        const clamped = Math.max(0, Math.min(1, v));
        document.documentElement.style.setProperty('--density-fine', clamped.toFixed(3));
        safeSetLSString(LS_FINE_KEY, String(clamped));
    } catch {}
}
export function setDensityFine(v: number): void {
    densityFine.set(v);
    applyFine(v);
}

/** Apply a mode to the DOM. Idempotent. */
function applyMode(mode: DensityMode): void {
    try {
        document.body.dataset.density = mode;
        safeSetLSString(LS_KEY, mode);
    } catch {}
}

/** Set the mode (also updates the store). */
export function setDensityMode(mode: DensityMode): void {
    densityMode.set(mode);
    applyMode(mode);
}

/** Cycle through modes — useful for a toggle button. Order: explore → focus → war-room → explore */
export function cycleDensityMode(): DensityMode {
    let current: DensityMode = 'explore';
    densityMode.subscribe(v => current = v)();
    const next: DensityMode =
        current === 'explore'  ? 'focus' :
        current === 'focus'    ? 'war-room' :
                                 'explore';
    setDensityMode(next);
    return next;
}

/** Init — restores last mode, hooks Ctrl+1/2/3, returns stop fn. */
export function startDensityMode(): () => void {
    const saved = safeGetLS(LS_KEY, 'explore') as DensityMode;
    const valid: DensityMode = (['focus', 'explore', 'war-room'] as const).includes(saved as DensityMode) ? saved : 'explore';
    setDensityMode(valid);
    // v1.4.16 — restore fine slider.
    const savedFine = parseFloat(safeGetLS(LS_FINE_KEY, '0.5'));
    setDensityFine(Number.isFinite(savedFine) ? savedFine : 0.5);

    const keyHandler = (e: KeyboardEvent) => {
        // Ctrl+1/2/3 — works EVERYWHERE including the chat input.
        // Reason: 1/2/3 don't produce text when combined with Ctrl in any
        // useful way (it's not a known shortcut for the textarea/contenteditable),
        // so taking them globally is safe and matches the user's mental model
        // ("anywhere in the app, Ctrl+1 = focus mode").
        //
        // We DO require Ctrl exactly, with no Shift/Alt/Meta — so we don't
        // collide with Ctrl+Shift+1 etc. that other features might use.
        if (!e.ctrlKey || e.shiftKey || e.altKey || e.metaKey) return;

        if (e.key === '1') { e.preventDefault(); e.stopPropagation(); setDensityMode('focus'); }
        else if (e.key === '2') { e.preventDefault(); e.stopPropagation(); setDensityMode('explore'); }
        else if (e.key === '3') { e.preventDefault(); e.stopPropagation(); setDensityMode('war-room'); }
    };
    // Use capture phase so we run BEFORE any component's own keydown handler.
    // Without `capture: true`, the chat input's keydown could swallow the event
    // (e.g. if any wrapper calls preventDefault on bubble).
    document.addEventListener('keydown', keyHandler, { capture: true });
    return () => document.removeEventListener('keydown', keyHandler, { capture: true } as any);
}

/** Helper for the auto-suggest: returns true if mode should be war-room. */
export function shouldSuggestWarRoom(activeIncidentCount: number, recentAnomalyCount: number): boolean {
    return activeIncidentCount > 0 || recentAnomalyCount >= 2;
}
