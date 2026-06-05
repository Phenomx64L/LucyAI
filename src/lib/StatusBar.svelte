<script lang="ts">
    import { createEventDispatcher, onMount, onDestroy } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import StatusOrb from '$lib/StatusOrb.svelte';
    import type { CostSummary, TokenBudgetConfig } from '$lib/stores';
    import { densityMode, cycleDensityMode } from '$lib/density-mode';
    // v1.4.17 — LucyTooltip migration (replaces native title=).
    import LucyTooltip from '$lib/LucyTooltip.svelte';
    // v1.7.27 — Inline sparkline for the stream-t/s chip.
    import Sparkline from '$lib/Sparkline.svelte';
    // v1.4.21 — StatusBar layout CSS extracted to a single global stylesheet
    // so the same duplicate-selector trap that bit the tab strip
    // (v1.4.17 → v1.4.19) doesn't recur here.
    import '$lib/styles/status-bar.css';
    import { getPricing, pricingLabel } from '$lib/model-pricing';
    import { getModelIcon } from '$lib/models.js';
    import { computeCacheHitPct, cacheHitTier, type CacheStats } from '$lib/cache-stats-helpers';
    // v1.7.1 — LLM tier health chip.
    import { tierHealth, aggregateStatus, statusGlyph, pingAllTiers,
             getLatencyStats, tierBreaker,
             type TierKey, type TierHealth } from '$lib/tier-health';
    // v1.4.15 — live cost ticker. We tween the displayed cost value so it
    // rolls upward smoothly during streaming instead of teleporting after
    // each chunk's usage event. A brief 'pulse' class fires on each update.
    import { tweened } from 'svelte/motion';
    import { cubicOut } from 'svelte/easing';

    export let hostName: string = '---';
    // v1.7.75 — `lucyConfig` removed. The pre-1.7.75 Host chip rendered
    // "Iván · PRECISION-X" combining the user's display name with the
    // hostname. The trimmed v1.7.75 chip shows only the hostname; the
    // user identity belongs in the welcome hero, not in the chrome.
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

    // v1.7.75 — Mission Strip chips folded in. These were on the top band
    // until v1.7.74; consolidating into the StatusBar frees the corner
    // above the close button and eliminates the hostname+posture
    // duplication that already existed between the strip and this bar.
    export let remoteHostsTotal: number = 0;
    export let remoteHostsOnline: number = 0;
    export let activeAlerts: number = 0;
    export let guardLabel: string = '';
    export let posture: 0 | 1 | 2 | 3 | 4 = 0;

    const dispatch = createEventDispatcher<{
        changelang: string;
        clickHosts: void;
        clickAlerts: void;
        clickGuard: void;
        clickPosture: void;
    }>();

    // ── v1.7.75 — Local clock (folded from MissionStrip) ─────────────────
    // Updates once per minute, aligned to the next minute boundary.
    let _now: string = '';
    let _clockTimer: ReturnType<typeof setInterval> | null = null;
    function _formatNow(): string {
        const d = new Date();
        return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
    }
    onMount(() => {
        _now = _formatNow();
        const msUntilNextMinute = (60 - new Date().getSeconds()) * 1000;
        const _bootstrap = setTimeout(() => {
            _now = _formatNow();
            _clockTimer = setInterval(() => { _now = _formatNow(); }, 60_000);
        }, msUntilNextMinute);
        return () => clearTimeout(_bootstrap);
    });
    onDestroy(() => { if (_clockTimer) clearInterval(_clockTimer); });

    // Severity classes for the folded-in chips. Same colour vocabulary as
    // the rest of the bar (cok / cy / cr / cm).
    $: hostsTone = remoteHostsTotal === 0           ? 'cm'
                 : remoteHostsOnline === remoteHostsTotal ? 'cok'
                 : remoteHostsOnline === 0           ? 'cr'
                 : 'cy';
    $: alertsTone = activeAlerts === 0  ? 'cok'
                  : activeAlerts === 1  ? 'cy'
                  : 'cr';
    $: guardTone  = guardLabel ? 'cv' : 'cm';   // violet when a skill is active
    $: postureTone = posture <= 0 ? 'cok'
                   : posture === 1 ? 'cy'
                   : posture === 2 ? 'cy'
                   : posture === 3 ? 'cr'
                   : 'cr';
    $: guardLabelShort = (() => {
        const s = String(guardLabel || '').trim();
        if (!s) return isEN ? 'clean' : 'limpio';
        return s.length > 28 ? s.slice(0, 27) + '…' : s;
    })();

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
    // v1.7.31 — Cost sparkline data. Pulls last 7 days of total_cost from
    // `daily_summary` via `get_cost_by_day`. Polled every 60s and after
    // any focus event so a cross-app workflow re-reads when the user
    // returns. The shape `[{date, cost}]` is folded into a number[] for
    // the Sparkline component.
    let costByDay: { date: string; cost: number }[] = [];
    // v1.7.69 — Defensive `?? []`. The reactive `.map` runs on every
    // assignment to `costByDay`; if the backend command isn't
    // registered (older builds), the test environment, or any path
    // returns null/undefined, the bare `.map` blew up the StatusBar
    // mount and cascaded into the failed cache-badge tests. Guard
    // here AND at the assignment site so neither path can break.
    $: costSeries = (costByDay ?? []).map(p => p.cost);
    async function refreshCostByDay() {
        try {
            const r = await invoke<{ date: string; cost: number }[]>('get_cost_by_day', { days: 7 });
            costByDay = Array.isArray(r) ? r : [];
        } catch (e) {
            // Silent — the backend command might not exist in older builds.
            // The chip just renders without the sparkline in that case.
            costByDay = [];
        }
    }

    onMount(() => {
        probeMlGuard();
        refreshCostByDay();
        const onFocus = () => { probeMlGuard(); refreshCostByDay(); };
        window.addEventListener('focus', onFocus);
        // Sprint 4, UI-7 — Prompt cache telemetry poll.
        const refreshCache = async () => {
            try { cacheStats = await invoke('get_cache_stats'); } catch {}
        };
        refreshCache();
        const cacheTimer = setInterval(refreshCache, 8000);
        // v1.7.31 — cost-by-day refresh every 60s. Cheap (single SQL agg).
        const costTimer = setInterval(refreshCostByDay, 60_000);
        return () => {
            window.removeEventListener('focus', onFocus);
            clearInterval(cacheTimer);
            clearInterval(costTimer);
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
        const breaker = $tierBreaker;
        const lines = order.map(k => {
            const e = s[k];
            const lat = e.latency_ms > 0 ? ` (${e.latency_ms} ms)` : '';
            const err = e.error ? ` — ${e.error}` : '';
            const stats = getLatencyStats(k);
            // v1.7.3: append 7-day p50/p95 when we have samples, plus
            // breaker indicator when open.
            const hist  = stats.samples > 0 ? `  [7d p50 ${stats.p50}ms · p95 ${stats.p95}ms · n=${stats.samples}]` : '';
            const brk   = breaker[k]?.is_open ? '  ⚡BREAKER OPEN' : '';
            return `${k}: ${e.status}${lat}${err}${hist}${brk}`;
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
    <!-- v1.7.75 — Host chip kept but trimmed: just the hostname (no
         "Host:" label, no user name prefix). Lucy's title bar carries
         the user's identity already; here we only need the machine. -->
    {#if hostName !== '---'}
    <div class="bi sb-host" title={`${isEN ? 'Local host' : 'Host local'}: ${hostName}`}>
        <span class="sb-host-dot" aria-hidden="true"></span>
        <span style="color:#0f7b5a;">{hostName}</span>
    </div>
    {/if}

    <!-- v1.7.75 — Mission Strip chips folded in: remote hosts, alerts,
         guard skill, clock, posture. Each is clickable and routes to
         the same view the strip's chip used to (NexShell, Dashboard,
         skill picker, Diagnostics). -->
    <button class="bi sb-ms-chip" type="button"
            on:click={() => dispatch('clickHosts')}
            title={isEN
                ? `Remote hosts online / total. Click to open NexShell.`
                : `Hosts remotos online / total. Click para abrir NexShell.`}>
        <span class="sb-ms-glyph">⚯</span>
        <span class={hostsTone}>{remoteHostsOnline}/{remoteHostsTotal}</span>
        <span class="sb-ms-unit">{isEN ? 'hosts' : 'hosts'}</span>
    </button>

    <button class="bi sb-ms-chip" type="button"
            on:click={() => dispatch('clickAlerts')}
            title={isEN
                ? `Active incident alerts. Click to open Dashboard.`
                : `Alertas de incidente activas. Click para abrir Dashboard.`}>
        <span class="sb-ms-glyph">⚠</span>
        <span class={alertsTone}>{activeAlerts}</span>
        <span class="sb-ms-unit">{isEN ? 'alerts' : 'alertas'}</span>
    </button>

    <button class="bi sb-ms-chip sb-ms-guard" type="button"
            on:click={() => dispatch('clickGuard')}
            title={isEN
                ? `Active security skill / guard. Click to change.`
                : `Skill / guard activo. Click para cambiar.`}>
        <span class="sb-ms-glyph">⊕</span>
        <span class={guardTone}>{guardLabelShort}</span>
    </button>

    <button class="bi sb-ms-chip sb-ms-posture" type="button"
            on:click={() => dispatch('clickPosture')}
            title={isEN
                ? `Operational posture: calm → vigilant → suspicious → alarmed → panic. Click for Diagnostics.`
                : `Postura operacional: calmo → vigilante → sospechoso → alarmado → pánico. Click para Diagnóstico.`}>
        {#each [0,1,2,3,4] as i}
            <span class="sb-posture-dot {i <= posture ? 'on ' + postureTone : ''}" aria-hidden="true"></span>
        {/each}
    </button>

    <span class="bi sb-ms-clock" aria-label={isEN ? 'Local time' : 'Hora local'}>
        <span class="sb-ms-glyph">◷</span>
        <span>{_now}</span>
    </span>

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
        <!-- v1.7.75 — Model name removed from the StatusBar. The composer's
             own model badge (.mbdg) shows the active model with proper
             label, icon, and dropdown to switch. Keeping it here was a
             stale duplicate that wasted horizontal space.

             The Rate chip stays — pricing is decision-critical info and
             doesn't appear anywhere else in the chrome. -->
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
            <!-- v1.7.31 — 7-day cost sparkline. Bars (not line) so days
                 with zero spend remain visible as a baseline. -->
            {#if costSeries.length > 1 && costSeries.some(v => v > 0)}
                <span class="sb-cost-spark" title="{isEN ? 'Cost last 7 days' : 'Costo últimos 7 días'}: {costByDay.map(p => `${p.date.slice(5)} $${p.cost.toFixed(3)}`).join(' · ')}">
                    <Sparkline values={costSeries}
                               width={36} height={11}
                               kind="bar"
                               stroke={_critical ? 'var(--red, #ef4444)' : _warn ? 'var(--amber, #f59e0b)' : 'var(--acc, #10b981)'} />
                </span>
            {/if}
        </div>
    {/if}

    {#if activeTab?._streamTPS && activeTab._streamTPS > 0}
        <div class="bi sb-stream"
             title={`${isEN ? 'Tokens per second (last 30s)' : 'Tokens por segundo (últimos 30s)'}${activeTab._streamTTFT ? ` · TTFT ${activeTab._streamTTFT}ms` : ''}`}>
            <span>{isEN ? 'Stream:' : 'Stream:'}</span>
            <span class="cok">~{activeTab._streamTPS}</span>
            {#if Array.isArray(activeTab._streamTpsHistory) && activeTab._streamTpsHistory.length > 1}
                <span class="sb-stream-spark">
                    <Sparkline values={activeTab._streamTpsHistory}
                               width={42} height={12}
                               kind="line"
                               stroke="var(--acc, #10b981)"
                               fill="var(--acc, #10b981)" />
                </span>
            {/if}
            <span class="sb-stream-unit">t/s</span>
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

    <!-- v1.7.25 — Guardrails indicator with per-layer LED dots.
         Replaces the single "GUARD" text with the brand glyph + a row
         of 5 mini LEDs (S1, S2, S5, S8, S10) so the user gets a
         per-layer at-a-glance read. All green when the layers are
         active, amber for ML downgraded, red for breached. Currently
         all layers are static-green; layer health hookups come in a
         later sprint (tracked). -->
    <div class="bi sb-guard" title={isEN
        ? 'Guardrail layer active — S1 destructive · S2 bypass shapes · S5 prompt injection · S8 force-execute · S10 UAC elevation'
        : 'Guardrails activos — S1 destructivo · S2 bypass · S5 prompt-injection · S8 force-execute · S10 elevación UAC'}>
        <span class="sb-guard-glyph">🛡</span>
        <span class="sb-guard-dots" aria-hidden="true">
            <span class="sb-led sb-led-ok" data-layer="S1"></span>
            <span class="sb-led sb-led-ok" data-layer="S2"></span>
            <span class="sb-led sb-led-ok" data-layer="S5"></span>
            <span class="sb-led sb-led-ok" data-layer="S8"></span>
            <span class="sb-led sb-led-ok" data-layer="S10"></span>
        </span>
    </div>

    <!-- PromptGuard 2 ML indicator (Phase 2 LlamaFirewall) — only shown
         when relevant. v1.7.31 — aligned with the GUARD/LLM LED-and-ring
         visual system so the three security/observability chips read as
         one family. Glyph + single LED dot, tinted by status. -->
    {#if mlBadge}
        {@const _mlTone = mlStatus === 'active'           ? 'ok'
                       :  mlStatus === 'feature_disabled' ? 'idle'
                       :  mlStatus === 'model_missing' || mlStatus === 'runtime_missing' ? 'warn'
                       :  'crit'}
        <div class="bi sb-ml" title={mlBadge.tip}>
            <span class="sb-ml-glyph">🧠</span>
            <span class="sb-led sb-led-{_mlTone}" aria-hidden="true"></span>
            <span class="sb-ml-label">ML</span>
        </div>
    {/if}

    <!-- v1.7.1 — LLM tier health chip + v1.7.25 — three per-tier mini
         rings (FAST, CHEAP, REASONING). The aggregate glyph stays for
         scan-ability; the rings give per-tier signal at a glance:
         green = ok, amber = slow, red = fail, grey = unknown. -->
    <div class="bi th-chip sb-llm-rings" title={tierHealthTooltip}
         on:click={reprobeTiers} role="button" tabindex="0"
         on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') reprobeTiers(); }}>
        <span class="th-glyph th-{tierHealthGlyph.tone}" style="letter-spacing:.3px;">
            {tierHealthBusy ? '⟳' : tierHealthGlyph.glyph} LLM
        </span>
        <span class="sb-rings" aria-hidden="true">
            {#each ['FAST', 'CHEAP', 'REASONING'] as tier (tier)}
                {@const t = $tierHealth[tier as TierKey]}
                {@const tone = !t || t.status === 'unknown' ? 'idle'
                            :  t.status === 'ok'    ? 'ok'
                            :  t.status === 'slow'  ? 'warn'
                            : 'crit'}
                <span class="sb-ring sb-ring-{tone}"
                      data-tier={tier}
                      title="{tier}: {t?.status ?? 'unknown'}{t?.latency_ms ? ` (${t.latency_ms}ms)` : ''}"></span>
            {/each}
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

    /* ── v1.7.25 — Visual upgrades to GUARD + LLM chips ───────────────────
       Replace text-only chips with a glyph + colored mini indicators so
       the footer reads like a flight panel instead of a console log. */

    /* GUARD — 5 LED dots beside the shield glyph, one per audit layer. */
    .sb-guard { display: inline-flex; align-items: center; gap: 6px; cursor: help; }
    .sb-guard-glyph {
        color: var(--acc, #10b981);
        font-size: 12px;
        line-height: 1;
        filter: drop-shadow(0 0 6px color-mix(in srgb, var(--acc, #10b981) 40%, transparent));
    }
    .sb-guard-dots {
        display: inline-flex; align-items: center; gap: 3px;
    }
    .sb-led {
        width: 5px; height: 5px; border-radius: 50%;
        display: inline-block;
        transition: background-color .2s, box-shadow .2s;
    }
    .sb-led-ok   { background: var(--acc, #10b981); box-shadow: 0 0 4px color-mix(in srgb, var(--acc, #10b981) 60%, transparent); }
    .sb-led-warn { background: var(--amber, #f59e0b); box-shadow: 0 0 4px color-mix(in srgb, var(--amber, #f59e0b) 60%, transparent); }
    .sb-led-crit { background: var(--red, #ef4444); box-shadow: 0 0 6px color-mix(in srgb, var(--red, #ef4444) 80%, transparent); animation: sbLedCrit 1.4s ease-in-out infinite; }
    .sb-led-idle { background: var(--txt3, #64748b); box-shadow: none; opacity: .4; }
    @keyframes sbLedCrit {
        0%,100% { box-shadow: 0 0 4px color-mix(in srgb, var(--red, #ef4444) 60%, transparent); }
        50%     { box-shadow: 0 0 10px color-mix(in srgb, var(--red, #ef4444) 100%, transparent); }
    }

    /* LLM — three per-tier mini rings (FAST / CHEAP / REASONING). The
       rings are open circles so they read as separate from the GUARD
       solid LEDs (visual differentiation reduces cognitive load). */
    .sb-llm-rings { display: inline-flex; align-items: center; gap: 6px; }
    .sb-rings {
        display: inline-flex; align-items: center; gap: 4px;
    }
    .sb-ring {
        width: 8px; height: 8px;
        border-radius: 50%;
        border: 1.5px solid var(--txt3, #64748b);
        background: transparent;
        display: inline-block;
        transition: border-color .2s, box-shadow .2s;
    }
    .sb-ring-ok   { border-color: var(--acc, #10b981); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--acc, #10b981) 30%, transparent); }
    .sb-ring-warn { border-color: var(--amber, #f59e0b); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--amber, #f59e0b) 30%, transparent); }
    .sb-ring-crit { border-color: var(--red, #ef4444); box-shadow: 0 0 6px color-mix(in srgb, var(--red, #ef4444) 60%, transparent); animation: sbRingCrit 1.4s ease-in-out infinite; }
    .sb-ring-idle { border-color: var(--txt3, #64748b); opacity: .4; }
    @keyframes sbRingCrit {
        0%,100% { box-shadow: 0 0 4px color-mix(in srgb, var(--red, #ef4444) 40%, transparent); }
        50%     { box-shadow: 0 0 10px color-mix(in srgb, var(--red, #ef4444) 80%, transparent); }
    }

    @media (prefers-reduced-motion: reduce) {
        .sb-led-crit, .sb-ring-crit { animation: none !important; }
    }

    /* v1.7.27 — Stream-tps chip layout with inline sparkline. */
    .sb-stream { display: inline-flex; align-items: center; gap: 5px; }
    .sb-stream-spark { display: inline-flex; align-items: center; }
    .sb-stream-unit  { font-family: var(--mono, ui-monospace, monospace); font-size: 10.5px; opacity: .7; }

    /* v1.7.31 — Cost chip 7-day sparkline. */
    .sb-cost-spark { display: inline-flex; align-items: center; margin-left: 4px; }

    /* v1.7.31 — ML chip aligned with GUARD/LLM family. Glyph + 1 LED + label. */
    .sb-ml { display: inline-flex; align-items: center; gap: 5px; cursor: help; }
    .sb-ml-glyph {
        font-size: 11px; line-height: 1;
        filter: drop-shadow(0 0 5px color-mix(in srgb, var(--accent, #10b981) 35%, transparent));
    }
    .sb-ml-label {
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 10.5px;
        letter-spacing: .3px;
        opacity: .85;
    }

    /* v1.7.75 — Mission Strip chips folded into the StatusBar. Same
       monospace + opacity vocabulary as the rest of the bar so they
       read as one family, not as an injected band. */
    .sb-host { display: inline-flex; align-items: center; gap: 5px; }
    .sb-host-dot {
        display: inline-block; width: 6px; height: 6px; border-radius: 50%;
        background: var(--acc, #10b981);
        box-shadow: 0 0 5px color-mix(in srgb, var(--acc, #10b981) 60%, transparent);
        animation: sbHostBeat 3.6s ease-in-out infinite;
    }
    @keyframes sbHostBeat { 0%,100% { opacity: .55; } 50% { opacity: 1; } }

    .sb-ms-chip {
        appearance: none;
        background: transparent;
        border: none;
        padding: 0 6px;
        margin: 0;
        display: inline-flex;
        align-items: center;
        gap: 5px;
        cursor: pointer;
        font-family: inherit;
        font-size: inherit;
        color: var(--txt3, #94a3b8);
        max-width: 220px;
        white-space: nowrap;
        overflow: hidden;
        transition: color .12s;
    }
    .sb-ms-chip:hover { color: var(--txt1, #f1f5f9); }
    .sb-ms-glyph    { opacity: .8; }
    .sb-ms-unit     { opacity: .55; font-size: .9em; }
    .sb-ms-clock    { display: inline-flex; align-items: center; gap: 5px; opacity: .7; }
    /* Skill guard label can be long — let the global text-overflow
       handle it. Glyph stays full opacity for scan-ability. */
    .sb-ms-guard > span:last-child { overflow: hidden; text-overflow: ellipsis; }

    /* Posture: five dots that light up cumulatively (0 = none, 4 = all). */
    .sb-ms-posture { gap: 3px; padding: 0 8px; }
    .sb-posture-dot {
        display: inline-block; width: 5px; height: 5px; border-radius: 50%;
        background: rgba(255, 255, 255, .12);
    }
    .sb-posture-dot.on.cok  { background: var(--acc,   #10b981); box-shadow: 0 0 4px color-mix(in srgb, var(--acc,   #10b981) 55%, transparent); }
    .sb-posture-dot.on.cy   { background: var(--amber, #f59e0b); box-shadow: 0 0 4px color-mix(in srgb, var(--amber, #f59e0b) 55%, transparent); }
    .sb-posture-dot.on.cr   { background: var(--red,   #ef4444); box-shadow: 0 0 6px color-mix(in srgb, var(--red,   #ef4444) 70%, transparent); }
    /* Violet tone for the guard skill label — distinct from "alert" amber.
       Aligned with the .fa-chip / Operations Console palette. */
    :global(.cv) { color: #a78bfa; }
</style>
