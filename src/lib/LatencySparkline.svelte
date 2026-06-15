<!--
  LatencySparkline.svelte — v1.7.99 (Option D3)

  Tiny canvas sparkline mounted in the status bar. Shows the latency
  trend of recent LLM/tool turns, color-coded by model. Polls the
  backend every 30 s for fresh samples; idle when minimised /
  prefers-reduced-motion.

  Why canvas and not uplot
  ────────────────────────
  uplot is overkill for a 80×16 px strip — its layout pass alone costs
  more than the actual draw. We use a hand-rolled 50-line canvas
  routine. Total work per repaint: ~30 line segments.

  Multi-model rendering
  ─────────────────────
  Each model gets its own polyline in its own color. When the user
  picks a focused model (drop-down upstream), unrelated models fade to
  background, keeping the strip readable.

  Tooltip on hover shows: model name · p50 · p95 · sample count over
  the rendered window.
-->
<script lang="ts">
    import { onMount, onDestroy, tick } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    // v1.7.174 — visibility-gated polling (skip IPC while window hidden).
    import { gatedInterval } from '$lib/poll';

    /** Optional focus: when set, the named model is highlighted and others
     *  fade. Otherwise every model is drawn at full intensity. */
    export let focusModel: string = '';

    /** English locale flag (tooltips). */
    export let isEN: boolean = false;

    /** Polling interval. 30 s is plenty — operator-facing trend, not
     *  a real-time scope. */
    const POLL_MS = 30_000;

    /** Max points kept per model. Frontend trim — backend already caps
     *  the row count globally. */
    const MAX_PER_MODEL = 30;

    /** Min raw points before we bother to draw. Avoids the sparkline
     *  flickering on first boot. */
    const MIN_POINTS = 3;

    interface Point { elapsed_ms: number; ts: number; }
    interface Series { model: string; points: Point[]; }

    let canvas: HTMLCanvasElement;
    let series: Series[] = [];
    let stopPoll: (() => void) | null = null;
    let hovering = false;
    let tipText = '';

    /** Colors per model. We pick by hash so additions don't shift
     *  existing assignments. Hot-path: 6 colors × any model name. */
    const PALETTE = ['#10b981', '#3b9eff', '#a78bfa', '#f59e0b', '#f472b6', '#22d3ee'];
    function colorFor(model: string): string {
        let h = 0;
        for (let i = 0; i < model.length; i++) h = ((h << 5) - h + model.charCodeAt(i)) | 0;
        return PALETTE[Math.abs(h) % PALETTE.length];
    }

    async function refresh() {
        try {
            const rows: Array<{ model: string; elapsed_ms: number; ts: number }> =
                await invoke('recent_model_latencies', { limit: 200 });
            // Bucket by model, oldest→newest, capped at MAX_PER_MODEL.
            const bucket = new Map<string, Point[]>();
            for (const r of rows) {
                const arr = bucket.get(r.model) || [];
                arr.push({ elapsed_ms: r.elapsed_ms, ts: r.ts });
                bucket.set(r.model, arr);
            }
            const next: Series[] = [];
            for (const [model, arr] of bucket.entries()) {
                // Backend returns DESC; flip and trim.
                arr.reverse();
                next.push({
                    model,
                    points: arr.slice(-MAX_PER_MODEL),
                });
            }
            // Stable order: most-sampled models first so dominant model
            // sets the y-scale anchor in the human's mental model.
            next.sort((a, b) => b.points.length - a.points.length);
            series = next;
            await tick();
            draw();
        } catch {
            // Backend missing/unreachable — leave the canvas blank rather
            // than throwing into the status bar.
        }
    }

    /** Compute summary stats for the tooltip. p50/p95 from the rendered
     *  window for the focused (or single) model. */
    function buildTip(): string {
        if (series.length === 0) return '';
        const tgt = focusModel
            ? series.find(s => s.model === focusModel)
            : series[0];
        if (!tgt || tgt.points.length === 0) return '';
        const sorted = tgt.points.map(p => p.elapsed_ms).sort((a, b) => a - b);
        const p50 = sorted[Math.floor(sorted.length * 0.50)];
        const p95 = sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95))];
        return isEN
            ? `${tgt.model} · p50 ${p50} ms · p95 ${p95} ms · ${sorted.length} samples`
            : `${tgt.model} · p50 ${p50} ms · p95 ${p95} ms · ${sorted.length} muestras`;
    }

    function draw() {
        if (!canvas) return;
        const dpr = window.devicePixelRatio || 1;
        const w = canvas.clientWidth, h = canvas.clientHeight;
        canvas.width  = Math.max(1, Math.floor(w * dpr));
        canvas.height = Math.max(1, Math.floor(h * dpr));
        const ctx = canvas.getContext('2d');
        if (!ctx) return;
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        ctx.clearRect(0, 0, w, h);

        // Skip if no usable data.
        const usable = series.filter(s => s.points.length >= MIN_POINTS);
        if (usable.length === 0) {
            ctx.fillStyle = 'rgba(148, 163, 184, 0.40)';
            ctx.font = `10px var(--font-mono, monospace)`;
            ctx.textBaseline = 'middle';
            ctx.fillText('—', 4, h / 2);
            return;
        }

        // Y-axis: shared log scale across all series so a slow outlier
        // doesn't collapse the rest into a flat line. Latency is
        // heavy-tailed; log reads better than linear here.
        let maxMs = 0;
        for (const s of usable) for (const p of s.points) {
            if (p.elapsed_ms > maxMs) maxMs = p.elapsed_ms;
        }
        const yMax = Math.log10(Math.max(maxMs, 200));   // floor at 200ms
        const yMin = Math.log10(50);                     // floor at 50ms

        // Draw each series as a polyline. Focused model is opaque +
        // 1.6 px stroke; others fade to ~25% alpha.
        for (const s of usable) {
            const focused = !focusModel || s.model === focusModel;
            const color = colorFor(s.model);
            ctx.strokeStyle = focused
                ? color
                : color + '40';   // 25% alpha hex suffix
            ctx.lineWidth = focused ? 1.6 : 1.0;
            ctx.beginPath();
            const n = s.points.length;
            for (let i = 0; i < n; i++) {
                const x = (i / (n - 1)) * (w - 2) + 1;
                const ly = Math.log10(Math.max(s.points[i].elapsed_ms, 1));
                const norm = (ly - yMin) / Math.max(0.001, yMax - yMin);
                const y = h - 1 - norm * (h - 2);
                if (i === 0) ctx.moveTo(x, y);
                else         ctx.lineTo(x, y);
            }
            ctx.stroke();

            // Last-sample dot for focused / single-model strip.
            if (focused) {
                const lp = s.points[s.points.length - 1];
                const ly = Math.log10(Math.max(lp.elapsed_ms, 1));
                const norm = (ly - yMin) / Math.max(0.001, yMax - yMin);
                const x = w - 1, y = h - 1 - norm * (h - 2);
                ctx.fillStyle = color;
                ctx.beginPath();
                ctx.arc(x, y, 1.6, 0, Math.PI * 2);
                ctx.fill();
            }
        }
    }

    function onHover(_e: MouseEvent) {
        hovering = true;
        tipText = buildTip();
    }
    function onLeave() {
        hovering = false;
    }

    onMount(() => {
        refresh();
        stopPoll = gatedInterval(refresh, POLL_MS);
        // Redraw on container resize so the sparkline always uses
        // the full canvas width. jsdom (vitest) doesn't ship
        // ResizeObserver — guard so the StatusBar unit tests stay green.
        if (typeof ResizeObserver === 'undefined') return;
        const ro = new ResizeObserver(() => draw());
        if (canvas) ro.observe(canvas);
        return () => ro.disconnect();
    });

    onDestroy(() => {
        if (stopPoll) stopPoll();
    });

    // Redraw when focus changes (highlight different model).
    $: if (canvas && series.length > 0) { focusModel; draw(); }
</script>

<div
    class="latency-spark-wrap"
    role="img"
    aria-label={isEN ? 'LLM latency sparkline' : 'Sparkline de latencia LLM'}
    on:mouseenter={onHover}
    on:mouseleave={onLeave}
    title={tipText || (isEN ? 'LLM latency · last samples' : 'Latencia LLM · últimas muestras')}
>
    <canvas bind:this={canvas} class="latency-spark" aria-hidden="true"></canvas>
    {#if hovering && tipText}
        <span class="latency-spark-tip">{tipText}</span>
    {/if}
</div>

<style>
    .latency-spark-wrap {
        display: inline-flex;
        align-items: center;
        position: relative;
        height: 16px;
        width: 90px;
        margin: 0 4px;
        cursor: help;
    }
    .latency-spark {
        width: 100%;
        height: 100%;
        display: block;
    }
    .latency-spark-tip {
        position: absolute;
        bottom: calc(100% + 6px);
        right: 0;
        white-space: nowrap;
        font-family: var(--font-mono, monospace);
        font-size: 10px;
        font-weight: 500;
        padding: 4px 8px;
        background: rgba(6, 10, 18, 0.95);
        color: var(--text-bright, #f1f5f9);
        border: 1px solid var(--border-light, #334155);
        border-radius: 4px;
        box-shadow: 0 6px 16px -4px rgba(0, 0, 0, 0.6);
        pointer-events: none;
        z-index: 50;
    }
</style>
