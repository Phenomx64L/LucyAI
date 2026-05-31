// ── lucy-api.ts — Typed wrappers for all Tauri invoke() calls ─────────────────
// Import from here instead of calling invoke() directly in components.

import { invoke } from '@tauri-apps/api/core';

// ── TYPES ─────────────────────────────────────────────────────────────────────

export interface DiskInfo {
    name: string;
    mount: string;
    total_gb: number;
    used_gb: number;
    free_gb: number;
    percent: number;
}

export interface ProcessInfo {
    name: string;
    cpu: number;
    mem_mb: number;
    pid?: number;
}

export interface CpuInfo {
    cores: number;
    global: number;
    per_core: number[];
}

export interface MemoryInfo {
    total_mb: number;
    used_mb: number;
    percent: number;
}

export interface SystemHealth {
    hostname: string;
    os: string;
    uptime_h: number;
    timestamp: string;
    cpu: CpuInfo;
    memory: MemoryInfo;
    disks: DiskInfo[];
    top_processes: ProcessInfo[];
}

export interface FilePayload {
    name: string;
    content: string;
    mime: string;
}

// ── RETRY UTILITY ─────────────────────────────────────────────────────────────

async function invokeWithRetry<T>(
    cmd: string,
    args?: Record<string, unknown>,
    retries = 2,
    delayMs = 500,
): Promise<T> {
    for (let i = 0; i <= retries; i++) {
        try {
            return await invoke<T>(cmd, args);
        } catch (err) {
            if (i === retries) throw err;
            await new Promise(r => setTimeout(r, delayMs));
        }
    }
    throw new Error('unreachable');
}

// ── AI ────────────────────────────────────────────────────────────────────────

export function askLucy(args: {
    prompt: string;
    context?: string;
    userName: string;
    model: string;
    images?: Array<{ mimeType: string; data: string }>;
    lang?: string;
    hostsJson?: string;
    runbooksDir?: string | null;
}): Promise<string> {
    return invoke('ask_lucy', {
        prompt: args.prompt,
        context: args.context ?? null,
        userName: args.userName,
        model: args.model,
        images: args.images ?? null,
        lang: args.lang ?? null,
        hostsJson: args.hostsJson ?? null,
        runbooksDir: args.runbooksDir ?? null,
    });
}

export function askLucyStream(args: {
    requestId: string;
    prompt: string;
    context?: string;
    userName: string;
    model: string;
    images?: Array<{ mimeType: string; data: string }>;
    lang?: string;
    hostsJson?: string;
    runbooksDir?: string | null;
}, window: unknown): Promise<string> {
    return invoke('ask_lucy_stream', {
        window,
        requestId: args.requestId,
        prompt: args.prompt,
        context: args.context ?? null,
        userName: args.userName,
        model: args.model,
        images: args.images ?? null,
        lang: args.lang ?? null,
        hostsJson: args.hostsJson ?? null,
        runbooksDir: args.runbooksDir ?? null,
    });
}

// ── CONFIG / CREDENTIALS ──────────────────────────────────────────────────────

export const saveLlmKey         = (provider: string, apiKey: string): Promise<void> => invoke('save_llm_key', { provider, apiKey });
export const getConfiguredProviders = (): Promise<string[]>                           => invoke('get_configured_providers');
export const testApiKey         = (provider: string, apiKey: string): Promise<void> => invoke('test_api_key', { provider, apiKey });

export const saveHostCredential = (hostId: string, password: string): Promise<void> =>
    invoke('save_host_credential', { hostId, password });
export const getHostCredential  = (hostId: string): Promise<string>  =>
    invoke('get_host_credential', { hostId });
export const deleteHostCredential = (hostId: string): Promise<void>  =>
    invoke('delete_host_credential', { hostId });

// ── UI / WINDOW ───────────────────────────────────────────────────────────────

export const copyToClipboard = (text: string): Promise<void>   => invoke('copy_to_clipboard', { text });
export const minimizeWindow  = (): Promise<void>               => invoke('minimize_window');
export const maximizeWindow  = (): Promise<void>               => invoke('maximize_window');
export const closeWindow     = (): Promise<void>               => invoke('close_window');
export const openShellWindow = (hostId: string, hostName: string): Promise<void> =>
    invoke('open_shell_window', { hostId, hostName });

export function pickAndReadFile(): Promise<[string, string, string] | null> {
    return invoke('pick_and_read_file');
}
export function pickMultipleFiles(): Promise<Array<[string, string, string]>> {
    return invoke('pick_multiple_files');
}
export const pickFilePath = (): Promise<string> => invoke('pick_file_path');

export const saveFileDialog = (defaultName: string, dataB64: string, filterName: string, extensions: string[]): Promise<string> =>
    invoke('save_file_dialog', { defaultName, dataB64: dataB64, filterName, extensions });

// ── SYSTEM (LOCAL) ────────────────────────────────────────────────────────────

export const getSystemHealth     = (): Promise<string>       => invoke('get_system_health');
export const getSystemHealthJson = (): Promise<SystemHealth> =>
    invokeWithRetry<SystemHealth>('get_system_health_json', {}, 1);

// ── HOSTS (REMOTE) ────────────────────────────────────────────────────────────

export const executeRemoteWindows = (args: {
    host: string; username: string; password: string; command: string;
}): Promise<string> => invoke('execute_remote_windows', args);

export const getRemoteHealthWindows = (args: {
    host: string; username: string; password: string;
}): Promise<SystemHealth> => invokeWithRetry<SystemHealth>('get_remote_health_windows', args, 1);

export const executeRemoteLinux = (args: {
    host: string; username: string; command: string; port?: number; keyPath?: string;
}): Promise<string> => invoke('execute_remote_linux', args);

export const getRemoteHealthLinux = (args: {
    host: string; username: string; port?: number; keyPath?: string;
}): Promise<SystemHealth> => invokeWithRetry<SystemHealth>('get_remote_health_linux', args, 1);

export const executeShellCmd = (args: {
    host: string; username: string; command: string; hostType: string;
    port?: number; password?: string; keyPath?: string;
}): Promise<string> => invoke('execute_shell_cmd', args);

// ── SHELL LOCAL ───────────────────────────────────────────────────────────────

// v1.5.0 — signatures cleaned up. The legacy forceExecute/forceWrite
// boolean parameters are gone; use bypassToken (cryptographic one-shot
// issued by a previous SECURITY_BLOCK response) for the approval path.
export const executePowershell = (script: string, bypassToken: string | null = null): Promise<string> =>
    invoke('execute_powershell', { script, bypassToken });

// ── SHELL STREAMING ───────────────────────────────────────────────────────────

export const streamShellCmd = (args: {
    sessionId: string; host: string; username: string; command: string;
    hostType: string; port?: number; password?: string; keyPath?: string;
}): Promise<void> => invoke('stream_shell_cmd', args);

export const sendShellInput  = (sessionId: string, input: string): Promise<void> =>
    invoke('send_shell_input', { sessionId, input });
export const killShellSession = (sessionId: string): Promise<void> =>
    invoke('kill_shell_session', { sessionId });

// ── LOCAL EXECUTION (CMD, WMIC, netsh, reg, cscript, native) ─────────────────

export interface NetConnection {
    protocol: string;
    localAddr: string;
    localPort: number;
    remoteAddr: string;
    remotePort: number;
    state: string;
    pid: number | null;
}

export interface EventEntry {
    time: string;
    level: string;
    source: string;
    eventId: string;
    message: string;
}

export interface TaskEntry {
    name: string;
    pid: number;
    session: string;
    memKb: number;
}

export const executeCmd      = (script: string, bypassToken: string | null = null): Promise<string> =>
    invoke('execute_cmd', { script, bypassToken });

export const executeWmic     = (query: string): Promise<string> =>
    invoke('execute_wmic', { query });

export const executeNetsh    = (args: string): Promise<string> =>
    invoke('execute_netsh', { args });

export const executeReg      = (args: string, bypassToken: string | null = null): Promise<string> =>
    invoke('execute_reg', { args, bypassToken });

export const executeCscript  = (scriptContent: string, bypassToken: string | null = null): Promise<string> =>
    invoke('execute_cscript', { scriptContent, bypassToken });

export const getNetworkConnections = (): Promise<NetConnection[]> =>
    invoke('get_network_connections');

export const getEventLog     = (logName: string, count: number, level?: string): Promise<EventEntry[]> =>
    invoke('get_event_log', { logName, count, level: level ?? null });

export const getTasklist     = (): Promise<TaskEntry[]> =>
    invoke('get_tasklist');

export const readRegistryValue = (hive: string, keyPath: string, valueName: string): Promise<string> =>
    invoke('read_registry_value', { hive, keyPath, valueName });

export const listRegistryKey = (hive: string, keyPath: string): Promise<{ subkeys: string[]; values: Array<{name: string; value: string}> }> =>
    invoke('list_registry_key', { hive, keyPath });

// ── LOGS ──────────────────────────────────────────────────────────────────────

export const readLogTail = (path: string, lines: number): Promise<string[]> =>
    invoke('read_log_tail', { path, lines });

export const readRemoteLogWindows = (args: {
    host: string; username: string; password: string; path: string; lines: number;
}): Promise<string[]> => invoke('read_remote_log_windows', args);

export const readRemoteLogLinux = (args: {
    host: string; username: string; path: string; lines: number; port?: number; keyPath?: string;
}): Promise<string[]> => invoke('read_remote_log_linux', args);

// ── INVENTORY ────────────────────────────────────────────────────────────────

export const discoverInventoryLocal = (): Promise<Record<string, unknown>> =>
    invoke('discover_inventory_local');

export const discoverInventoryLinux = (args: {
    host: string; username: string; port?: number; keyPath?: string;
}): Promise<Record<string, unknown>> => invoke('discover_inventory_linux', args);

export const discoverInventoryWindows = (args: {
    host: string; username: string; password: string;
}): Promise<Record<string, unknown>> => invoke('discover_inventory_windows', args);

// ── COMPLIANCE ───────────────────────────────────────────────────────────────

export const runComplianceLocal = (checksJson: string): Promise<unknown[]> =>
    invoke('run_compliance_local', { checksJson });

export const runComplianceLinux = (args: {
    host: string; username: string; checksJson: string; port?: number; keyPath?: string;
}): Promise<unknown[]> => invoke('run_compliance_linux', args);

export const runComplianceWindows = (args: {
    host: string; username: string; password: string; checksJson: string;
}): Promise<unknown[]> => invoke('run_compliance_windows', args);

// ── COST TRACKING & METRICS (Nivel 2) ─────────────────────────────────────

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

export interface TokenUsage {
    id: string;
    task_id: string;
    timestamp: string;
    model: string;
    input_tokens: number;
    output_tokens: number;
    total_cost: number;
    user: string;
    request_type: string;
}

export const initMetricsDb = (): Promise<void> =>
    invoke('init_metrics_db');

export const getCostSummary = (period: 'day' | 'month' | 'all'): Promise<CostSummary> =>
    invoke('get_cost_summary', { period });

export const getTokenHistory = (limit: number): Promise<TokenUsage[]> =>
    invoke('get_token_history', { limit });
