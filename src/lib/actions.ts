/**
 * actions.ts — Svelte actions reutilizables en todos los componentes de Lucy.
 *
 * Uso:
 *   import { focusTrap } from '$lib/actions';
 *   <div use:focusTrap>...</div>
 */

// ── Selectores de elementos que pueden recibir foco ─────────────────────────
const FOCUSABLE_SELECTORS = [
    'a[href]',
    'button:not([disabled])',
    'input:not([disabled])',
    'select:not([disabled])',
    'textarea:not([disabled])',
    '[tabindex]:not([tabindex="-1"])',
].join(',');

/**
 * focusTrap
 * Encierra el foco del teclado dentro del nodo mientras está activo.
 * Tab/Shift+Tab ciclan solo entre los elementos focuseables del nodo.
 * El primer elemento focuseable recibe el foco automáticamente al montar.
 *
 * @example
 *   <div class="modal-box" use:focusTrap>...</div>
 */
export function focusTrap(node: HTMLElement) {
    function getFocusable(): HTMLElement[] {
        return [...node.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTORS)].filter(
            (el) => !el.closest('[hidden]') && el.offsetParent !== null
        );
    }

    function onKeyDown(e: KeyboardEvent) {
        if (e.key !== 'Tab') return;
        const els   = getFocusable();
        if (!els.length) return;
        const first = els[0];
        const last  = els[els.length - 1];
        if (e.shiftKey) {
            if (document.activeElement === first) { e.preventDefault(); last.focus(); }
        } else {
            if (document.activeElement === last)  { e.preventDefault(); first.focus(); }
        }
    }

    // Foco automático en el primer elemento al activar el trap
    requestAnimationFrame(() => {
        const first = getFocusable()[0];
        first?.focus();
    });

    node.addEventListener('keydown', onKeyDown);
    return {
        destroy() { node.removeEventListener('keydown', onKeyDown); },
    };
}

/**
 * countUp
 * Anima el texto de un nodo desde 0 hasta `target` con ease-out cúbico.
 * Llama al `update()` automáticamente cuando cambia el valor.
 *
 * @example
 *   <span use:countUp={{ target: dashMetrics.cpu.global, suffix: '%' }}></span>
 */
export function countUp(
    node: HTMLElement,
    options: { target: number; suffix?: string; prefix?: string; duration?: number; decimals?: number; thousands?: boolean }
) {
    let rafId: number;

    function fmt(v: number, decimals: number, thousands: boolean): string {
        if (decimals > 0) {
            // Use toFixed for clean trailing zeros (e.g. 1.234 → "1.234").
            // Then apply thousands separator on the integer part if requested.
            const fixed = v.toFixed(decimals);
            if (!thousands) return fixed;
            const [intPart, decPart] = fixed.split('.');
            return Number(intPart).toLocaleString() + '.' + decPart;
        }
        const r = Math.round(v);
        return thousands ? r.toLocaleString() : String(r);
    }

    function animate(to: number, prefix: string, suffix: string, duration: number, decimals: number, thousands: boolean) {
        if (rafId) cancelAnimationFrame(rafId);
        const start   = performance.now();
        const step    = (now: number) => {
            const p      = Math.min((now - start) / duration, 1);
            const eased  = 1 - Math.pow(1 - p, 3); // cubic ease-out
            node.textContent = prefix + fmt(to * eased, decimals, thousands) + suffix;
            if (p < 1) rafId = requestAnimationFrame(step);
        };
        rafId = requestAnimationFrame(step);
    }

    const { target, prefix = '', suffix = '%', duration = 900, decimals = 0, thousands = false } = options;
    animate(target, prefix, suffix, duration, decimals, thousands);

    return {
        update(opts: typeof options) {
            const { target: t, prefix: p2 = '', suffix: s = '%', duration: d = 900, decimals: dec = 0, thousands: th = false } = opts;
            animate(t, p2, s, d, dec, th);
        },
        destroy() { if (rafId) cancelAnimationFrame(rafId); },
    };
}
