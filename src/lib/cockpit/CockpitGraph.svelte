<script>
  /* ==========================================================================
     Lucy 2.0 — Memory graph (cockpit)  ·  v1.7.233
     The V2 counterpart of MemoryGraphView: same backend (`memory_graph`,
     d3-force layout) re-designed with cockpit tokens + a full motion layer:
     staggered node pop-in, edge draw-in, continuous gentle simulation, hover
     neighbor-highlight (rest dims), drag with physics reheat, wheel zoom +
     background pan, community colors, tag filter pills and a detail card.
     Additive — does not touch MemoryGraphView.svelte.
     ========================================================================== */
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { forceSimulation, forceManyBody, forceLink, forceCenter, forceCollide } from 'd3-force';
  import Refresh from '@tabler/icons-svelte/icons/refresh';
  import X from '@tabler/icons-svelte/icons/x';
  import Share3 from '@tabler/icons-svelte/icons/share-3';

  // v1.7.237 — callbacks al padre (CockpitMemory): abrir la memoria del nodo en
  // la pestaña Memorias / avisar de un cambio (borrado) para que refresque.
  let { onOpen = () => {}, onChanged = () => {} } = $props();

  const W = 1200, H = 760;
  const PALETTE = ['#3dd6a4', '#56b4e9', '#a78bfa', '#f59e0b', '#f06e6e', '#e879f9', '#34d399', '#fbbf24'];
  const EDGE_COLOR = { tag: 'var(--accent)', content: '#56b4e9', embedding: '#a78bfa' };

  let nodes = $state([]);        // d3 mutates x/y on these proxied objects → SVG follows
  let edges = $state([]);
  let meta = $state(null);       // { total_memories, truncated_nodes, top_tags }
  let loading = $state(false);
  let error = $state('');
  let hoverId = $state(null);
  let selected = $state(null);   // node for the detail card
  let tagFilter = $state(null);
  let vb = $state({ x: 0, y: 0, w: W, h: H });   // viewBox (zoom/pan)
  let entered = $state(false);   // gates the one-shot enter animations

  let sim = null;
  // id → Set(neighbor ids). Mutated in-place (clear + fill) — the template
  // re-renders via the nodes/edges state updates that always accompany it.
  const adj = new Map();

  const nColor = (n) => (n.community >= 0 ? PALETTE[n.community % PALETTE.length] : '#5C6672');
  const nR = (n) => 5 + Math.min(3, n.importance) * 2.5 + Math.min(6, Math.log2((n.access_count || 0) + 1) * 1.6);
  const isNeighbor = (id) => hoverId != null && (id === hoverId || adj.get(hoverId)?.has(id));
  const dimmed = (id) => hoverId != null && !isNeighbor(id);
  const tagMatch = (n) => !tagFilter || (n.tags || []).includes(tagFilter);

  async function load() {
    loading = true; error = ''; entered = false;
    selected = null; hoverId = null;
    try {
      const g = await invoke('memory_graph', { limit: 150 });
      adj.clear();
      for (const e of g.edges) {
        (adj.get(e.source) ?? adj.set(e.source, new Set()).get(e.source)).add(e.target);
        (adj.get(e.target) ?? adj.set(e.target, new Set()).get(e.target)).add(e.source);
      }
      // Seed positions in a loose ring so the pop-in doesn't start from (0,0).
      const N = g.nodes.length || 1;
      g.nodes.forEach((n, i) => {
        const a = (i / N) * Math.PI * 2;
        n.x = W / 2 + Math.cos(a) * (H / 3.2) * (0.55 + 0.45 * ((i * 37) % 100) / 100);
        n.y = H / 2 + Math.sin(a) * (H / 3.2) * (0.55 + 0.45 * ((i * 61) % 100) / 100);
      });
      nodes = g.nodes;
      edges = g.edges;
      meta = { total: g.total_memories, truncated: g.truncated_nodes, topTags: g.top_tags || [] };
      startSim();
      requestAnimationFrame(() => { entered = true; });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function startSim() {
    sim?.stop();
    sim = forceSimulation(nodes)
      .force('charge', forceManyBody().strength(-190).distanceMax(360))
      .force('link', forceLink(edges).id((d) => d.id).distance((l) => 60 + (1 - l.weight) * 90).strength((l) => 0.25 + l.weight * 0.5))
      .force('center', forceCenter(W / 2, H / 2))
      .force('collide', forceCollide().radius((d) => nR(d) + 6))
      .alpha(1)
      .alphaDecay(0.02)
      // Never fully freeze — a tiny alphaTarget keeps the graph "breathing".
      .alphaTarget(0.004);
  }

  function relayout() {
    for (const n of nodes) { n.fx = null; n.fy = null; }
    sim?.alpha(0.9).restart();
  }
  // v1.7.237 — borrar la memoria del nodo seleccionado y recargar el grafo.
  async function delNode(n) {
    try {
      await invoke('delete_agent_memory', { id: n.id });
      selected = null;
      await load();
      onChanged();
    } catch (e) { error = String(e); }
  }

  // ── Drag (physics reheat while held) ──────────────────────────────────────
  let dragN = null;
  function svgPoint(ev, svg) {
    const r = svg.getBoundingClientRect();
    return { x: vb.x + ((ev.clientX - r.left) / r.width) * vb.w, y: vb.y + ((ev.clientY - r.top) / r.height) * vb.h };
  }
  function downNode(ev, n) {
    ev.stopPropagation();
    dragN = n;
    sim?.alphaTarget(0.25).restart();
    ev.currentTarget.setPointerCapture?.(ev.pointerId);
  }
  function moveSvg(ev) {
    const svg = ev.currentTarget;
    if (dragN) {
      const p = svgPoint(ev, svg);
      dragN.fx = p.x; dragN.fy = p.y;
    } else if (panning) {
      const r = svg.getBoundingClientRect();
      vb = { ...vb, x: panStart.vx - (ev.clientX - panStart.mx) * (vb.w / r.width), y: panStart.vy - (ev.clientY - panStart.my) * (vb.h / r.height) };
    }
  }
  function upSvg() {
    if (dragN) { dragN.fx = null; dragN.fy = null; dragN = null; sim?.alphaTarget(0.004); }
    panning = false;
  }

  // ── Pan (background drag) + wheel zoom to cursor ──────────────────────────
  let panning = false, panStart = null;
  function downBg(ev) { panning = true; panStart = { mx: ev.clientX, my: ev.clientY, vx: vb.x, vy: vb.y }; selected = null; }
  function wheel(ev) {
    ev.preventDefault();
    const svg = ev.currentTarget;
    const p = svgPoint(ev, svg);
    const k = ev.deltaY > 0 ? 1.18 : 1 / 1.18;
    const w = Math.max(240, Math.min(W * 3, vb.w * k));
    const h = w * (H / W);
    vb = { x: p.x - ((p.x - vb.x) / vb.w) * w, y: p.y - ((p.y - vb.y) / vb.h) * h, w, h };
  }

  const communities = $derived(new Set(nodes.filter((n) => n.community >= 0).map((n) => n.community)).size);

  onMount(load);
  onDestroy(() => sim?.stop());
</script>

<div class="gph">
  <div class="gph-bar">
    <span class="ck-led" class:on={loading} class:err={!!error} aria-hidden="true"></span>
    <span class="gph-title ck-lbl"><Share3 size={12} stroke={1.75} /> Grafo de memoria</span>
    {#if meta}
      <span class="gph-meta ck-num">{nodes.length} de {meta.total} memorias · {edges.length} enlaces · {communities} clústeres{meta.truncated ? ' · (recortado)' : ''}</span>
    {/if}
    <span class="ck-rule" aria-hidden="true"></span>
    <span class="gph-legend">
      <i style="background:var(--accent)"></i>tags <i style="background:#56b4e9"></i>contenido <i style="background:#a78bfa"></i>semántico
    </span>
    <button class="gph-btn" onclick={relayout} title="Reorganizar el grafo"><Refresh size={14} stroke={1.75} /> Reorganizar</button>
  </div>

  {#if meta?.topTags?.length}
    <div class="gph-tags">
      <button class="gtag" class:on={tagFilter === null} onclick={() => (tagFilter = null)}>todos</button>
      {#each meta.topTags.slice(0, 10) as [t, c] (t)}
        <button class="gtag" class:on={tagFilter === t} onclick={() => (tagFilter = tagFilter === t ? null : t)}>{t} <span>{c}</span></button>
      {/each}
    </div>
  {/if}

  {#if error}<div class="gph-err">{error}</div>{/if}

  <div class="gph-stage">
    {#if loading}
      <div class="gph-empty"><Share3 size={30} stroke={1.3} /><p class="ck-shimmer">Construyendo el grafo…</p></div>
    {:else if !nodes.length}
      <div class="gph-empty"><Share3 size={30} stroke={1.3} /><p>Aún no hay suficientes memorias conectadas.</p></div>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <svg viewBox="{vb.x} {vb.y} {vb.w} {vb.h}" onpointermove={moveSvg} onpointerup={upSvg} onpointercancel={upSvg} onwheel={wheel} onpointerdown={downBg} class:entered>
        {#each edges as e, i (i)}
          {@const dim = hoverId != null && !(isNeighbor(e.source.id ?? e.source) && isNeighbor(e.target.id ?? e.target))}
          <line
            class="edge" class:dim
            x1={e.source.x} y1={e.source.y} x2={e.target.x} y2={e.target.y}
            stroke={EDGE_COLOR[e.kind] ?? 'var(--text-faint)'}
            stroke-width={0.6 + e.weight * 2.2}
            pathLength="1"
            style="animation-delay:{Math.min(i * 6, 700)}ms"
          />
        {/each}
        {#each nodes as n, i (n.id)}
          {@const r = nR(n)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <g
            class="node" class:dim={dimmed(n.id) || !tagMatch(n)} class:hot={n.importance >= 3} class:sel={selected?.id === n.id}
            style="animation-delay:{Math.min(i * 14, 900)}ms"
            role="button" tabindex="-1"
            onpointerdown={(ev) => downNode(ev, n)}
            onpointerenter={() => (hoverId = n.id)}
            onpointerleave={() => (hoverId = null)}
            onclick={(ev) => { ev.stopPropagation(); selected = n; }}
          >
            <circle class="halo" cx={n.x} cy={n.y} r={r + 5} fill={nColor(n)} />
            <circle class="core" cx={n.x} cy={n.y} r={r} fill={nColor(n)} />
            {#if (hoverId === n.id) || selected?.id === n.id || r > 11}
              <text x={n.x} y={n.y - r - 7}>{n.title.length > 34 ? n.title.slice(0, 34) + '…' : n.title}</text>
            {/if}
          </g>
        {/each}
      </svg>

      {#if selected}
        <div class="gph-card">
          <button class="gph-card-x" aria-label="Cerrar" onclick={() => (selected = null)}><X size={13} stroke={1.75} /></button>
          <div class="gc-head">
            <span class="gc-dot" style="background:{nColor(selected)}"></span>
            <span class="gc-title">{selected.title}</span>
          </div>
          <div class="gc-meta">#{selected.id} · importancia {selected.importance} · {selected.access_count} accesos · {adj.get(selected.id)?.size ?? 0} conexiones</div>
          {#if selected.preview}<p class="gc-prev">{selected.preview}</p>{/if}
          {#if selected.tags?.length}
            <div class="gc-tags">{#each selected.tags.slice(0, 6) as t}<span>{t}</span>{/each}</div>
          {/if}
          <div class="gc-acts">
            <button class="gc-btn" onclick={() => onOpen(selected)}>Abrir en Memorias</button>
            <button class="gc-btn danger" onclick={() => delNode(selected)}>Borrar</button>
          </div>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .gph { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .gph-bar { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-bottom: 10px; }
  /* v1.7.235 "instrumento premium" — label de panel en mono tracked (la regla
     scoped gana al .ck-lbl global por orden de cascada, así que el tratamiento
     debe vivir aquí). */
  .gph-title { display: flex; align-items: center; gap: 7px; font-family: var(--font-mono); font-size: var(--fs-micro); font-weight: var(--fw-medium); letter-spacing: var(--ls-label); text-transform: uppercase; color: var(--text-faint); }
  .gph-meta { font-family: var(--font-mono); font-variant-numeric: tabular-nums; font-size: var(--fs-caption); color: var(--text-muted); }
  .gph-legend { display: flex; align-items: center; gap: 6px; margin-left: auto; font-size: var(--fs-caption); color: var(--text-faint); }
  .gph-legend i { width: 14px; height: 3px; border-radius: 2px; display: inline-block; margin-left: 6px; }
  .gph-btn {
    display: inline-flex; align-items: center; gap: 6px; cursor: pointer;
    font-size: var(--fs-caption); color: var(--text-secondary); font-family: var(--font-sans);
    background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-md); padding: 5px 11px;
    transition: color var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out);
  }
  .gph-btn:hover { color: var(--text-primary); border-color: var(--border-accent); }
  .gph-tags { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 10px; }
  .gtag {
    font-size: var(--fs-caption); color: var(--text-muted); cursor: pointer; font-family: var(--font-sans);
    background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-pill); padding: 3px 10px;
    transition: color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out);
  }
  .gtag span { color: var(--text-faint); }
  .gtag:hover { color: var(--text-primary); }
  .gtag.on { color: var(--accent-ink); background: var(--accent); border-color: var(--accent); }
  .gtag.on span { color: var(--accent-ink); opacity: 0.75; }
  .gph-err { font-size: var(--fs-caption); color: var(--danger); background: rgba(240,110,110,0.08); border: 1px solid rgba(240,110,110,0.25); border-radius: var(--r-md); padding: 7px 11px; margin-bottom: 10px; }

  .gph-stage { position: relative; flex: 1; min-height: 0; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--r-lg); overflow: hidden; }
  svg { width: 100%; height: 100%; display: block; cursor: grab; touch-action: none; }
  svg:active { cursor: grabbing; }

  /* ── Motion layer ── */
  .edge {
    opacity: 0.5; stroke-linecap: round;
    stroke-dasharray: 1; stroke-dashoffset: 1;
    transition: opacity var(--dur-base) var(--ease-out);
  }
  svg.entered .edge { animation: edge-in 700ms var(--ease-out) forwards; }
  @keyframes edge-in { to { stroke-dashoffset: 0; } }
  .edge.dim { opacity: 0.06; }

  .node { cursor: pointer; opacity: 0; transform-box: fill-box; transform-origin: center; }
  svg.entered .node { animation: node-in 460ms var(--ease-snap, var(--ease-out)) forwards; }
  @keyframes node-in { 0% { opacity: 0; transform: scale(0.2); } 70% { transform: scale(1.12); } 100% { opacity: 1; transform: scale(1); } }
  .node .core { stroke: rgba(0, 0, 0, 0.45); stroke-width: 1; transition: opacity var(--dur-fast) var(--ease-out); }
  .node .halo { opacity: 0.14; transition: opacity var(--dur-fast) var(--ease-out); }
  .node:hover .halo, .node.sel .halo { opacity: 0.34; }
  .node.hot .halo { animation: halo-pulse 2.6s var(--ease-out) infinite; }
  @keyframes halo-pulse { 0%, 100% { opacity: 0.14; } 50% { opacity: 0.38; } }
  .node.dim { opacity: 0.13 !important; animation: none; }
  .node text {
    font-family: var(--font-sans); font-size: 11px; fill: var(--text-secondary);
    text-anchor: middle; pointer-events: none; paint-order: stroke; stroke: rgba(6, 10, 16, 0.85); stroke-width: 3px;
  }
  .node.sel text, .node:hover text { fill: var(--text-primary); }

  .gph-empty { height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; color: var(--text-faint); }
  .gph-empty p { margin: 0; font-size: var(--fs-footnote); color: var(--text-muted); }

  .gph-card {
    position: absolute; right: 12px; top: 12px; width: 290px; max-height: 70%;
    overflow-y: auto; padding: 13px 15px;
    background: var(--surface-1); border: 1px solid var(--border-strong); border-radius: var(--r-lg);
    box-shadow: 0 12px 34px -10px rgba(0, 0, 0, 0.6);
    animation: card-in var(--dur-base) var(--ease-out);
  }
  @keyframes card-in { from { opacity: 0; transform: translateX(10px); } to { opacity: 1; transform: none; } }
  .gph-card-x { position: absolute; top: 8px; right: 8px; width: 22px; height: 22px; display: flex; align-items: center; justify-content: center; border: 0; border-radius: var(--r-sm); background: transparent; color: var(--text-faint); cursor: pointer; }
  .gph-card-x:hover { color: var(--text-primary); background: var(--surface-3); }
  .gc-head { display: flex; align-items: flex-start; gap: 8px; margin-bottom: 6px; padding-right: 18px; }
  .gc-dot { width: 10px; height: 10px; border-radius: 50%; flex-shrink: 0; margin-top: 3px; }
  .gc-title { font-size: var(--fs-footnote); font-weight: var(--fw-medium); color: var(--text-primary); line-height: var(--lh-body); }
  .gc-meta { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); margin-bottom: 8px; }
  .gc-prev { margin: 0 0 9px; font-size: var(--fs-caption); line-height: var(--lh-body); color: var(--text-secondary); }
  .gc-tags { display: flex; flex-wrap: wrap; gap: 5px; }
  .gc-tags span { font-size: var(--fs-caption); color: var(--text-muted); background: var(--surface-3); padding: 1px 8px; border-radius: 5px; }
  .gc-acts { display: flex; gap: 6px; margin-top: 11px; }
  .gc-btn { flex: 1; font-size: var(--fs-caption); color: var(--text-secondary); cursor: pointer; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-md); padding: 5px 10px; font-family: var(--font-sans); transition: color var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out); }
  .gc-btn:hover { color: var(--text-primary); border-color: var(--border-accent); }
  .gc-btn.danger { color: var(--danger); }
  .gc-btn.danger:hover { border-color: var(--danger); }

  @media (prefers-reduced-motion: reduce) {
    svg.entered .edge, svg.entered .node { animation: none; opacity: 1; stroke-dashoffset: 0; }
    .node.hot .halo, .gph-card { animation: none; }
  }
</style>
