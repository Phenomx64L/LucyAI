<!-- ── SkillPresetPicker.svelte (v1.6.1) ──────────────────────────────────
     bits-ui Dialog picker for the curated ECC-adapted skill presets.

     Opens via `bind:open`. Click a preset card → it becomes active and the
     picker closes. The currently active one renders with the green accent.

     A "Deactivate" button at the bottom clears the selection. The user can
     also press the deactivate button on the active card itself.
─────────────────────────────────────────────────────────────────────── -->
<script lang="ts">
    import { Dialog } from 'bits-ui';
    import { createEventDispatcher } from 'svelte';
    import {
        groupedPresets, CATEGORY_LABELS,
        type SkillPreset, type SkillPresetCategory,
    } from '$lib/skill-presets';
    import {
        activeSkillPresetId, setActivePresetId,
    } from '$lib/skill-preset-store';
    // v1.7.14 — Toast confirmation + cross-bridge clear. Selecting a
    // preset used to update the store silently and close the modal,
    // but the modal sometimes "stuck" visually and the user had no
    // explicit signal that activation happened. The toast removes the
    // ambiguity. We also clear any active security skill (from
    // /sec-skill use) so the single-active-framing invariant the
    // v1.7.5 chip system assumes holds.
    import { toast as sonnerToast } from 'svelte-sonner';
    import { clearActiveSecuritySkill } from '$lib/security-skill-bridge';

    export let open = false;
    export let isEN = false;

    const dispatch = createEventDispatcher<{ close: void }>();

    function onOpenChange(v: boolean) {
        if (open && !v) dispatch('close');
        open = v;
    }

    function activate(p: SkillPreset) {
        // Clear any active security skill — the chip system enforces
        // single-active-framing per turn.
        try { clearActiveSecuritySkill(); } catch { /* ignore */ }
        setActivePresetId(p.id);
        const nm = isEN ? p.name.en : p.name.es;
        sonnerToast.success(
            isEN ? `✦ Preset activated: ${nm}` : `✦ Plantilla activada: ${nm}`,
            { description: isEN
                ? 'Will shape Lucy\'s next response. A purple chip will appear in chat.'
                : 'Moldeará la próxima respuesta de Lucy. Verás un chip morado en el chat.',
              duration: 3500,
            },
        );
        open = false;
        dispatch('close');
    }
    function deactivate() {
        try { clearActiveSecuritySkill(); } catch { /* ignore */ }
        setActivePresetId(null);
        sonnerToast.success(
            isEN ? '✓ Preset deactivated' : '✓ Plantilla desactivada',
            { description: isEN
                ? 'Lucy will respond with default behaviour from the next turn.'
                : 'Lucy responderá con comportamiento por defecto desde el siguiente turno.',
              duration: 2500,
            },
        );
        open = false;
        dispatch('close');
    }

    $: groups = groupedPresets();
</script>

<Dialog.Root bind:open onOpenChange={onOpenChange}>
    <Dialog.Portal>
        <Dialog.Overlay class="spp-overlay" />
        <Dialog.Content class="spp-wrap">
            <div class="spp-card">
                <header class="spp-hdr">
                    <Dialog.Title class="spp-title">
                        ✦ {isEN ? 'Skill Presets' : 'Plantillas de habilidad'}
                    </Dialog.Title>
                    <Dialog.Description class="spp-sub">
                        {isEN
                            ? 'Pick a behavioural framing for Lucy. Adapted from ECC. The preset is prepended to the system prompt — it shapes behaviour, never removes your core memory or guardrails.'
                            : 'Elige un encuadre de comportamiento para Lucy. Adaptado de ECC. Se antepone al system prompt — modifica comportamiento, nunca elimina tu memoria core ni guardrails.'}
                    </Dialog.Description>
                    <Dialog.Close class="spp-x" aria-label="Close">✕</Dialog.Close>
                </header>

                <div class="spp-body">
                    {#each groups as g (g.category)}
                        <section class="spp-group">
                            <h3 class="spp-grp-title">
                                {isEN ? CATEGORY_LABELS[g.category].en : CATEGORY_LABELS[g.category].es}
                            </h3>
                            <ul class="spp-list">
                                {#each g.items as p (p.id)}
                                    <li class="spp-card-item"
                                        class:active={$activeSkillPresetId === p.id}>
                                        <button class="spp-card-btn"
                                                on:click={() => activate(p)}>
                                            <div class="spp-card-head">
                                                <span class="spp-card-name">{isEN ? p.name.en : p.name.es}</span>
                                                {#if $activeSkillPresetId === p.id}
                                                    <span class="spp-active-tag">
                                                        {isEN ? 'ACTIVE' : 'ACTIVO'}
                                                    </span>
                                                {/if}
                                            </div>
                                            <p class="spp-card-desc">{isEN ? p.description.en : p.description.es}</p>
                                            <code class="spp-card-source">{p.source}</code>
                                        </button>
                                    </li>
                                {/each}
                            </ul>
                        </section>
                    {/each}
                </div>

                <footer class="spp-foot">
                    <span class="spp-hint">
                        {#if $activeSkillPresetId}
                            {isEN
                                ? 'A preset is active. Click "Deactivate" to return to default behaviour.'
                                : 'Hay una plantilla activa. Click en "Desactivar" para volver al comportamiento por defecto.'}
                        {:else}
                            {isEN
                                ? 'No preset active. Lucy uses default behaviour.'
                                : 'Sin plantilla activa. Lucy usa comportamiento por defecto.'}
                        {/if}
                    </span>
                    {#if $activeSkillPresetId}
                        <button class="spp-deactivate" on:click={deactivate}>
                            {isEN ? 'Deactivate' : 'Desactivar'}
                        </button>
                    {/if}
                </footer>
            </div>
        </Dialog.Content>
    </Dialog.Portal>
</Dialog.Root>

<style>
    :global(.spp-overlay) {
        position: fixed; inset: 0;
        background: rgba(2, 6, 12, .84);
        backdrop-filter: blur(10px);
        z-index: 7000;
    }
    :global(.spp-wrap) {
        position: fixed; inset: 0;
        z-index: 7001;
        display: flex; align-items: center; justify-content: center;
        padding: 24px;
        pointer-events: none;
    }
    :global(.spp-wrap > .spp-card) { pointer-events: auto; }
    :global(.spp-card) {
        background: var(--bg2, #0b0e14);
        border: 1px solid var(--bdr, #1a2030);
        border-radius: 14px;
        width: min(820px, 96vw);
        max-height: 86vh;
        display: flex; flex-direction: column;
        color: var(--txt, #dde3ea);
        box-shadow: 0 28px 80px rgba(0, 0, 0, .7);
    }
    :global(.spp-hdr) {
        position: relative;
        padding: 18px 22px 14px;
        border-bottom: 1px solid var(--bdr, #1a2030);
    }
    :global(.spp-title) {
        margin: 0 0 4px;
        font-size: 17px;
        font-weight: 700;
        color: var(--acc, #10b981);
        letter-spacing: .3px;
    }
    :global(.spp-sub) {
        margin: 0;
        font-size: 11.5px;
        line-height: 1.5;
        color: var(--txt2, #94a3b8);
        max-width: 640px;
    }
    :global(.spp-x) {
        position: absolute; right: 14px; top: 14px;
        background: transparent;
        border: 1px solid var(--bdr2, #222c3a);
        color: var(--txt2, #94a3b8);
        width: 28px; height: 28px;
        border-radius: 7px;
        cursor: pointer;
        font-size: 14px;
    }
    :global(.spp-x:hover) {
        background: rgba(255, 255, 255, .06);
        color: var(--txt, #dde3ea);
    }

    :global(.spp-body) {
        padding: 14px 22px;
        overflow-y: auto;
        flex: 1;
        display: grid;
        gap: 18px;
    }
    :global(.spp-grp-title) {
        margin: 0 0 8px;
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 1.2px;
        text-transform: uppercase;
        color: var(--txt3, #475569);
    }
    :global(.spp-list) {
        list-style: none;
        margin: 0; padding: 0;
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
        gap: 8px;
    }
    :global(.spp-card-item) {
        display: contents;
    }
    :global(.spp-card-btn) {
        text-align: left;
        background: rgba(255, 255, 255, .025);
        border: 1px solid var(--bdr, #1a2030);
        border-radius: 9px;
        padding: 9px 12px;
        color: inherit;
        cursor: pointer;
        font-family: inherit;
        transition: background .12s, border-color .12s;
        display: flex; flex-direction: column; gap: 4px;
    }
    :global(.spp-card-btn:hover) {
        background: rgba(255, 255, 255, .04);
        border-color: var(--bdr2, #222c3a);
    }
    :global(.spp-card-item.active .spp-card-btn) {
        background: rgba(16, 185, 129, .08);
        border-color: rgba(16, 185, 129, .35);
        box-shadow: inset 0 0 0 1px rgba(16, 185, 129, .12);
    }
    :global(.spp-card-head) {
        display: flex; align-items: center;
        justify-content: space-between; gap: 8px;
    }
    :global(.spp-card-name) {
        font-size: 12.5px;
        font-weight: 600;
        color: var(--txt, #dde3ea);
    }
    :global(.spp-active-tag) {
        font-size: 9px;
        font-weight: 700;
        color: var(--acc, #10b981);
        background: rgba(16, 185, 129, .14);
        border: 1px solid rgba(16, 185, 129, .30);
        padding: 1px 6px;
        border-radius: 7px;
        letter-spacing: .4px;
    }
    :global(.spp-card-desc) {
        margin: 0;
        font-size: 11px;
        line-height: 1.45;
        color: var(--txt2, #94a3b8);
    }
    :global(.spp-card-source) {
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 9.5px;
        color: var(--txt3, #475569);
        margin-top: 2px;
    }

    :global(.spp-foot) {
        padding: 12px 22px;
        border-top: 1px solid var(--bdr, #1a2030);
        display: flex;
        align-items: center;
        gap: 12px;
        background: rgba(0, 0, 0, .15);
    }
    :global(.spp-hint) {
        flex: 1;
        font-size: 11px;
        color: var(--txt2, #94a3b8);
    }
    :global(.spp-deactivate) {
        background: rgba(239, 68, 68, .10);
        border: 1px solid rgba(239, 68, 68, .35);
        color: #f87171;
        font: inherit;
        font-size: 11.5px;
        padding: 5px 11px;
        border-radius: 6px;
        cursor: pointer;
    }
    :global(.spp-deactivate:hover) {
        background: rgba(239, 68, 68, .18);
    }
</style>
