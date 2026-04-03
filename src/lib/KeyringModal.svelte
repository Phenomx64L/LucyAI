<script lang="ts">
    import { createEventDispatcher, onMount } from 'svelte';
    import { ShieldAlert, ShieldCheck, Key, Shield } from 'lucide-svelte';
    import { testApiKey, saveLlmKey, getConfiguredProviders } from '$lib/lucy-api';

    const dispatch = createEventDispatcher();
    export let isEN = false;

    type Provider = 'gemini' | 'anthropic' | 'openai' | 'local';

    const providers: { id: Provider, name: string, icon: typeof Shield }[] = [
        { id: 'gemini', name: 'Google Gemini', icon: Shield },
        { id: 'anthropic', name: 'Anthropic Claude', icon: Shield },
        { id: 'openai', name: 'OpenAI GPT', icon: Shield },
        { id: 'local', name: 'Endpoint Local', icon: Shield },
    ];

    let activeTab: Provider = 'gemini';
    let keys: Record<Provider, string> = { gemini: '', anthropic: '', openai: '', local: '' };
    let configured: string[] = [];

    let loading = false;
    let errorMsg = '';
    let successMsg = '';

    onMount(async () => {
        try {
            configured = await getConfiguredProviders();
        } catch(e) {
            console.error("Error al leer la bóveda: ", e);
        }
    });

    async function handleSave() {
        if (!keys[activeTab]) {
            errorMsg = isEN ? 'API Key cannot be empty.' : 'La clave no puede estar vacía.';
            return;
        }

        loading = true;
        errorMsg = '';
        successMsg = '';

        try {
            await testApiKey(activeTab, keys[activeTab]);
            await saveLlmKey(activeTab, keys[activeTab]);
            
            successMsg = isEN ? 'Key securely saved to Vault!' : '¡Llave guardada de forma segura en la Bóveda!';
            if (!configured.includes(activeTab)) {
                configured = [...configured, activeTab];
            }
            keys[activeTab] = ''; // Limpiar el input después de guardar
        } catch (e: any) {
            errorMsg = e.toString();
        } finally {
            loading = false;
        }
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="kr-bg" on:click|self={() => dispatch('close')}>
    <div class="kr-box">
        <button class="kr-close" on:click={() => dispatch('close')}>✕</button>

        <div class="kr-hdr">
            <Key size={32} color="var(--acc)" style="margin-bottom:8px" />
            <h2 class="kr-title">{isEN ? 'Secure Keyring Vault' : 'Bóveda de Seguridad DPAPI'}</h2>
            <p class="kr-sub">
                {isEN 
                    ? 'All keys are encrypted locally using Windows Credential Manager' 
                    : 'Las llaves se encriptan a nivel de Sistema Operativo usando Credential Manager'}
            </p>
        </div>

        <div class="kr-tabs">
            {#each providers as prov}
                <button 
                    class="kr-tab {activeTab === prov.id ? 'active' : ''}" 
                    on:click={() => { activeTab = prov.id; errorMsg=''; successMsg=''; }}
                >
                    {prov.name}
                    {#if configured.includes(prov.id)}
                        <ShieldCheck size={14} color="var(--acc)" style="margin-left:5px"/>
                    {:else}
                        <ShieldAlert size={14} color="var(--red)" style="margin-left:5px"/>
                    {/if}
                </button>
            {/each}
        </div>

        <div class="kr-body">
            {#if configured.includes(activeTab)}
                <div class="kr-status configured">
                    <ShieldCheck size={18}/>
                    <span>{isEN ? 'API Key Configured and Protected' : 'API Key Configurada y Protegida'} (sk-*****)</span>
                </div>
            {:else}
                <div class="kr-status unconfigured">
                    <ShieldAlert size={18}/>
                    <span>
                        {#if activeTab === 'local'}
                            {isEN ? 'No endpoint configured' : 'Sin endpoint configurado'}
                        {:else}
                            {isEN ? 'No key configured' : 'Sin llave configurada'}
                        {/if}
                    </span>
                </div>
            {/if}

            <div class="kr-input-grp">
                <label for="kp">
                    {#if activeTab === 'local'}
                        {isEN ? 'Local Endpoint URL' : 'URL del Endpoint Local (Compatible con OpenAI API)'}
                    {:else}
                        API Key ({providers.find(p => p.id === activeTab)?.name})
                    {/if}
                </label>
                <input 
                    id="kp" 
                    type={activeTab === 'local' ? 'text' : 'password'} 
                    bind:value={keys[activeTab]} 
                    disabled={loading}
                    placeholder={activeTab === 'local' ? 'http://localhost:11434/v1/chat/completions' : 'sk-...'}
                    on:keydown={(e) => e.key === 'Enter' && handleSave()}
                />
            </div>

            {#if errorMsg}
                <div class="kr-error">{errorMsg}</div>
            {/if}
            {#if successMsg}
                <div class="kr-success">{successMsg}</div>
            {/if}

            <button class="kr-btn" on:click={handleSave} disabled={loading}>
                {#if loading}
                    <span class="kr-spin"></span> {isEN ? 'Verifying...' : 'Validando con API...'}
                {:else}
                    <Shield size={16}/> {isEN ? 'Encrypt & Save' : 'Encriptar y Guardar'}
                {/if}
            </button>
        </div>
    </div>
</div>

<style>
    .kr-bg {
        position: fixed; inset: 0; z-index: 10000;
        background: rgba(0,0,0,0.85); backdrop-filter: blur(5px);
        display: flex; align-items: center; justify-content: center;
    }
    .kr-box {
        background: var(--bg2, #0f141e);
        border: 1px solid var(--bdr, #1a2030);
        border-radius: 12px;
        width: 440px; max-width: 95vw;
        color: #fff;
        position: relative;
        box-shadow: 0 10px 40px rgba(0,0,0,0.8);
    }
    .kr-close {
        position: absolute; top: 12px; right: 12px;
        background: none; border: none; color: #888;
        font-size: 16px; cursor: pointer; transition: 0.2s;
    }
    .kr-close:hover { color: #fff; }
    
    .kr-hdr {
        padding: 30px 24px 20px;
        border-bottom: 1px solid rgba(255,255,255,0.05);
        text-align: center;
    }
    .kr-title { margin: 0 0 5px; font-size: 18px; font-weight: 600; }
    .kr-sub { margin: 0; font-size: 12px; color: #7a8a9a; }

    .kr-tabs {
        display: flex; background: rgba(0,0,0,0.3);
        border-bottom: 1px solid rgba(255,255,255,0.05);
    }
    .kr-tab {
        flex: 1; padding: 12px; background: none; border: none; color: #888;
        cursor: pointer; cursor: pointer; font-size: 13px; font-weight: 500;
        display: flex; align-items: center; justify-content: center;
        border-bottom: 2px solid transparent; transition: 0.2s;
    }
    .kr-tab:hover { color: #ddd; background: rgba(255,255,255,0.02); }
    .kr-tab.active {
        color: var(--acc, #10b981);
        border-bottom-color: var(--acc, #10b981);
        background: rgba(16,185,129,0.05);
    }

    .kr-body { padding: 24px; }
    
    .kr-status {
        display: flex; align-items: center; gap: 8px;
        padding: 10px 14px; border-radius: 6px; font-size: 13px; font-weight: 500; margin-bottom: 20px;
    }
    .kr-status.configured { background: rgba(16,185,129,0.1); color: var(--acc); border: 1px solid rgba(16,185,129,0.2); }
    .kr-status.unconfigured { background: rgba(255,68,68,0.1); color: var(--red); border: 1px solid rgba(255,68,68,0.2); }

    .kr-input-grp { margin-bottom: 20px; }
    .kr-input-grp label { display: block; font-size: 12px; color: #888; margin-bottom: 6px; }
    .kr-input-grp input {
        width: 100%; padding: 10px 12px; box-sizing: border-box;
        background: #000; border: 1px solid #333; border-radius: 6px;
        color: #fff; font-family: monospace; outline: none; transition: 0.2s;
    }
    .kr-input-grp input:focus { border-color: var(--acc); }

    .kr-btn {
        width: 100%; padding: 12px;
        background: var(--acc); color: #000;
        border: none; border-radius: 6px;
        font-size: 14px; font-weight: 600; cursor: pointer;
        display: flex; align-items: center; justify-content: center; gap: 8px;
        transition: 0.2s;
    }
    .kr-btn:hover:not(:disabled) { opacity: 0.9; transform: translateY(-1px); }
    .kr-btn:disabled { opacity: 0.5; cursor: not-allowed; }

    .kr-error { color: #ff5555; background: rgba(255,0,0,0.1); padding: 10px; border-radius: 6px; font-size: 13px; margin-bottom: 15px; border: 1px solid rgba(255,0,0,0.2); }
    .kr-success { color: #10b981; background: rgba(16,185,129,0.1); padding: 10px; border-radius: 6px; font-size: 13px; margin-bottom: 15px; border: 1px solid rgba(16,185,129,0.2); }

    .kr-spin {
        display: inline-block; width: 14px; height: 14px;
        border: 2px solid rgba(0,0,0,0.3); border-top-color: #000;
        border-radius: 50%; animation: spin 0.6s linear infinite;
    }
    @keyframes spin { to { transform: rotate(360deg); } }
</style>
