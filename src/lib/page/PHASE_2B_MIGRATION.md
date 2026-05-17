# Phase 2b — Per-tab store migration (Sprint D continuation)

> **Status (Sprint D, May 2026):** Foundation laid in Phase 2a. The actual
> reactivity gain happens when components migrate to subscribe to the
> stores directly. This file is the migration plan — work for the next
> session, NOT this one.

## What's already in place (Phase 2a)

- `src/lib/page/tabs-store.ts` — `tabsStore`, `activeTabIdStore`,
  `activeTabStore` (derived), `tabsRev` (global counter), per-tab `_rev`.
- `+page.svelte`:
  - `refresh()` now ALSO syncs to `tabsStore` and bumps `tabsRev`.
  - `refreshSoft(tabId)` available — bumps only the named tab's `_rev`.
  - Structural mutations (`crearTab`, `_ejecutarCierreTab`, session
    restore) call `syncTabsStore(tabs)`.
  - `$: setActiveTab(activeTabId)` mirrors active id into the store.
- The `let tabs` reactive variable still exists for back-compat with
  components that haven't migrated.

## What Phase 2b must do

### 1. Migrate `ChatThread.svelte` to subscribe to store

Currently:
```svelte
<ChatThread tab={tab} isEN={isEN} ... />
```

After:
```svelte
<!-- The component takes only the tab ID; reads everything else from the
     store. Saves the parent the cost of passing `tab` (which forces a
     re-evaluation even when the same ref is passed). -->
<ChatThread tabId={tab.id} isEN={isEN} ... />
```

Inside ChatThread:
```svelte
<script>
  import { tabsStore, tabsRev } from '$lib/page/tabs-store';
  export let tabId: string;

  // Subscribe to the specific tab's _rev — re-renders ONLY when this
  // tab's revision bumps. Other tabs streaming won't re-render us.
  $: rev = $tabsRev;                                  // gate: any change
  $: tab = $tabsStore.find(t => t.id === tabId);      // O(N) but N≤50 typical
  $: tabRev = tab?._rev ?? 0;                         // per-tab gate

  // Use `tabRev` as the dependency for inner reactives so they only
  // re-evaluate when THIS tab changes:
  $: visibleMessages = (tabRev, tab?.messages ? filterVisible(tab.messages) : []);
</script>
```

### 2. Replace hot-path `refresh()` with `refreshSoft(tabId)`

After ChatThread is on the store:

- `renderRevealed` (streaming reveal): `refresh() → refreshSoft(tabId)`
- `addMsg`: `refresh() → refreshSoft(tabId)` (the new message belongs to one tab)
- `runForced`/`runAI` exec lifecycle: `refreshSoft(tabId)` instead of `refresh()`

Reserved for `refresh()` (structural):
- Tab add/remove (already wrapped via syncTabsStore)
- Reordering tabs
- Cross-tab actions (e.g. "close all")

### 3. Migrate the 26 `$:` reactives in `+page.svelte`

Audit them one by one:

| Reactive | Currently depends on | After migration |
|---|---|---|
| `activeTab` | `tabs`, `activeTabId` | `$activeTabStore` |
| `lucyState` | `activeTab` | derive from `$activeTabStore` |
| `subAgentEffective` | `activeTab?.selectedModel` | derive from `$activeTabStore` |
| `verifierEffective` | same | same |
| `contextMax` | `activeTab?.contextMax` | derive from `$activeTabStore` |
| `postureHosts` | `$hosts, $localHealth, ...` | already store-driven, keep |
| `historyResults` | `activeTabId` | already minimal, keep |

After this:
- Switching tabs invalidates ONLY the reactives that read
  `$activeTabStore` (still 5-6, but they're cheap and only fire on switch).
- Streaming into one tab only re-renders THAT tab's ChatThread.
- Other tabs sleep entirely.

### 4. Remove `let tabs` + `refresh()`

Final state: `+page.svelte` doesn't hold the canonical tabs array
anywhere. All reads go through `$tabsStore`. All writes use
`tabsStore.update(...)` or the helpers in tabs-store.ts.

Expected delta after full Phase 2b:
- `+page.svelte` LOC: −300 to −500 (the `tabs = [...tabs]` thrash
  scattered everywhere collapses)
- Streaming FPS: stable 60Hz even with 50+ tabs open (today: drops to
  20-30 with that load)
- `runAI` extractable to `src/lib/page/agent-loop.ts` because it no
  longer touches the page's reactive `tabs` variable

## Why we didn't do this in Sprint D

Phase 2b touches every Svelte component that takes `tab` or `tabs` as
a prop (ChatThread, TabBar, ChatInput, NexShellView, StatusBar…).
That's a coordinated change across ~8 files with non-trivial
testing — best done in its own session with a clean diff and the
ability to revert cleanly if streaming breaks.

What we DID do here (Phase 2a):
- Built the foundation so the migration can be incremental.
- Synced the store on structural changes — components migrating in
  Phase 2b will see correct data on day one.
- Documented the exact migration order above.
