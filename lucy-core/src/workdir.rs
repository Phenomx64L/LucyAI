//! Dónde trabaja Lucy cuando no se le dice dónde.
//!
//! EL FALLO QUE ESTO CIERRA, contado por quien lo sufrió: «tiene tendencia a
//! usar su ruta de deploy y eso provoca que ciertos archivos se escriban en el
//! proyecto». Y no era una tendencia del modelo: se lo estábamos ORDENANDO. El
//! prompt de sistema emitía en cada turno
//!
//! ```text
//! Directorio de trabajo: {}. Cuando el operador nombre un fichero sin ruta
//! completa, resuélvelo respecto a este directorio.
//! ```
//!
//! con el valor de `std::env::current_dir()` — es decir, la carpeta desde la que
//! se lanzó el ejecutable. Instalada, `C:\Program Files\Lucy`. Lanzada con
//! `cargo run`, la carpeta del proyecto. Ninguna de las dos es donde alguien
//! quiere que le dejen sus ficheros, y la segunda explica por qué aparecían
//! dentro del repositorio.
//!
//! CUATRO SITIOS LO USABAN Y NINGUNO ESTABA DE ACUERDO CON OTRO:
//!
//!   · el prompt, que se lo decía al modelo;
//!   · `tools::resuelve`, que resolvía las rutas de las herramientas de fichero
//!     mirando la carpeta personal y NO el directorio del proceso — o sea que
//!     hacía lo contrario de lo que el prompt prometía;
//!   · `shell::run_powershell_utf8`, que lanza TODOS los comandos que Lucy
//!     ejecuta, y los lanzaba en la carpeta de instalación. Ése es el que de
//!     verdad escribe ficheros donde no toca: un `New-Item informe.txt` acababa
//!     dentro de Archivos de programa;
//!   · el PTY de NexShell, que abría la terminal en la carpeta de instalación.
//!
//! Ahora hay UN valor, lo elige el operador, y los cuatro lo leen de aquí.
//!
//! POR QUÉ EN EL NÚCLEO Y NO EN EL ALMACÉN DEL SHELL. `tools::resuelve` es una
//! función libre sin acceso a la aplicación, y `shell::run_powershell_utf8`
//! también. Pasarlo por parámetro obligaría a cambiar la firma de todo lo que
//! toca un fichero y a llevarlo por toda la pila. Y guardarlo solo en el almacén
//! de eframe tiene un agujero medible: ese almacén se vuelca cada treinta
//! segundos, así que configurar el directorio y cerrar Lucy de golpe lo perdía.
//! Aquí se escribe en la base al momento, y lo comparten las dos versiones.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// El valor vivo. `None` = todavía no se ha leído de la base.
///
/// EN MEMORIA Y NO UNA CONSULTA POR LLAMADA porque `resuelve` se llama en cada
/// herramienta de fichero y `run_powershell_utf8` en cada comando. Una ida a
/// SQLite por cada una sería pagar una escritura de disco para contestar algo
/// que cambia tres veces al año.
static ACTUAL: RwLock<Option<Estado>> = RwLock::new(None);

#[derive(Debug, Clone, PartialEq)]
struct Estado {
    /// Lo que eligió el operador. `None` = no ha elegido y vale el de por defecto.
    elegido: Option<PathBuf>,
}

/// El directorio de trabajo que rige ahora mismo.
///
/// NUNCA DEVUELVE EL DIRECTORIO DEL PROCESO, que es el fallo que este módulo
/// viene a cerrar. Sin elección del operador vale su carpeta personal: no es una
/// gran respuesta, pero es una respuesta de la que él es dueño, y cualquiera de
/// las dos alternativas —la de instalación o la del proyecto— es peor.
pub fn actual() -> PathBuf {
    configurado().unwrap_or_else(por_defecto)
}

/// Lo que el operador eligió, o `None` si no ha elegido.
///
/// Separado de `actual` porque la interfaz necesita distinguirlos: enseñar la
/// carpeta personal como si el operador la hubiera puesto es mentirle sobre el
/// estado de su propia configuración.
pub fn configurado() -> Option<PathBuf> {
    if let Ok(g) = ACTUAL.read() {
        if let Some(e) = g.as_ref() {
            return e.elegido.clone();
        }
    }
    // Primera lectura del proceso: se trae de la base y se cachea.
    carga()
}

/// Trae el valor guardado a memoria. Idempotente.
///
/// Devuelve lo que hubiera guardado. Se llama sola en la primera lectura, así
/// que el shell no tiene que acordarse — pero puede llamarla al arrancar para
/// que el primer turno no pague la consulta.
pub fn carga() -> Option<PathBuf> {
    let guardado = lee_de_la_base();
    // SE CACHEA AUNQUE NO HAYA NADA. Si no, cada llamada sin directorio
    // configurado —que es el caso por defecto— vuelve a preguntar a SQLite.
    if let Ok(mut g) = ACTUAL.write() {
        *g = Some(Estado { elegido: guardado.clone() });
    }
    guardado
}

/// Elige el directorio de trabajo. Valida, guarda y lo deja rigiendo.
///
/// Devuelve la ruta ya limpia —absoluta y sin `..`— que es la que hay que
/// enseñar: si el operador escribió `C:\proyectos\..\datos`, lo que va a ver en
/// el prompt y en los artefactos es `C:\datos`, y conviene que lo vea antes.
pub fn pon(p: &Path) -> Result<PathBuf, String> {
    let limpia = valida(p)?;
    guarda_en_la_base(&limpia)?;
    if let Ok(mut g) = ACTUAL.write() {
        *g = Some(Estado { elegido: Some(limpia.clone()) });
    }
    Ok(limpia)
}

/// Vuelve al de por defecto.
pub fn olvida() -> Result<(), String> {
    borra_de_la_base()?;
    if let Ok(mut g) = ACTUAL.write() {
        *g = Some(Estado { elegido: None });
    }
    Ok(())
}

/// Si una carpeta sirve como directorio de trabajo, y en qué forma se guarda.
///
/// SE RECHAZA LA CARPETA DE INSTALACIÓN, y no es celo: es exactamente la trampa
/// de la que sale todo esto. Poner ahí el directorio de trabajo a propósito
/// reproduce el fallo que se acaba de arreglar, y encima con la bendición del
/// operador, que ya no tendría a quién culpar cuando sus ficheros no aparezcan.
/// Además hace falta ser administrador para escribir, así que la mitad de los
/// intentos morirían con acceso denegado.
pub fn valida(p: &Path) -> Result<PathBuf, String> {
    if p.as_os_str().is_empty() {
        return Err("No se dijo qué carpeta.".into());
    }
    // ABSOLUTA Y SIN `..` ANTES DE MIRAR NADA. Una relativa aquí se resolvería
    // contra el directorio del proceso —lo único que este módulo existe para
    // evitar— y guardarla dejaría el fallo dentro de su propio arreglo.
    let abs = std::path::absolute(p).map_err(|e| format!("Ruta que no se entiende: {e}"))?;
    let limpia = sin_prefijo_largo(&abs);

    let meta = std::fs::metadata(&limpia)
        .map_err(|e| format!("No se puede usar «{}»: {e}", limpia.display()))?;
    if !meta.is_dir() {
        return Err(format!("«{}» es un fichero, no una carpeta.", limpia.display()));
    }
    if es_la_instalacion(&limpia) {
        return Err(format!(
            "«{}» es la carpeta donde está instalada Lucy. Ahí hace falta ser \
             administrador para escribir, y es justamente donde acababan los ficheros \
             que se perdían. Elige una carpeta tuya.",
            limpia.display()
        ));
    }
    Ok(limpia)
}

/// Si una ruta es la carpeta del ejecutable o cuelga de ella.
fn es_la_instalacion(p: &Path) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(dir) = exe.parent() else {
        return false;
    };
    let dir = sin_prefijo_largo(dir);
    // `starts_with` compara por COMPONENTES y no por texto, así que
    // `C:\Program Files\Lucy-datos` no cuenta como dentro de `C:\Program Files\Lucy`.
    // Con una comparación de cadenas sí contaría, y sería un rechazo que el
    // operador no podría explicarse.
    p.starts_with(&dir)
}

/// Quita el prefijo `\\?\` que mete `canonicalize` en Windows.
///
/// NO ES COSMÉTICA. Esa ruta va al prompt del modelo, a la etiqueta de Trace y
/// al panel de Artefactos. Un `\\?\C:\Users\...` en medio de una frase le enseña
/// al modelo a escribirlo de vuelta, y hay herramientas de Windows que no lo
/// aceptan. `absolute` no lo añade, pero sí lo tiene cualquier ruta que venga de
/// un `canonicalize` de otro sitio.
fn sin_prefijo_largo(p: &Path) -> PathBuf {
    let s = p.display().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(limpio) => PathBuf::from(limpio),
        None => p.to_path_buf(),
    }
}

/// El de por defecto: la carpeta personal del operador.
///
/// Y si no hay ni eso —un servicio sin perfil—, la temporal. Lo que NO puede
/// ser, bajo ningún camino, es `std::env::current_dir()`.
fn por_defecto() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(std::env::temp_dir)
}

// ── Persistencia ─────────────────────────────────────────────────────────────

pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            // UNA FILA, y el `CHECK` lo garantiza en el motor y no en el código
            // que escribe. Con dos filas, cuál rige lo decidiría el orden de
            // lectura, que es lo mismo que decir que lo decide el azar.
            "CREATE TABLE IF NOT EXISTS work_dir (
                 id   INTEGER PRIMARY KEY CHECK (id = 1),
                 ruta TEXT NOT NULL
             );",
        )
        .map_err(|e| e.to_string())
    })
}

/// NO DEVUELVE `Result`. Quien llama a esto está resolviendo una ruta o
/// lanzando un comando, y un fallo de base de datos no puede impedirlo: sin
/// directorio guardado rige el de por defecto, que es una respuesta correcta y
/// es exactamente lo que había antes de que este módulo existiera.
fn lee_de_la_base() -> Option<PathBuf> {
    ensure_schema().ok()?;
    crate::with_db(|c| {
        let mut st = c
            .prepare("SELECT ruta FROM work_dir WHERE id = 1")
            .map_err(|e| e.to_string())?;
        Ok(st.query_row([], |r| r.get::<_, String>(0)).ok())
    })
    .ok()
    .flatten()
    .map(PathBuf::from)
    // SE REVALIDA AL LEER. El operador pudo elegir una carpeta de un disco USB,
    // o borrarla, o la puso en una unidad de red que hoy no está montada. Un
    // directorio de trabajo que no existe convierte cada escritura en un error
    // raro; volver al de por defecto es peor de lo que eligió y mejor que nada.
    .filter(|p| p.is_dir())
}

fn guarda_en_la_base(p: &Path) -> Result<(), String> {
    ensure_schema()?;
    let ruta = p.display().to_string();
    crate::with_db(|c| {
        c.execute(
            "INSERT INTO work_dir (id, ruta) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET ruta = ?1",
            rusqlite::params![ruta],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    })
}

fn borra_de_la_base() -> Result<(), String> {
    ensure_schema()?;
    crate::with_db(|c| {
        c.execute("DELETE FROM work_dir WHERE id = 1", [])
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_de_por_defecto_no_es_nunca_el_del_proceso() {
        // LA PROPIEDAD ENTERA DEL MÓDULO, en una línea. Si esto cae, todo lo
        // demás da igual: los ficheros vuelven a la carpeta de instalación.
        let d = por_defecto();
        assert!(d.is_absolute(), "el de por defecto no es absoluto: {}", d.display());
        if let Ok(proceso) = std::env::current_dir() {
            // Salvo que el operador tenga a Lucy dentro de su propia carpeta
            // personal, que es legítimo y no es lo que se está midiendo.
            if proceso != por_defecto() {
                assert_ne!(d, proceso, "el de por defecto es el directorio del proceso");
            }
        }
    }

    #[test]
    fn una_carpeta_que_no_existe_se_rechaza_con_su_motivo() {
        let e = valida(Path::new(r"C:\esto-no-existe-de-lucy\ni-esto")).unwrap_err();
        assert!(e.contains("No se puede usar"), "el motivo no ayuda: {e}");
    }

    #[test]
    fn un_fichero_no_es_una_carpeta() {
        // Se elige con un diálogo de carpetas, pero el campo de texto acepta
        // cualquier cosa — y pegar la ruta de un fichero es lo natural cuando lo
        // que uno tiene a mano es el fichero.
        let f = std::env::temp_dir().join("lucy-esto-es-un-fichero.txt");
        std::fs::write(&f, "x").unwrap();
        let e = valida(&f).unwrap_err();
        assert!(e.contains("fichero"), "no dice que es un fichero: {e}");
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn la_carpeta_de_instalacion_se_rechaza() {
        // ES LA TRAMPA DE LA QUE SALE TODO ESTO. Poner ahí el directorio de
        // trabajo a propósito reproduce el fallo con la bendición del operador.
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let Some(dir) = exe.parent() else {
            return;
        };
        let e = valida(dir).unwrap_err();
        assert!(e.contains("instalada"), "no dice por qué se rechaza: {e}");
    }

    #[test]
    fn una_relativa_se_vuelve_absoluta_antes_de_guardarse() {
        // Guardar una relativa metería el fallo DENTRO de su propio arreglo: se
        // resolvería contra el directorio del proceso en cada lectura.
        let temp = std::env::temp_dir();
        std::env::set_current_dir(&temp).unwrap();
        let v = valida(Path::new(".")).unwrap();
        assert!(v.is_absolute(), "se guardaría una ruta relativa: {}", v.display());
    }

    #[test]
    fn el_prefijo_largo_de_windows_no_llega_al_prompt() {
        // `\\?\C:\Users\…` en medio de una frase del prompt le enseña al modelo
        // a escribirlo de vuelta, y hay herramientas de Windows que no lo comen.
        let p = sin_prefijo_largo(Path::new(r"\\?\C:\Users\alguien\proyecto"));
        assert_eq!(p, PathBuf::from(r"C:\Users\alguien\proyecto"));
        // Y una ruta normal no se toca.
        let n = sin_prefijo_largo(Path::new(r"C:\Users\alguien"));
        assert_eq!(n, PathBuf::from(r"C:\Users\alguien"));
    }

    #[test]
    fn una_carpeta_de_al_lado_no_cuenta_como_la_instalacion() {
        // `starts_with` compara por COMPONENTES. Con una comparación de cadenas,
        // `C:\Program Files\Lucy-datos` empezaría por `C:\Program Files\Lucy` y
        // se rechazaría — un «no puedes usar esa carpeta» inexplicable.
        assert!(!Path::new(r"C:\Program Files\Lucy-datos")
            .starts_with(Path::new(r"C:\Program Files\Lucy")));
        assert!(Path::new(r"C:\Program Files\Lucy\skills")
            .starts_with(Path::new(r"C:\Program Files\Lucy")));
    }
}
