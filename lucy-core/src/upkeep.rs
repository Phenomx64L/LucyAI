//! Cuidados de la base: copiarla, contar qué hay dentro y quitar lo que sobra.
//!
//! TODA LA MEMORIA DE LUCY VIVE EN UN FICHERO. Los cristales, los patrones, las
//! memorias automáticas de cada turno, los documentos ingeridos — todo está en
//! `lucy.db`, y hasta ahora la vista enseñaba su ruta y nada más. Enseñar dónde
//! está algo irreemplazable sin ofrecer copiarlo es dar media instrucción.
//!
//! Y AL REVÉS TAMBIÉN: cuatrocientas de las cuatrocientas cuatro filas de esta
//! instalación son trozos de un manual. Un recuento por tipo es lo que convierte
//! «la base ocupa 7 MB» en «casi todo es un PDF que ingeriste en abril», que es
//! una frase sobre la que se puede decidir.

/// Copia la base a un fichero, de forma que la copia sea CONSISTENTE.
///
/// Con la API de copia de seguridad de SQLite y no copiando el fichero a mano.
/// La diferencia importa: la aplicación tiene la base abierta —y la app Tauri
/// puede tenerla abierta a la vez— así que un `copy` del fichero puede llevarse
/// un estado a medio escribir, con el diario aparte. Esta forma coordina con el
/// motor y produce una base que abre.
pub fn backup(destino: &std::path::Path) -> Result<u64, String> {
    if let Some(padre) = destino.parent() {
        std::fs::create_dir_all(padre)
            .map_err(|e| format!("no se pudo crear la carpeta destino: {e}"))?;
    }
    crate::with_db(|c| {
        // `VACUUM INTO` es la copia consistente de SQLite, y además compacta:
        // una base de la que se han borrado documentos conserva su tamaño en
        // páginas libres hasta que alguien la compacta.
        c.execute("VACUUM INTO ?1", rusqlite::params![destino.to_string_lossy()])
            .map_err(|e| format!("no se pudo copiar la base: {e}"))?;
        Ok(())
    })?;
    std::fs::metadata(destino)
        .map(|m| m.len())
        .map_err(|e| format!("la copia no se pudo leer: {e}"))
}

/// Cuántas filas hay de cada cosa, y cuánto ocupa la base.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recuento {
    /// Memorias de verdad: ni trozos de documento ni retiradas.
    pub memorias: usize,
    /// Escritas solas al cerrar un turno.
    pub automaticas: usize,
    /// Promovidas desde un cristal.
    pub de_cristal: usize,
    /// Fijadas a mano.
    pub fijadas: usize,
    /// Trozos de documento ingerido.
    pub trozos: usize,
    /// Retiradas por la consolidación. Siguen ocupando.
    pub retiradas: usize,
    pub cristales: usize,
    pub patrones: usize,
    pub documentos: usize,
    pub vectores: usize,
    /// Lo que ocupa el fichero, en bytes.
    pub bytes: u64,
}

fn cuenta(c: &rusqlite::Connection, sql: &str) -> usize {
    c.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
}

/// El recuento. Una consulta por línea, todas sobre índices o tablas pequeñas.
pub fn recuento(ruta: &std::path::Path) -> Recuento {
    let bytes = std::fs::metadata(ruta).map(|m| m.len()).unwrap_or(0);
    crate::with_db(|c| {
        Ok(Recuento {
            memorias: cuenta(
                c,
                "SELECT COUNT(*) FROM agent_memories
                 WHERE (superseded_by IS NULL OR superseded_by = '')
                   AND session_id NOT LIKE 'pdf:%' AND session_id NOT LIKE 'pdf-doc:%'",
            ),
            automaticas: cuenta(
                c,
                "SELECT COUNT(*) FROM agent_memories
                 WHERE tags LIKE '%\"auto\"%'
                   AND (superseded_by IS NULL OR superseded_by = '')",
            ),
            de_cristal: cuenta(
                c,
                "SELECT COUNT(*) FROM agent_memories
                 WHERE tags LIKE '%\"crystal\"%'
                   AND (superseded_by IS NULL OR superseded_by = '')",
            ),
            fijadas: cuenta(c, "SELECT COUNT(*) FROM agent_memories WHERE pinned = 1"),
            trozos: cuenta(
                c,
                "SELECT COUNT(*) FROM agent_memories WHERE session_id LIKE 'pdf:%'",
            ),
            retiradas: cuenta(
                c,
                "SELECT COUNT(*) FROM agent_memories
                 WHERE superseded_by IS NOT NULL AND superseded_by != ''",
            ),
            cristales: cuenta(c, "SELECT COUNT(*) FROM agent_crystals"),
            patrones: cuenta(c, "SELECT COUNT(*) FROM agent_insights"),
            documentos: cuenta(c, "SELECT COUNT(*) FROM pdf_documents"),
            vectores: cuenta(c, "SELECT COUNT(*) FROM embeddings"),
            bytes,
        })
    })
    .unwrap_or(Recuento { bytes, ..Default::default() })
}

/// Qué se puede quitar en lote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purga {
    /// Las filas que la consolidación retiró. Ya no las lee nadie.
    ///
    /// LO ÚNICO QUE SE BORRA SIN PREGUNTAR DOS VECES, porque es lo único que ya
    /// estaba fuera de uso: una memoria retirada apunta a la que la sustituyó y
    /// ninguna consulta viva la devuelve. Lo que se pierde es el rastro de por
    /// qué la canónica dice lo que dice.
    Retiradas,
    /// Las memorias que Lucy se apuntó sola.
    ///
    /// Esto SÍ es material: son los desenlaces de cada turno. Se ofrece porque
    /// es lo que más crece y lo que menos se revisa, no porque sobre.
    Automaticas,
    /// Todo lo de un documento: trozos, resumen, vectores y su fila.
    Documentos,
}

impl Purga {
    /// Qué se lleva por delante, dicho antes de hacerlo.
    pub fn describe(self, r: &Recuento) -> String {
        match self {
            Purga::Retiradas => format!(
                "{} memorias retiradas por la consolidación. Ninguna consulta viva las \
                 devuelve ya; se pierde el rastro de por qué la que las sustituyó dice lo \
                 que dice.",
                r.retiradas
            ),
            Purga::Automaticas => format!(
                "{} memorias que Lucy se apuntó sola al cerrar turnos. Son desenlaces \
                 medidos en esta máquina — lo que más crece y lo que menos se revisa.",
                r.automaticas
            ),
            Purga::Documentos => format!(
                "{} documentos con sus {} trozos y sus vectores. Lucy dejará de poder \
                 consultarlos.",
                r.documentos, r.trozos
            ),
        }
    }
}

/// Ejecuta una purga. Devuelve cuántas filas se fueron.
///
/// LOS VECTORES SE VAN CON SUS FILAS, siempre. Es el mismo error que ya costó una
/// sesión entera en este proyecto: una fila borrada cuyo vector queda sigue
/// saliendo en la búsqueda por significado, citando algo que no existe.
pub fn purga(que: Purga) -> Result<usize, String> {
    crate::with_db(|c| {
        let tx = c
            .unchecked_transaction()
            .map_err(|e| format!("purga: tx: {e}"))?;
        let n = match que {
            Purga::Retiradas => {
                tx.execute(
                    "DELETE FROM embeddings WHERE entity_type = 'memory' AND entity_id IN (
                         SELECT CAST(id AS TEXT) FROM agent_memories
                         WHERE superseded_by IS NOT NULL AND superseded_by != '')",
                    [],
                )
                .map_err(|e| format!("purga: vectores: {e}"))?;
                tx.execute(
                    "DELETE FROM agent_memories
                     WHERE superseded_by IS NOT NULL AND superseded_by != ''",
                    [],
                )
                .map_err(|e| format!("purga: retiradas: {e}"))?
            }
            Purga::Automaticas => {
                tx.execute(
                    "DELETE FROM embeddings WHERE entity_type = 'memory' AND entity_id IN (
                         SELECT CAST(id AS TEXT) FROM agent_memories
                         WHERE tags LIKE '%\"auto\"%' AND pinned = 0)",
                    [],
                )
                .map_err(|e| format!("purga: vectores: {e}"))?;
                // LAS FIJADAS SE RESPETAN aunque sean automáticas: fijar una es
                // decir «ésta me la quedo», y una purga por lote que se la
                // llevara convertiría la chincheta en una promesa incumplida.
                tx.execute(
                    "DELETE FROM agent_memories WHERE tags LIKE '%\"auto\"%' AND pinned = 0",
                    [],
                )
                .map_err(|e| format!("purga: automáticas: {e}"))?
            }
            Purga::Documentos => {
                tx.execute(
                    "DELETE FROM embeddings WHERE entity_type = 'pdf_chunk'",
                    [],
                )
                .map_err(|e| format!("purga: vectores: {e}"))?;
                let n = tx
                    .execute(
                        "DELETE FROM agent_memories
                         WHERE session_id LIKE 'pdf:%' OR session_id LIKE 'pdf-doc:%'",
                        [],
                    )
                    .map_err(|e| format!("purga: trozos: {e}"))?;
                tx.execute("DELETE FROM pdf_documents", [])
                    .map_err(|e| format!("purga: documentos: {e}"))?;
                n
            }
        };
        tx.commit().map_err(|e| format!("purga: commit: {e}"))?;
        Ok(n)
    })
}

/// Las dos clases de fila que llevan vector, con el criterio de cada una.
///
/// PARAMETRIZADO Y NO COPIADO. Los trozos de PDF y las memorias viven en la
/// MISMA tabla `agent_memories` y se distinguen por el `session_id`; escribir el
/// criterio dos veces es como se llega a que una fila cuente como pendiente en
/// el contador y no en el arreglo, o al revés — y el síntoma sería un botón que
/// dice «6 sin vector» y al pulsarlo no hace nada.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clase {
    /// Un trozo de documento ingerido. `session_id` como `pdf:<algo>`.
    Trozo,
    /// Una memoria del agente. Todo lo demás.
    Memoria,
}

impl Clase {
    /// El `entity_type` con el que se guarda en `embeddings`.
    ///
    /// Son los que ya hay escritos en la base desde la V1 — `search` y
    /// `load_stored` buscan por ellos. Cambiar uno no daría error: daría cero
    /// resultados sobre una tabla llena.
    pub fn etiqueta(self) -> &'static str {
        match self {
            Clase::Trozo => "pdf_chunk",
            Clase::Memoria => "memory",
        }
    }

    /// El `WHERE` que separa una clase de la otra.
    ///
    /// LAS DOS MITADES DEL CRITERIO, y la segunda faltaba.
    ///
    /// Una ingesta escribe DOS clases de fila: `pdf:{id}` por cada trozo, y UNA
    /// `pdf-doc:{id}` con la ficha del documento. Este filtro solo excluía la
    /// primera, así que las cuatro fichas de esta base entraban como memorias y
    /// el botón ofrecía embeberlas — con `entity_type = 'memory'`, o sea
    /// compitiendo en el recuerdo semántico con lo que Lucy aprendió de verdad.
    ///
    /// Y estaba EN DESACUERDO CON `recuento`, ciento ochenta líneas más arriba
    /// en este mismo fichero, que sí lleva las dos. El panel decía «97 memorias»
    /// y el botón contaba sobre 101. Dos criterios sobre la misma tabla es como
    /// se llega a que una fila cuente en un sitio y no en el de al lado.
    ///
    /// `session_id IS NULL` cuenta como memoria: `NOT LIKE` sobre un nulo da
    /// NULL, no verdadero, así que sin esto las filas anteriores a que existiera
    /// la columna se caen del filtro y no las arregla nadie. Son justo las más
    /// viejas, las que más falta hace embeber.
    fn filtro(self) -> &'static str {
        match self {
            Clase::Trozo => "am.session_id LIKE 'pdf:%'",
            Clase::Memoria => {
                "(am.session_id IS NULL
                  OR (am.session_id NOT LIKE 'pdf:%' AND am.session_id NOT LIKE 'pdf-doc:%'))"
            }
        }
    }

    /// El texto que se embebe, tal y como lo compone quien la creó.
    ///
    /// TIENE QUE COINCIDIR CON EL DEL CAMINO NORMAL. `memories::guarda` embebe
    /// «título — contenido»; si el relleno embebiera solo el contenido, las
    /// filas arregladas quedarían en otro sitio del espacio vectorial que sus
    /// vecinas, y la búsqueda las ordenaría mal sin fallar.
    fn texto(self, titulo: &str, contenido: &str) -> String {
        match self {
            Clase::Trozo => contenido.to_string(),
            Clase::Memoria => format!("{titulo} — {contenido}"),
        }
    }
}

/// Cuántas filas de esa clase están sin vector.
///
/// La vista de Documentos ya decía «12 de 40 con vector» y no ofrecía arreglarlo.
/// Esto es lo que hace falta para poder ofrecerlo — y desde que acepta `Clase`,
/// también para las memorias, que era el lado sin remedio.
pub fn sin_vector(clase: Clase) -> usize {
    crate::with_db(|c| {
        Ok(cuenta(
            c,
            &format!(
                "SELECT COUNT(*) FROM agent_memories am
                 WHERE {}
                   AND NOT EXISTS (SELECT 1 FROM embeddings e
                                   WHERE e.entity_type = '{}'
                                     AND e.entity_id = CAST(am.id AS TEXT))",
                clase.filtro(),
                clase.etiqueta()
            ),
        ))
    })
    .unwrap_or(0)
}

/// Vuelve a embeber las filas de esa clase que se quedaron sin vector.
///
/// EL CASO QUE ARREGLA: algo que se guardó con Ollama caído queda buscable solo
/// por palabras. Para un documento la salida era borrarlo y volver a ingerirlo;
/// para una memoria NO HABÍA SALIDA — y eso importa más, porque una memoria no
/// se puede «volver a ingerir»: es lo que Lucy aprendió, y perder su mitad
/// semántica la deja fuera de todo lo que no case por palabra exacta.
///
/// Medido en la base de esta máquina cuando se escribió esto: 6 memorias de 101
/// sin vector, y 0 trozos de 797. El lado que tenía botón estaba al día.
///
/// Va por lotes y en el hilo de quien llame.
pub fn reembeber(clase: Clase, stop: &std::sync::atomic::AtomicBool) -> Result<usize, String> {
    let pendientes: Vec<(i64, String, String)> = crate::with_db(|c| {
        let mut st = c
            .prepare(&format!(
                "SELECT am.id, COALESCE(am.title, ''), am.content FROM agent_memories am
                 WHERE {}
                   AND NOT EXISTS (SELECT 1 FROM embeddings e
                                   WHERE e.entity_type = '{}'
                                     AND e.entity_id = CAST(am.id AS TEXT))",
                clase.filtro(),
                clase.etiqueta()
            ))
            .map_err(|e| format!("reembeber: {e}"))?;
        let v = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| format!("reembeber: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })?;
    if pendientes.is_empty() {
        return Ok(0);
    }
    let mut hechos = 0usize;
    for lote in pendientes.chunks(crate::vectors::EMBED_LOTE) {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        let textos: Vec<String> = lote.iter().map(|(_, t, c)| clase.texto(t, c)).collect();
        let (vs, modelo) = crate::vectors::embed_batch(&textos)?;
        let filas: Vec<(String, String, Vec<f32>)> = lote
            .iter()
            .zip(textos)
            .zip(vs)
            .map(|(((id, _, _), texto), v)| (id.to_string(), texto, v))
            .collect();
        hechos += crate::vectors::upsert(clase.etiqueta(), &filas, &modelo)?;
    }
    Ok(hechos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_purga_dice_lo_que_se_lleva_y_con_cuantas() {
        // Un botón de borrado en lote que no dice el número es un botón que no
        // se pulsa — o que se pulsa una vez y ya no se vuelve a confiar en él.
        let r = Recuento {
            retiradas: 14,
            automaticas: 203,
            documentos: 2,
            trozos: 800,
            ..Default::default()
        };
        assert!(Purga::Retiradas.describe(&r).contains("14"));
        assert!(Purga::Automaticas.describe(&r).contains("203"));
        let d = Purga::Documentos.describe(&r);
        assert!(d.contains('2') && d.contains("800"));
    }

    #[test]
    fn la_descripcion_dice_lo_que_se_pierde_y_no_solo_lo_que_se_borra() {
        // «Se borrarán 14 filas» no permite decidir. Lo que hace falta saber es
        // qué deja de poder hacerse después.
        assert!(Purga::Retiradas.describe(&Recuento::default()).contains("rastro"));
        assert!(Purga::Documentos.describe(&Recuento::default()).contains("dejará de"));
    }
}
