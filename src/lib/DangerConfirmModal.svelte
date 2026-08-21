<script>
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { createEventDispatcher, onMount } from 'svelte';
    import { riskColor, riskIcon } from '$lib/hooks/command-guard';
    import Monitor from '@tabler/icons-svelte/icons/device-desktop';

    import Lightbulb from '@tabler/icons-svelte/icons/bulb';
    const dispatch = createEventDispatcher();

    /** @type {import('$lib/hooks/command-guard').RiskAssessment} */
    export let assessment = null;
    export let hostName   = '';
    export let source     = 'manual'; // manual | ai | broadcast | playbook

    let countdown   = 0;
    let canConfirm  = false;
    let timer       = null;
    // Guard so the init only runs when a NEW assessment arrives — NOT on every
    // countdown change. The old block read `countdown`, so each `countdown--`
    // in the interval re-triggered it, reset countdown back to 3/5, and
    // restarted the timer → the counter was stuck forever and the confirm
    // button never enabled (the "Espera 3s…" freeze the user reported).
    let _lastAssessment = null;

    $: if (assessment && assessment !== _lastAssessment) {
        _lastAssessment = assessment;
        // Critical: 5s wait, High: 3s, else immediate
        countdown = assessment.level === 'critical' ? 5 : assessment.level === 'high' ? 3 : 0;
        canConfirm = countdown === 0;
        if (countdown > 0) startCountdown();
    }
    // Reset the guard when the modal closes so re-opening always re-arms.
    $: if (!assessment) _lastAssessment = null;

    function startCountdown() {
        canConfirm = false;
        if (timer) clearInterval(timer);
        timer = setInterval(() => {
            countdown--;
            if (countdown <= 0) {
                canConfirm = true;
                clearInterval(timer);
                timer = null;
            }
        }, 1000);
    }

    onMount(() => { return () => { if (timer) clearInterval(timer); }; });

    function confirm() {
        if (!canConfirm) return;
        dispatch('confirm');
    }
    function cancel() {
        dispatch('cancel');
    }
    function handleKey(e) {
        if (e.key === 'Escape') cancel();
        if (e.key === 'Enter' && canConfirm) confirm();
    }
</script>

<svelte:window on:keydown={handleKey} />

{#if assessment}
<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="dg-overlay" on:click|self={cancel}>
  <div class="dg-modal" style="--risk-color:{riskColor(assessment.level)}">
    <!-- Header -->
    <div class="dg-hdr">
      <span class="dg-hdr-icon">{riskIcon(assessment.level)}</span>
      <span class="dg-hdr-title">
        {#if assessment.level === 'critical'}
          {$trad('RIESGO CRITICO DETECTADO')}
        {:else if assessment.level === 'high'}
          {$trad('RIESGO ALTO DETECTADO')}
        {:else}
          {$trad('RIESGO MODERADO DETECTADO')}
        {/if}
      </span>
      <div class="dg-score">
        <span class="dg-score-val">{assessment.score}</span>
        <span class="dg-score-lbl">{$trad('Puntaje')}</span>
      </div>
    </div>

    <!-- Command preview -->
    <div class="dg-section">
      <div class="dg-label">{$trad('Comando')}</div>
      <pre class="dg-cmd">{assessment.command}</pre>
    </div>

    <!-- Target -->
    <div class="dg-meta">
      <span class="dg-meta-item" style="display:flex;align-items:center;gap:4px;"><Monitor size={12} stroke={2}/> {hostName || 'local'}</span>
      <span class="dg-meta-item">
        {#if source === 'ai'}✦ AI{:else if source === 'broadcast'}◎ Broadcast{:else if source === 'playbook'}≡ Playbook{:else}⌨ Manual{/if}
      </span>
    </div>

    <!-- Summary -->
    <div class="dg-section">
      <div class="dg-label">{$trad('Analisis')}</div>
      <div class="dg-summary">{assessment.summary}</div>
      {#if assessment.suggestion}
        <div class="dg-suggestion" style="display:flex;align-items:flex-start;gap:5px;"><Lightbulb size={11} stroke={2}/> {assessment.suggestion}</div>
      {/if}
    </div>

    <!-- Risk matches -->
    {#if assessment.matches.length > 0}
    <div class="dg-section">
      <div class="dg-label">{$trad('Detecciones')} ({assessment.matches.length})</div>
      <div class="dg-matches">
        {#each assessment.matches as m}
        <div class="dg-match">
          <span class="dg-match-sev" style="color:{riskColor(m.severity)}">{riskIcon(m.severity)}</span>
          <span class="dg-match-cat">{m.category}</span>
          <span class="dg-match-desc">{m.description}</span>
          <span class="dg-match-pts">+{m.points}</span>
        </div>
        {/each}
      </div>
    </div>
    {/if}

    <!-- Risk bar -->
    <div class="dg-bar-wrap">
      <div class="dg-bar">
        <div class="dg-bar-fill" style="width:{assessment.score}%"></div>
      </div>
      <div class="dg-bar-labels">
        <span style="color:#88aacc">{$trad('Bajo')}</span>
        <span style="color:#ffaa33">{$trad('Medio')}</span>
        <span style="color:#ff6644">{$trad('Alto')}</span>
        <span style="color:#ff2020">{$trad('Critico')}</span>
      </div>
    </div>

    <!-- Actions -->
    <div class="dg-actions">
      <button class="dg-btn cancel" on:click={cancel}>
        {$trad('Cancelar')}
      </button>
      <button class="dg-btn confirm" on:click={confirm} disabled={!canConfirm}
        style="--btn-color:{riskColor(assessment.level)}">
        {#if !canConfirm}
          {$trad('Espera')} {countdown}s...
        {:else}
          {$trad('Ejecutar de todas formas')}
        {/if}
      </button>
    </div>
  </div>
</div>
{/if}

<style>
    .dg-overlay{position:fixed;inset:0;z-index:995;background:rgba(0,0,0,.7);display:flex;align-items:center;justify-content:center;backdrop-filter:blur(6px);animation:dg-fadein .15s ease;}
    @keyframes dg-fadein{from{opacity:0}to{opacity:1}}

    .dg-modal{background:var(--bg2,#0b0e14);border:2px solid var(--risk-color);border-radius:14px;width:min(520px,92vw);max-height:80vh;overflow-y:auto;box-shadow:0 0 40px rgba(0,0,0,.6), 0 0 80px color-mix(in srgb, var(--risk-color) 15%, transparent);animation:dg-slidein .2s ease;}
    @keyframes dg-slidein{from{transform:translateY(20px);opacity:0}to{transform:translateY(0);opacity:1}}

    .dg-hdr{display:flex;align-items:center;gap:10px;padding:14px 18px;background:color-mix(in srgb, var(--risk-color) 8%, transparent);border-bottom:1px solid color-mix(in srgb, var(--risk-color) 20%, transparent);}
    .dg-hdr-icon{font-size:24px;}
    .dg-hdr-title{font-size:13px;font-weight:800;color:var(--risk-color);letter-spacing:.5px;text-transform:uppercase;flex:1;}
    .dg-score{display:flex;flex-direction:column;align-items:center;background:rgba(0,0,0,.3);border-radius:8px;padding:4px 10px;}
    .dg-score-val{font-size:22px;font-weight:300;color:var(--risk-color);font-family:var(--mono);}
    .dg-score-lbl{font-size:8px;color:#4a5a6a;text-transform:uppercase;letter-spacing:.3px;}

    .dg-section{padding:10px 18px;}
    .dg-label{font-size:9px;color:#4a5a6a;font-weight:700;text-transform:uppercase;letter-spacing:.4px;margin-bottom:5px;}
    .dg-cmd{background:rgba(0,0,0,.35);border:1px solid rgba(255,255,255,.05);border-radius:6px;padding:8px 10px;color:var(--txt);font-family:var(--mono);font-size:12px;white-space:pre-wrap;word-break:break-all;max-height:100px;overflow-y:auto;margin:0;}
    .dg-summary{font-size:12px;color:var(--txt2);line-height:1.6;}
    .dg-suggestion{margin-top:6px;font-size:11px;color:var(--amber,#ffaa33);line-height:1.5;padding:6px 10px;background:rgba(255,170,50,.06);border-radius:6px;border:1px solid rgba(255,170,50,.12);}

    .dg-meta{display:flex;gap:12px;padding:4px 18px;font-size:11px;color:#4a5a6a;}
    .dg-meta-item{display:flex;align-items:center;gap:4px;}

    .dg-matches{display:flex;flex-direction:column;gap:4px;}
    .dg-match{display:flex;align-items:center;gap:8px;padding:4px 8px;background:rgba(0,0,0,.15);border-radius:4px;font-size:11px;}
    .dg-match-sev{font-size:13px;flex-shrink:0;}
    .dg-match-cat{font-size:9px;color:var(--blue,#4aa8ff);font-weight:700;text-transform:uppercase;letter-spacing:.3px;background:rgba(74,168,255,.08);padding:1px 5px;border-radius:3px;flex-shrink:0;}
    .dg-match-desc{color:var(--txt2);flex:1;}
    .dg-match-pts{font-size:10px;color:var(--risk-color);font-weight:700;font-family:var(--mono);flex-shrink:0;}

    .dg-bar-wrap{padding:6px 18px 2px;}
    .dg-bar{height:6px;background:rgba(255,255,255,.04);border-radius:3px;overflow:hidden;}
    .dg-bar-fill{height:100%;background:linear-gradient(90deg, #88aacc 0%, #ffaa33 40%, #ff6644 70%, #ff2020 100%);border-radius:3px;transition:width .4s ease;}
    .dg-bar-labels{display:flex;justify-content:space-between;font-size:8px;margin-top:2px;padding:0 2px;}

    .dg-actions{display:flex;gap:10px;padding:14px 18px;justify-content:flex-end;border-top:1px solid rgba(255,255,255,.04);margin-top:4px;}
    .dg-btn{padding:8px 18px;border-radius:7px;font-size:12px;font-weight:600;cursor:pointer;transition:.15s;border:1px solid;}
    .dg-btn.cancel{background:rgba(255,255,255,.04);border-color:var(--bdr);color:var(--txt2);}
    .dg-btn.cancel:hover{background:rgba(255,255,255,.08);color:var(--txt);}
    .dg-btn.confirm{background:color-mix(in srgb, var(--btn-color) 12%, transparent);border-color:color-mix(in srgb, var(--btn-color) 30%, transparent);color:var(--btn-color);}
    .dg-btn.confirm:hover:not(:disabled){background:color-mix(in srgb, var(--btn-color) 20%, transparent);}
    .dg-btn.confirm:disabled{opacity:.4;cursor:not-allowed;}

    :global(:root.light) .dg-modal{background:#fff;}
    :global(:root.light) .dg-cmd{background:#f5f5f5;}
</style>
