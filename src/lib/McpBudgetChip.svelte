<!-- ── McpBudgetChip.svelte (v1.6.2) ──────────────────────────────────────
     Live MCP-budget indicator. Renders a compact pill summarizing how
     much of the context window is being eaten by enabled MCP tool
     definitions, with tone bands matching the ECC `mcp-budget` skill
     recommendations.

     Visual:
       ◉ 6/8 srv · 42 tools · ~32k tok        → ok    (green)
       ⚠ 9/8 srv · 70/80 tools · ~50k tok     → warn  (amber)
       ✕ 10/10 srv · 120 tools · ~85k tok     → crit  (red)

     Props:
       servers — McpServer[] from the parent (already-fetched cache,
                 no extra round-trip).
       isEN    — i18n
       compact — single-line vs. multi-line breakdown
─────────────────────────────────────────────────────────────────────── -->
<script lang="ts">
    import {
        computeBudget,
        BUDGET_SERVERS_CRIT, BUDGET_TOOLS_CRIT,
        type McpServerLite,
    } from '$lib/mcp-budget';

    export let servers: McpServerLite[] = [];
    export let isEN    = false;
    export let compact = false;

    $: budget = computeBudget(servers);

    $: tokenLabel = `${(budget.estimatedTokens / 1000).toFixed(1)}k`;
    $: glyph = budget.tone === 'crit' ? '✕'
            : budget.tone === 'warn' ? '⚠'
            : '◉';
    $: rationale = (() => {
        if (budget.tone === 'ok') {
            return isEN ? 'Within budget' : 'Dentro del presupuesto';
        }
        if (budget.tone === 'crit') {
            return isEN
                ? `Critical — disable unused servers (caps: ${BUDGET_SERVERS_CRIT} servers / ${BUDGET_TOOLS_CRIT} tools).`
                : `Crítico — deshabilita servidores sin uso (límites: ${BUDGET_SERVERS_CRIT} servidores / ${BUDGET_TOOLS_CRIT} tools).`;
        }
        return isEN
            ? 'Approaching limit. Consider trimming MCP servers before adding more.'
            : 'Cerca del límite. Considera reducir servidores MCP antes de agregar más.';
    })();
</script>

<button class="mbc mbc-{budget.tone}" type="button"
        title={`${budget.reason} — ${rationale}`}>
    <span class="mbc-glyph">{glyph}</span>
    {#if compact}
        <span class="mbc-counts">{budget.enabledServers} · {budget.enabledTools} · ~{tokenLabel}</span>
    {:else}
        <span class="mbc-counts">
            <span class="mbc-axis" class:mbc-bad={budget.serverTone !== 'ok'}>
                {budget.enabledServers}/{BUDGET_SERVERS_CRIT}
                <small>{isEN ? 'servers' : 'serv'}</small>
            </span>
            <span class="mbc-sep">·</span>
            <span class="mbc-axis" class:mbc-bad={budget.toolTone !== 'ok'}>
                {budget.enabledTools}/{BUDGET_TOOLS_CRIT}
                <small>tools</small>
            </span>
            <span class="mbc-sep">·</span>
            <span class="mbc-axis" class:mbc-bad={budget.tokenTone !== 'ok'}>
                ~{tokenLabel}
                <small>tok</small>
            </span>
        </span>
    {/if}
</button>

<style>
    .mbc {
        display: inline-flex;
        align-items: baseline;
        gap: 6px;
        padding: 3px 9px;
        border-radius: 9px;
        font-size: 11px;
        font-family: var(--mono, ui-monospace, monospace);
        font-weight: 600;
        line-height: 1.4;
        border: 1px solid transparent;
        cursor: help;
        transition: background .12s, border-color .12s;
        background: transparent;
        color: inherit;
    }
    .mbc-glyph { font-size: 10px; }
    .mbc-counts { display: inline-flex; gap: 5px; align-items: baseline; }
    .mbc-axis   { display: inline-flex; gap: 3px; align-items: baseline; }
    .mbc-axis small {
        opacity: .55;
        font-size: 9px;
        font-weight: 500;
    }
    .mbc-bad { color: inherit; opacity: 1; text-decoration: underline; text-decoration-thickness: 1px; text-underline-offset: 2px; }
    .mbc-sep { opacity: .45; }
    .mbc-ok {
        color: var(--acc, #10b981);
        background: rgba(16, 185, 129, .07);
        border-color: rgba(16, 185, 129, .22);
    }
    .mbc-warn {
        color: var(--amber, #f59e0b);
        background: rgba(245, 158, 11, .07);
        border-color: rgba(245, 158, 11, .28);
    }
    .mbc-crit {
        color: var(--red, #ef4444);
        background: rgba(239, 68, 68, .08);
        border-color: rgba(239, 68, 68, .32);
    }
    .mbc:hover { filter: brightness(1.15); }
</style>
