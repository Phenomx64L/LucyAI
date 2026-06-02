<!-- ── ContextStrip.svelte — Live view of Lucy's LLM context (v1.7.22) ──
     A horizontal strip that sits between the tab bar and the chat
     viewport, showing the user what Lucy has in her prompt RIGHT NOW:

       🧠 N memorias  ·  ⚡ skill: <id>  ·  ◇ preset: <name>
                                       ·  🔌 N MCP tools  ·  ◆ <used>/<max>

     This is Lucy's identity moment. No other AI assistant exposes its
     own context this transparently in real time, and for a sysadmin or
     security operator this is the equivalent of a flight panel: at a
     glance you know what shape the conversation is in.

     Behavior:
     - Sticky to the top of the chat viewport so it persists while
       scrolling history.
     - Each chip is a button that dispatches its `clickX` event so the
       parent can open the relevant modal (memory browser, skill picker,
       preset picker, MCP config) without this component having to know
       the implementation details.
     - Chip is hidden when its value is empty/zero so we don't ever
       render a row of grayed-out dead chips.
     - Whole strip is hidden when ALL chips would be empty (cold start,
       no skill, no preset, no MCP) so it doesn't waste vertical space
       on a fresh tab. -->
<script lang="ts">
    import { createEventDispatcher } from 'svelte';
    import { contextSnapshot } from '$lib/context-snapshot';

    const dispatch = createEventDispatcher();

    /** Compact mode collapses labels into icon-only chips. Default off. */
    export let compact = false;

    $: snap = $contextSnapshot;
    $: hasAny = !!(
        snap.memoriesCount   > 0 ||
        snap.skillId         ||
        snap.presetId        ||
        snap.mcpToolsCount   > 0 ||
        snap.estTokens       > 0
    );

    /** Format a skill id like "conducting-phishing-incident-response"
     *  → "phishing-incident-response" (drop common prefix) and clamp
     *  to ~22 chars so the chip stays narrow. */
    function shortSkill(id: string): string {
        if (!id) return '';
        let s = id.replace(/^(conducting|implementing|setting-up|managing)-/, '');
        if (s.length > 22) s = s.slice(0, 21) + '…';
        return s;
    }

    function shortPreset(id: string): string {
        if (!id) return '';
        if (id.length > 22) return id.slice(0, 21) + '…';
        return id;
    }

    /** Color-band the token chip by % of budget used. */
    function tokenTone(used: number, max: number): 'ok' | 'warn' | 'crit' | 'idle' {
        if (!max || !used) return 'idle';
        const pct = used / max;
        if (pct >= 0.85) return 'crit';
        if (pct >= 0.65) return 'warn';
        return 'ok';
    }

    function fmtTokens(n: number): string {
        if (n >= 1000) return (n / 1000).toFixed(1) + 'k';
        return String(n);
    }
</script>

{#if hasAny}
<div class="cs-strip" role="toolbar" aria-label="Lucy context snapshot">
    {#if snap.memoriesCount > 0}
        <button class="cs-chip cs-mem" type="button"
                on:click={() => dispatch('clickMemories')}
                title="{snap.memoriesCount} memorias en el contexto. Click para ver cuáles.">
            <span class="cs-glyph">🧠</span>
            <span class="cs-val">{snap.memoriesCount}</span>
            {#if !compact}<span class="cs-lbl">memorias</span>{/if}
        </button>
    {/if}

    {#if snap.skillId}
        <button class="cs-chip cs-skill" type="button"
                class:cs-manual={snap.skillSource === 'manual'}
                on:click={() => dispatch('clickSkill')}
                title="Skill activo: {snap.skillId} ({snap.skillSource ?? 'auto'}). Click para cambiar o desactivar.">
            <span class="cs-glyph">⚡</span>
            {#if !compact}<span class="cs-lbl">skill</span>{/if}
            <span class="cs-val">{shortSkill(snap.skillId)}</span>
        </button>
    {/if}

    {#if snap.presetId}
        <button class="cs-chip cs-preset" type="button"
                on:click={() => dispatch('clickPreset')}
                title="Preset ECC activo: {snap.presetId}. Click para cambiar o quitar.">
            <span class="cs-glyph">◇</span>
            {#if !compact}<span class="cs-lbl">preset</span>{/if}
            <span class="cs-val">{shortPreset(snap.presetId)}</span>
        </button>
    {/if}

    {#if snap.mcpToolsCount > 0}
        <button class="cs-chip cs-mcp" type="button"
                on:click={() => dispatch('clickMcp')}
                title="{snap.mcpToolsCount} MCP tools rankeados en el contexto. Click para abrir MCP Servers.">
            <span class="cs-glyph">🔌</span>
            <span class="cs-val">{snap.mcpToolsCount}</span>
            {#if !compact}<span class="cs-lbl">MCP tools</span>{/if}
        </button>
    {/if}

    {#if snap.estTokens > 0}
        <button class="cs-chip cs-tokens cs-tok-{tokenTone(snap.estTokens, snap.maxTokens)}"
                type="button"
                on:click={() => dispatch('clickTokens')}
                title="Tokens estimados inyectados: {snap.estTokens} de {snap.maxTokens || '?'} max. Click para detalles.">
            <span class="cs-glyph">◆</span>
            <span class="cs-val">
                {fmtTokens(snap.estTokens)}{snap.maxTokens ? '/' + fmtTokens(snap.maxTokens) : ''}
            </span>
            {#if !compact}<span class="cs-lbl">tokens</span>{/if}
        </button>
    {/if}
</div>
{/if}

<style>
    .cs-strip {
        position: sticky; top: 0; z-index: 50;
        display: flex; flex-wrap: wrap; gap: 6px;
        align-items: center;
        padding: 6px 14px;
        background: linear-gradient(180deg,
            color-mix(in srgb, var(--bg, #0a0e1a) 92%, transparent),
            color-mix(in srgb, var(--bg, #0a0e1a) 70%, transparent));
        backdrop-filter: blur(6px);
        border-bottom: 1px solid color-mix(in srgb, var(--bdr, #1e293b) 60%, transparent);
        font-family: var(--font-ui, ui-sans-serif, system-ui);
    }

    .cs-chip {
        display: inline-flex; align-items: center;
        gap: 6px;
        padding: 4px 10px;
        height: 24px;
        border-radius: 999px;
        border: 1px solid transparent;
        background: rgba(255,255,255,0.03);
        color: var(--txt2, #94a3b8);
        font-size: 11.5px; font-weight: 500;
        cursor: pointer;
        transition: background .12s, border-color .12s, transform .08s, color .12s;
        white-space: nowrap;
    }
    .cs-chip:hover  { background: rgba(255,255,255,0.07); transform: translateY(-1px); }
    .cs-chip:active { transform: translateY(0) scale(.97); }

    .cs-glyph { font-size: 12px; line-height: 1; opacity: .9; }
    .cs-val   { font-weight: 600; color: var(--txt1, #f1f5f9); font-variant-numeric: tabular-nums; }
    .cs-lbl   { opacity: .7; }

    /* ── Memory (cyan) ── */
    .cs-mem {
        color: #67e8f9;
        border-color: color-mix(in srgb, #06b6d4 35%, transparent);
        background: color-mix(in srgb, #06b6d4 10%, transparent);
    }
    .cs-mem .cs-val { color: #cffafe; }
    .cs-mem:hover { background: color-mix(in srgb, #06b6d4 20%, transparent); }

    /* ── Skill (magenta) ── */
    .cs-skill {
        color: #f0abfc;
        border-color: color-mix(in srgb, #d946ef 35%, transparent);
        background: color-mix(in srgb, #d946ef 10%, transparent);
    }
    .cs-skill .cs-val { color: #fae8ff; }
    .cs-skill:hover { background: color-mix(in srgb, #d946ef 20%, transparent); }
    /* Manual-activation marker: amber outline ring */
    .cs-skill.cs-manual {
        border-color: color-mix(in srgb, #f59e0b 60%, transparent);
        box-shadow: inset 0 0 0 1px color-mix(in srgb, #f59e0b 35%, transparent);
    }

    /* ── Preset (teal, the brand accent) ── */
    .cs-preset {
        color: var(--accent, #10b981);
        border-color: color-mix(in srgb, var(--accent, #10b981) 35%, transparent);
        background: color-mix(in srgb, var(--accent, #10b981) 10%, transparent);
    }
    .cs-preset .cs-val { color: color-mix(in srgb, var(--accent, #10b981) 60%, #f1f5f9); }
    .cs-preset:hover { background: color-mix(in srgb, var(--accent, #10b981) 20%, transparent); }

    /* ── MCP (violet) ── */
    .cs-mcp {
        color: #c4b5fd;
        border-color: color-mix(in srgb, #a78bfa 35%, transparent);
        background: color-mix(in srgb, #a78bfa 10%, transparent);
    }
    .cs-mcp .cs-val { color: #ede9fe; }
    .cs-mcp:hover { background: color-mix(in srgb, #a78bfa 20%, transparent); }

    /* ── Tokens — banded by % of context used ── */
    .cs-tokens {
        font-variant-numeric: tabular-nums;
    }
    .cs-tok-idle { color: var(--txt3, #64748b);
                   border-color: color-mix(in srgb, var(--bdr, #334155) 60%, transparent);
                   background: rgba(255,255,255,0.02); }
    .cs-tok-ok   { color: #6ee7b7;
                   border-color: color-mix(in srgb, #10b981 35%, transparent);
                   background: color-mix(in srgb, #10b981 10%, transparent); }
    .cs-tok-warn { color: #fcd34d;
                   border-color: color-mix(in srgb, #f59e0b 40%, transparent);
                   background: color-mix(in srgb, #f59e0b 12%, transparent); }
    .cs-tok-crit { color: #fca5a5;
                   border-color: color-mix(in srgb, #ef4444 50%, transparent);
                   background: color-mix(in srgb, #ef4444 14%, transparent);
                   animation: cs-pulse 2.2s ease-in-out infinite; }

    @keyframes cs-pulse {
        0%, 100% { box-shadow: 0 0 0 0 color-mix(in srgb, #ef4444 0%, transparent); }
        50%      { box-shadow: 0 0 0 4px color-mix(in srgb, #ef4444 18%, transparent); }
    }

    /* Hide entirely in print + screenshot modes — it's a runtime
       cockpit, not part of a saved transcript. */
    @media print { .cs-strip { display: none; } }
</style>
