<script>
    import { createEventDispatcher } from 'svelte';
    import Books from '@tabler/icons-svelte/icons/books';
    import { getAllSkills, getSkillsByCategory, searchSkills, categoryIcon, categoryLabel } from '$lib/skills/skill-engine';

    const dispatch = createEventDispatcher();

    export let isEN = false;
    export let hostType = 'linux';

    let query = '';
    let selectedCategory = 'all';
    let selectedSkill = null;
    let userInput = '';

    const categories = ['all', 'network', 'security', 'storage', 'services', 'monitoring'];

    $: skills = (() => {
        let list = query.trim()
            ? searchSkills(query.trim())
            : selectedCategory === 'all'
                ? getAllSkills()
                : getSkillsByCategory(selectedCategory);
        // Filter by OS compatibility
        return list.filter(s => s.os === 'both' || s.os === hostType);
    })();

    function selectSkill(skill) {
        selectedSkill = skill;
        userInput = '';
    }

    function runSkill() {
        if (!selectedSkill) return;
        dispatch('run', { skill: selectedSkill, userInput: userInput.trim() });
    }

    function close() {
        dispatch('close');
    }

    function handleKey(e) {
        if (e.key === 'Escape') {
            if (selectedSkill) selectedSkill = null;
            else close();
        }
    }
</script>

<svelte:window on:keydown={handleKey} />

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="sk-overlay" on:click|self={close}>
  <div class="sk-modal">
    <!-- Header -->
    <div class="sk-hdr">
      <span class="sk-hdr-ico"><Books size={18} strokeWidth={1.9} color="var(--acc, #10b981)"/></span>
      <span class="sk-hdr-title">{isEN ? 'Skill Browser' : 'Explorador de Skills'}</span>
      <button class="sk-close" on:click={close}>✕</button>
    </div>

    {#if !selectedSkill}
    <!-- ── BROWSE VIEW ── -->
    <div class="sk-browse">
      <!-- Search -->
      <div class="sk-search">
        <input type="text" class="sk-search-input"
          placeholder={isEN ? 'Search skills...' : 'Buscar skills...'}
          bind:value={query} />
      </div>

      <!-- Category tabs -->
      <div class="sk-cats">
        {#each categories as cat}
        <button class="sk-cat-btn" class:active={selectedCategory === cat}
          on:click={() => { selectedCategory = cat; query = ''; }}>
          {cat === 'all' ? '🔧' : categoryIcon(cat)}
          {cat === 'all' ? (isEN ? 'All' : 'Todas') : categoryLabel(cat, isEN)}
        </button>
        {/each}
      </div>

      <!-- Skill cards -->
      <div class="sk-grid">
        {#each skills as skill (skill.id)}
        <button class="sk-card" on:click={() => selectSkill(skill)}>
          <div class="sk-card-ico">{skill.icon}</div>
          <div class="sk-card-body">
            <div class="sk-card-name">{isEN ? skill.nameEN : skill.name}</div>
            <div class="sk-card-desc">{isEN ? skill.descriptionEN : skill.description}</div>
            <div class="sk-card-meta">
              <span class="sk-tag">{skill.phases.length} {isEN ? 'phases' : 'fases'}</span>
              <span class="sk-tag">{skill.os === 'both' ? 'Linux/Win' : skill.os}</span>
              {#if skill.version}<span class="sk-tag">v{skill.version}</span>{/if}
            </div>
          </div>
        </button>
        {/each}
        {#if skills.length === 0}
        <div class="sk-empty">{isEN ? 'No skills found.' : 'No se encontraron skills.'}</div>
        {/if}
      </div>
    </div>

    {:else}
    <!-- ── DETAIL VIEW ── -->
    <div class="sk-detail">
      <button class="sk-back" on:click={() => selectedSkill = null}>
        ← {isEN ? 'Back' : 'Volver'}
      </button>

      <div class="sk-detail-hdr">
        <span class="sk-detail-ico">{selectedSkill.icon}</span>
        <div>
          <div class="sk-detail-name">{isEN ? selectedSkill.nameEN : selectedSkill.name}</div>
          <div class="sk-detail-desc">{isEN ? selectedSkill.descriptionEN : selectedSkill.description}</div>
        </div>
      </div>

      <!-- Phases list -->
      <div class="sk-phases-title">{isEN ? 'Phases' : 'Fases'} ({selectedSkill.phases.length})</div>
      <div class="sk-phases">
        {#each selectedSkill.phases as phase, i}
        <div class="sk-phase" class:optional={phase.optional}>
          <div class="sk-phase-num">{i + 1}</div>
          <div class="sk-phase-info">
            <div class="sk-phase-name">
              {isEN ? phase.nameEN : phase.name}
              {#if phase.optional}<span class="sk-optional-tag">{isEN ? 'optional' : 'opcional'}</span>{/if}
              {#if phase.expectVerdict}<span class="sk-verdict-tag">verdict</span>{/if}
            </div>
          </div>
        </div>
        {/each}
      </div>

      <!-- Tags -->
      <div class="sk-tags-row">
        {#each selectedSkill.tags as tag}
        <span class="sk-tag-pill">{tag}</span>
        {/each}
      </div>

      <!-- User context input -->
      <div class="sk-run-section">
        <label for="sk-ctx" class="sk-run-label">{isEN ? 'Additional context (optional):' : 'Contexto adicional (opcional):'}</label>
        <textarea id="sk-ctx" class="sk-run-input" rows="2"
          placeholder={isEN ? 'e.g., domain: example.com, service: nginx...' : 'ej: dominio: ejemplo.com, servicio: nginx...'}
          bind:value={userInput}></textarea>
        <button class="sk-run-btn" on:click={runSkill}>
          ▶ {isEN ? 'Run Skill' : 'Ejecutar Skill'}
        </button>
      </div>
    </div>
    {/if}
  </div>
</div>

<style>
    .sk-overlay{position:fixed;inset:0;background:rgba(0,0,0,.6);display:flex;align-items:center;justify-content:center;z-index:900;backdrop-filter:blur(6px) saturate(120%);animation:sk-bg-in .18s ease;}
    @keyframes sk-bg-in{from{opacity:0;}to{opacity:1;}}
    .sk-modal{
      /* Depth: faint accent top glow over the base instead of a flat slab. */
      background:radial-gradient(120% 55% at 50% -6%, rgba(16,185,129,.08) 0%, transparent 56%),var(--bg2,#0d1117);
      border:1px solid var(--bdr,#1e2a3a);
      border-top:3px solid var(--acc,#10b981);
      border-radius:12px;width:640px;max-width:90vw;max-height:80vh;
      display:flex;flex-direction:column;overflow:hidden;
      box-shadow:0 24px 64px rgba(0,0,0,.5),0 -1px 28px -10px rgba(16,185,129,.4);
      animation:sk-modal-in .26s cubic-bezier(.16,1,.3,1);
    }
    @keyframes sk-modal-in{from{opacity:0;transform:translateY(14px) scale(.985);}to{opacity:1;transform:none;}}

    .sk-hdr{display:flex;align-items:center;gap:8px;padding:12px 16px;border-bottom:1px solid var(--bdr);}
    .sk-hdr-ico{display:inline-flex;align-items:center;justify-content:center;width:34px;height:34px;border-radius:10px;flex-shrink:0;background:rgba(16,185,129,.12);border:1px solid rgba(16,185,129,.28);box-shadow:0 0 16px -4px rgba(16,185,129,.5);}
    .sk-hdr-title{font-weight:700;font-size:14px;color:var(--txt);flex:1;}
    .sk-close{background:none;border:none;color:#4a5a6a;font-size:16px;cursor:pointer;padding:4px 8px;border-radius:4px;}
    .sk-close:hover{color:var(--txt);background:rgba(255,255,255,.06);}

    .sk-browse{display:flex;flex-direction:column;overflow:hidden;flex:1;}
    .sk-search{padding:10px 16px 6px;}
    .sk-search-input{width:100%;background:var(--bg,#010409);border:1px solid var(--bdr);border-radius:8px;padding:8px 12px;color:var(--txt);font-size:13px;outline:none;box-sizing:border-box;}
    .sk-search-input:focus{border-color:rgba(16,185,129,.3);}

    .sk-cats{display:flex;gap:4px;padding:6px 16px;overflow-x:auto;flex-shrink:0;}
    .sk-cat-btn{background:rgba(255,255,255,.03);border:1px solid transparent;border-radius:6px;padding:4px 10px;font-size:11px;color:#4a5a6a;cursor:pointer;white-space:nowrap;transition:.15s;}
    .sk-cat-btn:hover{background:rgba(255,255,255,.06);color:var(--txt);}
    .sk-cat-btn.active{border-color:rgba(16,185,129,.25);color:var(--acc);background:rgba(16,185,129,.06);}

    .sk-grid{flex:1;overflow-y:auto;padding:8px 16px 16px;display:flex;flex-direction:column;gap:6px;}
    .sk-card{display:flex;gap:12px;padding:10px 12px;background:rgba(255,255,255,.02);border:1px solid var(--bdr);border-radius:8px;cursor:pointer;transition:.15s;text-align:left;color:var(--txt);}
    .sk-card:hover{background:rgba(255,255,255,.05);border-color:rgba(16,185,129,.2);}
    .sk-card-ico{font-size:24px;flex-shrink:0;padding-top:2px;}
    .sk-card-body{flex:1;min-width:0;}
    .sk-card-name{font-weight:700;font-size:13px;margin-bottom:2px;}
    .sk-card-desc{font-size:11px;color:#5a6a7a;line-height:1.4;margin-bottom:4px;}
    .sk-card-meta{display:flex;gap:4px;flex-wrap:wrap;}
    .sk-tag{font-size:9px;padding:1px 6px;border-radius:3px;background:rgba(255,255,255,.04);color:#4a5a6a;border:1px solid rgba(255,255,255,.04);}

    .sk-empty{text-align:center;color:var(--txt3);font-size:12px;padding:24px 0;}

    /* Detail view */
    .sk-detail{flex:1;overflow-y:auto;padding:12px 16px 16px;}
    .sk-back{background:none;border:none;color:var(--acc);font-size:12px;cursor:pointer;padding:4px 0;margin-bottom:8px;}
    .sk-back:hover{text-decoration:underline;}

    .sk-detail-hdr{display:flex;gap:12px;margin-bottom:16px;}
    .sk-detail-ico{font-size:32px;flex-shrink:0;}
    .sk-detail-name{font-weight:700;font-size:16px;color:var(--txt);}
    .sk-detail-desc{font-size:12px;color:#5a6a7a;margin-top:2px;line-height:1.4;}

    .sk-phases-title{font-size:11px;font-weight:700;color:#4a5a6a;text-transform:uppercase;letter-spacing:.4px;margin-bottom:6px;}
    .sk-phases{display:flex;flex-direction:column;gap:4px;margin-bottom:12px;}
    .sk-phase{display:flex;align-items:center;gap:8px;padding:6px 8px;background:rgba(255,255,255,.02);border-radius:6px;border:1px solid rgba(255,255,255,.03);}
    .sk-phase.optional{opacity:.7;}
    .sk-phase-num{width:22px;height:22px;border-radius:50%;background:rgba(16,185,129,.08);border:1px solid rgba(16,185,129,.15);display:flex;align-items:center;justify-content:center;font-size:10px;font-weight:700;color:var(--acc);flex-shrink:0;}
    .sk-phase-info{flex:1;}
    .sk-phase-name{font-size:12px;color:var(--txt);display:flex;align-items:center;gap:6px;}
    .sk-optional-tag{font-size:8px;padding:1px 5px;border-radius:3px;background:rgba(255,170,0,.1);color:var(--amber,#ffaa33);border:1px solid rgba(255,170,0,.15);}
    .sk-verdict-tag{font-size:8px;padding:1px 5px;border-radius:3px;background:rgba(74,168,255,.08);color:var(--blue,#4aa8ff);border:1px solid rgba(74,168,255,.15);}

    .sk-tags-row{display:flex;gap:4px;flex-wrap:wrap;margin-bottom:14px;}
    .sk-tag-pill{font-size:10px;padding:2px 8px;border-radius:10px;background:rgba(255,255,255,.04);color:#5a6a7a;border:1px solid rgba(255,255,255,.05);}

    .sk-run-section{border-top:1px solid var(--bdr);padding-top:12px;}
    .sk-run-label{font-size:11px;color:#4a5a6a;display:block;margin-bottom:4px;}
    .sk-run-input{width:100%;background:var(--bg);border:1px solid var(--bdr);border-radius:6px;padding:8px 10px;color:var(--txt);font-size:12px;resize:vertical;outline:none;font-family:inherit;box-sizing:border-box;}
    .sk-run-input:focus{border-color:rgba(16,185,129,.3);}
    .sk-run-btn{margin-top:8px;width:100%;padding:10px;background:linear-gradient(135deg, rgba(16,185,129,.12), rgba(13,150,104,.08));border:1px solid rgba(16,185,129,.25);border-radius:8px;color:var(--acc);font-weight:700;font-size:13px;cursor:pointer;transition:.2s;}
    .sk-run-btn:hover{background:linear-gradient(135deg, rgba(16,185,129,.2), rgba(13,150,104,.15));box-shadow:0 0 16px rgba(16,185,129,.15);}
</style>
