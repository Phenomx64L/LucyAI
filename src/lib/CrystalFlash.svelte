<!--
  CrystalFlash.svelte — v1.7.99 (Option D2)

  Listens for the backend `memory:consolidated` Tauri event (fired by
  housekeeping/crystal_promo when one or more memories are promoted to
  crystals) and plays a brief shimmer over the viewport. Auto-clears
  itself 2 s after each fire so it never accumulates on screen.

  Mount once at the root of +page.svelte. The component renders
  nothing while idle.

  Design intent
  ─────────────
  Memory promotion happens silently in the background every 6 h. Without
  a visual cue the operator never learns the system is alive and
  consolidating. A short, gentle shimmer reads as "Lucy just learned
  something durable" without interrupting whatever the operator is
  doing.

  We deliberately don't toast — toasts are for things you might want to
  act on. Crystal promotion is a self-care event; the right register is
  ambient.
-->
<script lang="ts">
    import { onMount, onDestroy } from 'svelte';
    import { listen, type UnlistenFn } from '@tauri-apps/api/event';

    /** Multiple consecutive consolidation events queue here. We render
     *  the first one, wait its full visible duration, then peek at the
     *  next so the user never misses one. */
    let queue: Array<{ count: number; ts: number; key: number }> = [];
    let current: { count: number; ts: number; key: number } | null = null;
    let unlisten: UnlistenFn | null = null;
    let key = 0;

    /** Length of the shimmer in ms. Long enough to register, short
     *  enough to feel ambient (not interrupt-y). */
    const FLASH_MS = 1900;
    let flushTimer: ReturnType<typeof setTimeout> | null = null;

    function pump() {
        if (current) return;          // already animating
        const next = queue.shift();
        if (!next) return;
        current = next;
        flushTimer = setTimeout(() => {
            current = null;
            flushTimer = null;
            pump();                   // chain to the next queued event
        }, FLASH_MS);
    }

    onMount(async () => {
        try {
            unlisten = await listen<{ count: number; sample_ids: number[]; ts: number }>(
                'memory:consolidated',
                (event) => {
                    const payload = event.payload || ({} as any);
                    const count = Number(payload.count || 0);
                    if (count <= 0) return;
                    queue.push({ count, ts: Date.now(), key: ++key });
                    queue = queue;    // re-trigger Svelte reactivity
                    pump();
                }
            );
        } catch {
            // No Tauri runtime (dev hot-reload edge case) — component
            // is inert but harmless.
        }
    });

    onDestroy(() => {
        if (unlisten) { try { unlisten(); } catch { /* ignored */ } }
        if (flushTimer) { clearTimeout(flushTimer); flushTimer = null; }
    });
</script>

{#if current}
    <!-- Two-layer shimmer:
         · Outer  → vignette glow around the viewport
         · Inner  → pill at the top with the count + crystal glyph
    -->
    <div class="crystal-flash" aria-live="polite" aria-label="Memory consolidated">
        <div class="crystal-flash-vignette"></div>
        <div class="crystal-flash-pill">
            <span class="crystal-flash-glyph">◆</span>
            <span class="crystal-flash-text">
                {current.count === 1
                    ? '1 memory crystallized'
                    : `${current.count} memories crystallized`}
            </span>
        </div>
    </div>
{/if}

<style>
    .crystal-flash {
        position: fixed;
        inset: 0;
        pointer-events: none;
        z-index: 8500;       /* over chat, under modals */
    }

    /* Outer vignette — soft gold ring at the viewport edge that pulses
       once and fades. We use box-shadow inset so we don't allocate a
       new layer (cheaper repaint than radial-gradient). */
    .crystal-flash-vignette {
        position: absolute;
        inset: 0;
        box-shadow:
            inset 0 0 0 2px rgba(245, 158, 11, 0.45),
            inset 0 0 80px 0 rgba(245, 158, 11, 0.18),
            inset 0 0 160px 0 rgba(16, 185, 129, 0.10);
        opacity: 0;
        animation: crystal-vignette 1.9s var(--ease-out, cubic-bezier(.16,1,.3,1)) 1;
    }
    @keyframes crystal-vignette {
        0%   { opacity: 0; }
        14%  { opacity: 0.85; }
        40%  { opacity: 0.55; }
        100% { opacity: 0;    }
    }

    /* Center-top pill with the count. Mounted with a tiny rise-and-settle
       so it reads as "appearing", not "popping in". */
    .crystal-flash-pill {
        position: absolute;
        top: 56px;          /* below the title bar */
        left: 50%;
        transform: translateX(-50%);
        display: inline-flex;
        align-items: center;
        gap: 8px;
        padding: 7px 14px;
        font-family: var(--font-mono, monospace);
        font-size: 12px;
        font-weight: 600;
        letter-spacing: 0.4px;
        color: #fbbf24;
        background: rgba(6, 10, 18, 0.85);
        border: 1px solid rgba(245, 158, 11, 0.45);
        border-radius: 999px;
        box-shadow:
            0 0 0 1px rgba(245, 158, 11, 0.15),
            0 6px 18px -6px rgba(245, 158, 11, 0.45),
            0 0 22px 0 rgba(245, 158, 11, 0.30);
        backdrop-filter: blur(6px);
        animation: crystal-pill 1.9s var(--ease-out, cubic-bezier(.16,1,.3,1)) 1;
    }
    @keyframes crystal-pill {
        0%   { opacity: 0; transform: translate(-50%, -8px); }
        18%  { opacity: 1; transform: translate(-50%,  0);   }
        80%  { opacity: 1; transform: translate(-50%,  0);   }
        100% { opacity: 0; transform: translate(-50%, -4px); }
    }

    .crystal-flash-glyph {
        display: inline-block;
        font-size: 13px;
        line-height: 1;
        color: #fbbf24;
        text-shadow:
            0 0 6px rgba(245, 158, 11, 0.65),
            0 0 14px rgba(245, 158, 11, 0.35);
        animation: crystal-spin 1.9s linear 1;
    }
    @keyframes crystal-spin {
        from { transform: rotate(0deg); }
        to   { transform: rotate(360deg); }
    }

    .crystal-flash-text {
        color: #fde68a;
    }

    @media (prefers-reduced-motion: reduce) {
        .crystal-flash-vignette,
        .crystal-flash-pill,
        .crystal-flash-glyph { animation-duration: 0.6s; }
    }
</style>
