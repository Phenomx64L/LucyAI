// ── safe-html.ts — Centralized HTML sanitization helpers ─────────────────────
//
// Use these whenever rendering ANY string that came from outside the codebase
// (LLM output, command results, user input, network responses, file content)
// via Svelte's `{@html}` directive.
//
// Audit (current): all existing `{@html}` sites in the codebase are safe:
//   - DAILY_TIPS / TutorialOverlay STEPS: hardcoded dev content (no sanitization needed)
//   - msg.html in ChatThread: built via renderLucyMarkdown → DOMPurify
//   - renderJsonHtml in EnrichedOutputWidget: self-escapes & < > " in strings/keys
//   - sparklineSvg in DashboardView: builds SVG from typed numeric metrics
//
// New code MUST pass user/LLM/external strings through `safeHtml()` before
// injection via `{@html}`. Plain interpolation `{value}` is always safe.

import DOMPurify from 'dompurify';

/**
 * Escape the 5 HTML special characters. Use for text that will be inserted
 * as HTML body content but should be displayed as raw text (e.g. user input
 * inside a `<span>`). Does NOT allow any tags.
 */
export function escapeHtml(s: unknown): string {
    return String(s ?? '')
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');
}

/**
 * Escape a string for safe use inside an HTML attribute value (between quotes).
 * Strict subset of escapeHtml — the attribute parser is more forgiving.
 */
export function escapeHtmlAttr(s: unknown): string {
    return String(s ?? '')
        .replace(/&/g, '&amp;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
}

/**
 * Sanitize HTML allowing a curated set of safe tags. Use when the input is
 * supposed to contain SOME markup (e.g. markdown-rendered HTML from the LLM)
 * but we want to strip anything dangerous.
 *
 * Default profile blocks: <script>, <iframe>, <object>, <embed>, on*= handlers,
 * javascript: URLs, style= attributes (CSS exfil vectors).
 */
export function safeHtml(input: string, opts?: { allowImages?: boolean }): string {
    const allowImages = opts?.allowImages ?? false;
    const config: any = {
        ALLOWED_TAGS: [
            'p', 'span', 'div', 'br', 'hr',
            'strong', 'b', 'em', 'i', 'u', 'code', 'kbd', 'pre',
            'ul', 'ol', 'li',
            'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
            'a',
            'table', 'thead', 'tbody', 'tr', 'th', 'td',
            'blockquote', 'small', 'sup', 'sub',
        ],
        ALLOWED_ATTR: ['href', 'title', 'class', 'id', 'target', 'rel', 'colspan', 'rowspan'],
        ALLOW_DATA_ATTR: false,
        // Block inline event handlers and javascript: URLs explicitly
        FORBID_ATTR: ['onerror', 'onload', 'onclick', 'onmouseover', 'style'],
        FORBID_TAGS: ['style', 'script', 'iframe', 'object', 'embed', 'link', 'meta'],
        // Disable URL schemes other than http/https/mailto
        ALLOWED_URI_REGEXP: /^(?:(?:https?|mailto):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))/i,
    };
    if (allowImages) {
        config.ALLOWED_TAGS.push('img');
        config.ALLOWED_ATTR.push('src', 'alt', 'width', 'height');
    }
    return DOMPurify.sanitize(input, config) as unknown as string;
}

/**
 * Render plain text with newlines preserved as <br>. Use for displaying
 * shell output or other untrusted text that should keep its line breaks
 * but otherwise be inert.
 */
export function textToHtml(text: string): string {
    return escapeHtml(text).replace(/\n/g, '<br>');
}
