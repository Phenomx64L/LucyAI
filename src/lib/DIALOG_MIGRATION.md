# Migration guide — hand-rolled modals → bits-ui Dialog primitives

Status as of **v1.4.13**:

| Modal | Migrated | Notes |
|---|---|---|
| `ConfirmModal` | ✅ | Uses `AlertDialog` (confirmation semantics) |
| `McpServersModal` | ✅ | Uses `Dialog` (general modal) |
| `Settings` (inline in `+page.svelte`) | ⏳ | Pending — tabbed Settings with own state |
| `ProviderConfigModal` | ⏳ | Pending |
| `HistoryModal` | ⏳ | Pending |
| `ProfileModal` | ⏳ | Pending |
| `SkillsManagerModal` | retired | v1.4.1 — do not migrate |
| `PromptModal` | ⏳ | Pending |
| `KeyringModal` | ⏳ | Pending |
| `ShellRecordingPlayer` | ⏳ | Pending |

## Why migrate

The hand-rolled `<div role="dialog">` approach works visually but is missing:

- **Focus trap** with history restore — keyboard users get lost after close
- **Proper Escape handling** routed through a dialog stack — nested modals collide
- **Portal rendering** — z-index battles with sibling overlays
- **Pointer-outside dismissal** that respects nested triggers
- **ARIA wiring** — `aria-modal`, `aria-labelledby`, `aria-describedby` need to be manually added everywhere

bits-ui's `Dialog` / `AlertDialog` primitives give all of those for free with zero visual cost (we still own the CSS).

## When to use `AlertDialog` vs `Dialog`

- **`AlertDialog`** — confirmation-style modal where the user MUST choose
  (confirm / cancel). Cannot be dismissed by clicking outside; only by
  buttons. Use for: deletes, destructive actions, settings that need
  explicit acknowledgement.
- **`Dialog`** — general modal with a close button or outside-click
  dismissal. Use for: settings, viewers, forms, anything where "cancel"
  is implicit.

## The migration pattern (5 steps)

Pick a hand-rolled modal whose contents are basically:

```svelte
{#if isOpen}
  <div class="modal-backdrop" on:click|self={close}>
    <div class="modal-card" role="dialog" aria-modal="true">
      ... contents ...
    </div>
  </div>
{/if}
```

### Step 1 — Add the import

```svelte
import { Dialog } from 'bits-ui';
// OR for confirmations:
import { AlertDialog } from 'bits-ui';
```

### Step 2 — Replace the wrapper

```svelte
<Dialog.Root open={isOpen} onOpenChange={onOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="modal-backdrop" />
    <Dialog.Content class="modal-card-wrap">
      <div class="modal-card">
        ... contents UNCHANGED ...
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
```

### Step 3 — Wire `onOpenChange` to forward the close event

```svelte
function onOpenChange(v) {
  // When bits-ui closes the dialog from Escape / outside click /
  // explicit Close button, forward the legacy close event so the
  // parent component clears its state.
  if (isOpen && !v) dispatch('close');
}
```

### Step 4 — Remove the legacy close machinery

Delete:
- `onMount` listener that called `document.addEventListener('keydown', onKey)` for Escape
- The `on:click|self={close}` on the backdrop
- Any manual focus-trap action

bits-ui handles all of it.

### Step 5 — Adapt the CSS

The portal renders OUTSIDE the component's scoped CSS. Wrap the
selectors that target portal-rendered nodes in `:global()`:

```css
:global(.modal-backdrop) {
    position: fixed; inset: 0;
    background: rgba(2, 6, 12, 0.78);
    backdrop-filter: blur(4px);
    z-index: 5000;
}
:global(.modal-card-wrap) {
    position: fixed; inset: 0;
    z-index: 5001;
    display: flex; align-items: center; justify-content: center;
    padding: 24px;
    pointer-events: none;
}
:global(.modal-card-wrap > .modal-card) { pointer-events: auto; }
```

The `pointer-events: none` on the wrap + `auto` on the card lets the
overlay receive outside-click while the card itself stays interactive.

## What to keep

- ALL existing CSS for the card body, header, footer, content.
- All props, slots, and events (`on:close`, etc.).
- The toast/flash machinery if present.

## What changes visually

Nothing. The migration is API-internal. Compare ConfirmModal before/after
side-by-side — same gradient on the header, same button styles, same
animations.

## What changes for the user

- **Tab cycles** correctly through interactive elements inside the modal
- **Escape** closes the modal even if focus is on a non-cancel button
- **Shift+Tab** at the first focusable element wraps to the last
- **Focus returns** to the element that opened the modal on close
- **Outside click** dismisses (for `Dialog`; not for `AlertDialog`)

## Risks

- **Portal rendering position**: bits-ui appends to `<body>` by default.
  If the modal uses CSS that targets a specific parent selector (e.g.
  a custom theme class set on `.app-root`), you may need a `<Portal to=".app-root">` override.
- **z-index conflicts**: the portal escapes the local stacking context.
  Re-check toast / command-palette / context-menu z-indexes if a
  migrated modal now appears behind them.
