// ── capacity.ts — Capacity Planning API wrapper (P0 Feature 3) ───────────────
//
// Frontend interface for the capacity planning backend. Provides:
//   - Automatic metrics sampling (call from dashboard polling loop)
//   - Historical trend queries for charting (7/30/90 day views)
//   - "Days until full" projection data

import { invoke } from '@tauri-apps/api/core';

export interface TrendPoint {
    ts: number;
    cpu: number;
    ram: number;
    disk: number;
    ram_mb: number;
    disk_gb: number;
}

export interface TrendStats {
    avg_cpu: number;
    avg_ram: number;
    avg_disk: number;
    max_cpu: number;
    max_ram: number;
    max_disk: number;
    sample_count: number;
    span_hours: number;
}

export interface CapacityTrend {
    samples: TrendPoint[];
    disk_days_until_full: number | null;
    ram_days_until_critical: number | null;
    current: TrendPoint | null;
    stats: TrendStats;
}

/**
 * Save a metrics sample. If no values provided, auto-samples local system.
 * Call this from the dashboard polling interval (every 5 min).
 */
export async function saveMetricsSample(data?: {
    hostId?: string;
    cpu?: number;
    ram?: number;
    disk?: number;
    ramMb?: number;
    diskGb?: number;
    diskTotalGb?: number;
    ramTotalMb?: number;
}): Promise<number> {
    return invoke<number>('save_metrics_sample', {
        hostId: data?.hostId || null,
        cpu: data?.cpu ?? null,
        ram: data?.ram ?? null,
        disk: data?.disk ?? null,
        ramMb: data?.ramMb ?? null,
        diskGb: data?.diskGb ?? null,
        diskTotalGb: data?.diskTotalGb ?? null,
        ramTotalMb: data?.ramTotalMb ?? null,
    });
}

/**
 * Get capacity trends for charting and projection.
 * @param days Number of days to look back (7, 30, 90). Default: 7.
 * @param hostId Filter by host ID. Empty string = local.
 */
export async function getCapacityTrends(
    days: number = 7,
    hostId: string = ''
): Promise<CapacityTrend> {
    return invoke<CapacityTrend>('get_capacity_trends', {
        hostId: hostId || null,
        days,
    });
}

/**
 * Trigger manual downsampling of raw metrics into hourly buckets.
 * Normally runs automatically in the backend, but can be triggered manually.
 */
export async function downsampleMetrics(): Promise<number> {
    return invoke<number>('downsample_metrics');
}

/**
 * Format "days until full" for display.
 */
export function formatDaysUntil(days: number | null): string {
    if (days === null) return 'N/A (stable or insufficient data)';
    if (days === 0) return 'Already critical!';
    if (days < 1) return `< 1 day`;
    if (days < 7) return `${days.toFixed(1)} days`;
    if (days < 30) return `${Math.round(days)} days (~${Math.round(days / 7)} weeks)`;
    if (days < 365) return `~${Math.round(days / 30)} months`;
    return '> 1 year';
}
