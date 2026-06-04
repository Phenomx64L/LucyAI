<script lang="ts">
    // ── MissionStrip — Always-on operational pulse (v1.7.58, Direction A1) ───
    //
    // The horizontal band that lives between the title bar and the tab strip.
    // Communicates at a glance, without the operator having to look anywhere
    // else, the four signals an IT pro cares about in their peripheral vision:
    //
    //   ● local host (Lucy is up and watching this box)
    //   ⚯ remote host count (how many hosts you're managing via WinRM/SSH)
    //   ⚠ active alert / incident count
    //   ⊕ guard / security skill state
    //   HH:MM local time
    //   ●●●●● five-dot posture (calm → vigilant → suspicious → alarmed → panic)
    //
    // Design goals
    // ────────────
    //   • Always visible. The operator never has to switch tabs or open a
    //     panel to know whether the box is healthy.
    //   • Low motion. Subtle heartbeat on the local-host dot (3.6s cycle) so
    //     the band reads as "alive" but doesn't pull attention.
    //   • Single line, fixed height, monospace numeric. Same metaphor as the
    //     status line of tmux / htop / Splunk SOC console.
    //   • Click any chip → drills into the relevant view (incidents tab, host
    //     dashboard, security-skill picker, etc).
    //   • Cost: near-zero. No polling, no Tauri invokes, no setInterval at
    //     <1Hz. All data comes from props or a tick(60s) for the clock.

    import { createEventDispatcher, onMount, onDestroy } from 'svelte';

    /** Local machine display name (typically `lucyConfig.name` or os hostname). */
    export let localHost: string = 'LOCAL';
    /** Number of remote hosts configured (from $hosts). */
    export let remoteHostsTotal: number = 0;
    /** How many of those are currently online (from $hostReachability). */
    export let remoteHostsOnline: number = 0;
    /** Number of active alerts / incidents (0 = clean). */
    export let activeAlerts: number = 0;
    /** Security skill / guard state, free-form short label. Empty = clean. */
    export let guardLabel: string = '';
    /** Five-dot posture (0..4) — calm, vigilant, suspicious, alarmed, panic. */
    export let posture: 0 | 1 | 2 | 3 | 4 = 0;
    /** Optional override for the time string (mostly for tests / replay). */
    export let nowOverride: string | null = null;
    /** EN / ES copy switch — defaults to ES because that's Lucy's main locale. */
    export let isEN: boolean = false;

    const dispatch = createEventDispatcher<{
        clickLocal:  void;
        clickHosts:  void;
        clickAlerts: void;
        clickGuard:  void;
        clickPosture: void;
    }>();

    // ── Local clock ──────────────────────────────────────────────────────────
    // Updates once per minute. Aligned to the next minute boundary on mount so
    // the second hop is precise instead of "starting at some offset". We
    // deliberately avoid Date.now() polling — once a minute is plenty for an
    // ops band.
    let _now: string = '';
    let _timer: ReturnType<typeof setInterval> | null = null;

    function _formatNow(): string {
        if (nowOverride) return nowOverride;
        const d = new Date();
        const hh = String(d.getHours()).padStart(2, '0');
        const mm = String(d.getMinutes()).padStart(2, '0');
        return `${hh}:${mm}`;
    }

    onMount(() => {
        _now = _formatNow();
        const msUntilNextMinute = (60 - new Date().getSeconds()) * 1000;
        const _bootstrap = setTimeout(() => {
            _now = _formatNow();
            _timer = setInterval(() => { _now = _formatNow(); }, 60_000);
        }, msUntilNextMinute);
        return () => clearTimeout(_bootstrap);
    });
    onDestroy(() => { if (_timer) clearInterval(_timer); });

    // ── Severity → CSS class mapping ─────────────────────────────────────────
    $: hostsSeverity =
        remoteHostsTotal === 0                             ? 'ms-mute'
        : remoteHostsOnline === remoteHostsTotal           ? 'ms-ok'
        : remoteHostsOnline === 0                          ? 'ms-crit'
        : 'ms-warn';

    $: alertsSeverity =
        activeAlerts === 0                                  ? 'ms-ok'
        : activeAlerts === 1                                ? 'ms-warn'
        : 'ms-crit';

    $: guardSeverity = guardLabel ? 'ms-op' : 'ms-mute';
</script>

<div class="mission-strip" role="region" aria-label="Mission status">
    <!-- Local host LED — always green when Lucy is running. The slow heartbeat
         pulse on this dot is what makes the band read as "alive". -->
    <button class="ms-chip ms-local"
            on:click={() => dispatch('clickLocal')}
            title={isEN ? 'This machine (click for diagnostics)' : 'Esta máquina (click para diagnóstico)'}>
        <span class="ms-dot ms-dot-heartbeat ms-ok-dot" aria-hidden="true"></span>
        <span class="ms-host">{localHost}</span>
    </button>

    <span class="ms-sep" aria-hidden="true">·</span>

    <!-- Remote hosts chip — hidden when no hosts are configured to avoid noise. -->
    {#if remoteHostsTotal > 0}
        <button class="ms-chip {hostsSeverity}"
                on:click={() => dispatch('clickHosts')}
                title={isEN
                    ? `${remoteHostsOnline}/${remoteHostsTotal} remote hosts online`
                    : `${remoteHostsOnline}/${remoteHostsTotal} hosts remotos en línea`}>
            <span class="ms-glyph">⚯</span>
            <span class="ms-val">{remoteHostsOnline}/{remoteHostsTotal}</span>
            <span class="ms-lbl">{isEN ? 'hosts' : 'hosts'}</span>
        </button>
        <span class="ms-sep" aria-hidden="true">·</span>
    {/if}

    <!-- Active alerts / incidents. ms-ok when 0 reads as "clean". -->
    <button class="ms-chip {alertsSeverity}"
            on:click={() => dispatch('clickAlerts')}
            title={isEN
                ? `${activeAlerts} active incident${activeAlerts === 1 ? '' : 's'}`
                : `${activeAlerts} incidente${activeAlerts === 1 ? '' : 's'} activo${activeAlerts === 1 ? '' : 's'}`}>
        <span class="ms-glyph">⚠</span>
        <span class="ms-val">{activeAlerts}</span>
        <span class="ms-lbl">{isEN ? 'alerts' : 'alertas'}</span>
    </button>

    <span class="ms-sep" aria-hidden="true">·</span>

    <!-- Security skill / guard status. When no skill active, dim mute. -->
    <button class="ms-chip {guardSeverity}"
            on:click={() => dispatch('clickGuard')}
            title={isEN
                ? (guardLabel ? `Active security skill: ${guardLabel}` : 'No security skill active — guard clean')
                : (guardLabel ? `Skill de seguridad activo: ${guardLabel}` : 'Sin skill de seguridad activo — guard limpio')}>
        <span class="ms-glyph">⊕</span>
        <span class="ms-val">{guardLabel || (isEN ? 'clean' : 'limpio')}</span>
    </button>

    <span class="ms-sep" aria-hidden="true">·</span>

    <!-- Local clock — bottom-right of the band conventionally, but we keep it
         inline with the rest so the eye can scan the whole status line in
         one left-to-right sweep. -->
    <span class="ms-chip ms-clock" aria-label={isEN ? 'Local time' : 'Hora local'}>
        <span class="ms-clock-glyph">◷</span>
        <span class="ms-val">{_now}</span>
    </span>

    <!-- Spacer pushes the posture indicator to the right edge. -->
    <span class="ms-flex" aria-hidden="true"></span>

    <!-- Five-dot posture / stance.
         Posture levels:
           0 ●○○○○  calm        (default)
           1 ●●○○○  vigilant    (a query is in flight)
           2 ●●●○○  suspicious  (a TOOL/EXECUTE in progress)
           3 ●●●●○  alarmed     (active incident)
           4 ●●●●●  panic       (multiple incidents OR repeated guard hits)
         The colour shifts up the severity ladder along with the count. -->
    <button class="ms-chip ms-posture ms-posture-{posture}"
            on:click={() => dispatch('clickPosture')}
            title={isEN
                ? `Lucy posture: ${['calm','vigilant','suspicious','alarmed','panic'][posture]}`
                : `Postura: ${['tranquila','vigilante','sospechosa','alarmada','pánico'][posture]}`}>
        {#each [0, 1, 2, 3, 4] as i}
            <span class="ms-pdot" class:on={i <= posture}></span>
        {/each}
    </button>
</div>

<style>
    .mission-strip {
        display: flex;
        align-items: center;
        gap: 4px;
        height: 22px;
        padding: 0 14px;
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 10.5px;
        line-height: 1;
        background: linear-gradient(
            180deg,
            rgba(8, 14, 24, 0.55) 0%,
            rgba(8, 14, 24, 0.40) 100%
        );
        border-bottom: 1px solid rgba(255, 255, 255, 0.04);
        color: var(--txt3, #94a3b8);
        user-select: none;
        flex-shrink: 0;
        position: relative;
        z-index: 4;
    }

    /* Base chip — used by every cell except the spacers. Buttons are reset
       so the chip blends into the band. Hover: subtle row highlight. */
    .ms-chip {
        appearance: none;
        background: transparent;
        border: none;
        padding: 2px 6px;
        border-radius: 4px;
        font: inherit;
        color: inherit;
        cursor: pointer;
        display: inline-flex;
        align-items: center;
        gap: 4px;
        transition: background-color 120ms ease, color 120ms ease;
    }
    .ms-chip:hover {
        background: rgba(255, 255, 255, 0.04);
        color: var(--txt2, #cbd5e1);
    }
    .ms-clock { cursor: default; }
    .ms-clock:hover { background: transparent; color: inherit; }

    .ms-sep   { color: rgba(255, 255, 255, 0.10); padding: 0 2px; }
    .ms-flex  { flex: 1 1 auto; }

    .ms-host  { font-weight: 600; letter-spacing: 0.2px; }
    .ms-glyph { font-size: 11px; opacity: 0.7; }
    .ms-val   { font-variant-numeric: tabular-nums; font-weight: 600; }
    .ms-lbl   { opacity: 0.55; }
    .ms-clock-glyph { font-size: 11px; opacity: 0.55; }

    /* Local-host dot heartbeat. Slow & quiet — establishes "alive" without
       pulling focus. Stops under .lucy-quiescent (v1.7.44 idle saver). */
    .ms-dot {
        width: 6px; height: 6px; border-radius: 50%;
        flex-shrink: 0;
        background: var(--acc, #10b981);
        box-shadow: 0 0 4px rgba(16, 185, 129, 0.50);
    }
    .ms-dot-heartbeat { animation: ms-pulse 3.6s ease-in-out infinite; }
    @keyframes ms-pulse {
        0%, 100% {
            box-shadow: 0 0 4px rgba(16, 185, 129, 0.45),
                        0 0 0  0   rgba(16, 185, 129, 0.55);
        }
        50% {
            box-shadow: 0 0 8px rgba(16, 185, 129, 0.65),
                        0 0 0  4px rgba(16, 185, 129, 0.00);
        }
    }

    /* Semantic state classes — drive colour of glyph + val.
       Matches Lucy's existing sem-* palette (sb-led-* in StatusBar). */
    .ms-ok    .ms-glyph, .ms-ok    .ms-val { color: var(--acc, #10b981); }
    .ms-op    .ms-glyph, .ms-op    .ms-val { color: #60a5fa; }
    .ms-warn  .ms-glyph, .ms-warn  .ms-val { color: var(--amber, #f59e0b); }
    .ms-crit  .ms-glyph, .ms-crit  .ms-val { color: var(--red, #ef4444); }
    .ms-mute  .ms-glyph, .ms-mute  .ms-val { color: var(--txt3, #64748b); opacity: 0.85; }

    .ms-ok-dot   { background: var(--acc, #10b981); }

    /* Posture cluster — five tiny dots, colour ramps with severity. */
    .ms-posture {
        gap: 3px;
        padding: 0 6px;
    }
    .ms-pdot {
        width: 5px; height: 5px; border-radius: 50%;
        background: rgba(255, 255, 255, 0.10);
        flex-shrink: 0;
        transition: background-color 200ms ease, box-shadow 200ms ease;
    }
    .ms-posture-0 .ms-pdot.on { background: var(--acc, #10b981); box-shadow: 0 0 3px rgba(16,185,129,.5); }
    .ms-posture-1 .ms-pdot.on { background: #60a5fa;             box-shadow: 0 0 3px rgba(96,165,250,.5); }
    .ms-posture-2 .ms-pdot.on { background: var(--amber, #f59e0b); box-shadow: 0 0 3px rgba(245,158,11,.55); }
    .ms-posture-3 .ms-pdot.on { background: #fb923c;             box-shadow: 0 0 4px rgba(251,146,60,.6); }
    .ms-posture-4 .ms-pdot.on { background: var(--red, #ef4444);  box-shadow: 0 0 5px rgba(239,68,68,.7); }

    /* Idle / hidden state from v1.7.44 — pause the heartbeat to conserve GPU. */
    :global(html.app-hidden)     .ms-dot-heartbeat,
    :global(html.lucy-quiescent) .ms-dot-heartbeat {
        animation-play-state: paused;
    }

    @media (prefers-reduced-motion: reduce) {
        .ms-dot-heartbeat { animation: none; }
    }
</style>
