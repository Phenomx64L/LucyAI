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

// Tags Lucy emits that the host page renders semantically. Keep this list
// small — every attr added is a potential XSS vector.
const ADD_ATTR_BASE = ['style'];
const ADD_ATTR_BADGES = ['style', 'data-plan-id', 'data-plan-action'];

// LRU cache for parse-and-sanitize results. Keyed on `mode|md` so the same text
// rendered with different configs gets distinct entries.
const _CACHE_MAX = 200;
const _cache = new Map<string, string>();

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
    const key = `${mode}|${chipsOn ? 'c' : 'n'}|${md}`;
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
