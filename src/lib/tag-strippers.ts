// ── tag-strippers.ts — Pure regex utilities for LLM response cleanup ────────
//
// These patterns appear duplicated ~30+ times across +page.svelte. Centralizing
// them here serves two goals:
//   1) Single source of truth — if we need to fix a regex (e.g. handle a new
//      LLM tag variant), we change it once.
//   2) First step toward the larger P2 refactor (split +page.svelte into
//      domain modules). Pure utilities migrate first since they have no
//      hidden dependencies on app state.
//
// Convention: each function takes a string, returns a string, never mutates.
// Patterns use [\s\S] (not .) to match newlines inside multi-line tags.

// ── EXECUTE family ──────────────────────────────────────────────────────────
const RE_EXECUTE        = /<EXECUTE>[\s\S]*?<\/EXECUTE>/gi;
const RE_EXECUTE_CMD    = /<EXECUTE_CMD>[\s\S]*?<\/EXECUTE_CMD>/gi;
const RE_EXECUTE_WMIC   = /<EXECUTE_WMIC>[\s\S]*?<\/EXECUTE_WMIC>/gi;
const RE_EXECUTE_NETSH  = /<EXECUTE_NETSH>[\s\S]*?<\/EXECUTE_NETSH>/gi;
const RE_EXECUTE_REG    = /<EXECUTE_REG>[\s\S]*?<\/EXECUTE_REG>/gi;
const RE_EXECUTE_CSCRIPT= /<EXECUTE_CSCRIPT>[\s\S]*?<\/EXECUTE_CSCRIPT>/gi;
const RE_EXECUTE_REMOTE = /<EXECUTE_REMOTE[\s\S]*?<\/EXECUTE_REMOTE>/gi;
// Matches any opening EXECUTE-family tag (used to detect intent)
const RE_EXECUTE_ANY    = /<EXECUTE(?:_CMD|_WMIC|_NETSH|_REG|_CSCRIPT|_REMOTE)?\b/i;

// ── Conversational / structural tags ────────────────────────────────────────
const RE_THOUGHT  = /<THOUGHT>[\s\S]*?(?:<\/THOUGHT>|$)/gi;  // tolerant: matches unclosed at end
const RE_REMEMBER = /<REMEMBER[^>]*>[\s\S]*?<\/REMEMBER>/gi;
const RE_LEARN    = /<LEARN>[\s\S]*?<\/LEARN>/gi;
const RE_TOOL     = /<TOOL>[\s\S]*?<\/TOOL>/gi;
const RE_FILE     = /<FILECONTENT>[\s\S]*?<\/FILECONTENT>/gi;
const RE_PLAN     = /<PLAN>[\s\S]*?<\/PLAN>/gi;

// ── Truncation marker ───────────────────────────────────────────────────────
const TRUNCATED_MARKER = '__TRUNCATED__';

/** Remove all EXECUTE family tags (PowerShell, CMD, WMIC, etc) from a string. */
export function stripExecuteTags(s: string): string {
    return s
        .replace(RE_EXECUTE,         '')
        .replace(RE_EXECUTE_CMD,     '')
        .replace(RE_EXECUTE_WMIC,    '')
        .replace(RE_EXECUTE_NETSH,   '')
        .replace(RE_EXECUTE_REG,     '')
        .replace(RE_EXECUTE_CSCRIPT, '')
        .replace(RE_EXECUTE_REMOTE,  '');
}

/** Remove THOUGHT blocks (model's internal reasoning, not for display). */
export function stripThoughtTags(s: string): string {
    return s.replace(RE_THOUGHT, '');
}

/** Remove REMEMBER tags (memory persistence directives). */
export function stripRememberTags(s: string): string {
    return s.replace(RE_REMEMBER, '');
}

/** Remove TOOL tags (file/memory/web tool invocations). */
export function stripToolTags(s: string): string {
    return s.replace(RE_TOOL, '');
}

/** Remove FILECONTENT tags (file content payloads). */
export function stripFileContentTags(s: string): string {
    return s.replace(RE_FILE, '');
}

/** Remove LEARN tags (custom command learning). */
export function stripLearnTags(s: string): string {
    return s.replace(RE_LEARN, '');
}

/** Remove PLAN blocks (interactive plan cards). */
export function stripPlanTags(s: string): string {
    return s.replace(RE_PLAN, '');
}

/** Remove the truncation sentinel emitted by the backend when output is cut. */
export function stripTruncationMarker(s: string): string {
    return s.replace(TRUNCATED_MARKER, '');
}

/**
 * "Strong" cleanup: removes every machine-only directive before showing the
 * text to the user. Use this in the simple-response path (no code-gen intent,
 * no tool execution) and in the stream-display loop.
 *
 * Order matters: strip the truncation marker LAST so that downstream code can
 * detect __TRUNCATED__ on the raw response if needed (call this only after
 * truncation detection).
 */
export function cleanForDisplay(s: string): string {
    return stripFileContentTags(
        stripPlanTags(
            stripToolTags(
                stripThoughtTags(
                    stripRememberTags(
                        stripLearnTags(
                            stripExecuteTags(s)
                        )
                    )
                )
            )
        )
    ).replace(TRUNCATED_MARKER, '').trim();
}

/**
 * For code-generation intent: keep <EXECUTE> blocks but convert them to
 * markdown code fences so the user can copy/paste, rather than executing.
 */
export function executeToCodeBlocks(s: string): string {
    return s
        .replace(/<EXECUTE>([\s\S]*?)<\/EXECUTE>/gi,
                 (_, c) => '\n```powershell\n' + c.trim() + '\n```\n')
        .replace(/<EXECUTE_CMD>([\s\S]*?)<\/EXECUTE_CMD>/gi,
                 (_, c) => '\n```cmd\n' + c.trim() + '\n```\n');
}

/** Quick predicate: does the response contain ANY actionable EXECUTE tag? */
export function hasExecuteIntent(s: string): boolean {
    return RE_EXECUTE_ANY.test(s);
}

/** Quick predicate: does the response contain a <THOUGHT> block? */
export function hasThought(s: string): boolean {
    return /<THOUGHT>/i.test(s);
}

/** Quick predicate: does the response contain a <TOOL> invocation? */
export function hasToolCall(s: string): boolean {
    return /<TOOL>/i.test(s);
}
