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


// v1.8 — el decodificador de consola y los ayudantes de PowerShell viven en
// `lucy-core::shell`, compartidos con el shell nativo. Los tres necesitan lo
// mismo: lanzar PowerShell y leer bien lo que devuelve.
//
// Lo que se queda en este fichero es lo que NO es genérico: WinRM, la validación
// de hosts y los guardrails sobre credenciales. Eso es política de un anfitrión
// que expone comandos a un LLM, no mecanismo — la misma línea que se trazó con
// la guarda de rutas del visor de logs.
//
// Se reexportan para que ningún sitio de llamada de este crate cambie.
// `PS_UTF8_PREAMBLE` no lo consume nada aquí —`ps_utf8` lo aplica— pero forma
// parte de la superficie pública que este módulo tenía antes del movimiento.
#[allow(unused_imports)]
pub use lucy_core::shell::{
    decode_console, ps_utf8, run_powershell_utf8, PS_UTF8_PREAMBLE,
};

/// Agrega un host a TrustedHosts de WinRM (mejor-esfuerzo, requiere admin local).
/// Necesario para conectar por IP en lugar de nombre de dominio.
/// Si falla no interrumpe la operación — el error de WinRM se verá después.
pub fn ensure_trusted_host(host: &str) {
    // LOGIC-2 FIX: defense-in-depth — validate host format here even though callers
    // should have already called validate_host(). This function is pub and could be
    // called from any module with any string. A host containing ' could escape the
    // PS single-quoted literal.
    if host.contains('\'') || host.contains(';') || host.contains('`') || host.is_empty() || host.len() > 255 {
        crate::utils::logging::write_app_log(
            "WARNING",
            &format!("ensure_trusted_host: rejected suspicious host value: {:?}", crate::utils::safe_truncate(host, 60))
        );
        return;
    }
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
/// KNOWN RISK (SEC-3, audited May 2026): The decoded script is executed via
/// `Invoke-Expression` on the remote host. The ScriptBlock injection is prevented
/// (Base64 encoding), and the password is protected (stdin). However, the CONTENT
/// of the script itself is LLM-controlled — if the local guardrail layer is
/// bypassed (e.g., indirect prompt injection via a file Lucy reads), arbitrary
/// PowerShell runs on the remote host with the configured credential.
///
/// MITIGATION: Configure WinRM credentials with MINIMUM PRIVILEGE (no Domain Admin).
/// Consider adding a per-host "max command tier" that limits what categories of
/// commands can run on each remote host.
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preamble_sets_both_encoding_handles_before_the_payload() {
        // PowerShell fixes a stream's encoding on first write, so the order is
        // load-bearing, not cosmetic. Both handles matter: [Console] covers
        // what the process writes to the pipe, $OutputEncoding covers what it
        // hands to a downstream native command in a pipeline.
        let w = ps_utf8("Get-Date");
        let console = w.find("[Console]::OutputEncoding").expect("console handle missing");
        let pipeline = w.find("$OutputEncoding").expect("pipeline handle missing");
        let payload = w.find("Get-Date").expect("payload missing");
        assert!(console < payload && pipeline < payload, "encoding must be set first");
        assert!(w.contains("UTF8Encoding"));
    }

    #[test]
    fn preamble_does_not_run_into_the_payload() {
        // A missing newline would concatenate the last assignment with the
        // first line of the script and break every caller at once.
        assert!(PS_UTF8_PREAMBLE.ends_with('\n'));
        assert!(ps_utf8("Write-Output 'x'").lines().count() >= 3);
    }

    // ── Architectural guard ─────────────────────────────────────────────────
    //
    // The 2026-07-28 audit found that EVERY hand-rolled `Command::new(
    // "powershell")` in the codebase was missing the preamble — not some, all.
    // The failure is silent (mangled text is still valid JSON and still
    // parses), so nothing catches it downstream and no reviewer notices at the
    // call site. These files are pinned to the shared runner so the next one
    // has to be a deliberate choice rather than a copy-paste.
    //
    // If you legitimately need a raw spawn here (streaming, a custom stdin),
    // add the preamble yourself and amend this list with the reason.
    //
    // BLIND SPOT, stated so nobody mistakes this for full coverage: it matches
    // the literal string, so `Command::new(exe)` with a variable slips past —
    // `script_verify.rs` does exactly that (pwsh-or-powershell) and is fixed by
    // hand rather than by this guard. Widening it to catch variables would mean
    // parsing Rust, which is not worth it for a net whose job is to stop
    // copy-paste.

    #[test]
    fn migrated_callers_do_not_hand_roll_a_powershell_spawn() {
        const GUARDED: &[(&str, &str)] = &[
            ("cve_match.rs", include_str!("../commands/cve_match.rs")),
            ("inventory.rs", include_str!("../commands/inventory.rs")),
            ("compliance.rs", include_str!("../commands/compliance.rs")),
            ("housekeeping.rs", include_str!("../commands/housekeeping.rs")),
            ("dashboard_integrations.rs", include_str!("../commands/dashboard_integrations.rs")),
        ];
        for (name, src) in GUARDED {
            for pattern in ["Command::new(\"powershell\")", "Command::new(\"powershell.exe\")"] {
                assert!(
                    !src.contains(pattern),
                    "{} spawns PowerShell directly — use utils::shell::run_powershell_utf8, \
                     or add the PS_UTF8_PREAMBLE and document why not",
                    name,
                );
            }
        }
    }
}
