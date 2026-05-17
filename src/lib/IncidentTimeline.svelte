<!-- ── IncidentTimeline.svelte — Visual timeline for SRE incident phases ──────
     Shows when each phase was entered, with evidence/action markers along the
     timeline and a running duration. Uses OPSCENTER design tokens.
-->
<script lang="ts">
    import { invoke } from '@tauri-apps/api/core';
    import { onMount } from 'svelte';

    export let incidentId: string;
    export let isEN: boolean = false;

    interface PhaseEntry {
        phase: string;
        entered_at: number;
    }

    interface EvidenceMarker {
        id: string;
        kind: string;
        source: string;
        phase: string;
        timestamp: number;
    }

    interface ActionMarker {
        id: string;
        command: string;
        phase: string;
        executed_at: number;
        chain_hash: string;
    }

    interface IncidentRecord {
        id: string;
        title: string;
        phase: string;
        status: string;
        created_at: number;
        resolved_at: number | null;
    }

    let incident: IncidentRecord | null = null;
    let evidence: EvidenceMarker[] = [];
    let phases: PhaseEntry[] = [];
    let loading = true;
    let error = '';

    // Derive phases from evidence timestamps grouped by phase
    const PHASE_ORDER = ['detection', 'triage', 'investigation', 'containment', 'remediation', 'review'];
    const PHASE_COLORS: Record<string, string> = {
        detection:     'var(--ops-status-warn, #f59e0b)',
        triage:        'var(--ops-status-info, #60a5fa)',
        investigation: 'var(--ops-cpu, #a78bfa)',
        containment:   'var(--ops-status-error, #ef4444)',
        remediation:   'var(--ops-status-ok, #10b981)',
        review:        'var(--ops-status-muted, #475569)',
    };

    onMount(async () => {
        try {
            incident = await invoke<IncidentRecord>('incident_get', { incidentId });
            evidence = await invoke<EvidenceMarker[]>('incident_list_evidence', { incidentId });

            // Build phase entries from evidence phase transitions
            const phaseMap = new Map<string, number>();
            if (incident) {
                phaseMap.set('detection', incident.created_at);
            }
            for (const ev of evidence) {
                if (ev.phase && !phaseMap.has(ev.phase)) {
                    phaseMap.set(ev.phase, ev.timestamp);
                }
            }
            // Current phase if not yet seen
            if (incident?.phase && !phaseMap.has(incident.phase)) {
                phaseMap.set(incident.phase, Math.floor(Date.now() / 1000));
            }

            phases = PHASE_ORDER
                .filter(p => phaseMap.has(p))
                .map(p => ({ phase: p, entered_at: phaseMap.get(p)! }));

        } catch (e) {
            error = String(e);
        }
        loading = false;
    });

    function fmtTime(ts: number): string {
        return new Date(ts * 1000).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    }

    function fmtDuration(a: number, b: number): string {
        const s = Math.abs(b - a);
        if (s < 60) return `${s}s`;
        if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
        const h = Math.floor(s / 3600);
        const m = Math.floor((s % 3600) / 60);
        return `${h}h ${m}m`;
    }

    $: totalDuration = incident
        ? fmtDuration(incident.created_at, incident.resolved_at || Math.floor(Date.now() / 1000))
        : '';

    $: evidenceByPhase = (() => {
        const m = new Map<string, EvidenceMarker[]>();
        for (const ev of evidence) {
            if (!m.has(ev.phase)) m.set(ev.phase, []);
            m.get(ev.phase)!.push(ev);
        }
        return m;
    })();
</script>

{#if loading}
    <div class="tl-loading">{isEN ? 'Loading timeline...' : 'Cargando timeline...'}</div>
{:else if error}
    <div class="tl-error">{error}</div>
{:else if incident}
    <div class="tl-wrap">
        <div class="tl-header">
            <span class="tl-title">{isEN ? 'Incident Timeline' : 'Timeline del Incidente'}</span>
            <span class="tl-duration">{totalDuration}</span>
            <span class="tl-status" class:resolved={incident.status === 'resolved'}>
                {incident.status}
            </span>
        </div>

        <div class="tl-track">
            {#each phases as ph, i}
                {@const nextTs = i < phases.length - 1 ? phases[i + 1].entered_at : (incident.resolved_at || Math.floor(Date.now() / 1000))}
                {@const phaseDur = fmtDuration(ph.entered_at, nextTs)}
                {@const phaseEvidence = evidenceByPhase.get(ph.phase) || []}
                <div class="tl-phase">
                    <div class="tl-node" style="background:{PHASE_COLORS[ph.phase] || 'var(--ops-status-muted)'}">
                        <span class="tl-node-dot"></span>
                    </div>
                    <div class="tl-phase-body">
                        <div class="tl-phase-header">
                            <span class="tl-phase-name" style="color:{PHASE_COLORS[ph.phase] || 'var(--txt2)'}">{ph.phase}</span>
                            <span class="tl-phase-time">{fmtTime(ph.entered_at)}</span>
                            <span class="tl-phase-dur">{phaseDur}</span>
                        </div>
                        {#if phaseEvidence.length > 0}
                            <div class="tl-evidence-list">
                                {#each phaseEvidence.slice(0, 5) as ev}
                                    <div class="tl-evidence-item">
                                        <span class="tl-ev-kind">{ev.kind}</span>
                                        <span class="tl-ev-source">{ev.source}</span>
                                        <span class="tl-ev-time">{fmtTime(ev.timestamp)}</span>
                                    </div>
                                {/each}
                                {#if phaseEvidence.length > 5}
                                    <div class="tl-evidence-more">+{phaseEvidence.length - 5} more</div>
                                {/if}
                            </div>
                        {/if}
                    </div>
                    {#if i < phases.length - 1}
                        <div class="tl-connector"></div>
                    {/if}
                </div>
            {/each}

            {#if incident.resolved_at}
                <div class="tl-phase">
                    <div class="tl-node tl-node-final">
                        <span class="tl-node-dot"></span>
                    </div>
                    <div class="tl-phase-body">
                        <div class="tl-phase-header">
                            <span class="tl-phase-name" style="color:var(--ops-status-ok)">{isEN ? 'Resolved' : 'Resuelto'}</span>
                            <span class="tl-phase-time">{fmtTime(incident.resolved_at)}</span>
                        </div>
                    </div>
                </div>
            {/if}
        </div>
    </div>
{/if}

<style>
    .tl-wrap {
        font-family: var(--ops-font, monospace);
        font-size: var(--ops-font-size-sm, 11px);
        background: var(--ops-surface-1, #0c0f1a);
        border: 1px solid var(--ops-border, rgba(255,255,255,0.06));
        border-radius: var(--ops-radius-lg, 6px);
        padding: 12px 16px;
    }
    .tl-header {
        display: flex;
        align-items: center;
        gap: 10px;
        margin-bottom: 14px;
        padding-bottom: 8px;
        border-bottom: 1px solid var(--ops-border, rgba(255,255,255,0.06));
    }
    .tl-title {
        font-weight: 700;
        font-size: 12px;
        color: var(--txt1, #e2e8f0);
        text-transform: uppercase;
        letter-spacing: .4px;
    }
    .tl-duration {
        margin-left: auto;
        font-weight: 600;
        color: var(--ops-status-info, #60a5fa);
        font-variant-numeric: tabular-nums;
    }
    .tl-status {
        padding: 1px 8px;
        border-radius: 8px;
        font-size: 9px;
        font-weight: 600;
        text-transform: uppercase;
        background: rgba(245,158,11,.12);
        color: var(--ops-status-warn, #f59e0b);
    }
    .tl-status.resolved {
        background: rgba(16,185,129,.12);
        color: var(--ops-status-ok, #10b981);
    }

    .tl-track {
        position: relative;
        padding-left: 20px;
    }
    .tl-phase {
        position: relative;
        padding-bottom: 16px;
    }
    .tl-phase:last-child { padding-bottom: 0; }

    .tl-node {
        position: absolute;
        left: -20px;
        top: 2px;
        width: 12px;
        height: 12px;
        border-radius: 50%;
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .tl-node-dot {
        width: 5px;
        height: 5px;
        border-radius: 50%;
        background: var(--ops-surface-1, #0c0f1a);
    }
    .tl-node-final {
        background: var(--ops-status-ok, #10b981);
    }

    .tl-connector {
        position: absolute;
        left: -15px;
        top: 14px;
        bottom: 0;
        width: 2px;
        background: var(--ops-border, rgba(255,255,255,0.06));
    }

    .tl-phase-body { padding-left: 4px; }
    .tl-phase-header {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .tl-phase-name {
        font-weight: 700;
        font-size: 11px;
        text-transform: capitalize;
    }
    .tl-phase-time {
        font-size: 10px;
        color: var(--txt3, #475569);
        font-variant-numeric: tabular-nums;
    }
    .tl-phase-dur {
        font-size: 9px;
        color: var(--txt3, #475569);
        padding: 0 5px;
        background: rgba(255,255,255,.03);
        border-radius: 4px;
    }

    .tl-evidence-list {
        margin-top: 6px;
        padding-left: 8px;
        border-left: 1px solid rgba(255,255,255,.04);
    }
    .tl-evidence-item {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 2px 0;
        font-size: 10px;
        color: var(--txt2, #94a3b8);
    }
    .tl-ev-kind {
        padding: 0 4px;
        background: rgba(255,255,255,.04);
        border-radius: 3px;
        font-size: 9px;
        font-weight: 500;
    }
    .tl-ev-source { font-weight: 500; }
    .tl-ev-time {
        margin-left: auto;
        font-size: 9px;
        color: var(--txt3, #475569);
        font-variant-numeric: tabular-nums;
    }
    .tl-evidence-more {
        font-size: 9px;
        color: var(--txt3, #475569);
        padding: 2px 0;
        font-style: italic;
    }

    .tl-loading, .tl-error {
        padding: 12px;
        font-size: 11px;
        text-align: center;
    }
    .tl-error { color: var(--ops-status-error, #ef4444); }
    .tl-loading { color: var(--txt3, #475569); }
</style>
