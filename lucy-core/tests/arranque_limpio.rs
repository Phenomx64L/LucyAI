//! Que Lucy pueda arrancar en una máquina donde nunca ha estado.
//!
//! ES LO QUE ATABA EL SHELL NATIVO A LA APP TAURI. `init()` abría el pool sobre
//! una base que tenía que existir; sin ella, `load_memories` devolvía «DB no
//! encontrada» y el shell arrancaba sin memoria, sin forma de arreglarlo desde
//! dentro y sin decir por qué. Se podía copiar el binario a otra máquina, pero
//! no servía de nada.
//!
//! Va en `tests/` y no en el módulo porque necesita un proceso PARA ÉL SOLO: el
//! pool de `lucy-core` es un `OnceLock` global, así que una prueba que abre una
//! base vacía y otra que abre una llena no pueden convivir — la segunda se
//! encontraría el pool de la primera y pasaría por el motivo equivocado.

use std::path::PathBuf;

fn carpeta_nueva() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "lucy-limpio-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn en_una_maquina_sin_lucy_la_base_se_crea_y_se_puede_usar() {
    let dir = carpeta_nueva();
    let db = dir.join("com.lucy.dev").join("lucy.db");
    assert!(!db.exists(), "la prueba empieza sin base, o no prueba nada");

    // LA CARPETA TAMPOCO EXISTE. En una máquina limpia no hay
    // `%APPDATA%\com.lucy.dev`, y un `init` que solo abriera el fichero
    // fallaría con «unable to open database file» — que es un mensaje que no
    // dice que falta un directorio.
    lucy_core::schema::init_or_create(&db).expect("tiene que crearla");
    assert!(db.exists(), "no se creó el fichero");

    // Y SE PUEDE USAR, que es lo que de verdad se prueba. Crear el fichero y
    // que la primera consulta falle sería el mismo fallo con otra cara.
    let memorias = lucy_core::get_recent_memories(Some(10)).expect("consulta sobre base nueva");
    assert!(memorias.is_empty(), "una base recién creada no tiene memorias");

    // Una escritura y una lectura completas, tocando las columnas que la app
    // Tauri añade por migración — que son las que faltaban.
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, importance, pinned, confidence)
             VALUES ('s', 'prueba', 'contenido', '[\"t\"]', 3, 1, 0.9)",
            [],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("insertar con las columnas de la migración");

    let leidas = lucy_core::get_recent_memories(Some(10)).expect("releer");
    assert_eq!(leidas.len(), 1, "no volvió lo que se acababa de escribir");

    // Idempotente: volver a llamarlo sobre la base que acaba de crear no puede
    // romperla ni duplicar nada.
    lucy_core::schema::ensure().expect("segunda pasada");
    assert_eq!(
        lucy_core::get_recent_memories(Some(10)).map(|v| v.len()).unwrap_or(0),
        1,
        "la segunda pasada del esquema se llevó los datos por delante"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
