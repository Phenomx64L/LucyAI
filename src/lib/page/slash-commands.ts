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
import { safeJsonArray } from '$lib/safe-json';
import { localModels, refreshLocalModels } from '$lib/models.js';

// ── Context interface ────────────────────────────────────────────────────
export interface SlashCtx {
    isEN: boolean;
    currentTheme: string;
    lucyConfig: { name: string };
    /** Sprint 8 — open the floating skill picker. Wired by the page. */
    openSkillPicker?: () => void;
    /** v1.6.1 — open the ECC-adapted system-prompt skill preset picker.
     *  Distinct from openSkillPicker (which lists executable scripts);
     *  this one selects a behavioural framing prepended to the prompt. */
    openSkillPresetPicker?: () => void;
    /** Sprint 8 — open the KG mini-viewer for a path. Wired by the page. */
    openKgViewer?: (path: string) => void;
    /** Reactive accessors — passed in as snapshots so the module never
     *  reaches into Svelte stores directly (those are the page's
     *  responsibility to subscribe to). */
    hosts: Array<{ id: string; name: string; host: string; username: string; type?: 'windows'|'linux'; port?: number; sshKeyPath?: string | null }>;
    tabs: Array<{ id: string; title: string; selectedModel?: string }>;
    LLM_GROUPS: Array<{ label: string; options: Array<{ id: string; icon?: string }> }>;

    /** Config flags the user can toggle via slash commands (restored after
     *  Sprint D regression). Updated in place via setSmartRouting / setPrivacy. */
    lucyFlags: { smartRouting: boolean; privacyMode: boolean };
    /** Last routing decision from smart-router (for /route diagnostic). */
    lastRouteDecision: { modelId: string; reason: string; tier: number; autoSelected: boolean } | null;

    // Mutation callbacks — page wires these to its real functions
    getTab: (id: string) => { id: string; messages: any[]; selectedModel?: string } | null | undefined;
    addMsg: (tabId: string, msg: any) => any;
    setActiveTab: (id: string) => void;
    setTheme: (theme: string) => void;
    setTabModel: (tabId: string, modelId: string) => void;
    clearTabMessages: (tabId: string) => void;
    openRemoteDiff: (hostNameOrId: string, filePath: string) => void;
    runMultiCompare: (tabId: string, models: string[], prompt: string) => void;
    /** Persist a flag toggle + mirror back into lucyConfig. Page wires both. */
    setSmartRouting: (on: boolean) => void;
    setPrivacyMode:  (on: boolean) => void;
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
                /smart-router on|off · activa/desactiva la elección automática de modelo<br>
                /privacy on|off · hard-lock a Ollama local (sobrepasa cualquier selección cloud)<br>
                /route · muestra la última decisión del smart-router (tier + razón)<br>
                <b style="color:var(--blue)">— Frontier R&D —</b><br>
                /snapshot · captura el estado del sistema (procesos, RAM, discos) en este instante<br>
                /snapshots · lista snapshots recientes<br>
                /diff [from to] · compara dos snapshots (sin args = últimos 2)<br>
                <b style="color:var(--blue)">— Telemetría &amp; salud —</b><br>
                /loop-stats [días=30] · qué modelos disparan más anti-loops (tool / target / max)<br>
                /decay-stats · cuántas entradas Core memory están frescas / envejeciendo / stale<br>
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

        // ── Smart-router + privacy mode (restored from orphaned smart-router.ts) ──
        case 'smart-router':
        case 'smartrouter':
        case 'router': {
            const a = (arg || '').trim().toLowerCase();
            if (a === 'on' || a === '1' || a === 'true' || a === 'enable') {
                ctx.setSmartRouting(true);
                sysMsg(`<div class="mn" style="color:#34d399;">⚙ Smart router: ON</div>
                    <div style="font-size:11px;color:var(--txt2);">Lucy elige modelo automáticamente según complejidad. Tu selección manual sigue siendo respetable como hard-override.</div>`);
            } else if (a === 'off' || a === '0' || a === 'false' || a === 'disable') {
                ctx.setSmartRouting(false);
                sysMsg(`<div class="mn">⚙ Smart router: OFF</div>
                    <div style="font-size:11px;color:var(--txt2);">Lucy usa el modelo del dropdown sin re-routear.</div>`);
            } else {
                const cur = ctx.lucyFlags.smartRouting ? 'ON' : 'OFF';
                sysMsg(`Estado actual: <b>${cur}</b>. Uso: <code>/smart-router on</code> o <code>/smart-router off</code>.`);
            }
            return true;
        }

        case 'privacy':
        case 'privacy-mode': {
            const a = (arg || '').trim().toLowerCase();
            if (a === 'on' || a === '1' || a === 'true' || a === 'enable') {
                ctx.setPrivacyMode(true);
                sysMsg(`<div class="mn" style="color:#34d399;">🔒 Privacy mode: ON</div>
                    <div style="font-size:11px;color:var(--txt2);">Todo el tráfico LLM se enruta a Ollama local. Hard-lock — sobrepasa al smart-router y cualquier selección cloud.</div>`);
            } else if (a === 'off' || a === '0' || a === 'false' || a === 'disable') {
                ctx.setPrivacyMode(false);
                sysMsg(`<div class="mn">🔓 Privacy mode: OFF</div>
                    <div style="font-size:11px;color:var(--txt2);">Modelos cloud habilitados de nuevo.</div>`);
            } else {
                const cur = ctx.lucyFlags.privacyMode ? 'ON' : 'OFF';
                sysMsg(`Estado actual: <b>${cur}</b>. Uso: <code>/privacy on</code> o <code>/privacy off</code>.`);
            }
            return true;
        }

        case 'route': {
            const d = ctx.lastRouteDecision;
            if (!d) {
                sysMsg('No hay decisión de routing reciente. Envía un mensaje con <code>/smart-router on</code> primero.');
            } else {
                const auto = d.autoSelected ? '◆ auto' : '○ manual';
                sysMsg(`<div class="mn">⚙ Last route decision</div>
                    <div style="font-size:11px;">tier <b>${d.tier}</b> · ${auto} · <code>${escapeHtml(d.modelId)}</code></div>
                    <div style="font-size:11px;color:var(--txt2);margin-top:2px;">${escapeHtml(d.reason)}</div>`);
            }
            return true;
        }

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

        // ── F2 Frontier: State snapshots ──────────────────────────────────
        case 'snapshot': case 'snap': {
            sysMsg(ctx.isEN ? 'Capturing system snapshot…' : 'Capturando snapshot del sistema…');
            (async () => {
                try {
                    const id = await invoke<number>('state_snapshot_capture');
                    sysMsg(ctx.isEN
                        ? `Snapshot captured (id=${id}). Use /diff to compare with an earlier one.`
                        : `Snapshot capturado (id=${id}). Usa /diff para comparar con uno anterior.`,
                        'var(--acc)');
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }
        case 'snapshots': case 'snaps': {
            (async () => {
                try {
                    const list = await invoke<Array<{ id: number; captured_at: number; host_name: string }>>(
                        'state_snapshot_list', { sinceTs: null, limit: 20 });
                    if (!list || list.length === 0) {
                        sysMsg(ctx.isEN ? 'No snapshots yet. Run /snapshot to capture one.' : 'No hay snapshots aún. Usa /snapshot para crear uno.', 'var(--amber)');
                        return;
                    }
                    const rows = list.map(s => {
                        const dt = new Date(s.captured_at * 1000).toLocaleString();
                        return `<code style="font-family:var(--mono);font-size:11px;">id=${s.id} · ${dt} · ${s.host_name}</code>`;
                    }).join('<br>');
                    sysMsg(`<b>${ctx.isEN ? 'Recent snapshots:' : 'Snapshots recientes:'}</b><br>${rows}`);
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }
        case 'diff': {
            // /diff <from_id> <to_id>   or   /diff (uses latest 2)
            (async () => {
                try {
                    let fromId: number, toId: number;
                    const parts = arg.split(/\s+/).filter(Boolean);
                    if (parts.length >= 2) {
                        fromId = parseInt(parts[0], 10);
                        toId   = parseInt(parts[1], 10);
                        if (!fromId || !toId) { sysMsg('IDs inválidos. Uso: /diff <from_id> <to_id>', 'var(--amber)'); return; }
                    } else {
                        const list = await invoke<Array<{ id: number }>>('state_snapshot_list', { sinceTs: null, limit: 2 });
                        if (!list || list.length < 2) {
                            sysMsg(ctx.isEN ? 'Need at least 2 snapshots. Run /snapshot twice with some delay.' : 'Se necesitan al menos 2 snapshots. Usa /snapshot dos veces con tiempo entre ellos.', 'var(--amber)');
                            return;
                        }
                        toId   = list[0].id;
                        fromId = list[1].id;
                    }
                    const d = await invoke<any>('state_snapshot_diff', { fromId, toId });
                    const span = Math.round((d.to_ts - d.from_ts) / 60);

                    // v1.4.29 — block-based render. Each major axis
                    // (resources, processes, drives) gets its own
                    // collapsible section so the user can drill into
                    // the dimension they care about without scrolling
                    // a wall of inline HTML.
                    const blocks: ResultBlock[] = [];

                    // Headline section: resources delta, always open.
                    const cpuTone = Math.abs(d.cpu_delta_pct) >= 25 ? 'warn' : 'info';
                    const ramTone = Math.abs(d.ram_delta_mb) >= 1024 ? 'warn' : 'info';
                    const resourceTone = cpuTone === 'warn' || ramTone === 'warn' ? 'warn' : 'info';
                    blocks.push({
                        title: ctx.isEN ? 'Resource delta' : 'Δ de recursos',
                        icon: '◧',
                        tone: resourceTone,
                        defaultOpen: true,
                        html:
                            `<div class="rb-row"><span class="rb-k">CPU</span>` +
                            `<span class="rb-v ${cpuTone === 'warn' ? 'rb-v-warn' : ''}">${d.cpu_delta_pct.toFixed(1)}% Δ</span></div>` +
                            `<div class="rb-row"><span class="rb-k">RAM</span>` +
                            `<span class="rb-v ${ramTone === 'warn' ? 'rb-v-warn' : ''}">${d.ram_delta_mb} MB Δ</span></div>`,
                    });

                    if (d.processes_appeared.length > 0) {
                        const items = d.processes_appeared.slice(0, 12).map((p: any) =>
                            `<code class="rb-chip rb-chip-new">${escapeHtml(p.name)}</code>`).join('');
                        const more = d.processes_appeared.length > 12
                            ? `<span class="rb-more">+${d.processes_appeared.length - 12} ${ctx.isEN ? 'more' : 'más'}</span>` : '';
                        blocks.push({
                            title: `${ctx.isEN ? 'Processes appeared' : 'Procesos aparecidos'} (${d.processes_appeared.length})`,
                            icon: '⊕',
                            tone: 'info',
                            html: `<div class="rb-chips">${items}${more}</div>`,
                        });
                    }
                    if (d.processes_disappeared.length > 0) {
                        const items = d.processes_disappeared.slice(0, 12).map((p: any) =>
                            `<code class="rb-chip rb-chip-gone">${escapeHtml(p.name)}</code>`).join('');
                        const more = d.processes_disappeared.length > 12
                            ? `<span class="rb-more">+${d.processes_disappeared.length - 12} ${ctx.isEN ? 'more' : 'más'}</span>` : '';
                        blocks.push({
                            title: `${ctx.isEN ? 'Processes disappeared' : 'Procesos desaparecidos'} (${d.processes_disappeared.length})`,
                            icon: '⊖',
                            tone: 'warn',
                            html: `<div class="rb-chips">${items}${more}</div>`,
                        });
                    }
                    if (d.drive_changes.length > 0) {
                        const rows = d.drive_changes.map((c: any) => {
                            const sign = c.used_delta_gb >= 0 ? '+' : '';
                            const trend = c.to_pct > c.from_pct ? '↑' : '↓';
                            return `<div class="rb-row"><span class="rb-k">${escapeHtml(c.mount)}</span>` +
                                   `<span class="rb-v">${c.from_pct.toFixed(0)}% → ${c.to_pct.toFixed(0)}% ${trend} (${sign}${c.used_delta_gb} GB)</span></div>`;
                        }).join('');
                        blocks.push({
                            title: `${ctx.isEN ? 'Drive changes' : 'Cambios de discos'} (${d.drive_changes.length})`,
                            icon: '◳',
                            tone: 'info',
                            html: rows,
                        });
                    }

                    // No-change footer when nothing notable found.
                    if (blocks.length === 1 &&
                        d.cpu_delta_pct === 0 && d.ram_delta_mb === 0) {
                        blocks.push({
                            title: ctx.isEN ? 'No significant changes detected' : 'Sin cambios significativos detectados',
                            icon: '◎',
                            tone: 'ok',
                            defaultOpen: true,
                            html: `<div class="rb-row" style="opacity:.7;font-style:italic;">${ctx.isEN
                                ? 'The system was effectively static during this window.'
                                : 'El sistema estuvo prácticamente estático durante esta ventana.'}</div>`,
                        });
                    }

                    sysMsg(renderResultBlocks(
                        ctx.isEN ? `State diff · ${span} min` : `Diff de estado · ${span} min`,
                        blocks));
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }

        // ── F7 Frontier: runbook scan (+ auto-propose skill for high-confidence) ─
        case 'runbooks': case 'workflows': {
            const days = parseInt(arg, 10) || 30;
            (async () => {
                try {
                    const r = await invoke<any>('runbook_scan', { days, topK: 8 });
                    if (!r.candidates || r.candidates.length === 0) {
                        sysMsg(ctx.isEN
                            ? `No repeated workflows in the last ${days}d. Need 3+ sessions with the same 3-step sequence.`
                            : `Sin workflows repetidos en los últimos ${days}d. Se necesitan 3+ sesiones con la misma secuencia de 3 pasos.`,
                            'var(--amber)');
                        return;
                    }
                    const rows = r.candidates.map((c: any) => {
                        const pct = (c.confidence * 100).toFixed(0);
                        // Auto-propose: confidence ≥ 70% gets a "Save as skill" hint
                        const highConf = c.confidence >= 0.70;
                        const proposeChip = highConf
                            ? `<div style="margin-top:4px;font-size:10px;color:var(--blue);">
                                  ⌖ ${ctx.isEN ? 'High confidence — consider saving as skill' : 'Alta confianza — considera guardarlo como skill'}
                              </div>`
                            : '';
                        return `<div style="margin:6px 0;padding:4px 6px;background:rgba(16,185,129,${highConf ? 0.10 : 0.05});border-left:2px solid var(--acc);">
                            <b>${c.suggested_name}</b> · ${c.frequency}× · ${pct}% conf<br>
                            <code style="font-size:10px;opacity:0.85">${c.sequence.join(' → ')}</code>
                            ${proposeChip}
                        </div>`;
                    }).join('');
                    const topHigh = r.candidates.filter((c: any) => c.confidence >= 0.70).length;
                    const summary = topHigh > 0
                        ? `<br><div style="font-size:10px;color:var(--blue);margin-top:6px;">${ctx.isEN
                            ? `→ ${topHigh} candidate(s) ready to be promoted to skills.`
                            : `→ ${topHigh} candidato(s) listo(s) para promocionar a skill.`}</div>`
                        : '';
                    sysMsg(`<b>${ctx.isEN ? 'Detected workflows' : 'Workflows detectados'} (${r.days_analyzed}d, ${r.total_sessions} sessions):</b>${rows}${summary}`);
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }
        // ── F10 Frontier: daily patterns ──────────────────────────────────
        case 'routines': case 'pattern': case 'patterns': {
            const days = parseInt(arg, 10) || 28;
            (async () => {
                try {
                    const r = await invoke<any>('daily_patterns_scan', { days, minConfidence: 0.5 });
                    if (!r.patterns || r.patterns.length === 0) {
                        sysMsg(ctx.isEN
                            ? `No stable weekly routines in the last ${days}d. Need observations in 2+ weeks.`
                            : `Sin rutinas semanales estables en los últimos ${days}d. Se necesitan observaciones en 2+ semanas.`,
                            'var(--amber)');
                        return;
                    }
                    const byDay: Record<string, any[]> = {};
                    for (const p of r.patterns) {
                        (byDay[p.weekday_label] = byDay[p.weekday_label] || []).push(p);
                    }
                    let html = `<b>${ctx.isEN ? 'Daily routines' : 'Rutinas diarias'} (${r.weeks_covered} weeks):</b><br>`;
                    for (const day of ['Lun','Mar','Mié','Jue','Vie','Sáb','Dom']) {
                        if (!byDay[day]) continue;
                        html += `<div style="margin-top:4px;"><b style="color:var(--acc);">${day}</b><br>`;
                        for (const p of byDay[day].slice(0, 5)) {
                            const conf = (p.confidence * 100).toFixed(0);
                            const ico = p.kind === 'process' ? '⊞' : '⌨';
                            html += `&nbsp;&nbsp;<code style="font-size:10px">${p.hour_band}h ${ico} ${p.signal}</code> <span style="opacity:0.6;font-size:10px">${p.weeks_observed}w · ${conf}%</span><br>`;
                        }
                        html += `</div>`;
                    }
                    sysMsg(html);
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }
        // ── F5 Frontier: sandbox preview ──────────────────────────────────
        case 'preview': case 'sandbox': {
            if (!arg) {
                sysMsg(ctx.isEN ? 'Usage: /preview <command>' : 'Uso: /preview <comando>', 'var(--amber)');
                return true;
            }
            (async () => {
                try {
                    const r = await invoke<any>('sandbox_preview_command', { command: arg });
                    const pct = (r.risk_score * 100).toFixed(0);
                    const bandColor = r.risk_band === 'destructive' ? 'var(--red)'
                                    : r.risk_band === 'review' ? 'var(--amber)' : 'var(--acc)';
                    let html = `<b style="color:${bandColor};">⊠ ${r.risk_band.toUpperCase()} · risk ${pct}%</b><br>`;
                    html += `<code style="font-size:10px;opacity:0.85">${arg.slice(0, 200).replace(/[<>&]/g, m => ({'<':'&lt;','>':'&gt;','&':'&amp;'}[m] || m))}</code><br>`;
                    if (r.destructive_reason) html += `<div style="color:var(--red);">⚠ ${r.destructive_reason}</div>`;
                    if (r.elevation_required) html += `<div style="color:var(--amber);">⚡ ${ctx.isEN ? 'Requires elevation' : 'Requiere elevación'}</div>`;
                    if (r.affected_paths?.length) html += `<div>📁 ${r.affected_paths.length} path(s)</div>`;
                    if (r.affected_registry_keys?.length) html += `<div>🗝 ${r.affected_registry_keys.length} registry key(s)</div>`;
                    if (r.affected_services?.length) html += `<div>⚙ ${r.affected_services.length} service(s)</div>`;
                    if (r.network_endpoints?.length) html += `<div>🌐 ${r.network_endpoints.join(', ')}</div>`;
                    if (r.sandbox_wsb_path) {
                        html += `<div style="margin-top:6px;padding:4px 6px;background:rgba(59,158,255,0.10);border-left:2px solid var(--blue);font-size:10px;">
                            ⊠ Windows Sandbox config saved: <code>${r.sandbox_wsb_path}</code><br>
                            ${ctx.isEN ? 'Double-click that file to run the command in isolation.' : 'Doble clic ahí para ejecutar en aislamiento.'}
                        </div>`;
                    }
                    sysMsg(html);
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }

        // ── F9 Frontier: knowledge graph ─────────────────────────────────
        case 'kg-add': {
            if (!arg) { sysMsg(ctx.isEN ? 'Usage: /kg-add <directory-path>' : 'Uso: /kg-add <ruta-directorio>', 'var(--amber)'); return true; }
            (async () => {
                try {
                    await invoke('kg_add_root', { root: arg });
                    sysMsg(`✓ Root added: <code>${arg}</code>. Will be indexed within ~5 min, or run /kg-scan to force now.`);
                } catch (e) { sysMsg(`Error: ${String(e)}`, 'var(--red)'); }
            })();
            return true;
        }
        case 'kg-rm': case 'kg-remove': {
            if (!arg) { sysMsg('Usage: /kg-rm <directory-path>', 'var(--amber)'); return true; }
            (async () => {
                try {
                    await invoke('kg_remove_root', { root: arg });
                    sysMsg(`✓ Root removed: <code>${arg}</code>`);
                } catch (e) { sysMsg(`Error: ${String(e)}`, 'var(--red)'); }
            })();
            return true;
        }
        case 'kg-roots': case 'kg-list': {
            (async () => {
                try {
                    const roots = await invoke<string[]>('kg_list_roots');
                    if (!roots || roots.length === 0) {
                        sysMsg(ctx.isEN ? 'No KG roots configured. Use /kg-add <path> to start tracking.' : 'Sin roots configurados. Usa /kg-add <ruta>.', 'var(--amber)');
                        return;
                    }
                    sysMsg(`<b>Knowledge Graph roots:</b><br>${roots.map(r => `<code>${r}</code>`).join('<br>')}`);
                } catch (e) { sysMsg(`Error: ${String(e)}`, 'var(--red)'); }
            })();
            return true;
        }
        // ── F7 Sprint 7: promote a runbook candidate into a saved skill ──
        case 'promote-runbook': case 'promote': {
            // Usage: /promote-runbook <name> :: <step1> ; <step2> ; <step3>
            // Example: /promote-runbook git-flow :: git status ; git add ; git commit ; git push
            const m = arg.match(/^([^\s:]+)\s*::\s*(.+)$/);
            if (!m) {
                sysMsg(ctx.isEN
                    ? 'Usage: /promote-runbook <name> :: <cmd1> ; <cmd2> ; <cmd3>'
                    : 'Uso: /promote-runbook <nombre> :: <cmd1> ; <cmd2> ; <cmd3>', 'var(--amber)');
                return true;
            }
            const name = m[1].trim();
            const sequence = m[2].split(';').map(s => s.trim()).filter(Boolean);
            if (sequence.length < 2) {
                sysMsg(ctx.isEN ? 'Need at least 2 steps.' : 'Se necesitan al menos 2 pasos.', 'var(--amber)');
                return true;
            }
            (async () => {
                try {
                    const r = await invoke<any>('runbook_promote', {
                        args: { sequence, name, description: null, category: 'workflow' }
                    });
                    sysMsg(`<b style="color:var(--acc);">✓ Skill saved</b><br>
                        <code>${r.name}</code> (id: <code>${r.skill_id}</code>)<br>
                        <pre style="font-size:10px;margin:4px 0;padding:4px;background:rgba(0,0,0,0.25);">${r.script.replace(/[<>&]/g, (c: string) => (({'<':'&lt;','>':'&gt;','&':'&amp;'} as Record<string, string>)[c] || c))}</pre>
                        ${ctx.isEN ? 'Invoke later with /skill or by typing the name.' : 'Invócalo después con /skill o tecleando el nombre.'}`);
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }
        // ── Cross-feature: incident detective ────────────────────────────
        case 'detective': case 'investigate': {
            const windowSec = parseInt(arg, 10) || 300;
            sysMsg(ctx.isEN ? `Investigating window ±${windowSec}s…` : `Investigando ventana ±${windowSec}s…`);
            (async () => {
                try {
                    const r = await invoke<any>('incident_detective', { symptomTs: null, windowSec });
                    const pct = (r.confidence * 100).toFixed(0);
                    // Top-level tone bucket → tints the headline color
                    // and the first (narrative) block's border.
                    const tone: 'ok' | 'warn' | 'crit' =
                        r.confidence >= 0.55 ? 'crit' :
                        r.confidence >= 0.30 ? 'warn' : 'ok';

                    // v1.4.29 — block-based render. Narrative defaults
                    // OPEN (the "what Lucy thinks happened" summary the
                    // user came to read); threats/causes/files default
                    // closed so a clean inbox stays clean.
                    const blocks: ResultBlock[] = [];

                    blocks.push({
                        title: ctx.isEN ? 'Narrative' : 'Narrativa',
                        icon: '🔎',
                        tone,
                        defaultOpen: true,
                        html: `<div class="rb-narrative">${escapeHtml(r.narrative || '')}</div>`,
                    });

                    if (r.threats?.length) {
                        const rows = r.threats.slice(0, 8).map((t: any) => {
                            const tp = (t.score * 100).toFixed(0);
                            return `<div class="rb-row">` +
                                `<code class="rb-chip rb-chip-band-${escapeHtml(String(t.band || 'info'))}">${escapeHtml(t.band || '')}</code>` +
                                `<span class="rb-k">${escapeHtml(t.name)}</span>` +
                                `<span class="rb-v">pid ${t.pid} · ${tp}%</span>` +
                            `</div>`;
                        }).join('');
                        blocks.push({
                            title: `${ctx.isEN ? 'Threats' : 'Amenazas'} (${r.threats.length})`,
                            icon: '⚠',
                            tone: r.threats.some((t: any) => t.band === 'crit' || t.score >= 0.7) ? 'crit' : 'warn',
                            html: rows,
                        });
                    }
                    if (r.causal?.candidates?.length) {
                        const rows = r.causal.candidates.slice(0, 8).map((c: any) => {
                            const cp = (c.confidence * 100).toFixed(0);
                            return `<div class="rb-row">` +
                                `<span class="rb-k">${escapeHtml(c.name)}</span>` +
                                `<span class="rb-v">pid ${c.pid} · ${cp}%</span>` +
                            `</div>`;
                        }).join('');
                        blocks.push({
                            title: `${ctx.isEN ? 'Causal candidates' : 'Candidatos causales'} (${r.causal.candidates.length})`,
                            icon: '⌖',
                            tone: 'info',
                            html: rows,
                        });
                    }
                    if (r.file_changes?.length) {
                        blocks.push({
                            title: `${ctx.isEN ? 'File activity' : 'Actividad de archivos'} (${r.file_changes.length})`,
                            icon: '⊞',
                            tone: 'info',
                            html: `<div class="rb-row">${escapeHtml(r.touched_cluster_summary || '')}</div>`,
                        });
                    }

                    sysMsg(renderResultBlocks(
                        `🔎 ${ctx.isEN ? 'Detective' : 'Detective'} · ${pct}% ${ctx.isEN ? 'confidence' : 'confianza'}`,
                        blocks));
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }
        case 'kg-scan': {
            sysMsg(ctx.isEN ? 'Scanning KG roots…' : 'Escaneando roots del KG…');
            (async () => {
                try {
                    const lookback = parseInt(arg, 10) || (60 * 24); // default 24h lookback
                    const r = await invoke<any>('kg_index_now', { sinceMin: lookback });
                    if (!r.roots || r.roots.length === 0) {
                        sysMsg(ctx.isEN ? 'No roots to scan. Add one with /kg-add.' : 'Sin roots para escanear. Usa /kg-add.', 'var(--amber)');
                        return;
                    }
                    const rows = r.roots.map((s: any) =>
                        `<code style="font-size:10px">${s.root}</code>: ${s.files_recorded} files, ${s.edges_added} edges (${s.elapsed_ms}ms)`
                    ).join('<br>');
                    sysMsg(`<b>KG scan complete</b> (${r.total_files_scanned} scanned, ${r.total_files_recorded} recorded, ${r.total_edges} edges)<br>${rows}`);
                } catch (e) { sysMsg(`Error: ${String(e)}`, 'var(--red)'); }
            })();
            return true;
        }

        // ── Sprint 8: skill picker UI ────────────────────────────────────
        case 'skills': case 'skill-list': {
            if (ctx.openSkillPicker) {
                ctx.openSkillPicker();
            } else {
                sysMsg('Skill picker UI not wired in this context.', 'var(--amber)');
            }
            return true;
        }
        // ── v1.6.1: ECC-adapted skill preset picker ──────────────────────
        // /preset, /presets, /skill-preset all open the modal. The picker
        // sets a behavioural framing prepended to the system prompt; it
        // does NOT execute scripts (those are /skills).
        case 'preset': case 'presets': case 'skill-preset': {
            if (ctx.openSkillPresetPicker) {
                ctx.openSkillPresetPicker();
            } else {
                sysMsg('Skill preset picker UI not wired in this context.', 'var(--amber)');
            }
            return true;
        }
        // ── Sprint 8: KG mini-viewer ─────────────────────────────────────
        case 'kg-view': case 'kg-viz': {
            if (!arg) {
                sysMsg(ctx.isEN ? 'Usage: /kg-view <full-file-path>' : 'Uso: /kg-view <ruta-completa>', 'var(--amber)');
                return true;
            }
            if (ctx.openKgViewer) {
                ctx.openKgViewer(arg);
            } else {
                sysMsg('KG viewer not wired in this context.', 'var(--amber)');
            }
            return true;
        }
        // ── Sprint 8: Frontier telemetry summary ─────────────────────────
        case 'frontier-stats': case 'telemetry': {
            (async () => {
                try {
                    const rows = await invoke<any[]>('frontier_telemetry_summary');
                    if (!rows || rows.length === 0) {
                        sysMsg(ctx.isEN
                            ? 'No Frontier telemetry recorded yet. Use any Frontier tool (state_diff, threat_scan, detective…) and it will start populating.'
                            : 'Sin telemetría aún. Usa cualquier Frontier tool y comenzará a poblarse.',
                            'var(--amber)');
                        return;
                    }
                    const top = rows.slice(0, 12);
                    const total = rows.reduce((s, r) => s + r.invocations, 0);
                    const html = top.map((r) => {
                        const pct = total > 0 ? ((r.invocations / total) * 100).toFixed(0) : '0';
                        const avg = r.avg_ms ? `${r.avg_ms.toFixed(0)}ms` : '—';
                        const errPct = (r.error_rate * 100).toFixed(0);
                        const errStyle = r.error_rate > 0.1 ? 'color:var(--red)' : 'color:var(--text-muted)';
                        return `<div style="display:flex;gap:8px;font-size:10px;font-family:var(--mono);padding:2px 0;">
                            <code style="flex:1">${r.feature_id}</code>
                            <span style="color:var(--acc)">${r.invocations}×</span>
                            <span>${pct}%</span>
                            <span>${avg}</span>
                            <span style="${errStyle}">${errPct}% err</span>
                        </div>`;
                    }).join('');
                    sysMsg(`<b>${ctx.isEN ? 'Frontier feature usage' : 'Uso de features Frontier'} (${total} total invocations)</b>${html}`);
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }

        // ── /notebook — export tab as Jupyter .ipynb (v1.4.4 / Quick-win K) ──
        case 'notebook': case 'ipynb': case 'export-notebook': {
            (async () => {
                try {
                    const { buildNotebook, notebookToIpynb } = await import('$lib/notebook');
                    const nb = buildNotebook(t, {
                        lang:        ctx.isEN ? 'en-US' : 'es-MX',
                        lucyVersion: '1.4.4',
                        title:       (t as any).title,
                    });
                    if (!nb.cells || nb.cells.length === 0) {
                        sysMsg(ctx.isEN
                            ? 'Nothing to export — this tab is empty.'
                            : 'Nada que exportar — esta pestaña está vacía.', 'var(--amber)');
                        return;
                    }
                    const ipynbStr = notebookToIpynb(nb);
                    const defaultName = (((t as any).title as string) || 'lucy-session')
                        .replace(/[^\w\-]+/g, '_').slice(0, 60) + '.ipynb';
                    // pick_save_path is a Rust command (uses rfd::FileDialog)
                    const path = await invoke<string>('pick_save_path', {
                        defaultName, filterName: 'Jupyter Notebook', filterExts: ['ipynb'],
                    });
                    if (!path) return; // user cancelled
                    await invoke('write_file_content', { path, content: ipynbStr, force: true });
                    sysMsg(ctx.isEN
                        ? `✓ Exported ${nb.cells.length} cells to <code>${path}</code>`
                        : `✓ Exportadas ${nb.cells.length} celdas a <code>${path}</code>`,
                        'var(--acc)');
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }

        // ── /revert — undo the most recent writefile (v1.4.4 / Quick-win E) ──
        case 'revert': case 'undo-write': {
            (async () => {
                try {
                    // Per-tab undo buffer (was window._lucyWriteUndo before code
                    // review flagged the cross-tab collision). The agent loop
                    // populates `t._writeUndo` after every writefile.
                    const buf = (t as any)._writeUndo as Map<string, string> | undefined;
                    if (!buf || buf.size === 0) {
                        sysMsg(ctx.isEN
                            ? 'No write to revert. The undo buffer is only populated AFTER Lucy writes a file in this session.'
                            : 'No hay nada que revertir. El buffer se llena después de que Lucy escribe un archivo en esta sesión.',
                            'var(--amber)');
                        return;
                    }
                    const argPath = arg.trim();
                    // No path → revert the most recent one (last inserted).
                    const targetPath = argPath || Array.from(buf.keys()).pop()!;
                    if (!buf.has(targetPath)) {
                        sysMsg(ctx.isEN
                            ? `No undo buffer for "${targetPath}". Try one of: ${Array.from(buf.keys()).join(', ')}`
                            : `Sin buffer para "${targetPath}". Disponibles: ${Array.from(buf.keys()).join(', ')}`,
                            'var(--red)');
                        return;
                    }
                    const restoreTo = buf.get(targetPath) || '';
                    await invoke('write_file_content', { path: targetPath, content: restoreTo, force: true });
                    buf.delete(targetPath);
                    sysMsg(ctx.isEN
                        ? `✓ Reverted <code>${targetPath}</code> to its pre-write content (${restoreTo.length} chars).`
                        : `✓ Revertido <code>${targetPath}</code> al contenido previo (${restoreTo.length} chars).`,
                        'var(--acc)');
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }

        // ── /chip-stats — predictive-chip engagement summary (v1.4.3) ─────
        case 'chip-stats': case 'chips-stats': {
            (async () => {
                try {
                    const sum = await invoke<any>('chip_stats_summary', { days: 7 });
                    if (!sum || sum.total_clicks + sum.total_dismisses === 0) {
                        sysMsg(ctx.isEN
                            ? 'No chip activity in the last 7 days. Click or dismiss any predictive chip and stats will populate.'
                            : 'Sin actividad de chips en los últimos 7 días. Haz clic o descarta un chip y comenzará a llenarse.',
                            'var(--amber)');
                        return;
                    }
                    const hdr = ctx.isEN
                        ? `Chip engagement · last ${sum.days} days`
                        : `Engagement de chips · últimos ${sum.days} días`;
                    const totals = ctx.isEN
                        ? `${sum.total_clicks} clicks · ${sum.total_dismisses} dismisses · ${sum.unique_labels} unique`
                        : `${sum.total_clicks} clicks · ${sum.total_dismisses} descartes · ${sum.unique_labels} únicos`;
                    const rows = (sum.top || []).slice(0, 12).map((r: any) => {
                        const label = String(r.label || '').slice(0, 32);
                        const ratio = r.clicks > 0 || r.dismisses > 0
                            ? `${r.clicks}c / ${r.dismisses}d`
                            : '—';
                        const netColor = r.net >= 3 ? 'var(--acc)'
                                       : r.net <= 0 ? 'var(--red)'
                                       : 'var(--text-muted)';
                        return `<div style="display:flex;gap:8px;font-size:10px;font-family:var(--mono);padding:2px 0;">
                            <code style="flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">${label}</code>
                            <span style="color:var(--text-muted);min-width:62px;text-align:right;">${ratio}</span>
                            <span style="color:${netColor};min-width:42px;text-align:right;">net ${r.net.toFixed(1)}</span>
                        </div>`;
                    }).join('');
                    sysMsg(`<b>${hdr}</b><div style="font-size:10.5px;color:var(--text-muted);margin:2px 0 6px;">${totals}</div>${rows}`);
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }

        // ── v1.6.4 — /instinct-status (ECC continuous-learning-v2) ──
        // Renders Layer 3 chip patterns as confidence-banded instincts.
        // Compared to /chip-stats (raw counts), this presents the SAME
        // data through the ECC framing: an "instinct" is a learned
        // behaviour with sustained signal. Bands:
        //   instinct   net >= 3.0  AND  clicks >= 3   → kept as memory
        //   suggestion net 1.0–3.0 OR    clicks >= 2   → on watchlist
        //   noise      net <= 0    OR    only 1 sample → discard if old
        case 'instinct-status': case 'instincts': {
            const days = parseInt(arg, 10) || 14;
            (async () => {
                try {
                    const sum = await invoke<any>('chip_stats_summary', { days });
                    if (!sum || sum.total_clicks + sum.total_dismisses === 0) {
                        sysMsg(ctx.isEN
                            ? `No chip activity in the last ${days} days. Lucy needs interaction signal to learn instincts.`
                            : `Sin actividad de chips en los últimos ${days} días. Lucy necesita señal de interacción para aprender instintos.`,
                            'var(--amber)');
                        return;
                    }
                    // Bucket each row by band.
                    const rows = (sum.top || []) as Array<{ label: string; clicks: number; dismisses: number; net: number; last_at: number }>;
                    const instincts:   typeof rows = [];
                    const suggestions: typeof rows = [];
                    const noise:       typeof rows = [];
                    for (const r of rows) {
                        if (r.net >= 3.0 && r.clicks >= 3)            instincts.push(r);
                        else if (r.net >= 1.0 || r.clicks >= 2)       suggestions.push(r);
                        else                                          noise.push(r);
                    }
                    // Helper: render one row as a chip with confidence pct.
                    const renderRow = (r: { label: string; clicks: number; dismisses: number; net: number; last_at: number }) => {
                        const total = r.clicks + r.dismisses;
                        const pct = total > 0 ? Math.round((r.clicks / total) * 100) : 0;
                        const ageDays = Math.max(0, Math.floor((Date.now() / 1000 - r.last_at) / 86400));
                        return `<div class="rb-row">` +
                            `<span class="rb-k">${escapeHtml(r.label).slice(0, 28)}</span>` +
                            `<span class="rb-v">${r.clicks}c / ${r.dismisses}d · ${pct}% · ${ageDays}d ago</span>` +
                        `</div>`;
                    };

                    const blocks: ResultBlock[] = [];
                    blocks.push({
                        title: ctx.isEN ? 'Summary' : 'Resumen',
                        icon: '◆',
                        tone: 'info',
                        defaultOpen: true,
                        html:
                            `<div class="rb-row"><span class="rb-k">${ctx.isEN ? 'Total' : 'Total'}</span>` +
                            `<span class="rb-v">${sum.total_clicks} clicks · ${sum.total_dismisses} dismisses · ${sum.unique_labels} ${ctx.isEN ? 'unique labels' : 'etiquetas únicas'}</span></div>` +
                            `<div class="rb-row"><span class="rb-k">${ctx.isEN ? 'Bands' : 'Bandas'}</span>` +
                            `<span class="rb-v">${instincts.length} instincts · ${suggestions.length} suggestions · ${noise.length} noise</span></div>`,
                    });
                    if (instincts.length) {
                        blocks.push({
                            title: `${ctx.isEN ? 'Instincts' : 'Instintos'} (${instincts.length})`,
                            icon: '⚡',
                            tone: 'ok',
                            defaultOpen: true,
                            html: instincts.slice(0, 20).map(renderRow).join(''),
                        });
                    }
                    if (suggestions.length) {
                        blocks.push({
                            title: `${ctx.isEN ? 'Suggestions on watchlist' : 'Sugerencias en watchlist'} (${suggestions.length})`,
                            icon: '◇',
                            tone: 'warn',
                            html: suggestions.slice(0, 20).map(renderRow).join(''),
                        });
                    }
                    if (noise.length) {
                        blocks.push({
                            title: `${ctx.isEN ? 'Noise (candidates to prune)' : 'Ruido (candidatos a podar)'} (${noise.length})`,
                            icon: '⊘',
                            tone: 'crit',
                            html: noise.slice(0, 12).map(renderRow).join(''),
                        });
                    }
                    sysMsg(renderResultBlocks(
                        ctx.isEN ? `⚡ Instinct status · last ${days} days` : `⚡ Estado de instintos · últimos ${days} días`,
                        blocks));
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }

        // ── v1.6.4 — /evolve (ECC continuous-learning-v2 step) ──
        // Surfaces recurring instincts (clicks >= 4 AND net >= 4) that
        // are worth promoting to executable skills. Just proposes —
        // never auto-creates. The user clicks the "Save as skill" hint
        // (rendered as inline copy text) to act on the suggestion.
        case 'evolve': case 'instinct-evolve': {
            const days = parseInt(arg, 10) || 30;
            (async () => {
                try {
                    const sum = await invoke<any>('chip_stats_summary', { days });
                    const rows = (sum?.top || []) as Array<{ label: string; clicks: number; dismisses: number; net: number; last_at: number }>;
                    const candidates = rows.filter(r => r.clicks >= 4 && r.net >= 4 && (r.dismisses === 0 || r.clicks / r.dismisses >= 3));
                    if (candidates.length === 0) {
                        sysMsg(ctx.isEN
                            ? `No evolution candidates in the last ${days} days. Looking for: ≥ 4 clicks AND net engagement ≥ 4 AND click/dismiss ratio ≥ 3:1.`
                            : `Sin candidatos para evolución en los últimos ${days} días. Buscando: ≥ 4 clicks AND engagement neto ≥ 4 AND ratio click/dismiss ≥ 3:1.`,
                            'var(--amber)');
                        return;
                    }
                    const renderCandidate = (r: typeof candidates[number], i: number) => {
                        const ratio = r.dismisses === 0 ? '∞' : (r.clicks / r.dismisses).toFixed(1);
                        return `<details class="rb-block rb-tone-ok">` +
                            `<summary class="rb-summary">` +
                            `<span class="rb-ico">${i + 1}</span>` +
                            `<span class="rb-title">${escapeHtml(r.label).slice(0, 40)}</span>` +
                            `<span class="rb-chev">▾</span>` +
                            `</summary>` +
                            `<div class="rb-body">` +
                            `<div class="rb-row"><span class="rb-k">Signal</span>` +
                            `<span class="rb-v">${r.clicks}c / ${r.dismisses}d · ratio ${ratio} · net ${r.net.toFixed(1)}</span></div>` +
                            `<div class="rb-row"><span class="rb-k">${ctx.isEN ? 'Proposal' : 'Propuesta'}</span>` +
                            `<span class="rb-v">${ctx.isEN ? 'Open' : 'Abrir'} <code>/skills</code> ${ctx.isEN ? "and save a script triggered by this label." : 'y guarda un script disparado por esta etiqueta.'}</span></div>` +
                            `</div></details>`;
                    };
                    const headline = ctx.isEN
                        ? `✦ Evolution candidates · ${candidates.length} pattern(s) ready to consolidate`
                        : `✦ Candidatos a evolucionar · ${candidates.length} patrón(es) listos para consolidar`;
                    const intro = `<div class="rb-hdr">${headline}</div>` +
                        `<div class="rb-block rb-tone-info" open><div class="rb-body">${ctx.isEN
                            ? 'These chips have crossed the engagement threshold. Each is a candidate to become an executable skill via <code>/skills</code> so it stops needing Layer 3 ranking and becomes a deterministic shortcut.'
                            : 'Estos chips superaron el umbral de engagement. Cada uno es candidato a convertirse en una skill ejecutable vía <code>/skills</code> para que deje de necesitar ranking Layer 3 y pase a ser un atajo determinístico.'}</div></div>`;
                    const body = candidates.slice(0, 10).map(renderCandidate).join('');
                    sysMsg(`<div class="rb-wrap">${intro}${body}</div>`);
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }

        // ── v1.6.6 — /anneal (Kappa Graph ADR-200 annealing ontologies) ──
        // Read-only: scores each tag-cluster of agent_memories on mass /
        // coherence / exposure and emits promote/demote/watch verdicts.
        // The graph proposes; no mutations happen here. See ADR-200
        // §"Phase 3 produces proposals, not executions".
        case 'anneal': case 'ontology': case 'ontologies': {
            (async () => {
                try {
                    const rep = await invoke<any>('memory_annealing_report');
                    if (!rep || rep.n_clusters === 0) {
                        sysMsg(ctx.isEN
                            ? `No tagged memories yet. Annealing needs at least 2 memories sharing a tag to score a cluster.`
                            : `Aún no hay memorias etiquetadas. El annealing necesita al menos 2 memorias compartiendo una etiqueta.`,
                            'var(--amber)');
                        return;
                    }
                    type OS = { name: string; members: number; mass: number; coherence: number; exposure: number; promotion_score: number; protection_score: number; lifecycle_state: string; verdict: string };
                    const all: OS[] = rep.clusters || [];
                    const promote = all.filter(c => c.verdict === 'promote');
                    const demote  = all.filter(c => c.verdict === 'demote');
                    const watch   = all.filter(c => c.verdict === 'watch');
                    const stable  = all.filter(c => c.verdict === 'no_action');
                    const pct = (x: number) => `${Math.round(x * 100)}%`;
                    const renderRow = (c: OS) =>
                        `<div class="rb-row">` +
                        `<span class="rb-k">${escapeHtml(c.name).slice(0, 24)} <em style="opacity:.6;font-style:normal;">(${c.members})</em></span>` +
                        `<span class="rb-v">mass ${pct(c.mass)} · coh ${pct(c.coherence)} · exp ${pct(c.exposure)} · ${c.lifecycle_state}</span>` +
                        `</div>`;
                    const blocks: ResultBlock[] = [];
                    blocks.push({
                        title: ctx.isEN ? 'Summary' : 'Resumen',
                        icon: '◆', tone: 'info', defaultOpen: true,
                        html:
                            `<div class="rb-row"><span class="rb-k">${ctx.isEN ? 'Global epoch' : 'Época global'}</span>` +
                            `<span class="rb-v">${rep.global_epoch} ${ctx.isEN ? 'memories ever' : 'memorias totales'}</span></div>` +
                            `<div class="rb-row"><span class="rb-k">${ctx.isEN ? 'Clusters' : 'Cúmulos'}</span>` +
                            `<span class="rb-v">${rep.n_clusters} ${ctx.isEN ? 'scored' : 'evaluados'} · ${promote.length} promote · ${demote.length} demote · ${watch.length} watch · ${stable.length} stable</span></div>`,
                    });
                    if (promote.length) blocks.push({
                        title: `${ctx.isEN ? 'Promotion candidates' : 'Candidatos a promoción'} (${promote.length})`,
                        icon: '↥', tone: 'ok', defaultOpen: true,
                        html: promote.slice(0, 10).map(renderRow).join(''),
                    });
                    if (demote.length) blocks.push({
                        title: `${ctx.isEN ? 'Demotion candidates' : 'Candidatos a democión'} (${demote.length})`,
                        icon: '↧', tone: 'crit',
                        html: demote.slice(0, 10).map(renderRow).join(''),
                    });
                    if (watch.length) blocks.push({
                        title: `${ctx.isEN ? 'Watch (borderline)' : 'Vigilar (frontera)'} (${watch.length})`,
                        icon: '◇', tone: 'warn',
                        html: watch.slice(0, 10).map(renderRow).join(''),
                    });
                    if (stable.length) blocks.push({
                        title: `${ctx.isEN ? 'Stable / no action' : 'Estables / sin acción'} (${stable.length})`,
                        icon: '◯', tone: 'info',
                        html: stable.slice(0, 10).map(renderRow).join(''),
                    });
                    sysMsg(renderResultBlocks(
                        ctx.isEN ? `⌬ Annealing report · ${rep.n_clusters} cluster(s)` : `⌬ Reporte de annealing · ${rep.n_clusters} cúmulo(s)`,
                        blocks));
                } catch (e) {
                    sysMsg(`Error: ${String(e)}`, 'var(--red)');
                }
            })();
            return true;
        }

        // ── /loop-stats — agent-loop blocks by model (May 2026 telemetry) ──
        // Shows which models trigger the safety nets (tool-loop, target-loop,
        // error-repeat, max-loops) most often. Drives model-selection decisions.
        case 'loop-stats':
        case 'loops':
        case 'loopstats': {
            const days = parseInt(arg, 10);
            runLoopStats(Number.isFinite(days) && days > 0 ? days : 30, sysMsg);
            return true;
        }

        // ── /decay-stats — Core memory decay status (May 2026) ──
        // Shows how many Core memory entries are fresh / aging / stale, so
        // the user can decide what to re-affirm or prune.
        case 'decay-stats':
        case 'decay':
        case 'memorystats': {
            runDecayStats(sysMsg);
            return true;
        }

        default:
            sysMsg(`Comando desconocido: /${cmd}. Usa /help para ver disponibles.`, 'var(--amber)');
            return true;
    }
}

// ── /loop-stats runner ─────────────────────────────────────────────────────

interface LoopBlockStatRow {
    model:         string;
    event_subtype: string;
    count:         number;
    last_ts:       number;
}

/** Render the agent-loop block telemetry as an inline table. */
function runLoopStats(days: number, sysMsg: (html: string, color?: string) => void) {
    (async () => {
        try {
            const rows = await invoke<LoopBlockStatRow[]>('loop_block_stats', { days });
            if (!rows || rows.length === 0) {
                sysMsg(`<div class="mn">⌬ Loop telemetry (últimos ${days} días)</div>
                    <div style="margin-top:6px;font-size:11px;color:var(--txt2);">
                        Ninguna trigger de loop registrada — Lucy ha estado terminando turnos sin tropezar con
                        las safety nets. Si esperabas ver datos, verifica que los agent loops están corriendo
                        (cualquier mensaje que use &lt;TOOL&gt; o &lt;EXECUTE&gt;) y que haya pasado un mensaje
                        después de los fixes.
                    </div>`);
                return;
            }
            // Subtype → label/color/icon
            const subtypeMeta: Record<string, { label: string; color: string; icon: string }> = {
                tool_loop:    { label: 'Tool loop',    color: '#f59e0b', icon: '↻' },
                target_loop:  { label: 'Target loop',  color: '#ef4444', icon: '⊗' },
                error_repeat: { label: 'Error repeat', color: '#f87171', icon: '⚠' },
                max_loops:    { label: 'MAX_LOOPS',    color: '#dc2626', icon: '⊘' },
            };
            // Group by model so we can print one block per model with subtotals
            const byModel: Record<string, LoopBlockStatRow[]> = {};
            let grandTotal = 0;
            for (const r of rows) {
                (byModel[r.model] ||= []).push(r);
                grandTotal += r.count;
            }
            const modelOrder = Object.keys(byModel).sort((a, b) => {
                const sumA = byModel[a].reduce((s, r) => s + r.count, 0);
                const sumB = byModel[b].reduce((s, r) => s + r.count, 0);
                return sumB - sumA;
            });
            const formatAge = (ts: number) => {
                const ageSec = Math.max(0, Math.floor(Date.now() / 1000) - ts);
                if (ageSec < 60)    return `hace ${ageSec}s`;
                if (ageSec < 3600)  return `hace ${Math.floor(ageSec / 60)}m`;
                if (ageSec < 86400) return `hace ${Math.floor(ageSec / 3600)}h`;
                return `hace ${Math.floor(ageSec / 86400)}d`;
            };
            const modelBlocks = modelOrder.map(model => {
                const items = byModel[model];
                const sum = items.reduce((s, r) => s + r.count, 0);
                const rowsHtml = items.map(r => {
                    const meta = subtypeMeta[r.event_subtype] || { label: r.event_subtype, color: 'var(--txt2)', icon: '·' };
                    return `<tr>
                        <td style="padding:2px 8px 2px 0;color:${meta.color};">${meta.icon} ${meta.label}</td>
                        <td style="padding:2px 8px 2px 0;text-align:right;font-variant-numeric:tabular-nums;">${r.count}</td>
                        <td style="padding:2px 0;color:var(--txt3);font-size:10.5px;">${formatAge(r.last_ts)}</td>
                    </tr>`;
                }).join('');
                return `<div style="margin-top:8px;">
                    <div style="font-family:var(--mono);font-size:11.5px;color:var(--txt);"><b>${escapeHtml(model)}</b> <span style="color:var(--txt3);">— ${sum} block${sum === 1 ? '' : 's'}</span></div>
                    <table style="border-collapse:collapse;margin-top:2px;font-size:11px;">
                        ${rowsHtml}
                    </table>
                </div>`;
            }).join('');
            sysMsg(`<div class="mn">⌬ Loop telemetry (últimos ${days} días)</div>
                <div style="margin-top:2px;font-size:11px;color:var(--txt2);">
                    <b style="color:var(--txt);">${grandTotal}</b> trigger${grandTotal === 1 ? '' : 's'} totales
                    en <b>${modelOrder.length}</b> modelo${modelOrder.length === 1 ? '' : 's'}.
                    Modelos con muchos blocks suelen beneficiarse de un upgrade (Flash → Pro, Haiku → Sonnet) para tareas complejas.
                </div>
                ${modelBlocks}`);
        } catch (e) {
            sysMsg(`loop_block_stats falló: ${String(e).substring(0, 200)}`, 'var(--red)');
        }
    })();
}

// ── /decay-stats runner ────────────────────────────────────────────────────

interface DecayStatsResp {
    total:  number;
    fresh:  number;
    aging:  number;
    stale:  number;
    pinned: number;
    inject_threshold: number;
    aging_threshold:  number;
}

/** Render the Core memory decay summary. */
function runDecayStats(sysMsg: (html: string, color?: string) => void) {
    (async () => {
        try {
            const s = await invoke<DecayStatsResp>('memory_core_decay_stats');
            if (!s || s.total === 0) {
                sysMsg(`<div class="mn">⌬ Core memory decay</div>
                    <div style="margin-top:6px;font-size:11px;color:var(--txt2);">
                        No hay entradas Core memory todavía. Lucy las añade automáticamente cuando
                        descubre facts sobre tu identidad / preferencias / entorno (vía &lt;REMEMBER&gt;).
                    </div>`);
                return;
            }
            const bar = (label: string, val: number, color: string) => {
                const pct = s.total > 0 ? Math.round((val / s.total) * 100) : 0;
                return `<div style="display:flex;align-items:center;gap:8px;margin-top:3px;font-size:11px;">
                    <span style="width:60px;color:${color};">${label}</span>
                    <div style="flex:1;height:6px;background:rgba(255,255,255,0.05);border-radius:3px;overflow:hidden;">
                        <div style="width:${pct}%;height:100%;background:${color};"></div>
                    </div>
                    <span style="font-variant-numeric:tabular-nums;width:48px;text-align:right;color:var(--txt2);">${val} (${pct}%)</span>
                </div>`;
            };
            sysMsg(`<div class="mn">⌬ Core memory decay</div>
                <div style="margin-top:4px;font-size:11px;color:var(--txt2);">
                    <b style="color:var(--txt);">${s.total}</b> entradas totales.
                    Half-life por sección: identity 365d · preference 180d · host 90d · context 60d.
                </div>
                ${bar('● Fresh',  s.fresh,  '#34d399')}
                ${bar('● Aging',  s.aging,  '#fbbf24')}
                ${bar('● Stale',  s.stale,  '#f87171')}
                ${bar('● Pinned', s.pinned, '#60a5fa')}
                <div style="margin-top:8px;font-size:10.5px;color:var(--txt3);">
                    <b>Fresh</b> (score ≥ ${s.aging_threshold.toFixed(2)}): se inyectan normales al prompt.
                    <b>Aging</b> (entre ${s.inject_threshold.toFixed(2)} y ${s.aging_threshold.toFixed(2)}): se inyectan con tag <code>[~aging~]</code> para que Lucy hedge.
                    <b>Stale</b> (&lt; ${s.inject_threshold.toFixed(2)}): se filtran del prompt (visibles en UI por si quieres reconfirmar).
                    <b>Pinned</b>: opt-out total del decay (siempre score 1.0).
                </div>`);
        } catch (e) {
            sysMsg(`memory_core_decay_stats falló: ${String(e).substring(0, 200)}`, 'var(--red)');
        }
    })();
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
                    invoke<string>('execute_powershell', { script: cmd })
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
 * v1.4.29 — Block-based output for forensic commands.
 *
 * Renders a list of collapsible sections as native <details>/<summary>
 * markup. Picked native HTML over a Svelte component because slash-cmd
 * results flow through `sysMsg(html)` → `{@html msg.html}` and adding a
 * new component-routing path would require touching the message
 * renderer in ChatThread. <details> gets us free open/close behavior,
 * accessibility, and survives transcript export (markdown + print).
 *
 * `tone` paints the left border + summary icon color:
 *   ok    → green   (no significant change, succeeded)
 *   info  → neutral (default, descriptive sections)
 *   warn  → amber   (notable but not failing)
 *   crit  → red     (something bad found)
 *
 * Use `defaultOpen: true` for the headline section the user always
 * wants to see (e.g. the executive summary). Detail sections stay
 * collapsed so the bubble doesn't overwhelm the chat thread.
 */
export type ResultBlock = {
    title: string;
    icon?: string;
    tone?: 'ok' | 'info' | 'warn' | 'crit';
    html?: string;
    defaultOpen?: boolean;
};
export function renderResultBlocks(headline: string, blocks: ResultBlock[]): string {
    const sections = blocks.map(b => {
        const tone = b.tone || 'info';
        const open = b.defaultOpen ? ' open' : '';
        return `<details class="rb-block rb-tone-${tone}"${open}>` +
            `<summary class="rb-summary">` +
            `<span class="rb-ico">${b.icon || '·'}</span>` +
            `<span class="rb-title">${b.title}</span>` +
            `<span class="rb-chev">▾</span>` +
            `</summary>` +
            `<div class="rb-body">${b.html || ''}</div>` +
        `</details>`;
    }).join('');
    return `<div class="rb-wrap"><div class="rb-hdr">${headline}</div>${sections}</div>`;
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
            // safeJsonArray: protege contra rows con JSON malformado (migración parcial,
            // edición manual del .db, etc.). Antes lanzaba SyntaxError y la card quedaba en blanco.
            const outcomes: string[] = safeJsonArray<string>(c.key_outcomes);
            const files:    string[] = safeJsonArray<string>(c.files_affected);
            const lessons:  string[] = safeJsonArray<string>(c.lessons);
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
                        const tags = safeJsonArray<string>(n.memory.tags);
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
                const concepts: string[] = safeJsonArray<string>(i.concepts);
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
            // safeJsonArray: protege contra rows con JSON malformado (migración parcial,
            // edición manual del .db, etc.). Antes lanzaba SyntaxError y la card quedaba en blanco.
            const outcomes: string[] = safeJsonArray<string>(c.key_outcomes);
            const files:    string[] = safeJsonArray<string>(c.files_affected);
            const lessons:  string[] = safeJsonArray<string>(c.lessons);
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
