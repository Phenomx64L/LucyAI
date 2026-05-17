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
