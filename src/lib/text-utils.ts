// ── text-utils — pure helpers used across the chat/agent rendering ──────────
//
// Extracted from `+page.svelte` to keep the main route component focused on
// state + UI. These are framework-agnostic and unit-testable.

/**
 * HTML-escape a value for safe inclusion in `innerHTML` / template literals.
 * Use this for any user/agent-supplied string that becomes HTML.
 *
 * NOT a DOMPurify replacement — for full Markdown→HTML pipeline use
 * `renderMd()` from `$lib/md-render`.
 */
export function escapeHtml(s: unknown): string {
    return String(s ?? '')
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
}

/**
 * Normalize a Spanish/English phrase for fuzzy keyword matching:
 *   - strips diacritics (NFD + combining marks)
 *   - removes punctuation
 *   - lowercases and trims
 *
 * Used to match user voice/text input against trigger phrases without forcing
 * the user to type "exactly".
 */
export function normalizeForMatch(t: string): string {
    return String(t ?? '')
        .normalize('NFD')
        .replace(/[̀-ͯ]/g, '')
        .replace(/[.,:;!?¡¿]/g, '')
        .toLowerCase()
        .trim();
}

/**
 * Locale-aware HH:MM formatter for chat timestamps.
 *
 * @param lang  BCP-47 tag (e.g. 'es-MX'). Falls back to browser default if undefined.
 * @param date  Optional Date — defaults to now.
 */
export function formatTime(lang?: string, date: Date = new Date()): string {
    try {
        return date.toLocaleTimeString(lang || undefined, { hour: '2-digit', minute: '2-digit' });
    } catch {
        return date.toLocaleTimeString();
    }
}

/**
 * Truncate a string to maxLen, appending "…" if truncated.
 * Word-aware: breaks on the last whitespace before maxLen when possible to
 * avoid mid-word cuts.
 */
export function smartTruncate(s: string, maxLen: number): string {
    if (!s || s.length <= maxLen) return s || '';
    const slice = s.slice(0, maxLen - 1);
    const lastWs = slice.lastIndexOf(' ');
    // Only break on word boundary if it's not too far back (≥75% of maxLen)
    const cutAt = lastWs > maxLen * 0.75 ? lastWs : slice.length;
    return s.slice(0, cutAt) + '…';
}

/**
 * Render N as a human-friendly token count: 1234 → "1.2k", 1500000 → "1.5M".
 * Used in cost predictor + dashboards.
 */
export function formatTokens(n: number): string {
    if (!Number.isFinite(n) || n < 0) return '0';
    if (n < 1000) return String(Math.round(n));
    if (n < 100_000) return (n / 1000).toFixed(1).replace(/\.0$/, '') + 'k';
    if (n < 1_000_000) return Math.round(n / 1000) + 'k';
    return (n / 1_000_000).toFixed(1).replace(/\.0$/, '') + 'M';
}

/**
 * Render a byte count as a human-friendly size (was `_fmtBytes` in
 * +page.svelte). Null/undefined → "—".
 */
export function fmtBytes(n: number | null | undefined): string {
    if (n == null) return '—';
    if (n < 1024)               return `${n} B`;
    if (n < 1024 * 1024)        return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/**
 * Truncate `text` to `max` chars, appending an inline "+N chars" hint span
 * (was `truncarConHint` in +page.svelte). Returns HTML, so the caller must
 * already trust / escape `text`.
 */
export function truncateWithHint(text: string, max: number): string {
    if (!text || text.length <= max) return text || '';
    const restantes = text.length - max;
    return `${text.substring(0, max)}<span class="trunc-hint"> … [+${restantes} chars — ver Audit Log para salida completa]</span>`;
}
