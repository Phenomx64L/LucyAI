<!--
  ── KgMiniViewer.svelte — F9 lightweight graph visualizer ────────────────

  Renders a small canvas-based view of a file's co-touched neighbors.
  Deliberately NOT using D3/Cytoscape — adds 200KB+ for a single view that
  fits in 600 lines of native canvas drawing.

  Layout: radial. Center node is the queried `path`. Neighbors arranged on
  a ring, sized by weight, with edge thickness reflecting strength.

  Inputs:
    - path:  full path of the file at the center
    - neighbors: array returned by kg_neighbors command

  Interaction:
    - hover a node → tooltip with full path
    - click a neighbor → dispatches 'select' so the parent can re-query
                         with that neighbor as the new center
-->
<script context="module" lang="ts">
    export interface KgNeighborNode {
        path: string;
        weight: number;
        extension: string;
        last_mtime: number;
    }
</script>

<script lang="ts">
    import { onMount, createEventDispatcher, afterUpdate } from 'svelte';
    import { recencyIntensity } from '$lib/heat-layer';

    export let path: string = '';
    export let neighbors: KgNeighborNode[] = [];
    export let width: number = 320;
    export let height: number = 280;

    const dispatch = createEventDispatcher<{ select: { path: string } }>();

    let canvas: HTMLCanvasElement;
    let hoverIdx: number = -1;
    let hoverLabel: string = '';
    let hoverX: number = 0;
    let hoverY: number = 0;

    interface NodePos {
        x: number;
        y: number;
        r: number;
        path: string;
        weight: number;
        ext: string;
        recency: number;
    }

    let layout: NodePos[] = [];
    let centerNode: NodePos | null = null;

    /** Pure layout: compute node positions for the current props. */
    function computeLayout(): { center: NodePos | null; ring: NodePos[] } {
        if (!path) return { center: null, ring: [] };
        const cx = width / 2;
        const cy = height / 2;
        const center: NodePos = {
            x: cx, y: cy, r: 24,
            path,
            weight: neighbors.reduce((s, n) => s + n.weight, 0),
            ext: '',
            recency: 1.0,
        };
        if (neighbors.length === 0) return { center, ring: [] };

        // Max weight for normalizing node sizes
        const maxW = neighbors.reduce((m, n) => Math.max(m, n.weight), 1);
        const ringRadius = Math.min(width, height) / 2 - 36;
        const N = Math.min(neighbors.length, 16); // cap at 16 visible neighbors
        const ring: NodePos[] = [];
        for (let i = 0; i < N; i++) {
            const ang = (i / N) * Math.PI * 2 - Math.PI / 2;
            const x = cx + Math.cos(ang) * ringRadius;
            const y = cy + Math.sin(ang) * ringRadius;
            const n = neighbors[i];
            const sizeRatio = n.weight / maxW;
            const r = 8 + sizeRatio * 12; // 8..20
            ring.push({
                x, y, r,
                path: n.path,
                weight: n.weight,
                ext: n.extension || '',
                recency: recencyIntensity(n.last_mtime),
            });
        }
        return { center, ring };
    }

    function draw() {
        if (!canvas) return;
        const ctx = canvas.getContext('2d');
        if (!ctx) return;
        const dpr = window.devicePixelRatio || 1;
        canvas.width = width * dpr;
        canvas.height = height * dpr;
        canvas.style.width = `${width}px`;
        canvas.style.height = `${height}px`;
        ctx.scale(dpr, dpr);

        // Background
        ctx.fillStyle = 'rgba(255,255,255,0.015)';
        ctx.fillRect(0, 0, width, height);

        const { center, ring } = computeLayout();
        centerNode = center;
        layout = ring;
        if (!center) return;

        // Edges (center → each ring node), thickness proportional to weight
        const maxW = ring.reduce((m, n) => Math.max(m, n.weight), 1);
        for (let i = 0; i < ring.length; i++) {
            const n = ring[i];
            const w = Math.max(0.4, (n.weight / maxW) * 2.4);
            const alpha = 0.15 + (n.weight / maxW) * 0.5;
            ctx.strokeStyle = i === hoverIdx ? `rgba(16,185,129,0.85)` : `rgba(255,255,255,${alpha.toFixed(2)})`;
            ctx.lineWidth = w;
            ctx.beginPath();
            ctx.moveTo(center.x, center.y);
            ctx.lineTo(n.x, n.y);
            ctx.stroke();
        }

        // Ring nodes
        for (let i = 0; i < ring.length; i++) {
            const n = ring[i];
            // Color by extension hash → consistent color per file type
            const hue = hashHue(n.ext || 'unknown');
            const sat = 60;
            const light = 50 + (n.recency * 15);
            ctx.fillStyle = `hsla(${hue}, ${sat}%, ${light}%, ${0.7 + n.recency * 0.3})`;
            ctx.beginPath();
            ctx.arc(n.x, n.y, n.r, 0, Math.PI * 2);
            ctx.fill();
            // Hover highlight
            if (i === hoverIdx) {
                ctx.strokeStyle = 'rgba(16,185,129,0.95)';
                ctx.lineWidth = 2;
                ctx.beginPath();
                ctx.arc(n.x, n.y, n.r + 2, 0, Math.PI * 2);
                ctx.stroke();
            }
        }

        // Center node
        ctx.fillStyle = 'rgba(16,185,129,0.18)';
        ctx.beginPath();
        ctx.arc(center.x, center.y, center.r, 0, Math.PI * 2);
        ctx.fill();
        ctx.strokeStyle = 'rgba(16,185,129,0.85)';
        ctx.lineWidth = 2;
        ctx.stroke();

        // Center label
        ctx.fillStyle = '#e2e8f0';
        ctx.font = '11px monospace';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        const label = shortBasename(center.path);
        ctx.fillText(label.slice(0, 14), center.x, center.y);
    }

    /** Stable hash of a string → 0-359 hue. */
    function hashHue(s: string): number {
        let h = 0;
        for (let i = 0; i < s.length; i++) h = ((h << 5) - h + s.charCodeAt(i)) | 0;
        return Math.abs(h) % 360;
    }

    function shortBasename(p: string): string {
        const parts = p.split(/[\\/]/);
        return parts[parts.length - 1] || p;
    }

    function onMouseMove(ev: MouseEvent) {
        const rect = canvas.getBoundingClientRect();
        const mx = ev.clientX - rect.left;
        const my = ev.clientY - rect.top;
        let found = -1;
        for (let i = 0; i < layout.length; i++) {
            const n = layout[i];
            const dx = n.x - mx;
            const dy = n.y - my;
            if (dx * dx + dy * dy < n.r * n.r) { found = i; break; }
        }
        if (found !== hoverIdx) {
            hoverIdx = found;
            draw();
        }
        if (found >= 0) {
            hoverLabel = layout[found].path;
            hoverX = ev.clientX + 12;
            hoverY = ev.clientY + 12;
        } else {
            hoverLabel = '';
        }
    }

    function onMouseLeave() {
        if (hoverIdx !== -1) {
            hoverIdx = -1;
            draw();
        }
        hoverLabel = '';
    }

    function onClick() {
        if (hoverIdx >= 0 && hoverIdx < layout.length) {
            dispatch('select', { path: layout[hoverIdx].path });
        }
    }

    onMount(draw);
    afterUpdate(draw);
</script>

<div class="kgv-wrap" role="figure" aria-label="Knowledge graph neighbors">
    <canvas bind:this={canvas}
            on:mousemove={onMouseMove}
            on:mouseleave={onMouseLeave}
            on:click={onClick}
            style="cursor: {hoverIdx >= 0 ? 'pointer' : 'default'};"></canvas>
    {#if hoverLabel}
        <div class="kgv-tip" style="left:{hoverX}px; top:{hoverY}px;">{hoverLabel}</div>
    {/if}
    {#if neighbors.length === 0}
        <div class="kgv-empty">No co-touched neighbors recorded yet.</div>
    {/if}
</div>

<style>
    .kgv-wrap {
        position: relative;
        display: inline-block;
        background: rgba(0,0,0,0.20);
        border: 1px solid var(--border-color, #1e293b);
        border-radius: 6px;
        overflow: hidden;
    }
    .kgv-tip {
        position: fixed;
        z-index: 9100;
        padding: 4px 8px;
        background: var(--bg-card, #161b22);
        border: 1px solid var(--border-color, #1e293b);
        border-radius: 4px;
        font-family: var(--font-mono, monospace);
        font-size: 10px;
        color: var(--text-bright, #f1f5f9);
        pointer-events: none;
        max-width: 360px;
        word-break: break-all;
        box-shadow: 0 4px 12px -4px rgba(0,0,0,0.55);
    }
    .kgv-empty {
        position: absolute;
        top: 50%; left: 50%;
        transform: translate(-50%, -50%);
        font-family: var(--font-mono);
        font-size: 11px;
        color: var(--text-muted);
        font-style: italic;
        pointer-events: none;
    }
</style>
