<script>
    import { onMount, onDestroy, createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { countUp } from '$lib/actions';

    const dispatch = createEventDispatcher();

    // ── Props ────────────────────────────────────────────────────────────────
    export let hosts       = [];
    export let hostName    = '';
    // svelte-ignore export_let_unused
    export let lucyConfig  = {};
    export let userLang    = 'es-MX';
    export let isEN        = false;

    // ── Internal state ───────────────────────────────────────────────────────
    let dashMetrics        = null;
    let dashLoading        = false;
    let dashError          = '';
    let dashSelectedHost   = 'local';
    let dashRefreshTimer   = null;
    let dashLastUpdate     = '';

    // ── Metrics history (sparklines) ─────────────────────────────────────────
    let metricsHistory     = {};
    const METRICS_HIST_MAX = 20;

    // ── Proactive alerts ─────────────────────────────────────────────────────
    let alertRules         = [];
    let activeAlerts       = [];
    let showAlertsModal    = false;
    let alertForm          = { hostId:'all', metric:'cpu', threshold:85, enabled:true };

    // ── Helpers ──────────────────────────────────────────────────────────────

    function toast(msg, type = 'info') {
        dispatch('toast', { msg, type });
    }

    // ── Dashboard lifecycle ──────────────────────────────────────────────────

    async function startDashboard() {
        stopDashboard();
        dashMetrics = null; dashError = '';
        dashLoading = true;
        await refreshDash();
        dashRefreshTimer = setInterval(refreshDash, 10000);
    }

    function stopDashboard() {
        if (dashRefreshTimer) { clearInterval(dashRefreshTimer); dashRefreshTimer = null; }
    }

    async function refreshDash() {
        dashLoading = true; dashError = '';
        try {
            if (dashSelectedHost === 'local') {
                dashMetrics = await invoke('get_system_health_json');
            } else {
                const h = hosts.find(x => x.id === dashSelectedHost);
                if (!h) { dashError = isEN ? 'Host not found.' : 'Host no encontrado.'; dashLoading = false; return; }
                let pwd = '';
                try { pwd = await invoke('get_host_credential', { hostId: h.id }); } catch(e){}
                if (h.type === 'windows') {
                    dashMetrics = await invoke('get_remote_health_windows', { host:h.host, username:h.username, password:pwd });
                } else {
                    dashMetrics = await invoke('get_remote_health_linux', { host:h.host, username:h.username, port:h.port||22, keyPath:h.sshKeyPath||null });
                }
            }
            dashLastUpdate = new Date().toLocaleTimeString(userLang, {hour:'2-digit',minute:'2-digit',second:'2-digit'});
            pushMetricsHistory(dashSelectedHost, dashMetrics);
            checkAlerts(dashSelectedHost, dashMetrics);
        } catch(e) {
            dashError = String(e);
            dashMetrics = null;
        }
        dashLoading = false;
    }

    function onDashHostChange() {
        startDashboard();
    }

    // ── Sparklines / metrics history ─────────────────────────────────────────

    function sparklineSvg(history, key, color = '#10b981', w = 70, h = 24) {
        const data = (history || []).map(h => h[key] ?? 0);
        if (data.length < 2) return '';
        const min = Math.min(...data), max = Math.max(...data);
        const range = max - min || 1;
        const pts = data.map((v, i) => {
            const x = (i / (data.length - 1)) * w;
            const y = h - ((v - min) / range) * (h - 4) - 2;
            return `${x.toFixed(1)},${y.toFixed(1)}`;
        }).join(' ');
        const last = pts.trim().split(' ').pop().split(',');
        return `<svg width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" xmlns="http://www.w3.org/2000/svg"><polyline points="${pts}" fill="none" stroke="${color}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" opacity="0.6"/><circle cx="${last[0]}" cy="${last[1]}" r="2.5" fill="${color}"/></svg>`;
    }

    function pushMetricsHistory(hostId, metrics) {
        if (!metrics) return;
        if (!metricsHistory[hostId]) metricsHistory[hostId] = [];
        metricsHistory[hostId].push({
            ts:   Date.now(),
            cpu:  metrics.cpu?.global ?? 0,
            ram:  metrics.memory?.percent ?? 0,
            disk: metrics.disks?.length ? Math.max(...metrics.disks.map(d => d.percent)) : 0
        });
        if (metricsHistory[hostId].length > METRICS_HIST_MAX) metricsHistory[hostId].shift();
        metricsHistory = { ...metricsHistory };
        try { localStorage.setItem('lucy_metrics_history', JSON.stringify(metricsHistory)); } catch(e) {}
    }

    // ── Proactive alerts ─────────────────────────────────────────────────────

    function checkAlerts(hostId, metrics) {
        if (!metrics || !alertRules.length) return;
        const hostLabel = hostId === 'local' ? 'Local' : (hosts.find(h => h.id === hostId)?.name ?? hostId);
        for (const rule of alertRules.filter(r => r.enabled && (r.hostId === 'all' || r.hostId === hostId))) {
            let value = 0;
            if (rule.metric === 'cpu')  value = metrics.cpu?.global ?? 0;
            if (rule.metric === 'ram')  value = metrics.memory?.percent ?? 0;
            if (rule.metric === 'disk') value = metrics.disks?.length ? Math.max(...metrics.disks.map(d => d.percent)) : 0;
            const aId = `${rule.id}_${hostId}`;
            if (value >= rule.threshold) {
                if (!activeAlerts.find(a => a.id === aId)) {
                    const al = { id: aId, ruleId: rule.id, hostId, hostLabel, metric: rule.metric.toUpperCase(), value: Math.round(value), threshold: rule.threshold, ts: new Date().toLocaleTimeString() };
                    activeAlerts = [...activeAlerts, al];
                    toast(isEN ? `\u26a0\ufe0f ${al.metric} on ${hostLabel}: ${al.value}%` : `\u26a0\ufe0f ${al.metric} en ${hostLabel}: ${al.value}%`, 'warn');
                    try {
                        if (typeof Notification !== 'undefined' && Notification.permission === 'granted') {
                            new Notification(`\u26a0\ufe0f Lucy \u2014 ${al.metric} alto`, { body: `${hostLabel}: ${al.value}% (umbral ${rule.threshold}%)` });
                        }
                    } catch(e) {}
                }
            } else {
                activeAlerts = activeAlerts.filter(a => a.id !== aId);
            }
        }
    }

    function saveAlertRules() {
        try { localStorage.setItem('lucy_alert_rules', JSON.stringify(alertRules)); } catch(e) {}
    }

    function agregarAlertRule() {
        const thr = Number(alertForm.threshold);
        if (!thr || thr < 1 || thr > 100) return;
        alertRules = [...alertRules, { id: `ar_${Date.now()}`, hostId: alertForm.hostId, metric: alertForm.metric, threshold: thr, enabled: true }];
        saveAlertRules();
        alertForm = { hostId: 'all', metric: 'cpu', threshold: 85, enabled: true };
    }

    function eliminarAlertRule(id) {
        alertRules = alertRules.filter(r => r.id !== id);
        activeAlerts = activeAlerts.filter(a => a.ruleId !== id);
        saveAlertRules();
    }

    // ── Lifecycle ────────────────────────────────────────────────────────────

    onMount(() => {
        try { metricsHistory = JSON.parse(localStorage.getItem('lucy_metrics_history') || '{}'); } catch(e) { metricsHistory = {}; }
        try { alertRules = JSON.parse(localStorage.getItem('lucy_alert_rules') || '[]'); } catch(e) { alertRules = []; }
        try { if (typeof Notification !== 'undefined' && Notification.permission === 'default') Notification.requestPermission().catch(() => {}); } catch(e) {}
        startDashboard();
    });

    onDestroy(() => {
        stopDashboard();
    });
</script>

<div class="view-wrap">
  <div class="view-hdr">
    <div class="view-title" style={dashSelectedHost!=='local'?(()=>{const hc=hosts.find(h=>h.id===dashSelectedHost);return hc?.color?`border-left:3px solid ${hc.color};padding-left:10px;`:'';})():''}>📊 {isEN ? 'System Dashboard' : 'Dashboard de Sistema'}</div>
    <div style="display:flex;align-items:center;gap:8px;margin-left:auto;flex-wrap:wrap;">
      <select class="view-select" bind:value={dashSelectedHost} on:change={onDashHostChange}>
        <option value="local">🖥 Local ({hostName})</option>
        {#each hosts as h}<option value={h.id}>{h.type==='windows'?'🖥':'🐧'} {h.name}</option>{/each}
      </select>
      {#if dashRefreshTimer}
        <span class="dash-auto-badge" title={isEN ? 'Auto-refresh every 10s' : 'Auto-actualización cada 10s'}>
          <span class="dash-pulse"></span>auto
        </span>
      {/if}
      <button class="view-btn" on:click={refreshDash} disabled={dashLoading} title={isEN ? 'Refresh now' : 'Actualizar ahora'}>{dashLoading?'⏳':'↻'}</button>
      <button class="view-btn" on:click={() => showAlertsModal=true} title={isEN ? 'Configure proactive alerts' : 'Configurar alertas proactivas'}
        style="position:relative;">🔔{#if activeAlerts.length}<span class="alert-badge-btn">{activeAlerts.length}</span>{/if}</button>
      {#if dashLastUpdate}
        <span class="dash-last-update">{isEN ? 'Upd.' : 'Act.'} {dashLastUpdate}</span>
      {/if}
    </div>
  </div>
  {#if dashError}
    <div class="view-error">⚠️ {dashError}</div>
  {:else if !dashMetrics && dashLoading}
    <div class="dash-skeleton">
      <div class="dash-cards">
        <div class="sk-card"><div class="sk-lbl"></div><div class="sk-val"></div><div class="sk-bar"></div><div class="sk-sub"></div></div>
        <div class="sk-card"><div class="sk-lbl"></div><div class="sk-val"></div><div class="sk-bar"></div><div class="sk-sub"></div></div>
        <div class="sk-card"><div class="sk-lbl"></div><div class="sk-val"></div><div class="sk-sub"></div><div class="sk-sub short"></div></div>
      </div>
      <div class="sk-section">
        <div class="sk-row"></div><div class="sk-row short"></div><div class="sk-row"></div><div class="sk-row short"></div>
      </div>
    </div>
  {:else if dashMetrics}
  {#if activeAlerts.length}
  <div class="alert-bar">
    {#each activeAlerts as al}
    <div class="alert-item">
      <span class="alert-item-ico">⚠️</span>
      <span><b>{al.metric}</b> {isEN ? 'on' : 'en'} <b>{al.hostLabel}</b>: <span style="color:var(--red);font-weight:700;">{al.value}%</span> ({isEN ? 'threshold' : 'umbral'} {al.threshold}%) · {al.ts}</span>
      <button class="alert-dismiss" on:click={() => activeAlerts = activeAlerts.filter(x=>x.id!==al.id)} title={isEN ? 'Dismiss' : 'Descartar'}>✕</button>
    </div>
    {/each}
  </div>
  {/if}
  <div class="dash-scroll">
    <div class="dash-cards">
      <div class="dash-card">
        <div class="dc-label">CPU</div>
        <div style="display:flex;align-items:flex-end;justify-content:space-between;gap:8px;">
          <div>
            <div class="dc-value" style="color:{dashMetrics.cpu.global>85?'var(--red)':dashMetrics.cpu.global>60?'var(--amber)':'var(--acc)'}">
              <span use:countUp={{ target: dashMetrics.cpu.global, suffix: '%', duration: 900 }}></span>
            </div>
            <div class="dc-bar"><div class="dc-bar-fill" style="width:{dashMetrics.cpu.global}%;background:{dashMetrics.cpu.global>85?'var(--red)':dashMetrics.cpu.global>60?'var(--amber)':'var(--acc)'}"></div></div>
            <div class="dc-sub">{dashMetrics.cpu.cores} {isEN ? 'cores' : 'núcleos'}</div>
          </div>
          <div class="dc-sparkline">{@html sparklineSvg(metricsHistory[dashSelectedHost],'cpu',dashMetrics.cpu.global>85?'#ef4444':dashMetrics.cpu.global>60?'#f59e0b':'#10b981')}</div>
        </div>
      </div>
      <div class="dash-card">
        <div class="dc-label">RAM</div>
        <div style="display:flex;align-items:flex-end;justify-content:space-between;gap:8px;">
          <div>
            <div class="dc-value" style="color:{dashMetrics.memory.percent>85?'var(--red)':dashMetrics.memory.percent>70?'var(--amber)':'var(--blue)'}">
              <span use:countUp={{ target: dashMetrics.memory.percent, suffix: '%', duration: 900 }}></span>
            </div>
            <div class="dc-bar"><div class="dc-bar-fill" style="width:{dashMetrics.memory.percent}%;background:{dashMetrics.memory.percent>85?'var(--red)':dashMetrics.memory.percent>70?'var(--amber)':'var(--blue)'}"></div></div>
            <div class="dc-sub">{(dashMetrics.memory.used_mb/1024).toFixed(1)} / {(dashMetrics.memory.total_mb/1024).toFixed(1)} GB</div>
          </div>
          <div class="dc-sparkline">{@html sparklineSvg(metricsHistory[dashSelectedHost],'ram',dashMetrics.memory.percent>85?'#ef4444':dashMetrics.memory.percent>70?'#f59e0b':'#3b9eff')}</div>
        </div>
      </div>
      <div class="dash-card">
        <div class="dc-label">{isEN ? 'System' : 'Sistema'}</div>
        <div class="dc-value" style="font-size:13px;color:var(--txt);">{dashMetrics.hostname}</div>
        <div class="dc-sub">{dashMetrics.os}</div>
        <div class="dc-sub">Uptime: {dashMetrics.uptime_h}h</div>
        {#if metricsHistory[dashSelectedHost]?.length > 1}
        <div class="dc-sub" style="margin-top:4px;color:#2a4a3a;">📈 {metricsHistory[dashSelectedHost].length} {isEN ? 'samples' : 'muestras'}</div>
        {/if}
      </div>
    </div>
    {#if dashMetrics.cpu.per_core?.length}
    <div class="dash-section">
      <div class="ds-title">{isEN ? 'CPU Cores' : 'Núcleos CPU'}</div>
      <div class="core-grid">
        {#each dashMetrics.cpu.per_core as usage, i}
        <div class="core-item">
          <div class="core-bar-wrap"><div class="core-bar-fill" style="height:{usage}%;background:{usage>85?'rgba(239,68,68,.85)':usage>60?'rgba(245,158,11,.8)':usage>40?'rgba(16,185,129,.65)':'rgba(16,185,129,.45)'};"></div></div>
          <div class="core-label">C{i}</div>
          <div class="core-pct">{usage}%</div>
        </div>
        {/each}
      </div>
    </div>
    {/if}
    {#if dashMetrics.disks?.length}
    <div class="dash-section">
      <div class="ds-title">{isEN ? 'Storage' : 'Almacenamiento'}</div>
      {#each dashMetrics.disks as disk}
      <div class="disk-row">
        <div class="disk-name">{disk.name||disk.mount}</div>
        <div class="disk-bar-wrap"><div class="disk-bar-fill" style="width:{disk.percent}%;background:{disk.percent>90?'var(--red)':disk.percent>75?'var(--amber)':'var(--blue)'}"></div></div>
        <div class="disk-pct" style="color:{disk.percent>90?'var(--red)':disk.percent>75?'var(--amber)':'var(--txt2)'}">{disk.percent}%</div>
        <div class="disk-size">{disk.used_gb}G / {disk.total_gb}G</div>
      </div>
      {/each}
    </div>
    {/if}
    {#if dashMetrics.top_processes?.length}
    <div class="dash-section">
      <div class="ds-title">{isEN ? 'Top Processes (by RAM)' : 'Top procesos (por RAM)'}</div>
      <table class="proc-table">
        <thead><tr><th>{isEN ? 'Process' : 'Proceso'}</th><th>CPU %</th><th>RAM MB</th><th>PID</th></tr></thead>
        <tbody>
          {#each dashMetrics.top_processes as p}
          <tr>
            <td style="font-family:var(--mono);font-size:11px;color:var(--txt);">{p.name}</td>
            <td style="color:{Number(p.cpu)>50?'var(--amber)':'var(--txt2)'}">{p.cpu}</td>
            <td style="color:var(--blue)">{typeof p.mem_mb==='number'?p.mem_mb.toLocaleString():p.mem_mb}</td>
            <td style="color:#334155">{p.pid||'-'}</td>
          </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {/if}
  </div>
  {:else}
    <div class="view-loading"><span style="color:#334155">{isEN ? 'Select a host to view metrics' : 'Selecciona un host para ver métricas'}</span></div>
  {/if}
</div>

<style>
    /* ── View shared ──────────────────────────────── */
    .view-wrap{flex:1;display:flex;flex-direction:column;overflow:hidden;min-height:0;}
    .view-hdr{display:flex;align-items:center;padding:10px 16px;background:rgba(2,4,8,.6);border-bottom:1px solid var(--bdr);flex-shrink:0;gap:10px;}
    .view-title{font-size:13px;font-weight:700;color:var(--txt);white-space:nowrap;}
    .view-select{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt);font-size:12px;padding:4px 8px;cursor:pointer;outline:none;}
    .view-select:focus{border-color:var(--acc-b);}
    .view-btn{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt2);font-size:12px;padding:4px 10px;cursor:pointer;transition:.15s;white-space:nowrap;}
    .view-btn:hover{background:var(--bdr2);color:var(--txt);}
    .view-btn:disabled{opacity:.35;cursor:not-allowed;}
    .view-error{margin:12px 16px;padding:10px 14px;background:rgba(239,68,68,.08);border:1px solid rgba(239,68,68,.2);border-radius:6px;font-size:12px;color:var(--red);}
    .view-loading{flex:1;display:flex;align-items:center;justify-content:center;gap:12px;font-size:13px;color:#334155;}

    /* ── Dashboard auto-refresh badge ────────────── */
    .dash-auto-badge{display:inline-flex;align-items:center;gap:5px;font-size:10px;color:var(--acc);background:rgba(16,185,129,.07);border:1px solid rgba(16,185,129,.15);border-radius:10px;padding:2px 8px;white-space:nowrap;}
    .dash-pulse{width:6px;height:6px;border-radius:50%;background:var(--acc);animation:dash-pulse-anim 2s ease-in-out infinite;}
    @keyframes dash-pulse-anim{0%,100%{opacity:1;transform:scale(1);}50%{opacity:.4;transform:scale(.7);}}
    .dash-last-update{font-size:10px;color:#4a5a6a;white-space:nowrap;}

    /* ── Dashboard cards & layout ─────────────────── */
    .dash-scroll{flex:1;overflow-y:auto;padding:16px;}
    .dash-cards{display:grid;grid-template-columns:repeat(3,1fr);gap:12px;margin-bottom:16px;}
    .dash-card{background:rgba(0,0,0,.2);border:1px solid var(--bdr);border-radius:10px;padding:14px 16px;transition:border-color .3s,box-shadow .3s;}
    .dash-card:hover{border-color:rgba(255,255,255,.08);box-shadow:0 2px 16px rgba(0,0,0,.3);}
    .dc-label{font-size:10px;color:#334155;letter-spacing:.5px;text-transform:uppercase;font-weight:700;margin-bottom:6px;}
    .dc-value{font-size:28px;font-weight:400;margin-bottom:6px;line-height:1;}
    .dc-bar{height:3px;background:var(--bdr);border-radius:2px;margin-bottom:6px;overflow:hidden;}
    .dc-bar-fill{height:100%;border-radius:2px;transition:width .8s cubic-bezier(.4,0,.2,1);}
    .dc-sub{font-size:11px;color:#475569;margin-top:2px;}
    .dc-sparkline{opacity:.85;flex-shrink:0;align-self:flex-end;margin-bottom:2px;}

    /* ── Dashboard sections ───────────────────────── */
    .dash-section{background:rgba(0,0,0,.15);border:1px solid var(--bdr);border-radius:8px;padding:12px 14px;margin-bottom:12px;}
    .ds-title{font-size:11px;color:#475569;font-weight:700;letter-spacing:.3px;text-transform:uppercase;margin-bottom:10px;}

    /* ── CPU cores ─────────────────────────────────── */
    .core-grid{display:flex;gap:6px;flex-wrap:wrap;}
    .core-item{display:flex;flex-direction:column;align-items:center;gap:3px;}
    .core-bar-wrap{width:22px;height:64px;background:var(--bg4);border-radius:4px;overflow:hidden;display:flex;align-items:flex-end;border:1px solid var(--bdr);}
    .core-bar-fill{width:100%;border-radius:3px;transition:height .5s cubic-bezier(.34,1.3,.64,1),background .4s;animation:bar-entry .6s cubic-bezier(.34,1.3,.64,1);}
    @keyframes bar-entry{from{height:0!important;}}
    .core-label{font-size:9px;color:var(--txt3);}
    .core-pct{font-size:9px;color:var(--txt2);font-weight:600;}

    /* ── Disks ─────────────────────────────────────── */
    .disk-row{display:grid;grid-template-columns:100px 1fr 44px 80px;align-items:center;gap:10px;margin-bottom:8px;}
    .disk-name{font-size:12px;color:var(--txt2);font-family:var(--mono);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
    .disk-bar-wrap{height:6px;background:var(--bdr);border-radius:3px;overflow:hidden;}
    .disk-bar-fill{height:100%;border-radius:3px;transition:width .4s ease;}
    .disk-pct{font-size:11px;font-weight:600;text-align:right;}
    .disk-size{font-size:10px;color:#334155;font-family:var(--mono);}

    /* ── Process table ─────────────────────────────── */
    .proc-table{width:100%;border-collapse:collapse;font-size:12px;}
    .proc-table th{background:var(--bg4);color:#475569;padding:5px 10px;text-align:left;font-size:10px;font-weight:700;letter-spacing:.3px;text-transform:uppercase;}
    .proc-table td{padding:5px 10px;border-bottom:1px solid rgba(26,32,48,.4);}
    .proc-table tr:last-child td{border-bottom:none;}

    /* ── Alerts ────────────────────────────────────── */
    .alert-bar{background:rgba(239,68,68,.07);border-bottom:1px solid rgba(239,68,68,.18);padding:6px 14px;flex-shrink:0;box-shadow:0 2px 12px rgba(239,68,68,.06);animation:alert-glow 2s ease-in-out infinite;}
    @keyframes alert-glow{0%,100%{box-shadow:0 2px 12px rgba(239,68,68,.06);}50%{box-shadow:0 2px 16px rgba(239,68,68,.12);}}
    .alert-item{display:flex;align-items:center;gap:8px;font-size:12px;color:var(--txt2);padding:3px 0;}
    .alert-item-ico{flex-shrink:0;}
    .alert-dismiss{background:none;border:none;color:#3a2a2a;cursor:pointer;font-size:13px;margin-left:auto;padding:0 4px;line-height:1;flex-shrink:0;}
    .alert-dismiss:hover{color:var(--red);}
    .alert-badge-btn{position:absolute;top:-4px;right:-4px;background:var(--red);color:#fff;font-size:9px;font-weight:700;border-radius:50%;width:14px;height:14px;display:flex;align-items:center;justify-content:center;line-height:1;}

    /* ── Skeleton loaders ──────────────────────────── */
    @keyframes sk-shimmer{0%{background-position:200% 0;}100%{background-position:-200% 0;}}
    .dash-skeleton{padding:16px;display:flex;flex-direction:column;gap:16px;}
    .sk-card{background:var(--bg3);border:1px solid var(--bdr);border-radius:10px;padding:16px;display:flex;flex-direction:column;gap:10px;}
    .sk-lbl{height:10px;width:40px;border-radius:4px;background:linear-gradient(90deg,var(--bg4) 25%,rgba(255,255,255,.04) 50%,var(--bg4) 75%);background-size:200% 100%;animation:sk-shimmer 1.5s infinite;}
    .sk-val{height:28px;width:70px;border-radius:6px;background:linear-gradient(90deg,var(--bg4) 25%,rgba(255,255,255,.04) 50%,var(--bg4) 75%);background-size:200% 100%;animation:sk-shimmer 1.5s infinite;}
    .sk-bar{height:6px;width:100%;border-radius:3px;background:linear-gradient(90deg,var(--bg4) 25%,rgba(255,255,255,.04) 50%,var(--bg4) 75%);background-size:200% 100%;animation:sk-shimmer 1.5s infinite;}
    .sk-sub{height:9px;width:80px;border-radius:4px;background:linear-gradient(90deg,var(--bg4) 25%,rgba(255,255,255,.04) 50%,var(--bg4) 75%);background-size:200% 100%;animation:sk-shimmer 1.5s .1s infinite;}
    .sk-sub.short{width:50px;}
    .sk-section{background:var(--bg3);border:1px solid var(--bdr);border-radius:10px;padding:16px;display:flex;flex-direction:column;gap:8px;}
    .sk-row{height:11px;border-radius:4px;background:linear-gradient(90deg,var(--bg4) 25%,rgba(255,255,255,.04) 50%,var(--bg4) 75%);background-size:200% 100%;animation:sk-shimmer 1.5s infinite;}
    .sk-row.short{width:60%;}

    /* ── Light mode overrides ─────────────────────── */
    :global(:root.light) .view-wrap{background:var(--bg);}
    :global(:root.light) .view-hdr{background:rgba(224,230,238,.8);border-bottom-color:var(--bdr);}
    :global(:root.light) .view-loading{color:var(--txt3);}
    :global(:root.light) .dash-card{background:#fff;border-color:var(--bdr);}
    :global(:root.light) .dc-label{color:var(--txt3);}
    :global(:root.light) .dc-sub{color:var(--txt2);}
    :global(:root.light) .ds-title{color:var(--txt2);}
    :global(:root.light) .disk-size{color:var(--txt2);}
    :global(:root.light) .proc-table th{color:var(--txt2);background:var(--bg3);}
    :global(:root.light) .alert-dismiss{color:var(--red);}
</style>
