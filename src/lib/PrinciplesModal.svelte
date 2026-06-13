<script>
    // ── PrinciplesModal — manage Lucy's behavioral rules (Maestro-inspired)
    //
    // Backend: src-tauri/src/commands/principles.rs
    // Calls: list_principles / save_principle / update_principle /
    //        delete_principle
    //
    // The list shows ALL principles (regardless of scope) so the user can
    // toggle, edit, or delete any rule. New principles default to global
    // scope + priority 100. Higher-priority rules (lower number) bubble
    // to the top of the rendered prompt block.

    import { createEventDispatcher, onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { focusTrap } from '$lib/actions';
    import { staggerIn } from '$lib/stagger';
    import Bookmark from '@tabler/icons-svelte/icons/bookmark';

    import Plus from '@tabler/icons-svelte/icons/plus';

    import Trash2 from '@tabler/icons-svelte/icons/trash';

    import Check from '@tabler/icons-svelte/icons/check';

    import X from '@tabler/icons-svelte/icons/x';

    import Edit2 from '@tabler/icons-svelte/icons/edit';

    import ShieldCheck from '@tabler/icons-svelte/icons/shield-check';
    const dispatch = createEventDispatcher();

    export let isOpen = false;
    export let isEN   = false;

    // Local state
    let principles = [];
    let loading    = false;
    let error      = '';
    let formOpen   = false;
    let editingId  = null;
    let draft      = { name: '', rule: '', scope: '', priority: 100 };

    const t = isEN
        ? {
            title:        'Behavioral Principles',
            subtitle:     'Rules that Lucy follows silently in every turn',
            empty:        'No principles defined yet.',
            empty_hint:   'Create one to enforce house rules across all your sessions (e.g. "always validate Get-* before Set-*").',
            add:          'Add principle',
            edit:         'Edit',
            delete:       'Delete',
            cancel:       'Cancel',
            save:         'Save',
            name:         'Name (short)',
            rule:         'Rule (full text Lucy reads)',
            scope:        'Scope (host id, project tag, or "global")',
            priority:     'Priority (1-1000, lower = higher priority)',
            globalLabel:  'global',
            disabled:     'disabled',
            confirmDel:   'Delete this principle?',
            saved:        'Principle saved',
            deleted:      'Principle deleted',
        }
        : {
            title:        'Principios de Comportamiento',
            subtitle:     'Reglas que Lucy sigue silenciosamente en cada turno',
            empty:        'Aún no hay principios definidos.',
            empty_hint:   'Crea uno para imponer reglas de la casa en todas tus sesiones (ej. "valida con Get-* antes de cualquier Set-*").',
            add:          'Agregar principio',
            edit:         'Editar',
            delete:       'Eliminar',
            cancel:       'Cancelar',
            save:         'Guardar',
            name:         'Nombre (corto)',
            rule:         'Regla (texto completo que Lucy lee)',
            scope:        'Alcance (host id, proyecto, o "global")',
            priority:     'Prioridad (1-1000, menor = más alta)',
            globalLabel:  'global',
            disabled:     'desactivado',
            confirmDel:   '¿Eliminar este principio?',
            saved:        'Principio guardado',
            deleted:      'Principio eliminado',
        };

    function toast(msg, type = 'info') { dispatch('toast', { msg, type }); }

    async function load() {
        loading = true; error = '';
        try {
            principles = await invoke('list_principles', { scope: null }) || [];
        } catch (e) { error = String(e); }
        loading = false;
    }

    function openNew() {
        editingId = null;
        draft = { name: '', rule: '', scope: '', priority: 100 };
        formOpen = true;
    }
    function openEdit(p) {
        editingId = p.id;
        draft = { name: p.name, rule: p.rule, scope: p.scope || '', priority: p.priority };
        formOpen = true;
    }
    function cancelForm() { formOpen = false; editingId = null; }

    async function save() {
        if (!draft.name?.trim() || !draft.rule?.trim()) {
            toast(isEN ? 'Name and rule required' : 'Nombre y regla son obligatorios', 'warn'); return;
        }
        const scope = (draft.scope || '').trim().toLowerCase() === 'global' || !draft.scope?.trim()
            ? null : draft.scope.trim();
        const priority = Math.max(1, Math.min(1000, Number(draft.priority) || 100));
        try {
            if (editingId) {
                await invoke('update_principle', {
                    id: editingId,
                    name: draft.name.trim(),
                    rule: draft.rule.trim(),
                    scope: [scope],     // double-Option: [null] = clear, [val] = set
                    priority,
                    enabled: null,
                });
            } else {
                await invoke('save_principle', {
                    name: draft.name.trim(),
                    rule: draft.rule.trim(),
                    scope,
                    priority,
                });
            }
            toast(t.saved, 'ok');
            formOpen = false;
            editingId = null;
            await load();
        } catch (e) {
            toast(String(e).slice(0, 120), 'error');
        }
    }

    async function toggle(p) {
        try {
            await invoke('update_principle', {
                id: p.id, name: null, rule: null, scope: null, priority: null,
                enabled: !p.enabled,
            });
            await load();
        } catch (e) { toast(String(e).slice(0, 120), 'error'); }
    }

    async function remove(p) {
        // v1.7.17 — in-app dialog replaces native window.confirm.
        const { lucyConfirm } = await import('$lib/dialog-service');
        if (!await lucyConfirm(t.confirmDel,
            { tone: 'danger', description: p.name, confirmLabel: 'Delete' })) return;
        try {
            await invoke('delete_principle', { id: p.id });
            toast(t.deleted, 'ok');
            await load();
        } catch (e) { toast(String(e).slice(0, 120), 'error'); }
    }

    function close() { isOpen = false; dispatch('close'); }

    $: if (isOpen) load();
    onMount(load);
</script>

{#if isOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="pm-overlay" role="presentation" on:click={close}
         on:keydown={(e) => { if (e.key === 'Escape') close(); }}>
        <div class="pm-box" role="dialog" aria-modal="true" tabindex={-1}
             use:focusTrap on:click|stopPropagation>
            <div class="pm-hdr">
                <div class="pm-hdr-l">
                    <span class="pm-hdr-ico"><Bookmark size={18} strokeWidth={2} /></span>
                    <div>
                        <h3>{t.title}</h3>
                        <span class="pm-sub">{t.subtitle}</span>
                    </div>
                </div>
                <button class="pm-close" type="button" on:click={close} aria-label="Close">✕</button>
            </div>

            <div class="pm-body">
                {#if formOpen}
                    <div class="pm-form">
                        <label class="pm-field">
                            <span class="pm-lbl">{t.name}</span>
                            <input type="text" bind:value={draft.name} maxlength="120" />
                        </label>
                        <label class="pm-field">
                            <span class="pm-lbl">{t.rule}</span>
                            <textarea rows="3" bind:value={draft.rule} maxlength="1000"></textarea>
                        </label>
                        <div class="pm-row">
                            <label class="pm-field" style="flex:2;">
                                <span class="pm-lbl">{t.scope}</span>
                                <input type="text" bind:value={draft.scope} placeholder="global" />
                            </label>
                            <label class="pm-field" style="flex:1;">
                                <span class="pm-lbl">{t.priority}</span>
                                <input type="number" min="1" max="1000" bind:value={draft.priority} />
                            </label>
                        </div>
                        <div class="pm-form-foot">
                            <button class="pm-btn pm-cancel" on:click={cancelForm}>{t.cancel}</button>
                            <button class="pm-btn pm-save" on:click={save}>
                                <Check size={13} strokeWidth={2.5} /> {t.save}
                            </button>
                        </div>
                    </div>
                {/if}

                {#if !formOpen}
                    <button class="pm-add" on:click={openNew}>
                        <Plus size={14} strokeWidth={2.5} /> {t.add}
                    </button>
                {/if}

                {#if loading}
                    <div class="pm-loading">…</div>
                {:else if error}
                    <div class="pm-error">{error}</div>
                {:else if principles.length === 0}
                    <div class="pm-empty">
                        <ShieldCheck size={32} strokeWidth={1.4} style="opacity:.4;" />
                        <p>{t.empty}</p>
                        <span>{t.empty_hint}</span>
                    </div>
                {:else}
                    <div class="pm-list">
                        {#each principles as p, i (p.id)}
                            <div class="pm-item" class:disabled={!p.enabled} in:staggerIn={{ index: i, step: 24 }}>
                                <div class="pm-item-l">
                                    <div class="pm-item-hdr">
                                        <span class="pm-name">{p.name}</span>
                                        <span class="pm-tags">
                                            <span class="pm-tag pm-tag-scope">{p.scope || t.globalLabel}</span>
                                            <span class="pm-tag pm-tag-pri">P{p.priority}</span>
                                            {#if !p.enabled}
                                                <span class="pm-tag pm-tag-off">{t.disabled}</span>
                                            {/if}
                                        </span>
                                    </div>
                                    <p class="pm-rule">{p.rule}</p>
                                </div>
                                <div class="pm-item-r">
                                    <button class="pm-mini" title={p.enabled ? 'Disable' : 'Enable'} on:click={() => toggle(p)}>
                                        {#if p.enabled}<Check size={13} strokeWidth={2.5} />{:else}<X size={13} strokeWidth={2.5} />{/if}
                                    </button>
                                    <button class="pm-mini" title={t.edit} on:click={() => openEdit(p)}>
                                        <Edit2 size={13} strokeWidth={2} />
                                    </button>
                                    <button class="pm-mini pm-del" title={t.delete} on:click={() => remove(p)}>
                                        <Trash2 size={13} strokeWidth={2} />
                                    </button>
                                </div>
                            </div>
                        {/each}
                    </div>
                {/if}
            </div>
        </div>
    </div>
{/if}

<style>
    .pm-overlay {
        position: fixed; inset: 0; z-index: 8500;
        background: rgba(2, 6, 12, 0.74);
        backdrop-filter: blur(4px);
        display: flex; align-items: center; justify-content: center;
        animation: fade-in 200ms ease-out;
    }
    .pm-box {
        /* Depth: faint violet top glow (principles identity) over the base. */
        background:
            radial-gradient(120% 55% at 50% -6%, rgba(167,139,250,.08) 0%, transparent 56%),
            var(--bg-card, #161b22);
        border: 1px solid var(--border-light, #334155);
        border-top: 3px solid #a78bfa;
        border-radius: 12px;
        width: 640px; max-width: 92vw; max-height: 86vh;
        display: flex; flex-direction: column;
        box-shadow: 0 24px 64px rgba(0,0,0,0.6),
                    0 -1px 28px -10px rgba(167,139,250,.45);
        outline: none;
        animation: pm-pop 220ms cubic-bezier(0.34, 1.56, 0.64, 1);
    }
    @keyframes pm-pop {
        from { opacity: 0; transform: scale(0.96) translateY(8px); }
        to   { opacity: 1; transform: scale(1) translateY(0); }
    }
    .pm-hdr {
        display: flex; align-items: center; justify-content: space-between;
        padding: 14px 18px;
        border-bottom: 1px solid var(--border-color, #1e293b);
    }
    .pm-hdr-l { display: flex; align-items: center; gap: 10px; color: #c4b5fd; }
    .pm-hdr-ico {
        display: inline-flex; align-items: center; justify-content: center;
        width: 36px; height: 36px; border-radius: 11px; flex-shrink: 0;
        background: rgba(167,139,250,.12);
        border: 1px solid rgba(167,139,250,.28);
        box-shadow: 0 0 16px -4px rgba(167,139,250,.5);
    }
    .pm-hdr h3 { margin: 0; font-size: 13.5px; font-weight: 600; color: var(--text-bright, #f1f5f9); }
    .pm-sub { font-size: 10.5px; color: var(--text-muted, #94a3b8); }
    .pm-close {
        background: transparent; border: none; color: var(--text-muted, #64748b);
        font-size: 18px; cursor: pointer; padding: 0 4px; line-height: 1;
    }
    .pm-close:hover { color: var(--text-bright, #f1f5f9); }

    .pm-body {
        padding: 16px 18px; overflow-y: auto; flex: 1;
        display: flex; flex-direction: column; gap: 12px;
    }
    .pm-loading, .pm-error, .pm-empty {
        text-align: center; padding: 32px 16px;
        color: var(--text-muted, #94a3b8); font-size: 12px;
    }
    .pm-empty p { margin: 8px 0 4px; font-weight: 600; color: var(--text-main, #e2e8f0); }
    .pm-empty span { font-size: 11px; opacity: .8; }
    .pm-error { color: #f87171; }

    .pm-add {
        align-self: flex-start;
        background: rgba(167,139,250,0.10);
        border: 1px solid rgba(167,139,250,0.30);
        color: #c4b5fd;
        font-size: 12px; font-weight: 600; font-family: inherit;
        padding: 6px 12px; border-radius: 7px;
        display: inline-flex; align-items: center; gap: 6px;
        cursor: pointer; transition: background 160ms;
    }
    .pm-add:hover { background: rgba(167,139,250,0.18); }

    .pm-form {
        background: rgba(0,0,0,0.30);
        border: 1px solid var(--border-color, #334155);
        border-radius: 8px; padding: 14px;
        display: flex; flex-direction: column; gap: 10px;
    }
    .pm-row { display: flex; gap: 10px; }
    .pm-field { display: flex; flex-direction: column; gap: 5px; flex: 1; }
    .pm-lbl {
        font-size: 10px; font-weight: 700;
        color: var(--text-muted, #94a3b8);
        text-transform: uppercase; letter-spacing: 0.4px;
    }
    .pm-field input, .pm-field textarea {
        background: rgba(0,0,0,0.40);
        border: 1px solid var(--border-color, #334155);
        color: var(--text-main, #e2e8f0);
        font-family: inherit; font-size: 12.5px;
        padding: 7px 10px; border-radius: 6px;
        outline: none; resize: vertical;
    }
    .pm-field input:focus, .pm-field textarea:focus { border-color: #a78bfa; }
    .pm-form-foot { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
    .pm-btn {
        padding: 7px 14px; border-radius: 7px;
        font-size: 12px; font-weight: 600; font-family: inherit;
        cursor: pointer;
        display: inline-flex; align-items: center; gap: 5px;
    }
    .pm-cancel {
        background: transparent;
        border: 1px solid var(--border-color, #334155);
        color: var(--text-muted, #94a3b8);
    }
    .pm-cancel:hover { color: var(--text-bright, #f1f5f9); }
    .pm-save {
        background: #a78bfa; border: 1px solid #a78bfa; color: #1a1330;
    }
    .pm-save:hover { opacity: 0.92; }

    .pm-list { display: flex; flex-direction: column; gap: 6px; }
    .pm-item {
        display: flex; gap: 12px; padding: 10px 12px;
        background: rgba(255,255,255,0.025);
        border: 1px solid rgba(255,255,255,0.06);
        border-radius: 7px;
        transition: background 160ms, border-color 160ms;
    }
    .pm-item:hover { background: rgba(255,255,255,0.04); border-color: rgba(255,255,255,0.10); }
    .pm-item.disabled { opacity: 0.55; }
    .pm-item-l { flex: 1; min-width: 0; }
    .pm-item-hdr { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; margin-bottom: 4px; }
    .pm-name { font-size: 12.5px; font-weight: 600; color: var(--text-bright, #f1f5f9); }
    .pm-tags { display: inline-flex; gap: 4px; }
    .pm-tag {
        font-size: 9px; font-weight: 700;
        padding: 1px 7px; border-radius: 8px;
        font-family: var(--font-mono, monospace);
        letter-spacing: 0.3px;
    }
    .pm-tag-scope { background: rgba(167,139,250,0.10); color: #c4b5fd; border: 1px solid rgba(167,139,250,0.25); }
    .pm-tag-pri   { background: rgba(59,158,255,0.08);  color: #7dd3fc; border: 1px solid rgba(59,158,255,0.20); }
    .pm-tag-off   { background: rgba(255,255,255,0.04); color: #64748b; }
    .pm-rule {
        margin: 0; font-size: 11.5px; line-height: 1.5;
        color: var(--text-main, #e2e8f0);
        white-space: pre-wrap; word-break: break-word;
    }
    .pm-item-r { display: flex; gap: 4px; flex-shrink: 0; }
    .pm-mini {
        background: transparent;
        border: 1px solid var(--border-color, #334155);
        color: var(--text-muted, #94a3b8);
        padding: 4px 6px; border-radius: 5px;
        cursor: pointer; transition: all 160ms;
        display: inline-flex; align-items: center;
    }
    .pm-mini:hover {
        background: rgba(255,255,255,0.06);
        color: var(--text-bright, #f1f5f9);
    }
    .pm-del:hover { color: #f87171; border-color: rgba(248,113,113,0.40); }

    @keyframes fade-in { from { opacity: 0; } to { opacity: 1; } }
</style>
