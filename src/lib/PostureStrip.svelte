<script lang="ts">
    // ── PostureStrip — Always-on horizontal status bar (Lucy Gen 2) ────────
    //
    // Single-line summary of every connected host. Lives above the main
    // content area so the operator never has to switch tabs to know whether
    // anything is on fire. Click a host to drill into the full Posture view.
    //
    // Each row shows (compact, monospace):
    //   ● status-dot   hostname   CPU%   RAM%   ↓in ↑out   CIS:N(if errors)
    //
    // Color of the status-dot uses the semantic palette:
    //   --sem-ok    → online and healthy (cpu<60%, ram<70%)
    //   --sem-op    → online with mild load (no alerts)
    //   --sem-warn  → high load OR compliance warnings
    //   --sem-crit  → offline OR compliance errors
    //
    // This component is INTENTIONALLY low-frequency-update. It reads from a
    // shared store updated by the Dashboard's refresh cycle (or by separate
    // periodic pings if Dashboard is closed). Avoid making it pull metrics
    // directly — duplicate network traffic for the same data is wasted.

    import { createEventDispatcher } from 'svelte';

    export interface HostSnapshot {
        id: string;
        name: string;
        /** 'online' | 'offline' | 'unknown' */
        status: string;
        cpu?: number;
        ram?: number;
        netIn?: number;  // MB/s
        netOut?: number; // MB/s
        cisErrors?: number;
    }

    /** List of hosts to summarize. Pass an empty array to render nothing. */
    export let hosts: HostSnapshot[] = [];
    /** When true, the strip collapses to icons only on narrow viewports. */
    export let compact: boolean = false;

    const dispatch = createEventDispatcher<{ hostclick: { hostId: string } }>();

    /**
     * Semantic severity → drives the dot color.
     *
     *   crit (red)   — host is offline / has CIS errors
     *   mute (gray)  — host never reached or polled (unknown). Don't fake green.
     *   warn (amber) — cpu>=85 or ram>=90
     *   op   (blue)  — cpu>=60 or ram>=70 (medium load)
     *   ok   (green) — healthy and recently confirmed online
     */
    function severity(h: HostSnapshot): 'ok' | 'op' | 'warn' | 'crit' | 'mute' {
        if (h.status === 'offline') return 'crit';
        if (h.status !== 'online')  return 'mute';   // unknown / null / undefined
        if ((h.cisErrors ?? 0) > 0) return 'crit';
        if ((h.cpu ?? 0) >= 85 || (h.ram ?? 0) >= 90) return 'warn';
        if ((h.cpu ?? 0) >= 60 || (h.ram ?? 0) >= 70) return 'op';
        return 'ok';
    }

    function fmtPct(v: number | undefined): string {
        if (v == null) return '–';
        return `${v.toFixed(0)}%`;
    }
    function fmtNet(v: number | undefined): string {
        if (v == null) return '–';
        if (v < 1) return `${(v * 1024).toFixed(0)}K`;
        return `${v.toFixed(1)}M`;
    }
</script>

{#if hosts.length > 0}
    <div class="posture-strip" class:compact role="region" aria-label="Posture summary">
        {#each hosts as h (h.id)}
            <button class="ps-host ps-{severity(h)}"
                    on:click={() => dispatch('hostclick', { hostId: h.id })}
                    title={`${h.name} · click to inspect`}>
                <span class="ps-dot"></span>
                <span class="ps-name">{h.name}</span>
                {#if !compact && h.status === 'online'}
                    <span class="ps-metric">
                        <span class="ps-mlabel">CPU</span>
                        <span class="ps-mval">{fmtPct(h.cpu)}</span>
                    </span>
                    <span class="ps-metric">
                        <span class="ps-mlabel">RAM</span>
                        <span class="ps-mval">{fmtPct(h.ram)}</span>
                    </span>
                    {#if h.netIn != null || h.netOut != null}
                        <span class="ps-metric ps-net">
                            <span class="ps-mlabel">↓{fmtNet(h.netIn)}</span>
                            <span class="ps-mlabel">↑{fmtNet(h.netOut)}</span>
                        </span>
                    {/if}
                {/if}
                {#if (h.cisErrors ?? 0) > 0}
                    <span class="ps-cis" title="CIS compliance failures">CIS:{h.cisErrors}</span>
                {/if}
                {#if h.status === 'offline'}
                    <span class="ps-offline">OFFLINE</span>
                {:else if h.status !== 'online'}
                    <span class="ps-unknown" title="No metrics yet — open Dashboard to ping this host">?</span>
                {/if}
            </button>
        {/each}
    </div>
{/if}

<style>
    .posture-strip {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 12px;
        padding: 6px 14px;
        background: rgba(255,255,255,0.018);
        border-bottom: 1px solid rgba(255,255,255,0.05);
        font-family: var(--mono, monospace);
        font-size: 11px;
        line-height: 1;
        user-select: none;
    }
    .ps-host {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 4px 10px 4px 8px;
        background: rgba(255,255,255,0.02);
        border: 1px solid rgba(255,255,255,0.05);
        border-radius: 6px;
        cursor: pointer;
        color: var(--txt2, #94a3b8);
        font-family: inherit;
        font-size: inherit;
        line-height: 1;
        transition: background .15s ease, border-color .15s ease, transform .15s ease;
    }
    .ps-host:hover {
        background: rgba(255,255,255,0.05);
        transform: translateY(-1px);
    }
    .ps-dot {
        width: 7px;
        height: 7px;
        border-radius: 50%;
        flex-shrink: 0;
    }
    .ps-name {
        font-weight: 600;
        color: var(--txt, #e2e8f0);
        letter-spacing: .3px;
    }
    .ps-metric {
        display: inline-flex;
        align-items: baseline;
        gap: 3px;
    }
    .ps-mlabel {
        font-size: 9px;
        color: var(--txt3, #64748b);
        text-transform: uppercase;
        letter-spacing: .5px;
    }
    .ps-mval {
        color: var(--txt2, #94a3b8);
        font-weight: 500;
    }
    .ps-net {
        color: var(--sem-op, #5ec8ff);
    }
    .ps-cis {
        background: var(--sem-crit-dim, rgba(239,68,68,.08));
        color: var(--sem-crit, #ef4444);
        border: 1px solid var(--sem-crit-border, rgba(239,68,68,.22));
        padding: 1px 6px;
        border-radius: 3px;
        font-size: 9px;
        font-weight: 700;
        letter-spacing: .3px;
    }
    .ps-offline {
        color: var(--sem-crit, #ef4444);
        font-weight: 700;
        font-size: 9px;
        letter-spacing: 1px;
    }

    /* Severity-driven dot colors — feeds from the semantic palette */
    .ps-ok   .ps-dot { background: var(--sem-ok,   #10b981); box-shadow: var(--sem-ok-glow,   0 0 5px rgba(16,185,129,.5)); }
    .ps-op   .ps-dot { background: var(--sem-op,   #5ec8ff); box-shadow: var(--sem-op-glow,   0 0 5px rgba(94,200,255,.5)); }
    .ps-warn .ps-dot { background: var(--sem-warn, #f59e0b); box-shadow: var(--sem-warn-glow, 0 0 5px rgba(245,158,11,.5)); animation: ps-pulse 2s ease-in-out infinite; }
    .ps-crit .ps-dot { background: var(--sem-crit, #ef4444); box-shadow: var(--sem-crit-glow, 0 0 5px rgba(239,68,68,.6));  animation: ps-pulse 1.2s ease-in-out infinite; }
    /* Muted state — host never polled. NO glow, NO pulse, no green deception. */
    .ps-mute .ps-dot { background: rgba(148,163,184,.45); box-shadow: none; }
    .ps-mute        { opacity: .65; }

    .ps-unknown {
        color: var(--txt3, #64748b);
        font-weight: 700;
        font-size: 10px;
        letter-spacing: .5px;
        padding: 1px 5px;
        border-radius: 3px;
        background: rgba(148,163,184,.08);
        border: 1px solid rgba(148,163,184,.15);
    }

    /* Hover border picks up the severity color */
    .ps-warn:hover { border-color: var(--sem-warn-border, rgba(245,158,11,.3)); }
    .ps-crit:hover { border-color: var(--sem-crit-border, rgba(239,68,68,.3)); }

    @keyframes ps-pulse {
        0%, 100% { transform: scale(1);   opacity: 1;   }
        50%      { transform: scale(1.25); opacity: .75; }
    }

    @media (prefers-reduced-motion: reduce) {
        .ps-warn .ps-dot, .ps-crit .ps-dot { animation: none; }
        .ps-host:hover { transform: none; }
    }

    /* Compact mode for narrow viewports — drops metrics, keeps just name + dot */
    .posture-strip.compact .ps-metric,
    .posture-strip.compact .ps-net {
        display: none;
    }
</style>
