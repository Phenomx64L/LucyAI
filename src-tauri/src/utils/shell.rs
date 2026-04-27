// ── SHELL UTILS — Utilidades de bajo nivel para procesos y validación ─────────

use std::process::Command;
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
