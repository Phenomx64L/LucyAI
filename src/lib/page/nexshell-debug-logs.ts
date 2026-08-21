// La interfaz en cinco idiomas. `tr` y no `$trad`: esto no es un
// componente, así que no hay nada a lo que suscribirse. Ver `$lib/i18n`.
import { tr } from '$lib/i18n';
// ── NexShell debug logs (extracted from NexShellView.svelte, May 2026) ────
//
// Was 80 lines of state + logic at the top of a 3,649-line file. Extracted
// so future maintenance of the logging layer doesn't require scrolling past
// every other NexShell subsystem to find it.
//
// Why a module (not a Svelte store): the buffer is consumed by a download
// button and `window.__lucy_debug_export` — neither needs reactivity. A
// plain in-memory array + module-level state is the cheapest correct shape.
//
// Buffer size dropped from 2000 → 500 entries. Each entry is ~200 bytes
// after JSON.stringify of `data`, so 500 × 200 = ~100 KB upper bound. That's
// plenty for a debugging session; if you need more, hit "Download logs" to
// flush to disk and keep working.

const MAX_LOGS = 500;

export interface DebugLogEntry {
    timestamp: string;
    category:  string;
    message:   string;
    data:      unknown;
}

let debugLogs: DebugLogEntry[] = [];

/** Append a single entry. Mirrors to console + exposes under window for
 *  the "copy logs" muscle-memory devs already have. */
export function addDebugLog(category: string, message: string, data: unknown = null): void {
    const timestamp = new Date().toISOString();
    const entry: DebugLogEntry = { timestamp, category, message, data };
    debugLogs = [entry, ...debugLogs].slice(0, MAX_LOGS);

    // Real-time console mirror — single line so DevTools can collapse / filter.
    const logMsg = `[${timestamp}] [${category}] ${message}${data ? ' → ' + JSON.stringify(data) : ''}`;
    // eslint-disable-next-line no-console
    console.log(logMsg);

    // Window globals: kept for backward compat with anyone who ran
    // `window.__lucy_debug_export()` in DevTools before this refactor.
    if (typeof window !== 'undefined') {
        (window as any).__lucy_debug_logs = debugLogs;
        (window as any).__lucy_debug_export = () => {
            const content = debugLogs
                .map(l => `${l.timestamp} [${l.category}] ${l.message}${l.data ? ' → ' + JSON.stringify(l.data) : ''}`)
                .join('\n');
            // eslint-disable-next-line no-console
            console.log('=== COPY LOGS BELOW ===');
            // eslint-disable-next-line no-console
            console.log(content);
            // eslint-disable-next-line no-console
            console.log('=== END LOGS ===');
            return content;
        };
    }
}

/** Read-only snapshot of the current buffer. Callers that need to render
 *  the list should subscribe via Svelte reactivity to the source of truth
 *  in the parent — this module doesn't drive any UI directly. */
export function getDebugLogs(): readonly DebugLogEntry[] {
    return debugLogs;
}

/** Trigger a browser download of the current logs as a .log file.
 *  Takes a toast callback so this module stays UI-library-free —
 *  the caller (NexShellView) already has its own toast plumbing. */
export function downloadDebugLogs(
    toast: (msg: string, level?: string) => void,
    isEN: boolean,
): void {
    try {
        const content = debugLogs
            .map(l => `${l.timestamp} [${l.category}] ${l.message}${l.data ? ' → ' + JSON.stringify(l.data) : ''}`)
            .join('\n');
        if (!content.trim()) {
            toast(tr('No hay logs disponibles'), 'info');
            return;
        }
        const blob = new Blob([content], { type: 'text/plain;charset=utf-8' });
        const url = URL.createObjectURL(blob);
        const link = document.createElement('a');
        link.setAttribute('href', url);
        link.setAttribute('download', `nexshell-debug-${Date.now()}.log`);
        link.style.visibility = 'hidden';
        document.body.appendChild(link);
        link.click();
        document.body.removeChild(link);
        // Delay revoke so the download has time to start. Without this,
        // some browsers cancel the download as the URL is freed mid-flight.
        setTimeout(() => URL.revokeObjectURL(url), 1000);
        toast(tr('Logs descargados'), 'success');
    } catch (e) {
        toast(`Error: ${e}`, 'error');
        // eslint-disable-next-line no-console
        console.error('Download failed:', e);
    }
}

/** Wipe all in-memory logs. Useful for tests + the rare case where a
 *  session accumulates sensitive command output. */
export function clearDebugLogs(): void {
    debugLogs = [];
    if (typeof window !== 'undefined') {
        (window as any).__lucy_debug_logs = debugLogs;
    }
}
