<!-- ── Sparkline.svelte — Inline SVG sparkline (v1.7.27, theme "E") ─────────
     Tiny line/bar chart for footer chips. Takes an array of numbers, draws
     them in the available space, no external dep. Used by the StatusBar
     to add visual texture to numeric chips (cost, stream, latency).

     Props:
       values    – number[] (any length; auto-scaled to min/max)
       width     – px, default 56
       height    – px, default 14
       kind      – 'line' (default) or 'bar'
       stroke    – CSS colour for the line/bars, default currentColor
       fill      – CSS colour for area under line (only kind=line), default none

     The component is dumb — it doesn't smooth/decimate. Pass at most ~60
     points or perf will degrade with high-frequency updates. -->
<script lang="ts">
    export let values: number[] = [];
    export let width  = 56;
    export let height = 14;
    export let kind: 'line' | 'bar' = 'line';
    export let stroke: string = 'currentColor';
    export let fill:   string = 'none';

    $: clean = (values || []).filter(v => Number.isFinite(v));
    $: n     = clean.length;
    $: max   = n > 0 ? Math.max(...clean) : 1;
    $: min   = n > 0 ? Math.min(...clean) : 0;
    $: range = Math.max(0.0001, max - min);

    /** Polyline points for kind=line, normalised to box [0,0]-[w,h]. */
    $: linePoints = n < 2 ? '' : clean.map((v, i) => {
        const x = (i / (n - 1)) * width;
        const y = height - ((v - min) / range) * height;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
    }).join(' ');

    /** Path for the area fill below the line. */
    $: areaPath = n < 2 ? '' :
        `M 0 ${height} L ${linePoints.replace(/,/g, ' ').split(' ').reduce((acc, val, i) => {
            return i % 2 === 0 ? acc + ' ' + val : acc + ',' + val;
        }, '').trim().replace(/^/, '')} L ${width} ${height} Z`;

    /** Bar rects for kind=bar. */
    $: bars = n === 0 ? [] : clean.map((v, i) => {
        const bw = width / n;
        const bh = Math.max(1, ((v - min) / range) * height);
        return { x: i * bw, y: height - bh, w: Math.max(1, bw - 1), h: bh };
    });
</script>

{#if n > 0}
    <svg class="sl" {width} {height} viewBox="0 0 {width} {height}"
         preserveAspectRatio="none" aria-hidden="true">
        {#if kind === 'line'}
            {#if fill !== 'none'}
                <path d="M 0 {height} {clean.map((v, i) => {
                    const x = (i / Math.max(1, n - 1)) * width;
                    const y = height - ((v - min) / range) * height;
                    return `L ${x.toFixed(1)} ${y.toFixed(1)}`;
                }).join(' ')} L {width} {height} Z" {fill} stroke="none" opacity="0.4" />
            {/if}
            <polyline points={linePoints}
                      fill="none" {stroke}
                      stroke-width="1.4" stroke-linejoin="round" stroke-linecap="round" />
        {:else}
            {#each bars as b}
                <rect x={b.x} y={b.y} width={b.w} height={b.h} fill={stroke} opacity="0.85" />
            {/each}
        {/if}
    </svg>
{/if}

<style>
    .sl {
        display: inline-block;
        vertical-align: middle;
        flex-shrink: 0;
    }
</style>
