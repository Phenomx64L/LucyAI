// ── context-compressor — Hermes-Wiki v3 inspired pre-LLM tool result pruning
//
// Three cheap, local passes that run BEFORE we hand the agent loop's tool
// results back to the LLM. Each one is pure (no I/O, no LLM call) so they
// add ~0 latency but can collapse 30-50% of the token budget on
// repetitive/exploration-heavy turns.
//
//   Pass 1 — MD5-style dedup
//     If the same tool result content (≥200 chars) appeared earlier in the
//     session, replace the older copy with a placeholder and keep only the
//     newest. Typical case: Lucy reads `+page.svelte` five times during a
//     refactor — only the last copy carries information; the others are noise.
//
//   Pass 2 — Smart Collapse
//     Old tool outputs (older than KEEP_RECENT_RESULTS) get rewritten into a
//     single info-rich line that mentions the tool, the args, and the size:
//       [readfile] /path/to/X (1 200 chars, 47 lines)
//       [searchfiles] pattern="foo" → 12 matches
//       [exec_powershell] ran `Get-Service` → exit 0, 32 lines
//
//   Pass 3 — Anti-thrashing
//     Tracks the reduction ratio of the last two compactions in
//     `tab.workingMemory._compRatios`. If both reduced <10%, the next
//     compactionRequest() returns null — no point burning the LLM call when
//     there's nothing left to squeeze.
//
// All three passes preserve message order and never touch the most recent
// KEEP_RECENT_RESULTS results — Lucy needs verbatim recent context.

const KEEP_RECENT_RESULTS = 3;            // never collapse the last N results
const MIN_DEDUP_LEN       = 200;          // skip dedup for short outputs
const COLLAPSE_BLOCK_LEN  = 8_000;        // collapse blocks longer than this when old
const ANTI_THRASH_THRESHOLD = 0.10;       // 10% — below this, two in a row → skip

// Cheap content fingerprint. Not cryptographic — just a stable identity check
// so a deep-equal comparison doesn't have to scan tens of KB twice.
function fingerprint(s: string): string {
    if (!s) return '0';
    let h1 = 0x811c9dc5;       // FNV-1a-ish
    let h2 = 0x01000193;
    const len = s.length;
    // Sample first/last/middle 256 chars + length to stay O(1) for huge strings.
    const stride = Math.max(1, Math.floor(len / 768));
    for (let i = 0; i < len; i += stride) {
        const c = s.charCodeAt(i);
        h1 = (h1 ^ c) >>> 0;
        h1 = (h1 * 16777619) >>> 0;
        h2 = (h2 ^ (c << 1)) >>> 0;
        h2 = (h2 * 16777619) >>> 0;
    }
    return `${len.toString(36)}-${h1.toString(36)}-${h2.toString(36)}`;
}

/** Stand-alone tool-result entry as collected during the agent loop. */
export interface ToolResult {
    /** The tool name as parsed from the <TOOL>name:...</TOOL> tag. */
    kind?: string;
    /** Optional arg snapshot for Smart Collapse to reference. */
    args?: string;
    /** Raw text body that the LLM would see. */
    text: string;
}

/**
 * Pass 1 — dedup. Scans `results` from oldest→newest. If a fingerprint repeats,
 * the older instance is replaced by a placeholder line. The newest copy of
 * each duplicate set survives intact. Returns a NEW array (does not mutate).
 */
export function dedupToolResults(results: ToolResult[]): ToolResult[] {
    if (!results?.length) return results || [];
    // Walk newest→oldest so the FIRST time we see a fingerprint = the newest.
    const seen = new Set<string>();
    const out  = new Array<ToolResult>(results.length);
    for (let i = results.length - 1; i >= 0; i--) {
        const r = results[i];
        const text = r?.text ?? '';
        if (text.length < MIN_DEDUP_LEN) {
            out[i] = r;
            continue;
        }
        const fp = fingerprint(text);
        if (seen.has(fp)) {
            out[i] = { kind: r.kind, args: r.args, text:
                `[Duplicate tool output — same content as a more recent ${r.kind || 'result'}; collapsed to save tokens]`
            };
        } else {
            seen.add(fp);
            out[i] = r;
        }
    }
    return out;
}

/**
 * Pass 2 — Smart Collapse. Rewrites tool results OLDER than the keep window
 * into one informative line each. Recent results (last KEEP_RECENT_RESULTS)
 * pass through verbatim because Lucy needs them in full to keep working.
 */
export function smartCollapseOldResults(results: ToolResult[], keepRecent = KEEP_RECENT_RESULTS): ToolResult[] {
    if (!results?.length || results.length <= keepRecent) return results;
    const cutIdx = results.length - keepRecent;
    return results.map((r, i) => {
        if (i >= cutIdx) return r;                  // recent → keep verbatim
        const text = r?.text ?? '';
        // Don't collapse short outputs — already cheap.
        if (text.length < COLLAPSE_BLOCK_LEN / 4) return r;
        return { kind: r.kind, args: r.args, text: collapseLine(r) };
    });
}

/** Build a tool-aware single-line summary of a stale tool result. */
function collapseLine(r: ToolResult): string {
    const kind = (r.kind || 'tool').toLowerCase();
    const text = r.text || '';
    const lines = text.split('\n').length;
    const chars = text.length;
    const argsTag = r.args ? ` args="${String(r.args).slice(0, 80)}"` : '';
    // Per-tool flavor — the spirit of Hermes Wiki "Smart Collapse" templates.
    if (kind.includes('readfile') || kind.includes('readlines')) {
        return `[${kind}]${argsTag} → ${chars.toLocaleString()} chars, ${lines} lines (collapsed)`;
    }
    if (kind.includes('listdir') || kind.includes('searchfiles') || kind.includes('locate')) {
        const matches = (text.match(/\n/g) || []).length;
        return `[${kind}]${argsTag} → ${matches} entries (collapsed)`;
    }
    if (kind.includes('exec') || kind.includes('shell') || kind.includes('cmd') || kind.includes('powershell')) {
        const exitMatch = text.match(/\[EXIT_CODE:\s*(-?\d+)\]/i);
        const exit = exitMatch ? exitMatch[1] : '?';
        return `[${kind}]${argsTag} → exit ${exit}, ${lines} lines output (collapsed)`;
    }
    if (kind.includes('memoria') || kind.includes('memory')) {
        return `[${kind}]${argsTag} → ${chars.toLocaleString()} chars (collapsed)`;
    }
    if (kind.includes('search_web') || kind.includes('fetch')) {
        return `[${kind}]${argsTag} → ${chars.toLocaleString()} chars web content (collapsed)`;
    }
    return `[${kind}]${argsTag} → ${chars.toLocaleString()} chars, ${lines} lines (collapsed to save tokens)`;
}

/**
 * Pass 3 — anti-thrashing decision. Returns true when a fresh compaction is
 * worth attempting; false when the last two attempts gave <10% reduction
 * each, indicating we've hit the floor and another round is wasted work.
 */
export function shouldCompact(workingMemory: any): boolean {
    if (!workingMemory) return true;
    const ratios: number[] = Array.isArray(workingMemory._compRatios) ? workingMemory._compRatios : [];
    if (ratios.length < 2) return true;
    const lastTwo = ratios.slice(-2);
    return !lastTwo.every(r => r < ANTI_THRASH_THRESHOLD);
}

/**
 * Record the reduction ratio of the latest compaction so shouldCompact() can
 * use it next time. Trims to last 4 entries — older history is irrelevant.
 */
export function recordCompactionRatio(workingMemory: any, beforeChars: number, afterChars: number): void {
    if (!workingMemory || beforeChars <= 0) return;
    workingMemory._compRatios ??= [];
    const ratio = Math.max(0, (beforeChars - afterChars) / beforeChars);
    workingMemory._compRatios.push(Number(ratio.toFixed(3)));
    if (workingMemory._compRatios.length > 4) {
        workingMemory._compRatios = workingMemory._compRatios.slice(-4);
    }
}

/**
 * One-shot helper: run Pass 1 + Pass 2 in sequence, then return the joined
 * text plus a small report `{before, after, savings, ratio}` so the caller
 * can feed it to recordCompactionRatio + telemetry.
 */
export function compressToolResults(
    results: ToolResult[],
    opts: { keepRecent?: number } = {}
): { joined: string; before: number; after: number; ratio: number } {
    const before = results.reduce((n, r) => n + (r?.text?.length || 0), 0);
    const deduped   = dedupToolResults(results);
    const collapsed = smartCollapseOldResults(deduped, opts.keepRecent ?? KEEP_RECENT_RESULTS);
    const joined    = collapsed.map(r => r?.text ?? '').join('\n\n');
    const after     = joined.length;
    const ratio     = before > 0 ? Math.max(0, (before - after) / before) : 0;
    return { joined, before, after, ratio };
}

/**
 * Phase-1 LOCAL dedup for the RAW agent-context string (was the inline Phase 1
 * of `compressContext` in +page.svelte, v1.7.202). Free — no LLM call:
 *   • trims old `--- TOOL RESULTS (step N) ---` blocks to 600 chars,
 *   • collapses duplicate ≥200-char lines,
 *   • truncates very long `[EXECUTION RESULT]` bodies.
 * Only runs once the context is large (>8 KB) AND the loop has a few steps
 * (loop_i ≥ 2); otherwise returns the input unchanged. Faithful extraction —
 * behaviour (incl. quirks) preserved bit-for-bit.
 *
 * v1.7.230 #10 — `tight` mode for LOCAL models. Local engines run small context
 * windows (adaptive_num_ctx tops out where cloud is unbounded), so every char
 * of carried-over context is precious AND re-tokenized each continuation turn
 * with no cross-turn cache for the dynamic part. In tight mode the gate fires
 * far earlier (>3.5 KB, loop_i ≥ 1), old steps keep only 350 chars, and
 * execution output truncates at 2 KB. Cloud path (`tight=false`) is byte-for-byte
 * unchanged. Pairs with the loop cap (#6) so local sessions both loop less AND
 * carry less per turn.
 */
export function localDedupAgentContext(ctx: string, loop_i: number, tight: boolean = false): string {
    const minLen   = tight ? 3500 : 8000;
    const minLoop  = tight ? 1 : 2;
    const stepKeep = tight ? 350 : 600;
    const execCap  = tight ? 2000 : 4000;
    if (!(ctx.length > minLen && loop_i >= minLoop)) return ctx;
    let out = ctx;
    // Trim old steps (keep only `stepKeep` chars each, except recent 2).
    const keepRecent = Math.max(1, loop_i - 2);
    for (let s = 1; s < keepRecent; s++) {
        const stepRe = new RegExp(`--- TOOL RESULTS \\(step ${s}\\) ---\\n([\\s\\S]*?)(?=--- TOOL RESULTS \\(step ${s + 1}\\)|$)`);
        out = out.replace(stepRe, (full: string, body: string) => {
            if (body.length <= stepKeep) return full;
            return `--- TOOL RESULTS (step ${s}, trimmed) ---\n${body.substring(0, stepKeep)}\n[... ${body.length - stepKeep} chars omitted]\n`;
        });
    }
    // Remove duplicate large blocks (>200 chars identical lines).
    const seen = new Map<string, boolean>();
    out = out.replace(/^(.{200,})$/gm, (line: string) => {
        const key = line.trim().substring(0, 300);
        if (seen.has(key)) return '[... duplicate block omitted]';
        seen.set(key, true);
        return line;
    });
    // Truncate very long EXECUTION RESULTs (>execCap).
    // v1.7.204 — fixed a latent dead-no-op: the quantifier was `{4000,?}`, an
    // INVALID quantifier that JS treats as a literal, so this transform never
    // fired (audit-confirmed). Corrected to `{N,}?` (lazy "N or more") so
    // it actually truncates oversized execution output as the comment promised.
    const execRe = new RegExp(`(\\[EXECUTION RESULT\\]\\n)([\\s\\S]{${execCap},}?)(?=\\n\\n---|$)`, 'g');
    out = out.replace(execRe, (_m: string, prefix: string, body: string) => {
        return prefix + body.substring(0, execCap) + '\n[... output truncated for context compression]';
    });
    return out;
}
