<script>
  /* ==========================================================================
     Lucy 2.0 — Agent workspace panel  ·  Phase F3 (UI 2.0, direction C)
     The cockpit's right lane. Renders Plan / Ejecución / Trace / Artefactos
     PURELY from the agent-workspace store — no local agent logic. Fed by the
     demo driver today, by the +page.svelte bridge at integration time.

     v1.7.232 enrichment — each panel gained depth + a transversal layer:
       · timestamps on every row (stamped in the store's push fns)
       · entrance animation for new rows + auto-scroll-to-newest (stick)
       · export-run (→ clipboard) + clear-workspace toolbar
       · Plan: total-elapsed header + live "working…" row + error states
       · Ejecución: real ✓/✕ + exit code, collapsible output, copy output,
         "solo errores" filter
       · Trace: phase icons + timeline rail + phase filter chips
       · Artefactos: +N/−M diff stats + line-number gutter + type filter
     ========================================================================== */
  import { agentPlan, agentExec, agentTrace, agentArtifacts, agentStatus, resetWorkspace } from './agent-workspace';
  import CircleCheck from '@tabler/icons-svelte/icons/circle-check';
  import Circle from '@tabler/icons-svelte/icons/circle';
  import AlertCircle from '@tabler/icons-svelte/icons/alert-circle';
  import Terminal2 from '@tabler/icons-svelte/icons/terminal-2';
  import ListDetails from '@tabler/icons-svelte/icons/list-details';
  import Brain from '@tabler/icons-svelte/icons/brain';
  import Bolt from '@tabler/icons-svelte/icons/bolt';
  import Eye from '@tabler/icons-svelte/icons/eye';
  import InfoCircle from '@tabler/icons-svelte/icons/info-circle';
  import FileDiff from '@tabler/icons-svelte/icons/file-diff';
  import Sparkles from '@tabler/icons-svelte/icons/sparkles';
  import Copy from '@tabler/icons-svelte/icons/copy';
  import Check from '@tabler/icons-svelte/icons/check';
  import ExternalLink from '@tabler/icons-svelte/icons/external-link';
  import ChevronDown from '@tabler/icons-svelte/icons/chevron-down';
  import ClipboardText from '@tabler/icons-svelte/icons/clipboard-text';
  import Trash from '@tabler/icons-svelte/icons/trash';
  import { invoke } from '@tauri-apps/api/core';
  import { copyToClipboard } from '$lib/lucy-api';
  import { diffLines } from '$lib/diff-util';

  let activeTab = $state('plan');

  // ── Live clock — ticks each second while the agent runs (drives the plan's
  //    total-elapsed + the "working…" timer). Auto-stops when idle / unmounted. ──
  let _now = $state(Date.now());
  $effect(() => {
    if (!$agentStatus.running) return;
    const iv = setInterval(() => { _now = Date.now(); }, 1000);
    return () => clearInterval(iv);
  });

  const pad2 = (n) => String(n).padStart(2, '0');
  const clock = (ts) => { if (!ts) return ''; const d = new Date(ts); return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`; };
  const secs = (ms) => (ms >= 1000 ? (ms / 1000).toFixed(1) : (ms / 1000).toFixed(2)) + 's';
  const durLong = (ms) => { const s = Math.max(0, Math.floor(ms / 1000)); if (s < 60) return s + 's'; const m = Math.floor(s / 60); return `${m}m ${pad2(s % 60)}s`; };

  // Copy helper with a brief ✓ (keyed by an arbitrary id so cmd / output / path
  // buttons don't clash on the shared `copiedId`).
  let copiedId = $state(null);
  let _copyTimer = null;
  function _flashCopied(id) { copiedId = id; if (_copyTimer) clearTimeout(_copyTimer); _copyTimer = setTimeout(() => { copiedId = null; }, 1200); }
  async function _copy(text, id) { try { await copyToClipboard(text); } catch { try { navigator.clipboard?.writeText(text); } catch {} } _flashCopied(id); }

  // ── Plan — timeline / total elapsed / live "working" row ──────────────────
  const planStart = $derived($agentPlan.length ? ($agentPlan[0].ts ?? null) : null);
  const planEnd = $derived.by(() => { const l = $agentPlan[$agentPlan.length - 1]; return l ? (l.ts ?? 0) + (l.ms ?? 0) : null; });
  const planElapsed = $derived.by(() => { if (planStart == null) return 0; return ($agentStatus.running ? _now : (planEnd ?? _now)) - planStart; });
  const workingSecs = $derived.by(() => { if (!$agentStatus.running || planEnd == null) return 0; return Math.max(0, Math.floor((_now - planEnd) / 1000)); });
  const planErrors = $derived($agentPlan.filter((s) => s.status === 'error').length);

  // ── Ejecución — errors-only filter + expand-long-output + copy output ─────
  let execErrOnly = $state(false);
  let _openOut = $state({});     // exec id -> show full output
  const OUT_LINES = 14;
  const execView = $derived(execErrOnly ? $agentExec.filter((e) => !e.ok) : $agentExec);
  const execErrCount = $derived($agentExec.filter((e) => !e.ok).length);
  const outMeta = (out) => { const lines = String(out ?? '').split('\n'); return { total: lines.length, clamped: lines.length > OUT_LINES, head: lines.slice(0, OUT_LINES).join('\n') }; };

  // ── Trace — phase icon/color + filter chips + rail ────────────────────────
  const basePhase = (p) => (p || '').split('.')[0];
  const phaseIcon = (p) => { const b = basePhase(p); return b === 'think' ? Brain : b === 'react' ? Eye : b === 'info' ? InfoCircle : Bolt; };
  const phaseColor = (p) => { const b = basePhase(p); return b === 'react' ? 'var(--warning)' : b === 'think' ? 'var(--user)' : b === 'info' ? 'var(--text-muted)' : 'var(--accent)'; };
  let traceFilter = $state(null);
  const tracePhases = $derived([...new Set($agentTrace.map((t) => basePhase(t.phase)))]);
  const traceView = $derived(traceFilter ? $agentTrace.filter((t) => basePhase(t.phase) === traceFilter) : $agentTrace);

  // ── Artefactos — diff stats, line numbers, type filter, expand/copy/reveal ─
  let expandedArtId = $state(null);
  let revealErrId = $state(null);
  let artFilter = $state(null);
  const artKinds = $derived([...new Set($agentArtifacts.map((a) => a.kind))]);
  const artView = $derived(artFilter ? $agentArtifacts.filter((a) => a.kind === artFilter) : $agentArtifacts);
  function toggleDiff(id) { expandedArtId = expandedArtId === id ? null : id; }
  async function revealFile(a) {
    try { await invoke('reveal_in_explorer', { path: a.path }); revealErrId = null; }
    catch { revealErrId = a.id; setTimeout(() => { if (revealErrId === a.id) revealErrId = null; }, 2500); }
  }
  function diffStat(a) {
    if (a.before == null || a.after == null) return null;
    const d = diffLines(a.before, a.after);
    let add = 0, rem = 0;
    for (const x of d) { if (x.type === 'add') add++; else if (x.type === 'rem') rem++; }
    return { add, rem };
  }
  const artifactIcon = (k) => (k === 'memory' ? Brain : k === 'skill' ? Sparkles : FileDiff);

  const tabDefs = [
    { id: 'plan', label: 'Plan' },
    { id: 'exec', label: 'Ejecución' },
    { id: 'trace', label: 'Trace' },
    { id: 'artifacts', label: 'Artefactos' },
  ];
  const emptyMsg = {
    plan:      { title: 'Sin plan todavía',   hint: 'Lucy desglosa la tarea en pasos y los va marcando conforme avanza.' },
    exec:      { title: 'Nada ejecutado aún', hint: 'La salida de cada comando aparece aquí en vivo mientras el agente trabaja.' },
    trace:     { title: 'Trace vacío',        hint: 'El razonamiento del agente — pensar · actuar · observar — se registra aquí.' },
    artifacts: { title: 'Sin artefactos',     hint: 'Los archivos que Lucy edita o escribe aparecen aquí con su diff.' },
  };

  const counts = $derived({
    plan: $agentPlan.length, exec: $agentExec.length,
    trace: $agentTrace.length, artifacts: $agentArtifacts.length,
  });
  const totalItems = $derived(counts.plan + counts.exec + counts.trace + counts.artifacts);

  // ── Auto-scroll: stick to the newest row unless the user scrolled up ───────
  let bodyEl = $state(null);
  let _stick = true;
  function onBodyScroll() { if (bodyEl) _stick = bodyEl.scrollHeight - bodyEl.scrollTop - bodyEl.clientHeight < 48; }
  $effect(() => {
    const _dep = totalItems + $agentExec.length; void _dep; void activeTab;   // re-run on new rows / tab switch
    if (_stick && bodyEl) requestAnimationFrame(() => { if (bodyEl && _stick) bodyEl.scrollTop = bodyEl.scrollHeight; });
  });

  // ── Export the run as a plain-text report (→ clipboard) ───────────────────
  function exportReport() {
    const L = ['# Lucy — Reporte del run', ''];
    if ($agentStatus.model) L.push(`Modelo: ${$agentStatus.model}`);
    L.push(`Pasos: ${counts.plan}  ·  Ejecuciones: ${counts.exec}  ·  Artefactos: ${counts.artifacts}`, '');
    if ($agentPlan.length) {
      L.push('## Plan');
      $agentPlan.forEach((s, i) => L.push(`${i + 1}. [${s.status}] ${s.label}${s.ms ? ` (${secs(s.ms)})` : ''}`));
      L.push('');
    }
    if ($agentExec.length) {
      L.push('## Ejecución');
      $agentExec.forEach((e) => {
        L.push(`$ ${e.cmd}${e.code != null && e.code !== 0 ? `   [exit ${e.code}]` : ''}`);
        L.push(String(e.output ?? '').split('\n').map((x) => '  ' + x).join('\n'), '');
      });
    }
    if ($agentArtifacts.length) {
      L.push('## Artefactos');
      $agentArtifacts.forEach((a) => L.push(`- [${a.kind}] ${a.path}${a.summary ? ` — ${a.summary}` : ''}`));
      L.push('');
    }
    _copy(L.join('\n'), 'export');
  }
</script>

<div class="agent-ws">
  <div class="ws-tabs" role="tablist" aria-label="Workspace del agente">
    {#each tabDefs as t}
      <button
        class="ws-tab" class:active={activeTab === t.id}
        role="tab" aria-selected={activeTab === t.id}
        onclick={() => (activeTab = t.id)}
      >
        {t.label}
        {#if counts[t.id] > 0}<span class="tab-count">{counts[t.id]}</span>{/if}
      </button>
    {/each}
    {#if totalItems > 0}
      <div class="ws-tools">
        <button class="ws-tool" title="Exportar el run (copia al portapapeles)" aria-label="Exportar el run" onclick={exportReport}>
          {#if copiedId === 'export'}<Check size={14} stroke={2} />{:else}<ClipboardText size={14} stroke={1.75} />{/if}
        </button>
        <button class="ws-tool" title="Limpiar el workspace" aria-label="Limpiar el workspace" onclick={() => resetWorkspace()}><Trash size={14} stroke={1.75} /></button>
      </div>
    {/if}
  </div>

  {#if $agentStatus.running || $agentStatus.stepTotal > 0}
    <div class="ws-progress">
      <span class="ck-led" class:on={$agentStatus.running} aria-hidden="true"></span>
      {#if $agentStatus.stepTotal > 0}
        <span class="prog-label">Paso {$agentStatus.stepIndex} de {$agentStatus.stepTotal}</span>
        <div class="bar"><div class="fill" style="width:{Math.round(($agentStatus.stepIndex / $agentStatus.stepTotal) * 100)}%"></div></div>
      {:else}
        <span class="prog-label">Paso {$agentStatus.stepIndex || 1}</span>
        <div class="bar"><div class="fill indet"></div></div>
      {/if}
      {#if $agentStatus.running}
        <span class="pill run">en curso</span>
      {:else}
        <span class="pill done">listo</span>
      {/if}
    </div>
  {/if}

  <div class="ws-body" bind:this={bodyEl} onscroll={onBodyScroll}>

    {#if activeTab === 'plan'}
      {#if $agentPlan.length === 0}
        <div class="empty ck-blueprint"><span class="empty-tile"><ListDetails size={28} stroke={1.4} /></span><div class="empty-title">{emptyMsg.plan.title}</div><p>{emptyMsg.plan.hint}</p></div>
      {:else}
        <div class="plan-head">
          <span class="ph-count">{$agentPlan.length} {$agentPlan.length === 1 ? 'paso' : 'pasos'}</span>
          <span class="ph-time">⏱ {durLong(planElapsed)}</span>
          {#if planErrors > 0}<span class="ph-err">{planErrors} con error</span>{/if}
          {#if $agentStatus.running}<span class="ph-run">en curso</span>{/if}
        </div>
        <div class="plan">
          {#each $agentPlan as step (step.id)}
            <div class="step" class:running={step.status === 'running'} class:is-error={step.status === 'error'}>
              <span class="step-icon">
                {#if step.status === 'done'}
                  <CircleCheck size={19} stroke={1.75} color="var(--accent)" />
                {:else if step.status === 'error'}
                  <AlertCircle size={19} stroke={1.75} color="var(--danger)" />
                {:else if step.status === 'running'}
                  <span class="ring" aria-label="ejecutando"><span class="ring-dot"></span></span>
                {:else}
                  <Circle size={19} stroke={1.75} color="var(--text-disabled)" />
                {/if}
              </span>
              <div class="step-main">
                <div class="step-label" class:muted={step.status === 'pending'}>{step.label}</div>
                {#if step.detail || step.host || step.ms}
                  <div class="step-meta">
                    {step.detail ?? ''}{#if step.host} · {step.host}{/if}{#if step.ms} · {secs(step.ms)}{/if}
                  </div>
                {/if}
              </div>
              {#if step.ts}<span class="step-ts">{clock(step.ts)}</span>{/if}
            </div>
          {/each}
          {#if $agentStatus.running && !$agentPlan.some((s) => s.status === 'running' || s.status === 'pending')}
            <div class="step working">
              <span class="step-icon"><span class="ring"><span class="ring-dot"></span></span></span>
              <div class="step-main">
                <div class="step-label">Trabajando en el siguiente paso…</div>
                <div class="step-meta">{workingSecs}s</div>
              </div>
            </div>
          {/if}
        </div>
      {/if}

    {:else if activeTab === 'exec'}
      {#if $agentExec.length === 0}
        <div class="empty ck-blueprint"><span class="empty-tile"><Terminal2 size={28} stroke={1.4} /></span><div class="empty-title">{emptyMsg.exec.title}</div><p>{emptyMsg.exec.hint}</p></div>
      {:else}
        {#if execErrCount > 0}
          <div class="ws-filter">
            <button class="fchip" class:on={!execErrOnly} onclick={() => (execErrOnly = false)}>Todo ({$agentExec.length})</button>
            <button class="fchip danger" class:on={execErrOnly} onclick={() => (execErrOnly = true)}>Solo errores ({execErrCount})</button>
          </div>
        {/if}
        <div class="exec-list">
          {#each execView as e (e.id)}
            {@const om = outMeta(e.output)}
            {@const open = !!_openOut[e.id]}
            <div class="exec" class:err={!e.ok}>
              <div class="exec-head">
                <span class="exec-st" class:bad={!e.ok}>
                  {#if e.ok}<Check size={13} stroke={2.4} />{:else}<AlertCircle size={13} stroke={2} />{/if}
                </span>
                <Terminal2 size={14} stroke={1.75} />
                <span class="exec-cmd">{e.cmd}</span>
                {#if e.code != null && e.code !== 0}<span class="exec-code">exit {e.code}</span>{/if}
                <span class="exec-meta">{e.engine ?? ''}{#if e.ms} · {secs(e.ms)}{/if}{#if e.ts} · {clock(e.ts)}{/if}</span>
                <button class="exec-copy" class:ok={copiedId === 'c' + e.id} onclick={() => _copy(e.cmd, 'c' + e.id)} aria-label="Copiar comando" title="Copiar comando">
                  {#if copiedId === 'c' + e.id}<Check size={14} stroke={2} />{:else}<Copy size={14} stroke={1.75} />{/if}
                </button>
                <button class="exec-copy" class:ok={copiedId === 'o' + e.id} onclick={() => _copy(e.output, 'o' + e.id)} aria-label="Copiar salida" title="Copiar salida">
                  {#if copiedId === 'o' + e.id}<Check size={14} stroke={2} />{:else}<ClipboardText size={14} stroke={1.75} />{/if}
                </button>
              </div>
              <pre class="exec-out" class:err={!e.ok}>{open || !om.clamped ? e.output : om.head}</pre>
              {#if om.clamped}
                <button class="out-more" onclick={() => (_openOut = { ..._openOut, [e.id]: !open })}>
                  {open ? 'Ver menos' : `Ver ${om.total - OUT_LINES} líneas más`}
                  <span class="chev" class:up={open}><ChevronDown size={13} stroke={1.75} /></span>
                </button>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

    {:else if activeTab === 'trace'}
      {#if $agentTrace.length === 0}
        <div class="empty ck-blueprint"><span class="empty-tile"><ListDetails size={28} stroke={1.4} /></span><div class="empty-title">{emptyMsg.trace.title}</div><p>{emptyMsg.trace.hint}</p></div>
      {:else}
        {#if tracePhases.length > 1}
          <div class="ws-filter">
            <button class="fchip" class:on={traceFilter === null} onclick={() => (traceFilter = null)}>Todo</button>
            {#each tracePhases as p}
              <button class="fchip" class:on={traceFilter === p} onclick={() => (traceFilter = p)}>{p}</button>
            {/each}
          </div>
        {/if}
        <div class="trace">
          {#each traceView as tr (tr.id)}
            {@const PIcon = phaseIcon(tr.phase)}
            <div class="trace-row">
              <span class="trace-ic" style="color:{phaseColor(tr.phase)}"><PIcon size={13} stroke={1.9} /></span>
              <div class="trace-main">
                <div class="trace-label">{tr.label}{#if tr.step}<span class="trace-step"> · paso {tr.step}</span>{/if}</div>
                {#if tr.detail}<div class="trace-detail">{tr.detail}</div>{/if}
              </div>
              <span class="trace-side">
                {#if tr.ts}<span class="trace-ts">{clock(tr.ts)}</span>{/if}
                <span class="trace-phase">{tr.phase}</span>
              </span>
            </div>
          {/each}
        </div>
      {/if}

    {:else}
      {#if $agentArtifacts.length === 0}
        <div class="empty ck-blueprint"><span class="empty-tile"><FileDiff size={28} stroke={1.4} /></span><div class="empty-title">{emptyMsg.artifacts.title}</div><p>{emptyMsg.artifacts.hint}</p></div>
      {:else}
        {#if artKinds.length > 1}
          <div class="ws-filter">
            <button class="fchip" class:on={artFilter === null} onclick={() => (artFilter = null)}>Todo</button>
            {#each artKinds as k}
              <button class="fchip" class:on={artFilter === k} onclick={() => (artFilter = k)}>{k}</button>
            {/each}
          </div>
        {/if}
        <div class="art-list">
          {#each artView as a (a.id)}
            {@const Icon = artifactIcon(a.kind)}
            {@const hasBody = a.after != null || a.before != null}
            {@const stat = diffStat(a)}
            <div class="artifact" class:open={expandedArtId === a.id}>
              <div class="art-head">
                <span class="art-icon"><Icon size={17} stroke={1.75} /></span>
                <div class="art-main">
                  <div class="art-path" title={a.path}>{a.path}</div>
                  {#if a.summary}<div class="art-sum">{a.summary}</div>{/if}
                  {#if a.ts}<div class="art-time">{clock(a.ts)}</div>{/if}
                </div>
                <div class="art-acts">
                  {#if stat && (stat.add || stat.rem)}
                    <span class="art-stat">{#if stat.add}<span class="st-add">+{stat.add}</span>{/if}{#if stat.rem}<span class="st-rem">−{stat.rem}</span>{/if}</span>
                  {/if}
                  {#if hasBody}
                    <button class="art-btn" class:on={expandedArtId === a.id} title="Ver diff" aria-label="Ver diff" onclick={() => toggleDiff(a.id)}><FileDiff size={14} stroke={1.75} /></button>
                  {/if}
                  <button class="art-btn" title="Copiar ruta" aria-label="Copiar ruta" onclick={() => _copy(a.path, a.id)}>
                    {#if copiedId === a.id}<Check size={14} stroke={2} />{:else}<Copy size={14} stroke={1.75} />{/if}
                  </button>
                  <button class="art-btn" class:err={revealErrId === a.id} title={revealErrId === a.id ? 'No se pudo abrir (¿movido?)' : 'Mostrar en el Explorador'} aria-label="Mostrar en el Explorador" onclick={() => revealFile(a)}><ExternalLink size={14} stroke={1.75} /></button>
                  <span class="art-kind">{a.kind}</span>
                </div>
              </div>
              {#if expandedArtId === a.id}
                {@const _diff = (a.before != null && a.after != null) ? diffLines(a.before, a.after) : null}
                <div class="art-diff">
                  {#if _diff}
                    {#if _diff.length === 0}
                      <div class="dl-none">Sin cambios de contenido.</div>
                    {:else}
                      {#each _diff as d}
                        <div class="dl {d.type}"><span class="dl-n">{d.n ?? ''}</span><span class="dl-g">{d.type === 'add' ? '+' : d.type === 'rem' ? '−' : ''}</span><span class="dl-t">{d.text || ' '}</span></div>
                      {/each}
                    {/if}
                  {:else if a.after != null}
                    {#each a.after.split('\n') as line, i}
                      <div class="dl add"><span class="dl-n">{i + 1}</span><span class="dl-g">+</span><span class="dl-t">{line || ' '}</span></div>
                    {/each}
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    {/if}

  </div>
</div>

<style>
  .agent-ws { display: flex; flex-direction: column; height: 100%; min-height: 0; }

  .ws-tabs { display: flex; gap: 2px; padding: 10px 12px 0; border-bottom: 1px solid var(--border); }
  .ws-tab {
    display: flex; align-items: center; gap: 6px;
    font-size: var(--fs-footnote); color: var(--text-muted);
    background: transparent; border: 0; cursor: pointer;
    padding: 6px 10px; border-bottom: 2px solid transparent;
    transition: color var(--dur-fast) var(--ease-out);
  }
  .ws-tab:hover { color: var(--text-primary); }
  .ws-tab.active { color: var(--text-primary); border-bottom-color: var(--accent); }
  .tab-count {
    font-size: var(--fs-caption); color: var(--text-muted);
    background: var(--surface-2); padding: 0 6px; border-radius: var(--r-pill); min-width: 16px; text-align: center;
  }
  .ws-tab.active .tab-count { color: var(--accent); background: var(--accent-bg); }
  .ws-tools { margin-left: auto; display: flex; gap: 2px; align-self: center; padding-bottom: 4px; }
  .ws-tool {
    display: flex; align-items: center; justify-content: center; width: 26px; height: 26px;
    border: 0; border-radius: var(--r-sm); background: transparent; color: var(--text-faint); cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out);
  }
  .ws-tool:hover { background: var(--surface-3); color: var(--text-primary); }

  .ws-progress { display: flex; align-items: center; gap: 10px; padding: 9px 14px; border-bottom: 1px solid var(--border); }
  .prog-label { font-size: var(--fs-caption); color: var(--text-muted); white-space: nowrap; }
  .bar { flex: 1; height: 4px; background: var(--surface-3); border-radius: var(--r-pill); overflow: hidden; }
  .fill { height: 100%; background: var(--accent); border-radius: var(--r-pill); transition: width var(--dur-slow) var(--ease-out); }
  .fill.indet { width: 40%; transition: none; animation: ws-indet 1.3s var(--ease-out) infinite; }
  @keyframes ws-indet { 0% { margin-left: -40%; } 100% { margin-left: 100%; } }
  .pill { font-size: var(--fs-caption); padding: 2px 8px; border-radius: 6px; }
  .pill.run { color: var(--accent); background: var(--accent-bg); }
  .pill.done { color: var(--text-muted); background: var(--surface-2); }

  .ws-body { flex: 1; overflow-y: auto; scroll-behavior: smooth; }

  .empty {
    margin: auto; max-width: 240px; height: 100%;
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px;
    color: var(--text-faint); text-align: center; padding: 24px;
  }
  /* v1.7.235 "instrumento premium" — el icono del empty vive en un tile de
     instrumento apagado (no en el círculo genérico de plantilla); la textura
     blueprint del contenedor (.ck-blueprint) hace que la zona idle se lea como
     un instrumento en espera, no como un hueco. */
  .empty-tile {
    display: flex; align-items: center; justify-content: center;
    width: 52px; height: 52px; margin-bottom: 2px;
    background: var(--surface-1); border: 1px solid var(--border-strong);
    border-radius: var(--r-md); color: var(--text-faint);
  }
  .empty-title { font-size: var(--fs-footnote); font-weight: var(--fw-medium); color: var(--text-secondary); }
  .empty p { margin: 0; font-size: var(--fs-caption); line-height: var(--lh-body); color: var(--text-muted); }

  /* ── filter chips (exec / trace / artifacts) ─────────────────────────────── */
  .ws-filter { display: flex; flex-wrap: wrap; gap: 6px; padding: 11px 12px 2px; }
  .fchip {
    font-size: var(--fs-caption); color: var(--text-muted); cursor: pointer;
    background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-pill);
    padding: 3px 10px; transition: color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out);
  }
  .fchip:hover { color: var(--text-primary); }
  .fchip.on { color: var(--accent-ink); background: var(--accent); border-color: var(--accent); }
  .fchip.danger.on { color: #fff; background: var(--danger); border-color: var(--danger); }

  /* ── Plan ────────────────────────────────────────────────────────────────── */
  .plan-head {
    display: flex; align-items: center; gap: 9px; flex-wrap: wrap;
    padding: 10px 14px 4px; font-size: var(--fs-caption); color: var(--text-muted);
  }
  .ph-count { color: var(--text-secondary); font-weight: var(--fw-medium); }
  .ph-time { font-family: var(--font-mono); color: var(--text-muted); }
  .ph-err { color: var(--danger); background: rgba(240,110,110,0.12); padding: 1px 7px; border-radius: var(--r-pill); }
  .ph-run { color: var(--accent); background: var(--accent-bg); padding: 1px 7px; border-radius: var(--r-pill); margin-left: auto; }
  .plan { padding: 6px 12px 12px; display: flex; flex-direction: column; gap: 3px; }
  .step { display: flex; gap: 11px; align-items: flex-start; padding: 9px 10px; border-radius: var(--r-md); animation: ws-in var(--dur-base) var(--ease-out); }
  .step.running { border: 1px solid var(--border-accent); background: var(--surface-inset); }
  .step.is-error { background: rgba(240,110,110,0.06); }
  .step.working { opacity: 0.9; }
  .step-icon { flex-shrink: 0; height: 19px; display: flex; align-items: center; }
  .step-main { min-width: 0; flex: 1; }
  .step-label { font-size: var(--fs-footnote); color: var(--text-secondary); }
  .step-label.muted { color: var(--text-muted); }
  .step-meta { font-size: var(--fs-caption); color: var(--text-faint); margin-top: 2px; font-family: var(--font-mono); }
  .step-ts { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); flex-shrink: 0; align-self: center; }

  .ring { width: 18px; height: 18px; border-radius: var(--r-pill); border: 2px solid var(--accent); display: inline-flex; align-items: center; justify-content: center; }
  .ring-dot { width: 6px; height: 6px; border-radius: var(--r-pill); background: var(--accent); animation: pulse 1.2s var(--ease-out) infinite; }
  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.25; } }

  .exec-list, .art-list { padding: 12px; display: flex; flex-direction: column; gap: 10px; }

  /* ── Ejecución ─────────────────────────────────────────────────────────────── */
  .exec { border: 1px solid var(--border); border-radius: var(--r-lg); overflow: hidden; background: var(--surface-inset); animation: ws-in var(--dur-base) var(--ease-out); }
  .exec.err { border-color: rgba(240,110,110,0.4); }
  .exec-head { display: flex; align-items: center; gap: 8px; padding: 7px 11px; border-bottom: 1px solid var(--border); background: var(--surface-1); color: var(--text-muted); }
  .exec-st { display: flex; align-items: center; color: var(--accent); flex-shrink: 0; }
  .exec-st.bad { color: var(--danger); }
  .exec-cmd { flex: 1; min-width: 0; font-family: var(--font-mono); font-size: var(--fs-caption); color: var(--text-secondary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .exec-code { font-size: var(--fs-caption); color: var(--danger); background: rgba(240,110,110,0.12); padding: 1px 6px; border-radius: 5px; white-space: nowrap; flex-shrink: 0; }
  .exec-meta { font-size: var(--fs-caption); color: var(--text-faint); white-space: nowrap; font-family: var(--font-mono); }
  .exec-copy { display: flex; align-items: center; justify-content: center; padding: 3px; border: 0; background: transparent; color: var(--text-faint); cursor: pointer; border-radius: var(--r-sm); transition: color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out); }
  .exec-copy:hover { color: var(--text-primary); background: var(--surface-3); }
  .exec-copy.ok { color: var(--accent); }
  .exec-out { margin: 0; padding: 10px 12px; font-family: var(--font-mono); font-size: var(--fs-code); line-height: 1.7; color: var(--text-secondary); white-space: pre-wrap; word-break: break-word; }
  .exec-out.err { color: var(--danger); }
  .out-more {
    width: 100%; display: flex; align-items: center; justify-content: center; gap: 5px;
    padding: 6px; border: 0; border-top: 1px solid var(--border); background: var(--surface-1);
    color: var(--text-muted); font-size: var(--fs-caption); cursor: pointer;
    transition: color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out);
  }
  .out-more:hover { color: var(--text-primary); background: var(--surface-2); }
  .chev { display: inline-flex; transition: transform var(--dur-fast) var(--ease-out); }
  .chev.up { transform: rotate(180deg); }

  /* ── Trace (timeline rail) ─────────────────────────────────────────────────── */
  .trace { position: relative; padding: 12px 12px 12px 12px; display: flex; flex-direction: column; }
  .trace::before { content: ''; position: absolute; left: 22px; top: 20px; bottom: 20px; width: 2px; background: var(--border); }
  .trace-row { position: relative; display: flex; gap: 10px; align-items: flex-start; padding: 6px 0; animation: ws-in var(--dur-base) var(--ease-out); }
  .trace-ic { position: relative; z-index: 1; width: 21px; height: 21px; border-radius: var(--r-pill); display: flex; align-items: center; justify-content: center; background: var(--surface-1); border: 1px solid var(--border); flex-shrink: 0; }
  .trace-main { flex: 1; min-width: 0; padding-top: 1px; }
  .trace-label { font-size: var(--fs-footnote); color: var(--text-secondary); }
  .trace-step { color: var(--text-faint); font-family: var(--font-mono); }
  .trace-detail { font-size: var(--fs-caption); color: var(--text-muted); margin-top: 2px; }
  .trace-side { display: flex; flex-direction: column; align-items: flex-end; gap: 1px; flex-shrink: 0; }
  .trace-ts { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); }
  .trace-phase { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); }

  /* ── Artefactos ────────────────────────────────────────────────────────────── */
  .artifact { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); overflow: hidden; animation: ws-in var(--dur-base) var(--ease-out); }
  .artifact.open { border-color: var(--border-accent); }
  .art-head { display: flex; gap: 11px; align-items: flex-start; padding: 10px 11px; }
  .art-icon { color: var(--accent); flex-shrink: 0; height: 17px; display: flex; align-items: center; }
  .art-main { flex: 1; min-width: 0; }
  .art-path { font-size: var(--fs-footnote); color: var(--text-primary); overflow-wrap: anywhere; }
  .art-sum { font-size: var(--fs-caption); color: var(--text-muted); margin-top: 3px; line-height: var(--lh-body); }
  .art-time { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); margin-top: 3px; }
  .art-acts { display: flex; align-items: center; gap: 3px; flex-shrink: 0; }
  .art-stat { display: inline-flex; gap: 5px; font-family: var(--font-mono); font-size: var(--fs-caption); margin-right: 2px; }
  .st-add { color: var(--success, #3dd6a4); }
  .st-rem { color: var(--danger, #f06e6e); }
  .art-btn {
    display: flex; align-items: center; justify-content: center; width: 24px; height: 24px;
    padding: 0; border: 0; border-radius: 6px; background: transparent; color: var(--text-muted); cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out);
  }
  .art-btn:hover { background: var(--surface-3); color: var(--text-primary); }
  .art-btn.on { color: var(--accent); background: var(--accent-bg); }
  .art-btn.err { color: var(--danger); }
  .art-kind { font-size: var(--fs-caption); color: var(--text-muted); background: var(--surface-3); padding: 2px 8px; border-radius: 6px; margin-left: 3px; }
  .art-diff {
    border-top: 1px solid var(--border); background: rgba(0,0,0,0.22);
    max-height: 260px; overflow: auto; padding: 6px 0;
    font-family: var(--font-mono); font-size: 11px; line-height: 1.55;
  }
  .dl { display: flex; padding: 0 10px; white-space: pre-wrap; overflow-wrap: anywhere; }
  .dl-n { flex-shrink: 0; width: 26px; text-align: right; padding-right: 8px; color: var(--text-disabled); user-select: none; }
  .dl-g { flex-shrink: 0; width: 12px; color: var(--text-faint); user-select: none; }
  .dl-t { flex: 1; min-width: 0; }
  .dl.add { background: rgba(61, 214, 164, 0.10); }
  .dl.add .dl-g, .dl.add .dl-t { color: var(--success, #3dd6a4); }
  .dl.rem { background: rgba(240, 110, 110, 0.10); }
  .dl.rem .dl-g, .dl.rem .dl-t { color: var(--danger, #f06e6e); }
  .dl.eq .dl-t { color: var(--text-muted); }
  .dl-none { padding: 8px 12px; font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-sans); }

  @keyframes ws-in { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: none; } }

  @media (prefers-reduced-motion: reduce) {
    .step, .exec, .trace-row, .artifact { animation: none; }
    .ring-dot, .fill.indet { animation: none; }
    .ws-body { scroll-behavior: auto; }
  }
</style>
