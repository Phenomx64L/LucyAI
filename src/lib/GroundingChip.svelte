<!-- ── GroundingChip.svelte (v1.6.0) ───────────────────────────────────────
     Compact pill showing the live grounding score for a memory. The score
     is computed at query time by the Rust backend (per Kappa Graph
     ADR-044) — no caching, no stale values.

     Visual:
       ◉ 87%        → green, well-supported (strength ≥ 0.55)
       ◉ 35%        → amber, contested
       ◉ 12%        → red, default-filtered
       ◉ 50% prior  → blue, no observed evidence yet

     Click → emits 'expand' so the parent can show the instances popover.

     Props:
       memoryKind  — 'agent' | 'core'
       memoryId    — the memory's id (string in both cases)
       isEN        — i18n
       compact     — if true, show only the dot + pct (no "%" label)
─────────────────────────────────────────────────────────────────────── -->
<script lang="ts">
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { createEventDispatcher, onMount } from 'svelte';
    import {
        getGroundingScore, fmtStrengthPct, strengthTone,
        type GroundingScore, type MemoryKind,
    } from '$lib/memory-grounding';

    export let memoryKind: MemoryKind;
    export let memoryId:   string;
    export let isEN       = false;
    export let compact    = false;

    const dispatch = createEventDispatcher<{ expand: { memoryKind: MemoryKind; memoryId: string } }>();

    let score: GroundingScore | null = null;
    let loadErr = '';

    async function refresh() {
        loadErr = '';
        try {
            score = await getGroundingScore(memoryKind, memoryId);
        } catch (e) {
            loadErr = String(e);
        }
    }
    onMount(refresh);

    // Refresh when the memoryId changes (parent might recycle the component).
    $: if (memoryId) refresh();

    $: tone = score ? strengthTone(score) : 'info';
    $: pct  = score ? fmtStrengthPct(score) : '—';
    $: tooltip = (() => {
        if (!score) return $trad('Cargando…');
        if (score.from_prior) {
            return isEN
                ? `Prior confidence (no evidence yet). ${pct}`
                : `Confianza inicial (sin evidencia aún). ${pct}`;
        }
        return isEN
            ? `Grounding ${pct} · ${score.support_count} support / ${score.contradict_count} contradict · weighted ${score.support_weight.toFixed(1)} / ${score.contradict_weight.toFixed(1)}`
            : `Anclaje ${pct} · ${score.support_count} apoyos / ${score.contradict_count} contradicciones · ponderado ${score.support_weight.toFixed(1)} / ${score.contradict_weight.toFixed(1)}`;
    })();
</script>

{#if loadErr}
    <span class="gc gc-err" title={loadErr}>◉ err</span>
{:else if !score}
    <span class="gc gc-loading">◉ …</span>
{:else}
    <button class="gc gc-{tone}" type="button"
            title={tooltip}
            on:click={() => dispatch('expand', { memoryKind, memoryId })}>
        <span class="gc-dot">◉</span>
        <span class="gc-pct">{pct}</span>
        {#if !compact && score.from_prior}
            <span class="gc-tag">{$trad('inicial')}</span>
        {/if}
    </button>
{/if}

<style>
    .gc {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 1px 7px;
        border-radius: 9px;
        font-size: 10.5px;
        font-weight: 600;
        font-family: var(--mono, ui-monospace, monospace);
        line-height: 1.4;
        border: 1px solid transparent;
        background: transparent;
        cursor: pointer;
        transition: background .12s, border-color .12s;
    }
    .gc-dot { font-size: 9px; line-height: 1; }
    .gc-pct { letter-spacing: .2px; }
    .gc-tag {
        font-size: 9px;
        font-weight: 500;
        opacity: .7;
        margin-left: 2px;
        text-transform: uppercase;
        letter-spacing: .4px;
    }
    /* Tone bands mirror the ADR-044 example thresholds:
         crit  < 0.20  (filtered by default)
         warn  < 0.55  (contested but visible)
         ok    >= 0.55
         info          (prior — no observed evidence) */
    .gc-ok   { color: var(--acc, #10b981); background: rgba(16, 185, 129, .08);  border-color: rgba(16, 185, 129, .25); }
    .gc-warn { color: var(--amber, #f59e0b); background: rgba(245, 158, 11, .08); border-color: rgba(245, 158, 11, .25); }
    .gc-crit { color: var(--red, #ef4444); background: rgba(239, 68, 68, .08);   border-color: rgba(239, 68, 68, .30); }
    .gc-info { color: var(--blue, #3b9eff); background: rgba(59, 158, 255, .07); border-color: rgba(59, 158, 255, .22); }
    .gc:hover { filter: brightness(1.15); }
    .gc-loading, .gc-err {
        color: var(--txt3, #475569);
        background: rgba(255, 255, 255, .03);
        border: 1px solid var(--bdr, #1a2030);
        cursor: default;
    }
</style>
