// ── agent-tools-native.ts — native read-only tool handlers (registry) ───────
//
// runAI() de-monolith, Batch 2 (v1.7.213). The agent loop used to carry ~19
// inline `const xM = agentResp.match(...); if (xM) { … readOnlyTasks.push(…) }`
// blocks, one per native read-only tool. They were byte-for-byte uniform:
//   match → mark toolUsed → strip the tag from lucyText → push a {label, fn}
//   task whose fn invokes a backend command and returns a formatted string.
//
// This module lifts the PURE ones (those whose fn needs nothing from runAI's
// closure beyond `invoke` + the regex match) into a table. The agent loop now
// iterates NATIVE_READONLY_HANDLERS instead of hand-maintaining each block.
//
// Handlers that DO need runAI closures (system_diff/mcp_discover/fetch/
// search_web/search_runbooks via retryWithBackoff/_cachedFetch/config, obj_query
// via the tab object, graphify which writes a different sink) stay inline for a
// later batch — they need a deps object, out of scope for this conservative pass.
//
// Backend results are typed `any` on purpose: the source was untyped JS doing
// dynamic property access on Rust command payloads. Faithful = same behaviour.

import { invoke } from '@tauri-apps/api/core';

/** A deferred read-only tool call: a label for the UI + an async producer of a
 *  result string. Mirrors the inline `readOnlyTasks` shape in runAI. */
export interface ReadOnlyTask {
    label: string;
    fn: () => Promise<string>;
}

/** One native read-only tool: how to detect it, how to strip its tag, and how
 *  to build its task from the regex match. */
export interface NativeHandler {
    kind: string;
    matchRe: RegExp;
    stripRe: RegExp;
    build: (m: RegExpMatchArray) => ReadOnlyTask;
}

export const NATIVE_READONLY_HANDLERS: NativeHandler[] = [
    // F2 Frontier — temporal state diff (compare snapshots N minutes apart)
    {
        kind: 'state_diff',
        matchRe: /<TOOL>state_diff:(\d+)<\/TOOL>/i,
        stripRe: /<TOOL>state_diff:\d+<\/TOOL>/gi,
        build: (m) => {
            const lookbackMin = Math.max(1, Math.min(parseInt(m[1], 10) || 60, 60 * 24 * 14));
            return {
                label: `[◐ State Δ ${lookbackMin}min]`,
                fn: async () => {
                    try {
                        const nowSec = Math.floor(Date.now() / 1000);
                        const sinceSec = nowSec - lookbackMin * 60;
                        const list: any = await invoke('state_snapshot_list', { sinceTs: sinceSec, limit: 200 });
                        if (!list || list.length < 2) {
                            try { await invoke('state_snapshot_capture'); } catch {}
                            return `[STATE DIFF] No hay suficientes snapshots en los últimos ${lookbackMin}min. Capturé uno ahora, vuelve a preguntar en unos minutos.`;
                        }
                        const toId = list[0].id;
                        const fromId = list[list.length - 1].id;
                        const d: any = await invoke('state_snapshot_diff', { fromId, toId });
                        const summary = [
                            `[STATE DIFF — ventana ${Math.round((d.to_ts - d.from_ts) / 60)}min]`,
                            `CPU change: ${d.cpu_delta_pct.toFixed(1)}%`,
                            `RAM change: ${d.ram_delta_mb}MB`,
                        ];
                        if (d.processes_appeared?.length) {
                            summary.push(`New processes (${d.processes_appeared.length}):`);
                            for (const p of d.processes_appeared.slice(0, 12)) {
                                summary.push(`  + ${p.name} (pid ${p.pid}, ${p.mem_mb}MB)`);
                            }
                        }
                        if (d.processes_disappeared?.length) {
                            summary.push(`Vanished processes (${d.processes_disappeared.length}):`);
                            for (const p of d.processes_disappeared.slice(0, 12)) {
                                summary.push(`  - ${p.name} (pid ${p.pid})`);
                            }
                        }
                        if (d.drive_changes?.length) {
                            summary.push(`Drive changes:`);
                            for (const c of d.drive_changes) {
                                summary.push(`  ${c.mount}: ${c.from_pct.toFixed(1)}% → ${c.to_pct.toFixed(1)}% (${c.used_delta_gb >= 0 ? '+' : ''}${c.used_delta_gb}GB)`);
                            }
                        }
                        return summary.join('\n');
                    } catch (e) {
                        return `[STATE DIFF ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },

    // F1 Frontier — process lineage search (by name substring)
    {
        kind: 'process_lineage',
        matchRe: /<TOOL>process_lineage:([^<]+)<\/TOOL>/i,
        stripRe: /<TOOL>process_lineage:[^<]+<\/TOOL>/gi,
        build: (m) => {
            const query = m[1].trim().slice(0, 80);
            return {
                label: `[⚯ Lineage] ${query}`,
                fn: async () => {
                    try {
                        const rows: any = await invoke('process_lineage_search', { nameSubstring: query, limit: 25 });
                        if (!rows || rows.length === 0) return `[LINEAGE] No matches for "${query}"`;
                        const out = [`[LINEAGE for "${query}" — ${rows.length} results]`];
                        for (const r of rows.slice(0, 15)) {
                            const chain = JSON.parse(r.parent_chain || '[]').map((n: any) => `${n.name}(${n.pid})`).join(' ← ');
                            const when = new Date(r.first_seen * 1000).toLocaleString();
                            const alive = r.vanished_at ? `vanished ${new Date(r.vanished_at * 1000).toLocaleTimeString()}` : 'alive';
                            out.push(`pid=${r.pid} ${alive} · first seen ${when}`);
                            out.push(`  exe: ${r.exe_path}`);
                            out.push(`  chain: ${chain || '(orphan)'}`);
                        }
                        return out.join('\n');
                    } catch (e) {
                        return `[LINEAGE ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },

    // F1 Frontier — process ancestry for a specific pid
    {
        kind: 'process_ancestry',
        matchRe: /<TOOL>process_ancestry:(\d+)<\/TOOL>/i,
        stripRe: /<TOOL>process_ancestry:\d+<\/TOOL>/gi,
        build: (m) => {
            const pid = parseInt(m[1], 10);
            return {
                label: `[⚯ Ancestry pid ${pid}]`,
                fn: async () => {
                    try {
                        const r: any = await invoke('process_lineage_for_pid', { pid });
                        if (!r) return `[ANCESTRY] No lineage record for pid ${pid} (may be too short-lived or already evicted).`;
                        const chain = JSON.parse(r.parent_chain || '[]').map((n: any) => `${n.name}(${n.pid})`).join(' ← ');
                        return [
                            `[ANCESTRY pid=${pid}]`,
                            `exe: ${r.exe_path}`,
                            `cmdline: ${r.cmdline}`,
                            `first seen: ${new Date(r.first_seen * 1000).toLocaleString()}`,
                            `chain: ${chain || '(orphan)'}`,
                        ].join('\n');
                    } catch (e) {
                        return `[ANCESTRY ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },

    // F3 Frontier — causal inference: "why did the spike happen?"
    {
        kind: 'diagnose_spike',
        matchRe: /<TOOL>diagnose_spike(?::(\d+))?<\/TOOL>/i,
        stripRe: /<TOOL>diagnose_spike(?::\d+)?<\/TOOL>/gi,
        build: (m) => {
            const window = m[1] ? parseInt(m[1], 10) : 120;
            return {
                label: `[△ Diagnose ${window}s window]`,
                fn: async () => {
                    try {
                        const r: any = await invoke('diagnose_spike', { symptomTs: null, windowSec: window });
                        const out = [`[CAUSAL ANALYSIS — window ${r.window_sec}s around symptom]`];
                        out.push(r.metric_context);
                        if (r.state_diff_summary) out.push(r.state_diff_summary);
                        out.push('');
                        if (!r.candidates || r.candidates.length === 0) {
                            out.push('No candidate processes found in the suspect window.');
                            out.push('Try a wider window (e.g. <TOOL>diagnose_spike:600</TOOL>) or run state_diff first.');
                        } else {
                            out.push(`Top ${r.candidates.length} suspects (ranked by confidence):`);
                            for (let i = 0; i < r.candidates.length; i++) {
                                const c = r.candidates[i];
                                const conf = (c.confidence * 100).toFixed(0);
                                const offset = c.temporal_offset_sec >= 0
                                    ? `+${c.temporal_offset_sec}s after`
                                    : `${Math.abs(c.temporal_offset_sec)}s before`;
                                out.push(`${i + 1}. ${c.name} (pid ${c.pid}) — confidence ${conf}%, ${offset} symptom`);
                                out.push(`   exe: ${c.exe_path}`);
                                if (c.parent_chain_summary) out.push(`   chain: ${c.parent_chain_summary}`);
                                for (const reason of c.reasoning) out.push(`   • ${reason}`);
                            }
                        }
                        return out.join('\n');
                    } catch (e) {
                        return `[DIAGNOSE ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },

    // F8 Frontier — behavioral threat scan
    {
        kind: 'threat_scan',
        matchRe: /<TOOL>threat_scan(?::(\d+))?<\/TOOL>/i,
        stripRe: /<TOOL>threat_scan(?::\d+)?<\/TOOL>/gi,
        build: (m) => {
            const sinceMin = m[1] ? parseInt(m[1], 10) : 60;
            return {
                label: `[⚠ Threat scan ${sinceMin}min]`,
                fn: async () => {
                    try {
                        const r: any = await invoke('threat_scan', { sinceMin, minScore: 0.30, limit: 20 });
                        const out = [`[THREAT SCAN — last ${sinceMin}min · scanned ${r.total_scanned} processes]`];
                        out.push(`Alerts: ${r.alerts} · Reviews: ${r.reviews} · Benign: ${r.benign}`);
                        out.push(`Baseline window: ${r.baseline_days} days`);
                        if (!r.candidates || r.candidates.length === 0) {
                            out.push('No suspicious activity detected.');
                        } else {
                            for (const c of r.candidates) {
                                const pct = (c.score * 100).toFixed(0);
                                const stamp = new Date(c.first_seen * 1000).toLocaleTimeString();
                                out.push('');
                                out.push(`[${c.band.toUpperCase()}] ${c.name} (pid ${c.pid}) — score ${pct}% · started ${stamp}`);
                                if (c.exe_path) out.push(`  exe: ${c.exe_path}`);
                                if (c.parent_chain) out.push(`  chain: ${c.parent_chain}`);
                                if (c.cmdline_preview && c.cmdline_preview.length > 0) {
                                    out.push(`  cmdline: ${c.cmdline_preview}`);
                                }
                                for (const reason of c.reasons) out.push(`  • ${reason}`);
                            }
                        }
                        return out.join('\n');
                    } catch (e) {
                        return `[THREAT SCAN ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },

    // F7 Frontier — runbook generator from observed history
    {
        kind: 'runbook_scan',
        matchRe: /<TOOL>runbook_scan(?::(\d+))?<\/TOOL>/i,
        stripRe: /<TOOL>runbook_scan(?::\d+)?<\/TOOL>/gi,
        build: (m) => {
            const days = m[1] ? parseInt(m[1], 10) : 30;
            return {
                label: `[⌖ Runbook scan ${days}d]`,
                fn: async () => {
                    try {
                        const r: any = await invoke('runbook_scan', { days, topK: 8 });
                        const out = [`[RUNBOOK SCAN — ${r.days_analyzed}d · ${r.total_sessions} sessions · ${r.total_commands} commands]`];
                        if (!r.candidates || r.candidates.length === 0) {
                            out.push('No repeated workflows detected yet. Need at least 3 sessions with the same 3-step sequence.');
                        } else {
                            out.push(`Detected ${r.candidates.length} candidate workflow(s):`);
                            for (const c of r.candidates) {
                                const pct = (c.confidence * 100).toFixed(0);
                                out.push('');
                                out.push(`• "${c.suggested_name}" — confidence ${pct}%, repeated ${c.frequency}×`);
                                out.push(`  sequence: ${c.sequence.join(' → ')}`);
                                const firstSample = new Date(c.sample_times[0] * 1000).toLocaleDateString();
                                const lastSample = new Date(c.sample_times[c.sample_times.length - 1] * 1000).toLocaleDateString();
                                out.push(`  spans: ${firstSample} → ${lastSample}`);
                            }
                            out.push('');
                            out.push('Propose to the user: convert any high-confidence candidate into a saved skill / slash command.');
                        }
                        return out.join('\n');
                    } catch (e) {
                        return `[RUNBOOK ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },

    // F10 Frontier — daily routine patterns
    {
        kind: 'daily_patterns',
        matchRe: /<TOOL>daily_patterns(?::(\d+))?<\/TOOL>/i,
        stripRe: /<TOOL>daily_patterns(?::\d+)?<\/TOOL>/gi,
        build: (m) => {
            const days = m[1] ? parseInt(m[1], 10) : 28;
            return {
                label: `[⌚ Daily patterns ${days}d]`,
                fn: async () => {
                    try {
                        const r: any = await invoke('daily_patterns_scan', { days, minConfidence: 0.5 });
                        const out = [`[DAILY PATTERNS — ${r.days_analyzed}d · ${r.weeks_covered} weeks]`];
                        if (!r.patterns || r.patterns.length === 0) {
                            out.push('No stable weekly routines detected yet. Need ≥2 weeks of activity at the same time.');
                        } else {
                            const byDay: any = {};
                            for (const p of r.patterns) {
                                const k = p.weekday_label;
                                (byDay[k] = byDay[k] || []).push(p);
                            }
                            for (const day of ['Lun', 'Mar', 'Mié', 'Jue', 'Vie', 'Sáb', 'Dom']) {
                                if (!byDay[day]) continue;
                                out.push(`${day}:`);
                                for (const p of byDay[day].slice(0, 6)) {
                                    const conf = (p.confidence * 100).toFixed(0);
                                    out.push(`  ${p.hour_band}h · ${p.kind === 'process' ? '⊞' : '⌨'} ${p.signal} — ${p.weeks_observed} weeks (${conf}%)`);
                                }
                            }
                        }
                        return out.join('\n');
                    } catch (e) {
                        return `[DAILY PATTERNS ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },

    // F5 Frontier — sandbox preview of a destructive command
    {
        kind: 'sandbox_preview',
        matchRe: /<TOOL>sandbox_preview:([^<]+)<\/TOOL>/i,
        stripRe: /<TOOL>sandbox_preview:[^<]+<\/TOOL>/gi,
        build: (m) => {
            const cmd = m[1].trim();
            return {
                label: `[⊠ Sandbox preview]`,
                fn: async () => {
                    try {
                        const r: any = await invoke('sandbox_preview_command', { command: cmd });
                        const pct = (r.risk_score * 100).toFixed(0);
                        const out = [`[SANDBOX PREVIEW — risk ${pct}% (${r.risk_band})]`];
                        out.push(`Command: ${r.command}`);
                        if (r.destructive_reason) out.push(`Destructive: ${r.destructive_reason}`);
                        if (r.elevation_required) out.push('Requires elevation (admin/sudo)');
                        if (r.affected_paths?.length) out.push(`Paths touched: ${r.affected_paths.slice(0, 6).join(', ')}`);
                        if (r.affected_registry_keys?.length) out.push(`Registry keys: ${r.affected_registry_keys.join(', ')}`);
                        if (r.affected_services?.length) out.push(`Services: ${r.affected_services.join(' | ')}`);
                        if (r.network_endpoints?.length) out.push(`Network: ${r.network_endpoints.join(', ')}`);
                        if (r.static_findings?.length) {
                            out.push('Findings:');
                            for (const f of r.static_findings) out.push(`  • ${f}`);
                        }
                        if (r.sandbox_available && r.sandbox_wsb_path) {
                            out.push('');
                            out.push(`Windows Sandbox config ready: ${r.sandbox_wsb_path}`);
                            out.push('User can double-click that .wsb file to run the command in isolation.');
                        } else if (!r.sandbox_available && r.destructive) {
                            out.push('');
                            out.push('Windows Sandbox not available on this host — static preview only.');
                        }
                        return out.join('\n');
                    } catch (e) {
                        return `[SANDBOX PREVIEW ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },

    // F9 Frontier — knowledge graph: recent files in a directory
    {
        kind: 'kg_recent',
        matchRe: /<TOOL>kg_recent(?::([^<]+))?<\/TOOL>/i,
        stripRe: /<TOOL>kg_recent(?::[^<]+)?<\/TOOL>/gi,
        build: (m) => {
            const dirPrefix = m[1] ? m[1].trim() : null;
            return {
                label: `[◍ Recent files] ${dirPrefix || '(all)'}`,
                fn: async () => {
                    try {
                        const rows: any = await invoke('kg_recent_files', { dirPrefix, limit: 30 });
                        if (!rows || rows.length === 0) {
                            return `[KG RECENT] No indexed files yet${dirPrefix ? ' under "' + dirPrefix + '"' : ''}. Add a root with kg_add_root and run kg_index_now first.`;
                        }
                        const out = [`[KG RECENT FILES — ${rows.length} entries]`];
                        for (const f of rows.slice(0, 20)) {
                            const when = new Date(f.last_mtime * 1000).toLocaleString();
                            out.push(`  ${when} · ${f.path} (.${f.extension}, ${f.touch_count}×)`);
                        }
                        return out.join('\n');
                    } catch (e) {
                        return `[KG RECENT ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },

    // F9 Frontier — knowledge graph: co-touched neighbors of a file
    {
        kind: 'kg_neighbors',
        matchRe: /<TOOL>kg_neighbors:([^<]+)<\/TOOL>/i,
        stripRe: /<TOOL>kg_neighbors:[^<]+<\/TOOL>/gi,
        build: (m) => {
            const path = m[1].trim();
            return {
                label: `[⛓ Neighbors] ${path.split(/[\\/]/).pop()}`,
                fn: async () => {
                    try {
                        const rows: any = await invoke('kg_neighbors', { path, topK: 12 });
                        if (!rows || rows.length === 0) {
                            return `[KG NEIGHBORS] No co-touched edges recorded for "${path}". Either it's never been modified alongside another file, or it hasn't been indexed yet.`;
                        }
                        const out = [`[KG NEIGHBORS of ${path}]`];
                        for (const n of rows) {
                            const when = n.last_mtime ? new Date(n.last_mtime * 1000).toLocaleString() : '?';
                            out.push(`  weight=${n.weight} · ${n.path}  (.${n.extension}, ${when})`);
                        }
                        return out.join('\n');
                    } catch (e) {
                        return `[KG NEIGHBORS ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },

    // F9 Frontier — knowledge graph: extension breakdown
    {
        kind: 'kg_ext_summary',
        matchRe: /<TOOL>kg_ext_summary(?::([^<]+))?<\/TOOL>/i,
        stripRe: /<TOOL>kg_ext_summary(?::[^<]+)?<\/TOOL>/gi,
        build: (m) => {
            const dirPrefix = m[1] ? m[1].trim() : null;
            return {
                label: `[◐ Ext breakdown] ${dirPrefix || '(all)'}`,
                fn: async () => {
                    try {
                        const rows: any = await invoke('kg_ext_summary', { dirPrefix });
                        if (!rows || rows.length === 0) return `[KG EXT] No indexed files yet.`;
                        const out = [`[KG EXTENSION BREAKDOWN]`];
                        for (const r of rows) {
                            const ext = r.extension || '(no ext)';
                            out.push(`  .${ext} · ${r.file_count} files · ${r.total_touches} touches`);
                        }
                        return out.join('\n');
                    } catch (e) {
                        return `[KG EXT ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },

    // Cross-feature — incident detective (F3+F8+F9 synthesis)
    {
        kind: 'incident_detective',
        matchRe: /<TOOL>incident_detective(?::(\d+))?<\/TOOL>/i,
        stripRe: /<TOOL>incident_detective(?::\d+)?<\/TOOL>/gi,
        build: (m) => {
            const windowSec = m[1] ? parseInt(m[1], 10) : 300;
            return {
                label: `[🔎 Detective ${windowSec}s]`,
                fn: async () => {
                    try {
                        const r: any = await invoke('incident_detective', { symptomTs: null, windowSec });
                        const pct = (r.confidence * 100).toFixed(0);
                        const out = [`[INCIDENT DETECTIVE — window ±${r.window_sec}s · overall confidence ${pct}%]`];
                        out.push('');
                        out.push(`Narrative: ${r.narrative}`);
                        out.push('');
                        if (r.threats?.length) {
                            out.push(`Behavioral threats (${r.threats.length}):`);
                            for (const t of r.threats.slice(0, 5)) {
                                const tp = (t.score * 100).toFixed(0);
                                out.push(`  [${t.band}] ${t.name} (pid ${t.pid}) — ${tp}%`);
                                for (const reason of t.reasons.slice(0, 2)) out.push(`    • ${reason}`);
                            }
                            out.push('');
                        }
                        if (r.causal?.candidates?.length) {
                            out.push(`Top causal candidates:`);
                            for (const c of r.causal.candidates.slice(0, 4)) {
                                const cp = (c.confidence * 100).toFixed(0);
                                const off = c.temporal_offset_sec >= 0 ? `+${c.temporal_offset_sec}s` : `${c.temporal_offset_sec}s`;
                                out.push(`  ${c.name} (pid ${c.pid}) · ${cp}% · ${off} from symptom`);
                            }
                            out.push('');
                        }
                        out.push(`File activity: ${r.touched_cluster_summary}`);
                        if (r.file_changes?.length) {
                            for (const f of r.file_changes.slice(0, 6)) {
                                const when = new Date(f.last_mtime * 1000).toLocaleTimeString();
                                out.push(`  ${when} · ${f.path}`);
                            }
                        }
                        return out.join('\n');
                    } catch (e) {
                        return `[DETECTIVE ERROR] ${String(e)}`;
                    }
                },
            };
        },
    },
];
