<!-- ── DialogHost.svelte — Renderer for lucy{Confirm,Alert,Prompt} (v1.7.17) ──
     Mounted once near the root of the app. Subscribes to `activeDialog`
     from `$lib/dialog-service` and renders the current dialog with
     Lucy's visual identity. Pressing OK / Cancel / Esc / Enter calls
     `settle()` to advance the queue. -->
<script lang="ts">
    import { onMount, onDestroy, tick } from 'svelte';
    import { activeDialog, settle, type DialogRequest } from '$lib/dialog-service';

    let cur: DialogRequest | null = null;
    let value = '';
    let inputEl: HTMLInputElement | HTMLTextAreaElement | undefined;

    const unsub = activeDialog.subscribe(async (d) => {
        cur = d;
        if (d && d.kind === 'prompt') {
            value = d.defaultValue ?? '';
            await tick();
            inputEl?.focus();
            try { (inputEl as HTMLInputElement)?.select?.(); } catch { /* ignore */ }
        }
    });

    onDestroy(unsub);

    function ok()      { settle(cur?.kind === 'prompt' ? value : true); }
    function cancel()  { settle(cur?.kind === 'prompt' ? null  : false); }
    function alertOk() { settle(undefined); }

    function onKey(e: KeyboardEvent) {
        if (!cur) return;
        if (e.key === 'Escape')  { e.preventDefault(); cur.kind === 'alert' ? alertOk() : cancel(); }
        if (e.key === 'Enter' && (!cur.multiline || e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            if (cur.kind === 'alert')        alertOk();
            else if (cur.kind === 'confirm') ok();
            else if (cur.kind === 'prompt')  ok();
        }
    }

    function toneClass(t: string | undefined) {
        switch (t) {
            case 'danger':  return 'ldh-danger';
            case 'warning': return 'ldh-warning';
            case 'success': return 'ldh-success';
            case 'info':    return 'ldh-info';
            default:        return 'ldh-default';
        }
    }

    function toneGlyph(t: string | undefined) {
        switch (t) {
            case 'danger':  return '✕';
            case 'warning': return '⚠';
            case 'success': return '✓';
            case 'info':    return 'ℹ';
            default:        return '◆';
        }
    }
</script>

<svelte:window on:keydown={onKey} />

{#if cur}
    <div class="ldh-overlay" on:click|self={cancel} role="presentation">
        <div class="ldh-box {toneClass(cur.tone)}" role="dialog"
             aria-modal="true" aria-labelledby="ldh-title">
            <div class="ldh-hdr">
                <span class="ldh-glyph">{toneGlyph(cur.tone)}</span>
                <span id="ldh-title" class="ldh-title">{cur.message}</span>
            </div>
            {#if cur.description}
                <div class="ldh-desc">{cur.description}</div>
            {/if}
            {#if cur.kind === 'prompt'}
                {#if cur.multiline}
                    <textarea bind:this={inputEl} bind:value
                              class="ldh-input ldh-textarea"
                              placeholder={cur.placeholder}
                              rows="4"></textarea>
                {:else}
                    <input type="text" bind:this={inputEl} bind:value
                           class="ldh-input"
                           placeholder={cur.placeholder} />
                {/if}
            {/if}
            <div class="ldh-actions">
                {#if cur.kind === 'alert'}
                    <button class="ldh-btn ldh-btn-primary" on:click={alertOk}>
                        {cur.confirmLabel}
                    </button>
                {:else}
                    <button class="ldh-btn" on:click={cancel}>
                        {cur.cancelLabel || 'Cancelar'}
                    </button>
                    <button class="ldh-btn ldh-btn-primary"
                            on:click={ok}
                            disabled={cur.kind === 'prompt' && !value.trim().length}>
                        {cur.confirmLabel}
                    </button>
                {/if}
            </div>
            <div class="ldh-hint">
                {cur.kind === 'prompt' && cur.multiline
                    ? 'Ctrl+Enter para aceptar · Esc para cancelar'
                    : 'Enter para aceptar · Esc para cancelar'}
            </div>
        </div>
    </div>
{/if}

<style>
    .ldh-overlay {
        /* v1.7.232 — above the dev cockpit overlay (z 9999) so a lucyConfirm/
           lucyPrompt (e.g. the close-tab confirmation) renders OVER the cockpit,
           dimming it via this scrim, instead of forcing the cockpit to hide and
           reveal the classic UI behind. Unchanged in classic mode: still sits
           above all normal content, below the 10500 critical-overlay band. */
        position: fixed; inset: 0; z-index: 10050;
        background: rgba(2, 6, 12, 0.72);
        backdrop-filter: blur(4px);
        display: flex; align-items: center; justify-content: center;
        animation: ldh-fade-in 180ms ease;
    }
    .ldh-box {
        min-width: 380px; max-width: 540px; width: 90vw;
        background: var(--bg-card, #161b22);
        border: 1px solid color-mix(in srgb, var(--accent, #10b981) 35%, transparent);
        border-radius: 14px;
        box-shadow:
            0 24px 64px rgba(0, 0, 0, 0.62),
            0 0 28px color-mix(in srgb, var(--accent, #10b981) 16%, transparent);
        padding: 20px 22px 16px;
        animation: ldh-slide-in 220ms cubic-bezier(.22,.61,.36,1);
        font-family: var(--font-ui, ui-sans-serif, system-ui);
    }
    .ldh-hdr {
        display: flex; align-items: flex-start; gap: 12px;
        margin-bottom: 6px;
    }
    .ldh-glyph {
        font-size: 22px; line-height: 1;
        padding: 4px 8px; border-radius: 9px;
        background: rgba(255,255,255,0.05);
    }
    .ldh-title {
        font-size: 15px; font-weight: 600;
        color: var(--txt1, #f8fafc);
        line-height: 1.45;
        flex: 1;
    }
    .ldh-desc {
        font-size: 12.5px; line-height: 1.6;
        color: var(--txt2, #94a3b8);
        margin: 6px 0 12px 0;
        padding-left: 42px;
    }
    .ldh-input {
        display: block; width: 100%;
        margin: 8px 0 12px;
        padding: 10px 12px;
        border-radius: 8px;
        border: 1px solid var(--bdr, #334155);
        background: rgba(0, 0, 0, 0.25);
        color: var(--txt1, #f8fafc);
        font-family: var(--font-ui, inherit);
        font-size: 14px;
        outline: none;
        transition: border-color .12s;
    }
    .ldh-input:focus {
        border-color: var(--accent, #10b981);
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent, #10b981) 18%, transparent);
    }
    .ldh-textarea { resize: vertical; min-height: 70px; }
    .ldh-actions {
        display: flex; justify-content: flex-end; gap: 8px;
        margin-top: 10px;
    }
    .ldh-btn {
        padding: 7px 16px; border-radius: 8px;
        border: 1px solid var(--bdr, #334155);
        background: rgba(255, 255, 255, 0.04);
        color: var(--txt1, #f8fafc);
        font-size: 13px; font-weight: 500;
        cursor: pointer;
        transition: background .12s, border-color .12s, transform .08s;
    }
    .ldh-btn:hover  { background: rgba(255, 255, 255, 0.08); }
    .ldh-btn:active { transform: scale(.97); }
    .ldh-btn:disabled { opacity: .45; cursor: not-allowed; }
    .ldh-btn-primary {
        background: color-mix(in srgb, var(--accent, #10b981) 18%, transparent);
        border-color: color-mix(in srgb, var(--accent, #10b981) 60%, transparent);
        color: var(--accent, #10b981);
    }
    .ldh-btn-primary:hover {
        background: color-mix(in srgb, var(--accent, #10b981) 30%, transparent);
    }
    .ldh-hint {
        font-size: 10.5px; color: var(--txt3, #64748b);
        text-align: right; margin-top: 6px;
        font-family: var(--mono, ui-monospace, monospace);
    }

    /* Tone variants — change the accent and glyph color. */
    .ldh-danger  { border-color: color-mix(in srgb, var(--red,   #ef4444) 50%, transparent); }
    .ldh-danger  .ldh-glyph,
    .ldh-danger  .ldh-btn-primary { color: var(--red,   #ef4444); border-color: color-mix(in srgb, var(--red,#ef4444) 60%, transparent); background: color-mix(in srgb, var(--red, #ef4444) 20%, transparent); }
    .ldh-warning { border-color: color-mix(in srgb, var(--amber, #f59e0b) 50%, transparent); }
    .ldh-warning .ldh-glyph,
    .ldh-warning .ldh-btn-primary { color: var(--amber, #f59e0b); border-color: color-mix(in srgb, var(--amber, #f59e0b) 60%, transparent); background: color-mix(in srgb, var(--amber, #f59e0b) 20%, transparent); }
    .ldh-success { border-color: color-mix(in srgb, var(--acc,   #10b981) 50%, transparent); }
    .ldh-success .ldh-glyph { color: var(--acc, #10b981); }
    .ldh-info    { border-color: color-mix(in srgb, var(--blue,  #3b9eff) 50%, transparent); }
    .ldh-info    .ldh-glyph,
    .ldh-info    .ldh-btn-primary { color: var(--blue,  #3b9eff); border-color: color-mix(in srgb, var(--blue, #3b9eff) 60%, transparent); background: color-mix(in srgb, var(--blue, #3b9eff) 20%, transparent); }

    @keyframes ldh-fade-in    { from { opacity: 0; } to { opacity: 1; } }
    @keyframes ldh-slide-in   { from { opacity: 0; transform: translateY(8px) scale(.97); } to { opacity: 1; transform: none; } }
</style>
