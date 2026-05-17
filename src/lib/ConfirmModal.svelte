<script>
    // ── ConfirmModal ─────────────────────────────────────────────────────
    // Drop-in replacement for window.confirm() with proper Lucy styling.
    //
    // Usage:
    //   <ConfirmModal
    //     open={confirmState !== null}
    //     title="Eliminar host"
    //     message="¿Eliminar este host? Esta acción no se puede deshacer."
    //     detail={confirmState?.name}        <!-- optional code block -->
    //     variant="danger"                   <!-- 'danger' | 'warn' | 'info' -->
    //     confirmLabel="Eliminar"
    //     cancelLabel="Cancelar"
    //     on:confirm={handleConfirm}
    //     on:cancel={() => confirmState = null}
    //   />

    import { createEventDispatcher } from 'svelte';
    const dispatch = createEventDispatcher();

    export let open         = false;
    export let title        = '';
    export let message      = '';
    export let detail       = '';     // optional <code> block (e.g. filename, host)
    export let variant      = 'danger';   // 'danger' | 'warn' | 'info'
    export let confirmLabel = 'Confirm';
    export let cancelLabel  = 'Cancel';
    export let icon         = '';     // override icon (defaults by variant)

    $: defaultIcon = variant === 'danger' ? '🗑' : variant === 'warn' ? '⚠' : 'ℹ';
    $: shownIcon   = icon || defaultIcon;

    function onConfirm() { dispatch('confirm'); }
    function onCancel()  { dispatch('cancel');  }
    function onKey(e) {
        if (!open) return;
        if (e.key === 'Escape') { e.preventDefault(); onCancel(); }
        if (e.key === 'Enter')  { e.preventDefault(); onConfirm(); }
    }
</script>

<svelte:window on:keydown={onKey} />

{#if open}
<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="cm-overlay" on:click|self={onCancel}>
    <div class="cm-modal cm-{variant}" role="dialog" aria-modal="true">
        <div class="cm-hdr">
            <span class="cm-ico">{shownIcon}</span>
            <span class="cm-title">{title}</span>
        </div>
        <div class="cm-body">
            {#if message}<p class="cm-msg">{message}</p>{/if}
            {#if detail}<code class="cm-detail">{detail}</code>{/if}
            <slot />
        </div>
        <div class="cm-actions">
            <button class="cm-btn cancel" on:click={onCancel}>{cancelLabel}</button>
            <button class="cm-btn confirm cm-confirm-{variant}" on:click={onConfirm}>
                {confirmLabel}
            </button>
        </div>
    </div>
</div>
{/if}

<style>
    .cm-overlay {
        position: fixed; inset: 0;
        background: rgba(6,10,15,0.78); backdrop-filter: blur(6px);
        display: flex; align-items: center; justify-content: center;
        z-index: 10500; animation: cm-fade .15s ease;
    }
    @keyframes cm-fade { from { opacity: 0 } to { opacity: 1 } }

    .cm-modal {
        width: min(420px, 90vw);
        background: var(--bg2, #0f172a); color: var(--txt, #e2e8f0);
        border: 1px solid rgba(255,255,255,0.08);
        border-radius: 12px; overflow: hidden;
        box-shadow: 0 20px 50px rgba(0,0,0,0.6);
        animation: cm-slide .18s ease;
    }
    @keyframes cm-slide { from { transform: translateY(10px); opacity: 0 } to { transform: translateY(0); opacity: 1 } }

    /* Header tinted by variant */
    .cm-hdr {
        display: flex; align-items: center; gap: 9px;
        padding: 12px 16px;
        border-bottom: 1px solid rgba(255,255,255,0.05);
    }
    .cm-ico { font-size: 17px; }
    .cm-title { font-size: 13px; font-weight: 700; letter-spacing: .35px; text-transform: uppercase; }

    .cm-danger        { border-color: rgba(239,68,68,0.40); }
    .cm-danger .cm-hdr{ background: rgba(239,68,68,0.10); border-bottom-color: rgba(239,68,68,0.20); }
    .cm-danger .cm-title { color: #f87171; }

    .cm-warn        { border-color: rgba(251,191,36,0.40); }
    .cm-warn .cm-hdr{ background: rgba(251,191,36,0.10); border-bottom-color: rgba(251,191,36,0.20); }
    .cm-warn .cm-title { color: #fbbf24; }

    .cm-info        { border-color: rgba(99,102,241,0.40); }
    .cm-info .cm-hdr{ background: rgba(99,102,241,0.10); border-bottom-color: rgba(99,102,241,0.20); }
    .cm-info .cm-title { color: #818cf8; }

    /* Body */
    .cm-body { padding: 14px 16px; display: flex; flex-direction: column; gap: 9px; }
    .cm-msg  { margin: 0; font-size: 12.5px; line-height: 1.5; color: var(--txt, #e2e8f0); }
    .cm-detail {
        display: block; font-family: var(--mono, ui-monospace, monospace);
        font-size: 11px; color: var(--txt2, #cbd5e1);
        background: rgba(255,255,255,0.04);
        border: 1px solid rgba(255,255,255,0.06);
        border-radius: 6px; padding: 7px 9px;
        white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }

    /* Actions */
    .cm-actions {
        display: flex; justify-content: flex-end; gap: 8px;
        padding: 11px 16px;
        border-top: 1px solid rgba(255,255,255,0.05);
    }
    .cm-btn {
        padding: 7px 16px; border-radius: 6px; font-size: 12px;
        font-weight: 600; cursor: pointer; border: 1px solid;
        transition: background .15s, color .15s;
    }
    .cm-btn.cancel {
        background: rgba(255,255,255,0.04);
        border-color: rgba(255,255,255,0.10);
        color: var(--txt2, #cbd5e1);
    }
    .cm-btn.cancel:hover { background: rgba(255,255,255,0.09); color: var(--txt, #fff); }

    .cm-btn.confirm { font-weight: 700; }
    .cm-confirm-danger {
        background: rgba(239,68,68,0.18); border-color: rgba(239,68,68,0.50); color: #fca5a5;
    }
    .cm-confirm-danger:hover { background: rgba(239,68,68,0.32); color: #fff; }
    .cm-confirm-warn {
        background: rgba(251,191,36,0.18); border-color: rgba(251,191,36,0.50); color: #fcd34d;
    }
    .cm-confirm-warn:hover { background: rgba(251,191,36,0.32); color: #fff; }
    .cm-confirm-info {
        background: rgba(99,102,241,0.18); border-color: rgba(99,102,241,0.50); color: #a5b4fc;
    }
    .cm-confirm-info:hover { background: rgba(99,102,241,0.32); color: #fff; }

    :global(:root.light) .cm-modal { background: #fff; color: #1e293b; }
    :global(:root.light) .cm-detail { background: #f1f5f9; color: #475569; }
</style>
