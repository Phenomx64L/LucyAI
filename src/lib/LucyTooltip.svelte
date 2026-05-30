<!-- ── LucyTooltip.svelte (v1.4.16) ────────────────────────────────────────
     Thin wrapper around bits-ui Tooltip with Lucy's visual identity baked
     in. Replaces native `title=""` on action buttons so we get:
       - Consistent positioning + flip-on-overflow
       - Predictable show delay (default 350ms)
       - Keyboard-focus triggering (title= only fires on hover)
       - Animations honoring prefers-reduced-motion

     Usage:
         <LucyTooltip text="Pin to context">
             <button>·</button>
         </LucyTooltip>

     The `text` prop is the only required one; `delayMs` overrides the
     default, `side` picks 'top'|'bottom'|'left'|'right'. The whole thing
     is a single Provider per-mount, which is fine because bits-ui hoists
     the Portal so there's no DOM-tree contention.
─────────────────────────────────────────────────────────────────────── -->
<script>
    import { Tooltip } from 'bits-ui';
    export let text    = '';
    export let side    = 'top';      // 'top' | 'right' | 'bottom' | 'left'
    export let delayMs = 350;
    export let disabled = false;
</script>

{#if disabled || !text}
    <slot />
{:else}
    <Tooltip.Provider delayDuration={delayMs}>
        <Tooltip.Root>
            <Tooltip.Trigger class="lt-trigger"><slot /></Tooltip.Trigger>
            <Tooltip.Portal>
                <Tooltip.Content class="lt-content" sideOffset={6} side={side}>
                    {text}
                </Tooltip.Content>
            </Tooltip.Portal>
        </Tooltip.Root>
    </Tooltip.Provider>
{/if}

<style>
    :global(.lt-trigger){
        all: unset;
        display: contents;        /* Don't introduce a wrapper box. */
    }
    :global(.lt-content){
        background: var(--bg2, #0b0e14);
        color: var(--txt, #dde3ea);
        border: 1px solid var(--bdr, #1a2030);
        border-radius: 6px;
        padding: 5px 9px;
        font-size: 11.5px;
        line-height: 1.3;
        max-width: 280px;
        box-shadow: 0 6px 20px rgba(0,0,0,.45);
        z-index: 8500;
        animation: lt-pop .12s ease-out;
    }
    @keyframes lt-pop {
        from { opacity: 0; transform: translateY(2px) scale(.96); }
        to   { opacity: 1; transform: none; }
    }
    @media (prefers-reduced-motion: reduce) {
        :global(.lt-content){ animation: none; }
    }
</style>
