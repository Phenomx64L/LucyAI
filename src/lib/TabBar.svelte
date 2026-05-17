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
                     on:keydown>
                    <div class="tdot"></div>
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
            <OctagonX size={14} strokeWidth={2.2} />
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
    :global(.tdot){width:6px;height:6px;border-radius:50%;background:var(--purple);opacity:.6;flex-shrink:0;}
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
</style>
