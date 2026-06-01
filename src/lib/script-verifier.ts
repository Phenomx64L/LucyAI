// ── script-verifier.ts — Verify Lucy's code blocks before delivery (v1.7.16) ─
//
// When Lucy finishes streaming a response, the frontend scans her
// markdown for ```<lang> code blocks. For each block whose language is
// supported by the backend `verify_script` command (powershell, js,
// python, bash, json), we:
//
//   1. Call verify_script(language, content).
//   2. If ok → tag with `✓ Verified`.
//   3. If error → call CHEAP LLM tier with the error + original code,
//      ask for a syntax-only fix. Re-verify the fix.
//   4. If the fix verifies → tag with `✓ Auto-fixed` (1 attempt).
//   5. If still failing → tag with `⚠ Unverified` and the error tooltip.
//
// All entirely opt-in via `lucy_verify_scripts_v1` localStorage flag;
// default ON because the cost of a clean-script verify is ~50ms and
// catches the most common error class (typos / missing brackets /
// imports) before the user copies the code.

import { invoke } from '@tauri-apps/api/core';
import { LLM } from '$lib/llm-models';
import { resolveTierWithBreaker } from '$lib/tier-health';
import { safeGetLS, safeSetLSString } from '$lib/safe-ls';

const LS_KEY_ENABLED = 'lucy_verify_scripts_v1';
const LS_KEY_STATS   = 'lucy_verify_stats_v1';

export interface VerifyResult {
    ok:          boolean;
    language:    string;
    error:       string | null;
    line:        number | null;
    elapsed_ms:  number;
    skipped:     boolean;
    skip_reason: string | null;
}

export interface VerifyOutcome {
    /** Final code (auto-fixed if applicable, original otherwise). */
    code:        string;
    language:    string;
    state:       'verified' | 'auto-fixed' | 'unverified' | 'skipped';
    error:       string | null;
    line:        number | null;
    elapsed_ms:  number;
    /** Number of LLM auto-fix attempts spent (0 when clean on first try). */
    attempts:    number;
}

const SUPPORTED_LANGUAGES = new Set([
    'powershell', 'ps1', 'pwsh', 'ps',
    'javascript', 'js', 'node', 'nodejs',
    'python', 'py', 'python3',
    'bash', 'sh', 'shell',
    'json',
]);

// ── Settings ────────────────────────────────────────────────────────────

export function isVerifyEnabled(): boolean {
    return safeGetLS(LS_KEY_ENABLED, '') !== 'off';   // default ON
}

export function setVerifyEnabled(on: boolean): void {
    safeSetLSString(LS_KEY_ENABLED, on ? 'on' : 'off');
}

// ── Telemetry ───────────────────────────────────────────────────────────

interface VerifyStats {
    total_scanned:  number;
    clean_first:    number;
    auto_fixed:     number;
    unverified:     number;
    skipped:        number;
    by_language:    Record<string, number>;
}

function loadStats(): VerifyStats {
    const raw = safeGetLS(LS_KEY_STATS, '');
    const empty: VerifyStats = {
        total_scanned: 0, clean_first: 0, auto_fixed: 0,
        unverified: 0, skipped: 0, by_language: {},
    };
    if (!raw) return empty;
    try { return { ...empty, ...JSON.parse(raw) }; }
    catch { return empty; }
}

function persistStats(s: VerifyStats): void {
    try { safeSetLSString(LS_KEY_STATS, JSON.stringify(s)); }
    catch { /* quota */ }
}

function bumpStats(outcome: VerifyOutcome): void {
    const s = loadStats();
    s.total_scanned += 1;
    if (outcome.state === 'verified')    s.clean_first += 1;
    if (outcome.state === 'auto-fixed')  s.auto_fixed += 1;
    if (outcome.state === 'unverified')  s.unverified += 1;
    if (outcome.state === 'skipped')     s.skipped += 1;
    s.by_language[outcome.language] = (s.by_language[outcome.language] || 0) + 1;
    persistStats(s);
}

export function peekVerifyStats(): VerifyStats {
    return loadStats();
}

export function resetVerifyStats(): void {
    persistStats({
        total_scanned: 0, clean_first: 0, auto_fixed: 0,
        unverified: 0, skipped: 0, by_language: {},
    });
}

// ── Core verify + fix loop ──────────────────────────────────────────────

/**
 * Run verify_script. Returns null on error invoking the command
 * (the caller will fall through to "skipped" semantics).
 */
async function callVerify(language: string, content: string): Promise<VerifyResult | null> {
    try {
        return await invoke<VerifyResult>('verify_script', { language, content });
    } catch (e) {
        // eslint-disable-next-line no-console
        console.warn('[script-verifier] verify_script invoke failed:', e);
        return null;
    }
}

/**
 * Ask the CHEAP tier to patch a script. The prompt is intentionally
 * narrow: only fix the syntax error, do not rewrite, do not invent
 * substitute values. Returns the raw response — caller extracts the
 * code block.
 */
async function llmAutoFix(
    language: string, original: string, errorMsg: string, line: number | null,
): Promise<string | null> {
    const lineHint = line != null ? `(error reported on line ${line})` : '';
    const prompt =
`Fix ONLY the syntax error in this ${language} script ${lineHint}. Do not rewrite,
do not refactor, do not invent placeholder values. Preserve every line that
isn't directly involved in the error.

Respond with the fixed script inside a single \`\`\`${language} ... \`\`\` fence.
No commentary, no explanation, just the corrected code.

ERROR REPORTED:
${errorMsg.slice(0, 1200)}

ORIGINAL SCRIPT:
\`\`\`${language}
${original}
\`\`\``;
    try {
        const model = resolveTierWithBreaker(LLM.CHEAP);
        const reply = await invoke<string>('ask_lucy', {
            prompt, context: '',
            userName: 'lucy-script-verifier',
            runbooksDir: null,
            model,
            images: null,
            lang: 'en',
            hostsJson: null,
            // Bound the response. Most fixes are small deltas; a runaway
            // model can't blow this up beyond a few KB.
            maxTokensOverride: 1024,
        });
        return reply || null;
    } catch (e) {
        // eslint-disable-next-line no-console
        console.warn('[script-verifier] auto-fix LLM call failed:', e);
        return null;
    }
}

/** Extract the first ```lang ... ``` fenced block from an LLM reply. */
function extractFence(reply: string, language: string): string | null {
    const langTokens = [language, language.toLowerCase()];
    for (const tok of langTokens) {
        const re = new RegExp('```\\s*' + tok + '\\s*\\n([\\s\\S]*?)\\n?```', 'i');
        const m = reply.match(re);
        if (m && m[1]) return m[1];
    }
    // Fallback: ANY fence.
    const re2 = /```[a-zA-Z0-9]*\s*\n([\s\S]*?)\n?```/;
    const m2 = reply.match(re2);
    return m2 ? m2[1] : null;
}

/**
 * Verify a single code block and, if it fails, request ONE auto-fix
 * attempt from the CHEAP tier. Returns the final state.
 */
export async function verifyOrFix(
    language: string, content: string,
): Promise<VerifyOutcome> {
    if (!isVerifyEnabled()) {
        return { code: content, language, state: 'skipped', error: null, line: null,
                 elapsed_ms: 0, attempts: 0 };
    }
    if (!SUPPORTED_LANGUAGES.has(language.toLowerCase())) {
        const out: VerifyOutcome = { code: content, language, state: 'skipped',
            error: null, line: null, elapsed_ms: 0, attempts: 0 };
        bumpStats(out);
        return out;
    }
    const first = await callVerify(language, content);
    if (!first) {
        const out: VerifyOutcome = { code: content, language, state: 'skipped',
            error: 'verify command unavailable', line: null, elapsed_ms: 0, attempts: 0 };
        bumpStats(out);
        return out;
    }
    if (first.skipped) {
        const out: VerifyOutcome = { code: content, language: first.language,
            state: 'skipped', error: first.skip_reason, line: null,
            elapsed_ms: first.elapsed_ms, attempts: 0 };
        bumpStats(out);
        return out;
    }
    if (first.ok) {
        const out: VerifyOutcome = { code: content, language: first.language,
            state: 'verified', error: null, line: null,
            elapsed_ms: first.elapsed_ms, attempts: 0 };
        bumpStats(out);
        return out;
    }
    // Syntax error — try ONE auto-fix.
    const fixReply = await llmAutoFix(first.language, content,
        first.error || 'unknown', first.line);
    if (!fixReply) {
        const out: VerifyOutcome = { code: content, language: first.language,
            state: 'unverified', error: first.error, line: first.line,
            elapsed_ms: first.elapsed_ms, attempts: 0 };
        bumpStats(out);
        return out;
    }
    const fixed = extractFence(fixReply, first.language);
    if (!fixed || fixed.trim().length === 0) {
        const out: VerifyOutcome = { code: content, language: first.language,
            state: 'unverified', error: first.error, line: first.line,
            elapsed_ms: first.elapsed_ms, attempts: 1 };
        bumpStats(out);
        return out;
    }
    const second = await callVerify(first.language, fixed);
    if (second && second.ok) {
        const out: VerifyOutcome = { code: fixed, language: first.language,
            state: 'auto-fixed', error: null, line: null,
            elapsed_ms: first.elapsed_ms + (second.elapsed_ms || 0), attempts: 1 };
        bumpStats(out);
        return out;
    }
    // The fix didn't validate — deliver the ORIGINAL with unverified state.
    // Better to show the user the script Lucy actually wrote than to
    // ship a half-fix the user can't reason about.
    const out: VerifyOutcome = { code: content, language: first.language,
        state: 'unverified',
        error: (second?.error || first.error || 'fix failed'),
        line: (second?.line ?? first.line),
        elapsed_ms: first.elapsed_ms + (second?.elapsed_ms || 0), attempts: 1 };
    bumpStats(out);
    return out;
}

// ── HTML rendering helpers ──────────────────────────────────────────────

const BADGE_STYLES: Record<VerifyOutcome['state'], { glyph: string; cls: string; label: string }> = {
    'verified':   { glyph: '✓', cls: 'sv-ok',     label: 'Verified'    },
    'auto-fixed': { glyph: '✓', cls: 'sv-fix',    label: 'Auto-fixed'  },
    'unverified': { glyph: '⚠', cls: 'sv-warn',   label: 'Unverified'  },
    'skipped':    { glyph: '·', cls: 'sv-skip',   label: 'Not checked' },
};

/**
 * Build the tiny badge that prepends a code block in the rendered
 * markdown. The CSS classes live in `+page.svelte` global styles so
 * the sanitizer preserves them.
 */
export function renderBadge(outcome: VerifyOutcome): string {
    const s = BADGE_STYLES[outcome.state];
    const tooltip =
        outcome.state === 'verified'   ? `Syntax check passed (${outcome.elapsed_ms}ms)` :
        outcome.state === 'auto-fixed' ? `Syntax error caught and auto-fixed in 1 attempt` :
        outcome.state === 'unverified' ? `Syntax error: ${(outcome.error || '').slice(0, 200)}` :
                                         (outcome.error || 'Verifier not available for this language');
    const lineHint = outcome.line != null ? ` (line ${outcome.line})` : '';
    return `<span class="sv-badge ${s.cls}" title="${escapeAttr(tooltip + lineHint)}">${s.glyph} ${escapeAttr(s.label)}</span>`;
}

function escapeAttr(s: string): string {
    return String(s).replace(/&/g, '&amp;').replace(/"/g, '&quot;')
                    .replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/**
 * Scan markdown for fenced code blocks and run verify on each. Returns
 * the markdown with the original blocks replaced by `[badge]\n```...```
 * (badge prepended) and with auto-fixed contents inline. Stops at the
 * first 10 blocks per response to keep latency bounded.
 */
export async function verifyAndAnnotateMarkdown(markdown: string): Promise<string> {
    if (!isVerifyEnabled()) return markdown;
    const re = /```\s*([a-zA-Z0-9]+)\s*\n([\s\S]*?)\n?```/g;
    const blocks: Array<{ start: number; end: number; lang: string; code: string }> = [];
    let m: RegExpExecArray | null;
    while ((m = re.exec(markdown)) !== null) {
        blocks.push({
            start: m.index, end: m.index + m[0].length,
            lang: m[1], code: m[2],
        });
        if (blocks.length >= 10) break;
    }
    if (blocks.length === 0) return markdown;
    // Verify each block in parallel (network and process IO are
    // concurrency-friendly).
    const outcomes = await Promise.all(
        blocks.map(b => verifyOrFix(b.lang, b.code)),
    );
    // Stitch back, walking from the end so indices stay valid.
    let out = markdown;
    for (let i = blocks.length - 1; i >= 0; i--) {
        const b = blocks[i];
        const o = outcomes[i];
        const badge = renderBadge(o);
        const newBlock = `${badge}\n\n\`\`\`${b.lang}\n${o.code}\n\`\`\``;
        out = out.slice(0, b.start) + newBlock + out.slice(b.end);
    }
    return out;
}
