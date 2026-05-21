// ── knowledge-graph-loop.ts — F9 indexer scheduler ───────────────────────
//
// Walks the user's registered roots every 5 min, recording files modified
// in that window. Skipped when document is hidden or when no roots are set.
//
// Errors swallowed (observability tool — never blocks UX).

import { invoke } from '@tauri-apps/api/core';

const INTERVAL_MS = 5 * 60 * 1000;   // 5 minutes
const INITIAL_DELAY_MS = 45 * 1000;  // 45s after boot — let the FS settle
const SINCE_WINDOW_MIN = 7;          // scan files modified in last 7 min (slight overlap with interval)

let _initialTimer: ReturnType<typeof setTimeout> | null = null;
let _intervalTimer: ReturnType<typeof setInterval> | null = null;

async function tick(): Promise<void> {
    if (typeof document !== 'undefined' && document.visibilityState !== 'visible') return;
    try {
        // No roots → no work (kg_index_now returns empty in that case).
        const r = await invoke<{ total_files_recorded: number; total_edges: number }>('kg_index_now', { sinceMin: SINCE_WINDOW_MIN });
        if (r && (r.total_files_recorded > 0 || r.total_edges > 0)) {
            console.debug(`[kg] +${r.total_files_recorded} files, +${r.total_edges} edges`);
        }
    } catch (e) {
        console.warn('[kg] index error:', e);
    }
}

/** Start the KG indexer loop. Returns a stop fn. */
export function startKnowledgeGraphLoop(): () => void {
    _initialTimer = setTimeout(tick, INITIAL_DELAY_MS);
    _intervalTimer = setInterval(tick, INTERVAL_MS);
    return stopKnowledgeGraphLoop;
}

export function stopKnowledgeGraphLoop(): void {
    if (_initialTimer)  { clearTimeout(_initialTimer);   _initialTimer = null; }
    if (_intervalTimer) { clearInterval(_intervalTimer); _intervalTimer = null; }
}

/** Manual trigger — useful when wiring a "/kg-scan-now" slash command. */
export async function manualKnowledgeGraphScan(sinceMin = 60 * 24): Promise<unknown> {
    return invoke('kg_index_now', { sinceMin });
}
