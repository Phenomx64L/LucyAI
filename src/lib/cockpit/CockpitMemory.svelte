<script>
  /* ==========================================================================
     Lucy 2.0 — Memory view (cockpit)  ·  Phase F4/views. LIVE.
     Mirrors the classic MemoryBrowserView loader: `get_recent_memories` (or
     `search_agent_memories` when filtering) → AgentMemory[] { id, title,
     content, tags(JSON), importance 1-3, created_at }. Debounced backend
     search, like V1. Representative data is the FALLBACK for a plain-browser
     preview. Additive — does not touch MemoryBrowserView.svelte.

     v1.7.233 — V1 parity requested by the user:
       · Documentos tab: ingested PDFs (pdf_list_docs) + "Ingerir PDF"
         (pick_pdf_path → pdf_ingest, live pdf_progress bar) + per-doc delete
       · per-memory delete (delete_agent_memory)
       Deletes use a cockpit-native INLINE confirm (same pattern as NexShell
       host delete) — no dependency on the classic DialogHost.
     ========================================================================== */
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import Search from '@tabler/icons-svelte/icons/search';
  import Brain from '@tabler/icons-svelte/icons/brain';
  import Server from '@tabler/icons-svelte/icons/server';
  import FileText from '@tabler/icons-svelte/icons/file-text';
  import Bulb from '@tabler/icons-svelte/icons/bulb';
  import Trash from '@tabler/icons-svelte/icons/trash';
  import CockpitGraph from './CockpitGraph.svelte';
  import Diamond from '@tabler/icons-svelte/icons/diamond';
  import Book from '@tabler/icons-svelte/icons/book';
  import { getLessons, promoteLesson, dismissLesson } from '$lib/agentmemory/lessons';
  import Upload from '@tabler/icons-svelte/icons/upload';
  import Refresh from '@tabler/icons-svelte/icons/refresh';
  import Pin from '@tabler/icons-svelte/icons/pin';
  import Flag from '@tabler/icons-svelte/icons/flag';
  import ArrowUp from '@tabler/icons-svelte/icons/arrow-up';
  import Pencil from '@tabler/icons-svelte/icons/pencil';

  const IMP = { 3: { label: 'alta', cls: 'hi' }, 2: { label: 'media', cls: 'mid' }, 1: { label: 'baja', cls: 'lo' } };

  function parseTags(t) {
    if (Array.isArray(t)) return t;
    try { const a = JSON.parse(t || '[]'); return Array.isArray(a) ? a : []; } catch { return []; }
  }
  // v1.7.237 — arrays de crystal (key_outcomes/lessons/files_affected) llegan
  // como string JSON. Devuelve SIEMPRE un array legible; tolera string plano.
  function jsonList(v) {
    if (Array.isArray(v)) return v;
    if (v == null || v === '') return [];
    try { const a = JSON.parse(v); return Array.isArray(a) ? a : (a ? [String(a)] : []); }
    catch { return [String(v)]; }
  }
  function iconFor(tags) {
    const s = tags.join(' ').toLowerCase();
    if (/host|red|network|server|ssh|ip/.test(s)) return Server;
    if (/pdf|doc|file|manual|guide|informe/.test(s)) return FileText;
    if (/pref|insight|idea|lecci|lesson|regla/.test(s)) return Bulb;
    return Brain;
  }
  function relTime(sec) {
    const diff = Math.floor(Date.now() / 1000) - (sec || 0);
    if (diff < 60) return 'ahora';
    if (diff < 3600) return `hace ${Math.floor(diff / 60)} min`;
    if (diff < 86400) return `hace ${Math.floor(diff / 3600)} h`;
    return `hace ${Math.floor(diff / 86400)} d`;
  }
  const createdAt = (m) => m.created_at ?? m.createdAt ?? 0;

  // Representative fallback (browser preview / no backend).
  const SAMPLE = [
    { id: 2410, title: 'PROD-LINUX · Ethernet', content: 'PROD-LINUX: Ethernet cae por driver e2xw colgado → Restart-NetAdapter lo resuelve.', tags: ['PROD-LINUX', 'red'], importance: 3, ago: 5400 },
    { id: 2393, title: 'GoAnywhere MFT', content: 'GoAnywhere MFT 7.10.0 soporta DER, PEM, JKS, BCFKS y PKCS#12 para importar certificados.', tags: ['pdf', 'installation_guide'], importance: 2, ago: 90000 },
    { id: 2371, title: 'Preferencia de inventario', content: 'El usuario prefiere PowerShell nativo sobre wmic para inventario de red en hosts Windows.', tags: ['preferencia'], importance: 2, ago: 260000 },
    { id: 2350, title: 'Ruta de datos de Lucy', content: 'Los datos de Lucy viven en %APPDATA%\\Lucy — DB en lucy.db, logs en logs\\lucy_audit.log.', tags: ['lucy', 'rutas'], importance: 1, ago: 620000 },
  ];

  let mems = $state(null);
  let loading = $state(false);
  let live = $state(false);
  let error = $state('');
  let query = $state('');
  let _t = null;
  // v1.7.233 — real corpus totals (memory_browser_stats COUNT(*)); the cards
  // used to count over the ≤50 fetched rows and lie after a big PDF ingest.
  let realStats = $state(null);

  // ── Documentos (v1.7.233) ───────────────────────────────────────────────
  let view = $state('mems');        // 'mems' | 'docs' | 'graph' | 'crystals' | 'insights' | 'lessons'

  // ── Fase B (v1.7.234) — Crystals / Insights / Lecciones (solo lectura) ────
  // Mismos comandos que las pestañas de MemoryBrowserView; carga perezosa al
  // abrir cada pestaña. Acciones de gestión (borrar/promover) siguen en V1.
  let crystals = $state(null);
  let insights = $state(null);
  let lessons = $state(null);
  let principles = $state(null);    // v1.7.236 iter (#3) — Principios Activos (tabla principles)
  let subLoading = $state(false);
  let openCrystal = $state(null);   // id expandido
  async function loadPrinciples() {
    subLoading = true;
    try { principles = await invoke('list_principles', { scope: null }); }
    catch (e) { error = String(e); principles = principles || []; }
    finally { subLoading = false; }
  }
  async function delPrinciple(id) {
    try { await invoke('delete_principle', { id }); await loadPrinciples(); }
    catch (e) { error = String(e); }
  }
  // v1.7.237 — acciones portadas del V1 (el cockpit clonaba las vistas sin la
  // acción). Comandos ya registrados: delete_crystal / delete_insight /
  // update_principle; promoteLesson/dismissLesson viven en lessons.ts.
  async function togglePrinciple(p) {
    try { await invoke('update_principle', { id: p.id, enabled: !p.enabled }); await loadPrinciples(); }
    catch (e) { error = String(e); }
  }
  async function delCrystal(id) {
    try { await invoke('delete_crystal', { id }); await loadCrystals(); }
    catch (e) { error = String(e); }
  }
  async function delInsight(id) {
    try { await invoke('delete_insight', { id }); await loadInsights(); }
    catch (e) { error = String(e); }
  }
  async function promoteLes(l) {
    try { await promoteLesson(l); await loadLessons(); }
    catch (e) { error = String(e); }
  }
  async function dismissLes(l) {
    try {
      const ok = await dismissLesson(l);
      if (ok) await loadLessons();
      else error = 'Las lecciones de crystal no se pueden descartar por separado (viven en el crystal).';
    } catch (e) { error = String(e); }
  }
  async function loadCrystals() {
    subLoading = true;
    try { crystals = await invoke('list_crystals', { sessionId: null, project: null, limit: 50 }); }
    catch (e) { error = String(e); crystals = crystals || []; }
    finally { subLoading = false; }
  }
  async function loadInsights() {
    subLoading = true;
    try { insights = await invoke('list_insights', { limit: 50 }); }
    catch (e) { error = String(e); insights = insights || []; }
    finally { subLoading = false; }
  }
  async function loadLessons() {
    subLoading = true;
    try { lessons = await getLessons({ limit: 100 }); }
    catch (e) { error = String(e); lessons = lessons || []; }
    finally { subLoading = false; }
  }
  function openView(v) {
    view = v;
    if (v === 'docs' && !docs) loadDocs();
    else if (v === 'crystals' && !crystals) loadCrystals();
    else if (v === 'insights' && !insights) loadInsights();
    else if (v === 'lessons' && !lessons) loadLessons();
    else if (v === 'principles' && !principles) loadPrinciples();
  }
  let docs = $state(null);          // PdfDocument[] | null
  let docsLoading = $state(false);
  let ingesting = $state(false);
  let prog = $state(null);          // PdfProgress | null (live event)
  // Inline delete confirms (id pending confirmation), cockpit-native.
  let confirmMemId = $state(null);
  let confirmDocId = $state(null);
  // v1.7.237 — filtro por importancia (0=todas, 1/2/3) + edición in-situ de una
  // memoria (tags + importancia; el contenido no tiene comando de update).
  let impFilter = $state(0);
  let editMemId = $state(null);
  let editTags = $state('');
  let editImp = $state(1);
  function setImp(v) { impFilter = v; load(); }
  function startEdit(m) {
    editMemId = m.id;
    editTags = parseTags(m.tags).join(', ');
    editImp = m.importance || 1;
  }
  async function saveEdit(m) {
    try {
      if (editImp !== m.importance) await invoke('update_agent_memory_importance', { id: m.id, importance: editImp });
      const newTags = editTags.split(',').map((s) => s.trim()).filter(Boolean);
      const oldTags = parseTags(m.tags);
      if (JSON.stringify(newTags) !== JSON.stringify(oldTags)) await invoke('update_agent_memory_tags', { id: m.id, tags: newTags });
      editMemId = null;
      await load();
    } catch (e) { error = String(e); }
  }
  // v1.7.237 — procedencia (basename de archivos) + navegación desde el grafo.
  function shortFile(p) {
    const s = String(p);
    const i = Math.max(s.lastIndexOf('/'), s.lastIndexOf('\\'));
    return i >= 0 ? s.slice(i + 1) : s;
  }
  function openFromGraph(node) {
    if (!node) return;
    view = 'mems';
    impFilter = 0;
    query = node.title || '';
    load();
  }

  // v1.7.236 R5 — memorias fijadas: ids con pin=1 (📌 badge + toggle). Las
  // fijadas se inyectan SIEMPRE al contexto del agente (bloque MEMORIAS
  // FIJADAS en +page), pensadas como instrucciones del operador.
  let pinnedIds = $state(new Set());
  // v1.7.237 — objetos completos de las fijadas (get_pinned_memories ya los
  // devuelve). Se pintan en una sección "📌 Fijadas" SIEMPRE visible: una fijada
  // más vieja que las 50 recientes no aparecía en la lista, así que su botón de
  // desfijar nunca se renderizaba → acción de alto poder (se inyecta siempre)
  // sin superficie de reversa.
  let pinnedMems = $state(null);
  async function refreshPinned() {
    try {
      const p = await invoke('get_pinned_memories', { limit: 10 });
      const arr = Array.isArray(p) ? p : [];
      pinnedMems = arr;
      pinnedIds = new Set(arr.map((x) => x.id));
    } catch { /* columna nueva o DB fría — sin pines */ }
  }
  async function togglePin(id) {
    const on = !pinnedIds.has(id);
    try {
      await invoke('set_memory_pinned', { id, pinned: on });
      await refreshPinned();
    } catch (e) { error = String(e); }
  }

  async function load() {
    loading = true; error = '';
    try {
      const q = query.trim();
      let list;
      if (q) {
        list = await invoke('search_agent_memories', { query: q, limit: 50, excludeDocuments: true });
        // Filtro de importancia sobre resultados de búsqueda: client-side es
        // aceptable aquí porque la query ya acotó el conjunto.
        if (impFilter > 0) list = (Array.isArray(list) ? list : []).filter((m) => m.importance === impFilter);
      } else {
        // Sin query: el filtro corre en SQL (get_recent_memories_filtered).
        // Filtrar client-side tras el fetch de 50 ocultaría filas (V1 lo
        // aprendió: 50 recientes de imp2 → lista vacía al pedir imp1).
        list = await invoke('get_recent_memories_filtered', { limit: 50, importance: impFilter > 0 ? impFilter : null });
      }
      mems = Array.isArray(list) ? list : [];
      live = true;
      try { realStats = await invoke('memory_browser_stats'); } catch { /* fall back to page stats */ }
      await refreshPinned();
    } catch (e) {
      error = String(e);
      if (!mems) mems = SAMPLE.map((s) => ({ ...s, created_at: Math.floor(Date.now() / 1000) - s.ago }));
    } finally {
      loading = false;
    }
  }
  function onInput() { if (_t) clearTimeout(_t); _t = setTimeout(load, 300); }

  async function loadDocs() {
    docsLoading = true;
    try { docs = await invoke('pdf_list_docs'); }
    catch (e) { error = String(e); docs = docs || []; }
    finally { docsLoading = false; }
  }
  function openDocs() { view = 'docs'; if (!docs) loadDocs(); }

  async function delMem(id) {
    try {
      await invoke('delete_agent_memory', { id });
      confirmMemId = null;
      await load();
    } catch (e) { error = String(e); }
  }
  async function delDoc(id) {
    try {
      await invoke('pdf_delete_doc', { docId: id });
      confirmDocId = null;
      await loadDocs();
      load();  // stats + list (the doc's summary memory is gone too)
    } catch (e) { error = String(e); }
  }

  async function pickPdf() {
    if (ingesting) return;
    error = '';
    try {
      const path = await invoke('pick_pdf_path');
      if (!path) return;
      ingesting = true;
      prog = { phase: 'extracting', current: 0, total: 0, message: 'Iniciando…' };
      await invoke('pdf_ingest', { path });   // returns after chunks saved; embedding continues in bg
      await loadDocs();
      load();
    } catch (e) {
      error = String(e);   // includes the friendly "ya está ingerido…" idempotency message
      prog = null;
    } finally {
      ingesting = false;
    }
  }
  const pct = (p) => (p && p.total ? Math.round((p.current / p.total) * 100) : 0);
  const phaseLabel = (ph) => ({ extracting: 'Extrayendo texto…', saving: 'Guardando secciones…', embedding: 'Generando embeddings…', done: '¡Listo!', error: 'Error' }[ph] ?? ph);

  const stats = $derived.by(() => {
    const list = mems || [];
    const now = Math.floor(Date.now() / 1000);
    return [
      { n: realStats?.total ?? list.length, l: 'memorias' },
      { n: realStats?.high ?? list.filter((m) => (m.importance ?? 0) >= 3).length, l: 'alta import.' },
      { n: realStats?.tagged ?? list.filter((m) => parseTags(m.tags).length > 0).length, l: 'etiquetadas' },
      { n: realStats?.week ?? list.filter((m) => now - createdAt(m) < 7 * 86400).length, l: 'esta semana' },
    ];
  });

  onMount(() => {
    load();
    let un = null;
    (async () => {
      try {
        un = await listen('pdf_progress', (ev) => {
          prog = ev.payload;
          if (ev.payload?.phase === 'done') { loadDocs(); load(); }
        });
      } catch { /* browser preview — no Tauri events */ }
    })();
    return () => { if (un) un(); };
  });
</script>

<div class="mem">
  <div class="mem-head">
    <span class="mem-title">Explorador de memoria</span>
    {#if live}<span class="live"><span class="live-dot"></span>en vivo</span>{/if}
  </div>

  <div class="mem-stats">
    {#each stats as s}
      <div class="stat"><div class="stat-n">{s.n}</div><div class="stat-l">{s.l}</div></div>
    {/each}
    {#if realStats}
      <button class="stat stat-btn" class:on={view === 'docs'} onclick={openDocs}
              title="{realStats.pdf_chunks} secciones en {realStats.pdf_docs} documentos — clic para gestionar">
        <div class="stat-n">{realStats.pdf_docs}</div>
        <div class="stat-l">📄 documentos</div>
      </button>
    {/if}
  </div>

  <div class="mem-tabs">
    <button class="mtab" class:on={view === 'mems'} onclick={() => (view = 'mems')}>Memorias</button>
    <button class="mtab" class:on={view === 'docs'} onclick={openDocs}>Documentos</button>
    <button class="mtab" class:on={view === 'graph'} onclick={() => (view = 'graph')}>Grafo</button>
    <button class="mtab" class:on={view === 'crystals'} onclick={() => openView('crystals')}>Crystals</button>
    <button class="mtab" class:on={view === 'principles'} onclick={() => openView('principles')}>Principios</button>
    <button class="mtab" class:on={view === 'insights'} onclick={() => openView('insights')}>Insights</button>
    <button class="mtab" class:on={view === 'lessons'} onclick={() => openView('lessons')}>Lecciones</button>
    {#if view === 'docs'}
      <div class="mtab-acts">
        <button class="mem-btn" onclick={loadDocs} title="Recargar documentos"><Refresh size={14} stroke={1.75} /></button>
        <button class="mem-btn primary" disabled={ingesting} onclick={pickPdf} title="Ingerir un PDF (manual, documentación…)">
          <Upload size={14} stroke={1.75} /> Ingerir PDF
        </button>
      </div>
    {/if}
  </div>

  {#if error}<div class="mem-err">{error}</div>{/if}

  {#if view === 'graph'}
    <!-- ── Grafo (v1.7.233) — mount-on-open, se desmonta al salir ────────── -->
    <div class="mem-graph"><CockpitGraph onOpen={openFromGraph} onChanged={() => load()} /></div>
  {:else if view === 'crystals'}
    <!-- ── Crystals — digests LLM de sesiones (Fase B, solo lectura) ─────── -->
    {#if subLoading && !crystals}
      <div class="mem-empty"><Diamond size={28} stroke={1.4} /><p class="ck-shimmer">Cargando crystals…</p></div>
    {:else if crystals && crystals.length === 0}
      <div class="mem-empty"><Diamond size={28} stroke={1.4} /><p>Sin crystals todavía.</p><p class="mem-hint">Usa /crystallize en una conversación para destilar la sesión en un crystal (narrativa + resultados + lecciones).</p></div>
    {:else if crystals}
      <div class="mem-list">
        {#each crystals as c (c.id)}
          <div class="mcard clickable" role="button" tabindex="-1"
               onclick={() => (openCrystal = openCrystal === c.id ? null : c.id)}
               onkeydown={(e) => { if (e.key === 'Enter') openCrystal = openCrystal === c.id ? null : c.id; }}>
            <span class="mcard-icon"><Diamond size={17} stroke={1.75} /></span>
            <div class="mcard-main">
              <div class="mcard-top">
                <span class="mcard-id">#{c.id}</span>
                {#if c.project}<span class="mcard-name">{c.project}</span>{/if}
                <span class="mcard-tag">{Math.round((c.source_chars || 0) / 1000)}k chars</span>
                <span class="mcard-time">{relTime(c.created_at)}</span>
              </div>
              <div class="mcard-text">{c.narrative}</div>
              {#if openCrystal === c.id}
                {@const kos = jsonList(c.key_outcomes)}
                {@const les = jsonList(c.lessons)}
                {@const fas = jsonList(c.files_affected)}
                <div class="sub-detail">
                  {#if kos.length}<div><b>Resultados:</b><ul class="sub-bullets">{#each kos as k}<li>{k}</li>{/each}</ul></div>{/if}
                  {#if les.length}<div><b>Lecciones:</b><ul class="sub-bullets">{#each les as ls}<li>{ls}</li>{/each}</ul></div>{/if}
                  {#if fas.length}<div class="mono-sm"><b>Archivos:</b> {fas.join(', ')}</div>{/if}
                </div>
              {/if}
            </div>
            {#if live}
              <button class="mcard-del" title="Borrar crystal" aria-label="Borrar crystal" onclick={(e) => { e.stopPropagation(); delCrystal(c.id); }}>
                <Trash size={14} stroke={1.75} />
              </button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {:else if view === 'principles'}
    <!-- ── Principios Activos — directrices de máxima prioridad (v1.7.236) ──
         Lucy los guarda con <TOOL>principle_set> y afirmaba "guardado como P1"
         pero no había UI para verlos. Ahora sí. -->
    {#if subLoading && !principles}
      <div class="mem-empty"><Flag size={28} stroke={1.4} /><p class="ck-shimmer">Cargando principios…</p></div>
    {:else if principles && principles.length === 0}
      <div class="mem-empty"><Flag size={28} stroke={1.4} /><p>Sin principios activos.</p><p class="mem-hint">Pídele a Lucy que "guarde esta regla como principio" y aparecerá aquí como P#. Son las directrices de máxima prioridad que aplica siempre.</p></div>
    {:else if principles}
      <div class="mem-list">
        {#each principles as p (p.id)}
          <div class="mcard">
            <span class="mcard-icon"><Flag size={17} stroke={1.75} /></span>
            <div class="mcard-main">
              <div class="mcard-top">
                <span class="mcard-id">P{p.id}</span>
                <span class="mcard-name">{p.name}</span>
                {#if p.scope}<span class="mcard-tag">scope: {p.scope}</span>{/if}
                <span class="mcard-tag">prioridad {p.priority}</span>
                {#if !p.enabled}<span class="mcard-tag">desactivado</span>{/if}
              </div>
              <div class="mcard-text">{p.rule}</div>
            </div>
            {#if live}
              <button class="mcard-pin" class:on={p.enabled} title={p.enabled ? 'Desactivar principio — dejará de aplicarse' : 'Activar principio'} aria-label={p.enabled ? 'Desactivar principio' : 'Activar principio'} aria-pressed={p.enabled} onclick={() => togglePrinciple(p)}>
                <Flag size={13} stroke={1.75} />
              </button>
              <button class="mcard-del" title="Eliminar principio" aria-label="Eliminar principio" onclick={() => delPrinciple(p.id)}>
                <Trash size={14} stroke={1.75} />
              </button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {:else if view === 'insights'}
    <!-- ── Insights — meta-observaciones recurrentes (Fase B) ────────────── -->
    {#if subLoading && !insights}
      <div class="mem-empty"><Bulb size={28} stroke={1.4} /><p class="ck-shimmer">Cargando insights…</p></div>
    {:else if insights && insights.length === 0}
      <div class="mem-empty"><Bulb size={28} stroke={1.4} /><p>Sin insights consolidados aún.</p><p class="mem-hint">Lucy los genera cada 48 h reflexionando sobre patrones en sus memorias.</p></div>
    {:else if insights}
      <div class="mem-list">
        {#each insights as ins (ins.id)}
          <div class="mcard">
            <span class="mcard-icon"><Bulb size={17} stroke={1.75} /></span>
            <div class="mcard-main">
              <div class="mcard-top">
                <span class="mcard-tag" class:ok={ins.confidence >= 0.7}>{Math.round((ins.confidence || 0) * 100)}% confianza</span>
                {#if ins.reinforcements > 0}<span class="mcard-tag">×{ins.reinforcements} refuerzos</span>{/if}
                <span class="mcard-time">{relTime(ins.created_at)}</span>
              </div>
              <div class="mcard-text">{ins.content}</div>
            </div>
            {#if live}
              <button class="mcard-del" title="Borrar insight" aria-label="Borrar insight" onclick={() => delInsight(ins.id)}>
                <Trash size={14} stroke={1.75} />
              </button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {:else if view === 'lessons'}
    <!-- ── Lecciones — aprendizajes accionables (Fase B) ──────────────────── -->
    {#if subLoading && !lessons}
      <div class="mem-empty"><Book size={28} stroke={1.4} /><p class="ck-shimmer">Cargando lecciones…</p></div>
    {:else if lessons && lessons.length === 0}
      <div class="mem-empty"><Book size={28} stroke={1.4} /><p>Sin lecciones registradas.</p><p class="mem-hint">Salen de crystals y memorias marcadas como lección.</p></div>
    {:else if lessons}
      <div class="mem-list">
        {#each lessons as l (l.id)}
          <div class="mcard">
            <span class="mcard-icon"><Book size={17} stroke={1.75} /></span>
            <div class="mcard-main">
              <div class="mcard-top">
                <span class="mcard-tag">{l.source === 'crystal' ? '◆ crystal' : '🧠 memoria'}</span>
                {#each (l.tags || []).slice(0, 3) as t}<span class="mcard-tag">{t}</span>{/each}
                <span class="mcard-time">{relTime(l.createdAt)}</span>
                <span class="mcard-imp {(IMP[l.importance] ?? IMP[1]).cls}">{(IMP[l.importance] ?? IMP[1]).label}</span>
              </div>
              <div class="mcard-text">{l.text}</div>
            </div>
            {#if live}
              <button class="mcard-pin" title="Promover a memoria crítica (importancia 3)" aria-label="Promover lección" onclick={() => promoteLes(l)}>
                <ArrowUp size={14} stroke={1.75} />
              </button>
              {#if l.source === 'memory'}
                <button class="mcard-del" title="Descartar lección (borra la memoria origen)" aria-label="Descartar lección" onclick={() => dismissLes(l)}>
                  <Trash size={14} stroke={1.75} />
                </button>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {:else if view === 'docs'}
    <!-- ── Documentos ──────────────────────────────────────────────────── -->
    {#if prog && prog.phase !== 'done' && prog.phase !== 'error'}
      <div class="ing">
        <div class="ing-top">
          <span class="ck-shimmer">{phaseLabel(prog.phase)}</span>
          {#if prog.total}<span class="ing-n">{prog.current}/{prog.total}</span>{/if}
        </div>
        <div class="ing-bar"><div class="ing-fill" class:indet={!prog.total} style="width:{prog.total ? pct(prog) : 40}%"></div></div>
      </div>
    {/if}

    {#if docsLoading && !docs}
      <div class="mem-empty"><FileText size={28} stroke={1.4} /><p class="ck-shimmer">Cargando documentos…</p></div>
    {:else if docs && docs.length === 0}
      <div class="mem-empty">
        <FileText size={28} stroke={1.4} />
        <p>Sin documentos ingeridos.</p>
        <p class="mem-hint">Ingiere un PDF (manual, guía, informe) y Lucy podrá consultarlo con pdf_search — sin inundar la lista de memorias.</p>
      </div>
    {:else if docs}
      <div class="mem-list">
        {#each docs as d (d.id)}
          <div class="mcard">
            <span class="mcard-icon"><FileText size={17} stroke={1.75} /></span>
            <div class="mcard-main">
              <div class="mcard-top">
                <span class="mcard-name">{d.filename}</span>
                <span class="mcard-tag">{d.chunk_count} secciones</span>
                <span class="mcard-tag" class:warn={d.status !== 'done'}>{d.status}</span>
                {#if d.embedded_count != null}
                  <span class="mcard-tag" class:ok={d.embedded_count >= d.chunk_count} class:warn={d.embedded_count < d.chunk_count}
                        title={d.embedded_count >= d.chunk_count
                          ? 'Todas las secciones tienen embedding — búsqueda semántica (pdf_search) al 100%'
                          : 'Embeddings incompletos — pdf_search solo ve las secciones embebidas (¿Ollama estaba apagado? Reinicia Lucy para el backfill)'}>
                    {d.embedded_count}/{d.chunk_count} embebidos
                  </span>
                {/if}
                <span class="mcard-time">{relTime(d.ingested_at)}</span>
              </div>
              <div class="mcard-text mono" title={d.path}>{d.path}</div>
              {#if confirmDocId === d.id}
                <div class="confirm">
                  <span>¿Borrar el documento y sus {d.chunk_count} secciones (chunks + embeddings + resumen)?</span>
                  <button class="mem-btn danger" onclick={() => delDoc(d.id)}>Borrar</button>
                  <button class="mem-btn" onclick={() => (confirmDocId = null)}>Cancelar</button>
                </div>
              {/if}
            </div>
            <button class="mcard-del" title="Borrar documento" aria-label="Borrar documento" onclick={() => (confirmDocId = confirmDocId === d.id ? null : d.id)}>
              <Trash size={14} stroke={1.75} />
            </button>
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <!-- ── Memorias ────────────────────────────────────────────────────── -->
    <div class="mem-search">
      <Search size={16} stroke={1.75} />
      <input placeholder="Buscar memorias…" bind:value={query} oninput={onInput} />
    </div>
    <div class="mem-filter">
      <button class="mfilter" class:on={impFilter === 0} onclick={() => setImp(0)}>Todas</button>
      <button class="mfilter" class:on={impFilter === 3} onclick={() => setImp(3)}>Alta</button>
      <button class="mfilter" class:on={impFilter === 2} onclick={() => setImp(2)}>Media</button>
      <button class="mfilter" class:on={impFilter === 1} onclick={() => setImp(1)}>Baja</button>
    </div>

    {#if !live && mems}<div class="mem-note">Datos de ejemplo — sin backend disponible en esta vista previa.</div>{/if}

    {#if live && pinnedMems && pinnedMems.length}
      <div class="mem-pinned">
        <div class="mem-pinned-hdr"><Pin size={12} stroke={2} /> Fijadas — se inyectan SIEMPRE al contexto del agente</div>
        {#each pinnedMems as pm (pm.id)}
          <div class="mem-pinned-row">
            <span class="mcard-id">#{pm.id}</span>
            <span class="mem-pinned-title">{pm.title || (pm.content || '').slice(0, 90)}</span>
            <button class="mcard-pin on" title="Desfijar — dejará de inyectarse siempre" aria-label="Desfijar memoria" onclick={() => togglePin(pm.id)}>
              <Pin size={13} stroke={1.75} />
            </button>
          </div>
        {/each}
      </div>
    {/if}

    {#if !mems && loading}
      <div class="mem-empty"><Brain size={28} stroke={1.4} /><p class="ck-shimmer">Cargando memorias…</p></div>
    {:else if mems && mems.length === 0}
      <div class="mem-empty"><Brain size={28} stroke={1.4} /><p>{query ? 'Sin coincidencias para tu búsqueda.' : (impFilter ? 'No hay memorias de esta importancia.' : 'Aún no hay memorias guardadas.')}</p></div>
    {:else if mems}
      <div class="mem-list">
        {#each mems.filter((mm) => !pinnedIds.has(mm.id)) as m (m.id)}
          {@const tags = parseTags(m.tags)}
          {@const Icon = iconFor(tags)}
          {@const imp = IMP[m.importance] ?? IMP[1]}
          {@const provFiles = parseTags(m.files)}
          <div class="mcard">
            <span class="mcard-icon"><Icon size={17} stroke={1.75} /></span>
            <div class="mcard-main">
              <div class="mcard-top">
                <span class="mcard-id">#{m.id}</span>
                {#if m.title}<span class="mcard-name">{m.title}</span>{/if}
                {#each tags.slice(0, 3) as t}<span class="mcard-tag">{t}</span>{/each}
                <span class="mcard-time">{relTime(createdAt(m))}</span>
                <span class="mcard-imp {imp.cls}">{imp.label}</span>
              </div>
              <div class="mcard-text">{m.content}</div>
              {#if provFiles.length || (m.session_id && !/^\d+$/.test(String(m.session_id)))}
                <div class="mcard-prov">
                  {#if m.session_id && !/^\d+$/.test(String(m.session_id))}<span class="mcard-prov-sid" title="Sesión de origen">⌂ {String(m.session_id).slice(0, 18)}</span>{/if}
                  {#each provFiles.slice(0, 5) as f}<span class="mcard-prov-file" title={f}>📄 {shortFile(f)}</span>{/each}
                  {#if provFiles.length > 5}<span class="mcard-prov-more">+{provFiles.length - 5}</span>{/if}
                </div>
              {/if}
              {#if confirmMemId === m.id}
                <div class="confirm">
                  <span>¿Borrar la memoria #{m.id}? No se puede deshacer.</span>
                  <button class="mem-btn danger" onclick={() => delMem(m.id)}>Borrar</button>
                  <button class="mem-btn" onclick={() => (confirmMemId = null)}>Cancelar</button>
                </div>
              {/if}
              {#if editMemId === m.id}
                <div class="mem-edit">
                  <div class="mem-edit-imp">
                    <span class="mem-edit-lbl">Importancia</span>
                    <button class="mfilter" class:on={editImp === 1} onclick={() => (editImp = 1)}>baja</button>
                    <button class="mfilter" class:on={editImp === 2} onclick={() => (editImp = 2)}>media</button>
                    <button class="mfilter" class:on={editImp === 3} onclick={() => (editImp = 3)}>alta</button>
                  </div>
                  <input class="mem-edit-tags" placeholder="tags separados por coma" bind:value={editTags} />
                  <div class="mem-edit-acts">
                    <button class="mem-btn primary" onclick={() => saveEdit(m)}>Guardar</button>
                    <button class="mem-btn" onclick={() => (editMemId = null)}>Cancelar</button>
                  </div>
                </div>
              {/if}
            </div>
            {#if live}
              <button class="mcard-pin" class:on={editMemId === m.id} title="Editar tags/importancia" aria-label="Editar memoria" onclick={() => (editMemId === m.id ? (editMemId = null) : startEdit(m))}>
                <Pencil size={14} stroke={1.75} />
              </button>
              <button
                class="mcard-pin" class:on={pinnedIds.has(m.id)}
                title={pinnedIds.has(m.id) ? 'Desfijar — dejará de inyectarse siempre' : 'Fijar — se inyectará SIEMPRE al contexto del agente'}
                aria-label={pinnedIds.has(m.id) ? 'Desfijar memoria' : 'Fijar memoria'}
                aria-pressed={pinnedIds.has(m.id)}
                onclick={() => togglePin(m.id)}
              >
                <Pin size={14} stroke={1.75} />
              </button>
              <button class="mcard-del" title="Borrar memoria" aria-label="Borrar memoria" onclick={() => (confirmMemId = confirmMemId === m.id ? null : m.id)}>
                <Trash size={14} stroke={1.75} />
              </button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .mem { height: 100%; overflow-y: auto; padding: 18px 22px; }
  .mem-head { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }
  .mem-title { font-size: var(--fs-title); font-weight: var(--fw-medium); color: var(--text-primary); }
  .live { display: flex; align-items: center; gap: 6px; font-size: var(--fs-caption); color: var(--accent); }
  .live-dot { width: 7px; height: 7px; border-radius: var(--r-pill); background: var(--accent); animation: mem-pulse 1.6s var(--ease-out) infinite; }
  @keyframes mem-pulse { 0% { box-shadow: 0 0 0 0 rgba(61,214,164,0.5); } 70% { box-shadow: 0 0 0 5px rgba(61,214,164,0); } 100% { box-shadow: 0 0 0 0 rgba(61,214,164,0); } }

  /* v1.7.236 iter-2 — de "cajas gigantes con un dígito flotando" a TIRA de
     lecturas compactas: auto-fill con máximo de 190px para que las tarjetas
     midan lo que su contenido pide, no lo que el ancho de ventana regale. */
  .mem-stats { display: grid; grid-template-columns: repeat(auto-fill, minmax(130px, 190px)); gap: 10px; margin-bottom: 16px; }
  .stat { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--r-md); padding: 11px 14px; box-shadow: var(--inset-shadow); }
  /* v1.7.235 "instrumento premium" — conteo en cifras tabulares mono + label
     de instrumento: las stats se leen como lecturas, no como tarjetas. */
  .stat-n { font-family: var(--font-mono); font-variant-numeric: tabular-nums; font-size: 24px; font-weight: var(--fw-medium); color: var(--text-primary); line-height: 1; }
  .stat-l { font-family: var(--font-mono); font-size: var(--fs-micro); letter-spacing: var(--ls-label); text-transform: uppercase; color: var(--text-faint); margin-top: 7px; }
  .stat-btn { border: 1px solid transparent; cursor: pointer; text-align: left; font-family: var(--font-sans); transition: border-color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out); }
  .stat-btn:hover { border-color: var(--border-accent); }
  .stat-btn.on { border-color: var(--accent-line); background: var(--accent-bg); }
  .stat-btn.on .stat-n { color: var(--accent); }

  .mem-tabs { display: flex; align-items: center; gap: 6px; margin-bottom: 12px; }
  .mtab {
    font-size: var(--fs-footnote); color: var(--text-muted); cursor: pointer;
    background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-pill);
    padding: 5px 14px; font-family: var(--font-sans);
    transition: color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out);
  }
  .mtab:hover { color: var(--text-primary); }
  .mtab.on { color: var(--accent-ink); background: var(--accent); border-color: var(--accent); }
  .mtab-acts { margin-left: auto; display: flex; gap: 6px; }
  .mem-btn {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: var(--fs-caption); color: var(--text-secondary); cursor: pointer;
    background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-md);
    padding: 5px 11px; font-family: var(--font-sans);
    transition: color var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out);
  }
  .mem-btn:hover:not(:disabled) { color: var(--text-primary); border-color: var(--border-accent); }
  .mem-btn:disabled { opacity: 0.5; cursor: default; }
  .mem-btn.primary { color: var(--accent-ink); background: var(--accent); border-color: var(--accent); }
  .mem-btn.primary:hover:not(:disabled) { background: var(--accent-hover); color: var(--accent-ink); }
  .mem-btn.danger { color: #fff; background: var(--danger); border-color: var(--danger); }

  .mem-err { font-size: var(--fs-caption); color: var(--danger); background: rgba(240,110,110,0.08); border: 1px solid rgba(240,110,110,0.25); border-radius: var(--r-md); padding: 7px 11px; margin-bottom: 10px; white-space: pre-wrap; }
  .mem-graph { height: calc(100vh - 300px); min-height: 420px; }

  /* ── PDF ingest progress ── */
  .ing { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--r-lg); padding: 11px 14px; margin-bottom: 12px; }
  .ing-top { display: flex; align-items: center; justify-content: space-between; font-size: var(--fs-caption); color: var(--text-secondary); margin-bottom: 8px; }
  .ing-n { font-family: var(--font-mono); color: var(--text-muted); }
  .ing-bar { height: 4px; background: var(--surface-3); border-radius: var(--r-pill); overflow: hidden; }
  .ing-fill { height: 100%; background: var(--accent); border-radius: var(--r-pill); transition: width var(--dur-base) var(--ease-out); }
  .ing-fill.indet { animation: ing-indet 1.3s var(--ease-out) infinite; }
  @keyframes ing-indet { 0% { margin-left: -40%; } 100% { margin-left: 100%; } }

  .mem-search { display: flex; align-items: center; gap: 9px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-lg); padding: 8px 12px; margin-bottom: 12px; color: var(--text-muted); }
  .mem-search input { flex: 1; background: transparent; border: 0; outline: 0; color: var(--text-primary); font-size: var(--fs-body); font-family: var(--font-sans); }
  .mem-search input::placeholder { color: var(--text-faint); }

  /* v1.7.237 — filtro por importancia + editor in-situ de tags/importancia. */
  .mem-filter { display: flex; align-items: center; gap: 6px; margin-bottom: 12px; }
  .mfilter { font-size: var(--fs-caption); color: var(--text-muted); cursor: pointer; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-pill); padding: 3px 11px; font-family: var(--font-sans); transition: color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out); }
  .mfilter:hover { color: var(--text-primary); }
  .mfilter.on { color: var(--accent-ink); background: var(--accent); border-color: var(--accent); }
  .mem-edit { margin-top: 9px; padding: 10px 11px; display: flex; flex-direction: column; gap: 8px; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--r-md); }
  .mem-edit-imp { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .mem-edit-lbl { font-size: var(--fs-caption); color: var(--text-muted); margin-right: 2px; }
  .mem-edit-tags { background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-md); padding: 6px 10px; color: var(--text-primary); font-size: var(--fs-caption); font-family: var(--font-sans); outline: 0; }
  .mem-edit-tags::placeholder { color: var(--text-faint); }
  .mem-edit-acts { display: flex; gap: 6px; }
  .mem-note { font-size: var(--fs-caption); color: var(--text-faint); margin-bottom: 10px; }
  .mem-hint { max-width: 380px; }

  /* v1.7.237 — sección "📌 Fijadas": las memorias con pin siempre visibles y
     desfijables, aunque sean más viejas que las 50 recientes o no matcheen la
     búsqueda actual. */
  .mem-pinned { display: flex; flex-direction: column; gap: 6px; margin-bottom: 12px; padding: 10px 12px; background: var(--accent-bg); border: 1px solid var(--accent-line); border-radius: var(--r-lg); }
  .mem-pinned-hdr { display: flex; align-items: center; gap: 6px; font-size: var(--fs-micro); letter-spacing: var(--ls-label); text-transform: uppercase; color: var(--accent); font-family: var(--font-mono); }
  .mem-pinned-row { display: flex; align-items: center; gap: 9px; }
  .mem-pinned-title { flex: 1; min-width: 0; font-size: var(--fs-caption); color: var(--text-secondary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .mem-list { display: flex; flex-direction: column; gap: 10px; }
  .mcard { display: flex; gap: 12px; align-items: flex-start; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-lg); padding: 13px 15px; }
  .mcard-icon { color: var(--accent); flex-shrink: 0; height: 17px; display: flex; align-items: center; }
  .mcard-main { flex: 1; min-width: 0; }
  .mcard-top { display: flex; align-items: center; gap: 9px; margin-bottom: 7px; color: var(--text-muted); flex-wrap: wrap; }
  .mcard-id { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); }
  .mcard-name { font-size: var(--fs-caption); color: var(--text-secondary); font-weight: var(--fw-medium); }
  .mcard-tag { font-size: var(--fs-caption); color: var(--text-muted); background: var(--surface-3); padding: 1px 7px; border-radius: 5px; }
  .mcard-tag.warn { color: var(--warning); background: rgba(245,158,11,0.10); }
  .mcard-tag.ok { color: var(--accent); background: var(--accent-bg); }
  .mcard.clickable { cursor: pointer; transition: border-color var(--dur-fast) var(--ease-out); }
  .mcard.clickable:hover { border-color: var(--border-accent); }
  .sub-detail {
    margin-top: 9px; padding: 9px 11px; display: flex; flex-direction: column; gap: 6px;
    font-size: var(--fs-caption); line-height: var(--lh-body); color: var(--text-secondary);
    background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--r-md);
  }
  .sub-detail b { color: var(--text-primary); font-weight: var(--fw-medium); }
  .sub-detail .mono-sm { font-family: var(--font-mono); font-size: var(--fs-caption); color: var(--text-muted); overflow-wrap: anywhere; }
  .sub-bullets { margin: 3px 0 0; padding-left: 18px; display: flex; flex-direction: column; gap: 2px; }
  .sub-bullets li { list-style: disc; }
  .mcard-time { font-size: var(--fs-caption); color: var(--text-faint); margin-left: auto; }
  .mcard-imp { font-size: var(--fs-caption); padding: 1px 8px; border-radius: var(--r-pill); }
  .mcard-imp.hi { color: var(--accent); background: var(--accent-bg); }
  .mcard-imp.mid { color: var(--text-secondary); background: var(--surface-3); }
  .mcard-imp.lo { color: var(--text-faint); background: var(--surface-3); }
  .mcard-text { font-size: var(--fs-footnote); line-height: var(--lh-body); color: var(--text-secondary); }
  .mcard-text.mono { font-family: var(--font-mono); font-size: var(--fs-caption); color: var(--text-muted); overflow-wrap: anywhere; }
  /* v1.7.237 — procedencia: sesión de origen (si no es un timestamp) + archivos. */
  .mcard-prov { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 7px; }
  .mcard-prov-sid { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); background: var(--surface-3); padding: 1px 7px; border-radius: 5px; }
  .mcard-prov-file { font-size: var(--fs-caption); color: var(--text-muted); background: var(--surface-3); padding: 1px 7px; border-radius: 5px; max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mcard-prov-more { font-size: var(--fs-caption); color: var(--text-faint); align-self: center; }

  /* Delete affordance — appears on card hover (kept visible while confirming). */
  .mcard-del {
    flex-shrink: 0; display: flex; align-items: center; justify-content: center;
    width: 26px; height: 26px; padding: 0; border: 0; border-radius: var(--r-sm);
    background: transparent; color: var(--text-faint); cursor: pointer; opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out);
  }
  .mcard:hover .mcard-del, .mcard-del:focus-visible { opacity: 1; }
  .mcard-del:hover { color: var(--danger); background: rgba(240,110,110,0.10); }

  /* v1.7.236 R5 — pin del operador. Mismo patrón que .mcard-del; cuando la
     memoria está fijada el pin queda SIEMPRE visible (no solo en hover). */
  .mcard-pin {
    flex-shrink: 0; display: flex; align-items: center; justify-content: center;
    width: 26px; height: 26px; padding: 0; border: 0; border-radius: var(--r-sm);
    background: transparent; color: var(--text-faint); cursor: pointer; opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out);
  }
  .mcard:hover .mcard-pin, .mcard-pin:focus-visible { opacity: 1; }
  .mcard-pin:hover { color: var(--accent); background: var(--accent-bg); }
  .mcard-pin.on { opacity: 1; color: var(--accent); }

  .confirm {
    display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
    margin-top: 9px; padding: 8px 11px;
    font-size: var(--fs-caption); color: var(--text-secondary);
    background: rgba(240,110,110,0.06); border: 1px solid rgba(240,110,110,0.25); border-radius: var(--r-md);
  }
  .confirm span { flex: 1; min-width: 180px; }

  .mem-empty { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; color: var(--text-faint); text-align: center; padding: 48px 24px; }
  .mem-empty p { margin: 0; font-size: var(--fs-footnote); color: var(--text-muted); }
</style>
