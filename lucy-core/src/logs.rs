//! Lectura eficiente de la cola de un fichero de log.
//!
//! Tercer lote hacia el corazón compartido. Aquí vive el MECANISMO —leer las
//! últimas N líneas sin cargar el fichero entero— y no la POLÍTICA de qué rutas
//! pueden leerse.
//!
//! Esa separación es deliberada. El comando Tauri expone esto a un LLM, que
//! puede pedir cualquier ruta, así que valida contra
//! `enforce_sensitive_path` antes de llamar. El shell nativo lee la ruta fija de
//! `lucy_app.log` que él mismo construye. Mover la guarda aquí la convertiría en
//! una comprobación que el consumidor sin riesgo paga y el consumidor con riesgo
//! podría olvidarse de que existe, porque ya no la vería en su propio código.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Cuántas líneas como máximo, pase lo que pase. Un log rotado a 5 MB cabe de
/// sobra; el tope existe para que una petición absurda no reserve memoria sin
/// límite.
pub const MAX_LINES: usize = 50_000;

/// Últimas `lines` líneas de un fichero, en orden de lectura.
///
/// Lee hacia atrás en trozos de 64 KB: en un log de varios MB solo toca el final
/// en vez de cargarlo entero para quedarse con el 1 %.
pub fn tail(path: &Path, lines: usize) -> Result<Vec<String>, String> {
    let lines = lines.min(MAX_LINES);
    if lines == 0 {
        return Ok(vec![]);
    }
    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("No se pudo abrir '{}': {}", path.display(), e))?;

    let file_size = file.metadata().map_err(|e| e.to_string())?.len();
    if file_size == 0 {
        return Ok(vec![]);
    }

    let chunk_size: u64 = 65_536;
    let mut collected: Vec<String> = Vec::with_capacity(lines + 1);
    let mut pos = file_size;
    let mut remainder = String::new();
    // Un log termina en salto de línea, así que `split('\n')` produce una última
    // entrada vacía. La versión anterior la contaba como línea: pedir las 2
    // últimas devolvía una línea real y un blanco, y el blanco desplazaba todo
    // lo demás. Se descarta UNA VEZ, solo en el trozo final — un renglón vacío
    // en medio del log es contenido real y se conserva.
    let mut first_chunk = true;

    while pos > 0 && collected.len() < lines {
        let read_size = chunk_size.min(pos);
        pos -= read_size;
        file.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;

        let mut buf = vec![0u8; read_size as usize];
        file.read_exact(&mut buf).map_err(|e| e.to_string())?;

        // El log lleva BOM UTF-8 para que PowerShell y el Bloc de notas no
        // pinten las tildes como mojibake. Leyendo hacia atrás se llega al
        // offset 0, y el marcador saldría pegado a la primera línea.
        let mut chunk = String::from_utf8_lossy(&buf)
            .trim_start_matches('\u{FEFF}')
            .to_string()
            + &remainder;
        if first_chunk {
            // Solo el salto FINAL, y solo una vez.
            if chunk.ends_with('\n') {
                chunk.pop();
                if chunk.ends_with('\r') {
                    chunk.pop();
                }
            }
            first_chunk = false;
        }
        let mut chunk_lines: Vec<&str> = chunk.split('\n').collect();

        // La primera línea del trozo puede estar cortada por la mitad: se
        // guarda para pegarla al principio del trozo anterior.
        if pos > 0 {
            remainder = chunk_lines.remove(0).to_string();
        } else {
            remainder.clear();
        }

        for line in chunk_lines.into_iter().rev() {
            collected.push(line.trim_end_matches('\r').to_string());
            if collected.len() >= lines {
                break;
            }
        }
    }

    if !remainder.is_empty() && collected.len() < lines {
        collected.push(remainder.trim_end_matches('\r').to_string());
    }

    collected.reverse();
    Ok(collected)
}

/// Nivel de una línea de `lucy_app.log`, para filtrar y colorear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
}

impl Level {
    /// Deduce el nivel del formato `[fecha] [NIVEL] mensaje`.
    ///
    /// Acepta `WARN` y `WARNING`: el backend escribe los dos —
    /// `write_app_log("WARNING", …)` y `("WARN", …)` conviven en el árbol— y un
    /// filtro que reconociera solo uno escondería la mitad de los avisos, que es
    /// justo lo que un visor de logs no puede hacer.
    pub fn of(line: &str) -> Level {
        let upper = line.to_uppercase();
        if upper.contains("[ERROR]") {
            Level::Error
        } else if upper.contains("[WARN]") || upper.contains("[WARNING]") {
            Level::Warn
        } else {
            Level::Info
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_log(contents: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "lucy_core_logtest_{}.log",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let mut f = std::fs::File::create(&p).expect("crear temporal");
        f.write_all(contents.as_bytes()).expect("escribir");
        p
    }

    #[test]
    fn returns_the_last_lines_in_reading_order() {
        let p = tmp_log("uno\ndos\ntres\ncuatro\n");
        let got = tail(&p, 2).expect("tail");
        let _ = std::fs::remove_file(&p);
        // Las ÚLTIMAS dos, y en el orden en que se leen — no del revés.
        assert_eq!(got, vec!["tres".to_string(), "cuatro".to_string()]);
    }

    #[test]
    fn a_line_longer_than_the_chunk_is_not_cut_in_half() {
        // El caso que justifica el `remainder`: la lectura va hacia atrás en
        // trozos de 64 KB, así que una línea que cruza el límite aparece partida
        // si no se cose. Un stack trace en el log lo cruza sin esfuerzo.
        let long = "x".repeat(70_000);
        let p = tmp_log(&format!("primera\n{long}\nultima\n"));
        let got = tail(&p, 3).expect("tail");
        let _ = std::fs::remove_file(&p);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], "primera");
        assert_eq!(got[1].len(), 70_000, "la línea larga llegó partida");
        assert_eq!(got[2], "ultima");
    }

    #[test]
    fn the_bom_does_not_leak_into_the_first_line() {
        let p = tmp_log("\u{FEFF}[2026-01-01] [INFO] arranque\n");
        let got = tail(&p, 10).expect("tail");
        let _ = std::fs::remove_file(&p);
        assert_eq!(got.len(), 1);
        assert!(!got[0].starts_with('\u{FEFF}'), "el BOM llegó a la línea: {:?}", got[0]);
        assert!(got[0].starts_with("[2026"), "{:?}", got[0]);
    }

    #[test]
    fn an_empty_file_yields_nothing_rather_than_an_error() {
        let p = tmp_log("");
        let got = tail(&p, 10).expect("un log vacío es normal, no un fallo");
        let _ = std::fs::remove_file(&p);
        assert!(got.is_empty());
    }

    #[test]
    fn a_missing_file_says_which_one() {
        let err = tail(Path::new("C:/no/existe/lucy_x.log"), 10).expect_err("debe fallar");
        assert!(err.contains("lucy_x.log"), "el error debe nombrar la ruta: {err}");
    }

    #[test]
    fn level_recognises_both_warn_spellings() {
        // El backend escribe las dos formas. Reconocer solo una escondería la
        // mitad de los avisos.
        assert_eq!(Level::of("[2026-01-01] [WARN] algo"), Level::Warn);
        assert_eq!(Level::of("[2026-01-01] [WARNING] algo"), Level::Warn);
        assert_eq!(Level::of("[2026-01-01] [ERROR] algo"), Level::Error);
        assert_eq!(Level::of("[2026-01-01] [INFO] algo"), Level::Info);
        assert_eq!(Level::of("una línea sin nivel"), Level::Info);
    }
}
