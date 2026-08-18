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
///
/// CADA ALTERNATIVA DE LOS PATRONES NECESITA SU MARCADOR AQUÍ, o el atajo la
/// anula: el patrón clave=valor cubría `access_key` desde el principio, pero sin
/// un marcador `access` el atajo devolvía el texto intacto antes de que ningún
/// patrón corriera — `AZURE_STORAGE_ACCESS_KEY=…` se guardaba tal cual, con la
/// redacción escrita a un centímetro.
const MARCAS: &[&str] = &[
    "password", "passwd", "pwd", "api_key", "apikey", "api-key", "access_key", "access-key",
    "accesskey", "secret", "token", "bearer", "authorization", "akia", "sk-", "nvapi-", "tvly-",
    "ghp_", "gho_", "ghs_", "ghr_", "github_pat_", "-----begin", "://",
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
            // INTEGER, que es como la declara la app (`metrics.rs:282`) y como
            // está en la base real. Aquí ponía TEXT, y aunque el desajuste
            // resulta ser benigno —SQLite convierte el «57» que escribe la
            // consolidación a 57 por la afinidad de la columna, y el filtro
            // `= ''` sigue excluyendo la fila; medido sobre una copia de la base
            // real— dos declaraciones distintas de la misma columna significan
            // que qué tipo tiene depende de qué programa creó la base primero.
            // Eso convierte cualquier lectura del valor en una ruleta.
            "ALTER TABLE agent_memories ADD COLUMN superseded_by INTEGER NULL",
            "ALTER TABLE agent_memories ADD COLUMN expires_at INTEGER NOT NULL DEFAULT 0",
            // La chincheta. Existe en la base real porque la añade la app; se
            // declara aquí también para que una instalación que arranque por el
            // shell nativo tenga la misma tabla que una que arranque por la app.
            "ALTER TABLE agent_memories ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
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
    // LIMPIAR ANTES DE CORTAR, y el orden es una regla de seguridad, no de
    // estilo. El patrón de claves privadas necesita encontrar el `-----END`: si
    // el corte a 4 000 caracteres cae dentro del bloque, el marcador de cierre
    // desaparece, el patrón no casa, y el cuerpo de la clave se guarda tal cual.
    // Al revés, primero se redacta sobre el texto entero y luego se corta lo que
    // ya está limpio.
    let content = recorta(&scrub(n.content.trim()), MAX_CONTENT);
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

    let g = crate::with_db(|c| {
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
    })?;

    // Y SU VECTOR, o la fila nace medio invisible. Sin esto, nada de lo que
    // escribe este programa entraba jamás en la búsqueda por significado — ni en
    // su propia deduplicación por coseno, que compara contra vectores que nadie
    // escribía. El recuerdo parecía funcionar porque el respaldo léxico tapaba el
    // agujero encontrando menos.
    //
    // FUERA DE `with_db`, y eso no es una preferencia: embeber es una petición
    // HTTP con treinta segundos de plazo, y dentro del closure se haría con una
    // conexión del pool en la mano — cuatro guardados concurrentes con Ollama
    // lento agotarían el pool entero y pararían hasta las lecturas.
    //
    // Best-effort: sin Ollama la fila queda solo léxica, que es exactamente lo
    // que el respaldo sabe manejar. `ensure_schema` de vectores por si esta base
    // aún no tiene la tabla — este camino puede ser el primero en necesitarla.
    if g.es_nueva() {
        if let Ok((v, m)) = crate::vectors::embed_blocking(&format!("{title}. {content}")) {
            let _ = crate::vectors::ensure_schema();
            let _ = crate::vectors::upsert(
                "memory",
                &[(g.id.to_string(), format!("{title} — {content}"), v)],
                &m,
            );
        }
    }
    Ok(g)
}

/// Importancia de una memoria FIJADA a mano.
///
/// Diez, que es la convención de la app y la que ya respeta el consolidador: por
/// encima de este número no se funde nada. Es el número que distingue «esto lo
/// decidió una persona» de todo lo que Lucy se apuntó sola, y por eso ninguna
/// pasada automática puede concedérselo — la escritura de turno topa en 3 y la
/// consolidación en 9.
pub const FIJADA: i64 = 10;

/// Fija o suelta una memoria.
///
/// FIJAR ES DOS COSAS A LA VEZ, y las dos hacen falta. La columna `pinned` es lo
/// que la app enseña con su chincheta; la importancia en [`FIJADA`] es lo que la
/// pone por delante en el recuerdo y lo que impide que el consolidador la funda
/// con otra. Escribir solo una de las dos daría una chincheta decorativa —se ve
/// fijada y se comporta como cualquiera— o lo contrario, una memoria intocable
/// que nada explica.
///
/// Al soltarla vuelve a 3, que es el techo de lo automático: no se puede
/// recuperar la importancia que tenía antes —nadie la guardó— y dejarla en 10
/// sin la marca sería justo la memoria intocable sin explicación.
pub fn set_pinned(id: i64, fijada: bool) -> Result<(), String> {
    ensure_schema()?;
    crate::with_db(|c| {
        c.execute(
            "UPDATE agent_memories SET pinned = ?1, importance = ?2 WHERE id = ?3",
            rusqlite::params![
                i64::from(fijada),
                if fijada { FIJADA } else { MAX_AUTO_IMPORTANCE },
                id
            ],
        )
        .map_err(|e| format!("memories: fijar: {e}"))?;
        Ok(())
    })
}

/// Cambia la importancia de una memoria, sin tocar la chincheta.
///
/// El tope es [`FIJADA`] menos uno: llegar a diez es fijarla, y eso tiene su
/// propia función porque escribe además la columna. Un deslizador que pudiera
/// poner diez dejaría memorias con la importancia de fijada y sin la marca.
pub fn set_importance(id: i64, importancia: i64) -> Result<(), String> {
    ensure_schema()?;
    let n = importancia.clamp(1, FIJADA - 1);
    crate::with_db(|c| {
        c.execute(
            "UPDATE agent_memories SET importance = ?1 WHERE id = ?2",
            rusqlite::params![n, id],
        )
        .map_err(|e| format!("memories: importancia: {e}"))?;
        Ok(())
    })
}

/// Borra una memoria — y su vector, que es la mitad que se olvidaba en todas
/// partes: una fila borrada cuyo vector queda sigue saliendo en la búsqueda por
/// significado, citando algo que ya no existe.
pub fn delete(id: i64) -> Result<(), String> {
    crate::with_db(|c| {
        c.execute("DELETE FROM agent_memories WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("memories: borrar: {e}"))?;
        let _ = c.execute(
            "DELETE FROM embeddings WHERE entity_type = 'memory' AND entity_id = ?1",
            rusqlite::params![id.to_string()],
        );
        Ok(())
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

/// ¿La memoria sigue contando? Viva = no supersedida y no caducada.
///
/// EXISTE PORQUE LOS VECTORES NO LO SABEN. La tabla `embeddings` no tiene
/// columna de retirada y las búsquedas semánticas la leen sin join: cualquier
/// resultado que venga de ahí hay que contrastarlo contra la tabla de memorias
/// antes de creérselo. La app Tauri retira filas escribiendo solo la columna, y
/// sus vectores se quedan.
pub fn viva(id: i64) -> bool {
    crate::with_db(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM agent_memories
             WHERE id = ?1
               AND (superseded_by IS NULL OR superseded_by = '')
               AND (expires_at IS NULL OR expires_at = 0 OR expires_at > strftime('%s','now'))",
            rusqlite::params![id],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())
    })
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// Etapa 2. `None` si no hay parecido — o si no hay servicio de embeddings.
///
/// `entity_id` viene como texto porque la tabla de vectores guarda entidades de
/// varias clases. Una que no sea un número es de otra cosa, no un error: se
/// ignora en vez de fallar la escritura.
fn cosine_dup(texto: &str) -> Option<Guardado> {
    // Tres candidatos y no uno: si el más parecido resulta estar retirado, el
    // segundo puede seguir siendo un duplicado de verdad.
    let (hits, _avisos) = crate::vectors::search(texto, "memory", 3, COSINE_DUP).ok()?;
    for m in &hits {
        let Ok(id) = m.entity_id.parse::<i64>() else { continue };
        // CONTRA UNA FILA RETIRADA NO HAY DUPLICADO. Sin esta comprobación, el
        // vector huérfano de una memoria supersedida bastaba para responder
        // «ya lo sabía» y descartar el hecho NUEVO — en favor de una fila que
        // ningún lector enseña. El hecho se perdía de la vista para siempre.
        if !viva(id) {
            continue;
        }
        return Some(Guardado {
            id,
            accion: Accion::Duplicada {
                motivo: format!("dice lo mismo que la memoria {id} (parecido {:.2})", m.score),
            },
        });
    }
    None
}

// ── Escribir sola ───────────────────────────────────────────────────────────
//
// LO QUE HACE QUE NO HAYA QUE PEDÍRSELO. Hasta aquí, una memoria solo existía si
// el modelo emitía una etiqueta o el operador escribía un comando — o sea, si
// alguien se acordaba. Una memoria que depende de que alguien se acuerde es una
// memoria que no se escribe: el día que hace falta acordarse es justo el día en
// que se está resolviendo un incidente y nadie está pensando en documentar.
//
// SE ENGANCHA AL CIERRE DEL TURNO y no a que al modelo se le ocurra. El cierre es
// un evento del programa, ocurre siempre, y en ese momento se sabe todo lo que
// hace falta para decidir: qué se preguntó, qué corrió y cómo acabó.

/// Qué pasó en un turno. Lo que hace falta para decidir si merece una fila.
#[derive(Debug, Clone, Default)]
pub struct Turno<'a> {
    /// Lo que escribió el operador.
    pub pregunta: &'a str,
    /// Lo que contestó Lucy, ya sin etiquetas.
    pub respuesta: &'a str,
    /// Los comandos que se ejecutaron de verdad, con si fueron bien.
    pub comandos: &'a [(String, bool)],
    /// Cuántas herramientas de lectura se cumplieron.
    pub herramientas: usize,
    /// El turno terminó con error del proveedor.
    pub fallo: bool,
}

/// Largo mínimo de la respuesta para que el turno valga.
///
/// Doscientos caracteres. Por debajo es un «hecho», un «sí» o un «ese servicio
/// está parado» — cierto, pero nada que valga la pena recordar dentro de seis
/// meses.
pub const MIN_RESPUESTA: usize = 200;

/// ¿Este turno merece quedarse?
///
/// PURA, para que la regla se pueda discutir sin base de datos. Y con el listón
/// donde está por una razón concreta: lo que merece una fila es un DESENLACE, no
/// una charla. Un turno que solo habló no tiene nada que recordar dentro de seis
/// meses; uno que midió algo en la máquina, sí — y es exactamente lo que se
/// querría tener a mano la próxima vez que pase.
pub fn merece(t: &Turno) -> bool {
    // Un turno que falló no concluyó nada. Guardar su respuesta a medias sería
    // guardar una hipótesis con aspecto de hallazgo.
    if t.fallo {
        return false;
    }
    if t.pregunta.trim().is_empty() || t.respuesta.trim().len() < MIN_RESPUESTA {
        return false;
    }
    // TOCÓ LA MÁQUINA. Es la línea entera: sin esto se guardaría una fila por
    // cada «¿qué tal?», y en un mes la memoria sería un registro de
    // conversaciones en vez de un registro de hechos.
    !t.comandos.is_empty() || t.herramientas > 0
}

/// La memoria que sale de un turno.
///
/// SIN LLAMAR AL MODELO, y es deliberado: esto corre al cerrar CADA turno, y una
/// petición extra por turno es un coste que nadie ha pedido y una espera que
/// nadie ve. Es el mismo razonamiento por el que no se portó la tercera etapa de
/// deduplicación.
///
/// Lo que se guarda es lo que ya se sabe sin preguntarle a nadie: qué se
/// preguntó, qué se ejecutó y a qué se llegó. Un resumen redactado sería más
/// bonito y costaría una llamada por turno.
pub fn from_turn(t: &Turno) -> New {
    let titulo: String = t
        .pregunta
        .trim()
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(90)
        .collect();
    let mut cuerpo = String::new();
    if !t.comandos.is_empty() {
        cuerpo.push_str("Se ejecutó:\n");
        for (c, ok) in t.comandos.iter().take(6) {
            cuerpo.push_str(&format!("- {c}{}\n", if *ok { "" } else { "  (con error)" }));
        }
        cuerpo.push('\n');
    }
    cuerpo.push_str(t.respuesta.trim());
    New {
        title: if titulo.is_empty() { "Consulta".into() } else { titulo },
        content: cuerpo,
        // Etiquetada como automática para poder distinguirla de lo que dictó el
        // operador. Sin la marca, dentro de un mes no hay forma de saber qué
        // decidió una persona y qué se quedó solo.
        tags: vec!["auto".into()],
        session_id: String::new(),
        // Dos y no uno: es un desenlace medido en la máquina, que vale más que
        // una nota suelta. Y no tres, que se reserva para lo que alguien marcó.
        importance: 2,
    }
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
///
/// MEDIDO, NO ELEGIDO — y lo que se midió cambió el número por completo. Contra
/// el embebedor de verdad (`nomic-embed-text`, textos en español), tres memorias
/// de administración de sistemas y cuatro preguntas:
///
/// ```text
/// por qué no imprime la impresora     -> 0.689, 0.564, 0.547
/// cuándo caduca el certificado        -> 0.766, 0.601, 0.539
/// qué se hace con el gazpacho andaluz -> 0.591, 0.573, 0.547   ← nada que ver
/// receta de tortilla de patatas       -> 0.542, 0.538, 0.509   ← nada que ver
/// ```
///
/// El SUELO de dos textos en español que no tienen nada que ver está en 0,59. El
/// umbral que había en el shell era 0,40 — o sea que en CADA turno entraban cinco
/// memorias tomadas prácticamente al azar. Justo el fallo contra el que avisaba
/// el comentario que defendía ese 0,40: «el modelo empieza a construir sobre lo
/// que se le recordó en vez de sobre lo que se le preguntó».
///
/// 0,65 deja pasar el acierto de cabeza —0,69 y 0,77 en las medidas— y corta todo
/// lo demás. Los números son de ESTE embebedor y de textos en español; con otro
/// modelo hay que volver a medirlos, no arrastrarlos.
pub const MIN_MEMORIA: f32 = 0.65;

/// Y para que entre un trozo de documento. MÁS ALTO a propósito.
///
/// Una memoria irrelevante es una línea que sobra. Un trozo del manual
/// equivocado son tres párrafos con pinta de documentación oficial sobre los que
/// el modelo va a construir una respuesta con toda la seguridad del mundo. El
/// coste de colar de más no es el mismo, y el umbral tampoco puede serlo.
///
/// Medido igual, sobre un manual real troceado:
///
/// ```text
/// rotación de claves PGP            -> 0.827, 0.656, 0.623
/// instalar el agente en el servidor -> 0.711, 0.669, 0.668
/// receta de tortilla de patatas     -> 0.561, 0.556, 0.554   ← nada que ver
/// cómo se hace el gazpacho          -> 0.551, 0.550, 0.532   ← nada que ver
/// ```
pub const MIN_DOCUMENTO: f32 = 0.70;

/// Lo que se recordó, y de dónde.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recuerdo {
    /// Ya formateado para el prompt. Vacío = no se recordó nada.
    pub bloque: String,
    pub memorias: usize,
    pub documentos: usize,
    /// Cuántas entraron por estar FIJADAS, no por parecerse a la pregunta.
    ///
    /// Aparte de `memorias` porque llegaron por otro camino, y quien mire por
    /// qué el prompt trae lo que trae necesita distinguirlos: cuatro fijadas y
    /// cero semánticas significa que la búsqueda no encontró nada, no que Lucy
    /// recordara bien.
    pub fijadas: usize,
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

    // ── LAS FIJADAS, ANTES QUE NADA Y SIN PREGUNTARLE AL PARECIDO ────────────
    //
    // Es lo que significa fijar una memoria, y sin esto la chincheta sería
    // decorativa: entrarían solo cuando se parecieran a la pregunta, o sea
    // cuando ya no hacía falta acordarse de ellas. Es el mismo razonamiento que
    // separa un principio de una memoria, aplicado a las memorias que el
    // operador señaló a mano.
    //
    // Y NO GASTAN EL PRESUPUESTO de las recordadas por significado: si lo
    // gastaran, fijar tres memorias dejaría el recuerdo semántico sin sitio y
    // Lucy dejaría de traer lo que viene al caso. Tienen su propio tope pequeño
    // — fijar veinte memorias es no fijar ninguna.
    if let Ok(fijadas) = pinned(MAX_FIJADAS_EN_PROMPT) {
        for t in &fijadas {
            lineas.push(format!("- [fijada] {}", una_linea(t)));
        }
        r.fijadas = fijadas.len();
    }

    // LA CONSULTA SE EMBEBE UNA VEZ, no una por pata. Las dos patas semánticas
    // usan el mismo vector; pedirlo dos veces era pagar dos viajes a Ollama por
    // turno para recibir dos veces el mismo resultado — en el camino crítico de
    // cada pregunta. Y las filas van primero: con el corpus vacío no se paga
    // ningún viaje, que es la misma regla que ya tenía `vectors::search`.
    let mem_rows = crate::vectors::load_stored("memory").unwrap_or_default();
    let doc_rows = if n_doc > 0 {
        crate::vectors::load_stored("pdf_chunk").unwrap_or_default()
    } else {
        Vec::new()
    };
    if !mem_rows.is_empty() || !doc_rows.is_empty() {
        if let Ok((qvec, modelo)) = crate::vectors::embed_blocking(q) {
            let (mem, _) =
                crate::vectors::rank_by_cosine(mem_rows, &qvec, &modelo, MIN_MEMORIA, n_mem);
            for h in &mem {
                // CONTRASTADO CONTRA LA TABLA DE MEMORIAS. Los vectores no saben
                // de retiradas ni caducidades: sin esto, una memoria supersedida
                // por la consolidación —o por la app Tauri, que no borra
                // vectores— seguía entrando al prompt con su redacción vieja
                // para siempre.
                let Ok(id) = h.entity_id.parse::<i64>() else { continue };
                if !viva(id) {
                    continue;
                }
                lineas.push(format!("- {}", una_linea(&h.text)));
                r.memorias += 1;
            }

            let (docs, _) =
                crate::vectors::rank_by_cosine(doc_rows, &qvec, &modelo, MIN_DOCUMENTO, n_doc);
            for h in &docs {
                // Marcados como lo que son. Sin la marca, el modelo no distingue
                // un hecho que Lucy aprendió de este equipo de un párrafo de un
                // manual genérico, y los cita con la misma autoridad.
                lineas.push(format!("- [documento] {}", una_linea(&h.text)));
            }
            r.documentos = docs.len();
        }
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

/// Cuántas memorias fijadas entran en el prompt.
///
/// Cuatro. El tope es pequeño a propósito y por lo mismo que el de los
/// principios: si entraran todas, fijar veinte memorias sería no fijar ninguna —
/// el modelo las promedia y acaba siguiendo las que suenan más fuerte. Y como no
/// gastan el presupuesto de las semánticas, un número grande aquí desplazaría el
/// prompt entero hacia lo que el operador marcó hace meses.
pub const MAX_FIJADAS_EN_PROMPT: usize = 4;

/// Las memorias fijadas, de más importante a más reciente.
///
/// Sin filtro de parecido: entran porque alguien las señaló, no porque se
/// parezcan a la pregunta.
pub fn pinned(limite: usize) -> Result<Vec<String>, String> {
    ensure_schema()?;
    crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT title || ' — ' || content FROM agent_memories
                 WHERE pinned = 1
                   AND (superseded_by IS NULL OR superseded_by = '')
                   AND session_id NOT LIKE 'pdf:%'
                 ORDER BY importance DESC, created_at DESC LIMIT ?1",
            )
            .map_err(|e| format!("memories: fijadas: {e}"))?;
        let v = st
            .query_map([limite as i64], |r| r.get::<_, String>(0))
            .map_err(|e| format!("memories: fijadas: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })
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
                // Sin trozos de documento. Este respaldo corre justo cuando
                // Ollama no está —que es cuando no hay ranking semántico que los
                // mantenga a raya— y tras ingerir un manual, sus cuatrocientos
                // párrafos ganan cualquier búsqueda por palabras: el recuerdo
                // entero serían páginas de manual disfrazadas de memorias.
                "SELECT am.title || ' — ' || am.content
                 FROM agent_memories am
                 JOIN agent_memories_fts fts ON am.id = fts.rowid
                 WHERE agent_memories_fts MATCH ?1
                   AND (am.superseded_by IS NULL OR am.superseded_by = '')
                   AND am.session_id NOT LIKE 'pdf:%'
                   AND am.session_id NOT LIKE 'pdf-doc:%'
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
// Las aserciones de este modulo comparan CONSTANTES entre si. Clippy las ve
// evaluables en compilacion y avisa; no son aserciones muertas sino guardas de
// invariante: fijan una relacion de diseno para que cambiar un numero rompa el
// test en vez de cambiar el comportamiento en silencio.
#[allow(clippy::assertions_on_constants)]
mod tests {
    use super::*;

    fn turno<'a>(pregunta: &'a str, respuesta: &'a str, cmds: &'a [(String, bool)]) -> Turno<'a> {
        Turno { pregunta, respuesta, comandos: cmds, herramientas: 0, fallo: false }
    }

    #[test]
    fn solo_se_guarda_lo_que_toco_la_maquina() {
        // ES LA LÍNEA ENTERA. Sin esto se guardaría una fila por cada «¿qué
        // tal?», y en un mes la memoria sería un registro de conversaciones en
        // vez de un registro de hechos.
        let larga = "x".repeat(MIN_RESPUESTA + 50);
        let cmd = vec![("Get-Service Spooler".to_string(), true)];

        assert!(merece(&turno("¿por qué no imprime?", &larga, &cmd)));
        assert!(!merece(&turno("¿qué tal?", &larga, &[])), "guardó una charla");
        // Una herramienta de lectura también cuenta: leer un log es medir.
        let mut t = turno("¿qué dice el log?", &larga, &[]);
        t.herramientas = 1;
        assert!(merece(&t));
    }

    #[test]
    fn un_turno_que_fallo_no_concluyo_nada() {
        // Guardar su respuesta a medias sería guardar una hipótesis con aspecto
        // de hallazgo — y con el aval de haber quedado escrita.
        let larga = "x".repeat(MIN_RESPUESTA + 50);
        let cmd = vec![("Get-Service".to_string(), true)];
        let mut t = turno("algo", &larga, &cmd);
        t.fallo = true;
        assert!(!merece(&t));
    }

    #[test]
    fn una_respuesta_de_dos_palabras_no_es_un_hallazgo() {
        // «Sí», «hecho», «ese servicio está parado»: cierto, y nada que valga la
        // pena recordar dentro de seis meses.
        let cmd = vec![("Get-Service".to_string(), true)];
        assert!(!merece(&turno("¿está parado?", "Sí, está parado.", &cmd)));
    }

    #[test]
    fn la_memoria_automatica_lleva_lo_que_corrio_y_se_marca_como_tal() {
        let cmds = vec![
            ("Restart-Service Spooler".to_string(), true),
            ("Get-Service Spooler".to_string(), false),
        ];
        let n = from_turn(&turno(
            "¿por qué no imprime la impresora del segundo piso?",
            "El servicio Spooler estaba detenido. Se reinició y la cola volvió a fluir.",
            &cmds,
        ));
        assert!(n.content.contains("Restart-Service Spooler"));
        // Y si un comando falló, se dice: media verdad sobre lo que se ejecutó es
        // peor que no tener la fila.
        assert!(n.content.contains("(con error)"), "{}", n.content);
        assert!(n.content.contains("volvió a fluir"));
        // MARCADA COMO AUTOMÁTICA. Sin eso, dentro de un mes no hay forma de
        // saber qué decidió una persona y qué se quedó solo.
        assert!(n.tags.contains(&"auto".to_string()));
        assert_eq!(n.importance, 2);
        // El título es la pregunta, recortada y en una línea.
        assert!(n.title.starts_with("¿por qué no imprime"));
        assert!(!n.title.contains('\n'));
    }

    #[test]
    fn un_titulo_larguisimo_no_desborda_la_lista() {
        let n = from_turn(&turno(&"a".repeat(400), "b", &[]));
        assert!(n.title.chars().count() <= 90);
    }

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
    fn cada_alternativa_del_patron_kv_tiene_su_marcador() {
        // EL ATAJO PUEDE ANULAR AL PATRÓN, y lo hacía: el patrón clave=valor
        // cubría `access_key` desde el principio, pero MARCAS no tenía ningún
        // marcador que casara con «azure_storage_access_key=», así que scrub
        // devolvía el texto intacto sin correr ni una expresión. La redacción
        // estaba escrita a un centímetro y no se ejecutaba nunca.
        for e in [
            "AZURE_STORAGE_ACCESS_KEY=Zm9vYmFyYmF6cXV4",
            "ACCESS_KEY=hunter2hunter2",
            "access-key: abcd1234efgh",
        ] {
            let out = scrub(e);
            assert!(out.contains("[REDACTADO]"), "el atajo se lo tragó: {out}");
        }
    }

    #[test]
    fn una_clave_privada_cortada_no_sobrevive_por_perder_su_cierre() {
        // El patrón PEM necesita el -----END. Si se corta ANTES de limpiar, un
        // bloque que cruce el tope de 4 000 caracteres pierde el cierre, el
        // patrón no casa, y el cuerpo de la clave se guarda tal cual. El orden
        // limpiar→cortar es una regla de seguridad, no de estilo.
        let clave = format!(
            "-----BEGIN RSA PRIVATE KEY-----\n{}\n-----END RSA PRIVATE KEY-----",
            "MIIEowIBAAKCAQEA7".repeat(220) // ~3 700 caracteres de cuerpo
        );
        let n = New {
            title: "La clave del servidor".into(),
            // El bloque empieza pasado el carácter 800: el corte a 4 000 cae
            // DENTRO del cuerpo y se lleva el -----END por delante.
            content: format!("{}\n{clave}", "El binding TLS falla por esto. ".repeat(30)),
            tags: vec![],
            session_id: String::new(),
            importance: 1,
        };
        // Sin base de datos: se comprueba la transformación, no la fila. El
        // orden vive en `save`, así que se reproduce aquí tal cual lo hace save.
        let limpio = recorta(&scrub(n.content.trim()), MAX_CONTENT);
        assert!(!limpio.contains("MIIEowIBAAKCAQEA7"), "el cuerpo de la clave sobrevivió al corte");
        assert!(limpio.contains("[REDACTADO_CLAVE_PRIVADA]"));
        // Y el orden contrario —el que había— deja pasar la clave, que es
        // exactamente por qué este test existe.
        let mal = scrub(&recorta(n.content.trim(), MAX_CONTENT));
        assert!(mal.contains("MIIEowIBAAKCAQEA7"), "si esto falla, el orden viejo ya no es peligroso y el test miente");
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
