<script>
    import { createEventDispatcher, onMount } from 'svelte';
    import { getCapacityTrends, formatDaysUntil } from '$lib/capacity';
    import { staggerIn } from '$lib/stagger';
    import TrendingUp from '@tabler/icons-svelte/icons/trending-up';
    import RefreshCw from '@tabler/icons-svelte/icons/refresh';
    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
    import Clock from '@tabler/icons-svelte/icons/clock';

    export let isEN = false;

    const dispatch = createEventDispatcher();
    function toast(msg, type='info') { dispatch('toast', { msg, type }); }

    let loading = false;
    let trend = null;
    let error = '';
    let selectedRange = 7; // days

    const ranges = [
        { value: 7,   label: '7D' },
        { value: 30,  label: '30D' },
        { value: 90,  label: '90D' },
    ];

    onMount(() => { loadTrends(); });

    async function loadTrends() {
        loading = true;
        error = '';
        try {
            trend = await getCapacityTrends(selectedRange);
        } catch (e) {
            error = String(e);
        }
        loading = false;
    }

    function selectRange(days) {
        selectedRange = days;
        loadTrends();
    }

    // Simple inline sparkline SVG from samples
    function sparklinePath(samples, accessor, width = 300, height = 40) {
        if (!samples || samples.length < 2) return '';
        const vals = samples.map(accessor);
        const min = Math.min(...vals);
        const max = Math.max(...vals, min + 1);
        const xStep = width / (vals.length - 1);
        return vals.map((v, i) => {
            const x = i * xStep;
            const y = height - ((v - min) / (max - min)) * (height - 4) - 2;
            return `${i === 0 ? 'M' : 'L'} ${x.toFixed(1)} ${y.toFixed(1)}`;
        }).join(' ');
    }

    $: diskDays = trend?.disk_days_until_full;
    $: ramDays = trend?.ram_days_until_critical;
    $: hasCriticalProjection = (diskDays !== null && diskDays !== undefined && diskDays < 14) ||
                               (ramDays !== null && ramDays !== undefined && ramDays < 14);
</script>

<div class="view-wrap">
    <div class="view-hdr">
        <div class="view-title"><TrendingUp size={13} strokeWidth={2}/> {isEN ? 'Capacity Planning' : 'Planificación de Capacidad'}</div>
        <div style="display:flex;align-items:center;gap:6px;margin-left:auto;">
            {#each ranges as r}
                <button class="cp-range-btn" class:active={selectedRange === r.value}
                    on:click={() => selectRange(r.value)}>{r.label}</button>
            {/each}
            <button class="view-btn" on:click={loadTrends} disabled={loading} style="display:flex;align-items:center;gap:4px;">
                <RefreshCw size={12} strokeWidth={2}/> {loading ? '...' : 'Refresh'}
            </button>
        </div>
    </div>

    {#if error}
        <div class="cp-error">{error}</div>
    {/if}

    {#if !trend && !loading && !error}
        <div class="cp-empty">
            <Clock size={32} strokeWidth={1.2}/>
            <p>{isEN ? 'No metrics data yet. Samples are recorded automatically every 5 minutes.' : 'Sin datos aún. Las muestras se graban automáticamente cada 5 minutos.'}</p>
        </div>
    {/if}

    {#if trend}
    <div class="cp-body">
        <!-- ── Projection Cards ── -->
        <div class="cp-projections" in:staggerIn={{ index: 0, step: 60, duration: 200 }}>
            <div class="cp-proj-card" class:critical={diskDays !== null && diskDays < 14} class:warning={diskDays !== null && diskDays >= 14 && diskDays < 30}>
                {#if hasCriticalProjection && diskDays !== null && diskDays < 14}
                    <AlertTriangle size={14} strokeWidth={2}/>
                {/if}
                <span class="cp-proj-label">{isEN ? 'DISK FULL IN' : 'DISCO LLENO EN'}</span>
                <span class="cp-proj-value">{formatDaysUntil(diskDays)}</span>
            </div>
            <div class="cp-proj-card" class:critical={ramDays !== null && ramDays < 14} class:warning={ramDays !== null && ramDays >= 14 && ramDays < 30}>
                {#if ramDays !== null && ramDays < 14}
                    <AlertTriangle size={14} strokeWidth={2}/>
                {/if}
                <span class="cp-proj-label">{isEN ? 'RAM CRITICAL IN' : 'RAM CRITICA EN'}</span>
                <span class="cp-proj-value">{formatDaysUntil(ramDays)}</span>
            </div>
            <div class="cp-proj-card">
                <span class="cp-proj-label">{isEN ? 'SAMPLES' : 'MUESTRAS'}</span>
                <span class="cp-proj-value">{trend.stats.sample_count}</span>
                <span class="cp-proj-sub">{trend.stats.span_hours.toFixed(0)}h span</span>
            </div>
        </div>

        <!-- ── Current Values ── -->
        {#if trend.current}
        <div class="cp-current" in:staggerIn={{ index: 1, step: 60, duration: 200 }}>
            <div class="cp-metric">
                <span class="cp-met-label">CPU</span>
                <span class="cp-met-val" style="color:{trend.current.cpu > 90 ? 'var(--red)' : trend.current.cpu > 70 ? 'var(--amber)' : 'var(--acc)'}">
                    {trend.current.cpu.toFixed(1)}%
                </span>
                <span class="cp-met-avg">avg {trend.stats.avg_cpu.toFixed(1)}% · max {trend.stats.max_cpu.toFixed(1)}%</span>
            </div>
            <div class="cp-metric">
                <span class="cp-met-label">RAM</span>
                <span class="cp-met-val" style="color:{trend.current.ram > 90 ? 'var(--red)' : trend.current.ram > 70 ? 'var(--amber)' : 'var(--acc)'}">
                    {trend.current.ram.toFixed(1)}%
                </span>
                <span class="cp-met-avg">avg {trend.stats.avg_ram.toFixed(1)}% · max {trend.stats.max_ram.toFixed(1)}%</span>
            </div>
            <div class="cp-metric">
                <span class="cp-met-label">DISK</span>
                <span class="cp-met-val" style="color:{trend.current.disk > 90 ? 'var(--red)' : trend.current.disk > 70 ? 'var(--amber)' : 'var(--acc)'}">
                    {trend.current.disk.toFixed(1)}%
                </span>
                <span class="cp-met-avg">avg {trend.stats.avg_disk.toFixed(1)}% · max {trend.stats.max_disk.toFixed(1)}%</span>
            </div>
        </div>
        {/if}

        <!-- ── Sparkline Charts ── -->
        {#if trend.samples.length >= 2}
        <div class="cp-charts" in:staggerIn={{ index: 2, step: 60, duration: 200 }}>
            {#each [
                { label: 'CPU %', accessor: s => s.cpu, color: '#10b981' },
                { label: 'RAM %', accessor: s => s.ram, color: '#3b9eff' },
                { label: 'DISK %', accessor: s => s.disk, color: '#a78bfa' },
            ] as chart}
            <div class="cp-chart-row">
                <span class="cp-chart-label">{chart.label}</span>
                <svg class="cp-spark" viewBox="0 0 300 40" preserveAspectRatio="none">
                    <path d={sparklinePath(trend.samples, chart.accessor)}
                          fill="none" stroke={chart.color} stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
                </svg>
            </div>
            {/each}
        </div>
        {/if}

        <!-- ── Sample Table (last 20) ── -->
        <div class="cp-table-wrap" in:staggerIn={{ index: 3, step: 60, duration: 200 }}>
            <div class="cp-table-title">{isEN ? 'Recent Samples' : 'Muestras Recientes'}</div>
            <div class="cp-table-scroll">
                <table class="cp-table">
                    <thead><tr>
                        <th>{isEN ? 'Time' : 'Hora'}</th><th>CPU</th><th>RAM</th><th>Disk</th>
                        <th>{isEN ? 'RAM (MB)' : 'RAM (MB)'}</th><th>{isEN ? 'Disk (GB)' : 'Disco (GB)'}</th>
                    </tr></thead>
                    <tbody>
                    {#each trend.samples.slice(-20).reverse() as s}
                        <tr>
                            <td class="cp-td-ts">{new Date(s.ts * 1000).toLocaleString()}</td>
                            <td style="color:{s.cpu > 90 ? 'var(--red)' : s.cpu > 70 ? 'var(--amber)' : 'var(--txt)'}">{s.cpu.toFixed(1)}%</td>
                            <td style="color:{s.ram > 90 ? 'var(--red)' : s.ram > 70 ? 'var(--amber)' : 'var(--txt)'}">{s.ram.toFixed(1)}%</td>
                            <td style="color:{s.disk > 90 ? 'var(--red)' : s.disk > 70 ? 'var(--amber)' : 'var(--txt)'}">{s.disk.toFixed(1)}%</td>
                            <td>{s.ram_mb}</td>
                            <td>{s.disk_gb}</td>
                        </tr>
                    {/each}
                    </tbody>
                </table>
            </div>
        </div>
    </div>
    {/if}
</div>

<style>
    .view-wrap{flex:1;display:flex;flex-direction:column;overflow:hidden;min-height:0;}
    .view-hdr{display:flex;align-items:center;padding:10px 16px;background:rgba(2,4,8,.6);border-bottom:1px solid var(--bdr);flex-shrink:0;gap:10px;flex-wrap:wrap;}
    .view-title{font-size:13px;font-weight:700;color:var(--txt);white-space:nowrap;display:flex;align-items:center;gap:6px;}
    .view-btn{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt2);font-size:12px;padding:4px 10px;cursor:pointer;transition:.15s;white-space:nowrap;}
    .view-btn:hover{background:var(--bdr2);color:var(--txt);}

    .cp-range-btn{background:transparent;border:1px solid rgba(255,255,255,.06);color:var(--txt2);font-size:11px;font-family:var(--mono);font-weight:600;padding:3px 10px;border-radius:9px;cursor:pointer;transition:.12s;}
    .cp-range-btn:hover{background:rgba(255,255,255,.04);color:var(--txt);}
    .cp-range-btn.active{background:rgba(16,185,129,.10);border-color:rgba(16,185,129,.35);color:var(--acc);}

    .cp-error{padding:12px 16px;color:var(--red);font-size:12px;background:rgba(239,68,68,.06);border-bottom:1px solid var(--bdr);}
    .cp-empty{display:flex;flex-direction:column;align-items:center;justify-content:center;padding:60px 20px;color:#475569;gap:12px;text-align:center;font-size:13px;}

    .cp-body{flex:1;overflow-y:auto;padding:16px;}

    .cp-projections{display:flex;gap:12px;margin-bottom:16px;flex-wrap:wrap;}
    .cp-proj-card{flex:1;min-width:160px;background:rgba(0,0,0,.2);border:1px solid var(--bdr);border-radius:8px;padding:12px 14px;display:flex;flex-direction:column;gap:4px;}
    .cp-proj-card.critical{border-color:rgba(239,68,68,.4);background:rgba(239,68,68,.06);}
    .cp-proj-card.warning{border-color:rgba(245,158,11,.3);background:rgba(245,158,11,.04);}
    .cp-proj-label{font-size:9.5px;font-weight:700;letter-spacing:1px;color:#475569;text-transform:uppercase;}
    .cp-proj-value{font-size:18px;font-weight:800;color:var(--txt);font-family:var(--mono);}
    .cp-proj-sub{font-size:10px;color:#475569;font-family:var(--mono);}
    .cp-proj-card.critical .cp-proj-value{color:var(--red);}
    .cp-proj-card.warning .cp-proj-value{color:var(--amber);}

    .cp-current{display:flex;gap:16px;margin-bottom:16px;flex-wrap:wrap;}
    .cp-metric{flex:1;min-width:120px;background:rgba(0,0,0,.15);border:1px solid var(--bdr);border-radius:8px;padding:10px 14px;}
    .cp-met-label{font-size:10px;font-weight:700;letter-spacing:.8px;color:#475569;text-transform:uppercase;display:block;margin-bottom:4px;}
    .cp-met-val{font-size:22px;font-weight:800;font-family:var(--mono);display:block;}
    .cp-met-avg{font-size:10px;color:#475569;font-family:var(--mono);display:block;margin-top:2px;}

    .cp-charts{display:flex;flex-direction:column;gap:8px;margin-bottom:16px;}
    .cp-chart-row{display:flex;align-items:center;gap:10px;background:rgba(0,0,0,.12);border:1px solid var(--bdr);border-radius:6px;padding:6px 12px;}
    .cp-chart-label{font-size:10px;font-weight:700;letter-spacing:.5px;color:#475569;text-transform:uppercase;width:50px;flex-shrink:0;}
    .cp-spark{flex:1;height:40px;}

    .cp-table-wrap{background:rgba(0,0,0,.12);border:1px solid var(--bdr);border-radius:8px;overflow:hidden;}
    .cp-table-title{font-size:11px;font-weight:700;letter-spacing:.5px;color:#475569;text-transform:uppercase;padding:10px 14px;border-bottom:1px solid var(--bdr);}
    .cp-table-scroll{max-height:300px;overflow-y:auto;}
    .cp-table{width:100%;border-collapse:collapse;font-size:11px;font-family:var(--mono);}
    .cp-table th{text-align:left;padding:6px 10px;font-size:10px;font-weight:700;color:#475569;text-transform:uppercase;letter-spacing:.5px;border-bottom:1px solid var(--bdr);position:sticky;top:0;background:rgba(2,4,8,.8);}
    .cp-table td{padding:5px 10px;border-bottom:1px solid rgba(26,32,48,.2);color:var(--txt2);}
    .cp-td-ts{font-size:10px;color:#475569;}
</style>
