// ── shiki-highlight.ts — VSCode-grade code highlighting (v1.4.11) ──────
//
// Shiki uses the same TextMate grammars VSCode uses, so the output is
// indistinguishable from VSCode's terminal/editor highlighting. The
// difference vs highlight.js is most visible on PowerShell (which hljs
// renders weakly because its grammar is hand-rolled regex) and on long
// JSON blocks (real string-vs-key coloring).
//
// Architecture:
//
//   • Singleton highlighter, lazily initialized on first import. The
//     init is async but FIRES IMMEDIATELY at module load — so by the
//     time the first Lucy message renders (~500ms+ after page mount),
//     Shiki is usually ready and `highlightSync` succeeds.
//
//   • highlightSync returns null when the highlighter isn't ready yet
//     so the caller can fall back to highlight.js for the very first
//     code blocks. No flicker or replace-after-render — first paint
//     stays correct.
//
//   • We pre-bundle ONLY the 4 langs Lucy actually uses (powershell,
//     bash, json, yaml). Shiki dynamically imports each grammar, so
//     the cost is bounded at ~80 KB compressed across all four.

import {
    createHighlighter,
    type Highlighter,
    type BundledLanguage,
    type BundledTheme,
} from 'shiki';

// The four langs Lucy emits in markdown code fences. Keep this list
// tight — every entry adds a network round-trip on first use.
const LANGS = ['powershell', 'bash', 'json', 'yaml'] as const satisfies readonly BundledLanguage[];
// Match Lucy's terminal palette. github-dark-dimmed is the closest
// stock theme to our custom-neon-tokyo / default dark themes.
const THEME = 'github-dark-dimmed' as const satisfies BundledTheme;

let _highlighter: Highlighter | null = null;
let _initPromise: Promise<Highlighter> | null = null;

/** Kick off the load eagerly. Safe to call multiple times. */
function ensureInit(): Promise<Highlighter> {
    if (_initPromise) return _initPromise;
    _initPromise = createHighlighter({
        themes: [THEME],
        langs: LANGS as unknown as BundledLanguage[],
    }).then((h) => {
        _highlighter = h;
        return h;
    });
    return _initPromise;
}

// Eager-start at module import time so the highlighter is warm by the
// time the user sees the first code block. No await — the import
// resolves before the highlighter does, and `highlightSync` correctly
// returns null until it's ready.
if (typeof window !== 'undefined') {
    ensureInit().catch((e) => {
        // Stay silent on init failures so the caller's hljs fallback
        // takes over. Log once for debug builds.
        try { console.warn('[shiki] init failed, falling back to hljs:', e); } catch {}
    });
}

/**
 * Return Shiki-rendered HTML for a code block, or null if the
 * highlighter isn't ready yet. Caller MUST handle null by falling
 * back to a synchronous alternative (highlight.js).
 *
 * Why no async signature: the existing message-render path is
 * synchronous and refactoring it to async would touch every chat
 * message render. Sync-with-fallback gives ~98% Shiki coverage
 * (everything after the first second post-boot) without that
 * refactor cost. The remaining 2% still renders correctly via hljs.
 */
export function highlightSync(code: string, lang: string): string | null {
    const h = _highlighter;
    if (!h) return null;
    // Normalize Lucy's lang aliases to Shiki's bundled names.
    const langN = (
        lang === 'shell' || lang === 'sh' || lang === 'cmd' || lang === 'bash' ? 'bash' :
        lang === 'powershell' || lang === 'ps' || lang === 'ps1' ? 'powershell' :
        lang === 'json' ? 'json' :
        lang === 'yaml' || lang === 'yml' ? 'yaml' :
        null
    );
    if (!langN) return null;
    try {
        // codeToHtml wraps in <pre class="shiki"><code>...</code></pre>.
        // Our existing styles target `.shiki` or the inner spans, so this
        // drops in cleanly alongside the hljs path.
        return h.codeToHtml(code, { lang: langN, theme: THEME });
    } catch (e) {
        try { console.warn('[shiki] highlight failed:', e); } catch {}
        return null;
    }
}

/**
 * Strip the outer `<pre><code>` wrappers Shiki adds, leaving only the
 * colored inner HTML. Useful when the caller already has its own pre
 * and just wants to swap the code content (the message-render path
 * does this — it owns the pre and just replaces the code content).
 */
export function stripPreWrapper(shikiHtml: string): string {
    const m = shikiHtml.match(/<code[^>]*>([\s\S]*)<\/code>/);
    return m ? m[1] : shikiHtml;
}

/**
 * Test helper: return the underlying instance once initialized.
 * Production code MUST go through highlightSync to keep the
 * sync-with-fallback contract.
 */
export async function _getHighlighterForTesting(): Promise<Highlighter> {
    return ensureInit();
}
