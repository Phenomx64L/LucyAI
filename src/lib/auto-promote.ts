// ── auto-promote.ts — detect a safe bare command worth auto-executing ────────
//
// Extracted from +page.svelte `_autoPromoteSafeCmd` (Phase-3 refactor,
// v1.7.201). This is the SECURITY-relevant core: given a model response, decide
// whether it contains a single SAFE, invocable command the model emitted
// WITHOUT an <EXECUTE_CMD> tag (e.g. Gemini Flash answering "ábrelo" with a bare
// `Start-Process …`). Returns the command to promote, or null.
//
// PURE — no side effects. The component wrapper keeps the intent gate
// (don't promote when the user only wanted the command / code / a skill) and
// the trace + tag-append. Pulling the regex logic out makes the allow/deny
// lists unit-testable, which matters because a wrong call here auto-runs a
// command.

// Allow-list of SAFE actionable command prefixes. Two families:
//   • read-only inspection (Get-*, Test-*, whoami, ipconfig, …)
//   • open / launch / navigate (Start-Process, Invoke-Item, ii, explorer,
//     start, notepad, code, Set-Location/cd) — what "ábrelo" / "abre la
//     carpeta" / "ve a esa ruta" mean.
const SAFE_CMD_RE = /^\s*(Get-[A-Za-z]+|Test-[A-Za-z]+|Measure-[A-Za-z]+|Resolve-[A-Za-z]+|Select-[A-Za-z]+|Show-[A-Za-z]+|Find-[A-Za-z]+|Format-[A-Za-z]+|Start-Process|Invoke-Item|Set-Location|Push-Location|Pop-Location|whoami|hostname|systeminfo|ipconfig|ifconfig|date|time|echo|pwd|ver|uptime|nslookup|ping|tracert|ii|explorer|start|notepad|code|cd|dir|ls|cat|type)\b/i;

// Deny guard — even an allow-listed prefix is NOT auto-promoted if the line
// carries a destructive verb, an elevation request, or a code-download/eval
// pattern. Those still REQUIRE the model to emit <EXECUTE_CMD> explicitly (and
// the Rust blocklist gates them on top of that).
// v1.7.203 — the disk-`format` guard was `\bformat\b`, which also matched the
// benign `-Format` flag and `Format-Table`/`Format-List`, so harmless read-only
// commands (`Get-Date -Format o`, `… | Format-Table`) were never auto-promoted.
// Narrowed to the genuinely destructive shapes: `Format-Volume`, or `format`
// followed by a drive letter / `/FS:` style flag. Everything else unchanged.
const DANGER_RE = /-Verb\s+RunAs|\bRemove-|\brm\s|\bdel\s|\brmdir\b|\bFormat-Volume\b|\bformat\s+(?:\/|[A-Za-z]:)|\bmkfs\b|\bdd\s|\bStop-|\bRestart-Computer\b|\bClear-|\bUninstall-|\bDisable-|\bSet-ExecutionPolicy\b|\bnet\s+user\b|\btakeown\b|\bicacls\b|Invoke-Expression|\biex\b|DownloadString|DownloadFile|-EncodedCommand/i;

// A line qualifies only if it starts with a safe verb AND looks like a real
// invocation — a hyphenated PowerShell cmdlet (Start-Process, Get-Date) or it
// carries a quote / path / flag. Skips prose that merely begins with an
// allow-listed English/Spanish word ("start by…", "date is…", "ping the…").
function looksInvocable(ln: string): boolean {
    return /^\s*[A-Za-z][A-Za-z0-9]+-[A-Za-z]/.test(ln)
        || /["'\\]|\s\/[A-Za-z]|\s-[A-Za-z]/.test(ln);
}

/**
 * Find the first SAFE, invocable command line in a model response that has NO
 * explicit execution tag. Returns the command string to auto-execute, or null
 * when nothing qualifies (already tagged, only prose, or a dangerous line).
 */
export function detectPromotableSafeCmd(text: string | null | undefined): string | null {
    if (!text) return null;
    // Already carries an execution tag → leave it to the normal path.
    if (/<EXECUTE_CMD\b|<EXECUTE\b/i.test(text)) return null;

    // Strip scaffolding tags + fence markers (keep fenced CONTENT) so a command
    // surrounded by prose / fences is still found.
    const detect = String(text)
        .replace(/<THOUGHT>[\s\S]*?<\/THOUGHT>/gi, '')
        .replace(/<REMEMBER[\s\S]*?<\/REMEMBER>/gi, '')
        .replace(/<TOOL>[\s\S]*?<\/TOOL>/gi, '')
        .replace(/<FILECONTENT>[\s\S]*?<\/FILECONTENT>/gi, '')
        .replace(/```[a-zA-Z0-9]*/g, '');

    for (const ln of detect.split('\n')) {
        const t = ln.trim();
        if (!t || t.length > 300) continue;
        if (SAFE_CMD_RE.test(t) && !DANGER_RE.test(t) && looksInvocable(t)) {
            return t;
        }
    }
    return null;
}
