<script>
    // ── ForksMonitorPanel — Sprint 4 Pillar 1 ────────────────────────────────
    // Shows all fork_task sub-agents: running, done, and error.
    // Reads from both the in-memory `forkedTasks` object (current session)
    // and the SQLite `fork_results` table (cross-session persistence).

    import { invoke } from '@tauri-apps/api/core';
    import { createEventDispatcher, onMount, onDestroy } from 'svelte';
    import { staggerIn } from '$lib/stagger';

    export let isEN     = false;
    export let tabId    = '';

    const dispatch = createEventDispatcher();

    let forks      = [];
    let loading    = false;
    let pollTimer  = null;
    let expandedId = null;   // which row is expanded to show full result

    const t = (es, en) => isEN ? en : es;

    // ── Load from SQLite ───────────────────────────────────────────────────
    async function loadForks() {
        loading = true;
        try {
            forks = await invoke('fork_list', { tabId: tabId || null, limit: 50 });
        } catch (e) {
            console.debug('[ForksMonitor] load error:', e);
        } finally {
            loading = false;
        }
    }

    async function clearOldForks() {
        await invoke('fork_clear', { days: 7 }).catch(console.debug);
        await loadForks();
    }

    function toggleExpand(id) {
        expandedId = expandedId === id ? null : id;
    }

    function statusIcon(s) {
        if (s === 'running') return '⟳';
        if (s === 'done')    return '✓';
        return '✗';
    }

    function statusClass(s) {
        if (s === 'running') return 'status-running';
        if (s === 'done')    return 'status-done';
        return 'status-error';
    }

    function timeAgo(ts) {
        if (!ts) return '';
        const diff = Math.floor(Date.now() / 1000) - ts;
        if (diff < 60)   return `${diff}s`;
        if (diff < 3600) return `${Math.floor(diff/60)}m`;
        return `${Math.floor(diff/3600)}h`;
    }

    function elapsedMs(created, finished) {
        if (!finished) return null;
        return ((finished - created) * 1000).toLocaleString() + ' ms';
    }

    // ── Lifecycle ─────────────────────────────────────────────────────────
    onMount(() => {
        loadForks();
        // Poll every 3s while any fork is running
        pollTimer = setInterval(() => {
            if (forks.some(f => f.status === 'running')) loadForks();
        }, 3000);
    });

    onDestroy(() => {
        if (pollTimer) clearInterval(pollTimer);
    });

    $: runningCount = forks.filter(f => f.status === 'running').length;
    $: doneCount    = forks.filter(f => f.status === 'done').length;
    $: errorCount   = forks.filter(f => f.status === 'error').length;
</script>

<!-- ── Panel Header ─────────────────────────────────────────────────── -->
<div class="forks-panel">
    <div class="forks-header">
        <div class="header-left">
            <span class="panel-icon">⇉</span>
            <span class="panel-title">{t('Sub-Agentes', 'Sub-Agents')}</span>
            {#if runningCount > 0}
                <span class="badge running">{runningCount} {t('activos', 'active')}</span>
            {/if}
            {#if errorCount > 0}
                <span class="badge error">{errorCount} {t('error', 'error')}</span>
            {/if}
        </div>
        <div class="header-actions">
            <button class="btn-icon" on:click={loadForks} title={t('Actualizar', 'Refresh')}>↺</button>
            <button class="btn-icon" on:click={clearOldForks} title={t('Limpiar completados >7d', 'Clear done >7d')}>🗑</button>
            <button class="btn-icon close-btn" on:click={() => dispatch('close')}>✕</button>
        </div>
    </div>

    <!-- ── Summary bar ────────────────────────────────────────────────── -->
    <div class="summary-bar">
        <span class="s-item running">⟳ {runningCount}</span>
        <span class="s-item done">✓ {doneCount}</span>
        <span class="s-item error">✗ {errorCount}</span>
        <span class="s-total">{t('Total', 'Total')}: {forks.length}</span>
    </div>

    <!-- ── Fork list ──────────────────────────────────────────────────── -->
    <div class="forks-list">
        {#if loading && forks.length === 0}
            <div class="empty-state">{t('Cargando...', 'Loading...')}</div>
        {:else if forks.length === 0}
            <div class="empty-state">
                <span style="font-size:1.6rem">⇉</span>
                <p>{t('No hay sub-agentes registrados.', 'No sub-agents recorded yet.')}</p>
                <small>{t('Cuando Lucy use fork_task, los resultados aparecerán aquí y persistirán entre sesiones.', 'When Lucy uses fork_task, results will appear here and persist across sessions.')}</small>
            </div>
        {:else}
            {#each forks as fork, _fi (fork.id)}
                <div class="fork-row" class:expanded={expandedId === fork.id}
                     in:staggerIn={{ index: _fi, step: 26 }}>
                    <!-- Row header -->
                    <button class="fork-row-btn" on:click={() => toggleExpand(fork.id)}>
                        <span class="f-status {statusClass(fork.status)}">{statusIcon(fork.status)}</span>
                        <span class="f-task-id">{fork.task_id}</span>
                        <span class="f-model">{fork.model}</span>
                        <span class="f-time">{timeAgo(fork.created_at)}</span>
                        {#if fork.finished_at}
                            <span class="f-elapsed">{elapsedMs(fork.created_at, fork.finished_at)}</span>
                        {/if}
                        <span class="f-expand">{expandedId === fork.id ? '▲' : '▼'}</span>
                    </button>

                    <!-- Expanded details -->
                    {#if expandedId === fork.id}
                        <div class="fork-detail">
                            <div class="detail-section">
                                <span class="section-label">{t('Instrucción', 'Instruction')}</span>
                                <pre class="detail-pre instruction">{fork.instruction}</pre>
                            </div>
                            {#if fork.status === 'done' && fork.result}
                                <div class="detail-section">
                                    <span class="section-label">{t('Resultado', 'Result')}</span>
                                    <pre class="detail-pre result">{fork.result}</pre>
                                </div>
                            {/if}
                            {#if fork.status === 'error' && fork.error_msg}
                                <div class="detail-section error-detail">
                                    <span class="section-label">{t('Error', 'Error')}</span>
                                    <pre class="detail-pre error-pre">{fork.error_msg}</pre>
                                </div>
                            {/if}
                            {#if fork.status === 'running'}
                                <div class="running-indicator">
                                    <span class="spinner">⟳</span>
                                    {t('Sub-agente en progreso...', 'Sub-agent in progress...')}
                                </div>
                            {/if}
                        </div>
                    {/if}
                </div>
            {/each}
        {/if}
    </div>
</div>

<style>
    .forks-panel {
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
    .forks-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 10px 14px;
        background: var(--bg-header, rgba(15,23,42,0.9));
        border-bottom: 1px solid rgba(255,255,255,0.06);
        gap: 8px;
    }
    .header-left  { display: flex; align-items: center; gap: 8px; }
    .header-actions { display: flex; align-items: center; gap: 4px; }
    .panel-icon   { font-size: 15px; }
    .panel-title  { font-weight: 600; font-size: 13px; }
    .badge {
        font-size: 10px; padding: 2px 7px;
        border-radius: 9px; font-weight: 700;
    }
    .badge.running { background: rgba(234,179,8,0.2); color: #fbbf24; }
    .badge.error   { background: rgba(239,68,68,0.2); color: #f87171; }

    /* Summary bar */
    .summary-bar {
        display: flex; gap: 14px; align-items: center;
        padding: 6px 14px;
        background: rgba(255,255,255,0.02);
        border-bottom: 1px solid rgba(255,255,255,0.04);
        font-size: 11px;
    }
    .s-item        { font-weight: 600; }
    .s-item.running { color: #fbbf24; }
    .s-item.done    { color: #34d399; }
    .s-item.error   { color: #f87171; }
    .s-total        { color: var(--text-muted, #64748b); margin-left: auto; }

    /* Fork list */
    .forks-list {
        flex: 1;
        overflow-y: auto;
        padding: 4px 0;
    }
    .empty-state {
        display: flex; flex-direction: column; align-items: center;
        justify-content: center; padding: 32px 20px; gap: 8px;
        color: var(--text-muted, #64748b); text-align: center;
    }
    .empty-state p     { margin: 0; font-size: 13px; }
    .empty-state small { font-size: 11px; opacity: 0.7; max-width: 260px; }

    /* Fork row */
    .fork-row {
        border-bottom: 1px solid rgba(255,255,255,0.04);
    }
    .fork-row.expanded {
        background: rgba(255,255,255,0.02);
    }
    .fork-row-btn {
        display: flex; align-items: center; gap: 8px;
        width: 100%; padding: 8px 14px;
        background: none; border: none; cursor: pointer;
        color: inherit; text-align: left;
        transition: background 0.12s;
    }
    .fork-row-btn:hover { background: rgba(255,255,255,0.03); }

    .f-status      { font-size: 13px; width: 16px; text-align: center; flex-shrink: 0; }
    .f-task-id     { font-weight: 600; font-family: monospace; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .f-model       { color: var(--text-muted, #64748b); font-size: 10px; flex-shrink: 0; }
    .f-time        { color: var(--text-muted, #64748b); font-size: 10px; flex-shrink: 0; }
    .f-elapsed     { color: #818cf8; font-size: 10px; flex-shrink: 0; }
    .f-expand      { font-size: 9px; color: var(--text-muted, #64748b); flex-shrink: 0; }

    .status-running { color: #fbbf24; }
    .status-done    { color: #34d399; }
    .status-error   { color: #f87171; }

    /* Expanded detail */
    .fork-detail { padding: 8px 14px 12px 38px; }
    .detail-section { margin-bottom: 10px; }
    .detail-section .section-label {
        display: block; font-size: 10px; font-weight: 600;
        text-transform: uppercase; letter-spacing: 0.05em;
        color: var(--text-muted, #64748b); margin-bottom: 4px;
    }
    .detail-pre {
        background: rgba(0,0,0,0.3); border-radius: 6px;
        padding: 8px 10px; font-size: 11px; font-family: monospace;
        white-space: pre-wrap; word-break: break-all;
        max-height: 200px; overflow-y: auto;
        color: var(--text-primary, #e2e8f0);
        border: 1px solid rgba(255,255,255,0.06);
        margin: 0;
    }
    .detail-pre.instruction { color: #94a3b8; }
    .detail-pre.result      { color: #a7f3d0; }
    .detail-pre.error-pre   { color: #fca5a5; }
    .error-detail .section-label { color: #f87171; }

    .running-indicator {
        display: flex; align-items: center; gap: 8px;
        color: #fbbf24; font-size: 11px; padding: 4px 0;
    }
    .spinner {
        display: inline-block;
        animation: spin 1.2s linear infinite;
        font-size: 14px;
    }
    @keyframes spin { to { transform: rotate(360deg); } }

    /* Buttons */
    .btn-icon {
        background: none; border: none; cursor: pointer;
        color: var(--text-muted, #64748b); font-size: 14px;
        padding: 3px 5px; border-radius: 4px;
        transition: color 0.15s, background 0.15s;
    }
    .btn-icon:hover { color: var(--text-primary, #e2e8f0); background: rgba(255,255,255,0.06); }
    .close-btn:hover { color: #f87171; }

    /* Scrollbar */
    .forks-list::-webkit-scrollbar { width: 4px; }
    .forks-list::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.1); border-radius: 2px; }
</style>
