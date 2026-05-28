// ── cache-stats-helpers.ts ────────────────────────────────────────────────
//
// Sprint 5 follow-up — Pure helpers extracted from StatusBar.svelte (UI-7)
// so vitest can exercise the badge logic without the full component overhead.
//
// The footer indicator shows a "X% caché" badge whose color tier changes at
// 30% (green/cok) and 10% (yellow/cy). Below 10% we use the muted/cm class.
// These thresholds are UX decisions — pinning them with tests prevents the
// "I'll just tweak this one number" drift that erodes a deliberate design.

export interface CacheStats {
    /** Tokens billed at full input price (no cache involved). */
    input_tokens_total: number;
    /** Tokens written into the cache (Anthropic bills 1.25× of input). */
    cache_creation_total: number;
    /** Tokens served FROM cache (Anthropic bills 0.1×). The win. */
    cache_read_total: number;
    /** How many anthropic calls actually exercised the cache. */
    calls_with_cache_activity: number;
    /** Total anthropic calls — denominator for the activity ratio. */
    calls_total_anthropic: number;
}

/**
 * Compute the cache hit percentage from raw stats.
 *
 *   hit% = cache_read / (cache_read + input_uncached)
 *
 * Why this denominator and not (cache_read + cache_creation + input)?
 * Because cache_creation is a one-time write that pays off on subsequent
 * reads — counting writes against the hit rate would penalize the very
 * thing AI-1 was built to enable.
 *
 * Returns null when there's no signal to report — either no anthropic calls
 * have happened yet, or none of them touched the cache. The UI uses this
 * null as "hide the badge entirely" so a fresh app session doesn't show a
 * confusing 0%.
 */
export function computeCacheHitPct(stats: CacheStats | null | undefined): number | null {
    if (!stats) return null;
    const total = stats.input_tokens_total + stats.cache_read_total;
    if (total === 0 || stats.calls_with_cache_activity === 0) return null;
    return (stats.cache_read_total / total) * 100;
}

/**
 * Map a hit percentage to a footer-badge CSS class. The thresholds are:
 *   • ≥30%  → 'cok' (green, healthy AI-1 deploy)
 *   • ≥10%  → 'cy'  (yellow, working but unimpressive)
 *   •  <10% → 'cm'  (muted, cache present but barely helping)
 */
export function cacheHitTier(pct: number): 'cok' | 'cy' | 'cm' {
    if (pct >= 30) return 'cok';
    if (pct >= 10) return 'cy';
    return 'cm';
}
