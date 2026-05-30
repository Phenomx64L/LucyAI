<!-- ── EmptyState.svelte (v1.4.15) ──────────────────────────────────────
     Reusable empty-state block for "no data yet" panels. Replaces the
     scattered <p>No hay X aún</p> placeholders with something that:
       - Sets correct visual hierarchy (icon → title → description → CTA)
       - Gives the user a concrete next action via the action slot
       - Stays light enough that 5+ on one screen don't dominate

     Props:
       icon         — single string emoji or symbol (large)
       title        — short bolded headline
       description  — optional explainer text
       compact      — set true to squeeze padding for inline use
     Slots:
       action       — optional CTA element (button, link, kbd hint)
─────────────────────────────────────────────────────────────────────── -->
<script>
    export let icon        = '○';
    export let title       = '';
    export let description = '';
    export let compact     = false;
</script>

<div class="empty-state" class:compact>
    <div class="es-ico" aria-hidden="true">{icon}</div>
    {#if title}<div class="es-title">{title}</div>{/if}
    {#if description}<div class="es-desc">{description}</div>{/if}
    {#if $$slots.action}
        <div class="es-action"><slot name="action" /></div>
    {/if}
</div>

<style>
    .empty-state {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        text-align: center;
        padding: 36px 24px;
        gap: 6px;
        color: var(--txt2, #94a3b8);
        animation: es-fade .25s ease;
    }
    .empty-state.compact { padding: 18px 12px; gap: 4px; }
    @keyframes es-fade { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: none; } }

    .es-ico {
        font-size: 38px;
        line-height: 1;
        opacity: 0.55;
        margin-bottom: 4px;
        filter: drop-shadow(0 0 10px color-mix(in srgb, var(--acc, #10b981) 30%, transparent));
    }
    .compact .es-ico { font-size: 26px; }

    .es-title {
        font-size: 13.5px;
        font-weight: 700;
        color: var(--txt, #dde3ea);
        letter-spacing: .2px;
    }

    .es-desc {
        font-size: 11.5px;
        line-height: 1.55;
        max-width: 380px;
        color: var(--txt2, #94a3b8);
    }

    .es-action {
        margin-top: 10px;
        display: flex;
        gap: 8px;
        align-items: center;
        justify-content: center;
        flex-wrap: wrap;
    }
</style>
