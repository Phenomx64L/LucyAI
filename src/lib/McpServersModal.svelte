<!-- ── McpServersModal.svelte ──────────────────────────────────────────────
     First-class MCP (Model Context Protocol) servers manager.

     Parity goal: equivalent UX to claude_desktop_config.json / Cursor /
     Cline — name a server once, pick which secrets it needs from the
     Keyring bag, and Lucy invokes it by name in subsequent turns. Tools
     discovered via tools/list are cached for the system prompt block.

     Props
       isOpen   — open/closed (parent owns the boolean)
       isEN     — language switch
       mcpSecrets — full Keyring bag {KEY: VALUE} (passed in by parent so
                    we don't re-read the OS Keyring on every render)

     Events
       close      — modal dismissed
       updated    — list of servers changed (parent reloads cache)
─────────────────────────────────────────────────────────────────────── -->
<script>
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
  import { createEventDispatcher, onMount } from 'svelte';
  import { autoAnimate } from '$lib/actions/autoAnimate';
  // v1.4.13 — Dialog primitives from bits-ui replace the hand-rolled
  // backdrop + onKey listener. Focus trap, portal, Escape-route
  // through the dialog stack, aria-modal/labelledby all wired by
  // the primitive. Visual identity (modal-card styles) is preserved.
  import { Dialog } from 'bits-ui';
  // v1.4.15 — loading skeletons + empty-state component.
  import Skeleton from '$lib/Skeleton.svelte';
  import EmptyState from '$lib/EmptyState.svelte';
  // v1.6.2 — ECC-adapted MCP budget guard chip.
  import McpBudgetChip from '$lib/McpBudgetChip.svelte';
  // v1.4.15 — toast.promise for long ops (discover/test). Aliased so it
  // doesn't shadow the existing inline `toast` banner variable below.
  import { toast as sonnerToast } from 'svelte-sonner';
  import { invoke } from '@tauri-apps/api/core';
  import ConfirmModal from '$lib/ConfirmModal.svelte';
  import Plug from '@tabler/icons-svelte/icons/plug';

  export let isOpen = false;
  export let isEN = false;
  export let mcpSecrets = {};

  const dispatch = createEventDispatcher();

  let servers = [];
  let loading = false;
  let error = '';
  let busyName = '';               // name of server currently being tested/discovered
  let expandedName = '';           // name of server whose tools list is expanded
  let confirmDelete = null;        // { name } when pending confirm
  let toast = '';                  // ephemeral status banner

  // ── Form state ──────────────────────────────────────────────────────
  let formOpen = false;
  let editingOriginalName = '';    // '' when adding a new one
  let form = freshForm();

  // ── Manual tool invocation panel ────────────────────────────────────
  // Lets the operator try a tool from the cached catalog without going
  // through Lucy. Useful to verify args/schema shape.
  let runPanel = null; // { serverName, toolName, argsJson, output, error, running }

  function freshForm() {
    return { name: '', command: '', envKeys: [], enabled: true };
  }

  const T = {
    title:     $trad('Servidores MCP'),
    subtitle:  $trad('Registra servidores MCP una vez y Lucy los invoca por nombre. Las tools descubiertas vía tools/list se cachean para el system prompt.'),
    add:       $trad('+ Añadir servidor'),
    name:      $trad('Nombre'),
    command:   $trad('Comando'),
    envKeys:   $trad('Secretos a inyectar (env vars)'),
    enabled:   $trad('Habilitado'),
    save:      $trad('Guardar'),
    cancel:    $trad('Cancelar'),
    edit:      $trad('Editar'),
    delete:    $trad('Eliminar'),
    test:      $trad('Probar'),
    discover:  $trad('Descubrir tools'),
    tools:     $trad('Tools en caché'),
    noServers: $trad('Aún no hay servidores registrados.'),
    noTools:   $trad('Sin tools en caché. Pulsa "Descubrir tools" para llenarla.'),
    namePh:    $trad('ej. filesystem'),
    cmdPh:     'npx -y @modelcontextprotocol/server-filesystem C:/Users/eleue/Desktop',
    runTool:   $trad('Invocar'),
    runPanel:  $trad('Invocar tool'),
    runArgs:   $trad('Argumentos (JSON)'),
    confirmDel: $trad('¿Eliminar este servidor MCP?'),
    status_ok:    $trad('OK'),
    status_err:   $trad('Error'),
    status_pend:  $trad('Pendiente'),
    presets:   $trad('Plantillas'),
    close:     $trad('Cerrar'),
  };

  // ── Curated presets — these are the official anthropic/community servers
  //    most useful for a SysAdmin / dev workflow. Picking one fills the
  //    form; the user then tweaks the path / env keys before saving.
  const PRESETS = [
    {
      name: 'filesystem',
      command: 'npx -y @modelcontextprotocol/server-filesystem C:/Users/eleue/Desktop',
      envKeys: [],
      hint: $trad('Lee/escribe archivos bajo una raíz sandboxed.'),
    },
    {
      name: 'github',
      command: 'npx -y @modelcontextprotocol/server-github',
      envKeys: ['GITHUB_PERSONAL_ACCESS_TOKEN'],
      hint: $trad('Issues, PRs, búsqueda de código. Requiere GITHUB_PERSONAL_ACCESS_TOKEN.'),
    },
    {
      name: 'brave-search',
      command: 'npx -y @modelcontextprotocol/server-brave-search',
      envKeys: ['BRAVE_API_KEY'],
      hint: $trad('Búsqueda web vía Brave API.'),
    },
    {
      name: 'postgres',
      command: 'npx -y @modelcontextprotocol/server-postgres postgresql://user:pass@host:5432/db',
      envKeys: [],
      hint: $trad('Queries SQL read-only. Edita la URL antes de guardar.'),
    },
    {
      name: 'puppeteer',
      command: 'npx -y @modelcontextprotocol/server-puppeteer',
      envKeys: [],
      hint: $trad('Scraping con browser headless.'),
    },
    {
      name: 'slack',
      command: 'npx -y @modelcontextprotocol/server-slack',
      envKeys: ['SLACK_BOT_TOKEN', 'SLACK_TEAM_ID'],
      hint: $trad('Postea / lee canales. Requiere SLACK_BOT_TOKEN + SLACK_TEAM_ID.'),
    },
  ];

  $: secretKeys = Object.keys(mcpSecrets || {}).sort();

  // ── Lifecycle ───────────────────────────────────────────────────────
  $: if (isOpen) reload();

  async function reload() {
    loading = true;
    error = '';
    try {
      servers = await invoke('mcp_server_list');
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // ── Form helpers ────────────────────────────────────────────────────
  function openAdd() {
    editingOriginalName = '';
    form = freshForm();
    formOpen = true;
  }
  function openEdit(s) {
    editingOriginalName = s.name;
    form = {
      name: s.name,
      command: s.command,
      envKeys: [...(s.env_keys || [])],
      enabled: !!s.enabled,
    };
    formOpen = true;
  }
  function cancelForm() {
    formOpen = false;
    editingOriginalName = '';
  }
  function applyPreset(p) {
    form.name = p.name;
    form.command = p.command;
    form.envKeys = [...p.envKeys];
    form.enabled = true;
  }
  function toggleEnvKey(k) {
    const i = form.envKeys.indexOf(k);
    if (i >= 0) form.envKeys.splice(i, 1);
    else form.envKeys.push(k);
    form.envKeys = [...form.envKeys];
  }

  async function saveForm() {
    error = '';
    const name = (form.name || '').trim();
    const command = (form.command || '').trim();
    if (!name || !command) {
      error = $trad('Nombre y comando son obligatorios.');
      return;
    }
    try {
      await invoke('mcp_server_upsert', {
        name,
        command,
        envKeys: form.envKeys,
        enabled: form.enabled,
      });
      // If renamed, delete the old row.
      if (editingOriginalName && editingOriginalName !== name) {
        try { await invoke('mcp_server_delete', { name: editingOriginalName }); } catch {}
      }
      formOpen = false;
      editingOriginalName = '';
      await reload();
      dispatch('updated');
      flash($trad('Guardado'));
    } catch (e) {
      error = String(e);
    }
  }

  async function doDelete(name) {
    try {
      await invoke('mcp_server_delete', { name });
      await reload();
      dispatch('updated');
      flash($trad('Eliminado'));
    } catch (e) {
      error = String(e);
    } finally {
      confirmDelete = null;
    }
  }

  /**
   * v1.4.5 — Inline enable/disable toggle. Persists via mcp_server_upsert
   * (no dedicated set-enabled command exists; upsert is idempotent and
   * already evicts pooled sessions). Optimistic UI: flip the local row
   * first so the user gets instant feedback, roll back on failure.
   */
  async function doToggleEnabled(s) {
    busyName = s.name;
    const prev = s.enabled;
    s.enabled = !prev;
    servers = [...servers];  // trigger reactivity
    try {
      await invoke('mcp_server_upsert', {
        name: s.name, command: s.command,
        envKeys: s.env_keys || [], enabled: !prev,
      });
      dispatch('updated');
      flash(s.enabled
          ? (isEN ? `${s.name}: enabled` : `${s.name}: habilitado`)
          : (isEN ? `${s.name}: disabled (excluded from system prompt)` : `${s.name}: deshabilitado (excluido del system prompt)`));
    } catch (e) {
      // Rollback optimistic UI.
      s.enabled = prev;
      servers = [...servers];
      error = String(e);
    } finally {
      busyName = '';
    }
  }

  async function doTest(s) {
    // v1.4.15 — `toast.promise` shows a single toast that auto-transitions
    // loading→success/error. Replaces the silent busy spinner that left
    // users wondering whether the ping was still in flight.
    busyName = s.name;
    error = '';
    const p = invoke('mcp_server_test', { name: s.name, env: mcpSecrets });
    sonnerToast.promise(p, {
      loading: isEN ? `Pinging ${s.name}…` : `Probando ${s.name}…`,
      success: (r) => r.ok
          ? (isEN
              ? `${s.name} OK · ${r.tools_count} tools · ${r.latency_ms} ms`
              : `${s.name} OK · ${r.tools_count} tools · ${r.latency_ms} ms`)
          : (isEN ? `${s.name} failed` : `${s.name} falló`),
      error: (e) => `${s.name}: ${String(e)}`,
    });
    try {
      const r = await p;
      if (r.ok) flash(`${s.name}: ${r.tools_count} tools · ${r.latency_ms} ms`);
      else      error = `${s.name}: ${r.error || ($trad('falló'))}`;
    } catch (e) {
      error = String(e);
    } finally {
      busyName = '';
    }
  }

  async function doDiscover(s) {
    busyName = s.name;
    error = '';
    const p = invoke('mcp_server_discover', { name: s.name, env: mcpSecrets });
    sonnerToast.promise(p, {
      loading: isEN ? `Discovering tools on ${s.name}…` : `Descubriendo tools en ${s.name}…`,
      success: (updated) => {
        const count = Array.isArray(updated.tools_cache) ? updated.tools_cache.length : 0;
        return isEN
            ? `${s.name}: ${count} tools cached · ${updated.last_latency_ms} ms`
            : `${s.name}: ${count} tools cacheadas · ${updated.last_latency_ms} ms`;
      },
      error: (e) => `${s.name}: ${String(e)}`,
    });
    try {
      const updated = await p;
      // Splice updated row into the local list (avoid a full reload flicker).
      servers = servers.map(x => x.name === updated.name ? updated : x);
      expandedName = updated.name;
      dispatch('updated');
      const count = Array.isArray(updated.tools_cache) ? updated.tools_cache.length : 0;
      flash(`${s.name}: ${count} ${$trad('tools cacheadas')} · ${updated.last_latency_ms} ms`);
    } catch (e) {
      error = String(e);
    } finally {
      busyName = '';
    }
  }

  function openRunPanel(serverName, tool) {
    // Pre-fill args from the tool's inputSchema required fields, if any.
    let args = {};
    try {
      const req = (tool?.inputSchema?.required) || [];
      for (const k of req) args[k] = '';
    } catch {}
    runPanel = {
      serverName,
      toolName: tool.name,
      argsJson: JSON.stringify(args, null, 2),
      output: '',
      error: '',
      running: false,
    };
  }
  async function executeRunPanel() {
    if (!runPanel) return;
    runPanel.running = true;
    runPanel.output = '';
    runPanel.error = '';
    try {
      // Validate JSON early so we surface parse errors cleanly.
      JSON.parse(runPanel.argsJson || '{}');
      const out = await invoke('mcp_server_call', {
        name: runPanel.serverName,
        toolName: runPanel.toolName,
        argsJson: runPanel.argsJson || '{}',
        env: mcpSecrets,
      });
      runPanel.output = String(out || '');
    } catch (e) {
      runPanel.error = String(e);
    } finally {
      runPanel.running = false;
      runPanel = { ...runPanel };
    }
  }

  function fmtTs(ts) {
    if (!ts) return '—';
    const d = new Date(ts * 1000);
    return d.toLocaleString();
  }
  function statusLabel(s) {
    if (s.last_status === 'ok')    return T.status_ok;
    if (s.last_status === 'error') return T.status_err;
    return T.status_pend;
  }
  function statusClass(s) {
    if (s.last_status === 'ok')    return 'ok';
    if (s.last_status === 'error') return 'err';
    return 'pend';
  }
  function toolCount(s) {
    return Array.isArray(s.tools_cache) ? s.tools_cache.length : 0;
  }

  let _flashTimer = null;
  function flash(msg) {
    toast = msg;
    if (_flashTimer) clearTimeout(_flashTimer);
    _flashTimer = setTimeout(() => { toast = ''; }, 2400);
  }

  function close() { dispatch('close'); }

  // v1.4.13 — Dialog.Root drives the open state via bind:open. When
  // bits-ui closes the dialog (Escape, outside-click), we forward the
  // 'close' event so the parent component (+page.svelte) clears its
  // showMcpServersModal flag, matching the legacy behavior.
  function onOpenChange(v) {
    if (isOpen && !v) dispatch('close');
  }
  // onMount cleanup of the legacy document keydown listener is no
  // longer needed — Dialog primitive owns key routing.
</script>

<Dialog.Root open={isOpen} onOpenChange={onOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="modal-backdrop" />
    <Dialog.Content class="modal-card-wrap">
      <div class="modal-card">
      <header class="hdr">
        <div class="hdr-left">
          <span class="mcp-hdr-ico"><Plug size={20} stroke={1.8} color="var(--acc, #10b981)" /></span>
          <div>
            <h2>{T.title}</h2>
            <p class="sub">{T.subtitle}</p>
          </div>
        </div>
        <button class="x" on:click={close} aria-label={T.close}>✕</button>
      </header>

      {#if error}
        <div class="banner err">⚠ {error}</div>
      {/if}
      {#if toast}
        <div class="banner ok">{toast}</div>
      {/if}

      <!-- ── Server list ──────────────────────────────────────────── -->
      {#if !formOpen}
        <div class="toolbar">
          <button class="btn pri" on:click={openAdd}>{T.add}</button>
          <button class="btn ghost" on:click={reload} disabled={loading}>↻</button>
          <!-- v1.6.2 — MCP budget guard chip (ECC mcp-budget skill).
               Live counts of enabled servers / tools / token estimate
               with tone bands. Reduces the cognitive load of "am I
               about to blow my context window with one more server". -->
          <span class="mcp-budget-slot">
            <McpBudgetChip servers={servers} {isEN} />
          </span>
        </div>

        {#if loading}
          <!-- v1.4.15 — shimmer skeletons replace the bare ellipsis. -->
          <div class="srv-list">
            <Skeleton variant="card" height="76px" />
            <Skeleton variant="card" height="76px" />
            <Skeleton variant="card" height="76px" />
          </div>
        {:else if servers.length === 0}
          <!-- v1.4.15 — proper empty state with CTA via the action slot. -->
          <EmptyState
            icon="🔌"
            title={T.noServers}
            description={$trad('Registra tu primer servidor MCP para extender Lucy con herramientas externas (filesystem, github, postgres, brave-search…).')}>
            <button slot="action" class="btn pri" on:click={openAdd}>{T.add}</button>
          </EmptyState>
        {:else}
          <ul class="srv-list" use:autoAnimate>
            {#each servers as s (s.name)}
              <li class="srv">
                <div class="srv-head">
                  <div class="srv-name-block">
                    <!-- v1.4.5: inline enable/disable toggle. Lets the user
                         silence a noisy MCP catalog from the system prompt
                         (e.g. github with 26 tools when the current task is
                         pure Windows audit) without deleting the server. -->
                    <button class="srv-toggle" class:on={s.enabled}
                      title={s.enabled
                          ? ($trad('Habilitado — clic para deshabilitar'))
                          : ($trad('Deshabilitado — clic para habilitar'))}
                      on:click={() => doToggleEnabled(s)} disabled={busyName === s.name}>
                      <span class="srv-toggle-knob"></span>
                    </button>
                    <span class="srv-name">{s.name}</span>
                    <span class="pill {statusClass(s)}">{statusLabel(s)}</span>
                    {#if !s.enabled}
                      <span class="pill off">{$trad('deshabilitado')}</span>
                    {/if}
                    <span class="pill tools" title="{T.tools}">
                      {toolCount(s)} tools
                    </span>
                  </div>
                  <div class="srv-actions">
                    <button class="btn xs" on:click={() => doTest(s)} disabled={busyName === s.name}>
                      {busyName === s.name ? '…' : T.test}
                    </button>
                    <button class="btn xs" on:click={() => doDiscover(s)} disabled={busyName === s.name}>
                      {busyName === s.name ? '…' : T.discover}
                    </button>
                    <button class="btn xs ghost" on:click={() => openEdit(s)}>{T.edit}</button>
                    <button class="btn xs danger" on:click={() => confirmDelete = { name: s.name }}>{T.delete}</button>
                  </div>
                </div>
                <code class="cmd">{s.command}</code>
                <div class="meta">
                  {#if s.env_keys && s.env_keys.length > 0}
                    <span class="meta-row">env:&nbsp;
                      {#each s.env_keys as k}
                        <code class="env-chip {mcpSecrets[k] ? 'present' : 'missing'}" title={mcpSecrets[k] ? ($trad('presente en Keyring')) : ($trad('NO está en el Keyring'))}>{k}</code>
                      {/each}
                    </span>
                  {/if}
                  <span class="meta-row">{$trad('última discovery')}: {fmtTs(s.last_discovered)}{s.last_latency_ms ? ` · ${s.last_latency_ms} ms` : ''}</span>
                  {#if s.last_error}
                    <span class="meta-row err">↳ {s.last_error}</span>
                  {/if}
                </div>

                <!-- Expandable tools list -->
                <button class="expand" on:click={() => expandedName = expandedName === s.name ? '' : s.name}>
                  {expandedName === s.name ? '▾' : '▸'} {T.tools} ({toolCount(s)})
                </button>
                {#if expandedName === s.name}
                  {#if toolCount(s) === 0}
                    <p class="hint">{T.noTools}</p>
                  {:else}
                    <ul class="tool-list">
                      {#each s.tools_cache as t}
                        <li class="tool">
                          <div class="tool-row">
                            <code>{t.name}</code>
                            <button class="btn xxs" on:click={() => openRunPanel(s.name, t)}>{T.runTool}</button>
                          </div>
                          {#if t.description}
                            <p class="tool-desc">{t.description}</p>
                          {/if}
                        </li>
                      {/each}
                    </ul>
                  {/if}
                {/if}
              </li>
            {/each}
          </ul>
        {/if}

      {:else}
        <!-- ── Add / Edit form ──────────────────────────────────── -->
        <div class="form">
          <h3>{editingOriginalName ? T.edit : T.add}</h3>

          <label class="field">
            <span>{T.name}</span>
            <input type="text" bind:value={form.name} placeholder={T.namePh} />
          </label>

          <label class="field">
            <span>{T.command}</span>
            <input type="text" bind:value={form.command} placeholder={T.cmdPh} spellcheck="false" />
            <small class="hint">{$trad('Comando completo. Lucy añade "-y" automáticamente a npx.')}</small>
          </label>

          <div class="field">
            <span>{T.envKeys}</span>
            {#if secretKeys.length === 0}
              <p class="hint warn">
                {$trad('Aún no hay secretos MCP en el Keyring. Añádelos primero en la sección "Variables / API Keys para MCP" de Configuración.')}
              </p>
            {:else}
              <div class="env-grid">
                {#each secretKeys as k}
                  <label class="env-pick">
                    <input
                      type="checkbox"
                      checked={form.envKeys.includes(k)}
                      on:change={() => toggleEnvKey(k)}
                    />
                    <code>{k}</code>
                  </label>
                {/each}
              </div>
            {/if}
          </div>

          <label class="field checkbox">
            <input type="checkbox" bind:checked={form.enabled} />
            <span>{T.enabled}</span>
          </label>

          {#if !editingOriginalName}
            <div class="field">
              <span>{T.presets}</span>
              <div class="presets">
                {#each PRESETS as p}
                  <button type="button" class="preset" on:click={() => applyPreset(p)} title={p.hint}>
                    {p.name}
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          <div class="form-actions">
            <button class="btn ghost" on:click={cancelForm}>{T.cancel}</button>
            <button class="btn pri" on:click={saveForm}>{T.save}</button>
          </div>
        </div>
      {/if}

      <!-- ── Run-tool panel ───────────────────────────────────────── -->
      {#if runPanel}
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div class="run-panel-bg" on:click|self={() => runPanel = null}>
          <div class="run-panel">
            <header>
              <h3>{T.runPanel}: <code>{runPanel.serverName}</code> · <code>{runPanel.toolName}</code></h3>
              <button class="x" on:click={() => runPanel = null}>✕</button>
            </header>
            <label class="field">
              <span>{T.runArgs}</span>
              <textarea bind:value={runPanel.argsJson} rows="6" spellcheck="false"></textarea>
            </label>
            {#if runPanel.error}
              <pre class="run-err">{runPanel.error}</pre>
            {/if}
            {#if runPanel.output}
              <pre class="run-out">{runPanel.output}</pre>
            {/if}
            <div class="form-actions">
              <button class="btn ghost" on:click={() => runPanel = null}>{T.close}</button>
              <button class="btn pri" on:click={executeRunPanel} disabled={runPanel.running}>
                {runPanel.running ? '…' : T.runTool}
              </button>
            </div>
          </div>
        </div>
      {/if}
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<!-- Confirm delete — sits outside the parent Dialog so it can layer
     above it without being torn down on close. -->
<ConfirmModal
  open={!!confirmDelete}
  title={T.delete}
  message={T.confirmDel}
  detail={confirmDelete ? confirmDelete.name : ''}
  confirmLabel={T.delete}
  cancelLabel={T.cancel}
  variant="danger"
  on:confirm={() => confirmDelete && doDelete(confirmDelete.name)}
  on:cancel={() => confirmDelete = null}
/>

<style>
  /* v1.4.13 — bits-ui portals the overlay + content outside this scope,
     so all positioning rules are :global. Visual identity unchanged. */
  :global(.modal-backdrop) {
    position: fixed; inset: 0;
    background: rgba(2, 6, 12, 0.78);
    backdrop-filter: blur(6px) saturate(120%);
    z-index: 5000;
    animation: -global-mcp-bg-in .18s ease;
  }
  @keyframes -global-mcp-bg-in { from { opacity: 0; } to { opacity: 1; } }
  /* Dialog.Content wraps the actual card and provides the focus trap.
     We use it as the centering flex parent so the card stays middle-
     aligned even when its content is tall (overflow on the inner card). */
  :global(.modal-card-wrap) {
    position: fixed; inset: 0;
    z-index: 5001;
    display: flex; align-items: center; justify-content: center;
    padding: 24px;
    /* Pointer events pass through the wrap edges so outside-click
       (handled by the overlay below) still dismisses. */
    pointer-events: none;
  }
  :global(.modal-card-wrap > .modal-card) { pointer-events: auto; }
  :global(.modal-card) {
    /* Depth: faint accent top glow over the base instead of a flat slab. */
    background:
      radial-gradient(120% 60% at 50% -6%, rgba(16,185,129,.08) 0%, transparent 56%),
      var(--bg2, #0b0e14);
    border: 1px solid var(--bdr, #1a2030);
    border-top: 3px solid var(--acc, #10b981);
    border-radius: 14px;
    width: min(820px, 96vw);
    max-height: 92vh;
    overflow-y: auto;
    box-shadow:
      0 24px 64px rgba(0,0,0,0.7),
      0 -1px 28px -10px rgba(16,185,129,.38);
    padding: 22px 24px;
    color: var(--txt, #dde3ea);
    animation: -global-mcp-card-in .26s cubic-bezier(.16,1,.3,1);
  }
  @keyframes -global-mcp-card-in { from { opacity: 0; transform: translateY(14px) scale(.985); } to { opacity: 1; transform: none; } }
  .hdr { display: flex; justify-content: space-between; gap: 16px; align-items: flex-start; margin-bottom: 14px; }
  .hdr-left { display: flex; align-items: flex-start; gap: 13px; }
  .mcp-hdr-ico {
    display: inline-flex; align-items: center; justify-content: center;
    width: 40px; height: 40px; border-radius: 12px; flex-shrink: 0;
    background: rgba(16,185,129,.12);
    border: 1px solid rgba(16,185,129,.28);
    box-shadow: 0 0 18px -4px rgba(16,185,129,.5);
  }
  .hdr h2 { margin: 0 0 4px; font-size: 18px; }
  .sub { color: var(--txt2, #7a8a9a); font-size: 12px; margin: 0; line-height: 1.5; }
  .x {
    background: transparent; border: none; color: var(--txt2, #7a8a9a);
    cursor: pointer; font-size: 16px; padding: 4px 8px; border-radius: 6px;
  }
  .x:hover { background: rgba(255,255,255,.06); color: var(--txt, #dde3ea); }

  .banner {
    border-radius: 8px; padding: 8px 12px; margin: 6px 0 10px;
    font-size: 12px; line-height: 1.5;
  }
  .banner.err { background: rgba(239,68,68,.08); border: 1px solid rgba(239,68,68,.30); color: #ff6b6b; }
  .banner.ok  { background: rgba(16,185,129,.08); border: 1px solid rgba(16,185,129,.30); color: var(--acc, #10b981); }

  .toolbar { display: flex; gap: 8px; margin-bottom: 12px; align-items: center; }
  .mcp-budget-slot { margin-left: auto; }
  .empty { color: var(--txt3, #475569); padding: 24px 0; text-align: center; font-size: 13px; }

  /* ── Server list ────────────────────────────────────────────────── */
  .srv-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 10px; }
  .srv {
    background: rgba(255,255,255,.02);
    border: 1px solid var(--bdr, #1a2030);
    border-radius: 10px; padding: 12px 14px;
  }
  .srv-head { display: flex; justify-content: space-between; gap: 10px; flex-wrap: wrap; margin-bottom: 6px; }
  .srv-name-block { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  /* v1.4.5 — Inline enabled/disabled toggle switch. iOS-style for instant
     recognizability; off state is muted grey, on state is accent green. */
  .srv-toggle {
    position: relative;
    width: 28px; height: 16px;
    background: rgba(255,255,255,0.10);
    border: 1px solid var(--bdr2, #222c3a);
    border-radius: 9px;
    cursor: pointer;
    padding: 0;
    flex-shrink: 0;
    transition: background .18s, border-color .18s;
  }
  .srv-toggle:hover:not(:disabled) { border-color: var(--bdr, #1a2030); }
  .srv-toggle:disabled { opacity: .5; cursor: not-allowed; }
  .srv-toggle.on {
    background: rgba(16, 185, 129, 0.45);
    border-color: rgba(16, 185, 129, 0.65);
  }
  .srv-toggle-knob {
    position: absolute;
    top: 1px; left: 1px;
    width: 12px; height: 12px;
    background: #f1f5f9;
    border-radius: 50%;
    transition: transform .18s cubic-bezier(.34, 1.4, .64, 1);
    box-shadow: 0 1px 2px rgba(0,0,0,.4);
  }
  .srv-toggle.on .srv-toggle-knob {
    transform: translateX(12px);
    background: #fff;
  }
  .srv-name { font-weight: 700; font-size: 14px; }
  .srv-actions { display: flex; gap: 6px; flex-wrap: wrap; }

  .pill {
    font-size: 10px; font-weight: 700; padding: 2px 7px; border-radius: 10px;
    text-transform: uppercase; letter-spacing: .4px;
  }
  .pill.ok    { background: rgba(16,185,129,.12); color: var(--acc, #10b981); }
  .pill.err   { background: rgba(239,68,68,.12);  color: #ff6b6b; }
  .pill.pend  { background: rgba(148,163,184,.12); color: #94a3b8; }
  .pill.off   { background: rgba(245,158,11,.12); color: #f59e0b; }
  .pill.tools { background: rgba(59,158,255,.10); color: #3b9eff; }

  .cmd {
    display: block; background: rgba(0,0,0,.30); border: 1px solid var(--bdr2, #222c3a);
    border-radius: 6px; padding: 6px 10px; font-size: 11.5px; color: var(--txt, #dde3ea);
    overflow-x: auto; white-space: nowrap;
  }
  .meta { display: flex; flex-direction: column; gap: 2px; margin-top: 6px; font-size: 11px; color: var(--txt2, #7a8a9a); }
  .meta-row.err { color: #ff6b6b; }
  .env-chip {
    background: rgba(59,158,255,.10); color: #3b9eff; border-radius: 4px;
    padding: 1px 5px; font-size: 10.5px; margin: 0 4px 0 0;
  }
  .env-chip.missing { background: rgba(239,68,68,.12); color: #ff6b6b; }
  .env-chip.present { background: rgba(16,185,129,.12); color: var(--acc, #10b981); }

  .expand {
    background: transparent; border: none; color: var(--txt2, #7a8a9a);
    cursor: pointer; font-size: 11px; padding: 4px 0; margin-top: 6px;
    font-family: inherit;
  }
  .expand:hover { color: var(--acc, #10b981); }

  .tool-list { list-style: none; padding: 0 0 0 14px; margin: 4px 0 0; display: flex; flex-direction: column; gap: 6px; }
  .tool { background: rgba(0,0,0,.20); border-radius: 6px; padding: 6px 10px; }
  .tool-row { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
  .tool-desc { margin: 4px 0 0; font-size: 11px; color: var(--txt2, #7a8a9a); line-height: 1.4; }

  /* ── Form ───────────────────────────────────────────────────────── */
  .form { display: flex; flex-direction: column; gap: 12px; }
  .form h3 { margin: 0; font-size: 14px; color: var(--acc, #10b981); }
  .field { display: flex; flex-direction: column; gap: 4px; font-size: 12px; }
  .field > span { color: var(--txt2, #7a8a9a); font-weight: 600; }
  .field input[type="text"], .field textarea {
    background: rgba(0,0,0,.30); border: 1px solid var(--bdr2, #222c3a);
    border-radius: 7px; color: var(--txt, #dde3ea);
    font-size: 12px; padding: 8px 10px; font-family: inherit; outline: none;
  }
  .field textarea { font-family: var(--mono, monospace); resize: vertical; }
  .field input:focus, .field textarea:focus { border-color: var(--acc, #10b981); }
  .field.checkbox { flex-direction: row; align-items: center; gap: 6px; }
  .field.checkbox span { font-weight: 500; }
  .hint { font-size: 10.5px; color: var(--txt3, #475569); margin: 2px 0 0; }
  .hint.warn { color: #f59e0b; }
  .env-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 4px; }
  .env-pick {
    display: flex; align-items: center; gap: 6px; padding: 4px 6px;
    background: rgba(0,0,0,.18); border-radius: 5px; font-size: 11px;
    cursor: pointer;
  }
  .env-pick:hover { background: rgba(255,255,255,.04); }

  .presets { display: flex; flex-wrap: wrap; gap: 6px; }
  .preset {
    background: rgba(59,158,255,.08); border: 1px solid rgba(59,158,255,.25);
    color: #3b9eff; padding: 5px 10px; border-radius: 6px;
    font-size: 11.5px; font-family: inherit; cursor: pointer;
    transition: .15s;
  }
  .preset:hover { background: rgba(59,158,255,.15); border-color: rgba(59,158,255,.5); }

  .form-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 6px; }

  .btn {
    background: rgba(255,255,255,.05); border: 1px solid var(--bdr2, #222c3a);
    color: var(--txt, #dde3ea); padding: 6px 12px; border-radius: 6px;
    font-size: 11.5px; font-family: inherit; cursor: pointer; transition: .15s;
  }
  .btn:hover:not(:disabled) { background: rgba(255,255,255,.08); border-color: var(--bdr, #1a2030); }
  .btn.pri { background: var(--acc, #10b981); border-color: var(--acc, #10b981); color: #030b06; font-weight: 700; }
  .btn.pri:hover:not(:disabled) { opacity: .88; }
  .btn.ghost { background: transparent; }
  .btn.danger { color: #ff6b6b; border-color: rgba(239,68,68,.30); }
  .btn.danger:hover { background: rgba(239,68,68,.10); }
  .btn.xs { font-size: 10.5px; padding: 4px 8px; }
  .btn.xxs { font-size: 10px; padding: 3px 7px; }
  .btn:disabled { opacity: .5; cursor: not-allowed; }

  /* Light mode (v1.7.232) — translucent-white fills vanish on the white modal;
     give the server cards a real plate. .btn keeps its token border (visible on
     light), so its variant backgrounds (.pri/.danger) stay intact. */
  :global(:root.light) .srv { background: var(--bg4); border-color: var(--bdr); box-shadow: var(--shadow-card); }
  :global(:root.light) .x:hover,
  :global(:root.light) .env-pick:hover { background: rgba(15,23,42,.05); }

  /* ── Run-tool overlay ───────────────────────────────────────────── */
  .run-panel-bg {
    position: fixed; inset: 0; z-index: 5100;
    background: rgba(2,6,12,0.7);
    display: flex; align-items: center; justify-content: center; padding: 24px;
  }
  .run-panel {
    background: var(--bg2, #0b0e14); border: 1px solid var(--bdr, #1a2030);
    border-radius: 12px; padding: 20px 22px;
    width: min(680px, 96vw); max-height: 88vh; overflow-y: auto;
    display: flex; flex-direction: column; gap: 10px;
  }
  .run-panel header { display: flex; justify-content: space-between; align-items: center; }
  .run-panel header h3 { margin: 0; font-size: 13px; color: var(--txt, #dde3ea); }
  .run-out, .run-err {
    background: rgba(0,0,0,.30); border-radius: 6px; padding: 10px 12px;
    font-size: 11.5px; font-family: var(--mono, monospace);
    max-height: 280px; overflow: auto; white-space: pre-wrap;
  }
  .run-err { color: #ff6b6b; border: 1px solid rgba(239,68,68,.30); }
  .run-out { color: var(--txt, #dde3ea); border: 1px solid var(--bdr2, #222c3a); }
</style>
