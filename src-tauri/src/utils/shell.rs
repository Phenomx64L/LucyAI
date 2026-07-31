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

// ── Native console tools, decoded correctly ─────────────────────────────────
//
// PowerShell can be TOLD to write UTF-8 (see the preamble below). `tasklist`,
// `netstat`, `reg`, `wevtutil` and friends cannot: they write the system OEM
// code page — CP-850 on a Spanish install, CP-437 on a US one — and there is no
// switch. Every one of these was read with `from_utf8_lossy`, so a process
// named `Diseño gráfico.exe` came back with U+FFFD where its letters were.
//
// It matters more here than in a log viewer. This output is fed to the LLM as
// ground truth about the machine: a path with a replacement character in it is
// a path the agent will then propose commands against, and the corruption looks
// exactly like a filename it does not recognise. `execute_cmd` is the widest
// door — it is how the agent runs arbitrary console tools at all.
//
// `encoding_rs` does not help: it implements the WHATWG encoding set, which
// covers windows-1252 and not CP-850. The OEM code page is a Windows concept
// and only Windows can name it, hence `MultiByteToWideChar(CP_OEMCP)`.

/// Decode bytes written by a native Windows console tool.
///
/// UTF-8 is tried first and wins when it parses. That ordering is deliberate:
/// pure ASCII (the overwhelming majority of this output) is valid UTF-8 and
/// passes through untouched, and the handful of tools that do emit UTF-8 keep
/// working. Only bytes that are NOT valid UTF-8 — which is what OEM text with
/// accents looks like — go to the OEM decoder.
///
/// A short OEM sequence can in principle also be valid UTF-8 and be read as the
/// wrong characters. That residue is unavoidable without knowing each tool's
/// encoding, and it replaces a path that was wrong every single time.
pub fn decode_console(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => decode_oem(bytes),
    }
}

#[cfg(windows)]
fn decode_oem(bytes: &[u8]) -> String {
    use winapi::um::stringapiset::MultiByteToWideChar;
    use winapi::um::winnls::CP_OEMCP;

    if bytes.is_empty() {
        return String::new();
    }
    // `i32` because that is what the API takes; a console tool that produced
    // 2 GB of output has a different problem, and the lossy fallback is a
    // correct answer for it rather than a panic.
    let Ok(len) = i32::try_from(bytes.len()) else {
        return String::from_utf8_lossy(bytes).into_owned();
    };

    unsafe {
        // First call sizes the buffer, second fills it — the standard two-step.
        let needed = MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr() as *const i8, len, std::ptr::null_mut(), 0);
        if needed <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide: Vec<u16> = vec![0; needed as usize];
        let written = MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr() as *const i8, len, wide.as_mut_ptr(), needed);
        if written <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        wide.truncate(written as usize);
        String::from_utf16_lossy(&wide)
    }
}

#[cfg(not(windows))]
fn decode_oem(bytes: &[u8]) -> String {
    // There is no OEM code page off Windows, and this crate only ships there.
    // Present so the module still compiles under a non-Windows `cargo check`.
    String::from_utf8_lossy(bytes).into_owned()
}

// ── PowerShell, decoded correctly ────────────────────────────────────────────

/// Preamble that makes a spawned PowerShell write UTF-8 to the pipe.
///
/// Lucy is a GUI process with no console, so a PowerShell it spawns writes its
/// output in the system OEM code page — CP-850 on a Spanish install, where `ó`
/// is the single byte `0xA2`. That is not valid UTF-8, so `from_utf8_lossy`
/// substitutes U+FFFD and the text arrives corrupted. Nothing errors: mangled
/// text is still valid JSON and still parses, so the damage is silent.
///
/// Measured on an es-ES host: 1 of 135 installed-software entries came back as
/// "NVIDIA Controlador de gr<?>ficos".
///
/// Must run BEFORE the payload — PowerShell fixes a stream's encoding the first
/// time it writes to it, so assigning afterwards is too late.
pub const PS_UTF8_PREAMBLE: &str = "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()\n\
                                    $OutputEncoding = [System.Text.UTF8Encoding]::new()\n";

/// Prefix a script with [`PS_UTF8_PREAMBLE`].
pub fn ps_utf8(script: &str) -> String {
    format!("{}{}", PS_UTF8_PREAMBLE, script)
}

/// Run a PowerShell script and return `(stdout, stderr, success)`, decoded as
/// UTF-8 because [`ps_utf8`] made it UTF-8.
///
/// Use this instead of hand-rolling `Command::new("powershell")` — every
/// hand-rolled site in this codebase was missing the preamble.
///
/// NOTE this does NOT cover the native Windows tools (`tasklist`, `netstat`,
/// `wevtutil`, `wmic`, `reg`, `cmd`) that `local.rs` also spawns. They have no
/// equivalent switch: their output is OEM-encoded and has to be DECODED as
/// such, which is a different fix. Measured as affected: tasklist, netstat.
pub fn run_powershell_utf8(script: &str) -> Result<(String, String, bool), String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_utf8(script)])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("PowerShell spawn failed: {}", e))?;
    Ok((
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.success(),
    ))
}

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
mod console_decode_tests {
    use super::decode_console;

    // The accented Latin letters at 0x80–0xA5 are IDENTICAL in CP-437 (US
    // Windows, which is what CI runs) and CP-850 (Spanish Windows, the machine
    // this was found on). These fixtures therefore assert the same thing on
    // both, which is what makes them safe to gate on.
    const OEM_DISEÑO: &[u8] = &[b'D', b'i', b's', b'e', 0xA4, b'o'];        // ñ = 0xA4
    const OEM_ACCENTS: &[u8] = &[0xA0, 0x82, 0xA1, 0xA2, 0xA3];             // á é í ó ú

    #[test]
    fn ascii_survives_untouched() {
        // The overwhelming majority of console output. It is valid UTF-8, so
        // it takes the fast path and never reaches the OEM decoder.
        assert_eq!(decode_console(b"Image Name    PID Session"), "Image Name    PID Session");
        assert_eq!(decode_console(b""), "");
    }

    #[test]
    fn real_utf8_is_not_mangled_by_the_oem_path() {
        // A few tools do emit UTF-8. Trying UTF-8 first is what keeps them
        // working — decoding their bytes as OEM would turn every multi-byte
        // character into two or three wrong ones.
        let utf8 = "café — ñandú".as_bytes();
        assert_eq!(decode_console(utf8), "café — ñandú");
    }

    #[test]
    #[cfg(windows)]
    fn oem_bytes_decode_to_the_letters_the_tool_actually_printed() {
        // This is the bug. `tasklist` on a Spanish install writes a process
        // called "Diseño.exe" as these bytes, and `from_utf8_lossy` turned the
        // ñ into U+FFFD before the text ever reached the model — which then
        // reasoned about, and proposed commands against, a filename that does
        // not exist.
        assert_eq!(decode_console(OEM_DISEÑO), "Diseño");
        assert_eq!(decode_console(OEM_ACCENTS), "áéíóú");
    }

    #[test]
    fn the_old_decoder_really_did_corrupt_these_bytes() {
        // Pins the premise rather than the fix. If a future Windows/Rust
        // version made `from_utf8_lossy` handle this, the change above would be
        // pointless work and this test is where that shows up.
        let old = String::from_utf8_lossy(OEM_DISEÑO);
        assert!(old.contains('\u{FFFD}'), "expected replacement chars, got {old:?}");
        assert_ne!(old, "Diseño");
    }
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
