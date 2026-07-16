// ── COMPLIANCE — Ejecución de checks CIS Benchmark en hosts remotos ──────────

use std::process::Command;
use std::os::windows::process::CommandExt;
use crate::state::CREATE_NO_WINDOW;
use crate::utils::shell::{ensure_trusted_host, run_winrm};

/// Ejecuta un batch de checks de compliance en un host Linux via SSH.
/// Recibe un JSON array de { id, command } y devuelve { id, stdout, stderr, exit_code }.
#[tauri::command]
pub async fn run_compliance_linux(
    host: String,
    username: String,
    checks_json: String,
    port: Option<u16>,
    key_path: Option<String>,
) -> Result<serde_json::Value, String> {
    // SECURITY v1.7.232 (Phase-2 C12): validate host/username before they reach the
    // `ssh user@host` argv token (line ~46). A leading '-' is otherwise re-parsed by
    // SSH as an option (H10 argv-injection → -oProxyCommand local exec). Every sibling
    // SSH path in hosts.rs enforces these; this compliance path had been missed.
    crate::commands::hosts::validate_host(&host)?;
    crate::commands::hosts::validate_username(&username)?;

    let checks: Vec<serde_json::Value> = serde_json::from_str(&checks_json)
        .map_err(|e| format!("JSON inválido en checks: {}", e))?;

    // Build a bash script that runs each check and outputs JSON results
    let mut script = String::from("echo '['\n");
    for (i, check) in checks.iter().enumerate() {
        let id = check["id"].as_str().unwrap_or("?");
        let cmd = check["command"].as_str().unwrap_or("echo no-command");
        // Escape single quotes in command
        let cmd_esc = cmd.replace('\'', "'\\''");
        if i > 0 { script.push_str("echo ','\n"); }
        script.push_str(&format!(
            r#"__out=$(bash -c '{}' 2>&1); __ec=$?; printf '{{"id":"{}","exit_code":%d,"stdout":"%s"}}' "$__ec" "$(echo "$__out" | head -c 1000 | tr '"\\' '..' | tr '\n' '|')"
"#,
            cmd_esc, id
        ));
    }
    script.push_str("echo ']'\n");

    let port_str = port.unwrap_or(22).to_string();
    let raw = tokio::task::spawn_blocking(move || -> Result<String, String> {
        use std::io::Write;
        let mut ssh_cmd = Command::new("ssh");
        ssh_cmd.arg("-o").arg("StrictHostKeyChecking=accept-new")
               .arg("-o").arg("BatchMode=yes")
               .arg("-o").arg("ConnectTimeout=15")
               .arg("-p").arg(&port_str);
        if let Some(ref kp) = key_path { if !kp.is_empty() { ssh_cmd.arg("-i").arg(kp); } }
        ssh_cmd.arg(format!("{}@{}", username, host)).arg("bash -s");
        let mut child = ssh_cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("SSH no disponible: {}", e))?;
        if let Some(mut s) = child.stdin.take() {
            s.write_all(script.as_bytes())
                .map_err(|e| format!("Error enviando script: {}", e))?;
        }
        let output = child.wait_with_output()
            .map_err(|e| format!("Error esperando SSH: {}", e))?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }).await
        .map_err(|e| e.to_string())??;

    serde_json::from_str(&raw)
        .map_err(|e| format!("JSON inválido de compliance: {}. Raw: {}", e, crate::utils::safe_truncate(&raw, 400)))
}

/// Ejecuta un batch de checks de compliance en un host Windows via WinRM.
#[tauri::command]
pub async fn run_compliance_windows(
    host: String,
    username: String,
    password: String,
    checks_json: String,
) -> Result<serde_json::Value, String> {
    ensure_trusted_host(&host);
    let checks: Vec<serde_json::Value> = serde_json::from_str(&checks_json)
        .map_err(|e| format!("JSON inválido en checks: {}", e))?;

    // Build PowerShell script that runs each check
    let mut ps_script = String::from("$results = @()\n");
    for check in &checks {
        let id = check["id"].as_str().unwrap_or("?");
        let cmd = check["command"].as_str().unwrap_or("echo no-command");
        let cmd_esc = cmd.replace('\'', "''");
        ps_script.push_str(&format!(
            r#"try {{ $out = Invoke-Expression '{}' 2>&1 | Out-String; $ec = $LASTEXITCODE; if($null -eq $ec){{$ec=0}} }} catch {{ $out = $_.Exception.Message; $ec = 1 }}
$results += [PSCustomObject]@{{ id='{}'; exit_code=$ec; stdout=$out.Substring(0,[Math]::Min(1000,$out.Length)) }}
"#,
            cmd_esc, id
        ));
    }
    ps_script.push_str("$results | ConvertTo-Json -Depth 3\n");

    let output = run_winrm(host, username, password, ps_script).await?;

    if !output.status.success() {
        return Err(format!("WinRM Error: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    serde_json::from_str(&raw)
        .map_err(|e| format!("JSON inválido: {}. Raw: {}", e, crate::utils::safe_truncate(&raw, 400)))
}

/// Ejecuta un batch de checks de compliance en la máquina local.
#[tauri::command]
pub async fn run_compliance_local(
    checks_json: String,
) -> Result<serde_json::Value, String> {
    let checks: Vec<serde_json::Value> = serde_json::from_str(&checks_json)
        .map_err(|e| format!("JSON inválido en checks: {}", e))?;

    let mut ps_script = String::from("$results = @()\n");
    for check in &checks {
        let id = check["id"].as_str().unwrap_or("?");
        let cmd = check["command"].as_str().unwrap_or("echo no-command");
        let cmd_esc = cmd.replace('\'', "''");
        ps_script.push_str(&format!(
            r#"try {{ $out = Invoke-Expression '{}' 2>&1 | Out-String; $ec = $LASTEXITCODE; if($null -eq $ec){{$ec=0}} }} catch {{ $out = $_.Exception.Message; $ec = 1 }}
$results += [PSCustomObject]@{{ id='{}'; exit_code=$ec; stdout=$out.Substring(0,[Math]::Min(1000,$out.Length)) }}
"#,
            cmd_esc, id
        ));
    }
    ps_script.push_str("$results | ConvertTo-Json -Depth 3\n");

    let output = tokio::task::spawn_blocking(move || {
        Command::new("powershell")
            .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
            .arg("-Command").arg(&ps_script)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    }).await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Error PowerShell: {}", e))?;

    if !output.status.success() {
        return Err(format!("Error: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    serde_json::from_str(&raw)
        .map_err(|e| format!("JSON inválido: {}. Raw: {}", e, crate::utils::safe_truncate(&raw, 400)))
}
