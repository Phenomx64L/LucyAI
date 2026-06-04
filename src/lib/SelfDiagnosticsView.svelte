<script>
    import { createEventDispatcher, onMount } from 'svelte';
    import { runSelfDiagnostics, statusIcon, categoryLabel } from '$lib/diagnostics';
    import { staggerIn } from '$lib/stagger';
    import { invoke } from '@tauri-apps/api/core';
    import { toast } from 'svelte-sonner';
    import Stethoscope from '@tabler/icons-svelte/icons/stethoscope';
    import RefreshCw from '@tabler/icons-svelte/icons/refresh';
    import CircleCheck from '@tabler/icons-svelte/icons/circle-check';
    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
    import CircleX from '@tabler/icons-svelte/icons/circle-x';
    import Wrench from '@tabler/icons-svelte/icons/tool';

    export let isEN = false;

    const dispatch = createEventDispatcher();

    let loading = false;
    let report = null;
    let error = '';

    // v1.7.64 — Tracks which check is currently being repaired so we can
    // disable the corresponding button + show a spinner. Keyed on the check
    // name so multiple repair buttons can be queued/inspected independently.
    let repairing = new Set();

    /**
     * Inspect a check and return a repair descriptor if Lucy knows how to
     * fix this specific failure mode. Returns null when no repair is wired,
     * which keeps the button hidden.
     *
     * Adding a new repair: add another entry here, register the matching
     * Tauri command in the backend, and wire it in `runRepair()` below.
     */
    function detectRepair(check) {
        if (!check) return null;
        const msg = (check.message || '').toLowerCase();

        // ── Database checks ──────────────────────────────────────────────
        // agent_memories.confidence NULL — v1.7.64
        if (check.name === 'Database' && check.status === 'error'
            && msg.includes('null value') && msg.includes('confidence')) {
            return {
                command: 'repair_agent_memories_confidence',
                label:   isEN ? 'Repair NULL confidence' : 'Reparar confidence NULL',
                successDefault: isEN ? 'Repair complete' : 'Reparación completada',
            };
        }
        // v1.7.70 — DB size > 500 MB → VACUUM. The check raises a warning
        // (not error) when size_mb > 500. Pattern-match the message
        // because the check doesn't expose a structured "reason" field.
        if (check.name === 'Database' && check.status === 'warning'
            && msg.includes('integrity: ok')
            && /size:\s*[\d.]+\s*mb/.test(msg)) {
            return {
                command: 'repair_database_vacuum',
                label:   isEN ? 'VACUUM database' : 'VACUUM de la BD',
                successDefault: isEN ? 'VACUUM complete' : 'VACUUM completado',
            };
        }

        // ── Memory pipeline: expired pending cleanup ─────────────────────
        // v1.7.70 — Triggered when expired_pending_cleanup > 100 (warning).
        if (check.name === 'Memory Pipeline' && check.status === 'warning'
            && msg.includes('expired')) {
            return {
                command: 'repair_memory_purge_expired',
                label:   isEN ? 'Purge expired' : 'Purgar caducados',
                successDefault: isEN ? 'Purged' : 'Purgado',
            };
        }

        // ── Stream sessions: leaked entries ──────────────────────────────
        // v1.7.70 — Triggered when STREAM_SESSIONS.len() > 20.
        if (check.name === 'Stream Sessions' && check.status === 'warning') {
            return {
                command: 'repair_clear_leaked_stream_sessions',
                label:   isEN ? 'Clear leaked sessions' : 'Limpiar sesiones colgadas',
                successDefault: isEN ? 'Cleared' : 'Limpiado',
            };
        }

        // ── App log: oversized ───────────────────────────────────────────
        // v1.7.70 — Triggered when log file > 100 MB OR file missing.
        if (check.name === 'App Log'
            && (check.status === 'warning' || check.status === 'error')) {
            return {
                command: 'repair_rotate_app_log',
                label:   isEN ? 'Rotate log' : 'Rotar log',
                successDefault: isEN ? 'Log rotated' : 'Log rotado',
            };
        }

        return null;
    }

    async function runRepair(check, repair) {
        repairing = new Set([...repairing, check.name]);
        try {
            const result = await invoke(repair.command);
            toast.success(result?.message || repair.successDefault);
            // Re-run the diagnostic so the green/red state updates immediately.
            await runDiag();
        } catch (e) {
            toast.error((isEN ? 'Repair failed: ' : 'Reparación falló: ') + String(e));
        } finally {
            repairing = new Set([...repairing].filter(n => n !== check.name));
        }
    }

    onMount(() => { runDiag(); });

    async function runDiag() {
        loading = true;
        error = '';
        try {
            report = await runSelfDiagnostics();
        } catch (e) {
            error = String(e);
        }
        loading = false;
    }

    function statusColorCSS(status) {
        switch(status) {
            case 'ok': return 'var(--acc)';
            case 'warning': return 'var(--amber)';
            case 'error': return 'var(--red)';
            default: return '#475569';
        }
    }

    function groupByCategory(checks) {
        const groups = {};
        for (const c of checks) {
            if (!groups[c.category]) groups[c.category] = [];
            groups[c.category].push(c);
        }
        return Object.entries(groups);
    }

    $: grouped = report ? groupByCategory(report.checks) : [];
</script>

<div class="view-wrap">
    <div class="view-hdr">
        <div class="view-title"><Stethoscope size={13} strokeWidth={2}/> {isEN ? 'Self-Diagnostics' : 'Auto-Diagnóstico'}</div>
        <div style="display:flex;align-items:center;gap:8px;margin-left:auto;">
            {#if report}
                <span class="sd-overall" style="color:{statusColorCSS(report.overall_status)}">
                    {statusIcon(report.overall_status)} {report.overall_status.toUpperCase()}
                </span>
                <span class="sd-elapsed">{report.total_elapsed_ms}ms</span>
            {/if}
            <button class="view-btn" on:click={runDiag} disabled={loading} style="display:flex;align-items:center;gap:4px;">
                <RefreshCw size={12} strokeWidth={2}/> {loading ? (isEN ? 'Running...' : 'Ejecutando...') : (isEN ? 'Re-run' : 'Re-ejecutar')}
            </button>
        </div>
    </div>

    {#if error}
        <div class="sd-error">{error}</div>
    {/if}

    {#if loading && !report}
        <div class="sd-loading">
            <div class="sd-spinner"></div>
            <p>{isEN ? 'Running diagnostics...' : 'Ejecutando diagnósticos...'}</p>
        </div>
    {/if}

    {#if report}
    <div class="sd-body">
        <!-- Overall status banner -->
        <div class="sd-banner" class:ok={report.overall_status === 'ok'} class:warning={report.overall_status === 'warning'} class:err={report.overall_status === 'error'}
             in:staggerIn={{ index: 0, step: 40, duration: 200 }}>
            {#if report.overall_status === 'ok'}
                <CircleCheck size={20} strokeWidth={2}/>
                <span>{isEN ? 'All systems healthy' : 'Todos los sistemas saludables'}</span>
            {:else if report.overall_status === 'warning'}
                <AlertTriangle size={20} strokeWidth={2}/>
                <span>{isEN ? 'Some checks have warnings' : 'Algunos chequeos tienen advertencias'}</span>
            {:else}
                <CircleX size={20} strokeWidth={2}/>
                <span>{isEN ? 'Issues detected — review below' : 'Se detectaron problemas — revisar abajo'}</span>
            {/if}
            <span class="sd-banner-time">{new Date(report.timestamp * 1000).toLocaleString()}</span>
        </div>

        <!-- Grouped checks -->
        {#each grouped as [category, checks], gi}
        <div class="sd-group" in:staggerIn={{ index: gi + 1, step: 60, duration: 220 }}>
            <div class="sd-group-hdr">{categoryLabel(category)}</div>
            {#each checks as check}
            {@const _repair = detectRepair(check)}
            <div class="sd-check" class:ok={check.status === 'ok'} class:warn={check.status === 'warning'} class:err={check.status === 'error'}>
                <div class="sd-check-top">
                    <span class="sd-check-icon" style="color:{statusColorCSS(check.status)}">{statusIcon(check.status)}</span>
                    <span class="sd-check-name">{check.name}</span>
                    <span class="sd-check-ms">{check.elapsed_ms}ms</span>
                    <!-- v1.7.64 — "Reparar" button only renders when detectRepair()
                         matched the check. Hidden for unrecognised failures so
                         the operator isn't promised a fix Lucy can't deliver. -->
                    {#if _repair}
                        <button class="sd-repair-btn"
                                on:click={() => runRepair(check, _repair)}
                                disabled={repairing.has(check.name)}
                                title={_repair.label}>
                            <Wrench size={11} strokeWidth={2}/>
                            <span>{repairing.has(check.name) ? (isEN ? 'Repairing…' : 'Reparando…') : _repair.label}</span>
                        </button>
                    {/if}
                </div>
                <div class="sd-check-msg">{check.message}</div>
                {#if check.details}
                <details class="sd-check-details">
                    <summary>{isEN ? 'Details' : 'Detalles'}</summary>
                    <pre>{JSON.stringify(check.details, null, 2)}</pre>
                </details>
                {/if}
            </div>
            {/each}
        </div>
        {/each}
    </div>
    {/if}
</div>

<style>
    .view-wrap{flex:1;display:flex;flex-direction:column;overflow:hidden;min-height:0;}
    .view-hdr{display:flex;align-items:center;padding:10px 16px;background:rgba(2,4,8,.6);border-bottom:1px solid var(--bdr);flex-shrink:0;gap:10px;flex-wrap:wrap;}
    .view-title{font-size:13px;font-weight:700;color:var(--txt);white-space:nowrap;display:flex;align-items:center;gap:6px;}
    .view-btn{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt2);font-size:12px;padding:4px 10px;cursor:pointer;transition:.15s;white-space:nowrap;}
    .view-btn:hover{background:var(--bdr2);color:var(--txt);}

    .sd-overall{font-size:12px;font-weight:700;font-family:var(--mono);letter-spacing:.5px;}
    .sd-elapsed{font-size:10px;color:#475569;font-family:var(--mono);}

    .sd-error{padding:12px 16px;color:var(--red);font-size:12px;background:rgba(239,68,68,.06);border-bottom:1px solid var(--bdr);}
    .sd-loading{display:flex;flex-direction:column;align-items:center;justify-content:center;padding:60px;color:#475569;gap:12px;}
    .sd-spinner{width:24px;height:24px;border:2px solid rgba(255,255,255,.08);border-top-color:var(--acc);border-radius:50%;animation:spin 0.7s linear infinite;}
    @keyframes spin{to{transform:rotate(360deg);}}

    .sd-body{flex:1;overflow-y:auto;padding:16px;}

    .sd-banner{display:flex;align-items:center;gap:10px;padding:12px 16px;border-radius:8px;margin-bottom:16px;font-size:13px;font-weight:600;}
    .sd-banner.ok{background:rgba(16,185,129,.08);border:1px solid rgba(16,185,129,.25);color:var(--acc);}
    .sd-banner.warning{background:rgba(245,158,11,.08);border:1px solid rgba(245,158,11,.25);color:var(--amber);}
    .sd-banner.err{background:rgba(239,68,68,.08);border:1px solid rgba(239,68,68,.25);color:var(--red);}
    .sd-banner-time{margin-left:auto;font-size:10px;font-weight:400;font-family:var(--mono);opacity:.7;}

    .sd-group{margin-bottom:16px;}
    .sd-group-hdr{font-size:10px;font-weight:700;letter-spacing:1.2px;color:#475569;text-transform:uppercase;padding:0 0 6px;border-bottom:1px solid rgba(26,32,48,.3);margin-bottom:6px;}

    .sd-check{background:rgba(0,0,0,.15);border:1px solid var(--bdr);border-radius:6px;padding:8px 12px;margin-bottom:6px;transition:.15s;}
    .sd-check.err{border-left:2px solid var(--red);background:rgba(239,68,68,.03);}
    .sd-check.warn{border-left:2px solid var(--amber);background:rgba(245,158,11,.02);}
    .sd-check.ok{border-left:2px solid var(--acc);}

    .sd-check-top{display:flex;align-items:center;gap:8px;margin-bottom:3px;}
    .sd-check-icon{font-size:13px;font-weight:700;}
    .sd-check-name{font-size:12px;font-weight:600;color:var(--txt);}
    .sd-check-ms{margin-left:auto;font-size:10px;color:#475569;font-family:var(--mono);}

    /* v1.7.64 — Repair button. Compact pill that sits to the right of the
       elapsed-time chip on the same row as the check name. Hidden by
       default; only renders when `detectRepair(check)` matched a known
       fix. Disabled state shows "Reparando…" while the Tauri command
       runs. Colour is ambient amber so the operator's eye picks it up
       without competing with the red error icon. */
    .sd-repair-btn{
        display:inline-flex;
        align-items:center;
        gap:4px;
        font-size:10px;
        font-weight:700;
        font-family:var(--mono);
        letter-spacing:.3px;
        color:var(--amber, #f59e0b);
        background:rgba(245,158,11,.10);
        border:1px solid rgba(245,158,11,.30);
        border-radius:3px;
        padding:2px 7px;
        margin-left:6px;
        cursor:pointer;
        white-space:nowrap;
        transition:.15s;
    }
    .sd-repair-btn:hover{
        background:rgba(245,158,11,.18);
        border-color:rgba(245,158,11,.50);
        color:#fbbf24;
    }
    .sd-repair-btn:disabled{
        opacity:.6;
        cursor:wait;
    }
    .sd-check-msg{font-size:11px;color:var(--txt2);font-family:var(--mono);line-height:1.4;}

    .sd-check-details{margin-top:6px;}
    .sd-check-details summary{font-size:10px;color:#4a5a6a;cursor:pointer;}
    .sd-check-details pre{font-size:10px;color:#6a7a8a;font-family:var(--mono);background:rgba(0,0,0,.2);padding:6px 8px;border-radius:4px;margin-top:4px;overflow-x:auto;white-space:pre-wrap;word-break:break-all;max-height:200px;overflow-y:auto;}
</style>
