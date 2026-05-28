<!--
  ActivityFeedWidget — Sprint 1, UI-1

  Sidebar widget that aggregates "what happened" in Lucy in the last N hours.
  Pulls from the backend command `activity_feed` (incidents, audit_trail,
  scheduled_runs, snapshots, lineage, frontier telemetry).

  Why this exists: when the user opens Lucy in the morning, they want a
  one-glance answer to "did anything weird happen overnight?" before they
  start typing. Previously this required hunting across 4 different views.

  Design:
    • Header pill with counters (X incidents · Y commands · Z schedules).
    • Body: chronological list of events, newest first, capped at 20.
    • Auto-refresh every 5 min while the sidebar is visible. The component
      also calls `refresh()` whenever the user expands it.
    • Each event row dispatches a `navigate` event with `target_view` so
      the parent can route to the relevant module.
    • Persisted collapsed-state to localStorage (`lucy_activity_open`).
-->
<script lang="ts">
    import { onMount, onDestroy, createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { safeGetLS, safeSetLSString } from '$lib/safe-ls';
    import { relTime, sevClass, kindIcon } from '$lib/activity-feed-helpers';

    export let isEN: boolean = false;
    /** Whether the parent sidebar is collapsed to icons. When true, hide details. */
    export let sidebarCollapsed: boolean = false;

    const dispatch = createEventDispatcher<{
        navigate: { view: string; id?: string };
    }>();

    // Shape from backend (see activity_feed.rs)
    interface ActivityEvent {
        kind: string;
        severity: string;
        title: string;
        detail: string;
        ts: number;
        target_view: string | null;
        target_id: string | null;
    }
    interface ActivityCounters {
        incidents_open: number;
        incidents_resolved: number;
        schedules_run: number;
        schedules_failed: number;
        commands_run: number;
        snapshots: number;
        frontier_calls: number;
    }
    interface ActivityFeedData {
        since_ts: number;
        now_ts: number;
        events: ActivityEvent[];
        summary: ActivityCounters;
    }

    let data: ActivityFeedData | null = null;
    let loading = false;
    let error = '';
    /** Persisted expand state — defaults to OPEN so first-run users see the value. */
    let panelOpen: boolean = safeGetLS('lucy_activity_open', '1') !== '0';
    let refreshTimer: ReturnType<typeof setInterval> | null = null;

    /** Refresh cadence — 5 min is enough; this widget is observational, not real-time. */
    const REFRESH_MS = 5 * 60 * 1000;
    const HOURS_WINDOW = 24;

    async function refresh() {
        if (loading) return;
        loading = true;
        error = '';
        try {
            const r = await invoke<ActivityFeedData>('activity_feed', { hours: HOURS_WINDOW });
            data = r;
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    }

    function togglePanel() {
        panelOpen = !panelOpen;
        safeSetLSString('lucy_activity_open', panelOpen ? '1' : '0');
        // Lazy first-load: only fetch when the user actually expands it.
        if (panelOpen && !data) void refresh();
    }

    function onEventClick(ev: ActivityEvent) {
        if (ev.target_view) {
            dispatch('navigate', { view: ev.target_view, id: ev.target_id || undefined });
        }
    }

    // Helpers (relTime, sevClass, kindIcon) live in $lib/activity-feed-helpers
    // so vitest can exercise them directly. Logic is identical — this file
    // just consumes them.

    onMount(() => {
        // Always do the initial fetch — the header pill needs counters even
        // if the body stays collapsed.
        void refresh();
        refreshTimer = setInterval(() => { void refresh(); }, REFRESH_MS);
    });

    onDestroy(() => {
        if (refreshTimer) { clearInterval(refreshTimer); refreshTimer = null; }
    });

    // Compact summary string for the collapsed/header state.
    $: counters = data?.summary;
    $: hasOpenIncidents = (counters?.incidents_open ?? 0) > 0;
    $: hasFailedSchedules = (counters?.schedules_failed ?? 0) > 0;
    $: badgeText = (() => {
        if (!counters) return '';
        const bits: string[] = [];
        if (counters.incidents_open > 0)   bits.push(`${counters.incidents_open} ${isEN ? 'open' : 'abierto'}`);
        if (counters.incidents_resolved > 0) bits.push(`${counters.incidents_resolved} ${isEN ? 'resolved' : 'resuelto'}`);
        if (bits.length === 0 && counters.commands_run > 0) bits.push(`${counters.commands_run} cmds`);
        return bits.join(' · ');
    })();
</script>

{#if !sidebarCollapsed}
<div class="af-widget" class:af-alert={hasOpenIncidents || hasFailedSchedules}>
    <!-- Header — clickable to toggle the body, badge on the right. -->
    <button class="af-hdr" type="button"
            on:click={togglePanel}
            title={isEN ? 'Activity in the last 24 hours' : 'Actividad de las últimas 24 horas'}>
        <span class="af-hdr-glyph">⌒</span>
        <span class="af-hdr-title">{isEN ? 'Activity (24h)' : 'Actividad (24h)'}</span>
        {#if badgeText}
            <span class="af-hdr-badge" class:af-hdr-badge-alert={hasOpenIncidents || hasFailedSchedules}>
                {badgeText}
            </span>
        {/if}
        <span class="af-hdr-chev" class:open={panelOpen}>{panelOpen ? '▾' : '▸'}</span>
    </button>

    {#if panelOpen}
        <div class="af-body">
            {#if loading && !data}
                <div class="af-empty">{isEN ? 'Loading…' : 'Cargando…'}</div>
            {:else if error}
                <div class="af-empty af-err" title={error}>
                    {isEN ? 'Error loading' : 'Error al cargar'}
                </div>
            {:else if !data || data.events.length === 0}
                <div class="af-empty">
                    {isEN ? 'Nothing in the last 24h.' : 'Sin actividad en 24h.'}
                </div>
            {:else}
                {#each data.events as ev (ev.kind + ev.ts + ev.title)}
                    <button class="af-row {sevClass(ev.severity)}"
                            class:af-clickable={!!ev.target_view}
                            type="button"
                            on:click={() => onEventClick(ev)}
                            title={ev.detail || ev.title}>
                        <span class="af-row-glyph">{kindIcon(ev.kind)}</span>
                        <span class="af-row-title">{ev.title}</span>
                        <span class="af-row-time">{relTime(ev.ts, data.now_ts)}</span>
                    </button>
                {/each}
            {/if}
            <button class="af-refresh" type="button" on:click={refresh} disabled={loading}>
                {loading ? '…' : (isEN ? '↻ Refresh' : '↻ Actualizar')}
            </button>
        </div>
    {/if}
</div>
{/if}

<style>
    .af-widget {
        margin: 8px 6px 4px;
        padding: 0;
        background: rgba(255,255,255,0.018);
        border: 1px solid rgba(255,255,255,0.04);
        border-radius: 6px;
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace);
        font-size: 11px;
        line-height: 1.45;
    }
    /* When there's something to attend to, a soft amber tint on the whole widget */
    .af-widget.af-alert {
        background: rgba(245, 158, 11, 0.035);
        border-color: rgba(245, 158, 11, 0.18);
    }

    /* Header — clickable, button-shaped but visually a label */
    .af-hdr {
        display: flex;
        align-items: center;
        gap: 6px;
        width: 100%;
        padding: 7px 10px;
        background: transparent;
        border: 0;
        border-radius: 6px;
        color: var(--text-muted, #94a3b8);
        font-family: inherit;
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.5px;
        text-transform: uppercase;
        cursor: pointer;
        transition: background 140ms ease;
    }
    .af-hdr:hover { background: rgba(255,255,255,0.03); }
    .af-hdr-glyph { color: var(--accent, #10b981); font-size: 11px; }
    .af-hdr-title { flex: 1; text-align: left; }
    .af-hdr-badge {
        font-family: var(--font-mono);
        font-size: 9px;
        padding: 1px 6px;
        border-radius: 9px;
        background: rgba(255,255,255,0.05);
        color: var(--text-main, #cbd5e1);
        text-transform: none;
        letter-spacing: 0.2px;
    }
    .af-hdr-badge-alert {
        background: rgba(245, 158, 11, 0.16);
        color: #f59e0b;
    }
    .af-hdr-chev {
        font-size: 9px;
        color: var(--text-muted);
        transition: transform 140ms ease;
    }
    .af-hdr-chev.open { transform: rotate(0); }

    /* Body */
    .af-body {
        padding: 4px 6px 6px;
        display: flex; flex-direction: column;
        gap: 2px;
        max-height: 240px;
        overflow-y: auto;
    }
    .af-body::-webkit-scrollbar { width: 4px; }
    .af-body::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.10); border-radius: 2px; }

    .af-empty {
        padding: 10px 8px;
        text-align: center;
        color: var(--text-muted);
        font-size: 10px;
        font-style: italic;
    }
    .af-err { color: #ef4444; }

    /* Event rows */
    .af-row {
        display: grid;
        grid-template-columns: 14px 1fr auto;
        gap: 6px;
        align-items: baseline;
        width: 100%;
        padding: 4px 6px;
        background: transparent;
        border: 0;
        border-left: 2px solid transparent;
        border-radius: 3px;
        color: var(--text-main, #cbd5e1);
        font-family: inherit;
        font-size: 11px;
        text-align: left;
        cursor: default;
        transition: background 120ms ease, border-color 120ms ease;
    }
    .af-row.af-clickable { cursor: pointer; }
    .af-row:hover { background: rgba(255,255,255,0.04); }
    .af-row-glyph {
        font-size: 11px;
        opacity: 0.8;
        text-align: center;
    }
    .af-row-title {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .af-row-time {
        font-size: 9px;
        color: var(--text-muted);
        font-variant-numeric: tabular-nums;
    }
    .sev-error { border-left-color: rgba(239, 68, 68, 0.45); }
    .sev-error .af-row-glyph { color: #ef4444; }
    .sev-warn  { border-left-color: rgba(245, 158, 11, 0.45); }
    .sev-warn  .af-row-glyph { color: #f59e0b; }
    .sev-ok    { border-left-color: rgba(16, 185, 129, 0.40); }
    .sev-ok    .af-row-glyph { color: var(--accent, #10b981); }
    .sev-info  { border-left-color: rgba(148, 163, 184, 0.25); }
    .sev-info  .af-row-glyph { color: var(--text-muted); }

    .af-refresh {
        margin-top: 4px;
        padding: 3px 8px;
        background: transparent;
        border: 1px solid rgba(255,255,255,0.06);
        border-radius: 3px;
        color: var(--text-muted);
        font-family: inherit;
        font-size: 9px;
        text-align: center;
        cursor: pointer;
        transition: background 120ms ease, color 120ms ease;
    }
    .af-refresh:hover:not(:disabled) {
        background: rgba(255,255,255,0.04);
        color: var(--text-main);
    }
    .af-refresh:disabled { opacity: 0.5; cursor: default; }
</style>
