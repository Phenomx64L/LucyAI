// ── poll.ts — visibility-gated polling (v1.7.174) ────────────────────────────
//
// Lucy runs several background telemetry polls (prompt-cache stats, monthly
// cost, host health …). A plain `setInterval` keeps firing — and keeps paying
// the Tauri IPC boundary + a SQLite hit on the Rust side — even when the window
// is hidden behind another app, where nobody can see the result. That work
// competes with foreground rendering and burns CPU/battery for nothing.
//
// `gatedInterval()` runs `fn` ONLY while the document is visible. When the
// window is hidden the timer keeps ticking (that cost is negligible) but the
// body — the expensive IPC call — is skipped. On regaining visibility it fires
// `fn` once immediately so the just-revealed UI shows fresh data instead of
// waiting up to a full interval.
//
// The `*-loop.ts` scheduler modules already inline this `visibilityState`
// check; this helper centralises the same pattern for the component-level
// pollers (StatusBar, dashboards, …) so they don't each reinvent it.

export interface GatedIntervalOpts {
    /** Fire fn() immediately when the window regains visibility. Default true. */
    refreshOnVisible?: boolean;
    /** Fire fn() once right away on start (like a leading edge). Default false. */
    immediate?: boolean;
}

/**
 * setInterval that only invokes `fn` while the tab/window is visible.
 * Returns a stop function that clears the timer AND unbinds the
 * visibilitychange listener — call it from the component's onDestroy.
 */
export function gatedInterval(
    fn: () => void,
    ms: number,
    opts: GatedIntervalOpts = {},
): () => void {
    const { refreshOnVisible = true, immediate = false } = opts;

    const visible = () =>
        typeof document === 'undefined' || document.visibilityState === 'visible';

    if (immediate && visible()) fn();

    const timer = setInterval(() => { if (visible()) fn(); }, ms);

    let onVis: (() => void) | null = null;
    if (refreshOnVisible && typeof document !== 'undefined') {
        onVis = () => { if (document.visibilityState === 'visible') fn(); };
        document.addEventListener('visibilitychange', onVis, { passive: true });
    }

    return () => {
        clearInterval(timer);
        if (onVis) document.removeEventListener('visibilitychange', onVis);
    };
}
