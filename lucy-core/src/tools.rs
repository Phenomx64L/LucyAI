//! Las herramientas de LECTURA que Lucy puede pedir.
//!
//! LO QUE ESTO ARREGLA. El shell nativo reconocía `<TOOL>readfile:…</TOOL>`,
//! lo anotaba en el carril de Trace, y no hacía nada más. Lucy veía su propia
//! petición en pantalla y no le volvía nada, así que o insistía o se inventaba
//! el contenido. Anotar una petición sin cumplirla es peor que no reconocerla:
//! parece que funcionó.
//!
//! SOLO LECTURA, y a propósito. `writefile` y `editfile` existen en la V2 y no
//! están aquí: escribir en el disco de alguien necesita la misma puerta que
//! ejecutar un comando —que la vea una persona, o que el guardrail la deje
//! pasar— y colarlas junto a las lecturas sería meter la decisión difícil dentro
//! del sí que se dio a la fácil. El artefacto que hoy dice «propuesto — sin
//! escribir» seguirá diciendo la verdad hasta que esa puerta exista.
//!
//! NO HAY RESTRICCIÓN DE RUTA. Lucy es una herramienta de administración: leer
//! `C:\Windows\System32\drivers\etc\hosts` o un log de IIS es su trabajo. Lo que
//! sí hay son topes de TAMAÑO y detección de binario, que es de lo que protege
//! de verdad — un `readfile` sobre un .vhdx no es un problema de permisos, es
//! cuarenta gigas hacia una petición HTTP.

/// Tope del texto que vuelve al modelo.
///
/// El mismo que aplica la V2. No es el tamaño del fichero: es lo que cabe en un
/// turno sin comerse la conversación entera, y lo que se paga por leerlo.
pub const MAX_CHARS: usize = 16_000;

/// Tope de lo que se lee del disco, en bytes.
///
/// Se mira ANTES de abrir. El sentido del tope es no tener el fichero en
/// memoria, y comprobarlo después de cargarlo ya lo tuvo.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Cuántas entradas devuelve un listado.
///
/// `System32` tiene más de cinco mil. Volcarlas todas al modelo cuesta más que
/// la pregunta que las motivó y no ayuda a contestarla.
pub const MAX_ENTRIES: usize = 300;

/// El resultado de una herramienta, ya listo para volver al modelo.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    /// Cabecera corta para el carril de Trace.
    pub label: String,
    /// Lo que se le manda al modelo.
    pub body: String,
    pub ok: bool,
}

impl ToolResult {
    fn err(label: impl Into<String>, msg: impl Into<String>) -> Self {
        Self { label: label.into(), body: msg.into(), ok: false }
    }
}

/// Ejecuta una herramienta por nombre. `None` = este shell no la conoce.
///
/// Devuelve `None` en vez de un error para que quien llama pueda distinguir «la
/// intenté y falló» de «no la tengo». Lo segundo hay que decírselo al modelo de
/// otra forma: si le contestas «error» a una herramienta que no existe, la
/// vuelve a intentar con otros argumentos.
pub fn run(name: &str, args: &str) -> Option<ToolResult> {
    match name {
        "readfile" => Some(readfile(args)),
        "listdir" => Some(listdir(args)),
        _ => None,
    }
}

/// Las que este shell sabe cumplir, para poder nombrarlas en el prompt.
///
/// Prometer en el prompt una herramienta que no está es cómo se llega a que
/// Lucy pida cosas que nadie va a hacer — el fallo del que viene todo esto.
pub const AVAILABLE: &[(&str, &str)] = &[
    ("readfile", "<TOOL>readfile:C:\\ruta\\fichero.log</TOOL> — te devuelve su texto"),
    ("listdir", "<TOOL>listdir:C:\\ruta</TOOL> — te devuelve qué hay en esa carpeta"),
];

fn readfile(path: &str) -> ToolResult {
    let path = path.trim();
    let p = std::path::Path::new(path);
    let label = format!("readfile {path}");

    let meta = match std::fs::metadata(p) {
        Ok(m) => m,
        // El mensaje lleva la ruta. Sin ella, con tres lecturas en el mismo
        // turno el modelo no sabe cuál de las tres falló.
        Err(e) => return ToolResult::err(label, format!("No se pudo abrir '{path}': {e}")),
    };
    if meta.is_dir() {
        return ToolResult::err(
            label,
            format!("'{path}' es una carpeta. Usa <TOOL>listdir:{path}</TOOL>."),
        );
    }
    if meta.len() > MAX_BYTES {
        return ToolResult::err(
            label,
            format!(
                "'{path}' pesa {:.1} MB y el máximo son {} MB. Si es un log, lee el final \
                 con Get-Content -Tail.",
                meta.len() as f64 / 1_048_576.0,
                MAX_BYTES / 1_048_576
            ),
        );
    }

    let bytes = match std::fs::read(p) {
        Ok(b) => b,
        Err(e) => return ToolResult::err(label, format!("No se pudo leer '{path}': {e}")),
    };
    // ¿ES TEXTO? va ANTES que ¿en qué codificación?, y ese orden importa más de
    // lo que parece: un ejecutable empieza por `MZ` seguido de ceros, y todos
    // esos bytes son menores que 0x80, así que el fichero es UTF-8 PERFECTAMENTE
    // VÁLIDO. Comprobando la codificación primero, un .exe pasaba por texto y se
    // le mandaba al modelo lleno de caracteres nulos para que sacara
    // conclusiones sobre el ruido.
    if pinta_binario(&bytes) {
        return ToolResult::err(label, format!("'{path}' es un fichero binario, no texto."));
    }
    // Y ya sabiendo que es texto: UTF-8, y si no, la consola. Es lo que escriben
    // las herramientas nativas de Windows, y un volcado de `wevtutil` leído como
    // UTF-8 sale con rombos donde iban las tildes.
    let texto = match std::str::from_utf8(&bytes) {
        Ok(s) => s.to_string(),
        Err(_) => crate::shell::decode_console(&bytes),
    };

    let total = texto.chars().count();
    let (cuerpo, nota) = if total > MAX_CHARS {
        (
            texto.chars().take(MAX_CHARS).collect::<String>(),
            format!(
                "\n\n[…recortado: {total} caracteres en total, se te mandan los primeros \
                 {MAX_CHARS}…]"
            ),
        )
    } else {
        (texto, String::new())
    };
    ToolResult { label, body: format!("{cuerpo}{nota}"), ok: true }
}

/// Si unos bytes parecen un ejecutable o similar en vez de texto mal codificado.
///
/// Un NUL en los primeros mil bytes. Es la heurística que usa `git` y acierta:
/// ningún texto real lleva un byte cero, y todos los formatos binarios llevan
/// varios muy pronto.
fn pinta_binario(bytes: &[u8]) -> bool {
    bytes.iter().take(1024).any(|b| *b == 0)
}

fn listdir(path: &str) -> ToolResult {
    let path = path.trim();
    let label = format!("listdir {path}");
    let rd = match std::fs::read_dir(path) {
        Ok(r) => r,
        Err(e) => return ToolResult::err(label, format!("No se pudo listar '{path}': {e}")),
    };

    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut total = 0usize;
    for e in rd.flatten() {
        total += 1;
        if total > MAX_ENTRIES {
            continue;
        }
        let nombre = e.file_name().to_string_lossy().into_owned();
        match e.metadata() {
            Ok(m) if m.is_dir() => dirs.push(format!("{nombre}/")),
            // El TAMAÑO va en el listado. Sin él, decidir qué leer después es
            // adivinar, y la siguiente herramienta se gasta en un fichero de
            // cuatro gigas.
            Ok(m) => files.push(format!("{nombre} ({})", tamano(m.len()))),
            Err(_) => files.push(nombre),
        }
    }
    dirs.sort();
    files.sort();

    let mut body = String::new();
    for d in &dirs {
        body.push_str(d);
        body.push('\n');
    }
    for f in &files {
        body.push_str(f);
        body.push('\n');
    }
    if total > MAX_ENTRIES {
        body.push_str(&format!(
            "\n[…{} entradas en total, se te mandan {MAX_ENTRIES}…]",
            total
        ));
    }
    if body.is_empty() {
        body = "(carpeta vacía)".into();
    }
    ToolResult { label, body, ok: true }
}

fn tamano(bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K {
        format!("{bytes} B")
    } else if b < K * K {
        format!("{:.1} KB", b / K)
    } else if b < K * K * K {
        format!("{:.1} MB", b / (K * K))
    } else {
        format!("{:.1} GB", b / (K * K * K))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(nombre: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(nombre)
    }

    #[test]
    fn una_herramienta_desconocida_se_distingue_de_una_que_fallo() {
        // Si a una herramienta que no existe se le contesta "error", el modelo
        // la reintenta con otros argumentos y se come el presupuesto de pasos.
        assert!(run("graphify", "lo que sea").is_none());
        assert!(run("writefile", "x|||y").is_none(), "escribir no está y no debe fingirse");
        assert!(run("listdir", ".").is_some());
    }

    #[test]
    fn un_fichero_de_texto_vuelve_entero() {
        let p = tmp("lucy_tool_ok.txt");
        std::fs::write(&p, "línea uno\nlínea dos").unwrap();
        let r = readfile(&p.to_string_lossy());
        let _ = std::fs::remove_file(&p);
        assert!(r.ok, "{}", r.body);
        assert!(r.body.contains("línea dos"));
    }

    #[test]
    fn un_fichero_enorme_se_recorta_y_lo_dice() {
        // Recortar en silencio hace que el modelo saque conclusiones sobre un
        // fichero que cree haber leído entero.
        let p = tmp("lucy_tool_grande.txt");
        std::fs::write(&p, "x".repeat(MAX_CHARS + 5_000)).unwrap();
        let r = readfile(&p.to_string_lossy());
        let _ = std::fs::remove_file(&p);
        assert!(r.ok);
        assert!(r.body.contains("recortado"), "no avisa del recorte");
    }

    #[test]
    fn un_binario_se_rechaza_en_vez_de_mandar_ruido() {
        // Decirle al modelo que un .exe es texto mal codificado le hace
        // analizarlo y concluir cosas sobre el ruido.
        let p = tmp("lucy_tool_bin.dat");
        std::fs::write(&p, [0x4D, 0x5A, 0x00, 0x00, 0x03, 0x00]).unwrap();
        let r = readfile(&p.to_string_lossy());
        let _ = std::fs::remove_file(&p);
        assert!(!r.ok);
        assert!(r.body.contains("binario"), "{}", r.body);
    }

    #[test]
    fn una_carpeta_leida_como_fichero_sugiere_la_herramienta_buena() {
        // Un "no se pudo leer" a secas hace que el modelo lo reintente igual.
        // Nombrarle la herramienta correcta cierra el paso en una vuelta.
        let r = readfile(&std::env::temp_dir().to_string_lossy());
        assert!(!r.ok);
        assert!(r.body.contains("listdir"), "{}", r.body);
    }

    #[test]
    fn el_error_lleva_la_ruta_que_fallo() {
        // Con tres lecturas en el mismo turno, un error sin ruta no dice cuál.
        let r = readfile("C:\\no-existe-este-fichero-de-lucy.log");
        assert!(!r.ok);
        assert!(r.body.contains("no-existe-este-fichero-de-lucy.log"), "{}", r.body);
    }

    #[test]
    fn un_listado_marca_las_carpetas_y_da_los_tamanos() {
        // El tamaño no es adorno: es lo que evita que la siguiente lectura se
        // gaste en un fichero de cuatro gigas.
        let d = tmp("lucy_tool_dir");
        let _ = std::fs::create_dir_all(d.join("subcarpeta"));
        std::fs::write(d.join("dato.txt"), "hola").unwrap();
        let r = listdir(&d.to_string_lossy());
        let _ = std::fs::remove_dir_all(&d);
        assert!(r.ok, "{}", r.body);
        assert!(r.body.contains("subcarpeta/"), "las carpetas no se distinguen: {}", r.body);
        assert!(r.body.contains("dato.txt (4 B)"), "{}", r.body);
    }

    #[test]
    fn los_tamanos_se_leen_como_los_lee_una_persona() {
        assert_eq!(tamano(512), "512 B");
        assert_eq!(tamano(2048), "2.0 KB");
        assert_eq!(tamano(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(tamano(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}
