<!-- ── SetupOverlay.svelte ────────────────────────────────────────────────────
     Overlay de primer arranque: recoge nombre, idioma y API key de Gemini,
     valida la clave contra la API y emite el evento `configured` al padre.
     Toda la lógica de Keyring ocurre aquí — el padre solo reacciona al evento.
─────────────────────────────────────────────────────────────────────────── -->
<script>
    import { createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { focusTrap } from '$lib/actions';
    import { safeSetLSString } from '$lib/safe-ls';

    // ── Props ────────────────────────────────────────────────────────────────
    /** Lista completa de idiomas soportados: { code, label, stt, tts } */
    export let LANGS = [];
    /** Idioma pre-seleccionado (el código de idioma del sistema o último usado) */
    export let initialLang = 'es-MX';

    // ── Eventos ──────────────────────────────────────────────────────────────
    const dispatch = createEventDispatcher();

    // ── Estado local ─────────────────────────────────────────────────────────
    let setupName    = '';
    let setupKey     = '';
    let setupProv    = 'gemini';
    let setupLang    = initialLang;
    let setupLoading = false;
    let setupError   = '';
    let setupStep    = 'form'; // 'form' | 'success'
    let showWhatsNew = false;  // toggle the "What's new in 1.4.0" panel

    // Bumped each release. Keep in sync with package.json + Cargo.toml.
    const LUCY_VERSION = '1.4.0';

    // ── Helpers de i18n ──────────────────────────────────────────────────────
    $: t = (es, pt, en, fr = en, de = en) =>
        setupLang.startsWith('es') ? es :
        setupLang.startsWith('pt') ? pt :
        setupLang.startsWith('fr') ? fr :
        setupLang.startsWith('de') ? de : en;

    // ── Guardar configuración ─────────────────────────────────────────────────
    async function guardarConfig() {
        const name = setupName.trim();
        const key  = setupKey.trim();

        if (!name || !key) {
            setupError = t(
                'Completa los campos requeridos.',
                'Preencha os campos obrigatórios.',
                'Please fill in all required fields.'
            );
            return;
        }

        setupError   = '';
        setupLoading = true;
        try {
            // 1. Verificar que la clave sea válida contra la API antes de guardar
            await invoke('test_api_key', { provider: setupProv, apiKey: key });
            // 2. Persistir en Windows Credential Manager
            await invoke('save_llm_key', { provider: setupProv, apiKey: key });

            safeSetLSString('lucy_user_name', name);
            safeSetLSString('lucy_user_lang', setupLang);

            // 3. Mostrar animación de éxito
            setupStep = 'success';
            await new Promise(r => setTimeout(r, 1300));

            // 4. Notificar al padre
            dispatch('configured', { name, lang: setupLang });
            setupStep = 'form';
        } catch(e) {
            setupError =
                String(e).replace(/^Error:\s*/i, '').trim() ||
                t(
                    'Error desconocido. Verifica tu conexión.',
                    'Erro desconhecido. Verifique sua conexão.',
                    'Unknown error. Check your connection.'
                );
        } finally {
            setupLoading = false;
        }
    }

    function abrirApiStudio() {
        invoke('execute_powershell', {
            script: 'Start-Process "https://aistudio.google.com/apikey"',
            forceExecute: false
        }).catch(() => {});
    }
</script>

<!-- ── Overlay ──────────────────────────────────────────────────────────── -->
<div class="so-bg">
  <div class="so-box" use:focusTrap>

    {#if setupStep === 'success'}
      <!-- ── Paso de éxito ──────────────────────────────────────────────── -->
      <div class="so-success">
        <div class="so-success-ico">✓</div>
        <p class="so-success-title">
          {t('¡Clave verificada!', 'Chave verificada!', 'Key verified!', 'Clé vérifiée !', 'Schlüssel verifiziert!')}
        </p>
        <p class="so-success-sub">
          {t(
            `Bienvenido, ${setupName.trim()}. Iniciando Lucy…`,
            `Bem-vindo, ${setupName.trim()}. Iniciando Lucy…`,
            `Welcome, ${setupName.trim()}. Starting Lucy…`
          )}
        </p>
      </div>

    {:else}
      <!-- ── Formulario ─────────────────────────────────────────────────── -->
      <div class="so-header">
        <div class="so-icon">⚡</div>
        <span class="so-version-badge" title="Lucy Assistant v{LUCY_VERSION}">v{LUCY_VERSION}</span>
      </div>
      <h2 class="so-title">Lucy Assistant</h2>
      <p class="so-subtitle">
        {t(
          'Tu asistente SysAdmin con IA · Configuración inicial',
          'Seu assistente SysAdmin com IA · Configuração inicial',
          'Your AI-powered SysAdmin assistant · Initial setup'
        )}
      </p>

      <!-- ── What's new in 1.4.0 (collapsible) ─────────────────────────── -->
      <button class="so-whatsnew-toggle" type="button" on:click={() => showWhatsNew = !showWhatsNew}>
        <span class="so-spark">✦</span>
        <span>{t(
          `Novedades en v${LUCY_VERSION}`,
          `Novidades em v${LUCY_VERSION}`,
          `What's new in v${LUCY_VERSION}`,
          `Nouveautés en v${LUCY_VERSION}`,
          `Neu in v${LUCY_VERSION}`
        )}</span>
        <span class="so-chevron" class:open={showWhatsNew}>▸</span>
      </button>
      {#if showWhatsNew}
        <ul class="so-whatsnew-list">
          <li>
            <span class="so-bullet ok">●</span>
            {t(
              'Indicador de estado ambient (idle / pensando / ejecutando) en el footer',
              'Indicador de estado ambient (idle / pensando / executando) no rodapé',
              'Ambient status indicator (idle / thinking / executing) in the footer'
            )}
          </li>
          <li>
            <span class="so-bullet info">●</span>
            {t(
              'Detección de anomalías estadísticas en CPU / RAM (z-score)',
              'Detecção de anomalias estatísticas em CPU / RAM (z-score)',
              'Statistical anomaly detection on CPU / RAM (z-score)'
            )}
          </li>
          <li>
            <span class="so-bullet warn">●</span>
            {t(
              'Predictor de costo en vivo · estima tokens y USD antes de enviar',
              'Preditor de custo em tempo real · estima tokens e USD antes de enviar',
              'Live cost predictor · estimates tokens & USD before sending'
            )}
          </li>
          <li>
            <span class="so-bullet ok">●</span>
            {t(
              'Exportar conversación como Notebook reutilizable (.lucynote / .md)',
              'Exportar conversa como Notebook reutilizável (.lucynote / .md)',
              'Export conversation as a reusable Notebook (.lucynote / .md)'
            )}
          </li>
          <li>
            <span class="so-bullet info">●</span>
            {t(
              'Búsqueda fuzzy en Reglas de Permisos · filtrar por acción',
              'Busca fuzzy em Regras de Permissões · filtrar por ação',
              'Fuzzy search in Permission Rules · filter by action'
            )}
          </li>
          <li>
            <span class="so-bullet danger">●</span>
            {t(
              'Hardening: ofuscación de blocklist, integrity check, release LTO',
              'Hardening: ofuscação de blocklist, integrity check, release LTO',
              'Hardening: blocklist obfuscation, integrity check, release LTO'
            )}
          </li>
          <li>
            <span class="so-bullet ok">●</span>
            {t(
              'Lucy ya no se detiene a media tarea cuando le pides múltiples cosas',
              'Lucy não para mais no meio da tarefa quando você pede várias coisas',
              'Lucy no longer stops mid-task when given multi-step prompts'
            )}
          </li>
        </ul>
      {/if}

      <!-- Idioma -->
      <div class="so-field">
        <label class="so-label" for="su-lang">🌐 Language / Idioma *</label>
        <select id="su-lang" class="so-input" bind:value={setupLang} disabled={setupLoading}>
          {#each LANGS as lang}
            <option value={lang.code}>{lang.label}</option>
          {/each}
        </select>
      </div>

      <!-- Nombre -->
      <div class="so-field">
        <label class="so-label" for="su-name">
          {t(
            'Nombre del Administrador *',
            'Nome do Administrador *',
            'Administrator Name *',
            "Nom de l'Administrateur *",
            'Name des Administrators *'
          )}
        </label>
        <input id="su-name" class="so-input" type="text"
          placeholder={setupLang.startsWith('es') ? 'Ej. Iván' : setupLang.startsWith('pt') ? 'Ex. João' : 'e.g. John'}
          bind:value={setupName} disabled={setupLoading}
          on:keydown={(e) => { if (e.key === 'Enter') guardarConfig(); }}>
      </div>

      <!-- IA Provider -->
      <div class="so-field">
        <label class="so-label" for="su-prov">{t('Proveedor de Inteligencia Artificial *', 'Provedor de Inteligência Artificial *', 'AI Provider *')}</label>
        <select id="su-prov" class="so-input" bind:value={setupProv} disabled={setupLoading}>
          <option value="gemini">Google Gemini</option>
          <option value="anthropic">Anthropic Claude</option>
          <option value="openai">OpenAI GPT</option>
          <option value="local">Local LLM (OpenAI API Compatible)</option>
        </select>
      </div>

      <!-- API Key -->
      <div class="so-field" style="margin-bottom:{setupError ? '10px' : '22px'};">
        <label class="so-label" for="su-key">
          {#if setupProv === 'local'}
            {t(`URL del Endpoint (${setupProv}) *`, `URL do Endpoint (${setupProv}) *`, `Endpoint URL (${setupProv}) *`)}
          {:else}
            {t(`Llave de API (${setupProv}) *`, `Chave de API (${setupProv}) *`, `API Key (${setupProv}) *`)}
          {/if}
        </label>
        <input id="su-key" class="so-input" type={setupProv === 'local' ? 'text' : 'password'} 
          placeholder={setupProv === 'local' ? 'http://localhost:11434/v1/chat/completions' : 'sk-...'}
          bind:value={setupKey} disabled={setupLoading}
          on:keydown={(e) => { if (e.key === 'Enter') guardarConfig(); }}>
        {#if setupProv === 'gemini'}
        <div class="so-hint">
          {t('Obtén tu clave en', 'Obtenha sua chave em', 'Get your key at')} 
          <button class="so-link-btn" on:click={abrirApiStudio} tabindex="-1">aistudio.google.com/apikey</button>
        </div>
        {/if}
      </div>

      <!-- Error inline -->
      {#if setupError}
        <div class="so-error" role="alert">⚠ {setupError}</div>
      {/if}

      <!-- Botón -->
      <button class="so-btn" style="margin-top:{setupError ? '12px' : '0'};"
        on:click={guardarConfig} disabled={setupLoading}>
        {#if setupLoading}
          <span class="so-spinner"></span>
          {t('Verificando clave…', 'Verificando chave…', 'Verifying key…', 'Vérification…', 'Schlüssel prüfen…')}
        {:else}
          {t(
            '⚡ Conectar Módulo IA',
            '⚡ Conectar Módulo IA',
            '⚡ Connect AI Module',
            '⚡ Connecter le Module IA',
            '⚡ KI-Modul verbinden'
          )}
        {/if}
      </button>
    {/if}

  </div>
</div>

<style>
  /* ── Overlay ──────────────────────────────────────────────────────────────── */
  .so-bg {
    position: fixed; inset: 0; z-index: 9999;
    background: rgba(4, 8, 14, 0.92);
    backdrop-filter: blur(6px);
    display: flex; align-items: center; justify-content: center;
  }
  .so-box {
    background: var(--bg2, #0b0e14);
    border: 1px solid var(--bdr, #1a2030);
    border-radius: 14px;
    padding: 30px 28px;
    width: 360px;
    max-width: 92vw;
    text-align: center;
    box-shadow: 0 24px 64px rgba(0,0,0,0.7);
  }

  /* ── Cabecera ─────────────────────────────────────────────────────────────── */
  .so-header {
    display: flex; align-items: center; justify-content: center;
    gap: 10px; margin-bottom: 12px; position: relative;
  }
  .so-icon  { font-size: 34px; color: var(--acc, #10b981); line-height: 1; }
  .so-version-badge {
    position: absolute; top: 0; right: 0;
    font-size: 9px; font-weight: 700;
    color: var(--acc, #10b981);
    background: rgba(16,185,129,0.10);
    border: 1px solid rgba(16,185,129,0.25);
    padding: 2px 7px; border-radius: 10px;
    font-family: var(--mono, monospace);
    letter-spacing: 0.3px;
    box-shadow: 0 0 12px rgba(16,185,129,0.18);
  }
  .so-title { color: white; margin: 0 0 6px; font-size: 17px; font-weight: 600; }
  .so-subtitle { color: var(--txt2, #7a8a9a); font-size: 12px; margin-bottom: 16px; line-height: 1.5; }

  /* ── What's new (collapsible teaser) ──────────────────────────────────── */
  .so-whatsnew-toggle {
    display: flex; align-items: center; justify-content: center; gap: 8px;
    width: 100%;
    background: rgba(167,139,250,0.06);
    border: 1px solid rgba(167,139,250,0.20);
    color: #c4b5fd;
    border-radius: 8px;
    padding: 8px 12px;
    margin-bottom: 14px;
    font-size: 11.5px; font-weight: 600; font-family: inherit;
    cursor: pointer;
    transition: background 160ms ease, border-color 160ms ease, transform 80ms ease;
  }
  .so-whatsnew-toggle:hover {
    background: rgba(167,139,250,0.10);
    border-color: rgba(167,139,250,0.35);
  }
  .so-whatsnew-toggle:active { transform: scale(0.985); }
  .so-spark { color: #a78bfa; font-size: 11px; }
  .so-chevron {
    margin-left: auto;
    font-size: 10px;
    transition: transform 200ms cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .so-chevron.open { transform: rotate(90deg); }
  .so-whatsnew-list {
    list-style: none; padding: 0; margin: 0 0 18px;
    text-align: left;
    background: rgba(0, 0, 0, 0.25);
    border-radius: 8px;
    border: 1px solid rgba(167,139,250,0.12);
    padding: 10px 12px;
    animation: so-list-in 320ms cubic-bezier(0.16, 1, 0.3, 1);
  }
  .so-whatsnew-list li {
    display: flex; align-items: flex-start; gap: 8px;
    font-size: 11.5px; color: var(--txt, #cbd5e1); line-height: 1.5;
    padding: 4px 0;
  }
  .so-bullet { font-size: 8px; line-height: 1.6; flex-shrink: 0; }
  .so-bullet.ok     { color: #10b981; }
  .so-bullet.info   { color: #3b9eff; }
  .so-bullet.warn   { color: #f59e0b; }
  .so-bullet.danger { color: #ef4444; }
  @keyframes so-list-in {
    from { opacity: 0; transform: translateY(-6px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  /* ── Campos ───────────────────────────────────────────────────────────────── */
  .so-field { text-align: left; margin-bottom: 12px; }
  .so-label {
    color: var(--txt2, #7a8a9a); font-size: 12px; font-weight: 600;
    display: block; margin-bottom: 5px;
  }
  .so-input {
    width: 100%; padding: 8px 11px;
    background: rgba(0,0,0,.35);
    border: 1px solid var(--bdr2, #222c3a);
    border-radius: 7px;
    color: var(--txt, #dde3ea);
    font-size: 13px; font-family: inherit;
    transition: border-color .15s;
    box-sizing: border-box;
    outline: none;
  }
  .so-input:focus  { border-color: var(--acc, #10b981); }
  .so-input:disabled { opacity: .5; cursor: not-allowed; }
  .so-hint { font-size: 11px; color: #3a5a7a; margin-top: 5px; }

  /* ── Botón principal ──────────────────────────────────────────────────────── */
  .so-btn {
    width: 100%; padding: 11px;
    background: var(--acc, #10b981); color: #030b06;
    border: none; border-radius: 8px;
    font-size: 13px; font-weight: 700; font-family: inherit;
    cursor: pointer; transition: opacity .15s;
    display: flex; align-items: center; justify-content: center; gap: 6px;
  }
  .so-btn:hover:not(:disabled) { opacity: .88; }
  .so-btn:disabled { opacity: .5; cursor: not-allowed; }

  /* ── Link a API Studio ────────────────────────────────────────────────────── */
  .so-link-btn {
    background: none; border: none; color: #3a6a8a;
    cursor: pointer; font-size: 11px; font-style: italic; font-family: inherit;
    padding: 0; transition: .15s;
  }
  .so-link-btn:hover { color: var(--blue, #3b82f6); text-decoration: underline; }

  /* ── Error inline ─────────────────────────────────────────────────────────── */
  .so-error {
    background: rgba(255,68,68,.08); border: 1px solid rgba(255,68,68,.25);
    color: #ff6b6b; border-radius: 7px; padding: 9px 12px;
    font-size: 12px; text-align: left; line-height: 1.5;
  }

  /* ── Spinner ──────────────────────────────────────────────────────────────── */
  .so-spinner {
    display: inline-block; width: 13px; height: 13px;
    border: 2px solid rgba(0,0,0,.3); border-top-color: #000;
    border-radius: 50%; animation: so-spin .6s linear infinite;
    vertical-align: middle;
  }
  @keyframes so-spin { to { transform: rotate(360deg); } }

  /* ── Pantalla de éxito ────────────────────────────────────────────────────── */
  .so-success { padding: 18px 0 10px; display: flex; flex-direction: column; align-items: center; gap: 10px; }
  .so-success-ico {
    width: 54px; height: 54px; border-radius: 50%;
    background: rgba(16,185,129,.1); border: 2px solid var(--acc, #10b981);
    display: flex; align-items: center; justify-content: center;
    font-size: 24px; color: var(--acc, #10b981);
    animation: so-pop .35s ease;
  }
  .so-success-title { color: var(--acc, #10b981); font-size: 16px; font-weight: 700; margin: 0; }
  .so-success-sub   { color: var(--txt2, #7a8a9a); font-size: 12px; margin: 0; }
  @keyframes so-pop { from { transform: scale(.6); opacity: 0; } to { transform: scale(1); opacity: 1; } }
</style>
