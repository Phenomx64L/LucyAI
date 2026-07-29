// ── script_verify.rs — Pre-delivery script syntax verification (v1.7.16) ─
//
// When Lucy emits a code block in her response, the frontend can call
// this module to validate the syntax BEFORE showing the code to the
// user. If a syntax error is detected, the frontend then asks the
// CHEAP LLM tier to fix the script, re-verifies, and only delivers
// the clean version with a `✓ Verified` badge.
//
// This catches the most common error class — typos, missing brackets,
// imports — at zero cost (purely lint-time, no execution). Logical
// bugs (wrong path, missing module) are NOT caught; that needs the
// dry-run sandbox (future v1.7.17+).
//
// ── Supported languages ────────────────────────────────────────────────
//
//   powershell / ps1 — PowerShell 5/7 Parser via pwsh -NoProfile.
//                       Detects syntax errors with line numbers.
//   node / javascript / js — `node --check` (fast, well-tested).
//   python / py    — `python -m py_compile`.
//   bash / sh      — `bash -n` (syntax check only).
//   json           — In-process `serde_json::from_str`. No external
//                    process, ~microseconds.
//   yaml / yml     — In-process via `serde_yaml`-equivalent: we try
//                    a permissive subset because adding a YAML crate
//                    would inflate the binary; skipped for now.
//
// All external processes are spawned with 5-second timeout to prevent
// a hung interpreter from blocking the UI. Failures surface as
// `VerifyResult::ok = false` with the error message; the frontend then
// decides whether to attempt an auto-fix.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::time::Duration;

const PROC_TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResult {
    /// `true` when the parser/syntax checker accepted the content.
    pub ok:        bool,
    /// The language we actually checked (normalised — e.g. `js` → `javascript`).
    pub language:  String,
    /// Combined stderr/stdout from the checker when `ok == false`.
    /// Truncated to 4 KB so a runaway error doesn't bloat the response.
    pub error:     Option<String>,
    /// First-error line number when the checker could extract one.
    /// Best-effort; not all checkers report line numbers consistently.
    pub line:      Option<u32>,
    /// Wall time of the check in milliseconds.
    pub elapsed_ms: u64,
    /// `true` when we skipped the check (unsupported language, content
    /// too short, etc). Frontend treats this as "delivered as-is, no
    /// verification badge".
    pub skipped:   bool,
    /// Reason for skipping when `skipped == true`.
    pub skip_reason: Option<String>,
}

impl VerifyResult {
    fn ok(language: &str, elapsed_ms: u64) -> Self {
        Self { ok: true, language: language.into(), error: None, line: None,
               elapsed_ms, skipped: false, skip_reason: None }
    }
    fn err(language: &str, error: String, line: Option<u32>, elapsed_ms: u64) -> Self {
        Self { ok: false, language: language.into(),
               error: Some(truncate(error, 4096)), line,
               elapsed_ms, skipped: false, skip_reason: None }
    }
    fn skipped(language: &str, reason: &str) -> Self {
        Self { ok: true, language: language.into(), error: None, line: None,
               elapsed_ms: 0, skipped: true, skip_reason: Some(reason.into()) }
    }
}

fn truncate(s: String, max: usize) -> String {
    if s.len() <= max { return s; }
    let mut t = s.chars().take(max).collect::<String>();
    t.push_str("\n…(truncated)");
    t
}

/// Normalise a language label to the form we dispatch on. Returns
/// `None` for unknown languages so the caller can decide to skip.
fn normalise_lang(raw: &str) -> Option<&'static str> {
    match raw.trim().to_lowercase().as_str() {
        "powershell" | "ps1" | "pwsh" | "ps" => Some("powershell"),
        "javascript" | "js" | "node" | "nodejs" => Some("javascript"),
        "python" | "py" | "python3" => Some("python"),
        "bash" | "sh" | "shell" => Some("bash"),
        "json" => Some("json"),
        _ => None,
    }
}

// ── PowerShell ──────────────────────────────────────────────────────────
//
// Uses the .NET Parser via pwsh/powershell.exe. We write the content to
// a temp file (rather than passing it as a command-line argument) to
// avoid shell escaping headaches and to handle multi-line scripts
// cleanly. The wrapper Parser script returns 0 on clean parse, 1 + line
// number on syntax error.

fn verify_powershell(content: &str) -> VerifyResult {
    let t0 = std::time::Instant::now();
    let tmp = match write_tempfile("lucy_verify_ps_", ".ps1", content) {
        Ok(p) => p,
        Err(e) => return VerifyResult::err("powershell",
            format!("tempfile create: {}", e), None, t0.elapsed().as_millis() as u64),
    };
    // Wrapper script: parse the content and surface any error with line.
    let wrapper = format!(r#"
        $errors = @()
        $tokens = @()
        $content = [System.IO.File]::ReadAllText('{}')
        [void][System.Management.Automation.Language.Parser]::ParseInput($content, [ref]$tokens, [ref]$errors)
        if ($errors.Count -gt 0) {{
            $e = $errors[0]
            Write-Error ("LINE {{0}}: {{1}}" -f $e.Extent.StartLineNumber, $e.Message)
            exit 1
        }}
    "#, tmp.replace('\'', "''"));
    // Try pwsh (PowerShell 7) first, fall back to Windows PowerShell 5.
    let exe = if which("pwsh") { "pwsh" } else { "powershell" };
    // UTF-8 forced: what this returns is a PowerShell PARSER error, and those
    // are localised — "Falta el paréntesis de cierre…". Shown to the user with
    // the accents replaced by U+FFFD it reads like a second, unrelated fault.
    // pwsh 7 already defaults to UTF-8; Windows PowerShell 5 does not, and the
    // fallback is the common case.
    let out = std::process::Command::new(exe)
        .arg("-NoProfile").arg("-NonInteractive").arg("-Command")
        .arg(crate::utils::shell::ps_utf8(&wrapper))
        .output();
    let _ = std::fs::remove_file(&tmp);
    let ms = t0.elapsed().as_millis() as u64;
    match out {
        Ok(out) if out.status.success() => VerifyResult::ok("powershell", ms),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let combined = if stderr.is_empty() { stdout } else { stderr };
            let line = parse_line_from(&combined);
            VerifyResult::err("powershell", combined, line, ms)
        },
        Err(e) => VerifyResult::err("powershell",
            format!("pwsh/powershell invoke failed: {}", e), None, ms),
    }
}

// ── Node / JavaScript ──────────────────────────────────────────────────
//
// `node --check` is the canonical syntax-only validator. Exits 0 on
// clean parse, non-zero with stderr containing `file:line` on error.

fn verify_javascript(content: &str) -> VerifyResult {
    run_external_check(content, "lucy_verify_js_", ".js", "javascript",
                       "node", &["--check"])
}

// ── Python ─────────────────────────────────────────────────────────────

fn verify_python(content: &str) -> VerifyResult {
    run_external_check(content, "lucy_verify_py_", ".py", "python",
                       "python", &["-m", "py_compile"])
}

// ── Bash ───────────────────────────────────────────────────────────────

fn verify_bash(content: &str) -> VerifyResult {
    // bash -n <file> exits 0 on clean parse, prints error to stderr
    // otherwise. On Windows this needs WSL or Git-Bash; if neither is
    // available, we skip rather than fail (bash is opt-in for users
    // who actually have it).
    if !which("bash") {
        return VerifyResult::skipped("bash", "bash not on PATH");
    }
    run_external_check(content, "lucy_verify_sh_", ".sh", "bash",
                       "bash", &["-n"])
}

// ── JSON ───────────────────────────────────────────────────────────────
//
// Pure in-process. `serde_json` reports the position; we map it to a
// line number for friendly errors.

fn verify_json(content: &str) -> VerifyResult {
    let t0 = std::time::Instant::now();
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(_)  => VerifyResult::ok("json", t0.elapsed().as_millis() as u64),
        Err(e) => {
            let line = Some(e.line() as u32);
            let msg = format!("Line {}, column {}: {}", e.line(), e.column(), e);
            VerifyResult::err("json", msg, line, t0.elapsed().as_millis() as u64)
        }
    }
}

// ── External-process helper ────────────────────────────────────────────

fn run_external_check(
    content: &str, prefix: &str, suffix: &str, lang: &str,
    cmd: &str, args: &[&str],
) -> VerifyResult {
    let t0 = std::time::Instant::now();
    if !which(cmd) {
        return VerifyResult::skipped(lang, &format!("{} not on PATH", cmd));
    }
    let tmp = match write_tempfile(prefix, suffix, content) {
        Ok(p) => p,
        Err(e) => return VerifyResult::err(lang,
            format!("tempfile create: {}", e), None, t0.elapsed().as_millis() as u64),
    };
    let mut all_args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    all_args.push(tmp.clone());
    let out = std::process::Command::new(cmd)
        .args(&all_args)
        .output();
    let _ = std::fs::remove_file(&tmp);
    let ms = t0.elapsed().as_millis() as u64;
    match out {
        Ok(out) if out.status.success() => VerifyResult::ok(lang, ms),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let combined = if stderr.is_empty() { stdout } else { stderr };
            let line = parse_line_from(&combined);
            VerifyResult::err(lang, combined, line, ms)
        },
        Err(e) => VerifyResult::err(lang,
            format!("{} invoke failed: {}", cmd, e), None, ms),
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn write_tempfile(prefix: &str, suffix: &str, content: &str) -> std::io::Result<String> {
    let dir = std::env::temp_dir();
    let unique = format!("{}{}_{}", prefix,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos()).unwrap_or(0));
    let path = dir.join(format!("{}{}", unique, suffix));
    let mut f = std::fs::File::create(&path)?;
    f.write_all(content.as_bytes())?;
    Ok(path.to_string_lossy().to_string())
}

fn which(cmd: &str) -> bool {
    // Cheap PATH probe — on Windows tries .exe / .cmd extensions.
    let path = match std::env::var_os("PATH") { Some(p) => p, None => return false };
    let exts: Vec<&str> = if cfg!(windows) { vec!["", ".exe", ".cmd", ".bat"] } else { vec![""] };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let p = dir.join(format!("{}{}", cmd, ext));
            if p.is_file() { return true; }
        }
    }
    false
}

fn parse_line_from(msg: &str) -> Option<u32> {
    // Try several common formats:
    //   "LINE 42: ..."          (our PS wrapper)
    //   "file.js:42"             (node)
    //   "File \"x.py\", line 42" (python)
    //   ":42:" or " line 42"     (generic)
    let patterns = [
        regex_lite("LINE (\\d+)"),
        regex_lite(":(\\d+):"),
        regex_lite("line (\\d+)"),
    ];
    for re in patterns.iter().flatten() {
        if let Some(c) = re.captures(msg) {
            if let Some(m) = c.get(1) {
                if let Ok(n) = m.as_str().parse::<u32>() { return Some(n); }
            }
        }
    }
    None
}

fn regex_lite(pat: &str) -> Option<regex::Regex> {
    regex::Regex::new(pat).ok()
}

// ── Tauri command ──────────────────────────────────────────────────────

/// Verify the syntax of a code block. Returns a `VerifyResult` whose
/// `ok` field tells the caller whether to deliver the script as-is or
/// kick off an auto-fix loop. Always Ok — internal failures surface
/// as `ok=false` with a descriptive error.
#[tauri::command]
pub async fn verify_script(language: String, content: String) -> Result<VerifyResult, String> {
    // Quick gates: content too short / language unsupported.
    if content.trim().len() < 8 {
        return Ok(VerifyResult::skipped(&language, "content too short to verify"));
    }
    let lang = match normalise_lang(&language) {
        Some(l) => l,
        None => return Ok(VerifyResult::skipped(&language, "language not supported by verifier")),
    };
    // Dispatch. We wrap in tokio::task::spawn_blocking so the
    // external-process work doesn't block the async runtime.
    let content_owned = content.clone();
    let lang_label = lang.to_string();
    let res = tokio::time::timeout(
        Duration::from_secs(PROC_TIMEOUT_SECS + 1),
        tokio::task::spawn_blocking(move || {
            match lang {
                "powershell" => verify_powershell(&content_owned),
                "javascript" => verify_javascript(&content_owned),
                "python"     => verify_python(&content_owned),
                "bash"       => verify_bash(&content_owned),
                "json"       => verify_json(&content_owned),
                _            => VerifyResult::skipped(lang, "internal: unmapped after normalise"),
            }
        }),
    ).await;
    match res {
        Ok(Ok(vr)) => Ok(vr),
        Ok(Err(e)) => Ok(VerifyResult::err(&lang_label,
            format!("spawn_blocking failed: {}", e), None, 0)),
        Err(_) => Ok(VerifyResult::err(&lang_label,
            format!("verify timed out after {}s", PROC_TIMEOUT_SECS + 1), None,
            (PROC_TIMEOUT_SECS + 1) * 1000)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_clean() {
        let r = verify_json(r#"{"a": 1, "b": [2, 3]}"#);
        assert!(r.ok, "should be ok: {:?}", r);
    }

    #[test]
    fn json_bad() {
        let r = verify_json(r#"{"a": 1, "b": [2, 3,}"#);
        assert!(!r.ok);
        assert!(r.line.is_some(), "should extract line: {:?}", r);
    }

    #[test]
    fn json_empty() {
        let r = verify_json("");
        assert!(!r.ok);   // serde_json rejects empty input
    }

    #[test]
    fn normalise_powershell_aliases() {
        assert_eq!(normalise_lang("powershell"), Some("powershell"));
        assert_eq!(normalise_lang("ps1"),         Some("powershell"));
        assert_eq!(normalise_lang("PWSH"),        Some("powershell"));
        assert_eq!(normalise_lang(" Ps "),        Some("powershell"));
    }

    #[test]
    fn normalise_javascript_aliases() {
        assert_eq!(normalise_lang("javascript"), Some("javascript"));
        assert_eq!(normalise_lang("js"),         Some("javascript"));
        assert_eq!(normalise_lang("node"),       Some("javascript"));
    }

    #[test]
    fn normalise_unknown_returns_none() {
        assert_eq!(normalise_lang("rust"),       None);
        assert_eq!(normalise_lang("zsh"),        None);
    }

    #[test]
    fn parse_line_extracts_from_common_formats() {
        assert_eq!(parse_line_from("LINE 42: oops"), Some(42));
        assert_eq!(parse_line_from("file.js:17:5"),  Some(17));
        assert_eq!(parse_line_from("at line 8"),     Some(8));
        assert_eq!(parse_line_from("no number here"), None);
    }

    #[test]
    fn truncate_long_string() {
        let s = "x".repeat(5000);
        let t = truncate(s, 100);
        assert!(t.len() < 200);
        assert!(t.ends_with("…(truncated)"));
    }
}
