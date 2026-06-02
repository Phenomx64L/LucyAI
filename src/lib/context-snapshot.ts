// ── context-snapshot.ts — Live snapshot of Lucy's context per turn (v1.7.22) ─
//
// The ContextStrip needs a single reactive source for "what's in Lucy's
// LLM context RIGHT NOW". The data is scattered across:
//
//   • peekActiveSecuritySkill()  → security-skill-bridge
//   • peekActivePreset()         → skill-preset-store
//   • tab._unifiedPlan?.mcp_tools → unified-context per-tab cache
//   • the count of memory rows injected by construirContextoMemoria
//   • tab.tokens                  → running token estimate
//
// `+page.svelte` already gathers all of those when it builds a prompt.
// This module is just a writable store that the chat code updates at
// the end of each prompt-build pass; the ContextStrip subscribes to it
// and renders the chips.
//
// Why not derive everything from peek* functions on each render? Two
// reasons:
//   1. The memory-injection count isn't stored anywhere reactive — it
//      lives inside construirContextoMemoria as a local. Pushing it
//      into a store is cheaper than refactoring that function.
//   2. The tab._unifiedPlan is updated mid-stream and we want the chip
//      to settle on the FINAL plan, not the in-flight one.

import { writable, type Writable } from 'svelte/store';

export interface ContextSnapshot {
    /** How many agent_memories were injected on the most recent turn. */
    memoriesCount:    number;
    /** Active security skill id ("conducting-phishing-incident-response")
     *  or null when no skill is active. */
    skillId:          string | null;
    /** Source: 'auto' (auto-routed), 'manual' (/sec-skill use), or null. */
    skillSource:      'auto' | 'manual' | null;
    /** Active ECC preset id, or null when no preset. */
    presetId:         string | null;
    /** Top-N MCP tools that were ranked into context. */
    mcpToolsCount:    number;
    /** Estimated tokens used on the most recent turn. */
    estTokens:        number;
    /** Maximum context tokens for the active model. */
    maxTokens:        number;
    /** Model id we're sending the prompt to. */
    modelId:          string | null;
    /** When this snapshot was captured (ms since epoch). */
    capturedAt:       number;
}

const empty: ContextSnapshot = {
    memoriesCount:    0,
    skillId:          null,
    skillSource:      null,
    presetId:         null,
    mcpToolsCount:    0,
    estTokens:        0,
    maxTokens:        0,
    modelId:          null,
    capturedAt:       0,
};

export const contextSnapshot: Writable<ContextSnapshot> = writable({ ...empty });

/** Update the snapshot. Call from `+page.svelte` right after building
 *  the prompt so the strip reflects what actually went to the LLM.
 *  Partial — pass only the fields you have, the rest stay as-is. */
export function setContextSnapshot(patch: Partial<ContextSnapshot>): void {
    contextSnapshot.update(s => ({ ...s, ...patch, capturedAt: Date.now() }));
}

export function clearContextSnapshot(): void {
    contextSnapshot.set({ ...empty });
}
