// ── stores.ts — Svelte writable stores for shared app state ───────────────────
// Import from here to share state between components/pages.

import { writable, derived, get } from 'svelte/store';
import type { SystemHealth } from './lucy-api';

// ── TYPES ─────────────────────────────────────────────────────────────────────

export type HostCategory = 'shell' | 'database' | 'container' | 'kubernetes' | 'network';
export type DbType = 'postgres' | 'mysql' | 'mongodb' | 'redis' | 'mssql';

export interface Host {
    id: string;
    name: string;
    host: string;
    type: 'windows' | 'linux';
    username: string;
    port?: number;
    sshKeyPath?: string;
    tags?: string[];
    color?: string;
    category?: HostCategory;   // organizational category
    dbType?: DbType;           // only when category='database'
    lastActivity?: number;     // timestamp of last successful connection
}

export interface AlertRule {
    id: string;
    hostId: string;   // 'all' or specific host id
    metric: 'cpu' | 'ram' | 'disk';
    threshold: number;
    enabled: boolean;
}

export interface RunbookStep {
    label: string;
    cmd: string;
}

export interface Runbook {
    id: string;
    name: string;
    icon: string;
    steps: RunbookStep[];
}

export interface MetricsSample {
    ts: number;
    cpu: number;
    ram: number;
    disk: number;
}

// ── PERSISTENCE HELPERS ───────────────────────────────────────────────────────

/**
 * writable store con persistencia automática en localStorage.
 * Maneja transparentemente el formato versionado { version: N, data: [...] }
 * que escribe el código legacy de +page.svelte, de forma que la migración
 * a este store no rompa datos existentes.
 * Al escribir siempre usa el formato moderno (array/objeto plano).
 *
 * F11 audit (May 2026): writes are debounced (200ms). The previous
 * synchronous `localStorage.setItem` on EVERY store update was a real
 * bottleneck — for `auditTrail` (capped at ~1000 entries × ~500 bytes
 * ≈ 500KB JSON), each shell command serialized + wrote to disk-backed
 * storage synchronously, blocking the main thread. With a debounce we
 * coalesce bursts (a single agent turn that emits 10 audit entries
 * now triggers ONE write 200ms after the last update) without losing
 * data on a normal app close — the trailing edge fires reliably.
 *
 * For ABRUPT app crashes within the 200ms window, the last few entries
 * may be lost. That's an acceptable tradeoff for the perf gain; if
 * full crash safety is needed for a specific key, the caller can call
 * `localStorage.setItem` directly after critical writes.
 */
const PERSIST_DEBOUNCE_MS = 200;
function persistedWritable<T>(key: string, initial: T) {
    let stored: T = initial;
    try {
        const raw = localStorage.getItem(key);
        if (raw) {
            const parsed = JSON.parse(raw);
            // Desenvuelve el wrapper { version, data } del código legacy
            stored = (parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed) && 'data' in parsed)
                ? (parsed.data as T)
                : (parsed as T);
        }
    } catch { /* usar valor inicial */ }

    const store = writable<T>(stored);

    // Debounced persistence: collapse rapid bursts of updates into one
    // write. Keeps the latest value pending until the timer fires.
    let pending: T | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let initial_call = true;

    store.subscribe(value => {
        // First subscribe fires synchronously with the initial value —
        // there's nothing new to persist, skip.
        if (initial_call) { initial_call = false; return; }
        pending = value;
        if (timer !== null) return;
        timer = setTimeout(() => {
            timer = null;
            try { localStorage.setItem(key, JSON.stringify(pending)); }
            catch { /* quota exceeded / private mode — ignore */ }
            pending = null;
        }, PERSIST_DEBOUNCE_MS);
    });

    // Flush pending writes when the window unloads so we don't lose the
    // last 200ms of activity on a normal close.
    if (typeof window !== 'undefined') {
        window.addEventListener('beforeunload', () => {
            if (timer !== null) {
                clearTimeout(timer);
                timer = null;
                try { if (pending !== null) localStorage.setItem(key, JSON.stringify(pending)); }
                catch { /* ignore */ }
            }
        });
    }

    return store;
}

// ── HOSTS ─────────────────────────────────────────────────────────────────────

export const hosts = persistedWritable<Host[]>('lucy_hosts', []);

/**
 * Carga los hosts completos (con IP, usuario, etc.) desde el Keyring de Windows
 * y popula el store. Llama a esta función al arrancar la app si el usuario ya
 * tiene configuración guardada. Si el Keyring falla, el store conserva lo que
 * haya en localStorage (datos parciales de la versión legacy).
 *
 * @param invoker - referencia al `invoke` de @tauri-apps/api/core
 */
export async function initHostsFromKeyring(invoker: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>): Promise<void> {
    try {
        const raw = await invoker('get_host_credential', { hostId: 'lucy_hosts_index' }) as string;
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed) && parsed.length > 0) {
            hosts.set(parsed as Host[]);
        }
    } catch {
        // Si no hay entrada en Keyring (instalación nueva o limpia), el store
        // ya tiene los datos de localStorage — nada que hacer.
    }
}

/** Active tag filter — null means "show all hosts". */
export const hostTagFilter = writable<string | null>(null);

export const hostsFiltered = derived(
    [hosts, hostTagFilter],
    ([$hosts, $filter]) => $filter
        ? $hosts.filter(h => (h.tags ?? []).includes($filter))
        : $hosts
);

export const allTags = derived(hosts, $hosts =>
    [...new Set($hosts.flatMap(h => h.tags ?? []))]
);

// ── METRICS HISTORY ───────────────────────────────────────────────────────────

const METRICS_HIST_MAX = 20;

export const metricsHistory = persistedWritable<Record<string, MetricsSample[]>>(
    'lucy_metrics_history',
    {}
);

export function pushMetricsSample(hostId: string, sample: MetricsSample): void {
    metricsHistory.update(h => {
        const arr = h[hostId] ?? [];
        arr.push(sample);
        if (arr.length > METRICS_HIST_MAX) arr.splice(0, arr.length - METRICS_HIST_MAX);
        return { ...h, [hostId]: arr };
    });
}

// ── SYSTEM HEALTH (dashboard data) ───────────────────────────────────────────

export const localHealth   = writable<SystemHealth | null>(null);
export const remoteHealths = writable<Record<string, SystemHealth>>({});

/**
 * Per-host reachability tracker. The PostureStrip uses this to decide whether
 * to render a green (online), red (offline) or muted-gray (never-pinged) dot.
 * Updated by the background poller in +page.svelte every ~15s.
 *
 *   - `ts`        : last attempt timestamp
 *   - `reachable` : true if the last attempt succeeded, false if it errored,
 *                   undefined if we've never tried
 */
export interface HostReachability {
    ts: number;
    reachable: boolean;
}
export const hostReachability = writable<Record<string, HostReachability>>({});

/** Mark a host as reachable=true after a successful health/metric fetch. */
export function markHostReachable(hostId: string, ok: boolean): void {
    hostReachability.update(r => ({ ...r, [hostId]: { ts: Date.now(), reachable: ok } }));
}

// ── ALERT RULES ───────────────────────────────────────────────────────────────

export const alertRules   = persistedWritable<AlertRule[]>('lucy_alert_rules', []);
/**
 * Threshold alerts currently firing.
 *
 * Was `writable<string[]>` and never written by anyone. DashboardView kept its
 * own component-local `activeAlerts` holding objects, so the bell badge worked
 * — while the "Disparadas ahora" section of the Alertas Proactivas modal in
 * +page.svelte reads THIS store and was therefore permanently empty. Two
 * sources of truth for "which alerts are firing", one of them wired to the UI
 * and never populated.
 *
 * The `string[]` annotation was the tell: every consumer reached for `.metric`,
 * `.hostLabel`, `.value` and `.threshold` on its elements. DashboardView now
 * publishes here instead of keeping a private copy.
 */
export interface ActiveAlert {
    id: string;
    ruleId: string;
    hostId: string;
    hostLabel: string;
    /** Already uppercased at construction ('CPU' | 'RAM' | 'DISK'). */
    metric: string;
    value: number;
    threshold: number;
    /** Locale time string, for display only. */
    ts: string;
}
export const activeAlerts = writable<ActiveAlert[]>([]);

// ── RUNBOOKS ──────────────────────────────────────────────────────────────────

export const runbooks = persistedWritable<Runbook[]>('lucy_runbooks', []);

// ── UI STATE ──────────────────────────────────────────────────────────────────

export const showAlertsModal    = writable(false);
export const showRunbookModal   = writable(false);
export const showMultiHostModal = writable(false);
export const showAboutModal     = writable(false);
export const showChangeKeyModal = writable(false);
export const showNewActionModal = writable(false);
export const showMemoryModal    = writable(false);
export const showChipsModal     = writable(false);
export const showLearnConfirm   = writable(false);
export const showRunAsModal     = writable(false);
export const showHistoryModal   = writable(false);
export const showCloseTabModal  = writable(false);

export const multiHostSelected = writable<string[]>([]);
export const multiHostCmd      = writable('');
export const multiHostResults  = writable<Record<string, { status: 'running' | 'done' | 'error'; output: string }>>({});
export const multiHostRunning  = writable(false);

// ── INVENTORY ────────────────────────────────────────────────────────────────

export interface InventorySnapshot {
    hostId: string;
    hostName: string;
    timestamp: number;
    ports:     Array<{ port: number; process: string; state: string }>;
    services:  Array<{ name: string; status: string; description: string }>;
    software:  Array<{ name: string; version: string }>;
    certs:     Array<{ path: string; subject: string; expires: string; days_left: number }>;
    scheduled: Array<{ entry: string }>;
}

export const inventorySnapshots = persistedWritable<Record<string, InventorySnapshot>>(
    'lucy_inventory', {}
);

// ── COMPLIANCE ───────────────────────────────────────────────────────────────

export interface CISCheck {
    id: string;
    title: string;
    category: string;
    severity: 'critical' | 'high' | 'medium' | 'low';
    command: string;
    expect: { stdout_contains?: string; stdout_not_contains?: string; exit_code?: number; regex?: string };
    remediation: string;
}

export interface ComplianceResult {
    checkId: string;
    passed: boolean;
    stdout: string;
    exitCode: number;
}

export interface ComplianceScanReport {
    hostId: string;
    hostName: string;
    timestamp: number;
    osType: 'linux' | 'windows';
    results: ComplianceResult[];
    passRate: number;
}

export const complianceReports = persistedWritable<Record<string, ComplianceScanReport>>(
    'lucy_compliance', {}
);

// ── PROFILES (Multi-tenant) ──────────────────────────────────────────────────

export interface Profile {
    id: string;
    name: string;
    icon: string;
    hostIds: string[];
    createdAt: number;
    lastUsed: number;
}

export const profiles        = persistedWritable<Profile[]>('lucy_profiles', []);
export const activeProfileId = persistedWritable<string | null>('lucy_active_profile', null);

export const activeProfileHosts = derived(
    [hosts, activeProfileId, profiles],
    ([$hosts, $profileId, $profiles]) => {
        if (!$profileId) return $hosts;
        const profile = $profiles.find(p => p.id === $profileId);
        if (!profile) return $hosts;
        return $hosts.filter(h => profile.hostIds.includes(h.id));
    }
);

// ── AUDIT TRAIL ──────────────────────────────────────────────────────────────

export interface AuditEntry {
    id?: number;
    timestamp: string;
    hostId: string;
    hostName: string;
    command: string;
    source: 'manual' | 'runbook' | 'compliance' | 'ai' | 'broadcast';
    exitCode: number | null;
    durationMs: number | null;
    outputPreview: string;
    user: string;
}

export const auditTrail = persistedWritable<AuditEntry[]>('lucy_audit_trail', []);

// ── COMMAND GUARD (Pre-execution hooks) ─────────────────────────────────────

export interface GuardConfig {
    enabled: boolean;
    /** Minimum risk level that triggers confirmation: 'medium' | 'high' | 'critical' */
    threshold: 'medium' | 'high' | 'critical';
    /** Also intercept AI-generated commands */
    interceptAI: boolean;
    /** Also intercept broadcast commands */
    interceptBroadcast: boolean;
}

export const guardConfig = persistedWritable<GuardConfig>('lucy_guard_config', {
    enabled: true,
    threshold: 'high',
    interceptAI: true,
    interceptBroadcast: true,
});

// ── COST TRACKING (Nivel 2) ──────────────────────────────────────────────────

export interface ModelCostBreakdown {
    model: string;
    cost: number;
    tokens: number;
    requests: number;
}

export interface CostSummary {
    total_cost: number;
    total_tokens: number;
    request_count: number;
    per_model: ModelCostBreakdown[];
    period: 'day' | 'month' | 'all';
}

export interface TokenBudgetConfig {
    monthlyLimit: number; // USD
    alertThreshold: number; // percentage (e.g., 80 = alert at 80% of limit)
    enabled: boolean;
}

export const costSummaryDay = writable<CostSummary | null>(null);
export const costSummaryMonth = writable<CostSummary | null>(null);
export const costSummaryAll = writable<CostSummary | null>(null);
export const tokenBudgetConfig = persistedWritable<TokenBudgetConfig>('lucy_token_budget', {
    monthlyLimit: 10.0,
    alertThreshold: 80,
    enabled: true,
});
