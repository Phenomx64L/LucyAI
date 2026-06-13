<script lang="ts">
    import { createEventDispatcher, onMount } from 'svelte';
    import ShieldAlert from '@tabler/icons-svelte/icons/shield-exclamation';

    import ShieldCheck from '@tabler/icons-svelte/icons/shield-check';

    import Key from '@tabler/icons-svelte/icons/key';

    import Shield from '@tabler/icons-svelte/icons/shield';
    import { testApiKey, saveLlmKey, getConfiguredProviders } from '$lib/lucy-api';
    import { refreshNvidiaModels } from '$lib/models.js';

    const dispatch = createEventDispatcher();
    export let isEN = false;

    type Provider = 'gemini' | 'anthropic' | 'openai' | 'nvidia' | 'local';

    const providers: { id: Provider, name: string, icon: typeof Shield }[] = [
        { id: 'gemini',    name: 'Google Gemini',    icon: Shield },
        { id: 'anthropic', name: 'Anthropic Claude',  icon: Shield },
        { id: 'openai',    name: 'OpenAI GPT',        icon: Shield },
        { id: 'nvidia',    name: 'NVIDIA NIM',        icon: Shield },
        { id: 'local',     name: 'Endpoint Local',    icon: Shield },
    ];

    let activeTab: Provider = 'gemini';
    let keys: Record<Provider, string> = { gemini: '', anthropic: '', openai: '', nvidia: '', local: '' };
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
            keys[activeTab] = '';
            // Si se guardó NVIDIA, actualizar el catálogo de modelos inmediatamente
            if (activeTab === 'nvidia') {
                refreshNvidiaModels().catch(() => {});
            }
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
            <span class="kr-hdr-ico"><Key size={24} color="var(--acc, #10b981)"/></span>
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
                    {:else if activeTab === 'nvidia'}
                        {isEN ? 'NVIDIA NIM API Key — get it free at build.nvidia.com' : 'NVIDIA NIM API Key — gratis en build.nvidia.com'}
                    {:else}
                        API Key ({providers.find(p => p.id === activeTab)?.name})
                    {/if}
                </label>
                <input
                    id="kp"
                    type={activeTab === 'local' ? 'text' : 'password'}
                    bind:value={keys[activeTab]}
                    disabled={loading}
                    placeholder={activeTab === 'local' ? 'http://localhost:11434/v1/chat/completions' : activeTab === 'nvidia' ? 'nvapi-...' : 'sk-...'}
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
        background: rgba(0,0,0,0.85); backdrop-filter: blur(6px) saturate(120%);
        display: flex; align-items: center; justify-content: center;
        animation: kr-bg-in .18s ease;
    }
    @keyframes kr-bg-in { from { opacity: 0; } to { opacity: 1; } }
    .kr-box {
        /* Depth: a faint vault-green top glow over the base instead of a flat slab. */
        background:
            radial-gradient(120% 80% at 50% -10%, rgba(16,185,129,.10) 0%, transparent 60%),
            var(--bg2, #0f141e);
        border: 1px solid var(--bdr, #1a2030);
        border-top: 3px solid var(--acc, #10b981);
        border-radius: 12px;
        width: 440px; max-width: 95vw;
        color: #fff;
        position: relative;
        box-shadow:
            0 24px 64px rgba(0,0,0,.8),
            0 -1px 28px -10px rgba(16,185,129,.4);
        animation: kr-box-in .26s var(--ease-out, cubic-bezier(.16,1,.3,1));
    }
    @keyframes kr-box-in { from { opacity: 0; transform: translateY(14px) scale(.98); } to { opacity: 1; transform: none; } }
    .kr-close {
        position: absolute; top: 12px; right: 12px;
        background: none; border: none; color: #888;
        font-size: 16px; cursor: pointer; transition: 0.2s; border-radius: 5px; padding: 2px 6px;
    }
    .kr-close:hover { color: #fff; background: rgba(255,255,255,.06); }

    .kr-hdr {
        padding: 26px 24px 18px;
        border-bottom: 1px solid rgba(255,255,255,0.05);
        text-align: center;
    }
    .kr-hdr-ico {
        display: inline-flex; align-items: center; justify-content: center;
        width: 46px; height: 46px; border-radius: 13px; margin-bottom: 10px;
        background: rgba(16,185,129,.12);
        border: 1px solid rgba(16,185,129,.28);
        box-shadow: 0 0 18px -4px rgba(16,185,129,.5);
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
    .kr-input-grp input:focus { border-color: var(--acc); box-shadow: 0 0 0 3px rgba(16,185,129,.18); }

    .kr-btn {
        width: 100%; padding: 12px;
        background: var(--acc); color: #000;
        border: none; border-radius: 6px;
        font-size: 14px; font-weight: 600; cursor: pointer;
        display: flex; align-items: center; justify-content: center; gap: 8px;
        transition: 0.2s;
    }
    .kr-btn:hover:not(:disabled) { opacity: 0.95; transform: translateY(-1px); box-shadow: 0 6px 18px -6px rgba(16,185,129,.6); }
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
