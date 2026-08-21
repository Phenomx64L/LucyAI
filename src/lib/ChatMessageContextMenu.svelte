<!-- ── ChatMessageContextMenu.svelte (v1.4.15) ─────────────────────────
     Right-click context menu for chat bubbles. Positioned by (x,y) so a
     single mounted instance serves every message (cheaper than a per-
     message bits-ui ContextMenu primitive, which would mount ~N menus).

     Items:
       - Copy as Markdown
       - Copy plain text
       - Save as Memory  (Layer 1)
       - Branch from here (⌥)     [Lucy turns only]
       - Pin / Unpin (·)
       - Replay turn (⏪)          [Lucy turns only]
       - Delete

     Events dispatched up:
       copy-md, copy-txt, save-memory, branch, pin, replay, delete
       close (when user dismisses)

     The actual side-effects (mutating tab.messages, opening replay, etc.)
     live in +page.svelte — this component only emits intent.
─────────────────────────────────────────────────────────────────────── -->
<script>
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { createEventDispatcher, tick } from 'svelte';
    export let open = false;
    export let x    = 0;
    export let y    = 0;
    // `= null` alone makes TS infer the type as literally `null`, so every
    // read below reports "possibly null" even through `?.`. The messages this
    // menu acts on are the untyped tab-message objects from +page.svelte, so
    // `any` is the honest annotation rather than a fictional interface.
    /** @type {any} */
    export let msg  = null;       // the message object the menu acts on

    const dispatch = createEventDispatcher();

    // Adjust position if menu would overflow viewport. Run after open so
    // the actual rendered size is measured; pre-tick we don't yet know
    // the bounding box.
    let menuEl;
    let adjX = 0, adjY = 0;
    $: if (open && menuEl) tick().then(reposition);
    function reposition() {
        if (!menuEl) return;
        const rect = menuEl.getBoundingClientRect();
        const vw = window.innerWidth, vh = window.innerHeight;
        adjX = (x + rect.width  > vw - 8) ? Math.max(8, vw - rect.width  - 8) : x;
        adjY = (y + rect.height > vh - 8) ? Math.max(8, vh - rect.height - 8) : y;
    }

    function close() { open = false; dispatch('close'); }

    function pick(kind) {
        dispatch(kind, { msg });
        close();
    }

    function onKey(e) {
        if (!open) return;
        if (e.key === 'Escape') { e.preventDefault(); close(); }
    }

    $: isLucy = msg && (msg.role === 'lucy' || msg.role === 'streaming');
</script>

<svelte:window on:keydown={onKey} />

{#if open}
    <!-- Invisible backdrop swallows the next click to dismiss. -->
    <div class="ctx-backdrop"
         on:click={close}
         on:contextmenu|preventDefault={close}
         role="presentation"></div>

    <div bind:this={menuEl}
         class="ctx-menu"
         style="left:{adjX || x}px; top:{adjY || y}px"
         role="menu"
         aria-label={$trad('Acciones del mensaje')}>

        <button class="ctx-item" on:click={() => pick('copy-md')}>
            <span class="ctx-ico">⌘</span>
            {$trad('Copiar como Markdown')}
        </button>
        <button class="ctx-item" on:click={() => pick('copy-txt')}>
            <span class="ctx-ico">¶</span>
            {$trad('Copiar texto plano')}
        </button>

        <div class="ctx-sep"></div>

        <button class="ctx-item" on:click={() => pick('save-memory')}>
            <span class="ctx-ico">★</span>
            {$trad('Guardar como memoria')}
        </button>
        <button class="ctx-item" on:click={() => pick('pin')}>
            <span class="ctx-ico">·</span>
            {msg?.pinned
                ? ($trad('Quitar pin del contexto'))
                : ($trad('Fijar al contexto'))}
        </button>

        {#if isLucy}
            <div class="ctx-sep"></div>
            <!-- v1.7.79 — Promote to artifact side panel. Available on
                 every Lucy message; the +page handler checks if the
                 content actually has a long code block / markdown doc
                 and only opens the panel when it does. -->
            <button class="ctx-item" on:click={() => pick('open-as-artifact')}>
                <span class="ctx-ico">◐</span>
                {$trad('Abrir como artefacto')}
            </button>
            <button class="ctx-item" on:click={() => pick('branch')}>
                <span class="ctx-ico">⌥</span>
                {$trad('Bifurcar desde aquí')}
            </button>
            <button class="ctx-item" on:click={() => pick('replay')}>
                <span class="ctx-ico">⏪</span>
                {$trad('Reproducir este turno')}
            </button>
        {/if}

        <div class="ctx-sep"></div>

        <button class="ctx-item ctx-danger" on:click={() => pick('delete')}>
            <span class="ctx-ico">✕</span>
            {$trad('Eliminar mensaje')}
        </button>
    </div>
{/if}

<style>
    .ctx-backdrop {
        position: fixed; inset: 0; z-index: 9000;
        background: transparent;
    }
    .ctx-menu {
        position: fixed;
        z-index: 9001;
        min-width: 220px;
        padding: 5px;
        background: var(--bg2, #0b0e14);
        border: 1px solid var(--bdr, #1a2030);
        border-radius: 9px;
        box-shadow: 0 14px 38px rgba(0,0,0,.55);
        display: flex; flex-direction: column;
        animation: ctx-pop .12s ease-out;
    }
    @keyframes ctx-pop {
        from { opacity: 0; transform: scale(.96) translateY(-2px); }
        to   { opacity: 1; transform: none; }
    }

    .ctx-item {
        display: flex; align-items: center; gap: 9px;
        background: transparent;
        border: 0;
        color: var(--txt, #dde3ea);
        font-size: 12px;
        text-align: left;
        padding: 7px 10px;
        border-radius: 6px;
        cursor: pointer;
        transition: background .12s;
    }
    .ctx-item:hover, .ctx-item:focus-visible {
        background: rgba(255,255,255,.05);
        outline: none;
    }
    .ctx-ico {
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 11px;
        width: 14px;
        text-align: center;
        color: var(--acc, #10b981);
        opacity: .85;
    }
    .ctx-danger { color: #f87171; }
    .ctx-danger .ctx-ico { color: #f87171; }
    .ctx-danger:hover { background: rgba(248,113,113,.08); }

    .ctx-sep {
        height: 1px;
        margin: 4px 6px;
        background: var(--bdr, #1a2030);
    }
</style>
