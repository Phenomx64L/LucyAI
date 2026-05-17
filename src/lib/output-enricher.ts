// ── output-enricher.ts — Structured output detection and parsing ─────────────
// Analyzes raw command output and determines the optimal visualization.
// Pure functions — no Svelte, no Tauri. Safe to unit-test.

// ── Output types ─────────────────────────────────────────────────────────────

export type OutputType =
    | 'process-table'
    | 'service-list'
    | 'disk-usage'
    | 'network-table'
    | 'json-tree'
    | 'key-value'
    | 'event-log'
    | 'plain';

export interface EnrichedOutput {
    type:   OutputType;
    raw:    string;
    parsed: any;
}

// ── Process table ────────────────────────────────────────────────────────────

export interface ProcessRow {
    name: string;
    pid:  number;
    cpu:  number;
    mem:  string;
    id?:  number;
}

// ── Service list ─────────────────────────────────────────────────────────────

export interface ServiceRow {
    name:        string;
    displayName: string;
    status:      'Running' | 'Stopped' | 'Paused' | string;
    startType?:  string;
}

// ── Disk usage ───────────────────────────────────────────────────────────────

export interface DiskRow {
    drive:    string;
    usedGB:   number;
    freeGB:   number;
    totalGB:  number;
    usedPct:  number;
}

// ── Network table ────────────────────────────────────────────────────────────

export interface NetRow {
    protocol:   string;
    localAddr:  string;
    localPort:  number;
    remoteAddr: string;
    remotePort: number;
    state:      string;
    pid?:       number;
}

// ── Key-value pairs ──────────────────────────────────────────────────────────

export interface KVPair {
    key:   string;
    value: string;
}

// ── Event log ────────────────────────────────────────────────────────────

export interface EventLogRow {
    level:   'Information' | 'Warning' | 'Error' | 'Critical' | string;
    time:    string;
    source:  string;
    id:      number;
    message: string;
}

// ── Main enrichment function ─────────────────────────────────────────────────

/**
 * Detect output structure based on the command that produced it.
 * Returns typed parsed data + the optimal widget type.
 */
export function enrichOutput(command: string, output: string): EnrichedOutput {
    if (!output || !output.trim()) {
        return { type: 'plain', raw: output, parsed: null };
    }

    const cmd = (command || '').toLowerCase();
    const trimmed = output.trim();

    // 1. JSON detection (highest priority — unambiguous)
    if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
        const jsonParsed = tryParseJson(trimmed);
        if (jsonParsed !== undefined) {
            return { type: 'json-tree', raw: output, parsed: jsonParsed };
        }
    }

    // 2. Get-Service → service list
    if (/get-service/i.test(cmd) || detectServiceFormat(trimmed)) {
        const services = parseServiceList(trimmed);
        if (services.length >= 2) {
            return { type: 'service-list', raw: output, parsed: services };
        }
    }

    // 3. Get-Process / tasklist → process table
    if (/get-process|tasklist|top\s/i.test(cmd) || detectProcessFormat(trimmed)) {
        const procs = parseProcessTable(trimmed);
        if (procs.length >= 2) {
            return { type: 'process-table', raw: output, parsed: procs };
        }
    }

    // 4. Get-PSDrive / df → disk usage
    if (/get-psdrive|df\s|wmic.*diskdrive|get-volume/i.test(cmd) || detectDiskFormat(trimmed)) {
        const disks = parseDiskUsage(trimmed);
        if (disks.length >= 1) {
            return { type: 'disk-usage', raw: output, parsed: disks };
        }
    }

    // 5. netstat / Get-NetTCPConnection → network table
    if (/netstat|get-nettcp|get-netudp/i.test(cmd) || detectNetFormat(trimmed)) {
        const conns = parseNetTable(trimmed);
        if (conns.length >= 2) {
            return { type: 'network-table', raw: output, parsed: conns };
        }
    }

    // 6. Event log (Get-EventLog / Get-WinEvent / journalctl)
    if (/get-eventlog|get-winevent|journalctl/i.test(cmd) || detectEventLogFormat(trimmed)) {
        const events = parseEventLog(trimmed);
        if (events.length >= 2) {
            return { type: 'event-log', raw: output, parsed: events };
        }
    }

    // 7. Key=Value format (registry, env, config files)
    if (detectKeyValueFormat(trimmed)) {
        const pairs = parseKeyValue(trimmed);
        if (pairs.length >= 3) {
            return { type: 'key-value', raw: output, parsed: pairs };
        }
    }

    // 8. Default: plain text
    return { type: 'plain', raw: output, parsed: null };
}

// ── Detection heuristics ─────────────────────────────────────────────────────

function tryParseJson(text: string): any | undefined {
    try {
        return JSON.parse(text);
    } catch {
        return undefined;
    }
}

function detectServiceFormat(text: string): boolean {
    return /\b(Running|Stopped|Paused)\b/i.test(text) &&
           (/Status\s+Name/i.test(text) || /\bw3svc\b|\bwuauserv\b|\bspooler\b/i.test(text));
}

function detectProcessFormat(text: string): boolean {
    return (/\bPID\b/i.test(text) || /\bCPU\b.*\bMem/i.test(text)) &&
           /\d+/.test(text) &&
           text.split('\n').length >= 4;
}

function detectDiskFormat(text: string): boolean {
    return /\b(GB|MB|TB)\b/i.test(text) &&
           (/Used|Free|Size|Capacity/i.test(text) || /\d+\.\d+\s*(G|M|T)B/i.test(text));
}

function detectNetFormat(text: string): boolean {
    return /\b(ESTABLISHED|LISTEN|TIME_WAIT|CLOSE_WAIT|SYN_SENT)\b/i.test(text) &&
           /:\d+/.test(text);
}

function detectEventLogFormat(text: string): boolean {
    return /\b(Information|Warning|Error|Critical)\b/i.test(text) &&
           (/\b(Source|EventID|TimeGenerated|LogName)\b/i.test(text) ||
            /\d{1,2}\/\d{1,2}\/\d{2,4}\s+\d{1,2}:\d{2}/.test(text));
}

function detectKeyValueFormat(text: string): boolean {
    const lines = text.split('\n').filter(l => l.trim());
    if (lines.length < 3) return false;
    const kvLines = lines.filter(l => /^\s*[\w\s.-]+\s*[:=]\s*.+/.test(l));
    return kvLines.length >= lines.length * 0.5;
}

// ── Parsers ──────────────────────────────────────────────────────────────────

function parseServiceList(text: string): ServiceRow[] {
    const lines = text.split('\n').filter(l => l.trim());
    const services: ServiceRow[] = [];

    for (const line of lines) {
        // Pattern: "Status  Name  DisplayName" or "Running  W3SVC  World Wide Web..."
        const m = line.match(/^\s*(Running|Stopped|Paused)\s+(\S+)\s+(.*)/i);
        if (m) {
            services.push({
                status: m[1] as ServiceRow['status'],
                name: m[2],
                displayName: m[3].trim(),
            });
            continue;
        }
        // Alternative pattern: "Name  Status" columns
        const m2 = line.match(/^\s*(\S+)\s+(Running|Stopped|Paused)\s*(.*)/i);
        if (m2) {
            services.push({
                name: m2[1],
                status: m2[2] as ServiceRow['status'],
                displayName: m2[3]?.trim() || m2[1],
            });
        }
    }
    return services;
}

function parseProcessTable(text: string): ProcessRow[] {
    const lines = text.split('\n').filter(l => l.trim());
    const procs: ProcessRow[] = [];

    for (const line of lines) {
        // Pattern: Name  CPU  Id  Mem (various formats)
        const m = line.match(/^\s*(\S+)\s+[\d.]+\s+[\d.]+\s+[\d.,]+\s+(\d+)/);
        if (m) {
            procs.push({ name: m[1], pid: parseInt(m[2]), cpu: 0, mem: '' });
            continue;
        }
        // tasklist format: "name.exe  PID  Session  Mem"
        const m2 = line.match(/^\s*(.+?)\s+(\d{2,})\s+\w+\s+\d+\s+([\d,]+)\s*K?/i);
        if (m2) {
            const memKb = parseInt(m2[3].replace(/,/g, ''));
            procs.push({
                name: m2[1].trim(),
                pid: parseInt(m2[2]),
                cpu: 0,
                mem: memKb > 1024 ? `${(memKb / 1024).toFixed(1)} MB` : `${memKb} KB`,
            });
        }
    }
    return procs;
}

function parseDiskUsage(text: string): DiskRow[] {
    const lines = text.split('\n').filter(l => l.trim());
    const disks: DiskRow[] = [];

    for (const line of lines) {
        // Pattern: "C  Fixed  Used:45.2  Free:12.8  (78%)"
        const m = line.match(/([A-Z])\s+\w*\s*[\d.]+\s+([\d.]+)\s+([\d.]+)/i);
        if (m) {
            const used = parseFloat(m[2]);
            const free = parseFloat(m[3]);
            const total = used + free;
            disks.push({
                drive: m[1] + ':',
                usedGB: used,
                freeGB: free,
                totalGB: total,
                usedPct: total > 0 ? Math.round((used / total) * 100) : 0,
            });
            continue;
        }
        // df format: "/dev/sda1  50G  35G  15G  70% /home"
        const m2 = line.match(/(\S+)\s+([\d.]+)([GMTK])\s+([\d.]+)([GMTK])\s+([\d.]+)([GMTK])\s+(\d+)%\s+(\S+)/);
        if (m2) {
            const toGB = (val: number, unit: string) => {
                if (unit === 'T') return val * 1024;
                if (unit === 'G') return val;
                if (unit === 'M') return val / 1024;
                return val / (1024 * 1024);
            };
            const total = toGB(parseFloat(m2[2]), m2[3]);
            const used = toGB(parseFloat(m2[4]), m2[5]);
            const free = toGB(parseFloat(m2[6]), m2[7]);
            disks.push({
                drive: m2[9] || m2[1],
                usedGB: used, freeGB: free, totalGB: total,
                usedPct: parseInt(m2[8]),
            });
        }
    }
    return disks;
}

function parseNetTable(text: string): NetRow[] {
    const lines = text.split('\n').filter(l => l.trim());
    const conns: NetRow[] = [];

    for (const line of lines) {
        // netstat format: "TCP  0.0.0.0:80  192.168.1.5:54321  ESTABLISHED  1234"
        const m = line.match(/^\s*(TCP|UDP)\s+(\S+):(\d+)\s+(\S+):(\d+)\s+(\w+)(?:\s+(\d+))?/i);
        if (m) {
            conns.push({
                protocol: m[1].toUpperCase(),
                localAddr: m[2],
                localPort: parseInt(m[3]),
                remoteAddr: m[4],
                remotePort: parseInt(m[5]),
                state: m[6],
                pid: m[7] ? parseInt(m[7]) : undefined,
            });
        }
    }
    return conns;
}

function parseKeyValue(text: string): KVPair[] {
    const lines = text.split('\n').filter(l => l.trim());
    const pairs: KVPair[] = [];

    for (const line of lines) {
        const m = line.match(/^\s*([\w\s.-]+?)\s*[:=]\s*(.+)/);
        if (m) {
            pairs.push({ key: m[1].trim(), value: m[2].trim() });
        }
    }
    return pairs;
}

function parseEventLog(text: string): EventLogRow[] {
    const lines = text.split('\n').filter(l => l.trim());
    const events: EventLogRow[] = [];

    for (const line of lines) {
        // PowerShell Get-EventLog format: "Index  Time  EntryType  Source  InstanceID  Message"
        const m = line.match(/^\s*\d+\s+([\d/\-]+\s+[\d:]+\s*(?:AM|PM)?)\s+(Information|Warning|Error|Critical)\s+(\S+)\s+(\d+)\s+(.*)/i);
        if (m) {
            events.push({
                time: m[1].trim(),
                level: m[2] as EventLogRow['level'],
                source: m[3],
                id: parseInt(m[4]),
                message: m[5].trim(),
            });
            continue;
        }
        // Get-WinEvent format: "TimeCreated  Id  LevelDisplayName  Message"
        const m2 = line.match(/^\s*([\d/\-]+\s+[\d:]+(?:\s*(?:AM|PM))?)\s+(\d+)\s+(Information|Warning|Error|Critical)\s+(.*)/i);
        if (m2) {
            events.push({
                time: m2[1].trim(),
                id: parseInt(m2[2]),
                level: m2[3] as EventLogRow['level'],
                source: '',
                message: m2[4].trim(),
            });
            continue;
        }
        // journalctl format: "Mon DD HH:MM:SS hostname service[pid]: message"
        const m3 = line.match(/^(\w{3}\s+\d{1,2}\s+[\d:]+)\s+\S+\s+(\S+?)(?:\[\d+\])?:\s+(.*)/);
        if (m3) {
            const msg = m3[3].trim();
            const isErr = /error|fail|crit/i.test(msg);
            const isWarn = /warn/i.test(msg);
            events.push({
                time: m3[1],
                source: m3[2],
                id: 0,
                level: isErr ? 'Error' : isWarn ? 'Warning' : 'Information',
                message: msg,
            });
        }
    }
    return events;
}

// ── Sparkline utility ────────────────────────────────────────────────────────

const SPARK_CHARS = '▁▂▃▄▅▆▇█'; // ▁▂▃▄▅▆▇█

/**
 * Generate a Unicode sparkline from an array of numbers.
 * Useful for inline trend visualization next to metrics.
 */
export function sparkline(values: number[]): string {
    if (!values || values.length === 0) return '';
    const min = Math.min(...values);
    const max = Math.max(...values);
    const range = max - min || 1;
    return values.map(v => {
        const idx = Math.min(7, Math.round(((v - min) / range) * 7));
        return SPARK_CHARS[idx];
    }).join('');
}

/**
 * Format a metric value with its sparkline trend.
 * e.g. "73%  ▂▃▅▆▇█▆▅"
 */
export function metricWithTrend(current: number, history: number[], unit: string = '%'): string {
    const spark = sparkline(history);
    return `${current}${unit}  ${spark}`;
}
