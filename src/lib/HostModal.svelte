<!-- ── HostModal.svelte ──────────────────────────────────────────────────────
     Modal para agregar o editar hosts remotos.
     Props  : show (bindable), editingHost (Host | null)
     Events : saved  { hostObj, isEdit } — el padre actualiza el array de hosts
              error  string              — para mostrar un toast de error
              cancel
──────────────────────────────────────────────────────────────────────────── -->
<script>
    import { createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { focusTrap } from '$lib/actions';
    import ConfirmModal from '$lib/ConfirmModal.svelte';
    import { IconPencil as Pencil, IconPlus as Plus, IconX as X, IconTag as Tag, IconPalette as Palette, IconKey as Key, IconFolderOpen as FolderOpen, IconInfoCircle as Info, IconTrash as Trash2 } from '@tabler/icons-svelte';

    /** Controls visibility; supports bind:show */
    export let show        = false;
    /** Host being edited, or null for a new host */
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
  <div class="hm-box" use:focusTrap>

    <!-- Header -->
    <div class="hm-hdr">
      <h2 class="hm-title">
        <span style="color:var(--blue);display:flex;align-items:center;">{#if editingHost}<Pencil size={14}/>{:else}<Plus size={14}/>{/if}</span>
        {editingHost ? (isEN ? 'Edit Host' : 'Editar Host') : (isEN ? 'New Remote Host' : 'Nuevo Host Remoto')}
      </h2>
      <button class="hm-close" on:click={cancel}><X size={15}/></button>
    </div>

    <!-- Form grid -->
    <div class="hm-grid">
      <div>
        <label class="hm-label" for="hf-name">{isEN ? 'Name *' : 'Nombre *'}</label>
        <input id="hf-name" class="hm-inp" placeholder={isEN ? 'E.g. Prod-Web-01' : 'Ej. Prod-Web-01'}
          bind:value={hostForm.name}>
      </div>
      <div>
        <label class="hm-label" for="hf-proto">{isEN ? 'Protocol *' : 'Protocolo *'}</label>
        <select id="hf-proto" class="hm-inp" bind:value={hostForm.protocol}
          on:change={() => onProtocolChange(hostForm.protocol)}>
          <optgroup label="Shell / {isEN ? 'Remote access' : 'Acceso remoto'}">
            <option value="winrm">🖥 Windows (WinRM)</option>
            <option value="ssh">🐧 Linux (SSH)</option>
            <option value="rdp">🖥 Windows (RDP)</option>
          </optgroup>
          <optgroup label="{isEN ? 'Databases' : 'Bases de datos'}">
            <option value="postgres">🐘 PostgreSQL</option>
            <option value="mysql">🐬 MySQL / MariaDB</option>
            <option value="mongodb">🍃 MongoDB</option>
            <option value="redis">⚡ Redis</option>
            <option value="mssql">🪟 SQL Server</option>
          </optgroup>
          <optgroup label="{isEN ? 'Infrastructure' : 'Infraestructura'}">
            <option value="docker">🐳 Docker API</option>
            <option value="k8s">⎈ Kubernetes API</option>
            <option value="snmp">🌐 SNMP (Red)</option>
          </optgroup>
        </select>
      </div>
      <div>
        <label class="hm-label" for="hf-category">{isEN ? 'Category' : 'Categoría'}</label>
        <select id="hf-category" class="hm-inp" bind:value={hostForm.category}>
          <option value="shell">🖥 {isEN ? 'Server / Shell' : 'Servidor / Shell'}</option>
          <option value="database">🗄️ {isEN ? 'Database' : 'Base de datos'}</option>
          <option value="container">🐳 {isEN ? 'Container (Docker)' : 'Contenedor (Docker)'}</option>
          <option value="kubernetes">⎈ Kubernetes</option>
          <option value="network">🌐 {isEN ? 'Network Device' : 'Dispositivo de red'}</option>
        </select>
      </div>
      {#if hostForm.category === 'database' && !['postgres','mysql','mongodb','redis','mssql'].includes(hostForm.protocol)}
      <div class="hm-full">
        <label class="hm-label" for="hf-dbtype">{isEN ? 'Database Engine' : 'Motor de base de datos'}</label>
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
          placeholder={isEN ? '192.168.1.10 or server.company.com' : '192.168.1.10 ó servidor.empresa.com'}
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
        <label class="hm-label" for="hf-user">{isEN ? 'User ' : 'Usuario '}{hostForm.protocol === 'snmp' ? '(community)' : '*'}</label>
        <input id="hf-user" class="hm-inp hm-mono"
          placeholder={hostForm.protocol === 'snmp' ? 'public' : hostForm.type === 'windows' ? 'DOMINIO/usuario' : 'root'}
          bind:value={hostForm.username}>
      </div>
      <div>
        <label class="hm-label" for="hf-pass">
          {isEN ? 'Password ' : 'Contraseña '} {editingHost ? (isEN ? '(leave empty = no change)' : '(dejar vacío = no cambiar)') : '*'}
        </label>
        <input class="hm-inp" type="password" id="hf-pass"
          placeholder="••••••••" bind:value={hostPassword}>
      </div>
      <div class="hm-full">
        <label class="hm-label" for="hf-tags" style="display:flex;align-items:center;gap:5px;">
          <Tag size={12}/> Tags <span class="hm-sub">({isEN ? 'comma separated — e.g. prod, web, db' : 'separados por coma — ej: prod, web, db'})</span>
        </label>
        <input id="hf-tags" class="hm-inp hm-mono"
          placeholder="prod, web, linux" bind:value={hostForm.tags}>
      </div>
      <div class="hm-full">
        <span class="hm-label" style="display:flex;align-items:center;gap:5px;"><Palette size={12}/> {isEN ? 'Host Color' : 'Color del host'}</span>
        <div class="hm-swatches">
          {#each ['#10b981','#ef4444','#3b82f6','#f59e0b','#a78bfa','#ff6eb4','#00d4ff','#ff8c00'] as c}
          <button class="hm-swatch" class:active={hostForm.color === c}
            style="background:{c};box-shadow:{hostForm.color === c
              ? `0 0 0 2px #fff,0 0 0 4px ${c}`
              : 'none'};"
            on:click={() => hostForm.color = c} title={c}></button>
          {/each}
        </div>
      </div>
    </div>

    <!-- SSH key path (SSH protocol only) -->
    {#if hostForm.protocol === 'ssh'}
    <div class="hm-keypath">
      <label class="hm-label" for="hf-keypath" style="display:flex;align-items:center;gap:5px;">
        <Key size={12}/> {isEN ? 'Private SSH key path' : 'Ruta de clave SSH privada'}
        <span class="hm-sub">({isEN ? 'optional — leave empty to use password' : 'opcional — deja vacío para usar contraseña'})</span>
      </label>
      <div class="hm-keypath-row">
        <input id="hf-keypath" class="hm-inp hm-mono" style="flex:1;"
          placeholder="C:\Users\tu\.ssh\id_rsa o ~/.ssh/id_ed25519"
          bind:value={hostForm.sshKeyPath}>
        <button class="hm-pick" title="{isEN ? 'Select key file' : 'Seleccionar archivo de clave'}"
          on:click={async () => {
            const p = await invoke('pick_file_path').catch(() => '');
            if (p) hostForm.sshKeyPath = p;
          }}><FolderOpen size={14}/></button>
      </div>
      <p class="hm-note">
        {isEN ? 'If specified, <code>ssh -i &lt;path&gt;</code> will be used instead of a password.' : 'Si se especifica, se usa <code>ssh -i &lt;ruta&gt;</code> en lugar de contraseña.'}
      </p>
    </div>
    {/if}

    <!-- Protocol-specific info boxes -->
    {#if hostForm.protocol === 'ssh'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>{isEN ? 'SSH Requirement:' : 'Requisito SSH:'}</b> {isEN ? 'The local machine must have OpenSSH installed (included in Windows 10/11) and the remote host must allow authentication via password or SSH key.' : 'El equipo local debe tener OpenSSH instalado (incluido en Windows 10/11) y el host remoto debe permitir autenticación por contraseña o clave SSH.'}
    </div>
    {:else if hostForm.protocol === 'winrm'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>{isEN ? 'WinRM Requirement:' : 'Requisito WinRM:'}</b> {isEN ? 'The remote server must have WinRM enabled. Run on the server:' : 'El servidor remoto debe tener WinRM habilitado. Ejecuta en el servidor:'}
      <code class="hm-mono" style="font-size:10px;color:var(--acc);">Enable-PSRemoting -Force</code>
    </div>
    {:else if hostForm.protocol === 'rdp'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>RDP:</b> {isEN ? 'Lucy will launch a Remote Desktop session' : 'Lucy lanzará una sesión de Escritorio Remoto'}
      (<code class="hm-mono" style="font-size:10px;">mstsc.exe</code>) {isEN ? 'upon connection. Ensure port 3389 is accessible and remote access is enabled on the server.' : 'al conectar. Asegúrate de que el puerto 3389 esté accesible y el acceso remoto habilitado en el servidor.'}
    </div>
    {:else if hostForm.protocol === 'docker'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>Docker API:</b> {isEN ? 'Requires the Docker daemon to expose the TCP API. Configure in' : 'Requiere que el daemon de Docker exponga el API TCP. Configura en'} <code class="hm-mono" style="font-size:10px;">/etc/docker/daemon.json</code>:
      <code class="hm-mono" style="font-size:10px;color:var(--acc);">"hosts": ["tcp://0.0.0.0:2375"]</code>.
      {isEN ? 'Use TLS (2376) in production.' : 'Usa TLS (2376) en producción.'}
    </div>
    {:else if hostForm.protocol === 'k8s'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>Kubernetes:</b> {isEN ? 'Lucy will execute' : 'Lucy ejecutará comandos'}
      <code class="hm-mono" style="font-size:10px;">kubectl</code> {isEN ? 'commands against the API server. Ensure you have a valid' : 'contra el API server. Asegúrate de tener un'} <code class="hm-mono" style="font-size:10px;">kubeconfig</code> {isEN ? 'valid <code>kubeconfig</code> or a service token.' : 'válido o un token de servicio.'}
    </div>
    {:else if hostForm.protocol === 'snmp'}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>SNMP:</b> {isEN ? 'Lucy will perform SNMP queries (GET/WALK) to the device. The User field is used as the' : 'Lucy realizará consultas SNMP (GET/WALK) al dispositivo. El campo "Usuario" se usa como'} <b>community string</b> (v2c) {isEN ? 'or SNMPv3 user. Standard port: 161.' : 'o usuario SNMPv3. Puerto estándar: 161.'}
    </div>
    {:else if ['postgres','mysql','mongodb','redis','mssql'].includes(hostForm.protocol)}
    <div class="hm-info">
      <b style="color:var(--blue);display:inline-flex;align-items:center;gap:4px;"><Info size={11}/>{isEN ? 'Database' : 'Base de datos'}:</b> {isEN ? 'Lucy will connect to the' : 'Lucy se conectará al motor'}
      <b>{PROTOCOLS.find(p => p.value === hostForm.protocol)?.label.split(' ')[1] || hostForm.protocol}</b>
      {isEN ? `engine on port ${defaultPort(hostForm.protocol)}. Ensure the server accepts remote connections and the user has required permissions.` : `en el puerto ${defaultPort(hostForm.protocol)}. Asegúrate de que el servidor acepte conexiones remotas y que el usuario tenga los permisos necesarios.`}
    </div>
    {/if}

    <!-- Footer buttons -->
    <div class="hm-footer">
      {#if editingHost}
      <button class="hm-btn" style="background:var(--red, #ef4444); color:#fff; margin-right:auto;display:flex;align-items:center;gap:6px;" on:click={eliminar}>
        <Trash2 size={13}/> {isEN ? 'Delete' : 'Eliminar'}
      </button>
      {/if}
      <button class="hm-btn hm-ghost" on:click={cancel}>{isEN ? 'Cancel' : 'Cancelar'}</button>
      <button class="hm-btn hm-pri" on:click={guardarHost} disabled={hostSaving}>
        {hostSaving ? (isEN ? '↻ Saving...' : '↻ Guardando...') : editingHost ? (isEN ? 'Update Host' : 'Actualizar Host') : (isEN ? 'Save Host' : 'Guardar Host')}
      </button>
    </div>

  </div>
</div>
{/if}

<ConfirmModal
    open={confirmDelete}
    variant="danger"
    title={isEN ? 'Delete host' : 'Eliminar host'}
    message={isEN
        ? 'Are you sure you want to delete this host? This action cannot be undone.'
        : '¿Estás seguro de que deseas eliminar este host? Esta acción no se puede deshacer.'}
    detail={editingHost?.name || ''}
    confirmLabel={isEN ? 'Delete' : 'Eliminar'}
    cancelLabel={isEN ? 'Cancel' : 'Cancelar'}
    on:confirm={doDelete}
    on:cancel={() => confirmDelete = false}
/>

<style>
  /* ── Overlay ────────────────────────────────────────────────────────────── */
  .hm-bg {
    position: fixed; inset: 0; z-index: 9999;
    background: rgba(4,8,14,.85); backdrop-filter: blur(4px);
    display: flex; align-items: center; justify-content: center;
  }
  .hm-box {
    background: var(--bg2, #0b0e14);
    border: 1px solid var(--bdr, #1a2030);
    border-radius: 12px; padding: 22px 20px;
    width: 520px; max-width: 94vw; max-height: 90vh; overflow-y: auto;
    box-shadow: 0 24px 64px rgba(0,0,0,.7);
  }

  /* ── Header ─────────────────────────────────────────────────────────────── */
  .hm-hdr   { display: flex; align-items: center; justify-content: space-between; margin-bottom: 18px; }
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
  .hm-sub   { color: #475569; font-weight: 400; }
  .hm-inp   {
    width: 100%; background: rgba(0,0,0,.3); border: 1px solid var(--bdr2, #222c3a);
    color: var(--txt, #dde3ea); padding: 8px 10px; border-radius: 6px;
    outline: none; font-family: inherit; font-size: 13px;
    transition: border-color .15s; box-sizing: border-box;
  }
  .hm-inp:focus { border-color: var(--acc, #10b981); }
  .hm-mono  { font-family: var(--mono, monospace) !important; font-size: 12px !important; }

  /* ── Color swatches ─────────────────────────────────────────────────────── */
  .hm-swatches { display: flex; gap: 7px; flex-wrap: wrap; margin-top: 2px; }
  .hm-swatch   {
    width: 22px; height: 22px; border-radius: 50%; border: none;
    cursor: pointer; transition: transform .1s; flex-shrink: 0;
  }
  .hm-swatch:hover, .hm-swatch.active { transform: scale(1.25); }

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
  .hm-note code  { color: var(--acc, #10b981); }

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
