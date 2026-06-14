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
    // v1.7.167 — styles live in an external stylesheet (this component's
    // template is all portaled bits-ui primitives, so a scoped <style> had no
    // scopable elements and Svelte warned). Imported = global, which is what
    // the portaled .lcm-content needs anyway.
    import './LucyContextMenu.css';
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

<!-- styles moved to ./LucyContextMenu.css (imported above) -->
