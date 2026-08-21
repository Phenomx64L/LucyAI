// ── message-render.ts — HTML rendering utilities for Lucy chat messages ──────
// Pure functions + addCopyBtns (DOM-side). Imported by +page.svelte.

import DOMPurify        from 'dompurify';
import hljs             from 'highlight.js/lib/core';
// v1.4.11 — Shiki is the primary highlighter (VSCode-quality grammars).
// hljs stays as a synchronous fallback for the first ~1 s after page
// load while Shiki's grammars finish loading. See shiki-highlight.ts.
import { highlightSync as shikiHighlight, stripPreWrapper } from '$lib/shiki-highlight';
import hljsPS           from 'highlight.js/lib/languages/powershell';
import hljsBash         from 'highlight.js/lib/languages/bash';
import hljsJson         from 'highlight.js/lib/languages/json';
import hljsYaml         from 'highlight.js/lib/languages/yaml';
import hljsPlain        from 'highlight.js/lib/languages/plaintext';
import { renderMd }     from '$lib/md-render';

// La interfaz en cinco idiomas. `tr` y no `$trad`: esto no es un
// componente, así que no hay nada a lo que suscribirse. Ver `$lib/i18n`.
import { tr } from '$lib/i18n';
hljs.registerLanguage('powershell', hljsPS as any);
hljs.registerLanguage('bash',       hljsBash as any);
hljs.registerLanguage('shell',      hljsBash as any);
hljs.registerLanguage('json',       hljsJson as any);
hljs.registerLanguage('yaml',       hljsYaml as any);
hljs.registerLanguage('plaintext',  hljsPlain as any);

// ── warpBlock ────────────────────────────────────────────────────────────────
// Builds the collapsible command-output block injected into Lucy messages.
// When enrichedType is provided (not 'plain'), renders a structured data widget
// alongside the raw output for rich visualization.
//
// v1.7.60 — "Terminal recording" header (Mission Control A3). Adds:
//   • Three decorative traffic-light dots on the left (asciinema-style)
//   • Optional `hostname` chip (defaults to local hostname when absent)
//   • Optional `engine` icon — PS / cmd / bash / wmic / netsh
//   • Optional absolute `ts` timestamp (HH:MM:SS) alongside the elapsed time
//   • Exit-code badge: `exit 0` for ok / `exit ≠0` for error
//
// The `meta` parameter is OPTIONAL and backwards-compatible. Call sites that
// don't pass it render the same as before plus the new traffic-light dots
// and the styled exit-code badge — already a noticeable upgrade without
// requiring every caller to thread hostname/engine through.
export interface WarpBlockMeta {
    hostname?: string;
    /** powershell | cmd | bash | wmic | netsh | reg | cscript | other */
    engine?: string;
    /** ISO or `HH:MM:SS` string. Absent = no absolute time shown. */
    ts?: string;
    /** Optional explicit exit code; when omitted, derived from `ok` (0 or 1). */
    exitCode?: number;
}

/** Convert an engine label into a one-character glyph for the header. */
function _engineGlyph(engine?: string): string {
    if (!engine) return '$';
    const e = engine.toLowerCase();
    if (e === 'powershell' || e === 'ps' || e === 'ps1' || e === 'pwsh') return '⚡';
    if (e === 'cmd' || e === 'batch' || e === 'bat')                      return '▶';
    if (e === 'bash' || e === 'sh' || e === 'shell')                      return '$';
    if (e === 'wmic')                                                      return '◇';
    if (e === 'netsh')                                                     return '⌬';
    if (e === 'reg')                                                       return '☐';
    if (e === 'cscript' || e === 'vbs' || e === 'vbscript')                return '※';
    if (e === 'winrm' || e === 'ssh' || e === 'remote')                    return '⇄';
    return '$';
}

export function warpBlock(
    cmd: string,
    output: string,
    ok: boolean,
    elapsedMs: number,
    label = '',
    enrichedType?: string,
    enrichedJson?: string,
    meta?: WarpBlockMeta,
): string {
    const sc = cmd.replace(/</g, '&lt;').replace(/>/g, '&gt;');
    const so = DOMPurify.sanitize(output.replace(/</g, '&lt;').replace(/>/g, '&gt;'));
    const t  = elapsedMs < 1000 ? `${elapsedMs}ms` : `${(elapsedMs / 1000).toFixed(1)}s`;
    const st = ok ? 'wb-ok' : 'wb-err';
    const hl = label || (ok ? 'Ejecutado' : 'Error');
    // Enriched widget mount point: data attributes consumed by mountEnrichedWidgets()
    // Use encodeURIComponent to safely embed JSON in data attribute without DOMPurify corruption
    const enrichAttr = (enrichedType && enrichedType !== 'plain' && enrichedJson)
        ? ` data-enriched-type="${enrichedType}" data-enriched-json="${encodeURIComponent(enrichedJson)}"`
        : '';
    const enrichBadge = (enrichedType && enrichedType !== 'plain')
        ? `<span class="wb-enrich-badge">${enrichedType.replace('-', ' ')}</span>`
        : '';
    // Empty-state: visible indicator instead of invisible/dim plain text.
    // Covers commands that succeed silently (Stop-Process, Set-*, Move-Item, etc.)
    const outputHtml = so && so.trim()
        ? so
        : `<span class="wb-empty">${ok ? '✓ Comando completado (sin salida)' : '⚠ Sin salida de error'}</span>`;

    // v1.7.60 — Terminal-recording header pieces.
    const dots = `<span class="wb-dots" aria-hidden="true"><span></span><span></span><span></span></span>`;
    const hostChip = meta?.hostname
        ? `<span class="wb-host" title="Ejecutado en ${String(meta.hostname).replace(/"/g, '')}">${String(meta.hostname).replace(/</g, '&lt;').replace(/>/g, '&gt;')}</span>`
        : '';
    const tsChip = meta?.ts
        ? `<span class="wb-ts">${String(meta.ts).replace(/</g, '&lt;').replace(/>/g, '&gt;')}</span>`
        : '';
    const glyph = _engineGlyph(meta?.engine);
    const exitVal = typeof meta?.exitCode === 'number' ? meta.exitCode : (ok ? 0 : 1);
    const exitChip = `<span class="wb-exit ${exitVal === 0 ? 'wb-exit-ok' : 'wb-exit-err'}">exit ${exitVal}</span>`;

    return `<div class="warp-block ${st}"${enrichAttr}>
<div class="wb-hdr">
  ${dots}
  ${hostChip}
  <code class="wb-cmd"><span class="wb-prompt">${glyph}</span> ${sc}</code>
  ${tsChip}
  <span class="wb-time">${t}</span>
  ${exitChip}
  <span class="wb-lbl">${hl}</span>
  ${enrichBadge}
  <button class="wb-toggle" data-collapsed="0" title="Plegar / desplegar salida">▼</button>
</div>
<pre class="wb-out">${outputHtml}</pre>
<div class="wb-enriched-mount"></div>
</div>`;
}

// ── renderConfidenceTags ──────────────────────────────────────────────────────
// Turns <CONFIDENCE level="high|med|low">reason</CONFIDENCE> into colored badges.
// Applied BEFORE markdown parse so badges survive rendering.
export function renderConfidenceTags(text: string): string {
    if (!text) return text;
    let out = text;

    // ── Block-level: <CONFIDENCE level="high|med|low">reason</CONFIDENCE> ───
    if (out.includes('<CONFIDENCE')) {
        out = out.replace(/<CONFIDENCE\s+level=["']?(high|med|low)["']?\s*>([\s\S]*?)<\/CONFIDENCE>/gi, (_, lvl, reason) => {
            const L = String(lvl).toLowerCase() as 'high' | 'med' | 'low';
            const cfg = {
                high: { bg: 'rgba(52,211,153,.12)',  bd: '#34d399', fg: '#10b981', ico: '✓', label: 'HIGH' },
                med:  { bg: 'rgba(251,191,36,.12)',  bd: '#fbbf24', fg: '#d97706', ico: '◐', label: 'MED'  },
                low:  { bg: 'rgba(248,113,113,.12)', bd: '#f87171', fg: '#ef4444', ico: '⚠', label: 'LOW'  },
            }[L] ?? { bg: 'rgba(148,163,184,.12)', bd: '#94a3b8', fg: '#64748b', ico: '?', label: '?' };
            const safeReason = String(reason).trim().replace(/</g, '&lt;').replace(/>/g, '&gt;');
            return `\n\n<div class="conf-badge" style="display:inline-flex;align-items:center;gap:8px;margin:6px 0;padding:5px 10px;border-left:3px solid ${cfg.bd};background:${cfg.bg};border-radius:3px;font-size:11px;line-height:1.4;">
                <span style="font-weight:700;color:${cfg.fg};letter-spacing:0.5px;">${cfg.ico} ${cfg.label}</span>
                <span style="color:var(--txt2,#94a3b8);">${safeReason}</span>
            </div>\n\n`;
        });
    }

    // ── U8 v2 — Inline confidence markers ────────────────────────────────
    // Lightweight syntax for in-sentence hedging. The LLM can write
    // "Esto [~probablemente~] sea por X" and the rendering will style the
    // word as low-confidence with a tooltip.
    //
    //   [~text~]    — uncertain / hedge (dotted underline, italic)
    //   [!text!]    — high confidence assertion (subtle green underline)
    //   [?text?]    — speculative / guessing (italic + amber)
    //
    // Implemented as restricted character class so we don't conflict with
    // markdown links/lists. Allowed inside: word chars, punctuation, spaces.
    if (out.includes('[~') || out.includes('[!') || out.includes('[?')) {
        // [~uncertain~]
        out = out.replace(/\[~([^\[\]~\n]{1,160})~\]/g,
            (_, t) => `<span class="conf-inline conf-low" title="Lucy is hedging — supporting evidence is partial.">${escapeHtmlMini(t)}</span>`);
        // [!confident!]
        out = out.replace(/\[!([^\[\]!\n]{1,160})!\]/g,
            (_, t) => `<span class="conf-inline conf-high" title="Lucy has direct evidence for this claim.">${escapeHtmlMini(t)}</span>`);
        // [?speculative?]
        out = out.replace(/\[\?([^\[\]?\n]{1,160})\?\]/g,
            (_, t) => `<span class="conf-inline conf-spec" title="Speculation — no direct evidence.">${escapeHtmlMini(t)}</span>`);
    }

    // ── U8 v2 — Citation chips: <CITE src="...">label</CITE> ─────────────
    // Points back to where Lucy got the info: a memory id, file path,
    // tool result, or URL. Rendered as a small clickable chip.
    if (out.includes('<CITE')) {
        // v1.7.23 — Two-pass CITE handling:
        //   (1) Attribute-order-agnostic: pull `src` and `kind` from
        //       anywhere inside the opening tag. The old strict regex
        //       required `src` BEFORE `kind` — when the LLM swapped
        //       order, the regex didn't match, the raw <CITE> tag fell
        //       through to marked, marked passed the inline HTML to
        //       DOMPurify, DOMPurify stripped the unknown tag but left
        //       fragments like `kind="url">Red Hat Announcement` in
        //       the rendered prose.
        //   (2) Safety net: any <CITE> that STILL survives gets reduced
        //       to its label text. We never want a `<CITE>` reaching
        //       marked / DOMPurify.
        out = out.replace(/<CITE\b([^>]*)>([\s\S]*?)<\/CITE>/gi, (_, attrs, label) => {
            const srcMatch  = attrs.match(/\bsrc\s*=\s*["']([^"']+)["']/i);
            const kindMatch = attrs.match(/\bkind\s*=\s*["']([^"']+)["']/i);
            if (!srcMatch) {
                return escapeHtmlMini(String(label).trim());   // safety net (1)
            }
            const src  = srcMatch[1];
            const kind = kindMatch?.[1] || 'ref';
            const safeSrc   = String(src).replace(/"/g, '&quot;');
            const safeLabel = escapeHtmlMini(String(label).trim());
            const ico = kind === 'memory' ? '⌬' :
                        kind === 'file'   ? '⌸' :
                        kind === 'url'    ? '◈' :
                        kind === 'tool'   ? '⚯' : '※';
            return `<a class="cite-chip" data-cite-src="${safeSrc}" data-cite-kind="${kind}" title="${safeSrc}">${ico} ${safeLabel}</a>`;
        });
        // Safety net (2): any unterminated `<CITE …>` (no closing tag)
        // — strip the opening tag, keep nothing else around it.
        out = out.replace(/<CITE\b[^>]*>/gi, '');
        out = out.replace(/<\/CITE\s*>/gi, '');
    }

    return out;
}

// Tiny html escape for confidence inline text (not for full markdown — we
// want italics inside conf-spec to still get markdown'd).
function escapeHtmlMini(s: string): string {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

// ── renderLucyMarkdown ────────────────────────────────────────────────────────
// Full pipeline: CONFIDENCE badges → markdown.
export function renderLucyMarkdown(text: string): string {
    const withBadges = renderConfidenceTags(text || '');
    return renderMd(withBadges, { mode: 'badges' });
}

// ── applyShikiToHtml — v1.7.55 ────────────────────────────────────────────────
//
// Walks an already-rendered HTML string and substitutes the inner HTML of any
// `<pre><code class="language-XXX">...</code></pre>` block with Shiki's
// highlighted output, IF the highlighter is ready and the language is one of
// the four bundled languages.
//
// Why this exists:
//   The post-render `addCopyBtns()` DOM pass runs Shiki on each <code> element
//   AFTER Svelte has already painted the unhighlighted version. The user sees
//   a frame of plain text → a frame of highlighted text — the "los cuadros
//   tardan en cargar" observation reported after v1.7.54.
//
//   By pre-applying Shiki to the HTML string BEFORE assigning it to
//   `msg.html`, the very first frame Svelte renders already contains
//   highlighted code. No transition.
//
// Why this is safe to skip in the streaming path:
//   marked.parse is called on every drain tick; running Shiki on every chunk
//   would multiply per-frame cost by N (one call per code block). Streaming
//   accepts the unhighlighted look — the only consumer of this helper is the
//   FINAL render at promotion time, when stream completes.
//
// Why we don't break `addCopyBtns()`:
//   We tag the substituted code element with `class="shiki-rendered"`. The
//   existing DOM pass already checks for `.hc` and skips already-decorated
//   nodes; we extend it (in addCopyBtns below) to also recognise
//   `.shiki-rendered` so it skips the redundant re-highlight pass while still
//   wiring up the copy/run buttons and the `.code-wrap` shell.
export function applyShikiToHtml(html: string): string {
    if (!html) return html;
    return html.replace(
        /<pre><code class="language-(\w+)">([\s\S]*?)<\/code><\/pre>/g,
        (match, langRaw, codeEscaped) => {
            // Decode the HTML entities marked emitted so Shiki sees the raw text.
            const code = String(codeEscaped)
                .replace(/&lt;/g, '<')
                .replace(/&gt;/g, '>')
                .replace(/&quot;/g, '"')
                .replace(/&#39;/g, "'")
                .replace(/&amp;/g, '&');
            const lang = String(langRaw).toLowerCase();
            const langN: string | null =
                (lang === 'powershell' || lang === 'ps' || lang === 'ps1') ? 'powershell'
                : (lang === 'bash' || lang === 'sh' || lang === 'shell')    ? 'bash'
                : (lang === 'cmd' || lang === 'bat' || lang === 'batch')    ? 'bash'  // hljs fallback uses bash anyway
                : (lang === 'json')                                          ? 'json'
                : (lang === 'yaml' || lang === 'yml')                        ? 'yaml'
                : null;
            if (!langN) return match;
            const sh = shikiHighlight(code, langN);
            if (!sh) return match;
            // Shiki returns its own <pre><code>...</code></pre> wrapper. We
            // replace the inner of OUR <code> with the inner of Shiki's
            // wrapper so the existing language-XXX class on the outer <pre>
            // survives — that class is what `addCopyBtns()` reads to decide
            // run-button language.
            const inner = stripPreWrapper(sh);
            return `<pre><code class="language-${langRaw} shiki-rendered">${inner}</code></pre>`;
        },
    );
}

// ── addCopyBtns options ───────────────────────────────────────────────────────
export interface AddCopyBtnsOpts {
    isEN: boolean;
    getActiveTabId: () => string | null;
    getTab: (id: string) => any;
    runProcess: (tabId: string) => Promise<void>;
    setTabsExecEngine: (tabId: string, engine: string) => void;
    setTabInputValue: (tabId: string, value: string) => void;
    copyToClipboard: (text: string, btn: HTMLButtonElement) => void;
}

// ── addCopyBtns ───────────────────────────────────────────────────────────────
// Post-render DOM pass: syntax highlight + copy/run buttons on every undecorated
// <pre> inside .msg-lucy. Also wires up wb-toggle collapse buttons.
export async function addCopyBtns(opts: AddCopyBtnsOpts): Promise<void> {
    const { isEN, getActiveTabId, getTab, runProcess, setTabsExecEngine, setTabInputValue, copyToClipboard } = opts;

    document.querySelectorAll<HTMLElement>('.msg-lucy pre:not(.hc)').forEach(pre => {
        if (pre.classList.contains('wb-out')) return;
        pre.classList.add('hc');
        const codeEl = pre.querySelector<HTMLElement>('code');
        const rawText = codeEl ? codeEl.innerText.trim() : pre.innerText.trim();
        let lang = 'código';
        if (codeEl) {
            const cls = [...codeEl.classList].find(c => c.startsWith('language-'));
            if (cls) lang = cls.replace('language-', '');
        }

        const isPowershell = lang === 'powershell'
            || rawText.toLowerCase().startsWith('start-process')
            || rawText.includes('Get-') || rawText.includes('-Command') || rawText.includes('Invoke-');
        const isCmd   = lang === 'batch' || lang === 'cmd' || lang === 'bat'
            || rawText.toLowerCase().startsWith('net ') || rawText.toLowerCase().startsWith('ipconfig')
            || rawText.toLowerCase().startsWith('netstat') || rawText.toLowerCase().startsWith('sc ');
        const isWmic  = lang === 'wmic'  || rawText.toLowerCase().startsWith('wmic ');
        const isNetsh = lang === 'netsh' || rawText.toLowerCase().startsWith('netsh ');
        const isReg   = lang === 'reg'   || rawText.toLowerCase().startsWith('reg ');
        const isVbs   = lang === 'vbs' || lang === 'vbscript'
            || rawText.toLowerCase().startsWith('dim ') || rawText.toLowerCase().includes('createobject(');

        const isRunnable = isPowershell || isCmd || isWmic || isNetsh || isReg || isVbs;
        const execTypeInline = isCmd ? 'cmd' : isWmic ? 'wmic' : isNetsh ? 'netsh' : isReg ? 'reg' : isVbs ? 'cscript' : 'powershell';
        const langLabel = isPowershell ? 'PowerShell' : isCmd ? 'CMD' : isWmic ? 'WMIC' : isNetsh ? 'netsh'
            : isReg ? 'reg' : isVbs ? 'VBScript' : lang === 'código' ? 'salida' : lang;

        // Syntax highlighting
        // v1.4.11 — try Shiki first (VSCode TextMate grammars, much better
        // PowerShell + JSON coloring). If the highlighter isn't ready yet
        // — only happens during the first ~1 s of app boot before its
        // grammars finish loading — fall back to highlight.js so first
        // paint stays correct.
        // v1.7.55 — Skip the highlight pass entirely if `applyShikiToHtml()`
        // already baked Shiki output into the code element during the prior
        // string-level render. Detection: the `.shiki-rendered` class is
        // present (added by applyShikiToHtml). This avoids both the wasted
        // re-highlight cost and the visible flash that came from replacing
        // already-colourised inner HTML with a fresh pass.
        if (codeEl && !codeEl.classList.contains('shiki-rendered')) {
            const langN = isPowershell ? 'powershell'
                : (isCmd || isNetsh)   ? 'bash'
                : lang === 'json'      ? 'json'
                : lang === 'yaml'      ? 'yaml'
                : null;
            if (langN) {
                const shikiOut = shikiHighlight(codeEl.innerText || '', langN);
                if (shikiOut) {
                    // Shiki wraps in `<pre class="shiki"><code>…</code></pre>`;
                    // we already own the pre, so swap only the inner HTML.
                    codeEl.innerHTML = stripPreWrapper(shikiOut);
                    codeEl.classList.add('shiki-rendered');
                } else {
                    try {
                        const result = hljs.highlight(codeEl.innerText || '', { language: langN, ignoreIllegals: true });
                        codeEl.innerHTML = result.value;
                        codeEl.classList.add('hljs');
                    } catch (_) { /* degrade silently */ }
                }
            }
        }

        // v1.7.181 — the markdown renderer (md-render.ts) now emits the
        // Gemini-style wrapper (header + lang + copy) directly in the HTML, so
        // it's already present — even mid-stream. If this <pre> is inside one,
        // don't build a second wrapper/header; only add the run button to the
        // existing header for runnable commands.
        const _existingWrap = pre.closest('.code-wrap');
        if (_existingWrap) {
            if (isRunnable) {
                const _hdr = _existingWrap.querySelector('.code-header');
                if (_hdr && !_hdr.querySelector('.run-inline-btn')) {
                    const runBtn = document.createElement('button');
                    runBtn.className = 'run-inline-btn';
                    runBtn.title = tr('Ejecutar este comando');
                    runBtn.textContent = `▶ ${langLabel}`;
                    runBtn.onclick = (ev) => {
                        ev.stopPropagation();
                        const tabId = getActiveTabId();
                        if (!tabId) return;
                        const tab = getTab(tabId);
                        if (tab && !tab.isProcessing) {
                            const prevEngine = tab.execEngine;
                            setTabsExecEngine(tabId, execTypeInline);
                            setTabInputValue(tabId, rawText);
                            runProcess(tabId).finally(() => setTabsExecEngine(tabId, prevEngine));
                        }
                    };
                    _hdr.appendChild(runBtn);
                }
            }
            return;
        }

        const header = document.createElement('div');
        header.className = 'code-header';

        // SECURITY: langLabel may originate from AI-emitted ```<lang> markdown.
        // textContent (not innerHTML) prevents XSS via crafted lang strings.
        const langSpan = document.createElement('span');
        langSpan.className = 'code-lang';
        langSpan.textContent = langLabel;
        header.appendChild(langSpan);

        const btn = document.createElement('button');
        btn.className = 'copy-btn';
        btn.textContent = 'copiar';
        btn.onclick = (e) => { e.stopPropagation(); copyToClipboard(rawText, btn); };
        header.appendChild(btn);

        if (isRunnable) {
            const runBtn = document.createElement('button');
            runBtn.className = 'run-inline-btn';
            runBtn.title = tr('Ejecutar este comando');
            runBtn.textContent = `▶ ${langLabel}`;
            runBtn.onclick = (ev) => {
                ev.stopPropagation();
                const tabId = getActiveTabId();
                if (!tabId) return;
                const tab = getTab(tabId);
                if (tab && !tab.isProcessing) {
                    const prevEngine = tab.execEngine;
                    setTabsExecEngine(tabId, execTypeInline);
                    setTabInputValue(tabId, rawText);
                    runProcess(tabId).finally(() => setTabsExecEngine(tabId, prevEngine));
                }
            };
            header.appendChild(runBtn);
        }

        const wrapper = document.createElement('div');
        wrapper.className = 'code-wrap';
        pre.parentNode!.insertBefore(wrapper, pre);
        wrapper.appendChild(header);
        wrapper.appendChild(pre);
    });

    // Event delegation for warp-block collapse toggles
    document.querySelectorAll<HTMLElement>('.wb-toggle:not([data-bound])').forEach(btn => {
        btn.setAttribute('data-bound', '1');
        btn.addEventListener('click', (e) => {
            e.stopPropagation();
            const block = btn.closest('.warp-block');
            const out = block?.querySelector<HTMLElement>('.wb-out');
            const mount = block?.querySelector<HTMLElement>('.wb-enriched-mount');
            if (!out) return;
            const collapsed = btn.getAttribute('data-collapsed') === '1';
            out.style.display = collapsed ? '' : 'none';
            if (mount) mount.style.display = collapsed ? '' : 'none';
            btn.setAttribute('data-collapsed', collapsed ? '0' : '1');
            btn.textContent = collapsed ? '▼' : '▶';
        });
    });
}

// ── mountEnrichedWidgets ─────────────────────────────────────────────────────
// Post-render DOM pass: finds warp blocks with data-enriched-type/json attributes
// and mounts the EnrichedOutputWidget Svelte component into .wb-enriched-mount.
// Call this after addCopyBtns or any HTML render pass that inserts warp blocks.

/** Track mounted Svelte instances for cleanup.
 *  Uses Map (not WeakMap) because destroyEnrichedWidgets() needs to iterate.
 *  Entries are manually removed in destroy() and on tab clear. */
const mountedWidgets = new Map<HTMLElement, any>();

export async function mountEnrichedWidgets(): Promise<void> {
    const blocks = document.querySelectorAll<HTMLElement>('.warp-block[data-enriched-type]:not([data-enriched-mounted])');
    if (blocks.length === 0) return;

    // Dynamic import to avoid circular deps and code-split the widget
    const { default: EnrichedOutputWidget } = await import('$lib/EnrichedOutputWidget.svelte');

    blocks.forEach(block => {
        const type = block.getAttribute('data-enriched-type');
        const jsonStr = block.getAttribute('data-enriched-json');
        const mountEl = block.querySelector<HTMLElement>('.wb-enriched-mount');
        if (!type || !jsonStr || !mountEl) return;

        try {
            const parsed = JSON.parse(decodeURIComponent(jsonStr));
            const enrichedData = { type: type as import('$lib/output-enricher').OutputType, raw: '', parsed };

            // Mount Svelte 4 component
            const instance = new EnrichedOutputWidget({
                target: mountEl,
                props: { data: enrichedData, collapsed: false }
            });

            mountedWidgets.set(mountEl, instance);
            block.setAttribute('data-enriched-mounted', '1');
        } catch (e) {
            console.warn('[enriched-widget] mount failed:', e);
        }
    });
}

/** Destroy all mounted enriched widgets (call on tab clear / chat reset). */
export function destroyEnrichedWidgets(): void {
    for (const [el, instance] of mountedWidgets.entries()) {
        try {
            if (instance?.$destroy) instance.$destroy();
        } catch { /* already destroyed */ }
        mountedWidgets.delete(el);
    }
}
