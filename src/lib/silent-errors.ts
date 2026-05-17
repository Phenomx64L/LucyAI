// ── silent-errors.ts — Centralized reporter for fire-and-forget failures ────
//
// Lucy has many `.catch(() => {})` patterns: tool invocations that we don't
// want to disrupt the user with (e.g. background memory writes, embedding
// upserts, log rotations, etc). Suppressing them silently means real failures
// hide forever — there's no way to debug a "why didn't this work" without
// adding console.log everywhere.
//
// `reportSilent()` replaces `.catch(() => {})` with a single-line call that:
//   1) Logs to console.warn with a structured prefix `[silent:CTX]`
//   2) Appends to an in-memory ring buffer (last 100 events)
//   3) Increments a per-context counter so devtools can see "embeddings has
//      failed 8× in this session"
//   4) Exposes everything via `window._lucyErrors` so you can inspect from
//      DevTools console without rebuilding
//
// Usage:
//   invoke('upsert_embedding', {...}).catch(reportSilent('upsert_embedding'));
//   try { ... } catch (e) { reportSilent('parseConfig')(e); }
//
// The function returns a callback so it composes naturally with `.catch()`.

interface SilentEvent {
    ts: number;
    ctx: string;
    err: string;
    stack?: string;
}

const _buffer: SilentEvent[] = [];
const _counts: Map<string, number> = new Map();
const MAX_BUFFER = 100;

/**
 * Returns a `.catch()` handler that logs the error to the silent-errors
 * channel. Use as: `.catch(reportSilent('memory_save'))`.
 *
 * If you have a raw value (from try/catch), call the returned function:
 *   try {...} catch (e) { reportSilent('parse')(e); }
 */
/**
 * Format any error shape we might see — string, Error, or the new tagged
 * `LucyError` object from the Rust backend. Without this helper, a
 * `LucyError` would stringify as `"[object Object]"` and the silent log
 * would be useless.
 *
 * Wire shape from Rust: `{ code: "NotFound", data: { what: "...", path: "..." } }`.
 */
function formatSilentError(err: unknown): string {
    if (err == null) return '(null)';
    if (typeof err === 'string') return err;
    if (err instanceof Error) return err.message;
    if (typeof err === 'object' && 'code' in (err as Record<string, unknown>)) {
        // LucyError-like: extract message if available, otherwise show code + data
        const obj = err as { code: string; data?: Record<string, unknown> };
        const data = obj.data || {};
        if (typeof (data as { message?: string }).message === 'string') {
            return `${obj.code}: ${(data as { message: string }).message}`;
        }
        if (typeof (data as { reason?: string }).reason === 'string') {
            return `${obj.code}: ${(data as { reason: string }).reason}`;
        }
        try { return `${obj.code}: ${JSON.stringify(data)}`; }
        catch { return obj.code; }
    }
    try { return JSON.stringify(err); } catch { return String(err); }
}

export function reportSilent(ctx: string): (err: unknown) => void {
    return (err: unknown) => {
        const errStr = formatSilentError(err);
        const stack  = err instanceof Error ? err.stack : undefined;

        // Ring buffer cap
        if (_buffer.length >= MAX_BUFFER) _buffer.shift();
        _buffer.push({ ts: Date.now(), ctx, err: errStr, stack });

        _counts.set(ctx, (_counts.get(ctx) || 0) + 1);

        // Console output — devs can filter by `[silent:` in the console
        // eslint-disable-next-line no-console
        console.warn(`[silent:${ctx}]`, errStr);
    };
}

/**
 * Snapshot of all silent events recorded so far.
 * Useful for diagnostic dumps and tests.
 */
export function getSilentLog(): readonly SilentEvent[] {
    return [..._buffer];
}

/**
 * Per-context error counts: which subsystems are failing most?
 */
export function getSilentCounts(): Map<string, number> {
    return new Map(_counts);
}

/**
 * Reset the buffer + counters. Mostly for tests; production rarely needs this.
 */
export function clearSilentLog(): void {
    _buffer.length = 0;
    _counts.clear();
}

// Expose to DevTools without polluting module-import semantics.
// Usage in console:  _lucyErrors.log() | _lucyErrors.counts() | _lucyErrors.clear()
if (typeof window !== 'undefined') {
    (window as any)._lucyErrors = {
        log:    getSilentLog,
        counts: () => Object.fromEntries(_counts),
        clear:  clearSilentLog,
        recent: (n = 20) => _buffer.slice(-n),
    };
}
