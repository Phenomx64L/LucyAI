<script>
    // ── StatusOrb — ambient presence indicator ────────────────────────────
    //
    // A small pulsing orb fixed to the bottom-right of the window that
    // reflects Lucy's current activity state. Inspired by Cursor's status
    // indicator + Linear's connection dot.
    //
    // States (controlled via the `state` prop):
    //   'idle'      — soft green pulse every 4s (Lucy is ready)
    //   'thinking'  — faster cyan pulse (LLM streaming)
    //   'executing' — amber, with a circular progress sweep (tool running)
    //   'error'     — quick red flicker, settles back to idle
    //
    // The orb listens for state changes via the `state` prop (driven from
    // +page.svelte's tab.isProcessing + active reasoning + last error). It
    // never grabs focus, never blocks clicks (pointer-events:none on the
    // outer ring), and is visually subtle — Cursor-style restraint.

    export let state = 'idle';     // 'idle' | 'thinking' | 'executing' | 'error'
    export let label = '';         // optional tooltip label (visible on hover)
    export let visible = true;     // hide when in setup overlay etc.

    // Track the last error briefly so we can flash red even if state flips
    // back to idle quickly.
    let _flashUntil = 0;
    $: if (state === 'error') _flashUntil = Date.now() + 1200;
    $: effective = (Date.now() < _flashUntil && state === 'idle') ? 'error' : state;
</script>

{#if visible}
<div class="orb-wrap" data-state={effective} title={label || effective}
     aria-label="Lucy status: {effective}" role="status">
    <div class="orb-pulse"></div>
    <div class="orb-core"></div>
    {#if effective === 'executing'}
        <svg class="orb-progress" viewBox="0 0 36 36" aria-hidden="true">
            <circle cx="18" cy="18" r="15" />
        </svg>
    {/if}
</div>
{/if}

<style>
    .orb-wrap {
        /* Footer is 22px tall; sit JUST above it with 12px clearance so the
           orb never overlaps the language code / cost summary on the right. */
        position: fixed;
        right: 12px;
        bottom: 32px;
        width: 12px;
        height: 12px;
        z-index: 50;
        pointer-events: auto;
        cursor: default;
        /* Tooltip handled by browser via title attr. */
    }

    /* Core dot — solid color, sharp. */
    .orb-core {
        position: absolute; inset: 3px;
        border-radius: 50%;
        background: var(--state-color, var(--accent));
        box-shadow: 0 0 6px var(--state-color, var(--accent)),
                    0 0 14px color-mix(in srgb, var(--state-color, var(--accent)) 35%, transparent);
        transition: background var(--motion-base, 240ms) var(--ease-out, ease-out),
                    box-shadow var(--motion-base, 240ms) var(--ease-out, ease-out);
    }

    /* Pulsing halo — breathes. Period changes with state via --state-pulse. */
    .orb-pulse {
        position: absolute; inset: 0;
        border-radius: 50%;
        background: var(--state-color, var(--accent));
        opacity: 0.18;
        animation: orb-breathe var(--state-pulse, 4s) ease-in-out infinite;
        transition: background var(--motion-base, 240ms) var(--ease-out, ease-out);
    }

    @keyframes orb-breathe {
        0%, 100% { transform: scale(1);   opacity: 0.18; }
        50%      { transform: scale(2.2); opacity: 0.04; }
    }

    /* Executing: a thin sweeping arc on top of the core. */
    .orb-progress {
        position: absolute; inset: -3px;
        animation: orb-sweep 1.4s linear infinite;
    }
    .orb-progress circle {
        fill: none;
        stroke: var(--state-color, var(--amber));
        stroke-width: 1.5;
        stroke-dasharray: 24 70;     /* short visible arc, long gap */
        stroke-linecap: round;
        opacity: 0.85;
    }
    @keyframes orb-sweep {
        from { transform: rotate(0deg); }
        to   { transform: rotate(360deg); }
    }

    /* Light theme adaptation — orb still pops on bright backgrounds. */
    :global(:root.light) .orb-core {
        box-shadow: 0 0 4px var(--state-color, var(--accent)),
                    0 0 10px color-mix(in srgb, var(--state-color, var(--accent)) 25%, transparent);
    }

    /* Reduced motion: kill the breathe. Solid dot only. */
    @media (prefers-reduced-motion: reduce) {
        .orb-pulse { animation: none; opacity: 0.25; }
        .orb-progress { animation-duration: 4s; }
    }

    /* Error state: brief sharp flash then back to its color. */
    .orb-wrap[data-state="error"] .orb-core {
        animation: orb-flash 0.8s ease-out 1;
    }
    @keyframes orb-flash {
        0%, 30%  { transform: scale(1.2); }
        50%      { transform: scale(0.85); }
        100%     { transform: scale(1); }
    }
</style>
