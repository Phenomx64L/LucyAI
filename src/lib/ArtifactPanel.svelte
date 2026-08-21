<!-- ArtifactPanel.svelte — v1.7.79 ────────────────────────────────────────
     Claude-style side panel for long code blocks and documents.

     When Lucy emits content past the readability threshold (code > 30
     lines or markdown > 1500 chars), the operator can promote it to an
     artifact: a focused side panel with proper syntax highlighting,
     line numbers, full-height view, and one-click copy / download.

     Design goals
     ────────────
       • Non-intrusive. The chat thread keeps the original block; the
         artifact panel is an OPTIONAL second view, not a replacement.
       • Multi-artifact. Operators frequently work on several files in
         one investigation; tabs at the top of the panel switch between
         them. Last-used tab is auto-selected.
       • Native HTML primitives. No drag-resize library, no animation
         frameworks — just CSS transitions and the existing highlight.js
         + marked we already ship.
       • Per-session only. Artifacts live in memory; nothing persists
         to SQLite. Closing Lucy resets the artifact tabs.
-->
<script lang="ts">
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { createEventDispatcher } from 'svelte';
    import { marked } from 'marked';
    // v1.7.85 — Lazy-language highlight.js (matches message-render.ts).
    // `highlight.js/lib/common` bundles ~35 languages (~50 KB gzipped);
    // we only need the handful the artifact panel actually renders.
    // Each language registered explicitly stays under 8 KB total. Auto-
    // detect (used when no language hint is given) falls back to
    // plaintext + best effort across the registered set.
    import hljs       from 'highlight.js/lib/core';
    import hljsPS     from 'highlight.js/lib/languages/powershell';
    import hljsBash   from 'highlight.js/lib/languages/bash';
    import hljsJson   from 'highlight.js/lib/languages/json';
    import hljsYaml   from 'highlight.js/lib/languages/yaml';
    import hljsPython from 'highlight.js/lib/languages/python';
    import hljsRust   from 'highlight.js/lib/languages/rust';
    import hljsJs     from 'highlight.js/lib/languages/javascript';
    import hljsTs     from 'highlight.js/lib/languages/typescript';
    import hljsSql    from 'highlight.js/lib/languages/sql';
    import hljsPlain  from 'highlight.js/lib/languages/plaintext';
    // Same `as any` cast pattern used in message-render.ts — the
    // highlight.js v11 typings don't perfectly model the registered
    // language shape and the runtime contract is identical.
    hljs.registerLanguage('powershell', hljsPS     as any);
    hljs.registerLanguage('bash',       hljsBash   as any);
    hljs.registerLanguage('sh',         hljsBash   as any);
    hljs.registerLanguage('json',       hljsJson   as any);
    hljs.registerLanguage('yaml',       hljsYaml   as any);
    hljs.registerLanguage('yml',        hljsYaml   as any);
    hljs.registerLanguage('python',     hljsPython as any);
    hljs.registerLanguage('py',         hljsPython as any);
    hljs.registerLanguage('rust',       hljsRust   as any);
    hljs.registerLanguage('rs',         hljsRust   as any);
    hljs.registerLanguage('javascript', hljsJs     as any);
    hljs.registerLanguage('js',         hljsJs     as any);
    hljs.registerLanguage('typescript', hljsTs     as any);
    hljs.registerLanguage('ts',         hljsTs     as any);
    hljs.registerLanguage('sql',        hljsSql    as any);
    hljs.registerLanguage('plaintext',  hljsPlain  as any);
    import DOMPurify from 'dompurify';

    /** Open + ordered list of artifacts. Driven by the page's reactive store. */
    export let artifacts: Array<{
        id: string;
        title: string;
        kind: 'code' | 'markdown';
        language?: string;
        content: string;
        /** Tab id (or chat-message id) the artifact was promoted from — used
         *  for a "go back to source" link in the panel header. */
        sourceTabId?: string | null;
        createdAt: number;
    }> = [];

    /** Id of the currently shown artifact tab. */
    export let activeId: string | null = null;

    /** Whether the panel is rendered at all. */
    export let open: boolean = false;


    const dispatch = createEventDispatcher<{
        close: void;
        select: { id: string };
        remove: { id: string };
        gotoSource: { id: string; sourceTabId: string };
    }>();

    // ── Rendering ────────────────────────────────────────────────────────────
    // Both code and markdown go through the same hljs / marked pipeline the
    // chat uses. We isolate them in the panel so styling can be a touch
    // larger (operators read artifacts longer than chat replies).
    $: activeArtifact = artifacts.find(a => a.id === activeId) ?? null;

    function renderArtifact(a: typeof artifacts[number] | null): string {
        if (!a) return '';
        if (a.kind === 'code') {
            const lang = (a.language || '').toLowerCase();
            let highlighted: string;
            try {
                highlighted = lang && hljs.getLanguage(lang)
                    ? hljs.highlight(a.content, { language: lang, ignoreIllegals: true }).value
                    : hljs.highlightAuto(a.content).value;
            } catch {
                // Defensive — never let a render error blank the panel.
                highlighted = a.content
                    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
            }
            // Line numbers wrapper. Cheap CSS counter for the gutter.
            return `<pre class="art-code-pre"><code class="hljs ${lang ? `language-${lang}` : ''}">${highlighted}</code></pre>`;
        }
        // Markdown path: marked → DOMPurify so any inline HTML is safe.
        try {
            const html = marked.parse(a.content, { async: false }) as string;
            return DOMPurify.sanitize(html);
        } catch (e) {
            return `<pre>${String(e)}</pre>`;
        }
    }

    function copyToClipboard(): void {
        if (!activeArtifact) return;
        const text = activeArtifact.content;
        navigator.clipboard?.writeText(text).then(
            () => { _copied = true; setTimeout(() => { _copied = false; }, 1400); },
            () => {},
        );
    }
    let _copied = false;

    function download(): void {
        if (!activeArtifact) return;
        const ext = activeArtifact.kind === 'code'
            ? (activeArtifact.language && extByLang(activeArtifact.language)) || 'txt'
            : 'md';
        const name = sanitizeFilename(activeArtifact.title) + '.' + ext;
        const blob = new Blob([activeArtifact.content], { type: 'text/plain;charset=utf-8' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url; a.download = name;
        document.body.appendChild(a); a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
    }

    function extByLang(l: string): string {
        const m: Record<string, string> = {
            powershell: 'ps1', ps1: 'ps1',
            bash: 'sh', sh: 'sh',
            cmd: 'cmd', batch: 'bat', bat: 'bat',
            python: 'py', py: 'py',
            javascript: 'js', js: 'js', typescript: 'ts', ts: 'ts',
            rust: 'rs', rs: 'rs',
            json: 'json', yaml: 'yaml', yml: 'yml',
            sql: 'sql', html: 'html', css: 'css',
        };
        return m[l.toLowerCase()] || 'txt';
    }
    function sanitizeFilename(s: string): string {
        return s.replace(/[<>:"/\\|?*\x00-\x1F]/g, '_').slice(0, 80) || 'artifact';
    }

    function fmtAge(ms: number): string {
        const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
        if (s < 60)    return `${s}s`;
        if (s < 3600)  return `${Math.round(s / 60)}m`;
        if (s < 86400) return `${Math.round(s / 3600)}h`;
        return `${Math.round(s / 86400)}d`;
    }
</script>

{#if open && artifacts.length > 0}
<aside class="art-panel" aria-label={$trad('Artefactos')}>
    <header class="art-head">
        <div class="art-tabs" role="tablist">
            {#each artifacts as a (a.id)}
                <button type="button"
                        class="art-tab"
                        class:active={a.id === activeId}
                        role="tab"
                        aria-selected={a.id === activeId}
                        title={`${a.title}\n${a.kind === 'code' ? (a.language || 'code') : 'markdown'} · ${fmtAge(a.createdAt)}`}
                        on:click={() => dispatch('select', { id: a.id })}>
                    <span class="art-tab-kind">{a.kind === 'code' ? '⚯' : '◐'}</span>
                    <span class="art-tab-title">{a.title}</span>
                    <span class="art-tab-close"
                          role="button" tabindex="0"
                          title={$trad('Quitar de artefactos')}
                          on:click|stopPropagation={() => dispatch('remove', { id: a.id })}
                          on:keydown={(ev) => { if (ev.key === 'Enter' || ev.key === ' ') { ev.preventDefault(); dispatch('remove', { id: a.id }); } }}>
                        ✕
                    </span>
                </button>
            {/each}
        </div>
        <div class="art-actions">
            <button class="art-btn" type="button" on:click={copyToClipboard} title={$trad('Copiar al portapapeles')}>
                {_copied ? '✓' : '⧉'}
            </button>
            <button class="art-btn" type="button" on:click={download} title={$trad('Descargar como archivo')}>
                ↓
            </button>
            {#if activeArtifact && activeArtifact.sourceTabId}
                {@const _srcId = String(activeArtifact.sourceTabId)}
                <button class="art-btn" type="button"
                        on:click={() => dispatch('gotoSource', { id: activeArtifact.id, sourceTabId: _srcId })}
                        title={$trad('Ir al mensaje origen')}>
                    ↗
                </button>
            {/if}
            <button class="art-btn art-btn-close" type="button"
                    on:click={() => dispatch('close')}
                    title={$trad('Cerrar panel')}>
                ✕
            </button>
        </div>
    </header>

    {#if activeArtifact}
        <div class="art-meta">
            <span class="art-meta-kind">
                {activeArtifact.kind === 'code'
                    ? `${activeArtifact.language || 'code'}`
                    : 'markdown'}
            </span>
            <span class="art-meta-sep">·</span>
            <span class="art-meta-lines">
                {activeArtifact.content.split('\n').length} {$trad('líneas')}
            </span>
            <span class="art-meta-sep">·</span>
            <span class="art-meta-age">{fmtAge(activeArtifact.createdAt)}</span>
        </div>
        <div class="art-body" data-kind={activeArtifact.kind}>
            {@html renderArtifact(activeArtifact)}
        </div>
    {/if}
</aside>
{/if}

<style>
    .art-panel {
        position: fixed;
        right: 0;
        top: 0;
        bottom: 0;
        width: min(560px, 42vw);
        background: rgba(8, 14, 24, 0.96);
        border-left: 1px solid rgba(255, 255, 255, 0.08);
        box-shadow: -8px 0 24px rgba(0, 0, 0, 0.35);
        display: flex;
        flex-direction: column;
        z-index: 50;
        animation: art-slide-in 0.18s ease-out;
        font-family: var(--font-ui, ui-sans-serif, system-ui);
    }
    @keyframes art-slide-in {
        from { transform: translateX(20px); opacity: 0; }
        to   { transform: translateX(0);    opacity: 1; }
    }

    .art-head {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 6px 6px 6px 10px;
        background: rgba(2, 4, 8, 0.6);
        border-bottom: 1px solid rgba(255, 255, 255, 0.05);
        flex-shrink: 0;
    }
    .art-tabs {
        display: flex;
        align-items: center;
        gap: 2px;
        flex: 1;
        overflow-x: auto;
        scrollbar-width: thin;
    }
    .art-tab {
        appearance: none;
        background: transparent;
        border: 1px solid transparent;
        border-bottom: none;
        border-radius: 6px 6px 0 0;
        padding: 5px 8px 4px 10px;
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 11px;
        color: var(--txt2, #94a3b8);
        cursor: pointer;
        max-width: 200px;
        white-space: nowrap;
        transition: background 0.12s, color 0.12s;
    }
    .art-tab.active {
        background: rgba(34, 211, 238, 0.06);
        border-color: rgba(34, 211, 238, 0.25);
        color: #22d3ee;
    }
    .art-tab:hover:not(.active) {
        background: rgba(255, 255, 255, 0.04);
        color: var(--txt1, #f1f5f9);
    }
    .art-tab-kind { opacity: 0.75; }
    .art-tab-title {
        overflow: hidden;
        text-overflow: ellipsis;
        font-weight: 500;
    }
    .art-tab-close {
        opacity: 0;
        padding: 0 3px;
        margin-left: 2px;
        border-radius: 3px;
        transition: opacity 0.12s, background 0.12s;
    }
    .art-tab:hover .art-tab-close,
    .art-tab.active .art-tab-close { opacity: 0.6; }
    .art-tab-close:hover { opacity: 1; background: rgba(255, 255, 255, 0.08); }

    .art-actions {
        display: flex;
        gap: 2px;
        margin-left: auto;
        flex-shrink: 0;
    }
    .art-btn {
        appearance: none;
        background: transparent;
        border: 1px solid transparent;
        color: var(--txt2, #94a3b8);
        font-size: 13px;
        width: 28px;
        height: 26px;
        border-radius: 4px;
        cursor: pointer;
        transition: background 0.12s, color 0.12s;
    }
    .art-btn:hover {
        background: rgba(255, 255, 255, 0.06);
        color: var(--txt1, #f1f5f9);
    }
    .art-btn-close:hover {
        background: rgba(239, 68, 68, 0.20);
        color: white;
    }

    .art-meta {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 4px 12px;
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 10.5px;
        color: var(--txt3, #64748b);
        background: rgba(0, 0, 0, 0.20);
        border-bottom: 1px solid rgba(255, 255, 255, 0.04);
        flex-shrink: 0;
    }
    .art-meta-kind { color: #22d3ee; letter-spacing: 0.3px; }
    .art-meta-sep  { opacity: 0.35; }

    .art-body {
        flex: 1;
        overflow: auto;
        padding: 14px 16px 24px;
        font-size: 13px;
        line-height: 1.55;
        color: var(--txt1, #f1f5f9);
    }
    /* Code variant gets monospace + larger line-height for legibility. */
    .art-body[data-kind="code"] {
        padding: 0;
    }
    .art-body[data-kind="code"] :global(.art-code-pre) {
        margin: 0;
        padding: 16px 18px;
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 12.5px;
        line-height: 1.55;
        background: rgba(0, 0, 0, 0.30);
        overflow: visible;     /* outer .art-body handles scroll */
    }
    .art-body[data-kind="code"] :global(code.hljs) {
        background: transparent;
        padding: 0;
        font-family: inherit;
    }
    /* Markdown variant — tighter typography for long-form reading. */
    .art-body[data-kind="markdown"] :global(h1) { font-size: 1.5em; margin: 0 0 12px; }
    .art-body[data-kind="markdown"] :global(h2) { font-size: 1.25em; margin: 18px 0 8px; }
    .art-body[data-kind="markdown"] :global(h3) { font-size: 1.1em; margin: 14px 0 6px; }
    .art-body[data-kind="markdown"] :global(p)  { margin: 0 0 10px; }
    .art-body[data-kind="markdown"] :global(pre) {
        background: rgba(0, 0, 0, 0.30);
        padding: 10px 12px;
        border-radius: 5px;
        overflow-x: auto;
        font-size: 12px;
    }
    .art-body[data-kind="markdown"] :global(code) {
        background: rgba(255, 255, 255, 0.05);
        padding: 1px 5px;
        border-radius: 3px;
        font-size: 0.92em;
    }
    .art-body[data-kind="markdown"] :global(blockquote) {
        margin: 10px 0;
        padding: 6px 12px;
        border-left: 3px solid #22d3ee;
        background: rgba(34, 211, 238, 0.04);
        color: var(--txt2, #94a3b8);
    }
    .art-body[data-kind="markdown"] :global(table) {
        border-collapse: collapse;
        margin: 10px 0;
    }
    .art-body[data-kind="markdown"] :global(th),
    .art-body[data-kind="markdown"] :global(td) {
        border: 1px solid rgba(255, 255, 255, 0.10);
        padding: 4px 10px;
    }
</style>
