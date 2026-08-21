<!--
  ── SkillPicker.svelte — F7 Sprint 8 ───────────────────────────────────────

  Floating panel listing saved skills. Each row shows:
    • category badge + name
    • usage count (frequency)
    • short script preview (collapsible)
    • Invoke button (dispatches 'invoke' with the resolved trigger)
    • Edit / Delete (dispatches with id)

  Filtering:
    • Top text input — fuzzy match on name + description + triggers
    • Category chips (auto-derived from data)

  Opened via prop `open` (parent controls visibility). Closes on Escape or
  backdrop click.
-->
<script lang="ts">
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { createEventDispatcher, onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { fade, fly } from 'svelte/transition';

    export let open: boolean = false;
    export let isEN: boolean = false;

    const dispatch = createEventDispatcher<{
        close: void;
        invoke: { name: string; script: string };
        edit: { id: string };
    }>();

    interface SkillRow {
        id: string;
        name: string;
        category: string;
        triggers: string;
        script: string;
        description: string | null;
        usage_count: number;
        last_executed: number | null;
    }

    let skills: SkillRow[] = [];
    let loading = false;
    let error = '';
    let query = '';
    let activeCategory: string | null = null;
    let expandedId: string | null = null;

    async function loadSkills() {
        loading = true;
        error = '';
        try {
            skills = (await invoke('list_skills', { category: null })) as SkillRow[];
        } catch (e) {
            error = String(e);
            skills = [];
        }
        loading = false;
    }

    // ── Derived state ──────────────────────────────────────────────────
    $: categories = Array.from(new Set(skills.map(s => s.category))).sort();
    $: filtered = (() => {
        const q = query.trim().toLowerCase();
        return skills.filter(s => {
            if (activeCategory && s.category !== activeCategory) return false;
            if (!q) return true;
            const hay = `${s.name} ${s.description || ''} ${s.triggers}`.toLowerCase();
            return hay.includes(q);
        });
    })();

    function parseTriggers(json: string): string[] {
        try { return JSON.parse(json); } catch { return []; }
    }

    function invokeSkill(s: SkillRow) {
        const triggers = parseTriggers(s.triggers);
        const primary = triggers[0] || s.name;
        dispatch('invoke', { name: primary, script: s.script });
    }

    async function deleteSkill(s: SkillRow) {
        const { lucyConfirm, lucyAlert } = await import('$lib/dialog-service');
        const ok = await lucyConfirm(
            isEN ? `Delete skill "${s.name}"?` : `¿Eliminar skill "${s.name}"?`,
            { tone: 'danger',
              description: $trad('No se puede deshacer.'),
              confirmLabel: $trad('Eliminar') });
        if (!ok) return;
        try {
            await invoke('delete_skill', { id: s.id });
            await loadSkills();
        } catch (e) {
            await lucyAlert($trad('Falló la eliminación'),
                { tone: 'danger', description: String(e) });
        }
    }

    function relativeTime(unixSec: number | null): string {
        if (!unixSec) return $trad('sin uso');
        const ageSec = Math.floor(Date.now() / 1000) - unixSec;
        if (ageSec < 60) return $trad('ahora');
        if (ageSec < 3600) return `${Math.floor(ageSec / 60)}m`;
        if (ageSec < 86_400) return `${Math.floor(ageSec / 3600)}h`;
        return `${Math.floor(ageSec / 86_400)}d`;
    }

    function onKey(e: KeyboardEvent) {
        if (e.key === 'Escape') dispatch('close');
    }

    onMount(loadSkills);
    $: if (open) { loadSkills(); }
</script>

<svelte:window on:keydown={onKey} />

{#if open}
    <div class="sp-backdrop"
         role="button" tabindex="-1" aria-label="Close skill picker"
         on:click={() => dispatch('close')}
         on:keydown
         transition:fade={{ duration: 140 }}></div>

    <div class="sp-panel" role="dialog" aria-label="Skill picker"
         transition:fly={{ y: -8, duration: 200 }}>
        <header class="sp-head">
            <div class="sp-title">
                <span class="sp-glyph">⌖</span>
                <h2>{$trad('Skills guardadas')}</h2>
                <span class="sp-count">{filtered.length}/{skills.length}</span>
            </div>
            <button class="sp-close" on:click={() => dispatch('close')} title="Esc">✕</button>
        </header>

        <div class="sp-filter">
            <input type="text"
                   class="sp-search"
                   bind:value={query}
                   placeholder={$trad('Buscar nombre, descripción, triggers…')}
                   autocomplete="off" />
            {#if categories.length > 0}
                <div class="sp-cats">
                    <button class="sp-cat" class:active={activeCategory === null}
                            on:click={() => activeCategory = null}>
                        {$trad('todas')}
                    </button>
                    {#each categories as cat}
                        <button class="sp-cat" class:active={activeCategory === cat}
                                on:click={() => activeCategory = (activeCategory === cat ? null : cat)}>
                            {cat}
                        </button>
                    {/each}
                </div>
            {/if}
        </div>

        <div class="sp-list">
            {#if loading}
                <div class="sp-empty">{$trad('Cargando…')}</div>
            {:else if error}
                <div class="sp-empty sp-err">{error}</div>
            {:else if filtered.length === 0}
                <div class="sp-empty">
                    {#if skills.length === 0}
                        {$trad('Sin skills todavía. Promociona un runbook con /promote-runbook.')}
                    {:else}
                        {$trad('Sin coincidencias para el filtro.')}
                    {/if}
                </div>
            {:else}
                {#each filtered as s (s.id)}
                    {@const triggers = parseTriggers(s.triggers)}
                    {@const isOpen = expandedId === s.id}
                    <div class="sp-row" class:open={isOpen}>
                        <div class="sp-row-head">
                            <span class="sp-cat-badge">{s.category}</span>
                            <span class="sp-name">{s.name}</span>
                            <span class="sp-uc" title={$trad('Veces usado · última ejecución')}>
                                {s.usage_count}× · {relativeTime(s.last_executed)}
                            </span>
                            <button class="sp-btn sp-invoke"
                                    on:click={() => invokeSkill(s)}
                                    title={$trad('Insertar en el input')}>
                                ▶ {$trad('Invocar')}
                            </button>
                            <button class="sp-btn sp-expand"
                                    on:click={() => expandedId = (isOpen ? null : s.id)}
                                    title={isOpen ? ($trad('Contraer')) : ($trad('Ver script'))}>
                                {isOpen ? '▾' : '▸'}
                            </button>
                            <button class="sp-btn sp-del"
                                    on:click={() => deleteSkill(s)}
                                    title={$trad('Eliminar skill')}>✕</button>
                        </div>
                        {#if s.description}
                            <div class="sp-desc">{s.description}</div>
                        {/if}
                        {#if triggers.length > 0}
                            <div class="sp-triggers">
                                {#each triggers as t}
                                    <code class="sp-trig">{t}</code>
                                {/each}
                            </div>
                        {/if}
                        {#if isOpen}
                            <pre class="sp-script">{s.script}</pre>
                        {/if}
                    </div>
                {/each}
            {/if}
        </div>
    </div>
{/if}

<style>
    .sp-backdrop {
        position: fixed; inset: 0;
        z-index: 8500;
        background: rgba(0,0,0,0.45);
        backdrop-filter: blur(2px);
    }
    .sp-panel {
        position: fixed;
        z-index: 8600;
        top: 60px; left: 50%;
        transform: translateX(-50%);
        width: min(760px, 92vw);
        max-height: calc(100vh - 100px);
        background: var(--bg-card, #161b22);
        border: 1px solid var(--border-color, #1e293b);
        border-radius: 8px;
        box-shadow: 0 24px 64px -16px rgba(0,0,0,0.65),
                    0 0 0 1px rgba(16,185,129,0.10);
        display: flex; flex-direction: column;
        overflow: hidden;
    }
    .sp-head {
        display: flex;
        align-items: center;
        padding: 10px 14px;
        border-bottom: 1px solid var(--border-color);
        gap: 10px;
    }
    .sp-title {
        flex: 1;
        display: flex;
        align-items: baseline;
        gap: 8px;
    }
    .sp-glyph {
        font-size: 14px;
        color: var(--acc, #10b981);
    }
    .sp-title h2 {
        margin: 0;
        font-size: 13px;
        font-weight: 700;
        letter-spacing: 0.5px;
        text-transform: uppercase;
        color: var(--text-bright, #f1f5f9);
    }
    .sp-count {
        font-family: var(--font-mono);
        font-size: 10px;
        color: var(--text-muted);
        font-variant-numeric: tabular-nums;
    }
    .sp-close {
        width: 24px; height: 24px;
        border-radius: 4px;
        border: 1px solid rgba(255,255,255,0.08);
        background: transparent;
        color: var(--text-muted);
        cursor: pointer;
        font-size: 12px;
        line-height: 1;
        transition: background var(--motion-fast) var(--ease-out);
    }
    .sp-close:hover { background: rgba(255,255,255,0.06); color: var(--text-bright); }

    .sp-filter {
        padding: 10px 14px 6px;
        display: flex; flex-direction: column;
        gap: 8px;
        border-bottom: 1px solid rgba(255,255,255,0.04);
    }
    .sp-search {
        width: 100%;
        padding: 8px 10px;
        background: rgba(0,0,0,0.25);
        border: 1px solid var(--border-color);
        border-radius: 4px;
        font-family: var(--font-mono);
        font-size: 12px;
        color: var(--text-bright);
        outline: none;
        transition: border-color var(--motion-fast) var(--ease-out);
    }
    .sp-search:focus { border-color: var(--acc); }
    .sp-cats {
        display: flex; flex-wrap: wrap;
        gap: 4px;
    }
    .sp-cat {
        padding: 2px 8px;
        font-family: var(--font-mono);
        font-size: 10px;
        background: rgba(255,255,255,0.04);
        border: 1px solid rgba(255,255,255,0.08);
        border-radius: 12px;
        color: var(--text-muted);
        cursor: pointer;
        transition: background var(--motion-fast) var(--ease-out);
    }
    .sp-cat:hover { background: rgba(255,255,255,0.08); color: var(--text-main); }
    .sp-cat.active {
        background: rgba(16,185,129,0.12);
        border-color: rgba(16,185,129,0.35);
        color: var(--acc);
    }

    .sp-list {
        overflow-y: auto;
        padding: 6px 8px 12px;
        flex: 1;
        min-height: 80px;
    }
    .sp-empty {
        padding: 24px 16px;
        text-align: center;
        font-family: var(--font-mono);
        font-size: 11px;
        color: var(--text-muted);
        font-style: italic;
    }
    .sp-err { color: var(--red); }

    .sp-row {
        padding: 8px 10px;
        margin: 4px 4px;
        background: rgba(255,255,255,0.02);
        border: 1px solid rgba(255,255,255,0.04);
        border-radius: 5px;
        transition: background var(--motion-fast) var(--ease-out);
    }
    .sp-row:hover { background: rgba(255,255,255,0.04); }
    .sp-row.open { background: rgba(16,185,129,0.04); border-color: rgba(16,185,129,0.18); }

    .sp-row-head {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .sp-cat-badge {
        padding: 1px 6px;
        font-family: var(--font-mono);
        font-size: 9px;
        text-transform: uppercase;
        letter-spacing: 0.4px;
        color: var(--blue, #3b9eff);
        background: rgba(59,158,255,0.08);
        border: 1px solid rgba(59,158,255,0.20);
        border-radius: 3px;
    }
    .sp-name {
        flex: 1;
        font-family: var(--font-mono);
        font-size: 12px;
        font-weight: 600;
        color: var(--text-bright);
    }
    .sp-uc {
        font-family: var(--font-mono);
        font-size: 10px;
        color: var(--text-muted);
        font-variant-numeric: tabular-nums;
    }
    .sp-btn {
        padding: 3px 8px;
        font-family: var(--font-mono);
        font-size: 10px;
        font-weight: 500;
        background: rgba(255,255,255,0.04);
        border: 1px solid rgba(255,255,255,0.10);
        border-radius: 3px;
        color: var(--text-main);
        cursor: pointer;
        transition: background var(--motion-fast) var(--ease-out),
                    border-color var(--motion-fast) var(--ease-out);
    }
    .sp-btn:hover { background: rgba(255,255,255,0.08); border-color: rgba(255,255,255,0.18); }
    .sp-invoke {
        color: var(--acc);
        border-color: rgba(16,185,129,0.25);
        background: rgba(16,185,129,0.06);
    }
    .sp-invoke:hover {
        background: rgba(16,185,129,0.14);
        border-color: rgba(16,185,129,0.45);
    }
    .sp-del:hover {
        color: var(--red);
        border-color: rgba(239,68,68,0.40);
        background: rgba(239,68,68,0.08);
    }

    .sp-desc {
        margin-top: 4px;
        font-size: 11px;
        color: var(--text-main);
        opacity: 0.8;
    }
    .sp-triggers {
        margin-top: 4px;
        display: flex; flex-wrap: wrap;
        gap: 4px;
    }
    .sp-trig {
        padding: 1px 5px;
        font-family: var(--font-mono);
        font-size: 9px;
        background: rgba(255,255,255,0.04);
        border-radius: 3px;
        color: var(--text-muted);
    }
    .sp-script {
        margin: 6px 0 0;
        padding: 6px 8px;
        background: rgba(0,0,0,0.35);
        border-radius: 4px;
        font-family: var(--font-mono);
        font-size: 10px;
        line-height: 1.45;
        color: var(--text-main);
        white-space: pre;
        overflow-x: auto;
        max-height: 200px;
    }
</style>
