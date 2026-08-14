//! El prompt de sistema, por secciones.
//!
//! Port de la ARQUITECTURA de `commands/prompt_sections.rs`: cada sección decide
//! si viene al caso, se ordena por prioridad y se puede apagar en caliente. Lo
//! que NO se porta es su texto entero, y la razón es la misma que da el módulo
//! que sustituye: allí hay veinticuatro secciones que describen herramientas de
//! la app Tauri —servidores MCP, grafo de conocimiento, sub-agentes, ejecución
//! remota— y este shell no tiene ninguna de ellas todavía. Copiarlas sería
//! decirle al modelo que puede hacer cosas que no puede, que es exactamente el
//! fallo del que venimos: Lucy prometiendo un paso de aprobación que nadie había
//! construido.
//!
//! LA REGLA DE ESTE FICHERO: una sección solo entra cuando lo que dice es cierto
//! en el shell que la manda. Cuando se migre la ejecución remota, su sección se
//! enciende; hasta entonces no está.
//!
//! POR QUÉ POR SECCIONES Y NO UN `format!` GRANDE. Un prompt monolítico se manda
//! entero siempre: el bloque de hosts remotos viaja aunque no haya ninguno, y el
//! de memoria aunque no se haya recordado nada. Cada bloque inútil enseña al
//! modelo a saltarse ese encabezado, así que el día que sí trae algo tampoco lo
//! mira. Y cuesta tokens en cada turno de cada conversación.

use std::fmt::Write;
use std::sync::{Mutex, OnceLock};

/// Marca entre la mitad ESTABLE del prompt y la que cambia en cada turno.
///
/// Anthropic cobra la escritura de caché más cara que una lectura normal, así
/// que solo compensa cachear lo que se repite idéntico turno tras turno: quién
/// es Lucy y sus reglas, sí; el porcentaje de CPU de hace tres segundos, no.
/// Misma cadena que en la app —un comentario HTML— para que si algún día una la
/// manda y la otra la lee, se entiendan.
pub const CACHE_BOUNDARY: &str = "<!-- LUCY_CACHE_BOUNDARY -->";

/// Todo lo que una sección necesita para decidir si viene al caso.
///
/// Referencias, sin reservar memoria: se construye en cada turno y llega a
/// veinte secciones que en su mayoría lo van a ignorar.
pub struct Ctx<'a> {
    /// Cómo se llama el operador. Vacío = no se ha configurado todavía.
    pub user_name: &'a str,
    /// Lo que se sabe de él y conviene tener presente. Vacío = nada.
    pub user_profile: &'a str,
    /// Directorio de trabajo, para resolver rutas relativas.
    pub working_dir: &'a str,
    /// El equipo, medido hace segundos.
    pub machine: Option<&'a crate::system::SysSnapshot>,
    /// Servicios automáticos que están caídos.
    pub services: &'a [crate::system::DownService],
    /// Últimas líneas del log de la aplicación.
    pub log: &'a [String],
    /// Equipos remotos configurados, ya formateados. Vacío = no hay ninguno.
    pub hosts: &'a str,
    /// El catálogo de skills, ya formateado. Vacío = no hay ninguno.
    ///
    /// Solo el NOMBRE y la DESCRIPCIÓN de cada uno. El cuerpo entero de cinco
    /// skills serían miles de tokens en cada turno de cada conversación, casi
    /// siempre para no usar ninguno; se carga el que pida, cuando lo pida.
    pub skills: &'a str,
    /// El skill FIJADO, ya formateado. Vacío = ninguno.
    ///
    /// Va con prioridad alta y ESTABLE: es un modo que el operador puso a
    /// propósito y que enmarca todo lo que venga, así que se lee antes que el
    /// estado de la máquina y se cobra como caché.
    pub preset: &'a str,
    /// Memorias recordadas para esta orden, ya formateadas.
    pub memories: &'a str,
    /// Modelo flojo siguiendo instrucciones: se le manda lo justo.
    pub weak_model: bool,
    /// Si este shell puede ejecutar lo que proponga. Cambia el contrato entero:
    /// con `false`, un comando es una propuesta que el operador aprueba.
    pub can_execute: bool,
    /// Si la cadena avanza sola. Solo significa algo junto a `can_execute`.
    ///
    /// Decírselo NO es cortesía. Con el automático encendido y sin esta línea,
    /// Lucy sigue escribiendo «¿quieres que lo ejecute?» y esperando una
    /// respuesta que ya no hace falta: se gasta una vuelta del presupuesto en
    /// pedir un permiso que el operador dio al encender el modo.
    pub auto: bool,
}

impl Default for Ctx<'_> {
    fn default() -> Self {
        Self {
            user_name: "",
            user_profile: "",
            working_dir: "",
            machine: None,
            services: &[],
            log: &[],
            hosts: "",
            skills: "",
            preset: "",
            memories: "",
            weak_model: false,
            can_execute: false,
            auto: false,
        }
    }
}

/// Un modelo flojo siguiendo instrucciones.
///
/// Port de `model_is_weak`. Los niveles baratos y rápidos se ahogan con seis mil
/// tokens de reglas y contestan en prosa en vez de emitir etiquetas: no es que
/// desobedezcan, es que la instrucción se pierde en el ruido. Se les manda un
/// prompt corto.
///
/// `-mini` y `-lite` con guion A PROPÓSITO: `mini` suelto haría que "geMINI"
/// entrara aquí, y Gemini Pro no es un modelo flojo.
pub fn model_is_weak(model: &str) -> bool {
    // PRIMERO SE LE PREGUNTA AL CATÁLOGO, que ya sabe la respuesta.
    //
    // El glifo de cada modelo codifica su nivel —◆ insignia, ◐ equilibrado,
    // ◯ económico— y este clasificador lo ignoraba para adivinarlo del nombre.
    // Se equivocaba en las dos direcciones: degradaba a `gemini-3.5-flash`, que
    // el propio catálogo describe como «rendimiento de frontera sostenido», por
    // llevar «flash» en el id; y ascendía a `gpt-5.6-luna`, que es ◯ económico,
    // porque su nombre no casaba con ninguna de las cadenas. La heurística se
    // escribió cuando «Flash» quería decir pequeño y barato, y dejó de ser
    // verdad sin que nadie tocara esta función.
    let base = model.split_once("::").map_or(model, |(b, _)| b);
    for g in crate::models::GROUPS {
        for o in g.options {
            let oid = o.id.split_once("::").map_or(o.id, |(b, _)| b);
            if oid == base {
                return o.icon == "◯";
            }
        }
    }
    // Y si no está en el catálogo es de Ollama, que se descubre en la máquina.
    // Ahí sí hay que adivinar, y el tamaño en el nombre es lo único que hay.
    let m = model.to_ascii_lowercase();
    m.contains("-mini")
        || m.contains("-lite")
        || m.contains(":8b")
        || m.contains(":7b")
        || m.contains(":3b")
        || m.contains(":1")
        || m.starts_with("local-")
}

/// Una pieza del prompt.
pub trait Section: Send + Sync {
    /// Nombre estable. Es por lo que se apaga en caliente, así que no es texto
    /// de interfaz y no se traduce.
    fn name(&self) -> &'static str;
    /// Si viene al caso en este turno. Se pregunta UNA vez por construcción.
    fn relevant(&self, ctx: &Ctx) -> bool;
    /// Dónde va. Menor = antes.
    fn priority(&self) -> u32;
    /// Si su contenido es el mismo turno tras turno. Las estables van antes de
    /// la marca de caché; las demás, después.
    fn stable(&self) -> bool {
        true
    }
    fn render(&self, ctx: &Ctx) -> String;
}

// ── Interruptores en caliente ────────────────────────────────────────────────

fn apagadas() -> &'static Mutex<Vec<String>> {
    static D: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(Vec::new()))
}

/// Apaga una sección por su nombre.
///
/// Sirve para lo que sirve un interruptor: aislar. Cuando Lucy empieza a
/// contestar raro, apagar una sección y repetir la orden dice en un turno si era
/// esa, sin recompilar.
pub fn disable(name: &str) {
    if let Ok(mut d) = apagadas().lock() {
        if !d.iter().any(|x| x == name) {
            d.push(name.to_string());
        }
    }
}

/// Vuelve a encender una sección.
pub fn enable(name: &str) {
    if let Ok(mut d) = apagadas().lock() {
        d.retain(|x| x != name);
    }
}

/// Las que están apagadas ahora mismo.
pub fn disabled() -> Vec<String> {
    apagadas().lock().map(|d| d.clone()).unwrap_or_default()
}

fn is_disabled(name: &str) -> bool {
    apagadas().lock().map(|d| d.iter().any(|x| x == name)).unwrap_or(false)
}

// ── Las secciones ────────────────────────────────────────────────────────────

struct Identity;
impl Section for Identity {
    fn name(&self) -> &'static str {
        "Identity"
    }
    fn relevant(&self, _c: &Ctx) -> bool {
        true
    }
    fn priority(&self) -> u32 {
        0
    }
    fn render(&self, c: &Ctx) -> String {
        let mut s = String::from(
            "Eres Lucy, una asistente de administración de sistemas Windows. Hablas en \
             español, vas al grano y no adornas.\n\n\
             NO eres un modelo genérico sin acceso: estás integrada en una aplicación de \
             escritorio que corre EN el equipo del operador y que te entrega sus lecturas \
             reales. Los datos que vienen abajo están medidos en esta máquina hace \
             segundos. Úsalos. No pidas al operador que ejecute comandos para averiguar \
             algo que ya tienes delante, y no digas que no tienes acceso a su equipo.",
        );
        // El nombre y el perfil solo si los hay. "El operador se llama (vacío)"
        // es peor que no decir nada: el modelo lo trata como un dato.
        if !c.user_name.is_empty() {
            let _ = write!(s, "\n\nEl operador se llama {}.", c.user_name);
        }
        // El perfil va en su PROPIO párrafo, con encabezado. Antes se pegaba
        // detrás del nombre con un espacio, y el bloque es multilínea y viene
        // agrupado por categorías: el resultado era una frase que empezaba
        // "El operador se llama Iván. [IDENTIDAD] - dominio ad: corp.local".
        if !c.user_profile.is_empty() {
            let _ = write!(
                s,
                "\n\nLO QUE YA SABES DE ÉL (de conversaciones anteriores):\n{}",
                c.user_profile
            );
        }
        if !c.working_dir.is_empty() {
            let _ = write!(
                s,
                "\nDirectorio de trabajo: {}. Cuando el operador nombre un fichero sin \
                 ruta completa, resuélvelo respecto a este directorio.",
                c.working_dir
            );
        }
        s
    }
}

struct Style;
impl Section for Style {
    fn name(&self) -> &'static str {
        "Style"
    }
    fn relevant(&self, _c: &Ctx) -> bool {
        true
    }
    fn priority(&self) -> u32 {
        10
    }
    fn render(&self, _c: &Ctx) -> String {
        // Port de las reglas 29 y 39 de la V2, que son las que sobreviven al
        // cambio de shell: son sobre CÓMO se contesta, no sobre qué herramientas
        // hay.
        String::from(
            "LARGO DE LA RESPUESTA: proporcional a la pregunta. Una pregunta directa \
             —qué hace X, cuál es el puerto de Y— se contesta en tres párrafos como mucho, \
             o en un bloque de código. Un diagnóstico ocupa lo que haga falta, con \
             encabezados. Confirmar algo que ya se hizo son dos líneas y la salida. Nunca \
             rellenes con «espero que esto ayude» ni «cualquier duda avísame»: cuando la \
             respuesta está completa, se acaba.\n\
             CÓMO TRABAJAS: en una tarea de varios pasos, di en una línea qué vas a hacer \
             antes de cada paso, para que el operador siga tu razonamiento en vivo en vez \
             de verte callada y recibir un volcado al final. Tono directo y competente, \
             sin disculpas de más ni relleno corporativo.",
        )
    }
}

struct Safety;
impl Section for Safety {
    fn name(&self) -> &'static str {
        "Safety"
    }
    fn relevant(&self, _c: &Ctx) -> bool {
        true
    }
    fn priority(&self) -> u32 {
        20
    }
    fn render(&self, _c: &Ctx) -> String {
        // Reglas 30, 31, 33 y 35 de la V2. Las que se quedan fuera describen
        // herramientas que este shell no tiene; éstas cuatro son sobre no
        // inventarse cosas y no ser peligrosa, que aplica en cualquier shell.
        String::from(
            "CREDENCIALES: si el mensaje del operador contiene algo que parezca un secreto \
             (password=, api_key=, token=, Bearer, cadenas tipo «eyJ», una cadena de \
             conexión con usuario y contraseña), NO lo repitas en tu respuesta —usa \
             [REDACTADO]— y avisa una vez de que no conviene pegar secretos en el chat. \
             Luego sigue con la tarea.\n\
             AMBIGÜEDAD: cuando una orden se pueda entender de dos formas con consecuencias \
             distintas —«borra el log» puede ser vaciarlo o borrar el fichero; «reinicia» \
             puede ser el servicio o la máquina—, pregunta UNA cosa con dos o tres opciones \
             concretas ANTES de generar ningún comando. En lo irreversible no se adivina.\n\
             EVIDENCIA: no cites nunca un dato específico —un ID de evento, un PID, un \
             tamaño exacto, un código de salida, un número de puerto, una versión— si ese \
             valor exacto no salió en la salida de un comando que se ejecutó en ESTA \
             conversación. Si lo sospechas por experiencia y no lo has medido, dilo como \
             hipótesis a comprobar y propón el comando que la confirmaría. Afirmar un \
             número que no mediste es el fallo más dañino que puedes cometer: parece \
             análisis y es invención.\n\
             SINTAXIS DE TERCEROS: lo mismo con los nombres exactos de campos, claves de \
             configuración, parámetros de API o rutas de registro que no hayas leído en \
             esta conversación. O lo citas de algo que leíste, o lo marcas como sin \
             verificar. Inventarse el nombre de un atributo y darlo por bueno hace perder \
             el tiempo a base de prueba y error.",
        )
    }
}

struct Actions;
impl Section for Actions {
    fn name(&self) -> &'static str {
        "Actions"
    }
    fn relevant(&self, _c: &Ctx) -> bool {
        true
    }
    fn priority(&self) -> u32 {
        30
    }
    fn render(&self, c: &Ctx) -> String {
        let mut s = String::from(
            "CÓMO PEDIR ACCIONES\n\
             Para proponer un comando de PowerShell, enciérralo en <EXECUTE>…</EXECUTE>.\n\
             Para otros motores, cada uno tiene su etiqueta y su contenido va SIN el nombre \
             del programa delante, que ya lo pone el shell:\n\
             · <EXECUTE_CMD>ipconfig /all</EXECUTE_CMD> — cmd.exe\n\
             · <EXECUTE_WMIC>cpu get name</EXECUTE_WMIC> — wmic.exe, solo clases Win32_*\n\
             · <EXECUTE_NETSH>interface ip show config</EXECUTE_NETSH> — netsh.exe\n\
             · <EXECUTE_REG>query HKLM\\SOFTWARE\\… /v Nombre</EXECUTE_REG> — reg.exe\n\
             · <EXECUTE_CSCRIPT>script.vbs</EXECUTE_CSCRIPT> — cscript.exe\n\
             Una consulta al registro va SIEMPRE en <EXECUTE_REG> y NUNCA en <EXECUTE_WMIC>: \
             WMI no es el registro.\n\
             Si necesitas razonar antes de responder, hazlo dentro de <THOUGHT>…</THOUGHT>: \
             se guarda aparte y no ensucia la respuesta.\n\
             Herramientas de lectura, cuyo resultado te vuelve en el turno siguiente:\n\
             · <TOOL>readfile:C:\\ruta\\fichero.log</TOOL> — el texto del fichero\n\
             · <TOOL>listdir:C:\\ruta</TOOL> — qué hay en esa carpeta\n\
             · <TOOL>readlines:C:\\ruta|desde|cuántas</TOOL> — un tramo concreto. Es lo que \
             usas cuando `readfile` te avise de que recortó: lo que buscas en un log casi \
             siempre está al final, no en los primeros miles de caracteres.\n\
             · <TOOL>pdf_search:qué buscar</TOOL> — los manuales y guías que el operador ha \
             ingerido. ÚSALA sin que te lo pidan cuando la pregunta sea sobre un producto, \
             un procedimiento o una configuración concreta: si hay un manual de eso, dice \
             más y mejor que lo que tú recuerdes, y citarlo es la diferencia entre una \
             respuesta y una respuesta comprobable.\n\
             Y dos para cambiar ficheros, que NO escriben solas: preparan el cambio en \
             el panel de Artefactos con su diff, y escribe el operador al aprobarlo.\n\
             · <TOOL>writefile:C:\\ruta\\fichero.txt|||CONTENIDO</TOOL>\n\
             · <TOOL>editfile:C:\\ruta|||TEXTO_VIEJO|||TEXTO_NUEVO</TOOL>\n\
             En `editfile`, el TEXTO_VIEJO tiene que aparecer UNA sola vez en el \
             fichero: si aparece varias, el cambio se rechaza y hay que darle más \
             contexto —la línea de antes y la de después— hasta que sea único.\n\
             Y dos para repartirte el trabajo cuando tengas VARIAS averiguaciones que \
             no dependen unas de otras —cuatro logs, tres carpetas, dos equipos—:\n\
             · <TOOL>fork_task:nombre-corto|qué tiene que averiguar</TOOL>\n\
             · <TOOL>wait_task:nombre-corto</TOOL> — o <TOOL>wait_task:*</TOOL> para todas\n\
             Lanza las que necesites en un mismo turno y recógelas después; el nombre lo \
             pones tú y es por el que las pides. Una tarea auxiliar SOLO LEE ficheros y \
             carpetas: no ejecuta comandos ni toca equipos remotos, así que lo que haya \
             que ejecutar lo propones TÚ con lo que ella te cuente. Y no la uses para lo \
             que puedes hacer de un tirón: una sola lectura sale más barata leyéndola.\n\
             Esas y ninguna más: lo que pidas con otro nombre no lo va a cumplir \
             nadie, y te quedarás esperando un resultado que no llega.\n\
             Cuando el operador te enseñe un PROCEDIMIENTO —cómo se hace algo aquí, en \
             esta casa— y te diga que lo apuntes, guárdalo con \
             <LEARN>cuándo aplica|comando|qué responder</LEARN>. Queda propuesto en \
             Artefactos como un skill, y el operador lo aprueba. Solo si te lo pide o si \
             es evidente que quiere que se quede: apuntar cada cosa que se dice llena el \
             catálogo de procedimientos que nadie escribió a propósito.\n\
             Cuando el operador te diga un dato SUYO que vayas a necesitar otro día \
             —su dominio, sus servidores, cómo prefiere que hagas las cosas— guárdalo \
             con <REMEMBER category=\"identidad|preferencia|contexto|host\">clave: \
             valor</REMEMBER>. Una etiqueta por dato, la clave corta. Lo guardado te \
             llegará en el prompt de todas las conversaciones siguientes, así que \
             guarda hechos estables y NO lo de este turno: ni cifras que cambian, ni \
             salidas de comandos, ni nada que puedas volver a medir.",
        );
        // El automático se mira ANTES que `can_execute` y no en conjunción con
        // él. Los dos describen quién aprieta el gatillo, y el shell nativo pone
        // `can_execute: false` para decir «yo propongo, ejecuta el operador» —
        // que es cierto hasta el momento en que el operador delega justamente
        // eso. Exigir las dos banderas dejaba a Lucy leyendo «los comandos NO se
        // ejecutan solos» mientras se ejecutaban solos.
        if c.auto {
            s.push_str(
                "\nEl operador ha encendido el modo AUTOMÁTICO: tus comandos se ejecutan \
                 sin que nadie los apruebe, y su salida literal te vuelve en el turno \
                 siguiente. Encadena los pasos que hagan falta hasta poder responder, uno \
                 por turno, y para en cuanto tengas la respuesta. Hay un tope de pasos, \
                 así que no explores de más. Algunos comandos —elevación, formas raras— \
                 seguirán parándose para que los apruebe una persona: si eso pasa, dilo y \
                 espera, no busques otra forma de darlos.",
            );
        } else if c.can_execute {
            s.push_str(
                "\nLos comandos se ejecutan cuando el operador los aprueba en el panel de \
                 Plan, y su salida literal te vuelve en el turno siguiente.",
            );
        } else {
            s.push_str(
                "\nIMPORTANTE: los comandos NO se ejecutan solos. Aparecen en el panel de \
                 Plan como pasos PENDIENTES con un botón de Ejecutar, y solo corren cuando \
                 el operador lo pulsa. Cuando lo haga, la salida literal te volverá en el \
                 turno siguiente para que la resumas. Así que propón el comando y explica \
                 qué hace, pero NUNCA digas que ya lo ejecutaste ni des por hecho su \
                 salida: todavía no ha corrido.",
            );
        }
        s
    }
}

struct Elevation;
impl Section for Elevation {
    fn name(&self) -> &'static str {
        "Elevation"
    }
    fn relevant(&self, _c: &Ctx) -> bool {
        true
    }
    fn priority(&self) -> u32 {
        35
    }
    fn render(&self, _c: &Ctx) -> String {
        // La V2 decía lo contrario —"nunca emitas -Verb RunAs, Lucy no puede
        // lanzar un UAC"— porque entonces era verdad. Ya no: `elevate.rs` lo
        // hace. Dejar el texto viejo haría que Lucy se negara a proponer lo que
        // ahora sabe hacer.
        String::from(
            "PRIVILEGIOS: el shell sabe elevar. Si un comando necesita administrador, \
             proponlo NORMAL —sin `-Verb RunAs` y sin `Start-Process`— y di en una línea \
             que hace falta elevación: el panel enseña un botón aparte que lanza el UAC y \
             corre ese mismo comando elevado.\n\
             Qué necesita administrador: leer el canal «Security» del registro de eventos, \
             borrar registros de eventos, instalar o modificar servicios, escribir en \
             HKLM, escribir en %ProgramFiles% o %SystemRoot%, y tocar reglas de \
             cortafuegos. Qué NO: Get-Service, Get-Hotfix, Get-NetTCPConnection, y todo \
             HKCU.",
        )
    }
}

struct Preset;
impl Section for Preset {
    fn name(&self) -> &'static str {
        "Preset"
    }
    fn relevant(&self, c: &Ctx) -> bool {
        !c.preset.is_empty()
    }
    fn priority(&self) -> u32 {
        12
    }
    fn render(&self, c: &Ctx) -> String {
        c.preset.trim_end().to_string()
    }
}

struct Skills;
impl Section for Skills {
    fn name(&self) -> &'static str {
        "Skills"
    }
    fn relevant(&self, c: &Ctx) -> bool {
        !c.skills.is_empty()
    }
    fn priority(&self) -> u32 {
        35
    }
    /// ESTABLE: el catálogo solo cambia cuando alguien añade un fichero, así que
    /// va antes de la marca de caché y se cobra como lectura en vez de como
    /// tokens nuevos en cada turno de cada conversación.
    fn stable(&self) -> bool {
        true
    }
    fn render(&self, c: &Ctx) -> String {
        c.skills.trim_end().to_string()
    }
}

struct HostRouting;
impl Section for HostRouting {
    fn name(&self) -> &'static str {
        "HostRouting"
    }
    fn relevant(&self, c: &Ctx) -> bool {
        !c.hosts.is_empty()
    }
    fn priority(&self) -> u32 {
        40
    }
    fn stable(&self) -> bool {
        false
    }
    fn render(&self, c: &Ctx) -> String {
        format!(
            "EQUIPOS REMOTOS CONFIGURADOS\n{}\n\
             Para correr algo en uno de ellos usa \
             <EXECUTE_REMOTE target=\"id\">comando</EXECUTE_REMOTE> con el id que aparece \
             arriba. El comando se ejecuta ALLÍ, así que escríbelo en el shell de ESE \
             equipo: PowerShell en los Windows, bash en los Linux.\n\
             Y no uses <EXECUTE> para algo que era para otra máquina: ése corre aquí, y \
             mediría el equipo equivocado diciendo que midió el bueno.",
            c.hosts.trim_end()
        )
    }
}

struct Memories;
impl Section for Memories {
    fn name(&self) -> &'static str {
        "Memories"
    }
    fn relevant(&self, c: &Ctx) -> bool {
        !c.memories.is_empty()
    }
    fn priority(&self) -> u32 {
        50
    }
    fn stable(&self) -> bool {
        false
    }
    fn render(&self, c: &Ctx) -> String {
        format!(
            "MEMORIAS RECORDADAS (parecidas a esta orden)\n{}\n\
             Úsalas como base cuando vengan al caso. NO son parte de la orden actual: si no \
             encajan, ignóralas sin mencionarlas.",
            c.memories.trim_end()
        )
    }
}

struct Machine;
impl Section for Machine {
    fn name(&self) -> &'static str {
        "Machine"
    }
    fn relevant(&self, c: &Ctx) -> bool {
        c.machine.is_some()
    }
    fn priority(&self) -> u32 {
        60
    }
    fn stable(&self) -> bool {
        false
    }
    fn render(&self, c: &Ctx) -> String {
        let Some(s) = c.machine else { return String::new() };
        let mut p = String::from("EQUIPO\n");
        let _ = writeln!(p, "Nombre: {}", s.host);
        let _ = writeln!(p, "Sistema: {} · kernel {}", s.os, s.kernel);
        let _ = writeln!(p, "CPU: {} · {} núcleos · {:.0}% de uso", s.cpu_brand, s.cores, s.cpu_pct);
        let _ = writeln!(
            p,
            "RAM: {:.1} GB usados de {:.1} GB",
            s.mem_used as f64 / 1e9,
            s.mem_total as f64 / 1e9
        );
        for d in &s.disks {
            let pct = if d.total > 0 {
                d.total.saturating_sub(d.avail) as f64 / d.total as f64 * 100.0
            } else {
                0.0
            };
            let _ = writeln!(
                p,
                "Disco {}: {:.0}% ocupado, {:.1} GB libres de {:.1} GB",
                d.mount,
                pct,
                d.avail as f64 / 1e9,
                d.total as f64 / 1e9
            );
        }
        let _ = write!(p, "Uptime: {} segundos", s.uptime_secs);
        p
    }
}

struct Services;
impl Section for Services {
    fn name(&self) -> &'static str {
        "Services"
    }
    fn relevant(&self, c: &Ctx) -> bool {
        !c.services.is_empty()
    }
    fn priority(&self) -> u32 {
        65
    }
    fn stable(&self) -> bool {
        false
    }
    fn render(&self, c: &Ctx) -> String {
        let mut p = String::from("SERVICIOS AUTOMÁTICOS DETENIDOS\n");
        for sv in c.services {
            let _ = writeln!(
                p,
                "{} — {}",
                sv.name,
                if sv.crashed() {
                    format!("FALLÓ (código {})", sv.exit_code)
                } else {
                    "detenido limpiamente".to_string()
                }
            );
        }
        p.truncate(p.trim_end().len());
        p
    }
}

/// Cuántas líneas de log se le pasan al modelo.
///
/// Veinte cabe de sobra en cualquier ventana de contexto y sigue siendo
/// suficiente para ver un patrón. Pasarle el fichero entero gasta tokens en
/// líneas idénticas repetidas mil veces.
pub const LOG_LINES: usize = 20;

struct Log;
impl Section for Log {
    fn name(&self) -> &'static str {
        "Log"
    }
    fn relevant(&self, c: &Ctx) -> bool {
        !c.log.is_empty()
    }
    fn priority(&self) -> u32 {
        70
    }
    fn stable(&self) -> bool {
        false
    }
    fn render(&self, c: &Ctx) -> String {
        let mut p = String::from("ÚLTIMAS LÍNEAS DEL LOG DE LUCY (lucy_app.log)\n");
        // Las ÚLTIMAS, porque un incidente se lee por el final; y en su orden
        // original, porque un log del revés se interpreta del revés.
        for l in c.log.iter().rev().take(LOG_LINES).rev() {
            let _ = writeln!(p, "{l}");
        }
        // Que el modelo sepa QUÉ log es. Sin esta línea confunde el log de la
        // aplicación con el registro de eventos de Windows y responde sobre el
        // que no es.
        p.push_str("(Es el log de la propia aplicación Lucy, NO el registro de eventos de Windows.)");
        p
    }
}

/// Todas las secciones, en el orden en que se declaran (que no es el orden en
/// que salen: eso lo decide `priority`).
fn secciones() -> Vec<Box<dyn Section>> {
    vec![
        Box::new(Identity),
        Box::new(Style),
        Box::new(Safety),
        Box::new(Actions),
        Box::new(Elevation),
        Box::new(Preset),
        Box::new(Skills),
        Box::new(HostRouting),
        Box::new(Memories),
        Box::new(Machine),
        Box::new(Services),
        Box::new(Log),
    ]
}

/// Los nombres de todas las secciones y su prioridad, para poder listarlas sin
/// construir un prompt.
pub fn section_names() -> Vec<(&'static str, u32)> {
    let mut v: Vec<(&'static str, u32)> =
        secciones().iter().map(|s| (s.name(), s.priority())).collect();
    v.sort_by_key(|(_, p)| *p);
    v
}

/// Lo mínimo para un modelo que se ahoga con lo demás.
///
/// No es el prompt normal recortado: es otro. Un modelo flojo al que se le dan
/// cuatro reglas las cumple; al que se le dan cuarenta contesta en prosa y no
/// emite ninguna etiqueta. La identidad y el estado del equipo sí van —sin ellos
/// vuelve a decir que no tiene acceso—, las reglas de estilo y de evidencia no.
fn prompt_weak(c: &Ctx) -> String {
    let mut p = Identity.render(c);
    p.push_str("\n\n");
    p.push_str(
        "Para proponer un comando de PowerShell, enciérralo en <EXECUTE>…</EXECUTE>. \
         No se ejecuta solo: el operador lo aprueba y la salida te vuelve después. \
         No digas que ya lo ejecutaste.\n\
         No te inventes números que no hayas medido.",
    );
    if let Some(m) = c.machine {
        let _ = write!(
            p,
            "\n\nEQUIPO\nNombre: {}\nSistema: {}\nCPU: {:.0}% · RAM: {:.1} de {:.1} GB",
            m.host,
            m.os,
            m.cpu_pct,
            m.mem_used as f64 / 1e9,
            m.mem_total as f64 / 1e9
        );
    }
    if !c.memories.is_empty() {
        let _ = write!(p, "\n\nRECUERDAS\n{}", c.memories.trim_end());
    }
    p
}

/// Monta el prompt de sistema.
///
/// Las secciones estables van primero y luego la marca de caché: no es estética,
/// es lo que permite que Anthropic cobre la mitad de arriba como lectura de
/// caché en vez de como tokens nuevos en cada turno de la conversación.
pub fn build(c: &Ctx) -> String {
    if c.weak_model {
        return prompt_weak(c);
    }
    let todas = secciones();
    let mut vivas: Vec<&Box<dyn Section>> = todas
        .iter()
        .filter(|s| !is_disabled(s.name()) && s.relevant(c))
        .collect();
    vivas.sort_by_key(|s| s.priority());

    let mut p = String::new();
    let mut marcada = false;
    for s in vivas {
        let txt = s.render(c);
        if txt.trim().is_empty() {
            continue;
        }
        // La marca va justo antes de la PRIMERA sección inestable. Ponerla al
        // final de las estables sería lo mismo solo si siempre hubiera alguna
        // inestable, y no siempre la hay.
        if !s.stable() && !marcada {
            p.push_str(CACHE_BOUNDARY);
            p.push('\n');
            marcada = true;
        }
        p.push_str(&txt);
        p.push_str("\n\n");
    }
    p.truncate(p.trim_end().len());
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::{DiskInfo, DownService, SysSnapshot};

    /// Los interruptores son ESTADO GLOBAL, y los tests corren en paralelo en el
    /// mismo proceso. Sin esto, el que apaga una sección se la quita a cualquier
    /// otro que esté montando un prompt en ese instante — un fallo intermitente
    /// que saldría una vez cada cincuenta ejecuciones y costaría una tarde.
    fn serie() -> std::sync::MutexGuard<'static, ()> {
        static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
        L.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn snap() -> SysSnapshot {
        SysSnapshot {
            host: "WORKSTATION-16".into(),
            os: "Windows 11 Pro".into(),
            kernel: "26200".into(),
            cpu_brand: "Intel i9".into(),
            cpu_pct: 7.0,
            per_core: vec![],
            mem_used: 10_000_000_000,
            mem_total: 31_200_000_000,
            swap_used: 0,
            swap_total: 0,
            uptime_secs: 3600,
            cores: 32,
            disks: vec![DiskInfo {
                name: "Local Disk".into(),
                mount: "C:\\".into(),
                total: 1_000_000_000_000,
                avail: 710_000_000_000,
            }],
        }
    }

    #[test]
    fn el_prompt_lleva_los_datos_reales_del_equipo() {
        let _s = serie();
        // Sin esto, el modelo contesta "no tengo acceso a tu computadora" — que
        // es exactamente lo que pasaba, y no era culpa suya.
        let m = snap();
        let p = build(&Ctx { machine: Some(&m), ..Default::default() });
        assert!(p.contains("WORKSTATION-16"));
        assert!(p.contains("Windows 11 Pro"));
        assert!(p.contains("32 núcleos"));
        assert!(p.contains("C:\\"));
        assert!(p.contains("no digas que no tienes acceso"));
    }

    #[test]
    fn avisa_de_que_los_comandos_no_se_ejecutan_solos() {
        let _s = serie();
        // Callarlo haría que el modelo escribiera "ya lo he ejecutado" sobre una
        // máquina que nadie tocó — peor que no poder ejecutar.
        let p = build(&Ctx::default());
        assert!(p.contains("NO se ejecutan"));
        assert!(p.contains("NUNCA digas que ya"));

        // Y cuando el shell SÍ ejecuta, no se le dice lo contrario.
        let p2 = build(&Ctx { can_execute: true, ..Default::default() });
        assert!(!p2.contains("NUNCA digas que ya"));
    }

    #[test]
    fn solo_se_le_prometen_las_herramientas_que_existen() {
        let _s = serie();
        // Prometer en el prompt una herramienta que nadie cumple es cómo se
        // llegó a esto: Lucy pedía `readfile`, el shell lo anotaba y no pasaba
        // nada. Las dos que están se nombran; que no están las demás se dice.
        let p = build(&Ctx::default());
        for (nombre, _) in crate::tools::AVAILABLE {
            assert!(p.contains(nombre), "no se le ofrece {nombre}");
        }
        assert!(p.contains("ninguna más"), "no se le acota el catálogo");
    }

    #[test]
    fn las_de_escribir_se_le_ofrecen_como_propuestas_y_no_como_escrituras() {
        let _s = serie();
        // La diferencia no es de matiz. Si Lucy cree que `writefile` escribe,
        // dice "ya lo he dejado arreglado" y se va; el operador se encuentra el
        // fichero intacto y un artefacto esperando en un panel que no miró.
        let p = build(&Ctx::default());
        assert!(p.contains("NO escriben solas"), "no se le dice que solo proponen");
        assert!(p.contains("Artefactos"), "no se le dice dónde acaba la propuesta");
        // Y el caso que rechaza el cambio, dicho ANTES de que lo intente: sin
        // esto lo descubre por un error y gasta una vuelta en enterarse.
        assert!(p.contains("UNA sola vez"), "no se le avisa de la coincidencia única");
    }

    #[test]
    fn se_le_dice_como_guardar_lo_que_aprende() {
        let _s = serie();
        // Sin esta instrucción la etiqueta no se emite nunca, y todo lo que hay
        // detrás —parsear, guardar, volver a inyectarlo— es maquinaria que no
        // arranca. Es el fallo del que venimos: el hueco del perfil existía en
        // el prompt y viajaba vacío porque nadie había cerrado el círculo.
        let p = build(&Ctx::default());
        assert!(p.contains("<REMEMBER"), "no se le enseña la etiqueta");
        // Y se le dice qué NO guardar. Sin ese límite, un modelo servicial
        // guarda el porcentaje de CPU de hoy y lo arrastra a cada turno futuro.
        assert!(p.contains("hechos estables"), "no se le acota qué guardar");
    }

    #[test]
    fn el_perfil_va_en_su_propio_parrafo() {
        let _s = serie();
        // Se pegaba detrás del nombre con un espacio, y el bloque es multilínea
        // con encabezados de categoría: salía "El operador se llama Iván.
        // [IDENTIDAD] - dominio ad: corp.local" como si fuera una frase.
        let p = build(&Ctx {
            user_name: "Iván",
            user_profile: "[IDENTIDAD]\n- dominio ad: corp.local",
            ..Default::default()
        });
        assert!(p.contains("se llama Iván"), "{p}");
        assert!(p.contains("LO QUE YA SABES DE ÉL"), "el perfil no tiene encabezado");
        assert!(!p.contains("Iván. [IDENTIDAD]"), "el perfil quedó pegado a la frase");
    }

    #[test]
    fn sin_perfil_no_se_anuncia_un_encabezado_vacio() {
        let _s = serie();
        // Un bloque que dice "lo que sabes de él: (nada)" enseña al modelo a
        // saltarse ese encabezado, y el día que traiga algo tampoco lo mirará.
        let p = build(&Ctx { user_name: "Iván", ..Default::default() });
        assert!(!p.contains("LO QUE YA SABES"));
    }

    #[test]
    fn el_automatico_cambia_el_contrato_aunque_can_execute_siga_en_falso() {
        let _s = serie();
        // El shell nativo pone `can_execute: false` para decir "yo propongo,
        // ejecuta el operador", y eso deja de ser cierto en cuanto el operador
        // delega. Si las dos banderas tuvieran que coincidir, Lucy leería "los
        // comandos NO se ejecutan solos" mientras se ejecutaban solos — y
        // seguiría pidiendo un permiso ya concedido, gastando una vuelta del
        // presupuesto en cada paso.
        let p = build(&Ctx { auto: true, ..Default::default() });
        assert!(p.contains("AUTOMÁTICO"), "no se le dice en qué modo está");
        assert!(!p.contains("NO se ejecutan solos"), "se le dice lo contrario de lo que pasa");
        // Y se le avisa de que algunos pasos van a pararse igual: sin esto,
        // ante una pausa del guardrail busca otra forma de dar el mismo paso.
        assert!(p.contains("apruebe una persona"), "no se le avisa de las pausas");
    }

    #[test]
    fn las_secciones_sin_datos_no_aparecen() {
        let _s = serie();
        // Una sección que dice "ninguno" en cada turno gasta tokens en decir que
        // no pasa nada, y enseña al modelo a saltarse ese bloque el día que sí
        // traiga algo.
        let p = build(&Ctx::default());
        assert!(!p.contains("SERVICIOS AUTOMÁTICOS DETENIDOS"));
        assert!(!p.contains("LOG DE LUCY"));
        assert!(!p.contains("EQUIPOS REMOTOS"));
        assert!(!p.contains("MEMORIAS RECORDADAS"));
        assert!(!p.contains("EQUIPO\n"));

        let svc = [DownService { name: "gpsvc".into(), exit_code: 1 }];
        let log = ["ERROR algo".to_string()];
        let p2 = build(&Ctx { services: &svc, log: &log, ..Default::default() });
        assert!(p2.contains("gpsvc — FALLÓ (código 1)"));
        assert!(p2.contains("ERROR algo"));
    }

    #[test]
    fn del_log_van_las_ultimas_lineas_y_en_su_orden() {
        let _s = serie();
        // Las ÚLTIMAS, porque un incidente se lee por el final; y en su orden
        // original, porque un log al revés se interpreta al revés.
        let log: Vec<String> = (0..50).map(|i| format!("linea {i}")).collect();
        let p = build(&Ctx { log: &log, ..Default::default() });
        assert!(p.contains("linea 49"));
        assert!(!p.contains("linea 29"), "solo las últimas {LOG_LINES}");
        assert!(p.find("linea 30") < p.find("linea 49"), "el log va en su orden");
    }

    #[test]
    fn la_marca_de_cache_separa_lo_fijo_de_lo_que_cambia() {
        let _s = serie();
        // Cachear la mitad de arriba solo sale a cuenta si de verdad es la misma
        // en cada turno. Si el porcentaje de CPU quedara por encima, cada turno
        // escribiría una caché nueva — que se cobra MÁS cara que no cachear.
        let m = snap();
        let p = build(&Ctx { machine: Some(&m), ..Default::default() });
        let corte = p.find(CACHE_BOUNDARY).expect("falta la marca");
        assert!(p[..corte].contains("Eres Lucy"), "la identidad es estable");
        assert!(p[corte..].contains("WORKSTATION-16"), "el equipo cambia");
        assert!(!p[..corte].contains("WORKSTATION-16"));
    }

    #[test]
    fn sin_nada_que_cambie_no_hay_marca() {
        let _s = serie();
        // Poner la marca al final del todo la haría inútil y confundiría a quien
        // parta por ella: no habría segunda mitad.
        let p = build(&Ctx::default());
        assert!(!p.contains(CACHE_BOUNDARY));
    }

    #[test]
    fn una_seccion_apagada_no_sale_y_vuelve_al_encenderla() {
        let _s = serie();
        // El interruptor existe para aislar: cuando Lucy contesta raro, apagar
        // una sección y repetir la orden dice en un turno si era esa.
        assert!(build(&Ctx::default()).contains("CREDENCIALES"));
        disable("Safety");
        assert!(!build(&Ctx::default()).contains("CREDENCIALES"));
        assert_eq!(disabled(), vec!["Safety".to_string()]);
        enable("Safety");
        assert!(build(&Ctx::default()).contains("CREDENCIALES"));
    }

    #[test]
    fn el_prompt_de_un_modelo_flojo_es_otro_y_no_el_mismo_recortado() {
        let _s = serie();
        // Cuatro reglas se cumplen; cuarenta se ignoran y se contesta en prosa
        // sin emitir ninguna etiqueta.
        let m = snap();
        let full = build(&Ctx { machine: Some(&m), ..Default::default() });
        let weak = build(&Ctx { machine: Some(&m), weak_model: true, ..Default::default() });
        assert!(weak.len() * 3 < full.len(), "{} vs {}", weak.len(), full.len());
        // Lo que no se puede perder: quién es y en qué equipo está.
        assert!(weak.contains("Eres Lucy"));
        assert!(weak.contains("WORKSTATION-16"));
        assert!(weak.contains("<EXECUTE>"));
    }

    #[test]
    fn el_nivel_lo_dice_el_catalogo_y_no_el_nombre() {
        let _s = serie();
        // Lo económico es flojo, lo demás no. Probado con los que la heurística
        // vieja clasificaba MAL, que son los que importan:
        //
        //   • «flash» dejó de querer decir pequeño. El catálogo describe a
        //     `gemini-3.5-flash` como «rendimiento de frontera sostenido», y en
        //     una prueba real compuso un comando compuesto y analizó tres
        //     ficheros. Se le estaba mandando el prompt corto, sin herramientas.
        assert!(!model_is_weak("gemini-3.5-flash"), "◐ equilibrado no es flojo");
        assert!(!model_is_weak("gemini-3.6-flash"), "◐ equilibrado no es flojo");
        assert!(!model_is_weak("deepseek-v4-flash"), "◇ intermedio no es flojo");
        assert!(!model_is_weak("claude-haiku-4-5"), "▸ rápido no es flojo");
        //   • y al revés: un económico que el nombre no delataba se estaba
        //     llevando el prompt entero.
        assert!(model_is_weak("gpt-5.6-luna"), "◯ económico sí es flojo");
        assert!(model_is_weak("gemini-3.5-flash-lite"));
        assert!(model_is_weak("mistralai/mistral-7b-instruct-v0.3"));

        // Lo de arriba del catálogo, intacto.
        assert!(!model_is_weak("claude-opus-5"));
        assert!(!model_is_weak("gemini-3.1-pro-preview"));
        // Y con sufijo de esfuerzo, que es como llegan de verdad del selector.
        assert!(!model_is_weak("claude-opus-5::xhigh"));
    }

    #[test]
    fn un_modelo_local_se_juzga_por_su_tamano() {
        let _s = serie();
        // Los de Ollama no están en el catálogo —se descubren en la máquina—,
        // así que ahí sí hay que adivinar, y el tamaño en el nombre es lo único
        // que hay.
        assert!(model_is_weak("llama3:8b"));
        assert!(model_is_weak("qwen3:3b"));
        assert!(!model_is_weak("llama3:70b"));
        // `mini` suelto haría que "geMINI" entrara aquí. El guion lo evita.
        assert!(!model_is_weak("gemini-loquesea"));
    }

    #[test]
    fn el_perfil_del_operador_solo_sale_si_lo_hay() {
        let _s = serie();
        // "El operador se llama (vacío)" es peor que no decir nada: el modelo lo
        // trata como un dato y contesta a un nombre en blanco.
        assert!(!build(&Ctx::default()).contains("El operador se llama"));
        let p = build(&Ctx { user_name: "Iván", ..Default::default() });
        assert!(p.contains("El operador se llama Iván"));
    }

    #[test]
    fn con_hosts_configurados_se_le_ofrece_la_ejecucion_remota() {
        let _s = serie();
        // Antes esta sección le decía que lo remoto «TODAVÍA NO está migrado» y
        // le pedía que diera el comando para copiar a mano. Era cierto hasta que
        // se migró NexShell; desde entonces el registro de equipos y
        // `hosts::run_remote` existen, y la frase se quedó pidiéndole que no
        // usara algo que sí funciona.
        let p = build(&Ctx { hosts: "srv-01 (10.0.0.5)", ..Default::default() });
        assert!(p.contains("srv-01"));
        assert!(p.contains("EXECUTE_REMOTE"), "no se le ofrece la etiqueta");
        assert!(!p.contains("TODAVÍA NO"), "sigue diciendo que no se puede");
        // Y la advertencia que sí sigue en pie: un <EXECUTE> a secas corre AQUÍ.
        assert!(p.contains("equipo equivocado"));
        // El shell del equipo de destino, que no tiene por qué ser PowerShell.
        assert!(p.contains("bash"), "no le dice que un Linux lleva bash");
    }
}
