<script>
    import { createEventDispatcher, onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import Key from '@tabler/icons-svelte/icons/key';

    import Globe from '@tabler/icons-svelte/icons/world';

    import Save from '@tabler/icons-svelte/icons/device-floppy';

    import AlertCircle from '@tabler/icons-svelte/icons/alert-circle';

    import CheckCircle from '@tabler/icons-svelte/icons/circle-check';

    import IconSparkles from '@tabler/icons-svelte/icons/sparkles';

    import IconBrandGoogle from '@tabler/icons-svelte/icons/brand-google';

    import IconBrandOpenai from '@tabler/icons-svelte/icons/brand-openai';

    import IconServer2 from '@tabler/icons-svelte/icons/server-2';

    import IconBolt from '@tabler/icons-svelte/icons/bolt';
    import { refreshNvidiaModels } from '$lib/models.js';

    const dispatch = createEventDispatcher();

    // Props
    export let isOpen = false;
    export let isEN = false;

    // State
    let loading = false;
    let error = '';
    let success = '';
    let activeTab = 'anthropic'; // 'anthropic', 'gemini', 'openai', 'nvidia', 'ollama', 'tavily'
    let credentials = {
        anthropic: { key: '', configured: false },
        gemini: { key: '', configured: false },
        openai: { key: '', configured: false },
        nvidia: { key: '', configured: false },
        ollama: { endpoint: 'http://localhost:11434', configured: false },
        // Tavily is the preferred backend for the `<TOOL>search_web</TOOL>`
        // tool. Without a key Lucy falls back to scraping DuckDuckGo, which
        // is fragile + rate-limited. Free tier covers 1000 searches/month.
        tavily: { key: '', configured: false },
    };
    let healthStatus = {
        anthropic: null,
        gemini: null,
        openai: null,
        nvidia: null,
        ollama: null,
        tavily: null,
    };

    const labels = {
        'es-MX': {
            title: 'Configuración de Proveedores',
            subtitle: 'Gestiona credenciales de API para múltiples proveedores de IA',
            anthropic: {
                label: 'API Key Anthropic',
                placeholder: 'sk-ant-...',
                hint: 'Obtén tu clave en https://console.anthropic.com/account/keys',
                feature: 'Automatización GUI Nativa (Computer Use)'
            },
            gemini: {
                label: 'API Key Google Gemini',
                placeholder: 'AIza...',
                hint: 'Obtén tu clave en https://aistudio.google.com/app/apikeys',
                feature: 'Visión + Prompts Estructurados'
            },
            openai: {
                label: 'API Key OpenAI',
                placeholder: 'sk-...',
                hint: 'Obtén tu clave en https://platform.openai.com/account/api-keys',
                feature: 'GPT-4 Vision + Análisis JSON'
            },
            nvidia: {
                label: 'NVIDIA NIM API Key',
                placeholder: 'nvapi-...',
                hint: 'Obtén tu clave gratis en https://build.nvidia.com → Get API Key',
                feature: 'Llama 3.1/3.3, Nemotron, Mistral, Gemma 4 y más'
            },
            ollama: {
                label: 'Endpoint Ollama',
                placeholder: 'http://localhost:11434',
                hint: 'Instala Ollama desde https://ollama.ai y descarga llava',
                feature: 'Privacidad Total - Ejecución Local'
            },
            tavily: {
                label: 'API Key Tavily',
                placeholder: 'tvly-...',
                hint: 'Obtén tu clave gratis en https://app.tavily.com (1000 búsquedas/mes incluidas)',
                feature: 'Búsqueda web optimizada para agentes — sin scraping ni rate-limits'
            },
            guardrails: {
                title: 'Capa de Guardrails',
                regex_active: 'Regex bank activo — protege contra prompt injection, SSRF, UAC injection y bypass de cmd (audit S1/S2/S5/S10).',
                ml_active: 'PromptGuard 2 ML ACTIVO — detecta jailbreaks parafraseados.',
                ml_model_missing: 'Feature ML compilado pero modelo no encontrado.',
                ml_runtime_missing: 'Modelo en disco pero ONNX Runtime DLL no detectada.',
                ml_feature_disabled: 'Build sin ML — solo regex está activo. Para activar: cargo build --features ml-guard',
                ml_failed: 'Error al cargar:',
                recheck: 'Re-verificar',
                install_guide: 'Ver guía de instalación: src-tauri/src/guardrails/PROMPT_GUARD_INSTALL.md'
            },
            save: 'Guardar Credenciales',
            test: 'Probar Conexión',
            close: 'Cerrar',
            testing: 'Probando...',
            success: 'Credenciales guardadas',
            required: 'Este campo es requerido',
            checkHealth: 'Verificar Salud',
            health: {
                ok: 'Conectado',
                error: 'Error de conexión',
                unconfigured: 'No configurado'
            }
        },
        'en-US': {
            title: 'Provider Configuration',
            subtitle: 'Manage API credentials for multiple AI providers',
            anthropic: {
                label: 'Anthropic API Key',
                placeholder: 'sk-ant-...',
                hint: 'Get your key at https://console.anthropic.com/account/keys',
                feature: 'Native GUI Automation (Computer Use)'
            },
            gemini: {
                label: 'Google Gemini API Key',
                placeholder: 'AIza...',
                hint: 'Get your key at https://aistudio.google.com/app/apikeys',
                feature: 'Vision + Structured Prompts'
            },
            openai: {
                label: 'OpenAI API Key',
                placeholder: 'sk-...',
                hint: 'Get your key at https://platform.openai.com/account/api-keys',
                feature: 'GPT-4 Vision + JSON Analysis'
            },
            nvidia: {
                label: 'NVIDIA NIM API Key',
                placeholder: 'nvapi-...',
                hint: 'Get your free key at https://build.nvidia.com → Get API Key',
                feature: 'Llama 3.1/3.3, Nemotron, Mistral, Gemma 4 and more'
            },
            tavily: {
                label: 'Tavily API Key',
                placeholder: 'tvly-...',
                hint: 'Get your free key at https://app.tavily.com (free tier: 1000 searches/month)',
                feature: 'Agent-optimized web search — no scraping, no rate limits'
            },
            guardrails: {
                title: 'Guardrail Layer',
                regex_active: 'Regex bank active — protects against prompt injection, SSRF, UAC injection and cmd bypass (audit S1/S2/S5/S10).',
                ml_active: 'PromptGuard 2 ML ACTIVE — detects paraphrased jailbreaks.',
                ml_model_missing: 'ML feature compiled but model not found.',
                ml_runtime_missing: 'Model on disk but ONNX Runtime DLL not detected.',
                ml_feature_disabled: 'Build without ML — regex only. To enable: cargo build --features ml-guard',
                ml_failed: 'Load failed:',
                recheck: 'Re-check',
                install_guide: 'Install guide: src-tauri/src/guardrails/PROMPT_GUARD_INSTALL.md'
            },
            ollama: {
                label: 'Ollama Endpoint',
                placeholder: 'http://localhost:11434',
                hint: 'Install Ollama from https://ollama.ai and download llava',
                feature: 'Total Privacy - Local Execution'
            },
            save: 'Save Credentials',
            test: 'Test Connection',
            close: 'Close',
            testing: 'Testing...',
            success: 'Credentials saved',
            required: 'This field is required',
            checkHealth: 'Check Health',
            health: {
                ok: 'Connected',
                error: 'Connection error',
                unconfigured: 'Not configured'
            }
        }
    };

    const l = labels[isEN ? 'en-US' : 'es-MX'];

    // ── Load configured state on mount (May 2026 UX polish) ──
    // Previously the modal showed every tab as "unconfigured" until the user
    // saved a new key, even if the key was already in the keyring from a
    // previous session. Now we probe `get_configured_providers` once when
    // the modal first becomes visible and stamp the green checkmarks.
    async function loadConfiguredState() {
        try {
            const provs = await invoke('get_configured_providers');
            const set = new Set(Array.isArray(provs) ? provs.map(String) : []);
            for (const p of Object.keys(credentials)) {
                // `ollama` is stored under the `local` keyring slot
                const key = (p === 'ollama') ? 'local' : p;
                credentials[p].configured = set.has(key);
            }
            credentials = credentials;   // trigger reactivity
        } catch { /* keyring failure → leave defaults */ }
    }

    // ── PromptGuard 2 status (Phase 2 LlamaFirewall, May 2026) ──
    // Shown in the "Guardrails" tab so users can verify their ML install
    // without diving into DevTools. Re-probed on every modal open + when
    // the user explicitly hits "Re-check".
    let mlGuard = null;          // { status, model_path, note }
    async function probeMlGuard() {
        try { mlGuard = await invoke('prompt_guard_status'); }
        catch (e) { mlGuard = { status: 'failed', model_path: '', note: String(e) }; }
    }
    onMount(() => { loadConfiguredState(); probeMlGuard(); });
    // Re-probe whenever the modal is opened, so a user who configured a
    // provider elsewhere (e.g. via /command) sees the updated state.
    $: if (isOpen) { loadConfiguredState(); probeMlGuard(); }

    async function saveCredentials() {
        loading = true;
        error = '';
        success = '';

        try {
            const value = credentials[activeTab].key || credentials[activeTab].endpoint || '';
            if (!value.trim()) { error = l.required; return; }

            // save_llm_key handles gemini / anthropic / openai / nvidia / local / tavily
            await invoke('save_llm_key', { provider: activeTab === 'ollama' ? 'local' : activeTab, apiKey: value.trim() });
            credentials[activeTab].configured = true;
            success = l.success;

            // If NVIDIA key saved, refresh model list immediately
            if (activeTab === 'nvidia') {
                refreshNvidiaModels().catch(() => {});
            }

            setTimeout(() => { success = ''; }, 3000);
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    }

    async function testConnection() {
        loading = true;
        error = '';

        try {
            const value = credentials[activeTab].key || credentials[activeTab].endpoint || '';
            if (!value.trim()) { error = l.required; loading = false; return; }

            await invoke('test_api_key', { provider: activeTab === 'ollama' ? 'local' : activeTab, apiKey: value.trim() });
            healthStatus[activeTab] = { status: 'ok' };
            success = l.health.ok;
            setTimeout(() => { success = ''; }, 3000);
        } catch (e) {
            error = String(e);
            healthStatus[activeTab] = { status: 'error', message: String(e) };
        } finally {
            loading = false;
        }
    }

    function getHealthIcon(status) {
        if (status === 'ok') return CheckCircle;
        if (status === 'error') return AlertCircle;
        return null;
    }

    function getHealthColor(status) {
        if (status === 'ok') return '#10b981';
        if (status === 'error') return '#ef4444';
        return '#6b7280';
    }
</script>

{#if isOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="modal-overlay" role="presentation" on:click={() => dispatch('close')}
         on:keydown={(e) => { if (e.key === 'Escape') dispatch('close'); }}>
        <div class="modal-content" role="dialog" aria-modal="true" tabindex="-1" on:click|stopPropagation>
            <div class="modal-header">
                <div class="header-content">
                    <h2>{l.title}</h2>
                    <p class="subtitle">{l.subtitle}</p>
                </div>
                <button class="close-btn" on:click={() => dispatch('close')}>✕</button>
            </div>

            <div class="modal-body">
                <!-- Tabs -->
                <div class="tabs">
                    <button
                        class="tab"
                        class:active={activeTab === 'anthropic'}
                        on:click={() => activeTab = 'anthropic'}
                    >
                        <span style="display:inline-flex;align-items:center;gap:6px;"><IconSparkles size={14} strokeWidth={1.8} color="#d97757" />Anthropic</span>
                        {#if credentials.anthropic.configured}
                            <CheckCircle size={14} color="#10b981" />
                        {/if}
                    </button>
                    <button
                        class="tab"
                        class:active={activeTab === 'gemini'}
                        on:click={() => activeTab = 'gemini'}
                    >
                        <span style="display:inline-flex;align-items:center;gap:6px;"><IconBrandGoogle size={14} strokeWidth={1.8} color="#4285f4" />Gemini</span>
                        {#if credentials.gemini.configured}
                            <CheckCircle size={14} color="#10b981" />
                        {/if}
                    </button>
                    <button
                        class="tab"
                        class:active={activeTab === 'openai'}
                        on:click={() => activeTab = 'openai'}
                    >
                        <span style="display:inline-flex;align-items:center;gap:6px;"><IconBrandOpenai size={14} strokeWidth={1.8} color="#10a37f" />OpenAI</span>
                        {#if credentials.openai.configured}
                            <CheckCircle size={14} color="#10b981" />
                        {/if}
                    </button>
                    <button
                        class="tab"
                        class:active={activeTab === 'nvidia'}
                        on:click={() => activeTab = 'nvidia'}
                    >
                        <span style="display:inline-flex;align-items:center;gap:6px;"><IconBolt size={14} strokeWidth={1.8} color="#76b900" />NVIDIA</span>
                        {#if credentials.nvidia.configured}
                            <CheckCircle size={14} color="#10b981" />
                        {/if}
                    </button>
                    <button
                        class="tab"
                        class:active={activeTab === 'ollama'}
                        on:click={() => activeTab = 'ollama'}
                    >
                        <span style="display:inline-flex;align-items:center;gap:6px;"><IconServer2 size={14} strokeWidth={1.8} color="#a78bfa" />Ollama</span>
                        {#if credentials.ollama.configured}
                            <CheckCircle size={14} color="#10b981" />
                        {/if}
                    </button>
                    <button
                        class="tab"
                        class:active={activeTab === 'tavily'}
                        on:click={() => activeTab = 'tavily'}
                        title="Web search backend for <TOOL>search_web</TOOL>"
                    >
                        <span style="display:inline-flex;align-items:center;gap:6px;"><Globe size={14} strokeWidth={1.8} color="#3b9eff" />Tavily</span>
                        {#if credentials.tavily.configured}
                            <CheckCircle size={14} color="#10b981" />
                        {/if}
                    </button>
                    <button
                        class="tab"
                        class:active={activeTab === 'guardrails'}
                        on:click={() => { activeTab = 'guardrails'; probeMlGuard(); }}
                        title="Security guardrail layer status"
                    >
                        <span style="display:inline-flex;align-items:center;gap:6px;">🛡 Guardrails</span>
                        {#if mlGuard?.status === 'active'}
                            <CheckCircle size={14} color="#10b981" />
                        {/if}
                    </button>
                </div>

                <!-- Tab Content -->
                <div class="tab-content">
                    {#if activeTab === 'anthropic'}
                        <div class="config-section">
                            <div class="feature-badge">{l.anthropic.feature}</div>
                            <label>
                                <span>{l.anthropic.label}</span>
                                <input
                                    type="password"
                                    bind:value={credentials.anthropic.key}
                                    placeholder={l.anthropic.placeholder}
                                />
                            </label>
                            <p class="hint">
                                <Key size={14} /> {l.anthropic.hint}
                            </p>
                        </div>
                    {:else if activeTab === 'gemini'}
                        <div class="config-section">
                            <div class="feature-badge">{l.gemini.feature}</div>
                            <label>
                                <span>{l.gemini.label}</span>
                                <input
                                    type="password"
                                    bind:value={credentials.gemini.key}
                                    placeholder={l.gemini.placeholder}
                                />
                            </label>
                            <p class="hint">
                                <Key size={14} /> {l.gemini.hint}
                            </p>
                        </div>
                    {:else if activeTab === 'openai'}
                        <div class="config-section">
                            <div class="feature-badge">{l.openai.feature}</div>
                            <label>
                                <span>{l.openai.label}</span>
                                <input
                                    type="password"
                                    bind:value={credentials.openai.key}
                                    placeholder={l.openai.placeholder}
                                />
                            </label>
                            <p class="hint">
                                <Key size={14} /> {l.openai.hint}
                            </p>
                        </div>
                    {:else if activeTab === 'nvidia'}
                        <div class="config-section">
                            <div class="feature-badge" style="background:rgba(118,185,0,0.15);color:#76b900;">{l.nvidia.feature}</div>
                            <label>
                                <span>{l.nvidia.label}</span>
                                <input
                                    type="password"
                                    bind:value={credentials.nvidia.key}
                                    placeholder={l.nvidia.placeholder}
                                />
                            </label>
                            <p class="hint">
                                <Key size={14} /> {l.nvidia.hint}
                            </p>
                        </div>
                    {:else if activeTab === 'ollama'}
                        <div class="config-section">
                            <div class="feature-badge">{l.ollama.feature}</div>
                            <label>
                                <span>{l.ollama.label}</span>
                                <input
                                    type="text"
                                    bind:value={credentials.ollama.endpoint}
                                    placeholder={l.ollama.placeholder}
                                />
                            </label>
                            <p class="hint">
                                <Globe size={14} /> {l.ollama.hint}
                            </p>
                        </div>
                    {:else if activeTab === 'tavily'}
                        <div class="config-section">
                            <div class="feature-badge" style="background:rgba(59,158,255,0.15);color:#3b9eff;">{l.tavily.feature}</div>
                            <label>
                                <span>{l.tavily.label}</span>
                                <input
                                    type="password"
                                    bind:value={credentials.tavily.key}
                                    placeholder={l.tavily.placeholder}
                                />
                            </label>
                            <p class="hint">
                                <Key size={14} /> {l.tavily.hint}
                            </p>
                        </div>
                    {:else if activeTab === 'guardrails'}
                        <div class="config-section">
                            <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:12px;">
                                <h3 style="margin:0;font-size:14px;color:var(--txt);">{l.guardrails.title}</h3>
                                <button class="re-check" on:click={probeMlGuard} title="Re-check status">↻ {l.guardrails.recheck}</button>
                            </div>

                            <!-- Regex layer — always-on -->
                            <div class="guard-row guard-ok">
                                <CheckCircle size={16} color="#10b981" />
                                <div>
                                    <strong>Regex bank</strong>
                                    <div class="hint" style="margin-top:2px;">{l.guardrails.regex_active}</div>
                                </div>
                            </div>

                            <!-- ML layer — status varies -->
                            {#if mlGuard?.status === 'active'}
                                <div class="guard-row guard-ok">
                                    <CheckCircle size={16} color="#10b981" />
                                    <div>
                                        <strong>PromptGuard 2 ML</strong>
                                        <div class="hint" style="margin-top:2px;">{l.guardrails.ml_active}</div>
                                        {#if mlGuard.model_path}
                                            <div class="hint" style="font-family:var(--mono);font-size:10px;margin-top:4px;opacity:0.7;">{mlGuard.model_path}</div>
                                        {/if}
                                    </div>
                                </div>
                            {:else if mlGuard?.status === 'model_missing'}
                                <div class="guard-row guard-warn">
                                    <AlertCircle size={16} color="#f59e0b" />
                                    <div>
                                        <strong>PromptGuard 2 ML</strong>
                                        <div class="hint" style="margin-top:2px;">{l.guardrails.ml_model_missing}</div>
                                        <div class="hint" style="font-family:var(--mono);font-size:10px;margin-top:4px;opacity:0.7;">{mlGuard.model_path}</div>
                                        <div class="hint" style="margin-top:6px;">{l.guardrails.install_guide}</div>
                                    </div>
                                </div>
                            {:else if mlGuard?.status === 'runtime_missing'}
                                <div class="guard-row guard-warn">
                                    <AlertCircle size={16} color="#f59e0b" />
                                    <div>
                                        <strong>PromptGuard 2 ML</strong>
                                        <div class="hint" style="margin-top:2px;">{l.guardrails.ml_runtime_missing}</div>
                                    </div>
                                </div>
                            {:else if mlGuard?.status === 'failed'}
                                <div class="guard-row guard-err">
                                    <AlertCircle size={16} color="#ef4444" />
                                    <div>
                                        <strong>PromptGuard 2 ML</strong>
                                        <div class="hint" style="margin-top:2px;">{l.guardrails.ml_failed} {mlGuard.note ?? ''}</div>
                                    </div>
                                </div>
                            {:else}
                                <div class="guard-row guard-muted">
                                    <span style="opacity:0.5;font-size:14px;">○</span>
                                    <div>
                                        <strong>PromptGuard 2 ML</strong>
                                        <div class="hint" style="margin-top:2px;">{l.guardrails.ml_feature_disabled}</div>
                                    </div>
                                </div>
                            {/if}
                        </div>
                    {/if}

                    <!-- Health Status -->
                    {#if healthStatus[activeTab]}
                        <div class="health-status" style="border-left-color: {getHealthColor(healthStatus[activeTab].status)}">
                            <div class="health-icon">
                                {#if healthStatus[activeTab].status === 'ok'}
                                    <CheckCircle size={18} color="#10b981" />
                                {:else}
                                    <AlertCircle size={18} color="#ef4444" />
                                {/if}
                            </div>
                            <div class="health-info">
                                <div class="health-status-text">
                                    {#if healthStatus[activeTab].status === 'ok'}
                                        {l.health.ok}
                                    {:else}
                                        {l.health.error}
                                    {/if}
                                </div>
                                {#if healthStatus[activeTab].message}
                                    <p class="health-message">{healthStatus[activeTab].message}</p>
                                {/if}
                            </div>
                        </div>
                    {/if}

                    <!-- Feedback Messages -->
                    {#if error}
                        <div class="error-message">
                            <AlertCircle size={16} />
                            {error}
                        </div>
                    {/if}
                    {#if success}
                        <div class="success-message">
                            <CheckCircle size={16} />
                            {success}
                        </div>
                    {/if}
                </div>
            </div>

            <div class="modal-footer">
                <button class="btn-secondary" on:click={() => dispatch('close')}>
                    {l.close}
                </button>
                <div class="action-buttons">
                    <button
                        class="btn-test"
                        on:click={testConnection}
                        disabled={loading}
                    >
                        {loading ? l.testing : l.test}
                    </button>
                    <button
                        class="btn-primary"
                        on:click={saveCredentials}
                        disabled={loading}
                    >
                        <Save size={14} /> {l.save}
                    </button>
                </div>
            </div>
        </div>
    </div>
{/if}

<style>
    .modal-overlay {
        position: fixed;
        top: 0;
        left: 0;
        right: 0;
        bottom: 0;
        background: rgba(0, 0, 0, 0.7);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 1000;
    }

    .modal-content {
        background: var(--bg);
        border: 1px solid var(--bd);
        border-radius: 12px;
        width: 90%;
        max-width: 600px;
        max-height: 85vh;
        display: flex;
        flex-direction: column;
        box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3);
    }

    .modal-header {
        padding: 24px;
        border-bottom: 1px solid var(--bd);
        display: flex;
        justify-content: space-between;
        align-items: flex-start;
    }

    .header-content h2 {
        margin: 0 0 8px 0;
        font-size: 20px;
        font-weight: 600;
        color: var(--txt);
    }

    .subtitle {
        margin: 0;
        font-size: 13px;
        color: var(--txt2);
    }

    .close-btn {
        background: none;
        border: none;
        font-size: 24px;
        cursor: pointer;
        color: var(--txt2);
        padding: 0;
        width: 32px;
        height: 32px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 6px;
        transition: all 0.2s;
    }

    .close-btn:hover {
        background: var(--acc-bg);
        color: var(--acc);
    }

    .modal-body {
        flex: 1;
        overflow-y: auto;
        display: flex;
        flex-direction: column;
    }

    .tabs {
        display: flex;
        gap: 0;
        border-bottom: 1px solid var(--bd);
        padding: 0;
        background: var(--bg-alt);
    }

    .tab {
        flex: 1;
        padding: 14px 16px;
        background: none;
        border: none;
        cursor: pointer;
        color: var(--txt2);
        font-size: 13px;
        font-weight: 500;
        transition: all 0.2s;
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
        border-bottom: 2px solid transparent;
        white-space: nowrap;
    }

    .tab:hover {
        color: var(--txt);
        background: rgba(255, 255, 255, 0.05);
    }

    .tab.active {
        color: var(--acc);
        border-bottom-color: var(--acc);
        background: rgba(52, 211, 153, 0.05);
    }

    .tab-content {
        padding: 24px;
        flex: 1;
        display: flex;
        flex-direction: column;
        gap: 16px;
    }

    .config-section {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }

    /* Guardrails status rows */
    .guard-row {
        display: flex; align-items: flex-start; gap: 10px;
        padding: 10px 12px;
        border-radius: 6px;
        border: 1px solid var(--bdr);
        background: rgba(255,255,255,0.015);
    }
    .guard-row strong { color: var(--txt); font-size: 13px; }
    .guard-ok    { border-left: 3px solid #10b981; }
    .guard-warn  { border-left: 3px solid #f59e0b; background: rgba(245,158,11,0.04); }
    .guard-err   { border-left: 3px solid #ef4444; background: rgba(239,68,68,0.04); }
    .guard-muted { border-left: 3px solid var(--bdr); opacity: 0.7; }

    .re-check {
        background: rgba(255,255,255,0.04);
        border: 1px solid var(--bdr);
        color: var(--txt2);
        padding: 4px 10px;
        border-radius: 4px;
        font-size: 11px;
        cursor: pointer;
        transition: background .15s ease;
    }
    .re-check:hover { background: rgba(255,255,255,0.08); color: var(--txt); }

    .feature-badge {
        display: inline-block;
        background: rgba(52, 211, 153, 0.15);
        color: #10b981;
        padding: 6px 12px;
        border-radius: 6px;
        font-size: 12px;
        font-weight: 600;
        width: fit-content;
    }

    label {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    label span {
        font-size: 13px;
        font-weight: 500;
        color: var(--txt);
    }

    input[type="password"],
    input[type="text"] {
        padding: 10px 12px;
        border: 1px solid var(--bd);
        border-radius: 6px;
        background: var(--bg-alt);
        color: var(--txt);
        font-size: 13px;
        font-family: monospace;
        transition: all 0.2s;
    }

    input:focus {
        outline: none;
        border-color: var(--acc);
        background: var(--bg);
        box-shadow: 0 0 0 3px rgba(52, 211, 153, 0.1);
    }

    .hint {
        display: flex;
        align-items: center;
        gap: 8px;
        margin: 0;
        font-size: 12px;
        color: var(--txt3);
    }

    .health-status {
        padding: 12px 16px;
        border-left: 3px solid;
        border-radius: 6px;
        background: rgba(0, 0, 0, 0.2);
        display: flex;
        gap: 12px;
        align-items: flex-start;
    }

    .health-icon {
        flex-shrink: 0;
        margin-top: 2px;
    }

    .health-info {
        flex: 1;
    }

    .health-status-text {
        font-size: 13px;
        font-weight: 500;
        color: var(--txt);
    }

    .health-message {
        margin: 4px 0 0 0;
        font-size: 12px;
        color: var(--txt2);
    }

    .error-message,
    .success-message {
        padding: 12px 16px;
        border-radius: 6px;
        display: flex;
        gap: 8px;
        align-items: center;
        font-size: 13px;
    }

    .error-message {
        background: rgba(239, 68, 68, 0.15);
        color: #ef4444;
    }

    .success-message {
        background: rgba(16, 185, 129, 0.15);
        color: #10b981;
    }

    .modal-footer {
        padding: 16px 24px;
        border-top: 1px solid var(--bd);
        display: flex;
        justify-content: space-between;
        gap: 12px;
    }

    .btn-secondary,
    .btn-primary,
    .btn-test {
        padding: 10px 16px;
        border: none;
        border-radius: 6px;
        font-size: 13px;
        font-weight: 500;
        cursor: pointer;
        transition: all 0.2s;
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
    }

    .btn-secondary {
        background: transparent;
        color: var(--txt2);
        border: 1px solid var(--bd);
    }

    .btn-secondary:hover {
        background: var(--bg-alt);
        color: var(--txt);
    }

    .btn-test {
        background: var(--bg-alt);
        color: var(--txt2);
        border: 1px solid var(--bd);
    }

    .btn-test:hover:not(:disabled) {
        background: rgba(52, 211, 153, 0.1);
        color: #10b981;
    }

    .btn-primary {
        background: var(--acc);
        color: white;
    }

    .btn-primary:hover:not(:disabled) {
        background: #059669;
        box-shadow: 0 4px 12px rgba(16, 185, 129, 0.3);
    }

    button:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    .action-buttons {
        display: flex;
        gap: 12px;
    }
</style>
