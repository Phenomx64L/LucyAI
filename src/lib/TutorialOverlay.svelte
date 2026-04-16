<!-- ── TutorialOverlay.svelte ────────────────────────────────────────────────
     Guided spotlight tour — recorre la UI de arriba hacia abajo.
     SVG mask crea un "hole" sobre cada módulo; tooltip flotante con
     posicionamiento adaptativo que no sale del viewport.
     Props  : show (bindable), isEN
     Events : done
──────────────────────────────────────────────────────────────────────────── -->
<script>
    import { createEventDispatcher, tick, onMount, onDestroy } from 'svelte';

    export let show = false;
    export let isEN = false;

    const dispatch = createEventDispatcher();

    // ── Steps — ordered top→bottom following the UI layout ─────────────────
    // tip: 'bottom'|'top'|'right'|'left'  where to place the tooltip callout
    // view: which activeView the parent must switch to before spotlighting
                const STEPS = [
        {
            sel: ['.chat-wrap.on .chat-area', '.chat-wrap.on', '.panel'],
            tip: 'left',
            view: 'terminal',
            tES: '💬 Terminal IA — Bucle Agéntico',
            tEN: '💬 AI Terminal — Agentic Loop',
            dES: 'El corazón de Lucy. Escribe tu instrucción y la IA no solo te contestará, sino que <b>evaluará, verificará y ejecutará</b> automáticamente hasta completar la tarea (Bucle de Auto-curación).',
            dEN: 'The core of Lucy. Type an instruction and the AI will not only reply, but <b>evaluate, verify and auto-execute</b> commands until the task is complete (Self-Healing Loop).',
        },
        {
            sel: ['.sidebar .sb-it[title*="NexShell"]', '.sidebar .sb-it[title*="exShell"]'],
            fallback: '.sidebar .sb-it[title*="NexShell"]',
            tip: 'right',
            view: 'nexshell',
            tES: '🔌 NexShell — Infraestructura',
            tEN: '🔌 NexShell — Infrastructure',
            dES: 'Conecta servidores por SSH, WinRM, Bases de Datos o clústeres Kubernetes de forma nativa. La terminal de NexShell incluye <b>Sugerencias Inteligentes (Ghost Text)</b> en tiempo real.',
            dEN: 'Bind to SSH, WinRM, Databases, or Kubernetes clusters natively. the NexShell terminal includes real-time <b>Intelligent Ghost Text Suggestions</b>.',
        },
        {
            sel: ['.sidebar .sb-it[title*="Skills"]', '.sidebar .sb-it[title*="abilidades"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '⚡ Skills Manager',
            tEN: '⚡ Skills Manager',
            dES: 'Automatización pura. Crea "Macros" y Playbooks ejecutables con 1 clic impulsados por IA. Guarda tus rutinas frecuentes de administración y Lucy las ejecutará cuando menciones las palabras clave (Triggers).',
            dEN: 'Pure automation. Create 1-click executable Macros & Playbooks powered by AI. Save your frequent routines and Lucy will execute them whenever you mention the Triggers.',
        },
        {
            sel: ['.sidebar [title*="Settings"]', '.sidebar [title*="Configurac"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '⚙️ Configuración & Inteligencia',
            tEN: '⚙️ System Settings',
            dES: 'Configura tus Modelos (LLM), gestiona Permisos Locales y administra <b>Secretos MCP</b>. El protocolo MCP (Model Context Protocol) permite a Lucy aprender nuevas herramientas de forma dinámica.',
            dEN: 'Configure LLMs, manage Security Permissions and <b>MCP Secrets</b>. The MCP Protocol enables dynamic "plug and play" tool learning for Lucy.',
        },
        {
            sel: ['.sidebar .sb-it[title*="Dashboard"]', '.sidebar'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'dashboard',
            tES: '📊 Dashboard y Métricas',
            tEN: '📊 Metrics Dashboard',
            dES: 'Métricas instantáneas locales o de hosts remotos. Monitorea CPU, Memoria, Disco y red a través de gráficas vectoriales. Una vista panorámica del rendimiento.',
            dEN: 'Instant metrics for local or remote hosts. Monitor CPU, Memory, Disk and Network via vector graphs. A panoramic view of performance.',
        }
    ];

    // ── State (plain let — never assigned inside $: derivations) ────────────
    let step   = 0;
    let W      = 0;
    let H      = 0;
    let spot   = { x: 50, y: 50, w: 300, h: 60, r: 8 };
    let ready  = false;
    let _shown = false;

    // ── Read-only derivations ───────────────────────────────────────────────
    $: cur      = STEPS[step] || STEPS[0];
    $: title    = isEN ? cur.tEN : cur.tES;
    $: descHtml = isEN ? cur.dEN : cur.dES;
    $: isLast   = step === STEPS.length - 1;
    $: pct      = Number(((step + 1) / STEPS.length * 100).toFixed(1));

    // ── Watch show toggle — safe: only reads show, writes go through function ─
    $: handleShowChange(show);

    function handleShowChange(s) {
        if (s && !_shown) { _shown = true;  goToStep(0); }
        else if (!s)      { _shown = false; ready = false; }
    }

    async function goToStep(n) {
        step  = n;
        ready = false;
        // Navigate parent to the correct view for this step
        const targetView = STEPS[n]?.view;
        if (targetView) {
            dispatch('navigate', targetView);
        }
        // Give the view transition time to render (~200ms) before computing spotlight
        await tick();
        await new Promise(r => setTimeout(r, 250));
        await tick();
        calcSpot();
    }

    function calcSpot() {
        W = window.innerWidth;
        H = window.innerHeight;
        const c = STEPS[step] || STEPS[0];
        let el = null;
        for (const sel of c.sel) {
            el = document.querySelector(sel);
            if (el) break;
        }
        if (!el && c.fallback) el = document.querySelector(c.fallback);
        if (el) {
            const r   = el.getBoundingClientRect();
            const pad = 10;
            spot = {
                x: Math.max(0,     r.left   - pad),
                y: Math.max(0,     r.top    - pad),
                w: Math.min(W - 4, r.width  + pad * 2),
                h: Math.min(H - 4, r.height + pad * 2),
                r: 10,
            };
        } else {
            spot = { x: W / 2 - 180, y: H / 2 - 60, w: 360, h: 120, r: 10 };
        }
        ready = true;
    }

    // ── Tooltip positioning — clamps fully inside viewport ──────────────────
    // TW/TH are conservative max estimates; CSS also enforces max-height+scroll
    function tipStyle(s, w, h) {
        const TW  = 320;
        const TH  = 340;   // conservative upper bound — CSS max-height handles overflow
        const pos = (STEPS[step] || STEPS[0]).tip;
        const cx  = s.x + s.w / 2;
        const cy  = s.y + s.h / 2;
        const p   = 14;

        if (pos === 'right') {
            const left = Math.min(s.x + s.w + p, w - TW - p);
            const top  = Math.max(p, Math.min(cy - TH / 2, h - TH - p));
            return `left:${left}px;top:${top}px;`;
        }
        if (pos === 'left') {
            const left = Math.max(p, s.x - TW - p);
            const top  = Math.max(p, Math.min(cy - TH / 2, h - TH - p));
            return `left:${left}px;top:${top}px;`;
        }
        if (pos === 'bottom') {
            const left = Math.max(p, Math.min(cx - TW / 2, w - TW - p));
            const top  = Math.min(s.y + s.h + p, h - TH - p);
            return `left:${left}px;top:${top}px;`;
        }
        // 'top' — show tooltip ABOVE the spotlight; force above if spotlight is near bottom
        const left  = Math.max(p, Math.min(cx - TW / 2, w - TW - p));
        // If element is in bottom 40% of screen, always show above it
        const above = s.y - TH - p;
        const below = s.y + s.h + p;
        const top   = above >= p ? above : Math.max(p, below);
        return `left:${left}px;top:${top}px;`;
    }

    // ── Navigation ──────────────────────────────────────────────────────────
    function next() { if (!isLast) goToStep(step + 1); else done(); }
    function prev() { if (step > 0) goToStep(step - 1); }
    function done() {
        show = false;
        localStorage.setItem('lucy_tutorial_done', '1');
        dispatch('done');
    }
    function onKey(e) {
        if      (e.key === 'ArrowRight' || e.key === 'Enter') { e.preventDefault(); next(); }
        else if (e.key === 'ArrowLeft')                       { e.preventDefault(); prev(); }
        else if (e.key === 'Escape')                          { done(); }
    }

    // ── Resize ───────────────────────────────────────────────────────────────
    function onResize() { if (show) calcSpot(); }
    onMount(()   => window.addEventListener('resize', onResize));
    onDestroy(() => window.removeEventListener('resize', onResize));
</script>

{#if show}
<!-- ── SVG spotlight overlay ──────────────────────────────────────────── -->
<svg class="tut-svg"
     viewBox="0 0 {W || 1280} {H || 800}"
     preserveAspectRatio="none"
     style="width:{W || 1280}px;height:{H || 800}px"
     aria-hidden="true">
  <defs>
    <mask id="tut-hole">
      <rect width="100%" height="100%" fill="white"/>
      {#if ready}
        <rect x={spot.x} y={spot.y} width={spot.w} height={spot.h}
              rx={spot.r} fill="black"/>
      {/if}
    </mask>
  </defs>
  <rect width="100%" height="100%" fill="rgba(2,6,12,0.88)" mask="url(#tut-hole)"/>
  {#if ready}
    <rect x={spot.x - 1} y={spot.y - 1}
          width={spot.w + 2} height={spot.h + 2}
          rx={spot.r + 1}
          fill="none"
          stroke="rgba(16,185,129,0.6)"
          stroke-width="1.5"
          class="tut-ring"/>
  {/if}
</svg>

<!-- ── Tooltip callout ─────────────────────────────────────────────────── -->
{#if ready}
<div class="tut-tip"
     style={tipStyle(spot, W, H)}
     role="dialog"
     aria-label={isEN ? 'Tutorial' : 'Tutorial de Lucy'}
     on:keydown={onKey}
     tabindex="-1">

  <!-- Progress bar -->
  <div class="tut-prog"><div class="tut-bar" style="width:{pct}%"></div></div>

  <!-- Header -->
  <div class="tut-hdr">
    <span class="tut-badge">{step + 1} / {STEPS.length}</span>
    <button class="tut-skip" on:click={done}>{isEN ? 'Exit' : 'Salir'} ✕</button>
  </div>

  <!-- Scrollable content area -->
  <div class="tut-body">
    <h3 class="tut-title">{title}</h3>
    <div class="tut-desc">{@html descHtml}</div>
  </div>

  <!-- Dot indicators -->
  <div class="tut-dots" role="tablist">
    {#each STEPS as _, i}
      <button class="tut-dot" class:on={i === step}
              role="tab" aria-selected={i === step}
              aria-label="{isEN ? 'Step' : 'Paso'} {i + 1}"
              on:click={() => goToStep(i)}></button>
    {/each}
  </div>

  <!-- Navigation buttons -->
  <div class="tut-foot">
    <button class="tut-btn tut-ghost" on:click={prev} disabled={step === 0}>
      ← {isEN ? 'Back' : 'Atrás'}
    </button>
    <button class="tut-btn tut-pri" on:click={next}>
      {#if isLast}{isEN ? '🚀 Done!' : '🚀 ¡Listo!'}{:else}{isEN ? 'Next →' : 'Siguiente →'}{/if}
    </button>
  </div>
</div>
{/if}
{/if}

<style>
  /* ── SVG overlay ──────────────────────────────────────────────────────── */
  .tut-svg {
    position: fixed; top: 0; left: 0; z-index: 10000;
    pointer-events: none; display: block;
  }
  .tut-ring {
    animation: tut-pulse 2s ease-in-out infinite;
    filter: drop-shadow(0 0 6px rgba(16,185,129,0.5)) drop-shadow(0 0 12px rgba(16,185,129,0.25));
  }
  @keyframes tut-pulse {
    0%,100% { stroke-opacity:.5; stroke-width:1.5; }
    50%     { stroke-opacity:1;  stroke-width:2.5; }
  }

  /* ── Tooltip card ─────────────────────────────────────────────────────── */
  .tut-tip {
    position: fixed; z-index: 10001;
    width: 320px;
    max-height: calc(100vh - 28px);        /* never overflow screen */
    display: flex; flex-direction: column; /* children stack vertically */
    background: var(--bg2, #0b0e14);
    border: 1px solid var(--bdr, #1a2030);
    border-radius: 14px; overflow: hidden;
    box-shadow: 0 20px 60px rgba(0,0,0,.8);
    animation: tut-pop .25s cubic-bezier(.34,1.4,.64,1);
    outline: none;
  }
  @keyframes tut-pop { from{opacity:0;transform:scale(.93);} to{opacity:1;transform:scale(1);} }

  /* ── Progress ─────────────────────────────────────────────────────────── */
  .tut-prog { height:3px; background:var(--bdr,#1a2030); flex-shrink:0; }
  .tut-bar  { height:100%; background:var(--acc,#10b981); border-radius:0 2px 2px 0; transition:width .35s cubic-bezier(.4,0,.2,1); }

  /* ── Header ───────────────────────────────────────────────────────────── */
  .tut-hdr {
    display:flex; align-items:center; justify-content:space-between;
    padding:10px 14px 0; flex-shrink:0;
  }
  .tut-badge { font-size:10px; color:var(--txt3,#475569); font-weight:700; letter-spacing:.5px; }
  .tut-skip  {
    background:none; border:none; font-family:inherit;
    color:var(--txt3,#475569); font-size:10px;
    cursor:pointer; padding:2px 5px; border-radius:4px; transition:.15s;
  }
  .tut-skip:hover { color:var(--txt2,#7a8a9a); background:rgba(255,255,255,.05); }

  /* ── Scrollable body ──────────────────────────────────────────────────── */
  .tut-body {
    flex: 1; overflow-y: auto; min-height: 0;
    padding-bottom: 4px;
  }
  .tut-body::-webkit-scrollbar { width:3px; }
  .tut-body::-webkit-scrollbar-thumb { background:var(--bdr2,#222c3a); border-radius:2px; }

  .tut-title {
    font-size:14px; font-weight:700; color:white;
    margin:10px 14px 6px; line-height:1.3;
  }
  .tut-desc {
    margin:0 14px 10px;
    font-size:12px; line-height:1.8;
    color:var(--txt2,#7a8a9a);
  }
  :global(.tut-desc b)    { color:var(--txt,#dde3ea); font-weight:600; }
  :global(.tut-desc code) {
    font-family:var(--mono,monospace); font-size:11px;
    color:var(--acc,#10b981); background:rgba(16,185,129,.07);
    padding:1px 4px; border-radius:3px;
  }
  :global(.tut-desc kbd) {
    background:rgba(255,255,255,.07); border:1px solid var(--bdr2,#222c3a);
    border-radius:4px; color:var(--acc,#10b981);
    font-size:10px; font-family:inherit;
    padding:1px 5px; white-space:nowrap;
  }

  /* ── Dot indicators ───────────────────────────────────────────────────── */
  .tut-dots { display:flex; gap:4px; justify-content:center; padding:6px 14px 4px; flex-shrink:0; flex-wrap:wrap; }
  .tut-dot {
    width:5px; height:5px; border-radius:50%;
    border:none; padding:0; cursor:pointer;
    background:var(--bdr2,#222c3a); transition:.2s; flex-shrink:0;
  }
  .tut-dot.on { background:var(--acc,#10b981); width:14px; border-radius:3px; }
  .tut-dot:hover:not(.on) { background:var(--txt3,#475569); }

  /* ── Footer buttons ───────────────────────────────────────────────────── */
  .tut-foot {
    display:flex; gap:8px; padding:10px 14px 12px;
    border-top:1px solid var(--bdr,#1a2030); flex-shrink:0;
  }
  .tut-btn {
    flex:1; padding:9px; border:none; border-radius:7px;
    font-size:12px; font-weight:600; font-family:inherit;
    cursor:pointer; transition:.15s;
  }
  .tut-pri  { background:var(--acc,#10b981); color:#030b06; }
  .tut-pri:hover { opacity:.88; }
  .tut-ghost {
    background:transparent; color:var(--txt2,#7a8a9a);
    border:1px solid var(--bdr,#1a2030) !important;
  }
  .tut-ghost:hover:not(:disabled) { background:rgba(255,255,255,.04); color:var(--txt,#dde3ea); }
  .tut-ghost:disabled { opacity:.3; cursor:not-allowed; }

  /* ── Light theme ──────────────────────────────────────────────────────── */
  :global(:root.light .tut-tip)   { background:#fff; border-color:var(--bdr2); box-shadow:0 20px 50px rgba(0,0,0,.2); }
  :global(:root.light .tut-title) { color:var(--txt); }
  :global(:root.light .tut-desc)  { color:var(--txt2); }
  :global(:root.light .tut-desc b)    { color:var(--txt); }
  :global(:root.light .tut-desc code) { background:rgba(0,168,107,.08); color:var(--acc); }
  :global(:root.light .tut-desc kbd)  { background:var(--bg3); border-color:var(--bdr2); color:var(--acc); }
  :global(:root.light .tut-foot)  { border-top-color:var(--bdr); }
  :global(:root.light .tut-dot)   { background:var(--bdr2); }
  :global(:root.light .tut-dot.on){ background:var(--acc); }
  :global(:root.light .tut-ghost) { border-color:var(--bdr) !important; }
  :global(:root.light .tut-ghost:hover:not(:disabled)) { background:rgba(0,0,0,.04); }
  :global(:root.light .tut-skip)  { color:var(--txt3); }
  :global(:root.light .tut-skip:hover) { background:rgba(0,0,0,.05); color:var(--txt2); }
  :global(:root.light .tut-badge) { color:var(--txt3); }
  :global(:root.light .tut-body::-webkit-scrollbar-thumb) { background:var(--bdr2); }
</style>
