// ── time-of-day.ts — Circadian theme nudger ──────────────────────────────
//
// Sets a `data-tod` attribute on <html> driven by the user's local clock.
// CSS hooks consume this to apply subtle hue/saturation/contrast shifts.
//
// Design:
//   • Pure observational hook — never asks user permission, never sends data
//   • Updates every 15 min (covers boundary transitions) + on visibility change
//   • Respects prefers-reduced-motion (CSS does — the JS just sets the attr)
//   • Defaults to 'day' if anything fails (safe neutral)
//
// Why a function, not a Svelte store: this is set-and-forget. No reactivity
// downstream needs to read the band — it's purely CSS-driven.

export type ToDBand = 'morning' | 'day' | 'afternoon' | 'evening' | 'night';

/** Map an hour (0-23) to its circadian band. */
export function bandForHour(hour: number): ToDBand {
    if (hour < 6 || hour >= 22) return 'night';
    if (hour < 11)              return 'morning';
    if (hour < 17)              return 'day';
    if (hour < 19)              return 'afternoon';
    return 'evening';
}

/** Apply the current band to <html>. Idempotent. */
function applyCurrentBand(): ToDBand {
    try {
        const h = new Date().getHours();
        const band = bandForHour(h);
        const root = document.documentElement;
        if (root.dataset.tod !== band) {
            root.dataset.tod = band;
        }
        return band;
    } catch {
        return 'day';
    }
}

let _intervalId: ReturnType<typeof setInterval> | null = null;
let _visListener: (() => void) | null = null;

/** Start the circadian nudger. Returns a stop fn. Call once per app lifetime. */
export function startTimeOfDay(): () => void {
    applyCurrentBand();
    // Every 15 minutes — covers any band transition within ~7.5min of arrival
    _intervalId = setInterval(applyCurrentBand, 15 * 60 * 1000);
    // Recheck whenever the window comes back from hidden (suspended laptop case)
    _visListener = () => { if (document.visibilityState === 'visible') applyCurrentBand(); };
    document.addEventListener('visibilitychange', _visListener);
    return stopTimeOfDay;
}

export function stopTimeOfDay(): void {
    if (_intervalId) { clearInterval(_intervalId); _intervalId = null; }
    if (_visListener) { document.removeEventListener('visibilitychange', _visListener); _visListener = null; }
}
