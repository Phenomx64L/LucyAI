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
    const LUCY_VERSION = '1.4.0';

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
            tES: `✦ Bienvenido a Lucy v${LUCY_VERSION} — R&D Frontier`,
            tEN: `✦ Welcome to Lucy v${LUCY_VERSION} — R&D Frontier`,
            dES: `Hola Iván — Lucy v${LUCY_VERSION} cruza una frontera real: <b>diez capacidades que ningún otro asistente de IA tiene hoy</b>, todas locales, todas con audit trail.<br><br>` +
                `<b>🔬 Frontier Capabilities (10/10)</b><br>` +
                `• <b>F1 Process Lineage</b> — graba el árbol de cada proceso (parent → root) con hash SHA-256 verificable. <code>/help</code> + <code>&lt;TOOL&gt;process_ancestry:PID&lt;/TOOL&gt;</code><br>` +
                `• <b>F2 State Snapshots</b> — captura estado del sistema cada 15 min y permite diff temporal. <code>/snapshot</code>, <code>/diff</code><br>` +
                `• <b>F3 Causal Engine</b> — correlaciona arrivals de procesos con síntomas y rankea causas con reasoning. <code>&lt;TOOL&gt;diagnose_spike:60&lt;/TOOL&gt;</code><br>` +
                `• <b>F4 Self-Healing</b> — recuerda fixes pasados y los propone (con HITL) ante síntomas parecidos. Auto-crystallize en cada incident resuelto.<br>` +
                `• <b>F5 Sandbox Preview</b> — análisis estático antes de ejecutar destructivos + .wsb config para Windows Sandbox. <code>/preview &lt;cmd&gt;</code><br>` +
                `• <b>F6 Object Bridge</b> — mantiene PowerShell objects vivos entre turnos; pipeable con un mini-DSL. <code>&lt;TOOL&gt;obj_query:procs | where Mem &gt; 100&lt;/TOOL&gt;</code><br>` +
                `• <b>F7 Runbook Mining</b> — detecta workflows repetidos y los promueve a skills reusables. <code>/runbooks</code> → <code>/promote-runbook</code><br>` +
                `• <b>F8 Mini-EDR</b> — clasifica procesos por 7 heurísticos (path/parent/cmdline/entropy/novelty/timing). <code>/preview</code> destructivos<br>` +
                `• <b>F9 Knowledge Graph</b> — indexa tus repos y aprende qué archivos se tocan juntos. <code>/kg-add</code>, <code>/kg-view &lt;path&gt;</code><br>` +
                `• <b>F10 Daily Patterns</b> — aprende tus rutinas semanales (Lun 9am → VSCode + Spotify). <code>/routines</code><br>` +
                `• <b>🔎 Incident Detective</b> — sintetiza F3+F8+F9 en una sola consulta forense. <code>/detective</code><br><br>` +
                `<b>🎨 UX nueva (10/10)</b><br>` +
                `• <b>Living Avatar</b> — Lucy ahora respira (idle), pulsa cyan al pensar, brilla dorado al ejecutar, ámbar al preocuparse.<br>` +
                `• <b>Density Modes</b> — Ctrl+1 (focus) · Ctrl+2 (explore) · Ctrl+3 (war room).<br>` +
                `• <b>Chapter View</b> — investigaciones multi-paso se renderizan como un libro con índice navegable.<br>` +
                `• <b>Predictive Chips</b> — sugerencias contextuales arriba del input según el último turno.<br>` +
                `• <b>Drag-to-Lucy</b> — arrastra URLs, texto, imágenes, archivos: Lucy infiere qué hacer.<br>` +
                `• <b>Tab Hover Preview</b> — pasa el mouse sobre una pestaña >500ms y ves los últimos mensajes.<br>` +
                `• <b>Heat Layers</b> — overlays de severidad/recencia en listas de procesos y archivos.<br>` +
                `• <b>Confidence Markers v2</b> — Lucy distingue [!hechos!], [~hedges~], [?especulaciones?] visualmente.<br>` +
                `• <b>Circadian Theme</b> — los acentos se enfrían suavemente de día a noche.<br>` +
                `• <b>Skill Picker</b> — <code>/skills</code> abre un modal con búsqueda fuzzy.<br><br>` +
                `<b>📊 Telemetría interna</b><br>` +
                `<code>/frontier-stats</code> muestra qué Frontier tools usaste, cuánto duraron, y error rate. 100% local.<br><br>` +
                `<b>🏆 Numbers</b><br>` +
                `103 tests passing · 9 stress tests · ~8,300 LOC R&D · 36 Tauri commands Frontier · 0 deuda técnica visible.<br><br>` +
                `Vamos a recorrer la interfaz — y después un apartado dedicado a los <b>comandos internos</b>.`,
            dEN: `Hi Iván — Lucy v${LUCY_VERSION} crosses a real frontier: <b>ten capabilities that no other AI assistant has today</b>, all local, all with an audit trail.<br><br>` +
                `<b>🔬 Frontier Capabilities (10/10)</b><br>` +
                `• <b>F1 Process Lineage</b> — records every process's parent chain (current → root) with verifiable SHA-256 hash.<br>` +
                `• <b>F2 State Snapshots</b> — captures system state every 15 min and lets you diff over time. <code>/snapshot</code>, <code>/diff</code><br>` +
                `• <b>F3 Causal Engine</b> — correlates process arrivals with symptoms and ranks causes with explainable reasoning.<br>` +
                `• <b>F4 Self-Healing</b> — recalls past fixes and proposes them (with HITL) on similar symptoms. Auto-crystallizes resolved incidents.<br>` +
                `• <b>F5 Sandbox Preview</b> — static analysis before destructive commands + .wsb config for Windows Sandbox. <code>/preview &lt;cmd&gt;</code><br>` +
                `• <b>F6 Object Bridge</b> — keeps PowerShell objects alive across turns, pipeable with a small DSL.<br>` +
                `• <b>F7 Runbook Mining</b> — detects repeated workflows and promotes them to reusable skills.<br>` +
                `• <b>F8 Mini-EDR</b> — classifies processes by 7 behavioral heuristics (path/parent/cmdline/entropy/novelty/timing).<br>` +
                `• <b>F9 Knowledge Graph</b> — indexes your repos and learns which files are touched together. <code>/kg-add</code>, <code>/kg-view</code><br>` +
                `• <b>F10 Daily Patterns</b> — learns your weekly routines (Mon 9am → VSCode + Spotify). <code>/routines</code><br>` +
                `• <b>🔎 Incident Detective</b> — synthesizes F3+F8+F9 into a single forensic query. <code>/detective</code><br><br>` +
                `<b>🎨 New UX (10/10)</b><br>` +
                `• <b>Living Avatar</b> — Lucy now breathes (idle), pulses cyan when thinking, glows gold when executing, amber when concerned.<br>` +
                `• <b>Density Modes</b> — Ctrl+1 (focus) · Ctrl+2 (explore) · Ctrl+3 (war room).<br>` +
                `• <b>Chapter View</b> — multi-step investigations render as a book with a navigable index.<br>` +
                `• <b>Predictive Chips</b> — contextual suggestions above the input based on the last turn.<br>` +
                `• <b>Drag-to-Lucy</b> — drag URLs, text, images, files: Lucy infers what to do.<br>` +
                `• <b>Tab Hover Preview</b> — hover over a tab >500ms to see the last messages.<br>` +
                `• <b>Heat Layers</b> — severity/recency overlays on process and file lists.<br>` +
                `• <b>Confidence Markers v2</b> — Lucy visually distinguishes [!facts!], [~hedges~], [?speculation?].<br>` +
                `• <b>Circadian Theme</b> — accents subtly cool from day to night.<br>` +
                `• <b>Skill Picker</b> — <code>/skills</code> opens a modal with fuzzy search.<br><br>` +
                `<b>📊 Internal telemetry</b><br>` +
                `<code>/frontier-stats</code> shows which Frontier tools you used, how long they took, and error rate. 100% local.<br><br>` +
                `<b>🏆 Numbers</b><br>` +
                `103 tests passing · 9 stress tests · ~8,300 LOC R&D · 36 Tauri Frontier commands · 0 visible tech debt.<br><br>` +
                `Let's walk through the interface — and we'll wrap up with a dedicated <b>internal commands</b> reference.`,
        },
        {
            sel: ['.chat-wrap.on .chat-area', '.chat-wrap.on', '.panel'],
            tip: 'left',
            view: 'terminal',
            tES: '↗ Terminal IA — Bucle Agéntico',
            tEN: '↗ AI Terminal — Agentic Loop',
            dES: 'El corazón de Lucy. Escribe tu instrucción y la IA no solo te contestará, sino que <b>evaluará, verificará y ejecutará</b> automáticamente hasta completar la tarea. Incluye <b>PLAN/VERIFY/ROLLBACK</b> — para cambios riesgosos Lucy propone un plan, verifica el resultado y revierte automáticamente si falla. <br><br><b>NUEVO en v1.4.0</b>: empieza a escribir <code>rm -</code> o <code>find . -</code> y verás autocompletado inline de flags (Tab/Enter). Los destructivos van al final con ⚠.',
            dEN: 'The core of Lucy. Type an instruction and the AI will not only reply, but <b>evaluate, verify and auto-execute</b> commands until the task is complete. Includes <b>PLAN/VERIFY/ROLLBACK</b> — for risky changes Lucy proposes a plan, verifies the outcome, and auto-rolls back if it fails. <br><br><b>NEW in v1.4.0</b>: start typing <code>rm -</code> or <code>find . -</code> and you get inline flag autocomplete (Tab/Enter). Destructive flags go last with ⚠.',
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
            dES: '<b>Ctrl+P</b> · Paleta de comandos (busca cualquier vista, host o acción) — incluye <b>Exportar pestaña como Notebook</b>.<br><b>Ctrl+T</b> · Nueva terminal.<br><b>Ctrl+L</b> · Limpiar sesión actual.<br><b>Ctrl+F</b> · Buscar en NexShell.<br><b>Ctrl+Shift+Enter</b> · Ejecutar en background.<br><b>Tab</b> · Autocompletar comandos.<br><b>Esc</b> · Cancelar el agente o cerrar modal.<br><br><b>NUEVO en v' + LUCY_VERSION + '</b>: <b>Ctrl+1/2/3</b> para alternar densidad (Focus / Explore / War Room).',
            dEN: '<b>Ctrl+P</b> · Command palette (find any view, host or action) — includes <b>Export tab as Notebook</b>.<br><b>Ctrl+T</b> · New terminal.<br><b>Ctrl+L</b> · Clear current session.<br><b>Ctrl+F</b> · Find in NexShell.<br><b>Ctrl+Shift+Enter</b> · Run in background.<br><b>Tab</b> · Autocomplete commands.<br><b>Esc</b> · Cancel the agent or close modal.<br><br><b>NEW in v' + LUCY_VERSION + '</b>: <b>Ctrl+1/2/3</b> to toggle density (Focus / Explore / War Room).',
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
