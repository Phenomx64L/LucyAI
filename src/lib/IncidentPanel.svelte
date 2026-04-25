<!--
  IncidentPanel — SRE Mode UI (Nivel 4)

  A focused sidebar that sits above or beside the shell when Incident Mode
  is active. Its job is NOT to execute commands — that stays with Lucy's
  normal agent loop — but to surface the structured state the backend
  maintains: current phase, evidence collected, hypotheses on the table,
  validity score.

  The component is intentionally read-mostly. The user drives the flow via
  chat (start incident, answer questions, approve rerouting). The panel
  gives them visibility into what Lucy "believes" right now.
-->
<script>
    import { invoke } from '@tauri-apps/api/core';
    import { createEventDispatcher, onDestroy } from 'svelte';
    import ConfirmModal from '$lib/ConfirmModal.svelte';
    import { IconAlertTriangle as AlertTriangle, IconActivity as Activity, IconSearch as Search, IconBulb as Lightbulb, IconCircleCheck as CheckCircle2, IconCircleX as XCircle, IconFileText as FileText, IconRotate2 as RotateCcw, IconFlag as Flag } from '@tabler/icons-svelte';

    export let incidentId = null;
    export let isEN = false;

    const dispatch = createEventDispatcher();

    let incident = null;
    let evidence = [];
    let hypotheses = [];
    let loading = false;
    let error = null;
    let pollTimer = null;

    // Phase metadata — drives the progress stepper + colors
    const PHASES = [
        { id: 'extract',     labelEn: 'Extract',     labelEs: 'Extraer',      icon: AlertTriangle, color: '#f59e0b' },
        { id: 'plan',        labelEn: 'Plan',        labelEs: 'Planificar',   icon: Search,        color: '#8b5cf6' },
        { id: 'investigate', labelEn: 'Investigate', labelEs: 'Investigar',   icon: Activity,      color: '#3b82f6' },
        { id: 'diagnose',    labelEn: 'Diagnose',    labelEs: 'Diagnosticar', icon: Lightbulb,     color: '#10b981' },
        { id: 'report',      labelEn: 'Report',      labelEs: 'Reportar',     icon: FileText,      color: '#06b6d4' },
        { id: 'done',        labelEn: 'Done',        labelEs: 'Listo',        icon: CheckCircle2,  color: '#22c55e' },
    ];

    $: currentPhaseIdx = incident ? PHASES.findIndex(p => p.id === incident.phase) : -1;
    $: scorePercent = incident ? Math.round(incident.validity_score * 100) : 0;
    // Threshold below which the LLM should reroute rather than finalize.
    // Matches the backend's behavioral guidance in the DIAGNOSE prompt.
    $: scoreColor = scorePercent >= 60 ? '#10b981' : scorePercent >= 30 ? '#f59e0b' : '#ef4444';

    async function loadIncident() {
        if (!incidentId) return;
        try {
            const [inc, ev, hyp] = await Promise.all([
                invoke('incident_get', { incidentId }),
                invoke('incident_list_evidence', { incidentId }),
                invoke('incident_list_hypotheses', { incidentId }),
            ]);
            incident = inc;
            evidence = ev;
            hypotheses = hyp;
            error = null;
        } catch (e) {
            error = String(e);
        }
    }

    // Polling strategy: cheap queries, refresh every 3s while panel is open.
    // We could subscribe to DB events instead, but polling is simpler and
    // the load is trivial (SQLite on local disk).
    function startPolling() {
        stopPolling();
        pollTimer = setInterval(loadIncident, 3000);
    }
    function stopPolling() {
        if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
    }

    $: if (incidentId) {
        loadIncident();
        startPolling();
    } else {
        stopPolling();
        incident = null; evidence = []; hypotheses = [];
    }

    onDestroy(stopPolling);

    async function advanceTo(phase) {
        loading = true;
        try {
            incident = await invoke('incident_advance_phase', { incidentId, toPhase: phase });
            dispatch('phase-changed', { phase, incident });
            await loadIncident();
        } catch (e) {
            error = String(e);
        } finally {
            loading = false;
        }
    }

    async function recalcScore() {
        try {
            await invoke('incident_calculate_score', { incidentId });
            await loadIncident();
        } catch (e) {
            error = String(e);
        }
    }

    let showAbandonConfirm = false;
    function abandonIncident() { showAbandonConfirm = true; }
    async function doAbandon() {
        showAbandonConfirm = false;
        try {
            await invoke('incident_finalize', {
                args: {
                    incident_id: incidentId,
                    summary: 'Abandoned by user',
                    root_cause: 'unresolved',
                    status: 'abandoned',
                }
            });
            dispatch('closed');
        } catch (e) { error = String(e); }
    }

    function evidenceKindIcon(kind) {
        switch (kind) {
            case 'command_output': return '⚡';
            case 'log':            return '📋';
            case 'metric':         return '📊';
            case 'user_input':     return '💬';
            case 'observation':    return '👁';
            case 'hypothesis_test':return '🧪';
            default:               return '•';
        }
    }

    function parseJson(s, fallback) {
        try { return JSON.parse(s); } catch { return fallback; }
    }

    function fmtTime(ts) {
        if (!ts) return '';
        return new Date(ts * 1000).toLocaleTimeString();
    }
</script>

{#if incident}
<div class="inc-panel">
    <!-- ── Header ─────────────────────────────────────────────────────────── -->
    <div class="inc-header">
        <div class="inc-title-row">
            <Flag size={14} style="color:#ef4444;flex-shrink:0;" />
            <div class="inc-title" title={incident.title}>{incident.title}</div>
            <span class="inc-status inc-status-{incident.status}">{incident.status}</span>
        </div>
        <div class="inc-meta">
            <span>{incident.host_name || 'local'}</span>
            <span>·</span>
            <span>{isEN ? 'Loop' : 'Ciclo'} {incident.loop_count}/{incident.max_loops}</span>
            <span>·</span>
            <span style="color:{scoreColor};font-weight:600;" title={isEN ? 'Validity score — higher means hypotheses are better backed by evidence' : 'Score de validez — más alto = hipótesis mejor respaldadas'}>
                {isEN ? 'Score' : 'Validez'}: {scorePercent}%
            </span>
        </div>
    </div>

    <!-- ── Phase stepper ──────────────────────────────────────────────────── -->
    <div class="inc-phases">
        {#each PHASES as ph, i}
            {@const Icon = ph.icon}
            <div class="inc-phase {i === currentPhaseIdx ? 'active' : ''} {i < currentPhaseIdx ? 'past' : ''}"
                 style="--ph-color:{ph.color};">
                <Icon size={12} />
                <span class="inc-phase-label">{isEN ? ph.labelEn : ph.labelEs}</span>
            </div>
            {#if i < PHASES.length - 1}
                <span class="inc-phase-sep {i < currentPhaseIdx ? 'past' : ''}"></span>
            {/if}
        {/each}
    </div>

    <!-- ── Phase actions ──────────────────────────────────────────────────── -->
    {#if incident.status === 'open'}
    <div class="inc-actions">
        {#if incident.phase === 'extract'}
            <button class="inc-btn inc-btn-primary" on:click={() => advanceTo('plan')} disabled={loading}>
                → {isEN ? 'Start Planning' : 'Iniciar Planificación'}
            </button>
        {:else if incident.phase === 'plan'}
            <button class="inc-btn inc-btn-primary" on:click={() => advanceTo('investigate')} disabled={loading}>
                → {isEN ? 'Begin Investigation' : 'Iniciar Investigación'}
            </button>
        {:else if incident.phase === 'investigate'}
            <button class="inc-btn inc-btn-primary" on:click={() => advanceTo('diagnose')} disabled={loading}>
                → {isEN ? 'Move to Diagnosis' : 'Pasar a Diagnóstico'}
            </button>
            <button class="inc-btn inc-btn-secondary" on:click={() => advanceTo('plan')} disabled={loading} title={isEN ? 'Need more data — replan' : 'Necesito más datos — replanificar'}>
                <RotateCcw size={12}/> {isEN ? 'Replan' : 'Replanificar'}
            </button>
        {:else if incident.phase === 'diagnose'}
            <button class="inc-btn inc-btn-primary" on:click={() => advanceTo('report')} disabled={loading || scorePercent < 30}
                title={scorePercent < 30 ? (isEN ? 'Score too low — gather more evidence first' : 'Score muy bajo — más evidencia primero') : ''}>
                → {isEN ? 'Generate Report' : 'Generar Reporte'}
            </button>
            <button class="inc-btn inc-btn-secondary" on:click={() => advanceTo('plan')} disabled={loading}>
                <RotateCcw size={12}/> {isEN ? 'Reroute to Plan' : 'Volver a Plan'}
            </button>
            <button class="inc-btn inc-btn-subtle" on:click={recalcScore} disabled={loading}>
                {isEN ? 'Recalc Score' : 'Recalcular'}
            </button>
        {:else if incident.phase === 'report'}
            <button class="inc-btn inc-btn-primary" on:click={() => advanceTo('done')} disabled={loading}>
                <CheckCircle2 size={12}/> {isEN ? 'Mark Done' : 'Marcar Listo'}
            </button>
        {/if}

        <button class="inc-btn inc-btn-danger" on:click={abandonIncident} disabled={loading}>
            <XCircle size={12}/> {isEN ? 'Abandon' : 'Abandonar'}
        </button>
    </div>
    {/if}

    <!-- ── Evidence list ──────────────────────────────────────────────────── -->
    <div class="inc-section">
        <div class="inc-section-header">
            <span>{isEN ? 'Evidence' : 'Evidencia'}</span>
            <span class="inc-count">{evidence.length}</span>
        </div>
        {#if evidence.length === 0}
            <div class="inc-empty">{isEN ? 'No evidence captured yet.' : 'Sin evidencia capturada aún.'}</div>
        {:else}
            <div class="inc-evidence-list">
                {#each evidence as ev (ev.id)}
                    {@const tags = parseJson(ev.tags, [])}
                    <div class="inc-evidence">
                        <div class="inc-evidence-head">
                            <span class="inc-evidence-kind">{evidenceKindIcon(ev.kind)} {ev.kind}</span>
                            <span class="inc-evidence-source" title={ev.source}>{ev.source}</span>
                            <span class="inc-evidence-time">{fmtTime(ev.timestamp)}</span>
                        </div>
                        {#if tags.length > 0}
                        <div class="inc-evidence-tags">
                            {#each tags as tag}<span class="inc-tag">{tag}</span>{/each}
                        </div>
                        {/if}
                        <details class="inc-evidence-body">
                            <summary>{ev.content.slice(0, 80)}{ev.content.length > 80 ? '…' : ''}</summary>
                            <pre>{ev.content}</pre>
                        </details>
                        <div class="inc-evidence-id" title={isEN ? 'Hypotheses can reference this id' : 'Hipótesis pueden referenciar este id'}>id: {ev.id.slice(0, 12)}</div>
                    </div>
                {/each}
            </div>
        {/if}
    </div>

    <!-- ── Hypotheses ─────────────────────────────────────────────────────── -->
    <div class="inc-section">
        <div class="inc-section-header">
            <span>{isEN ? 'Hypotheses' : 'Hipótesis'}</span>
            <span class="inc-count">{hypotheses.length}</span>
        </div>
        {#if hypotheses.length === 0}
            <div class="inc-empty">{isEN ? 'No hypotheses proposed yet.' : 'Sin hipótesis aún.'}</div>
        {:else}
            {#each hypotheses as h (h.id)}
                {@const support = parseJson(h.supporting_evidence_ids, [])}
                {@const contra = parseJson(h.contradicting_evidence_ids, [])}
                <div class="inc-hypothesis inc-hyp-{h.status}">
                    <div class="inc-hyp-claim">{h.claim}</div>
                    <div class="inc-hyp-meta">
                        <span class="inc-hyp-status">{h.status}</span>
                        <span class="inc-hyp-conf">conf: {(h.confidence * 100).toFixed(0)}%</span>
                        <span class="inc-hyp-ev">+{support.length} / −{contra.length}</span>
                    </div>
                </div>
            {/each}
        {/if}
    </div>

    <!-- ── Final report ───────────────────────────────────────────────────── -->
    {#if incident.status !== 'open' && (incident.summary || incident.root_cause)}
    <div class="inc-section">
        <div class="inc-section-header">{isEN ? 'Resolution' : 'Resolución'}</div>
        {#if incident.root_cause}
            <div class="inc-resolution-rc"><strong>{isEN ? 'Root cause' : 'Causa raíz'}:</strong> {incident.root_cause}</div>
        {/if}
        {#if incident.summary}
            <div class="inc-resolution-sum">{incident.summary}</div>
        {/if}
    </div>
    {/if}

    {#if error}
        <div class="inc-error">⚠ {error}</div>
    {/if}
</div>
{/if}

<ConfirmModal
    open={showAbandonConfirm}
    variant="warn"
    icon="🏳"
    title={isEN ? 'Abandon incident' : 'Abandonar incidente'}
    message={isEN
        ? 'Abandon this incident? It will be marked as unresolved.'
        : '¿Abandonar este incidente? Se marcará como sin resolver.'}
    confirmLabel={isEN ? 'Abandon' : 'Abandonar'}
    cancelLabel={isEN ? 'Cancel' : 'Cancelar'}
    on:confirm={doAbandon}
    on:cancel={() => showAbandonConfirm = false}
/>

<style>
    .inc-panel {
        background: linear-gradient(180deg, rgba(239,68,68,.04) 0%, rgba(0,0,0,.2) 100%);
        border: 1px solid rgba(239,68,68,.25);
        border-radius: 6px;
        padding: 10px 12px;
        font-size: 12px;
        color: #e5e7eb;
        display: flex;
        flex-direction: column;
        gap: 10px;
    }
    .inc-header { border-bottom: 1px solid rgba(255,255,255,.05); padding-bottom: 8px; }
    .inc-title-row { display:flex; align-items:center; gap:6px; }
    .inc-title { font-weight: 600; flex: 1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
    .inc-status { font-size:9px; padding:2px 6px; border-radius:3px; text-transform:uppercase; letter-spacing:.5px; }
    .inc-status-open     { background: rgba(239,68,68,.15);  color:#fca5a5; }
    .inc-status-resolved { background: rgba(34,197,94,.15);  color:#86efac; }
    .inc-status-abandoned{ background: rgba(156,163,175,.15);color:#d1d5db; }
    .inc-meta { font-size:10px; color:#9ca3af; margin-top:4px; display:flex; gap:6px; flex-wrap:wrap; }

    .inc-phases { display:flex; align-items:center; gap:3px; flex-wrap:wrap; }
    .inc-phase {
        display:flex; align-items:center; gap:3px;
        font-size:10px; padding:3px 6px; border-radius:3px;
        background: rgba(255,255,255,.03);
        color:#6b7280;
        border: 1px solid transparent;
    }
    .inc-phase.past { color:#9ca3af; }
    .inc-phase.active {
        background: rgba(var(--ph-color-rgb, 139,92,246),.15);
        border-color: var(--ph-color);
        color: var(--ph-color);
        font-weight:600;
    }
    .inc-phase-sep { display:inline-block; width:10px; height:1px; background: rgba(255,255,255,.08); }
    .inc-phase-sep.past { background: rgba(16,185,129,.4); }

    .inc-actions { display:flex; gap:5px; flex-wrap:wrap; }
    .inc-btn {
        border-radius:4px; border:1px solid; padding:4px 10px;
        font-size:11px; cursor:pointer; display:inline-flex; align-items:center; gap:4px;
        transition:.12s;
    }
    .inc-btn:disabled { opacity:.4; cursor:not-allowed; }
    .inc-btn-primary   { background: rgba(16,185,129,.15); border-color: rgba(16,185,129,.4); color:#6ee7b7; }
    .inc-btn-primary:hover:not(:disabled)   { background: rgba(16,185,129,.25); }
    .inc-btn-secondary { background: rgba(139,92,246,.12); border-color: rgba(139,92,246,.35); color:#c4b5fd; }
    .inc-btn-secondary:hover:not(:disabled) { background: rgba(139,92,246,.22); }
    .inc-btn-subtle    { background: rgba(255,255,255,.04); border-color: rgba(255,255,255,.1); color:#9ca3af; }
    .inc-btn-danger    { background: rgba(239,68,68,.1); border-color: rgba(239,68,68,.3); color:#fca5a5; margin-left:auto; }
    .inc-btn-danger:hover:not(:disabled)    { background: rgba(239,68,68,.2); }

    .inc-section { border-top: 1px solid rgba(255,255,255,.05); padding-top:8px; }
    .inc-section-header {
        display:flex; justify-content:space-between; align-items:center;
        font-size:10px; text-transform:uppercase; letter-spacing:.5px;
        color:#9ca3af; margin-bottom:6px;
    }
    .inc-count { background:rgba(255,255,255,.06); padding:1px 6px; border-radius:8px; font-weight:600; }
    .inc-empty { font-size:11px; color:#6b7280; font-style:italic; padding:4px 0; }

    .inc-evidence-list { display:flex; flex-direction:column; gap:5px; max-height:300px; overflow-y:auto; }
    .inc-evidence {
        background: rgba(255,255,255,.02);
        border: 1px solid rgba(255,255,255,.05);
        border-radius:4px; padding:5px 7px;
    }
    .inc-evidence-head { display:flex; gap:6px; align-items:center; font-size:10px; }
    .inc-evidence-kind { color:#93c5fd; font-weight:500; }
    .inc-evidence-source { color:#9ca3af; flex:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
    .inc-evidence-time { color:#6b7280; font-size:9px; }
    .inc-evidence-tags { display:flex; gap:3px; flex-wrap:wrap; margin-top:3px; }
    .inc-tag { background: rgba(139,92,246,.15); color:#c4b5fd; padding:1px 5px; border-radius:8px; font-size:9px; }
    .inc-evidence-body { margin-top:4px; font-size:11px; }
    .inc-evidence-body summary { cursor:pointer; color:#d1d5db; }
    .inc-evidence-body pre {
        margin-top:4px; font-size:10px; background:rgba(0,0,0,.3);
        padding:6px; border-radius:3px; max-height:200px; overflow:auto;
        white-space:pre-wrap; word-break:break-word;
    }
    .inc-evidence-id { font-family:monospace; font-size:9px; color:#6b7280; margin-top:3px; }

    .inc-hypothesis {
        background: rgba(255,255,255,.02);
        border-left: 3px solid #8b5cf6;
        padding: 5px 8px; margin-top:4px;
    }
    .inc-hyp-validated    { border-left-color:#10b981; }
    .inc-hyp-refuted      { border-left-color:#ef4444; }
    .inc-hyp-inconclusive { border-left-color:#f59e0b; }
    .inc-hyp-claim { font-weight:500; color:#f3f4f6; }
    .inc-hyp-meta { display:flex; gap:8px; font-size:10px; color:#9ca3af; margin-top:3px; }
    .inc-hyp-status { text-transform:uppercase; font-weight:600; }

    .inc-resolution-rc { margin-bottom:4px; }
    .inc-resolution-sum { color:#d1d5db; font-size:11px; white-space:pre-wrap; }

    .inc-error {
        background: rgba(239,68,68,.1); border:1px solid rgba(239,68,68,.3);
        color:#fca5a5; padding:6px 8px; border-radius:4px; font-size:11px;
    }
</style>
