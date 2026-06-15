<!-- ── CommandPalette.svelte ─────────────────────────────────────────────────
     Ctrl+P command palette overlay.
     Props  : allItems [{ icon, label, cat, action, hint? }] — unfiltered list
              show (bindable) — controls visibility
     Query filtering and keyboard navigation are handled internally.
──────────────────────────────────────────────────────────────────────────── -->
<script>
    import { tick } from 'svelte';
    import { focusTrap } from '$lib/actions';
    // v1.4.12 — fzf-style fuzzy matcher upgrades the substring filter
    // below. Same look, much better ranking (boundary > mid-word,
    // contiguous > scattered, consecutive > sparse).
    import { fuzzyFilter } from '$lib/fuzzy-match';

    /**
     * Full unfiltered list of palette items.
     * Each item: { icon: string, label: string, cat: string, action: () => void, hint?: string }
     */
    export let allItems = [];
    /** Controls visibility; supports bind:show */
    export let show = false;
    export let isEN = false;

    let query = '';
    let idx   = 0;

    // v1.4.12 — fuzzy match over `label cat hint` joined. The fzf-style
    // scorer ranks by boundary matches + contiguous runs + consecutive
    // chars (see fuzzy-match.ts). Empty query returns the full list
    // unchanged (preserves caller's intended order, e.g. recent first).
    $: filtered = fuzzyFilter(
        allItems,
        query.trim(),
        (i) => `${i.label} ${i.cat} ${i.hint || ''}`,
    );

    // Reset state and focus the search input each time the palette opens
    $: if (show) {
        query = '';
        idx   = 0;
        tick().then(() => { const el = document.getElementById('cp-input'); if (el) el.focus(); });
    }

    function runItem(item) {
        item.action();  // action already closes via showPalette binding in parent scope
        show = false;   // belt-and-suspenders: ensure closed even if action omits it
    }

    function close() { show = false; }
</script>

{#if show}
<!-- Backdrop -->
<div class="cp-backdrop" role="button" tabindex="-1"
  on:click={close} on:keydown={(e) => e.key === 'Escape' && close()}>
</div>

<!-- Panel -->
<div class="cp-wrap" role="dialog" aria-label="Command palette" use:focusTrap>

  <div class="cp-search">
    <span class="cp-ico">⌕</span>
    <input id="cp-input" class="cp-input"
      placeholder={isEN ? 'Search commands, actions, hosts...' : 'Buscar comandos, acciones, hosts...'}
      bind:value={query}
      on:input={() => idx = 0}
      on:keydown={(e) => {
        if      (e.key === 'ArrowDown') { e.preventDefault(); idx = Math.min(idx + 1, filtered.length - 1); }
        else if (e.key === 'ArrowUp')   { e.preventDefault(); idx = Math.max(idx - 1, 0); }
        else if (e.key === 'Enter')     { e.preventDefault(); if (filtered[idx]) runItem(filtered[idx]); }
        else if (e.key === 'Escape')    { close(); }
      }}>
    <kbd class="cp-esc">Esc</kbd>
  </div>

  <div class="cp-list">
    {#each filtered.slice(0, 12) as item, i}
      <button class="cp-item" class:cp-active={i === idx}
        on:click={() => runItem(item)}
        on:mouseenter={() => idx = i}>
        <span class="cp-item-ico">{item.icon}</span>
        <span class="cp-item-lbl">{item.label}</span>
        <span class="cp-item-cat">{item.cat}</span>
        {#if item.hint}<kbd class="cp-item-hint">{item.hint}</kbd>{/if}
      </button>
    {/each}
    {#if filtered.length === 0}
      <div class="cp-empty">{isEN ? 'No results for' : 'Sin resultados para'} "{query}"</div>
    {/if}
  </div>

  <div class="cp-footer">
    <span>↑↓ {isEN ? 'navigate' : 'navegar'}</span>
    <span>↵ {isEN ? 'execute' : 'ejecutar'}</span>
    <span>Ctrl+P {isEN ? 'close' : 'cerrar'}</span>
  </div>

</div>
{/if}

<style>
  /* ── Backdrop ───────────────────────────────────────────────────────────── */
  .cp-backdrop {
    position: fixed; inset: 0; background: rgba(0,0,0,.6); z-index: var(--z-palette, 3000);
  }

  /* ── Panel ──────────────────────────────────────────────────────────────── */
  .cp-wrap {
    position: fixed; top: 80px; left: 50%; transform: translateX(-50%);
    width: 580px; max-width: calc(100vw - 48px);
    background: #0b0e14; border: 1px solid #1e2a3a; border-radius: 12px;
    z-index: calc(var(--z-palette, 3000) + 1); overflow: hidden;
    box-shadow: 0 24px 60px rgba(0,0,0,.6);
  }

  /* ── Search bar ─────────────────────────────────────────────────────────── */
  .cp-search { display: flex; align-items: center; gap: 10px; padding: 12px 16px; border-bottom: 1px solid var(--bdr, #1a2030); }
  .cp-ico    { color: var(--txt3); font-size: 16px; flex-shrink: 0; }
  .cp-input  { flex: 1; background: transparent; border: none; color: white; font-size: 14px; font-family: inherit; outline: none; }
  .cp-input::placeholder { color: #2a3a4a; }
  .cp-esc    {
    background: rgba(255,255,255,.06); border: 1px solid var(--bdr, #1a2030);
    border-radius: 4px; color: var(--txt3); font-size: 10px; padding: 2px 6px;
    font-family: inherit;
  }

  /* ── Item list ──────────────────────────────────────────────────────────── */
  .cp-list  { max-height: 380px; overflow-y: auto; }
  .cp-item  {
    display: flex; align-items: center; gap: 10px;
    width: 100%; padding: 9px 16px;
    background: none; border: none; color: var(--txt, #dde3ea);
    cursor: pointer; font-size: 12px; font-family: inherit;
    text-align: left; transition: background .1s;
  }
  .cp-item:hover, .cp-active { background: rgba(16,185,129,.05); }
  .cp-item-ico  { font-size: 14px; width: 20px; text-align: center; flex-shrink: 0; }
  .cp-item-lbl  { flex: 1; }
  .cp-item-cat  { color: var(--txt3); font-size: 10px; flex-shrink: 0; padding-right: 6px; }
  .cp-item-hint {
    background: rgba(255,255,255,.06); border: 1px solid #1e2a3a;
    border-radius: 3px; color: #4a6a8a; font-size: 10px;
    padding: 1px 5px; font-family: inherit;
  }
  .cp-empty { padding: 16px; text-align: center; color: var(--txt3); font-size: 12px; }

  /* ── Footer ─────────────────────────────────────────────────────────────── */
  .cp-footer {
    display: flex; gap: 16px; padding: 8px 16px;
    border-top: 1px solid var(--bdr, #1a2030);
    font-size: 10px; color: var(--txt3);
  }

  /* ── Light theme overrides ──────────────────────────────────────────────── */
  :global(:root.light .cp-wrap)   { background: #fff; border-color: var(--bdr2); box-shadow: 0 20px 50px rgba(0,0,0,.18); }
  :global(:root.light .cp-search) { border-bottom-color: var(--bdr); }
  :global(:root.light .cp-input)  { color: var(--txt); }
  :global(:root.light .cp-input::placeholder) { color: var(--txt3); }
  :global(:root.light .cp-ico)    { color: var(--txt3); }
</style>
