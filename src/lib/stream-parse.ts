// ── stream-parse.ts — pure parsers for streamed LLM text ─────────────────────
//
// Extracted from +page.svelte (refactor, v1.7.196) so the streaming parse
// logic — historically a source of subtle bugs — lives in one small, unit-
// tested module instead of nested inside the 10k-line component script.
//
// Everything here is PURE: no Svelte reactivity, no stores, no DOM.

/**
 * Stateful `<THOUGHT>` streamer (was `_makeThoughtStreamer` in +page.svelte,
 * audit M6). Returns a per-stream callback that, given the GROWING accumulated
 * text, emits only the NEWLY-revealed reasoning text between `<THOUGHT>` and
 * `</THOUGHT>`.
 *
 * Cheap by design: finds the open tag once (cached index), uses native
 * `indexOf` (no regex backtracking over the whole accumulator), and STOPS all
 * work permanently once `</THOUGHT>` is seen — every later chunk becomes a
 * single boolean check.
 *
 * @param emit receives only the newly-revealed reasoning delta.
 * @returns a function to call with each new accumulated-text snapshot.
 */
export function makeThoughtStreamer(emit: (delta: string) => void): (acc: string) => void {
    let startIdx = -1;   // index just past "<THOUGHT>"
    let lastLen = 0;     // chars of thought already emitted
    let closed = false;  // </THOUGHT> seen → done forever
    return (acc: string) => {
        if (closed || !acc) return;
        if (startIdx === -1) {
            const open = acc.search(/<THOUGHT>/i);
            if (open === -1) return;            // no thought yet
            startIdx = open + '<THOUGHT>'.length;
        }
        const closeAt = acc.indexOf('</THOUGHT>', startIdx);
        const end = closeAt === -1 ? acc.length : closeAt;
        const cur = acc.slice(startIdx, end);
        if (cur.length > lastLen) {
            emit(cur.slice(lastLen));
            lastLen = cur.length;
        }
        if (closeAt !== -1) closed = true;      // nothing more to stream
    };
}
