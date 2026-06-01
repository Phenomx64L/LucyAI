<!-- ── KeyboardCheatsheet.svelte (v1.4.15) ────────────────────────────────
     Global keyboard reference modal. Opened with Shift+? from anywhere
     except inside an input/textarea (so typing `?` in chat doesn't
     hijack it). Uses bits-ui Dialog for focus trap + portal + Escape.

     Why this is high-impact: Lucy has ~25 working shortcuts. They're
     listed in the welcome tutorial but the user only sees that once.
     A persistent cheatsheet is what Linear/VSCode/GitHub use — and
     it's the lowest-effort way to make existing functionality
     discoverable.

     Props:
       open  — bindable boolean
       isEN  — i18n switch
─────────────────────────────────────────────────────────────────────── -->
<script>
    import { Dialog } from 'bits-ui';
    import { createEventDispatcher } from 'svelte';
    export let open = false;
    export let isEN = false;
    const dispatch = createEventDispatcher();

    function onOpenChange(v) {
        if (open && !v) dispatch('close');
        open = v;
    }

    // Grouped shortcuts. Order matters — most-used first within each
    // section. `keys` is a string array so we can render each chord as
    // a stack of <kbd> elements without parsing.
    $: groups = [
        {
            title: isEN ? 'Navigation' : 'Navegación',
            items: [
                { keys: ['Ctrl', 'P'], desc: isEN ? 'Command palette' : 'Paleta de comandos' },
                { keys: ['Ctrl', 'T'], desc: isEN ? 'New tab' : 'Nueva pestaña' },
                { keys: ['Ctrl', 'W'], desc: isEN ? 'Close tab' : 'Cerrar pestaña' },
                { keys: ['Ctrl', 'Tab'], desc: isEN ? 'Next tab' : 'Siguiente pestaña' },
                { keys: ['Ctrl', 'Shift', 'Tab'], desc: isEN ? 'Previous tab' : 'Anterior pestaña' },
                { keys: ['Ctrl', '1'], desc: isEN ? 'Density: focus' : 'Densidad: focus' },
                { keys: ['Ctrl', '2'], desc: isEN ? 'Density: explore' : 'Densidad: explore' },
                { keys: ['Ctrl', '3'], desc: isEN ? 'Density: war room' : 'Densidad: war room' },
            ],
        },
        {
            title: isEN ? 'In Chat' : 'En el chat',
            items: [
                { keys: ['Enter'], desc: isEN ? 'Send message' : 'Enviar mensaje' },
                { keys: ['Shift', 'Enter'], desc: isEN ? 'New line' : 'Nueva línea' },
                { keys: ['Ctrl', 'Shift', 'Enter'], desc: isEN ? 'Run in background' : 'Ejecutar en background' },
                { keys: ['Esc'], desc: isEN ? 'Cancel agent / close modal' : 'Cancelar agente / cerrar modal' },
                { keys: ['Ctrl', 'L'], desc: isEN ? 'Clear current session' : 'Limpiar sesión actual' },
                { keys: ['Ctrl', 'F'], desc: isEN ? 'Find in chat' : 'Buscar en chat' },
                { keys: ['Tab'], desc: isEN ? 'Autocomplete command flag' : 'Autocompletar flag' },
            ],
        },
        {
            title: isEN ? 'On a message' : 'En un mensaje',
            items: [
                { keys: ['·'], desc: isEN ? 'Pin / unpin' : 'Pin / quitar pin' },
                { keys: ['⌥'], desc: isEN ? 'Branch from here (new tab)' : 'Bifurcar desde aquí (tab nueva)' },
                { keys: ['⏪'], desc: isEN ? 'Replay this turn' : 'Reproducir este turno' },
                { keys: ['Right-click'], desc: isEN ? 'Open context menu' : 'Abrir menú contextual' },
            ],
        },
        {
            title: isEN ? 'Slash commands' : 'Comandos slash',
            items: [
                { keys: ['/help'], desc: isEN ? 'List every slash command' : 'Lista todos los comandos' },
                { keys: ['/snapshot'], desc: isEN ? 'Capture system state' : 'Captura estado del sistema' },
                { keys: ['/diff'], desc: isEN ? 'Compare two snapshots' : 'Compara snapshots' },
                { keys: ['/detective'], desc: isEN ? 'Synthesize F3+F8+F9 forensic query' : 'Sintetiza F3+F8+F9 forense' },
                { keys: ['/recall'], desc: isEN ? 'Search conversation history' : 'Busca en historial' },
                { keys: ['/crystallize'], desc: isEN ? 'Distill session into a crystal' : 'Destila sesión en crystal' },
                { keys: ['/notebook'], desc: isEN ? 'Export tab as .ipynb' : 'Exporta pestaña como .ipynb' },
                { keys: ['/revert'], desc: isEN ? 'Undo last writefile' : 'Revierte última escritura' },
                { keys: ['/chip-stats'], desc: isEN ? 'Predictive-chip engagement' : 'Engagement de chips' },
                { keys: ['/instinct-status'], desc: isEN ? 'Layer 3 patterns banded by confidence' : 'Patrones Layer 3 por bandas de confianza' },
                { keys: ['/evolve'], desc: isEN ? 'Promote recurring patterns into skills' : 'Promueve patrones recurrentes a skills' },
                { keys: ['/polarity'], desc: isEN ? 'Project text onto SUPPORTS↔CONTRADICTS axis' : 'Proyecta texto al eje APOYA↔CONTRADICE' },
                { keys: ['/llm-health'], desc: isEN ? 'LLM tier health, latency & breaker state' : 'Salud de capas LLM, latencia y breaker' },
                { keys: ['/sec-skill'], desc: isEN ? 'Search 213 cybersecurity skills (MITRE / NIST)' : 'Buscar 213 skills de ciberseguridad (MITRE / NIST)' },
                { keys: ['/sec-skill auto'], desc: isEN ? 'Auto-routing status & toggles' : 'Estado y toggles de auto-routing' },
                { keys: ['/sec-skill folder'], desc: isEN ? 'Open user skills folder' : 'Abre la carpeta de skills del usuario' },
                { keys: ['/sec-skill new <id>'], desc: isEN ? 'Generate a starter SKILL.md template' : 'Genera plantilla starter SKILL.md' },
                { keys: ['/anneal'], desc: isEN ? 'Ontology cluster scoring (promote/demote)' : 'Scoring de ontologías (promover/democionar)' },
                { keys: ['/demote-tag'], desc: isEN ? 'Re-tag memories off a failed cluster' : 'Re-etiqueta memorias de un cúmulo fallido' },
                { keys: ['/preset'], desc: isEN ? 'Open the skill-preset picker' : 'Abre el selector de plantillas de habilidad' },
                { keys: ['/frontier-stats'], desc: isEN ? 'Frontier feature telemetry' : 'Telemetría Frontier' },
                { keys: ['/model'], desc: isEN ? 'Switch model (partial match OK)' : 'Cambia modelo (match parcial)' },
                { keys: ['/theme'], desc: isEN ? 'Switch theme' : 'Cambia tema' },
            ],
        },
        {
            title: isEN ? 'System' : 'Sistema',
            items: [
                { keys: ['Shift', '?'], desc: isEN ? 'Open this cheatsheet' : 'Abrir este cheatsheet' },
                { keys: ['Ctrl', 'M'], desc: isEN ? 'Toggle focus mode' : 'Alternar focus mode' },
                { keys: ['Ctrl', 'B'], desc: isEN ? 'Branch tab from last Lucy reply' : 'Bifurcar tab de última respuesta' },
                { keys: ['Ctrl', 'R'], desc: isEN ? 'Search history' : 'Buscar historial' },
            ],
        },
    ];
</script>

<Dialog.Root bind:open onOpenChange={onOpenChange}>
    <Dialog.Portal>
        <Dialog.Overlay class="kb-overlay" />
        <Dialog.Content class="kb-wrap">
            <div class="kb-card">
                <header class="kb-hdr">
                    <Dialog.Title class="kb-title">
                        ⌨ {isEN ? 'Keyboard Shortcuts' : 'Atajos de Teclado'}
                    </Dialog.Title>
                    <Dialog.Description class="kb-sub">
                        {isEN
                            ? 'Press Shift+? from anywhere to bring this back. Esc to close.'
                            : 'Pulsa Shift+? desde cualquier lugar para volver aquí. Esc para cerrar.'}
                    </Dialog.Description>
                    <Dialog.Close class="kb-x" aria-label="Close">✕</Dialog.Close>
                </header>

                <div class="kb-grid">
                    {#each groups as g}
                        <section class="kb-group">
                            <h3 class="kb-grp-title">{g.title}</h3>
                            <ul class="kb-list">
                                {#each g.items as it}
                                    <li class="kb-row">
                                        <span class="kb-chord">
                                            {#each it.keys as k, i}
                                                <kbd>{k}</kbd>{#if i < it.keys.length - 1}<span class="kb-plus">+</span>{/if}
                                            {/each}
                                        </span>
                                        <span class="kb-desc">{it.desc}</span>
                                    </li>
                                {/each}
                            </ul>
                        </section>
                    {/each}
                </div>
            </div>
        </Dialog.Content>
    </Dialog.Portal>
</Dialog.Root>

<style>
    :global(.kb-overlay) {
        position: fixed; inset: 0;
        background: rgba(2, 6, 12, 0.84);
        backdrop-filter: blur(10px);
        z-index: 7000;
        animation: kb-fade .18s ease;
    }
    @keyframes kb-fade { from { opacity: 0; } to { opacity: 1; } }

    :global(.kb-wrap) {
        position: fixed; inset: 0;
        z-index: 7001;
        display: flex; align-items: center; justify-content: center;
        padding: 24px;
        pointer-events: none;
    }
    :global(.kb-wrap > .kb-card) { pointer-events: auto; }
    :global(.kb-card) {
        background: var(--bg2, #0b0e14);
        border: 1px solid var(--bdr, #1a2030);
        border-radius: 16px;
        width: min(880px, 96vw);
        max-height: 88vh;
        overflow-y: auto;
        padding: 22px 26px 28px;
        color: var(--txt, #dde3ea);
        box-shadow: 0 28px 80px rgba(0,0,0,0.7);
        animation: kb-pop .22s cubic-bezier(.34,1.4,.64,1);
    }
    @keyframes kb-pop {
        from { opacity: 0; transform: translateY(8px) scale(.97); }
        to   { opacity: 1; transform: translateY(0)   scale(1); }
    }

    :global(.kb-hdr) {
        position: relative;
        padding-bottom: 14px;
        margin-bottom: 18px;
        border-bottom: 1px solid var(--bdr, #1a2030);
    }
    :global(.kb-title) {
        margin: 0 0 4px;
        font-size: 18px;
        font-weight: 700;
        color: var(--acc, #10b981);
        letter-spacing: .3px;
    }
    :global(.kb-sub) {
        margin: 0;
        font-size: 11.5px;
        color: var(--txt2, #94a3b8);
    }
    :global(.kb-x) {
        position: absolute;
        right: 0; top: 0;
        background: transparent;
        border: 1px solid var(--bdr2, #222c3a);
        color: var(--txt2, #94a3b8);
        width: 28px; height: 28px;
        border-radius: 7px;
        cursor: pointer;
        font-size: 14px;
        transition: .15s;
    }
    :global(.kb-x:hover) {
        background: rgba(255,255,255,.06);
        color: var(--txt, #dde3ea);
        border-color: var(--bdr, #1a2030);
    }

    :global(.kb-grid) {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
        gap: 22px 28px;
    }
    :global(.kb-group) { min-width: 0; }
    :global(.kb-grp-title) {
        margin: 0 0 8px;
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 1.2px;
        text-transform: uppercase;
        color: var(--txt3, #475569);
    }
    :global(.kb-list) {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 5px;
    }
    :global(.kb-row) {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 4px 0;
        font-size: 12px;
    }
    :global(.kb-chord) {
        display: inline-flex;
        align-items: center;
        gap: 2px;
        flex-shrink: 0;
        min-width: 0;
    }
    :global(.kb-chord kbd) {
        font-family: var(--mono, ui-monospace, monospace);
        font-size: 10.5px;
        font-weight: 600;
        line-height: 1;
        color: var(--txt, #dde3ea);
        background: rgba(255,255,255,.06);
        border: 1px solid rgba(255,255,255,.10);
        border-bottom-width: 2px;
        border-radius: 4px;
        padding: 3px 6px;
    }
    :global(.kb-plus) {
        color: var(--txt3, #475569);
        font-size: 11px;
        margin: 0 1px;
    }
    :global(.kb-desc) {
        color: var(--txt2, #94a3b8);
        line-height: 1.4;
    }
</style>
