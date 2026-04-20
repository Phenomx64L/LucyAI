<script>
    import { createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { IconKey as Key, IconWorld as Globe, IconDeviceFloppy as Save, IconAlertCircle as AlertCircle, IconCircleCheck as CheckCircle, IconSparkles, IconBrandGoogle, IconBrandOpenai, IconServer2 } from '@tabler/icons-svelte';

    const dispatch = createEventDispatcher();

    // Props
    export let isOpen = false;
    export let isEN = false;

    // State
    let loading = false;
    let error = '';
    let success = '';
    let activeTab = 'anthropic'; // 'anthropic', 'gemini', 'openai', 'ollama'
    let credentials = {
        anthropic: { key: '', configured: false },
        gemini: { key: '', configured: false },
        openai: { key: '', configured: false },
        ollama: { endpoint: 'http://localhost:11434', configured: false }
    };
    let healthStatus = {
        anthropic: null,
        gemini: null,
        openai: null,
        ollama: null
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
            ollama: {
                label: 'Endpoint Ollama',
                placeholder: 'http://localhost:11434',
                hint: 'Instala Ollama desde https://ollama.ai y descarga llava',
                feature: 'Privacidad Total - Ejecución Local'
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

    async function saveCredentials() {
        loading = true;
        error = '';
        success = '';

        try {
            // Save to keyring via Tauri
            if (credentials[activeTab].key || credentials[activeTab].endpoint) {
                const key = `${activeTab}_api_key`;
                const value = credentials[activeTab].key || credentials[activeTab].endpoint;

                await invoke('save_credential', { key, value });
                credentials[activeTab].configured = true;
                success = l.success;

                setTimeout(() => { success = ''; }, 3000);
            } else {
                error = l.required;
            }
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
            const result = await invoke('check_provider_health', { provider: activeTab });
            healthStatus[activeTab] = result;
            if (result.status === 'ok') {
                success = l.health.ok;
                setTimeout(() => { success = ''; }, 3000);
            } else {
                error = result.message || l.health.error;
            }
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
    <div class="modal-overlay" on:click={() => dispatch('close')}>
        <div class="modal-content" on:click|stopPropagation>
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
                        class:active={activeTab === 'ollama'}
                        on:click={() => activeTab = 'ollama'}
                    >
                        <span style="display:inline-flex;align-items:center;gap:6px;"><IconServer2 size={14} strokeWidth={1.8} color="#a78bfa" />Ollama</span>
                        {#if credentials.ollama.configured}
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
