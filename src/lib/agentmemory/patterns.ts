// ── patterns.ts — Deterministic pattern detection on memory/lessons ──────
//
// Unlike `reflect_run` (which uses LLM clustering to extract meta-insights),
// this module runs DETERMINISTIC rules over the memory/crystal/lesson corpus
// to surface recurring patterns that don't need an LLM to detect:
//
//   • Tag frequency hotspots — tags appearing in 5+ memories
//   • File hotspots — files touched across 3+ sessions
//   • Temporal clusters — bursts of similar memories within 24h windows
//   • Error recurrence — same error pattern in multiple crystals
//   • Lesson redundancy — near-duplicate lessons across projects
//
// All detection is client-side (reads from existing Tauri commands).
// Results feed the MemoryBrowserView "Patterns" section and can trigger
// sentinel rules.

import { invoke } from '@tauri-apps/api/core';

// ── Types ────────────────────────────────────────────────────────────────

export type PatternKind =
    | 'tag_hotspot'
    | 'file_hotspot'
    | 'temporal_cluster'
    | 'error_recurrence'
    | 'lesson_redundancy';

export interface DetectedPattern {
    kind: PatternKind;
    /** Human-readable title */
    title: string;
    /** Detailed description */
    description: string;
    /** Confidence 0-1 (higher = more certain) */
    confidence: number;
    /** Number of memories/crystals involved */
    evidenceCount: number;
    /** Related entity IDs (memory or crystal) */
    relatedIds: number[];
    /** Primary data key (tag name, file path, etc.) */
    key: string;
    /** When detected (epoch ms) */
    detectedAt: number;
}

export interface PatternReport {
    patterns: DetectedPattern[];
    scannedMemories: number;
    scannedCrystals: number;
    scanDurationMs: number;
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

interface RawCrystal {
    id: number;
    session_id: string;
    project: string;
    narrative: string;
    key_outcomes: string;
    files_affected: string;
    lessons: string;
    source_chars: number;
    created_at: number;
}

// ── Detection Engine ─────────────────────────────────────────────────────

/**
 * Run all pattern detectors over the memory corpus.
 * Lightweight (~50-200ms) — safe to run on UI interaction.
 */
export async function detectPatterns(): Promise<PatternReport> {
    const t0 = Date.now();

    // Fetch data
    let memories: RawMemory[] = [];
    let crystals: RawCrystal[] = [];
    try { memories = await invoke('get_recent_memories', { limit: 50 }); } catch {}
    try { crystals = await invoke('list_crystals', { sessionId: null, project: null, limit: 50 }); } catch {}

    const patterns: DetectedPattern[] = [];

    // Run all detectors
    patterns.push(...detectTagHotspots(memories));
    patterns.push(...detectFileHotspots(memories, crystals));
    patterns.push(...detectTemporalClusters(memories));
    patterns.push(...detectErrorRecurrence(crystals));
    patterns.push(...detectLessonRedundancy(crystals));

    // Sort by confidence DESC, then evidence count DESC
    patterns.sort((a, b) => b.confidence - a.confidence || b.evidenceCount - a.evidenceCount);

    return {
        patterns,
        scannedMemories: memories.length,
        scannedCrystals: crystals.length,
        scanDurationMs: Date.now() - t0,
    };
}

// ── Individual Detectors ─────────────────────────────────────────────────

/** Tags that appear in 5+ memories indicate a recurring theme. */
function detectTagHotspots(memories: RawMemory[]): DetectedPattern[] {
    const tagCounts = new Map<string, number[]>();  // tag → memory ids
    for (const m of memories) {
        for (const tag of safeParse(m.tags)) {
            if (!tag || tag === 'crystal' || tag === 'lesson') continue;  // skip meta-tags
            const ids = tagCounts.get(tag) || [];
            ids.push(m.id);
            tagCounts.set(tag, ids);
        }
    }

    const patterns: DetectedPattern[] = [];
    const MIN_TAG_COUNT = 5;
    for (const [tag, ids] of tagCounts) {
        if (ids.length >= MIN_TAG_COUNT) {
            patterns.push({
                kind: 'tag_hotspot',
                title: `Tag recurrente: "${tag}"`,
                description: `El tag "${tag}" aparece en ${ids.length} memorias. Puede indicar un tema dominante o área de trabajo frecuente.`,
                confidence: Math.min(1, ids.length / 10),
                evidenceCount: ids.length,
                relatedIds: ids.slice(0, 20),
                key: tag,
                detectedAt: Date.now(),
            });
        }
    }
    return patterns;
}

/** Files touched across 3+ distinct sessions suggest infrastructure hotspots. */
function detectFileHotspots(memories: RawMemory[], crystals: RawCrystal[]): DetectedPattern[] {
    const fileSessions = new Map<string, Set<string>>();  // file → session_ids

    for (const m of memories) {
        const files = safeParse(m.files);
        for (const f of files) {
            if (!f) continue;
            const sessions = fileSessions.get(f) || new Set();
            sessions.add(m.session_id);
            fileSessions.set(f, sessions);
        }
    }
    for (const c of crystals) {
        const files = safeParse(c.files_affected);
        for (const f of files) {
            if (!f) continue;
            const sessions = fileSessions.get(f) || new Set();
            sessions.add(c.session_id);
            fileSessions.set(f, sessions);
        }
    }

    const patterns: DetectedPattern[] = [];
    const MIN_SESSIONS = 3;
    for (const [file, sessions] of fileSessions) {
        if (sessions.size >= MIN_SESSIONS) {
            patterns.push({
                kind: 'file_hotspot',
                title: `Archivo frecuente: ${shortenPath(file)}`,
                description: `"${file}" fue modificado/referenciado en ${sessions.size} sesiones distintas. Posible punto de fricción o componente crítico.`,
                confidence: Math.min(1, sessions.size / 6),
                evidenceCount: sessions.size,
                relatedIds: [],
                key: file,
                detectedAt: Date.now(),
            });
        }
    }
    return patterns;
}

/** Memories created within 24h windows of each other with overlapping tags. */
function detectTemporalClusters(memories: RawMemory[]): DetectedPattern[] {
    if (memories.length < 4) return [];

    const WINDOW = 24 * 3600;  // 24 hours in seconds
    const sorted = [...memories].sort((a, b) => a.created_at - b.created_at);
    const patterns: DetectedPattern[] = [];

    let i = 0;
    while (i < sorted.length) {
        const cluster: RawMemory[] = [sorted[i]];
        let j = i + 1;
        while (j < sorted.length && (sorted[j].created_at - sorted[i].created_at) <= WINDOW) {
            cluster.push(sorted[j]);
            j++;
        }

        if (cluster.length >= 4) {
            // Check tag overlap
            const tagSets = cluster.map(m => new Set(safeParse(m.tags)));
            const commonTags = intersectAll(tagSets);
            if (commonTags.size > 0) {
                const date = new Date(sorted[i].created_at * 1000).toLocaleDateString();
                patterns.push({
                    kind: 'temporal_cluster',
                    title: `Cluster temporal (${date})`,
                    description: `${cluster.length} memorias creadas en 24h con tags comunes: ${[...commonTags].join(', ')}. Posible sesión de trabajo intensiva.`,
                    confidence: Math.min(1, cluster.length / 8),
                    evidenceCount: cluster.length,
                    relatedIds: cluster.map(m => m.id),
                    key: date,
                    detectedAt: Date.now(),
                });
            }
        }
        i = j > i + 1 ? j : i + 1;  // skip past cluster
    }
    return patterns;
}

/** Crystal outcomes mentioning "error", "fail", "fix" patterns recurring. */
function detectErrorRecurrence(crystals: RawCrystal[]): DetectedPattern[] {
    const errorPatterns = new Map<string, number[]>();  // normalized error → crystal ids
    const errorRx = /\b(error|fail|bug|crash|exception|broken|fix)\b/i;

    for (const c of crystals) {
        const outcomes = safeParse(c.key_outcomes);
        for (const outcome of outcomes) {
            if (!errorRx.test(outcome)) continue;
            const key = normalizePattern(outcome);
            const ids = errorPatterns.get(key) || [];
            ids.push(c.id);
            errorPatterns.set(key, ids);
        }
    }

    const patterns: DetectedPattern[] = [];
    for (const [key, ids] of errorPatterns) {
        if (ids.length >= 2) {
            patterns.push({
                kind: 'error_recurrence',
                title: `Error recurrente: "${key.slice(0, 60)}"`,
                description: `Un patrón de error similar aparece en ${ids.length} crystals. Puede indicar un problema no resuelto completamente.`,
                confidence: Math.min(1, ids.length / 4),
                evidenceCount: ids.length,
                relatedIds: ids,
                key,
                detectedAt: Date.now(),
            });
        }
    }
    return patterns;
}

/** Near-duplicate lessons across different crystals/projects. */
function detectLessonRedundancy(crystals: RawCrystal[]): DetectedPattern[] {
    const lessonIndex = new Map<string, { text: string; crystalIds: number[] }>();

    for (const c of crystals) {
        const lessons = safeParse(c.lessons);
        for (const lesson of lessons) {
            if (!lesson || lesson.length < 10) continue;
            const key = normalizePattern(lesson);
            const entry = lessonIndex.get(key) || { text: lesson, crystalIds: [] };
            entry.crystalIds.push(c.id);
            lessonIndex.set(key, entry);
        }
    }

    const patterns: DetectedPattern[] = [];
    for (const [, entry] of lessonIndex) {
        const uniqueCrystals = [...new Set(entry.crystalIds)];
        if (uniqueCrystals.length >= 2) {
            patterns.push({
                kind: 'lesson_redundancy',
                title: `Lección redundante: "${entry.text.slice(0, 50)}…"`,
                description: `La misma lección aparece en ${uniqueCrystals.length} crystals distintos. Considere promoverla a insight.`,
                confidence: Math.min(1, uniqueCrystals.length / 3),
                evidenceCount: uniqueCrystals.length,
                relatedIds: uniqueCrystals,
                key: entry.text,
                detectedAt: Date.now(),
            });
        }
    }
    return patterns;
}

// ── Helpers ──────────────────────────────────────────────────────────────

function safeParse(json: string): string[] {
    try {
        const arr = JSON.parse(json);
        return Array.isArray(arr) ? arr : [];
    } catch { return []; }
}

/** Normalize text for pattern matching: lowercase, collapse whitespace, remove numbers. */
function normalizePattern(text: string): string {
    return text
        .toLowerCase()
        .replace(/\d+/g, 'N')
        .replace(/\s+/g, ' ')
        .trim()
        .slice(0, 120);
}

function shortenPath(path: string): string {
    const parts = path.replace(/\\/g, '/').split('/');
    return parts.length > 3 ? `…/${parts.slice(-3).join('/')}` : path;
}

function intersectAll(sets: Set<string>[]): Set<string> {
    if (sets.length === 0) return new Set();
    let result = new Set(sets[0]);
    for (let i = 1; i < sets.length; i++) {
        result = new Set([...result].filter(x => sets[i].has(x)));
    }
    return result;
}

// ── Pattern kind labels ──────────────────────────────────────────────────

export function patternKindLabel(kind: PatternKind, isEN = false): string {
    const labels: Record<PatternKind, [string, string]> = {
        tag_hotspot:       ['Tag recurrente',     'Recurring tag'],
        file_hotspot:      ['Archivo frecuente',  'Frequent file'],
        temporal_cluster:  ['Cluster temporal',   'Temporal cluster'],
        error_recurrence:  ['Error recurrente',   'Recurring error'],
        lesson_redundancy: ['Lección redundante', 'Redundant lesson'],
    };
    return labels[kind]?.[isEN ? 1 : 0] || kind;
}

export function patternKindIcon(kind: PatternKind): string {
    const icons: Record<PatternKind, string> = {
        tag_hotspot:       '🏷',
        file_hotspot:      '📁',
        temporal_cluster:  '⏰',
        error_recurrence:  '🔴',
        lesson_redundancy: '🔄',
    };
    return icons[kind] || '◉';
}

export function confidenceColor(confidence: number): string {
    if (confidence >= 0.8) return '#f87171';  // red — high confidence
    if (confidence >= 0.5) return '#fbbf24';  // amber
    return '#60a5fa';                         // blue — low confidence
}
