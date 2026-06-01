// ── dialog-service.ts — Promise-based dialog API (v1.7.17) ──────────────
//
// Replaces every native `confirm()` / `alert()` / `prompt()` in Lucy.
// Those were leaking the dev URL ("localhost:1420 dice...") into the
// UI and breaking the visual identity of the app. This module exposes
// the same ergonomic API but routed through an internal store rendered
// by `DialogHost.svelte`, a single component mounted once in
// `+page.svelte`.
//
// ── Usage ──────────────────────────────────────────────────────────────
//
//   import { lucyConfirm, lucyAlert, lucyPrompt } from '$lib/dialog-service';
//
//   if (!await lucyConfirm('¿Borrar memoria #2?')) return;
//
//   await lucyAlert('Operación completada', { tone: 'success' });
//
//   const name = await lucyPrompt('Nombre del preset:', {
//     defaultValue: 'mi-preset',
//     placeholder: 'kebab-case',
//   });
//   if (name === null) return;   // user cancelled
//
// ── Semantics ──────────────────────────────────────────────────────────
//
// - `lucyConfirm` resolves to `true` (OK) or `false` (cancel / Esc).
// - `lucyAlert` resolves when the user dismisses (single OK button).
// - `lucyPrompt` resolves to the entered string or `null` if cancelled.
//
// Each call is queued — if two open at once, the second waits for the
// first to settle. This matches native `confirm()` blocking semantics
// in spirit without freezing the UI.

import { writable, get, type Writable } from 'svelte/store';

export type DialogTone = 'default' | 'danger' | 'warning' | 'success' | 'info';
export type DialogKind = 'confirm' | 'alert' | 'prompt';

export interface DialogRequest {
    id:           number;
    kind:         DialogKind;
    /** Markdown-safe text shown as the headline / question. */
    message:      string;
    /** Optional secondary description. */
    description?: string;
    tone:         DialogTone;
    confirmLabel: string;
    cancelLabel?: string;
    /** Prompt-only. */
    defaultValue?: string;
    placeholder?:  string;
    multiline?:    boolean;
    /** Internal — called by DialogHost to settle the promise. */
    _resolve: (value: any) => void;
}

const queue: Writable<DialogRequest | null> = writable(null);
const pending: DialogRequest[] = [];

/** Subscribed by DialogHost. Reactive readers see the active dialog. */
export const activeDialog = queue;

let _id = 1;

function enqueue(req: Omit<DialogRequest, 'id'>): void {
    const full: DialogRequest = { ...req, id: _id++ };
    if (get(queue) === null) {
        queue.set(full);
    } else {
        pending.push(full);
    }
}

/** Called by DialogHost after the user makes a choice. */
export function settle(value: any): void {
    const cur = get(queue);
    if (!cur) return;
    cur._resolve(value);
    const next = pending.shift() || null;
    queue.set(next);
}

// ── Public API ──────────────────────────────────────────────────────────

export interface ConfirmOpts {
    description?: string;
    tone?:        DialogTone;
    confirmLabel?: string;
    cancelLabel?:  string;
}

export function lucyConfirm(message: string, opts: ConfirmOpts = {}): Promise<boolean> {
    return new Promise<boolean>(resolve => {
        enqueue({
            kind: 'confirm', message,
            description: opts.description,
            tone: opts.tone ?? 'default',
            confirmLabel: opts.confirmLabel ?? 'Aceptar',
            cancelLabel:  opts.cancelLabel  ?? 'Cancelar',
            _resolve: (v: boolean) => resolve(!!v),
        });
    });
}

export interface AlertOpts {
    description?: string;
    tone?:        DialogTone;
    confirmLabel?: string;
}

export function lucyAlert(message: string, opts: AlertOpts = {}): Promise<void> {
    return new Promise<void>(resolve => {
        enqueue({
            kind: 'alert', message,
            description: opts.description,
            tone: opts.tone ?? 'info',
            confirmLabel: opts.confirmLabel ?? 'Aceptar',
            _resolve: () => resolve(),
        });
    });
}

export interface PromptOpts {
    description?: string;
    tone?:        DialogTone;
    defaultValue?: string;
    placeholder?:  string;
    multiline?:    boolean;
    confirmLabel?: string;
    cancelLabel?:  string;
}

export function lucyPrompt(message: string, opts: PromptOpts = {}): Promise<string | null> {
    return new Promise<string | null>(resolve => {
        enqueue({
            kind: 'prompt', message,
            description: opts.description,
            tone: opts.tone ?? 'default',
            defaultValue: opts.defaultValue ?? '',
            placeholder:  opts.placeholder  ?? '',
            multiline:    !!opts.multiline,
            confirmLabel: opts.confirmLabel ?? 'Aceptar',
            cancelLabel:  opts.cancelLabel  ?? 'Cancelar',
            _resolve: (v: string | null) => resolve(v),
        });
    });
}
