<script>
    import { onMount, createEventDispatcher } from 'svelte';
    import { getCostSummary } from '$lib/lucy-api';
    import { costSummaryDay, costSummaryMonth, costSummaryAll, tokenBudgetConfig } from '$lib/stores';

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
    <!-- Header -->
    <div class="header">
        <h2>{lang.title}</h2>
        <div class="controls">
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

    <!-- Budget Alert -->
    {#if budgetAlert && $tokenBudgetConfig.enabled}
        <div class="budget-alert">
            ⚠️ {lang.budgetAlert}: {budgetPercentage}% {lang.spent}
        </div>
    {/if}

    <!-- Main metrics -->
    {#if currentSummary}
        <div class="metrics-grid">
            <div class="metric-card">
                <div class="metric-label">{lang.totalCost}</div>
                <div class="metric-value">${currentSummary.total_cost.toFixed(2)}</div>
            </div>
            <div class="metric-card">
                <div class="metric-label">{lang.totalTokens}</div>
                <div class="metric-value">{currentSummary.total_tokens.toLocaleString()}</div>
            </div>
            <div class="metric-card">
                <div class="metric-label">{lang.requests}</div>
                <div class="metric-value">{currentSummary.request_count}</div>
            </div>
            {#if selectedPeriod === 'month' && $tokenBudgetConfig.enabled}
                <div class="metric-card budget">
                    <div class="metric-label">{lang.budget}</div>
                    <div class="budget-bar">
                        <div
                            class="budget-fill"
                            style="width: {Math.min(budgetPercentage, 100)}%"
                        ></div>
                    </div>
                    <div class="budget-text">
                        ${currentSummary.total_cost.toFixed(2)} / ${$tokenBudgetConfig.monthlyLimit.toFixed(2)}
                    </div>
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
</style>
