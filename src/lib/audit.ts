// ── audit.ts — Audit Trail: persistent SQLite-backed command log (P0 Feature 1)
//
// Replaces the old localStorage-backed auditTrail store with durable SQLite
// storage via Tauri commands. Falls back to in-memory cache for offline display.

import { invoke } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import { auditTrail } from './stores';
import type { AuditEntry } from './stores';

const PREVIEW_LENGTH = 500;

/**
 * Log a command execution to the persistent audit trail (SQLite).
 * Also updates the in-memory store for instant UI reactivity.
 */
export async function logAuditEntry(entry: Omit<AuditEntry, 'id' | 'timestamp'>): Promise<void> {
    const timestamp = new Date().toISOString();
    const preview = (entry.outputPreview || '').substring(0, PREVIEW_LENGTH);

    // Persist to SQLite via Tauri command
    try {
        const id = await invoke<number>('save_audit_entry', {
            timestamp,
            hostId: entry.hostId || '',
            hostName: entry.hostName || 'local',
            command: entry.command,
            source: entry.source || 'manual',
            exitCode: entry.exitCode ?? null,
            durationMs: entry.durationMs ?? null,
            outputPreview: preview,
            user: entry.user || '',
        });

        // Update in-memory store for reactive UI
        const full: AuditEntry = {
            ...entry,
            id: id,
            timestamp,
            outputPreview: preview,
        };
        auditTrail.update(trail => {
            const updated = [...trail, full];
            if (updated.length > 200) updated.splice(0, updated.length - 200);
            return updated;
        });
    } catch (e) {
        // Fallback: still update in-memory store even if SQLite fails
        console.warn('[audit] SQLite persist failed, using in-memory fallback:', e);
        const full: AuditEntry = {
            ...entry,
            id: Date.now() + Math.random(),
            timestamp,
            outputPreview: preview,
        };
        auditTrail.update(trail => {
            const updated = [...trail, full];
            if (updated.length > 500) updated.splice(0, updated.length - 500);
            return updated;
        });
    }
}

/** Legacy sync wrapper — calls the async version fire-and-forget. */
export function logAuditEntrySync(entry: Omit<AuditEntry, 'id' | 'timestamp'>): void {
    logAuditEntry(entry).catch(() => {});
}

export interface AuditQueryFilters {
    hostId?: string;
    source?: string;
    search?: string;
    fromTs?: number;
    toTs?: number;
    limit?: number;
    offset?: number;
}

export interface AuditQueryResult {
    entries: AuditEntry[];
    total_count: number;
    has_more: boolean;
}

/**
 * Query the persistent audit trail from SQLite with optional filters.
 * Returns paginated results for the Audit panel.
 */
export async function queryAuditTrail(filters?: AuditQueryFilters): Promise<AuditQueryResult> {
    try {
        return await invoke<AuditQueryResult>('query_audit_trail', {
            hostId: filters?.hostId || null,
            source: filters?.source || null,
            search: filters?.search || null,
            fromTs: filters?.fromTs || null,
            toTs: filters?.toTs || null,
            limit: filters?.limit || 100,
            offset: filters?.offset || 0,
        });
    } catch (e) {
        // Fallback: return in-memory entries
        console.warn('[audit] SQLite query failed, using in-memory fallback:', e);
        let entries = get(auditTrail);
        if (filters?.hostId) entries = entries.filter(e => e.hostId === filters.hostId);
        if (filters?.source) entries = entries.filter(e => e.source === filters.source);
        if (filters?.search) {
            const q = filters.search.toLowerCase();
            entries = entries.filter(e =>
                e.command.toLowerCase().includes(q) ||
                e.hostName.toLowerCase().includes(q) ||
                e.outputPreview.toLowerCase().includes(q)
            );
        }
        return { entries, total_count: entries.length, has_more: false };
    }
}

/** Get audit stats (total count, by source, by host). */
export async function getAuditStats(): Promise<{
    total: number;
    by_source: Record<string, number>;
    by_host: Record<string, number>;
}> {
    return await invoke('audit_stats');
}

/** Prune old audit entries. Returns count of deleted rows. */
export async function pruneAuditTrail(days = 90): Promise<number> {
    return await invoke<number>('prune_audit_trail', { days });
}

// ── Legacy compat ───────────────────────────────────────────────────────────

/** @deprecated Use queryAuditTrail instead */
export function getAuditEntries(filters?: {
    hostId?: string;
    source?: string;
    search?: string;
    since?: number;
}): AuditEntry[] {
    let entries = get(auditTrail);
    if (filters?.hostId) entries = entries.filter(e => e.hostId === filters.hostId);
    if (filters?.source) entries = entries.filter(e => e.source === filters.source);
    const sinceTimestamp = filters?.since;
    if (sinceTimestamp) entries = entries.filter(e => new Date(e.timestamp).getTime() >= sinceTimestamp);
    if (filters?.search) {
        const q = filters.search.toLowerCase();
        entries = entries.filter(e =>
            e.command.toLowerCase().includes(q) ||
            e.hostName.toLowerCase().includes(q) ||
            e.outputPreview.toLowerCase().includes(q)
        );
    }
    return entries;
}

export function clearAuditTrail(): void {
    auditTrail.set([]);
}
