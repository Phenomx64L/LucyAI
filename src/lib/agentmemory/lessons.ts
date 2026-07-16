// ── lessons.ts — Lessons Learned panel, separated from crystals ──────────
//
// Lessons are reusable patterns or insights extracted during crystal
// creation (`crystallize_session`). They live in `agent_memories` tagged
// "crystal,lesson" AND in each crystal's `lessons` JSON array.
//
// This module provides a unified API to:
//   1. Gather all lessons from both sources (deduped)
//   2. Search/filter by keyword, project, date range
//   3. Promote a lesson to an insight (high confidence)
//   4. Dismiss / archive a lesson
//   5. Rate a lesson (adjust importance)

import { invoke } from '@tauri-apps/api/core';

// ── Types ────────────────────────────────────────────────────────────────

export interface Lesson {
    id: string;                // unique: 'mem-<id>' or 'crystal-<crystalId>-<idx>'
    text: string;
    source: 'memory' | 'crystal';
    sourceId: number;          // agent_memories.id or agent_crystals.id
    project: string;
    sessionId: string;
    importance: number;        // 1-3
    createdAt: number;         // epoch seconds
    tags: string[];
    files: string[];
}

export interface LessonFilter {
    query?: string;
    project?: string;
    minImportance?: number;
    limit?: number;
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

// ── Core API ─────────────────────────────────────────────────────────────

/**
 * Fetch all lessons from both sources, deduped by text similarity.
 * Returns newest-first, capped at `filter.limit` (default 100).
 */
export async function getLessons(filter: LessonFilter = {}): Promise<Lesson[]> {
    const limit = filter.limit ?? 100;
    const minImp = filter.minImportance ?? 0;

    // 1. Fetch lesson-tagged memories
    let memLessons: Lesson[] = [];
    try {
        const mems: RawMemory[] = filter.query
            ? await invoke('search_agent_memories', { query: `lesson ${filter.query}`, limit: 50, excludeDocuments: true })
            : await invoke('get_recent_memories', { limit: 50 });

        memLessons = mems
            .filter(m => {
                const tags = safeParseTags(m.tags);
                return tags.includes('lesson') || tags.includes('crystallized');
            })
            .map(m => ({
                id: `mem-${m.id}`,
                text: m.content,
                source: 'memory' as const,
                sourceId: m.id,
                project: '',
                sessionId: m.session_id,
                importance: m.importance,
                createdAt: m.created_at,
                tags: safeParseTags(m.tags),
                files: safeParseTags(m.files),
            }));
    } catch { /* backend unavailable */ }

    // 2. Fetch crystal lessons
    let crystalLessons: Lesson[] = [];
    try {
        const crystals: RawCrystal[] = await invoke('list_crystals', {
            sessionId: null,
            project: filter.project ?? null,
            limit: 50,
        });

        for (const c of crystals) {
            const lessons = safeParseTags(c.lessons);
            const files = safeParseTags(c.files_affected);
            for (let i = 0; i < lessons.length; i++) {
                crystalLessons.push({
                    id: `crystal-${c.id}-${i}`,
                    text: lessons[i],
                    source: 'crystal',
                    sourceId: c.id,
                    project: c.project || '',
                    sessionId: c.session_id,
                    importance: 2,  // crystal lessons are importance 2 by default
                    createdAt: c.created_at,
                    tags: ['crystal', 'lesson'],
                    files,
                });
            }
        }
    } catch { /* backend unavailable */ }

    // 3. Merge + dedupe (prefer memory version if text matches)
    const seen = new Set<string>();
    const merged: Lesson[] = [];

    for (const l of [...memLessons, ...crystalLessons]) {
        const key = normalizeForDedup(l.text);
        if (seen.has(key)) continue;
        seen.add(key);

        if (minImp > 0 && l.importance < minImp) continue;
        if (filter.query) {
            const q = filter.query.toLowerCase();
            if (!l.text.toLowerCase().includes(q) && !l.project.toLowerCase().includes(q)) continue;
        }
        if (filter.project && l.project && !l.project.toLowerCase().includes(filter.project.toLowerCase())) continue;

        merged.push(l);
    }

    // Sort newest first
    merged.sort((a, b) => b.createdAt - a.createdAt);
    return merged.slice(0, limit);
}

/**
 * Promote a lesson to an agent_memory with importance 3 (critical),
 * making it visible in the Insights pipeline for future reflection.
 */
export async function promoteLesson(lesson: Lesson): Promise<void> {
    if (lesson.source === 'memory') {
        // Update importance via supersede pattern: save a new memory with higher importance
        await invoke('save_agent_memory', {
            title: `[Promoted Lesson] ${lesson.text.slice(0, 80)}`,
            content: lesson.text,
            tags: JSON.stringify([...new Set([...lesson.tags, 'promoted', 'lesson'])]),
            files: JSON.stringify(lesson.files),
            sessionId: lesson.sessionId || null,
            importance: 3,
            ttlDays: null,
        });
    } else {
        // Crystal lesson → create new memory entry
        await invoke('save_agent_memory', {
            title: `[Promoted Lesson] ${lesson.text.slice(0, 80)}`,
            content: lesson.text,
            tags: JSON.stringify(['crystal', 'lesson', 'promoted']),
            files: JSON.stringify(lesson.files),
            sessionId: lesson.sessionId || null,
            importance: 3,
            ttlDays: null,
        });
    }
}

/**
 * Dismiss a lesson by deleting the underlying memory (only for memory-sourced lessons).
 * Crystal-sourced lessons can't be individually deleted.
 */
export async function dismissLesson(lesson: Lesson): Promise<boolean> {
    if (lesson.source === 'memory') {
        try {
            await invoke('delete_agent_memory', { id: lesson.sourceId });
            return true;
        } catch { return false; }
    }
    return false;  // crystal lessons survive — they're part of the crystal
}

/**
 * Get unique project names from all crystals (for filter dropdown).
 */
export async function getLessonProjects(): Promise<string[]> {
    try {
        const crystals: RawCrystal[] = await invoke('list_crystals', {
            sessionId: null, project: null, limit: 100,
        });
        const projects = new Set<string>();
        for (const c of crystals) {
            if (c.project) projects.add(c.project);
        }
        return [...projects].sort();
    } catch { return []; }
}

// ── Helpers ──────────────────────────────────────────────────────────────

function safeParseTags(json: string): string[] {
    try {
        const parsed = JSON.parse(json);
        return Array.isArray(parsed) ? parsed : [];
    } catch { return []; }
}

function normalizeForDedup(text: string): string {
    return text.toLowerCase().replace(/\s+/g, ' ').trim().slice(0, 200);
}
