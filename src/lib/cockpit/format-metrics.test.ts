// ── format-metrics.test.ts ───────────────────────────────────────────────────
//
// Both functions replace a display that rounded a correctly-measured value into
// uselessness. The tests therefore pin the SMALL end hardest: that is where the
// old code collapsed everything to a constant, and where a future "simplify
// this" would collapse it again without breaking anything obvious.

import { describe, it, expect } from 'vitest';
import { fmtRate, fmtUptime } from './format-metrics';

describe('fmtRate', () => {
    it('renders real idle traffic instead of flattening it to zero', () => {
        // Measured on the affected host over a 3 s window: 0.005 Mbps down,
        // 0.062 up. Both rendered "0.0 Mbps" before — the panel looked dead.
        expect(fmtRate(0.005)).toEqual({ n: 5, u: 'kbps' });
        expect(fmtRate(0.062)).toEqual({ n: 62, u: 'kbps' });
    });

    it('keeps 1 kbps of resolution, which is why the backend sends 3 decimals', () => {
        expect(fmtRate(0.001)).toEqual({ n: 1, u: 'kbps' });
        expect(fmtRate(0.049)).toEqual({ n: 49, u: 'kbps' });   // under the old 0.05 floor
    });

    it('switches to Mbps at exactly 1', () => {
        expect(fmtRate(0.999)).toEqual({ n: 999, u: 'kbps' });
        expect(fmtRate(1)).toEqual({ n: '1.0', u: 'Mbps' });
    });

    it('formats fast links readably', () => {
        expect(fmtRate(1.25)).toEqual({ n: '1.3', u: 'Mbps' });
        expect(fmtRate(943.7)).toEqual({ n: '943.7', u: 'Mbps' });
    });

    it('treats absent, negative and non-numeric input as zero', () => {
        for (const bad of [0, -1, null, undefined, NaN, 'x']) {
            expect(fmtRate(bad)).toEqual({ n: 0, u: 'kbps' });
        }
    });
});

describe('fmtUptime', () => {
    it('does not report a freshly booted host as "0 h"', () => {
        // THE case. "0 h" is indistinguishable from "unknown", in the exact
        // window where the operator is asking whether the box just rebooted.
        expect(fmtUptime({ uptime_s: 42 })).toBe('<1 min');
        expect(fmtUptime({ uptime_s: 600 })).toBe('10 min');
        expect(fmtUptime({ uptime_s: 3540 })).toBe('59 min');
    });

    it('matches the host it was verified against', () => {
        // Real reading during the fix: 1.69 h / 101 min, displayed as "1 h".
        expect(fmtUptime({ uptime_s: 101 * 60 })).toBe('1 h 41 min');
    });

    it('drops the minutes when they are zero', () => {
        expect(fmtUptime({ uptime_s: 3600 })).toBe('1 h');
        expect(fmtUptime({ uptime_s: 86_400 })).toBe('1 d');
    });

    it('switches to days past 24 h', () => {
        expect(fmtUptime({ uptime_s: 25 * 3600 })).toBe('1 d 1 h');
        expect(fmtUptime({ uptime_s: 40 * 86_400 + 7 * 3600 })).toBe('40 d 7 h');
    });

    it('falls back to whole hours for remote hosts, which only send those', () => {
        // hosts.rs reports uptime_h and no uptime_s — not dead code.
        expect(fmtUptime({ uptime_h: 5 })).toBe('5 h');
        expect(fmtUptime({ uptime_h: 0 })).toBe('0 h');
    });

    it('never throws on a malformed or missing payload', () => {
        expect(fmtUptime(null)).toBe('0 h');
        expect(fmtUptime(undefined)).toBe('0 h');
        expect(fmtUptime({})).toBe('0 h');
        expect(fmtUptime({ uptime_s: 'nope' })).toBe('0 h');
        expect(fmtUptime({ uptime_s: -5 })).toBe('0 h');
    });
});
