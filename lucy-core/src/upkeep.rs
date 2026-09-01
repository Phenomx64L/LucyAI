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

/// Cuántos trozos de documento están sin vector.
///
/// La vista de Documentos ya dice «12 de 40 con vector» y no ofrecía arreglarlo.
/// Esto es lo que hace falta para poder ofrecerlo.
pub fn sin_vector() -> usize {
    crate::with_db(|c| {
        Ok(cuenta(
            c,
            "SELECT COUNT(*) FROM agent_memories am
             WHERE am.session_id LIKE 'pdf:%'
               AND NOT EXISTS (SELECT 1 FROM embeddings e
                               WHERE e.entity_type = 'pdf_chunk'
                                 AND e.entity_id = CAST(am.id AS TEXT))",
        ))
    })
    .unwrap_or(0)
}

/// Vuelve a embeber los trozos que se quedaron sin vector.
///
/// EL CASO QUE ARREGLA: una ingesta que empezó con Ollama caído deja el documento
/// buscable solo por palabras, y hasta ahora la única salida era borrarlo y
/// volver a ingerirlo. Va por lotes y en el hilo de quien llame.
pub fn reembeber(stop: &std::sync::atomic::AtomicBool) -> Result<usize, String> {
    let pendientes: Vec<(i64, String)> = crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT am.id, am.content FROM agent_memories am
                 WHERE am.session_id LIKE 'pdf:%'
                   AND NOT EXISTS (SELECT 1 FROM embeddings e
                                   WHERE e.entity_type = 'pdf_chunk'
                                     AND e.entity_id = CAST(am.id AS TEXT))",
            )
            .map_err(|e| format!("reembeber: {e}"))?;
        let v = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
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
        let textos: Vec<String> = lote.iter().map(|(_, t)| t.clone()).collect();
        let (vs, modelo) = crate::vectors::embed_batch(&textos)?;
        let filas: Vec<(String, String, Vec<f32>)> = lote
            .iter()
            .zip(vs)
            .map(|((id, t), v)| (id.to_string(), t.clone(), v))
            .collect();
        hechos += crate::vectors::upsert("pdf_chunk", &filas, &modelo)?;
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
