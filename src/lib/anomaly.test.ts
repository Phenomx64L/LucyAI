import { describe, it, expect } from 'vitest';
import { meanStdDev, detectAnomaly, anyAnomaly, severityToCssVar } from './anomaly';

describe('anomaly / meanStdDev', () => {
    it('returns zero for empty input', () => {
        const r = meanStdDev([]);
        expect(r.mean).toBe(0); expect(r.stdDev).toBe(0);
    });

    it('returns zero stddev for single sample', () => {
        const r = meanStdDev([42]);
        expect(r.mean).toBe(42); expect(r.stdDev).toBe(0);
    });

    it('matches known values for [2,4,4,4,5,5,7,9]', () => {
        const r = meanStdDev([2, 4, 4, 4, 5, 5, 7, 9]);
        expect(r.mean).toBeCloseTo(5.0, 5);
        // sample stddev = 2.13808...
        expect(r.stdDev).toBeCloseTo(2.13808993529939, 5);
    });

    it('skips non-finite inputs without crashing', () => {
        const r = meanStdDev([1, NaN, 3, Infinity, 5]);
        expect(r.mean).toBeCloseTo(3, 5);
        expect(r.stdDev).toBeGreaterThan(0);
    });
});

describe('anomaly / detectAnomaly', () => {
    it('returns insufficient when history is short', () => {
        const r = detectAnomaly([1, 2], 100);
        expect(r.insufficient).toBe(true);
        expect(r.severity).toBe('normal');
    });

    it('reports normal for in-range values', () => {
        const r = detectAnomaly([10, 12, 11, 13, 10, 12, 11], 11.5);
        expect(r.severity).toBe('normal');
        expect(Math.abs(r.sigma)).toBeLessThan(2);
    });

    it('reports mild around 2σ', () => {
        // mean=10, stddev~1 → val=12 → sigma=2
        const r = detectAnomaly([9, 10, 11, 9, 10, 11, 9, 10, 11], 12);
        expect(['mild', 'strong']).toContain(r.severity);
    });

    it('reports extreme on huge deviation', () => {
        const r = detectAnomaly([10, 11, 12, 11, 10, 12, 11, 10, 12], 100);
        expect(r.severity).toBe('extreme');
        expect(r.sigma).toBeGreaterThan(4);
    });

    it('handles zero-variance windows safely', () => {
        const r1 = detectAnomaly([5, 5, 5, 5, 5, 5], 5);
        expect(r1.severity).toBe('normal');
        const r2 = detectAnomaly([5, 5, 5, 5, 5, 5], 80);
        expect(r2.severity).toBe('extreme');
        expect(r2.sigma).toBe(Number.POSITIVE_INFINITY);
    });

    it('returns normal for non-finite current value', () => {
        const r = detectAnomaly([1, 2, 3, 4, 5], NaN);
        expect(r.severity).toBe('normal');
        expect(r.insufficient).toBe(true);
    });
});

describe('anomaly / anyAnomaly', () => {
    it('reports the worst metric when several deviate', () => {
        const cpuH  = [10, 11, 9, 10, 11, 9, 10];
        const ramH  = [40, 41, 39, 40, 41, 39, 40];
        const diskH = [60, 61, 59, 60, 61, 59, 60];
        // CPU spike 100σ-ish, RAM normal, disk normal.
        const r = anyAnomaly(cpuH, 100, ramH, 40, diskH, 60);
        expect(r.metric).toBe('cpu');
        expect(r.worst.severity).toBe('extreme');
    });

    it('returns null metric when nothing is anomalous', () => {
        const cpuH  = [10, 11, 9, 10, 11, 9, 10];
        const r = anyAnomaly(cpuH, 11, cpuH, 10.5, cpuH, 9.8);
        expect(r.metric).toBeNull();
    });
});

describe('anomaly / severityToCssVar', () => {
    it('maps each severity to a non-empty CSS var', () => {
        for (const s of ['normal', 'mild', 'strong', 'extreme'] as const) {
            const v = severityToCssVar(s);
            expect(typeof v).toBe('string');
            expect(v.length).toBeGreaterThan(0);
        }
    });
});
