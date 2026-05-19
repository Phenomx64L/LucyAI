<!--
  VirtualList — lightweight virtual-scroll component for Svelte.

  Renders only the visible slice of a large list plus a configurable
  overscan buffer above/below.  Ideal for uniform-height rows (trace
  entries, log lines, shell history) where the DOM node count would
  otherwise explode.

  Usage:
    <VirtualList items={myArray} itemHeight={26} let:item let:index>
      <div>{item.label}</div>
    </VirtualList>

  Props:
    items        — the full array (reactivity: will recalc on change)
    itemHeight   — fixed row height in px (estimate is fine)
    overscan     — extra rows rendered above/below viewport (default 8)
    stickyBottom — auto-scroll to bottom when new items arrive (default false)
    onScroll     — optional callback(event, { scrollTop, isAtBottom })

  Exposes:
    scrollToBottom()  — programmatic jump to the end
    scrollToIndex(i)  — programmatic jump to row i
    isAtBottom        — readable: true when within 40px of bottom
-->
<script>
    import { onMount, onDestroy, tick, createEventDispatcher } from 'svelte';

    /** @type {any[]} */
    export let items = [];
    /** Fixed row height estimate in px */
    export let itemHeight = 26;
    /** Extra rows rendered above/below viewport */
    export let overscan = 8;
    /** Auto-scroll to bottom when items.length grows */
    export let stickyBottom = false;
    /** Optional CSS class on the outer wrapper */
    export let wrapClass = '';
    /** Optional CSS style on the outer wrapper */
    export let wrapStyle = '';

    const dispatch = createEventDispatcher();

    let viewport;
    let viewportHeight = 400;
    let scrollTop = 0;
    let isAtBottom = true;
    let _prevLen = 0;

    // Derived: how many items fit in the viewport
    $: totalHeight = items.length * itemHeight;
    $: startRaw = Math.floor(scrollTop / itemHeight) - overscan;
    $: start = Math.max(0, startRaw);
    $: endRaw = Math.ceil((scrollTop + viewportHeight) / itemHeight) + overscan;
    $: end = Math.min(items.length, endRaw);
    $: visibleSlice = items.slice(start, end);
    $: topPad = start * itemHeight;
    $: bottomPad = Math.max(0, (items.length - end) * itemHeight);

    // Sticky-bottom: when items grow, auto-scroll if we were at the bottom.
    $: if (stickyBottom && items.length > _prevLen && isAtBottom && viewport) {
        _prevLen = items.length;
        scheduleScrollToBottom();
    }
    // Also track length to avoid infinite loops
    $: _prevLen = items.length;

    let _rafId = 0;
    function scheduleScrollToBottom() {
        if (_rafId) return;
        _rafId = requestAnimationFrame(() => {
            _rafId = 0;
            if (viewport) viewport.scrollTop = viewport.scrollHeight;
        });
    }

    function onScroll() {
        if (!viewport) return;
        scrollTop = viewport.scrollTop;
        const distBottom = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight;
        isAtBottom = distBottom < 40;
        dispatch('scroll', { scrollTop, isAtBottom });
    }

    /** Programmatic scroll to bottom */
    export function scrollToBottom() {
        if (viewport) {
            viewport.scrollTop = viewport.scrollHeight;
            isAtBottom = true;
        }
    }

    /** Programmatic scroll to a specific item index */
    export function scrollToIndex(i) {
        if (viewport && i >= 0 && i < items.length) {
            viewport.scrollTop = i * itemHeight;
        }
    }

    /** Whether the user is scrolled to the bottom */
    export function getIsAtBottom() { return isAtBottom; }

    // Measure viewport on mount
    let _ro;
    onMount(() => {
        if (viewport) {
            viewportHeight = viewport.clientHeight;
            _ro = new ResizeObserver(entries => {
                for (const e of entries) viewportHeight = e.contentRect.height;
            });
            _ro.observe(viewport);
        }
    });
    onDestroy(() => {
        if (_rafId) cancelAnimationFrame(_rafId);
        _ro?.disconnect();
    });
</script>

<div
    class="vl-viewport {wrapClass}"
    style={wrapStyle}
    bind:this={viewport}
    on:scroll={onScroll}
>
    <div class="vl-spacer" style="height:{totalHeight}px; position:relative;">
        <div class="vl-content" style="position:absolute; top:{topPad}px; left:0; right:0;">
            {#each visibleSlice as item, i (item.id ?? item._vlKey ?? (start + i))}
                <slot {item} index={start + i} />
            {/each}
        </div>
    </div>
</div>

<style>
    .vl-viewport {
        overflow-y: auto;
        flex: 1;
        min-height: 0;
    }
</style>
