<!-- ── LucyCombobox.svelte (v1.4.16) ───────────────────────────────────────
     bits-ui Combobox wrapper. Fuzzy-filterable picker for things like the
     /model command palette or any "type to narrow a list" surface that
     today is a hand-rolled <input> + <ul>. Gets us:
       - Built-in keyboard nav (↑↓ Enter Esc Home End)
       - aria-activedescendant + listbox roles
       - Open/close + selection state managed by the primitive

     Items: array of { value: string, label: string, hint?: string }.

     Bind:value gets the selected value (string). Bind:inputValue gets
     the filter text — letting the parent observe it for analytics or
     debounced searches.
─────────────────────────────────────────────────────────────────────── -->
<script>
    import { Combobox } from 'bits-ui';
    export let items       = [];        // [{value,label,hint?}]
    export let value       = '';
    export let inputValue  = '';
    export let placeholder = '';
    export let ariaLabel   = 'Picker';

    // Case-insensitive substring match. Cheap; if perf becomes a problem
    // we can swap in fzf later — the existing fzf-style fuzzy matcher
    // lives in $lib/fuzzy-match.
    $: filtered = inputValue
        ? items.filter(it =>
            (it.label || '').toLowerCase().includes(inputValue.toLowerCase()) ||
            (it.value || '').toLowerCase().includes(inputValue.toLowerCase()))
        : items;
</script>

<Combobox.Root bind:value bind:inputValue type="single">
    <div class="lcb-wrap">
        <Combobox.Input
            class="lcb-input"
            placeholder={placeholder}
            aria-label={ariaLabel} />
        <Combobox.Trigger class="lcb-chev" aria-label={ariaLabel + ' toggle'}>▾</Combobox.Trigger>
    </div>
    <Combobox.Portal>
        <Combobox.Content class="lcb-content" sideOffset={4}>
            {#each filtered as it (it.value)}
                <Combobox.Item value={it.value} label={it.label} class="lcb-item">
                    <span class="lcb-item-label">{it.label}</span>
                    {#if it.hint}<span class="lcb-item-hint">{it.hint}</span>{/if}
                </Combobox.Item>
            {/each}
            {#if filtered.length === 0}
                <div class="lcb-empty">—</div>
            {/if}
        </Combobox.Content>
    </Combobox.Portal>
</Combobox.Root>

<style>
    .lcb-wrap{
        position:relative; display:inline-flex; align-items:center;
        border:1px solid var(--bdr,#1a2030); border-radius:7px;
        background:var(--bg3,#0f1520);
    }
    :global(.lcb-input){
        background:transparent; color:var(--txt,#dde3ea);
        border:0; outline:none;
        padding:6px 10px; font-size:12.5px;
        min-width:180px;
    }
    :global(.lcb-chev){
        background:transparent; border:0; color:var(--txt3,#475569);
        padding:0 8px 0 4px; cursor:pointer; font-size:12px;
    }
    :global(.lcb-content){
        background:var(--bg2,#0b0e14); border:1px solid var(--bdr,#1a2030);
        border-radius:8px; padding:4px;
        min-width:240px; max-height:300px; overflow-y:auto;
        z-index:8500; box-shadow:0 14px 38px rgba(0,0,0,.55);
        animation:lcb-pop .12s ease-out;
    }
    :global(.lcb-item){
        display:flex; align-items:baseline; gap:10px;
        padding:6px 10px; border-radius:5px;
        font-size:12px; color:var(--txt,#dde3ea);
        cursor:pointer;
    }
    :global(.lcb-item[data-highlighted]){
        background:rgba(255,255,255,.06);
    }
    :global(.lcb-item[data-selected]){
        color:var(--acc,#10b981);
    }
    :global(.lcb-item-hint){
        font-family:var(--mono, ui-monospace, monospace);
        font-size:10.5px; color:var(--txt3,#475569);
        margin-left:auto;
    }
    .lcb-empty{
        padding:10px; text-align:center;
        color:var(--txt3,#475569); font-size:11px;
    }
    @keyframes lcb-pop{
        from{opacity:0; transform:translateY(-2px) scale(.97);}
        to  {opacity:1; transform:none;}
    }
    @media (prefers-reduced-motion: reduce){
        :global(.lcb-content){animation:none;}
    }
</style>
