// ── page/ql-popover.ts ────────────────────────────────────────────────────
//
// Quick-Look popover for tool-card references extracted from `+page.svelte`
// (Sprint D). When the user hovers a `[1]`/`[2]` ref inside a chat message,
// this delegated handler shows a floating preview of the tool output without
// requiring click+scroll.
//
// The popover lives as a SINGLE detached DOM node appended to <body>. We
// attach delegated mouseover/mouseout listeners on `document` so any current
// or future `.tc-ref` anchor is covered without per-message wiring.
//
// `attach()` returns a `detach()` function the caller MUST call on
// component destroy / HMR to avoid stale listeners + orphan DOM nodes.

export interface QlOptions {
    isEN: boolean;
}

export interface QlHandle {
    detach(): void;
    /** The popover element, kept so the caller can clean up if it forgot. */
    el: HTMLDivElement | null;
}

export function attachQlPopover(opts: QlOptions): QlHandle {
    const { isEN } = opts;
    let popover: HTMLDivElement | null = null;
    let anchor: Element | null = null;
    let hideTimer: number | null = null;

    const ensurePop = (): HTMLDivElement => {
        if (popover) return popover;
        const p = document.createElement('div');
        p.className = 'ql-popover';
        p.setAttribute('role', 'tooltip');
        p.setAttribute('aria-hidden', 'true');
        p.innerHTML = `
            <div class="ql-head">
              <span class="ql-icon"></span>
              <span class="ql-label"></span>
              <span class="ql-status"></span>
            </div>
            <pre class="ql-body"></pre>
            <div class="ql-foot">${isEN ? 'Click to expand' : 'Click para expandir'}</div>
        `;
        // Keep popover alive while user hovers it (lets them select text)
        p.addEventListener('mouseenter', () => { if (hideTimer !== null) { clearTimeout(hideTimer); hideTimer = null; } });
        p.addEventListener('mouseleave', scheduleHide);
        document.body.appendChild(p);
        popover = p;
        return p;
    };

    const scheduleHide = (): void => {
        if (hideTimer !== null) clearTimeout(hideTimer);
        hideTimer = window.setTimeout(() => {
            if (popover) {
                popover.classList.remove('ql-show');
                popover.setAttribute('aria-hidden', 'true');
            }
            anchor = null;
            hideTimer = null;
        }, 120);
    };

    const showFor = (a: Element): void => {
        const preview = a.getAttribute('data-preview') || '';
        const label   = a.getAttribute('data-label')   || '';
        const status  = a.getAttribute('data-status')  || 'done';
        const icon    = a.getAttribute('data-icon')    || '';
        if (!preview && !label) return; // nothing useful to show

        const pop = ensurePop();
        (pop.querySelector('.ql-icon')   as HTMLElement).textContent = icon;
        (pop.querySelector('.ql-label')  as HTMLElement).textContent = label;
        const st = pop.querySelector('.ql-status') as HTMLElement;
        st.textContent = status === 'error' ? '✕' : status === 'running' ? '↻' : '✓';
        st.className   = 'ql-status ql-st-' + status;
        (pop.querySelector('.ql-body') as HTMLElement).textContent = preview || (isEN ? '(no output)' : '(sin salida)');
        pop.classList.add('ql-show');
        pop.setAttribute('aria-hidden', 'false');

        // Anchor above; flip below if no vertical room. Clamp horizontally.
        const r = (a as HTMLElement).getBoundingClientRect();
        pop.style.left = '0px';
        pop.style.top  = '0px';
        const pw = pop.offsetWidth;
        const ph = pop.offsetHeight;
        let left = r.left + (r.width / 2) - (pw / 2);
        let top  = r.top - ph - 10;
        left = Math.max(8, Math.min(left, window.innerWidth - pw - 8));
        if (top < 8) top = r.bottom + 10;
        pop.style.left = left + 'px';
        pop.style.top  = top + 'px';
    };

    const onOver = (e: MouseEvent): void => {
        const a = (e.target as Element | null)?.closest?.('a.tc-ref');
        if (!a || a === anchor) return;
        anchor = a;
        if (hideTimer !== null) { clearTimeout(hideTimer); hideTimer = null; }
        showFor(a);
    };
    const onOut = (e: MouseEvent): void => {
        const a = (e.target as Element | null)?.closest?.('a.tc-ref');
        if (!a) return;
        // Don't hide if we moved into the same anchor or into the popover body
        const next = e.relatedTarget as Element | null;
        if (next?.closest && (next.closest('a.tc-ref') === a || next.closest('.ql-popover'))) return;
        scheduleHide();
    };

    document.addEventListener('mouseover', onOver);
    document.addEventListener('mouseout',  onOut);

    return {
        get el() { return popover; },
        detach() {
            document.removeEventListener('mouseover', onOver);
            document.removeEventListener('mouseout',  onOut);
            if (hideTimer !== null) { clearTimeout(hideTimer); hideTimer = null; }
            if (popover && popover.parentNode) {
                try { popover.parentNode.removeChild(popover); } catch { /* ignore */ }
            }
            popover = null;
            anchor = null;
        },
    };
}
