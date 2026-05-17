// ── SHELL UTILS — Utilidades de bajo nivel para procesos y validación ─────────

use std::process::{Command, Stdio};
use std::io::Write;
use std::os::windows::process::CommandExt;
use crate::state::CREATE_NO_WINDOW;

/// Elimina secuencias de escape ANSI/VT100 de la salida del terminal.
/// Necesario cuando SSH asigna un pseudo-TTY (-tt) y el host devuelve
/// códigos de color, movimiento de cursor, etc.
pub fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            i += 1;
            match bytes[i] {
                b'[' => {
                    // CSI sequence: ESC [ ... letter
                    i += 1;
                    while i < bytes.len() && !bytes[i].is_ascii_alphabetic() { i += 1; }
                    if i < bytes.len() { i += 1; }
                }
                b']' => {
                    // OSC sequence: ESC ] ... BEL or ESC '\'
                    i += 1;
                    while i < bytes.len() && bytes[i] != 0x07 && bytes[i] != 0x1b { i += 1; }
                    if i < bytes.len() { i += 1; }
                }
                _ => { i += 1; }
            }
        } else if bytes[i] == b'\r' {
            i += 1; // Ignorar CR (común en PTY output)
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Agrega un host a TrustedHosts de WinRM (mejor-esfuerzo, requiere admin local).
/// Necesario para conectar por IP en lugar de nombre de dominio.
/// Si falla no interrumpe la operación — el error de WinRM se verá después.
pub fn ensure_trusted_host(host: &str) {
    let cmd = format!(
        "Set-Item WSMan:\\localhost\\Client\\TrustedHosts -Value '{}' -Force \
         -Concatenate -ErrorAction SilentlyContinue 2>$null",
        host
    );
    let _ = Command::new("powershell")
        .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
        .arg("-NonInteractive").arg("-Command").arg(&cmd)
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

/// Valida que un host_id solo contenga caracteres alfanuméricos ASCII, guion o guion bajo.
/// SECURITY: Usa is_ascii_alphanumeric() (no is_alphanumeric()) para rechazar
/// Unicode look-alikes que podrían inyectarse en las claves del Credential Manager.
pub fn validate_host_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 128 {
        return Err("host_id inválido: longitud fuera de rango (1-128)".to_string());
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(format!(
            "host_id '{}' contiene caracteres no permitidos (solo a-z ASCII, 0-9, _ y -)", id
        ));
    }
    Ok(())
}

/// Codifica una cadena para incluirla en URLs de forma simple.
/// Solo caracteres alfanuméricos, guion y guion bajo pasan sin codificar.
pub fn urlencoding_simple(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

// ── Secure WinRM invocation ──────────────────────────────────────────────────

/// Build a PowerShell wrapper script that invokes a remote ScriptBlock via WinRM.
///
/// SECURITY FIX 1 — Password not in process args:
///   The full PS script is passed via `-EncodedCommand` (base64 of the wrapper),
///   so the password never appears in `Get-Process` / Task Manager / auditing.
///
/// SECURITY FIX 2 — ScriptBlock injection prevention:
///   The user-provided script is Base64-encoded and decoded inside the
///   ScriptBlock, so `}` or `$()` in the script cannot escape the block.
///
/// SECURITY FIX 3 (audit S1 — May 2026) — Password NOT in a string literal:
///   Previously we interpolated the password into a single-quoted PS literal
///   (`ConvertTo-SecureString '{password}' -AsPlainText -Force`). A stored
///   password containing `'; Invoke-Expression(...); #` could escape the
///   literal and execute arbitrary PS on the local machine. The fix: the
///   wrapper now reads the password from stdin at runtime via
///   `[Console]::In.ReadLine()`. The password never appears in ANY string
///   literal of the encoded script, so no quote-escape attack is possible.
///   `run_winrm_sync` writes `password\n` to the child's stdin.
fn build_winrm_script(host: &str, username: &str, _password_unused: &str, script: &str) -> String {
    // Base64-encode the inner user script to prevent ScriptBlock breakout
    let encoded = base64_encode_utf16le(script);

    format!(
        // Read password from stdin (set by run_winrm_sync) — NEVER interpolated.
        "$pass_plain = [Console]::In.ReadLine(); \
         $pass = ConvertTo-SecureString $pass_plain -AsPlainText -Force; \
         $cred = New-Object System.Management.Automation.PSCredential('{}', $pass); \
         Invoke-Command -ComputerName '{}' -Credential $cred -ScriptBlock {{ \
           $s = [System.Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('{}')); \
           Invoke-Expression $s \
         }} -ErrorAction Stop",
        username.replace('\'', "''"),
        host.replace('\'', "''"),
        encoded
    )
}

/// Base64-encode a string as UTF-16LE (PowerShell's native encoding).
fn base64_encode_utf16le(s: &str) -> String {
    let utf16: Vec<u8> = s.encode_utf16()
        .flat_map(|c| c.to_le_bytes())
        .collect();
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(&utf16)
}

/// Invoke a script on a remote Windows host via WinRM.
/// Password is passed via stdin (not process args).
/// Script is Base64-encoded to prevent ScriptBlock injection.
///
/// Returns (stdout, stderr, success).
pub fn run_winrm_sync(
    host: &str,
    username: &str,
    password: &str,
    script: &str,
) -> Result<std::process::Output, String> {
    // ── Guardrail (audit S1): password injection inside SecureString literal ──
    // Belt-and-braces: the structural fix below (stdin-fed password, no
    // string-literal interpolation) closes this vector entirely, but we
    // keep the guardrail scan as defense-in-depth — refuse passwords that
    // contain PS-injection signatures even before we try to use them.
    let pscan = crate::guardrails::scan(password, crate::guardrails::Role::SecretMaterial);
    if matches!(pscan.decision, crate::guardrails::ScanDecision::Block) {
        return Err(format!(
            "Credencial del host rechazada por guardrail: {}. \
             Actualiza el password almacenado para este host.",
            pscan.reason
        ));
    }

    // S1 structural fix: encode the wrapper script with -EncodedCommand and
    // pipe the password through stdin. The password is now NEVER part of any
    // PowerShell string literal — quote-escape attacks are impossible.
    let ps_script = build_winrm_script(host, username, password, script);
    let encoded_wrapper = base64_encode_utf16le(&ps_script);

    let mut child = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy").arg("Bypass")
        .arg("-NonInteractive")
        .arg("-EncodedCommand").arg(&encoded_wrapper)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Error spawning PowerShell: {}", e))?;

    // The wrapper's first action is `[Console]::In.ReadLine()` — feed the
    // password followed by a newline. Then close stdin so the child knows
    // there's no more input coming.
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(password.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|e| format!("Error writing password to PowerShell stdin: {}", e))?;
    }

    child.wait_with_output()
        .map_err(|e| format!("Error waiting for PowerShell: {}", e))
}

/// Async wrapper for `run_winrm_sync` using tokio spawn_blocking.
pub async fn run_winrm(
    host: String,
    username: String,
    password: String,
    script: String,
) -> Result<std::process::Output, String> {
    tokio::task::spawn_blocking(move || {
        run_winrm_sync(&host, &username, &password, &script)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Build a WinRM script with a custom ScriptBlock suffix (e.g. exit-code capture).
/// Used by shell.rs streaming which needs extra logic after the user's command.
fn build_winrm_script_with_suffix(
    host: &str,
    username: &str,
    _password_unused: &str,
    script: &str,
    suffix: &str,
) -> String {
    // S1 audit (May 2026): password is now read from stdin via
    // [Console]::In.ReadLine() instead of being interpolated into a PS
    // string literal. See build_winrm_script for full rationale.
    let encoded = base64_encode_utf16le(script);
    let suffix_encoded = base64_encode_utf16le(suffix);

    format!(
        "$pass_plain = [Console]::In.ReadLine(); \
         $pass = ConvertTo-SecureString $pass_plain -AsPlainText -Force; \
         $cred = New-Object System.Management.Automation.PSCredential('{}', $pass); \
         Invoke-Command -ComputerName '{}' -Credential $cred \
           -ScriptBlock {{ \
             $ErrorActionPreference='Continue'; \
             $s = [System.Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('{}')); \
             Invoke-Expression $s; \
             $sf = [System.Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('{}')); \
             Invoke-Expression $sf \
           }} -ErrorAction Stop",
        username.replace('\'', "''"),
        host.replace('\'', "''"),
        encoded,
        suffix_encoded
    )
}

/// Spawn a WinRM child process for streaming output (used by NexShell).
/// Returns the spawned child process with piped stdout/stderr.
pub fn spawn_winrm_streaming(
    host: &str,
    username: &str,
    password: &str,
    script: &str,
    suffix: &str,
) -> Result<std::process::Child, String> {
    // Guardrail S1 — mirror of run_winrm_sync. See that function for rationale.
    let pscan = crate::guardrails::scan(password, crate::guardrails::Role::SecretMaterial);
    if matches!(pscan.decision, crate::guardrails::ScanDecision::Block) {
        return Err(format!(
            "Credencial del host rechazada por guardrail: {}. \
             Actualiza el password almacenado para este host.",
            pscan.reason
        ));
    }

    // S1 audit structural fix: pass wrapper via -EncodedCommand, feed
    // password through stdin. See run_winrm_sync for full rationale.
    let ps_script = build_winrm_script_with_suffix(host, username, password, script, suffix);
    let encoded_wrapper = base64_encode_utf16le(&ps_script);

    let mut child = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy").arg("Bypass")
        .arg("-NonInteractive")
        .arg("-EncodedCommand").arg(&encoded_wrapper)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Error spawning PowerShell: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(password.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|e| format!("Error writing password to PowerShell stdin: {}", e))?;
    }

    Ok(child)
}
