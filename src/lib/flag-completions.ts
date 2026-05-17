// ── flag-completions.ts — inline flag suggestions for the chat input ─────
//
// Powers the autocomplete popover that appears while the user is typing a
// command in ChatInput. Reads from the `command-signatures.ts` catalog so
// the same hand-curated flag DB drives both safety analysis (Guardian)
// AND completion suggestions — single source of truth.
//
// Design rules
// ------------
// • The popover appears ONLY when the user is in the "flag context": cursor
//   is on a token that starts with `-` (or `--`) AND the command at the
//   start of the line has a known signature. Otherwise no popover at all
//   (would be too noisy on free-form chat).
// • Returns at most 8 suggestions, sorted by:
//     1) destructive flags last (don't auto-complete `rm -rf`)
//     2) prefix-match score
//     3) alphabetical
// • Caller is responsible for rendering UI and applying the chosen
//   completion — this module is purely the search/scoring engine.

import { SIGNATURES, lookupSignature, type FlagSpec } from './command-signatures';

export interface FlagSuggestion {
    /** The flag string ready to be inserted (e.g. "--recursive"). */
    flag: string;
    /** One-line description for the popover tooltip. */
    desc: string;
    /** True if the flag is marked destructive — UI shows red border. */
    destructive: boolean;
    /** Match score, higher is better. For sort + tie-break. */
    score: number;
}

/**
 * Find the first whitespace-separated token of `line`. Returns lowercased
 * for matching against signature names. Empty string if blank.
 */
function commandName(line: string): string {
    const trimmed = line.replace(/^\s+/, '');
    const m = /^(\S+)/.exec(trimmed);
    return m ? m[1].toLowerCase() : '';
}

/**
 * Extract the partial flag token at `cursorPos` inside `line`. Returns the
 * substring (including leading dashes) or `null` if the cursor isn't
 * sitting on a flag-shaped token.
 *
 * Examples (| marks cursor):
 *   "rm -rf /tmp"      cursor at "rm -|f /tmp"   → "-"
 *   "rm --rec|"        →                          → "--rec"
 *   "rm /tmp"          → null
 */
export function activeFlagPartial(line: string, cursorPos: number): string | null {
    if (cursorPos <= 0 || cursorPos > line.length) return null;
    // Walk backwards from cursor until whitespace
    let start = cursorPos;
    while (start > 0 && !/\s/.test(line[start - 1])) start--;
    const token = line.slice(start, cursorPos);
    if (token.startsWith('-')) return token;
    return null;
}

/**
 * Top-N flag suggestions for the current line + cursor position.
 * Returns [] when there's nothing useful to suggest (unknown command,
 * cursor not on flag, etc.).
 */
export function suggestFlags(line: string, cursorPos: number, max = 8): FlagSuggestion[] {
    const partial = activeFlagPartial(line, cursorPos);
    if (partial === null) return [];

    const cmdName = commandName(line);
    if (!cmdName) return [];
    const sig = lookupSignature(cmdName);
    if (!sig || !sig.flags || sig.flags.length === 0) return [];

    const partialLower = partial.toLowerCase();

    const scored: FlagSuggestion[] = sig.flags
        .map((f: FlagSpec) => ({
            flag: f.flag,
            desc: f.desc,
            destructive: !!f.destructive,
            score: scoreMatch(f.flag.toLowerCase(), partialLower),
        }))
        .filter(s => s.score > 0);

    // Sort: non-destructive first, then by score desc, then alpha
    scored.sort((a, b) => {
        if (a.destructive !== b.destructive) return a.destructive ? 1 : -1;
        if (a.score !== b.score)             return b.score - a.score;
        return a.flag.localeCompare(b.flag);
    });

    return scored.slice(0, max);
}

/** Higher = better match. 0 = no match. */
function scoreMatch(flag: string, partial: string): number {
    if (!flag.startsWith('-')) return 0;
    // Exact prefix match — best signal
    if (flag.startsWith(partial)) {
        // Shorter completion is preferred (less typing diff)
        return 100 - (flag.length - partial.length);
    }
    // For `--recursive` to match `-r` shorthand → check if the long form
    // starts with `-` + partial without leading dashes
    const partialBody = partial.replace(/^-+/, '');
    const flagBody    = flag.replace(/^-+/, '');
    if (partialBody && flagBody.startsWith(partialBody)) {
        return 50 - (flagBody.length - partialBody.length);
    }
    return 0;
}

/**
 * Replace the flag partial under the cursor with `chosen`. Returns the
 * new line + new cursor position the caller should set.
 */
export function applyFlagCompletion(
    line: string,
    cursorPos: number,
    chosen: string,
): { line: string; cursor: number } {
    let start = cursorPos;
    while (start > 0 && !/\s/.test(line[start - 1])) start--;
    const before = line.slice(0, start);
    const after  = line.slice(cursorPos);
    // Add a trailing space so the user can immediately type the next token
    const insert = chosen + ' ';
    return { line: before + insert + after, cursor: before.length + insert.length };
}

// ── Diagnostics ─────────────────────────────────────────────────────────
// Returns the count of known signatures + flags for quick health-check
// without exposing the full catalog. Used by the Settings panel to show
// "Flag autocomplete: 28 commands, 142 flags".
export function catalogStats(): { commands: number; flags: number } {
    return {
        commands: SIGNATURES.length,
        flags: SIGNATURES.reduce((acc, s) => acc + s.flags.length, 0),
    };
}
