// ── page/host-preflight.ts ────────────────────────────────────────────────
//
// Reachability test for remote hosts — extracted from `+page.svelte`.
//
// Why we need this: Lucy invokes commands via WinRM (Windows, port 5985) or
// SSH (Linux, port 22). When a host is offline, the underlying call hangs
// for 15+ seconds before timing out — terrible UX for "execute on host X".
// Doing a fast TCP probe FIRST (Test-NetConnection wrapped in PowerShell
// on the LOCAL machine) tells us in <2s whether to even attempt the call.
//
// 30s TTL cache: hosts don't go up/down often enough to justify probing on
// every keystroke. The cache key prefers `h.id` (stable) but falls back to
// `h.host` for unsaved hosts.

import { invoke } from '@tauri-apps/api/core';

export interface PreflightHost {
    id?: string;
    host: string;
    port?: number;
    type?: 'windows' | 'linux';
}

export interface PreflightResult {
    ok: boolean;
    err: string | null;
    ms?: number;
    cached?: boolean;
}

interface CacheEntry {
    ts: number;
    ok: boolean;
    err: string | null;
    ms?: number;
}

const cache = new Map<string, CacheEntry>();
const TTL_MS = 30_000;

/** Test TCP reachability for a host. Cached for 30s per (id|host) key. */
export async function preflightHost(h: PreflightHost): Promise<PreflightResult> {
    if (!h || !h.host) return { ok: false, err: 'Host inválido' };
    const key = h.id || h.host;
    const cached = cache.get(key);
    if (cached && (Date.now() - cached.ts) < TTL_MS) {
        return { ok: cached.ok, err: cached.err, ms: cached.ms, cached: true };
    }

    const port = h.port || (h.type === 'linux' ? 22 : 5985);
    // Test-NetConnection works for any TCP port on Windows; we run it on the
    // local host so it never depends on the remote being reachable.
    const escaped = h.host.replace(/'/g, "''");
    const script = `$ErrorActionPreference='Stop'; try { $r = Test-NetConnection -ComputerName '${escaped}' -Port ${port} -InformationLevel Quiet -WarningAction SilentlyContinue; if ($r) { 'OK' } else { throw "TCP ${port} cerrado o host no responde" } } catch { Write-Error $_.Exception.Message }`;

    const t0 = Date.now();
    let result: PreflightResult;
    try {
        // v1.5.0 — forceExecute parameter removed. Preflight runs a
        // safe Test-NetConnection script that never hits the guardrail,
        // so no bypass token is needed.
        const out = await invoke<string>('execute_powershell', { script });
        const ok = String(out || '').trim().toUpperCase().includes('OK');
        result = ok
            ? { ok: true, err: null, ms: Date.now() - t0 }
            : { ok: false, err: `Puerto ${port} no responde en ${h.host}`, ms: Date.now() - t0 };
    } catch (e) {
        result = {
            ok: false,
            err: `Host ${h.host}:${port} inaccesible — ${String(e).substring(0, 200)}`,
            ms: Date.now() - t0,
        };
    }
    cache.set(key, { ts: Date.now(), ok: result.ok, err: result.err, ms: result.ms });
    return result;
}

/** Drop a single cache entry — e.g. after the user updates the host config. */
export function invalidatePreflight(key: string): void { cache.delete(key); }

/** Drop ALL entries — useful on connection-policy changes. */
export function clearPreflightCache(): void { cache.clear(); }
