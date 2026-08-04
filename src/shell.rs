//! Ejecución de PowerShell y decodificación de lo que devuelve.
//!
//! Cuarto lote hacia el corazón compartido, y el primero que no desbloquea una
//! vista sino tres: Inventario, Compliance y buena parte de NexShell hacen lo
//! mismo —lanzar PowerShell y leer su salida— y ninguna puede migrar sin esto.
//!
//! Lo que se queda en `src-tauri/src/utils/shell.rs` es lo que no es genérico:
//! WinRM, la validación de hosts y los guardrails sobre credenciales. Eso es
//! política de un anfitrión que expone comandos a un LLM, igual que la guarda de
//! rutas del lote anterior.

/// Lanza el proceso sin abrir una ventana de consola. Lucy es una app gráfica:
/// sin esto, cada comando parpadea una ventana negra en la cara del usuario.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ── Decodificar lo que escriben las herramientas nativas ─────────────────────
//
// A PowerShell se le PUEDE pedir que escriba UTF-8 (ver el preámbulo abajo).
// A `tasklist`, `netstat`, `reg` o `ipconfig` no: escriben la página de códigos
// OEM del sistema —CP-850 en una instalación en español, CP-437 en una en
// inglés— y no hay interruptor.
//
// Medido en esta máquina con la consola ya en UTF-8: `tasklist` y `systeminfo`
// devuelven UTF-8 válido, `netstat -an` e `ipconfig /all` no. Es por herramienta,
// no por máquina. Leídas todas como UTF-8, un adaptador llamado
// "Descripción" llegaba como "Descripci<?>n" — y esa salida se le entrega al
// modelo como verdad sobre el equipo, así que una ruta con un carácter de
// reemplazo es una ruta contra la que después propone comandos.
//
// `encoding_rs` no sirve: implementa el juego WHATWG, que cubre windows-1252 y
// no CP-850. La página OEM es un concepto de Windows y solo Windows sabe cuál
// es, de ahí `MultiByteToWideChar(CP_OEMCP)`.

/// Decodifica bytes escritos por una herramienta nativa de consola.
///
/// UTF-8 primero, y gana cuando es válido. El orden es deliberado: ASCII puro
/// —la inmensa mayoría de esta salida— es UTF-8 válido y pasa intacto, y las
/// herramientas que sí emiten UTF-8 siguen funcionando. Solo los bytes que NO
/// son UTF-8 válido, que es a lo que se parece el texto OEM con acentos, van al
/// decodificador OEM.
///
/// Una secuencia OEM corta puede ser además UTF-8 válido y leerse como los
/// caracteres equivocados. Ese residuo es inevitable sin conocer la codificación
/// de cada herramienta, y sustituye a una ruta que salía mal SIEMPRE.
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
    // `i32` porque es lo que toma la API. Una herramienta que produjera 2 GB
    // tiene otro problema, y el respaldo lossy es una respuesta correcta para
    // ella en vez de un pánico.
    let Ok(len) = i32::try_from(bytes.len()) else {
        return String::from_utf8_lossy(bytes).into_owned();
    };

    unsafe {
        // Primera llamada para dimensionar, segunda para llenar.
        let needed = MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr() as *const i8,
            len,
            std::ptr::null_mut(),
            0,
        );
        if needed <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide: Vec<u16> = vec![0; needed as usize];
        let written = MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr() as *const i8,
            len,
            wide.as_mut_ptr(),
            needed,
        );
        if written <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        wide.truncate(written as usize);
        String::from_utf16_lossy(&wide)
    }
}

#[cfg(not(windows))]
fn decode_oem(bytes: &[u8]) -> String {
    // Fuera de Windows no hay página OEM. Presente para que el módulo compile
    // en un `cargo check` de otra plataforma.
    String::from_utf8_lossy(bytes).into_owned()
}

// ── PowerShell, decodificado bien ────────────────────────────────────────────

/// Preámbulo que hace que un PowerShell lanzado escriba UTF-8 a la tubería.
///
/// Lucy es un proceso gráfico sin consola, así que un PowerShell hijo escribe en
/// la página OEM del sistema — donde `ó` es el byte suelto `0xA2`. Eso no es
/// UTF-8 válido y `from_utf8_lossy` lo sustituye por U+FFFD: el texto llega roto
/// sin que nada dé error.
///
/// El ORDEN importa y no es cosmético: PowerShell fija la codificación de un
/// flujo en la primera escritura, así que esto tiene que ir ANTES del script.
/// Y hacen falta los dos manejadores: `[Console]::OutputEncoding` cubre lo que
/// el proceso escribe a la tubería, `$OutputEncoding` cubre lo que le pasa a un
/// comando nativo en una tubería interna.
pub const PS_UTF8_PREAMBLE: &str =
    "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()\n\
     $OutputEncoding = [System.Text.UTF8Encoding]::new()\n";

/// Antepone el preámbulo a un script.
pub fn ps_utf8(script: &str) -> String {
    format!("{}{}", PS_UTF8_PREAMBLE, script)
}

/// Ejecuta un script de PowerShell y devuelve `(stdout, stderr, ok)`.
///
/// La salida pasa por `decode_console`, no por `from_utf8_lossy`: el preámbulo
/// cubre lo que PowerShell escribe por sí mismo, pero un script que invoca una
/// herramienta nativa —`netstat`, `reg`— trae los bytes de ésa mezclados.
#[cfg(windows)]
pub fn run_powershell_utf8(script: &str) -> Result<(String, String, bool), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_utf8(script)])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("PowerShell spawn failed: {}", e))?;
    Ok((
        decode_console(&out.stdout),
        decode_console(&out.stderr),
        out.status.success(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Las letras acentuadas de 0x80–0xA5 son IDÉNTICAS en CP-437 (Windows en
    // inglés, que es lo que corre el CI) y CP-850 (español, donde se encontró
    // esto). Por eso estos fixtures afirman lo mismo en las dos.
    const OEM_DISEÑO: &[u8] = &[b'D', b'i', b's', b'e', 0xA4, b'o']; // ñ = 0xA4
    const OEM_ACCENTS: &[u8] = &[0xA0, 0x82, 0xA1, 0xA2, 0xA3]; // á é í ó ú

    #[test]
    fn ascii_survives_untouched() {
        assert_eq!(decode_console(b"Image Name    PID"), "Image Name    PID");
        assert_eq!(decode_console(b""), "");
    }

    #[test]
    fn real_utf8_is_not_mangled_by_the_oem_path() {
        // Algunas herramientas SÍ emiten UTF-8. Probar UTF-8 primero es lo que
        // las mantiene funcionando: decodificarlas como OEM convertiría cada
        // carácter multibyte en dos o tres equivocados.
        let utf8 = "café — ñandú".as_bytes();
        assert_eq!(decode_console(utf8), "café — ñandú");
    }

    #[test]
    #[cfg(windows)]
    fn oem_bytes_decode_to_the_letters_the_tool_printed() {
        assert_eq!(decode_console(OEM_DISEÑO), "Diseño");
        assert_eq!(decode_console(OEM_ACCENTS), "áéíóú");
    }

    #[test]
    fn the_naive_decoder_really_did_corrupt_these_bytes() {
        // Fija la PREMISA, no el arreglo. Si un Windows o un Rust futuro
        // hicieran que `from_utf8_lossy` acertara con esto, el trabajo de arriba
        // sobraría — y aquí es donde se vería.
        let old = String::from_utf8_lossy(OEM_DISEÑO);
        assert!(old.contains('\u{FFFD}'), "esperaba caracteres de reemplazo: {old:?}");
        assert_ne!(old, "Diseño");
    }

    #[test]
    fn the_preamble_sets_both_handles_before_the_payload() {
        let w = ps_utf8("Get-Date");
        assert!(w.starts_with("[Console]::OutputEncoding"), "el preámbulo va primero");
        assert!(w.contains("$OutputEncoding"), "falta el segundo manejador");
        let script_at = w.find("Get-Date").expect("el script debe seguir ahí");
        let out_enc_at = w.find("$OutputEncoding").expect("...");
        assert!(out_enc_at < script_at, "la codificación se fija en la primera escritura");
    }
}
