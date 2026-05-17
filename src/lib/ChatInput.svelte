<script lang="ts">
    import { createEventDispatcher } from 'svelte';
    import Paperclip from '@tabler/icons-svelte/icons/paperclip';

    import Mic from '@tabler/icons-svelte/icons/microphone';

    import MicOff from '@tabler/icons-svelte/icons/microphone-off';

    import Eraser from '@tabler/icons-svelte/icons/eraser';
    import { ollamaOnline, nvidiaConfigured, nvidiaModels, localModels } from '$lib/models.js';
    import { suggestFlags, applyFlagCompletion, type FlagSuggestion } from '$lib/flag-completions';

    export let tab: any;
    export let isEN: boolean = false;
    export let costPrediction: any = null;
    export let userChips: any[] = [];
    export let chipsHidden: boolean = false;
    export let pendingSecurityBlock: any = null;
    export let LLM_GROUPS: any[] = [];
    export let showChatSearch: boolean = false;
    export let chatSearch: string = '';
    export let chatSearchCount: number = 0;
    export let isActiveTab: boolean = false;
    export let cmdPlaceholder: string = '';
    export let getEffectiveModel: (tab: any) => string = (t) => t?.selectedModel || '';
    export let getModelDescription: (model: string, isEN: boolean) => string = () => '';
    export let formatTokens: (n: number) => string = (n) => String(n);

    const dispatch = createEventDispatcher<{
        attach: void;
        togglemic: void;
        clearsession: void;
        removefile: { tabId: string; fileName: string };
        runchip: { clave: string };
        addchip: void;
        editchip: { index: number };
        deletechip: { index: number };
        togglechips: void;
        authorizesecurity: void;
        clearsecurity: void;
        send: void;
        stop: void;
        inputchange: { event: Event };
        keydown: { event: KeyboardEvent };
        cancelpending: void;
        chatSearchChange: { value: string };
        closeChatSearch: void;
        filedrop: { event: DragEvent };
    }>();

    // ── Flag autocomplete (May 2026 UX) ──
    // Driven by $lib/flag-completions.ts which queries the hand-curated
    // signatures catalog. The popover renders only when the cursor sits
    // on a "-flag-shaped" token of a known command. Tab/Enter to insert,
    // Esc to dismiss, ArrowUp/Down to navigate.
    let _textareaEl: HTMLTextAreaElement | null = null;
    let _flagSuggestions: FlagSuggestion[] = [];
    let _flagSelIdx = 0;

    function refreshFlagSuggestions() {
        if (!_textareaEl) { _flagSuggestions = []; return; }
        const line = _textareaEl.value;
        const pos  = _textareaEl.selectionStart ?? 0;
        const next = suggestFlags(line, pos, 8);
        _flagSuggestions = next;
        if (_flagSelIdx >= next.length) _flagSelIdx = 0;
    }

    function applySuggestion(flag: string) {
        if (!_textareaEl) return;
        const line = _textareaEl.value;
        const pos  = _textareaEl.selectionStart ?? 0;
        const { line: newLine, cursor } = applyFlagCompletion(line, pos, flag);
        tab.inputValue = newLine;
        // Defer caret update until Svelte applies the bind:value
        requestAnimationFrame(() => {
            if (_textareaEl) {
                _textareaEl.selectionStart = _textareaEl.selectionEnd = cursor;
                _textareaEl.focus();
            }
        });
        _flagSuggestions = [];
        _flagSelIdx = 0;
    }

    /** Returns true if the key was consumed by the suggestion popover. */
    function handleSuggestionKey(e: KeyboardEvent): boolean {
        if (_flagSuggestions.length === 0) return false;
        if (e.key === 'Escape')    { _flagSuggestions = []; e.preventDefault(); return true; }
        if (e.key === 'ArrowDown') { _flagSelIdx = (_flagSelIdx + 1) % _flagSuggestions.length; e.preventDefault(); return true; }
        if (e.key === 'ArrowUp')   { _flagSelIdx = (_flagSelIdx - 1 + _flagSuggestions.length) % _flagSuggestions.length; e.preventDefault(); return true; }
        if (e.key === 'Tab' || e.key === 'Enter') {
            const choice = _flagSuggestions[_flagSelIdx];
            if (choice) { applySuggestion(choice.flag); e.preventDefault(); return true; }
        }
        return false;
    }
</script>

<!-- ── STAGED FILES ── -->
<div class="staged">
    {#each tab.attachedFiles ?? [] as file}
        <div class="sf-bdg">
            {#if file.type === 'image'}
                <img src={file.previewUrl} alt="p" style="width:22px;height:22px;object-fit:cover;border-radius:3px;">
            {:else}
                <span>·</span>
            {/if}
            <span style="font-size:12px;">{file.name}</span>
            <button class="sf-rm" on:click={() => dispatch('removefile', { tabId: tab.id, fileName: file.name })} on:keydown>✕</button>
        </div>
    {/each}
</div>

<!-- ── CHIPS BAR ── -->
<div class="chips" class:chips-collapsed={chipsHidden}>
    <button class="chips-toggle" on:click={() => dispatch('togglechips')}
        title={chipsHidden
            ? (isEN ? `Show ${userChips.length} Lucy shortcuts` : `Mostrar ${userChips.length} atajos de Lucy`)
            : (isEN ? 'Hide Lucy shortcuts' : 'Ocultar atajos de Lucy')}>
        <span class="chips-lucy-label">Lucy ↗</span>
        {#if chipsHidden}<span class="chips-count">{userChips.length}</span>{/if}
        <span class="chips-chevron">{chipsHidden ? '▸' : '▾'}</span>
    </button>
    {#if !chipsHidden}
        {#each userChips as chip, i}
            <div class="chip-wrap">
                <button class="chip chip-user" on:click={() => dispatch('runchip', { clave: chip.clave })}
                    disabled={tab.isProcessing} title="Enviar a Lucy: {chip.clave}">{chip.label}</button>
                <div class="chip-actions">
                    <button class="chip-act" on:click|stopPropagation={() => dispatch('editchip', { index: i })} title="Editar">✎</button>
                    <button class="chip-act chip-del" on:click|stopPropagation={() => dispatch('deletechip', { index: i })} title="Eliminar">✕</button>
                </div>
            </div>
        {/each}
        <button class="chip chip-add" on:click={() => dispatch('addchip')}
            title={isEN ? 'Add message shortcut for Lucy' : 'Agregar atajo de mensaje para Lucy'}>＋</button>
    {/if}
</div>

<!-- ── SECURITY BLOCK BANNER ── -->
{#if pendingSecurityBlock?.tabId === tab.id}
<div class="sec-banner" role="alert">
    <div class="sec-banner-hdr">
        <span class="sec-banner-ico">⬡</span>
        <div class="sec-banner-info">
            <span class="sec-banner-title">Instrucción bloqueada por seguridad</span>
            <span class="sec-banner-rule">Regla: <code>{pendingSecurityBlock.blockWord}</code></span>
        </div>
    </div>
    <code class="sec-banner-cmd">{pendingSecurityBlock.displayCmd}</code>
    <div class="sec-banner-actions">
        <button class="mbtn ghost" style="font-size:12px;padding:6px 14px;" on:click={() => dispatch('clearsecurity')}>Cancelar</button>
        <button class="mbtn warn" style="font-size:12px;padding:6px 14px;" on:click={() => dispatch('authorizesecurity')}>! Autorizar y Ejecutar</button>
    </div>
</div>
{/if}

<!-- ── CHAT SEARCH BAR ── -->
{#if showChatSearch && isActiveTab}
<div class="chat-search-bar">
    <span class="cs-ico">◎</span>
    <input id="chat-search-inp" class="cs-inp" bind:value={chatSearch}
        placeholder={isEN ? 'Search in conversation…' : 'Buscar en conversación…'}
        on:input={(e) => dispatch('chatSearchChange', { value: chatSearch })}
        on:keydown={(e) => { if (e.key === 'Escape') dispatch('closeChatSearch'); }} />
    {#if chatSearch}<span class="cs-count">{chatSearchCount} {isEN ? 'results' : 'resultados'}</span>{/if}
    <button class="cs-close" on:click={() => dispatch('closeChatSearch')}>✕</button>
</div>
{/if}

<!-- ── INPUT BAR ── -->
<div class="ibar" role="region" aria-label={isEN ? 'Message input area' : 'Área de entrada de mensaje'}
    on:dragover|preventDefault={(e) => { e.dataTransfer.dropEffect = 'copy'; e.currentTarget.classList.add('drag-over'); }}
    on:dragleave={(e) => e.currentTarget.classList.remove('drag-over')}
    on:drop|preventDefault={(e) => { e.currentTarget.classList.remove('drag-over'); dispatch('filedrop', { event: e }); }}>

    <!-- Pending message indicator -->
    {#if tab.pendingMessage}
    <div class="pending-msg-bar">
        <span class="pending-msg-dot"></span>
        <span class="pending-msg-text">{isEN ? 'Queued' : 'En espera'}: "{tab.pendingMessage.text.length > 50 ? tab.pendingMessage.text.slice(0, 50) + '…' : tab.pendingMessage.text}"</span>
        <button class="pending-msg-cancel" title={isEN ? 'Cancel queued message' : 'Cancelar mensaje en espera'}
            on:click={() => dispatch('cancelpending')}>✕</button>
    </div>
    {/if}

    <div class="igrp" style="position:relative;">
        <textarea class="ibox" rows="1"
            placeholder={tab.pendingMessage
                ? (isEN ? 'Message queued — waiting for Lucy…' : 'Mensaje en espera — esperando a Lucy…')
                : tab.isProcessing
                    ? (isEN ? 'Type here — will send when Lucy finishes…' : 'Escribe aquí — se enviará cuando Lucy termine…')
                    : cmdPlaceholder}
            bind:value={tab.inputValue}
            bind:this={_textareaEl}
            on:input={(e) => { dispatch('inputchange', { event: e }); refreshFlagSuggestions(); }}
            on:keydown={(e) => { if (handleSuggestionKey(e)) return; dispatch('keydown', { event: e }); }}
            on:blur={() => setTimeout(() => { _flagSuggestions = []; }, 120)}
            disabled={!!tab.pendingMessage}></textarea>

        <!-- Flag autocomplete popover — appears only when caret is on a flag-shaped token of a known command -->
        {#if _flagSuggestions.length > 0}
            <div class="flag-pop" role="listbox" aria-label="Flag suggestions">
                {#each _flagSuggestions as s, i (s.flag)}
                    <button class="flag-row" class:sel={i === _flagSelIdx} class:destructive={s.destructive}
                        on:mousedown|preventDefault={() => applySuggestion(s.flag)}>
                        <span class="flag-flag">{s.flag}</span>
                        <span class="flag-desc">{s.desc}</span>
                        {#if s.destructive}<span class="flag-warn" title={isEN ? 'destructive' : 'destructivo'}>⚠</span>{/if}
                    </button>
                {/each}
                <div class="flag-foot">{isEN ? 'Tab/Enter: insert · Esc: close' : 'Tab/Enter: insertar · Esc: cerrar'}</div>
            </div>
        {/if}

        <div class="iside">
            <button class="ia-btn" title={isEN ? 'Attach file' : 'Adjuntar archivo'}
                on:click={() => dispatch('attach')} disabled={!!tab.pendingMessage}>
                <Paperclip size={15} strokeWidth={1.8} />
            </button>
            <button class="ia-btn {tab.isListening ? 'mic-on' : ''}"
                title={isEN ? 'Voice input' : 'Entrada de voz'}
                on:click={() => dispatch('togglemic')}
                disabled={tab.isProcessing && !tab.isListening}>
                {#if tab.isListening}<MicOff size={15} strokeWidth={1.8} />{:else}<Mic size={15} strokeWidth={1.8} />{/if}
            </button>
            <button class="ia-btn" title={isEN ? 'Clear session (Ctrl+L)' : 'Limpiar sesión (Ctrl+L)'}
                on:click={() => dispatch('clearsession')} disabled={tab.isProcessing}>
                <Eraser size={15} strokeWidth={1.8} />
            </button>
            <div class="ia-sep"></div>

            {#if isActiveTab && costPrediction}
                <span class="cost-predict cost-predict-{costPrediction.level}"
                    title={costPrediction.level === 'free'
                        ? (isEN ? 'Local model — no API cost' : 'Modelo local — sin costo de API')
                        : `${isEN ? 'Estimated cost' : 'Costo estimado'}: $${costPrediction.cost.toFixed(4)}\n${isEN ? 'Input' : 'Entrada'}: ~${costPrediction.inputTokens} tokens\n${isEN ? 'Output' : 'Salida'}: ~${costPrediction.outputTokens} tokens\n${isEN ? 'Model' : 'Modelo'}: ${costPrediction.model}`}>
                    {#if costPrediction.level === 'free'}
                        <span class="cp-icon">●</span>{isEN ? 'free' : 'gratis'}
                    {:else}
                        <span class="cp-icon">≈</span>
                        <span class="cp-tokens">{formatTokens(costPrediction.totalTokens)}</span>
                        <span class="cp-cost">${costPrediction.cost < 0.001 ? costPrediction.cost.toFixed(4) : costPrediction.cost.toFixed(3)}</span>
                    {/if}
                </span>
                <div class="ia-sep"></div>
            {/if}

            <div class="mbdg">
                {#if tab.selectedModel?.startsWith('local-')}
                    <span class="ollama-dot" class:on={$ollamaOnline}
                        title={$ollamaOnline ? 'Ollama online' : 'Ollama offline'}></span>
                {:else if tab.selectedModel?.includes('/') || tab.selectedModel === 'nvidia-custom'}
                    <span class="ollama-dot" class:on={$nvidiaConfigured}
                        title={$nvidiaConfigured ? 'NVIDIA NIM ✓' : 'NVIDIA API Key no configurada'}></span>
                {/if}
                <select bind:value={tab.selectedModel} disabled={tab.isProcessing}
                    title={getModelDescription(tab.selectedModel, isEN)}>
                    {#each LLM_GROUPS as group}
                        <optgroup label={group.label}>
                            {#if group.label.includes('Locales')}
                                {#each $localModels as opt}
                                    <option value={opt.id}>{opt.icon} {isEN ? opt.nameEn : opt.nameEs}</option>
                                {/each}
                            {:else if group.provider === 'nvidia' && $nvidiaModels.length > 0}
                                {#each $nvidiaModels as opt}
                                    <option value={opt.id}>{opt.icon} {isEN ? opt.nameEn : opt.nameEs}</option>
                                {/each}
                            {:else}
                                {#each group.options as opt}
                                    <option value={opt.id}>{opt.icon} {isEN ? opt.nameEn : opt.nameEs}</option>
                                {/each}
                            {/if}
                        </optgroup>
                    {/each}
                </select>
                {#if tab.selectedModel === 'nvidia-custom'}
                    <input class="nvidia-custom-input" type="text"
                        bind:value={tab.nvidiaCustomModel}
                        disabled={tab.isProcessing}
                        placeholder="owner/model  (ej: nicoboss/DeepSeek-R1-Distill-Qwen-32B-Uncensored)"
                        title={isEN ? 'Type the exact NVIDIA NIM model ID (owner/model-name)' : 'Escribe el ID exacto del modelo NVIDIA NIM (owner/model-name)'} />
                {/if}
            </div>
        </div>
    </div>

    <!-- Send / Stop toggle -->
    {#if tab.isProcessing}
        <button class="sbtn sbtn-stop" on:click={() => dispatch('stop')}
            title={isEN ? 'Stop (Escape)' : 'Detener (Escape)'}>
            <svg width="13" height="13" viewBox="0 0 13 13" fill="currentColor">
                <rect x="1.5" y="1.5" width="10" height="10" rx="2"/>
            </svg>
        </button>
    {:else}
        <button class="sbtn" on:click={() => dispatch('send')}
            disabled={!tab.inputValue?.trim() && !tab.attachedFiles?.length}>▶</button>
    {/if}
</div>

<style>
    /* Flag autocomplete popover (May 2026 UX) */
    .flag-pop {
        position: absolute;
        bottom: calc(100% + 4px);
        left: 0;
        right: auto;
        min-width: 280px;
        max-width: 480px;
        background: var(--panel-bg, #0f1520);
        border: 1px solid var(--bdr);
        border-radius: 8px;
        padding: 4px;
        box-shadow: 0 8px 32px rgba(0,0,0,0.45);
        z-index: 50;
        max-height: 300px;
        overflow-y: auto;
    }
    .flag-row {
        display: flex; align-items: center; gap: 8px;
        width: 100%;
        background: transparent;
        border: 1px solid transparent;
        border-radius: 4px;
        padding: 5px 8px;
        text-align: left;
        cursor: pointer;
        color: var(--txt2);
        font-size: 12px;
        transition: background .12s ease;
    }
    .flag-row:hover, .flag-row.sel { background: rgba(255,255,255,0.06); border-color: var(--bdr); color: var(--txt); }
    .flag-row.destructive { color: #fca5a5; }
    .flag-row.destructive.sel, .flag-row.destructive:hover { background: rgba(239,68,68,0.08); }
    .flag-flag {
        font-family: var(--mono);
        font-weight: 600;
        font-size: 11px;
        min-width: 90px;
        color: inherit;
    }
    .flag-desc {
        flex: 1;
        font-size: 11px;
        opacity: 0.8;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .flag-warn { color: #ef4444; font-size: 12px; }
    .flag-foot {
        padding: 4px 8px 2px;
        font-size: 10px;
        color: var(--txt3);
        border-top: 1px solid var(--bdr);
        margin-top: 2px;
        font-family: var(--mono);
    }

    :global(.staged){padding:4px 14px;display:flex;flex-wrap:wrap;gap:4px;}
    :global(.sf-bdg){display:inline-flex;align-items:center;gap:6px;background:var(--acc-d);border:1px solid var(--acc-b);color:var(--acc);padding:3px 10px;border-radius:20px;font-size:12px;}
    :global(.sf-rm){background:none;border:none;color:var(--red);cursor:pointer;font-weight:bold;padding:0 3px;font-size:12px;line-height:1;}
    :global(.chips){display:flex;align-items:center;gap:5px;padding:5px 14px;overflow-x:auto;border-top:1px solid #131825;flex-shrink:0;}
    :global(.chips.chips-collapsed){padding:3px 14px;}
    :global(.chips-lucy-label){flex-shrink:0;font-size:10px;font-weight:700;color:rgba(16,185,129,.4);letter-spacing:.5px;padding:2px 6px 2px 0;white-space:nowrap;user-select:none;}
    :global(.chips:not(.chips-collapsed) .chips-lucy-label){border-right:1px solid rgba(16,185,129,.1);margin-right:3px;}
    :global(.chips::-webkit-scrollbar){display:none;}
    :global(.chips-toggle){display:inline-flex;align-items:center;gap:5px;background:transparent;border:none;cursor:pointer;padding:0;flex-shrink:0;transition:opacity .15s;}
    :global(.chips-toggle:hover){opacity:.85;}
    :global(.chips-toggle:hover .chips-lucy-label){color:rgba(16,185,129,.7);}
    :global(.chips-count){display:inline-block;font-family:var(--mono);font-size:10px;font-weight:700;color:rgba(16,185,129,.55);background:rgba(16,185,129,.08);padding:1px 6px;border-radius:8px;letter-spacing:.2px;}
    :global(.chips-chevron){font-size:9px;color:rgba(16,185,129,.45);transition:transform .15s;user-select:none;width:10px;text-align:center;}
    :global(:root.light .chips){border-top-color:rgba(0,0,0,.08);}
    :global(:root.light .chips-lucy-label){color:rgba(15,123,90,.7);}
    :global(:root.light .chips:not(.chips-collapsed) .chips-lucy-label){border-right-color:rgba(15,123,90,.18);}
    :global(:root.light .chips-count){color:#0f7b5a;background:rgba(15,123,90,.10);}
    :global(:root.light .chips-chevron){color:rgba(15,123,90,.55);}
    :global(:root.light .chips-toggle:hover .chips-lucy-label){color:#0f7b5a;}
    :global(.chip){display:flex;align-items:center;gap:4px;white-space:nowrap;background:rgba(16,185,129,.04);border:1px solid rgba(16,185,129,.09);color:#0d9668;border-radius:12px;padding:3px 10px;font-size:11px;cursor:pointer;transition:.15s;flex-shrink:0;font-family:inherit;}
    :global(.chip:hover){background:rgba(16,185,129,.1);color:#0d9668;border-color:rgba(16,185,129,.22);}
    :global(.chip:disabled){opacity:.3;cursor:not-allowed;}
    :global(.chip-user){background:rgba(59,130,246,.06);border-color:rgba(59,130,246,.15);color:#4a7aaa;}
    :global(.chip-user:hover){background:rgba(59,130,246,.12);}
    :global(.chip-add){background:transparent;border-color:rgba(255,255,255,.08);color:#475569;padding:3px 8px;}
    :global(.chip-add:hover){border-color:var(--acc-b);color:var(--acc);}
    :global(.chip-wrap){position:relative;display:flex;align-items:center;flex-shrink:0;}
    :global(.chip-actions){display:none;position:absolute;right:-2px;top:-6px;gap:2px;background:var(--bg3);border:1px solid var(--bdr2);border-radius:6px;padding:2px 3px;}
    :global(.chip-wrap:hover .chip-actions){display:flex;}
    :global(.chip-act){background:none;border:none;cursor:pointer;font-size:10px;color:var(--txt2);padding:1px 3px;border-radius:3px;line-height:1;transition:.1s;}
    :global(.chip-act:hover){background:var(--bdr2);color:var(--txt);}
    :global(.chip-del:hover){color:var(--red)!important;}
    .sec-banner{background:rgba(255,170,0,.05);border:1px solid rgba(255,170,0,.25);border-radius:8px;padding:12px 14px;margin:0 14px 8px;display:flex;flex-direction:column;gap:8px;flex-shrink:0;}
    .sec-banner-hdr{display:flex;align-items:flex-start;gap:10px;}
    .sec-banner-ico{font-size:18px;flex-shrink:0;line-height:1;}
    .sec-banner-info{display:flex;flex-direction:column;gap:2px;}
    .sec-banner-title{font-size:12px;font-weight:700;color:var(--amber);}
    .sec-banner-rule{font-size:11px;color:var(--txt2);}
    .sec-banner-rule code{color:var(--amber);background:rgba(255,170,0,.1);padding:1px 5px;border-radius:3px;}
    .sec-banner-cmd{display:block;font-family:var(--mono);font-size:11px;color:#94a3b8;background:rgba(0,0,0,.3);border:1px solid var(--bdr);border-radius:5px;padding:8px 10px;white-space:pre-wrap;word-break:break-all;max-height:80px;overflow-y:auto;}
    .sec-banner-actions{display:flex;gap:8px;justify-content:flex-end;}
    .chat-search-bar{display:flex;align-items:center;gap:8px;padding:6px 14px;background:rgba(14,21,32,.9);border-top:1px solid var(--bdr);flex-shrink:0;}
    :global(.cs-ico){color:var(--acc);font-size:13px;flex-shrink:0;}
    :global(.cs-inp){flex:1;background:transparent;border:none;outline:none;color:var(--txt);font-size:12px;font-family:inherit;}
    :global(.cs-count){font-size:10px;color:var(--txt3);white-space:nowrap;}
    :global(.cs-close){background:none;border:none;color:var(--txt3);cursor:pointer;font-size:11px;padding:2px 5px;border-radius:3px;transition:.12s;}
    :global(.cs-close:hover){color:var(--red);}
    :global(:root.light .chat-search-bar){background:rgba(220,228,236,.9);border-top-color:var(--bdr);}
    :global(.ibar){display:flex;flex-direction:row;align-items:flex-end;gap:8px;padding:10px 14px;background:#12141e;border-top:1px solid var(--bdr);flex-shrink:0;position:relative;}
    :global(.ibar.drag-over){outline:2px dashed var(--acc);outline-offset:-4px;background:rgba(99,102,241,.06);}
    :global(.igrp){display:flex;flex-direction:column;align-items:stretch;gap:6px;background:rgba(255,255,255,.025);border:1px solid rgba(255,255,255,.07);border-radius:10px;flex:1;padding:8px 10px;transition:border-color var(--motion-base) var(--ease-out),box-shadow var(--motion-base) var(--ease-out),background var(--motion-base) var(--ease-out);}
    :global(body[data-state="idle"]) :global(.igrp:focus-within){border-color:color-mix(in srgb,var(--state-color,var(--accent)) 45%,transparent);box-shadow:0 0 0 3px color-mix(in srgb,var(--state-color,var(--accent)) 8%,transparent),0 0 14px color-mix(in srgb,var(--state-color,var(--accent)) 18%,transparent);}
    :global(body[data-state="thinking"]) :global(.igrp),:global(body[data-state="executing"]) :global(.igrp),:global(body[data-state="error"]) :global(.igrp){border-color:color-mix(in srgb,var(--state-color) 45%,transparent);box-shadow:0 0 0 1px color-mix(in srgb,var(--state-color) 14%,transparent),0 0 12px color-mix(in srgb,var(--state-color) 14%,transparent);background:color-mix(in srgb,var(--state-color) 2.5%,rgba(255,255,255,.025));}
    @supports not (color: color-mix(in srgb, red 50%, blue)){:global(.igrp:focus-within){border-color:var(--state-color,rgba(16,185,129,.4));}}
    :global(.ibox){flex:1;background:transparent;border:none;color:white;font-family:inherit;font-size:13px;outline:none;resize:none;min-height:90px;max-height:320px;overflow-y:auto;line-height:1.5;padding:2px 0;}
    :global(.ibox::placeholder){color:#334155;}
    :global(.iside){display:flex;align-items:center;gap:3px;flex-shrink:0;justify-content:flex-end;padding-top:6px;border-top:1px solid rgba(255,255,255,.04);}
    :global(.ia-btn){background:none;border:none;color:#475569;cursor:pointer;padding:5px 7px;border-radius:6px;font-size:14px;transition:.15s;line-height:1;display:flex;align-items:center;justify-content:center;}
    :global(.ia-btn:hover){background:rgba(255,255,255,.07);color:#94a3b8;}
    :global(.ia-btn:disabled){opacity:.25;cursor:not-allowed;}
    :global(.ia-btn.mic-on){color:var(--red);animation:mp 1.5s infinite;}
    :global(.ia-sep){width:1px;height:16px;background:var(--bdr);margin:0 2px;}
    :global(.mbdg){display:flex;align-items:center;gap:4px;font-size:11px;color:#475569;padding:3px 8px;border:1px solid var(--bdr);border-radius:5px;cursor:pointer;background:rgba(0,0,0,.2);transition:.15s;min-width:130px;}
    :global(.mbdg:hover){border-color:var(--bdr2);color:var(--txt2);}
    :global(.mbdg select){background:none;border:none;outline:none;color:inherit;font:inherit;cursor:pointer;appearance:none;padding:0;width:100%;}
    :global(.nvidia-custom-input){background:none;border:none;border-left:1px solid var(--bdr);outline:none;color:var(--acc);font:inherit;font-size:11px;padding:0 0 0 6px;min-width:220px;width:auto;cursor:text;}
    :global(.nvidia-custom-input::placeholder){color:#476;font-style:italic;}
    :global(.nvidia-custom-input:focus){color:var(--txt);}
    :global(.mbdg option){background:#131825;color:var(--txt);}
    :global(.mbdg optgroup){background:#060a0f;color:#475569;font-size:10px;font-weight:700;letter-spacing:.3px;}
    :global(.ollama-dot){display:inline-block;width:6px;height:6px;border-radius:50%;background:#334155;flex-shrink:0;transition:.2s;}
    :global(.ollama-dot.on){background:var(--acc);box-shadow:0 0 4px rgba(16,185,129,.4);}
    :global(.sbtn){width:36px;height:36px;border-radius:8px;border:none;cursor:pointer;background:rgba(16,185,129,.12);color:var(--acc);display:flex;align-items:center;justify-content:center;font-size:13px;transition:.15s;flex-shrink:0;}
    :global(.sbtn:hover){background:rgba(16,185,129,.22);}
    :global(.sbtn:disabled){opacity:.35;cursor:not-allowed;}
    :global(.sbtn-stop){background:rgba(239,68,68,.1);color:#ef4444;}
    :global(.sbtn-stop:hover){background:rgba(239,68,68,.22);transform:scale(1.08);}
    :global(.pending-msg-bar){display:flex;align-items:center;gap:6px;padding:4px 10px;background:rgba(251,191,36,.06);border-bottom:1px solid rgba(251,191,36,.15);border-radius:8px 8px 0 0;font-size:11px;color:#fbbf24;}
    :global(.pending-msg-dot){width:6px;height:6px;border-radius:50%;background:#fbbf24;animation:pulse-pending 1.2s ease-in-out infinite;flex-shrink:0;}
    @keyframes pulse-pending{0%,100%{opacity:1}50%{opacity:.35}}
    :global(.pending-msg-text){flex:1;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;opacity:.85;}
    :global(.pending-msg-cancel){background:none;border:none;color:#fbbf24;cursor:pointer;opacity:.6;font-size:11px;padding:1px 4px;border-radius:3px;transition:.12s;flex-shrink:0;}
    :global(.pending-msg-cancel:hover){opacity:1;background:rgba(251,191,36,.12);}
    :global(.cost-predict){display:inline-flex;align-items:center;gap:5px;font-family:var(--mono);font-size:10px;font-weight:600;padding:3px 8px;border-radius:9px;letter-spacing:.2px;cursor:help;user-select:none;transition:.18s ease;animation:cp-fade .25s ease;white-space:nowrap;}
    @keyframes cp-fade{from{opacity:0;transform:translateY(-2px);}to{opacity:1;transform:none;}}
    :global(.cost-predict .cp-icon){opacity:.7;font-size:10px;}
    :global(.cost-predict .cp-tokens){opacity:.85;}
    :global(.cost-predict .cp-cost){font-weight:700;letter-spacing:.1px;}
    :global(.cost-predict-ok){color:#a78bfa;background:rgba(167,139,250,.08);border:1px solid rgba(167,139,250,.20);}
    :global(.cost-predict-warn){color:#fbbf24;background:rgba(251,191,36,.08);border:1px solid rgba(251,191,36,.30);}
    :global(.cost-predict-high){color:#ef4444;background:rgba(239,68,68,.10);border:1px solid rgba(239,68,68,.40);box-shadow:0 0 8px rgba(239,68,68,.25);}
    :global(.cost-predict-free){color:var(--acc);background:rgba(16,185,129,.08);border:1px solid rgba(16,185,129,.25);}
    :global(.cost-predict-free .cp-icon){color:var(--acc);animation:cp-pulse 2s ease-in-out infinite;}
    @keyframes cp-pulse{0%,100%{opacity:1;}50%{opacity:.4;}}
    :global(:root.light .cost-predict-ok){color:#6366f1;background:rgba(99,102,241,.08);border-color:rgba(99,102,241,.25);}
    :global(:root.light .cost-predict-warn){color:#92400e;background:rgba(245,158,11,.10);border-color:rgba(245,158,11,.35);}
    :global(:root.light .cost-predict-high){color:#991b1b;background:rgba(239,68,68,.10);border-color:rgba(239,68,68,.45);}
    :global(:root.light .cost-predict-free){color:#0f7b5a;background:rgba(15,123,90,.10);border-color:rgba(15,123,90,.30);}
    :global(:root.light .ibar){background:rgba(224,230,238,.8);border-top-color:var(--bdr);}
    :global(:root.light .igrp){background:rgba(255,255,255,.8);border-color:var(--bdr);}
    :global(:root.light .igrp:focus-within){border-color:var(--acc);}
    :global(:root.light .ibox){background:transparent;color:var(--txt);}
    :global(:root.light .ibox::placeholder){color:var(--txt3);}
    :global(:root.light .ia-btn){color:var(--txt3);}
    :global(:root.light .ia-btn:hover){background:rgba(0,0,0,.06);color:var(--txt2);}
    :global(:root.light .ia-sep){background:var(--bdr);}
    :global(:root.light .chips){border-top-color:var(--bdr);background:var(--bg2);}
    :global(:root.light .mbdg){color:var(--txt2);background:rgba(0,0,0,.04);}
    :global(:root.light .mbdg option){background:#fff;color:var(--txt);}
    :global(:root.light .mbdg optgroup){background:#f0f4f8;color:var(--txt3);}
</style>
