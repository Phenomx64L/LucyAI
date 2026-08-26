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

/// Convierte lo que diga el modelo en una ruta absoluta, SIN pasar por el
/// directorio de trabajo del proceso.
///
/// POR QUÉ NO VALE DEJÁRSELO A `std::fs`. Una ruta relativa se resuelve contra
/// el directorio de trabajo, y el de Lucy instalada es `C:\Program Files\Lucy`
/// —de ahí arranca el acceso directo—. Nadie ha guardado nunca un informe ahí.
/// Así que `readfile:informe.txt` fallaba con «no se encuentra el archivo» sin
/// decir DÓNDE había mirado, y el modelo lo reintentaba igual; y
/// `writefile:notas.txt` preparaba un artefacto que, al aplicarlo, o moría con
/// acceso denegado o —con Lucy elevada— escribía dentro de Archivos de programa,
/// que es peor: sale bien y el fichero no aparece donde se esperaba.
///
/// LO QUE SE HACE EN SU LUGAR. Una relativa se busca PRIMERO en el directorio de
/// trabajo —el que el operador eligió; ver `crate::workdir`— y después en las
/// tres carpetas donde de verdad están sus ficheros. Gana la primera que exista;
/// si no existe en ninguna, se crea en el directorio de trabajo. Y la ruta que
/// sale de aquí es la que se le enseña al modelo y la que va al artefacto, así
/// que la elección se ve en el carril de Trace y en el diff — no se decide a
/// escondidas.
///
/// EL DIRECTORIO DE TRABAJO VA PRIMERO Y TAMBIÉN ES EL RESPALDO, y las dos cosas
/// importan. Primero, porque es lo que el operador dijo que quería. Y respaldo,
/// porque es la mitad que hace que esto sirva de algo: un fichero NUEVO no está
/// en ninguna parte, así que sin respaldo la elección solo contaría para los que
/// ya existen — justo al revés de lo que se pidió, que era no tener que sondear
/// la ruta cada vez que hay que escribir algo.
///
/// Las carpetas van en inglés porque Windows traduce el NOMBRE QUE SE MUESTRA y
/// no el del disco: en un Windows en español, «Escritorio» sigue siendo
/// `Desktop` para cualquier cosa que abra un fichero.
pub fn resuelve(path: &str) -> std::path::PathBuf {
    let bruto = path.trim();
    // `~` a secas o `~\algo`. No es sintaxis de Windows, pero el modelo la
    // escribe.
    //
    // Y SOLO ESAS DOS FORMAS. Un `~` pegado a otra cosa NO es la carpeta
    // personal: `~$informe.docx` es el fichero de bloqueo que Word deja al lado
    // de un documento abierto, y en la máquina de un administrador hay varios a
    // la vista. Tratarlo como «carpeta personal + $informe.docx» lo manda a otro
    // sitio, y el error dice que no existe un fichero que sí está.
    if bruto == "~" || bruto.starts_with("~/") || bruto.starts_with("~\\") {
        if let Some(c) = casa() {
            let resto = bruto[1..].trim_start_matches(['\\', '/']);
            return if resto.is_empty() { c } else { c.join(resto) };
        }
    }
    let p = std::path::Path::new(bruto);
    // `has_root` además de `is_absolute` por `\Windows\System32`: no lleva
    // unidad, así que no es absoluta, pero tampoco es relativa a una carpeta
    // personal. Ésa la resuelve el sistema contra la unidad actual y hace bien.
    if p.is_absolute() || p.has_root() {
        return p.to_path_buf();
    }
    let trabajo = crate::workdir::actual();
    if trabajo.join(p).exists() {
        return trabajo.join(p);
    }
    if let Some(c) = casa() {
        for sub in ["", "Desktop", "Documents", "Downloads"] {
            let cand = if sub.is_empty() { c.join(p) } else { c.join(sub).join(p) };
            if cand.exists() {
                return cand;
            }
        }
    }
    // No existe en ninguna parte: es un fichero nuevo, y va donde el operador
    // dijo que trabaja.
    trabajo.join(p)
}

/// La carpeta del operador.
fn casa() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// La ruta ya resuelta, y —si no es la que pidió— cómo decírselo al modelo.
///
/// El aviso va en el CUERPO de la respuesta y no solo en la etiqueta, porque la
/// etiqueta es para el operador y el cuerpo es lo único que lee el modelo. Sin
/// él, pide `informe.txt`, se le contesta con el contenido de otra carpeta, y en
/// el turno siguiente vuelve a escribir la relativa.
fn resuelto(pedido: &str) -> (std::path::PathBuf, String) {
    let p = resuelve(pedido);
    let mostrado = p.display().to_string();
    let aviso = if mostrado.eq_ignore_ascii_case(pedido.trim()) {
        String::new()
    } else {
        format!(
            "\n\n[«{}» es una ruta relativa; se resolvió a {mostrado}. Usa la ruta \
             completa para no depender de esto.]",
            pedido.trim()
        )
    };
    (p, aviso)
}

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

/// Prepara un `writefile:RUTA|||CONTENIDO` sin tocar el disco.
///
/// Devuelve el artefacto con el antes y el después, para que el operador vea
/// exactamente qué cambia antes de decidir. NO escribe: eso es `apply`.
pub fn prepare_write(args: &str) -> crate::agent::Artifact {
    let (path, content) = match args.split_once("|||") {
        Some((p, c)) => (p.trim().to_string(), c.to_string()),
        None => {
            return bloqueado(
                crate::agent::ArtifactKind::Write,
                args.trim(),
                "Falta el separador `|||`. El formato es writefile:RUTA|||CONTENIDO.",
            )
        }
    };
    if path.is_empty() {
        return bloqueado(crate::agent::ArtifactKind::Write, "", "No se dijo qué fichero.");
    }
    // ABSOLUTA ANTES DE GUARDARLA EN EL ARTEFACTO, y no al aplicarlo. `apply`
    // escribe en `a.path` sin volver a mirarlo, así que si aquí se guardara la
    // relativa, el diff que aprueba el operador y el fichero que se escribe
    // podrían no ser el mismo — y lo que se escribe se decidiría por dónde
    // estuviera el proceso, que es `C:\Program Files\Lucy`.
    let path = resuelve(&path).display().to_string();
    // Se lee lo que hay para poder enseñar el diff. Que no exista no es un
    // error —crear un fichero nuevo es escribir— y entonces el "antes" es vacío.
    let before = std::fs::read_to_string(&path).unwrap_or_default();
    let nuevo = before.is_empty() && !std::path::Path::new(&path).exists();
    crate::agent::Artifact {
        id: String::new(),
        kind: crate::agent::ArtifactKind::Write,
        summary: if nuevo {
            format!("crear · {} líneas", content.lines().count())
        } else {
            format!("sobrescribir · {} → {} líneas", before.lines().count(), content.lines().count())
        },
        path,
        before,
        after: content,
        ts: 0,
        applied: false,
        blocked: String::new(),
    }
}

/// Prepara un `editfile:RUTA|||VIEJO|||NUEVO` sin tocar el disco.
///
/// AQUÍ VIVE EL FALLO QUE HUNDE A TODAS LAS HERRAMIENTAS DE EDICIÓN: qué hacer
/// cuando el texto viejo aparece MÁS DE UNA VEZ. Sustituir la primera es lo
/// cómodo y es lo que produce el bug que nadie encuentra — se cambia una línea
/// que se parecía, en otro sitio del fichero, y el diff que el operador aprobó
/// no es el que se aplicó. Aquí es un error: que Lucy dé más contexto.
pub fn prepare_edit(args: &str) -> crate::agent::Artifact {
    let kind = crate::agent::ArtifactKind::Edit;
    let mut partes = args.splitn(3, "|||");
    let (Some(path), Some(viejo), Some(nuevo)) = (partes.next(), partes.next(), partes.next())
    else {
        return bloqueado(
            kind,
            args.trim(),
            "Faltan partes. El formato es editfile:RUTA|||TEXTO_VIEJO|||TEXTO_NUEVO.",
        );
    };
    // Igual que en `prepare_write`: absoluta antes de guardarla, para que el
    // fichero del diff y el fichero que se escribe sean el mismo.
    let path = resuelve(path).display().to_string();
    let before = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => return bloqueado(kind, &path, &format!("No se pudo leer '{path}': {e}")),
    };

    let veces = before.matches(viejo).count();
    if veces == 0 {
        return bloqueado(
            kind,
            &path,
            "El texto que quieres sustituir no aparece en el fichero. Léelo con \
             readfile y copia el fragmento exacto.",
        );
    }
    if veces > 1 {
        return bloqueado(
            kind,
            &path,
            &format!(
                "El texto aparece {veces} veces y no se puede saber cuál cambiar. \
                 Incluye más contexto —la línea de antes y la de después— para que sea único."
            ),
        );
    }
    let after = before.replacen(viejo, nuevo, 1);
    crate::agent::Artifact {
        id: String::new(),
        kind,
        summary: format!("editar · {} → {} líneas", before.lines().count(), after.lines().count()),
        path,
        before,
        after,
        ts: 0,
        applied: false,
        blocked: String::new(),
    }
}

fn bloqueado(
    kind: crate::agent::ArtifactKind,
    path: &str,
    por_que: &str,
) -> crate::agent::Artifact {
    crate::agent::Artifact {
        id: String::new(),
        kind,
        path: path.to_string(),
        summary: "no se puede aplicar".into(),
        before: String::new(),
        after: String::new(),
        ts: 0,
        applied: false,
        blocked: por_que.to_string(),
    }
}

/// Escribe el artefacto en el disco. Es lo único de este módulo que MODIFICA.
///
/// Se comprueba que el fichero no haya cambiado desde que se preparó la
/// propuesta. Entre que Lucy la propone y el operador la aprueba pueden pasar
/// minutos, y en ese hueco cabe otro editor guardando: aplicar el `after` tal
/// cual borraría ese trabajo sin que nadie se enterara.
pub fn apply(a: &crate::agent::Artifact) -> Result<(), String> {
    if !a.blocked.is_empty() {
        return Err(a.blocked.clone());
    }
    let actual = std::fs::read_to_string(&a.path).unwrap_or_default();
    if actual != a.before {
        return Err(format!(
            "'{}' ha cambiado desde que se preparó este cambio. Vuelve a leerlo antes \
             de escribir.",
            a.path
        ));
    }
    if let Some(dir) = std::path::Path::new(&a.path).parent() {
        if !dir.as_os_str().is_empty() && !dir.exists() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("No se pudo crear la carpeta '{}': {e}", dir.display()))?;
        }
    }
    std::fs::write(&a.path, &a.after)
        .map_err(|e| format!("No se pudo escribir '{}': {e}", a.path))
}

/// Ejecuta una herramienta por nombre. `None` = este shell no la conoce.
///
/// Devuelve `None` en vez de un error para que quien llama pueda distinguir «la
/// intenté y falló» de «no la tengo». Lo segundo hay que decírselo al modelo de
/// otra forma: si le contestas «error» a una herramienta que no existe, la
/// vuelve a intentar con otros argumentos.
pub fn run(name: &str, args: &str) -> Option<ToolResult> {
    // EL CATÁLOGO SE MIRA ANTES DEL DESPACHO, para que `DE_LECTURA` sea la lista
    // de verdad y no una copia que se parece. Quien quiera saber qué cumple esta
    // función —el prompt de un sub-agente, un test— lee la constante y acierta.
    if !DE_LECTURA.contains(&name) {
        return None;
    }
    Some(match name {
        "readfile" => readfile(args),
        "listdir" => listdir(args),
        // `readlines` estaba ESCRITA, PROBADA, ANUNCIADA EN EL PROMPT Y NO
        // DESPACHADA: se añadió como la salida del callejón de `readfile`
        // —«recorté, sigue con readlines»— y esta línea no llegó a existir. El
        // efecto es el peor de los posibles: el propio aviso del recorte le
        // dice al modelo cómo continuar, lo intenta, `run` contesta `None`, y
        // el callejón que la herramienta venía a abrir seguía cerrado.
        "readlines" => readlines(args),
        // EL ESLABÓN QUE DECIDE SI LA INGESTA SIRVE. Un manual de trescientas
        // páginas ingerido y una herramienta que Lucy no sabe que existe son lo
        // mismo que no haberlo ingerido — y no da error, da silencio.
        "pdf_search" => pdf_search(args),
        // Un nombre en `DE_LECTURA` sin brazo aquí. `None` y no `unreachable!`:
        // un pánico dentro del bucle de un agente se lleva la aplicación por
        // delante, y esto lo caza un test antes de llegar a nadie.
        _ => return None,
    })
}

/// Lo que `run` cumple por sí sola: leer, sin efectos y sin red.
///
/// ES EL CATÁLOGO DE UNA TAREA AUXILIAR, y vive aquí —al lado de la función que
/// lo cumple— y no en `forks` a propósito. Estaba escrito a mano en el prompt de
/// `forks::system_prompt`: «tienes estas herramientas y ninguna más», y las
/// nombraba. Cuando `pdf_search` entró en `run`, el prompt siguió diciendo tres.
///
/// EL FALLO QUE ESO PRODUCE NO SE PARECE A UN FALLO. La herramienta ESTABA —el
/// sub-agente podía cumplirla— y se le decía que no la tenía. Así que la
/// conversación principal reparte «busca esto en el manual» entre varias tareas,
/// cada una contesta «no puedo, solo leo ficheros», y Lucy concluye que no logró
/// procesar el documento. Con el manual ingerido y la búsqueda funcionando.
///
/// Un catálogo escrito dos veces se desincroniza por el lado que nadie mira.
pub const DE_LECTURA: &[&str] = &["readfile", "listdir", "readlines", "pdf_search"];

/// Busca en los documentos ingeridos.
fn pdf_search(args: &str) -> ToolResult {
    let label = format!("pdf_search {args}");
    match crate::docs::search(args) {
        Err(e) => ToolResult::err(label, e),
        Ok(v) if v.is_empty() => ToolResult::err(
            label,
            // Los que SÍ hay, en el error. Sin la lista, el modelo prueba otras
            // palabras contra un corpus que a lo mejor está vacío, y cada intento
            // cuesta un turno.
            match crate::docs::list() {
                Ok(d) if d.is_empty() => {
                    "No hay ningún documento ingerido todavía. El operador puede añadir uno \
                     desde la pestaña Documentos de Memoria."
                        .to_string()
                }
                Ok(d) => format!(
                    "Nada sobre eso en los documentos ingeridos. Los que hay: {}.",
                    d.iter().map(|x| x.nombre.as_str()).collect::<Vec<_>>().join(", ")
                ),
                Err(_) => "Nada sobre eso en los documentos ingeridos.".to_string(),
            },
        ),
        Ok(v) => {
            let mut body = String::new();
            for (titulo, texto) in &v {
                // CON SU PROCEDENCIA. Un párrafo de manual sin decir de cuál sale
                // se cita con la misma autoridad que un hecho medido en la
                // máquina, y no son lo mismo.
                body.push_str(&format!("--- {titulo} ---\n{texto}\n\n"));
            }
            ToolResult { label, body, ok: true }
        }
    }
}

/// Igual, pero sabiendo qué skills hay instalados.
///
/// UN SKILL ES UNA HERRAMIENTA y no una etiqueta nueva, y eso ahorra media
/// migración: la tubería que recoge el resultado, lo junta con los demás y se lo
/// devuelve al modelo en un solo turno ya existe y ya está probada. Una etiqueta
/// `<SKILL>` habría necesitado su propio camino para hacer exactamente lo mismo.
pub fn run_with_skills(
    name: &str,
    args: &str,
    skills: &[crate::skills::Skill],
) -> Option<ToolResult> {
    if name == "skill" {
        return Some(match crate::skills::find(skills, args) {
            Some(k) => ToolResult {
                label: format!("skill {}", k.name),
                body: format!(
                    "Instrucciones del skill «{}». Síguelas para esta tarea; si algo no \
                     encaja con lo que ves en la máquina, manda lo que ves.\n\n{}",
                    k.name, k.body
                ),
                ok: true,
            },
            // Los que SÍ hay, en el error. Decir solo «no existe» deja al modelo
            // probando nombres, y cada intento cuesta un turno.
            None => ToolResult::err(
                format!("skill {args}"),
                if skills.is_empty() {
                    "No hay ningún skill instalado en este equipo.".to_string()
                } else {
                    format!(
                        "No hay ningún skill llamado «{args}». Los que hay: {}.",
                        skills
                            .iter()
                            .map(|k| k.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                },
            ),
        });
    }
    run(name, args)
}

/// Las que este shell sabe cumplir, para poder nombrarlas en el prompt.
///
/// Prometer en el prompt una herramienta que no está es cómo se llega a que
/// Lucy pida cosas que nadie va a hacer — el fallo del que viene todo esto.
pub const AVAILABLE: &[(&str, &str)] = &[
    ("readfile", "<TOOL>readfile:C:\\ruta\\fichero.log</TOOL> — te devuelve su texto"),
    ("listdir", "<TOOL>listdir:C:\\ruta</TOOL> — te devuelve qué hay en esa carpeta"),
    (
        "readlines",
        "<TOOL>readlines:C:\\ruta|desde|cuántas</TOOL> — un tramo, para ficheros largos",
    ),
    (
        "writefile",
        "<TOOL>writefile:C:\\ruta\\fichero.txt|||CONTENIDO</TOOL> — PROPONE escribirlo",
    ),
    (
        "editfile",
        "<TOOL>editfile:C:\\ruta|||TEXTO_VIEJO|||TEXTO_NUEVO</TOOL> — PROPONE un cambio",
    ),
    // Las dos de sub-agente NO las cumple `run`, las cumple el shell: necesitan
    // hilos, un canal por tarea y un modelo elegido, y nada de eso cabe en una
    // función que devuelve un `ToolResult` de golpe. Están en esta lista porque
    // esta lista es lo que el prompt promete, y el criterio es «alguien lo
    // cumple», no «lo cumple este módulo» — igual que `writefile`.
    (
        "pdf_search",
        "<TOOL>pdf_search:qué buscar</TOOL> — busca en los manuales y guías que el \
         operador ha ingerido",
    ),
    (
        "fork_task",
        "<TOOL>fork_task:nombre|qué tiene que averiguar</TOOL> — lanza una tarea en paralelo",
    ),
    ("wait_task", "<TOOL>wait_task:nombre</TOOL> — recoge lo que averiguó"),
];

/// Cuántas líneas devuelve `readlines` como mucho de una vez.
///
/// Quinientas son unas veinte pantallas: de sobra para entender un tramo, y poco
/// para que quepan tres tramos en un turno sin que la conversación se ahogue.
pub const MAX_LINES: usize = 500;

/// `readlines:ruta|desde|cuántas` — un tramo concreto de un fichero.
///
/// EXISTE PORQUE `readfile` DEJABA UN CALLEJÓN SIN SALIDA. Recortaba a 16 000
/// caracteres y avisaba —«se te mandan los primeros»— sin dar forma de ver el
/// resto. En un log eso es justamente lo peor: lo que se busca casi siempre está
/// al final, y Lucy solo alcanzaba el principio.
///
/// `desde` cuenta DESDE 1, como cualquier editor y como cualquier mensaje de
/// error. Contar desde cero aquí obligaría a restar uno cada vez que alguien
/// dice «mira la línea 342», y esa resta se olvida.
fn readlines(args: &str) -> ToolResult {
    let mut p = args.split('|');
    let pedido = p.next().unwrap_or("").trim();
    let desde: usize = p.next().and_then(|x| x.trim().parse().ok()).unwrap_or(1).max(1);
    let cuantas: usize = p
        .next()
        .and_then(|x| x.trim().parse().ok())
        .unwrap_or(MAX_LINES)
        .clamp(1, MAX_LINES);

    if pedido.is_empty() {
        return ToolResult::err(
            "readlines".to_string(),
            "Falta la ruta: readlines:C:\\ruta|desde|cuántas",
        );
    }
    // La resuelta, y no la que pidió, para que la continuación que se le sugiere
    // abajo apunte al MISMO fichero que se acaba de leer. Con la relativa, el
    // siguiente tramo vuelve a pasar por la búsqueda y puede caer en otro sitio
    // si entretanto apareció uno con ese nombre en una carpeta anterior.
    let path = resuelve(pedido).display().to_string();
    let label = format!("readlines {path} desde {desde}");
    // Se reutiliza `readfile` para no tener dos lectores con dos criterios
    // distintos sobre qué es un binario y qué codificación tiene un log.
    let entero = readfile(&path);
    if !entero.ok {
        return ToolResult { label, ..entero };
    }
    let lineas: Vec<&str> = entero.body.lines().collect();
    let total = lineas.len();
    if desde > total {
        return ToolResult::err(
            label,
            format!("'{path}' tiene {total} líneas; pediste desde la {desde}."),
        );
    }
    let fin = (desde - 1 + cuantas).min(total);
    let tramo = lineas[desde - 1..fin].join("\n");
    // DÓNDE ESTÁ Y CUÁNTO QUEDA, en la propia respuesta. Sin eso, el modelo no
    // sabe si ha llegado al final y o para antes de tiempo o pide un tramo que
    // no existe.
    let nota = if fin < total {
        format!("\n\n[líneas {desde}–{fin} de {total}. Sigue con readlines:{path}|{}|{cuantas}]", fin + 1)
    } else {
        format!("\n\n[líneas {desde}–{fin} de {total} — es el final del fichero]")
    };
    ToolResult { label, body: format!("{tramo}{nota}"), ok: true }
}

fn readfile(pedido: &str) -> ToolResult {
    let (p, aviso) = resuelto(pedido);
    // LA RESUELTA Y NO LA QUE PIDIÓ, en todas partes. El error tiene que decir
    // dónde se miró de verdad: «no se encuentra 'informe.txt'» deja al modelo
    // reintentando lo mismo, y «no se encuentra 'C:\Users\x\informe.txt'» le
    // dice en una línea que se buscó donde él no quería.
    let path = p.display().to_string();
    let label = format!("readfile {path}");

    let meta = match std::fs::metadata(&p) {
        Ok(m) => m,
        // El mensaje lleva la ruta. Sin ella, con tres lecturas en el mismo
        // turno el modelo no sabe cuál de las tres falló.
        Err(e) => {
            return ToolResult::err(label, format!("No se pudo abrir '{path}': {e}{aviso}"))
        }
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

    let bytes = match std::fs::read(&p) {
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
            // CON LA SALIDA, no solo con el aviso. Decir «recortado» y callar
            // deja al modelo sabiendo que le falta algo y sin forma de pedirlo,
            // que en un log es lo peor: lo que se busca suele estar al final.
            format!(
                "\n\n[…recortado: {total} caracteres, {} líneas. Se te mandan los primeros \
                 {MAX_CHARS} caracteres. Para el resto: readlines:{path}|desde|cuántas…]",
                texto.lines().count()
            ),
        )
    } else {
        (texto, String::new())
    };
    ToolResult { label, body: format!("{cuerpo}{nota}{aviso}"), ok: true }
}

/// Si unos bytes parecen un ejecutable o similar en vez de texto mal codificado.
///
/// Un NUL en los primeros mil bytes. Es la heurística que usa `git` y acierta:
/// ningún texto real lleva un byte cero, y todos los formatos binarios llevan
/// varios muy pronto.
fn pinta_binario(bytes: &[u8]) -> bool {
    bytes.iter().take(1024).any(|b| *b == 0)
}

fn listdir(pedido: &str) -> ToolResult {
    let (p, aviso) = resuelto(pedido);
    let path = p.display().to_string();
    let label = format!("listdir {path}");
    let rd = match std::fs::read_dir(&p) {
        Ok(r) => r,
        Err(e) => {
            return ToolResult::err(label, format!("No se pudo listar '{path}': {e}{aviso}"))
        }
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
    ToolResult { label, body: format!("{body}{aviso}"), ok: true }
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
    fn un_skill_que_no_esta_dice_cuales_si() {
        // «No existe» a secas deja al modelo probando nombres, y cada intento
        // cuesta un turno de red. Con la lista delante acierta al siguiente.
        let ks = vec![crate::skills::parse("dns", "---\ndescription: d\n---\ncuerpo", true)];
        let r = run_with_skills("skill", "loquesea", &ks).unwrap();
        assert!(!r.ok);
        assert!(r.body.contains("dns"), "{}", r.body);
    }

    #[test]
    fn un_skill_llega_con_sus_instrucciones_y_un_recordatorio() {
        // El recordatorio no es adorno: un procedimiento escrito hace meses
        // puede no encajar con la máquina de hoy, y lo que hay que hacer
        // entonces es mandar lo que se ve, no lo que dice el papel.
        let ks =
            vec![crate::skills::parse("dns", "---\ndescription: d\n---\nMira resolv.conf", true)];
        let r = run_with_skills("skill", "dns", &ks).unwrap();
        assert!(r.ok);
        assert!(r.body.contains("Mira resolv.conf"));
        assert!(r.body.contains("manda lo que ves"), "{}", r.body);
    }

    #[test]
    fn sin_skills_instalados_se_dice_eso_y_no_otra_cosa() {
        let r = run_with_skills("skill", "dns", &[]).unwrap();
        assert!(r.body.contains("No hay ningún skill instalado"), "{}", r.body);
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
    fn readlines_esta_despachada_y_no_solo_escrita() {
        // Estuvo escrita, probada y anunciada en el prompt sin esta línea de
        // `run`. Y era el peor sitio donde faltar: el aviso de recorte de
        // `readfile` le dice al modelo que siga con `readlines`, lo intentaba, y
        // `run` contestaba «no la tengo». Los tests de abajo pasaban porque
        // llaman a la función privada directamente, sin pasar por el despacho.
        let p = tmp("lucy_tool_despacho.txt");
        std::fs::write(&p, "una\ndos\ntres\n").unwrap();
        let r = run("readlines", &format!("{}|2|1", p.to_string_lossy()));
        let _ = std::fs::remove_file(&p);
        let r = r.expect("readlines no llega al despacho");
        assert!(r.body.starts_with("dos"), "{}", r.body);
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
    fn un_tramo_dice_donde_esta_y_como_seguir() {
        // Sin eso el modelo no sabe si llegó al final: o para antes de tiempo o
        // pide un tramo que no existe, y las dos cuestan un turno.
        let p = tmp("lucy_tool_tramos.txt");
        let contenido: String = (1..=50).map(|i| format!("linea {i}\n")).collect();
        std::fs::write(&p, &contenido).unwrap();
        let ruta = p.to_string_lossy().to_string();

        let r = readlines(&format!("{ruta}|10|5"));
        assert!(r.ok, "{}", r.body);
        assert!(r.body.contains("linea 10"), "no empieza donde se pidió: {}", r.body);
        assert!(r.body.contains("linea 14"));
        assert!(!r.body.contains("linea 15"), "se pasó de las que se pidieron");
        assert!(r.body.contains("de 50"), "no dice cuántas hay: {}", r.body);
        assert!(r.body.contains("|15|"), "no dice cómo seguir: {}", r.body);

        // Y el final se dice que es el final, en vez de invitar a otra vuelta.
        let f = readlines(&format!("{ruta}|46|20"));
        assert!(f.body.contains("final del fichero"), "{}", f.body);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn las_lineas_se_cuentan_desde_uno() {
        // Como cualquier editor y como cualquier mensaje de error. Contar desde
        // cero obligaría a restar uno cada vez que alguien dice «la línea 342», y
        // esa resta se olvida.
        let p = tmp("lucy_tool_base1.txt");
        std::fs::write(&p, "primera\nsegunda\ntercera\n").unwrap();
        let r = readlines(&format!("{}|1|1", p.to_string_lossy()));
        assert!(r.body.starts_with("primera"), "{}", r.body);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn pedir_mas_alla_del_final_se_explica() {
        let p = tmp("lucy_tool_corto.txt");
        std::fs::write(&p, "una\ndos\n").unwrap();
        let r = readlines(&format!("{}|99|10", p.to_string_lossy()));
        assert!(!r.ok);
        assert!(r.body.contains("2 líneas"), "{}", r.body);
        let _ = std::fs::remove_file(&p);
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
    fn un_texto_que_aparece_dos_veces_no_se_edita_a_ciegas() {
        // EL FALLO QUE HUNDE A TODAS LAS HERRAMIENTAS DE EDICIÓN. Sustituir la
        // primera coincidencia es lo cómodo, y produce el bug que nadie
        // encuentra: se cambia una línea parecida en otro sitio del fichero, y
        // el diff que el operador aprobó no es el que se aplicó.
        let p = tmp("lucy_tool_ambiguo.txt");
        std::fs::write(&p, "valor = 1\notra cosa\nvalor = 1\n").unwrap();
        let a = prepare_edit(&format!("{}|||valor = 1|||valor = 2", p.display()));
        let _ = std::fs::remove_file(&p);
        assert!(!a.blocked.is_empty(), "editó a ciegas");
        assert!(a.blocked.contains("2 veces"), "{}", a.blocked);
        assert!(a.blocked.contains("contexto"), "no dice cómo arreglarlo: {}", a.blocked);
    }

    #[test]
    fn un_texto_que_no_esta_lo_dice_en_vez_de_no_hacer_nada() {
        let p = tmp("lucy_tool_ausente.txt");
        std::fs::write(&p, "una cosa\n").unwrap();
        let a = prepare_edit(&format!("{}|||lo que sea|||otra", p.display()));
        let _ = std::fs::remove_file(&p);
        assert!(a.blocked.contains("no aparece"), "{}", a.blocked);
        assert!(a.blocked.contains("readfile"), "no le dice cómo recuperarse");
    }

    #[test]
    fn preparar_no_toca_el_disco() {
        // Es la garantía entera del carril de artefactos: lo que se ve es una
        // propuesta, y hasta que alguien pulsa no ha pasado nada.
        let p = tmp("lucy_tool_intacto.txt");
        std::fs::write(&p, "original").unwrap();
        let a = prepare_write(&format!("{}|||machacado", p.display()));
        let en_disco = std::fs::read_to_string(&p).unwrap();
        assert_eq!(en_disco, "original", "preparar escribió");
        assert!(!a.applied);
        assert_eq!(a.after, "machacado");
        assert_eq!(a.before, "original");
        // Y aplicar sí escribe.
        apply(&a).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "machacado");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn un_fichero_que_cambio_por_debajo_no_se_machaca() {
        // Entre que Lucy propone y el operador aprueba pasan minutos, y en ese
        // hueco cabe otro editor guardando. Aplicar el `after` tal cual borraría
        // ese trabajo sin que nadie se enterara.
        let p = tmp("lucy_tool_carrera.txt");
        std::fs::write(&p, "v1").unwrap();
        let a = prepare_write(&format!("{}|||v2 de Lucy", p.display()));
        std::fs::write(&p, "v1 con mis cambios a mano").unwrap();
        let e = apply(&a).unwrap_err();
        assert!(e.contains("ha cambiado"), "{e}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "v1 con mis cambios a mano");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn crear_un_fichero_nuevo_es_escribir_y_no_un_error() {
        let p = tmp("lucy_tool_nuevo_de_verdad.txt");
        let _ = std::fs::remove_file(&p);
        let a = prepare_write(&format!("{}|||contenido", p.display()));
        assert!(a.blocked.is_empty(), "{}", a.blocked);
        assert!(a.summary.contains("crear"), "{}", a.summary);
        assert_eq!(a.before, "");
        apply(&a).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "contenido");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn un_formato_mal_puesto_se_explica_en_vez_de_perderse() {
        // Sin separador no hay contenido. Descartarlo en silencio dejaría a
        // Lucy esperando un artefacto que nunca aparece.
        let a = prepare_write("C:\\solo\\una\\ruta.txt");
        assert!(a.blocked.contains("|||"), "{}", a.blocked);
        let b = prepare_edit("C:\\ruta|||solo el viejo");
        assert!(b.blocked.contains("Faltan partes"), "{}", b.blocked);
    }

    #[test]
    fn un_artefacto_bloqueado_no_se_puede_aplicar() {
        let a = prepare_write("sin separador");
        assert!(apply(&a).is_err());
    }

    #[test]
    fn los_tamanos_se_leen_como_los_lee_una_persona() {
        assert_eq!(tamano(512), "512 B");
        assert_eq!(tamano(2048), "2.0 KB");
        assert_eq!(tamano(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(tamano(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}
