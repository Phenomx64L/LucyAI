// ── reflection-gate.ts — Frontend integration for the Rust ReflectionGate ────
// Calls the Tauri command `reflect_on_response` after agent loop streaming
// completes but BEFORE the response is processed for execution.
// Returns a verdict: Pass, Warn, or Escalate with risk level + reasons.

import { invoke } from '@tauri-apps/api/core';

// ── Utilities ──────────────────────────────────────────────────────────────

/** Escape a string for safe use inside HTML attribute values (title="..."). */
function escapeHtmlAttr(s: string): string {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}

// ── Types (mirror Rust enums) ───────────────────────────────────────────────

export type RiskLevel = 'Medium' | 'High' | 'Critical';

export type ReflectionVerdict =
    | 'Pass'
    | { Warn: { reasons: string[] } }
    | { Escalate: { risk: RiskLevel; reasons: string[] } };

export interface WorkingMemoryInput {
    currentCwd: string | null;
    recentPaths: string[];
    lastOutputs: string[];
    lastCommands: string[];
}

// ── Main gate function ────────────────────���─────────────────────────────────

/**
 * Analyze an LLM response through the Rust ReflectionGate before processing.
 * Sub-millisecond latency — no LLM calls, pure regex + context comparison.
 *
 * @param response - The full LLM response text (including tags)
 * @param memory - Working memory snapshot for context-aware checks
 * @returns The verdict: Pass, Warn, or Escalate
 */
export async function reflectBeforeEmit(
    response: string,
    memory?: Partial<WorkingMemoryInput>
): Promise<ReflectionVerdict> {
    try {
        const verdict = await invoke<ReflectionVerdict>('reflect_on_response', {
            response,
            currentCwd: memory?.currentCwd ?? null,
            recentPaths: memory?.recentPaths ?? [],
            lastOutputs: memory?.lastOutputs ?? [],
            lastCommands: memory?.lastCommands ?? [],
        });
        return verdict;
    } catch (e) {
        // If the gate itself fails, default to Pass (fail-open to avoid blocking)
        console.warn('[reflection-gate] invoke failed, defaulting to Pass:', e);
        return 'Pass';
    }
}

// ── Verdict helpers ──────────────────���────────────────────���─────────────────

export function isPass(v: ReflectionVerdict): boolean {
    return v === 'Pass';
}

export function isWarn(v: ReflectionVerdict): v is { Warn: { reasons: string[] } } {
    return typeof v === 'object' && 'Warn' in v;
}

export function isEscalate(v: ReflectionVerdict): v is { Escalate: { risk: RiskLevel; reasons: string[] } } {
    return typeof v === 'object' && 'Escalate' in v;
}

export function getReasons(v: ReflectionVerdict): string[] {
    if (isWarn(v)) return v.Warn.reasons;
    if (isEscalate(v)) return v.Escalate.reasons;
    return [];
}

export function getRisk(v: ReflectionVerdict): RiskLevel | null {
    if (isEscalate(v)) return v.Escalate.risk;
    return null;
}

// ── HTML rendering for verdict badges ────────────────────────��──────────────

/**
 * Render a safety verdict as an inline HTML badge for the chat UI.
 * Returns empty string for Pass (no badge needed).
 */
export function renderVerdictBadge(v: ReflectionVerdict): string {
    if (isPass(v)) return '';

    if (isWarn(v)) {
        const reasons = v.Warn.reasons.map(r => escapeHtmlAttr(r)).join('; ');
        return `<span class="reflection-badge warn" title="${reasons}">⚠ Safety warning</span>`;
    }

    if (isEscalate(v)) {
        const risk = v.Escalate.risk;
        const reasons = v.Escalate.reasons.map(r => escapeHtmlAttr(r)).join('; ');
        const icon = risk === 'Critical' ? '🛑' : '⛔';
        return `<div class="reflection-escalate">
            <span class="reflection-badge escalate ${risk.toLowerCase()}" title="${reasons}">${icon} ${risk} risk — blocked</span>
            <div class="reflection-reasons">${v.Escalate.reasons.map(r => `<div>• ${r.replace(/</g, '&lt;')}</div>`).join('')}</div>
        </div>`;
    }

    return '';
}
