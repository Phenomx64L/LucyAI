<!--
  XtermPane.svelte — v1.7.100 (Option D1)

  Embeds xterm.js bound to the Rust singleton PTY (commands/pty.rs).
  Lifecycle:
    1. onMount → import xterm + FitAddon, mount the terminal, fit,
       open the PTY with our col/row count.
    2. Listen for `pty:data` → base64-decode → write to xterm.
    3. xterm onData → invoke('pty_write').
    4. ResizeObserver on the wrapper → fit + invoke('pty_resize').
    5. onDestroy → invoke('pty_close') only when this component is
       genuinely going away (e.g. parent unmounts), NOT on a panel
       toggle — see `keepAlive` prop.

  We intentionally keep this component DOM-isolated: the parent passes
  in just visibility + sizing, and we do everything else internally.
  That keeps the PTY's lifecycle a property of this single component
  instead of leaking across the page-level state machine.
-->
<script lang="ts">
    import { onMount, onDestroy } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { listen, type UnlistenFn } from '@tauri-apps/api/event';

    /** If true, do NOT close the PTY when this component is destroyed.
     *  Lets the side panel toggle off the UI while leaving the shell
     *  process running so the next "open" lands the user back at their
     *  prompt with full scrollback. Default true — destroying a panel
     *  shouldn't kill a long-running shell. */
    export let keepAlive: boolean = true;

    /** English locale (placeholder text). */
    export let isEN: boolean = false;

    let host: HTMLDivElement;
    let term: any = null;        // xterm.Terminal instance
    let fitAddon: any = null;
    let unlistenData: UnlistenFn | null = null;
    let unlistenExit: UnlistenFn | null = null;
    let resizeObs: ResizeObserver | null = null;
    let initError = '';
    let opened = false;

    /** Throttle PTY resize calls — fit() can fire 5-10x during a
     *  drag-resize. We coalesce to the trailing dimensions. */
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;

    async function safeFit() {
        if (!fitAddon || !term) return;
        try {
            fitAddon.fit();
            if (resizeTimer) clearTimeout(resizeTimer);
            resizeTimer = setTimeout(async () => {
                try {
                    await invoke('pty_resize', { cols: term.cols, rows: term.rows });
                } catch {
                    // PTY may not be open yet — caller retries on next observer tick.
                }
                resizeTimer = null;
            }, 80);
        } catch {
            // fit() throws if the host has zero dimensions (panel
            // hidden via display:none). Silently skip.
        }
    }

    /** Decode base64 → Uint8Array. Browser-native atob is sync and
     *  fast enough for typical PTY chunks (4 KB). xterm's write()
     *  accepts both strings and Uint8Array. */
    function decode(b64: string): Uint8Array {
        const raw = atob(b64);
        const out = new Uint8Array(raw.length);
        for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
        return out;
    }

    onMount(async () => {
        try {
            // Dynamic import keeps xterm out of the initial app bundle —
            // the side panel is opt-in and many sessions never open it.
            const { Terminal } = await import('@xterm/xterm');
            const { FitAddon } = await import('@xterm/addon-fit');
            // Stylesheet for xterm — also dynamically imported.
            await import('@xterm/xterm/css/xterm.css');

            term = new Terminal({
                fontFamily: 'JetBrains Mono, Cascadia Code, Consolas, monospace',
                fontSize: 12,
                lineHeight: 1.18,
                cursorBlink: true,
                allowProposedApi: true,
                scrollback: 5000,
                theme: {
                    // Match Lucy's dark surface so the embed reads as
                    // part of the app, not a foreign element.
                    background: '#060810',
                    foreground: '#e2e8f0',
                    cursor:     '#10b981',
                    cursorAccent: '#0d1117',
                    selectionBackground: 'rgba(16,185,129,0.30)',
                    black:   '#1e293b',
                    red:     '#ef4444',
                    green:   '#10b981',
                    yellow:  '#f59e0b',
                    blue:    '#3b9eff',
                    magenta: '#a78bfa',
                    cyan:    '#22d3ee',
                    white:   '#e2e8f0',
                },
            });
            fitAddon = new FitAddon();
            term.loadAddon(fitAddon);
            term.open(host);
            await safeFit();

            // Stream data → terminal.
            unlistenData = await listen<{ bytes: string }>('pty:data', (e) => {
                if (!term) return;
                try { term.write(decode(e.payload.bytes)); } catch { /* ignore */ }
            });
            // Backend signalled the child process exited.
            unlistenExit = await listen<{ code: number; err?: string }>('pty:exit', (e) => {
                if (!term) return;
                const why = e.payload.err ? ` (${e.payload.err})` : '';
                term.write(`\r\n\x1b[2m── shell exited code ${e.payload.code}${why} ──\x1b[0m\r\n`);
                opened = false;
            });

            // User keystrokes → backend.
            term.onData((s: string) => {
                invoke('pty_write', { data: s }).catch(() => {
                    // Best-effort — if PTY is gone, the exit banner
                    // already explained why.
                });
            });

            // Open the PTY with the freshly measured dimensions.
            await invoke('pty_open', { cols: term.cols, rows: term.rows });
            opened = true;

            // Re-fit on host resize.
            resizeObs = new ResizeObserver(() => safeFit());
            resizeObs.observe(host);

            // Tiny nudge so the prompt actually paints on the first
            // frame. Most shells emit their prompt asynchronously after
            // process start; a newline kicks the render loop.
            setTimeout(() => term?.focus(), 60);
        } catch (err: any) {
            initError = String(err?.message || err);
        }
    });

    onDestroy(async () => {
        if (resizeTimer) { clearTimeout(resizeTimer); resizeTimer = null; }
        if (unlistenData) { try { unlistenData(); } catch { /* ignored */ } }
        if (unlistenExit) { try { unlistenExit(); } catch { /* ignored */ } }
        if (resizeObs)    { try { resizeObs.disconnect(); } catch { /* ignored */ } }
        if (term)         { try { term.dispose(); } catch { /* ignored */ } }
        if (!keepAlive) {
            try { await invoke('pty_close'); } catch { /* ignored */ }
        }
    });
</script>

<div class="xterm-pane-wrap">
    {#if initError}
        <div class="xterm-pane-err">
            {isEN ? 'Terminal failed to start' : 'Falló el arranque de la terminal'}:
            <code>{initError}</code>
        </div>
    {/if}
    <div class="xterm-pane-host" bind:this={host}></div>
    {#if !opened && !initError}
        <div class="xterm-pane-boot">
            {isEN ? '· starting shell ·' : '· iniciando shell ·'}
        </div>
    {/if}
</div>

<style>
    .xterm-pane-wrap {
        position: relative;
        width: 100%;
        height: 100%;
        background: #060810;
        overflow: hidden;
        display: flex;
        flex-direction: column;
    }
    .xterm-pane-host {
        flex: 1;
        min-height: 0;
        padding: 6px 4px 4px 8px;
    }
    .xterm-pane-err {
        padding: 10px 14px;
        background: rgba(239, 68, 68, 0.10);
        color: var(--red, #ef4444);
        font-family: var(--font-mono, monospace);
        font-size: 12px;
        border-bottom: 1px solid rgba(239, 68, 68, 0.30);
    }
    .xterm-pane-err code {
        display: block;
        margin-top: 4px;
        opacity: 0.85;
        font-size: 11px;
        white-space: pre-wrap;
    }
    .xterm-pane-boot {
        position: absolute;
        inset: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        font-family: var(--font-mono, monospace);
        font-size: 11px;
        color: var(--text-muted, #64748b);
        pointer-events: none;
    }
</style>
