<!-- ── HostModal.svelte ──────────────────────────────────────────────────────
     Modal para agregar o editar hosts remotos.
     Props  : show (bindable), editingHost (Host | null)
     Events : saved  { hostObj, isEdit } — el padre actualiza el array de hosts
              error  string              — para mostrar un toast de error
              cancel
──────────────────────────────────────────────────────────────────────────── -->
<script>
  // La interfaz en cinco idiomas. Ver `$lib/i18n`.
  import { trad } from '$lib/i18n';
    import { createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { focusTrap } from '$lib/actions';
    import ConfirmModal from '$lib/ConfirmModal.svelte';
    import Pencil from '@tabler/icons-svelte/icons/pencil';

    import Plus from '@tabler/icons-svelte/icons/plus';

    import X from '@tabler/icons-svelte/icons/x';

    import Tag from '@tabler/icons-svelte/icons/tag';

    import Palette from '@tabler/icons-svelte/icons/palette';

    import Key from '@tabler/icons-svelte/icons/key';

    import FolderOpen from '@tabler/icons-svelte/icons/folder-open';

    import Info from '@tabler/icons-svelte/icons/info-circle';

    import Trash2 from '@tabler/icons-svelte/icons/trash';
    /** Controls visibility; supports bind:show */
    export let show        = false;
    /** Host being edited, or null for a new host */
    // Without the annotation TS infers the type as literally `null` from the
    // initializer and flags every field read as "possibly null".
    /** @type {import('$lib/stores').Host | null} */
    export let editingHost = null;
    export let isEN = false;

    const dispatch = createEventDispatcher();

    let hostForm     = { name:'', type:'windows', protocol:'winrm', host:'', username:'', port:5985, sshKeyPath:'', tags:'', color:'#10b981', category:'shell', dbType:'postgres' };
    let hostPassword = '';
    let hostSaving   = false;

    // Re-initialize the form whenever the modal opens
    $: if (show) resetForm(editingHost);

    function resetForm(host) {
        hostPassword = '';
        hostSaving   = false;
        hostForm = host
            ? {
                name:      host.name,
                type:      host.type,
                protocol:  host.protocol || (host.type === 'linux' ? 'ssh' : 'winrm'),
                host:      host.host,
                username:  host.username,
                port:      host.port || defaultPort(host.protocol || (host.type === 'linux' ? 'ssh' : 'winrm')),
                sshKeyPath:host.sshKeyPath || '',
                tags:      (host.tags || []).join(', '),
                color:     host.color || '#10b981',
                category:  host.category || 'shell',
                dbType:    host.dbType || 'postgres',
              }
            : { name:'', type:'windows', protocol:'winrm', host:'', username:'', port:5985, sshKeyPath:'', tags:'', color:'#10b981', category:'shell', dbType:'postgres' };
    }

    // ── Protocol definitions ────────────────────────────────────────────────
    const PROTOCOLS = [
        { value:'winrm',    label:'🖥 Windows (WinRM)',        port:5985, os:'windows' },
        { value:'ssh',      label:'🐧 Linux (SSH)',            port:22,   os:'linux' },
        { value:'rdp',      label:'🖥 Windows (RDP)',          port:3389, os:'windows' },
        { value:'snmp',     label:'🌐 SNMP (Red)',             port:161,  os:'linux' },
        { value:'docker',   label:'🐳 Docker API (TCP)',       port:2375, os:'linux' },
        { value:'k8s',      label:'⎈ Kubernetes API',          port:6443, os:'linux' },
        { value:'postgres', label:'🐘 PostgreSQL',             port:5432, os:'linux' },
        { value:'mysql',    label:'🐬 MySQL / MariaDB',        port:3306, os:'linux' },
        { value:'mongodb',  label:'🍃 MongoDB',                port:27017,os:'linux' },
        { value:'redis',    label:'⚡ Redis',                  port:6379, os:'linux' },
        { value:'mssql',    label:'🪟 SQL Server (MSSQL)',     port:1433, os:'windows' },
    ];

    function defaultPort(proto) {
        return PROTOCOLS.find(p => p.value === proto)?.port || 22;
    }

    function onProtocolChange(proto) {
        const def = PROTOCOLS.find(p => p.value === proto);
        if (def) {
            hostForm.type = def.os;
            hostForm.port = def.port;
            // Auto-set category based on protocol
            if (['postgres','mysql','mongodb','redis','mssql'].includes(proto)) {
                hostForm.category = 'database';
                hostForm.dbType   = proto === 'mssql' ? 'mssql' : proto;
            } else if (proto === 'docker') {
                hostForm.category = 'container';
            } else if (proto === 'k8s') {
                hostForm.category = 'kubernetes';
            } else if (proto === 'snmp') {
                hostForm.category = 'network';
            } else {
                hostForm.category = 'shell';
            }
        }
    }

    // Reactive: filter protocols by category for a cleaner UX
    $: protocolHint = PROTOCOLS.find(p => p.value === hostForm.protocol);

    // ── Test connection state ──────────────────────────────────────────────
    // Lets the user verify the host is reachable BEFORE saving — avoids the
    // "saved a typo, fail at first use" frustration that was the #1 NexShell
    // pain point reported in the May 2026 audit.
    let testResult = null;    // { ok: bool, message: string, elapsedMs: number } | null
    let testRunning = false;

    /** Validates inputs, then runs a tiny `echo` command against the host
     *  to confirm credentials + network reach. Idempotent — the user can
     *  run it multiple times; each run replaces the previous result.
     */
    async function testConnection() {
        // Pre-validate same fields as guardarHost — surface friendly errors
        // up front instead of failing inside the Rust execute_shell_cmd call.
        if (!hostForm.host.trim())     { testResult = { ok: false, message: 'Falta host/IP', elapsedMs: 0 }; return; }
        if (!hostForm.username.trim()) { testResult = { ok: false, message: 'Falta usuario', elapsedMs: 0 }; return; }
        // Determine which protocols we can actually preflight via execute_shell_cmd.
        // Database/HTTP protocols don't have an SSH-style echo, so we skip them.
        const supported = ['ssh', 'winrm'];
        if (!supported.includes(hostForm.protocol)) {
            testResult = { ok: false, message: `Test no disponible para protocolo "${hostForm.protocol}" — sólo SSH y WinRM por ahora.`, elapsedMs: 0 };
            return;
        }
        // For SSH, password OR key path must be provided
        if (hostForm.protocol === 'ssh' && !hostPassword.trim() && !hostForm.sshKeyPath.trim()) {
            testResult = { ok: false, message: 'SSH requiere contraseña o ruta a clave privada', elapsedMs: 0 };
            return;
        }
        // For WinRM, password is required
        if (hostForm.protocol === 'winrm' && !hostPassword.trim()) {
            testResult = { ok: false, message: 'WinRM requiere contraseña', elapsedMs: 0 };
            return;
        }

        testRunning = true;
        testResult = null;
        const hostType = hostForm.type;
        const port = hostForm.port || defaultPort(hostForm.protocol);
        const testCmd = hostType === 'linux'
            ? 'echo "Lucy:OK" && uname -a 2>&1 | head -1'
            : 'echo Lucy:OK';
        const t0 = Date.now();
        try {
            const out = await invoke('execute_shell_cmd', {
                host:     hostForm.host.trim(),
                username: hostForm.username.trim(),
                command:  testCmd,
                hostType,
                port,
                password: hostPassword.trim() || null,
                keyPath:  hostForm.sshKeyPath.trim() || null,
            });
            const elapsed = Date.now() - t0;
            const safeOut = String(out || '').trim();
            if (safeOut.includes('Lucy:OK')) {
                // Take the most informative line — usually uname -a output on Linux,
                // empty echo on Windows. Truncate to keep the UI compact.
                const detail = safeOut.replace(/Lucy:OK\s*/g, '').trim().slice(0, 120);
                testResult = {
                    ok: true,
                    message: detail ? `Conectado: ${detail}` : 'Conexión exitosa',
                    elapsedMs: elapsed,
                };
            } else {
                testResult = {
                    ok: false,
                    message: `Respuesta inesperada: ${safeOut.slice(0, 200)}`,
                    elapsedMs: elapsed,
                };
            }
        } catch (e) {
            testResult = {
                ok: false,
                message: String(e).slice(0, 240),
                elapsedMs: Date.now() - t0,
            };
        } finally {
            testRunning = false;
        }
    }

    async function guardarHost() {
        if (!hostForm.name.trim() || !hostForm.host.trim() || !hostForm.username.trim()) return;
        hostSaving = true;
        try {
            const id      = editingHost ? editingHost.id : `h_${Date.now()}`;
            const hostObj = {
                id,
                name:      hostForm.name.trim(),
                type:      hostForm.type,
                protocol:  hostForm.protocol || 'winrm',
                host:      hostForm.host.trim(),
                username:  hostForm.username.trim(),
                port:      hostForm.port || defaultPort(hostForm.protocol),
                sshKeyPath:hostForm.sshKeyPath.trim(),
                tags:      (hostForm.tags || '').split(',').map(t => t.trim()).filter(Boolean),
                color:     hostForm.color || '#10b981',
                category:  hostForm.category || 'shell',
                dbType:    hostForm.category === 'database' ? (hostForm.dbType || 'postgres') : undefined,
            };
            if (hostPassword.trim()) {
                await invoke('save_host_credential', { hostId: id, password: hostPassword.trim() });
            }
            dispatch('saved', { hostObj, isEdit: !!editingHost });
            show = false;
        } catch(e) {
            dispatch('error', String(e));
        } finally {
            hostSaving = false;
        }
    }

    function cancel() {
        show = false;
        dispatch('cancel');
    }

    let confirmDelete = false;
    function eliminar() { confirmDelete = true; }
    function doDelete() {
        confirmDelete = false;
        dispatch('delete', editingHost.id);
        show = false;
    }
</script>

{#if show}
<div class="hm-bg">
  <div class="hm-box" use:focusTrap style="--host-color:{hostForm.color};">

    <!-- Header -->
    <div class="hm-hdr">
      <span class="hm-hdr-ico">{#if editingHost}<Pencil size={16}/>{:else}<Plus size={16}/>{/if}</span>
      <div class="hm-hdr-text">
        <h2 class="hm-title">{editingHost ? ($trad('Editar Host')) : ($trad('Nuevo Host Remoto'))}</h2>
        <span class="hm-subtitle">{(protocolHint?.label || hostForm.protocol)} · {hostForm.host || ($trad('sin dirección aún'))}</span>
      </div>
      <button class="hm-close" on:click={cancel}><X size={15}/></button>
    </div>

    <!-- Form grid -->
    <div class="hm-grid">
      <div>
        <label class="hm-label" for="hf-name">{$trad('Nombre *')}</label>
        <input id="hf-name" class="hm-inp" placeholder={$trad('Ej. Prod-Web-01')}
          bind:value={hostForm.name}>
      </div>
      <div>
        <label class="hm-label" for="hf-proto">{$trad('Protocolo *')}</label>
        <select id="hf-proto" class="hm-inp" bind:value={hostForm.protocol}
          on:change={() => onProtocolChange(hostForm.protocol)}>
          <optgroup label="Shell / {$trad('Acceso remoto')}">
            <option value="winrm">🖥 Windows (WinRM)</option>
            <option value="ssh">🐧 Linux (SSH)</option>
            <option value="rdp">🖥 Windows (RDP)</option>
          </optgroup>
          <optgroup label="{$trad('Bases de datos')}">
            <option value="postgres">🐘 PostgreSQL</option>
            <option value="mysql">🐬 MySQL / MariaDB</option>
            <option value="mongodb">🍃 MongoDB</option>
            <option value="redis">⚡ Redis</option>
            <option value="mssql">🪟 SQL Server</option>
          </optgroup>
          <optgroup label="{$trad('Infraestructura')}">
            <option value="docker">🐳 Docker API</option>
            <option value="k8s">⎈ Kubernetes API</option>
            <option value="snmp">🌐 SNMP (Red)</option>
          </optgroup>
        </select>
      </div>
      <div>
        <label class="hm-label" for="hf-category">{$trad('Categoría')}</label>
        <select id="hf-category" class="hm-inp" bind:value={hostForm.category}>
          <option value="shell">🖥 {$trad('Servidor / Shell')}</option>
          <option value="database">🗄️ {$trad('Base de datos')}</option>
          <option value="container">🐳 {$trad('Contenedor (Docker)')}</option>
          <option value="kubernetes">⎈ Kubernetes</option>
          <option value="network">🌐 {$trad('Dispositivo de red')}</option>
        </select>
      </div>
      {#if hostForm.category === 'database' && !['postgres','mysql','mongodb','redis','mssql'].includes(hostForm.protocol)}
      <div class="hm-full">
        <label class="hm-label" for="hf-dbtype">{$trad('Motor de base de datos')}</label>
        <select id="hf-dbtype" class="hm-inp" bind:value={hostForm.dbType}>
          <option value="postgres">🐘 PostgreSQL</option>
          <option value="mysql">🐬 MySQL / MariaDB</option>
          <option value="mongodb">🍃 MongoDB</option>
          <option value="redis">⚡ Redis</option>
          <option value="mssql">🪟 SQL Server (MSSQL)</option>
        </select>
      </div>
      {/if}
      <div>
        <label class="hm-label" for="hf-host">IP / Hostname *</label>
        <input id="hf-host" class="hm-inp hm-mono"
          placeholder={$trad('192.168.1.10 ó servidor.empresa.com')}
          bind:value={hostForm.host}>
      </div>
      <div>
        <label class="hm-label" for="hf-port">
          Puerto ({protocolHint?.label?.split('(')[0]?.trim() || hostForm.protocol.toUpperCase()})
        </label>
        <input class="hm-inp hm-mono" type="number" id="hf-port"
          placeholder={String(defaultPort(hostForm.protocol))}
          bind:value={hostForm.port}>
      </div>
      <div>
        <label class="hm-label" for="hf-user">{$trad('Usuario ')}{hostForm.protocol === 'snmp' ? '(community)' : '*'}</label>
        <input id="hf-user" class="hm-inp hm-mono"
          placeholder={hostForm.protocol === 'snmp' ? 'public' : hostForm.type === 'windows' ? 'DOMINIO/usuario' : 'root'}
          bind:value={hostForm.username}>
      </div>
      <div>
        <label class="hm-label" for="hf-pass">
          {$trad('Contraseña ')} {editingHost ? ($trad('(dejar vacío = no cambiar)')) : '*'}
        </label>
        <input class="hm-inp" type="password" id="hf-pass"
          placeholder="••••••••" bind:value={hostPassword}>
      </div>
      <div class="hm-full">
        <label class="hm-label" for="hf-tags" style="display:flex;align-items:center;gap:5px;">
          <Tag size={12}/> Tags <span class="hm-sub">({$trad('separados por coma — ej: prod, web, db')})</span>
        </label>
        <input id="hf-tags" class="hm-inp hm-mono"
          placeholder="prod, web, linux" bind:value={hostForm.tags}>
      </div>
      <div class="hm-full">
        <span class="hm-label" style="display:flex;align-items:center;gap:5px;"><Palette size={12}/> {$trad('Color del host')}</span>
        <div class="hm-swatches">
          {#each ['#10b981','#ef4444','#3b82f6','#f59e0b','#a78bfa','#ff6eb4','#00d4ff','#ff8c00'] as c}
          <button class="hm-swatch" class:active={hostForm.color === c}
            style="background:{c};box-shadow:{hostForm.color === c
              ? `0 0 0 2px var(--bg2,#0b0e14),0 0 0 4px ${c},0 0 12px -2px ${c}`
              : 'none'};"
            on:click={() => hostForm.color = c} title={c} aria-label={c}>
            {#if hostForm.color === c}<span class="hm-swatch-check">✓</span>{/if}
          </button>
          {/each}
        </div>
      </div>
    </div>

    <!-- SSH key path (SSH protocol only) -->
    {#if hostForm.protocol === 'ssh'}
    <div class="hm-keypath">
      <label class="hm-label" for="hf-keypath" style="display:flex;align-items:center;gap:5px;">
        <Key size={12}/> {$trad('Ruta de clave SSH privada')}
        <span class="hm-sub">({$trad('opcional — deja vacío para usar contraseña')})</span>
      </label>
      <div class="hm-keypath-row">
        <input id="hf-keypath" class="hm-inp hm-mono" style="flex:1;"
          placeholder="C:\Users\tu\.ssh\id_rsa o ~/.ssh/id_ed25519"
          bind:value={hostForm.sshKeyPath}>
        <button class="hm-pick" title="{$trad('Seleccionar archivo de clave')}"
          on:click={async () => {
            const p = await invoke('pick_file_path').catch(() => '');
            if (p) hostForm.sshKeyPath = p;
          }}><FolderOpen size={14}/></button>
      </div>
      <p class="hm-note">
        {$trad('Si se especifica, se usa <code>ssh -i &lt;ruta&gt;</code> en lugar de contraseña.')}
      </p>
    </div>
    {/if}

    <!-- Protocol-specific info boxes -->
    {#if hostForm.protocol === 'ssh'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>{$trad('Requisito SSH:')}</b> {$trad('El equipo local debe tener OpenSSH instalado (incluido en Windows 10/11) y el host remoto debe permitir autenticación por contraseña o clave SSH.')}
    </div>
    {:else if hostForm.protocol === 'winrm'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>{$trad('Requisito WinRM:')}</b> {$trad('El servidor remoto debe tener WinRM habilitado. Ejecuta en el servidor:')}
      <code class="hm-mono" style="font-size:10px;color:var(--acc);">Enable-PSRemoting -Force</code>
    </div>
    {:else if hostForm.protocol === 'rdp'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>RDP:</b> {$trad('Lucy lanzará una sesión de Escritorio Remoto')}
      (<code class="hm-mono" style="font-size:10px;">mstsc.exe</code>) {$trad('al conectar. Asegúrate de que el puerto 3389 esté accesible y el acceso remoto habilitado en el servidor.')}
    </div>
    {:else if hostForm.protocol === 'docker'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>Docker API:</b> {$trad('Requiere que el daemon de Docker exponga el API TCP. Configura en')} <code class="hm-mono" style="font-size:10px;">/etc/docker/daemon.json</code>:
      <code class="hm-mono" style="font-size:10px;color:var(--acc);">"hosts": ["tcp://0.0.0.0:2375"]</code>.
      {$trad('Usa TLS (2376) en producción.')}
    </div>
    {:else if hostForm.protocol === 'k8s'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>Kubernetes:</b> {$trad('Lucy ejecutará comandos')}
      <code class="hm-mono" style="font-size:10px;">kubectl</code> {$trad('contra el API server. Asegúrate de tener un')} <code class="hm-mono" style="font-size:10px;">kubeconfig</code> {$trad('válido o un token de servicio.')}
    </div>
    {:else if hostForm.protocol === 'snmp'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>SNMP:</b> {$trad('Lucy realizará consultas SNMP (GET/WALK) al dispositivo. El campo "Usuario" se usa como')} <b>community string</b> (v2c) {$trad('o usuario SNMPv3. Puerto estándar: 161.')}
    </div>
    {:else if ['postgres','mysql','mongodb','redis','mssql'].includes(hostForm.protocol)}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>{$trad('Base de datos')}:</b> {$trad('Lucy se conectará al motor')}
      <b>{PROTOCOLS.find(p => p.value === hostForm.protocol)?.label.split(' ')[1] || hostForm.protocol}</b>
      {isEN ? `engine on port ${defaultPort(hostForm.protocol)}. Ensure the server accepts remote connections and the user has required permissions.` : `en el puerto ${defaultPort(hostForm.protocol)}. Asegúrate de que el servidor acepte conexiones remotas y que el usuario tenga los permisos necesarios.`}
    </div>
    {/if}

    <!-- Test connection result (May 2026) — visible only after first attempt -->
    {#if testResult}
      <div style="margin:0 14px 8px;padding:8px 10px;border-radius:6px;font-size:11px;line-height:1.4;
                  background:{testResult.ok ? 'rgba(52,211,153,0.08)' : 'rgba(248,113,113,0.08)'};
                  border:1px solid {testResult.ok ? 'rgba(52,211,153,0.25)' : 'rgba(248,113,113,0.25)'};
                  color:{testResult.ok ? '#10b981' : '#f87171'};">
        <div style="display:flex;align-items:center;gap:6px;">
          <b style="font-family:var(--mono);">{testResult.ok ? '✓' : '✗'}</b>
          <span style="flex:1;color:var(--txt);">{testResult.message}</span>
          {#if testResult.elapsedMs > 0}
            <span style="font-size:10px;color:var(--txt2);font-variant-numeric:tabular-nums;">{testResult.elapsedMs < 1000 ? `${testResult.elapsedMs}ms` : `${(testResult.elapsedMs/1000).toFixed(1)}s`}</span>
          {/if}
        </div>
      </div>
    {/if}

    <!-- Footer buttons -->
    <div class="hm-footer">
      {#if editingHost}
      <button class="hm-btn" style="background:var(--red, #ef4444); color:#fff; margin-right:auto;display:flex;align-items:center;gap:6px;" on:click={eliminar}>
        <Trash2 size={13}/> {$trad('Eliminar')}
      </button>
      {/if}
      <!-- Test button — only meaningful for SSH/WinRM; disabled while running.
           Lives between Cancel and Save so it's discoverable without dominating
           the footer. Uses ghost styling so it doesn't compete with Save. -->
      <button class="hm-btn hm-ghost" style="display:flex;align-items:center;gap:6px;"
        on:click={testConnection}
        disabled={testRunning || hostSaving}
        title={$trad('Verificar conexión antes de guardar')}>
        {testRunning ? ($trad('↻ Probando...')) : ($trad('⚡ Probar conexión'))}
      </button>
      <button class="hm-btn hm-ghost" on:click={cancel}>{$trad('Cancelar')}</button>
      <button class="hm-btn hm-pri" on:click={guardarHost} disabled={hostSaving || testRunning}>
        {hostSaving ? ($trad('↻ Guardando...')) : editingHost ? ($trad('Actualizar Host')) : ($trad('Guardar Host'))}
      </button>
    </div>

  </div>
</div>
{/if}

<ConfirmModal
    open={confirmDelete}
    variant="danger"
    title={$trad('Eliminar host')}
    message={$trad('¿Estás seguro de que deseas eliminar este host? Esta acción no se puede deshacer.')}
    detail={editingHost?.name || ''}
    confirmLabel={$trad('Eliminar')}
    cancelLabel={$trad('Cancelar')}
    on:confirm={doDelete}
    on:cancel={() => confirmDelete = false}
/>

<style>
  /* ── Overlay ────────────────────────────────────────────────────────────── */
  .hm-bg {
    position: fixed; inset: 0; z-index: 9999;
    background: rgba(4,8,14,.85); backdrop-filter: blur(6px) saturate(120%);
    display: flex; align-items: center; justify-content: center;
    animation: hm-bg-in .18s ease;
  }
  @keyframes hm-bg-in { from { opacity: 0; } to { opacity: 1; } }
  .hm-box {
    position: relative;
    /* Depth: faint top-lit gradient over the base instead of a flat slab. */
    background:
      radial-gradient(120% 80% at 50% -10%, color-mix(in srgb, var(--host-color,#10b981) 9%, transparent) 0%, transparent 60%),
      var(--bg2, #0b0e14);
    border: 1px solid var(--bdr, #1a2030);
    /* The chosen host colour themes the top edge — ties the "Host Color"
       picker to the modal's identity. */
    border-top: 3px solid var(--host-color, var(--acc, #10b981));
    border-radius: 12px; padding: 22px 20px;
    width: 520px; max-width: 94vw; max-height: 90vh; overflow-y: auto;
    box-shadow:
      0 24px 64px rgba(0,0,0,.7),
      0 0 0 1px color-mix(in srgb, var(--host-color,#10b981) 10%, transparent),
      0 -1px 30px -10px color-mix(in srgb, var(--host-color,#10b981) 40%, transparent);
    animation: hm-box-in .26s var(--ease-out, cubic-bezier(.16,1,.3,1));
  }
  @keyframes hm-box-in { from { opacity: 0; transform: translateY(14px) scale(.98); } to { opacity: 1; transform: none; } }

  /* ── Header ─────────────────────────────────────────────────────────────── */
  .hm-hdr   { display: flex; align-items: center; gap: 11px; margin-bottom: 18px; }
  .hm-hdr-ico {
    display: flex; align-items: center; justify-content: center;
    width: 34px; height: 34px; border-radius: 9px; flex-shrink: 0;
    color: var(--host-color, var(--acc, #10b981));
    background: color-mix(in srgb, var(--host-color,#10b981) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--host-color,#10b981) 28%, transparent);
  }
  .hm-hdr-text { flex: 1; min-width: 0; }
  .hm-subtitle {
    display: block; font-size: 11px; color: var(--txt2, #7a8a9a);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; margin-top: 1px;
  }
  .hm-title { color: var(--txt, #dde3ea); font-size: 15px; font-weight: 600; margin: 0; }
  .hm-close {
    background: transparent; border: none; color: var(--txt2, #7a8a9a);
    font-size: 16px; cursor: pointer; padding: 4px 8px; border-radius: 4px; transition: .15s;
  }
  .hm-close:hover { color: var(--txt, #dde3ea); background: rgba(255,255,255,.05); }

  /* ── Form grid ──────────────────────────────────────────────────────────── */
  .hm-grid  { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-bottom: 14px; }
  .hm-full  { grid-column: 1 / -1; }
  .hm-label { color: var(--txt2, #7a8a9a); font-size: 12px; font-weight: 600; display: block; margin-bottom: 5px; }
  .hm-sub   { color: var(--txt3); font-weight: 400; }
  .hm-inp   {
    width: 100%; background: rgba(0,0,0,.3); border: 1px solid var(--bdr2, #222c3a);
    color: var(--txt, #dde3ea); padding: 8px 10px; border-radius: 6px;
    outline: none; font-family: inherit; font-size: 13px;
    transition: border-color .15s; box-sizing: border-box;
  }
  .hm-inp:focus { border-color: var(--acc, #10b981); box-shadow: 0 0 0 3px color-mix(in srgb, var(--acc,#10b981) 18%, transparent); }
  .hm-mono  { font-family: var(--mono, monospace) !important; font-size: 12px !important; }

  /* ── Color swatches ─────────────────────────────────────────────────────── */
  .hm-swatches { display: flex; gap: 9px; flex-wrap: wrap; margin-top: 4px; }
  .hm-swatch   {
    width: 24px; height: 24px; border-radius: 50%; border: none; padding: 0;
    cursor: pointer; transition: transform .12s var(--ease-out, ease); flex-shrink: 0;
    display: flex; align-items: center; justify-content: center;
  }
  .hm-swatch:hover  { transform: scale(1.18); }
  .hm-swatch.active { transform: scale(1.15); }
  .hm-swatch-check  { color: #fff; font-size: 12px; font-weight: 800; line-height: 1; text-shadow: 0 1px 2px rgba(0,0,0,.55); }

  /* ── SSH key path ───────────────────────────────────────────────────────── */
  .hm-keypath     { margin-bottom: 14px; }
  .hm-keypath-row { display: flex; gap: 6px; }
  .hm-pick {
    background: rgba(0,0,0,.3); border: 1px solid var(--bdr2, #222c3a);
    color: var(--txt, #dde3ea); border-radius: 6px; padding: 6px 10px;
    cursor: pointer; font-size: 14px; flex-shrink: 0; transition: .15s;
  }
  .hm-pick:hover { border-color: var(--acc, #10b981); }
  .hm-note       { font-size: 10px; color: #2a4a3a; margin: 4px 0 0; }
  /* :global so <code> rendered inside .hm-note via {@html} or markdown gets the color */
  .hm-note :global(code) { color: var(--acc, #10b981); }

  /* ── Info box ───────────────────────────────────────────────────────────── */
  .hm-info {
    background: rgba(59,130,246,.06); border: 1px solid rgba(59,130,246,.15);
    border-radius: 6px; padding: 10px 12px; margin-bottom: 14px;
    font-size: 11px; color: #3a5a7a; line-height: 1.6;
  }

  /* ── Footer buttons ─────────────────────────────────────────────────────── */
  .hm-footer { display: flex; gap: 10px; justify-content: flex-end; }
  .hm-btn    {
    padding: 8px 16px; border-radius: 7px; font-size: 13px; font-weight: 600;
    font-family: inherit; cursor: pointer; transition: .15s; border: none;
  }
  .hm-pri  { background: var(--acc, #10b981); color: #030b06; }
  .hm-pri:hover:not(:disabled) { opacity: .85; }
  .hm-pri:disabled { opacity: .5; cursor: not-allowed; }
  .hm-ghost {
    background: transparent; color: var(--txt2, #7a8a9a);
    border: 1px solid var(--bdr, #1a2030) !important;
  }
  .hm-ghost:hover { background: rgba(255,255,255,.04); color: var(--txt, #dde3ea); }
</style>
