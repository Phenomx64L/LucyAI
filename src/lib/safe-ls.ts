// ── safe-ls — defensive helpers for localStorage I/O ────────────────────────
//
// localStorage is a corruption-prone surface: users edit it manually, browser
// extensions write to it, system disk full / quota exceeded errors throw on
// setItem(), and any malformed JSON kills `JSON.parse()` → unhandled exception
// → uncaught error boundary → blank screen.
//
// All Lucy localStorage I/O should go through these helpers. They:
//   1. Never throw (return fallback on any error).
//   2. Log via console.warn so devs can spot corruption in the console.
//   3. Auto-reset corrupted entries so the user isn't stuck on a broken state.

const _warned = new Set<string>();
function _warnOnce(key: string, msg: string) {
    if (_warned.has(key)) return;
    _warned.add(key);
    try { console.warn(`[safe-ls] ${key}: ${msg}`); } catch {}
}

/**
 * Read a JSON value from localStorage. Falls back to `fallback` if:
 *   - the key is missing
 *   - the stored value is not valid JSON (corrupted)
 *   - localStorage itself throws (private mode, disabled, etc.)
 *
 * @param key       localStorage key
 * @param fallback  value returned when read or parse fails
 * @param resetOnCorrupt  if true (default), corrupted entries are deleted
 *                        so the next session starts fresh.
 */
export function safeParseLS<T>(key: string, fallback: T, resetOnCorrupt = true): T {
    let raw: string | null = null;
    try {
        raw = localStorage.getItem(key);
    } catch (e) {
        _warnOnce(key, `getItem failed: ${e}`);
        return fallback;
    }
    if (raw === null || raw === '') return fallback;
    try {
        const v = JSON.parse(raw) as T;
        // Defense-in-depth: if the parsed value is null/undefined when caller
        // expected a non-null fallback, return fallback to keep types stable.
        if (v === null && fallback !== null) return fallback;
        return v;
    } catch (e) {
        _warnOnce(key, `parse failed (corrupted), resetting. Raw: ${raw.slice(0, 60)}…`);
        if (resetOnCorrupt) {
            try { localStorage.removeItem(key); } catch {}
        }
        return fallback;
    }
}

/**
 * Read a plain string from localStorage with safety checks (no JSON parse).
 * Use this for simple flags like 'lucy_dark' = "true" / "false".
 */
export function safeGetLS(key: string, fallback = ''): string {
    try {
        const v = localStorage.getItem(key);
        return v === null ? fallback : v;
    } catch (e) {
        _warnOnce(key, `getItem failed: ${e}`);
        return fallback;
    }
}

/**
 * Write a JSON value to localStorage. Returns true on success, false on failure.
 * NEVER throws. Failures (quota exceeded, disabled storage) are logged once.
 *
 * Important: the JSON serialization can throw for circular structures.
 * That case is caught and reported.
 */
export function safeSetLS(key: string, value: unknown): boolean {
    let json: string;
    try {
        json = JSON.stringify(value);
    } catch (e) {
        _warnOnce(key, `stringify failed (circular reference?): ${e}`);
        return false;
    }
    try {
        localStorage.setItem(key, json);
        return true;
    } catch (e) {
        _warnOnce(key, `setItem failed (quota?): ${e}`);
        return false;
    }
}

/**
 * Write a plain string to localStorage. Returns true on success.
 */
export function safeSetLSString(key: string, value: string): boolean {
    try {
        localStorage.setItem(key, value);
        return true;
    } catch (e) {
        _warnOnce(key, `setItem string failed: ${e}`);
        return false;
    }
}

/**
 * Remove a key from localStorage without throwing.
 */
export function safeRemoveLS(key: string): void {
    try { localStorage.removeItem(key); } catch (e) {
        _warnOnce(key, `removeItem failed: ${e}`);
    }
}
