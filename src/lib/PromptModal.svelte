<script>
    // ── PromptModal ──────────────────────────────────────────────────────
    // In-app replacement for window.prompt(). The host component owns the
    // open state and reads the value from the `submit` event.
    //
    // Usage:
    //   <PromptModal
    //     open={showPrompt}
    //     title="Preset name"
    //     placeholder="My preset"
    //     defaultValue=""
    //     multiline={false}
    //     on:submit={(e) => handleValue(e.detail)}
    //     on:cancel={() => showPrompt = false}
    //   />

    import { createEventDispatcher, tick } from 'svelte';
    const dispatch = createEventDispatcher();

    export let open         = false;
    export let title        = '';
    export let label        = '';     // optional secondary label above input
    export let placeholder  = '';
    export let defaultValue = '';
    export let confirmLabel = 'OK';
    export let cancelLabel  = 'Cancel';
    export let multiline    = false;
    export let required     = true;   // when true, empty input disables submit

    let value = '';
    let inputEl;

    // Reset value when the modal opens
    $: if (open) { value = defaultValue; tick().then(() => inputEl?.focus()); }

    $: canSubmit = required ? value.trim().length > 0 : true;

    function onSubmit() {
        if (!canSubmit) return;
        dispatch('submit', value);
    }
    function onCancel() { dispatch('cancel'); }
    function onKey(e) {
        if (!open) return;
        if (e.key === 'Escape') { e.preventDefault(); onCancel(); }
        if (e.key === 'Enter' && !multiline && canSubmit) { e.preventDefault(); onSubmit(); }
    }
</script>

<svelte:window on:keydown={onKey} />

{#if open}
<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="pm-overlay" on:click|self={onCancel}>
    <div class="pm-modal" role="dialog" aria-modal="true">
        <div class="pm-hdr"><span class="pm-title">{title}</span></div>
        <div class="pm-body">
            {#if label}<label class="pm-label" for="pm-input-field">{label}</label>{/if}
            {#if multiline}
                <textarea
                    id="pm-input-field"
                    bind:this={inputEl}
                    bind:value
                    {placeholder}
                    class="pm-input pm-textarea"
                    rows="4"
                ></textarea>
            {:else}
                <input
                    id="pm-input-field"
                    bind:this={inputEl}
                    bind:value
                    {placeholder}
                    class="pm-input"
                    type="text"
                />
            {/if}
        </div>
        <div class="pm-actions">
            <button class="pm-btn cancel" on:click={onCancel}>{cancelLabel}</button>
            <button class="pm-btn confirm" on:click={onSubmit} disabled={!canSubmit}>
                {confirmLabel}
            </button>
        </div>
    </div>
</div>
{/if}

<style>
    .pm-overlay {
        position: fixed; inset: 0;
        background: rgba(6,10,15,0.78); backdrop-filter: blur(6px);
        display: flex; align-items: center; justify-content: center;
        z-index: 8000; animation: pm-fade .15s ease;
    }
    @keyframes pm-fade { from { opacity: 0 } to { opacity: 1 } }

    .pm-modal {
        width: min(440px, 92vw);
        background: var(--bg2, #0f172a); color: var(--txt, #e2e8f0);
        border: 1px solid rgba(99,102,241,0.40);
        border-radius: 12px; overflow: hidden;
        box-shadow: 0 20px 50px rgba(0,0,0,0.6);
        animation: pm-slide .18s ease;
    }
    @keyframes pm-slide { from { transform: translateY(10px); opacity: 0 } to { transform: translateY(0); opacity: 1 } }

    .pm-hdr {
        padding: 12px 16px;
        background: rgba(99,102,241,0.10);
        border-bottom: 1px solid rgba(99,102,241,0.20);
    }
    .pm-title {
        font-size: 13px; font-weight: 700; letter-spacing: .35px;
        text-transform: uppercase; color: #818cf8;
    }
    .pm-body  { padding: 14px 16px; display: flex; flex-direction: column; gap: 8px; }
    .pm-label { font-size: 11px; color: var(--txt2, #94a3b8); font-weight: 600; }
    .pm-input {
        width: 100%; box-sizing: border-box;
        padding: 9px 11px;
        background: rgba(255,255,255,0.04);
        border: 1px solid rgba(255,255,255,0.10);
        border-radius: 6px; color: var(--txt, #e2e8f0);
        font-size: 13px; font-family: inherit;
        outline: none;
        transition: border-color .15s, background .15s;
    }
    .pm-input:focus {
        border-color: rgba(99,102,241,0.55);
        background: rgba(255,255,255,0.06);
    }
    .pm-textarea { resize: vertical; min-height: 80px; line-height: 1.5; }

    .pm-actions {
        display: flex; justify-content: flex-end; gap: 8px;
        padding: 11px 16px;
        border-top: 1px solid rgba(255,255,255,0.05);
    }
    .pm-btn {
        padding: 7px 16px; border-radius: 6px; font-size: 12px;
        font-weight: 600; cursor: pointer; border: 1px solid;
        transition: background .15s, color .15s;
    }
    .pm-btn.cancel {
        background: rgba(255,255,255,0.04);
        border-color: rgba(255,255,255,0.10);
        color: var(--txt2, #cbd5e1);
    }
    .pm-btn.cancel:hover { background: rgba(255,255,255,0.09); color: var(--txt, #fff); }
    .pm-btn.confirm {
        background: rgba(99,102,241,0.18);
        border-color: rgba(99,102,241,0.50);
        color: #a5b4fc; font-weight: 700;
    }
    .pm-btn.confirm:hover:not(:disabled) { background: rgba(99,102,241,0.32); color: #fff; }
    .pm-btn.confirm:disabled { opacity: .4; cursor: not-allowed; }

    :global(:root.light) .pm-modal { background: #fff; color: #1e293b; }
    :global(:root.light) .pm-input { background: #f8fafc; border-color: #cbd5e1; color: #1e293b; }
</style>
