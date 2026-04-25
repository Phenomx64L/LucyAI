<script>
    // ── PdfIngestPanel — Sprint 4 Pillar 4 ───────────────────────────────────
    // Drag-drop PDF ingestion UI. Saves chunks to agent_memories + embeddings.
    // Lucy can then answer questions with <TOOL>pdf_search:query</TOOL>.

    import { invoke }               from '@tauri-apps/api/core';
    import { listen }               from '@tauri-apps/api/event';
    import { createEventDispatcher, onMount, onDestroy } from 'svelte';

    export let isEN = false;

    const dispatch = createEventDispatcher();
    const t = (es, en) => isEN ? en : es;

    // ── State ─────────────────────────────────────────────────────────────
    let docs         = [];          // PdfDocument[]
    let loading      = false;
    let dragging     = false;
    let ingesting    = false;       // true while a doc is being processed
    let progress     = null;        // PdfProgress | null
    let progressMap  = {};          // doc_id → PdfProgress (per-doc)
    let error        = null;
    let unlisten     = null;
    let confirmDel   = null;       // {id, filename} | null — drives the in-app confirm modal

    // ── Helpers ───────────────────────────────────────────────────────────
    function timeAgo(ts) {
        if (!ts) return '';
        const diff = Math.floor(Date.now() / 1000) - ts;
        if (diff < 60)   return `${diff}s`;
        if (diff < 3600) return `${Math.floor(diff / 60)}m`;
        if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
        return `${Math.floor(diff / 86400)}d`;
    }

    function phaseLabel(phase) {
        switch (phase) {
            case 'extracting': return t('Extrayendo texto…', 'Extracting text…');
            case 'saving':     return t('Guardando chunks…', 'Saving chunks…');
            case 'embedding':  return t('Generando embeddings…', 'Generating embeddings…');
            case 'done':       return t('¡Listo!', 'Done!');
            case 'error':      return t('Error', 'Error');
            default:           return phase;
        }
    }

    function pct(p) {
        if (!p || !p.total) return 0;
        return Math.round((p.current / p.total) * 100);
    }

    // ── Load docs ─────────────────────────────────────────────────────────
    async function loadDocs() {
        loading = true;
        try {
            docs = await invoke('pdf_list_docs');
        } catch (e) {
            console.debug('[PdfPanel] load error:', e);
        } finally {
            loading = false;
        }
    }

    // ── Ingest ────────────────────────────────────────────────────────────
    async function ingestFile(path) {
        if (ingesting) return;
        error   = null;
        ingesting = true;
        progress  = { doc_id: '', current: 0, total: 0,
                      phase: 'extracting', message: t('Iniciando…', 'Starting…') };
        try {
            const result = await invoke('pdf_ingest', { path });
            // Reload list once saving is done (embedding runs in background)
            await loadDocs();
        } catch (e) {
            error = String(e);
        } finally {
            ingesting = false;
        }
    }

    async function pickAndIngest() {
        try {
            const path = await invoke('pick_pdf_path');
            if (path) await ingestFile(path);
        } catch (e) {
            error = String(e);
        }
    }

    // ── Delete ────────────────────────────────────────────────────────────
    function deleteDoc(id, filename) {
        // Show the in-app confirm modal instead of the native browser confirm()
        confirmDel = { id, filename };
    }
    async function confirmDelete() {
        if (!confirmDel) return;
        const { id } = confirmDel;
        confirmDel = null;
        try {
            await invoke('pdf_delete_doc', { docId: id });
            await loadDocs();
        } catch (e) {
            error = String(e);
        }
    }
    function cancelDelete() { confirmDel = null; }

    // ── Drag & drop ───────────────────────────────────────────────────────
    function onDragOver(e) {
        e.preventDefault();
        e.stopPropagation();
        dragging = true;
    }
    function onDragLeave(e) {
        e.stopPropagation();
        dragging = false;
    }
    async function onDrop(e) {
        e.preventDefault();
        e.stopPropagation();
        dragging = false;
        const files = e.dataTransfer?.files;
        if (!files || files.length === 0) return;
        const f = files[0];
        if (!f.name.toLowerCase().endsWith('.pdf')) {
            error = t('Solo se aceptan archivos .pdf', 'Only .pdf files are accepted');
            return;
        }
        // Tauri webviews don't expose File.path → read bytes and write to a
        // temp file via the backend, then ingest by path.
        try {
            const buf  = await f.arrayBuffer();
            const b64  = arrayBufferToBase64(buf);
            const path = await invoke('save_temp_pdf', { filename: f.name, dataB64: b64 });
            await ingestFile(path);
        } catch (err) {
            error = t(`No se pudo procesar el PDF: ${err}`, `Could not process PDF: ${err}`);
        }
    }

    /** Convert an ArrayBuffer to base64 in chunks (avoid stack overflow on large PDFs). */
    function arrayBufferToBase64(buf) {
        const bytes = new Uint8Array(buf);
        const CHUNK = 0x8000;
        let binary = '';
        for (let i = 0; i < bytes.length; i += CHUNK) {
            binary += String.fromCharCode.apply(null, bytes.subarray(i, i + CHUNK));
        }
        return btoa(binary);
    }

    // ── Progress listener ─────────────────────────────────────────────────
    onMount(async () => {
        await loadDocs();
        unlisten = await listen('pdf_progress', (ev) => {
            const p = ev.payload;
            progressMap = { ...progressMap, [p.doc_id]: p };
            // Mirror latest to top-level progress while ingesting
            if (ingesting || p.phase !== 'done') progress = p;
            if (p.phase === 'done') {
                ingesting = false;
                loadDocs();
            }
        });
    });

    onDestroy(() => {
        if (unlisten) unlisten();
    });

    $: activeProgress = progress && progress.phase !== 'done' ? progress : null;
</script>

<!-- ── Panel ──────────────────────────────────────────────────────────── -->
<div class="pdf-panel">

    <!-- Header -->
    <div class="pdf-header">
        <div class="hd-left">
            <span class="hd-icon">📄</span>
            <span class="hd-title">{t('Documentos PDF', 'PDF Documents')}</span>
            {#if docs.length > 0}
                <span class="hd-badge">{docs.length}</span>
            {/if}
        </div>
        <div class="hd-actions">
            <button class="btn-ico" on:click={loadDocs}  title={t('Actualizar','Refresh')}>↺</button>
            <button class="btn-ico" on:click={pickAndIngest}
                title={t('Abrir PDF','Open PDF')} disabled={ingesting}>＋</button>
            <button class="btn-ico close-btn" on:click={() => dispatch('close')}>✕</button>
        </div>
    </div>

    <!-- Drop zone -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
        class="drop-zone"
        class:drag-active={dragging}
        class:is-ingesting={ingesting}
        on:dragover={onDragOver}
        on:dragleave={onDragLeave}
        on:drop={onDrop}
    >
        {#if ingesting && activeProgress}
            <!-- Progress view -->
            <div class="progress-view">
                <div class="prog-phase">{phaseLabel(activeProgress.phase)}</div>
                <div class="prog-bar-wrap">
                    <div class="prog-bar" style="width:{pct(activeProgress)}%"></div>
                </div>
                <div class="prog-msg">{activeProgress.message}</div>
                {#if activeProgress.total > 0}
                    <div class="prog-pct">{pct(activeProgress)}%</div>
                {/if}
            </div>
        {:else}
            <div class="dz-inner">
                <span class="dz-icon">📥</span>
                <p class="dz-label">{t('Arrastra un PDF aquí', 'Drop a PDF here')}</p>
                <small class="dz-hint">
                    {t('o haz clic en ＋ para seleccionarlo',
                       'or click ＋ to select one')}
                </small>
            </div>
        {/if}
    </div>

    <!-- Error -->
    {#if error}
        <div class="err-bar">
            <span>⚠ {error}</span>
            <button on:click={() => error = null}>✕</button>
        </div>
    {/if}

    <!-- Docs list -->
    <div class="docs-list">
        {#if loading && docs.length === 0}
            <div class="empty">{t('Cargando…', 'Loading…')}</div>
        {:else if docs.length === 0}
            <div class="empty">
                <span class="empty-ico">📚</span>
                <p>{t('Sin documentos ingresados aún.', 'No documents ingested yet.')}</p>
                <small>
                    {t('Arrastra un PDF arriba o usa ＋ para que Lucy pueda responder preguntas sobre manuales y documentación.',
                       'Drop a PDF above or use ＋ so Lucy can answer questions from manuals and documentation.')}
                </small>
            </div>
        {:else}
            {#each docs as doc (doc.id)}
                {@const dp = progressMap[doc.id]}
                <div class="doc-row">
                    <div class="doc-ico">📄</div>
                    <div class="doc-info">
                        <div class="doc-name" title={doc.path}>{doc.filename}</div>
                        <div class="doc-meta">
                            <span class="meta-chip">{doc.chunk_count} {t('chunks','chunks')}</span>
                            <span class="meta-sep">·</span>
                            <span class="meta-time">{timeAgo(doc.ingested_at)}</span>
                            {#if doc.status === 'ingesting'}
                                <span class="meta-chip ingesting">⟳ {t('procesando…','processing…')}</span>
                            {/if}
                            {#if dp && dp.phase === 'embedding'}
                                <span class="meta-chip embedding">
                                    🔮 {t('embedding','embedding')} {pct(dp)}%
                                </span>
                            {/if}
                            {#if dp && dp.phase === 'done'}
                                <span class="meta-chip ready">✓ {t('listo','ready')}</span>
                            {/if}
                        </div>
                    </div>
                    <button class="del-btn" on:click={() => deleteDoc(doc.id, doc.filename)}
                            title={t('Eliminar documento','Delete document')}>🗑</button>
                </div>
            {/each}
        {/if}
    </div>

    <!-- Usage hint -->
    <div class="hint-bar">
        <span>💡 {t(
            'Lucy buscará en los PDFs automáticamente al responder preguntas.',
            'Lucy automatically searches PDFs when answering questions.'
        )}</span>
    </div>

    <!-- ── In-app delete confirmation (replaces native browser confirm()) ── -->
    {#if confirmDel}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="cf-overlay" on:click|self={cancelDelete}>
        <div class="cf-modal" role="dialog" aria-modal="true">
            <div class="cf-hdr">
                <span class="cf-ico">🗑</span>
                <span class="cf-title">{t('Eliminar documento', 'Delete document')}</span>
            </div>
            <div class="cf-body">
                <p class="cf-msg">
                    {t('¿Eliminar este documento y todos sus chunks?',
                       'Delete this document and all its chunks?')}
                </p>
                <code class="cf-fname">{confirmDel.filename}</code>
                <p class="cf-warn">
                    {t('Esta acción no se puede deshacer.', 'This action cannot be undone.')}
                </p>
            </div>
            <div class="cf-actions">
                <button class="cf-btn cancel" on:click={cancelDelete}>
                    {t('Cancelar', 'Cancel')}
                </button>
                <button class="cf-btn danger" on:click={confirmDelete}>
                    {t('Eliminar', 'Delete')}
                </button>
            </div>
        </div>
    </div>
    {/if}
</div>

<style>
    .pdf-panel {
        display: flex;
        flex-direction: column;
        height: 100%;
        background: var(--bg-card, #0f172a);
        border-radius: 10px;
        overflow: hidden;
        font-size: 12px;
        color: var(--text-primary, #e2e8f0);
    }

    /* Header */
    .pdf-header {
        display: flex; align-items: center; justify-content: space-between;
        padding: 10px 14px;
        background: var(--bg-header, rgba(15,23,42,0.9));
        border-bottom: 1px solid rgba(255,255,255,0.06);
        gap: 8px; flex-shrink: 0;
    }
    .hd-left  { display: flex; align-items: center; gap: 8px; }
    .hd-actions { display: flex; align-items: center; gap: 4px; }
    .hd-icon  { font-size: 15px; }
    .hd-title { font-weight: 600; font-size: 13px; }
    .hd-badge {
        font-size: 10px; padding: 2px 7px;
        border-radius: 9px; font-weight: 700;
        background: rgba(99,102,241,0.2); color: #818cf8;
    }

    /* Drop zone */
    .drop-zone {
        margin: 10px 12px;
        border: 2px dashed rgba(255,255,255,0.1);
        border-radius: 8px;
        min-height: 90px;
        display: flex; align-items: center; justify-content: center;
        transition: border-color 0.2s, background 0.2s;
        flex-shrink: 0;
        cursor: pointer;
    }
    .drop-zone:hover  { border-color: rgba(99,102,241,0.4); }
    .drag-active      { border-color: #818cf8; background: rgba(99,102,241,0.08); }
    .is-ingesting     { border-color: rgba(251,191,36,0.4); cursor: default; }

    .dz-inner  { text-align: center; padding: 12px; }
    .dz-icon   { font-size: 22px; display: block; margin-bottom: 4px; }
    .dz-label  { margin: 0; font-size: 12px; color: var(--text-primary, #e2e8f0); }
    .dz-hint   { font-size: 10px; color: var(--text-muted, #64748b); }

    /* Progress view inside drop-zone */
    .progress-view {
        width: 100%; padding: 12px 16px; text-align: center;
    }
    .prog-phase {
        font-weight: 600; font-size: 11px; color: #fbbf24; margin-bottom: 6px;
        text-transform: uppercase; letter-spacing: 0.06em;
    }
    .prog-bar-wrap {
        height: 5px; background: rgba(255,255,255,0.07);
        border-radius: 3px; overflow: hidden; margin-bottom: 6px;
    }
    .prog-bar {
        height: 100%; background: linear-gradient(90deg,#6366f1,#818cf8);
        border-radius: 3px; transition: width 0.3s ease;
    }
    .prog-msg  { font-size: 10px; color: var(--text-muted,#64748b); }
    .prog-pct  { font-size: 11px; font-weight: 700; color: #818cf8; margin-top: 2px; }

    /* Error bar */
    .err-bar {
        display: flex; align-items: center; justify-content: space-between;
        margin: 0 12px 8px;
        padding: 6px 10px;
        background: rgba(239,68,68,0.12); border: 1px solid rgba(239,68,68,0.25);
        border-radius: 6px; color: #f87171; font-size: 11px;
        flex-shrink: 0;
    }
    .err-bar button {
        background: none; border: none; color: #f87171; cursor: pointer;
        font-size: 12px; padding: 0 2px; line-height: 1;
    }

    /* Docs list */
    .docs-list { flex: 1; overflow-y: auto; padding: 4px 0; }
    .empty {
        display: flex; flex-direction: column; align-items: center;
        padding: 20px 18px; gap: 6px;
        color: var(--text-muted,#64748b); text-align: center;
    }
    .empty-ico { font-size: 1.8rem; }
    .empty p   { margin: 0; font-size: 12px; }
    .empty small { font-size: 10px; opacity: 0.75; max-width: 260px; }

    .doc-row {
        display: flex; align-items: center; gap: 8px;
        padding: 8px 14px;
        border-bottom: 1px solid rgba(255,255,255,0.04);
        transition: background 0.1s;
    }
    .doc-row:hover { background: rgba(255,255,255,0.02); }
    .doc-ico  { font-size: 16px; flex-shrink: 0; }
    .doc-info { flex: 1; min-width: 0; }
    .doc-name {
        font-weight: 600; font-size: 12px;
        white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
        color: var(--text-primary,#e2e8f0);
    }
    .doc-meta {
        display: flex; align-items: center; gap: 5px;
        flex-wrap: wrap; margin-top: 2px;
    }
    .meta-chip {
        font-size: 9px; padding: 1px 6px; border-radius: 8px;
        background: rgba(255,255,255,0.06); color: var(--text-muted,#64748b);
        font-weight: 600;
    }
    .meta-chip.ingesting { background: rgba(251,191,36,0.15); color: #fbbf24; }
    .meta-chip.embedding { background: rgba(99,102,241,0.15); color: #818cf8; }
    .meta-chip.ready     { background: rgba(52,211,153,0.12); color: #34d399; }
    .meta-sep  { color: var(--text-muted,#64748b); font-size: 9px; }
    .meta-time { font-size: 9px; color: var(--text-muted,#64748b); }

    .del-btn {
        background: none; border: none; cursor: pointer;
        color: var(--text-muted,#64748b); font-size: 13px;
        padding: 3px 5px; border-radius: 4px; flex-shrink: 0;
        transition: color 0.15s, background 0.15s;
    }
    .del-btn:hover { color: #f87171; background: rgba(239,68,68,0.1); }

    /* Hint */
    .hint-bar {
        padding: 7px 14px;
        background: rgba(255,255,255,0.02);
        border-top: 1px solid rgba(255,255,255,0.04);
        font-size: 10px; color: var(--text-muted,#64748b);
        flex-shrink: 0;
    }

    /* Buttons */
    .btn-ico {
        background: none; border: none; cursor: pointer;
        color: var(--text-muted,#64748b); font-size: 14px;
        padding: 3px 5px; border-radius: 4px;
        transition: color 0.15s, background 0.15s;
    }
    .btn-ico:hover:not(:disabled) { color: var(--text-primary,#e2e8f0); background: rgba(255,255,255,0.06); }
    .btn-ico:disabled { opacity: 0.4; cursor: default; }
    .close-btn:hover { color: #f87171; }

    /* Scrollbar */
    .docs-list::-webkit-scrollbar { width: 4px; }
    .docs-list::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.1); border-radius: 2px; }

    /* In-app confirm modal (no browser confirm()) */
    .cf-overlay {
        position: absolute; inset: 0;
        background: rgba(6,10,15,0.78); backdrop-filter: blur(4px);
        display: flex; align-items: center; justify-content: center;
        z-index: 50; animation: cf-fade .15s ease;
    }
    @keyframes cf-fade { from { opacity: 0 } to { opacity: 1 } }
    .cf-modal {
        width: min(360px, 88%);
        background: #0f172a; color: #e2e8f0;
        border: 1px solid rgba(239,68,68,0.35);
        border-radius: 10px; overflow: hidden;
        box-shadow: 0 16px 40px rgba(0,0,0,0.55);
    }
    .cf-hdr {
        display: flex; align-items: center; gap: 8px;
        padding: 10px 14px;
        background: rgba(239,68,68,0.10);
        border-bottom: 1px solid rgba(239,68,68,0.20);
    }
    .cf-ico   { font-size: 16px; }
    .cf-title { font-weight: 700; font-size: 12px; color: #f87171; letter-spacing: .3px; text-transform: uppercase; }
    .cf-body  { padding: 14px; display: flex; flex-direction: column; gap: 8px; }
    .cf-msg   { margin: 0; font-size: 12px; color: #e2e8f0; }
    .cf-fname {
        display: block; font-family: var(--mono, ui-monospace, monospace);
        font-size: 11px; color: #cbd5e1;
        background: rgba(255,255,255,0.04);
        border: 1px solid rgba(255,255,255,0.06);
        border-radius: 5px; padding: 6px 8px;
        white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }
    .cf-warn  { margin: 0; font-size: 10px; color: #94a3b8; }
    .cf-actions {
        display: flex; justify-content: flex-end; gap: 8px;
        padding: 10px 14px;
        border-top: 1px solid rgba(255,255,255,0.05);
    }
    .cf-btn {
        padding: 6px 14px; border-radius: 6px; font-size: 12px;
        font-weight: 600; cursor: pointer; border: 1px solid;
        transition: background .15s, color .15s;
    }
    .cf-btn.cancel {
        background: rgba(255,255,255,0.04);
        border-color: rgba(255,255,255,0.08);
        color: #cbd5e1;
    }
    .cf-btn.cancel:hover { background: rgba(255,255,255,0.08); color: #fff; }
    .cf-btn.danger {
        background: rgba(239,68,68,0.15);
        border-color: rgba(239,68,68,0.40);
        color: #f87171;
    }
    .cf-btn.danger:hover { background: rgba(239,68,68,0.28); color: #fecaca; }
</style>
