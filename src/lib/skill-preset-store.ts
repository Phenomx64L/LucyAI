// ── skill-preset-store.ts (v1.6.1) ────────────────────────────────────────
//
// Reactive store wrapping `activeSkillPresetId`. Persisted to localStorage
// so the user's choice survives reloads. The store ALWAYS resolves to a
// valid SkillPreset object (or null) — the picker just sets the id; the
// rest of the app reads `activeSkillPreset` and gets the resolved object.

import { writable, derived, get } from 'svelte/store';
import { safeGetLS, safeSetLSString } from '$lib/safe-ls';
import { SKILL_PRESETS, getPreset, type SkillPreset } from '$lib/skill-presets';

const LS_KEY = 'lucy_active_skill_preset';

/** Raw id store. `null` = no preset active (Lucy uses default behaviour). */
export const activeSkillPresetId = writable<string | null>(
    safeGetLS(LS_KEY, '') || null,
);

activeSkillPresetId.subscribe(id => {
    safeSetLSString(LS_KEY, id ?? '');
});

/** Resolved preset object (or null). Read this from components. */
export const activeSkillPreset = derived<typeof activeSkillPresetId, SkillPreset | null>(
    activeSkillPresetId,
    id => getPreset(id),
);

/** Imperative setter for non-reactive callers (e.g. slash-command
 *  handlers). Validates the id against the catalog before storing. */
export function setActivePresetId(id: string | null): void {
    if (id !== null && !SKILL_PRESETS.some(p => p.id === id)) {
        console.warn(`[skill-preset-store] unknown preset id: ${id}`);
        return;
    }
    activeSkillPresetId.set(id);
}

/** Synchronous peek without subscribing — useful inside prompt-builders. */
export function peekActivePreset(): SkillPreset | null {
    return get(activeSkillPreset);
}
