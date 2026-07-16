// ── security — destructive command detection (frontend gate) ────────────────
//
// The agent loop and the direct-execution path both need to recognise commands
// that would mutate the system irreversibly (delete files, stop services,
// reformat disks, kill critical processes, etc).
//
// Strategy:
//   1. Normalize the input to defeat trivial obfuscation (backticks, string
//      concatenation, environment variable expansion, Unicode normalization).
//   2. Match the normalized form against a curated regex of known-destructive
//      patterns.
//
// IMPORTANT: this is a *frontend* helper. It NEVER replaces backend permission
// rules or the OS-level keychain — it's a UX nudge that makes Lucy ask before
// executing. The real enforcement lives in `src-tauri/src/commands/shell.rs`
// and the permission rule database.

/**
 * Defeat common obfuscation tricks before pattern matching.
 * Specifically:
 *   - PowerShell backtick line-escapes:   `n  → n
 *   - cmd caret line-escapes:             ^n  → n
 *   - String concatenation:               'a'+'b'  → 'ab' (up to 6 levels deep)
 *   - %WINDIR%, $env:WINDIR style env var expansion
 *   - NFKC Unicode normalization (collapses fullwidth/look-alike chars)
 */
export function normalizeCmd(cmd: string): string {
    let s = String(cmd || '');
    s = s.replace(/`([^\r\n])/g, '$1');                          // PS backtick
    s = s.replace(/\^([^\r\n])/g, '$1');                         // cmd caret
    for (let i = 0; i < 6; i++) {                                // 'a'+'b' → 'ab'
        const before = s;
        s = s.replace(/(['"])([^'"`]*)\1\s*\+\s*(['"])([^'"`]*)\3/g,
            (_m, q1, a, _q2, b) => `${q1}${a}${b}${q1}`);
        if (s === before) break;
    }
    const envMap: Record<string, string> = {
        systemroot:        'C:\\Windows',
        windir:            'C:\\Windows',
        systemdrive:       'C:',
        programfiles:      'C:\\Program Files',
        'programfiles(x86)':'C:\\Program Files (x86)',
        programdata:       'C:\\ProgramData',
        temp:              'C:\\Windows\\Temp',
        tmp:               'C:\\Windows\\Temp',
    };
    s = s.replace(/%([A-Za-z_][A-Za-z0-9_()]*)%/g,
        (_m, n) => envMap[n.toLowerCase()] || `%${n}%`);
    s = s.replace(/\$\{?env:([A-Za-z_][A-Za-z0-9_()]*)\}?/gi,
        (_m, n) => envMap[n.toLowerCase()] || `$env:${n}`);
    try { s = s.normalize('NFKC'); } catch { /* runtime without ICU */ }
    return s;
}

/**
 * Curated regex of commands that mutate the system in ways the user should
 * confirm before executing. Order doesn't matter — alternatives are independent.
 */
// v1.7.211 — added the Windows cmd delete verbs (del / erase / rmdir / rd /s),
// shadow-copy deletion (vssadmin delete — the classic ransomware/anti-recovery
// move), disk wipe (Clear-Disk / diskpart), and secure free-space wipe
// (cipher /w). These were a real gap: the agent's exec dispatch in +page.svelte
// auto-runs anything isDestructiveCmd() returns false for, so `del /f /s /q C:\…`
// or `rd /s /q` were executing WITHOUT the confirmation modal. Backslashes that
// precede a slash flag are escaped (\/) for clarity.
// v1.7.232 phase-1 security review (C3) — added remote-code-fetch / download
// verbs. "Download a script and run it" (iwr/Invoke-WebRequest/Invoke-RestMethod
// -OutFile then run; iex (New-Object Net.WebClient).DownloadString(...);
// certutil -urlcache; Start-BitsTransfer/bitsadmin; curl/wget -o) is a classic
// RCE staging primitive. The download-to-disk verbs survived BOTH the old
// frontend deny-list AND the backend obfstr blocklist, so an LLM-emitted fetch-
// exec command auto-ran with no confirmation. These now route through HITL. (The
// "ask once too many" doctrine above accepts a confirm on a benign web GET.)
export const DESTRUCTIVE_RE = /(?:netsh\s+interface|Set-NetAdapter|Remove-|Stop-Service|Restart-Service|Disable-|Set-Service|Set-ItemProperty|Invoke-WmiMethod|Uninstall-\w+|Reset-\w+|Disable-NetAdapter|reg\s+(?:delete|add)\b|net\s+(?:stop|user|group|localgroup)|Clear-EventLog|wevtutil\s+(?:cl|clear-log)\b|Restart-Computer|Stop-Computer|Enable-PSRemoting|Set-ExecutionPolicy|Format-Volume|Initialize-Disk|Clear-Disk|(?:C:\\Windows\\System32|System32\\\\?)|\bshutdown\b|\breboot\b|\bsc\s+(?:delete|stop|config)\b|\btaskkill\b|\bkill\s+-9\b|\brm\s+-rf\b|\bdd\s+if=|\bmkfs|\bfdisk\b|\bformat\s+[A-Z]:|\bsystemctl\s+(?:stop|disable|mask|reset)\b|\biptables\s+-F\b|\b(?:del|erase)\s|\brmdir\b|\brd\s+\/|vssadmin\s+delete|\bdiskpart\b|\bcipher\s+\/w|Invoke-WebRequest|Invoke-RestMethod|\biwr\b|\birm\b|\bcurl\b|\bwget\b|DownloadString|DownloadFile|DownloadData|Net\.WebClient|Invoke-Expression|\biex\b|Start-BitsTransfer|\bbitsadmin\b|certutil[^\n]*-urlcache)/i;

/**
 * True if `cmd` looks destructive after normalization. Use this to gate
 * auto-execution and prompt the user for confirmation.
 *
 * NOTE: false positives are intentional — better to ask once too many than
 * to silently nuke the user's machine.
 */
export function isDestructiveCmd(cmd: string): boolean {
    if (!cmd) return false;
    return DESTRUCTIVE_RE.test(cmd) || DESTRUCTIVE_RE.test(normalizeCmd(cmd));
}
