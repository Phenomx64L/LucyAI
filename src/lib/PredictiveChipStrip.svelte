<!--
  ── PredictiveChipStrip.svelte — U5 contextual suggestion strip ────────────

  Renders 0-3 chips horizontally just above the input bar. Each chip:
    • Has icon + label + tooltip
    • Click → dispatches 'chipaction' with the chip's action payload
    • Has a tiny dismiss × that hides it for the rest of this turn

  Auto-hidden when chips array is empty (no visual noise).
-->
<script lang="ts">
    import { createEventDispatcher } from 'svelte';
    import { fly, fade } from 'svelte/transition';
    import { dismissChip, isDismissed, recordChipDismiss } from '$lib/predictive-chips';
    import type { PredictiveChip } from '$lib/predictive-chips';
    // v1.4.11 — auto-animate provides FLIP transitions when chips swap
    // (LLM layer arrives ~600ms after heuristic, replacing some chips).
    import { autoAnimate } from '$lib/actions/autoAnimate';

    export let chips: PredictiveChip[] = [];

    const dispatch = createEventDispatcher<{
        chipaction:  { chip: PredictiveChip };
        chipdismiss: { chip: PredictiveChip };
    }>();

    // Filter out dismissed ones reactively
    $: visibleChips = chips.filter(c => !isDismissed(c.id));

    // Source labels for the provenance badge. Kept here (not in the .ts
    // module) because they're pure presentation — backend doesn't care.
    const SRC_BADGE: Record<string, { glyph: string; title: string }> = {
        heuristic: { glyph: '⚡', title: 'Heuristic — pattern matched on the last turn' },
        llm:       { glyph: '✦', title: 'AI suggestion — generated from the recent conversation' },
        memory:    { glyph: '◊', title: 'Learned from your past sessions' },
    };

    function onClick(chip: PredictiveChip) {
        dispatch('chipaction', { chip });
    }

    function onDismiss(ev: Event, chip: PredictiveChip) {
        ev.stopPropagation();
        dismissChip(chip.id);        // hides for this turn
        recordChipDismiss(chip.id);  // persists: lowers engagement score long-term
        dispatch('chipdismiss', { chip }); // let parent log it for Layer-3
        chips = chips; // trigger reactivity
    }
</script>

{#if visibleChips.length > 0}
    <div class="chip-strip" role="region" aria-label="Suggested next actions" use:autoAnimate>
        {#each visibleChips as chip, i (chip.id)}
            <!-- Use div+role="button" so we can nest a dismiss control (HTML disallows button-in-button) -->
            <div class="pchip pchip-{chip.severity || 'info'}"
                 role="button"
                 tabindex="0"
                 title={chip.tooltip}
                 on:click={() => onClick(chip)}
                 on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onClick(chip); } }}
                 in:fly={{ y: 6, duration: 200, delay: i * 40 }}
                 out:fade={{ duration: 120 }}>
                <span class="pchip-icon">{chip.icon}</span>
                <span class="pchip-label">{chip.label}</span>
                {#if chip.source && SRC_BADGE[chip.source]}
                    <span class="pchip-src pchip-src-{chip.source}"
                          title={SRC_BADGE[chip.source].title}
                          aria-label={SRC_BADGE[chip.source].title}>
                        {SRC_BADGE[chip.source].glyph}
                    </span>
                {/if}
                <span class="pchip-x"
                      role="button"
                      tabindex="0"
                      on:click={(e) => onDismiss(e, chip)}
                      on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); e.stopPropagation(); onDismiss(e, chip); } }}
                      title="Descartar sugerencia"
                      aria-label="Dismiss">×</span>
            </div>
        {/each}
    </div>
{/if}

<style>
    .chip-strip {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 6px;
        padding: 4px 14px 6px;
        background: linear-gradient(to bottom, rgba(0,0,0,0.10), transparent);
        border-top: 1px solid rgba(255,255,255,0.04);
    }
    .pchip {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 4px 8px 4px 10px;
        font-family: var(--font-mono, monospace);
        font-size: 11px;
        font-weight: 500;
        color: var(--text-main, #e2e8f0);
        background: rgba(255,255,255,0.03);
        border: 1px solid rgba(255,255,255,0.08);
        border-radius: 14px;
        cursor: pointer;
        transition: background var(--motion-fast) var(--ease-out),
                    border-color var(--motion-fast) var(--ease-out),
                    transform var(--motion-fast) var(--ease-out);
    }
    .pchip:hover {
        background: rgba(255,255,255,0.06);
        border-color: rgba(255,255,255,0.18);
        transform: translateY(-1px);
    }
    .pchip-icon {
        font-size: 12px;
        opacity: 0.85;
        line-height: 1;
    }
    .pchip-label {
        line-height: 1.2;
        white-space: nowrap;
    }
    /* Provenance badge — tiny glyph distinguishing heuristic / LLM / memory.
       Sits between label and dismiss, neutral by default with a per-source
       accent so the eye can scan a strip and tell at a glance which chips
       come from the AI vs the rules. */
    .pchip-src {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        font-size: 9.5px;
        line-height: 1;
        opacity: 0.75;
        padding: 1px 4px;
        border-radius: 4px;
        margin-left: 4px;
        background: rgba(255,255,255,0.06);
        color: var(--txt2, #94a3b8);
        font-weight: 600;
    }
    .pchip:hover .pchip-src { opacity: 1; }
    .pchip-src-llm {
        background: rgba(167,139,250,0.14);
        color: #c4b5fd;
    }
    .pchip-src-heuristic {
        background: rgba(16,185,129,0.10);
        color: var(--acc, #10b981);
    }
    .pchip-src-memory {
        background: rgba(59,158,255,0.12);
        color: #3b9eff;
    }

    .pchip-x {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        margin-left: 2px;
        width: 14px;
        height: 14px;
        padding: 0;
        font-size: 12px;
        line-height: 1;
        color: var(--text-muted, #64748b);
        background: transparent;
        border: none;
        border-radius: 50%;
        cursor: pointer;
        opacity: 0;
        transition: opacity var(--motion-fast) var(--ease-out),
                    background var(--motion-fast) var(--ease-out);
    }
    .pchip:hover .pchip-x { opacity: 0.8; }
    .pchip-x:hover {
        opacity: 1 !important;
        background: rgba(255,255,255,0.12);
        color: var(--text-bright);
    }

    /* Severity variants — subtle but distinct.
       .pchip-info uses the default neutral styling above. */
    .pchip-suggest {
        color: var(--blue, #3b9eff);
        border-color: rgba(59,158,255,0.25);
        background: rgba(59,158,255,0.05);
    }
    .pchip-suggest:hover {
        background: rgba(59,158,255,0.10);
        border-color: rgba(59,158,255,0.40);
    }
    .pchip-caution {
        color: var(--amber, #f59e0b);
        border-color: rgba(245,158,11,0.25);
        background: rgba(245,158,11,0.05);
    }
    .pchip-caution:hover {
        background: rgba(245,158,11,0.10);
        border-color: rgba(245,158,11,0.45);
    }

    /* ── Light mode (v1.7.232) ─────────────────────────────────────────────
       Strip + neutral chips were dark-tuned: the strip's black gradient smudges
       on white, and the neutral chip's `var(--text-main)` (#e2e8f0, no light
       value) is light-grey on white = invisible (the washed-out "Relanzar").
       Target `.pchip-info` for the neutral case so the colored variants keep
       their hue; deepen the suggest/caution tints for parity on white. */
    :global(:root.light) .chip-strip {
        background: linear-gradient(to bottom, rgba(15,23,42,0.035), transparent);
        border-top-color: var(--bdr, #cbd5e1);
    }
    :global(:root.light) .pchip-info {
        background: var(--bg4, #fff);
        border-color: var(--bdr2, #94a3b8);
        color: var(--txt, #0f172a);
        box-shadow: var(--shadow-card, 0 1px 2px rgba(15,23,42,.05), 0 2px 6px rgba(15,23,42,.06));
    }
    :global(:root.light) .pchip-info:hover {
        background: var(--bg2, #f1f5f9);
        border-color: var(--acc, #059669);
    }
    :global(:root.light) .pchip-suggest {
        background: color-mix(in srgb, var(--blue, #2563eb) 9%, #fff);
        border-color: color-mix(in srgb, var(--blue, #2563eb) 38%, transparent);
    }
    :global(:root.light) .pchip-suggest:hover {
        background: color-mix(in srgb, var(--blue, #2563eb) 15%, #fff);
        border-color: var(--blue, #2563eb);
    }
    :global(:root.light) .pchip-caution {
        background: color-mix(in srgb, var(--amber, #d97706) 12%, #fff);
        border-color: color-mix(in srgb, var(--amber, #d97706) 42%, transparent);
    }
    :global(:root.light) .pchip-caution:hover {
        background: color-mix(in srgb, var(--amber, #d97706) 18%, #fff);
        border-color: var(--amber, #d97706);
    }
    /* Provenance LLM badge is light-purple (#c4b5fd) — illegible on white. */
    :global(:root.light) .pchip-src-llm { color: #7c3aed; }
    :global(:root.light) .pchip-x { color: var(--txt3, #64748b); }
    :global(:root.light) .pchip-x:hover { background: rgba(15,23,42,0.08); color: var(--txt, #0f172a); }
</style>
