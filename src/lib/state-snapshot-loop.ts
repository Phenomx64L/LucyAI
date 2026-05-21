// ── state-snapshot-loop.ts — Periodic system snapshot scheduler ──────────
//
// Runs a low-frequency loop (default 15 min) that triggers the Rust
// `state_snapshot_capture` command. This builds the temporal database
// that powers F2 (state diff) and F3 (causal inference) — without this
// loop, Lucy has no historical state to compare against.
//
// Design:
//   • One snapshot every 15 min (configurable)
//   • First snapshot 30s after app boot (lets the system settle)
//   • Skipped if the document is hidden (laptop closed, app backgrounded)
//   • Skipped if last capture < 60s ago (manual capture race protection)
//   • Errors logged to console but never thrown — observability tool, not critical path

import { invoke } from '@tauri-apps/api/core';

const CAPTURE_INTERVAL_MS = 15 * 60 * 1000; // 15 minutes
const INITIAL_DELAY_MS    = 30 * 1000;       // 30 sec after boot
const MIN_GAP_MS          = 60 * 1000;       // 60 sec dedup window

let _initialTimer: ReturnType<typeof setTimeout> | null = null;
let _intervalTimer: ReturnType<typeof setInterval> | null = null;
let _lastCaptureMs = 0;

async function tryCapture(reason: string): Promise<void> {
    if (typeof document !== 'undefined' && document.visibilityState !== 'visible') {
        // Skip capture when window is hidden — saves battery, avoids zombie snapshots
        return;
    }
    const sinceLast = Date.now() - _lastCaptureMs;
    if (sinceLast < MIN_GAP_MS) return;
    _lastCaptureMs = Date.now();
    try {
        const id = await invoke<number>('state_snapshot_capture');
        console.debug(`[state-snapshot] Captured (${reason}) → id=${id}`);
    } catch (e) {
        console.warn('[state-snapshot] Capture failed:', e);
    }
}

/** Start the snapshot loop. Returns a stop fn. Call once at app boot. */
export function startSnapshotLoop(): () => void {
    _initialTimer = setTimeout(() => tryCapture('initial'), INITIAL_DELAY_MS);
    _intervalTimer = setInterval(() => tryCapture('periodic'), CAPTURE_INTERVAL_MS);
    return stopSnapshotLoop;
}

export function stopSnapshotLoop(): void {
    if (_initialTimer)  { clearTimeout(_initialTimer);   _initialTimer = null; }
    if (_intervalTimer) { clearInterval(_intervalTimer); _intervalTimer = null; }
}

/** Manual capture trigger (for "/snapshot" slash command or button). */
export async function manualSnapshot(): Promise<number | null> {
    try {
        const id = await invoke<number>('state_snapshot_capture');
        _lastCaptureMs = Date.now();
        return id;
    } catch (e) {
        console.warn('[state-snapshot] Manual capture failed:', e);
        return null;
    }
}
