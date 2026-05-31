<script lang="ts">
    import { createEventDispatcher, onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import StatusOrb from '$lib/StatusOrb.svelte';
    import type { CostSummary, TokenBudgetConfig } from '$lib/stores';
    import { densityMode, cycleDensityMode } from '$lib/density-mode';
    // v1.4.17 — LucyTooltip migration (replaces native title=).
    import LucyTooltip from '$lib/LucyTooltip.svelte';
    // v1.4.21 — StatusBar layout CSS extracted to a single global stylesheet
    // so the same duplicate-selector trap that bit the tab strip
    // (v1.4.17 → v1.4.19) doesn't recur here.
    import '$lib/styles/status-bar.css';
    import { getPricing, pricingLabel } from '$lib/model-pricing';
    import { getModelIcon } from '$lib/models.js';
    import { computeCacheHitPct, cacheHitTier, type CacheStats } from '$lib/cache-stats-helpers';
    // v1.7.1 — LLM tier health chip.
    import { tierHealth, aggregateStatus, statusGlyph, pingAllTiers,
             type TierKey, type TierHealth } from '$lib/tier-health';
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

    // ── v1.7.1 — LLM tier health chip ────────────────────────────────────
    // The chip aggregates 3 tier probes (FAST / CHEAP / REASONING) into
    // a single glyph + label. Hover → per-tier breakdown via LucyTooltip.
    // Click → re-probe immediately, bypassing the 6h cache.
    let tierHealthBusy = false;
    async function reprobeTiers() {
        if (tierHealthBusy) return;
        tierHealthBusy = true;
        try { await pingAllTiers(); }
        finally { tierHealthBusy = false; }
    }
    $: tierHealthAgg     = aggregateStatus($tierHealth);
    $: tierHealthGlyph   = statusGlyph(tierHealthAgg);
    $: tierHealthTooltip = buildTierHealthTooltip($tierHealth, isEN);

    function buildTierHealthTooltip(s: Record<TierKey, TierHealth>, isEnLang: boolean): string {
        const order: TierKey[] = ['FAST', 'CHEAP', 'REASONING'];
        const lines = order.map(k => {
            const e = s[k];
            const lat = e.latency_ms > 0 ? ` (${e.latency_ms} ms)` : '';
            const err = e.error ? ` — ${e.error}` : '';
            return `${k}: ${e.status}${lat}${err}`;
        });
        const head = isEnLang ? 'LLM tier health (click to re-probe)' : 'Salud de tiers LLM (clic para re-probar)';
        return `${head}\n${lines.join('\n')}`;
    }

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

    <!-- U6 — Density mode pill: click to cycle focus → explore → war-room
         v1.4.17 — wrapped in LucyTooltip (replaces native title=); now
         shows on keyboard focus too, with a 350ms delay token. -->
    <LucyTooltip text={isEN
            ? `Density: ${$densityMode}. Click to cycle. Ctrl+1=Focus, Ctrl+2=Explore, Ctrl+3=War Room.`
            : `Densidad: ${$densityMode}. Click para alternar. Ctrl+1=Focus, Ctrl+2=Explore, Ctrl+3=War Room.`}>
        <button class="density-pill" on:click={cycleDensityMode}>
            <span class="density-glyph">
                {$densityMode === 'focus'    ? '◉' :
                 $densityMode === 'war-room' ? '▦' : '◫'}
            </span>
            <span>{$densityMode === 'war-room' ? 'WAR' : $densityMode.toUpperCase()}</span>
        </button>
    </LucyTooltip>

    <!-- v1.5.5 — density-fine range slider removed per user feedback.
         The 3-mode density pill above (Focus / Explore / War-room)
         already gives the user the gross-grained control they actually
         use; the 0..1 fine-tune added in v1.4.16 was visual noise next
         to the FOCUS pill without delivering proportional value. The
         densityFine store + setDensityFine function stay in
         $lib/density-mode for any future surface that wants to expose
         it (Settings modal, density section there). -->

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

    <!-- v1.7.1 — LLM tier health chip. Aggregates 3 tier probes into
         one glyph. Hover for breakdown, click to re-probe. -->
    <div class="bi th-chip" title={tierHealthTooltip}
         on:click={reprobeTiers} role="button" tabindex="0"
         on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') reprobeTiers(); }}>
        <span class="th-glyph th-{tierHealthGlyph.tone}" style="letter-spacing:.3px;">
            {tierHealthBusy ? '⟳' : tierHealthGlyph.glyph} LLM
        </span>
    </div>

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
    /* v1.4.21 — All static layout for the bottom bar lives in
       $lib/styles/status-bar.css (imported from <script>). This file
       is intentionally empty: every former rule here was a duplicate
       of page.css, and page.css's copy silently won the cascade. The
       layout file is now the single source of truth.

       If a future StatusBar-specific rule needs to be added (something
       not in status-bar.css and not a duplicate elsewhere), put it
       here. Otherwise edit status-bar.css. */

    /* v1.7.1 — LLM tier health chip. The chip lives in the footer
       between GUARD and version. Click to re-probe; hover for per-tier
       breakdown via the native title= tooltip. */
    .th-chip { cursor: pointer; user-select: none; transition: opacity .12s; }
    .th-chip:hover  { opacity: .85; }
    .th-chip:active { opacity: .65; }
    .th-glyph { font-family: var(--mono, ui-monospace, monospace); font-size: 11px; font-weight: 600; }
    .th-ok    { color: var(--acc,   #10b981); }
    .th-warn  { color: var(--amber, #f59e0b); }
    .th-crit  { color: var(--red,   #ef4444); }
    .th-info  { color: var(--txt2,  #94a3b8); opacity: .7; }
</style>
