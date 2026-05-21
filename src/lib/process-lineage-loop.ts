// ── process-lineage-loop.ts — F1 polling loop ────────────────────────────
//
// Calls `process_lineage_poll` on the Rust backend every N seconds so that
// short-lived processes still get a chance to be recorded between two
// polls. 8s is the default: empirically catches most processes (LLM
// inference jobs, build steps, scheduled tasks) without measurable
// perf cost on a modern desktop.
//
// Skipped when the window is hidden (battery friendliness).
// Errors swallowed — this is observational; failures shouldn't disrupt UX.

import { invoke } from '@tauri-apps/api/core';

const POLL_INTERVAL_MS = 8 * 1000;  // 8 seconds
const INITIAL_DELAY_MS = 10 * 1000; // 10s after boot — let the system settle

let _initialTimer: ReturnType<typeof setTimeout> | null = null;
let _intervalTimer: ReturnType<typeof setInterval> | null = null;

async function tick(): Promise<void> {
    if (typeof document !== 'undefined' && document.visibilityState !== 'visible') return;
    try {
        const [newCount, vanishedCount] = await invoke<[number, number]>('process_lineage_poll');
        if (newCount > 0 || vanishedCount > 0) {
            console.debug(`[lineage] +${newCount} new, -${vanishedCount} vanished`);
        }
    } catch (e) {
        console.warn('[lineage] poll error:', e);
    }
}

/** Start the lineage polling loop. Returns a stop fn. */
export function startProcessLineageLoop(): () => void {
    _initialTimer = setTimeout(tick, INITIAL_DELAY_MS);
    _intervalTimer = setInterval(tick, POLL_INTERVAL_MS);
    return stopProcessLineageLoop;
}

export function stopProcessLineageLoop(): void {
    if (_initialTimer)  { clearTimeout(_initialTimer);   _initialTimer = null; }
    if (_intervalTimer) { clearInterval(_intervalTimer); _intervalTimer = null; }
}
