// ── log-analysis.ts — Log Pattern Analysis API wrapper (P0 Feature 2) ────────
//
// Frontend interface for structured log parsing, error clustering, and
// frequency analysis. Wraps the analyze_log_patterns Tauri command.

import { invoke } from '@tauri-apps/api/core';

export interface ErrorCluster {
    pattern: string;
    count: number;
    first_seen: string;
    last_seen: string;
    sample: string;
    level: string;
}

export interface TimelineBucket {
    timestamp: string;
    error_count: number;
    warning_count: number;
    total_count: number;
}

export interface LogAnalysisResult {
    total_lines: number;
    parsed_lines: number;
    levels: Record<string, number>;
    error_clusters: ErrorCluster[];
    timeline: TimelineBucket[];
    top_sources: [string, number][];
    format_detected: string;
}

/**
 * Analyze an array of log lines for patterns, clusters, and timeline.
 * @param lines Array of log lines (from read_log_tail or read_remote_log_*)
 * @param bucketMinutes Timeline bucket size in minutes. Default: 60 (hourly).
 */
export async function analyzeLogPatterns(
    lines: string[],
    bucketMinutes: number = 60
): Promise<LogAnalysisResult> {
    return invoke<LogAnalysisResult>('analyze_log_patterns', {
        lines,
        bucketMinutes,
    });
}

/**
 * Get a CSS color for a log level.
 */
export function levelColor(level: string): string {
    switch (level.toUpperCase()) {
        case 'CRITICAL':
        case 'FATAL':
        case 'PANIC':
            return 'text-red-500';
        case 'ERROR':
            return 'text-red-400';
        case 'WARNING':
        case 'WARN':
            return 'text-amber-400';
        case 'INFO':
        case 'NOTICE':
            return 'text-blue-400';
        case 'DEBUG':
        case 'TRACE':
        case 'VERBOSE':
            return 'text-gray-500';
        default:
            return 'text-gray-400';
    }
}

/**
 * Format a level count map into a sorted summary string.
 */
export function formatLevelSummary(levels: Record<string, number>): string {
    const order = ['CRITICAL', 'FATAL', 'ERROR', 'WARNING', 'WARN', 'INFO', 'DEBUG', 'TRACE'];
    return order
        .filter(l => levels[l] && levels[l] > 0)
        .map(l => `${l}: ${levels[l]}`)
        .join(' | ');
}
