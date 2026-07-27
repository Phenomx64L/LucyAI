// ── agent-quick-tools.ts — the single-shot native tool short-circuit ────────
//
// Phase 2 of the runAI() de-monolithing effort (v1.7.239), the first phase to
// move behind the $lib/agent-host port introduced in Phase 1.
//
// What this phase is
// ------------------
// When the model's reply asks for exactly ONE read-only native tool and the
// turn has no reason to keep going, runAI() answers it directly and ends the
// turn — no agent loop, no second LLM round-trip. That short-circuit is worth
// a lot of latency on the most common diagnostic prompts ("cuánta RAM tengo",
// "qué procesos hay").
//
// The caller owns the DECISION to try it; this module owns the HANDLING. The
// gate stays at the call site because it is about the surrounding turn (is the
// reply multi-step? did the user ask for multiple things? does the answer have
// to land on disk?), not about any individual tool.
//
// Every effect goes through the host, so this runs unchanged whether the caller
// is the chat UI or a headless agent.

import { isSensitiveRegistry } from './page/agent-checkpoints';
import type { AgentHost } from './agent-host';

/** The narrow slice of the host this phase needs. */
export type QuickToolHost = Pick<AgentHost, 'invoke' | 'addMsg' | 'fin' | 'speak'>;

export interface QuickToolContext {
    tabId: string | number;
    /** Voice-initiated turn — speak the sysinfo confirmation. */
    doSpeak: boolean;
    host: QuickToolHost;
}

/**
 * Handle `resp` if it is a single-shot native tool call.
 *
 * @returns `true` if the turn was handled and finished (the caller must return
 *          immediately), `false` if nothing matched and the turn should fall
 *          through to the agent loop.
 *
 * Contract notes, all preserved from the inline original:
 *  - Each branch calls `host.fin()` itself, including on the error paths — a
 *    failed tool still ENDS the turn rather than falling through to the loop.
 *  - Tool errors are reported to the user as a message, never thrown.
 *  - The sensitive-registry refusal happens BEFORE the backend call, so a
 *    blocked path is never read.
 */
export async function tryQuickNativeTool(resp: string, ctx: QuickToolContext): Promise<boolean> {
    const { tabId, doSpeak, host } = ctx;

    if (resp.includes('<TOOL>sysinfo</TOOL>')) {
        const r = await host.invoke('get_system_health');
        host.addMsg(tabId, { role: 'lucy', html: `<div class="mn">Lucy (Hardware)</div><pre>${r}</pre>`, rawRole: 'Lucy', rawContent: r });
        if (doSpeak) host.speak('Aquí tienes el reporte.');
        host.fin(tabId);
        return true;
    }

    if (resp.includes('<TOOL>netconn</TOOL>')) {
        try {
            const conns: any[] = await host.invoke('get_network_connections');
            const rows = conns.slice(0, 30).map((c: any) => `${c.protocol.padEnd(4)} ${(c.local_addr + ':' + c.local_port).padEnd(22)} ${(c.remote_addr ? c.remote_addr + ':' + c.remote_port : '').padEnd(22)} ${c.state} (PID ${c.pid ?? '-'})`).join('\n');
            host.addMsg(tabId, { role: 'lucy', html: `<div class="mn">Lucy (Red)</div><pre style="font-size:11px;">${rows || 'Sin conexiones activas.'}</pre>`, rawRole: 'Lucy', rawContent: rows });
        } catch (e) {
            host.addMsg(tabId, { role: 'lucy', html: `<div class="mn">! Red</div>${e}`, style: 'border-left-color:#ef4444;' });
        }
        host.fin(tabId);
        return true;
    }

    if (resp.includes('<TOOL>tasklist</TOOL>')) {
        try {
            const tasks: any[] = await host.invoke('get_tasklist');
            const rows = tasks.slice(0, 25).map((t: any) => `${t.name.padEnd(30)} PID:${String(t.pid).padEnd(6)} ${(t.mem_kb / 1024).toFixed(1)} MB`).join('\n');
            host.addMsg(tabId, { role: 'lucy', html: `<div class="mn">Lucy (Procesos)</div><pre style="font-size:11px;">${rows}</pre>`, rawRole: 'Lucy', rawContent: rows });
        } catch (e) {
            host.addMsg(tabId, { role: 'lucy', html: `<div class="mn">! Tasklist</div>${e}`, style: 'border-left-color:#ef4444;' });
        }
        host.fin(tabId);
        return true;
    }

    const evtM0 = resp.match(/<TOOL>eventlog:([^<:]+):(\d+)(?::([^<]+))?<\/TOOL>/i);
    if (evtM0) {
        try {
            // Count is clamped: the model routinely asks for more events than the
            // bubble can usefully render, and a huge fetch stalls the turn.
            const safeCount = Math.min(parseInt(evtM0[2]), 500);
            const events: any[] = await host.invoke('get_event_log', { logName: evtM0[1], count: safeCount, level: evtM0[3] || null });
            const rows = events.map((e: any) => `[${e.level}] ${e.time} · ${e.source} (ID ${e.event_id})\n  ${e.message}`).join('\n\n');
            host.addMsg(tabId, { role: 'lucy', html: `<div class="mn">Lucy (EventLog: ${evtM0[1]})</div><pre style="font-size:11px;">${rows || 'Sin eventos.'}</pre>`, rawRole: 'Lucy', rawContent: rows });
        } catch (e) {
            host.addMsg(tabId, { role: 'lucy', html: `<div class="mn">! EventLog</div>${e}`, style: 'border-left-color:#ef4444;' });
        }
        host.fin(tabId);
        return true;
    }

    const regM0 = resp.match(/<TOOL>registry:([^|<]+)\|([^|<]+)\|([^<]*)<\/TOOL>/i);
    if (regM0) {
        if (isSensitiveRegistry(regM0[2])) {
            host.addMsg(tabId, { role: 'lucy', html: `<div class="mn">⊗ Registro</div>Acceso denegado a ruta sensible: ${regM0[1]}\\${regM0[2]}`, style: 'border-left-color:#ef4444;' });
            host.fin(tabId);
            return true;
        }
        try {
            const val = await host.invoke('read_registry_value', { hive: regM0[1], keyPath: regM0[2], valueName: regM0[3] || '' });
            host.addMsg(tabId, { role: 'lucy', html: `<div class="mn">Lucy (Registro)</div><code style="font-family:var(--mono);font-size:12px;">${regM0[1]}\\${regM0[2]}\\${regM0[3] || '(Default)'} = ${val}</code>`, rawRole: 'Lucy', rawContent: val });
        } catch (e) {
            host.addMsg(tabId, { role: 'lucy', html: `<div class="mn">! Registro</div>${e}`, style: 'border-left-color:#ef4444;' });
        }
        host.fin(tabId);
        return true;
    }

    return false;
}
