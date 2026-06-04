<script lang="ts">
    // v1.4.23 — composer layout extracted to a single global stylesheet
    // (40+ selectors: .chip*, .chips*, .ibar/.igrp/.ibox/.iside, .ia-*,
    // .mbdg, .ollama-dot, .sbtn*, .sec-banner*, .pending-msg-*,
    // .heavy-nudge*, .cost-predict*, .chat-search-bar/.cs-*). Same
    // duplicate-selector trap rationale as tab-strip (v1.4.20).
    import '$lib/styles/composer.css';
    import { createEventDispatcher, onMount, tick } from 'svelte';
    import Paperclip from '@tabler/icons-svelte/icons/paperclip';

    import Mic from '@tabler/icons-svelte/icons/microphone';

    import MicOff from '@tabler/icons-svelte/icons/microphone-off';

    import Eraser from '@tabler/icons-svelte/icons/eraser';
    import { ollamaOnline, nvidiaConfigured, nvidiaModels as _nvidiaModels, localModels as _localModels } from '$lib/models.js';
    // TS can't infer store types from .js — cast to Any writable
    const localModels = _localModels as import('svelte/store').Writable<any[]>;
    const nvidiaModels = _nvidiaModels as import('svelte/store').Writable<any[]>;
    import { suggestFlags, applyFlagCompletion, type FlagSuggestion } from '$lib/flag-completions';
    import { detectHeavyPrompt } from '$lib/smart-router';

    export let tab: any;
    export let isEN: boolean = false;
    /** Quick-win D — Brief mode toggle (lucyConfig.briefMode, lifted to parent). */
    export let briefMode: boolean = false;
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
    // Forwarded from the parent so we can resolve the effective model when
    // smart-routing or nvidia-custom is in play. Currently used only by
    // the model-picker tooltip — the visible <select> is bound to
    // tab.selectedModel directly so the user sees their manual choice
    // rather than the routed one.
    // svelte-ignore unused-export-let
    export let getEffectiveModel: (tab: any) => string = (t) => t?.selectedModel || '';
    export let getModelDescription: (model: string, isEN: boolean) => string = () => '';
    export let formatTokens: (n: number) => string = (n) => String(n);

    const dispatch = createEventDispatcher<{
        attach: void;
        togglemic: void;
        clearsession: void;
        togglebrief: void;
        togglepause: void;
        skipnexttool: void;
        upgrademodel: void;
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

    // ── Auto-grow textarea (restored from pre-refactor) ──
    // CAP must match the .ibox max-height in page.css. The JS reads the
    // CSS-computed max-height at runtime so this stays in sync even if
    // someone tweaks the stylesheet later.
    const CAP_PX = 100;
    const FLOOR_PX = 24;

    // ── Empty-state shortcut hints (quick win C) ──
    // Shown only when the textarea is empty AND not focused AND not in any
    // special state. This is the "first 30s discoverability" surface: a
    // subtle overlay listing the keyboard chords Lucy actually responds to,
    // so a new user finds Ctrl+P / Tab / Esc without having to read docs.
    let _ifocused = false;
    $: _showShortcutHints = !_ifocused
        && !tab?.inputValue
        && !tab?.pendingMessage
        && !tab?.isProcessing;

    // ── Heavy-prompt nudge (v1.4.5) ──
    // When the user is typing a structurally complex prompt (audit,
    // multi-task enumeration, large context) AND has a fast/cheap model
    // selected AND smart-routing is OFF, show a non-intrusive suggestion
    // chip offering to upgrade to a stronger reasoner. This is the bench-
    // mark fix from Caso 2: users would otherwise hit Send on a long
    // audit prompt with Flash and end up with a truncated/half-finished
    // result. The nudge gives them a one-click upgrade BEFORE they pay
    // the $ + time of a bad run.
    export let smartRoutingEnabled: boolean = false;
    $: _isFastModel = !!tab?.selectedModel && /^(gemini.*flash|gemini-3.*lite|claude-haiku|local-)/i.test(tab.selectedModel);
    $: _heavyReason = (!smartRoutingEnabled && _isFastModel && tab?.inputValue)
        ? detectHeavyPrompt(String(tab.inputValue), Math.ceil(String(tab.inputValue).length / 3.6))
        : null;
    let _nudgeDismissed = false;
    function _dispatchUpgrade() {
        dispatch('upgrademodel');
        _nudgeDismissed = true;
    }

    function autoResize() {
        if (!_textareaEl) return;
        // Reset first so scrollHeight reports natural content height —
        // setting to 'auto' (vs '0px') plays nicer with browsers that
        // refuse to shrink under min-height after a 0px touch.
        _textareaEl.style.height = 'auto';
        const sh = _textareaEl.scrollHeight;
        const target = Math.max(FLOOR_PX, Math.min(sh, CAP_PX));
        _textareaEl.style.height = target + 'px';
        // Scrollbar only past the cap. Anything else = hidden so the box
        // never shows a scrollbar when it doesn't need one.
        _textareaEl.style.overflowY = sh > CAP_PX ? 'auto' : 'hidden';
    }

    // ── Reactive on tab.inputValue (with cached-last-value guard) ──
    // Svelte 4 compiles `$:` reactives by tracking ALL identifiers read.
    // `tab.inputValue` makes `tab` a dep, and the parent does
    // `tabs = [...tabs]` ~30× per agent turn — so this $: ALWAYS refires
    // 30× per turn regardless of whether inputValue actually changed.
    // Without the runtime guard below, that was enough to queue enough
    // microtasks to stall the splash → main UI transition (startup hang
    // reproduced twice now).
    //
    // Fix: cache the last value we resized for. The $: block still
    // re-evaluates 30× per turn (compile-time fact, can't avoid), but
    // only QUEUES the autoResize microtask when the cached value
    // actually changed. Cheap two-string comparisons replace heavy
    // microtask flooding.
    let _lastInputValueSeen = '';
    $: {
        if (_textareaEl && tab) {
            const cur = typeof tab.inputValue === 'string' ? tab.inputValue : '';
            if (cur !== _lastInputValueSeen) {
                _lastInputValueSeen = cur;
                tick().then(autoResize);
            }
        }
    }

    // Initial size on mount.
    onMount(async () => {
        await tick();
        autoResize();
    });

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
                <img src={file.previewUrl} alt="p" style="height:48px;width:auto;max-width:80px;object-fit:cover;border-radius:4px;display:block;">
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
    on:dragover|preventDefault={(e) => { if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'; e.currentTarget.classList.add('drag-over'); }}
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

    <!-- v1.4.5 — Heavy-prompt nudge. Surfaces ABOVE the input when the
         user is on a fast model + smart-routing OFF + a structurally
         heavy prompt was typed. One-click upgrade dispatched as
         'upgrademodel' so the parent can swap the tab.selectedModel
         to Sonnet (or whatever the appropriate heavy tier is). -->
    {#if _heavyReason && !_nudgeDismissed}
    <div class="heavy-nudge" role="status">
        <span class="heavy-nudge-glyph">⚡</span>
        <span class="heavy-nudge-text">
            {isEN ? 'Complex prompt detected' : 'Prompt complejo detectado'}
            <small>· {_heavyReason}</small>
        </span>
        <button class="heavy-nudge-act" on:click={_dispatchUpgrade}
            title={isEN ? 'Switch to Claude Sonnet for better synthesis' : 'Cambiar a Claude Sonnet para mejor síntesis'}>
            {isEN ? 'Upgrade →' : 'Mejorar →'}
        </button>
        <button class="heavy-nudge-x" on:click={() => _nudgeDismissed = true}
            title={isEN ? 'Dismiss' : 'Descartar'} aria-label="Dismiss">✕</button>
    </div>
    {/if}

    <!-- v1.7.63 — Composer ops-aesthetic. `iprompt` is a small lambda/dollar
         glyph that frames the textarea as a command-line. `igrp` gets a dot
         grid background on focus via CSS; the `class:islash` toggle below
         tints the prompt amber when the buffer starts with `/` to hint at
         slash-command mode. Purely cosmetic — no behavioural change. -->
    <div class="igrp" class:islash={(tab.inputValue || '').trimStart().startsWith('/')} style="position:relative;">
        <span class="iprompt" aria-hidden="true">λ</span>
        <textarea class="ibox" rows="1"
            placeholder={tab.pendingMessage
                ? (isEN ? 'Message queued — waiting for Lucy…' : 'Mensaje en espera — esperando a Lucy…')
                : tab.isProcessing
                    ? (isEN ? 'Type here — will send when Lucy finishes…' : 'Escribe aquí — se enviará cuando Lucy termine…')
                    : cmdPlaceholder}
            bind:value={tab.inputValue}
            bind:this={_textareaEl}
            on:focus={() => _ifocused = true}
            on:input={(e) => { autoResize(); refreshFlagSuggestions(); dispatch('inputchange', { event: e }); }}
            on:paste={() => tick().then(autoResize)}
            on:cut={() => tick().then(autoResize)}
            on:keydown={(e) => { if (handleSuggestionKey(e)) return; dispatch('keydown', { event: e }); }}
            on:blur={() => { _ifocused = false; setTimeout(() => { _flagSuggestions = []; }, 120); }}
            disabled={!!tab.pendingMessage}></textarea>

        <!-- v1.5.5 — Empty-state shortcut hints row hidden per user
             feedback: Ctrl+P / Tab / @ / Esc are advertised here as
             "available" but several don't actually route to anything
             yet (Ctrl+P palette only opens in the Settings modal route,
             @ host has no autocompleter, Esc only cancels active
             agent runs not the composer). Advertising broken
             shortcuts is worse than not advertising any. The
             KeyboardCheatsheet (Shift+?) remains the single source of
             truth for what actually works. Guarded with `{#if false}`
             so the markup stays in place for a future re-enable once
             the underlying handlers are wired. -->
        {#if false}
            <div class="ihints" aria-hidden="true">
                <kbd>Ctrl+P</kbd> <span>{isEN ? 'palette' : 'paleta'}</span>
                <span class="ihint-sep">·</span>
                <kbd>Tab</kbd> <span>{isEN ? 'autocomplete' : 'autocompletar'}</span>
                <span class="ihint-sep">·</span>
                <kbd>/</kbd> <span>{isEN ? 'commands' : 'comandos'}</span>
                <span class="ihint-sep">·</span>
                <kbd>@</kbd> <span>{isEN ? 'host' : 'host'}</span>
                <span class="ihint-sep">·</span>
                <kbd>Esc</kbd> <span>{isEN ? 'cancel' : 'cancelar'}</span>
            </div>
        {/if}

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
                <Paperclip size={15} stroke={1.8} />
            </button>
            <button class="ia-btn {tab.isListening ? 'mic-on' : ''}"
                title={isEN ? 'Voice input' : 'Entrada de voz'}
                on:click={() => dispatch('togglemic')}
                disabled={tab.isProcessing && !tab.isListening}>
                {#if tab.isListening}<MicOff size={15} stroke={1.8} />{:else}<Mic size={15} stroke={1.8} />{/if}
            </button>
            <button class="ia-btn" title={isEN ? 'Clear session (Ctrl+L)' : 'Limpiar sesión (Ctrl+L)'}
                on:click={() => dispatch('clearsession')} disabled={tab.isProcessing}>
                <Eraser size={15} stroke={1.8} />
            </button>
            <!-- Quick-win D — Brief Mode toggle. When on, prepends a terse-output
                 directive to every prompt sent. Visible state via .brief-on class. -->
            <button class="ia-btn brief-btn" class:brief-on={briefMode}
                title={briefMode
                    ? (isEN ? 'Brief mode ON — Lucy answers in 3 lines max' : 'Modo conciso ACTIVO — Lucy responde en 3 líneas máx.')
                    : (isEN ? 'Brief mode OFF — toggle for short answers' : 'Modo conciso INACTIVO — activa para respuestas cortas')}
                on:click={() => dispatch('togglebrief')}>
                <span class="brief-glyph" aria-hidden="true">≡</span>
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
        <!-- Quick-win F — Granular cancel: ⏸ pause between iterations,
             ⏭ skip the next tool call, 🛑 cancel everything (the existing
             stop button). The three live in a small inline cluster so the
             user can downgrade severity instead of going straight to kill. -->
        <button class="sbtn sbtn-pause" class:on={tab._paused}
            on:click={() => dispatch('togglepause')}
            title={tab._paused
                ? (isEN ? 'Resume' : 'Reanudar')
                : (isEN ? 'Pause after current step' : 'Pausar tras el paso actual')}>
            {#if tab._paused}
                <svg width="11" height="11" viewBox="0 0 11 11" fill="currentColor"><path d="M2 1.5v8l7-4z"/></svg>
            {:else}
                <svg width="11" height="11" viewBox="0 0 11 11" fill="currentColor"><rect x="2" y="1.5" width="2.5" height="8" rx="1"/><rect x="6.5" y="1.5" width="2.5" height="8" rx="1"/></svg>
            {/if}
        </button>
        <button class="sbtn sbtn-skip"
            on:click={() => dispatch('skipnexttool')}
            title={isEN ? 'Skip next tool call' : 'Saltar próxima herramienta'}>
            <svg width="11" height="11" viewBox="0 0 11 11" fill="currentColor">
                <path d="M2 1.5v8l5-4z"/><rect x="8" y="1.5" width="1.5" height="8" rx="0.5"/>
            </svg>
        </button>
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

    /* v1.4.23 — composer layout extracted to $lib/styles/composer.css
       (imported from <script>). What was here:
         .staged / .sf-* / .chip* / .chips* (+ light theme variants)
         .sec-banner* (security warning above input)
         .chat-search-bar / .cs-* (Ctrl+F bar)
         .ibar / .igrp (+ state-aware glow) / .ibox / .iside
         .ia-btn (+ mic-on, brief-on) / .ia-sep
         .mbdg (+ select, option, optgroup) / .nvidia-custom-input
         .ollama-dot (runtime status)
         .sbtn / .sbtn-stop / .sbtn-pause / .sbtn-skip
         .pending-msg-* / .heavy-nudge*
         .cost-predict* (+ light theme variants)
         composer-wide :root.light overrides
       40+ selectors total. The page.css copies (mostly winning the
       cascade tiebreaker over these) are also deleted. See
       composer.css header for the drift notes (page.css values won
       where they differed). */

    /* ── Quick-win C: empty-state shortcut hints ─────────────────────
       Absolute-positioned ribbon inside the .igrp wrapper. Sits behind
       the textarea (z-index 0 + pointer-events:none) so clicks always
       land on the textarea itself. Fades when the user starts typing
       — that's gated in the markup via {#if _showShortcutHints}. */
    .ihints {
        position: absolute;
        left: 14px;
        right: 14px;
        top: 50%;
        transform: translateY(-50%);
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 6px;
        font-size: 11px;
        color: var(--txt3, #475569);
        pointer-events: none;
        z-index: 0;
        opacity: 0.78;
        line-height: 1;
        user-select: none;
        animation: ihint-fade 240ms ease-out;
    }
    .ihints kbd {
        font-family: var(--mono, monospace);
        font-size: 10px;
        font-weight: 600;
        color: var(--txt2, #94a3b8);
        background: rgba(255, 255, 255, 0.04);
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 4px;
        padding: 1px 5px;
        line-height: 1.4;
    }
    .ihint-sep {
        opacity: 0.4;
        margin: 0 2px;
    }
    @keyframes ihint-fade {
        from { opacity: 0; }
        to   { opacity: 0.78; }
    }
    /* On narrow widths drop the last few chips so the row never wraps. */
    @media (max-width: 720px) {
        .ihints kbd:nth-of-type(n+4),
        .ihints span:nth-of-type(n+4),
        .ihints .ihint-sep:nth-of-type(n+3) { display: none; }
    }
</style>
