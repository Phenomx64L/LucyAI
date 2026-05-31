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
 *  (Promise.all over Promise-wrapped functions that never reject). */
export async function pingAllTiers(): Promise<void> {
    const tiers: TierKey[] = ['FAST', 'CHEAP', 'REASONING'];
    const results = await Promise.all(tiers.map(probeOne));
    const next = emptyHealth();
    tiers.forEach((t, i) => { next[t] = results[i]; });
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
