//! El esquema COMPLETO de lo que `lucy-core` lee y escribe.
//!
//! POR QUÉ EXISTE. Hasta ahora `init()` abría el pool sobre una `lucy.db` que
//! tenía que existir ya, y su propio comentario lo decía: «la app Tauri crea el
//! esquema». Eso ataba el shell nativo a que la app Tauri se hubiera ejecutado
//! antes en esa máquina — o sea que no se podía instalar solo. Con esto, quien
//! llegue primero crea lo que hace falta.
//!
//! QUÉ CUBRE Y QUÉ NO. Cubre las dieciséis tablas que `lucy-core` toca, ni una
//! más. `src-tauri` crea otras veinte largas —grafo de conocimiento, telemetría
//! de fronteras, caché de disposición del grafo— y las crea al vuelo el módulo
//! que las necesita. No se traen aquí porque el shell nativo no tiene esas
//! funciones, y copiar esquema que nadie usa es esquema que se queda viejo sin
//! que nadie lo note.
//!
//! LAS COLUMNAS DEL `ALTER` SON LA PARTE DELICADA. `agent_memories` nace con
//! ocho columnas y la app Tauri le añade seis más con `ALTER TABLE ADD COLUMN`
//! —`pinned`, `superseded_by`, `expires_at`…— desde una migración de mayo de
//! 2026. `lucy-core` LEE esas seis. Si creara la tabla sin ellas, una base nueva
//! hecha por el shell nativo reventaría en cuanto alguien pidiera una memoria
//! fijada, y `CREATE TABLE IF NOT EXISTS` no lo arreglaría después: no corrige
//! una tabla que ya está. Por eso se crean completas de entrada, y los `ALTER`
//! quedan para las bases viejas.
//!
//! Hay un test que compara esto con lo que produce `src-tauri`. Dos esquemas
//! que se parecen son peores que dos que se diferencian: el fallo aparece al
//! insertar, meses después, en la máquina de otro.

/// Las tablas de las que `lucy-core` es dueño, con TODAS sus columnas.
///
/// `agent_memories` sale ya completa —las ocho de origen más las seis que la
/// app Tauri añade por migración— para que una base recién creada por el shell
/// nativo sea indistinguible de una creada por la app de escritorio.
const NUCLEO: &str = r#"
CREATE TABLE IF NOT EXISTS agent_memories (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id       TEXT    NOT NULL DEFAULT '',
    title            TEXT    NOT NULL,
    content          TEXT    NOT NULL,
    tags             TEXT    NOT NULL DEFAULT '[]',
    files            TEXT    NOT NULL DEFAULT '[]',
    importance       INTEGER NOT NULL DEFAULT 1,
    created_at       INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    last_accessed_at INTEGER NOT NULL DEFAULT 0,
    access_count     INTEGER NOT NULL DEFAULT 0,
    superseded_by    INTEGER NULL,
    expires_at       INTEGER NOT NULL DEFAULT 0,
    pinned           INTEGER NOT NULL DEFAULT 0,
    confidence       REAL    NOT NULL DEFAULT 0.5
);
CREATE INDEX IF NOT EXISTS idx_agent_memories_created
    ON agent_memories(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_memories_importance
    ON agent_memories(importance DESC);

-- La memoria NÚCLEO: lo que entra en todos los prompts.
CREATE TABLE IF NOT EXISTS memory_core (
    id         TEXT PRIMARY KEY,
    section    TEXT NOT NULL,
    key        TEXT NOT NULL,
    value      TEXT NOT NULL,
    pinned     INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    confidence REAL    NOT NULL DEFAULT 0.5
);
CREATE INDEX IF NOT EXISTS idx_memory_core_section ON memory_core(section);

-- Lo que Lucy sabe del operador: nombre, preferencias, contexto.
CREATE TABLE IF NOT EXISTS user_profile (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    category   TEXT NOT NULL DEFAULT 'general',
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
"#;

/// Las columnas que la app Tauri añadió por migración.
///
/// SOLO PARA BASES VIEJAS. Una base creada por `NUCLEO` ya las tiene; esto es
/// para las que se crearon antes de mayo de 2026 con la tabla corta. SQLite no
/// tiene `ADD COLUMN IF NOT EXISTS`, así que se intenta y se ignora el error de
/// columna repetida — que es exactamente lo que hace `metrics::init()` en la
/// app Tauri.
const MIGRACIONES: &[(&str, &str)] = &[
    ("agent_memories", "last_accessed_at INTEGER NOT NULL DEFAULT 0"),
    ("agent_memories", "access_count     INTEGER NOT NULL DEFAULT 0"),
    ("agent_memories", "superseded_by    INTEGER NULL"),
    ("agent_memories", "expires_at       INTEGER NOT NULL DEFAULT 0"),
    ("agent_memories", "pinned           INTEGER NOT NULL DEFAULT 0"),
    ("agent_memories", "confidence       REAL    NOT NULL DEFAULT 0.5"),
    ("memory_core", "confidence REAL NOT NULL DEFAULT 0.5"),
];

/// Deja la base lista para todo lo que `lucy-core` sabe hacer. Idempotente.
///
/// Se puede llamar sobre una base que creó la app Tauri: todo va con
/// `IF NOT EXISTS` y los `ALTER` se tragan el error de columna repetida.
pub fn ensure() -> Result<(), String> {
    crate::with_db(|c| c.execute_batch(NUCLEO).map_err(|e| e.to_string()))?;

    for (tabla, columna) in MIGRACIONES {
        // El error se descarta A PROPÓSITO y solo aquí: en una base al día,
        // «duplicate column name» es el resultado ESPERADO, no un fallo. Se
        // mira el mensaje en vez de tragarse todo para que un error de verdad
        // —la tabla no existe, el fichero es de solo lectura— siga saliendo.
        let r = crate::with_db(|c| {
            c.execute(&format!("ALTER TABLE {tabla} ADD COLUMN {columna}"), [])
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
        if let Err(e) = r {
            if !e.to_lowercase().contains("duplicate column") {
                return Err(format!("migrando {tabla}: {e}"));
            }
        }
    }

    // Y lo que cada módulo sabe crearse. Se llaman todos aquí para que exista
    // UN sitio del que tirar: repartidos, cada pantalla creaba lo suyo la
    // primera vez que se abría, y una que no se abriera nunca dejaba su tabla
    // sin existir hasta que alguien la pedía.
    crate::memories::ensure_schema()?;
    crate::audit::ensure_schema()?;
    crate::crystals::ensure_schema()?;
    crate::insights::ensure_schema()?;
    crate::principles::ensure_schema()?;
    crate::vectors::ensure_schema()?;
    crate::docs::ensure_schema()?;
    crate::drift::ensure_schema()?;
    crate::posture::ensure_schema()?;
    crate::maintenance::ensure_schema()?;
    crate::history::ensure_schema()?;
    crate::thresholds::ensure_schema()?;
    Ok(())
}

/// Abre la base —creándola si no está— y deja el esquema listo.
///
/// Es lo que llama un shell que no depende de la app Tauri. `init()` sigue
/// existiendo y sigue esperando una base hecha: quien ya la tiene no necesita
/// pagar el `ensure` en cada arranque.
pub fn init_or_create(path: &std::path::Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("creando {}: {e}", dir.display()))?;
    }
    crate::init(path)?;
    ensure()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Las columnas de una tabla, tal y como quedaron en la base.
    fn columnas(tabla: &str) -> Vec<String> {
        crate::with_db(|c| {
            let mut st = c
                .prepare(&format!("PRAGMA table_info({tabla})"))
                .map_err(|e| e.to_string())?;
            let v = st
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .collect();
            Ok(v)
        })
        .unwrap_or_default()
    }

    #[test]
    fn una_base_recien_creada_tiene_las_columnas_que_lucy_core_lee() {
        // EL FALLO QUE ESTO CIERRA, y es el que impedía instalar el shell
        // nativo solo: `lucy-core` lee `pinned`, `superseded_by`, `expires_at`,
        // `confidence`, `access_count` y `last_accessed_at`, pero su
        // `CREATE TABLE` creaba la tabla de ocho columnas. En una base nueva,
        // la primera consulta que pidiera una memoria fijada fallaba.
        //
        // Y no se arreglaba solo: `CREATE TABLE IF NOT EXISTS` no toca una
        // tabla que ya está, así que la base quedaba coja para siempre.
        let dir = std::env::temp_dir().join(format!("lucy-schema-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        init_or_create(&dir.join("lucy.db")).expect("tiene que poder crearla");

        let cols = columnas("agent_memories");
        for c in [
            "id", "session_id", "title", "content", "tags", "files", "importance",
            "created_at", "last_accessed_at", "access_count", "superseded_by",
            "expires_at", "pinned", "confidence",
        ] {
            assert!(cols.iter().any(|x| x == c), "falta {c} en agent_memories: {cols:?}");
        }
        assert!(columnas("memory_core").iter().any(|c| c == "confidence"));
        assert!(columnas("user_profile").iter().any(|c| c == "category"));
    }

    #[test]
    fn el_esquema_del_nucleo_no_se_ha_separado_del_de_la_app_tauri() {
        // DOS ESQUEMAS QUE SE PARECEN son peores que dos que se diferencian: la
        // base abre con los dos y el fallo sale al insertar, meses después, en
        // la máquina de otro. Esto lee el `INIT_SQL` de `src-tauri` y compara
        // las columnas de `agent_memories`, que es la tabla compartida que más
        // se toca.
        //
        // Se lee del fuente y no de una copia, por lo mismo: una copia es otro
        // sitio del que separarse.
        let ruta = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../lucy-svelte/src-tauri/src/utils/db.rs");
        let Ok(fuente) = std::fs::read_to_string(&ruta) else {
            // En un consumidor de `lucy-core` que no tenga `src-tauri` al lado,
            // esto no aplica. No se falla: se salta.
            return;
        };

        let de_tauri = columnas_declaradas(&fuente, "agent_memories");
        let de_core = columnas_declaradas(NUCLEO, "agent_memories");

        // SUELO. Si el rascador deja de encontrar columnas —porque cambió el
        // formato del `CREATE TABLE`, o la ruta, o el nombre— este test pasaría
        // siempre comparando dos listas vacías. Un test que se apaga solo es
        // peor que no tenerlo: ocupa el sitio del que sí miraría.
        assert!(
            de_tauri.len() >= 8,
            "el rascador solo vio {} columnas en src-tauri: ya no está leyendo el esquema",
            de_tauri.len()
        );
        assert!(de_core.len() >= 14, "el núcleo declara {} columnas", de_core.len());

        for c in &de_tauri {
            assert!(
                de_core.contains(c),
                "`{c}` está en el esquema de src-tauri y no en el del núcleo"
            );
        }
        // Y las que añade por migración.
        for (tabla, columna) in MIGRACIONES {
            if *tabla != "agent_memories" {
                continue;
            }
            let nombre = columna.split_whitespace().next().unwrap();
            assert!(
                de_core.contains(&nombre.to_string()),
                "`{nombre}` se migra pero no se crea de entrada"
            );
        }
    }

    /// Los nombres de columna dentro del primer `CREATE TABLE <tabla>` del texto.
    fn columnas_declaradas(sql: &str, tabla: &str) -> Vec<String> {
        let Some(i) = sql.find(&format!("CREATE TABLE IF NOT EXISTS {tabla} (")) else {
            return Vec::new();
        };
        let cuerpo = &sql[i..];
        let Some(fin) = cuerpo.find(");") else { return Vec::new() };
        cuerpo[..fin]
            .lines()
            .skip(1)
            .filter_map(|l| {
                let t = l.trim();
                if t.is_empty() || t.starts_with("--") {
                    return None;
                }
                t.split_whitespace().next().map(|s| s.trim_matches(',').to_string())
            })
            .collect()
    }
}
