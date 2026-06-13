<script>
    // v1.4.22 — Broadcast .bc-* layout extracted to a single global stylesheet
    // so the duplicate-selector trap (tab-strip v1.4.17 → v1.4.19) doesn't recur.
    import '$lib/styles/nexshell.css';
    import { invoke } from '@tauri-apps/api/core';
    import { listen } from '@tauri-apps/api/event';
    import { LLM_GROUPS, getModelDescription } from '$lib/models.js';
    // v1.7.0 — central model catalog (single source of truth).
    import { LLM } from '$lib/llm-models';
    import Shield from '@tabler/icons-svelte/icons/shield';

    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';
    import { tick, createEventDispatcher, onDestroy } from 'svelte';
    import { safeParseLS, safeSetLS } from '$lib/safe-ls';
    import { staggerIn } from '$lib/stagger';
    import ShellRecordingPlayer from '$lib/ShellRecordingPlayer.svelte';
    import { logAuditEntry } from '$lib/audit';
    import { analyzeCommand, shouldBlock, checkPermissionRules } from '$lib/hooks/command-guard';
    import { guardConfig } from '$lib/stores';
    import DangerConfirmModal from '$lib/DangerConfirmModal.svelte';
    import TurnLoopPanel from '$lib/TurnLoopPanel.svelte';
    import SkillBrowserModal from '$lib/SkillBrowserModal.svelte';
    import IncidentPanel from '$lib/IncidentPanel.svelte';
    import PromptModal from '$lib/PromptModal.svelte';
    import { IconRocket as Rocket, IconHash as Hash, IconGitBranch as GitBranch, IconSparkles as Sparkles, IconMicrophone as Mic, IconClock as Timer, IconRadio as Radio, IconWorld as Globe, IconBookmark as BookMarked, IconFolderCog as FolderSync, IconActivity as Activity, IconDeviceDesktop as Monitor, IconServer as Server, IconX as X, IconPlayerPlay as Play, IconFolderOpen as FolderOpen, IconBook2 as BookOpen, IconAntenna as Antenna, IconUpload as Upload, IconDownload as Download, IconArrowUp as ArrowUp, IconArrowDown as ArrowDown, IconCpu as Cpu, IconCamera as Camera, IconCircleCheck as CheckCircle, IconAlertCircle as AlertCircle, IconPlayerPause as Pause, IconMessageCircle as MessageCircle, IconLoader as Loader, IconBolt as Zap, IconEdit as Edit2, IconPlug as Plug, IconRefresh as RefreshCw, IconTrash as Trash2, IconFileText as FileText, IconAlarm as Siren } from '@tabler/icons-svelte';
    import {
        createTurnLoop, extractCommand, extractVerdict, cleanAiResponse,
        getDiagnosePrompt, getAnalyzePrompt, getProposePrompt, getVerifyPrompt, getResultPrompt,
        detectStuck, saveTurnLoopCheckpoint, clearTurnLoopCheckpoint,
    } from '$lib/hooks/turn-loop';
    import {
        registerSkill, getSkill, getAllSkills, searchSkills,
        createSkillRun, buildPhasePrompt,
        extractCommand as skExtractCommand, extractVerdict as skExtractVerdict, cleanResponse as skCleanResponse
    } from '$lib/skills/skill-engine';
    import { registerBuiltinSkills } from '$lib/skills/builtin/index';
    // v1.7.160 — render Lucy's prose/analysis as sanitized Markdown (marked +
    // DOMPurify, cached) instead of showing literal ###/** in the log.
    import { renderMd } from '$lib/md-render';
    // Debug logs subsystem extracted to its own module (May 2026 audit).
    // addDebugLog + downloadDebugLogs preserve the original API so call sites
    // throughout this file are unchanged. The buffer + window globals live
    // in the module now.
    import { addDebugLog, downloadDebugLogs as _downloadDebugLogsRaw } from '$lib/page/nexshell-debug-logs';

    // Register built-in skills once on module load
    registerBuiltinSkills();

    const dispatch = createEventDispatcher();

    // Thin wrapper so existing call sites don't need to pass toast/isEN.
    function downloadDebugLogs() { _downloadDebugLogsRaw(toast, isEN); }

    // ── Props (from parent) ─────────────────────────────────────────────────
    export let rshellSessions = [];
    export let activeShellId  = null;
    export let hosts          = [];
    export let lucyConfig     = {};
    export let userLang       = 'es';
    export let selectedModel  = 'gemini-2.5-flash';
    export let isEN           = false;

    // ── Internal NexShell state ─────────────────────────────────────────────
    let nexshellFilter     = '';
    let nsHostsCollapsed   = false;
    let nsInputsCollapsed  = false;
    let nsSort             = 'status';
    let nsCategoryFilter   = 'all';

    // ── Performance: history render cap ──────────────────────────────────
    // Instead of rendering all history entries (which can reach 10k+ in long
    // sessions), we cap the rendered slice to the most recent N entries.
    // User can click "Show more" to expand.
    const NS_RENDER_CAP_DEFAULT = 300;
    const NS_RENDER_CAP_STEP    = 500;
    let nsRenderCap = {};  // per-shell: { [shellId]: number }

    function nsGetCap(shellId) { return nsRenderCap[shellId] || NS_RENDER_CAP_DEFAULT; }
    function nsExpandCap(shellId) {
        nsRenderCap = { ...nsRenderCap, [shellId]: nsGetCap(shellId) + NS_RENDER_CAP_STEP };
    }
    /** Slice history for rendering: returns the most recent N entries */
    function nsVisibleHistory(history, shellId) {
        const cap = nsGetCap(shellId);
        if (history.length <= cap) return history;
        return history.slice(-cap);
    }
    function nsHiddenCount(history, shellId) {
        const cap = nsGetCap(shellId);
        return Math.max(0, history.length - cap);
    }

    function getHostTypeComponent(type) {
        return type === 'windows' ? Monitor : Server;
    }

    // ── Modal states ────────────────────────────────────────────────────────
    let showPlaybookModal      = false;
    let playbookShellId        = null;
    let pbForm                 = { name: '', commands: '' };

    let showBroadcast          = false;
    // Tier S #3 — Session recording player overlay
    let showRecPlayer          = false;
    let recPlayerHostId        = null;
    let recPlayerOpenId        = null;
    let broadcastShellId       = null;
    let broadcastCmd           = '';
    let broadcastSelected      = new Set();
    let broadcastResults       = [];
    let broadcastRunning       = false;

    let showFileTransferModal  = false;
    let ftShellId              = null;
    let ftDirection            = 'upload';
    let ftLocalPath            = '';
    let ftRemotePath           = '';
    let ftRunning              = false;
    let ftResult               = '';

    let showTailModal          = false;
    let tailShellId            = null;
    let tailPath               = '';
    let tailIntervals          = {};

    // ── Command Guard (pre-execution hook) ──────────────────────────────────
    let guardAssessment        = null;
    let guardHostName          = '';
    let guardSource            = 'manual';
    let guardPendingAction     = null;  // () => Promise<void>

    // ── Turn-Loop (formalized troubleshooting) ──────────────────────────────
    let turnLoops              = {};    // shellId → TurnLoopState

    // ── Skill System ────────────────────────────────────────────────────────
    let skillRuns              = {};    // shellId → SkillRunState
    let showSkillBrowser       = false;
    let skillBrowserShellId    = null;

    // ── Output search (Ctrl+F) ──────────────────────────────────────────────
    let nsSearch = {};   // { [shellId]: { query, currentIdx, open } }

    function nsSearchOpen(shellId) {
        nsSearch = { ...nsSearch, [shellId]: { query: '', currentIdx: 0, open: true } };
        tick().then(() => document.getElementById(`ns-sf-${shellId}`)?.focus());
    }
    function nsSearchClose(shellId) {
        nsSearch = { ...nsSearch, [shellId]: { ...nsSearch[shellId], open: false, query: '' } };
    }
    function nsGetMatchIdxs(shellId, query) {
        const s = getShell(shellId);
        if (!s || !query.trim()) return [];
        const q = query.toLowerCase();
        return s.history.reduce((acc, e, i) => {
            if ((e.text || '').toLowerCase().includes(q)) acc.push(i);
            return acc;
        }, []);
    }
    function nsSearchNav(shellId, dir) {
        const st = nsSearch[shellId];
        if (!st) return;
        const idxs = nsGetMatchIdxs(shellId, st.query);
        if (!idxs.length) return;
        let next = (st.currentIdx + dir + idxs.length) % idxs.length;
        nsSearch = { ...nsSearch, [shellId]: { ...st, currentIdx: next } };
        tick().then(() => {
            document.getElementById(`ns-m-${shellId}-${idxs[next]}`)?.scrollIntoView({ block: 'center', behavior: 'smooth' });
        });
    }
    function nsSearchKeydown(e, shellId) {
        if (e.key === 'Escape') { nsSearchClose(shellId); return; }
        if (e.key === 'Enter') { e.preventDefault(); nsSearchNav(shellId, e.shiftKey ? -1 : 1); }
    }

    // ── Helpers ─────────────────────────────────────────────────────────────
    function toast(msg, type = 'info') {
        dispatch('toast', { msg, type });
    }

    const ahora = () => new Date().toLocaleTimeString(userLang, { hour: '2-digit', minute: '2-digit' });

    const getShell = (id) => rshellSessions.find(s => s.id === id);

    /**
     * Adaptive watchdog timeout (Sprint 1, NS-2).
     *
     * The watchdog kills a remote session if no chunks arrive for the budget
     * window. A flat 5min was killing legitimate long-running commands
     * (apt upgrade, npm install, cargo build, rsync, docker pull...).
     *
     * Heuristic table — first match wins, ordered most-specific first:
     *   60 min — system upgrades, dist-upgrade, OS package transactions
     *   30 min — large rsync/scp/sftp transfers
     *   30 min — heavy builds (cargo, gradle, maven, npm install)
     *   15 min — git clone/pull, docker pull, downloads (wget/curl -O)
     *    5 min — default for everything else
     *
     * NOT a hard cap on runtime — the user can still cancel manually. This
     * only governs the "silent for too long" detection.
     */
    function computeWatchdogMs(cmd) {
        if (!cmd) return 5 * 60_000;
        const c = cmd.toLowerCase();
        // 60 min — full-system updates
        if (/\b(apt|apt-get)\s+(upgrade|dist-upgrade|full-upgrade)\b/.test(c)) return 60 * 60_000;
        if (/\b(yum|dnf|zypper)\s+(update|upgrade|distro-sync)\b/.test(c))     return 60 * 60_000;
        if (/\bpacman\s+-Syu\b/.test(c))                                       return 60 * 60_000;
        if (/\bchocolatey\s+upgrade\b|\bchoco\s+upgrade\b|\bwinget\s+upgrade\s+--all\b/.test(c)) return 60 * 60_000;
        // 30 min — large transfers
        if (/\brsync\b|\bscp\b|\bsftp\b\s/.test(c))                            return 30 * 60_000;
        // 30 min — heavy builds + npm/cargo/pip installs
        if (/\bnpm\s+install\b|\byarn\s+install\b|\bpnpm\s+install\b/.test(c)) return 30 * 60_000;
        if (/\bcargo\s+(build|install|update)\b/.test(c))                      return 30 * 60_000;
        if (/\bgradle\s+(build|assemble)\b|\bmvn\s+(install|package)\b/.test(c)) return 30 * 60_000;
        if (/\bdocker\s+build\b/.test(c))                                      return 30 * 60_000;
        if (/\bpip\s+install\b/.test(c))                                       return 30 * 60_000;
        // 15 min — moderate downloads / clones
        if (/\bgit\s+(clone|pull|fetch)\b/.test(c))                            return 15 * 60_000;
        if (/\bdocker\s+pull\b/.test(c))                                       return 15 * 60_000;
        if (/\bwget\b|\bcurl\b.*\s-O\b|\bcurl\b.*--output\b/.test(c))          return 15 * 60_000;
        if (/\binvoke-webrequest\b/.test(c))                                   return 15 * 60_000;
        // Default — quick commands, no excuse to be silent for >5 min
        return 5 * 60_000;
    }

    // ── Reactive: sorted/filtered host list ─────────────────────────────────
    $: nsHostsSorted = (() => {
        let list = hosts.filter(h =>
            (nexshellFilter === '' || h.name.toLowerCase().includes(nexshellFilter.toLowerCase()) || h.host.includes(nexshellFilter)) &&
            (nsCategoryFilter === 'all' || (h.category || 'shell') === nsCategoryFilter)
        );
        if (nsSort === 'name')     return [...list].sort((a,b) => a.name.localeCompare(b.name));
        if (nsSort === 'type')     return [...list].sort((a,b) => (a.category||'shell').localeCompare(b.category||'shell') || a.name.localeCompare(b.name));
        if (nsSort === 'activity') return [...list].sort((a,b) => (b.lastActivity||0) - (a.lastActivity||0));
        return [...list].sort((a,b) => {
            const ac = rshellSessions.find(s=>s.host.id===a.id)?.connected ? 1 : 0;
            const bc = rshellSessions.find(s=>s.host.id===b.id)?.connected ? 1 : 0;
            return bc - ac || a.name.localeCompare(b.name);
        });
    })();

    // ── Icon / label helpers ────────────────────────────────────────────────
    function nsHostIcon(h) {
        const cat = h.category || 'shell';
        if (cat === 'database') { const m={postgres:'◈',mysql:'◈',mongodb:'◈',redis:'⚡',mssql:'⊡'}; return m[h.dbType]||'⊞'; }
        if (cat === 'container')  return '⊟';
        if (cat === 'kubernetes') return '⎈';
        if (cat === 'network')    return '◉';
        return h.type === 'windows' ? '⊡' : '◈';
    }

    function nsProtoLabel(h) {
        if (h.protocol) {
            const labels = { winrm:'WinRM', ssh:'SSH', rdp:'RDP', snmp:'SNMP', docker:'Docker',
                k8s:'K8s', postgres:'PgSQL', mysql:'MySQL', mongodb:'Mongo', redis:'Redis', mssql:'MSSQL' };
            return labels[h.protocol] || h.protocol.toUpperCase();
        }
        const cat = h.category || 'shell';
        if (cat === 'database')   return (h.dbType||'DB').toUpperCase();
        if (cat === 'container')  return 'Docker';
        if (cat === 'kubernetes') return 'K8s';
        if (cat === 'network')    return 'SNMP';
        return h.type === 'windows' ? 'WinRM' : 'SSH';
    }

    function nsRelTime(ts) {
        if (!ts) return '';
        const d = Date.now() - ts;
        if (d < 60000)    return 'hace <1m';
        if (d < 3600000)  return `hace ${Math.floor(d/60000)}m`;
        if (d < 86400000) return `hace ${Math.floor(d/3600000)}h`;
        return `hace ${Math.floor(d/86400000)}d`;
    }

    // ── Scroll helper ───────────────────────────────────────────────────────
    // Sticky autoscroll: only follow the bottom if the user is already near it.
    // If they scrolled up to read backlog, we don't yank them back down.
    function rsScrollBottom(id, force = false) {
        tick().then(() => {
            requestAnimationFrame(() => {
                const el = document.getElementById(`rshell-out-${id}`);
                if (!el) return;
                const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
                // "Near bottom" = within 80px. Force = always.
                if (force || distFromBottom < 80) {
                    el.scrollTop = el.scrollHeight;
                }
            });
        });
    }

    // ── Session Recording (Tier S #3) ──────────────────────────────────────
    // Persistent timeline of every cmd/out/err/exit for a shell session.
    // Storage in SQLite via shell_recording_* commands. The frontend keeps
    // a tiny per-session metadata object (recording_id + t0_ms) on the
    // shell session itself: `s._rec = { id, t0 }`. When grabbing, every
    // chunk handler fires-and-forgets an append.
    //
    // We never await the append — a slow disk shouldn't stall the chat.
    // Failed appends are silently dropped; the recording will be missing
    // some events but the live UI keeps working.
    async function rsRecordingStart(id) {
        const s = getShell(id);
        if (!s || s._rec) return;
        try {
            const recId = await invoke('shell_recording_start', {
                args: {
                    session_id: id,
                    host_id: s.host?.id || '',
                    host_name: s.host?.name || '',
                    host_type: s.host?.type || '',
                    title: '',
                },
            });
            s._rec = { id: recId, t0: Date.now() };
            rshellSessions = [...rshellSessions];
            rsLogTo(id, 'info', `● Grabación iniciada (#${recId})`);
        } catch (e) {
            rsLogTo(id, 'err', `No se pudo iniciar grabación: ${String(e)}`);
        }
    }

    async function rsRecordingStop(id) {
        const s = getShell(id);
        if (!s || !s._rec) return;
        const recId = s._rec.id;
        s._rec = null;
        rshellSessions = [...rshellSessions];
        try {
            await invoke('shell_recording_finish', { recordingId: recId, title: null });
            rsLogTo(id, 'info', `■ Grabación finalizada (#${recId})`);
        } catch (e) {
            rsLogTo(id, 'err', `No se pudo finalizar grabación: ${String(e)}`);
        }
    }

    /** Append fire-and-forget. Never await; never throw. */
    function rsRecordingAppend(id, kind, data) {
        const s = getShell(id);
        if (!s?._rec || !data) return;
        const tMs = Date.now() - s._rec.t0;
        // We slice large chunks to keep individual rows reasonable. SQLite
        // handles big TEXT fine, but the player will paginate output and
        // smaller rows give smoother scrubbing.
        const MAX_ROW = 8192;
        const text = String(data);
        if (text.length <= MAX_ROW) {
            invoke('shell_recording_append', {
                recordingId: s._rec.id, tMs, kind, data: text,
            }).catch(() => {});
        } else {
            // Chunk into ≤8KB pieces, all sharing the same t_ms so the
            // player can re-assemble them as one logical burst.
            for (let i = 0; i < text.length; i += MAX_ROW) {
                const slice = text.slice(i, i + MAX_ROW);
                invoke('shell_recording_append', {
                    recordingId: s._rec.id, tMs, kind, data: slice,
                }).catch(() => {});
            }
        }
    }

    // ── Log to history ──────────────────────────────────────────────────────
    function rsLogTo(id, type, text, meta = {}) {
        const s = getShell(id);
        if (!s) return;
        const entryId = 'e-' + Date.now().toString(36) + '-' + Math.random().toString(36).slice(2,7);
        s.history = [...s.history, { id: entryId, type, text, time: ahora(), ...meta }];
        if (s.history.length > 300) s.history = s.history.slice(-300);
        rshellSessions = [...rshellSessions];
        rsScrollBottom(id);
        // Persistir conversación Lucy al recibir entradas relevantes
        if (_LUCY_CONV_TYPES && _LUCY_CONV_TYPES.has(type) && s.host?.id) {
            rsSaveLucyConv(s.host.id, s.history);
        }
    }

    // ── v1.7.157 — Proactive common-error detection ─────────────────────────
    // Scans a finished command's output for well-known failure fingerprints
    // and returns a one-click "fix chip" descriptor. The suggested commands
    // are DIAGNOSTIC / safe (ps, journalctl, df, ss, dnf provides) — never a
    // blind destructive action (e.g. we identify who holds the rpm lock, we do
    // NOT rm the lock file). The chip is HITL: clicking it only prefills the
    // direct-command box so the user reviews + runs it through the guard.
    function nsDetectCommonError(text, exitCode, hostType) {
        if (!text) return null;
        const linux = hostType !== 'windows';
        // 1) rpm / dnf lock held by another process (fires even on exit 0 — the
        //    Fedora scriptlet case where the transaction "completes" but the
        //    lock errors flood the scriptlet output).
        if (/\.rpm\.lock|recurso no disponible temporalmente|resource temporarily unavailable|another app is currently holding|waiting for process with pid|existing lock .* is held|could not get lock .*(?:dpkg|apt)/i.test(text)) {
            return {
                id: 'pkg-lock', icon: '🔒',
                title: isEN ? 'Package manager is locked' : 'Gestor de paquetes bloqueado',
                hint: isEN
                    ? 'Another dnf/PackageKit/apt process holds the lock. Identify it first — don\'t delete the lock blindly.'
                    : 'Otro proceso (dnf/PackageKit/apt) tiene el lock. Identifícalo primero — no borres el lock a ciegas.',
                fixCmd: linux
                    ? 'ps -eo pid,user,etime,cmd | grep -E "dnf|packagekit|rpm|apt" | grep -v grep'
                    : 'Get-Process msiexec,TrustedInstaller -ErrorAction SilentlyContinue',
            };
        }
        // 2) command not found
        const cnf = text.match(/([\w.\-]+):\s*(?:command not found|orden no encontrada|no se encontró la orden)/i)
                 || text.match(/(?:bash|zsh|sh):\s*([\w.\-]+):/i);
        if (cnf && /command not found|orden no encontrada|no se encontró la orden/i.test(text)) {
            const miss = cnf[1];
            return {
                id: 'cmd-not-found', icon: '❓',
                title: (isEN ? 'Command not found: ' : 'Comando no encontrado: ') + miss,
                hint: isEN ? 'Find which package provides it.' : 'Busca qué paquete lo provee.',
                fixCmd: linux
                    ? `dnf provides ${miss} 2>/dev/null | head -20 || apt-cache search ${miss} 2>/dev/null | head -20`
                    : `Get-Command ${miss} -ErrorAction SilentlyContinue`,
            };
        }
        // 3) permission denied / needs root
        if (/permission denied|acceso denegado|operation not permitted|operación no permitida|must be (?:root|superuser)|debe ser (?:root|superusuario)|are you root\??/i.test(text)) {
            return {
                id: 'perm-denied', icon: '⛔',
                title: isEN ? 'Permission denied' : 'Permiso denegado',
                hint: isEN ? 'Retry the previous command with elevated privileges.' : 'Reintenta el comando anterior con privilegios elevados.',
                fixCmd: linux ? 'sudo !!' : 'Start-Process powershell -Verb RunAs',
            };
        }
        // 4) systemd service failed to start
        if (/job for .+ failed|failed to start|active:\s*failed|unit .+\.service .* failed/i.test(text)) {
            const svc = (text.match(/job for ([\w.@\-]+)\.service/i)
                      || text.match(/unit ([\w.@\-]+)\.service/i) || [])[1];
            return {
                id: 'svc-failed', icon: '🚨',
                title: isEN ? 'Service failed to start' : 'El servicio falló al arrancar',
                hint: isEN ? 'Inspect the journal for the root cause.' : 'Revisa el journal para ver la causa raíz.',
                fixCmd: svc ? `journalctl -xeu ${svc}.service --no-pager | tail -40` : 'journalctl -xe --no-pager | tail -40',
            };
        }
        // 5) disk full
        if (/no space left on device|no queda espacio en el dispositivo|disk quota exceeded/i.test(text)) {
            return {
                id: 'disk-full', icon: '💽',
                title: isEN ? 'No space left on device' : 'Sin espacio en disco',
                hint: isEN ? 'Find what is filling the disk.' : 'Identifica qué está llenando el disco.',
                fixCmd: linux
                    ? 'df -h; echo "── biggest dirs ──"; du -xh / 2>/dev/null | sort -rh | head -20'
                    : 'Get-PSDrive -PSProvider FileSystem | Select Name,Used,Free',
            };
        }
        // 6) port / address already in use
        if (/address already in use|dirección ya está en uso|port .* is already allocated/i.test(text)) {
            const pm = text.match(/:(\d{2,5})\b/);
            const p = pm ? pm[1] : '';
            return {
                id: 'port-in-use', icon: '🔌',
                title: isEN ? 'Address already in use' : 'Dirección/puerto ya en uso',
                hint: isEN ? 'Find which process owns the port.' : 'Identifica qué proceso tiene el puerto.',
                fixCmd: linux
                    ? `ss -ltnp ${p ? `| grep ':${p}'` : ''}`.trim()
                    : `Get-NetTCPConnection ${p ? `-LocalPort ${p}` : ''}`.trim(),
            };
        }
        return null;
    }

    /** Apply a fix chip: prefill the direct-command box (HITL) + focus it.
     *  Never auto-runs — the user reviews and presses Enter (→ guardCheck). */
    function nsApplyFix(shellId, cmd) {
        const s = getShell(shellId);
        if (!s) return;
        s.directIn = cmd;
        rshellSessions = [...rshellSessions];
        setTimeout(() => {
            const el = document.getElementById(`ns-direct-${shellId}`);
            if (el instanceof HTMLElement) { el.focus(); }
        }, 30);
    }

    // ── v1.7.159 — per-command actions (copy / re-run / explain) ────────────
    // Re-run reuses nsApplyFix (prefill the direct box, HITL). Explain prefills
    // the Lucy IA box with a question scoped to that command (HITL — Lucy
    // already has the command's output in its session context).
    let _nsCopiedId = null;
    function nsCmdCopy(entryId, cmd) {
        try { navigator.clipboard.writeText(cmd); } catch { /* clipboard denied */ }
        _nsCopiedId = entryId;
        setTimeout(() => { if (_nsCopiedId === entryId) _nsCopiedId = null; }, 1200);
    }
    function nsCmdExplain(shellId, cmd) {
        const s = getShell(shellId);
        if (!s) return;
        s.lucyIn = (isEN
            ? 'Explain what this command did and whether it errored (check its output above): '
            : 'Explícame qué hizo este comando y si tuvo algún error (revisa su salida arriba): ') + '`' + cmd + '`';
        rshellSessions = [...rshellSessions];
        setTimeout(() => {
            const el = document.getElementById(`ns-lucy-${shellId}`);
            if (el instanceof HTMLElement) el.focus();
        }, 30);
    }

    // ── Interactive prompt patterns ─────────────────────────────────────────
    const RS_PROMPT_PATTERNS = [
        { re: /\[sudo\]\s*password\s+for\s+\S+\s*:/i,    hint: 'Contraseña sudo',   mask: true  },
        { re: /password\s+for\s+\S+@\S+\s*:/i,           hint: 'Contraseña SSH',    mask: true  },
        { re: /(?:^|\n)Password:\s*$/i,                   hint: 'Contraseña',        mask: true  },
        { re: /Enter passphrase for key/i,                hint: 'Passphrase de clave', mask: true },
        { re: /Do you want to continue\?\s*\[Y\/n\]/i,   hint: 'Y o n (Enter = Y)', mask: false },
        { re: /\[Y\/n\]\s*$/i,                            hint: 'Y o n (Enter = Y)', mask: false },
        { re: /\[y\/N\]\s*$/i,                            hint: 'y o N (Enter = N)', mask: false },
        { re: /\(yes\/no(?:\/\[fingerprint\])?\)\s*[?:]?\s*$/i, hint: 'yes o no',   mask: false },
        { re: /Are you sure.*\?\s*$/i,                    hint: 'yes o no',          mask: false },
    ];

    // ── Stream chunk handler ────────────────────────────────────────────────
    function rsHandleStreamChunk(id, chunk, isErr) {
        const s = getShell(id);
        if (!s || !s.isStreaming) return;
        // Bump the watchdog — any chunk means the connection is alive.
        if (typeof s._streamWatchdogBump === 'function') s._streamWatchdogBump();
        // Tier S #3 — Record this chunk if a session recording is active.
        rsRecordingAppend(id, isErr ? 'err' : 'out', chunk);
        s.streamOut = (s.streamOut || '') + chunk;
        // Cap streaming buffer at 100KB to prevent unbounded memory growth
        // on long-running shell sessions (verbose logs, tail -f, etc.). The
        // 100KB tail is more than enough for any human to read on screen.
        const MAX_STREAM = 102400;
        if (s.streamOut.length > MAX_STREAM) {
            s.streamOut = '…[truncado]\n' + s.streamOut.slice(-MAX_STREAM);
        }
        if (!s.waitingForInput) {
            for (const p of RS_PROMPT_PATTERNS) {
                if (p.re.test(s.streamOut)) {
                    s.waitingForInput = true;
                    s.promptIsPassword = p.mask;
                    s.promptHint = p.hint;
                    break;
                }
            }
        }
        rshellSessions = [...rshellSessions];
        // Force-follow the bottom during live streaming. The sticky "only if
        // near bottom" check failed on bursty output: a multi-line chunk grows
        // scrollHeight by more than the 80px threshold in one tick, so the
        // auto-scroll bailed and the user had to scroll manually. While a
        // command is actively streaming we pin to the bottom like a terminal.
        rsScrollBottom(id, true);
    }

    // ── Cancel active stream ────────────────────────────────────────────────
    async function cancelarStream(id) {
        const s = getShell(id);
        if (!s || !s.isStreaming) return;
        const partial  = s.streamOut?.trim() || '';
        const resolve  = s._streamResolve;
        s.streamOut = '';
        s.isStreaming = false;
        s.running = false;
        s.waitingForInput = false;
        s.interactiveInput = '';
        s._streamResolve = null;
        rshellSessions = [...rshellSessions];
        if (partial) rsLogTo(id, 'out', partial);
        rsLogTo(id, 'info', '! Ejecución cancelada por el usuario');
        if (resolve) resolve(partial);
        invoke('kill_shell_session', { sessionId: id }).catch(() => {});
    }

    // ── Stream done ─────────────────────────────────────────────────────────
    function rsStreamDone(id, exitCode = null, durationMs = null) {
        const s = getShell(id);
        if (!s || !s.isStreaming) return;
        // Stop the watchdog — natural completion, no hang to worry about.
        if (s._streamWatchdogInterval) {
            try { clearInterval(s._streamWatchdogInterval); } catch {}
            s._streamWatchdogInterval = null;
            s._streamWatchdogBump = null;
        }
        // Tier S #3 — record the exit event before any final cleanup. The
        // payload is JSON-encoded so the player can show exit + duration
        // without parsing free text.
        rsRecordingAppend(id, 'exit', JSON.stringify({
            exit_code: exitCode, duration_ms: durationMs,
        }));
        const finalOut = s.streamOut || '';
        if (finalOut.trim()) rsLogTo(id, 'out', finalOut, { exitCode, durationMs });
        // v1.7.157 — proactive fix chip for well-known failures (rpm lock,
        // command-not-found, perms, service-failed, disk-full, port-in-use).
        // Light dedup so a repeating turn-loop command doesn't stack chips.
        if (finalOut.trim()) {
            try {
                const _fix = nsDetectCommonError(finalOut, exitCode, s.host?.type);
                if (_fix && !s.history.slice(-6).some(h => h.type === 'fix-chip' && h.fix?.id === _fix.id)) {
                    rsLogTo(id, 'fix-chip', _fix.title, { fix: _fix });
                }
            } catch { /* detection is best-effort */ }
        }
        // SSH exit 255 = connection-level failure (drop / keepalive timeout),
        // NOT the remote command's own exit code. With a PTY the command was
        // SIGHUP'd, but long rpm/apt transactions survive on the host — so this
        // "done" does NOT mean the task finished. Warn loudly so neither the
        // user nor the agent assumes success.
        if (exitCode === 255) {
            rsLogTo(id, 'err', isEN
                ? '⚠ SSH connection closed (exit 255) — NOT a clean finish. The command may STILL be running on the remote host. Reconnect and verify before assuming it completed.'
                : '⚠ La conexión SSH se cerró (exit 255) — NO es una finalización limpia. El comando PUEDE seguir ejecutándose en el host remoto. Reconéctate y verifica antes de darlo por terminado.');
        }
        // Log to audit trail
        const lastCmd = [...(s.history || [])].reverse().find(h => h.type === 'cmd' || h.type === 'lucy-in');
        if (lastCmd) {
            logAuditEntry({
                hostId: s.host?.id || 'unknown',
                hostName: s.host?.name || 'Unknown',
                command: lastCmd.text.replace(/^\$ /, ''),
                source: lastCmd.type === 'lucy-in' ? 'ai' : 'manual',
                exitCode, durationMs,
                outputPreview: finalOut.substring(0, 500),
                user: lucyConfig?.name || '',
            });
        }
        // ── Incident Mode: auto-capture command output as evidence ──────────
        if (s.incidentId && lastCmd) {
            const cmdText = String(lastCmd.text || '').replace(/^\$ /, '').substring(0, 200);
            const src = `${s.host?.type || 'shell'}:${cmdText}`;
            const tags = [
                lastCmd.type === 'lucy-in' ? 'ai' : 'manual',
                exitCode !== null && exitCode !== 0 ? 'error' : 'ok',
                s.host?.name || 'local',
            ];
            // Fire-and-forget; helper is safe & no-ops on failure
            captureEvidenceIfIncident(id, src, finalOut, tags);
        }
        s.streamOut = '';
        s.isStreaming = false;
        s.running = false;
        s.waitingForInput = false;
        s.promptHint = '';
        s.interactiveInput = '';
        rshellSessions = [...rshellSessions];
        if (s._streamResolve) {
            const resolve = s._streamResolve;
            s._streamResolve = null;
            resolve(finalOut);
        }
        rsScrollBottom(id);
    }

    // ── Send interactive input ──────────────────────────────────────────────
    async function rsEnviarInput(id) {
        const s = getShell(id);
        if (!s || !s.waitingForInput) return;
        const input = s.interactiveInput;
        s.waitingForInput = false;
        s.interactiveInput = '';
        const display = s.promptIsPassword ? '••••••\n' : input + '\n';
        s.streamOut = (s.streamOut || '') + display;
        rshellSessions = [...rshellSessions];
        try {
            await invoke('send_shell_input', { sessionId: id, input });
        } catch(e) {
            rsLogTo(id, 'err', `Error enviando input: ${e}`);
        }
    }

    // ── Command Guard — pre-execution risk check + permission rules ──────────
    async function guardCheck(cmd, hostType, hostName, source, action) {
        if (!$guardConfig.enabled) { action(); return; }
        if (source === 'ai'        && !$guardConfig.interceptAI)        { action(); return; }
        if (source === 'broadcast'  && !$guardConfig.interceptBroadcast) { action(); return; }

        // Check permission rules first (takes priority over risk assessment)
        const permResult = await checkPermissionRules(cmd, 'command');
        if (permResult.action === 'block') {
            // Permission denied - show error
            const msg = isEN
                ? `Permission denied: ${permResult.reason}`
                : `Permiso denegado: ${permResult.reason}`;
            console.warn('Permission blocked:', cmd, permResult.reason);
            // Could show a toast or modal here instead of just logging
            return;
        } else if (permResult.action === 'ask') {
            // Permission rule requires confirmation - show confirmation modal
            guardAssessment    = {
                level: 'high',
                score: 80,
                command: cmd,
                matches: [],
                summary: isEN
                    ? `Permission rule requires confirmation: ${permResult.reason}`
                    : `Regla de permiso requiere confirmación: ${permResult.reason}`,
            };
            guardHostName      = hostName || 'local';
            guardSource        = source;
            guardPendingAction = action;
            return;
        }
        // If action === 'allow', continue to risk assessment

        const assessment = analyzeCommand(cmd, hostType || 'linux', isEN);
        if (shouldBlock(assessment, $guardConfig.threshold)) {
            guardAssessment    = assessment;
            guardHostName      = hostName || 'local';
            guardSource        = source;
            guardPendingAction = action;
        } else {
            action();
        }
    }

    function guardConfirm() {
        const action = guardPendingAction;
        guardAssessment = null;
        guardPendingAction = null;
        if (action) action();
    }

    function guardCancel() {
        const shellId = activeShellId;
        if (shellId && guardAssessment) {
            rsLogTo(shellId, 'info', `⬡ ${isEN ? 'Command blocked by guard' : 'Comando bloqueado por guardia'}: ${guardAssessment.command.substring(0, 80)}`);
        }
        guardAssessment = null;
        guardPendingAction = null;
    }

    // ── Turn-Loop orchestrator ────────────────────────────────────────────
    function tlStop(shellId) {
        const tl = turnLoops[shellId];
        if (tl) {
            tl.active = false;
            tl.phase = 'failed';
            tl.summary = isEN ? 'Stopped by user' : 'Detenido por el usuario';
            turnLoops = { ...turnLoops };
            rsLogTo(shellId, 'info', `↻ Turn-Loop ${isEN ? 'stopped' : 'detenido'}`);
        }
    }

    async function tlAskLucy(shellId, prompt, context) {
        const s = getShell(shellId);
        return invoke('ask_lucy', {
            prompt, context,
            userName: lucyConfig.name,
            runbooksDir: lucyConfig.runbooksDir || null,
            model: selectedModel || 'gemini-2.5-flash',
            images: null, lang: userLang, hostsJson: null
        });
    }

    function tlBootCtx(shellId) {
        const s = getShell(shellId);
        if (!s) return '';
        const b = s.bootstrap;
        return b ? [
            b.os ? `OS: ${b.os}` : '', b.kernel ? `Kernel: ${b.kernel}` : '',
            b.user ? `User: ${b.user}` : '', b.tools ? `Tools: ${b.tools}` : '',
        ].filter(Boolean).join(', ') : '';
    }

    function tlCleanCmd(cmd, hostType) {
        if (hostType !== 'linux') {
            const icM = cmd.match(/Invoke-Command\s+(?:-\S+\s+\S+\s+)*-ScriptBlock\s*\{([\s\S]+)\}/i);
            if (icM) cmd = icM[1].trim();
        } else {
            if (/^ssh\s/i.test(cmd)) {
                const sshM = cmd.match(/ssh(?:\s+-\S+\s+\S+)*\s+\S+@\S+\s+["']?([\s\S]+?)["']?\s*$/i);
                if (sshM) cmd = sshM[1].trim();
            }
        }
        return cmd;
    }

    async function tlRunTurnLoop(shellId, problem) {
        const s = getShell(shellId);
        if (!s) return;
        const tl = createTurnLoop(problem, s.host.name, s.host.type === 'linux' ? 'Linux' : 'Windows');
        turnLoops = { ...turnLoops, [shellId]: tl };
        rsLogTo(shellId, 'info', `↻ Turn-Loop ${isEN ? 'started' : 'iniciado'}: "${problem.substring(0, 80)}"`);

        while (tl.active && tl.iteration <= tl.maxIterations) {
            turnLoops = { ...turnLoops };

            // ── STUCK CHECK (before each iteration) ──
            const stuckSignal = detectStuck(tl);
            if (stuckSignal.isStuck) {
                rsLogTo(shellId, 'err', `⚠ ${isEN ? 'Stuck detected' : 'Estancamiento detectado'}: ${stuckSignal.reason}`);
                if (stuckSignal.severity === 'critical') {
                    tl.phase = 'failed'; tl.active = false;
                    tl.summary = isEN ? `Stopped: ${stuckSignal.reason}` : `Detenido: ${stuckSignal.reason}`;
                    rsLogTo(shellId, 'info', `✗ Turn-Loop: ${tl.summary}`);
                    saveTurnLoopCheckpoint(shellId, tl);
                    break;
                }
                // warning: log but continue (user can manually stop)
            }

            // ── PHASE 1: DIAGNOSE ──
            tl.phase = 'diagnose';
            turnLoops = { ...turnLoops };
            rsLogTo(shellId, 'lucy-out', `◎ **Turn-Loop [${tl.iteration}/${tl.maxIterations}]** — ${isEN ? 'Diagnosing...' : 'Diagnosticando...'}`);

            let diagResp;
            try {
                diagResp = await tlAskLucy(shellId, getDiagnosePrompt(tl, tlBootCtx(shellId), isEN), '');
            } catch(e) { rsLogTo(shellId, 'err', `Turn-Loop diag error: ${e}`); tl.phase = 'failed'; tl.active = false; break; }

            const diagCmd = extractCommand(diagResp);
            const diagClean = cleanAiResponse(diagResp);
            if (diagClean) rsLogTo(shellId, 'lucy-out', diagClean);

            if (!diagCmd) {
                rsLogTo(shellId, 'info', `↻ ${isEN ? 'No diagnostic command generated' : 'Sin comando diagnostico generado'}`);
                tl.steps.push({ phase: 'diagnose', timestamp: Date.now(), aiResponse: diagClean });
                tl.phase = 'failed'; tl.active = false; break;
            }

            const cleanDiagCmd = tlCleanCmd(diagCmd, s.host.type);
            rsLogTo(shellId, 'cmd', `$ ${cleanDiagCmd}`);
            let diagOut = '';
            try { diagOut = await rsRunStreaming(shellId, cleanDiagCmd); } catch(e) { diagOut = String(e); }
            tl.steps.push({ phase: 'diagnose', timestamp: Date.now(), command: cleanDiagCmd, output: diagOut, aiResponse: diagClean });
            saveTurnLoopCheckpoint(shellId, tl);
            if (!tl.active) break;

            // ── PHASE 2: ANALYZE ──
            tl.phase = 'analyze';
            turnLoops = { ...turnLoops };
            rsLogTo(shellId, 'lucy-out', `◑ ${isEN ? 'Analyzing results...' : 'Analizando resultados...'}`);

            let analyzeResp;
            try {
                analyzeResp = await tlAskLucy(shellId, getAnalyzePrompt(tl, diagOut, isEN), '');
            } catch(e) { rsLogTo(shellId, 'err', `Turn-Loop analyze error: ${e}`); tl.phase = 'failed'; tl.active = false; break; }

            const analyzeClean = cleanAiResponse(analyzeResp);
            rsLogTo(shellId, 'lucy-out', analyzeClean);
            tl.steps.push({ phase: 'analyze', timestamp: Date.now(), aiResponse: analyzeClean });

            const verdict1 = extractVerdict(analyzeResp);
            if (verdict1 === 'NO_ISSUE') {
                tl.phase = 'done'; tl.resolved = true; tl.active = false;
                tl.summary = isEN ? 'No issue detected.' : 'No se detecto problema.';
                rsLogTo(shellId, 'info', `✓ Turn-Loop: ${tl.summary}`);
                break;
            }
            // Si no hay VERDICT, asumir CAN_FIX y continuar al PROPOSE en vez de hacer más diag
            if (verdict1 === 'NEEDS_MORE_DIAG') {
                // Run another diagnostic in same iteration
                rsLogTo(shellId, 'info', `◎ ${isEN ? 'Running additional diagnosis...' : 'Ejecutando diagnostico adicional...'}`);
                tl.phase = 'diagnose';
                turnLoops = { ...turnLoops };
                let diag2Resp;
                try { diag2Resp = await tlAskLucy(shellId, getDiagnosePrompt(tl, tlBootCtx(shellId), isEN), ''); } catch(e) { break; }
                const diag2Cmd = extractCommand(diag2Resp);
                const diag2Clean = cleanAiResponse(diag2Resp);
                if (diag2Clean) rsLogTo(shellId, 'lucy-out', diag2Clean);
                if (diag2Cmd) {
                    const cleanDiag2 = tlCleanCmd(diag2Cmd, s.host.type);
                    rsLogTo(shellId, 'cmd', `$ ${cleanDiag2}`);
                    let d2Out = '';
                    try { d2Out = await rsRunStreaming(shellId, cleanDiag2); } catch(e) { d2Out = String(e); }
                    tl.steps.push({ phase: 'diagnose', timestamp: Date.now(), command: cleanDiag2, output: d2Out, aiResponse: diag2Clean });
                    diagOut += '\n' + d2Out;
                }
                if (!tl.active) break;
            }

            // ── PHASE 3: PROPOSE ──
            tl.phase = 'propose';
            turnLoops = { ...turnLoops };
            rsLogTo(shellId, 'lucy-out', `→ ${isEN ? 'Proposing fix...' : 'Proponiendo solucion...'}`);

            let proposeResp;
            try {
                proposeResp = await tlAskLucy(shellId, getProposePrompt(tl, isEN), '');
            } catch(e) { rsLogTo(shellId, 'err', `Turn-Loop propose error: ${e}`); tl.phase = 'failed'; tl.active = false; break; }

            const proposeCmd = extractCommand(proposeResp);
            const proposeClean = cleanAiResponse(proposeResp);
            if (proposeClean) rsLogTo(shellId, 'lucy-out', proposeClean);
            tl.steps.push({ phase: 'propose', timestamp: Date.now(), command: proposeCmd, aiResponse: proposeClean });

            if (!proposeCmd) {
                rsLogTo(shellId, 'info', `↻ ${isEN ? 'No fix command proposed' : 'Sin comando de fix propuesto'}`);
                tl.phase = 'failed'; tl.active = false; break;
            }
            if (!tl.active) break;

            // ── PHASE 4: APPLY (with guard!) ──
            tl.phase = 'apply';
            turnLoops = { ...turnLoops };
            const cleanFixCmd = tlCleanCmd(proposeCmd, s.host.type);

            let fixApplied = false;
            let fixOut = '';
            await new Promise(async (resolve) => {
                await guardCheck(cleanFixCmd, s.host.type, s.host.name, 'ai', async () => {
                    rsLogTo(shellId, 'cmd', `$ ${cleanFixCmd}`);
                    try {
                        fixOut = await rsRunStreaming(shellId, cleanFixCmd);
                        fixApplied = true;
                    } catch(e) { fixOut = String(e); }
                    resolve();
                });
                // If guard blocks and user cancels, we still need to resolve
                const checkCancel = setInterval(() => {
                    // Defensive: stop polling if the component went away
                    // mid-wait (user navigated to another view). Without
                    // this guard the interval ticks forever against stale state.
                    if (_componentDestroyed) {
                        clearInterval(checkCancel);
                        resolve();
                        return;
                    }
                    if (!guardAssessment && !fixApplied) {
                        clearInterval(checkCancel);
                        resolve();
                    }
                }, 500);
            });

            if (!fixApplied) {
                rsLogTo(shellId, 'info', `⬡ ${isEN ? 'Fix was blocked/cancelled. Stopping loop.' : 'Fix bloqueado/cancelado. Deteniendo loop.'}`);
                tl.steps.push({ phase: 'apply', timestamp: Date.now(), command: cleanFixCmd, aiResponse: isEN ? 'Blocked by guard' : 'Bloqueado por guardia' });
                tl.phase = 'failed'; tl.active = false; break;
            }
            tl.steps.push({ phase: 'apply', timestamp: Date.now(), command: cleanFixCmd, output: fixOut });
            saveTurnLoopCheckpoint(shellId, tl);
            if (!tl.active) break;

            // ── PHASE 5: VERIFY ──
            tl.phase = 'verify';
            turnLoops = { ...turnLoops };
            rsLogTo(shellId, 'lucy-out', `✓ ${isEN ? 'Verifying fix...' : 'Verificando solucion...'}`);

            let verifyResp;
            try {
                verifyResp = await tlAskLucy(shellId, getVerifyPrompt(tl, fixOut, isEN), '');
            } catch(e) { rsLogTo(shellId, 'err', `Turn-Loop verify error: ${e}`); tl.phase = 'failed'; tl.active = false; break; }

            const verifyCmd = extractCommand(verifyResp);
            const verifyClean = cleanAiResponse(verifyResp);
            if (verifyClean) rsLogTo(shellId, 'lucy-out', verifyClean);

            let verifyOut = '';
            if (verifyCmd) {
                const cleanVerify = tlCleanCmd(verifyCmd, s.host.type);
                rsLogTo(shellId, 'cmd', `$ ${cleanVerify}`);
                try { verifyOut = await rsRunStreaming(shellId, cleanVerify); } catch(e) { verifyOut = String(e); }
                tl.steps.push({ phase: 'verify', timestamp: Date.now(), command: cleanVerify, output: verifyOut, aiResponse: verifyClean });
            } else {
                tl.steps.push({ phase: 'verify', timestamp: Date.now(), aiResponse: verifyClean });
            }
            if (!tl.active) break;

            // ── PHASE 6: RESULT CHECK ──
            let resultResp;
            try {
                resultResp = await tlAskLucy(shellId, getResultPrompt(tl, verifyOut || fixOut, isEN), '');
            } catch(e) { tl.phase = 'failed'; tl.active = false; break; }

            const resultClean = cleanAiResponse(resultResp);
            rsLogTo(shellId, 'lucy-out', resultClean);

            const verdict2 = extractVerdict(resultResp);

            // Si la IA no devuelve VERDICT tag, tratar como PARTIAL para evitar
            // que el loop continúe indefinidamente con un veredicto silencioso.
            if (verdict2 === null) {
                tl.summary = resultClean || (isEN ? 'No verdict from AI — manual review recommended.' : 'Sin veredicto de la IA — revisión manual recomendada.');
                tl.phase = 'done'; tl.resolved = false; tl.active = false;
                rsLogTo(shellId, 'info', `! Turn-Loop: ${isEN ? 'No VERDICT tag in AI response. Stopping.' : 'Sin tag VERDICT en respuesta IA. Deteniendo.'}`);
                break;
            }

            if (verdict2 === 'RESOLVED') {
                tl.phase = 'done'; tl.resolved = true; tl.active = false;
                tl.summary = resultClean;
                rsLogTo(shellId, 'info', `✓ Turn-Loop: ${isEN ? 'Problem resolved!' : 'Problema resuelto!'}`);
                break;
            } else if (verdict2 === 'PARTIAL') {
                tl.summary = resultClean;
                tl.phase = 'done'; tl.resolved = false; tl.active = false;
                rsLogTo(shellId, 'info', `! Turn-Loop: ${isEN ? 'Partially resolved. Manual intervention may be needed.' : 'Parcialmente resuelto. Puede necesitar intervencion manual.'}`);
                break;
            } else {
                // NOT_RESOLVED — loop again
                tl.iteration++;
                if (tl.iteration > tl.maxIterations) {
                    tl.phase = 'failed'; tl.active = false;
                    tl.summary = isEN ? 'Max iterations reached without full resolution.' : 'Iteraciones maximas alcanzadas sin resolucion completa.';
                    rsLogTo(shellId, 'info', `! Turn-Loop: ${tl.summary}`);
                } else {
                    rsLogTo(shellId, 'info', `↻ ${isEN ? 'Not resolved. Starting iteration' : 'No resuelto. Iniciando iteracion'} ${tl.iteration}...`);
                }
            }
        }
        // Clean up checkpoint when loop finishes (success or failure)
        clearTurnLoopCheckpoint(shellId);
        turnLoops = { ...turnLoops };
    }

    // ── Skill Orchestrator ─────────────────────────────────────────────────
    async function skRunSkill(shellId, skill, userInput = '') {
        const s = getShell(shellId);
        if (!s) return;

        const run = createSkillRun(skill, userInput);
        skillRuns = { ...skillRuns, [shellId]: run };
        rsLogTo(shellId, 'info', `≡ ${isEN ? 'Skill started' : 'Skill iniciado'}: ${skill.icon} ${isEN ? skill.nameEN : skill.name}`);

        const bootCtx = tlBootCtx(shellId);
        const hostType = s.host.type === 'linux' ? 'Linux' : 'Windows';

        for (let i = 0; i < skill.phases.length; i++) {
            const phase = skill.phases[i];
            run.currentPhaseIdx = i;
            run.phaseStatus = 'running';
            skillRuns = { ...skillRuns };

            rsLogTo(shellId, 'lucy-out', `**[${i + 1}/${skill.phases.length}] ${isEN ? phase.nameEN : phase.name}**`);

            // Build phase prompt and ask AI
            const prompt = buildPhasePrompt(skill, phase, run, s.host.name, hostType, bootCtx, isEN);
            let aiResp;
            try {
                aiResp = await tlAskLucy(shellId, prompt, '');
            } catch (e) {
                rsLogTo(shellId, 'err', `Skill phase error: ${e}`);
                run.phaseStatus = 'error';
                run.active = false;
                skillRuns = { ...skillRuns };
                return;
            }

            const cmd = skExtractCommand(aiResp);
            const verdict = skExtractVerdict(aiResp);
            const clean = skCleanResponse(aiResp);
            if (clean) rsLogTo(shellId, 'lucy-out', clean);

            let output = '';
            if (cmd) {
                const cleanCmd = tlCleanCmd(cmd, s.host.type);
                // Use guard for commands
                let executed = false;
                await new Promise(async (resolve) => {
                    await guardCheck(cleanCmd, s.host.type, s.host.name, 'ai', async () => {
                        rsLogTo(shellId, 'cmd', `$ ${cleanCmd}`);
                        try {
                            output = await rsRunStreaming(shellId, cleanCmd);
                            executed = true;
                        } catch (e) { output = String(e); executed = true; }
                        resolve();
                    });
                    const check = setInterval(() => {
                        // Same defensive guard as turn-loop checkCancel — bail
                        // out cleanly if the component unmounted mid-wait.
                        if (_componentDestroyed) { clearInterval(check); resolve(); return; }
                        if (!guardAssessment && !executed) { clearInterval(check); resolve(); }
                    }, 500);
                });

                if (!executed) {
                    rsLogTo(shellId, 'info', `⬡ ${isEN ? 'Command blocked. Skipping phase.' : 'Comando bloqueado. Saltando fase.'}`);
                }
            }

            // Record result
            run.results.push({
                phaseId: phase.id,
                phaseName: isEN ? phase.nameEN : phase.name,
                command: cmd || undefined,
                output: output || undefined,
                aiResponse: clean || undefined,
                verdict: verdict || undefined,
                timestamp: Date.now(),
            });

            run.phaseStatus = 'done';
            skillRuns = { ...skillRuns };

            if (!run.active) break;

            // If verdict says DONE or ESCALATE, stop
            if (verdict === 'DONE' || verdict === 'ESCALATE') {
                rsLogTo(shellId, 'info', verdict === 'DONE'
                    ? `✓ ${isEN ? 'Skill completed successfully.' : 'Skill completado exitosamente.'}`
                    : `! ${isEN ? 'Skill escalated. Manual intervention needed.' : 'Skill escalado. Se necesita intervención manual.'}`);
                break;
            }
        }

        run.active = false;
        skillRuns = { ...skillRuns };
        rsLogTo(shellId, 'info', `≡ ${isEN ? 'Skill finished' : 'Skill finalizado'}: ${skill.icon} ${isEN ? skill.nameEN : skill.name}`);
    }

    function skOpenBrowser(shellId) {
        skillBrowserShellId = shellId;
        showSkillBrowser = true;
    }

    function skOnRun(e) {
        showSkillBrowser = false;
        const { skill, userInput } = e.detail;
        if (skillBrowserShellId) {
            skRunSkill(skillBrowserShellId, skill, userInput);
        }
    }

    // ── Core streaming ──────────────────────────────────────────────────────
    function rsRunStreaming(id, cmd) {
        const s = getShell(id);
        if (!s) return Promise.resolve('');
        // Tier S #3 — record the command BEFORE we mark isStreaming so the
        // chunk handler can already consume the 'cmd' event before the
        // first stdout arrives.
        rsRecordingAppend(id, 'cmd', cmd);
        s.running = true;
        s.isStreaming = true;
        s.streamOut = '';
        s.waitingForInput = false;
        s.promptHint = '';
        s.interactiveInput = '';
        rshellSessions = [...rshellSessions];
        return new Promise((resolve) => {
            s._streamResolve = resolve;
            // ── Adaptive Watchdog (Sprint 1, NS-2) ───────────────────────────
            // Was a hardcoded 5 min for ALL commands — but legitimate long
            // operations (`apt upgrade`, `npm install`, `cargo build`, `rsync`)
            // legitimately go silent for 10-30 min while compiling / fetching.
            // The flat watchdog killed them as false-positives.
            //
            // Now: timeout is selected from a small heuristic table by
            // command shape. The user sees the chosen budget as a badge
            // during execution so they know what to expect.
            //
            // Categories (silent-period budget in MS):
            //   60 min — heavy installs / system upgrades / sync
            //   30 min — long downloads, builds, image pulls
            //   15 min — clones, transfers
            //    5 min — default (Get-*, ls, ping, anything quick)
            const watchdogMs = computeWatchdogMs(cmd);
            s._streamWatchdogBudget = watchdogMs;
            let _lastChunkAt = Date.now();
            s._streamWatchdogBump = () => { _lastChunkAt = Date.now(); };
            const _watchdog = setInterval(() => {
                if (!s.isStreaming) {
                    clearInterval(_watchdog);
                    return;
                }
                if (Date.now() - _lastChunkAt > watchdogMs) {
                    clearInterval(_watchdog);
                    const mins = Math.round(watchdogMs / 60000);
                    rsLogTo(id, 'err', `⏱ Watchdog: ${mins} min sin chunks. Cerrando sesión por seguridad.`);
                    invoke('kill_shell_session', { sessionId: id }).catch(() => {});
                    const sx = getShell(id);
                    if (sx) { sx.running = false; sx.isStreaming = false; rshellSessions = [...rshellSessions]; }
                    if (s._streamResolve) { s._streamResolve(s.streamOut || ''); s._streamResolve = null; }
                }
            }, 30_000); // check every 30s — cheap, doesn't need to be precise
            s._streamWatchdogInterval = _watchdog;

            invoke('stream_shell_cmd', {
                sessionId: id,
                host: s.host.host, username: s.host.username, command: cmd,
                hostType: s.host.type,
                port: s.host.port || (s.host.type === 'linux' ? 22 : 5985),
                password: s.host.password || null, keyPath: s.host.sshKeyPath || null
            }).catch(e => {
                clearInterval(_watchdog);
                rsLogTo(id, 'err', String(e));
                const sx = getShell(id);
                if (sx) { sx.running = false; sx.isStreaming = false; }
                rshellSessions = [...rshellSessions];
                if (s._streamResolve) { s._streamResolve(''); s._streamResolve = null; }
            });
        });
    }

    // ── Open remote shell ───────────────────────────────────────────────────
    async function abrirRShell(h) {
        const existing = rshellSessions.find(s => s.host.id === h.id);
        if (existing) {
            activeShellId = existing.id;
            existing.minimized = false;
            rshellSessions = [...rshellSessions];
            return;
        }
        const pwd = await invoke('get_host_credential', { hostId: h.id }).catch(() => '');
        const id  = `shell_${h.id}_${Date.now()}`;
        const _restoredConv = rsLoadLucyConv(h.id);
        const isRdp = (h.protocol === 'rdp');

        rshellSessions = [...rshellSessions, {
            id, host: { ...h, password: pwd },
            connected: isRdp, // RDP sessions are always "connected" in clipboard mode
            rdpMode: isRdp,
            rdpClipboardCmd: null,   // last <EXECUTE> extracted for clipboard
            rdpResultIn: '',         // textarea for pasting RDP output
            // ── RDP Computer-Use agent ──────────────────────────────────────
            rdpAgentRunning: false,
            rdpAgentLog: [],         // [{kind, data, detail, ts}]
            rdpAgentScreenshot: null,// base64 PNG of latest frame
            rdpAgentTask: '',
            rdpAgentPanel: false,
            rdpAgentHwnd: null,
            rdpAgentProvider: 'anthropic',    // fixed: Claude only for RDP Computer Use
            rdpAgentModel: 'claude-sonnet-4-5', // fixed: best for Computer Use (OSWorld-optimized)
            rdpAgentProviderStatus: null,     // 'ok' | 'error' | null
            history: _restoredConv,
            directIn: '', lucyIn: '',
            running: false, lucyRunning: false, minimized: false,
            bootstrap: null,
            streamOut: '',
            isStreaming: false,
            waitingForInput: false,
            promptIsPassword: false,
            promptHint: '',
            interactiveInput: '',
            _streamResolve: null,
            _unlisten: null,
            _aiSugg: '',
            _aiSuggLoading: false,
            _aiSuggTimer: null,
            bgTasks: [],
            // ── Incident Response / SRE Mode (Nivel 4) ──────────────────────
            // When active, command outputs auto-tag as evidence for the current
            // incident, and the LLM receives the phase-specific system prompt.
            incidentId: null,          // active incident record id, or null
            incidentPhase: null,       // cached phase for quick prompt lookup
            incidentPanelOpen: false,  // UI visibility toggle
        }];
        activeShellId = id;

        // ── RDP mode: launch mstsc.exe and skip WinRM connection ──────────────
        if (isRdp) {
            rsLogTo(id, 'info', `⊡ Modo RDP — lanzando sesión de Escritorio Remoto hacia ${h.name} (${h.host})…`);
            rsLogTo(id, 'info', `· ${isEN ? 'Lucy operates in clipboard copilot mode: she generates commands, you paste them in the RDP window and return the output.' : 'Lucy opera en modo copiloto de portapapeles: genera comandos, pégalos en la ventana RDP y devuelve el resultado.'}`);
            try {
                await invoke('launch_rdp', { host: h.host, port: h.port || 3389 });
                rsLogTo(id, 'info', `✓ mstsc.exe lanzado · ${isEN ? 'Connect to the remote desktop and ask Lucy for help.' : 'Conéctate al escritorio remoto y pide ayuda a Lucy.'}`);
            } catch(e) {
                rsLogTo(id, 'err', `✗ No se pudo lanzar mstsc.exe: ${e}`);
            }
            return;
        }

        // ── NS-5 (Sprint 3): SSH key path pre-flight ────────────────────────
        // If the user configured a key path but the file is missing/typo'd,
        // ssh will fail with the generic "Permission denied (publickey)" —
        // making it look like an auth problem when it's actually a typo.
        // Surface a precise error BEFORE we burn a connection attempt.
        if (h.type === 'linux' && h.sshKeyPath) {
            try {
                const probe = await invoke('path_exists', { path: h.sshKeyPath });
                if (!probe?.exists) {
                    rsLogTo(id, 'err',
                        `✗ SSH key no encontrada: ${h.sshKeyPath} · Verifica la ruta en el host (Editar → SSH Key Path).`);
                    return;
                }
                if (!probe?.is_file) {
                    rsLogTo(id, 'err',
                        `✗ La ruta de SSH key no es un archivo: ${h.sshKeyPath}`);
                    return;
                }
            } catch (e) {
                // Probe itself errored (e.g. permission denied reading metadata).
                // Don't block — surface a warning and let ssh try anyway.
                rsLogTo(id, 'warn',
                    `· No pude verificar la SSH key (${String(e).slice(0,120)}). Intento conectar de todas formas…`);
            }
        }

        // ── Normal WinRM/SSH path ──────────────────────────────────────────────
        const outUnlisten  = await listen(`ssh-out-${id}`,  (e) => rsHandleStreamChunk(id, String(e.payload), false));
        const errUnlisten  = await listen(`ssh-err-${id}`,  (e) => rsHandleStreamChunk(id, String(e.payload), true));
        const doneUnlisten = await listen(`ssh-done-${id}`, (e) => {
            let exitCode = null, durationMs = null;
            try { const p = JSON.parse(e.payload); exitCode = p.exit_code ?? null; durationMs = p.duration_ms ?? null; } catch {}
            rsStreamDone(id, exitCode, durationMs);
        });
        const sInit = getShell(id);
        if (sInit) {
            sInit._unlisten = () => { outUnlisten(); errUnlisten(); doneUnlisten(); };
            rshellSessions = [...rshellSessions];
        }

        rsLogTo(id, 'info', `Conectando a ${h.name} (${h.host})...`);
        const testCmd = h.type === 'linux' ? 'echo "Lucy:OK" && uname -a' : 'echo "Lucy:OK"; $env:OS';
        // ── NS-4 (Sprint 2): Auto-reconnect con backoff exponencial ─────────
        // Networks blip. WinRM listeners restart. SSH daemons get reloaded.
        // A failed first attempt rarely means the host is permanently down —
        // it usually means "try again in a few seconds". We retry up to
        // RECONNECT_MAX times with delays 2s / 4s / 8s before surfacing the
        // failure to the user. Cancelable: if the user closes the shell
        // mid-retry the existence check (getShell) bails out.
        const RECONNECT_MAX = 3;
        const RECONNECT_DELAYS_MS = [2_000, 4_000, 8_000];
        let out = null;
        let lastErr = null;
        for (let attempt = 0; attempt <= RECONNECT_MAX; attempt++) {
            // Bail if the user closed the shell mid-retry.
            if (!getShell(id)) return;
            if (attempt > 0) {
                const delay = RECONNECT_DELAYS_MS[attempt - 1] || 8_000;
                rsLogTo(id, 'info', `⟳ Reintento ${attempt}/${RECONNECT_MAX} en ${Math.round(delay/1000)}s…`);
                await new Promise(r => setTimeout(r, delay));
                if (!getShell(id)) return; // user closed during the wait
            }
            try {
                out = await invoke('execute_shell_cmd', {
                    host: h.host, username: h.username, command: testCmd,
                    hostType: h.type, port: h.port || (h.type === 'linux' ? 22 : 5985), password: pwd || null, keyPath: h.sshKeyPath||null
                });
                lastErr = null;
                break; // success — exit retry loop
            } catch (e) {
                lastErr = e;
                if (attempt < RECONNECT_MAX) {
                    rsLogTo(id, 'warn', `· Intento ${attempt + 1} falló: ${String(e).slice(0, 120)}`);
                }
            }
        }
        try {
            if (lastErr) throw lastErr;
            const s = getShell(id);
            if (s) { s.connected = true; rshellSessions = [...rshellSessions]; }
            rsLogTo(id, 'info', `✓ ${isEN ? 'Connected to' : 'Conectado a'} ${h.name} · ${h.type === 'linux' ? 'SSH activo' : 'WinRM'}`);
            rsLogTo(id, 'out', out.replace('Lucy:OK\n','').trim());

            invoke('nexshell_bootstrap', {
                host: h.host, username: h.username, hostType: h.type,
                port: h.port || (h.type === 'linux' ? 22 : 5985),
                password: pwd || null, keyPath: h.sshKeyPath || null
            }).then(data => {
                const sb = getShell(id);
                if (!sb) return;
                sb.bootstrap = data;
                rshellSessions = [...rshellSessions];
                const env = [];
                if (data.git_branch) env.push(`⊕ ${data.git_branch}${data.git_dirty ? '*' : ''}`);
                if (data.k8s_ctx)    env.push(`⎈ ${data.k8s_ctx}`);
                if (data.docker)     env.push('⊟ Docker');
                if (data.node_ver)   env.push(`⬡ Node ${data.node_ver}`);
                if (data.python_venv)env.push(`◈ ${data.python_venv}`);
                if (env.length)      rsLogTo(id, 'info', `Entorno: ${env.join(' · ')}`);
                if (data.tools)      rsLogTo(id, 'info', `Herramientas: ${data.tools}`);
            }).catch(() => {});

        } catch(e) {
            rsLogTo(id, 'err', `✗ No se pudo conectar tras ${RECONNECT_MAX + 1} intentos: ${e}`);
        }
    }

    // ── Close shell ─────────────────────────────────────────────────────────
    function cerrarShell(id) {
        const dying = getShell(id);
        // Tier S #3 — if a recording is active, finalize it BEFORE we wipe
        // the session state. The finish call is async but we don't await
        // (it's an INSERT, milliseconds) — the user closing the shell
        // shouldn't have to wait for SQLite.
        if (dying?._rec) rsRecordingStop(id);
        if (dying?._unlisten) dying._unlisten();
        if (dying?.isStreaming) invoke('kill_shell_session', { sessionId: id }).catch(() => {});
        // ── BUG FIX (May 2026): _streamResolve hang on shell close ──────────
        // Any code awaiting `rsRunStreaming(id, cmd)` when the user closes
        // the shell would hang forever — the done event never fires after
        // _unlisten() removed the listener. Resolve with empty string so
        // the caller's chain progresses to its finally block.
        if (dying?._streamResolve) {
            try { dying._streamResolve(''); } catch {}
            dying._streamResolve = null;
        }
        // Stop the watchdog if active (mirrors rsStreamDone cleanup).
        if (dying?._streamWatchdogInterval) {
            try { clearInterval(dying._streamWatchdogInterval); } catch {}
            dying._streamWatchdogInterval = null;
            dying._streamWatchdogBump = null;
        }
        // ── BUG FIX (May 2026): orphaned tailIntervals on shell close ───────
        // tailIntervals keys are `${shellId}::${path}` — when the shell goes
        // away we must clear any tails that were polling it, otherwise the
        // timers keep firing against a dead session, push log noise into the
        // closed shell's history (now invisible), and leak memory.
        const prefix = `${id}::`;
        for (const key of Object.keys(tailIntervals)) {
            if (key.startsWith(prefix)) {
                clearTimeout(tailIntervals[key]);
                delete tailIntervals[key];
            }
        }
        // Cancel any pending AI suggestion timer for this shell too.
        if (dying?._aiSuggTimer) {
            try { clearTimeout(dying._aiSuggTimer); } catch {}
            dying._aiSuggTimer = null;
        }
        rshellSessions = rshellSessions.filter(s => s.id !== id);
        if (activeShellId === id) {
            const otra = rshellSessions.find(s => !s.minimized) || rshellSessions[rshellSessions.length-1];
            activeShellId = otra?.id || null;
        }
    }

    function minimizarShell(id) {
        const s = getShell(id);
        if (s) { s.minimized = true; rshellSessions = [...rshellSessions]; }
    }

    function restaurarShell(id) {
        const s = getShell(id);
        if (s) { s.minimized = false; rshellSessions = [...rshellSessions]; }
        activeShellId = id;
    }

    // ── Send direct command ─────────────────────────────────────────────────
    async function rsEnviarDirecto(id) {
        const s = getShell(id);
        if (!s) return;
        const cmd = s.directIn.trim();
        if (!cmd || s.running || s.isStreaming) return;
        s.directIn = ''; s._histIdx = undefined;
        s.running = true;                          // activa spinner "Verificando…" inmediatamente
        rshellSessions = [...rshellSessions];
        rsSaveHistory(s.host.id, cmd);
        await guardCheck(cmd, s.host.type, s.host.name, 'manual', async () => {
            rsLogTo(id, 'cmd', `$ ${cmd}`);
            await rsRunStreaming(id, cmd);
        });
        // Si el guard bloqueó y no llegó a rsRunStreaming, resetear el flag
        const sx = getShell(id);
        if (sx && !sx.isStreaming) { sx.running = false; rshellSessions = [...rshellSessions]; }
    }

    // ── askLucyStream wrapper (chunks vía Tauri events) ─────────────────────
    async function askLucyStream(params, onChunk) {
        const requestId = `req_${Date.now()}_${Math.random().toString(36).slice(2)}`;
        let accumulated = '';
        const unlisten = await listen(`lucy-chunk-${requestId}`, (event) => {
            accumulated += event.payload;
            onChunk(accumulated);
        });
        try {
            const result = await invoke('ask_lucy_stream', { ...params, requestId });
            return result;
        } finally {
            unlisten();
        }
    }

    // ── Helpers para entries de razonamiento y tool-cards ──────────────────
    function rsPushReasoning(id) {
        const s = getShell(id);
        if (!s) return null;
        const entry = {
            type: 'reasoning',
            id: 'r-' + Math.random().toString(36).slice(2,9),
            time: ahora(),
            startTs: Date.now(),
            duration: 0,
            content: '',
            active: true,
            collapsed: false,
        };
        s.history = [...s.history, entry];
        if (s.history.length > 300) s.history = s.history.slice(-300);
        rshellSessions = [...rshellSessions];
        rsScrollBottom(id);
        return entry;
    }
    function rsUpdateReasoning(id, entry, deltaOrContent, isFullReplace = false) {
        const s = getShell(id);
        if (!s || !entry) return;
        if (isFullReplace) entry.content = deltaOrContent;
        else entry.content += deltaOrContent;
        entry.duration = (Date.now() - entry.startTs) / 1000;
        rshellSessions = [...rshellSessions];
        rsScrollBottom(id);
    }
    function rsFinishReasoning(id, entry) {
        if (!entry) return;
        entry.active = false;
        entry.collapsed = true;
        entry.duration = (Date.now() - entry.startTs) / 1000;
        rshellSessions = [...rshellSessions];
    }
    function rsPushToolCard(id, icon, label, kind = 'exec') {
        const s = getShell(id);
        if (!s) return null;
        const entry = {
            type: 'tool-card',
            id: 'tc-' + Math.random().toString(36).slice(2,9),
            time: ahora(),
            icon, label, kind,
            status: 'running',
            startTs: Date.now(),
            duration: 0,
            output: '',
        };
        s.history = [...s.history, entry];
        if (s.history.length > 300) s.history = s.history.slice(-300);
        rshellSessions = [...rshellSessions];
        rsScrollBottom(id);
        return entry;
    }
    function rsFinishToolCard(id, entry, output, ok = true) {
        if (!entry) return;
        entry.status = ok ? 'done' : 'error';
        entry.duration = (Date.now() - entry.startTs) / 1000;
        entry.output = output || '';
        rshellSessions = [...rshellSessions];
        rsScrollBottom(id);
    }

    // ── Send Lucy query ─────────────────────────────────────────────────────
    async function rsEnviarLucy(id) {
        const s = getShell(id);
        if (!s) return;
        const raw = s.lucyIn.trim();
        if (!raw || s.lucyRunning) {
            addDebugLog('LUCY_STATE', `Skipping - raw empty or already running`, { raw: !!raw, running: s.lucyRunning });
            return;
        }
        addDebugLog('LUCY_STATE', 'Starting Lucy request', { shellId: id, input: raw.substring(0, 50), model: selectedModel });
        s.lucyIn = ''; s.lucyRunning = true;
        rshellSessions = [...rshellSessions];
        rsLogTo(id, 'lucy-in', raw);

        // ── /fix trigger → start Turn-Loop ──
        const fixMatch = raw.match(/^\/fix\s+(.+)$/i);
        if (fixMatch) {
            const problem = fixMatch[1].trim();
            s.lucyRunning = false; rshellSessions = [...rshellSessions];
            await tlRunTurnLoop(id, problem);
            return;
        }
        const b = s.bootstrap;
        const bootCtx = b ? [
            b.os       ? `OS: ${b.os}`                                              : '',
            b.kernel   ? `Kernel: ${b.kernel}`                                      : '',
            b.user     ? `User: ${b.user}`                                          : '',
            b.cwd      ? `CWD: ${b.cwd}`                                            : '',
            b.git_branch ? `Git branch: ${b.git_branch}${b.git_dirty ? ' (dirty)' : ''}` : '',
            b.k8s_ctx  ? `Kubernetes context: ${b.k8s_ctx}`                         : '',
            b.docker   ? 'Docker: available'                                         : '',
            b.node_ver ? `Node.js: v${b.node_ver}`                                  : '',
            b.python_venv ? `Python venv: ${b.python_venv}`                         : '',
            b.tools    ? `Installed tools: ${b.tools}`                              : '',
        ].filter(Boolean).join(', ') : '';
        // ── Context: RDP clipboard mode vs normal WinRM/SSH ──────────────────
        const hostCtx = s.rdpMode
            ? `REMOTE DESKTOP (RDP) CLIPBOARD COPILOT MODE — Server: "${s.host.name}", Windows, IP: ${s.host.host}, User: ${s.host.username}.
Lucy CANNOT execute commands directly. The user is working in a graphical Windows Remote Desktop session.
WORKFLOW:
1. Generate ONE PowerShell command at a time inside <EXECUTE></EXECUTE> — it will appear in a clipboard strip for the user to copy and paste in the RDP window.
2. After pasting, the user will run it, copy the output, and paste it back here as a result.
3. Analyze the output and decide the next step.
4. If the user pastes a SCREENSHOT, analyze what you see on the remote desktop and guide accordingly.
RULES:
- Use PowerShell syntax (Windows desktop environment).
- Wrap commands in <EXECUTE> as usual — the clipboard engine picks them up.
- Never assume you ran something — wait for the user to paste the result.
- Reason with a Markdown blockquote (> · Razonamiento:) before each command.
Recent history:\n${s.history.slice(-6).map(h=>`[${h.type}] ${String(h.text ?? h.content ?? h.label ?? '').substring(0,100)}`).join('\n')}`
            : `ACTIVE REMOTE SHELL — the WinRM/SSH session to "${s.host.name}" is ALREADY ESTABLISHED. Type: ${s.host.type === 'linux' ? 'Linux (SSH)' : 'Windows (WinRM)'}. IP: ${s.host.host}. User: ${s.host.username}.${bootCtx ? '\nHost context: ' + bootCtx : ''}\nCRITICAL RULES FOR THIS CONTEXT:\n1. ALWAYS reason first with a Markdown blockquote (> · Razonamiento:) and THEN wrap commands in <EXECUTE></EXECUTE> — even for informational requests like "is X installed?" or "check Y". The execution engine will run the command and show output.\n2. Generate ONLY raw commands inside <EXECUTE> — NO Invoke-Command, NO -ComputerName, NO -Credential, NO ssh wrappers.\n3. If the user asks a question about the remote host, answer with a command that retrieves the answer (<EXECUTE>), NOT with an explanation.\n4. OVERRIDE RULE 8, RULE 9 and RULE 0 for this context — always use <EXECUTE>.\nRecent history:\n${s.history.slice(-6).map(h=>`[${h.type}] ${String(h.text ?? h.content ?? h.label ?? '').substring(0,100)}`).join('\n')}`;
        try {
            let enrichedCtx = hostCtx;
            const rsUrlMatches = [...raw.matchAll(/https?:\/\/[^\s"'<>()]+/gi)];
            if (rsUrlMatches.length > 0) {
                rsLogTo(id, 'info', `↻ Leyendo ${rsUrlMatches.length} URL${rsUrlMatches.length>1?'s':''}…`);
                const rsUrls = rsUrlMatches.slice(0,2).map(m=>m[0]);
                const rsFetched = await Promise.allSettled(rsUrls.map(u=>invoke('fetch_url_content',{url:u})));
                rsFetched.forEach((r,i) => {
                    if (r.status==='fulfilled' && r.value)
                        enrichedCtx += `\n\n--- CONTENIDO WEB: ${rsUrls[i]} ---\n${r.value}\n--- FIN CONTENIDO WEB ---`;
                });
            }
            // ── Live reasoning bubble ──
            const reasoningEntry = rsPushReasoning(id);
            let _lastThoughtLen = 0;
            addDebugLog('LUCY', 'askLucyStream iniciado', { model: selectedModel, shellId: id });
            const startTime = Date.now();
            const resp = await askLucyStream({
                prompt: raw, context: enrichedCtx, userName: lucyConfig.name, runbooksDir: lucyConfig.runbooksDir || null,
                model: selectedModel || 'gemini-2.5-flash',
                images: null, lang: userLang, hostsJson: JSON.stringify(hosts)
            }, (acc) => {
                addDebugLog('STREAM_CHUNK', `${acc.length} chars acumulados`, null);
                const m = acc.match(/<THOUGHT>([\s\S]*?)(?:<\/THOUGHT>|$)/i);
                if (m) {
                    const cur = m[1];
                    if (cur.length > _lastThoughtLen) {
                        const delta = cur.slice(_lastThoughtLen);
                        _lastThoughtLen = cur.length;
                        rsUpdateReasoning(id, reasoningEntry, delta);
                    }
                }
            });
            const elapsed = Date.now() - startTime;
            addDebugLog('LUCY', 'askLucyStream completado', { elapsed: `${elapsed}ms`, respLength: resp.length });
            rsFinishReasoning(id, reasoningEntry);

            const execM = resp.match(/<EXECUTE>([\s\S]*?)<\/EXECUTE>/i)
                       || resp.match(/```(?:powershell|ps1|batch|cmd|bash|sh)?\s*\n([\s\S]*?)\n```/i);
            if (execM) {
                let cmd = execM[1].trim();
                if (s.host.type !== 'linux') {
                    const icM = cmd.match(/Invoke-Command\s+(?:-\S+\s+\S+\s+)*-ScriptBlock\s*\{([\s\S]+)\}/i);
                    if (icM) cmd = icM[1].trim();
                    const npsM = cmd.match(/Invoke-Command\s+-Session\s+\S+\s+-ScriptBlock\s*\{([\s\S]+)\}/i);
                    if (npsM) cmd = npsM[1].trim();
                } else {
                    if (/^ssh\s/i.test(cmd)) {
                        const sshM = cmd.match(/ssh(?:\s+-\S+\s+\S+)*\s+\S+@\S+\s+["']?([\s\S]+?)["']?\s*$/i);
                        if (sshM) cmd = sshM[1].trim();
                    }
                }
                const clean = resp.replace(/<EXECUTE>[\s\S]*?(?:<\/EXECUTE>|$)/gi, '').replace(/<THOUGHT>[\s\S]*?(?:<\/THOUGHT>|$)/gi, '').trim();
                if (clean) rsLogTo(id, 'lucy-out', clean);

                // ── RDP clipboard mode: surface command for manual execution ──
                if (s.rdpMode) {
                    rsLogTo(id, 'cmd', `$ ${cmd}`);
                    const sRdp = getShell(id);
                    if (sRdp) {
                        sRdp.rdpClipboardCmd = cmd;
                        rshellSessions = [...rshellSessions]; // show clipboard strip
                    }
                    // lucyRunning reset is handled by the finally block below
                    return;
                }

                const aiExec = async () => {
                    // Tool card #1 — command execution
                    const execIcon = s.host.type === 'linux' ? '◈' : '⚡';
                    const execCard = rsPushToolCard(id, execIcon, `${s.host.type === 'linux' ? 'bash' : 'powershell'}: ${cmd.substring(0,80)}`, 'exec');
                    rsLogTo(id, 'cmd', `$ ${cmd}`);
                    try {
                        const out = await rsRunStreaming(id, cmd);
                        rsFinishToolCard(id, execCard, out || '(sin salida)', true);
                        if (out.trim()) {
                            // Tool card #2 — analysis
                            const analysisCard = rsPushToolCard(id, '◎', 'Analizando salida…', 'analyze');
                            try {
                                const analysis = await invoke('ask_lucy', {
                                    prompt: `[SYSTEM ANALYSIS — análisis detallado del resultado]\nHost: ${s.host.name} (${s.host.type === 'linux' ? 'Linux' : 'Windows'})\nComando ejecutado: \`${cmd.substring(0,300)}\`\nOutput completo:\n\`\`\`\n${out.substring(0,4000)}\n\`\`\`\n\nAnaliza el resultado detalladamente:\n1. ¿Se ejecutó correctamente?\n2. ¿Qué información relevante muestra?\n3. ¿Hay errores, advertencias o situaciones que requieran atención?\n4. Si aplica, sugiere el siguiente paso.\nNO uses <EXECUTE>. Responde en Markdown.`,
                                    context: '', userName: lucyConfig.name, runbooksDir: lucyConfig.runbooksDir || null,
                                    model: selectedModel || 'gemini-2.5-flash',
                                    images: null, lang: userLang, hostsJson: null
                                });
                                const cleanAnalysis = analysis.replace(/<[^>]*>/g,'').trim();
                                rsFinishToolCard(id, analysisCard, 'Análisis completado', true);
                                rsLogTo(id, 'lucy-out', cleanAnalysis);
                            } catch(e) {
                                rsFinishToolCard(id, analysisCard, String(e), false);
                            }
                        }
                    } catch(e) {
                        rsFinishToolCard(id, execCard, String(e), false);
                        rsLogTo(id, 'err', String(e));
                    }
                };
                await guardCheck(cmd, s.host.type, s.host.name, 'ai', aiExec);
            } else { rsLogTo(id, 'lucy-out', resp.replace(/<[^>]*>/g,'').trim()); }
        } catch(e) {
            addDebugLog('LUCY_ERROR', String(e), { shellId: id });
            rsLogTo(id, 'err', `Lucy error: ${e}`);
        }
        finally {
            // Always clear lucyRunning regardless of which path was taken
            // (normal exit, RDP early-return, guard-blocked, or thrown error)
            const s2 = getShell(id);
            if (s2 && s2.lucyRunning) {
                addDebugLog('LUCY_STATE', 'Setting lucyRunning=false in finally', { shellId: id });
                s2.lucyRunning = false;
                rshellSessions = [...rshellSessions];
            }
            rsScrollBottom(id);
        }
    }

    // ── Autocomplete suggestions ────────────────────────────────────────────
    const RS_SUGGESTIONS = {
        linux:   ['systemctl restart ','systemctl status ','journalctl -u ','journalctl -f','df -h','free -m','top -bn1','ps aux | grep ','tail -f ','cat /var/log/','netstat -tulnp','ss -tulnp','who','uptime','uname -a','ls -la','find / -name ','chmod +x ','scp ','apt install ','yum install ','dnf install '],
        windows: ['Get-Service ','Stop-Service ','Start-Service ','Restart-Service ','Get-EventLog -LogName System -Newest 20','Get-Process | Sort CPU -Desc | Select -First 10','Get-Disk','Get-NetAdapter','Get-NetIPAddress','Get-WindowsUpdate','Get-HotFix','Test-NetConnection ','Invoke-Command ','Get-ChildItem ','Remove-Item ','Copy-Item ','Get-Content ','Set-Content ','Get-WinEvent -LogName Security -MaxEvents 20']
    };

    function rsSuggestion(id) {
        const s = getShell(id);
        if (!s || !s.directIn) return '';
        const input = s.directIn.toLowerCase();
        const bootTools = s.bootstrap?.tools
            ? s.bootstrap.tools.split(',').filter(Boolean).map(t => t.trim() + ' ')
            : [];
        const staticList = RS_SUGGESTIONS[s.host.type] || [];
        const list = [...bootTools, ...staticList];
        return list.find(c => c.toLowerCase().startsWith(input) && c.toLowerCase() !== input) || '';
    }

    function rsAcceptSuggestion(e, id) {
        if (e.key !== 'Tab') return;
        const s = getShell(id);
        if (!s) return;
        const sugg = rsSuggestion(id) || s._aiSugg;
        if (!sugg) return;
        e.preventDefault();
        s.directIn = sugg;
        s._aiSugg = ''; s._aiSuggLoading = false;
        if (s._aiSuggTimer) { clearTimeout(s._aiSuggTimer); s._aiSuggTimer = null; }
        rshellSessions = [...rshellSessions];
    }

    // ── Persistent command history per host ────────────────────────────────
    function rsSaveHistory(hostId, cmd) {
        const key = `lucy_rsh_${hostId}`;
        const hist = safeParseLS(key, []);
        const filtered = hist.filter(c => c !== cmd);
        filtered.push(cmd);
        safeSetLS(key, filtered.slice(-100));
    }

    function rsGetHistory(hostId) {
        return safeParseLS(`lucy_rsh_${hostId}`, []);
    }

    // ── Persistent Lucy conversation per host ───────────────────────────────
    // Guarda solo las entradas Lucy (lucy-in, lucy-out, info) — no el output raw de comandos.
    const _LUCY_CONV_TYPES = new Set(['lucy-in', 'lucy-out', 'info']);
    const _LUCY_CONV_MAX   = 60;   // últimas N entradas lucy para no crecer indefinidamente

    function rsSaveLucyConv(hostId, history) {
        const entries = history
            .filter(e => _LUCY_CONV_TYPES.has(e.type))
            .slice(-_LUCY_CONV_MAX)
            .map(e => ({ type: e.type, text: e.text, time: e.time }));
        safeSetLS(`lucy_nxh_${hostId}`, entries);
    }

    function rsLoadLucyConv(hostId) {
        const arr = safeParseLS(`lucy_nxh_${hostId}`, []);
        if (!Array.isArray(arr)) return [];
        return arr.map(e => ({
            ...e,
            id: 'r-' + Math.random().toString(36).slice(2, 9),
            restored: true,   // marca visual opcional
        }));
    }

    function rsKeyDirect(e, id) {
        const s = getShell(id);
        if (!s) return;
        if (e.key === 'Tab') { rsAcceptSuggestion(e, id); return; }
        if (e.key === 'Enter' && e.ctrlKey) {
            e.preventDefault();
            const cmd = s.directIn.trim();
            if (cmd && !s.isStreaming) { rsSaveHistory(s.host.id, cmd); rsRunBackground(id, cmd); }
            return;
        }
        if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); if (!s.isStreaming) rsEnviarDirecto(id); return; }
        if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
            e.preventDefault();
            const hist = rsGetHistory(s.host.id);
            if (!hist.length) return;
            if (s._histIdx === undefined) s._histIdx = hist.length;
            s._histIdx = e.key === 'ArrowUp'
                ? Math.max(0, s._histIdx - 1)
                : Math.min(hist.length, s._histIdx + 1);
            s.directIn = s._histIdx === hist.length ? '' : hist[s._histIdx];
            rshellSessions = [...rshellSessions];
        } else {
            s._histIdx = undefined;
        }
    }

    function rsKeyLucy(e, id) {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            rsEnviarLucy(id);
            if (e.target) { e.target.style.height = 'auto'; }
        }
    }

    // ── AI ghost text ───────────────────────────────────────────────────────
    function rsHandleDirectInput(id) {
        const s = getShell(id);
        if (!s) return;
        if (s._aiSugg) { s._aiSugg = ''; }
        if (s._aiSuggTimer) { clearTimeout(s._aiSuggTimer); s._aiSuggTimer = null; }
        const input = s.directIn.trim();
        if (input.length >= 3 && !rsSuggestion(id)) {
            s._aiSuggTimer = setTimeout(() => rsAISuggest(id), 250); // ROI inmenso: Timeout reducido de 520 a 250ms para UX fluida Warp-style
        }
        rshellSessions = [...rshellSessions];
    }

    async function rsAISuggest(id) {
        const s = getShell(id);
        if (!s || s.isStreaming || s.running || !s.directIn.trim()) return;
        const partial = s.directIn.trim();
        if (partial.length < 3) return;
        s._aiSuggLoading = true;
        rshellSessions = [...rshellSessions];
        const b = s.bootstrap;
        const ctx = b
            ? `OS: ${b.os || s.host.type}, CWD: ${b.cwd || '/'}, shell: ${b.shell || 'bash'}, tools: ${b.tools || 'standard'}`
            : `type: ${s.host.type}`;
        try {
            const resp = await invoke('ask_lucy', {
                prompt: `[SHELL AUTOCOMPLETE — one line only]\nHost context: ${ctx}.\nComplete this partial command: \`${partial}\`\nRules: respond with ONLY the completed command. No explanation, no markdown, no backticks. Single line.`,
                context: '', userName: lucyConfig?.name || 'admin',
                // v1.7.0: shell autocomplete is throwaway, single-line —
                // CHEAP tier saves money without hurting quality.
                model: LLM.CHEAP,
                images: null, lang: 'en', hostsJson: null
            });
            const completion = resp.trim().replace(/^`+|`+$/g, '').split('\n')[0].trim();
            if (completion && completion.toLowerCase().startsWith(partial.toLowerCase())) {
                const sx = getShell(id);
                if (sx && sx.directIn.trim() === partial) {
                    sx._aiSugg = completion;
                    rshellSessions = [...rshellSessions];
                }
            }
        } catch { /* AI suggestion is optional */ }
        const sx = getShell(id);
        if (sx) { sx._aiSuggLoading = false; rshellSessions = [...rshellSessions]; }
    }

    // ── Background tasks ────────────────────────────────────────────────────
    async function rsRunBackground(shellId, cmd) {
        const s = getShell(shellId);
        if (!s || !cmd.trim()) return;
        const bgId = `bg_${shellId}_${Date.now()}`;
        const task = { id: bgId, cmd, startTime: Date.now(), streamOut: '', done: false, exitCode: null, durationMs: null };
        s.bgTasks = [...s.bgTasks, task];
        s.directIn = '';
        rshellSessions = [...rshellSessions];
        rsLogTo(shellId, 'info', `⏳ Background iniciado: ${cmd}`);

        const outUl  = await listen(`ssh-out-${bgId}`,  (e) => {
            const t = getShell(shellId)?.bgTasks?.find(t => t.id === bgId);
            if (t) { t.streamOut += String(e.payload); if (t.streamOut.length > 102400) t.streamOut = '…[truncado]\n' + t.streamOut.slice(-102400); rshellSessions = [...rshellSessions]; }
        });
        const errUl  = await listen(`ssh-err-${bgId}`,  (e) => {
            const t = getShell(shellId)?.bgTasks?.find(t => t.id === bgId);
            if (t) { t.streamOut += String(e.payload); if (t.streamOut.length > 102400) t.streamOut = '…[truncado]\n' + t.streamOut.slice(-102400); rshellSessions = [...rshellSessions]; }
        });
        const doneUl = await listen(`ssh-done-${bgId}`, (e) => {
            let exitCode = null, durationMs = null;
            try { const p = JSON.parse(e.payload); exitCode = p.exit_code; durationMs = p.duration_ms; } catch {}
            outUl(); errUl(); doneUl();
            const sx = getShell(shellId);
            const t  = sx?.bgTasks?.find(t => t.id === bgId);
            if (t) { t.done = true; t.exitCode = exitCode; t.durationMs = durationMs; }
            const dur = durationMs != null ? (durationMs / 1000).toFixed(1) + 's' : '';
            const ok  = exitCode === 0;
            rsLogTo(shellId, ok ? 'info' : 'err',
                `${ok ? '✓' : '✗'} Background completado: ${cmd.substring(0, 60)}${dur ? ' (' + dur + ')' : ''}`);
            if (t?.streamOut?.trim()) rsLogTo(shellId, 'out', t.streamOut.trim(), { exitCode, durationMs });
            toast(`${ok ? '✓' : '!'} BG: ${cmd.substring(0, 35)}${dur ? ' · ' + dur : ''}`, ok ? 'info' : 'warn');
            if (sx) { sx.bgTasks = sx.bgTasks.filter(bt => bt.id !== bgId); rshellSessions = [...rshellSessions]; }
        });

        invoke('stream_shell_cmd', {
            sessionId: bgId,
            host: s.host.host, username: s.host.username, command: cmd,
            hostType: s.host.type,
            port: s.host.port || (s.host.type === 'linux' ? 22 : 5985),
            password: s.host.password || null, keyPath: s.host.sshKeyPath || null
        }).catch(err => {
            outUl(); errUl(); doneUl();
            rsLogTo(shellId, 'err', `BG error: ${err}`);
            const sx = getShell(shellId);
            if (sx) { sx.bgTasks = sx.bgTasks.filter(bt => bt.id !== bgId); rshellSessions = [...rshellSessions]; }
        });
    }

    // ── Broadcast ───────────────────────────────────────────────────────────
    function abrirBroadcast(shellId) {
        broadcastShellId  = shellId;
        broadcastCmd      = '';
        broadcastSelected = new Set(hosts.filter(h => h.id !== null).map(h => h.id).slice(0, 3));
        broadcastResults  = [];
        broadcastRunning  = false;
        showBroadcast     = true;
    }

    async function runBroadcast() {
        if (!broadcastCmd.trim() || broadcastSelected.size === 0 || broadcastRunning) return;
        // Guard check for broadcast — uses first target's type for analysis
        const firstTarget = hosts.find(h => broadcastSelected.has(h.id));
        const bcExec = async () => { await _runBroadcastInner(); };
        await guardCheck(broadcastCmd, firstTarget?.type || 'linux', `${broadcastSelected.size} hosts`, 'broadcast', bcExec);
    }

    async function _runBroadcastInner() {
        broadcastRunning = true;
        broadcastResults = [];
        const targets = hosts.filter(h => broadcastSelected.has(h.id));
        const jobs = targets.map(async (h) => {
            const pwd = await invoke('get_host_credential', { hostId: h.id }).catch(() => '');
            const t0  = Date.now();
            try {
                const out = await invoke('execute_shell_cmd', {
                    host: h.host, username: h.username, command: broadcastCmd,
                    hostType: h.type, port: h.port || (h.type === 'linux' ? 22 : 5985),
                    password: pwd || null, keyPath: h.sshKeyPath || null
                });
                return { hostName: h.name, hostType: h.type, output: out.trim(), exitCode: 0, durationMs: Date.now() - t0, error: null };
            } catch(e) {
                return { hostName: h.name, hostType: h.type, output: '', exitCode: 1, durationMs: Date.now() - t0, error: String(e) };
            }
        });
        broadcastResults = await Promise.all(jobs);
        broadcastRunning = false;
        const s = getShell(broadcastShellId);
        if (s) {
            const ok = broadcastResults.filter(r => r.exitCode === 0).length;
            rsLogTo(broadcastShellId, 'info',
                `◉ Broadcast completado: ${ok}/${broadcastResults.length} hosts OK — "${broadcastCmd.substring(0,50)}"`);
            // Incident Mode: log each host result as evidence
            if (s.incidentId) {
                for (const r of broadcastResults) {
                    const tags = ['broadcast', r.hostName, r.exitCode === 0 ? 'ok' : 'error'];
                    captureEvidenceIfIncident(
                        broadcastShellId,
                        `broadcast:${r.hostName}:${broadcastCmd.substring(0,80)}`,
                        r.error ? `ERROR: ${r.error}\n${r.output}` : r.output,
                        tags
                    );
                }
            }
        }
    }

    // ── Playbooks ───────────────────────────────────────────────────────────
    function rsGetPlaybooks(hostId) {
        return safeParseLS(`lucy_pb_${hostId}`, []);
    }

    function rsSavePlaybook(hostId, pb) {
        const pbs = rsGetPlaybooks(hostId).filter(p => p.id !== pb.id);
        pbs.push(pb);
        safeSetLS(`lucy_pb_${hostId}`, pbs);
    }

    function rsDeletePlaybook(hostId, pbId) {
        const pbs = rsGetPlaybooks(hostId).filter(p => p.id !== pbId);
        // localStorage can throw on quota-exceeded or in private-browsing mode —
        // wrap defensively so deleting a playbook never crashes the panel.
        try {
            localStorage.setItem(`lucy_pb_${hostId}`, JSON.stringify(pbs));
        } catch (e) {
            console.warn('[playbooks] localStorage write failed:', e);
        }
        rshellSessions = [...rshellSessions];
    }

    async function rsRunPlaybook(shellId, pb) {
        showPlaybookModal = false;
        const cmds = pb.commands.filter(c => c.trim());
        rsLogTo(shellId, 'info', `▶ Playbook: ${pb.name} (${cmds.length} comandos)`);
        for (const cmd of cmds) {
            await rsEnviarDirectoCmd(shellId, cmd);
        }
    }

    async function rsEnviarDirectoCmd(id, cmd) {
        const s = getShell(id);
        if (!s) return;
        return new Promise(async (resolve) => {
            await guardCheck(cmd, s.host.type, s.host.name, 'playbook', async () => {
                rsLogTo(id, 'cmd', `$ ${cmd}`);
                rsSaveHistory(s.host.id, cmd);
                await rsRunStreaming(id, cmd);
                resolve();
            });
        });
    }

    function rsGuardarPlaybook() {
        const s = getShell(playbookShellId);
        if (!s || !pbForm.name.trim()) return;
        const pb = {
            id: Date.now().toString(),
            name: pbForm.name.trim(),
            hostId: s.host.id,
            commands: pbForm.commands.split('\n').map(c => c.trim()).filter(Boolean)
        };
        rsSavePlaybook(s.host.id, pb);
        showPlaybookModal = false;
        pbForm = { name: '', commands: '' };
        rshellSessions = [...rshellSessions];
    }

    // ── File transfer ───────────────────────────────────────────────────────
    async function rsPickFile() {
        try {
            const path = await invoke('pick_file_path');
            if (path && typeof path === 'string' && path.trim()) {
                ftLocalPath = path;
            }
        } catch(e) {
            console.error('pick_file_path error:', e);
        }
    }

    async function rsEjecutarTransferencia() {
        const s = getShell(ftShellId);
        if (!s || !ftLocalPath || !ftRemotePath) return;
        ftRunning = true; ftResult = '';
        try {
            let cmd = '';
            if (s.host.type === 'linux') {
                if (ftDirection === 'upload') {
                    cmd = `scp -P ${s.host.port||22} "${ftLocalPath}" ${s.host.username}@${s.host.host}:"${ftRemotePath}"`;
                } else {
                    cmd = `scp -P ${s.host.port||22} ${s.host.username}@${s.host.host}:"${ftRemotePath}" "${ftLocalPath}"`;
                }
                const out = await invoke('execute_powershell', { script: cmd });
                ftResult = `✓ ${ftDirection === 'upload' ? 'Subido' : 'Descargado'} correctamente`;
                rsLogTo(ftShellId, 'info', `⊞ ${isEN ? 'Transfer complete' : 'Transferencia completada'}: ${ftLocalPath} ↔ ${s.host.host}:${ftRemotePath}`);
            } else {
                const ps = ftDirection === 'upload'
                    ? `Copy-Item -Path "${ftLocalPath}" -Destination "${ftRemotePath}" -ToSession (New-PSSession -ComputerName ${s.host.host})`
                    : `Copy-Item -Path "${ftRemotePath}" -Destination "${ftLocalPath}" -FromSession (New-PSSession -ComputerName ${s.host.host})`;
                await invoke('execute_powershell', { script: ps });
                ftResult = `✓ ${isEN ? 'Transfer complete' : 'Transferencia completada'}`;
                rsLogTo(ftShellId, 'info', `⊞ ${isEN ? 'Transfer complete' : 'Transferencia completada'}`);
            }
        } catch(e) {
            ftResult = `✗ Error: ${String(e).substring(0,200)}`;
        }
        ftRunning = false;
    }

    // ── Tail -f ─────────────────────────────────────────────────────────────
    function rsIniciarTail(shellId, logPath) {
        const key = `${shellId}_${logPath}`;
        if (tailIntervals[key]) return;
        const s = getShell(shellId);
        if (!s) return;
        rsLogTo(shellId, 'info', `◉ Iniciando tail: ${logPath}`);
        showTailModal = false;
        let lastLines = '';
        // Use recursive setTimeout instead of setInterval to prevent stacking
        // when the remote command takes longer than the poll interval.
        const poll = async () => {
            try {
                const cmd = s.host.type === 'linux'
                    ? `tail -n 20 "${logPath}"`
                    : `Get-Content "${logPath}" -Tail 20`;
                const out = await invoke('execute_shell_cmd', {
                    host: s.host.host, username: s.host.username,
                    command: cmd, hostType: s.host.type,
                    port: s.host.port || (s.host.type === 'linux' ? 22 : 5985),
                    password: s.host.password || null, keyPath: s.host.sshKeyPath||null
                });
                if (out !== lastLines) {
                    const newLines = out.replace(lastLines, '').trim();
                    if (newLines) rsLogTo(shellId, 'out', newLines);
                    lastLines = out;
                }
            } catch(e) { rsDetenerTail(shellId, logPath); return; }
            // Schedule next poll only after current one completes
            if (tailIntervals[key]) tailIntervals[key] = setTimeout(poll, 3000);
        };
        const interval = setTimeout(poll, 3000);
        tailIntervals = { ...tailIntervals, [key]: interval };
        rshellSessions = [...rshellSessions];
    }

    function rsDetenerTail(shellId, logPath) {
        const key = `${shellId}_${logPath}`;
        if (tailIntervals[key]) {
            clearTimeout(tailIntervals[key]);
            const { [key]: _, ...rest } = tailIntervals;
            tailIntervals = rest;
            rsLogTo(shellId, 'info', `⏹ Tail detenido: ${logPath}`);
        }
    }

    function rsTailActivo(shellId) {
        return Object.keys(tailIntervals).some(k => k.startsWith(shellId));
    }

    function rsDetenerTodosTails(shellId) {
        Object.keys(tailIntervals).filter(k => k.startsWith(shellId)).forEach(k => {
            clearTimeout(tailIntervals[k]);
        });
        tailIntervals = Object.fromEntries(Object.entries(tailIntervals).filter(([k]) => !k.startsWith(shellId)));
    }

    // ── Dispatch host modal open to parent ──────────────────────────────────
    function abrirHostModal(host = null) {
        dispatch('openHostModal', { host });
    }

    // ── RDP Computer-Use Agent ──────────────────────────────────────────────

    let rdpAgentUnlisten = null;
    let _componentDestroyed = false;

    // Start listening for agent events — use IIFE to avoid top-level await race.
    // If the component is destroyed BEFORE listen() resolves, we still call the
    // unlisten fn (otherwise the handler keeps firing on a dead component and
    // leaks memory through closure references to rshellSessions).
    (async () => {
        const fn = await listen('rdp_agent_step', (event) => {
            const { hwnd, kind, data, detail } = event.payload;
            const s = rshellSessions.find(s => s.rdpAgentHwnd == hwnd);
            if (!s) return;

            const entry = { kind, data: kind === 'screenshot' ? '' : data, detail, ts: new Date() };
            s.rdpAgentLog = [...s.rdpAgentLog, entry];

            if (kind === 'screenshot' && data) {
                s.rdpAgentScreenshot = data;
            }
            if (kind === 'done' || kind === 'error') {
                s.rdpAgentRunning = false;
                if (kind === 'done') rsLogTo(s.id, 'lucy-out', `[Agent] Agente completó tarea: ${detail}`);
                else                 rsLogTo(s.id, 'err',      `[Agent] Agente error: ${detail}`);
            }
            rshellSessions = [...rshellSessions];
        });
        // If component was destroyed while awaiting, unlisten immediately
        if (_componentDestroyed) { fn(); } else { rdpAgentUnlisten = fn; }
    })();

    async function checkProviderHealthRdp(shellId) {
        const s = getShell(shellId);
        if (!s) return;

        try {
            const result = await invoke('check_provider_health', { provider: s.rdpAgentProvider });
            s.rdpAgentProviderStatus = result.status;
            rshellSessions = [...rshellSessions];

            const statusMsg = result.status === 'ok'
                ? (isEN ? 'Provider healthy' : 'Proveedor operativo')
                : (isEN ? `Provider error: ${result.message}` : `Error del proveedor: ${result.message}`);
            rsLogTo(shellId, result.status === 'ok' ? 'info' : 'err', `[Health] ${statusMsg}`);
        } catch (e) {
            s.rdpAgentProviderStatus = 'error';
            rshellSessions = [...rshellSessions];
            rsLogTo(shellId, 'err', `[Health] ${isEN ? 'Health check failed' : 'Verificación de salud fallida'}: ${e}`);
        }
    }

    function rsClearHistory(shellId) {
        const s = getShell(shellId);
        if (!s) return;
        s.history = [];
        rshellSessions = [...rshellSessions];
        toast(isEN ? 'Terminal cleared' : 'Terminal limpiada', 'success');
    }

    // ── Incident Response / SRE Mode helpers ─────────────────────────────
    //
    // Lifecycle: user clicks the Siren button with a short title + optional
    // description → backend creates a row in 'incidents' → we store the id
    // in the shell state and open the panel. From that point, every
    // successful command execution on this shell is auto-captured as
    // evidence. Phase transitions are driven from the IncidentPanel UI.
    //
    // Ending an incident happens either by the user (panel abandon button)
    // or by the LLM calling finalize during REPORT → DONE transition.

    // Two-step in-app prompt flow that replaces the chained window.prompt() calls
    let incidentPrompt = null;   // {step:'title'|'description', shellId, title?} | null

    function startIncidentMode(shellId) {
        const s = getShell(shellId);
        if (!s) return;
        if (s.incidentId) {
            toast(isEN ? 'Incident already active' : 'Ya hay un incidente activo', 'info');
            s.incidentPanelOpen = true;
            rshellSessions = [...rshellSessions];
            return;
        }
        incidentPrompt = { step: 'title', shellId };
    }

    function onIncidentPromptSubmit(value) {
        if (!incidentPrompt) return;
        if (incidentPrompt.step === 'title') {
            const title = (value || '').trim();
            if (!title) { incidentPrompt = null; return; }
            // advance to description step
            incidentPrompt = { step: 'description', shellId: incidentPrompt.shellId, title };
        } else {
            const description = (value || '').trim();
            const { shellId, title } = incidentPrompt;
            incidentPrompt = null;
            void doStartIncident(shellId, title, description);
        }
    }

    async function doStartIncident(shellId, title, description) {
        const s = getShell(shellId);
        if (!s) return;
        try {
            const incident = await invoke('incident_start', {
                args: {
                    shell_id: shellId,
                    host_name: s.host?.name || 'local',
                    title,
                    description,
                    max_loops: 5,
                }
            });
            s.incidentId = incident.id;
            s.incidentPhase = incident.phase;
            s.incidentPanelOpen = true;
            rshellSessions = [...rshellSessions];
            rsLogTo(shellId, 'info', `🚨 ${isEN ? 'Incident started' : 'Incidente iniciado'}: "${incident.title}" [${incident.id.slice(0,8)}]`);
        } catch (e) {
            toast(String(e), 'error');
        }
    }

    function toggleIncidentPanel(shellId) {
        const s = getShell(shellId);
        if (!s) return;
        s.incidentPanelOpen = !s.incidentPanelOpen;
        rshellSessions = [...rshellSessions];
    }

    /// Capture a command execution as evidence tied to the active incident.
    /// Safe to call unconditionally — it no-ops when no incident is active.
    /// Keeps evidence capture decoupled from UI flow; callers don't need to
    /// know anything about incident mode.
    async function captureEvidenceIfIncident(shellId, source, content, tags = []) {
        const s = getShell(shellId);
        if (!s || !s.incidentId) return;
        try {
            await invoke('incident_add_evidence', {
                args: {
                    incident_id: s.incidentId,
                    kind: 'command_output',
                    source,
                    content: typeof content === 'string' ? content : JSON.stringify(content),
                    tags,
                }
            });
        } catch (e) {
            // Non-fatal — we don't want evidence logging to break the shell.
            console.warn('[incident] capture failed:', e);
        }
    }

    // Expose on window for legacy call sites (old modules that don't have
    // direct access to the component scope). Svelte reactivity still works
    // because we read from live 'rshellSessions' each call.
    if (typeof window !== 'undefined') {
        window.__lucy_capture_evidence = captureEvidenceIfIncident;
    }

    function handleIncidentClosed(shellId) {
        const s = getShell(shellId);
        if (!s) return;
        s.incidentId = null;
        s.incidentPhase = null;
        s.incidentPanelOpen = false;
        rshellSessions = [...rshellSessions];
        rsLogTo(shellId, 'info', `✓ ${isEN ? 'Incident closed' : 'Incidente cerrado'}`);
    }

    function handleIncidentPhaseChanged(shellId, ev) {
        const s = getShell(shellId);
        if (!s) return;
        s.incidentPhase = ev.detail?.phase || s.incidentPhase;
        rshellSessions = [...rshellSessions];
    }

    async function startRdpAgent(shellId) {
        const s = getShell(shellId);
        if (!s || !s.rdpAgentTask?.trim() || s.rdpAgentRunning) return;

        // Find the mstsc window matching this host
        let windows = [];
        try { windows = await invoke('find_rdp_windows'); } catch(e) {}

        const win = windows.find(w =>
                w.title.toLowerCase().includes(s.host.host?.toLowerCase()) ||
                w.title.toLowerCase().includes(s.host.name?.toLowerCase()))
            || windows[0]; // fallback: first mstsc window

        if (!win) {
            rsLogTo(shellId, 'err', '[Error] No se encontró ninguna ventana mstsc.exe activa. Abre la sesión RDP primero.');
            return;
        }

        s.rdpAgentRunning  = true;
        s.rdpAgentHwnd     = win.hwnd;
        s.rdpAgentLog      = [];
        s.rdpAgentScreenshot = null;
        rshellSessions = [...rshellSessions];
        rsLogTo(shellId, 'info', `[Agent] Agente GUI iniciado — proveedor: ${s.rdpAgentProvider} / modelo: ${s.rdpAgentModel} — tarea: "${s.rdpAgentTask}"`);

        invoke('run_rdp_agent', {
            hwnd:     win.hwnd,
            task:     s.rdpAgentTask,
            model:    s.rdpAgentModel,
            maxSteps: 20,
        }).catch(e => {
            rsLogTo(shellId, 'err', `[Agent] Agente error: ${e}`);
            const sx = getShell(shellId);
            if (sx) { sx.rdpAgentRunning = false; rshellSessions = [...rshellSessions]; }
        });
    }

    function stopRdpAgent(shellId) {
        const s = getShell(shellId);
        if (!s) return;
        s.rdpAgentRunning = false;
        rshellSessions = [...rshellSessions];
        rsLogTo(shellId, 'info', '[Agent] Agente detenido por el usuario.');
    }

    // ── Cleanup on destroy ──────────────────────────────────────────────────
    onDestroy(() => {
        _componentDestroyed = true;
        Object.values(tailIntervals).forEach(id => clearTimeout(id));
        if (rdpAgentUnlisten) rdpAgentUnlisten();
    });
</script>

<!-- ══════════════════════════════════════════════════════════════════════════ -->
<!-- NexShell View Template                                                     -->
<!-- ══════════════════════════════════════════════════════════════════════════ -->

<div class="view-wrap ns-view">

  <!-- ── HEADER ── -->
  <div class="view-hdr ns-hdr">
    <div class="ns-hdr-left">
      <span class="view-title">⊟ NexShell</span>
      {#if rshellSessions.length > 0}
        <span class="ns-summary-badge">{rshellSessions.filter(s=>s.connected).length}/{rshellSessions.length} sesión{rshellSessions.length!==1?'es':''}</span>
      {/if}
    </div>
    <div style="display:flex;align-items:center;gap:8px;">
      <button class="ns-panel-toggle" on:click={() => nsHostsCollapsed = !nsHostsCollapsed}
        title={nsHostsCollapsed ? (isEN ? 'Expand hosts panel' : 'Mostrar panel de hosts') : (isEN ? 'Collapse hosts panel' : 'Colapsar panel de hosts')}>
        {nsHostsCollapsed ? (isEN ? '▶ Hosts' : '▶ Hosts') : (isEN ? '◀ Collapse' : '◀ Colapsar')}
      </button>
      <button class="ns-add-btn" on:click={() => abrirHostModal()}>{isEN ? '+ Add host' : '+ Añadir host'}</button>
      <button class="ns-guard-btn" class:active={$guardConfig.enabled}
        on:click={() => { $guardConfig = { ...$guardConfig, enabled: !$guardConfig.enabled }; }}
        title={$guardConfig.enabled
          ? (isEN ? 'Command Guard: ON (click to disable)' : 'Guardia: ACTIVO (clic para desactivar)')
          : (isEN ? 'Command Guard: OFF (click to enable)' : 'Guardia: INACTIVO (clic para activar)')}>
        ⬡{$guardConfig.enabled ? '' : ' OFF'}
      </button>
    </div>
  </div>

  <!-- ── BODY ── -->
  <div class="ns-body {nsHostsCollapsed ? 'ns-body-full' : ''}">

    <!-- ── LEFT: collapsible host catalogue ── -->
    {#if !nsHostsCollapsed}
    <div class="ns-hosts-col">

      <!-- Search + Sort toolbar -->
      <div class="ns-col-toolbar">
        <input class="ns-search" placeholder={isEN ? 'Search host…' : 'Buscar host…'} bind:value={nexshellFilter}/>
        <select class="ns-sort-sel" bind:value={nsSort} title={isEN ? 'Sort hosts' : 'Ordenar hosts'}>
          <option value="status">⬤ {isEN ? 'Status' : 'Estado'}</option>
          <option value="name">A–Z {isEN ? 'Name' : 'Nombre'}</option>
          <option value="type">⊞ {isEN ? 'Type' : 'Tipo'}</option>
          <option value="activity">⏱ {isEN ? 'Activity' : 'Actividad'}</option>
        </select>
      </div>

      <!-- Category filter chips -->
      <div class="ns-cat-chips">
        {#each [['all','Todos'],['shell','Shell'],['database','DB'],['container','Docker'],['kubernetes','K8s'],['network','Red']] as [v,l]}
          <button class="ns-cat-chip {nsCategoryFilter===v?'ns-cat-active':''}"
            on:click={() => nsCategoryFilter = v}>{l}</button>
        {/each}
      </div>

      <div class="ns-col-lbl">{isEN ? 'CONFIGURED HOSTS' : 'HOSTS CONFIGURADOS'} <span class="ns-col-count">{nsHostsSorted.length}</span></div>

      {#each nsHostsSorted as h, i (h.id)}
        {@const sess = rshellSessions.find(s => s.host.id === h.id)}
        {@const isActive = sess?.id === activeShellId}
        <!-- in:staggerIn runs ONLY on mount — re-renders won't replay it,
             so cards never disappear when sessions update. -->
        <div class="ns-host-card {sess ? (sess.connected ? 'ns-card-on' : 'ns-card-connecting') : ''} {isActive ? 'ns-card-focused' : ''}"
          in:staggerIn={{ index: i, step: 32 }}
          role="button" tabindex="0"
          on:click={() => { if(sess) activeShellId = sess.id; }}
          on:keydown={(e) => e.key==='Enter' && sess && (activeShellId = sess.id)}>
          <div class="ns-card-top">
            <span class="ns-card-ico"><svelte:component this={getHostTypeComponent(h.type)} size={20}/></span>
            <div class="ns-card-info">
              <span class="ns-card-name">{h.name}</span>
              <span class="ns-card-addr">{h.host}:{h.port||(h.type==='windows'?5985:22)}</span>
            </div>
            <span class="ns-proto-badge ns-cat-badge-{h.category||'shell'}">{nsProtoLabel(h)}</span>
          </div>
          <div class="ns-card-meta">
            <span class="ns-card-user">◈ {h.username}</span>
            {#if h.color && h.color !== '#10b981'}<span class="ns-color-dot" style="background:{h.color};"></span>{/if}
            {#if sess}
              <span class="ns-conn-pill {sess.connected ? 'ns-conn-ok' : 'ns-conn-wait'}">{sess.connected ? '● Conectado' : '⟳ Conectando…'}</span>
              {#if sess._rec}<span class="ns-rec-badge" title={isEN ? 'Recording active' : 'Grabando'}>● REC</span>{/if}
            {:else if h.lastActivity}
              <span class="ns-activity-ts">{nsRelTime(h.lastActivity)}</span>
            {/if}
          </div>
          {#if sess?.bootstrap}
            <div class="ns-card-env">
              {#if sess.bootstrap.os}<span class="ns-env-tag">⊡ {sess.bootstrap.os.split(' ').slice(0,2).join(' ')}</span>{/if}
              {#if sess.bootstrap.git_branch}<span class="ns-env-tag">⊕ {sess.bootstrap.git_branch}{sess.bootstrap.git_dirty?'*':''}</span>{/if}
              {#if sess.bootstrap.docker}<span class="ns-env-tag">⊟</span>{/if}
              {#if sess.bootstrap.k8s_ctx}<span class="ns-env-tag">⎈ {sess.bootstrap.k8s_ctx}</span>{/if}
            </div>
          {/if}
          <div class="ns-card-actions">
            {#if sess}
              <button class="ns-act-btn ns-act-open" on:click|stopPropagation={() => activeShellId = sess.id}><ArrowUp size={11}/> Ver</button>
              <!-- Tier S #3 — Toggle recording on/off for this shell session.
                   Only meaningful for non-RDP sessions (RDP is clipboard-mode,
                   the actual command stream isn't observed). -->
              {#if !sess.rdpMode}
                <button class="ns-act-btn"
                        class:ns-act-recording={sess._rec}
                        on:click|stopPropagation={() => sess._rec ? rsRecordingStop(sess.id) : rsRecordingStart(sess.id)}
                        title={sess._rec
                            ? (isEN ? 'Stop recording' : 'Detener grabación')
                            : (isEN ? 'Start recording' : 'Iniciar grabación')}>
                    {sess._rec ? '■ REC' : '● REC'}
                </button>
              {/if}
              <!-- NS-6 (Sprint 4) — Manual reconnect when the session is dead.
                   Pairs with NS-4's auto-retry on initial connect: that covered
                   the "didn't connect the first time" path. This covers "lost
                   the connection mid-session" (e.g. laptop sleep, VPN drop)
                   where automatic recovery isn't safe and the user wants a
                   one-click resurrection. We tear down the dead session first
                   so abrirRShell creates a fresh one. -->
              {#if !sess.connected && !sess.running && !sess.isStreaming && !sess.rdpMode}
                <button class="ns-act-btn ns-act-connect" on:click|stopPropagation={() => {
                    rsDetenerTodosTails(sess.id);
                    cerrarShell(sess.id);
                    abrirRShell(h);
                  }}><Zap size={11}/> {isEN ? 'Reconnect' : 'Reconectar'}</button>
              {/if}
              <button class="ns-act-btn ns-act-close" on:click|stopPropagation={() => { rsDetenerTodosTails(sess.id); cerrarShell(sess.id); }}><X size={12}/></button>
            {:else}
              <button class="ns-act-btn ns-act-connect" on:click|stopPropagation={() => abrirRShell(h)}><Zap size={11}/> {isEN ? 'Connect' : 'Conectar'}</button>
            {/if}
            <button class="ns-act-btn ns-act-edit" on:click|stopPropagation={() => abrirHostModal(h)}><Edit2 size={11}/></button>
            <!-- Tier S #3 — Open the recording player scoped to this host -->
            <button class="ns-act-btn ns-act-play"
                    on:click|stopPropagation={() => { recPlayerHostId = h.id; recPlayerOpenId = null; showRecPlayer = true; }}
                    title={isEN ? 'Open recordings for this host' : 'Ver grabaciones de este host'}>►</button>
          </div>
        </div>
      {/each}

      {#if !hosts.length}
        <div class="ns-empty-hosts">
          <span style="font-size:32px;">⊟</span>
          <p>{isEN ? 'No hosts configured' : 'Sin hosts configurados'}</p>
          <button class="ns-add-btn" on:click={() => abrirHostModal()}>{isEN ? '+ Add first host' : '+ Añadir primer host'}</button>
        </div>
      {/if}

    </div><!-- /ns-hosts-col -->
    {/if}

    <!-- ── RIGHT: session workspace ── -->
    <div class="ns-workspace">

      {#if rshellSessions.length === 0}
        <!-- Welcome screen -->
        <div class="ns-welcome">
          <div class="ns-welcome-ico" style="animation: host-hover 3s ease-in-out infinite;">
            <Rocket size={48} color="var(--acc)"/>
          </div>
          <h3 class="ns-welcome-title">NexShell — {isEN ? 'Smart Shell' : 'Shell Inteligente'}</h3>
          <p class="ns-welcome-sub">{isEN ? 'Connect to a host to start. Lucy co-pilots every session.' : 'Conecta a un host para comenzar. Lucy co-pilota cada sesión.'}</p>
          <div class="ns-caps-grid">
            <div class="ns-cap-item"><span style="color:var(--txt2);"><Hash size={18}/></span><span>{isEN ? 'Exit code + duration per command' : 'Exit code + duración por comando'}</span></div>
            <div class="ns-cap-item"><span style="color:var(--txt2);"><GitBranch size={18}/></span><span>{isEN ? 'Git, K8s, Docker context on connect' : 'Contexto Git, K8s, Docker al conectar'}</span></div>
            <div class="ns-cap-item"><span style="color:var(--txt2);"><Sparkles size={18}/></span><span>{isEN ? 'Real-time AI autocomplete' : 'Autocompletado IA en tiempo real'}</span></div>
            <div class="ns-cap-item"><span style="color:var(--txt2);"><Mic size={18}/></span><span>{isEN ? 'Natural language commands' : 'Órdenes en lenguaje natural'}</span></div>
            <div class="ns-cap-item"><span style="color:var(--txt2);"><Timer size={18}/></span><span>{isEN ? 'Background tasks (Ctrl+Enter)' : 'Tareas en background (Ctrl+Enter)'}</span></div>
            <div class="ns-cap-item"><span style="color:var(--txt2);"><Radio size={18}/></span><span>{isEN ? 'Simultaneous multi-host broadcast' : 'Broadcast multi-host simultáneo'}</span></div>
            <div class="ns-cap-item"><span style="color:var(--txt2);"><Globe size={18}/></span><span>{isEN ? 'Reads URL docs in context' : 'Lee documentación de URLs en contexto'}</span></div>
            <div class="ns-cap-item"><span style="color:var(--txt2);"><BookMarked size={18}/></span><span>{isEN ? 'Playbooks: script sequences' : 'Playbooks: secuencias de comandos'}</span></div>
            <div class="ns-cap-item"><span style="color:var(--txt2);"><FolderSync size={18}/></span><span>{isEN ? 'File transfer' : 'Transferencia de archivos'}</span></div>
            <div class="ns-cap-item"><span style="color:var(--txt2);"><Activity size={18}/></span><span>{isEN ? 'Real-time log tailing' : 'Log tail en tiempo real'}</span></div>
          </div>
        </div>

      {:else}
        <!-- Session tab bar -->
        <div class="ns-session-tabs">
          {#each rshellSessions as s (s.id)}
            <div class="ns-stab {s.id === activeShellId ? 'ns-stab-active' : ''}"
              role="tab" tabindex="0" aria-selected={s.id === activeShellId}
              on:click={() => activeShellId = s.id}
              on:keydown={(e) => e.key === 'Enter' && (activeShellId = s.id)}>
              <span class="ns-stab-ico"><svelte:component this={getHostTypeComponent(s.host.type)} size={16}/></span>
              <span class="ns-stab-name">{s.host.name}</span>
              <span class="ns-stab-dot {s.connected?'ok':'wait'}">●</span>
              {#if s.running||s.lucyRunning}<span class="ns-stab-spin">◌</span>{/if}
              <button class="ns-stab-close" on:click|stopPropagation={() => { rsDetenerTodosTails(s.id); cerrarShell(s.id); }} title="{isEN ? 'Close session' : 'Cerrar sesión'}"><X size={11}/></button>
            </div>
          {/each}
        </div>

        <!-- Active session shell content -->
        {#each rshellSessions.filter(s => s.id === activeShellId) as s (s.id)}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions
             Ctrl+F search shortcut MUST be captured at the wrapper level to
             work regardless of which inner element has focus. The inner
             search panel + terminal already provide proper interactive UI. -->
        <div class="ns-shell-wrap" role="region" aria-label="NexShell session"
          on:keydown={(e) => { if (e.ctrlKey && e.key === 'f') { e.preventDefault(); nsSearchOpen(s.id); } }}>

          <!-- Shell header -->
          <div class="ns-shell-hdr">
            <div class="rshell-hdr-left">
              <span class="rshell-ico"><svelte:component this={getHostTypeComponent(s.host.type)} size={24}/></span>
              <div>
                <div class="rshell-title">{s.host.name}</div>
                <div class="rshell-sub">{s.host.username}@{s.host.host}:{s.host.port||3389}
                  {#if s.rdpMode}
                    <span class="rshell-badge rdp">⊡ RDP Copilot</span>
                  {:else if s.connected}
                    <span class="rshell-badge ok">● Conectado</span>
                  {:else}
                    <span class="rshell-badge err">● Sin conexión</span>
                  {/if}
                  {#if s.bootstrap}
                    {#if s.bootstrap.git_branch}<span class="rs-ctx-badge ctx-git">⊕ {s.bootstrap.git_branch}{s.bootstrap.git_dirty?'*':''}</span>{/if}
                    {#if s.bootstrap.k8s_ctx}<span class="rs-ctx-badge ctx-k8s">⎈ {s.bootstrap.k8s_ctx}</span>{/if}
                    {#if s.bootstrap.docker}<span class="rs-ctx-badge ctx-docker">⊟</span>{/if}
                    {#if s.bootstrap.node_ver}<span class="rs-ctx-badge ctx-node">⬡ {s.bootstrap.node_ver}</span>{/if}
                    {#if s.bootstrap.python_venv}<span class="rs-ctx-badge ctx-venv">◈ {s.bootstrap.python_venv}</span>{/if}
                  {:else if s.connected}<span class="rs-ctx-badge ctx-loading">⟳ analizando…</span>{/if}
                </div>
              </div>
            </div>
            <div style="display:flex;gap:5px;align-items:center;">
              <!-- ── Group 1: Terminal view (search, clear, logs) ───────────── -->
              <button class="rshell-feat-btn" title="{isEN ? 'Search output (Ctrl+F)' : 'Buscar en salida (Ctrl+F)'}"
                on:click={() => nsSearchOpen(s.id)}>⌕</button>
              <button class="rshell-feat-btn rs-feat-danger" title="{isEN ? 'Clear terminal' : 'Limpiar terminal'}"
                on:click={() => rsClearHistory(s.id)}><Trash2 size={13}/></button>
              <button class="rshell-feat-btn" title="{isEN ? 'Download debug logs' : 'Descargar logs de depuración'}"
                on:click={() => downloadDebugLogs()}><FileText size={13}/></button>

              <span class="rshell-toolbar-sep"></span>

              <!-- ── Incident Mode (SRE) — structured troubleshooting session ── -->
              <button class="rshell-feat-btn {s.incidentId ? 'rs-feat-incident-active' : ''}"
                title="{s.incidentId
                    ? (isEN ? 'Toggle incident panel (session active)' : 'Abrir/cerrar panel de incidente (sesión activa)')
                    : (isEN ? 'Start Incident Mode — structured troubleshooting' : 'Iniciar Modo Incidente — troubleshooting estructurado')}"
                on:click={() => s.incidentId ? toggleIncidentPanel(s.id) : startIncidentMode(s.id)}>
                <Siren size={13}/>
              </button>

              <span class="rshell-toolbar-sep"></span>

              <!-- ── Group 2: Remote operations (playbooks, files, tail, broadcast) ── -->
              <button class="rshell-feat-btn" title="Playbooks"
                on:click={() => { playbookShellId=s.id; pbForm={name:'',commands:''}; showPlaybookModal=true; }}><BookOpen size={13}/></button>
              <button class="rshell-feat-btn" title="{isEN ? 'File transfer' : 'Transferir archivos'}"
                on:click={() => { ftShellId=s.id; ftDirection='upload'; ftLocalPath=''; ftRemotePath=''; ftResult=''; showFileTransferModal=true; }}><FolderSync size={13}/></button>
              <button class="rshell-feat-btn {rsTailActivo(s.id)?'rs-feat-active':''}"
                title="{rsTailActivo(s.id)?(isEN?'Stop tail':'Detener tail'):(isEN?'Start log tail':'Iniciar tail de logs')}"
                on:click={() => { if(rsTailActivo(s.id)) { rsDetenerTodosTails(s.id); } else { tailShellId=s.id; tailPath=''; showTailModal=true; } }}><Antenna size={13}/></button>
              {#if !s.rdpMode}
              <button class="rshell-feat-btn" title="Broadcast" on:click={() => abrirBroadcast(s.id)}><Radio size={13}/></button>
              {/if}

              <!-- ── Group 3: RDP-specific (agent, reconnect) ───────────────── -->
              {#if s.rdpMode}
              <span class="rshell-toolbar-sep"></span>
              <button class="rshell-feat-btn rdp-agent-btn"
                class:rdp-agent-active={s.rdpAgentRunning}
                title="{isEN ? 'RDP Computer-Use Agent — Lucy controls the GUI autonomously' : 'Agente GUI — Lucy controla el escritorio remoto de forma autónoma'}"
                on:click={() => { const sx=getShell(s.id); if(sx){sx.rdpAgentPanel=!sx.rdpAgentPanel; rshellSessions=[...rshellSessions];} }}>
                <Cpu size={13} style="display:inline;vertical-align:middle;" />
                {isEN ? 'Agent' : 'Agente'}{#if s.rdpAgentRunning} <Loader size={11} style="display:inline;margin-left:4px;animation:spin 1.2s linear infinite;" />{/if}
              </button>
              <button class="rshell-feat-btn rdp-reconnect-btn" title="{isEN ? 'Open new RDP session' : 'Abrir nueva sesión RDP'}"
                on:click={() => invoke('launch_rdp', { host: s.host.host, port: s.host.port || 3389 }).catch(e => toast(String(e), 'error'))}>
                ↗ RDP
              </button>
              {/if}
            </div>
          </div>

          <!-- Ctrl+F search bar -->
          {#if nsSearch[s.id]?.open}
          <div class="ns-search-bar">
            <span class="ns-search-ico">⌕</span>
            <input id="ns-sf-{s.id}"
              class="ns-search-input"
              placeholder="{isEN ? 'Search output…' : 'Buscar en salida…'}"
              bind:value={nsSearch[s.id].query}
              on:input={() => { nsSearch = { ...nsSearch, [s.id]: { ...nsSearch[s.id], currentIdx: 0 } }; }}
              on:keydown={(e) => nsSearchKeydown(e, s.id)} />
            {#if nsSearch[s.id].query.trim()}
              {@const idxs = nsGetMatchIdxs(s.id, nsSearch[s.id].query)}
              <span class="ns-search-count" class:ns-search-zero={!idxs.length}>
                {idxs.length ? `${Math.min(nsSearch[s.id].currentIdx + 1, idxs.length)}/${idxs.length}` : (isEN ? '0 matches' : '0 coincidencias')}
              </span>
              <button class="ns-search-nav" on:click={() => nsSearchNav(s.id, -1)} title="Anterior (Shift+Enter)">↑</button>
              <button class="ns-search-nav" on:click={() => nsSearchNav(s.id, 1)} title="Siguiente (Enter)">↓</button>
            {/if}
            <button class="ns-search-close" on:click={() => nsSearchClose(s.id)} title="Cerrar (Esc)">✕</button>
          </div>
          {/if}

          <!-- RDP Copilot banner -->
          {#if s.rdpMode}
          <div class="rdp-banner">
            <span class="rdp-banner-ico">⊡</span>
            <span class="rdp-banner-txt">
              {isEN ? 'RDP Clipboard Copilot — Lucy generates commands. Copy them into the RDP window, paste results back.' : 'Copiloto RDP — Lucy genera comandos. Cópialos en la ventana RDP y pega el resultado aquí.'}
            </span>
          </div>
          {/if}

          <!-- ── Incident Mode Panel (SRE) ─────────────────────────────────── -->
          {#if s.incidentId && s.incidentPanelOpen}
          <div style="margin:8px 0;">
            <IncidentPanel
              incidentId={s.incidentId}
              {isEN}
              on:phase-changed={(ev) => handleIncidentPhaseChanged(s.id, ev)}
              on:closed={() => handleIncidentClosed(s.id)}
            />
          </div>
          {/if}

          <!-- ── RDP Computer-Use Agent Panel ────────────────────────────────── -->
          {#if s.rdpMode && s.rdpAgentPanel}
          <div class="rdp-agent-panel">
            <!-- Provider & Model selector row (Claude only for Computer Use) -->
            <div class="rdp-agent-config-row">
              <Plug size={14} style="color:#8b5cf6;flex-shrink:0;margin-right:4px;" />
              <span style="font-size:11px;color:#888;flex-shrink:0;">
                {isEN ? 'Vision Engine:' : 'Motor de Visión:'}
              </span>
              <!-- Fixed to Claude Sonnet 4.5 — empirically the most reliable for GUI
                   Computer Use (OSWorld benchmark leader, ~5x cheaper than Opus,
                   faster iteration loops). Not user-configurable to prevent
                   picking a weaker model for this specialized task. -->
              <span
                class="rdp-agent-model-fixed"
                title={isEN
                    ? 'Claude Sonnet 4.5 — specialized for Computer Use. Fixed for reliability.'
                    : 'Claude Sonnet 4.5 — especializado en Computer Use. Fijo por confiabilidad.'}
              >
                ◉ Claude Sonnet 4.5
                <span style="font-size:9px;color:#666;margin-left:4px;">({isEN ? 'locked' : 'fijo'})</span>
              </span>

            </div>

            <!-- Task input row -->
            <div class="rdp-agent-task-row">
              <Cpu size={15} style="color:#34d399;flex-shrink:0;" />
              <input
                class="rdp-agent-task-input"
                placeholder={isEN ? 'Describe the task… (e.g. "Open regedit and navigate to HKLM\\Software")' : 'Describe la tarea… (ej: "Abre regedit y navega a HKLM\\Software")'}
                bind:value={s.rdpAgentTask}
                disabled={s.rdpAgentRunning}
                on:keydown={(e) => { if (e.key === 'Enter' && !s.rdpAgentRunning) startRdpAgent(s.id); }}
              />
              {#if !s.rdpAgentRunning}
              <button class="rdp-agent-run-btn"
                disabled={!s.rdpAgentTask?.trim()}
                on:click={() => startRdpAgent(s.id)}>
                <Play size={13} style="display:inline;margin-right:4px;" /> {isEN ? 'Run' : 'Ejecutar'}
              </button>
              {:else}
              <button class="rdp-agent-stop-btn" on:click={() => stopRdpAgent(s.id)}>
                <Pause size={13} style="display:inline;margin-right:4px;" /> {isEN ? 'Stop' : 'Detener'}
              </button>
              {/if}
            </div>

            <!-- Live screenshot + log -->
            {#if s.rdpAgentScreenshot || s.rdpAgentLog.length > 0}
            <div class="rdp-agent-body">
              <!-- Latest screenshot -->
              {#if s.rdpAgentScreenshot}
              <div class="rdp-agent-screen-wrap">
                <img
                  class="rdp-agent-screen"
                  src="data:image/png;base64,{s.rdpAgentScreenshot}"
                  alt="RDP screen"
                />
                <div class="rdp-agent-screen-label">
                  {isEN ? 'Latest frame' : 'Último frame'}
                  {#if s.rdpAgentRunning}<span class="rdp-agent-pulse">●</span>{/if}
                </div>
              </div>
              {/if}

              <!-- Action log -->
              <div class="rdp-agent-log">
                {#each s.rdpAgentLog as entry}
                  {#if entry.kind === 'action'}
                  <div class="rdpa-entry rdpa-action"><Zap size={12} style="display:inline;margin-right:3px;" /> {entry.detail}</div>
                  {:else if entry.kind === 'text'}
                  <div class="rdpa-entry rdpa-text"><MessageCircle size={12} style="display:inline;margin-right:3px;" /> {entry.detail}</div>
                  {:else if entry.kind === 'screenshot'}
                  <div class="rdpa-entry rdpa-shot"><Camera size={12} style="display:inline;margin-right:3px;" /> {isEN ? 'Screenshot captured' : 'Captura tomada'}</div>
                  {:else if entry.kind === 'done'}
                  <div class="rdpa-entry rdpa-done"><CheckCircle size={12} style="display:inline;margin-right:3px;" /> {entry.detail || (isEN ? 'Task complete' : 'Tarea completa')}</div>
                  {:else if entry.kind === 'error'}
                  <div class="rdpa-entry rdpa-error"><AlertCircle size={12} style="display:inline;margin-right:3px;" /> {entry.detail}</div>
                  {/if}
                {/each}
                {#if s.rdpAgentRunning}
                <div class="rdpa-entry rdpa-thinking">
                  <span class="rdpa-dots">···</span> {isEN ? 'Working…' : 'Trabajando…'}
                </div>
                {/if}
              </div>
            </div>
            {/if}

            <!-- How-it-works hint (shown when no log yet) -->
            {#if !s.rdpAgentLog.length && !s.rdpAgentRunning}
            <div class="rdp-agent-hint">
              <strong>{isEN ? 'How it works:' : 'Cómo funciona:'}</strong>
              {isEN
                ? 'Lucy uses Claude\'s native Computer Use API to take screenshots of the mstsc window, then autonomously clicks, types and navigates to complete your task. The agentic loop runs up to 20 steps. Check Claude API health status before running.'
                : 'Lucy usa la API Computer Use nativa de Claude para capturar la ventana mstsc y luego hace clic, escribe y navega de forma autónoma para completar tu tarea. El loop agentic ejecuta hasta 20 pasos. Verifica el estado de la API de Claude antes de ejecutar.'}
            </div>
            {/if}
          </div>
          {/if}
          <!-- ── END Agent Panel ──────────────────────────────────────────────── -->

          <!-- Turn-Loop tracker -->
          {#if turnLoops[s.id]}
          <div style="padding:0 8px;">
            <TurnLoopPanel loop={turnLoops[s.id]} {isEN} on:stop={() => tlStop(s.id)} />
          </div>
          {/if}

          <!-- Output area -->
          <div class="rshell-out" id="rshell-out-{s.id}">
            {#if s.history.some(e => e.restored)}
              <div style="font-size:10px;color:#1e3a5f;padding:4px 10px;border-bottom:1px solid #0f1a2e;opacity:0.7;">
                ↻ {isEN ? 'Conversation restored from previous session' : 'Conversación restaurada de sesión anterior'}
              </div>
            {/if}
            {#if nsHiddenCount(s.history, s.id) > 0}
              <button class="ns-show-more" on:click={() => nsExpandCap(s.id)}>
                ↑ {isEN ? `Show ${Math.min(NS_RENDER_CAP_STEP, nsHiddenCount(s.history, s.id))} more (${nsHiddenCount(s.history, s.id)} hidden)` : `Mostrar ${Math.min(NS_RENDER_CAP_STEP, nsHiddenCount(s.history, s.id))} más (${nsHiddenCount(s.history, s.id)} ocultos)`}
              </button>
            {/if}
            {#each nsVisibleHistory(s.history, s.id) as entry, _i (entry.id || (entry.type + '-' + _i + '-' + entry.time))}
              {@const _sq = nsSearch[s.id]?.query?.trim()}
              {@const _sm = _sq && (entry.text||'').toLowerCase().includes(_sq.toLowerCase())}
              {@const _sc = _sm && nsGetMatchIdxs(s.id, _sq)[nsSearch[s.id]?.currentIdx] === _i}
              <div class="rshell-line rsl-{entry.type}" class:rsl-search-match={_sm} class:rsl-search-current={_sc}
                id={_sm ? `ns-m-${s.id}-${_i}` : undefined}>
                {#if entry.type === 'cmd'}
                  <span class="rsl-prompt">$</span><span class="rsl-cmd">{entry.text.replace(/^\$ /,'')}</span>
                  <span class="ns-cmd-acts">
                    <button class="ns-cmd-act" type="button" title={isEN ? 'Copy command' : 'Copiar comando'}
                      on:click={() => nsCmdCopy(entry.id, entry.text.replace(/^\$ /,''))}>{_nsCopiedId === entry.id ? '✓' : '⧉'}</button>
                    <button class="ns-cmd-act" type="button" title={isEN ? 'Re-run (prefills the box)' : 'Re-ejecutar (lo deja en la barra)'}
                      on:click={() => nsApplyFix(s.id, entry.text.replace(/^\$ /,''))}>↻</button>
                    <button class="ns-cmd-act" type="button" title={isEN ? 'Ask Lucy to explain this' : 'Pide a Lucy que lo explique'}
                      on:click={() => nsCmdExplain(s.id, entry.text.replace(/^\$ /,''))}>?</button>
                  </span>
                {:else if entry.type === 'lucy-in'}
                  <span class="rsl-prompt">→</span><span class="rsl-lucy-in">{entry.text}</span>
                {:else if entry.type === 'lucy-out'}
                  <span class="rsl-prompt lucy-dot">●</span><span class="rsl-lucy-out">{@html renderMd(entry.text, { chips: false })}</span>
                {:else if entry.type === 'reasoning'}
                  <div class="ns-reasoning {entry.active ? 'nr-active' : 'nr-done'} {entry.collapsed ? 'nr-collapsed' : ''}">
                    <button type="button" class="nr-head" on:click={() => { entry.collapsed = !entry.collapsed; rshellSessions = [...rshellSessions]; }}>
                      <span class="nr-icon">·</span>
                      <span class="nr-title">{entry.active ? 'Pensando…' : `Pensó durante ${entry.duration.toFixed(1)}s`}</span>
                      {#if entry.active}<span class="nr-timer">{entry.duration.toFixed(1)}s</span>{/if}
                      <span class="nr-chev">{entry.collapsed ? '▸' : '▾'}</span>
                    </button>
                    {#if !entry.collapsed && entry.content}
                      <pre class="nr-body">{entry.content}</pre>
                    {/if}
                  </div>
                {:else if entry.type === 'tool-card'}
                  <details class="ns-toolcard ntc-{entry.status}" open={entry.status === 'error'}>
                    <summary class="ntc-head">
                      <span class="ntc-icon">{entry.icon}</span>
                      <span class="ntc-label">{entry.label}</span>
                      {#if entry.duration > 0}<span class="ntc-dur">{entry.duration.toFixed(2)}s</span>{/if}
                      <span class="ntc-status">
                        {#if entry.status === 'running'}<span class="ntc-spinner"></span>{:else if entry.status === 'error'}✕{:else}✓{/if}
                      </span>
                    </summary>
                    {#if entry.output}
                      <pre class="ntc-body">{entry.output.length > 4000 ? entry.output.slice(0,4000) + '\n… [truncated]' : entry.output}</pre>
                    {/if}
                  </details>
                {:else if entry.type === 'fix-chip' && entry.fix}
                  <div class="ns-fixchip">
                    <span class="nfx-ico">{entry.fix.icon}</span>
                    <div class="nfx-body">
                      <div class="nfx-title">{entry.fix.title}</div>
                      <div class="nfx-hint">{entry.fix.hint}</div>
                      <code class="nfx-cmd">{entry.fix.fixCmd}</code>
                    </div>
                    <button class="nfx-btn" type="button" on:click={() => nsApplyFix(s.id, entry.fix.fixCmd)}>
                      {isEN ? 'Apply fix' : 'Aplicar fix'}
                    </button>
                  </div>
                {:else if entry.type === 'err'}
                  <span class="rsl-err-txt">{entry.text}</span>
                {:else if entry.type === 'info'}
                  <span class="rsl-info-txt">{entry.text}</span>
                {:else}
                  <pre class="rsl-out-txt">{entry.text}</pre>
                  {#if entry.exitCode !== null && entry.exitCode !== undefined}
                    <div class="rsl-meta-row">
                      <span class="rsl-exit-badge {entry.exitCode === 0 ? 'ok' : 'err'}">{entry.exitCode === 0 ? '✓ exit 0' : `✗ exit ${entry.exitCode}`}</span>
                      {#if entry.durationMs !== null && entry.durationMs !== undefined}
                        <span class="rsl-dur">⏱ {entry.durationMs < 1000 ? `${entry.durationMs}ms` : `${(entry.durationMs / 1000).toFixed(1)}s`}</span>
                      {/if}
                    </div>
                  {/if}
                {/if}
                <span class="rsl-time">{entry.time}</span>
              </div>
            {/each}
            {#if s.isStreaming}
              <div class="rsl-live-block">
                <div class="rsl-live-hdr">
                  <span class="rsl-live-dot"></span>
                  <span class="rsl-live-label">{isEN ? 'Running…' : 'En ejecución…'}</span>
                  {#if s._streamWatchdogBudget && s._streamWatchdogBudget > 5 * 60000}
                    <span class="rsl-watchdog"
                          title={isEN ? 'Adaptive silence watchdog — this command type gets a longer grace window before it is considered hung' : 'Watchdog adaptativo de silencio — este tipo de comando recibe una ventana más larga antes de considerarse colgado'}>⏱ {Math.round(s._streamWatchdogBudget / 60000)}m</span>
                  {/if}
                  <button class="rsl-live-input-btn"
                    on:click={() => { const sx=getShell(s.id); if(sx){ sx.waitingForInput=!sx.waitingForInput; sx.promptHint='Input'; sx.promptIsPassword=false; rshellSessions=[...rshellSessions]; } }}>⌨ Input</button>
                  <button class="rsl-cancel-btn" on:click={() => cancelarStream(s.id)}>✕ Cancelar</button>
                </div>
                <pre class="rsl-live-pre">{s.streamOut}<span class="rsl-live-cursor"></span></pre>
                {#if s.waitingForInput}
                  <div class="rsl-iprompt-row">
                    <span class="rsl-iprompt-hint">{s.promptHint || 'Input'}:</span>
                    <input class="rsl-iprompt-input" type={s.promptIsPassword ? 'password' : 'text'}
                      bind:value={s.interactiveInput}
                      on:keydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); rsEnviarInput(s.id); } else if (e.key === 'Escape') { const sx=getShell(s.id); if(sx){sx.waitingForInput=false;rshellSessions=[...rshellSessions];} } }}
                      placeholder={s.promptHint || (isEN ? 'Type and press Enter…' : 'Escribe y presiona Enter…')}>
                    <button class="rsl-iprompt-send" on:click={() => rsEnviarInput(s.id)}>↵</button>
                  </div>
                {/if}
              </div>
            {:else if s.running && !s.isStreaming}
              <!-- Guard evaluando / conectando — gap visual entre enviar y primer chunk -->
              <div class="rshell-line rsl-pending">
                <span class="rsl-spin" style="opacity:0.5;">◌</span>
                <span style="color:#334155;font-size:11px;">{isEN ? 'Checking…' : 'Verificando…'}</span>
              </div>
            {:else if s.lucyRunning}
              <div class="rshell-line rsl-running">
                <span class="rsl-spin">◌</span>
                <span style="color:#475569;font-size:11px;">Lucy procesando...</span>
              </div>
            {/if}
          </div><!-- /rshell-out -->

          <!-- ── RDP CLIPBOARD STRIP ────────────────────────────────────────── -->
          {#if s.rdpMode && s.rdpClipboardCmd}
          <div class="rdp-clip-strip">
            <span class="rdp-clip-label">📋 {isEN ? 'Copy & paste in RDP:' : 'Copia y pega en RDP:'}</span>
            <code class="rdp-clip-cmd">{s.rdpClipboardCmd}</code>
            <button class="rdp-clip-copy" on:click={() => {
              navigator.clipboard.writeText(s.rdpClipboardCmd).then(() => toast(isEN ? 'Copied to clipboard' : 'Copiado al portapapeles', 'success'));
            }}>📋 {isEN ? 'Copy' : 'Copiar'}</button>
            <button class="rdp-clip-dismiss" on:click={() => { const sx=getShell(s.id); if(sx){sx.rdpClipboardCmd=null;rshellSessions=[...rshellSessions];} }} title="Descartar">✕</button>
          </div>
          {/if}

          <!-- Toggle bar para colapsar/expandir inputs -->
          <div class="ns-input-toggle-bar" role="button" tabindex="0" on:click={() => nsInputsCollapsed = !nsInputsCollapsed} on:keydown={(e) => { if(e.key==='Enter'||e.key===' ') nsInputsCollapsed = !nsInputsCollapsed; }}
            title={nsInputsCollapsed ? (isEN ? 'Expand command panel' : 'Expandir panel de comandos') : (isEN ? 'Collapse command panel' : 'Colapsar panel de comandos')}>
            <span class="ns-input-toggle-ico">{nsInputsCollapsed ? (isEN ? '▲ Show commands' : '▲ Mostrar comandos') : (isEN ? '▼ Hide commands' : '▼ Ocultar comandos')}</span>
            {#if nsInputsCollapsed && s.connected}
              <span class="ns-input-toggle-hint">{isEN ? 'Click or Ctrl+I to expand · Type to auto-open' : 'Click o Ctrl+I para expandir · Escribe para abrir automáticamente'}</span>
            {/if}
          </div>

          <!-- Input area (colapsable) -->
          {#if !nsInputsCollapsed}
          <div class="rshell-inputs">

          <!-- ── RDP INPUT PANEL (replaces direct terminal for RDP sessions) ── -->
          {#if s.rdpMode}
            <div class="rshell-input-wrap rdp-result-wrap">
              <div class="rshell-input-label">
                <span class="rs-label-ico">📥</span>
                <span>{isEN ? 'Paste result from RDP:' : 'Pega el resultado desde RDP:'}</span>
                <span class="rs-hint">{isEN ? 'Run the command above in the RDP window, copy the output and paste it here' : 'Ejecuta el comando de arriba en la ventana RDP, copia el resultado y pégalo aquí'}</span>
              </div>
              <div class="rshell-input-row" style="align-items:flex-end;">
                <textarea class="rsi-box rdp-result-box" rows="3"
                  placeholder={isEN ? 'Paste output here and press Enter…' : 'Pega la salida aquí y presiona Enter…'}
                  bind:value={s.rdpResultIn}
                  on:keydown={(e) => {
                    if (e.key === 'Enter' && !e.shiftKey) {
                      e.preventDefault();
                      if (s.rdpResultIn.trim()) {
                        const result = s.rdpResultIn.trim();
                        const sx = getShell(s.id);
                        if (sx) { sx.rdpResultIn = ''; sx.rdpClipboardCmd = null; rshellSessions=[...rshellSessions]; }
                        rsLogTo(s.id, 'out', result);
                        // Feed result back to Lucy
                        const sx2 = getShell(s.id);
                        if (sx2) { sx2.lucyIn = `[RESULTADO DEL COMANDO EN RDP]\n${result}`; rshellSessions=[...rshellSessions]; }
                        rsEnviarLucy(s.id);
                      }
                    }
                  }}>
                </textarea>
                <button class="rsi-send" style="align-self:flex-end;margin-bottom:2px;"
                  disabled={s.lucyRunning || !s.rdpResultIn?.trim()}
                  on:click={() => {
                    const sx = getShell(s.id);
                    if (!sx?.rdpResultIn?.trim()) return;
                    const result = sx.rdpResultIn.trim();
                    sx.rdpResultIn = ''; sx.rdpClipboardCmd = null; rshellSessions=[...rshellSessions];
                    rsLogTo(s.id, 'out', result);
                    const sx2 = getShell(s.id);
                    if (sx2) { sx2.lucyIn = `[RESULTADO DEL COMANDO EN RDP]\n${result}`; rshellSessions=[...rshellSessions]; }
                    rsEnviarLucy(s.id);
                  }}><Play size={12}/></button>
              </div>
            </div>
          {/if}
          <!-- ── END RDP INPUT PANEL ─────────────────────────────────────────── -->
          {#if !s.rdpMode}
            <div class="rshell-input-wrap rs-direct">
              <div class="rshell-input-label">
                <code class="rs-label-ico">&gt;_</code><span>Comando directo</span>
                <span class="rs-hint">↑↓ historial · Tab autocompletar · Enter enviar · Ctrl+Enter background</span>
                {#if s.bgTasks?.length}<span class="rs-bg-badge">{s.bgTasks.length} bg</span>{/if}
              </div>
              <div class="rshell-input-row" style="position:relative;">
                <span class="rsi-prompt">{s.host.type==='linux'?'bash':'PS'} &gt;</span>
                <div style="flex:1;position:relative;">
                  {#if rsSuggestion(s.id)}
                    <div class="rs-suggestion" aria-hidden="true">
                      <span style="opacity:0">{s.directIn}</span><span>{rsSuggestion(s.id).slice(s.directIn.length)}</span>
                    </div>
                  {:else if s._aiSugg}
                    <div class="rs-suggestion rs-sugg-ai" aria-hidden="true">
                      <span style="opacity:0">{s.directIn}</span><span>→ {s._aiSugg.slice(s.directIn.length)}</span>
                    </div>
                  {/if}
                  <input class="rsi-box rs-direct-box"
                    id={`ns-direct-${s.id}`}
                    placeholder="systemctl restart sshd · whoami · ls -la ..."
                    bind:value={s.directIn}
                    on:keydown={(e) => rsKeyDirect(e, s.id)}
                    on:input={() => rsHandleDirectInput(s.id)}
                    disabled={s.running || s.isStreaming || !s.connected}>
                </div>
                <button class="rsi-send" on:click={() => rsEnviarDirecto(s.id)}
                  disabled={s.running || s.isStreaming || !s.connected || !s.directIn.trim()}><Play size={12}/></button>
              </div>
              {#if s._aiSuggLoading}
                <div class="rs-ai-spinner">↻ <span class="rs-ai-spin-dot">Lucy pensando…</span></div>
              {/if}
            </div>
          {/if}<!-- /!s.rdpMode direct input -->
            <div class="rshell-input-wrap rs-lucy">
              <div class="rshell-input-label" style="display:flex; align-items:center;">
                <span class="rs-label-ico">→</span><span>Lucy — IA interactiva</span>
                <div style="margin-left: 10px; display:inline-block;">
                  <select class="ns-llm-select" bind:value={selectedModel} title={getModelDescription(selectedModel, isEN)}>
                    {#each LLM_GROUPS as group}
                      <optgroup label={group.label}>
                        {#each group.options as opt}
                          <option value={opt.id}>{opt.icon} {isEN ? opt.nameEn : opt.nameEs}</option>
                        {/each}
                      </optgroup>
                    {/each}
                  </select>
                </div>
                <span class="rs-hint">Enter enviar · Shift+Enter nueva línea</span>
              </div>
              <div class="rshell-input-row" style="align-items:flex-end;">
                <span class="rsi-prompt" style="color:var(--acc);padding-bottom:9px;">Lucy &gt;</span>
                <textarea class="rsi-box rs-lucy-box rs-lucy-ta" rows="1"
                  id={`ns-lucy-${s.id}`}
                  placeholder={s.rdpMode
                    ? (isEN ? 'Ask Lucy, describe what you see, or paste a screenshot…' : 'Pregunta a Lucy, describe lo que ves, o pega una captura…')
                    : (isEN ? '/fix [problem] for auto-troubleshoot · or ask Lucy anything...' : '/fix [problema] para auto-diagnostico · o pregunta lo que sea a Lucy...')}
                  bind:value={s.lucyIn}
                  on:keydown={(e) => rsKeyLucy(e, s.id)}
                  on:input={(e) => { e.target.style.height='auto'; e.target.style.height=Math.min(e.target.scrollHeight,140)+'px'; }}
                  disabled={s.lucyRunning || (!s.connected && !s.rdpMode)}></textarea>
                <button class="rsi-send rs-lucy-send" style="align-self:flex-end;margin-bottom:2px;" on:click={() => rsEnviarLucy(s.id)}
                  disabled={s.lucyRunning || (!s.connected && !s.rdpMode) || !s.lucyIn.trim()}><Play size={12}/></button>
              </div>
            </div>
          </div><!-- /rshell-inputs -->
          {/if}

        </div><!-- /ns-shell-wrap -->
        {/each}

      {/if}<!-- /sessions -->

    </div><!-- /ns-workspace -->

  </div><!-- /ns-body -->
</div><!-- /ns-view -->

<!-- ── MODAL: PLAYBOOKS ── -->
{#if showPlaybookModal}
<div class="mb" role="button" tabindex="-1" on:click|self={() => showPlaybookModal=false} on:keydown>
  <div class="mbox lg">
    <div class="mhdr">
      <h3 class="mtitle" style="display:flex;align-items:center;gap:6px;"><BookOpen size={15}/> Playbooks — {getShell(playbookShellId)?.host.name}</h3>
      <button class="mclose" on:click={() => showPlaybookModal=false}><X size={14}/></button>
    </div>
    <div style="display:flex;flex-direction:column;gap:14px;">
      {#each rsGetPlaybooks(getShell(playbookShellId)?.host.id||'') as pb (pb.id)}
      <div class="pb-item">
        <div class="pb-name" style="display:flex;align-items:center;gap:5px;"><Play size={11}/> {pb.name}</div>
        <div class="pb-cmds">{pb.commands.join(' → ')}</div>
        <div style="display:flex;gap:6px;margin-top:6px;">
          <button class="mbtn pri" style="display:flex;align-items:center;gap:5px;" on:click={() => rsRunPlaybook(playbookShellId, pb)}><Play size={11}/> Ejecutar</button>
          <button class="mbtn ghost" style="display:flex;align-items:center;gap:5px;" on:click={() => { rsDeletePlaybook(getShell(playbookShellId)?.host.id, pb.id); }}><X size={11}/> Eliminar</button>
        </div>
      </div>
      {:else}
      <p style="color:#475569;font-size:12px;">No hay playbooks guardados para este host.</p>
      {/each}
      <div style="border-top:1px solid var(--bdr);padding-top:12px;">
        <div style="margin-bottom:8px;">
          <label style="font-size:11px;color:var(--txt3);display:block;margin-bottom:4px;" for="pb-name">{isEN ? 'Name' : 'Nombre'} del playbook</label>
          <input id="pb-name" class="minp" bind:value={pbForm.name} placeholder="Diagnóstico de sistema">
        </div>
        <div>
          <label style="font-size:11px;color:var(--txt3);display:block;margin-bottom:4px;" for="pb-cmds">Comandos (uno por línea)</label>
          <textarea id="pb-cmds" class="minp" style="height:90px;resize:vertical;font-family:var(--mono);font-size:11px;"
            bind:value={pbForm.commands}
            placeholder="df -h&#10;free -m&#10;systemctl --failed&#10;uptime"></textarea>
        </div>
        <button class="mbtn pri" style="margin-top:8px;display:flex;align-items:center;gap:5px;" on:click={rsGuardarPlaybook}
          disabled={!pbForm.name.trim() || !pbForm.commands.trim()}>↓ Guardar playbook</button>
      </div>
    </div>
  </div>
</div>
{/if}

<!-- ── MODAL: FILE TRANSFER ── -->
{#if showFileTransferModal}
<div class="mb" role="button" tabindex="-1" on:click|self={() => showFileTransferModal=false} on:keydown>
  <div class="mbox lg">
    <div class="mhdr">
      <h3 class="mtitle" style="display:flex;align-items:center;gap:6px;"><FolderSync size={15}/> Transferir archivos — {getShell(ftShellId)?.host.name}</h3>
      <button class="mclose" on:click={() => showFileTransferModal=false}><X size={14}/></button>
    </div>
    <div style="display:flex;flex-direction:column;gap:12px;">

      <div style="display:flex;gap:8px;">
        <button class="mbtn {ftDirection==='upload'?'pri':'ghost'}" style="flex:1;display:flex;align-items:center;justify-content:center;gap:5px;"
          on:click={() => { ftDirection='upload'; ftLocalPath=''; ftRemotePath=''; ftResult=''; }}>
          <Upload size={13}/> Subir al servidor
        </button>
        <button class="mbtn {ftDirection==='download'?'pri':'ghost'}" style="flex:1;display:flex;align-items:center;justify-content:center;gap:5px;"
          on:click={() => { ftDirection='download'; ftLocalPath=''; ftRemotePath=''; ftResult=''; }}>
          <Download size={13}/> Bajar del servidor
        </button>
      </div>

      <div style="display:flex;align-items:center;gap:8px;padding:8px 10px;background:rgba(0,0,0,.3);border-radius:6px;font-size:11px;color:#475569;">
        {#if ftDirection === 'upload'}
          <span style="color:var(--txt2);display:flex;align-items:center;gap:4px;"><Monitor size={12}/> Tu PC</span>
          <span style="flex:1;text-align:center;color:var(--acc);">──── <Upload size={11} style="display:inline;vertical-align:middle"/> ────→</span>
          <span style="color:var(--txt2);display:flex;align-items:center;gap:4px;"><Server size={12}/> {getShell(ftShellId)?.host.name}</span>
        {:else}
          <span style="color:var(--txt2);display:flex;align-items:center;gap:4px;"><Monitor size={12}/> Tu PC</span>
          <span style="flex:1;text-align:center;color:#6ab0ff;">←──── <Download size={11} style="display:inline;vertical-align:middle"/> ────</span>
          <span style="color:var(--txt2);display:flex;align-items:center;gap:4px;"><Server size={12}/> {getShell(ftShellId)?.host.name}</span>
        {/if}
      </div>

      {#if ftDirection === 'upload'}
        <div>
          <label style="font-size:11px;color:var(--txt3);display:block;margin-bottom:4px;" for="ft-local">Archivo en tu PC (origen)</label>
          <div style="display:flex;gap:6px;">
            <input id="ft-local" class="minp" style="flex:1;" bind:value={ftLocalPath}
              placeholder="C:\Users\tu\archivo.txt">
            <button class="mbtn ghost" title="{isEN ? 'Select' : 'Seleccionar'} archivo" on:click={rsPickFile}><FolderOpen size={13}/></button>
          </div>
        </div>
        <div>
          <label style="font-size:11px;color:var(--txt3);display:block;margin-bottom:4px;" for="ft-remote">Carpeta destino en el servidor</label>
          <input id="ft-remote" class="minp" bind:value={ftRemotePath}
            placeholder="/home/usuario/ o /opt/scripts/">
        </div>
      {:else}
        <div>
          <label style="font-size:11px;color:var(--txt3);display:block;margin-bottom:4px;" for="ft-remote">Archivo en el servidor (origen)</label>
          <input id="ft-remote" class="minp" bind:value={ftRemotePath}
            placeholder="/var/log/app.log o /etc/nginx/nginx.conf">
        </div>
        <div>
          <label style="font-size:11px;color:var(--txt3);display:block;margin-bottom:4px;" for="ft-local">Carpeta destino en tu PC</label>
          <div style="display:flex;gap:6px;">
            <input id="ft-local" class="minp" style="flex:1;" bind:value={ftLocalPath}
              placeholder="C:\\Users\\tu\\Descargas\\">
            <button class="mbtn ghost" title="{isEN ? 'Select' : 'Seleccionar'} carpeta destino" on:click={async () => {
              const p = await invoke('pick_file_path').catch(()=>'');
              if(p) ftLocalPath = p.substring(0, p.lastIndexOf('\\') + 1) || p;
            }}><FolderOpen size={13}/></button>
          </div>
        </div>
      {/if}

      {#if ftResult}
      <div style="font-size:12px;padding:8px 10px;border-radius:6px;
        background:{ftResult.startsWith('✓')?'rgba(16,185,129,.06)':'rgba(255,68,68,.06)'};
        color:{ftResult.startsWith('✓')?'var(--acc)':'var(--red)'};">{ftResult}</div>
      {/if}

      <button class="mbtn pri" on:click={rsEjecutarTransferencia}
        disabled={ftRunning || !(typeof ftLocalPath==='string'&&ftLocalPath.trim()) || !(typeof ftRemotePath==='string'&&ftRemotePath.trim())}>
        {#if ftRunning}⏳ Transfiriendo...{:else if ftDirection==='upload'}⬆ Subir archivo{:else}⬇ Bajar archivo{/if}
      </button>
    </div>
  </div>
</div>
{/if}

<!-- ── MODAL: TAIL -F ── -->
{#if showTailModal}
<div class="mb" role="button" tabindex="-1" on:click|self={() => showTailModal=false} on:keydown>
  <div class="mbox md">
    <div class="mhdr">
      <h3 class="mtitle" style="display:flex;align-items:center;gap:6px;"><Antenna size={15}/> Tail de logs — {getShell(tailShellId)?.host.name}</h3>
      <button class="mclose" on:click={() => showTailModal=false}><X size={14}/></button>
    </div>
    <div style="display:flex;flex-direction:column;gap:12px;">
      <div>
        <label style="font-size:11px;color:var(--txt3);display:block;margin-bottom:4px;" for="tail-path">Ruta del log</label>
        <input id="tail-path" class="minp" bind:value={tailPath} placeholder="/var/log/syslog">
      </div>
      <div style="display:flex;flex-wrap:wrap;gap:5px;">
        {#each (getShell(tailShellId)?.host.type==='linux'
    ? ['/var/log/syslog','/var/log/auth.log','/var/log/nginx/access.log','/var/log/nginx/error.log']
    : ['C:/Windows/Logs/CBS/CBS.log','C:/inetpub/logs/LogFiles']) as p}
        <button class="rs-log-preset" on:click={() => tailPath=p}>{p.replace(/\\/g,'/').split('/').pop()}</button>
        {/each}
      </div>
      <button class="mbtn pri" style="display:flex;align-items:center;gap:5px;" on:click={() => rsIniciarTail(tailShellId, tailPath)}
        disabled={!tailPath.trim()}><Antenna size={13}/> Iniciar tail</button>
    </div>
  </div>
</div>
{/if}

<!-- ── MODAL: BROADCAST ── -->
{#if showBroadcast}
<div class="mb" role="button" tabindex="-1" on:click|self={() => showBroadcast=false} on:keydown>
  <div class="mbox" style="width:580px;">
    <div class="mhdr">
      <h3 class="mtitle" style="display:flex;align-items:center;gap:6px;"><Radio size={15}/> Broadcast — Ejecutar en múltiples hosts</h3>
      <button class="mclose" on:click={() => showBroadcast=false}><X size={14}/></button>
    </div>
    <div style="display:flex;flex-direction:column;gap:14px;">
      <div>
        <label for={'bc-cmd-'+(broadcastShellId||'x')} style="font-size:11px;color:var(--txt3);display:block;margin-bottom:4px;">{isEN ? 'Command to execute on all selected hosts' : 'Comando a ejecutar en todos los hosts seleccionados'}</label>
        <input id={'bc-cmd-'+(broadcastShellId||'x')} class="minp" style="font-family:var(--mono);font-size:12px;"
          placeholder="systemctl status nginx · uptime · df -h ..."
          bind:value={broadcastCmd}>
      </div>
      <div>
        <span style="font-size:11px;color:var(--txt3);display:block;margin-bottom:4px;">{isEN ? `Target hosts (${broadcastSelected.size} selected)` : `Hosts destino (${broadcastSelected.size} seleccionados)`}</span>
        <div class="bc-host-list">
          {#each hosts.filter(h => h.id !== null) as h}
            <label class="bc-host-item">
              <input type="checkbox"
                checked={broadcastSelected.has(h.id)}
                on:change={(e) => {
                  if (e.target.checked) broadcastSelected.add(h.id);
                  else broadcastSelected.delete(h.id);
                  broadcastSelected = new Set(broadcastSelected);
                }}>
              <span class="bc-host-ico">{h.type === 'windows' ? '⊡' : '◈'}</span>
              <span class="bc-host-name">{h.name}</span>
              <span class="bc-host-addr">{h.host}:{h.port}</span>
            </label>
          {/each}
        </div>
      </div>
      {#if broadcastResults.length > 0}
        <div>
          <span style="font-size:11px;color:var(--txt3);display:block;margin-bottom:4px;">{isEN ? 'Results' : 'Resultados'} ({broadcastResults.filter(r=>r.exitCode===0).length}/{broadcastResults.length} OK)</span>
          <div class="bc-results">
            {#each broadcastResults as r}
              <div class="bc-result-row {r.exitCode === 0 ? 'bc-ok' : r.error ? 'bc-fail' : 'bc-warn'}">
                <span class="bc-r-host">{r.hostName}</span>
                <span class="bc-r-badge">{r.error ? '✗ error' : r.exitCode === 0 ? '✓ ok' : `✗ exit ${r.exitCode}`}</span>
                <pre class="bc-r-out">{r.error || r.output || ''}</pre>
              </div>
            {/each}
          </div>
        </div>
      {/if}
      <div style="display:flex;gap:8px;justify-content:flex-end;">
        <button class="mbtn ghost" on:click={() => showBroadcast=false}>Cancelar</button>
        <button class="mbtn pri"
          disabled={broadcastRunning || !broadcastCmd.trim() || broadcastSelected.size === 0}
          on:click={runBroadcast}>
          {#if broadcastRunning}<span class="rsl-spin">◌</span>{:else}◉{/if}
          Ejecutar en {broadcastSelected.size} host{broadcastSelected.size!==1?'s':''}
        </button>
      </div>
    </div>
  </div>
</div>
{/if}

<!-- ── DANGER CONFIRM MODAL (pre-execution hook) ── -->
{#if guardAssessment}
<DangerConfirmModal
  assessment={guardAssessment}
  hostName={guardHostName}
  source={guardSource}
  {isEN}
  on:confirm={guardConfirm}
  on:cancel={guardCancel}
/>
{/if}

<!-- ── In-app two-step prompt for starting an incident (replaces window.prompt()) ── -->
<PromptModal
  open={incidentPrompt?.step === 'title'}
  title={isEN ? 'Start incident' : 'Iniciar incidente'}
  label={isEN ? 'Incident title (1 line)' : 'Título del incidente (1 línea)'}
  defaultValue={isEN ? 'Investigating…' : 'Investigando…'}
  placeholder={isEN ? 'Brief summary' : 'Resumen breve'}
  confirmLabel={isEN ? 'Next' : 'Siguiente'}
  cancelLabel={isEN ? 'Cancel' : 'Cancelar'}
  on:submit={(e) => onIncidentPromptSubmit(e.detail)}
  on:cancel={() => incidentPrompt = null}
/>
<PromptModal
  open={incidentPrompt?.step === 'description'}
  title={isEN ? 'Start incident' : 'Iniciar incidente'}
  label={isEN ? 'Context / symptoms (optional)' : 'Contexto / síntomas (opcional)'}
  defaultValue=""
  placeholder={isEN ? 'What are you seeing? Any errors, timing, scope…' : '¿Qué estás viendo? Errores, timing, alcance…'}
  multiline={true}
  required={false}
  confirmLabel={isEN ? 'Start' : 'Iniciar'}
  cancelLabel={isEN ? 'Back' : 'Atrás'}
  on:submit={(e) => onIncidentPromptSubmit(e.detail)}
  on:cancel={() => incidentPrompt = null}
/>

<!-- Tier S #3 — Shell recording player overlay -->
{#if showRecPlayer}
    <ShellRecordingPlayer
        {isEN}
        initialHostId={recPlayerHostId}
        initialRecordingId={recPlayerOpenId}
        on:close={() => { showRecPlayer = false; recPlayerHostId = null; recPlayerOpenId = null; }}/>
{/if}

<style>
    /* ══════════════════════════════════════════════════════════════════════════ */
    /* NexShell View Styles                                                      */
    /* ══════════════════════════════════════════════════════════════════════════ */

    /* Full-view wrapper */
    .ns-view{
        display:flex;flex-direction:column;
        height:100%;overflow:hidden;
    }

    /* Header */
    .ns-hdr{
        display:flex;align-items:center;gap:12px;
        padding:10px 18px;
        background:var(--panel);
        border-bottom:1px solid var(--bdr);
        flex-shrink:0;
    }
    .ns-hdr-left{ display:flex;align-items:center;gap:10px; }
    .ns-hdr-center{
        flex:1;display:flex;justify-content:center;
    }
    .ns-search{
        width:260px;max-width:100%;
        background:rgba(255,255,255,.05);
        border:1px solid var(--bdr);
        border-radius:6px;
        color:var(--txt);
        font-size:13px;
        padding:5px 10px;
        outline:none;
        transition:.15s;
    }
    .ns-search:focus{ border-color:var(--acc); background:rgba(255,255,255,.08); }
    .ns-search::placeholder{ color:var(--txt3); }

    .ns-summary-badge{
        font-size:11px;color:var(--txt3);
        background:rgba(255,255,255,.06);
        border:1px solid var(--bdr);
        border-radius:10px;padding:2px 10px;
        white-space:nowrap;
    }
    .ns-add-btn{
        background:var(--acc);color:#000;
        border:none;border-radius:6px;
        font-size:12px;font-weight:700;
        padding:5px 13px;cursor:pointer;
        transition:.15s;white-space:nowrap;
    }
    .ns-add-btn:hover{ filter:brightness(1.15); }
    .ns-guard-btn{background:rgba(255,255,255,.04);border:1px solid var(--bdr);border-radius:6px;font-size:11px;padding:4px 8px;cursor:pointer;transition:.15s;color:#4a5a6a;white-space:nowrap;}
    .ns-guard-btn:hover{background:rgba(255,255,255,.08);color:var(--txt);}
    .ns-guard-btn.active{border-color:rgba(16,185,129,.25);color:var(--acc);background:rgba(16,185,129,.06);}
    .ns-panel-toggle{ background:rgba(255,255,255,.05);border:1px solid #1a2030;color:var(--txt2);padding:4px 10px;border-radius:5px;font-size:11px;cursor:pointer;transition:.15s;white-space:nowrap; }
    .ns-panel-toggle:hover{ background:rgba(255,255,255,.09);color:var(--txt); }

    /* Body: two-column layout */
    .ns-body{
        display:grid;
        grid-template-columns:310px 1fr;
        flex:1;
        overflow:hidden;
        gap:0;
    }
    .ns-body-full{ grid-template-columns:1fr; }
    .ns-body-full .ns-workspace{ width:100%; }

    /* Column labels */
    .ns-col-lbl{
        font-size:10px;font-weight:700;letter-spacing:.08em;
        color:var(--txt3);
        padding:10px 14px 6px;
        text-transform:uppercase;
        display:flex;align-items:center;gap:6px;
        flex-shrink:0;
    }
    .ns-col-count{
        background:rgba(255,255,255,.1);
        border-radius:8px;padding:1px 6px;
        font-size:10px;color:var(--txt2);
    }

    /* Toolbar */
    .ns-col-toolbar{ display:flex;gap:6px;padding:8px 10px;border-bottom:1px solid #1a2030;flex-shrink:0; }
    .ns-sort-sel{ background:#0a1018;border:1px solid #1a2030;color:var(--txt2);padding:4px 8px;border-radius:5px;font-size:11px;cursor:pointer;flex-shrink:0;outline:none; }
    .ns-sort-sel:focus{ border-color:var(--acc); }
    .ns-cat-chips{ display:flex;gap:4px;padding:6px 10px;flex-wrap:wrap;border-bottom:1px solid #1a2030;flex-shrink:0; }
    .ns-cat-chip{ background:rgba(255,255,255,.05);border:1px solid #1a2030;color:var(--txt3);padding:2px 8px;border-radius:10px;font-size:10px;cursor:pointer;transition:.15s;white-space:nowrap; }
    .ns-cat-chip:hover{ background:rgba(255,255,255,.09);color:var(--txt); }
    .ns-cat-active{ background:rgba(16,185,129,.12);border-color:rgba(16,185,129,.35);color:var(--acc); }

    /* Category badge variants */
    .ns-cat-badge-shell     { background:rgba(0,120,215,.25);color:#60b8ff;border-color:rgba(0,120,215,.4); }
    .ns-cat-badge-database  { background:rgba(139,92,246,.25);color:#c084fc;border-color:rgba(139,92,246,.4); }
    .ns-cat-badge-container { background:rgba(14,165,233,.25);color:#38bdf8;border-color:rgba(14,165,233,.4); }
    .ns-cat-badge-kubernetes{ background:rgba(59,130,246,.25);color:#93c5fd;border-color:rgba(59,130,246,.4); }
    .ns-cat-badge-network   { background:rgba(16,185,129,.25);color:#6ee7b7;border-color:rgba(16,185,129,.4); }

    /* LEFT COLUMN — host catalogue */
    .ns-hosts-col{
        display:flex;flex-direction:column;
        border-right:1px solid var(--bdr);
        overflow-y:auto;
        overflow-x:hidden;
        background:var(--panel);
    }

    /* Host card.
       BUG FIX: previously had `opacity:0` + CSS animation with delay. Each
       time rshellSessions updated, Svelte re-emitted the {#each} and the
       animation replayed — if a new render arrived before the delay
       elapsed, the card stuck at opacity:0 forever. Result: hosts
       disappeared while the counter still said 3 (visible in user repro
       once 3 sessions were live).
       Now the entrance is driven by Svelte's `in:` transition (in the
       template), which only runs on mount/unmount — not on re-renders.
       The CSS holds only the resting state (always visible). */
    .ns-host-card{
        margin:4px 8px;
        background:rgba(255,255,255,.04);
        border:1px solid var(--bdr);
        border-radius:10px;
        padding:10px 12px;
        transition:border-color .2s, box-shadow .2s, transform .2s;
        cursor:default;
    }
    .ns-host-card:hover{
        border-color:rgba(255,255,255,.15);
        box-shadow:0 6px 16px rgba(0,0,0,.4);
        transform: translateY(-1px);
    }
    .ns-card-on{
        border-color:rgba(0,230,130,.35)!important;
        box-shadow:0 0 0 1px rgba(0,230,130,.15), 0 0 14px rgba(16,185,129,.08);
    }
    .ns-card-connecting{
        border-color:rgba(255,200,0,.3)!important;
        position:relative;
        overflow:hidden;
    }
    /* Diagonal shimmer that sweeps across cards in "connecting" state */
    .ns-card-connecting::before{
        content:'';position:absolute;top:0;left:-50%;width:50%;height:100%;
        background:linear-gradient(110deg, transparent 35%, rgba(255,200,0,.12) 50%, transparent 65%);
        animation: ns-card-shimmer 1.4s ease-in-out infinite;
        pointer-events:none;
    }
    @keyframes ns-card-shimmer{
        0%   { left:-50%; }
        100% { left:120%; }
    }
    /* Pulsing dot + outer ring for the connecting indicator */
    .ns-card-connecting .ns-conn-pill{
        position:relative;
    }
    .ns-card-connecting .ns-conn-pill::after{
        content:'';position:absolute;left:6px;top:50%;width:4px;height:4px;
        border-radius:50%;background:rgba(255,200,0,.85);
        transform:translateY(-50%) scale(1);
        animation:ns-conn-pulse 1.2s ease-in-out infinite;
    }
    @keyframes ns-conn-pulse{
        0%,100% { transform:translateY(-50%) scale(1);    box-shadow:0 0 0 0   rgba(255,200,0,.55); }
        70%     { transform:translateY(-50%) scale(1.15); box-shadow:0 0 0 6px rgba(255,200,0,0); }
    }
    /* Subtle pop animation when a connected card transitions to "on" state */
    .ns-card-on{
        animation: ns-card-pop .42s cubic-bezier(0.34,1.56,0.64,1);
    }
    @keyframes ns-card-pop{
        0%   { transform: scale(1); }
        50%  { transform: scale(1.018); }
        100% { transform: scale(1); }
    }
    /* Original connecting border (kept after the shimmer additions) */
    .ns-card-connecting-orig{
        border-color:rgba(255,200,0,.3)!important;
        box-shadow:0 0 10px rgba(255,200,0,.06);
    }
    .ns-card-focused{ border-color:rgba(16,185,129,.45)!important;background:rgba(16,185,129,.05)!important;box-shadow:0 0 0 1px rgba(16,185,129,.2),0 0 18px rgba(16,185,129,.1)!important; }

    .ns-card-top{
        display:flex;align-items:center;gap:8px;
        margin-bottom:6px;
    }
    .ns-card-ico{ font-size:18px;flex-shrink:0; }
    .ns-card-info{
        display:flex;flex-direction:column;flex:1;min-width:0;
    }
    .ns-card-name{
        font-size:13px;font-weight:600;color:var(--txt);
        white-space:nowrap;overflow:hidden;text-overflow:ellipsis;
    }
    .ns-card-addr{
        font-size:11px;color:var(--txt3);font-family:monospace;
        white-space:nowrap;overflow:hidden;text-overflow:ellipsis;
    }
    .ns-proto-badge{
        font-size:10px;font-weight:700;border-radius:5px;
        padding:2px 6px;flex-shrink:0;
        border:1px solid transparent;
    }

    .ns-card-meta{
        display:flex;align-items:center;gap:8px;
        font-size:11px;margin-bottom:6px;flex-wrap:wrap;
    }
    .ns-card-user{ color:var(--txt3); }
    .ns-color-dot{
        width:8px;height:8px;border-radius:50%;flex-shrink:0;
    }
    .ns-conn-pill{
        font-size:10px;font-weight:600;border-radius:8px;
        padding:2px 8px;border:1px solid transparent;
    }
    .ns-conn-ok  { background:rgba(0,230,130,.15);color:#00e682;border-color:rgba(0,230,130,.3); }
    .ns-conn-wait{ background:rgba(255,200,0,.12);color:#ffc800;border-color:rgba(255,200,0,.25); }
    .ns-activity-ts{ font-size:9px;color:#2a3a4a;margin-left:auto; }
    /* Tier S #3 — Recording badge + button states */
    .ns-rec-badge{
        font-size:9px;font-weight:700;letter-spacing:0.4px;
        background:rgba(239,68,68,.18);color:#ef4444;
        border:1px solid rgba(239,68,68,.32);
        padding:1px 6px;border-radius:8px;
        animation: nsRecPulse 1.6s ease-in-out infinite;
    }
    @keyframes nsRecPulse {
        0%, 100% { opacity: 1; }
        50%      { opacity: 0.55; }
    }
    .ns-act-recording{
        background:rgba(239,68,68,.18) !important;
        color:#ef4444 !important;
        border-color:rgba(239,68,68,.30) !important;
    }
    .ns-act-play{
        background:rgba(96,165,250,.12) !important;
        color:#60a5fa !important;
        border-color:rgba(96,165,250,.24) !important;
    }
    .ns-act-play:hover{ background:rgba(96,165,250,.22) !important; }

    /* Bootstrap env badges on card */
    .ns-card-env{
        display:flex;flex-wrap:wrap;gap:4px;
        margin-bottom:6px;
    }
    .ns-env-tag{
        font-size:10px;background:rgba(255,255,255,.07);
        border:1px solid var(--bdr);border-radius:5px;
        padding:1px 6px;color:var(--txt2);
        white-space:nowrap;
    }

    /* Card action buttons */
    .ns-card-actions{
        display:flex;gap:5px;flex-wrap:wrap;
    }
    .ns-act-btn{
        font-size:11px;font-weight:600;border-radius:5px;
        padding:3px 10px;cursor:pointer;border:1px solid var(--bdr);
        background:rgba(255,255,255,.06);color:var(--txt2);
        transition:.15s;
    }
    .ns-act-btn:hover{ background:rgba(255,255,255,.12);color:var(--txt); }
    .ns-act-open   { color:var(--acc);border-color:rgba(16,185,129,.25); }
    .ns-act-open:hover{ background:rgba(16,185,129,.1); }
    .ns-act-close  { color:#ff6b6b;border-color:rgba(255,107,107,.25); }
    .ns-act-close:hover{ background:rgba(255,107,107,.1); }
    .ns-act-connect{ color:#60b8ff;border-color:rgba(96,184,255,.3); }
    .ns-act-connect:hover{ background:rgba(96,184,255,.1); }
    .ns-act-edit   { color:var(--txt3); }
    .ns-act-edit:hover{ color:var(--txt); }

    /* Empty state */
    .ns-empty-hosts{
        flex:1;display:flex;flex-direction:column;
        align-items:center;justify-content:center;
        gap:10px;padding:32px 16px;
        color:var(--txt3);font-size:13px;text-align:center;
    }

    /* Workspace panel */
    .ns-workspace{ flex:1;display:flex;flex-direction:column;overflow:hidden;min-width:0; }

    /* Welcome / capabilities screen */
    .ns-welcome{
        flex:1;display:flex;flex-direction:column;
        align-items:center;justify-content:center;
        padding:32px 24px;gap:12px;text-align:center;
        overflow-y:auto;
    }
    .ns-welcome-ico{ font-size:42px; }
    .ns-welcome-title{
        font-size:18px;font-weight:700;color:var(--txt);margin:0;
    }
    .ns-welcome-sub{
        font-size:13px;color:var(--txt3);margin:0;max-width:420px;
    }
    .ns-caps-grid{
        display:grid;
        grid-template-columns:repeat(auto-fill,minmax(200px,1fr));
        gap:8px;
        width:100%;max-width:600px;
        margin-top:8px;
    }
    .ns-cap-item{
        display:flex;align-items:center;gap:8px;
        background:rgba(255,255,255,.04);
        border:1px solid var(--bdr);border-radius:8px;
        padding:8px 12px;font-size:12px;color:var(--txt2);
        text-align:left;
    }
    .ns-cap-item span:first-child{ font-size:16px;flex-shrink:0; }

    /* Session tabs */
    .ns-session-tabs{ display:flex;gap:1px;background:var(--bg);padding:4px 8px 0;border-bottom:1px solid var(--bdr);flex-wrap:nowrap;overflow-x:auto;flex-shrink:0; }
    .ns-session-tabs::-webkit-scrollbar{ height:3px; }
    .ns-stab{ display:flex;align-items:center;gap:5px;padding:5px 10px;border-radius:6px 6px 0 0;border:1px solid transparent;background:rgba(255,255,255,.04);color:var(--txt2);font-size:11px;cursor:pointer;transition:.15s;white-space:nowrap;flex-shrink:0; }
    .ns-stab:hover{ background:rgba(255,255,255,.08);color:var(--txt); }
    .ns-stab-active{ background:var(--bg2);border-color:var(--bdr);border-bottom-color:var(--bg2);color:var(--txt); }
    .ns-stab-ico{ font-size:13px; }
    .ns-stab-name{ max-width:110px;overflow:hidden;text-overflow:ellipsis; }
    .ns-stab-dot.ok{ color:var(--acc); animation:ns-dot-pulse 2.4s ease-in-out infinite; }
    @keyframes ns-dot-pulse{ 0%,100%{ text-shadow:0 0 0 transparent; opacity:.8 } 50%{ text-shadow:0 0 6px var(--acc); opacity:1 } }
    .ns-stab-dot.wait{ color:var(--amber); }
    .ns-stab-spin{ color:var(--amber);animation:spin 1s linear infinite; }
    .ns-stab-close{ background:none;border:none;color:#475569;cursor:pointer;padding:0 2px;font-size:10px;line-height:1;border-radius:3px;transition:.15s; }
    .ns-stab-close:hover{ background:rgba(255,107,107,.2);color:#ff6b6b; }

    /* Shell wrap inside workspace */
    .ns-shell-wrap{ flex:1;display:flex;flex-direction:column;overflow:hidden; }
    .ns-shell-hdr{ display:flex;align-items:center;justify-content:space-between;padding:10px 16px;background:linear-gradient(180deg, rgba(16,185,129,.05), transparent 60%),var(--bg2);border-bottom:1px solid var(--bdr);box-shadow:inset 0 2px 0 rgba(16,185,129,.45);flex-shrink:0;animation:ns-hdr-in .26s cubic-bezier(.16,1,.3,1); }
    @keyframes ns-hdr-in{from{opacity:0;transform:translateY(-4px);}to{opacity:1;transform:none;}}

    /* ── RDP Copilot styles ─────────────────────────────────────────────── */
    .rshell-badge.rdp{background:rgba(99,102,241,.18);color:#818cf8;border:1px solid rgba(99,102,241,.3);}
    .rdp-reconnect-btn{background:rgba(99,102,241,.15)!important;color:#818cf8!important;border-color:rgba(99,102,241,.4)!important;font-size:11px!important;padding:2px 7px!important;font-weight:700;}
    .rdp-reconnect-btn:hover{background:rgba(99,102,241,.28)!important;}

    /* ── RDP Agent button ───────────────────────────────────────────────── */
    .rdp-agent-btn{background:rgba(16,185,129,.12)!important;color:#34d399!important;border-color:rgba(16,185,129,.35)!important;font-size:11px!important;padding:2px 8px!important;font-weight:700;display:flex;align-items:center;gap:4px;}
    .rdp-agent-btn:hover{background:rgba(16,185,129,.25)!important;}
    .rdp-agent-btn.rdp-agent-active{background:rgba(16,185,129,.3)!important;animation:agent-glow 1.8s ease-in-out infinite;}
    @keyframes agent-glow{0%,100%{box-shadow:0 0 0 rgba(52,211,153,0);}50%{box-shadow:0 0 8px rgba(52,211,153,.45);}}
    .rdp-agent-spinner{font-size:12px;display:inline-block;animation:spin 1.2s linear infinite;}
    @keyframes spin{to{transform:rotate(360deg);}}

    /* ── Incident Mode (SRE) active button ──────────────────────────────── */
    .rs-feat-incident-active{background:rgba(239,68,68,.28)!important;color:#fca5a5!important;border-color:rgba(239,68,68,.5)!important;animation:incident-pulse 1.8s ease-in-out infinite;}
    @keyframes incident-pulse{0%,100%{box-shadow:0 0 0 rgba(239,68,68,0);}50%{box-shadow:0 0 8px rgba(239,68,68,.55);}}

    /* ── RDP Agent Panel ────────────────────────────────────────────────── */
    .rdp-agent-panel{
      border-top:1px solid rgba(16,185,129,.25);
      border-bottom:1px solid rgba(16,185,129,.25);
      background:rgba(16,185,129,.04);
      flex-shrink:0;
      display:flex;flex-direction:column;gap:0;
      max-height:420px;overflow:hidden;
    }
    .rdp-agent-config-row{
      display:flex;align-items:center;gap:6px;
      padding:8px 12px;
      border-bottom:1px solid rgba(16,185,129,.15);
      flex-shrink:0;
      flex-wrap:wrap;
    }
    .rdp-agent-provider-select,
    .rdp-agent-model-select{
      background:rgba(0,0,0,.35);border:1px solid rgba(16,185,129,.3);
      border-radius:4px;color:#d1fae5;font-size:11px;padding:4px 6px;outline:none;
      flex-shrink:0;
    }
    .rdp-agent-provider-select:focus,
    .rdp-agent-model-select:focus{
      border-color:rgba(16,185,129,.6);
    }
    .rdp-agent-provider-select:disabled,
    .rdp-agent-model-select:disabled{
      opacity:.5;cursor:default;
    }
    .rdp-agent-model-fixed{
      background:rgba(16,185,129,.12);
      border:1px solid rgba(16,185,129,.35);
      border-radius:4px;
      color:#d1fae5;
      font-size:11px;
      font-weight:500;
      padding:4px 10px;
      white-space:nowrap;
      display:inline-flex;
      align-items:center;
      flex-shrink:0;
      cursor:default;
    }
    .rdp-agent-health-btn{
      background:rgba(168,85,247,.2);border:1px solid rgba(168,85,247,.4);
      border-radius:4px;color:#d8b4fe;cursor:pointer;font-size:11px;
      padding:4px 6px;white-space:nowrap;transition:.12s;flex-shrink:0;
    }
    .rdp-agent-health-btn:hover:not(:disabled){background:rgba(168,85,247,.3);}
    .rdp-agent-health-btn:disabled{opacity:.4;cursor:default;}
    .rdp-agent-task-row{
      display:flex;align-items:center;gap:8px;
      padding:8px 12px;
      border-bottom:1px solid rgba(16,185,129,.15);
      flex-shrink:0;
    }
    .rdp-agent-ico{font-size:15px;flex-shrink:0;}
    .rdp-agent-task-input{
      flex:1;background:rgba(0,0,0,.35);border:1px solid rgba(16,185,129,.3);
      border-radius:5px;color:#d1fae5;font-size:12px;padding:5px 9px;outline:none;
    }
    .rdp-agent-task-input:focus{border-color:rgba(16,185,129,.6);}
    .rdp-agent-task-input::placeholder{color:#064e3b;opacity:.8;}
    .rdp-agent-task-input:disabled{opacity:.5;}
    .rdp-agent-run-btn{
      background:rgba(16,185,129,.25);border:1px solid rgba(16,185,129,.5);
      border-radius:5px;color:#34d399;cursor:pointer;font-size:11px;font-weight:700;
      padding:5px 12px;white-space:nowrap;transition:.12s;flex-shrink:0;
    }
    .rdp-agent-run-btn:hover:not(:disabled){background:rgba(16,185,129,.4);}
    .rdp-agent-run-btn:disabled{opacity:.4;cursor:default;}
    .rdp-agent-stop-btn{
      background:rgba(239,68,68,.18);border:1px solid rgba(239,68,68,.4);
      border-radius:5px;color:#f87171;cursor:pointer;font-size:11px;font-weight:700;
      padding:5px 12px;white-space:nowrap;transition:.12s;flex-shrink:0;
    }
    .rdp-agent-stop-btn:hover{background:rgba(239,68,68,.3);}

    .rdp-agent-body{
      display:grid;grid-template-columns:1fr 220px;
      flex:1;overflow:hidden;min-height:0;
    }
    .rdp-agent-screen-wrap{
      position:relative;padding:8px;border-right:1px solid rgba(16,185,129,.15);
      overflow:hidden;display:flex;flex-direction:column;gap:4px;min-height:0;
    }
    .rdp-agent-screen{
      width:100%;height:auto;max-height:280px;object-fit:contain;
      border:1px solid rgba(16,185,129,.2);border-radius:4px;
      background:#000;cursor:zoom-in;
    }
    .rdp-agent-screen-label{
      font-size:10px;color:#065f46;display:flex;align-items:center;gap:5px;flex-shrink:0;
    }
    .rdp-agent-pulse{color:#34d399;animation:pulse-fade 1s ease-in-out infinite;}
    @keyframes pulse-fade{0%,100%{opacity:1;}50%{opacity:.3;}}

    .rdp-agent-log{
      display:flex;flex-direction:column;gap:2px;
      overflow-y:auto;padding:8px 10px;font-size:10.5px;
      scrollbar-width:thin;scrollbar-color:rgba(16,185,129,.2) transparent;
    }
    .rdpa-entry{padding:2px 0;line-height:1.4;word-break:break-word;}
    .rdpa-action{color:#6ee7b7;}
    .rdpa-text{color:#a7f3d0;font-style:italic;}
    .rdpa-shot{color:#047857;opacity:.8;}
    .rdpa-done{color:#34d399;font-weight:700;}
    .rdpa-error{color:#f87171;}
    .rdpa-thinking{color:#6ee7b7;opacity:.7;display:flex;align-items:center;gap:5px;}
    .rdpa-dots{font-size:16px;letter-spacing:2px;animation:blink 1.2s ease-in-out infinite;}
    @keyframes blink{0%,100%{opacity:.3;}50%{opacity:1;}}

    .rdp-agent-hint{
      padding:8px 14px;font-size:11px;color:#065f46;
      line-height:1.5;background:rgba(16,185,129,.03);
    }
    /* Banner */
    .rdp-banner{display:flex;align-items:center;gap:8px;padding:5px 14px;background:rgba(99,102,241,.07);border-bottom:1px solid rgba(99,102,241,.2);flex-shrink:0;}
    .rdp-banner-ico{color:#818cf8;font-size:13px;flex-shrink:0;}
    .rdp-banner-txt{font-size:11px;color:#6366f1;line-height:1.4;}
    /* Clipboard strip */
    .rdp-clip-strip{display:flex;align-items:center;gap:8px;padding:7px 14px;background:rgba(99,102,241,.06);border-top:1px solid rgba(99,102,241,.2);flex-shrink:0;}
    .rdp-clip-label{font-size:11px;color:#818cf8;white-space:nowrap;flex-shrink:0;}
    .rdp-clip-cmd{flex:1;font-family:var(--mono);font-size:11px;color:#c7d2fe;background:rgba(0,0,0,.3);border:1px solid rgba(99,102,241,.2);border-radius:4px;padding:3px 8px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
    .rdp-clip-copy{background:rgba(99,102,241,.2);border:1px solid rgba(99,102,241,.4);border-radius:5px;color:#818cf8;cursor:pointer;font-size:11px;font-weight:700;padding:3px 10px;white-space:nowrap;transition:.12s;flex-shrink:0;}
    .rdp-clip-copy:hover{background:rgba(99,102,241,.35);}
    .rdp-clip-dismiss{background:none;border:none;color:#334155;cursor:pointer;font-size:13px;padding:0 3px;flex-shrink:0;}
    .rdp-clip-dismiss:hover{color:var(--red);}
    /* Result paste input */
    .rdp-result-wrap{border-top:1px solid rgba(99,102,241,.2)!important;}
    .rdp-result-box{resize:vertical;font-family:var(--mono);font-size:12px;min-height:56px;max-height:160px;}

    /* ── Ctrl+F search bar ──────────────────────────────────────────────── */
    .ns-search-bar{display:flex;align-items:center;gap:6px;padding:5px 10px;background:#0a0d14;border-bottom:1px solid #1e3a5f;flex-shrink:0;}
    .ns-search-ico{color:#334155;font-size:13px;flex-shrink:0;}
    .ns-search-input{flex:1;background:#070a10;border:1px solid #1e293b;border-radius:4px;color:var(--txt);font-size:12px;font-family:var(--mono);padding:3px 8px;outline:none;min-width:0;}
    .ns-search-input:focus{border-color:#1e4a7f;box-shadow:0 0 0 2px rgba(30,74,127,0.25);}
    .ns-search-count{font-size:11px;color:#475569;white-space:nowrap;flex-shrink:0;font-family:var(--mono);}
    .ns-search-count.ns-search-zero{color:var(--red);}
    .ns-search-nav{background:none;border:1px solid #1e293b;border-radius:3px;color:#475569;cursor:pointer;font-size:11px;padding:1px 6px;transition:.12s;}
    .ns-search-nav:hover{background:rgba(255,255,255,0.04);color:var(--txt2);}
    .ns-search-close{background:none;border:none;color:#334155;cursor:pointer;font-size:13px;padding:0 3px;transition:.12s;flex-shrink:0;}
    .ns-search-close:hover{color:var(--red);}
    /* ── Match highlights ───────────────────────────────────────────────── */
    .rsl-search-match{background:rgba(251,191,36,0.07);border-left:2px solid rgba(251,191,36,0.4)!important;padding-left:6px;}
    .rsl-search-current{background:rgba(251,191,36,0.16)!important;border-left:2px solid #fbbf24!important;}

    /* Input toggle bar */
    .ns-input-toggle-bar{
        display:flex;align-items:center;justify-content:center;gap:10px;
        padding:3px 0;cursor:pointer;
        background:var(--bg2);border-top:1px solid var(--bdr);
        transition:background .15s;flex-shrink:0;
        user-select:none;
    }
    .ns-input-toggle-bar:hover{ background:#0d1520; }
    .ns-input-toggle-ico{ font-size:10px;color:#3a5a7a;font-weight:600;letter-spacing:.05em; }
    .ns-input-toggle-hint{ font-size:9px;color:#2a3a4a; }

    /* ── REMOTE SHELL STYLES ─────────────────────────────────────────────── */
    .rshell-hdr-left{display:flex;align-items:center;gap:12px;}
    .rshell-ico{display:inline-flex;align-items:center;justify-content:center;width:42px;height:42px;border-radius:12px;flex-shrink:0;color:var(--acc);background:rgba(16,185,129,.12);border:1px solid rgba(16,185,129,.28);box-shadow:0 0 18px -4px rgba(16,185,129,.5);}
    .rshell-title{font-size:14px;font-weight:600;color:white;}
    .rshell-sub{font-size:11px;color:#475569;font-family:var(--mono);margin-top:2px;display:flex;align-items:center;gap:8px;}
    .rshell-badge{font-size:10px;font-weight:700;padding:1px 6px;border-radius:10px;}
    .rshell-badge.ok{color:#10b981;background:rgba(16,185,129,.1);}
    .rshell-badge.err{color:#ef4444;background:rgba(255,68,68,.1);}

    /* Context Badges (bootstrap) */
    .rs-ctx-badge{font-size:10px;font-weight:600;padding:1px 7px;border-radius:10px;white-space:nowrap;flex-shrink:0;transition:opacity .2s;}
    .ctx-git   {color:#00cc78;background:rgba(0,204,120,.12);border:1px solid rgba(0,204,120,.2);}
    .ctx-k8s   {color:#6496ff;background:rgba(100,150,255,.12);border:1px solid rgba(100,150,255,.2);}
    .ctx-docker{color:#2496ed;background:rgba(36,150,237,.12);border:1px solid rgba(36,150,237,.2);}
    .ctx-node  {color:#68a063;background:rgba(104,160,99,.12);border:1px solid rgba(104,160,99,.2);}
    .ctx-venv  {color:#ffb86c;background:rgba(255,184,108,.12);border:1px solid rgba(255,184,108,.2);}
    .ctx-loading{color:#2a3a4a;background:transparent;border:none;animation:ctx-pulse 1.5s ease-in-out infinite;}
    @keyframes ctx-pulse{0%,100%{opacity:.4}50%{opacity:1}}

    /* Feature buttons */
    .rshell-toolbar-sep{display:inline-block;width:1px;height:18px;background:rgba(255,255,255,.08);margin:0 4px;flex-shrink:0;}
    .rshell-feat-btn{background:rgba(255,255,255,.04);border:1px solid #1a2030;border-radius:5px;color:#0f7b5a;cursor:pointer;font-size:13px;padding:3px 7px;transition:.15s;line-height:1;}
    .rshell-feat-btn:hover{background:rgba(16,185,129,.08);border-color:rgba(16,185,129,.2);color:var(--acc);transform:translateY(-1px);}
    .rshell-feat-btn.rs-feat-danger:hover{background:rgba(239,68,68,.1);border-color:rgba(239,68,68,.3);color:#f87171;}
    .rs-feat-active{background:rgba(16,185,129,.1)!important;border-color:rgba(16,185,129,.3)!important;color:var(--acc)!important;}

    /* Autocomplete suggestion */
    .rs-suggestion{position:absolute;top:0;left:0;right:0;bottom:0;display:flex;align-items:center;padding:7px 10px;font-family:var(--mono);font-size:12px;color:#2a3a4a;pointer-events:none;white-space:pre;overflow:hidden;}
    .rs-sugg-ai{color:#5a3a7a;}
    .rs-sugg-ai span:last-child{color:#7a4a9a;font-style:italic;}
    .rs-ai-spinner{display:flex;align-items:center;gap:5px;padding:2px 4px;font-size:10px;color:#5a3a7a;font-family:var(--mono);}
    .rs-ai-spin-dot{animation:ai-pulse 1.2s ease-in-out infinite;}
    @keyframes ai-pulse{0%,100%{opacity:.3}50%{opacity:1}}
    .rs-bg-badge{font-size:9px;font-weight:700;padding:1px 6px;border-radius:8px;background:rgba(90,58,122,.25);border:1px solid rgba(120,80,160,.3);color:#9a6aba;font-family:var(--mono);margin-left:6px;}

    /* Output area */
    .rshell-out{flex:1;overflow-y:auto;padding:12px 16px;font-family:var(--mono);font-size:12px;background:#020407;display:flex;flex-direction:column;gap:3px;}
    .ns-show-more{display:block;width:100%;padding:6px 0;background:rgba(96,165,250,.06);border:1px dashed rgba(96,165,250,.2);border-radius:4px;color:var(--accent,#60a5fa);font-size:10px;font-family:var(--mono);cursor:pointer;text-align:center;margin-bottom:4px;transition:.15s;}
    .ns-show-more:hover{background:rgba(96,165,250,.12);border-color:rgba(96,165,250,.35);}
    .rshell-line{display:flex;align-items:flex-start;gap:8px;padding:2px 0;border-bottom:1px solid rgba(26,32,48,.3);flex-wrap:wrap;content-visibility:auto;contain-intrinsic-size:0 28px;}
    .rsl-time{margin-left:auto;font-size:10px;color:#1a2030;flex-shrink:0;align-self:center;}
    .rsl-prompt{flex-shrink:0;font-weight:700;color:#0f7b5a;user-select:none;}
    .lucy-dot{color:var(--acc)!important;}
    .rsl-cmd{color:var(--acc);flex:1;word-break:break-all;}
    .rsl-lucy-in{color:#aaa;flex:1;font-family:var(--font-sans);font-size:12px;}
    .rsl-lucy-out{color:#8ba8c8;flex:1;font-family:var(--font-sans);font-size:12px;line-height:1.6;white-space:pre-wrap;}
    .rsl-out-txt{color:#7a8a9a;flex:1;margin:0;white-space:pre-wrap;word-break:break-all;max-height:200px;overflow-y:auto;}
    .rsl-err-txt{color:#ef4444;flex:1;white-space:pre-wrap;}
    .rsl-info-txt{color:#0f7b5a;flex:1;}
    .rsl-running .rsl-spin{color:#475569;animation:spin .8s linear infinite;display:inline-block;}

    /* Streaming */
    .rsl-live-block{display:flex;flex-direction:column;gap:0;border-left:2px solid rgba(16,185,129,.25);margin:2px 0;background:rgba(16,185,129,.02);border-radius:0 4px 4px 0;}
    .rsl-live-hdr{display:flex;align-items:center;gap:7px;padding:4px 10px;background:rgba(16,185,129,.04);border-bottom:1px solid rgba(16,185,129,.08);}
    .rsl-live-dot{width:7px;height:7px;border-radius:50%;background:var(--acc);animation:stream-blink .7s ease-in-out infinite, rsl-dot-glow 1.6s ease-in-out infinite;flex-shrink:0;}
    @keyframes rsl-dot-glow{ 0%,100%{ box-shadow:0 0 4px 0 rgba(16,185,129,.30) } 50%{ box-shadow:0 0 9px 1px rgba(16,185,129,.62) } }
    .rsl-live-label{color:#0d9668;font-size:11px;flex:1;}
    .rsl-watchdog{font-size:9.5px;color:#0d9668;background:rgba(16,185,129,.10);border:1px solid rgba(16,185,129,.22);border-radius:7px;padding:1px 6px;flex-shrink:0;font-family:var(--mono);letter-spacing:.3px;}
    .rsl-live-input-btn{background:rgba(100,149,255,.1);border:1px solid rgba(100,149,255,.25);border-radius:4px;color:#6495ff;cursor:pointer;font-size:10px;padding:2px 7px;transition:.15s;flex-shrink:0;}
    .rsl-live-input-btn:hover{background:rgba(100,149,255,.2);}
    .rsl-cancel-btn{background:rgba(255,68,68,.1);border:1px solid rgba(255,68,68,.25);border-radius:4px;color:#ff6464;cursor:pointer;font-size:10px;font-weight:600;padding:2px 8px;transition:.15s;flex-shrink:0;}
    .rsl-cancel-btn:hover{background:rgba(255,68,68,.2);}
    .rsl-live-pre{color:#7aaa8a;margin:0;padding:6px 10px;white-space:pre-wrap;word-break:break-all;font-size:11.5px;font-family:var(--mono);line-height:1.5;}

    /* ── Live reasoning bubble (NexShell port) ── */
    .ns-reasoning{
      width:100%;
      margin:4px 0;
      border-radius:6px;
      background:rgba(167,139,250,.04);
      border:1px solid rgba(167,139,250,.14);
      border-left:2px solid transparent;
      overflow:hidden;
      transition:background .25s, border-color .25s;
    }
    .ns-reasoning.nr-active{
      background:linear-gradient(110deg, rgba(167,139,250,.06) 0%, rgba(99,102,241,.10) 50%, rgba(167,139,250,.06) 100%);
      background-size:200% 100%;
      animation:nrShimmer 2.4s linear infinite;
      border-left-color:#a78bfa;
      box-shadow:0 0 0 1px rgba(167,139,250,.10), 0 4px 14px -8px rgba(99,102,241,.35);
    }
    .ns-reasoning.nr-done{
      background:rgba(255,255,255,.015);
      border-left-color:rgba(167,139,250,.35);
    }
    @keyframes nrShimmer{0%{background-position:0% 50%;}100%{background-position:200% 50%;}}
    .nr-head{
      display:flex;align-items:center;gap:8px;
      width:100%;
      padding:6px 11px;
      background:transparent;border:0;
      color:#cbd5e1;font-size:11.5px;font-weight:500;
      cursor:pointer;text-align:left;font-family:inherit;
    }
    .nr-head:hover{background:rgba(255,255,255,.02);}
    .nr-icon{font-size:13px;}
    .nr-active .nr-icon{animation:nrPulse 1.6s ease-in-out infinite;}
    @keyframes nrPulse{0%,100%{opacity:.55;transform:scale(1);}50%{opacity:1;transform:scale(1.15);}}
    .nr-title{flex:1;letter-spacing:.1px;}
    .nr-active .nr-title{
      background:linear-gradient(90deg,#cbd5e1 0%,#a78bfa 50%,#cbd5e1 100%);
      background-size:200% auto;
      -webkit-background-clip:text;background-clip:text;
      -webkit-text-fill-color:transparent;
      animation:nrTextShine 2.4s linear infinite;
    }
    @keyframes nrTextShine{0%{background-position:0% 50%;}100%{background-position:200% 50%;}}
    .nr-timer{
      font-family:var(--mono);font-size:10px;color:#a78bfa;
      background:rgba(167,139,250,.10);
      padding:1px 7px;border-radius:10px;
      border:1px solid rgba(167,139,250,.20);
    }
    .nr-chev{font-size:10px;opacity:.55;}
    .nr-body{
      margin:0;padding:2px 14px 10px;
      font-size:11px;line-height:1.55;color:#94a3b8;
      font-family:var(--mono);white-space:pre-wrap;
      border-top:1px solid rgba(167,139,250,.08);
      max-height:300px;overflow-y:auto;
    }

    /* ── Tool cards (NexShell port) ── */
    .ns-toolcard{
      width:100%;
      margin:4px 0;
      border:1px solid rgba(255,255,255,.07);
      border-left:2px solid rgba(167,139,250,.4);
      border-radius:6px;
      background:rgba(255,255,255,.015);
      overflow:hidden;
      transition:border-color .25s, background .25s;
    }
    .ns-toolcard.ntc-running{
      border-left-color:#a78bfa;
      background:linear-gradient(110deg, rgba(167,139,250,.05) 0%, rgba(99,102,241,.09) 50%, rgba(167,139,250,.05) 100%);
      background-size:200% 100%;
      animation:nrShimmer 2.4s linear infinite;
    }
    .ns-toolcard.ntc-done{border-left-color:#10b981;}
    .ns-toolcard.ntc-error{border-left-color:#ef4444;background:rgba(239,68,68,.04);}
    .ns-toolcard .ntc-head{
      display:flex;align-items:center;gap:9px;
      padding:6px 11px;cursor:pointer;list-style:none;
      font-size:11.5px;color:#cbd5e1;user-select:none;
    }
    .ns-toolcard .ntc-head::-webkit-details-marker{display:none;}
    .ns-toolcard .ntc-head:hover{background:rgba(255,255,255,.025);}
    .ns-toolcard .ntc-icon{font-size:13px;flex-shrink:0;}
    .ns-toolcard .ntc-label{
      flex:1;font-family:var(--mono);font-size:11px;
      overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:#cbd5e1;
    }
    .ns-toolcard .ntc-dur{
      font-family:var(--mono);font-size:10px;color:#94a3b8;
      background:rgba(255,255,255,.04);padding:1px 6px;border-radius:8px;
    }
    .ns-toolcard .ntc-status{
      font-size:11px;font-weight:700;min-width:14px;text-align:center;
    }
    .ntc-running .ntc-status{color:#a78bfa;}
    .ntc-done .ntc-status{color:#10b981;}
    .ntc-error .ntc-status{color:#ef4444;}
    .ntc-spinner{
      display:inline-block;width:10px;height:10px;
      border:1.5px solid rgba(167,139,250,.25);
      border-top-color:#a78bfa;
      border-radius:50%;
      animation:ntcSpin .7s linear infinite;
    }
    @keyframes ntcSpin{to{transform:rotate(360deg);}}
    .ns-toolcard .ntc-body{
      margin:0;padding:8px 12px;
      font-family:var(--mono);font-size:11px;line-height:1.5;
      color:#94a3b8;background:rgba(0,0,0,.18);
      border-top:1px solid rgba(255,255,255,.04);
      white-space:pre-wrap;word-break:break-word;
      max-height:280px;overflow-y:auto;
    }
    .rsl-live-cursor{display:inline-block;width:7px;height:12px;background:var(--acc);border-radius:1px;vertical-align:middle;margin-left:1px;animation:stream-blink .7s ease-in-out infinite;}
    @keyframes stream-blink{0%,100%{opacity:1;}50%{opacity:0;}}

    /* Exit code + duration */
    .rsl-meta-row{display:flex;align-items:center;gap:10px;padding:3px 0 2px;border-top:1px solid rgba(255,255,255,.04);margin-top:3px;}
    .rsl-exit-badge{font-size:10px;font-weight:700;padding:1px 8px;border-radius:10px;font-family:var(--mono);letter-spacing:.3px;}
    .rsl-exit-badge.ok{color:#10b981;background:rgba(16,185,129,.09);border:1px solid rgba(16,185,129,.2);}
    .rsl-exit-badge.err{color:#ff5555;background:rgba(255,68,68,.09);border:1px solid rgba(255,68,68,.2);}
    .rsl-dur{font-size:10px;color:#2a3a4a;font-family:var(--mono);}

    /* Interactive prompt */
    .rsl-iprompt-row{display:flex;align-items:center;gap:6px;padding:6px 10px;background:rgba(255,170,0,.05);border-top:1px solid rgba(255,170,0,.15);}
    .rsl-iprompt-hint{color:#f59e0b;font-size:11px;font-weight:600;white-space:nowrap;flex-shrink:0;}
    .rsl-iprompt-input{flex:1;background:var(--bg);border:1px solid rgba(255,170,0,.35);border-radius:4px;color:#fff;font-size:12px;font-family:var(--mono);padding:4px 8px;outline:none;transition:.15s;}
    .rsl-iprompt-input:focus{border-color:#f59e0b;box-shadow:0 0 0 2px rgba(255,170,0,.12);}
    .rsl-iprompt-send{background:rgba(255,170,0,.15);border:1px solid rgba(255,170,0,.35);border-radius:4px;color:#f59e0b;cursor:pointer;font-size:14px;padding:3px 8px;transition:.15s;flex-shrink:0;}
    .rsl-iprompt-send:hover{background:rgba(255,170,0,.25);}

    /* Input area */
    .rshell-inputs{flex-shrink:0;border-top:1px solid var(--bdr);}
    .rshell-input-wrap{padding:10px 14px;border-bottom:1px solid #0e1520;}
    .rs-direct{background:rgba(0,0,0,.3);}
    .rs-lucy{background:rgba(16,185,129,.02);}
    .rshell-input-label{display:flex;align-items:center;gap:6px;font-size:10px;color:#334155;margin-bottom:6px;font-weight:600;letter-spacing:.3px;text-transform:uppercase;}
    .rs-label-ico{font-size:11px;color:#0f7b5a;background:rgba(0,0,0,.4);padding:1px 5px;border-radius:3px;}
    .rs-lucy .rs-label-ico{color:var(--acc);}
    .rs-hint{margin-left:auto;font-size:10px;color:#1a2030;font-weight:400;text-transform:none;letter-spacing:0;}
    .rshell-input-row{display:flex;align-items:center;gap:8px;}
    .rsi-prompt{font-family:var(--mono);font-size:11px;color:#334155;flex-shrink:0;}
    .rsi-box{flex:1;background:rgba(0,0,0,.5);border:1px solid #1a2030;border-radius:6px;color:white;font-family:var(--mono);font-size:12px;padding:7px 10px;outline:none;transition:.15s;}
    .rsi-box:focus{border-color:#2a3a5a;}
    .rs-lucy-box{font-family:var(--font-sans);font-size:12px;}
    .rs-lucy-box:focus{border-color:rgba(16,185,129,.2);}
    .rs-lucy-ta{resize:none;min-height:34px;max-height:140px;line-height:1.45;overflow-y:auto;}
    .rsi-box:disabled{opacity:.4;cursor:not-allowed;}
    .rsi-send{background:rgba(255,255,255,.06);border:1px solid var(--bdr);border-radius:6px;color:#475569;cursor:pointer;font-size:12px;padding:6px 10px;transition:.15s;flex-shrink:0;}
    .rsi-send:hover:not(:disabled){background:rgba(255,255,255,.1);color:white;}
    .rsi-send:disabled{opacity:.3;cursor:not-allowed;}
    .rs-lucy-send:not(:disabled){border-color:rgba(16,185,129,.2);color:var(--acc);}
    .rs-lucy-send:hover:not(:disabled){background:rgba(16,185,129,.08);}

    /* v1.4.22 — Broadcast modal .bc-* family extracted to
       $lib/styles/nexshell.css (imported from <script>). The
       page.css copy that was silently overriding these rules is
       also deleted. Visual output unchanged (page.css's hex
       values are the ones that were actually rendering — the
       theme-variable versions here never reached the DOM). */

    /* Playbooks */
    .pb-item{background:var(--bg2);border:1px solid var(--bdr);border-radius:7px;padding:10px 12px;}
    .pb-name{font-size:12px;font-weight:600;color:var(--acc);margin-bottom:4px;}
    .pb-cmds{font-size:10px;color:#475569;font-family:var(--mono);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}

    /* Tail log presets */
    .rs-log-preset{background:rgba(255,255,255,.04);border:1px solid #1a2030;border-radius:4px;color:#475569;cursor:pointer;font-size:10px;font-family:var(--mono);padding:3px 8px;transition:.1s;}
    .rs-log-preset:hover{background:rgba(16,185,129,.06);color:var(--acc);border-color:rgba(16,185,129,.2);}

    /* Spin animation */
    .rsl-spin{color:#475569;animation:spin .8s linear infinite;display:inline-block;font-size:12px;}
    @keyframes spin{to{transform:rotate(360deg);}}

    /* ── Modal styles (scoped) ───────────────────────────────────────────── */
    .mb{position:fixed;inset:0;background:rgba(4,8,14,.92);backdrop-filter:blur(6px);z-index:var(--z-mb, 4000);display:flex;justify-content:center;align-items:center;}
    .mbox{background:var(--bg2);border:1px solid var(--bdr2);border-radius:12px;padding:28px;max-height:85vh;overflow-y:auto;box-shadow:0 20px 60px rgba(0,0,0,.6);}
    .mbox.sm{width:380px;}.mbox.md{width:440px;}.mbox.lg{width:520px;}
    .mhdr{display:flex;justify-content:space-between;align-items:center;border-bottom:1px solid var(--bdr);padding-bottom:14px;margin-bottom:18px;}
    .mtitle{color:white;margin:0;font-size:15px;font-weight:600;display:flex;align-items:center;gap:8px;}
    .mclose{background:transparent;border:none;color:var(--txt2);font-size:18px;cursor:pointer;padding:2px 6px;border-radius:4px;transition:.15s;line-height:1;}
    .mclose:hover{color:var(--red);background:rgba(255,68,68,.08);}
    .mbtn{padding:9px 16px;border-radius:6px;cursor:pointer;font-size:13px;font-weight:600;font-family:inherit;transition:.15s;border:1px solid var(--bdr);}
    .mbtn.pri{background:var(--acc);color:#000;border:none;}.mbtn.pri:hover{opacity:.85;}
    .mbtn.ghost{background:transparent;color:var(--txt2);}.mbtn.ghost:hover{background:rgba(255,255,255,.04);color:var(--txt);}
    .mbtn:disabled{opacity:.4;cursor:not-allowed;}
    .minp{width:100%;background:rgba(0,0,0,.3);border:1px solid var(--bdr2);color:white;padding:10px 12px;border-radius:7px;outline:none;font-family:inherit;font-size:13px;transition:border-color .2s;box-sizing:border-box;}
    .minp:focus{border-color:var(--acc-b);}

    /* ── View shared styles ──────────────────────────────────────────────── */
    .view-wrap{flex:1;display:flex;flex-direction:column;overflow:hidden;min-height:0;}
    .view-hdr{display:flex;align-items:center;padding:10px 16px;background:rgba(2,4,8,.6);border-bottom:1px solid var(--bdr);flex-shrink:0;gap:10px;}
    .view-title{font-size:13px;font-weight:700;color:var(--txt);white-space:nowrap;}

    /* ── Light theme overrides ───────────────────────────────────────────── */
    :global(:root.light) .ns-view          { background:var(--bg); }
    :global(:root.light) .ns-hdr           { background:#e8eef5;border-bottom-color:var(--bdr); }
    :global(:root.light) .ns-hosts-col     { background:#f0f4f8;border-right-color:var(--bdr); }
    :global(:root.light) .ns-workspace     { background:#f8fafc; }
    :global(:root.light) .ns-host-card     { background:rgba(0,0,0,.03);border-color:var(--bdr); }
    :global(:root.light) .ns-host-card:hover{ border-color:rgba(0,0,0,.18); }
    :global(:root.light) .ns-card-name     { color:var(--txt); }
    :global(:root.light) .ns-search        { background:rgba(0,0,0,.05);border-color:var(--bdr);color:var(--txt); }
    :global(:root.light) .ns-search:focus  { border-color:var(--acc);background:rgba(0,0,0,.08); }
    :global(:root.light) .ns-act-btn       { background:rgba(0,0,0,.05);border-color:var(--bdr);color:var(--txt2); }
    :global(:root.light) .ns-act-btn:hover { background:rgba(0,0,0,.1);color:var(--txt); }
    :global(:root.light) .ns-cap-item      { background:rgba(0,0,0,.03);border-color:var(--bdr); }
    :global(:root.light) .ns-env-tag       { background:rgba(0,0,0,.06);border-color:var(--bdr); }
    :global(:root.light) .ns-summary-badge { background:rgba(0,0,0,.06);border-color:var(--bdr); }
    :global(:root.light) .ns-sort-sel      { background:#f0f4f8;border-color:var(--bdr);color:var(--txt); }
    :global(:root.light) .ns-cat-chip      { background:rgba(0,0,0,.05);border-color:var(--bdr);color:var(--txt2); }
    :global(:root.light) .ns-cat-active    { background:rgba(0,168,107,.1);border-color:rgba(0,168,107,.3);color:var(--acc); }
    :global(:root.light) .ns-stab          { background:rgba(0,0,0,.04);color:var(--txt2);border-color:transparent; }
    :global(:root.light) .ns-stab:hover    { background:rgba(0,0,0,.08);color:var(--txt); }
    :global(:root.light) .ns-stab-active   { background:#fff;border-color:var(--bdr);border-bottom-color:#fff;color:var(--txt); }
    :global(:root.light) .ns-session-tabs  { background:#e4eaf0;border-bottom-color:var(--bdr); }
    :global(:root.light) .ns-shell-hdr     { background:#e8eef5;border-bottom-color:var(--bdr); }
    :global(:root.light) .ns-panel-toggle  { background:rgba(0,0,0,.05);border-color:var(--bdr);color:var(--txt2); }
    :global(:root.light) .ns-input-toggle-bar { background:#e8eef5;border-top-color:var(--bdr); }
    :global(:root.light) .ns-input-toggle-bar:hover { background:#dde5ee; }
    :global(:root.light) .ns-input-toggle-ico { color:var(--txt3); }
    :global(:root.light) .ns-input-toggle-hint { color:var(--txt3); }
    :global(:root.light) .ns-col-toolbar   { border-bottom-color:var(--bdr); }
    :global(:root.light) .ns-cat-chips     { border-bottom-color:var(--bdr); }
    :global(:root.light) .rshell-out       { background:#f6f8fb; }
    :global(:root.light) .rshell-line      { border-bottom-color:var(--bdr); }
    :global(:root.light) .rsl-prompt       { color:#00784e; }
    :global(:root.light) .rsl-cmd          { color:#0b6045; }
    :global(:root.light) .rsl-lucy-out     { color:#1e3a5a; }
    :global(:root.light) .rsl-out-txt      { color:#475569; }
    :global(:root.light) .rshell-inputs    { border-top-color:var(--bdr); }
    :global(:root.light) .rshell-input-wrap{ border-bottom-color:var(--bdr); background:#fcfcfc; }
    :global(:root.light) .rsi-box          { background:#fff; border-color:var(--bdr); color:var(--txt); }
    :global(:root.light) .ns-llm-select    { background:#ececec; color:#0e6a4e; border-color:rgba(16,185,129,.5); }
    :global(:root.light) .ns-llm-select option { background:#ffffff; color:#333; }
    :global(:root.light) .ns-llm-select optgroup { background:#f5f5f5; color:#555; }
    
    .ns-llm-select {
        background:rgba(0,0,0,0.4); color:var(--acc); 
        border:1px solid rgba(16,185,129,0.3); border-radius:4px; 
        font-size:10px; padding:2px 4px; outline:none; 
        font-family:var(--font-sans); cursor:pointer;
        transition: .15s;
    }
    .ns-llm-select option, .ns-llm-select optgroup {
        background: var(--bg2);
        color: var(--txt);
    }
    :global(:root.light) .rsi-box:focus    { border-color:var(--acc); }
    :global(:root.light) .rs-label-ico     { background:rgba(0,168,107,.1); color:#00784e; }
    :global(:root.light) .rsi-prompt       { color:#0f172a; }
    :global(:root.light) .rs-lucy          { background:rgba(16,185,129,.05); }
    :global(:root.light) .rs-direct        { background:#f1f5f9; }
    :global(:root.light) .rshell-input-label select { background:#fff !important; color:#00784e !important; border-color:var(--bdr) !important; }
</style>

