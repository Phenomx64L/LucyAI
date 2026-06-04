// ── scripts/build-manual.cjs — v1.7.66 ─────────────────────────────────────
//
// Generates Lucy_Assistant_Manual_v1.7.66.docx onto the user's Desktop using
// the globally-installed docx-js library. Run with:
//
//   node scripts/build-manual.cjs
//
// The script doesn't depend on the project's node_modules — it pulls docx
// from `npm root -g`. That keeps the project's lock files clean.

const path = require('path');
const fs   = require('fs');
const os   = require('os');
const { execSync } = require('child_process');

// Locate the globally-installed docx package.
const globalRoot = execSync('npm root -g', { encoding: 'utf8' }).trim();
const docxPath   = path.join(globalRoot, 'docx');
if (!fs.existsSync(docxPath)) {
    console.error('docx not found in global node_modules. Install: npm install -g docx');
    process.exit(1);
}
const {
    Document, Packer, Paragraph, TextRun, Table, TableRow, TableCell,
    Header, Footer, AlignmentType, LevelFormat,
    TabStopType, TabStopPosition,
    HeadingLevel, BorderStyle, WidthType, ShadingType,
    PageNumber,
} = require(docxPath);

// ── Style + colour palette (matches Lucy's accent green) ───────────────────
const ACCENT = '10B981';       // primary green
const ACCENT_DARK = '0F7B5A';  // header underline tone
const SLATE_500 = '64748B';    // body subtle text
const SLATE_700 = '334155';    // body strong text
const AMBER = 'F59E0B';
const VIOLET = 'A78BFA';
const BLUE = '60A5FA';
const RED = 'EF4444';

const PAGE_WIDTH  = 12240;  // US Letter
const PAGE_HEIGHT = 15840;
const MARGIN      = 1440;    // 1 inch
const CONTENT_W   = PAGE_WIDTH - 2 * MARGIN;

// Border helpers
const BR = { style: BorderStyle.SINGLE, size: 1, color: 'D9D9D9' };
const cellBorders = { top: BR, bottom: BR, left: BR, right: BR };

// ── Helper builders ────────────────────────────────────────────────────────

function p(text, opts = {}) {
    const runs = Array.isArray(text)
        ? text.map(r => r instanceof TextRun ? r : new TextRun(r))
        : [new TextRun({ text: text, font: opts.mono ? 'Consolas' : 'Arial' })];
    return new Paragraph({
        children: runs,
        spacing: { before: opts.before ?? 60, after: opts.after ?? 60 },
        ...(opts.heading ? { heading: opts.heading } : {}),
        ...(opts.align ? { alignment: opts.align } : {}),
    });
}

function bullet(text) {
    return new Paragraph({
        numbering: { reference: 'bullets', level: 0 },
        children: text instanceof TextRun ? [text] : [new TextRun({ text, font: 'Arial' })],
    });
}

function bold(text, opts = {}) {
    return new TextRun({ text, bold: true, font: opts.mono ? 'Consolas' : 'Arial', ...opts });
}

function mono(text) {
    return new TextRun({ text, font: 'Consolas', size: 20 });
}

function h(text, level) {
    return new Paragraph({
        heading: level,
        children: [new TextRun({ text, font: 'Arial', bold: true, color: level === HeadingLevel.HEADING_1 ? ACCENT_DARK : '1a1a1a' })],
        spacing: { before: level === HeadingLevel.HEADING_1 ? 360 : 240, after: level === HeadingLevel.HEADING_1 ? 180 : 120 },
    });
}

function rule() {
    return new Paragraph({
        children: [],
        border: { bottom: { style: BorderStyle.SINGLE, size: 6, color: ACCENT, space: 1 } },
        spacing: { before: 120, after: 120 },
    });
}

function table(headers, rows, colWidths) {
    const totalW = colWidths.reduce((a, b) => a + b, 0);
    const headerCells = headers.map((text, i) =>
        new TableCell({
            borders: cellBorders,
            width: { size: colWidths[i], type: WidthType.DXA },
            shading: { fill: ACCENT_DARK, type: ShadingType.CLEAR, color: 'auto' },
            margins: { top: 80, bottom: 80, left: 120, right: 120 },
            children: [new Paragraph({
                children: [new TextRun({ text, bold: true, color: 'FFFFFF', font: 'Arial', size: 20 })],
            })],
        })
    );
    const dataRows = rows.map(row => new TableRow({
        children: row.map((text, i) => new TableCell({
            borders: cellBorders,
            width: { size: colWidths[i], type: WidthType.DXA },
            margins: { top: 60, bottom: 60, left: 120, right: 120 },
            children: [new Paragraph({
                children: [new TextRun({ text: String(text), font: 'Arial', size: 20 })],
            })],
        })),
    }));
    return new Table({
        width: { size: totalW, type: WidthType.DXA },
        columnWidths: colWidths,
        rows: [new TableRow({ children: headerCells }), ...dataRows],
    });
}

// ── Document content ──────────────────────────────────────────────────────

const children = [];

// Cover page
children.push(new Paragraph({
    alignment: AlignmentType.CENTER,
    spacing: { before: 2400, after: 240 },
    children: [new TextRun({ text: 'Lucy Assistant', bold: true, font: 'Arial', size: 56, color: ACCENT_DARK })],
}));
children.push(new Paragraph({
    alignment: AlignmentType.CENTER,
    spacing: { before: 0, after: 240 },
    children: [new TextRun({ text: 'Operations Console for SysAdmin & DevOps', italics: true, font: 'Arial', size: 28, color: SLATE_700 })],
}));
children.push(new Paragraph({
    alignment: AlignmentType.CENTER,
    spacing: { before: 480, after: 0 },
    children: [new TextRun({ text: 'User Manual', font: 'Arial', size: 32, color: SLATE_500 })],
}));
children.push(new Paragraph({
    alignment: AlignmentType.CENTER,
    spacing: { before: 120, after: 1200 },
    children: [new TextRun({ text: 'Version 1.7.66 — June 2026', font: 'Consolas', size: 22, color: SLATE_500 })],
}));
children.push(new Paragraph({
    alignment: AlignmentType.CENTER,
    spacing: { before: 0, after: 60 },
    children: [new TextRun({ text: 'Built on Tauri 2 + SvelteKit + Rust', font: 'Arial', size: 20, color: SLATE_500 })],
}));
children.push(new Paragraph({
    alignment: AlignmentType.CENTER,
    spacing: { before: 0, after: 0 },
    children: [new TextRun({ text: 'Author: Iván Eduardo Luna (@Phenomx64L) · GPLv3', font: 'Arial', size: 18, color: SLATE_500 })],
}));
children.push(new Paragraph({ children: [], pageBreakBefore: true }));

// ── 1. What's new in v1.7 ───────────────────────────────────────────────
children.push(h('1. What\'s New in v1.7 — Operations Console Era', HeadingLevel.HEADING_1));
children.push(p('A 15-version arc (v1.6.0 → v1.7.66) transformed Lucy from an AI chat assistant with sysadmin tools into a full operations console with its own visual identity. Below are the headline shifts. The turn-by-turn detail lives in CHANGELOG.md.'));

children.push(h('1.1 Operations Console UI', HeadingLevel.HEADING_2));
children.push(p('The chrome above and below the chat thread now signals "this is an operator\'s console", not a generic copilot.'));

children.push(table(
    ['Surface', 'Version', 'What it adds'],
    [
        ['Mission Strip',          'v1.7.58', 'Always-on band: local host heartbeat, remote hosts, alerts, guard skill, clock, 5-dot posture'],
        ['Per-tab purpose tint',   'v1.7.59', 'Tab top-border colours by category: incident red, executing violet, investigation amber, reference blue'],
        ['Terminal-recording blocks', 'v1.7.60', 'Code blocks read as forensic recordings: traffic lights, hostname chip, engine glyph, timestamp, exit code'],
        ['Sidebar category rails', 'v1.7.63', '2-px left rail per section: Sistema green, Runbooks amber, Acciones violet, Registros blue'],
        ['Evidence pills',         'v1.7.63', 'Inline citations colour-coded by kind: memory cyan, file green, URL blue, tool amber'],
        ['Composer ops aesthetic', 'v1.7.63', 'Lambda prompt, dot grid on focus, amber slash-command mode, block-shape caret'],
        ['Self-diagnostic repair', 'v1.7.64-66', 'One-click repairs for common DB / log / credential issues'],
    ],
    [2200, 1200, CONTENT_W - 2200 - 1200],
));

children.push(h('1.2 Intelligence', HeadingLevel.HEADING_2));
children.push(bullet('Grounding (v1.6.0) — every memory carries a confidence score driven by evidence; contradicting evidence downgrades, reinforcement raises it.'));
children.push(bullet('Curated skill presets (v1.6.1) — 18+ ready-to-use ECC presets (cost-aware, security-review, hypothesis-driven-debug, etc.) plus an auto-loader for docs/security-skills/.'));
children.push(bullet('Polarity classification (v1.6.5-7) — auto-suggested follow-up chips are scored by polarity so destructive suggestions stop sneaking in.'));
children.push(bullet('Annealing ontology scoring (v1.6.6-8) — memory-consolidation picks are stable across runs via simulated annealing.'));
children.push(bullet('Centralised model catalog + tier health (v1.7.0-5) — single source of truth for every supported model; boot-time health check.'));
children.push(bullet('Multi-intent + RULE 0b (v1.7.49-50) — prompts that ask for a "report" with a file destination always become a multi-step plan with real writefile.'));

children.push(h('1.3 Streaming overhaul', HeadingLevel.HEADING_2));
children.push(bullet('morphdom DOM diffing (v1.7.56) — only the changed nodes are touched; the rest of the bubble stays physically intact. Eliminates the residual streaming shimmer.'));
children.push(bullet('rAF throttle + CSS-owned cursor (v1.7.45) — multiple drain ticks coalesce into one paint; the blinking cursor lives in CSS so it never gets destroyed on every chunk.'));
children.push(bullet('Open-tag placeholder + role-gated fin() (v1.7.46-54) — when Lucy emits <THOUGHT> before any prose, the UI shows "◌ Lucy está razonando…" instead of going blank.'));
children.push(bullet('Gemini-style aura (v1.7.57) — streaming bubbles glow with a soft accent text-shadow; each new element fades in over 280 ms with a slight blur lift.'));

children.push(h('1.4 Performance', HeadingLevel.HEADING_2));
children.push(bullet('GPU vendor hints (v1.7.42) — NvOptimusEnablement = 1 and AmdPowerXpressRequestHighPerformance = 1 exported as static symbols so hybrid laptops bind Lucy to the discrete GPU.'));
children.push(bullet('WebView2 GPU flags (v1.7.42) — --enable-gpu-rasterization --enable-zero-copy --ignore-gpu-blocklist requested at process launch.'));
children.push(bullet('Single window effect (v1.7.42) — Mica only, no acrylic, so DWM stops running two blur passes per frame on the same surface.'));
children.push(bullet('Idle saver (v1.7.44) — html.lucy-quiescent after 8 s of no input pauses every infinite CSS animation app-wide. Idle GPU drops to ~1-3%.'));

children.push(h('1.5 Reliability', HeadingLevel.HEADING_2));
children.push(bullet('persistirNow (v1.7.51) — bypasses the 500 ms debounce for structural changes (close tab, rename, clear). Closing Lucy right after an edit never loses it.'));
children.push(bullet('Self-diagnostic data repair (v1.7.64-65) — one-click command backfills NULL confidence values across three tables, force-rewrites every row to clear stale storage state, REINDEXes, and verifies.'));

children.push(new Paragraph({ children: [], pageBreakBefore: true }));

// ── 2. Mission Strip reference ───────────────────────────────────────────
children.push(h('2. Mission Strip — Always-on Operational Pulse', HeadingLevel.HEADING_1));
children.push(p('The thin band between the title bar and the tab strip communicates the four signals an IT pro tracks in their peripheral vision — without opening any panel.'));

children.push(h('2.1 Layout (left to right)', HeadingLevel.HEADING_2));
children.push(table(
    ['Chip', 'Reads', 'Click action'],
    [
        ['● HOSTNAME',  'Local machine; dot heartbeats every 3.6 s',     'Open Diagnostics'],
        ['⚯ N/M hosts', 'Remote hosts online / total (only ≥ 1 host)',   'Open NexShell'],
        ['⚠ N alerts',  'Active incidents from activeIncidentId',        'Open Dashboard'],
        ['⊕ guard',     'Active security skill, or "clean"',             'Open skill picker'],
        ['HH:MM',       'Local time; updates once per minute',           '(passive)'],
        ['●●●○○',       '5-dot posture: calm → vigilant → suspicious → alarmed → panic', 'Open Diagnostics'],
    ],
    [1600, 4800, CONTENT_W - 1600 - 4800],
));

children.push(h('2.2 Posture derivation', HeadingLevel.HEADING_2));
children.push(p('The five-dot indicator is derived from runtime state, not configured manually:'));
children.push(bullet('calm (0 dots) — default state.'));
children.push(bullet('vigilant (1 dot) — any tab has isProcessing = true.'));
children.push(bullet('suspicious (2 dots) — any tab has isExecuting = true (tools running).'));
children.push(bullet('alarmed (3 dots) — activeIncidentId is truthy.'));
children.push(bullet('panic (4 dots) — multiple concurrent incidents OR repeated guard hits.'));

children.push(new Paragraph({ children: [], pageBreakBefore: true }));

// ── 3. Per-tab purpose tint ──────────────────────────────────────────────
children.push(h('3. Per-tab Purpose Tint', HeadingLevel.HEADING_1));
children.push(p('Each tab carries a coloured top accent that classifies its operational role at a glance. Tab strip becomes a session map instead of a list of indistinguishable chat threads.'));

children.push(table(
    ['Purpose', 'Trigger', 'Colour'],
    [
        ['incident',      'tab.activeIncident truthy',                              'Red (with slow pulse)'],
        ['executing',     'tab.isExecuting = true (tools running)',                 'Violet'],
        ['investigation', 'keywords in title or last 3 messages: phishing, malware, threat, breach, forensic, CVE, ransom, c2, intrusion, etc.', 'Amber'],
        ['reference',     'keywords: docs, guide, manual, how-to, tutorial, qué es, cómo, explica',  'Blue'],
        ['chat (default)','none of the above',                                      'Green (existing accent)'],
    ],
    [1600, 5800, CONTENT_W - 1600 - 5800],
));

children.push(new Paragraph({ children: [], pageBreakBefore: true }));

// ── 4. Terminal-recording code blocks ────────────────────────────────────
children.push(h('4. Terminal-Recording Code Blocks', HeadingLevel.HEADING_1));
children.push(p('Lucy\'s command-output blocks (warp-block) now read as forensic recordings instead of generic code-fenced dumps.'));

children.push(h('4.1 Header anatomy', HeadingLevel.HEADING_2));
children.push(table(
    ['Element', 'Meaning'],
    [
        ['Three traffic-light dots', 'Decorative, asciinema-style. The leftmost dot acts as a tiny health LED — green on success, red on error.'],
        ['Hostname chip',            'Renders only when meta.hostname is passed by the caller. Small monospace pill ("PRECISION-X / web-01 / …").'],
        ['Engine glyph prompt',      '⚡ powershell · ▶ cmd · $ bash · ◇ wmic · ⌬ netsh · ☐ reg · ※ cscript · ⇄ remote'],
        ['Absolute timestamp',       'HH:MM:SS, renders only when meta.ts is provided.'],
        ['Elapsed ms',               'Tabular numerals, always shown.'],
        ['Exit-code badge',          '"exit 0" (green pill) or "exit ≠0" (red pill). First-class metadata > vague checkmark.'],
    ],
    [2800, CONTENT_W - 2800],
));

children.push(new Paragraph({ children: [], pageBreakBefore: true }));

// ── 5. Self-diagnostics + repair ─────────────────────────────────────────
children.push(h('5. Self-Diagnostics & One-Click Repair', HeadingLevel.HEADING_1));
children.push(p('Lucy ships a Diagnostics panel that runs eight health checks (system resources, DB integrity, LLM tier health, memory pipeline, stream sessions, app log, credential store, guardrails) and surfaces repair buttons for the failures it knows how to fix.'));

children.push(h('5.1 Repair: NULL confidence values', HeadingLevel.HEADING_2));
children.push(p('When PRAGMA quick_check reports "NULL value in agent_memories.confidence", the row gains a "Reparar confidence NULL" button. The repair runs four phases:'));
children.push(bullet('Phase 1 — Counts NULLs per table (agent_memories, memory_core, agent_insights) for reporting.'));
children.push(bullet('Phase 2 — UPDATE … SET confidence = COALESCE(confidence, 0.5) per table in one transaction. Forces SQLite to rewrite every storage page.'));
children.push(bullet('Phase 3 — REINDEX. Rebuilds all indexes and FTS5 shadow tables.'));
children.push(bullet('Phase 4 — Fresh PRAGMA quick_check. The result is surfaced in the toast.'));

children.push(p('The repair is idempotent — running on a clean DB returns "no NULLs were present" with a refreshed-rows count.'));

children.push(new Paragraph({ children: [], pageBreakBefore: true }));

// ── 6. Slash commands ────────────────────────────────────────────────────
children.push(h('6. Slash Command Reference', HeadingLevel.HEADING_1));
children.push(p('Type / inside the composer at any time to discover commands. Below are the categories most used in v1.7.66.'));

children.push(table(
    ['Command', 'Purpose'],
    [
        ['/memory',              'Open the Memory Browser (memories, crystals, insights, graph).'],
        ['/kg',                  'Open the Knowledge Graph viewer.'],
        ['/sec-skill',           'Browse + activate security skills (phishing, forensics, IR, etc.).'],
        ['/cpu',                 'Show CPU SIMD / vendor info (AVX-512 / AVX2 detection).'],
        ['/capabilities',        'Lucy\'s self-introspection of loaded skills, frameworks, MCP servers.'],
        ['/snapshot',            'Capture a state snapshot (F2 Frontier).'],
        ['/diff',                'Diff two state snapshots over time.'],
        ['/detective',           'Incident Detective — synthesises F3 + F8 + F9 into one forensic query.'],
        ['/routines',            'View / edit daily routines learned by F10.'],
        ['/notebook',            'Export the current tab as .ipynb / .lucynote.'],
        ['/revert <path>',       'Restore the pre-write content of a file Lucy modified.'],
        ['/preview <cmd>',       'Sandbox preview before running a destructive command.'],
        ['/runbooks',            'Open the runbook list.'],
        ['/model',               'Switch the active model for this tab.'],
    ],
    [2200, CONTENT_W - 2200],
));

children.push(new Paragraph({ children: [], pageBreakBefore: true }));

// ── 7. Troubleshooting ───────────────────────────────────────────────────
children.push(h('7. Troubleshooting', HeadingLevel.HEADING_1));

children.push(h('7.1 Diagnostics panel shows a yellow / red row', HeadingLevel.HEADING_2));
children.push(p('Open Auto-Diagnóstico (sidebar → Diagnóstico). If the failing row carries a "Reparar" button, click it — Lucy will run a known fix and re-scan automatically. If no button shows, the toast or row tooltip explains the exact SQLite / OS error so it can be investigated manually.'));

children.push(h('7.2 GPU usage seems high', HeadingLevel.HEADING_2));
children.push(p('Open Task Manager → Details → enable the GPU column. Verify msedgewebview2.exe (Lucy\'s renderer) is using the discrete GPU on hybrid laptops. If not, confirm Lucy is running a release build (v1.7.42+) — the vendor hints only export in release.'));

children.push(h('7.3 Tutorial keeps opening on every launch', HeadingLevel.HEADING_2));
children.push(p('Fixed in v1.7.21 by passing currentVersion as a prop. If it still re-opens, check that package.json and tauri.conf.json agree on the version string — the host compares against tauri.conf.json\'s appVersion.'));

children.push(h('7.4 Text disappears while Lucy is streaming', HeadingLevel.HEADING_2));
children.push(p('Should not happen after the v1.7.45-65 streaming sprint. If it does, open DevTools → Console; the rAF + morphdom path logs warnings when a late callback tries to clobber a promoted bubble. Capture the warning + a screenshot and file an issue.'));

children.push(new Paragraph({ children: [], pageBreakBefore: true }));

// ── 8. License + attribution ─────────────────────────────────────────────
children.push(h('8. License & Attribution', HeadingLevel.HEADING_1));
children.push(p('Lucy is licensed under GNU GPLv3.'));
children.push(p([
    bold('Author / maintainer: '),
    new TextRun({ text: 'Iván Eduardo Luna (@Phenomx64L) · ', font: 'Arial' }),
    new TextRun({ text: 'https://github.com/Phenomx64L/LucyAI', font: 'Arial', color: '0563C1', underline: { type: 'single' } }),
]));
children.push(p([
    bold('System prompt RULES (intellectual property): '),
    new TextRun({ text: 'derived from 10+ years of SysAdmin expertise. Protected by GPLv3.', font: 'Arial' }),
]));

// Footer for all pages — page number + manual reference
const footer = new Footer({
    children: [new Paragraph({
        alignment: AlignmentType.CENTER,
        children: [
            new TextRun({ text: 'Lucy Assistant Manual v1.7.66 · Page ', font: 'Arial', size: 18, color: SLATE_500 }),
            new TextRun({ children: [PageNumber.CURRENT], font: 'Arial', size: 18, color: SLATE_500 }),
        ],
    })],
});

// ── Assemble ───────────────────────────────────────────────────────────────
const doc = new Document({
    styles: {
        default: { document: { run: { font: 'Arial', size: 22 } } },
        paragraphStyles: [
            {
                id: 'Heading1', name: 'Heading 1', basedOn: 'Normal', next: 'Normal', quickFormat: true,
                run: { size: 36, bold: true, font: 'Arial', color: ACCENT_DARK },
                paragraph: { spacing: { before: 360, after: 180 }, outlineLevel: 0 },
            },
            {
                id: 'Heading2', name: 'Heading 2', basedOn: 'Normal', next: 'Normal', quickFormat: true,
                run: { size: 28, bold: true, font: 'Arial' },
                paragraph: { spacing: { before: 240, after: 120 }, outlineLevel: 1 },
            },
        ],
    },
    numbering: {
        config: [{
            reference: 'bullets',
            levels: [{
                level: 0, format: LevelFormat.BULLET, text: '•',
                alignment: AlignmentType.LEFT,
                style: { paragraph: { indent: { left: 720, hanging: 360 } } },
            }],
        }],
    },
    sections: [{
        properties: {
            page: {
                size: { width: PAGE_WIDTH, height: PAGE_HEIGHT },
                margin: { top: MARGIN, right: MARGIN, bottom: MARGIN, left: MARGIN },
            },
        },
        footers: { default: footer },
        children,
    }],
});

const outPath = path.join(os.homedir(), 'Desktop', 'Lucy_Assistant_Manual_v1.7.66.docx');
Packer.toBuffer(doc).then(buf => {
    fs.writeFileSync(outPath, buf);
    console.log('wrote:', outPath, '(' + (buf.length / 1024).toFixed(1) + ' KB)');
});
