<!-- ── Skeleton.svelte (v1.4.15) ────────────────────────────────────────
     Shimmer placeholder for slow loading states. Drop-in for any place
     that currently shows a spinner OR nothing.

     Variants:
       row    — single horizontal bar (default). Use inside a list while
                fetching N items.
       card   — block placeholder for card-shaped UI (chips, settings rows)
       chart  — multi-bar with descending heights to fake a chart
       avatar — circle for user/lucy avatars
       text   — short bar for inline text

     Sizing is via `width` / `height` props (CSS values, default '100%' /
     auto). Lucy's existing skel-line class lives in chat (streaming
     placeholder) — this component is for the rest of the surface area.
─────────────────────────────────────────────────────────────────────── -->
<script>
    export let variant = 'row';       // 'row' | 'card' | 'chart' | 'avatar' | 'text'
    export let width   = undefined;    // CSS value, optional override
    export let height  = undefined;
    /** Number of skeleton rows when variant === 'row' or 'text'. */
    export let count   = 1;
</script>

{#if variant === 'chart'}
    <div class="skel-chart" style={width ? `width:${width};` : ''}>
        <span class="skel-bar" style="height:55%"></span>
        <span class="skel-bar" style="height:80%"></span>
        <span class="skel-bar" style="height:35%"></span>
        <span class="skel-bar" style="height:65%"></span>
        <span class="skel-bar" style="height:90%"></span>
        <span class="skel-bar" style="height:45%"></span>
        <span class="skel-bar" style="height:70%"></span>
    </div>
{:else if variant === 'avatar'}
    <span class="skel-avatar" style="width:{width || '32px'};height:{height || '32px'};"></span>
{:else if variant === 'card'}
    <div class="skel-card" style="width:{width || '100%'};height:{height || '60px'};"></div>
{:else if variant === 'text'}
    {#each Array(count) as _, i}
        <span class="skel-text" style="width:{i === count - 1 ? '60%' : '92%'};"></span>
    {/each}
{:else}
    {#each Array(count) as _, i}
        <div class="skel-row" style="width:{width || '100%'};height:{height || '14px'};"></div>
    {/each}
{/if}

<style>
    /* All variants share the same base shimmer animation — accent-tinted
       gradient that runs left→right. Mirrors the chat skeleton style in
       ChatThread so the visual language is consistent. */
    .skel-row, .skel-card, .skel-text, .skel-bar, .skel-avatar {
        background: linear-gradient(90deg,
            var(--bg3, #0f1520) 0%,
            var(--bg4, #1e293b) 30%,
            color-mix(in srgb, var(--acc, #10b981) 26%, var(--bg4, #1e293b)) 50%,
            var(--bg4, #1e293b) 70%,
            var(--bg3, #0f1520) 100%);
        background-size: 220% 100%;
        animation: skel-shimmer 1.4s ease-in-out infinite;
        border-radius: 6px;
        display: block;
    }

    .skel-row + .skel-row { margin-top: 8px; }

    .skel-text {
        display: block;
        height: 12px;
        margin: 4px 0;
        border-radius: 3px;
    }

    .skel-card {
        border-radius: 10px;
    }

    .skel-avatar {
        border-radius: 50%;
        display: inline-block;
    }

    .skel-chart {
        display: flex;
        align-items: flex-end;
        gap: 6px;
        height: 80px;
        padding: 0 4px;
    }
    .skel-bar {
        flex: 1;
        min-width: 12px;
        border-radius: 3px 3px 0 0;
    }

    @keyframes skel-shimmer {
        0%   { background-position: 200% 0; }
        100% { background-position: -200% 0; }
    }

    @media (prefers-reduced-motion: reduce) {
        .skel-row, .skel-card, .skel-text, .skel-bar, .skel-avatar {
            animation: none;
            background: var(--bg4, #1e293b);
        }
    }
</style>
