<!-- ── TutorialOverlay.svelte ────────────────────────────────────────────────
     Guided spotlight tour — recorre la UI de arriba hacia abajo.
     SVG mask crea un "hole" sobre cada módulo; tooltip flotante con
     posicionamiento adaptativo que no sale del viewport.
     Props  : show (bindable), isEN
     Events : done
──────────────────────────────────────────────────────────────────────────── -->
<script>
    import { createEventDispatcher, tick, onMount, onDestroy } from 'svelte';
    import { safeSetLSString } from '$lib/safe-ls';

    export let show = false;
    export let isEN = false;

    const dispatch = createEventDispatcher();

    // Bumped per release. Keep in sync with package.json + Cargo.toml.
    const LUCY_VERSION = '1.2.1';

    // ── Steps — ordered top→bottom following the UI layout ─────────────────
    // tip: 'bottom'|'top'|'right'|'left'  where to place the tooltip callout
    // view: which activeView the parent must switch to before spotlighting
    // welcome=true skips the spotlight (centered card) for the intro step.
                const STEPS = [
        {
            sel: ['.bbar', '.ws'],
            fallback: 'body',
            tip: 'top',
            view: 'terminal',
            welcome: true,
            tES: `✦ Bienvenido a Lucy v${LUCY_VERSION}`,
            tEN: `✦ Welcome to Lucy v${LUCY_VERSION}`,
            dES: 'Hola — esta es una <b>versión renovada</b> con foco en estabilidad, observabilidad y experiencia visual. <br><br>Lo nuevo en v' + LUCY_VERSION + ':<br>• <b>Indicador de estado ambient</b> en el footer (idle / pensando / ejecutando)<br>• <b>Detección de anomalías</b> estadísticas en CPU/RAM<br>• <b>Predictor de costo</b> en vivo en el input<br>• <b>Notebook export</b> para reusar conversaciones<br>• <b>Búsqueda fuzzy</b> en Reglas de Permisos<br>• Bug fixes: NexShell, multi-step agent, timer "pensando"<br><br>Vamos a recorrer la interfaz.',
            dEN: `Hi — this is a <b>refreshed release</b> focused on stability, observability and visual polish. <br><br>What's new in v${LUCY_VERSION}:<br>• <b>Ambient status indicator</b> in the footer (idle / thinking / executing)<br>• <b>Statistical anomaly detection</b> on CPU/RAM<br>• <b>Live cost predictor</b> in the input<br>• <b>Notebook export</b> to replay sessions<br>• <b>Fuzzy search</b> in Permission Rules<br>• Bug fixes: NexShell, multi-step agent, "thinking" timer<br><br>Let's walk through the interface.`,
        },
        {
            sel: ['.chat-wrap.on .chat-area', '.chat-wrap.on', '.panel'],
            tip: 'left',
            view: 'terminal',
            tES: '↗ Terminal IA — Bucle Agéntico',
            tEN: '↗ AI Terminal — Agentic Loop',
            dES: 'El corazón de Lucy. Escribe tu instrucción y la IA no solo te contestará, sino que <b>evaluará, verificará y ejecutará</b> automáticamente hasta completar la tarea. Incluye <b>PLAN/VERIFY/ROLLBACK</b> — para cambios riesgosos Lucy propone un plan, verifica el resultado y revierte automáticamente si falla. <br><br><b>NUEVO en v1.2.1</b>: cuando le pides múltiples cosas en un solo mensaje (ej. "checa specs y busca en internet"), Lucy ya no se detiene a media tarea.',
            dEN: 'The core of Lucy. Type an instruction and the AI will not only reply, but <b>evaluate, verify and auto-execute</b> commands until the task is complete. Includes <b>PLAN/VERIFY/ROLLBACK</b> — for risky changes Lucy proposes a plan, verifies the outcome, and auto-rolls back if it fails. <br><br><b>NEW in v1.2.1</b>: when you give Lucy multi-step prompts (e.g. "check my specs and search the web"), it no longer stops mid-task.',
        },
        {
            sel: ['.sidebar .sb-it[title*="NexShell"]', '.sidebar .sb-it[title*="exShell"]'],
            fallback: '.sidebar .sb-it[title*="NexShell"]',
            tip: 'right',
            view: 'nexshell',
            tES: '⊟ NexShell — Infraestructura',
            tEN: '⊟ NexShell — Infrastructure',
            dES: 'Conecta servidores por SSH, WinRM, Bases de Datos o clústeres Kubernetes de forma nativa. Incluye <b>Ghost Text en tiempo real</b> y <b>host preflight</b>: antes de ejecutar un comando remoto Lucy verifica conectividad TCP y falla rápido si el host no responde, sin esperar 15 s a un timeout WinRM críptico.',
            dEN: 'Native SSH, WinRM, Database, or Kubernetes connectivity. Features real-time <b>Ghost Text suggestions</b> and <b>host preflight</b>: before any remote command Lucy tests TCP reachability and fails fast when a host is down — no more cryptic 15 s WinRM timeouts.',
        },
        {
            sel: ['.sidebar .sb-it[title*="Log Viewer"]', '.sidebar .sb-it[title*="og Viewer"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'logviewer',
            tES: '≡ Log Viewer — Observabilidad',
            tEN: '≡ Log Viewer — Observability',
            dES: 'Abre, filtra y sigue en tiempo real cualquier log local o remoto (Event Viewer, IIS, /var/log, journalctl). Búsqueda instantánea, resaltado de niveles (ERROR/WARN/INFO) y tail continuo vía SSH o WinRM.',
            dEN: 'Open, filter, and tail any local or remote log (Event Viewer, IIS, /var/log, journalctl). Instant search, level highlighting (ERROR/WARN/INFO), and continuous tail over SSH or WinRM.',
        },
        {
            sel: ['.sidebar .sb-it[title*="Inventory"]', '.sidebar .sb-it[title*="nventario"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'inventory',
            tES: '⊞ Inventory & Compliance',
            tEN: '⊞ Inventory & Compliance',
            dES: 'Descubre automáticamente el hardware, software, servicios y parches de cada host, y evalúalos contra <b>CIS Benchmarks</b> (Windows Server, Ubuntu/RHEL). Genera reportes PDF con el veredicto de cumplimiento de cada control.',
            dEN: 'Auto-discover each host\'s hardware, software, services, and patches, and evaluate them against <b>CIS Benchmarks</b> (Windows Server, Ubuntu/RHEL). Generate PDF reports with pass/fail verdicts per control.',
        },
        {
            sel: ['.sidebar .sb-it[title*="Audit"]', '.sidebar .sb-it[title*="uditor"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'audittrail',
            tES: '◉ Audit Trail — Trazabilidad',
            tEN: '◉ Audit Trail — Accountability',
            dES: 'Cada comando ejecutado, cada skill invocado y cada decisión del agente queda registrado con timestamp, usuario, host destino y resultado. Exporta a PDF para auditorías SOX, ISO 27001 o evidencia forense.',
            dEN: 'Every executed command, invoked skill and agent decision is logged with timestamp, user, target host and result. Export to PDF for SOX, ISO 27001 audits or forensic evidence.',
        },
        {
            // BUG FIX: previous selectors used S-capital "Skills" but the actual
            // title attribute is "Manage skills and runbooks" / "Gestionar
            // skills y runbooks" — lowercase. CSS attribute selectors are
            // case-sensitive on values. Without a match the fallback ".sidebar"
            // fired and the spotlight illuminated the ENTIRE sidebar.
            sel: ['.sidebar .sb-it[title*="skills"]', '.sidebar .sb-it[title*="Skills"]'],
            fallback: '.sidebar .sb-it[title*="killbook"], .sidebar .sb-it[title*="runbook"]',
            tip: 'right',
            view: 'terminal',
            tES: '▸ Skills Manager',
            tEN: '▸ Skills Manager',
            dES: 'Automatización pura. Crea "Macros" y Playbooks ejecutables con 1 clic impulsados por IA. Guarda tus rutinas frecuentes — Lucy las ejecutará al detectar los <b>triggers</b>. Parámetros, tags y contadores de uso persistidos en SQLite.',
            dEN: 'Pure automation. Create 1-click executable Macros & Playbooks powered by AI. Save your frequent routines — Lucy runs them when it detects the <b>triggers</b>. Parameters, tags and usage counters persisted in SQLite.',
        },
        {
            sel: ['.sidebar [title*="Settings"]', '.sidebar [title*="Configurac"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '⚙ Configuración & Seguridad',
            tEN: '⚙ System Settings',
            dES: 'Configura tus Modelos (LLM multi-provider), <b>Permission Rules</b> (reglas allow/block/ask basadas en regex), <b>Cost Tracking</b> (tokens y gasto por modelo), y <b>MCP Secrets</b>. El protocolo MCP permite a Lucy aprender nuevas herramientas de forma dinámica.',
            dEN: 'Configure LLMs (multi-provider), <b>Permission Rules</b> (regex-based allow/block/ask), <b>Cost Tracking</b> (tokens and spend per model), and <b>MCP Secrets</b>. The MCP protocol enables dynamic "plug and play" tool learning.',
        },
        {
            sel: ['.sidebar .sb-it[title*="Dashboard"]', '.sidebar'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'dashboard',
            tES: '◑ Dashboard y Métricas',
            tEN: '◑ Metrics Dashboard',
            dES: 'Métricas instantáneas locales o de hosts remotos. Monitorea CPU, Memoria, Disco y red a través de gráficas vectoriales. Una vista panorámica del rendimiento de toda tu infraestructura.',
            dEN: 'Instant metrics for local or remote hosts. Monitor CPU, Memory, Disk and Network via vector graphs. A panoramic view of your infrastructure\'s performance.',
        },
        {
            // .dash-cards (plural — the GRID) covers all three cards in one
            // spotlight rect. Singular .dash-card would only highlight the
            // first one and leave the others outside the cutout.
            sel: ['.dash-cards', '.dash-card'],
            fallback: '.dash-scroll',
            tip: 'top',
            view: 'dashboard',
            tES: '✦ NUEVO — Detección de Anomalías',
            tEN: '✦ NEW — Anomaly Detection',
            dES: 'Las cards de CPU y RAM ahora muestran un <b>badge σ</b> (sigma) cuando un valor se desvía estadísticamente del promedio reciente del host. Solo aparece para anomalías <b>fuertes</b> (≥3σ) o <b>extremas</b> (≥4σ) — sin alarmismo. Detecta picos sospechosos sin necesidad de configurar umbrales fijos.',
            dEN: 'CPU & RAM cards now show a <b>σ badge</b> (sigma) when a value deviates statistically from the host\'s recent average. Surfaces only on <b>strong</b> (≥3σ) or <b>extreme</b> (≥4σ) anomalies — no false alarms. Catches suspicious spikes without hand-tuning fixed thresholds.',
        },
        {
            // .igrp is the inner input wrapper (textarea + chips). Tighter
            // bounding box than .ibar (which also includes side buttons).
            // Spotlight reads as "the input itself", which is what the
            // tutorial copy talks about.
            sel: ['.igrp', '.ibar'],
            fallback: '.ibar',
            tip: 'top',
            view: 'terminal',
            tES: '✦ NUEVO — Predictor de Costo',
            tEN: '✦ NEW — Cost Predictor',
            dES: 'Mientras escribes en el input, Lucy estima cuántos <b>tokens</b> y cuánto <b>USD</b> costará tu prompt antes de enviarlo. Útil para elegir un modelo más barato cuando la tarea es exploratoria. Aparece junto al modelo activo cuando el prompt supera ~8 caracteres.',
            dEN: 'While you type in the input, Lucy estimates how many <b>tokens</b> and how much <b>USD</b> your prompt will cost before sending. Useful for picking a cheaper model when the task is exploratory. Shown next to the active model when the prompt exceeds ~8 characters.',
        },
        {
            // The footer (.bbar) is only 22px tall — too thin for the spotlight
            // ring to read. We pad the spotlight artificially via the new
            // `padY` field (consumed in calcSpot) so the highlight reads as a
            // band, not a pencil-line. Tip is 'top' so the tooltip floats
            // above the band, never crashing into the input above.
            sel: ['.bbar'],
            fallback: '.bbar',
            tip: 'top',
            view: 'terminal',
            padY: 6,
            tES: '✦ NUEVO — Indicador de Estado',
            tEN: '✦ NEW — Status Indicator',
            dES: 'En la <b>esquina inferior derecha del footer</b> verás un punto que respira con el estado de Lucy: <br>• <span style="color:#10b981;">●</span> verde lento → inactiva, lista<br>• <span style="color:#3b9eff;">●</span> azul rápido → pensando<br>• <span style="color:#f59e0b;">●</span> ámbar con arco → ejecutando<br>• <span style="color:#ef4444;">●</span> rojo flash → error reciente<br><br>El borde del input también adopta el color del estado.',
            dEN: 'In the <b>bottom-right of the footer</b> you\'ll see a dot that breathes with Lucy\'s state: <br>• <span style="color:#10b981;">●</span> slow green → idle, ready<br>• <span style="color:#3b9eff;">●</span> fast blue → thinking<br>• <span style="color:#f59e0b;">●</span> amber with arc → executing<br>• <span style="color:#ef4444;">●</span> red flash → recent error<br><br>The input border also adopts the state color.',
        },
        {
            sel: ['body'],
            fallback: 'body',
            tip: 'top',
            view: 'terminal',
            welcome: true,
            tES: '✦ Atajos esenciales',
            tEN: '✦ Essential shortcuts',
            dES: '<b>Ctrl+P</b> · Paleta de comandos (busca cualquier vista, host o acción) — incluye <b>Exportar pestaña como Notebook</b>.<br><b>Ctrl+T</b> · Nueva terminal.<br><b>Ctrl+L</b> · Limpiar sesión actual.<br><b>Ctrl+F</b> · Buscar en NexShell.<br><b>Ctrl+Shift+Enter</b> · Ejecutar en background.<br><b>Tab</b> · Autocompletar comandos.<br><b>Esc</b> · Cancelar el agente o cerrar modal.',
            dEN: '<b>Ctrl+P</b> · Command palette (find any view, host or action) — includes <b>Export tab as Notebook</b>.<br><b>Ctrl+T</b> · New terminal.<br><b>Ctrl+L</b> · Clear current session.<br><b>Ctrl+F</b> · Find in NexShell.<br><b>Ctrl+Shift+Enter</b> · Run in background.<br><b>Tab</b> · Autocomplete commands.<br><b>Esc</b> · Cancel the agent or close modal.',
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
        // Welcome / overview steps don't spotlight a specific element —
        // they show a centered card over the whole screen with no cutout.
        if (c.welcome) {
            spot = { x: W / 2 - 200, y: H / 2 - 80, w: 400, h: 160, r: 14, welcome: true };
            ready = true;
            return;
        }
        let el = null;
        for (const sel of c.sel) {
            el = document.querySelector(sel);
            if (el) break;
        }
        if (!el && c.fallback) el = document.querySelector(c.fallback);
        if (el) {
            const r    = el.getBoundingClientRect();
            const pad  = 10;
            // padY: per-step vertical inflation. Useful for very thin
            // elements (footer status bar at 22px) where the default 10px
            // padding still produces a band too small to read.
            const padY = (c.padY ?? 0) + pad;
            spot = {
                x: Math.max(0,     r.left   - pad),
                y: Math.max(0,     r.top    - padY),
                w: Math.min(W - 4, r.width  + pad  * 2),
                h: Math.min(H - 4, r.height + padY * 2),
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

        // Welcome / overview steps: center the card horizontally + vertically.
        if (s.welcome) {
            const left = Math.max(p, w / 2 - TW / 2);
            const top  = Math.max(p, h / 2 - TH / 2);
            return `left:${left}px;top:${top}px;`;
        }

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
        // Versioned tutorial flag: re-shows the tour after a release bump
        // when the user upgrades. Keeps onboarding fresh without nagging
        // users who finished it on this version.
        safeSetLSString('lucy_tutorial_done', LUCY_VERSION);
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
      {#if ready && !spot.welcome}
        <rect x={spot.x} y={spot.y} width={spot.w} height={spot.h}
              rx={spot.r} fill="black"/>
      {/if}
    </mask>
  </defs>
  <rect width="100%" height="100%" fill="rgba(2,6,12,0.88)" mask="url(#tut-hole)"/>
  {#if ready && !spot.welcome}
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
      {#if isLast}{isEN ? '✓ Done!' : '✓ ¡Listo!'}{:else}{isEN ? 'Next →' : 'Siguiente →'}{/if}
    </button>
  </div>
</div>
{/if}
{/if}

<style>
  /* ── SVG overlay ──────────────────────────────────────────────────────── */
  .tut-svg {
    position: fixed; top: 0; left: 0; z-index: var(--z-tutorial, 6000);
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
    position: fixed; z-index: calc(var(--z-tutorial, 6000) + 1);
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
