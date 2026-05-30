// ── vitest.setup.ts ──────────────────────────────────────────────────────
// Global setup file loaded BEFORE any test module imports execute. Lives
// here because ES import hoisting means per-test stubs land too late for
// modules that pull svelte/motion (which constructs MediaQuery at load
// time for prefers-reduced-motion detection).
//
// v1.4.15 — Added because StatusBar.svelte now uses `tweened()` for the
// live cost ticker animation; tweened reads window.matchMedia, jsdom
// doesn't provide it.

if (typeof window !== 'undefined' && !window.matchMedia) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (window as any).matchMedia = (q: string) => ({
        matches: false, media: q, onchange: null,
        addListener: () => {}, removeListener: () => {},
        addEventListener: () => {}, removeEventListener: () => {},
        dispatchEvent: () => false,
    });
}
