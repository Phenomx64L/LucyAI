// ── mcp-budget.test.ts (v1.6.2) ───────────────────────────────────────────
//
// Unit tests for the MCP budget calculator.

import { describe, it, expect } from 'vitest';
import {
    computeBudget, wouldExceedCritical,
    BUDGET_SERVERS_CRIT, BUDGET_TOOLS_CRIT, BUDGET_TOKENS_CRIT,
    BUDGET_SERVERS_WARN, BUDGET_TOOLS_WARN,
    type McpServerLite,
} from './mcp-budget';

function mockTool(name = 't', descChars = 200) {
    return { name, description: 'x'.repeat(descChars), inputSchema: { type: 'object' } };
}

function mockServer(id: string, enabled: boolean, toolCount: number): McpServerLite {
    return {
        id, name: id, enabled,
        tools_cache: Array.from({ length: toolCount }, (_, i) => mockTool(`${id}-${i}`)),
    };
}

describe('mcp-budget — computeBudget', () => {
    it('returns all-zeros + ok for empty input', () => {
        const b = computeBudget([]);
        expect(b.enabledServers).toBe(0);
        expect(b.enabledTools).toBe(0);
        expect(b.estimatedTokens).toBe(0);
        expect(b.tone).toBe('ok');
    });

    it('ignores disabled servers', () => {
        const b = computeBudget([
            mockServer('a', false, 50),
            mockServer('b', false, 50),
        ]);
        expect(b.enabledServers).toBe(0);
        expect(b.enabledTools).toBe(0);
    });

    it('counts enabled servers and tools', () => {
        const b = computeBudget([
            mockServer('a', true, 5),
            mockServer('b', true, 3),
            mockServer('c', false, 100),
        ]);
        expect(b.enabledServers).toBe(2);
        expect(b.enabledTools).toBe(8);
    });

    it('warns at server warning threshold', () => {
        const servers = Array.from({ length: BUDGET_SERVERS_WARN }, (_, i) =>
            mockServer(`s${i}`, true, 1));
        const b = computeBudget(servers);
        expect(b.serverTone).toBe('warn');
        expect(b.tone).not.toBe('ok');
    });

    it('crits at server critical threshold', () => {
        const servers = Array.from({ length: BUDGET_SERVERS_CRIT }, (_, i) =>
            mockServer(`s${i}`, true, 1));
        const b = computeBudget(servers);
        expect(b.serverTone).toBe('crit');
        expect(b.tone).toBe('crit');
    });

    it('escalates tone to worst-of when multiple axes degrade', () => {
        // Just under server-warn, exactly at tool-crit.
        const s = mockServer('huge', true, BUDGET_TOOLS_CRIT);
        const b = computeBudget([s]);
        expect(b.serverTone).toBe('ok');
        expect(b.toolTone).toBe('crit');
        expect(b.tone).toBe('crit');
    });

    it('estimates tokens roughly proportional to tool count', () => {
        const small = computeBudget([mockServer('s', true, 1)]);
        const big   = computeBudget([mockServer('s', true, 10)]);
        expect(big.estimatedTokens).toBeGreaterThan(small.estimatedTokens * 5);
    });

    it('reason string mentions the offending axis at crit', () => {
        const s = mockServer('huge', true, BUDGET_TOOLS_CRIT);
        const b = computeBudget([s]);
        expect(b.reason).toMatch(/tools/);
    });
});

describe('mcp-budget — wouldExceedCritical', () => {
    it('returns false when we have headroom', () => {
        const b = computeBudget([mockServer('a', true, 5)]);
        expect(wouldExceedCritical(b)).toBe(false);
    });

    it('returns true when adding another server hits server-crit', () => {
        const servers = Array.from(
            { length: BUDGET_SERVERS_CRIT - 1 },
            (_, i) => mockServer(`s${i}`, true, 1),
        );
        const b = computeBudget(servers);
        expect(wouldExceedCritical(b)).toBe(true);
    });

    it('returns true when tool count would cross the tool-crit', () => {
        const b = computeBudget([mockServer('huge', true, BUDGET_TOOLS_CRIT - 1)]);
        expect(wouldExceedCritical(b, 5)).toBe(true);
    });

    it('returns true when tokens would cross the token-crit', () => {
        // A single server with a tool whose description balloons the
        // token estimate over the critical threshold.
        const heavyTool = { name: 'heavy', description: 'x'.repeat(BUDGET_TOKENS_CRIT * 4) };
        const s: McpServerLite = { id: 's', name: 's', enabled: true, tools_cache: [heavyTool] };
        const b = computeBudget([s]);
        expect(b.tokenTone).toBe('crit');
        expect(wouldExceedCritical(b)).toBe(true);
    });
});
