// ── cache-stats-helpers.test.ts ───────────────────────────────────────────
//
// Sprint 5 follow-up — Coverage for the UI-7 badge logic. Pins the cache
// hit ratio formula and the three color tiers so any future "tweak the
// threshold" change has to be deliberate (i.e. update the test too).

import { describe, it, expect } from 'vitest';
import {
    computeCacheHitPct,
    cacheHitTier,
    type CacheStats,
} from './cache-stats-helpers';

// Helper — minimal stats with sensible defaults so each test only specifies
// the fields it actually exercises.
function stats(overrides: Partial<CacheStats> = {}): CacheStats {
    return {
        input_tokens_total: 0,
        cache_creation_total: 0,
        cache_read_total: 0,
        calls_with_cache_activity: 0,
        calls_total_anthropic: 0,
        ...overrides,
    };
}

describe('computeCacheHitPct', () => {
    it('returns null when stats is null/undefined (hide badge entirely)', () => {
        expect(computeCacheHitPct(null)).toBeNull();
        expect(computeCacheHitPct(undefined)).toBeNull();
    });

    it('returns null when no anthropic call has touched the cache yet', () => {
        // A fresh session with API calls but none using cache_control →
        // we hide the badge so the user isn't confronted with "0% cached".
        expect(computeCacheHitPct(stats({
            input_tokens_total: 5000,
            calls_total_anthropic: 3,
        }))).toBeNull();
    });

    it('returns null when total tokens is zero (no signal)', () => {
        expect(computeCacheHitPct(stats({
            calls_with_cache_activity: 1,
            calls_total_anthropic: 1,
        }))).toBeNull();
    });

    it('reports 100% when all input was served from cache', () => {
        const pct = computeCacheHitPct(stats({
            input_tokens_total: 0,
            cache_read_total: 10_000,
            calls_with_cache_activity: 1,
        }));
        expect(pct).toBe(100);
    });

    it('reports 0% when cache_read is zero but activity flag is set', () => {
        // Edge case: a write-only call (cache_creation > 0, cache_read = 0).
        // calls_with_cache_activity gets bumped but no read tokens yet.
        // The user should see "0% caché" — a real signal that the cache is
        // being primed but not yet paying off.
        const pct = computeCacheHitPct(stats({
            input_tokens_total: 5000,
            cache_creation_total: 5000,
            cache_read_total: 0,
            calls_with_cache_activity: 1,
        }));
        expect(pct).toBe(0);
    });

    it('uses (cache_read + input_uncached) as denominator', () => {
        // 6000 cache reads + 4000 uncached input = 10000 denominator.
        // cache_creation does NOT enter the denominator — it would penalize
        // the act of populating the cache.
        const pct = computeCacheHitPct(stats({
            input_tokens_total: 4000,
            cache_creation_total: 99_999,  // intentionally huge — should not matter
            cache_read_total: 6000,
            calls_with_cache_activity: 5,
        }));
        expect(pct).toBe(60);
    });

    it('handles fractional percentages without rounding', () => {
        // 1 cache_read out of 3 total → 33.33…%
        const pct = computeCacheHitPct(stats({
            input_tokens_total: 2,
            cache_read_total: 1,
            calls_with_cache_activity: 1,
        }));
        expect(pct).toBeCloseTo(33.33, 1);
    });
});

describe('cacheHitTier', () => {
    it('maps ≥30% to cok (green/healthy)', () => {
        expect(cacheHitTier(30)).toBe('cok');
        expect(cacheHitTier(75)).toBe('cok');
        expect(cacheHitTier(100)).toBe('cok');
    });

    it('maps 10..30 to cy (yellow/working)', () => {
        expect(cacheHitTier(10)).toBe('cy');
        expect(cacheHitTier(15)).toBe('cy');
        expect(cacheHitTier(29.9)).toBe('cy');
    });

    it('maps <10% to cm (muted/barely helping)', () => {
        expect(cacheHitTier(0)).toBe('cm');
        expect(cacheHitTier(5)).toBe('cm');
        expect(cacheHitTier(9.9)).toBe('cm');
    });

    it('threshold boundaries are inclusive of the higher tier', () => {
        // Exactly 30 → cok (not cy). Exactly 10 → cy (not cm).
        // These are user-facing — flipping inclusivity changes what the
        // user sees at the round numbers they're most likely to land on.
        expect(cacheHitTier(30.0)).toBe('cok');
        expect(cacheHitTier(10.0)).toBe('cy');
    });
});
