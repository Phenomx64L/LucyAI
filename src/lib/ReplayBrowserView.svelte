<!--
  ReplayBrowserView.svelte — Tier S #1 (Deterministic Replay Mode)

  Full-screen overlay that lists every captured LLM turn and lets the user:
    • Inspect the EXACT input that produced any past Lucy answer
    • Re-run the snapshot through the SAME or a DIFFERENT model
    • Compare the original vs. replay output via a drift score
    • Relabel snapshots ("turno donde Lucy alucinó X")
    • Prune old snapshots

  Why this exists: every conventional AI tool (Cursor, Cline, Hermes,
  OpenInterpreter) loses the exact prompt → answer pairing the moment the
  chat scrolls past. Lucy persists the COMPLETE turn so the user can
  reproduce, audit, or experiment with prompt engineering against
  historical input.
-->
<script lang="ts">
    import { onMount, onDestroy, createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';

    export let isEN: boolean = false;
    /** Optional — when present, only snapshots from this tab show by default */
    export let initialTabId: string | null = null;

    const dispatch = createEventDispatcher<{ close: void }>();

    // ── Backend types ──────────────────────────────────────────────────
    interface ReplayMeta {
        id: number;
        created_at: number;
        label: string;
        tab_id: string;
        model: string;
        effort: string;
        prompt_preview: string;
        original_tokens_in: number;
        original_tokens_out: number;
        original_latency_ms: number;
        replays_run: number;
    }
    interface ReplaySnapshot extends ReplayMeta {
        task_id: string;
        system_prompt: string;
        user_prompt: string;
        context_block: string;
        images_b64: string;
        original_response: string;
        temperature: number;
        seed: number | null;
    }
    interface DriftScore {
        char_jaccard: number;
        length_delta_pct: number;
        is_identical: boolean;
    }

    let snapshots: ReplayMeta[] = [];
    let loading = false;
    let error = '';
    let scope: 'all' | 'tab' = initialTabId ? 'tab' : 'all';

    let selected: ReplaySnapshot | null = null;
    let replayOutput: string | null = null;
    let replayBusy = false;
    let replayLatency = 0;
    let drift: DriftScore | null = null;
    let overrideModel = '';

    // ── Load list ──────────────────────────────────────────────────────
    async function loadList(): Promise<void> {
        loading = true;
        error = '';
        try {
            const tabId = scope === 'tab' ? (initialTabId || null) : null;
            snapshots = await invoke<ReplayMeta[]>('replay_list',
                { limit: 100, tabId });
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    }
    $: if (scope) { loadList(); }

    async function openSnapshot(meta: ReplayMeta): Promise<void> {
        try {
            const s = await invoke<ReplaySnapshot | null>('replay_get', { id: meta.id });
            if (s) {
                selected = s;
                replayOutput = null;
                drift = null;
                overrideModel = ''; // default = original model
            }
        } catch (e) {
            error = String(e);
        }
    }

    async function runReplay(): Promise<void> {
        if (!selected) return;
        replayBusy = true;
        replayOutput = null;
        drift = null;
        const t0 = performance.now();
        try {
            const modelForRun = overrideModel.trim() ||
                (selected.effort ? `${selected.model}::${selected.effort}` : selected.model);
            // Use ask_lucy (non-streaming) for replay — we want the final
            // text in one call to compare deterministically against the
            // original. Streaming output ordering can subtly differ between
            // runs even with temperature=0.
            const result = await invoke<string>('ask_lucy', {
                prompt: selected.user_prompt,
                context: selected.context_block,
                userName: '',                // server uses lucyConfig.name internally — empty is fine
                runbooksDir: null,
                model: modelForRun,
                lang: 'es-MX',
                hostsJson: null,
                images: selected.images_b64 && selected.images_b64 !== '[]'
                    ? JSON.parse(selected.images_b64) : null,
                maxTokensOverride: null,
            });
            replayOutput = String(result || '');
            replayLatency = Math.round(performance.now() - t0);
            // Compute drift via backend (shingle Jaccard is identical Rust↔TS
            // logic, but we keep one source of truth in the backend tests).
            drift = await invoke<DriftScore>('replay_drift', {
                original: selected.original_response,
                replay: replayOutput,
            });
            // Bump replays_run so the list shows "ran N×"
            invoke('replay_bump_count', { id: selected.id }).catch(() => {});
            // Refresh list metadata in the background
            loadList();
        } catch (e) {
            error = String(e);
        } finally {
            replayBusy = false;
        }
    }

    async function relabel(meta: ReplayMeta): Promise<void> {
        const next = prompt(
            isEN ? 'New label for this snapshot:' : 'Nueva etiqueta:',
            meta.label
        );
        if (next == null) return;
        try {
            await invoke('replay_relabel', { id: meta.id, label: next });
            await loadList();
            if (selected?.id === meta.id) selected = { ...selected, label: next };
        } catch (e) { error = String(e); }
    }

    async function deleteSnapshot(meta: ReplayMeta): Promise<void> {
        if (!confirm(isEN
            ? `Delete snapshot #${meta.id}? This cannot be undone.`
            : `¿Borrar snapshot #${meta.id}? Esta acción es irreversible.`)) return;
        try {
            await invoke('replay_delete', { id: meta.id });
            if (selected?.id === meta.id) selected = null;
            await loadList();
        } catch (e) { error = String(e); }
    }

    async function pruneOld(): Promise<void> {
        if (!confirm(isEN
            ? 'Delete snapshots older than 30 days?'
            : '¿Borrar snapshots con más de 30 días?')) return;
        try {
            const n = await invoke<number>('replay_clear_old', { days: 30 });
            error = (isEN ? 'Deleted ' : 'Borrados: ') + n;
            await loadList();
        } catch (e) { error = String(e); }
    }

    // ── Formatting helpers ────────────────────────────────────────────
    function fmtDate(ts: number): string {
        return new Date(ts * 1000).toLocaleString();
    }
    function driftClass(d: DriftScore): string {
        if (d.is_identical) return 'd-identical';
        if (d.char_jaccard >= 0.85) return 'd-near';
        if (d.char_jaccard >= 0.50) return 'd-moderate';
        return 'd-far';
    }
    function driftLabel(d: DriftScore): string {
        if (d.is_identical) return isEN ? 'IDENTICAL' : 'IDÉNTICO';
        if (d.char_jaccard >= 0.85) return isEN ? 'near-identical' : 'casi idéntico';
        if (d.char_jaccard >= 0.50) return isEN ? 'moderate drift' : 'deriva moderada';
        return isEN ? 'high drift' : 'deriva alta';
    }

    function onKeyDown(ev: KeyboardEvent): void {
        if (ev.key === 'Escape') {
            if (selected) selected = null;
            else dispatch('close');
        }
    }

    onMount(() => {
        window.addEventListener('keydown', onKeyDown);
        loadList();
    });
    onDestroy(() => window.removeEventListener('keydown', onKeyDown));
</script>

<div class="rp-overlay" role="dialog" aria-label={isEN ? 'Replay browser' : 'Navegador de replays'}>
    <div class="rp-header">
        <div class="rp-title">
            <span class="rp-glyph">⌕</span>
            <span>{isEN ? 'Replay browser' : 'Navegador de replays'}</span>
            <span class="rp-count">{snapshots.length} {isEN ? 'snapshots' : 'snapshots'}</span>
        </div>
        <div class="rp-actions">
            <span class="rp-scope-toggle" role="tablist">
                <button class:active={scope === 'all'} on:click={() => scope = 'all'}>
                    {isEN ? 'All' : 'Todos'}
                </button>
                <button class:active={scope === 'tab'} on:click={() => scope = 'tab'}
                        disabled={!initialTabId}>
                    {isEN ? 'This tab' : 'Esta tab'}
                </button>
            </span>
            <button class="rp-btn" on:click={loadList} title={isEN ? 'Refresh' : 'Recargar'} disabled={loading}>↻</button>
            <button class="rp-btn" on:click={pruneOld} title={isEN ? 'Prune older than 30d' : 'Borrar > 30 días'}>🗑 30d+</button>
            <button class="rp-btn rp-close" on:click={() => dispatch('close')} title="Esc">✕</button>
        </div>
    </div>

    <div class="rp-body">
        <!-- LEFT — list of snapshots -->
        <aside class="rp-list-pane">
            {#if loading && snapshots.length === 0}
                <div class="rp-empty">{isEN ? 'Loading…' : 'Cargando…'}</div>
            {:else if error}
                <div class="rp-empty rp-err">{error}</div>
            {:else if snapshots.length === 0}
                <div class="rp-empty">
                    <div style="font-size:14px;margin-bottom:6px;">⌕</div>
                    {isEN
                        ? 'No snapshots yet. As you chat, every turn is captured automatically.'
                        : 'Sin snapshots todavía. A medida que conversas, cada turno se captura automáticamente.'}
                </div>
            {:else}
                <ul class="rp-list">
                    {#each snapshots as m (m.id)}
                        <li class="rp-row"
                            class:selected={selected?.id === m.id}>
                            <button class="rp-row-btn" on:click={() => openSnapshot(m)}>
                                <div class="rp-row-head">
                                    <span class="rp-row-model">{m.model}{m.effort ? '::' + m.effort : ''}</span>
                                    <span class="rp-row-time">{fmtDate(m.created_at)}</span>
                                    {#if m.replays_run > 0}
                                        <span class="rp-row-runs" title={isEN ? 'Replayed count' : 'Veces re-ejecutado'}>↻ {m.replays_run}</span>
                                    {/if}
                                </div>
                                {#if m.label}<div class="rp-row-label">⚑ {m.label}</div>{/if}
                                <div class="rp-row-prompt">{m.prompt_preview}{m.prompt_preview.length >= 160 ? '…' : ''}</div>
                                <div class="rp-row-meta">
                                    <span>↑ {m.original_tokens_in || '—'}</span>
                                    <span>↓ {m.original_tokens_out || '—'}</span>
                                    <span>{m.original_latency_ms}ms</span>
                                </div>
                            </button>
                            <div class="rp-row-actions">
                                <button on:click|stopPropagation={() => relabel(m)} title={isEN ? 'Label' : 'Etiquetar'}>⚑</button>
                                <button on:click|stopPropagation={() => deleteSnapshot(m)} title={isEN ? 'Delete' : 'Borrar'}>✕</button>
                            </div>
                        </li>
                    {/each}
                </ul>
            {/if}
        </aside>

        <!-- RIGHT — detail + replay panel -->
        <section class="rp-detail-pane">
            {#if !selected}
                <div class="rp-empty">
                    {isEN
                        ? 'Select a snapshot on the left to inspect or replay it.'
                        : 'Selecciona un snapshot a la izquierda para inspeccionar o re-ejecutar.'}
                </div>
            {:else}
                {@const s = selected}
                <header class="rp-detail-hdr">
                    <div class="rp-detail-meta">
                        <span class="rp-tag">{s.model}{s.effort ? '::' + s.effort : ''}</span>
                        <span class="rp-tag-muted">#{s.id} · {fmtDate(s.created_at)}</span>
                        <span class="rp-tag-muted">↑ {s.original_tokens_in || '—'} ↓ {s.original_tokens_out || '—'} · {s.original_latency_ms}ms</span>
                    </div>
                    {#if s.label}<div class="rp-detail-label">⚑ {s.label}</div>{/if}
                </header>

                <!-- Input pane -->
                <details class="rp-section" open>
                    <summary>{isEN ? 'User prompt' : 'Prompt del usuario'} <span class="rp-bytes">{s.user_prompt.length} chars</span></summary>
                    <pre class="rp-code">{s.user_prompt}</pre>
                </details>

                {#if s.context_block}
                <details class="rp-section">
                    <summary>{isEN ? 'Context block (memories / history / files)' : 'Bloque de contexto (memorias / historial / archivos)'} <span class="rp-bytes">{s.context_block.length} chars</span></summary>
                    <pre class="rp-code">{s.context_block}</pre>
                </details>
                {/if}

                <!-- Replay control row -->
                <div class="rp-replay-bar">
                    <label class="rp-replay-label">
                        {isEN ? 'Model for replay' : 'Modelo para replay'}:
                        <input type="text"
                               bind:value={overrideModel}
                               placeholder="{s.model}{s.effort ? '::' + s.effort : ''} ({isEN ? 'leave empty = same' : 'vacío = mismo'})"
                               disabled={replayBusy}/>
                    </label>
                    <button class="rp-btn rp-btn-primary"
                            on:click={runReplay}
                            disabled={replayBusy}>
                        {replayBusy ? (isEN ? '⟳ Running…' : '⟳ Ejecutando…') : (isEN ? '▶ Run replay' : '▶ Re-ejecutar')}
                    </button>
                </div>

                <!-- Side-by-side comparison -->
                <div class="rp-compare">
                    <div class="rp-pane">
                        <div class="rp-pane-hdr">{isEN ? 'Original' : 'Original'} <span class="rp-tag-muted">{s.original_latency_ms}ms</span></div>
                        <pre class="rp-code">{s.original_response}</pre>
                    </div>
                    <div class="rp-pane">
                        <div class="rp-pane-hdr">
                            {isEN ? 'Replay' : 'Re-ejecución'}
                            {#if drift}
                                <span class="rp-drift {driftClass(drift)}">
                                    {driftLabel(drift)}
                                    · jaccard {drift.char_jaccard.toFixed(3)}
                                    · Δlen {(drift.length_delta_pct * 100).toFixed(1)}%
                                </span>
                            {/if}
                            {#if replayOutput !== null}
                                <span class="rp-tag-muted">{replayLatency}ms</span>
                            {/if}
                        </div>
                        {#if replayOutput === null}
                            <div class="rp-empty">{isEN ? 'No replay yet.' : 'Sin re-ejecución todavía.'}</div>
                        {:else}
                            <pre class="rp-code">{replayOutput}</pre>
                        {/if}
                    </div>
                </div>
            {/if}
        </section>
    </div>
</div>

<style>
    .rp-overlay {
        position: fixed; inset: 0;
        background: rgba(8, 10, 18, 0.97);
        display: flex; flex-direction: column;
        z-index: 9000;
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace);
        color: var(--text-main, #cbd5e1);
        font-size: 11px;
    }
    .rp-header {
        display: flex; align-items: center; justify-content: space-between;
        padding: 10px 16px;
        background: rgba(15, 19, 31, 0.96);
        border-bottom: 1px solid rgba(255,255,255,0.06);
    }
    .rp-title { display: flex; align-items: center; gap: 10px; font-size: 13px; font-weight: 600; letter-spacing: 0.5px; }
    .rp-glyph { color: var(--accent, #10b981); font-size: 16px; }
    .rp-count { font-size: 10px; color: var(--text-muted, #94a3b8); font-weight: 400; }
    .rp-actions { display: flex; align-items: center; gap: 8px; }
    .rp-btn {
        background: rgba(255,255,255,0.04); border: 1px solid rgba(255,255,255,0.06);
        color: var(--text-muted); font: inherit; font-size: 11px;
        padding: 4px 10px; border-radius: 5px; cursor: pointer;
    }
    .rp-btn:hover:not(:disabled) { background: rgba(255,255,255,0.07); color: var(--text-main); }
    .rp-btn:disabled { opacity: 0.45; cursor: default; }
    .rp-btn-primary { background: rgba(16,185,129,0.18); color: var(--accent); border-color: rgba(16,185,129,0.3); }
    .rp-btn-primary:hover:not(:disabled) { background: rgba(16,185,129,0.28); }
    .rp-close:hover { background: rgba(239,68,68,0.15); color: #ef4444; }

    .rp-scope-toggle { display: inline-flex; padding: 2px;
        background: rgba(255,255,255,0.04); border-radius: 5px; }
    .rp-scope-toggle button {
        background: transparent; border: 0; color: var(--text-muted);
        font: inherit; font-size: 10px; padding: 3px 10px; border-radius: 3px;
        cursor: pointer;
    }
    .rp-scope-toggle button:hover:not(:disabled) { color: var(--text-main); }
    .rp-scope-toggle button.active { background: rgba(16,185,129,0.18); color: var(--accent); }
    .rp-scope-toggle button:disabled { opacity: 0.45; cursor: not-allowed; }

    .rp-body { flex: 1; display: grid; grid-template-columns: 360px 1fr; overflow: hidden; }
    .rp-list-pane {
        border-right: 1px solid rgba(255,255,255,0.06);
        overflow-y: auto;
        padding: 4px 0;
    }
    .rp-list { list-style: none; margin: 0; padding: 0; }
    .rp-row {
        position: relative;
        border-bottom: 1px solid rgba(255,255,255,0.03);
        transition: background .12s;
    }
    .rp-row:hover { background: rgba(255,255,255,0.02); }
    .rp-row.selected { background: rgba(16,185,129,0.08); }
    .rp-row-btn {
        width: 100%; background: transparent; border: 0;
        text-align: left; cursor: pointer;
        padding: 8px 12px;
        color: var(--text-main); font: inherit;
    }
    .rp-row-head { display: flex; align-items: baseline; gap: 8px; font-size: 10px; }
    .rp-row-model { color: var(--accent); font-weight: 600; }
    .rp-row-time { color: var(--text-muted); margin-left: auto; }
    .rp-row-runs { color: #f59e0b; font-size: 9px; }
    .rp-row-label { font-size: 11px; color: #f472b6; margin: 2px 0; }
    .rp-row-prompt {
        font-size: 11px; color: var(--text-muted);
        white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
        margin: 3px 0;
    }
    .rp-row-meta { display: flex; gap: 10px; font-size: 9px; color: var(--text-muted); }
    .rp-row-actions {
        position: absolute; right: 8px; top: 8px;
        display: flex; gap: 2px; opacity: 0;
        transition: opacity .12s;
    }
    .rp-row:hover .rp-row-actions { opacity: 1; }
    .rp-row-actions button {
        background: rgba(0,0,0,0.4); border: 0; color: var(--text-muted);
        font-size: 11px; padding: 3px 6px; border-radius: 3px; cursor: pointer;
    }
    .rp-row-actions button:hover { color: var(--text-main); background: rgba(255,255,255,0.1); }

    .rp-detail-pane { overflow-y: auto; padding: 12px 18px; }
    .rp-detail-hdr { margin-bottom: 14px; }
    .rp-detail-meta { display: flex; align-items: baseline; gap: 12px; font-size: 11px; }
    .rp-tag { background: rgba(16,185,129,0.15); color: var(--accent);
              padding: 2px 8px; border-radius: 4px; font-weight: 600; }
    .rp-tag-muted { color: var(--text-muted); font-size: 10px; }
    .rp-detail-label { color: #f472b6; font-size: 12px; margin-top: 6px; }

    .rp-section { margin: 10px 0; }
    .rp-section summary {
        cursor: pointer; padding: 6px 10px;
        background: rgba(255,255,255,0.03); border-radius: 4px;
        font-size: 11px; color: var(--text-main);
        user-select: none;
    }
    .rp-section summary:hover { background: rgba(255,255,255,0.05); }
    .rp-bytes { color: var(--text-muted); font-size: 9px; margin-left: 8px; }

    .rp-code {
        margin: 4px 0;
        padding: 10px 12px;
        background: rgba(0,0,0,0.30);
        border: 1px solid rgba(255,255,255,0.04);
        border-radius: 4px;
        white-space: pre-wrap; word-break: break-word;
        max-height: 360px; overflow-y: auto;
        font-size: 11px; line-height: 1.5;
        color: #d1d5db;
    }

    .rp-replay-bar {
        display: flex; align-items: center; gap: 12px;
        padding: 8px 0;
        margin: 14px 0 6px;
        border-top: 1px solid rgba(255,255,255,0.04);
    }
    .rp-replay-label {
        flex: 1; display: inline-flex; align-items: center; gap: 8px;
        font-size: 10px; color: var(--text-muted);
    }
    .rp-replay-label input {
        flex: 1; background: rgba(0,0,0,0.30);
        border: 1px solid rgba(255,255,255,0.06); border-radius: 4px;
        padding: 4px 8px; color: var(--text-main);
        font: inherit; font-size: 11px;
    }

    .rp-compare { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-top: 10px; }
    .rp-pane { display: flex; flex-direction: column; }
    .rp-pane-hdr {
        display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap;
        font-size: 10px; color: var(--text-muted);
        text-transform: uppercase; letter-spacing: 0.4px;
        padding-bottom: 4px;
    }

    .rp-drift {
        padding: 2px 8px; border-radius: 8px;
        font-size: 9px; font-weight: 700; letter-spacing: 0.3px;
        text-transform: uppercase;
    }
    .d-identical { background: rgba(16,185,129,0.18); color: var(--accent); }
    .d-near      { background: rgba(96,165,250,0.18); color: #60a5fa; }
    .d-moderate  { background: rgba(245,158,11,0.18); color: #f59e0b; }
    .d-far       { background: rgba(239,68,68,0.18);  color: #ef4444; }

    .rp-empty {
        padding: 30px 20px;
        text-align: center;
        font-size: 11px;
        color: var(--text-muted);
        font-style: italic;
    }
    .rp-err { color: #ef4444; font-style: normal; }
</style>
