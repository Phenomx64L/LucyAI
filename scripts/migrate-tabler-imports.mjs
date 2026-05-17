#!/usr/bin/env node
// ── migrate-tabler-imports.mjs ────────────────────────────────────────────
//
// One-shot migration (audit P3): converts barrel imports from
// `@tabler/icons-svelte` into per-icon imports from
// `@tabler/icons-svelte/dist/icons/<kebab-case>.svelte`.
//
// Why: the barrel re-exports ~4500 icons. Vite/Rollup CAN tree-shake it
// but on dev SSR + cold builds the icon graph traversal is expensive,
// and some chunks pick up icons they don't actually use. Per-icon
// imports give the bundler the tightest possible dep graph.
//
// The Tabler `exports` map in package.json publishes `./icons/*` → the
// internal `./dist/icons/*.svelte` files. We MUST use the public
// `./icons/<name>` form (no `dist/`, no `.svelte` suffix) — that path
// resolves through the exports map; the `dist/...` path is not exported
// publicly and rollup refuses it.
//
// Pattern matched (single-line, multi-icon):
//   import { IconChartBar as BarChart3, IconBell, ... } from '@tabler/icons-svelte';
// Becomes:
//   import BarChart3 from '@tabler/icons-svelte/icons/chart-bar';
//   import IconBell  from '@tabler/icons-svelte/icons/bell';
//
// Naming convention: strip `Icon` prefix, then camelCase → kebab-case.
//   `IconChartBar`       → `chart-bar`
//   `IconAlertTriangle`  → `alert-triangle`
//   `IconFileTypePdf`    → `file-type-pdf`
//   `IconArrowsDoubleNeSw` → `arrows-double-ne-sw`  (consecutive caps stay together until next word)

import { readFileSync, writeFileSync } from 'node:fs';
import { execSync } from 'node:child_process';

const ROOT = process.argv[2] || 'src';
const DRY_RUN = process.argv.includes('--dry');

function kebab(iconName) {
    // `IconChartBar` → `chart-bar` · `IconTerminal2` → `terminal-2`
    const noPrefix = iconName.replace(/^Icon/, '');
    return noPrefix
        // Letter → digit (e.g. `Terminal2` → `Terminal-2`)
        .replace(/([a-zA-Z])([0-9])/g, '$1-$2')
        // Digit → uppercase letter (e.g. `H1Letter` → `H1-Letter`)
        .replace(/([0-9])([A-Z])/g, '$1-$2')
        // Lowercase/digit → uppercase letter
        .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
        // Uppercase run followed by uppercase + lowercase (e.g. `NeSw`)
        .replace(/([A-Z]+)([A-Z][a-z])/g, '$1-$2')
        .toLowerCase();
}

const IMPORT_RE = /^(\s*)import\s*\{\s*([^}]+)\}\s*from\s*['"]@tabler\/icons-svelte['"];?\s*$/m;

function rewriteFile(path) {
    const src = readFileSync(path, 'utf8');
    const m = src.match(IMPORT_RE);
    if (!m) return { path, status: 'no-match' };

    const [, indent, body] = m;
    // Parse each entry: `IconChartBar as BarChart3` or `IconBell`
    const entries = body
        .split(',')
        .map(s => s.trim())
        .filter(Boolean)
        .map(part => {
            const aliasMatch = part.match(/^(Icon\w+)\s+as\s+(\w+)$/);
            if (aliasMatch) return { source: aliasMatch[1], local: aliasMatch[2] };
            const plainMatch = part.match(/^(Icon\w+)$/);
            if (plainMatch) return { source: plainMatch[1], local: plainMatch[1] };
            return { source: null, local: null, raw: part };
        });

    const bad = entries.find(e => !e.source);
    if (bad) return { path, status: 'parse-failed', detail: bad.raw };

    const newImports = entries
        .map(e => `${indent}import ${e.local} from '@tabler/icons-svelte/icons/${kebab(e.source)}';`)
        .join('\n');

    const out = src.replace(IMPORT_RE, newImports);
    if (DRY_RUN) {
        return { path, status: 'would-write', count: entries.length };
    }
    writeFileSync(path, out, 'utf8');
    return { path, status: 'rewritten', count: entries.length };
}

// Enumerate target files via grep — cheaper than a recursive walk
const grepCmd = `grep -rln "from '@tabler/icons-svelte'" ${ROOT}`;
const files = execSync(grepCmd, { encoding: 'utf8' })
    .split('\n')
    .filter(Boolean);

let total = 0;
const errors = [];
for (const f of files) {
    const r = rewriteFile(f);
    if (r.status === 'rewritten' || r.status === 'would-write') {
        total += r.count;
        console.log(`  ${r.status === 'would-write' ? '[dry] ' : ''}${f} (${r.count} icons)`);
    } else if (r.status === 'parse-failed') {
        errors.push(`PARSE FAIL: ${f} — ${r.detail}`);
    } else {
        console.log(`  skip ${f}: ${r.status}`);
    }
}

if (errors.length) {
    console.error('\nErrors:');
    for (const e of errors) console.error(`  ${e}`);
    process.exit(1);
}
console.log(`\n${DRY_RUN ? '[DRY RUN] ' : ''}Total icons migrated: ${total} across ${files.length} files.`);
