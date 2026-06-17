// ── reflection.rs — Pre-emission safety gate ────────────────────────────────
//
// PURPOSE: Deterministic pattern classifier that runs AFTER the LLM response
// is complete but BEFORE it's emitted to the WebView for execution. Zero LLM
// calls — pure regex + working memory comparison. Sub-millisecond latency.
//
// THREAT MODEL:
//   • LLM emits destructive command without PLAN wrapper → ESCALATE
//   • LLM hallucinates a file path that doesn't match CWD/project → SANITIZE
//   • LLM contradicts its own recent execution evidence → WARN
//   • LLM attempts to write to sensitive system paths → BLOCK
//
// INTEGRATION POINT: Called from the frontend (via Tauri command) after
// ask_lucy_stream returns but before the response is processed by the agent
// loop. Lightweight enough to call on every response.

use regex::Regex;
use serde::{Deserialize, Serialize};
use once_cell::sync::Lazy;

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReflectionVerdict {
    /// Response is safe — emit to frontend as-is
    Pass,
    /// Response contains concerning patterns — emit with warning banner
    Warn { reasons: Vec<String> },
    /// Response contains dangerous patterns without safeguards — block and ask user
    Escalate { risk: RiskLevel, reasons: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskLevel {
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct WorkingMemorySnapshot {
    /// Current working directory for path validation
    pub current_cwd: Option<String>,
    /// Recently accessed/created paths (for hallucination detection)
    pub recent_paths: Vec<String>,
    /// Last N command outputs (for contradiction detection)
    pub last_outputs: Vec<String>,
    /// Last N commands executed (for loop detection)
    pub last_commands: Vec<String>,
}


// ── Compiled patterns (static, compiled once) ────────────────────────────────

/// Destructive commands that MUST be wrapped in <PLAN> tags
static DESTRUCTIVE_NO_PLAN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)<EXECUTE[^>]*>\s*(Remove-Item|rm\s+-rf|format\s+[A-Z]:|del\s+/[fsq]|Stop-Service|Restart-Service|Restart-Computer|taskkill\s+/f|kill\s+-9|DROP\s+TABLE|DELETE\s+FROM|Disable-NetAdapter|Reset-|sc\s+delete|netsh.*reset|iptables\s+-F|systemctl\s+(stop|disable|mask)|mkfs|fdisk|dd\s+if=)").unwrap()
});

/// Sensitive system paths that should never be written to by the agent
static SENSITIVE_WRITE_PATHS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)<TOOL>(?:writefile|editfile):(C:\\Windows\\System32|C:\\Windows\\SysWOW64|/etc/passwd|/etc/shadow|/etc/sudoers|C:\\Boot|C:\\bootmgr|HKLM\\SAM|HKLM\\SECURITY)").unwrap()
});

/// Path extraction from writefile/editfile tools
static WRITE_PATH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)<TOOL>(?:writefile|editfile):([^|<\n]+)").unwrap()
});

/// Infinite loop indicators: same EXECUTE content repeated
static EXECUTE_CONTENT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"<EXECUTE[^>]*>([\s\S]*?)</EXECUTE").unwrap()
});

// ── Core analysis ────────────────────────────────────────────────────────────

/// Analyze an LLM response for safety concerns.
/// Returns a verdict indicating whether to pass, warn, or escalate.
pub fn analyze_response(
    response: &str,
    working_memory: Option<&WorkingMemorySnapshot>,
) -> ReflectionVerdict {
    let mut reasons: Vec<String> = Vec::new();
    let mut max_risk = None;

    // ── Check 1: Destructive command without PLAN wrapper ─────────────────
    if DESTRUCTIVE_NO_PLAN.is_match(response) && !response.contains("<PLAN") {
        reasons.push("Comando destructivo sin <PLAN> wrapper detectado".into());
        max_risk = Some(RiskLevel::High);
    }

    // ── Check 2: Writing to sensitive system paths ────────────────────────
    if SENSITIVE_WRITE_PATHS.is_match(response) {
        reasons.push("Intento de escritura a path de sistema sensible".into());
        max_risk = Some(RiskLevel::Critical);
    }

    // ── Check 3: Path hallucination detection ────────────────────────────
    if let Some(wm) = working_memory {
        if let Some(cwd) = &wm.current_cwd {
            for caps in WRITE_PATH_RE.captures_iter(response) {
                let target = caps.get(1).map_or("", |m| m.as_str()).trim();
                if !target.is_empty() && !path_is_plausible(target, cwd, &wm.recent_paths) {
                    reasons.push(format!(
                        "Path sospechoso: '{}' no coincide con CWD ({}) ni paths recientes",
                        &target[..target.len().min(80)], cwd
                    ));
                    if max_risk.is_none() {
                        max_risk = Some(RiskLevel::Medium);
                    }
                }
            }
        }
    }

    // ── Check 4: State contradiction ─────────────────────────────────────
    if let Some(wm) = working_memory {
        if let Some(contradiction) = detect_contradiction(response, wm) {
            reasons.push(contradiction);
            if max_risk.is_none() {
                max_risk = Some(RiskLevel::Medium);
            }
        }
    }

    // ── Check 5: Agent loop detection ────────────────────────────────────
    if let Some(wm) = working_memory {
        if is_repeating_command(response, &wm.last_commands) {
            reasons.push("Comando repetido detectado — posible loop de agente".into());
            if max_risk.is_none() {
                max_risk = Some(RiskLevel::Medium);
            }
        }
    }

    // ── Verdict ──────────────────────────────────────────────────────────
    match max_risk {
        None => ReflectionVerdict::Pass,
        Some(RiskLevel::Medium) => ReflectionVerdict::Warn { reasons },
        Some(risk) => ReflectionVerdict::Escalate { risk, reasons },
    }
}

// ── Helper functions ─────────────────────────────────────────────────────────

/// Check if a target path is plausible given the current context.
/// A path is plausible if:
///   - It starts with the CWD (or is relative)
///   - It matches a recently accessed path
///   - It's in a common project directory (Desktop, Documents, repos)
fn path_is_plausible(target: &str, cwd: &str, recent_paths: &[String]) -> bool {
    let t = target.to_lowercase().replace('/', "\\");
    let c = cwd.to_lowercase().replace('/', "\\");

    // Relative paths are always plausible (resolved against CWD)
    if !t.contains(':') && !t.starts_with("\\\\") {
        return true;
    }

    // Under CWD
    if t.starts_with(&c) {
        return true;
    }

    // Matches a recently accessed path (or parent thereof)
    for rp in recent_paths {
        let rp_lower = rp.to_lowercase().replace('/', "\\");
        if t.starts_with(&rp_lower) || rp_lower.starts_with(&t) {
            return true;
        }
    }

    // Common safe directories
    let safe_prefixes = [
        "c:\\users\\", "d:\\", "e:\\",
        "/home/", "/tmp/", "/var/log/",
    ];
    if safe_prefixes.iter().any(|p| t.starts_with(p)) {
        return true;
    }

    false
}

/// Detect state contradictions between LLM claims and recent evidence.
fn detect_contradiction(response: &str, wm: &WorkingMemorySnapshot) -> Option<String> {
    let resp_lower = response.to_lowercase();

    for output in wm.last_outputs.iter().rev().take(3) {
        let out_lower = output.to_lowercase();

        // Service state contradictions
        if out_lower.contains("stopped") && resp_lower.contains("the service is running") {
            return Some("LLM afirma servicio running pero última evidencia muestra Stopped".into());
        }
        if out_lower.contains("running") && resp_lower.contains("the service is stopped") {
            return Some("LLM afirma servicio stopped pero última evidencia muestra Running".into());
        }

        // File existence contradictions
        if (out_lower.contains("cannot find path") || out_lower.contains("does not exist"))
            && resp_lower.contains("file exists") {
            return Some("LLM afirma archivo existe pero última evidencia: not found".into());
        }

        // Permission contradictions
        if out_lower.contains("access denied") && resp_lower.contains("successfully") {
            return Some("LLM reporta éxito pero última evidencia: Access Denied".into());
        }
    }

    None
}

/// Check if the response is repeating a command that was already executed recently.
fn is_repeating_command(response: &str, last_commands: &[String]) -> bool {
    if last_commands.len() < 2 { return false; }

    // Extract EXECUTE content from the response
    let current_cmds: Vec<String> = EXECUTE_CONTENT_RE.captures_iter(response)
        .filter_map(|c| c.get(1).map(|m| m.as_str().trim().to_lowercase()))
        .collect();

    if current_cmds.is_empty() { return false; }

    // Check if any extracted command matches the last 2 commands
    let recent: Vec<String> = last_commands.iter().rev().take(2)
        .map(|c| c.trim().to_lowercase())
        .collect();

    current_cmds.iter().any(|cmd| recent.iter().any(|r| r == cmd))
}

// ── Tauri command ────────────────────────────────────────────────────────────

/// Analyze an LLM response for safety concerns before the frontend processes it.
/// Called from the Svelte agent loop after streaming completes.
#[tauri::command]
pub async fn reflect_on_response(
    response: String,
    current_cwd: Option<String>,
    recent_paths: Option<Vec<String>>,
    last_outputs: Option<Vec<String>>,
    last_commands: Option<Vec<String>>,
) -> Result<ReflectionVerdict, String> {
    let wm = WorkingMemorySnapshot {
        current_cwd,
        recent_paths: recent_paths.unwrap_or_default(),
        last_outputs: last_outputs.unwrap_or_default(),
        last_commands: last_commands.unwrap_or_default(),
    };

    Ok(analyze_response(&response, Some(&wm)))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_destructive_without_plan_escalates() {
        let resp = "<EXECUTE>Remove-Item C:\\Users\\data -Recurse -Force</EXECUTE>";
        let v = analyze_response(resp, None);
        assert!(matches!(v, ReflectionVerdict::Escalate { .. }));
    }

    #[test]
    fn test_destructive_with_plan_passes() {
        let resp = "<PLAN risk=\"high\"><CMD>Remove-Item C:\\tmp -Recurse</CMD></PLAN>";
        let v = analyze_response(resp, None);
        assert!(matches!(v, ReflectionVerdict::Pass));
    }

    #[test]
    fn test_sensitive_path_blocks() {
        let resp = "<TOOL>writefile:C:\\Windows\\System32\\drivers\\etc\\hosts</TOOL>";
        let v = analyze_response(resp, None);
        assert!(matches!(v, ReflectionVerdict::Escalate { risk: RiskLevel::Critical, .. }));
    }

    #[test]
    fn test_normal_response_passes() {
        let resp = "Here's how to check your CPU usage:\n<EXECUTE>Get-Process | Sort CPU -Desc | Select -First 5</EXECUTE>";
        let v = analyze_response(resp, None);
        assert!(matches!(v, ReflectionVerdict::Pass));
    }

    #[test]
    fn test_path_hallucination_warns() {
        let wm = WorkingMemorySnapshot {
            current_cwd: Some("C:\\Projects\\Lucy".into()),
            recent_paths: vec!["C:\\Projects\\Lucy\\src".into()],
            last_outputs: vec![],
            last_commands: vec![],
        };
        let resp = "<TOOL>writefile:Z:\\NonExistent\\Secret\\hack.ps1</TOOL><FILECONTENT>bad</FILECONTENT>";
        let v = analyze_response(resp, Some(&wm));
        assert!(!matches!(v, ReflectionVerdict::Pass));
    }

    #[test]
    fn test_contradiction_warns() {
        let wm = WorkingMemorySnapshot {
            current_cwd: None,
            recent_paths: vec![],
            last_outputs: vec!["Status: Stopped".into()],
            last_commands: vec![],
        };
        let resp = "The service is running normally. No action needed.";
        let v = analyze_response(resp, Some(&wm));
        assert!(matches!(v, ReflectionVerdict::Warn { .. }));
    }
}
