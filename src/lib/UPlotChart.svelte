<!-- ── UPlotChart.svelte — fast canvas charts for big data (v1.4.12) ──────
     Wraps the uPlot library so the rest of Lucy can render large time-
     series (capacity history, performance dashboards, replay timelines)
     at 60fps without thinking about the canvas API.

     When NOT to use this component:
       • Inline sparklines with <60 points — the existing SVG sparkline
         in DashboardView is faster for that case because uPlot's canvas
         overhead doesn't amortize.

     When to use it:
       • >500 points
       • Multi-series overlays (RAM + CPU + Disk on one chart)
       • Anything where you want crosshair + tooltips + zoom

     Props
       data       — uPlot's AlignedData: [xs[], ys[], ys[], ...]
       series     — array of series defs (label, stroke, fill, scale)
       width/height — canvas dimensions in CSS pixels
       theme      — 'dark' (default — matches Lucy palette) or 'light'

     Slots — none. Tooltips are uPlot's built-in cursor hover.
─────────────────────────────────────────────────────────────────────── -->
<script>
    import { onMount, onDestroy } from 'svelte';
    import uPlot from 'uplot';

    /** uPlot AlignedData: [xs[], ys[], ys[], ...] */
    // uPlot's `AlignedData` is `[xs, ...ys]` where each series is a TypedArray
    // OR a plain number array — its own runtime accepts both, but the shipped
    // types only describe the TypedArray form. Every caller here passes plain
    // arrays (they come from JSON), so the annotation follows the library's
    // behaviour rather than its declaration; the alternative is converting to
    // Float64Array at every call site to satisfy a constraint uPlot does not
    // actually impose.
    /** @type {import('uplot').AlignedData} */
    export let data = /** @type {any} */ ([[], []]);
    /** Series array. First entry is the x-axis (omit stroke), rest are y-series. */
    export let series = [
        {},
        { label: 'Value', stroke: 'rgb(16, 185, 129)', fill: 'rgba(16, 185, 129, 0.10)', width: 1.5 },
    ];
    /** Outer dimensions in CSS pixels. */
    export let width = 480;
    export let height = 200;
    /** Optional axis customization passed through to uPlot. */
    export let axes = undefined;
    /** 'dark' (default — matches Lucy) or 'light'. */
    export let theme = 'dark';
    /** When true, suppress the grid lines (cleaner for sparkline-style usage). */
    export let minimal = false;

    let container;
    let plot = null;

    // Theme-aware colors. We keep these inline (not from CSS variables)
    // because uPlot draws to canvas — it can't read CSS at draw time.
    const palette = theme === 'dark'
        ? { grid: 'rgba(255,255,255,0.05)', label: '#94a3b8', stroke: '#475569' }
        : { grid: 'rgba(0,0,0,0.06)',       label: '#64748b', stroke: '#cbd5e1' };

    function buildOpts(w, h) {
        const baseAxes = axes ?? [
            // X axis (time)
            { stroke: palette.stroke, grid: { stroke: palette.grid, width: 1 },
              ticks: { stroke: palette.stroke }, font: '11px ui-monospace,monospace',
              labelFont: '11px ui-sans-serif', labelSize: 0 },
            // Y axis (value)
            { stroke: palette.stroke, grid: { stroke: palette.grid, width: 1 },
              ticks: { stroke: palette.stroke }, font: '11px ui-monospace,monospace',
              labelFont: '11px ui-sans-serif', labelSize: 0,
              size: 36 },
        ];
        return {
            width: w, height: h,
            series,
            // Crosshair + tooltip out of the box.
            cursor: {
                drag:  { x: true, y: false, setScale: true }, // shift-drag to zoom x
                focus: { prox: 16 },
            },
            scales: {
                x: { time: true },
                y: { auto: true },
            },
            axes: minimal ? [] : baseAxes,
            legend: { show: !minimal, live: true },
        };
    }

    onMount(() => {
        plot = new uPlot(buildOpts(width, height), data, container);
    });

    // Reactive updates: data swap = setData (fast in-place redraw).
    // Resize = setSize (causes layout). Reset chart wholesale only when
    // series defs change because that's a structural change.
    let _prevSeriesKey = JSON.stringify(series);
    $: if (plot && data) {
        try { plot.setData(data); } catch (e) { console.warn('[uplot] setData failed:', e); }
    }
    $: if (plot && (width || height)) {
        try { plot.setSize({ width, height }); } catch (e) { console.warn('[uplot] setSize failed:', e); }
    }
    $: {
        const k = JSON.stringify(series);
        if (plot && k !== _prevSeriesKey) {
            try {
                plot.destroy();
                plot = new uPlot(buildOpts(width, height), data, container);
                _prevSeriesKey = k;
            } catch (e) { console.warn('[uplot] series rebuild failed:', e); }
        }
    }

    onDestroy(() => {
        if (plot) { try { plot.destroy(); } catch {} plot = null; }
    });
</script>

<div bind:this={container} class="uplot-host" style="width:{width}px;height:{height}px;"></div>

<!-- We import the uPlot stylesheet GLOBALLY at the chart level so the
     consuming page doesn't need to remember it. The styles are scoped
     by uPlot's class prefixes (`.uplot-*`) so they don't bleed. -->
<svelte:head>
    <link rel="stylesheet" href="https://unpkg.com/uplot@1.6.32/dist/uPlot.min.css" crossorigin="anonymous" />
</svelte:head>

<style>
    .uplot-host {
        display: block;
        position: relative;
    }
    /* Force the canvas to match the host. uPlot's default styles size
       to its own internal width/height but on retina displays a
       'host-width' container makes the chart center cleanly. */
    :global(.uplot-host .uplot) {
        font-family: inherit;
    }
</style>
