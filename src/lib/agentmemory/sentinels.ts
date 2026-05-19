// ── sentinels.ts — Watchdog rules for proactive alerts ──────────────────
//
// A sentinel is a user-defined (or auto-generated) rule that watches a
// condition and fires an OS notification when triggered.  Think of them
// as lightweight "if this then notify" guards that run on each system
// health poll, memory save, or capacity check.
//
// Storage: localStorage (sentinel rules are small, per-user, and don't
// need cross-device sync). The sentinel engine runs client-side.
//
// Integration:
//   • anomaly-bridge.ts — existing anomaly detection feeds sentinel checks
//   • capacity.ts       — days-until-full projection triggers
//   • diagnostics.ts    — health check results feed sentinel checks
//   • notify.ts         — OS-level toast notifications

import { sendNotification } from '$lib/notify';
import { safeParseLS, safeSetLS } from '$lib/safe-ls';

// ── Types ────────────────────────────────────────────────────────────────

export type SentinelMetric =
    | 'cpu_percent'
    | 'ram_percent'
    | 'disk_percent'
    | 'disk_days_until_full'
    | 'ram_days_until_critical'
    | 'error_count'
    | 'anomaly_sigma'
    | 'diag_status'         // 0=ok, 1=warning, 2=error
    | 'lesson_count'
    | 'custom';

export type SentinelOp = 'gt' | 'lt' | 'eq' | 'gte' | 'lte' | 'ne';

export interface SentinelRule {
    id: string;
    name: string;
    enabled: boolean;
    metric: SentinelMetric;
    op: SentinelOp;
    threshold: number;
    /** Custom metric key (only used when metric === 'custom') */
    customKey?: string;
    /** Cooldown in minutes — don't re-fire within this window */
    cooldownMin: number;
    /** Last time this rule fired (epoch ms), 0 = never */
    lastFired: number;
    /** How many times this rule has fired total */
    fireCount: number;
    /** Optional: host filter. Empty = all hosts. */
    hostFilter: string;
    createdAt: number;
}

export interface SentinelCheckResult {
    ruleId: string;
    ruleName: string;
    fired: boolean;
    currentValue: number;
    threshold: number;
    message: string;
}

// ── Storage ──────────────────────────────────────────────────────────────

const LS_KEY = 'lucy_sentinels';

export function loadSentinels(): SentinelRule[] {
    return safeParseLS(LS_KEY, [] as SentinelRule[]);
}

export function saveSentinels(rules: SentinelRule[]): void {
    safeSetLS(LS_KEY, rules);
}

export function addSentinel(rule: Omit<SentinelRule, 'id' | 'lastFired' | 'fireCount' | 'createdAt'>): SentinelRule {
    const rules = loadSentinels();
    const newRule: SentinelRule = {
        ...rule,
        id: `sentinel_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
        lastFired: 0,
        fireCount: 0,
        createdAt: Date.now(),
    };
    rules.push(newRule);
    saveSentinels(rules);
    return newRule;
}

export function updateSentinel(id: string, updates: Partial<SentinelRule>): void {
    const rules = loadSentinels();
    const idx = rules.findIndex(r => r.id === id);
    if (idx >= 0) {
        rules[idx] = { ...rules[idx], ...updates };
        saveSentinels(rules);
    }
}

export function deleteSentinel(id: string): void {
    const rules = loadSentinels().filter(r => r.id !== id);
    saveSentinels(rules);
}

export function toggleSentinel(id: string): void {
    const rules = loadSentinels();
    const rule = rules.find(r => r.id === id);
    if (rule) {
        rule.enabled = !rule.enabled;
        saveSentinels(rules);
    }
}

// ── Evaluation Engine ────────────────────────────────────────────────────

/**
 * Evaluate a single rule against a metric value.
 * Returns true if the condition is met (rule should fire).
 */
function evalCondition(value: number, op: SentinelOp, threshold: number): boolean {
    switch (op) {
        case 'gt':  return value > threshold;
        case 'lt':  return value < threshold;
        case 'eq':  return value === threshold;
        case 'gte': return value >= threshold;
        case 'lte': return value <= threshold;
        case 'ne':  return value !== threshold;
    }
}

/**
 * Check all enabled sentinels against a set of current metric values.
 * Fires OS notifications for triggered rules (respecting cooldown).
 *
 * @param metrics — map of metric name → current numeric value
 * @param hostName — optional host context for filtering
 * @returns array of check results (only for enabled rules)
 */
export async function checkSentinels(
    metrics: Partial<Record<SentinelMetric, number>> & Record<string, number>,
    hostName?: string,
): Promise<SentinelCheckResult[]> {
    const rules = loadSentinels().filter(r => r.enabled);
    const results: SentinelCheckResult[] = [];
    const now = Date.now();
    let needsSave = false;

    for (const rule of rules) {
        // Host filter
        if (rule.hostFilter && hostName && !hostName.toLowerCase().includes(rule.hostFilter.toLowerCase())) {
            continue;
        }

        // Get metric value
        const key = rule.metric === 'custom' ? (rule.customKey || '') : rule.metric;
        const value = metrics[key];
        if (value === undefined || value === null) continue;

        const fired = evalCondition(value, rule.op, rule.threshold);
        const result: SentinelCheckResult = {
            ruleId: rule.id,
            ruleName: rule.name,
            fired,
            currentValue: value,
            threshold: rule.threshold,
            message: fired
                ? `${rule.name}: ${key} is ${value} (${opLabel(rule.op)} ${rule.threshold})`
                : '',
        };
        results.push(result);

        if (fired) {
            // Cooldown check
            const cooldownMs = rule.cooldownMin * 60_000;
            if (rule.lastFired > 0 && (now - rule.lastFired) < cooldownMs) {
                continue;  // still in cooldown
            }

            // Fire notification
            try {
                await sendNotification(
                    `⚠ Sentinel: ${rule.name}`,
                    `${key} = ${formatValue(value, rule.metric)} (threshold: ${opLabel(rule.op)} ${rule.threshold})`
                );
            } catch { /* notification permission denied or unavailable */ }

            // Update fire tracking
            rule.lastFired = now;
            rule.fireCount++;
            needsSave = true;
        }
    }

    if (needsSave) saveSentinels(rules);
    return results;
}

// ── Built-in Sentinel Templates ──────────────────────────────────────────

export interface SentinelTemplate {
    name: string;
    nameEN: string;
    metric: SentinelMetric;
    op: SentinelOp;
    threshold: number;
    cooldownMin: number;
}

export const BUILTIN_TEMPLATES: SentinelTemplate[] = [
    { name: 'CPU alto (>90%)',           nameEN: 'High CPU (>90%)',            metric: 'cpu_percent',             op: 'gt',  threshold: 90,  cooldownMin: 15 },
    { name: 'RAM alta (>85%)',           nameEN: 'High RAM (>85%)',            metric: 'ram_percent',             op: 'gt',  threshold: 85,  cooldownMin: 15 },
    { name: 'Disco casi lleno (>90%)',   nameEN: 'Disk almost full (>90%)',    metric: 'disk_percent',            op: 'gt',  threshold: 90,  cooldownMin: 60 },
    { name: 'Disco <7 días para lleno',  nameEN: 'Disk <7 days until full',   metric: 'disk_days_until_full',    op: 'lt',  threshold: 7,   cooldownMin: 360 },
    { name: 'Anomalía alta (>3σ)',       nameEN: 'High anomaly (>3σ)',        metric: 'anomaly_sigma',           op: 'gt',  threshold: 3,   cooldownMin: 30 },
    { name: 'Diagnóstico con errores',   nameEN: 'Diagnostics errors',        metric: 'diag_status',             op: 'gte', threshold: 2,   cooldownMin: 60 },
];

// ── Helpers ──────────────────────────────────────────────────────────────

function opLabel(op: SentinelOp): string {
    const labels: Record<SentinelOp, string> = {
        gt: '>', lt: '<', eq: '=', gte: '≥', lte: '≤', ne: '≠',
    };
    return labels[op] || op;
}

function formatValue(value: number, metric: SentinelMetric): string {
    if (metric.includes('percent')) return `${value.toFixed(1)}%`;
    if (metric.includes('days'))    return `${value.toFixed(0)} days`;
    if (metric === 'anomaly_sigma') return `${value.toFixed(2)}σ`;
    return String(value);
}

export function opOptions(): { value: SentinelOp; label: string }[] {
    return [
        { value: 'gt',  label: '>' },
        { value: 'lt',  label: '<' },
        { value: 'eq',  label: '=' },
        { value: 'gte', label: '≥' },
        { value: 'lte', label: '≤' },
        { value: 'ne',  label: '≠' },
    ];
}
