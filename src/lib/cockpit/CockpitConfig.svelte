<script>
  /* ==========================================================================
     Lucy 2.0 — Config (cockpit)  ·  Phase F4/views. EDITABLE.
     Real, wired settings:
       • API-key status via `get_configured_providers` (booleans only — the key
         VALUE never crosses IPC; "Configurar" opens the classic provider modal
         through the overlay-yield callback).
       • Spend cap persisted to `lucy_spend_cap_usd` (localStorage; the agent
         loop reads it fresh each run, so editing here takes effect immediately).
       • Lucy personality → +page.svelte callback (updates the live var + LS).
     Guardrails HITL/SSRF/scrubber are backend-enforced invariants (shown as
     "obligatorio"). Additive — does not touch the classic settings UI.
     ========================================================================== */
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Settings from '@tabler/icons-svelte/icons/settings';
  import Cpu from '@tabler/icons-svelte/icons/cpu';
  import Key from '@tabler/icons-svelte/icons/key';
  import Bell from '@tabler/icons-svelte/icons/bell';
  import Plug from '@tabler/icons-svelte/icons/plug';
  import Clock from '@tabler/icons-svelte/icons/clock';
  import { loadMcpSecrets } from '$lib/page/mcp-secrets';
  import ShieldLock from '@tabler/icons-svelte/icons/shield-lock';
  import Database from '@tabler/icons-svelte/icons/database';
  import Palette from '@tabler/icons-svelte/icons/palette';
  import CircleCheck from '@tabler/icons-svelte/icons/circle-check';
  import CircleX from '@tabler/icons-svelte/icons/circle-x';
  import { localModels, ollamaOnline, refreshLocalModels } from '$lib/models.js';
  import { ensureTtsVoices, resolveTtsVoice, speak } from '$lib/voice';

  let { personality = 'balanced', onSetPersonality = undefined, onConfigureKeys = undefined, onOpenSettings = undefined, model = null, accent = 'emerald', onSetAccent = undefined, theme = 'dark', onSetTheme = undefined, smartRouting = false, privacyMode = false, onSetPrivacyMode = undefined } = $props();

  // ── Enrutado: estado REAL, no una etiqueta fija ─────────────────────────────
  // Esta fila mostraba un `<span class="badge accent">Auto</span>` hardcodeado.
  // Si el operador apagaba el enrutamiento inteligente desde la configuración
  // clásica, este panel seguía afirmando "Auto" para siempre: parecía un
  // indicador de estado y no leía ningún estado. Ahora llega por props desde
  // +page.svelte, igual que `personality`, así que refleja el valor vivo.
  //
  // El modo privacidad gana: bloquea TODO el tráfico a Ollama local sin
  // importar el router, de modo que anunciar "Auto" en ese caso sería la misma
  // mentira con otro nombre.
  const routeLabel = $derived(privacyMode ? 'Local' : (smartRouting ? 'Auto' : 'Manual'));
  const routeTitle = $derived(
    privacyMode ? 'Modo privacidad activo: todo el tráfico LLM queda fijado a Ollama local.'
    : smartRouting ? 'Enrutamiento inteligente activo: Lucy elige el modelo por turno según la complejidad.'
    : 'Enrutamiento manual: se usa el modelo seleccionado en el desplegable.'
  );
  const THEME_OPTS = [{ k: 'dark', l: 'Oscuro' }, { k: 'light', l: 'Claro' }, { k: 'auto', l: 'Auto' }];
  const ACCENT_KEYS = ['emerald', 'blue', 'violet', 'amber', 'pink', 'cyan'];

  const localCount = $derived(($localModels || []).filter((m) => m.id !== 'local-custom').length);
  function refreshLocal() { refreshLocalModels().catch(() => {}); }

  // Order matters only for display. Each `match` entry is substring-matched
  // against the configured-credential names, so a provider whose key is stored
  // as `<provider>_api_key` (which is how ai.rs derives it) needs its provider
  // slug listed here or the row reads "sin configurar" with a working key.
  const KEY_PROVIDERS = [
    { name: 'Google Gemini', match: ['gemini', 'google'] },
    { name: 'OpenAI',        match: ['openai', 'gpt'] },
    { name: 'Anthropic',     match: ['anthropic', 'claude'] },
    { name: 'xAI (Grok)',    match: ['xai', 'grok'] },
    { name: 'DeepSeek',      match: ['deepseek'] },
    { name: 'NVIDIA NIM',    match: ['nvidia'] },
    { name: 'Tavily (web)',  match: ['tavily'] },
  ];
  let configured = $state([]);
  const keyOk = (matches) => matches.some((x) => configured.some((p) => String(p).toLowerCase().includes(x)));

  // ── Avisos externos (Telegram / Slack / webhook) ────────────────────────────
  // Vive junto a "Claves API" porque es la misma forma: un secreto en el keyring
  // del SO del que la interfaz solo conoce el ESTADO. El token no vuelve nunca
  // por IPC, así que el formulario escribe pero no puede releer — por eso el
  // campo se limpia tras guardar en vez de mostrar lo guardado.
  const BRIDGE_KINDS = [
    { k: 'telegram', l: 'Telegram', hint: 'Token del bot (@BotFather)', needsTarget: true },
    { k: 'slack',    l: 'Slack',    hint: 'URL del incoming webhook',   needsTarget: false },
    { k: 'webhook',  l: 'Webhook',  hint: 'URL que recibirá el JSON',   needsTarget: false },
  ];
  const SEVERITIES = [
    { k: 'info',     l: 'Todo' },
    { k: 'warning',  l: 'Avisos' },
    { k: 'critical', l: 'Solo críticos' },
  ];
  let bridge = $state({ configured: false, enabled: false, kind: null, min_severity: 'warning' });
  let bForm = $state({ kind: 'telegram', secret: '', target: '', min_severity: 'warning', enabled: true });
  let bBusy = $state(false);
  let bMsg = $state(null);          // { ok: boolean, text: string }
  const bKind = $derived(BRIDGE_KINDS.find((x) => x.k === bForm.kind) ?? BRIDGE_KINDS[0]);

  async function loadBridge() {
    try { bridge = await invoke('notify_bridge_status'); } catch { /* sin backend */ }
  }
  async function saveBridge() {
    bBusy = true; bMsg = null;
    try {
      await invoke('notify_bridge_save', { config: { ...bForm } });
      bForm.secret = ''; bForm.target = '';   // no se puede releer: no fingir que sí
      await loadBridge();
      bMsg = { ok: true, text: 'Canal guardado.' };
    } catch (e) {
      bMsg = { ok: false, text: String(e) };
    } finally { bBusy = false; }
  }
  async function testBridge() {
    bBusy = true; bMsg = null;
    try {
      await invoke('notify_bridge_test');
      bMsg = { ok: true, text: 'Enviado. Revisa el canal.' };
    } catch (e) {
      bMsg = { ok: false, text: String(e) };
    } finally { bBusy = false; }
  }
  async function clearBridge() {
    bBusy = true; bMsg = null;
    try { await invoke('notify_bridge_clear'); await loadBridge(); bMsg = { ok: true, text: 'Canal eliminado.' }; }
    catch (e) { bMsg = { ok: false, text: String(e) }; }
    finally { bBusy = false; }
  }

  // ── Servidores MCP ──────────────────────────────────────────────────────────
  // MCP es LA superficie de integración de Lucy y estaba en cero en el cockpit:
  // el cliente completo (list/upsert/test/discover/call, pool y presupuesto) ya
  // existía y solo se alcanzaba desde la UI clásica. Aquí no se construye
  // ninguna integración — se hacen visibles y verificables las que ya hay.
  //
  // Alta y edición siguen en el modal clásico a propósito: son 872 líneas de
  // formulario (comando, transporte, variables de entorno, secretos) y
  // reimplementarlas para el cockpit duplicaría la superficie que más cuesta
  // mantener. Este panel responde lo que un operador pregunta a diario —
  // ¿qué hay conectado, funciona, y qué herramientas expone?
  let mcpServers = $state([]);
  let mcpBusy = $state(null);      // nombre del servidor en prueba
  let mcpOpen = $state(null);      // nombre del servidor desplegado
  let mcpMsg = $state(null);       // { name, ok, text }

  const mcpToolCount = (s) => (Array.isArray(s?.tools_cache) ? s.tools_cache.length : 0);

  async function loadMcp() {
    try { const l = await invoke('mcp_server_list'); mcpServers = Array.isArray(l) ? l : []; }
    catch { mcpServers = []; }
  }
  /** Prueba y descubrimiento comparten la resolución de secretos: sin ella un
   *  servidor que necesita una API key falla con un error que parece de red. */
  async function mcpRun(cmd, name) {
    mcpBusy = name; mcpMsg = null;
    try {
      const env = await loadMcpSecrets().catch(() => ({}));
      const r = await invoke(cmd, { name, env });
      await loadMcp();
      const tools = cmd === 'mcp_server_discover' ? mcpToolCount(r) : null;
      mcpMsg = { name, ok: true, text: tools != null ? `${tools} herramienta(s) descubierta(s).` : 'Conexión correcta.' };
    } catch (e) {
      mcpMsg = { name, ok: false, text: String(e).slice(0, 200) };
    } finally { mcpBusy = null; }
  }

  // ── Tareas programadas ──────────────────────────────────────────────────────
  // Backend completo (listar, activar, borrar) y cero superficie en V2. Importa
  // más de lo que parece: una tarea programada corre SIN humano delante, así que
  // si nadie puede ver qué hay programado ni su último resultado, el trabajo
  // desatendido es invisible por definición.
  let tasks = $state([]);
  let tasksBusy = $state(null);
  const fmtWhen = (ts) => (ts ? new Date(ts * 1000).toLocaleString('es', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' }) : '—');
  async function loadTasks() {
    try { const l = await invoke('list_scheduled_tasks'); tasks = Array.isArray(l) ? l : []; }
    catch { tasks = []; }
  }
  async function toggleTask(t) {
    tasksBusy = t.id;
    try { await invoke('toggle_scheduled_task', { id: t.id, enabled: !t.enabled }); await loadTasks(); }
    catch {} finally { tasksBusy = null; }
  }
  async function delTask(t) {
    tasksBusy = t.id;
    try { await invoke('delete_scheduled_task', { id: t.id }); await loadTasks(); }
    catch {} finally { tasksBusy = null; }
  }

  // ── Reglas de permisos ──────────────────────────────────────────────────────
  // Es el guardarraíl que decide qué se ejecuta sin preguntar. Mostrarlo junto a
  // los guardarraíles de solo lectura completa la respuesta a "qué puede hacer
  // Lucy sin mi permiso" — hasta ahora la mitad de esa respuesta era inalcanzable.
  let rules = $state([]);
  let rulesBusy = $state(null);
  async function loadRules() {
    try { const l = await invoke('list_permission_rules', { appliesTo: null }); rules = Array.isArray(l) ? l : []; }
    catch { rules = []; }
  }
  async function delRule(r) {
    rulesBusy = r.id;
    try { await invoke('delete_permission_rule', { ruleId: r.id }); await loadRules(); }
    catch {} finally { rulesBusy = null; }
  }

  let spendCap = $state(0);
  function saveSpendCap() { const n = Math.max(0, Number(spendCap) || 0); spendCap = n; try { localStorage.setItem('lucy_spend_cap_usd', String(n)); } catch {} }

  const PERSONAS = [
    { id: 'concise',  label: 'Conciso' },
    { id: 'balanced', label: 'Equilibrado' },
    { id: 'detailed', label: 'Detallado' },
  ];

  const GUARDS = [
    { name: 'HITL en comandos destructivos', desc: 'Confirmación humana antes de ejecutar' },
    { name: 'Guardia SSRF en fetch',          desc: 'Bloquea loopback / RFC1918 / metadata' },
    { name: 'Depurador de secretos',          desc: 'Redacta claves en memoria y auditoría' },
  ];

  const cloudLabel = $derived((String(model || '').split('::')[0].split('/').pop()) || 'Opus 4.8');

  // v1.7.236 — versión DINÁMICA desde Tauri (getVersion lee tauri.conf.json)
  // en vez de un string hardcodeado que se desincroniza cada release.
  let appVersion = $state('1.7.236');
  import { getVersion } from '@tauri-apps/api/app';

  // ── Voz de Lucy (v1.7.235) ──────────────────────────────────────────────────
  // El TTS tomaba la primera voz del idioma (en Windows suele ser "Raúl",
  // masculina). Ahora: default inteligente (neural femenina, ver voice.ts) +
  // este selector para fijar una voz concreta (localStorage lucy_tts_voice).
  const _userLang = (() => { try { return localStorage.getItem('lucy_user_lang') === 'en' ? 'en-US' : 'es-MX'; } catch { return 'es-MX'; } })();
  let ttsVoices = $state([]);        // voces del idioma activo (nombre + lang)
  let ttsChoice = $state('');        // '' = auto (ranking); si no, voice.name fijado
  let ttsAutoName = $state('');      // qué elige el auto — se muestra en la opción
  function saveTtsChoice() {
    try {
      if (ttsChoice) localStorage.setItem('lucy_tts_voice', ttsChoice);
      else localStorage.removeItem('lucy_tts_voice');
    } catch {}
  }
  function testTtsVoice() {
    saveTtsChoice(); // speak() lee la preferencia — probar = oír lo elegido
    const sample = _userLang.startsWith('es')
      ? 'Hola, soy Lucy. Así sonará mi voz cuando te hable.'
      : 'Hi, I am Lucy. This is how my voice will sound.';
    speak(sample, { getActiveLang: () => ({ stt: _userLang, tts: _userLang }) }).catch(() => {});
  }

  // ── Ruta de datos: preguntada, no adivinada ─────────────────────────────────
  // Estas dos filas tenían `%APPDATA%\Lucy` y `lucy.db` escritos a mano. La
  // ruta era FALSA: el `identifier` de tauri.conf.json es `com.lucy.dev`, así
  // que los datos viven en `%APPDATA%\com.lucy.dev`. Quien fuera a buscar su
  // base de datos guiándose por este panel no la encontraba.
  //
  // `db_info` ya devuelve la ruta absoluta resuelta por el backend y la
  // configuración clásica ya la usa — una sola fuente de verdad en vez de una
  // segunda cadena literal que se desincroniza igual que la primera.
  let dbPath = $state('');
  const dbDir  = $derived(dbPath ? dbPath.replace(/[\\/][^\\/]*$/, '') : '');
  const dbFile = $derived(dbPath ? (dbPath.split(/[\\/]/).pop() || '') : '');

  onMount(async () => {
    try { appVersion = await getVersion(); } catch {}
    try { const p = await invoke('get_configured_providers'); if (Array.isArray(p)) configured = p; } catch {}
    // Si falla, las filas muestran «no disponible» — nunca una ruta inventada.
    try { const info = await invoke('db_info'); if (info?.path) dbPath = String(info.path); } catch {}
    await loadBridge();
    await loadMcp();
    await loadTasks();
    await loadRules();
    try { spendCap = parseFloat(localStorage.getItem('lucy_spend_cap_usd') || '0') || 0; } catch {}
    try {
      const all = await ensureTtsVoices();
      const prefix = _userLang.split('-')[0];
      ttsVoices = all.filter((v) => v.lang.startsWith(prefix)).map((v) => ({ name: v.name, lang: v.lang }));
      ttsAutoName = resolveTtsVoice(all, _userLang)?.name || '';
      const pinned = localStorage.getItem('lucy_tts_voice');
      if (pinned && ttsVoices.some((v) => v.name === pinned)) ttsChoice = pinned;
    } catch {}
  });
</script>

<div class="cfg">
  <div class="cfg-head">
    <span class="cfg-title">Configuración</span>
    <span class="src-pill"><Settings size={14} stroke={1.75} /> Lucy v{appVersion}</span>
    <button class="full-btn" onclick={() => onOpenSettings?.()} title="Apariencia · IA · MCP · Sistema · Idioma">Configuración completa →</button>
  </div>

  <div class="grid">
    <!-- Modelos + preferencias -->
    <section class="panel">
      <div class="panel-head"><Cpu size={16} stroke={1.75} /><span class="ck-led" class:on={$ollamaOnline}></span><span class="ck-lbl">Modelos y comportamiento</span><span class="ck-rule" aria-hidden="true"></span></div>
      <div class="rows">
        <div class="row"><span class="row-l">Modelo activo</span><span class="row-v">{cloudLabel}</span></div>
        <div class="row"><span class="row-l">Enrutado</span><span class="row-v"><span class="badge" class:accent={smartRouting || privacyMode} title={routeTitle}>{routeLabel}</span></span></div>
        <!-- Modo privacidad.
             Deliberadamente un control propio y no un tercer estado del selector
             de enrutado: no es una estrategia de enrutado, es un CIERRE. Fija
             todo el tráfico LLM a Ollama local sin importar el router ni el
             modelo elegido. Fundirlo con "Auto / Manual" haría que cambiar de
             estrategia lo desactivara sin querer.
             Es la función de cumplimiento de Lucy y hasta ahora solo existía en
             la configuración clásica: quien usara el cockpit no podía llegar a
             ella. -->
        <div class="row">
          <span class="row-l">Modo privacidad
            <span class="row-hint">todo el tráfico a Ollama local</span>
          </span>
          <span class="row-v seg">
            <button class="seg-btn" class:on={privacyMode} onclick={() => onSetPrivacyMode?.(true)}
              title="Bloquea TODO el tráfico LLM a Ollama local. Para entornos con requisitos de cumplimiento o sin salida a internet.">Activado</button>
            <button class="seg-btn" class:on={!privacyMode} onclick={() => onSetPrivacyMode?.(false)}
              title="Permite modelos en la nube según el enrutado.">Apagado</button>
          </span>
        </div>
        {#if privacyMode && !$ollamaOnline}
          <!-- El cierre sin destino es el peor de los mundos: la nube bloqueada
               y el local ausente. Decirlo aquí, no cuando falle el primer turno. -->
          <div class="row-warn">Ollama no responde. Con el modo privacidad activo, Lucy no tiene ningún modelo disponible.</div>
        {/if}
        <div class="row">
          <span class="row-l">Modelos locales (Ollama)</span>
          <span class="row-v"><span class="ol-dot" class:on={$ollamaOnline}></span>{localCount} detectados<button class="mini-btn" onclick={refreshLocal} title="Redetectar">↻</button></span>
        </div>
        <div class="row">
          <span class="row-l">Límite de gasto / sesión</span>
          <span class="row-v">
            <span class="cap-usd">$</span>
            <input class="cap-input" type="number" min="0" step="0.5" bind:value={spendCap} onchange={saveSpendCap} />
          </span>
        </div>
        <div class="row col">
          <span class="row-l">Personalidad de Lucy</span>
          <div class="seg">
            {#each PERSONAS as p}
              <button class="seg-btn" class:on={personality === p.id} onclick={() => onSetPersonality?.(p.id)}>{p.label}</button>
            {/each}
          </div>
        </div>
        <div class="row col">
          <span class="row-l">Voz de Lucy (TTS)</span>
          <div class="voice-row">
            <select class="voice-sel" bind:value={ttsChoice} onchange={saveTtsChoice} disabled={!ttsVoices.length}>
              <option value="">{ttsAutoName ? `Auto — ${ttsAutoName}` : 'Auto (recomendada)'}</option>
              {#each ttsVoices as v (v.name)}
                <option value={v.name}>{v.name} · {v.lang}</option>
              {/each}
            </select>
            <button class="mini-btn" onclick={testTtsVoice} disabled={!ttsVoices.length} title="Escuchar una muestra">▶ Probar</button>
          </div>
          {#if !ttsVoices.length}
            <div class="voice-hint">No se detectaron voces del sistema para el idioma activo.</div>
          {/if}
        </div>
      </div>
      <div class="note">El límite de gasto detiene el loop automático al cruzar el monto (0 = sin límite).</div>
    </section>

    <!-- Claves API -->
    <section class="panel">
      <div class="panel-head"><Key size={16} stroke={1.75} /> Claves API
        <button class="head-btn" onclick={() => onConfigureKeys?.()}>Configurar</button>
      </div>
      <div class="rows">
        {#each KEY_PROVIDERS as k}
          <div class="row">
            <span class="row-l">{k.name}</span>
            <span class="row-v">
              {#if keyOk(k.match)}
                <span class="kstat ok"><CircleCheck size={14} stroke={1.9} /> configurada</span>
              {:else}
                <span class="kstat no"><CircleX size={14} stroke={1.9} /> sin configurar</span>
              {/if}
            </span>
          </div>
        {/each}
        <div class="note">La clave nunca se muestra ni cruza al frontend — solo su estado.</div>
      </div>
    </section>

    <!-- Avisos externos -->
    <section class="panel">
      <div class="panel-head"><Bell size={16} stroke={1.75} /> Avisos externos
        {#if bridge.configured}
          <span class="kstat ok" style="margin-left:auto"><CircleCheck size={14} stroke={1.9} /> {bridge.kind}</span>
        {:else}
          <span class="kstat no" style="margin-left:auto"><CircleX size={14} stroke={1.9} /> sin configurar</span>
        {/if}
      </div>
      <div class="rows">
        <div class="row col">
          <span class="row-l">Canal</span>
          <div class="seg">
            {#each BRIDGE_KINDS as k}
              <button class="seg-btn" class:on={bForm.kind === k.k} onclick={() => (bForm.kind = k.k)}>{k.l}</button>
            {/each}
          </div>
        </div>

        <div class="row col">
          <span class="row-l">{bKind.hint}</span>
          <!-- type=password: el token no debe quedar legible por encima del hombro.
               Se envía al keyring y no se puede releer desde aquí. -->
          <input class="b-input" type="password" autocomplete="off" spellcheck="false"
            bind:value={bForm.secret}
            placeholder={bridge.configured ? '•••••• (guardado — escribe para reemplazar)' : bKind.hint} />
        </div>

        {#if bKind.needsTarget}
          <div class="row col">
            <span class="row-l">Chat id</span>
            <input class="b-input" type="text" autocomplete="off" bind:value={bForm.target} placeholder="ej. 123456789" />
          </div>
        {/if}

        <div class="row col">
          <span class="row-l">Qué reenviar
            <span class="row-hint">reenviar de todo acaba en silenciarlo</span>
          </span>
          <div class="seg">
            {#each SEVERITIES as s}
              <button class="seg-btn" class:on={bForm.min_severity === s.k} onclick={() => (bForm.min_severity = s.k)}>{s.l}</button>
            {/each}
          </div>
        </div>

        <div class="b-actions">
          <button class="b-btn primary" onclick={saveBridge} disabled={bBusy || !bForm.secret.trim()}>Guardar</button>
          <button class="b-btn" onclick={testBridge} disabled={bBusy || !bridge.configured}>Probar</button>
          {#if bridge.configured}
            <button class="b-btn danger" onclick={clearBridge} disabled={bBusy}>Quitar</button>
          {/if}
        </div>

        {#if bMsg}
          <div class="b-msg" class:bad={!bMsg.ok}>{bMsg.text}</div>
        {/if}

        <div class="note">
          Solo salida: Lucy avisa, no recibe órdenes por este canal. Todo lo enviado pasa
          antes por el depurador de secretos, y el destino no puede ser una dirección interna.
        </div>
      </div>
    </section>

    <!-- Servidores MCP -->
    <section class="panel span-2">
      <div class="panel-head"><Plug size={16} stroke={1.75} /> Servidores MCP
        <button class="head-btn" onclick={() => onOpenSettings?.()}>Gestionar</button>
      </div>
      {#if mcpServers.length === 0}
        <div class="note" style="margin-top:0">
          Ningún servidor configurado. MCP es la vía por la que Lucy habla con herramientas
          externas — sistemas de tickets, repositorios, bases de datos — sin escribir código
          para cada una. Se añaden desde «Gestionar».
        </div>
      {:else}
        <div class="rows">
          {#each mcpServers as s (s.id)}
            <div class="row col mcp-row">
              <div class="mcp-line">
                <!-- El estado va en un punto: una lista de filas de color compite
                     con los propios paneles y deja de leerse como estado. -->
                <span class="mcp-dot {s.last_status}" title={s.last_error || s.last_status}></span>
                <span class="mcp-name" class:off={!s.enabled}>{s.name}</span>
                {#if !s.enabled}<span class="mcp-tag">desactivado</span>{/if}
                {#if mcpToolCount(s) > 0}
                  <button class="mcp-tools" onclick={() => (mcpOpen = mcpOpen === s.name ? null : s.name)}>
                    {mcpToolCount(s)} herramienta{mcpToolCount(s) === 1 ? '' : 's'}
                  </button>
                {/if}
                {#if s.last_latency_ms != null}<span class="mcp-lat">{s.last_latency_ms} ms</span>{/if}
                <span class="mcp-acts">
                  <button class="b-btn" disabled={mcpBusy === s.name} onclick={() => mcpRun('mcp_server_test', s.name)}>Probar</button>
                  <button class="b-btn" disabled={mcpBusy === s.name} onclick={() => mcpRun('mcp_server_discover', s.name)}>Descubrir</button>
                </span>
              </div>
              <div class="mcp-cmd" title={s.command}>{s.command}</div>
              {#if s.last_status === 'error' && s.last_error}
                <div class="b-msg bad">{s.last_error}</div>
              {/if}
              {#if mcpMsg && mcpMsg.name === s.name}
                <div class="b-msg" class:bad={!mcpMsg.ok}>{mcpMsg.text}</div>
              {/if}
              {#if mcpOpen === s.name && mcpToolCount(s) > 0}
                <div class="mcp-tool-list">
                  {#each s.tools_cache as t}
                    <span class="mcp-tool" title={t.description || ''}>{t.name}</span>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        </div>
        <div class="note">
          «Descubrir» vuelve a preguntar al servidor qué herramientas expone y refresca
          la caché que usa el agente. Los secretos se resuelven desde el almacén de
          credenciales — nunca se escriben en la configuración del servidor.
        </div>
      {/if}
    </section>

    <!-- Tareas programadas -->
    <section class="panel">
      <div class="panel-head"><Clock size={16} stroke={1.75} /> Tareas programadas
        <button class="head-btn" onclick={() => onOpenSettings?.()}>Gestionar</button>
      </div>
      {#if tasks.length === 0}
        <div class="note" style="margin-top:0">Ninguna tarea programada. Corren sin supervisión y con herramientas de solo lectura.</div>
      {:else}
        <div class="rows">
          {#each tasks as t (t.id)}
            <div class="row col mcp-row">
              <div class="mcp-line">
                <span class="mcp-dot {t.last_status === 'ok' ? 'ok' : t.last_status === 'error' ? 'error' : 'pending'}"></span>
                <span class="mcp-name" class:off={!t.enabled}>{t.name}</span>
                {#if t.cron_expr}<span class="mcp-tag" title="Expresión cron">{t.cron_expr}</span>{/if}
                <span class="mcp-lat" title="Próxima ejecución">→ {fmtWhen(t.next_run)}</span>
                <span class="mcp-acts">
                  <button class="b-btn" disabled={tasksBusy === t.id} onclick={() => toggleTask(t)}>{t.enabled ? 'Pausar' : 'Activar'}</button>
                  <button class="b-btn danger" disabled={tasksBusy === t.id} onclick={() => delTask(t)}>Borrar</button>
                </span>
              </div>
              {#if t.last_run}
                <div class="mcp-cmd">Última: {fmtWhen(t.last_run)} · {t.last_status ?? '—'}</div>
              {/if}
              <!-- El resultado va truncado y en línea: el valor está en ver de un
                   vistazo si la última pasada hizo algo, no en leer el informe. -->
              {#if t.last_status === 'error' && t.last_output}
                <div class="b-msg bad">{String(t.last_output).slice(0, 220)}</div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </section>

    <!-- Reglas de permisos -->
    <section class="panel">
      <div class="panel-head"><ShieldLock size={16} stroke={1.75} /> Reglas de permisos
        <button class="head-btn" onclick={() => onOpenSettings?.()}>Gestionar</button>
      </div>
      {#if rules.length === 0}
        <div class="note" style="margin-top:0">
          Sin reglas propias. Todo comando destructivo pasa por confirmación humana — que es
          el comportamiento por defecto y el más seguro.
        </div>
      {:else}
        <div class="rows">
          {#each rules as r (r.id)}
            <div class="row col mcp-row">
              <div class="mcp-line">
                <!-- El color va por ACCIÓN, no por estado: 'allow' es la que
                     salta la confirmación, y es la que hay que poder localizar. -->
                <span class="rule-act {r.action}">{r.action === 'allow' ? 'permitir' : r.action === 'block' ? 'bloquear' : 'preguntar'}</span>
                <span class="mcp-name" class:off={!r.enabled}>{r.pattern}</span>
                <span class="mcp-tag">{r.applies_to}</span>
                <span class="mcp-acts">
                  <button class="b-btn danger" disabled={rulesBusy === r.id} onclick={() => delRule(r)}>Borrar</button>
                </span>
              </div>
              {#if r.description}<div class="mcp-cmd">{r.description}</div>{/if}
            </div>
          {/each}
        </div>
      {/if}
    </section>

    <!-- Guardarraíles -->
    <section class="panel span-2">
      <div class="panel-head"><ShieldLock size={16} stroke={1.75} /> Guardarraíles de seguridad</div>
      <div class="guards">
        {#each GUARDS as g (g.name)}
          <div class="guard locked">
            <span class="g-main">
              <span class="g-name">{g.name}<span class="lockchip">obligatorio</span></span>
              <span class="g-desc">{g.desc}</span>
            </span>
            <span class="switch on"><span class="knob"></span></span>
          </div>
        {/each}
      </div>
    </section>

    <!-- Datos -->
    <section class="panel">
      <div class="panel-head"><Database size={16} stroke={1.75} /> Datos</div>
      <div class="rows">
        <div class="row"><span class="row-l">Ruta de datos</span><span class="row-v mono path" title={dbDir || undefined}>{dbDir || 'no disponible'}</span></div>
        <div class="row"><span class="row-l">Base de datos</span><span class="row-v mono path" title={dbFile || undefined}>{dbFile || 'no disponible'}</span></div>
        <div class="row"><span class="row-l">Proveedores activos</span><span class="row-v">{configured.length}</span></div>
      </div>
    </section>

    <!-- Apariencia -->
    <section class="panel">
      <div class="panel-head"><Palette size={16} stroke={1.75} /> Apariencia</div>
      <div class="rows">
        <div class="row">
          <span class="row-l">Tema</span>
          <span class="row-v seg">
            {#each THEME_OPTS as o}
              <button class="seg-btn" class:on={theme === o.k} onclick={() => onSetTheme?.(o.k)}>{o.l}</button>
            {/each}
          </span>
        </div>
        <div class="row">
          <span class="row-l">Acento del cockpit</span>
          <span class="row-v acc-swatches">
            {#each ACCENT_KEYS as k}
              <button class="acc-sw {k}" class:on={accent === k} onclick={() => onSetAccent?.(k)} aria-label={k}></button>
            {/each}
          </span>
        </div>
        <div class="row"><span class="row-l">Idioma</span><span class="row-v">Español <span class="hint-mini">· cámbialo en configuración completa</span></span></div>
      </div>
    </section>
  </div>
</div>

<style>
  .cfg { height: 100%; overflow-y: auto; padding: 18px 22px; }

  .cfg-head { display: flex; align-items: center; gap: 12px; margin-bottom: 18px; }
  .cfg-title { font-size: var(--fs-title); font-weight: var(--fw-medium); color: var(--text-primary); }
  .src-pill { display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--text-muted); background: var(--surface-2); border: 1px solid var(--border); padding: 4px 10px; border-radius: var(--r-sm); }
  .full-btn { margin-left: auto; font-size: var(--fs-footnote); color: var(--accent-ink); background: var(--accent); border: 0; border-radius: var(--r-md); padding: 6px 13px; cursor: pointer; transition: background var(--dur-fast) var(--ease-out); }
  .full-btn:hover { background: var(--accent-hover); }
  .ol-dot { width: 7px; height: 7px; border-radius: var(--r-pill); background: var(--text-disabled); flex-shrink: 0; }
  .ol-dot.on { background: var(--accent); }
  .mini-btn { background: transparent; border: 0; color: var(--accent); cursor: pointer; font-size: var(--fs-footnote); padding: 0 2px; }

  .grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 14px; }
  .panel { background: var(--surface-1); border: 1px solid var(--border); border-radius: var(--r-lg); padding: 15px 17px; }
  .span-2 { grid-column: 1 / -1; }
  /* v1.7.235 "instrumento premium" — cabecera de panel como label de
     instrumento (la regla cubre las 5 secciones de una vez; el ck-lbl del
     primer head es el mismo tratamiento, así que queda idéntico). */
  .panel-head { display: flex; align-items: center; gap: 8px; font-family: var(--font-mono); font-size: var(--fs-micro); font-weight: var(--fw-medium); letter-spacing: var(--ls-label); text-transform: uppercase; color: var(--text-faint); margin-bottom: 13px; }
  .panel-head :global(svg) { color: var(--accent); }
  .head-btn { margin-left: auto; font-size: var(--fs-caption); color: var(--accent); background: var(--accent-bg); border: 0; border-radius: var(--r-sm); padding: 3px 10px; cursor: pointer; }
  .head-btn:hover { background: rgba(61, 214, 164, 0.18); }

  .rows { display: flex; flex-direction: column; }
  .row { display: flex; align-items: center; gap: 12px; padding: 9px 2px; border-top: 1px solid var(--border); }
  .row.col { flex-direction: column; align-items: stretch; gap: 8px; }
  .row:first-child { border-top: 0; }
  .row-l { flex: 1; font-size: var(--fs-footnote); color: var(--text-muted); }
  .row-v { display: flex; align-items: center; gap: 7px; font-size: var(--fs-footnote); color: var(--text-primary); }
  .row-v.mono { font-family: var(--font-mono); font-size: var(--fs-caption); color: var(--text-secondary); }
  /* Una ruta absoluta real es larga; que se recorte con puntos suspensivos en
     vez de empujar la etiqueta fuera de la fila (el título lleva el valor completo). */
  .row-v.path { min-width: 0; max-width: 62%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge { font-size: var(--fs-caption); color: var(--text-secondary); background: var(--surface-3); padding: 2px 9px; border-radius: var(--r-pill); }
  .badge.accent { color: var(--accent); background: var(--accent-bg); }
  .swatch { width: 12px; height: 12px; border-radius: 4px; background: var(--accent); }
  .acc-swatches { display: flex; gap: 6px; }
  .acc-sw { width: 20px; height: 20px; border-radius: var(--r-pill); border: 2px solid transparent; cursor: pointer; padding: 0; transition: transform var(--dur-fast) var(--ease-out); }
  .acc-sw:hover { transform: scale(1.12); }
  .acc-sw.on { border-color: var(--text-primary); }
  .acc-sw.emerald { background: #3DD6A4; }
  .acc-sw.blue { background: #5B9DF9; }
  .acc-sw.violet { background: #A78BFA; }
  .acc-sw.amber { background: #E5B567; }
  .acc-sw.pink { background: #F06EA9; }
  .acc-sw.cyan { background: #4FD1E0; }
  .hint-mini { font-size: var(--fs-caption); color: var(--text-faint); }
  .row-hint { display: block; font-size: var(--fs-caption); color: var(--text-faint); margin-top: 1px; }

  /* ── Avisos externos ──────────────────────────────────────────────────── */
  .b-input {
    width: 100%; box-sizing: border-box;
    background: var(--surface-2); color: var(--text-primary);
    border: 1px solid var(--border-strong); border-radius: var(--r-sm);
    font-size: var(--fs-footnote); font-family: var(--font-mono);
    padding: 6px 9px; outline: 0;
  }
  .b-input:focus { border-color: var(--border-accent); }
  .b-actions { display: flex; gap: 7px; padding: 11px 2px 2px; flex-wrap: wrap; }
  .b-btn {
    font-size: var(--fs-caption); color: var(--text-secondary);
    background: var(--surface-2); border: 1px solid var(--border-strong);
    border-radius: var(--r-sm); padding: 5px 12px; cursor: pointer;
  }
  .b-btn:hover:not(:disabled) { background: var(--surface-3); color: var(--text-primary); }
  .b-btn:disabled { opacity: 0.45; cursor: default; }
  .b-btn.primary { color: var(--accent-ink); background: var(--accent); border-color: transparent; }
  .b-btn.primary:hover:not(:disabled) { background: var(--accent-hover); }
  .b-btn.danger { color: var(--danger); border-color: var(--danger); background: transparent; }
  .b-msg {
    margin-top: 9px; padding: 7px 10px; border-radius: var(--r-sm);
    font-size: var(--fs-caption); line-height: var(--lh-tight);
    color: var(--accent); background: var(--accent-bg); border: 1px solid var(--accent-line);
    word-break: break-word;
  }
  .b-msg.bad { color: var(--danger); background: rgba(240,110,110,0.10); border-color: var(--danger); }

  /* ── Servidores MCP ───────────────────────────────────────────────────── */
  .mcp-row { gap: 5px; }
  .mcp-line { display: flex; align-items: center; gap: 9px; flex-wrap: wrap; }
  .mcp-dot { width: 7px; height: 7px; border-radius: var(--r-pill); flex-shrink: 0; background: var(--text-disabled); }
  .mcp-dot.ok { background: var(--accent); }
  .mcp-dot.error { background: var(--danger); }
  .mcp-dot.pending { background: #E5B567; }
  .mcp-name { font-size: var(--fs-footnote); color: var(--text-primary); }
  .mcp-name.off { color: var(--text-faint); text-decoration: line-through; }
  .mcp-tag { font-size: var(--fs-caption); color: var(--text-faint); background: var(--surface-3); padding: 1px 7px; border-radius: var(--r-pill); }
  .mcp-tools {
    font-size: var(--fs-caption); color: var(--accent); background: var(--accent-bg);
    border: 1px solid var(--accent-line); border-radius: var(--r-pill);
    padding: 1px 9px; cursor: pointer;
  }
  .mcp-lat { font-family: var(--font-mono); font-size: var(--fs-caption); color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .mcp-acts { margin-left: auto; display: flex; gap: 6px; }
  .mcp-cmd {
    font-family: var(--font-mono); font-size: var(--fs-caption); color: var(--text-muted);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%;
  }
  /* Acción de la regla — el color por ACCIÓN, no por estado: 'permitir' es la
     que salta la confirmación humana y debe localizarse de un vistazo. */
  .rule-act {
    font-family: var(--font-mono); font-size: var(--fs-caption);
    padding: 1px 8px; border-radius: var(--r-pill); flex-shrink: 0;
    color: var(--text-faint); background: var(--surface-3);
  }
  .rule-act.allow { color: var(--danger); background: rgba(240,110,110,0.12); }
  .rule-act.block { color: var(--accent); background: var(--accent-bg); }
  .rule-act.ask   { color: #E5B567; background: rgba(229,181,103,0.12); }

  .mcp-tool-list { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 4px; }
  .mcp-tool {
    font-family: var(--font-mono); font-size: var(--fs-caption); color: var(--text-secondary);
    background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-sm);
    padding: 2px 7px;
  }
  .row-warn {
    display: flex; align-items: flex-start; gap: 7px;
    margin: 2px 0 8px; padding: 8px 11px;
    background: var(--warning-bg, rgba(229,181,103,0.10));
    border: 1px solid var(--warning, #E5B567); border-radius: var(--r-sm);
    font-size: var(--fs-caption); color: var(--text-secondary); line-height: var(--lh-tight);
  }
  .note { font-size: var(--fs-caption); color: var(--text-faint); margin-top: 10px; line-height: var(--lh-tight); }

  .cap-usd { color: var(--text-muted); font-family: var(--font-mono); }
  .cap-input { width: 72px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-sm); color: var(--text-primary); font-size: var(--fs-footnote); font-family: var(--font-mono); padding: 4px 8px; text-align: right; outline: 0; }
  .cap-input:focus { border-color: var(--border-accent); }

  .seg { display: flex; gap: 4px; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-md); padding: 3px; }
  .seg-btn { flex: 1; font-size: var(--fs-caption); color: var(--text-muted); background: transparent; border: 0; border-radius: var(--r-sm); padding: 6px 8px; cursor: pointer; transition: color var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out); }
  .seg-btn:hover { color: var(--text-primary); }
  .seg-btn.on { color: var(--accent-ink); background: var(--accent); }

  /* v1.7.235 — selector de voz TTS */
  .voice-row { display: flex; align-items: center; gap: 8px; }
  .voice-sel {
    flex: 1; min-width: 0;
    background: var(--surface-2); color: var(--text-secondary);
    border: 1px solid var(--border); border-radius: var(--r-sm);
    font-size: var(--fs-caption); font-family: var(--font-sans);
    padding: 6px 8px; cursor: pointer;
  }
  .voice-sel:disabled { opacity: 0.5; cursor: default; }
  .voice-hint { font-size: var(--fs-caption); color: var(--text-faint); margin-top: 4px; }

  .kstat { display: flex; align-items: center; gap: 5px; font-size: var(--fs-caption); padding: 2px 9px; border-radius: var(--r-pill); }
  .kstat.ok { color: var(--success); background: var(--success-bg); }
  .kstat.no { color: var(--text-faint); background: var(--surface-3); }

  .guards { display: grid; grid-template-columns: repeat(2, 1fr); gap: 10px; }
  .guard { display: flex; align-items: center; gap: 12px; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-md); padding: 12px 14px; }
  .g-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .g-name { display: flex; align-items: center; gap: 8px; font-size: var(--fs-footnote); color: var(--text-primary); }
  .lockchip { font-size: var(--fs-caption); color: var(--accent); background: var(--accent-bg); padding: 0 7px; border-radius: var(--r-pill); }
  .g-desc { font-size: var(--fs-caption); color: var(--text-muted); }
  .switch { width: 36px; height: 20px; border-radius: var(--r-pill); background: var(--surface-3); border: 1px solid var(--border-strong); flex-shrink: 0; position: relative; }
  .switch.on { background: var(--accent); border-color: transparent; }
  .knob { position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: var(--r-pill); background: var(--text-primary); }
  .switch.on .knob { transform: translateX(16px); background: var(--accent-ink); }

  @media (max-width: 720px) {
    .grid { grid-template-columns: 1fr; }
    .guards { grid-template-columns: 1fr; }
  }
</style>
