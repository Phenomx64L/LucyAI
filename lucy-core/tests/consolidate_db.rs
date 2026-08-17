//! La consolidación, contra una base de datos de verdad.
//!
//! EN `tests/` Y NO EN EL MÓDULO, y por una razón concreta: `lucy_core::init`
//! guarda el pool en un `OnceCell` global, así que el primer test que lo inicie
//! decide la base de todos los demás. Un test unitario que abriera su propia
//! `lucy.db` haría que el siguiente en ejecutarse trabajara contra ella sin
//! saberlo, y el fallo aparecería solo al cambiar el orden. Cada fichero de
//! `tests/` corre en SU PROPIO PROCESO, que es exactamente lo que hace falta.
//!
//! Lo que se prueba aquí no se podía probar sin base: que consolidar tenga
//! EFECTO. Los tests del módulo comprueban el agrupamiento —qué se parece a
//! qué— y pasaban en verde mientras la marca se escribía en un sitio que no
//! miraba ninguna lectura.

use std::path::PathBuf;

fn base() -> PathBuf {
    let p = std::env::temp_dir().join("lucy_core_consolidate_test.db");
    let _ = std::fs::remove_file(&p);
    p
}

/// El trozo del esquema real que hace falta aquí — COPIADO DE LA BASE DE VERDAD,
/// no inventado: los tipos salen de un `PRAGMA table_info` sobre la lucy.db real,
/// incluidas las columnas que la app añade por `ALTER TABLE`.
///
/// El tipo de `superseded_by` importa y estaba mal aquí: TEXT en el test,
/// INTEGER en la base. Un test que fabrica un esquema más cómodo que el real es
/// exactamente lo que dejó pasar el «no such column: sha» de los documentos.
const DDL: &str = "
CREATE TABLE IF NOT EXISTS agent_memories (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT    NOT NULL DEFAULT '',
    title      TEXT    NOT NULL,
    content    TEXT    NOT NULL,
    tags       TEXT    NOT NULL DEFAULT '[]',
    files      TEXT    NOT NULL DEFAULT '[]',
    importance INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    last_accessed_at INTEGER,
    access_count INTEGER NOT NULL DEFAULT 0,
    superseded_by INTEGER,
    expires_at INTEGER NOT NULL DEFAULT 0,
    pinned INTEGER NOT NULL DEFAULT 0,
    confidence REAL NOT NULL DEFAULT 0.5
);";

fn mete(titulo: &str, texto: &str, tags: &str, importancia: i64) -> i64 {
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, importance)
             VALUES ('s1', ?1, ?2, ?3, ?4)",
            rusqlite::params![titulo, texto, tags, importancia],
        )
        .map_err(|e| e.to_string())?;
        Ok(c.last_insert_rowid())
    })
    .expect("insert")
}

/// Las memorias que un consumidor REAL vería. Es la misma condición que usa
/// `lucy_core::get_recent_memories` y la consulta de la app: por la COLUMNA.
fn visibles() -> Vec<i64> {
    lucy_core::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT id FROM agent_memories
                 WHERE (superseded_by IS NULL OR superseded_by = '')
                 ORDER BY id",
            )
            .map_err(|e| e.to_string())?;
        let v = st
            .query_map([], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(v)
    })
    .expect("select")
}

fn mete_pdf(titulo: &str, texto: &str) -> i64 {
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, importance)
             VALUES ('pdf:7', ?1, ?2, '[\"documento\"]', 1)",
            rusqlite::params![titulo, texto],
        )
        .map_err(|e| e.to_string())?;
        Ok(c.last_insert_rowid())
    })
    .expect("insert pdf")
}

fn vector_de(id: i64) {
    lucy_core::vectors::ensure_schema().expect("esquema vectores");
    lucy_core::vectors::upsert(
        "memory",
        &[(id.to_string(), format!("texto de {id}"), vec![0.5_f32, 0.5])],
        "nomic-embed-text",
    )
    .expect("upsert");
}

fn hay_vector(id: i64) -> bool {
    lucy_core::with_db(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM embeddings WHERE entity_type='memory' AND entity_id=?1",
            rusqlite::params![id.to_string()],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())
    })
    .map(|n| n > 0)
    .unwrap_or(false)
}

#[test]
fn consolidar_saca_de_la_vista_las_memorias_fundidas() {
    lucy_core::init(&base()).expect("init");
    lucy_core::with_db(|c| c.execute_batch(DDL).map_err(|e| e.to_string())).expect("ddl");

    // Tres que dicen lo mismo con otras palabras, y una que no tiene que ver.
    // El agrupamiento ya está probado en el módulo; aquí lo que se comprueba es
    // qué le pasa a la tabla.
    let a = mete(
        "Reiniciar el servicio de impresión",
        "Para arreglar la cola de impresión hay que reiniciar el servicio spooler en el servidor",
        r#"["impresion","spooler"]"#,
        3,
    );
    let b = mete(
        "Cola de impresión atascada",
        "Cuando la cola de impresión se atasca se reinicia el servicio spooler del servidor",
        r#"["impresion","spooler"]"#,
        2,
    );
    let c = mete(
        "Arreglar impresión reiniciando spooler",
        "El servicio spooler del servidor se reinicia para arreglar la cola de impresión atascada",
        r#"["impresion","spooler"]"#,
        1,
    );
    let otra = mete(
        "Rotar el certificado de la VPN",
        "El certificado de la VPN se renueva desde la consola de la autoridad certificadora",
        r#"["vpn","certificados"]"#,
        3,
    );
    // Cada una con su vector, como los deja la app cuando embebe al guardar.
    for id in [a, b, c, otra] {
        vector_de(id);
    }

    // UNA RETIRADA AL ESTILO TAURI: columna escrita, etiqueta intacta. Dice lo
    // mismo que las tres del spooler, así que con el filtro viejo —que miraba
    // `tags`— volvía a entrar como candidata y podía salir elegida canónica:
    // memorias vivas fundidas hacia un id que ningún lector enseña.
    let muerta = mete(
        "Impresión: reiniciar spooler",
        "La cola de impresión atascada se arregla reiniciando el servicio spooler del servidor",
        r#"["impresion","spooler"]"#,
        2,
    );
    lucy_core::with_db(|con| {
        con.execute(
            "UPDATE agent_memories SET superseded_by = '999' WHERE id = ?1",
            rusqlite::params![muerta],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("retirar");

    // Y TROZOS DE DOCUMENTO REPETIDOS. Un manual trae párrafos casi idénticos
    // (cabeceras, avisos legales); sin la exclusión, la pasada desatendida los
    // fundía entre sí — es decir, agujereaba el documento.
    let p1 = mete_pdf("manual — parte 1/3", "Aviso legal: este documento es confidencial y propiedad de la empresa");
    let p2 = mete_pdf("manual — parte 2/3", "Aviso legal: este documento es confidencial y propiedad de la empresa SA");

    assert_eq!(
        visibles(),
        vec![a, b, c, otra, p1, p2],
        "de partida se ven las cuatro más los trozos"
    );
    let creado_a: i64 = lucy_core::with_db(|con| {
        con.query_row("SELECT created_at FROM agent_memories WHERE id=?1", [a], |r| r.get(0))
            .map_err(|e| e.to_string())
    })
    .expect("created_at");

    let r = lucy_core::consolidate::run(false).expect("consolidar");
    assert!(r.memories_merged > 0, "no fundió nada: {r:?}");

    // EL EFECTO, que es lo que no se probaba. La marca se escribía solo dentro
    // del JSON de `tags` mientras todas las lecturas filtran por la COLUMNA, así
    // que la consolidación informaba de haber fundido catorce memorias sobre una
    // tabla que seguía devolviéndolas todas.
    let quedan = visibles();
    assert!(
        quedan.iter().filter(|id| [a, b, c].contains(id)).count() < 3,
        "consolidar dijo que fundió {} y siguen viéndose las tres: {quedan:?}",
        r.memories_merged
    );
    assert!(quedan.contains(&otra), "se llevó por delante una memoria que no tocaba");
    // Los trozos de documento no se tocan: fundirlos es agujerear el manual.
    assert!(quedan.contains(&p1) && quedan.contains(&p2), "fundió trozos de documento: {quedan:?}");
    // Y la retirada al estilo Tauri no fue elegida canónica de nadie: nada
    // apunta hacia ella.
    let hacia_muerta: i64 = lucy_core::with_db(|con| {
        con.query_row(
            "SELECT COUNT(*) FROM agent_memories WHERE superseded_by = ?1",
            rusqlite::params![muerta.to_string()],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .expect("count");
    assert_eq!(hacia_muerta, 0, "una fila retirada por la app salió elegida canónica");
    // Su puntero tampoco se pisó: sigue apuntando a donde la app la mandó.
    let puntero: i64 = lucy_core::with_db(|con| {
        con.query_row(
            "SELECT superseded_by FROM agent_memories WHERE id = ?1",
            rusqlite::params![muerta],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .expect("puntero");
    assert_eq!(puntero, 999, "consolidar pisó un puntero que no era suyo");

    // La canónica sigue viva y las fundidas apuntan a ella.
    let canonica = *quedan.first().expect("queda al menos una");
    let apuntan: Vec<(i64, Option<i64>)> = lucy_core::with_db(|c| {
        let mut st = c
            .prepare("SELECT id, superseded_by FROM agent_memories WHERE superseded_by IS NOT NULL")
            .map_err(|e| e.to_string())?;
        let v = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(v)
    })
    .expect("select");
    assert!(!apuntan.is_empty());
    for (id, hacia) in &apuntan {
        // `muerta` la retiró «la app» hacia 999 y ese puntero es suyo.
        if *id == muerta {
            continue;
        }
        assert_eq!(*hacia, Some(canonica), "la memoria {id} apunta a otra cosa");
    }

    // Y NO SE BORRA NINGUNA FILA: siete entraron, siete están. Una memoria
    // fundida es parte del rastro de por qué la canónica existe.
    let total: i64 = lucy_core::with_db(|c| {
        c.query_row("SELECT COUNT(*) FROM agent_memories", [], |r| r.get(0))
            .map_err(|e| e.to_string())
    })
    .expect("count");
    assert_eq!(total, 7, "consolidar borró filas en vez de marcarlas");

    // PERO SUS VECTORES SÍ. La búsqueda semántica lee `embeddings` sin join:
    // dejando el vector, la fila retirada seguía saliendo en el recuerdo y la
    // deduplicación por coseno podía descartar un hecho nuevo en su favor.
    for id in [a, b, c] {
        let fundida = !visibles().contains(&id);
        assert_eq!(
            hay_vector(id),
            !fundida,
            "la memoria {id} (fundida={fundida}) tiene el vector al revés"
        );
    }
    assert!(hay_vector(otra), "se llevó el vector de una memoria viva");

    // `viva` es el contraste que usan el recuerdo y la deduplicación: las
    // fundidas dejan de estarlo, la canónica y la ajena no.
    for id in [a, b, c, otra] {
        assert_eq!(lucy_core::memories::viva(id), visibles().contains(&id), "viva({id})");
    }
    assert!(!lucy_core::memories::viva(muerta), "una retirada por la app cuenta como viva");

    // La canónica NO se re-fecha. Se re-fechaba a «ahora», y eso devolvía a la
    // memoria más corroborada al fondo de la cola de los insights —que solo
    // miran filas con más de cinco días— cada vez que se consolidaba.
    let canonica_id = *quedan
        .iter()
        .find(|id| [a, b, c].contains(id))
        .expect("una del spooler sigue viva");
    let creado_ahora: i64 = lucy_core::with_db(|con| {
        con.query_row(
            "SELECT created_at FROM agent_memories WHERE id=?1",
            rusqlite::params![canonica_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .expect("created_at");
    if canonica_id == a {
        assert_eq!(creado_ahora, creado_a, "consolidar re-fechó la canónica");
    }

    // Correr otra vez no encuentra nada nuevo: lo ya marcado queda fuera del
    // barrido. Sin esto, cada pasada volvería a «fundir» lo mismo y el informe
    // contaría un trabajo que no hizo.
    let segunda = lucy_core::consolidate::run(false).expect("segunda pasada");
    assert_eq!(segunda.memories_merged, 0, "vuelve a fundir lo ya fundido: {segunda:?}");
}
