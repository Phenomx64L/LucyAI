<!--
  LiveTraceDock — always-visible activity sparkline.

  Renders a thin vertical 24px strip on the right edge of the chat area
  with 60 buckets representing the last 60 seconds of agent activity.
  Each bucket is a stacked bar colored by the dominant phase in that
  second (LLM blue / tool amber / exec green / error red). The whole
  strip auto-shifts left every second so the bottom = "now".

  Click anywhere → opens the full LiveTracePanel.

  Why this exists: the panel itself is opt-in (FAB or Alt+T). The dock
  gives you a permanent "is Lucy alive?" signal — you can see ticks
  appearing in real time without opening anything. Cursor has a similar
  bar in the bottom-right corner; this is Lucy's version.

  Props:
    activeTabId   — only count events from this tab (others greyed out)
    on:click      — parent opens the full panel
-->
<script lang="ts">
    import { onMount, onDestroy, createEventDispatcher } from 'svelte';
    import { liveTrace, type TraceEntry, type TracePhase } from '$lib/liveTrace';

    export let activeTabId: string = '';
    export let isEN: boolean = false;
    /** Hide entirely when the chat view isn't terminal. */
    export let visible: boolean = true;

    const dispatch = createEventDispatcher<{ click: void }>();

    // ── Bucketing ───────────────────────────────────────────────────────
    // 60 buckets × 1s = the last 60 seconds. We DON'T rely on a regular
    // 1Hz tick to fill them — instead we recompute on every store update
    // AND on a 1s heartbeat so empty seconds still get rendered as gaps.
    const BUCKETS = 60;
    const BUCKET_MS = 1000;

    interface Bucket {
        count: number;
        dominant: TracePhase | null;
        /** number of error-flavoured events for the red overlay */
        errors: number;
    }
    let buckets: Bucket[] = Array.from({ length: BUCKETS }, () => emptyBucket());

    function emptyBucket(): Bucket {
        return { count: 0, dominant: null, errors: 0 };
    }

    let _now = Date.now();

    function recompute() {
        _now = Date.now();
        const start = _now - BUCKETS * BUCKET_MS;
        // Re-init.
        buckets = Array.from({ length: BUCKETS }, () => emptyBucket());
        const phaseCount: Record<number, Map<TracePhase, number>> = {};
        const entries = $liveTrace || [];
        for (const e of entries) {
            if (!e || typeof e.ts !== 'number') continue;
            if (e.ts < start) continue;
            // Optional: scope by tab. If activeTabId is set and the event
            // has a different tabId, skip — keeps the visual focused.
            if (activeTabId && e.tabId && e.tabId !== activeTabId) continue;
            const idx = Math.min(
                BUCKETS - 1,
                Math.max(0, Math.floor((e.ts - start) / BUCKET_MS))
            );
            buckets[idx].count++;
            if (e.ok === false) buckets[idx].errors++;
            const m = phaseCount[idx] ?? (phaseCount[idx] = new Map());
            m.set(e.phase, (m.get(e.phase) ?? 0) + 1);
        }
        for (const [idxStr, m] of Object.entries(phaseCount)) {
            const idx = +idxStr;
            let top: TracePhase | null = null; let topN = 0;
            for (const [p, n] of m) {
                if (n > topN) { top = p; topN = n; }
            }
            buckets[idx].dominant = top;
        }
        buckets = buckets;  // trigger reactivity
    }

    let unsubscribe: (() => void) | null = null;
    let interval: ReturnType<typeof setInterval> | null = null;

    onMount(() => {
        // Recompute on every store change AND every second.
        unsubscribe = liveTrace.subscribe(() => recompute());
        interval = setInterval(recompute, BUCKET_MS);
        recompute();
    });
    onDestroy(() => {
        if (unsubscribe) unsubscribe();
        if (interval) clearInterval(interval);
    });

    // ── Color mapping ───────────────────────────────────────────────────
    function colorFor(b: Bucket): string {
        if (b.count === 0) return 'transparent';
        if (b.errors > 0)  return 'var(--red, #ef4444)';
        switch (b.dominant) {
            case 'thought':      return '#a78bfa';  // violet — reasoning
            case 'llm.turn':     return 'var(--blue, #3b9eff)';
            case 'tool.start':
            case 'tool.end':     return '#f59e0b';
            case 'exec.start':
            case 'exec.end':     return 'var(--acc, #10b981)';
            case 'react.reflect':return '#ec4899';
            case 'plan':         return '#fbbf24';
            case 'info':         return 'var(--txt3, #475569)';
            default:             return 'var(--txt3, #475569)';
        }
    }

    // Height of each bar scaled to bucket count (log-ish so a 20-event
    // burst doesn't dwarf the steady 1-2 events/sec baseline).
    function heightPct(b: Bucket): number {
        if (b.count === 0) return 0;
        const v = Math.log2(b.count + 1) / Math.log2(20);  // 1 → 0.23, 5 → 0.62, 20 → 1.0
        return Math.min(100, Math.max(8, v * 100));
    }

    // Total events in window — used for the "alive" pulse animation.
    $: total = buckets.reduce((a, b) => a + b.count, 0);

    function onClick() { dispatch('click'); }
</script>

{#if visible}
<div class="lt-dock"
     role="button" tabindex="0"
     title={isEN
        ? `Agent activity — last 60s · ${total} events · click for details`
        : `Actividad del agente — últimos 60s · ${total} eventos · clic para detalle`}
     aria-label={isEN ? 'Open live trace' : 'Abrir traza en vivo'}
     on:click={onClick}
     on:keydown={(e) => { if (e.key==='Enter' || e.key===' ') { e.preventDefault(); onClick(); } }}>

    <!-- Header tick — pulses when activity is happening "now". -->
    <div class="lt-dock-head" class:lt-dock-live={buckets[BUCKETS-1].count > 0}>
        <span class="lt-dot"></span>
    </div>

    <!-- Bars: bucket 0 (oldest) at top, bucket 59 (now) at bottom. -->
    <div class="lt-dock-bars">
        {#each buckets as b, i (i)}
            <div class="lt-bar"
                 style="background:{colorFor(b)}; height:{heightPct(b)}%; opacity:{b.count > 0 ? 0.85 : 0};">
            </div>
        {/each}
    </div>

    <!-- Count footer — total events in the visible window. -->
    <div class="lt-dock-foot">
        {#if total > 0}<span class="lt-count">{total}</span>{/if}
    </div>
</div>
{/if}

<style>
    /* The dock sits at the right edge of the chat container, full height
       minus a small inset so it doesn't overlap the tab strip or the
       input bar. Width is intentionally tiny (22px) — it's an at-a-glance
       indicator, not a full panel. */
    .lt-dock {
        position: absolute;
        top: 8px; bottom: 8px; right: 6px;
        width: 22px;
        background: rgba(0, 0, 0, 0.18);
        border: 1px solid rgba(255, 255, 255, 0.05);
        border-radius: 8px;
        display: flex;
        flex-direction: column;
        align-items: stretch;
        cursor: pointer;
        transition: background .15s, border-color .15s;
        z-index: 10;
        backdrop-filter: blur(2px);
        user-select: none;
    }
    .lt-dock:hover {
        background: rgba(0, 0, 0, 0.28);
        border-color: rgba(16, 185, 129, 0.30);
    }
    .lt-dock-head {
        height: 14px;
        display: flex; align-items: center; justify-content: center;
        border-bottom: 1px solid rgba(255,255,255,0.04);
    }
    .lt-dot {
        width: 6px; height: 6px; border-radius: 50%;
        background: var(--txt3, #475569);
        transition: background .2s, box-shadow .2s;
    }
    .lt-dock-live .lt-dot {
        background: var(--acc, #10b981);
        box-shadow: 0 0 6px rgba(16, 185, 129, 0.55);
        animation: lt-pulse 1s ease-in-out infinite;
    }
    @keyframes lt-pulse {
        0%,100% { transform: scale(1);  }
        50%     { transform: scale(1.35); }
    }
    .lt-dock-bars {
        flex: 1;
        display: flex;
        flex-direction: column;
        gap: 0;
        padding: 2px 4px;
        overflow: hidden;
    }
    /* Each bar is rendered as a horizontal strip stacked vertically.
       Visual goal: a thin stripe-meter, NOT a bar chart — so width is
       100% and height varies. */
    .lt-bar {
        width: 100%;
        flex-shrink: 0;
        min-height: 1px;
        margin-bottom: 1px;
        border-radius: 1px;
        transition: opacity .12s linear, height .12s linear, background .12s linear;
    }
    .lt-dock-foot {
        height: 16px;
        font-family: var(--mono, monospace);
        font-size: 8.5px;
        color: var(--txt3, #475569);
        display: flex; align-items: center; justify-content: center;
        border-top: 1px solid rgba(255,255,255,0.04);
    }
    .lt-count { font-weight: 700; opacity: 0.7; }
</style>
