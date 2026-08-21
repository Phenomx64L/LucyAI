<!-- ── TutorialOverlay.svelte ────────────────────────────────────────────────
     Guided spotlight tour — recorre la UI de arriba hacia abajo.
     SVG mask crea un "hole" sobre cada módulo; tooltip flotante con
     posicionamiento adaptativo que no sale del viewport.
     Props  : show (bindable), isEN
     Events : done
──────────────────────────────────────────────────────────────────────────── -->
<script>
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { createEventDispatcher, tick, onMount, onDestroy } from 'svelte';
    import { safeSetLSString } from '$lib/safe-ls';

    export let show = false;
    export let isEN = false;

    const dispatch = createEventDispatcher();

    // Bumped per release. Keep in sync with package.json + Cargo.toml.
    // v1.7.21 — was a hardcoded '1.6.4'. Every patch bump invalidated
    // the saved flag (TutorialOverlay wrote '1.6.4' on close, +page.svelte
    // compared against tauri.conf.json's appVersion '1.7.x', never matched
    // → the tutorial opened on every launch). Now received as prop so the
    // host always passes the real running version.
    // v1.7.67 — default bumped from '1.7.0' to '1.7.66' so a developer
    // who instantiates the overlay in isolation (e.g. Storybook, test
    // harness) doesn't see a stale version label.
    export let currentVersion = '1.7.66';
    // v1.7.165 — BUG FIX: this was `$: LUCY_VERSION = currentVersion`, a reactive
    // statement that runs AFTER the synchronous `const STEPS = [...]` below is
    // built. So every `v${LUCY_VERSION}` in STEPS interpolated `undefined` →
    // "Lucy vundefined". Props are bound before the body runs, so a plain const
    // reads the real running version in time for STEPS.
    const LUCY_VERSION = currentVersion || '1.7';

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
            tES: `✦ Bienvenido a Lucy v${LUCY_VERSION} — Operations Console`,
            tEN: `✦ Welcome to Lucy v${LUCY_VERSION} — Operations Console`,
            dES: `Hola Iván — Lucy v${LUCY_VERSION} es una <b>consola operacional</b> de administración de sistemas: memoria persistente, ejecución local y remota (SSH/WinRM con shell streaming) y su propia identidad visual.<br><br>` +
                `<b>🛰 Operations Console UI</b><br>` +
                `• <b>Mission Strip</b> arriba siempre visible — ● local · ⚯ hosts · ⚠ alertas · ⊕ guard · HH:MM · postura 5-dot.<br>` +
                `• <b>Tabs por propósito</b> — incidente rojo · ejecutando violeta · investigación ámbar · referencia azul · chat verde.<br>` +
                `• <b>Code blocks como terminal recordings</b> — traffic lights, hostname, engine glyph (⚡PS · ▶cmd · $bash), timestamp, exit code.<br>` +
                `• <b>Sidebar con jerarquía</b> — rail de color por sección: Sistema verde · Runbooks ámbar · Acciones violeta · Registros azul.<br>` +
                `• <b>Evidence pills</b> — citas inline color-coded por origen: memoria cyan · file verde · URL azul · tool ámbar.<br>` +
                `• <b>Composer ops</b> — prompt λ, dot grid en focus, slash-command ámbar, caret tipo bloque.<br>` +
                `• <b>Self-Diagnóstico con repair de un click</b> — Diagnóstico te muestra issues + botón "🔧 Reparar" para los conocidos.<br><br>` +
                `<b>🤖 Intelligence</b><br>` +
                `• <b>Grounding</b> — cada memoria tiene confidence (0..1) driven by evidence; contradicciones la bajan, refuerzos la suben.<br>` +
                `• <b>Skill presets curados</b> (v1.6.1) — 18+ presets ECC listos para usar (cost-aware, security-review, hypothesis-driven-debug…).<br>` +
                `• <b>Multi-intent + RULE 0b</b> (v1.7.49-50) — prompts tipo "genera un reporte detallado del estado de mi maquina, depositalo en el escritorio" siempre se convierten en plan multi-paso con writefile real.<br>` +
                `• <b>Centralised model catalog + tier health</b> (v1.7.0-v1.7.5) — un único source of truth para todos los modelos soportados, con health check al boot.<br><br>` +
                `<b>⚡ Streaming</b><br>` +
                `• <b>morphdom DOM diffing</b> (v1.7.56) — el texto fluye sin parpadeo. Indistinguible de ChatGPT/Claude.ai.<br>` +
                `• <b>Aura Gemini-style</b> (v1.7.57) — text-shadow pulsante mientras Lucy escribe + fade-in por token.<br>` +
                `• <b>Open-tag placeholder</b> (v1.7.47) — "◌ Lucy está razonando…" en lugar de bubble vacío cuando emite &lt;THOUGHT&gt;.<br><br>` +
                `<b>🔧 Performance</b><br>` +
                `• <b>GPU vendor hints</b> — Lucy se asigna a la dGPU en laptops Optimus/PowerXpress automáticamente.<br>` +
                `• <b>Idle saver</b> — todas las animaciones se pausan tras 8s sin input. GPU idle ~1-3%.<br><br>` +
                `<b>💾 Reliability</b><br>` +
                `• <b>persistirNow</b> — cerrar Lucy inmediatamente después de cualquier edit (close tab, rename) nunca pierde el cambio.<br>` +
                `• <b>One-click DB repair</b> — backfill + REINDEX + verify desde el panel Diagnóstico sin SQL ni DB Browser.<br><br>` +
                `Vamos a recorrer la nueva interfaz — empezamos por la franja superior, lo más distintivo.`,
            dEN: `Hi Iván — Lucy v${LUCY_VERSION} is a full SysAdmin <b>operations console</b>: persistent memory, local and remote execution (SSH/WinRM with streaming shell) and her own visual identity.<br><br>` +
                `<b>🛰 Operations Console UI</b><br>` +
                `• <b>Mission Strip</b> always-on top band — ● local · ⚯ hosts · ⚠ alerts · ⊕ guard · HH:MM · 5-dot posture.<br>` +
                `• <b>Per-tab purpose tint</b> — incident red · executing violet · investigation amber · reference blue · chat green.<br>` +
                `• <b>Terminal-recording code blocks</b> — traffic lights, hostname, engine glyph (⚡PS · ▶cmd · $bash), timestamp, exit code.<br>` +
                `• <b>Sidebar category rails</b> — left colour rail per section: System green · Runbooks amber · Actions violet · Records blue.<br>` +
                `• <b>Evidence pills</b> — inline citations colour-coded by origin: memory cyan · file green · URL blue · tool amber.<br>` +
                `• <b>Composer ops aesthetic</b> — λ prompt, focus-only dot grid, amber slash-command mode, block-shape caret.<br>` +
                `• <b>Self-diagnostics with one-click repair</b> — Diagnostics panel ships repair buttons for known issues.<br><br>` +
                `<b>🤖 Intelligence</b><br>` +
                `• <b>Grounding</b> — every memory carries a confidence score (0..1) driven by evidence accumulation.<br>` +
                `• <b>Curated skill presets</b> (v1.6.1) — 18+ ready-to-use ECC presets (cost-aware, security-review, hypothesis-driven-debug…).<br>` +
                `• <b>Multi-intent + RULE 0b</b> (v1.7.49-50) — prompts like "generate a detailed report of my machine state, save to desktop" always become a multi-step plan with real writefile.<br>` +
                `• <b>Centralised model catalog + tier health</b> (v1.7.0-v1.7.5) — single source of truth across providers, boot-time health check.<br><br>` +
                `<b>⚡ Streaming</b><br>` +
                `• <b>morphdom DOM diffing</b> (v1.7.56) — text flows without flicker. Indistinguishable from ChatGPT/Claude.ai.<br>` +
                `• <b>Gemini-style aura</b> (v1.7.57) — soft text-shadow pulse while Lucy writes + per-token fade-in.<br>` +
                `• <b>Open-tag placeholder</b> (v1.7.47) — "◌ Lucy is reasoning…" instead of a blank bubble during &lt;THOUGHT&gt;.<br><br>` +
                `<b>🔧 Performance</b><br>` +
                `• <b>GPU vendor hints</b> — auto-binds to the dGPU on Optimus / PowerXpress laptops.<br>` +
                `• <b>Idle saver</b> — every infinite animation pauses after 8 s of no input. Idle GPU drops to ~1-3%.<br><br>` +
                `<b>💾 Reliability</b><br>` +
                `• <b>persistirNow</b> — closing Lucy right after any structural edit (close tab, rename) never loses the change.<br>` +
                `• <b>One-click DB repair</b> — backfill + REINDEX + verify from Diagnostics, no SQL or DB Browser needed.<br><br>` +
                `Let's walk through the new interface — starting with the top band, the most distinctive piece.`,
        },
        {
            // v1.7.67 — New step for the Mission Strip (Direction A1, v1.7.58).
            // Always above the tab strip, always visible, communicates four
            // peripheral signals an IT pro tracks without switching tabs.
            sel: ['.mission-strip'],
            fallback: '.mission-strip',
            tip: 'bottom',
            view: 'terminal',
            tES: '🛰 Mission Strip — Pulse Operacional',
            tEN: '🛰 Mission Strip — Operational Pulse',
            dES: `La franja superior es tu <b>cockpit siempre visible</b>. Léela de izquierda a derecha:<br><br>` +
                `• <b>● HOSTNAME</b> — tu máquina; el dot latido cada 3.6 s indica que Lucy está viva.<br>` +
                `• <b>⚯ N/M hosts</b> — hosts remotos en línea (solo aparece si tienes hosts configurados).<br>` +
                `• <b>⚠ N alertas</b> — incidentes activos. Click → Dashboard.<br>` +
                `• <b>⊕ guard</b> — skill de seguridad activo, o "limpio".<br>` +
                `• <b>HH:MM</b> — hora local, actualiza cada minuto.<br>` +
                `• <b>●●●○○ postura</b> — 5 dots: calm → vigilant → suspicious → alarmed → panic.<br><br>` +
                `Cualquier chip pasa a ámbar o rojo solo cuando hay algo que merece tu atención. No tienes que cambiar de pestaña para enterarte. <b>Click en cualquier chip → te lleva a la vista relevante.</b>`,
            dEN: `The top band is your <b>always-visible cockpit</b>. Read it left to right:<br><br>` +
                `• <b>● HOSTNAME</b> — your machine; the dot heartbeats every 3.6 s to signal Lucy is alive.<br>` +
                `• <b>⚯ N/M hosts</b> — remote hosts online (only shows when hosts are configured).<br>` +
                `• <b>⚠ N alerts</b> — active incidents. Click → Dashboard.<br>` +
                `• <b>⊕ guard</b> — active security skill, or "clean".<br>` +
                `• <b>HH:MM</b> — local time, ticks once per minute.<br>` +
                `• <b>●●●○○ posture</b> — 5 dots: calm → vigilant → suspicious → alarmed → panic.<br><br>` +
                `Any chip turns amber or red only when something actually needs attention. You don't have to switch tabs to find out. <b>Click any chip → opens the relevant view.</b>`,
        },
        {
            sel: ['.chat-wrap.on .chat-area', '.chat-wrap.on', '.panel'],
            tip: 'left',
            view: 'terminal',
            tES: '↗ Terminal IA — Bucle Agéntico',
            tEN: '↗ AI Terminal — Agentic Loop',
            dES: 'El corazón de Lucy. Escribe tu instrucción y la IA no solo te contestará, sino que <b>evaluará, verificará y ejecutará</b> automáticamente hasta completar la tarea — <b>local</b> (PowerShell) o <b>remoto</b> en tus hosts SSH/WinRM. Incluye <b>PLAN/VERIFY/ROLLBACK</b>: para cambios riesgosos Lucy propone un plan, verifica el resultado y revierte si falla, siempre tras tu confirmación. <br><br>Autocompletado inline de flags: empieza a escribir <code>rm -</code> o <code>find . -</code> (Tab/Enter); los destructivos van al final con ⚠.',
            dEN: 'The core of Lucy. Type an instruction and the AI will not only reply, but <b>evaluate, verify and execute</b> automatically until the task is complete — <b>locally</b> (PowerShell) or <b>remotely</b> on your SSH/WinRM hosts. Includes <b>PLAN/VERIFY/ROLLBACK</b>: for risky changes Lucy proposes a plan, verifies the outcome, and rolls back if it fails, always after your confirmation. <br><br>Inline flag autocomplete: start typing <code>rm -</code> or <code>find . -</code> (Tab/Enter); destructive flags go last with ⚠.',
        },
        {
            sel: ['.sidebar .sb-it[title*="NexShell"]', '.sidebar .sb-it[title*="exShell"]'],
            fallback: '.sidebar .sb-it[title*="NexShell"]',
            tip: 'right',
            view: 'nexshell',
            tES: '⊟ NexShell — Infraestructura',
            tEN: '⊟ NexShell — Infrastructure',
            dES: 'Conecta servidores por SSH, WinRM, Bases de Datos o clústeres Kubernetes de forma nativa, con <b>Ghost Text en tiempo real</b> y <b>host preflight</b> (verifica conectividad TCP y falla rápido si el host no responde). Cada comando del log expone <b>acciones</b> (copiar · re-ejecutar · explícame), un <b>chip de fix proactivo</b> cuando algo falla (lock de rpm, permisos, puerto en uso…), y puedes lanzar <b>playbooks multi-fase</b> con el botón 📚 — o <code>/playbooks</code> para correrlos en tu máquina local.',
            dEN: 'Native SSH, WinRM, Database, or Kubernetes connectivity, with real-time <b>Ghost Text</b> and <b>host preflight</b> (tests TCP reachability and fails fast when a host is down). Every command in the log exposes <b>actions</b> (copy · re-run · explain), a proactive <b>fix chip</b> when something fails (rpm lock, perms, port in use…), and you can run <b>multi-phase playbooks</b> from the 📚 button — or <code>/playbooks</code> to run them on your local machine.',
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
            sel: ['.sidebar .sb-it[title*="Compliance"]', '.sidebar .sb-it[title*="ompliance"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'compliance',
            tES: '⬡ Compliance — CIS Benchmarks',
            tEN: '⬡ Compliance — CIS Benchmarks',
            dES: 'Evalúa cada host contra los <b>CIS Benchmarks</b> (Center for Internet Security) para Windows Server, Ubuntu y RHEL. Cada control muestra <b>PASS / FAIL / N/A</b> con su evidencia y el comando de remediación. Exporta reportes PDF para auditores.',
            dEN: 'Evaluate each host against <b>CIS Benchmarks</b> (Center for Internet Security) for Windows Server, Ubuntu and RHEL. Each control shows <b>PASS / FAIL / N/A</b> with its evidence and remediation command. Export PDF reports for auditors.',
        },
        {
            sel: ['.sidebar .sb-it[title*="Memory"]', '.sidebar .sb-it[title*="emoria"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'memory',
            tES: '◊ Explorador de Memoria',
            tEN: '◊ Memory Browser',
            dES: 'Visualiza todo lo que Lucy ha aprendido: hechos sobre tu infraestructura, comandos enseñados, preferencias, principios y <b>healing-patterns</b> auto-cristalizados desde incidentes resueltos. Filtra por tag, edita o elimina entradas. Persiste en SQLite con búsqueda de texto completo.',
            dEN: 'Browse everything Lucy has learned: infrastructure facts, taught commands, preferences, principles and <b>healing-patterns</b> auto-crystallized from resolved incidents. Filter by tag, edit or delete entries. Persisted in SQLite with full-text search.',
        },
        {
            sel: ['.sidebar .sb-it[title*="Capacity"]', '.sidebar .sb-it[title*="apacidad"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'capacity',
            tES: '↗ Capacity Planning',
            tEN: '↗ Capacity Planning',
            dES: 'Proyección de uso de recursos basada en tendencias históricas. Lucy estima cuánto te queda de disco, RAM o CPU al ritmo actual de crecimiento, y avisa cuando un host se acerca al límite. Útil para presupuestar hardware con datos en mano.',
            dEN: 'Resource usage projection based on historical trends. Lucy estimates how much disk, RAM or CPU you have left at the current growth rate, and warns when a host approaches its limit. Useful for hardware budgeting with hard data.',
        },
        {
            sel: ['.sidebar .sb-it[title*="Diagn"]', '.sidebar .sb-it[title*="iagnostic"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'diagnostics',
            tES: '⚕ Self-Diagnostics',
            tEN: '⚕ Self-Diagnostics',
            dES: 'Lucy se inspecciona a sí misma: estado del pool SQLite, salud de la cola de embeddings, MCP servers conectados, integridad del audit chain, espacio de la DB local, y health checks de cada feature Frontier. Si algo falla en Lucy, esta vista lo muestra primero.',
            dEN: 'Lucy inspects herself: SQLite pool health, embeddings queue, connected MCP servers, audit chain integrity, local DB size, and health checks for every Frontier feature. If anything in Lucy is wrong, this view shows it first.',
        },
        {
            sel: ['.sidebar .sb-it[title*="permission"]', '.sidebar .sb-it[title*="ermiso"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '🛡 Permisos — Reglas allow/block/ask',
            tEN: '🛡 Permissions — allow/block/ask rules',
            dES: 'Define <b>reglas regex</b> que decide Lucy antes de ejecutar cualquier comando o tocar cualquier ruta: <code>allow</code> (sin preguntar), <code>block</code> (rechazar), <code>ask</code> (HITL obligatorio). Cada regla tiene scope (comando, path, host) y registro de quién la creó. Es la primera barrera ante errores accidentales y la base del HITL universal.',
            dEN: 'Define <b>regex rules</b> Lucy checks before running any command or touching any path: <code>allow</code> (no prompt), <code>block</code> (reject), <code>ask</code> (HITL required). Each rule has a scope (command, path, host) and an author log. First line of defense against accidental damage and the foundation of universal HITL.',
        },
        {
            sel: ['.sidebar .sb-it[title*="rinciple"]', '.sidebar .sb-it[title*="rincipi"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '◈ Principios — Reglas que Lucy sigue',
            tEN: '◈ Principles — Rules Lucy follows',
            dES: 'Reglas de alto nivel que <b>siempre acompañan</b> al prompt del agente. Ejemplos: "Nunca tocar producción sin confirmación", "Si hay error de DNS, intenta resolver con 8.8.8.8 antes de escalarlo", "Prefiere PowerShell nativo sobre choco install". Son las máximas que Lucy nunca olvida.',
            dEN: 'High-level rules that <b>always travel</b> with the agent prompt. Examples: "Never touch production without confirmation", "On DNS errors, try resolving with 8.8.8.8 before escalating", "Prefer native PowerShell over choco install". The maxims Lucy never forgets.',
        },
        {
            sel: ['.sidebar .sb-it[title*="Scheduled"]', '.sidebar .sb-it[title*="rogramad"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '⏰ Programadas — Tareas recurrentes',
            tEN: '⏰ Scheduled tasks',
            dES: 'Lanza skills, runbooks o comandos en <b>cron</b> o intervalos: "verifica salud de servidores cada lunes 7am", "rota logs diarios", "scan de threats cada hora". Lucy registra cada ejecución en el audit trail. Los disparadores también pueden ser eventos (anomaly detected, incident opened).',
            dEN: 'Launch skills, runbooks or commands on <b>cron</b> or intervals: "check server health every Monday 7am", "rotate logs daily", "threat scan every hour". Lucy logs each run in the audit trail. Triggers can also be events (anomaly detected, incident opened).',
        },
        {
            sel: ['.sidebar .sb-it[title*="ub-Agent"]', '.sidebar .sb-it[title*="ub-Agent"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '⊞ Sub-Agentes — Fork / Wait',
            tEN: '⊞ Sub-Agents — Fork / Wait',
            dES: 'Lucy puede <b>lanzar sub-tareas en paralelo</b> con <code>&lt;TOOL&gt;fork_task&lt;/TOOL&gt;</code> y esperar sus resultados con <code>&lt;TOOL&gt;wait_task&lt;/TOOL&gt;</code>. Útil para investigar 4 hosts simultáneamente o probar 3 hipótesis en paralelo. Este panel muestra forks activos y su output en tiempo real.',
            dEN: 'Lucy can <b>fan out parallel sub-tasks</b> with <code>&lt;TOOL&gt;fork_task&lt;/TOOL&gt;</code> and gather their results via <code>&lt;TOOL&gt;wait_task&lt;/TOOL&gt;</code>. Useful for investigating 4 hosts simultaneously or testing 3 hypotheses in parallel. This panel shows active forks and their live output.',
        },
        {
            sel: ['.sidebar .sb-it[title*="PDF"]', '.sidebar .sb-it[title*="ngest manual"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '📕 PDF Intelligence',
            tEN: '📕 PDF Intelligence',
            dES: 'Arrastra manuales, RFPs, runbooks de proveedores o documentación de hardware en PDF — Lucy los ingiere, los <b>indexa por chunks</b>, y los expone como herramienta de búsqueda semántica (<code>&lt;TOOL&gt;pdf_search:query&lt;/TOOL&gt;</code>). Convierte tus PDF en conocimiento accionable.',
            dEN: 'Drop vendor manuals, RFPs, runbooks or hardware docs as PDF — Lucy ingests, <b>chunks-indexes</b>, and exposes them as a semantic search tool (<code>&lt;TOOL&gt;pdf_search:query&lt;/TOOL&gt;</code>). Turn your PDFs into actionable knowledge.',
        },
        {
            sel: ['.sidebar [title*="Settings"]', '.sidebar [title*="Configurac"]'],
            fallback: '.sidebar',
            tip: 'right',
            view: 'terminal',
            tES: '⚙ Configuración — Hub central',
            tEN: '⚙ Settings — Central hub',
            dES: 'Abre el modal de Configuración (icono ⚙ en la sidebar o <code>Ctrl+P</code> → "Configuración"). Es el hub donde vives entre sesiones: <b>Proveedores LLM</b>, <b>Privacidad</b>, <b>Temas</b>, <b>Datos</b>, <b>MCP</b>, <b>Verificador</b>, <b>Perfiles</b>, <b>Permisos</b>, <b>Runbooks</b>. En los siguientes pasos recorreremos cada sub-módulo.',
            dEN: 'Open the Settings modal (⚙ icon in the sidebar or <code>Ctrl+P</code> → "Settings"). The hub you live in between sessions: <b>LLM Providers</b>, <b>Privacy</b>, <b>Themes</b>, <b>Data</b>, <b>MCP</b>, <b>Verifier</b>, <b>Profiles</b>, <b>Permissions</b>, <b>Runbooks</b>. The next steps walk through every sub-module.',
        },
        {
            sel: ['body'], fallback: 'body', tip: 'top', view: 'terminal', welcome: true,
            tES: '⚙ Configuración · Proveedores LLM & API Keys',
            tEN: '⚙ Settings · LLM Providers & API Keys',
            dES: `<b>Sub-módulo: Proveedores</b><br><br>` +
                `Lucy es <b>multi-LLM</b> — puedes mezclar Claude (Anthropic), Gemini (Google), GPT (OpenAI), Ollama local y NVIDIA NIM en una sola sesión.<br><br>` +
                `• <b>Configurar Proveedores</b> abre un modal donde pegas la API key de cada proveedor. Las claves se guardan en <b>Windows Credential Manager</b> (keyring), nunca en localStorage.<br>` +
                `• <b>Probar conexión</b> valida la key antes de guardar.<br>` +
                `• <b>Modelo activo</b> se ve en el StatusBar inferior. Puedes cambiarlo con <code>/model &lt;nombre&gt;</code> (matching parcial: "sonnet", "qwen", "flash").<br>` +
                `• <b>Effort suffix</b>: añade <code>::low|medium|high|xhigh|max</code> al nombre del modelo para controlar reasoning/tokens.<br>` +
                `• <b>Tavily API key</b> (opcional) — habilita búsqueda web premium con resultados extraíbles. Si no la configuras, Lucy cae a DuckDuckGo lite.<br><br>` +
                `<i>Tip</i>: la key value NUNCA cruza el IPC boundary del backend — el frontend solo recibe un boolean de status.`,
            dEN: `<b>Sub-module: Providers</b><br><br>` +
                `Lucy is <b>multi-LLM</b> — mix Claude (Anthropic), Gemini (Google), GPT (OpenAI), local Ollama and NVIDIA NIM in the same session.<br><br>` +
                `• <b>Configure Providers</b> opens a modal to paste each provider's API key. Keys are stored in <b>Windows Credential Manager</b> (keyring), never in localStorage.<br>` +
                `• <b>Test connection</b> validates the key before saving.<br>` +
                `• <b>Active model</b> shows in the bottom StatusBar. Switch with <code>/model &lt;name&gt;</code> (partial match: "sonnet", "qwen", "flash").<br>` +
                `• <b>Effort suffix</b>: append <code>::low|medium|high|xhigh|max</code> to the model name to control reasoning/tokens.<br>` +
                `• <b>Tavily API key</b> (optional) — enables premium web search with extractable results. If absent, Lucy falls back to DuckDuckGo lite.<br><br>` +
                `<i>Tip</i>: the key value NEVER crosses the IPC boundary from the backend — the frontend only receives a status boolean.`,
        },
        {
            sel: ['body'], fallback: 'body', tip: 'top', view: 'terminal', welcome: true,
            tES: '⚙ Configuración · Privacidad, Smart-Router & Economy',
            tEN: '⚙ Settings · Privacy, Smart-Router & Economy',
            dES: `<b>Sub-módulo: Privacidad & Routing</b><br><br>` +
                `• <b>Privacy mode</b> — fuerza a Lucy a usar solo modelos <b>locales</b> (Ollama / NVIDIA NIM on-prem). Bloquea cualquier llamada saliente a Anthropic/Google/OpenAI. Útil para datos regulados (HIPAA, datos clasificados).<br>` +
                `• <b>Smart-Routing</b> — Lucy analiza tu prompt y elige el modelo más barato capaz de resolverlo. Para "muestra ls" usa un modelo barato; para "diagnostica este crash dump" escala a un modelo grande. Ahorra ~40-60% en gasto típico.<br>` +
                `• <b>Economy mode</b> — caps duros: limita tokens por turno y prefiere caching agresivo. Ideal cuando estás cerca del budget mensual.<br>` +
                `• <code>/route</code> — explica POR QUÉ el smart-router eligió el modelo que eligió en el último turno.<br>` +
                `• <code>/smart-router on|off</code> y <code>/privacy on|off</code> son alternativas vía slash command.<br><br>` +
                `<b>Budget tracking</b>: el StatusBar muestra cache-hit % y gasto acumulado del mes. <b>Reset mensual</b> automático el día 1.`,
            dEN: `<b>Sub-module: Privacy & Routing</b><br><br>` +
                `• <b>Privacy mode</b> — forces Lucy to use only <b>local</b> models (Ollama / on-prem NVIDIA NIM). Blocks any outbound call to Anthropic/Google/OpenAI. Useful for regulated data (HIPAA, classified).<br>` +
                `• <b>Smart-Routing</b> — Lucy analyzes your prompt and picks the cheapest model that can solve it. "show ls" gets a cheap model; "diagnose this crash dump" escalates to a large model. Typical savings: ~40-60%.<br>` +
                `• <b>Economy mode</b> — hard caps: limits tokens per turn and prefers aggressive caching. Ideal near monthly budget cap.<br>` +
                `• <code>/route</code> — explains WHY the smart-router picked the model it picked last turn.<br>` +
                `• <code>/smart-router on|off</code> and <code>/privacy on|off</code> are slash-command alternatives.<br><br>` +
                `<b>Budget tracking</b>: StatusBar shows cache-hit % and month-to-date spend. Auto-reset on day 1.`,
        },
        {
            sel: ['body'], fallback: 'body', tip: 'top', view: 'terminal', welcome: true,
            tES: '⚙ Configuración · Temas personalizados (JSON)',
            tEN: '⚙ Settings · Custom themes (JSON)',
            dES: `<b>Sub-módulo: Apariencia</b><br><br>` +
                `Temas built-in: <code>default · ocean · hacker · sunset · forest · twilight · mocha · graphite</code>. Cambia con <code>/theme &lt;nombre&gt;</code> o desde el modal.<br><br>` +
                `<b>Custom theme JSON</b> — pega un JSON con exactamente <b>9 variables CSS</b> whitelistadas:<br>` +
                `<code>--bg</code>, <code>--bg2</code>, <code>--bg3</code>, <code>--txt</code>, <code>--txt2</code>, <code>--txt3</code>, <code>--acc</code>, <code>--bdr</code>, <code>--bdr2</code>.<br><br>` +
                `Cualquier variable fuera de ese whitelist se rechaza — no se permite inyección arbitraria de CSS. Esto protege contra XSS y mantiene la UI consistente.<br><br>` +
                `<b>Circadian theme</b>: los acentos se enfrían suavemente del día a la noche (hue shift basado en hora local). Se aplica encima del tema base.`,
            dEN: `<b>Sub-module: Appearance</b><br><br>` +
                `Built-in themes: <code>default · ocean · hacker · sunset · forest · twilight · mocha · graphite</code>. Switch with <code>/theme &lt;name&gt;</code> or from the modal.<br><br>` +
                `<b>Custom theme JSON</b> — paste a JSON with exactly <b>9 whitelisted CSS variables</b>:<br>` +
                `<code>--bg</code>, <code>--bg2</code>, <code>--bg3</code>, <code>--txt</code>, <code>--txt2</code>, <code>--txt3</code>, <code>--acc</code>, <code>--bdr</code>, <code>--bdr2</code>.<br><br>` +
                `Any variable outside that whitelist is rejected — no arbitrary CSS injection allowed. This protects against XSS and keeps the UI consistent.<br><br>` +
                `<b>Circadian theme</b>: accents subtly cool from day to night (hue shift based on local time). Applied on top of the base theme.`,
        },
        {
            sel: ['body'], fallback: 'body', tip: 'top', view: 'terminal', welcome: true,
            tES: '⚙ Configuración · Datos (Backup, Restore, Support Bundle)',
            tEN: '⚙ Settings · Data (Backup, Restore, Support Bundle)',
            dES: `<b>Sub-módulo: Datos & Soporte</b><br><br>` +
                `<b>📦 DB Backup</b> — copia atómica de toda la base SQLite (memorias, audit chain, incidents, snapshots, embeddings) usando <code>VACUUM INTO</code>. Genera un archivo <code>.lucydb</code> en la ruta que elijas (file picker nativo). Es atómico: no produce archivos corruptos aunque crashee a la mitad.<br><br>` +
                `<b>♻ DB Restore</b> — carga un <code>.lucydb</code>. Antes de sobrescribir crea un <b>safety backup</b> de tu DB actual. La restauración valida que el archivo contenga las tablas marker (<code>agent_memories</code> + <code>audit_chain</code>) — rechaza archivos que no sean de Lucy.<br><br>` +
                `<b>📤 Support Bundle</b> — exporta una carpeta con manifest, audit CSV, incidents JSON, system snapshot, token usage CSV y diagnostics. Adjúntala a un ticket de soporte. <b>NUNCA</b> incluye API keys ni contenido completo de memorias.<br><br>` +
                `<i>Recomendación</i>: backup semanal a un disco externo + bundle solo si reportas un bug.`,
            dEN: `<b>Sub-module: Data & Support</b><br><br>` +
                `<b>📦 DB Backup</b> — atomic copy of the entire SQLite DB (memories, audit chain, incidents, snapshots, embeddings) using <code>VACUUM INTO</code>. Writes a <code>.lucydb</code> file at the path you pick (native file picker). Atomic: no corrupt files even if it crashes mid-flight.<br><br>` +
                `<b>♻ DB Restore</b> — loads a <code>.lucydb</code>. Before overwriting, creates a <b>safety backup</b> of your current DB. Restore validates the file contains marker tables (<code>agent_memories</code> + <code>audit_chain</code>) — rejects non-Lucy files.<br><br>` +
                `<b>📤 Support Bundle</b> — exports a folder with manifest, audit CSV, incidents JSON, system snapshot, token usage CSV and diagnostics. Attach to a support ticket. <b>NEVER</b> includes API keys or full memory contents.<br><br>` +
                `<i>Recommendation</i>: weekly backup to external disk + bundle only when reporting a bug.`,
        },
        {
            sel: ['body'], fallback: 'body', tip: 'top', view: 'terminal', welcome: true,
            tES: '⚙ Configuración · MCP — Model Context Protocol',
            tEN: '⚙ Settings · MCP — Model Context Protocol',
            dES: `<b>Sub-módulo: MCP — cómo extender Lucy hoy</b><br><br>` +
                `<b>MCP (Model Context Protocol)</b> es el estándar abierto de Anthropic para que Lucy hable con <b>herramientas externas</b> (filesystem, GitHub, Postgres, Brave Search, Slack…) sin recompilar. Puedes <b>registrar servers de forma persistente</b> en el modal <b>Servidores MCP</b> (Configuración → MCP): los nombras una vez, eliges qué secretos inyectarles y Lucy los invoca por nombre, cacheando sus tools. También funciona <b>on-demand</b>: arranca el server como subproceso JSON-RPC, recupera lo que necesita y lo cierra.<br><br>` +
                `<b>🔧 Flujo real en Lucy</b><br>` +
                `<b>1. Guarda las variables/API keys en Configuración</b><br>` +
                `Configuración → sección "<b>Variables / API Keys para MCP</b>". Añade pares clave-valor (ej. <code>GITHUB_TOKEN</code> = <code>ghp_xxx</code>, <code>BRAVE_API_KEY</code> = <code>BSA…</code>). Se persisten en <b>Windows Credential Manager</b> (OS Keyring) — nunca en localStorage. Lucy las inyecta como variables de entorno al spawnear el server.<br><br>` +
                `<b>2. Descubre las tools de un server</b><br>` +
                `Pídele a Lucy en el chat: <i>"descubre las tools del server <code>npx -y @modelcontextprotocol/server-filesystem C:/Users/eleue/Desktop</code>"</i>. Lucy ejecuta <code>&lt;TOOL&gt;mcp_discover:&lt;cmd&gt;&lt;/TOOL&gt;</code> y te devuelve el catálogo (nombre, descripción, schema de cada tool).<br><br>` +
                `<b>3. Invoca una tool</b><br>` +
                `Lucy llama <code>&lt;TOOL&gt;mcp_query:&lt;cmd&gt;|||&lt;tool_name&gt;|||&lt;args_json&gt;&lt;/TOOL&gt;</code>. Ejemplo: <code>mcp_query:npx -y @modelcontextprotocol/server-filesystem C:/data|||read_file|||{"path":"notes.md"}</code>.<br><br>` +
                `<b>4. Permisos</b><br>` +
                `Cada <code>mcp_query</code> pasa por las mismas <b>Permission Rules</b> que las tools nativas — bloquéalas/permítelas por regex.<br><br>` +
                `<b>Servers útiles (npm)</b>: <code>@modelcontextprotocol/server-filesystem</code>, <code>-server-github</code>, <code>-server-postgres</code>, <code>-server-brave-search</code>, <code>-server-slack</code>, <code>-server-puppeteer</code>.<br><br>` +
                `<i>Tip</i>: requiere Node + <code>npx</code> en PATH (o sustituye por el comando que arranque tu server local).`,
            dEN: `<b>Sub-module: MCP — how to extend Lucy today</b><br><br>` +
                `<b>MCP (Model Context Protocol)</b> is Anthropic's open standard so Lucy can talk to <b>external tools</b> (filesystem, GitHub, Postgres, Brave Search, Slack…) without recompiling. You can <b>register servers persistently</b> in the <b>MCP Servers</b> modal (Settings → MCP): name them once, pick which secrets to inject, and Lucy invokes them by name, caching their tools. It also works <b>on-demand</b>: spawn the server as a JSON-RPC subprocess, get what it needs, and close it.<br><br>` +
                `<b>🔧 Real flow in Lucy</b><br>` +
                `<b>1. Save vars/API keys in Settings</b><br>` +
                `Settings → "<b>Variables / API Keys for MCP</b>" section. Add key-value pairs (e.g. <code>GITHUB_TOKEN</code> = <code>ghp_xxx</code>, <code>BRAVE_API_KEY</code> = <code>BSA…</code>). Persisted in <b>Windows Credential Manager</b> (OS Keyring) — never in localStorage. Lucy injects them as env vars when spawning the server.<br><br>` +
                `<b>2. Discover a server's tools</b><br>` +
                `Ask Lucy in chat: <i>"discover the tools of server <code>npx -y @modelcontextprotocol/server-filesystem C:/Users/eleue/Desktop</code>"</i>. Lucy runs <code>&lt;TOOL&gt;mcp_discover:&lt;cmd&gt;&lt;/TOOL&gt;</code> and returns the catalog (name, description, schema for each tool).<br><br>` +
                `<b>3. Invoke a tool</b><br>` +
                `Lucy calls <code>&lt;TOOL&gt;mcp_query:&lt;cmd&gt;|||&lt;tool_name&gt;|||&lt;args_json&gt;&lt;/TOOL&gt;</code>. Example: <code>mcp_query:npx -y @modelcontextprotocol/server-filesystem C:/data|||read_file|||{"path":"notes.md"}</code>.<br><br>` +
                `<b>4. Permissions</b><br>` +
                `Every <code>mcp_query</code> goes through the same <b>Permission Rules</b> as native tools — block/allow them by regex.<br><br>` +
                `<b>Useful servers (npm)</b>: <code>@modelcontextprotocol/server-filesystem</code>, <code>-server-github</code>, <code>-server-postgres</code>, <code>-server-brave-search</code>, <code>-server-slack</code>, <code>-server-puppeteer</code>.<br><br>` +
                `<i>Tip</i>: requires Node + <code>npx</code> on PATH (or replace with whatever command starts your local server).`,
        },
        {
            sel: ['body'], fallback: 'body', tip: 'top', view: 'terminal', welcome: true,
            tES: '⚙ Configuración · Verificador & Sub-Agentes',
            tEN: '⚙ Settings · Verifier & Sub-Agents',
            dES: `<b>Sub-módulo: Verificador</b><br><br>` +
                `Lucy ejecuta un <b>bucle agéntico</b>: ejecuta → verifica → repite. El <b>Verifier</b> es el modelo que evalúa si el output cumple el objetivo.<br><br>` +
                `• <b>Verifier mode</b>: <code>same</code> (mismo modelo del turno), <code>cheaper</code> (un modelo barato), <code>off</code> (sin verificación).<br>` +
                `• <b>Verifier model</b>: si eliges <code>cheaper</code>, aquí seleccionas cuál (típicamente Haiku o Flash o un Ollama local).<br>` +
                `• <b>Sub-Agents model</b>: el modelo que usan los forks lanzados con <code>fork_task</code>. Suele ser un modelo intermedio — los sub-agentes investigan en paralelo y Lucy sintetiza.<br><br>` +
                `<b>Cost ledger</b>: cada fork registra <code>tokens_in/out</code> y <code>cost_usd</code> en el panel Sub-Agentes. Útil para ver si los forks valen la pena.`,
            dEN: `<b>Sub-module: Verifier</b><br><br>` +
                `Lucy runs an <b>agentic loop</b>: execute → verify → iterate. The <b>Verifier</b> is the model that evaluates whether the output meets the goal.<br><br>` +
                `• <b>Verifier mode</b>: <code>same</code> (same model as turn), <code>cheaper</code> (a cheap model), <code>off</code> (no verification).<br>` +
                `• <b>Verifier model</b>: if you pick <code>cheaper</code>, here you select which one (typically Haiku, Flash, or a local Ollama).<br>` +
                `• <b>Sub-Agents model</b>: the model used by forks launched with <code>fork_task</code>. Typically an intermediate model — sub-agents investigate in parallel and Lucy synthesizes.<br><br>` +
                `<b>Cost ledger</b>: every fork records <code>tokens_in/out</code> and <code>cost_usd</code> in the Sub-Agents panel. Helps see if forks are worth it.`,
        },
        {
            sel: ['body'], fallback: 'body', tip: 'top', view: 'terminal', welcome: true,
            tES: '⚙ Configuración · Perfiles, Permisos & Runbooks',
            tEN: '⚙ Settings · Profiles, Permissions & Runbooks',
            dES: `<b>Sub-módulos finales</b><br><br>` +
                `<b>👤 Profiles</b> — múltiples identidades con configuración independiente. Útil si compartes la máquina o separas "trabajo" / "lab personal" / "incident response". Cada profile tiene su propia DB, sus memorias y sus reglas.<br><br>` +
                `<b>🛡 Permission Rules</b> — reglas regex que Lucy consulta ANTES de ejecutar cualquier comando o tocar cualquier ruta: <code>allow</code> (sin preguntar), <code>block</code> (rechazar), <code>ask</code> (HITL obligatorio). Scope por comando, path o host. Es tu primera barrera contra errores accidentales.<br><br>` +
                `<b>📂 Runbooks dir</b> — carpeta donde Lucy guarda runbooks markdown auto-promovidos desde workflows repetidos (F7 Runbook Mining). Por defecto: <code>%APPDATA%/Lucy/runbooks</code>. Puedes apuntarla a un repo Git para versionarlos en equipo.<br><br>` +
                `<b>◈ Principles</b> — máximas de alto nivel que SIEMPRE acompañan al prompt del agente (ej. "Nunca tocar producción sin confirmación"). Configurables aquí o en su panel dedicado.<br><br>` +
                `<b>🗑 Reset & Wipe</b> — al final del modal: "Borrar conversaciones", "Resetear memorias", "Factory reset". Cada acción pide confirmación doble.`,
            dEN: `<b>Final sub-modules</b><br><br>` +
                `<b>👤 Profiles</b> — multiple identities with independent config. Useful if you share the machine or separate "work" / "personal lab" / "incident response". Each profile has its own DB, memories and rules.<br><br>` +
                `<b>🛡 Permission Rules</b> — regex rules Lucy checks BEFORE running any command or touching any path: <code>allow</code> (no prompt), <code>block</code> (reject), <code>ask</code> (HITL required). Scope by command, path or host. Your first barrier against accidental damage.<br><br>` +
                `<b>📂 Runbooks dir</b> — folder where Lucy stores markdown runbooks auto-promoted from repeated workflows (F7 Runbook Mining). Default: <code>%APPDATA%/Lucy/runbooks</code>. Point it at a Git repo to version-control them as a team.<br><br>` +
                `<b>◈ Principles</b> — high-level maxims that ALWAYS travel with the agent prompt (e.g. "Never touch production without confirmation"). Configurable here or in their dedicated panel.<br><br>` +
                `<b>🗑 Reset & Wipe</b> — at the modal's bottom: "Clear conversations", "Reset memories", "Factory reset". Each requires double confirmation.`,
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
            tES: '✦ Detección de Anomalías',
            tEN: '✦ Anomaly Detection',
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
            tES: '✦ Predictor de Costo',
            tEN: '✦ Cost Predictor',
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
            tES: '✦ Indicador de Estado',
            tEN: '✦ Status Indicator',
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
            dES: '<b>Ctrl+P</b> · Paleta de comandos (busca cualquier vista, host o acción) — incluye <b>Exportar pestaña como Notebook</b>.<br><b>Ctrl+T</b> · Nueva terminal.<br><b>Ctrl+L</b> · Limpiar sesión actual.<br><b>Ctrl+F</b> · Buscar en NexShell.<br><b>Ctrl+Shift+Enter</b> · Ejecutar en background.<br><b>Tab</b> · Autocompletar comandos.<br><b>Esc</b> · Cancelar el agente o cerrar modal.<br><b>Ctrl+1/2/3</b> · Alternar densidad (Focus / Explore / War Room).<br><br><i>Tip</i>: escribe <code>/</code> en el chat para ver todos los comandos disponibles.',
            dEN: '<b>Ctrl+P</b> · Command palette (find any view, host or action) — includes <b>Export tab as Notebook</b>.<br><b>Ctrl+T</b> · New terminal.<br><b>Ctrl+L</b> · Clear current session.<br><b>Ctrl+F</b> · Find in NexShell.<br><b>Ctrl+Shift+Enter</b> · Run in background.<br><b>Tab</b> · Autocomplete commands.<br><b>Esc</b> · Cancel the agent or close modal.<br><b>Ctrl+1/2/3</b> · Toggle density (Focus / Explore / War Room).<br><br><i>Tip</i>: type <code>/</code> in chat to see every available command.',
        },
        {
            // ── Comandos internos / Internal commands ──
            // Final step de la introducción a v1.7.0 — referencia rápida de
            // los slash commands Frontier que el usuario puede teclear en cualquier
            // momento. Welcome card (no spotlight) para que se vea cómoda de leer.
            sel: ['body'],
            fallback: 'body',
            tip: 'top',
            view: 'terminal',
            welcome: true,
            tES: '✦ Comandos internos · Referencia rápida',
            tEN: '✦ Internal commands · Quick reference',
            dES: `Todos estos se escriben en el input del chat. <b>Lucy también los puede usar autónomamente</b> vía <code>&lt;TOOL&gt;</code> tags durante el agent loop.<br><br>` +
                `<b>📸 Observación temporal (F1 · F2 · F9)</b><br>` +
                `<code>/snapshot</code> — captura el estado del sistema ahora<br>` +
                `<code>/snapshots</code> — lista los snapshots recientes<br>` +
                `<code>/diff [from to]</code> — compara dos snapshots (sin args: últimos 2)<br>` +
                `<code>/kg-add &lt;dir&gt;</code> — añade un directorio al Knowledge Graph<br>` +
                `<code>/kg-rm &lt;dir&gt;</code> — quita un directorio del KG<br>` +
                `<code>/kg-roots</code> — lista los directorios indexados<br>` +
                `<code>/kg-scan [lookback_min]</code> — fuerza un scan inmediato<br>` +
                `<code>/kg-view &lt;path&gt;</code> — abre el grafo radial centrado en ese archivo<br><br>` +
                `<b>🔎 Investigación (F3 · F8 · synthesis)</b><br>` +
                `<code>/detective [seconds]</code> — investiga la ventana actual con F3+F8+F9 (alias: <code>/investigate</code>)<br><br>` +
                `<b>🧠 Memoria y aprendizaje (F4 · F7 · F10)</b><br>` +
                `<code>/runbooks [days]</code> — detecta workflows repetidos (alias: <code>/workflows</code>)<br>` +
                `<code>/promote-runbook &lt;name&gt; :: &lt;cmd1&gt; ; &lt;cmd2&gt; ; ...</code> — convierte un runbook en skill<br>` +
                `<code>/routines [days]</code> — aprende tus rutinas semanales (alias: <code>/patterns</code>)<br>` +
                `<code>/skills</code> — abre el picker con búsqueda fuzzy (alias: <code>/skill-list</code>)<br><br>` +
                `<b>🛡 Seguridad y preview (F5)</b><br>` +
                `<code>/preview &lt;cmd&gt;</code> — análisis estático + .wsb opcional (alias: <code>/sandbox</code>)<br><br>` +
                `<b>📊 Introspección</b><br>` +
                `<code>/frontier-stats</code> — qué Frontier features usas más (alias: <code>/telemetry</code>)<br>` +
                `<code>/recall &lt;query&gt;</code> — busca en tu historial de conversación<br>` +
                `<code>/crystallize</code> — destila la sesión actual en un crystal<br>` +
                `<code>/insights</code> — lista los insights destilados<br>` +
                `<code>/route</code> — explica la última decisión del smart-router<br><br>` +
                `<b>🎛 Configuración / utilidades</b><br>` +
                `<code>/help</code> — lista completa<br>` +
                `<code>/model &lt;nombre&gt;</code> — cambia modelo (parcial OK: "sonnet", "qwen", "flash")<br>` +
                `<code>/theme &lt;nombre&gt;</code> — default, ocean, hacker, sunset, forest, twilight, mocha, graphite<br>` +
                `<code>/smart-router on|off</code> · <code>/privacy on|off</code><br>` +
                `<code>/clear</code> — limpia el chat actual<br><br>` +
                `<b>Tip</b>: hover sobre una pestaña >500ms para ver un preview de sus últimos mensajes sin cambiar de tab.`,
            dEN: `All of these are typed in the chat input. <b>Lucy can also use them autonomously</b> via <code>&lt;TOOL&gt;</code> tags during the agent loop.<br><br>` +
                `<b>📸 Temporal observation (F1 · F2 · F9)</b><br>` +
                `<code>/snapshot</code> — capture system state now<br>` +
                `<code>/snapshots</code> — list recent snapshots<br>` +
                `<code>/diff [from to]</code> — compare two snapshots (no args: last 2)<br>` +
                `<code>/kg-add &lt;dir&gt;</code> — add a directory to the Knowledge Graph<br>` +
                `<code>/kg-rm &lt;dir&gt;</code> — remove a directory from the KG<br>` +
                `<code>/kg-roots</code> — list indexed directories<br>` +
                `<code>/kg-scan [lookback_min]</code> — force an immediate scan<br>` +
                `<code>/kg-view &lt;path&gt;</code> — open the radial graph centered on this file<br><br>` +
                `<b>🔎 Investigation (F3 · F8 · synthesis)</b><br>` +
                `<code>/detective [seconds]</code> — investigates current window with F3+F8+F9 (alias: <code>/investigate</code>)<br><br>` +
                `<b>🧠 Memory and learning (F4 · F7 · F10)</b><br>` +
                `<code>/runbooks [days]</code> — detects repeated workflows (alias: <code>/workflows</code>)<br>` +
                `<code>/promote-runbook &lt;name&gt; :: &lt;cmd1&gt; ; &lt;cmd2&gt; ; ...</code> — turn a runbook into a skill<br>` +
                `<code>/routines [days]</code> — learn your weekly routines (alias: <code>/patterns</code>)<br>` +
                `<code>/skills</code> — open the fuzzy-search picker (alias: <code>/skill-list</code>)<br><br>` +
                `<b>🛡 Safety and preview (F5)</b><br>` +
                `<code>/preview &lt;cmd&gt;</code> — static analysis + optional .wsb (alias: <code>/sandbox</code>)<br><br>` +
                `<b>📊 Introspection</b><br>` +
                `<code>/frontier-stats</code> — which Frontier features you use most (alias: <code>/telemetry</code>)<br>` +
                `<code>/recall &lt;query&gt;</code> — search your conversation history<br>` +
                `<code>/crystallize</code> — distill the current session into a crystal<br>` +
                `<code>/insights</code> — list distilled insights<br>` +
                `<code>/route</code> — explain the last smart-router decision<br><br>` +
                `<b>🎛 Settings / utilities</b><br>` +
                `<code>/help</code> — full list<br>` +
                `<code>/model &lt;name&gt;</code> — switch model (partial OK: "sonnet", "qwen", "flash")<br>` +
                `<code>/theme &lt;name&gt;</code> — default, ocean, hacker, sunset, forest, twilight, mocha, graphite<br>` +
                `<code>/smart-router on|off</code> · <code>/privacy on|off</code><br>` +
                `<code>/clear</code> — clear current chat<br><br>` +
                `<b>Tip</b>: hover a tab >500ms to preview its last messages without switching.`,
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
            // Sprint follow-up — auto-scroll target into view BEFORE measuring,
            // so steps near the bottom of a scroll container (sidebar, dashboard)
            // bring their target up to the safe area of the viewport.
            try {
                el.scrollIntoView({ behavior: 'instant', block: 'center', inline: 'center' });
            } catch {
                // Older browsers don't accept 'instant' — fall back to default.
                try { el.scrollIntoView(); } catch {}
            }
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
    //
    // Strategy v2 (fixes the "Comandos internos" step being cut off at the
    // bottom): we anchor the card at a SAFE top and let the CSS
    // `max-height: calc(100vh - 28px)` + `.tut-body { overflow-y: auto }`
    // guarantee the navigation footer is always visible. We DO NOT try to
    // guess the actual height — we just place the card so the bottom is at
    // most `h - p` away from the viewport edge.
    function tipStyle(s, w, h) {
        const TW  = 320;
        const p   = 14;
        // Effective max card height = full viewport minus margins.
        const TH_MAX = h - p * 2;
        // Welcome steps want a generous height so all content shows above the fold.
        // Step-bound tooltips want a tighter card so they don't cover the target.
        const TH_PREFERRED = (STEPS[step] || STEPS[0])?.welcome ? Math.min(640, TH_MAX) : Math.min(420, TH_MAX);
        const pos = (STEPS[step] || STEPS[0]).tip;
        const cx  = s.x + s.w / 2;
        const cy  = s.y + s.h / 2;

        const clampTop = (raw) => Math.max(p, Math.min(raw, h - TH_PREFERRED - p));

        // Welcome / overview steps: center the card horizontally + vertically.
        if (s.welcome) {
            const left = Math.max(p, w / 2 - TW / 2);
            const top  = Math.max(p, h / 2 - TH_PREFERRED / 2);
            return `left:${left}px;top:${top}px;max-height:${TH_PREFERRED}px;`;
        }

        if (pos === 'right') {
            const left = Math.min(s.x + s.w + p, w - TW - p);
            const top  = clampTop(cy - TH_PREFERRED / 2);
            return `left:${left}px;top:${top}px;max-height:${TH_PREFERRED}px;`;
        }
        if (pos === 'left') {
            const left = Math.max(p, s.x - TW - p);
            const top  = clampTop(cy - TH_PREFERRED / 2);
            return `left:${left}px;top:${top}px;max-height:${TH_PREFERRED}px;`;
        }
        if (pos === 'bottom') {
            const left = Math.max(p, Math.min(cx - TW / 2, w - TW - p));
            const top  = clampTop(s.y + s.h + p);
            return `left:${left}px;top:${top}px;max-height:${TH_PREFERRED}px;`;
        }
        // 'top' — show tooltip ABOVE the spotlight; force above if spotlight is near bottom
        const left = Math.max(p, Math.min(cx - TW / 2, w - TW - p));
        const above = s.y - TH_PREFERRED - p;
        const below = s.y + s.h + p;
        const top  = above >= p ? above : clampTop(below);
        return `left:${left}px;top:${top}px;max-height:${TH_PREFERRED}px;`;
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
     aria-label={$trad('Tutorial de Lucy')}
     on:keydown={onKey}
     tabindex="-1">

  <!-- Progress bar -->
  <div class="tut-prog"><div class="tut-bar" style="width:{pct}%"></div></div>

  <!-- Header -->
  <div class="tut-hdr">
    <span class="tut-badge">{step + 1} / {STEPS.length}</span>
    <button class="tut-skip" on:click={done}>{$trad('Salir')} ✕</button>
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
              aria-label="{$trad('Paso')} {i + 1}"
              on:click={() => goToStep(i)}></button>
    {/each}
  </div>

  <!-- Navigation buttons -->
  <div class="tut-foot">
    <button class="tut-btn tut-ghost" on:click={prev} disabled={step === 0}>
      ← {$trad('Atrás')}
    </button>
    <button class="tut-btn tut-pri" on:click={next}>
      {#if isLast}{$trad('✓ ¡Listo!')}{:else}{$trad('Siguiente →')}{/if}
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
