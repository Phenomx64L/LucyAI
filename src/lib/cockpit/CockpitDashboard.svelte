<script>
  /* ==========================================================================
     Lucy 2.0 — Cockpit dashboard  ·  LIVE, local + remote, rich.
     Drives every panel from the SystemHealth JSON (local: get_system_health_json
     — which also carries network + temperature sensors; remote:
     get_remote_health_{windows,linux}). Adds: sparklines (CPU/RAM history),
     network throughput, temperature sensors, derived alerts, down-services
     (throttled read-only probe), and a RAM/CPU process toggle. Representative
     data is the fallback for a plain-browser preview.
     ========================================================================== */
  import { onMount } from 'svelte';

  // Un insight puede traer un siguiente paso sugerido. Sin esta salida el
  // operador tendría que copiarlo a mano desde un texto.
  let { onRunCommand = undefined } = $props();
  import { tweened } from 'svelte/motion';
  import { cubicOut } from 'svelte/easing';
  import { invoke } from '@tauri-apps/api/core';
  import { getSystemHealthJson } from '$lib/lucy-api';
  import { hosts } from '$lib/stores';
  import Bulb from '@tabler/icons-svelte/icons/bulb';
  import Cpu from '@tabler/icons-svelte/icons/cpu';
  import DeviceDesktop from '@tabler/icons-svelte/icons/device-desktop';
  import Database from '@tabler/icons-svelte/icons/database';
  import Activity from '@tabler/icons-svelte/icons/activity';
  import Refresh from '@tabler/icons-svelte/icons/refresh';
  import Server from '@tabler/icons-svelte/icons/server';
  import ChevronDown from '@tabler/icons-svelte/icons/chevron-down';
  import Network from '@tabler/icons-svelte/icons/network';
  import Temperature from '@tabler/icons-svelte/icons/temperature';
  import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
  import Settings2 from '@tabler/icons-svelte/icons/settings-2';

  // ── Representative fallback (until the first real fetch / browser preview) ──
  let cpu = $state(24);
  let ram = $state({ pct: 43, used: 13.4, total: 31.3 });
  // Valor inicial neutro: `apply()` lo sustituye con el hostname real en el
  // primer refresco, pero hasta entonces mostraba el nombre de la maqueta — que
  // en otra máquina es sencillamente falso. Un guion no engaña a nadie.
  let sys = $state({ name: '—', os: '', uptime: '—' });
  let storage = $state({ pct: 69, free: '146 G', total: '475 G' });
  let disks = $state([{ mount: 'C:\\', pct: 69, used: 329, total: 475, free: 146 }]);
  let cores = $state([42, 20, 28, 19, 29, 25, 19, 32, 24, 30, 22, 15, 12, 24, 24, 24]);
  let procs = $state([
    { name: 'msedgewebview2.exe', ram: 608, cpu: 3, pid: 17304 },
    { name: 'llama-server.exe', ram: 553, cpu: 12, pid: 26944 },
    { name: 'Memory Compression', ram: 539, cpu: 0, pid: 4260 },
    { name: 'claude.exe', ram: 467, cpu: 5, pid: 31264 },
    { name: 'SupportAssistAgent.exe', ram: 481, cpu: 1, pid: 6672 },
  ]);
  let net = $state({ recv: 0.3, send: 0.1, ifaces: ['Wi-Fi', 'Ethernet'], hasRate: true });
  let temps = $state([{ label: 'CPU Package', c: 52, max: 90, crit: 100 }]);
  let services = $state({ failed: [], checked: false });
  let cpuHist = $state([]);
  let ramHist = $state([]);
  let procSort = $state('ram');

  let selectedHost = $state('local');
  let hostMenuOpen = $state(false);
  let live = $state(false);
  let loading = $state(false);
  let error = $state('');
  let lastUpdate = $state('');
  let _timer = null, _svcTimer = null, _vulnTimer = null, _insTimer = null, _secTimer = null;

  // ── Insights proactivos ───────────────────────────────────────────────────
  // Solo LECTURA de lo que el detector de fondo ya escribió. Se filtran los
  // descartados en el backend; aquí solo se pide y se muestra.
  // ── Briefs de seguridad ─────────────────────────────────────────────────
  // `dashboard_failed_logins_24h` y `dashboard_open_incidents` existían con
  // cero superficie en V2. Solo se piden para el equipo local: ambos consultan
  // esta máquina, así que mostrarlos mientras el selector apunta a un host
  // remoto atribuiría datos locales al equipo equivocado.
  let sec = $state({ logins: null, incidents: null });
  async function loadSecurity() {
    if (selectedHost !== 'local') { sec = { logins: null, incidents: null }; return; }
    try { sec.logins = await invoke('dashboard_failed_logins_24h'); } catch { sec.logins = null; }
    try { sec.incidents = await invoke('dashboard_open_incidents', { hostName: sys.name || '' }); } catch { sec.incidents = null; }
  }

  let insights = $state([]);
  async function loadInsights() {
    try {
      const rows = await invoke('proactive_insights_recent', { limit: 6 });
      insights = Array.isArray(rows) ? rows.filter((r) => !r.dismissed) : [];
    } catch {
      insights = [];   // sin backend (preview en navegador) — sin insights
    }
  }
  async function dismissInsight(id) {
    // Optimista: la fila desaparece al instante y el backend se entera después.
    // Si el descarte fallara, la próxima recarga la devuelve — preferible a que
    // el botón parezca no hacer nada.
    insights = insights.filter((i) => i.id !== id);
    try { await invoke('proactive_insight_dismiss', { id }); } catch {}
  }
  // Software vulnerabilities on the LOCAL host (offline CVE DB, same as the
  // Inventory view's cve_scan). Surfaced as a dashboard alert.
  let vulns = $state({ total: 0, critical: 0, high: 0, at: '' });

  const hostList = $derived([{ id: 'local', name: 'Este equipo', kind: 'local' }, ...$hosts.map((h) => ({ id: h.id, name: h.name, kind: h.type }))]);
  const selectedName = $derived(hostList.find((h) => h.id === selectedHost)?.name ?? 'Este equipo');
  const procsSorted = $derived([...procs].sort((a, b) => (procSort === 'cpu' ? b.cpu - a.cpu : b.ram - a.ram)).slice(0, 7));

  // Big metric numbers count up from 0 on first load and slide smoothly to the
  // new value on each live poll (so they never read as static text).
  const cpuDisp = tweened(0, { duration: 650, easing: cubicOut });
  const ramDisp = tweened(0, { duration: 650, easing: cubicOut });
  const stoDisp = tweened(0, { duration: 650, easing: cubicOut });
  $effect(() => { cpuDisp.set(cpu); });
  $effect(() => { ramDisp.set(ram.pct); });
  $effect(() => { stoDisp.set(storage.pct); });

  const alerts = $derived.by(() => {
    const a = [];
    if (cpu >= 90) a.push({ sev: 'bad', text: `CPU al ${cpu}%` });
    else if (cpu >= 78) a.push({ sev: 'warn', text: `CPU alta (${cpu}%)` });
    if (ram.pct >= 92) a.push({ sev: 'bad', text: `RAM al ${ram.pct}%` });
    else if (ram.pct >= 82) a.push({ sev: 'warn', text: `RAM alta (${ram.pct}%)` });
    for (const d of disks) {
      if (d.pct >= 93) a.push({ sev: 'bad', text: `Disco ${d.mount} al ${d.pct}%` });
      else if (d.pct >= 86) a.push({ sev: 'warn', text: `Disco ${d.mount} al ${d.pct}%` });
    }
    for (const t of temps) {
      if (t.crit && t.c >= t.crit) a.push({ sev: 'bad', text: `${t.label} ${t.c}°C (crítico)` });
      else if (t.c >= 82) a.push({ sev: 'warn', text: `${t.label} ${t.c}°C` });
    }
    if (services.failed.length) a.push({ sev: 'warn', text: `${services.failed.length} servicio(s) detenido(s)` });
    if (vulns.total > 0) a.push({
      sev: (vulns.critical || vulns.high) ? 'bad' : 'warn',
      text: `${vulns.total} vulnerabilidad${vulns.total === 1 ? '' : 'es'} en software${vulns.critical ? ` · ${vulns.critical} crítica${vulns.critical === 1 ? '' : 's'}` : vulns.high ? ` · ${vulns.high} alta${vulns.high === 1 ? '' : 's'}` : ''}`,
    });
    return a;
  });
  const health = $derived(alerts.some((a) => a.sev === 'bad') ? { cls: 'bad', label: 'Crítico' } : alerts.length ? { cls: 'warn', label: 'Atención' } : { cls: 'ok', label: 'Saludable' });
  const cpuSpark = $derived(sparkPath(cpuHist));
  const ramSpark = $derived(sparkPath(ramHist));

  const G = (mb) => +(mb / 1024).toFixed(1);
  function pickSystemDisk(list) {
    if (!list || !list.length) return null;
    return list.find((d) => /^[Cc]:/.test(d.mount || '') || d.mount === '/') ?? [...list].sort((a, b) => (b.total_gb || 0) - (a.total_gb || 0))[0];
  }
  function sparkPath(arr, w = 100, h = 26) {
    if (!arr || arr.length < 2) return '';
    const step = w / (arr.length - 1);
    return arr.map((v, i) => `${i === 0 ? 'M' : 'L'}${(i * step).toFixed(1)},${(h - (Math.min(100, Math.max(0, v)) / 100) * h).toFixed(1)}`).join(' ');
  }
  function apply(h) {
    cpu = Math.round(h.cpu?.global ?? 0);
    ram = { pct: Math.round(h.memory?.percent ?? 0), used: G(h.memory?.used_mb ?? 0), total: G(h.memory?.total_mb ?? 0) };
    sys = { name: h.hostname || '—', os: h.os || '', uptime: `${h.uptime_h ?? 0} h` };
    disks = (h.disks ?? []).map((d) => ({ mount: d.mount || d.name || '—', pct: Math.round(d.percent ?? 0), used: Math.round(d.used_gb ?? 0), total: Math.round(d.total_gb ?? 0), free: Math.round(d.free_gb ?? 0) }));
    const sd = pickSystemDisk(h.disks);
    if (sd) storage = { pct: Math.round(sd.percent ?? 0), free: `${Math.round(sd.free_gb ?? 0)} G`, total: `${Math.round(sd.total_gb ?? 0)} G` };
    cores = (h.cpu?.per_core ?? []).map((c) => Math.round(c));
    procs = (h.top_processes ?? []).slice(0, 10).map((p) => ({ name: p.name, ram: Math.round(p.mem_mb ?? 0), cpu: Math.round(p.cpu ?? 0), pid: p.pid ?? 0 }));
    // Local health carries network + temperature inline; for remote hosts these
    // come from a separate ad-hoc probe (fetchExtras), so don't clobber them here.
    if (selectedHost === 'local') {
      net = h.network ? { recv: +(h.network.recv_mbps || 0), send: +(h.network.send_mbps || 0), ifaces: (h.network.interfaces || []).map((i) => i.name), hasRate: true } : null;
      temps = h.temperatures?.available ? (h.temperatures.sensors || []).map((s) => ({ label: s.label, c: s.celsius, max: s.max, crit: s.critical })) : [];
    }
    cpuHist = [...cpuHist, cpu].slice(-44);
    ramHist = [...ramHist, ram.pct].slice(-44);
  }

  let _fetchSeq = 0;
  async function refresh() {
    if (loading) return;
    const forHost = selectedHost;
    const tok = ++_fetchSeq;
    loading = true; error = '';
    try {
      let h;
      if (forHost === 'local') {
        h = await getSystemHealthJson();
      } else {
        const host = $hosts.find((x) => x.id === forHost);
        if (!host) return;
        let pwd = ''; try { pwd = await invoke('get_host_credential', { hostId: host.id }); } catch {}
        h = host.type === 'windows'
          ? await invoke('get_remote_health_windows', { host: host.host, username: host.username, password: pwd })
          : await invoke('get_remote_health_linux', { host: host.host, username: host.username, port: host.port || 22, keyPath: host.sshKeyPath || null });
      }
      // Discard if a newer refresh started or the user switched host mid-fetch.
      if (tok !== _fetchSeq || forHost !== selectedHost) return;
      apply(h);
      live = true;
      lastUpdate = new Date().toLocaleTimeString('es', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    } catch (e) {
      if (forHost === selectedHost && forHost !== 'local') { error = String(e); live = false; }
    } finally {
      if (tok === _fetchSeq) loading = false;
    }
  }

  // Down/failed services — a read-only probe on a slow cadence (30 s), separate
  // from the metrics poll. Windows: stopped auto-start services; Linux: failed units.
  async function fetchServices() {
    const forHost = selectedHost;
    try {
      const host = forHost === 'local' ? null : $hosts.find((x) => x.id === forHost);
      const isWin = forHost === 'local' ? /windows/i.test(sys.os) : host?.type === 'windows';
      // Windows: CIM, not Get-Service. `Get-Service`'s StartType reports
      // "Automatic (Delayed Start)" as plain `Automatic` — the delayed flag
      // simply is not on that object — so the old query could not tell a
      // service that FAILED to start from one Windows deliberately starts
      // late. On a freshly booted machine that meant asus, edgeupdate,
      // MapsBroker and sppsvc (all DelayedAutoStart) were reported as down
      // while behaving exactly as designed, and since any non-empty list
      // raises a `warn` alert, the whole host flipped from Saludable to
      // Atención roughly a minute after boot. An indicator that cries wolf
      // on every single boot teaches the operator to stop reading it.
      //
      // `ExitCode -ne 0` keeps the useful half: a delayed-start service that
      // genuinely CRASHED still surfaces. Only the "late or idle, exited
      // cleanly" case is filtered out.
      //
      // Not solved here: trigger-start services (AppXSvc) idle themselves and
      // reappear. Neither WMI nor CIM exposes triggers, and `sc.exe
      // qtriggerinfo` is no help — its exit code is 0 either way and its
      // output is LOCALISED, which is the same trap that made the security
      // log read as unreadable on Spanish Windows.
      const cmd = isWin
        ? "Get-CimInstance Win32_Service -Filter \"StartMode='Auto' AND State='Stopped'\" | Where-Object { -not $_.DelayedAutoStart -or $_.ExitCode -ne 0 } | Select-Object -First 12 -ExpandProperty Name"
        : "systemctl --failed --no-legend --plain 2>/dev/null | awk '{print $1}' | head -12";
      let out;
      if (forHost === 'local') {
        if (!isWin) { services = { failed: [], checked: true }; return; }
        out = await invoke('execute_powershell', { script: cmd, bypassToken: null });
      } else {
        if (!host) return;
        let pwd = ''; try { pwd = await invoke('get_host_credential', { hostId: host.id }); } catch {}
        out = await invoke('execute_shell_cmd', { host: host.host, username: host.username, command: cmd, hostType: host.type, port: host.port || (host.type === 'linux' ? 22 : 5985), password: pwd || null, keyPath: host.sshKeyPath || null });
      }
      if (forHost !== selectedHost) return;   // host switched mid-probe
      const failed = String(out || '').split('\n').map((l) => l.trim()).filter((l) => l && !/^\[stderr/i.test(l)).slice(0, 12);
      services = { failed, checked: true };
    } catch { if (forHost === selectedHost) services = { failed: [], checked: true }; }
  }

  // Network + temperature for REMOTE hosts (get_remote_health_* omit them). One
  // read-only command per host on the slow cadence. Linux double-samples
  // /proc/net/dev (1 s apart) for a real Mbps rate; Windows reports IPs only.
  async function fetchExtras() {
    const forHost = selectedHost;
    const host = $hosts.find((x) => x.id === forHost);
    if (!host) return;
    try {
      let pwd = ''; try { pwd = await invoke('get_host_credential', { hostId: host.id }); } catch {}
      const isWin = host.type === 'windows';
      const cmd = isWin
        ? `Write-Output "===NET==="; Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' } | ForEach-Object { "$($_.InterfaceAlias)|$($_.IPAddress)" }; Write-Output "===TEMP==="; Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue | ForEach-Object { "Thermal|$([math]::Round(($_.CurrentTemperature - 2732)/10))" }`
        : `echo ===NET===; ip -o -4 addr show scope global 2>/dev/null | awk '{print $2"|"$4}'; echo ===RATE===; awk 'NR>2 && $1 !~ /^lo:/{r+=$2;t+=$10}END{print r"|"t}' /proc/net/dev; sleep 1; awk 'NR>2 && $1 !~ /^lo:/{r+=$2;t+=$10}END{print r"|"t}' /proc/net/dev; echo ===TEMP===; for f in /sys/class/thermal/thermal_zone*/temp; do [ -f "$f" ] && echo "$(cat "$(dirname "$f")/type" 2>/dev/null)|$(cat "$f")"; done`;
      const out = await invoke('execute_shell_cmd', { host: host.host, username: host.username, command: cmd, hostType: host.type, port: host.port || (isWin ? 5985 : 22), password: pwd || null, keyPath: host.sshKeyPath || null });
      if (forHost !== selectedHost) return;   // host switched mid-probe
      parseExtras(String(out || ''), isWin);
    } catch { /* keep whatever we had */ }
  }
  function parseExtras(out, isWin) {
    const lines = out.split('\n').map((l) => l.trim());
    let sect = '';
    const ifaces = [], rates = [], tArr = [];
    for (const l of lines) {
      if (l === '===NET===') { sect = 'net'; continue; }
      if (l === '===RATE===') { sect = 'rate'; continue; }
      if (l === '===TEMP===') { sect = 'temp'; continue; }
      if (!l || !l.includes('|')) continue;
      if (sect === 'net') { const [n, ip] = l.split('|'); ifaces.push(`${(n || '').trim()} ${(ip || '').trim()}`.trim()); }
      else if (sect === 'rate') { const [r, t] = l.split('|').map(Number); if (isFinite(r) && isFinite(t)) rates.push({ r, t }); }
      else if (sect === 'temp') { const [lb, v] = l.split('|'); const c = isWin ? Number(v) : Math.round(Number(v) / 1000); if (isFinite(c) && c > 0 && c < 200) tArr.push({ label: lb || 'Sensor', c, max: null, crit: null }); }
    }
    let recv = 0, send = 0;
    const hasRate = !isWin && rates.length >= 2;
    if (hasRate) { recv = Math.max(0, +((rates[1].r - rates[0].r) * 8 / 1e6).toFixed(2)); send = Math.max(0, +((rates[1].t - rates[0].t) * 8 / 1e6).toFixed(2)); }
    net = (ifaces.length || hasRate) ? { recv, send, ifaces: ifaces.slice(0, 6), hasRate } : null;
    temps = tArr.slice(0, 6);
  }

  function slowProbe() { fetchServices(); if (selectedHost !== 'local') fetchExtras(); }

  // Vulnerability check — LOCAL host only (discover_inventory_local is local,
  // and remote software inventory needs creds). Heavy (spawns a scan), so it
  // runs on mount, on host switch, and on a slow 5-min interval — never on the
  // 5 s health poll. Matches the Inventory view's offline CVE DB.
  async function vulnScan() {
    if (selectedHost !== 'local') { vulns = { total: 0, critical: 0, high: 0, at: '' }; return; }
    const forHost = selectedHost;
    try {
      const inv = await invoke('discover_inventory_local');
      if (forHost !== selectedHost) return;
      const soft = Array.isArray(inv?.software) ? inv.software.map((s) => ({ name: s.name, version: s.version || null })) : [];
      if (!soft.length) { vulns = { total: 0, critical: 0, high: 0, at: '' }; return; }
      const res = await invoke('cve_scan', { software: soft });
      if (forHost !== selectedHost) return;
      const m = Array.isArray(res?.matches) ? res.matches : [];
      vulns = {
        total: m.length,
        critical: m.filter((x) => x.cve.severity === 'CRITICAL').length,
        high: m.filter((x) => x.cve.severity === 'HIGH').length,
        at: new Date().toLocaleTimeString('es', { hour: '2-digit', minute: '2-digit' }),
      };
    } catch { /* backend absent (browser preview) — leave vulns at 0 */ }
  }

  function startPoll() {
    if (_timer) clearInterval(_timer);
    _timer = setInterval(refresh, selectedHost === 'local' ? 5000 : 12000);
    if (_svcTimer) clearInterval(_svcTimer);
    _svcTimer = setInterval(slowProbe, 30000);
    if (_vulnTimer) clearInterval(_vulnTimer);
    _vulnTimer = setInterval(vulnScan, 5 * 60 * 1000);
  }
  function selectHost(id) {
    hostMenuOpen = false;
    if (id === selectedHost) return;
    selectedHost = id; live = false; loading = false; error = ''; cpuHist = []; ramHist = []; services = { failed: [], checked: false }; net = null; temps = [];
    vulns = { total: 0, critical: 0, high: 0, at: '' };
    refresh(); slowProbe(); vulnScan(); startPoll();
  }

  onMount(() => {
    refresh(); slowProbe(); vulnScan(); startPoll();
    // Cada 2 min, igual que la UI clásica: el detector de fondo escribe con
    // cadencia de minutos, sondear más rápido solo gasta.
    loadInsights();
    _insTimer = setInterval(loadInsights, 120000);
    // El primer sondeo espera a que `sys.name` tenga el hostname real: el brief
    // de incidentes se consulta POR host, y pedirlo con el placeholder
    // devolvería siempre cero.
    setTimeout(loadSecurity, 2500);
    _secTimer = setInterval(loadSecurity, 300000);
    return () => {
      if (_timer) clearInterval(_timer);
      if (_svcTimer) clearInterval(_svcTimer);
      if (_vulnTimer) clearInterval(_vulnTimer);
      if (_insTimer) clearInterval(_insTimer);
      if (_secTimer) clearInterval(_secTimer);
    };
  });
</script>

<div class="dash">
  <div class="dash-head">
    <span class="dash-title">Dashboard de sistema</span>

    <div class="host-picker">
      <button class="host-pill" onclick={() => (hostMenuOpen = !hostMenuOpen)} title="Cambiar host">
        {#if selectedHost === 'local'}<DeviceDesktop size={14} stroke={1.75} />{:else}<Server size={14} stroke={1.75} />{/if}
        {selectedName}<ChevronDown size={13} stroke={1.75} />
      </button>
      {#if hostMenuOpen}
        <button class="host-backdrop" onclick={() => (hostMenuOpen = false)} aria-label="Cerrar"></button>
        <div class="host-menu">
          {#each hostList as h (h.id)}
            <button class="host-opt" class:sel={h.id === selectedHost} onclick={() => selectHost(h.id)}>
              {#if h.kind === 'local'}<DeviceDesktop size={14} stroke={1.75} />{:else}<Server size={14} stroke={1.75} />{/if}
              <span class="ho-name">{h.name}</span>
              <span class="ho-kind">{h.kind === 'local' ? 'local' : h.kind === 'windows' ? 'WinRM' : 'SSH'}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <span class="status-chip {health.cls}"><span class="st-dot"></span>{health.label}</span>
    {#if live && lastUpdate}<span class="upd">act. {lastUpdate}</span>{/if}
    <button class="ghost-btn" class:spin={loading} onclick={refresh} aria-label="Actualizar" title="Actualizar ahora"><Refresh size={15} stroke={1.75} /></button>
  </div>

  {#if error}
    <div class="dash-error">No se pudo leer <b>{selectedName}</b>: {error}</div>
  {/if}

  {#if alerts.length}
    <div class="alerts">
      <span class="alerts-lbl"><AlertTriangle size={14} stroke={1.85} /> {alerts.length} alerta{alerts.length > 1 ? 's' : ''}</span>
      {#each alerts as a}<span class="alert {a.sev}">{a.text}</span>{/each}
    </div>
  {/if}

  <!-- ── Insights proactivos ────────────────────────────────────────────────
       El detector corre en segundo plano desde el arranque y escribe a la tabla
       `proactive_insights`. La UI clásica lo sondeaba y el cockpit no preguntaba
       nunca: un operador en V2 no recibía ni un aviso. Aquí no se detecta nada
       nuevo — solo se muestra lo que Lucy ya sabía.

       Van sobre las alertas derivadas porque son de otra naturaleza: aquellas
       describen el estado actual (CPU alta, servicios caídos); estas son cosas
       que Lucy CONCLUYÓ observando en el tiempo. -->
  <!-- ── Seguridad ────────────────────────────────────────────────────────
       Solo aparece cuando hay algo que decir: un panel que anuncia "0 intentos
       fallidos" cada día enseña a saltárselo, y el día que ponga 40 tampoco se
       leerá. El caso "no disponible" sí se muestra, porque un cero por falta de
       permisos no es lo mismo que un cero real — y confundirlos es peor que no
       mirar. -->
  {#if sec.logins?.count_24h > 0 || sec.incidents?.open_count > 0 || (sec.logins && !sec.logins.available)}
    <div class="secbar">
      {#if sec.logins?.count_24h > 0}
        <button class="sec-item warn" onclick={() => onRunCommand?.('Analiza los inicios de sesión fallidos de las últimas 24 horas en este equipo y dime si hay un patrón de ataque')}>
          <AlertTriangle size={14} stroke={1.85} />
          <b>{sec.logins.count_24h}</b> inicio{sec.logins.count_24h === 1 ? '' : 's'} de sesión fallido{sec.logins.count_24h === 1 ? '' : 's'} · 24 h
        </button>
      {/if}
      {#if sec.incidents?.open_count > 0}
        <button class="sec-item bad" onclick={() => onRunCommand?.(`Resume el incidente abierto "${sec.incidents.latest_title}" y propón los siguientes pasos`)}>
          <AlertTriangle size={14} stroke={1.85} />
          <b>{sec.incidents.open_count}</b> incidente{sec.incidents.open_count === 1 ? '' : 's'} abierto{sec.incidents.open_count === 1 ? '' : 's'}
          {#if sec.incidents.latest_title}<span class="sec-sub">· {sec.incidents.latest_title}</span>{/if}
        </button>
      {/if}
      {#if sec.logins && !sec.logins.available}
        <span class="sec-item muted" title={sec.logins.note}>Registro de seguridad no legible — {sec.logins.note}</span>
      {/if}
    </div>
  {/if}

  {#if insights.length}
    <div class="insights">
      <div class="ins-head">
        <Bulb size={14} stroke={1.85} />
        <span>Lucy ha observado</span>
        <span class="ins-count">{insights.length}</span>
      </div>
      {#each insights as ins (ins.id)}
        <div class="ins {ins.severity}">
          <span class="ins-dot"></span>
          <div class="ins-main">
            <div class="ins-title">{ins.title}</div>
            {#if ins.detail}<div class="ins-detail">{ins.detail}</div>{/if}
            {#if ins.action_hint}
              <!-- El detector sugiere el siguiente paso. Sin un botón, el
                   operador tiene que copiarlo a mano desde un texto. -->
              <button class="ins-act" onclick={() => onRunCommand?.(ins.action_hint)}
                title="Enviar a Terminal IA">{ins.action_hint}</button>
            {/if}
          </div>
          <button class="ins-x" onclick={() => dismissInsight(ins.id)} title="Descartar" aria-label="Descartar">✕</button>
        </div>
      {/each}
    </div>
  {/if}

  <div class="metrics">
    <div class="metric">
      <div class="metric-head"><Cpu size={16} stroke={1.75} /> CPU</div>
      <div class="metric-val">{Math.round($cpuDisp)}<span class="unit">%</span></div>
      {#key cpuSpark !== ''}<svg class="spark" viewBox="0 0 100 26" preserveAspectRatio="none"><path d={cpuSpark} fill="none" stroke="var(--accent)" stroke-width="1.5" vector-effect="non-scaling-stroke" /></svg>{/key}
      <div class="metric-sub">{cores.length} núcleos</div>
    </div>
    <div class="metric">
      <div class="metric-head"><Activity size={16} stroke={1.75} /> RAM</div>
      <div class="metric-val">{Math.round($ramDisp)}<span class="unit">%</span></div>
      {#key ramSpark !== ''}<svg class="spark" viewBox="0 0 100 26" preserveAspectRatio="none"><path d={ramSpark} fill="none" stroke="var(--accent)" stroke-width="1.5" vector-effect="non-scaling-stroke" /></svg>{/key}
      <div class="metric-sub">{ram.used} / {ram.total} GB</div>
    </div>
    <div class="metric">
      <div class="metric-head"><Database size={16} stroke={1.75} /> Disco sistema</div>
      <div class="metric-val">{Math.round($stoDisp)}<span class="unit">%</span></div>
      <div class="metric-bar"><div class="fill" class:hot={storage.pct >= 90} style="width:{storage.pct}%"></div></div>
      <div class="metric-sub">{storage.free} libres de {storage.total}</div>
    </div>
    <div class="metric">
      <div class="metric-head"><DeviceDesktop size={16} stroke={1.75} /> Sistema</div>
      <div class="metric-val sm">{sys.name}</div>
      <div class="metric-sub">{sys.os}</div>
      <div class="metric-sub">Uptime {sys.uptime}</div>
    </div>
  </div>

  <div class="wide">
    {#if net}
      <div class="panel sm">
        <div class="panel-title"><Network size={14} stroke={1.75} /> Red</div>
        {#if net.hasRate}<div class="net-rates"><span class="net-r down">↓ {net.recv.toFixed(1)} <i>Mbps</i></span><span class="net-r up">↑ {net.send.toFixed(1)} <i>Mbps</i></span></div>{/if}
        <div class="net-ifaces">{#each net.ifaces.slice(0, 4) as n}<span class="iface">{n}</span>{/each}{#if !net.ifaces.length}<span class="dim">sin interfaces activas</span>{/if}</div>
      </div>
    {/if}

    {#if temps.length}
      <div class="panel sm">
        <div class="panel-title"><Temperature size={14} stroke={1.75} /> Temperatura</div>
        <div class="temps">
          {#each temps.slice(0, 5) as t}
            <div class="temp"><span class="temp-lbl">{t.label}</span><span class="temp-c" class:hot={t.crit && t.c >= t.crit} class:warn={t.c >= 82}>{t.c}°C</span></div>
          {/each}
        </div>
      </div>
    {/if}

    <div class="panel sm">
      <div class="panel-title"><Settings2 size={14} stroke={1.75} /> Servicios detenidos</div>
      {#if !services.checked}
        <div class="dim svc-empty">Comprobando…</div>
      {:else if services.failed.length === 0}
        <div class="svc-ok">✓ Todos los servicios automáticos en ejecución</div>
      {:else}
        <div class="svc-list">{#each services.failed as s}<span class="svc">{s}</span>{/each}</div>
      {/if}
    </div>
  </div>

  <div class="panel">
    <div class="panel-title">Núcleos <span class="panel-sub">{cores.length}</span></div>
    <div class="cores">
      {#each cores as c, i}
        <div class="core">
          <div class="core-top"><span class="core-id">C{i}</span><span class="core-pct">{c}%</span></div>
          <div class="core-bar"><div class="fill" class:hot={c >= 85} style="width:{c}%"></div></div>
        </div>
      {/each}
    </div>
  </div>

  {#if disks.length}
    <div class="panel">
      <div class="panel-title">Discos <span class="panel-sub">{disks.length} volúmenes</span></div>
      <div class="disks">
        {#each disks as d (d.mount)}
          <div class="disk">
            <div class="disk-top"><span class="disk-mount">{d.mount}</span><span class="disk-pct" class:hot={d.pct >= 90} class:warn={d.pct >= 80 && d.pct < 90}>{d.pct}%</span></div>
            <div class="disk-bar"><div class="fill" class:hot={d.pct >= 90} class:warn={d.pct >= 80 && d.pct < 90} style="width:{d.pct}%"></div></div>
            <div class="disk-sub">{d.free} GB libres · {d.used}/{d.total} GB</div>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <div class="panel">
    <div class="panel-title">Top procesos
      <span class="proc-toggle">
        <button class:on={procSort === 'ram'} onclick={() => (procSort = 'ram')}>RAM</button>
        <button class:on={procSort === 'cpu'} onclick={() => (procSort = 'cpu')}>CPU</button>
      </span>
    </div>
    <div class="procs">
      <div class="proc phead"><span class="proc-name">Proceso</span><span class="proc-cpu">CPU</span><span class="proc-ram">RAM</span><span class="proc-pid">PID</span></div>
      {#each procsSorted as p (p.pid + p.name)}
        <div class="proc">
          <span class="proc-name">{p.name}</span>
          <span class="proc-cpu" class:hi={procSort === 'cpu'}>{p.cpu}%</span>
          <span class="proc-ram" class:hi={procSort === 'ram'}>{p.ram} MB</span>
          <span class="proc-pid">{p.pid}</span>
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .dash { height: 100%; overflow-y: auto; padding: 18px 22px; }

  /* `position: relative; z-index: 5` is what keeps the host dropdown ON TOP of
     the metric tiles, and it is not redundant with the menu's own z-index.
     `.dash > *` below gives EVERY direct child an entrance animation on
     `transform` with fill-mode `both`. A filled transform animation makes each
     child its own stacking context, so the head and the tile rows became
     siblings with z-index auto — and the tiles, being later in the DOM, painted
     over the head. The menu's z-index 41 could not help: it only ordered things
     INSIDE the head's context, which as a whole sat below the tiles. Raising the
     head itself is what lets its descendants escape. */
  .dash-head { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; position: relative; z-index: 5; }
  .dash-title { font-size: var(--fs-title); font-weight: var(--fw-medium); color: var(--text-primary); }

  .host-picker { position: relative; }
  .host-pill { display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--text-secondary); background: var(--surface-2); border: 1px solid var(--border); padding: 4px 10px; border-radius: var(--r-sm); cursor: pointer; transition: background var(--dur-fast) var(--ease-out); }
  .host-pill:hover { background: var(--surface-3); }
  .host-pill :global(svg) { color: var(--accent); }
  .host-backdrop { position: fixed; inset: 0; z-index: 40; background: transparent; border: 0; cursor: default; }
  .host-menu { position: absolute; top: calc(100% + 6px); left: 0; z-index: 41; min-width: 230px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-lg); box-shadow: var(--shadow-pop); padding: 6px; display: flex; flex-direction: column; gap: 1px; }
  .host-opt { display: flex; align-items: center; gap: 9px; padding: 8px 10px; border: 0; background: transparent; border-radius: var(--r-sm); cursor: pointer; text-align: left; color: var(--text-secondary); font-size: var(--fs-footnote); }
  .host-opt:hover { background: var(--surface-3); }
  .host-opt.sel { color: var(--accent); }
  .host-opt :global(svg) { color: var(--text-muted); flex-shrink: 0; }
  .host-opt.sel :global(svg) { color: var(--accent); }
  .ho-name { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ho-kind { font-size: var(--fs-caption); color: var(--text-faint); background: var(--surface-3); padding: 1px 6px; border-radius: 5px; }

  .status-chip { display: flex; align-items: center; gap: 6px; font-size: var(--fs-caption); padding: 3px 10px; border-radius: var(--r-pill); }
  .status-chip.ok { color: var(--success); background: var(--success-bg); }
  .status-chip.warn { color: var(--warning); background: var(--warning-bg); }
  .status-chip.bad { color: var(--danger); background: var(--danger-bg); }
  .st-dot { width: 6px; height: 6px; border-radius: var(--r-pill); background: currentColor; }
  .upd { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); }

  .ghost-btn { margin-left: auto; width: 30px; height: 30px; border-radius: var(--r-md); display: flex; align-items: center; justify-content: center; background: transparent; border: 1px solid var(--border); color: var(--text-muted); cursor: pointer; transition: color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out); }
  .ghost-btn:hover { color: var(--text-primary); background: var(--surface-2); }
  .ghost-btn.spin :global(svg) { animation: dash-spin 0.8s linear infinite; }
  @keyframes dash-spin { to { transform: rotate(360deg); } }

  .dash-error { margin-bottom: 14px; padding: 10px 14px; background: var(--danger-bg); border: 1px solid var(--danger); border-radius: var(--r-md); font-size: var(--fs-footnote); color: var(--text-secondary); }

  .alerts { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin-bottom: 14px; padding: 10px 14px; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--r-lg); }

  /* ── Briefs de seguridad ──────────────────────────────────────────────── */
  .secbar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin-bottom: 14px; }
  .sec-item {
    display: flex; align-items: center; gap: 7px;
    font-size: var(--fs-footnote); color: var(--text-secondary);
    background: var(--surface-1); border: 1px solid var(--border);
    border-radius: var(--r-lg); padding: 9px 13px; cursor: pointer; text-align: left;
  }
  .sec-item b { color: var(--text-primary); font-variant-numeric: tabular-nums; }
  .sec-item.warn { border-color: rgba(229,181,103,0.45); }
  .sec-item.warn :global(svg) { color: #E5B567; }
  .sec-item.bad { border-color: rgba(240,110,110,0.45); }
  .sec-item.bad :global(svg) { color: var(--danger); }
  .sec-item:hover:not(.muted) { background: var(--surface-2); }
  .sec-item:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .sec-item.muted { cursor: default; color: var(--text-faint); font-size: var(--fs-caption); }
  .sec-sub { color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 280px; }

  /* ── Insights proactivos ──────────────────────────────────────────────── */
  .insights { margin-bottom: 14px; background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--r-lg); padding: 10px 14px 12px; }
  .ins-head { display: flex; align-items: center; gap: 8px; font-family: var(--font-mono); font-size: var(--fs-micro); letter-spacing: var(--ls-label); text-transform: uppercase; color: var(--text-faint); margin-bottom: 8px; }
  .ins-head :global(svg) { color: var(--accent); }
  .ins-count { color: var(--accent); background: var(--accent-bg); padding: 0 7px; border-radius: var(--r-pill); }
  .ins { display: flex; align-items: flex-start; gap: 10px; padding: 8px 2px; border-top: 1px solid var(--border); }
  .ins:first-of-type { border-top: 0; }
  /* Severidad en el punto, no en el fondo: varias filas con fondo de color
     compiten con las métricas y convierten el panel en un semáforo. */
  .ins-dot { width: 7px; height: 7px; border-radius: var(--r-pill); margin-top: 6px; flex-shrink: 0; background: var(--text-disabled); }
  .ins.warning .ins-dot { background: #E5B567; }
  .ins.critical .ins-dot { background: var(--danger); }
  .ins.info .ins-dot { background: var(--accent); }
  .ins-main { flex: 1; min-width: 0; }
  .ins-title { font-size: var(--fs-footnote); color: var(--text-primary); }
  .ins-detail { font-size: var(--fs-caption); color: var(--text-muted); margin-top: 2px; line-height: var(--lh-tight); }
  .ins-act {
    margin-top: 6px; font-family: var(--font-mono); font-size: var(--fs-caption);
    color: var(--accent); background: var(--accent-bg); border: 1px solid var(--accent-line);
    border-radius: var(--r-sm); padding: 3px 9px; cursor: pointer; text-align: left;
    max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ins-act:hover { background: rgba(61, 214, 164, 0.18); }
  .ins-act:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .ins-x { background: transparent; border: 0; color: var(--text-faint); cursor: pointer; font-size: 13px; padding: 0 2px; line-height: 1; flex-shrink: 0; }
  .ins-x:hover { color: var(--text-primary); }
  .alerts-lbl { display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); font-weight: var(--fw-medium); color: var(--warning); }
  .alert { font-size: var(--fs-caption); padding: 2px 9px; border-radius: var(--r-pill); }
  .alert.warn { color: var(--warning); background: var(--warning-bg); }
  .alert.bad { color: var(--danger); background: var(--danger-bg); }

  .metrics { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; margin-bottom: 14px; }
  .metric { background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-lg); padding: 14px 16px; }
  /* v1.7.235 "instrumento premium" — la cabecera de cada métrica es un label
     de instrumento (mono tracked) y el valor héroe usa cifras tabulares en
     mono: las lecturas dejan de "bailar" al refrescar y el panel se lee como
     instrumental, no como tarjeta de plantilla. */
  .metric-head { display: flex; align-items: center; gap: 7px; font-family: var(--font-mono); font-size: var(--fs-micro); font-weight: var(--fw-medium); letter-spacing: var(--ls-label); text-transform: uppercase; color: var(--text-faint); margin-bottom: 10px; }
  .metric-head :global(svg) { color: var(--accent); }
  .metric-val { font-family: var(--font-mono); font-variant-numeric: tabular-nums; font-size: 28px; font-weight: var(--fw-medium); color: var(--text-primary); line-height: 1; }
  .metric-val.sm { font-size: var(--fs-heading); font-family: var(--font-sans); }
  .unit { font-size: 14px; color: var(--text-muted); margin-left: 2px; }
  .spark { width: 100%; height: 26px; display: block; margin: 10px 0 6px; overflow: visible; }
  /* The trend line draws itself left→right the moment it first has data. The
     {#key} on `spark !== ''` re-mounts the svg once (empty → populated), which
     re-fires this. clip-path avoids depending on the (non-scaling-stroke) path
     length. */
  @keyframes spark-reveal { from { clip-path: inset(0 100% 0 0); } to { clip-path: inset(0 0 0 0); } }
  .spark { animation: spark-reveal var(--dur-slow) var(--ease-out) both; }
  .metric-bar { height: 5px; background: var(--surface-3); border-radius: var(--r-pill); overflow: hidden; margin: 12px 0 8px; }
  .metric-bar .fill { height: 100%; background: var(--accent); border-radius: var(--r-pill); transition: width var(--dur-slow) var(--ease-out); }
  .metric-sub { font-size: var(--fs-caption); color: var(--text-faint); margin-top: 3px; }

  /* v1.7.236 iter-2 — align-items:start: cada panel mide lo que su CONTENIDO
     necesita. Antes el grid estiraba Red a la altura de Servicios y el hueco
     muerto bajo los adaptadores era la mitad de la tarjeta. */
  .wide { display: grid; grid-template-columns: repeat(auto-fit, minmax(230px, 1fr)); gap: 12px; margin-bottom: 14px; align-items: start; }
  .panel { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--r-lg); padding: 14px 16px; margin-bottom: 14px; }
  .panel.sm { margin-bottom: 0; }
  .panel-title { display: flex; align-items: center; gap: 7px; font-family: var(--font-mono); font-size: var(--fs-micro); font-weight: var(--fw-medium); letter-spacing: var(--ls-label); text-transform: uppercase; color: var(--text-faint); margin-bottom: 12px; }
  .panel-title :global(svg) { color: var(--accent); }
  .panel-sub { color: var(--text-faint); font-weight: var(--fw-regular); }

  .net-rates { display: flex; gap: 16px; margin-bottom: 10px; }
  .net-r { font-size: var(--fs-heading); font-weight: var(--fw-medium); font-family: var(--font-mono); }
  .net-r i { font-size: var(--fs-caption); font-style: normal; color: var(--text-faint); font-family: var(--font-sans); }
  .net-r.down { color: var(--accent); } .net-r.up { color: var(--user); }
  .net-ifaces { display: flex; gap: 6px; flex-wrap: wrap; }
  .iface { font-size: var(--fs-caption); color: var(--text-muted); background: var(--surface-3); padding: 2px 8px; border-radius: var(--r-pill); font-family: var(--font-mono); }
  .dim { color: var(--text-faint); font-size: var(--fs-caption); }

  .temps { display: flex; flex-direction: column; gap: 7px; }
  .temp { display: flex; align-items: center; justify-content: space-between; }
  .temp-lbl { font-size: var(--fs-caption); color: var(--text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .temp-c { font-size: var(--fs-footnote); color: var(--text-secondary); font-family: var(--font-mono); }
  .temp-c.warn { color: var(--warning); } .temp-c.hot { color: var(--danger); }

  .svc-ok { font-size: var(--fs-caption); color: var(--success); }
  .svc-empty { font-size: var(--fs-caption); }
  /* v1.7.236 iter-2 — de "texto flotante" a FILAS de instrumento: LED de
     alerta + nombre mono, separadas por hairlines, con tope de alto y scroll
     (10 servicios ya no estiran el panel vecino de Red hasta el vacío). */
  .svc-list { display: flex; flex-direction: column; max-height: 218px; overflow-y: auto; scrollbar-width: thin; }
  .svc {
    display: flex; align-items: center; gap: 8px;
    padding: 5px 2px; border-bottom: 1px solid var(--border);
    font-size: var(--fs-caption); color: var(--text-secondary); font-family: var(--font-mono);
  }
  .svc:last-child { border-bottom: 0; }
  .svc::before {
    content: ''; flex: 0 0 auto; width: 5px; height: 5px; border-radius: var(--r-pill);
    background: var(--warning); box-shadow: 0 0 6px var(--warning-bg);
  }

  .cores { display: grid; grid-template-columns: repeat(auto-fit, minmax(92px, 1fr)); gap: 8px; }
  .core { background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-md); padding: 8px 10px; }
  .core-top { display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 6px; }
  .core-id { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); }
  .core-pct { font-size: var(--fs-footnote); color: var(--text-secondary); }
  .core-bar { height: 4px; background: var(--surface-3); border-radius: var(--r-pill); overflow: hidden; }
  .core-bar .fill { height: 100%; background: var(--accent); border-radius: var(--r-pill); transition: width var(--dur-base) var(--ease-out); }

  .disks { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 10px; }
  .disk { background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-md); padding: 11px 13px; }
  .disk-top { display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 8px; }
  .disk-mount { font-size: var(--fs-footnote); color: var(--text-primary); font-family: var(--font-mono); }
  .disk-pct { font-size: var(--fs-footnote); color: var(--accent); }
  .disk-pct.warn { color: var(--warning); } .disk-pct.hot { color: var(--danger); }
  .disk-bar { height: 5px; background: var(--surface-3); border-radius: var(--r-pill); overflow: hidden; margin-bottom: 8px; }
  .disk-bar .fill { height: 100%; background: var(--accent); border-radius: var(--r-pill); transition: width var(--dur-slow) var(--ease-out); }
  .fill.warn { background: var(--warning); } .fill.hot { background: var(--danger); }
  .disk-sub { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); }

  .proc-toggle { margin-left: auto; display: flex; gap: 2px; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-sm); padding: 2px; }
  .proc-toggle button { font-size: var(--fs-caption); color: var(--text-muted); background: transparent; border: 0; border-radius: 6px; padding: 3px 10px; cursor: pointer; }
  .proc-toggle button.on { color: var(--accent-ink); background: var(--accent); }
  .procs { display: flex; flex-direction: column; }
  .proc { display: grid; grid-template-columns: 1fr 60px 76px 60px; align-items: center; gap: 12px; padding: 9px 4px; border-top: 1px solid var(--border); }
  .proc:first-child, .proc.phead { border-top: 0; }
  .proc.phead span { font-size: var(--fs-caption); color: var(--text-faint); text-transform: uppercase; letter-spacing: 0.03em; font-family: var(--font-sans); }
  .proc-name { min-width: 0; font-size: var(--fs-footnote); color: var(--text-secondary); font-family: var(--font-mono); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .proc-cpu, .proc-ram { font-size: var(--fs-footnote); color: var(--text-muted); text-align: right; font-family: var(--font-mono); }
  .proc-cpu.hi, .proc-ram.hi { color: var(--accent); }
  .proc-pid { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); text-align: right; }

  /* ── Entrance — the panels cascade in, then the meters grow ───────────────
     Runs once when the view mounts (elements persist across the 5 s poll, so
     the numbers/bars still update via their own width transitions without
     re-triggering these). Makes a static snapshot feel like it's assembling. */
  @keyframes dash-in { from { opacity: 0; transform: translateY(10px); } to { opacity: 1; transform: none; } }
  .dash > * { animation: dash-in var(--dur-slow) var(--ease-out) both; }
  .dash > *:nth-child(1) { animation-delay: 0ms; }
  .dash > *:nth-child(2) { animation-delay: 50ms; }
  .dash > *:nth-child(3) { animation-delay: 100ms; }
  .dash > *:nth-child(4) { animation-delay: 150ms; }
  .dash > *:nth-child(5) { animation-delay: 200ms; }
  .dash > *:nth-child(6) { animation-delay: 250ms; }
  .dash > *:nth-child(n+7) { animation-delay: 300ms; }

  /* Meters grow from the left on entrance (transform, so it coexists with the
     existing width transition used for live poll updates). */
  @keyframes dash-bar-grow { from { transform: scaleX(0); } to { transform: scaleX(1); } }
  .metric-bar .fill, .core-bar .fill, .disk-bar .fill {
    transform-origin: left center;
    animation: dash-bar-grow var(--dur-slow) var(--ease-out) both;
    animation-delay: 180ms;
  }

  @media (prefers-reduced-motion: reduce) {
    .dash > *, .spark,
    .metric-bar .fill, .core-bar .fill, .disk-bar .fill { animation: none !important; }
  }
</style>
