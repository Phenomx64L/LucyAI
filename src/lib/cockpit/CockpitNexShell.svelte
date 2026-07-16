<script>
  /* ==========================================================================
     Lucy 2.0 — NexShell (cockpit)  ·  Phase F4/views. LIVE.
     Real remote shell over the configured `hosts` store. Command execution
     STREAMS in real time via `stream_shell_cmd` (+ ssh-out/err/done events),
     like the classic NexShell — output appears as it arrives and interactive
     prompts (y/n, sudo) can be answered by typing while it runs
     (`send_shell_input`, SSH only); `kill_shell_session` stops it. Quick probes
     (connect, OS detection) use the blocking `execute_shell_cmd`. Natural-
     language requests are translated by Lucy into ONE command for the detected
     distro. Credentials via `get_host_credential` — never rendered. Destructive
     commands raise an inline HITL confirm bar INSIDE the overlay. Additive —
     does not touch NexShellView.svelte.
     ========================================================================== */
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { hosts } from '$lib/stores';
  import { LLM } from '$lib/llm-models';
  import { copyToClipboard } from '$lib/lucy-api';
  import Server from '@tabler/icons-svelte/icons/server';
  import DeviceDesktop from '@tabler/icons-svelte/icons/device-desktop';
  import PlugConnected from '@tabler/icons-svelte/icons/plug-connected';
  import Terminal2 from '@tabler/icons-svelte/icons/terminal-2';
  import ArrowUp from '@tabler/icons-svelte/icons/arrow-up';
  import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
  import Plus from '@tabler/icons-svelte/icons/plus';
  import Edit from '@tabler/icons-svelte/icons/edit';
  import Trash from '@tabler/icons-svelte/icons/trash';
  import Copy from '@tabler/icons-svelte/icons/copy';
  import Check from '@tabler/icons-svelte/icons/check';
  import Eraser from '@tabler/icons-svelte/icons/eraser';

  // Host management is delegated to +page.svelte's existing HostModal flow
  // (secure credential save + keyring persistence) via these callbacks. The
  // overlay yields so the real modal is visible — same pattern as HITL.
  let { onAdd = undefined, onEdit = undefined, onDelete = undefined } = $props();
  let confirmDeleteId = $state(null);

  // Catastrophic-command deny list → inline confirm before running.
  const DESTRUCTIVE = /(\brm\s+-rf|\bmkfs\b|\bdd\s+(if|of)=|:\(\)\s*\{|\bshutdown\b|\breboot\b|\bhalt\b|\bformat\s|\bdel\s+\/[sfq]|\brd\s+\/s|Remove-Item[^\n]*-Recurse|drop\s+(database|table)|truncate\s+table|>\s*\/dev\/sd)/i;
  const isDestructive = (c) => DESTRUCTIVE.test(c);

  let activeId = $state(null);
  let sessions = $state({});     // { [hostId]: { lines: [{type,text}], connected, connecting } }
  let draft = $state('');
  let running = $state(false);
  let busyText = $state('ejecutando…');
  let pendingConfirm = $state(null);
  let termEl = $state(null);
  let stream = $state(null);   // { sessionId, hostId } while a streaming command runs
  let _streamSeq = 0;
  let streamElapsed = $state(0);
  let _elapsedTimer = null;
  // Command-history recall (↑/↓ in the input) + copy-output confirmation.
  let _history = [];   // typed commands/requests, oldest→newest (non-rendered)
  let _histIdx = -1;   // -1 = editing a fresh line
  let _copiedOut = $state(false);

  const active = $derived($hosts.find((h) => h.id === activeId) ?? null);
  const prompt = $derived(active ? `${active.username}@${active.name}${active.type === 'linux' ? ':~#' : '>'}` : '');
  const lines = $derived(activeId && sessions[activeId] ? sessions[activeId].lines : []);

  // Auto-select the first host once the store hydrates.
  $effect(() => { if (!activeId && $hosts.length) activeId = $hosts[0].id; });
  // Keep the terminal pinned to the newest line.
  $effect(() => { const _n = lines.length; void _n; if (termEl) termEl.scrollTop = termEl.scrollHeight; });

  function ses(id) {
    if (!sessions[id]) sessions[id] = { lines: [], connected: false, connecting: false };
    return sessions[id];
  }
  function push(id, type, text) {
    const s = ses(id);
    s.lines.push({ type, text });
    if (s.lines.length > 500) s.lines.splice(0, s.lines.length - 500);
  }
  async function cred(h) { try { return await invoke('get_host_credential', { hostId: h.id }); } catch { return ''; } }
  const portFor = (h) => h.port || (h.type === 'linux' ? 22 : 5985);

  async function connect(h) {
    if (!h) return;
    const s = ses(h.id); if (s.connecting) return;
    s.connecting = true;
    push(h.id, 'info', `Conectando a ${h.name} (${h.host}:${portFor(h)})…`);
    try {
      const pwd = await cred(h);
      const probe = h.type === 'linux'
        ? '. /etc/os-release 2>/dev/null; echo "Lucy:OK|$(uname -sr)|$PRETTY_NAME"'
        : '"Lucy:OK|$([System.Environment]::OSVersion.VersionString)|$((Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue).Caption)"';
      const out = await invoke('execute_shell_cmd', { host: h.host, username: h.username, command: probe, hostType: h.type, port: portFor(h), password: pwd || null, keyPath: h.sshKeyPath || null });
      s.connected = true;
      const parts = (String(out).split('\n').find((l) => l.includes('Lucy:OK')) || '').split('|').map((p) => p.trim());
      s.osInfo = parts[2] || parts[1] || '';
      push(h.id, 'info', `✓ Conectado · ${h.type === 'linux' ? 'SSH' : 'WinRM'}${s.osInfo ? ` · ${s.osInfo}` : ''}`);
      if (parts[1]) push(h.id, 'out', parts[1]);
    } catch (e) {
      push(h.id, 'err', String(e));
    } finally {
      s.connecting = false;
    }
  }

  const scrollNow = () => { if (termEl) termEl.scrollTop = termEl.scrollHeight; };

  // Best-effort OS/distro detection so NL translation picks the right package
  // manager (dnf vs apt vs pacman…). Cached per session.
  async function detectOs(h) {
    if (ses(h.id).osInfo) return ses(h.id).osInfo;
    try {
      const probe = h.type === 'linux'
        ? '. /etc/os-release 2>/dev/null; echo "${PRETTY_NAME:-$(uname -sr)}"'
        : '(Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue).Caption';
      const pwd = await cred(h);
      const out = await invoke('execute_shell_cmd', { host: h.host, username: h.username, command: probe, hostType: h.type, port: portFor(h), password: pwd || null, keyPath: h.sshKeyPath || null });
      const info = String(out).trim().split('\n').map((l) => l.trim()).filter(Boolean).pop() || '';
      if (info) ses(h.id).osInfo = info;
    } catch {}
    return ses(h.id).osInfo || '';
  }

  // Real-time streaming execution (like the classic NexShell): output arrives
  // via ssh-out/err events and accumulates into one growing line; ssh-done
  // finalizes. While it runs, the input box feeds `send_shell_input`.
  async function execStream(h, cmd) {
    push(h.id, 'cmd', cmd);
    const s = ses(h.id);
    const sessionId = `nx_${h.id}_${++_streamSeq}`;
    const idx = s.lines.push({ type: 'out', text: '' }) - 1;
    let uns = [];
    const cleanup = () => { for (const u of uns) { try { u(); } catch {} } uns = []; };
    busyText = 'ejecutando…'; running = true; stream = { sessionId, hostId: h.id };
    streamElapsed = 0;
    const _t0 = Date.now();
    if (_elapsedTimer) clearInterval(_elapsedTimer);
    _elapsedTimer = setInterval(() => { streamElapsed = Math.round((Date.now() - _t0) / 1000); }, 1000);
    await new Promise(async (resolve) => {
      let done = false;
      const finish = (info) => {
        if (done) return; done = true;
        if (_elapsedTimer) { clearInterval(_elapsedTimer); _elapsedTimer = null; }
        cleanup(); stream = null; running = false;
        if (!String(s.lines[idx]?.text || '').trim()) s.lines[idx].text = '(sin salida)';
        if (info) push(h.id, 'info', info);
        scrollNow(); resolve();
      };
      try {
        uns.push(await listen(`ssh-out-${sessionId}`, (e) => { s.lines[idx].text += String(e.payload); scrollNow(); }));
        uns.push(await listen(`ssh-err-${sessionId}`, (e) => { s.lines[idx].text += String(e.payload); scrollNow(); }));
        uns.push(await listen(`ssh-done-${sessionId}`, (e) => {
          let exit = null, dur = null;
          try { const p = JSON.parse(String(e.payload)); exit = p.exit_code; dur = p.duration_ms; } catch {}
          s.connected = true;
          finish(exit != null && exit !== 0 ? `⏹ exit ${exit}${dur != null ? ` · ${(dur / 1000).toFixed(1)} s` : ''}` : null);
        }));
        const pwd = await cred(h);
        await invoke('stream_shell_cmd', { sessionId, host: h.host, username: h.username, command: cmd, hostType: h.type, port: portFor(h), password: pwd || null, keyPath: h.sshKeyPath || null });
      } catch (e) {
        s.lines[idx].text += (s.lines[idx].text ? '\n' : '') + String(e);
        finish(null);
      }
    });
  }
  async function runCmd(cmd) {
    const h = active; if (!h || !cmd) return;
    await execStream(h, cmd);
  }
  async function sendInput(text) {
    if (!stream) return;
    push(stream.hostId, 'in', text);
    try { await invoke('send_shell_input', { sessionId: stream.sessionId, input: text + '\n' }); }
    catch (e) { push(stream.hostId, 'err', `input: ${String(e)}`); }
    scrollNow();
  }
  async function stopStream() {
    if (!stream) return;
    try { await invoke('kill_shell_session', { sessionId: stream.sessionId }); } catch {}
    push(stream.hostId, 'info', '■ Detenido por el usuario.');
  }

  // ── Natural language → command ─────────────────────────────────────────────
  // A shell-command-looking input runs raw; a prose request is sent to Lucy to
  // translate into ONE non-interactive command for the host's OS, then executed
  // (routed through the same destructive-confirm gate).
  const KNOWN_CMDS = new Set('ls cd cat pwd echo df du ps top htop free uname whoami id uptime date systemctl service journalctl dnf yum apt apt-get pacman zypper rpm dpkg snap docker podman kubectl git grep egrep awk sed find tail head less more ss netstat ip ifconfig ping curl wget ssh scp rsync tar gzip gunzip zip unzip chmod chown mkdir rmdir rm cp mv ln touch kill killall pkill nohup crontab mount umount lsblk fdisk mkfs sudo su env export set history man which whereis type sh bash zsh python python3 node npm pip pip3 make gcc vim nano reboot shutdown poweroff halt hostnamectl timedatectl firewall-cmd iptables nft getenforce dir ipconfig tasklist taskkill sc net reg wmic powershell pwsh'.split(' '));
  function looksLikeCommand(s) {
    const t = s.trim();
    const first = (t.split(/\s+/)[0] || '').toLowerCase().replace(/[;|&].*$/, '');
    if (KNOWN_CMDS.has(first)) return true;
    if (/^[.~/]/.test(t) || /[|<>]|&&|\|\||\$\(/.test(t) || /^\S+\s+-{1,2}\w/.test(t) || /^[A-Za-z_][\w-]*=/.test(t)) return true;
    if (!/\s/.test(t)) return true;   // single token → assume it's a command
    return false;                     // multi-word, unknown verb → natural language
  }
  function cleanCmd(s) {
    let c = (s || '').trim();
    c = c.replace(/^```[a-z]*\s*/i, '').replace(/```\s*$/, '').trim();
    c = c.replace(/^`+|`+$/g, '').trim();
    c = (c.split('\n').map((l) => l.trim()).filter(Boolean)[0] || '');
    return c.replace(/^\$\s+/, '').trim();
  }
  async function interpretNL(text) {
    const h = active; if (!h) return;
    push(h.id, 'nl', text);
    busyText = 'interpretando…'; running = true;
    let cmd = '';
    try {
      const osInfo = await detectOs(h);
      const shell = h.type === 'linux' ? 'bash' : 'PowerShell';
      const osLabel = osInfo || (h.type === 'linux' ? 'Linux' : 'Windows');
      const prompt = `Traduce la siguiente petición a UN SOLO comando de shell (${shell}) para un host que ejecuta: ${osLabel}. Petición: "${text}". Reglas: responde SOLO con el comando, sin explicación, sin markdown, sin comillas envolventes. Usa el gestor de paquetes correcto según la distro (dnf/yum en Fedora/RHEL/CentOS, apt en Debian/Ubuntu, pacman en Arch, zypper en SUSE, apk en Alpine). Prefiere comandos NO interactivos (añade -y cuando aplique). Para comprobar actualizaciones usa 'dnf check-update' / 'apt list --upgradable' / 'pacman -Qu' según la distro. Si no se puede resolver en un comando, responde exactamente: echo "sin comando".`;
      const raw = await invoke('ask_lucy', { prompt, context: null, userName: 'usuario', model: LLM.FAST, images: null, lang: 'es', hostsJson: null, runbooksDir: null });
      cmd = cleanCmd(String(raw));
    } catch (e) {
      push(h.id, 'err', `IA: ${String(e)}`);
    }
    running = false;
    if (!cmd || /^echo\s+["']?sin comando/i.test(cmd)) { push(h.id, 'err', 'No pude convertir eso en un comando.'); return; }
    if (isDestructive(cmd)) { pendingConfirm = cmd; return; }
    await runCmd(cmd);
  }

  // ── History recall / terminal utilities ───────────────────────────────────
  function _pushHistory(cmd) {
    if (!cmd) return;
    if (_history[_history.length - 1] !== cmd) _history.push(cmd);
    if (_history.length > 100) _history.splice(0, _history.length - 100);
    _histIdx = -1;
  }
  function recallHistory(dir) {
    if (!_history.length) return;
    if (dir < 0) {                       // ↑ older
      _histIdx = _histIdx === -1 ? _history.length - 1 : Math.max(0, _histIdx - 1);
      draft = _history[_histIdx];
    } else {                             // ↓ newer
      if (_histIdx === -1) return;
      _histIdx += 1;
      if (_histIdx >= _history.length) { _histIdx = -1; draft = ''; }
      else draft = _history[_histIdx];
    }
  }
  function clearTerm() { const s = sessions[activeId]; if (s) s.lines = []; }
  async function copyOutput() {
    const txt = lines.map((l) => (l.type === 'cmd' ? `${prompt} ${l.text}` : l.text)).join('\n');
    if (!txt.trim()) return;
    try { await copyToClipboard(txt); }
    catch { try { navigator.clipboard?.writeText(txt); } catch {} }
    _copiedOut = true; setTimeout(() => { _copiedOut = false; }, 1200);
  }

  function submit() {
    const input = draft.trim();
    if (!input || !active) return;
    if (stream) { draft = ''; sendInput(input); return; }   // feed the running command
    if (running) return;                                     // interpreting / connecting
    draft = '';
    _pushHistory(input);
    if (looksLikeCommand(input)) {
      if (isDestructive(input)) { pendingConfirm = input; return; }
      runCmd(input);
    } else {
      interpretNL(input);
    }
  }
  function confirmRun() { const c = pendingConfirm; pendingConfirm = null; if (c) runCmd(c); }
  function cancelRun() { const c = pendingConfirm; pendingConfirm = null; if (c && active) push(active.id, 'info', `✗ Cancelado (destructivo): ${c}`); }
</script>

<div class="nx">
  <aside class="nx-hosts">
    <div class="nx-hosts-head">
      <span class="nx-title">Hosts</span>
      <span class="nx-count">{$hosts.length}</span>
      <button class="add-btn" onclick={() => onAdd?.()} title="Añadir host"><Plus size={14} stroke={2} /> Añadir</button>
    </div>
    {#if $hosts.length === 0}
      <div class="nx-nohosts">
        <Server size={22} stroke={1.4} />
        <p>Sin hosts configurados.</p>
        <button class="add-first" onclick={() => onAdd?.()}><Plus size={14} stroke={2} /> Añadir primer host</button>
      </div>
    {:else}
      {#each $hosts as h (h.id)}
        {@const s = sessions[h.id]}
        <div class="host" class:active={activeId === h.id}>
          <button class="host-hit" onclick={() => (activeId = h.id)}>
            <span class="host-icon">{#if h.type === 'windows'}<DeviceDesktop size={16} stroke={1.75} />{:else}<Server size={16} stroke={1.75} />{/if}</span>
            <span class="host-main">
              <span class="host-name">{h.name}</span>
              <span class="host-addr">{h.host}:{portFor(h)}</span>
            </span>
            <span class="host-side">
              <span class="kind">{h.type === 'linux' ? 'SSH' : 'WinRM'}</span>
              <span class="udot" class:up={s?.connected} class:busy={s?.connecting}></span>
            </span>
          </button>
          {#if confirmDeleteId === h.id}
            <div class="host-confirm"><span>¿Eliminar?</span><button class="hc yes" onclick={() => { onDelete?.(h); confirmDeleteId = null; }}>Sí</button><button class="hc no" onclick={() => (confirmDeleteId = null)}>No</button></div>
          {:else}
            <div class="host-acts">
              <button class="host-act" onclick={() => onEdit?.(h)} title="Editar" aria-label={`Editar ${h.name}`}><Edit size={13} stroke={1.75} /></button>
              <button class="host-act danger" onclick={() => (confirmDeleteId = h.id)} title="Eliminar" aria-label={`Eliminar ${h.name}`}><Trash size={13} stroke={1.75} /></button>
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </aside>

  <section class="nx-main">
    <div class="nx-bar">
      {#if active}
        <span class="nx-host">{#if active.type === 'windows'}<DeviceDesktop size={14} stroke={1.75} />{:else}<Server size={14} stroke={1.75} />{/if} {active.name}</span>
        <span class="nx-user">{active.username}</span>
        {#if stream && stream.hostId === activeId}
          <button class="stop" onclick={stopStream} title="Detener (kill)">■ Detener</button>
        {:else}
          <button class="connect" class:busy={sessions[activeId]?.connecting} onclick={() => connect(active)} disabled={sessions[activeId]?.connecting}>
            <PlugConnected size={14} stroke={1.75} /> {sessions[activeId]?.connected ? 'Reconectar' : 'Conectar'}
          </button>
        {/if}
        <button class="nx-tool" onclick={copyOutput} disabled={!lines.length} title="Copiar salida" aria-label="Copiar salida">
          {#if _copiedOut}<Check size={14} stroke={2} />{:else}<Copy size={14} stroke={1.75} />{/if}
        </button>
        <button class="nx-tool" onclick={clearTerm} disabled={!lines.length} title="Limpiar terminal" aria-label="Limpiar terminal"><Eraser size={14} stroke={1.75} /></button>
      {:else}
        <span class="nx-host muted">Selecciona un host</span>
      {/if}
    </div>

    <div class="term" bind:this={termEl}>
      {#if !active}
        <div class="term-empty">Elige un host de la izquierda para abrir una sesión.</div>
      {:else if lines.length === 0}
        <div class="term-empty ck-blueprint">
          <span class="te-tile"><Terminal2 size={26} stroke={1.4} /></span>
          <div class="te-title">Listo para operar</div>
          <p>Conecta o escribe un comando para <b>{active.name}</b>.</p>
        </div>
      {:else}
        {#each lines as l, i (i)}
          {#if l.type === 'cmd'}
            <div class="tl"><span class="prompt">{prompt}</span> <span class="cmd">{l.text}</span></div>
          {:else if l.type === 'nl'}
            <div class="tl nl">🧠 {l.text}</div>
          {:else if l.type === 'in'}
            <div class="tl in">❯ {l.text}</div>
          {:else if l.type === 'err'}
            <div class="tl err">{l.text}</div>
          {:else if l.type === 'info'}
            <div class="tl info">{l.text}</div>
          {:else}
            <div class="tl out">{l.text}</div>
          {/if}
        {/each}
      {/if}
      {#if running}<div class="tl running"><span class="spin"></span> {busyText}{#if stream && streamElapsed > 0} · {streamElapsed}s · <button class="inline-stop" onclick={stopStream}>detener</button>{/if}</div>{/if}
    </div>

    {#if pendingConfirm}
      <div class="confirm">
        <AlertTriangle size={16} stroke={1.85} />
        <span class="confirm-txt">Comando destructivo en <b>{active?.name}</b>: <code>{pendingConfirm}</code></span>
        <button class="cbtn danger" onclick={confirmRun}>Ejecutar</button>
        <button class="cbtn ghost" onclick={cancelRun}>Cancelar</button>
      </div>
    {/if}

    <div class="nx-input" class:disabled={!active} class:live={stream}>
      <Terminal2 size={16} stroke={1.75} />
      <input
        class="nx-inp"
        placeholder={stream ? 'Respuesta interactiva (p.ej. y) + Enter…' : active ? `Comando o petición en lenguaje natural para ${active.name}…` : 'Selecciona un host…'}
        bind:value={draft}
        disabled={!active || (running && !stream)}
        onkeydown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); submit(); }
          else if (!stream && e.key === 'ArrowUp') { e.preventDefault(); recallHistory(-1); }
          else if (!stream && e.key === 'ArrowDown') { e.preventDefault(); recallHistory(1); }
        }}
      />
      <button class="nx-send" aria-label={stream ? 'Enviar respuesta' : 'Ejecutar'} onclick={submit} disabled={!active || (running && !stream) || !draft.trim()}><ArrowUp size={16} stroke={2} /></button>
    </div>
  </section>
</div>

<style>
  .nx { height: 100%; display: flex; }

  .nx-hosts { width: 240px; border-right: 1px solid var(--border); background: var(--surface-1); padding: 12px; display: flex; flex-direction: column; gap: 6px; overflow-y: auto; }
  .nx-hosts-head { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; }
  /* v1.7.235 "instrumento premium" — label de instrumento + conteo tabular. */
  .nx-title { font-family: var(--font-mono); font-size: var(--fs-micro); font-weight: var(--fw-medium); letter-spacing: var(--ls-label); text-transform: uppercase; color: var(--text-faint); }
  .nx-count { font-family: var(--font-mono); font-variant-numeric: tabular-nums; font-size: var(--fs-caption); color: var(--text-muted); background: var(--surface-3); padding: 0 7px; border-radius: var(--r-pill); }
  .nx-nohosts { background-image: linear-gradient(var(--grid-line) 1px, transparent 1px), linear-gradient(90deg, var(--grid-line) 1px, transparent 1px); background-size: 24px 24px; border-radius: var(--r-md); }
  .add-btn { margin-left: auto; display: flex; align-items: center; gap: 5px; font-size: var(--fs-caption); color: var(--accent); background: var(--accent-bg); border: 0; border-radius: var(--r-sm); padding: 4px 9px; cursor: pointer; transition: background var(--dur-fast) var(--ease-out); }
  .add-btn:hover { background: rgba(61, 214, 164, 0.18); }
  .add-first { display: flex; align-items: center; gap: 6px; margin-top: 6px; font-size: var(--fs-footnote); color: var(--accent); background: var(--accent-bg); border: 0; border-radius: var(--r-md); padding: 7px 12px; cursor: pointer; }

  .nx-nohosts { display: flex; flex-direction: column; align-items: center; gap: 8px; text-align: center; color: var(--text-faint); padding: 32px 10px; }
  .nx-nohosts p { margin: 0; font-size: var(--fs-footnote); color: var(--text-muted); }

  .host { position: relative; display: flex; align-items: center; border-radius: var(--r-md); border: 1px solid transparent; }
  .host:hover { background: var(--surface-2); }
  .host.active { background: var(--surface-2); border-color: var(--border-accent); }
  .host-hit { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; padding: 9px 10px; background: transparent; border: 0; cursor: pointer; text-align: left; }
  .host-icon { color: var(--text-muted); flex-shrink: 0; display: flex; }
  .host.active .host-icon { color: var(--accent); }
  .host-main { display: flex; flex-direction: column; min-width: 0; flex: 1; }
  .host-name { font-size: var(--fs-footnote); color: var(--text-primary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .host-addr { font-size: var(--fs-caption); color: var(--text-faint); font-family: var(--font-mono); }
  .host-side { display: flex; align-items: center; gap: 8px; }
  .kind { font-size: var(--fs-caption); color: var(--text-muted); background: var(--surface-3); padding: 1px 6px; border-radius: 5px; }
  .udot { width: 7px; height: 7px; border-radius: var(--r-pill); background: var(--text-disabled); flex-shrink: 0; }
  .udot.up { background: var(--accent); }
  .udot.busy { background: var(--warning); animation: nx-blink 1s steps(2) infinite; }
  @keyframes nx-blink { 50% { opacity: 0.3; } }

  .host-acts { display: none; position: absolute; right: 7px; gap: 2px; background: var(--surface-2); padding-left: 8px; }
  .host:hover .host-acts { display: flex; }
  .host-act { width: 24px; height: 24px; display: flex; align-items: center; justify-content: center; border: 0; background: var(--surface-3); color: var(--text-muted); border-radius: var(--r-sm); cursor: pointer; transition: color var(--dur-fast) var(--ease-out); }
  .host-act:hover { color: var(--text-primary); }
  .host-act.danger:hover { color: var(--danger); }
  .host-confirm { position: absolute; right: 7px; display: flex; align-items: center; gap: 6px; font-size: var(--fs-caption); color: var(--text-secondary); background: var(--surface-3); padding: 3px 6px; border-radius: var(--r-sm); }
  .hc { font-size: var(--fs-caption); border: 0; border-radius: 4px; padding: 2px 9px; cursor: pointer; }
  .hc.yes { background: var(--danger); color: #12060a; }
  .hc.no { background: transparent; color: var(--text-muted); border: 1px solid var(--border-strong); }

  .nx-main { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .nx-bar { display: flex; align-items: center; gap: 10px; padding: 11px 16px; border-bottom: 1px solid var(--border); }
  .nx-host { display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--text-primary); }
  .nx-host :global(svg) { color: var(--accent); }
  .nx-host.muted { color: var(--text-faint); }
  .nx-host.muted :global(svg) { color: var(--text-faint); }
  .nx-user { font-size: var(--fs-caption); color: var(--text-muted); font-family: var(--font-mono); }
  .connect { margin-left: auto; display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--accent-ink); background: var(--accent); border: 0; border-radius: var(--r-md); padding: 6px 12px; cursor: pointer; transition: background var(--dur-fast) var(--ease-out); }
  .connect:hover { background: var(--accent-hover); }
  .connect.busy, .connect:disabled { opacity: 0.7; cursor: default; }
  .stop { margin-left: auto; display: flex; align-items: center; gap: 6px; font-size: var(--fs-footnote); color: var(--danger); background: var(--danger-bg); border: 1px solid var(--danger); border-radius: var(--r-md); padding: 6px 12px; cursor: pointer; }
  .stop:hover { background: rgba(240, 110, 110, 0.18); }
  /* Terminal utilities (copy output / clear) — sit after connect/stop. */
  .nx-tool {
    display: flex; align-items: center; justify-content: center; width: 28px; height: 28px;
    padding: 0; border: 1px solid var(--border); border-radius: var(--r-md);
    background: var(--surface-2); color: var(--text-muted); cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out);
  }
  .nx-tool:hover:not(:disabled) { background: var(--surface-3); color: var(--text-primary); }
  .nx-tool:disabled { opacity: 0.4; cursor: default; }

  .term { flex: 1; overflow-y: auto; padding: 14px 16px; font-family: var(--font-mono); font-size: var(--fs-code); line-height: 1.75; background: var(--surface-0); }
  /* v1.7.236 iter-2 — de una línea flotante arriba-izquierda en un void de
     pantalla completa a un empty state real: centrado, con tile de instrumento
     y blueprint desvanecido. El área en reposo se lee intencional. */
  .term-empty {
    height: 100%; max-width: 360px; margin: 0 auto;
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px;
    text-align: center; color: var(--text-faint);
    font-family: var(--font-sans); font-size: var(--fs-footnote);
  }
  .term-empty p { margin: 0; color: var(--text-muted); }
  .te-tile {
    display: flex; align-items: center; justify-content: center;
    width: 52px; height: 52px;
    background: var(--surface-1); border: 1px solid var(--border-strong);
    border-radius: var(--r-md); color: var(--text-faint);
  }
  .te-title { font-family: var(--font-display); font-size: var(--fs-heading); color: var(--text-secondary); letter-spacing: -0.01em; }
  .tl { white-space: pre-wrap; word-break: break-word; }
  .tl.out { color: var(--text-secondary); }
  .tl.err { color: var(--danger); }
  .tl.info { color: var(--accent); }
  .tl.nl { color: var(--user); }
  .tl.in { color: var(--user); }
  .tl.running { color: var(--text-muted); display: flex; align-items: center; gap: 8px; }
  .inline-stop { background: transparent; border: 0; color: var(--danger); cursor: pointer; font-family: inherit; font-size: inherit; padding: 0; text-decoration: underline; }
  .prompt { color: var(--accent); }
  .cmd { color: var(--text-primary); }
  .spin { width: 11px; height: 11px; border-radius: var(--r-pill); border: 2px solid var(--text-muted); border-top-color: transparent; animation: nx-spin 0.7s linear infinite; }
  @keyframes nx-spin { to { transform: rotate(360deg); } }

  .confirm { display: flex; align-items: center; gap: 10px; margin: 0 16px; padding: 9px 12px; background: var(--danger-bg); border: 1px solid var(--danger); border-radius: var(--r-md); color: var(--danger); }
  .confirm-txt { flex: 1; min-width: 0; font-size: var(--fs-footnote); color: var(--text-secondary); }
  .confirm-txt code { font-family: var(--font-mono); color: var(--danger); background: rgba(240,110,110,0.1); padding: 1px 5px; border-radius: 4px; }
  .cbtn { font-size: var(--fs-caption); padding: 5px 11px; border-radius: var(--r-sm); cursor: pointer; border: 0; }
  .cbtn.danger { background: var(--danger); color: #12060a; }
  .cbtn.ghost { background: transparent; border: 1px solid var(--border-strong); color: var(--text-muted); }

  .nx-input { display: flex; align-items: center; gap: 9px; margin: 12px 16px 16px; padding: 8px 12px; background: var(--surface-2); border: 1px solid var(--border-strong); border-radius: var(--r-lg); color: var(--text-muted); }
  .nx-input.disabled { opacity: 0.6; }
  .nx-input.live { border-color: var(--border-accent); }
  .nx-inp { flex: 1; min-width: 0; background: transparent; border: 0; outline: 0; color: var(--text-primary); font-size: var(--fs-body); font-family: var(--font-mono); }
  .nx-inp::placeholder { color: var(--text-faint); font-family: var(--font-sans); }
  .nx-send { width: 28px; height: 28px; border-radius: var(--r-md); border: 0; cursor: pointer; background: var(--accent); color: var(--accent-ink); display: flex; align-items: center; justify-content: center; flex-shrink: 0; }
  .nx-send:disabled { opacity: 0.4; cursor: default; }
</style>
