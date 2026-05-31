// ── verify.ts — Cross-verification of contradictory memories ─────────────
//
// When Lucy accumulates hundreds of memories across sessions, some will
// inevitably contradict each other:
//   • "Service X runs on port 8080" vs "Service X runs on port 3000"
//   • "Config is in /etc/app.conf" vs "Config moved to /opt/app/config.yaml"
//   • "Use npm install" vs "Project migrated to pnpm"
//
// This module detects potential contradictions by:
//   1. Grouping memories by overlapping tags/concepts
//   2. Comparing content within groups for semantic opposition signals
//   3. Flagging pairs with high contradiction probability
//
// The user can then resolve: keep newer, keep both, merge, or delete one.
// All detection is client-side (heuristic, no LLM needed).

import { invoke } from '@tauri-apps/api/core';

// ── Types ────────────────────────────────────────────────────────────────

export interface MemoryPair {
    older: VerifyMemory;
    newer: VerifyMemory;
}

export interface VerifyMemory {
    id: number;
    title: string;
    content: string;
    tags: string[];
    files: string[];
    importance: number;
    createdAt: number;
    sessionId: string;
}

export type ConflictSeverity = 'low' | 'medium' | 'high';

export interface Contradiction {
    id: string;
    pair: MemoryPair;
    severity: ConflictSeverity;
    reason: string;
    /** Specific conflicting snippets */
    snippetOlder: string;
    snippetNewer: string;
    /** Shared tags/concepts that caused the grouping */
    sharedContext: string[];
    detectedAt: number;
}

export type Resolution = 'keep_newer' | 'keep_older' | 'keep_both' | 'merge' | 'dismiss';

export interface VerifyReport {
    contradictions: Contradiction[];
    pairsChecked: number;
    memoriesScanned: number;
    scanDurationMs: number;
}

// ── Detection Engine ─────────────────────────────────────────────────────

/**
 * Scan recent memories for potential contradictions.
 * Runs ~100-300ms depending on corpus size — safe for UI interaction.
 */
export async function verifyMemories(): Promise<VerifyReport> {
    const t0 = Date.now();

    let raw: RawMemory[] = [];
    try {
        raw = await invoke('get_recent_memories', { limit: 50 });
    } catch { return emptyReport(t0); }

    const memories = raw.map(toVerifyMemory);
    const contradictions: Contradiction[] = [];
    let pairsChecked = 0;

    // Group by overlapping tags (at least 1 shared non-meta tag)
    const groups = groupByOverlap(memories);

    for (const group of groups) {
        // Compare all pairs within the group
        for (let i = 0; i < group.length; i++) {
            for (let j = i + 1; j < group.length; j++) {
                pairsChecked++;
                const [older, newer] = group[i].createdAt <= group[j].createdAt
                    ? [group[i], group[j]]
                    : [group[j], group[i]];

                const result = detectContradiction(older, newer);
                if (result) {
                    contradictions.push(result);
                }
            }
        }
    }

    // Sort by severity (high first), then by recency
    contradictions.sort((a, b) => {
        const sevOrder: Record<ConflictSeverity, number> = { high: 3, medium: 2, low: 1 };
        return sevOrder[b.severity] - sevOrder[a.severity]
            || b.detectedAt - a.detectedAt;
    });

    return {
        contradictions,
        pairsChecked,
        memoriesScanned: memories.length,
        scanDurationMs: Date.now() - t0,
    };
}

/**
 * Resolve a contradiction by applying the chosen resolution.
 *
 * v1.6.13: previously wrapped every branch in `try { ... } catch
 * { return false }` which silently swallowed Tauri rejections. When
 * supersede_memory or save_agent_memory failed (bad args, missing
 * id, DB lock), the UI saw "success" + an unchanged list and looked
 * like the buttons did nothing. Now errors propagate so the caller
 * can surface them on screen.
 *
 * For `merge`, we also supersede BOTH originals against the new
 * merged row — before the fix, the merged row was created but the
 * two originals stayed visible, so the next /verify scan kept
 * flagging the same pair.
 */
export async function resolveContradiction(
    contradiction: Contradiction,
    resolution: Resolution,
): Promise<boolean> {
    switch (resolution) {
        case 'keep_newer':
            // Supersede older memory.
            await invoke('supersede_memory', {
                oldId: contradiction.pair.older.id,
                newId: contradiction.pair.newer.id,
            });
            return true;

        case 'keep_older':
            // Supersede newer memory.
            await invoke('supersede_memory', {
                oldId: contradiction.pair.newer.id,
                newId: contradiction.pair.older.id,
            });
            return true;

        case 'keep_both':
            // No backend action — both stay. The caller is
            // responsible for hiding this conflict from the
            // visible list (loadVerify would just re-detect it).
            return true;

        case 'merge': {
            // Create merged memory, supersede BOTH originals
            // against it. v1.6.13: the supersede was missing
            // before the fix, so the conflict reappeared on the
            // next scan even after a successful merge.
            const merged = mergeMemories(contradiction.pair);
            const result = await invoke<{ id: number; action: string }>('save_agent_memory', {
                title: merged.title,
                content: merged.content,
                tags: JSON.stringify(merged.tags),
                files: JSON.stringify(merged.files),
                sessionId: null,
                importance: Math.max(
                    contradiction.pair.older.importance,
                    contradiction.pair.newer.importance,
                ),
                ttlDays: null,
            });
            const newId = result?.id;
            if (newId && newId > 0) {
                await invoke('supersede_memory', {
                    oldId: contradiction.pair.older.id,
                    newId,
                }).catch(() => { /* best effort */ });
                await invoke('supersede_memory', {
                    oldId: contradiction.pair.newer.id,
                    newId,
                }).catch(() => { /* best effort */ });
            }
            return true;
        }

        case 'dismiss':
            // Pure client-side. Caller hides the row.
            return true;
    }
}

// ── Contradiction Detection ──────────────────────────────────────────────

// Signal words that indicate version/change — when both memories share
// context but use these differently, it's likely a contradiction.
const VERSION_SIGNALS = /\b(port|version|path|config|file|moved|changed|migrated|updated|replaced|now|was|old|new|deprecated|removed|switched)\b/i;
const NEGATION_SIGNALS = /\b(not|no|don't|doesn't|never|removed|disabled|stopped|avoid|instead)\b/i;
const NUMBER_RX = /\b\d+(\.\d+)?\b/g;
const PATH_RX = /(?:\/[\w.-]+){2,}|(?:[A-Z]:\\[\w.-\\]+)/g;

function detectContradiction(older: VerifyMemory, newer: VerifyMemory): Contradiction | null {
    const sharedTags = older.tags.filter(t => newer.tags.includes(t) && !META_TAGS.has(t));
    if (sharedTags.length === 0) return null;

    const olderLc = older.content.toLowerCase();
    const newerLc = newer.content.toLowerCase();

    // Skip if content is very similar (not contradictory, just duplicate)
    if (textSimilarity(olderLc, newerLc) > 0.85) return null;

    let severity: ConflictSeverity = 'low';
    let reason = '';
    let snippetOlder = '';
    let snippetNewer = '';

    // 1. Number conflicts (ports, versions)
    const olderNums = extractNumberContexts(older.content);
    const newerNums = extractNumberContexts(newer.content);
    for (const [ctx, num] of olderNums) {
        for (const [ctx2, num2] of newerNums) {
            if (num !== num2 && contextOverlap(ctx, ctx2)) {
                severity = 'high';
                reason = `Conflicto numérico: ${num} vs ${num2} en contexto similar`;
                snippetOlder = ctx;
                snippetNewer = ctx2;
            }
        }
    }

    // 2. Path conflicts
    if (!reason) {
        const olderPaths = older.content.match(PATH_RX) || [];
        const newerPaths = newer.content.match(PATH_RX) || [];
        if (olderPaths.length > 0 && newerPaths.length > 0) {
            // Same filename, different directory
            for (const op of olderPaths) {
                const oName = op.split(/[/\\]/).pop()!;
                for (const np of newerPaths) {
                    const nName = np.split(/[/\\]/).pop()!;
                    if (oName === nName && op !== np) {
                        severity = 'medium';
                        reason = `Ruta diferente para "${oName}": ${op} vs ${np}`;
                        snippetOlder = op;
                        snippetNewer = np;
                    }
                }
            }
        }
    }

    // 3. Negation pattern (one affirms, other negates same concept)
    if (!reason) {
        if (NEGATION_SIGNALS.test(newerLc) && !NEGATION_SIGNALS.test(olderLc) && VERSION_SIGNALS.test(newerLc)) {
            severity = 'medium';
            reason = 'La memoria más reciente niega o modifica lo que afirma la anterior';
            snippetOlder = older.content.slice(0, 100);
            snippetNewer = newer.content.slice(0, 100);
        }
    }

    // 4. Version/migration signals with shared file context
    if (!reason) {
        const sharedFiles = older.files.filter(f => newer.files.includes(f));
        if (sharedFiles.length > 0 && VERSION_SIGNALS.test(newerLc)) {
            severity = 'low';
            reason = `Posible versión desactualizada para archivo(s): ${sharedFiles.slice(0, 3).join(', ')}`;
            snippetOlder = older.content.slice(0, 80);
            snippetNewer = newer.content.slice(0, 80);
        }
    }

    if (!reason) return null;

    return {
        id: `conflict-${older.id}-${newer.id}`,
        pair: { older, newer },
        severity,
        reason,
        snippetOlder,
        snippetNewer,
        sharedContext: sharedTags,
        detectedAt: Date.now(),
    };
}

// ── Grouping & Helpers ───────────────────────────────────────────────────

const META_TAGS = new Set(['crystal', 'lesson', 'crystallized', 'promoted', 'consolidated']);

function groupByOverlap(memories: VerifyMemory[]): VerifyMemory[][] {
    // Simple Union-Find on tag overlap
    const parent = memories.map((_, i) => i);
    const find = (x: number): number => parent[x] === x ? x : (parent[x] = find(parent[x]));
    const union = (a: number, b: number) => { parent[find(a)] = find(b); };

    for (let i = 0; i < memories.length; i++) {
        const tagsI = new Set(memories[i].tags.filter(t => !META_TAGS.has(t)));
        if (tagsI.size === 0) continue;
        for (let j = i + 1; j < memories.length; j++) {
            const tagsJ = memories[j].tags.filter(t => !META_TAGS.has(t));
            if (tagsJ.some(t => tagsI.has(t))) {
                union(i, j);
            }
        }
    }

    const groups = new Map<number, VerifyMemory[]>();
    for (let i = 0; i < memories.length; i++) {
        const root = find(i);
        const g = groups.get(root) || [];
        g.push(memories[i]);
        groups.set(root, g);
    }

    // Only return groups with 2+ members
    return [...groups.values()].filter(g => g.length >= 2);
}

function textSimilarity(a: string, b: string): number {
    // Jaccard similarity on word trigrams
    const triA = wordTrigrams(a);
    const triB = wordTrigrams(b);
    if (triA.size === 0 && triB.size === 0) return 1;
    if (triA.size === 0 || triB.size === 0) return 0;
    let intersection = 0;
    for (const t of triA) if (triB.has(t)) intersection++;
    return intersection / (triA.size + triB.size - intersection);
}

function wordTrigrams(text: string): Set<string> {
    const words = text.split(/\s+/).filter(w => w.length > 2);
    const set = new Set<string>();
    for (let i = 0; i <= words.length - 3; i++) {
        set.add(`${words[i]} ${words[i+1]} ${words[i+2]}`);
    }
    return set;
}

function extractNumberContexts(text: string): [string, string][] {
    const results: [string, string][] = [];
    let m: RegExpExecArray | null;
    const rx = new RegExp(NUMBER_RX.source, 'g');
    while ((m = rx.exec(text)) !== null) {
        // Grab ~40 chars of surrounding context
        const start = Math.max(0, m.index - 20);
        const end = Math.min(text.length, m.index + m[0].length + 20);
        results.push([text.slice(start, end).trim(), m[0]]);
    }
    return results;
}

function contextOverlap(ctx1: string, ctx2: string): boolean {
    const words1 = new Set(ctx1.toLowerCase().split(/\W+/).filter(w => w.length > 3));
    const words2 = new Set(ctx2.toLowerCase().split(/\W+/).filter(w => w.length > 3));
    let shared = 0;
    for (const w of words1) if (words2.has(w)) shared++;
    return shared >= 1;  // at least 1 significant shared word
}

function mergeMemories(pair: MemoryPair): { title: string; content: string; tags: string[]; files: string[] } {
    return {
        title: `[Merged] ${pair.newer.title}`,
        content: `[Versión anterior (${fmtDate(pair.older.createdAt)})]\n${pair.older.content}\n\n[Versión actual (${fmtDate(pair.newer.createdAt)})]\n${pair.newer.content}`,
        tags: [...new Set([...pair.older.tags, ...pair.newer.tags, 'merged'])],
        files: [...new Set([...pair.older.files, ...pair.newer.files])],
    };
}

function fmtDate(epoch: number): string {
    return new Date(epoch * 1000).toLocaleDateString();
}

interface RawMemory {
    id: number;
    session_id: string;
    title: string;
    content: string;
    tags: string;
    files: string;
    importance: number;
    created_at: number;
}

function toVerifyMemory(m: RawMemory): VerifyMemory {
    return {
        id: m.id,
        title: m.title,
        content: m.content,
        tags: safeParse(m.tags),
        files: safeParse(m.files),
        importance: m.importance,
        createdAt: m.created_at,
        sessionId: m.session_id,
    };
}

function safeParse(json: string): string[] {
    try {
        const arr = JSON.parse(json);
        return Array.isArray(arr) ? arr : [];
    } catch { return []; }
}

function emptyReport(t0: number): VerifyReport {
    return { contradictions: [], pairsChecked: 0, memoriesScanned: 0, scanDurationMs: Date.now() - t0 };
}

// ── Labels ───────────────────────────────────────────────────────────────

export function severityLabel(sev: ConflictSeverity, isEN = false): string {
    const labels: Record<ConflictSeverity, [string, string]> = {
        high:   ['Alta',   'High'],
        medium: ['Media',  'Medium'],
        low:    ['Baja',   'Low'],
    };
    return labels[sev]?.[isEN ? 1 : 0] || sev;
}

export function severityColor(sev: ConflictSeverity): string {
    return { high: '#f87171', medium: '#fbbf24', low: '#60a5fa' }[sev] || '#94a3b8';
}

export function resolutionLabel(res: Resolution, isEN = false): string {
    const labels: Record<Resolution, [string, string]> = {
        keep_newer: ['Mantener más reciente', 'Keep newer'],
        keep_older: ['Mantener más antigua',  'Keep older'],
        keep_both:  ['Mantener ambas',        'Keep both'],
        merge:      ['Fusionar',              'Merge'],
        dismiss:    ['Ignorar',               'Dismiss'],
    };
    return labels[res]?.[isEN ? 1 : 0] || res;
}
