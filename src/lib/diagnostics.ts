// ── diagnostics.ts — Self-Diagnostics API wrapper (P0 Feature 5) ─────────────
//
// Frontend interface for the unified self-diagnostics panel.
// Calls the backend run_self_diagnostics command and formats results.

import { invoke } from '@tauri-apps/api/core';

export interface DiagnosticCheck {
    name: string;
    category: string;   // 'system' | 'database' | 'ai' | 'memory' | 'security' | 'network'
    status: string;     // 'ok' | 'warning' | 'error' | 'unknown'
    message: string;
    details: Record<string, unknown> | null;
    elapsed_ms: number;
}

export interface DiagnosticsReport {
    checks: DiagnosticCheck[];
    overall_status: string;
    total_elapsed_ms: number;
    timestamp: number;
}

/**
 * Run all self-diagnostic checks. Returns a unified report with
 * status per check and overall status (worst of all checks).
 */
export async function runSelfDiagnostics(): Promise<DiagnosticsReport> {
    return invoke<DiagnosticsReport>('run_self_diagnostics');
}

/**
 * Get a CSS color class for a diagnostic status.
 */
export function statusColor(status: string): string {
    switch (status) {
        case 'ok': return 'text-emerald-400';
        case 'warning': return 'text-amber-400';
        case 'error': return 'text-red-400';
        default: return 'text-gray-400';
    }
}

/**
 * Get a status icon character for a diagnostic status.
 */
export function statusIcon(status: string): string {
    switch (status) {
        case 'ok': return '✓';       // checkmark
        case 'warning': return '⚠';  // warning triangle
        case 'error': return '✗';    // X mark
        default: return '?';
    }
}

/**
 * Get a human-friendly category label.
 */
export function categoryLabel(category: string): string {
    const labels: Record<string, string> = {
        system: 'SYSTEM',
        database: 'DATABASE',
        ai: 'AI PROVIDERS',
        memory: 'MEMORY',
        security: 'SECURITY',
        network: 'NETWORK',
    };
    return labels[category] || category.toUpperCase();
}
