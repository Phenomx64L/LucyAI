// ── page/chips-quick-actions.ts ──────────────────────────────────────────
//
// Two small CRUD subsystems extracted from `+page.svelte` (Sprint D):
//
//   • **Chips** — bottom-bar shortcut buttons. Each chip has a visible label
//     and a `clave` that gets typed into the input when clicked.
//   • **Quick actions** — sidebar shortcut buttons. Each runs a PowerShell
//     `script` directly (no AI loop). One reserved keyword `TOOL_SYSINFO`
//     routes to the native sysinfo command instead of PS.
//
// All persistence goes through `safeSetLS`. The mutation helpers all return
// the NEW array — callers reassign their reactive variable to trigger
// Svelte's invalidation.

import { safeSetLS } from '$lib/safe-ls';

// ── Chip types ────────────────────────────────────────────────────────────
export interface Chip {
    label: string;
    clave: string;
}

const CHIPS_KEY = 'lucy_user_chips';

/** Persist + return the array unchanged (helper for inline calls). */
export function persistChips(chips: Chip[]): Chip[] {
    safeSetLS(CHIPS_KEY, chips);
    return chips;
}

export function upsertChip(chips: Chip[], idx: number | null, chip: Chip): Chip[] {
    const label = chip.label.trim();
    const clave = chip.clave.trim();
    if (!label || !clave) return chips;
    let next: Chip[];
    if (idx === null) {
        next = [...chips, { label, clave }];
    } else {
        next = chips.slice();
        next[idx] = { label, clave };
    }
    return persistChips(next);
}

export function deleteChip(chips: Chip[], idx: number): Chip[] {
    const next = chips.slice();
    next.splice(idx, 1);
    return persistChips(next);
}

// ── Quick Actions types ───────────────────────────────────────────────────
export interface QuickAction {
    icono: string;
    nombre: string;
    script: string;
}

const ACTIONS_KEY = 'lucy_quick_actions';

/** Normalize the icon key — fall back to `bolt` if not in the user's icon map. */
function safeIcon(iconKey: string, iconMap: Record<string, unknown>): string {
    return iconMap[iconKey] ? iconKey : 'bolt';
}

export function upsertQuickAction(
    actions: QuickAction[],
    idx: number | null,
    draft: { nombre: string; script: string; icono: string },
    iconMap: Record<string, unknown>,
): QuickAction[] {
    if (!draft.nombre.trim() || !draft.script.trim()) return actions;
    const icono = safeIcon(draft.icono, iconMap);
    let next: QuickAction[];
    if (idx !== null) {
        next = actions.slice();
        next[idx] = { ...next[idx], nombre: draft.nombre, script: draft.script, icono };
    } else {
        next = [...actions, { icono, nombre: draft.nombre, script: draft.script }];
    }
    safeSetLS(ACTIONS_KEY, next);
    return next;
}

export function deleteQuickAction(actions: QuickAction[], idx: number): QuickAction[] {
    const next = actions.slice();
    next.splice(idx, 1);
    safeSetLS(ACTIONS_KEY, next);
    return next;
}
