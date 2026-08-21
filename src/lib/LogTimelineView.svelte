<!--
  LogTimelineView.svelte — Multi-host log timeline (Sprint B #4)

  Fetches the same log path from N hosts in parallel and renders the
  entries interleaved by parsed timestamp. Color-codes each host so the
  reader can scan correlated events across machines (e.g. "node-A had
  a connection refused 200ms BEFORE node-B noticed").

  No backend changes needed — reuses the existing per-host commands:
    • read_log_tail              (local)
    • read_remote_log_windows    (WinRM)
    • read_remote_log_linux      (SSH)

  Timestamp extraction is best-effort: tries 4 common formats
  (ISO-8601, syslog, Apache CLF, bracket-tag). Lines without a parseable
  timestamp fall through into the "unsorted tail" group at the bottom.

  Open from the Log Viewer toolbar (button "⇄ Multi-host"); the user
  picks 2+ hosts, types a path, and clicks fetch.
-->
<script>
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { invoke } from '@tauri-apps/api/core';
    import { createEventDispatcher, onMount, onDestroy } from 'svelte';
    import Search from '@tabler/icons-svelte/icons/search';
    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';

    export let hosts = [];
    export let hostName = '';
    export let isEN = false;

    const dispatch = createEventDispatcher();

    // ── State ──────────────────────────────────────────────────────────
    let selectedHostIds = new Set();   // Set<string> — '' = local
    let logPath = '';
    let perHostTail = 200;
    let entries = [];                  // { hostId, hostLabel, color, ts, text, level }
    let unsorted = [];                 // entries with no parseable timestamp
    let fetching = false;
    let lastFetchAt = '';
    let error = '';
    let regexFilter = '';
    let levelFilter = 'all';           // 'all'|'error'|'warn'|'info'|'debug'
    let autoRefreshTimer = null;
    let autoRefreshOn = false;

    // ── Per-host color palette (deterministic by host id) ──────────────
    const HOST_COLORS = [
        '#56b4e9', '#e69f00', '#009e73', '#cc79a7',
        '#0072b2', '#f0e442', '#d55e00', '#bfbfbf',
    ];
    function colorForHost(hid) {
        // Hash the id (or "local") into [0, 7] deterministically.
        let h = 0;
        for (const ch of (hid || 'local')) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
        return HOST_COLORS[h % HOST_COLORS.length];
    }
    function labelForHost(hid) {
        if (hid === '' || hid === 'local') return isEN ? 'local · ' + hostName : 'local · ' + hostName;
        const h = hosts.find(x => x.id === hid);
        return h ? h.name : hid;
    }

    // ── Timestamp parsing ──────────────────────────────────────────────
    // 4 strategies, tried in order. We capture the matched timestamp text
    // AND a Date object so the entry can be merge-sorted by `ts`.
    const TS_PATTERNS = [
        // ISO 8601: 2025-12-01T14:23:45.123Z  or  2025-12-01 14:23:45
        /^(\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?)/,
        // Bracket-tagged: [2025-12-01 14:23:45]
        /^\[(\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?)\]/,
        // Syslog: Dec  1 14:23:45  (year inferred = current)
        /^([A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2})/,
        // Apache CLF: [01/Dec/2025:14:23:45 +0000]
        /^.*?\[(\d{2}\/[A-Z][a-z]{2}\/\d{4}:\d{2}:\d{2}:\d{2}\s+[+-]\d{4})\]/,
    ];
    function parseTimestamp(line) {
        for (const re of TS_PATTERNS) {
            const m = re.exec(line);
            if (!m) continue;
            // Build a JS Date from the captured text. Browser Date parser
            // accepts ISO; syslog/Apache need manual conversion.
            let txt = m[1];
            let d = new Date(txt);
            if (!isFinite(d.getTime())) {
                // Syslog (Jan -> 0-indexed month)
                const sl = /^([A-Z][a-z]{2})\s+(\d{1,2})\s+(\d{2}):(\d{2}):(\d{2})/.exec(txt);
                if (sl) {
                    const months = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];
                    const mo = months.indexOf(sl[1]);
                    if (mo >= 0) {
                        const year = new Date().getFullYear();
                        d = new Date(year, mo, +sl[2], +sl[3], +sl[4], +sl[5]);
                    }
                }
            }
            if (isFinite(d.getTime())) return { ts: d.getTime(), tsText: txt };
        }
        return null;
    }

    function detectLevel(line) {
        const l = line.toLowerCase();
        if (/\b(error|fatal|critical|panic|severe)\b/.test(l)) return 'error';
        if (/\bwarn(ing)?\b/.test(l)) return 'warn';
        if (/\binfo\b/.test(l)) return 'info';
        if (/\b(debug|trace|verbose)\b/.test(l)) return 'debug';
        return 'info';
    }

    // ── Fetching ───────────────────────────────────────────────────────
    async function fetchAllHosts() {
        if (!logPath.trim()) {
            error = $trad('Ingresa la ruta del log.');
            return;
        }
        if (selectedHostIds.size === 0) {
            error = $trad('Selecciona al menos un host.');
            return;
        }
        fetching = true;
        error = '';
        const fetched = [];
        const tasks = [];
        for (const hid of selectedHostIds) {
            tasks.push(fetchOne(hid).then(lines => fetched.push({ hid, lines })));
        }
        await Promise.allSettled(tasks);
        // Merge: each line gets parsed into a structured entry; entries with
        // a timestamp go into `entries`, the rest into `unsorted`.
        const sorted = [];
        const tail = [];
        for (const { hid, lines } of fetched) {
            const label = labelForHost(hid);
            const color = colorForHost(hid);
            for (const line of (lines || [])) {
                const t = parseTimestamp(line);
                const lvl = detectLevel(line);
                if (t) {
                    sorted.push({ hostId: hid, hostLabel: label, color, ts: t.ts, tsText: t.tsText, text: line, level: lvl });
                } else {
                    tail.push({ hostId: hid, hostLabel: label, color, ts: 0, tsText: '', text: line, level: lvl });
                }
            }
        }
        sorted.sort((a, b) => a.ts - b.ts);
        entries = sorted;
        unsorted = tail;
        fetching = false;
        lastFetchAt = new Date().toLocaleTimeString();
    }

    async function fetchOne(hid) {
        try {
            if (hid === '' || hid === 'local') {
                return await invoke('read_log_tail', { path: logPath.trim(), lines: perHostTail });
            }
            const h = hosts.find(x => x.id === hid);
            if (!h) return [];
            let pwd = '';
            try { pwd = await invoke('get_host_credential', { hostId: h.id }); } catch {}
            if (h.type === 'windows') {
                return await invoke('read_remote_log_windows', {
                    host: h.host, username: h.username, password: pwd,
                    path: logPath.trim(), lines: perHostTail,
                });
            }
            return await invoke('read_remote_log_linux', {
                host: h.host, username: h.username,
                path: logPath.trim(), lines: perHostTail,
                port: h.port || 22, keyPath: h.sshKeyPath || null,
            });
        } catch (e) {
            // Per-host failure shouldn't abort the whole batch — capture in
            // a warning entry so the user sees that this host went silent.
            return [`[Lucy] Fetch failed for ${labelForHost(hid)}: ${e}`];
        }
    }

    // ── Filtering ──────────────────────────────────────────────────────
    let compiledFilter = null;
    $: {
        compiledFilter = null;
        if (regexFilter.trim()) {
            try { compiledFilter = new RegExp(regexFilter, 'i'); }
            catch { compiledFilter = null; }
        }
    }

    $: filteredEntries = entries.filter(e => {
        if (levelFilter !== 'all' && e.level !== levelFilter) return false;
        if (compiledFilter && !compiledFilter.test(e.text)) return false;
        if (!compiledFilter && regexFilter.trim() && !e.text.toLowerCase().includes(regexFilter.toLowerCase())) return false;
        return true;
    });
    $: filteredUnsorted = unsorted.filter(e => {
        if (levelFilter !== 'all' && e.level !== levelFilter) return false;
        if (compiledFilter && !compiledFilter.test(e.text)) return false;
        if (!compiledFilter && regexFilter.trim() && !e.text.toLowerCase().includes(regexFilter.toLowerCase())) return false;
        return true;
    });

    // Per-host counts in the filtered set (drives the legend badges).
    $: hostCounts = (() => {
        const m = new Map();
        for (const e of filteredEntries) m.set(e.hostId, (m.get(e.hostId) || 0) + 1);
        for (const e of filteredUnsorted) m.set(e.hostId, (m.get(e.hostId) || 0) + 1);
        return m;
    })();

    // ── Auto-refresh ───────────────────────────────────────────────────
    function toggleAutoRefresh() {
        autoRefreshOn = !autoRefreshOn;
        if (autoRefreshOn) {
            autoRefreshTimer = setInterval(fetchAllHosts, 10_000);
        } else if (autoRefreshTimer) {
            clearInterval(autoRefreshTimer);
            autoRefreshTimer = null;
        }
    }
    onDestroy(() => { if (autoRefreshTimer) clearInterval(autoRefreshTimer); });

    function toggleHost(hid) {
        const next = new Set(selectedHostIds);
        if (next.has(hid)) next.delete(hid); else next.add(hid);
        selectedHostIds = next;
    }

    function fmtAbsTs(ms) {
        if (!ms) return '';
        const d = new Date(ms);
        return d.toISOString().replace('T', ' ').slice(11, 23);  // HH:MM:SS.mmm
    }
    function levelClass(lvl) {
        if (lvl === 'error') return 'tl-error';
        if (lvl === 'warn')  return 'tl-warn';
        if (lvl === 'debug') return 'tl-debug';
        return 'tl-info';
    }

    onMount(() => {
        // Default: local + first remote, if available
        selectedHostIds = new Set(['']);
        if (hosts.length > 0) selectedHostIds.add(hosts[0].id);
        selectedHostIds = selectedHostIds; // trigger reactivity
    });
</script>

<div class="tl-overlay">
    <!-- Header -->
    <div class="tl-header">
        <div class="tl-title">
            <span class="tl-glyph">⇄</span>
            <span>{$trad('Timeline multi-host de logs')}</span>
            {#if entries.length || unsorted.length}
                <span class="tl-count">
                    {filteredEntries.length + filteredUnsorted.length} / {entries.length + unsorted.length}
                    · {selectedHostIds.size} {$trad('hosts')}
                    {#if lastFetchAt} · {lastFetchAt}{/if}
                </span>
            {/if}
        </div>
        <div class="tl-actions">
            <button class="tl-btn tl-btn-primary" on:click={fetchAllHosts} disabled={fetching}>
                {fetching ? '⟳' : '↓'} {$trad('Cargar')}
            </button>
            <button class="tl-btn" on:click={toggleAutoRefresh}
                    class:active={autoRefreshOn}
                    title={$trad('Auto-refresh cada 10s')}>
                ⌛ {autoRefreshOn ? ($trad('auto')) : ($trad('manual'))}
            </button>
            <button class="tl-btn tl-btn-close" on:click={() => dispatch('close')} title="Esc">✕</button>
        </div>
    </div>

    <!-- Controls -->
    <div class="tl-controls">
        <div class="tl-row">
            <input class="tl-input mono" type="text"
                   bind:value={logPath}
                   placeholder={$trad('Ruta del log (misma en todos los hosts)')}
                   on:keydown={(e) => { if (e.key === 'Enter') fetchAllHosts(); }}/>
            <select class="tl-input" bind:value={perHostTail}>
                <option value={50}>50 {$trad('líneas/host')}</option>
                <option value={100}>100</option>
                <option value={200}>200</option>
                <option value={500}>500</option>
            </select>
        </div>
        <div class="tl-row tl-host-chips">
            <span class="tl-row-label">{$trad('Hosts:')}</span>
            <button class="tl-host-chip" class:active={selectedHostIds.has('')}
                    style="--host-color: {colorForHost('')};"
                    on:click={() => toggleHost('')}>
                <span class="tl-host-dot"></span>local · {hostName}
                {#if hostCounts.get('')}<span class="tl-host-count">{hostCounts.get('')}</span>{/if}
            </button>
            {#each hosts as h}
                <button class="tl-host-chip" class:active={selectedHostIds.has(h.id)}
                        style="--host-color: {colorForHost(h.id)};"
                        on:click={() => toggleHost(h.id)}>
                    <span class="tl-host-dot"></span>{h.name}
                    {#if hostCounts.get(h.id)}<span class="tl-host-count">{hostCounts.get(h.id)}</span>{/if}
                </button>
            {/each}
        </div>
        <div class="tl-row">
            <div class="tl-search">
                <Search size={13}/>
                <input class="tl-input" type="text"
                       bind:value={regexFilter}
                       placeholder={$trad('Filtrar (regex válido)…')}/>
            </div>
            <select class="tl-input" bind:value={levelFilter}>
                <option value="all">{$trad('Todos los niveles')}</option>
                <option value="error">error</option>
                <option value="warn">warn</option>
                <option value="info">info</option>
                <option value="debug">debug</option>
            </select>
        </div>
    </div>

    {#if error}
        <div class="tl-error"><AlertTriangle size={12}/> {error}</div>
    {/if}

    <!-- Timeline -->
    <div class="tl-scroll">
        {#if !entries.length && !unsorted.length && !fetching}
            <div class="tl-empty">
                {$trad('Selecciona hosts + ruta de log, pulsa Cargar.')}
            </div>
        {:else if !filteredEntries.length && !filteredUnsorted.length}
            <div class="tl-empty">
                {$trad('Ningún entry cumple los filtros actuales.')}
            </div>
        {:else}
            {#each filteredEntries as e, i (e.hostId + ':' + e.ts + ':' + i)}
                <div class="tl-row-entry {levelClass(e.level)}" style="--host-color: {e.color};">
                    <span class="tl-bar"></span>
                    <span class="tl-host-label">{e.hostLabel}</span>
                    <span class="tl-ts mono">{fmtAbsTs(e.ts)}</span>
                    <span class="tl-text mono">{e.text}</span>
                </div>
            {/each}
            {#if filteredUnsorted.length}
                <div class="tl-unsorted-hdr">
                    {isEN
                        ? `--- ${filteredUnsorted.length} entries with no parseable timestamp ---`
                        : `--- ${filteredUnsorted.length} entradas sin timestamp parseable ---`}
                </div>
                {#each filteredUnsorted as e, i (e.hostId + ':U:' + i)}
                    <div class="tl-row-entry {levelClass(e.level)}" style="--host-color: {e.color};">
                        <span class="tl-bar"></span>
                        <span class="tl-host-label">{e.hostLabel}</span>
                        <span class="tl-ts mono">—</span>
                        <span class="tl-text mono">{e.text}</span>
                    </div>
                {/each}
            {/if}
        {/if}
    </div>
</div>

<style>
    .tl-overlay {
        position: fixed; inset: 0;
        background: #0a0d18;
        backdrop-filter: blur(10px) saturate(140%);
        -webkit-backdrop-filter: blur(10px) saturate(140%);
        display: flex; flex-direction: column;
        z-index: 9000;
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace);
        color: var(--text-main, #cbd5e1);
        font-size: 11px;
    }
    .tl-header {
        display: flex; align-items: center; justify-content: space-between;
        padding: 10px 16px; gap: 10px;
        background: rgba(15, 19, 31, 0.96);
        border-bottom: 1px solid rgba(255,255,255,0.06);
    }
    .tl-title { display: flex; align-items: center; gap: 10px; font-size: 13px; font-weight: 600; letter-spacing: 0.5px; }
    .tl-glyph { color: var(--accent, #10b981); font-size: 16px; }
    .tl-count { font-size: 10px; color: var(--text-muted, #94a3b8); font-weight: 400; }
    .tl-actions { display: flex; gap: 8px; }
    .tl-btn {
        background: rgba(255,255,255,0.04); border: 1px solid rgba(255,255,255,0.06);
        color: var(--text-muted, #94a3b8); font: inherit; font-size: 11px;
        padding: 4px 10px; border-radius: 5px; cursor: pointer; transition: background .12s;
    }
    .tl-btn:hover:not(:disabled) { background: rgba(255,255,255,0.07); color: var(--text-main); }
    .tl-btn:disabled { opacity: 0.45; cursor: default; }
    .tl-btn.active { background: rgba(16,185,129,0.18); color: var(--accent, #10b981); border-color: rgba(16,185,129,0.30); }
    .tl-btn-primary { background: rgba(16,185,129,0.18); color: var(--accent, #10b981); border-color: rgba(16,185,129,0.30); }
    .tl-btn-primary:hover:not(:disabled) { background: rgba(16,185,129,0.28); }
    .tl-btn-close:hover { background: rgba(239,68,68,0.15); color: #ef4444; border-color: rgba(239,68,68,0.30); }

    .tl-controls { display: flex; flex-direction: column; gap: 6px; padding: 8px 16px;
        background: rgba(15,19,31,0.92); border-bottom: 1px solid rgba(255,255,255,0.04); }
    .tl-row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
    .tl-row-label { font-size: 10px; color: var(--text-muted); }
    .tl-input {
        background: rgba(0,0,0,0.30); border: 1px solid rgba(255,255,255,0.06); border-radius: 5px;
        color: var(--text-main); font: inherit; font-size: 11px; padding: 4px 8px; outline: none;
    }
    .tl-input:focus { border-color: var(--accent, #10b981); }
    input.tl-input { flex: 1; min-width: 120px; }
    .tl-row .tl-search { flex: 1; display: flex; align-items: center; gap: 6px;
        background: rgba(0,0,0,0.30); border: 1px solid rgba(255,255,255,0.06); border-radius: 5px;
        padding: 0 8px; color: var(--text-muted); }
    .tl-row .tl-search input { background: transparent; border: 0; flex: 1; padding: 4px 0; }
    .tl-row .tl-search input:focus { outline: none; }

    .tl-host-chips { flex-wrap: wrap; }
    .tl-host-chip {
        background: rgba(255,255,255,0.03); border: 1px solid rgba(255,255,255,0.06);
        color: var(--text-muted); font: inherit; font-size: 10px; padding: 3px 8px;
        border-radius: 12px; cursor: pointer; display: inline-flex; align-items: center; gap: 5px;
    }
    .tl-host-chip:hover { background: rgba(255,255,255,0.06); }
    .tl-host-chip.active {
        background: color-mix(in srgb, var(--host-color) 18%, transparent);
        color: var(--text-main);
        border-color: color-mix(in srgb, var(--host-color) 40%, transparent);
    }
    .tl-host-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--host-color); }
    .tl-host-count { font-size: 9px; color: var(--host-color); font-weight: 700; }

    .tl-error { margin: 6px 16px; padding: 6px 10px; font-size: 11px;
        background: rgba(239,68,68,.08); border: 1px solid rgba(239,68,68,.2);
        border-radius: 4px; color: #ef4444; display: flex; align-items: center; gap: 6px; }

    .tl-scroll { flex: 1; overflow-y: auto; padding: 8px 16px; }
    .tl-empty { text-align: center; padding: 30px 20px; color: var(--text-muted); font-style: italic; }

    .tl-row-entry {
        display: grid;
        grid-template-columns: 4px 110px 96px 1fr;
        gap: 8px; padding: 1px 0;
        align-items: baseline;
        line-height: 1.55;
    }
    .tl-bar { background: var(--host-color); border-radius: 2px; height: 12px; align-self: center; }
    .tl-host-label { color: var(--host-color); font-size: 10px;
        white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .tl-ts { color: var(--text-muted); font-size: 10px; font-variant-numeric: tabular-nums; }
    .tl-text { color: var(--text-main); font-size: 11px;
        white-space: pre-wrap; word-break: break-word; }
    .mono { font-family: var(--font-mono); }

    .tl-row-entry.tl-error .tl-text { color: #ef4444; }
    .tl-row-entry.tl-warn  .tl-text { color: #f59e0b; }
    .tl-row-entry.tl-info  .tl-text { color: var(--text-main); }
    .tl-row-entry.tl-debug .tl-text { color: var(--text-muted); }

    .tl-unsorted-hdr {
        text-align: center; color: var(--text-muted); font-size: 10px;
        padding: 14px 0 8px; font-style: italic;
        border-top: 1px dashed rgba(255,255,255,0.10); margin-top: 12px;
    }
</style>
