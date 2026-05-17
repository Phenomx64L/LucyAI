<script>
    import { onMount, createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import ScanSearch from '@tabler/icons-svelte/icons/scan';

    import FileText from '@tabler/icons-svelte/icons/file-text';

    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
    import { inventorySnapshots } from '$lib/stores';
    import { exportInventoryPdf } from '$lib/reports/ReportGenerator';

    const dispatch = createEventDispatcher();

    export let hosts    = [];
    export let hostName = '';
    export let isEN     = false;

    let selectedHost = 'local';
    let scanning     = false;
    let error        = '';
    let activeTab    = 'ports';
    let searchQuery  = '';

    $: snapshot = $inventorySnapshots[selectedHost] || null;

    $: filteredPorts = (snapshot?.ports || []).filter(p =>
        !searchQuery || String(p.port).includes(searchQuery) || (p.process||'').toLowerCase().includes(searchQuery.toLowerCase())
    );
    $: filteredServices = (snapshot?.services || []).filter(s =>
        !searchQuery || s.name.toLowerCase().includes(searchQuery.toLowerCase()) || (s.description||'').toLowerCase().includes(searchQuery.toLowerCase())
    );
    $: filteredSoftware = (snapshot?.software || []).filter(s =>
        !searchQuery || s.name.toLowerCase().includes(searchQuery.toLowerCase())
    );

    function toast(msg, type='info') { dispatch('toast', { msg, type }); }

    async function runScan() {
        scanning = true; error = '';
        try {
            let data;
            if (selectedHost === 'local') {
                data = await invoke('discover_inventory_local');
            } else {
                const h = hosts.find(x => x.id === selectedHost);
                if (!h) { error = 'Host not found'; scanning = false; return; }
                let pwd = '';
                try { pwd = await invoke('get_host_credential', { hostId: h.id }); } catch(e){}
                if (h.type === 'windows') {
                    data = await invoke('discover_inventory_windows', { host: h.host, username: h.username, password: pwd });
                } else {
                    data = await invoke('discover_inventory_linux', { host: h.host, username: h.username, port: h.port||22, keyPath: h.sshKeyPath||null });
                }
            }
            const snap = {
                hostId: selectedHost,
                hostName: selectedHost === 'local' ? hostName : (hosts.find(h=>h.id===selectedHost)?.name || selectedHost),
                timestamp: Date.now(),
                ports:     Array.isArray(data.ports)     ? data.ports     : [],
                services:  Array.isArray(data.services)  ? data.services  : [],
                software:  Array.isArray(data.software)  ? data.software  : [],
                certs:     Array.isArray(data.certs)      ? data.certs     : [],
                scheduled: Array.isArray(data.scheduled) ? data.scheduled : [],
            };
            $inventorySnapshots = { ...$inventorySnapshots, [selectedHost]: snap };
            toast(isEN ? `Inventory scan completed: ${snap.ports.length} ports, ${snap.services.length} services` : `Inventario completado: ${snap.ports.length} puertos, ${snap.services.length} servicios`);
        } catch(e) {
            error = String(e);
        }
        scanning = false;
    }

    let exporting = false;
    async function exportPdf() {
        if (!snapshot) return;
        exporting = true;
        try {
            await exportInventoryPdf({
                hostName: snapshot.hostName || selectedHost,
                scanDate: new Date(snapshot.timestamp).toLocaleString(),
                ports: snapshot.ports || [],
                services: snapshot.services || [],
                software: snapshot.software || [],
                certs: snapshot.certs || [],
                scheduled: snapshot.scheduled || [],
            }, isEN);
            toast(isEN ? 'PDF exported' : 'PDF exportado');
        } catch(e) {
            if (String(e) !== 'Cancelled') toast('Error: ' + e, 'error');
        }
        exporting = false;
    }

    function relTime(ts) {
        if (!ts) return '';
        const d = Date.now() - ts;
        if (d < 60000) return isEN ? 'just now' : 'ahora';
        if (d < 3600000) return `${Math.floor(d/60000)}m`;
        if (d < 86400000) return `${Math.floor(d/3600000)}h`;
        return `${Math.floor(d/86400000)}d`;
    }
</script>

<div class="view-wrap">
  <div class="view-hdr">
    <div class="view-title" style="display:flex;align-items:center;gap:6px;"><ScanSearch size={13} strokeWidth={2}/> {isEN ? 'Infrastructure Inventory' : 'Inventario de Infraestructura'}</div>
    <div style="display:flex;align-items:center;gap:8px;margin-left:auto;flex-wrap:wrap;">
      <select class="view-select" bind:value={selectedHost}>
        <option value="local">⊡ Local ({hostName})</option>
        {#each hosts as h}<option value={h.id}>{h.type==='windows'?'⊡':'◈'} {h.name}</option>{/each}
      </select>
      <button class="view-btn" on:click={runScan} disabled={scanning} style="display:flex;align-items:center;gap:5px;">
        {#if scanning}↻ {isEN ? 'Scanning...' : 'Escaneando...'}{:else}<ScanSearch size={12} strokeWidth={2}/> {isEN ? 'Scan' : 'Escanear'}{/if}
      </button>
      {#if snapshot}
        <button class="view-btn" on:click={exportPdf} disabled={exporting} title="Export PDF" style="display:flex;align-items:center;gap:5px;">
          {#if exporting}↻{:else}<FileText size={12} strokeWidth={2}/>{/if} PDF
        </button>
        <span style="font-size:10px;color:#4a5a6a;">{relTime(snapshot.timestamp)}</span>
      {/if}
    </div>
  </div>

  {#if error}
    <div class="view-error" style="display:flex;align-items:center;gap:6px;"><AlertTriangle size={12} strokeWidth={2}/> {error}</div>
  {/if}

  {#if snapshot}
  <!-- Summary cards -->
  <div class="inv-summary">
    <button class="inv-card" class:active={activeTab==='ports'} on:click={() => activeTab='ports'}>
      <span class="inv-card-num">{snapshot.ports.length}</span>
      <span class="inv-card-lbl">{isEN ? 'Open Ports' : 'Puertos'}</span>
    </button>
    <button class="inv-card" class:active={activeTab==='services'} on:click={() => activeTab='services'}>
      <span class="inv-card-num">{snapshot.services.length}</span>
      <span class="inv-card-lbl">{isEN ? 'Services' : 'Servicios'}</span>
    </button>
    <button class="inv-card" class:active={activeTab==='software'} on:click={() => activeTab='software'}>
      <span class="inv-card-num">{snapshot.software.length}</span>
      <span class="inv-card-lbl">Software</span>
    </button>
    <button class="inv-card" class:active={activeTab==='certs'} on:click={() => activeTab='certs'}>
      <span class="inv-card-num" style={snapshot.certs.some(c=>c.days_left<30)?'color:var(--red)':''}>{snapshot.certs.length}</span>
      <span class="inv-card-lbl">{isEN ? 'SSL Certs' : 'Certificados'}</span>
    </button>
    <button class="inv-card" class:active={activeTab==='scheduled'} on:click={() => activeTab='scheduled'}>
      <span class="inv-card-num">{snapshot.scheduled.length}</span>
      <span class="inv-card-lbl">{isEN ? 'Scheduled' : 'Programadas'}</span>
    </button>
  </div>

  <!-- Search -->
  <div class="inv-search">
    <input class="inv-search-inp" type="text" bind:value={searchQuery}
      placeholder={isEN ? 'Filter...' : 'Filtrar...'}>
  </div>

  <div class="inv-scroll">
    {#if activeTab === 'ports'}
    <table class="inv-table">
      <thead><tr><th>{isEN ? 'Port' : 'Puerto'}</th><th>{isEN ? 'Process' : 'Proceso'}</th><th>{isEN ? 'State' : 'Estado'}</th></tr></thead>
      <tbody>
        {#each filteredPorts as p}
        <tr><td class="mono">{p.port}</td><td>{p.process || '-'}</td><td><span class="badge ok">LISTEN</span></td></tr>
        {/each}
        {#if !filteredPorts.length}<tr><td colspan="3" class="empty">{isEN ? 'No ports found' : 'Sin puertos'}</td></tr>{/if}
      </tbody>
    </table>

    {:else if activeTab === 'services'}
    <table class="inv-table">
      <thead><tr><th>{isEN ? 'Service' : 'Servicio'}</th><th>{isEN ? 'Description' : 'Descripción'}</th><th>{isEN ? 'Status' : 'Estado'}</th></tr></thead>
      <tbody>
        {#each filteredServices as s}
        <tr><td class="mono">{s.name}</td><td>{s.description || '-'}</td><td><span class="badge ok">{s.status}</span></td></tr>
        {/each}
        {#if !filteredServices.length}<tr><td colspan="3" class="empty">{isEN ? 'No services found' : 'Sin servicios'}</td></tr>{/if}
      </tbody>
    </table>

    {:else if activeTab === 'software'}
    <table class="inv-table">
      <thead><tr><th>Software</th><th>{isEN ? 'Version' : 'Versión'}</th></tr></thead>
      <tbody>
        {#each filteredSoftware as s}
        <tr><td>{s.name}</td><td class="mono">{s.version || '-'}</td></tr>
        {/each}
        {#if !filteredSoftware.length}<tr><td colspan="2" class="empty">{isEN ? 'No software found' : 'Sin software'}</td></tr>{/if}
      </tbody>
    </table>

    {:else if activeTab === 'certs'}
    <table class="inv-table">
      <thead><tr><th>Subject</th><th>{isEN ? 'Expires' : 'Expira'}</th><th>{isEN ? 'Days Left' : 'Días rest.'}</th></tr></thead>
      <tbody>
        {#each snapshot.certs as c}
        <tr>
          <td style="font-size:11px;">{c.subject || c.path}</td>
          <td class="mono">{c.expires}</td>
          <td style="font-weight:700;color:{c.days_left<0?'var(--red)':c.days_left<30?'var(--amber)':'var(--acc)'}">
            {c.days_left}d
          </td>
        </tr>
        {/each}
        {#if !snapshot.certs.length}<tr><td colspan="3" class="empty">{isEN ? 'No certificates found' : 'Sin certificados'}</td></tr>{/if}
      </tbody>
    </table>

    {:else if activeTab === 'scheduled'}
    <table class="inv-table">
      <thead><tr><th>{isEN ? 'Task / Cron Entry' : 'Tarea / Entrada Cron'}</th></tr></thead>
      <tbody>
        {#each snapshot.scheduled as s}
        <tr><td class="mono" style="font-size:11px;">{s.entry}</td></tr>
        {/each}
        {#if !snapshot.scheduled.length}<tr><td class="empty">{isEN ? 'No scheduled tasks' : 'Sin tareas programadas'}</td></tr>{/if}
      </tbody>
    </table>
    {/if}
  </div>
  {:else if !scanning}
  <div class="view-loading"><span style="color:#334155">{isEN ? 'Select a host and click Scan' : 'Selecciona un host y haz clic en Escanear'}</span></div>
  {:else}
  <div class="view-loading"><span style="color:var(--acc)">↻ {isEN ? 'Scanning...' : 'Escaneando...'}</span></div>
  {/if}
</div>

<style>
    .view-wrap{flex:1;display:flex;flex-direction:column;overflow:hidden;min-height:0;}
    .view-hdr{display:flex;align-items:center;padding:10px 16px;background:rgba(2,4,8,.6);border-bottom:1px solid var(--bdr);flex-shrink:0;gap:10px;}
    .view-title{font-size:13px;font-weight:700;color:var(--txt);white-space:nowrap;}
    .view-select{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt);font-size:12px;padding:4px 8px;cursor:pointer;outline:none;}
    .view-btn{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt2);font-size:12px;padding:4px 10px;cursor:pointer;transition:.15s;white-space:nowrap;}
    .view-btn:hover{background:var(--bdr2);color:var(--txt);}
    .view-btn:disabled{opacity:.35;cursor:not-allowed;}
    .view-error{margin:12px 16px;padding:10px 14px;background:rgba(255,68,68,.08);border:1px solid rgba(255,68,68,.2);border-radius:6px;font-size:12px;color:var(--red);}
    .view-loading{flex:1;display:flex;align-items:center;justify-content:center;font-size:13px;}

    .inv-summary{display:flex;gap:8px;padding:12px 16px;flex-shrink:0;overflow-x:auto;}
    .inv-card{background:rgba(0,0,0,.2);border:1px solid var(--bdr);border-radius:8px;padding:10px 16px;cursor:pointer;transition:.15s;display:flex;flex-direction:column;align-items:center;gap:2px;min-width:80px;flex-shrink:0;}
    .inv-card:hover{border-color:rgba(255,255,255,.08);}
    .inv-card.active{border-color:var(--acc);background:rgba(16,185,129,.05);box-shadow:0 0 12px rgba(16,185,129,.08);}
    .inv-card-num{font-size:22px;font-weight:300;color:var(--txt);}
    .inv-card-lbl{font-size:10px;color:#4a5a6a;text-transform:uppercase;letter-spacing:.3px;font-weight:600;}

    .inv-search{padding:0 16px 8px;flex-shrink:0;}
    .inv-search-inp{width:100%;background:rgba(0,0,0,.2);border:1px solid var(--bdr);color:var(--txt);padding:6px 10px;border-radius:6px;font-size:12px;outline:none;font-family:inherit;}
    .inv-search-inp:focus{border-color:var(--acc-b);}

    .inv-scroll{flex:1;overflow-y:auto;padding:0 16px 16px;}

    .inv-table{width:100%;border-collapse:collapse;font-size:12px;}
    .inv-table th{background:rgba(0,0,0,.15);color:#475569;padding:6px 10px;text-align:left;font-size:10px;font-weight:700;letter-spacing:.3px;text-transform:uppercase;position:sticky;top:0;z-index:1;}
    .inv-table td{padding:5px 10px;border-bottom:1px solid rgba(26,32,48,.3);color:var(--txt2);}
    .inv-table tr:hover td{background:rgba(16,185,129,.02);}
    .mono{font-family:var(--mono);font-size:11px;}
    .badge{font-size:10px;padding:1px 6px;border-radius:8px;font-weight:600;}
    .badge.ok{background:rgba(16,185,129,.08);color:var(--acc);}
    .empty{text-align:center;color:#334155;padding:20px!important;font-style:italic;}

    :global(:root.light) .inv-card{background:#fff;}
    :global(:root.light) .inv-table th{background:var(--bg3);}
</style>
