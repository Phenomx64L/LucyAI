<script>
    import { createEventDispatcher } from 'svelte';
    import { PHASE_ORDER, phaseLabel, phaseIcon } from '$lib/hooks/turn-loop';
    import CheckCircle from '@tabler/icons-svelte/icons/circle-check';

    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
    const dispatch = createEventDispatcher();

    /** @type {import('$lib/hooks/turn-loop').TurnLoopState | null} */
    export let loop = null;
    export let isEN = false;

    $: elapsed = loop ? Math.round((Date.now() - loop.startTime) / 1000) : 0;
    $: phaseIdx = loop ? PHASE_ORDER.indexOf(loop.phase) : -1;
    $: progress = loop
        ? loop.phase === 'done' ? 100
        : loop.phase === 'failed' ? 100
        : Math.round(((phaseIdx >= 0 ? phaseIdx : 0) / (PHASE_ORDER.length - 1)) * 100)
        : 0;

    function stop() { dispatch('stop'); }

    function fmtTime(s) {
        if (s < 60) return s + 's';
        return Math.floor(s / 60) + 'm ' + (s % 60) + 's';
    }
</script>

{#if loop && loop.active}
<div class="tl-panel" class:resolved={loop.phase === 'done' && loop.resolved}
     class:failed={loop.phase === 'failed' || (loop.phase === 'done' && !loop.resolved)}>

  <!-- Header -->
  <div class="tl-hdr">
    <span class="tl-icon">↻</span>
    <span class="tl-title">Turn-Loop</span>
    <span class="tl-iter">{isEN ? 'Iter' : 'Iter'} {loop.iteration}/{loop.maxIterations}</span>
    <span class="tl-time">{fmtTime(elapsed)}</span>
    {#if loop.phase !== 'done' && loop.phase !== 'failed'}
    <button class="tl-stop" on:click={stop} title={isEN ? 'Stop loop' : 'Detener loop'}>■</button>
    {/if}
  </div>

  <!-- Problem -->
  <div class="tl-problem">
    <span class="tl-problem-lbl">{isEN ? 'Problem' : 'Problema'}:</span>
    {loop.problem.substring(0, 120)}{loop.problem.length > 120 ? '...' : ''}
  </div>

  <!-- Phase progress -->
  <div class="tl-phases">
    {#each PHASE_ORDER as p, i}
    {@const isCurrent = loop.phase === p}
    {@const isDone = phaseIdx > i || loop.phase === 'done'}
    {@const isFailed = loop.phase === 'failed' && phaseIdx === i}
    <div class="tl-phase" class:current={isCurrent} class:done={isDone} class:failed={isFailed}>
      <div class="tl-phase-dot">
        {#if isDone}✓{:else if isCurrent}●{:else if isFailed}✗{:else}○{/if}
      </div>
      <div class="tl-phase-lbl">{phaseIcon(p)} {phaseLabel(p, isEN)}</div>
    </div>
    {#if i < PHASE_ORDER.length - 1}
    <div class="tl-phase-line" class:done={isDone}></div>
    {/if}
    {/each}
  </div>

  <!-- Progress bar -->
  <div class="tl-bar">
    <div class="tl-bar-fill" style="width:{progress}%"></div>
  </div>

  <!-- Steps log -->
  {#if loop.steps.length > 0}
  <div class="tl-steps">
    {#each loop.steps.slice(-4) as step}
    <div class="tl-step">
      <span class="tl-step-phase">{phaseIcon(step.phase)}</span>
      {#if step.command}
        <span class="tl-step-cmd">$ {step.command.substring(0, 60)}{step.command.length > 60 ? '...' : ''}</span>
      {/if}
      {#if step.aiResponse}
        <span class="tl-step-ai">{step.aiResponse.substring(0, 80)}{step.aiResponse.length > 80 ? '...' : ''}</span>
      {/if}
      {#if step.exitCode !== null && step.exitCode !== undefined}
        <span class="tl-step-exit" style="color:{step.exitCode === 0 ? 'var(--acc)' : 'var(--red)'}">
          {step.exitCode === 0 ? '✓' : '✗'}{step.exitCode}
        </span>
      {/if}
    </div>
    {/each}
  </div>
  {/if}

  <!-- Summary when done -->
  {#if loop.phase === 'done' || loop.phase === 'failed'}
  <div class="tl-summary" class:ok={loop.resolved}>
    {#if loop.resolved}
      <CheckCircle size={13} strokeWidth={2} style="color:var(--acc)"/> {isEN ? 'Problem resolved!' : 'Problema resuelto!'}
    {:else}
      <AlertTriangle size={13} strokeWidth={2} style="color:var(--amber)"/> {isEN ? 'Could not fully resolve.' : 'No se pudo resolver completamente.'}
    {/if}
    {#if loop.summary}
      <div class="tl-summary-text">{loop.summary}</div>
    {/if}
  </div>
  {/if}
</div>
{/if}

<style>
    .tl-panel{background:rgba(0,100,200,.04);border:1px solid rgba(0,150,255,.12);border-radius:8px;margin:8px 0;overflow:hidden;transition:.2s;}
    .tl-panel.resolved{border-color:rgba(16,185,129,.2);background:rgba(16,185,129,.03);}
    .tl-panel.failed{border-color:rgba(255,68,68,.2);background:rgba(255,68,68,.03);}

    .tl-hdr{display:flex;align-items:center;gap:8px;padding:8px 12px;border-bottom:1px solid rgba(255,255,255,.04);font-size:12px;}
    .tl-icon{font-size:14px;animation:tl-spin 2s linear infinite;}
    @keyframes tl-spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}}
    .tl-panel.resolved .tl-icon, .tl-panel.failed .tl-icon{animation:none;}
    .tl-title{font-weight:700;color:var(--txt);letter-spacing:.3px;}
    .tl-iter{font-size:10px;color:var(--blue,#4aa8ff);font-family:var(--mono);background:rgba(74,168,255,.08);padding:1px 6px;border-radius:4px;}
    .tl-time{font-size:10px;color:#4a5a6a;font-family:var(--mono);margin-left:auto;}
    .tl-stop{background:rgba(255,68,68,.1);border:1px solid rgba(255,68,68,.2);border-radius:4px;color:var(--red);font-size:10px;padding:2px 6px;cursor:pointer;transition:.15s;}
    .tl-stop:hover{background:rgba(255,68,68,.2);}

    .tl-problem{padding:6px 12px;font-size:11px;color:var(--txt2);border-bottom:1px solid rgba(255,255,255,.03);}
    .tl-problem-lbl{font-weight:700;color:#4a5a6a;font-size:9px;text-transform:uppercase;letter-spacing:.3px;margin-right:4px;}

    .tl-phases{display:flex;align-items:center;padding:10px 12px;gap:0;overflow-x:auto;}
    .tl-phase{display:flex;align-items:center;gap:3px;flex-shrink:0;}
    .tl-phase-dot{width:16px;height:16px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:8px;color:var(--txt3);background:rgba(255,255,255,.03);border:1px solid rgba(255,255,255,.06);flex-shrink:0;font-weight:700;}
    .tl-phase.current .tl-phase-dot{color:var(--acc);border-color:var(--acc);background:rgba(16,185,129,.1);animation:tl-pulse 1.5s ease infinite;}
    .tl-phase.done .tl-phase-dot{color:var(--acc);border-color:rgba(16,185,129,.3);background:rgba(16,185,129,.12);}
    .tl-phase.failed .tl-phase-dot{color:var(--red);border-color:rgba(255,68,68,.3);background:rgba(255,68,68,.1);}
    @keyframes tl-pulse{0%,100%{box-shadow:0 0 0 0 rgba(16,185,129,.2)}50%{box-shadow:0 0 0 4px rgba(16,185,129,.1)}}
    .tl-phase-lbl{font-size:9px;color:#4a5a6a;white-space:nowrap;}
    .tl-phase.current .tl-phase-lbl{color:var(--acc);font-weight:700;}
    .tl-phase.done .tl-phase-lbl{color:var(--acc);}
    .tl-phase-line{width:12px;height:1px;background:rgba(255,255,255,.06);flex-shrink:0;margin:0 2px;}
    .tl-phase-line.done{background:rgba(16,185,129,.3);}

    .tl-bar{height:3px;background:rgba(255,255,255,.04);margin:0 12px 8px;border-radius:2px;overflow:hidden;}
    .tl-bar-fill{height:100%;background:linear-gradient(90deg, var(--blue,#4aa8ff), var(--acc));border-radius:2px;transition:width .4s ease;}
    .tl-panel.failed .tl-bar-fill{background:var(--red);}

    .tl-steps{padding:0 12px 6px;display:flex;flex-direction:column;gap:3px;}
    .tl-step{display:flex;align-items:center;gap:6px;font-size:10px;color:#4a5a6a;padding:2px 0;}
    .tl-step-phase{flex-shrink:0;}
    .tl-step-cmd{font-family:var(--mono);color:var(--txt2);flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
    .tl-step-ai{color:#5a6a7a;font-style:italic;flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
    .tl-step-exit{font-family:var(--mono);font-weight:700;flex-shrink:0;font-size:9px;}

    .tl-summary{padding:8px 12px;font-size:12px;font-weight:600;border-top:1px solid rgba(255,255,255,.04);}
    .tl-summary.ok{color:var(--acc);}
    .tl-summary:not(.ok){color:var(--amber,#ffaa33);}
    .tl-summary-text{font-size:11px;font-weight:400;color:var(--txt2);margin-top:4px;line-height:1.5;}
</style>
