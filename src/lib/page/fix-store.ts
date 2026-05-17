// ── page/fix-store.ts ─────────────────────────────────────────────────────
//
// Small FIFO-evicted key→value store used by the sidebar autofix flow.
// When Lucy proposes a fix script in chat, it's stashed here keyed by a
// short string the UI can put inside a "Fix" button's data attribute. The
// button click pulls the script back out (one-shot read or persistent
// access — `delete()` is exposed for one-shot semantics).
//
// Bounded to 50 entries with FIFO eviction so a long session can't leak
// unbounded fix references into RAM. Map preserves insertion order, which
// is what we exploit to find the oldest key for eviction.

const CAP = 50;
const store = new Map<string, unknown>();

/** Insert or replace a key, evicting the oldest entry if over capacity. */
export function setFix(key: string, value: unknown): void {
    if (store.size >= CAP) {
        const oldest = store.keys().next().value;
        if (oldest !== undefined) store.delete(oldest);
    }
    store.set(key, value);
}

export function getFix<T = unknown>(key: string): T | undefined {
    return store.get(key) as T | undefined;
}

export function deleteFix(key: string): boolean {
    return store.delete(key);
}

/** Clear the entire store (for tests / explicit teardown). */
export function clearFixStore(): void { store.clear(); }
