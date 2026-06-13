// ── unified-context.ts — Unified per-turn context orchestrator (v1.7.5) ──
//
// Lucy historically built her prompt context by concatenating three
// independent paths:
//
//   - Memory injection from `construirContextoMemoria` (+page.svelte)
//   - Active skill preset / security skill (v1.6.1 / v1.7.4)
//   - MCP server tool catalog (rendered separately in ai.rs)
//
// Each path was budget-blind: a 600-row memory result and an 8 KB
// skill body could both fire on the same turn, blowing the context
// window with no coordination. This module is the new single
// orchestrator that pulls the three signal sources together for each
// turn, ranks them by relevance, and applies a global token budget.
//
// ── Pipeline ────────────────────────────────────────────────────────────
//
//   buildUnifiedContext(userPrompt, opts) -> UnifiedContext {
//     1. Auto-route a security skill (Tier 1+2+3 hybrid via
//        security_skills_auto_route + LLM disambiguation when needed).
//     2. Score the active MCP servers' tool catalog against the prompt.
//        Surface up to N most-relevant tools (compact form).
//     3. The caller already pulls top memory hits — we just expose a
//        helper so memory + skill + MCP all live in one struct that
//        chip rendering can consume.
//     4. Return a render plan with a tier breakdown the chat UI shows.
//   }
//
// ── Budgeting ───────────────────────────────────────────────────────────
//
//   skill body          → up to 8000 chars (already capped in v1.7.4)
//   MCP tool list       → up to 3000 chars (compact one-line summaries)
//   memory hits         → caller-managed (existing construirContextoMemoria)
//
// Total target injection: 4-6k tokens (~16-24 KB) on top of the user's
// message and persistent system prompt. Leaves comfortable room for
// long agent turns.

import { invoke } from '@tauri-apps/api/core';
import { LLM } from '$lib/llm-models';
import { resolveTierWithBreaker } from '$lib/tier-health';
import { peekActiveSecuritySkill, type SecuritySkillFull } from '$lib/security-skill-bridge';
import { peekActivePreset } from '$lib/skill-preset-store';
import { safeGetLS, safeSetLSString } from '$lib/safe-ls';

// ── Settings ────────────────────────────────────────────────────────────

const LS_KEY_AUTOROUTE   = 'lucy_skill_autoroute_enabled';
const LS_KEY_LLM_DISAMB  = 'lucy_skill_autoroute_llm_disamb';
const LS_KEY_LAST_ROUTE  = 'lucy_skill_last_autoroute_v1';

/** Auto-route enabled by default. User can disable via /sec-skill auto off
 *  if they want fully manual control. */
export function isAutoRouteEnabled(): boolean {
    const v = safeGetLS(LS_KEY_AUTOROUTE, '');
    return v !== 'off';   // unset = enabled (default-on)
}

export function setAutoRouteEnabled(on: boolean): void {
    safeSetLSString(LS_KEY_AUTOROUTE, on ? 'on' : 'off');
}

/** Tier 3 LLM disambiguation costs ~$0.0001 per ambiguous turn.
 *  Default on — the cost is negligible and the quality gain is real. */
export function isLlmDisambEnabled(): boolean {
    const v = safeGetLS(LS_KEY_LLM_DISAMB, '');
    return v !== 'off';
}

export function setLlmDisambEnabled(on: boolean): void {
    safeSetLSString(LS_KEY_LLM_DISAMB, on ? 'on' : 'off');
}

// ── Types ───────────────────────────────────────────────────────────────

export interface UnifiedRouteResult {
    /** Which tier won, in order of cost: keyword < embedding < llm < manual */
    method:      'keyword' | 'embedding' | 'fused' | 'llm' | 'manual' | 'preset' | 'none';
    /** Active skill if one was selected this turn. */
    skill:       SecuritySkillFull | null;
    /** Confidence score in [0, 1]. Keyword scores are normalized by 100. */
    score:       number;
    /** Top-N candidates the router considered. Useful for the chip
     *  tooltip when method='llm' or 'ambiguous'. */
    candidates:  Array<{ id: string; name: string; score: number }>;
    /** Diagnostic — was Ollama reachable? Drives the "fallback" badge. */
    embeddings_available: boolean;
    /** Wall time of the routing decision (ms). Surfaces in the chip
     *  tooltip when the user hovers. */
    elapsed_ms:  number;
}

export interface McpToolHit {
    server:    string;
    tool:      string;
    summary:   string;
    score:     number;
}

export interface UnifiedContextPlan {
    route:      UnifiedRouteResult;
    /** Top MCP tools across all registered servers, ranked against
     *  the prompt. Empty when no servers are registered or when the
     *  user disabled MCP injection. */
    mcp_tools:  McpToolHit[];
    /** Estimated tokens this plan will inject. Rough — assumes
     *  4 chars / token. */
    est_tokens: number;
}

// ── Auto-route ──────────────────────────────────────────────────────────

/** Backend AutoRouteResult mirrored. */
interface BackendAutoRoute {
    method: string;
    top: { meta: any; score: number; preview: string } | null;
    candidates: Array<{ meta: any; score: number; preview: string }>;
    embeddings_available: boolean;
}

/**
 * Full hybrid auto-router. Tiers 1+2 happen in Rust; tier 3 (LLM
 * disambiguation) happens here because it spends an `ask_lucy` call
 * and the frontend already coordinates LLM budget.
 */
export async function autoRouteSkill(userPrompt: string): Promise<UnifiedRouteResult> {
    const t0 = performance.now();
    const empty: UnifiedRouteResult = {
        method: 'none', skill: null, score: 0,
        candidates: [], embeddings_available: false,
        elapsed_ms: 0,
    };

    // Respect the user's manual choice if a skill or preset is already active.
    const manualSkill = peekActiveSecuritySkill();
    if (manualSkill) {
        return { ...empty, method: 'manual', skill: manualSkill, score: 1.0,
                 elapsed_ms: performance.now() - t0 };
    }
    if (peekActivePreset()) {
        return { ...empty, method: 'preset', score: 1.0,
                 elapsed_ms: performance.now() - t0 };
    }
    if (!isAutoRouteEnabled()) {
        return { ...empty, elapsed_ms: performance.now() - t0 };
    }
    if (!userPrompt || userPrompt.trim().length < 8) {
        // Too short to route reliably.
        return { ...empty, elapsed_ms: performance.now() - t0 };
    }

    let raw: BackendAutoRoute;
    try {
        raw = await invoke<BackendAutoRoute>('security_skills_auto_route',
                                              { userPrompt: userPrompt.trim() });
    } catch (e) {
        console.warn('[unified-context] auto-route failed:', e);
        return { ...empty, elapsed_ms: performance.now() - t0 };
    }

    const candidates = raw.candidates.map(c => ({
        id: c.meta.id, name: c.meta.name, score: c.score,
    }));

    // Tier 1, 2, or 2.5 (v1.7.88 RRF-fused) succeeded.
    if (raw.method === 'keyword' || raw.method === 'embedding' || raw.method === 'fused') {
        if (!raw.top) return { ...empty, elapsed_ms: performance.now() - t0 };
        const full = await loadSkillBody(raw.top.meta.id);
        // v1.7.88 — keyword keeps its 0..100 → 0..1 normalization; embedding
        // is already a cosine in 0..1 * 100; fused inherits whatever the
        // backend assigned to top (typically the embedding cosine).
        const score = raw.method === 'keyword' ? Math.min(1.0, raw.top.score / 100) : raw.top.score / 100;
        return {
            method: raw.method as any,
            skill: full,
            score,
            candidates,
            embeddings_available: raw.embeddings_available,
            elapsed_ms: performance.now() - t0,
        };
    }

    // Tier 3 — ambiguous zone. Only invoke LLM if the user opted in.
    if (raw.method === 'ambiguous' && raw.candidates.length >= 2 && isLlmDisambEnabled()) {
        const picked = await llmDisambiguate(userPrompt, raw.candidates);
        if (picked) {
            const meta = raw.candidates.find(c => c.meta.id === picked)?.meta;
            if (meta) {
                const full = await loadSkillBody(meta.id);
                const score = raw.candidates[0].score / 100;
                return {
                    method: 'llm',
                    skill: full,
                    score,
                    candidates,
                    embeddings_available: raw.embeddings_available,
                    elapsed_ms: performance.now() - t0,
                };
            }
        }
    }

    return { ...empty, candidates, embeddings_available: raw.embeddings_available,
             elapsed_ms: performance.now() - t0 };
}

async function loadSkillBody(id: string): Promise<SecuritySkillFull | null> {
    try {
        return await invoke<SecuritySkillFull>('security_skills_get', { id });
    } catch (e) {
        console.warn('[unified-context] get skill failed:', e);
        return null;
    }
}

/**
 * Tier 3 — ask CHEAP tier to pick the most relevant skill from the
 * ambiguous candidates. Returns the picked id or null.
 *
 * Prompt is deliberately minimal: id list + descriptions, ask for id.
 * We bound `maxTokensOverride: 32` so even a verbose model can't make
 * this call expensive.
 */
async function llmDisambiguate(
    userPrompt: string,
    candidates: BackendAutoRoute['candidates'],
): Promise<string | null> {
    const list = candidates.slice(0, 5).map((c, i) =>
        `${i + 1}. ${c.meta.id}: ${c.meta.description.slice(0, 140)}`
    ).join('\n');
    const prompt =
        `User just said: "${userPrompt.slice(0, 400)}"\n\n` +
        `Which ONE of these cybersecurity skills is most relevant? ` +
        `Respond ONLY with the exact skill id (the part before the colon), or "none".\n\n` +
        list;
    try {
        const model = resolveTierWithBreaker(LLM.CHEAP);
        const reply = await invoke<string>('ask_lucy', {
            prompt, context: '',
            userName: 'lucy-auto-route',
            runbooksDir: null,
            model,
            images: null,
            lang: 'en',
            hostsJson: null,
            maxTokensOverride: 32,
        });
        const raw = String(reply || '').trim().toLowerCase();
        if (!raw || raw === 'none') return null;
        // Match against candidate ids (case-insensitive, allow noise).
        for (const c of candidates) {
            if (raw.includes(c.meta.id.toLowerCase())) return c.meta.id;
        }
        return null;
    } catch (e) {
        console.warn('[unified-context] LLM disambiguation failed:', e);
        return null;
    }
}

// ── MCP tool ranking ────────────────────────────────────────────────────

/**
 * Rank registered MCP server tools against the prompt. Returns the
 * top-N most-relevant. Cheap — keyword overlap, no embeddings.
 */
export function rankMcpTools(
    userPrompt: string,
    servers: Array<{ name: string; tools_cache?: any[]; enabled?: boolean }>,
    limit = 8,
): McpToolHit[] {
    const tokens = new Set(
        userPrompt.toLowerCase()
            .split(/[^a-z0-9]+/)
            .filter(w => w.length >= 3)
    );
    if (tokens.size === 0) return [];
    const hits: McpToolHit[] = [];
    for (const s of servers) {
        if (s.enabled === false) continue;
        const tools = Array.isArray(s.tools_cache) ? s.tools_cache : [];
        for (const t of tools) {
            const name = String(t?.name || '');
            const desc = String(t?.description || '');
            if (!name) continue;
            const blob = `${name} ${desc}`.toLowerCase();
            let score = 0;
            for (const tok of tokens) {
                if (blob.includes(tok)) score += 1;
            }
            if (score === 0) continue;
            hits.push({
                server: s.name,
                tool: name,
                summary: desc.slice(0, 120),
                score,
            });
        }
    }
    hits.sort((a, b) => b.score - a.score);
    return hits.slice(0, limit);
}

/**
 * Render MCP tool hits as a compact prompt block. Bounded at 3000
 * chars so this never crowds the skill body or memory budget.
 */
export function renderMcpToolsBlock(hits: McpToolHit[]): string {
    if (hits.length === 0) return '';
    const lines = hits.map(h =>
        `  • ${h.server}.${h.tool}: ${h.summary}`
    ).join('\n');
    const block = `\n\n--- AVAILABLE MCP TOOLS (ranked by relevance to this turn) ---\n${lines}\n`;
    return block.length > 3000 ? block.slice(0, 3000) + '\n  …(truncated)' : block;
}

// ── Orchestrator ────────────────────────────────────────────────────────

/**
 * Single entry point. The caller (+page.svelte) invokes this once per
 * turn, gets back a unified plan, and:
 *   1. If `route.skill` is set, that skill body is prepended to the
 *      system prompt (via security-skill-bridge — auto-route activates
 *      the skill so the existing injection point picks it up).
 *   2. If `mcp_tools` is non-empty, the block is appended to the
 *      memory context.
 *   3. The chip is rendered above the user message.
 */
export async function buildUnifiedContext(
    userPrompt: string,
    mcpServers: Array<{ name: string; tools_cache?: any[]; enabled?: boolean }>,
): Promise<UnifiedContextPlan> {
    const [route, mcp] = await Promise.all([
        autoRouteSkill(userPrompt),
        Promise.resolve(rankMcpTools(userPrompt, mcpServers, 8)),
    ]);

    // v1.7.153 — Auto-ACTIVATION of the routed security skill is DISABLED.
    // Persisting an auto-routed skill silently flipped Lucy into
    // "explain, don't execute" mode for ALL subsequent turns:
    // renderSecuritySkillForPrompt() injects a "DEFAULT MODE = EXPLAIN, NOT
    // EXECUTE" framing AND +page.svelte's `skillInfoIntent` downgrades every
    // <EXECUTE> to a non-running code fence. A SysAdmin asking "verifica
    // updates en mi servidor" matched a patch/vuln skill and lost command
    // execution entirely — with a stale skill stuck in localStorage that even
    // survived /preset clear (this function re-activated it each turn).
    // Security skills now activate ONLY via an explicit `/sec-skill use <id>`.
    // `route` is still returned below for the chip, /route-status and the
    // token estimate.
    //   (was: if (keyword|embedding|llm && route.skill) setSecuritySkillAsPreset(route.skill); )

    const est_tokens = Math.ceil(
        ((route.skill?.body.length || 0) + (mcp.length * 200)) / 4
    );

    // Persist the last route for the chip and `/route-status` slash cmd.
    try {
        safeSetLSString(LS_KEY_LAST_ROUTE, JSON.stringify({
            method: route.method,
            skill_id: route.skill?.meta.id || null,
            score: route.score,
            elapsed_ms: route.elapsed_ms,
            ts: Date.now(),
        }));
    } catch { /* quota */ }

    return { route, mcp_tools: mcp, est_tokens };
}

export function peekLastRoute(): { method: string; skill_id: string | null; score: number; elapsed_ms: number; ts: number } | null {
    const raw = safeGetLS(LS_KEY_LAST_ROUTE, '');
    if (!raw) return null;
    try { return JSON.parse(raw); } catch { return null; }
}
