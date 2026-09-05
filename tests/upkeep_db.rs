//! Cuidados de la base, contra una de verdad.
//!
//! Lo que no se puede probar sin disco: que la copia ABRA, que una purga se
//! lleve los vectores además de las filas, y que respete lo que el operador
//! fijó a mano. Un solo `#[test]` con secciones, por el `OnceCell` del pool.

use lucy_core::upkeep::{self, Purga};

fn arranca() -> std::path::PathBuf {
    let p = std::env::temp_dir().join("lucy_core_upkeep_test.db");
    let _ = std::fs::remove_file(&p);
    lucy_core::init(&p).expect("init");
    lucy_core::memories::ensure_schema().expect("memorias");
    lucy_core::vectors::ensure_schema().expect("vectores");
    lucy_core::crystals::ensure_schema().expect("cristales");
    lucy_core::insights::ensure_schema().expect("patrones");
    lucy_core::docs::ensure_schema().expect("docs");
    p
}

/// Una memoria con sus etiquetas, y su vector.
fn mete(titulo: &str, tags: &str, sesion: &str) -> i64 {
    let id = lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags)
             VALUES (?1, ?2, 'contenido de prueba', ?3)",
            rusqlite::params![sesion, titulo, tags],
        )
        .map_err(|e| e.to_string())?;
        Ok(c.last_insert_rowid())
    })
    .expect("insert");
    let tipo = if sesion.starts_with("pdf:") { "pdf_chunk" } else { "memory" };
    lucy_core::vectors::upsert(
        tipo,
        &[(id.to_string(), titulo.to_string(), vec![0.5_f32, 0.5])],
        "nomic-embed-text",
    )
    .expect("vector");
    id
}

fn vectores() -> usize {
    lucy_core::with_db(|c| {
        c.query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())
    })
    .unwrap_or(0) as usize
}

#[test]
fn los_cuidados_de_la_base() {
    let ruta = arranca();

    let auto1 = mete("Turno de ayer", r#"["auto"]"#, "s1");
    let auto2 = mete("Turno de hoy", r#"["auto"]"#, "s1");
    let fijada = mete("Ventana de mantenimiento", r#"["auto"]"#, "s1");
    let vieja = mete("Lo que decíamos antes", r#"["auto"]"#, "s1");
    mete("manual — parte 1/2", r#"["documento"]"#, "pdf:7");
    mete("manual — parte 2/2", r#"["documento"]"#, "pdf:7");
    lucy_core::memories::set_pinned(fijada, true).expect("fijar");
    lucy_core::with_db(|c| {
        c.execute(
            "UPDATE agent_memories SET superseded_by = '999' WHERE id = ?1",
            rusqlite::params![vieja],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("retirar");

    // ── 1. El recuento separa lo que de verdad es distinto ──────────────────
    // «La base ocupa 7 MB» no permite decidir nada. «Casi todo son trozos de un
    // PDF que ingeriste en abril» sí.
    let r = upkeep::recuento(&ruta);
    assert_eq!(r.trozos, 2);
    assert_eq!(r.retiradas, 1);
    assert_eq!(r.fijadas, 1);
    assert_eq!(r.memorias, 3, "vivas y sin contar trozos: {r:?}");
    assert!(r.bytes > 0, "el tamaño del fichero no se leyó");
    assert_eq!(r.vectores, 6);

    // ── 2. La copia ABRE, que es lo único que importa de una copia ──────────
    //
    // Con `VACUUM INTO` y no copiando el fichero: la aplicación tiene la base
    // abierta —y la app Tauri puede tenerla abierta a la vez— así que un `copy`
    // puede llevarse un estado a medio escribir.
    let destino = std::env::temp_dir().join("lucy_backup_prueba").join("copia.db");
    let _ = std::fs::remove_file(&destino);
    let bytes = upkeep::backup(&destino).expect("copiar");
    assert!(bytes > 0);
    let copia = rusqlite::Connection::open(&destino).expect("la copia no abre");
    let n: i64 = copia
        .query_row("SELECT COUNT(*) FROM agent_memories", [], |r| r.get(0))
        .expect("la copia no tiene la tabla");
    assert_eq!(n, 6, "la copia no trae las mismas filas");
    drop(copia);

    // ── 3. Purgar las retiradas se lleva también su vector ──────────────────
    let antes = vectores();
    let n = upkeep::purga(Purga::Retiradas).expect("purga");
    assert_eq!(n, 1);
    assert_eq!(vectores(), antes - 1, "quedó un vector huérfano");
    assert_eq!(upkeep::recuento(&ruta).retiradas, 0);

    // ── 4. Una purga por lote RESPETA lo que el operador fijó ───────────────
    //
    // Fijar una memoria es decir «ésta me la quedo»; una purga que se la llevara
    // convertiría la chincheta en una promesa incumplida.
    let n = upkeep::purga(Purga::Automaticas).expect("purga");
    assert_eq!(n, 2, "debería llevarse las dos automáticas no fijadas");
    let quedan: Vec<i64> = lucy_core::with_db(|c| {
        let mut st = c
            .prepare("SELECT id FROM agent_memories WHERE session_id = 's1' ORDER BY id")
            .map_err(|e| e.to_string())?;
        let v = st
            .query_map([], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })
    .expect("select");
    assert_eq!(quedan, vec![fijada], "se llevó una fijada: {quedan:?}");
    let _ = (auto1, auto2);

    // ── 5. Lo que está sin vector se cuenta y se puede rehacer ──────────────
    //
    // El caso real: algo que se guardó con Ollama caído queda buscable solo por
    // palabras. Para un documento la salida era borrarlo y volver a ingerirlo;
    // para una memoria no había salida.
    use upkeep::Clase;
    assert_eq!(upkeep::sin_vector(Clase::Trozo), 0, "de partida están todos");
    assert_eq!(upkeep::sin_vector(Clase::Memoria), 0, "de partida están todas");
    lucy_core::with_db(|c| {
        c.execute("DELETE FROM embeddings WHERE entity_type = 'pdf_chunk'", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("quitar vectores");
    assert_eq!(upkeep::sin_vector(Clase::Trozo), 2, "no vio los trozos huérfanos");

    // LAS DOS CLASES NO SE PISAN, y ése es el riesgo de haberlas parametrizado:
    // viven en la MISMA tabla `agent_memories` y solo las separa el
    // `session_id`. Un filtro mal escrito no daría error — daría un botón que
    // ofrece rehacer los vectores de las memorias y se pone a rehacer los de los
    // PDF, o al revés.
    //
    // Aquí acaban de quedarse huérfanos los DOS trozos, y las memorias tienen su
    // vector. Si el filtro de memoria se colara al lado de los pdf, esto diría 2.
    assert_eq!(
        upkeep::sin_vector(Clase::Memoria),
        0,
        "el contador de memorias se llevó por delante los trozos"
    );

    // Y ahora al revés: se quitan los de las memorias y el de trozos no se mueve.
    lucy_core::with_db(|c| {
        c.execute("DELETE FROM embeddings WHERE entity_type = 'memory'", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("quitar vectores de memoria");
    // Queda UNA memoria viva a estas alturas: la fijada. La retirada se la llevó
    // la purga del paso 3 y las dos automáticas la del paso 4.
    assert_eq!(upkeep::sin_vector(Clase::Memoria), 1, "no vio las memorias huérfanas");
    assert_eq!(upkeep::sin_vector(Clase::Trozo), 2, "las clases se contaminaron");

    // ── 6. Purgar documentos se lleva las tres cosas ────────────────────────
    let n = upkeep::purga(Purga::Documentos).expect("purga");
    assert_eq!(n, 2);
    let r = upkeep::recuento(&ruta);
    assert_eq!(r.trozos, 0);
    assert_eq!(r.documentos, 0);
    assert_eq!(upkeep::sin_vector(Clase::Trozo), 0);

    let _ = std::fs::remove_file(&destino);
}
