<script>
  /* ==========================================================================
     Lucy 2.0 — Cockpit shell  ·  Phases F2–F4 (UI 2.0, direction C "spatial agentic")
     --------------------------------------------------------------------------
     Multi-view host: the icon RAIL switches the center between the Terminal IA
     two-lane view (CONVERSATION + agent WORKSPACE, the workspace collapsible)
     and other module views (Dashboard built; the rest show a placeholder). The
     workspace is fed by the agent-workspace store — demo driver today, the real
     +page.svelte bridge at integration. Reachable at /cockpit, fully additive:
     it does NOT touch +page.svelte or the current UI.
     ========================================================================== */
  import '$lib/styles/cockpit-tokens.css';

  import LayoutDashboard from '@tabler/icons-svelte/icons/layout-dashboard';
  import Sparkles from '@tabler/icons-svelte/icons/sparkles';
  import Terminal2 from '@tabler/icons-svelte/icons/terminal-2';
  import FileText from '@tabler/icons-svelte/icons/file-text';
  import Activity from '@tabler/icons-svelte/icons/activity';
  import Server from '@tabler/icons-svelte/icons/server';
  import ShieldCheck from '@tabler/icons-svelte/icons/shield-check';
  import Brain from '@tabler/icons-svelte/icons/brain';
  import ArrowUp from '@tabler/icons-svelte/icons/arrow-up';
  import Minus from '@tabler/icons-svelte/icons/minus';
  import Square from '@tabler/icons-svelte/icons/square';
  import X from '@tabler/icons-svelte/icons/x';
  import Plus from '@tabler/icons-svelte/icons/plus';
  import Paperclip from '@tabler/icons-svelte/icons/paperclip';
  import Microphone from '@tabler/icons-svelte/icons/microphone';
  import MicrophoneOff from '@tabler/icons-svelte/icons/microphone-off';
  import Message2 from '@tabler/icons-svelte/icons/message-2';
  import PlayerStopFilled from '@tabler/icons-svelte/icons/player-stop-filled';
  import ThumbUp from '@tabler/icons-svelte/icons/thumb-up';
  import ThumbDown from '@tabler/icons-svelte/icons/thumb-down';
  import Pencil from '@tabler/icons-svelte/icons/pencil';
  import Refresh from '@tabler/icons-svelte/icons/refresh';
  import LayoutSidebarRight from '@tabler/icons-svelte/icons/layout-sidebar-right';
  import Settings from '@tabler/icons-svelte/icons/settings';

  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { t } from './i18n';
  import AgentWorkspace from './AgentWorkspace.svelte';
  import CockpitDashboard from './CockpitDashboard.svelte';
  import CockpitNexShell from './CockpitNexShell.svelte';
  import CockpitMemory from './CockpitMemory.svelte';
  import CockpitLogs from './CockpitLogs.svelte';
  import CockpitInventory from './CockpitInventory.svelte';
  import CockpitCompliance from './CockpitCompliance.svelte';
  import CockpitConfig from './CockpitConfig.svelte';
  import { localHostname, ensureLocalHostname, hostLabel } from './host-info';
  import CockpitTour from './CockpitTour.svelte';
  import Help from '@tabler/icons-svelte/icons/help';
  import { agentStatus, agentConvo, agentStream, agentTrace } from './agent-workspace';

  // v1.7.236 — el hilo muestra QUÉ hace Lucy, no solo "pensando". Traduce la
  // fase del último trace (think/act/react/info) a una etiqueta corta.
  const _PHASE_WORD = {
    think: () => t('pensando', 'thinking'),
    act:   () => t('ejecutando', 'running'),
    react: () => t('analizando', 'analyzing'),
    info:  () => t('nota', 'note'),
  };
  function phaseWord(p) { return (_PHASE_WORD[p] || (() => t('trabajando', 'working')))(); }

  // v1.7.236 — primeras N líneas no vacías de una salida, para la vista previa
  // inline de las tarjetas de tool: el hilo enseña el resultado sin desplegar,
  // como Claude Code, en vez de una fila colapsada muda.
  function previewLines(text, n = 2) {
    if (!text) return '';
    const lines = String(text).split('\n').map((l) => l.trim()).filter(Boolean).slice(0, n);
    const joined = lines.join('  ·  ');
    return joined.length > 160 ? joined.slice(0, 160) + '…' : joined;
  }
  import { renderMd } from '$lib/md-render';
  import { morphHtml } from '$lib/morph-html';
  import { copyToClipboard } from '$lib/lucy-api';
  import { runCockpitDemo } from './cockpit-demo';
  import { LLM_GROUPS, getModelDescription, getModelIcon, localModels, ollamaOnline, refreshLocalModels } from '$lib/models.js';
  import ChevronDown from '@tabler/icons-svelte/icons/chevron-down';
  import Check from '@tabler/icons-svelte/icons/check';
  import Copy from '@tabler/icons-svelte/icons/copy';
  import { LUCY_AVATAR_DATA_URL } from '$lib/assets/lucy-avatar-data';

  const rail = [
    { id: 'dashboard',  Icon: LayoutDashboard, label: 'Dashboard' },
    { id: 'terminal',   Icon: Sparkles,        label: 'Terminal IA' },
    { id: 'nexshell',   Icon: Terminal2,       label: 'NexShell' },
    { id: 'logs',       Icon: FileText,        label: 'Log Viewer' },
    { id: 'inventory',  Icon: Server,          label: 'Inventario' },
    { id: 'compliance', Icon: ShieldCheck,     label: 'Compliance' },
    { id: 'memory',     Icon: Brain,           label: 'Memoria' },
    { id: 'config',     Icon: Settings,        label: 'Configuración' },
  ];

  // `live` = embedded in +page.svelte with the real agent feeding the store.
  // The route preview (/cockpit) leaves it false and plays the demo instead.
  // `onSubmit(text)` runs the REAL agent when the cockpit composer is used.
  let { live = false, onSubmit = undefined, onHostAdd = undefined, onHostEdit = undefined, onHostDelete = undefined, model = null, onModelChange = undefined, personality = 'balanced', onSetPersonality = undefined, onConfigureKeys = undefined, onOpenSettings = undefined,
        smartRouting = false, privacyMode = false, onSetPrivacyMode = undefined,
        tabs = [], activeTabId = null, onSelectTab = undefined, onNewTab = undefined, onCloseTab = undefined,
        onStop = undefined, hitl = null, onHitlApprove = undefined, onHitlCancel = undefined,
        onRegenerate = undefined, onReact = undefined,
        userName = '',
        attachments = [], onAttach = undefined, onRemoveAttach = undefined } = $props();

  // ── Momento de firma — saludo por hora del día + iniciales del usuario ──────
  // El empty-state de Terminal IA saluda con el rostro de Lucy y el nombre real
  // del usuario (lucyConfig.name), variando por franja horaria como en V1.
  function _greetWord() {
    const h = new Date().getHours();
    if (h < 12) return t('Buenos días', 'Good morning');
    if (h < 19) return t('Buenas tardes', 'Good afternoon');
    return t('Buenas noches', 'Good evening');
  }
  const _firstName = $derived((userName || '').trim().split(/\s+/).filter(Boolean)[0] || '');
  const greeting = $derived(_firstName ? `${_greetWord()}, ${_firstName}` : _greetWord());
  const userInitials = $derived.by(() => {
    const parts = (userName || '').trim().split(/\s+/).filter(Boolean);
    if (parts.length === 0) return 'U';
    if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
    return (parts[0][0] + parts[1][0]).toUpperCase();
  });

  // ── Model picker (cloud catalog + auto-discovered local LLMs) ──────────────
  let modelMenuOpen = $state(false);
  let modelQuery = $state('');
  const PROVIDER_LABEL = { anthropic: 'Anthropic Claude', gemini: 'Google Gemini', openai: 'OpenAI', nvidia: 'NVIDIA NIM', ollama: 'Local (Ollama)' };
  const modelGroups = $derived.by(() => {
    const _dep = $localModels; void _dep;   // re-run when local models are (re)discovered
    const q = modelQuery.trim().toLowerCase();
    return LLM_GROUPS.map((g) => ({
      provider: g.provider,
      label: PROVIDER_LABEL[g.provider] || String(g.label || g.provider).replace(/[─-]/g, '').trim(),
      options: (g.options || [])
        .map((o) => ({ id: o.id, icon: o.icon, name: o.nameEs || o.nameEn }))
        .filter((o) => !q || (o.name || '').toLowerCase().includes(q) || (o.id || '').toLowerCase().includes(q) || (g.provider || '').includes(q)),
    })).filter((g) => g.options.length > 0);
  });
  const currentModelIcon = $derived(model ? getModelIcon(model) : '◆');
  const currentModelLabel = $derived(model ? getModelDescription(model, false) : 'Elegir modelo');
  function pickModel(id) { onModelChange?.(id); modelMenuOpen = false; modelQuery = ''; }
  function refreshLocal() { refreshLocalModels().catch(() => {}); }

  // ── Cockpit accent theme (overrides the .lucy-v2 accent tokens on the root;
  //    independent of the classic V1 color scheme). Persisted in localStorage. ──
  const ACCENTS = {
    emerald: { c: '#3DD6A4', h: '#34C296', ink: '#07130E', bg: 'rgba(61,214,164,0.12)', ln: 'rgba(61,214,164,0.28)' },
    blue:    { c: '#5B9DF9', h: '#4A8CE8', ink: '#06101F', bg: 'rgba(91,157,249,0.14)', ln: 'rgba(91,157,249,0.30)' },
    violet:  { c: '#A78BFA', h: '#9B7BF5', ink: '#120A20', bg: 'rgba(167,139,250,0.16)', ln: 'rgba(167,139,250,0.32)' },
    amber:   { c: '#E5B567', h: '#D9A855', ink: '#1C1405', bg: 'rgba(229,181,103,0.16)', ln: 'rgba(229,181,103,0.32)' },
    pink:    { c: '#F06EA9', h: '#E85D9C', ink: '#1F0812', bg: 'rgba(240,110,169,0.16)', ln: 'rgba(240,110,169,0.32)' },
    cyan:    { c: '#4FD1E0', h: '#3EC0CF', ink: '#04161A', bg: 'rgba(79,209,224,0.16)', ln: 'rgba(79,209,224,0.32)' },
  };
  const _loadAccent = () => { try { const a = localStorage.getItem('lucy_cockpit_accent'); return a && ACCENTS[a] ? a : 'emerald'; } catch { return 'emerald'; } };
  let accentKey = $state(_loadAccent());
  const accentVars = $derived.by(() => {
    const a = ACCENTS[accentKey] || ACCENTS.emerald;
    return `--accent:${a.c};--accent-hover:${a.h};--accent-ink:${a.ink};--accent-bg:${a.bg};--accent-line:${a.ln};--border-accent:${a.ln};`;
  });
  function setAccent(k) { if (!ACCENTS[k]) return; accentKey = k; try { localStorage.setItem('lucy_cockpit_accent', k); } catch {} }

  // ── Tema claro/oscuro (v1.7.234) ────────────────────────────────────────────
  // themeKey ∈ 'dark' | 'light' | 'auto'. 'auto' sigue al SO. El atributo
  // data-theme resuelto va en el root .lucy-v2 → activa el bloque de tokens
  // claros de cockpit-tokens.css. Persistido en lucy_cockpit_theme.
  const _loadTheme = () => { try { const v = localStorage.getItem('lucy_cockpit_theme'); return (v === 'light' || v === 'auto') ? v : 'dark'; } catch { return 'dark'; } };
  let themeKey = $state(_loadTheme());
  let _osLight = $state(false);
  let _mqTheme = null, _mqRef = null;   // matchMedia listener handles (cleanup)
  const effectiveTheme = $derived(themeKey === 'auto' ? (_osLight ? 'light' : 'dark') : themeKey);
  function setTheme(k) { if (!['dark', 'light', 'auto'].includes(k)) return; themeKey = k; try { localStorage.setItem('lucy_cockpit_theme', k); } catch {} }

  let draft = $state('');
  let inputEl = $state(null);   // composer <input> ref — refocus after palette pick
  function send() {
    const t = draft.trim();
    if (!t && !(attachments && attachments.length)) return;   // allow attachment-only sends
    if (micOn) stopMic();                                     // sending closes the mic
    onSubmit?.(t, { voice: _voiceUsed });                     // voice → Lucy responde hablando (TTS del pipeline V1)
    _voiceUsed = false;
    draft = '';
  }

  // ── Fase A — Voz en el composer (paridad V1) ────────────────────────────────
  // Reconocimiento LOCAL del cockpit (Web Speech API) que dicta sobre `draft`;
  // no toca el t.inputValue de V1. Si la orden llegó por voz, send() lo marca y
  // el pipeline clásico hace speak() de la respuesta (mismo TTS de siempre).
  let micOn = $state(false);
  let _voiceUsed = false;
  let _recog = null;
  const micSupported = typeof window !== 'undefined' && !!(window.SpeechRecognition || window.webkitSpeechRecognition);
  function stopMic() {
    micOn = false;
    try { _recog?.stop(); } catch {}
    _recog = null;
  }
  function toggleMic() {
    if (micOn) { stopMic(); return; }
    const SR = window.SpeechRecognition || window.webkitSpeechRecognition;
    if (!SR) return;
    try {
      _recog = new SR();
      _recog.lang = (navigator.language || 'es-MX');
      _recog.continuous = true;
      _recog.interimResults = false;
      _recog.onresult = (ev) => {
        for (let i = ev.resultIndex; i < ev.results.length; i++) {
          if (ev.results[i].isFinal) {
            const txt = String(ev.results[i][0]?.transcript || '').trim();
            if (txt) { draft = draft ? `${draft} ${txt}` : txt; _voiceUsed = true; }
          }
        }
      };
      _recog.onerror = () => stopMic();
      _recog.onend = () => { if (micOn) { try { _recog?.start(); } catch { stopMic(); } } };
      _recog.start();
      micOn = true;
      inputEl?.focus();
    } catch { stopMic(); }
  }

  // ── Composer #4: suggested prompts + slash-command palette ──────────────────
  // Empty-state suggestions — one click runs a common SysAdmin task.
  const SUGGESTIONS = [
    { icon: Activity,    label: t('Salud del sistema', 'System health'),
      text: t('Revisa la salud del sistema (CPU, RAM, disco, servicios) y dame un resumen del estado.',
              'Check system health (CPU, RAM, disk, services) and give me a status summary.') },
    { icon: ShieldCheck, label: t('Vulnerabilidades', 'Vulnerabilities'),
      text: t('Escanea el software instalado en busca de vulnerabilidades conocidas y dime cómo parcharlas.',
              'Scan installed software for known vulnerabilities and tell me how to patch them.') },
    { icon: Server,      label: t('Servicios detenidos', 'Stopped services'),
      text: t('¿Qué servicios de inicio automático están detenidos ahora mismo? Muéstramelos.',
              'Which automatic-start services are stopped right now? Show them to me.') },
    { icon: FileText,    label: t('Errores recientes', 'Recent errors'),
      text: t('Resume los errores más recientes del registro de eventos del sistema (últimas 24 h).',
              'Summarize the most recent System event log errors (last 24 h).') },
  ];
  // Palette is a discovery aid; the command actually EXECUTES because onSubmit
  // routes '/'-prefixed text through the classic slash pipeline (handleSlashCommand).
  // Catálogo completo (Fase A — paridad con el /help de V1). OJO: algunos
  // abren vistas/modals de la V1 (el overlay cede o queda detrás) — la
  // portada de esas vistas al cockpit es Fase B.
  const SLASH = [
    { cmd: '/model',        desc: 'Cambiar el modelo activo' },
    { cmd: '/clear',        desc: 'Limpiar el chat actual' },
    { cmd: '/memory',       desc: 'Explorador de memoria (V1)' },
    { cmd: '/kg',           desc: 'Grafo de conocimiento (V1)' },
    { cmd: '/link',         desc: 'Relaciones tipadas entre memorias' },
    { cmd: '/recall',       desc: 'Recuperar memorias por consulta' },
    { cmd: '/crystals',     desc: 'Ver crystals de memoria' },
    { cmd: '/crystallize',  desc: 'Destilar la sesión en un crystal' },
    { cmd: '/insights',     desc: 'Insights consolidados' },
    { cmd: '/consolidate',  desc: 'Ejecutar consolidación ahora' },
    { cmd: '/playbooks',    desc: 'Playbooks multi-fase curados' },
    { cmd: '/skills',       desc: 'Picker de skills ejecutables' },
    { cmd: '/preset',       desc: 'Presets de framing (AD, Hyper-V, SQL…)' },
    { cmd: '/sec-skill',    desc: 'Catálogo security/forensics (200+)' },
    { cmd: '/skills-manager', desc: 'Gestionar skills cargadas' },
    { cmd: '/capabilities', desc: 'Auto-introspección: skills, MCPs, frameworks' },
    { cmd: '/route',        desc: 'Ver la última decisión de routing' },
    { cmd: '/serial',       desc: 'Bypass del fork advisor (esta pestaña)' },
    { cmd: '/smart-router', desc: 'Smart-router on/off' },
    { cmd: '/proactive',    desc: 'Listar insights proactivos' },
    { cmd: '/snapshot',     desc: 'Capturar snapshot del sistema' },
    { cmd: '/diff',         desc: 'Comparar dos snapshots' },
    { cmd: '/detective',    desc: 'Síntesis forense de incidente' },
    { cmd: '/runbooks',     desc: 'Lista de runbooks (V1)' },
    { cmd: '/pantalla',     desc: 'Lucy ve tu pantalla (captura + pregunta)' },
    { cmd: '/polarity',     desc: 'Proyección de polaridad de un texto' },
    { cmd: '/privacy',      desc: 'Modo privacidad (sólo LLM local)' },
    { cmd: '/theme',        desc: 'Cambiar el tema visual' },
    { cmd: '/help',         desc: 'Referencia completa de comandos' },
  ];
  let _slashIdx = $state(0);
  // Show only while typing the command token (single '/'-prefixed word, no space
  // yet). Once args start, get out of the way so Enter sends normally.
  const slashMatches = $derived.by(() => {
    const d = draft.trim();
    if (!/^\/\S*$/.test(d)) return [];
    const q = d.toLowerCase();
    return SLASH.filter((s) => s.cmd.toLowerCase().startsWith(q));
  });
  const slashOpen = $derived(slashMatches.length > 0);
  $effect(() => { if (_slashIdx >= slashMatches.length) _slashIdx = 0; }); // keep highlight in range
  function pickSlash(s) { if (s) { draft = s.cmd + ' '; inputEl?.focus(); } }   // fill; user adds args or hits Enter to run
  // v1.7.236 iter — composer multilínea autoajustable. El <textarea> crece con
  // el contenido (1 línea → hasta ~6) y se re-mide cuando `draft` cambia por
  // fuera (envío que lo limpia, dictado por voz, pick de slash). Enter envía;
  // Shift+Enter inserta salto (lo maneja onComposerKey, que ya distingue shift).
  function autoGrow(node, _dep) {
    const MAX = 150;
    const fit = () => {
      node.style.height = 'auto';
      node.style.height = Math.min(node.scrollHeight, MAX) + 'px';
      node.style.overflowY = node.scrollHeight > MAX ? 'auto' : 'hidden';
    };
    fit();
    node.addEventListener('input', fit);
    return { update() { fit(); }, destroy() { node.removeEventListener('input', fit); } };
  }

  function onComposerKey(e) {
    if (slashOpen) {
      if (e.key === 'ArrowDown') { e.preventDefault(); _slashIdx = (_slashIdx + 1) % slashMatches.length; return; }
      if (e.key === 'ArrowUp')   { e.preventDefault(); _slashIdx = (_slashIdx - 1 + slashMatches.length) % slashMatches.length; return; }
      if (e.key === 'Tab')       { e.preventDefault(); pickSlash(slashMatches[_slashIdx]); return; }
      if (e.key === 'Escape')    { e.preventDefault(); draft = ''; return; }
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        const d = draft.trim().toLowerCase();
        const exact = slashMatches.some((s) => s.cmd.toLowerCase() === d);
        if (exact) send(); else pickSlash(slashMatches[_slashIdx]);
        return;
      }
    }
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); }
  }

  let activeModule = $state('terminal');
  let workspaceCollapsed = $state(false);
  // Expand/collapse state for inline reasoning + tool-call cards (keyed by msg id).
  let _actOpen = $state({});
  // Per-message actions (copy / regenerate / edit / 👍👎).
  let _copiedMsg = $state(null);
  let _reactions = $state({});      // convo msg id → 'up' | 'down'
  let _copyMsgTimer = null;
  async function copyMsg(m) {
    try { await copyToClipboard(m.text); } catch { try { navigator.clipboard?.writeText(m.text); } catch {} }
    _copiedMsg = m.id;
    if (_copyMsgTimer) clearTimeout(_copyMsgTimer);
    _copyMsgTimer = setTimeout(() => { _copiedMsg = null; }, 1200);
  }
  function editUser(m) { draft = m.text; }   // load the user prompt back into the composer to re-edit & resend
  function react(m, kind) {
    const next = _reactions[m.id] === kind ? null : kind;
    _reactions[m.id] = next;
    if (next) onReact?.(next, m.text);
  }
  const activeLabel = $derived(rail.find((r) => r.id === activeModule)?.label ?? 'Cockpit');

  // Keep the conversation pinned to the newest line as it streams / grows.
  let threadEl = $state(null);
  $effect(() => {
    const _grow = $agentConvo.length + $agentStream.length; // deps: re-run on new lines or stream tokens
    void _grow;
    if (threadEl) threadEl.scrollTop = threadEl.scrollHeight;
  });

  // Drag-to-resize the agent workspace lane (grab its left edge). Clamped so the
  // conversation never collapses below its min. The workspace sits on the right,
  // so dragging LEFT (smaller clientX) widens it.
  const WS_WIDTH_KEY = 'lucy_cockpit_ws_width';
  const _loadWsWidth = () => {
    try { const v = parseInt(localStorage.getItem(WS_WIDTH_KEY) || '', 10); return v >= 280 && v <= 560 ? v : 340; }
    catch { return 340; }
  };
  let wsWidth = $state(_loadWsWidth());
  let _resizing = $state(false);
  let _startX = 0, _startW = 0;
  function startResize(e) {
    _resizing = true; _startX = e.clientX; _startW = wsWidth;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function moveResize(e) {
    if (!_resizing) return;
    wsWidth = Math.max(280, Math.min(560, _startW + (_startX - e.clientX)));
  }
  function endResize(e) {
    _resizing = false;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    try { localStorage.setItem(WS_WIDTH_KEY, String(wsWidth)); } catch {}
  }

  // Window controls (Tauri) — the classic titlebar uses these same commands.
  const winMin = () => invoke('minimize_window').catch(() => {});
  const winMax = () => invoke('maximize_window').catch(() => {});
  const winClose = () => invoke('close_window').catch(() => {});

  // Demo: scripted agent run so the workspace is alive at /cockpit — skipped in
  // live mode, where the real +page.svelte bridge feeds the store instead.
  let cancelDemo = () => {};
  // ── Cockpit tour (v1.7.233) — V2 counterpart of V1's TutorialOverlay ──────
  // Auto-opens once per install (localStorage flag); relaunchable anytime via
  // the titlebar «?». Steps switch the live module behind the card.
  let tourOpen = $state(false);
  function tourDone() {
    tourOpen = false;
    try { localStorage.setItem('lucy_cockpit_tour_done', '1'); } catch {}
  }

  let _raf = null;
  onMount(() => {
    if (!live) cancelDemo = runCockpitDemo();
    // Una sola resolución para todo el cockpit; los módulos que la piden en su
    // propio onMount comparten esta llamada.
    ensureLocalHostname();
    refreshLocalModels().catch(() => {});   // auto-discover local Ollama models
    // First-run tour (live only — the /cockpit browser preview plays the demo).
    try { if (live && localStorage.getItem('lucy_cockpit_tour_done') !== '1') tourOpen = true; } catch {}
    // Seguir el tema del SO cuando themeKey === 'auto'.
    try {
      const mq = window.matchMedia('(prefers-color-scheme: light)');
      _osLight = mq.matches;
      _mqTheme = (e) => { _osLight = e.matches; };
      mq.addEventListener('change', _mqTheme);
      _mqRef = mq;
    } catch {}
    // WebView2 pauses compositing when idle, so live DOM updates (streaming
    // shell output, polling) only paint after a mouse move "wakes" it. A
    // self-sustaining rAF loop keeps the frame loop alive while the cockpit is
    // open, so updates paint continuously without user interaction.
    const pump = () => { _raf = requestAnimationFrame(pump); };
    _raf = requestAnimationFrame(pump);
    return () => { cancelDemo(); if (_raf) cancelAnimationFrame(_raf); try { _mqRef?.removeEventListener('change', _mqTheme); } catch {} };
  });
  function replayDemo() {
    cancelDemo();
    cancelDemo = runCockpitDemo();
  }

</script>

<div class="lucy-v2 cockpit" data-theme={effectiveTheme} style={accentVars}>

  <!-- data-tauri-drag-region: makes the titlebar a window-drag handle. Tauri 2 /
       WebView2 needs this explicit attribute (the -webkit-app-region CSS alone is
       ignored); interactive children opt out with data-tauri-drag-region="false". -->
  <header class="titlebar" data-tauri-drag-region>
    <span class="brand"><Sparkles size={16} stroke={1.75} /> Lucy</span>
    <span class="sep"></span>
    <span class="tab">{activeLabel}<span class="tab-badge">cockpit</span></span>
    {#if $agentStatus.running}
      <span class="run-chip"><span class="run-ring" aria-hidden="true"></span>{$agentStatus.stepIndex > 0 ? `Ejecutando · paso ${$agentStatus.stepIndex}` : 'Ejecutando…'}</span>
    {/if}
    <span class="tb-right">
      {#if activeModule === 'terminal'}
        {#if !live}<button class="pill-btn" data-tauri-drag-region="false" onclick={replayDemo}><Refresh size={13} stroke={1.75} /> reproducir demo</button>{/if}
        <button class="pill-btn icon" data-tauri-drag-region="false" onclick={() => (workspaceCollapsed = !workspaceCollapsed)} aria-label="Alternar workspace"><LayoutSidebarRight size={15} stroke={1.75} /></button>
      {/if}
      <button class="pill-btn icon" data-tauri-drag-region="false" onclick={() => (tourOpen = true)} aria-label={t('Tutorial del cockpit', 'Cockpit tutorial')} title={t('Tutorial del cockpit', 'Cockpit tutorial')}><Help size={15} stroke={1.75} /></button>
      <span class="win">
        <button class="win-btn" data-tauri-drag-region="false" onclick={winMin} aria-label="Minimizar"><Minus size={15} stroke={1.75} /></button>
        <button class="win-btn" data-tauri-drag-region="false" onclick={winMax} aria-label="Maximizar"><Square size={13} stroke={1.75} /></button>
        <button class="win-btn close" data-tauri-drag-region="false" onclick={winClose} aria-label="Cerrar"><X size={15} stroke={1.75} /></button>
      </span>
    </span>
  </header>

  <div class="body">

    <nav class="rail" aria-label="Módulos">
      {#each rail as item}
        {@const Icon = item.Icon}
        <button
          class="rail-btn"
          class:active={activeModule === item.id}
          aria-label={item.label}
          aria-current={activeModule === item.id ? 'page' : undefined}
          onclick={() => (activeModule = item.id)}
        >
          <Icon size={19} stroke={1.75} />
          {#if item.id === 'terminal' && $agentStatus.running}<span class="rail-pulse" aria-hidden="true"></span>{/if}
          <!-- The name is permanently visible, so the hover tooltip that used
               to live here is gone: a tooltip repeating text already on screen
               is noise, and it hid the module names behind a hover the user had
               to perform eight times to learn the rail. -->
          <span class="rail-label">{item.label}</span>
        </button>
      {/each}
      <span class="rail-spacer"></span>
      <div class="rail-avatar" aria-label={userName || 'Usuario'} title={userName || undefined}>{userInitials}</div>
    </nav>

    {#if activeModule === 'terminal'}
      <section class="conversation" aria-label="Conversación">
        <!-- v1.7.234: NO gatear en `tabs.length` — con 0 pestañas el «+» quedaba
             oculto y no había forma de crear una desde el cockpit (fresh install
             sin welcome de V1 = callejón sin salida). El «+» debe estar siempre. -->
        {#if live && onSelectTab}
          <div class="conv-tabs" role="tablist" aria-label="Terminales">
            {#each tabs as t (t.id)}
              <div class="conv-tab" class:on={t.id === activeTabId}>
                <button class="ct-hit" role="tab" aria-selected={t.id === activeTabId} title={t.title} onclick={() => onSelectTab?.(t.id)}>
                  <Message2 size={12} stroke={1.75} />
                  <span class="ct-title">{t.title}</span>
                </button>
                {#if tabs.length > 1}
                  <button class="ct-close" title="Cerrar terminal" aria-label="Cerrar terminal" onclick={() => onCloseTab?.(t.id)}>
                    <X size={12} stroke={2} />
                  </button>
                {/if}
              </div>
            {/each}
            {#if onNewTab}
              <button class="ct-new" title="Nueva terminal" aria-label="Nueva terminal" onclick={() => onNewTab?.()}>
                <Plus size={14} stroke={2} />
              </button>
            {/if}
          </div>
        {/if}
        <div class="lane-head">
          <span class="ck-led" class:on={$agentStatus.running} aria-hidden="true"></span>
          <span class="lane-title ck-lbl">{t('Conversación', 'Conversation')}</span>
          <span class="ck-rule" aria-hidden="true"></span>
          {#if onModelChange}
            <div class="model-picker">
              <button class="model-chip" onclick={() => (modelMenuOpen = !modelMenuOpen)} title="Cambiar modelo">
                <span class="mchip-ic">{currentModelIcon}</span>
                <span class="mchip-lb">{currentModelLabel}</span>
                <ChevronDown size={13} stroke={1.75} />
              </button>
              {#if modelMenuOpen}
                <button class="model-backdrop" onclick={() => (modelMenuOpen = false)} aria-label="Cerrar"></button>
                <div class="model-menu">
                  <input class="model-search" placeholder={t('Buscar modelo…', 'Search model…')} bind:value={modelQuery} />
                  <div class="model-list">
                    {#each modelGroups as grp (grp.provider)}
                      <div class="model-grp">{grp.label}</div>
                      {#each grp.options as it (it.id)}
                        <button class="model-opt" class:sel={it.id === model} onclick={() => pickModel(it.id)}>
                          <span class="mopt-ic">{it.icon}</span>
                          <span class="mopt-nm">{it.name}</span>
                          {#if it.id === model}<Check size={13} stroke={2} />{/if}
                        </button>
                      {/each}
                    {/each}
                    {#if modelGroups.length === 0}<div class="model-none">Sin coincidencias.</div>{/if}
                  </div>
                  <div class="model-foot">
                    <span class="ol-dot" class:on={$ollamaOnline}></span>{$ollamaOnline ? 'Ollama en línea' : 'Ollama offline'}
                    <button class="mrefresh" onclick={refreshLocal}>↻ redetectar</button>
                  </div>
                </div>
              {/if}
            </div>
          {:else}
            <span class="host-chip"><Server size={13} stroke={1.75} /> {model ?? 'Opus 4.8'}</span>
          {/if}
        </div>

        <div class="thread" bind:this={threadEl}>
          {#if $agentConvo.length > 0 || $agentStream}
            {#each $agentConvo as m (m.id)}
              {#if m.role === 'user'}
                <div class="msg user">
                  <div class="avatar user">IV</div>
                  <div class="ubub">
                    {#if m.atts?.length}
                      <!-- Fase A: miniaturas de las imágenes adjuntas al mensaje -->
                      <div class="msg-atts">
                        {#each m.atts as a (a.name)}
                          <img class="msg-att" src={a.previewUrl} alt={a.name} title={a.name} />
                        {/each}
                      </div>
                    {/if}
                    {#if m.text}<div class="bubble">{m.text}</div>{/if}
                    <div class="msg-acts">
                      <button class="ma-btn" title={t('Copiar', 'Copy')} aria-label={t('Copiar', 'Copy')} onclick={() => copyMsg(m)}>{#if _copiedMsg === m.id}<Check size={13} stroke={2} />{:else}<Copy size={13} stroke={1.75} />{/if}</button>
                      <button class="ma-btn" title={t('Editar y reenviar', 'Edit and resend')} aria-label={t('Editar', 'Edit')} onclick={() => editUser(m)}><Pencil size={13} stroke={1.75} /></button>
                    </div>
                  </div>
                </div>
              {:else if m.role === 'thought'}
                <div class="activity thought" class:open={_actOpen[m.id]}>
                  <button class="act-row" onclick={() => (_actOpen[m.id] = !_actOpen[m.id])}>
                    <span class="act-ic"><Brain size={13} stroke={1.75} /></span>
                    <span class="act-lb">{t('Pensó durante', 'Reasoned for')} {(m.dur ?? 0).toFixed(1)} s</span>
                    <span class="act-chev"><ChevronDown size={13} stroke={1.75} /></span>
                  </button>
                  <!-- v1.7.236 — vista previa del razonamiento en el hilo (1 línea,
                       atenuada) cuando está colapsado; el texto completo al desplegar. -->
                  {#if !_actOpen[m.id] && m.text}<div class="act-preview">{previewLines(m.text, 1)}</div>{/if}
                  {#if _actOpen[m.id]}<div class="act-body md">{@html renderMd(m.text, { chips: false })}</div>{/if}
                </div>
              {:else if m.role === 'tool'}
                <div class="activity tool" class:open={_actOpen[m.id]} class:bad={m.ok === false}>
                  <button class="act-row" onclick={() => m.detail && (_actOpen[m.id] = !_actOpen[m.id])}>
                    <span class="act-ic tool-ic">{#if m.kind === 'exec'}<Terminal2 size={13} stroke={1.75} />{:else}<FileText size={13} stroke={1.75} />{/if}</span>
                    <span class="act-lb mono">{m.text}</span>
                    <span class="act-st" class:bad={m.ok === false}>{m.ok === false ? '✕' : '✓'}</span>
                    {#if m.detail}<span class="act-chev"><ChevronDown size={13} stroke={1.75} /></span>{/if}
                  </button>
                  <!-- v1.7.236 — vista previa de la salida en el hilo (2 líneas mono,
                       atenuada) sin desplegar; salida completa al hacer clic. -->
                  {#if m.detail && !_actOpen[m.id]}<div class="act-preview mono">{previewLines(m.detail, 2)}</div>{/if}
                  {#if _actOpen[m.id] && m.detail}<pre class="act-out">{m.detail}</pre>{/if}
                </div>
              {:else}
                <div class="msg lucy" class:acted={!!_reactions[m.id]}>
                  <div class="avatar lucy"><Sparkles size={15} stroke={1.75} /></div>
                  <div class="lucy-body">
                    <div class="lucy-name">Lucy{#if m.ts}<span class="msg-ts">{new Date(m.ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>{/if}</div>
                    <!-- Sanitized markdown (marked → DOMPurify via renderMd); chips off
                         since the cockpit has no classic chip click-handlers. -->
                    <div class="md">{@html renderMd(m.text, { chips: false })}</div>
                    <div class="msg-acts">
                      <button class="ma-btn" title="Copiar" aria-label="Copiar" onclick={() => copyMsg(m)}>{#if _copiedMsg === m.id}<Check size={13} stroke={2} />{:else}<Copy size={13} stroke={1.75} />{/if}</button>
                      {#if live && onRegenerate}<button class="ma-btn" title="Regenerar respuesta" aria-label="Regenerar" onclick={() => onRegenerate?.()}><Refresh size={13} stroke={1.75} /></button>{/if}
                      <button class="ma-btn" class:on={_reactions[m.id] === 'up'} title="Útil" aria-label="Útil" onclick={() => react(m, 'up')}><ThumbUp size={13} stroke={1.75} /></button>
                      <button class="ma-btn" class:down={_reactions[m.id] === 'down'} title="No útil" aria-label="No útil" onclick={() => react(m, 'down')}><ThumbDown size={13} stroke={1.75} /></button>
                    </div>
                  </div>
                </div>
              {/if}
            {/each}
            {#if $agentStream}
              <div class="msg lucy">
                <!-- v1.7.236 — el avatar sigue pulsando mientras escribe (antes
                     solo pulsaba al pensar): la respuesta se siente viva de punta
                     a punta, no muerta en cuanto empieza a llegar texto. -->
                <div class="avatar lucy pulse"><Sparkles size={15} stroke={1.75} /></div>
                <div class="lucy-body">
                  <div class="lucy-name">Lucy</div>
                  <!-- Fase A: el stream vivo también se renderiza como markdown
                       (antes texto plano hasta el mensaje final). renderMd está
                       cacheado y el texto son pocos KB — coste por frame ~1ms.
                       v1.7.234 — `use:morphHtml` en vez de {@html}: {@html}
                       destruye y recrea TODO el subárbol por frame; morphdom
                       hace un diff in-place de la cola de texto → daño mínimo
                       (solo el texto nuevo) en la región visible en vez de un
                       repintado completo. Es el mismo action que usa V1. La
                       clase `stream-live` promueve la burbuja viva a su propia
                       capa de composición para que el present-pump la incluya. -->
                  <div class="md stream-live" use:morphHtml={renderMd($agentStream, { chips: false })}></div>
                </div>
              </div>
            {:else if live && $agentStatus.running}
              <div class="msg lucy">
                <div class="avatar lucy pulse"><Sparkles size={15} stroke={1.75} /></div>
                <div class="lucy-body">
                  <div class="lucy-name">Lucy</div>
                  <div class="think">
                    {#if $agentTrace.length > 0}
                      {@const _tr = $agentTrace[$agentTrace.length - 1]}
                      <!-- v1.7.236 — muestra la ACTIVIDAD real (último trace) en vez
                           de un spinner genérico: la conversación deja de sentirse
                           muerta mientras Lucy trabaja. -->
                      <span class="think-phase phase-{_tr.phase}">{phaseWord(_tr.phase)}</span>
                      <span class="think-txt live">{_tr.label}</span>
                    {:else}
                      <span class="think-txt">{$agentStatus.stepIndex > 0 ? `${t('pensando · paso', 'thinking · step')} ${$agentStatus.stepIndex}` : t('pensando', 'thinking')}</span>
                    {/if}
                    <span class="think-dots"><i></i><i></i><i></i></span>
                  </div>
                </div>
              </div>
            {/if}
          {:else if live}
            <div class="thread-empty">
              <div class="empty-avatar"><img src={LUCY_AVATAR_DATA_URL} alt="Lucy" draggable="false" /></div>
              <div class="empty-title">{greeting}</div>
              <div class="empty-sub">{t('Escribe una orden y Lucy la ejecuta — el plan, la salida y el trace se llenan en el workspace →', 'Type an order and Lucy runs it — plan, output and trace fill the workspace →')}</div>
              <div class="empty-chips">
                {#each SUGGESTIONS as s (s.label)}
                  {@const Icon = s.icon}
                  <button type="button" class="empty-chip" onclick={() => onSubmit?.(s.text)}>
                    <Icon size={14} stroke={1.75} />
                    <span>{s.label}</span>
                  </button>
                {/each}
              </div>
            </div>
          {:else}
            <div class="msg user">
              <div class="avatar user">IV</div>
              <div class="bubble">Diagnostica por qué Ethernet está caído en PROD-LINUX y arréglalo.</div>
            </div>
            <div class="msg lucy">
              <div class="avatar lucy"><Sparkles size={15} stroke={1.75} /></div>
              <div class="lucy-body">
                <div class="lucy-name">Lucy <span class="model-tag">Opus 4.8</span></div>
                <p>Voy a inspeccionar el enlace, revisar el driver y reactivar la interfaz. Puedes seguir cada paso en el workspace →</p>
              </div>
            </div>
          {/if}
        </div>

        <div class="composer-wrap">
          {#if live && slashOpen}
            <div class="slash-pop" role="listbox" aria-label="Comandos">
              {#each slashMatches as s, i (s.cmd)}
                <button
                  type="button" role="option" aria-selected={i === _slashIdx}
                  class="slash-row" class:active={i === _slashIdx}
                  onmouseenter={() => (_slashIdx = i)} onclick={() => pickSlash(s)}
                >
                  <span class="slash-cmd">{s.cmd}</span>
                  <span class="slash-desc">{s.desc}</span>
                </button>
              {/each}
            </div>
          {/if}
          {#if live && attachments.length}
            <div class="attach-row">
              {#each attachments as f (f.name)}
                <div class="attach-chip" title={f.name}>
                  {#if f.type === 'image' && f.previewUrl}
                    <img class="attach-thumb" src={f.previewUrl} alt={f.name} />
                  {:else}
                    <FileText size={13} stroke={1.75} />
                  {/if}
                  <span class="attach-name">{f.name}</span>
                  <button class="attach-x" aria-label="Quitar adjunto" onclick={() => onRemoveAttach?.(f.name)}><X size={12} /></button>
                </div>
              {/each}
            </div>
          {/if}
          <div class="composer">
            {#if live}
              <button class="attach-btn" aria-label={t('Adjuntar archivo', 'Attach file')} title={t('Adjuntar imagen o archivo de texto', 'Attach an image or text file')} onclick={() => onAttach?.()}><Paperclip size={16} stroke={1.75} /></button>
              {#if micSupported}
                <button class="attach-btn mic" class:on={micOn} aria-label={micOn ? t('Detener dictado', 'Stop dictation') : t('Dictar por voz', 'Dictate by voice')} title={micOn ? t('Escuchando… (clic para detener)', 'Listening… (click to stop)') : t('Dictar por voz — Lucy responderá hablando', 'Dictate by voice — Lucy will speak her reply')} onclick={toggleMic}>
                  {#if micOn}<Microphone size={16} stroke={1.75} />{:else}<MicrophoneOff size={16} stroke={1.75} />{/if}
                </button>
              {/if}
            {/if}
            <span class="prompt-glyph" class:live={draft.length > 0} aria-hidden="true">❯</span>
            <textarea class="composer-input" class:cmd={draft.startsWith('/')} rows="1" bind:this={inputEl} bind:value={draft} use:autoGrow={draft} placeholder={t('Escribe una orden…  ·  «/» para comandos  ·  Shift+Enter = salto de línea', 'Type an order…  ·  «/» for commands  ·  Shift+Enter = newline')} onkeydown={onComposerKey}></textarea>
            {#if live && $agentStatus.running}
              <button class="send stop" aria-label={t('Detener', 'Stop')} title={t('Detener la tarea en curso', 'Stop the running task')} onclick={() => onStop?.()}><PlayerStopFilled size={15} /></button>
            {:else}
              <button class="send" aria-label={t('Enviar', 'Send')} onclick={send}><ArrowUp size={16} stroke={2} /></button>
            {/if}
          </div>
        </div>
      </section>

      {#if !workspaceCollapsed}
        <aside class="workspace" style="width:{wsWidth}px" aria-label="Workspace del agente">
          <div
            class="ws-resize" class:dragging={_resizing}
            role="separator" aria-label="Redimensionar workspace" aria-orientation="vertical"
            onpointerdown={startResize} onpointermove={moveResize} onpointerup={endResize} onpointercancel={endResize}
          ></div>
          <AgentWorkspace />
        </aside>
      {/if}
    {:else if activeModule === 'dashboard'}
      <!-- Un insight accionable salta a Terminal IA y lo envía: el operador ve
           la sugerencia y la ejecuta sin cambiar de módulo ni copiar texto. -->
      <div class="view-full"><CockpitDashboard onRunCommand={(text) => { activeModule = 'terminal'; onSubmit?.(text); }} /></div>
    {:else if activeModule === 'nexshell'}
      <!-- NexShell renders in the always-mounted host below (keeps SSH sessions alive) -->
    {:else if activeModule === 'memory'}
      <div class="view-full"><CockpitMemory /></div>
    {:else if activeModule === 'logs'}
      <div class="view-full"><CockpitLogs /></div>
    {:else if activeModule === 'inventory'}
      <div class="view-full"><CockpitInventory /></div>
    {:else if activeModule === 'compliance'}
      <div class="view-full"><CockpitCompliance /></div>
    {:else if activeModule === 'config'}
      <div class="view-full"><CockpitConfig {model} {personality} {onSetPersonality} {onConfigureKeys} {onOpenSettings} {smartRouting} {privacyMode} {onSetPrivacyMode} accent={accentKey} onSetAccent={setAccent} theme={themeKey} onSetTheme={setTheme} /></div>
    {:else}
      {@const M = rail.find((r) => r.id === activeModule)}
      <div class="view-full view-placeholder">
        {#if M}<M.Icon size={30} stroke={1.5} />{/if}
        <p>{activeLabel} llega al cockpit en una próxima fase.</p>
        <span class="ph-note">La vista actual sigue disponible en la UI clásica.</span>
      </div>
    {/if}

    <!-- NexShell host — ALWAYS mounted (only shown when active) so remote
         sessions, streaming output and connection state survive switching
         modules. Other views can safely remount/re-fetch on demand. -->
    <div class="view-full nexshell-host" class:hidden={activeModule !== 'nexshell'}>
      <CockpitNexShell onAdd={onHostAdd} onEdit={onHostEdit} onDelete={onHostDelete} />
    </div>

  </div>

  <footer class="footer">
    <span class="dot-item"><span class="dot" class:running={$agentStatus.running}></span> {$agentStatus.running ? 'Ejecutando…' : hostLabel($localHostname, 'equipo local')}</span>
    <span class="foot-item"><ShieldCheck size={13} stroke={1.75} /> Limpio</span>
    <span class="foot-spacer"></span>
    <span class="foot-item">{$agentStatus.model ?? 'Opus 4.8'}</span>
    <span class="foot-cost">${$agentStatus.costUsd.toFixed(2)}</span>
  </footer>

  <!-- Human-in-the-loop authorization, cockpit-native. Surfaces the SAME
       approve/cancel actions as the classic RunAs modal / SecurityBlock (the
       server-verified bypass-token flow is untouched — this is only the UI). -->
  {#if hitl}
    <div class="hitl-scrim" role="dialog" aria-modal="true" aria-labelledby="hitl-title">
      <div class="hitl-box">
        <div class="hitl-ic"><ShieldCheck size={26} stroke={1.6} /></div>
        <h3 id="hitl-title">{hitl.kind === 'runas' ? t('Comando con privilegios de Administrador', 'Command with Administrator privileges') : t('Comando que requiere autorización', 'Command that requires authorization')}</h3>
        <p class="hitl-sub">
          {hitl.kind === 'runas'
            ? t('Lucy quiere ejecutar esto con elevación de permisos (RunAs). Windows mostrará un aviso UAC. Procede solo si confías en el comando.', 'Lucy wants to run this with elevated permissions (RunAs). Windows will show a UAC prompt. Proceed only if you trust the command.')
            : t('Lucy necesita tu autorización explícita para ejecutar este comando.', 'Lucy needs your explicit authorization to run this command.')}
        </p>
        {#if hitl.rule}<div class="hitl-rule">Regla: <code>{hitl.rule}</code></div>{/if}
        <pre class="hitl-cmd">{hitl.cmd}</pre>
        <div class="hitl-actions">
          <button class="hitl-btn cancel" onclick={() => onHitlCancel?.()}>{t('Cancelar', 'Cancel')}</button>
          <button class="hitl-btn approve" onclick={() => onHitlApprove?.()}>{t('Permitir ejecución', 'Allow execution')}</button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Guided tour (v1.7.233) — steps switch the live module behind the card. -->
  <CockpitTour open={tourOpen} onClose={tourDone} onModule={(m) => { if (m) activeModule = m; }} />

</div>

<style>
  .cockpit :global(*) { box-sizing: border-box; }

  .cockpit {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--surface-0);
    color: var(--text-secondary);
    font-family: var(--font-sans);
    font-size: var(--fs-body);
    overflow: hidden;
  }

  .titlebar {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: 10px 14px;
    background: var(--surface-1);
    border-bottom: 1px solid var(--border);
  }
  .brand { display: flex; align-items: center; gap: 7px; color: var(--accent); font-weight: var(--fw-medium); }
  .sep { width: 1px; height: 14px; background: var(--border-strong); }
  .tab {
    display: flex; align-items: center; gap: 8px;
    font-size: var(--fs-footnote); color: var(--text-primary);
    background: var(--surface-3); padding: 5px 11px; border-radius: var(--r-sm);
  }
  .tab-badge { font-family: var(--font-mono); font-size: var(--fs-micro); letter-spacing: var(--ls-label); text-transform: uppercase; color: var(--accent); background: var(--accent-bg); padding: 2px 7px; border-radius: 6px; }
  .run-chip { display: flex; align-items: center; gap: 7px; font-size: var(--fs-caption); color: var(--accent); background: var(--accent-bg); padding: 3px 10px; border-radius: var(--r-pill); white-space: nowrap; }
  .run-ring { width: 9px; height: 9px; border-radius: var(--r-pill); border: 2px solid var(--accent); border-top-color: transparent; animation: cockpit-spin 0.8s linear infinite; }
  @keyframes cockpit-spin { to { transform: rotate(360deg); } }
  .tb-right { margin-left: auto; display: flex; align-items: center; gap: 10px; }
  .pill-btn {
    display: flex; align-items: center; gap: 5px;
    font-size: var(--fs-caption); color: var(--text-muted);
    background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-sm);
    padding: 4px 9px; cursor: pointer;
    transition: color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-snap);
  }
  .pill-btn.icon { padding: 4px 7px; }
  .pill-btn:hover { color: var(--text-primary); background: var(--surface-3); }
  .pill-btn:active { transform: scale(0.94); }
  .win { display: flex; gap: 2px; }
  .win-btn { display: flex; align-items: center; justify-content: center; width: 30px; height: 26px; border: 0; background: transparent; color: var(--text-faint); border-radius: var(--r-sm); cursor: pointer; transition: color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-snap); }
  .win-btn:hover { color: var(--text-primary); background: var(--surface-3); }
  .win-btn.close:hover { color: #fff; background: var(--danger); }
  .win-btn:active { transform: scale(0.9); }

  .body { display: flex; flex: 1; min-height: 0; }

  .rail {
    width: var(--rail-w);
    background: var(--surface-1);
    border-right: 1px solid var(--border);
    padding: 10px 0;
    /* Gap tightened from 6px: the buttons are ~14px taller now that each
       carries its name, and the window can be as short as 560px (tauri.conf
       minHeight). At 6px the eight modules plus the avatar overflowed the rail
       at that size and pushed the avatar out of view. */
    display: flex; flex-direction: column; align-items: center; gap: 3px;
  }
  .rail-btn {
    position: relative;
    /* Fills the rail minus its gutters so the hit target matches what the eye
       reads as one item — icon and name together, not an icon with a caption. */
    width: calc(var(--rail-w) - 16px);
    border-radius: var(--r-md);
    padding: 7px 3px 6px;
    display: flex; flex-direction: column; align-items: center; gap: 4px;
    background: transparent; border: 0; cursor: pointer;
    color: var(--text-muted);
    transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-snap);
  }
  .rail-btn:hover { background: var(--surface-2); color: var(--text-primary); }
  .rail-btn:active { transform: scale(0.9); }
  .rail-btn.active { background: var(--surface-3); color: var(--accent); box-shadow: inset 2px 0 0 var(--accent); }
  .rail-label {
    /* Sans, not the mono instrument treatment used elsewhere: at 10px the
       tracked monospace makes "Configuración" ~90px, which would force either a
       much wider rail or a truncated word. Sans fits every current label on one
       line inside 84px. */
    font-family: var(--font-sans);
    font-size: 10px;
    line-height: 1.15;
    letter-spacing: 0.01em;
    text-align: center;
    color: inherit;
    /* Safety net for a future label longer than the rail: break the word rather
       than overflow into the conversation lane. */
    max-width: 100%;
    overflow-wrap: anywhere;
  }
  /* Sits on the icon, not the button — the button is now tall enough that a
     corner dot would float away from what it annotates. */
  .rail-pulse { position: absolute; top: 5px; right: calc(50% - 15px); width: 7px; height: 7px; border-radius: var(--r-pill); background: var(--accent); animation: cockpit-pulse 1.6s var(--ease-out) infinite; }
  @keyframes cockpit-pulse { 0% { box-shadow: 0 0 0 0 rgba(61, 214, 164, 0.5); } 70% { box-shadow: 0 0 0 6px rgba(61, 214, 164, 0); } 100% { box-shadow: 0 0 0 0 rgba(61, 214, 164, 0); } }
  .rail-spacer { flex: 1; }
  .rail-avatar {
    width: 30px; height: 30px; border-radius: var(--r-md);
    background: var(--user-bg); color: var(--user);
    display: flex; align-items: center; justify-content: center;
    font-size: var(--fs-footnote); font-weight: var(--fw-medium);
  }

  .conversation {
    flex: 1; min-width: var(--conversation-min);
    display: flex; flex-direction: column;
    background: var(--surface-0);
    animation: view-in var(--dur-base) var(--ease-out);
  }

  /* ── Terminal tabs / conversation history ─────────────────────────── */
  .conv-tabs {
    display: flex; align-items: center; gap: 4px;
    padding: 6px 8px; border-bottom: 1px solid var(--border);
    background: var(--surface-1);
    overflow-x: auto; scrollbar-width: thin;
    scroll-behavior: smooth;
  }
  .conv-tabs::-webkit-scrollbar { height: 5px; }
  .conv-tabs::-webkit-scrollbar-thumb { background: var(--border); border-radius: 3px; }
  .conv-tab {
    display: flex; align-items: center; flex: 0 0 auto;
    max-width: 172px; border-radius: 8px;
    background: var(--surface-2); border: 1px solid transparent;
    transition: background var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out);
  }
  .conv-tab:hover { background: var(--surface-3, var(--surface-2)); }
  .conv-tab.on {
    background: var(--accent-bg); border-color: var(--border-accent);
  }
  .ct-hit {
    display: flex; align-items: center; gap: 6px;
    padding: 5px 4px 5px 9px; min-width: 0;
    background: transparent; border: 0; cursor: pointer;
    font-size: var(--fs-caption); color: var(--text-muted);
    transition: color var(--dur-fast) var(--ease-out);
  }
  .conv-tab.on .ct-hit { color: var(--text-primary); }
  .conv-tab.on .ct-hit :global(svg) { color: var(--accent); }
  .ct-title { max-width: 118px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ct-close {
    display: flex; align-items: center; justify-content: center;
    width: 20px; height: 22px; margin-right: 3px; padding: 0;
    background: transparent; border: 0; border-radius: 6px; cursor: pointer;
    color: var(--text-muted); opacity: 0.55;
    transition: opacity var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out);
  }
  .ct-close:hover { opacity: 1; background: var(--danger-bg, rgba(240,90,90,0.16)); color: var(--danger, #f05a5a); }
  .ct-new {
    display: flex; align-items: center; justify-content: center; flex: 0 0 auto;
    width: 28px; height: 28px; padding: 0; margin-left: 2px;
    background: var(--surface-2); border: 1px solid var(--border); border-radius: 8px;
    color: var(--text-muted); cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-out);
  }
  .ct-new:hover { background: var(--accent-bg); border-color: var(--border-accent); color: var(--accent); }
  .ct-new:active { transform: scale(0.9); }

  .lane-head {
    display: flex; align-items: center; gap: 9px;
    padding: 10px 15px; border-bottom: 1px solid var(--border);
  }
  /* v1.7.235 — label de instrumento (mono, tracked, uppercase) en vez de texto
     plano: la cabecera se lee como chrome de cabina, no como un <span> suelto. */
  .lane-title {
    font-family: var(--font-mono); font-size: var(--fs-micro);
    font-weight: var(--fw-medium); letter-spacing: var(--ls-label);
    text-transform: uppercase; color: var(--text-faint);
  }
  .host-chip {
    margin-left: auto; display: flex; align-items: center; gap: 5px;
    font-size: var(--fs-caption); color: var(--text-muted);
    background: var(--surface-2); padding: 3px 9px; border-radius: 7px;
  }
  .host-chip :global(svg) { color: var(--accent); }

  .model-picker { margin-left: auto; position: relative; }
  .model-chip { display: flex; align-items: center; gap: 6px; max-width: 280px; font-size: var(--fs-caption); color: var(--text-secondary); background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-sm); padding: 4px 9px; cursor: pointer; transition: background var(--dur-fast) var(--ease-out); }
  .model-chip:hover { background: var(--surface-3); }
  .mchip-ic { color: var(--accent); }
  .mchip-lb { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .model-backdrop { position: fixed; inset: 0; z-index: 40; background: transparent; border: 0; cursor: default; }
  .model-menu { position: absolute; top: calc(100% + 6px); right: 0; z-index: 41; width: 300px; max-height: 380px; display: flex; flex-direction: column; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-lg); box-shadow: var(--shadow-pop); overflow: hidden; }
  .model-search { margin: 8px; padding: 7px 10px; background: var(--surface-1); border: 1px solid var(--border-strong); border-radius: var(--r-md); color: var(--text-primary); font-size: var(--fs-footnote); font-family: var(--font-sans); outline: 0; }
  .model-search::placeholder { color: var(--text-faint); }
  .model-list { flex: 1; overflow-y: auto; padding: 0 6px 6px; display: flex; flex-direction: column; gap: 1px; }
  .model-grp { padding: 9px 9px 4px; font-size: var(--fs-caption); font-weight: var(--fw-medium); color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.04em; }
  .model-opt { display: flex; align-items: center; gap: 9px; padding: 7px 9px; border: 0; background: transparent; border-radius: var(--r-sm); cursor: pointer; text-align: left; color: var(--text-secondary); font-size: var(--fs-footnote); transition: background var(--dur-fast) var(--ease-out); }
  .model-opt:hover { background: var(--surface-3); }
  .model-opt.sel { color: var(--accent); }
  .mopt-ic { color: var(--text-muted); width: 15px; text-align: center; flex-shrink: 0; }
  .model-opt.sel .mopt-ic { color: var(--accent); }
  .mopt-nm { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .model-none { padding: 14px; text-align: center; font-size: var(--fs-caption); color: var(--text-faint); }
  .model-foot { display: flex; align-items: center; gap: 7px; padding: 8px 11px; border-top: 1px solid var(--border); font-size: var(--fs-caption); color: var(--text-muted); }
  .ol-dot { width: 7px; height: 7px; border-radius: var(--r-pill); background: var(--text-disabled); flex-shrink: 0; }
  .ol-dot.on { background: var(--accent); }
  .mrefresh { margin-left: auto; background: transparent; border: 0; color: var(--accent); cursor: pointer; font-size: var(--fs-caption); }

  /* v1.7.235 "instrumento premium" — columna de lectura anclada: el contenido
     vive en una banda centrada de ~820px en vez de flotar a lo ancho de todo el
     carril (la causa #1 del look "plantilla vacía"). Se centra con PADDING
     calculado — no con max-width por hijo — para que .activity conserve su
     indentación y el scrollbar quede en el borde del carril. */
  .thread { flex: 1; overflow-y: auto; padding: 20px max(18px, calc((100% - 820px) / 2)); display: flex; flex-direction: column; gap: 16px; scroll-behavior: smooth; }
  .msg { display: flex; gap: 10px; animation: msg-in var(--dur-base) var(--ease-out) both; }
  .msg.user { flex-direction: row-reverse; animation-name: msg-in-r; }
  .msg.lucy { animation-name: msg-in-l; }
  @keyframes msg-in   { from { opacity: 0; transform: translateY(8px); }   to { opacity: 1; transform: none; } }
  @keyframes msg-in-r { from { opacity: 0; transform: translateX(16px); }  to { opacity: 1; transform: none; } }
  @keyframes msg-in-l { from { opacity: 0; transform: translateX(-16px); } to { opacity: 1; transform: none; } }
  .avatar {
    width: 28px; height: 28px; border-radius: var(--r-sm); flex-shrink: 0;
    display: flex; align-items: center; justify-content: center;
    font-size: var(--fs-caption); font-weight: var(--fw-medium);
    animation: avatar-pop var(--dur-base) var(--ease-snap) both;
  }
  @keyframes avatar-pop { from { opacity: 0; transform: scale(0.5); } to { opacity: 1; transform: scale(1); } }
  .avatar.user { background: var(--user-bg); color: var(--user); }
  .avatar.lucy { background: var(--accent-bg); color: var(--accent); }
  /* Lucy's avatar pulses a ring while she's thinking. */
  .avatar.pulse { animation: avatar-pop var(--dur-base) var(--ease-snap) both, avatar-ring 1.6s var(--ease-out) infinite; }
  @keyframes avatar-ring { 0% { box-shadow: 0 0 0 0 rgba(61,214,164,0.4); } 70% { box-shadow: 0 0 0 7px rgba(61,214,164,0); } 100% { box-shadow: 0 0 0 0 rgba(61,214,164,0); } }
  /* "Pensando…" typing indicator (three bouncing dots). */
  .think { display: flex; align-items: center; gap: 9px; padding: 2px 0; }
  .think-txt { font-size: var(--fs-footnote); color: var(--text-muted); font-style: italic; }
  /* v1.7.236 — chip de fase + label de actividad viva en el indicador de trabajo. */
  .think-phase {
    flex: 0 0 auto; font-family: var(--font-mono); font-size: var(--fs-micro); font-weight: 600;
    text-transform: uppercase; letter-spacing: 0.06em;
    padding: 1px 7px; border-radius: 999px; background: var(--accent-bg); color: var(--accent);
  }
  .think-phase.phase-think { background: var(--user-bg); color: var(--user); }
  .think-phase.phase-react { background: var(--surface-3, var(--surface-2)); color: var(--text-secondary); }
  .think-phase.phase-info  { background: var(--surface-3, var(--surface-2)); color: var(--text-muted); }
  .think-txt.live {
    font-style: normal; color: var(--text-secondary);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 440px;
  }
  .think-dots { display: inline-flex; gap: 4px; }
  .think-dots i { width: 5px; height: 5px; border-radius: 50%; background: var(--accent); animation: think-bounce 1.2s ease-in-out infinite; }
  .think-dots i:nth-child(2) { animation-delay: 0.18s; }
  .think-dots i:nth-child(3) { animation-delay: 0.36s; }
  @keyframes think-bounce { 0%, 60%, 100% { opacity: 0.35; transform: translateY(0); } 30% { opacity: 1; transform: translateY(-4px); } }

  /* Inline activity — collapsible reasoning ("thought") + tool-call cards,
     indented under Lucy (avatar width + gap). Lighter than message bubbles. */
  .activity { margin-left: 38px; border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-2); overflow: hidden; animation: msg-in-l var(--dur-base) var(--ease-out) both; }
  .act-row { display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 10px; background: transparent; border: 0; cursor: pointer; text-align: left; }
  .act-row:hover { background: var(--surface-3); }
  .act-ic { display: flex; flex-shrink: 0; color: var(--text-faint); }
  .activity.thought .act-ic { color: var(--user); }
  .tool-ic { color: var(--accent); }
  .act-lb { flex: 1; min-width: 0; font-size: var(--fs-caption); color: var(--text-secondary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .act-lb.mono { font-family: var(--font-mono); }
  .act-st { flex-shrink: 0; font-size: var(--fs-caption); color: var(--success); }
  .act-st.bad { color: var(--danger); }
  .act-chev { display: flex; flex-shrink: 0; color: var(--text-faint); transition: transform var(--dur-fast) var(--ease-out); }
  .activity.open .act-chev { transform: rotate(180deg); }
  .act-body { padding: 6px 12px 10px; border-top: 1px solid var(--border); }
  .act-out { margin: 0; border-top: 1px solid var(--border); background: rgba(0, 0, 0, 0.28); padding: 8px 11px; max-height: 220px; overflow: auto; font-family: var(--font-mono); font-size: 11px; line-height: 1.5; color: var(--text-muted); white-space: pre-wrap; overflow-wrap: anywhere; }
  /* v1.7.236 — vista previa inline (colapsada) de razonamiento/salida: el hilo
     enseña un vistazo real de la actividad sin obligar a desplegar. */
  .act-preview {
    padding: 4px 11px 7px 31px; font-size: 11.5px; line-height: 1.45;
    color: var(--text-faint); overflow: hidden; overflow-wrap: anywhere;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical;
  }
  .act-preview.mono { font-family: var(--font-mono); font-size: 11px; }
  .activity.tool.bad { border-color: color-mix(in srgb, var(--danger) 45%, var(--border)); }
  .activity.tool.bad .act-preview { color: color-mix(in srgb, var(--danger) 75%, var(--text-faint)); }
  .bubble {
    font-size: 13.5px; line-height: 1.55; color: var(--text-primary);
    background: var(--surface-2); border: 1px solid var(--border);
    padding: 9px 12px; border-radius: var(--r-lg); max-width: 100%;
  }
  .lucy-body { min-width: 0; }
  .lucy-name { display: flex; align-items: center; gap: 7px; font-size: var(--fs-footnote); font-weight: var(--fw-medium); color: var(--text-primary); margin-bottom: 5px; }
  /* v1.7.236 iter-2 — hora mono tabular junto al nombre: micro-detalle de
     producto real (el hilo deja de ser burbujas sin tiempo). */
  .msg-ts { font-family: var(--font-mono); font-variant-numeric: tabular-nums; font-size: var(--fs-micro); font-weight: var(--fw-regular); color: var(--text-faint); letter-spacing: 0.04em; }
  /* Per-message actions (copy / regenerate / edit / 👍👎) — revealed on hover. */
  .ubub { display: flex; flex-direction: column; align-items: flex-end; min-width: 0; max-width: 76%; }
  .msg-atts { display: flex; flex-wrap: wrap; gap: 6px; justify-content: flex-end; margin-bottom: 6px; }
  .msg-att {
    max-width: 180px; max-height: 130px; border-radius: var(--r-md);
    border: 1px solid var(--border-strong); object-fit: cover;
    animation: msg-in-r var(--dur-slow) var(--ease-out);
  }
  .msg-acts { display: flex; gap: 2px; margin-top: 4px; opacity: 0; transition: opacity var(--dur-fast) var(--ease-out); }
  .msg:hover .msg-acts, .msg.acted .msg-acts { opacity: 1; }
  .ma-btn { display: flex; align-items: center; justify-content: center; width: 24px; height: 24px; padding: 0; border: 0; border-radius: 6px; background: transparent; color: var(--text-faint); cursor: pointer; transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-snap); }
  .ma-btn:hover { background: var(--surface-3); color: var(--text-primary); }
  .ma-btn:active { transform: scale(0.88); }
  .ma-btn.on { color: var(--accent); }
  .ma-btn.down { color: var(--danger); }
  .model-tag { font-size: var(--fs-caption); color: var(--text-muted); background: var(--surface-2); padding: 2px 6px; border-radius: 6px; font-weight: var(--fw-regular); }
  .lucy-body p { margin: 0; font-size: 13.5px; line-height: 1.7; color: var(--text-secondary); white-space: pre-wrap; }

  /* Rendered Markdown of a Lucy reply (renderMd → marked → DOMPurify). Its
     descendants are injected via {@html} so they carry NO Svelte scope hash —
     they must be targeted with :global(), confined to the scoped `.md`. */
  /* v1.7.236 iter-2 — la prosa de Lucy sube medio punto (13 → 13.5px) con
     interlineado 1.7: el contenido principal del cockpit debe leerse cómodo,
     no al mismo tamaño que el chrome. */
  .md { font-size: 13.5px; line-height: 1.7; color: var(--text-secondary); }
  .md > :global(:first-child) { margin-top: 0; }
  .md > :global(:last-child) { margin-bottom: 0; }
  .md :global(p) { margin: 0 0 8px; }
  .md :global(h1), .md :global(h2), .md :global(h3), .md :global(h4) { margin: 14px 0 6px; color: var(--text-primary); font-weight: var(--fw-medium); line-height: 1.3; }
  .md :global(h1) { font-size: var(--fs-title); }
  .md :global(h2) { font-size: var(--fs-body); }
  .md :global(h3), .md :global(h4) { font-size: var(--fs-footnote); }
  .md :global(ul), .md :global(ol) { margin: 4px 0 8px; padding-left: 20px; }
  .md :global(li) { margin: 2px 0; }
  .md :global(li::marker) { color: var(--text-faint); }
  .md :global(a) { color: var(--accent); text-decoration: none; }
  .md :global(a:hover) { text-decoration: underline; }
  .md :global(strong) { color: var(--text-primary); font-weight: var(--fw-medium); }
  .md :global(hr) { border: 0; border-top: 1px solid var(--border); margin: 12px 0; }
  .md :global(blockquote) { margin: 8px 0; padding: 2px 12px; border-left: 2px solid var(--border-accent); color: var(--text-muted); }
  .md :global(code) { font-family: var(--font-mono); font-size: 0.86em; background: var(--surface-3); padding: 1px 5px; border-radius: 5px; color: var(--text-primary); }
  .md :global(pre) { margin: 8px 0; padding: 9px 11px; background: rgba(0, 0, 0, 0.28); border: 1px solid var(--border); border-left: 2px solid var(--accent); border-radius: var(--r-sm); overflow-x: auto; }
  .md :global(pre code) { display: block; padding: 0; background: transparent; border-radius: 0; font-size: 12px; line-height: 1.55; color: var(--text-primary); white-space: pre; }
  .md :global(table) { border-collapse: collapse; margin: 8px 0; font-size: var(--fs-caption); display: block; overflow-x: auto; }
  .md :global(th), .md :global(td) { border: 1px solid var(--border); padding: 5px 9px; text-align: left; }
  .md :global(th) { background: var(--surface-1); color: var(--text-primary); font-weight: var(--fw-medium); }
  /* v1.7.236 — cursor de escritura INLINE al final del texto en curso. Antes un
     <span> suelto caía en su propia línea DEBAJO del párrafo (se veía flotando).
     Ahora es un ::after sobre el ÚLTIMO bloque del markdown en vivo → parpadea
     justo tras la última palabra, como un cursor de terminal real. El último
     bloque además pierde su margen inferior para que el cursor lo abrace. */
  /* :global — los hijos del markdown los inserta morphHtml (fuera del template),
     así que el scoping de Svelte no los alcanza. Acotado a .lucy-v2 .stream-live
     para que jamás toque otra cosa. */
  :global(.lucy-v2 .stream-live > *:last-child) { margin-bottom: 0; }
  :global(.lucy-v2 .stream-live > *:last-child)::after {
    content: ''; display: inline-block; width: 6px; height: 1em;
    margin-left: 3px; vertical-align: -0.14em; border-radius: 1px;
    background: var(--accent); box-shadow: 0 0 6px var(--accent-line);
    animation: cockpit-blink 1s steps(2, start) infinite;
  }
  /* -global- para que la regla :global() de arriba resuelva el keyframe. */
  @keyframes -global-cockpit-blink { 50% { opacity: 0; } }
  /* v1.7.234 — la burbuja que está escribiendo vive en su propia capa de
     composición: el present-pump (ver +page) fuerza el swap de superficie
     completa y esta capa se incluye limpiamente en él. Solo la burbuja VIVA
     (transitoria) se promueve, no cada mensaje, así que el coste en memoria de
     capa es de una sola. */
  .stream-live { will-change: transform; transform: translateZ(0); }

  /* Centrado con la misma banda de ~820px que .thread — la conversación y el
     composer comparten eje, la composición deja de "flotar". */
  .composer-wrap { padding: 11px max(15px, calc((100% - 820px) / 2)) 14px; position: relative; }

  /* ── Slash-command palette (floats above the composer) ──────────────────── */
  .slash-pop {
    position: absolute; bottom: calc(100% - 6px);
    left: max(15px, calc((100% - 820px) / 2)); right: max(15px, calc((100% - 820px) / 2));
    z-index: 5; max-height: 246px; overflow-y: auto;
    background: var(--surface-1); border: 1px solid var(--border-strong);
    border-radius: var(--r-lg); padding: 5px;
    box-shadow: 0 -6px 24px -8px rgba(0, 0, 0, 0.5);
    animation: slash-rise var(--dur-base) var(--ease-out);
  }
  @keyframes slash-rise { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: none; } }
  .slash-row {
    width: 100%; display: flex; align-items: baseline; gap: 9px;
    padding: 7px 9px; border: 0; border-radius: var(--r-md); cursor: pointer;
    background: transparent; text-align: left; color: var(--text-primary);
    transition: background var(--dur-fast) var(--ease-out);
  }
  .slash-row.active { background: var(--accent-bg); }
  .slash-cmd { font-family: var(--mono, ui-monospace, monospace); font-size: var(--fs-footnote); font-weight: 600; color: var(--accent); flex: 0 0 auto; }
  .slash-desc { font-size: var(--fs-caption); color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .composer {
    display: flex; align-items: flex-end; gap: 9px;
    background: var(--surface-2); border: 1px solid var(--border-strong);
    border-radius: var(--r-lg); padding: 7px 11px;
    transition: border-color var(--dur-base) var(--ease-out), box-shadow var(--dur-base) var(--ease-out);
  }
  .composer:focus-within { border-color: var(--border-accent); box-shadow: 0 0 0 3px var(--accent-bg); }
  /* v1.7.236 iter — textarea multilínea: crece con el contenido (action autoGrow
     lo cabecea a 150px y activa scroll). resize:none quita el asa de arrastre. */
  .composer-input {
    flex: 1; min-width: 0; background: transparent; border: 0; outline: 0;
    color: var(--text-primary); font-size: 13.5px; font-family: var(--font-sans);
    line-height: 1.5; resize: none; overflow-y: hidden; max-height: 150px;
    padding: 5px 0; margin: 0; display: block;
  }
  .composer-input:focus { outline: 0; box-shadow: none; }
  .composer-input::placeholder { color: var(--text-faint); }
  /* v1.7.235 — instrumento: glifo de prompt tipo terminal. Apagado en reposo,
     se enciende (accent + glow) en cuanto hay texto. */
  .prompt-glyph {
    flex: 0 0 auto; user-select: none;
    font-family: var(--font-mono); font-size: 13.5px; font-weight: 600;
    color: var(--text-disabled); margin-left: 2px;
    /* v1.7.236 — align to the composer text baseline. The row is `align-items:
       flex-end`, so the glyph must share the input's vertical box (line-height +
       padding) or it sinks ~5px below the first line (reported as "› descuadrado").
       Matching line-height/padding puts both bottom edges — and baselines — level. */
    line-height: 1.5; padding: 5px 0;
    transition: color var(--dur-fast) var(--ease-out), text-shadow var(--dur-fast) var(--ease-out);
  }
  .prompt-glyph.live { color: var(--accent); text-shadow: 0 0 8px var(--accent-line); }
  /* Modo comando: al teclear «/» el input cambia a mono — feedback inmediato de
     que estás hablando con la máquina, no con Lucy. */
  .composer-input.cmd { font-family: var(--font-mono); font-size: 12.5px; color: var(--accent); }

  /* ── Attachments (composer #4) ──────────────────────────────────────────── */
  .attach-btn {
    flex: 0 0 auto; width: 29px; height: 29px; border-radius: var(--r-md); border: 0; cursor: pointer;
    background: transparent; color: var(--text-muted);
    display: flex; align-items: center; justify-content: center;
    transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out);
  }
  .attach-btn:hover { background: var(--surface-3, var(--surface-2)); color: var(--accent); }
  .attach-btn.mic.on { color: var(--danger, #f06e6e); animation: mic-pulse 1.4s var(--ease-out) infinite; }
  @keyframes mic-pulse { 0%, 100% { box-shadow: 0 0 0 0 rgba(240,110,110,0.35); } 50% { box-shadow: 0 0 0 5px rgba(240,110,110,0); } }
  .attach-row { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 8px; }
  .attach-chip {
    display: inline-flex; align-items: center; gap: 6px; max-width: 210px;
    padding: 4px 7px 4px 5px; border-radius: var(--r-md);
    background: var(--surface-2); border: 1px solid var(--border-strong);
    color: var(--text-secondary); font-size: var(--fs-caption);
    animation: slash-rise var(--dur-base) var(--ease-out);
  }
  .attach-chip > :global(svg) { color: var(--accent); flex: 0 0 auto; }
  .attach-thumb { width: 22px; height: 22px; border-radius: 4px; object-fit: cover; flex: 0 0 auto; }
  .attach-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .attach-x {
    flex: 0 0 auto; width: 17px; height: 17px; border-radius: 4px; border: 0; cursor: pointer;
    background: transparent; color: var(--text-faint);
    display: flex; align-items: center; justify-content: center;
    transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out);
  }
  .attach-x:hover { background: var(--danger-bg, rgba(239, 68, 68, 0.12)); color: var(--danger, #ef4444); }
  .thread-empty {
    margin: auto; max-width: 380px; text-align: center;
    display: flex; flex-direction: column; align-items: center; gap: 4px;
    animation: msg-in-l var(--dur-slow) var(--ease-out);
  }
  .empty-avatar {
    width: 66px; height: 66px; margin-bottom: 12px; border-radius: 50%;
    position: relative; overflow: hidden;
    border: 1.5px solid var(--accent-line);
    box-shadow: 0 0 0 6px var(--accent-bg), var(--shadow-pop);
    animation: empty-avatar-in var(--dur-slow) var(--ease-out) both;
  }
  .empty-avatar img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .empty-avatar::after {
    content: ''; position: absolute; inset: -1.5px; border-radius: 50%;
    box-shadow: 0 0 20px 2px var(--accent-line); pointer-events: none;
    animation: empty-avatar-glow 3.6s var(--ease-out) infinite;
  }
  @keyframes empty-avatar-in {
    from { opacity: 0; transform: scale(0.82) translateY(8px); }
    to   { opacity: 1; transform: none; }
  }
  @keyframes empty-avatar-glow { 0%, 100% { opacity: 0.35; } 50% { opacity: 0.8; } }
  .empty-title { font-size: var(--fs-title); font-weight: 600; color: var(--text-primary); letter-spacing: -0.01em; }
  .empty-sub { font-size: var(--fs-footnote); line-height: var(--lh-body); color: var(--text-muted); }
  .empty-chips { display: flex; flex-wrap: wrap; justify-content: center; gap: 7px; margin-top: 12px; }
  .empty-chip {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 7px 12px; border-radius: 999px; cursor: pointer;
    background: var(--surface-2); border: 1px solid var(--border-strong);
    color: var(--text-secondary); font-size: var(--fs-caption); font-family: var(--font-sans);
    transition: border-color var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-out);
  }
  .empty-chip:hover { border-color: var(--border-accent); color: var(--text-primary); background: var(--accent-bg); transform: translateY(-1px); }
  .empty-chip :global(svg) { color: var(--accent); flex: 0 0 auto; }
  .send {
    width: 29px; height: 29px; border-radius: var(--r-md); border: 0; cursor: pointer;
    background: var(--accent); color: var(--accent-ink);
    display: flex; align-items: center; justify-content: center;
    transition: background var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-snap);
  }
  .send:hover { background: var(--accent-hover); }
  .send:active { transform: scale(0.9); }
  /* While a task runs the send button becomes a STOP button. */
  .send.stop { background: var(--danger); color: #fff; }
  .send.stop:hover { background: var(--danger); filter: brightness(1.12); }

  /* ── HITL authorization panel (cockpit-native) ────────────────────────── */
  .hitl-scrim {
    position: fixed; inset: 0; z-index: 60;
    background: rgba(2, 6, 12, 0.74); backdrop-filter: blur(5px);
    display: flex; align-items: center; justify-content: center; padding: 24px;
    animation: view-in var(--dur-base) var(--ease-out);
  }
  .hitl-box {
    width: min(460px, 92vw); padding: 22px 22px 18px;
    background: var(--surface-1); border: 1px solid var(--warning, #f0b450);
    border-radius: var(--r-md); text-align: center; box-shadow: var(--shadow-pop);
  }
  .hitl-ic { display: flex; justify-content: center; margin-bottom: 10px; color: var(--warning, #f0b450); }
  .hitl-box h3 { margin: 0 0 6px; font-size: var(--fs-body); font-weight: var(--fw-medium); color: var(--text-primary); }
  .hitl-sub { margin: 0 0 14px; font-size: var(--fs-footnote); line-height: var(--lh-body); color: var(--text-muted); }
  .hitl-rule { margin: 0 0 8px; font-size: var(--fs-caption); color: var(--text-muted); }
  .hitl-rule code { font-family: var(--font-mono); color: var(--warning, #f0b450); }
  .hitl-cmd {
    margin: 0 0 18px; padding: 10px 12px; text-align: left;
    background: rgba(240, 180, 80, 0.06); border: 1px solid rgba(240, 180, 80, 0.22);
    border-radius: var(--r-sm); font-family: var(--font-mono); font-size: 11px;
    line-height: 1.5; color: #d8b57a; max-height: 130px; overflow: auto;
    white-space: pre-wrap; word-break: break-word;
  }
  .hitl-actions { display: flex; gap: 10px; justify-content: center; }
  .hitl-btn {
    padding: 8px 16px; border-radius: var(--r-sm); border: 1px solid transparent;
    font-size: var(--fs-footnote); font-weight: var(--fw-medium); cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out), filter var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-snap);
  }
  .hitl-btn:active { transform: scale(0.96); }
  .hitl-btn.cancel { background: var(--surface-3); color: var(--text-secondary); border-color: var(--border); }
  .hitl-btn.cancel:hover { background: var(--surface-2); color: var(--text-primary); }
  .hitl-btn.approve { background: var(--warning, #f0b450); color: #1a1206; }
  .hitl-btn.approve:hover { filter: brightness(1.08); }

  .workspace { position: relative; width: var(--workspace-w); background: var(--surface-workspace); border-left: 1px solid var(--border); display: flex; flex-direction: column; }
  .ws-resize { position: absolute; left: -3px; top: 0; bottom: 0; width: 7px; cursor: col-resize; z-index: 5; touch-action: none; }
  .ws-resize::after { content: ''; position: absolute; left: 3px; top: 0; bottom: 0; width: 2px; background: transparent; transition: background var(--dur-fast) var(--ease-out); }
  .ws-resize:hover::after, .ws-resize.dragging::after { background: var(--accent); }

  /* View-switch entrance — each router branch mounts fresh when its module
     becomes active, so this replays on every rail switch. The always-mounted
     NexShell host plays it once at boot (persistent session, no re-animation
     on re-show — which is the desired "instant restore" for live shells). */
  @keyframes view-in { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: none; } }
  .view-full { flex: 1; min-width: 0; overflow: hidden; background: var(--surface-0); animation: view-in var(--dur-base) var(--ease-out); }
  .nexshell-host.hidden { display: none; }
  .view-placeholder {
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px;
    color: var(--text-faint); text-align: center; padding: 24px;
  }
  .view-placeholder p { margin: 0; font-size: var(--fs-body); color: var(--text-muted); }
  .ph-note { font-size: var(--fs-caption); color: var(--text-faint); }

  /* v1.7.235 — el footer es TELEMETRÍA, no una barra de texto: mono con
     tracking, cifras tabulares, LEDs. La línea de instrumentos de la cabina. */
  .footer {
    display: flex; align-items: center; gap: 16px;
    padding: 7px 16px; background: var(--surface-1); border-top: 1px solid var(--border);
    font-family: var(--font-mono); font-size: var(--fs-micro);
    letter-spacing: 0.05em; color: var(--text-faint);
    font-variant-numeric: tabular-nums;
  }
  .dot-item, .foot-item { display: flex; align-items: center; gap: 6px; }
  .dot { width: 6px; height: 6px; border-radius: var(--r-pill); background: var(--accent); box-shadow: 0 0 7px var(--accent-line); }
  .dot.running { animation: cockpit-pulse 1.6s var(--ease-out) infinite; }
  .foot-spacer { flex: 1; }
  .foot-cost { color: var(--text-muted); }

  /* Accessibility — honor the OS "reduce motion" setting. Scoped to .lucy-v2 so
     it never touches the classic UI. Neutralizes entrance keyframes (view-in,
     msg-in), the running-pulse/spin/blink loops, and press/hover transforms,
     while leaving instantaneous color/opacity state changes intact. */
  @media (prefers-reduced-motion: reduce) {
    .lucy-v2 *,
    .lucy-v2 *::before,
    .lucy-v2 *::after {
      animation-duration: 0.001ms !important;
      animation-iteration-count: 1 !important;
      transition-duration: 0.001ms !important;
      scroll-behavior: auto !important;
    }
    .lucy-v2 .thread { scroll-behavior: auto !important; }
  }
</style>
