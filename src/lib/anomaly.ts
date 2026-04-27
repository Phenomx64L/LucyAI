// ── anomaly — lightweight statistical anomaly detection ─────────────────────
//
// Why exist:
//   The Dashboard already collects rolling metrics (CPU/RAM/Disk per host).
//   "85% CPU" is meaningless without context — a database server at 85% is
//   normal, a desktop at 85% is alarming.
//
//   This module turns the existing samples into per-host *deviation* signals:
//   "this value is 3.2σ above the host's recent average — investigate."
//
// Design:
//   • Pure stats (z-score over rolling window). No ML, no extra deps.
//   • Robust to small sample sizes (≥4 needed) and to zero-variance series.
//   • Returns a severity level the UI can map to color.
//
// Usage:
//   import { detectAnomaly } from '$lib/anomaly';
//   const a = detectAnomaly(history.map(s => s.cpu), currentCpu);
//   if (a.severity !== 'normal') showAlert(a.message);

export type AnomalySeverity = 'normal' | 'mild' | 'strong' | 'extreme';

export interface AnomalyResult {
    severity: AnomalySeverity;
    /** How many σ the current sample is from the mean. Negative = below. */
    sigma: number;
    /** Mean of the sample window. */
    mean: number;
    /** Standard deviation of the window. */
    stdDev: number;
    /** True if not enough data to compute (severity will be 'normal'). */
    insufficient: boolean;
    /** Optional human-readable summary, e.g. "3.2σ above recent avg (45%)". */
    message: string;
}

/** Minimum samples needed before we trust a z-score. Below this, severity='normal'. */
const MIN_SAMPLES = 4;

/** Threshold tuning — z-score absolute value mapped to severity. */
const T_MILD    = 2.0;   // 2σ ≈ p<0.05
const T_STRONG  = 3.0;   // 3σ ≈ p<0.003
const T_EXTREME = 4.0;   // 4σ ≈ p<0.0001

/**
 * Compute mean and (sample) standard deviation of a numeric array.
 * Uses Welford-style running calculation for numerical stability.
 */
export function meanStdDev(values: readonly number[]): { mean: number; stdDev: number } {
    if (values.length === 0) return { mean: 0, stdDev: 0 };
    let mean = 0;
    let m2   = 0;
    let n    = 0;
    for (const v of values) {
        if (!Number.isFinite(v)) continue;
        n += 1;
        const delta = v - mean;
        mean += delta / n;
        m2   += delta * (v - mean);
    }
    if (n < 2) return { mean, stdDev: 0 };
    return { mean, stdDev: Math.sqrt(m2 / (n - 1)) };
}

/**
 * Detect whether `current` is anomalous w.r.t. the rolling `history` window.
 *
 * Edge cases handled:
 *   • Empty / too-small window → severity 'normal', insufficient=true
 *   • Zero-variance window (all values identical) → 'normal' unless current
 *     differs from the constant (then 'extreme').
 *   • Non-finite inputs → ignored
 */
export function detectAnomaly(
    history: readonly number[],
    current: number
): AnomalyResult {
    if (!Number.isFinite(current) || history.length < MIN_SAMPLES) {
        return {
            severity: 'normal',
            sigma: 0,
            mean: 0,
            stdDev: 0,
            insufficient: true,
            message: 'Not enough data',
        };
    }

    const { mean, stdDev } = meanStdDev(history);

    // Zero-variance case: any difference is technically infinite-σ.
    if (stdDev === 0) {
        const diff = Math.abs(current - mean);
        if (diff < 1e-9) {
            return { severity: 'normal', sigma: 0, mean, stdDev, insufficient: false, message: 'stable' };
        }
        return {
            severity: 'extreme',
            sigma: Number.POSITIVE_INFINITY,
            mean, stdDev,
            insufficient: false,
            message: `step change from steady ${mean.toFixed(1)}`,
        };
    }

    const sigma = (current - mean) / stdDev;
    const abs   = Math.abs(sigma);

    let severity: AnomalySeverity = 'normal';
    if      (abs >= T_EXTREME) severity = 'extreme';
    else if (abs >= T_STRONG)  severity = 'strong';
    else if (abs >= T_MILD)    severity = 'mild';

    const direction = sigma > 0 ? 'above' : 'below';
    const message = severity === 'normal'
        ? 'within normal range'
        : `${abs.toFixed(1)}σ ${direction} recent avg (${mean.toFixed(0)})`;

    return { severity, sigma, mean, stdDev, insufficient: false, message };
}

/**
 * Convert severity to a CSS variable name compatible with Lucy's theme.
 * UI uses these for badges/borders without hardcoding hex.
 */
export function severityToCssVar(s: AnomalySeverity): string {
    switch (s) {
        case 'extreme': return 'var(--red, #ef4444)';
        case 'strong':  return 'var(--red, #ef4444)';
        case 'mild':    return 'var(--amber, #f59e0b)';
        default:        return 'var(--acc, #10b981)';
    }
}

/** Tiny helper for the dashboard: returns true if any of CPU/RAM/Disk is anomalous. */
export function anyAnomaly(
    cpuHist: readonly number[], cpu: number,
    ramHist: readonly number[], ram: number,
    diskHist: readonly number[], disk: number,
): { worst: AnomalyResult; metric: 'cpu' | 'ram' | 'disk' | null } {
    const aCpu  = detectAnomaly(cpuHist, cpu);
    const aRam  = detectAnomaly(ramHist, ram);
    const aDisk = detectAnomaly(diskHist, disk);
    const ranked = [
        { r: aCpu,  m: 'cpu'  as const },
        { r: aRam,  m: 'ram'  as const },
        { r: aDisk, m: 'disk' as const },
    ].sort((a, b) => Math.abs(b.r.sigma) - Math.abs(a.r.sigma));
    const top = ranked[0];
    return {
        worst: top.r,
        metric: top.r.severity === 'normal' ? null : top.m,
    };
}
