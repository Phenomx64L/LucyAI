// ── F5 — Sandbox-First Execution Preview (MVP) ───────────────────────────
//
// Before Lucy executes a potentially destructive command (delete, format,
// modify Registry root keys, install software, mass-modify files), this
// module produces a STATIC PREVIEW of what the command would touch:
//
//   - Filesystem: paths that would be created / modified / deleted
//   - Registry:   keys that would be written / removed
//   - Services:   services that would be started / stopped / installed
//   - Network:    domains that would be contacted
//   - Privilege:  whether elevation is required
//
// Plus, if Windows Sandbox is available on the host, it emits a `.wsb`
// configuration the user can launch with a single click to actually run
// the command in isolation. This is the OPTIONAL second tier — the static
// analysis runs everywhere and is the main value.
//
// Why a separate module from the security layer:
//   - `security.rs` decides "should this be blocked?" — binary verdict
//   - `sandbox_preview.rs` answers "what would happen?" — explanatory
//
// Both can run in parallel for the same command; they don't overlap in
// purpose. UI flow: when a destructive command is detected, show the
// preview *before* asking for HITL confirmation.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPreview {
    pub command: String,
    pub destructive: bool,
    pub destructive_reason: Option<String>,
    pub elevation_required: bool,
    pub affected_paths: Vec<String>,
    pub affected_registry_keys: Vec<String>,
    pub affected_services: Vec<String>,
    pub network_endpoints: Vec<String>,
    pub static_findings: Vec<String>,
    pub sandbox_available: bool,
    pub sandbox_wsb_path: Option<String>,
    pub risk_score: f64,    // 0-1
    pub risk_band: String,  // "safe" | "review" | "destructive"
}

/// Detect destructive patterns. Mirrors security.ts but more granular —
/// returns the specific reason instead of a yes/no.
fn classify_command(cmd: &str) -> (bool, Option<String>) {
    let lower = cmd.to_lowercase();
    let patterns: &[(&str, &str)] = &[
        ("rm -rf /",         "delete root filesystem"),
        ("rm -rf ~",         "delete home directory"),
        ("rm -rf $",         "shell-variable rm — risky if variable empty"),
        ("del /f /s /q c:\\","mass delete on C:\\"),
        ("format c:",        "format C: drive"),
        ("format c ",        "format C: drive"),
        ("diskpart",         "disk partitioning"),
        ("reg delete hkey_local_machine", "delete HKLM registry hive"),
        ("reg delete hklm",  "delete HKLM registry hive"),
        ("schtasks /delete", "delete scheduled task"),
        ("net user ",        "modify user accounts"),
        ("net localgroup administrators", "modify admin group"),
        ("icacls",           "change file ACLs"),
        ("takeown",          "change file ownership"),
        ("vssadmin delete shadows", "delete volume shadow copies"),
        ("bcdedit",          "modify boot configuration"),
        ("shutdown /r",      "system reboot"),
        ("shutdown /s",      "system shutdown"),
        ("powershell.exe -enc", "encoded PowerShell payload"),
        ("invoke-webrequest -outfile", "download to disk"),
        ("invoke-expression",          "dynamic code execution"),
        (" iex ",                       "dynamic code execution"),
        ("set-executionpolicy unrestricted", "weaken PowerShell security"),
        ("get-content -raw | invoke-expression", "fileless execution"),
    ];
    for (pat, why) in patterns {
        if lower.contains(pat) {
            return (true, Some((*why).to_string()));
        }
    }
    (false, None)
}

/// Best-effort extraction of paths mentioned in the command. Very simple:
/// looks for tokens that start with C:\\, D:\\, /, ~, %something%, or contain
/// '/' or '\\'.
fn extract_paths(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for tok in cmd.split_whitespace() {
        let stripped = tok.trim_matches(|c: char| c == ',' || c == ';' || c == '"' || c == '\'');
        let looks_like_path = stripped.len() > 2 && (
            stripped.starts_with("C:\\") || stripped.starts_with("D:\\") ||
            stripped.starts_with("E:\\") || stripped.starts_with("F:\\") ||
            stripped.starts_with('/') || stripped.starts_with('~') ||
            stripped.starts_with('%') || stripped.contains("\\") ||
            stripped.contains("/")
        );
        if looks_like_path && stripped.len() < 300 && seen.insert(stripped.to_string()) {
            out.push(stripped.to_string());
            if out.len() >= 20 { break; }
        }
    }
    out
}

fn extract_registry_keys(cmd: &str) -> Vec<String> {
    let lower = cmd.to_lowercase();
    let prefixes = ["hklm\\", "hkcu\\", "hkey_local_machine\\", "hkey_current_user\\", "hkey_classes_root\\", "hkcr\\", "hku\\"];
    let mut out = Vec::new();
    for tok in lower.split_whitespace() {
        let t = tok.trim_matches(|c: char| c == ',' || c == ';' || c == '"');
        if prefixes.iter().any(|p| t.starts_with(p)) {
            out.push(t.to_string());
        }
    }
    out
}

fn extract_services(cmd: &str) -> Vec<String> {
    let lower = cmd.to_lowercase();
    let triggers = ["sc start ", "sc stop ", "sc create ", "sc delete ",
                    "net start ", "net stop ", "start-service ", "stop-service "];
    let mut out = Vec::new();
    for trigger in triggers.iter() {
        if let Some(idx) = lower.find(trigger) {
            let after = &cmd[idx + trigger.len()..];
            let name: String = after.chars().take_while(|c| !c.is_whitespace() && *c != '"').collect();
            if !name.is_empty() && name.len() < 80 {
                out.push(format!("{}{}", trigger.trim(), if name.is_empty() {""} else {" "}) + &name);
            }
        }
    }
    out
}

fn needs_elevation(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    let triggers = ["sudo ", "runas ", "elevation", "/elevate",
                    "reg delete hklm", "reg add hklm", "icacls", "takeown",
                    "sc create", "sc delete", "net localgroup", "net user",
                    "shutdown /", "bcdedit", "diskpart", "format ",
                    "set-executionpolicy", "new-service", "remove-service"];
    triggers.iter().any(|t| lower.contains(t))
}

fn extract_network(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = cmd.to_lowercase();
    // Naive URL extraction
    for tok in lower.split_whitespace() {
        let t = tok.trim_matches(|c: char| c == ',' || c == ';' || c == '"' || c == ')');
        if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("ftp://") {
            // Pull host
            if let Some(without_scheme) = t.split("://").nth(1) {
                let host: String = without_scheme.chars().take_while(|c| *c != '/' && *c != ':').collect();
                if !host.is_empty() && !out.contains(&host) {
                    out.push(host);
                }
            }
        }
    }
    out
}

/// Detect whether Windows Sandbox is available on this host. Cheap check:
/// look for `WindowsSandbox.exe` in the standard System32 path.
fn windows_sandbox_available() -> bool {
    let candidates = [
        "C:\\Windows\\System32\\WindowsSandbox.exe",
        "C:\\Windows\\Sysnative\\WindowsSandbox.exe",
    ];
    candidates.iter().any(|p| std::path::Path::new(p).exists())
}

/// Generate a .wsb (Windows Sandbox configuration) file that will run
/// the command inside the sandbox. Returns the file path. The sandbox
/// must be launched manually by the user via Explorer / double-click.
fn write_wsb_config(cmd: &str) -> Result<String, String> {
    let mut path: PathBuf = std::env::temp_dir();
    path.push(format!("lucy-sandbox-{}.wsb", std::process::id()));

    // Escape the command for XML CData (no ]]> sequences)
    let safe_cmd = cmd.replace("]]>", "]]&gt;");

    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration>
    <Networking>Enable</Networking>
    <PrinterRedirection>Disable</PrinterRedirection>
    <ClipboardRedirection>Disable</ClipboardRedirection>
    <AudioInput>Disable</AudioInput>
    <VideoInput>Disable</VideoInput>
    <LogonCommand>
        <Command>cmd.exe /c "{}"</Command>
    </LogonCommand>
</Configuration>"#,
        safe_cmd
    );

    std::fs::write(&path, content).map_err(|e| format!("write .wsb: {}", e))?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn sandbox_preview_command(command: String) -> Result<SandboxPreview, String> {
    let (destructive, reason) = classify_command(&command);
    let affected_paths = extract_paths(&command);
    let affected_registry_keys = extract_registry_keys(&command);
    let affected_services = extract_services(&command);
    let network_endpoints = extract_network(&command);
    let elevation = needs_elevation(&command);

    let mut findings: Vec<String> = Vec::new();
    if destructive {
        findings.push(format!("DESTRUCTIVE: {}", reason.clone().unwrap_or_else(|| "matched destructive pattern".to_string())));
    }
    if elevation {
        findings.push("Requires elevation (admin / sudo)".to_string());
    }
    if !affected_paths.is_empty() {
        findings.push(format!("Touches {} filesystem path(s)", affected_paths.len()));
    }
    if !affected_registry_keys.is_empty() {
        findings.push(format!("Modifies {} registry key(s)", affected_registry_keys.len()));
    }
    if !affected_services.is_empty() {
        findings.push(format!("Touches {} service(s)", affected_services.len()));
    }
    if !network_endpoints.is_empty() {
        findings.push(format!("Network egress to {}", network_endpoints.join(", ")));
    }

    // Risk score: weighted sum of signals (each capped at 1.0)
    let risk_score = (
        (if destructive { 0.55 } else { 0.0 }) +
        (if elevation { 0.20 } else { 0.0 }) +
        ((affected_paths.len() as f64 / 5.0).min(0.10)) +
        ((affected_registry_keys.len() as f64 / 3.0).min(0.10)) +
        ((affected_services.len() as f64 / 3.0).min(0.05))
    ).min(1.0);

    let risk_band = if risk_score >= 0.55 { "destructive" }
                    else if risk_score >= 0.20 { "review" }
                    else { "safe" };

    let sandbox_available = windows_sandbox_available();
    let sandbox_wsb_path = if sandbox_available && destructive {
        // Only write the .wsb when the command is destructive — no need to
        // litter %TEMP% otherwise.
        write_wsb_config(&command).ok()
    } else {
        None
    };

    Ok(SandboxPreview {
        command,
        destructive,
        destructive_reason: reason,
        elevation_required: elevation,
        affected_paths,
        affected_registry_keys,
        affected_services,
        network_endpoints,
        static_findings: findings,
        sandbox_available,
        sandbox_wsb_path,
        risk_score,
        risk_band: risk_band.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rm_rf_root() {
        let (d, r) = classify_command("rm -rf /");
        assert!(d);
        assert!(r.is_some());
    }

    #[test]
    fn detects_encoded_powershell() {
        let (d, _) = classify_command("powershell.exe -enc JABjAGwA");
        assert!(d);
    }

    #[test]
    fn benign_command_passes() {
        let (d, _) = classify_command("git status");
        assert!(!d);
    }

    #[test]
    fn extracts_paths_from_rm() {
        let p = extract_paths("rm -rf C:\\Users\\u\\Downloads\\old");
        assert!(p.iter().any(|x| x.contains("Downloads")));
    }

    #[test]
    fn extracts_registry_keys() {
        let k = extract_registry_keys("reg add HKLM\\Software\\Foo /v bar /d 1");
        assert_eq!(k.len(), 1);
        assert!(k[0].starts_with("hklm"));
    }

    #[test]
    fn elevation_detected_for_sc_create() {
        assert!(needs_elevation("sc create svc binPath=foo"));
        assert!(!needs_elevation("git status"));
    }

    #[test]
    fn extracts_https_host() {
        let n = extract_network("curl https://example.com/file -o out");
        assert_eq!(n, vec!["example.com".to_string()]);
    }

    #[test]
    fn risk_band_safe_for_git_status() {
        // We can't await in a sync test without tokio runtime; verify
        // via direct calls to component helpers instead.
        let (d, _) = classify_command("git status");
        assert!(!d);
        assert!(extract_paths("git status").len() <= 1);
    }
}
