<script>
    // ── RemoteFileDiffModal ───────────────────────────────────────────────────
    // Reads a file from a remote host, lets the user edit/preview a unified
    // diff, then writes it back via write_remote_file.
    //
    // Workflow:
    //   1. Open with `host` + `path` props
    //   2. Component fetches the file (read_remote_file)
    //   3. User edits a textarea on the right side
    //   4. Live diff (line-based, simple) renders on the left
    //   5. Apply/Cancel — Apply triggers write_remote_file with backup
    //
    // SECURITY: Permission rules apply to the underlying execute_shell_cmd
    // calls used by read_remote_file/write_remote_file.

    import { createEventDispatcher, onMount, tick } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import FileText from '@tabler/icons-svelte/icons/file-text';

    import Refresh from '@tabler/icons-svelte/icons/refresh';

    import Check from '@tabler/icons-svelte/icons/check';

    import XIcon from '@tabler/icons-svelte/icons/x';

    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
    const dispatch = createEventDispatcher();

    // Props
    export let open       = false;
    export let host       = null;     // { id, name, host, username, type, port, sshKeyPath }
    export let initialPath = '';
    export let isEN       = false;

    // State
    let path           = '';
    let originalContent = '';
    let editedContent   = '';
    let loading        = false;
    let saving         = false;
    let error          = '';
    let lastResult     = '';
    let createBackup   = true;

    $: if (open && initialPath && initialPath !== path) {
        path = initialPath;
        loadFile();
    }

    function toast(msg, type = 'info') { dispatch('toast', { msg, type }); }

    async function loadFile() {
        if (!host) { error = 'No host selected'; return; }
        if (!path?.trim()) { error = 'Path is empty'; return; }
        loading = true;
        error = '';
        lastResult = '';
        try {
            // Pull credentials from keyring (matches NexShell pattern)
            let password = null;
            try { password = await invoke('get_host_credential', { hostId: host.id }); } catch { /* key auth or missing */ }
            const content = await invoke('read_remote_file', {
                host: host.host,
                username: host.username,
                path: path.trim(),
                hostType: host.type || 'linux',
                port: host.port || null,
                password,
                keyPath: host.sshKeyPath || null,
            });
            originalContent = String(content);
            editedContent = String(content);
        } catch (e) {
            error = String(e?.message || e);
        }
        loading = false;
    }

    async function applyChanges() {
        if (originalContent === editedContent) {
            toast(isEN ? 'No changes to apply' : 'Sin cambios para aplicar', 'info');
            return;
        }
        saving = true;
        error = '';
        try {
            let password = null;
            try { password = await invoke('get_host_credential', { hostId: host.id }); } catch {}
            const result = await invoke('write_remote_file', {
                host: host.host,
                username: host.username,
                path: path.trim(),
                content: editedContent,
                hostType: host.type || 'linux',
                port: host.port || null,
                password,
                keyPath: host.sshKeyPath || null,
                createBackup,
            });
            lastResult = String(result);
            originalContent = editedContent; // diff cleared after successful apply
            toast(isEN ? 'File updated' : 'Archivo actualizado', 'success');
        } catch (e) {
            error = String(e?.message || e);
            toast((isEN ? 'Apply failed: ' : 'Falló: ') + error.slice(0, 80), 'error');
        }
        saving = false;
    }

    function close() {
        if (saving) return;
        if (originalContent !== editedContent) {
            const ok = confirm(isEN
                ? 'You have unsaved changes. Discard them?'
                : 'Tienes cambios sin guardar. ¿Descartar?');
            if (!ok) return;
        }
        open = false;
        path = '';
        originalContent = '';
        editedContent = '';
        error = '';
        lastResult = '';
        dispatch('close');
    }

    function onKey(e) {
        if (!open) return;
        if (e.key === 'Escape') { e.preventDefault(); close(); }
        // Ctrl+S = apply
        if ((e.ctrlKey || e.metaKey) && (e.key === 's' || e.key === 'S')) {
            e.preventDefault();
            if (!saving && originalContent !== editedContent) applyChanges();
        }
    }

    // ── Simple line-based diff (no external lib needed) ────────────────────
    // Returns an array of { type: 'eq' | 'add' | 'rem', text } based on a
    // longest-common-subsequence-light algorithm. For files >2k lines this
    // would be slow, but our 1MB limit + typical config files keep it fast.
    function diffLines(a, b) {
        if (a === b) return [];
        const aL = a.split('\n');
        const bL = b.split('\n');
        // Trim common prefix and suffix to keep the diff tight
        let pre = 0;
        while (pre < aL.length && pre < bL.length && aL[pre] === bL[pre]) pre++;
        let suf = 0;
        while (suf < aL.length - pre && suf < bL.length - pre
            && aL[aL.length - 1 - suf] === bL[bL.length - 1 - suf]) suf++;
        const aMid = aL.slice(pre, aL.length - suf);
        const bMid = bL.slice(pre, bL.length - suf);
        const out = [];
        // Show 1 line of context above and below changes
        if (pre > 0) {
            const ctx = Math.min(pre, 1);
            for (let i = pre - ctx; i < pre; i++) out.push({ type: 'eq', text: aL[i], n: i + 1 });
        }
        for (const line of aMid) out.push({ type: 'rem', text: line });
        for (const line of bMid) out.push({ type: 'add', text: line });
        if (suf > 0) {
            const start = aL.length - suf;
            const ctx = Math.min(suf, 1);
            for (let i = start; i < start + ctx; i++) out.push({ type: 'eq', text: aL[i], n: i + 1 });
        }
        return out;
    }

    $: diff = diffLines(originalContent, editedContent);
    $: hasChanges = originalContent !== editedContent;
    $: addCount = diff.filter(d => d.type === 'add').length;
    $: remCount = diff.filter(d => d.type === 'rem').length;

    onMount(() => { /* keydown registered via svelte:window below */ });
</script>

<svelte:window on:keydown={onKey} />

{#if open}
<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="rfd-overlay" on:click|self={close}>
    <div class="rfd-modal" role="dialog" aria-modal="true" aria-label="Remote File Diff">
        <div class="rfd-hdr">
            <div class="rfd-hdr-l">
                <span class="rfd-hdr-icon"><FileText size={16}/></span>
                <span class="rfd-host">{host?.name || '?'}</span>
                <span class="rfd-sep">›</span>
                <input class="rfd-path" type="text" bind:value={path}
                    placeholder="/etc/nginx/nginx.conf"
                    disabled={loading || saving}
                    on:keydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); loadFile(); } }} />
                <button class="rfd-btn-mini" on:click={loadFile} disabled={loading || saving} title={isEN ? 'Load (Enter)' : 'Cargar (Enter)'}>
                    <Refresh size={13} strokeWidth={2}/>
                </button>
            </div>
            <button class="rfd-close" on:click={close} title="Esc">✕</button>
        </div>

        {#if loading}
            <div class="rfd-status rfd-loading">↻ {isEN ? 'Reading remote file…' : 'Leyendo archivo remoto…'}</div>
        {:else if error}
            <div class="rfd-status rfd-error">
                <AlertTriangle size={13}/> {error}
            </div>
        {:else if originalContent || editedContent}
            <div class="rfd-statbar">
                <span class="rfd-stat">{originalContent.split('\n').length} {isEN ? 'lines' : 'líneas'}</span>
                <span class="rfd-stat">{originalContent.length} bytes</span>
                {#if hasChanges}
                    <span class="rfd-stat rfd-stat-add">+{addCount}</span>
                    <span class="rfd-stat rfd-stat-rem">−{remCount}</span>
                {:else}
                    <span class="rfd-stat rfd-stat-clean">✓ {isEN ? 'no changes' : 'sin cambios'}</span>
                {/if}
                {#if lastResult}<span class="rfd-stat rfd-stat-saved">✓ {lastResult}</span>{/if}
            </div>
            <div class="rfd-body">
                <div class="rfd-pane">
                    <div class="rfd-pane-hdr">{isEN ? 'Diff Preview' : 'Vista previa diff'}</div>
                    <div class="rfd-diff">
                        {#if !hasChanges}
                            <div class="rfd-empty">{isEN ? 'Edit the file on the right to see the diff here' : 'Edita el archivo a la derecha para ver el diff aquí'}</div>
                        {:else}
                            {#each diff as d}
                                {#if d.type === 'eq'}
                                    <div class="rfd-d-eq"><span class="rfd-d-mark"> </span>{d.text}</div>
                                {:else if d.type === 'rem'}
                                    <div class="rfd-d-rem"><span class="rfd-d-mark">−</span>{d.text}</div>
                                {:else}
                                    <div class="rfd-d-add"><span class="rfd-d-mark">+</span>{d.text}</div>
                                {/if}
                            {/each}
                        {/if}
                    </div>
                </div>
                <div class="rfd-pane">
                    <div class="rfd-pane-hdr">{isEN ? 'Edit (Ctrl+S = Apply)' : 'Editar (Ctrl+S = Aplicar)'}</div>
                    <textarea class="rfd-edit" spellcheck="false"
                        bind:value={editedContent}
                        disabled={saving}
                        placeholder={isEN ? 'File content…' : 'Contenido del archivo…'}></textarea>
                </div>
            </div>
        {:else}
            <div class="rfd-status">{isEN ? 'Enter a path above and press Enter to load.' : 'Ingresa una ruta arriba y presiona Enter.'}</div>
        {/if}

        <div class="rfd-foot">
            <label class="rfd-bk">
                <input type="checkbox" bind:checked={createBackup} disabled={saving}/>
                <span>{isEN ? 'Create .lucy.bak backup before applying' : 'Crear backup .lucy.bak antes de aplicar'}</span>
            </label>
            <div class="rfd-btns">
                <button class="rfd-btn rfd-btn-ghost" on:click={close} disabled={saving}>
                    <XIcon size={13}/> {isEN ? 'Cancel' : 'Cancelar'}
                </button>
                <button class="rfd-btn rfd-btn-pri" on:click={applyChanges}
                    disabled={!hasChanges || saving || loading}
                    title={hasChanges ? (isEN ? 'Write back to remote (Ctrl+S)' : 'Escribir en remoto (Ctrl+S)') : ''}>
                    {#if saving}↻ {isEN ? 'Applying…' : 'Aplicando…'}{:else}<Check size={13}/> {isEN ? 'Apply Changes' : 'Aplicar Cambios'}{/if}
                </button>
            </div>
        </div>
    </div>
</div>
{/if}

<style>
    .rfd-overlay{position:fixed;inset:0;z-index:8800;background:rgba(2,4,8,.78);backdrop-filter:blur(8px);display:flex;align-items:center;justify-content:center;padding:24px;animation:rfd-fade .15s ease;}
    @keyframes rfd-fade{from{opacity:0;}to{opacity:1;}}
    .rfd-modal{
      width:min(1100px, 98vw);height:min(84vh, 720px);
      background:linear-gradient(180deg, rgba(15,23,42,.96), rgba(8,12,22,.98));
      border:1px solid rgba(99,102,241,.32);
      border-radius:12px;
      box-shadow:0 24px 64px rgba(0,0,0,.55);
      display:flex;flex-direction:column;overflow:hidden;
      animation:rfd-slide .18s cubic-bezier(0.16,1,0.3,1);
    }
    @keyframes rfd-slide{from{transform:translateY(8px);opacity:0}to{transform:none;opacity:1}}

    .rfd-hdr{display:flex;align-items:center;justify-content:space-between;padding:12px 16px;background:rgba(99,102,241,.06);border-bottom:1px solid rgba(99,102,241,.18);}
    .rfd-hdr-l{display:flex;align-items:center;gap:8px;flex:1;min-width:0;}
    .rfd-hdr-icon{color:#a5b4fc;display:inline-flex;}
    .rfd-host{font-family:var(--mono);font-size:12px;font-weight:700;color:#7dd3fc;background:rgba(125,211,252,.10);padding:2px 8px;border-radius:6px;flex-shrink:0;}
    .rfd-sep{color:var(--txt3);}
    .rfd-path{
      flex:1;min-width:120px;
      background:rgba(0,0,0,.30);border:1px solid rgba(255,255,255,.08);
      color:var(--txt);font-family:var(--mono);font-size:12px;
      padding:5px 10px;border-radius:5px;outline:none;
    }
    .rfd-path:focus{border-color:rgba(99,102,241,.50);}
    .rfd-btn-mini{
      background:rgba(99,102,241,.10);border:1px solid rgba(99,102,241,.28);color:#a5b4fc;
      padding:4px 8px;border-radius:5px;cursor:pointer;display:inline-flex;align-items:center;
    }
    .rfd-btn-mini:hover:not(:disabled){background:rgba(99,102,241,.20);}
    .rfd-btn-mini:disabled{opacity:.4;cursor:not-allowed;}
    .rfd-close{background:transparent;border:1px solid rgba(255,255,255,.08);color:var(--txt2);width:28px;height:28px;border-radius:6px;cursor:pointer;font-size:13px;flex-shrink:0;}
    .rfd-close:hover{background:rgba(239,68,68,.10);color:var(--red);}

    .rfd-status{padding:12px 16px;color:var(--txt2);font-size:12px;}
    .rfd-loading{color:#a78bfa;animation:rfd-pulse 1.4s ease-in-out infinite;}
    @keyframes rfd-pulse{0%,100%{opacity:1;}50%{opacity:.55;}}
    .rfd-error{color:var(--red);background:rgba(239,68,68,.06);border-left:3px solid var(--red);display:flex;align-items:center;gap:8px;font-family:var(--mono);font-size:11px;}

    .rfd-statbar{display:flex;gap:10px;padding:6px 16px;background:rgba(0,0,0,.18);border-bottom:1px solid rgba(255,255,255,.04);font-size:10.5px;font-family:var(--mono);color:var(--txt3);align-items:center;flex-shrink:0;}
    .rfd-stat-add{color:var(--acc);font-weight:700;}
    .rfd-stat-rem{color:var(--red);font-weight:700;}
    .rfd-stat-clean{color:var(--acc);}
    .rfd-stat-saved{color:var(--acc);margin-left:auto;}

    .rfd-body{flex:1;display:grid;grid-template-columns:1fr 1fr;gap:1px;background:rgba(255,255,255,.04);overflow:hidden;min-height:0;}
    .rfd-pane{display:flex;flex-direction:column;background:rgba(8,12,22,.40);min-height:0;overflow:hidden;}
    .rfd-pane-hdr{font-size:10px;text-transform:uppercase;letter-spacing:1px;font-weight:700;color:var(--txt3);padding:6px 12px;background:rgba(0,0,0,.20);border-bottom:1px solid rgba(255,255,255,.04);flex-shrink:0;}
    .rfd-diff{flex:1;overflow:auto;padding:8px 0;font-family:var(--mono);font-size:11px;line-height:1.55;}
    .rfd-empty{color:var(--txt3);font-style:italic;padding:20px;text-align:center;font-family:var(--font-ui);font-size:11.5px;}
    .rfd-d-eq, .rfd-d-rem, .rfd-d-add{padding:0 12px;white-space:pre-wrap;word-break:break-word;}
    .rfd-d-eq{color:var(--txt3);}
    .rfd-d-rem{color:#fca5a5;background:rgba(239,68,68,.10);}
    .rfd-d-add{color:#86efac;background:rgba(16,185,129,.10);}
    .rfd-d-mark{display:inline-block;width:14px;color:var(--txt3);user-select:none;}
    .rfd-d-rem .rfd-d-mark{color:#ef4444;}
    .rfd-d-add .rfd-d-mark{color:#10b981;}

    .rfd-edit{
      flex:1;width:100%;
      background:rgba(0,0,0,.30);border:none;color:var(--txt);
      font-family:var(--mono);font-size:11px;line-height:1.55;
      padding:8px 12px;outline:none;resize:none;
    }
    .rfd-edit:disabled{opacity:.6;}

    .rfd-foot{display:flex;justify-content:space-between;align-items:center;gap:10px;padding:10px 16px;border-top:1px solid rgba(255,255,255,.05);background:rgba(0,0,0,.18);flex-shrink:0;}
    .rfd-bk{display:flex;align-items:center;gap:6px;font-size:11px;color:var(--txt2);cursor:pointer;}
    .rfd-bk input{cursor:pointer;}
    .rfd-btns{display:flex;gap:8px;}
    .rfd-btn{display:inline-flex;align-items:center;gap:6px;padding:6px 14px;border-radius:6px;font-size:12px;font-weight:600;cursor:pointer;transition:.15s;border:1px solid;}
    .rfd-btn-ghost{background:transparent;border-color:rgba(255,255,255,.10);color:var(--txt2);}
    .rfd-btn-ghost:hover:not(:disabled){background:rgba(255,255,255,.05);color:var(--txt);}
    .rfd-btn-pri{background:rgba(16,185,129,.12);border-color:rgba(16,185,129,.40);color:var(--acc);font-weight:700;}
    .rfd-btn-pri:hover:not(:disabled){background:rgba(16,185,129,.22);}
    .rfd-btn:disabled{opacity:.4;cursor:not-allowed;}

    /* Light theme */
    :global(:root.light) .rfd-modal{background:linear-gradient(180deg, #ffffff, #f8fafc);border-color:rgba(99,102,241,.35);}
    :global(:root.light) .rfd-hdr{background:rgba(99,102,241,.08);border-bottom-color:rgba(99,102,241,.20);}
    :global(:root.light) .rfd-host{color:#0369a1;background:rgba(125,211,252,.18);}
    :global(:root.light) .rfd-path{background:#fff;border-color:#cbd5e1;color:#1e293b;}
    :global(:root.light) .rfd-pane{background:#f8fafc;}
    :global(:root.light) .rfd-pane-hdr{background:#e2e8f0;color:#475569;}
    :global(:root.light) .rfd-edit{background:#ffffff;color:#1e293b;}
    :global(:root.light) .rfd-d-eq{color:#64748b;}
    :global(:root.light) .rfd-d-rem{color:#991b1b;background:rgba(239,68,68,.10);}
    :global(:root.light) .rfd-d-add{color:#065f46;background:rgba(16,185,129,.12);}
    :global(:root.light) .rfd-foot{background:#f1f5f9;border-top-color:#e2e8f0;}
</style>
