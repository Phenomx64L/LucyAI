// ── debug — central logger for Lucy frontend ───────────────────────────────
//
// Why:
//   • Production WebView2 should NOT show developer noise (perf + UX).
//   • Errors and warnings stay always-on so real problems are visible.
//   • A single chokepoint lets us later route logs to a ring-buffer for the
//     Bug Report, or expose a runtime toggle ("verbose mode").
//
// Usage:
//   import { debug } from '$lib/debug';
//   debug.log('[NexShell] connected to', host);   // dev-only
//   debug.warn('[storage] parse failed:', e);     // always logged
//   debug.error('[ai] stream broken:', e);        // always logged + tagged
//
// Drop-in replacement for `console.log` / `console.warn` / `console.error`.

// Vite's import.meta.env.DEV is `true` in `npm run dev`, `false` in `npm run build`.
// Tauri webview gets the build artifact in production → DEV is false there.
const _IS_DEV: boolean = (() => {
    try { return Boolean((import.meta as any)?.env?.DEV); }
    catch { return false; }
})();

// Runtime override — useful when troubleshooting a packaged app:
//   localStorage.setItem('lucy_debug', '1') → reload → debug.log() now visible.
const _RUNTIME_FLAG: boolean = (() => {
    try { return localStorage.getItem('lucy_debug') === '1'; }
    catch { return false; }
})();

const _ENABLED = _IS_DEV || _RUNTIME_FLAG;

// Ring buffer of recent warns/errors. Bug Report pulls these so the user can
// share a snapshot of recent silent failures even without DevTools open.
const _RING_MAX = 100;
const _ring: { ts: number; level: 'warn' | 'error'; args: unknown[] }[] = [];
function _pushRing(level: 'warn' | 'error', args: unknown[]) {
    _ring.push({ ts: Date.now(), level, args });
    if (_ring.length > _RING_MAX) _ring.shift();
}
export function getRecentLogs() {
    return _ring.slice();
}

export const debug = {
    /** Verbose log — only emitted when DEV mode or runtime flag is on. */
    log: (...args: unknown[]) => { if (_ENABLED) try { console.log(...args); } catch {} },
    /** Same as log() but visually grouped under a label */
    info: (...args: unknown[]) => { if (_ENABLED) try { console.info(...args); } catch {} },
    /** Always emitted. Captured into ring buffer for Bug Report. */
    warn: (...args: unknown[]) => {
        try { console.warn(...args); } catch {}
        _pushRing('warn', args);
    },
    /** Always emitted. Captured into ring buffer for Bug Report. */
    error: (...args: unknown[]) => {
        try { console.error(...args); } catch {}
        _pushRing('error', args);
    },
    /** True if verbose logs are flowing — useful for guards: `if (debug.enabled) computeExpensiveLog()` */
    get enabled() { return _ENABLED; },
};
