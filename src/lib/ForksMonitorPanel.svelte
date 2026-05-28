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

    // ── Filter scope (Sprint 6 fix) ────────────────────────────────────────
    // Bug original: el panel filtraba SIEMPRE por tab_id activo, así que tras
    // recargar la app (cuando los IDs de tab cambian) el panel quedaba vacío
    // aunque hubiera forks históricos persistidos. Default 'all' garantiza
    // que el usuario VEA el histórico al abrir el panel. Puede acotar a la
    // tab actual con un toggle.
    let scope = 'all';  // 'all' | 'tab'

    const t = (es, en) => isEN ? en : es;

    // ── Load from SQLite ───────────────────────────────────────────────────
    async function loadForks() {
        loading = true;
        try {
            const filterTab = scope === 'tab' ? (tabId || null) : null;
            forks = await invoke('fork_list', { tabId: filterTab, limit: 100 });
        } catch (e) {
            console.debug('[ForksMonitor] load error:', e);
        } finally {
            loading = false;
        }
    }

    // Re-cargar cuando el usuario cambia el scope.
    $: if (scope) { loadForks(); }

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
        // BUG FIX: previously this only polled "while any fork is running".
        // That meant a user who opened the panel BEFORE running a task with
        // forks (empty array → some() returns false) never saw the panel
        // refresh as forks completed. The panel is only visible when the
        // user explicitly opens it, so polling every 3s is cheap:
        //   • One SELECT with LIMIT 100 (<1ms over the local SQLite pool)
        //   • The panel is closed by default → no overhead in normal use
        //   • Stops on unmount (onDestroy below)
        pollTimer = setInterval(() => { loadForks(); }, 3000);
    });

    onDestroy(() => {
        if (pollTimer) clearInterval(pollTimer);
    });

    $: runningCount = forks.filter(f => f.status === 'running').length;
    $: doneCount    = forks.filter(f => f.status === 'done').length;
    $: errorCount   = forks.filter(f => f.status === 'error').length;

    // ── Tier A #1 — Cost ledger + tree shape ──────────────────────────────
    // Sum cost across the visible window so the user sees "this session
    // burned $X on sub-agents". Tokens are summed the same way for the
    // detail tooltip.
    $: totalCostUsd = forks.reduce((s, f) => s + (f.cost_usd || 0), 0);
    $: totalTokens  = forks.reduce((s, f) => s + (f.tokens_in || 0) + (f.tokens_out || 0), 0);

    /**
     * Build a flat list of {fork, depth} for rendering. Roots come first,
     * children indented under their parent. Children orphaned by missing
     * parents still render at depth 0 so nothing disappears.
     */
    $: ledgerRows = (() => {
        const byTaskId = new Map();
        for (const f of forks) byTaskId.set(f.task_id, f);
        const childrenOf = new Map();
        for (const f of forks) {
            const p = f.parent_task_id || '';
            if (!p || !byTaskId.has(p)) continue;
            if (!childrenOf.has(p)) childrenOf.set(p, []);
            childrenOf.get(p).push(f);
        }
        const roots = forks.filter(f => !f.parent_task_id || !byTaskId.has(f.parent_task_id));
        const out = [];
        const walk = (f, depth) => {
            out.push({ fork: f, depth });
            const kids = childrenOf.get(f.task_id) || [];
            for (const k of kids) walk(k, depth + 1);
        };
        for (const r of roots) walk(r, 0);
        return out;
    })();

    function fmtCost(c) {
        if (!c) return '—';
        if (c < 0.001) return `$${c.toFixed(4)}`;
        if (c < 0.1)   return `$${c.toFixed(3)}`;
        return `$${c.toFixed(2)}`;
    }
    function fmtTokens(n) {
        if (!n) return '—';
        if (n < 1000) return String(n);
        return `${(n/1000).toFixed(1)}k`;
    }
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
        <!-- Tier A #1 — Cost ledger across visible forks -->
        {#if totalCostUsd > 0 || totalTokens > 0}
            <span class="s-cost" title={t(`Costo total de los ${forks.length} sub-agentes visibles. Incluye tokens de input + output. Estimado con ~4 chars/token.`, `Total cost across ${forks.length} visible sub-agents. Includes input + output tokens. Estimated at ~4 chars/token.`)}>
                ⛁ {fmtCost(totalCostUsd)} · {fmtTokens(totalTokens)} {t('tokens', 'tokens')}
            </span>
        {/if}
        <!-- Scope toggle — default 'all' (global history); 'tab' acota -->
        <span class="scope-toggle" role="tablist" aria-label={t('Alcance', 'Scope')}>
            <button class:active={scope === 'all'} on:click={() => scope = 'all'}
                    title={t('Mostrar todos los sub-agentes registrados', 'Show all recorded sub-agents')}>
                {t('Todos', 'All')}
            </button>
            <button class:active={scope === 'tab'} on:click={() => scope = 'tab'}
                    disabled={!tabId}
                    title={t('Solo los de esta pestaña', 'Only this tab')}>
                {t('Esta tab', 'This tab')}
            </button>
        </span>
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
            {#each ledgerRows as row, _fi (row.fork.id)}
                {@const fork = row.fork}
                <div class="fork-row" class:expanded={expandedId === fork.id}
                     class:fork-child={row.depth > 0}
                     style="padding-left: {row.depth * 16}px"
                     in:staggerIn={{ index: _fi, step: 26 }}>
                    <!-- Row header -->
                    <button class="fork-row-btn" on:click={() => toggleExpand(fork.id)}>
                        {#if row.depth > 0}<span class="f-tree" aria-hidden="true">└─</span>{/if}
                        <span class="f-status {statusClass(fork.status)}">{statusIcon(fork.status)}</span>
                        <span class="f-task-id">{fork.task_id}</span>
                        <span class="f-model">{fork.model}</span>
                        <span class="f-time">{timeAgo(fork.created_at)}</span>
                        {#if fork.cost_usd > 0 || fork.tokens_in > 0}
                            <span class="f-cost"
                                  title={t(`tokens: ${fork.tokens_in} in / ${fork.tokens_out} out`, `tokens: ${fork.tokens_in} in / ${fork.tokens_out} out`)}>
                                {fmtCost(fork.cost_usd)}
                            </span>
                        {/if}
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

    /* Scope toggle — Sprint 6 fix for "Sub-Agents never reports info" */
    .scope-toggle { display: inline-flex; gap: 2px; padding: 2px;
        background: rgba(255,255,255,0.04); border-radius: 6px; }
    .scope-toggle button {
        background: transparent; border: 0; color: var(--text-muted, #94a3b8);
        font: inherit; font-size: 10px; padding: 3px 10px; border-radius: 4px;
        cursor: pointer; transition: background .12s, color .12s;
    }
    .scope-toggle button:hover:not(:disabled) { background: rgba(255,255,255,0.05); color: var(--text-main, #cbd5e1); }
    .scope-toggle button.active { background: rgba(16,185,129,0.18); color: var(--accent, #10b981); }
    .scope-toggle button:disabled { opacity: 0.4; cursor: not-allowed; }

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
    /* Tier A #1 — Cost ledger + tree */
    .f-cost        { color: #10b981; font-size: 10px; flex-shrink: 0; font-weight: 600; }
    .f-tree        { color: var(--text-muted, #64748b); font-size: 11px; flex-shrink: 0; margin-right: 4px; }
    .fork-child    { background: rgba(255,255,255,0.015); }
    .s-cost        {
        color: #10b981;
        background: rgba(16,185,129,0.10);
        padding: 1px 8px;
        border-radius: 8px;
        font-size: 10px;
        font-weight: 600;
        margin-left: 4px;
    }

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
