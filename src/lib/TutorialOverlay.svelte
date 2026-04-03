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
        // 1. Main chat terminal (the most important feature, center of the app)
        {
            sel: ['.chat-wrap.on .chat-area', '.chat-wrap.on', '.panel'],
            tip: 'left',
            view: 'terminal',
            tES: '💬 Terminal IA — El corazón de Lucy',
            tEN: '💬 AI Terminal — The heart of Lucy',
            dES: 'Escribe cualquier instrucción en <b>lenguaje natural</b> y Lucy genera, verifica y ejecuta el comando adecuado automáticamente.<br><br>Puedes pedir diagnósticos, ejecutar scripts, adjuntar logs o imágenes. Lucy explica cada resultado y sugiere los próximos pasos.',
            dEN: 'Type any instruction in <b>natural language</b> and Lucy automatically generates, verifies, and runs the right command.<br><br>Request diagnostics, run scripts, attach logs or images. Lucy explains each result and suggests next steps.',
        },
        // 2. Input bar — directly below the chat
        {
            sel: ['.ibar', '.igrp'],
            tip: 'top',
            view: 'terminal',
            tES: '⌨️ Barra de Entrada',
            tEN: '⌨️ Input Bar',
            dES: 'Aquí escribes tus instrucciones. Botones adicionales permiten:<br>• 📎 Adjuntar archivos, logs o imágenes<br>• 🎙️ Activar entrada por <b>voz</b> (dictado)<br>• Elegir dinámicamente el <b>modelo de IA</b> (Claude, GPT, Gemini o Local)',
            dEN: 'Type your instructions here. Additional buttons allow:<br>• 📎 Attach files, logs or images<br>• 🎙️ Activate <b>voice</b> input<br>• Dynamically pick the <b>AI Model</b> (Claude, GPT, Gemini or Local)',
        },
        // 2.b Chat Shortcuts
        {
            sel: ['.chips', '.ibar'],
            tip: 'top',
            view: 'terminal',
            tES: '⚡ Atajos de Mensaje (Chips)',
            tEN: '⚡ Message Shortcuts (Chips)',
            dES: 'Justo encima de la barra de chat hay atajos de mensaje que se envían directamente a Lucy.<br><br>💡 <b>Cómo crearlos:</b> Haz clic en el botón <b>＋</b> al final de la lista de atajos para agregar comandos o instrucciones que envíes frecuentemente a la IA.',
            dEN: 'Right above the chat input are quick message shortcuts that are sent directly to Lucy.<br><br>💡 <b>How to create:</b> Click the <b>＋</b> button at the end of the shortcut row to add commands or instructions you frequently send to the AI.',
        },
        // 3. Tabs — title bar, top of app
        {
            sel: ['#tabs-list', '.tabs-area', '.tb'],
            tip: 'bottom',
            view: 'terminal',
            tES: '📑 Pestañas de Sesión',
            tEN: '📑 Session Tabs',
            dES: 'Lucy permite <b>múltiples sesiones simultáneas</b>. Crea nuevas pestañas con el botón <b>+</b> y renómbralas con <b>doble clic</b> sobre el nombre.<br><br>💡 Lucy <b>renombra automáticamente</b> cada pestaña según la actividad ejecutada — sin necesidad de intervención manual.',
            dEN: 'Lucy supports <b>multiple simultaneous sessions</b>. Create new tabs with <b>+</b> and rename them by <b>double-clicking</b> the name.<br><br>💡 Lucy <b>auto-renames</b> each tab based on the current activity — no manual action needed.',
        },
        // 4. Dashboard — first item in sidebar Sistema section
        {
            sel: ['.sidebar .sb-it[title*="Dashboard"]', '.sidebar .sb-it[title*="ashboard"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'dashboard',
            tES: '📊 Dashboard — Métricas en Vivo',
            tEN: '📊 Dashboard — Live Metrics',
            dES: 'Vista de métricas en tiempo real: <b>CPU, RAM, disco y red</b> del equipo local o de cualquier host remoto conectado.<br><br>Incluye tabla de procesos activos, <b>gráficas sparkline</b> de historial y alertas proactivas configurables por umbral. Selecciona el host en el selector superior.',
            dEN: 'Real-time metrics: <b>CPU, RAM, disk and network</b> for the local machine or any connected remote host.<br><br>Includes active process table, <b>sparkline</b> history charts, and configurable threshold-based alerts. Select the host in the top selector.',
        },
        // 5. Log Viewer
        {
            sel: ['.sidebar .sb-it[title*="Log Viewer"]', '.sidebar .sb-it[title*="og View"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'logviewer',
            tES: '🗂️ Log Viewer',
            tEN: '🗂️ Log Viewer',
            dES: 'Lee y filtra logs del sistema en tiempo real, tanto locales como de hosts remotos. Soporta archivos <code>.log</code>, <code>.txt</code> y <code>.csv</code>.<br><br>Errores y advertencias se <b>colorean automáticamente</b>. Puedes preguntar a Lucy sobre cualquier entrada del log directamente en el chat.',
            dEN: 'Read and filter system logs in real-time, both local and from remote hosts. Supports <code>.log</code>, <code>.txt</code> and <code>.csv</code> files.<br><br>Errors and warnings are <b>automatically color-coded</b>. Ask Lucy about any log entry directly in the chat.',
        },
        // 6. NexShell — host catalogue & categories
        {
            sel: ['.ns-hosts-col', '.ns-view', '.ns-body'],
            fallback: '.sidebar .sb-it[title*="NexShell"]',
            tip: 'right',
            view: 'nexshell',
            tES: '🔌 NexShell — Infraestructura Remota',
            tEN: '🔌 NexShell — Remote Infrastructure',
            dES: '<b>NexShell</b> centraliza toda tu infraestructura remota. Desde aquí administras y conectas cualquier tipo de servidor:<br><br>• 🖥 <b>Shell</b> — servidores Linux (SSH) y Windows (WinRM)<br>• 🗄️ <b>Base de datos</b> — PostgreSQL 🐘, MySQL 🐬, MongoDB 🍃, Redis ⚡, MSSQL 🪟<br>• 🐳 <b>Contenedor (Docker)</b> — gestión de contenedores vía SSH<br>• ⎈ <b>Kubernetes</b> — control plane con <code>kubectl</code> asistido por Lucy<br>• 🌐 <b>Red</b> — dispositivos de red y monitoreo<br><br>Usa los filtros y el ordenamiento por <b>estado · nombre · tipo · actividad</b>. El panel de hosts se puede <b>colapsar</b> (◀) para maximizar el workspace.',
            dEN: '<b>NexShell</b> centralises all your remote infrastructure. Manage and connect to any server type:<br><br>• 🖥 <b>Shell</b> — Linux (SSH) and Windows (WinRM) servers<br>• 🗄️ <b>Database</b> — PostgreSQL 🐘, MySQL 🐬, MongoDB 🍃, Redis ⚡, MSSQL 🪟<br>• 🐳 <b>Container (Docker)</b> — container management via SSH<br>• ⎈ <b>Kubernetes</b> — control plane with Lucy-assisted <code>kubectl</code><br>• 🌐 <b>Network</b> — network devices and monitoring<br><br>Use filters and sort by <b>status · name · type · activity</b>. The hosts panel can be <b>collapsed</b> (◀) to maximise workspace.',
        },
        // 7. NexShell — embedded workspace + Lucy co-pilot
        {
            sel: ['.ns-workspace', '.ns-shell-wrap', '.ns-session-tabs'],
            fallback: '.sidebar .sb-it[title*="NexShell"]',
            tip: 'left',
            view: 'nexshell',
            tES: '🤖 NexShell — Shell con Co-piloto IA',
            tEN: '🤖 NexShell — AI Co-pilot Shell',
            dES: 'Al conectarte a un host la terminal se abre <b>inline</b> en el workspace.<br><br><b>Dos formas de interactuar:</b><br>• <code>&gt;_</code> <b>Comando directo</b> — escribe el comando, Lucy sugiere en tiempo real (ghost text ✨)<br>• <b>✨ Lucy</b> — interactúa en lenguaje natural con tu <b>modelo de IA</b> preferido configurado por host<br><br><b>Capacidades avanzadas:</b><br>• 🌿 Bootstrap automático al conectar<br>• ⏱ <b>Ctrl+Enter</b> — ejecuta en background<br>• 📡 <b>Broadcast</b> — comandos multi-host<br>• 📋 <b>Playbooks</b> — secuencias pregrabadas<br>• 📁 <b>Transferencias SCP</b><br>• 📊 <b>Log tail</b> y Exit codes en vivo',
            dEN: 'When connecting to a host, the terminal opens <b>inline</b> in the workspace.<br><br><b>Two interaction modes:</b><br>• <code>&gt;_</code> <b>Direct command</b> — type a command, Lucy suggests in real time (ghost text ✨)<br>• <b>✨ Lucy</b> — use natural language powered by your preferred <b>AI model</b> selection per host<br><br><b>Advanced capabilities:</b><br>• 🌿 Auto-bootstrap on connect<br>• ⏱ <b>Ctrl+Enter</b> — background execution<br>• 📡 <b>Broadcast</b> — multi-host commands<br>• 📋 <b>Playbooks</b> — preserved sequences<br>• 📁 <b>SCP file transfer</b><br>• 📊 <b>Log tail</b> and live Exit codes',
        },
        // 9. Direct Actions
        {
            sel: ['.sidebar [title*="Health"]', '.sidebar [title*="Salud"]', '.sidebar [title*="Flush DNS"]', '.sidebar [title*="Direct actions"]', '.sidebar [title*="Acciones directas"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '⚡ Acciones Directas',
            tEN: '⚡ Direct Actions',
            dES: 'Atajos de <b>un clic</b> para tareas de administración frecuentes. Cada acción se ejecuta en tu máquina <b>sin usar la IA</b>.<br><br>💡 <b>Cómo crearlas:</b> Usa el botón <b>+</b> junto al texto "Acciones directas" para agregar tus propios scripts de PowerShell con nombre e ícono personalizados.',
            dEN: '<b>One-click</b> shortcuts for frequent admin tasks. Each executes locally <b>without using the AI</b>.<br><br>💡 <b>How to create:</b> Use the <b>+</b> button next to "Direct actions" to add your own PowerShell scripts with a custom name and icon.',
        },
        // 10. Runbooks
        {
            sel: ['.sidebar [title*="New runbook"]', '.sidebar [title*="Nuevo runbook"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '📋 Runbooks — Automatización',
            tEN: '📋 Runbooks — Automation',
            dES: 'Crea <b>flujos de trabajo automatizados</b>. Cada runbook es una secuencia de comandos que Lucy ejecuta paso a paso.<br><br>💡 <b>Cómo crearlos:</b> Haz clic en el botón <b>+</b> junto al texto "RUNBOOKS" en la barra lateral superior. Rellena los comandos de cada paso, ideal para auditorías y mantenimientos.',
            dEN: 'Create <b>automated workflows</b>. Each runbook is a sequence of commands Lucy runs step-by-step.<br><br>💡 <b>How to create:</b> Click the <b>+</b> button next to "RUNBOOKS" in the upper sidebar. Fill in the commands for each step, ideal for audits and maintenance.',
        },
        // 11. Status bar — very bottom
        {
            sel: ['.sbar'],
            tip: 'top',
            view: 'terminal',
            tES: '📡 Barra de Estado',
            tEN: '📡 Status Bar',
            dES: 'Panel de estado en tiempo real:<br>• 🟢 <b>Keyring seguro</b> — credenciales protegidas en Windows Credential Manager<br>• <b>Host activo</b> — servidor remoto seleccionado<br>• 🟡 <b>Procesando…</b> — Lucy ejecutando una tarea (con timer)<br>• Versión de Lucy, alertas de seguridad de red y host activo',
            dEN: 'Real-time status panel:<br>• 🟢 <b>Secure keyring</b> — credentials in Windows Credential Manager<br>• <b>Active host</b> — selected remote server<br>• 🟡 <b>Processing…</b> — Lucy running a task (with timer)<br>• Lucy OS version, network security alerts and active host',
        },
        // 12. Win controls — top right (theme, focus, Ctrl+P)
        {
            sel: ['.win-controls'],
            tip: 'bottom',
            view: 'terminal',
            tES: '🌗 Controles y Accesos Rápidos',
            tEN: '🌗 Controls & Quick Access',
            dES: '• <b>☀️/🌙</b> Alterna entre tema claro y oscuro<br>• <b>⊟/⊞</b> Modo focus — oculta el panel lateral (<kbd>Ctrl+M</kbd>)<br><br>💡 Presiona <kbd>Ctrl+P</kbd> en cualquier momento para la <b>paleta de comandos</b> con acceso instantáneo a todas las funciones de Lucy.',
            dEN: '• <b>☀️/🌙</b> Toggle light / dark theme<br>• <b>⊟/⊞</b> Focus mode — hides sidebar (<kbd>Ctrl+M</kbd>)<br><br>💡 Press <kbd>Ctrl+P</kbd> at any time for the <b>command palette</b> with instant access to all Lucy features.',
        },
        // 13. Sidebar overview
        {
            sel: ['.sidebar'],
            tip: 'right',
            view: 'terminal',
            tES: '🧭 Panel Lateral — Navegación',
            tEN: '🧭 Sidebar — Navigation',
            dES: 'Centro de control de Lucy. Accede a todas las vistas desde aquí.<br><br>Colapsa el panel con el botón <b>‹</b> para maximizar el espacio. El <b>modo focus</b> (<kbd>Ctrl+M</kbd>) lo oculta completamente. Arrastra el borde derecho para ajustar el ancho a tu gusto.',
            dEN: 'Lucy\'s control center. Access all views from here.<br><br>Collapse with the <b>‹</b> button to maximize workspace. <b>Focus mode</b> (<kbd>Ctrl+M</kbd>) hides it completely. Drag the right edge to adjust the width.',
        },
        // 14. Bottom options — last step, points to Tutorial/About buttons
        {
            sel: ['.sidebar [title*="Tutorial"]', '.sidebar [title*="Acerca"]', '.sidebar [title*="About"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '⚙️ Registros y Preferencias',
            tEN: '⚙️ Logs & Preferences',
            dES: 'En la parte inferior de la barra lateral:<br>• 📋 <b>Comandos / Audit Log / Exportar Log</b> — registro de toda la actividad<br>• 🎓 <b>Ver Tutorial</b> — relanza este tour en cualquier momento<br>• ℹ️ <b>Acerca de Lucy</b> — versión y modelo activo<br>• 🔑 <b>Cambiar API Key</b> — actualiza tu clave de Gemini',
            dEN: 'At the bottom of the sidebar:<br>• 📋 <b>Commands / Audit Log / Export</b> — full activity log<br>• 🎓 <b>Show Tutorial</b> — relaunch this tour anytime<br>• ℹ️ <b>About Lucy</b> — version and active model<br>• 🔑 <b>Change API Key</b> — update your Gemini key',
        },
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
