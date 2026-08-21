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
─────────────────────────────────────────────────────────────────────── -->
<script>
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { Dialog } from 'bits-ui';
    import { createEventDispatcher } from 'svelte';
    export let open = false;
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
            title: $trad('Navegación'),
            items: [
                { keys: ['Ctrl', 'P'], desc: $trad('Paleta de comandos') },
                { keys: ['Ctrl', 'T'], desc: $trad('Nueva pestaña') },
                { keys: ['Ctrl', 'W'], desc: $trad('Cerrar pestaña') },
                { keys: ['Ctrl', 'Tab'], desc: $trad('Siguiente pestaña') },
                { keys: ['Ctrl', 'Shift', 'Tab'], desc: $trad('Anterior pestaña') },
                { keys: ['Ctrl', '1'], desc: $trad('Densidad: focus') },
                { keys: ['Ctrl', '2'], desc: $trad('Densidad: explore') },
                { keys: ['Ctrl', '3'], desc: $trad('Densidad: war room') },
            ],
        },
        {
            title: $trad('En el chat'),
            items: [
                { keys: ['Enter'], desc: $trad('Enviar mensaje') },
                { keys: ['Shift', 'Enter'], desc: $trad('Nueva línea') },
                { keys: ['Ctrl', 'Shift', 'Enter'], desc: $trad('Ejecutar en background') },
                { keys: ['Esc'], desc: $trad('Cancelar agente / cerrar modal') },
                { keys: ['Ctrl', 'L'], desc: $trad('Limpiar sesión actual') },
                { keys: ['Ctrl', 'F'], desc: $trad('Buscar en chat') },
                { keys: ['Tab'], desc: $trad('Autocompletar flag') },
            ],
        },
        {
            title: $trad('En un mensaje'),
            items: [
                { keys: ['·'], desc: $trad('Pin / quitar pin') },
                { keys: ['⌥'], desc: $trad('Bifurcar desde aquí (tab nueva)') },
                { keys: ['⏪'], desc: $trad('Reproducir este turno') },
                { keys: ['Right-click'], desc: $trad('Abrir menú contextual') },
            ],
        },
        {
            title: $trad('Comandos slash'),
            items: [
                { keys: ['/help'], desc: $trad('Lista todos los comandos') },
                { keys: ['/snapshot'], desc: $trad('Captura estado del sistema') },
                { keys: ['/diff'], desc: $trad('Compara snapshots') },
                { keys: ['/detective'], desc: $trad('Sintetiza F3+F8+F9 forense') },
                { keys: ['/recall'], desc: $trad('Busca en historial') },
                { keys: ['/crystallize'], desc: $trad('Destila sesión en crystal') },
                { keys: ['/notebook'], desc: $trad('Exporta pestaña como .ipynb') },
                { keys: ['/revert'], desc: $trad('Revierte última escritura') },
                { keys: ['/chip-stats'], desc: $trad('Engagement de chips') },
                { keys: ['/instinct-status'], desc: $trad('Patrones Layer 3 por bandas de confianza') },
                { keys: ['/evolve'], desc: $trad('Promueve patrones recurrentes a skills') },
                { keys: ['/polarity'], desc: $trad('Proyecta texto al eje APOYA↔CONTRADICE') },
                { keys: ['/llm-health'], desc: $trad('Salud de capas LLM, latencia y breaker') },
                { keys: ['/verify'], desc: $trad('Estado y toggles del verificador de scripts') },
                { keys: ['/sec-skill'], desc: $trad('Buscar 213 skills de ciberseguridad (MITRE / NIST)') },
                { keys: ['/sec-skill auto'], desc: $trad('Estado y toggles de auto-routing') },
                { keys: ['/sec-skill folder'], desc: $trad('Abre la carpeta de skills del usuario') },
                { keys: ['/sec-skill new <id>'], desc: $trad('Genera plantilla starter SKILL.md') },
                { keys: ['/anneal'], desc: $trad('Scoring de ontologías (promover/democionar)') },
                { keys: ['/demote-tag'], desc: $trad('Re-etiqueta memorias de un cúmulo fallido') },
                { keys: ['/preset'], desc: $trad('Abre el selector de plantillas de habilidad') },
                { keys: ['/frontier-stats'], desc: $trad('Telemetría Frontier') },
                { keys: ['/model'], desc: $trad('Cambia modelo (match parcial)') },
                { keys: ['/theme'], desc: $trad('Cambia tema') },
            ],
        },
        {
            title: $trad('Sistema'),
            items: [
                { keys: ['Shift', '?'], desc: $trad('Abrir este cheatsheet') },
                { keys: ['Ctrl', 'M'], desc: $trad('Alternar focus mode') },
                { keys: ['Ctrl', 'B'], desc: $trad('Bifurcar tab de última respuesta') },
                { keys: ['Ctrl', 'R'], desc: $trad('Buscar historial') },
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
                        ⌨ {$trad('Atajos de Teclado')}
                    </Dialog.Title>
                    <Dialog.Description class="kb-sub">
                        {$trad('Pulsa Shift+? desde cualquier lugar para volver aquí. Esc para cerrar.')}
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
