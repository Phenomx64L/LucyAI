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
///
/// ── ESO ES CIERTO SOLO SI LAS ETIQUETAS DICEN DE QUÉ VA LA MEMORIA ───────────
///
/// Medido sobre un corpus real de 94 candidatas (`cuanto_se_parecen_de_verdad`),
/// no había más que TRES conjuntos de etiquetas en toda la tabla:
///
/// ```text
///   ["auto"]                65 memorias   se guardó sola
///   ["crystal","leccion"]   15 memorias   vino de la cristalización
///   ["crystal","sesion"]    14 memorias   vino de un resumen de sesión
/// ```
///
/// Son marcas de PROCEDENCIA, no de tema. Con ellas esta puerta no pregunta «¿de
/// qué hablan?» sino «¿de dónde venís?», y el resultado es que dejaba pasar 2276
/// pares de 4371 —la mitad, muchos con parecido 1.00 entre memorias que no tienen
/// nada que ver— mientras bloqueaba los dos únicos pares con contenido parecido,
/// por venir uno de `auto` y el otro de `crystal`.
///
/// NO SE QUITA, y esto costó una medición: sin ella, el corpus real fundía
/// «La versión activa más reciente de Fedora» con «qué mejoras tiene la 26.6 de
/// CyberArk EPM». Dos textos largos sobre versiones de software comparten
/// suficientes palabras para pasar el 0.35 sin hablar de lo mismo. La puerta
/// bloqueaba esa fusión falsa, aunque fuera por accidente.
///
/// Lo que se hace es AÑADIR un segundo camino que no depende de las etiquetas.
/// Ver [`MIN_TITLE_JACCARD`].
pub const MIN_TAG_OVERLAP: f32 = 0.50;

/// Parecido mínimo de TÍTULOS para el segundo camino.
///
/// 0.85 es «el mismo título salvo una palabra». No es un parecido: es una
/// identidad. El título es el resumen que la memoria hace de sí misma, y es la
/// única señal temática que no depende de que alguien haya etiquetado bien.
///
/// ── POR QUÉ TAN ALTO, Y POR QUÉ HACE FALTA LA SEGUNDA CONDICIÓN ──────────────
///
/// Medido sobre el corpus real, con el título solo —sin pedir nada del cuerpo—
/// se fundían 20 memorias en 13 grupos, y varias estaban mal:
///
/// ```text
///   «ahora verifica este proyecto» x2      pueden ser DOS PROYECTOS distintos
///   «intenta editarlo nuevamente» x3       relleno, no dice de qué va
///   «esta es la ruta: …INCODE_S3_COPY.xml» x3   misma ruta, preguntas distintas
/// ```
///
/// Pidiendo ADEMÁS [`MIN_TITLE_CONTENT`] de CUERPO, esas se caen solas y quedan
/// 6 memorias en 5 grupos, todas duplicados de verdad, con puntuaciones entre
/// 0.60 y 0.64:
///
/// ```text
///   «Escanea el software instalado en busca de vulnerabilidades…»  x2
///   «Resume los errores más recientes del registro de eventos…»    x3
///   «Revisa la salud del sistema (CPU, RAM, disco, servicios)…»    x2
///   «instala sysinternals»                                         x2
///   «C:\Users\…\INCODE_S3_COPY.xml»                                x2
/// ```
///
/// El título dice de qué va; el cuerpo confirma que además dicen lo mismo.
pub const MIN_TITLE_JACCARD: f32 = 0.85;

/// Cuerpo mínimo cuando el que manda es el título.
///
/// Más bajo que [`MIN_CONTENT_JACCARD`] porque no está decidiendo: está
/// CONFIRMANDO. Con el título ya casi idéntico, esto solo tiene que distinguir
/// «la misma pregunta con la misma respuesta» de «la misma pregunta sobre otra
/// cosa». Ese es el trabajo que hace caer los falsos de arriba.
///
/// SE MIDE CONTRA EL CUERPO SOLO, y esa palabra es media pieza: comparándolo
/// contra título+cuerpo, las palabras del título —que ya se sabe que coinciden—
/// entraban en los dos conjuntos y subían el número por su cuenta. La condición
/// se cumplía por lo que ya se había comprobado. Con el cuerpo aparte, la
/// medición sobre el corpus real bajó de 11 memorias fundidas a 6: las otras
/// cinco se fundían por su propio título. Ver `Prep::body_toks`.
pub const MIN_TITLE_CONTENT: f32 = 0.20;

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

/// Las palabras de un TÍTULO. Aquí no se tira ninguna, por corta que sea.
///
/// ── LO CONTRARIO QUE EN EL CONTENIDO, Y A PROPÓSITO ──────────────────────────
///
/// [`tokens`] tira las palabras de menos de tres letras porque en un párrafo
/// «de», «la» y «el» dominan la intersección y hacen parecidos dos textos que
/// solo comparten idioma. En un título de cuatro palabras eso no pasa: no hay
/// suficiente relleno para dominar nada.
///
/// Y LA PALABRA CORTA ES EL DATO. Con el filtro de tres letras, «Disco C lleno»
/// y «Disco D lleno» quedan las dos en `{disco, lleno}` — idénticas, parecido
/// 1.00— porque la única letra que las distingue se cae por corta. Fundirlas es
/// exactamente el fallo que arruina la consolidación: perder un hecho creyendo
/// que era el mismo hecho.
///
/// Lo encontró el test de la casa `dos_asuntos_distintos_con_palabras_parecidas_no_se_funden`
/// a la primera, que es para lo que estaba puesto. Con este tokenizador esos dos
/// títulos dan `{disco, c, lleno}` y `{disco, d, lleno}`: 0.50, muy por debajo
/// del umbral.
pub(crate) fn tokens_titulo(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
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
    /// Las palabras del título SOLO. Aparte de `content_toks` —que lleva el
    /// título y el cuerpo juntos— porque ahí dentro el título queda diluido: dos
    /// memorias con el título literalmente idéntico se quedaban en 0.29 de
    /// parecido, por debajo del umbral, cuando el cuerpo era largo y distinto.
    title_toks: HashSet<String>,
    content_toks: HashSet<String>,
    /// El CUERPO solo, sin el título delante.
    ///
    /// `content_toks` lleva los dos juntos, y para el camino de etiquetas está
    /// bien: los títulos son distintos y aportan señal. Para el camino de título
    /// NO SIRVE, y esto costó un test: con dos títulos idénticos, sus palabras
    /// entran en los dos conjuntos y suben el parecido de contenido solas. La
    /// condición «y además el cuerpo se parece» acababa cumpliéndose por el
    /// título, es decir, no comprobaba nada. Dos memorias tituladas «ahora
    /// verifica este proyecto» sobre DOS PROYECTOS DISTINTOS daban 0.25 y se
    /// fundían.
    body_toks: HashSet<String>,
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
            let cont = jaccard(&p.content_toks, &q.content_toks);
            // ── DOS CAMINOS, Y BASTA CON UNO ────────────────────────────────
            //
            // POR ETIQUETAS, que es el de siempre y sigue mandando donde las
            // etiquetas dicen de qué va la memoria.
            let tag = jaccard(&p.tag_set, &q.tag_set);
            let por_etiquetas = tag >= MIN_TAG_OVERLAP && cont >= MIN_CONTENT_JACCARD;
            // POR TÍTULO, para cuando no lo dicen. Medido: en un corpus real las
            // etiquetas eran marcas de procedencia («auto», «crystal») y este
            // camino era el único que podía encontrar los cuatro ejemplares de
            // «Escanea el software instalado en busca de vulnerabilidades».
            //
            // SE AÑADE, NO SUSTITUYE: así una instalación cuyas etiquetas SÍ
            // sean temáticas no pierde ni una fusión de las que ya hacía.
            let tit = jaccard(&p.title_toks, &q.title_toks);
            // EL CUERPO SOLO, no `cont`. Con los títulos ya casi idénticos, sus
            // palabras están dentro de `content_toks` en los dos lados y suben
            // ese número por su cuenta: la condición se cumpliría por el título
            // que ya se comprobó, y no confirmaría nada. Ver `Prep::body_toks`.
            let cuerpo = jaccard(&p.body_toks, &q.body_toks);
            let por_titulo = tit >= MIN_TITLE_JACCARD && cuerpo >= MIN_TITLE_CONTENT;
            if !por_etiquetas && !por_titulo {
                continue;
            }
            visited.insert(q.id);
            merged.push(q.id);
            // La puntuación, del camino que de verdad abrió la puerta. Promediar
            // los cuatro números daría un 0.4 tibio a una fusión que se decidió
            // por un título idéntico, y esa cifra se enseña al operador cuando
            // mira si se fía de la pasada.
            let punto = if por_etiquetas { (tag + cont) * 0.5 } else { (tit + cuerpo) * 0.5 };
            best = best.max(punto);
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
                title_toks: tokens_titulo(title),
                content_toks: tokens(&format!("{title} {content}")),
                body_toks: tokens(content),
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
            title_toks: tokens_titulo(titulo),
            content_toks: tokens(&format!("{titulo} {texto}")),
            body_toks: tokens(texto),
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
    fn el_mismo_titulo_funde_aunque_las_etiquetas_no_digan_nada() {
        // EL CAMINO NUEVO. Las dos llevan la misma etiqueta de procedencia
        // —«auto», que es lo que había en el corpus real— así que la puerta de
        // etiquetas las deja pasar pero el contenido se queda corto: 0.35 sobre
        // dos respuestas distintas a la misma pregunta no se alcanza. Antes esto
        // no se fundía nunca; medido en la máquina del operador, cuatro copias de
        // esta misma frase llevaban meses en la tabla.
        let v = cluster(&[
            prep(
                1,
                "Escanea el software instalado en busca de vulnerabilidades",
                "se revisaron los paquetes instalados y se encontraron avisos",
                &["auto"],
            ),
            prep(
                2,
                "Escanea el software instalado en busca de vulnerabilidades",
                "se revisaron los paquetes y no se encontraron avisos nuevos",
                &["auto"],
            ),
        ]);
        assert_eq!(v.len(), 1, "el título idéntico no fundió: {v:?}");
        assert_eq!(v[0].canonical_id, 1, "la canónica es la más reciente");
    }

    #[test]
    fn el_mismo_titulo_con_otro_asunto_dentro_no_funde() {
        // «ahora verifica este proyecto» aparecía DOS VECES en el corpus real, y
        // el título no dice cuál es el proyecto. Sin la segunda condición se
        // fundirían dos hechos distintos; con ella, no.
        let v = cluster(&[
            prep(1, "ahora verifica este proyecto", "el repositorio de facturación compila sin avisos", &["auto"]),
            prep(2, "ahora verifica este proyecto", "la migración del inventario tiene ocho pruebas rojas", &["auto"]),
        ]);
        assert!(v.is_empty(), "fundió dos proyectos distintos: {v:?}");
    }

    #[test]
    fn un_numero_de_version_en_el_titulo_separa() {
        // La misma familia que «Disco C lleno» / «Disco D lleno», pero con lo que
        // de verdad aparece en las memorias de un administrador. Si el
        // tokenizador de títulos volviera a tirar las palabras cortas, «v1» y
        // «v2» desaparecerían, los títulos serían el mismo, y con los cuerpos
        // idénticos el camino de título fundiría dos despliegues distintos.
        //
        // LAS ETIQUETAS NO SE PISAN A PROPÓSITO, y eso hay que explicarlo. Con
        // las dos en «auto» este test no probaría nada de lo que dice probar:
        // dispararía el camino de ETIQUETAS —etiqueta 1.00, cuerpo 1.00— que es
        // el de siempre y funde igual desde antes de todo esto. Cerrándolo, la
        // única puerta que queda abierta es la nueva, que es la que se examina.
        let v = cluster(&[
            prep(1, "actualiza el agente a v1", "el despliegue del agente terminó sin incidencias", &["alfa"]),
            prep(2, "actualiza el agente a v2", "el despliegue del agente terminó sin incidencias", &["beta"]),
        ]);
        assert!(v.is_empty(), "la versión del título no separó: {v:?}");
        // Y la razón exacta, para que si esto se rompe se sepa cuál de los dos
        // números cambió.
        assert!(
            jaccard(&tokens_titulo("actualiza el agente a v1"), &tokens_titulo("actualiza el agente a v2"))
                < MIN_TITLE_JACCARD,
            "el tokenizador de títulos volvió a tirar las palabras cortas"
        );
    }

    #[test]
    fn con_etiquetas_de_procedencia_el_camino_viejo_se_fia_del_cuerpo() {
        // ESTO NO ES UN ARREGLO, ES UN AVISO ESCRITO EN UN TEST. Lo de arriba
        // sale fundido si las dos memorias comparten etiqueta, y en el corpus
        // real el 69 % comparte «auto». O sea: el camino de etiquetas, con
        // etiquetas de procedencia, decide SOLO por el cuerpo — y un cuerpo
        // idéntico con títulos que se diferencian en la versión se funde.
        //
        // Se deja como está y no se toca aquí por dos razones: es de antes de
        // este trabajo, y sobre el corpus medido no dispara ni una vez (cero
        // grupos por este camino en 4371 pares). Cambiarlo sería QUITAR fusiones
        // sin haber medido a quién se las quito. Si algún día hay que apretarlo,
        // este test dice exactamente qué caso hay que mirar.
        let v = cluster(&[
            prep(1, "actualiza el agente a v1", "el despliegue del agente terminó sin incidencias", &["auto"]),
            prep(2, "actualiza el agente a v2", "el despliegue del agente terminó sin incidencias", &["auto"]),
        ]);
        assert_eq!(v.len(), 1, "esto cambió: repasa si el cambio era el que querías");
    }

    #[test]
    fn el_camino_de_etiquetas_sigue_intacto() {
        // AÑADIR NO ES SUSTITUIR. Una instalación cuyas etiquetas sí sean
        // temáticas tiene que fundir exactamente lo que fundía antes, con títulos
        // que no se parecen en nada.
        let v = cluster(&[
            prep(
                1,
                "Spooler detenido",
                "el servicio de impresión se detiene al reiniciar el equipo por la mañana",
                &["impresion", "servicios"],
            ),
            prep(
                2,
                "No arranca la impresión",
                "el servicio de impresión se detiene al reiniciar el equipo por la mañana",
                &["impresion", "servicios"],
            ),
        ]);
        assert_eq!(v.len(), 1, "el camino de siempre dejó de funcionar: {v:?}");
        assert!(
            jaccard(&tokens_titulo("Spooler detenido"), &tokens_titulo("No arranca la impresión"))
                < MIN_TITLE_JACCARD,
            "este test dejaría de probar el camino de etiquetas si los títulos se parecieran"
        );
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
                .join("../lucy-svelte/src-tauri/src/commands/memory.rs"),
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

    /// Mide el parecido REAL del corpus de ESTA máquina. Solo lectura.
    ///
    /// ── POR QUÉ HACE FALTA UN INSTRUMENTO Y NO UN RAZONAMIENTO ───────────────
    ///
    /// La bitácora de mantenimiento dice que la consolidación ha corrido cuatro
    /// veces en tres días y medio y ha fundido CERO memorias, siempre. Eso admite
    /// dos lecturas opuestas:
    ///
    ///   • el corpus no tiene duplicados, y el trabajo está haciendo lo correcto;
    ///   • los umbrales son inalcanzables, y el trabajo no puede fundir nada
    ///     nunca — que es el patrón de la casa con otra cara: una pieza que corre,
    ///     informa de su éxito y no hace nada.
    ///
    /// Desde fuera son indistinguibles: las dos escriben «0 fundidas». La única
    /// forma de separarlas es mirar la DISTRIBUCIÓN de parecidos y ver a qué
    /// distancia se queda el par más parecido del corpus.
    ///
    /// Corre el tokenizador y el `jaccard` DE VERDAD, no una copia en otro
    /// lenguaje: reimplementarlos mediría un criterio parecido pero distinto, que
    /// es justo el error que este módulo lleva dos comentarios intentando evitar.
    ///
    /// `cargo test -p lucy-core --lib consolidate::tests::cuanto_se_parecen_de_verdad -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn cuanto_se_parecen_de_verdad() {
        let db = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default())
            .join("AppData/Roaming/com.lucy.dev/lucy.db");
        if !db.exists() {
            println!("sin base en {db:?}: nada que medir");
            return;
        }
        if let Err(e) = crate::init(&db) {
            println!("no se pudo abrir: {e}");
            return;
        }

        // EXACTAMENTE la consulta de `run`, para mirar las mismas filas.
        let rows: Vec<(i64, String, String, String)> = crate::with_db(|c| {
            let mut st = c
                .prepare(
                    "SELECT id, title, content, tags FROM agent_memories \
                     WHERE importance < ?1 \
                       AND (superseded_by IS NULL OR superseded_by = '') \
                       AND session_id NOT LIKE 'pdf:%' \
                       AND session_id NOT LIKE 'pdf-doc:%' \
                     ORDER BY created_at DESC LIMIT ?2",
                )
                .map_err(|e| e.to_string())?;
            let v = st
                .query_map(rusqlite::params![PINNED, MAX_INPUT as i64], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            Ok(v)
        })
        .expect("consulta");

        let total: i64 = crate::with_db(|c| {
            c.query_row("SELECT COUNT(*) FROM agent_memories", [], |r| r.get(0))
                .map_err(|e| e.to_string())
        })
        .unwrap_or(-1);
        println!("\n{total} memorias en la tabla, {} pasan el filtro", rows.len());

        let prepped: Vec<Prep> = rows
            .iter()
            .map(|(id, title, content, tags_json)| Prep {
                id: *id,
                title: title.clone(),
                title_toks: tokens_titulo(title),
                content_toks: tokens(&format!("{title} {content}")),
                body_toks: tokens(content),
                tag_set: serde_json::from_str::<Vec<String>>(tags_json)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| t.to_lowercase())
                    .collect(),
            })
            .collect();

        let sin_tags = prepped.iter().filter(|p| p.tag_set.is_empty()).count();
        println!("{sin_tags} de {} no tienen NINGUNA etiqueta", prepped.len());

        // Todos los pares. Se guarda el parecido de etiquetas y el de contenido
        // por separado, porque los dos umbrales son puertas independientes y
        // saber CUÁL cierra es lo que dice qué habría que tocar.
        let mut pares: Vec<(f32, f32, usize, usize)> = Vec::new();
        for i in 0..prepped.len() {
            for j in (i + 1)..prepped.len() {
                let t = jaccard(&prepped[i].tag_set, &prepped[j].tag_set);
                let c = jaccard(&prepped[i].content_toks, &prepped[j].content_toks);
                pares.push((t, c, i, j));
            }
        }
        println!("{} pares comparados\n", pares.len());

        let pasa_tag = pares.iter().filter(|(t, _, _, _)| *t >= MIN_TAG_OVERLAP).count();
        let pasa_cont = pares.iter().filter(|(_, c, _, _)| *c >= MIN_CONTENT_JACCARD).count();
        let pasan_ambas = pares
            .iter()
            .filter(|(t, c, _, _)| *t >= MIN_TAG_OVERLAP && *c >= MIN_CONTENT_JACCARD)
            .count();
        println!("── LAS DOS PUERTAS ──");
        println!("  etiquetas >= {MIN_TAG_OVERLAP}: {pasa_tag} pares");
        println!("  contenido >= {MIN_CONTENT_JACCARD}: {pasa_cont} pares");
        println!("  LAS DOS:                 {pasan_ambas} pares  <- estos se fundirían\n");

        let mut por_tag = pares.clone();
        por_tag.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!("── LOS 8 PARES CON MÁS ETIQUETAS EN COMÚN ──");
        for (t, c, i, j) in por_tag.iter().take(8) {
            let corta = |s: &str| s.chars().take(46).collect::<String>();
            println!(
                "  tag {t:.2} cont {c:.2} {} «{}» / «{}»",
                if *t >= MIN_TAG_OVERLAP && *c >= MIN_CONTENT_JACCARD { "SI" } else { "no" },
                corta(&prepped[*i].title),
                corta(&prepped[*j].title)
            );
        }

        let mut por_cont = pares.clone();
        por_cont.sort_by(|a, b| b.1.total_cmp(&a.1));
        println!("\n── LOS 8 PARES CON MÁS CONTENIDO EN COMÚN ──");
        for (t, c, i, j) in por_cont.iter().take(8) {
            let corta = |s: &str| s.chars().take(46).collect::<String>();
            println!(
                "  cont {c:.2} tag {t:.2} {} «{}» / «{}»",
                if *t >= MIN_TAG_OVERLAP && *c >= MIN_CONTENT_JACCARD { "SI" } else { "no" },
                corta(&prepped[*i].title),
                corta(&prepped[*j].title)
            );
        }

        // Y lo que diría la pasada de verdad, en seco.
        match run(true) {
            Ok(r) => {
                println!(
                    "\nrun(en seco): {} miradas, {} grupos, {} se fundirían",
                    r.scanned, r.clusters_found, r.memories_merged
                );
                let por_id: std::collections::HashMap<i64, &str> =
                    prepped.iter().map(|p| (p.id, p.title.as_str())).collect();
                for cl in &r.clusters {
                    let corta = |s: &str| s.chars().take(62).collect::<String>();
                    println!(
                        "   [{:.2}] queda #{} «{}»",
                        cl.overlap_score,
                        cl.canonical_id,
                        corta(&cl.canonical_title)
                    );
                    for id in &cl.merged_ids {
                        println!(
                            "          se retira #{id} «{}»",
                            corta(por_id.get(id).copied().unwrap_or("?"))
                        );
                    }
                }
            }
            Err(e) => println!("\nrun falló: {e}"),
        }

        // ── QUÉ HARÍA CADA ALTERNATIVA ──────────────────────────────────────
        //
        // Cambiar el criterio es cambiar una ESCRITURA DESATENDIDA sobre la
        // memoria del operador. Proponer un número sin enseñar a quién le toca
        // sería pedirle que se fíe. Esto enseña, para cada criterio candidato,
        // exactamente qué memorias se fundirían con cuáles.
        let titulo_toks: Vec<HashSet<String>> =
            prepped.iter().map(|p| tokens_titulo(&p.title)).collect();
        // Un nombre y una regla. El tipo es feo porque cada regla cierra sobre
        // `prepped`, y ponerle un alias no lo haría más legible.
        #[allow(clippy::type_complexity)]
        let criterios: Vec<(&str, Box<dyn Fn(usize, usize) -> bool>)> = vec![
            (
                "HOY: etiquetas>=0.50 Y contenido>=0.35",
                Box::new(|i: usize, j: usize| {
                    jaccard(&prepped[i].tag_set, &prepped[j].tag_set) >= MIN_TAG_OVERLAP
                        && jaccard(&prepped[i].content_toks, &prepped[j].content_toks)
                            >= MIN_CONTENT_JACCARD
                }),
            ),
            (
                "A: sin la puerta de etiquetas, contenido>=0.35",
                Box::new(|i: usize, j: usize| {
                    jaccard(&prepped[i].content_toks, &prepped[j].content_toks)
                        >= MIN_CONTENT_JACCARD
                }),
            ),
            (
                "B: el TÍTULO idéntico (normalizado), y nada más",
                Box::new(|i: usize, j: usize| {
                    prepped[i].title.trim().to_lowercase() == prepped[j].title.trim().to_lowercase()
                        && !prepped[i].title.trim().is_empty()
                }),
            ),
            (
                "C: título>=0.85 Y contenido>=0.20",
                Box::new(|i: usize, j: usize| {
                    jaccard(&titulo_toks[i], &titulo_toks[j]) >= 0.85
                        && jaccard(&prepped[i].content_toks, &prepped[j].content_toks) >= 0.20
                }),
            ),
        ];

        for (nombre, pasa) in &criterios {
            // Mismo agrupado que `cluster`: el primero de un grupo es el
            // canónico y nadie entra en dos grupos.
            let mut visto: HashSet<usize> = HashSet::new();
            let mut grupos: Vec<(usize, Vec<usize>)> = Vec::new();
            for i in 0..prepped.len() {
                if !visto.insert(i) {
                    continue;
                }
                let mut con = Vec::new();
                for j in (i + 1)..prepped.len() {
                    if !visto.contains(&j) && pasa(i, j) {
                        visto.insert(j);
                        con.push(j);
                    }
                }
                if !con.is_empty() {
                    grupos.push((i, con));
                }
            }
            let fundidas: usize = grupos.iter().map(|(_, v)| v.len()).sum();
            println!("\n════ {nombre}");
            println!("     {} grupos, {fundidas} memorias se retirarían", grupos.len());
            for (c, v) in grupos.iter() {
                let corta = |s: &str| s.chars().take(58).collect::<String>();
                println!(
                    "       [{} palabras en el título] queda «{}»",
                    titulo_toks[*c].len(),
                    corta(&prepped[*c].title)
                );
                for j in v {
                    println!("         se retira «{}»", corta(&prepped[*j].title));
                }
            }
        }
        println!();
    }
}
