<script lang="ts">
    import { createEventDispatcher, onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import StatusOrb from '$lib/StatusOrb.svelte';
    import type { CostSummary, TokenBudgetConfig } from '$lib/stores';
    import { densityMode, cycleDensityMode, densityFine, setDensityFine } from '$lib/density-mode';
    import { getPricing, pricingLabel } from '$lib/model-pricing';
    import { getModelIcon } from '$lib/models.js';
    import { computeCacheHitPct, cacheHitTier, type CacheStats } from '$lib/cache-stats-helpers';
    // v1.4.15 — live cost ticker. We tween the displayed cost value so it
    // rolls upward smoothly during streaming instead of teleporting after
    // each chunk's usage event. A brief 'pulse' class fires on each update.
    import { tweened } from 'svelte/motion';
    import { cubicOut } from 'svelte/easing';

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
        // Sprint 4, UI-7 — Prompt cache telemetry poll.
        // Reads the process-wide counters exposed by Rust `get_cache_stats`.
        // 8s cadence: cheap (one Mutex lock + JSON serialize) and matches the
        // user's reading pace — they don't need real-time refresh on a footer.
        const refreshCache = async () => {
            try { cacheStats = await invoke('get_cache_stats'); } catch {}
        };
        refreshCache();
        const cacheTimer = setInterval(refreshCache, 8000);
        return () => {
            window.removeEventListener('focus', onFocus);
            clearInterval(cacheTimer);
        };
    });

    // ── UI-7 — Cache hit footer indicator ────────────────────────────────
    // Compute logic lives in $lib/cache-stats-helpers (testable in vitest).
    // Only renders when Anthropic responses have actually exercised the cache.
    let cacheStats: CacheStats | null = null;
    $: cacheHitPct = computeCacheHitPct(cacheStats);

    // Visual mapping. The badge is intentionally subtle — we show it only
    // when it's `active` (a positive signal worth seeing) or when the
    // user explicitly opted in via `--features ml-guard` but the model
    // isn't loaded yet (actionable: tells them to finish setup).
    // v1.4.15 — tweened cost ticker. We feed the tweened store from the
    // raw cost; the displayed number animates over 500ms so token-burst
    // updates feel continuous. `costPulse` toggles a class for ~400ms
    // each time the cost increases — purely cosmetic feedback.
    const tweenedCost = tweened(0, { duration: 500, easing: cubicOut });
    let _lastCost = 0;
    let costPulse = false;
    let _pulseTimer: ReturnType<typeof setTimeout> | null = null;
    $: {
        const c = costSummaryMonth?.total_cost ?? 0;
        tweenedCost.set(c);
        if (c > _lastCost + 1e-6) {
            costPulse = true;
            if (_pulseTimer) clearTimeout(_pulseTimer);
            _pulseTimer = setTimeout(() => { costPulse = false; }, 420);
        }
        _lastCost = c;
    }

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

    <!-- U6 — Density mode pill: click to cycle focus → explore → war-room -->
    <button class="density-pill"
            on:click={cycleDensityMode}
            title={isEN
                ? `Density: ${$densityMode}. Click to cycle. Ctrl+1=Focus, Ctrl+2=Explore, Ctrl+3=War Room.`
                : `Densidad: ${$densityMode}. Click para alternar. Ctrl+1=Focus, Ctrl+2=Explore, Ctrl+3=War Room.`}>
        <span class="density-glyph">
            {$densityMode === 'focus'    ? '◉' :
             $densityMode === 'war-room' ? '▦' : '◫'}
        </span>
        <span>{$densityMode === 'war-room' ? 'WAR' : $densityMode.toUpperCase()}</span>
    </button>

    <!-- v1.4.16 — fine-grained density slider. Orthogonal to the 3-mode
         pill: tweaks --density-fine (0..1) globally so the user can dial
         in just a bit more breathing room inside any mode without losing
         their preset. Bound to the densityFine store. -->
    <label class="density-fine-wrap"
           title={isEN
               ? 'Fine density (0 = tighter, 1 = roomier). Stacks on top of the mode preset.'
               : 'Densidad fina (0 = más compacto, 1 = más espacioso). Se suma al modo elegido.'}>
        <input type="range" min="0" max="1" step="0.05"
               class="density-fine-range"
               value={$densityFine}
               on:input={(e) => setDensityFine(parseFloat(e.currentTarget.value))} />
    </label>

    {#if activeTab}
        {@const _model = getEffectiveModel(activeTab)}
        {@const _shortModel = _model.includes('/') ? _model.split('/').pop() : _model}
        {@const _pricing = getPricing(_model)}
        {@const _isFree = _pricing.inputPer1K === 0 && _pricing.outputPer1K === 0}
        {@const _modelIcon = getModelIcon(_model) || '◉'}
        <div class="bi" title={`${isEN ? 'Active model in this tab' : 'Modelo activo en esta pestaña'}: ${_model}`}>
            <span>{isEN ? 'Model:' : 'Modelo:'}</span><span class="cm">{_modelIcon} {_shortModel}</span>
        </div>

        <!-- ── Dynamic per-model rate pill ──
             Updates instantly when the user picks a different model or
             changes the effort level. The pill shows the WORK rate
             (input / output per 1M tokens) so the user can immediately
             see what each call will cost them. -->
        <div class="bi rate-pill" class:rate-free={_isFree}
             title={isEN
                ? `Rate for ${_shortModel}: ${pricingLabel(_model)}${_pricing.effort ? ` · effort ${_pricing.effort}` : ''}. Effort only multiplies token COUNT, not the per-token price.`
                : `Tarifa para ${_shortModel}: ${pricingLabel(_model)}${_pricing.effort ? ` · esfuerzo ${_pricing.effort}` : ''}. El esfuerzo solo multiplica el NÚMERO de tokens, no la tarifa por token.`}>
            <span>{isEN ? 'Rate:' : 'Tarifa:'}</span>
            {#if _isFree}
                <span class="rate-val rate-free-tag">{isEN ? 'Free · Local' : 'Gratis · Local'}</span>
            {:else}
                <span class="rate-val">
                    ${(_pricing.inputPer1K * 1000).toFixed(_pricing.inputPer1K < 0.001 ? 3 : 2)}
                    <span class="rate-sep">/</span>
                    ${(_pricing.outputPer1K * 1000).toFixed(_pricing.outputPer1K < 0.001 ? 3 : 2)}
                </span>
                <span class="rate-unit">/1M</span>
                {#if _pricing.effort}
                    <span class="rate-effort" title={isEN ? 'Effort multiplier on token count' : 'Multiplicador del nivel de esfuerzo sobre el conteo de tokens'}>·{_pricing.effort}</span>
                {/if}
            {/if}
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
            <span class="cost-num {_critical ? 'cr' : _warn ? 'cy' : 'cok'}" class:cost-pulse={costPulse}
                  >${$tweenedCost.toFixed(_budget > 0 && _budget < 1 ? 4 : 3)}</span>
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

    {#if cacheHitPct !== null && cacheStats}
        <!-- UI-7 — Prompt cache hit indicator. Only renders for sessions
             where Anthropic responses actually exercised the ephemeral cache. -->
        <div class="bi"
             title={isEN
                ? `Prompt cache (this session): ${cacheStats.calls_with_cache_activity}/${cacheStats.calls_total_anthropic} Anthropic calls used the cache. Read: ${cacheStats.cache_read_total.toLocaleString()} tokens at 0.1× price. Write: ${cacheStats.cache_creation_total.toLocaleString()} tokens at 1.25× price.`
                : `Cache de prompt (esta sesión): ${cacheStats.calls_with_cache_activity}/${cacheStats.calls_total_anthropic} llamadas Anthropic usaron caché. Leído: ${cacheStats.cache_read_total.toLocaleString()} tokens a 0.1× precio. Escrito: ${cacheStats.cache_creation_total.toLocaleString()} tokens a 1.25×.`}>
            <span>⚡</span><span class={cacheHitTier(cacheHitPct)} data-testid="cache-badge-pct">{cacheHitPct.toFixed(0)}% {isEN ? 'cached' : 'caché'}</span>
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
    /* v1.4.16 — fine density slider. Narrow range input next to the
       density pill; styled to match Lucy's footer rhythm. */
    .density-fine-wrap{display:inline-flex;align-items:center;margin-left:4px;height:18px;}
    .density-fine-range{
        -webkit-appearance:none; appearance:none;
        width:48px; height:3px;
        background:var(--bdr); border-radius:2px;
        cursor:pointer; outline:none;
    }
    .density-fine-range::-webkit-slider-thumb{
        -webkit-appearance:none; appearance:none;
        width:10px; height:10px; border-radius:50%;
        background:var(--acc, #10b981);
        border:none; cursor:pointer;
        box-shadow:0 0 4px color-mix(in srgb, var(--acc,#10b981) 50%, transparent);
    }
    .density-fine-range::-moz-range-thumb{
        width:10px; height:10px; border-radius:50%;
        background:var(--acc, #10b981); border:none; cursor:pointer;
    }

    /* v1.4.15 — live cost ticker pulse. Tabular-nums so the rolling
       digits don't reflow neighboring badges as they tween. */
    .cost-num{font-variant-numeric: tabular-nums; transition: text-shadow .2s;}
    .cost-pulse{text-shadow: 0 0 8px color-mix(in srgb, var(--acc, #10b981) 70%, transparent);}
    @media (prefers-reduced-motion: reduce) {
        .cost-pulse{ text-shadow: none; }
    }
    :global(:root:not(.light)) .bbar{border-top:1px solid var(--border-glass, var(--bdr))!important;}

    /* ── Dynamic per-model rate pill ─────────────────────────────────────
       Sits next to "Modelo:". Reactive — recomputes when the active tab
       or its selected model changes, including the ::effort suffix. */
    .rate-pill {
        cursor: help;
    }
    .rate-pill .rate-val {
        font-variant-numeric: tabular-nums;
        color: var(--amber, #f59e0b);
        font-weight: 600;
    }
    .rate-pill .rate-sep {
        opacity: 0.45;
        margin: 0 1px;
    }
    .rate-pill .rate-unit {
        font-size: 9px;
        opacity: 0.55;
        margin-left: 2px;
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }
    .rate-pill .rate-effort {
        font-family: var(--font-mono);
        font-size: 9px;
        margin-left: 4px;
        padding: 0 5px;
        border-radius: 7px;
        background: rgba(167, 139, 250, 0.10);
        color: var(--purple, #a78bfa);
        text-transform: uppercase;
        letter-spacing: 0.4px;
    }
    .rate-pill.rate-free .rate-val {
        color: var(--acc, #10b981);
    }
    .rate-pill .rate-free-tag {
        font-weight: 600;
        font-size: 10px;
    }
</style>
