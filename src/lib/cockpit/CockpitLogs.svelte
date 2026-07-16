<script>
  /* ==========================================================================
     Lucy 2.0 — Log Viewer (cockpit)  ·  Phase F4/views. LIVE.
     Streams Lucy's structured audit trail via `query_audit_trail` (the same
     DB-backed command behind AuditTrailView) — entries carry timestamp / host
     / command / source / exit_code / output. No file-path resolution needed
     (unlike raw `read_log_tail`, which can't expand %APPDATA% from the front).
     Level is derived from exit_code. Polls every 5 s while mounted.
     Representative data is the FALLBACK for a plain-browser preview.
     ========================================================================== */
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { copyToClipboard } from '$lib/lucy-api';
  import FileText from '@tabler/icons-svelte/icons/file-text';
  import Search from '@tabler/icons-svelte/icons/search';
  import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
  import CircleX from '@tabler/icons-svelte/icons/circle-x';
  import InfoCircle from '@tabler/icons-svelte/icons/info-circle';
  import Copy from '@tabler/icons-svelte/icons/copy';
  import Check from '@tabler/icons-svelte/icons/check';
  import PlayerPause from '@tabler/icons-svelte/icons/player-pause';
  import PlayerPlay from '@tabler/icons-svelte/icons/player-play';

  const LV = {
    error: { cls: 'error', Icon: CircleX },
    warn:  { cls: 'warn',  Icon: AlertTriangle },
    info:  { cls: 'info',  Icon: InfoCircle },
  };

  // Deterministic representative fallback (browser preview / no backend).
  const SAMPLE = [
    { id: -1, t: '14:22:12', lv: 'info',  src: 'net',        m: "Restart-NetAdapter 'Ethernet 2' — link up (1 Gbps)" },
    { id: -2, t: '14:22:07', lv: 'error', src: 'net',        m: 'e2xw driver timeout on adapter «Ethernet 2» — link down' },
    { id: -3, t: '14:20:15', lv: 'error', src: 'sshd',       m: 'failed password for invalid user admin from 10.0.4.9' },
    { id: -4, t: '14:19:02', lv: 'warn',  src: 'disk',       m: 'Partición / al 87% en db-replica-01' },
    { id: -5, t: '14:18:33', lv: 'info',  src: 'nginx',      m: 'reload completado — 0 errores de configuración' },
    { id: -6, t: '14:17:51', lv: 'info',  src: 'scheduler',  m: "Tarea 'LucyBackup' ejecutada (exit 0, 4.2 s)" },
    { id: -7, t: '14:15:08', lv: 'warn',  src: 'compliance', m: 'Actualizaciones automáticas deshabilitadas en win-dc-02' },
    { id: -8, t: '14:14:44', lv: 'info',  src: 'audit',      m: 'execute_powershell autorizado por operador (token single-use)' },
  ];

  let rows = $state(null);
  let live = $state(false);
  let loading = $state(false);
  let lastUpdate = $state('');
  let active = $state('all');
  let query = $state('');
  let _timer = null;
  let paused = $state(false);      // pause the 5 s live poll to read without the list jumping
  let _copiedAll = $state(false);
  let _copiedId = $state(null);

  function fmtTime(e) {
    let s = e.created_at;
    if (s) { if (s > 1e12) s = Math.floor(s / 1000); try { return new Date(s * 1000).toLocaleTimeString('es', { hour: '2-digit', minute: '2-digit', second: '2-digit' }); } catch {} }
    return (e.timestamp || '').slice(11, 19) || (e.timestamp || '');
  }
  function levelOf(e) {
    if (e.exit_code != null && e.exit_code !== 0) return 'error';
    const blob = ((e.command || '') + ' ' + (e.output_preview || '')).toLowerCase();
    if (/\b(warn|warning|advertenc|aviso|deprecat)\b/.test(blob)) return 'warn';
    return 'info';
  }
  const normalize = (e, i) => ({ id: e.id ?? -i, t: fmtTime(e), lv: levelOf(e), src: e.source || 'lucy', m: e.command || e.output_preview || '' });

  async function load() {
    loading = true;
    try {
      const res = await invoke('query_audit_trail', { limit: 200 });
      const entries = Array.isArray(res?.entries) ? res.entries : Array.isArray(res) ? res : [];
      rows = entries.map(normalize);
      live = true;
      lastUpdate = new Date().toLocaleTimeString('es', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    } catch {
      if (!rows) rows = SAMPLE;
    } finally {
      loading = false;
    }
  }

  const counts = $derived.by(() => {
    const l = rows || [];
    return { error: l.filter((r) => r.lv === 'error').length, warn: l.filter((r) => r.lv === 'warn').length, info: l.filter((r) => r.lv === 'info').length };
  });
  const filters = $derived([
    { id: 'all',   label: 'Todos', n: rows ? rows.length : 0 },
    { id: 'error', label: 'Error', n: counts.error },
    { id: 'warn',  label: 'Warn',  n: counts.warn },
    { id: 'info',  label: 'Info',  n: counts.info },
  ]);
  const shown = $derived.by(() => {
    if (!rows) return [];
    const q = query.trim().toLowerCase();
    return rows
      .filter((r) => active === 'all' || r.lv === active)
      .filter((r) => !q || (r.m + ' ' + r.src).toLowerCase().includes(q));
  });

  const _rowText = (l) => `${l.t}  ${l.lv.toUpperCase().padEnd(5)}  ${l.src}  ${l.m}`;
  async function copyAll() {
    const txt = shown.map(_rowText).join('\n');
    if (!txt) return;
    try { await copyToClipboard(txt); } catch { try { navigator.clipboard?.writeText(txt); } catch {} }
    _copiedAll = true; setTimeout(() => { _copiedAll = false; }, 1200);
  }
  async function copyRow(l) {
    try { await copyToClipboard(_rowText(l)); } catch { try { navigator.clipboard?.writeText(_rowText(l)); } catch {} }
    _copiedId = l.id; setTimeout(() => { if (_copiedId === l.id) _copiedId = null; }, 1000);
  }

  onMount(() => {
    load();
    _timer = setInterval(() => { if (!paused) load(); }, 5000);
    return () => { if (_timer) clearInterval(_timer); };
  });
</script>

<div class="logs">
  <div class="logs-head">
    <span class="ck-led" class:on={live && !paused} class:warn={live && paused} aria-hidden="true"></span>
    <span class="logs-title">Visor de logs</span>
    <span class="src-pill"><FileText size={14} stroke={1.75} /> audit trail</span>
    {#if live && lastUpdate}
      <span class="live" class:paused>{#if paused}⏸ pausado · {lastUpdate}{:else}<span class="live-dot"></span>en vivo · {lastUpdate}{/if}</span>
    {/if}
    <span class="logs-actions">
      <button class="logs-tool" onclick={() => (paused = !paused)} title={paused ? 'Reanudar actualización en vivo' : 'Pausar actualización en vivo'} aria-label={paused ? 'Reanudar' : 'Pausar'}>
        {#if paused}<PlayerPlay size={15} stroke={1.75} />{:else}<PlayerPause size={15} stroke={1.75} />{/if}
      </button>
      <button class="logs-tool" onclick={copyAll} disabled={!shown.length} title="Copiar los logs visibles" aria-label="Copiar logs">
        {#if _copiedAll}<Check size={15} stroke={2} />{:else}<Copy size={15} stroke={1.75} />{/if}
      </button>
    </span>
  </div>

  <div class="logs-toolbar">
    <div class="filters">
      {#each filters as f}
        <button class="fchip" class:on={active === f.id} onclick={() => (active = f.id)}>{f.label}<span class="fchip-n">{f.n}</span></button>
      {/each}
    </div>
    <div class="logs-search">
      <Search size={15} stroke={1.75} />
      <input placeholder="Filtrar mensajes…" bind:value={query} />
    </div>
  </div>

  {#if rows && !live}<div class="logs-note">Datos de ejemplo — sin backend disponible en esta vista previa.</div>{/if}

  <div class="stream" role="log">
    {#if !rows}
      <div class="empty"><FileText size={24} stroke={1.5} /><p class="ck-shimmer">Cargando registro…</p></div>
    {:else if shown.length === 0}
      <div class="empty"><Search size={24} stroke={1.5} /><p>{query || active !== 'all' ? 'Sin coincidencias.' : 'Sin actividad registrada.'}</p></div>
    {:else}
      {#each shown as l (l.id)}
        {@const meta = LV[l.lv]}
        <div class="row {meta.cls}">
          <span class="row-t">{l.t}</span>
          <span class="row-lv"><meta.Icon size={13} stroke={1.9} /> {l.lv.toUpperCase()}</span>
          <span class="row-src">{l.src}</span>
          <span class="row-m">{l.m}</span>
          <button class="row-copy" title="Copiar línea" aria-label="Copiar línea" onclick={() => copyRow(l)}>{#if _copiedId === l.id}<Check size={12} stroke={2} />{:else}<Copy size={12} stroke={1.75} />{/if}</button>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .logs { height: 100%; display: flex; flex-direction: column; }

  .logs-head { display: flex; align-items: center; gap: 12px; padding: 18px 22px 14px; }
  .logs-title { font-size: var(--fs-title); font-weight: var(--fw-medium); color: var(--text-primary); }
  .src-pill { display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--text-muted); background: var(--surface-2); border: 1px solid var(--border); padding: 4px 10px; border-radius: var(--r-sm); font-family: var(--font-mono); }
  .live { display: flex; align-items: center; gap: 6px; font-size: var(--fs-caption); color: var(--accent); }
  .live-dot { width: 7px; height: 7px; border-radius: var(--r-pill); background: var(--accent); animation: logs-pulse 1.6s var(--ease-out) infinite; }
  @keyframes logs-pulse { 0% { box-shadow: 0 0 0 0 rgba(61,214,164,0.5); } 70% { box-shadow: 0 0 0 5px rgba(61,214,164,0); } 100% { box-shadow: 0 0 0 0 rgba(61,214,164,0); } }
  .live.paused { color: var(--warning); }
  .logs-actions { margin-left: auto; display: flex; align-items: center; gap: 6px; }
  .logs-tool { display: flex; align-items: center; justify-content: center; width: 30px; height: 30px; padding: 0; border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-2); color: var(--text-muted); cursor: pointer; transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out); }
  .logs-tool:hover:not(:disabled) { background: var(--surface-3); color: var(--text-primary); }
  .logs-tool:disabled { opacity: 0.4; cursor: default; }

  .logs-toolbar { display: flex; align-items: center; gap: 12px; padding: 0 22px 10px; flex-wrap: wrap; }
  .filters { display: flex; gap: 6px; }
  .fchip { display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--text-muted); background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-sm); padding: 5px 10px; cursor: pointer; transition: color var(--dur-fast) var(--ease-out); }
  .fchip:hover { color: var(--text-primary); }
  .fchip.on { color: var(--accent); background: var(--accent-bg); border-color: var(--border-accent); }
  .fchip-n { font-size: var(--fs-caption); color: var(--text-faint); background: var(--surface-3); padding: 0 6px; border-radius: var(--r-pill); }
  .fchip.on .fchip-n { color: var(--accent); background: transparent; }

  .logs-search { flex: 1; min-width: 180px; display: flex; align-items: center; gap: 8px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-md); padding: 6px 11px; color: var(--text-muted); }
  .logs-search input { flex: 1; background: transparent; border: 0; outline: 0; color: var(--text-primary); font-size: var(--fs-body); font-family: var(--font-sans); }
  .logs-search input::placeholder { color: var(--text-faint); }
  .logs-note { padding: 0 22px 8px; font-size: var(--fs-caption); color: var(--text-faint); }

  .stream { flex: 1; overflow-y: auto; padding: 4px 16px 16px; font-family: var(--font-mono); }
  .row { display: grid; grid-template-columns: 62px 78px 110px 1fr 22px; gap: 12px; align-items: baseline; padding: 6px 10px; border-radius: var(--r-sm); border-left: 2px solid transparent; }
  .row:hover { background: var(--surface-2); }
  .row-copy { align-self: center; display: flex; align-items: center; justify-content: center; width: 22px; height: 22px; padding: 0; border: 0; border-radius: 5px; background: transparent; color: var(--text-faint); cursor: pointer; opacity: 0; transition: opacity var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out); }
  .row:hover .row-copy { opacity: 1; }
  .row-copy:hover { background: var(--surface-3); color: var(--text-primary); }
  .row-t { font-size: var(--fs-caption); color: var(--text-faint); }
  .row-lv { display: flex; align-items: center; gap: 4px; font-size: var(--fs-caption); font-weight: var(--fw-medium); letter-spacing: 0.02em; }
  .row-src { font-size: var(--fs-caption); color: var(--text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .row-m { font-size: var(--fs-code); color: var(--text-secondary); line-height: var(--lh-tight); word-break: break-word; }

  .row.error { border-left-color: var(--danger); }
  .row.error .row-lv { color: var(--danger); }
  .row.warn { border-left-color: var(--warning); }
  .row.warn .row-lv { color: var(--warning); }
  .row.info .row-lv { color: var(--text-muted); }

  .empty { height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; color: var(--text-faint); }
  .empty p { margin: 0; font-size: var(--fs-footnote); }
</style>
