// ── command-signatures.ts — Curated DB of SysAdmin commands ────────────────
//
// What this is
// ------------
// A small, hand-curated catalog of commands that Lucy frequently encounters,
// each annotated with:
//   • flags (and short descriptions)
//   • destructive: boolean flag (drives confirm prompts)
//   • category (network / fs / process / service / user / package / etc.)
//   • shells where it's available (bash, pwsh, cmd)
//
// What it enables (next steps, not yet wired up everywhere)
//   1) Pre-execution explanation:
//      "you're about to run `rm -rf /var/log` — recursive AND force flags, this
//       will permanently delete 1.2 GB across 47 files. Confirm?"
//   2) Flag autocomplete in the input bar (Tab to expand `-r` to `--recursive`).
//   3) Smarter Guardian: today only matches exact keywords; with signatures we
//      can detect ANY combination of destructive flags + sensitive paths.
//   4) Better runbook authoring: agent can suggest the safest flags
//      (`-i` interactive, `--dry-run`) when generating multi-step plans.
//
// Why hand-curated (not LLM-generated)
//   This catalog gates SECURITY decisions. An LLM hallucinating "this flag is
//   safe" could brick a production host. Curated by humans, reviewed, versioned.
//   Start small with the 30 most-used commands; expand as needed.
//
// Schema design notes
//   - `dangerous` is a tri-state: 'always' / 'with-flags' / 'never'.
//     'with-flags' means safe by default but turns destructive with specific
//     combinations (e.g. `rm` alone errors, `rm -rf` nukes).
//   - `requireConfirm` is the SUBSET of dangerous flags that trigger a
//     mandatory confirm dialog before execution.
//   - `shells` lets us hide signatures inappropriate for the active shell.

export type Shell = 'bash' | 'pwsh' | 'cmd' | 'any';
export type Category =
    | 'fs'         // filesystem ops
    | 'net'        // networking / connectivity
    | 'process'    // process management
    | 'service'    // systemd / service control
    | 'user'       // user/account ops
    | 'package'    // package managers (apt, dnf, choco, winget)
    | 'storage'    // disk / partition / fs creation
    | 'security'   // perms, ACLs, firewall
    | 'inspect'    // read-only diagnostics
    | 'other';
export type Danger = 'always' | 'with-flags' | 'never';

export interface FlagSpec {
    /** Flag form as the user would type it. e.g. "-r", "--recursive" */
    flag: string;
    /** Short description (used in tooltip/autocomplete) */
    desc: string;
    /** True if this flag specifically makes the command destructive */
    destructive?: boolean;
}

export interface CommandSignature {
    /** Lowercased command name as it appears at the START of the input */
    name: string;
    /** All aliases that should match (e.g. PowerShell's "ls" → "Get-ChildItem") */
    aliases?: string[];
    /** Shells where this command exists. 'any' means cross-platform */
    shells: Shell[];
    category: Category;
    /** Plain-text one-line description for tooltips and explainers */
    summary: string;
    /** Tri-state danger classification */
    dangerous: Danger;
    /** Flags catalog — partial; covers the most-asked ones */
    flags: FlagSpec[];
    /**
     * Flag combinations that REQUIRE a confirm modal before execution.
     * Each entry is a list of flags ALL of which must be present.
     * e.g. for `rm` → [['-r', '-f'], ['--recursive', '--force']]
     */
    requireConfirm?: string[][];
    /** Optional safer alternative to suggest instead. */
    saferAlternative?: { command: string; reason: string };
}

// ── The catalog ──────────────────────────────────────────────────────────
// Start with 30 most-encountered. Reorder by frequency in your env.
export const SIGNATURES: readonly CommandSignature[] = Object.freeze([
    // ── Filesystem ──────────────────────────────────────────────────────
    {
        name: 'rm', shells: ['bash'], category: 'fs',
        summary: 'Remove files or directories',
        dangerous: 'with-flags',
        flags: [
            { flag: '-r', desc: 'Recursive (also -R, --recursive)', destructive: true },
            { flag: '-f', desc: 'Force, ignore prompts and missing files', destructive: true },
            { flag: '-i', desc: 'Interactive — prompt before each removal' },
            { flag: '-v', desc: 'Verbose — list what was deleted' },
            { flag: '--preserve-root', desc: 'Refuse to act on /' },
        ],
        requireConfirm: [['-r', '-f'], ['--recursive', '--force'], ['-rf'], ['-fr']],
        saferAlternative: { command: 'trash', reason: 'Sends to recycle bin instead of unlinking permanently' },
    },
    {
        name: 'cp', shells: ['bash'], category: 'fs',
        summary: 'Copy files / directories',
        dangerous: 'never',
        flags: [
            { flag: '-r', desc: 'Recursive — copy directories' },
            { flag: '-p', desc: 'Preserve mode, owner, timestamps' },
            { flag: '-a', desc: 'Archive — preserve everything (= -dR --preserve=all)' },
            { flag: '-v', desc: 'Verbose' },
            { flag: '-i', desc: 'Interactive — prompt before overwrite' },
        ],
    },
    {
        name: 'mv', shells: ['bash'], category: 'fs',
        summary: 'Move / rename',
        dangerous: 'never',
        flags: [
            { flag: '-f', desc: 'Force overwrite without prompt' },
            { flag: '-i', desc: 'Interactive — confirm overwrites' },
            { flag: '-n', desc: 'Never overwrite an existing file' },
        ],
    },
    {
        name: 'find', shells: ['bash'], category: 'inspect',
        summary: 'Recursive file search',
        dangerous: 'with-flags',
        flags: [
            { flag: '-name', desc: 'Match by filename glob' },
            { flag: '-type', desc: 'Filter by type (f=file, d=dir, l=symlink)' },
            { flag: '-delete', desc: 'DELETE matching entries', destructive: true },
            { flag: '-exec', desc: 'Execute command on each match', destructive: true },
            { flag: '-mtime', desc: 'Modified N days ago' },
        ],
        requireConfirm: [['-delete']],
    },
    {
        name: 'chmod', shells: ['bash'], category: 'security',
        summary: 'Change file permissions',
        dangerous: 'with-flags',
        flags: [
            { flag: '-R', desc: 'Recursive', destructive: true },
            { flag: '777', desc: 'World-writable mode (DANGEROUS for sensitive files)', destructive: true },
        ],
    },
    {
        name: 'chown', shells: ['bash'], category: 'security',
        summary: 'Change file owner / group',
        dangerous: 'with-flags',
        flags: [
            { flag: '-R', desc: 'Recursive', destructive: true },
        ],
    },

    // ── Process / Service ──────────────────────────────────────────────
    {
        name: 'kill', shells: ['bash'], category: 'process',
        summary: 'Send signal to process',
        dangerous: 'with-flags',
        flags: [
            { flag: '-9',     desc: 'SIGKILL — uncatchable termination', destructive: true },
            { flag: '-SIGKILL', desc: 'Same as -9', destructive: true },
            { flag: '-15',    desc: 'SIGTERM — graceful (default)' },
            { flag: '-l',     desc: 'List signals' },
        ],
    },
    {
        name: 'systemctl', shells: ['bash'], category: 'service',
        summary: 'systemd unit control',
        dangerous: 'with-flags',
        flags: [
            { flag: 'stop',     desc: 'Stop a unit', destructive: true },
            { flag: 'disable',  desc: 'Disable at boot', destructive: true },
            { flag: 'mask',     desc: 'Forbid the unit from being started', destructive: true },
            { flag: 'restart',  desc: 'Restart' },
            { flag: 'status',   desc: 'Show status (read-only)' },
            { flag: 'enable',   desc: 'Enable at boot' },
            { flag: 'reload',   desc: 'Reload config without full restart' },
        ],
    },

    // ── Network ─────────────────────────────────────────────────────────
    {
        name: 'iptables', shells: ['bash'], category: 'net',
        summary: 'Linux firewall rule mgmt',
        dangerous: 'with-flags',
        flags: [
            { flag: '-F', desc: 'Flush ALL rules', destructive: true },
            { flag: '-A', desc: 'Append rule to chain' },
            { flag: '-D', desc: 'Delete rule', destructive: true },
            { flag: '-L', desc: 'List rules (read-only)' },
            { flag: '-P', desc: 'Set default policy', destructive: true },
        ],
        requireConfirm: [['-F']],
    },
    {
        name: 'ssh', shells: ['bash', 'pwsh'], category: 'net',
        summary: 'Secure shell connection',
        dangerous: 'never',
        flags: [
            { flag: '-i', desc: 'Identity file (private key)' },
            { flag: '-p', desc: 'Port' },
            { flag: '-L', desc: 'Local port forward' },
            { flag: '-R', desc: 'Remote port forward' },
            { flag: '-N', desc: 'No command — just tunnel' },
        ],
    },
    {
        name: 'curl', shells: ['bash', 'pwsh'], category: 'net',
        summary: 'HTTP/FTP transfer',
        dangerous: 'never',
        flags: [
            { flag: '-X', desc: 'HTTP method' },
            { flag: '-H', desc: 'Header' },
            { flag: '-d', desc: 'POST body data' },
            { flag: '-k', desc: 'INSECURE — skip TLS verification', destructive: false },
            { flag: '-o', desc: 'Output to file' },
            { flag: '-L', desc: 'Follow redirects' },
        ],
    },

    // ── Package management ─────────────────────────────────────────────
    {
        name: 'apt', shells: ['bash'], category: 'package',
        summary: 'Debian/Ubuntu package mgmt',
        dangerous: 'with-flags',
        flags: [
            { flag: 'install', desc: 'Install package(s)' },
            { flag: 'remove',  desc: 'Remove package keeping config', destructive: true },
            { flag: 'purge',   desc: 'Remove package AND config', destructive: true },
            { flag: 'autoremove', desc: 'Remove orphan deps', destructive: true },
            { flag: 'update',  desc: 'Refresh package lists' },
            { flag: 'upgrade', desc: 'Upgrade installed packages' },
        ],
    },

    // ── PowerShell (Windows) ────────────────────────────────────────────
    {
        name: 'remove-item', aliases: ['rm', 'del', 'erase', 'rmdir', 'rd'],
        shells: ['pwsh', 'cmd'], category: 'fs',
        summary: 'Remove items (PowerShell)',
        dangerous: 'with-flags',
        flags: [
            { flag: '-Recurse', desc: 'Delete contents of folders recursively', destructive: true },
            { flag: '-Force',   desc: 'Override read-only / hidden / system', destructive: true },
            { flag: '-Confirm', desc: 'Force confirmation prompt' },
            { flag: '-WhatIf',  desc: 'Dry-run — show what would happen' },
        ],
        requireConfirm: [['-Recurse', '-Force']],
    },
    {
        name: 'stop-service', shells: ['pwsh'], category: 'service',
        summary: 'Stop a Windows service',
        dangerous: 'always',
        flags: [
            { flag: '-Force', desc: 'Stop dependent services too', destructive: true },
            { flag: '-NoWait', desc: 'Return immediately' },
        ],
    },
    {
        name: 'restart-computer', shells: ['pwsh'], category: 'service',
        summary: 'Reboot the machine',
        dangerous: 'always',
        flags: [
            { flag: '-Force', desc: 'Force restart even with pending changes', destructive: true },
            { flag: '-Wait',  desc: 'Block until computer is up again' },
        ],
        requireConfirm: [[]],  // empty = always confirm
    },
    {
        name: 'get-process', aliases: ['ps'], shells: ['pwsh'], category: 'inspect',
        summary: 'List running processes',
        dangerous: 'never',
        flags: [
            { flag: '-Name', desc: 'Filter by process name' },
            { flag: '-Id',   desc: 'Filter by PID' },
        ],
    },
    {
        name: 'get-service', shells: ['pwsh'], category: 'inspect',
        summary: 'List services',
        dangerous: 'never',
        flags: [
            { flag: '-Name',         desc: 'Filter by service name' },
            { flag: '-DisplayName',  desc: 'Filter by display name' },
            { flag: '-DependentServices', desc: 'Also show services depending on this' },
        ],
    },
    {
        name: 'invoke-command', shells: ['pwsh'], category: 'net',
        summary: 'Run script block on remote host via WinRM',
        dangerous: 'with-flags',
        flags: [
            { flag: '-ComputerName', desc: 'Target host(s)' },
            { flag: '-ScriptBlock',  desc: 'Code to execute' },
            { flag: '-FilePath',     desc: 'Script file to execute' },
            { flag: '-AsJob',        desc: 'Run as background job' },
        ],
    },

    // ── Common inspect (cross-platform) ─────────────────────────────────
    {
        name: 'cat', shells: ['bash', 'pwsh'], category: 'inspect',
        summary: 'Concatenate / print file contents',
        dangerous: 'never',
        flags: [
            { flag: '-n', desc: 'Number all output lines' },
            { flag: '-A', desc: 'Show all chars (=-vET)' },
        ],
    },
    {
        name: 'tail', shells: ['bash'], category: 'inspect',
        summary: 'Last N lines of a file (or follow)',
        dangerous: 'never',
        flags: [
            { flag: '-n', desc: 'Output last N lines' },
            { flag: '-f', desc: 'Follow — keep streaming new lines' },
            { flag: '-F', desc: 'Follow + retry if rotated' },
        ],
    },
    {
        name: 'grep', shells: ['bash'], category: 'inspect',
        summary: 'Search text in files',
        dangerous: 'never',
        flags: [
            { flag: '-r',   desc: 'Recursive' },
            { flag: '-i',   desc: 'Case-insensitive' },
            { flag: '-v',   desc: 'Invert match' },
            { flag: '-n',   desc: 'Line numbers' },
            { flag: '-E',   desc: 'Extended regex' },
            { flag: '-l',   desc: 'Just filenames with match' },
        ],
    },
    {
        name: 'docker', shells: ['bash', 'pwsh'], category: 'process',
        summary: 'Container runtime',
        dangerous: 'with-flags',
        flags: [
            { flag: 'ps',     desc: 'List running containers' },
            { flag: 'logs',   desc: 'Fetch container logs' },
            { flag: 'exec',   desc: 'Run command in container' },
            { flag: 'stop',   desc: 'Stop container(s)', destructive: true },
            { flag: 'rm',     desc: 'Remove container(s)', destructive: true },
            { flag: 'rmi',    desc: 'Remove image(s)', destructive: true },
            { flag: 'system prune', desc: 'Aggressively delete unused data', destructive: true },
        ],
    },
    {
        name: 'kubectl', shells: ['bash', 'pwsh'], category: 'process',
        summary: 'Kubernetes CLI',
        dangerous: 'with-flags',
        flags: [
            { flag: 'get',      desc: 'List resources (read-only)' },
            { flag: 'describe', desc: 'Detail a resource (read-only)' },
            { flag: 'delete',   desc: 'Remove resource', destructive: true },
            { flag: 'apply',    desc: 'Create/update from manifest' },
            { flag: 'logs',     desc: 'Pod logs' },
            { flag: 'exec',     desc: 'Run in pod' },
            { flag: 'drain',    desc: 'Cordon + evict pods from node', destructive: true },
        ],
    },
]);

// ── Lookup API ────────────────────────────────────────────────────────
// Indexes built lazily on first lookup to keep import cost zero.

let _byName: Map<string, CommandSignature> | null = null;

/** Build the case-insensitive index lazily. */
function buildIndex() {
    if (_byName) return;
    _byName = new Map();
    for (const sig of SIGNATURES) {
        _byName.set(sig.name.toLowerCase(), sig);
        for (const alias of sig.aliases ?? []) {
            _byName.set(alias.toLowerCase(), sig);
        }
    }
}

/**
 * Look up a command by its first token. Returns null if unknown.
 *
 * Usage:
 *   const sig = lookupSignature('Get-ChildItem');
 *   if (sig?.dangerous === 'always') { ... }
 */
export function lookupSignature(commandFirstWord: string): CommandSignature | null {
    if (!commandFirstWord) return null;
    buildIndex();
    return _byName!.get(commandFirstWord.toLowerCase()) ?? null;
}

/**
 * Given a full command line, find the signature for the leading token and
 * report any flags the user typed.
 *
 * Naive tokenization — splits on whitespace, doesn't honor quoted args.
 * Good enough for security pre-check (we only inspect the flags, not the values).
 */
export interface CommandAnalysis {
    signature: CommandSignature | null;
    detectedFlags: string[];
    /** True if the user's flag combination matches any entry in requireConfirm */
    requiresConfirm: boolean;
    /** The specific destructive flags found, in input order */
    destructiveFlags: string[];
}

export function analyzeCommand(commandLine: string): CommandAnalysis {
    const tokens = commandLine.trim().split(/\s+/).filter(Boolean);
    const sig = lookupSignature(tokens[0] ?? '');
    if (!sig) {
        return { signature: null, detectedFlags: [], requiresConfirm: false, destructiveFlags: [] };
    }
    // Flags are tokens that start with `-` (POSIX) or `/` (Windows CMD), or
    // PowerShell parameter names starting with `-`. For PowerShell we match
    // case-insensitively because users type `-recurse` and `-Recurse` interchangeably.
    const detectedFlags = tokens.slice(1).filter(t => /^[-\/]/.test(t));

    const destructiveFlags = sig.flags
        .filter(f => f.destructive)
        .filter(f => detectedFlags.some(d => d.toLowerCase() === f.flag.toLowerCase()))
        .map(f => f.flag);

    const requiresConfirm = sig.requireConfirm?.some(combo => {
        if (combo.length === 0) return true; // empty combo = always require confirm
        return combo.every(needed =>
            detectedFlags.some(d => d.toLowerCase() === needed.toLowerCase())
        );
    }) ?? false;

    return { signature: sig, detectedFlags, requiresConfirm, destructiveFlags };
}
