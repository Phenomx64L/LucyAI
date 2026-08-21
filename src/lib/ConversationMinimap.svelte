<!--
  ConversationMinimap.svelte — v1.7.98 (Option D4)

  Vertical strip rendered to the right of the chat area that gives a
  bird's-eye view of a long conversation. Each "tick" is one turn,
  color-coded by role (user / lucy / tool / error). The strip also
  paints a translucent "viewport" rectangle over the section currently
  scrolled into view so the operator always knows where they are.

  Why a standalone component
  ──────────────────────────
  ChatThread.svelte is a heavy component with its own DOM mutation
  pipeline (morphdom). We deliberately DON'T re-render or hook into
  its internals — instead we observe its rendered DOM via a
  MutationObserver + IntersectionObserver. The minimap is therefore
  decoupled: any future change in how messages are rendered keeps
  working as long as message bubbles still carry a recognisable
  class.

  Click on a tick → smooth-scrolls the chat-area to that message.
  Drag on the strip → scrubs (for very long threads, this is faster
  than wheel scroll).

  Auto-hides when fewer than MIN_TURNS messages are present — empty
  chats and short threads don't benefit from a minimap.
-->
<script lang="ts">
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { onMount, onDestroy, tick } from 'svelte';

    /** The active tab. We accept the whole object and read .id + .messages
     *  reactively. Passed in (not subscribed via store) because every
     *  parent already has the tabs array in scope. */
    export let tab: any;

    /** Whether THIS tab is currently visible. The component still renders
     *  for inactive tabs but does no DOM observation while hidden. */
    export let isActiveTab: boolean = true;

    /** English locale flag — for tooltips and aria. */
    export let isEN: boolean = false;

    /** Minimum messages before the minimap appears. Short conversations
     *  don't benefit from one and the strip would just be noise. */
    const MIN_TURNS = 8;

    /** Selector for the chat area we're tracking. ChatThread.svelte
     *  renders its scroll container as `.chat-wrap.on .chat-area`. */
    const CHAT_AREA_SELECTOR = '.chat-wrap.on .chat-area';

    /** Selector matching every message bubble inside the chat area.
     *  Both `.msg-user` and `.msg-lucy` are emitted by ChatThread; we
     *  union them so the minimap stays role-aware. */
    const MSG_SELECTOR = '.msg-user, .msg-lucy';

    let strip: HTMLDivElement;
    /** One entry per rendered message bubble. */
    let ticks: Array<{
        id: string;
        kind: 'user' | 'lucy' | 'tool' | 'error';
        topPct: number;       // 0..1 — position on the strip
        heightPct: number;    // 0..1 — minimum 0.6% so ticks stay clickable
        element: HTMLElement; // for scrolling to it
    }> = [];
    /** Viewport indicator rectangle on the strip. */
    let viewportTop = 0;
    let viewportHeight = 0;
    /** Whether the strip is being dragged for scrubbing. */
    let scrubbing = false;

    let chatArea: HTMLElement | null = null;
    let mutationObs: MutationObserver | null = null;
    let scrollHandler: (() => void) | null = null;
    let resizeObs: ResizeObserver | null = null;

    function classifyTick(el: HTMLElement): 'user' | 'lucy' | 'tool' | 'error' {
        // Order matters: error wins over other roles so the operator can
        // spot failed turns at a glance. We probe via classList because
        // ChatThread layers role + state classes independently.
        if (el.classList.contains('msg-error') || el.querySelector('.msg-error-banner')) return 'error';
        if (el.classList.contains('msg-tool') || el.dataset.role === 'tool') return 'tool';
        if (el.classList.contains('msg-user')) return 'user';
        return 'lucy';
    }

    /** Recompute the tick list from the live DOM. Cheap to call: O(N)
     *  in number of messages, no allocations inside the loop besides
     *  the result array. */
    async function recompute() {
        if (!isActiveTab) { ticks = []; return; }
        await tick();
        chatArea = document.querySelector(CHAT_AREA_SELECTOR) as HTMLElement | null;
        if (!chatArea) { ticks = []; return; }
        const total = chatArea.scrollHeight || 1;
        const nodes = Array.from(chatArea.querySelectorAll(MSG_SELECTOR)) as HTMLElement[];
        if (nodes.length < MIN_TURNS) { ticks = []; updateViewport(); return; }
        ticks = nodes.map((el, i) => {
            // offsetTop is relative to the offset parent, which for chat
            // bubbles is the chat-area itself (position:relative in
            // chat-thread.css). Good enough — we don't need pixel
            // perfection on a 6px-wide strip.
            const top = el.offsetTop;
            const h = el.offsetHeight;
            return {
                id: el.id || `tick-${i}`,
                kind: classifyTick(el),
                topPct: top / total,
                // Cap minimum to 0.6% — anything tinier is unclickable.
                heightPct: Math.max(0.006, h / total),
                element: el,
            };
        });
        updateViewport();
    }

    /** Recompute the viewport indicator (the highlighted rectangle).
     *  Called on every scroll + resize tick. Pure math, no DOM writes
     *  beyond reactive variable assignment. */
    function updateViewport() {
        if (!chatArea) { viewportTop = 0; viewportHeight = 0; return; }
        const total = chatArea.scrollHeight || 1;
        viewportTop = chatArea.scrollTop / total;
        viewportHeight = Math.max(0.04, chatArea.clientHeight / total);
    }

    /** Click handler — scrolls the chat area to put `el` at the top of
     *  the viewport with a small lead-in margin. We use scrollIntoView
     *  rather than manual scrollTop so the smooth-scroll feels native. */
    function scrollToTick(t: { element: HTMLElement }) {
        if (!chatArea) return;
        const top = t.element.offsetTop - 40;  // small lead-in
        chatArea.scrollTo({ top, behavior: 'smooth' });
    }

    /** Drag-scrub: while pointer is down on the strip, map vertical
     *  position to scrollTop. Lets you jump 100 turns in one swipe. */
    function onStripPointerDown(e: PointerEvent) {
        if (!chatArea || !strip) return;
        scrubbing = true;
        strip.setPointerCapture(e.pointerId);
        applyScrubAt(e.clientY);
    }
    function onStripPointerMove(e: PointerEvent) {
        if (!scrubbing) return;
        applyScrubAt(e.clientY);
    }
    function onStripPointerUp(e: PointerEvent) {
        if (!scrubbing) return;
        scrubbing = false;
        try { strip.releasePointerCapture(e.pointerId); } catch { /* ignored */ }
    }
    function applyScrubAt(clientY: number) {
        if (!chatArea || !strip) return;
        const r = strip.getBoundingClientRect();
        const pct = Math.max(0, Math.min(1, (clientY - r.top) / r.height));
        chatArea.scrollTop = pct * chatArea.scrollHeight;
    }

    /** Mount observers on the live chat-area DOM. Re-runs whenever the
     *  active tab changes (because ChatThread swaps in a new chat-area
     *  node for the new tab). */
    async function mountObservers() {
        await tick();
        teardownObservers();
        chatArea = document.querySelector(CHAT_AREA_SELECTOR) as HTMLElement | null;
        if (!chatArea) return;

        // 1) Mutation observer — re-collects ticks whenever ChatThread
        //    adds, removes, or restructures bubbles (streaming, replay,
        //    branch, etc.).
        //
        //    v1.7.104 Sprint-4 perf: the audit measured 1-3ms per
        //    animation frame at 60fps on a 200-msg thread during a
        //    stream, sustained ~10-15% main-thread cost.
        //
        //    Two tightenings:
        //    a) Filter at the callback: only fire `recompute()` when a
        //       mutation actually adds/removes a `.msg-user`/.msg-lucy`
        //       *child* of chatArea. Mid-stream {@html} reassignments
        //       fire a flood of subtree mutations on a bubble's INNER
        //       nodes that we don't care about — they don't change
        //       tick count or per-tick offsets.
        //    b) Debounce to 250 ms while a stream is active (detected
        //       by ANY mutation in the last 200 ms — heuristic but
        //       cheap). When the stream stops, the trailing tick
        //       redraws within one rAF.
        let pending = false;
        let lastMutationAt = 0;
        const STREAM_QUIET_MS = 200;
        const STREAM_DEBOUNCE_MS = 250;
        let debounceTimer: ReturnType<typeof setTimeout> | null = null;

        const interestingNode = (n: Node): boolean => {
            if (!(n instanceof HTMLElement)) return false;
            // True if THIS node, or one of its closest ancestors up to
            // chat-area, is a msg-* bubble being added/removed.
            return !!n.matches?.('.msg-user, .msg-lucy')
                || !!n.querySelector?.('.msg-user, .msg-lucy');
        };
        const hasInterestingChange = (records: MutationRecord[]): boolean => {
            for (const r of records) {
                if (r.type !== 'childList') continue;
                if ([...r.addedNodes].some(interestingNode)) return true;
                if ([...r.removedNodes].some(interestingNode)) return true;
            }
            return false;
        };

        mutationObs = new MutationObserver((records) => {
            lastMutationAt = performance.now();
            if (!hasInterestingChange(records)) return;
            const inStream = (performance.now() - lastMutationAt) < STREAM_QUIET_MS;
            if (inStream) {
                if (debounceTimer) clearTimeout(debounceTimer);
                debounceTimer = setTimeout(() => {
                    debounceTimer = null;
                    recompute();
                }, STREAM_DEBOUNCE_MS);
                return;
            }
            if (pending) return;
            pending = true;
            requestAnimationFrame(() => { pending = false; recompute(); });
        });
        mutationObs.observe(chatArea, {
            childList: true, subtree: true,
            characterData: false,
        });

        // 2) Scroll handler — moves the viewport rectangle.
        scrollHandler = () => updateViewport();
        chatArea.addEventListener('scroll', scrollHandler, { passive: true });

        // 3) Resize observer — recompute % positions if the chat-area
        //    itself changes size (sidebar collapse, window resize).
        //    v1.7.104 Sprint-4 perf: coalesce with rAF so a typing-driven
        //    composer-growth doesn't fire recompute every keystroke.
        let resizePending = false;
        resizeObs = new ResizeObserver(() => {
            if (resizePending) return;
            resizePending = true;
            requestAnimationFrame(() => { resizePending = false; recompute(); });
        });
        resizeObs.observe(chatArea);

        await recompute();
    }

    function teardownObservers() {
        if (mutationObs) { mutationObs.disconnect(); mutationObs = null; }
        if (resizeObs)   { resizeObs.disconnect();   resizeObs = null; }
        if (chatArea && scrollHandler) {
            chatArea.removeEventListener('scroll', scrollHandler);
            scrollHandler = null;
        }
    }

    // v1.7.104 Sprint-4 perf: gate on tab.id specifically (not the
    // whole `tab` object) so streaming reassignments of `tab.messages`
    // don't tear down + recreate observers on every token. The audit
    // measured this as a visible perf cliff on long threads.
    let _boundTabId: string | number | null = null;
    $: if (isActiveTab && tab?.id && tab.id !== _boundTabId) {
        _boundTabId = tab.id;
        mountObservers();
    }
    $: if (!isActiveTab) { teardownObservers(); ticks = []; _boundTabId = null; }

    onMount(() => { mountObservers(); });
    onDestroy(() => { teardownObservers(); });
</script>

{#if ticks.length >= MIN_TURNS}
    <div
        class="conv-minimap"
        bind:this={strip}
        on:pointerdown={onStripPointerDown}
        on:pointermove={onStripPointerMove}
        on:pointerup={onStripPointerUp}
        on:pointercancel={onStripPointerUp}
        title={$trad('Mapa de la conversación · click para saltar, arrastra para recorrer')}
        role="navigation"
        aria-label={$trad('Mapa de conversación')}
    >
        <!-- Viewport indicator — soft tinted rectangle over the section
             currently scrolled into view. Pointer-events:none so the
             pointer events fall through to the strip. -->
        <div
            class="conv-minimap-viewport"
            style="top: {viewportTop * 100}%; height: {viewportHeight * 100}%;"
        ></div>

        <!-- Ticks. Click handler stops propagation so it doesn't also
             trigger the scrub-down path. -->
        {#each ticks as t (t.id)}
            <button
                type="button"
                class="conv-minimap-tick conv-minimap-tick-{t.kind}"
                style="top: {t.topPct * 100}%; height: {Math.max(0.6, t.heightPct * 100)}%;"
                on:click|stopPropagation={() => scrollToTick(t)}
                on:pointerdown|stopPropagation
                tabindex="-1"
                aria-label={isEN ? `Jump to ${t.kind} turn` : `Saltar a turno ${t.kind}`}
            ></button>
        {/each}
    </div>
{/if}

<style>
    .conv-minimap {
        position: absolute;
        right: 4px;
        top: 18px;
        bottom: 78px;          /* leave room for the composer dock */
        width: 6px;
        background: rgba(255, 255, 255, 0.02);
        border-radius: 3px;
        z-index: 5;
        cursor: pointer;
        user-select: none;
        transition: background-color var(--motion-fast) var(--ease-out),
                    width var(--motion-fast) var(--ease-out);
    }
    .conv-minimap:hover {
        background: rgba(255, 255, 255, 0.05);
        width: 9px;
    }

    .conv-minimap-viewport {
        position: absolute;
        left: 0; right: 0;
        background: rgba(16, 185, 129, 0.16);
        border-left: 1.5px solid var(--accent);
        border-radius: 2px;
        pointer-events: none;
        transition: top 80ms linear, height 80ms linear;
    }

    .conv-minimap-tick {
        position: absolute;
        left: 0; right: 0;
        border: none;
        background: var(--text-muted);
        padding: 0;
        margin: 0;
        cursor: pointer;
        border-radius: 1px;
        opacity: 0.55;
        transition: opacity var(--motion-fast) var(--ease-out),
                    transform var(--motion-fast) var(--ease-out);
    }
    .conv-minimap-tick:hover {
        opacity: 1;
        transform: scaleX(1.6);
    }

    /* Role colors — match the chat bubble accents so the minimap reads
       as a faithful summary of the thread at a glance. */
    .conv-minimap-tick-user  { background: var(--blue);   }
    .conv-minimap-tick-lucy  { background: var(--accent); }
    .conv-minimap-tick-tool  { background: var(--purple); }
    .conv-minimap-tick-error { background: var(--red);    opacity: 0.85; }

    @media (prefers-reduced-motion: reduce) {
        .conv-minimap,
        .conv-minimap-tick,
        .conv-minimap-viewport { transition: none; }
    }
</style>
