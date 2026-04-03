// ── audit.ts — Helper para registrar entradas en el Audit Trail ────────────────

import { get } from 'svelte/store';
import { auditTrail } from './stores';
import type { AuditEntry } from './stores';

const MAX_ENTRIES = 500;
const PREVIEW_LENGTH = 500;

export function logAuditEntry(entry: Omit<AuditEntry, 'id' | 'timestamp'>): void {
    const full: AuditEntry = {
        ...entry,
        timestamp: new Date().toISOString(),
        outputPreview: (entry.outputPreview || '').substring(0, PREVIEW_LENGTH),
    };
    auditTrail.update(trail => {
        const updated = [...trail, full];
        // Keep only the last MAX_ENTRIES
        if (updated.length > MAX_ENTRIES) updated.splice(0, updated.length - MAX_ENTRIES);
        return updated;
    });
}

export function getAuditEntries(filters?: {
    hostId?: string;
    source?: string;
    search?: string;
    since?: number; // timestamp
}): AuditEntry[] {
    let entries = get(auditTrail);
    if (filters?.hostId) entries = entries.filter(e => e.hostId === filters.hostId);
    if (filters?.source) entries = entries.filter(e => e.source === filters.source);
    const sinceTimestamp = filters?.since;
    if (sinceTimestamp)  entries = entries.filter(e => new Date(e.timestamp).getTime() >= sinceTimestamp);
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
