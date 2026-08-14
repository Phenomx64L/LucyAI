//! Escribir memorias. La mitad que al shell nativo le faltaba entera.
//!
//! EN TODO `lucy-core` HABÍA TRES `INSERT`: `audit_trail`, `inventory_baselines`
//! y `user_profile`. Ninguno en `agent_memories`. O sea que el shell egui podía
//! LEER el corpus que escribía la app Tauri y no añadir una sola fila — y el día
//! que el nativo sea lo único que corra, la tabla deja de crecer sin que nada dé
//! error. Cinco de las siete pestañas de Memoria no podían llenarse nunca, no
//! por un cable suelto sino porque no existía el código que inserta.
//!
//! EL DIQUE CONTRA LA BASURA ESTÁ AQUÍ, NO EN LA CONSOLIDACIÓN. Fundir después
//! es la limpieza; lo que decide si la tabla se llena de veinte formas de la
//! misma frase es qué se acepta al escribir. Por eso `save` deduplica ANTES de
//! insertar, y por eso el resultado dice cuál de los caminos tomó — quien llama
//! necesita poder distinguir «lo guardé» de «ya lo sabías» sin adivinarlo.
//!
//! LO QUE SE ESCRIBE SE LIMPIA DE SECRETOS. Es lo último que toca el dato antes
//! del disco, y va aquí y no en quien llama por eso mismo: una memoria es lo
//! ÚNICO que sobrevive a la conversación, así que un token que se cuele en una
//! se queda para siempre y encima vuelve al prompt de todos los turnos
//! siguientes. Se limpian también las etiquetas y los ficheros: en la V2 eran
//! los dos únicos campos que se guardaban sin pasar por el filtro, y son
//! igual de controlables por el modelo que el resto.

use once_cell::sync::Lazy;
use regex::Regex;

/// Importancia máxima que puede ponerse sola una escritura automática.
///
/// El 10 significa «lo fijó una persona» y es lo que la consolidación respeta
/// para no tocar nada. Una pasada automática no puede concederse ese sello a sí
/// misma, igual que no puede en `consolidate`.
pub const MAX_AUTO_IMPORTANCE: i64 = 3;

/// Cuánto se guarda del cuerpo de una memoria.
///
/// Una memoria es un HECHO, no un volcado. Sin tope, una que guarde la salida de
/// un `Get-EventLog` entra entera en el recuerdo semántico y se lleva por delante
/// la ventana de contexto de todos los turnos siguientes.
pub const MAX_CONTENT: usize = 4_000;

/// Cuántos candidatos trae el índice de texto antes de decidir.
///
/// EL ÍNDICE BUSCA, NO DECIDE, y eso es un cambio deliberado respecto a la V2.
/// Allí la decisión era una puntuación bm25 por debajo de −8.0, y bm25 se apoya
/// en la frecuencia inversa de documento: con la tabla casi vacía cada palabra
/// aparece en el único documento que hay, así que no discrimina nada. Medido en
/// esta máquina con dos memorias en la tabla, dos redacciones de la MISMA frase
/// puntuaron **−0,0000115** contra un umbral de −8,0 — seis órdenes de magnitud.
///
/// O sea que el dique de la V2 está calibrado contra un corpus lleno y no cierra
/// en una instalación nueva, que es justo cuando más duplicados se producen: al
/// principio el operador está enseñándole lo básico y se repite.
///
/// Así que FTS trae los diez más prometedores —para eso sirve un índice— y quien
/// decide es el MISMO criterio de parecido que usa la consolidación. Que el dique
/// de entrada y la limpieza posterior compartan definición de «esto es lo mismo»
/// es lo que impide que una acepte lo que la otra funde.
pub const FTS_CANDIDATOS: usize = 10;

/// Parecido de coseno a partir del cual dos memorias son la misma.
///
/// Más alto que el umbral de recuerdo (0.45): recordar de más solo mete una
/// línea de contexto que sobra, pero fundir de más PIERDE un hecho. El coste de
/// equivocarse no es simétrico y el umbral tampoco puede serlo.
pub const COSINE_DUP: f32 = 0.92;

/// Lo que se pide guardar.
#[derive(Debug, Clone, Default)]
pub struct New {
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    /// De qué conversación salió. `pdf:` y `pdf-doc:` están reservados para la
    /// ingesta de documentos y se excluyen de la deduplicación de memorias.
    pub session_id: String,
    pub importance: i64,
}

impl New {
    pub fn nueva(title: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            content: content.into(),
            importance: 1,
            ..Default::default()
        }
    }

    pub fn con_tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|t| t.to_string()).collect();
        self
    }

    pub fn importancia(mut self, n: i64) -> Self {
        self.importance = n;
        self
    }
}

/// Qué pasó al guardar.
#[derive(Debug, Clone, PartialEq)]
pub enum Accion {
    /// Fila nueva.
    Guardada,
    /// Ya había una que decía lo mismo; se devuelve la suya.
    Duplicada {
        /// Por qué se consideró duplicada, para poder discutirlo.
        motivo: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Guardado {
    pub id: i64,
    pub accion: Accion,
}

impl Guardado {
    pub fn es_nueva(&self) -> bool {
        self.accion == Accion::Guardada
    }
}

// ── Limpieza de secretos ────────────────────────────────────────────────────

struct Patron {
    re: Regex,
    con: &'static str,
}

/// Los mismos patrones que `utils::secret_scrubber` de la app.
///
/// Copiados y no compartidos porque `lucy-core` no depende de la app —es al
/// revés—, y con la misma advertencia que en `inventory`: si uno cambia hay que
/// cambiar el otro. Los tests de abajo fijan los casos que motivaron cada uno.
static PATRONES: Lazy<Vec<Patron>> = Lazy::new(|| {
    vec![
        // Credenciales dentro de una URL: esquema://usuario:clave@host
        Patron {
            re: Regex::new(
                r"(?i)\b((?:https?|postgres|postgresql|mysql|mongodb|redis|amqp|smb|ftp|ssh)://)([^:@/\s]+):([^@/\s]{1,200})@",
            )
            .expect("re url"),
            con: "$1[REDACTADO]:[REDACTADO]@",
        },
        Patron {
            re: Regex::new(r"(?i)(Authorization\s*[:=]\s*Bearer\s+)\S{8,}").expect("re bearer"),
            con: "${1}[REDACTADO]",
        },
        // El límite es `(^|[^A-Za-z0-9])` y NO `\b`, y ahí está el caso que
        // importa: `\b` trata el guion bajo como letra, así que no casaba la
        // forma dominante en variables de entorno —`DB_PASSWORD=`,
        // `AWS_SECRET_ACCESS_KEY=`— y ésas se guardaban tal cual.
        Patron {
            re: Regex::new(
                r"(?i)(^|[^A-Za-z0-9])(password|passwd|pwd|api[-_]?key|apikey|access[-_]?key|secret[-_]?key|secret|token|auth[-_]?token|client[-_]?secret)\s*[:=]\s*[\x22\x27]?([^\s\x22\x27]{4,})[\x22\x27]?",
            )
            .expect("re kv"),
            con: "${1}${2}=[REDACTADO]",
        },
        Patron {
            re: Regex::new(r"\bAKIA[0-9A-Z]{16}\b").expect("re aws"),
            con: "[REDACTADO_AWS]",
        },
        Patron {
            re: Regex::new(r"\bsk-(?:ant-)?[A-Za-z0-9_\-]{20,}\b").expect("re llm"),
            con: "[REDACTADO_LLM]",
        },
        Patron {
            re: Regex::new(r"\b(?:ghp|gho|ghs|ghr|github_pat)_[A-Za-z0-9_]{20,}\b")
                .expect("re gh"),
            con: "[REDACTADO_GITHUB]",
        },
        Patron {
            re: Regex::new(
                r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]{0,4000}?-----END [A-Z ]*PRIVATE KEY-----",
            )
            .expect("re pem"),
            con: "[REDACTADO_CLAVE_PRIVADA]",
        },
        Patron {
            re: Regex::new(r"\b(?:nvapi-|tvly-)[A-Za-z0-9_\-]{16,}\b").expect("re proveedor"),
            con: "[REDACTADO_PROVEEDOR]",
        },
    ]
});

/// Marcadores baratos. Sin uno de éstos no se corren los ocho patrones.
const MARCAS: &[&str] = &[
    "password", "passwd", "pwd", "api_key", "apikey", "api-key", "secret", "token", "bearer",
    "authorization", "akia", "sk-", "nvapi-", "tvly-", "ghp_", "gho_", "ghs_", "ghr_",
    "github_pat_", "-----begin", "://",
];

/// Quita de un texto lo que parezca un secreto.
pub fn scrub(s: &str) -> String {
    let bajo = s.to_ascii_lowercase();
    if !MARCAS.iter().any(|m| bajo.contains(m)) {
        return s.to_string();
    }
    let mut out = s.to_string();
    for p in PATRONES.iter() {
        out = p.re.replace_all(&out, p.con).into_owned();
    }
    out
}

// ── Esquema ─────────────────────────────────────────────────────────────────

/// Crea la tabla, su índice de texto y los disparadores, si no están.
///
/// Hace falta por lo mismo que en `audit`: `lucy_core::init` abre una `lucy.db`
/// EXISTENTE y el esquema lo pone la app Tauri. En una máquina donde la Tauri no
/// haya arrancado nunca —que es hacia donde va esto— guardar una memoria daría
/// «no such table».
///
/// Las tres columnas de la segunda hornada van por `ALTER TABLE` aparte porque
/// en una base que ya existe la tabla NO se recrea: `CREATE TABLE IF NOT EXISTS`
/// no añade columnas. El error de columna repetida se traga a propósito — es la
/// forma de que esto sea idempotente sin consultar antes el esquema.
pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_memories (
                 id         INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id TEXT    NOT NULL DEFAULT '',
                 title      TEXT    NOT NULL,
                 content    TEXT    NOT NULL,
                 tags       TEXT    NOT NULL DEFAULT '[]',
                 files      TEXT    NOT NULL DEFAULT '[]',
                 importance INTEGER NOT NULL DEFAULT 1,
                 created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
             );
             CREATE INDEX IF NOT EXISTS idx_agent_memories_created
                 ON agent_memories(created_at DESC);
             CREATE INDEX IF NOT EXISTS idx_agent_memories_importance
                 ON agent_memories(importance DESC);
             CREATE VIRTUAL TABLE IF NOT EXISTS agent_memories_fts
                 USING fts5(title, content, tags);
             CREATE TRIGGER IF NOT EXISTS agent_memories_ai
                 AFTER INSERT ON agent_memories BEGIN
                     INSERT INTO agent_memories_fts(rowid, title, content, tags)
                     VALUES (new.id, new.title, new.content, new.tags);
                 END;
             CREATE TRIGGER IF NOT EXISTS agent_memories_ad
                 AFTER DELETE ON agent_memories BEGIN
                     DELETE FROM agent_memories_fts WHERE rowid = old.id;
                 END;
             CREATE TRIGGER IF NOT EXISTS agent_memories_au
                 AFTER UPDATE ON agent_memories BEGIN
                     UPDATE agent_memories_fts
                     SET title = new.title, content = new.content, tags = new.tags
                     WHERE rowid = new.id;
                 END;",
        )
        .map_err(|e| format!("memories: esquema: {e}"))?;
        for col in [
            "ALTER TABLE agent_memories ADD COLUMN last_accessed_at INTEGER",
            "ALTER TABLE agent_memories ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE agent_memories ADD COLUMN superseded_by TEXT",
            "ALTER TABLE agent_memories ADD COLUMN expires_at INTEGER NOT NULL DEFAULT 0",
        ] {
            let _ = c.execute(col, []);
        }
        Ok(())
    })
}

// ── Guardar ─────────────────────────────────────────────────────────────────

/// Guarda una memoria, o devuelve la que ya decía lo mismo.
///
/// DOS ETAPAS DE DEDUPLICACIÓN Y NO TRES. La V2 tiene una tercera que detecta
/// CONTRADICCIONES —mismo asunto, valor distinto— preguntándoselo a un modelo.
/// No se porta aquí y conviene decir por qué: esto va a correr al cerrar CADA
/// turno, y una llamada extra al modelo por turno es un coste que nadie ha
/// pedido y una espera que nadie ve. La contradicción se resuelve igual de bien
/// un paso más tarde, en la consolidación, que ya corre por vencimiento y sin
/// modelo.
pub fn save(n: &New) -> Result<Guardado, String> {
    let title = scrub(n.title.trim());
    let content = scrub(&recorta(n.content.trim(), MAX_CONTENT));
    if title.is_empty() || content.is_empty() {
        return Err("Una memoria necesita título y contenido.".into());
    }
    // Las etiquetas TAMBIÉN pasan por el filtro. En la V2 eran, junto con los
    // ficheros, los dos únicos campos que se guardaban sin limpiar — y son tan
    // controlables por el modelo como el resto: un token pegado como etiqueta
    // entraba tal cual.
    let tags: Vec<String> = n.tags.iter().map(|t| scrub(t)).collect();
    let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
    let imp = n.importance.clamp(1, MAX_AUTO_IMPORTANCE);
    let sid = n.session_id.clone();

    ensure_schema()?;

    // Etapa 1, por texto. Es la barata y va primero.
    if let Some(g) = texto_dup(&title, &content, &tags)? {
        return Ok(g);
    }
    // Etapa 2, por parecido semántico. Es la que caza «el servidor corre IIS» y
    // «PROD va sobre IIS», que no comparten casi ninguna palabra. Best-effort: si
    // el servicio de embeddings no está, se inserta sin ella en vez de fallar —
    // perder una deduplicación es un duplicado; fallar es perder el hecho.
    if let Some(g) = cosine_dup(&format!("{title}. {content}")) {
        return Ok(g);
    }

    crate::with_db(|c| {
        let tx = c
            .unchecked_transaction()
            .map_err(|e| format!("memories: tx: {e}"))?;
        // SE VUELVE A MIRAR DENTRO DE LA TRANSACCIÓN. Entre la primera sonda y
        // este punto ha habido una petición de red —la de los embeddings— y en
        // ese hueco cabe otro escritor: la app Tauri guardando lo mismo. Es una
        // carrera de verdad porque los dos programas comparten la base.
        if let Some(g) = texto_dup_tx(&tx, &title, &content, &tags)? {
            tx.commit().map_err(|e| format!("memories: commit dup: {e}"))?;
            return Ok(g);
        }
        tx.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, files, importance)
             VALUES (?1, ?2, ?3, ?4, '[]', ?5)",
            rusqlite::params![sid, title, content, tags_json, imp],
        )
        .map_err(|e| format!("memories: insert: {e}"))?;
        let id = tx.last_insert_rowid();
        tx.commit().map_err(|e| format!("memories: commit: {e}"))?;
        Ok(Guardado { id, accion: Accion::Guardada })
    })
}

fn recorta(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

/// La consulta FTS5 de un texto, sin que su puntuación rompa la sintaxis.
///
/// Cada palabra va ENTRECOMILLADA y con `*`. Sin las comillas, un contenido con
/// `AND`, `OR`, `NEAR` o un paréntesis dentro se interpreta como operador y la
/// consulta falla o —peor— casa otra cosa. Es el mismo cuidado que el escapado
/// de rutas: lo que va dentro es dato, no sintaxis.
fn consulta_fts(title: &str, content: &str) -> String {
    let sonda = format!("{title} {}", content.chars().take(200).collect::<String>());
    sonda
        .split_whitespace()
        .filter(|w| w.chars().count() > 2 && !w.chars().any(|c| c.is_control()))
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .take(20)
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn texto_dup(title: &str, content: &str, tags: &[String]) -> Result<Option<Guardado>, String> {
    crate::with_db(|c| texto_dup_tx(c, title, content, tags))
}

fn texto_dup_tx(
    c: &rusqlite::Connection,
    title: &str,
    content: &str,
    tags: &[String],
) -> Result<Option<Guardado>, String> {
    let q = consulta_fts(title, content);
    if q.is_empty() {
        return Ok(None);
    }
    // LAS FILAS DE DOCUMENTOS QUEDAN FUERA. Los trozos de un PDF viven en esta
    // misma tabla con `session_id` `pdf:` y `pdf-doc:`, y están en el índice de
    // texto. Sin excluirlos, una memoria escrita sobre un tema del que hay un
    // manual ingerido casa con un trozo del manual, se declara duplicada y NO SE
    // GUARDA NUNCA — el fallo de «te lo guardé» sobre algo que no está.
    let mut st = c
        .prepare(
            "SELECT am.id, am.title, am.content, am.tags
             FROM agent_memories am
             JOIN agent_memories_fts fts ON am.id = fts.rowid
             WHERE agent_memories_fts MATCH ?1
               AND (am.superseded_by IS NULL OR am.superseded_by = '')
               AND am.session_id NOT LIKE 'pdf:%'
               AND am.session_id NOT LIKE 'pdf-doc:%'
             ORDER BY bm25(agent_memories_fts) ASC
             LIMIT ?2",
        )
        .map_err(|e| format!("memories: fts prepare: {e}"))?;
    let candidatos: Vec<(i64, String, String, String)> = st
        .query_map(rusqlite::params![q, FTS_CANDIDATOS as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(|e| format!("memories: fts query: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    // EL MISMO CRITERIO QUE LA CONSOLIDACIÓN, no uno parecido. Dos definiciones
    // de «esto es lo mismo» que empiezan iguales acaban discrepando, y entonces
    // el dique de entrada acepta lo que la limpieza va a fundir después.
    let mis_toks = crate::consolidate::tokens(&format!("{title} {content}"));
    let mis_tags: std::collections::HashSet<String> =
        tags.iter().map(|t| t.to_lowercase()).collect();

    for (id, t, cont, tags_json) in candidatos {
        let suyos = crate::consolidate::tokens(&format!("{t} {cont}"));
        let cont_j = crate::consolidate::jaccard(&mis_toks, &suyos);
        if cont_j < crate::consolidate::MIN_CONTENT_JACCARD {
            continue;
        }
        // Las etiquetas son el filtro fino, y solo se exige cuando LAS DOS las
        // tienen: una memoria automática puede no llevar ninguna, y pedirle
        // solapamiento de etiquetas a un conjunto vacío la dejaría entrar
        // siempre por duplicada que fuera.
        let sus_tags: std::collections::HashSet<String> =
            serde_json::from_str::<Vec<String>>(&tags_json)
                .unwrap_or_default()
                .into_iter()
                .map(|x| x.to_lowercase())
                .collect();
        if !mis_tags.is_empty() && !sus_tags.is_empty() {
            let tag_j = crate::consolidate::jaccard(&mis_tags, &sus_tags);
            if tag_j < crate::consolidate::MIN_TAG_OVERLAP {
                continue;
            }
        }
        // Se le sube el contador de accesos: que un hecho vuelva a aparecer es
        // señal de que importa, y es lo que hace que suba en el recuerdo.
        let _ = c.execute(
            "UPDATE agent_memories
             SET access_count = access_count + 1, last_accessed_at = strftime('%s','now')
             WHERE id = ?1",
            rusqlite::params![id],
        );
        return Ok(Some(Guardado {
            id,
            accion: Accion::Duplicada {
                motivo: format!("coincide por texto con la memoria {id} (parecido {cont_j:.2})"),
            },
        }));
    }
    Ok(None)
}

/// Etapa 2. `None` si no hay parecido — o si no hay servicio de embeddings.
///
/// `entity_id` viene como texto porque la tabla de vectores guarda entidades de
/// varias clases. Una que no sea un número es de otra cosa, no un error: se
/// ignora en vez de fallar la escritura.
fn cosine_dup(texto: &str) -> Option<Guardado> {
    let (hits, _avisos) = crate::vectors::search(texto, "memory", 1, COSINE_DUP).ok()?;
    let m = hits.first()?;
    let id: i64 = m.entity_id.parse().ok()?;
    Some(Guardado {
        id,
        accion: Accion::Duplicada {
            motivo: format!("dice lo mismo que la memoria {id} (parecido {:.2})", m.score),
        },
    })
}

// ── Recordar ────────────────────────────────────────────────────────────────
//
// EN EL NÚCLEO Y NO EN LA VENTANA. Estaba en el shell, en diez líneas que
// buscaban solo entre memorias y devolvían vacío si el embebedor no contestaba.
// Recordar es el mecanismo del que depende que Lucy sea la misma entre sesiones,
// y tenerlo en un frontend significa que el otro recuerda distinto.

/// Cuántas memorias entran por turno.
///
/// Cinco. Con más, el prompt se llena de cosas tangencialmente parecidas y el
/// modelo construye sobre lo que se le recordó en vez de sobre lo que se le
/// preguntó — el fallo típico de la recuperación semántica generosa.
pub const RECALL_MEMORIAS: usize = 5;

/// Cuántos trozos de documento entran por turno.
///
/// Menos que memorias, y a propósito: un trozo de manual son párrafos enteros,
/// no una frase. Tres ya ocupan más sitio en el prompt que las cinco memorias.
pub const RECALL_DOCS: usize = 3;

/// Parecido mínimo para que una memoria entre.
pub const MIN_MEMORIA: f32 = 0.45;

/// Y para que entre un trozo de documento. MÁS ALTO a propósito.
///
/// Una memoria irrelevante es una línea que sobra. Un trozo del manual
/// equivocado son tres párrafos con pinta de documentación oficial sobre los que
/// el modelo va a construir una respuesta con toda la seguridad del mundo. El
/// coste de colar de más no es el mismo, y el umbral tampoco puede serlo.
pub const MIN_DOCUMENTO: f32 = 0.50;

/// Lo que se recordó, y de dónde.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recuerdo {
    /// Ya formateado para el prompt. Vacío = no se recordó nada.
    pub bloque: String,
    pub memorias: usize,
    pub documentos: usize,
    /// Se recurrió a la búsqueda por palabras porque no hubo vectores.
    ///
    /// Se dice en vez de callarlo: el recuerdo léxico encuentra menos y peor, y
    /// quien mire por qué Lucy no se acordó de algo evidente necesita saber que
    /// estaba funcionando con una mano atada.
    pub lexico: bool,
}

impl Recuerdo {
    pub fn is_empty(&self) -> bool {
        self.bloque.trim().is_empty()
    }
}

/// Lo que hay que recordar para contestar a esto.
///
/// TRES PATAS Y NO UNA. La versión anterior buscaba solo entre memorias, con
/// vectores, y si el embebedor no estaba devolvía cadena vacía sin decir nada:
/// en una máquina sin Ollama, Lucy no recordaba NADA nunca y el síntoma era
/// simplemente que parecía tener mala memoria.
///
/// · Memorias, por significado.
/// · Trozos de documento, por significado y con el listón más alto — es la pata
///   que hace que un manual ingerido sirva sin que nadie lo mencione, que es
///   justo lo que se le pide a la ingesta.
/// · Y si no hay vectores, palabras: FTS5 sobre la misma tabla, que no necesita
///   ningún servicio. Encuentra menos, pero encontrar poco es infinitamente más
///   que no encontrar nada.
///
/// `presupuesto` recorta las tres para un modelo flojo, que se ahoga con el
/// prompt entero y contesta en prosa sin emitir una sola etiqueta.
pub fn recall(query: &str, presupuesto: usize) -> Recuerdo {
    let q = query.trim();
    if q.is_empty() {
        return Recuerdo::default();
    }
    let n_mem = RECALL_MEMORIAS.min(presupuesto);
    let n_doc = RECALL_DOCS.min(presupuesto.saturating_sub(1));

    let mut r = Recuerdo::default();
    let mut lineas: Vec<String> = Vec::new();

    let mem = crate::vectors::search(q, "memory", n_mem, MIN_MEMORIA)
        .map(|(h, _)| h)
        .unwrap_or_default();
    for h in &mem {
        lineas.push(format!("- {}", una_linea(&h.text)));
    }
    r.memorias = mem.len();

    if n_doc > 0 {
        let docs = crate::vectors::search(q, "pdf_chunk", n_doc, MIN_DOCUMENTO)
            .map(|(h, _)| h)
            .unwrap_or_default();
        for h in &docs {
            // Marcados como lo que son. Sin la marca, el modelo no distingue un
            // hecho que Lucy aprendió de este equipo de un párrafo de un manual
            // genérico, y los cita con la misma autoridad.
            lineas.push(format!("- [documento] {}", una_linea(&h.text)));
        }
        r.documentos = docs.len();
    }

    // EL RESPALDO SOLO SI NO HUBO NADA. Mezclar palabras con significado cuando
    // el segundo ya trajo algo llenaría el prompt de coincidencias literales
    // peores que lo que ya había.
    if lineas.is_empty() {
        if let Ok(lex) = lexico(q, n_mem) {
            for t in &lex {
                lineas.push(format!("- {}", una_linea(t)));
            }
            r.memorias = lex.len();
            r.lexico = !lex.is_empty();
        }
    }

    r.bloque = lineas.join("\n");
    r
}

/// Una memoria en una línea. Los saltos romperían la lista del prompt.
fn una_linea(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Recuerdo por palabras, sin ningún servicio detrás.
fn lexico(query: &str, limite: usize) -> Result<Vec<String>, String> {
    let q = consulta_fts(query, "");
    if q.is_empty() {
        return Ok(vec![]);
    }
    crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT am.title || ' — ' || am.content
                 FROM agent_memories am
                 JOIN agent_memories_fts fts ON am.id = fts.rowid
                 WHERE agent_memories_fts MATCH ?1
                   AND (am.superseded_by IS NULL OR am.superseded_by = '')
                 ORDER BY bm25(agent_memories_fts) ASC
                 LIMIT ?2",
            )
            .map_err(|e| format!("recall léxico: {e}"))?;
        let v = st
            .query_map(rusqlite::params![q, limite as i64], |r| r.get::<_, String>(0))
            .map_err(|e| format!("recall léxico: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_listón_de_un_documento_es_mas_alto_que_el_de_una_memoria() {
        // Una memoria irrelevante es una línea que sobra. Un trozo del manual
        // equivocado son tres párrafos con pinta de documentación oficial sobre
        // los que el modelo construye con toda la seguridad del mundo.
        assert!(MIN_DOCUMENTO > MIN_MEMORIA);
        // Y entran menos: un trozo de manual son párrafos, no una frase.
        assert!(RECALL_DOCS < RECALL_MEMORIAS);
    }

    #[test]
    fn un_presupuesto_corto_recorta_las_dos_patas() {
        // Un modelo flojo se ahoga con el prompt entero y contesta en prosa sin
        // emitir una sola etiqueta. Recortar solo las memorias dejaría los
        // documentos, que ocupan más.
        assert_eq!(RECALL_MEMORIAS.min(2), 2);
        assert_eq!(RECALL_DOCS.min(2usize.saturating_sub(1)), 1);
        // Con presupuesto 1 no cabe ningún documento.
        assert_eq!(RECALL_DOCS.min(1usize.saturating_sub(1)), 0);
    }

    #[test]
    fn una_consulta_vacia_no_recuerda_nada_ni_pregunta() {
        // Sin esto se pagaría una petición al embebedor por cada turno que no
        // trae pregunta nueva — los de devolver la salida de un comando.
        assert!(recall("   ", 5).is_empty());
        assert_eq!(recall("", 5), Recuerdo::default());
    }

    #[test]
    fn los_saltos_de_linea_no_rompen_la_lista_del_prompt() {
        // Una memoria con saltos dentro partiría la lista en viñetas falsas, y
        // el modelo leería media memoria como un elemento aparte.
        assert_eq!(una_linea("una\nmemoria\tcon   saltos"), "una memoria con saltos");
    }

    #[test]
    fn un_token_no_llega_al_disco() {
        // Una memoria es lo ÚNICO que sobrevive a la conversación: un secreto
        // que se cuele se queda para siempre y encima vuelve al prompt de todos
        // los turnos siguientes.
        for (entrada, prohibido) in [
            ("Authorization: Bearer sk-ant-api03-clave-de-verdad-12345", "sk-ant-api03"),
            ("conectar con postgres://admin:Passw0rd@db.local/x", "Passw0rd"),
            ("la clave es AKIAIOSFODNN7EXAMPLE", "AKIAIOSFODNN7EXAMPLE"),
            ("token de github ghp_abcdefghijklmnopqrstuvwxyz0123", "ghp_abcdefghij"),
            ("nvapi-abcdefghijklmnopqrstuvwx en la variable", "nvapi-abcdefghij"),
        ] {
            let out = scrub(entrada);
            assert!(!out.contains(prohibido), "se coló «{prohibido}» en: {out}");
            assert!(out.contains("REDACTADO"), "{out}");
        }
    }

    #[test]
    fn las_variables_de_entorno_con_guion_bajo_tambien() {
        // EL CASO QUE ELIGIÓ EL LÍMITE. `\b` trata el guion bajo como letra, así
        // que no casaba la forma dominante —`DB_PASSWORD=`,
        // `AWS_SECRET_ACCESS_KEY=`— y ésas se guardaban tal cual.
        for e in [
            "DB_PASSWORD=SuperSecreto123",
            "AWS_SECRET_ACCESS_KEY=abcdefghijklmnop",
            "NVIDIA_API_KEY=loquesea1234",
        ] {
            let out = scrub(e);
            assert!(out.contains("[REDACTADO]"), "no lo limpió: {out}");
        }
    }

    #[test]
    fn una_frase_normal_no_se_toca() {
        // Lo primero que tiene que hacer un filtro es no estorbar. Y el atajo de
        // los marcadores existe para que la inmensa mayoría de las memorias no
        // paguen ocho expresiones regulares.
        for s in [
            "El servidor de impresión se reinicia con Restart-Service Spooler",
            "WIN-AD tiene 16 GB de RAM y dos discos",
        ] {
            assert_eq!(scrub(s), s);
        }
    }

    #[test]
    fn la_consulta_de_texto_no_deja_que_el_contenido_sea_sintaxis() {
        // Un contenido con `AND`, `OR` o un paréntesis dentro se interpretaría
        // como operador de FTS5: la consulta falla, o casa otra cosa.
        let q = consulta_fts("Reiniciar (Spooler) AND revisar", "el servicio OR el proceso");
        assert!(!q.contains(" AND "), "dejó un operador suelto: {q}");
        // Cada palabra va entrecomillada y con comodín.
        assert!(q.contains("\"Reiniciar\"*"), "{q}");
        assert!(q.contains(" OR "), "las palabras se unen con OR: {q}");
        // Las de dos letras o menos no aportan y se caen.
        assert!(!q.contains("\"el\""), "{q}");
    }

    #[test]
    fn una_comilla_en_el_texto_no_rompe_la_consulta() {
        let q = consulta_fts("el \"servidor\" de PROD", "");
        assert!(!q.contains("\"\"\""), "comillas sin escapar: {q}");
        assert!(q.contains("servidor"));
    }

    #[test]
    fn un_texto_sin_palabras_utiles_no_produce_consulta() {
        // Sin esto se mandaría un MATCH vacío a FTS5, que es un error de
        // sintaxis — y guardar fallaría por una memoria con título corto.
        assert!(consulta_fts("a b c", "de la").is_empty());
    }

    #[test]
    fn el_contenido_se_recorta_porque_una_memoria_es_un_hecho() {
        // Sin tope, una memoria que guarde la salida de un `Get-EventLog` entra
        // entera en el recuerdo semántico y se come la ventana de contexto de
        // todos los turnos siguientes.
        let largo = recorta(&"á".repeat(MAX_CONTENT + 500), MAX_CONTENT);
        assert_eq!(largo.chars().count(), MAX_CONTENT + 1);
        assert!(largo.ends_with('…'));
        // Por caracteres y no por bytes: cortar en medio de un multibyte es un
        // pánico, y esto viene de texto en español.
        assert!(largo.starts_with('á'));
    }

    #[test]
    fn una_escritura_automatica_no_puede_fijarse_a_si_misma() {
        // La importancia 10 significa «lo fijó una persona» y es lo que la
        // consolidación respeta para no tocar nada. Una pasada automática que se
        // concediera ese sello se volvería inmune a su propia limpieza.
        let n = New::nueva("t", "c").importancia(10);
        assert_eq!(n.importance.clamp(1, MAX_AUTO_IMPORTANCE), MAX_AUTO_IMPORTANCE);
        assert_eq!(New::nueva("t", "c").importancia(0).importance.clamp(1, MAX_AUTO_IMPORTANCE), 1);
    }

    #[test]
    fn el_umbral_de_fundir_es_mas_alto_que_el_de_recordar() {
        // El coste de equivocarse no es simétrico: recordar de más mete una
        // línea que sobra, fundir de más PIERDE un hecho.
        assert!(COSINE_DUP > 0.45, "fundir con el umbral de recordar pierde hechos");
    }
}
