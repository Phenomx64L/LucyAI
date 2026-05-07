// ── working-memory.ts — Tab working-memory utilities ─────────────────────────
// Pure / nearly-pure functions extracted from +page.svelte.
// No Svelte or Tauri imports — safe to unit-test.

// ── buildWorkingMemoryDigest ─────────────────────────────────────────────────
// Builds a <500-token digest of tab state for injection into the system prompt.
export function buildWorkingMemoryDigest(tab: any): string {
    const wm = tab?.workingMemory;
    if (!wm) return '';
    const parts: string[] = [];
    if (wm.currentHost) {
        parts.push(`current_host: ${wm.currentHost.name} (${wm.currentHost.type}, id=${wm.currentHost.id})`);
    }
    if (wm.currentCwd) {
        parts.push(`current_cwd: ${wm.currentCwd}`);
    }
    if (wm.lastCommands?.length) {
        const lines = wm.lastCommands.map((c: any) => {
            const mark = c.ok ? '✓' : '✗';
            const detail = c.ok ? `${c.ms}ms` : (c.err ? c.err.slice(0, 80) : 'failed');
            return `  ${mark} [${c.target}] ${c.cmd.slice(0, 100)}${c.cmd.length > 100 ? '…' : ''} (${detail})`;
        }).join('\n');
        parts.push(`recent_cmds:\n${lines}`);
    }
    if (wm.recentFiles?.length) {
        const lines = (wm.recentFiles as any[]).slice(-5).map(f => {
            const ago = Math.max(1, Math.round((Date.now() - f.ts) / 1000));
            const agoLabel = ago < 60 ? `${ago}s` : ago < 3600 ? `${Math.round(ago / 60)}m` : `${Math.round(ago / 3600)}h`;
            return `  · ${f.op} ${f.path} (${agoLabel} ago)`;
        }).join('\n');
        parts.push(`recent_files:\n${lines}`);
    }
    if (wm.recentErrors?.length) {
        parts.push(`recent_errors:\n${(wm.recentErrors as string[]).map(e => `  · ${e.slice(0, 120)}`).join('\n')}`);
    }
    if (wm.activeIncident) {
        parts.push(`active_incident: ${wm.activeIncident.id} (phase: ${wm.activeIncident.phase})`);
    }
    if (!parts.length) return '';
    return `\n\n--- WORKING MEMORY (tab state) ---\n${parts.join('\n')}\n(Use this to avoid re-asking, re-creating files, or re-running successful commands. If last 2 cmds failed with the same cause, propose a DIFFERENT approach.)`;
}

// ── SlotRelevance ─────────────────────────────────────────────────────────────
export interface SlotRelevance {
    host: boolean;
    runbook: boolean;
    environment: boolean;
}

// Relevance heuristic for lazy slots — avoids inflating the system prompt
// needlessly, but biased toward inclusion ("better to over-recall than forget").
export function slotRelevance(userInput: string): SlotRelevance {
    const s = (userInput || '').toLowerCase();
    return {
        host: /\b(host|server|servidor|prod|test|dev|remote|remoto|ssh|winrm|invoke|rdp|iis|sql|nginx|apache|linux|windows)\b/.test(s)
            || /[a-z0-9]+-[a-z0-9]+-?\d*/i.test(s),
        runbook: /\b(how|como|cómo|fix|arregla|troubleshoot|diagnos|procedure|procedimiento|runbook|install|deploy|configure|configura|restart|reinicia|setup|crear|create|generar|generate|script|comando|command|ejecuta|run|edita|edit|crea archivo|new file)\b/.test(s),
        environment: true, // always-on: prevents "Lucy has dementia" symptom
    };
}

// ── WorkingMemoryEvent ────────────────────────────────────────────────────────
export type WMEventType = 'exec' | 'file' | 'cwd' | 'incident' | 'turn';

export interface WMEvent {
    type: WMEventType;
    // exec
    cmd?: string;
    target?: string;
    ok?: boolean;
    ms?: number;
    err?: string | null;
    host?: { id: string; name: string; type: string } | null;
    // file
    path?: string;
    op?: 'created' | 'edited' | 'read';
    // incident
    incident?: { id: string; phase: string } | null;
}

// ── updateWorkingMemory ───────────────────────────────────────────────────────
// Mutates tab.workingMemory in-place. Keeps arrays bounded.
export function updateWorkingMemory(tab: any, ev: WMEvent, logFn?: (type: string, sub: string, ms: number) => void): void {
    if (!tab) return;
    tab.workingMemory ??= {
        currentHost: null, currentCwd: null,
        lastCommands: [], recentFiles: [], recentErrors: [],
        activeIncident: null, turnCount: 0, compactedDigest: '',
    };
    const wm = tab.workingMemory;
    // Backfill any fields added after the tab was first serialized
    wm.lastCommands  ??= [];
    wm.recentFiles   ??= [];
    wm.recentErrors  ??= [];
    wm.currentHost   ??= null;
    wm.currentCwd    ??= null;
    wm.activeIncident ??= null;
    wm.turnCount     ??= 0;
    wm.compactedDigest ??= '';

    if (ev.type === 'exec') {
        wm.lastCommands.push({
            cmd:    (ev.cmd || '').slice(0, 160),
            target: ev.target || 'local',
            ok:     !!ev.ok,
            ms:     ev.ms || 0,
            ts:     Date.now(),
            err:    ev.err ? String(ev.err).slice(0, 200) : null,
        });
        if (wm.lastCommands.length > 5) wm.lastCommands.splice(0, wm.lastCommands.length - 5);
        if (ev.ok && ev.host) {
            wm.currentHost = { id: ev.host.id, name: ev.host.name, type: ev.host.type };
        }
        if (!ev.ok && ev.err) {
            wm.recentErrors.push(String(ev.err).slice(0, 200));
            if (wm.recentErrors.length > 3) wm.recentErrors.splice(0, wm.recentErrors.length - 3);
        }
        if (logFn) {
            const sub = (ev.target === 'local') ? 'local' : 'remote';
            logFn(ev.ok ? 'exec_success' : 'exec_failure', sub, ev.ms || 0);
        }
    } else if (ev.type === 'file') {
        const entry = {
            path: String(ev.path || '').slice(0, 200),
            op:   ev.op || 'edited',
            ts:   Date.now(),
        };
        wm.recentFiles = (wm.recentFiles as any[]).filter((f: any) => !(f.path === entry.path && f.op === entry.op));
        wm.recentFiles.push(entry);
        if (wm.recentFiles.length > 8) wm.recentFiles.splice(0, wm.recentFiles.length - 8);
    } else if (ev.type === 'cwd') {
        if (ev.path) wm.currentCwd = String(ev.path).slice(0, 300);
    } else if (ev.type === 'incident') {
        wm.activeIncident = ev.incident ? { id: ev.incident.id, phase: ev.incident.phase } : null;
    } else if (ev.type === 'turn') {
        wm.turnCount = (wm.turnCount || 0) + 1;
    }
}

// ── compactOldTurns ───────────────────────────────────────────────────────────
// Compacts the first half of long tabs into a short digest.
// Threshold: > 10 verbatim turns. Returns { keepFrom, digest }.
export function compactOldTurns(tab: any, userName: string): { keepFrom: number; digest: string } {
    if (!tab?.messages) return { keepFrom: 0, digest: '' };
    const valid = tab.messages.filter((m: any) => m.rawRole);
    if (valid.length <= 10) return { keepFrom: 0, digest: '' };
    const half = Math.floor(valid.length / 2);

    const cached   = tab.workingMemory?.compactedDigest;
    const cachedAt = tab.workingMemory?._lastDigestAt || 0;
    const cacheStillValid = cached && (valid.length - cachedAt) < 6;

    let digest: string;
    if (cacheStillValid) {
        digest = `(Smart digest of first ${cachedAt} turns)\n${cached}`;
    } else {
        const older     = valid.slice(0, half);
        const userTurns = older.filter((m: any) => m.rawRole === userName);
        const lucyExecs = older.filter((m: any) => m.rawRole === 'Lucy' || m.rawRole === 'Sistema');
        const intents   = userTurns.slice(-8).map((m: any) => `· ${String(m.rawContent || '').slice(0, 140).replace(/\s+/g, ' ')}`).join('\n');
        digest = `Se conversaron ${valid.length} turnos (se resumen ${older.length} más antiguos).\nÚltimas intenciones del usuario:\n${intents}\nLucy ejecutó/respondió aprox. ${lucyExecs.length} acciones previas.`;
    }

    const keepMsg = valid[half];
    const keepIdx = tab.messages.indexOf(keepMsg);
    return { keepFrom: keepIdx >= 0 ? keepIdx : 0, digest };
}
