// ── md-render — centralized Markdown→safe-HTML pipeline ─────────────────────
//
// Why exist:
//   • There were 7+ call sites scattered around `+page.svelte` doing
//     `DOMPurify.sanitize(marked.parse(...))` with subtly different configs.
//     One forgot ADD_ATTR for `style`, another missed `data-plan-id`. Drift =
//     bugs (e.g. plan badges not clickable in some agent renders).
//   • Long conversations (>50 msgs) re-render messages on every `refresh()`,
//     wasting CPU on re-parsing identical content. A small LRU cache eliminates
//     that completely without changing semantics.
//
// Usage:
//   import { renderMd } from '$lib/md-render';
//   element.innerHTML = renderMd(markdownText);
//   element.innerHTML = renderMd(markdownText, { withBadges: true });

import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { addCiteChips } from '$lib/cite-chips';

// v1.7.23 — Disable GFM strikethrough.
//
// Lucy's prose spec doesn't include strikethrough — the only place
// `~~text~~` appeared in the rendered chat came from the LLM emitting
// it accidentally around emphasized phrases (often around bracketed
// labels like `~~[modelos de IA]~~`), which marked happily turned into
// `<del>[modelos de IA]</del>`. That looks like text was retracted, not
// emphasized. We disable the tokenizer entirely; if a future surface
// genuinely needs strikethrough we can re-enable it for that surface.
let _markedConfigured = false;
function _configureMarked() {
    if (_markedConfigured) return;
    _markedConfigured = true;
    try {
        // The cast is needed because marked's TS types don't expose the
        // tokenizer override shape cleanly. The runtime contract is:
        // returning `undefined` makes marked fall through and treat the
        // matched substring as plain text.
        (marked as any).use({
            tokenizer: {
                del(_src: string) { return undefined; },
            },
            renderer: {
                // v1.7.181 — Gemini-style fenced code blocks. The language
                // header + copy button are emitted INTO the HTML string (not
                // bolted on post-render by addCopyBtns), so they appear the
                // instant a ``` fence opens DURING streaming and survive the
                // morphdom diff. Copy is wired by a delegated listener on
                // `.copy-btn[data-copy]` in +page.svelte (a per-element onclick
                // would be morphed away on the next chunk). The inner
                // `<pre><code class="language-X">` shape is kept verbatim so
                // applyShikiToHtml's regex and addCopyBtns' run-button pass
                // still work unchanged.
                code(codeOrToken: any, maybeInfo?: string) {
                    const isTok = codeOrToken && typeof codeOrToken === 'object';
                    const rawCode = String(isTok ? (codeOrToken.text ?? '') : (codeOrToken ?? ''));
                    const info = String(isTok ? (codeOrToken.lang ?? '') : (maybeInfo ?? '')).trim();
                    const langRaw = info.split(/\s+/)[0].toLowerCase();
                    const lang = /^[a-z0-9+#.\-]{1,24}$/.test(langRaw) ? langRaw : '';
                    const esc = rawCode
                        .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
                    const langClass = lang ? ` class="language-${lang}"` : '';
                    const label = lang || 'texto';
                    return '<div class="code-wrap" data-cw="1">'
                        +    '<div class="code-header">'
                        +      `<span class="code-lang">${label}</span>`
                        +      '<button class="copy-btn" type="button" data-copy title="Copiar al portapapeles" aria-label="Copiar">'
                        +        '<span class="copy-ico" aria-hidden="true">⧉</span>'
                        +        '<span class="copy-lbl">Copiar</span>'
                        +      '</button>'
                        +    '</div>'
                        +    `<pre><code${langClass}>${esc}</code></pre>`
                        +  '</div>';
                },
            },
        });
    } catch (e) {
        // eslint-disable-next-line no-console
        console.warn('[md-render] could not disable strikethrough:', e);
    }
}
_configureMarked();

// Tags Lucy emits that the host page renders semantically. Keep this list
// small — every attr added is a potential XSS vector.
const ADD_ATTR_BASE = ['style'];
const ADD_ATTR_BADGES = ['style', 'data-plan-id', 'data-plan-action'];

// LRU cache for parse-and-sanitize results. Keyed on `mode|md` so the same text
// rendered with different configs gets distinct entries.
//
// v1.7.83 — Two tuning changes after a profiling pass on long chat
// sessions:
//
//   1. Bumped `_CACHE_MAX` from 200 to 500. The pricer here is RAM —
//      500 × (key + cached HTML) of typical 2-8 KB renders is ~3-5 MB,
//      negligible on a desktop app. The win: long investigation tabs
//      (50+ messages with full markdown) stop evicting their own
//      mid-conversation messages every refresh, which was the source
//      of the ~15 ms re-render hitches.
//
//   2. Key is now a FNV-1a 32-bit hash of the (mode|chips|md) tuple
//      instead of the raw concatenation. Same collision rate at our
//      scale (500 entries → < 0.01% birthday risk) but the Map stores
//      8-char hex keys instead of N-KB strings. Net: cache footprint
//      goes from O(N × md_size) to O(N) — fixed regardless of how
//      large the cached markdown is.
const _CACHE_MAX = 500;
const _cache = new Map<string, string>();

/** FNV-1a 32-bit hash. Fast, deterministic, branchless inner loop.
 *  Good enough for cache keys at our scale — we tolerate the negligible
 *  collision probability in exchange for not paying full string keys. */
function _hash32(s: string): string {
    let h = 0x811c9dc5;
    for (let i = 0; i < s.length; i++) {
        h ^= s.charCodeAt(i);
        // imul + >>> 0 keeps the multiply in u32 land in JS.
        h = Math.imul(h, 0x01000193) >>> 0;
    }
    return h.toString(16).padStart(8, '0');
}

function _cacheGet(key: string): string | undefined {
    const v = _cache.get(key);
    if (v === undefined) return undefined;
    // Move-to-front for LRU semantics (Map preserves insertion order).
    _cache.delete(key);
    _cache.set(key, v);
    return v;
}
function _cacheSet(key: string, value: string) {
    if (_cache.has(key)) _cache.delete(key);
    _cache.set(key, value);
    if (_cache.size > _CACHE_MAX) {
        // Evict oldest = first key in the iteration order
        const oldestKey = _cache.keys().next().value;
        if (oldestKey !== undefined) _cache.delete(oldestKey);
    }
}

/**
 * Convert Markdown to sanitized HTML using Lucy's standard config.
 *
 * Modes:
 *   default   – standard Markdown, allows inline `style`.
 *   badges    – additionally allows `data-plan-id` and `data-plan-action`
 *               attributes used by the Plan/Act/Verify badge pipeline.
 *   raw       – sanitize a raw HTML string WITHOUT running marked first
 *               (useful when caller already produced HTML, e.g. tool cards).
 */
export function renderMd(
    md: string | null | undefined,
    opts: { mode?: 'default' | 'badges' | 'raw'; chips?: boolean } = {}
): string {
    if (!md) return '';
    const mode = opts.mode ?? 'default';
    // chips defaults to TRUE for default/badges modes so Lucy's prose gets
    // inline clickable file/host/memory chips. Caller can pass {chips:false}
    // to opt out (e.g. for tooltips where chips would be visually noisy).
    const chipsOn = opts.chips ?? (mode !== 'raw');
    // v1.7.83 — hash key (see _hash32 comment above). Include length as
    // a coarse anti-collision prefix so two markdowns whose hashes
    // collide but whose lengths differ never share a cache slot.
    const key = `${mode}|${chipsOn ? 'c' : 'n'}|${md.length}|${_hash32(md)}`;
    const cached = _cacheGet(key);
    if (cached !== undefined) return cached;

    let html: string;
    try {
        if (mode === 'raw') {
            html = DOMPurify.sanitize(md, { ADD_ATTR: ADD_ATTR_BASE });
        } else if (mode === 'badges') {
            const parsed = marked.parse(md) as string;
            html = DOMPurify.sanitize(parsed, { ADD_ATTR: ADD_ATTR_BADGES });
        } else {
            const parsed = marked.parse(md) as string;
            html = DOMPurify.sanitize(parsed);
        }
        if (chipsOn) {
            html = addCiteChips(html);
        }
    } catch (e) {
        // Don't crash chat over malformed markdown — fall back to escaped text
        try { console.warn('[md-render] failed:', e); } catch {}
        html = String(md)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;');
    }

    _cacheSet(key, html);
    return html;
}

/**
 * Drop the cache (call from "Clear chat" or after a Lucy update so users
 * don't see stale rendered HTML across versions).
 */
export function clearMdCache() {
    _cache.clear();
}
