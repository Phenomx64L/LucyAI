// scripts/benchmark.mjs — single-shot health & capability benchmark for Lucy.
//
// Runs (in order):
//   1. cargo check          — Rust compiles, no warnings
//   2. cargo test            — Rust unit tests (cron parser, etc.)
//   3. svelte-check          — TypeScript / Svelte type-check
//   4. vitest run            — pure-logic test suite (compressor, anomaly, …)
//   5. vite build            — the production frontend bundle compiles
//
// Each stage is timed and reported. Exit non-zero on any failure so this
// can be wired into pre-commit hooks or CI later.
//
// Usage:
//   npm run benchmark          # one-shot, prints summary
//   node scripts/benchmark.mjs # same thing without npm wrapper

import { spawn } from 'node:child_process';
import { performance } from 'node:perf_hooks';

const STAGES = [
    { id: 'cargo-check',  cmd: 'cargo',     args: ['check', '--quiet'],                         cwd: 'src-tauri' },
    { id: 'cargo-test',   cmd: 'cargo',     args: ['test',  '--lib', '--quiet'],                cwd: 'src-tauri' },
    { id: 'svelte-check', cmd: 'npx',       args: ['svelte-check', '--threshold', 'warning'],   cwd: '.' },
    { id: 'vitest',       cmd: 'npx',       args: ['vitest', 'run', '--reporter=basic'],        cwd: '.' },
    { id: 'vite-build',   cmd: 'npm',       args: ['run', 'build'],                             cwd: '.' },
];

const isWindows = process.platform === 'win32';

function run(stage) {
    return new Promise(resolve => {
        const t0 = performance.now();
        const proc = spawn(stage.cmd, stage.args, {
            cwd: stage.cwd,
            stdio: ['ignore', 'pipe', 'pipe'],
            shell: isWindows,        // Windows needs shell:true to find npm/npx/cargo
        });
        let stdout = '';
        let stderr = '';
        proc.stdout.on('data', d => { stdout += d.toString(); });
        proc.stderr.on('data', d => { stderr += d.toString(); });
        proc.on('exit', code => {
            const ms = performance.now() - t0;
            resolve({ stage, code: code ?? -1, ms, stdout, stderr });
        });
        proc.on('error', e => {
            const ms = performance.now() - t0;
            resolve({ stage, code: -1, ms, stdout, stderr: String(e.message || e) });
        });
    });
}

function color(s, c) {
    const codes = { red: 31, green: 32, yellow: 33, cyan: 36, gray: 90 };
    return process.stdout.isTTY ? `\x1b[${codes[c] ?? 0}m${s}\x1b[0m` : s;
}

function fmtMs(ms) {
    if (ms < 1000) return `${ms.toFixed(0)} ms`;
    if (ms < 60000) return `${(ms / 1000).toFixed(2)} s`;
    return `${(ms / 60000).toFixed(2)} min`;
}

const banner = `
${color('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━', 'cyan')}
   ${color('Lucy benchmark — capability & health check', 'cyan')}
${color('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━', 'cyan')}
`;
console.log(banner);

const results = [];
const tStart = performance.now();
for (const stage of STAGES) {
    process.stdout.write(`▸ ${stage.id.padEnd(14)} `);
    const r = await run(stage);
    results.push(r);
    if (r.code === 0) {
        console.log(`${color('PASS', 'green')}   ${color(fmtMs(r.ms), 'gray')}`);
    } else {
        console.log(`${color('FAIL', 'red')}   ${color(fmtMs(r.ms), 'gray')}  (exit ${r.code})`);
        // Surface the tail of the failed stage's output so we can diagnose.
        const tail = (r.stderr || r.stdout || '').split('\n').slice(-30).join('\n');
        console.log(color('--- failure output (last 30 lines) ---', 'yellow'));
        console.log(tail);
        console.log(color('--------------------------------------', 'yellow'));
    }
}

const tTotal = performance.now() - tStart;
const passed = results.filter(r => r.code === 0).length;
const failed = results.length - passed;

console.log('');
console.log(color('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━', 'cyan'));
console.log(
    `${color('Result', 'cyan')}: ` +
    (failed === 0
        ? color(`✓ all ${passed} stages passed`, 'green')
        : color(`✗ ${failed} of ${results.length} stages failed`, 'red')) +
    `   ${color(`(total ${fmtMs(tTotal)})`, 'gray')}`
);
console.log(color('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━', 'cyan'));

process.exit(failed === 0 ? 0 : 1);
