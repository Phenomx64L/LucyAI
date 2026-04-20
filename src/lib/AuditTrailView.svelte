<script>
    import { createEventDispatcher } from 'svelte';
    import { auditTrail } from '$lib/stores';
    import { exportAuditPdf } from '$lib/reports/ReportGenerator';
    import { IconClipboardList as ClipboardList, IconDownload as Download, IconTrash as Trash2, IconFileText as FileText, IconSparkles as Sparkles, IconBook2 as BookOpen, IconShieldCheck as ShieldCheck, IconRadio as Radio, IconKeyboard as Keyboard, IconAlertTriangle as AlertTriangle } from '@tabler/icons-svelte';

    const dispatch = createEventDispatcher();

    export let hosts    = [];
    export let isEN     = false;

    let filterHost   = 'all';
    let filterSource = 'all';
    let searchQuery  = '';

    $: entries = [...$auditTrail]
        .filter(e => filterHost   === 'all' || e.hostId === filterHost)
        .filter(e => filterSource === 'all' || e.source === filterSource)
        .filter(e => !searchQuery || e.command.toLowerCase().includes(searchQuery.toLowerCase()) || e.hostName.toLowerCase().includes(searchQuery.toLowerCase()) || e.outputPreview.toLowerCase().includes(searchQuery.toLowerCase()))
        .reverse();

    $: totalEntries   = $auditTrail.length;
    $: uniqueHosts    = [...new Set($auditTrail.map(e => e.hostId))];
    $: failedCount    = $auditTrail.filter(e => e.exitCode !== null && e.exitCode !== 0).length;
    $: sourceCounts   = $auditTrail.reduce((acc, e) => { acc[e.source] = (acc[e.source]||0)+1; return acc; }, {});

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
    <span class="at-stat">{totalEntries} {isEN ? 'entries' : 'entradas'}</span>
    <span class="at-stat">{uniqueHosts.length} hosts</span>
    {#if failedCount > 0}<span class="at-stat" style="color:var(--red)">{failedCount} {isEN ? 'failed' : 'fallidos'}</span>{/if}
    {#each Object.entries(sourceCounts) as [src, cnt]}
      <span class="at-stat">{sourceIcon(src)} {cnt}</span>
    {/each}
  </div>

  <!-- Search -->
  <div class="at-search">
    <input class="at-search-inp" type="text" bind:value={searchQuery}
      placeholder={isEN ? 'Search commands, hosts, output...' : 'Buscar comandos, hosts, salida...'}>
  </div>

  <div class="at-scroll">
    {#each entries as e, i}
    <div class="at-entry" class:fail={e.exitCode !== null && e.exitCode !== 0}>
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

    .at-search{padding:8px 16px;flex-shrink:0;}
    .at-search-inp{width:100%;background:rgba(0,0,0,.2);border:1px solid var(--bdr);color:var(--txt);padding:6px 10px;border-radius:6px;font-size:12px;outline:none;font-family:inherit;}
    .at-search-inp:focus{border-color:var(--acc-b);}

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
