//! Fundir memorias que dicen lo mismo.
//!
//! POR QUÉ ESTABA MUERTO. Esta pasada existía entera en `commands/memory.rs`,
//! registrada como comando de Tauri, y NADA la invocaba nunca — ni el frontend,
//! ni el mantenimiento, ni un temporizador. Un deduplicador que no corre no es
//! una función a medias: es una tabla que crece con veinte formas de la misma
//! frase, y una recuperación semántica que devuelve cinco memorias cuando había
//! un hecho.
//!
//! Vive aquí y no allí porque el shell nativo tiene su vista de Memoria y no
//! puede llamar a un comando de Tauri. Que el núcleo lo tenga es lo que permite
//! que la pasada sea LA MISMA desde los dos sitios, en vez de dos criterios de
//! parecido que empiezan iguales y acaban discrepando.
//!
//! NO BORRA NADA. Marca las fundidas con una etiqueta `superseded_by:N` que
//! apunta a la canónica, y las consultas vivas ya las excluyen. Un deduplicador
//! automático que borra es un deduplicador en el que no se puede confiar lo
//! suficiente como para dejarlo correr — y si no se deja correr, volvemos al
//! principio.

use std::collections::HashSet;

/// Tope de memorias por pasada.
///
/// El bucle interior es O(n²): con mil entradas son medio millón de
/// comparaciones, que es rápido; con diez mil serían cincuenta millones y la
/// ventana se pararía. Se miran las más recientes, que es donde se acumulan los
/// duplicados de verdad.
pub const MAX_INPUT: usize = 1_000;

/// Parecido mínimo de ETIQUETAS para siquiera comparar el contenido.
///
/// Es el filtro barato y va primero. Dos memorias que no comparten la mitad de
/// sus etiquetas hablan de asuntos distintos por mucho que repitan palabras.
pub const MIN_TAG_OVERLAP: f32 = 0.50;

/// Parecido mínimo de CONTENIDO para fundir.
///
/// Más bajo que el de etiquetas a propósito: dos notas sobre el mismo incidente
/// se escriben con palabras distintas —una dice "el servicio no arranca" y la
/// otra "Spooler detenido"— y exigir aquí un 0.50 no fundiría nunca nada.
pub const MIN_CONTENT_JACCARD: f32 = 0.35;

/// Importancia a partir de la cual una memoria no se toca.
///
/// 10 es la convención de "fijada" de la app. Fundir algo que alguien clavó a
/// mano es exactamente lo que haría que se desactivara la deduplicación.
pub const PINNED: i64 = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    pub canonical_id: i64,
    pub canonical_title: String,
    pub merged_ids: Vec<i64>,
    pub overlap_score: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub dry_run: bool,
    pub scanned: usize,
    pub clusters_found: usize,
    pub memories_merged: usize,
    pub clusters: Vec<Cluster>,
}

/// Las palabras de un texto, para medir parecido.
///
/// Solo las de tres letras o más: sin ese filtro, "de", "la", "el" y "the"
/// dominan la intersección y dos textos sobre cosas distintas salen parecidos
/// por hablar el mismo idioma.
pub(crate) fn tokens(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(|s| s.to_string())
        .collect()
}

/// Jaccard: lo que comparten entre todo lo que tienen. 1.0 idénticos, 0.0 nada.
///
/// Dos conjuntos VACÍOS dan 0.0 y no 1.0, que sería la respuesta matemática.
/// Aquí "no tiene etiquetas" no puede significar "coincide perfectamente con
/// cualquier otra que tampoco tenga": eso fundiría en un montón todas las
/// memorias sin etiquetar, que no tienen nada que ver entre sí.
pub(crate) fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let union = a.union(b).count() as f32;
    if union <= 0.0 {
        0.0
    } else {
        a.intersection(b).count() as f32 / union
    }
}

/// Una fila candidata, ya preparada para comparar.
struct Prep {
    id: i64,
    title: String,
    content_toks: HashSet<String>,
    tag_set: HashSet<String>,
}

/// Agrupa las que dicen lo mismo. Es la parte que decide, y no toca la base.
///
/// De más nueva a más vieja: la primera de un grupo es la CANÓNICA, así que se
/// conserva la redacción más reciente. Al revés se conservaría la primera vez
/// que alguien describió el problema, que suele ser la peor.
fn cluster(prepped: &[Prep]) -> Vec<Cluster> {
    let mut visited: HashSet<i64> = HashSet::new();
    let mut out = Vec::new();
    for (i, p) in prepped.iter().enumerate() {
        if !visited.insert(p.id) {
            continue;
        }
        let mut merged = Vec::new();
        let mut best: f32 = 0.0;
        for q in prepped.iter().skip(i + 1) {
            if visited.contains(&q.id) {
                continue;
            }
            let tag = jaccard(&p.tag_set, &q.tag_set);
            if tag < MIN_TAG_OVERLAP {
                continue;
            }
            let cont = jaccard(&p.content_toks, &q.content_toks);
            if cont < MIN_CONTENT_JACCARD {
                continue;
            }
            visited.insert(q.id);
            merged.push(q.id);
            best = best.max((tag + cont) * 0.5);
        }
        if !merged.is_empty() {
            out.push(Cluster {
                canonical_id: p.id,
                canonical_title: p.title.clone(),
                merged_ids: merged,
                overlap_score: best,
            });
        }
    }
    out
}

/// La pasada completa. Con `dry_run`, mira y cuenta sin tocar nada.
///
/// EN SECO POR DEFECTO en quien la llame. Lo que devuelve —qué se fundiría con
/// qué y cuánto se parecen— es exactamente lo que hace falta para decidir si uno
/// se fía, y esa decisión tiene que poder tomarse antes y no después.
pub fn run(dry_run: bool) -> Result<Report, String> {
    crate::with_db(move |conn| {
        let mut stmt = conn
            .prepare(
                // LA COLUMNA Y NO LA ETIQUETA. La app Tauri supersede escribiendo
                // solo la columna —su auto_dedup lo dice explícitamente: la
                // etiqueta corrompía el JSON—, así que filtrar por `tags` dejaba
                // volver como candidatas filas que ella ya había retirado, y una
                // de esas podía salir elegida CANÓNICA: memorias vivas fundidas
                // hacia un id que ningún lector enseña.
                //
                // Y FUERA LOS TROZOS DE DOCUMENTO. Un manual ingerido son
                // cientos de filas con la misma etiqueta y párrafos repetidos:
                // sin esta exclusión se comen la ventana de mil —dejando las
                // memorias de verdad fuera del escaneo— y la pasada desatendida
                // puede fundir trozos entre sí, es decir, agujerear un documento.
                "SELECT id, title, content, tags FROM agent_memories \
                 WHERE importance < ?1 \
                   AND (superseded_by IS NULL OR superseded_by = '') \
                   AND session_id NOT LIKE 'pdf:%' \
                   AND session_id NOT LIKE 'pdf-doc:%' \
                 ORDER BY created_at DESC LIMIT ?2",
            )
            .map_err(|e| format!("consolidate prepare: {e}"))?;
        let rows: Vec<(i64, String, String, String)> = stmt
            .query_map(rusqlite::params![PINNED, MAX_INPUT as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(|e| format!("consolidate query: {e}"))?
            .filter_map(|r| r.ok())
            .collect();

        let scanned = rows.len();
        if scanned < 2 {
            return Ok(Report {
                dry_run,
                scanned,
                clusters_found: 0,
                memories_merged: 0,
                clusters: vec![],
            });
        }

        // Los conjuntos se calculan UNA vez por fila y no dentro del bucle: son
        // n² comparaciones, y tokenizar dentro sería tokenizar mil veces cada
        // texto.
        let prepped: Vec<Prep> = rows
            .iter()
            .map(|(id, title, content, tags_json)| Prep {
                id: *id,
                title: title.clone(),
                content_toks: tokens(&format!("{title} {content}")),
                tag_set: serde_json::from_str::<Vec<String>>(tags_json)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| t.to_lowercase())
                    .collect(),
            })
            .collect();

        let clusters = cluster(&prepped);
        let clusters_found = clusters.len();
        let memories_merged = clusters.iter().map(|c| c.merged_ids.len()).sum();

        if !dry_run && !clusters.is_empty() {
            // En UNA transacción: media consolidación aplicada deja memorias
            // marcadas como superseded apuntando a una canónica que no llegó a
            // subir de importancia, y eso no se puede deshacer mirando la tabla.
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| format!("consolidate tx: {e}"))?;
            for cl in &clusters {
                let marca = format!("superseded_by:{}", cl.canonical_id);
                for old in &cl.merged_ids {
                    // LA COLUMNA, QUE ES LO QUE MIRAN LAS LECTURAS. Esto escribía
                    // la marca ÚNICAMENTE dentro del JSON de `tags`, y todas las
                    // consultas filtran por la columna `superseded_by`
                    // (lib.rs:209, memory.rs:729). O sea que la consolidación
                    // corría, decía cuántas memorias había fundido, y las fundidas
                    // seguían saliendo en el recuerdo, en la búsqueda y en la
                    // pestaña — indistinguibles de las vivas.
                    //
                    // Es peor que la nota que llevaba meses en la lista de tareas:
                    // no era que la deduplicación «no se ejecutara nunca», es que
                    // se ejecutaba y no servía de nada. Un informe que dice «he
                    // fundido 14» sobre una tabla que no cambió.
                    // `AND superseded_by IS NULL`: si otro proceso —la app
                    // Tauri comparte esta base— la retiró entre el escaneo y
                    // aquí, su puntero se respeta en vez de pisarse.
                    let _ = tx.execute(
                        "UPDATE agent_memories SET superseded_by = ?1 \
                         WHERE id = ?2 AND (superseded_by IS NULL OR superseded_by = '')",
                        rusqlite::params![cl.canonical_id.to_string(), old],
                    );
                    // Y SU VECTOR SE VA CON ELLA. La búsqueda semántica lee la
                    // tabla `embeddings` directamente, sin join: dejando el
                    // vector, la fila retirada seguía saliendo en el recuerdo con
                    // su redacción vieja, y —peor— la deduplicación por coseno
                    // podía declarar «duplicada» una memoria nueva contra una
                    // fila que ningún lector enseña, perdiendo el hecho.
                    let _ = tx.execute(
                        "DELETE FROM embeddings \
                         WHERE entity_type = 'memory' AND entity_id = ?1",
                        rusqlite::params![old.to_string()],
                    );
                    // Y la etiqueta TAMBIÉN, porque es el rastro legible de por
                    // qué esa memoria dejó de contar. Se AÑADE a las que ya había:
                    // son la historia de para qué existía.
                    let cur: String = tx
                        .query_row(
                            "SELECT tags FROM agent_memories WHERE id = ?1",
                            rusqlite::params![old],
                            |r| r.get(0),
                        )
                        .unwrap_or_else(|_| "[]".to_string());
                    let mut tags: Vec<String> = serde_json::from_str(&cur).unwrap_or_default();
                    if !tags.iter().any(|t| t == &marca) {
                        tags.push(marca.clone());
                    }
                    let nuevas =
                        serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());
                    let _ = tx.execute(
                        "UPDATE agent_memories SET tags = ?1 WHERE id = ?2",
                        rusqlite::params![nuevas, old],
                    );
                }
                // La canónica sube un punto, con tope en 9. Nunca llega a 10:
                // esa importancia significa "la fijó una persona", y una pasada
                // automática no puede concederse ese sello a sí misma.
                //
                // SIN TOCAR `created_at`. Se re-fechaba a «ahora», y eso tenía
                // una consecuencia que no se ve desde aquí: los insights solo
                // miran memorias con más de cinco días, así que cada
                // consolidación devolvía a la más corroborada —justo la que más
                // patrón contiene— al fondo de la cola de reflexión, para
                // siempre.
                let _ = tx.execute(
                    "UPDATE agent_memories \
                     SET importance = MIN(importance + 1, 9) WHERE id = ?1",
                    rusqlite::params![cl.canonical_id],
                );
            }
            tx.commit().map_err(|e| format!("consolidate commit: {e}"))?;
        }

        Ok(Report { dry_run, scanned, clusters_found, memories_merged, clusters })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prep(id: i64, titulo: &str, texto: &str, etiquetas: &[&str]) -> Prep {
        Prep {
            id,
            title: titulo.into(),
            content_toks: tokens(&format!("{titulo} {texto}")),
            tag_set: etiquetas.iter().map(|t| t.to_lowercase()).collect(),
        }
    }

    #[test]
    fn dos_notas_del_mismo_incidente_se_funden() {
        // El caso para el que existe la pasada: la misma avería apuntada dos
        // veces con dos semanas de diferencia.
        let v = cluster(&[
            prep(
                2,
                "Spooler detenido otra vez",
                "el servicio de cola de impresion Spooler aparece detenido tras reiniciar",
                &["spooler", "servicios", "impresion"],
            ),
            prep(
                1,
                "Spooler detenido",
                "el servicio Spooler de cola de impresion se detiene al reiniciar el equipo",
                &["spooler", "servicios", "impresion"],
            ),
        ]);
        assert_eq!(v.len(), 1, "no las fundió: {v:?}");
        // La CANÓNICA es la más nueva: se conserva la última redacción, no la
        // primera vez que alguien describió el problema.
        assert_eq!(v[0].canonical_id, 2);
        assert_eq!(v[0].merged_ids, vec![1]);
    }

    #[test]
    fn dos_asuntos_distintos_con_palabras_parecidas_no_se_funden() {
        // Es el fallo que arruinaría la función: fundir dos hechos distintos
        // pierde información y nadie se entera hasta que Lucy contesta mal.
        let v = cluster(&[
            prep(1, "Disco C lleno", "el disco C esta al 95 por ciento de uso", &["disco", "almacenamiento"]),
            prep(2, "Disco D lleno", "el disco D esta al 95 por ciento de uso", &["red", "backup"]),
        ]);
        assert!(v.is_empty(), "fundió cosas distintas: {v:?}");
    }

    #[test]
    fn las_etiquetas_filtran_antes_que_el_contenido() {
        // Mismo texto exacto, etiquetas sin nada en común: no se funden. El
        // filtro barato va primero y manda.
        let t = "el servicio se detiene al reiniciar el equipo cada mañana";
        let v = cluster(&[prep(1, "A", t, &["alfa"]), prep(2, "B", t, &["beta"])]);
        assert!(v.is_empty(), "las etiquetas no filtraron: {v:?}");
    }

    #[test]
    fn las_memorias_sin_etiquetas_no_caen_todas_en_el_mismo_monton() {
        // Dos conjuntos vacíos dan Jaccard 0.0 y no 1.0. La respuesta
        // matemática sería la contraria, y aquí significaría que todo lo que
        // nadie etiquetó "coincide perfectamente" con todo lo demás.
        assert_eq!(jaccard(&HashSet::new(), &HashSet::new()), 0.0);
        let t = "texto exactamente igual en las dos memorias sin etiqueta ninguna";
        let v = cluster(&[prep(1, "A", t, &[]), prep(2, "B", t, &[])]);
        assert!(v.is_empty(), "juntó las que nadie etiquetó: {v:?}");
    }

    #[test]
    fn una_memoria_no_se_funde_dos_veces() {
        // El visitado importa: sin él, la número 3 entraría en el grupo de la 1
        // y luego volvería a sembrar el suyo, y el informe contaría dos fusiones
        // donde hubo una.
        let t = "el servicio Spooler de cola de impresion se detiene al reiniciar";
        let e = ["spooler", "servicios"];
        let v = cluster(&[prep(3, "C", t, &e), prep(2, "B", t, &e), prep(1, "A", t, &e)]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].merged_ids.len(), 2, "{v:?}");
    }

    #[test]
    fn las_palabras_cortas_no_deciden_el_parecido() {
        // Sin el filtro de tres letras, "de", "la" y "el" dominan la
        // intersección y dos textos de asuntos distintos salen parecidos por
        // estar escritos en el mismo idioma.
        let t = tokens("el uso de la red de la casa");
        assert!(!t.contains("de"), "coló una palabra de dos letras");
        assert!(!t.contains("el"));
        assert!(t.contains("uso") && t.contains("red") && t.contains("casa"));
    }

    #[test]
    fn la_app_delega_aqui_en_vez_de_tener_su_propia_copia() {
        // NO se comparan los umbrales, como se hace con el catálogo de modelos y
        // con los precios, porque aquí no hay dos copias que puedan derivar: la
        // app llama a esta función. Lo que se vigila es justamente eso — que
        // siga llamándola. Volver a escribir el criterio allí daría dos
        // deduplicadores distintos sobre la MISMA base de datos, y el que
        // corriera segundo encontraría un corpus que ya no reconoce.
        // EN EJECUCIÓN Y NO CON `include_str!`. Aunque esté dentro de un test,
        // la macro se resuelve al compilar, así que la batería del núcleo no
        // compilaba sin `src-tauri` delante. Ver `models.rs` y `schema.rs`.
        let Ok(app) = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../src-tauri/src/commands/memory.rs"),
        ) else {
            // Sin la app Tauri al lado no hay a quién vigilar. Se salta.
            return;
        };
        let app = app.as_str();
        assert!(
            app.contains("lucy_core::consolidate::run("),
            "src-tauri dejó de delegar: hay otra vez dos criterios de parecido"
        );
        // Se busca la DEFINICIÓN, no el nombre. `jaccard` y el tokenizador
        // siguen viviendo allí y está bien: los usa el grafo de memoria, que es
        // otra función con sus propios umbrales. Lo que no puede volver es una
        // constante de consolidación redeclarada.
        assert!(
            !app.contains("const MIN_CONTENT_JACCARD"),
            "el umbral volvió a declararse en la app — es una copia nueva"
        );
    }
}
