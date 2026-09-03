//! Lo que hay en el portapapeles cuando NO es texto.
//!
//! ── POR QUÉ HACE FALTA UN MÓDULO PARA ESTO ───────────────────────────────────
//!
//! Un `Ctrl+V` con una captura de pantalla dentro no llega a la aplicación. egui
//! entrega `Event::Paste` con una cadena y nada más: la integración de
//! portapapeles de `eframe` es de TEXTO. Una imagen o una lista de ficheros
//! copiada del Explorador se quedan fuera, y el operador ve que no pasa nada.
//!
//! Lo curioso es que el resto de la tubería ya estaba: arrastrar y soltar
//! funciona, los adjuntos se leen en su hilo, los PDF se trocean y los chips se
//! pintan. Faltaba una sola forma de entrar — la que más se usa.
//!
//! ── POR QUÉ POWERSHELL Y NO UN CRATE ─────────────────────────────────────────
//!
//! `arboard`, que ya está en el árbol porque lo arrastra `eframe`, sabe leer
//! imágenes pero NO listas de ficheros: `CF_HDROP` no está en su API. Habría que
//! añadir un segundo crate para la mitad que falta, y entonces tendríamos dos
//! mecanismos de portapapeles conviviendo.
//!
//! `System.Windows.Forms.Clipboard` hace las dos cosas en una sola llamada, y
//! deja la imagen ESCRITA COMO PNG EN DISCO — que es exactamente lo que la
//! tubería de adjuntos ya sabe tragar, porque es lo mismo que le llega al soltar
//! un fichero. Cero dependencias nuevas y un solo camino de entrada.
//!
//! Es además el precedente de la casa: `notify` ya llama a PowerShell con C# en
//! línea para escribir el AUMID del acceso directo.

/// Dónde se dejan las imágenes pegadas.
///
/// EN EL TEMPORAL DEL SISTEMA y con un nombre propio, para que se distingan de
/// un vistazo y para que quien limpie temporales se las lleve. Una imagen pegada
/// es un intermedio: lo que importa se guarda dentro de la conversación.
pub const PREFIJO: &str = "lucy-pegado-";

/// Cuánto se espera al portapapeles antes de rendirse.
///
/// Cuatro segundos. Arrancar PowerShell y cargar `System.Windows.Forms` son del
/// orden de medio segundo la primera vez, pero el portapapeles lo puede tener
/// bloqueado otra aplicación —pasa con las suites de ofimática— y ahí la llamada
/// se queda esperando. Un `Ctrl+V` que cuelga la ventana es peor que uno que no
/// hace nada.
pub const PLAZO_SECS: u64 = 4;

/// Las rutas que hay que adjuntar. Vacío = el portapapeles no traía nada que
/// sirva, que es el caso normal cuando lo que hay es texto.
///
/// BLOQUEANTE: quien llama ya está en un hilo. Ver [`PLAZO_SECS`].
#[cfg(windows)]
pub fn del_portapapeles() -> Result<Vec<std::path::PathBuf>, String> {
    // ORDEN DELIBERADO: primero ficheros, luego imagen. Copiar un `.png` del
    // Explorador deja en el portapapeles LAS DOS COSAS —la ruta y una vista
    // previa— y adjuntar el fichero original es mejor que adjuntar una copia
    // recomprimida de su miniatura.
    const GUION: &str = r#"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
if ([Windows.Forms.Clipboard]::ContainsFileDropList()) {
  [Windows.Forms.Clipboard]::GetFileDropList() | ForEach-Object { $_ }
} elseif ([Windows.Forms.Clipboard]::ContainsImage()) {
  $img = [Windows.Forms.Clipboard]::GetImage()
  $p = Join-Path $env:TEMP ('lucy-pegado-' + [Guid]::NewGuid().ToString('N').Substring(0,8) + '.png')
  $img.Save($p, [System.Drawing.Imaging.ImageFormat]::Png)
  $img.Dispose()
  $p
}
"#;
    // `-Sta`: el portapapeles de Windows EXIGE un apartamento de hilo único.
    // `powershell.exe` 5.1 ya arranca así, pero decirlo evita que esto se rompa
    // en silencio el día que alguien cambie el intérprete.
    // SIN VENTANA DE CONSOLA. Lucy es una aplicacion grafica: sin esta
    // bandera, cada llamada parpadea —o deja abierta— una ventana negra en la
    // cara del operador. Reportado al pegar: «se abre una ventana extraña de
    // PowerShell, eso nunca habia pasado». La casa ya lo hacia en `hosts` y en
    // `shell`; estos dos modulos son nuevos y se dejaron el detalle.
    use std::os::windows::process::CommandExt;
    let salida = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Sta", "-ExecutionPolicy", "Bypass", "-Command", GUION])
        .creation_flags(crate::shell::CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("no se pudo leer el portapapeles: {e}"))?;
    if !salida.status.success() {
        // EN UNA LÍNEA. Un error de PowerShell trae la traza entera con sus
        // saltos, y esto acaba en un aviso de una sola línea. Se colapsa aquí y
        // no con un ayudante compartido porque `una_linea` está declarada
        // PRIVADA tres veces en este crate —`memories`, `skills` y `watch`— y
        // añadir la cuarta copia, o hacer pública una de ellas de paso, es más
        // ruido del que ahorra.
        let motivo: String = String::from_utf8_lossy(&salida.stderr)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        return Err(format!("el portapapeles no contestó: {motivo}"));
    }
    Ok(rutas(&String::from_utf8_lossy(&salida.stdout)))
}

#[cfg(not(windows))]
pub fn del_portapapeles() -> Result<Vec<std::path::PathBuf>, String> {
    Err("solo en Windows".into())
}

/// La parte pura: de lo que escupe el guion a rutas que existen.
///
/// SE COMPRUEBA QUE EXISTAN, y no es paranoia. `GetFileDropList` devuelve lo que
/// se copió, no lo que sigue estando: cortar un fichero y pegarlo en otra
/// carpeta deja en el portapapeles una ruta que ya no apunta a nada. Adjuntar
/// eso daría un chip con un nombre y un error al leerlo, en vez de nada.
pub fn rutas(salida: &str) -> Vec<std::path::PathBuf> {
    salida
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
        .collect()
}

#[cfg(test)]
// Igual que en `maintenance` y `suggest`: la asercion del plazo compara
// CONSTANTES entre si. Clippy la ve evaluable en compilacion y avisa; no es una
// asercion muerta sino una guarda de invariante — fija que un pegado no pueda
// tardar tanto que parezca que la ventana se colgo.
#[allow(clippy::assertions_on_constants)]
mod tests {
    use super::*;

    #[test]
    fn una_salida_vacia_no_adjunta_nada() {
        // El caso NORMAL: el portapapeles lleva texto, el guion no imprime nada,
        // y esto no puede devolver una ruta vacía que luego dé un chip sin nombre.
        assert!(rutas("").is_empty());
        assert!(rutas("\n\n  \n").is_empty());
    }

    #[test]
    fn una_ruta_que_ya_no_existe_se_descarta() {
        // Cortar un fichero y pegarlo en otra carpeta deja en el portapapeles
        // una ruta que ya no apunta a nada. Adjuntarla daría un chip con nombre
        // y un error al leerlo.
        assert!(rutas(r"C:\no\existe\esto.png").is_empty());
    }

    #[test]
    fn se_leen_las_rutas_que_si_estan() {
        // Contra ficheros de verdad: los del propio crate.
        let a = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let b = concat!(env!("CARGO_MANIFEST_DIR"), "/src/clipboard.rs");
        let v = rutas(&format!("{a}\r\n{b}\r\n"));
        assert_eq!(v.len(), 2, "se perdió alguna: {v:?}");
        // Con los saltos de Windows, que es lo que devuelve PowerShell.
        assert!(v[0].ends_with("Cargo.toml"));
    }

    #[test]
    fn un_directorio_no_es_un_adjunto() {
        // `GetFileDropList` devuelve carpetas igual que ficheros si se copió una.
        // Adjuntar una carpeta no significa nada aquí.
        let d = env!("CARGO_MANIFEST_DIR");
        assert!(rutas(d).is_empty(), "se coló un directorio");
    }

    #[test]
    fn el_plazo_es_corto_porque_bloquea_un_ctrl_v() {
        // Otra aplicación puede tener el portapapeles bloqueado. Un `Ctrl+V` que
        // tarda diez segundos en no hacer nada es peor que uno que no hace nada.
        assert!(PLAZO_SECS <= 5, "un pegado no puede tardar esto");
        assert!(PLAZO_SECS >= 2, "arrancar PowerShell ya se lleva medio segundo");
    }

    /// Lee el portapapeles de ESTA máquina. Solo lectura salvo por el PNG que
    /// escribe en el temporal cuando lo que hay es una imagen.
    ///
    /// `cargo test -p lucy-core --lib clipboard::tests::que_hay_pegado -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn que_hay_pegado() {
        match del_portapapeles() {
            Ok(v) if v.is_empty() => println!("\n  nada que adjuntar (será texto)\n"),
            Ok(v) => {
                println!();
                for p in &v {
                    let n = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                    println!("  {}  ({n} bytes)", p.display());
                }
                println!();
            }
            Err(e) => println!("\n  falló: {e}\n"),
        }
    }
}
