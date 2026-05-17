// ── page/tabs-store.ts ────────────────────────────────────────────────────
//
// Per-tab reactivity foundation (Sprint D · audit P2 HIGH).
//
// Why this exists
// ---------------
// The old +page.svelte had `refresh = () => tabs = [...tabs]` called 30+
// times per agent turn. Every refresh re-cloned the entire `tabs` array,
// invalidating ALL 26 `$:` reactive blocks AND every component that took
// `tabs` or `activeTab` as a prop. With 50 open tabs and a streaming
// response (~33 reveals/sec), that thrash was visible jank in DevTools.
//
// The fix — phased
// ----------------
// Phase 2a (this module): introduce a writable-store source of truth +
// per-tab revision counters + helpers that mutate ONLY the affected tab.
// Components can now subscribe to a derived `activeTabStore` or to a
// per-tab `_rev` counter and only re-render when THEIR data changes.
//
// Phase 2b (future): migrate ChatThread.svelte to take a single-tab store
// instead of the whole array. Once that's done, the page-level `tabs`
// `let` can disappear entirely and `refresh()` retires.
//
// Migration strategy
// ------------------
// During phase 2a, both styles coexist:
//   • The page keeps `let tabs = [...]` as the canonical mutable surface.
//   • This module mirrors that array into `tabsStore` + bumps `tabsRev`
//     whenever the page calls `bumpTabsRev()` / `bumpTab(id)`.
//   • New hot reactives subscribe to `$tabsRev` (scalar — cheap) instead
//     of to `tabs` directly, so the spread-clone in `refresh()` no longer
//     fans out to them.
//
// IMPORTANT: callers MUST call `syncTabsStore(tabs)` AFTER any structural
// change to the page's `tabs` array (push, splice, reassign) so the store
// stays in lockstep. For in-place tab mutations (e.g. `t.messages.push`),
// call `bumpTab(t.id)` instead — much cheaper, only fans out to that tab's
// subscribers.

import { writable, derived, get, type Readable, type Writable } from 'svelte/store';

// Minimal Tab shape — only fields this module needs to function. The
// page-level `Tab` is much larger; we intentionally don't pull it in so
// this module stays decoupled and easy to test.
export interface MinimalTab {
    id: string;
    title?: string;
    selectedModel?: string;
    isProcessing?: boolean;
    messages?: unknown[];
    /** Per-tab revision counter. Bumped on every in-place mutation that
     *  components care about (e.g. message append, isProcessing change).
     *  Subscribe to this in a `$:` block to re-render ONLY when the tab
     *  changes — not on every cousin tab's update. */
    _rev?: number;
    /** Tolerate any other fields the page adds (attachedFiles, _streamTPS…). */
    [key: string]: unknown;
}

/** Source of truth for the tab list. Page mirrors its `let tabs` here. */
export const tabsStore: Writable<MinimalTab[]> = writable([]);

/** Active tab id, separate writable so changing it doesn't ripple through
 *  the whole tabs array. */
export const activeTabIdStore: Writable<string | null> = writable(null);

/** Global revision counter — bumped on ANY structural change to the
 *  tabs array (add/remove/reorder/replace). Hot reactive blocks that just
 *  need "something changed, recompute" subscribe to this scalar instead
 *  of to the full `tabs` array — way cheaper invalidation. */
export const tabsRev: Writable<number> = writable(0);

/** Per-tab revision stores — created lazily the first time a tab id is
 *  requested. ChatThread + sibling components subscribe to THEIR tab's
 *  store so they re-render ONLY when their tab streams a token, isn't
 *  affected by cousin tabs updating. This is the granular reactivity
 *  win audit P2 prescribed.
 *
 *  Lazy allocation keeps memory down: a session with 50 tabs only
 *  creates 50 stores once, not on every render. We delete the entry
 *  when the tab is closed (see `disposeTabRev`).
 */
const _perTabRev = new Map<string, Writable<number>>();

/** Get-or-create the per-tab revision store for the given id. Safe to
 *  call from a `$:` reactive — the same id always returns the same
 *  store instance, so subscriptions stay stable. */
export function getTabRevStore(id: string): Writable<number> {
    let s = _perTabRev.get(id);
    if (!s) {
        s = writable(0);
        _perTabRev.set(id, s);
    }
    return s;
}

/** Drop the per-tab rev store after a tab is closed. Callers MUST
 *  invoke this in `_ejecutarCierreTab` to prevent slow memory growth
 *  across long sessions with many tab opens/closes. */
export function disposeTabRev(id: string): void {
    _perTabRev.delete(id);
}

/** Derived: the currently active tab, recomputed only when `tabsStore` or
 *  `activeTabIdStore` actually change. */
export const activeTabStore: Readable<MinimalTab | null> = derived(
    [tabsStore, activeTabIdStore],
    ([$tabs, $id]) => {
        if (!$id) return null;
        return $tabs.find(t => t.id === $id) ?? null;
    },
);

/** Mirror the page's `tabs` array into the store. Called after every
 *  structural mutation (add/remove/reorder). For in-place mutations of a
 *  single tab, prefer `bumpTab(id)` — it's cheaper. */
export function syncTabsStore(tabs: MinimalTab[]): void {
    tabsStore.set(tabs);
    tabsRev.update(n => n + 1);
}

/** Bump the global revision counter without re-setting the array. Use
 *  this when a single tab's field changed in place (e.g. `t.inputValue =
 *  '...'` or `t.messages.push(...)`). Combined with per-tab `_rev` bump
 *  so component-level `$: rev = tab._rev` reactives fire ONLY for the
 *  tab that changed. */
export function bumpTabsRev(): void {
    tabsRev.update(n => n + 1);
}

/** Bump a specific tab's `_rev` field AND its per-tab revision store.
 *  This is the granular reactivity primitive (audit P2): components
 *  subscribed to THIS tab's `getTabRevStore(id)` re-render only when
 *  their tab updates. Cousin tabs are NOT invalidated.
 *
 *  We also bump the global `tabsRev` for any consumer that doesn't
 *  bother with per-tab granularity (the cost is one scalar update,
 *  negligible compared to the array-clone the old `refresh()` did).
 */
export function bumpTab(id: string | null | undefined): void {
    if (!id) return;
    const tabs = get(tabsStore);
    const t = tabs.find(x => x.id === id);
    if (t) t._rev = ((t._rev as number) || 0) + 1;
    const perTab = _perTabRev.get(id);
    if (perTab) perTab.update(n => n + 1);
    tabsRev.update(n => n + 1);
}

/** O(1) lookup helper. Returns the LIVE object (mutations to its fields
 *  are seen by subscribers as long as `bumpTab` is called). */
export function getTabSync(id: string | null | undefined): MinimalTab | null {
    if (!id) return null;
    return get(tabsStore).find(t => t.id === id) ?? null;
}

/** Set the active tab id. Cheap — only one writable updates. */
export function setActiveTab(id: string | null): void {
    activeTabIdStore.set(id);
}

// ── Diagnostics (DevTools) ───────────────────────────────────────────────
// Expose the stores on `window` in dev mode so you can poke at the state
// from the console without re-mounting.
if (typeof window !== 'undefined') {
    (window as unknown as Record<string, unknown>)._lucyTabsStore = {
        tabsStore,
        activeTabIdStore,
        activeTabStore,
        tabsRev,
        get: getTabSync,
    };
}
