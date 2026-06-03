<!-- ── MemoryFeed.svelte — Recent memories ticker (v1.7.27, theme "A") ──────
     A small widget that lives under the Sistema section in the sidebar.
     Shows the most-recent 3 agent_memories rolled out vertically so the
     operator gets the constant signal "Lucy has memory and it's growing".

     Pulls from `get_recent_memories` (already exposed). Refreshes:
       • on mount
       • every 60s
       • when /reload-mem event fires (future)

     Click on a row → opens Memory Browser scoped to that memory.

     Visually: each row is a colour-tinted strip with the memory's
     summary truncated to 2 lines, time-ago in monospace, and a tiny
     "score badge" if grounding strength is available.

     Hidden when sidebar is collapsed (no horizontal space). -->
<script lang="ts">
    import { onMount, onDestroy, createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    // v1.7.32 — Brain glyph from Tabler instead of the 🧠 emoji
    // (renders inconsistently per OS, doesn't match Lucy's icon vocabulary).
    import Brain from '@tabler/icons-svelte/icons/brain';

    export let isEN = false;
    export let sidebarCollapsed = false;

    interface Memory {
        id:           number;
        summary:      string;
        importance:   number;
        created_at:   number;
        tags?:        string;
    }

    const dispatch = createEventDispatcher<{ open: { id: number } }>();

    let memories: Memory[] = [];
    let loading = true;
    let _timer: number | null = null;

    async function refresh() {
        try {
            const rows = await invoke<Memory[]>('get_recent_memories', { limit: 3 });
            memories = Array.isArray(rows) ? rows : [];
        } catch (e) {
            // Silent — the sidebar is non-critical surface.
            memories = [];
        } finally {
            loading = false;
        }
    }

    function timeAgo(unixSec: number): string {
        const now = Math.floor(Date.now() / 1000);
        const d = now - unixSec;
        if (d < 60)     return isEN ? 'now'   : 'ahora';
        if (d < 3600)   return `${Math.floor(d/60)}m`;
        if (d < 86400)  return `${Math.floor(d/3600)}h`;
        if (d < 604800) return `${Math.floor(d/86400)}d`;
        return `${Math.floor(d/604800)}w`;
    }

    function truncate(s: string, n: number): string {
        if (!s) return '';
        return s.length > n ? s.slice(0, n - 1) + '…' : s;
    }

    onMount(() => {
        refresh();
        _timer = window.setInterval(refresh, 60_000);
    });
    onDestroy(() => { if (_timer !== null) clearInterval(_timer); });
</script>

{#if !sidebarCollapsed}
    <div class="mf-wrap">
        <div class="mf-header">
            <span class="mf-glyph"><Brain size={12} stroke={2}/></span>
            <span class="mf-title">{isEN ? 'Recent memory' : 'Memoria reciente'}</span>
            {#if memories.length > 0}
                <span class="mf-count">{memories.length}</span>
            {/if}
        </div>

        {#if loading}
            <div class="mf-skel">
                <div class="mf-skel-line"></div>
                <div class="mf-skel-line" style="width:62%;"></div>
                <div class="mf-skel-line" style="width:78%;"></div>
            </div>
        {:else if memories.length === 0}
            <div class="mf-empty">
                {isEN ? 'No memories yet — Lucy will start remembering as you work.' : 'Sin memorias aún — Lucy empezará a recordar al trabajar.'}
            </div>
        {:else}
            <div class="mf-list">
                {#each memories as m (m.id)}
                    <button class="mf-row" type="button"
                            on:click={() => dispatch('open', { id: m.id })}
                            title={m.summary}>
                        <span class="mf-row-text">{truncate(m.summary, 70)}</span>
                        <span class="mf-row-meta">
                            <span class="mf-row-time">{timeAgo(m.created_at)}</span>
                            {#if m.importance >= 0.7}
                                <span class="mf-row-imp" title="High importance">●</span>
                            {/if}
                        </span>
                    </button>
                {/each}
            </div>
        {/if}
    </div>
{/if}

<style>
    /* v1.7.36 — When the widget sits IMMEDIATELY under the "Memoria"
       sb-it (its parent concept), the previous top-border divider made
       it look like a SECTION break, not a CONTINUATION of Memoria. The
       new look is a thin left rail that visually nests the widget
       under its parent — same pattern as a folder tree node. */
    .mf-wrap {
        display: flex; flex-direction: column;
        gap: 6px;
        padding: 6px 12px 10px 24px;   /* extra left padding for nesting */
        margin: 2px 8px 6px 8px;
        border-left: 2px solid color-mix(in srgb, #06b6d4 35%, transparent);
        border-radius: 0 0 0 4px;
        background: color-mix(in srgb, #06b6d4 4%, transparent);
        animation: mf-fade-in 200ms ease;
    }

    .mf-header {
        display: flex; align-items: center; gap: 6px;
        font-size: 10.5px;
        letter-spacing: 0.5px;
        color: var(--txt3, #64748b);
        text-transform: uppercase;
        font-weight: 600;
    }
    .mf-glyph { font-size: 11px; opacity: 0.85; display: inline-flex; align-items: center; color: #67e8f9; }
    .mf-title { flex: 1; }
    .mf-count {
        padding: 1px 6px;
        border-radius: 9px;
        background: color-mix(in srgb, #06b6d4 18%, transparent);
        color: #67e8f9;
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 9.5px;
        font-weight: 700;
        letter-spacing: 0;
    }

    .mf-skel { display: flex; flex-direction: column; gap: 4px; padding: 2px 0; }
    .mf-skel-line {
        height: 10px;
        border-radius: 3px;
        background: linear-gradient(90deg,
            rgba(255,255,255,0.03) 0%,
            rgba(255,255,255,0.06) 50%,
            rgba(255,255,255,0.03) 100%);
        background-size: 200% 100%;
        animation: mf-skel-shimmer 1.4s ease-in-out infinite;
    }
    .mf-skel-line:nth-child(1) { width: 88%; }
    @keyframes mf-skel-shimmer { 0%{background-position:200% 0;} 100%{background-position:-200% 0;} }

    .mf-empty {
        font-size: 11px;
        color: var(--txt3, #64748b);
        line-height: 1.5;
        padding: 4px 0;
        font-style: italic;
        opacity: 0.7;
    }

    .mf-list { display: flex; flex-direction: column; gap: 3px; }
    .mf-row {
        display: flex; align-items: flex-start; gap: 8px;
        text-align: left;
        padding: 6px 8px;
        border-radius: 6px;
        border: 1px solid transparent;
        background: rgba(255,255,255,0.015);
        color: var(--txt2, #94a3b8);
        font-size: 11px;
        line-height: 1.35;
        cursor: pointer;
        transition: background .12s, border-color .12s, color .12s, transform .08s;
        font-family: inherit;
    }
    .mf-row:hover {
        background: color-mix(in srgb, #06b6d4 8%, transparent);
        border-color: color-mix(in srgb, #06b6d4 30%, transparent);
        color: var(--txt1, #f1f5f9);
        transform: translateY(-1px);
    }
    .mf-row:active { transform: translateY(0); }

    .mf-row-text {
        flex: 1;
        display: -webkit-box;
        -webkit-line-clamp: 2;
        line-clamp: 2;
        -webkit-box-orient: vertical;
        overflow: hidden;
    }
    .mf-row-meta {
        display: flex; align-items: center; gap: 4px;
        flex-shrink: 0;
        margin-top: 1px;
    }
    .mf-row-time {
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 9.5px;
        color: var(--txt3, #64748b);
        opacity: 0.7;
    }
    .mf-row-imp {
        font-size: 8px;
        color: var(--amber, #f59e0b);
        line-height: 1;
    }

    @keyframes mf-fade-in {
        from { opacity: 0; transform: translateY(-4px); }
        to   { opacity: 1; transform: none; }
    }
    @media (prefers-reduced-motion: reduce) {
        .mf-wrap, .mf-skel-line { animation: none !important; }
        .mf-row { transition: none !important; }
    }
</style>
