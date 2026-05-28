// ── activity-feed-helpers.ts ──────────────────────────────────────────────
//
// Pure helpers extracted from ActivityFeedWidget.svelte (Sprint 5, TEST-4)
// so they can be tested directly with vitest. Logic kept verbatim — moving
// them into a module doesn't change runtime behaviour.

/** Relative time formatter — "3m", "2h", "1d". Cheap, no library. */
export function relTime(ts: number, now: number): string {
    const age = Math.max(0, now - ts);
    if (age < 60)    return `${age}s`;
    if (age < 3600)  return `${Math.floor(age / 60)}m`;
    if (age < 86400) return `${Math.floor(age / 3600)}h`;
    return `${Math.floor(age / 86400)}d`;
}

/** Maps backend severity to a CSS class. */
export function sevClass(s: string): string {
    switch (s) {
        case 'error': return 'sev-error';
        case 'warn':  return 'sev-warn';
        case 'ok':    return 'sev-ok';
        default:      return 'sev-info';
    }
}

/** Glyph per kind — matches Lucy's geometric vocabulary. */
export function kindIcon(k: string): string {
    switch (k) {
        case 'incident': return '◆';
        case 'audit':    return '›';
        case 'schedule': return '⏰';
        case 'rollup':   return '◇';
        case 'snapshot': return '◫';
        case 'frontier': return '⌬';
        default:         return '·';
    }
}
