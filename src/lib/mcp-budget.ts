// ── mcp-budget.ts (v1.6.2) ────────────────────────────────────────────────
//
// MCP budget guard inspired by the ECC `mcp-budget` skill:
//   - hard cap of ~10 active servers per project
//   - hard cap of ~80 active tools across all enabled servers
//   - a 200k context window shrinks to ~70k usable once tool descriptions
//     consume budget, so we account for that bite explicitly
//
// The original ECC skill was a behavioural prompt for Claude Code; this
// module implements the SAME thresholds as a runtime guard that watches
// Lucy's `McpServer[]` and surfaces an `McpBudgetState` the UI renders.
//
// Token estimation: rough but useful. We estimate ~4 chars per token
// across the JSON-encoded tool description + parameters schema. Anthropic's
// own tool definitions average ~3.6 chars/token, Gemini ~4.2 — splitting
// the difference at 4 keeps the math fast and the conservative direction
// (slight overestimate → user gets warned earlier, which is what they
// asked for).

export interface McpServerLite {
    /** Unique id of the server (matches the Rust McpServer.id). */
    id: string;
    name: string;
    enabled: boolean;
    /** Raw JSON array of tool definitions from tools/list. */
    tools_cache: unknown;
}

export type BudgetTone = 'ok' | 'warn' | 'crit';

export interface McpBudgetState {
    /** Count of `enabled === true` servers. */
    enabledServers: number;
    /** Sum of tool count across enabled servers. */
    enabledTools:   number;
    /** Rough token-count estimate of all enabled tool descriptions. */
    estimatedTokens: number;
    /** Worst tier across the three axes. */
    tone: BudgetTone;
    /** Per-axis tone for UI breakdown. */
    serverTone: BudgetTone;
    toolTone:   BudgetTone;
    tokenTone:  BudgetTone;
    /** Human-readable explanation of why the tone is what it is. */
    reason: string;
}

// ── Thresholds (mirrors ECC `mcp-budget` recommendations) ──────────────────
//
// Servers:  warn at 8, crit at 10 — beyond 10 the ECC skill says "config
//           review needed" and recommends disabling something first.
// Tools:    warn at 60, crit at 80. Past 80 the context cost outpaces
//           usefulness for almost any task.
// Tokens:   warn at 40k, crit at 60k. A 200k window with 60k spent on
//           tool defs leaves you ~140k for actual conversation — close to
//           the "usable shrinks to ~70k" pain point the ECC docs cite.
export const BUDGET_SERVERS_WARN = 8;
export const BUDGET_SERVERS_CRIT = 10;
export const BUDGET_TOOLS_WARN   = 60;
export const BUDGET_TOOLS_CRIT   = 80;
export const BUDGET_TOKENS_WARN  = 40_000;
export const BUDGET_TOKENS_CRIT  = 60_000;

// ── Estimation ─────────────────────────────────────────────────────────────

const CHARS_PER_TOKEN = 4;

/** Coerce an `unknown` `tools_cache` into a usable array length + token
 *  estimate. Defensive: handles missing / malformed cache silently. */
function inspectToolsCache(cache: unknown): { count: number; estTokens: number } {
    if (!Array.isArray(cache)) return { count: 0, estTokens: 0 };
    const count = cache.length;
    // Estimate via JSON length of the whole array — captures tool name,
    // description, and parameter schema in a single measurement.
    let chars = 0;
    try { chars = JSON.stringify(cache).length; } catch { chars = count * 200; }
    const estTokens = Math.ceil(chars / CHARS_PER_TOKEN);
    return { count, estTokens };
}

function pickTone(
    val: number, warn: number, crit: number,
): BudgetTone {
    if (val >= crit) return 'crit';
    if (val >= warn) return 'warn';
    return 'ok';
}

function worst(...tones: BudgetTone[]): BudgetTone {
    if (tones.includes('crit')) return 'crit';
    if (tones.includes('warn')) return 'warn';
    return 'ok';
}

// ── Main API ───────────────────────────────────────────────────────────────

export function computeBudget(servers: McpServerLite[]): McpBudgetState {
    const enabled = servers.filter(s => s.enabled);
    const enabledServers = enabled.length;

    let enabledTools = 0;
    let estimatedTokens = 0;
    for (const s of enabled) {
        const { count, estTokens } = inspectToolsCache(s.tools_cache);
        enabledTools    += count;
        estimatedTokens += estTokens;
    }

    const serverTone = pickTone(enabledServers, BUDGET_SERVERS_WARN, BUDGET_SERVERS_CRIT);
    const toolTone   = pickTone(enabledTools,   BUDGET_TOOLS_WARN,   BUDGET_TOOLS_CRIT);
    const tokenTone  = pickTone(estimatedTokens, BUDGET_TOKENS_WARN, BUDGET_TOKENS_CRIT);

    const tone = worst(serverTone, toolTone, tokenTone);
    const reason = explainTone(tone, enabledServers, enabledTools, estimatedTokens);

    return {
        enabledServers, enabledTools, estimatedTokens,
        tone, serverTone, toolTone, tokenTone, reason,
    };
}

function explainTone(
    tone: BudgetTone,
    s: number, t: number, k: number,
): string {
    if (tone === 'ok') {
        return `${s} servers · ${t} tools · ~${(k / 1000).toFixed(1)}k tokens — within budget`;
    }
    const parts: string[] = [];
    if (s >= BUDGET_SERVERS_CRIT) parts.push(`${s} servers (max ${BUDGET_SERVERS_CRIT})`);
    else if (s >= BUDGET_SERVERS_WARN) parts.push(`${s} servers (warn at ${BUDGET_SERVERS_WARN})`);
    if (t >= BUDGET_TOOLS_CRIT) parts.push(`${t} tools (max ${BUDGET_TOOLS_CRIT})`);
    else if (t >= BUDGET_TOOLS_WARN) parts.push(`${t} tools (warn at ${BUDGET_TOOLS_WARN})`);
    if (k >= BUDGET_TOKENS_CRIT) parts.push(`~${(k / 1000).toFixed(1)}k tool tokens (crit at ${BUDGET_TOKENS_CRIT / 1000}k)`);
    else if (k >= BUDGET_TOKENS_WARN) parts.push(`~${(k / 1000).toFixed(1)}k tool tokens (warn at ${BUDGET_TOKENS_WARN / 1000}k)`);
    return parts.join(' · ');
}

/** Synchronous helper: should the UI block the user from enabling
 *  another server? `true` when the next server WOULD push them over
 *  a critical limit. */
export function wouldExceedCritical(
    current: McpBudgetState,
    nextToolCount = 5,           // optimistic default if unknown
    nextEstTokens = 5 * 200 / CHARS_PER_TOKEN,
): boolean {
    return (current.enabledServers + 1) >= BUDGET_SERVERS_CRIT
        || (current.enabledTools + nextToolCount) >= BUDGET_TOOLS_CRIT
        || (current.estimatedTokens + nextEstTokens) >= BUDGET_TOKENS_CRIT;
}
