#!/usr/bin/env node
/**
 * lucy-safe-dev.mjs — Safety wrapper for Lucy development
 *
 * Usage:
 *   node lucy-safe-dev.mjs snapshot          → Create a named snapshot (auto-commit)
 *   node lucy-safe-dev.mjs check             → Verify the build works
 *   node lucy-safe-dev.mjs restore <hash>    → Restore a previous snapshot
 *   node lucy-safe-dev.mjs list              → List all snapshots
 *   node lucy-safe-dev.mjs diff              → Show what has changed vs HEAD
 *   node lucy-safe-dev.mjs sync-css          → Sync src/app.css ← src/routes/app.css
 */

import { execSync, spawnSync } from 'child_process';
import fs from 'fs';
import path from 'path';

const SNAPSHOT_TAG_PREFIX = 'lucy-snapshot-';
const CRITICAL_FILES = [
  'src/routes/+page.svelte',
  'src/routes/app.css',
  'src/app.css',
  'src-tauri/src/commands/ai.rs',
  'src-tauri/src/lib.rs',
];

function run(cmd, opts = {}) {
  try {
    return execSync(cmd, { encoding: 'utf8', stdio: opts.quiet ? 'pipe' : 'inherit', ...opts });
  } catch (e) {
    return null;
  }
}

function runSilent(cmd) {
  return run(cmd, { quiet: true }) || '';
}

const [,, command, ...args] = process.argv;

switch (command) {

  case 'snapshot': {
    const label = args[0] || new Date().toISOString().replace(/[:.]/g, '-');
    const tag = SNAPSHOT_TAG_PREFIX + label;
    // Auto-add all modified files
    run('git add -A');
    const status = runSilent('git status --short').trim();
    if (!status) {
      console.log('ℹ️  No changes to snapshot — working tree is clean.');
      const lastTag = runSilent('git tag -l "'+SNAPSHOT_TAG_PREFIX+'*" --sort=-creatordate').split('\n')[0].trim();
      console.log('   Last snapshot:', lastTag || '(none)');
      break;
    }
    run(`git commit -m "snapshot: ${label}"`);
    run(`git tag ${tag}`);
    console.log(`✅ Snapshot created: ${tag}`);
    console.log('   Files saved:');
    status.split('\n').forEach(l => console.log('   ', l));
    break;
  }

  case 'list': {
    const tags = runSilent(`git tag -l "${SNAPSHOT_TAG_PREFIX}*" --sort=-creatordate`).trim();
    if (!tags) { console.log('ℹ️  No snapshots found.'); break; }
    console.log('📋 Lucy snapshots (newest first):');
    tags.split('\n').forEach(t => {
      const hash = runSilent(`git rev-parse --short ${t}`).trim();
      const date = runSilent(`git log -1 --format=%ci ${t}`).trim();
      console.log(`  ${hash}  ${t.replace(SNAPSHOT_TAG_PREFIX,'')}  (${date})`);
    });
    break;
  }

  case 'restore': {
    const target = args[0];
    if (!target) { console.log('Usage: node lucy-safe-dev.mjs restore <snapshot-label-or-hash>'); break; }
    // Auto-snapshot current state before restoring
    run('git add -A');
    const dirty = runSilent('git status --short').trim();
    if (dirty) {
      const now = new Date().toISOString().replace(/[:.]/g, '-');
      run(`git commit -m "auto-snapshot before restore: ${now}"`);
      run(`git tag ${SNAPSHOT_TAG_PREFIX}before-restore-${now}`);
      console.log('✅ Current state saved before restoring.');
    }
    const tag = target.startsWith(SNAPSHOT_TAG_PREFIX) ? target : SNAPSHOT_TAG_PREFIX + target;
    const hash = runSilent(`git rev-parse --short ${tag} 2>/dev/null`).trim()
              || runSilent(`git rev-parse --short ${target} 2>/dev/null`).trim();
    if (!hash) { console.log(`❌ Snapshot "${target}" not found.`); break; }
    // Restore ONLY critical files, not the whole repo
    console.log(`🔄 Restoring critical files to ${hash}...`);
    CRITICAL_FILES.forEach(f => {
      const result = spawnSync('git', ['checkout', hash, '--', f], { encoding: 'utf8' });
      if (result.status === 0) console.log(`   ✅ ${f}`);
      else console.log(`   ⚠️  ${f} — not found in ${hash}, skipped`);
    });
    break;
  }

  case 'check': {
    console.log('🔍 Running build sanity check...');
    // 1. Check critical files exist
    let ok = true;
    CRITICAL_FILES.forEach(f => {
      if (!fs.existsSync(f)) { console.log(`❌ MISSING: ${f}`); ok = false; }
      else console.log(`✅ EXISTS: ${f}`);
    });

    // 2. Check CSS sync
    const srcCss = fs.readFileSync('src/app.css', 'utf8');
    const routesCss = fs.readFileSync('src/routes/app.css', 'utf8');
    if (srcCss === routesCss) {
      console.log('✅ src/app.css is in sync with src/routes/app.css');
    } else {
      console.log('⚠️  WARNING: src/app.css and src/routes/app.css are OUT OF SYNC!');
      console.log('   Run: node lucy-safe-dev.mjs sync-css');
      ok = false;
    }

    // 3. Check MAX_LOOPS
    const page = fs.readFileSync('src/routes/+page.svelte', 'utf8');
    const loopsMatch = page.match(/const MAX_LOOPS = (\d+)/);
    const loops = loopsMatch ? parseInt(loopsMatch[1]) : 0;
    if (loops >= 100) console.log(`✅ MAX_LOOPS = ${loops}`);
    else { console.log(`⚠️  MAX_LOOPS = ${loops} (should be >= 100)`); ok = false; }

    // 4. Check key features
    const checks = [
      ['backgroundTasks', 'backgroundTasks store (fork/wait)'],
      ['identicalErrorCount', 'Anti-Stuck protocol'],
      ['setTheme', 'Theme switcher'],
      ['sidebar-glass', 'Glassmorphism sidebar'],
      ['bg-warp-gradient', 'Warp gradient background'],
      ['theme-picker', 'Theme picker dots'],
      ['wait_task', 'wait_task tool handler'],
      ['fork_task', 'fork_task tool handler'],
    ];
    checks.forEach(([needle, label]) => {
      if (page.includes(needle)) console.log(`✅ ${label}`);
      else { console.log(`❌ MISSING: ${label} (search: ${needle})`); ok = false; }
    });

    console.log(ok ? '\n🚀 All checks passed!' : '\n⚠️  Some checks failed — review above.');
    break;
  }

  case 'sync-css': {
    const routesCss = fs.readFileSync('src/routes/app.css', 'utf8');
    fs.writeFileSync('src/app.css', routesCss);
    console.log('✅ src/app.css synced from src/routes/app.css');
    console.log('   Size:', routesCss.length, 'bytes');
    break;
  }

  case 'diff': {
    console.log('📊 Changes vs HEAD:');
    run('git diff --stat HEAD');
    break;
  }

  default:
    console.log(`
Lucy Safe Dev Tool
==================
Commands:
  node lucy-safe-dev.mjs snapshot [label]   → Save current state
  node lucy-safe-dev.mjs list               → View all snapshots
  node lucy-safe-dev.mjs restore <label>    → Restore a snapshot (saves current first)
  node lucy-safe-dev.mjs check              → Verify all features are present
  node lucy-safe-dev.mjs sync-css           → Sync src/app.css ← src/routes/app.css
  node lucy-safe-dev.mjs diff               → Show current changes

⚠️  NEVER run: git checkout src/routes/+page.svelte  (this DELETES uncommitted work)
    Use 'restore' instead — it saves before restoring.
`);
}
