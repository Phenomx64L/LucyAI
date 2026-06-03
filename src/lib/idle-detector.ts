// ── idle-detector.ts — quiesce the UI when nobody's looking ─────────────────
//
// v1.7.44 — Goal: bring idle GPU usage close to zero without removing the
// living-cockpit feel during active use. Lucy's UI has dozens of `@keyframes
// ... infinite` rules (breathing avatars, pulsing LEDs, shimmering bars,
// sparkline updates). When the user is actively typing or reading, those
// animations are part of the product's character. When the user has
// stepped away from the keyboard, or the window is in the background, the
// GPU still pays the full per-frame cost of compositing every one of
// those layers — even though nobody can see the result.
//
// This module installs two listeners and exposes one entry point.
//
//   1. `visibilitychange` → toggles `html.app-hidden`
//      WebView2 keeps the page rendered when the host window is hidden,
//      minimised, or behind another window with the focus. The `.app-hidden`
//      CSS rule (in `routes/page.css`) sets `animation-play-state: paused`
//      on every element + pseudo-element, dropping GPU draw cost to
//      effectively zero. Existing comment in that file said the class was
//      "toggled from onMount via document.visibilitychange" but the actual
//      wiring was missing — so the rule never fired in practice.
//
//   2. Input activity → toggles `html.lucy-quiescent` with a debounce
//      `pointermove`, `pointerdown`, `keydown`, `wheel`, and `touchstart`
//      reset an inactivity timer. After IDLE_MS without input the
//      `lucy-quiescent` class is added; on any subsequent event it is
//      removed instantly. The CSS rule applies the same `animation-play-
//      state: paused` as `.app-hidden` so the two paths share the same
//      pause behaviour with no code duplication.
//
// The two paths are independent: `.app-hidden` covers "window not visible",
// `.lucy-quiescent` covers "window visible but user away". Both can be
// active simultaneously.
//
// Tunable: IDLE_MS defaults to 8 s — short enough that idle GPU drops
// quickly when the user steps away, long enough that brief reading
// pauses (skimming a long Lucy response) don't cause visible animation
// freezes. Override via `setIdleThreshold(ms)` if a future setting needs
// to expose this.

const IDLE_MS_DEFAULT = 8_000;

let _started        = false;
let _idleMs         = IDLE_MS_DEFAULT;
let _idleTimer: ReturnType<typeof setTimeout> | null = null;

/** Add or remove a class on `<html>` without thrashing the classList if
 *  the class is already in the desired state. Cheap, but the visibility
 *  listener fires on every tab switch so it's worth the guard. */
function setHtmlClass(name: string, on: boolean): void {
    const html = document.documentElement;
    const has  = html.classList.contains(name);
    if (on && !has)      html.classList.add(name);
    else if (!on && has) html.classList.remove(name);
}

function clearIdleTimer(): void {
    if (_idleTimer !== null) {
        clearTimeout(_idleTimer);
        _idleTimer = null;
    }
}

function armIdleTimer(): void {
    clearIdleTimer();
    _idleTimer = setTimeout(() => {
        // Don't engage quiescent mode if the window already paused via
        // visibility — `.app-hidden` already covers that case and the
        // dual class is redundant noise in DevTools.
        if (document.visibilityState === 'visible') {
            setHtmlClass('lucy-quiescent', true);
        }
        _idleTimer = null;
    }, _idleMs);
}

function onUserActivity(): void {
    setHtmlClass('lucy-quiescent', false);
    armIdleTimer();
}

function onVisibilityChange(): void {
    const hidden = document.visibilityState === 'hidden';
    setHtmlClass('app-hidden', hidden);
    if (hidden) {
        // Cancel the idle countdown — there's no point engaging quiescent
        // mode while the window is hidden, and we don't want a stale
        // timer to fire and add the class right as the user re-focuses
        // the window.
        clearIdleTimer();
    } else {
        // Window just became visible. Treat that as activity and start
        // a fresh idle countdown so the user gets a few seconds of full
        // animation before the quiescent class kicks back in.
        onUserActivity();
    }
}

/** Adjust the idle threshold. No-op until `startIdleDetector()` runs. */
export function setIdleThreshold(ms: number): void {
    _idleMs = Math.max(1_000, ms | 0);
    if (_started) armIdleTimer();
}

/** Idempotent — safe to call multiple times during HMR. */
export function startIdleDetector(): void {
    if (_started || typeof document === 'undefined') return;
    _started = true;

    // Initial state: align with whatever the document is doing right now.
    onVisibilityChange();
    armIdleTimer();

    document.addEventListener('visibilitychange', onVisibilityChange, { passive: true });

    // `passive: true` is critical — the browser can skip scroll-blocking
    // checks and the event lands faster. We never preventDefault here.
    const opts: AddEventListenerOptions = { passive: true, capture: true };
    window.addEventListener('pointermove', onUserActivity, opts);
    window.addEventListener('pointerdown', onUserActivity, opts);
    window.addEventListener('keydown',     onUserActivity, opts);
    window.addEventListener('wheel',       onUserActivity, opts);
    window.addEventListener('touchstart',  onUserActivity, opts);
}

/** Mostly for tests / HMR cleanup. */
export function stopIdleDetector(): void {
    if (!_started) return;
    _started = false;
    clearIdleTimer();
    document.removeEventListener('visibilitychange', onVisibilityChange);
    const opts: AddEventListenerOptions = { capture: true };
    window.removeEventListener('pointermove', onUserActivity, opts);
    window.removeEventListener('pointerdown', onUserActivity, opts);
    window.removeEventListener('keydown',     onUserActivity, opts);
    window.removeEventListener('wheel',       onUserActivity, opts);
    window.removeEventListener('touchstart',  onUserActivity, opts);
    setHtmlClass('app-hidden',     false);
    setHtmlClass('lucy-quiescent', false);
}
