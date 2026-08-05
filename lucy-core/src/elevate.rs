//! Ejecutar un comando COMO ADMINISTRADOR, con su UAC.
//!
//! POR QUÉ LOS FLUJOS DE LA V1 Y LA V2 NO FUNCIONABAN. Elevar en Windows no es
//! un parámetro: es lanzar un proceso NUEVO con `Start-Process -Verb RunAs`, y
//! ese proceso corre en otra sesión de integridad con sus propios descriptores
//! de salida. El padre —sin elevar— NO PUEDE leer su stdout. Cualquier intento
//! de capturarlo directamente devuelve vacío, que es exactamente el síntoma de
//! "se ejecuta pero no trae nada".
//!
//! La solución es la única que hay: el hijo escribe su salida a un FICHERO, y el
//! padre lo lee cuando el hijo termina. Suena rodeo y no lo es — es el
//! mecanismo, y no conocerlo es lo que hace que este flujo se implemente tres
//! veces y las tres se quede a medias.
//!
//! Y EL COMANDO VIAJA EN UN `.ps1`, no en la línea de órdenes. Meterlo como
//! argumento obliga a escapar comillas dentro de comillas dentro de
//! `-ArgumentList`, y un comando de PowerShell real —con `Where-Object { $_.X
//! -eq 'Y' }`— lleva las tres clases a la vez. Con un fichero no hay nada que
//! escapar: el comando se escribe tal cual y se ejecuta tal cual.

use std::path::PathBuf;

/// Dónde se dejan el script y su salida.
///
/// En el temporal del usuario y con el id del proceso en el nombre: dos
/// ventanas de Lucy abiertas a la vez no pueden pisarse el fichero.
fn temp_pair() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir();
    let stamp = format!("{}-{:?}", std::process::id(), std::time::Instant::now());
    let key: u64 = stamp.bytes().fold(1469598103934665603, |h, b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    });
    (
        dir.join(format!("lucy-elev-{key:x}.ps1")),
        dir.join(format!("lucy-elev-{key:x}.out")),
    )
}

/// El contenido del `.ps1` que correrá elevado.
///
/// `*>` redirige TODOS los flujos —salida, errores, avisos, verboso— al mismo
/// fichero. Con solo `>` el error de un comando fallido se quedaría en una
/// consola que se cierra al instante, y el operador vería un fichero vacío
/// creyendo que no pasó nada.
pub fn build_script(cmd: &str, out: &std::path::Path) -> String {
    format!(
        "$ErrorActionPreference = 'Continue'\r\n\
         & {{\r\n{cmd}\r\n}} *> '{}'\r\n\
         exit $LASTEXITCODE\r\n",
        out.display()
    )
}

/// El comando que el padre lanza para pedir la elevación.
///
/// `-Wait` es obligatorio: sin él, `Start-Process` vuelve en cuanto UAC acepta y
/// el padre leería el fichero de salida antes de que el hijo escriba nada.
/// `-PassThru` da el proceso para poder mirar su código de salida.
pub fn build_launcher(script: &std::path::Path) -> String {
    format!(
        "$p = Start-Process powershell -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
         -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','{}'; \
         exit $p.ExitCode",
        script.display()
    )
}

/// Corre `cmd` elevado y devuelve `(salida, ok)`.
///
/// Un UAC cancelado NO es un fallo del comando y se distingue: el operador dijo
/// que no, y decirle "el comando falló" sería culpar a la herramienta de una
/// decisión suya.
#[cfg(windows)]
pub fn run_elevated(cmd: &str) -> Result<(String, bool), String> {
    let (ps, out) = temp_pair();
    std::fs::write(&ps, build_script(cmd, &out))
        .map_err(|e| format!("No se pudo preparar el script elevado: {e}"))?;

    let r = crate::shell::run_powershell_utf8(&build_launcher(&ps));
    let salida = std::fs::read_to_string(&out).unwrap_or_default();
    // Se limpian los dos SIEMPRE, incluso si algo falló: son ficheros con
    // comandos del operador dentro y no tienen por qué quedarse en el temporal.
    let _ = std::fs::remove_file(&ps);
    let _ = std::fs::remove_file(&out);

    match r {
        Ok((_, err, ok)) => {
            if !ok && salida.trim().is_empty() {
                // `Start-Process -Verb RunAs` falla con esto cuando el usuario
                // cierra el diálogo de UAC.
                let cancelado = err.contains("cancel") || err.contains("canceló")
                    || err.contains("1223");
                return Ok((
                    if cancelado {
                        "El operador canceló la elevación: el comando no se ejecutó.".into()
                    } else {
                        format!("No se pudo elevar: {}", err.trim())
                    },
                    false,
                ));
            }
            Ok((salida, ok))
        }
        Err(e) => Err(e),
    }
}

#[cfg(not(windows))]
pub fn run_elevated(_cmd: &str) -> Result<(String, bool), String> {
    Err("La elevación solo existe en Windows".into())
}

/// ¿La salida de un comando dice que le faltaron privilegios?
///
/// Sirve para OFRECER la elevación después de un fallo, en vez de pedirla por
/// adelantado. Un UAC que salta antes de saber si hace falta enseña a aceptarlo
/// sin leerlo, y ese hábito es peor que el comando que se quería correr.
pub fn looks_like_access_denied(output: &str) -> bool {
    let o = output.to_lowercase();
    [
        "access is denied",
        "acceso denegado",
        "no se puede abrir el servicio",
        "cannot open",
        "openerror",
        "requires elevation",
        "requiere elevación",
        "unauthorizedaccess",
        "permissiondenied",
        "5)", // el código 5 de Win32: ERROR_ACCESS_DENIED
    ]
    .iter()
    .any(|m| o.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn el_script_redirige_todos_los_flujos() {
        // Con solo `>` el error de un comando fallido se queda en una consola
        // que se cierra al instante, y el operador ve un fichero vacío creyendo
        // que no pasó nada.
        let s = build_script("Get-Service", Path::new("C:/tmp/o.txt"));
        assert!(s.contains("*>"), "no redirige todos los flujos: {s}");
        assert!(s.contains("Get-Service"));
        assert!(s.contains("C:/tmp/o.txt"));
    }

    #[test]
    fn el_comando_va_dentro_del_script_sin_escapar_nada() {
        // Un comando real lleva comillas simples, dobles y llaves a la vez.
        // Como argumento habría que escaparlas tres veces; en un fichero se
        // escribe tal cual, y ESA es la razón de usar un `.ps1`.
        let cmd = "Get-Service | Where-Object { $_.Status -eq 'Stopped' -and $_.Name -ne \"x\" }";
        let s = build_script(cmd, Path::new("C:/tmp/o.txt"));
        assert!(s.contains(cmd), "el comando se alteró al construir el script");
    }

    #[test]
    fn el_lanzador_espera_al_hijo() {
        // Sin `-Wait`, `Start-Process` vuelve en cuanto UAC acepta y el padre
        // leería el fichero antes de que el hijo escriba nada — el fallo que
        // deja "se ejecutó pero no trajo nada".
        let l = build_launcher(Path::new("C:/tmp/s.ps1"));
        assert!(l.contains("-Wait"), "sin -Wait se lee el fichero vacío");
        assert!(l.contains("-Verb RunAs"), "sin RunAs no hay elevación");
        assert!(l.contains("C:/tmp/s.ps1"));
    }

    #[test]
    fn se_reconoce_un_acceso_denegado_en_los_dos_idiomas() {
        // Windows responde en el idioma del sistema, y esta app corre en
        // español: mirar solo el inglés es cómo se pierde la mitad de los casos.
        assert!(looks_like_access_denied("Access is denied"));
        assert!(looks_like_access_denied("Acceso denegado."));
        assert!(looks_like_access_denied(
            "Start-Service : No se puede abrir el servicio gpsvc en el equipo"
        ));
        assert!(looks_like_access_denied("Error: OpenError"));
        assert!(!looks_like_access_denied("El servicio está detenido"));
        assert!(!looks_like_access_denied(""));
    }

    #[test]
    fn dos_ejecuciones_no_comparten_fichero() {
        // Dos ventanas de Lucy abiertas a la vez no pueden pisarse la salida.
        let (a, _) = temp_pair();
        let (b, _) = temp_pair();
        assert_ne!(a, b);
    }
}
