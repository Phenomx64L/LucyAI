//! Recuperación semántica: almacenamiento de vectores, filtros y ranking.
//!
//! Segundo lote hacia el corazón compartido, y con una frontera distinta a la
//! del primero. `commands/embeddings.rs` tiene trece funciones `async` sobre
//! `reqwest`; moverlas aquí arrastraría tokio a un crate que lo evita a
//! propósito — y el shell nativo, que es un bucle inmediato síncrono, quiere
//! precisamente lo contrario.
//!
//! Así que la costura no está en "mover embeddings.rs". Está aquí:
//!
//!   • Este módulo posee lo que NO debe duplicarse: el formato del blob, el
//!     coseno, y sobre todo las reglas de filtrado que deciden qué fila puede
//!     compararse con qué consulta.
//!   • Cada frontend obtiene el vector de consulta como le conviene: la app
//!     Tauri con `reqwest` async, el shell nativo con `ureq` bloqueante.
//!
//! Lo que se comparte es el juicio; lo que se duplica es el transporte, que es
//! el reparto correcto.

use serde::{Deserialize, Serialize};

/// Un acierto de búsqueda semántica.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticHit {
    pub entity_type: String,
    pub entity_id: String,
    pub text: String,
    pub score: f32,
}

/// Una fila candidata leída de `embeddings`, tal cual sale de la DB.
pub struct StoredVector {
    pub entity_type: String,
    pub entity_id: String,
    pub text: String,
    pub vec: Vec<f32>,
    pub model: String,
}

/// f32 → bytes little-endian.
pub fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// Inversa de `vec_to_blob`. Ignora una cola incompleta en vez de entrar en
/// pánico: un blob truncado es una fila corrupta, no un fallo del proceso.
pub fn blob_to_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Similitud coseno. Devuelve 0.0 si alguno tiene magnitud cero (evita NaN) o
/// si las longitudes difieren.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Por qué se descartó una fila candidata. Devuelto junto al ranking para que
/// quien llama pueda avisar al operador en vez de tragárselo.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RankSkips {
    /// Dimensión distinta a la de la consulta.
    pub dims: usize,
    /// Embebedor distinto al de la consulta.
    pub model: usize,
    /// Filas examinadas en total.
    pub total: usize,
}

/// Ordena candidatos por coseno, descartando los que no son comparables.
///
/// LOS DOS FILTROS SON DISTINTOS Y AMBOS HACEN FALTA.
///
/// La dimensión es el obvio: un vector de otra longitud vive en otro espacio y
/// `cosine` devolvería 0.0 en silencio, así que la fila desaparecería del
/// recuerdo sin señal.
///
/// El modelo es el que no se ve. `nomic-embed-text` (Ollama) y
/// `text-embedding-004` (Gemini) emiten AMBOS 768 dimensiones, y Lucy alterna
/// entre ellos porque la escritura tiene fallback. Comparar entre esos dos
/// espacios no da error ni un cero delator: da un número plausible que ordena
/// mal. La columna `model` se escribía en cada fila desde que existe la tabla y
/// no la leía nadie.
pub fn rank_by_cosine(
    rows: Vec<StoredVector>,
    query: &[f32],
    query_model: &str,
    min_score: f32,
    limit: usize,
) -> (Vec<SemanticHit>, RankSkips) {
    let qdim = query.len();
    let mut skips = RankSkips { total: rows.len(), ..Default::default() };

    let mut scored: Vec<SemanticHit> = rows
        .into_iter()
        .filter_map(|r| {
            if r.vec.len() != qdim {
                skips.dims += 1;
                return None;
            }
            if r.model != query_model {
                skips.model += 1;
                return None;
            }
            let s = cosine(query, &r.vec);
            if s >= min_score {
                Some(SemanticHit {
                    entity_type: r.entity_type,
                    entity_id: r.entity_id,
                    text: r.text,
                    score: s,
                })
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    (scored, skips)
}

/// Aviso para filas de otra DIMENSIÓN. `None` si no se descartó ninguna.
pub fn dim_mismatch_warning(skipped: usize, total: usize, query_dim: usize) -> Option<String> {
    if skipped == 0 {
        return None;
    }
    Some(format!(
        "semantic_search: {}/{} stored embeddings have a dimension ≠ {} \
         (embedding model changed?). They are invisible to recall until \
         re-embedded — run backfill_embeddings.",
        skipped, total, query_dim
    ))
}

/// Aviso para filas de otro EMBEBEDOR, que es el caso que la dimensión no ve.
///
/// Mensaje separado a propósito: el de dimensión significa "tu embebedor cambió
/// de forma" y manda al operador a buscar un ajuste que no tocó; éste significa
/// "parte del corpus se escribió mientras Ollama estaba caído". Mismo remedio,
/// causa distinta.
pub fn model_mismatch_warning(skipped: usize, total: usize, query_model: &str) -> Option<String> {
    if skipped == 0 {
        return None;
    }
    Some(format!(
        "semantic_search: {}/{} stored embeddings came from a different embedder \
         than the query ('{}'). Cosine across two models is noise even at equal \
         dimensions, so they are excluded rather than ranked — re-embed them to \
         make them recallable again.",
        skipped, total, query_model
    ))
}

// ── El lado que toca el mundo ────────────────────────────────────────────────
//
// Bloqueante a propósito. El shell nativo es un bucle inmediato síncrono; meter
// tokio para una petición HTTP le añadiría un runtime entero y el `async` se
// contagiaría hacia arriba hasta el `update()`. La app Tauri sigue usando su
// `reqwest` async — este módulo no la toca.

const OLLAMA: &str = "http://localhost:11434";
/// Mismo modelo por defecto que `commands/embeddings.rs`. Si divergen, cada
/// frontend escribe en un espacio latente distinto y el filtro de modelo de
/// `rank_by_cosine` los separará — correcto, pero cada uno vería la mitad del
/// corpus.
pub const DEFAULT_EMBED_MODEL: &str = "nomic-embed-text";

/// Embebe un texto con Ollama. Devuelve `(vector, modelo_usado)`.
///
/// Sin fallback a la nube: aquí no hay claves de API ni keyring. Si Ollama no
/// responde, quien llama decide qué enseñar — y decirlo es mejor que devolver
/// resultados de un embebedor distinto sin avisar.
pub fn embed_blocking(text: &str) -> Result<(Vec<f32>, String), String> {
    let body = serde_json::json!({ "model": DEFAULT_EMBED_MODEL, "prompt": text });
    let resp = ureq::post(&format!("{OLLAMA}/api/embeddings"))
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(30))
        .send_string(&body.to_string())
        .map_err(|e| format!("Ollama no respondió ({OLLAMA}): {e}"))?;
    // `into_string()` + parse, igual que `chat.rs`: `into_json()` requiere la
    // feature `json` de ureq, que este crate no activa a propósito para no
    // duplicar el serde_json que ya trae.
    let text_body = resp
        .into_string()
        .map_err(|e| format!("respuesta de Ollama ilegible: {e}"))?;
    let json: serde_json::Value = serde_json::from_str(&text_body)
        .map_err(|e| format!("JSON inválido de Ollama: {e}"))?;
    let arr = json["embedding"]
        .as_array()
        .ok_or("la respuesta de Ollama no trae 'embedding'")?;
    let v: Vec<f32> = arr.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect();
    if v.is_empty() {
        return Err("Ollama devolvió un vector vacío".into());
    }
    Ok((v, DEFAULT_EMBED_MODEL.to_string()))
}

/// Carga los vectores almacenados de un tipo de entidad.
///
/// Se traen todos y se ordenan en memoria. Es el mismo escaneo lineal que hace
/// la app Tauri cuando el índice no está listo: para el tamaño de un corpus
/// personal —miles, no millones— cuesta milisegundos y no arrastra sqlite-vec al
/// crate compartido.
/// Cuántos textos van en una petición al embebedor.
///
/// Treinta y dos. El límite no es el servidor sino la memoria del modelo: cada
/// texto de un lote se procesa a la vez, y un lote de trescientos trozos de
/// manual hace que Ollama reserve de golpe lo que no tiene.
pub const EMBED_LOTE: usize = 32;

/// Vectores de VARIOS textos en una sola petición.
///
/// EXISTE PORQUE `embed_blocking` ES DE UNO EN UNO. Un manual de trescientas
/// páginas son unos cuatrocientos trozos, y con la función de uno en uno eso son
/// cuatrocientas peticiones HTTP EN SERIE, cada una con treinta segundos de
/// plazo. Ingerir un documento por ese camino no es lento: es una tarde.
///
/// Usa `/api/embed`, que acepta una lista, y cae a la vieja `/api/embeddings` de
/// uno en uno si el Ollama instalado es anterior — perder el lote es ir despacio,
/// y fallar es no poder ingerir nada.
pub fn embed_batch(textos: &[String]) -> Result<(Vec<Vec<f32>>, String), String> {
    if textos.is_empty() {
        return Ok((vec![], DEFAULT_EMBED_MODEL.to_string()));
    }
    let body = serde_json::json!({ "model": DEFAULT_EMBED_MODEL, "input": textos });
    let resp = ureq::post(&format!("{OLLAMA}/api/embed"))
        .set("content-type", "application/json")
        // Más que los 30 s de uno suelto: son treinta y dos textos en la misma
        // petición, y el plazo tiene que dar para el lote entero.
        .timeout(std::time::Duration::from_secs(120))
        .send_string(&body.to_string());

    let resp = match resp {
        Ok(r) => r,
        // Un Ollama anterior a `/api/embed` contesta 404. Se cae al camino
        // lento en vez de rendirse: ir despacio es peor que ir rápido, y no
        // poder ingerir es peor que las dos cosas.
        Err(_) => return uno_a_uno(textos),
    };
    let cuerpo = resp
        .into_string()
        .map_err(|e| format!("respuesta de Ollama ilegible: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&cuerpo).map_err(|e| format!("JSON inválido de Ollama: {e}"))?;
    let Some(arr) = json["embeddings"].as_array() else {
        return uno_a_uno(textos);
    };
    let vs: Vec<Vec<f32>> = arr
        .iter()
        .map(|v| {
            v.as_array()
                .map(|a| a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect())
                .unwrap_or_default()
        })
        .collect();
    // UNO POR TEXTO, o no vale. Un lote que devuelve menos vectores que textos
    // desalinea la correspondencia y cada trozo acaba con el vector del
    // siguiente — que no da error, da búsquedas que devuelven el párrafo de al
    // lado.
    if vs.len() != textos.len() || vs.iter().any(|v| v.is_empty()) {
        return uno_a_uno(textos);
    }
    Ok((vs, DEFAULT_EMBED_MODEL.to_string()))
}

fn uno_a_uno(textos: &[String]) -> Result<(Vec<Vec<f32>>, String), String> {
    let mut out = Vec::with_capacity(textos.len());
    let mut modelo = DEFAULT_EMBED_MODEL.to_string();
    for t in textos {
        let (v, m) = embed_blocking(t)?;
        modelo = m;
        out.push(v);
    }
    Ok((out, modelo))
}

/// Guarda —o reemplaza— los vectores de un conjunto de entidades.
///
/// `ON CONFLICT` sobre `(entity_type, entity_id)`: reingerir un documento tiene
/// que sustituir sus vectores, no acumular una segunda copia que competiría con
/// la primera en cada búsqueda.
pub fn upsert(
    entity_type: &str,
    filas: &[(String, String, Vec<f32>)],
    modelo: &str,
) -> Result<usize, String> {
    if filas.is_empty() {
        return Ok(0);
    }
    crate::with_db(|c| {
        let tx = c
            .unchecked_transaction()
            .map_err(|e| format!("vectors upsert tx: {e}"))?;
        for (entity_id, texto, v) in filas {
            tx.execute(
                "INSERT INTO embeddings (id, entity_type, entity_id, text, vec, dims, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(entity_type, entity_id) DO UPDATE SET
                     text = excluded.text, vec = excluded.vec,
                     dims = excluded.dims, model = excluded.model",
                rusqlite::params![
                    format!("{entity_type}:{entity_id}"),
                    entity_type,
                    entity_id,
                    texto,
                    vec_to_blob(v),
                    v.len() as i64,
                    modelo,
                ],
            )
            .map_err(|e| format!("vectors upsert: {e}"))?;
        }
        tx.commit().map_err(|e| format!("vectors upsert commit: {e}"))?;
        Ok(filas.len())
    })
}

/// Crea la tabla de vectores si no está. Mismo DDL que la app.
pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS embeddings (
                 id           TEXT PRIMARY KEY,
                 entity_type  TEXT NOT NULL,
                 entity_id    TEXT NOT NULL,
                 text         TEXT NOT NULL,
                 vec          BLOB NOT NULL,
                 dims         INTEGER NOT NULL,
                 model        TEXT NOT NULL,
                 created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now'))
             );
             CREATE UNIQUE INDEX IF NOT EXISTS idx_embeddings_entity
                 ON embeddings(entity_type, entity_id);",
        )
        .map_err(|e| format!("vectors: esquema: {e}"))
    })
}

pub fn load_stored(entity_type: &str) -> Result<Vec<StoredVector>, String> {
    crate::with_db(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT entity_type, entity_id, text, vec, model
                 FROM embeddings WHERE entity_type = ?1",
            )
            .map_err(|e| format!("prepare load_stored: {e}"))?;
        let rows = stmt
            .query_map([entity_type], |r| {
                let blob: Vec<u8> = r.get(3)?;
                Ok(StoredVector {
                    entity_type: r.get(0)?,
                    entity_id: r.get(1)?,
                    text: r.get(2)?,
                    vec: blob_to_vec(&blob),
                    model: r.get(4)?,
                })
            })
            .map_err(|e| format!("query load_stored: {e}"))?;
        Ok(rows.flatten().collect())
    })
}

/// Búsqueda semántica completa: embebe la consulta, carga, ordena.
///
/// Devuelve además los avisos de filas descartadas, para que la interfaz pueda
/// decir "40 de tus 200 memorias no son buscables" en vez de enseñar menos
/// resultados sin explicar por qué. Ese silencio es exactamente el fallo que
/// esta sesión ha ido persiguiendo.
pub fn search(
    query: &str,
    entity_type: &str,
    limit: usize,
    min_score: f32,
) -> Result<(Vec<SemanticHit>, Vec<String>), String> {
    // LAS FILAS PRIMERO, Y SI NO HAY, NI SE PREGUNTA. Estaba al revés, y el
    // orden importa por lo que cuesta cada mitad: `load_stored` es una consulta
    // local de microsegundos, y `embed_blocking` es una petición HTTP a Ollama
    // con treinta segundos de espera. Con la tabla vacía —que es como está una
    // instalación nueva, y como estaba la de esta máquina— se pagaba un viaje a
    // la red en CADA turno para buscar entre cero vectores.
    let rows = load_stored(entity_type)?;
    if rows.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let (qvec, model) = embed_blocking(query)?;
    let (hits, skips) = rank_by_cosine(rows, &qvec, &model, min_score, limit);
    let notes = [
        dim_mismatch_warning(skips.dims, skips.total, qvec.len()),
        model_mismatch_warning(skips.model, skips.total, &model),
    ]
    .into_iter()
    .flatten()
    .collect();
    Ok((hits, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, v: Vec<f32>, model: &str) -> StoredVector {
        StoredVector {
            entity_type: "memory".into(),
            entity_id: id.into(),
            text: format!("texto {id}"),
            vec: v,
            model: model.into(),
        }
    }

    #[test]
    fn blob_roundtrip_survives_the_database() {
        let v = vec![0.0_f32, 1.5, -2.25, 3.125];
        let b = vec_to_blob(&v);
        assert_eq!(b.len(), v.len() * 4);
        assert_eq!(blob_to_vec(&b), v);
    }

    #[test]
    fn a_truncated_blob_loses_the_tail_instead_of_panicking() {
        // Una fila corrupta no debe tumbar una búsqueda.
        let mut b = vec_to_blob(&[1.0, 2.0]);
        b.pop();
        assert_eq!(blob_to_vec(&b), vec![1.0]);
    }

    #[test]
    fn cosine_refuses_mismatched_and_zero_vectors_instead_of_returning_nan() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn rows_from_another_embedder_are_excluded_even_at_equal_dimensions() {
        // EL caso. Ambas filas tienen la misma dimensión que la consulta, y la
        // del embebedor equivocado está MÁS CERCA. Sin el filtro de modelo
        // ganaría, con una puntuación alta y sin significar nada.
        let rows = vec![
            row("propia", vec![0.8, 0.6], "nomic-embed-text"),
            row("ajena", vec![1.0, 0.0], "text-embedding-004"),
        ];
        let (hits, skips) = rank_by_cosine(rows, &[1.0, 0.0], "nomic-embed-text", 0.0, 10);

        assert_eq!(hits.len(), 1, "solo el embebedor de la consulta debe rankear");
        assert_eq!(hits[0].entity_id, "propia");
        assert_eq!(skips.model, 1);
        assert_eq!(skips.dims, 0);
    }

    #[test]
    fn rows_of_another_dimension_are_excluded_and_counted_apart() {
        let rows = vec![
            row("ok", vec![1.0, 0.0], "m"),
            row("otra-dim", vec![1.0, 0.0, 0.0], "m"),
        ];
        let (hits, skips) = rank_by_cosine(rows, &[1.0, 0.0], "m", 0.0, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(skips.dims, 1);
        assert_eq!(skips.model, 0, "una dimensión distinta no debe contarse como modelo distinto");
    }

    #[test]
    fn results_come_back_best_first_and_respect_the_limit() {
        let rows = vec![
            row("lejos", vec![0.0, 1.0], "m"),
            row("cerca", vec![1.0, 0.0], "m"),
            row("medio", vec![0.7, 0.7], "m"),
        ];
        let (hits, _) = rank_by_cosine(rows, &[1.0, 0.0], "m", 0.0, 2);
        assert_eq!(hits.len(), 2, "el límite se respeta");
        assert_eq!(hits[0].entity_id, "cerca");
        assert_eq!(hits[1].entity_id, "medio");
    }

    #[test]
    fn min_score_drops_the_weak_matches() {
        let rows = vec![row("ortogonal", vec![0.0, 1.0], "m")];
        let (hits, _) = rank_by_cosine(rows, &[1.0, 0.0], "m", 0.25, 10);
        assert!(hits.is_empty(), "un coseno de 0 no debe pasar un umbral de 0.25");
    }

    #[test]
    fn the_two_warnings_are_not_interchangeable() {
        // Dicen cosas distintas al operador y no deben confundirse: una manda a
        // revisar un ajuste, la otra a re-embeber lo escrito durante una caída.
        let dim = dim_mismatch_warning(3, 100, 768).expect("debe avisar");
        let model = model_mismatch_warning(3, 100, "nomic-embed-text").expect("debe avisar");
        assert_ne!(dim, model);
        assert!(dim.contains("dimension ≠"));
        assert!(!model.contains("dimension ≠"));
        assert!(model.contains("nomic-embed-text"));
        assert!(dim_mismatch_warning(0, 100, 768).is_none());
        assert!(model_mismatch_warning(0, 100, "m").is_none());
    }
}
