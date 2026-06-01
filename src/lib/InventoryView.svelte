<script>
    import { onMount, createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    // v1.7.17 — In-app dialogs.
    import { lucyConfirm, lucyPrompt } from '$lib/dialog-service';
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

    // ── Sprint B #2 — Drift detection ──────────────────────────────────
    // Lets the user pin "this is the trusted state" then see what's
    // drifted since. Per-host baseline stored in inventory_baselines.
    let baseline = null;        // InventoryBaseline | null
    let driftReport = null;     // DriftReport | null
    let driftLoading = false;
    let driftError = '';

    async function refreshBaselineInfo() {
        try {
            baseline = await invoke('inventory_get_baseline', { hostId: selectedHost });
        } catch {
            baseline = null;
        }
    }

    async function saveBaseline() {
        if (!snapshot) {
            toast(isEN ? 'Run a scan first.' : 'Escanea primero.', 'warn');
            return;
        }
        const label = await lucyPrompt(
            isEN ? 'Label for this baseline (optional)' : 'Etiqueta para este baseline (opcional)',
            { defaultValue: new Date().toISOString().slice(0, 10),
              placeholder: 'YYYY-MM-DD' });
        if (label === null) return;
        try {
            await invoke('inventory_set_baseline', {
                hostId: selectedHost,
                snapshot: {
                    ports: snapshot.ports,
                    services: snapshot.services,
                    software: snapshot.software,
                    certs: snapshot.certs,
                    scheduled: snapshot.scheduled,
                },
                label: label || '',
            });
            await refreshBaselineInfo();
            driftReport = null; // reset — current snapshot IS the new baseline now
            toast(isEN
                ? `Baseline saved (${snapshot.software.length} software, ${snapshot.ports.length} ports).`
                : `Baseline guardado (${snapshot.software.length} software, ${snapshot.ports.length} puertos).`);
        } catch (e) {
            toast('Save error: ' + e, 'error');
        }
    }

    async function computeDrift() {
        if (!snapshot) {
            toast(isEN ? 'Run a scan first.' : 'Escanea primero.', 'warn');
            return;
        }
        if (!baseline) {
            toast(isEN ? 'No baseline saved for this host yet.' : 'Sin baseline guardado para este host.', 'warn');
            return;
        }
        driftLoading = true; driftError = '';
        try {
            driftReport = await invoke('inventory_compute_drift', {
                hostId: selectedHost,
                currentSnapshot: {
                    ports: snapshot.ports,
                    services: snapshot.services,
                    software: snapshot.software,
                    certs: snapshot.certs,
                    scheduled: snapshot.scheduled,
                },
            });
            activeTab = 'drift';
            const t = driftReport.totals;
            const n = t.added + t.removed + t.changed;
            toast(n === 0
                ? (isEN ? '✓ No drift detected since baseline.' : '✓ Sin deriva desde el baseline.')
                : (isEN ? `Drift: +${t.added} / -${t.removed} / Δ${t.changed}`
                        : `Deriva: +${t.added} / -${t.removed} / Δ${t.changed}`),
                n === 0 ? 'info' : 'warn',
            );
        } catch (e) {
            driftError = String(e);
        }
        driftLoading = false;
    }

    async function deleteBaseline() {
        if (!await lucyConfirm(
            isEN ? 'Delete the baseline for this host?' : '¿Borrar el baseline de este host?',
            { tone: 'danger',
              description: isEN ? 'This cannot be undone.' : 'No se puede deshacer.',
              confirmLabel: isEN ? 'Delete' : 'Borrar' })) return;
        try {
            await invoke('inventory_delete_baseline', { hostId: selectedHost });
            baseline = null;
            driftReport = null;
            toast(isEN ? 'Baseline removed.' : 'Baseline borrado.');
        } catch (e) {
            toast('Delete error: ' + e, 'error');
        }
    }

    function fmtRelTime(unixSec) {
        const d = Math.floor(Date.now()/1000) - unixSec;
        if (d < 60)    return isEN ? 'just now' : 'ahora';
        if (d < 3600)  return `${Math.floor(d/60)}m`;
        if (d < 86400) return `${Math.floor(d/3600)}h`;
        return `${Math.floor(d/86400)}d`;
    }

    // Auto-refresh baseline info when the user switches host.
    $: if (selectedHost) {
        refreshBaselineInfo();
        // Stale drift report when host changes — could be misleading
        driftReport = null;
    }

    // ── Tier A #4 (sprint extra) — CVE scan against curated DB ──────────
    // Sends the current snapshot's `software[]` to the backend cve_scan
    // command and renders the result as a new tab. Pure-local, no cloud.
    let cveScanning = false;
    let cveResult   = null;  // { matches: [...], scanned_count, db_size, stale_warning }

    async function runCveScan() {
        if (!snapshot?.software?.length) {
            toast(isEN ? 'No software in snapshot — scan inventory first.'
                       : 'Sin software en el snapshot — escanea inventario primero.', 'warn');
            return;
        }
        cveScanning = true;
        try {
            const payload = snapshot.software.map(s => ({
                name: s.name || '',
                version: s.version || null,
            }));
            cveResult = await invoke('cve_scan', { software: payload });
            activeTab = 'cves';
            const n = cveResult.matches.length;
            const crit = cveResult.matches.filter(m => m.cve.severity === 'CRITICAL').length;
            toast(
                isEN
                    ? `CVE scan: ${n} matches (${crit} CRITICAL) across ${cveResult.scanned_count} products`
                    : `CVE scan: ${n} coincidencias (${crit} CRÍTICAS) sobre ${cveResult.scanned_count} productos`,
                n > 0 ? 'warn' : 'info',
            );
        } catch(e) {
            toast('CVE scan error: ' + e, 'error');
        }
        cveScanning = false;
    }

    function severityClass(sev) {
        if (sev === 'CRITICAL') return 'sev-crit';
        if (sev === 'HIGH')     return 'sev-high';
        if (sev === 'MEDIUM')   return 'sev-med';
        return 'sev-low';
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
        <!-- Sprint B #2 — Drift detection group: baseline save + drift compute -->
        {#if baseline}
          <button class="view-btn" on:click={computeDrift} disabled={driftLoading}
                  title={isEN
                    ? `Compute drift vs baseline (${fmtRelTime(baseline.updated_at)} old${baseline.label ? ': ' + baseline.label : ''})`
                    : `Calcular deriva vs baseline (${fmtRelTime(baseline.updated_at)} de antigüedad${baseline.label ? ': ' + baseline.label : ''})`}
                  style="display:flex;align-items:center;gap:5px;">
            {#if driftLoading}↻{:else}◑{/if} {isEN ? 'Drift' : 'Deriva'}
          </button>
        {/if}
        <button class="view-btn" on:click={saveBaseline}
                title={baseline
                  ? (isEN ? 'Overwrite baseline with current snapshot' : 'Sobrescribir baseline con snapshot actual')
                  : (isEN ? 'Save current snapshot as the trusted baseline for this host' : 'Guardar snapshot actual como baseline de confianza')}
                style="display:flex;align-items:center;gap:5px;">
          ◇ {baseline ? (isEN ? 'Update baseline' : 'Actualizar baseline') : (isEN ? 'Set baseline' : 'Set baseline')}
        </button>
        <!-- Tier A #4 — CVE scan trigger. Requires a snapshot with software list. -->
        <button class="view-btn" on:click={runCveScan} disabled={cveScanning || !snapshot.software?.length}
                title={isEN ? 'Scan installed software against curated CVE DB' : 'Escanear software contra DB curada de CVEs'}
                style="display:flex;align-items:center;gap:5px;">
          {#if cveScanning}↻{:else}🛡{/if} CVEs
        </button>
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
    {#if cveResult}
      <!-- Tier A #4 — CVE summary card. Color reflects worst severity. -->
      {@const _crit = cveResult.matches.filter(m => m.cve.severity === 'CRITICAL').length}
      {@const _high = cveResult.matches.filter(m => m.cve.severity === 'HIGH').length}
      <button class="inv-card inv-card-cve" class:active={activeTab==='cves'} on:click={() => activeTab='cves'}>
        <span class="inv-card-num" style={_crit > 0 ? 'color:#ef4444' : _high > 0 ? 'color:#f59e0b' : 'color:var(--acc)'}>
          {cveResult.matches.length}
        </span>
        <span class="inv-card-lbl">CVEs</span>
      </button>
    {/if}
    {#if driftReport}
      <!-- Sprint B #2 — Drift summary card. Color: green=clean, amber=changes, red=removed -->
      {@const _t = driftReport.totals}
      {@const _n = _t.added + _t.removed + _t.changed}
      {@const _color = _n === 0 ? 'color:var(--acc)' : _t.removed > 0 ? 'color:#ef4444' : 'color:#f59e0b'}
      <button class="inv-card inv-card-cve" class:active={activeTab==='drift'} on:click={() => activeTab='drift'}>
        <span class="inv-card-num" style={_color}>{_n}</span>
        <span class="inv-card-lbl">{isEN ? 'drift' : 'deriva'}</span>
      </button>
    {/if}
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

    {:else if activeTab === 'drift' && driftReport}
    <!-- Sprint B #2 — Drift detection report. -->
    {#if driftError}
      <div class="view-error" style="margin-bottom:8px;">{driftError}</div>
    {/if}
    <div class="drift-meta">
      {#if driftReport.has_baseline}
        {isEN ? 'Comparing against baseline saved' : 'Comparando contra baseline guardado'}
        <strong>{fmtRelTime(driftReport.baseline_age_secs > 0 ? Math.floor(Date.now()/1000) - driftReport.baseline_age_secs : Math.floor(Date.now()/1000))}</strong>
        {isEN ? 'ago' : 'antes'}{baseline?.label ? ` (${baseline.label})` : ''}
        ·
        <span style="color:var(--acc);">+{driftReport.totals.added}</span>
        <span style="color:#ef4444;">-{driftReport.totals.removed}</span>
        <span style="color:#f59e0b;">Δ{driftReport.totals.changed}</span>
        ·
        <button class="drift-mini-btn" on:click={deleteBaseline}>{isEN ? 'Forget baseline' : 'Borrar baseline'}</button>
      {:else}
        {isEN ? 'No baseline saved.' : 'Sin baseline guardado.'}
      {/if}
    </div>
    {#each driftReport.categories as cat}
      {#if cat.added.length + cat.removed.length + cat.changed.length > 0}
      <div class="drift-cat">
        <div class="drift-cat-hdr">
          <span class="drift-cat-name">{cat.name}</span>
          <span class="drift-cat-counts">
            {#if cat.added.length}<span class="drift-pill add">+{cat.added.length}</span>{/if}
            {#if cat.removed.length}<span class="drift-pill rem">−{cat.removed.length}</span>{/if}
            {#if cat.changed.length}<span class="drift-pill chg">Δ{cat.changed.length}</span>{/if}
          </span>
        </div>
        {#if cat.added.length}
          <div class="drift-list">
            <div class="drift-list-hdr">+ {isEN ? 'Added' : 'Añadido'}</div>
            {#each cat.added as item}
              <div class="drift-row add">
                <span class="drift-icon">+</span>
                <span class="drift-detail">
                  {#if cat.name === 'software'}{item.name} <span class="mono">{item.version || ''}</span>
                  {:else if cat.name === 'ports'}<span class="mono">{item.port}</span> · {item.process || '-'}
                  {:else if cat.name === 'services'}{item.name} <span class="mono">({item.status})</span>
                  {:else if cat.name === 'certs'}{item.subject || item.path}
                  {:else}<span class="mono">{item.entry}</span>
                  {/if}
                </span>
              </div>
            {/each}
          </div>
        {/if}
        {#if cat.removed.length}
          <div class="drift-list">
            <div class="drift-list-hdr">− {isEN ? 'Removed' : 'Removido'}</div>
            {#each cat.removed as item}
              <div class="drift-row rem">
                <span class="drift-icon">−</span>
                <span class="drift-detail">
                  {#if cat.name === 'software'}{item.name} <span class="mono">{item.version || ''}</span>
                  {:else if cat.name === 'ports'}<span class="mono">{item.port}</span> · {item.process || '-'}
                  {:else if cat.name === 'services'}{item.name} <span class="mono">({item.status})</span>
                  {:else if cat.name === 'certs'}{item.subject || item.path}
                  {:else}<span class="mono">{item.entry}</span>
                  {/if}
                </span>
              </div>
            {/each}
          </div>
        {/if}
        {#if cat.changed.length}
          <div class="drift-list">
            <div class="drift-list-hdr">Δ {isEN ? 'Changed' : 'Cambiado'}</div>
            {#each cat.changed as ch}
              <div class="drift-row chg">
                <span class="drift-icon">Δ</span>
                <span class="drift-detail">
                  <strong>{ch.identifier}</strong>
                  · <span class="mono">{ch.field}</span>:
                  <span class="mono" style="color:#ef4444;">{ch.from}</span>
                  →
                  <span class="mono" style="color:var(--acc);">{ch.to}</span>
                </span>
              </div>
            {/each}
          </div>
        {/if}
      </div>
      {/if}
    {/each}
    {#if driftReport.totals.added + driftReport.totals.removed + driftReport.totals.changed === 0}
      <div class="empty" style="color:var(--acc);">
        ✓ {isEN ? 'No drift detected since baseline.' : 'Sin deriva desde el baseline.'}
      </div>
    {/if}

    {:else if activeTab === 'cves' && cveResult}
    <!-- Tier A #4 — CVE matches. Curated DB scan results. -->
    <div class="cve-meta">
      {isEN ? 'Scanned' : 'Escaneados'}: {cveResult.scanned_count} ·
      DB: {cveResult.db_size} CVEs ·
      <span style="color:#94a3b8;">{cveResult.stale_warning}</span>
    </div>
    <table class="inv-table">
      <thead><tr>
        <th>{isEN ? 'Severity' : 'Severidad'}</th>
        <th>CVE</th>
        <th>{isEN ? 'Product' : 'Producto'}</th>
        <th>{isEN ? 'Installed' : 'Instalado'}</th>
        <th>{isEN ? 'Description' : 'Descripción'}</th>
        <th>CVSS</th>
      </tr></thead>
      <tbody>
        {#each cveResult.matches as m}
        <tr>
          <td><span class="badge {severityClass(m.cve.severity)}">{m.cve.severity}</span></td>
          <td class="mono"><a href="https://nvd.nist.gov/vuln/detail/{m.cve.cve_id}" target="_blank" rel="noopener noreferrer" style="color:var(--acc);text-decoration:none;">{m.cve.cve_id}</a></td>
          <td class="mono">{m.cve.product}</td>
          <td class="mono">{m.software_version}</td>
          <td style="font-size:11px;max-width:480px;">{m.cve.description}</td>
          <td class="mono" style="text-align:right;font-weight:600;">{m.cve.cvss_score.toFixed(1)}</td>
        </tr>
        {/each}
        {#if !cveResult.matches.length}
          <tr><td colspan="6" class="empty" style="color:var(--acc);">
            ✓ {isEN ? 'No known CVEs match the installed software in this snapshot.' : 'Sin CVEs conocidos contra el software instalado.'}
          </td></tr>
        {/if}
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
    .badge.sev-crit{background:rgba(239,68,68,.18);color:#ef4444;border:1px solid rgba(239,68,68,.35);}
    .badge.sev-high{background:rgba(245,158,11,.16);color:#f59e0b;border:1px solid rgba(245,158,11,.30);}
    .badge.sev-med {background:rgba(96,165,250,.14);color:#60a5fa;border:1px solid rgba(96,165,250,.25);}
    .badge.sev-low {background:rgba(148,163,184,.14);color:#94a3b8;}
    .empty{text-align:center;color:#334155;padding:20px!important;font-style:italic;}
    .cve-meta{padding:4px 0 10px;font-size:10px;color:#64748b;letter-spacing:0.3px;}
    /* Sprint B #2 — Drift visualization */
    .drift-meta{padding:6px 10px 10px;font-size:11px;color:var(--txt2);display:flex;gap:6px;align-items:center;flex-wrap:wrap;}
    .drift-mini-btn{background:transparent;border:1px solid var(--bdr);color:var(--txt2);font-size:10px;padding:2px 8px;border-radius:4px;cursor:pointer;}
    .drift-mini-btn:hover{background:rgba(239,68,68,.10);color:#ef4444;border-color:rgba(239,68,68,.30);}
    .drift-cat{margin-bottom:12px;background:rgba(0,0,0,.15);border:1px solid var(--bdr);border-radius:6px;padding:10px 12px;}
    .drift-cat-hdr{display:flex;align-items:center;gap:8px;margin-bottom:8px;}
    .drift-cat-name{font-size:11px;font-weight:700;color:var(--txt);text-transform:uppercase;letter-spacing:.3px;}
    .drift-cat-counts{display:flex;gap:4px;margin-left:auto;}
    .drift-pill{font-size:9px;padding:1px 6px;border-radius:8px;font-weight:600;font-family:var(--mono);font-variant-numeric:tabular-nums;}
    .drift-pill.add{background:rgba(16,185,129,.18);color:var(--acc);}
    .drift-pill.rem{background:rgba(239,68,68,.18);color:#ef4444;}
    .drift-pill.chg{background:rgba(245,158,11,.16);color:#f59e0b;}
    .drift-list{margin-top:4px;}
    .drift-list-hdr{font-size:9px;color:#64748b;text-transform:uppercase;letter-spacing:.4px;padding:6px 0 3px;}
    .drift-row{display:flex;gap:8px;padding:3px 0;font-size:11px;align-items:baseline;}
    .drift-row .drift-icon{width:14px;text-align:center;font-weight:700;flex-shrink:0;font-family:var(--mono);}
    .drift-row.add  .drift-icon{color:var(--acc);}
    .drift-row.rem  .drift-icon{color:#ef4444;}
    .drift-row.chg  .drift-icon{color:#f59e0b;}
    .drift-detail{color:var(--txt);}

    :global(:root.light) .inv-card{background:#fff;}
    :global(:root.light) .inv-table th{background:var(--bg3);}
</style>
