<script lang="ts">
    // ── CpuHeatmap — Grid visualization of per-core CPU usage (Lucy Gen 2) ─
    //
    // Replaces the 1×N vertical bars in DashboardView with a 2D grid that
    // respects physical CPU topology (P-cores top row, E-cores or HT siblings
    // bottom row). Designed first for Ryzen-class CPUs (6/8/12/16 cores) but
    // works for any core count — the grid adapts.
    //
    // Visual:
    //   ┌──┬──┬──┬──┬──┬──┐
    //   │░░│▒▒│▓▓│▓▓│░░│▒▒│   row 0
    //   ├──┼──┼──┼──┼──┼──┤
    //   │░░│▒▒│▓▓│▓▓│░░│▒▒│   row 1
    //   └──┴──┴──┴──┴──┴──┘
    //
    // Each cell is a CSS-grid item with background tied to the severity of
    // its usage value. Color tokens from page.css semantic palette so future
    // theme changes propagate automatically.

    /** Per-core usage percentages, in physical order. Lucy backend returns up to 12. */
    export let cores: number[] = [];
    /** Optional process name per core for the hover tooltip. */
    export let topProcessPerCore: (string | undefined)[] = [];
    /** Number of columns. Auto-computed if not provided: sqrt(n) rounded. */
    export let cols: number | undefined = undefined;
    /** Display labels under each cell (C0, C1, ...) */
    export let showLabels: boolean = true;
    /** Show usage % below each cell */
    export let showPct: boolean = true;

    // Auto-compute columns: prefer 6 for typical desktop CPUs (12/24 cores
    // = 6×2/4 grid). Fall back to a near-square layout for unusual counts.
    $: actualCols = cols ?? (
        cores.length === 0 ? 1
        : cores.length <= 4 ? cores.length
        : cores.length % 6 === 0 ? 6
        : cores.length % 8 === 0 ? 8
        : Math.ceil(Math.sqrt(cores.length))
    );

    $: rows = Math.max(1, Math.ceil(cores.length / actualCols));

    /** Severity bucket for color routing. Tied to semantic palette. */
    function bucket(v: number): 'ok' | 'op' | 'warn' | 'crit' {
        if (v >= 85) return 'crit';
        if (v >= 60) return 'warn';
        if (v >= 30) return 'op';
        return 'ok';
    }

    /** Tooltip text — falls back gracefully when topProcessPerCore is sparse. */
    function tooltip(i: number, v: number): string {
        const proc = topProcessPerCore[i];
        const base = `Core ${i} · ${v.toFixed(1)}%`;
        return proc ? `${base} · ${proc}` : base;
    }
</script>

{#if cores.length > 0}
    <div class="heatmap-wrap" style="--cols:{actualCols};--rows:{rows};">
        {#each cores as usage, i}
            <div class="hm-cell hm-{bucket(usage)}"
                 title={tooltip(i, usage)}
                 aria-label={tooltip(i, usage)}>
                <div class="hm-fill" style="--fill:{Math.max(2, Math.min(100, usage))}%"></div>
                {#if showLabels}<span class="hm-label">C{i}</span>{/if}
                {#if showPct}<span class="hm-pct">{usage.toFixed(0)}%</span>{/if}
            </div>
        {/each}
    </div>
{:else}
    <div class="heatmap-empty">No per-core data</div>
{/if}

<style>
    .heatmap-wrap {
        display: grid;
        grid-template-columns: repeat(var(--cols, 6), minmax(28px, 1fr));
        grid-template-rows: repeat(var(--rows, 2), auto);
        gap: 6px;
        padding: 4px 0;
    }
    .hm-cell {
        position: relative;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: flex-end;
        min-height: 42px;
        border-radius: 6px;
        /* Semi-matte: very subtle base, no glow, no gradient. */
        background: rgba(148,163,184,0.035);
        border: 1px solid rgba(148,163,184,0.08);
        overflow: hidden;
        transition: border-color .18s ease, background .18s ease;
        cursor: default;
    }
    .hm-cell:hover {
        background: rgba(148,163,184,0.06);
    }
    .hm-fill {
        position: absolute;
        left: 0; right: 0; bottom: 0;
        height: var(--fill, 0%);
        transition: height .35s cubic-bezier(.16,1,.3,1), background .25s ease;
        pointer-events: none;
        /* Flat low-saturation tint. The severity classes override only `background`
           (a translucent rgba) — no gradient, no glow. This is the semi-matte look. */
        opacity: 1;
    }
    .hm-label {
        font-family: var(--mono, monospace);
        font-size: 9px;
        color: var(--txt3, #475569);
        letter-spacing: .5px;
        margin: 2px 0 1px;
        position: relative;
        z-index: 1;
    }
    .hm-pct {
        font-family: var(--mono, monospace);
        font-size: 11px;
        font-weight: 600;
        color: var(--txt, #e2e8f0);
        /* No text-shadow — the fill is muted enough that the % stays readable. */
        margin-bottom: 4px;
        position: relative;
        z-index: 1;
    }

    /* Severity color mapping — FLAT, semi-translucent tints (no gradient, no glow).
       Tuned so the cell reads as "tinted matte plate" rather than a saturated bar. */
    .hm-ok    .hm-fill { background: rgba(16,185,129,0.22); }
    .hm-op    .hm-fill { background: rgba(94,200,255,0.22); }
    .hm-warn  .hm-fill { background: rgba(245,158,11,0.26); }
    .hm-crit  .hm-fill { background: rgba(239,68,68,0.30); }

    /* Severity-tinted borders so the cell color reads even when fill is small */
    .hm-ok    { border-color: rgba(16,185,129,0.18); }
    .hm-op    { border-color: rgba(94,200,255,0.18); }
    .hm-warn  { border-color: rgba(245,158,11,0.24); }
    .hm-crit  { border-color: rgba(239,68,68,0.28); }

    /* Hover: bump the border to its full severity color (still no glow) */
    .hm-ok:hover    { border-color: rgba(16,185,129,0.4); }
    .hm-op:hover    { border-color: rgba(94,200,255,0.4); }
    .hm-warn:hover  { border-color: rgba(245,158,11,0.42); }
    .hm-crit:hover  { border-color: rgba(239,68,68,0.46); }

    .heatmap-empty {
        padding: 12px;
        text-align: center;
        font-size: 11px;
        color: var(--txt3, #64748b);
        font-style: italic;
    }

    /* ── Light mode (v1.7.232) ────────────────────────────────────────────
       The semi-matte tints (cell base 3.5% slate, fills ~22–30% alpha) are
       calibrated to glow on a dark plate. On the light canvas the cell base
       vanishes and the fills wash out to pale pastels. In light mode give the
       cell a real grey plate and push the fills to a higher alpha so each core
       reads as a clearly-tinted bar, not a faint smudge. Blue uses blue-600
       (the light `op` bucket's #5ec8ff is too pale on white). */
    :global(:root.light) .hm-cell {
        background: #eef2f7;
        border-color: var(--bdr, #cbd5e1);
    }
    :global(:root.light) .hm-cell:hover { background: #e4eaf1; }
    :global(:root.light) .hm-ok   .hm-fill { background: rgba(5,150,105,0.40); }
    :global(:root.light) .hm-op   .hm-fill { background: rgba(37,99,235,0.34); }
    :global(:root.light) .hm-warn .hm-fill { background: rgba(217,119,6,0.44); }
    :global(:root.light) .hm-crit .hm-fill { background: rgba(220,38,38,0.48); }
    :global(:root.light) .hm-ok   { border-color: rgba(5,150,105,0.34); }
    :global(:root.light) .hm-op   { border-color: rgba(37,99,235,0.32); }
    :global(:root.light) .hm-warn { border-color: rgba(217,119,6,0.40); }
    :global(:root.light) .hm-crit { border-color: rgba(220,38,38,0.44); }

    /* Reduce motion for accessibility */
    @media (prefers-reduced-motion: reduce) {
        .hm-fill { transition: none; }
        .hm-cell:hover { transform: none; }
    }
</style>
