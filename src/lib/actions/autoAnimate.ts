// ── autoAnimate Svelte action (v1.4.11) ────────────────────────────────
//
// Wraps @formkit/auto-animate so any list / container in Lucy can opt in
// to FLIP animations with a single attribute:
//
//   <div use:autoAnimate>
//     {#each items as item (item.id)}
//       <div>{item.label}</div>
//     {/each}
//   </div>
//
// auto-animate detects added / removed / moved children and animates
// them with a 250ms ease-out by default — no Svelte transition / flip
// boilerplate needed.
//
// We expose ONE action so the import surface is small and the option
// surface is opinionated to Lucy's motion language (slightly faster
// than the library default; matches the 200ms used elsewhere in the
// app).

import autoAnimateRaw from '@formkit/auto-animate';
import type { Action } from 'svelte/action';

type AutoAnimateParams = Partial<{ duration: number; easing: string; disrespectUserMotionPreference: boolean }>;
export const autoAnimate: Action<HTMLElement, AutoAnimateParams | undefined> = (node, params) => {
    // Apply once on mount. The library installs its own MutationObserver
    // and runs until the node is removed, so there's nothing to clean up
    // beyond that — the observer disconnects automatically when the node
    // detaches.
    autoAnimateRaw(node, {
        duration: params?.duration ?? 220,
        easing:   params?.easing ?? 'ease-in-out',
        // Default true: honor prefers-reduced-motion. Operators with
        // vestibular sensitivities are real users.
        disrespectUserMotionPreference: params?.disrespectUserMotionPreference ?? false,
    });
    return {
        // No teardown needed; the observer self-disconnects on detach.
        // Update is a no-op because we'd need to dispose+recreate to
        // change params at runtime, and the caller can wrap with a key
        // block instead if that's truly needed.
        update() {},
        destroy() {},
    };
};
