<!-- ── LucyContextMenu.svelte (v1.4.27) ───────────────────────────────────
     bits-ui ContextMenu wrapper. Drops in around any element that should
     respond to right-click with a menu (instead of the browser default
     context menu). Visual identity mirrors LucyDropdown so the family
     stays consistent.

     Why this isn't a duplicate of LucyDropdown:
       - DropdownMenu opens from an explicit button click.
       - ContextMenu opens from a right-click on its trigger area,
         positioned at the cursor (handled by bits-ui).
       - The two primitives have different keyboard semantics — Esc
         dismisses both, but ContextMenu also closes on outside-click
         without needing a backdrop.

     Usage:
         <LucyContextMenu>
             <div slot="trigger">Right-click me</div>
             <button on:click={…}>Rename</button>
             <button on:click={…}>Close</button>
             <hr />
             <button on:click={…} class="ldd-danger">Delete</button>
         </LucyContextMenu>

     The menu content is the default slot; the right-clickable area is
     the `trigger` slot. Direct `<button>` children get the same
     auto-styling treatment as LucyDropdown for consistency.
─────────────────────────────────────────────────────────────────────── -->
<script>
    import { ContextMenu } from 'bits-ui';
</script>

<ContextMenu.Root>
    <ContextMenu.Trigger class="lcm-trigger">
        <slot name="trigger" />
    </ContextMenu.Trigger>
    <ContextMenu.Portal>
        <ContextMenu.Content class="lcm-content">
            <slot />
        </ContextMenu.Content>
    </ContextMenu.Portal>
</ContextMenu.Root>

<style>
    :global(.lcm-trigger) {
        display: contents;          /* no extra wrapper box around the trigger */
    }

    :global(.lcm-content) {
        background: var(--bg2, #0b0e14);
        border: 1px solid var(--bdr, #1a2030);
        border-radius: 9px;
        padding: 5px;
        min-width: 200px;
        z-index: 8500;
        box-shadow: 0 14px 38px rgba(0, 0, 0, .55);
        display: flex;
        flex-direction: column;
        gap: 2px;
        animation: lcm-pop .12s ease-out;
    }
    @keyframes lcm-pop {
        from { opacity: 0; transform: scale(.96) translateY(-2px); }
        to   { opacity: 1; transform: none; }
    }

    /* Auto-style direct <button> children so consumers don't need to
       repeat the same chrome. Consumers can override per-button. */
    :global(.lcm-content > button) {
        display: flex; align-items: center; gap: 8px;
        background: transparent; border: 0;
        color: var(--txt, #dde3ea);
        font-size: 12px;
        text-align: left;
        padding: 7px 10px;
        border-radius: 6px;
        cursor: pointer;
        transition: background .12s;
        font-family: inherit;
    }
    :global(.lcm-content > button:hover),
    :global(.lcm-content > button:focus-visible) {
        background: rgba(255, 255, 255, .06);
        outline: none;
    }
    :global(.lcm-content > button.lcm-danger) { color: #f87171; }
    :global(.lcm-content > button.lcm-danger:hover) { background: rgba(248, 113, 113, .08); }

    /* Visual separator between groups of items. */
    :global(.lcm-content > hr) {
        margin: 4px 6px;
        border: 0;
        border-top: 1px solid var(--bdr, #1a2030);
    }

    @media (prefers-reduced-motion: reduce) {
        :global(.lcm-content) { animation: none; }
    }
</style>
