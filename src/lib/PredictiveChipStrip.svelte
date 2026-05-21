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
    import { dismissChip, isDismissed } from '$lib/predictive-chips';
    import type { PredictiveChip } from '$lib/predictive-chips';

    export let chips: PredictiveChip[] = [];

    const dispatch = createEventDispatcher<{
        chipaction: { chip: PredictiveChip };
    }>();

    // Filter out dismissed ones reactively
    $: visibleChips = chips.filter(c => !isDismissed(c.id));

    function onClick(chip: PredictiveChip) {
        dispatch('chipaction', { chip });
    }

    function onDismiss(ev: Event, id: string) {
        ev.stopPropagation();
        dismissChip(id);
        chips = chips; // trigger reactivity
    }
</script>

{#if visibleChips.length > 0}
    <div class="chip-strip" role="region" aria-label="Suggested next actions">
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
                <span class="pchip-x"
                      role="button"
                      tabindex="0"
                      on:click={(e) => onDismiss(e, chip.id)}
                      on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); e.stopPropagation(); onDismiss(e, chip.id); } }}
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
</style>
