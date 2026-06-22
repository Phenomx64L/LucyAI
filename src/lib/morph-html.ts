// ── morph-html.ts — Svelte action that DOM-diffs an HTML string into a host
// element instead of replacing innerHTML wholesale (v1.7.56).
//
// Why this exists
// ---------------
// Lucy's streaming render path mutates `msg.html` ~25-60 times per second.
// Svelte's `{@html msg.html}` binding implements that mutation as
// `parentNode.innerHTML = newHtml`, which is the cheapest possible
// implementation but also the most destructive: every text node, every
// element, every backdrop-filter sibling inside the bubble gets DESTROYED
// and re-created on every chunk. Even with the rAF throttle, the destroy/
// re-create cycle costs paint time and is visible as a low-level shimmer
// across the bubble — the residual flicker the user reported after all the
// streaming-pipeline fixes in v1.7.45-55.
//
// morphdom replaces that wholesale wipe with a tree diff: it compares the
// old and new DOM, mutates only the nodes that actually changed (usually
// the trailing few characters of the last text node), and leaves the rest
// physically untouched. The browser doesn't re-rasterize text, doesn't
// re-blur backdrop-filter layers, doesn't even re-style nodes whose
// outerHTML didn't change. Visible result: text appears to "type itself"
// onto a stable substrate, exactly like ChatGPT or Claude.ai.
//
// Why a Svelte action (not a wrapper component)
// ---------------------------------------------
// `use:morphHtml={msg.html}` is a 1-liner per call site in ChatThread.svelte
// — drop-in for the existing `{@html msg.html}` blocks. The action attaches
// to the host element, sets the initial inner HTML in `mount()`, and
// morphs the children in `update()` whenever msg.html changes. No extra
// component, no extra reactive trigger, no extra prop drilling.

import morphdom from 'morphdom';

/**
 * Svelte action that maintains the host element's inner HTML to match the
 * provided string, using morphdom for in-place DOM diffing on updates.
 *
 * Usage:
 *   <div use:morphHtml={msg.html} class="bubble-content"></div>
 *
 * Initial mount: behaves identically to `innerHTML = html` — single parse,
 * single insert.
 *
 * Subsequent updates: morphdom walks the existing DOM tree against the new
 * one and applies only the minimal mutations needed. Text nodes whose
 * content didn't change are LEFT IN PLACE (not even touched), so the
 * compositor never repaints them. Backdrop-filter siblings around them
 * inherit that stability — they don't recompose either.
 *
 * Edge cases the implementation handles:
 *   • Null / undefined html → treated as empty string. Won't throw.
 *   • Equal-content updates → morphdom's `onBeforeElUpdated` short-circuits
 *     when nodes are equivalent, avoiding pointless walks.
 *   • Post-render DOM decorations (e.g. addCopyBtns wrapping <pre> in
 *     .code-wrap): once the message has been promoted to 'lucy' and the
 *     decorations applied, msg.html stops changing, so this action's
 *     `update()` stops firing. The decorations are never disturbed by a
 *     later morphdom pass.
 */
export function morphHtml(node: HTMLElement, initialHtml: string | null | undefined) {
    // Initial mount — set innerHTML once. morphdom isn't needed here
    // because there's nothing to diff against yet.
    node.innerHTML = initialHtml ?? '';
    // v1.7.57 — Tracks whether this is the first delta-update. On the very
    // first update the entire new tree is "new" from morphdom's POV, and
    // animating every element in would produce a jarring full-bubble flash
    // — exactly what we're trying to avoid. From the second update onward
    // only the truly-incremental nodes are tagged for the fade-in effect.
    let _firstUpdate = true;

    return {
        update(newHtml: string | null | undefined) {
            const safe = newHtml ?? '';
            // v1.7.208 — Pin-to-bottom for scrollable hosts (the reasoning
            // bubble's .reasoning-body is `overflow-y:auto; max-height:340px`).
            // The streaming THOUGHT kept landing BELOW the fold while the box
            // stayed scrolled at the top, so the user saw frozen old reasoning
            // and "couldn't see what Lucy was writing". We capture whether the
            // host was scrolled near its bottom BEFORE morphing, then re-pin it
            // AFTER — but only when the host is itself an overflowing scroll
            // container (display:contents message bodies have no scrollHeight,
            // so this is a no-op for them) and only when the user hadn't
            // scrolled up to read (respect manual scroll-back).
            const isScrollable = node.scrollHeight > node.clientHeight + 4;
            const wasNearBottom = isScrollable &&
                (node.scrollHeight - node.scrollTop - node.clientHeight) < 40;
            // morphdom needs a single root element to morph FROM and TO.
            // We clone the host's shallow shape (same tag, no children) and
            // populate it with the desired new HTML. Then we ask morphdom
            // to morph the host to match — with `childrenOnly: true` it
            // leaves the host's identity, attributes, and event listeners
            // intact and just reconciles the descendant tree.
            const target = node.cloneNode(false) as HTMLElement;
            target.innerHTML = safe;
            const isFirst = _firstUpdate;
            _firstUpdate = false;
            morphdom(node, target, {
                childrenOnly: true,
                onBeforeElUpdated(fromEl, toEl) {
                    // Skip subtrees that are already byte-identical. Cheap
                    // O(n) walk, but it pays off massively for the common
                    // streaming case where only the trailing text node
                    // changed.
                    if (fromEl.isEqualNode(toEl)) return false;
                    return true;
                },
                onNodeAdded(addedNode) {
                    // v1.7.57 — "Gemini-style" token fade-in. Skip on the
                    // first update (everything is "new" then; animating
                    // it all at once would be ugly). For subsequent
                    // updates, tag element nodes that live INSIDE a
                    // `.stream-body` ancestor so the CSS animation
                    // `lucy-token-in` fires once per newly-rendered piece.
                    // Text nodes (nodeType 3) can't have classes; if a
                    // chunk lands inside an existing paragraph and only
                    // grows its trailing text node, no animation fires
                    // for that — which is fine: the residual flicker
                    // morphdom already eliminated covers that case.
                    if (!isFirst && (addedNode as Node).nodeType === 1) {
                        const el = addedNode as Element;
                        let p: Element | null = el.parentElement;
                        while (p) {
                            if (p.classList && p.classList.contains('stream-body')) {
                                el.classList?.add('lucy-new-token');
                                break;
                            }
                            p = p.parentElement;
                        }
                    }
                    return addedNode as Node;
                },
            });
            // Re-pin to the latest content if the user was following along.
            if (wasNearBottom) {
                node.scrollTop = node.scrollHeight;
            }
        },
        destroy() {
            // No event listeners or external state to clean up — morphdom's
            // mutations are all on `node`, which Svelte itself will remove
            // when the parent component unmounts. Leaving the body empty
            // is intentional.
        },
    };
}
