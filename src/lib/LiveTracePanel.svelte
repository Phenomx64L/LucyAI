<!--
  LiveTracePanel — real-time agent telemetry view.

  Surfaces the $liveTrace store (ring buffer of recent agent events) as
  a slide-in panel from the right edge. Operators can watch Lucy's
  reasoning, tool calls, exec timings, and ReAct reflections as they
  happen — answering "what is Lucy doing right now?" without poking
  console logs.

  Filterable by phase + scoped to the active tab so multi-tab sessions
  don't bleed events. Auto-scrolls to the latest entry unless the user
  scrolls up (sticky-scroll discipline).
-->
<script lang="ts">
    import { onDestroy } from 'svelte';
    import { liveTrace, clearTrace, type TraceEntry, type TracePhase } from '$lib/liveTrace';
    import VirtualList from '$lib/VirtualList.svelte';
    import Activity     from '@tabler/icons-svelte/icons/activity';
    import X            from '@tabler/icons-svelte/icons/x';
    import Trash        from '@tabler/icons-svelte/icons/trash';
    import Filter       from '@tabler/icons-svelte/icons/filter';
    import ChevronDown  from '@tabler/icons-svelte/icons/chevron-down';
    import ChevronUp    from '@tabler/icons-svelte/icons/chevron-up';

    export let isEN: boolean = false;
    export let activeTabId: string = '';
    /** When true, panel is open. Bind from parent to allow external toggle. */
    export let open: boolean = false;

    // Filter state — what phases to show
    let enabledPhases = new Set<TracePhase>([
        'thought', 'tool.start', 'tool.end', 'exec.start', 'exec.end',
        'llm.turn', 'react.reflect', 'plan', 'info',
    ]);
    let scopeToActiveTab = true;
    let showFilters = false;
    let stickyScroll = true;
    let virtualList: VirtualList;

    // No hard cap — the ring buffer in liveTrace.ts is 2000 and we use
    // virtual scrolling so only the viewport + overscan rows exist in DOM.
    $: visible = (() => {
        const list = $liveTrace || [];
        const out: TraceEntry[] = [];
        for (let i = 0; i < list.length; i++) {
            const e = list[i];
            if (!enabledPhases.has(e.phase)) continue;
            if (scopeToActiveTab && activeTabId && e.tabId && e.tabId !== activeTabId) continue;
            out.push(e);
        }
        return out;
    })();

    function onVlScroll(e: CustomEvent) {
        stickyScroll = e.detail.isAtBottom;
    }

    function togglePhase(p: TracePhase) {
        if (enabledPhases.has(p)) enabledPhases.delete(p);
        else                       enabledPhases.add(p);
        enabledPhases = new Set(enabledPhases);  // trigger reactivity
    }

    function phaseIcon(p: TracePhase): string {
        switch (p) {
            case 'thought':       return '◇';
            case 'tool.start':    return '▸';
            case 'tool.end':      return '▪';
            case 'exec.start':    return '▶';
            case 'exec.end':      return '■';
            case 'llm.turn':      return '◯';
            case 'react.reflect': return '↻';
            case 'plan':          return '⊟';
            case 'info':          return '·';
        }
    }
    function phaseColor(p: TracePhase): string {
        switch (p) {
            case 'thought':       return '#a78bfa';
            case 'tool.start':    return '#60a5fa';
            case 'tool.end':      return '#34d399';
            case 'exec.start':    return '#fbbf24';
            case 'exec.end':      return '#10b981';
            case 'llm.turn':      return '#06b6d4';
            case 'react.reflect': return '#f87171';
            case 'plan':          return '#c4b5fd';
            case 'info':          return '#94a3b8';
        }
    }
    function fmtTime(ts: number): string {
        const d = new Date(ts);
        return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
    }
    function fmtElapsed(ms?: number): string {
        if (ms === undefined || ms === null) return '';
        if (ms < 1000) return `${ms}ms`;
        if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
        return `${Math.floor(ms / 60000)}m${Math.round((ms % 60000) / 1000)}s`;
    }

    let expandedDetail = new Set<number>();
    function toggleDetail(id: number) {
        if (expandedDetail.has(id)) expandedDetail.delete(id);
        else                         expandedDetail.add(id);
        expandedDetail = new Set(expandedDetail);
    }

    const ALL_PHASES: TracePhase[] = [
        'thought', 'tool.start', 'tool.end', 'exec.start', 'exec.end',
        'llm.turn', 'react.reflect', 'plan', 'info',
    ];
</script>

{#if open}
<aside class="trace-panel" aria-label="Live agent trace">
    <header class="tp-head">
        <div class="tp-title">
            <span class="tp-ico"><Activity size={14} stroke={2.2}/></span>
            <span>{isEN ? 'Agent trace' : 'Telemetría'}</span>
            <span class="tp-count">{visible.length}</span>
        </div>
        <div class="tp-actions">
            <button class="tp-btn" class:on={showFilters}
                title={isEN ? 'Filters' : 'Filtros'} on:click={() => showFilters = !showFilters}>
                <Filter size={13}/>
            </button>
            <button class="tp-btn" title={isEN ? 'Clear' : 'Limpiar'} on:click={clearTrace}>
                <Trash size={13}/>
            </button>
            <button class="tp-btn close" title={isEN ? 'Close' : 'Cerrar'} on:click={() => open = false}>
                <X size={13}/>
            </button>
        </div>
    </header>

    {#if showFilters}
        <div class="tp-filters">
            <label class="tp-scope">
                <input type="checkbox" bind:checked={scopeToActiveTab}/>
                <span>{isEN ? 'Active tab only' : 'Solo pestaña activa'}</span>
            </label>
            <div class="tp-phases">
                {#each ALL_PHASES as p}
                    <button class="tp-phase-toggle" class:on={enabledPhases.has(p)}
                        style="color:{enabledPhases.has(p) ? phaseColor(p) : 'var(--txt2)'};"
                        on:click={() => togglePhase(p)}>
                        {phaseIcon(p)} {p}
                    </button>
                {/each}
            </div>
        </div>
    {/if}

    {#if visible.length === 0}
        <div class="tp-list">
            <div class="tp-empty">
                {isEN ? 'No events yet. Start a task — Lucy will narrate every step here.' : 'Sin eventos aún. Inicia una tarea — Lucy narrará cada paso aquí.'}
            </div>
        </div>
    {:else}
        <VirtualList
            bind:this={virtualList}
            items={visible}
            itemHeight={26}
            overscan={12}
            stickyBottom={stickyScroll}
            wrapClass="tp-list"
            on:scroll={onVlScroll}
            let:item={e}
        >
            <div class="tp-entry" style="--phase-color:{phaseColor(e.phase)};">
                <span class="tp-time">{fmtTime(e.ts)}</span>
                <span class="tp-icon">{phaseIcon(e.phase)}</span>
                <span class="tp-phase">{e.phase}</span>
                {#if e.step !== undefined}<span class="tp-step">#{e.step}</span>{/if}
                <span class="tp-label" title={e.label}>{e.label}</span>
                {#if e.elapsedMs !== undefined}
                    <span class="tp-elapsed" class:warn={e.elapsedMs > 5000}>{fmtElapsed(e.elapsedMs)}</span>
                {/if}
                {#if e.ok === false}<span class="tp-fail">FAIL{e.exitCode !== undefined && e.exitCode !== null ? `:${e.exitCode}` : ''}</span>{/if}
                {#if e.ok === true}<span class="tp-ok">OK</span>{/if}
                {#if e.detail}
                    <button class="tp-expand" on:click={() => toggleDetail(e.id)}
                        title={isEN ? 'Show detail' : 'Mostrar detalle'}>
                        {#if expandedDetail.has(e.id)}<ChevronUp size={11}/>{:else}<ChevronDown size={11}/>{/if}
                    </button>
                {/if}
            </div>
            {#if e.detail && expandedDetail.has(e.id)}
                <pre class="tp-detail">{e.detail}</pre>
            {/if}
        </VirtualList>
    {/if}

    {#if !stickyScroll}
        <button class="tp-jump" on:click={() => { stickyScroll = true; virtualList?.scrollToBottom(); }}>
            ↓ {isEN ? 'Jump to latest' : 'Ir al último'}
        </button>
    {/if}
</aside>
{/if}

<style>
    .trace-panel {
        position: fixed;
        right: 16px; bottom: 60px; top: 60px;
        width: 380px;
        max-width: calc(100vw - 32px);
        background: rgba(10, 14, 22, 0.96);
        backdrop-filter: blur(8px);
        border: 1px solid rgba(255,255,255,.08);
        border-top: 3px solid #60a5fa;      /* telemetry identity accent bar */
        border-radius: 10px;
        box-shadow: 0 12px 40px -8px rgba(0,0,0,.6),
                    0 0 0 1px rgba(96,165,250,.10) inset,
                    0 -1px 24px -10px rgba(96,165,250,.5);
        display: flex; flex-direction: column;
        z-index: 9000;
        font-size: 11px;
        color: var(--txt);
        /* Honour the "slide-in panel from the right edge" the header docs
           describe — it was never actually animated. */
        animation: tp-slide-in .28s cubic-bezier(.16,1,.3,1);
    }
    @keyframes tp-slide-in { from { opacity: 0; transform: translateX(26px); } to { opacity: 1; transform: none; } }
    .tp-head {
        display: flex; align-items: center; justify-content: space-between;
        padding: 8px 10px;
        border-bottom: 1px solid rgba(255,255,255,.06);
        flex-shrink: 0;
    }
    .tp-title { display: flex; align-items: center; gap: 6px; font-weight: 600; color: var(--txt); }
    .tp-ico {
        display: inline-flex; align-items: center; justify-content: center;
        width: 24px; height: 24px; border-radius: 7px; flex-shrink: 0;
        color: #60a5fa;
        background: rgba(96,165,250,.12);
        border: 1px solid rgba(96,165,250,.28);
        box-shadow: 0 0 12px -4px rgba(96,165,250,.5);
    }
    .tp-count {
        background: rgba(96,165,250,.15); color: var(--accent);
        padding: 1px 6px; border-radius: 8px; font-size: 9px; font-family: var(--mono);
    }
    .tp-actions { display: flex; gap: 4px; }
    .tp-btn {
        background: transparent; border: none; cursor: pointer;
        color: var(--txt2); padding: 4px 6px; border-radius: 4px;
        transition: .15s;
    }
    .tp-btn:hover { background: rgba(255,255,255,.07); color: var(--txt); }
    .tp-btn.on { background: rgba(96,165,250,.12); color: var(--accent); }
    .tp-btn.close:hover { background: rgba(239,68,68,.12); color: #f87171; }

    .tp-filters {
        padding: 8px 10px;
        border-bottom: 1px solid rgba(255,255,255,.06);
        background: rgba(255,255,255,.015);
        flex-shrink: 0;
    }
    .tp-scope {
        display: flex; align-items: center; gap: 6px;
        font-size: 10px; color: var(--txt2);
        cursor: pointer; user-select: none;
        margin-bottom: 6px;
    }
    .tp-phases { display: flex; flex-wrap: wrap; gap: 3px; }
    .tp-phase-toggle {
        background: rgba(255,255,255,.03); border: 1px solid rgba(255,255,255,.05);
        border-radius: 3px; padding: 2px 5px;
        font-size: 9px; font-family: var(--mono);
        cursor: pointer; transition: .15s;
        opacity: .5;
    }
    .tp-phase-toggle.on { opacity: 1; border-color: currentColor; background: rgba(255,255,255,.05); }

    .tp-list {
        flex: 1; overflow-y: auto;
        padding: 4px 0;
        scroll-behavior: smooth;
    }
    .tp-empty {
        padding: 30px 16px;
        color: var(--txt2);
        font-size: 11px; font-style: italic; text-align: center;
    }
    .tp-entry {
        display: flex; align-items: center; gap: 5px;
        padding: 2px 10px;
        border-left: 2px solid var(--phase-color, transparent);
        line-height: 1.4;
        font-family: var(--mono);
    }
    .tp-entry:hover { background: rgba(255,255,255,.025); }
    .tp-time { color: var(--txt2); font-size: 9px; flex-shrink: 0; }
    .tp-icon { color: var(--phase-color); font-size: 11px; flex-shrink: 0; width: 12px; text-align: center; }
    .tp-phase { color: var(--phase-color); font-size: 9px; opacity: .9; flex-shrink: 0; min-width: 60px; }
    .tp-step {
        background: rgba(167,139,250,.12); color: #c4b5fd;
        padding: 0 4px; border-radius: 3px; font-size: 9px; flex-shrink: 0;
    }
    .tp-label {
        flex: 1; min-width: 0;
        font-family: var(--mono); font-size: 11px;
        color: var(--txt);
        overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    }
    .tp-elapsed {
        color: var(--txt2); font-size: 9px; flex-shrink: 0;
        background: rgba(255,255,255,.04);
        padding: 1px 4px; border-radius: 3px;
    }
    .tp-elapsed.warn { color: #fbbf24; background: rgba(251,191,36,.08); }
    .tp-fail { color: #f87171; font-size: 9px; background: rgba(239,68,68,.10); padding: 1px 4px; border-radius: 3px; flex-shrink: 0; }
    .tp-ok { color: #34d399; font-size: 9px; background: rgba(52,211,153,.08); padding: 1px 4px; border-radius: 3px; flex-shrink: 0; }
    .tp-expand {
        background: transparent; border: none; color: var(--txt2);
        cursor: pointer; padding: 2px; border-radius: 3px; flex-shrink: 0;
    }
    .tp-expand:hover { background: rgba(255,255,255,.06); color: var(--txt); }
    .tp-detail {
        margin: 2px 12px 4px 24px;
        padding: 6px 8px;
        background: rgba(0,0,0,.25);
        border-radius: 4px;
        color: var(--txt2);
        font-family: var(--mono); font-size: 10px;
        max-height: 200px; overflow: auto;
        white-space: pre-wrap;
        line-height: 1.4;
    }
    .tp-jump {
        position: absolute; bottom: 6px; left: 50%; transform: translateX(-50%);
        background: rgba(96,165,250,.85); color: white;
        border: none; border-radius: 14px;
        padding: 4px 12px; font-size: 10px; font-weight: 600;
        cursor: pointer; box-shadow: 0 4px 12px -2px rgba(0,0,0,.4);
    }
    .tp-jump:hover { background: rgba(96,165,250,1); }
</style>
