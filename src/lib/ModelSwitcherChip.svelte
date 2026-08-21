<!-- ── ModelSwitcherChip.svelte (v1.4.28) ─────────────────────────────────
     In-chat model switcher chip. Compact pill showing the current model;
     click → opens a floating fuzzy-filterable picker covering every
     provider/model in LLM_GROUPS. Picking writes back via `bind:value`.

     Why a self-contained popover rather than LucyCombobox: the
     bits-ui Combobox primitive always shows an input. A chip should
     collapse to icon + label and only expand the search UI when
     opened. Easier to build that explicitly than to fight the
     primitive's open/close + input visibility wiring.

     Keyboard:
       • / or Ctrl+Shift+M when chip is focused → open
       • ↑ ↓ to navigate the filtered list
       • Enter → pick highlighted
       • Esc → close without changing

     Props:
       value     — bound model id (e.g. "claude-sonnet-4-6::medium")
       isEN      — i18n switch
       compact   — true = icon + 3-char code only; false = icon + label
─────────────────────────────────────────────────────────────────────── -->
<script>
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { tick } from 'svelte';
    import { LLM_GROUPS, getModelDescription, getModelIcon } from '$lib/models.js';

    export let value   = '';
    export let isEN    = false;
    export let compact = false;

    let open = false;
    let query = '';
    let inputEl;
    let highlighted = 0;

    // Flatten the LLM_GROUPS tree into a flat list of pickable items.
    // Each entry carries enough context to render and to filter against.
    $: allItems = LLM_GROUPS.flatMap(g =>
        g.options.map(o => ({
            id:       o.id,
            icon:     o.icon,
            name:     isEN ? o.nameEn : o.nameEs,
            provider: g.provider,
        }))
    );

    // Case-insensitive substring match over name + id + provider. Cheap
    // (~50 entries today) and "haiku" / "sonnet high" / "gpt-4o" all
    // narrow correctly. If perf becomes an issue later, swap in the
    // fzf scorer from $lib/fuzzy-match.
    $: filtered = (() => {
        const q = query.trim().toLowerCase();
        if (!q) return allItems;
        return allItems.filter(it =>
            it.name.toLowerCase().includes(q) ||
            it.id.toLowerCase().includes(q) ||
            it.provider.toLowerCase().includes(q));
    })();

    // Keep highlight inside the filtered range whenever query changes.
    $: if (highlighted >= filtered.length) highlighted = 0;

    $: currentIcon  = getModelIcon(value);
    $: currentLabel = value ? getModelDescription(value, isEN)
                            : ($trad('Elegir modelo'));

    function shortCode(id) {
        if (!id) return '—';
        const head = String(id).split('::')[0].split('/').pop();
        const m = head.match(/[a-z]+/i);
        return (m ? m[0] : head).slice(0, 3).toUpperCase();
    }

    async function openPicker() {
        open = true;
        highlighted = Math.max(0, filtered.findIndex(it => it.id === value));
        await tick();
        inputEl?.focus();
    }
    function closePicker() { open = false; query = ''; }
    function pick(it) {
        value = it.id;
        closePicker();
    }

    function onKey(e) {
        if (!open) return;
        if (e.key === 'Escape')       { e.preventDefault(); closePicker(); }
        else if (e.key === 'ArrowDown') { e.preventDefault(); highlighted = Math.min(filtered.length - 1, highlighted + 1); scrollIntoView(); }
        else if (e.key === 'ArrowUp')   { e.preventDefault(); highlighted = Math.max(0, highlighted - 1); scrollIntoView(); }
        else if (e.key === 'Enter')     { e.preventDefault(); if (filtered[highlighted]) pick(filtered[highlighted]); }
    }

    function scrollIntoView() {
        tick().then(() => {
            const el = document.querySelector(`.msc-item.msc-hl`);
            el?.scrollIntoView({ block: 'nearest' });
        });
    }
</script>

<svelte:window on:keydown={onKey} />

<button class="msc-chip" class:msc-compact={compact}
        on:click={openPicker}
        title={isEN ? `Active model: ${currentLabel}. Click to switch.`
                    : `Modelo activo: ${currentLabel}. Click para cambiar.`}>
    <span class="msc-ico" aria-hidden="true">{currentIcon}</span>
    {#if compact}
        <span class="msc-code">{shortCode(value)}</span>
    {:else}
        <span class="msc-label">{currentLabel}</span>
    {/if}
    <span class="msc-chev" aria-hidden="true">▾</span>
</button>

{#if open}
    <!-- Outside-click backdrop. Click anywhere outside → close. -->
    <div class="msc-backdrop" role="presentation"
         on:click={closePicker}
         on:contextmenu|preventDefault={closePicker}></div>

    <div class="msc-popover" role="listbox"
         aria-label={$trad('Selector de modelo')}>
        <div class="msc-search-row">
            <input bind:this={inputEl}
                   class="msc-search"
                   type="text"
                   bind:value={query}
                   placeholder={$trad('Buscar modelos…')} />
            <span class="msc-count">{filtered.length}</span>
        </div>
        <div class="msc-list">
            {#each filtered as it, i (it.id)}
                <button class="msc-item"
                        class:msc-hl={i === highlighted}
                        class:msc-sel={it.id === value}
                        role="option"
                        aria-selected={it.id === value}
                        on:click={() => pick(it)}
                        on:mouseenter={() => highlighted = i}>
                    <span class="msc-item-ico">{it.icon}</span>
                    <span class="msc-item-name">{it.name}</span>
                    <span class="msc-item-prov">{it.provider}</span>
                </button>
            {/each}
            {#if filtered.length === 0}
                <div class="msc-empty">{$trad('Sin coincidencias')}</div>
            {/if}
        </div>
    </div>
{/if}

<style>
    /* ── Chip ────────────────────────────────────────────────────────── */
    .msc-chip {
        display: inline-flex; align-items: center; gap: 5px;
        padding: 3px 8px;
        border: 1px solid rgba(16, 185, 129, .22);
        background: rgba(16, 185, 129, .06);
        border-radius: 14px;
        font-size: 11px;
        color: var(--acc, #10b981);
        cursor: pointer;
        transition: background .15s, border-color .15s;
        font-family: inherit;
    }
    .msc-chip:hover {
        background: rgba(16, 185, 129, .12);
        border-color: rgba(16, 185, 129, .35);
    }
    .msc-ico   { font-size: 12px; line-height: 1; opacity: .9; }
    .msc-label {
        max-width: 220px;
        overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    }
    .msc-code {
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 9.5px; font-weight: 700; letter-spacing: .4px;
    }
    .msc-chev { font-size: 8px; opacity: .55; }
    .msc-compact { padding: 2px 6px; gap: 4px; font-size: 10.5px; }

    /* ── Popover ─────────────────────────────────────────────────────── */
    .msc-backdrop {
        position: fixed; inset: 0;
        z-index: 9000;
        background: transparent;
    }
    .msc-popover {
        position: fixed;
        bottom: 78px;   /* sits above the composer */
        left: 50%;
        transform: translateX(-50%);
        z-index: 9001;
        width: min(440px, 92vw);
        max-height: 380px;
        display: flex; flex-direction: column;
        background: var(--bg2, #0b0e14);
        border: 1px solid var(--bdr, #1a2030);
        border-radius: 10px;
        box-shadow: 0 18px 50px rgba(0, 0, 0, .6);
        overflow: hidden;
        animation: msc-pop .14s ease-out;
    }
    @keyframes msc-pop {
        from { opacity: 0; transform: translate(-50%, 4px) scale(.97); }
        to   { opacity: 1; transform: translate(-50%, 0) scale(1); }
    }

    .msc-search-row {
        display: flex; align-items: center; gap: 8px;
        padding: 8px 12px;
        border-bottom: 1px solid var(--bdr, #1a2030);
    }
    .msc-search {
        flex: 1;
        background: transparent; border: 0; outline: none;
        color: var(--txt, #dde3ea);
        font-family: inherit; font-size: 12.5px;
    }
    .msc-search::placeholder { color: var(--txt3, #475569); }
    .msc-count {
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 10px;
        color: var(--txt3, #475569);
        background: rgba(255, 255, 255, .04);
        padding: 1px 6px;
        border-radius: 8px;
    }

    .msc-list {
        flex: 1;
        overflow-y: auto;
        padding: 4px;
    }
    .msc-item {
        display: flex; align-items: center; gap: 8px;
        width: 100%;
        padding: 7px 10px;
        background: transparent; border: 0;
        color: var(--txt, #dde3ea);
        font-family: inherit; font-size: 12px;
        text-align: left;
        border-radius: 6px;
        cursor: pointer;
        transition: background .1s;
    }
    .msc-hl  { background: rgba(255, 255, 255, .06); }
    .msc-sel { color: var(--acc, #10b981); }
    .msc-sel::before { content: '✓ '; margin-right: 2px; }
    .msc-item-ico  { font-size: 12px; flex-shrink: 0; width: 16px; text-align: center; }
    .msc-item-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .msc-item-prov {
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 9.5px;
        color: var(--txt3, #475569);
        text-transform: uppercase;
        letter-spacing: .5px;
    }
    .msc-empty {
        padding: 24px 10px;
        text-align: center;
        font-size: 11px;
        color: var(--txt3, #475569);
    }

    @media (prefers-reduced-motion: reduce) {
        .msc-popover { animation: none; }
    }
</style>
