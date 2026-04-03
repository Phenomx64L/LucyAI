<script>
    import { invoke } from '@tauri-apps/api/core';
    import { createEventDispatcher, onMount, onDestroy, tick } from 'svelte';

    export let hosts = [];
    // svelte-ignore export_let_unused
    export let hostName = '';
    export let isEN = false;

    const dispatch = createEventDispatcher();

    let logLines       = [];
    let logPath        = '';
    let logSelectedHost = 'local';
    let logFilter      = '';
    let logLoading     = false;
    let logError       = '';
    let logAutoScroll  = true;
    let logRefreshTimer = null;
    let logLinesEl     = null;
    let logTailCount   = 100;

    $: filteredLog = logFilter
        ? logLines.filter(l => l.toLowerCase().includes(logFilter.toLowerCase()))
        : logLines;

    $: logLevelClass = (line) => {
        const l = line.toLowerCase();
        if (l.includes('error') || l.includes('fatal') || l.includes('critical')) return 'log-error';
        if (l.includes('warn'))  return 'log-warn';
        if (l.includes('info'))  return 'log-info';
        if (l.includes('debug')) return 'log-debug';
        return '';
    };

    async function startLogViewer() {
        if (!logPath.trim()) return;
        logLines = []; logError = '';
        await refreshLog();
        if (logRefreshTimer) clearInterval(logRefreshTimer);
        logRefreshTimer = setInterval(refreshLog, 5000);
    }

    function stopLogViewer() {
        if (logRefreshTimer) { clearInterval(logRefreshTimer); logRefreshTimer = null; }
    }

    async function refreshLog() {
        if (!logPath.trim()) return;
        logLoading = true; logError = '';
        try {
            let newLines = [];
            if (logSelectedHost === 'local') {
                newLines = await invoke('read_log_tail', { path: logPath.trim(), lines: logTailCount });
            } else {
                const h = hosts.find(x => x.id === logSelectedHost);
                if (!h) { logError = isEN ? 'Host not found.' : 'Host no encontrado.'; logLoading = false; return; }
                let pwd = '';
                try { pwd = await invoke('get_host_credential', { hostId: h.id }); } catch(e){}
                if (h.type === 'windows') {
                    newLines = await invoke('read_remote_log_windows', { host:h.host, username:h.username, password:pwd, path:logPath.trim(), lines:logTailCount });
                } else {
                    newLines = await invoke('read_remote_log_linux', { host:h.host, username:h.username, path:logPath.trim(), lines:logTailCount, port:h.port||22, keyPath:h.sshKeyPath||null });
                }
            }
            logLines = newLines;
            if (logAutoScroll) await tick().then(() => { if(logLinesEl) logLinesEl.scrollTop = logLinesEl.scrollHeight; });
        } catch(e) { logError = String(e); }
        logLoading = false;
    }

    onMount(() => {
        startLogViewer();
    });

    onDestroy(() => {
        stopLogViewer();
    });
</script>

<div class="view-wrap">
  <div class="view-hdr">
    <div class="view-title">🔍 Log Viewer</div>
    <div style="display:flex;align-items:center;gap:8px;flex:1;margin-left:12px;min-width:0;">
      <select class="view-select" bind:value={logSelectedHost} style="flex-shrink:0;">
        <option value="local">🖥 Local</option>
        {#each hosts as h}<option value={h.id}>{h.type==='windows'?'🖥':'🐧'} {h.name}</option>{/each}
      </select>
      <input class="minp" placeholder={logSelectedHost==='local'?'C:/inetpub/logs/archivo.log':'/var/log/nginx/error.log'} bind:value={logPath} style="flex:1;height:32px;padding:0 10px;font-family:var(--mono);font-size:12px;" on:keydown={(e)=>{if(e.key==='Enter')startLogViewer();}}>
      <button class="view-btn" on:click={startLogViewer} disabled={!logPath.trim()||logLoading}>{logLoading?'⏳':(isEN ? '▶ Open' : '▶ Abrir')}</button>
      <button class="view-btn" on:click={stopLogViewer} disabled={!logRefreshTimer}>⏹</button>
    </div>
  </div>
  <div class="log-toolbar">
    <input class="minp" placeholder={isEN ? "Filter lines..." : "Filtrar líneas..."} bind:value={logFilter} style="flex:1;height:28px;padding:0 8px;font-size:12px;">
    <select class="view-select" bind:value={logTailCount} style="width:110px;">
      <option value={50}>50 {isEN ? 'lines' : 'líneas'}</option>
      <option value={100}>100 {isEN ? 'lines' : 'líneas'}</option>
      <option value={250}>250 {isEN ? 'lines' : 'líneas'}</option>
      <option value={500}>500 {isEN ? 'lines' : 'líneas'}</option>
    </select>
    <label style="display:flex;align-items:center;gap:5px;font-size:11px;color:var(--txt2);cursor:pointer;white-space:nowrap;">
      <input type="checkbox" bind:checked={logAutoScroll}> Auto-scroll
    </label>
    <span style="font-size:10px;color:#334155;margin-left:auto;white-space:nowrap;">{filteredLog.length} {isEN ? 'lines' : 'líneas'}{logFilter?` / ${logLines.length} total`:''}</span>
  </div>
  {#if logError}<div class="view-error">⚠️ {logError}</div>{/if}
  <div class="log-lines" bind:this={logLinesEl}>
    {#if !logLines.length && !logLoading}
      <div style="padding:30px;text-align:center;color:#334155;font-style:italic;font-size:12px;">
        {logPath.trim() ? (isEN ? 'Click ▶ Open to tail log' : 'Haz clic en ▶ Abrir para cargar el log') : (isEN ? 'Enter log file path and click ▶ Open' : 'Introduce la ruta del archivo de log y pulsa ▶ Abrir')}
      </div>
    {:else}
      {#each filteredLog as line, i}
      <div class="log-line {logLevelClass(line)}">
        <span class="log-num">{logLines.length-filteredLog.length+i+1}</span>
        <span class="log-txt">{line}</span>
      </div>
      {/each}
      {#if logLoading}<div class="log-line" style="color:#334155;font-style:italic;">{isEN ? 'Updating...' : 'Actualizando...'}</div>{/if}
    {/if}
  </div>
</div>

<style>
    .minp{width:100%;background:rgba(0,0,0,.3);border:1px solid var(--bdr2);color:white;padding:10px 12px;border-radius:7px;outline:none;font-family:inherit;font-size:13px;transition:border-color .2s;}
    .minp:focus{border-color:var(--acc-b);}
    .minp:disabled{opacity:.5;cursor:not-allowed;}
    .view-wrap{flex:1;display:flex;flex-direction:column;overflow:hidden;min-height:0;}
    .view-hdr{display:flex;align-items:center;padding:10px 16px;background:rgba(2,4,8,.6);border-bottom:1px solid var(--bdr);flex-shrink:0;gap:10px;}
    .view-title{font-size:13px;font-weight:700;color:var(--txt);white-space:nowrap;}
    .view-select{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt);font-size:12px;padding:4px 8px;cursor:pointer;outline:none;}
    .view-select:focus{border-color:var(--acc-b);}
    .view-btn{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt2);font-size:12px;padding:4px 10px;cursor:pointer;transition:.15s;white-space:nowrap;}
    .view-btn:hover{background:var(--bdr2);color:var(--txt);}
    .view-btn:disabled{opacity:.35;cursor:not-allowed;}
    .view-error{margin:12px 16px;padding:10px 14px;background:rgba(255,68,68,.08);border:1px solid rgba(255,68,68,.2);border-radius:6px;font-size:12px;color:var(--red);}
    .log-toolbar{display:flex;align-items:center;gap:8px;padding:6px 12px;background:rgba(2,4,8,.4);border-bottom:1px solid var(--bdr);flex-shrink:0;}
    .log-lines{flex:1;overflow-y:auto;font-family:var(--mono);font-size:11px;background:#020407;}
    .log-line{display:flex;gap:0;align-items:baseline;padding:1px 0;border-bottom:1px solid rgba(26,32,48,.2);line-height:1.5;}
    .log-line:hover{background:rgba(255,255,255,.02);}
    .log-num{min-width:48px;text-align:right;padding-right:12px;color:#1a2a3a;user-select:none;flex-shrink:0;}
    .log-txt{flex:1;color:#6a7a8a;word-break:break-all;padding-right:12px;}
    .log-line.log-error .log-txt{color:var(--red);}
    .log-line.log-error{background:rgba(255,68,68,.04);}
    .log-line.log-warn  .log-txt{color:var(--amber);}
    .log-line.log-warn {background:rgba(255,170,0,.03);}
    .log-line.log-info  .log-txt{color:#4a7a9a;}
    .log-line.log-debug .log-txt{color:#0f7b5a;}
    :global(:root.light .log-lines){background:var(--bg2);}
    :global(:root.light .log-line)          { border-bottom-color:rgba(0,0,0,.07); }
    :global(:root.light .log-line:hover)    { background:rgba(0,0,0,.03); }
    :global(:root.light .log-line.log-info  .log-txt){ color:#1a5a8a; }
    :global(:root.light .log-line.log-debug .log-txt){ color:#1a5a2a; }
</style>
