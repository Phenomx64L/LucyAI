// ── diff-util.ts — line-based diff for the cockpit Artifacts panel ───────────
//
// Extracted from RemoteFileDiffModal's inline `diffLines` (v1.7.232) so the
// cockpit can render an edit/write artifact's before→after without pulling in a
// diff library. Trims the common prefix and suffix and shows only the changed
// region plus one line of surrounding context — good enough for config-file and
// code edits (an LCS would be overkill and slow on large files).

export type DiffLineType = 'eq' | 'add' | 'rem';
export interface DiffLine {
    type: DiffLineType;
    text: string;
    n?: number; // 1-based source line number, only on context ('eq') lines
}

/**
 * Diff two strings line-by-line. Returns [] when identical. Removed lines
 * ('rem') come from `a`, added lines ('add') from `b`; unchanged context lines
 * ('eq') bracket the change (at most one line above and below).
 */
export function diffLines(a: string | null | undefined, b: string | null | undefined): DiffLine[] {
    const A = String(a ?? '');
    const B = String(b ?? '');
    if (A === B) return [];
    const aL = A.split('\n');
    const bL = B.split('\n');
    // Trim the common prefix/suffix so the diff stays tight around the change.
    let pre = 0;
    while (pre < aL.length && pre < bL.length && aL[pre] === bL[pre]) pre++;
    let suf = 0;
    while (
        suf < aL.length - pre &&
        suf < bL.length - pre &&
        aL[aL.length - 1 - suf] === bL[bL.length - 1 - suf]
    ) suf++;
    const aMid = aL.slice(pre, aL.length - suf);
    const bMid = bL.slice(pre, bL.length - suf);
    const out: DiffLine[] = [];
    if (pre > 0) {
        const ctx = Math.min(pre, 1);
        for (let i = pre - ctx; i < pre; i++) out.push({ type: 'eq', text: aL[i], n: i + 1 });
    }
    for (const line of aMid) out.push({ type: 'rem', text: line });
    for (const line of bMid) out.push({ type: 'add', text: line });
    if (suf > 0) {
        const start = aL.length - suf;
        const ctx = Math.min(suf, 1);
        for (let i = start; i < start + ctx; i++) out.push({ type: 'eq', text: aL[i], n: i + 1 });
    }
    return out;
}
