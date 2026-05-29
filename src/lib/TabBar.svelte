<script lang="ts">
    import { createEventDispatcher, tick } from 'svelte';
    import OctagonX from '@tabler/icons-svelte/icons/octagon-minus';
    export let tabs: any[] = [];
    export let activeTabId: string | null = null;
    export let canScrollLeft: boolean = false;
    export let canScrollRight: boolean = false;
    export let renamingTabId: string | null = null;
    export let renameValue: string = '';
    export let focusMode: boolean = false;
    export let isEN: boolean = false;
    export let tabsListEl: HTMLElement | null = null;

    const dispatch = createEventDispatcher<{
        newtab: void;
        selecttab: { tabId: string };
        closetab: { tabId: string; event: MouseEvent };
        scrollleft: void;
        scrollright: void;
        startRename: { tabId: string };
        confirmRename: { tabId: string };
        renameKey: { event: KeyboardEvent; tabId: string };
        toggleFocus: void;
        minimize: void;
        maximize: void;
        closeApp: void;
        panic: void;
        showWelcome: void;
    }>();

    function onTabClick(tabId: string) {
        dispatch('selecttab', { tabId });
    }

    function onCloseTab(tabId: string, e: MouseEvent) {
        dispatch('closetab', { tabId, event: e });
    }

    function onTabPickerSelect(tabId: string) {
        showTabPicker = false;
        dispatch('selecttab', { tabId });
    }

    let showTabPicker = false;

    // U1 — Tab preview hover: shows a tiny snapshot of the tab's last
    // few messages when hovering for >500ms. No invasive workspace refactor —
    // just a peek so the user can find the right tab faster.
    let hoverTabId: string | null = null;
    let hoverTimer: ReturnType<typeof setTimeout> | null = null;
    let previewVisible = false;
    let previewAnchorX = 0;
    let previewAnchorY = 0;

    function onTabEnter(ev: MouseEvent, tab: any) {
        if (hoverTimer) { clearTimeout(hoverTimer); hoverTimer = null; }
        hoverTabId = tab.id;
        const rect = (ev.currentTarget as HTMLElement).getBoundingClientRect();
        previewAnchorX = rect.left;
        previewAnchorY = rect.bottom + 6;
        hoverTimer = setTimeout(() => {
            previewVisible = true;
        }, 500);
    }

    function onTabLeave() {
        if (hoverTimer) { clearTimeout(hoverTimer); hoverTimer = null; }
        previewVisible = false;
        hoverTabId = null;
    }

    /** Compose the preview content from the last 3 messages of a tab. */
    function buildPreview(tab: any): { lines: Array<{ role: string; text: string }>; cwd?: string } {
        const msgs = Array.isArray(tab?.messages) ? tab.messages : [];
        const recent = msgs
            .filter((m: any) => m.rawContent && (m.role === 'user' || m.role === 'lucy'))
            .slice(-3)
            .map((m: any) => ({
                role: m.role,
                text: String(m.rawContent || '').slice(0, 120).replace(/\s+/g, ' ').trim(),
            }));
        return { lines: recent, cwd: tab?.cwd };
    }

    // ── Quick-win A — Rich tab header helpers ────────────────────────────
    // We surface 3 pieces of metadata per tab without bloating the strip:
    //   • A tiny 3-char model pill at the end of the title (gem/son/hai/gpt/oll)
    //   • The status dot color reflects activity (idle / processing / fork / stale)
    //   • The hover preview popover now includes model + tokens + cost + age
    // Keeping it minimal because the strip is only 34px tall and competing
    // for horizontal space with the tab title itself.

    /** 3-letter shorthand for the selected model. Stable across model
     *  catalog growth — we only key off the provider prefix. */
    function modelShort(model: string | undefined | null): string {
        if (!model) return '—';
        const m = model.toLowerCase();
        if (m.startsWith('local-') || m.startsWith('ollama')) return 'oll';
        if (m.startsWith('gemini'))   return 'gem';
        if (m.startsWith('claude-opus')) return 'opu';
        if (m.startsWith('claude-sonnet')) return 'son';
        if (m.startsWith('claude-haiku'))  return 'hai';
        if (m.startsWith('claude'))   return 'cla';
        if (m.startsWith('gpt-4'))    return 'gpt';
        if (m.startsWith('gpt'))      return 'gpt';
        if (m.includes('/'))          return 'nvi'; // NVIDIA NIM owner/model
        return m.slice(0, 3);
    }

    /** Status classification used for the leading dot color and the
     *  preview badge. Ordered by priority — processing wins over stale. */
    function tabState(tab: any): 'processing' | 'fork' | 'error' | 'stale' | 'idle' {
        if (!tab) return 'idle';
        if (tab.isProcessing) return 'processing';
        if (Array.isArray(tab.forkedTasks) && tab.forkedTasks.some((f: any) => f?.status === 'running')) return 'fork';
        if (tab._lastError) return 'error';
        // Stale = no activity in 30+ minutes. _lastActivityTs is updated
        // by the parent on every send/receive; fall back to created.
        const ts = tab._lastActivityTs || tab.createdAt || 0;
        if (ts && (Date.now() - ts) > 30 * 60 * 1000) return 'stale';
        return 'idle';
    }

    /** Rolled-up token + cost numbers for the preview popover. Pulled
     *  from the same _tokens / _cost counters the StatusBar already
     *  maintains per tab — no new tracking required. */
    function tabStats(tab: any): { tokens: number; cost: number; turns: number; ageMin: number } {
        const msgs = Array.isArray(tab?.messages) ? tab.messages : [];
        const turns = msgs.filter((m: any) => m.role === 'user' || m.role === 'lucy').length;
        const tokens = Number(tab?._tokensTotal ?? tab?._tokensIn ?? 0)
                     + Number(tab?._tokensOut ?? 0);
        const cost   = Number(tab?._costTotal ?? 0);
        const ts     = tab?._lastActivityTs || tab?.createdAt || Date.now();
        const ageMin = Math.max(0, Math.round((Date.now() - ts) / 60000));
        return { tokens, cost, turns, ageMin };
    }

    function fmtTokens(n: number): string {
        if (!n) return '0';
        if (n < 1000)   return String(n);
        if (n < 10000)  return (n / 1000).toFixed(1) + 'k';
        if (n < 1e6)    return Math.round(n / 1000) + 'k';
        return (n / 1e6).toFixed(1) + 'M';
    }
    function fmtCost(c: number): string {
        if (!c) return '$0';
        if (c < 0.01) return '$' + c.toFixed(4);
        if (c < 1)    return '$' + c.toFixed(3);
        return '$' + c.toFixed(2);
    }

    $: hoverTab = hoverTabId ? tabs.find(t => t.id === hoverTabId) : null;
    $: hoverPreview = hoverTab ? buildPreview(hoverTab) : { lines: [] };
    $: hoverStats   = hoverTab ? tabStats(hoverTab) : null;
    $: hoverState   = hoverTab ? tabState(hoverTab) : 'idle';
</script>

<header class="tb" data-tauri-drag-region>
    <!-- Brand / welcome toggle -->
    <div class="brand" role="button" tabindex="0"
         title="Ver capacidades de Lucy"
         on:click={() => dispatch('showWelcome')}
         on:keydown={(e) => e.key === 'Enter' && dispatch('showWelcome')}>
        <div class="bdot"></div>LUCY
    </div>

    <!-- Tab strip -->
    <div class="tabs-area" style="-webkit-app-region: no-drag;">
        {#if canScrollLeft}
        <button class="tab-scroll-btn" on:click={() => dispatch('scrollleft')} title="Pestañas anteriores">‹</button>
        {/if}

        <div id="tabs-list" bind:this={tabsListEl}>
            {#each tabs as tab (tab.id)}
                <div class="tab" class:active={activeTabId === tab.id}
                     role="button" tabindex="0"
                     on:click={() => onTabClick(tab.id)}
                     on:keydown
                     on:mouseenter={(e) => onTabEnter(e, tab)}
                     on:mouseleave={onTabLeave}>
                    <!-- Quick-win A — Status dot colored by tab activity.
                         Replaces the static purple dot so the user can scan
                         the strip and tell which tab is busy at a glance. -->
                    <div class="tdot tdot-{tabState(tab)}"
                         title={tabState(tab) === 'processing' ? (isEN ? 'Lucy is working' : 'Lucy procesando')
                              : tabState(tab) === 'fork'       ? (isEN ? 'Sub-agent running' : 'Sub-agente activo')
                              : tabState(tab) === 'error'      ? (isEN ? 'Last turn had an error' : 'Último turno con error')
                              : tabState(tab) === 'stale'      ? (isEN ? 'Inactive >30 min' : 'Inactiva >30 min')
                              : ''}></div>
                    {#if renamingTabId === tab.id}
                        <input
                            id="rename-{tab.id}"
                            class="tab-rename-input"
                            bind:value={renameValue}
                            on:keydown={(e) => dispatch('renameKey', { event: e, tabId: tab.id })}
                            on:blur={() => dispatch('confirmRename', { tabId: tab.id })}
                            on:click|stopPropagation
                        >
                    {:else}
                        <span class="tab-title-txt"
                              role="button" tabindex="0"
                              on:dblclick|stopPropagation={() => dispatch('startRename', { tabId: tab.id })}
                              title={`${tab.title}\n(${isEN ? 'Double-click to rename' : 'Doble clic para renombrar'})`}>
                            {tab.title}
                        </span>
                        <!-- Quick-win A — Model shorthand pill. 3-char code
                             keeps it from competing with the title for space. -->
                        {#if tab.selectedModel && activeTabId === tab.id}
                            <span class="tab-model-pill"
                                  title={`${isEN ? 'Active model' : 'Modelo activo'}: ${tab.selectedModel}`}>
                                {modelShort(tab.selectedModel)}
                            </span>
                        {/if}
                    {/if}
                    <span class="tx" role="button" tabindex="0"
                          on:click={(e) => onCloseTab(tab.id, e)}
                          on:keydown>✕</span>
                </div>
            {/each}
        </div>

        {#if canScrollRight}
        <button class="tab-scroll-btn" on:click={() => dispatch('scrollright')} title="Más pestañas">›</button>
        {/if}

        {#if tabs.length > 1}
        <div class="tab-picker-wrap">
            <button class="tab-picker-btn" title="Ver todas las terminales ({tabs.length})"
                    on:click={() => showTabPicker = !showTabPicker}>
                <svg width="11" height="11" viewBox="0 0 11 11" fill="currentColor">
                    <path d="M1 3h9v1H1zm0 3h9v1H1zm0 3h9v1H1z"/>
                </svg>
                {#if tabs.length > 1}<span class="tab-count">{tabs.length}</span>{/if}
            </button>

            {#if showTabPicker}
            <div class="tab-picker-backdrop" role="button" tabindex="-1" aria-label="Cerrar"
                 on:click={() => showTabPicker = false} on:keydown></div>
            <div class="tab-picker-menu">
                <div class="tab-picker-header">Terminales abiertas</div>
                {#each tabs as tab, i (tab.id)}
                    <div class="tab-picker-item" class:tpi-active={activeTabId === tab.id}
                         role="button" tabindex="0"
                         on:click={() => onTabPickerSelect(tab.id)}
                         on:keydown>
                        <div class="tpi-dot" class:tpi-dot-active={activeTabId === tab.id}></div>
                        <span class="tpi-num">{i + 1}</span>
                        <span class="tpi-title" role="button" tabindex="0"
                              on:dblclick|stopPropagation={() => { showTabPicker = false; tick().then(() => dispatch('startRename', { tabId: tab.id })); }}
                              title="Doble clic para renombrar">{tab.title}</span>
                        <button class="tpi-close" title="Cerrar"
                                on:click|stopPropagation={(e) => { onCloseTab(tab.id, e); if(tabs.length <= 1) showTabPicker = false; }}
                                on:keydown>✕</button>
                    </div>
                {/each}
            </div>
            {/if}
        </div>
        {/if}
    </div>

    <!-- Right side controls -->
    <div class="tb-btns">
        <button class="btn-new" title="Nueva terminal (Ctrl+T)" on:click={() => dispatch('newtab')}>+</button>
    </div>
    <div class="drag-sp" data-tauri-drag-region></div>
    <div class="win-controls">
        <button class="win-btn-icon panic-btn" on:click={() => dispatch('panic')}
                title={isEN ? 'Stop All Processes (Panic)' : 'Detener todo (Pánico)'}>
            <OctagonX size={14} stroke={2.2} />
        </button>
        <button class="win-btn-icon" on:click={() => dispatch('toggleFocus')}
                title={focusMode ? 'Ctrl+M — salir de focus' : 'Ctrl+M — modo focus'}>
            {focusMode ? '⊞' : '⊟'}
        </button>
        <div class="win-btn" role="button" tabindex="0" title="Minimizar"
             on:click={() => dispatch('minimize')} on:keydown>
            <svg width="11" height="11" viewBox="0 0 11 11" fill="currentColor"><path d="M11 5H0V6H11V5Z"/></svg>
        </div>
        <div class="win-btn" role="button" tabindex="0" title="Maximizar"
             on:click={() => dispatch('maximize')} on:keydown>
            <svg width="11" height="11" viewBox="0 0 11 11" fill="currentColor"><path d="M10 1H1V10H10V1ZM11 0V11H0V0H11Z" fill-rule="evenodd" clip-rule="evenodd"/></svg>
        </div>
        <div class="win-btn wc" role="button" tabindex="0" title="Cerrar"
             on:click={() => dispatch('closeApp')} on:keydown>
            <svg width="11" height="11" viewBox="0 0 11 11" fill="currentColor"><path d="M10.854 1.146L9.854 0.146L5.5 4.5L1.146 0.146L0.146 1.146L4.5 5.5L0.146 9.854L1.146 10.854L5.5 6.5L9.854 10.854L10.854 9.854L6.5 5.5L10.854 1.146Z"/></svg>
        </div>
    </div>
</header>

<!-- U1 — Tab hover preview popover. Fixed-positioned so it doesn't constrain the header. -->
{#if previewVisible && hoverTab}
    <div class="tab-preview-pop"
         style="left:{previewAnchorX}px; top:{previewAnchorY}px;"
         role="tooltip">
        <div class="tp-head">
            <span class="tp-title">{hoverTab.title || 'Tab'}</span>
            {#if hoverPreview.cwd}
                <span class="tp-cwd" title={hoverPreview.cwd}>{hoverPreview.cwd}</span>
            {/if}
        </div>
        <!-- Quick-win A — Stats strip: model + turns + tokens + cost + state.
             Single row of pills so the popover stays compact. -->
        {#if hoverStats}
            <div class="tp-stats">
                <span class="tp-pill tp-pill-model" title={hoverTab.selectedModel || ''}>
                    {modelShort(hoverTab.selectedModel)}
                </span>
                {#if hoverStats.turns > 0}
                    <span class="tp-pill" title={isEN ? 'Turns in this tab' : 'Turnos en esta pestaña'}>
                        {hoverStats.turns} {isEN ? 'turns' : 'turnos'}
                    </span>
                {/if}
                {#if hoverStats.tokens > 0}
                    <span class="tp-pill" title={isEN ? 'Tokens used in this tab' : 'Tokens usados en esta pestaña'}>
                        {fmtTokens(hoverStats.tokens)} tok
                    </span>
                {/if}
                {#if hoverStats.cost > 0}
                    <span class="tp-pill tp-pill-cost" title={isEN ? 'Cost accrued in this tab' : 'Costo acumulado en esta pestaña'}>
                        {fmtCost(hoverStats.cost)}
                    </span>
                {/if}
                {#if hoverState !== 'idle'}
                    <span class="tp-pill tp-pill-state tp-pill-{hoverState}">
                        {hoverState === 'processing' ? (isEN ? 'working' : 'trabajando')
                       : hoverState === 'fork'       ? (isEN ? 'fork active' : 'fork activo')
                       : hoverState === 'error'      ? (isEN ? 'error' : 'error')
                       : hoverState === 'stale'      ? (isEN ? `${hoverStats.ageMin} min idle` : `${hoverStats.ageMin} min`) : ''}
                    </span>
                {/if}
            </div>
        {/if}
        {#if hoverPreview.lines.length === 0}
            <div class="tp-empty">{isEN ? '(empty — no messages yet)' : '(vacía — sin mensajes)'}</div>
        {:else}
            {#each hoverPreview.lines as line}
                <div class="tp-line tp-{line.role}">
                    <span class="tp-role">{line.role === 'user' ? '›' : '◆'}</span>
                    <span class="tp-text">{line.text}</span>
                </div>
            {/each}
        {/if}
    </div>
{/if}

<style>
    .tb{display:flex;align-items:center;height:38px;background:#0b0d14;border-bottom:1px solid var(--bdr);padding:0 0 0 14px;user-select:none;-webkit-app-region:drag;flex-shrink:0;}
    :global(#tabs-list,.win-btn,.btn-new,.tb-btns,.tabs-area){-webkit-app-region:no-drag;}
    .brand{display:flex;align-items:center;gap:7px;font-size:12px;font-weight:700;color:var(--acc);letter-spacing:1px;margin-right:8px;flex-shrink:0;cursor:pointer;opacity:1;transition:opacity .15s;-webkit-app-region:no-drag;}
    .brand:hover{opacity:.75;}
    .bdot{width:7px;height:7px;border-radius:50%;background:var(--acc);box-shadow:0 0 6px rgba(16,185,129,0.5);}
    .tabs-area{display:flex;align-items:flex-end;max-width:480px;min-width:0;height:38px;position:relative;}
    :global(#tabs-list){display:flex;gap:1px;flex:1;max-width:480px;height:38px;align-items:flex-end;overflow-x:auto;scroll-behavior:smooth;min-width:0;}
    :global(#tabs-list::-webkit-scrollbar){display:none;}
    :global(.tab){display:flex;align-items:center;gap:6px;padding:0 12px;height:34px;font-size:12px;color:var(--txt2);cursor:pointer;border:1px solid transparent;border-bottom:none;border-radius:6px 6px 0 0;margin-top:4px;transition:0.15s;white-space:nowrap;flex-shrink:0;}
    :global(.tab:hover){background:rgba(255,255,255,0.03);color:#94a3b8;}
    :global(.tab.active){background:var(--bg2);color:var(--acc);border-color:var(--bdr);border-top:2px solid var(--acc);}
    :global(.tab-title-txt){max-width:170px;overflow:hidden;text-overflow:ellipsis;cursor:pointer;white-space:nowrap;}
    :global(.tab-rename-input){background:rgba(0,0,0,.4);border:1px solid var(--acc-b);border-radius:3px;color:var(--acc);font-size:12px;font-family:inherit;padding:1px 5px;width:110px;outline:none;-webkit-app-region:no-drag;}
    :global(.tdot){width:6px;height:6px;border-radius:50%;background:var(--purple);opacity:.6;flex-shrink:0;transition:background .2s, box-shadow .2s, opacity .2s;}
    /* Quick-win A — Status dot variants. Idle stays purple (default),
       others get themed colors with a gentle glow so the user spots
       active tabs without reading anything. */
    :global(.tdot-processing){background:var(--blue, #3b9eff)!important;opacity:1!important;box-shadow:0 0 6px rgba(59,158,255,.55);animation:tdot-pulse 1.2s ease-in-out infinite;}
    :global(.tdot-fork){background:#f59e0b!important;opacity:1!important;box-shadow:0 0 6px rgba(245,158,11,.5);}
    :global(.tdot-error){background:#ef4444!important;opacity:1!important;box-shadow:0 0 6px rgba(239,68,68,.6);}
    :global(.tdot-stale){background:var(--txt3, #475569)!important;opacity:.4!important;box-shadow:none;}
    @keyframes tdot-pulse{0%,100%{transform:scale(1);}50%{transform:scale(1.35);}}
    /* Model shorthand pill — only on the active tab to avoid clutter. */
    :global(.tab-model-pill){
        font-family:var(--mono, monospace);
        font-size:9px;
        font-weight:700;
        color:var(--acc, #10b981);
        background:rgba(16,185,129,.10);
        border:1px solid rgba(16,185,129,.25);
        padding:1px 5px;
        border-radius:8px;
        line-height:1.5;
        letter-spacing:.5px;
        text-transform:uppercase;
        flex-shrink:0;
        opacity:.85;
    }
    :global(.tab.active .tab-model-pill){opacity:1;}
    :global(.tx){font-size:10px;color:transparent;padding:1px 3px;border-radius:3px;flex-shrink:0;}
    :global(.tab:hover .tx){color:var(--txt2);}
    :global(.tx:hover){color:var(--red)!important;background:rgba(255,68,68,.1);}
    .tab-scroll-btn{background:rgba(10,15,20,0.9);border:none;border-bottom:none;color:var(--txt2);cursor:pointer;font-size:16px;width:22px;height:34px;display:flex;align-items:center;justify-content:center;transition:.15s;flex-shrink:0;align-self:flex-end;border-radius:4px 4px 0 0;}
    .tab-scroll-btn:hover{background:rgba(16,185,129,0.08);color:var(--acc);}
    .tab-picker-wrap{position:relative;align-self:flex-end;flex-shrink:0;}
    .tab-picker-btn{background:rgba(10,15,20,0.9);border:1px solid var(--bdr);border-bottom:none;border-radius:5px 5px 0 0;color:var(--txt2);cursor:pointer;height:30px;min-width:28px;display:flex;align-items:center;justify-content:center;gap:4px;padding:0 7px;font-size:11px;transition:.15s;align-self:flex-end;margin-bottom:0;}
    .tab-picker-btn:hover{background:rgba(16,185,129,0.07);color:var(--acc);border-color:var(--acc-b);}
    .tab-count{font-size:10px;font-weight:700;color:var(--acc);background:rgba(16,185,129,0.12);padding:1px 5px;border-radius:8px;line-height:1.4;}
    .tab-picker-backdrop{position:fixed;inset:0;z-index:var(--z-tab-pick);}
    .tab-picker-menu{position:absolute;top:calc(100% + 2px);right:0;background:rgba(12,18,28,0.98);border:1px solid var(--bdr2);border-radius:8px;min-width:220px;z-index:calc(var(--z-tab-pick) + 1);box-shadow:0 8px 32px rgba(0,0,0,0.6);overflow:hidden;}
    .tab-picker-header{font-size:10px;color:#334155;letter-spacing:1px;text-transform:uppercase;font-weight:700;padding:8px 12px 6px;border-bottom:1px solid var(--bdr);}
    .tab-picker-item{display:flex;align-items:center;gap:8px;padding:8px 12px;cursor:pointer;font-size:12px;color:var(--txt2);transition:.12s;border-bottom:1px solid rgba(26,32,48,0.4);}
    .tab-picker-item:last-child{border-bottom:none;}
    .tab-picker-item:hover{background:rgba(16,185,129,0.05);color:var(--txt);}
    .tab-picker-item.tpi-active{background:rgba(16,185,129,0.07);color:var(--acc);}
    .tpi-dot{width:5px;height:5px;border-radius:50%;background:var(--bdr2);flex-shrink:0;}
    .tpi-dot.tpi-dot-active{background:var(--acc);box-shadow:0 0 4px rgba(16,185,129,0.4);}
    .tpi-num{font-size:10px;color:#334155;min-width:14px;text-align:center;flex-shrink:0;}
    .tpi-title{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
    .tpi-close{background:none;border:none;color:transparent;cursor:pointer;font-size:10px;padding:1px 4px;border-radius:3px;transition:.12s;flex-shrink:0;}
    .tab-picker-item:hover .tpi-close{color:var(--txt2);}
    .tpi-close:hover{color:var(--red)!important;background:rgba(255,68,68,.1);}
    .btn-new{background:var(--bg4);border:1px solid var(--bdr);color:var(--acc);border-radius:5px;width:24px;height:24px;display:flex;align-items:center;justify-content:center;cursor:pointer;font-size:15px;transition:.15s;}
    .btn-new:hover{background:var(--bdr);color:white;}
    .tb-btns{display:flex;align-items:center;gap:6px;padding:0 8px;}
    .drag-sp{flex-grow:1;height:38px;}
    .win-controls{display:flex;height:38px;}
    .win-btn{width:46px;height:100%;display:flex;align-items:center;justify-content:center;cursor:pointer;color:var(--txt2);transition:.15s;}
    .win-btn:hover{background:rgba(255,255,255,.07);color:white;}
    .wc:hover{background:#e81123!important;color:white!important;}
    .win-btn-icon{background:transparent;border:none;width:46px;height:100%;display:flex;align-items:center;justify-content:center;cursor:pointer;color:var(--txt2);transition:.15s;}
    .win-btn-icon:hover{background:rgba(255,255,255,.07);color:white;}
    :global(.panic-btn){color:#ef4444 !important;}
    :global(.panic-btn:hover){color:#f87171 !important;background:rgba(239,68,68,.12) !important;}
    :global(:root:not(.light)) .tb{background:transparent !important;border-bottom:1px solid var(--border-glass, var(--bdr)) !important;}
    :global(:root:not(.light)) :global(.tab.active){background:var(--sidebar-overlay, var(--bg2)) !important;backdrop-filter:blur(8px);-webkit-backdrop-filter:blur(8px);border-color:var(--border-glass, var(--bdr)) !important;}

    /* ── U1 — Tab hover preview popover ─────────────────────────────────
       Fixed-positioned so it floats over the chat without disturbing layout.
       Pointer-events: none so the user can move into the chat area to dismiss
       without clicking through it. */
    .tab-preview-pop {
        position: fixed;
        z-index: 9000;
        min-width: 240px;
        max-width: 360px;
        background: var(--bg-card, #161b22);
        border: 1px solid var(--border-color, #1e293b);
        border-radius: 6px;
        padding: 8px 10px;
        font-family: var(--font-mono, monospace);
        font-size: 11px;
        line-height: 1.5;
        color: var(--text-main, #e2e8f0);
        box-shadow: 0 8px 24px -8px rgba(0,0,0,0.55),
                    0 0 0 1px rgba(16,185,129,0.10);
        pointer-events: none;
        animation: tp-fade-in 140ms cubic-bezier(0.16, 1, 0.3, 1);
    }
    @keyframes tp-fade-in {
        from { opacity: 0; transform: translateY(-2px); }
        to   { opacity: 1; transform: translateY(0); }
    }
    .tp-head {
        display: flex;
        align-items: center;
        gap: 8px;
        padding-bottom: 6px;
        margin-bottom: 6px;
        border-bottom: 1px solid rgba(255,255,255,0.06);
    }
    .tp-title {
        flex: 1;
        font-weight: 700;
        color: var(--acc, #10b981);
        font-size: 11px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .tp-cwd {
        font-size: 9px;
        color: var(--text-muted, #64748b);
        max-width: 140px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .tp-line {
        display: flex;
        gap: 6px;
        align-items: flex-start;
        padding: 2px 0;
    }
    .tp-role {
        flex-shrink: 0;
        font-weight: 700;
        opacity: 0.65;
    }
    .tp-lucy .tp-role { color: var(--acc); }
    .tp-user .tp-role { color: var(--blue, #3b9eff); }
    .tp-text {
        overflow: hidden;
        text-overflow: ellipsis;
        display: -webkit-box;
        -webkit-line-clamp: 2;
        line-clamp: 2;
        -webkit-box-orient: vertical;
        opacity: 0.85;
    }
    .tp-empty {
        font-style: italic;
        color: var(--text-muted);
        font-size: 10px;
    }
    /* Quick-win A — Stats pills in the hover preview popover. */
    .tp-stats {
        display: flex;
        gap: 5px;
        flex-wrap: wrap;
        padding-bottom: 6px;
        margin-bottom: 6px;
        border-bottom: 1px solid rgba(255,255,255,0.06);
    }
    .tp-pill {
        font-size: 9.5px;
        font-weight: 600;
        padding: 1px 6px;
        border-radius: 8px;
        background: rgba(255,255,255,0.05);
        color: var(--txt2, #94a3b8);
        line-height: 1.5;
        letter-spacing: .2px;
        white-space: nowrap;
    }
    .tp-pill-model {
        font-family: var(--mono, monospace);
        color: var(--acc, #10b981);
        background: rgba(16,185,129,.10);
        text-transform: uppercase;
    }
    .tp-pill-cost {
        font-family: var(--mono, monospace);
        color: #fbbf24;
        background: rgba(251,191,36,.10);
    }
    .tp-pill-state.tp-pill-processing { color: var(--blue, #3b9eff); background: rgba(59,158,255,.12); }
    .tp-pill-state.tp-pill-fork       { color: #f59e0b; background: rgba(245,158,11,.12); }
    .tp-pill-state.tp-pill-error      { color: #ef4444; background: rgba(239,68,68,.12); }
    .tp-pill-state.tp-pill-stale      { color: var(--txt3, #475569); background: rgba(71,85,105,.15); }
</style>
