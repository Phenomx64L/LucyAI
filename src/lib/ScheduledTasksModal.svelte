<script>
    // ── ScheduledTasksModal — manage natural-language cron tasks
    //
    // Backend: src-tauri/src/commands/scheduled.rs
    // Calls: list_scheduled_tasks / save_scheduled_task / toggle / delete
    //
    // The user describes tasks in plain language ("every weekday at 9 AM,
    // generate a system health report") with a 5-field cron expression and
    // a next_run datetime. The runtime ticker (in +page.svelte) polls
    // due_scheduled_tasks every 60s and dispatches each fire-and-forget
    // through ask_lucy.

    import { createEventDispatcher, onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { focusTrap } from '$lib/actions';
    import { staggerIn } from '$lib/stagger';
    import Clock from '@tabler/icons-svelte/icons/clock';

    import Plus from '@tabler/icons-svelte/icons/plus';

    import Trash2 from '@tabler/icons-svelte/icons/trash';

    import Check from '@tabler/icons-svelte/icons/check';

    import X from '@tabler/icons-svelte/icons/x';

    import Play from '@tabler/icons-svelte/icons/player-play';

    import Pause from '@tabler/icons-svelte/icons/player-pause';

    import Calendar from '@tabler/icons-svelte/icons/calendar';
    const dispatch = createEventDispatcher();

    export let isOpen = false;
    export let isEN   = false;

    let tasks    = [];
    let loading  = false;
    let error    = '';
    let formOpen = false;
    let draft    = { name: '', prompt: '', cronExpr: '', nextRun: '' };

    const t = isEN
        ? {
            title:        'Scheduled Tasks',
            subtitle:     'Recurring or one-shot work Lucy runs unattended',
            empty:        'No scheduled tasks yet.',
            empty_hint:   'Schedule recurring jobs in plain language. Lucy will run them in the background and persist the results.',
            add:          'New scheduled task',
            cancel:       'Cancel',
            save:         'Save',
            name:         'Name',
            prompt:       'Prompt (what Lucy should do)',
            cron:         'Cron expression (5 fields, empty = one-shot)',
            cronHelp:     'e.g. "0 9 * * 1-5" → 9 AM weekdays. Leave empty for a one-shot run.',
            nextRun:      'Next run (ISO datetime)',
            disable:      'Disable',
            enable:       'Enable',
            delete:       'Delete',
            confirmDel:   'Delete this scheduled task?',
            saved:        'Task saved',
            deleted:      'Task deleted',
            never:        '—',
        }
        : {
            title:        'Tareas Programadas',
            subtitle:     'Trabajo recurrente o de una vez que Lucy ejecuta desatendido',
            empty:        'No hay tareas programadas.',
            empty_hint:   'Programa trabajos recurrentes en lenguaje natural. Lucy las ejecutará en segundo plano y persistirá los resultados.',
            add:          'Nueva tarea programada',
            cancel:       'Cancelar',
            save:         'Guardar',
            name:         'Nombre',
            prompt:       'Prompt (lo que debe hacer Lucy)',
            cron:         'Expresión cron (5 campos, vacío = una vez)',
            cronHelp:     'ej. "0 9 * * 1-5" → 9 AM lun-vie. Vacío para ejecución única.',
            nextRun:      'Próxima ejecución (ISO datetime)',
            disable:      'Desactivar',
            enable:       'Activar',
            delete:       'Eliminar',
            confirmDel:   '¿Eliminar esta tarea programada?',
            saved:        'Tarea guardada',
            deleted:      'Tarea eliminada',
            never:        '—',
        };

    function toast(msg, type = 'info') { dispatch('toast', { msg, type }); }

    async function load() {
        loading = true; error = '';
        try {
            tasks = await invoke('list_scheduled_tasks') || [];
        } catch (e) { error = String(e); }
        loading = false;
    }

    function openNew() {
        // Default next_run = 5 minutes from now, ISO local-ish.
        const future = new Date(Date.now() + 5 * 60_000);
        draft = {
            name: '',
            prompt: '',
            cronExpr: '',
            nextRun: future.toISOString().slice(0, 16),
        };
        formOpen = true;
    }
    function cancelForm() { formOpen = false; }

    async function save() {
        if (!draft.name?.trim() || !draft.prompt?.trim() || !draft.nextRun?.trim()) {
            toast(isEN ? 'Name, prompt and next run are required' : 'Nombre, prompt y próxima ejecución son obligatorios', 'warn'); return;
        }
        const epoch = Math.floor(Date.parse(draft.nextRun) / 1000);
        if (!Number.isFinite(epoch) || epoch <= 0) {
            toast(isEN ? 'Invalid next run datetime' : 'Datetime de próxima ejecución inválido', 'error'); return;
        }
        try {
            await invoke('save_scheduled_task', {
                name: draft.name.trim(),
                prompt: draft.prompt.trim(),
                cronExpr: draft.cronExpr?.trim() || null,
                nextRun: epoch,
            });
            toast(t.saved, 'ok');
            formOpen = false;
            await load();
        } catch (e) {
            toast(String(e).slice(0, 160), 'error');
        }
    }

    async function toggle(task) {
        try {
            await invoke('toggle_scheduled_task', { id: task.id, enabled: !task.enabled });
            await load();
        } catch (e) { toast(String(e).slice(0, 120), 'error'); }
    }

    async function remove(task) {
        // v1.7.17 — in-app dialog replaces native window.confirm.
        const { lucyConfirm } = await import('$lib/dialog-service');
        if (!await lucyConfirm(t.confirmDel,
            { tone: 'danger', description: task.name, confirmLabel: 'Delete' })) return;
        try {
            await invoke('delete_scheduled_task', { id: task.id });
            toast(t.deleted, 'ok');
            await load();
        } catch (e) { toast(String(e).slice(0, 120), 'error'); }
    }

    function fmt(epoch) {
        if (!epoch) return t.never;
        try { return new Date(epoch * 1000).toLocaleString(isEN ? 'en-US' : 'es-MX', { hour12: false }); }
        catch { return String(epoch); }
    }

    function close() { isOpen = false; dispatch('close'); }

    $: if (isOpen) load();
    onMount(load);
</script>

{#if isOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="st-overlay" role="presentation" on:click={close}
         on:keydown={(e) => { if (e.key === 'Escape') close(); }}>
        <div class="st-box" role="dialog" aria-modal="true" tabindex={-1}
             use:focusTrap on:click|stopPropagation>
            <div class="st-hdr">
                <div class="st-hdr-l">
                    <span class="st-hdr-ico"><Clock size={18} stroke={2} /></span>
                    <div>
                        <h3>{t.title}</h3>
                        <span class="st-sub">{t.subtitle}</span>
                    </div>
                </div>
                <button class="st-close" type="button" on:click={close} aria-label="Close">✕</button>
            </div>

            <div class="st-body">
                {#if formOpen}
                    <div class="st-form">
                        <label class="st-field">
                            <span class="st-lbl">{t.name}</span>
                            <input type="text" bind:value={draft.name} maxlength="120" />
                        </label>
                        <label class="st-field">
                            <span class="st-lbl">{t.prompt}</span>
                            <textarea rows="3" bind:value={draft.prompt} maxlength="4000"></textarea>
                        </label>
                        <div class="st-row">
                            <label class="st-field" style="flex:1.4;">
                                <span class="st-lbl">{t.cron}</span>
                                <input type="text" bind:value={draft.cronExpr} placeholder="0 9 * * 1-5" />
                                <span class="st-hint">{t.cronHelp}</span>
                            </label>
                            <label class="st-field" style="flex:1;">
                                <span class="st-lbl">{t.nextRun}</span>
                                <input type="datetime-local" bind:value={draft.nextRun} />
                            </label>
                        </div>
                        <div class="st-form-foot">
                            <button class="st-btn st-cancel" on:click={cancelForm}>{t.cancel}</button>
                            <button class="st-btn st-save" on:click={save}>
                                <Check size={13} stroke={2.5} /> {t.save}
                            </button>
                        </div>
                    </div>
                {/if}

                {#if !formOpen}
                    <button class="st-add" on:click={openNew}>
                        <Plus size={14} stroke={2.5} /> {t.add}
                    </button>
                {/if}

                {#if loading}
                    <div class="st-loading">…</div>
                {:else if error}
                    <div class="st-error">{error}</div>
                {:else if tasks.length === 0}
                    <div class="st-empty">
                        <Calendar size={32} stroke={1.4} style="opacity:.4;" />
                        <p>{t.empty}</p>
                        <span>{t.empty_hint}</span>
                    </div>
                {:else}
                    <div class="st-list">
                        {#each tasks as task, i (task.id)}
                            <div class="st-item" class:disabled={!task.enabled} in:staggerIn={{ index: i, step: 24 }}>
                                <div class="st-item-l">
                                    <div class="st-item-hdr">
                                        <span class="st-name">{task.name}</span>
                                        <span class="st-tags">
                                            {#if task.cron_expr}
                                                <span class="st-tag st-tag-cron">{task.cron_expr}</span>
                                            {:else}
                                                <span class="st-tag st-tag-once">one-shot</span>
                                            {/if}
                                            {#if task.last_status}
                                                <span class="st-tag st-tag-{task.last_status}">{task.last_status}</span>
                                            {/if}
                                            {#if !task.enabled}
                                                <span class="st-tag st-tag-off">disabled</span>
                                            {/if}
                                        </span>
                                    </div>
                                    <p class="st-prompt">{task.prompt.length > 140 ? task.prompt.slice(0, 140) + '…' : task.prompt}</p>
                                    <div class="st-meta">
                                        <span>{isEN ? 'Next' : 'Próx.'}: <code>{fmt(task.next_run)}</code></span>
                                        <span>·</span>
                                        <span>{isEN ? 'Last' : 'Última'}: <code>{fmt(task.last_run)}</code></span>
                                    </div>
                                </div>
                                <div class="st-item-r">
                                    <button class="st-mini" title={task.enabled ? t.disable : t.enable} on:click={() => toggle(task)}>
                                        {#if task.enabled}<Pause size={13} stroke={2} />{:else}<Play size={13} stroke={2} />{/if}
                                    </button>
                                    <button class="st-mini st-del" title={t.delete} on:click={() => remove(task)}>
                                        <Trash2 size={13} stroke={2} />
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
    .st-overlay {
        position: fixed; inset: 0; z-index: 8500;
        background: rgba(2, 6, 12, 0.74);
        backdrop-filter: blur(4px);
        display: flex; align-items: center; justify-content: center;
        animation: fade-in 200ms ease-out;
    }
    .st-box {
        /* Depth: faint amber top glow (scheduler identity) over the base. */
        background:
            radial-gradient(120% 55% at 50% -6%, rgba(245,158,11,.08) 0%, transparent 56%),
            var(--bg-card, #161b22);
        border: 1px solid var(--border-light, #334155);
        border-top: 3px solid #fbbf24;
        border-radius: 12px;
        width: 720px; max-width: 92vw; max-height: 86vh;
        display: flex; flex-direction: column;
        box-shadow: 0 24px 64px rgba(0,0,0,0.6),
                    0 -1px 28px -10px rgba(245,158,11,.45);
        outline: none;
        animation: st-pop 220ms cubic-bezier(0.34, 1.56, 0.64, 1);
    }
    @keyframes st-pop {
        from { opacity: 0; transform: scale(0.96) translateY(8px); }
        to   { opacity: 1; transform: scale(1) translateY(0); }
    }
    .st-hdr {
        display: flex; align-items: center; justify-content: space-between;
        padding: 14px 18px;
        border-bottom: 1px solid var(--border-color, #1e293b);
    }
    .st-hdr-l { display: flex; align-items: center; gap: 10px; color: #fbbf24; }
    .st-hdr-ico {
        display: inline-flex; align-items: center; justify-content: center;
        width: 36px; height: 36px; border-radius: 11px; flex-shrink: 0;
        background: rgba(245,158,11,.12);
        border: 1px solid rgba(245,158,11,.28);
        box-shadow: 0 0 16px -4px rgba(245,158,11,.5);
    }
    .st-hdr h3 { margin: 0; font-size: 13.5px; font-weight: 600; color: var(--text-bright, #f1f5f9); }
    .st-sub { font-size: 10.5px; color: var(--text-muted, #94a3b8); }
    .st-close {
        background: transparent; border: none; color: var(--text-muted, #64748b);
        font-size: 18px; cursor: pointer; padding: 0 4px; line-height: 1;
    }
    .st-close:hover { color: var(--text-bright, #f1f5f9); }

    .st-body {
        padding: 16px 18px; overflow-y: auto; flex: 1;
        display: flex; flex-direction: column; gap: 12px;
    }
    .st-loading, .st-error, .st-empty {
        text-align: center; padding: 32px 16px;
        color: var(--text-muted, #94a3b8); font-size: 12px;
    }
    .st-empty p { margin: 8px 0 4px; font-weight: 600; color: var(--text-main, #e2e8f0); }
    .st-empty span { font-size: 11px; opacity: .8; }
    .st-error { color: #f87171; }

    .st-add {
        align-self: flex-start;
        background: rgba(245,158,11,0.10);
        border: 1px solid rgba(245,158,11,0.30);
        color: #fbbf24;
        font-size: 12px; font-weight: 600; font-family: inherit;
        padding: 6px 12px; border-radius: 7px;
        display: inline-flex; align-items: center; gap: 6px;
        cursor: pointer; transition: background 160ms;
    }
    .st-add:hover { background: rgba(245,158,11,0.18); }

    .st-form {
        background: rgba(0,0,0,0.30);
        border: 1px solid var(--border-color, #334155);
        border-radius: 8px; padding: 14px;
        display: flex; flex-direction: column; gap: 10px;
    }
    .st-row { display: flex; gap: 10px; flex-wrap: wrap; }
    .st-field { display: flex; flex-direction: column; gap: 5px; flex: 1; min-width: 200px; }
    .st-lbl {
        font-size: 10px; font-weight: 700;
        color: var(--text-muted, #94a3b8);
        text-transform: uppercase; letter-spacing: 0.4px;
    }
    .st-field input, .st-field textarea {
        background: rgba(0,0,0,0.40);
        border: 1px solid var(--border-color, #334155);
        color: var(--text-main, #e2e8f0);
        font-family: inherit; font-size: 12.5px;
        padding: 7px 10px; border-radius: 6px;
        outline: none; resize: vertical;
    }
    .st-field input:focus, .st-field textarea:focus { border-color: #fbbf24; }
    .st-hint { font-size: 10px; color: var(--text-muted, #64748b); }
    .st-form-foot { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
    .st-btn {
        padding: 7px 14px; border-radius: 7px;
        font-size: 12px; font-weight: 600; font-family: inherit;
        cursor: pointer;
        display: inline-flex; align-items: center; gap: 5px;
    }
    .st-cancel {
        background: transparent;
        border: 1px solid var(--border-color, #334155);
        color: var(--text-muted, #94a3b8);
    }
    .st-cancel:hover { color: var(--text-bright, #f1f5f9); }
    .st-save {
        background: #fbbf24; border: 1px solid #fbbf24; color: #1a1330;
    }
    .st-save:hover { opacity: 0.92; }

    .st-list { display: flex; flex-direction: column; gap: 6px; }
    .st-item {
        display: flex; gap: 12px; padding: 10px 12px;
        background: rgba(255,255,255,0.025);
        border: 1px solid rgba(255,255,255,0.06);
        border-radius: 7px;
        transition: background 160ms, border-color 160ms;
    }
    .st-item:hover { background: rgba(255,255,255,0.04); border-color: rgba(255,255,255,0.10); }
    .st-item.disabled { opacity: 0.55; }
    .st-item-l { flex: 1; min-width: 0; }
    .st-item-hdr { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; margin-bottom: 4px; }
    .st-name { font-size: 12.5px; font-weight: 600; color: var(--text-bright, #f1f5f9); }
    .st-tags { display: inline-flex; gap: 4px; flex-wrap: wrap; }
    .st-tag {
        font-size: 9px; font-weight: 700;
        padding: 1px 7px; border-radius: 8px;
        font-family: var(--font-mono, monospace);
        letter-spacing: 0.3px;
    }
    .st-tag-cron  { background: rgba(245,158,11,0.10); color: #fbbf24; border: 1px solid rgba(245,158,11,0.25); }
    .st-tag-once  { background: rgba(167,139,250,0.10); color: #c4b5fd; border: 1px solid rgba(167,139,250,0.25); }
    .st-tag-ok    { background: rgba(16,185,129,0.10); color: #10b981; }
    .st-tag-error { background: rgba(239,68,68,0.10);  color: #f87171; }
    .st-tag-running, .st-tag-skipped { background: rgba(255,255,255,0.04); color: #94a3b8; }
    .st-tag-off   { background: rgba(255,255,255,0.04); color: #64748b; }
    .st-prompt {
        margin: 0; font-size: 11.5px; line-height: 1.5;
        color: var(--text-main, #e2e8f0);
        white-space: pre-wrap; word-break: break-word;
    }
    .st-meta {
        margin-top: 4px;
        display: flex; gap: 6px; flex-wrap: wrap;
        font-size: 10.5px; color: var(--text-muted, #94a3b8);
    }
    .st-meta code {
        font-family: var(--font-mono, monospace);
        color: var(--text-main, #e2e8f0);
        background: rgba(0,0,0,0.20);
        padding: 1px 5px; border-radius: 3px;
    }
    .st-item-r { display: flex; gap: 4px; flex-shrink: 0; }
    .st-mini {
        background: transparent;
        border: 1px solid var(--border-color, #334155);
        color: var(--text-muted, #94a3b8);
        padding: 4px 6px; border-radius: 5px;
        cursor: pointer; transition: all 160ms;
        display: inline-flex; align-items: center;
    }
    .st-mini:hover {
        background: rgba(255,255,255,0.06);
        color: var(--text-bright, #f1f5f9);
    }
    .st-del:hover { color: #f87171; border-color: rgba(248,113,113,0.40); }

    @keyframes fade-in { from { opacity: 0; } to { opacity: 1; } }
</style>
