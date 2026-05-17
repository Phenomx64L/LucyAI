// ── invoke-typed.ts — Type-safe wrapper around Tauri's invoke() ─────────────
//
// Why
// ---
// The raw `invoke()` from `@tauri-apps/api/core` returns `Promise<unknown>`.
// Every callsite has to cast manually, and refactors on the Rust side break
// the frontend silently (you get `undefined.foo` runtime errors instead of
// compile-time type mismatches).
//
// `invokeTyped<R>` adds a generic return-type parameter that's enforced by
// TypeScript. Combined with the `ts-rs`-generated bindings in `lib/types/`,
// this gives end-to-end type safety:
//
//     import type { SystemHealth } from '$lib/types/SystemHealth';
//     const h = await invokeTyped<SystemHealth>('get_system_health_json');
//     h.memory.percent  // ← typed, autocomplete works
//
// The wrapper also:
//   • Distinguishes LucyError objects (typed) from legacy plain-string errors
//   • Lets you opt into automatic silent-error reporting via `.silent`
//   • Optionally validates the response shape at runtime if you pass a guard
//
// Backward-compat note
// --------------------
// Old callsites using `invoke('foo', args)` keep working untouched. Migrate
// incrementally: one command at a time, pick the highest-frequency or
// most-bug-prone first.

import { invoke } from '@tauri-apps/api/core';
import { reportSilent } from './silent-errors';

/**
 * Shape emitted by the Rust `LucyError` enum (utils/error.rs).
 * Tagged-union JSON via serde: `{ code: "NotFound", data: { ... } }`.
 */
export interface LucyErrorPayload {
    code:
        | 'PermissionDenied'
        | 'NotFound'
        | 'InvalidInput'
        | 'Timeout'
        | 'Io'
        | 'SecurityBlock'
        | 'ProviderError'
        | 'Internal';
    data: Record<string, unknown>;
}

/** Type guard: is this a LucyError shape vs a legacy plain string? */
export function isLucyError(err: unknown): err is LucyErrorPayload {
    return (
        typeof err === 'object' &&
        err !== null &&
        'code' in err &&
        typeof (err as any).code === 'string'
    );
}

/**
 * Extract a human-readable message from any error shape we might receive
 * from Tauri (LucyError object | plain string | Error | anything).
 */
export function errorMessage(err: unknown): string {
    if (isLucyError(err)) {
        const data = err.data || {};
        // Prefer a `message` field if the variant carries one, otherwise
        // fall back to the code + JSON stringify of data.
        if (typeof (data as any).message === 'string') return (data as any).message;
        if (typeof (data as any).reason === 'string')  return (data as any).reason;
        try { return `${err.code}: ${JSON.stringify(data)}`; }
        catch { return err.code; }
    }
    if (err instanceof Error) return err.message;
    if (typeof err === 'string') return err;
    try { return JSON.stringify(err); } catch { return String(err); }
}

export interface InvokeOptions<R> {
    /**
     * If provided, returns a `.catch(reportSilent(ctx))`-style result so
     * fire-and-forget failures get recorded to the silent-error log.
     * Use when you don't want the error to bubble to the caller.
     */
    silent?: string;
    /**
     * Optional runtime validator. If the validator returns false, the call
     * rejects with an InvalidInput-like error. Cheap defense against Rust
     * side schema drift.
     */
    validate?: (response: unknown) => response is R;
}

/**
 * Typed wrapper around Tauri's invoke. The generic `R` parameter is the
 * shape you expect Rust to return. TypeScript enforces it from this point on.
 *
 * Throws:
 *   • If `silent` is NOT set: rejects with the raw error (preserves
 *     `LucyError` shape) so the caller can `.catch` and inspect.
 *   • If `silent` IS set: never rejects — logs via reportSilent and resolves
 *     to `undefined`. Use for background tasks you don't want to surface.
 *
 * @param command  The Tauri command name (snake_case, matches Rust `#[tauri::command]`).
 * @param args     Argument object. Tauri auto-converts snake_case ↔ camelCase keys.
 * @param opts     Optional behavior overrides.
 */
export async function invokeTyped<R>(
    command: string,
    args?: Record<string, unknown>,
    opts: InvokeOptions<R> = {}
): Promise<R | undefined> {
    try {
        const result = await invoke<unknown>(command, args);
        if (opts.validate && !opts.validate(result)) {
            // The response didn't match the expected shape — log and either
            // surface to caller or swallow depending on silent mode.
            const err = `[invokeTyped] '${command}' returned unexpected shape`;
            if (opts.silent) {
                reportSilent(opts.silent)(err);
                return undefined;
            }
            throw new Error(err);
        }
        return result as R;
    } catch (err) {
        if (opts.silent) {
            reportSilent(opts.silent)(err);
            return undefined;
        }
        throw err;
    }
}

/**
 * Convenience: fire-and-forget invocation that NEVER throws. Always returns
 * a Promise that resolves (with either the value or `undefined` on error).
 * Equivalent to `invokeTyped(cmd, args, { silent: ctx })`.
 */
export function invokeSilent<R = void>(
    ctx: string,
    command: string,
    args?: Record<string, unknown>
): Promise<R | undefined> {
    return invokeTyped<R>(command, args, { silent: ctx });
}
