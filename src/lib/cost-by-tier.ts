// ── cost-by-tier.ts — Attribute spend per LLM tier (v1.7.3) ──────────────
//
// The backend already returns a per-model breakdown in `CostSummary.per_model`
// (see `metrics.rs::get_cost_summary`). This module groups that breakdown
// by the same tier vocabulary `$lib/llm-models` exposes (FAST / CHEAP /
// REASONING / VISION / LEGACY) so the cost dashboard can show:
//
//   FAST       $1.43   18.4k tokens   34 requests
//   CHEAP      $0.27    6.0k tokens   88 requests
//   REASONING  $4.12    3.1k tokens    7 requests
//
// instead of a flat list where the user has to mentally match every model
// id back to a tier. Same data, different aggregation, no backend change.

import { LLM, type LlmTier } from '$lib/llm-models';

export interface ModelCostRow {
    model:    string;
    cost:     number;
    tokens:   number;
    requests: number;
}

export interface TierCostRow {
    tier:     LlmTier;
    cost:     number;
    tokens:   number;
    requests: number;
    /** The individual model ids that contributed to this tier — handy
     *  for the tooltip ("FAST aggregates gemini-3.5-flash + the legacy
     *  alias gemini-3-flash-preview"). */
    models:   string[];
}

/** Reverse map from model id → tier. We include the legacy alias so
 *  historical token_usage rows keep grouping under their original tier. */
function tierFor(modelId: string): LlmTier | null {
    // Strip ::high / ::medium suffix the way ai.rs::resolve_gemini_model does.
    const base = modelId.split('::')[0];
    // Match against the catalog. Note FAST and VISION share a model id
    // (Gemini multimodal is unified) — we attribute to FAST since that's
    // the canonical entry for `gemini-3.5-flash`.
    if (base === LLM.FAST.split('::')[0])      return 'FAST';
    if (base === LLM.CHEAP.split('::')[0])     return 'CHEAP';
    if (base === LLM.REASONING.split('::')[0]) return 'REASONING';
    if (base === LLM.LEGACY)                   return 'FAST';   // legacy alias rolls up to FAST
    // Older / one-off / Claude / Ollama ids: not in this aggregation.
    // Callers can show them in an "Other" bucket if they want.
    return null;
}

/** Group a flat per-model breakdown into the 4 tier buckets. Models
 *  that don't map to a known tier (Claude, Ollama, deprecated Gemini
 *  ids) are returned as a flat list in `unattributed` so the dashboard
 *  can show them separately rather than dropping them. */
export function costByTier(rows: ModelCostRow[]): {
    tiers: TierCostRow[];
    unattributed: ModelCostRow[];
    total_cost: number;
} {
    const buckets: Record<LlmTier, TierCostRow> = {
        FAST:      { tier: 'FAST',      cost: 0, tokens: 0, requests: 0, models: [] },
        CHEAP:     { tier: 'CHEAP',     cost: 0, tokens: 0, requests: 0, models: [] },
        REASONING: { tier: 'REASONING', cost: 0, tokens: 0, requests: 0, models: [] },
        VISION:    { tier: 'VISION',    cost: 0, tokens: 0, requests: 0, models: [] },
        LEGACY:    { tier: 'LEGACY',    cost: 0, tokens: 0, requests: 0, models: [] },
    };
    const unattributed: ModelCostRow[] = [];
    let total_cost = 0;

    for (const r of rows) {
        total_cost += r.cost;
        const t = tierFor(r.model);
        if (t === null) { unattributed.push(r); continue; }
        const b = buckets[t];
        b.cost     += r.cost;
        b.tokens   += r.tokens;
        b.requests += r.requests;
        if (!b.models.includes(r.model)) b.models.push(r.model);
    }

    // Drop empty tiers and sort by cost descending so the dashboard
    // shows the spendy ones first.
    const tiers = (Object.values(buckets) as TierCostRow[])
        .filter(b => b.cost > 0 || b.tokens > 0)
        .sort((a, b) => b.cost - a.cost);

    return { tiers, unattributed, total_cost };
}

/** Format a tier cost row for display: `$1.43 · 18.4k tok · 34 req`. */
export function formatTierCost(t: TierCostRow): string {
    const dollars = t.cost < 0.01 && t.cost > 0 ? '<$0.01' : `$${t.cost.toFixed(2)}`;
    const ktok    = t.tokens >= 1000 ? `${(t.tokens / 1000).toFixed(1)}k` : String(t.tokens);
    return `${dollars} · ${ktok} tok · ${t.requests} req`;
}
