<!--
  ── AgentChapterView.svelte — U3 Frontier UX ────────────────────────────────

  Renders the result of a multi-step agent task as a "chapter book" instead
  of a vertical spam of tool cards. Goal: restore narrative coherence for
  long investigations (e.g. SRE incident workflows).

  Structure:
    • Cover: title + objective + total elapsed
    • Chapters: one per logical phase (Thought, Step 1..N, Conclusion)
    • Side index for navigation
    • Toggle to flip back to the linear "raw" view at any time

  Inputs:
    - tab: the tab object (so we can read agentToolCards, stepsHtml etc.)
    - displayText: the final text answer Lucy produced
    - title: user prompt (cover heading)
    - elapsed: ms since start (so we can display "Investigation · 47s")

  Design notes:
    - Component is presentational. Heavy parsing of step content is done
      upstream by +page.svelte and passed in via `steps`.
    - Uses ROS-style chapter markers (◆ chapter break) for the index.
    - Keyboard: arrow keys to navigate chapters when focused.
-->
<script context="module" lang="ts">
    export interface ChapterStep {
        /** Numeric step index, 1-based. */
        index: number;
        /** Short label, e.g. "Read file main.rs" or "Web search". */
        label: string;
        /** Status badge color. */
        status: 'ok' | 'error' | 'pending' | 'info';
        /** HTML body. Already DOMPurified upstream. */
        bodyHtml: string;
        /** Optional rationale Lucy gave before doing it (THOUGHT). */
        rationale?: string;
    }
</script>
<script lang="ts">
    import { createEventDispatcher } from 'svelte';

    export let title: string = 'Agent task';
    export let objective: string = '';
    export let elapsedMs: number = 0;
    export let steps: ChapterStep[] = [];
    export let finalHtml: string = '';
    export let isEN: boolean = false;

    const dispatch = createEventDispatcher<{ flip: { mode: 'linear' | 'chapter' } }>();

    let activeIdx: number = 0;
    /** Total chapters: cover + steps + conclusion (if finalHtml present) */
    $: chapterCount = 1 + steps.length + (finalHtml ? 1 : 0);

    function setActive(i: number) {
        activeIdx = Math.max(0, Math.min(i, chapterCount - 1));
    }

    function fmtElapsed(ms: number): string {
        const s = Math.round(ms / 1000);
        if (s < 60) return `${s}s`;
        const m = Math.floor(s / 60);
        const r = s - m * 60;
        return `${m}m ${r}s`;
    }

    function statusGlyph(s: ChapterStep['status']): string {
        switch (s) {
            case 'ok':      return '✓';
            case 'error':   return '✕';
            case 'pending': return '◐';
            default:        return '·';
        }
    }
</script>

<div class="chap-book" role="document" aria-label="Agent investigation chapters">
    <!-- ── Side index ── -->
    <nav class="chap-index" aria-label="Chapter navigation">
        <button class="chap-tab" class:active={activeIdx === 0} on:click={() => setActive(0)} title={isEN ? 'Cover' : 'Portada'}>
            <span class="chap-num">◆</span>
            <span class="chap-label">{isEN ? 'Cover' : 'Portada'}</span>
        </button>
        {#each steps as st, i (st.index)}
            <button class="chap-tab"
                    class:active={activeIdx === i + 1}
                    class:err={st.status === 'error'}
                    on:click={() => setActive(i + 1)}
                    title={st.label}>
                <span class="chap-num">{st.index}</span>
                <span class="chap-label">{st.label}</span>
                <span class="chap-status">{statusGlyph(st.status)}</span>
            </button>
        {/each}
        {#if finalHtml}
            <button class="chap-tab" class:active={activeIdx === chapterCount - 1} on:click={() => setActive(chapterCount - 1)}>
                <span class="chap-num">◇</span>
                <span class="chap-label">{isEN ? 'Conclusion' : 'Conclusión'}</span>
            </button>
        {/if}
        <div class="chap-meta">
            <span class="chap-elapsed">{fmtElapsed(elapsedMs)}</span>
            <button class="chap-flip"
                    on:click={() => dispatch('flip', { mode: 'linear' })}
                    title={isEN ? 'Switch to linear view' : 'Cambiar a vista lineal'}>
                {isEN ? '↻ Linear' : '↻ Lineal'}
            </button>
        </div>
    </nav>

    <!-- ── Page content ── -->
    <article class="chap-page">
        {#if activeIdx === 0}
            <header class="chap-cover">
                <div class="chap-cover-eyebrow">{isEN ? 'Investigation' : 'Investigación'}</div>
                <h2 class="chap-cover-title">{title}</h2>
                {#if objective}
                    <p class="chap-cover-objective">{objective}</p>
                {/if}
                <div class="chap-cover-meta">
                    <span>{steps.length} {isEN ? 'steps' : 'pasos'}</span>
                    <span class="dot">·</span>
                    <span>{fmtElapsed(elapsedMs)}</span>
                </div>
            </header>
        {:else if activeIdx <= steps.length}
            {@const st = steps[activeIdx - 1]}
            <header class="chap-step-head">
                <span class="chap-step-num">{st.index}</span>
                <h3 class="chap-step-title">{st.label}</h3>
                <span class="chap-step-badge chap-step-{st.status}">{statusGlyph(st.status)} {st.status.toUpperCase()}</span>
            </header>
            {#if st.rationale}
                <aside class="chap-rationale">
                    <span class="chap-rationale-mark">{isEN ? 'Why' : 'Por qué'}</span>
                    <span>{st.rationale}</span>
                </aside>
            {/if}
            <section class="chap-body">{@html st.bodyHtml}</section>
        {:else if finalHtml}
            <header class="chap-conclusion-head">
                <span class="chap-conclusion-mark">◇</span>
                <h3>{isEN ? 'Conclusion' : 'Conclusión'}</h3>
            </header>
            <section class="chap-body">{@html finalHtml}</section>
        {/if}

        <!-- Bottom nav -->
        <footer class="chap-nav">
            <button class="chap-nav-btn"
                    disabled={activeIdx === 0}
                    on:click={() => setActive(activeIdx - 1)}>← {isEN ? 'Prev' : 'Anterior'}</button>
            <span class="chap-pageno">{activeIdx + 1} / {chapterCount}</span>
            <button class="chap-nav-btn"
                    disabled={activeIdx === chapterCount - 1}
                    on:click={() => setActive(activeIdx + 1)}>{isEN ? 'Next' : 'Siguiente'} →</button>
        </footer>
    </article>
</div>

<style>
    .chap-book {
        display: grid;
        grid-template-columns: 200px 1fr;
        gap: 0;
        background: var(--bg-card, #161b22);
        border: 1px solid var(--border-color, #1e293b);
        border-radius: 8px;
        overflow: hidden;
        margin: 8px 0;
        /* Locked dimensions: no matter how long any single step's output is,
           the chapter card never grows beyond 520px tall. Bodies that don't
           fit scroll internally — see .chap-body. */
        height: 520px;
        min-height: 320px;
        max-height: 70vh;     /* still respect viewport on tiny windows */
    }
    .chap-index {
        display: flex;
        flex-direction: column;
        gap: 2px;
        padding: 10px 6px;
        background: rgba(0,0,0,0.20);
        border-right: 1px solid var(--border-color, #1e293b);
        overflow-y: auto;
        /* Fill the book height — no separate cap means sidebar tracks the
           page perfectly. */
        height: 100%;
        min-height: 0;
    }
    .chap-tab {
        display: grid;
        grid-template-columns: 22px 1fr auto;
        align-items: center;
        gap: 6px;
        padding: 6px 8px;
        font-family: var(--font-mono);
        font-size: 11px;
        color: var(--text-muted);
        background: transparent;
        border: 1px solid transparent;
        border-radius: 4px;
        cursor: pointer;
        text-align: left;
        transition: background var(--motion-fast) var(--ease-out),
                    color    var(--motion-fast) var(--ease-out),
                    border-color var(--motion-fast) var(--ease-out);
    }
    .chap-tab:hover {
        background: rgba(255,255,255,0.04);
        color: var(--text-main);
    }
    .chap-tab.active {
        background: rgba(16,185,129,0.10);
        border-color: rgba(16,185,129,0.30);
        color: var(--text-bright);
    }
    .chap-tab.err {
        color: var(--red, #ef4444);
    }
    .chap-num {
        font-weight: 700;
        font-size: 10px;
        color: var(--accent, #10b981);
        opacity: 0.75;
    }
    .chap-tab.err .chap-num { color: var(--red, #ef4444); }
    .chap-label {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .chap-status {
        font-size: 11px;
        opacity: 0.65;
    }
    .chap-meta {
        margin-top: auto;
        padding: 8px 6px 4px;
        border-top: 1px solid rgba(255,255,255,0.06);
        display: flex;
        flex-direction: column;
        gap: 6px;
        align-items: stretch;
    }
    .chap-elapsed {
        font-family: var(--font-mono);
        font-size: 10px;
        color: var(--text-muted);
        text-align: center;
        font-variant-numeric: tabular-nums;
    }
    .chap-flip {
        font-family: var(--font-mono);
        font-size: 10px;
        padding: 4px 6px;
        background: transparent;
        color: var(--text-muted);
        border: 1px solid rgba(255,255,255,0.10);
        border-radius: 4px;
        cursor: pointer;
        transition: background var(--motion-fast) var(--ease-out);
    }
    .chap-flip:hover {
        background: rgba(255,255,255,0.05);
        color: var(--text-main);
    }

    /* ── Page area ─────────────────────────────────────────── */
    .chap-page {
        padding: 18px 22px;
        display: grid;
        /* Header row · scrollable body · footer row. The middle row gets
           1fr — it expands or contracts to fill whatever the cover/header
           and the nav footer don't use. Inside it the body scrolls. */
        grid-template-rows: auto 1fr auto;
        gap: 14px;
        min-height: 0;
        overflow: hidden;
    }
    .chap-cover {
        display: flex;
        flex-direction: column;
        gap: 8px;
        padding: 16px 0;
    }
    .chap-cover-eyebrow {
        font-family: var(--font-mono);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 2px;
        color: var(--accent);
        text-transform: uppercase;
    }
    .chap-cover-title {
        font-size: 18px;
        font-weight: 700;
        line-height: 1.3;
        margin: 0;
        color: var(--text-bright);
    }
    .chap-cover-objective {
        font-size: 12px;
        line-height: 1.5;
        color: var(--text-main);
        margin: 0;
        padding-left: 10px;
        border-left: 2px solid var(--accent-border);
    }
    .chap-cover-meta {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-family: var(--font-mono);
        font-size: 11px;
        color: var(--text-muted);
    }
    .dot { opacity: 0.5; }

    .chap-step-head {
        display: flex;
        align-items: center;
        gap: 10px;
        padding-bottom: 8px;
        border-bottom: 1px solid var(--border-color);
    }
    .chap-step-num {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 24px;
        height: 24px;
        border-radius: 50%;
        font-family: var(--font-mono);
        font-size: 11px;
        font-weight: 700;
        background: rgba(16,185,129,0.12);
        border: 1px solid var(--accent-border);
        color: var(--accent);
    }
    .chap-step-title {
        flex: 1;
        font-size: 13px;
        font-weight: 600;
        margin: 0;
        color: var(--text-bright);
    }
    .chap-step-badge {
        padding: 2px 8px;
        font-family: var(--font-mono);
        font-size: 10px;
        font-weight: 700;
        border-radius: 3px;
        letter-spacing: 0.4px;
    }
    .chap-step-ok      { background: rgba(16,185,129,0.12); color: var(--accent); }
    .chap-step-error   { background: rgba(239,68,68,0.12);  color: var(--red); }
    .chap-step-pending { background: rgba(59,158,255,0.12); color: var(--blue); }
    .chap-step-info    { background: rgba(167,139,250,0.12); color: var(--purple); }

    .chap-rationale {
        display: flex;
        gap: 8px;
        align-items: baseline;
        padding: 8px 12px;
        background: rgba(255,255,255,0.025);
        border-left: 2px solid var(--blue);
        border-radius: 3px;
        font-size: 12px;
        line-height: 1.5;
        color: var(--text-main);
    }
    .chap-rationale-mark {
        font-family: var(--font-mono);
        font-size: 10px;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.6px;
        color: var(--blue);
        flex-shrink: 0;
    }

    .chap-conclusion-head {
        display: flex;
        align-items: center;
        gap: 10px;
        padding-bottom: 8px;
        border-bottom: 1px solid var(--border-color);
    }
    .chap-conclusion-head h3 {
        margin: 0;
        font-size: 14px;
        font-weight: 700;
        color: var(--accent);
    }
    .chap-conclusion-mark {
        font-size: 16px;
        color: var(--accent);
    }

    .chap-body {
        font-size: 13px;
        line-height: 1.55;
        color: var(--text-main);
        /* Internal scroll: any chapter whose content overflows scrolls
           ITSELF instead of growing the card. Combined with grid 1fr above
           this keeps every chapter at the same visual size. */
        overflow-y: auto;
        overflow-x: auto;
        min-height: 0;
        /* Subtle gradient mask so the cut-off at the bottom looks intentional */
        mask-image: linear-gradient(to bottom, black calc(100% - 12px), transparent);
        -webkit-mask-image: linear-gradient(to bottom, black calc(100% - 12px), transparent);
        padding-right: 4px;
    }
    /* Slim scrollbar so it doesn't dominate when scrolling tool output */
    .chap-body::-webkit-scrollbar { width: 8px; height: 8px; }
    .chap-body::-webkit-scrollbar-track { background: transparent; }
    .chap-body::-webkit-scrollbar-thumb {
        background: rgba(255,255,255,0.08);
        border-radius: 4px;
    }
    .chap-body::-webkit-scrollbar-thumb:hover { background: rgba(255,255,255,0.18); }
    .chap-body :global(pre) {
        margin: 6px 0;
        padding: 10px 12px;
        background: rgba(0,0,0,0.35);
        border-radius: 4px;
        font-size: 11px;
        line-height: 1.45;
        /* Pre blocks can be wider than the page — let them scroll horizontally
           but never grow vertically beyond their content. */
        overflow-x: auto;
        white-space: pre;
        max-width: 100%;
    }
    .chap-body :global(pre)::-webkit-scrollbar { height: 6px; }
    .chap-body :global(code) {
        font-family: var(--font-mono);
        font-size: 11px;
    }

    .chap-nav {
        margin-top: auto;
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding-top: 10px;
        border-top: 1px solid var(--border-color);
    }
    .chap-nav-btn {
        font-family: var(--font-mono);
        font-size: 11px;
        padding: 4px 10px;
        background: transparent;
        color: var(--text-muted);
        border: 1px solid rgba(255,255,255,0.10);
        border-radius: 4px;
        cursor: pointer;
        transition: background var(--motion-fast) var(--ease-out);
    }
    .chap-nav-btn:hover:not(:disabled) {
        background: rgba(255,255,255,0.05);
        color: var(--text-main);
    }
    .chap-nav-btn:disabled {
        opacity: 0.35;
        cursor: not-allowed;
    }
    .chap-pageno {
        font-family: var(--font-mono);
        font-size: 11px;
        color: var(--text-muted);
        font-variant-numeric: tabular-nums;
    }

    @media (max-width: 720px) {
        .chap-book { grid-template-columns: 1fr; }
        .chap-index {
            flex-direction: row;
            overflow-x: auto;
            max-height: none;
            border-right: none;
            border-bottom: 1px solid var(--border-color);
        }
        .chap-tab { min-width: 100px; }
        .chap-meta { flex-direction: row; margin-left: auto; border-top: none; }
    }
</style>
