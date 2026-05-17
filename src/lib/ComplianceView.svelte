<script>
    import { createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import ShieldCheck from '@tabler/icons-svelte/icons/shield-check';

    import FileText from '@tabler/icons-svelte/icons/file-text';

    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';

    import Lightbulb from '@tabler/icons-svelte/icons/bulb';
    import { complianceReports } from '$lib/stores';
    import { exportCompliancePdf } from '$lib/reports/ReportGenerator';
    import cisLinux from '$lib/compliance/cis-linux.json';
    import cisWindows from '$lib/compliance/cis-windows.json';

    const dispatch = createEventDispatcher();

    export let hosts      = [];
    export let hostName   = '';
    // svelte-ignore export_let_unused
    export let lucyConfig = {};
    // svelte-ignore export_let_unused
    export let userLang   = 'es-MX';
    export let isEN       = false;

    let selectedHost  = 'local';
    let scanning      = false;
    let scanProgress  = 0;
    let error         = '';
    let filterStatus  = 'all'; // all | pass | fail
    let filterSeverity = 'all';
    let filterCategory = 'all';

    $: report = $complianceReports[selectedHost] || null;
    $: checks = selectedHost === 'local' || (hosts.find(h=>h.id===selectedHost)?.type === 'windows') ? cisWindows : cisLinux;
    $: categories = [...new Set(checks.map(c => c.category))];

    $: enrichedResults = report ? report.results.map(r => {
        const check = checks.find(c => c.id === r.checkId);
        return { ...r, ...(check || { title: r.checkId, category: '?', severity: 'medium', remediation: '' }) };
    }) : [];

    $: filteredResults = enrichedResults
        .filter(r => filterStatus  === 'all' || (filterStatus === 'pass' ? r.passed : !r.passed))
        .filter(r => filterSeverity === 'all' || r.severity === filterSeverity)
        .filter(r => filterCategory === 'all' || r.category === filterCategory);

    $: passCount = enrichedResults.filter(r => r.passed).length;
    $: failCount = enrichedResults.filter(r => !r.passed).length;
    $: passRate  = enrichedResults.length ? Math.round((passCount / enrichedResults.length) * 100) : 0;

    function toast(msg, type='info') { dispatch('toast', { msg, type }); }

    function evaluateCheck(check, result) {
        const stdout = (result.stdout || '').trim();
        const exitCode = result.exit_code ?? result.exitCode ?? -1;
        const expect = check.expect || {};
        if (expect.stdout_contains && !stdout.includes(expect.stdout_contains)) return false;
        if (expect.stdout_not_contains && stdout.includes(expect.stdout_not_contains)) return false;
        if (expect.exit_code !== undefined && exitCode !== expect.exit_code) return false;
        if (expect.regex && !new RegExp(expect.regex).test(stdout)) return false;
        // Default: if has stdout_contains expectation, it was already checked; otherwise check exit code
        if (!expect.stdout_contains && !expect.regex && expect.exit_code === undefined) return exitCode === 0;
        return true;
    }

    async function runScan() {
        scanning = true; error = ''; scanProgress = 0;
        try {
            const isLocal = selectedHost === 'local';
            const h = isLocal ? null : hosts.find(x => x.id === selectedHost);
            if (!isLocal && !h) { error = 'Host not found'; scanning = false; return; }
            const isWin = isLocal || h?.type === 'windows';
            const checkList = isWin ? cisWindows : cisLinux;
            const checksJson = JSON.stringify(checkList.map(c => ({ id: c.id, command: c.command })));

            let pwd = '';
            if (!isLocal && h) {
                try { pwd = await invoke('get_host_credential', { hostId: h.id }); } catch(e){}
            }

            let rawResults;
            if (isLocal) {
                rawResults = await invoke('run_compliance_local', { checksJson });
            } else if (isWin) {
                rawResults = await invoke('run_compliance_windows', { host: h.host, username: h.username, password: pwd, checksJson });
            } else {
                rawResults = await invoke('run_compliance_linux', { host: h.host, username: h.username, checksJson, port: h.port||22, keyPath: h.sshKeyPath||null });
            }

            // Ensure array
            if (!Array.isArray(rawResults)) rawResults = [rawResults];

            const results = rawResults.map(r => {
                const check = checkList.find(c => c.id === r.id);
                return {
                    checkId: r.id,
                    passed: check ? evaluateCheck(check, r) : r.exit_code === 0,
                    stdout: r.stdout || '',
                    exitCode: r.exit_code ?? r.exitCode ?? -1,
                };
            });

            const passR = results.filter(r => r.passed).length;
            const scanReport = {
                hostId: selectedHost,
                hostName: isLocal ? hostName : (h?.name || selectedHost),
                timestamp: Date.now(),
                osType: isWin ? 'windows' : 'linux',
                results,
                passRate: Math.round((passR / results.length) * 100),
            };
            $complianceReports = { ...$complianceReports, [selectedHost]: scanReport };
            toast(isEN
                ? `Compliance scan done: ${passR}/${results.length} passed (${scanReport.passRate}%)`
                : `Scan completado: ${passR}/${results.length} pasaron (${scanReport.passRate}%)`);
        } catch(e) {
            error = String(e);
        }
        scanning = false;
    }

    function sevColor(s) {
        if (s === 'critical') return 'var(--red)';
        if (s === 'high') return 'var(--amber)';
        if (s === 'medium') return 'var(--blue)';
        return '#4a5a6a';
    }

    let exporting = false;
    async function exportPdf() {
        if (!report) return;
        exporting = true;
        try {
            await exportCompliancePdf({
                hostName: report.hostName,
                scanDate: new Date(report.timestamp).toLocaleString(),
                passRate, passCount, failCount,
                results: enrichedResults.map(r => ({
                    title: r.title, category: r.category,
                    severity: r.severity, passed: r.passed,
                    remediation: r.remediation,
                })),
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
        return `${Math.floor(d/3600000)}h`;
    }
</script>

<div class="view-wrap">
  <div class="view-hdr">
    <div class="view-title" style="display:flex;align-items:center;gap:6px;"><ShieldCheck size={13} strokeWidth={2}/> {isEN ? 'Compliance Scanning' : 'Auditoría de Compliance'}</div>
    <div style="display:flex;align-items:center;gap:8px;margin-left:auto;flex-wrap:wrap;">
      <select class="view-select" bind:value={selectedHost}>
        <option value="local">⊡ Local ({hostName})</option>
        {#each hosts as h}<option value={h.id}>{h.type==='windows'?'⊡':'◈'} {h.name}</option>{/each}
      </select>
      <button class="view-btn" on:click={runScan} disabled={scanning} style="display:flex;align-items:center;gap:5px;">
        {#if scanning}↻ {isEN ? 'Scanning...' : 'Escaneando...'}{:else}<ShieldCheck size={12} strokeWidth={2}/> Run CIS Scan{/if}
      </button>
      {#if report}
        <button class="view-btn" on:click={exportPdf} disabled={exporting} title="Export PDF" style="display:flex;align-items:center;gap:5px;">
          {#if exporting}↻{:else}<FileText size={12} strokeWidth={2}/>{/if} PDF
        </button>
        <span style="font-size:10px;color:#4a5a6a;">{checks.length} checks · {relTime(report.timestamp)}</span>
      {/if}
    </div>
  </div>

  {#if error}
    <div class="view-error" style="display:flex;align-items:center;gap:6px;"><AlertTriangle size={12} strokeWidth={2}/> {error}</div>
  {/if}

  {#if report}
  <!-- Score bar -->
  <div class="comp-score">
    <div class="comp-score-ring" style="--pct:{passRate}; --color:{passRate>=80?'var(--acc)':passRate>=50?'var(--amber)':'var(--red)'}">
      <span class="comp-score-val">{passRate}%</span>
    </div>
    <div class="comp-score-stats">
      <div class="comp-stat pass">✓ {passCount} {isEN ? 'Passed' : 'Pasaron'}</div>
      <div class="comp-stat fail">✗ {failCount} {isEN ? 'Failed' : 'Fallaron'}</div>
      <div class="comp-stat total">{enrichedResults.length} total · {report.osType === 'windows' ? 'Windows CIS' : 'Linux CIS'}</div>
    </div>
    <div class="comp-filters">
      <select class="view-select" bind:value={filterStatus}>
        <option value="all">{isEN ? 'All' : 'Todos'}</option>
        <option value="pass">✓ Pass</option>
        <option value="fail">✗ Fail</option>
      </select>
      <select class="view-select" bind:value={filterSeverity}>
        <option value="all">{isEN ? 'All Severity' : 'Severidad'}</option>
        <option value="critical">Critical</option>
        <option value="high">High</option>
        <option value="medium">Medium</option>
        <option value="low">Low</option>
      </select>
      <select class="view-select" bind:value={filterCategory}>
        <option value="all">{isEN ? 'All Categories' : 'Categorías'}</option>
        {#each categories as cat}<option value={cat}>{cat}</option>{/each}
      </select>
    </div>
  </div>

  <div class="comp-scroll">
    {#each filteredResults as r}
    <div class="comp-item" class:pass={r.passed} class:fail={!r.passed}>
      <div class="comp-item-status">{r.passed ? '✓' : '✗'}</div>
      <div class="comp-item-body">
        <div class="comp-item-title">
          <span class="comp-id">{r.checkId}</span>
          {r.title}
          <span class="comp-sev" style="color:{sevColor(r.severity)}">{r.severity}</span>
          <span class="comp-cat">{r.category}</span>
        </div>
        {#if !r.passed && r.remediation}
        <div class="comp-item-rem" style="display:flex;align-items:flex-start;gap:5px;"><Lightbulb size={11} strokeWidth={2}/> {r.remediation}</div>
        {/if}
        {#if r.stdout}
        <details class="comp-item-detail">
          <summary>Output</summary>
          <pre>{r.stdout.replace(/\|/g, '\n')}</pre>
        </details>
        {/if}
      </div>
    </div>
    {/each}
    {#if !filteredResults.length}
    <div style="text-align:center;color:#334155;padding:30px;font-size:13px;">{isEN ? 'No results match filters' : 'Sin resultados con estos filtros'}</div>
    {/if}
  </div>
  {:else if !scanning}
  <div class="view-loading"><span style="color:#334155">{isEN ? 'Select a host and run a CIS Benchmark scan' : 'Selecciona un host y ejecuta un escaneo CIS Benchmark'}</span></div>
  {:else}
  <div class="view-loading"><span style="color:var(--acc)">↻ {isEN ? 'Running compliance checks...' : 'Ejecutando checks de compliance...'}</span></div>
  {/if}
</div>

<style>
    .view-wrap{flex:1;display:flex;flex-direction:column;overflow:hidden;min-height:0;}
    .view-hdr{display:flex;align-items:center;padding:10px 16px;background:rgba(2,4,8,.6);border-bottom:1px solid var(--bdr);flex-shrink:0;gap:10px;flex-wrap:wrap;}
    .view-title{font-size:13px;font-weight:700;color:var(--txt);white-space:nowrap;}
    .view-select{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt);font-size:11px;padding:3px 6px;cursor:pointer;outline:none;}
    .view-btn{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt2);font-size:12px;padding:4px 10px;cursor:pointer;transition:.15s;white-space:nowrap;}
    .view-btn:hover{background:var(--bdr2);color:var(--txt);}
    .view-btn:disabled{opacity:.35;cursor:not-allowed;}
    .view-error{margin:12px 16px;padding:10px 14px;background:rgba(255,68,68,.08);border:1px solid rgba(255,68,68,.2);border-radius:6px;font-size:12px;color:var(--red);}
    .view-loading{flex:1;display:flex;align-items:center;justify-content:center;font-size:13px;}

    /* Score */
    .comp-score{display:flex;align-items:center;gap:16px;padding:14px 16px;border-bottom:1px solid var(--bdr);flex-shrink:0;flex-wrap:wrap;}
    .comp-score-ring{width:56px;height:56px;border-radius:50%;background:conic-gradient(var(--color) calc(var(--pct)*3.6deg), rgba(255,255,255,.05) 0);display:flex;align-items:center;justify-content:center;flex-shrink:0;position:relative;}
    .comp-score-ring::before{content:'';position:absolute;width:42px;height:42px;border-radius:50%;background:var(--bg2,#0b0e14);}
    .comp-score-val{position:relative;z-index:1;font-size:14px;font-weight:700;color:var(--txt);}
    .comp-score-stats{display:flex;flex-direction:column;gap:2px;}
    .comp-stat{font-size:12px;color:var(--txt2);}
    .comp-stat.pass{color:var(--acc);}
    .comp-stat.fail{color:var(--red);}
    .comp-stat.total{font-size:10px;color:#475569;}
    .comp-filters{display:flex;gap:6px;margin-left:auto;}

    /* Results list */
    .comp-scroll{flex:1;overflow-y:auto;padding:8px 16px 16px;}
    .comp-item{display:flex;gap:10px;padding:8px 10px;border:1px solid var(--bdr);border-radius:6px;margin-bottom:6px;transition:.15s;}
    .comp-item:hover{border-color:rgba(255,255,255,.06);}
    .comp-item.pass .comp-item-status{color:var(--acc);}
    .comp-item.fail{border-left:2px solid var(--red);background:rgba(255,68,68,.03);}
    .comp-item.fail .comp-item-status{color:var(--red);}
    .comp-item-status{font-size:16px;font-weight:700;width:20px;text-align:center;flex-shrink:0;padding-top:2px;}
    .comp-item-body{flex:1;min-width:0;}
    .comp-item-title{font-size:12px;color:var(--txt);display:flex;align-items:center;gap:6px;flex-wrap:wrap;}
    .comp-id{font-size:10px;color:var(--blue);font-weight:700;font-family:var(--mono);}
    .comp-sev{font-size:9px;font-weight:700;text-transform:uppercase;letter-spacing:.3px;margin-left:auto;}
    .comp-cat{font-size:9px;color:#475569;background:rgba(255,255,255,.04);padding:1px 5px;border-radius:4px;}
    .comp-item-rem{font-size:11px;color:var(--amber);margin-top:4px;line-height:1.5;}
    .comp-item-detail{margin-top:4px;}
    .comp-item-detail summary{font-size:10px;color:#4a5a6a;cursor:pointer;}
    .comp-item-detail pre{font-size:10px;color:#6a7a8a;font-family:var(--mono);background:rgba(0,0,0,.2);padding:6px 8px;border-radius:4px;margin-top:4px;overflow-x:auto;white-space:pre-wrap;word-break:break-all;}

    :global(:root.light) .comp-score-ring::before{background:#fff;}
    :global(:root.light) .comp-item{background:#fff;}
    :global(:root.light) .comp-item.fail{background:rgba(255,68,68,.04);}
</style>
