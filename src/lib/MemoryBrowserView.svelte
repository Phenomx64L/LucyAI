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

    export let isEN: boolean = false;

    // ── Tab state ────────────────────────────────────────────────────────
    type Tab = 'memorias' | 'crystals' | 'insights' | 'grafo';
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
    onMount(loadMemorias);

    // Switch tabs (load on first switch)
    let loadedTabs = new Set<Tab>(['memorias']);
    function switchTab(t: Tab) {
        activeTab = t;
        if (loadedTabs.has(t)) return;
        loadedTabs.add(t);
        if (t === 'crystals') loadCrystals();
        else if (t === 'insights') loadInsights();
    }
</script>

<div class="memory-view">
    <header class="mv-header">
        <div class="mv-title">
            <Brain size={22} strokeWidth={2}/>
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
        </nav>
    </header>

    <!-- ══════════════════════════ MEMORIES ══════════════════════════ -->
    {#if activeTab === 'memorias'}
    <section class="mv-section">
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
            <div class="mv-loading">{isEN ? 'Loading…' : 'Cargando…'}</div>
        {:else if memorias.length === 0}
            <div class="mv-empty">{isEN ? 'No memories. Lucy will populate this as she works.' : 'Sin memorias. Lucy las llenará conforme trabajes.'}</div>
        {:else}
            <ul class="mv-list">
                {#each memorias as m (m.id)}
                    <li class="mv-card">
                        <div class="mv-card-head">
                            <span class="mv-id">#{m.id}</span>
                            <span class="mv-card-title">{m.title}</span>
                            <span class="mv-imp" style="color:{importanceColor(m.importance)};" title="importance">
                                {'◆'.repeat(m.importance)}{'·'.repeat(3 - m.importance)}
                            </span>
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
                    </li>
                {/each}
            </ul>
        {/if}
    </section>
    {/if}

    <!-- ══════════════════════════ CRYSTALS ══════════════════════════ -->
    {#if activeTab === 'crystals'}
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
            <div class="mv-empty">{isEN ? 'No crystals yet.' : 'Sin crystals todavía.'}</div>
        {:else}
            <ul class="mv-list">
                {#each crystals as c (c.id)}
                    <li class="mv-card" class:expanded={expandedCrystal === c.id}>
                        <div class="mv-card-head clickable"
                            on:click={() => expandedCrystal = expandedCrystal === c.id ? null : c.id}
                            on:keydown={(e) => e.key === 'Enter' && (expandedCrystal = expandedCrystal === c.id ? null : c.id)}
                            role="button" tabindex="0">
                            <Diamond size={14} color="#a78bfa"/>
                            <span class="mv-id">#{c.id}</span>
                            <span class="mv-card-title">{previewText(c.narrative, 120)}</span>
                            <span class="mv-date">{fmtDate(c.created_at)}</span>
                            <button class="mv-del" title={isEN ? 'Delete' : 'Borrar'}
                                on:click|stopPropagation={() => deleteCrystal(c.id)}><Trash size={13}/></button>
                        </div>
                        {#if expandedCrystal === c.id}
                            <div class="mv-card-detail">
                                <p><strong>{isEN ? 'Narrative' : 'Narrativa'}:</strong> {c.narrative}</p>
                                {#if tagList(c.key_outcomes).length}
                                    <div><strong>Outcomes:</strong>
                                        <ul>{#each tagList(c.key_outcomes) as o}<li>{o}</li>{/each}</ul>
                                    </div>
                                {/if}
                                {#if tagList(c.files_affected).length}
                                    <div><strong>{isEN ? 'Files' : 'Archivos'}:</strong>
                                        <ul class="mono">{#each tagList(c.files_affected) as f}<li>{f}</li>{/each}</ul>
                                    </div>
                                {/if}
                                {#if tagList(c.lessons).length}
                                    <div><strong>{isEN ? 'Lessons' : 'Lecciones'}:</strong>
                                        <ul>{#each tagList(c.lessons) as l}<li>{l}</li>{/each}</ul>
                                    </div>
                                {/if}
                                <p class="mv-meta">{isEN ? 'Source' : 'Fuente'}: {c.source_chars.toLocaleString()} chars · session <code>{(c.session_id || '—').slice(0,12)}</code></p>
                            </div>
                        {/if}
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
</div>

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
    .mv-card.expanded { background: rgba(167,139,250,.04); border-color: rgba(167,139,250,.25); }
    .mv-card-head {
        display: flex; align-items: center; gap: 8px;
        flex-wrap: wrap;
    }
    .mv-card-head.clickable { cursor: pointer; }
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
    .mv-card-detail {
        padding-top: 8px;
        border-top: 1px solid rgba(255,255,255,.06);
        color: var(--txt2); font-size: 12px;
        line-height: 1.55;
        margin-top: 8px;
    }
    .mv-card-detail strong { color: var(--txt); font-weight: 600; }
    .mv-card-detail ul { padding-left: 18px; margin: 4px 0; }
    .mv-card-detail ul.mono { font-family: var(--mono); font-size: 11px; }
    .mv-meta { color: var(--txt2); font-size: 10px; font-style: italic; margin-top: 6px; }
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
</style>
