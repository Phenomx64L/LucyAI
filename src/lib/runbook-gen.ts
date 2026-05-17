// ── runbook-gen.ts — Auto-generate runbooks from resolved incidents ──────────
//
// PURPOSE: When an incident is finalized as 'resolved', this module
// synthesizes the investigation trail (evidence, actions, hypotheses) into
// a structured Markdown runbook. The runbook is saved to the configured
// runbooks directory so future `search_runbooks` queries can find it.
//
// DESIGN:
//   • Extracts structured data from the incident record + its child tables
//   • Generates idempotent markdown (same incident → same filename, overwrite)
//   • Embeds the runbook for semantic search via the embeddings system
//   • Zero LLM calls — pure template synthesis from structured data
//
// INTEGRATION:
//   Called after successful `incident_finalize` from the frontend.

import { invoke } from '@tauri-apps/api/core';

// ── Types ───────────────────────────────────────────────────────────────────

interface IncidentRecord {
    id: string;
    host_name: string;
    title: string;
    description: string;
    phase: string;
    status: string;
    summary: string | null;
    root_cause: string | null;
    created_at: number;
    resolved_at: number | null;
}

interface EvidenceRecord {
    id: string;
    kind: string;
    source: string;
    content: string;
    tags: string;
    timestamp: number;
    phase: string;
}

interface ActionRecord {
    id: string;
    phase: string;
    rationale: string;
    command: string;
    executed_at: number;
}

interface HypothesisRecord {
    id: string;
    claim: string;
    supporting_evidence_ids: string;
    contradicting_evidence_ids: string;
    status: string;
    confidence: number;
}

// ── Main API ────────────────────────────────────────────────────────────────

/**
 * Generate a runbook from a resolved incident and save it to the runbooks directory.
 * Returns the file path where the runbook was written, or null if conditions aren't met.
 *
 * @param incidentId - The resolved incident ID
 * @param runbooksDir - Path to the runbooks directory (from lucyConfig.runbooksDir)
 */
export async function generateRunbookFromIncident(
    incidentId: string,
    runbooksDir: string | null,
): Promise<string | null> {
    if (!runbooksDir) {
        console.log('[runbook-gen] No runbooks directory configured, skipping');
        return null;
    }

    try {
        // Fetch incident + related data
        const incident = await invoke<IncidentRecord>('incident_get', { incidentId });
        if (!incident || incident.status !== 'resolved') {
            return null;
        }

        const evidence = await invoke<EvidenceRecord[]>('incident_list_evidence', {
            incidentId,
        });

        const hypotheses = await invoke<HypothesisRecord[]>('incident_list_hypotheses', {
            incidentId,
        });

        // Build the markdown content
        const markdown = buildRunbookMarkdown(incident, evidence, hypotheses);

        // Generate filename from title (sanitized)
        const slug = sanitizeFilename(incident.title);
        const dateStr = new Date(incident.created_at * 1000).toISOString().split('T')[0];
        const filename = `${dateStr}-${slug}.md`;
        const fullPath = `${runbooksDir.replace(/[/\\]$/, '')}/${filename}`;

        // Write the file
        await invoke('write_file_content', {
            path: fullPath,
            content: markdown,
        });

        // Embed for semantic search (fire-and-forget)
        invoke('upsert_embedding', {
            entityType: 'runbook',
            entityId: `runbook:${incident.id}`,
            text: `${incident.title} - ${incident.summary || ''} - Root cause: ${incident.root_cause || ''}`,
            model: null,
        }).catch(() => {}); // Best-effort, don't block

        console.log(`[runbook-gen] Generated runbook: ${fullPath}`);
        return fullPath;
    } catch (e) {
        console.warn('[runbook-gen] Failed to generate runbook:', e);
        return null;
    }
}

// ── Template builder ────────────────────────────────────────────────────────

function buildRunbookMarkdown(
    incident: IncidentRecord,
    evidence: EvidenceRecord[],
    hypotheses: HypothesisRecord[],
): string {
    const created = new Date(incident.created_at * 1000).toISOString();
    const resolved = incident.resolved_at
        ? new Date(incident.resolved_at * 1000).toISOString()
        : 'N/A';
    const duration = incident.resolved_at
        ? formatDuration(incident.resolved_at - incident.created_at)
        : 'N/A';

    const sections: string[] = [];

    // ── Header
    sections.push(`# ${incident.title}\n`);
    sections.push(`> Auto-generated runbook from resolved incident \`${incident.id}\`\n`);

    // ── Metadata table
    sections.push(`## Metadata\n`);
    sections.push(`| Field | Value |`);
    sections.push(`|-------|-------|`);
    sections.push(`| Host | ${incident.host_name} |`);
    sections.push(`| Created | ${created} |`);
    sections.push(`| Resolved | ${resolved} |`);
    sections.push(`| Duration | ${duration} |`);
    sections.push(`| Status | ${incident.status} |`);
    sections.push('');

    // ── Summary
    if (incident.summary) {
        sections.push(`## Summary\n`);
        sections.push(incident.summary);
        sections.push('');
    }

    // ── Root Cause
    if (incident.root_cause) {
        sections.push(`## Root Cause\n`);
        sections.push(incident.root_cause);
        sections.push('');
    }

    // ── Symptoms (from description)
    sections.push(`## Symptoms\n`);
    sections.push(incident.description);
    sections.push('');

    // ── Key Evidence
    if (evidence.length > 0) {
        sections.push(`## Key Evidence\n`);
        const sorted = [...evidence].sort((a, b) => a.timestamp - b.timestamp);
        for (const ev of sorted.slice(0, 15)) {
            const ts = new Date(ev.timestamp * 1000).toLocaleTimeString();
            sections.push(`### [${ts}] ${ev.source}\n`);
            sections.push(`**Type:** ${ev.kind} | **Phase:** ${ev.phase}\n`);
            // Truncate very long evidence to keep runbook readable
            const content = ev.content.length > 800
                ? ev.content.slice(0, 800) + '\n\n*[truncated]*'
                : ev.content;
            sections.push('```');
            sections.push(content);
            sections.push('```\n');
        }
        if (evidence.length > 15) {
            sections.push(`*+${evidence.length - 15} additional evidence items omitted*\n`);
        }
    }

    // ── Hypotheses
    if (hypotheses.length > 0) {
        sections.push(`## Hypotheses Evaluated\n`);
        const sorted = [...hypotheses].sort((a, b) => b.confidence - a.confidence);
        for (const h of sorted) {
            const conf = (h.confidence * 100).toFixed(0);
            const statusIcon = h.status === 'validated' ? '✓' : h.status === 'refuted' ? '✗' : '◐';
            sections.push(`- ${statusIcon} **[${conf}%]** (${h.status}) ${h.claim}`);
        }
        sections.push('');
    }

    // ── Resolution Steps (what to do if this happens again)
    sections.push(`## Resolution Steps\n`);
    sections.push(`If this issue recurs, follow these steps:\n`);
    if (incident.root_cause) {
        sections.push(`1. Verify the root cause: ${incident.root_cause.split('\n')[0]}`);
    }
    sections.push(`2. Check the host \`${incident.host_name}\` for the same symptoms`);
    if (evidence.length > 0) {
        sections.push(`3. Collect evidence: run the same diagnostic commands used in this investigation`);
    }
    sections.push(`4. Apply the fix documented above`);
    sections.push(`5. Monitor for ${duration || '15 minutes'} to confirm resolution`);
    sections.push('');

    // ── Footer
    sections.push('---');
    sections.push(`*Generated by Lucy SRE Mode on ${new Date().toISOString()}*`);

    return sections.join('\n');
}

// ── Utilities ───────────────────────────────────────────────────────────────

function sanitizeFilename(title: string): string {
    const slug = title
        .toLowerCase()
        .replace(/\[auto\]\s*/i, '')
        .replace(/[^a-z0-9\s-]/g, '')
        .replace(/\s+/g, '-')
        .slice(0, 60)
        .replace(/-+$/, '');
    // Fallback if title was all special chars (e.g. emojis, CJK)
    return slug || 'runbook';
}

function formatDuration(seconds: number): string {
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
    const h = Math.floor(seconds / 3600);
    const m = Math.round((seconds % 3600) / 60);
    return `${h}h ${m}m`;
}
