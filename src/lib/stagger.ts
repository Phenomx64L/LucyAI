// ── stagger — Svelte transition for list entrance ─────────────────────────
//
// Drop-in transition that makes each item in a `{#each}` block enter with
// a small incremental delay (and a soft fade-up). Used to make audit log
// entries, host lists, skill cards etc. feel "composed" rather than
// dumped on screen all at once. Inspired by Linear's list reveals.
//
// Usage:
//   <script>import { staggerIn } from '$lib/stagger';</script>
//   {#each items as item, i}
//     <div in:staggerIn={{ index: i }}>...</div>
//   {/each}
//
// All values are tunable per call site:
//   index    — position in the list (drives the delay)
//   step     — ms between successive items (default 30)
//   duration — total fade duration (default 240ms)
//   distance — vertical translate offset (default 8px → up from below)
//   delayMax — clamp delay so very long lists don't take forever
//
// Honors prefers-reduced-motion automatically (skips animation entirely).

import { cubicOut } from 'svelte/easing';

export interface StaggerInParams {
    /** Position in the list. The transition uses this to compute the delay. */
    index?: number;
    /** Milliseconds between successive items. */
    step?: number;
    /** Total fade-in duration (ms). */
    duration?: number;
    /** Vertical pixel offset items rise from. */
    distance?: number;
    /** Maximum delay cap (ms) to avoid the 30th item entering 1s late. */
    delayMax?: number;
    /** Optional extra delay added to all items (ms). */
    baseDelay?: number;
}

function _prefersReducedMotion(): boolean {
    if (typeof window === 'undefined') return false;
    try {
        return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    } catch { return false; }
}

export function staggerIn(_node: Element, params: StaggerInParams = {}) {
    const {
        index    = 0,
        step     = 30,
        duration = 240,
        distance = 8,
        delayMax = 360,
        baseDelay = 0,
    } = params;

    if (_prefersReducedMotion()) {
        return { duration: 0, css: () => '' };
    }

    const delay = Math.min(delayMax, baseDelay + Math.max(0, index) * step);

    return {
        delay,
        duration,
        easing: cubicOut,
        css: (t: number) => {
            const eased = t;
            return `
                opacity: ${eased};
                transform: translateY(${(1 - eased) * distance}px);
            `;
        },
    };
}

/** Same as staggerIn but for `out:` direction — items fade DOWN as they leave. */
export function staggerOut(_node: Element, params: StaggerInParams = {}) {
    const {
        duration = 160,
        distance = 6,
    } = params;

    if (_prefersReducedMotion()) {
        return { duration: 0, css: () => '' };
    }

    return {
        duration,
        easing: cubicOut,
        css: (t: number) => `
            opacity: ${t};
            transform: translateY(${(1 - t) * -distance}px);
        `,
    };
}
