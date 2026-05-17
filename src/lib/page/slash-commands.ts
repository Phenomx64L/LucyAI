// ── page/slash-commands.ts ────────────────────────────────────────────────
//
// Slash-command dispatcher extracted from `+page.svelte` (Phase 2c
// refactor, May 2026). Originally a 240-line `switch` inside the page
// component that touched ~12 different state slots. Pulling it into a
// dedicated module:
//
//   • Keeps the page Svelte file focused on UI + reactive plumbing.
//   • Lets every command be tested independently (each `case` is now a
//     pure function over the injected `SlashCtx`).
//   • Makes adding a new slash command a single-file edit (no need to
//     scroll through a 7k-line component).
//
// Pattern
// -------
// The page builds a `SlashCtx` once (the bag of state references the
// commands need: `addMsg`, `getTab`, `invoke`, `$hosts`, etc.) and
// passes it to `dispatchSlashCommand(tabId, raw, ctx)`. Each command
// receives the same ctx; mutations go through the provided callbacks
// so reactivity stays under the page's control.
//
// What this is NOT
// ----------------
// • Not a full DI framework — it's a single-purpose context bag.
// • Not async-first — most commands fire `async () => { ... }` IIFEs
//   internally to keep the dispatcher's signature synchronous. The
//   page doesn't need to await individual commands.

import { invoke } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import { localModels, refreshLocalModels } from '$lib/models.js';

// ── Context interface ────────────────────────────────────────────────────
export interface SlashCtx {
    isEN: boolean;
    currentTheme: string;
    lucyConfig: { name: string };
    /** Reactive accessors — passed in as snapshots so the module never
     *  reaches into Svelte stores directly (those are the page's
     *  responsibility to subscribe to). */
    hosts: Array<{ id: string; name: string; host: string; username: string; type?: 'windows'|'linux'; port?: number; sshKeyPath?: string | null }>;
    tabs: Array<{ id: string; title: string; selectedModel?: string }>;
    LLM_GROUPS: Array<{ label: string; options: Array<{ id: string; icon?: string }> }>;

    // Mutation callbacks — page wires these to its real functions
    getTab: (id: string) => { id: string; messages: any[]; selectedModel?: string } | null | undefined;
    addMsg: (tabId: string, msg: any) => any;
    setActiveTab: (id: string) => void;
    setTheme: (theme: string) => void;
    setTabModel: (tabId: string, modelId: string) => void;
    clearTabMessages: (tabId: string) => void;
    openRemoteDiff: (hostNameOrId: string, filePath: string) => void;
    runMultiCompare: (tabId: string, models: string[], prompt: string) => void;
}

/**
 * Run the slash command parsed out of `raw` (leading `/`). Returns
 * `true` when the command was handled (page should NOT pass it to the
 * LLM), `false` only if `raw` didn't look like a slash command in the
 * first place (defensive — the page already checks `startsWith('/')`).
 */
export function dispatchSlashCommand(tabId: string, raw: string, ctx: SlashCtx): boolean {
    const t = ctx.getTab(tabId);
    if (!t) return false;

    const [cmd, ...rest] = raw.slice(1).trim().split(/\s+/);
    const arg = rest.join(' ').trim();
    const sysMsg = (html: string, color = 'var(--acc)') =>
        ctx.addMsg(tabId, { role: 'system', html: `<div style="color:${color};font-size:11px;font-family:var(--mono);">${html}</div>` });

    switch (cmd.toLowerCase()) {
        case 'help': case '?':
            sysMsg(`<b>Comandos disponibles:</b><br>
                /clear · limpia el chat actual<br>
                /model &lt;nombre&gt; · cambia modelo (parcial: "qwen", "flash", "sonnet")<br>
                /theme &lt;nombre&gt; · default, ocean, hacker, sunset, forest, twilight, mocha, graphite, midnight, amoled, nord<br>
                /editremote &lt;host&gt; &lt;ruta&gt; · edita archivo remoto con diff visual antes de aplicar<br>
                /tab &lt;texto&gt; · saltar a otra pestaña por título<br>
                /models · lista todos los modelos disponibles<br>
                /refresh · re-detecta modelos Ollama<br>
                /compare &lt;m1,m2,...&gt; &lt;prompt&gt; · ejecuta el mismo prompt en N modelos en paralelo<br>
                /recall &lt;query&gt; · busca en el historial de conversaciones pasadas<br>
                /crystallize · destila la sesión actual en un crystal (narrativa + outcomes + lecciones)<br>
                /crystals · lista los crystals más recientes<br>
                /crystal &lt;id&gt; · muestra el detalle de un crystal<br>
                /consolidate · preview (dry-run) de fusión automática de memorias relacionadas<br>
                /consolidate-now · ejecuta la fusión real (LLM por cluster + supersede)<br>
                /reranker · estado del cross-encoder reranker (feature/model/runtime)<br>
                /reranker-install · descarga el modelo ms-marco-MiniLM (22 MB) si la feature está activa<br>
                /reflect · preview de la reflexión (dry-run, sin tocar nada)<br>
                /reflect-now · genera/refuerza insights desde clusters de memorias<br>
                /insights · lista los insights por confidence DESC<br>
                /graph-rebuild · reconstruye el grafo de memoria (concepts/files/sessions)<br>
                /graph &lt;id&gt; [hops] · BFS desde una memoria — descubre lo relacionado vía 1-3 hops<br>
                /help · muestra esta ayuda`);
            return true;

        case 'diagnose-cpu': case 'diagnose-memory': case 'diagnose-disk':
            runQuickDiagnose(tabId, cmd.split('-')[1] as 'cpu'|'memory'|'disk', arg, ctx, sysMsg);
            return true;

        case 'recall':
            if (!arg) { sysMsg('Uso: <code>/recall &lt;consulta&gt;</code> — busca texto en conversaciones pasadas. Ej: <code>/recall iis reset prod</code>'); return true; }
            runRecall(arg, ctx, sysMsg);
            return true;

        case 'clear': case 'cls':
            ctx.clearTabMessages(tabId);
            return true;

        // ── Crystals (Tier 2 #4) ────────────────────────────────────────
        // /crystallize — distill the current tab's transcript into a stored
        //                crystal (narrative + outcomes + files + lessons)
        // /crystals    — list the most recent crystals (newest first)
        // /crystal <id> — print the full body of one crystal
        case 'crystallize':
        case 'crystalize':
            runCrystallize(tabId, ctx, sysMsg);
            return true;

        case 'crystals':
            runCrystalsList(ctx, sysMsg);
            return true;

        case 'crystal':
            if (!arg) { sysMsg('Uso: <code>/crystal &lt;id&gt;</code> — muestra el detalle de un crystal. Lista con <code>/crystals</code>.'); return true; }
            runCrystalGet(arg.trim(), ctx, sysMsg);
            return true;

        // ── Auto-consolidation (Tier 2 #5) ──────────────────────────────
        // /consolidate     — dry-run: muestra clusters propuestos sin tocar nada
        // /consolidate-now — corre la fusión real (LLM por cluster + supersede)
        case 'consolidate':
            runConsolidate(true, sysMsg);
            return true;

        case 'consolidate-now':
        case 'consolidate!':
            runConsolidate(false, sysMsg);
            return true;

        // ── Cross-encoder reranker (Tier 3 #7) ──────────────────────────
        // /reranker          — status check (feature/model/runtime)
        // /reranker-install  — download the 22 MB ONNX model from HF
        case 'reranker':
            runRerankerStatus(sysMsg);
            return true;

        case 'reranker-install':
        case 'install-reranker':
            runRerankerInstall(sysMsg);
            return true;

        // ── Reflection / Insights (Tier 3 #8) ───────────────────────────
        // /reflect     — preview de cuántos clusters se procesarían (dry-run)
        // /reflect-now — corre la reflexión real (genera/refuerza insights)
        // /insights    — lista los insights ordenados por confidence DESC
        case 'reflect':
            runReflect(true, sysMsg);
            return true;
        case 'reflect-now':
        case 'reflect!':
            runReflect(false, sysMsg);
            return true;
        case 'insights':
            runInsightsList(sysMsg);
            return true;

        // ── Memory graph (Tier 3 #9) ────────────────────────────────────
        // /graph-rebuild         — reconstruye los edges del grafo (corre auto cada 24 h)
        // /graph <id> [hops]     — BFS desde la memoria id, hasta hops levels
        case 'graph-rebuild':
        case 'rebuild-graph':
            runGraphRebuild(sysMsg);
            return true;
        case 'graph':
            if (!arg) { sysMsg('Uso: <code>/graph &lt;memory-id&gt; [hops=2]</code> — explora memorias relacionadas vía BFS.'); return true; }
            runGraphNeighbors(arg.trim(), ctx, sysMsg);
            return true;

        case 'editremote':
        case 'edit-remote':
        case 'edr': {
            if (!arg) { sysMsg(`Uso: /editremote &lt;host&gt; &lt;ruta&gt;<br>Ej: /editremote PARROT /etc/nginx/nginx.conf`); return true; }
            const sp = arg.indexOf(' ');
            if (sp < 0) { sysMsg(`Falta la ruta. Uso: /editremote &lt;host&gt; &lt;ruta&gt;`, 'var(--red)'); return true; }
            const hostName = arg.slice(0, sp).trim();
            const filePath = arg.slice(sp + 1).trim();
            ctx.openRemoteDiff(hostName, filePath);
            return true;
        }

        case 'theme': {
            const valid = ['default','ocean','hacker','sunset','forest','twilight','mocha','graphite','midnight','amoled','nord'];
            if (!arg) { sysMsg(`Tema actual: <b>${ctx.currentTheme}</b>. Disponibles: ${valid.join(', ')}`); return true; }
            if (!valid.includes(arg)) { sysMsg(`Tema "${arg}" no existe. Usa: ${valid.join(', ')}`, 'var(--red)'); return true; }
            ctx.setTheme(arg);
            sysMsg(`Tema cambiado a <b>${arg}</b>`);
            return true;
        }

        case 'model': {
            if (!arg) { sysMsg(`Modelo actual: <b>${t.selectedModel}</b>. Usa /models para ver todos.`); return true; }
            // Buscar match parcial entre todos los modelos (cloud + locales)
            const all = collectModelIds(ctx);
            const match = all.find((id) => id.toLowerCase().includes(arg.toLowerCase()));
            if (!match) { sysMsg(`Modelo "${arg}" no encontrado. Usa /models para ver disponibles.`, 'var(--red)'); return true; }
            ctx.setTabModel(tabId, match);
            sysMsg(`Modelo cambiado a <b>${match}</b>`);
            return true;
        }

        case 'models': {
            const all: string[] = [];
            for (const g of ctx.LLM_GROUPS) {
                if (g.label.includes('Locales')) {
                    for (const o of get(localModels)) all.push(`${(o as any).icon ?? ''} ${(o as any).id}`);
                } else for (const o of g.options) all.push(`${o.icon ?? ''} ${o.id}`);
            }
            sysMsg(`<b>Modelos disponibles:</b><br>${all.join('<br>')}`);
            return true;
        }

        case 'tab': {
            if (!arg) { sysMsg(`Pestañas: ${ctx.tabs.map((x) => x.title).join(', ')}`); return true; }
            const target = ctx.tabs.find((x) => x.title.toLowerCase().includes(arg.toLowerCase()));
            if (target) { ctx.setActiveTab(target.id); sysMsg(`→ ${target.title}`); }
            else sysMsg(`No se encontró pestaña "${arg}"`, 'var(--red)');
            return true;
        }

        case 'refresh':
            refreshLocalModels()
                .then((r) => sysMsg(`✓ ${r.length} modelos locales detectados`))
                .catch((e) => sysMsg(`Error: ${e}`, 'var(--red)'));
            return true;

        case 'compare': {
            // /compare gemini-3.1-flash-lite,local-qwen2.5 ¿qué es un firewall?
            const m = arg.match(/^([^\s]+)\s+([\s\S]+)$/);
            if (!m) { sysMsg(`Uso: /compare modelo1,modelo2 &lt;prompt&gt;`, 'var(--amber)'); return true; }
            const models = m[1].split(',').map((s) => s.trim()).filter(Boolean);
            const prompt = m[2].trim();
            if (models.length < 2) { sysMsg(`Necesitas al menos 2 modelos`, 'var(--amber)'); return true; }
            ctx.runMultiCompare(tabId, models, prompt);
            return true;
        }

        default:
            sysMsg(`Comando desconocido: /${cmd}. Usa /help para ver disponibles.`, 'var(--amber)');
            return true;
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/** Flatten all model ids across cloud + local groups for `/model` fuzzy match. */
function collectModelIds(ctx: SlashCtx): string[] {
    const all: string[] = [];
    for (const g of ctx.LLM_GROUPS) {
        if (g.label.includes('Locales')) {
            for (const o of get(localModels)) all.push((o as any).id);
        } else for (const o of g.options) all.push(o.id);
    }
    return all;
}

/** `/recall` — search past conversations indexed by FTS5 in the Rust DB. */
async function runRecall(query: string, ctx: SlashCtx, sysMsg: (html: string, color?: string) => any): Promise<void> {
    try {
        const results = await invoke<Array<{ role: string; content: string; tab_title?: string; created_at: number }>>(
            'recall_conversations',
            { query, limit: 12 },
        );
        if (!results || !results.length) {
            sysMsg(`Sin coincidencias para <b>"${query}"</b>.`, 'var(--yellow,#f59e0b)');
            return;
        }
        const fmt = (t: number) => new Date(t * 1000).toLocaleString();
        const rows = results.map((r) => {
            const icon = r.role === 'user' ? '👤' : r.role === 'lucy' ? '✦' : 'ℹ';
            const snippet = r.content.length > 240 ? r.content.slice(0, 240) + '…' : r.content;
            const safe = snippet.replace(/</g, '&lt;').replace(/>/g, '&gt;');
            const qRe = new RegExp(`(${query.split(/\s+/).filter(Boolean).map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|')})`, 'ig');
            const hl = safe.replace(qRe, '<mark style="background:rgba(250,204,21,.35);color:inherit;">$1</mark>');
            const tabLbl = r.tab_title ? ` · <em>${r.tab_title.replace(/</g, '&lt;')}</em>` : '';
            return `<div style="margin:6px 0;padding:6px 8px;border-left:2px solid var(--acc);background:rgba(99,102,241,.04);">
                <div style="font-size:10px;color:var(--txt2);margin-bottom:3px;">${icon} ${r.role}${tabLbl} · ${fmt(r.created_at)}</div>
                <div style="font-size:12px;line-height:1.4;white-space:pre-wrap;">${hl}</div>
            </div>`;
        }).join('');
        sysMsg(`<b>🔍 Recall — ${results.length} coincidencia${results.length > 1 ? 's' : ''} para "${query}":</b>${rows}`);
    } catch (e) {
        sysMsg(`Error en /recall: ${String(e).slice(0, 200)}`, 'var(--red)');
    }
}

/** `/diagnose-{cpu,memory,disk}` — bundle of parallel PowerShell probes for a
 *  fast triage report. Targets `local` by default, accepts a host name as arg. */
function runQuickDiagnose(
    tabId: string,
    type: 'cpu' | 'memory' | 'disk',
    arg: string,
    ctx: SlashCtx,
    sysMsg: (html: string, color?: string) => any,
): void {
    const hostTarget = arg && arg.trim() ? arg.trim() : 'local';

    const suites: Record<typeof type, string[]> = {
        cpu: [
            'Get-Process | Sort-Object CPU -Descending | Select-Object -First 15 Name, CPU, Id, WorkingSet | Format-Table -AutoSize',
            'Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess > 0" | Sort-Object PercentProcessorTime -Descending | Select-Object -First 10 Name, PercentProcessorTime, IDProcess | Format-Table -AutoSize',
            'Get-Counter "\\Processor(_Total)\\% Processor Time" | Select-Object -ExpandProperty CounterSamples | Select-Object CookedValue',
            'wmic process get name,processid,workingsetsize /format:csv | ConvertFrom-Csv | Sort-Object WorkingSetSize -Descending | Select-Object -First 10',
        ],
        memory: [
            'Get-CimInstance Win32_LogicalMemoryConfiguration | Format-List InstallDate, TotalPhysicalMemory',
            'Get-Process | Sort-Object WorkingSet -Descending | Select-Object -First 15 Name, @{n="Memory(MB)";e={[Math]::Round($_.WorkingSet/1MB,2)}}, Id | Format-Table -AutoSize',
            '[Math]::Round((Get-CimInstance Win32_LogicalMemoryConfiguration).TotalPhysicalMemory / 1GB, 2)',
            'Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory | Select-Object AvailableMBytes, CommittedBytes',
        ],
        disk: [
            'Get-PSDrive -PSProvider FileSystem | Where-Object { $_.Used -ne $null } | Select-Object Name, @{n="Used(GB)";e={[Math]::Round($_.Used/1GB,2)}}, @{n="Free(GB)";e={[Math]::Round($_.Free/1GB,2)}}, @{n="Total(GB)";e={[Math]::Round(($_.Used+$_.Free)/1GB,2)}}, @{n="%Used";e={if(($_.Used+$_.Free) -gt 0){[Math]::Round($_.Used/($_.Used+$_.Free)*100,1)}else{0}}} | Format-Table -AutoSize',
            'Get-Volume | Where-Object DriveLetter -ne $null | Select-Object DriveLetter, FileSystem, HealthStatus, SizeRemaining, Size | Format-Table -AutoSize',
            '$d = Get-CimInstance Win32_LogicalDisk -Filter "DriveType=3" -ErrorAction SilentlyContinue; if ($d) { $d | Select-Object DeviceID, @{n="Size(GB)";e={[Math]::Round($_.Size/1GB,2)}}, @{n="Free(GB)";e={[Math]::Round($_.FreeSpace/1GB,2)}}, @{n="Used%";e={if($_.Size -gt 0){[Math]::Round((1-($_.FreeSpace/$_.Size))*100,1)}else{0}}} | Format-Table -AutoSize } else { "Win32_LogicalDisk returned no fixed drives (DriveType=3)." }',
            'Get-Counter "\\PhysicalDisk(_Total)\\% Disk Time" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty CounterSamples | Select-Object CookedValue',
        ],
    };

    const commands = suites[type] || suites.cpu;
    const hostDisplay = hostTarget === 'local' ? 'this machine' : hostTarget;
    sysMsg(`<b>🔍 Quick ${type.toUpperCase()} Diagnosis — ${hostDisplay}</b><br>Executing ${commands.length} commands in parallel…`);

    (async () => {
        try {
            const t0 = Date.now();
            if (hostTarget === 'local') {
                const promises = commands.map((cmd, i) =>
                    invoke<string>('execute_powershell', { script: cmd, forceExecute: false })
                        .then((out) => ({ idx: i, out, error: null as string | null }))
                        .catch((e) => ({ idx: i, out: null as string | null, error: String(e) })),
                );
                const results = await Promise.all(promises);
                const elapsed = Date.now() - t0;

                const html = results.map((r, i) => {
                    const ok = !r.error;
                    const content = ok ? (r.out ?? '') : `ERROR: ${r.error}`;
                    const safe = content.replace(/</g, '&lt;').replace(/>/g, '&gt;');
                    const snippet = commands[i].substring(0, 80) + (commands[i].length > 80 ? '…' : '');
                    return `<div style="margin:12px 0;border-left:3px ${ok ? '#34d399' : '#f87171'};padding:10px;background:${ok ? 'rgba(52,211,153,.04)' : 'rgba(248,113,113,.04)'}">
                        <div style="font-size:10px;color:var(--txt2);margin-bottom:6px;"><strong>[${i + 1}]</strong> ${snippet}</div>
                        <pre style="margin:0;font-size:10px;max-height:150px;overflow:auto;color:#999;">${safe.substring(0, 500)}</pre>
                    </div>`;
                }).join('');

                ctx.addMsg(tabId, {
                    role: 'lucy',
                    html: `<div class="mn">Lucy</div><div style="font-size:11px;color:var(--txt2);margin:8px 0;">⚡ ${commands.length} commands, ${elapsed}ms</div>${html}`,
                    rawContent: results.map((r) => r.out || r.error).join('\n---\n'),
                });
            } else {
                const hostIdClean = hostTarget.replace(/^LucyHost_/, '');
                const h = ctx.hosts.find((x) => x.id === hostIdClean || x.name === hostTarget);
                if (!h) throw new Error(`Host '${hostTarget}' not found`);

                const pwd = await invoke<string>('get_host_credential', { hostId: h.id }).catch(() => null);
                const promises = commands.map((cmd, i) =>
                    invoke<string>('execute_shell_cmd', {
                        host: h.host, username: h.username, command: cmd,
                        hostType: h.type, port: h.port || (h.type === 'linux' ? 22 : 5985),
                        password: pwd, keyPath: h.sshKeyPath || null,
                    })
                        .then((out) => ({ idx: i, out, error: null as string | null }))
                        .catch((e) => ({ idx: i, out: null as string | null, error: String(e) })),
                );
                const results = await Promise.all(promises);
                const elapsed = Date.now() - t0;

                const html = results.map((r) => {
                    const ok = !r.error;
                    const content = ok ? (r.out ?? '') : `ERROR: ${r.error}`;
                    const safe = content.replace(/</g, '&lt;').replace(/>/g, '&gt;');
                    return `<div style="margin:8px 0;border-left:3px ${ok ? '#34d399' : '#f87171'};padding:8px;background:${ok ? 'rgba(52,211,153,.04)' : 'rgba(248,113,113,.04)'}">
                        <pre style="margin:0;font-size:10px;max-height:100px;overflow:auto;">${safe.substring(0, 300)}</pre>
                    </div>`;
                }).join('');

                ctx.addMsg(tabId, {
                    role: 'lucy',
                    html: `<div class="mn">Lucy</div><div style="font-size:11px;color:var(--txt2);">⚡ ${h.name} (${commands.length} commands, ${elapsed}ms)</div>${html}`,
                    rawContent: results.map((r) => r.out || r.error).join('\n'),
                });
            }
        } catch (e) {
            sysMsg(`Error: ${String(e).substring(0, 150)}`, 'var(--red)');
        }
    })();
}

// ── Crystals (Tier 2 #4) ────────────────────────────────────────────────
// Wire the backend agent_crystals commands as slash actions. Keeps the
// surface tiny — no new view, no new modal — while giving the user full
// access to create/list/inspect crystals. Once the workflow proves
// itself a richer Crystals panel can replace this UX.

interface CrystalRow {
    id: number;
    session_id: string;
    project: string;
    narrative: string;
    key_outcomes: string;     // JSON-encoded string[]
    files_affected: string;   // JSON-encoded string[]
    lessons: string;          // JSON-encoded string[]
    source_chars: number;
    created_at: number;
}

function escapeHtml(s: string): string {
    return s.replace(/[&<>"']/g, c => ({
        '&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;',
    }[c] as string));
}

/**
 * Build a transcript from the current tab's messages, then call
 * crystallize_session. The Rust side calls Ollama with a strict XML
 * prompt — depending on model size, expect ~5-20 s. We show a
 * "working…" message immediately so the user knows it's running.
 */
function runCrystallize(
    tabId: string,
    ctx: SlashCtx,
    sysMsg: (html: string, color?: string) => void,
) {
    const t = ctx.getTab(tabId);
    if (!t || !t.messages || t.messages.length === 0) {
        sysMsg('No hay mensajes en esta sesión para cristalizar.', 'var(--yellow)');
        return;
    }

    // Build transcript: role + rawContent of every meaningful message.
    // Skip system markers (toasts, security blocks, etc.) — they're UI
    // noise, not session content the LLM should summarise.
    const lines: string[] = [];
    for (const m of t.messages) {
        const role: string = (m && m.role) || '';
        if (role === 'system' || role === 'toast') continue;
        const raw = (m && (m.rawContent || m.text || m.html || '')) as string;
        const trimmed = String(raw).replace(/<[^>]+>/g, '').trim();
        if (!trimmed) continue;
        lines.push(`[${role}] ${trimmed}`);
    }
    const transcript = lines.join('\n\n');
    if (transcript.length < 80) {
        sysMsg('Sesión muy corta para destilar en un crystal (mínimo ~80 caracteres de contenido real).', 'var(--yellow)');
        return;
    }

    sysMsg(`◆ Cristalizando sesión <code>${tabId.slice(0, 8)}</code>… (${transcript.length.toLocaleString()} chars → Ollama, esto puede tardar 10-30 s)`);

    (async () => {
        try {
            const newId = await invoke<number>('crystallize_session', {
                sessionId: tabId,
                project:   '',
                transcript,
            });
            // Fetch the row back so we can render its body
            const c = await invoke<CrystalRow | null>('get_crystal', { id: newId });
            if (!c) {
                sysMsg(`Crystal creado (id=${newId}) pero no pudo releerse.`, 'var(--yellow)');
                return;
            }
            const outcomes: string[] = JSON.parse(c.key_outcomes || '[]');
            const files:    string[] = JSON.parse(c.files_affected || '[]');
            const lessons:  string[] = JSON.parse(c.lessons || '[]');
            const body = `
                <div class="mn">◆ Crystal #${c.id}</div>
                <div style="margin-top:6px;font-size:13px;line-height:1.5;"><b>Narrativa:</b> ${escapeHtml(c.narrative)}</div>
                ${outcomes.length ? `<div style="margin-top:8px;"><b>Outcomes</b><ul style="margin:4px 0 0 18px;padding:0;font-size:12px;">${outcomes.map(o => `<li>${escapeHtml(o)}</li>`).join('')}</ul></div>` : ''}
                ${files.length ?    `<div style="margin-top:8px;"><b>Archivos</b><ul style="margin:4px 0 0 18px;padding:0;font-size:12px;font-family:var(--mono);">${files.map(f => `<li>${escapeHtml(f)}</li>`).join('')}</ul></div>` : ''}
                ${lessons.length ?  `<div style="margin-top:8px;"><b>Lecciones</b> <span style="color:var(--txt2);font-size:11px;">— también guardadas como memorias</span><ul style="margin:4px 0 0 18px;padding:0;font-size:12px;">${lessons.map(l => `<li>${escapeHtml(l)}</li>`).join('')}</ul></div>` : ''}
                <div style="margin-top:6px;font-size:10px;color:var(--txt2);">Fuente: ${c.source_chars.toLocaleString()} chars · ID: ${c.id}</div>
            `;
            ctx.addMsg(tabId, { role: 'lucy', html: body, rawContent: c.narrative });
        } catch (e) {
            sysMsg(`Crystallize falló: ${String(e).substring(0, 200)}`, 'var(--red)');
        }
    })();
}

/** Print the 10 most recent crystals as a compact list. */
function runCrystalsList(
    ctx: SlashCtx,
    sysMsg: (html: string, color?: string) => void,
) {
    (async () => {
        try {
            const list = await invoke<CrystalRow[]>('list_crystals', {
                sessionId: null,
                project:   null,
                limit:     10,
            });
            if (!list || list.length === 0) {
                sysMsg('No hay crystals todavía. Crea uno con <code>/crystallize</code> al final de una sesión.');
                return;
            }
            const rows = list.map(c => {
                const date = new Date(c.created_at * 1000).toLocaleString();
                const session = (c.session_id || '').slice(0, 8) || '—';
                return `<div style="padding:6px 8px;border-left:2px solid var(--accent);margin-bottom:6px;">
                    <div style="display:flex;justify-content:space-between;font-size:11px;color:var(--txt2);">
                        <span>#${c.id} · sess <code>${session}</code></span><span>${date}</span>
                    </div>
                    <div style="margin-top:2px;font-size:12px;">${escapeHtml(c.narrative)}</div>
                    <div style="margin-top:2px;font-size:10px;color:var(--txt2);">Detalle: <code>/crystal ${c.id}</code></div>
                </div>`;
            }).join('');
            sysMsg(`<div class="mn">◆ Crystals (${list.length} recientes)</div>${rows}`);
        } catch (e) {
            sysMsg(`list_crystals falló: ${String(e).substring(0, 200)}`, 'var(--red)');
        }
    })();
}

// ── Consolidation (Tier 2 #5) ──────────────────────────────────────────
// Wire the auto_consolidate_run backend command. Two modes — dry-run
// shows what WOULD be fused; the explicit /consolidate-now actually
// runs the LLM calls and supersedes the originals.

interface ConsolidationCluster {
    source_ids: number[];
    shared_tags: string[];
    new_memory_id: number | null;
    new_title: string | null;
}
interface AutoConsolidateReport {
    dry_run: boolean;
    eligible_memories: number;
    clusters_found: number;
    clusters_processed: number;
    memories_superseded: number;
    new_memories: number;
    clusters: ConsolidationCluster[];
}

function runConsolidate(
    dryRun: boolean,
    sysMsg: (html: string, color?: string) => void,
) {
    const label = dryRun ? 'preview' : 'ejecutando';
    sysMsg(`◯ Consolidación de memorias — ${label}… (puede tardar ${dryRun ? 'pocos segundos' : '15-90 s por cluster'})`);

    (async () => {
        try {
            const r = await invoke<AutoConsolidateReport>('auto_consolidate_run', { dryRun });

            if (r.clusters.length === 0) {
                sysMsg(`<div class="mn">◯ Sin clusters viables</div>
                    <div style="font-size:11px;color:var(--txt2);">
                        ${r.eligible_memories} memorias elegibles, ${r.clusters_found} clusters encontrados.
                        Necesitas ≥ 3 memorias con ≥ 2 tags compartidos (y ≥ 7 días de antigüedad) para fusionar.
                    </div>`);
                return;
            }

            const rows = r.clusters.map(c => {
                const status = c.new_memory_id
                    ? `<span style="color:#34d399;">✓ fusionado → #${c.new_memory_id}</span>`
                    : c.new_title
                        ? `<span style="color:#f87171;">${escapeHtml(c.new_title)}</span>`
                        : `<span style="color:var(--txt2);">(propuesta)</span>`;
                const tags = c.shared_tags.length
                    ? c.shared_tags.map(t => `<code style="font-size:10px;">${escapeHtml(t)}</code>`).join(' ')
                    : '<span style="color:var(--txt2);">(sin tags compartidos)</span>';
                const newTitle = c.new_title && c.new_memory_id
                    ? `<div style="margin-top:2px;font-size:11px;"><b>Nuevo:</b> ${escapeHtml(c.new_title)}</div>`
                    : '';
                return `<div style="padding:6px 8px;border-left:2px solid var(--accent);margin-bottom:6px;">
                    <div style="display:flex;justify-content:space-between;font-size:11px;">
                        <span>${c.source_ids.length} memorias → 1</span>${status}
                    </div>
                    <div style="margin-top:2px;font-size:10px;color:var(--txt2);">
                        IDs: ${c.source_ids.join(', ')}
                    </div>
                    <div style="margin-top:2px;">tags: ${tags}</div>
                    ${newTitle}
                </div>`;
            }).join('');

            const summary = r.dry_run
                ? `${r.eligible_memories} elegibles · ${r.clusters_found} clusters · ${r.clusters_processed} se procesarían. Ejecuta <code>/consolidate-now</code> para fusionar de verdad.`
                : `${r.new_memories} nuevas memorias · ${r.memories_superseded} originales marcadas superseded (preservadas).`;

            sysMsg(`<div class="mn">◯ Consolidación — ${r.dry_run ? 'DRY-RUN' : 'COMPLETADA'}</div>
                <div style="font-size:11px;color:var(--txt2);margin-bottom:6px;">${summary}</div>
                ${rows}`);
        } catch (e) {
            sysMsg(`auto_consolidate_run falló: ${String(e).substring(0, 200)}`, 'var(--red)');
        }
    })();
}

// ── Memory graph (Tier 3 #9) ────────────────────────────────────────────

interface GraphRebuildReport {
    eligible_memories: number;
    concept_edges: number;
    file_edges: number;
    session_edges: number;
    total_directed_edges: number;
}

interface GraphNeighbor {
    memory_id: number;
    hops: number;
    score: number;
    edge_types: string;  // pipe-joined
    memory: {
        id: number;
        title: string;
        content: string;
        tags: string;
        created_at: number;
    };
}

function runGraphRebuild(sysMsg: (html: string, color?: string) => void) {
    sysMsg('◈ Reconstruyendo grafo de memoria…');
    (async () => {
        try {
            const r = await invoke<GraphRebuildReport>('graph_rebuild_edges_run');
            sysMsg(`<div class="mn">◈ Grafo reconstruido</div>
                <div style="font-size:11px;">
                    <b>${r.eligible_memories}</b> nodos ·
                    <b>${r.total_directed_edges.toLocaleString()}</b> aristas (kept tras cap)
                </div>
                <div style="font-size:11px;color:var(--txt2);margin-top:2px;">
                    pre-cap: ${r.concept_edges.toLocaleString()} concept ·
                    ${r.file_edges.toLocaleString()} file ·
                    ${r.session_edges.toLocaleString()} session
                </div>`);
        } catch (e) {
            sysMsg(`graph_rebuild falló: ${String(e).substring(0, 200)}`, 'var(--red)');
        }
    })();
}

function runGraphNeighbors(
    argRaw: string,
    _ctx: SlashCtx,
    sysMsg: (html: string, color?: string) => void,
) {
    // Parse "<id> [hops]" — second token optional
    const parts = argRaw.split(/\s+/).filter(p => p.length);
    const seedId = parseInt(parts[0] || '', 10);
    const hops = parts[1] ? Math.max(1, Math.min(4, parseInt(parts[1], 10))) : 2;
    if (!Number.isFinite(seedId) || seedId <= 0) {
        sysMsg(`memory-id inválido: <code>${escapeHtml(argRaw)}</code>`, 'var(--red)');
        return;
    }
    sysMsg(`◈ BFS desde memoria #${seedId} hasta ${hops} hops…`);
    (async () => {
        try {
            const list = await invoke<GraphNeighbor[]>('graph_neighbors', {
                seedId, maxHops: hops, limit: 15,
            });
            if (!list || list.length === 0) {
                sysMsg(`No hay memorias relacionadas a #${seedId} dentro de ${hops} hops. Quizás el grafo no está construido — prueba <code>/graph-rebuild</code>.`);
                return;
            }
            // Group by hop for cleaner output
            const byHop: Record<number, GraphNeighbor[]> = {};
            for (const n of list) {
                (byHop[n.hops] ||= []).push(n);
            }
            const sections = Object.keys(byHop)
                .map(h => parseInt(h, 10))
                .sort((a, b) => a - b)
                .map(h => {
                    const rows = byHop[h].map(n => {
                        const tags = (() => {
                            try { return JSON.parse(n.memory.tags || '[]'); } catch { return []; }
                        })();
                        const tagStr = tags.length
                            ? `<span style="color:var(--txt2);font-size:10px;">${tags.slice(0, 4).map((t: string) => escapeHtml(t)).join(', ')}</span>`
                            : '';
                        const edgeTypes = n.edge_types.split('|').map(et => {
                            const sym = et === 'shares_concept' ? '◇' : et === 'shares_file' ? '⊟' : '⌖';
                            return `<span title="${escapeHtml(et)}" style="color:var(--accent);font-size:10px;">${sym}</span>`;
                        }).join(' ');
                        return `<div style="padding:4px 6px;border-left:2px solid var(--accent);margin-bottom:4px;">
                            <div style="display:flex;justify-content:space-between;font-size:10px;color:var(--txt2);">
                                <span>#${n.memory.id} · score=${n.score.toFixed(3)}</span>
                                <span>${edgeTypes}</span>
                            </div>
                            <div style="font-size:12px;line-height:1.3;">${escapeHtml(n.memory.title)}</div>
                            ${tagStr}
                        </div>`;
                    }).join('');
                    return `<div style="margin-top:6px;font-size:11px;color:var(--txt2);">— hop ${h} —</div>${rows}`;
                }).join('');
            sysMsg(`<div class="mn">◈ Vecindario de #${seedId} (${list.length} memorias en ${hops} hops)</div>${sections}`);
        } catch (e) {
            sysMsg(`graph_neighbors falló: ${String(e).substring(0, 200)}`, 'var(--red)');
        }
    })();
}

// ── Reflection / Insights (Tier 3 #8) ───────────────────────────────────

interface ReflectReport {
    dry_run: boolean;
    eligible_memories: number;
    clusters_processed: number;
    insights_created: number;
    insights_reinforced: number;
}

interface AgentInsight {
    id: number;
    content: string;
    fingerprint: string;
    confidence: number;
    reinforcements: number;
    concepts: string;  // JSON-encoded string[]
    source_count: number;
    last_reinforced_at: number;
    created_at: number;
    updated_at: number;
}

function runReflect(dryRun: boolean, sysMsg: (html: string, color?: string) => void) {
    sysMsg(`◇ Reflexión — ${dryRun ? 'preview' : 'ejecutando'}… (${dryRun ? 'segundos' : '20-90 s por cluster'})`);
    (async () => {
        try {
            const r = await invoke<ReflectReport>('reflect_run', { dryRun });
            if (r.dry_run) {
                sysMsg(`<div class="mn">◇ Reflexión DRY-RUN</div>
                    <div style="font-size:11px;">${r.eligible_memories} memorias elegibles · ${r.clusters_processed} clusters se procesarían.</div>
                    <div style="font-size:11px;color:var(--txt2);">Ejecuta <code>/reflect-now</code> para generar insights.</div>`);
            } else {
                const color = (r.insights_created + r.insights_reinforced) > 0 ? '#34d399' : 'var(--txt2)';
                sysMsg(`<div class="mn" style="color:${color};">◇ Reflexión completada</div>
                    <div style="font-size:11px;">
                        ${r.clusters_processed} clusters procesados ·
                        <b>${r.insights_created}</b> nuevos insights ·
                        <b>${r.insights_reinforced}</b> insights reforzados
                    </div>
                    <div style="font-size:11px;color:var(--txt2);">Lista con <code>/insights</code>.</div>`);
            }
        } catch (e) {
            sysMsg(`reflect_run falló: ${String(e).substring(0, 200)}`, 'var(--red)');
        }
    })();
}

function runInsightsList(sysMsg: (html: string, color?: string) => void) {
    (async () => {
        try {
            const list = await invoke<AgentInsight[]>('list_insights', { limit: 20 });
            if (!list || list.length === 0) {
                sysMsg('No hay insights aún. La reflexión nocturna corre cada 48 h, o lánzala manualmente con <code>/reflect-now</code>.');
                return;
            }
            const rows = list.map(i => {
                const concepts: string[] = JSON.parse(i.concepts || '[]');
                const conf = (i.confidence * 100).toFixed(0);
                const date = new Date(i.last_reinforced_at * 1000).toLocaleDateString();
                const bar = '█'.repeat(Math.round(i.confidence * 10)) + '░'.repeat(10 - Math.round(i.confidence * 10));
                return `<div style="padding:6px 8px;border-left:2px solid var(--accent);margin-bottom:6px;">
                    <div style="display:flex;justify-content:space-between;font-size:10px;color:var(--txt2);">
                        <span>#${i.id} · ${conf}% <code>${bar}</code></span>
                        <span>×${i.reinforcements} · ${date}</span>
                    </div>
                    <div style="margin-top:3px;font-size:12px;line-height:1.4;">${escapeHtml(i.content)}</div>
                    ${concepts.length ? `<div style="margin-top:3px;font-size:10px;color:var(--txt2);">${concepts.map(c => `<code style="font-size:10px;">${escapeHtml(c)}</code>`).join(' ')}</div>` : ''}
                </div>`;
            }).join('');
            sysMsg(`<div class="mn">◇ Insights (${list.length} top por confidence)</div>${rows}`);
        } catch (e) {
            sysMsg(`list_insights falló: ${String(e).substring(0, 200)}`, 'var(--red)');
        }
    })();
}

// ── Reranker (Tier 3 #7) ────────────────────────────────────────────────

interface RerankerStatus {
    status: 'feature_disabled' | 'model_missing' | 'runtime_missing' | 'active' | 'failed';
    model_path: string;
    note?: string | null;
}

function runRerankerStatus(sysMsg: (html: string, color?: string) => void) {
    (async () => {
        try {
            const s = await invoke<RerankerStatus>('reranker_status');
            const colorMap: Record<string, string> = {
                active:           '#34d399',
                feature_disabled: 'var(--txt2)',
                model_missing:    '#f59e0b',
                runtime_missing:  '#f59e0b',
                failed:           '#f87171',
            };
            const labelMap: Record<string, string> = {
                active:           '✓ Activo',
                feature_disabled: '○ Feature deshabilitada (rebuild con --features ml-reranker)',
                model_missing:    '⚠ Modelo no descargado',
                runtime_missing:  '⚠ ONNX Runtime no instalado',
                failed:           '✗ Falló la carga',
            };
            sysMsg(`<div class="mn">⚖ Cross-encoder reranker</div>
                <div style="margin-top:4px;color:${colorMap[s.status]};">${labelMap[s.status]}</div>
                <div style="margin-top:4px;font-size:11px;color:var(--txt2);">Path: <code>${escapeHtml(s.model_path)}</code></div>
                ${s.note ? `<div style="margin-top:4px;font-size:11px;color:var(--txt2);">${escapeHtml(s.note)}</div>` : ''}
                ${s.status === 'model_missing' ? `<div style="margin-top:6px;font-size:11px;">Ejecuta <code>/reranker-install</code> para descargar.</div>` : ''}`);
        } catch (e) {
            sysMsg(`reranker_status falló: ${String(e).substring(0, 200)}`, 'var(--red)');
        }
    })();
}

function runRerankerInstall(sysMsg: (html: string, color?: string) => void) {
    sysMsg(`⬇ Descargando ms-marco-MiniLM-L-6-v2 desde HuggingFace (~22 MB, sin auth)…`);
    (async () => {
        try {
            const msg = await invoke<string>('download_reranker_model');
            sysMsg(`<div class="mn" style="color:#34d399;">✓ Reranker instalado</div>
                <div style="margin-top:4px;font-size:11px;">${escapeHtml(msg)}</div>
                <div style="margin-top:4px;font-size:11px;color:var(--txt2);">Verifica con <code>/reranker</code>. Si la feature ml-reranker está activa, las búsquedas expandidas ya usan el cross-encoder.</div>`);
        } catch (e) {
            sysMsg(`download_reranker_model falló: ${String(e).substring(0, 250)}`, 'var(--red)');
        }
    })();
}

/** Show the full body of one crystal by id. */
function runCrystalGet(
    idRaw: string,
    ctx: SlashCtx,
    sysMsg: (html: string, color?: string) => void,
) {
    const id = parseInt(idRaw, 10);
    if (!Number.isFinite(id) || id <= 0) {
        sysMsg(`ID inválido: <code>${escapeHtml(idRaw)}</code>`, 'var(--red)');
        return;
    }
    (async () => {
        try {
            const c = await invoke<CrystalRow | null>('get_crystal', { id });
            if (!c) {
                sysMsg(`Crystal #${id} no existe.`, 'var(--yellow)');
                return;
            }
            const outcomes: string[] = JSON.parse(c.key_outcomes || '[]');
            const files:    string[] = JSON.parse(c.files_affected || '[]');
            const lessons:  string[] = JSON.parse(c.lessons || '[]');
            const date = new Date(c.created_at * 1000).toLocaleString();
            const body = `
                <div class="mn">◆ Crystal #${c.id}</div>
                <div style="font-size:10px;color:var(--txt2);">sess <code>${(c.session_id || '—').slice(0, 12)}</code> · ${date}</div>
                <div style="margin-top:8px;font-size:13px;line-height:1.5;"><b>Narrativa:</b> ${escapeHtml(c.narrative)}</div>
                ${outcomes.length ? `<div style="margin-top:8px;"><b>Outcomes</b><ul style="margin:4px 0 0 18px;padding:0;font-size:12px;">${outcomes.map(o => `<li>${escapeHtml(o)}</li>`).join('')}</ul></div>` : ''}
                ${files.length ?    `<div style="margin-top:8px;"><b>Archivos</b><ul style="margin:4px 0 0 18px;padding:0;font-size:12px;font-family:var(--mono);">${files.map(f => `<li>${escapeHtml(f)}</li>`).join('')}</ul></div>` : ''}
                ${lessons.length ?  `<div style="margin-top:8px;"><b>Lecciones</b><ul style="margin:4px 0 0 18px;padding:0;font-size:12px;">${lessons.map(l => `<li>${escapeHtml(l)}</li>`).join('')}</ul></div>` : ''}
            `;
            sysMsg(body);
        } catch (e) {
            sysMsg(`get_crystal falló: ${String(e).substring(0, 200)}`, 'var(--red)');
        }
    })();
}
