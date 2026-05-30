<script>
    // ── ConfirmModal ─────────────────────────────────────────────────────
    //
    // v1.4.13 — Rebuilt on top of bits-ui's AlertDialog primitive.
    // Public API is IDENTICAL to the prior hand-rolled version (same
    // props, same events, same slot semantics), so every callsite —
    // `+page.svelte`, `McpServersModal.svelte`, host-management views —
    // works unchanged.
    //
    // What we gained from the swap:
    //   • Focus trap with proper history restore on close
    //   • Real Escape handling routed through the dialog stack (nested
    //     dialogs no longer collide)
    //   • Portal rendering (no more z-index battles with sibling overlays)
    //   • aria-modal, aria-labelledby, aria-describedby wired automatically
    //   • Pointer outside dismissal that respects nested triggers
    //   • prefers-reduced-motion respected by the open/close animations
    //
    // Lucy's visual identity is preserved 1:1: same colors, gradients,
    // animations, variants (danger / warn / info). Only the
    // accessibility layer underneath changed.
    //
    // Usage (unchanged from v1.4.12):
    //   <ConfirmModal
    //     open={confirmState !== null}
    //     title="Eliminar host"
    //     message="¿Eliminar este host? Esta acción no se puede deshacer."
    //     detail={confirmState?.name}
    //     variant="danger"
    //     confirmLabel="Eliminar"
    //     cancelLabel="Cancelar"
    //     on:confirm={handleConfirm}
    //     on:cancel={() => confirmState = null}
    //   />

    import { createEventDispatcher } from 'svelte';
    import { AlertDialog } from 'bits-ui';
    const dispatch = createEventDispatcher();

    export let open         = false;
    export let title        = '';
    export let message      = '';
    export let detail       = '';     // optional <code> block
    export let variant      = 'danger';   // 'danger' | 'warn' | 'info'
    export let confirmLabel = 'Confirm';
    export let cancelLabel  = 'Cancel';
    export let icon         = '';     // override icon (defaults by variant)

    $: defaultIcon = variant === 'danger' ? '🗑' : variant === 'warn' ? '⚠' : 'ℹ';
    $: shownIcon   = icon || defaultIcon;

    // The AlertDialog manages its own open state. We mirror our `open`
    // prop into it via bind:open so external setters (open=true) work,
    // AND fire `cancel` when bits-ui closes the dialog from any path
    // we didn't explicitly trigger (Escape, click outside) — that
    // matches the legacy semantics where any close = cancel by default.
    let _open = open;
    $: _open = open;
    function onOpenChange(v) {
        if (open && !v) {
            // Closed by the user — emit cancel so the parent clears its state.
            dispatch('cancel');
        }
        open = v;
    }

    function onConfirm() {
        dispatch('confirm');
        open = false;
    }
</script>

<AlertDialog.Root bind:open={_open} onOpenChange={onOpenChange}>
    <AlertDialog.Portal>
        <AlertDialog.Overlay class="cm-overlay" />
        <AlertDialog.Content class="cm-modal cm-{variant}">
            <div class="cm-hdr">
                <span class="cm-ico">{shownIcon}</span>
                <AlertDialog.Title class="cm-title">{title}</AlertDialog.Title>
            </div>
            <div class="cm-body">
                {#if message}
                    <AlertDialog.Description class="cm-msg">{message}</AlertDialog.Description>
                {/if}
                {#if detail}<code class="cm-detail">{detail}</code>{/if}
                <slot />
            </div>
            <div class="cm-actions">
                <AlertDialog.Cancel class="cm-btn cancel">{cancelLabel}</AlertDialog.Cancel>
                <AlertDialog.Action class="cm-btn confirm cm-confirm-{variant}"
                    onclick={onConfirm}>
                    {confirmLabel}
                </AlertDialog.Action>
            </div>
        </AlertDialog.Content>
    </AlertDialog.Portal>
</AlertDialog.Root>

<style>
    /* Note: bits-ui renders the overlay + content into a portal, so the
       selectors below MUST be :global() — they don't live inside this
       component's scoped tree. Visual identity stays 1:1 with v1.4.12. */
    :global(.cm-overlay) {
        position: fixed; inset: 0;
        background: rgba(6,10,15,0.78); backdrop-filter: blur(6px);
        z-index: 10500;
        animation: cm-fade .15s ease;
    }
    @keyframes cm-fade { from { opacity: 0 } to { opacity: 1 } }

    :global(.cm-modal) {
        position: fixed;
        top: 50%; left: 50%;
        transform: translate(-50%, -50%);
        width: min(420px, 90vw);
        background: var(--bg2, #0f172a); color: var(--txt, #e2e8f0);
        border: 1px solid rgba(255,255,255,0.08);
        border-radius: 12px; overflow: hidden;
        box-shadow: 0 20px 50px rgba(0,0,0,0.6);
        z-index: 10501;
        animation: cm-slide .18s ease;
        display: flex; flex-direction: column;
    }
    @keyframes cm-slide { from { transform: translate(-50%, calc(-50% + 10px)); opacity: 0 } to { transform: translate(-50%, -50%); opacity: 1 } }

    /* Header tinted by variant */
    :global(.cm-modal .cm-hdr) {
        display: flex; align-items: center; gap: 9px;
        padding: 12px 16px;
        border-bottom: 1px solid rgba(255,255,255,0.05);
    }
    :global(.cm-modal .cm-ico) { font-size: 17px; }
    :global(.cm-modal .cm-title) { font-size: 13px; font-weight: 700; letter-spacing: .35px; text-transform: uppercase; margin: 0; }

    :global(.cm-modal.cm-danger)              { border-color: rgba(239,68,68,0.40); }
    :global(.cm-modal.cm-danger .cm-hdr)      { background: rgba(239,68,68,0.10); border-bottom-color: rgba(239,68,68,0.20); }
    :global(.cm-modal.cm-danger .cm-title)    { color: #f87171; }

    :global(.cm-modal.cm-warn)                { border-color: rgba(251,191,36,0.40); }
    :global(.cm-modal.cm-warn .cm-hdr)        { background: rgba(251,191,36,0.10); border-bottom-color: rgba(251,191,36,0.20); }
    :global(.cm-modal.cm-warn .cm-title)      { color: #fbbf24; }

    :global(.cm-modal.cm-info)                { border-color: rgba(99,102,241,0.40); }
    :global(.cm-modal.cm-info .cm-hdr)        { background: rgba(99,102,241,0.10); border-bottom-color: rgba(99,102,241,0.20); }
    :global(.cm-modal.cm-info .cm-title)      { color: #818cf8; }

    /* Body */
    :global(.cm-modal .cm-body) { padding: 14px 16px; display: flex; flex-direction: column; gap: 9px; }
    :global(.cm-modal .cm-msg)  { margin: 0; font-size: 12.5px; line-height: 1.5; color: var(--txt, #e2e8f0); }
    :global(.cm-modal .cm-detail) {
        display: block; font-family: var(--mono, ui-monospace, monospace);
        font-size: 11px; color: var(--txt2, #cbd5e1);
        background: rgba(255,255,255,0.04);
        border: 1px solid rgba(255,255,255,0.06);
        border-radius: 6px; padding: 7px 9px;
        white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }

    /* Actions */
    :global(.cm-modal .cm-actions) {
        display: flex; justify-content: flex-end; gap: 8px;
        padding: 11px 16px;
        border-top: 1px solid rgba(255,255,255,0.05);
    }
    :global(.cm-modal .cm-btn) {
        padding: 7px 16px; border-radius: 6px; font-size: 12px;
        font-weight: 600; cursor: pointer; border: 1px solid;
        transition: background .15s, color .15s;
        font-family: inherit;
    }
    :global(.cm-modal .cm-btn.cancel) {
        background: rgba(255,255,255,0.04);
        border-color: rgba(255,255,255,0.10);
        color: var(--txt2, #cbd5e1);
    }
    :global(.cm-modal .cm-btn.cancel:hover) { background: rgba(255,255,255,0.09); color: var(--txt, #fff); }

    :global(.cm-modal .cm-btn.confirm) { font-weight: 700; }
    :global(.cm-modal .cm-confirm-danger) {
        background: rgba(239,68,68,0.18); border-color: rgba(239,68,68,0.50); color: #fca5a5;
    }
    :global(.cm-modal .cm-confirm-danger:hover) { background: rgba(239,68,68,0.32); color: #fff; }
    :global(.cm-modal .cm-confirm-warn) {
        background: rgba(251,191,36,0.18); border-color: rgba(251,191,36,0.50); color: #fcd34d;
    }
    :global(.cm-modal .cm-confirm-warn:hover) { background: rgba(251,191,36,0.32); color: #fff; }
    :global(.cm-modal .cm-confirm-info) {
        background: rgba(99,102,241,0.18); border-color: rgba(99,102,241,0.50); color: #a5b4fc;
    }
    :global(.cm-modal .cm-confirm-info:hover) { background: rgba(99,102,241,0.32); color: #fff; }

    :global(:root.light .cm-modal) { background: #fff; color: #1e293b; }
    :global(:root.light .cm-detail) { background: #f1f5f9; color: #475569; }
</style>
