<!-- SlashTypeahead.svelte — v1.7.91 ──────────────────────────────────────
     Floating typeahead for slash commands.

     Activation
     ──────────
     The host (ChatInput) feeds the current textarea value via `value`
     prop. We activate when:
       • `value.trim()` starts with `/`
       • there's at least one character AFTER the slash (so `/`+Enter
         keeps the existing menu-on-empty behaviour from v1.7.89)
       • the textarea has focus (host gates via `focused` prop)

     Filtering
     ─────────
     We match the typed substring (after `/`) against each command:
       • Prefix match scores highest
       • Substring anywhere scores lower
       • Description substring scores lowest
     Top 8 by score are shown.

     Keyboard
     ────────
     We expose `handleKey(KeyboardEvent): boolean` that returns `true`
     when the key was consumed and the host should NOT propagate it
     further. Wired via `bind:this` from the host so it can call us
     during its own `on:keydown`.

     Selection
     ─────────
     Dispatches `select` with `{ cmd: '/the-chosen-command' }`. The
     host replaces the input value (or just the slash chunk) with the
     command + ' ' and re-focuses the textarea.

     Styling
     ───────
     Inline <style> — class names are scoped by Svelte. No `:global`
     leaks. The popover positions absolutely above the textarea (in
     CSS terms: bottom: 100%) so it can't be clipped by a smaller
     viewport AND it doesn't push the chat content. -->
<script lang="ts">
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { createEventDispatcher, tick } from 'svelte';

    /** Current textarea value. */
    export let value: string = '';
    /** Whether the host's textarea is focused. We only show when it is. */
    export let focused: boolean = false;
    /** EN/ES copy switch. Defaults to ES (Lucy's primary locale). */
    export let isEN: boolean = false;

    const dispatch = createEventDispatcher<{
        select: { cmd: string };
    }>();

    // ── Catalog (kept in sync with slash-commands.ts empty-cmd menu) ──
    //
    // The same 19 commands the v1.7.89 menu surfaces, in the same
    // category order. If you add or remove items here, mirror the
    // change in src/lib/page/slash-commands.ts so the two surfaces stay
    // consistent. (The typeahead doesn't fall back to the central list
    // because it would couple this component to the host's full
    // command tree — heavier than the few commands typeahead actually
    // wants to surface.)
    interface CatItem { cmd: string; desc_en: string; desc_es: string; }
    const CATALOG: CatItem[] = [
        // Memory & Graph
        { cmd: '/memory',      desc_en: 'Open Memory Browser',                desc_es: 'Abrir el explorador de memoria' },
        { cmd: '/kg',          desc_en: 'Open Knowledge Graph',                desc_es: 'Abrir el grafo de conocimiento' },
        { cmd: '/link',        desc_en: 'Manage typed semantic links',         desc_es: 'Gestionar relaciones tipadas' },
        { cmd: '/recall',      desc_en: 'Recall memories by query',            desc_es: 'Recuperar memorias por consulta' },
        { cmd: '/crystals',    desc_en: 'View memory crystals',                desc_es: 'Ver crystals de memoria' },
        { cmd: '/insights',    desc_en: 'View consolidated insights',          desc_es: 'Ver insights consolidados' },
        { cmd: '/consolidate', desc_en: 'Run consolidation now',               desc_es: 'Ejecutar consolidación ahora' },
        // Skills (4 distinct universes — see slash-commands.ts for details)
        { cmd: '/skills',      desc_en: 'Executable skill picker (runbook-style)',  desc_es: 'Picker de skills ejecutables' },
        { cmd: '/preset',      desc_en: 'Behavioural presets (AD, Hyper-V, SQL…)',  desc_es: 'Presets de framing (AD, Hyper-V, SQL…)' },
        { cmd: '/sec-skill',   desc_en: 'Anthropic security / forensic catalog',    desc_es: 'Catálogo de security / forensics' },
        { cmd: '/capabilities',desc_en: 'Self-introspection: skills + MCPs loaded', desc_es: 'Auto-introspección: skills + MCPs' },
        // Routing
        { cmd: '/model',       desc_en: 'Change active model',                 desc_es: 'Cambiar el modelo activo' },
        { cmd: '/route',       desc_en: 'Show last routing decision',          desc_es: 'Última decisión de routing' },
        { cmd: '/serial',      desc_en: 'Toggle fork advisor bypass',          desc_es: 'Bypass del fork advisor' },
        { cmd: '/smart-router',desc_en: 'Toggle smart-router on/off',          desc_es: 'Activar/desactivar smart-router' },
        // Operations
        { cmd: '/proactive',   desc_en: 'List proactive insights',             desc_es: 'Listar insights proactivos' },
        { cmd: '/snapshot',    desc_en: 'Capture a state snapshot',            desc_es: 'Capturar snapshot del sistema' },
        { cmd: '/diff',        desc_en: 'Diff two snapshots',                  desc_es: 'Comparar snapshots' },
        { cmd: '/detective',   desc_en: 'Incident forensics synthesis',        desc_es: 'Síntesis forense de incidente' },
        { cmd: '/runbooks',    desc_en: 'Open runbook list',                   desc_es: 'Abrir la lista de runbooks' },
        // Workspace
        { cmd: '/clear',       desc_en: 'Clear current chat',                  desc_es: 'Limpiar el chat actual' },
        { cmd: '/theme',       desc_en: 'Change visual theme',                 desc_es: 'Cambiar tema visual' },
        { cmd: '/privacy',     desc_en: 'Toggle privacy mode',                 desc_es: 'Modo privacidad' },
        { cmd: '/help',        desc_en: 'Full command reference',              desc_es: 'Referencia completa' },
    ];

    // ── Active state ─────────────────────────────────────────────────
    /** Top-N matches we're currently showing. */
    let matches: CatItem[] = [];
    /** Currently highlighted index for keyboard nav. */
    let selectedIdx: number = 0;

    /** The "active query" — substring after the leading slash, lower-cased,
     *  trimmed of leading whitespace inside the slash chunk. */
    function activeQuery(v: string): string | null {
        const t = v.trimStart();
        if (!t.startsWith('/')) return null;
        // Don't surface for the bare-`/` case — the v1.7.89 menu handles
        // that. We only activate when the operator has typed AT LEAST
        // one character after the slash.
        const after = t.slice(1);
        if (after.length === 0) return null;
        // Stop when a space appears — once the operator has typed the
        // command name and started args, the typeahead is done. (Args
        // belong to that command, not to the typeahead.)
        const firstSpace = after.indexOf(' ');
        if (firstSpace !== -1) return null;
        return after.toLowerCase();
    }

    /** Score a catalog entry against the query. Higher = better.
     *  Returns null if there's no match at all so we can filter it out. */
    function score(item: CatItem, q: string): number | null {
        const cmdLower = item.cmd.slice(1).toLowerCase();   // drop leading "/"
        if (cmdLower.startsWith(q)) return 100 - (cmdLower.length - q.length);
        if (cmdLower.includes(q))   return 50 - cmdLower.indexOf(q);
        const desc = (isEN ? item.desc_en : item.desc_es).toLowerCase();
        if (desc.includes(q))       return 10 - desc.indexOf(q) * 0.1;
        return null;
    }

    function recompute(v: string): void {
        const q = activeQuery(v);
        if (q == null) {
            matches = [];
            selectedIdx = 0;
            return;
        }
        const scored: Array<{ item: CatItem; s: number }> = [];
        for (const item of CATALOG) {
            const s = score(item, q);
            if (s != null) scored.push({ item, s });
        }
        scored.sort((a, b) => b.s - a.s);
        matches = scored.slice(0, 8).map(x => x.item);
        // Clamp selectedIdx when the list shrinks under us.
        if (selectedIdx >= matches.length) selectedIdx = 0;
    }

    $: recompute(value);

    /** Visible when: the host's textarea is focused AND we have ≥1 match. */
    $: visible = focused && matches.length > 0;

    // ── Public API for the host ──────────────────────────────────────
    //
    // `handleKey` is called from ChatInput's `on:keydown` BEFORE the
    // event reaches the textarea's default behaviour. Return true to
    // consume the key (the host must call preventDefault on its end).

    /** Called by the host to route arrow/Enter/Tab/Escape keys to us.
     *  Returns true when consumed. */
    export function handleKey(ev: KeyboardEvent): boolean {
        if (!visible) return false;
        if (ev.key === 'ArrowDown') {
            ev.preventDefault();
            selectedIdx = (selectedIdx + 1) % matches.length;
            return true;
        }
        if (ev.key === 'ArrowUp') {
            ev.preventDefault();
            selectedIdx = (selectedIdx - 1 + matches.length) % matches.length;
            return true;
        }
        if (ev.key === 'Enter' || ev.key === 'Tab') {
            ev.preventDefault();
            const picked = matches[selectedIdx];
            if (picked) pick(picked);
            return true;
        }
        if (ev.key === 'Escape') {
            ev.preventDefault();
            matches = [];
            return true;
        }
        return false;
    }

    function pick(item: CatItem): void {
        dispatch('select', { cmd: item.cmd });
        // Clear matches so the popover hides immediately. The host will
        // also refocus the textarea via its on:select handler.
        matches = [];
        selectedIdx = 0;
    }

    function onClickItem(item: CatItem, ev: MouseEvent): void {
        ev.preventDefault();
        ev.stopPropagation();
        pick(item);
    }
</script>

{#if visible}
<div class="sl-th-pop" role="listbox" aria-label={$trad('Comandos slash')}>
    {#each matches as item, i (item.cmd)}
        <button type="button"
                class="sl-th-row"
                class:on={i === selectedIdx}
                role="option"
                aria-selected={i === selectedIdx}
                tabindex="-1"
                on:mousedown={(e) => onClickItem(item, e)}
                on:mouseenter={() => { selectedIdx = i; }}>
            <span class="sl-th-cmd">{item.cmd}</span>
            <span class="sl-th-desc">{isEN ? item.desc_en : item.desc_es}</span>
        </button>
    {/each}
    <div class="sl-th-foot">
        <span class="sl-th-hint">
            <kbd>↑</kbd><kbd>↓</kbd>
            {$trad('navegar')}
            <kbd>↵</kbd>
            {$trad('seleccionar')}
            <kbd>Esc</kbd>
            {$trad('cerrar')}
        </span>
    </div>
</div>
{/if}

<style>
    /* Positioning anchor is the host element (ChatInput's textarea wrap).
       We place ABOVE the textarea so the popover never gets clipped by a
       short viewport, AND so it doesn't shove the typed text down. */
    .sl-th-pop {
        position: absolute;
        bottom: 100%;
        left: 0;
        right: 0;
        max-height: 320px;
        margin-bottom: 6px;
        background: rgba(8, 14, 24, 0.97);
        border: 1px solid rgba(255, 255, 255, 0.10);
        border-radius: 8px;
        box-shadow: 0 -8px 24px rgba(0, 0, 0, 0.35);
        overflow-y: auto;
        z-index: 60;          /* above composer toolbar */
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 11.5px;
        padding: 4px;
    }
    .sl-th-row {
        appearance: none;
        background: transparent;
        border: none;
        color: var(--txt2, #94a3b8);
        font: inherit;
        text-align: left;
        width: 100%;
        padding: 5px 9px;
        border-radius: 5px;
        cursor: pointer;
        display: flex;
        align-items: baseline;
        gap: 10px;
        transition: background 0.10s, color 0.10s;
    }
    .sl-th-row.on {
        background: rgba(16, 185, 129, 0.10);
        color: var(--txt1, #f1f5f9);
    }
    .sl-th-row:hover {
        background: rgba(16, 185, 129, 0.06);
    }
    .sl-th-cmd {
        color: var(--acc, #10b981);
        font-weight: 700;
        flex-shrink: 0;
        white-space: nowrap;
    }
    .sl-th-row.on .sl-th-cmd {
        color: #34d399;
    }
    .sl-th-desc {
        color: var(--txt2, #94a3b8);
        opacity: 0.75;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .sl-th-foot {
        margin-top: 4px;
        padding: 5px 9px 3px;
        border-top: 1px solid rgba(255, 255, 255, 0.05);
        font-size: 10px;
        color: var(--txt3, #64748b);
        opacity: 0.7;
    }
    .sl-th-hint kbd {
        background: rgba(255, 255, 255, 0.06);
        border: 1px solid rgba(255, 255, 255, 0.10);
        border-radius: 3px;
        padding: 0 4px;
        font-size: 9.5px;
        font-family: inherit;
        margin: 0 3px;
        color: var(--txt2, #94a3b8);
    }
</style>
