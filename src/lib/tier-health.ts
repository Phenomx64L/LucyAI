// ── tier-health.ts — Lightweight LLM tier health probe (v1.7.1) ──────────
//
// Why this exists
// ───────────────
// v1.6.10 and v1.6.16 were the same bug: a frontend callsite hardcoded
// a Gemini model id that didn't exist, the backend rejected the call,
// and a silent `catch` swallowed the error so the user never knew. The
// failure surface was "tag suggestion produced nothing useful" — not
// "model ID rejected" — which is the worst kind of bug to debug.
//
// v1.7.0 centralised the model ids in `$lib/llm-models` so a typo can
// no longer slip through. But the contract between the frontend and
// Google's API is still implicit: when Google deprecates a model id,
// Lucy doesn't find out until a user happens to invoke that tier.
//
// This module probes each tier at boot — one tiny `ask_lucy` call per
// tier, cached for 6 hours in localStorage — and exposes the result
// as a Svelte store the StatusBar renders as a single health chip.
// Total cost per boot: 3 tiers × ~$0.0001 ≈ $0.0003. Cached 6h means
// at most ~4 probe cycles/day if the user reopens Lucy frequently;
// idle days cost zero.
//
// ── Probe semantics ──────────────────────────────────────────────────────
//
//   ok       — `ask_lucy` returned a non-empty string for this tier.
//               The contract is healthy.
//   slow     — call succeeded but took > 8s. The model exists but is
//               under load / cold-starting. Not failure; surfaces as a
//               warn-tone amber.
//   fail     — call rejected, threw, or timed out > 15s. The most
//               common cause historically: model id no longer accepted.
//   unknown  — never probed in this session, or cache evicted.
//
// ── On-disk cache format ────────────────────────────────────────────────
//
//   localStorage key: `lucy_tier_health_v1`
//   value: JSON({ FAST: TierHealth, CHEAP: TierHealth, REASONING: TierHealth })
//
// `built_at` on each entry lets us evict entries older than CACHE_TTL_MS.
// Bumping `v1` invalidates everyone's cache — useful if we change the
// probe semantics.

import { writable, get, type Writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { LLM } from '$lib/llm-models';
import { safeGetLS, safeSetLS } from '$lib/safe-ls';

// v1.7.3 additions:
//   - Rolling 7-day latency window (P50 / P95 surfaced in tooltip & /llm-health)
//   - Circuit breaker (auto-route REASONING → FAST after N consecutive fails)
//   - Helpers consumed by /llm-health slash command and cost dashboard.

export type TierStatus = 'ok' | 'slow' | 'fail' | 'unknown';
export type TierKey    = 'FAST' | 'CHEAP' | 'REASONING';

export interface TierHealth {
    status:     TierStatus;
    latency_ms: number;       // 0 when status is fail/unknown
    model_id:   string;       // canonical id at probe time (so tier swaps are auditable)
    built_at:   number;       // ms epoch
    error?:     string;       // present on fail
}

/** 6 hours. Why: long enough to avoid hammering the API on every reopen,
 *  short enough that a Google deprecation gets caught the same day. */
export const CACHE_TTL_MS = 6 * 60 * 60 * 1000;

/** 15s hard ceiling on each probe. Above this we mark fail — a model
 *  that takes 15s to emit one token is functionally broken for Lucy. */
const PROBE_TIMEOUT_MS = 15_000;

/** 8s soft threshold — over this is slow but not failed. */
const SLOW_THRESHOLD_MS = 8_000;

const LS_KEY = 'lucy_tier_health_v1';

// v1.7.3 — latency history.
const LS_KEY_LATENCY = 'lucy_tier_latency_v1';
/** 7 days of history. Older samples are evicted on each probe. */
const LATENCY_WINDOW_MS = 7 * 24 * 60 * 60 * 1000;
/** Hard cap on samples retained per tier. Even at 1 probe/min that's
 *  44k samples in 30 days — we want a soft ceiling. */
const LATENCY_MAX_SAMPLES = 500;

// v1.7.3 — circuit breaker.
const LS_KEY_BREAKER = 'lucy_tier_breaker_v1';
/** After this many consecutive fails on REASONING, callers should
 *  re-route to FAST automatically. 3 strikes is a conservative
 *  threshold — one transient blip won't open the breaker. */
const BREAKER_OPEN_AFTER = 3;
/** How long the breaker stays open before allowing a probe. */
const BREAKER_HALF_OPEN_AFTER_MS = 10 * 60 * 1000;   // 10 min

// ── Store ──────────────────────────────────────────────────────────────

function emptyHealth(): Record<TierKey, TierHealth> {
    const base: TierHealth = {
        status: 'unknown', latency_ms: 0, model_id: '', built_at: 0,
    };
    return { FAST: { ...base }, CHEAP: { ...base }, REASONING: { ...base } };
}

function tierToModel(t: TierKey): string {
    switch (t) {
        case 'FAST':      return LLM.FAST;
        case 'CHEAP':     return LLM.CHEAP;
        case 'REASONING': return LLM.REASONING;
    }
}

function loadCached(): Record<TierKey, TierHealth> {
    const raw = safeGetLS(LS_KEY, '');
    if (!raw) return emptyHealth();
    try {
        const parsed = JSON.parse(raw) as Record<TierKey, TierHealth>;
        const now = Date.now();
        const out = emptyHealth();
        for (const k of Object.keys(out) as TierKey[]) {
            const v = parsed[k];
            if (v && (now - v.built_at) < CACHE_TTL_MS) {
                out[k] = v;
            }
        }
        return out;
    } catch {
        return emptyHealth();
    }
}

function persist(state: Record<TierKey, TierHealth>) {
    try { safeSetLS(LS_KEY, JSON.stringify(state)); } catch { /* quota */ }
}

/** Public store. Subscribed by StatusBar (chip) and any panel that
 *  wants to gate behaviour on tier availability. */
export const tierHealth: Writable<Record<TierKey, TierHealth>> = writable(loadCached());

// ── v1.7.3 — Latency history store ─────────────────────────────────────

export interface LatencySample {
    /** ms epoch when the probe completed. */
    ts: number;
    /** Round-trip latency in ms. */
    ms: number;
}

function loadLatencyHistory(): Record<TierKey, LatencySample[]> {
    const raw = safeGetLS(LS_KEY_LATENCY, '');
    const empty: Record<TierKey, LatencySample[]> = { FAST: [], CHEAP: [], REASONING: [] };
    if (!raw) return empty;
    try {
        const parsed = JSON.parse(raw) as Record<TierKey, LatencySample[]>;
        const cutoff = Date.now() - LATENCY_WINDOW_MS;
        const out = { ...empty };
        for (const k of Object.keys(empty) as TierKey[]) {
            out[k] = (parsed[k] || []).filter(s => s.ts >= cutoff);
        }
        return out;
    } catch { return empty; }
}

function persistLatency(state: Record<TierKey, LatencySample[]>) {
    try { safeSetLS(LS_KEY_LATENCY, JSON.stringify(state)); } catch { /* quota */ }
}

export const tierLatencyHistory: Writable<Record<TierKey, LatencySample[]>> =
    writable(loadLatencyHistory());

/** Append a new latency sample, evict anything outside the 7-day window,
 *  cap to MAX_SAMPLES. Called from `pingAllTiers` for every success. */
function recordLatency(tier: TierKey, ms: number) {
    tierLatencyHistory.update(s => {
        const cutoff = Date.now() - LATENCY_WINDOW_MS;
        const next = { ...s };
        const fresh = (next[tier] || []).filter(x => x.ts >= cutoff);
        fresh.push({ ts: Date.now(), ms });
        // Keep the tail (most recent) when capping.
        if (fresh.length > LATENCY_MAX_SAMPLES) {
            fresh.splice(0, fresh.length - LATENCY_MAX_SAMPLES);
        }
        next[tier] = fresh;
        persistLatency(next);
        return next;
    });
}

/** Percentile helper. Returns 0 for empty input — caller decides
 *  what "no data" means. p in [0, 100]. */
function percentile(sorted: number[], p: number): number {
    if (sorted.length === 0) return 0;
    const idx = (p / 100) * (sorted.length - 1);
    const lo  = Math.floor(idx);
    const hi  = Math.ceil(idx);
    if (lo === hi) return sorted[lo];
    return Math.round(sorted[lo] * (hi - idx) + sorted[hi] * (idx - lo));
}

/** P50 / P95 for a tier over the rolling 7-day window. */
export function getLatencyStats(tier: TierKey): {
    samples: number; p50: number; p95: number; mean: number;
} {
    const hist = get(tierLatencyHistory)[tier] || [];
    if (hist.length === 0) return { samples: 0, p50: 0, p95: 0, mean: 0 };
    const sorted = hist.map(s => s.ms).sort((a, b) => a - b);
    const mean = Math.round(sorted.reduce((acc, x) => acc + x, 0) / sorted.length);
    return {
        samples: hist.length,
        p50:     percentile(sorted, 50),
        p95:     percentile(sorted, 95),
        mean,
    };
}

// ── v1.7.3 — Circuit breaker ────────────────────────────────────────────
//
// Lucy's REASONING tier (Gemini 3.1 Pro) is more prone to transient
// 503 / overload responses than FAST. We track consecutive failures
// per tier; when REASONING accumulates BREAKER_OPEN_AFTER, the
// breaker opens and `resolveTierWithBreaker(LLM.REASONING)` returns
// `LLM.FAST` instead. The breaker auto-closes after
// BREAKER_HALF_OPEN_AFTER_MS so we re-test the original tier.
//
// Why only REASONING and not all tiers: FAST and CHEAP have no
// graceful fallback below them, so opening their breaker would
// just disable LLM features. REASONING → FAST is a degraded but
// usable mode.

export interface TierBreakerState {
    /** Consecutive failures since the last success. Resets on success. */
    consecutive_fails: number;
    /** True when the breaker is open (callers should re-route). */
    is_open: boolean;
    /** ms epoch when the breaker opened, used to compute half-open. */
    opened_at: number;
}

function emptyBreakerState(): Record<TierKey, TierBreakerState> {
    const base: TierBreakerState = { consecutive_fails: 0, is_open: false, opened_at: 0 };
    return { FAST: { ...base }, CHEAP: { ...base }, REASONING: { ...base } };
}

function loadBreaker(): Record<TierKey, TierBreakerState> {
    const raw = safeGetLS(LS_KEY_BREAKER, '');
    if (!raw) return emptyBreakerState();
    try {
        return { ...emptyBreakerState(), ...(JSON.parse(raw) as Record<TierKey, TierBreakerState>) };
    } catch { return emptyBreakerState(); }
}

function persistBreaker(state: Record<TierKey, TierBreakerState>) {
    try { safeSetLS(LS_KEY_BREAKER, JSON.stringify(state)); } catch { /* quota */ }
}

export const tierBreaker: Writable<Record<TierKey, TierBreakerState>> =
    writable(loadBreaker());

function recordProbeResult(tier: TierKey, success: boolean) {
    tierBreaker.update(s => {
        const next = { ...s };
        const cur  = { ...next[tier] };
        if (success) {
            cur.consecutive_fails = 0;
            cur.is_open = false;
            cur.opened_at = 0;
        } else {
            cur.consecutive_fails += 1;
            if (cur.consecutive_fails >= BREAKER_OPEN_AFTER && !cur.is_open) {
                cur.is_open = true;
                cur.opened_at = Date.now();
                // eslint-disable-next-line no-console
                console.warn(`[tier-health] Circuit breaker OPEN for ${tier} after ${cur.consecutive_fails} consecutive fails`);
            }
        }
        next[tier] = cur;
        persistBreaker(next);
        return next;
    });
}

/**
 * Resolve a tier id through the circuit breaker. Use at any callsite
 * where the model is configurable so degradation is automatic:
 *
 *     import { LLM } from '$lib/llm-models';
 *     import { resolveTierWithBreaker } from '$lib/tier-health';
 *     const model = resolveTierWithBreaker(LLM.REASONING);
 *     await invoke('ask_lucy', { ..., model });
 *
 * Behaviour:
 *   - FAST / CHEAP → returned unchanged (no fallback below them).
 *   - REASONING when breaker closed → returned unchanged.
 *   - REASONING when breaker open but BREAKER_HALF_OPEN_AFTER_MS
 *     elapsed since open → returned unchanged (probe attempt).
 *   - REASONING when breaker open and within cooldown → FAST.
 */
export function resolveTierWithBreaker(rawModel: string): string {
    if (rawModel !== LLM.REASONING) return rawModel;
    const state = get(tierBreaker).REASONING;
    if (!state.is_open) return rawModel;
    const since = Date.now() - state.opened_at;
    if (since >= BREAKER_HALF_OPEN_AFTER_MS) {
        // Half-open: let one attempt through. If it fails the breaker
        // will re-open via recordProbeResult on the next probe cycle.
        return rawModel;
    }
    return LLM.FAST;
}

// ── Probing ────────────────────────────────────────────────────────────

/** Race a promise against a timeout. We can't AbortController a Tauri
 *  invoke (the Rust side will still complete), but we can stop waiting
 *  on the JS side and mark the tier failed. */
function withTimeout<T>(p: Promise<T>, ms: number): Promise<T> {
    return new Promise((resolve, reject) => {
        const t = setTimeout(() => reject(new Error(`timeout ${ms}ms`)), ms);
        p.then(v => { clearTimeout(t); resolve(v); },
               e => { clearTimeout(t); reject(e); });
    });
}

/** Probe one tier. Returns the result without mutating the store — the
 *  caller decides when to commit (so we can batch all three before
 *  triggering one reactive update). */
async function probeOne(tier: TierKey): Promise<TierHealth> {
    const model_id = tierToModel(tier);
    const built_at = Date.now();
    const t0 = performance.now();
    try {
        // Minimal prompt. We want a 1-token response so cost ≈ floor.
        // "ok" is well-tokenised across both Spanish and English contexts
        // so neither locale path produces extra padding.
        const result = await withTimeout(invoke<string>('ask_lucy', {
            prompt:      'Respond with the single word: ok',
            context:     '',
            userName:    'lucy-health-check',
            runbooksDir: null,
            model:       model_id,
            images:      null,
            lang:        'en',
            hostsJson:   null,
            maxTokensOverride: 8,   // bound the response in case the model rambles
        }), PROBE_TIMEOUT_MS);
        const latency_ms = Math.round(performance.now() - t0);
        const non_empty = typeof result === 'string' && result.trim().length > 0;
        if (!non_empty) {
            return { status: 'fail', latency_ms, model_id, built_at,
                     error: 'empty response' };
        }
        const status: TierStatus = latency_ms > SLOW_THRESHOLD_MS ? 'slow' : 'ok';
        return { status, latency_ms, model_id, built_at };
    } catch (e) {
        const latency_ms = Math.round(performance.now() - t0);
        return { status: 'fail', latency_ms, model_id, built_at,
                 error: String(e).slice(0, 200) };
    }
}

/** Probe all three tiers in parallel. Returns when ALL have settled
 *  (Promise.all over Promise-wrapped functions that never reject).
 *  v1.7.3: also records latency samples + drives circuit breaker. */
export async function pingAllTiers(): Promise<void> {
    const tiers: TierKey[] = ['FAST', 'CHEAP', 'REASONING'];
    const results = await Promise.all(tiers.map(probeOne));
    const next = emptyHealth();
    tiers.forEach((t, i) => {
        next[t] = results[i];
        // Record latency for successful (ok/slow) probes only — fails
        // would skew the rolling distribution with timeout-clamped
        // values that don't reflect real-API latency.
        const r = results[i];
        if (r.status === 'ok' || r.status === 'slow') {
            recordLatency(t, r.latency_ms);
            recordProbeResult(t, true);
        } else {
            recordProbeResult(t, false);
        }
    });
    tierHealth.set(next);
    persist(next);
}

/** Boot helper. Runs `pingAllTiers` only if any tier's cached entry is
 *  stale (older than `CACHE_TTL_MS`) or unknown. Avoids burning API
 *  calls on every reopen during a single day. */
export async function pingAllTiersIfStale(): Promise<void> {
    const now = Date.now();
    const cur = get(tierHealth);
    const stale = (Object.keys(cur) as TierKey[]).some(k => {
        const e = cur[k];
        return e.status === 'unknown' || (now - e.built_at) > CACHE_TTL_MS;
    });
    if (stale) {
        await pingAllTiers();
    }
}

// ── Derived helpers (used by the StatusBar chip) ────────────────────────

/** Aggregate status: worst tier wins. fail > slow > ok > unknown. */
export function aggregateStatus(s: Record<TierKey, TierHealth>): TierStatus {
    const vals = [s.FAST.status, s.CHEAP.status, s.REASONING.status];
    if (vals.includes('fail'))    return 'fail';
    if (vals.includes('slow'))    return 'slow';
    if (vals.every(v => v === 'ok')) return 'ok';
    return 'unknown';
}

/** Glyph + tone label per status. Used by the StatusBar chip CSS. */
export function statusGlyph(s: TierStatus): { glyph: string; tone: string } {
    switch (s) {
        case 'ok':      return { glyph: '◉', tone: 'ok'   };
        case 'slow':    return { glyph: '◑', tone: 'warn' };
        case 'fail':    return { glyph: '◯', tone: 'crit' };
        case 'unknown': return { glyph: '·', tone: 'info' };
    }
}
