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

    /// El nivel de una línea de UN LOG CUALQUIERA, no de `lucy_app.log`.
    ///
    /// `of` busca `[NIVEL]` entre corchetes porque ése es el formato que escribe
    /// Lucy. Sobre un log de IIS, un syslog o la salida de un servicio no hay
    /// corchetes en ninguna parte, así que `of` devuelve `Info` PARA TODO: el
    /// visor pinta tres mil líneas del mismo color y el contador de errores
    /// marca cero mientras el fichero está lleno de fallos. Es el único sitio
    /// donde un visor de logs no puede equivocarse.
    ///
    /// Se mira primero lo que `of` reconoce, para que un `lucy_app.log` leído por
    /// el modo Archivo dé exactamente lo mismo que leído por el modo Auditoría —
    /// dos criterios distintos sobre el mismo fichero es cómo se llega a que la
    /// misma línea sea un aviso en una pestaña y un error en la de al lado.
    ///
    /// PALABRAS ENTERAS. Sin eso, `error` casa dentro de `no-errors`, `sin
    /// errores` y `ErrorActionPreference`, y prácticamente todo el fichero sale
    /// en rojo.
    pub fn sniff(line: &str) -> Level {
        let etiquetado = Level::of(line);
        if etiquetado != Level::Info {
            return etiquetado;
        }
        // Español e inglés: los logs de una máquina Windows en español mezclan
        // los dos, y a menudo en la misma línea.
        const ERROR: [&str; 10] = [
            "error", "errores", "fatal", "fail", "failed", "failure", "denied",
            "exception", "denegado", "critical",
        ];
        const WARN: [&str; 8] = [
            "warn", "warning", "advertencia", "advertencias", "aviso", "avisos",
            "deprecated", "obsoleto",
        ];
        let bajo = line.to_lowercase();
        for p in palabras(&bajo) {
            if ERROR.contains(&p) {
                return Level::Error;
            }
        }
        for p in palabras(&bajo) {
            if WARN.contains(&p) {
                return Level::Warn;
            }
        }
        Level::Info
    }
}

/// Parte una línea en palabras, quedándose solo con letras y dígitos.
///
/// Por carácter alfanumérico UNICODE y no ASCII: `advertencia` no lleva tilde
/// pero `conexión` sí, y partir por `is_ascii_alphanumeric` cortaría la palabra
/// en la tilde y dejaría dos trozos que no casan con nada.
fn palabras(bajo: &str) -> impl Iterator<Item = &str> {
    bajo.split(|c: char| !c.is_alphanumeric()).filter(|p| !p.is_empty())
}

/// El comando que hay que correr en un equipo remoto para leer la cola de un log.
///
/// SEPARADO DE LA EJECUCIÓN A PROPÓSITO. Es donde vive el escapado de la ruta, y
/// un escapado mal hecho no da un error: da una ejecución de más. Así se puede
/// probar sin un servidor delante, que es la diferencia entre tener test y no
/// tenerlo — el mismo motivo por el que `winrm_wrapper` ya está aparte.
pub fn remote_script(h: &crate::hosts::Host, path: &str, lines: usize) -> Result<String, String> {
    if !h.protocol.can_shell() {
        return Err(format!(
            "«{}» está dado de alta como {} y por ahí no se leen ficheros. Hace falta \
             WinRM o SSH.",
            h.name,
            h.protocol.label()
        ));
    }
    let path = path.trim();
    if path.is_empty() {
        return Err("Falta la ruta del fichero.".into());
    }
    let lines = lines.clamp(1, MAX_LINES);
    Ok(match h.protocol {
        // `-ErrorAction Stop` DENTRO del ScriptBlock. El `$ErrorActionPreference`
        // del envoltorio de WinRM es local al proceso que lanza la conexión, no
        // al bloque que corre al otro lado: sin esto, una ruta que no existe
        // vuelve con código de salida CORRECTO y sin líneas, y el visor pinta
        // «log vacío» donde debería decir «ese fichero no está».
        crate::hosts::Protocol::Winrm => format!(
            "Get-Content -Path '{}' -Tail {lines} -ErrorAction Stop",
            crate::hosts::ps_quote(path)
        ),
        _ => format!("tail -n {lines} '{}'", crate::hosts::sh_quote(path)),
    })
}

/// La cola de un log en un equipo remoto, en orden de lectura.
///
/// EN ORDEN DE LECTURA, igual que `tail`, y no del revés. Que lo más reciente se
/// enseñe arriba es una decisión de la vista; devolverlo ya invertido desde aquí
/// obligaría a quien quisiera lo contrario a volver a darle la vuelta, y haría
/// que las dos funciones de este módulo con el mismo nombre devolvieran cosas
/// distintas.
pub fn tail_remote(
    h: &crate::hosts::Host,
    password: &str,
    path: &str,
    lines: usize,
) -> Result<Vec<String>, String> {
    let script = remote_script(h, path, lines)?;
    let (out, err, ok) = crate::hosts::run_remote(h, password, &script)?;
    if !ok {
        let motivo = err.trim();
        return Err(if motivo.is_empty() {
            format!("No se pudo leer '{path}' en {}.", h.name)
        } else {
            motivo.to_string()
        });
    }
    Ok(out.lines().map(|l| l.trim_end_matches('\r').to_string()).collect())
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

    fn host(protocol: crate::hosts::Protocol) -> crate::hosts::Host {
        crate::hosts::Host {
            id: "h1".into(),
            name: "WIN-AD".into(),
            os: "windows".into(),
            protocol,
            host: "10.0.0.5".into(),
            username: "admin".into(),
            port: 5985,
            ssh_key_path: String::new(),
            tags: Vec::new(),
            color: "#3dd6a4".into(),
            category: crate::hosts::Category::Shell,
            db_type: None,
        }
    }

    #[test]
    fn un_log_sin_corchetes_no_sale_todo_en_info() {
        // `of` busca `[NIVEL]` porque ése es el formato de lucy_app.log. Sobre un
        // IIS o un syslog no hay corchetes en ninguna parte, así que devolvía
        // Info PARA TODO: el visor pintaba tres mil líneas del mismo color y el
        // contador de errores marcaba cero con el fichero lleno de fallos.
        assert_eq!(Level::sniff("sshd: failed password for admin"), Level::Error);
        assert_eq!(Level::sniff("kernel: I/O exception on sda"), Level::Error);
        assert_eq!(Level::sniff("acceso denegado al recurso"), Level::Error);
        assert_eq!(Level::sniff("TLS 1.0 is deprecated"), Level::Warn);
        assert_eq!(Level::sniff("Advertencia: quedan 2 GB"), Level::Warn);
        assert_eq!(Level::sniff("reload completado en 4.2 s"), Level::Info);
    }

    #[test]
    fn sniff_no_tine_de_rojo_las_lineas_que_dicen_que_todo_va_bien() {
        // Buscar `error` como subcadena casa dentro de `no-errors`,
        // `ErrorActionPreference` y `sin errores`, y entonces prácticamente todo
        // el fichero sale en rojo — que es igual de inútil que todo en gris.
        assert_eq!(Level::sniff("reload completado — 0 no-errors"), Level::Info);
        assert_eq!(Level::sniff("set ErrorActionPreference=Stop"), Level::Info);
        // Pero la palabra suelta sí cuenta, aunque venga pegada a puntuación.
        assert_eq!(Level::sniff("resultado: ERROR."), Level::Error);
        assert_eq!(Level::sniff("[error] algo"), Level::Error);
    }

    #[test]
    fn el_mismo_fichero_se_lee_igual_por_los_dos_caminos() {
        // Una línea de lucy_app.log abierta en el modo Archivo tiene que dar el
        // mismo nivel que en el de Auditoría. Dos criterios sobre el mismo
        // fichero es cómo se llega a que la misma línea sea un aviso en una
        // pestaña y un error en la de al lado.
        for l in [
            "[2026-01-01] [ERROR] fallo",
            "[2026-01-01] [WARN] ojo",
            "[2026-01-01] [WARNING] ojo",
            "[2026-01-01] [INFO] arranque",
        ] {
            assert_eq!(Level::sniff(l), Level::of(l), "{l}");
        }
    }

    #[test]
    fn la_ruta_no_puede_cerrar_la_comilla_que_la_envuelve() {
        // No da un error: da una ejecución de más, que es la clase de fallo que
        // solo se descubre mirando qué corrió.
        let veneno = "C:\\logs\\a'; Invoke-Expression 'calc'; $x='";
        let s = remote_script(&host(crate::hosts::Protocol::Winrm), veneno, 200).unwrap();
        assert!(s.contains("''"), "no dobló la comilla: {s}");
        // El literal que abre `-Path '` tiene que seguir abierto hasta el que lo
        // cierra: un número IMPAR de comillas sueltas significaría lo contrario.
        assert_eq!(s.matches('\'').count() % 2, 0, "{s}");

        let posix = remote_script(&host(crate::hosts::Protocol::Ssh), "/var/log/a'; id; '", 50)
            .unwrap();
        assert!(posix.contains("'\\''"), "escapado POSIX mal hecho: {posix}");
    }

    #[test]
    fn una_ruta_que_no_existe_tiene_que_fallar_y_no_volver_vacia() {
        // `-ErrorAction Stop` DENTRO del ScriptBlock. El
        // `$ErrorActionPreference` del envoltorio es local al proceso que lanza
        // la conexión, no al bloque que corre al otro lado: sin esto, una ruta
        // inexistente vuelve con código de salida correcto y cero líneas, y el
        // visor dice «log vacío» donde debería decir «ese fichero no está».
        let s = remote_script(&host(crate::hosts::Protocol::Winrm), "C:\\x.log", 200).unwrap();
        assert!(s.contains("-ErrorAction Stop"), "{s}");
    }

    #[test]
    fn cada_protocolo_lleva_su_comando_y_los_demas_se_rechazan() {
        let win = remote_script(&host(crate::hosts::Protocol::Winrm), "C:\\x.log", 10).unwrap();
        assert!(win.starts_with("Get-Content"), "{win}");
        let ssh = remote_script(&host(crate::hosts::Protocol::Ssh), "/var/log/syslog", 10).unwrap();
        assert!(ssh.starts_with("tail -n 10 "), "{ssh}");

        // Un equipo dado de alta como base de datos no lee ficheros, y decirlo
        // es mejor que mandarle un `tail` que no va a entender.
        let e = remote_script(&host(crate::hosts::Protocol::Redis), "/var/log/x", 10).unwrap_err();
        assert!(e.contains("WinRM o SSH"), "{e}");
    }

    #[test]
    fn el_tope_de_lineas_se_respeta_tambien_en_remoto() {
        // Sin esto, pedir un millón de líneas trae el fichero entero por la red
        // y lo mete en memoria — el mismo tope que ya se aplica en local.
        let s = remote_script(&host(crate::hosts::Protocol::Ssh), "/var/log/x", 999_999).unwrap();
        assert!(s.contains(&format!("tail -n {MAX_LINES} ")), "{s}");
        // Y cero líneas no es una petición válida: devolvería un comando que no
        // hace nada y una lista vacía indistinguible de un log vacío.
        let z = remote_script(&host(crate::hosts::Protocol::Ssh), "/var/log/x", 0).unwrap();
        assert!(z.contains("tail -n 1 "), "{z}");
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
