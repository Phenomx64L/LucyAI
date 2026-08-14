//! Documentos ingeridos: de un PDF a algo que Lucy consulta sola.
//!
//! ES LA PIEZA DE LA QUE SALE GRAN PARTE DE LA MEMORIA, y por eso lo que importa
//! aquí no es la vista sino el último eslabón: que `pdf_search` esté en el
//! catálogo de herramientas y que los trozos entren en el recuerdo previo al
//! turno. Sin esas dos cosas, ingerir un manual de trescientas páginas no cambia
//! NADA de lo que Lucy sabe — y no da error, da silencio, que es la forma de
//! fallar más cara de encontrar.
//!
//! LOS TROZOS VIVEN EN `agent_memories`, no en una tabla propia. Es la decisión
//! de la V2 y es la correcta: así el índice de texto, la búsqueda semántica y la
//! consolidación funcionan sobre ellos sin escribir nada nuevo. Se distinguen por
//! el `session_id` —`pdf:{id}` los trozos, `pdf-doc:{id}` el resumen— y todas las
//! consultas de memoria los excluyen por ese prefijo, que es lo que impide que
//! una memoria del operador se deduplique contra un párrafo de un manual.
//!
//! Y EL RESUMEN ES UNA FILA APARTE, con más importancia que los trozos. Es la que
//! contesta «¿qué documentos tienes?» sin traerse cuatrocientos párrafos, y la que
//! hace que Lucy sepa que el manual existe aunque ningún trozo suyo venga al caso.

/// Cuánto texto lleva un trozo.
///
/// Dos mil quinientos caracteres. Es el tamaño de la V2 y responde a dos cosas a
/// la vez: cabe en el contexto sin comérselo, y es lo bastante grande para que un
/// procedimiento de siete pasos no se parta en dos trozos que se buscan por
/// separado y se encuentran sueltos.
pub const CHUNK: usize = 2_500;

/// Cuánto se solapan dos trozos consecutivos.
///
/// EXISTE PORQUE UN CORTE CAE SIEMPRE EN MEDIO DE ALGO. Sin solape, la frase que
/// queda a caballo entre dos trozos no está entera en ninguno de los dos, y es
/// justo la que se buscaba: los cortes no saben dónde acaban las ideas.
pub const SOLAPE: usize = 200;

/// Tope de trozos por documento.
///
/// Cuatrocientos, que a 2 500 caracteres son un millón — un manual muy gordo. El
/// tope no es por el disco sino por el embebedor: pasado eso, ingerir se
/// convierte en una espera de minutos que nadie va a dejar terminar.
pub const MAX_TROZOS: usize = 400;

#[derive(Debug, Clone, PartialEq)]
pub struct Documento {
    pub id: i64,
    pub nombre: String,
    pub ruta: String,
    pub trozos: usize,
    /// Vectores calculados. Menos que `trozos` = la ingesta se quedó a medias.
    pub vectorizados: usize,
    pub sha: String,
    pub creado: i64,
}

impl Documento {
    /// Si el documento contesta a búsquedas. Un PDF ingerido SIN vectores existe
    /// en la lista y no aparece en ningún recuerdo — el estado más confuso
    /// posible, porque parece que está.
    pub fn utilizable(&self) -> bool {
        self.vectorizados > 0
    }
}

/// Trocea un texto respetando dónde acaban las cosas.
///
/// POR PÁRRAFOS Y NO POR CARACTERES A SECAS. Cortar en el carácter 2 500 parte
/// una frase por la mitad, y ese trozo se embebe con media idea: no casa con la
/// pregunta que lo buscaba ni con ninguna otra. Se acumulan párrafos hasta llenar
/// el trozo y se corta ENTRE ellos.
///
/// Un párrafo más largo que un trozo entero —una tabla, un volcado de
/// configuración— sí se parte a lo bruto, porque la alternativa sería un trozo de
/// cincuenta mil caracteres que no cabe en ninguna ventana.
pub fn chunk(texto: &str) -> Vec<String> {
    let texto = texto.trim();
    if texto.is_empty() {
        return vec![];
    }
    let mut out: Vec<String> = Vec::new();
    let mut actual = String::new();

    for parrafo in texto.split("\n\n") {
        let p = parrafo.trim();
        if p.is_empty() {
            continue;
        }
        // Un párrafo que no cabe ni él solo se parte por caracteres.
        if p.chars().count() > CHUNK {
            if !actual.trim().is_empty() {
                out.push(std::mem::take(&mut actual));
            }
            let cs: Vec<char> = p.chars().collect();
            let mut i = 0;
            while i < cs.len() {
                let fin = (i + CHUNK).min(cs.len());
                out.push(cs[i..fin].iter().collect());
                if fin == cs.len() {
                    break;
                }
                i = fin.saturating_sub(SOLAPE);
            }
            continue;
        }
        if actual.chars().count() + p.chars().count() + 2 > CHUNK {
            // EL SOLAPE SE LLEVA LA COLA del trozo que se cierra. Sin él, la
            // frase que queda a caballo no está entera en ninguno de los dos.
            let cola: String = {
                let cs: Vec<char> = actual.chars().collect();
                cs[cs.len().saturating_sub(SOLAPE)..].iter().collect()
            };
            out.push(std::mem::take(&mut actual));
            actual = cola;
        }
        if !actual.is_empty() {
            actual.push_str("\n\n");
        }
        actual.push_str(p);
    }
    if !actual.trim().is_empty() {
        out.push(actual);
    }
    out.truncate(MAX_TROZOS);
    out
}

/// Huella del contenido, para no ingerir dos veces lo mismo.
///
/// Del TEXTO EXTRAÍDO y no del fichero: el mismo manual guardado dos veces con
/// nombres distintos tiene dos sumas de fichero y un solo contenido, y lo que
/// duplicaría el corpus es el contenido.
///
/// FNV-1a de 64 bits en hexadecimal. No hace falta resistencia criptográfica —
/// nadie va a fabricar una colisión para colar un documento— y sí no arrastrar un
/// crate de hashes por una comprobación de igualdad.
pub fn huella(texto: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in texto.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Crea la tabla de documentos si no está.
pub fn ensure_schema() -> Result<(), String> {
    crate::memories::ensure_schema()?;
    crate::vectors::ensure_schema()?;
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS pdf_documents (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 filename    TEXT NOT NULL,
                 path        TEXT NOT NULL DEFAULT '',
                 sha         TEXT NOT NULL DEFAULT '',
                 chunks      INTEGER NOT NULL DEFAULT 0,
                 embedded    INTEGER NOT NULL DEFAULT 0,
                 created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
             );
             CREATE INDEX IF NOT EXISTS idx_pdf_documents_sha ON pdf_documents(sha);",
        )
        .map_err(|e| format!("docs: esquema: {e}"))
    })
}

/// Extensiones que ya son texto y no hay que extraer de nada.
///
/// NO SOLO PDF, y es un hueco de la V2 que costaba tres líneas cerrar. Un
/// administrador tiene sus procedimientos en Markdown y en texto plano mucho más
/// a menudo que en PDF —los `README` de los repositorios, las notas de un
/// despliegue, un `.conf` comentado— y obligar a convertirlos a PDF para que Lucy
/// pueda leerlos es pedirle al operador que trabaje para la herramienta.
pub const TEXTO_PLANO: &[&str] = &[
    "txt", "md", "markdown", "log", "conf", "cfg", "ini", "yaml", "yml", "json", "csv", "ps1",
    "sh", "sql", "xml",
];

/// El texto de un fichero, venga de donde venga.
fn extrae(ruta: &std::path::Path) -> Result<String, String> {
    let ext = ruta
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if TEXTO_PLANO.contains(&ext.as_str()) {
        let bytes = std::fs::read(ruta)
            .map_err(|e| format!("no se pudo leer: {e}"))?;
        // UTF-8 y, si no, la consola: un `.log` o un `.ps1` escritos por una
        // herramienta nativa de Windows vienen en la página de códigos del
        // sistema, y leerlos como UTF-8 los llena de rombos justo donde iban las
        // tildes.
        return Ok(match std::str::from_utf8(&bytes) {
            Ok(s) => s.to_string(),
            Err(_) => crate::shell::decode_console(&bytes),
        });
    }
    crate::pdf::extract_text(ruta)
}

/// Qué está pasando durante una ingesta, para poder enseñarlo.
#[derive(Debug, Clone, PartialEq)]
pub enum Paso {
    Extrayendo,
    Troceando(usize),
    /// Vectores calculados de un total.
    Embebiendo(usize, usize),
    Listo(Documento),
    /// Se ingirió el texto pero no se pudo vectorizar. El documento EXISTE y no
    /// contesta a nada — hay que decirlo, no dejarlo en la lista como si nada.
    SinVectores(Documento, String),
    Error(String),
}

/// Ingiere un fichero. Va informando por el canal.
///
/// EN UN HILO, SIEMPRE. La extracción de un PDF de trescientas páginas son
/// segundos y la vectorización son minutos; llamar a esto desde el hilo de la
/// interfaz congela la ventana entera. En la V2 el extractor de reserva ni
/// siquiera va en `spawn_blocking`.
pub fn ingest(
    ruta: &std::path::Path,
    tx: &std::sync::mpsc::Sender<Paso>,
    stop: &std::sync::atomic::AtomicBool,
) {
    use std::sync::atomic::Ordering;
    let _ = tx.send(Paso::Extrayendo);
    if let Err(e) = ensure_schema() {
        let _ = tx.send(Paso::Error(e));
        return;
    }
    let nombre = ruta
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "documento".into());

    let texto = match extrae(ruta) {
        Ok(t) => t,
        Err(e) => {
            // Un PDF escaneado no tiene capa de texto y esto falla limpio. El
            // mensaje del extractor explica la causa y se conserva tal cual: es
            // la diferencia entre «este PDF son imágenes, hace falta OCR» y un
            // «no se pudo» que manda a mirar permisos.
            let _ = tx.send(Paso::Error(format!("No se pudo leer «{nombre}»: {e}")));
            return;
        }
    };
    if texto.trim().is_empty() {
        let _ = tx.send(Paso::Error(format!(
            "«{nombre}» no tiene texto que extraer. Si es un PDF escaneado, son imágenes \
             y haría falta OCR."
        )));
        return;
    }

    let sha = huella(&texto);
    // MISMO CONTENIDO, NO SE REINGIERE. Sin esto, arrastrar dos veces el mismo
    // manual duplica cuatrocientos trozos que después compiten entre sí en cada
    // búsqueda y se llevan el sitio de todo lo demás.
    match ya_esta(&sha) {
        Ok(Some(d)) => {
            let _ = tx.send(Paso::Listo(d));
            return;
        }
        Err(e) => {
            let _ = tx.send(Paso::Error(e));
            return;
        }
        Ok(None) => {}
    }

    // EL FILTRO DE SECRETOS PASA POR AQUÍ TAMBIÉN, y sobre el texto ENTERO,
    // antes de trocear. Los formatos que más se ingieren en este oficio —.conf,
    // .ini, .yml, .ps1, .env volcado a un .txt— son exactamente donde viven las
    // credenciales, y estas filas viajan después al prompt en cada recuerdo. Se
    // limpia antes de trocear y no trozo a trozo porque una clave privada puede
    // caer partida entre dos trozos: ninguno tendría el bloque completo y el
    // patrón no casaría en ninguno. La huella se calcula ANTES, sobre el texto
    // tal cual está en disco, para que reingerir el mismo fichero se detecte.
    let texto = crate::memories::scrub(&texto);
    let trozos = chunk(&texto);
    if trozos.is_empty() {
        let _ = tx.send(Paso::Error(format!("«{nombre}» no dio ningún trozo.")));
        return;
    }
    let _ = tx.send(Paso::Troceando(trozos.len()));

    let doc = match alta(&nombre, &ruta.to_string_lossy(), &sha, trozos.len()) {
        Ok(d) => d,
        Err(e) => {
            let _ = tx.send(Paso::Error(e));
            return;
        }
    };

    // Las filas de texto primero. Aunque el embebedor no esté, el documento
    // queda buscable POR PALABRAS —el índice FTS es un disparador de la tabla—
    // que es infinitamente más que no quedar de ninguna forma.
    let ids = match filas(&doc, &trozos, &texto) {
        Ok(ids) => ids,
        Err(e) => {
            let _ = tx.send(Paso::Error(e));
            return;
        }
    };

    let mut hechos = 0usize;
    for (i, lote) in trozos.chunks(crate::vectors::EMBED_LOTE).enumerate() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let textos: Vec<String> = lote.to_vec();
        match crate::vectors::embed_batch(&textos) {
            Ok((vs, modelo)) => {
                let base = i * crate::vectors::EMBED_LOTE;
                // EL ID DEL VECTOR ES EL ID DE LA FILA, no «doc#parte» — porque
                // la app Tauri usa el id de fila y las dos comparten base. Con
                // dos esquemas, el borrado de cada aplicación limpiaba solo los
                // vectores que ella misma escribió: borrar desde la otra dejaba
                // los huérfanos rankeando en cada búsqueda, citando un documento
                // que ya no existe.
                let filas: Vec<(String, String, Vec<f32>)> = vs
                    .into_iter()
                    .enumerate()
                    .map(|(k, v)| (ids[base + k].to_string(), textos[k].clone(), v))
                    .collect();
                match crate::vectors::upsert("pdf_chunk", &filas, &modelo) {
                    Ok(n) => hechos += n,
                    Err(e) => {
                        let _ = tx.send(Paso::SinVectores(doc.clone(), e));
                        return;
                    }
                }
                let _ = tx.send(Paso::Embebiendo(hechos, trozos.len()));
            }
            Err(e) => {
                let _ = marca_embebidos(doc.id, hechos);
                let mut d = doc.clone();
                d.vectorizados = hechos;
                let _ = tx.send(Paso::SinVectores(d, e));
                return;
            }
        }
    }
    let _ = marca_embebidos(doc.id, hechos);
    let mut d = doc;
    d.vectorizados = hechos;
    let _ = tx.send(Paso::Listo(d));
}

fn ya_esta(sha: &str) -> Result<Option<Documento>, String> {
    crate::with_db(|c| {
        let r = c
            .query_row(
                "SELECT id, filename, path, chunks, embedded, sha, created_at
                 FROM pdf_documents WHERE sha = ?1 LIMIT 1",
                rusqlite::params![sha],
                fila_doc,
            )
            .ok();
        Ok(r)
    })
}

fn fila_doc(r: &rusqlite::Row) -> rusqlite::Result<Documento> {
    Ok(Documento {
        id: r.get(0)?,
        nombre: r.get(1)?,
        ruta: r.get(2)?,
        trozos: r.get::<_, i64>(3)? as usize,
        vectorizados: r.get::<_, i64>(4)? as usize,
        sha: r.get(5)?,
        creado: r.get(6)?,
    })
}

fn alta(nombre: &str, ruta: &str, sha: &str, trozos: usize) -> Result<Documento, String> {
    crate::with_db(|c| {
        c.execute(
            "INSERT INTO pdf_documents (filename, path, sha, chunks) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![nombre, ruta, sha, trozos as i64],
        )
        .map_err(|e| format!("docs: alta: {e}"))?;
        let id = c.last_insert_rowid();
        c.query_row(
            "SELECT id, filename, path, chunks, embedded, sha, created_at
             FROM pdf_documents WHERE id = ?1",
            rusqlite::params![id],
            fila_doc,
        )
        .map_err(|e| format!("docs: releer: {e}"))
    })
}

fn marca_embebidos(id: i64, n: usize) -> Result<(), String> {
    crate::with_db(|c| {
        c.execute(
            "UPDATE pdf_documents SET embedded = ?1 WHERE id = ?2",
            rusqlite::params![n as i64, id],
        )
        .map_err(|e| format!("docs: marcar: {e}"))?;
        Ok(())
    })
}

/// Las filas de memoria del documento: una por trozo, más la del resumen.
///
/// Devuelve los ids de las filas de TROZO, en el orden de los trozos: son la
/// clave con la que se guardan sus vectores.
fn filas(d: &Documento, trozos: &[String], texto: &str) -> Result<Vec<i64>, String> {
    crate::with_db(|c| {
        let tx = c.unchecked_transaction().map_err(|e| format!("docs tx: {e}"))?;
        let mut ids = Vec::with_capacity(trozos.len());
        for (i, t) in trozos.iter().enumerate() {
            tx.execute(
                "INSERT INTO agent_memories (session_id, title, content, tags, files, importance)
                 VALUES (?1, ?2, ?3, '[\"documento\"]', '[]', 1)",
                rusqlite::params![
                    format!("pdf:{}", d.id),
                    format!("{} — parte {}/{}", d.nombre, i + 1, trozos.len()),
                    t
                ],
            )
            .map_err(|e| format!("docs: trozo: {e}"))?;
            ids.push(tx.last_insert_rowid());
        }
        // EL RESUMEN, con más importancia. Es la fila que contesta «¿qué
        // documentos tienes?» sin traerse cuatrocientos párrafos, y la que hace
        // que Lucy sepa que el manual existe aunque ningún trozo suyo venga al
        // caso de esta pregunta.
        let inicio: String = texto.chars().take(600).collect();
        tx.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, files, importance)
             VALUES (?1, ?2, ?3, '[\"documento\"]', '[]', 2)",
            rusqlite::params![
                format!("pdf-doc:{}", d.id),
                format!("Documento ingerido: {}", d.nombre),
                format!(
                    "«{}» está ingerido y se puede consultar con pdf_search. {} trozos. \
                     Empieza así: {inicio}",
                    d.nombre,
                    trozos.len()
                )
            ],
        )
        .map_err(|e| format!("docs: resumen: {e}"))?;
        tx.commit().map_err(|e| format!("docs commit: {e}"))?;
        Ok(ids)
    })
}

/// Los documentos ingeridos, del más reciente al más viejo.
pub fn list() -> Result<Vec<Documento>, String> {
    ensure_schema()?;
    crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT id, filename, path, chunks, embedded, sha, created_at
                 FROM pdf_documents ORDER BY created_at DESC, id DESC",
            )
            .map_err(|e| format!("docs list: {e}"))?;
        let v = st
            .query_map([], fila_doc)
            .map_err(|e| format!("docs list: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })
}

/// Borra un documento, sus trozos y sus vectores.
pub fn delete(id: i64) -> Result<(), String> {
    crate::with_db(|c| {
        let tx = c.unchecked_transaction().map_err(|e| format!("docs tx: {e}"))?;
        // Los tres sitios, o quedan huérfanos: trozos sin documento que siguen
        // saliendo en las búsquedas, y vectores apuntando a filas que no están.
        //
        // LOS VECTORES PRIMERO, porque su clave es el id de fila y se resuelve
        // con una subconsulta sobre las filas — que en la orden siguiente dejan
        // de existir. Es además la misma consulta que usa la app Tauri para lo
        // mismo, así que cada aplicación limpia también lo que ingirió la otra.
        tx.execute(
            "DELETE FROM embeddings
              WHERE entity_type = 'pdf_chunk'
                AND entity_id IN (SELECT CAST(id AS TEXT) FROM agent_memories
                                   WHERE session_id = ?1 OR session_id = ?2)",
            rusqlite::params![format!("pdf:{id}"), format!("pdf-doc:{id}")],
        )
        .map_err(|e| format!("docs: borrar vectores: {e}"))?;
        // Y el esquema viejo «doc#parte», por las filas que este mismo programa
        // escribió antes de alinear la clave con la de la app.
        tx.execute(
            "DELETE FROM embeddings WHERE entity_type = 'pdf_chunk' AND entity_id LIKE ?1",
            rusqlite::params![format!("{id}#%")],
        )
        .map_err(|e| format!("docs: borrar vectores viejos: {e}"))?;
        tx.execute(
            "DELETE FROM agent_memories WHERE session_id = ?1 OR session_id = ?2",
            rusqlite::params![format!("pdf:{id}"), format!("pdf-doc:{id}")],
        )
        .map_err(|e| format!("docs: borrar trozos: {e}"))?;
        tx.execute("DELETE FROM pdf_documents WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("docs: borrar documento: {e}"))?;
        tx.commit().map_err(|e| format!("docs commit: {e}"))?;
        Ok(())
    })
}

/// Cuántos trozos devuelve una búsqueda en documentos.
pub const SEARCH_LIMIT: usize = 5;

/// Parecido mínimo cuando la búsqueda la pide el modelo por su nombre.
///
/// Más bajo que el del recuerdo automático (0,70) y por un motivo concreto: aquí
/// el modelo ha PREGUNTADO, así que un acierto flojo sigue siendo información que
/// él puede descartar; en el recuerdo se le mete sin que nadie lo mire.
///
/// Pero no por debajo del suelo del ruido. Medido contra el embebedor real, dos
/// textos en español sin nada que ver puntúan 0,55; con el 0,35 que tenía esto,
/// preguntar por una receta de tortilla devolvía tres párrafos de un manual de
/// transferencia de ficheros — y el modelo los habría usado para contestar.
pub const SEARCH_MIN: f32 = 0.60;

/// Busca en lo ingerido. Es lo que hay detrás de la herramienta `pdf_search`.
///
/// Por significado y, si no hay vectores, por palabras — la misma escalera que el
/// recuerdo. Un manual ingerido sin embebedor sigue contestando a una búsqueda
/// por términos, que es como se busca en un manual de todos modos.
pub fn search(query: &str) -> Result<Vec<(String, String)>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("Falta qué buscar.".into());
    }
    ensure_schema()?;
    if let Ok((hits, _)) = crate::vectors::search(q, "pdf_chunk", SEARCH_LIMIT, SEARCH_MIN) {
        if !hits.is_empty() {
            return Ok(hits
                .into_iter()
                .map(|h| (titulo_de(&h.entity_id), h.text))
                .collect());
        }
    }
    lexico(q)
}

/// Del id del vector al título de su fila.
///
/// La clave nueva es el id de fila en `agent_memories`, cuyo título ya dice
/// «manual.pdf — parte 3/40». La forma vieja `12#7` se sigue entendiendo por las
/// filas escritas antes de alinear la clave con la de la app.
fn titulo_de(entity_id: &str) -> String {
    if let Some((doc, parte)) = entity_id.split_once('#') {
        let nombre = doc
            .parse::<i64>()
            .ok()
            .and_then(|id| {
                crate::with_db(|c| {
                    Ok(c.query_row(
                        "SELECT filename FROM pdf_documents WHERE id = ?1",
                        rusqlite::params![id],
                        |r| r.get::<_, String>(0),
                    )
                    .ok())
                })
                .ok()
                .flatten()
            })
            .unwrap_or_else(|| "documento".into());
        return format!(
            "{nombre} · parte {}",
            parte.parse::<usize>().map(|n| n + 1).unwrap_or(0)
        );
    }
    entity_id
        .parse::<i64>()
        .ok()
        .and_then(|id| {
            crate::with_db(|c| {
                Ok(c.query_row(
                    "SELECT title FROM agent_memories WHERE id = ?1",
                    rusqlite::params![id],
                    |r| r.get::<_, String>(0),
                )
                .ok())
            })
            .ok()
            .flatten()
        })
        .unwrap_or_else(|| "documento".into())
}

fn lexico(q: &str) -> Result<Vec<(String, String)>, String> {
    let terminos: String = q
        .split_whitespace()
        .filter(|w| w.chars().count() > 2)
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .take(20)
        .collect::<Vec<_>>()
        .join(" OR ");
    if terminos.is_empty() {
        return Ok(vec![]);
    }
    crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT am.title, am.content
                 FROM agent_memories am
                 JOIN agent_memories_fts fts ON am.id = fts.rowid
                 WHERE agent_memories_fts MATCH ?1
                   AND am.session_id LIKE 'pdf:%'
                 ORDER BY bm25(agent_memories_fts) ASC
                 LIMIT ?2",
            )
            .map_err(|e| format!("docs búsqueda: {e}"))?;
        let v = st
            .query_map(rusqlite::params![terminos, SEARCH_LIMIT as i64], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(|e| format!("docs búsqueda: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_corte_cae_entre_parrafos_y_no_en_medio_de_una_frase() {
        // Cortar en el carácter 2 500 parte una frase por la mitad, y ese trozo
        // se embebe con media idea: no casa con la pregunta que lo buscaba ni con
        // ninguna otra.
        let p = "Este párrafo tiene una idea completa y termina donde debe.";
        let texto = vec![p; 120].join("\n\n");
        let ts = chunk(&texto);
        assert!(ts.len() > 1, "no troceó nada");
        for t in &ts {
            assert!(t.chars().count() <= CHUNK + SOLAPE + 4, "trozo de {}", t.chars().count());
            // TERMINAN donde termina un párrafo. El principio no: ahí está la
            // cola de solape, que empieza a media frase por definición — para eso
            // existe, la frase está entera en el trozo anterior.
            assert!(t.trim_end().ends_with("debe."), "cortó a media frase: …{}", &t[t.len().saturating_sub(40)..]);
        }
        // Y no se pierde nada por el camino: cada párrafo del original aparece
        // en algún trozo. Un troceador que se coma contenido no da error, da
        // respuestas que no citan la parte que importaba.
        assert!(ts.iter().any(|t| t.contains(p)));
    }

    #[test]
    fn los_trozos_se_solapan_para_no_perder_la_frase_del_corte() {
        // Sin solape, la frase a caballo entre dos trozos no está entera en
        // ninguno de los dos — y es justo la que se buscaba.
        let texto = (0..80)
            .map(|i| format!("Párrafo número {i} con texto suficiente para llenar el trozo poco a poco."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let ts = chunk(&texto);
        assert!(ts.len() >= 2);
        // La cola del primero aparece al principio del segundo.
        let cola: String = ts[0].chars().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect();
        assert!(ts[1].contains(cola.trim()), "no hay solape:\n1: …{cola}\n2: {}", &ts[1][..80.min(ts[1].len())]);
    }

    #[test]
    fn un_parrafo_gigante_se_parte_en_vez_de_no_caber() {
        // Una tabla o un volcado de configuración pueden ser más largos que un
        // trozo entero. La alternativa a partirlo es un trozo de cincuenta mil
        // caracteres que no cabe en ninguna ventana de contexto.
        let gigante = "x".repeat(CHUNK * 3);
        let ts = chunk(&gigante);
        assert!(ts.len() >= 3, "{}", ts.len());
        assert!(ts.iter().all(|t| t.chars().count() <= CHUNK));
    }

    #[test]
    fn un_texto_vacio_no_produce_trozos() {
        assert!(chunk("").is_empty());
        assert!(chunk("   \n\n  \n ").is_empty());
    }

    #[test]
    fn el_tope_de_trozos_se_respeta() {
        // No es por el disco: pasado eso, ingerir se convierte en una espera de
        // minutos que nadie va a dejar terminar.
        let texto = (0..MAX_TROZOS * 3)
            .map(|i| format!("{} {}", "palabra ".repeat(400), i))
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(chunk(&texto).len(), MAX_TROZOS);
    }

    #[test]
    fn la_huella_es_del_contenido_y_no_del_nombre() {
        // El mismo manual guardado dos veces con nombres distintos tiene dos
        // sumas de fichero y un solo contenido, y lo que duplicaría el corpus es
        // el contenido.
        assert_eq!(huella("mismo texto"), huella("mismo texto"));
        assert_ne!(huella("un texto"), huella("otro texto"));
        // Estable entre ejecuciones: si cambiara, cada arranque reingeriría todo.
        assert_eq!(huella("Lucy"), huella("Lucy"));
        assert_eq!(huella("").len(), 16);
    }

    #[test]
    fn un_documento_sin_vectores_no_se_da_por_utilizable() {
        // Es el estado más confuso posible: existe en la lista y no contesta a
        // nada, así que parece que está.
        let d = Documento {
            id: 1,
            nombre: "manual.pdf".into(),
            ruta: String::new(),
            trozos: 40,
            vectorizados: 0,
            sha: String::new(),
            creado: 0,
        };
        assert!(!d.utilizable());
        assert!(Documento { vectorizados: 40, ..d }.utilizable());
    }
}
