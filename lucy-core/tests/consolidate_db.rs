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

/// El trozo del esquema real que hace falta aquí. Copiado de `utils/db.rs`,
/// incluidas las columnas que se añaden por `ALTER TABLE` en la app.
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
    superseded_by TEXT
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

    assert_eq!(visibles(), vec![a, b, c, otra], "de partida se ven las cuatro");

    let r = lucy_core::consolidate::run(false).expect("consolidar");
    assert!(r.memories_merged > 0, "no fundió nada: {r:?}");

    // EL EFECTO, que es lo que no se probaba. La marca se escribía solo dentro
    // del JSON de `tags` mientras todas las lecturas filtran por la COLUMNA, así
    // que la consolidación informaba de haber fundido catorce memorias sobre una
    // tabla que seguía devolviéndolas todas.
    let quedan = visibles();
    assert!(
        quedan.len() < 4,
        "consolidar dijo que fundió {} y siguen viéndose las cuatro: {quedan:?}",
        r.memories_merged
    );
    assert!(quedan.contains(&otra), "se llevó por delante una memoria que no tocaba");

    // La canónica sigue viva y las fundidas apuntan a ella.
    let canonica = *quedan.first().expect("queda al menos una");
    let apuntan: Vec<(i64, Option<String>)> = lucy_core::with_db(|c| {
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
        assert_eq!(
            hacia.as_deref(),
            Some(canonica.to_string().as_str()),
            "la memoria {id} apunta a otra cosa"
        );
    }

    // Y NO SE BORRA NADA: las cuatro filas siguen ahí. Una memoria fundida es
    // parte del rastro de por qué la canónica existe, y borrarla haría que un
    // «esto ya lo sabías» no se pudiera comprobar nunca.
    let total: i64 = lucy_core::with_db(|c| {
        c.query_row("SELECT COUNT(*) FROM agent_memories", [], |r| r.get(0))
            .map_err(|e| e.to_string())
    })
    .expect("count");
    assert_eq!(total, 4, "consolidar borró filas en vez de marcarlas");

    // Correr otra vez no encuentra nada nuevo: lo ya marcado queda fuera del
    // barrido. Sin esto, cada pasada volvería a «fundir» lo mismo y el informe
    // contaría un trabajo que no hizo.
    let segunda = lucy_core::consolidate::run(false).expect("segunda pasada");
    assert_eq!(segunda.memories_merged, 0, "vuelve a fundir lo ya fundido: {segunda:?}");
}
