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
    import { createEventDispatcher } from 'svelte';

    export let userName: string = '';
    export let isEN: boolean = false;
    /** Optional contextual starter suggestions. Each is one short imperative
     *  phrase that becomes the input value (does NOT submit immediately). */
    export let suggestions: Array<{ glyph: string; label: string; prompt: string }> = [];

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
        { glyph: '🧠', label: 'Open Memoria',   prompt: '/memory' },
        { glyph: '⚡', label: 'Browse skills',   prompt: '/sec-skill' },
        { glyph: '📜', label: 'Last runbook',    prompt: '/runbooks' },
        { glyph: '◆',  label: 'CPU SIMD info',   prompt: '/cpu' },
    ] : [
        { glyph: '🧠', label: 'Abrir Memoria',   prompt: '/memory' },
        { glyph: '⚡', label: 'Ver skills',       prompt: '/sec-skill' },
        { glyph: '📜', label: 'Último runbook',  prompt: '/runbooks' },
        { glyph: '◆',  label: 'Info CPU SIMD',   prompt: '/cpu' },
    ];

    $: rendered = suggestions.length > 0 ? suggestions : defaultSuggestions;
</script>

<div class="ces-wrap">
    <div class="ces-mark">✦</div>
    <h1 class="ces-title">Lucy</h1>
    <p class="ces-greet">
        {greeting()}{userName ? ', ' : ''}<strong>{userName}</strong>
        <span class="ces-sub">{isEN ? '· what can I help with?' : '· ¿en qué te ayudo?'}</span>
    </p>

    <div class="ces-hint">
        {#if isEN}
            Type below — or use <kbd>/</kbd> to discover commands
        {:else}
            Escribe abajo — o usa <kbd>/</kbd> para descubrir comandos
        {/if}
    </div>

    <div class="ces-suggestions" role="list"
         aria-label={isEN ? 'Suggested starters' : 'Sugerencias para arrancar'}>
        {#each rendered as s}
            <button class="ces-sug" type="button" role="listitem"
                    title={s.prompt}
                    on:click={() => dispatch('suggest', s.prompt)}>
                <span class="ces-sug-glyph">{s.glyph}</span>
                <span class="ces-sug-label">{s.label}</span>
                <span class="ces-sug-cmd">{s.prompt}</span>
            </button>
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

    .ces-mark {
        font-size: 36px; line-height: 1;
        color: var(--accent, #10b981);
        filter: drop-shadow(0 0 18px color-mix(in srgb, var(--accent, #10b981) 50%, transparent));
        animation: ces-mark-breathe 4s ease-in-out infinite;
        margin-bottom: 4px;
    }
    @keyframes ces-mark-breathe {
        0%,100% { transform: scale(1);    filter: drop-shadow(0 0 18px color-mix(in srgb, var(--accent, #10b981) 40%, transparent)); }
        50%     { transform: scale(1.08); filter: drop-shadow(0 0 28px color-mix(in srgb, var(--accent, #10b981) 70%, transparent)); }
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
    @media (prefers-reduced-motion: reduce) {
        .ces-wrap, .ces-mark { animation: none !important; }
    }
</style>
