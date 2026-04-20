// ── LOGS — Visor de logs local y remoto ─────────────────────────────────────────

use std::process::Command;
use std::os::windows::process::CommandExt;
use std::io::{Read, Seek, SeekFrom};
use crate::state::CREATE_NO_WINDOW;

/// Lee las últimas N líneas de un archivo local de forma eficiente.
/// Lee en chunks de 64KB desde el final — nunca carga el archivo completo en RAM.
#[tauri::command]
pub fn read_log_tail(path: String, lines: usize) -> Result<Vec<String>, String> {
    let mut file = std::fs::File::open(&path)
        .map_err(|e| format!("No se pudo abrir '{}': {}", path, e))?;

    let file_size = file.metadata().map_err(|e| e.to_string())?.len();
    if file_size == 0 { return Ok(vec![]); }

    let chunk_size: u64 = 65536;
    let mut collected: Vec<String> = Vec::with_capacity(lines + 1);
    let mut pos = file_size;
    let mut remainder = String::new();

    while pos > 0 && collected.len() < lines {
        let read_size = chunk_size.min(pos);
        pos -= read_size;
        file.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;

        let mut buf = vec![0u8; read_size as usize];
        file.read_exact(&mut buf).map_err(|e| e.to_string())?;

        let chunk = String::from_utf8_lossy(&buf).to_string() + &remainder;
        let mut chunk_lines: Vec<&str> = chunk.split('\n').collect();

        if pos > 0 {
            remainder = chunk_lines.remove(0).to_string();
        } else {
            remainder.clear();
        }

        for line in chunk_lines.into_iter().rev() {
            collected.push(line.trim_end_matches('\r').to_string());
            if collected.len() >= lines { break; }
        }
    }

    if !remainder.is_empty() && collected.len() < lines {
        collected.push(remainder.trim_end_matches('\r').to_string());
    }

    collected.reverse();
    Ok(collected)
}

/// Lee las últimas N líneas de un log remoto Windows via WinRM.
#[tauri::command]
pub async fn read_remote_log_windows(
    host: String,
    username: String,
    password: String,
    path: String,
    lines: usize,
) -> Result<Vec<String>, String> {
    let script = format!("Get-Content -Path '{}' -Tail {} -ErrorAction Stop", path, lines);
    let pw_esc = password.replace('\'', "''");
    let ps = format!(
        "$pass = ConvertTo-SecureString '{}' -AsPlainText -Force; \
         $cred = New-Object System.Management.Automation.PSCredential('{}', $pass); \
         Invoke-Command -ComputerName '{}' -Credential $cred -ScriptBlock {{ {} }} -ErrorAction Stop",
        pw_esc, username, host, script
    );
    let output = tokio::task::spawn_blocking(move || {
        Command::new("powershell")
            .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
            .arg("-Command").arg(&ps)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    }).await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Error WinRM: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).lines().map(String::from).collect())
    } else {
        Err(format!("WinRM Error: {}", String::from_utf8_lossy(&output.stderr)))
    }
}

/// Lee las últimas N líneas de un log remoto Linux via SSH.
#[tauri::command]
pub async fn read_remote_log_linux(
    host: String,
    username: String,
    path: String,
    lines: usize,
    port: Option<u16>,
    key_path: Option<String>,
) -> Result<Vec<String>, String> {
    let port_str = port.unwrap_or(22).to_string();
    let cmd = format!("tail -{} '{}'", lines, path.replace('\'', "'\\''"));
    let mut ssh_cmd = Command::new("ssh");
    ssh_cmd.arg("-o").arg("StrictHostKeyChecking=accept-new")
           .arg("-o").arg("BatchMode=yes")
           .arg("-o").arg("ConnectTimeout=10")
           .arg("-p").arg(&port_str);
    if let Some(ref kp) = key_path { if !kp.is_empty() { ssh_cmd.arg("-i").arg(kp); } }
    ssh_cmd.arg(&format!("{}@{}", username, host))
           .arg(&cmd);
    ssh_cmd.creation_flags(CREATE_NO_WINDOW);
    let output = tokio::task::spawn_blocking(move || ssh_cmd.output())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("SSH Error: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).lines().map(String::from).collect())
    } else {
        Err(format!("SSH Error: {}", String::from_utf8_lossy(&output.stderr)))
    }
}
