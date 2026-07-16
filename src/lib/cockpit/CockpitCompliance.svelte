<script>
  /* ==========================================================================
     Lucy 2.0 — Compliance (cockpit)  ·  Phase F4/views. LIVE.
     Mirrors the classic ComplianceView: loads the CIS check set, runs the
     commands on the local host via `run_compliance_local`, evaluates each
     result against its `expect` rules (same logic as V1), and reports a
     pass-rate score + per-check status. Auto-scans once on mount; the button
     re-scans. Representative data is the FALLBACK for a plain-browser preview.
     V1's status is binary pass/fail — we triage a FAIL by severity into
     "Falla" (critical/high) vs "Aviso" (medium/low) so the score cards stay
     meaningful. Local host (Windows) only for now. Additive — does not touch
     ComplianceView.svelte.
     ========================================================================== */
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { copyToClipboard } from '$lib/lucy-api';
  import cisWindows from '$lib/compliance/cis-windows.json';
  import ShieldCheck from '@tabler/icons-svelte/icons/shield-check';
  import ScanSearch from '@tabler/icons-svelte/icons/scan';
  import CircleCheck from '@tabler/icons-svelte/icons/circle-check';
  import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
  import CircleX from '@tabler/icons-svelte/icons/circle-x';
  import ChevronDown from '@tabler/icons-svelte/icons/chevron-down';
  import Copy from '@tabler/icons-svelte/icons/copy';
  import Check from '@tabler/icons-svelte/icons/check';
  import Download from '@tabler/icons-svelte/icons/download';

  const CHECKS = cisWindows; // local host = Windows (PRECISION-X)
  const SEV_LABEL = { critical: 'crítica', high: 'alta', medium: 'media', low: 'baja' };
  const ICON = { pass: CircleCheck, warn: AlertTriangle, fail: CircleX };

  // Same evaluation logic as ComplianceView.evaluateCheck (kept in sync).
  function evaluateCheck(check, result) {
    const stdout = (result.stdout || '').trim();
    const exitCode = result.exit_code ?? result.exitCode ?? -1;
    const expect = check.expect || {};
    if (expect.stdout_contains && !stdout.includes(expect.stdout_contains)) return false;
    if (expect.stdout_not_contains && stdout.includes(expect.stdout_not_contains)) return false;
    if (expect.exit_code !== undefined && exitCode !== expect.exit_code) return false;
    if (expect.regex && !new RegExp(expect.regex).test(stdout)) return false;
    if (!expect.stdout_contains && !expect.regex && expect.exit_code === undefined) return exitCode === 0;
    return true;
  }
  const statusOf = (passed, severity) =>
    passed ? 'pass' : (severity === 'critical' || severity === 'high') ? 'fail' : 'warn';

  // Deterministic representative fallback (browser preview / no backend).
  const SAMPLE_FAILS = new Set(['W3.2', 'W4.1', 'W7.2', 'W8.3']);
  function enrich(rawList, useSample) {
    return CHECKS.map((c) => {
      const r = useSample ? null : rawList.find((x) => x.id === c.id);
      const passed = useSample ? !SAMPLE_FAILS.has(c.id) : (r ? evaluateCheck(c, r) : false);
      return {
        id: c.id, title: c.title, category: c.category, severity: c.severity,
        remediation: c.remediation, command: c.command, passed, status: statusOf(passed, c.severity),
        // Drill-down: what ran and what it returned (why the verdict).
        stdout: String(r?.stdout ?? '').trim(), exitCode: r?.exit_code ?? r?.exitCode ?? null,
      };
    });
  }

  let results = $state(null);
  let scanning = $state(false);
  let live = $state(false);
  let lastScan = $state('');
  let error = $state('');
  let filter = $state('all');
  let expandedId = $state(null);
  let _copiedKey = $state(null);
  function toggleCheck(id) { expandedId = expandedId === id ? null : id; }
  async function copyKey(key, text) {
    try { await copyToClipboard(text); } catch { try { navigator.clipboard?.writeText(text); } catch {} }
    _copiedKey = key; setTimeout(() => { if (_copiedKey === key) _copiedKey = null; }, 1200);
  }
  function exportReport() {
    if (!results) return;
    const head = `Compliance CIS — PRECISION-X — ${score}% conforme (${pass} conformes · ${warn} avisos · ${fail} fallas)${lastScan ? ` — ${lastScan}` : ''}`;
    const body = results.map((r) => {
      const tag = r.status === 'pass' ? 'OK  ' : r.status === 'fail' ? 'FALLA' : 'AVISO';
      const fix = r.status !== 'pass' && r.remediation ? `\n      ↳ ${r.remediation}` : '';
      return `[${tag}] ${r.id} — ${r.title} (${SEV_LABEL[r.severity] ?? r.severity})${fix}`;
    }).join('\n');
    copyKey('export', `${head}\n\n${body}`);
  }

  const pass = $derived(results ? results.filter((r) => r.status === 'pass').length : 0);
  const warn = $derived(results ? results.filter((r) => r.status === 'warn').length : 0);
  const fail = $derived(results ? results.filter((r) => r.status === 'fail').length : 0);
  const score = $derived(results && results.length ? Math.round((results.filter((r) => r.passed).length / results.length) * 100) : 0);
  const scoreBand = $derived(score >= 85 ? 'good' : score >= 60 ? 'mid' : 'bad');
  const shown = $derived(!results ? [] : filter === 'all' ? results : results.filter((r) => r.status === filter));

  const FILTERS = $derived([
    { id: 'all',  label: 'Todos',     n: results ? results.length : 0 },
    { id: 'pass', label: 'Conformes', n: pass },
    { id: 'warn', label: 'Avisos',    n: warn },
    { id: 'fail', label: 'Fallas',    n: fail },
  ]);

  async function scan() {
    if (scanning) return;
    scanning = true; error = '';
    try {
      const checksJson = JSON.stringify(CHECKS.map((c) => ({ id: c.id, command: c.command })));
      let raw = await invoke('run_compliance_local', { checksJson });
      if (!Array.isArray(raw)) raw = [raw];
      results = enrich(raw, false);
      live = true;
      lastScan = new Date().toLocaleTimeString('es', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    } catch (e) {
      error = String(e);
      if (!results) results = enrich([], true);
    } finally {
      scanning = false;
    }
  }

  onMount(() => { scan(); });
</script>

<div class="cmp">
  <div class="cmp-head">
    <span class="cmp-title">Compliance</span>
    <span class="host-pill"><ShieldCheck size={14} stroke={1.75} /> CIS · PRECISION-X · {CHECKS.length} checks</span>
    {#if live && lastScan}<span class="live"><span class="live-dot"></span>escaneado {lastScan}</span>{/if}
    <button class="scan-btn" class:busy={scanning} onclick={scan} disabled={scanning}>
      <ScanSearch size={15} stroke={1.75} /> {scanning ? 'Escaneando…' : 'Escanear'}
    </button>
    {#if results}
      <button class="cmp-tool" onclick={exportReport} title="Copiar informe de cumplimiento" aria-label="Copiar informe">
        {#if _copiedKey === 'export'}<Check size={15} stroke={2} />{:else}<Download size={15} stroke={1.75} />{/if}
      </button>
    {/if}
  </div>

  {#if !results && scanning}
    <div class="cmp-loading"><ScanSearch size={30} stroke={1.4} /><p class="ck-shimmer">Ejecutando {CHECKS.length} controles CIS en PRECISION-X…</p></div>
  {:else if results}
    <div class="cmp-top">
      <div class="score {scoreBand}">
        <div class="score-ring" style="--pct:{score}">
          <div class="score-inner"><span class="score-n">{score}<span class="score-pct">%</span></span><span class="score-l">conforme</span></div>
        </div>
      </div>
      <div class="summary">
        <div class="scard pass"><div class="scard-n">{pass}</div><div class="scard-l">conformes</div></div>
        <div class="scard warn"><div class="scard-n">{warn}</div><div class="scard-l">avisos</div></div>
        <div class="scard fail"><div class="scard-n">{fail}</div><div class="scard-l">fallas</div></div>
      </div>
    </div>

    {#if !error && !live}<div class="cmp-note">Datos de ejemplo — sin backend disponible en esta vista previa.</div>{/if}

    <div class="filters">
      {#each FILTERS as f}
        <button class="fchip" class:on={filter === f.id} onclick={() => (filter = f.id)}>{f.label}<span class="fchip-n">{f.n}</span></button>
      {/each}
    </div>

    <div class="checks">
      {#each shown as c (c.id)}
        {@const Icon = ICON[c.status]}
        <div class="check {c.status}" class:open={expandedId === c.id}>
          <button class="chk-head" onclick={() => toggleCheck(c.id)} aria-expanded={expandedId === c.id}>
            <span class="chk-icon"><Icon size={18} stroke={1.85} /></span>
            <div class="chk-main">
              <div class="chk-name">{c.title}</div>
              <div class="chk-meta">
                <span class="chk-id">{c.id}</span>
                <span class="chk-cat">{c.category}</span>
                {#if c.status !== 'pass' && c.remediation}<span class="chk-fix">↳ {c.remediation}</span>{/if}
              </div>
            </div>
            <span class="chk-sev sev-{c.severity}">{SEV_LABEL[c.severity] ?? c.severity}</span>
            <span class="chk-st">{c.status === 'pass' ? 'OK' : c.status === 'fail' ? 'Falla' : 'Aviso'}</span>
            <span class="chk-chev"><ChevronDown size={15} stroke={1.75} /></span>
          </button>
          {#if expandedId === c.id}
            <div class="chk-detail">
              {#if c.status !== 'pass' && c.remediation}
                <div class="cd-fix">
                  <ShieldCheck size={13} stroke={1.75} />
                  <span class="cd-fix-txt">{c.remediation}</span>
                  <button class="cd-copy" title="Copiar remediación" aria-label="Copiar remediación" onclick={() => copyKey('fix-' + c.id, c.remediation)}>{#if _copiedKey === 'fix-' + c.id}<Check size={12} stroke={2} />{:else}<Copy size={12} stroke={1.75} />{/if}</button>
                </div>
              {/if}
              {#if c.command}
                <div class="cd-label">Comando de verificación</div>
                <div class="cd-code"><code>{c.command}</code><button class="cd-copy" title="Copiar comando" aria-label="Copiar comando" onclick={() => copyKey('cmd-' + c.id, c.command)}>{#if _copiedKey === 'cmd-' + c.id}<Check size={12} stroke={2} />{:else}<Copy size={12} stroke={1.75} />{/if}</button></div>
              {/if}
              {#if c.stdout}
                <div class="cd-label">Salida observada{#if c.exitCode != null} · exit {c.exitCode}{/if}</div>
                <pre class="cd-out">{c.stdout.length > 1200 ? c.stdout.slice(0, 1200) + '\n… (truncado)' : c.stdout}</pre>
              {:else if live}
                <div class="cd-label dim">Sin salida capturada por el control.</div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
      {#if shown.length === 0}<div class="chk-empty">Sin controles en este filtro.</div>{/if}
    </div>
  {:else}
    <div class="cmp-loading"><ShieldCheck size={30} stroke={1.4} /><p>{error || 'Pulsa Escanear para evaluar el cumplimiento.'}</p></div>
  {/if}
</div>

<style>
  .cmp { height: 100%; overflow-y: auto; padding: 18px 22px; display: flex; flex-direction: column; }

  .cmp-head { display: flex; align-items: center; gap: 12px; margin-bottom: 18px; }
  .cmp-title { font-size: var(--fs-title); font-weight: var(--fw-medium); color: var(--text-primary); }
  .host-pill { display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--text-muted); background: var(--surface-2); border: 1px solid var(--border); padding: 4px 10px; border-radius: var(--r-sm); }
  .live { display: flex; align-items: center; gap: 6px; font-size: var(--fs-caption); color: var(--accent); }
  .live-dot { width: 7px; height: 7px; border-radius: var(--r-pill); background: var(--accent); animation: cmp-pulse 1.6s var(--ease-out) infinite; }
  @keyframes cmp-pulse { 0% { box-shadow: 0 0 0 0 rgba(61,214,164,0.5); } 70% { box-shadow: 0 0 0 5px rgba(61,214,164,0); } 100% { box-shadow: 0 0 0 0 rgba(61,214,164,0); } }
  .scan-btn { margin-left: auto; display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--accent-ink); background: var(--accent); border: 0; border-radius: var(--r-md); padding: 6px 12px; cursor: pointer; transition: background var(--dur-fast) var(--ease-out); }
  .scan-btn:hover { background: var(--accent-hover); }
  .scan-btn.busy, .scan-btn:disabled { opacity: 0.7; cursor: default; }

  .cmp-top { display: flex; gap: 16px; align-items: stretch; margin-bottom: 16px; flex-wrap: wrap; }
  .score { display: flex; align-items: center; justify-content: center; padding: 6px; }
  .score-ring {
    --pct: 0; width: 128px; height: 128px; border-radius: var(--r-pill);
    background: conic-gradient(var(--ring) calc(var(--pct) * 1%), var(--surface-3) 0);
    display: flex; align-items: center; justify-content: center;
    transition: background var(--dur-slow) var(--ease-out);
  }
  .score.good { --ring: var(--success); } .score.mid { --ring: var(--warning); } .score.bad { --ring: var(--danger); }
  .score-inner { width: 104px; height: 104px; border-radius: var(--r-pill); background: var(--surface-1); display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 2px; }
  /* v1.7.235 "instrumento premium" — score y conteos en cifras tabulares mono,
     labels de instrumento: el panel se lee como medidor, no como tarjeta. */
  .score-n { font-family: var(--font-mono); font-variant-numeric: tabular-nums; font-size: 32px; font-weight: var(--fw-medium); color: var(--text-primary); line-height: 1; }
  .score-pct { font-family: var(--font-mono); font-size: 14px; color: var(--text-muted); }
  .score-l { font-family: var(--font-mono); font-size: var(--fs-micro); letter-spacing: var(--ls-label); text-transform: uppercase; color: var(--text-faint); }

  .summary { flex: 1; min-width: 220px; display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; }
  .scard { background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-lg); padding: 16px; display: flex; flex-direction: column; justify-content: center; border-left-width: 3px; box-shadow: var(--inset-shadow); }
  .scard-n { font-family: var(--font-mono); font-variant-numeric: tabular-nums; font-size: 28px; font-weight: var(--fw-medium); line-height: 1; }
  .scard-l { font-family: var(--font-mono); font-size: var(--fs-micro); letter-spacing: var(--ls-label); text-transform: uppercase; color: var(--text-faint); margin-top: 8px; }
  .scard.pass { border-left-color: var(--success); } .scard.pass .scard-n { color: var(--success); }
  .scard.warn { border-left-color: var(--warning); } .scard.warn .scard-n { color: var(--warning); }
  .scard.fail { border-left-color: var(--danger); }  .scard.fail .scard-n { color: var(--danger); }

  .cmp-note { font-size: var(--fs-caption); color: var(--text-faint); margin-bottom: 10px; }

  .filters { display: flex; gap: 6px; margin-bottom: 12px; }
  .fchip { display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--text-muted); background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-sm); padding: 5px 10px; cursor: pointer; transition: color var(--dur-fast) var(--ease-out); }
  .fchip:hover { color: var(--text-primary); }
  .fchip.on { color: var(--accent); background: var(--accent-bg); border-color: var(--border-accent); }
  .fchip-n { font-size: var(--fs-caption); color: var(--text-faint); background: var(--surface-3); padding: 0 6px; border-radius: var(--r-pill); }
  .fchip.on .fchip-n { color: var(--accent); background: transparent; }

  .checks { display: flex; flex-direction: column; gap: 8px; }
  .check { background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-lg); border-left-width: 3px; overflow: hidden; }
  .check.open { border-color: var(--border-accent); }
  .check.pass { border-left-color: var(--success); } .check.pass .chk-icon { color: var(--success); }
  .check.warn { border-left-color: var(--warning); } .check.warn .chk-icon { color: var(--warning); }
  .check.fail { border-left-color: var(--danger); }  .check.fail .chk-icon { color: var(--danger); }
  .chk-head { display: flex; align-items: center; gap: 12px; width: 100%; padding: 12px 15px; background: transparent; border: 0; cursor: pointer; text-align: left; }
  .chk-head:hover { background: var(--surface-3); }
  .chk-chev { display: flex; color: var(--text-faint); flex-shrink: 0; transition: transform var(--dur-fast) var(--ease-out); }
  .check.open .chk-chev { transform: rotate(180deg); }
  .chk-icon { flex-shrink: 0; display: flex; }
  .chk-main { flex: 1; min-width: 0; }
  .chk-name { font-size: var(--fs-footnote); color: var(--text-primary); margin-bottom: 5px; }
  .chk-meta { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .chk-id { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); }
  .chk-cat { font-size: var(--fs-caption); color: var(--text-muted); background: var(--surface-3); padding: 1px 7px; border-radius: 5px; }
  .chk-fix { font-size: var(--fs-caption); color: var(--text-muted); font-family: var(--font-mono); }
  .chk-sev { font-size: var(--fs-caption); padding: 2px 9px; border-radius: var(--r-pill); }
  .sev-critical { color: var(--danger);  background: var(--danger-bg); }
  .sev-high     { color: var(--warning); background: var(--warning-bg); }
  .sev-medium   { color: var(--text-secondary); background: var(--surface-3); }
  .sev-low      { color: var(--text-muted); background: var(--surface-3); }
  .chk-st { font-size: var(--fs-footnote); color: var(--text-secondary); min-width: 46px; text-align: right; }
  .chk-empty { padding: 20px; text-align: center; font-size: var(--fs-footnote); color: var(--text-faint); }

  /* Drill-down detail (why a control passed/failed + how to fix). */
  .chk-detail { padding: 11px 15px 13px; border-top: 1px solid var(--border); background: rgba(0,0,0,0.14); }
  .cd-fix { display: flex; align-items: flex-start; gap: 7px; font-size: var(--fs-caption); color: var(--text-secondary); line-height: var(--lh-body); margin-bottom: 10px; }
  .cd-fix :global(svg) { color: var(--success); flex-shrink: 0; margin-top: 1px; }
  .cd-fix-txt { flex: 1; }
  .cd-label { font-size: var(--fs-caption); color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.03em; margin: 8px 0 4px; }
  .cd-label.dim { text-transform: none; color: var(--text-faint); }
  .cd-code { display: flex; align-items: center; gap: 8px; background: rgba(0,0,0,0.28); border: 1px solid var(--border); border-radius: var(--r-sm); padding: 6px 10px; }
  .cd-code code { flex: 1; font-family: var(--font-mono); font-size: 11px; color: var(--text-secondary); overflow-wrap: anywhere; }
  .cd-out { margin: 0; max-height: 180px; overflow: auto; background: rgba(0,0,0,0.28); border: 1px solid var(--border); border-radius: var(--r-sm); padding: 7px 10px; font-family: var(--font-mono); font-size: 11px; line-height: 1.5; color: var(--text-muted); white-space: pre-wrap; overflow-wrap: anywhere; }
  .cd-copy { display: flex; align-items: center; justify-content: center; width: 22px; height: 22px; flex-shrink: 0; padding: 0; border: 0; border-radius: 5px; background: transparent; color: var(--text-muted); cursor: pointer; transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out); }
  .cd-copy:hover { background: var(--surface-3); color: var(--text-primary); }

  .cmp-tool { display: flex; align-items: center; justify-content: center; width: 30px; height: 30px; padding: 0; border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-2); color: var(--text-muted); cursor: pointer; transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out); }
  .cmp-tool:hover { background: var(--surface-3); color: var(--text-primary); }

  .cmp-loading { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; color: var(--text-faint); text-align: center; padding: 24px; }
  .cmp-loading p { margin: 0; font-size: var(--fs-footnote); color: var(--text-muted); max-width: 340px; }
</style>
