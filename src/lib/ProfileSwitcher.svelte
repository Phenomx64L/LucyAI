<script>
    import { createEventDispatcher } from 'svelte';
    import { profiles, activeProfileId } from '$lib/stores';

    const dispatch = createEventDispatcher();

    export let isEN = false;

    let open = false;

    $: active = $profiles.find(p => p.id === $activeProfileId);

    function select(id) {
        if ($activeProfileId === id) {
            $activeProfileId = null;
        } else {
            $activeProfileId = id;
            const p = $profiles.find(x => x.id === id);
            if (p) {
                p.lastUsed = Date.now();
                $profiles = $profiles;
            }
        }
        open = false;
        dispatch('change');
    }

    function handleKey(e) {
        if (e.key === 'Escape') open = false;
    }
</script>

<svelte:window on:keydown={handleKey} />

<div class="ps-wrap">
    <button class="ps-btn" on:click={() => open = !open} title={isEN ? 'Switch profile' : 'Cambiar perfil'}>
        <span class="ps-icon">{active ? active.icon : '👤'}</span>
        <span class="ps-name">{active ? active.name : (isEN ? 'All Hosts' : 'Todos')}</span>
        <span class="ps-caret">{open ? '▴' : '▾'}</span>
    </button>

    {#if open}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="ps-backdrop" on:click={() => open = false}></div>
    <div class="ps-dropdown">
        <button class="ps-opt" class:active={!$activeProfileId} on:click={() => select(null)}>
            <span class="ps-opt-icon">🌐</span>
            <span>{isEN ? 'All Hosts (No Filter)' : 'Todos los Hosts (Sin Filtro)'}</span>
        </button>
        {#each $profiles as p}
        <button class="ps-opt" class:active={$activeProfileId === p.id} on:click={() => select(p.id)}>
            <span class="ps-opt-icon">{p.icon}</span>
            <span class="ps-opt-name">{p.name}</span>
            <span class="ps-opt-count">{p.hostIds.length}h</span>
        </button>
        {/each}
        {#if $profiles.length === 0}
        <div class="ps-empty">{isEN ? 'No profiles yet' : 'Sin perfiles aún'}</div>
        {/if}
        <button class="ps-opt ps-add" on:click={() => { open = false; dispatch('manage'); }}>
            <span class="ps-opt-icon">⚙</span>
            <span>{isEN ? 'Manage Profiles...' : 'Gestionar Perfiles...'}</span>
        </button>
    </div>
    {/if}
</div>

<style>
    .ps-wrap{position:relative;}
    .ps-btn{display:flex;align-items:center;gap:4px;background:rgba(0,0,0,.25);border:1px solid var(--bdr);border-radius:6px;padding:3px 8px;cursor:pointer;transition:.15s;color:var(--txt2);font-size:11px;white-space:nowrap;}
    .ps-btn:hover{border-color:var(--acc-b);color:var(--txt);}
    .ps-icon{font-size:13px;}
    .ps-name{max-width:100px;overflow:hidden;text-overflow:ellipsis;}
    .ps-caret{font-size:9px;color:#475569;}

    .ps-backdrop{position:fixed;inset:0;z-index:900;}
    .ps-dropdown{position:absolute;top:calc(100% + 4px);right:0;background:var(--bg2,#161822);border:1px solid var(--bdr);border-radius:8px;min-width:200px;z-index:901;box-shadow:0 8px 24px rgba(0,0,0,.4);overflow:hidden;}

    .ps-opt{display:flex;align-items:center;gap:8px;width:100%;background:none;border:none;padding:8px 12px;cursor:pointer;font-size:11px;color:var(--txt2);transition:.1s;text-align:left;}
    .ps-opt:hover{background:rgba(16,185,129,.06);color:var(--txt);}
    .ps-opt.active{background:rgba(16,185,129,.08);color:var(--acc);}
    .ps-opt-icon{font-size:14px;flex-shrink:0;}
    .ps-opt-name{flex:1;overflow:hidden;text-overflow:ellipsis;}
    .ps-opt-count{font-size:9px;color:#475569;font-family:var(--mono);}
    .ps-add{border-top:1px solid var(--bdr);color:#475569;}
    .ps-add:hover{color:var(--acc);}
    .ps-empty{padding:12px;text-align:center;font-size:10px;color:#475569;}

    :global(:root.light) .ps-dropdown{background:#fff;box-shadow:0 8px 24px rgba(0,0,0,.12);}
</style>
