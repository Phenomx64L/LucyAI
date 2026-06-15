<script>
    import { createEventDispatcher } from 'svelte';
    import { profiles, hosts } from '$lib/stores';
    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';

    import Pencil from '@tabler/icons-svelte/icons/pencil';

    import Trash2 from '@tabler/icons-svelte/icons/trash';

    import User from '@tabler/icons-svelte/icons/user';
    const dispatch = createEventDispatcher();

    export let isEN = false;

    let editingId   = null;
    let form        = { name: '', icon: '⊞', hostIds: [] };
    let showForm    = false;
    let importError = '';

    const ICONS = ['⊞','◈','◉','⬡','⊕','⊟','·','▶','⚡','○','⊡','⊗','⚙'];

    function resetForm() {
        form = { name: '', icon: '⊞', hostIds: [] };
        editingId = null;
        showForm = false;
    }

    function openNew() {
        resetForm();
        showForm = true;
    }

    function openEdit(p) {
        editingId = p.id;
        form = { name: p.name, icon: p.icon, hostIds: [...p.hostIds] };
        showForm = true;
    }

    function toggleHost(id) {
        if (form.hostIds.includes(id)) {
            form.hostIds = form.hostIds.filter(x => x !== id);
        } else {
            form.hostIds = [...form.hostIds, id];
        }
    }

    function saveProfile() {
        if (!form.name.trim()) return;
        if (editingId) {
            $profiles = $profiles.map(p => p.id === editingId
                ? { ...p, name: form.name, icon: form.icon, hostIds: form.hostIds }
                : p
            );
        } else {
            const newP = {
                id: 'prof_' + Date.now(),
                name: form.name,
                icon: form.icon,
                hostIds: form.hostIds,
                createdAt: Date.now(),
                lastUsed: Date.now(),
            };
            $profiles = [...$profiles, newP];
        }
        resetForm();
        dispatch('toast', { msg: isEN ? 'Profile saved' : 'Perfil guardado', type: 'info' });
    }

    function deleteProfile(id) {
        $profiles = $profiles.filter(p => p.id !== id);
        dispatch('toast', { msg: isEN ? 'Profile deleted' : 'Perfil eliminado', type: 'info' });
    }

    function exportProfiles() {
        const data = JSON.stringify($profiles, null, 2);
        const blob = new Blob([data], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url; a.download = `lucy-profiles-${new Date().toISOString().split('T')[0]}.json`;
        a.click(); URL.revokeObjectURL(url);
    }

    function importProfiles(e) {
        importError = '';
        const file = e.target?.files?.[0];
        if (!file) return;
        const reader = new FileReader();
        reader.onload = () => {
            try {
                const arr = JSON.parse(reader.result);
                if (!Array.isArray(arr)) throw new Error('Not an array');
                let imported = 0;
                for (const p of arr) {
                    if (!p.name || !p.id) continue;
                    if ($profiles.find(x => x.id === p.id)) continue;
                    $profiles = [...$profiles, { ...p, createdAt: p.createdAt || Date.now(), lastUsed: Date.now() }];
                    imported++;
                }
                dispatch('toast', { msg: `${imported} ${isEN ? 'profiles imported' : 'perfiles importados'}`, type: 'info' });
            } catch(err) {
                importError = String(err);
            }
        };
        reader.readAsText(file);
        e.target.value = '';
    }

    function close() { dispatch('close'); }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="pm-overlay" on:click|self={close}>
  <div class="pm-modal">
    <div class="pm-hdr">
      <span class="pm-hdr-ico"><User size={16} strokeWidth={1.9} color="var(--acc, #10b981)"/></span>
      <span class="pm-title">{isEN ? 'Manage Profiles' : 'Gestionar Perfiles'}</span>
      <div style="display:flex;gap:6px;margin-left:auto;">
        <button class="pm-btn sm" on:click={exportProfiles} title="Export JSON">⬇ JSON</button>
        <label class="pm-btn sm" title="Import JSON">
          ⬆ Import
          <input type="file" accept=".json" on:change={importProfiles} style="display:none;">
        </label>
        <button class="pm-btn sm" on:click={openNew}>+ {isEN ? 'New' : 'Nuevo'}</button>
      </div>
      <button class="pm-close" on:click={close}>✕</button>
    </div>

    {#if importError}
    <div class="pm-error" style="display:flex;align-items:center;gap:6px;"><AlertTriangle size={12} strokeWidth={2}/> {importError}</div>
    {/if}

    {#if showForm}
    <div class="pm-form">
      <div class="pm-form-row">
        <label for="pm-name">{isEN ? 'Name' : 'Nombre'}</label>
        <input id="pm-name" type="text" class="pm-input" bind:value={form.name} placeholder={isEN ? 'e.g. Production' : 'ej. Producción'} maxlength="40">
      </div>
      <div class="pm-form-row">
        <span>{isEN ? 'Icon' : 'Icono'}</span>
        <div class="pm-icons">
          {#each ICONS as ic}
          <button class="pm-icon-btn" class:sel={form.icon === ic} on:click={() => form.icon = ic}>{ic}</button>
          {/each}
        </div>
      </div>
      <div class="pm-form-row">
        <span>{isEN ? 'Hosts' : 'Hosts'} ({form.hostIds.length})</span>
        <div class="pm-hosts">
          {#each $hosts as h}
          <button class="pm-host-btn" class:sel={form.hostIds.includes(h.id)} on:click={() => toggleHost(h.id)}>
            {h.type === 'windows' ? '⊡' : '◈'} {h.name}
          </button>
          {/each}
          {#if $hosts.length === 0}
          <span class="pm-nohosts">{isEN ? 'No hosts configured' : 'Sin hosts configurados'}</span>
          {/if}
        </div>
      </div>
      <div class="pm-form-actions">
        <button class="pm-btn" on:click={resetForm}>{isEN ? 'Cancel' : 'Cancelar'}</button>
        <button class="pm-btn accent" on:click={saveProfile} disabled={!form.name.trim()}>
          {editingId ? (isEN ? 'Update' : 'Actualizar') : (isEN ? 'Create' : 'Crear')}
        </button>
      </div>
    </div>
    {/if}

    <div class="pm-list">
      {#each $profiles as p}
      <div class="pm-item">
        <span class="pm-item-icon">{p.icon}</span>
        <div class="pm-item-info">
          <div class="pm-item-name">{p.name}</div>
          <div class="pm-item-meta">{p.hostIds.length} hosts · {isEN ? 'Created' : 'Creado'} {new Date(p.createdAt).toLocaleDateString()}</div>
        </div>
        <button class="pm-btn sm" on:click={() => openEdit(p)} style="display:flex;align-items:center;justify-content:center;"><Pencil size={12} strokeWidth={2}/></button>
        <button class="pm-btn sm danger" on:click={() => deleteProfile(p.id)} style="display:flex;align-items:center;justify-content:center;"><Trash2 size={12} strokeWidth={2}/></button>
      </div>
      {/each}
      {#if $profiles.length === 0 && !showForm}
      <div class="pm-empty">
        <div style="font-size:32px;margin-bottom:8px;display:flex;justify-content:center;"><User size={32} strokeWidth={1.5} style="color:var(--txt3)"/></div>
        <div>{isEN ? 'No profiles yet. Create one to group your hosts.' : 'Sin perfiles. Crea uno para agrupar tus hosts.'}</div>
        <button class="pm-btn accent" style="margin-top:12px;" on:click={openNew}>+ {isEN ? 'Create Profile' : 'Crear Perfil'}</button>
      </div>
      {/if}
    </div>
  </div>
</div>

<style>
    .pm-overlay{position:fixed;inset:0;z-index:var(--z-modal, 2000);background:rgba(0,0,0,.55);display:flex;align-items:center;justify-content:center;backdrop-filter:blur(6px) saturate(120%);animation:pm-bg-in .18s ease;}
    @keyframes pm-bg-in{from{opacity:0;}to{opacity:1;}}
    .pm-modal{
        /* Depth: faint accent top glow over the base instead of a flat slab. */
        background:
            radial-gradient(120% 60% at 50% -8%, rgba(16,185,129,.09) 0%, transparent 58%),
            var(--bg2,#0f1724);
        border:1px solid var(--bdr);
        border-top:3px solid var(--acc, #10b981);
        border-radius:12px;width:min(560px,92vw);max-height:80vh;
        display:flex;flex-direction:column;overflow:hidden;
        box-shadow:0 24px 64px rgba(0,0,0,.6), 0 -1px 28px -10px rgba(16,185,129,.4);
        animation:pm-box-in .26s cubic-bezier(.16,1,.3,1);
    }
    @keyframes pm-box-in{from{opacity:0;transform:translateY(14px) scale(.985);}to{opacity:1;transform:none;}}
    .pm-hdr{display:flex;align-items:center;gap:8px;padding:12px 16px;border-bottom:1px solid var(--bdr);flex-shrink:0;}
    .pm-hdr-ico{display:inline-flex;align-items:center;justify-content:center;width:30px;height:30px;border-radius:9px;flex-shrink:0;background:rgba(16,185,129,.12);border:1px solid rgba(16,185,129,.28);box-shadow:0 0 14px -4px rgba(16,185,129,.5);}
    .pm-title{font-size:14px;font-weight:700;color:var(--txt);}
    .pm-close{background:none;border:none;color:#4a5a6a;font-size:16px;cursor:pointer;padding:4px;margin-left:8px;}
    .pm-close:hover{color:var(--txt);}
    .pm-error{margin:8px 16px 0;padding:8px 12px;background:rgba(255,68,68,.08);border:1px solid rgba(255,68,68,.2);border-radius:6px;font-size:11px;color:var(--red);}

    .pm-btn{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt2);font-size:11px;padding:4px 10px;cursor:pointer;transition:.15s;white-space:nowrap;}
    .pm-btn:hover{background:var(--bdr2);color:var(--txt);}
    .pm-btn.sm{font-size:10px;padding:3px 7px;}
    .pm-btn.accent{background:rgba(16,185,129,.1);border-color:rgba(16,185,129,.2);color:var(--acc);}
    .pm-btn.accent:hover{background:rgba(16,185,129,.18);}
    .pm-btn.accent:disabled{opacity:.4;cursor:not-allowed;}
    .pm-btn.danger:hover{background:rgba(255,68,68,.12);border-color:rgba(255,68,68,.3);color:var(--red);}

    .pm-form{padding:12px 16px;border-bottom:1px solid var(--bdr);display:flex;flex-direction:column;gap:10px;flex-shrink:0;}
    .pm-form-row{display:flex;flex-direction:column;gap:4px;}
    .pm-form-row label{font-size:10px;color:#4a5a6a;font-weight:600;text-transform:uppercase;letter-spacing:.3px;}
    .pm-input{background:rgba(0,0,0,.2);border:1px solid var(--bdr);color:var(--txt);padding:6px 10px;border-radius:6px;font-size:12px;outline:none;font-family:inherit;}
    .pm-input:focus{border-color:var(--acc-b);}
    .pm-icons{display:flex;gap:4px;flex-wrap:wrap;}
    .pm-icon-btn{width:32px;height:32px;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,.15);border:1px solid var(--bdr);border-radius:6px;cursor:pointer;font-size:16px;transition:.15s;}
    .pm-icon-btn:hover{border-color:rgba(255,255,255,.1);}
    .pm-icon-btn.sel{border-color:var(--acc);background:rgba(16,185,129,.08);box-shadow:0 0 8px rgba(16,185,129,.1);}
    .pm-hosts{display:flex;gap:4px;flex-wrap:wrap;max-height:120px;overflow-y:auto;}
    .pm-host-btn{background:rgba(0,0,0,.15);border:1px solid var(--bdr);border-radius:5px;padding:4px 8px;font-size:11px;color:var(--txt2);cursor:pointer;transition:.15s;white-space:nowrap;}
    .pm-host-btn:hover{border-color:rgba(255,255,255,.1);}
    .pm-host-btn.sel{border-color:var(--acc);background:rgba(16,185,129,.08);color:var(--acc);}
    .pm-nohosts{font-size:11px;color:var(--txt3);font-style:italic;}
    .pm-form-actions{display:flex;gap:8px;justify-content:flex-end;}

    .pm-list{flex:1;overflow-y:auto;padding:8px 16px 16px;}
    .pm-item{display:flex;align-items:center;gap:10px;padding:10px 12px;border:1px solid var(--bdr);border-radius:8px;margin-bottom:6px;transition:.15s;}
    .pm-item:hover{border-color:rgba(255,255,255,.06);}
    .pm-item-icon{font-size:22px;flex-shrink:0;}
    .pm-item-info{flex:1;min-width:0;}
    .pm-item-name{font-size:13px;font-weight:600;color:var(--txt);}
    .pm-item-meta{font-size:10px;color:#4a5a6a;margin-top:2px;}

    .pm-empty{text-align:center;padding:40px 20px;color:var(--txt3);font-size:12px;}

    :global(:root.light) .pm-modal{background:#fff;}
    :global(:root.light) .pm-item{background:#fff;}
</style>
