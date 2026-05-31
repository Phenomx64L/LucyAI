<!--
  MemoryBrowserView — visual surface for Lucy's memory layers.

  Replaces the chat-only slash-command UX (/crystallize, /insights, etc.)
  with a dedicated panel users can browse, filter, and inspect at leisure.
  Four tabs:
    • Memorias  — agent_memories table, searchable + tag/importance filters
    • Crystals  — agent_crystals, click-through to outcomes/files/lessons
    • Insights  — agent_insights ranked by confidence with reinforce bar
    • Grafo     — pick a memory + hop count → BFS via graph_neighbors

  All backend calls are existing Tauri commands. No new backend work; this
  is purely a richer client over the same APIs.
-->
<script lang="ts">
    import { onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import Brain         from '@tabler/icons-svelte/icons/brain';
    import Diamond       from '@tabler/icons-svelte/icons/diamond';
    import Sparkles      from '@tabler/icons-svelte/icons/sparkles';
    import Share3        from '@tabler/icons-svelte/icons/share-3';
    import Search        from '@tabler/icons-svelte/icons/search';
    import Trash         from '@tabler/icons-svelte/icons/trash';
    import RefreshCw     from '@tabler/icons-svelte/icons/refresh';
    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
    import BookOpen      from '@tabler/icons-svelte/icons/book-2';
    import Shield        from '@tabler/icons-svelte/icons/shield';
    import Radar         from '@tabler/icons-svelte/icons/radar-2';
    import GitCompare    from '@tabler/icons-svelte/icons/git-compare';

    import { getLessons, promoteLesson, dismissLesson, getLessonProjects } from '$lib/agentmemory/lessons';
    import type { Lesson } from '$lib/agentmemory/lessons';
    import { loadSentinels, addSentinel, deleteSentinel, toggleSentinel, BUILTIN_TEMPLATES, opOptions } from '$lib/agentmemory/sentinels';
    import type { SentinelRule } from '$lib/agentmemory/sentinels';
    import { detectPatterns, patternKindLabel, patternKindIcon, confidenceColor } from '$lib/agentmemory/patterns';
    // v1.4.15 — shimmer placeholders + structured empty states.
    import Skeleton from '$lib/Skeleton.svelte';
    import EmptyState from '$lib/EmptyState.svelte';
    // v1.6.0 — Kappa Graph ADR-044 grounding score per memory.
    import GroundingChip from '$lib/GroundingChip.svelte';
    // v1.6.9 — Kappa Graph ADR-200 cluster verdict per memory's tags.
    // Each memory carries its worst-verdict tag as a small chip so users
    // see at a glance which entries belong to demote/watch-band clusters
    // identified by /anneal.
    import { invoke as tauriInvoke } from '@tauri-apps/api/core';
    let annealVerdictByTag: Map<string, string> = new Map();
    async function refreshAnnealingVerdicts() {
        try {
            const rep = await tauriInvoke<any>('memory_annealing_report');
            const m = new Map<string, string>();
            for (const c of (rep?.clusters || [])) m.set(c.name, c.verdict);
            annealVerdictByTag = m;
        } catch { /* annealing is opt-in; failure means no chips */ }
    }
    // Verdict ordering: demote (worst) > watch > promote > no_action.
    const VERDICT_RANK: Record<string, number> = { demote: 3, watch: 2, promote: 1, no_action: 0 };
    function worstClusterVerdict(tagsJson: string): { verdict: string; tag: string } | null {
        try {
            const tags = JSON.parse(tagsJson) as string[];
            let best: { verdict: string; tag: string } | null = null;
            for (const t of tags) {
                const v = annealVerdictByTag.get(t);
                if (!v || v === 'no_action') continue;
                if (!best || (VERDICT_RANK[v] || 0) > (VERDICT_RANK[best.verdict] || 0)) {
                    best = { verdict: v, tag: t };
                }
            }
            return best;
        } catch { return null; }
    }
    import type { DetectedPattern, PatternReport } from '$lib/agentmemory/patterns';
    import { verifyMemories, resolveContradiction, severityLabel, severityColor, resolutionLabel } from '$lib/agentmemory/verify';
    import type { Contradiction, VerifyReport, Resolution } from '$lib/agentmemory/verify';
    import MemoryGraphView from '$lib/MemoryGraphView.svelte';

    export let isEN: boolean = false;

    // ── Tab state ────────────────────────────────────────────────────────
    type Tab = 'memorias' | 'crystals' | 'insights' | 'grafo' | 'lessons' | 'sentinels' | 'patterns' | 'verify';
    let activeTab: Tab = 'memorias';

    // ── Memorias ──
    interface Memory {
        id: number; session_id: string; title: string; content: string;
        tags: string; files: string; importance: number; created_at: number;
    }
    let memorias: Memory[] = [];
    let memQuery = '';
    let memImportance = 0; // 0 = todas, 1/2/3
    let memLoading = false;
    let memError: string | null = null;
    let memSearchTimer: any = null;
    /** V12 — When the user jumps from the graph via "Abrir en Memorias",
     *  we highlight + scroll to this memory id instead of filtering. FTS5
     *  doesn't index the numeric id, so a filter-by-id query returns empty
     *  (the bug the user reported). Highlight-and-scroll is also better UX. */
    let highlightedMemId: number | null = null;
    let _highlightTimer: ReturnType<typeof setTimeout> | null = null;

    // ── Sprint D — Memory health timeline ──────────────────────────────
    // Renders a tiny histogram of memory creation events across the last
    // 30 days, plus rolling counts. Drives the "Memory health" mini-chart
    // at the top of the Memorias tab.
    interface TimelineBucket {
        day_iso: string;
        date: Date;
        created: number;
    }
    $: memTimeline = (() => {
        if (!memorias.length) return [];
        const now = Date.now();
        const cutoff = now - 30 * 86400_000;
        const buckets: Map<string, TimelineBucket> = new Map();
        // Seed 30 day buckets so empty days render too.
        for (let i = 29; i >= 0; i--) {
            const d = new Date(now - i * 86400_000);
            const iso = d.toISOString().slice(0, 10);
            buckets.set(iso, { day_iso: iso, date: d, created: 0 });
        }
        for (const m of memorias) {
            const ts = (m.created_at || 0) * 1000;
            if (ts < cutoff) continue;
            const iso = new Date(ts).toISOString().slice(0, 10);
            const b = buckets.get(iso);
            if (b) b.created++;
        }
        return Array.from(buckets.values());
    })();
    $: memTimelineMax = Math.max(1, ...memTimeline.map(b => b.created));

    /** Jump to a specific memory id: clear filters, ensure it's loaded,
     *  scroll it into view, flash a green border. */
    async function jumpToMemory(id: number) {
        // Clear filters so the memory is guaranteed to be in the visible list.
        memQuery = '';
        memImportance = 0;
        await loadMemorias();
        highlightedMemId = id;
        // Wait for Svelte to render the list, then scroll the row.
        setTimeout(() => {
            const el = document.querySelector(
                `[data-mem-id="${id}"]`
            ) as HTMLElement | null;
            if (el) {
                el.scrollIntoView({ behavior: 'smooth', block: 'center' });
            }
        }, 80);
        // Auto-clear the highlight after 4s so the visual cue doesn't linger.
        if (_highlightTimer) clearTimeout(_highlightTimer);
        _highlightTimer = setTimeout(() => { highlightedMemId = null; }, 4000);
    }

    async function loadMemorias() {
        memLoading = true;
        memError = null;
        try {
            if (memQuery.trim().length === 0) {
                memorias = await invoke<Memory[]>('get_recent_memories', { limit: 50 });
            } else {
                memorias = await invoke<Memory[]>('search_agent_memories', { query: memQuery, limit: 50 });
            }
            if (memImportance > 0) {
                memorias = memorias.filter(m => m.importance === memImportance);
            }
        } catch (e) {
            memError = String(e);
            memorias = [];
        } finally {
            memLoading = false;
        }
    }

    function onMemQueryInput() {
        if (memSearchTimer) clearTimeout(memSearchTimer);
        memSearchTimer = setTimeout(loadMemorias, 300);
    }

    async function deleteMemoria(id: number) {
        if (!confirm(isEN ? `Delete memory #${id}?` : `¿Borrar memoria #${id}?`)) return;
        try {
            await invoke('delete_agent_memory', { id });
            await loadMemorias();
        } catch (e) {
            memError = String(e);
        }
    }

    // ── M1 — Stats dashboard ─────────────────────────────────────────────
    // Derived from the current `memorias` list. Cheap O(n) — no DB hit.
    $: memStats = (() => {
        const now = Math.floor(Date.now() / 1000);
        const total = memorias.length;
        let pinned = 0, untagged = 0, expiringSoon = 0, recent7d = 0;
        let highestImp = 0;
        for (const m of memorias) {
            if (m.importance === 10) pinned++;
            if (!tagList(m.tags).length) untagged++;
            // expires_at: 0 = never. >0 = unix sec; "soon" = within 7d.
            const exp = ((m as unknown) as { expires_at?: number }).expires_at || 0;
            if (exp > 0 && exp < now + 7 * 86400) expiringSoon++;
            if (m.created_at >= now - 7 * 86400) recent7d++;
            if (m.importance > highestImp) highestImp = m.importance;
        }
        return { total, pinned, untagged, expiringSoon, recent7d, highestImp };
    })();

    // ── M2 — Bulk operations ─────────────────────────────────────────────
    // A Set of selected memory ids. Empty = bulk bar hidden. The bar
    // appears above the list with "Tag N", "Promote N", "Delete N" buttons.
    let bulkSelected: Set<number> = new Set();
    let bulkBusy = false;

    function toggleBulkSelect(id: number, ev?: Event) {
        if (ev) ev.stopPropagation();
        const next = new Set(bulkSelected);
        if (next.has(id)) next.delete(id); else next.add(id);
        bulkSelected = next;
    }
    function bulkSelectAll() {
        bulkSelected = new Set(memorias.map(m => m.id));
    }
    function bulkClearSelection() {
        bulkSelected = new Set();
    }
    async function bulkDelete() {
        if (bulkSelected.size === 0) return;
        if (!confirm(isEN
            ? `Delete ${bulkSelected.size} selected memories? Cannot be undone.`
            : `¿Borrar ${bulkSelected.size} memorias seleccionadas? Irreversible.`)) return;
        bulkBusy = true;
        try {
            // Run sequentially — DB pool is single-conn aware and the UI
            // shows progress better. For 100 memories this is ~2-3 seconds.
            for (const id of bulkSelected) {
                try { await invoke('delete_agent_memory', { id }); }
                catch { /* keep going; report at the end */ }
            }
            bulkSelected = new Set();
            await loadMemorias();
        } finally {
            bulkBusy = false;
        }
    }
    async function bulkPromote() {
        // Bumps importance +1 on each selected memory (capped at 10).
        // Uses save_agent_memory_full which is the "update or insert" path.
        if (bulkSelected.size === 0) return;
        bulkBusy = true;
        try {
            for (const id of bulkSelected) {
                const m = memorias.find(x => x.id === id);
                if (!m) continue;
                const newImp = Math.min(10, (m.importance || 1) + 1);
                if (newImp === m.importance) continue;
                try {
                    await invoke('save_agent_memory_full', {
                        id, title: m.title, content: m.content,
                        tags: tagList(m.tags), files: tagList(m.files),
                        importance: newImp, sessionId: m.session_id || '',
                    });
                } catch { /* ignore per-row failures */ }
            }
            bulkSelected = new Set();
            await loadMemorias();
        } finally {
            bulkBusy = false;
        }
    }
    async function bulkAddTag() {
        if (bulkSelected.size === 0) return;
        const t = prompt(isEN
            ? `Add tag to ${bulkSelected.size} memories:`
            : `Añadir tag a ${bulkSelected.size} memorias:`);
        const tagClean = (t || '').trim().toLowerCase();
        if (!tagClean) return;
        bulkBusy = true;
        try {
            for (const id of bulkSelected) {
                const m = memorias.find(x => x.id === id);
                if (!m) continue;
                const existing = tagList(m.tags);
                if (existing.includes(tagClean)) continue;
                try {
                    await invoke('save_agent_memory_full', {
                        id, title: m.title, content: m.content,
                        tags: [...existing, tagClean],
                        files: tagList(m.files),
                        importance: m.importance,
                        sessionId: m.session_id || '',
                    });
                } catch { /* keep going */ }
            }
            bulkSelected = new Set();
            await loadMemorias();
        } finally {
            bulkBusy = false;
        }
    }

    // ── M3 — Auto-tag suggestion via LLM ─────────────────────────────────
    // For each selected memory without tags, ask Gemini Flash Lite to
    // suggest 3-5 short tags. The user reviews + accepts/rejects per memory.
    let autoTagBusy = false;
    let autoTagSuggestions: Record<number, string[]> = {};

    async function autoTagSelected() {
        if (bulkSelected.size === 0) return;
        autoTagBusy = true;
        autoTagSuggestions = {};
        try {
            for (const id of bulkSelected) {
                const m = memorias.find(x => x.id === id);
                if (!m) continue;
                // Skip memories that already have tags — auto-tag is for
                // the untagged ones unless the user really wants to overwrite.
                const existing = tagList(m.tags);
                const prompt =
                    `Suggest 3 to 5 short, lowercased, kebab-case tags for this memory. ` +
                    `Output ONLY a comma-separated list, no explanation, no markdown.\n\n` +
                    `Title: ${m.title}\n\nContent:\n${m.content.slice(0, 1500)}` +
                    (existing.length ? `\n\nExisting tags (avoid duplicating): ${existing.join(', ')}` : '');
                try {
                    // v1.6.10: was 'gemini-3.5-flash-lite' which is not a
                    // real Gemini model id, so every call failed silently
                    // and the catch landed in "no usable tags". Switching
                    // to the currently-active model (preview) so the user
                    // sees whatever Lucy is actually using elsewhere.
                    const result = await invoke<string>('ask_lucy', {
                        prompt, context: '',
                        userName: '', runbooksDir: null,
                        model: 'gemini-3-flash-preview',
                        lang: 'es-MX', hostsJson: null, images: null,
                    });
                    // Gemini often prefixes a preamble. Scan ALL lines and
                    // pick the first comma-separated one, instead of
                    // assuming line 1 is the answer.
                    const raw = String(result || '');
                    const candidateLine =
                        raw.split('\n').map(l => l.trim()).find(l => l.includes(',')) ||
                        raw.split('\n')[0] || '';
                    const cleaned = candidateLine
                        .split(',')
                        .map(s => s.trim().toLowerCase().replace(/[^a-z0-9-]/g, '-').replace(/-+/g, '-').replace(/^-|-$/g, ''))
                        .filter(s => s.length >= 2 && s.length <= 30 && !existing.includes(s));
                    autoTagSuggestions = { ...autoTagSuggestions, [id]: cleaned.slice(0, 5) };
                } catch (e) {
                    // Surface the real error so the user can act on it
                    // instead of seeing the misleading "no usable tags".
                    console.warn(`autoTag #${id} failed:`, e);
                    autoTagSuggestions = { ...autoTagSuggestions, [id]: [] };
                }
            }
        } finally {
            autoTagBusy = false;
        }
    }

    /** Accept the suggested tags for a specific memory (M3 confirmation step). */
    async function acceptAutoTags(id: number) {
        const sugg = autoTagSuggestions[id] || [];
        if (sugg.length === 0) return;
        const m = memorias.find(x => x.id === id);
        if (!m) return;
        const merged = [...new Set([...tagList(m.tags), ...sugg])];
        try {
            // v1.6.11: was 'save_agent_memory_full' which was never
            // implemented in the backend — every Accept failed silently
            // until the user saw the error. Use the dedicated tag-update
            // command shipped in v1.6.11.
            await invoke('update_agent_memory_tags', { id, tags: merged });
            const next = { ...autoTagSuggestions };
            delete next[id];
            autoTagSuggestions = next;
            await loadMemorias();
        } catch (e) { memError = String(e); }
    }
    function rejectAutoTags(id: number) {
        const next = { ...autoTagSuggestions };
        delete next[id];
        autoTagSuggestions = next;
    }

    // ── Crystals ──
    interface Crystal {
        id: number; session_id: string; project: string;
        narrative: string; key_outcomes: string; files_affected: string;
        lessons: string; source_chars: number; created_at: number;
    }
    let crystals: Crystal[] = [];
    let crystalsLoading = false;
    let crystalsError: string | null = null;
    let expandedCrystal: number | null = null;

    async function loadCrystals() {
        crystalsLoading = true;
        crystalsError = null;
        try {
            crystals = await invoke<Crystal[]>('list_crystals', { sessionId: null, project: null, limit: 50 });
        } catch (e) {
            crystalsError = String(e);
            crystals = [];
        } finally {
            crystalsLoading = false;
        }
    }

    async function deleteCrystal(id: number) {
        if (!confirm(isEN ? `Delete crystal #${id}?` : `¿Borrar crystal #${id}?`)) return;
        try {
            await invoke('delete_crystal', { id });
            await loadCrystals();
        } catch (e) {
            crystalsError = String(e);
        }
    }

    // ── Insights ──
    interface Insight {
        id: number; content: string; fingerprint: string;
        confidence: number; reinforcements: number; concepts: string;
        source_count: number; last_reinforced_at: number;
        created_at: number; updated_at: number;
    }
    let insights: Insight[] = [];
    let insightsLoading = false;
    let insightsError: string | null = null;

    async function loadInsights() {
        insightsLoading = true;
        insightsError = null;
        try {
            insights = await invoke<Insight[]>('list_insights', { limit: 50 });
        } catch (e) {
            insightsError = String(e);
            insights = [];
        } finally {
            insightsLoading = false;
        }
    }

    async function deleteInsight(id: number) {
        if (!confirm(isEN ? `Delete insight #${id}?` : `¿Borrar insight #${id}?`)) return;
        try {
            await invoke('delete_insight', { id });
            await loadInsights();
        } catch (e) {
            insightsError = String(e);
        }
    }

    // ── Grafo ──
    interface GraphNeighbor {
        memory_id: number; hops: number; score: number; edge_types: string;
        memory: Memory;
    }
    let graphSeedId = '';
    let graphHops = 2;
    let graphResults: GraphNeighbor[] = [];
    let graphLoading = false;
    let graphError: string | null = null;
    /** Tier S #2 — switch between text BFS and full visual force-directed graph */
    let graphMode: 'table' | 'visual' = 'table';
    let showVisualGraph = false;

    async function runGraphQuery() {
        const id = parseInt(graphSeedId, 10);
        if (!Number.isFinite(id) || id <= 0) {
            graphError = isEN ? 'Enter a valid memory id.' : 'Ingresa un id de memoria válido.';
            return;
        }
        graphLoading = true;
        graphError = null;
        try {
            graphResults = await invoke<GraphNeighbor[]>('graph_neighbors', {
                seedId: id, maxHops: graphHops, limit: 30,
            });
            if (graphResults.length === 0) {
                graphError = isEN
                    ? 'No neighbors found. Rebuild the graph first if you just saved this memory.'
                    : 'Sin vecinos. Reconstruye el grafo si la memoria es reciente.';
            }
        } catch (e) {
            graphError = String(e);
            graphResults = [];
        } finally {
            graphLoading = false;
        }
    }

    async function rebuildGraph() {
        graphLoading = true;
        graphError = null;
        try {
            const r = await invoke<{ total_directed_edges: number; eligible_memories: number }>('graph_rebuild_edges_run');
            graphError = isEN
                ? `Graph rebuilt: ${r.eligible_memories} nodes, ${r.total_directed_edges} edges. Now query a memory id.`
                : `Grafo reconstruido: ${r.eligible_memories} nodos, ${r.total_directed_edges} aristas. Ahora consulta un id.`;
        } catch (e) {
            graphError = String(e);
        } finally {
            graphLoading = false;
        }
    }

    // ── Auto-forget / consolidate / reflect — admin actions ─────────────
    let adminBusy = false;
    let adminMsg = '';

    async function runAutoForgetPreview() {
        adminBusy = true; adminMsg = '';
        try {
            const r = await invoke<any>('auto_forget_run', { dryRun: true });
            adminMsg = isEN
                ? `Auto-forget DRY-RUN: would delete ${r.ttl_expired} TTL-expired + ${r.low_value} low-value = ${r.total_deleted} memories.`
                : `Auto-forget DRY-RUN: borraría ${r.ttl_expired} con TTL + ${r.low_value} de bajo valor = ${r.total_deleted} memorias.`;
        } catch (e) { adminMsg = `auto_forget_run failed: ${e}`; }
        finally { adminBusy = false; }
    }
    async function runAutoForget() {
        if (!confirm(isEN ? 'Run auto-forget for real?' : '¿Ejecutar auto-forget en serio?')) return;
        adminBusy = true; adminMsg = '';
        try {
            const r = await invoke<any>('auto_forget_run', { dryRun: false });
            adminMsg = isEN
                ? `Auto-forget complete: ${r.total_deleted} memories removed.`
                : `Auto-forget completado: ${r.total_deleted} memorias eliminadas.`;
            if (activeTab === 'memorias') await loadMemorias();
        } catch (e) { adminMsg = `auto_forget_run failed: ${e}`; }
        finally { adminBusy = false; }
    }
    async function runConsolidatePreview() {
        adminBusy = true; adminMsg = '';
        try {
            const r = await invoke<any>('auto_consolidate_run', { dryRun: true });
            adminMsg = isEN
                ? `Consolidate DRY-RUN: ${r.eligible_memories} eligible, ${r.clusters_processed} clusters would fuse.`
                : `Consolidate DRY-RUN: ${r.eligible_memories} elegibles, ${r.clusters_processed} clusters se fusionarían.`;
        } catch (e) { adminMsg = `auto_consolidate failed: ${e}`; }
        finally { adminBusy = false; }
    }
    async function runReflectPreview() {
        adminBusy = true; adminMsg = '';
        try {
            const r = await invoke<any>('reflect_run', { dryRun: true });
            adminMsg = isEN
                ? `Reflect DRY-RUN: ${r.eligible_memories} eligible, ${r.clusters_processed} clusters would produce insights.`
                : `Reflect DRY-RUN: ${r.eligible_memories} elegibles, ${r.clusters_processed} clusters generarían insights.`;
        } catch (e) { adminMsg = `reflect_run failed: ${e}`; }
        finally { adminBusy = false; }
    }

    // ── Lessons ──
    let lessons: Lesson[] = [];
    let lessonsLoading = false;
    let lessonsError: string | null = null;
    let lessonQuery = '';
    let lessonProjects: string[] = [];
    let lessonProjectFilter = '';

    async function loadLessons() {
        lessonsLoading = true; lessonsError = null;
        try {
            lessons = await getLessons({ query: lessonQuery || undefined, project: lessonProjectFilter || undefined, limit: 100 });
            if (lessonProjects.length === 0) lessonProjects = await getLessonProjects();
        } catch (e) { lessonsError = String(e); lessons = []; }
        finally { lessonsLoading = false; }
    }
    async function doPromoteLesson(l: Lesson) {
        try { await promoteLesson(l); await loadLessons(); } catch (e) { lessonsError = String(e); }
    }
    async function doDismissLesson(l: Lesson) {
        if (!confirm(isEN ? `Dismiss lesson?` : `¿Descartar lección?`)) return;
        try { await dismissLesson(l); await loadLessons(); } catch (e) { lessonsError = String(e); }
    }

    // ── Sentinels ──
    let sentinelRules: SentinelRule[] = [];
    let showSentinelAdd = false;
    let newSentinelName = '';
    let newSentinelMetric = 'cpu_percent';
    let newSentinelOp = 'gt';
    let newSentinelThreshold = 90;
    let newSentinelCooldown = 15;

    function refreshSentinels() { sentinelRules = loadSentinels(); }
    function doDeleteSentinel(id: string) {
        if (!confirm(isEN ? 'Delete this sentinel?' : '¿Borrar este sentinel?')) return;
        deleteSentinel(id); refreshSentinels();
    }
    function doToggleSentinel(id: string) { toggleSentinel(id); refreshSentinels(); }
    function doAddSentinel() {
        if (!newSentinelName.trim()) return;
        addSentinel({
            name: newSentinelName.trim(),
            enabled: true,
            metric: newSentinelMetric as any,
            op: newSentinelOp as any,
            threshold: newSentinelThreshold,
            cooldownMin: newSentinelCooldown,
            hostFilter: '',
        });
        showSentinelAdd = false;
        newSentinelName = '';
        refreshSentinels();
    }
    function addFromTemplate(tpl: typeof BUILTIN_TEMPLATES[0]) {
        addSentinel({
            name: isEN ? tpl.nameEN : tpl.name,
            enabled: true,
            metric: tpl.metric,
            op: tpl.op,
            threshold: tpl.threshold,
            cooldownMin: tpl.cooldownMin,
            hostFilter: '',
        });
        refreshSentinels();
    }

    // ── Patterns ──
    let patternReport: PatternReport | null = null;
    let patternsLoading = false;
    let patternsError: string | null = null;

    async function loadPatterns() {
        patternsLoading = true; patternsError = null;
        try { patternReport = await detectPatterns(); }
        catch (e) { patternsError = String(e); patternReport = null; }
        finally { patternsLoading = false; }
    }

    // ── Verify ──
    let verifyReport: VerifyReport | null = null;
    let verifyLoading = false;
    let verifyError: string | null = null;

    async function loadVerify() {
        verifyLoading = true; verifyError = null;
        try { verifyReport = await verifyMemories(); }
        catch (e) { verifyError = String(e); verifyReport = null; }
        finally { verifyLoading = false; }
    }
    async function doResolve(c: Contradiction, res: Resolution) {
        try {
            await resolveContradiction(c, res);
            await loadVerify();
        } catch (e) { verifyError = String(e); }
    }

    // ── Helpers ─────────────────────────────────────────────────────────
    function fmtDate(ts: number): string {
        return new Date(ts * 1000).toLocaleString();
    }
    function tagList(raw: string): string[] {
        try { return JSON.parse(raw || '[]'); } catch { return []; }
    }
    function previewText(s: string, n = 200): string {
        if (!s) return '';
        return s.length > n ? s.slice(0, n) + '…' : s;
    }
    function importanceColor(imp: number): string {
        if (imp >= 3) return '#f59e0b';
        if (imp >= 2) return '#3b82f6';
        return 'var(--txt2)';
    }
    function edgeTypeIcon(et: string): string {
        if (et.includes('shares_concept'))  return '◇';
        if (et.includes('shares_file'))     return '⊟';
        if (et.includes('same_session'))    return '⌖';
        return '·';
    }

    // ── Mount: load default tab ─────────────────────────────────────────
    onMount(async () => {
        await loadMemorias();
        refreshAnnealingVerdicts();   // fire-and-forget; chip absence is fine
    });

    // Switch tabs (load on first switch)
    let loadedTabs = new Set<Tab>(['memorias']);
    function switchTab(t: Tab) {
        activeTab = t;
        if (loadedTabs.has(t)) return;
        loadedTabs.add(t);
        if (t === 'crystals') loadCrystals();
        else if (t === 'insights') loadInsights();
        else if (t === 'lessons') loadLessons();
        else if (t === 'sentinels') refreshSentinels();
        else if (t === 'patterns') loadPatterns();
        else if (t === 'verify') loadVerify();
    }
</script>

<div class="memory-view">
    <header class="mv-header">
        <div class="mv-title">
            <Brain size={22} stroke={2}/>
            <h2>{isEN ? 'Memory Browser' : 'Explorador de Memoria'}</h2>
        </div>
        <nav class="mv-tabs">
            <button class:active={activeTab === 'memorias'} on:click={() => switchTab('memorias')}>
                <Brain size={14}/> {isEN ? 'Memories' : 'Memorias'}
            </button>
            <button class:active={activeTab === 'crystals'} on:click={() => switchTab('crystals')}>
                <Diamond size={14}/> Crystals
            </button>
            <button class:active={activeTab === 'insights'} on:click={() => switchTab('insights')}>
                <Sparkles size={14}/> Insights
            </button>
            <button class:active={activeTab === 'grafo'} on:click={() => switchTab('grafo')}>
                <Share3 size={14}/> {isEN ? 'Graph' : 'Grafo'}
            </button>
            <span class="mv-tab-sep">│</span>
            <button class:active={activeTab === 'lessons'} on:click={() => switchTab('lessons')}>
                <BookOpen size={14}/> {isEN ? 'Lessons' : 'Lecciones'}
            </button>
            <button class:active={activeTab === 'sentinels'} on:click={() => switchTab('sentinels')}>
                <Shield size={14}/> Sentinels
            </button>
            <button class:active={activeTab === 'patterns'} on:click={() => switchTab('patterns')}>
                <Radar size={14}/> {isEN ? 'Patterns' : 'Patrones'}
            </button>
            <button class:active={activeTab === 'verify'} on:click={() => switchTab('verify')}>
                <GitCompare size={14}/> {isEN ? 'Verify' : 'Verificar'}
            </button>
        </nav>
    </header>

    <!-- ══════════════════════════ MEMORIES ══════════════════════════ -->
    {#if activeTab === 'memorias'}
    <section class="mv-section">
        <!-- M1 — Stats dashboard. Read-only summary above the search row. -->
        {#if memorias.length > 0}
        <div class="mv-stats">
            <div class="mv-stat">
                <span class="mv-stat-num">{memStats.total}</span>
                <span class="mv-stat-lbl">{isEN ? 'memories' : 'memorias'}</span>
            </div>
            <div class="mv-stat" class:hot={memStats.pinned > 0}>
                <span class="mv-stat-num">{memStats.pinned}</span>
                <span class="mv-stat-lbl">📌 {isEN ? 'pinned' : 'fijadas'}</span>
            </div>
            <div class="mv-stat" class:hot={memStats.untagged > 0}>
                <span class="mv-stat-num">{memStats.untagged}</span>
                <span class="mv-stat-lbl">∅ {isEN ? 'untagged' : 'sin tags'}</span>
            </div>
            <div class="mv-stat" class:warn={memStats.expiringSoon > 0}>
                <span class="mv-stat-num">{memStats.expiringSoon}</span>
                <span class="mv-stat-lbl">⏱ {isEN ? 'expire <7d' : 'expiran <7d'}</span>
            </div>
            <div class="mv-stat">
                <span class="mv-stat-num">{memStats.recent7d}</span>
                <span class="mv-stat-lbl">⊕ {isEN ? 'new this week' : 'nuevas 7d'}</span>
            </div>
            <div class="mv-stat">
                <span class="mv-stat-num">{memStats.highestImp}</span>
                <span class="mv-stat-lbl">⮬ {isEN ? 'top importance' : 'top importancia'}</span>
            </div>
        </div>

        <!-- Sprint D — Memory health timeline (30-day creation histogram) -->
        {#if memTimeline.length}
        <div class="mv-timeline" title={isEN ? '30-day memory creation cadence' : 'Cadencia de creación de memorias (30 días)'}>
            <div class="mv-timeline-lbl">⏱ {isEN ? '30 days' : '30 días'}</div>
            <div class="mv-timeline-bars">
                {#each memTimeline as b}
                    <div class="mv-tl-bar"
                         style="height: {Math.max(2, (b.created / memTimelineMax) * 24)}px;
                                background: {b.created === 0
                                    ? 'rgba(148,163,184,.10)'
                                    : b.created >= memTimelineMax * 0.66
                                        ? '#10b981'
                                        : b.created >= memTimelineMax * 0.33
                                            ? '#56b4e9'
                                            : 'rgba(86,180,233,.55)'};"
                         title={`${b.day_iso}: ${b.created} ${isEN ? 'memories created' : 'memorias creadas'}`}></div>
                {/each}
            </div>
            <div class="mv-timeline-end">
                <span>{memTimeline[0]?.day_iso?.slice(5) || ''}</span>
                <span>{isEN ? 'today' : 'hoy'}</span>
            </div>
        </div>
        {/if}
        {/if}

        <div class="mv-toolbar">
            <div class="mv-search">
                <Search size={14}/>
                <input type="text" placeholder={isEN ? 'Search memories…' : 'Buscar memorias…'}
                    bind:value={memQuery} on:input={onMemQueryInput}/>
            </div>
            <select bind:value={memImportance} on:change={loadMemorias} class="mv-select">
                <option value={0}>{isEN ? 'All importance' : 'Toda importancia'}</option>
                <option value={1}>{isEN ? '◇ Normal (1)' : '◇ Normal (1)'}</option>
                <option value={2}>{isEN ? '◈ High (2)' : '◈ Alta (2)'}</option>
                <option value={3}>{isEN ? '◆ Critical (3)' : '◆ Crítica (3)'}</option>
            </select>
            <button class="mv-icon-btn" on:click={loadMemorias} title={isEN ? 'Refresh' : 'Recargar'}>
                <RefreshCw size={14}/>
            </button>
        </div>

        <!-- M2 — Bulk operations bar. Only visible when ≥1 memory selected. -->
        {#if bulkSelected.size > 0}
        <div class="mv-bulk-bar">
            <span class="mv-bulk-count">
                {bulkSelected.size} {isEN ? 'selected' : 'seleccionadas'}
            </span>
            <button class="mv-bulk-btn" on:click={bulkClearSelection}>
                {isEN ? 'Clear' : 'Limpiar'}
            </button>
            <button class="mv-bulk-btn" on:click={bulkSelectAll}>
                {isEN ? `Select all (${memorias.length})` : `Seleccionar todas (${memorias.length})`}
            </button>
            <button class="mv-bulk-btn" disabled={bulkBusy} on:click={bulkAddTag}>
                ⌖ {isEN ? 'Add tag' : 'Añadir tag'}
            </button>
            <button class="mv-bulk-btn" disabled={bulkBusy} on:click={bulkPromote}>
                ⮬ {isEN ? 'Promote +1' : 'Subir +1'}
            </button>
            <button class="mv-bulk-btn" disabled={autoTagBusy || bulkBusy} on:click={autoTagSelected}
                    title={isEN ? 'Use LLM to suggest tags for each selected memory' : 'Usa LLM para sugerir tags por memoria'}>
                {autoTagBusy ? '⟳ ' : '✦ '}{isEN ? 'AI suggest tags' : 'Sugerir tags (IA)'}
            </button>
            <button class="mv-bulk-btn warn" disabled={bulkBusy} on:click={bulkDelete}>
                ✕ {isEN ? 'Delete' : 'Borrar'}
            </button>
        </div>
        {/if}

        <!-- Admin actions strip -->
        <div class="mv-admin">
            <button class="mv-admin-btn" disabled={adminBusy} on:click={runAutoForgetPreview}>{isEN ? 'Forget preview' : 'Olvido (preview)'}</button>
            <button class="mv-admin-btn warn" disabled={adminBusy} on:click={runAutoForget}>{isEN ? 'Forget now' : 'Olvidar ya'}</button>
            <button class="mv-admin-btn" disabled={adminBusy} on:click={runConsolidatePreview}>{isEN ? 'Consolidate preview' : 'Consolidar (preview)'}</button>
            <button class="mv-admin-btn" disabled={adminBusy} on:click={runReflectPreview}>{isEN ? 'Reflect preview' : 'Reflexión (preview)'}</button>
            {#if adminMsg}<span class="mv-admin-msg">{adminMsg}</span>{/if}
        </div>

        {#if memError}
            <div class="mv-error"><AlertTriangle size={14}/> {memError}</div>
        {/if}
        {#if memLoading}
            <!-- v1.4.15 — skeleton rows replace the bare "Loading…" label. -->
            <div class="mv-loading">
                <Skeleton variant="card" height="58px" />
                <Skeleton variant="card" height="58px" />
                <Skeleton variant="card" height="58px" />
                <Skeleton variant="card" height="58px" />
            </div>
        {:else if memorias.length === 0}
            <!-- v1.4.15 — proper empty state with hint instead of one-line text. -->
            <EmptyState
                icon="🧠"
                title={isEN ? 'No memories yet' : 'Sin memorias aún'}
                description={isEN
                    ? 'Lucy populates this as she works. Pin a chat message (·) or run /crystallize after a useful session to seed it manually.'
                    : 'Lucy las irá creando conforme trabajes. Fija un mensaje (·) o usa /crystallize después de una sesión útil para sembrarlas manualmente.'} />
        {:else}
            <ul class="mv-list">
                {#each memorias as m (m.id)}
                    <li class="mv-card"
                        class:mv-card-selected={bulkSelected.has(m.id)}
                        class:mv-card-highlight={highlightedMemId === m.id}
                        data-mem-id={m.id}>
                        <div class="mv-card-head">
                            <!-- M2 — Bulk select checkbox -->
                            <input type="checkbox" class="mv-bulk-cb"
                                   checked={bulkSelected.has(m.id)}
                                   on:change={(e) => toggleBulkSelect(m.id, e)}
                                   on:click|stopPropagation
                                   title={isEN ? 'Select for bulk action' : 'Seleccionar para acción en lote'}/>
                            <span class="mv-id">#{m.id}</span>
                            <span class="mv-card-title">{m.title}</span>
                            <span class="mv-imp" style="color:{importanceColor(m.importance)};" title="importance">
                                {'◆'.repeat(m.importance)}{'·'.repeat(3 - m.importance)}
                            </span>
                            <!-- v1.6.0 — live grounding score (ADR-044).
                                 Computed query-time by the Rust backend;
                                 never cached. Click → opens the instances
                                 popover (TODO: v1.6.0.1). -->
                            <GroundingChip
                                memoryKind="agent"
                                memoryId={String(m.id)}
                                {isEN}
                                compact={true} />
                            <!-- v1.6.9 — Annealing cluster verdict chip.
                                 Shows up only when this memory belongs to a
                                 tag flagged demote/watch/promote by /anneal.
                                 Color-codes the worst verdict among its tags.
                                 We use a single-iteration {#each} so we can
                                 capture the helper result in a local without
                                 leaning on {@const} (which needs a parent
                                 block in Svelte 5). -->
                            {#each (worstClusterVerdict(m.tags) ? [worstClusterVerdict(m.tags)] : []) as _v (m.id)}
                                {#if _v}
                                    <span class="mv-anneal mv-anneal-{_v.verdict}"
                                          title={isEN
                                              ? `Annealing verdict: ${_v.verdict} (tag '${_v.tag}')`
                                              : `Veredicto de annealing: ${_v.verdict} (etiqueta '${_v.tag}')`}>
                                        {_v.verdict === 'demote' ? '↧' : _v.verdict === 'promote' ? '↥' : '◇'}
                                        {_v.verdict === 'demote' ? (isEN ? 'demote' : 'democ') : _v.verdict === 'promote' ? (isEN ? 'promote' : 'promov') : (isEN ? 'watch' : 'vigilar')}
                                    </span>
                                {/if}
                            {/each}
                            <button class="mv-del" title={isEN ? 'Delete' : 'Borrar'}
                                on:click={() => deleteMemoria(m.id)}><Trash size={13}/></button>
                        </div>
                        <p class="mv-card-content">{previewText(m.content, 300)}</p>
                        <div class="mv-card-foot">
                            <span class="mv-date">{fmtDate(m.created_at)}</span>
                            {#each tagList(m.tags) as t}
                                <span class="mv-tag">{t}</span>
                            {/each}
                        </div>
                        <!-- M3 — Suggested tags panel (visible after Auto-tag run) -->
                        {#if autoTagSuggestions[m.id]}
                            <div class="mv-autotag-panel">
                                {#if autoTagSuggestions[m.id].length === 0}
                                    <span class="mv-autotag-empty">
                                        ⟳ {isEN ? 'LLM returned no usable tags' : 'LLM no devolvió tags utilizables'}
                                    </span>
                                    <button class="mv-bulk-btn" on:click={() => rejectAutoTags(m.id)}>{isEN ? 'Dismiss' : 'Descartar'}</button>
                                {:else}
                                    <span class="mv-autotag-label">✦ {isEN ? 'AI suggests:' : 'IA sugiere:'}</span>
                                    {#each autoTagSuggestions[m.id] as st}
                                        <span class="mv-tag mv-tag-sugg">{st}</span>
                                    {/each}
                                    <button class="mv-bulk-btn accept" on:click={() => acceptAutoTags(m.id)}>
                                        ✓ {isEN ? 'Apply' : 'Aplicar'}
                                    </button>
                                    <button class="mv-bulk-btn" on:click={() => rejectAutoTags(m.id)}>
                                        ✕ {isEN ? 'Reject' : 'Rechazar'}
                                    </button>
                                {/if}
                            </div>
                        {/if}
                    </li>
                {/each}
            </ul>
        {/if}
    </section>
    {/if}

    <!-- ══════════════════════════ CRYSTALS ══════════════════════════ -->
    {#if activeTab === 'crystals'}
    <!-- Sprint E — Crystal viewer redesign.
         Old viewer was a flat text block; new design uses gradient cards
         with badge sections (Narrative / Outcomes / Files / Lessons) so
         the operator can scan a session distillation at a glance. -->
    <section class="mv-section">
        <div class="mv-toolbar">
            <span class="mv-hint">{isEN ? 'Run /crystallize from a chat tab to distill a session.' : 'Usa /crystallize en una pestaña para destilar una sesión.'}</span>
            <button class="mv-icon-btn" on:click={loadCrystals} title={isEN ? 'Refresh' : 'Recargar'}>
                <RefreshCw size={14}/>
            </button>
        </div>
        {#if crystalsError}<div class="mv-error"><AlertTriangle size={14}/> {crystalsError}</div>{/if}
        {#if crystalsLoading}
            <div class="mv-loading">{isEN ? 'Loading…' : 'Cargando…'}</div>
        {:else if crystals.length === 0}
            <div class="mv-empty crystal-empty">
                <Diamond size={32} color="#a78bfa" />
                <p>{isEN ? 'No crystals yet.' : 'Sin crystals todavía.'}</p>
                <small>{isEN
                    ? 'Crystals are LLM-distilled summaries of completed agent sessions: narrative + outcomes + files affected + lessons learned. Run /crystallize in a chat tab to create one.'
                    : 'Los crystals son resúmenes destilados por LLM de sesiones completadas: narrativa + outcomes + archivos afectados + lecciones. Usa /crystallize en una pestaña para crear uno.'}</small>
            </div>
        {:else}
            <ul class="crystal-grid">
                {#each crystals as c (c.id)}
                    {@const outcomes = tagList(c.key_outcomes)}
                    {@const files = tagList(c.files_affected)}
                    {@const lessons = tagList(c.lessons)}
                    {@const isExpanded = expandedCrystal === c.id}
                    <li class="crystal-card" class:expanded={isExpanded}>
                        <div class="crystal-shimmer"></div>
                        <header class="crystal-head">
                            <div class="crystal-icon">
                                <Diamond size={18} color="#a78bfa" />
                            </div>
                            <div class="crystal-meta">
                                <div class="crystal-id-row">
                                    <span class="crystal-id">#{c.id}</span>
                                    {#if c.project}<span class="crystal-project">{c.project}</span>{/if}
                                    <span class="crystal-date">{fmtDate(c.created_at)}</span>
                                </div>
                                <div class="crystal-stats">
                                    {#if outcomes.length}<span class="crystal-stat-pill outcomes-pill">⊕ {outcomes.length} {isEN ? 'outcomes' : 'outcomes'}</span>{/if}
                                    {#if files.length}<span class="crystal-stat-pill files-pill">⌗ {files.length} {isEN ? 'files' : 'archivos'}</span>{/if}
                                    {#if lessons.length}<span class="crystal-stat-pill lessons-pill">✦ {lessons.length} {isEN ? 'lessons' : 'lecciones'}</span>{/if}
                                </div>
                            </div>
                            <button class="crystal-del" title={isEN ? 'Delete' : 'Borrar'}
                                    on:click={() => deleteCrystal(c.id)}>
                                <Trash size={13}/>
                            </button>
                        </header>

                        <!-- Narrative is always visible — it's the headline. -->
                        <div class="crystal-narrative">
                            {#if isExpanded}
                                {c.narrative}
                            {:else}
                                {previewText(c.narrative, 280)}
                            {/if}
                        </div>

                        {#if isExpanded}
                            {#if outcomes.length}
                                <div class="crystal-section outcomes">
                                    <div class="crystal-section-hdr">
                                        <span class="crystal-section-glyph">⊕</span>
                                        <span>{isEN ? 'Outcomes' : 'Outcomes'}</span>
                                    </div>
                                    <ul class="crystal-bullet-list">
                                        {#each outcomes as o}<li>{o}</li>{/each}
                                    </ul>
                                </div>
                            {/if}
                            {#if files.length}
                                <div class="crystal-section files">
                                    <div class="crystal-section-hdr">
                                        <span class="crystal-section-glyph">⌗</span>
                                        <span>{isEN ? 'Files affected' : 'Archivos afectados'}</span>
                                    </div>
                                    <ul class="crystal-file-list">
                                        {#each files as f}<li class="mono">{f}</li>{/each}
                                    </ul>
                                </div>
                            {/if}
                            {#if lessons.length}
                                <div class="crystal-section lessons">
                                    <div class="crystal-section-hdr">
                                        <span class="crystal-section-glyph">✦</span>
                                        <span>{isEN ? 'Lessons learned' : 'Lecciones'}</span>
                                    </div>
                                    <ul class="crystal-bullet-list">
                                        {#each lessons as l}<li>{l}</li>{/each}
                                    </ul>
                                </div>
                            {/if}
                            <div class="crystal-footer">
                                <span class="mono">{c.source_chars.toLocaleString()} chars</span>
                                {#if c.session_id}
                                    <span> · session </span><code class="mono">{c.session_id.slice(0, 16)}</code>
                                {/if}
                            </div>
                        {/if}

                        <button class="crystal-expand-btn"
                                on:click={() => expandedCrystal = isExpanded ? null : c.id}>
                            {isExpanded
                                ? (isEN ? '▴ Collapse' : '▴ Colapsar')
                                : (isEN ? '▾ Show details' : '▾ Ver detalles')}
                        </button>
                    </li>
                {/each}
            </ul>
        {/if}
    </section>
    {/if}

    <!-- ══════════════════════════ INSIGHTS ══════════════════════════ -->
    {#if activeTab === 'insights'}
    <section class="mv-section">
        <div class="mv-toolbar">
            <span class="mv-hint">{isEN ? 'Auto-generated by reflect every 48h. Confidence grows with each reinforcement.' : 'Auto-generadas por reflect cada 48h. La confianza crece con cada refuerzo.'}</span>
            <button class="mv-icon-btn" on:click={loadInsights} title={isEN ? 'Refresh' : 'Recargar'}>
                <RefreshCw size={14}/>
            </button>
        </div>
        {#if insightsError}<div class="mv-error"><AlertTriangle size={14}/> {insightsError}</div>{/if}
        {#if insightsLoading}
            <div class="mv-loading">{isEN ? 'Loading…' : 'Cargando…'}</div>
        {:else if insights.length === 0}
            <div class="mv-empty">{isEN ? 'No insights yet. Run /reflect-now to derive some, or wait for the 48h auto-pass.' : 'Sin insights aún. Usa /reflect-now o espera al pase de 48h.'}</div>
        {:else}
            <ul class="mv-list">
                {#each insights as i (i.id)}
                    <li class="mv-card insight-card">
                        <div class="mv-card-head">
                            <Sparkles size={14} color="#fbbf24"/>
                            <span class="mv-id">#{i.id}</span>
                            <span class="mv-card-content insight-content">{i.content}</span>
                            <button class="mv-del" title={isEN ? 'Delete' : 'Borrar'}
                                on:click={() => deleteInsight(i.id)}><Trash size={13}/></button>
                        </div>
                        <div class="mv-confidence">
                            <div class="mv-conf-bar">
                                <div class="mv-conf-fill" style="width:{Math.round(i.confidence * 100)}%"></div>
                            </div>
                            <span class="mv-conf-pct">{Math.round(i.confidence * 100)}%</span>
                            <span class="mv-reinforce">×{i.reinforcements}</span>
                        </div>
                        <div class="mv-card-foot">
                            <span class="mv-date">{isEN ? 'last reinforced' : 'último refuerzo'}: {fmtDate(i.last_reinforced_at)}</span>
                            {#each tagList(i.concepts) as c}<span class="mv-tag concept">{c}</span>{/each}
                        </div>
                    </li>
                {/each}
            </ul>
        {/if}
    </section>
    {/if}

    <!-- ══════════════════════════ GRAFO ══════════════════════════ -->
    {#if activeTab === 'grafo'}
    <section class="mv-section">
        <div class="mv-toolbar">
            <span class="mv-hint">{isEN ? 'BFS from a memory id over shared concepts / files / sessions.' : 'BFS desde un id de memoria por concepts / files / sessions.'}</span>
            <!-- Tier S #2 — open full visual force-directed graph in overlay -->
            <button class="mv-graph-btn" on:click={() => showVisualGraph = true}
                    title={isEN ? 'Open interactive visual graph (force-directed)' : 'Abrir grafo visual interactivo (force-directed)'}
                    style="margin-left:auto;">
                ◊ {isEN ? 'Visual graph' : 'Grafo visual'}
            </button>
        </div>
        <div class="mv-graph-form">
            <label>
                {isEN ? 'Memory id' : 'Id memoria'}:
                <input type="number" bind:value={graphSeedId} placeholder="42" min="1"/>
            </label>
            <label>
                Hops:
                <select bind:value={graphHops}>
                    <option value={1}>1</option>
                    <option value={2}>2</option>
                    <option value={3}>3</option>
                    <option value={4}>4</option>
                </select>
            </label>
            <button class="mv-graph-btn" on:click={runGraphQuery} disabled={graphLoading}>
                {isEN ? 'Explore' : 'Explorar'}
            </button>
            <button class="mv-graph-btn alt" on:click={rebuildGraph} disabled={graphLoading} title={isEN ? 'Rebuild memory edges' : 'Reconstruir aristas del grafo'}>
                <RefreshCw size={13}/> {isEN ? 'Rebuild' : 'Reconstruir'}
            </button>
        </div>
        {#if graphError}<div class="mv-error"><AlertTriangle size={14}/> {graphError}</div>{/if}
        {#if graphLoading}
            <div class="mv-loading">{isEN ? 'Traversing…' : 'Recorriendo…'}</div>
        {:else if graphResults.length > 0}
            <ul class="mv-list">
                {#each graphResults as n (n.memory_id)}
                    <li class="mv-card">
                        <div class="mv-card-head">
                            <span class="mv-hop">hop {n.hops}</span>
                            <span class="mv-id">#{n.memory_id}</span>
                            <span class="mv-card-title">{n.memory.title}</span>
                            <span class="mv-edges" title={n.edge_types}>
                                {#each n.edge_types.split('|') as et}<span class="mv-edge">{edgeTypeIcon(et)}</span>{/each}
                            </span>
                            <span class="mv-score">{n.score.toFixed(3)}</span>
                        </div>
                        <p class="mv-card-content">{previewText(n.memory.content, 220)}</p>
                        <div class="mv-card-foot">
                            {#each tagList(n.memory.tags) as t}<span class="mv-tag">{t}</span>{/each}
                        </div>
                    </li>
                {/each}
            </ul>
        {/if}
    </section>
    {/if}

    <!-- ══════════════════════════ LESSONS ══════════════════════════ -->
    {#if activeTab === 'lessons'}
    <section class="mv-section">
        <div class="mv-toolbar">
            <div class="mv-search">
                <Search size={14}/>
                <input type="text" placeholder={isEN ? 'Search lessons…' : 'Buscar lecciones…'}
                    bind:value={lessonQuery} on:input={() => { clearTimeout(memSearchTimer); memSearchTimer = setTimeout(loadLessons, 300); }}/>
            </div>
            {#if lessonProjects.length > 0}
                <select bind:value={lessonProjectFilter} on:change={loadLessons} class="mv-select">
                    <option value="">{isEN ? 'All projects' : 'Todos los proyectos'}</option>
                    {#each lessonProjects as p}<option value={p}>{p}</option>{/each}
                </select>
            {/if}
            <button class="mv-icon-btn" on:click={loadLessons} title={isEN ? 'Refresh' : 'Recargar'}>
                <RefreshCw size={14}/>
            </button>
        </div>
        {#if lessonsError}<div class="mv-error"><AlertTriangle size={14}/> {lessonsError}</div>{/if}
        {#if lessonsLoading}
            <div class="mv-loading">{isEN ? 'Loading…' : 'Cargando…'}</div>
        {:else if lessons.length === 0}
            <div class="mv-empty">{isEN ? 'No lessons yet. Run /crystallize to extract lessons from sessions.' : 'Sin lecciones. Usa /crystallize para extraerlas de sesiones.'}</div>
        {:else}
            <ul class="mv-list">
                {#each lessons as l (l.id)}
                    <li class="mv-card lesson-card">
                        <div class="mv-card-head">
                            <BookOpen size={14} color="#34d399"/>
                            <span class="mv-lesson-source" title={l.source}>{l.source === 'memory' ? '◇' : '◆'}</span>
                            <span class="mv-card-content" style="flex:1;">{l.text}</span>
                        </div>
                        <div class="mv-card-foot">
                            <span class="mv-date">{fmtDate(l.createdAt)}</span>
                            {#if l.project}<span class="mv-tag concept">{l.project}</span>{/if}
                            {#each l.tags.slice(0, 5) as t}<span class="mv-tag">{t}</span>{/each}
                            <span style="flex:1;"></span>
                            <button class="mv-admin-btn" title={isEN ? 'Promote to critical memory' : 'Promover a memoria crítica'}
                                on:click={() => doPromoteLesson(l)}>↑ {isEN ? 'Promote' : 'Promover'}</button>
                            {#if l.source === 'memory'}
                                <button class="mv-admin-btn warn" title={isEN ? 'Dismiss' : 'Descartar'}
                                    on:click={() => doDismissLesson(l)}>✕</button>
                            {/if}
                        </div>
                    </li>
                {/each}
            </ul>
        {/if}
    </section>
    {/if}

    <!-- ══════════════════════════ SENTINELS ══════════════════════════ -->
    {#if activeTab === 'sentinels'}
    <section class="mv-section">
        <div class="mv-toolbar">
            <span class="mv-hint">{isEN ? 'Watchdog rules that trigger OS notifications when conditions are met.' : 'Reglas watchdog que disparan notificaciones cuando se cumple la condición.'}</span>
            <button class="mv-admin-btn" on:click={() => showSentinelAdd = !showSentinelAdd}>
                + {isEN ? 'New rule' : 'Nueva regla'}
            </button>
        </div>

        {#if showSentinelAdd}
            <div class="mv-sentinel-form">
                <input type="text" placeholder={isEN ? 'Rule name…' : 'Nombre de regla…'} bind:value={newSentinelName} class="mv-input"/>
                <select bind:value={newSentinelMetric} class="mv-select">
                    <option value="cpu_percent">CPU %</option>
                    <option value="ram_percent">RAM %</option>
                    <option value="disk_percent">Disk %</option>
                    <option value="disk_days_until_full">{isEN ? 'Disk days left' : 'Días disco'}</option>
                    <option value="anomaly_sigma">{isEN ? 'Anomaly σ' : 'Anomalía σ'}</option>
                    <option value="diag_status">{isEN ? 'Diag status' : 'Estado diag'}</option>
                </select>
                <select bind:value={newSentinelOp} class="mv-select">
                    {#each opOptions() as o}<option value={o.value}>{o.label}</option>{/each}
                </select>
                <input type="number" bind:value={newSentinelThreshold} class="mv-input" style="width:70px;"/>
                <input type="number" bind:value={newSentinelCooldown} class="mv-input" style="width:60px;" title={isEN ? 'Cooldown (min)' : 'Cooldown (min)'}/>
                <button class="mv-admin-btn" on:click={doAddSentinel}>{isEN ? 'Add' : 'Agregar'}</button>
            </div>
            <div class="mv-sentinel-templates">
                <span style="font-size:10px;color:var(--txt2);">{isEN ? 'Quick add:' : 'Agregar rápido:'}</span>
                {#each BUILTIN_TEMPLATES as tpl}
                    <button class="mv-sentinel-tpl" on:click={() => addFromTemplate(tpl)}>
                        {isEN ? tpl.nameEN : tpl.name}
                    </button>
                {/each}
            </div>
        {/if}

        {#if sentinelRules.length === 0}
            <div class="mv-empty">{isEN ? 'No sentinel rules. Add one above or use a template.' : 'Sin reglas sentinel. Agrega una arriba o usa una plantilla.'}</div>
        {:else}
            <ul class="mv-list">
                {#each sentinelRules as rule (rule.id)}
                    <li class="mv-card sentinel-card" class:disabled={!rule.enabled}>
                        <div class="mv-card-head">
                            <Shield size={14} color={rule.enabled ? '#34d399' : '#64748b'}/>
                            <span class="mv-card-title" style="flex:1;">{rule.name}</span>
                            <span class="mv-sentinel-cond">{rule.metric} {opOptions().find(o => o.value === rule.op)?.label || rule.op} {rule.threshold}</span>
                            <span class="mv-sentinel-fires" title={isEN ? 'Times fired' : 'Veces disparado'}>×{rule.fireCount}</span>
                        </div>
                        <div class="mv-card-foot">
                            <span class="mv-date">cooldown: {rule.cooldownMin}min</span>
                            {#if rule.lastFired > 0}<span class="mv-date">{isEN ? 'last' : 'último'}: {new Date(rule.lastFired).toLocaleString()}</span>{/if}
                            <span style="flex:1;"></span>
                            <button class="mv-admin-btn" on:click={() => doToggleSentinel(rule.id)}>
                                {rule.enabled ? (isEN ? 'Disable' : 'Desactivar') : (isEN ? 'Enable' : 'Activar')}
                            </button>
                            <button class="mv-admin-btn warn" on:click={() => doDeleteSentinel(rule.id)}>
                                <Trash size={12}/>
                            </button>
                        </div>
                    </li>
                {/each}
            </ul>
        {/if}
    </section>
    {/if}

    <!-- ══════════════════════════ PATTERNS ══════════════════════════ -->
    {#if activeTab === 'patterns'}
    <section class="mv-section">
        <div class="mv-toolbar">
            <span class="mv-hint">{isEN ? 'Deterministic pattern detection over memory corpus. No LLM needed.' : 'Detección determinista de patrones sobre el corpus de memorias. Sin LLM.'}</span>
            <button class="mv-icon-btn" on:click={loadPatterns} title={isEN ? 'Scan' : 'Escanear'}>
                <RefreshCw size={14}/>
            </button>
        </div>
        {#if patternsError}<div class="mv-error"><AlertTriangle size={14}/> {patternsError}</div>{/if}
        {#if patternsLoading}
            <div class="mv-loading">{isEN ? 'Scanning patterns…' : 'Escaneando patrones…'}</div>
        {:else if patternReport}
            <div class="mv-pattern-stats">
                {isEN ? 'Scanned' : 'Escaneado'}: {patternReport.scannedMemories} {isEN ? 'memories' : 'memorias'} + {patternReport.scannedCrystals} crystals · {patternReport.scanDurationMs}ms · {patternReport.patterns.length} {isEN ? 'patterns found' : 'patrones encontrados'}
            </div>
            {#if patternReport.patterns.length === 0}
                <div class="mv-empty">{isEN ? 'No patterns detected. More data needed.' : 'Sin patrones detectados. Se necesitan más datos.'}</div>
            {:else}
                <ul class="mv-list">
                    {#each patternReport.patterns as p, idx (idx)}
                        <li class="mv-card pattern-card">
                            <div class="mv-card-head">
                                <span class="mv-pattern-icon">{patternKindIcon(p.kind)}</span>
                                <span class="mv-card-title" style="flex:1;">{p.title}</span>
                                <span class="mv-pattern-conf" style="color:{confidenceColor(p.confidence)};">{Math.round(p.confidence * 100)}%</span>
                                <span class="mv-pattern-evidence">×{p.evidenceCount}</span>
                            </div>
                            <p class="mv-card-content">{p.description}</p>
                            <div class="mv-card-foot">
                                <span class="mv-tag concept">{patternKindLabel(p.kind, isEN)}</span>
                            </div>
                        </li>
                    {/each}
                </ul>
            {/if}
        {:else}
            <div class="mv-empty">{isEN ? 'Click Scan to detect patterns.' : 'Haz clic en Escanear para detectar patrones.'}</div>
        {/if}
    </section>
    {/if}

    <!-- ══════════════════════════ VERIFY ══════════════════════════ -->
    {#if activeTab === 'verify'}
    <section class="mv-section">
        <div class="mv-toolbar">
            <span class="mv-hint">{isEN ? 'Cross-check memories for contradictions. Heuristic detection.' : 'Verificar memorias en busca de contradicciones. Detección heurística.'}</span>
            <button class="mv-icon-btn" on:click={loadVerify} title={isEN ? 'Scan' : 'Escanear'}>
                <RefreshCw size={14}/>
            </button>
        </div>
        {#if verifyError}<div class="mv-error"><AlertTriangle size={14}/> {verifyError}</div>{/if}
        {#if verifyLoading}
            <div class="mv-loading">{isEN ? 'Verifying…' : 'Verificando…'}</div>
        {:else if verifyReport}
            <div class="mv-pattern-stats">
                {isEN ? 'Checked' : 'Verificado'}: {verifyReport.memoriesScanned} {isEN ? 'memories' : 'memorias'}, {verifyReport.pairsChecked} {isEN ? 'pairs' : 'pares'} · {verifyReport.scanDurationMs}ms · {verifyReport.contradictions.length} {isEN ? 'conflicts' : 'conflictos'}
            </div>
            {#if verifyReport.contradictions.length === 0}
                <div class="mv-empty">{isEN ? 'No contradictions detected!' : '¡Sin contradicciones detectadas!'}</div>
            {:else}
                <ul class="mv-list">
                    {#each verifyReport.contradictions as c (c.id)}
                        <li class="mv-card verify-card">
                            <div class="mv-card-head">
                                <span class="mv-verify-sev" style="color:{severityColor(c.severity)};">●</span>
                                <span class="mv-card-title" style="flex:1;">{c.reason}</span>
                                <span class="mv-tag" style="background:{severityColor(c.severity)}20;color:{severityColor(c.severity)};">{severityLabel(c.severity, isEN)}</span>
                            </div>
                            <div class="mv-verify-pair">
                                <div class="mv-verify-mem older">
                                    <span class="mv-verify-label">{isEN ? 'Older' : 'Anterior'} #{c.pair.older.id}</span>
                                    <p>{previewText(c.pair.older.content, 150)}</p>
                                    {#if c.snippetOlder}<code class="mv-verify-snippet">{c.snippetOlder}</code>{/if}
                                </div>
                                <div class="mv-verify-mem newer">
                                    <span class="mv-verify-label">{isEN ? 'Newer' : 'Reciente'} #{c.pair.newer.id}</span>
                                    <p>{previewText(c.pair.newer.content, 150)}</p>
                                    {#if c.snippetNewer}<code class="mv-verify-snippet">{c.snippetNewer}</code>{/if}
                                </div>
                            </div>
                            <div class="mv-card-foot mv-verify-actions">
                                <button class="mv-admin-btn" on:click={() => doResolve(c, 'keep_newer')}>{resolutionLabel('keep_newer', isEN)}</button>
                                <button class="mv-admin-btn" on:click={() => doResolve(c, 'keep_older')}>{resolutionLabel('keep_older', isEN)}</button>
                                <button class="mv-admin-btn" on:click={() => doResolve(c, 'keep_both')}>{resolutionLabel('keep_both', isEN)}</button>
                                <button class="mv-admin-btn" on:click={() => doResolve(c, 'merge')}>{resolutionLabel('merge', isEN)}</button>
                                <button class="mv-admin-btn" on:click={() => doResolve(c, 'dismiss')}>{resolutionLabel('dismiss', isEN)}</button>
                            </div>
                        </li>
                    {/each}
                </ul>
            {/if}
        {:else}
            <div class="mv-empty">{isEN ? 'Click Scan to check for contradictions.' : 'Haz clic en Escanear para buscar contradicciones.'}</div>
        {/if}
    </section>
    {/if}
</div>

<!-- Tier S #2 — Full-screen force-directed memory graph overlay -->
{#if showVisualGraph}
    <MemoryGraphView {isEN}
        on:close={() => showVisualGraph = false}
        on:openmemoria={(e) => {
            // V12 — Memory Graph 2.0: jump from the visual graph to the
            // Memorias tab. We do NOT filter by id — FTS5 doesn't index
            // the numeric id, so a filter-by-id would return empty even
            // though the memory exists (bug reported by user). Instead,
            // jumpToMemory() clears filters, loads everything, scrolls to
            // the row, and flashes a highlight border.
            showVisualGraph = false;
            activeTab = 'memorias';
            jumpToMemory(e.detail.memoryId);
        }}/>
{/if}

<style>
    .memory-view {
        flex: 1;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        padding: 14px 22px;
        gap: 12px;
        max-width: 1100px;
        width: 100%;
        margin: 0 auto;
    }
    .mv-header {
        display: flex; align-items: center; justify-content: space-between;
        flex-shrink: 0;
        gap: 16px;
        flex-wrap: wrap;
    }
    .mv-title { display: flex; align-items: center; gap: 8px; color: var(--txt); }
    .mv-title h2 { margin: 0; font-size: 16px; font-weight: 600; letter-spacing: .3px; }
    .mv-tabs {
        display: flex; gap: 4px;
        background: rgba(255,255,255,.03);
        border: 1px solid rgba(255,255,255,.06);
        border-radius: 8px;
        padding: 3px;
    }
    .mv-tabs button {
        display: flex; align-items: center; gap: 5px;
        background: transparent; border: none;
        color: var(--txt2);
        padding: 5px 10px;
        border-radius: 6px;
        cursor: pointer;
        font-size: 12px;
        transition: .15s;
    }
    .mv-tabs button:hover { background: rgba(255,255,255,.05); color: var(--txt); }
    .mv-tabs button.active {
        background: rgba(96,165,250,.15);
        color: var(--accent);
        box-shadow: inset 0 0 0 1px rgba(96,165,250,.25);
    }
    .mv-section {
        flex: 1;
        display: flex; flex-direction: column;
        overflow: hidden;
        gap: 10px;
    }
    .mv-toolbar {
        display: flex; align-items: center; gap: 8px;
        flex-wrap: wrap;
        flex-shrink: 0;
    }
    .mv-search {
        display: flex; align-items: center; gap: 6px;
        background: rgba(255,255,255,.03);
        border: 1px solid rgba(255,255,255,.07);
        border-radius: 6px;
        padding: 5px 9px;
        flex: 1;
        min-width: 260px;
        color: var(--txt2);
    }
    .mv-search input {
        flex: 1; background: transparent; border: none; outline: none;
        color: var(--txt); font-size: 12px;
    }
    .mv-select {
        background: rgba(255,255,255,.03); color: var(--txt);
        border: 1px solid rgba(255,255,255,.07); border-radius: 6px;
        padding: 5px 8px; font-size: 12px;
    }
    .mv-icon-btn, .mv-graph-btn, .mv-admin-btn {
        background: rgba(255,255,255,.03); color: var(--txt2);
        border: 1px solid rgba(255,255,255,.07); border-radius: 6px;
        padding: 5px 9px; font-size: 12px; cursor: pointer;
        display: inline-flex; align-items: center; gap: 5px;
        transition: .15s;
    }
    .mv-icon-btn:hover, .mv-graph-btn:hover, .mv-admin-btn:hover {
        background: rgba(255,255,255,.07); color: var(--txt);
    }
    .mv-graph-btn { padding: 6px 12px; }
    .mv-graph-btn.alt { background: rgba(245,158,11,.08); border-color: rgba(245,158,11,.2); color: #f59e0b; }
    .mv-graph-btn:disabled, .mv-admin-btn:disabled { opacity: .4; cursor: not-allowed; }
    .mv-admin { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; flex-shrink: 0; font-size: 11px; }
    .mv-admin-btn { font-size: 11px; padding: 3px 8px; }
    .mv-admin-btn.warn { background: rgba(239,68,68,.08); border-color: rgba(239,68,68,.25); color: #f87171; }
    .mv-admin-msg { color: var(--txt2); font-size: 11px; font-style: italic; margin-left: 6px; }
    .mv-hint { color: var(--txt2); font-size: 11px; font-style: italic; flex: 1; }
    .mv-graph-form {
        display: flex; align-items: center; gap: 12px; flex-wrap: wrap;
        background: rgba(255,255,255,.02);
        border: 1px solid rgba(255,255,255,.06);
        border-radius: 8px;
        padding: 10px 14px;
        flex-shrink: 0;
    }
    .mv-graph-form label { display: inline-flex; align-items: center; gap: 6px; color: var(--txt2); font-size: 12px; }
    .mv-graph-form input[type=number] {
        background: rgba(255,255,255,.04); color: var(--txt);
        border: 1px solid rgba(255,255,255,.08); border-radius: 5px;
        padding: 4px 8px; font-size: 12px; width: 90px;
    }
    .mv-graph-form select {
        background: rgba(255,255,255,.04); color: var(--txt);
        border: 1px solid rgba(255,255,255,.08); border-radius: 5px;
        padding: 4px 8px; font-size: 12px;
    }

    .mv-error {
        display: flex; align-items: center; gap: 8px;
        background: rgba(239,68,68,.08);
        border: 1px solid rgba(239,68,68,.25);
        color: #f87171;
        padding: 8px 12px; border-radius: 6px; font-size: 12px;
        flex-shrink: 0;
    }
    .mv-loading, .mv-empty {
        color: var(--txt2); text-align: center; padding: 30px 10px;
        font-size: 12px;
    }
    .mv-list {
        list-style: none; padding: 0; margin: 0;
        overflow-y: auto;
        flex: 1;
        display: flex; flex-direction: column; gap: 6px;
    }
    .mv-card {
        background: rgba(255,255,255,.02);
        border: 1px solid rgba(255,255,255,.06);
        border-radius: 8px;
        padding: 8px 12px;
        transition: border-color .15s, background .15s;
    }
    .mv-card:hover { border-color: rgba(255,255,255,.12); }
    /* .mv-card.expanded retired Sprint E — crystals use dedicated styles now */
    /* M2 — selected card visual state */
    .mv-card.mv-card-selected {
        background: rgba(16,185,129,.06);
        border-color: rgba(16,185,129,.30);
        box-shadow: inset 2px 0 0 var(--acc, #10b981);
    }
    /* V12 — Highlight when arrived from the visual graph "Open in Memorias".
       Pulsa por 4s después de jumpToMemory(); auto-aclara por timer en JS. */
    .mv-card.mv-card-highlight {
        background: rgba(16,185,129,.10) !important;
        border-color: rgba(16,185,129,.60) !important;
        box-shadow: 0 0 0 2px rgba(16,185,129,.35),
                    0 6px 22px -8px rgba(16,185,129,.40);
        animation: mvHighlightPulse 0.9s ease-in-out 2;
    }
    @keyframes mvHighlightPulse {
        0%, 100% { box-shadow: 0 0 0 2px rgba(16,185,129,.35),
                               0 6px 22px -8px rgba(16,185,129,.40); }
        50%      { box-shadow: 0 0 0 4px rgba(16,185,129,.55),
                               0 10px 30px -6px rgba(16,185,129,.55); }
    }
    .mv-bulk-cb {
        cursor: pointer; margin-right: 4px;
        width: 14px; height: 14px;
        accent-color: var(--acc, #10b981);
    }
    /* M1 — Stats dashboard cards */
    .mv-stats {
        display: flex; gap: 8px; padding: 8px 12px;
        flex-wrap: wrap; flex-shrink: 0;
    }
    .mv-stat {
        background: rgba(255,255,255,.03);
        border: 1px solid rgba(255,255,255,.06);
        border-radius: 6px;
        padding: 6px 12px;
        display: flex; flex-direction: column; align-items: center;
        min-width: 72px;
    }
    .mv-stat-num { font-size: 18px; font-weight: 300; color: var(--txt); line-height: 1.1; }
    .mv-stat-lbl { font-size: 9px; color: var(--txt2); text-transform: uppercase; letter-spacing: .3px; margin-top: 2px; }
    .mv-stat.hot   { border-color: rgba(245,158,11,.30); background: rgba(245,158,11,.05); }
    .mv-stat.hot   .mv-stat-num { color: #f59e0b; }
    .mv-stat.warn  { border-color: rgba(239,68,68,.30); background: rgba(239,68,68,.05); }
    .mv-stat.warn  .mv-stat-num { color: #ef4444; }
    /* Sprint E — Crystal viewer redesign */
    .crystal-empty {
        display: flex; flex-direction: column; align-items: center;
        gap: 12px; padding: 40px 24px; max-width: 480px; margin: 0 auto;
        line-height: 1.6;
    }
    .crystal-empty small { color: var(--txt2); font-size: 11px; }

    .crystal-grid {
        list-style: none; margin: 0; padding: 0 12px 12px;
        display: flex; flex-direction: column; gap: 12px;
    }
    .crystal-card {
        position: relative; overflow: hidden;
        background: linear-gradient(135deg, rgba(167,139,250,.04), rgba(96,165,250,.02));
        border: 1px solid rgba(167,139,250,.16);
        border-radius: 10px;
        padding: 14px 16px;
        transition: border-color .15s, box-shadow .15s;
    }
    .crystal-card:hover {
        border-color: rgba(167,139,250,.32);
        box-shadow: 0 4px 18px -8px rgba(167,139,250,.30);
    }
    .crystal-card.expanded {
        background: linear-gradient(135deg, rgba(167,139,250,.08), rgba(96,165,250,.04));
        border-color: rgba(167,139,250,.40);
    }
    /* Subtle diagonal shimmer for visual interest */
    .crystal-shimmer {
        position: absolute; top: -40%; right: -20%;
        width: 140px; height: 220px;
        background: linear-gradient(135deg, transparent, rgba(167,139,250,.06), transparent);
        transform: rotate(15deg);
        pointer-events: none;
    }
    .crystal-head {
        display: flex; align-items: flex-start; gap: 10px;
        margin-bottom: 10px; position: relative;
    }
    .crystal-icon {
        background: rgba(167,139,250,.10);
        border: 1px solid rgba(167,139,250,.25);
        border-radius: 8px; padding: 6px;
        display: flex; align-items: center; justify-content: center;
        flex-shrink: 0;
    }
    .crystal-meta { flex: 1; min-width: 0; }
    .crystal-id-row {
        display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap;
        margin-bottom: 6px;
    }
    .crystal-id {
        color: #a78bfa; font-family: var(--mono);
        font-size: 11px; font-weight: 700;
        background: rgba(167,139,250,.10);
        padding: 1px 8px; border-radius: 4px;
    }
    .crystal-project {
        color: var(--txt2); font-size: 10px; font-family: var(--mono);
        background: rgba(255,255,255,.04);
        padding: 1px 6px; border-radius: 4px;
    }
    .crystal-date {
        color: var(--txt2); font-size: 10px;
        margin-left: auto; font-family: var(--mono);
    }
    .crystal-stats {
        display: flex; gap: 6px; flex-wrap: wrap;
    }
    .crystal-stat-pill {
        font-size: 10px; padding: 2px 8px;
        border-radius: 10px; font-weight: 600;
        border: 1px solid;
    }
    .crystal-stat-pill.outcomes-pill {
        background: rgba(16,185,129,.10); color: #10b981;
        border-color: rgba(16,185,129,.25);
    }
    .crystal-stat-pill.files-pill {
        background: rgba(96,165,250,.10); color: #60a5fa;
        border-color: rgba(96,165,250,.25);
    }
    .crystal-stat-pill.lessons-pill {
        background: rgba(251,191,36,.10); color: #fbbf24;
        border-color: rgba(251,191,36,.25);
    }
    .crystal-del {
        background: transparent; border: 0; padding: 4px;
        color: var(--txt2); cursor: pointer;
        opacity: 0.5; transition: opacity .15s, color .15s;
        align-self: flex-start;
    }
    .crystal-del:hover { opacity: 1; color: #ef4444; }
    .crystal-narrative {
        color: var(--txt); font-size: 12px; line-height: 1.65;
        padding: 8px 12px;
        background: rgba(0,0,0,.18);
        border-left: 3px solid rgba(167,139,250,.40);
        border-radius: 0 6px 6px 0;
        margin-bottom: 8px;
    }
    .crystal-card.expanded .crystal-narrative {
        white-space: pre-wrap; word-break: break-word;
    }
    .crystal-section {
        margin-top: 10px;
        padding: 10px 12px;
        background: rgba(255,255,255,.02);
        border-radius: 6px;
    }
    .crystal-section.outcomes { border-left: 3px solid rgba(16,185,129,.50); }
    .crystal-section.files    { border-left: 3px solid rgba(96,165,250,.50); }
    .crystal-section.lessons  { border-left: 3px solid rgba(251,191,36,.50); }
    .crystal-section-hdr {
        display: flex; align-items: center; gap: 6px;
        font-size: 10px; font-weight: 700; letter-spacing: .4px;
        text-transform: uppercase; margin-bottom: 6px;
        color: var(--txt2);
    }
    .crystal-section.outcomes .crystal-section-hdr { color: #10b981; }
    .crystal-section.files    .crystal-section-hdr { color: #60a5fa; }
    .crystal-section.lessons  .crystal-section-hdr { color: #fbbf24; }
    .crystal-section-glyph { font-size: 13px; }
    .crystal-bullet-list, .crystal-file-list {
        margin: 0; padding: 0 0 0 18px;
        font-size: 11px; line-height: 1.7; color: var(--txt);
    }
    .crystal-bullet-list li { margin-bottom: 2px; }
    .crystal-file-list li {
        list-style: none; position: relative;
        padding-left: 14px; margin-bottom: 2px;
        word-break: break-all;
    }
    .crystal-file-list li::before {
        content: '›'; position: absolute; left: 0;
        color: #60a5fa; font-weight: 700;
    }
    .crystal-footer {
        margin-top: 10px; padding-top: 8px;
        border-top: 1px solid rgba(255,255,255,.06);
        font-size: 10px; color: var(--txt2);
    }
    .crystal-expand-btn {
        margin-top: 10px; width: 100%;
        background: rgba(167,139,250,.08);
        border: 1px solid rgba(167,139,250,.20);
        color: #a78bfa; font: inherit; font-size: 10px;
        padding: 5px 10px; border-radius: 5px;
        cursor: pointer; letter-spacing: .3px;
        transition: background .12s, border-color .12s;
    }
    .crystal-expand-btn:hover {
        background: rgba(167,139,250,.16);
        border-color: rgba(167,139,250,.35);
    }

    /* Sprint D — Memory health timeline */
    .mv-timeline {
        display: flex; align-items: flex-end; gap: 8px;
        padding: 8px 12px;
        background: rgba(255,255,255,.02);
        border: 1px solid rgba(255,255,255,.04);
        border-radius: 6px;
        margin: 0 12px 8px;
    }
    .mv-timeline-lbl {
        font-size: 9px; color: var(--txt2);
        text-transform: uppercase; letter-spacing: .4px;
        min-width: 60px;
    }
    .mv-timeline-bars {
        flex: 1; display: flex; align-items: flex-end;
        gap: 2px; height: 28px;
    }
    .mv-tl-bar {
        flex: 1; border-radius: 1px;
        transition: background .15s;
        cursor: help;
    }
    .mv-tl-bar:hover { filter: brightness(1.4); }
    .mv-timeline-end {
        display: flex; flex-direction: column; gap: 2px;
        font-size: 9px; color: var(--txt2);
        align-items: flex-end; min-width: 36px;
    }
    /* M2 — Bulk operations bar */
    .mv-bulk-bar {
        display: flex; align-items: center; gap: 6px;
        padding: 6px 12px;
        background: rgba(16,185,129,.06);
        border: 1px solid rgba(16,185,129,.20);
        border-radius: 6px; margin: 0 12px 6px;
        flex-wrap: wrap;
    }
    .mv-bulk-count {
        font-size: 11px; font-weight: 600;
        color: var(--acc, #10b981);
        margin-right: 6px;
    }
    .mv-bulk-btn {
        background: rgba(255,255,255,.04);
        border: 1px solid rgba(255,255,255,.08);
        color: var(--txt); font: inherit; font-size: 11px;
        padding: 3px 9px; border-radius: 4px; cursor: pointer;
        transition: background .12s, border-color .12s;
    }
    .mv-bulk-btn:hover:not(:disabled) {
        background: rgba(255,255,255,.07);
        border-color: rgba(255,255,255,.16);
    }
    .mv-bulk-btn:disabled { opacity: 0.45; cursor: default; }
    .mv-bulk-btn.warn {
        background: rgba(239,68,68,.10); color: #ef4444;
        border-color: rgba(239,68,68,.25);
    }
    .mv-bulk-btn.warn:hover:not(:disabled) { background: rgba(239,68,68,.20); }
    .mv-bulk-btn.accept {
        background: rgba(16,185,129,.18); color: var(--acc, #10b981);
        border-color: rgba(16,185,129,.35);
    }
    .mv-bulk-btn.accept:hover:not(:disabled) { background: rgba(16,185,129,.28); }
    /* M3 — Auto-tag suggestions panel inside card */
    .mv-autotag-panel {
        display: flex; align-items: center; gap: 6px;
        margin-top: 6px; padding: 6px 8px;
        background: rgba(168,85,247,.06);
        border: 1px dashed rgba(168,85,247,.25);
        border-radius: 5px;
        flex-wrap: wrap;
    }
    .mv-autotag-label { font-size: 10px; color: #a855f7; font-weight: 600; letter-spacing: .3px; }
    .mv-autotag-empty { font-size: 11px; color: var(--txt2); font-style: italic; flex: 1; }
    .mv-tag-sugg {
        background: rgba(168,85,247,.15) !important;
        color: #a855f7 !important;
        border: 1px dashed rgba(168,85,247,.40);
    }
    .mv-card-head {
        display: flex; align-items: center; gap: 8px;
        flex-wrap: wrap;
    }
    /* .mv-card-head.clickable retired Sprint E with crystal redesign */
    .mv-id { color: var(--txt2); font-size: 10px; font-family: var(--mono); }
    .mv-card-title { color: var(--txt); font-size: 13px; font-weight: 500; flex: 1; min-width: 0; }
    .mv-imp { font-family: var(--mono); font-size: 11px; letter-spacing: 2px; }
    .mv-date { color: var(--txt2); font-size: 10px; font-family: var(--mono); }
    .mv-card-content {
        color: var(--txt2); font-size: 12px; line-height: 1.5;
        margin: 6px 0 4px 0;
        white-space: pre-wrap;
    }
    .insight-content { font-style: italic; color: var(--txt); }
    .mv-card-foot {
        display: flex; align-items: center; gap: 6px; flex-wrap: wrap;
        font-size: 10px; color: var(--txt2);
        margin-top: 4px;
    }
    .mv-tag {
        background: rgba(96,165,250,.08);
        border: 1px solid rgba(96,165,250,.18);
        color: #93c5fd;
        padding: 1px 6px; border-radius: 3px; font-size: 10px;
    }
    .mv-tag.concept { background: rgba(251,191,36,.08); border-color: rgba(251,191,36,.20); color: #fde68a; }
    .mv-del {
        background: transparent; border: none; color: var(--txt2); cursor: pointer;
        padding: 3px; border-radius: 4px;
        transition: .15s;
    }
    .mv-del:hover { background: rgba(239,68,68,.12); color: #f87171; }
    /* v1.6.9 — annealing cluster verdict pill, parallel to GroundingChip */
    .mv-anneal {
        display: inline-flex; align-items: center; gap: 3px;
        padding: 1px 7px; border-radius: 9px;
        font-size: 10px; font-weight: 600;
        font-family: var(--mono, ui-monospace, monospace);
        line-height: 1.4; border: 1px solid transparent;
        letter-spacing: .2px;
    }
    .mv-anneal-demote  { color: var(--red, #ef4444);  background: rgba(239, 68, 68, .08);  border-color: rgba(239, 68, 68, .30); }
    .mv-anneal-watch   { color: var(--amber, #f59e0b); background: rgba(245, 158, 11, .08); border-color: rgba(245, 158, 11, .25); }
    .mv-anneal-promote { color: var(--acc, #10b981);  background: rgba(16, 185, 129, .08); border-color: rgba(16, 185, 129, .25); }
    /* .mv-card-detail retired Sprint E with crystal redesign */
    .mv-confidence {
        display: flex; align-items: center; gap: 8px;
        margin: 4px 0;
    }
    .mv-conf-bar {
        flex: 1; max-width: 220px; height: 5px;
        background: rgba(255,255,255,.05);
        border-radius: 3px; overflow: hidden;
    }
    .mv-conf-fill {
        height: 100%; background: linear-gradient(90deg, #f59e0b, #fbbf24);
        transition: width .4s ease;
    }
    .mv-conf-pct { color: #fbbf24; font-size: 11px; font-weight: 600; font-family: var(--mono); }
    .mv-reinforce { color: var(--txt2); font-size: 10px; font-family: var(--mono); }
    .mv-hop {
        background: rgba(167,139,250,.10); color: #c4b5fd;
        padding: 1px 6px; border-radius: 3px;
        font-size: 10px; font-family: var(--mono);
    }
    .mv-edges { display: inline-flex; gap: 2px; }
    .mv-edge { color: var(--accent); font-size: 11px; }
    .mv-score { color: var(--txt2); font-size: 10px; font-family: var(--mono); }
    code { font-family: var(--mono); font-size: 11px; background: rgba(255,255,255,.04); padding: 1px 4px; border-radius: 3px; }

    /* ── Tab separator ── */
    .mv-tab-sep { color: rgba(255,255,255,.1); font-size: 10px; align-self: center; user-select: none; }

    /* ── Lessons ── */
    .lesson-card .mv-card-content { font-size: 12px; line-height: 1.5; }
    .mv-lesson-source { font-size: 12px; opacity: .6; flex-shrink: 0; }

    /* ── Sentinels ── */
    .mv-sentinel-form {
        display: flex; align-items: center; gap: 6px; flex-wrap: wrap;
        padding: 10px; background: rgba(255,255,255,.02);
        border-radius: 6px; border: 1px solid rgba(255,255,255,.06);
    }
    .mv-sentinel-templates {
        display: flex; align-items: center; gap: 4px; flex-wrap: wrap;
        padding: 6px 10px; margin-top: 4px;
    }
    .mv-sentinel-tpl {
        background: rgba(52,211,153,.06); border: 1px solid rgba(52,211,153,.15);
        color: #34d399; padding: 2px 8px; border-radius: 4px;
        font-size: 10px; cursor: pointer; transition: .15s;
    }
    .mv-sentinel-tpl:hover { background: rgba(52,211,153,.12); border-color: rgba(52,211,153,.3); }
    .sentinel-card.disabled { opacity: .5; }
    .mv-sentinel-cond {
        font-family: var(--mono); font-size: 11px;
        color: var(--accent); background: rgba(96,165,250,.08);
        padding: 1px 6px; border-radius: 3px;
    }
    .mv-sentinel-fires { color: var(--txt2); font-size: 10px; font-family: var(--mono); }
    .mv-input {
        background: rgba(255,255,255,.04); border: 1px solid rgba(255,255,255,.08);
        color: var(--txt); padding: 4px 8px; border-radius: 4px;
        font-size: 11px; font-family: var(--mono);
    }
    .mv-input:focus { border-color: var(--accent); outline: none; }

    /* ── Patterns ── */
    .mv-pattern-stats {
        font-size: 10px; color: var(--txt2); font-family: var(--mono);
        padding: 4px 0; border-bottom: 1px solid rgba(255,255,255,.04);
    }
    .mv-pattern-icon { font-size: 14px; flex-shrink: 0; }
    .mv-pattern-conf { font-size: 11px; font-weight: 600; font-family: var(--mono); }
    .mv-pattern-evidence { color: var(--txt2); font-size: 10px; font-family: var(--mono); }

    /* ── Verify ── */
    .mv-verify-sev { font-size: 14px; flex-shrink: 0; }
    .mv-verify-pair {
        display: grid; grid-template-columns: 1fr 1fr; gap: 8px;
        padding: 8px 0;
    }
    .mv-verify-mem {
        padding: 8px; border-radius: 5px;
        font-size: 11px; line-height: 1.5;
    }
    .mv-verify-mem.older { background: rgba(248,113,113,.04); border: 1px solid rgba(248,113,113,.12); }
    .mv-verify-mem.newer { background: rgba(52,211,153,.04); border: 1px solid rgba(52,211,153,.12); }
    .mv-verify-mem p { margin: 4px 0; color: var(--txt2); }
    .mv-verify-label {
        font-size: 10px; font-weight: 600; font-family: var(--mono);
        text-transform: uppercase; letter-spacing: .5px;
    }
    .mv-verify-mem.older .mv-verify-label { color: #f87171; }
    .mv-verify-mem.newer .mv-verify-label { color: #34d399; }
    .mv-verify-snippet {
        display: block; margin-top: 4px; padding: 3px 6px;
        background: rgba(0,0,0,.2); border-radius: 3px;
        font-size: 10px; word-break: break-all;
    }
    .mv-verify-actions { gap: 4px; flex-wrap: wrap; }
</style>
