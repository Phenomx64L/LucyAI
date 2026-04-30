<script>
    import { onMount, createEventDispatcher } from 'svelte';
    import { getCostSummary } from '$lib/lucy-api';
    import { IconAlertTriangle as AlertTriangle, IconSettings as Settings, IconCheck as Check } from '@tabler/icons-svelte';
    import { costSummaryDay, costSummaryMonth, costSummaryAll, tokenBudgetConfig } from '$lib/stores';
    import { countUp } from '$lib/actions';
    import { focusTrap } from '$lib/actions';

    const dispatch = createEventDispatcher();

    // Props
    export let userLang = 'es-MX';
    export let isEN = false;

    // Internal state
    let loading = false;
    let error = '';
    let selectedPeriod = 'month';
    let refreshTimer = null;
    let lastUpdate = '';

    // ── Budget editor modal ────────────────────────────────────────────────
    // Lets the user raise / lower / disable the monthly budget. Previously
    // the Cost Dashboard showed "109% spent" with no way to act on it.
    let budgetModalOpen = false;
    let budgetDraft = { monthlyLimit: 10, alertThreshold: 80, enabled: true };
    function openBudgetModal() {
        // Snapshot the current config so cancel doesn't mutate.
        const cur = $tokenBudgetConfig || { monthlyLimit: 10, alertThreshold: 80, enabled: true };
        budgetDraft = {
            monthlyLimit: Number(cur.monthlyLimit) || 10,
            alertThreshold: Number(cur.alertThreshold) || 80,
            enabled: cur.enabled !== false,
        };
        budgetModalOpen = true;
    }
    function closeBudgetModal() { budgetModalOpen = false; }
    function saveBudget() {
        // Sanitize: positive numbers only, threshold 1-100.
        const m = Math.max(0.01, Number(budgetDraft.monthlyLimit) || 10);
        const th = Math.min(100, Math.max(1, Number(budgetDraft.alertThreshold) || 80));
        tokenBudgetConfig.set({
            monthlyLimit: Math.round(m * 100) / 100,
            alertThreshold: Math.round(th),
            enabled: !!budgetDraft.enabled,
        });
        budgetModalOpen = false;
        toast(isEN
            ? `Budget updated: $${m.toFixed(2)}/month, alert at ${th}%`
            : `Presupuesto actualizado: $${m.toFixed(2)}/mes, alerta al ${th}%`,
            'ok');
    }
    // Quick presets — common bumps so the user doesn't always reach for the
    // numeric input. Mirrors what GitHub Copilot / Cursor offer.
    const BUDGET_PRESETS = [10, 25, 50, 100, 250];

    // Computed summaries
    let currentSummary = null;
    let budgetPercentage = 0;
    let budgetAlert = false;

    const labels = {
        'es-MX': {
            title: 'Dashboard de Costos',
            period: 'Período',
            day: 'Día',
            month: 'Mes',
            all: 'Todo',
            totalCost: 'Costo Total',
            totalTokens: 'Tokens Totales',
            requests: 'Solicitudes',
            model: 'Modelo',
            cost: 'Costo',
            tokens: 'Tokens',
            budget: 'Presupuesto',
            spent: 'Gastado',
            remaining: 'Restante',
            perModel: 'Desglose por Modelo',
            noData: 'Sin datos disponibles',
            error: 'Error',
            loading: 'Cargando...',
            refresh: 'Actualizar',
            lastUpdate: 'Última actualización',
            budgetAlert: 'Alerta de presupuesto',
            budgetExceeded: 'Presupuesto excedido',
            editBudget: 'Editar presupuesto',
            budgetTitle: 'Configurar presupuesto mensual',
            monthlyLimit: 'Límite mensual (USD)',
            alertAt: 'Alerta al',
            enableBudget: 'Activar control de presupuesto',
            quickPresets: 'Atajos',
            cancel: 'Cancelar',
            save: 'Guardar',
            disable: 'Desactivar control',
            budgetHint: 'Si lo desactivas, Lucy no mostrará alertas de gasto.',
        },
        'en-US': {
            title: 'Cost Dashboard',
            period: 'Period',
            day: 'Day',
            month: 'Month',
            all: 'All Time',
            totalCost: 'Total Cost',
            totalTokens: 'Total Tokens',
            requests: 'Requests',
            model: 'Model',
            cost: 'Cost',
            tokens: 'Tokens',
            budget: 'Budget',
            spent: 'Spent',
            remaining: 'Remaining',
            perModel: 'Per-Model Breakdown',
            noData: 'No data available',
            error: 'Error',
            loading: 'Loading...',
            refresh: 'Refresh',
            lastUpdate: 'Last updated',
            budgetAlert: 'Budget Alert',
            budgetExceeded: 'Budget exceeded',
            editBudget: 'Edit budget',
            budgetTitle: 'Configure monthly budget',
            monthlyLimit: 'Monthly limit (USD)',
            alertAt: 'Alert at',
            enableBudget: 'Enable budget tracking',
            quickPresets: 'Quick presets',
            cancel: 'Cancel',
            save: 'Save',
            disable: 'Disable tracking',
            budgetHint: 'If disabled, Lucy will stop showing budget alerts.',
        }
    };

    $: lang = labels[isEN ? 'en-US' : 'es-MX'];

    // Track current summary based on period
    $: currentSummary = selectedPeriod === 'day'
        ? $costSummaryDay
        : selectedPeriod === 'month'
        ? $costSummaryMonth
        : $costSummaryAll;

    // Calculate budget percentage
    $: if ($tokenBudgetConfig && currentSummary) {
        budgetPercentage = Math.round((currentSummary.total_cost / $tokenBudgetConfig.monthlyLimit) * 100);
        budgetAlert = budgetPercentage >= $tokenBudgetConfig.alertThreshold && selectedPeriod === 'month';
    }

    function toast(msg, type = 'info') {
        dispatch('toast', { msg, type });
    }

    async function refreshCosts() {
        loading = true;
        error = '';
        try {
            const day = await getCostSummary('day');
            const month = await getCostSummary('month');
            const all = await getCostSummary('all');

            costSummaryDay.set(day);
            costSummaryMonth.set(month);
            costSummaryAll.set(all);

            lastUpdate = new Date().toLocaleTimeString(userLang, {
                hour: '2-digit',
                minute: '2-digit',
                second: '2-digit'
            });
        } catch (e) {
            error = String(e);
            toast(lang.error + ': ' + error, 'error');
        }
        loading = false;
    }

    function startAutoRefresh() {
        refreshCosts();
        refreshTimer = setInterval(refreshCosts, 300000); // 5 minutes
    }

    function stopAutoRefresh() {
        if (refreshTimer) {
            clearInterval(refreshTimer);
            refreshTimer = null;
        }
    }

    function onPeriodChange(event) {
        selectedPeriod = event.target.value;
    }

    onMount(() => {
        startAutoRefresh();
        return () => stopAutoRefresh();
    });
</script>

<div class="cost-dashboard">
    <!-- Header — prominent budget editor button always visible here so the
         user never has to hunt for it (the small ⚙ on the budget card is
         too subtle, per user feedback). -->
    <div class="header">
        <h2>{lang.title}</h2>
        <div class="controls">
            <button class="budget-cta" type="button" on:click={openBudgetModal} title={lang.editBudget}>
                <Settings size={13} strokeWidth={2}/>
                <span>{lang.editBudget}</span>
            </button>
            <select value={selectedPeriod} on:change={onPeriodChange} disabled={loading}>
                <option value="day">{lang.day}</option>
                <option value="month">{lang.month}</option>
                <option value="all">{lang.all}</option>
            </select>
            <button on:click={refreshCosts} disabled={loading}>
                {loading ? lang.loading : lang.refresh}
            </button>
        </div>
    </div>

    <!-- Last update -->
    {#if lastUpdate}
        <div class="last-update">
            {lang.lastUpdate}: {lastUpdate}
        </div>
    {/if}

    <!-- Error -->
    {#if error}
        <div class="error-box">{lang.error}: {error}</div>
    {/if}

    <!-- Budget Alert + inline action button -->
    {#if budgetAlert && $tokenBudgetConfig.enabled}
        <div class="budget-alert" style="display:flex;align-items:center;gap:8px;">
            <AlertTriangle size={13} strokeWidth={2}/>
            <span>{lang.budgetAlert}: {budgetPercentage}% {lang.spent}</span>
            <button class="budget-alert-action" type="button" on:click={openBudgetModal}>
                <Settings size={11} strokeWidth={2}/> {lang.editBudget}
            </button>
        </div>
    {/if}

    <!-- Main metrics -->
    {#if currentSummary}
        <div class="metrics-grid">
            <div class="metric-card">
                <div class="metric-label">{lang.totalCost}</div>
                <div class="metric-value">
                    <span use:countUp={{ target: currentSummary.total_cost, prefix: '$', suffix: '', decimals: 2, duration: 1100 }}></span>
                </div>
            </div>
            <div class="metric-card">
                <div class="metric-label">{lang.totalTokens}</div>
                <div class="metric-value">
                    <span use:countUp={{ target: currentSummary.total_tokens, suffix: '', thousands: true, duration: 1100 }}></span>
                </div>
            </div>
            <div class="metric-card">
                <div class="metric-label">{lang.requests}</div>
                <div class="metric-value">
                    <span use:countUp={{ target: currentSummary.request_count, suffix: '', thousands: true, duration: 900 }}></span>
                </div>
            </div>
            {#if selectedPeriod === 'month' && $tokenBudgetConfig.enabled}
                <!-- Whole card is now clickable: opens the budget editor. -->
                <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
                <div class="metric-card budget budget-clickable"
                     role="button" tabindex="0"
                     title={lang.editBudget}
                     on:click={openBudgetModal}
                     on:keydown={(e) => { if (e.key === 'Enter' || e.key === ' ') openBudgetModal(); }}>
                    <div class="metric-label" style="display:flex;align-items:center;justify-content:space-between;gap:6px;">
                        <span>{lang.budget}</span>
                        <button class="budget-edit-btn" type="button"
                                on:click|stopPropagation={openBudgetModal}
                                title={lang.editBudget}
                                aria-label={lang.editBudget}>
                            <Settings size={13} strokeWidth={2}/>
                            <span class="budget-edit-text">{lang.editBudget}</span>
                        </button>
                    </div>
                    <div class="budget-bar">
                        <div
                            class="budget-fill"
                            class:over={budgetPercentage > 100}
                            style="width: {Math.min(budgetPercentage, 100)}%"
                        ></div>
                    </div>
                    <div class="budget-text">
                        ${currentSummary.total_cost.toFixed(2)} / ${$tokenBudgetConfig.monthlyLimit.toFixed(2)}
                        {#if budgetPercentage > 100}
                            <span class="budget-over-pct">({budgetPercentage}%)</span>
                        {/if}
                    </div>
                </div>
            {:else if selectedPeriod === 'month' && !$tokenBudgetConfig.enabled}
                <div class="metric-card budget budget-disabled">
                    <div class="metric-label">{lang.budget}</div>
                    <div class="budget-text" style="opacity:.7;">
                        {isEN ? 'Tracking disabled' : 'Control desactivado'}
                    </div>
                    <button class="budget-enable-btn" type="button" on:click={openBudgetModal}>
                        <Settings size={11} strokeWidth={2}/> {lang.editBudget}
                    </button>
                </div>
            {/if}
        </div>

        <!-- Per-model breakdown -->
        {#if currentSummary.per_model && currentSummary.per_model.length > 0}
            <div class="per-model-section">
                <h3>{lang.perModel}</h3>
                <table>
                    <thead>
                        <tr>
                            <th>{lang.model}</th>
                            <th>{lang.cost}</th>
                            <th>{lang.tokens}</th>
                            <th>{lang.requests}</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each currentSummary.per_model as model (model.model)}
                            <tr>
                                <td class="model-name">{model.model}</td>
                                <td class="number">${model.cost.toFixed(2)}</td>
                                <td class="number">{model.tokens.toLocaleString()}</td>
                                <td class="number">{model.requests}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {:else if !error}
        <div class="no-data">{lang.noData}</div>
    {/if}
</div>

<!-- ── Budget editor modal ────────────────────────────────────────────────── -->
{#if budgetModalOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="bm-overlay" role="presentation" on:click={closeBudgetModal}
         on:keydown={(e) => { if (e.key === 'Escape') closeBudgetModal(); }}>
        <div class="bm-box" role="dialog" aria-modal="true" tabindex={-1}
             use:focusTrap on:click|stopPropagation>
            <div class="bm-hdr">
                <h3>{lang.budgetTitle}</h3>
                <button class="bm-close" type="button" on:click={closeBudgetModal} aria-label="Close">✕</button>
            </div>

            <div class="bm-body">
                <!-- Monthly limit -->
                <label class="bm-field" for="bm-limit">
                    <span class="bm-lbl">{lang.monthlyLimit}</span>
                    <div class="bm-input-wrap">
                        <span class="bm-currency">$</span>
                        <input id="bm-limit" type="number" min="0.01" step="0.01"
                               bind:value={budgetDraft.monthlyLimit} />
                    </div>
                </label>

                <!-- Quick presets -->
                <div class="bm-presets-row">
                    <span class="bm-presets-lbl">{lang.quickPresets}:</span>
                    {#each BUDGET_PRESETS as p}
                        <button class="bm-preset" type="button"
                                class:active={Number(budgetDraft.monthlyLimit) === p}
                                on:click={() => budgetDraft.monthlyLimit = p}>
                            ${p}
                        </button>
                    {/each}
                </div>

                <!-- Alert threshold -->
                <label class="bm-field" for="bm-thresh">
                    <span class="bm-lbl">{lang.alertAt}</span>
                    <div class="bm-input-wrap">
                        <input id="bm-thresh" type="number" min="1" max="100" step="1"
                               bind:value={budgetDraft.alertThreshold} />
                        <span class="bm-currency">%</span>
                    </div>
                </label>

                <!-- Enable toggle -->
                <label class="bm-toggle">
                    <input type="checkbox" bind:checked={budgetDraft.enabled} />
                    <span>{lang.enableBudget}</span>
                </label>
                {#if !budgetDraft.enabled}
                    <p class="bm-hint">{lang.budgetHint}</p>
                {/if}
            </div>

            <div class="bm-foot">
                <button class="bm-btn bm-cancel" type="button" on:click={closeBudgetModal}>
                    {lang.cancel}
                </button>
                <button class="bm-btn bm-save" type="button" on:click={saveBudget}>
                    <Check size={13} strokeWidth={2.5}/> {lang.save}
                </button>
            </div>
        </div>
    </div>
{/if}

<style>
    .cost-dashboard {
        padding: 1.5rem;
        background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
        border-radius: 8px;
        color: #e0e0e0;
        font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
    }

    .header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        margin-bottom: 1.5rem;
        padding-bottom: 1rem;
        border-bottom: 1px solid #3a3a5c;
    }

    .header h2 {
        margin: 0;
        font-size: 1.5rem;
        color: #fff;
    }

    .controls {
        display: flex;
        gap: 1rem;
        align-items: center;
    }

    select, button {
        padding: 0.5rem 1rem;
        border: 1px solid #3a3a5c;
        border-radius: 4px;
        background: #0f3460;
        color: #e0e0e0;
        cursor: pointer;
        font-size: 0.9rem;
    }

    button:hover:not(:disabled) {
        background: #16213e;
        border-color: #4a4a7c;
    }

    button:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    .last-update {
        font-size: 0.85rem;
        color: #888;
        margin-bottom: 1rem;
    }

    .error-box {
        padding: 1rem;
        margin-bottom: 1rem;
        background: #3e1b1b;
        border: 1px solid #8b3333;
        border-radius: 4px;
        color: #ff9999;
    }

    .budget-alert {
        padding: 1rem;
        margin-bottom: 1rem;
        background: #4a3f1a;
        border: 1px solid #8b7c3a;
        border-radius: 4px;
        color: #ffd966;
    }

    .metrics-grid {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
        gap: 1rem;
        margin-bottom: 2rem;
    }

    .metric-card {
        padding: 1.5rem;
        background: #0f3460;
        border: 1px solid #3a3a5c;
        border-radius: 6px;
        text-align: center;
    }

    .metric-card.budget {
        grid-column: 1 / -1;
    }

    .metric-label {
        font-size: 0.85rem;
        color: #888;
        margin-bottom: 0.5rem;
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }

    .metric-value {
        font-size: 2rem;
        font-weight: bold;
        color: #4a9eff;
    }

    .budget-bar {
        width: 100%;
        height: 24px;
        background: #1a1a2e;
        border-radius: 12px;
        overflow: hidden;
        margin: 1rem 0;
        border: 1px solid #3a3a5c;
    }

    .budget-fill {
        height: 100%;
        background: linear-gradient(90deg, #4a9eff 0%, #ff6b6b 100%);
        transition: width 0.3s ease;
    }

    .budget-text {
        font-size: 0.9rem;
        color: #e0e0e0;
    }

    .no-data {
        text-align: center;
        padding: 3rem 1rem;
        color: #666;
        font-size: 1rem;
    }

    .per-model-section {
        margin-top: 2rem;
    }

    .per-model-section h3 {
        margin: 0 0 1rem 0;
        font-size: 1.1rem;
        color: #fff;
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }

    table {
        width: 100%;
        border-collapse: collapse;
        background: #0f3460;
        border: 1px solid #3a3a5c;
        border-radius: 6px;
        overflow: hidden;
    }

    thead {
        background: #16213e;
    }

    th {
        padding: 1rem;
        text-align: left;
        font-weight: 600;
        color: #888;
        text-transform: uppercase;
        font-size: 0.8rem;
        letter-spacing: 0.5px;
        border-bottom: 1px solid #3a3a5c;
    }

    td {
        padding: 1rem;
        border-bottom: 1px solid #1a1a2e;
    }

    tbody tr:hover {
        background: #1a3a52;
    }

    .model-name {
        color: #4a9eff;
        font-weight: 500;
    }

    .number {
        text-align: right;
        color: #e0e0e0;
        font-family: 'Courier New', monospace;
    }

    /* Responsive */
    @media (max-width: 768px) {
        .header {
            flex-direction: column;
            align-items: flex-start;
            gap: 1rem;
        }

        .controls {
            width: 100%;
            justify-content: space-between;
        }

        .metrics-grid {
            grid-template-columns: 1fr;
        }

        table {
            font-size: 0.85rem;
        }

        th, td {
            padding: 0.75rem;
        }
    }

    /* ── Budget edit affordances ─────────────────────────────────────── */
    /* Big CTA in the dashboard header — primary affordance, always visible. */
    .budget-cta {
        display: inline-flex; align-items: center; gap: 6px;
        background: rgba(16,185,129,0.10);
        border: 1px solid rgba(16,185,129,0.30);
        color: var(--accent, #10b981);
        font-size: 12px; font-weight: 600; font-family: inherit;
        padding: 6px 12px; border-radius: 7px;
        cursor: pointer;
        transition: background 160ms ease, border-color 160ms ease;
    }
    .budget-cta:hover {
        background: rgba(16,185,129,0.18);
        border-color: var(--accent, #10b981);
    }
    /* Inline button on the budget card — now has TEXT next to the icon
       so it's not invisible at small sizes. */
    .budget-edit-btn {
        background: transparent;
        border: 1px solid rgba(255,255,255,0.14);
        color: var(--text-muted, #94a3b8);
        border-radius: 6px;
        padding: 3px 8px;
        cursor: pointer;
        font-family: inherit; font-size: 10px; font-weight: 600;
        text-transform: uppercase; letter-spacing: 0.4px;
        display: inline-flex; align-items: center; gap: 4px;
        transition: background 160ms ease, border-color 160ms ease, color 160ms ease;
    }
    .budget-edit-btn:hover {
        background: rgba(16,185,129,0.10);
        border-color: rgba(16,185,129,0.35);
        color: var(--accent, #10b981);
    }
    .budget-edit-text {
        white-space: nowrap;
    }
    /* The whole budget card lights up as clickable now. */
    .budget-clickable {
        cursor: pointer;
        transition: background 160ms ease, border-color 160ms ease, transform 160ms ease;
    }
    .budget-clickable:hover {
        background: rgba(16,185,129,0.04);
        border-color: rgba(16,185,129,0.25);
    }
    .budget-clickable:active { transform: scale(0.995); }
    .budget-fill.over {
        background: linear-gradient(90deg, #f59e0b 0%, #ef4444 100%);
    }
    .budget-over-pct {
        color: #ef4444; font-weight: 700; margin-left: 4px;
    }
    .budget-disabled .budget-text { font-style: italic; }
    .budget-enable-btn {
        margin-top: 6px;
        background: transparent;
        border: 1px solid rgba(16,185,129,0.30);
        color: var(--accent, #10b981);
        font-size: 11px; font-weight: 600;
        padding: 4px 10px; border-radius: 6px;
        cursor: pointer;
        display: inline-flex; align-items: center; gap: 4px;
        transition: background 160ms ease;
    }
    .budget-enable-btn:hover { background: rgba(16,185,129,0.10); }
    .budget-alert-action {
        margin-left: auto;
        background: transparent;
        border: 1px solid rgba(245,158,11,0.45);
        color: #fbbf24;
        font-size: 11px; font-weight: 600;
        padding: 3px 9px; border-radius: 6px;
        cursor: pointer;
        display: inline-flex; align-items: center; gap: 4px;
        transition: background 160ms ease;
    }
    .budget-alert-action:hover { background: rgba(245,158,11,0.12); }

    /* ── Modal: budget editor ────────────────────────────────────────── */
    .bm-overlay {
        position: fixed; inset: 0;
        background: rgba(2, 6, 12, 0.74);
        backdrop-filter: blur(4px);
        z-index: 9000;
        display: flex; align-items: center; justify-content: center;
        animation: bm-fade-in 200ms ease-out;
    }
    @keyframes bm-fade-in { from { opacity: 0; } to { opacity: 1; } }
    .bm-box {
        background: var(--bg-card, #161b22);
        border: 1px solid var(--border-light, #334155);
        border-radius: 12px;
        width: 380px; max-width: 92vw;
        box-shadow: 0 24px 64px rgba(0,0,0,0.6),
                    0 0 0 1px rgba(16,185,129,0.10);
        outline: none;
        animation: bm-pop-in 220ms cubic-bezier(0.34, 1.56, 0.64, 1);
    }
    @keyframes bm-pop-in {
        from { opacity: 0; transform: scale(0.94) translateY(8px); }
        to   { opacity: 1; transform: scale(1) translateY(0); }
    }
    .bm-hdr {
        display: flex; align-items: center; justify-content: space-between;
        padding: 14px 18px;
        border-bottom: 1px solid var(--border-color, #1e293b);
    }
    .bm-hdr h3 {
        margin: 0; font-size: 14px; font-weight: 600;
        color: var(--text-bright, #f1f5f9);
    }
    .bm-close {
        background: transparent; border: none;
        color: var(--text-muted, #64748b);
        font-size: 18px; cursor: pointer; padding: 0 4px;
        line-height: 1;
    }
    .bm-close:hover { color: var(--text-bright, #f1f5f9); }

    .bm-body {
        padding: 18px;
        display: flex; flex-direction: column; gap: 14px;
    }
    .bm-field {
        display: flex; flex-direction: column; gap: 6px;
    }
    .bm-lbl {
        font-size: 11px; font-weight: 600;
        color: var(--text-muted, #94a3b8);
        text-transform: uppercase; letter-spacing: 0.4px;
    }
    .bm-input-wrap {
        display: flex; align-items: center; gap: 6px;
        background: rgba(0,0,0,0.30);
        border: 1px solid var(--border-color, #334155);
        border-radius: 7px;
        padding: 6px 10px;
        transition: border-color 160ms ease;
    }
    .bm-input-wrap:focus-within {
        border-color: var(--accent, #10b981);
        box-shadow: 0 0 0 3px rgba(16,185,129,0.10);
    }
    .bm-currency {
        color: var(--text-muted, #94a3b8);
        font-family: var(--font-mono, monospace);
        font-size: 13px;
    }
    .bm-input-wrap input {
        flex: 1;
        background: transparent;
        border: none; outline: none;
        color: var(--text-bright, #f1f5f9);
        font-size: 14px; font-family: inherit;
        font-weight: 600;
    }
    .bm-presets-row {
        display: flex; align-items: center; gap: 6px; flex-wrap: wrap;
    }
    .bm-presets-lbl {
        font-size: 10px; font-weight: 600;
        color: var(--text-muted, #94a3b8);
        text-transform: uppercase; letter-spacing: 0.4px;
        margin-right: 2px;
    }
    .bm-preset {
        background: rgba(255,255,255,0.04);
        border: 1px solid var(--border-color, #334155);
        color: var(--text-main, #e2e8f0);
        border-radius: 6px;
        padding: 3px 9px;
        font-size: 11px; font-weight: 600;
        font-family: var(--font-mono, monospace);
        cursor: pointer;
        transition: background 160ms ease, border-color 160ms ease;
    }
    .bm-preset:hover {
        background: rgba(16,185,129,0.10);
        border-color: rgba(16,185,129,0.30);
    }
    .bm-preset.active {
        background: rgba(16,185,129,0.18);
        border-color: var(--accent, #10b981);
        color: var(--accent, #10b981);
    }
    .bm-toggle {
        display: flex; align-items: center; gap: 8px;
        cursor: pointer; user-select: none;
        font-size: 12px;
        color: var(--text-main, #e2e8f0);
    }
    .bm-toggle input { accent-color: var(--accent, #10b981); cursor: pointer; }
    .bm-hint {
        margin: 0; padding: 8px 10px;
        background: rgba(245,158,11,0.06);
        border-left: 2px solid rgba(245,158,11,0.40);
        border-radius: 0 6px 6px 0;
        font-size: 11px; color: #fbbf24; line-height: 1.5;
    }

    .bm-foot {
        display: flex; justify-content: flex-end; gap: 8px;
        padding: 12px 18px;
        border-top: 1px solid var(--border-color, #1e293b);
    }
    .bm-btn {
        border-radius: 7px; padding: 7px 14px;
        font-size: 12px; font-weight: 600; font-family: inherit;
        cursor: pointer;
        display: inline-flex; align-items: center; gap: 5px;
        transition: opacity 150ms ease, background 150ms ease;
    }
    .bm-cancel {
        background: transparent;
        border: 1px solid var(--border-color, #334155);
        color: var(--text-muted, #94a3b8);
    }
    .bm-cancel:hover { color: var(--text-bright, #f1f5f9); border-color: var(--border-light, #475569); }
    .bm-save {
        background: var(--accent, #10b981);
        border: 1px solid var(--accent, #10b981);
        color: #032b1c;
    }
    .bm-save:hover { opacity: 0.92; }
</style>
