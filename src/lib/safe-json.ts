// ── safe-json.ts — Defensive JSON parsers for DB payloads ────────────────
//
// Lucy stores arrays / objects as TEXT columns in SQLite (tags, files,
// concepts, lessons, etc.). When a row is hand-edited, partially migrated,
// or a Rust serializer changes, a raw `JSON.parse()` throws and the
// surrounding feature crashes silently.
//
// These helpers convert any parse error into a typed fallback so the UI
// degrades gracefully instead of going blank.
//
// Logging policy:
//   • A bad parse logs a single console.warn — useful when investigating
//     a corrupted row but not noisy in production.
//   • Errors are NEVER re-thrown.

export function safeJsonArray<T = unknown>(raw: unknown, fallback: T[] = []): T[] {
    if (Array.isArray(raw)) return raw as T[];
    if (typeof raw !== 'string' || !raw) return fallback;
    try {
        const parsed = JSON.parse(raw);
        return Array.isArray(parsed) ? (parsed as T[]) : fallback;
    } catch (e) {
        console.warn('[safe-json] array parse failed:', e, '· raw:', String(raw).slice(0, 80));
        return fallback;
    }
}

export function safeJsonObject<T extends Record<string, unknown> = Record<string, unknown>>(
    raw: unknown,
    fallback: T = ({} as T),
): T {
    if (raw && typeof raw === 'object' && !Array.isArray(raw)) return raw as T;
    if (typeof raw !== 'string' || !raw) return fallback;
    try {
        const parsed = JSON.parse(raw);
        return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
            ? (parsed as T)
            : fallback;
    } catch (e) {
        console.warn('[safe-json] object parse failed:', e, '· raw:', String(raw).slice(0, 80));
        return fallback;
    }
}

/**
 * Generic safe parse — returns the parsed value or the fallback on any error.
 * Use this when the shape (array vs object vs primitive) varies between rows.
 */
export function safeJsonParse<T = unknown>(raw: unknown, fallback: T): T {
    if (raw === null || raw === undefined) return fallback;
    if (typeof raw !== 'string') return raw as T;
    try {
        return JSON.parse(raw) as T;
    } catch (e) {
        console.warn('[safe-json] parse failed:', e, '· raw:', String(raw).slice(0, 80));
        return fallback;
    }
}
