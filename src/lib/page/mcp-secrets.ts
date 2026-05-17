// ── page/mcp-secrets.ts ───────────────────────────────────────────────────
//
// MCP secret-store helpers extracted from `+page.svelte` (Sprint D refactor).
// The MCP secret index lives in the OS keyring (Windows Credential Manager
// on Windows; libsecret/Keychain elsewhere) — one named entry per secret
// plus a special `_index` entry listing all names so we can enumerate.
//
// The Tauri commands themselves live in the Rust backend; this module is
// just the JS-side bookkeeping (load → in-memory map; save/delete keep both
// the map AND the index in sync).

import { invoke } from '@tauri-apps/api/core';

export type SecretMap = Record<string, string>;

/** Load all MCP secrets from the OS keyring into a plain object map. */
export async function loadMcpSecrets(): Promise<SecretMap> {
    try {
        const names = await invoke<string[]>('list_mcp_secrets');
        const entries = await Promise.all(
            names.map(async (n) => {
                try { return [n, await invoke<string>('get_mcp_secret', { name: n })] as const; }
                catch { return [n, ''] as const; }
            })
        );
        return Object.fromEntries(entries);
    } catch (e) {
        // eslint-disable-next-line no-console
        console.warn('[MCP] keyring load failed:', e);
        return {};
    }
}

/** Persist a single secret and refresh the index. Returns the updated map. */
export async function saveMcpSecret(
    current: SecretMap,
    name: string,
    value: string,
): Promise<SecretMap> {
    await invoke('save_mcp_secret', { name, value });
    const next = { ...current, [name]: value };
    await invoke('set_mcp_secret_index', { names: Object.keys(next) });
    return next;
}

/** Remove a secret from keyring + map. Returns the updated map. */
export async function deleteMcpSecret(
    current: SecretMap,
    name: string,
): Promise<SecretMap> {
    try { await invoke('delete_mcp_secret', { name }); } catch { /* tolerate keyring drift */ }
    const next = { ...current };
    delete next[name];
    await invoke('set_mcp_secret_index', { names: Object.keys(next) });
    return next;
}
