<!-- ── LucyDropdown.svelte (v1.4.16) ───────────────────────────────────────
     bits-ui DropdownMenu wrapper. Replaces hand-rolled <div class="popover">
     overflow menus that lack focus trap, arrow-key navigation and Esc
     handling. Stays opinion-free about content: the consumer fills the
     `menu` slot with whatever they need (LucyDropdown.Item helpers below
     would be over-engineering for now — the wrapper alone is the win).

     Usage:
         <LucyDropdown label="≡" ariaLabel="Overflow">
             <button on:click={…}>Action A</button>
             <button on:click={…}>Action B</button>
         </LucyDropdown>
─────────────────────────────────────────────────────────────────────── -->
<script>
    import { DropdownMenu } from 'bits-ui';
    export let label     = '≡';
    export let ariaLabel = 'Menu';
    export let align     = 'end';   // 'start' | 'center' | 'end'
    export let triggerClass = 'ldd-trigger';
</script>

<DropdownMenu.Root>
    <DropdownMenu.Trigger class={triggerClass} aria-label={ariaLabel}>
        {label}
    </DropdownMenu.Trigger>
    <DropdownMenu.Portal>
        <DropdownMenu.Content class="ldd-content" sideOffset={6} align={align}>
            <slot />
        </DropdownMenu.Content>
    </DropdownMenu.Portal>
</DropdownMenu.Root>

<style>
    :global(.ldd-trigger){
        background: transparent;
        border: 1px solid var(--bdr, #1a2030);
        color: var(--txt, #dde3ea);
        border-radius: 6px;
        padding: 3px 8px;
        font-size: 12px;
        cursor: pointer;
        transition: background .12s;
    }
    :global(.ldd-trigger:hover){ background: rgba(255,255,255,.06); }
    :global(.ldd-trigger[data-state="open"]){ background: rgba(255,255,255,.10); }

    :global(.ldd-content){
        background: var(--bg2, #0b0e14);
        border: 1px solid var(--bdr, #1a2030);
        border-radius: 9px;
        padding: 5px;
        min-width: 200px;
        z-index: 8500;
        box-shadow: 0 14px 38px rgba(0,0,0,.55);
        display: flex;
        flex-direction: column;
        gap: 2px;
        animation: ldd-pop .12s ease-out;
    }
    @keyframes ldd-pop {
        from { opacity: 0; transform: scale(.96) translateY(-2px); }
        to   { opacity: 1; transform: none; }
    }

    /* Children — auto-style any direct <button> in the menu for visual
       consistency. Consumers can override by passing their own classes. */
    :global(.ldd-content > button){
        display: flex; align-items: center; gap: 8px;
        background: transparent; border: 0;
        color: var(--txt, #dde3ea);
        font-size: 12px;
        text-align: left;
        padding: 7px 10px;
        border-radius: 6px;
        cursor: pointer;
        transition: background .12s;
    }
    :global(.ldd-content > button:hover),
    :global(.ldd-content > button:focus-visible){
        background: rgba(255,255,255,.06);
        outline: none;
    }
    @media (prefers-reduced-motion: reduce){
        :global(.ldd-content){ animation: none; }
    }
</style>
