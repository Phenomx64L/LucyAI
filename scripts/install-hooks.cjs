#!/usr/bin/env node
// ──────────────────────────────────────────────────────────────────────────────
// Lucy hooks installer.
//
// One-shot setup: points git at the project's tracked .githooks directory
// instead of the per-repo .git/hooks (which isn't version-controlled).
//
// Run:    node scripts/install-hooks.cjs   (or `npm run hooks:install`)
// Uninstall: git config --unset core.hooksPath
//
// Cross-platform — uses git itself, no symlinks or chmod tricks.
// Idempotent — safe to run repeatedly.
// ──────────────────────────────────────────────────────────────────────────────

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const REPO_ROOT = path.resolve(__dirname, '..');
const HOOKS_DIR = path.join(REPO_ROOT, '.githooks');

function run(cmd, opts = {}) {
    return execSync(cmd, { cwd: REPO_ROOT, encoding: 'utf8', stdio: 'pipe', ...opts }).trim();
}

// Sanity: are we even in a git repo?
try {
    run('git rev-parse --git-dir');
} catch (e) {
    console.error('✗ Not inside a git repository — aborting.');
    process.exit(1);
}

if (!fs.existsSync(HOOKS_DIR)) {
    console.error(`✗ ${HOOKS_DIR} does not exist. Did you fetch the .githooks directory?`);
    process.exit(1);
}

// Set core.hooksPath. Git accepts forward slashes on Windows too.
run('git config core.hooksPath .githooks');

// Verify the setting stuck.
const current = run('git config --get core.hooksPath');
if (current !== '.githooks') {
    console.error(`✗ Failed to set core.hooksPath — git reports "${current}".`);
    process.exit(1);
}

// On Unix-like systems the hook file must be executable. On Windows git-bash
// runs the hook regardless of the exec bit, but flipping it is harmless and
// makes Linux/macOS contributors happy.
const preCommit = path.join(HOOKS_DIR, 'pre-commit');
if (fs.existsSync(preCommit) && process.platform !== 'win32') {
    try { fs.chmodSync(preCommit, 0o755); } catch (_) {}
}

console.log('✓ Lucy git hooks installed.');
console.log('  core.hooksPath → .githooks');
console.log('  Active hooks  :');
for (const f of fs.readdirSync(HOOKS_DIR)) {
    const p = path.join(HOOKS_DIR, f);
    if (fs.statSync(p).isFile() && !f.startsWith('.')) {
        console.log(`     • ${f}`);
    }
}
console.log('');
console.log('  To disable temporarily: git commit --no-verify');
console.log('  To uninstall fully    : git config --unset core.hooksPath');
