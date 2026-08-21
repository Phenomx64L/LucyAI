<!-- ── ChatEmptyState.svelte — Hero for tabs with no messages (v1.7.26) ──
     What Cursor / Claude Code / ChatGPT desktop show on a fresh tab:
     just an empty chat scroll area. Vaguely sad.

     What Lucy shows now: a centred composer hero with a warm greeting
     keyed to the time of day, 3 contextual starters generated from the
     operator's persistent memory, and a discoverability hint pointing
     at the slash command system.

     The starters are intentionally HETEROGENEOUS — one workflow
     (continue something open), one navigation (jump to a recent
     surface), one capability tip (showcase a Lucy-only feature). The
     mix communicates depth without information overload. -->
<script lang="ts">
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { createEventDispatcher } from 'svelte';
    // v1.7.32 — Brand mark uses the real Lucy avatar instead of a unicode
    // ✦ that overlapped visually with Google Gemini's brand glyph.
    import { LUCY_AVATAR_DATA_URL as lucyAvatarUrl } from '$lib/assets/lucy-avatar-data';
    // v1.7.32 — Suggestion icons use Tabler (Lucy's existing pack) instead of
    // unicode emojis (🧠⚡⌬◆) which render inconsistently across OSes and
    // don't match the rest of the UI vocabulary.
    import Brain    from '@tabler/icons-svelte/icons/brain';
    import Share3   from '@tabler/icons-svelte/icons/share-3';
    import Bolt     from '@tabler/icons-svelte/icons/bolt';
    import Cpu      from '@tabler/icons-svelte/icons/cpu';
    import Book     from '@tabler/icons-svelte/icons/book-2';
    import Sparkles from '@tabler/icons-svelte/icons/sparkles';
    import ShieldLock from '@tabler/icons-svelte/icons/shield-lock';

    export let userName: string = '';
    export let isEN: boolean = false;
    /** Optional contextual starter suggestions. Each maps a Tabler icon
     *  component to a label + prompt. The host can override but the
     *  defaults below cover the v1.7.32 + v1.7.29 surfaces. */
    export let suggestions: Array<{ icon: any; label: string; prompt: string }> = [];

    const dispatch = createEventDispatcher<{ suggest: string }>();

    // Time-of-day greeting in user language. Defensive on locale —
    // `Date.prototype.getHours()` is local timezone which is what we want.
    function greeting(): string {
        const h = new Date().getHours();
        if (isEN) {
            if (h < 5)  return 'Working late';
            if (h < 12) return 'Good morning';
            if (h < 19) return 'Good afternoon';
            return 'Good evening';
        }
        if (h < 5)  return 'Trabajando tarde';
        if (h < 12) return 'Buenos días';
        if (h < 19) return 'Buenas tardes';
        return 'Buenas noches';
    }

    // Defaults when the host doesn't pass any suggestions. Showcase the
    // distinctive surfaces (skills, memory, runbooks) rather than the
    // generic "help me with X".
    $: defaultSuggestions = isEN ? [
        { icon: Brain,      label: 'Open Memory',      prompt: '/memory' },
        { icon: Share3,     label: 'Knowledge graph',  prompt: '/kg' },
        { icon: Bolt,       label: 'Browse skills',    prompt: '/skills' },
        { icon: ShieldLock, label: 'Security catalog', prompt: '/sec-skill' },
        { icon: Sparkles,   label: 'Capabilities',     prompt: '/capabilities' },
        { icon: Cpu,        label: 'CPU SIMD info',    prompt: '/cpu' },
    ] : [
        { icon: Brain,      label: 'Abrir Memoria',         prompt: '/memory' },
        { icon: Share3,     label: 'Grafo de conocimiento', prompt: '/kg' },
        { icon: Bolt,       label: 'Ver skills',            prompt: '/skills' },
        { icon: ShieldLock, label: 'Catálogo seguridad',    prompt: '/sec-skill' },
        { icon: Sparkles,   label: 'Capacidades',           prompt: '/capabilities' },
        { icon: Cpu,        label: 'Info CPU SIMD',         prompt: '/cpu' },
    ];

    $: rendered = suggestions.length > 0 ? suggestions : defaultSuggestions;
</script>

<div class="ces-wrap">
    <!-- v1.7.32 — Hero mark uses the actual Lucy avatar art (loaded as a
         base64 PNG from $lib/assets/lucy-avatar-data) instead of the
         unicode ✦ that resembled Google Gemini's brand glyph. The
         drop-shadow + breathing animation transfer to the <img>. -->
    <div class="ces-mark-wrap">
        <img class="ces-mark-img" src={lucyAvatarUrl} alt="Lucy" draggable="false" />
    </div>
    <h1 class="ces-title">Lucy</h1>
    <p class="ces-greet">
        {greeting()}{userName ? ', ' : ''}<strong>{userName}</strong>
        <span class="ces-sub">{$trad('· ¿en qué te ayudo?')}</span>
    </p>

    <div class="ces-hint">
        {#if isEN}
            Type below — or use <kbd>/</kbd> to discover commands
        {:else}
            Escribe abajo — o usa <kbd>/</kbd> para descubrir comandos
        {/if}
    </div>

    <div class="ces-suggestions" role="list"
         aria-label={$trad('Sugerencias para arrancar')}>
        {#each rendered as s}
            <!-- v1.7.69 — a11y: a `<button role="listitem">` is invalid
                 (listitem is a non-interactive role and buttons are
                 interactive). Wrap each button in a `<div
                 role="listitem">` so the parent's `role="list"` has
                 valid children while keeping the button as the
                 keyboard-focusable affordance. -->
            <div role="listitem" class="ces-sug-item">
                <button class="ces-sug" type="button"
                        title={s.prompt}
                        on:click={() => dispatch('suggest', s.prompt)}>
                    <span class="ces-sug-glyph">
                        <svelte:component this={s.icon} size={16} stroke={1.8} />
                    </span>
                    <span class="ces-sug-label">{s.label}</span>
                    <span class="ces-sug-cmd">{s.prompt}</span>
                </button>
            </div>
        {/each}
    </div>
</div>

<style>
    .ces-wrap {
        display: flex; flex-direction: column;
        align-items: center; justify-content: center;
        padding: 60px 28px 80px;
        gap: 8px;
        text-align: center;
        font-family: var(--font-ui, ui-sans-serif, system-ui);
        animation: ces-fade-in 280ms ease;
    }

    /* v1.7.32 — Lucy avatar hero mark. The wrap controls the breathing
       animation so the <img> doesn't need a re-encode every frame.

       v1.7.43 — Split the breathing effect into compositor-friendly
       layers:
         • The wrap itself only animates `transform: scale()` — the
           one transform property the compositor can interpolate on
           its own thread with zero repaint cost.
         • The glow lives in a separate `::before` pseudo-element with
           a STATIC box-shadow; we breathe its `opacity` (also
           compositor-only) instead of animating `box-shadow` blur
           radius (which is a per-frame raster pass on a ~128×128
           area — small but the GPU pays it 60 times a second).
       Net effect to the eye is identical; GPU cost drops to near zero. */
    .ces-mark-wrap {
        position: relative;
        width: 56px; height: 56px;
        margin-bottom: 8px;
        border-radius: 14px;
        overflow: visible; /* glow now lives in ::before, needs to escape */
        background: color-mix(in srgb, var(--accent, #10b981) 8%, transparent);
        border: 1px solid color-mix(in srgb, var(--accent, #10b981) 35%, transparent);
        box-shadow: 0 0 0 1px rgba(0,0,0,.35);
        animation: ces-mark-breathe 4s ease-in-out infinite;
        display: flex; align-items: center; justify-content: center;
        will-change: transform;
    }
    .ces-mark-wrap::before {
        content: '';
        position: absolute;
        inset: -2px;
        border-radius: inherit;
        pointer-events: none;
        z-index: -1;
        box-shadow: 0 0 36px 0 color-mix(in srgb, var(--accent, #10b981) 75%, transparent);
        animation: ces-mark-glow 4s ease-in-out infinite;
        will-change: opacity;
    }
    .ces-mark-img {
        width: 100%; height: 100%;
        object-fit: cover;
        display: block;
        border-radius: inherit; /* clip image now that wrap is overflow:visible */
        user-select: none;
        -webkit-user-drag: none;
    }
    @keyframes ces-mark-breathe {
        0%,100% { transform: scale(1);    }
        50%     { transform: scale(1.05); }
    }
    @keyframes ces-mark-glow {
        0%,100% { opacity: 0.55; }
        50%     { opacity: 1;    }
    }
    /* Honour reduced-motion preferences. */
    @media (prefers-reduced-motion: reduce) {
        .ces-mark-wrap         { animation: none; transform: scale(1.02); }
        .ces-mark-wrap::before { animation: none; opacity: 0.7; }
    }

    .ces-title {
        font-size: 30px;
        font-weight: 700;
        letter-spacing: .3px;
        color: var(--txt1, #f1f5f9);
        margin: 0;
        line-height: 1.1;
    }
    .ces-greet {
        font-size: 14px;
        color: var(--txt2, #94a3b8);
        margin: 4px 0 0;
        line-height: 1.4;
    }
    .ces-greet strong { color: var(--txt1, #f1f5f9); font-weight: 600; }
    .ces-sub { opacity: .75; }

    .ces-hint {
        margin-top: 22px;
        font-size: 12px;
        color: var(--txt3, #64748b);
        font-family: var(--mono, ui-monospace, monospace);
    }
    .ces-hint kbd {
        display: inline-block;
        padding: 1px 6px;
        border-radius: 4px;
        border: 1px solid var(--bdr, #334155);
        background: rgba(255, 255, 255, .04);
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 11px;
        color: var(--accent, #10b981);
        margin: 0 2px;
    }

    .ces-suggestions {
        margin-top: 28px;
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
        gap: 8px;
        max-width: 720px;
        width: 100%;
    }

    .ces-sug {
        display: flex; align-items: center; gap: 10px;
        padding: 10px 14px;
        border-radius: 10px;
        border: 1px solid color-mix(in srgb, var(--bdr, #334155) 80%, transparent);
        background: rgba(255, 255, 255, .015);
        color: var(--txt2, #94a3b8);
        font-size: 13px;
        font-family: var(--font-ui, inherit);
        text-align: left;
        cursor: pointer;
        transition: background .12s, border-color .12s, transform .08s, color .12s;
    }
    .ces-sug:hover {
        background: color-mix(in srgb, var(--accent, #10b981) 6%, transparent);
        border-color: color-mix(in srgb, var(--accent, #10b981) 35%, transparent);
        color: var(--txt1, #f1f5f9);
        transform: translateY(-1px);
    }
    .ces-sug:active { transform: translateY(0); }

    .ces-sug-glyph {
        font-size: 16px; line-height: 1;
        flex-shrink: 0;
        color: var(--accent, #10b981);
        display: inline-flex; align-items: center; justify-content: center;
        width: 18px; height: 18px;
    }
    .ces-sug-label {
        flex: 1;
        font-weight: 500;
        white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }
    .ces-sug-cmd {
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 10.5px;
        color: var(--txt3, #64748b);
        opacity: .75;
        flex-shrink: 0;
    }
    .ces-sug:hover .ces-sug-cmd { color: var(--accent, #10b981); opacity: 1; }

    @keyframes ces-fade-in {
        from { opacity: 0; transform: translateY(6px); }
        to   { opacity: 1; transform: none; }
    }
    /* v1.7.69 — the `.ces-mark` selector was a leftover from the
       v1.7.31 unicode-glyph era. The breathing animation now lives on
       `.ces-mark-wrap` (already covered by its own reduced-motion
       block above), so only `.ces-wrap`'s fade-in needs killing here. */
    .ces-sug-item { display: contents; } /* keep grid layout unchanged */
    @media (prefers-reduced-motion: reduce) {
        .ces-wrap { animation: none !important; }
    }

    /* ── Light mode (v1.7.232) ────────────────────────────────────────────
       The base card uses background:rgba(255,255,255,.015) + a translucent
       border — values tuned to "add light" on a dark canvas, which render
       invisible on the light canvas. In light mode give each starter a real
       white surface, a defined border, and a soft shadow so depth comes from
       elevation (the light-UI idiom) instead of a fill that isn't there. */
    :global(:root.light) .ces-sug {
        background: var(--bg4, #fff);
        border-color: var(--bdr2, #94a3b8);
        color: var(--txt2, #475569);
        box-shadow: var(--shadow-card, 0 1px 2px rgba(15,23,42,.05), 0 2px 6px rgba(15,23,42,.06));
    }
    :global(:root.light) .ces-sug:hover {
        background: color-mix(in srgb, var(--accent, #059669) 7%, #fff);
        border-color: var(--accent, #059669);
        color: var(--txt1, #0f172a);
        box-shadow: var(--shadow-pop, 0 4px 14px rgba(5,150,105,.16));
    }
    /* The avatar's dark contact-ring (rgba(0,0,0,.35)) is harsh on white. */
    :global(:root.light) .ces-mark-wrap {
        box-shadow: 0 0 0 1px rgba(15,23,42,.08);
    }
    :global(:root.light) .ces-hint kbd {
        background: var(--bg2, #f1f5f9);
        border-color: var(--bdr2, #94a3b8);
    }
</style>
