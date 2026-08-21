<!--
  ShellRecordingPlayer.svelte — Tier S #3

  Full-screen player + browser for NexShell session recordings.

  Two modes:
    • Browser (default) — list of all recordings, click to open
    • Player — timeline scrubber, play/pause, speed control, live re-render
                of cmd/out/err/exit events into a terminal-like pane

  Why a custom player instead of asciinema:
    • asciinema is browser-side JS + a separate format. Adding it would
      pull a runtime dependency for a minor convenience win.
    • Our events are already structured (cmd / out / err / exit / meta);
      a player that consumes them directly is ~150 LOC and gives us
      tighter control over the UX (e.g. per-kind colors, scrubbing
      snaps to command boundaries, exit-code badges).
    • Lucy stays a single-binary tool — no extra JS to bundle.

  Storage shape is documented in shell_recording.rs.
-->
<script lang="ts">
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { onMount, onDestroy, createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';

    export let isEN: boolean = false;
    /** Optional preselect a recording when opened from a host card. */
    export let initialRecordingId: number | null = null;
    /** Optional filter — when present, the browser scopes to this host. */
    export let initialHostId: string | null = null;

    const dispatch = createEventDispatcher<{ close: void }>();

    // ── Backend types ──────────────────────────────────────────────────
    interface ShellRecording {
        id: number;
        session_id: string;
        host_id: string;
        host_name: string;
        host_type: string;
        title: string;
        started_at: number;
        ended_at: number | null;
        event_count: number;
        byte_count: number;
    }
    interface ShellRecordingEvent {
        id: number;
        recording_id: number;
        t_ms: number;
        kind: string;        // 'cmd' | 'out' | 'err' | 'meta' | 'exit'
        data: string;
    }

    let recordings: ShellRecording[] = [];
    let scope: 'all' | 'host' = initialHostId ? 'host' : 'all';
    let loadingList = false;
    let listError = '';

    let selected: ShellRecording | null = null;
    let events: ShellRecordingEvent[] = [];
    let loadingEvents = false;
    let totalDurationMs = 0;

    // ── Playback state ─────────────────────────────────────────────────
    let playhead = 0;          // current t_ms
    let isPlaying = false;
    let speed: 0.5 | 1 | 2 | 5 | 0 = 1; // 0 = instant (no animation)
    let rafId: number | null = null;
    let lastTick = 0;

    // The "rendered terminal" — built by replaying events up to `playhead`.
    // We cache `lastPlayhead` so we only re-replay from scratch when the
    // user scrubs BACKWARDS; forward motion only appends new events.
    let renderedLines: { kind: string; text: string }[] = [];
    let lastRendered = -1;

    // ── List loading ───────────────────────────────────────────────────
    async function loadList(): Promise<void> {
        loadingList = true; listError = '';
        try {
            const hostId = scope === 'host' ? (initialHostId || null) : null;
            recordings = await invoke<ShellRecording[]>('shell_recording_list',
                { hostId, limit: 100 });
        } catch (e) { listError = String(e); }
        finally { loadingList = false; }
    }
    $: if (scope) { loadList(); }

    async function openRecording(rec: ShellRecording): Promise<void> {
        selected = rec;
        events = [];
        renderedLines = [];
        playhead = 0;
        lastRendered = -1;
        isPlaying = false;
        loadingEvents = true;
        try {
            events = await invoke<ShellRecordingEvent[]>('shell_recording_events',
                { recordingId: rec.id });
            totalDurationMs = events.length > 0
                ? events[events.length - 1].t_ms
                : 0;
            // Initial render at t=0
            rebuildRender();
        } catch (e) {
            listError = String(e);
        } finally {
            loadingEvents = false;
        }
    }

    function closeRecording(): void {
        stopPlayback();
        selected = null;
        events = [];
        renderedLines = [];
        playhead = 0;
        lastRendered = -1;
    }

    async function deleteRecording(rec: ShellRecording): Promise<void> {
        const { lucyConfirm } = await import('$lib/dialog-service');
        if (!await lucyConfirm(
            isEN ? `Delete recording #${rec.id}?` : `¿Borrar grabación #${rec.id}?`,
            { tone: 'danger',
              description: $trad('No se puede deshacer.'),
              confirmLabel: $trad('Borrar') })) return;
        try {
            await invoke('shell_recording_delete', { recordingId: rec.id });
            if (selected?.id === rec.id) closeRecording();
            await loadList();
        } catch (e) { listError = String(e); }
    }

    async function renameRecording(rec: ShellRecording): Promise<void> {
        const { lucyPrompt } = await import('$lib/dialog-service');
        const next = await lucyPrompt($trad('Nuevo título'),
            { defaultValue: rec.title });
        if (next == null) return;
        try {
            await invoke('shell_recording_rename', {
                recordingId: rec.id, title: next,
            });
            await loadList();
            if (selected?.id === rec.id) selected = { ...selected, title: next };
        } catch (e) { listError = String(e); }
    }

    // ── Render engine ──────────────────────────────────────────────────
    /**
     * Re-walk the events array up to `playhead` and rebuild renderedLines.
     * Cheap: events count is bounded at 50_000 and most recordings are
     * 1-5k events. Could be optimized to incremental forward-append but
     * the linear pass keeps the code one-screenful.
     */
    function rebuildRender(): void {
        const out: { kind: string; text: string }[] = [];
        for (const e of events) {
            if (e.t_ms > playhead) break;
            // Each event becomes a line block. We coalesce consecutive
            // 'out' events from the SAME chunk burst (same t_ms) into
            // one block so the player feels less choppy.
            const last = out[out.length - 1];
            if (last && last.kind === e.kind && (e.kind === 'out' || e.kind === 'err')) {
                last.text += e.data;
            } else {
                out.push({ kind: e.kind, text: e.data });
            }
        }
        renderedLines = out;
        lastRendered = playhead;
    }

    /** Forward-only fast path: append events between lastRendered and playhead. */
    function appendForward(): void {
        if (lastRendered < 0) { rebuildRender(); return; }
        for (const e of events) {
            if (e.t_ms <= lastRendered) continue;
            if (e.t_ms > playhead) break;
            const last = renderedLines[renderedLines.length - 1];
            if (last && last.kind === e.kind && (e.kind === 'out' || e.kind === 'err')) {
                last.text += e.data;
            } else {
                renderedLines.push({ kind: e.kind, text: e.data });
            }
        }
        renderedLines = renderedLines; // trigger Svelte
        lastRendered = playhead;
    }

    // ── Playback loop ──────────────────────────────────────────────────
    function startPlayback(): void {
        if (!selected || isPlaying) return;
        if (playhead >= totalDurationMs) playhead = 0;
        if (speed === 0) {
            // Instant: jump to end
            playhead = totalDurationMs;
            appendForward();
            return;
        }
        isPlaying = true;
        lastTick = performance.now();
        const tick = (now: number) => {
            if (!isPlaying) return;
            const dt = now - lastTick;
            lastTick = now;
            playhead = Math.min(totalDurationMs, playhead + dt * (speed as number));
            appendForward();
            if (playhead >= totalDurationMs) {
                isPlaying = false;
                rafId = null;
                return;
            }
            rafId = requestAnimationFrame(tick);
        };
        rafId = requestAnimationFrame(tick);
    }
    function stopPlayback(): void {
        isPlaying = false;
        if (rafId != null) { cancelAnimationFrame(rafId); rafId = null; }
    }
    function scrub(toMs: number): void {
        stopPlayback();
        const next = Math.max(0, Math.min(totalDurationMs, toMs));
        const goingBack = next < playhead;
        playhead = next;
        if (goingBack) rebuildRender();
        else appendForward();
    }

    // ── Formatting helpers ─────────────────────────────────────────────
    function fmtDate(ts: number): string { return new Date(ts * 1000).toLocaleString(); }
    function fmtDuration(rec: ShellRecording): string {
        if (!rec.ended_at) return $trad('en vivo');
        const s = rec.ended_at - rec.started_at;
        if (s < 60)   return `${s}s`;
        if (s < 3600) return `${Math.floor(s/60)}m ${s%60}s`;
        return `${Math.floor(s/3600)}h ${Math.floor((s%3600)/60)}m`;
    }
    function fmtBytes(n: number): string {
        if (n < 1024)        return `${n} B`;
        if (n < 1024 * 1024) return `${(n/1024).toFixed(1)} KB`;
        return `${(n/1024/1024).toFixed(2)} MB`;
    }
    function fmtMs(ms: number): string {
        const s = Math.floor(ms / 1000);
        const m = Math.floor(s / 60);
        const ss = s % 60;
        return `${m}:${String(ss).padStart(2, '0')}`;
    }

    function eventKindLabel(k: string): string {
        if (k === 'cmd') return '$';
        if (k === 'err') return '✗';
        if (k === 'exit') return '⏚';
        if (k === 'meta') return '·';
        return '';
    }
    function eventKindClass(k: string): string {
        return `ev-${k}`;
    }

    function onKeyDown(ev: KeyboardEvent): void {
        if (ev.key === 'Escape') {
            if (selected) closeRecording();
            else dispatch('close');
        } else if (selected && (ev.key === ' ' || ev.key === 'k')) {
            ev.preventDefault();
            isPlaying ? stopPlayback() : startPlayback();
        }
    }

    onMount(() => {
        window.addEventListener('keydown', onKeyDown);
        loadList().then(async () => {
            if (initialRecordingId) {
                const r = recordings.find(x => x.id === initialRecordingId);
                if (r) await openRecording(r);
            }
        });
    });
    onDestroy(() => {
        stopPlayback();
        window.removeEventListener('keydown', onKeyDown);
    });
</script>

<div class="srp-overlay" role="dialog" aria-label={$trad('Reproductor de grabaciones')}>
    <div class="srp-header">
        <div class="srp-title">
            <span class="srp-glyph">●</span>
            <span>{$trad('Grabaciones de shell')}</span>
            <span class="srp-count">{recordings.length}</span>
        </div>
        <div class="srp-actions">
            <span class="srp-scope" role="tablist">
                <button class:active={scope === 'all'} on:click={() => scope = 'all'}>
                    {$trad('Todos')}
                </button>
                <button class:active={scope === 'host'} on:click={() => scope = 'host'}
                        disabled={!initialHostId}>
                    {$trad('Este host')}
                </button>
            </span>
            <button class="srp-btn" on:click={loadList} disabled={loadingList} title={$trad('Recargar')}>↻</button>
            <button class="srp-btn srp-close" on:click={() => dispatch('close')} title="Esc">✕</button>
        </div>
    </div>

    <div class="srp-body">
        <!-- LEFT — recordings list -->
        <aside class="srp-list-pane">
            {#if loadingList && recordings.length === 0}
                <div class="srp-empty">{$trad('Cargando…')}</div>
            {:else if listError}
                <div class="srp-empty srp-err">{listError}</div>
            {:else if recordings.length === 0}
                <div class="srp-empty">
                    {$trad('Sin grabaciones todavía. Pulsa ● REC en una sesión para empezar.')}
                </div>
            {:else}
                <ul class="srp-list">
                    {#each recordings as r (r.id)}
                        <li class:selected={selected?.id === r.id} class="srp-row">
                            <button class="srp-row-btn" on:click={() => openRecording(r)}>
                                <div class="srp-row-head">
                                    <span class="srp-row-host">{r.host_name || r.session_id.slice(0, 14)}</span>
                                    <span class="srp-row-dur">{fmtDuration(r)}</span>
                                    {#if !r.ended_at}<span class="srp-live">● LIVE</span>{/if}
                                </div>
                                {#if r.title}<div class="srp-row-title">{r.title}</div>{/if}
                                <div class="srp-row-meta">
                                    <span>{r.event_count} {$trad('eventos')}</span>
                                    <span>{fmtBytes(r.byte_count)}</span>
                                    <span class="srp-row-when">{fmtDate(r.started_at)}</span>
                                </div>
                            </button>
                            <div class="srp-row-actions">
                                <button on:click|stopPropagation={() => renameRecording(r)} title={$trad('Renombrar')}>⚑</button>
                                <button on:click|stopPropagation={() => deleteRecording(r)} title={$trad('Borrar')}>✕</button>
                            </div>
                        </li>
                    {/each}
                </ul>
            {/if}
        </aside>

        <!-- RIGHT — player -->
        <section class="srp-player-pane">
            {#if !selected}
                <div class="srp-empty">
                    {$trad('Selecciona una grabación a la izquierda para reproducir.')}
                </div>
            {:else if loadingEvents}
                <div class="srp-empty">{$trad('Cargando eventos…')}</div>
            {:else}
                <header class="srp-player-hdr">
                    <div class="srp-player-meta">
                        <strong>{selected.host_name || selected.session_id}</strong>
                        <span class="srp-tag-muted">{selected.host_type || ''}</span>
                        <span class="srp-tag-muted">{fmtDate(selected.started_at)}</span>
                        <span class="srp-tag-muted">{events.length} {$trad('eventos')} · {fmtBytes(selected.byte_count)}</span>
                    </div>
                    {#if selected.title}<div class="srp-player-title">⚑ {selected.title}</div>{/if}
                </header>

                <!-- Terminal pane -->
                <div class="srp-terminal" role="log" aria-live="polite">
                    {#each renderedLines as line, _i}
                        <div class={'srp-line ' + eventKindClass(line.kind)}>
                            {#if line.kind === 'cmd'}
                                <span class="srp-prefix">$</span> <span class="srp-cmd-text">{line.text}</span>
                            {:else if line.kind === 'exit'}
                                {@const parsed = (() => { try { return JSON.parse(line.text); } catch { return {}; } })()}
                                <span class="srp-prefix">⏚</span>
                                <span class="srp-exit-text">
                                    exit {parsed.exit_code ?? '—'}
                                    {parsed.duration_ms != null ? ` · ${parsed.duration_ms}ms` : ''}
                                </span>
                            {:else if line.kind === 'meta'}
                                <span class="srp-prefix">·</span> <span class="srp-meta-text">{line.text}</span>
                            {:else if line.kind === 'err'}
                                <span class="srp-err-text">{line.text}</span>
                            {:else}
                                <span class="srp-out-text">{line.text}</span>
                            {/if}
                        </div>
                    {/each}
                    {#if renderedLines.length === 0}
                        <div class="srp-terminal-empty">
                            {$trad('Sin eventos en este momento. Pulsa Play.')}
                        </div>
                    {/if}
                </div>

                <!-- Timeline + transport -->
                <div class="srp-transport">
                    <div class="srp-time-row">
                        <span class="srp-time">{fmtMs(playhead)} / {fmtMs(totalDurationMs)}</span>
                        <input class="srp-scrub"
                               type="range"
                               min="0" max={totalDurationMs}
                               step="50"
                               value={playhead}
                               on:input={(e) => scrub(Number((e.currentTarget as HTMLInputElement).value))}/>
                    </div>
                    <div class="srp-controls">
                        <button class="srp-btn srp-btn-primary"
                                on:click={() => isPlaying ? stopPlayback() : startPlayback()}
                                disabled={totalDurationMs === 0}>
                            {isPlaying ? '❚❚ ' + ($trad('Pausa')) : '▶ ' + ($trad('Play'))}
                        </button>
                        <button class="srp-btn" on:click={() => scrub(0)} title={$trad('Reiniciar')}>↺</button>
                        <span class="srp-speed">
                            {#each [0.5, 1, 2, 5, 0] as s}
                                <button class:active={speed === s}
                                        on:click={() => speed = s as 0.5 | 1 | 2 | 5 | 0}>
                                    {s === 0 ? '∞' : s + '×'}
                                </button>
                            {/each}
                        </span>
                    </div>
                </div>
            {/if}
        </section>
    </div>
</div>

<style>
    .srp-overlay {
        position: fixed; inset: 0;
        background: rgba(8, 10, 18, 0.97);
        display: flex; flex-direction: column;
        z-index: 9000;
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace);
        color: var(--text-main, #cbd5e1);
        font-size: 11px;
    }
    .srp-header {
        display: flex; align-items: center; justify-content: space-between;
        padding: 10px 16px;
        background: rgba(15, 19, 31, 0.96);
        border-bottom: 1px solid rgba(255,255,255,0.06);
    }
    .srp-title { display: flex; align-items: center; gap: 10px; font-size: 13px; font-weight: 600; letter-spacing: 0.5px; }
    .srp-glyph { color: #ef4444; font-size: 14px; }
    .srp-count { font-size: 10px; color: var(--text-muted, #94a3b8); font-weight: 400; }
    .srp-actions { display: flex; align-items: center; gap: 8px; }
    .srp-btn {
        background: rgba(255,255,255,0.04); border: 1px solid rgba(255,255,255,0.06);
        color: var(--text-muted); font: inherit; font-size: 11px;
        padding: 4px 10px; border-radius: 5px; cursor: pointer;
    }
    .srp-btn:hover:not(:disabled) { background: rgba(255,255,255,0.07); color: var(--text-main); }
    .srp-btn:disabled { opacity: 0.45; cursor: default; }
    .srp-btn-primary { background: rgba(16,185,129,0.18); color: var(--accent, #10b981); border-color: rgba(16,185,129,0.3); }
    .srp-btn-primary:hover:not(:disabled) { background: rgba(16,185,129,0.28); }
    .srp-close:hover { background: rgba(239,68,68,0.15); color: #ef4444; }
    .srp-scope { display: inline-flex; padding: 2px;
        background: rgba(255,255,255,0.04); border-radius: 5px; }
    .srp-scope button {
        background: transparent; border: 0; color: var(--text-muted);
        font: inherit; font-size: 10px; padding: 3px 10px; border-radius: 3px;
        cursor: pointer;
    }
    .srp-scope button:hover:not(:disabled) { color: var(--text-main); }
    .srp-scope button.active { background: rgba(16,185,129,0.18); color: var(--accent, #10b981); }
    .srp-scope button:disabled { opacity: 0.45; cursor: not-allowed; }

    .srp-body { flex: 1; display: grid; grid-template-columns: 340px 1fr; overflow: hidden; }
    .srp-list-pane {
        border-right: 1px solid rgba(255,255,255,0.06);
        overflow-y: auto;
    }
    .srp-list { list-style: none; margin: 0; padding: 4px 0; }
    .srp-row {
        position: relative;
        border-bottom: 1px solid rgba(255,255,255,0.03);
    }
    .srp-row:hover { background: rgba(255,255,255,0.02); }
    .srp-row.selected { background: rgba(16,185,129,0.08); }
    .srp-row-btn {
        width: 100%; background: transparent; border: 0;
        text-align: left; cursor: pointer;
        padding: 8px 12px;
        color: var(--text-main); font: inherit;
    }
    .srp-row-head { display: flex; gap: 8px; align-items: baseline; font-size: 11px; }
    .srp-row-host { color: var(--accent, #10b981); font-weight: 600; }
    .srp-row-dur { color: var(--text-muted); font-size: 10px; margin-left: auto; }
    .srp-live { background: rgba(239,68,68,0.18); color: #ef4444; padding: 1px 6px; border-radius: 8px; font-size: 9px; font-weight: 700; letter-spacing: 0.4px; }
    .srp-row-title { font-size: 10px; color: #f472b6; margin: 2px 0; }
    .srp-row-meta { display: flex; gap: 10px; font-size: 9px; color: var(--text-muted); margin-top: 2px; }
    .srp-row-when { margin-left: auto; }
    .srp-row-actions {
        position: absolute; right: 8px; top: 8px;
        display: flex; gap: 2px; opacity: 0; transition: opacity .12s;
    }
    .srp-row:hover .srp-row-actions { opacity: 1; }
    .srp-row-actions button {
        background: rgba(0,0,0,0.4); border: 0; color: var(--text-muted);
        font-size: 11px; padding: 3px 6px; border-radius: 3px; cursor: pointer;
    }
    .srp-row-actions button:hover { color: var(--text-main); background: rgba(255,255,255,0.1); }

    .srp-player-pane { display: flex; flex-direction: column; overflow: hidden; padding: 12px 16px; }
    .srp-player-hdr { margin-bottom: 8px; }
    .srp-player-meta { display: flex; gap: 12px; align-items: baseline; font-size: 11px; }
    .srp-tag-muted { color: var(--text-muted); font-size: 10px; }
    .srp-player-title { color: #f472b6; font-size: 12px; margin-top: 4px; }

    /* Terminal */
    .srp-terminal {
        flex: 1; overflow-y: auto;
        background: #0b0f1a;
        border: 1px solid rgba(255,255,255,0.06);
        border-radius: 6px;
        padding: 10px 14px;
        font-family: var(--font-mono);
        font-size: 11px; line-height: 1.55;
        white-space: pre-wrap; word-break: break-word;
    }
    .srp-line { padding: 1px 0; }
    .srp-prefix { color: var(--text-muted); display: inline-block; min-width: 16px; }
    .srp-cmd-text { color: var(--accent, #10b981); font-weight: 600; }
    .srp-out-text { color: #d1d5db; }
    .srp-err-text { color: #f87171; }
    .srp-exit-text { color: #fbbf24; }
    .srp-meta-text { color: #94a3b8; font-style: italic; }
    .srp-terminal-empty { color: var(--text-muted); font-style: italic; text-align: center; padding: 30px 0; }

    /* Transport */
    .srp-transport {
        margin-top: 8px;
        padding: 8px 0 0;
        border-top: 1px solid rgba(255,255,255,0.04);
    }
    .srp-time-row { display: flex; gap: 12px; align-items: center; margin-bottom: 6px; }
    .srp-time { color: var(--text-muted); font-size: 10px; min-width: 90px; font-variant-numeric: tabular-nums; }
    .srp-scrub { flex: 1; }
    .srp-controls { display: flex; gap: 10px; align-items: center; }
    .srp-speed {
        display: inline-flex; padding: 2px;
        background: rgba(255,255,255,0.04); border-radius: 5px;
        margin-left: auto;
    }
    .srp-speed button {
        background: transparent; border: 0; color: var(--text-muted);
        font: inherit; font-size: 10px; padding: 3px 8px; border-radius: 3px;
        cursor: pointer;
    }
    .srp-speed button.active { background: rgba(16,185,129,0.18); color: var(--accent, #10b981); }
    .srp-speed button:hover:not(.active) { color: var(--text-main); }

    .srp-empty {
        padding: 30px 20px; text-align: center;
        font-size: 11px; color: var(--text-muted); font-style: italic;
    }
    .srp-err { color: #ef4444; font-style: normal; }
</style>
