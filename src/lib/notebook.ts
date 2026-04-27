// ── notebook — export/import a chat tab as a `.lucynote` file ────────────────
//
// Why exist:
//   A successful incident-response chat is a *runbook* in disguise. After the
//   storm, it gets lost in tab history. This module turns a tab into a
//   replayable, shareable artifact:
//
//   • Markdown comments describe the *why*
//   • Tool invocations + outputs are preserved verbatim
//   • Re-runnable: a future session can load it and re-execute commands
//     (with the user re-confirming each step) against any host.
//
// Format: JSON envelope, version-stamped, human-readable.
//
//   {
//     "kind": "lucynote",
//     "version": 1,
//     "title": "AD failover root cause - 2026-04-26",
//     "created_at": 1714180000000,
//     "lucy_version": "1.2.0",
//     "lang": "es-MX",
//     "cells": [
//       { "type": "user",     "text": "..." },
//       { "type": "thought",  "text": "..." },
//       { "type": "command",  "engine": "powershell", "cmd": "...", "output": "..." },
//       { "type": "lucy",     "text": "..." }
//     ]
//   }
//
// Kept deliberately small: no schema, no tooling — the format IS the spec.

export type NotebookCellType = 'user' | 'lucy' | 'thought' | 'command' | 'tool';

export interface NotebookCell {
    type: NotebookCellType;
    text?: string;
    /** For 'command' cells: the shell engine (powershell, cmd, ssh, etc) */
    engine?: string;
    /** For 'command' cells: the command string */
    cmd?: string;
    /** For 'command' / 'tool' cells: the captured output */
    output?: string;
    /** Wall-clock timestamp (ms) when the cell was produced. */
    ts?: number;
}

export interface Notebook {
    kind: 'lucynote';
    version: 1;
    title: string;
    created_at: number;
    lucy_version: string;
    lang: string;
    /** Optional: the model that produced the original session, for cost/recall context. */
    model?: string;
    cells: NotebookCell[];
}

const NOTEBOOK_VERSION = 1 as const;

/**
 * Strip HTML/markdown chrome from a chat message back to plain text we
 * can safely round-trip. Keeps fenced code blocks intact.
 */
function _toPlainText(raw: string): string {
    if (!raw) return '';
    // Remove <THOUGHT>...</THOUGHT> wrappers — they show up as separate cells.
    let s = raw.replace(/<THOUGHT>[\s\S]*?<\/THOUGHT>/gi, '').trim();
    // Strip toolbar-style HTML our renderer adds (e.g. badges, mn divs)
    s = s.replace(/<div class="mn">[\s\S]*?<\/div>/gi, '').trim();
    return s;
}

/**
 * Build a Notebook from a tab's message log + metadata.
 *
 * @param tab        the active Tab object (must have .messages, .selectedModel)
 * @param meta       extra context: { lang, lucyVersion, title? }
 *
 * Cell selection rules:
 *   • role 'user'         → user cell
 *   • role 'lucy'         → lucy cell (final answer)
 *   • THOUGHT blocks      → thought cell (extracted, not duplicated)
 *   • EXECUTE_CMD results → command cell with output
 *   • Tool cards          → tool cell
 */
export function buildNotebook(
    tab: { messages?: any[]; selectedModel?: string; title?: string },
    meta: { lang?: string; lucyVersion?: string; title?: string }
): Notebook {
    const cells: NotebookCell[] = [];
    const msgs = Array.isArray(tab.messages) ? tab.messages : [];

    for (const m of msgs) {
        if (!m || !m.role) continue;
        const baseTs = typeof m.ts === 'number' ? m.ts : undefined;
        const raw    = String(m.rawContent ?? m.text ?? '');

        if (m.role === 'user') {
            cells.push({ type: 'user', text: _toPlainText(raw), ts: baseTs });
            continue;
        }
        if (m.role === 'system') {
            // Skip system noise (tutorial banners etc); user can re-add manually.
            continue;
        }

        // Pull THOUGHT first so it appears before the answer body.
        const tm = raw.match(/<THOUGHT>([\s\S]*?)<\/THOUGHT>/i);
        if (tm && tm[1].trim()) {
            cells.push({ type: 'thought', text: tm[1].trim(), ts: baseTs });
        }

        // Detect a command echo: <EXECUTE_CMD engine="...">...</EXECUTE_CMD>
        const cm = raw.match(/<EXECUTE_CMD\s+engine\s*=\s*"([^"]+)"\s*>([\s\S]*?)<\/EXECUTE_CMD>/i);
        if (cm) {
            cells.push({
                type: 'command',
                engine: cm[1].trim(),
                cmd:    cm[2].trim(),
                output: m.rawOutput ? String(m.rawOutput) : undefined,
                ts: baseTs,
            });
            continue;
        }

        // Tool card output (m.cards is sometimes set by the agent loop)
        if (Array.isArray(m.cards) && m.cards.length > 0) {
            for (const c of m.cards) {
                cells.push({
                    type: 'tool',
                    text: c.label || c.title,
                    output: c.output,
                    ts: baseTs,
                });
            }
        }

        const plain = _toPlainText(raw);
        if (plain) cells.push({ type: 'lucy', text: plain, ts: baseTs });
    }

    return {
        kind: 'lucynote',
        version: NOTEBOOK_VERSION,
        title: meta.title || tab.title || 'Untitled session',
        created_at: Date.now(),
        lucy_version: meta.lucyVersion || '0.0.0',
        lang: meta.lang || 'es-MX',
        model: tab.selectedModel,
        cells,
    };
}

/** Pretty-print a notebook to a markdown string for human reading. */
export function notebookToMarkdown(nb: Notebook): string {
    const lines: string[] = [];
    lines.push(`# ${nb.title}`);
    lines.push('');
    lines.push(`*Created ${new Date(nb.created_at).toISOString()} · Lucy ${nb.lucy_version} · model: ${nb.model || 'unknown'}*`);
    lines.push('');
    for (const c of nb.cells) {
        switch (c.type) {
            case 'user':
                lines.push(`### 👤 User`);
                lines.push(c.text || '');
                lines.push('');
                break;
            case 'thought':
                lines.push(`> 💭 ${c.text || ''}`);
                lines.push('');
                break;
            case 'lucy':
                lines.push(`### 🤖 Lucy`);
                lines.push(c.text || '');
                lines.push('');
                break;
            case 'command':
                lines.push(`#### ▶ \`${c.engine || 'shell'}\``);
                lines.push('```' + (c.engine === 'powershell' ? 'powershell' : 'bash'));
                lines.push(c.cmd || '');
                lines.push('```');
                if (c.output) {
                    lines.push('');
                    lines.push('**Output:**');
                    lines.push('```');
                    lines.push(c.output.slice(0, 4000));
                    lines.push('```');
                }
                lines.push('');
                break;
            case 'tool':
                lines.push(`*🔧 ${c.text || 'tool'}*`);
                if (c.output) {
                    lines.push('```');
                    lines.push(c.output.slice(0, 1000));
                    lines.push('```');
                }
                lines.push('');
                break;
        }
    }
    return lines.join('\n');
}

/**
 * Validate an arbitrary parsed JSON object as a Notebook.
 * Throws with a precise reason on failure — used by the import flow.
 */
export function parseNotebook(input: unknown): Notebook {
    if (!input || typeof input !== 'object') throw new Error('Not a JSON object');
    const o = input as Record<string, unknown>;
    if (o.kind !== 'lucynote') throw new Error('Missing/invalid `kind`: expected "lucynote"');
    if (typeof o.version !== 'number' || o.version > NOTEBOOK_VERSION) {
        throw new Error(`Unsupported notebook version: ${o.version}`);
    }
    if (!Array.isArray(o.cells)) throw new Error('Missing `cells` array');
    return o as unknown as Notebook;
}

/**
 * Trigger a download of the notebook as a `.lucynote` file in the browser.
 * In Tauri, the user sees the standard save dialog via the anchor click.
 */
export function downloadNotebook(nb: Notebook): void {
    const json = JSON.stringify(nb, null, 2);
    const blob = new Blob([json], { type: 'application/json' });
    const url  = URL.createObjectURL(blob);
    const a    = document.createElement('a');
    a.href     = url;
    const safeTitle = nb.title.replace(/[^a-z0-9-_]+/gi, '_').slice(0, 60);
    a.download = `${safeTitle || 'lucy_session'}.lucynote`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** Same but for the markdown export. */
export function downloadNotebookMarkdown(nb: Notebook): void {
    const md   = notebookToMarkdown(nb);
    const blob = new Blob([md], { type: 'text/markdown' });
    const url  = URL.createObjectURL(blob);
    const a    = document.createElement('a');
    a.href     = url;
    const safeTitle = nb.title.replace(/[^a-z0-9-_]+/gi, '_').slice(0, 60);
    a.download = `${safeTitle || 'lucy_session'}.md`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(() => URL.revokeObjectURL(url), 1000);
}
