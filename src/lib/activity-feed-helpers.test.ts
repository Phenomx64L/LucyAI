// ── activity-feed-helpers.test.ts ─────────────────────────────────────────
//
// Sprint 5, TEST-4 — Vitest coverage for the formatting helpers used by
// ActivityFeedWidget. These functions are user-facing — every event row in
// the sidebar runs through them — so the labels they emit must stay stable.

import { describe, it, expect } from 'vitest';
import { relTime, sevClass, kindIcon } from './activity-feed-helpers';

describe('relTime', () => {
    const now = 1_000_000_000;

    it('returns seconds for ages under a minute', () => {
        expect(relTime(now - 0,  now)).toBe('0s');
        expect(relTime(now - 30, now)).toBe('30s');
        expect(relTime(now - 59, now)).toBe('59s');
    });

    it('returns minutes for ages between 1m and 59m', () => {
        expect(relTime(now - 60,    now)).toBe('1m');
        expect(relTime(now - 90,    now)).toBe('1m');   // floored
        expect(relTime(now - 3_599, now)).toBe('59m');
    });

    it('returns hours for ages between 1h and 23h', () => {
        expect(relTime(now - 3_600,  now)).toBe('1h');
        expect(relTime(now - 7_200,  now)).toBe('2h');
        expect(relTime(now - 86_399, now)).toBe('23h');
    });

    it('returns days for anything ≥1d', () => {
        expect(relTime(now - 86_400,  now)).toBe('1d');
        expect(relTime(now - 172_800, now)).toBe('2d');
        expect(relTime(now - 3_000_000, now)).toBe('34d');
    });

    it('clamps negative ages to 0s (clock skew protection)', () => {
        // If a backend timestamp is slightly in the future (NTP drift), we
        // shouldn't say "-5s" — we say "0s". The Math.max(0, …) inside
        // relTime is the guard.
        expect(relTime(now + 5, now)).toBe('0s');
    });
});

describe('sevClass', () => {
    it('maps known severities to dedicated classes', () => {
        expect(sevClass('error')).toBe('sev-error');
        expect(sevClass('warn')).toBe('sev-warn');
        expect(sevClass('ok')).toBe('sev-ok');
    });

    it('falls back to sev-info for unknown severities', () => {
        expect(sevClass('info')).toBe('sev-info');     // explicit info
        expect(sevClass('debug')).toBe('sev-info');    // unknown
        expect(sevClass('')).toBe('sev-info');         // empty
        expect(sevClass('UNKNOWN')).toBe('sev-info');  // case-sensitive miss
    });
});

describe('kindIcon', () => {
    it('uses geometric glyphs (no emojis) per kind', () => {
        // These specific glyphs were chosen to match Lucy's visual vocabulary
        // — changing them is a UX decision, not a refactor. If a test breaks
        // here, confirm the change is intentional before updating the assertion.
        expect(kindIcon('incident')).toBe('◆');
        expect(kindIcon('audit')).toBe('›');
        expect(kindIcon('rollup')).toBe('◇');
        expect(kindIcon('snapshot')).toBe('◫');
        expect(kindIcon('frontier')).toBe('⌬');
    });

    it('falls back to · for unknown kinds', () => {
        expect(kindIcon('unknown')).toBe('·');
        expect(kindIcon('')).toBe('·');
        expect(kindIcon('INCIDENT')).toBe('·'); // case-sensitive
    });
});
