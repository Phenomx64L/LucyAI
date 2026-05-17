<script lang="ts">
    import { createEventDispatcher, onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import StatusOrb from '$lib/StatusOrb.svelte';
    import type { CostSummary, TokenBudgetConfig } from '$lib/stores';

    export let hostName: string = '---';
    export let lucyConfig: { name: string } = { name: '' };
    export let activeTab: any = null;
    export let keyringOk: boolean = true;
    export let auditAlerts: number = 0;
    export let appVersion: string = '---';
    export let userLang: string = 'es-MX';
    export let isEN: boolean = false;
    export let lucyState: string = 'idle';
    export let appReady: boolean = false;
    export let showSetupOverlay: boolean = false;
    export let costSummaryMonth: CostSummary | null = null;
    export let tokenBudgetConfig: TokenBudgetConfig | null = null;
    export let getEffectiveModel: (tab: any) => string = (t) => t?.selectedModel || '';

    const dispatch = createEventDispatcher<{ changelang: string }>();

    // ── PromptGuard 2 ML status (Phase 2 LlamaFirewall, May 2026) ──
    // Probed once at mount. Drives the small "ML" badge after "GUARD".
    // Re-probed on demand if the user installs the model — we listen for
    // a focus event so when they alt-tab back from installing, status
    // refreshes without a full app restart.
    type GuardStatus = 'active' | 'model_missing' | 'runtime_missing' | 'feature_disabled' | 'failed';
    let mlStatus: GuardStatus | null = null;
    let mlNote = '';
    async function probeMlGuard() {
        try {
            const r = await invoke<{ status: GuardStatus; note: string | null }>('prompt_guard_status');
            mlStatus = r.status;
            mlNote = r.note ?? '';
        } catch (e) {
            mlStatus = 'failed';
            mlNote = String(e);
        }
    }
    onMount(() => {
        probeMlGuard();
        const onFocus = () => probeMlGuard();
        window.addEventListener('focus', onFocus);
        return () => window.removeEventListener('focus', onFocus);
    });

    // Visual mapping. The badge is intentionally subtle — we show it only
    // when it's `active` (a positive signal worth seeing) or when the
    // user explicitly opted in via `--features ml-guard` but the model
    // isn't loaded yet (actionable: tells them to finish setup).
    $: mlBadge =
        mlStatus === 'active'           ? { txt: '🧠 ML', cls: 'cok',  tip: isEN ? 'PromptGuard 2 ML active — catches paraphrased prompt injection.' : 'PromptGuard 2 ML activo — detecta prompt injection parafraseado.' }
      : mlStatus === 'model_missing'   ? { txt: '🧠 ?',  cls: 'cy',   tip: isEN ? 'ML feature compiled but PromptGuard model not installed. See PROMPT_GUARD_INSTALL.md.' : 'Feature ML compilado pero modelo PromptGuard no instalado. Ver PROMPT_GUARD_INSTALL.md.' }
      : mlStatus === 'runtime_missing' ? { txt: '🧠 !',  cls: 'cy',   tip: isEN ? 'PromptGuard model is on disk but ONNX Runtime DLL is missing.' : 'Modelo PromptGuard en disco pero falta ONNX Runtime DLL.' }
      : mlStatus === 'failed'          ? { txt: '🧠 ✕',  cls: 'cr',   tip: `ML load failed: ${mlNote}` }
      : null;  // feature_disabled / null → no badge (default build)
</script>

{#if !showSetupOverlay}
<div class="bbar">
    {#if hostName !== '---'}
    <div class="bi"><span>Host:</span><span style="color:#0f7b5a;">{lucyConfig.name} · {hostName}</span></div>
    {/if}

    {#if activeTab}
        {@const _model = getEffectiveModel(activeTab)}
        {@const _shortModel = _model.includes('/') ? _model.split('/').pop() : _model}
        <div class="bi" title={`${isEN ? 'Active model in this tab' : 'Modelo activo en esta pestaña'}: ${_model}`}>
            <span>{isEN ? 'Model:' : 'Modelo:'}</span><span class="cm">◇ {_shortModel}</span>
        </div>
    {/if}

    {#if costSummaryMonth && costSummaryMonth.total_cost > 0}
        {@const _budget = tokenBudgetConfig?.monthlyLimit || 0}
        {@const _pct = _budget > 0 ? (costSummaryMonth.total_cost / _budget) * 100 : 0}
        {@const _critical = _budget > 0 && _pct >= (tokenBudgetConfig?.alertThreshold || 80)}
        {@const _warn = _budget > 0 && _pct >= 60 && !_critical}
        <div class="bi" title={_budget > 0
            ? `${isEN ? 'This month' : 'Este mes'}: $${costSummaryMonth.total_cost.toFixed(4)} de $${_budget.toFixed(2)} (${_pct.toFixed(1)}%) · ${costSummaryMonth.total_tokens.toLocaleString()} tokens · ${costSummaryMonth.request_count} ${isEN?'requests':'consultas'}`
            : `${isEN ? 'This month' : 'Este mes'}: $${costSummaryMonth.total_cost.toFixed(4)} · ${costSummaryMonth.total_tokens.toLocaleString()} tokens`}>
            <span>{isEN ? 'Cost:' : 'Costo:'}</span>
            <span class="{_critical ? 'cr' : _warn ? 'cy' : 'cok'}">${costSummaryMonth.total_cost.toFixed(_budget > 0 && _budget < 1 ? 4 : 3)}</span>
            {#if _budget > 0}
                <span class="cost-budget-track" title="Budget: ${_budget.toFixed(2)}">
                    <span class="cost-budget-fill {_critical ? 'cr-bg' : _warn ? 'cy-bg' : 'cok-bg'}" style="width:{Math.min(100, _pct).toFixed(1)}%;"></span>
                </span>
            {/if}
        </div>
    {/if}

    {#if activeTab?._streamTPS && activeTab._streamTPS > 0}
        <div class="bi" title={`${isEN ? 'Tokens per second' : 'Tokens por segundo'}${activeTab._streamTTFT ? ` · TTFT ${activeTab._streamTTFT}ms` : ''}`}>
            <span>{isEN ? 'Stream:' : 'Stream:'}</span><span class="cok">~{activeTab._streamTPS} t/s</span>
        </div>
    {/if}

    {#if !keyringOk}
        <div class="bi" title={isEN ? 'Keyring unavailable — credentials cannot be saved securely' : 'Keyring no disponible — las credenciales no se pueden guardar de forma segura'}>
            <span>⚠</span><span class="cr">{isEN ? 'Keyring failed' : 'Keyring falló'}</span>
        </div>
    {/if}

    {#if auditAlerts > 0}
        <div class="bi"><span>Alertas:</span><span class="cy">{auditAlerts} bypass</span></div>
    {/if}

    <!-- Guardrails indicator (audit S1/S2/S5/S10 defense layer, Lucy 1.3.1+; ML badge added in 1.4.0) -->
    <div class="bi" title={isEN
        ? 'Guardrail layer active — scans inputs for prompt injection, SSRF, UAC injection, cmd bypass shapes'
        : 'Capa de Guardrails activa — escanea entradas en busca de prompt injection, SSRF, UAC injection, bypass de cmd'}>
        <span class="cok" style="letter-spacing:.3px;">🛡 GUARD</span>
    </div>

    <!-- PromptGuard 2 ML indicator (Phase 2 LlamaFirewall) — only shown when relevant -->
    {#if mlBadge}
        <div class="bi" title={mlBadge.tip}>
            <span class={mlBadge.cls} style="letter-spacing:.3px;">{mlBadge.txt}</span>
        </div>
    {/if}

    <div class="bi r" style="opacity:0.6; font-size:12px;">
        Lucy OS v{appVersion} · {userLang}
    </div>

    <StatusOrb state={lucyState} visible={appReady && !showSetupOverlay} inline={true}
               label={isEN
                  ? `Lucy: ${lucyState}`
                  : `Lucy: ${lucyState === 'idle' ? 'inactiva' : lucyState === 'thinking' ? 'pensando' : lucyState === 'executing' ? 'ejecutando' : 'error'}`} />
</div>
{/if}

<style>
    .bbar{display:flex;flex-direction:row;align-items:center;height:22px;background:#0b0d14;border-top:1px solid var(--bdr);padding:0 12px;font-size:10px;color:var(--txt3);flex-shrink:0;}
    .bi{display:flex;align-items:center;gap:4px;padding-right:10px;margin-right:10px;border-right:1px solid var(--bdr);white-space:nowrap;}
    .bi:last-child{border-right:none;margin-right:0;}
    .bi.r{margin-left:auto;}
    .cok{color:var(--acc);}.cy{color:var(--amber);}.cr{color:var(--red);}
    .cm{color:#7dd3fc;font-family:var(--mono);font-size:10px;font-weight:600;letter-spacing:.2px;max-width:160px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;display:inline-block;vertical-align:middle;}
    :global(:root.light .cm){color:#0369a1;}
    .cost-budget-track{display:inline-block;width:42px;height:3px;background:var(--bdr);border-radius:2px;margin-left:5px;vertical-align:middle;position:relative;overflow:hidden;}
    .cost-budget-fill{position:absolute;left:0;top:0;height:100%;border-radius:2px;transition:width .4s ease;}
    .cost-budget-fill.cok-bg{background:var(--acc);}
    .cost-budget-fill.cy-bg{background:var(--amber);}
    .cost-budget-fill.cr-bg{background:var(--red);box-shadow:0 0 6px rgba(239,68,68,.45);}
    :global(:root:not(.light)) .bbar{border-top:1px solid var(--border-glass, var(--bdr))!important;}
</style>
