// ── cite-chips.ts — Inline cite-chips post-processor (v1.4.4) ──────────────
//
// Lucy's responses are full of unstructured references: file paths
// ("based on `appsettings.json`…"), memory IDs ("memoria #42 confirms…"),
// host names ("@server-prod returned…"). They used to render as plain
// text — readable but inert. This module wraps them in clickable spans
// so a single click opens the file, jumps to the memory, or scopes the
// next prompt to that host.
//
// Pipeline:
//   md → marked.parse → DOMPurify.sanitize → addCiteChips → final HTML
//
// We run AFTER sanitization so the inserted spans use only allowlisted
// attributes (class + data-cite-*). The click handler lives in
// ChatThread.svelte (handleAreaClick) and dispatches a typed event the
// parent maps to actions.
//
// Design decisions:
//   • Pure regex, no LLM call. < 1ms per render.
//   • Skip matches inside <code>, <pre>, <a> — those are already special.
//   • Idempotent: running it twice doesn't double-wrap (gate by class).

/** What kind of entity a chip points at — drives the icon + action. */
export type CiteKind = 'file' | 'memory' | 'host' | 'url';

export interface CiteChipPayload {
    kind:  CiteKind;
    value: string;   // raw entity ref (path, "42", host name…)
    text:  string;   // what was displayed inline (sometimes truncated)
}

// ── Patterns ─────────────────────────────────────────────────────────────
// All regexes are GLOBAL so .replace() walks every occurrence.

// Windows paths (C:\foo\bar.txt) and Unix-style paths (/home/user/file).
// Constraints: extension OR has at least one separator. Avoids matching
// arbitrary words like "config" or "home".
const FILE_PATH_RE = new RegExp(
    [
        // Windows: drive letter + backslash sequences with optional extension
        '(?:[A-Z]:\\\\[\\w\\-. \\\\]+\\.[\\w]{1,8})',
        // POSIX absolute with extension
        '(?:/[\\w\\-./]+\\.[\\w]{1,8})',
        // Relative ./ or ../ paths with extension
        '(?:\\.\\.?/[\\w\\-./]+\\.[\\w]{1,8})',
    ].join('|'),
    'g'
);

// Memory IDs Lucy actually uses: "memoria #42", "memory #42", "[mem:42]".
const MEMORY_RE = /(?:\bmemori[ae]\s*#\s*(\d{1,7}))|(?:\bmemory\s*#\s*(\d{1,7}))|(?:\[mem:(\d{1,7})\])/gi;

// Host names tagged with @. Word boundary to avoid emails like x@y.com.
const HOST_RE = /(?<![\w.])@([A-Za-z][\w\-]{1,40})(?![\w.\-])/g;

// HTTP(S) URLs are matched LAST and only outside other chip kinds.
// We use a conservative pattern; marked already linkifies most so this
// is a backstop for plain URLs that weren't in markdown form.
const URL_RE = /https?:\/\/[^\s<>"']+/g;

// ── Skip-zone protection ─────────────────────────────────────────────────
// We must NOT replace inside <code>, <pre>, <a>, or our own existing
// .cite-chip spans. Strategy: split the HTML into runs at those tags,
// process only "outside" runs, then re-stitch. Cheap one-pass parser.

interface Run {
    /** When true the chunk is INSIDE a forbidden tag and must be left alone. */
    protected: boolean;
    text:      string;
}

function splitRuns(html: string): Run[] {
    const runs: Run[] = [];
    // Match opening tags that begin a skip zone, plus their balanced
    // close. Names are limited to a small whitelist for robustness.
    // The ALSO-protected pattern: span.cite-chip (idempotency guard so
    // we never double-wrap when processed twice).
    const openTagRe   = /<\s*(code|pre|a|svg|style|script)\b[^>]*>/i;
    const citeChipRe  = /<span\s+class="cite-chip[^"]*"[^>]*>/i;
    let cursor = 0;
    while (cursor < html.length) {
        const slice = html.slice(cursor);

        // Check both candidates and take whichever appears first.
        const mTag  = slice.match(openTagRe);
        const mChip = slice.match(citeChipRe);
        const tagIdx  = mTag  && mTag.index  !== undefined ? mTag.index  : -1;
        const chipIdx = mChip && mChip.index !== undefined ? mChip.index : -1;

        let useChip = false;
        let pickIdx = -1;
        if (tagIdx === -1 && chipIdx === -1) {
            runs.push({ protected: false, text: slice });
            break;
        } else if (tagIdx === -1)            { useChip = true;  pickIdx = chipIdx; }
        else if (chipIdx === -1)             { useChip = false; pickIdx = tagIdx; }
        else if (chipIdx < tagIdx)           { useChip = true;  pickIdx = chipIdx; }
        else                                  { useChip = false; pickIdx = tagIdx; }

        if (pickIdx > 0) {
            runs.push({ protected: false, text: slice.slice(0, pickIdx) });
        }

        if (useChip && mChip) {
            // cite-chip wrapper: protect span + content + closing </span>.
            const openLen = mChip[0].length;
            const rest = slice.slice(pickIdx + openLen);
            const closeMatch = rest.match(/<\/\s*span\s*>/i);
            if (!closeMatch || closeMatch.index === undefined) {
                runs.push({ protected: true, text: slice.slice(pickIdx) });
                break;
            }
            const closeEnd = pickIdx + openLen + closeMatch.index + closeMatch[0].length;
            runs.push({ protected: true, text: slice.slice(pickIdx, closeEnd) });
            cursor += closeEnd;
        } else if (mTag) {
            const tagName = mTag[1].toLowerCase();
            const openLen = mTag[0].length;
            const closeRe = new RegExp(`</\\s*${tagName}\\s*>`, 'i');
            const rest = slice.slice(pickIdx + openLen);
            const closeMatch = rest.match(closeRe);
            if (!closeMatch || closeMatch.index === undefined) {
                runs.push({ protected: true, text: slice.slice(pickIdx) });
                break;
            }
            const closeEnd = pickIdx + openLen + closeMatch.index + closeMatch[0].length;
            runs.push({ protected: true, text: slice.slice(pickIdx, closeEnd) });
            cursor += closeEnd;
        }
    }
    return runs;
}

// ── Chip emission ────────────────────────────────────────────────────────

/** Escape HTML attribute values so a path with quotes can't break out. */
function escAttr(s: string): string {
    return s.replace(/&/g, '&amp;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;');
}

function chip(kind: CiteKind, value: string, text: string): string {
    const v = escAttr(value);
    const t = escAttr(text);
    return `<span class="cite-chip cite-${kind}" data-cite-kind="${kind}" data-cite-value="${v}" role="button" tabindex="0">${t}</span>`;
}

function transformOutsideRun(html: string): string {
    // Strategy: each pass replaces matches with ` + index + `
    // placeholders. After all passes, restore placeholders to their
    // generated chips. This prevents later passes from MATCHING INSIDE
    // an earlier pass's output (e.g. URL "https://x.com" containing
    // "/x.com" which would otherwise look like a POSIX file path).
    const stash: string[] = [];
    const stamp = (s: string) => {
        stash.push(s);
        return `${stash.length - 1}`;
    };
    let out = html;

    // URL FIRST — biggest catchall, contains punctuation other patterns might hit.
    out = out.replace(URL_RE, (url) => stamp(chip('url', url, url)));
    // Memory IDs second — bracket form is specific, hard to confuse.
    out = out.replace(MEMORY_RE, (full, a, b, c) => {
        const id = a || b || c;
        return stamp(chip('memory', id, full));
    });
    // File paths next.
    out = out.replace(FILE_PATH_RE, (path) => stamp(chip('file', path, path)));
    // Hosts last — single-token, lowest precedence.
    out = out.replace(HOST_RE, (full, name) => stamp(chip('host', name, full)));

    // Restore placeholders.
    out = out.replace(/(\d+)/g, (_, idx) => stash[+idx] ?? '');
    return out;
}

/**
 * Wrap recognized entity references in clickable cite-chips. Idempotent
 * — running it twice produces the same output. Safe to feed any
 * already-sanitized HTML; we don't inject scripts or arbitrary tags.
 */
export function addCiteChips(html: string): string {
    if (!html || typeof html !== 'string') return html;
    // Cheap exit: if the input has no plausible entity markers, bail.
    if (!/[/\\@#:]|https?:/.test(html)) return html;
    const runs = splitRuns(html);
    let out = '';
    for (const run of runs) {
        out += run.protected ? run.text : transformOutsideRun(run.text);
    }
    return out;
}

// ── Tests live in cite-chips.test.ts (vitest). ───────────────────────────
