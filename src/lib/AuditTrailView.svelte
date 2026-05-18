<script>
    import { createEventDispatcher, onMount } from 'svelte';
    import { auditTrail } from '$lib/stores';
    import { queryAuditTrail } from '$lib/audit';
    import { exportAuditPdf } from '$lib/reports/ReportGenerator';
    import { staggerIn } from '$lib/stagger';
    import ClipboardList from '@tabler/icons-svelte/icons/clipboard-list';

    import Download from '@tabler/icons-svelte/icons/download';

    import Trash2 from '@tabler/icons-svelte/icons/trash';

    import FileText from '@tabler/icons-svelte/icons/file-text';

    import Sparkles from '@tabler/icons-svelte/icons/sparkles';

    import BookOpen from '@tabler/icons-svelte/icons/book-2';

    import ShieldCheck from '@tabler/icons-svelte/icons/shield-check';

    import Radio from '@tabler/icons-svelte/icons/radio';

    import Keyboard from '@tabler/icons-svelte/icons/keyboard';

    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
    const dispatch = createEventDispatcher();

    export let hosts    = [];
    export let isEN     = false;

    // P0 Feature 1: Load recent audit entries from SQLite on mount
    onMount(async () => {
        try {
            const result = await queryAuditTrail({ limit: 500 });
            if (result.entries.length > 0) {
                auditTrail.set(result.entries);
            }
        } catch (e) {
            console.warn('[AuditTrailView] SQLite load failed, using localStorage fallback:', e);
        }
    });

    let filterHost   = 'all';
    let filterSource = 'all';
    let filterStatus = 'all';     // 'all' | 'success' | 'fail'
    let filterRange  = 'all';     // 'all' | '1h' | '24h' | '7d'
    let searchQuery  = '';

    // Resolve filter range to an absolute timestamp threshold
    function _rangeMs(range) {
        switch (range) {
            case '1h':  return Date.now() - 3600_000;
            case '24h': return Date.now() - 86400_000;
            case '7d':  return Date.now() - 7*86400_000;
            default: return 0; // 'all' — no lower bound
        }
    }
    function _entryTs(e) {
        const t = e?.timestamp;
        if (typeof t === 'number') return t;
        const parsed = Date.parse(t);
        return Number.isFinite(parsed) ? parsed : 0;
    }

    $: rangeFloor = _rangeMs(filterRange);
    $: entries = [...$auditTrail]
        .filter(e => filterHost   === 'all' || e.hostId === filterHost)
        .filter(e => filterSource === 'all' || e.source === filterSource)
        .filter(e => filterStatus === 'all'
            || (filterStatus === 'success' && e.exitCode === 0)
            || (filterStatus === 'fail' && e.exitCode !== null && e.exitCode !== 0))
        .filter(e => rangeFloor === 0 || _entryTs(e) >= rangeFloor)
        .filter(e => !searchQuery
            || e.command.toLowerCase().includes(searchQuery.toLowerCase())
            || e.hostName.toLowerCase().includes(searchQuery.toLowerCase())
            || e.outputPreview.toLowerCase().includes(searchQuery.toLowerCase()))
        .reverse();

    $: totalEntries   = $auditTrail.length;
    $: visibleEntries = entries.length;
    $: uniqueHosts    = [...new Set($auditTrail.map(e => e.hostId))];
    $: failedCount    = $auditTrail.filter(e => e.exitCode !== null && e.exitCode !== 0).length;
    $: sourceCounts   = $auditTrail.reduce((acc, e) => { acc[e.source] = (acc[e.source]||0)+1; return acc; }, {});

    // Activity heatmap: 24 buckets covering the visible range (or last 24h if 'all')
    $: heatmap = (() => {
        const N = 24;
        const span = filterRange === '1h' ? 3600_000
                   : filterRange === '24h' ? 86400_000
                   : filterRange === '7d' ? 7*86400_000
                   : 86400_000; // 'all' → just show last 24h activity
        const now = Date.now();
        const bucketSize = span / N;
        const buckets = new Array(N).fill(0);
        const failed  = new Array(N).fill(0);
        for (const e of $auditTrail) {
            const ts = _entryTs(e);
            const age = now - ts;
            if (age < 0 || age > span) continue;
            const idx = N - 1 - Math.min(N-1, Math.floor(age / bucketSize));
            buckets[idx]++;
            if (e.exitCode !== null && e.exitCode !== 0) failed[idx]++;
        }
        const max = Math.max(1, ...buckets);
        return { buckets, failed, max, span };
    })();
    function _heatLabel() {
        const span = heatmap.span;
        if (span <= 3600_000)  return isEN ? 'Last hour'  : 'Última hora';
        if (span <= 86400_000) return isEN ? 'Last 24h'   : 'Últimas 24h';
        return isEN ? 'Last 7 days' : 'Últimos 7 días';
    }

    function toast(msg, type='info') { dispatch('toast', { msg, type }); }

    function clearTrail() {
        if (entries.length === 0) return;
        auditTrail.set([]);
        toast(isEN ? 'Audit trail cleared' : 'Registro de auditoría limpiado');
    }

    function exportTrail() {
        const csv = ['Timestamp,Host,Command,Source,ExitCode,Duration(ms),Output']
            .concat($auditTrail.map(e =>
                `"${e.timestamp}","${e.hostName}","${(e.command||'').replace(/"/g,'""')}","${e.source}",${e.exitCode??''},${e.durationMs??''},"${(e.outputPreview||'').replace(/"/g,'""').substring(0,200)}"`
            )).join('\n');
        const blob = new Blob([csv], { type: 'text/csv' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url; a.download = `lucy-audit-${new Date().toISOString().split('T')[0]}.csv`;
        a.click(); URL.revokeObjectURL(url);
        toast(isEN ? 'Audit trail exported as CSV' : 'Registro exportado como CSV');
    }

    let exportingPdf = false;
    async function exportPdf() {
        exportingPdf = true;
        try {
            await exportAuditPdf({ entries: $auditTrail }, isEN);
            toast(isEN ? 'PDF exported' : 'PDF exportado');
        } catch(e) {
            if (String(e) !== 'Cancelled') toast('Error: ' + e, 'error');
        }
        exportingPdf = false;
    }

    function fmtDate(ts) {
        try { return new Date(ts).toLocaleString(); } catch { return ts; }
    }

    function sourceIcon(s) {
        if (s === 'ai') return '✦';
        if (s === 'runbook') return '≡';
        if (s === 'compliance') return '⬡';
        if (s === 'broadcast') return '◎';
        return '⌨';
    }
</script>

<div class="view-wrap">
  <div class="view-hdr">
    <div class="view-title"><ClipboardList size={13} strokeWidth={2}/> {isEN ? 'Audit Trail' : 'Registro de Auditoría'}</div>
    <div style="display:flex;align-items:center;gap:8px;margin-left:auto;flex-wrap:wrap;">
      <select class="view-select" bind:value={filterHost}>
        <option value="all">{isEN ? 'All Hosts' : 'Todos los hosts'}</option>
        <option value="local">⊡ Local</option>
        {#each hosts as h}<option value={h.id}>{h.name}</option>{/each}
      </select>
      <select class="view-select" bind:value={filterSource}>
        <option value="all">{isEN ? 'All Sources' : 'Todas'}</option>
        <option value="manual">⌨ Manual</option>
        <option value="ai">✦ AI</option>
        <option value="runbook">≡ Runbook</option>
        <option value="compliance">⬡ Compliance</option>
        <option value="broadcast">◎ Broadcast</option>
      </select>
      <button class="view-btn" on:click={exportTrail} title="Export CSV" style="display:flex;align-items:center;gap:5px;"><Download size={12} strokeWidth={2}/> CSV</button>
      <button class="view-btn" on:click={exportPdf} disabled={exportingPdf} title="Export PDF" style="display:flex;align-items:center;gap:5px;">{#if exportingPdf}↻{:else}<FileText size={12} strokeWidth={2}/>{/if} PDF</button>
      <button class="view-btn" on:click={clearTrail} title="Clear" style="display:flex;align-items:center;justify-content:center;padding:4px 8px;"><Trash2 size={13} strokeWidth={1.8}/></button>
    </div>
  </div>

  <!-- Stats bar -->
  <div class="at-stats">
    <span class="at-stat">{visibleEntries} / {totalEntries} {isEN ? 'entries' : 'entradas'}</span>
    <span class="at-stat">{uniqueHosts.length} hosts</span>
    {#if failedCount > 0}<span class="at-stat" style="color:var(--red)">{failedCount} {isEN ? 'failed' : 'fallidos'}</span>{/if}
    {#each Object.entries(sourceCounts) as [src, cnt]}
      <span class="at-stat">{sourceIcon(src)} {cnt}</span>
    {/each}
  </div>

  <!-- Activity heatmap (24 buckets over the active range) -->
  <div class="at-heatmap" title={isEN ? 'Activity over time — failed runs in red' : 'Actividad en el tiempo — rojos = fallos'}>
    <span class="at-heat-label">{_heatLabel()}</span>
    <div class="at-heat-bars">
      {#each heatmap.buckets as count, i}
        <div class="at-heat-bar"
          style="height:{Math.max(2, (count / heatmap.max) * 22)}px;background:{heatmap.failed[i] > 0 ? 'rgba(239,68,68,.55)' : count > 0 ? 'var(--acc)' : 'rgba(255,255,255,.04)'};"
          title="{count} {isEN ? 'commands' : 'comandos'}{heatmap.failed[i] > 0 ? ` · ${heatmap.failed[i]} ${isEN ? 'failed' : 'fallidos'}` : ''}"></div>
      {/each}
    </div>
    <span class="at-heat-meta">{heatmap.buckets.reduce((s,n)=>s+n,0)} {isEN ? 'cmd' : 'cmd'}</span>
  </div>

  <!-- Quick filter chips -->
  <div class="at-quick-filters">
    <span class="at-qf-label">{isEN ? 'Range:' : 'Rango:'}</span>
    {#each [['all', isEN?'All':'Todo'], ['1h', '1h'], ['24h', '24h'], ['7d', '7d']] as [val, label]}
      <button class="at-qf-chip" class:active={filterRange === val} on:click={() => filterRange = val}>{label}</button>
    {/each}
    <span class="at-qf-sep"></span>
    <span class="at-qf-label">{isEN ? 'Status:' : 'Estado:'}</span>
    <button class="at-qf-chip" class:active={filterStatus === 'all'}     on:click={() => filterStatus = 'all'}>{isEN ? 'All' : 'Todo'}</button>
    <button class="at-qf-chip at-qf-ok" class:active={filterStatus === 'success'} on:click={() => filterStatus = 'success'}>✓ OK</button>
    <button class="at-qf-chip at-qf-err" class:active={filterStatus === 'fail'}    on:click={() => filterStatus = 'fail'}>✗ {isEN ? 'Failed' : 'Fallidos'}</button>
  </div>

  <!-- Search -->
  <div class="at-search">
    <input class="at-search-inp" type="text" bind:value={searchQuery}
      placeholder={isEN ? 'Search commands, hosts, output...' : 'Buscar comandos, hosts, salida...'}>
    {#if searchQuery}
      <button class="at-search-clear" on:click={() => searchQuery = ''} title={isEN ? 'Clear' : 'Limpiar'}>✕</button>
    {/if}
  </div>

  <div class="at-scroll">
    {#each entries as e, i (e.id ?? `${e.timestamp}-${i}`)}
    <div class="at-entry" class:fail={e.exitCode !== null && e.exitCode !== 0}
         in:staggerIn={{ index: i, step: 24, duration: 220 }}>
      <div class="at-entry-meta">
        <span class="at-ts">{fmtDate(e.timestamp)}</span>
        <span class="at-host">{e.hostName}</span>
        <span class="at-source">{sourceIcon(e.source)} {e.source}</span>
        {#if e.exitCode !== null}
          <span class="at-exit" style="color:{e.exitCode===0?'var(--acc)':'var(--red)'}">
            {e.exitCode === 0 ? '✓' : '✗'} {e.exitCode}
          </span>
        {/if}
        {#if e.durationMs !== null}
          <span class="at-dur">{e.durationMs < 1000 ? e.durationMs + 'ms' : (e.durationMs/1000).toFixed(1) + 's'}</span>
        {/if}
      </div>
      <div class="at-cmd">{e.command}</div>
      {#if e.outputPreview}
      <details class="at-output">
        <summary>{isEN ? 'Output' : 'Salida'} ({e.outputPreview.length} chars)</summary>
        <pre>{e.outputPreview}</pre>
      </details>
      {/if}
    </div>
    {/each}
    {#if !entries.length}
    <div style="text-align:center;color:#334155;padding:40px;font-size:13px;">
      {isEN ? 'No audit entries yet. Commands executed in NexShell and Terminal will appear here.' : 'Sin entradas. Los comandos ejecutados en NexShell y Terminal aparecerán aquí.'}
    </div>
    {/if}
  </div>
</div>

<style>
    .view-wrap{flex:1;display:flex;flex-direction:column;overflow:hidden;min-height:0;}
    .view-hdr{display:flex;align-items:center;padding:10px 16px;background:rgba(2,4,8,.6);border-bottom:1px solid var(--bdr);flex-shrink:0;gap:10px;flex-wrap:wrap;}
    .view-title{font-size:13px;font-weight:700;color:var(--txt);white-space:nowrap;}
    .view-select{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt);font-size:11px;padding:3px 6px;cursor:pointer;outline:none;}
    .view-btn{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt2);font-size:12px;padding:4px 10px;cursor:pointer;transition:.15s;white-space:nowrap;}
    .view-btn:hover{background:var(--bdr2);color:var(--txt);}

    .at-stats{display:flex;gap:12px;padding:8px 16px;font-size:11px;color:#4a5a6a;border-bottom:1px solid rgba(26,32,48,.3);flex-shrink:0;}
    .at-stat{display:flex;align-items:center;gap:4px;}

    /* Activity heatmap row */
    .at-heatmap{display:flex;align-items:center;gap:10px;padding:6px 16px 6px;border-bottom:1px solid rgba(26,32,48,.3);flex-shrink:0;}
    .at-heat-label{font-size:9.5px;color:var(--txt3);font-family:var(--mono);text-transform:uppercase;letter-spacing:.5px;flex-shrink:0;min-width:80px;}
    .at-heat-bars{flex:1;display:flex;align-items:flex-end;gap:1.5px;height:24px;background:rgba(0,0,0,.18);padding:1px 4px;border-radius:4px;}
    .at-heat-bar{flex:1;border-radius:1px;transition:opacity .15s;cursor:default;min-height:2px;}
    .at-heat-bar:hover{opacity:.7;}
    .at-heat-meta{font-size:9.5px;color:var(--txt3);font-family:var(--mono);flex-shrink:0;min-width:60px;text-align:right;}

    /* Quick filter chips */
    .at-quick-filters{display:flex;align-items:center;gap:4px;padding:6px 16px;border-bottom:1px solid rgba(26,32,48,.3);flex-shrink:0;flex-wrap:wrap;}
    .at-qf-label{font-size:10px;color:var(--txt3);font-weight:600;letter-spacing:.3px;margin-right:2px;}
    .at-qf-sep{width:1px;height:14px;background:rgba(255,255,255,.08);margin:0 6px;}
    .at-qf-chip{
      background:transparent;border:1px solid rgba(255,255,255,.06);
      color:var(--txt2);font-size:10.5px;font-family:var(--mono);font-weight:600;
      padding:2px 8px;border-radius:9px;cursor:pointer;transition:.12s;
    }
    .at-qf-chip:hover{background:rgba(255,255,255,.04);color:var(--txt);}
    .at-qf-chip.active{background:rgba(16,185,129,.10);border-color:rgba(16,185,129,.35);color:var(--acc);}
    .at-qf-chip.at-qf-ok.active{background:rgba(16,185,129,.10);border-color:rgba(16,185,129,.35);color:var(--acc);}
    .at-qf-chip.at-qf-err.active{background:rgba(239,68,68,.10);border-color:rgba(239,68,68,.35);color:var(--red);}

    .at-search{padding:8px 16px;flex-shrink:0;position:relative;}
    .at-search-inp{width:100%;background:rgba(0,0,0,.2);border:1px solid var(--bdr);color:var(--txt);padding:6px 28px 6px 10px;border-radius:6px;font-size:12px;outline:none;font-family:inherit;}
    .at-search-inp:focus{border-color:var(--acc-b);}
    .at-search-clear{
      position:absolute;right:22px;top:50%;transform:translateY(-50%);
      background:transparent;border:none;color:var(--txt3);cursor:pointer;
      font-size:11px;padding:2px 6px;border-radius:3px;
    }
    .at-search-clear:hover{background:rgba(255,255,255,.06);color:var(--txt);}

    .at-scroll{flex:1;overflow-y:auto;padding:0 16px 16px;}

    .at-entry{border:1px solid var(--bdr);border-radius:6px;padding:8px 10px;margin-bottom:6px;transition:.15s;}
    .at-entry:hover{border-color:rgba(255,255,255,.06);}
    .at-entry.fail{border-left:2px solid var(--red);background:rgba(255,68,68,.02);}
    .at-entry-meta{display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin-bottom:4px;}
    .at-ts{font-size:10px;color:#475569;font-family:var(--mono);}
    .at-host{font-size:10px;color:var(--blue);font-weight:600;}
    .at-source{font-size:10px;color:#4a5a6a;}
    .at-exit{font-size:10px;font-weight:700;font-family:var(--mono);}
    .at-dur{font-size:10px;color:#475569;font-family:var(--mono);}
    .at-cmd{font-size:12px;color:var(--txt);font-family:var(--mono);word-break:break-all;line-height:1.5;}
    .at-output{margin-top:4px;}
    .at-output summary{font-size:10px;color:#4a5a6a;cursor:pointer;}
    .at-output pre{font-size:10px;color:#6a7a8a;font-family:var(--mono);background:rgba(0,0,0,.2);padding:6px 8px;border-radius:4px;margin-top:4px;overflow-x:auto;white-space:pre-wrap;word-break:break-all;max-height:200px;overflow-y:auto;}

    :global(:root.light) .at-entry{background:#fff;}
    :global(:root.light) .at-entry.fail{background:rgba(255,68,68,.04);}
</style>
