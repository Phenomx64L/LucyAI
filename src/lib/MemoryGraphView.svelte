<!--
  MemoryGraphView.svelte — Tier S #2 + Memory Graph 2.0 (sprint largo)

  Interactive force-directed visualization of Lucy's long-term memory.
  Reads from the `memory_graph` Tauri command (memory.rs) which returns
  up to 250 nodes + 1500 edges from agent_memories.

  Memory Graph 2.0 additions:
    V1  — Opaque background (no more bleed-through of underlying UI)
    V2  — Always-visible labels for connected nodes
    V3  — Color nodes by detected community (palette of 8)
    V5  — Search bar that fades non-matching nodes
    V7  — Highlight neighbors on hover (rest dimmed)
    V9  — Header stats bar (nodes · edges · clusters · density)
    V11 — Bigger nodes (more legible)
    V12 — "Open in browser" button that closes overlay + jumps to memoria
    V14 — Runtime threshold sliders (tag / content / embedding)
    +   — Tag pill filter row
    +   — Legend always visible (with community count)

  Backend additions consumed:
    • community: i32       — Louvain-lite community assignment per node
    • stats: {...}         — edges_by_kind, density, community_count, orphans
    • top_tags: [(s,n)]    — top 12 tags for the filter pills

  Why SVG + custom force simulation (no D3.js):
    • D3 weighs ~70KB gzipped; we use ~200 lines of physics here
    • Svelte 5 reactivity drives the redraw — no manual DOM diff cost
    • At ≤250 nodes the O(n²) repulsion is < 4ms per tick on a desktop

  Interactions:
    • Drag node — lock its position (re-drag to release)
    • Wheel — zoom in/out (cursor-anchored)
    • Drag empty space — pan
    • Click node — open detail panel
    • Hover node — highlight its neighbors, dim the rest
    • Type in search — fade non-matching nodes
    • Click tag pill — filter to nodes carrying that tag
    • Sliders — adjust tag/content/embedding thresholds in realtime
    • ESC — close the overlay (or close detail panel if open)
-->
<script lang="ts">
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { onMount, onDestroy, createEventDispatcher, tick } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    // v1.7.71 — d3-force replaces the hand-rolled Euler integrator.
    // Why: the previous custom physics (K_REPEL / d² with simple Euler
    // + clamp) blew up when two nodes started near each other — one
    // tick produced a 10⁴-magnitude force, velocity-clamped at 12 px,
    // but cumulative drift sent a handful of nodes far off-canvas
    // before the springs could pull them back. With 40 embedding
    // edges, every escaped node became the apex of a long cone of
    // edges (the "weird pyramid" the user reported).
    //
    // d3-force is the industry standard (since 2015, used by Observable,
    // d3 itself, every viz tutorial on the web). It uses Verlet
    // integration with a Barnes-Hut quadtree for repulsion — so the
    // d² singularity gets a soft minimum and forces stay bounded.
    // Alpha decays automatically over ~300 ticks, removing the manual
    // ticksSinceLoad cutoff. Bundle cost: ~25 KB gzipped.
    import {
        forceSimulation, forceManyBody, forceLink, forceCenter, forceX, forceY,
        type Simulation, type SimulationNodeDatum, type SimulationLinkDatum,
    } from 'd3-force';

    const dispatch = createEventDispatcher<{
        close: void;
        openmemoria: { memoryId: number };
    }>();

    // ── Backend shape (mirror of MemoryGraph in memory.rs) ───────────────
    interface MemoryNode {
        id: number;
        title: string;
        importance: number;
        access_count: number;
        created_at: number;
        tags: string[];
        preview: string;
        community: number;    // -1 = singleton
    }
    interface MemoryEdge {
        source: number;
        target: number;
        kind: 'tag' | 'content' | 'embedding';
        weight: number;
    }
    interface MemoryGraphStats {
        edges_tag: number;
        edges_content: number;
        edges_embedding: number;
        community_count: number;
        density: number;
        orphan_count: number;
    }
    interface MemoryGraph {
        nodes: MemoryNode[];
        edges: MemoryEdge[];
        truncated_nodes: boolean;
        truncated_edges: boolean;
        total_memories: number;
        stats: MemoryGraphStats;
        top_tags: Array<[string, number]>;
    }

    // ── Internal simulation state ────────────────────────────────────────
    interface SimNode extends MemoryNode {
        x: number; y: number;
        vx: number; vy: number;
        pinned: boolean;
        degree: number;
    }

    let graph: MemoryGraph | null = null;
    let simNodes: SimNode[] = [];
    let nodesById: Map<number, SimNode> = new Map();
    /** id → Set of neighbor ids — precomputed once per load for fast hover */
    let neighborsOf: Map<number, Set<number>> = new Map();
    let loading = false;
    let error = '';
    let minImportance = 0;
    let selectedNode: SimNode | null = null;
    let hoveredNodeId: number | null = null;

    // V5 — Search state
    let searchQuery = '';
    // Tag filter — when set, only nodes carrying this tag stay full opacity
    let activeTagFilter: string | null = null;

    // V14 — Threshold sliders (sent to backend, trigger refetch)
    let tagThreshold = 0.30;
    let contentThreshold = 0.25;
    let embeddingThreshold = 0.65;
    let useEmbeddings = true;

    // ── Viewport ─────────────────────────────────────────────────────────
    let viewW = 800, viewH = 600;
    let zoom = 1.0;
    let panX = 0, panY = 0;

    // v1.7.86 — Canvas2D edge underlay. The reference is bound in the
    // template; paintEdges() is called from the simulation tick AND from
    // hover/pan handlers so the edges stay in sync with both the layout
    // settling and the viewport transform.
    let edgeCanvas: HTMLCanvasElement | null = null;
    let _edgeCtx: CanvasRenderingContext2D | null = null;
    // The drawable area is measured from this wrapper so the SVG/canvas fill the
    // whole panel (no smaller "cage" that clips the graph when panned).
    let canvasWrapEl: HTMLDivElement | null = null;
    let _resizeObs: ResizeObserver | null = null;

    // v1.7.87 — Typed semantic links. Loaded once per graph open
    // alongside the similarity edges. Rendered in the same canvas pass
    // but with a distinct colour palette per kind so the operator can
    // tell similarity from reasoning at a glance.
    interface SemLink {
        id: number; source_id: number; target_id: number;
        kind: string; confidence: number; note: string | null;
    }
    let semanticLinks: SemLink[] = [];
    const SEM_COLORS: Record<string, string> = {
        causal:       '#ef4444',  // red — flow of cause
        resolves:     '#10b981',  // green — fixed/solved
        derives_from: '#22d3ee',  // cyan — derivation
        references:   '#94a3b8',  // grey — neutral pointer
        contradicts:  '#f59e0b',  // amber — conflict
        refines:      '#a78bfa',  // violet — improvement
    };
    /** Cache DPR-corrected context. Re-acquired when canvas binds. */
    function _getEdgeCtx(): CanvasRenderingContext2D | null {
        if (!edgeCanvas) return null;
        if (!_edgeCtx) {
            _edgeCtx = edgeCanvas.getContext('2d', { alpha: true });
        }
        return _edgeCtx;
    }
    /** Repaint all edges in ONE pass. Runs in ~0.3 ms for 100 edges and
     *  doesn't touch the DOM. The hover-dim case is folded into the same
     *  alpha calculation as edgeOpacity uses for the (now-removed) SVG. */
    function paintEdges(): void {
        const ctx = _getEdgeCtx();
        if (!ctx || !graph) return;
        const dpr = window.devicePixelRatio || 1;
        // Clear in physical pixels.
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.clearRect(0, 0, edgeCanvas!.width, edgeCanvas!.height);
        // World-space transform: same convention as the SVG group
        // `transform="translate(panX*zoom, panY*zoom) scale(zoom)"`,
        // adjusted for devicePixelRatio so HiDPI displays render sharp.
        ctx.setTransform(zoom * dpr, 0, 0, zoom * dpr, panX * zoom * dpr, panY * zoom * dpr);

        // P1 — Node glow underlay: a soft radial halo in each node's colour
        // gives the flat circles depth ("neural node" look). Painted on the
        // canvas (below the crisp SVG nodes) so it costs one fill per node and
        // never touches the DOM. Grows in with the entrance animation.
        for (const node of simNodes) {
            const f = entranceFactor(node);
            if (f < 0.04) continue;
            const op = nodeOpacity(node);
            if (op < 0.05) continue;
            const r = nodeRadius(node) * (0.3 + 0.7 * f);
            const glowR = r * 2.6;
            const col = nodeColor(node);
            const g = ctx.createRadialGradient(node.x, node.y, r * 0.35, node.x, node.y, glowR);
            g.addColorStop(0, _hexToRgba(col, 0.36 * op));
            g.addColorStop(1, _hexToRgba(col, 0));
            ctx.globalAlpha = 1;
            ctx.fillStyle = g;
            ctx.beginPath();
            ctx.arc(node.x, node.y, glowR, 0, Math.PI * 2);
            ctx.fill();
        }

        for (const edge of graph.edges) {
            const a = nodesById.get(edge.source);
            const b = nodesById.get(edge.target);
            if (!a || !b) continue;
            const op = edgeOpacity(edge);
            if (op < 0.02) continue;       // sub-perceptual cull
            ctx.strokeStyle = edgeStroke(edge);
            ctx.globalAlpha = op;
            ctx.lineWidth = 0.7 + edge.weight * 2.2;
            // P1 — "alive" edges: embedding (semantic-similarity) links get a
            // slow flowing dash so the graph reads as a living memory rather
            // than a static diagram. The dash offset advances in the flow loop.
            if (edge.kind === 'embedding') {
                ctx.setLineDash([3, 9]);
                ctx.lineDashOffset = -(flowPhase * 0.5);
            } else {
                ctx.setLineDash([]);
            }
            ctx.beginPath();
            ctx.moveTo(a.x, a.y);
            ctx.lineTo(b.x, b.y);
            ctx.stroke();
        }
        ctx.setLineDash([]);
        // v1.7.87 — Typed semantic links drawn ON TOP of similarity
        // edges, slightly thicker so they read as the operational layer.
        // Each kind gets its own colour; we render an arrowhead on the
        // target end so the direction is unambiguous.
        for (const link of semanticLinks) {
            const a = nodesById.get(link.source_id);
            const b = nodesById.get(link.target_id);
            if (!a || !b) continue;
            const colour = SEM_COLORS[link.kind] || '#cbd5e1';
            const alpha = 0.5 + 0.5 * link.confidence;
            ctx.strokeStyle = colour;
            ctx.fillStyle   = colour;
            ctx.globalAlpha = alpha;
            ctx.lineWidth = 1.8;
            // Shorten the line slightly so the arrowhead doesn't overlap
            // the target circle. Use the node radius as the visual offset.
            const dx = b.x - a.x, dy = b.y - a.y;
            const dist = Math.hypot(dx, dy) || 1;
            const ux = dx / dist, uy = dy / dist;
            const targetR = nodeRadius(b) + 2;
            const tx = b.x - ux * targetR;
            const ty = b.y - uy * targetR;
            ctx.beginPath();
            ctx.moveTo(a.x, a.y);
            ctx.lineTo(tx, ty);
            ctx.stroke();
            // Arrowhead: 8-px triangle pointing at the target.
            const head = 8;
            const px = -uy, py = ux;          // perpendicular unit vector
            ctx.beginPath();
            ctx.moveTo(tx, ty);
            ctx.lineTo(tx - ux * head + px * head * 0.45, ty - uy * head + py * head * 0.45);
            ctx.lineTo(tx - ux * head - px * head * 0.45, ty - uy * head - py * head * 0.45);
            ctx.closePath();
            ctx.fill();
        }
        ctx.globalAlpha = 1;
    }

    let draggingNode: SimNode | null = null;
    let draggingPan = false;
    let lastPx = 0, lastPy = 0;

    // ── Force parameters (d3-force calibrated) ───────────────────────────
    // The d3 force model uses different units than our old Euler loop,
    // so these are not direct ports of the previous constants:
    //   • forceManyBody().strength(-NEG)  — repulsion, negative = repel.
    //     Values around -300 to -800 work well for graphs up to ~250
    //     nodes; we ramp it based on count so the layout stays
    //     comfortable at both small and large scales.
    //   • forceLink().distance(REST_LEN)  — preferred edge length.
    //   • forceLink().strength(s)         — spring stiffness, 0..1.
    //     Higher = edges contract harder. Same-community edges get a
    //     stronger pull so clusters separate visually.
    //   • forceX/forceY().strength(K)     — soft centering on the
    //     viewport midline. forceCenter() alone is too weak when
    //     edges drag nodes outward.
    const REST_LEN       = 110;
    const REPEL_STRENGTH = -420;   // base repulsion charge
    const SPRING_STRENGTH = 0.35;  // base edge stiffness

    /** Reference to the running d3 simulation. Replaced on every load. */
    let sim: Simulation<SimNode, SimulationLinkDatum<SimNode>> | null = null;
    let simRunning = false;
    let rafId: number | null = null;
    let ticksSinceLoad = 0;

    // ── Visual polish (P1): staggered entrance + idle edge-flow + node glow ──
    // entranceT 0→1 drives a fade/scale-in; default 1 so nodes are always
    // visible if anything short-circuits the animation. The flow loop animates
    // a subtle dash offset on the "alive" edges once the layout has settled;
    // it self-pauses while the sim runs and whenever the document is hidden, so
    // there is no idle CPU cost when the graph isn't on screen.
    let entranceT = 1;
    let _entrancePending = false;
    let entranceOrder = new Map<number, number>(); // id → stagger slot (hubs first)
    const ENTRANCE_MS = 520;
    let flowPhase = 0;
    let flowRafId: number | null = null;

    function _easeOutCubic(x: number): number { const t = 1 - x; return 1 - t * t * t; }
    /** Per-node entrance progress 0→1, staggered by degree (hubs pop first). */
    function entranceFactor(n: SimNode): number {
        if (entranceT >= 1) return 1;
        const N = Math.max(simNodes.length, 1);
        const slot = entranceOrder.get(n.id) ?? 0;
        return _easeOutCubic(Math.max(0, Math.min(1, entranceT * 2 - slot / N)));
    }
    /** Parse #rgb / #rrggbb → rgba() string with the given alpha. */
    function _hexToRgba(hex: string, a: number): string {
        let h = hex.replace('#', '');
        if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
        const r = parseInt(h.slice(0, 2), 16), g = parseInt(h.slice(2, 4), 16), b = parseInt(h.slice(4, 6), 16);
        return `rgba(${r},${g},${b},${a})`;
    }
    function runEntrance(): void {
        entranceT = 0;
        const start = performance.now();
        const step = () => {
            entranceT = Math.min(1, (performance.now() - start) / ENTRANCE_MS);
            simNodes = simNodes;        // re-render with the new factor
            paintEdges();
            if (entranceT < 1) requestAnimationFrame(step);
            else startFlowLoop();
        };
        requestAnimationFrame(step);
    }
    function startFlowLoop(): void {
        if (flowRafId != null || simRunning || entranceT < 1) return;
        if (typeof document !== 'undefined' && document.hidden) return;
        const step = () => {
            if (simRunning || (typeof document !== 'undefined' && document.hidden)) { flowRafId = null; return; }
            flowPhase += 1;
            paintEdges();
            flowRafId = requestAnimationFrame(step);
        };
        flowRafId = requestAnimationFrame(step);
    }
    function stopFlowLoop(): void {
        if (flowRafId != null) { cancelAnimationFrame(flowRafId); flowRafId = null; }
    }
    function onVisibilityChange(): void {
        if (typeof document !== 'undefined' && document.hidden) stopFlowLoop();
        else if (!simRunning && entranceT >= 1) startFlowLoop();
    }

    // ── V3 — Community palette (8 categorical colors, color-blind friendly)
    // Picked from the Okabe-Ito palette + tweaks for dark backgrounds.
    const COMMUNITY_PALETTE = [
        '#56b4e9',  // sky blue
        '#e69f00',  // orange
        '#009e73',  // bluish green
        '#f0e442',  // yellow
        '#d55e00',  // vermillion
        '#cc79a7',  // reddish purple
        '#0072b2',  // blue
        '#bfbfbf',  // gray
    ];
    function communityColor(c: number): string {
        if (c < 0) return '#5a6478'; // singleton
        return COMMUNITY_PALETTE[c % COMMUNITY_PALETTE.length];
    }

    // ── Backend fetch ────────────────────────────────────────────────────
    let _refetchTimer: ReturnType<typeof setTimeout> | null = null;
    function scheduleRefetch(): void {
        // Debounce slider input — wait 300ms after the last drag before
        // hitting the DB. Without this, dragging the slider triggers ~60
        // refetches per second.
        if (_refetchTimer) clearTimeout(_refetchTimer);
        _refetchTimer = setTimeout(() => { loadGraph(); }, 300);
    }

    async function loadGraph(): Promise<void> {
        loading = true;
        error = '';
        try {
            const g = await invoke<MemoryGraph>('memory_graph', {
                limit: 200,
                minImportance,
                tagThreshold,
                contentThreshold,
                embeddingThreshold,
                useEmbeddings,
            });
            graph = g;
            await initSimulation(g);
        } catch (e) {
            error = String(e);
            graph = null;
        } finally {
            loading = false;
        }
    }

    async function initSimulation(g: MemoryGraph): Promise<void> {
        stopSim();
        // Pre-compute degree + neighbors for sizing and V7 hover-highlight.
        const degree = new Map<number, number>();
        const neighbors = new Map<number, Set<number>>();
        for (const e of g.edges) {
            degree.set(e.source, (degree.get(e.source) ?? 0) + 1);
            degree.set(e.target, (degree.get(e.target) ?? 0) + 1);
            if (!neighbors.has(e.source)) neighbors.set(e.source, new Set());
            if (!neighbors.has(e.target)) neighbors.set(e.target, new Set());
            neighbors.get(e.source)!.add(e.target);
            neighbors.get(e.target)!.add(e.source);
        }
        neighborsOf = neighbors;

        const cx = viewW / 2;
        const cy = viewH / 2;
        const R = Math.min(viewW, viewH) * 0.35;

        // v1.7.85 — Layout cache. Load any previously-saved (x,y) for nodes
        // we've seen before. If the cache has the node, seed sim with its
        // last known position so d3-force starts from a stable layout
        // instead of the community ring → the pre-warm converges in ~30
        // ticks instead of 300, and the operator sees the same graph they
        // closed last time. Cache miss falls through to the existing
        // community-ring seed.
        //
        // Cache loads non-blocking but BEFORE the simNodes assignment so
        // its result is available when we build each node. We swallow
        // errors silently — cache is best-effort and the fallback layout
        // is fine.
        const _cachedPos = new Map<number, { x: number; y: number; pinned: number }>();
        try {
            const _rows = await invoke<Array<{ node_id: number; x: number; y: number; pinned: number }>>(
                'graph_layout_load'
            );
            for (const r of _rows) _cachedPos.set(r.node_id, r);
        } catch (_e) { /* no cache, no problem */ }

        // v1.7.87 — Pull typed semantic links in parallel with the
        // layout cache. Best-effort: failure just means the second edge
        // layer doesn't render this session.
        try {
            semanticLinks = await invoke<SemLink[]>('memory_link_list');
        } catch (_e) { semanticLinks = []; }

        simNodes = g.nodes.map((n, i) => {
            // v1.7.85 — Cache hit short-circuit. Same node id → same
            // position; no need to reseed by community.
            const _hit = _cachedPos.get(n.id);
            if (_hit) {
                return {
                    ...n,
                    x: _hit.x, y: _hit.y,
                    vx: 0, vy: 0,
                    pinned: _hit.pinned === 1,
                    degree: degree.get(n.id) ?? 0,
                };
            }
            // V3 — seed initial position by community: nodes of the same
            // community cluster start near each other → faster convergence,
            // visible clusters from frame 1. d3-force respects whatever
            // x/y we pre-set; if we left them undefined it would use its
            // own phyllotaxis seed which doesn't know about communities.
            const c = n.community < 0 ? i : n.community;
            const angle = (c / Math.max(g.stats.community_count, 1)) * Math.PI * 2
                        + (i % 7) * 0.3;
            const r = R * (0.6 + 0.4 * Math.random());
            return {
                ...n,
                x: cx + Math.cos(angle) * r,
                y: cy + Math.sin(angle) * r,
                vx: 0, vy: 0,
                pinned: false,
                degree: degree.get(n.id) ?? 0,
            };
        });
        nodesById = new Map(simNodes.map((n) => [n.id, n]));
        // P1 — entrance stagger order: hubs (high degree) pop in first so the
        // graph "grows" from its backbone outward. Re-armed on every load.
        const _ordered = [...simNodes].sort((a, b) => b.degree - a.degree);
        entranceOrder = new Map(_ordered.map((n, i) => [n.id, i]));
        _entrancePending = true;
        ticksSinceLoad = 0;
        // v1.7.85 — Re-arm the layout-persist one-shot per reload so a
        // threshold change saves the new arrangement once it settles.
        _layoutPersisted = false;
        // Re-arm the auto-fit one-shot whenever the data reloads. Without
        // this, refetch (changing thresholds) keeps stale viewport.
        autoFitPending = true;
        startSim();
    }

    /** When true, the next time `ticksSinceLoad` reaches AUTOFIT_AT we'll
     *  auto-fit the viewport to the graph. Reset on every load.
     *  AUTOFIT_AT = 60: enough ticks for d3-force to settle (alpha ~0.3)
     *  but BEFORE the user has had time to interact (drag/zoom). */
    const AUTOFIT_AT = 60;
    let autoFitPending = true;

    function startSim(): void {
        if (!graph) return;
        stopFlowLoop();   // the sim's own RAF drives painting while it runs
        // Tear down any prior simulation before building a new one.
        if (sim) { sim.stop(); sim = null; }

        // v1.7.72 — Deduplicate edges for the spring force. The backend
        // emits up to 3 edges between the same pair of nodes (one per
        // kind: tag, content, embedding). Feeding all of them to
        // forceLink stacks 3 parallel springs, which crushes node pairs
        // together and inflates the effective stiffness so that any
        // residual repulsion sends outliers flying. We keep the RENDER
        // loop iterating over graph.edges (so colours stay correct) but
        // pass only the MAX-weight edge per pair to the physics.
        const pairKey = (s: number, t: number) => (s < t ? `${s}-${t}` : `${t}-${s}`);
        const bestPerPair = new Map<string, { source: number; target: number; weight: number }>();
        for (const e of graph.edges) {
            if (!nodesById.has(e.source) || !nodesById.has(e.target)) continue;
            const k = pairKey(e.source, e.target);
            const prev = bestPerPair.get(k);
            if (!prev || (e.weight ?? 0) > (prev.weight ?? 0)) {
                bestPerPair.set(k, { source: e.source, target: e.target, weight: e.weight });
            }
        }
        const links: SimulationLinkDatum<SimNode>[] = Array.from(bestPerPair.values()).map(e => ({
            source: nodesById.get(e.source)!,
            target: nodesById.get(e.target)!,
            weight: e.weight,
        } as any));

        // Repulsion strength: scale gently with node count so dense
        // graphs don't compress into a hairball and sparse ones don't
        // explode outward. Range stays bounded so nothing escapes.
        const n = simNodes.length;
        const repelStrength = REPEL_STRENGTH * (n < 20 ? 1.3 : n < 80 ? 1.0 : 0.8);

        // v1.7.72 — per-node gravity. Orphans (degree 0) carry no springs
        // pulling them back to the cluster, so pure repulsion would
        // drift them off-canvas. Give them ~5× the centering strength of
        // a connected node so they settle near the centre as halo
        // around the main cluster instead of running to the edges.
        const gravityX = (d: SimNode) => (d.degree === 0 ? 0.20 : 0.045);
        const gravityY = (d: SimNode) => (d.degree === 0 ? 0.20 : 0.045);

        sim = forceSimulation<SimNode>(simNodes)
            .force('charge', forceManyBody<SimNode>()
                .strength(repelStrength)
                .distanceMin(20)     // soft minimum — kills the d² singularity
                .distanceMax(420))   // tighter cutoff: outliers no longer feel push from the cluster
            .force('link', forceLink<SimNode, SimulationLinkDatum<SimNode>>(links)
                .id((d) => d.id)
                .distance((l: any) => {
                    // Slightly shorter rest length within a community so
                    // clusters look tight; longer across communities.
                    const a = l.source as SimNode, b = l.target as SimNode;
                    const sameComm = a.community >= 0 && a.community === b.community;
                    return sameComm ? REST_LEN * 0.85 : REST_LEN * 1.15;
                })
                .strength((l: any) => {
                    const a = l.source as SimNode, b = l.target as SimNode;
                    const sameComm = a.community >= 0 && a.community === b.community;
                    const w = (l as any).weight ?? 0.5;
                    return SPRING_STRENGTH * (0.6 + w) * (sameComm ? 1.3 : 1.0);
                }))
            .force('center', forceCenter(viewW / 2, viewH / 2).strength(0.06))
            .force('x', forceX<SimNode>(viewW / 2).strength(gravityX))
            .force('y', forceY<SimNode>(viewH / 2).strength(gravityY))
            .alpha(1)
            .alphaDecay(0.022)   // standard d3 default → settles in ~300 ticks
            .alphaMin(0.001)
            .velocityDecay(0.45) // a touch heavier damping reduces overshoot
            .stop();             // we drive ticks manually so the SVG can re-render via Svelte reactivity

        // v1.7.72 — Pre-warm synchronously. Run the sim to near
        // convergence BEFORE the first paint so the user never sees an
        // intermediate state (the "weird pyramid" was a half-settled
        // sim that was then frozen by an early autofit). 300 ticks is
        // enough for alpha to fall below 0.002 at our decay rate.
        for (let i = 0; i < 300; i++) {
            sim.tick();
        }

        // Autofit once on already-settled positions, then immediately
        // again on the next frame (cheap; SVG reads x/y reactively).
        fitToView();
        autoFitPending = false;
        ticksSinceLoad = 300;
        simNodes = simNodes;   // trigger first render with settled positions
        paintEdges();          // v1.7.86 — initial canvas paint after pre-warm

        // P1 — play the staggered entrance once per fresh load (not on the
        // re-heat startSim that a drag triggers, so dragging never replays it).
        if (_entrancePending) { _entrancePending = false; runEntrance(); }
        else { entranceT = 1; }

        // Continue the sim for the residual decay + interactivity. With
        // alpha already < 0.002, this loop costs ~0 CPU until the user
        // drags a node, which calls reheat() to wake it up.
        simRunning = true;
        const tick = () => {
            if (!simRunning || !sim) return;
            sim.tick();
            // Honour user-pinned nodes (drag). d3 honours fx/fy set by
            // the drag handler; we just zero velocity here as belt+braces.
            for (const node of simNodes) {
                if (node.pinned) {
                    node.vx = 0; node.vy = 0;
                }
            }
            ticksSinceLoad++;
            if ((sim.alpha() < sim.alphaMin()) || ticksSinceLoad > 900) {
                simRunning = false;
                rafId = null;
                // v1.7.85 — Persist final settled layout once the sim
                // converges, so next open is instant.
                _persistLayoutIfNeeded();
                startFlowLoop();   // P1 — hand off to the idle edge-flow shimmer
                return;
            }
            simNodes = simNodes;   // trigger Svelte re-render
            paintEdges();          // v1.7.86 — canvas edge repaint
            rafId = requestAnimationFrame(tick);
        };
        rafId = requestAnimationFrame(tick);
    }
    function stopSim(): void {
        simRunning = false;
        if (rafId != null) { cancelAnimationFrame(rafId); rafId = null; }
        if (sim) { sim.stop(); sim = null; }
    }

    /** v1.7.85 — Persist current (x,y) layout so the next graph open is
     *  instant. Called from onDestroy and from sim alpha-decay completion.
     *  Fire-and-forget — failure is silent, the operator just gets a
     *  cold restart on next open. NaN/Inf are filtered backend-side as
     *  a defensive belt-and-braces. */
    let _layoutPersisted = false;
    async function _persistLayoutIfNeeded(): Promise<void> {
        if (_layoutPersisted || simNodes.length === 0) return;
        _layoutPersisted = true;
        const _entries = simNodes
            .filter(n => Number.isFinite(n.x) && Number.isFinite(n.y))
            .map(n => ({ node_id: n.id, x: n.x, y: n.y, pinned: n.pinned ? 1 : 0 }));
        try {
            await invoke('graph_layout_save_bulk', { entries: _entries });
        } catch (_e) { /* best-effort */ }
    }

    /** Re-heat the simulation when the user interacts (drag, slider, etc.).
     *  Equivalent to d3's restart() pattern. */
    function reheat(alpha = 0.3): void {
        if (!sim) return;
        sim.alpha(alpha).restart();
        sim.stop();              // we keep manual tick control
        ticksSinceLoad = 0;
        if (!simRunning) startSim();
    }

    // ── Pointer handlers ─────────────────────────────────────────────────
    function screenToWorld(sx: number, sy: number, rect: DOMRect): { x: number; y: number } {
        return {
            x: (sx - rect.left) / zoom - panX,
            y: (sy - rect.top)  / zoom - panY,
        };
    }
    function onNodePointerDown(ev: PointerEvent, node: SimNode): void {
        ev.stopPropagation();
        (ev.target as Element)?.setPointerCapture?.(ev.pointerId);
        draggingNode = node;
        node.pinned = true;
        // v1.7.71 — d3 honours node.fx / node.fy as "fixed" coordinates
        // it won't integrate. Without these, sim.tick() would keep
        // moving the dragged node based on stale velocity.
        (node as any).fx = node.x;
        (node as any).fy = node.y;
        reheat(0.3);
    }
    function onPointerMove(ev: PointerEvent): void {
        if (draggingNode) {
            const svgEl = ev.currentTarget as SVGElement;
            const rect = svgEl.getBoundingClientRect();
            const w = screenToWorld(ev.clientX, ev.clientY, rect);
            draggingNode.x = w.x;
            draggingNode.y = w.y;
            (draggingNode as any).fx = w.x;
            (draggingNode as any).fy = w.y;
            simNodes = simNodes;
            paintEdges();                  // v1.7.86 — drag is mid-frame
        } else if (draggingPan) {
            panX += (ev.clientX - lastPx) / zoom;
            panY += (ev.clientY - lastPy) / zoom;
            lastPx = ev.clientX;
            lastPy = ev.clientY;
            clampPan();                    // keep the graph from leaving the view
            paintEdges();                  // v1.7.86 — repaint on pan
        }
    }
    function onPointerUp(ev: PointerEvent): void {
        if (draggingNode) {
            (ev.target as Element)?.releasePointerCapture?.(ev.pointerId);
            // v1.7.71 — release the d3 fix so the node rejoins the
            // simulation. Pinning toggle still lives on node.pinned for
            // the visual indicator + autofit.
            (draggingNode as any).fx = null;
            (draggingNode as any).fy = null;
            draggingNode = null;
        }
        draggingPan = false;
    }
    function onSvgPointerDown(_ev: PointerEvent): void {
        draggingPan = true;
        lastPx = _ev.clientX;
        lastPy = _ev.clientY;
    }
    function onWheel(ev: WheelEvent): void {
        ev.preventDefault();
        const svgEl = ev.currentTarget as SVGElement;
        const rect = svgEl.getBoundingClientRect();
        const wxBefore = (ev.clientX - rect.left) / zoom - panX;
        const wyBefore = (ev.clientY - rect.top)  / zoom - panY;
        const factor = ev.deltaY < 0 ? 1.15 : 1 / 1.15;
        zoom = Math.max(0.2, Math.min(4, zoom * factor));
        const wxAfter = (ev.clientX - rect.left) / zoom - panX;
        const wyAfter = (ev.clientY - rect.top)  / zoom - panY;
        panX += (wxAfter - wxBefore);
        panY += (wyAfter - wyBefore);
        clampPan();                        // keep the graph from leaving the view
        paintEdges();                      // v1.7.86 — repaint on zoom
    }
    function onNodeClick(ev: MouseEvent, node: SimNode): void {
        ev.stopPropagation();
        selectedNode = node;
    }
    function closeDetail(): void { selectedNode = null; }

    /**
     * Auto-fit zoom + pan so every node is visible with comfortable padding.
     * Without this, after the force sim runs, nodes often end up partially
     * off-screen — especially small graphs where the center attraction is
     * weak relative to repulsion. The user reported one edge going off the
     * canvas — this prevents it.
     *
     * Padding: 60px on each side so labels at the edges aren't clipped.
     * Zoom clamped to [0.5, 2.5] to avoid extremes that hurt readability.
     */
    function fitToView(): void {
        if (simNodes.length === 0) return;
        let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
        for (const n of simNodes) {
            if (n.x < minX) minX = n.x;
            if (n.x > maxX) maxX = n.x;
            if (n.y < minY) minY = n.y;
            if (n.y > maxY) maxY = n.y;
        }
        const PAD = 60;
        const bboxW = Math.max(1, maxX - minX);
        const bboxH = Math.max(1, maxY - minY);
        const scaleX = (viewW - PAD * 2) / bboxW;
        const scaleY = (viewH - PAD * 2) / bboxH;
        // BUG FIX (May 2026): autofit used to allow zoom UP TO 2.5 — this
        // magnified small graphs so nodes + labels grew proportionally and
        // labels overlapped because the visual radius was now huge. The fix:
        // never zoom past 1.0 (= "show at natural size"). Small graphs
        // simply use the viewport; the user can wheel-in if they want detail.
        const newZoom = Math.min(scaleX, scaleY, 1.0);
        zoom = Math.max(0.3, newZoom);
        // Center the bbox in the viewport. The transform is
        // `translate(panX*zoom, panY*zoom) scale(zoom)` so we need:
        //   bboxCenter * zoom + panOffset = viewCenter
        // → panOffset (= panX * zoom) = viewCenter - bboxCenter * zoom
        const bboxCx = (minX + maxX) / 2;
        const bboxCy = (minY + maxY) / 2;
        panX = (viewW / 2 - bboxCx * zoom) / zoom;
        panY = (viewH / 2 - bboxCy * zoom) / zoom;
    }

    /** Keep the graph from being panned entirely off-screen ("dragging right
     *  made the data disappear"). Clamps panX/panY so at least MARGIN screen px
     *  of the node cloud stays inside the viewport on every side. If the graph
     *  is larger than the viewport on an axis, that axis is left free so the
     *  user can still explore a big graph. */
    function clampPan(): void {
        if (simNodes.length === 0) return;
        let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
        for (const n of simNodes) {
            if (n.x < minX) minX = n.x; if (n.x > maxX) maxX = n.x;
            if (n.y < minY) minY = n.y; if (n.y > maxY) maxY = n.y;
        }
        const MARGIN = 80; // screen px of the graph that must stay visible
        // screenX = (wx + panX) * zoom  ⇒ solve the visibility bounds for panX.
        const minPanX = MARGIN / zoom - maxX, maxPanX = (viewW - MARGIN) / zoom - minX;
        const minPanY = MARGIN / zoom - maxY, maxPanY = (viewH - MARGIN) / zoom - minY;
        if (minPanX <= maxPanX) panX = Math.max(minPanX, Math.min(maxPanX, panX));
        if (minPanY <= maxPanY) panY = Math.max(minPanY, Math.min(maxPanY, panY));
    }

    /** "Reset view" button — re-fits instead of returning to zoom=1/pan=0,
     *  which is what the user actually wants ("show me everything"). */
    function resetView(): void { fitToView(); }
    function resetPins(): void {
        // v1.7.71 — Also clear the d3 fx/fy fields, otherwise the
        // "pin" toggle would visually flip but the node would stay
        // frozen at the fixed coordinate.
        for (const n of simNodes) {
            n.pinned = false;
            (n as any).fx = null;
            (n as any).fy = null;
        }
        ticksSinceLoad = 0;
        autoFitPending = true;
        reheat(0.6);
    }
    function onKeyDown(ev: KeyboardEvent): void {
        if (ev.key === 'Escape') {
            if (selectedNode) closeDetail();
            else dispatch('close');
        }
    }

    // V12 — Open this memory in the Memorias tab and close the overlay.
    function openInBrowser(node: SimNode): void {
        dispatch('openmemoria', { memoryId: node.id });
        dispatch('close');
    }

    // ── Reactive filters ─────────────────────────────────────────────────
    $: searchQueryLower = searchQuery.trim().toLowerCase();
    function nodeMatchesSearch(n: SimNode): boolean {
        if (!searchQueryLower) return true;
        if (n.title.toLowerCase().includes(searchQueryLower)) return true;
        if (n.preview.toLowerCase().includes(searchQueryLower)) return true;
        for (const t of n.tags) if (t.toLowerCase().includes(searchQueryLower)) return true;
        return false;
    }
    function nodeMatchesTag(n: SimNode): boolean {
        if (!activeTagFilter) return true;
        const af = activeTagFilter.toLowerCase();
        for (const t of n.tags) if (t.toLowerCase() === af) return true;
        return false;
    }

    /** V7 — Opacity of a node given current hover/search/tag filters. */
    function nodeOpacity(n: SimNode): number {
        const matchesSearch = nodeMatchesSearch(n);
        const matchesTag = nodeMatchesTag(n);
        if (!matchesSearch || !matchesTag) return 0.12;
        if (hoveredNodeId == null) return 1.0;
        if (n.id === hoveredNodeId) return 1.0;
        const nbrs = neighborsOf.get(hoveredNodeId);
        if (nbrs && nbrs.has(n.id)) return 1.0;
        return 0.2;
    }
    function edgeOpacity(e: MemoryEdge): number {
        const aMatches = nodesById.get(e.source);
        const bMatches = nodesById.get(e.target);
        if (!aMatches || !bMatches) return 0;
        if (!nodeMatchesSearch(aMatches) || !nodeMatchesSearch(bMatches)) return 0.05;
        if (!nodeMatchesTag(aMatches) || !nodeMatchesTag(bMatches)) return 0.05;
        if (hoveredNodeId == null) return 1.0;
        if (e.source === hoveredNodeId || e.target === hoveredNodeId) return 1.0;
        return 0.08;
    }

    // ── Visual mapping ───────────────────────────────────────────────────
    // V11 — Bigger nodes: more weight on importance + degree.
    function nodeRadius(n: SimNode): number {
        const baseR = 6 + Math.min(n.importance, 10) * 1.0;
        const degBonus = Math.min(n.degree * 0.8, 8);
        return baseR + degBonus;
    }
    function nodeColor(n: SimNode): string {
        if (n.pinned) return '#f59e0b';
        if (selectedNode?.id === n.id) return '#10b981';
        return communityColor(n.community);
    }
    function edgeStroke(e: MemoryEdge): string {
        if (e.kind === 'tag')       return 'rgba(244,114,182,0.45)';
        if (e.kind === 'embedding') return 'rgba(168,85,247,0.40)';
        return 'rgba(96,165,250,0.30)'; // content
    }

    // V14 — Slider input is debounced so we only refetch once.
    $: if (tagThreshold || contentThreshold || embeddingThreshold || useEmbeddings != null) {
        // The condition is artificial — Svelte tracks deps. Always-true.
        // We use scheduleRefetch directly to debounce.
    }

    /**
     * Bug fix (May 2026): when two nodes are visually close, their labels
     * overlap and become illegible. We hide the label of the LOWER-degree
     * node when a higher-degree neighbor sits within ~80 userspace px.
     * Hovered or selected nodes always keep their label regardless.
     *
     * Cheap: O(n) per node since we already have `nodesById`. Recomputed
     * reactively only when sim or zoom changes.
     */
    function shouldShowLabel(n: SimNode): boolean {
        // Always show for the actively-engaged node.
        if (hoveredNodeId === n.id) return true;
        if (selectedNode?.id === n.id) return true;
        // During the entrance animation, labels fade in only once their node
        // has mostly arrived — avoids text floating over half-sized dots.
        if (entranceT < 1 && entranceFactor(n) < 0.85) return false;
        // Hide labels of isolated nodes unless zoomed in (they're often
        // noise — auto-saved memories without context). The user can wheel-
        // in to see them.
        if (n.degree === 0 && zoom < 1.3) return false;
        // Anti-collision using approximate LABEL BOXES, not just node distance.
        // Long titles overlap horizontally even when their nodes sit far apart
        // ("Skill OSINT…" over "CyberArk EPM…"), so we compare each label's
        // actual extent (width from char count, height from line height) and
        // hide the lower-degree node's label on a real overlap. Ties: lower id.
        const myHalfW  = _labelHalfWidth(n);
        const myLabelY = n.y - labelYOffset(nodeRadius(n));
        const lineH    = labelFontSize * 1.25;
        for (const other of simNodes) {
            if (other.id === n.id) continue;
            if (other.degree === 0 && zoom < 1.3) continue; // wouldn't show a label anyway
            const overlapX = Math.abs(other.x - n.x) < (myHalfW + _labelHalfWidth(other) + 4);
            const overlapY = Math.abs((other.y - labelYOffset(nodeRadius(other))) - myLabelY) < lineH;
            if (overlapX && overlapY) {
                if (other.degree > n.degree) return false;
                if (other.degree === n.degree && other.id < n.id) return false;
            }
        }
        return true;
    }

    /** Approximate half-width of a node's rendered label in userspace px. The
     *  label font is mono (~0.6 em/char); labelFontSize already counter-scales
     *  the wheel zoom, so this stays correct at any zoom level. */
    function _labelHalfWidth(n: SimNode): number {
        const chars = Math.min(n.title.length, 32);
        return chars * labelFontSize * 0.30;
    }

    /**
     * Counter-scale font size so labels stay approximately 11px on screen
     * regardless of the user's wheel-zoom level. Without this, zooming in
     * past 1.5x makes text gigantic and unreadable. The text element lives
     * inside the scaled <g>, so we divide our target size by zoom.
     */
    $: labelFontSize = Math.max(7, Math.min(14, 11 / zoom));
    /** Same idea for the y-offset above the node. */
    $: labelYOffset = (size: number) => size + 6 / zoom;

    function onTagPillClick(tag: string): void {
        activeTagFilter = (activeTagFilter === tag) ? null : tag;
    }

    /** Programmatically focus the SVG so keyboard works without click. */
    function focusCanvas(): void {
        tick().then(() => {
            // `getElementById` returns `HTMLElement | null`, and TS refuses a
            // direct cast to `SVGElement` because neither type contains the
            // other. The element IS an <svg>, so the honest route is through
            // `Element` — which both extend — rather than the double cast this
            // had, which asserted SVGElement and then immediately asserted it
            // back to HTMLElement to call focus().
            const el = document.getElementById('mg-canvas') as Element | null;
            (el as HTMLElement | null)?.focus?.();
        });
    }

    // ── Lifecycle ────────────────────────────────────────────────────────
    /** Match the drawable area (viewW/viewH) to the actual panel size so the
     *  graph never sits in a smaller box than its container. Returns true if the
     *  size changed. */
    function measureViewport(): boolean {
        if (!canvasWrapEl) return false;
        const w = canvasWrapEl.clientWidth;
        const h = canvasWrapEl.clientHeight;
        if (w > 0 && h > 0 && (w !== viewW || h !== viewH)) {
            viewW = w; viewH = h;
            return true;
        }
        return false;
    }

    onMount(() => {
        window.addEventListener('keydown', onKeyDown);
        document.addEventListener('visibilitychange', onVisibilityChange);
        // Fill the whole panel (was capped at 1400×900, which left the SVG
        // smaller than its container on large screens — the "cage" that clipped
        // the graph when panned). Re-fit on resize.
        measureViewport();
        if (typeof ResizeObserver !== 'undefined' && canvasWrapEl) {
            _resizeObs = new ResizeObserver(() => {
                if (measureViewport() && graph) { fitToView(); paintEdges(); }
            });
            _resizeObs.observe(canvasWrapEl);
        }
        loadGraph().then(focusCanvas);
    });
    onDestroy(() => {
        stopFlowLoop();
        if (_resizeObs) { try { _resizeObs.disconnect(); } catch {} _resizeObs = null; }
        document.removeEventListener('visibilitychange', onVisibilityChange);
        // v1.7.85 — Persist layout before tearing down. Sync-fire so the
        // bulk write hits the backend before the component unmounts.
        // (Tauri invokes are network-async but the Rust handler enqueues
        // immediately; even if it doesn't fully complete before the
        // component is gone, the call is in flight.)
        _persistLayoutIfNeeded();
        stopSim();
        window.removeEventListener('keydown', onKeyDown);
        if (_refetchTimer) clearTimeout(_refetchTimer);
    });

    // Re-fetch on importance filter change (immediate, no debounce — slider rarely moves)
    let lastMinImp = minImportance;
    $: if (minImportance !== lastMinImp) {
        lastMinImp = minImportance;
        loadGraph();
    }
</script>

<div class="mg-overlay" role="dialog" aria-label="Memory graph">
    <!-- ── Header ───────────────────────────────────────────────────────── -->
    <div class="mg-header">
        <div class="mg-title">
            <span class="mg-glyph">◊</span>
            <span>{$trad('Grafo de Memoria')}</span>
            {#if graph}
                <span class="mg-count">
                    {graph.nodes.length}{graph.truncated_nodes ? '/' + graph.total_memories : ''} {$trad('nodos')}
                    · {graph.edges.length} {$trad('aristas')}
                    · {graph.stats.community_count} {$trad('clusters')}
                    · {graph.stats.orphan_count} {$trad('huérfanas')}
                </span>
            {/if}
        </div>
        <div class="mg-actions">
            <!-- V5 — Search bar -->
            <input class="mg-search" type="search"
                   bind:value={searchQuery}
                   placeholder={$trad('⌕ Buscar título / tag / contenido…')}/>
            <label class="mg-filter" title={$trad('Importancia mínima')}>
                <span>min⮬</span>
                <input type="range" min="0" max="10" step="1" bind:value={minImportance}/>
                <span class="mg-filter-val">{minImportance}</span>
            </label>
            <button class="mg-btn" on:click={resetView}>↺ {$trad('vista')}</button>
            <button class="mg-btn" on:click={resetPins}>⤓ {$trad('fijados')}</button>
            <button class="mg-btn" on:click={loadGraph} disabled={loading}>↻</button>
            <button class="mg-btn mg-close" on:click={() => dispatch('close')} title="Esc">✕</button>
        </div>
    </div>

    <!-- ── Filter row: tag pills + threshold sliders ───────────────────── -->
    {#if graph}
    <div class="mg-filter-row">
        <!-- Tag pills (top 12) -->
        <div class="mg-tag-pills">
            <span class="mg-tag-label">{$trad('Tags:')}</span>
            {#each graph.top_tags as [tag, count]}
                <button class="mg-tag-pill"
                        class:active={activeTagFilter === tag}
                        on:click={() => onTagPillClick(tag)}>
                    {tag} <span class="mg-tag-count">{count}</span>
                </button>
            {/each}
            {#if activeTagFilter}
                <button class="mg-tag-clear" on:click={() => activeTagFilter = null}
                        title={$trad('Limpiar filtro de tag')}>✕</button>
            {/if}
        </div>
        <!-- V14 — Threshold sliders -->
        <details class="mg-thresholds">
            <summary>⚙ {$trad('umbrales')}</summary>
            <div class="mg-thr-grid">
                <label>
                    <span class="mg-thr-lbl pink">tag</span>
                    <input type="range" min="0.05" max="0.95" step="0.05"
                           bind:value={tagThreshold}
                           on:input={scheduleRefetch}/>
                    <span class="mg-thr-val">{tagThreshold.toFixed(2)}</span>
                </label>
                <label>
                    <span class="mg-thr-lbl blue">content</span>
                    <input type="range" min="0.05" max="0.95" step="0.05"
                           bind:value={contentThreshold}
                           on:input={scheduleRefetch}/>
                    <span class="mg-thr-val">{contentThreshold.toFixed(2)}</span>
                </label>
                <label>
                    <span class="mg-thr-lbl purple">embedding</span>
                    <input type="range" min="0.30" max="0.99" step="0.01"
                           bind:value={embeddingThreshold}
                           on:input={scheduleRefetch}
                           disabled={!useEmbeddings}/>
                    <span class="mg-thr-val">{embeddingThreshold.toFixed(2)}</span>
                </label>
                <label class="mg-thr-toggle">
                    <input type="checkbox"
                           bind:checked={useEmbeddings}
                           on:change={scheduleRefetch}/>
                    <span>{$trad('Usar embeddings')}</span>
                </label>
            </div>
        </details>
    </div>
    {/if}

    <!-- ── Canvas ──────────────────────────────────────────────────────── -->
    <div class="mg-canvas-wrap" bind:this={canvasWrapEl}>
        {#if loading && !graph}
            <div class="mg-empty">{$trad('Cargando…')}</div>
        {:else if error}
            <div class="mg-empty mg-err">{error}</div>
        {:else if graph && graph.nodes.length === 0}
            <div class="mg-empty">
                {$trad('Sin memorias todavía. A medida que Lucy aprende, este grafo se llena.')}
            </div>
        {:else if graph && graph.edges.length === 0}
            <div class="mg-empty mg-hint">
                <div style="font-size:16px;margin-bottom:8px;">∅</div>
                {$trad('Tus memorias existen pero ninguna alcanza los umbrales de similitud para conectarse. Baja los umbrales en ⚙ arriba (prueba tag 0.15 / content 0.15) para ver aristas.')}
            </div>
        {:else if graph}
            <!-- v1.7.86 — Canvas2D overlay for edges. The previous SVG
                 implementation rendered every edge as a <line> DOM node;
                 with 17 nodes and ~90 edges, every d3-force tick updated
                 90 DOM elements × 60 fps = 5400 DOM mutations/sec just
                 for the edges. Even with morphdom and Svelte's reactive
                 batching, this was the single biggest cost in the graph
                 view — visible as stutter on larger graphs.

                 The canvas underlay paints all edges in ONE draw call
                 per simulation tick. SVG above keeps the nodes (which
                 still need pointer events for drag/hover/click) and the
                 labels (few, and benefit from native text rendering).
                 The two layers share the same world-space transform so
                 they overlay perfectly: edges connect node centers
                 exactly even at high zoom levels. -->
            <canvas bind:this={edgeCanvas}
                    class="mg-edge-canvas"
                    width={viewW * (window.devicePixelRatio || 1)}
                    height={viewH * (window.devicePixelRatio || 1)}
                    style="width:{viewW}px;height:{viewH}px;"
                    aria-hidden="true"></canvas>
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <svg id="mg-canvas" width={viewW} height={viewH}
                 role="application" tabindex="0"
                 aria-label={$trad('Grafo de memoria — arrastra nodos / rueda zoom / escribe para buscar')}
                 on:pointerdown={onSvgPointerDown}
                 on:pointermove={onPointerMove}
                 on:pointerup={onPointerUp}
                 on:wheel|preventDefault={onWheel}
                 viewBox="0 0 {viewW} {viewH}"
                 class:dragging={draggingPan || draggingNode !== null}
                 style="touch-action:none;">
                <defs>
                    <radialGradient id="mg-sheen" cx="50%" cy="50%" r="50%">
                        <stop offset="0%"   stop-color="rgba(255,255,255,0.50)"/>
                        <stop offset="55%"  stop-color="rgba(255,255,255,0.08)"/>
                        <stop offset="100%" stop-color="rgba(255,255,255,0)"/>
                    </radialGradient>
                </defs>
                <g transform="translate({panX * zoom},{panY * zoom}) scale({zoom})">
                    <!-- v1.7.86 — Edges moved to the <canvas> above for
                         performance (SVG <line> per edge × 60 fps was
                         the bottleneck). The data + helpers (edgeOpacity,
                         edgeStroke) live on, consumed by paintEdges(). -->

                    <!-- Nodes -->
                    {#each simNodes as node (node.id)}
                        {@const f  = entranceFactor(node)}
                        {@const op = nodeOpacity(node) * f}
                        {@const r  = nodeRadius(node) * (0.35 + 0.65 * f)}
                        <g class="mg-node" class:pinned={node.pinned}
                           role="button" tabindex="0"
                           aria-label={node.title}
                           opacity={op}
                           on:pointerdown={(e) => onNodePointerDown(e, node)}
                           on:click={(e) => onNodeClick(e, node)}
                           on:mouseenter={() => { hoveredNodeId = node.id; paintEdges(); }}
                           on:mouseleave={() => { hoveredNodeId = null; paintEdges(); }}
                           on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); selectedNode = node; } }}>
                            <!-- P2 — pinned ring (amber, slow breath) -->
                            {#if node.pinned}
                                <circle class="mg-pin-ring" cx={node.x} cy={node.y} r={r + 3}
                                        fill="none" stroke="#f59e0b" stroke-width="1.3" pointer-events="none"/>
                            {/if}
                            <!-- P2 — selection halo (pulsing) on the open node -->
                            {#if selectedNode?.id === node.id}
                                <circle class="mg-sel-halo" cx={node.x} cy={node.y} r={r + 4}
                                        fill="none" stroke="#10b981" stroke-width="1.6" pointer-events="none"/>
                            {/if}
                            <!-- P2 — hover ripple: an expanding ring (CSS-animated on
                                 :hover of the group), in the node's own colour. -->
                            <circle class="mg-ripple" cx={node.x} cy={node.y} r={r}
                                    fill="none" stroke={nodeColor(node)} stroke-width="1.3" pointer-events="none"/>
                            <circle class="mg-node-core" cx={node.x} cy={node.y} r={r}
                                    fill={nodeColor(node)}
                                    stroke="rgba(255,255,255,0.55)"
                                    stroke-width="0.9"/>
                            <!-- P1 — glossy sheen: a white radial highlight offset
                                 up-left gives the flat disc a lit, dimensional feel. -->
                            <circle cx={node.x} cy={node.y - r * 0.3} r={r * 0.74}
                                    fill="url(#mg-sheen)" pointer-events="none"/>
                            <!-- V2 — Label with anti-collision + zoom-counter-scale.
                                 Hidden when a denser neighbor is within ~90px to
                                 avoid the overlap the user reported in
                                 Memory Graph 2.0 first iteration. -->
                            {#if shouldShowLabel(node)}
                                <text x={node.x} y={node.y - labelYOffset(r)}
                                      text-anchor="middle"
                                      font-size={labelFontSize}
                                      class="mg-node-label">{node.title.slice(0, 32)}</text>
                            {/if}
                        </g>
                    {/each}
                </g>
            </svg>
        {/if}

        <!-- ── Detail panel ────────────────────────────────────────────── -->
        {#if selectedNode}
            <aside class="mg-detail">
                <div class="mg-detail-hdr">
                    <span class="mg-detail-title">{selectedNode.title}</span>
                    <button class="mg-btn mg-close" on:click={closeDetail}>✕</button>
                </div>
                <div class="mg-detail-meta">
                    <span title={$trad('Importancia')}>⮬ {selectedNode.importance}</span>
                    <span title={$trad('Veces accedida')}>◎ {selectedNode.access_count}</span>
                    <span title={$trad('Conexiones')}>⛓ {selectedNode.degree}</span>
                    {#if selectedNode.community >= 0}
                        <span class="mg-comm-chip"
                              style="background:{communityColor(selectedNode.community)}33; color:{communityColor(selectedNode.community)};"
                              title={$trad('Cluster')}>
                            ◯ cluster {selectedNode.community}
                        </span>
                    {/if}
                </div>
                {#if selectedNode.tags.length}
                    <div class="mg-detail-tags">
                        {#each selectedNode.tags as tag}<span class="mg-tag">{tag}</span>{/each}
                    </div>
                {/if}
                <div class="mg-detail-preview">{selectedNode.preview}</div>
                <!-- V12 — Open in Memorias tab -->
                <button class="mg-open-btn" on:click={() => selectedNode && openInBrowser(selectedNode)}>
                    → {$trad('Abrir en Memorias')}
                </button>
            </aside>
        {/if}

        <!-- ── Legend ──────────────────────────────────────────────────── -->
        {#if graph}
        <div class="mg-legend">
            <span><span class="lg-swatch lg-tag"></span> {$trad('tag')} ({graph.stats.edges_tag})</span>
            <span><span class="lg-swatch lg-content"></span> {$trad('contenido')} ({graph.stats.edges_content})</span>
            <span><span class="lg-swatch lg-emb"></span> {$trad('embedding')} ({graph.stats.edges_embedding})</span>
            <span><span class="lg-dot lg-pinned"></span> {$trad('fijado')}</span>
            <span class="mg-density">
                {$trad('densidad')}: {(graph.stats.density * 100).toFixed(2)}%
            </span>
        </div>
        {/if}
    </div>
</div>

<style>
    /* V1 — Opaque background (was rgba(8,10,18,0.97) — bled through) */
    .mg-overlay {
        position: fixed; inset: 0;
        /* P1 — depth vignette: a faint elevation glow at the cluster's centre
           fading to the palette base (#0d1117) at the edges, so the graph sits
           in space instead of on a flat slab. */
        background:
            radial-gradient(120% 90% at 50% 34%, #16203a 0%, #0d1117 58%, #090b11 100%);
        backdrop-filter: blur(10px) saturate(140%);
        -webkit-backdrop-filter: blur(10px) saturate(140%);
        display: flex; flex-direction: column;
        z-index: 9000;
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace);
        color: var(--text-main, #cbd5e1);
    }
    .mg-header {
        display: flex; align-items: center; justify-content: space-between;
        padding: 10px 16px;
        background: rgba(15, 19, 31, 0.98);
        border-bottom: 1px solid rgba(255,255,255,0.06);
        gap: 10px; flex-wrap: wrap;
    }
    .mg-title { display: flex; align-items: center; gap: 10px; font-size: 13px; font-weight: 600; letter-spacing: 0.5px; }
    .mg-glyph { color: var(--accent, #10b981); font-size: 16px; }
    .mg-count { font-size: 10px; color: var(--text-muted, #94a3b8); font-weight: 400; letter-spacing: 0.3px; }
    .mg-actions { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }

    /* V5 — Search input */
    .mg-search {
        background: rgba(255,255,255,0.04);
        border: 1px solid rgba(255,255,255,0.10);
        color: var(--text-main, #cbd5e1);
        font: inherit; font-size: 11px;
        padding: 4px 10px; border-radius: 5px;
        width: 260px; outline: none;
        transition: border-color .12s;
    }
    .mg-search:focus { border-color: var(--accent, #10b981); }
    .mg-search::placeholder { color: var(--text-muted, #94a3b8); }

    .mg-btn {
        background: rgba(255,255,255,0.04); border: 1px solid rgba(255,255,255,0.06);
        color: var(--text-muted, #94a3b8); font: inherit; font-size: 11px;
        padding: 4px 10px; border-radius: 5px; cursor: pointer;
        transition: background .12s, color .12s, border-color .12s;
    }
    .mg-btn:hover:not(:disabled) { background: rgba(255,255,255,0.07); color: var(--text-main, #cbd5e1); }
    .mg-btn:disabled { opacity: 0.45; cursor: default; }
    .mg-btn.mg-close:hover { background: rgba(239,68,68,0.15); color: #ef4444; border-color: rgba(239,68,68,0.3); }
    .mg-filter {
        display: inline-flex; align-items: center; gap: 6px; font-size: 10px;
        color: var(--text-muted); background: rgba(255,255,255,0.03);
        padding: 3px 8px; border-radius: 5px;
    }
    .mg-filter input[type="range"] { width: 80px; }
    .mg-filter-val { color: var(--accent, #10b981); font-weight: 600; min-width: 12px; }

    /* Filter row (tags + threshold sliders) */
    .mg-filter-row {
        display: flex; align-items: center; gap: 12px;
        padding: 6px 16px;
        background: rgba(15, 19, 31, 0.94);
        border-bottom: 1px solid rgba(255,255,255,0.04);
        flex-wrap: wrap;
    }
    .mg-tag-pills { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; flex: 1; }
    .mg-tag-label { font-size: 10px; color: var(--text-muted); letter-spacing: 0.3px; }
    .mg-tag-pill {
        background: rgba(244,114,182,0.08); border: 1px solid rgba(244,114,182,0.18);
        color: #f472b6; font: inherit; font-size: 10px;
        padding: 2px 8px; border-radius: 10px; cursor: pointer;
        transition: background .12s, border-color .12s;
    }
    .mg-tag-pill:hover { background: rgba(244,114,182,0.18); }
    .mg-tag-pill.active {
        background: rgba(244,114,182,0.30); border-color: rgba(244,114,182,0.55);
        color: #fff;
    }
    .mg-tag-count { color: #f472b6; opacity: 0.7; margin-left: 3px; }
    .mg-tag-pill.active .mg-tag-count { color: #fff; }
    .mg-tag-clear {
        background: rgba(239,68,68,0.10); border: 1px solid rgba(239,68,68,0.20);
        color: #ef4444; font: inherit; font-size: 9px;
        padding: 2px 6px; border-radius: 8px; cursor: pointer;
    }

    /* Threshold dropdown — anchored to the right edge of its parent so it
       never bleeds past the viewport (bug reported when the page was
       narrow + the details menu opened naturally at the natural position). */
    .mg-thresholds {
        font-size: 10px; color: var(--text-muted);
        position: relative;
    }
    .mg-thresholds summary {
        cursor: pointer; padding: 3px 8px;
        background: rgba(255,255,255,0.04); border-radius: 5px;
        user-select: none;
        list-style: none;          /* hide native disclosure triangle */
    }
    .mg-thresholds summary::-webkit-details-marker { display: none; }
    .mg-thresholds summary:hover { background: rgba(255,255,255,0.06); }
    .mg-thresholds[open] summary { background: rgba(16,185,129,0.10); color: var(--accent); }
    .mg-thr-grid {
        /* Single column: each slider gets its own row → labels never collide
           with values from the next column. The 2-col grid was visually
           cramped and confusing — clearer to stack. */
        display: flex; flex-direction: column;
        gap: 10px; padding: 12px 14px;
        background: rgba(20, 24, 36, 0.98);
        border: 1px solid rgba(255,255,255,0.12);
        border-radius: 8px;
        box-shadow: 0 8px 32px -8px rgba(0,0,0,0.55);
        /* Anchor to the right edge of the parent so it grows leftward and
           never crosses the viewport boundary. top:100%+6px places it just
           below the summary chip. */
        position: absolute;
        top: calc(100% + 6px);
        right: 0;
        z-index: 50;
        width: 280px;
        max-width: calc(100vw - 32px);
    }
    .mg-thr-grid label {
        display: grid;
        grid-template-columns: 80px 1fr 38px;  /* label · slider · value */
        align-items: center; gap: 10px;
        font-size: 10px; white-space: nowrap;
    }
    .mg-thr-grid input[type="range"] { width: 100%; min-width: 0; }
    .mg-thr-grid .mg-thr-toggle {
        grid-template-columns: 18px 1fr; /* checkbox · label */
        padding-top: 6px;
        border-top: 1px solid rgba(255,255,255,0.06);
    }
    .mg-thr-lbl {
        display: inline-block; min-width: 60px; padding: 1px 6px; border-radius: 4px;
        font-weight: 600; text-align: center;
    }
    .mg-thr-lbl.pink   { background: rgba(244,114,182,0.18); color: #f472b6; }
    .mg-thr-lbl.blue   { background: rgba(96,165,250,0.18); color: #60a5fa; }
    .mg-thr-lbl.purple { background: rgba(168,85,247,0.18); color: #a855f7; }
    .mg-thr-val { font-variant-numeric: tabular-nums; color: var(--accent); min-width: 32px; text-align: right; }
    .mg-thr-toggle { grid-column: 1 / -1; gap: 8px; }

    .mg-canvas-wrap { flex: 1; position: relative; overflow: hidden; }
    /* v1.7.86 — Canvas2D edge underlay. Absolutely positioned to occupy
       the same space as the SVG above it; pointer-events:none so the
       SVG stays the event-capture layer (nodes need pointerdown for
       drag, the empty SVG area handles pan). image-rendering hints
       the browser to use bilinear sampling on HiDPI scaling. */
    .mg-edge-canvas {
        position: absolute;
        inset: 0;
        pointer-events: none;
        image-rendering: auto;
    }
    #mg-canvas {
        position: absolute;
        inset: 0;
        background: transparent;
    }
    svg { display: block; cursor: grab; outline: none; }
    svg.dragging { cursor: grabbing; }
    /* The graph SVG is keyboard-focusable (role=application, tabindex=0), so the
       app-wide :focus-visible ring (app.css) traced the whole canvas and read as
       a green "cage". Suppress it just here — the higher specificity (#id) wins.
       Keyboard/ESC still work; only the heavy outline is gone. */
    #mg-canvas:focus, #mg-canvas:focus-visible { outline: none; box-shadow: none; }
    .mg-node { cursor: pointer; transition: opacity .15s; }
    /* Scoped to the core disc so the hover highlight doesn't repaint the
       ripple / halo / pin rings white. */
    .mg-node:hover .mg-node-core { stroke: #fff; stroke-width: 1.6; }

    /* P2 — hover ripple: an expanding ring that fades, in the node's colour.
       transform-box:fill-box anchors the scale to each circle's own centre. */
    .mg-ripple { transform-box: fill-box; transform-origin: center; opacity: 0; }
    .mg-node:hover .mg-ripple { animation: mg-ripple .95s ease-out infinite; }
    @keyframes mg-ripple {
        0%   { transform: scale(1);   opacity: .5; }
        100% { transform: scale(2.5); opacity: 0; }
    }
    /* P2 — selection halo: a gentle pulse around the open node. */
    .mg-sel-halo { transform-box: fill-box; transform-origin: center; animation: mg-sel-pulse 1.8s ease-in-out infinite; }
    @keyframes mg-sel-pulse {
        0%, 100% { transform: scale(1);    opacity: .45; }
        50%      { transform: scale(1.16); opacity: .85; }
    }
    /* P2 — pinned ring: a slow amber breath (the canvas glow is already amber). */
    .mg-pin-ring { animation: mg-pin-breath 2.6s ease-in-out infinite; }
    @keyframes mg-pin-breath { 0%, 100% { opacity: .4; } 50% { opacity: .85; } }
    /* Font-size is set inline per-node so the value reflects current zoom
       (counter-scaled to remain ~11px on screen regardless of zoom level). */
    .mg-node-label {
        font-family: var(--font-mono);
        fill: rgba(220,228,240,0.92);
        user-select: none;
        pointer-events: none;
        /* Tiny dark stroke makes labels readable over edges + lighter nodes */
        paint-order: stroke fill;
        stroke: rgba(10,13,24,0.85);
        stroke-width: 2.5;
    }
    .mg-node.pinned .mg-node-label { fill: #f59e0b; }

    .mg-empty {
        position: absolute; inset: 0; display: flex; flex-direction: column;
        align-items: center; justify-content: center;
        font-size: 12px; color: var(--text-muted); padding: 20px;
        text-align: center;
    }
    .mg-empty.mg-hint { max-width: 420px; margin: auto; line-height: 1.6; }
    .mg-err { color: #ef4444; }

    .mg-detail {
        position: absolute; top: 12px; right: 12px;
        width: 340px; max-height: calc(100vh - 140px);
        overflow-y: auto;
        background: rgba(20, 24, 36, 0.98);
        border: 1px solid rgba(255,255,255,0.10);
        border-radius: 8px; padding: 12px 14px;
        font-size: 11px; line-height: 1.5;
        box-shadow: 0 8px 32px -8px rgba(0,0,0,0.6);
    }
    .mg-detail-hdr { display: flex; justify-content: space-between; align-items: flex-start; gap: 8px; margin-bottom: 8px; }
    .mg-detail-title { font-size: 12px; font-weight: 600; color: var(--accent); }
    .mg-detail-meta { display: flex; gap: 12px; font-size: 10px; color: var(--text-muted); margin-bottom: 8px; align-items: center; flex-wrap: wrap; }
    .mg-comm-chip { font-size: 9px; padding: 1px 6px; border-radius: 8px; font-weight: 600; }
    .mg-detail-tags { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 8px; }
    .mg-tag {
        font-size: 9px; padding: 1px 6px; border-radius: 8px;
        background: rgba(244,114,182,0.12); color: #f472b6;
    }
    .mg-detail-preview {
        font-family: var(--font-mono); font-size: 11px;
        color: var(--text-main); white-space: pre-wrap; word-break: break-word;
        max-height: 240px; overflow-y: auto;
    }
    /* V12 — Open in Memorias button */
    .mg-open-btn {
        margin-top: 10px; width: 100%;
        background: rgba(16,185,129,0.15);
        border: 1px solid rgba(16,185,129,0.30);
        color: var(--accent, #10b981); font: inherit; font-size: 11px;
        padding: 5px 10px; border-radius: 5px; cursor: pointer;
        transition: background .12s;
    }
    .mg-open-btn:hover { background: rgba(16,185,129,0.25); }

    .mg-legend {
        position: absolute; bottom: 12px; left: 12px;
        display: flex; gap: 14px; font-size: 10px; color: var(--text-muted);
        background: rgba(20, 24, 36, 0.92);
        padding: 6px 12px; border-radius: 6px;
        border: 1px solid rgba(255,255,255,0.06);
        align-items: center; flex-wrap: wrap;
        max-width: calc(100vw - 380px);
    }
    .lg-swatch { display: inline-block; width: 14px; height: 2px; vertical-align: middle; margin-right: 4px; }
    .lg-tag    { background: rgba(244,114,182,0.6); }
    .lg-content{ background: rgba(96,165,250,0.6); }
    .lg-emb    { background: rgba(168,85,247,0.6); }
    .lg-dot    { display: inline-block; width: 8px; height: 8px; border-radius: 50%; vertical-align: middle; margin-right: 4px; }
    .lg-pinned { background: #f59e0b; }
    .mg-density { color: var(--accent, #10b981); font-weight: 600; }
</style>
