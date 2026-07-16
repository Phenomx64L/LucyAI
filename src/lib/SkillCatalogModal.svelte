<!-- ── SkillCatalogModal.svelte (v1.7.168) ──────────────────────────────────
     Manage Lucy's loaded security/forensic skill catalogue (the /sec-skill
     library): list every loaded skill, search, view its body, activate it
     for the next turn, and DELETE user skills. Bundled skills (shipped with
     Lucy) are read-only — the delete action is hidden for them.

     Backend: security_skills_list / _get / _delete / _reload / _user_dir.
     Adding is folder-based (drag a SKILL.md into the user dir or chat, or
     `/sec-skill new <id>`), so the header exposes "Open folder" + "Reload".
─────────────────────────────────────────────────────────────────────────── -->
<script>
    import { createEventDispatcher, onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { toast } from 'svelte-sonner';
    import { setSecuritySkillAsPreset, peekActiveSecuritySkill, clearActiveSecuritySkill } from '$lib/security-skill-bridge';
    import { lucyConfirm } from '$lib/dialog-service';
    import { renderMd } from '$lib/md-render';
    import ShieldCheck from '@tabler/icons-svelte/icons/shield-check';
    import Search from '@tabler/icons-svelte/icons/search';
    import Trash2 from '@tabler/icons-svelte/icons/trash';
    import FolderOpen from '@tabler/icons-svelte/icons/folder-open';
    import RefreshCw from '@tabler/icons-svelte/icons/refresh';
    import Eye from '@tabler/icons-svelte/icons/eye';
    import Bolt from '@tabler/icons-svelte/icons/bolt';

    export let isOpen = false;
    export let isEN = false;
    const dispatch = createEventDispatcher();

    let skills = [];          // SkillMeta[]
    let loading = false;
    let error = '';
    let query = '';
    let domainFilter = 'all';
    let activeId = null;
    let viewing = null;       // SkillFull { meta, body } when previewing
    let busy = '';            // id currently activating/deleting/viewing

    $: domains = ['all', ...Array.from(new Set(skills.map(s => s.domain).filter(Boolean))).sort()];

    $: filtered = (() => {
        const q = query.trim().toLowerCase();
        const list = skills.filter(s => {
            if (domainFilter !== 'all' && s.domain !== domainFilter) return false;
            if (!q) return true;
            const hay = `${s.name} ${s.description} ${(s.tags || []).join(' ')} ${s.domain} ${s.subdomain} ${(s.mitre_attck || []).join(' ')}`.toLowerCase();
            return hay.includes(q);
        });
        // active first, then user skills, then alpha
        return list.sort((a, b) => {
            const aa = a.id === activeId ? 0 : 1, ba = b.id === activeId ? 0 : 1;
            if (aa !== ba) return aa - ba;
            const au = a.source === 'user' ? 0 : 1, bu = b.source === 'user' ? 0 : 1;
            if (au !== bu) return au - bu;
            return a.name.localeCompare(b.name);
        });
    })();

    $: userCount = skills.filter(s => s.source === 'user').length;

    async function load() {
        loading = true; error = '';
        try {
            skills = await invoke('security_skills_list');
            const active = peekActiveSecuritySkill();
            activeId = active?.meta?.id || null;
        } catch (e) { error = String(e); }
        loading = false;
    }
    onMount(load);
    $: if (isOpen) load();

    async function activate(s) {
        busy = s.id;
        try {
            const full = await invoke('security_skills_get', { id: s.id });
            setSecuritySkillAsPreset(full, true);
            activeId = s.id;
            toast.success((isEN ? 'Activated: ' : 'Activada: ') + s.name);
        } catch (e) { toast.error(String(e)); }
        busy = '';
    }
    function deactivate() {
        clearActiveSecuritySkill();
        activeId = null;
        toast(isEN ? 'No active skill' : 'Sin skill activa');
    }
    async function view(s) {
        busy = s.id;
        try { viewing = await invoke('security_skills_get', { id: s.id }); }
        catch (e) { toast.error(String(e)); }
        busy = '';
    }
    async function del(s) {
        if (s.source !== 'user') return;
        const ok = await lucyConfirm(
            isEN ? `Delete skill "${s.name}"?` : `¿Eliminar la skill "${s.name}"?`,
            { tone: 'danger', description: s.id, confirmLabel: isEN ? 'Delete' : 'Eliminar' });
        if (!ok) return;
        busy = s.id;
        try {
            await invoke('security_skills_delete', { id: s.id });
            if (activeId === s.id) { clearActiveSecuritySkill(); activeId = null; }
            await load();
            toast.success((isEN ? 'Deleted: ' : 'Eliminada: ') + s.name);
        } catch (e) { toast.error(String(e)); }
        busy = '';
    }
    async function openFolder() {
        try {
            const info = await invoke('security_skills_user_dir');
            await invoke('execute_powershell', { script: `Start-Process "${String(info.path).replace(/"/g, '`"')}"`, bypassToken: null }).catch(() => {});
            toast(isEN ? `Opened: ${info.path}` : `Carpeta abierta: ${info.path}`);
        } catch (e) { toast.error(String(e)); }
    }
    async function reload() {
        try {
            const n = await invoke('security_skills_reload');
            await load();
            toast.success((isEN ? 'Reloaded — ' : 'Recargado — ') + n + ' skills');
        } catch (e) { toast.error(String(e)); }
    }
    function close() { dispatch('close'); }
</script>

{#if isOpen}
<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="scm-bg" role="presentation" on:click={close} on:keydown={(e) => { if (e.key === 'Escape') close(); }}>
  <div class="scm-box" role="dialog" aria-modal="true" tabindex="-1" on:click|stopPropagation>
    <div class="scm-hdr">
      <span class="scm-hdr-ico"><ShieldCheck size={20} strokeWidth={1.9} color="var(--acc, #10b981)"/></span>
      <div class="scm-hdr-text">
        <h2>{isEN ? 'Skills Manager' : 'Gestor de Skills'}</h2>
        <p class="scm-sub">{isEN
          ? `${skills.length} loaded · ${userCount} user · the rest are bundled (read-only)`
          : `${skills.length} cargadas · ${userCount} de usuario · el resto vienen con Lucy (solo lectura)`}</p>
      </div>
      <div class="scm-hdr-acts">
        <button class="scm-tbtn" on:click={openFolder} title={isEN ? 'Open user skills folder' : 'Abrir carpeta de skills de usuario'}><FolderOpen size={14}/></button>
        <button class="scm-tbtn" on:click={reload} title={isEN ? 'Reload index' : 'Recargar índice'}><RefreshCw size={14}/></button>
        <button class="scm-close" on:click={close} aria-label="Close">✕</button>
      </div>
    </div>

    {#if viewing}
      <!-- ── Detail / preview ── -->
      <div class="scm-detail">
        <button class="scm-back" on:click={() => viewing = null}>← {isEN ? 'Back to list' : 'Volver a la lista'}</button>
        <div class="scm-detail-hd">
          <h3>{viewing.meta.name}</h3>
          <span class="scm-src scm-src-{viewing.meta.source}">{viewing.meta.source}</span>
        </div>
        <div class="scm-detail-meta">
          {#if viewing.meta.domain}<span class="scm-chip">{viewing.meta.domain}{viewing.meta.subdomain ? ` / ${viewing.meta.subdomain}` : ''}</span>{/if}
          {#each (viewing.meta.mitre_attck || []).slice(0, 6) as code}<span class="scm-chip scm-chip-mitre">{code}</span>{/each}
          {#each (viewing.meta.tags || []).slice(0, 6) as t}<span class="scm-chip scm-chip-tag">{t}</span>{/each}
        </div>
        <div class="scm-detail-body">{@html renderMd(viewing.body, { chips: false })}</div>
        <div class="scm-detail-acts">
          {#if activeId === viewing.meta.id}
            <button class="scm-btn scm-btn-ghost" on:click={deactivate}>{isEN ? 'Deactivate' : 'Desactivar'}</button>
          {:else}
            <button class="scm-btn scm-btn-pri" on:click={() => activate(viewing.meta)}><Bolt size={13}/> {isEN ? 'Activate' : 'Activar'}</button>
          {/if}
          {#if viewing.meta.source === 'user'}
            <button class="scm-btn scm-btn-danger" on:click={() => del(viewing.meta)}><Trash2 size={13}/> {isEN ? 'Delete' : 'Eliminar'}</button>
          {/if}
        </div>
      </div>
    {:else}
      <!-- ── List ── -->
      <div class="scm-tools">
        <div class="scm-search">
          <Search size={14} color="var(--txt3,#64748b)"/>
          <input type="text" bind:value={query}
            placeholder={isEN ? 'Search by name, tag, domain, MITRE code…' : 'Buscar por nombre, tag, dominio, código MITRE…'} />
        </div>
        <select class="scm-domain" bind:value={domainFilter} aria-label={isEN ? 'Domain' : 'Dominio'}>
          {#each domains as d}<option value={d}>{d === 'all' ? (isEN ? 'All domains' : 'Todos los dominios') : d}</option>{/each}
        </select>
      </div>

      <div class="scm-list">
        {#if loading && skills.length === 0}
          <div class="scm-empty">{isEN ? 'Loading…' : 'Cargando…'}</div>
        {:else if error}
          <div class="scm-empty scm-err">{error}</div>
        {:else if filtered.length === 0}
          <div class="scm-empty">{isEN ? 'No skills match.' : 'Ninguna skill coincide.'}</div>
        {:else}
          {#each filtered as s (s.id)}
            <div class="scm-row" class:scm-row-active={s.id === activeId}>
              <div class="scm-row-main">
                <div class="scm-row-top">
                  <span class="scm-name">{s.name}</span>
                  {#if s.id === activeId}<span class="scm-badge scm-badge-active">{isEN ? 'ACTIVE' : 'ACTIVA'}</span>{/if}
                  <span class="scm-src scm-src-{s.source}">{s.source}</span>
                </div>
                <div class="scm-desc">{s.description}</div>
                <div class="scm-tags">
                  {#if s.domain}<span class="scm-chip">{s.domain}</span>{/if}
                  {#each (s.mitre_attck || []).slice(0, 3) as code}<span class="scm-chip scm-chip-mitre">{code}</span>{/each}
                </div>
              </div>
              <div class="scm-row-acts">
                <button class="scm-act" title={isEN ? 'View' : 'Ver'} on:click={() => view(s)} disabled={busy === s.id}><Eye size={14}/></button>
                {#if s.id === activeId}
                  <button class="scm-act scm-act-on" title={isEN ? 'Deactivate' : 'Desactivar'} on:click={deactivate}><Bolt size={14}/></button>
                {:else}
                  <button class="scm-act" title={isEN ? 'Activate' : 'Activar'} on:click={() => activate(s)} disabled={busy === s.id}><Bolt size={14}/></button>
                {/if}
                {#if s.source === 'user'}
                  <button class="scm-act scm-act-danger" title={isEN ? 'Delete' : 'Eliminar'} on:click={() => del(s)} disabled={busy === s.id}><Trash2 size={14}/></button>
                {/if}
              </div>
            </div>
          {/each}
        {/if}
      </div>

      <div class="scm-foot">
        {isEN
          ? 'Add: drop a SKILL.md into the folder (or chat), or run /sec-skill new <id>, then Reload. Only user skills can be deleted.'
          : 'Agregar: arrastra un SKILL.md a la carpeta (o al chat), o usa /sec-skill new <id>, luego Recargar. Solo las skills de usuario se pueden eliminar.'}
      </div>
    {/if}
  </div>
</div>
{/if}

<style>
    .scm-bg {
        position: fixed; inset: 0; z-index: 10000;
        background: rgba(0,0,0,.78); backdrop-filter: blur(6px) saturate(120%);
        display: flex; align-items: center; justify-content: center;
        animation: scm-bg-in .18s ease;
    }
    @keyframes scm-bg-in { from { opacity: 0; } to { opacity: 1; } }
    .scm-box {
        background:
            radial-gradient(120% 60% at 50% -8%, rgba(16,185,129,.09) 0%, transparent 58%),
            var(--bg2, #0f141e);
        border: 1px solid var(--bdr, #1a2030);
        border-top: 3px solid var(--acc, #10b981);
        border-radius: 12px;
        width: 760px; max-width: 95vw; max-height: 86vh;
        display: flex; flex-direction: column;
        color: var(--txt, #e2e8f0);
        box-shadow: 0 24px 64px rgba(0,0,0,.7), 0 -1px 28px -10px rgba(16,185,129,.4);
        animation: scm-box-in .26s cubic-bezier(.16,1,.3,1);
    }
    @keyframes scm-box-in { from { opacity: 0; transform: translateY(14px) scale(.985); } to { opacity: 1; transform: none; } }

    .scm-hdr { display: flex; align-items: center; gap: 13px; padding: 16px 18px; border-bottom: 1px solid var(--bdr, #1a2030); flex-shrink: 0; }
    .scm-hdr-ico {
        display: inline-flex; align-items: center; justify-content: center;
        width: 40px; height: 40px; border-radius: 12px; flex-shrink: 0;
        background: rgba(16,185,129,.12); border: 1px solid rgba(16,185,129,.28);
        box-shadow: 0 0 18px -4px rgba(16,185,129,.5);
    }
    .scm-hdr-text { flex: 1; min-width: 0; }
    .scm-hdr-text h2 { margin: 0; font-size: 16px; font-weight: 600; color: var(--txt, #f1f5f9); }
    .scm-sub { margin: 2px 0 0; font-size: 11.5px; color: var(--txt2, #94a3b8); }
    .scm-hdr-acts { display: flex; align-items: center; gap: 6px; }
    .scm-tbtn {
        background: rgba(255,255,255,.04); border: 1px solid var(--bdr2, #222c3a);
        color: var(--txt2, #94a3b8); border-radius: 6px; cursor: pointer;
        width: 28px; height: 28px; display: flex; align-items: center; justify-content: center; transition: .15s;
    }
    .scm-tbtn:hover { background: rgba(16,185,129,.1); border-color: rgba(16,185,129,.3); color: var(--acc, #10b981); }
    .scm-close { background: none; border: none; color: #888; font-size: 18px; cursor: pointer; padding: 2px 6px; border-radius: 5px; }
    .scm-close:hover { color: var(--txt, #fff); background: rgba(255,255,255,.06); }

    .scm-tools { display: flex; gap: 8px; padding: 12px 18px 8px; flex-shrink: 0; }
    .scm-search { flex: 1; display: flex; align-items: center; gap: 7px; background: var(--bg, #0a0d14); border: 1px solid var(--bdr, #1a2030); border-radius: 7px; padding: 6px 10px; }
    .scm-search input { flex: 1; background: none; border: none; outline: none; color: var(--txt, #e2e8f0); font-size: 12.5px; font-family: inherit; }
    .scm-domain { background: var(--bg, #0a0d14); border: 1px solid var(--bdr, #1a2030); color: var(--txt, #e2e8f0); border-radius: 7px; font-size: 12px; padding: 6px 8px; cursor: pointer; max-width: 180px; }

    .scm-list { flex: 1; overflow-y: auto; padding: 4px 12px 8px; display: flex; flex-direction: column; gap: 5px; }
    .scm-empty { text-align: center; padding: 36px 16px; color: var(--txt3, #64748b); font-size: 12.5px; }
    .scm-err { color: #f87171; }

    .scm-row { display: flex; gap: 10px; padding: 9px 11px; background: rgba(255,255,255,.02); border: 1px solid var(--bdr, #1a2030); border-radius: 8px; transition: background .12s, border-color .12s; }
    .scm-row:hover { background: rgba(255,255,255,.04); border-color: rgba(255,255,255,.1); }
    .scm-row-active { border-color: rgba(16,185,129,.4); background: rgba(16,185,129,.05); }
    .scm-row-main { flex: 1; min-width: 0; }
    .scm-row-top { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
    .scm-name { font-size: 13px; font-weight: 600; color: var(--txt, #e8edf2); }
    .scm-badge { font-size: 9px; font-weight: 700; letter-spacing: .4px; padding: 1px 6px; border-radius: 8px; text-transform: uppercase; }
    .scm-badge-active { background: rgba(16,185,129,.14); color: var(--acc, #10b981); border: 1px solid rgba(16,185,129,.3); }
    .scm-src { font-size: 9px; font-weight: 700; padding: 1px 6px; border-radius: 8px; text-transform: uppercase; letter-spacing: .3px; font-family: var(--mono, monospace); }
    .scm-src-user { background: rgba(59,158,255,.12); color: #3b9eff; border: 1px solid rgba(59,158,255,.25); }
    .scm-src-bundled { background: rgba(255,255,255,.05); color: var(--txt3, #64748b); border: 1px solid rgba(255,255,255,.06); }
    .scm-desc { font-size: 11.5px; color: var(--txt2, #94a3b8); line-height: 1.45; margin: 3px 0; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
    .scm-tags { display: flex; gap: 4px; flex-wrap: wrap; }
    .scm-chip { font-size: 9.5px; padding: 1px 7px; border-radius: 8px; background: rgba(255,255,255,.04); color: var(--txt3, #64748b); border: 1px solid rgba(255,255,255,.05); }
    .scm-chip-mitre { background: rgba(239,68,68,.08); color: #f87171; border-color: rgba(239,68,68,.2); font-family: var(--mono, monospace); }
    .scm-chip-tag { background: rgba(167,139,250,.08); color: #a78bfa; border-color: rgba(167,139,250,.2); }

    .scm-row-acts { display: flex; align-items: flex-start; gap: 4px; flex-shrink: 0; }
    .scm-act { background: rgba(255,255,255,.04); border: 1px solid var(--bdr2, #222c3a); color: var(--txt2, #94a3b8); border-radius: 6px; cursor: pointer; width: 28px; height: 28px; display: flex; align-items: center; justify-content: center; transition: .12s; }
    .scm-act:hover:not(:disabled) { background: rgba(16,185,129,.1); border-color: rgba(16,185,129,.3); color: var(--acc, #10b981); }
    .scm-act:disabled { opacity: .4; cursor: not-allowed; }
    .scm-act-on { background: rgba(16,185,129,.14); border-color: rgba(16,185,129,.35); color: var(--acc, #10b981); }
    .scm-act-danger:hover:not(:disabled) { background: rgba(239,68,68,.1); border-color: rgba(239,68,68,.3); color: #f87171; }

    .scm-foot { padding: 8px 18px 12px; font-size: 10.5px; color: var(--txt3, #64748b); border-top: 1px solid var(--bdr, #1a2030); line-height: 1.5; flex-shrink: 0; }
    .scm-foot :global(code) { color: var(--acc, #10b981); }

    /* Detail / preview */
    .scm-detail { flex: 1; overflow-y: auto; padding: 14px 18px 16px; }
    .scm-back { background: none; border: none; color: var(--acc, #10b981); font-size: 12px; cursor: pointer; padding: 2px 0; margin-bottom: 8px; }
    .scm-back:hover { text-decoration: underline; }
    .scm-detail-hd { display: flex; align-items: center; gap: 8px; }
    .scm-detail-hd h3 { margin: 0; font-size: 15px; color: var(--txt, #f1f5f9); }
    .scm-detail-meta { display: flex; gap: 5px; flex-wrap: wrap; margin: 8px 0 10px; }
    .scm-detail-body { font-size: 12.5px; line-height: 1.6; color: var(--txt2, #94a3b8); border-top: 1px solid var(--bdr, #1a2030); padding-top: 10px; }
    .scm-detail-body :global(h1), .scm-detail-body :global(h2), .scm-detail-body :global(h3) { color: #e8edf2; font-size: 13.5px; margin: 12px 0 5px; }
    .scm-detail-body :global(code) { background: rgba(16,185,129,.06); color: var(--acc, #10b981); padding: 1px 5px; border-radius: 3px; font-family: var(--mono, monospace); font-size: 11.5px; }
    .scm-detail-body :global(pre) { background: rgba(0,0,0,.3); border: 1px solid var(--bdr, #1a2030); border-radius: 6px; padding: 9px 11px; overflow-x: auto; }
    .scm-detail-body :global(pre code) { background: none; color: #cbd5e1; padding: 0; }
    .scm-detail-body :global(ul), .scm-detail-body :global(ol) { padding-left: 18px; }
    .scm-detail-acts { display: flex; gap: 8px; margin-top: 14px; }
    .scm-btn { display: inline-flex; align-items: center; gap: 6px; padding: 7px 14px; border-radius: 7px; font-size: 12px; font-weight: 600; font-family: inherit; cursor: pointer; border: 1px solid transparent; }
    .scm-btn-pri { background: var(--acc, #10b981); color: #052e1c; }
    .scm-btn-pri:hover { filter: brightness(1.08); }
    .scm-btn-ghost { background: transparent; border-color: var(--bdr2, #222c3a); color: var(--txt2, #94a3b8); }
    .scm-btn-ghost:hover { color: var(--txt, #e2e8f0); }
    .scm-btn-danger { background: rgba(239,68,68,.1); border-color: rgba(239,68,68,.3); color: #f87171; }
    .scm-btn-danger:hover { background: rgba(239,68,68,.18); }

    /* ── Light mode (v1.7.232) — translucent-white fills wash out on the white
       modal surface; remap rows/controls to real plates with defined borders. */
    :global(:root.light) .scm-tbtn,
    :global(:root.light) .scm-act { background: var(--bg2); border-color: var(--bdr2); }
    :global(:root.light) .scm-row { background: var(--bg4); border-color: var(--bdr); }
    :global(:root.light) .scm-row:hover { background: var(--bg2); border-color: var(--bdr2); }
    :global(:root.light) .scm-close:hover,
    :global(:root.light) .scm-act:hover { background: rgba(15,23,42,.05); }
    :global(:root.light) .scm-chip,
    :global(:root.light) .scm-src-bundled { background: var(--bg2); border-color: var(--bdr); }
</style>
