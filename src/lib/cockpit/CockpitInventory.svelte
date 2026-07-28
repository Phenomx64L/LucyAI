<script>
  /* ==========================================================================
     Lucy 2.0 — Inventory (cockpit)  ·  Phase F4/views. LIVE.
     Mirrors the classic InventoryView: a per-host ASSET SCAN of the local
     machine via `discover_inventory_local`, returning ports / services /
     software / certs / scheduled tasks. Auto-scans once on mount; the button
     re-scans. Representative data is the FALLBACK for a plain-browser preview
     (no Tauri backend). Local host only for now (remote needs host + creds).
     Additive — does not touch InventoryView.svelte.
     ========================================================================== */
  import { onMount } from 'svelte';
  // Nombre REAL del equipo — antes aquí había la constante de la maqueta.
  import { localHostname, ensureLocalHostname, hostLabel } from './host-info';
  import { invoke } from '@tauri-apps/api/core';
  import { copyToClipboard } from '$lib/lucy-api';
  import Server from '@tabler/icons-svelte/icons/server';
  import Search from '@tabler/icons-svelte/icons/search';
  import ScanSearch from '@tabler/icons-svelte/icons/scan';
  import ShieldCheck from '@tabler/icons-svelte/icons/shield-check';
  import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
  import Copy from '@tabler/icons-svelte/icons/copy';
  import Check from '@tabler/icons-svelte/icons/check';

  const CATS = [
    { id: 'ports',     label: 'Puertos' },
    { id: 'services',  label: 'Servicios' },
    { id: 'software',  label: 'Software' },
    { id: 'vulns',     label: 'Vulnerabilidades' },
    { id: 'certs',     label: 'Certificados' },
    { id: 'scheduled', label: 'Tareas' },
  ];
  const SEV_RANK = { CRITICAL: 0, HIGH: 1, MEDIUM: 2, LOW: 3 };
  const sevColor = (s) => s === 'CRITICAL' ? 'var(--danger)' : s === 'HIGH' ? '#f0863c' : s === 'MEDIUM' ? 'var(--warning)' : 'var(--text-muted)';

  // Representative fallback (browser preview / before first scan).
  const SAMPLE = {
    ports: [
      { port: 135, process: 'svchost (RPC)' }, { port: 445, process: 'System (SMB)' },
      { port: 3389, process: 'TermService (RDP)' }, { port: 8080, process: 'llama-server' },
      { port: 11434, process: 'ollama' }, { port: 5432, process: 'postgres' },
    ],
    services: [
      { name: 'Dhcp', description: 'Cliente DHCP', status: 'running' },
      { name: 'Dnscache', description: 'Cliente DNS', status: 'running' },
      { name: 'WinDefend', description: 'Microsoft Defender', status: 'running' },
      { name: 'Spooler', description: 'Cola de impresión', status: 'stopped' },
      { name: 'MSSQLSERVER', description: 'SQL Server', status: 'running' },
    ],
    software: [
      { name: 'Microsoft Edge', version: '126.0.2592.68' }, { name: 'Visual Studio Code', version: '1.90.2' },
      { name: 'Node.js', version: '20.14.0' }, { name: 'Git', version: '2.45.1' },
      { name: 'PowerShell 7', version: '7.4.3' }, { name: 'Docker Desktop', version: '4.31.0' },
      { name: '7-Zip', version: '24.05' },
    ],
    certs: [
      { subject: 'CN=PRECISION-X', expires: '2026-11-02', days_left: 123 },
      { subject: 'CN=localhost (dev)', expires: '2026-07-20', days_left: 18 },
    ],
    scheduled: [
      { entry: 'LucyBackup — Diario 03:00' }, { entry: 'GoogleUpdateTaskMachineCore — 09:00' },
      { entry: 'OneDrive Standalone Update — cada 24h' },
    ],
  };

  let snap = $state(null);         // { ports, services, software, certs, scheduled } | null
  let scanning = $state(false);
  let live = $state(false);        // true once a real scan lands
  let error = $state('');
  let lastScan = $state('');
  let activeCat = $state('ports');
  let query = $state('');

  // ── Vulnerability scan (real, offline curated CVE DB via cve_scan) ─────────
  let cve = $state(null);          // CveScanResult { matches, scanned_count, db_size, stale_warning } | null
  let lastVulnScan = $state('');
  let _copiedKey = $state(null);
  let _periodicTimer = null;

  const vulns = $derived(cve?.matches ?? []);
  const vulnTotal = $derived(vulns.length);
  const vulnBySeverity = $derived.by(() => {
    const c = { CRITICAL: 0, HIGH: 0, MEDIUM: 0, LOW: 0 };
    for (const m of vulns) if (c[m.cve.severity] != null) c[m.cve.severity]++;
    return c;
  });
  const topSeverity = $derived(vulnBySeverity.CRITICAL ? 'CRITICAL' : vulnBySeverity.HIGH ? 'HIGH' : vulnBySeverity.MEDIUM ? 'MEDIUM' : vulnTotal ? 'LOW' : null);

  const counts = $derived(snap ? {
    ports: snap.ports.length, services: snap.services.length, software: snap.software.length,
    vulns: vulnTotal, certs: snap.certs.length, scheduled: snap.scheduled.length,
  } : { ports: 0, services: 0, software: 0, vulns: 0, certs: 0, scheduled: 0 });

  const rows = $derived.by(() => {
    if (!snap) return [];
    const q = query.trim().toLowerCase();
    const list = snap[activeCat] || [];
    if (!q) return list;
    return list.filter((it) => JSON.stringify(it).toLowerCase().includes(q));
  });
  const vulnRows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return vulns;
    return vulns.filter((m) => `${m.software_name} ${m.software_version} ${m.cve.cve_id} ${m.cve.description}`.toLowerCase().includes(q));
  });

  // Patch guidance: max_version is the first FIXED version, so upgrade past it.
  // El comando de parcheo es una SUGERENCIA, no una certeza, y la interfaz debe
  // decirlo. `winget upgrade --name "<DisplayName>"` da por hecho que el nombre
  // del registro de desinstalación es el de un paquete de winget, y para
  // controladores y software OEM casi nunca lo es: el operador lo pega, recibe
  // «No se encontró ningún paquete» y se queda sin saber si el problema es el
  // comando o el hallazgo.
  //
  // `--query` en vez de `--name`: busca por coincidencia parcial en lugar de
  // exigir el nombre exacto del paquete, que es la razón concreta por la que la
  // versión anterior fallaba.
  const patchCmd = (m) => `winget upgrade --query "${m.software_name}"`;
  /** Comprobación previa: dice si winget conoce siquiera este software. */
  const patchProbe = (m) => `winget search --query "${m.software_name}"`;
  const fixHint = (m) => m.cve.max_version ? `Actualiza a ≥ ${m.cve.max_version}` : 'Actualiza a la última versión del proveedor';
  const nvdUrl = (id) => `https://nvd.nist.gov/vuln/detail/${id}`;
  async function copyKey(key, text) {
    try { await copyToClipboard(text); } catch { try { navigator.clipboard?.writeText(text); } catch {} }
    _copiedKey = key; setTimeout(() => { if (_copiedKey === key) _copiedKey = null; }, 1200);
  }

  async function scanVulns() {
    if (!snap || !Array.isArray(snap.software) || snap.software.length === 0) { cve = null; return; }
    try {
      const soft = snap.software.map((s) => ({ name: s.name, version: s.version || null }));
      cve = await invoke('cve_scan', { software: soft });
      lastVulnScan = new Date().toLocaleTimeString('es', { hour: '2-digit', minute: '2-digit' });
    } catch {
      cve = null;   // backend absent (browser preview) — no vuln data
    }
  }

  async function scan() {
    if (scanning) return;
    scanning = true; error = '';
    try {
      const data = await invoke('discover_inventory_local');
      snap = {
        ports:     Array.isArray(data.ports)     ? data.ports     : [],
        services:  Array.isArray(data.services)  ? data.services  : [],
        software:  Array.isArray(data.software)  ? data.software  : [],
        certs:     Array.isArray(data.certs)     ? data.certs     : [],
        scheduled: Array.isArray(data.scheduled) ? data.scheduled : [],
      };
      live = true;
      lastScan = new Date().toLocaleTimeString('es', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
      await scanVulns();
    } catch (e) {
      error = String(e);
      if (!snap) { snap = SAMPLE; await scanVulns(); }   // preview / no-backend fallback so the view isn't empty
    } finally {
      scanning = false;
    }
  }

  onMount(() => {
    ensureLocalHostname();
    scan();
    // Periodic re-scan while this view is open — catches newly-installed
    // software and re-checks it against the CVE DB. (True background alerting
    // even when the view is closed would run through the app scheduler.)
    _periodicTimer = setInterval(() => { if (!scanning) scan(); }, 30 * 60 * 1000);
    return () => { if (_periodicTimer) clearInterval(_periodicTimer); };
  });
</script>

<div class="inv">
  <div class="inv-head">
    <span class="ck-led" class:on={live && !scanning} class:warn={scanning} class:err={!!error && !scanning}></span>
    <span class="inv-title">Inventario</span>
    <span class="host-pill"><Server size={14} stroke={1.75} /> {hostLabel($localHostname)} · local</span>
    {#if live && lastScan}<span class="live"><span class="live-dot"></span>escaneado {lastScan}</span>{/if}
    <span class="ck-rule" aria-hidden="true"></span>
    <button class="scan-btn" class:busy={scanning} onclick={scan} disabled={scanning}>
      <ScanSearch size={15} stroke={1.75} /> {scanning ? 'Escaneando…' : 'Escanear'}
    </button>
  </div>

  {#if !snap && scanning}
    <div class="inv-loading ck-blueprint"><ScanSearch size={30} stroke={1.4} /><p class="ck-shimmer">Escaneando el inventario de {hostLabel($localHostname, 'este equipo')}…</p></div>
  {:else if snap}
    {#if vulnTotal > 0}
      <button class="vuln-alert" style="--sev:{sevColor(topSeverity)}" onclick={() => (activeCat = 'vulns')}>
        <AlertTriangle size={17} stroke={1.85} />
        <span class="va-txt">
          <b>{vulnTotal} vulnerabilidad{vulnTotal === 1 ? '' : 'es'}</b> en software instalado
          {#if vulnBySeverity.CRITICAL}· <span class="va-c">{vulnBySeverity.CRITICAL} crítica{vulnBySeverity.CRITICAL === 1 ? '' : 's'}</span>{/if}
          {#if vulnBySeverity.HIGH}· {vulnBySeverity.HIGH} alta{vulnBySeverity.HIGH === 1 ? '' : 's'}{/if}
        </span>
        <span class="va-cta">Ver y parchar →</span>
      </button>
    {/if}

    <div class="inv-cards">
      {#each CATS as c}
        <button class="inv-card" class:active={activeCat === c.id} class:danger={c.id === 'vulns' && counts.vulns > 0} onclick={() => (activeCat = c.id)}>
          <span class="card-num">{counts[c.id]}</span>
          <span class="card-lbl">{c.label}</span>
        </button>
      {/each}
    </div>

    <div class="inv-search">
      <Search size={15} stroke={1.75} />
      <input placeholder="Filtrar {CATS.find((c) => c.id === activeCat)?.label.toLowerCase()}…" bind:value={query} />
    </div>

    {#if !error && !live}<div class="inv-note">Datos de ejemplo — sin backend disponible en esta vista previa.</div>{/if}

    {#if activeCat === 'vulns'}
      <div class="vuln-list">
        {#each vulnRows as m (m.cve.cve_id + '·' + m.software_name)}
          <div class="vuln-card" style="--sev:{sevColor(m.cve.severity)}">
            <div class="vc-head">
              <span class="vc-sev">{m.cve.severity}</span>
              <span class="vc-sw">{m.software_name} <span class="vc-ver">{m.software_version || '?'}</span></span>
              <span class="vc-cvss">CVSS {m.cve.cvss_score.toFixed(1)}</span>
            </div>
            <div class="vc-desc">{m.cve.description}</div>
            <div class="vc-foot">
              <button class="vc-cve" title="Copiar ID · abrir en NVD" onclick={() => copyKey('cve-' + m.cve.cve_id + m.software_name, m.cve.cve_id)}>
                {#if _copiedKey === 'cve-' + m.cve.cve_id + m.software_name}<Check size={12} stroke={2} />{:else}<Copy size={12} stroke={1.75} />{/if} {m.cve.cve_id}
              </button>
              <span class="vc-fix"><ShieldCheck size={13} stroke={1.75} /> {fixHint(m)}</span>
            </div>
            <div class="vc-patch">
              <code title={nvdUrl(m.cve.cve_id)}>{patchCmd(m)}</code>
              <button class="vc-copy" title="Copiar comando de parche" aria-label="Copiar comando de parche" onclick={() => copyKey('cmd-' + m.cve.cve_id + m.software_name, patchCmd(m))}>
                {#if _copiedKey === 'cmd-' + m.cve.cve_id + m.software_name}<Check size={13} stroke={2} />{:else}<Copy size={13} stroke={1.75} />{/if}
              </button>
            </div>
            <!-- Decir que es una sugerencia y ofrecer la comprobación previa
                 evita el callejón sin salida de «no se encontró ningún paquete»
                 sin pista de qué hacer después. -->
            <div class="vc-note">
              Sugerencia — mucho software OEM y los controladores no están en winget.
              Comprueba antes con
              <button class="vc-inline" onclick={() => copyKey('probe-' + m.cve.cve_id + m.software_name, patchProbe(m))}
                title="Copiar comprobación">{patchProbe(m)}</button>
              {#if _copiedKey === 'probe-' + m.cve.cve_id + m.software_name}<span class="vc-ok">copiado</span>{/if}
              ; si no aparece, actualízalo desde el fabricante.
            </div>
          </div>
        {/each}
        {#if vulnRows.length === 0}
          <div class="vuln-none">
            {#if query}
              <p>Sin coincidencias.</p>
            {:else}
              <ShieldCheck size={30} stroke={1.4} />
              <p>Sin vulnerabilidades conocidas en el software instalado.</p>
              {#if cve}<span class="vn-meta">{cve.scanned_count} paquetes verificados contra {cve.db_size} CVEs{#if lastVulnScan} · {lastVulnScan}{/if}{#if cve.stale_warning} · {cve.stale_warning}{/if}</span>{/if}
            {/if}
          </div>
        {:else if cve?.stale_warning}
          <div class="vn-meta stale">⚠ {cve.stale_warning}</div>
        {/if}
      </div>
    {:else}
    <div class="inv-table" role="table">
      {#if activeCat === 'ports'}
        <div class="tr th" role="row"><span>Puerto</span><span>Proceso</span><span class="c-end">Estado</span></div>
        {#each rows as p, i (i)}
          <div class="tr" role="row"><span class="mono">{p.port}</span><span>{p.process || '—'}</span><span class="c-end"><span class="badge ok">LISTEN</span></span></div>
        {/each}
      {:else if activeCat === 'services'}
        <div class="tr th s3" role="row"><span>Servicio</span><span>Descripción</span><span class="c-end">Estado</span></div>
        {#each rows as s, i (i)}
          <div class="tr s3" role="row"><span class="mono">{s.name}</span><span class="dim">{s.description || '—'}</span><span class="c-end"><span class="badge" class:ok={/run|activ/i.test(s.status || '')}>{s.status || '—'}</span></span></div>
        {/each}
      {:else if activeCat === 'software'}
        <div class="tr th s2" role="row"><span>Software</span><span class="c-end">Versión</span></div>
        {#each rows as s, i (i)}
          <div class="tr s2" role="row"><span>{s.name}</span><span class="c-end mono dim">{s.version || '—'}</span></div>
        {/each}
      {:else if activeCat === 'certs'}
        <div class="tr th s3" role="row"><span>Subject</span><span>Expira</span><span class="c-end">Días</span></div>
        {#each rows as c, i (i)}
          <div class="tr s3" role="row"><span class="tiny">{c.subject || c.path || '—'}</span><span class="mono dim">{c.expires || '—'}</span><span class="c-end days" style="color:{c.days_left < 0 ? 'var(--danger)' : c.days_left < 30 ? 'var(--warning)' : 'var(--accent)'}">{c.days_left ?? '—'}d</span></div>
        {/each}
      {:else}
        <div class="tr th s1" role="row"><span>Tarea / Entrada Cron</span></div>
        {#each rows as s, i (i)}
          <div class="tr s1" role="row"><span class="mono tiny">{s.entry}</span></div>
        {/each}
      {/if}
      {#if rows.length === 0}
        <div class="tr-empty">{query ? 'Sin coincidencias.' : 'Sin elementos en esta categoría.'}</div>
      {/if}
    </div>
    {/if}
  {:else}
    <div class="inv-loading"><ScanSearch size={30} stroke={1.4} /><p>{error || 'Pulsa Escanear para inventariar este host.'}</p></div>
  {/if}
</div>

<style>
  .inv { height: 100%; display: flex; flex-direction: column; }

  .inv-head { display: flex; align-items: center; gap: 12px; padding: 18px 22px 14px; }
  .inv-title { font-size: var(--fs-title); font-weight: var(--fw-medium); color: var(--text-primary); }
  .host-pill { display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--text-muted); background: var(--surface-2); border: 1px solid var(--border); padding: 4px 10px; border-radius: var(--r-sm); }
  .live { display: flex; align-items: center; gap: 6px; font-size: var(--fs-caption); color: var(--accent); }
  .live-dot { width: 7px; height: 7px; border-radius: var(--r-pill); background: var(--accent); animation: inv-pulse 1.6s var(--ease-out) infinite; }
  @keyframes inv-pulse { 0% { box-shadow: 0 0 0 0 rgba(61,214,164,0.5); } 70% { box-shadow: 0 0 0 5px rgba(61,214,164,0); } 100% { box-shadow: 0 0 0 0 rgba(61,214,164,0); } }
  .scan-btn { margin-left: auto; display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--accent-ink); background: var(--accent); border: 0; border-radius: var(--r-md); padding: 6px 12px; cursor: pointer; transition: background var(--dur-fast) var(--ease-out); }
  .scan-btn:hover { background: var(--accent-hover); }
  .scan-btn.busy, .scan-btn:disabled { opacity: 0.7; cursor: default; }

  .inv-cards { display: flex; gap: 8px; padding: 0 22px 12px; flex-wrap: wrap; }
  .inv-card { display: flex; flex-direction: column; align-items: center; gap: 3px; min-width: 84px; padding: 10px 16px; border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); cursor: pointer; transition: border-color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out); }
  .inv-card:hover { border-color: var(--border-strong); }
  .inv-card.active { border-color: var(--border-accent); background: var(--accent-bg); }
  .card-num { font-size: 22px; font-weight: var(--fw-medium); color: var(--text-primary); line-height: 1; }
  .inv-card.active .card-num { color: var(--accent); }
  .card-lbl { font-size: var(--fs-caption); color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.03em; }

  .inv-search { display: flex; align-items: center; gap: 9px; margin: 0 22px 10px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-md); padding: 7px 12px; color: var(--text-muted); }
  .inv-search input { flex: 1; background: transparent; border: 0; outline: 0; color: var(--text-primary); font-size: var(--fs-body); font-family: var(--font-sans); }
  .inv-search input::placeholder { color: var(--text-faint); }
  .inv-note { margin: 0 22px 8px; font-size: var(--fs-caption); color: var(--text-faint); }

  .inv-table { flex: 1; overflow-y: auto; margin: 0 16px 16px; border: 1px solid var(--border); border-radius: var(--r-lg); }
  .tr { display: grid; grid-template-columns: 120px 1fr 90px; gap: 12px; align-items: center; padding: 8px 15px; border-top: 1px solid var(--border); }
  .tr.s2 { grid-template-columns: 1fr 160px; }
  .tr.s1 { grid-template-columns: 1fr; }
  .tr:first-child { border-top: 0; }
  .tr:not(.th):hover { background: var(--surface-2); }
  .th { position: sticky; top: 0; background: var(--surface-1); z-index: 1; }
  .th span { font-size: var(--fs-caption); color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.03em; }
  .tr > span { font-size: var(--fs-footnote); color: var(--text-secondary); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .c-end { text-align: right; justify-self: end; }
  .mono { font-family: var(--font-mono); font-size: var(--fs-caption); }
  .tiny { font-size: var(--fs-caption); }
  .dim { color: var(--text-muted); }
  .days { font-weight: var(--fw-medium); font-family: var(--font-mono); }
  .badge { font-size: var(--fs-caption); padding: 1px 8px; border-radius: var(--r-pill); background: var(--surface-3); color: var(--text-muted); }
  .badge.ok { background: var(--success-bg); color: var(--success); }
  .tr-empty { padding: 20px; text-align: center; font-size: var(--fs-footnote); color: var(--text-faint); }

  .inv-loading { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; color: var(--text-faint); text-align: center; padding: 24px; }
  .inv-loading p { margin: 0; font-size: var(--fs-footnote); color: var(--text-muted); max-width: 320px; }

  /* ── Vulnerabilities ─────────────────────────────────────────────────── */
  .vuln-alert {
    display: flex; align-items: center; gap: 10px; width: calc(100% - 44px);
    margin: 0 22px 12px; padding: 10px 14px; text-align: left; cursor: pointer;
    border: 1px solid var(--sev); border-left: 3px solid var(--sev); border-radius: var(--r-md);
    background: color-mix(in srgb, var(--sev) 10%, transparent);
    color: var(--text-secondary); font-size: var(--fs-footnote);
    transition: background var(--dur-fast) var(--ease-out);
  }
  .vuln-alert:hover { background: color-mix(in srgb, var(--sev) 17%, transparent); }
  .vuln-alert :global(svg) { color: var(--sev); flex-shrink: 0; }
  .va-txt { flex: 1; }
  .va-txt b { color: var(--text-primary); }
  .va-c { color: var(--danger); font-weight: var(--fw-medium); }
  .va-cta { color: var(--sev); font-weight: var(--fw-medium); white-space: nowrap; }

  .inv-card.danger { border-color: color-mix(in srgb, var(--danger) 45%, transparent); background: color-mix(in srgb, var(--danger) 10%, transparent); }
  .inv-card.danger .card-num { color: var(--danger); }

  .vuln-list { flex: 1; overflow-y: auto; margin: 0 16px 16px; display: flex; flex-direction: column; gap: 10px; }
  .vuln-card { border: 1px solid var(--border); border-left: 3px solid var(--sev); border-radius: var(--r-lg); background: var(--surface-2); padding: 11px 13px; }
  .vc-head { display: flex; align-items: center; gap: 9px; }
  .vc-sev { font-size: 9px; font-weight: var(--fw-medium); letter-spacing: 0.04em; color: #fff; background: var(--sev); padding: 2px 7px; border-radius: var(--r-pill); }
  .vc-sw { flex: 1; font-size: var(--fs-footnote); color: var(--text-primary); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .vc-ver { color: var(--text-muted); font-family: var(--font-mono); font-size: var(--fs-caption); }
  .vc-cvss { font-size: var(--fs-caption); font-family: var(--font-mono); color: var(--sev); flex-shrink: 0; }
  .vc-desc { margin: 7px 0; font-size: var(--fs-caption); line-height: var(--lh-body); color: var(--text-muted); }
  .vc-foot { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
  .vc-cve { display: flex; align-items: center; gap: 5px; font-family: var(--font-mono); font-size: var(--fs-caption); color: var(--text-secondary); background: transparent; border: 0; padding: 0; cursor: pointer; }
  .vc-cve:hover { color: var(--accent); }
  .vc-fix { display: flex; align-items: center; gap: 5px; font-size: var(--fs-caption); color: var(--success); }
  .vc-fix :global(svg) { color: var(--success); }
  .vc-patch { display: flex; align-items: center; gap: 8px; margin-top: 9px; padding: 6px 10px; background: rgba(0,0,0,0.28); border: 1px solid var(--border); border-radius: var(--r-sm); }
  .vc-note { margin-top: 6px; font-size: var(--fs-caption); color: var(--text-faint); line-height: var(--lh-tight); }
  .vc-inline {
    font-family: var(--font-mono); font-size: var(--fs-caption);
    color: var(--text-secondary); background: var(--surface-2);
    border: 1px solid var(--border); border-radius: var(--r-sm);
    padding: 1px 6px; cursor: pointer;
  }
  .vc-inline:hover { color: var(--text-primary); border-color: var(--border-strong); }
  .vc-ok { color: var(--accent); margin-left: 4px; }
  .vc-patch code { flex: 1; font-family: var(--font-mono); font-size: 11px; color: var(--text-secondary); overflow-wrap: anywhere; }
  .vc-copy { display: flex; align-items: center; justify-content: center; width: 24px; height: 24px; flex-shrink: 0; padding: 0; border: 0; border-radius: 6px; background: transparent; color: var(--text-muted); cursor: pointer; transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out); }
  .vc-copy:hover { background: var(--surface-3); color: var(--text-primary); }
  .vuln-none { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; padding: 30px; text-align: center; color: var(--text-faint); }
  .vuln-none :global(svg) { color: var(--success); }
  .vuln-none p { margin: 0; font-size: var(--fs-footnote); color: var(--text-muted); }
  .vn-meta { font-size: var(--fs-caption); color: var(--text-faint); }
  .vn-meta.stale { padding: 6px 16px; color: var(--warning); }
</style>
