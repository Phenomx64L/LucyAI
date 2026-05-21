// ── heat-layer.ts — U4 visual overlay helpers ────────────────────────────
//
// Pure functions that map a value (severity, recency, etc.) to a CSS class
// or inline style. The component using them just adds the class — all
// visual rules live in heat-layer.css (loaded by app.css).
//
// Two channels of "heat":
//   1. Severity heat   — value 0-1, mapped to green→amber→red ramps. Use for
//                        scores like threat_score, CPU%, error rate.
//   2. Recency heat    — unix timestamp, mapped to opacity + accent glow.
//                        "Fresh" (last 5 min) → bright; older → fading.
//
// Both are NON-DESTRUCTIVE: applied as overlay classes that compose with
// the host element's existing styling.

export type HeatBand = 'cool' | 'warm' | 'hot' | 'critical';

/** Map a 0-1 score to a 4-band class name. */
export function severityBand(score: number): HeatBand {
    if (!Number.isFinite(score)) return 'cool';
    if (score >= 0.75) return 'critical';
    if (score >= 0.50) return 'hot';
    if (score >= 0.25) return 'warm';
    return 'cool';
}

/** CSS class for an element that should reflect a severity. */
export function severityClass(score: number, prefix = 'heat'): string {
    return `${prefix}-${severityBand(score)}`;
}

/**
 * Map a unix timestamp (seconds) to a "recency intensity" 0-1.
 *   <5 min   → 1.0
 *   <30 min  → 0.7
 *   <2 hr    → 0.4
 *   <24 hr   → 0.2
 *   older    → 0.05
 */
export function recencyIntensity(unixSec: number, nowSec?: number): number {
    const now = nowSec ?? Math.floor(Date.now() / 1000);
    if (!Number.isFinite(unixSec) || unixSec <= 0) return 0;
    const age = now - unixSec;
    if (age < 5 * 60) return 1.0;
    if (age < 30 * 60) return 0.7;
    if (age < 2 * 3600) return 0.4;
    if (age < 24 * 3600) return 0.2;
    return 0.05;
}

/** Inline style fragment for a recency overlay — opacity + glow. */
export function recencyStyle(unixSec: number): string {
    const i = recencyIntensity(unixSec);
    // Glow: only above 0.3 — older items don't shimmer
    const glow = i >= 0.3 ? `0 0 ${Math.round(8 * i)}px rgba(16,185,129,${(0.25 * i).toFixed(2)})` : 'none';
    return `--heat-recency: ${i.toFixed(2)}; box-shadow: ${glow};`;
}

/** One-shot: returns BOTH severity class + recency style. Convenient for templates. */
export function heatAttrs(score: number, unixSec?: number): { className: string; style: string } {
    return {
        className: severityClass(score),
        style: unixSec ? recencyStyle(unixSec) : '',
    };
}

/** Map a 0-1 score to a tiny ascii bar (for monospace lists). */
export function heatBar(score: number, width = 6): string {
    if (!Number.isFinite(score)) return ' '.repeat(width);
    const filled = Math.round(score * width);
    return '█'.repeat(filled) + '░'.repeat(width - filled);
}
