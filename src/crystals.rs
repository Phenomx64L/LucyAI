//! Cristales: lo que quedó de una sesión entera, destilado por el modelo local.
//!
//! Una memoria automática (`memories::from_turn`) guarda UN turno: qué se
//! preguntó, qué corrió, a qué se llegó. Sirve para «¿ya miré esto?», y no sirve
//! para «¿cómo arreglé lo del certificado de WinRM hace tres semanas?» — porque
//! aquello no fue un turno, fueron once, y ninguno por separado contiene la
//! respuesta.
//!
//! El cristal es esa segunda cosa: una narración de la sesión, los desenlaces,
//! los ficheros que se tocaron y —lo único que de verdad se reutiliza— las
//! lecciones. Y las lecciones se promueven a memorias normales, porque un cristal
//! que solo se ve en su propia pestaña es documentación, y la documentación que
//! hay que ir a buscar no se busca.
//!
//! CUESTA UNA LLAMADA AL MODELO, y por eso todo lo de aquí está construido
//! alrededor de no hacerla. Las puertas son puras y baratas, se evalúan ANTES de
//! despertar a Ollama, y hay como mucho un cristal por sesión — así el bucle del
//! agente puede invocar esto al cerrar cada turno sin pensárselo.

use std::sync::atomic::{AtomicBool, Ordering};

// ── Las puertas ─────────────────────────────────────────────────────────────

/// Turnos mínimos. Por debajo no hubo sesión, hubo una pregunta.
pub const MIN_TURNOS: usize = 4;

/// Herramientas o comandos mínimos.
///
/// Es la misma línea que traza `memories::merece` para un turno suelto y por el
/// mismo motivo: lo que merece guardarse es un desenlace medido en la máquina.
/// Una sesión de cuarenta turnos que solo conversó no destila nada.
pub const MIN_HERRAMIENTAS: usize = 3;

/// Caracteres mínimos de transcripción.
pub const MIN_CARACTERES: usize = 800;

/// Tope de lo que se le manda al modelo.
///
/// El contexto de un modelo local es finito y esto es un resumen de una pasada,
/// no una lectura profunda. Lo que pasa de aquí se corta y se dice que se cortó,
/// en vez de dejar que Ollama trunque por su cuenta por el lado que le toque.
pub const MAX_CARACTERES: usize = 24_000;

/// Estado de la sesión al cerrar un turno. Lo que hace falta para decidir.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sesion {
    pub turnos: usize,
    /// Comandos ejecutados + herramientas de lectura cumplidas, acumulados.
    pub herramientas: usize,
    pub caracteres: usize,
}

/// ¿Esta sesión merece que se despierte al modelo?
///
/// PURA y con el motivo dentro del `Err`: cuando alguien pregunta por qué no se
/// cristalizó nada, la respuesta tiene que existir. La V2 devolvía el motivo en
/// un campo `reason` que la interfaz no enseñaba en ninguna parte.
pub fn merece(s: &Sesion) -> Result<(), String> {
    if s.turnos < MIN_TURNOS {
        return Err(format!("solo {} turnos (hacen falta {MIN_TURNOS})", s.turnos));
    }
    if s.herramientas < MIN_HERRAMIENTAS {
        return Err(format!(
            "solo {} herramientas (hacen falta {MIN_HERRAMIENTAS})",
            s.herramientas
        ));
    }
    if s.caracteres < MIN_CARACTERES {
        return Err(format!(
            "transcripción de {} caracteres (hacen falta {MIN_CARACTERES})",
            s.caracteres
        ));
    }
    Ok(())
}

// ── Qué modelo destila ──────────────────────────────────────────────────────

const OLLAMA: &str = "http://localhost:11434";

/// Plazo de la destilación.
///
/// Noventa segundos, más que los cuarenta y cinco de la V2: esto corre en un
/// hilo de fondo donde nadie espera, y un modelo de 7B leyendo veinticuatro mil
/// caracteres en una máquina sin GPU tarda más que eso. Un plazo corto no ahorra
/// nada aquí, solo tira la destilación cuando ya estaba pagada.
pub const TIMEOUT_SECS: u64 = 90;

/// Orden de preferencia para destilar.
///
/// EN VEZ DE UN MODELO FIJO, y esto no es cosmética. La V2 pedía `qwen3:4b` a
/// pelo; en esta máquina hay instalados `mistral`, `qwen3`, `qwen3:0.6b`,
/// `deepseek-r1:1.5b` y `deepseek-r1:7b`, y NO `qwen3:4b`. O sea que la
/// cristalización automática de la V2 no es que fallara a veces: no ha llegado a
/// ocurrir nunca en esta instalación, y el error —Ollama contesta «model not
/// found»— se tragaba entero el `skip()` que no muestra nadie.
///
/// El orden pone delante los que siguen un formato sin discutirlo. Los razonadores
/// (`deepseek-r1`) van al final: gastan el presupuesto de salida pensando en voz
/// alta antes de escribir la primera etiqueta.
pub const PREFERIDOS: &[&str] = &[
    "mistral",
    "llama3.1",
    "llama3.2",
    "llama3",
    "gemma3",
    "gemma2",
    "phi4",
    "phi3",
    "qwen2.5",
    "qwen3",
    "deepseek-r1",
];

/// Elige, de entre los instalados, el mejor para destilar.
///
/// `None` = Ollama no contesta o no hay ningún modelo de texto. Los de embeddings
/// se excluyen: `nomic-embed-text` no tiene `/api/chat` y pedírselo devuelve un
/// error que parece de red.
pub fn modelo() -> Option<String> {
    elige(&crate::chat::list_models())
}

/// La parte pura de `modelo`, para poder fijarla en un test sin Ollama.
pub fn elige(instalados: &[String]) -> Option<String> {
    let texto: Vec<&String> = instalados
        .iter()
        .filter(|m| {
            let b = m.to_ascii_lowercase();
            !b.contains("embed") && !b.contains("bge-") && !b.contains("minilm")
        })
        .collect();
    for p in PREFERIDOS {
        if let Some(m) = texto.iter().find(|m| m.to_ascii_lowercase().starts_with(p)) {
            return Some((*m).clone());
        }
    }
    // Ninguno conocido, pero hay algo. Destilar con un modelo raro es peor que
    // con uno bueno y mucho mejor que no destilar: el formato de salida se
    // valida igual, y si no lo cumple se descarta sin guardar nada.
    texto.first().map(|m| (*m).clone())
}

// ── El formato ──────────────────────────────────────────────────────────────

/// XML y no JSON, por lo mismo que en la V2: un modelo pequeño se deja el
/// paréntesis o mete una coma de más y el JSON entero se pierde; en XML una
/// etiqueta mal cerrada solo pierde ese elemento.
/// SIN ETIQUETAS CONTENEDORAS, y eso está medido en esta máquina. Con el formato
/// anidado de la V2 —`<outcomes>` alrededor de `<outcome>`— `mistral` devolvió
/// esto, tal cual:
///
/// ```text
/// <outcomes>El servicio Spooler está arriba y acepta trabajos.</outcome>
/// <lessons>Conviene poner un aviso si PRINTERS pasa de 100 ficheros.</lesson>
/// ```
///
/// Abre el plural y cierra el singular: colapsa los dos niveles en uno. El
/// resultado se parseaba a cero hitos y cero lecciones —o sea, un cristal con
/// narración y nada más, que es justo la parte que no se reutiliza— y no daba
/// error en ninguna parte. Un nivel menos es una cosa menos que un modelo
/// pequeño puede equivocar.
const SISTEMA: &str = "Eres el destilador de memoria de una asistente de administración de sistemas. \
Te dan la transcripción de una sesión terminada y la resumes en una memoria duradera.

Devuelve EXACTAMENTE este XML, sin markdown, sin comentarios y sin bloques de código:
<crystal>
<narrative>Una o dos frases, en pasado, sobre qué se consiguió.</narrative>
<outcome>Un resultado o decisión concreta (menos de 100 caracteres)</outcome>
<outcome>Otro resultado</outcome>
<file>ruta/al/fichero</file>
<lesson>Un patrón reutilizable, menos de 120 caracteres</lesson>
</crystal>

Reglas:
- REPITE la etiqueta por cada elemento. NO uses <outcomes>, <files> ni <lessons>.
- De 2 a 5 <outcome>, de 0 a 10 <file>, de 1 a 4 <lesson>.
- Las lecciones tienen que servir la próxima vez (\"WinRM sobre HTTPS necesita el \
certificado en LocalMachine\\My\"), no describir esta (\"se reinició el servicio a las 3\").
- Si algo está vacío, omítelo entero. NO escribas <outcome></outcome>.
- Escribe en el idioma de la transcripción.
- Solo el bloque <crystal>. Sin preámbulo.";

/// Lo que el modelo devolvió, ya validado.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Crudo {
    pub narrativa: String,
    pub hitos: Vec<String>,
    pub archivos: Vec<String>,
    pub lecciones: Vec<String>,
}

pub const MAX_HITOS: usize = 5;
pub const MAX_ARCHIVOS: usize = 10;
/// Tope de lecciones que se guardan Y se promueven.
///
/// Cuatro, que es lo que pide el prompt. La V2 guardaba en el cristal TODAS las
/// que emitiera el modelo y solo capaba a seis al promoverlas — así que un modelo
/// local desbocado dejaba cuarenta lecciones en la fila del cristal y seis en
/// memoria, y las dos cifras se contradecían en pantalla.
pub const MAX_LECCIONES: usize = 4;
/// Tope de cada línea. El prompt pide menos de 120; esto es el seguro.
const MAX_LINEA: usize = 240;
const MAX_NARRATIVA: usize = 600;

/// Quita el pensamiento en voz alta.
///
/// HACE FALTA DE VERDAD. `qwen3` y `deepseek-r1` emiten `<think>…</think>` antes
/// de contestar, y ahí dentro ensayan la respuesta —incluidas las etiquetas—. La
/// V2 cogía el PRIMER `<narrative>` del texto, que en un modelo de estos es el
/// del borrador: el cristal se quedaba con el ensayo y descartaba la respuesta.
pub(crate) fn sin_pensamiento(s: &str) -> &str {
    match s.rfind("</think>") {
        Some(i) => &s[i + "</think>".len()..],
        None => s,
    }
}

/// Se queda con el ÚLTIMO `<crystal>`, por el mismo motivo: si el modelo enseña
/// un ejemplo antes de la respuesta buena, la buena es la de después.
fn ultimo_bloque(s: &str) -> &str {
    match s.rfind("<crystal>") {
        Some(i) => &s[i..],
        None => s,
    }
}

pub(crate) fn recoge(xml: &str, tag: &str) -> Vec<String> {
    let abre = format!("<{tag}>");
    let cierra = format!("</{tag}>");
    let mut out = Vec::new();
    let mut cur = 0usize;
    while let Some(s) = xml[cur..].find(&abre) {
        let ini = cur + s + abre.len();
        let Some(e) = xml[ini..].find(&cierra) else { break };
        let dentro = xml[ini..ini + e].trim();
        if !dentro.is_empty() {
            out.push(corta(dentro, MAX_LINEA));
        }
        cur = ini + e + cierra.len();
    }
    out
}

/// Lo mismo, pero rescatando lo que el modelo metió DENTRO del contenedor.
///
/// El prompt ya no pide contenedores, pero los modelos pequeños los emiten igual
/// porque es el patrón que han visto mil veces. Cuando eso pasa el texto queda
/// entre `<outcomes>` y el primer `<`, que es donde se mira aquí.
///
/// SE SUMAN LAS DOS PASADAS en vez de elegir una: `<lessons>a</lesson><lesson>b</lesson>`
/// es medio colapso —una dentro del contenedor y otra bien puesta— y quedarse con
/// la pasada estricta perdería la primera en silencio.
fn recoge_flexible(xml: &str, item: &str, contenedor: &str) -> Vec<String> {
    let mut out = Vec::new();
    let abre = format!("<{contenedor}>");
    let mut cur = 0usize;
    while let Some(s) = xml[cur..].find(&abre) {
        let ini = cur + s + abre.len();
        let fin = xml[ini..].find('<').map_or(xml.len(), |i| ini + i);
        for linea in xml[ini..fin].lines() {
            let l = linea.trim();
            if !l.is_empty() {
                out.push(corta(l, MAX_LINEA));
            }
        }
        cur = ini;
    }
    for v in recoge(xml, item) {
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

pub(crate) fn corta(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

/// Lee la respuesta del modelo. `Err` = no cumplió el formato.
///
/// Sin narrativa no hay cristal: es el único campo que no puede faltar, porque es
/// el que contesta «¿de qué iba aquella sesión?». Una fila con las lecciones y sin
/// narración es un montón de consejos sin contexto.
pub fn parse(salida: &str) -> Result<Crudo, String> {
    let xml = ultimo_bloque(sin_pensamiento(salida));
    let narrativa = recoge(xml, "narrative")
        .into_iter()
        .next()
        .ok_or_else(|| {
            format!(
                "el modelo no devolvió <narrative>: {}",
                corta(salida.trim(), 160)
            )
        })?;
    let mut hitos = recoge_flexible(xml, "outcome", "outcomes");
    hitos.truncate(MAX_HITOS);
    let mut archivos = recoge_flexible(xml, "file", "files");
    archivos.truncate(MAX_ARCHIVOS);
    let mut lecciones = recoge_flexible(xml, "lesson", "lessons");
    lecciones.truncate(MAX_LECCIONES);
    Ok(Crudo {
        narrativa: corta(&narrativa, MAX_NARRATIVA),
        hitos,
        archivos,
        lecciones,
    })
}

/// Pide la destilación a Ollama. Bloqueante: quien llama ya está en un hilo.
pub fn destila(transcripcion: &str, modelo: &str) -> Result<Crudo, String> {
    let t = transcripcion.trim();
    let trozo = if t.chars().count() > MAX_CARACTERES {
        let cortado: String = t.chars().take(MAX_CARACTERES).collect();
        format!("{cortado}\n\n[transcripción cortada en {MAX_CARACTERES} caracteres]")
    } else {
        t.to_string()
    };
    let body = serde_json::json!({
        "model": modelo,
        "messages": [
            { "role": "system", "content": SISTEMA },
            { "role": "user", "content": format!("Transcripción de la sesión:\n\n{trozo}") },
        ],
        "stream": false,
        // Dos mil y no los mil doscientos de la V2: un razonador se gasta la
        // mitad del presupuesto pensando antes de abrir la primera etiqueta, y un
        // cristal cortado a la mitad no parsea.
        "options": { "temperature": 0.3, "num_predict": 2000 },
    });
    let resp = ureq::post(&format!("{OLLAMA}/api/chat"))
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .send_string(&body.to_string())
        .map_err(|e| format!("Ollama no respondió ({OLLAMA}): {e}"))?;
    let cuerpo = resp
        .into_string()
        .map_err(|e| format!("respuesta de Ollama ilegible: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&cuerpo).map_err(|e| format!("JSON inválido de Ollama: {e}"))?;
    // Ollama devuelve 200 con un `error` dentro cuando el modelo no está. Sin
    // esta rama el mensaje que llegaba arriba era «no devolvió <narrative>», que
    // manda a mirar el prompt en vez de a instalar el modelo.
    if let Some(e) = json.get("error").and_then(|e| e.as_str()) {
        return Err(format!("Ollama: {e}"));
    }
    let salida = json
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    parse(salida)
}

// ── La fila ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Cristal {
    pub id: i64,
    pub session_id: String,
    pub narrativa: String,
    pub hitos: Vec<String>,
    pub archivos: Vec<String>,
    pub lecciones: Vec<String>,
    pub caracteres: i64,
    pub creado: i64,
}

pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_crystals (
                 id              INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id      TEXT    NOT NULL DEFAULT '',
                 project         TEXT    NOT NULL DEFAULT '',
                 narrative       TEXT    NOT NULL,
                 key_outcomes    TEXT    NOT NULL DEFAULT '[]',
                 files_affected  TEXT    NOT NULL DEFAULT '[]',
                 lessons         TEXT    NOT NULL DEFAULT '[]',
                 source_chars    INTEGER NOT NULL DEFAULT 0,
                 created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now'))
             );
             CREATE INDEX IF NOT EXISTS idx_agent_crystals_session
                 ON agent_crystals(session_id, created_at DESC);
             CREATE INDEX IF NOT EXISTS idx_agent_crystals_created
                 ON agent_crystals(created_at DESC);",
        )
        .map_err(|e| format!("crystals: esquema: {e}"))
    })
}

/// ¿Ya hay cristal de esta sesión?
///
/// La sonda barata que hace idempotente llamar a esto en cada cierre de turno.
pub fn existe(session_id: &str) -> bool {
    if session_id.trim().is_empty() {
        return false;
    }
    ensure_schema().ok();
    crate::with_db(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM agent_crystals WHERE session_id = ?1",
            rusqlite::params![session_id],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())
    })
    .unwrap_or(0)
        > 0
}

fn lista_json(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn lista_de(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

/// Guarda el cristal y promueve sus lecciones. Devuelve `(id, lecciones nuevas)`.
///
/// EL TEXTO PASA POR EL FILTRO DE SECRETOS ANTES DE ENTRAR. En la V2 no: las
/// lecciones se limpiaban al promoverlas —porque las limpia `save_agent_memory`—
/// pero la fila del cristal se insertaba tal cual, y esa fila es justo la que se
/// enseña en el visor. Una contraseña que el modelo copiara de la transcripción a
/// una narración se quedaba visible ahí para siempre.
pub fn guardar(session_id: &str, c: &Crudo, caracteres: usize) -> Result<(i64, usize), String> {
    ensure_schema()?;
    let narrativa = crate::memories::scrub(&c.narrativa);
    let hitos: Vec<String> = c.hitos.iter().map(|s| crate::memories::scrub(s)).collect();
    let archivos: Vec<String> = c.archivos.iter().map(|s| crate::memories::scrub(s)).collect();
    let lecciones: Vec<String> = c.lecciones.iter().map(|s| crate::memories::scrub(s)).collect();

    let id = crate::with_db(|conn| {
        conn.execute(
            "INSERT INTO agent_crystals
                 (session_id, narrative, key_outcomes, files_affected, lessons, source_chars)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                session_id,
                narrativa,
                lista_json(&hitos),
                lista_json(&archivos),
                lista_json(&lecciones),
                caracteres as i64,
            ],
        )
        .map_err(|e| format!("crystals: insert: {e}"))?;
        Ok(conn.last_insert_rowid())
    })?;

    // ── Promoción ────────────────────────────────────────────────────────────
    // Un cristal que solo vive en su pestaña es documentación, y la documentación
    // que hay que ir a buscar no se busca. Las lecciones entran por la puerta
    // normal de las memorias, con su deduplicación: una sesión larga sobre lo
    // mismo de siempre no vuelve a escribir las mismas cuatro lecciones.
    let mut nuevas = 0usize;
    for l in &lecciones {
        let n = crate::memories::New {
            title: format!("Lección: {}", corta(l, 60)),
            content: l.clone(),
            tags: vec!["crystal".into(), "leccion".into()],
            session_id: session_id.to_string(),
            // Dos, como una memoria automática de turno: es un desenlace
            // destilado, no algo que marcara una persona.
            importance: 2,
        };
        if let Ok(g) = crate::memories::save(&n) {
            if g.es_nueva() {
                nuevas += 1;
            }
        }
    }
    // Y la narración, que es lo que contesta «¿ya me pasó esto?». Las lecciones
    // dicen el CÓMO; sin esta fila, la pregunta por el QUÉ no encuentra nada.
    // Importancia uno: es contexto de sesión, no un hecho duro, y no debe ganarle
    // el sitio a uno en el recuerdo.
    let mut cuerpo = narrativa.clone();
    if !hitos.is_empty() {
        cuerpo.push_str("\n\n");
        for h in &hitos {
            cuerpo.push_str(&format!("- {h}\n"));
        }
    }
    // Las rutas van DENTRO del cuerpo y no en la columna `files`: así las
    // encuentra la búsqueda por texto, que es como se pregunta por ellas
    // («¿tocamos algo en System32?»).
    if !archivos.is_empty() {
        cuerpo.push_str(&format!("\nFicheros: {}", archivos.join(", ")));
    }
    let _ = crate::memories::save(&crate::memories::New {
        title: format!("Sesión: {}", corta(&narrativa, 60)),
        content: cuerpo,
        tags: vec!["crystal".into(), "sesion".into()],
        session_id: session_id.to_string(),
        importance: 1,
    });
    Ok((id, nuevas))
}

/// Cómo acabó el intento. Nunca es un error: una puerta cerrada o un Ollama caído
/// no pueden romper el cierre de un turno.
#[derive(Debug, Clone, PartialEq)]
pub struct Resultado {
    pub id: Option<i64>,
    /// Qué pasó, en una línea que se puede enseñar.
    pub motivo: String,
    /// Lecciones que resultaron ser nuevas.
    pub lecciones: usize,
    /// Si merece la pena volver a intentarlo en esta sesión.
    ///
    /// LA DISTINCIÓN EXISTE POR UN CASO CONCRETO: un modelo instalado que no
    /// cumple el formato —un razonador que quema los dos mil tokens pensando—
    /// falla IGUAL en cada intento, y reintentar al cerrar cada turno son noventa
    /// segundos de Ollama por turno, para siempre. Eso no es transitorio y no se
    /// reintenta. Un Ollama que no contesta sí lo es: arrancarlo lo arregla.
    pub reintentable: bool,
}

impl Resultado {
    /// Fallo transitorio: reintentar en el próximo cierre puede salir bien.
    fn no(motivo: impl Into<String>) -> Self {
        Self { id: None, motivo: motivo.into(), lecciones: 0, reintentable: true }
    }

    /// Fallo determinista o caso cerrado: volver a intentarlo dará lo mismo.
    fn nunca(motivo: impl Into<String>) -> Self {
        Self { reintentable: false, ..Self::no(motivo) }
    }
}

/// El camino entero: puertas, modelo, destilación, fila, promoción.
///
/// Se puede llamar al cerrar cada turno. Todo lo caro está detrás de algo barato.
pub fn cristaliza(
    session_id: &str,
    transcripcion: &str,
    s: &Sesion,
    stop: &AtomicBool,
) -> Resultado {
    if session_id.trim().is_empty() {
        return Resultado::nunca("sesión sin identificador");
    }
    // Una puerta cerrada es reintentable por definición: la sesión sigue
    // creciendo y el turno siguiente puede abrirla.
    if let Err(e) = merece(s) {
        return Resultado::no(e);
    }
    // Antes de Ollama, que es lo único que cuesta.
    if existe(session_id) {
        return Resultado::nunca("esta sesión ya tiene cristal");
    }
    let Some(m) = modelo() else {
        // Transitorio: arrancar Ollama o instalar un modelo lo arregla.
        return Resultado::no("no hay ningún modelo de texto en Ollama");
    };
    if stop.load(Ordering::Relaxed) {
        return Resultado::no("cancelado");
    }
    let crudo = match destila(transcripcion, &m) {
        Ok(c) => c,
        // Ollama caído a media petición = transitorio. El formato incumplido =
        // determinista: el mismo modelo con la misma transcripción va a volver a
        // no cumplirlo, y cada reintento cuesta la petición entera.
        Err(e) if e.contains("no respondió") || e.contains("ilegible") => {
            return Resultado::no(format!("no se pudo destilar: {e}"))
        }
        Err(e) => return Resultado::nunca(format!("no se pudo destilar: {e}")),
    };
    // La destilación tarda; en ese hueco cabe otro cierre de turno de la misma
    // sesión. Se vuelve a mirar antes de escribir para no dejar dos cristales.
    if existe(session_id) {
        return Resultado::nunca("esta sesión ya tiene cristal");
    }
    match guardar(session_id, &crudo, transcripcion.chars().count()) {
        Ok((id, nuevas)) => Resultado {
            id: Some(id),
            motivo: format!("cristalizada con {m}"),
            lecciones: nuevas,
            reintentable: false,
        },
        Err(e) => Resultado::no(format!("no se pudo guardar: {e}")),
    }
}

/// Los cristales, del más reciente al más viejo.
pub fn list(limite: usize) -> Result<Vec<Cristal>, String> {
    ensure_schema()?;
    let lim = limite.clamp(1, 200) as i64;
    crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT id, session_id, narrative, key_outcomes, files_affected,
                        lessons, source_chars, created_at
                 FROM agent_crystals ORDER BY created_at DESC, id DESC LIMIT ?1",
            )
            .map_err(|e| format!("crystals: listar: {e}"))?;
        let v = st
            .query_map([lim], |r| {
                Ok(Cristal {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    narrativa: r.get(2)?,
                    hitos: lista_de(&r.get::<_, String>(3)?),
                    archivos: lista_de(&r.get::<_, String>(4)?),
                    lecciones: lista_de(&r.get::<_, String>(5)?),
                    caracteres: r.get(6)?,
                    creado: r.get(7)?,
                })
            })
            .map_err(|e| format!("crystals: listar: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })
}

/// Borra un cristal. Las memorias que promovió se quedan: son suyas desde que se
/// guardaron, y borrarlas con el cristal quitaría lecciones que ya se habían
/// confirmado en otras sesiones.
pub fn delete(id: i64) -> Result<(), String> {
    crate::with_db(|c| {
        c.execute("DELETE FROM agent_crystals WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("crystals: borrar: {e}"))?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sesion(t: usize, h: usize, c: usize) -> Sesion {
        Sesion { turnos: t, herramientas: h, caracteres: c }
    }

    #[test]
    fn las_puertas_dicen_cual_falto() {
        // El motivo tiene que existir: sin él, «¿por qué no cristalizó?» no tiene
        // respuesta y la función parece rota cuando está funcionando.
        assert!(merece(&sesion(2, 9, 9_000)).unwrap_err().contains("turnos"));
        assert!(merece(&sesion(9, 1, 9_000)).unwrap_err().contains("herramientas"));
        assert!(merece(&sesion(9, 9, 100)).unwrap_err().contains("caracteres"));
        assert!(merece(&sesion(MIN_TURNOS, MIN_HERRAMIENTAS, MIN_CARACTERES)).is_ok());
    }

    #[test]
    fn una_sesion_larga_que_solo_converso_no_destila() {
        // Cuarenta turnos y treinta mil caracteres, y ni un comando. Es la charla
        // que la puerta de herramientas existe para dejar fuera.
        assert!(merece(&sesion(40, 0, 30_000)).is_err());
    }

    #[test]
    fn el_ensayo_de_un_modelo_pensante_no_se_confunde_con_la_respuesta() {
        // EL caso que la V2 tenía mal. `qwen3` y `deepseek-r1` ensayan dentro de
        // <think>, etiquetas incluidas; coger el primer <narrative> se queda con
        // el borrador y tira la respuesta buena.
        let salida = "<think>A ver, pondría <narrative>Borrador equivocado</narrative> \
                      pero mejor lo reescribo.</think>\
                      <crystal><narrative>Se arregló el certificado de WinRM</narrative>\
                      <lessons><lesson>El certificado va en LocalMachine\\My</lesson></lessons>\
                      </crystal>";
        let c = parse(salida).expect("debe parsear");
        assert_eq!(c.narrativa, "Se arregló el certificado de WinRM");
        assert_eq!(c.lecciones.len(), 1);
    }

    #[test]
    fn de_dos_bloques_gana_el_ultimo() {
        let salida = "<crystal><narrative>Ejemplo</narrative></crystal>\n\
                      Ahora el de verdad:\n\
                      <crystal><narrative>El bueno</narrative></crystal>";
        assert_eq!(parse(salida).unwrap().narrativa, "El bueno");
    }

    #[test]
    fn sin_narrativa_no_hay_cristal() {
        let e = parse("<crystal><lessons><lesson>algo</lesson></lessons></crystal>").unwrap_err();
        assert!(e.contains("narrative"), "el error debe decir qué faltó: {e}");
        // Y el error lleva un trozo de lo que dijo el modelo, que es lo único con
        // lo que se puede diagnosticar un formato incumplido.
        assert!(parse("me parece que la sesión fue bien").unwrap_err().contains("sesión fue bien"));
    }

    #[test]
    fn el_contenedor_colapsado_que_devolvio_mistral_en_esta_maquina() {
        // COPIADO LITERAL de lo que salió al probar contra el Ollama de aquí.
        // Abre el plural y cierra el singular. Con el parseo estricto —el de la
        // V2— esto da cero hitos y cero lecciones: un cristal con narración y sin
        // nada reutilizable, y sin un error que lo delate.
        let salida = " <crystal>\n\
          <narrative>La cola de impresión se atascó y el Spooler se detuvo.</narrative>\n\
          <outcomes>El servicio Spooler está arriba y acepta trabajos.</outcome>\n\
          <files></files>\n\
          <lessons>Conviene avisar si PRINTERS pasa de 100 ficheros.</lesson>\n\
        </crystal>";
        let c = parse(salida).expect("debe parsear");
        assert_eq!(c.hitos, vec!["El servicio Spooler está arriba y acepta trabajos.".to_string()]);
        assert_eq!(c.lecciones, vec!["Conviene avisar si PRINTERS pasa de 100 ficheros.".to_string()]);
        assert!(c.archivos.is_empty(), "<files></files> vacío no es un fichero");
    }

    #[test]
    fn un_colapso_a_medias_no_pierde_la_de_dentro() {
        // Una dentro del contenedor y otra bien puesta. Quedarse solo con la
        // pasada estricta tiraría la primera sin decir nada.
        let salida = "<crystal><narrative>N</narrative>\
                      <lessons>La de dentro</lessons><lesson>La de fuera</lesson></crystal>";
        let c = parse(salida).unwrap();
        assert_eq!(c.lecciones, vec!["La de dentro".to_string(), "La de fuera".to_string()]);
    }

    #[test]
    fn las_etiquetas_vacias_no_cuentan_como_elementos() {
        // El prompt las prohíbe y los modelos pequeños las emiten igual. Sin este
        // filtro, un cristal enseña cinco viñetas en blanco.
        let c = parse(
            "<crystal><narrative>N</narrative>\
             <outcomes><outcome></outcome><outcome>  </outcome><outcome>Real</outcome></outcomes>\
             </crystal>",
        )
        .unwrap();
        assert_eq!(c.hitos, vec!["Real".to_string()]);
    }

    #[test]
    fn un_modelo_desbocado_no_deja_cuarenta_lecciones() {
        // La V2 capaba a seis al PROMOVER pero guardaba todas en la fila, así que
        // el visor y la memoria enseñaban dos cifras distintas de lo mismo.
        let mut xml = String::from("<crystal><narrative>N</narrative><lessons>");
        for i in 0..40 {
            xml.push_str(&format!("<lesson>lección {i}</lesson>"));
        }
        xml.push_str("</lessons></crystal>");
        let c = parse(&xml).unwrap();
        assert_eq!(c.lecciones.len(), MAX_LECCIONES);
    }

    #[test]
    fn una_leccion_kilometrica_se_corta() {
        let larga = "x".repeat(MAX_LINEA + 500);
        let xml = format!("<crystal><narrative>N</narrative><lessons><lesson>{larga}</lesson></lessons></crystal>");
        let c = parse(&xml).unwrap();
        assert!(c.lecciones[0].chars().count() <= MAX_LINEA + 1);
    }

    #[test]
    fn se_elige_un_modelo_que_este_instalado() {
        // El fallo real: la V2 pedía `qwen3:4b` a pelo y en esta máquina no está.
        let instalados: Vec<String> = ["nomic-embed-text:latest", "qwen3:0.6b", "mistral:latest"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(elige(&instalados).as_deref(), Some("mistral:latest"));
    }

    #[test]
    fn el_embebedor_nunca_destila() {
        // `nomic-embed-text` no tiene /api/chat: pedírselo devuelve un error que
        // parece de red y manda a mirar si Ollama está levantado.
        let solo_embed = vec!["nomic-embed-text:latest".to_string()];
        assert_eq!(elige(&solo_embed), None);
    }

    #[test]
    fn un_razonador_es_el_ultimo_recurso_pero_es_un_recurso() {
        let v: Vec<String> = vec!["deepseek-r1:7b".into(), "qwen3:latest".into()];
        // qwen3 va antes que deepseek-r1 en la lista de preferencia.
        assert_eq!(elige(&v).as_deref(), Some("qwen3:latest"));
        assert_eq!(elige(&["deepseek-r1:7b".to_string()]).as_deref(), Some("deepseek-r1:7b"));
    }

    #[test]
    fn sin_ollama_no_se_intenta_y_se_dice() {
        // Sesión que pasa todas las puertas; lo que falla es el destilador. El
        // motivo tiene que llegar arriba, no un silencio.
        let r = Resultado::no("no hay ningún modelo de texto en Ollama");
        assert!(r.id.is_none());
        assert!(r.motivo.contains("Ollama"));
    }

    #[test]
    fn un_fallo_de_formato_no_se_reintenta_y_uno_de_red_si() {
        // El caso que separa los dos: un razonador instalado que quema el
        // presupuesto pensando falla IGUAL en cada intento, y reintentar al
        // cerrar cada turno son noventa segundos de Ollama por turno, para
        // siempre. Un Ollama caído, en cambio, se arranca y ya.
        assert!(Resultado::no("Ollama no respondió: connection refused").reintentable);
        assert!(!Resultado::nunca("el modelo no devolvió <narrative>").reintentable);
        // Y el éxito no deja nada pendiente de reintentar.
        let ok = Resultado {
            id: Some(1),
            motivo: "cristalizada con mistral".into(),
            lecciones: 2,
            reintentable: false,
        };
        assert!(!ok.reintentable);
    }
}
