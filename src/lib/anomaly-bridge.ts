// ── anomaly-bridge.ts — Connects anomaly detection to the incident system ────
//
// PURPOSE: When the Dashboard detects a 'strong' or 'extreme' anomaly that
// persists for DEBOUNCE_MS, this module auto-triggers an incident via the
// backend `incident_start` command. It also emits a frontend event so the
// UI can show an alert banner / switch to SRE mode.
//
// DESIGN:
//   • Debounced: an anomaly must persist for ≥30s before triggering (avoids
//     one-off spikes from triggering full incidents)
//   • Deduplication: won't create a second incident for the same metric on
//     the same host if one is already open
//   • Configurable: severity threshold and debounce are adjustable
//   • Emits custom event 'lucy:auto-incident' for frontend consumption
//
// INTEGRATION:
//   Called from DashboardView on every metrics refresh when anomaly is detected.

import { invoke } from '@tauri-apps/api/core';
import type { AnomalySeverity, AnomalyResult } from '$lib/anomaly';

// ── Configuration ───────────────────────────────────────────────────────────

/** Minimum severity to trigger auto-incident. */
export type TriggerSeverity = 'strong' | 'extreme';

export interface AnomalyBridgeConfig {
    /** Minimum severity to trigger ('strong' or 'extreme'). Default: 'strong'. */
    minSeverity: TriggerSeverity;
    /** How long (ms) the anomaly must persist before triggering. Default: 30000. */
    debounceMs: number;
    /** Whether auto-incidents are enabled. Default: true. */
    enabled: boolean;
    /** Maximum open auto-incidents per host. Default: 2. */
    maxPerHost: number;
}

const DEFAULT_CONFIG: AnomalyBridgeConfig = {
    minSeverity: 'strong',
    debounceMs: 30_000,
    enabled: true,
    maxPerHost: 2,
};

// ── State ───────────────────────────────────────────────────────────────────

interface PendingAnomaly {
    hostId: string;
    metric: 'cpu' | 'ram' | 'disk';
    severity: AnomalySeverity;
    sigma: number;
    message: string;
    firstSeen: number;
    lastSeen: number;
}

/** Active pending anomalies being debounced (keyed by hostId:metric). */
const pending = new Map<string, PendingAnomaly>();

/** Set of incident IDs auto-created (keyed by hostId:metric) to prevent duplicates.
 *  Value is { id, createdAt } — entries older than TTL_MS are auto-evicted. */
const AUTO_INCIDENT_TTL_MS = 4 * 60 * 60 * 1000; // 4 hours
const activeAutoIncidents = new Map<string, { id: string; createdAt: number }>();

/** Evict stale entries older than TTL from the activeAutoIncidents map. */
function evictStaleIncidents(): void {
    const now = Date.now();
    for (const [key, entry] of activeAutoIncidents.entries()) {
        if (now - entry.createdAt > AUTO_INCIDENT_TTL_MS) {
            activeAutoIncidents.delete(key);
        }
    }
}

let config: AnomalyBridgeConfig = { ...DEFAULT_CONFIG };

// ── Public API ──────────────────────────────────────────────────────────────

/**
 * Update bridge configuration. Merges with defaults.
 */
export function configureBridge(partial: Partial<AnomalyBridgeConfig>): void {
    config = { ...DEFAULT_CONFIG, ...partial };
}

/**
 * Report an anomaly detection result. Called by the Dashboard on every refresh.
 * Handles debouncing and auto-trigger logic internally.
 *
 * @param hostId - Host identifier ('local' or host.id)
 * @param hostName - Display name for the incident title
 * @param metric - Which metric ('cpu', 'ram', 'disk')
 * @param anomaly - The AnomalyResult from detectAnomaly()
 * @param shellId - Shell/tab ID to associate the incident with
 */
export async function reportAnomaly(
    hostId: string,
    hostName: string,
    metric: 'cpu' | 'ram' | 'disk',
    anomaly: AnomalyResult,
    shellId: string = 'dashboard',
): Promise<void> {
    if (!config.enabled) return;

    const key = `${hostId}:${metric}`;
    const now = Date.now();

    // If anomaly cleared or below threshold, remove from pending
    if (!meetsThreshold(anomaly.severity)) {
        pending.delete(key);
        return;
    }

    // Update or create pending entry
    const existing = pending.get(key);
    if (existing) {
        existing.lastSeen = now;
        existing.severity = anomaly.severity;
        existing.sigma = anomaly.sigma;
        existing.message = anomaly.message;
    } else {
        pending.set(key, {
            hostId, metric,
            severity: anomaly.severity,
            sigma: anomaly.sigma,
            message: anomaly.message,
            firstSeen: now,
            lastSeen: now,
        });
    }

    // Check if debounce period has elapsed
    const entry = pending.get(key)!;
    const elapsed = now - entry.firstSeen;
    if (elapsed < config.debounceMs) return;

    // Evict stale entries before checking
    evictStaleIncidents();

    // Check dedup: don't create if we already have an open auto-incident for this key
    if (activeAutoIncidents.has(key)) return;

    // Check max per host
    const hostIncidentCount = [...activeAutoIncidents.keys()].filter(k => k.startsWith(hostId + ':')).length;
    if (hostIncidentCount >= config.maxPerHost) return;

    // Guard against concurrent async calls: mark as in-flight immediately
    // to prevent a second call from passing the dedup check above while
    // the invoke() is awaited.
    activeAutoIncidents.set(key, { id: '__pending__', createdAt: Date.now() });

    // ── Trigger auto-incident ───────────────────────────────────────────
    try {
        const title = `[AUTO] ${metric.toUpperCase()} anomaly on ${hostName}`;
        const description = buildDescription(entry, hostName);

        const incident = await invoke<any>('incident_start', {
            args: {
                shell_id: shellId,
                host_name: hostName,
                title,
                description,
                max_loops: 8, // generous budget for auto-triggered investigations
            }
        });

        activeAutoIncidents.set(key, { id: incident.id, createdAt: Date.now() });
        pending.delete(key);

        // Emit custom event for UI notification
        window.dispatchEvent(new CustomEvent('lucy:auto-incident', {
            detail: {
                incidentId: incident.id,
                hostId, hostName, metric,
                severity: entry.severity,
                sigma: entry.sigma,
                message: entry.message,
            }
        }));

        console.log(`[anomaly-bridge] Auto-triggered incident ${incident.id}: ${title}`);
    } catch (e) {
        // Remove the in-flight guard so a retry can succeed
        activeAutoIncidents.delete(key);
        console.warn('[anomaly-bridge] Failed to create auto-incident:', e);
    }
}

/**
 * Mark an auto-incident as resolved (removes from dedup set).
 * Call when incident_finalize succeeds on an auto-triggered incident.
 */
export function markResolved(hostId: string, metric: string): void {
    const key = `${hostId}:${metric}`;
    activeAutoIncidents.delete(key);
}

/**
 * Mark a specific incident ID as resolved (by scanning the map).
 */
export function markIncidentResolved(incidentId: string): void {
    for (const [key, entry] of activeAutoIncidents.entries()) {
        if (entry.id === incidentId) {
            activeAutoIncidents.delete(key);
            break;
        }
    }
}

/**
 * Get current pending anomalies (for UI display).
 */
export function getPendingAnomalies(): PendingAnomaly[] {
    return [...pending.values()];
}

/**
 * Get active auto-incident count.
 */
export function getAutoIncidentCount(): number {
    return activeAutoIncidents.size;
}

/**
 * Reset all state (useful for testing or when switching profiles).
 */
export function resetBridge(): void {
    pending.clear();
    activeAutoIncidents.clear();
}

// ── Internal helpers ────────────────────────────────────────────────────────

function meetsThreshold(severity: AnomalySeverity): boolean {
    if (config.minSeverity === 'extreme') return severity === 'extreme';
    // 'strong' threshold means both 'strong' and 'extreme' qualify
    return severity === 'strong' || severity === 'extreme';
}

function buildDescription(entry: PendingAnomaly, hostName: string): string {
    const metricLabel = { cpu: 'CPU usage', ram: 'Memory usage', disk: 'Disk usage' }[entry.metric];
    const durationSec = Math.round((entry.lastSeen - entry.firstSeen) / 1000);
    return [
        `Auto-detected ${entry.severity} anomaly on ${hostName}.`,
        `Metric: ${metricLabel}`,
        `Deviation: ${Math.abs(entry.sigma).toFixed(1)}σ from recent average`,
        `Duration: ${durationSec}s sustained`,
        `Signal: ${entry.message}`,
        '',
        'Investigation should determine if this is a legitimate load spike,',
        'a runaway process, or the beginning of a capacity issue.',
    ].join('\n');
}
