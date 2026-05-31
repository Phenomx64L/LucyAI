// ── memory-grounding.ts (v1.6.0) ──────────────────────────────────────────
//
// Thin wrappers around the Rust commands defined in
// src-tauri/src/commands/grounding.rs.
//
// See docs/research/kappa-graph/adrs/ADR-044-probabilistic-truth-convergence.md
// for the design rationale behind the query-time computation and the
// 0.20 default threshold.
//
// Why a thin wrapper file (not just inline `invoke` calls): centralised
// types + a single place to add the threshold-filter helper as the
// in-prompt injection pipeline starts using it.

import { invoke } from '@tauri-apps/api/core';

export type MemoryKind = 'agent' | 'core';
export type EvidenceKind = 'support' | 'contradict';
export type SourceKind   = 'chat' | 'doc' | 'shell' | 'pdf' | 'web' | 'unknown';

export interface GroundingScore {
    strength:          number;  // [0.0, 1.0]
    support_weight:    number;
    contradict_weight: number;
    support_count:     number;
    contradict_count:  number;
    /** true when strength came from the prior confidence column,
     *  not from observed evidence. */
    from_prior:        boolean;
}

export interface MemoryInstance {
    id:           number;
    memory_kind:  MemoryKind;
    quote_text:   string;
    source_kind:  SourceKind | string;
    source_ref:   string;
    offset_start: number;
    offset_end:   number;
    created_at:   number;
}

export interface InstanceHit {
    instance:  MemoryInstance;
    memory_id: string;
}

// ── Default grounding threshold ────────────────────────────────────────────
// The ADR-044 default is 0.20 — memories below this strength are not
// injected into the system prompt by default. Surfaced as a setting in
// MemoryBrowserView so the user can dial confidence up or down.
export const GROUNDING_DEFAULT_THRESHOLD = 0.20;

// ── Tauri wrappers ─────────────────────────────────────────────────────────

export function getGroundingScore(
    memoryKind: MemoryKind,
    memoryId: string,
): Promise<GroundingScore> {
    return invoke<GroundingScore>('memory_grounding', {
        memoryKind, memoryId,
    });
}

export function logEvidence(
    memoryKind: MemoryKind,
    memoryId: string,
    kind: EvidenceKind,
    weight = 1.0,
    sourceRef = '',
): Promise<number> {
    return invoke<number>('memory_evidence_log', {
        event: {
            memory_kind: memoryKind,
            memory_id: memoryId,
            kind, weight,
            source_ref: sourceRef,
        },
    });
}

export function saveInstance(input: {
    memoryKind: MemoryKind;
    memoryId: string;
    quoteText: string;
    sourceKind?: SourceKind | string;
    sourceRef?: string;
    offsetStart?: number;
    offsetEnd?: number;
}): Promise<number> {
    return invoke<number>('memory_instance_save', {
        inst: {
            memory_kind:  input.memoryKind,
            memory_id:    input.memoryId,
            quote_text:   input.quoteText,
            source_kind:  input.sourceKind ?? 'chat',
            source_ref:   input.sourceRef ?? '',
            offset_start: input.offsetStart ?? 0,
            offset_end:   input.offsetEnd ?? 0,
        },
    });
}

export function getInstancesFor(
    memoryKind: MemoryKind,
    memoryId: string,
    limit = 20,
): Promise<MemoryInstance[]> {
    return invoke<MemoryInstance[]>('memory_instances_for', {
        memoryKind, memoryId, limit,
    });
}

export function searchInstances(
    query: string,
    limit = 10,
): Promise<InstanceHit[]> {
    return invoke<InstanceHit[]>('memory_instances_search', { query, limit });
}

// ── Helpers ────────────────────────────────────────────────────────────────

/** Render a grounding strength as "87%" for the UI. */
export function fmtStrengthPct(g: GroundingScore): string {
    return `${Math.round(g.strength * 100)}%`;
}

/** Pick a tone bucket for visual rendering. Three bands mirror the
 *  ADR-044 example (filter at 0.20, comfortable >0.55, strong >0.80). */
export function strengthTone(g: GroundingScore): 'crit' | 'warn' | 'ok' | 'info' {
    if (g.from_prior)        return 'info';  // no observed evidence
    if (g.strength < 0.20)   return 'crit';
    if (g.strength < 0.55)   return 'warn';
    return 'ok';
}
