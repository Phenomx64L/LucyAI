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
    // v1.7.167 — styles externalized (portaled bits-ui template has no
    // scopable elements; imported CSS is global, which is what .lt-content needs).
    import './LucyTooltip.css';
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

<!-- styles moved to ./LucyTooltip.css (imported above) -->
