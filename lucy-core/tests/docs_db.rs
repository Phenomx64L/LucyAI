//! La ingesta de documentos, de punta a punta.
//!
//! UN SOLO `#[test]` CON SECCIONES, por lo mismo que `memories_db`: el pool es un
//! `OnceCell` global y las funciones de test de un fichero corren en paralelo.
//!
//! Lo que se prueba es la cadena entera y, sobre todo, el eslabón del que
//! depende que esto sirva de algo: que `pdf_search` —la herramienta, por su
//! nombre, tal y como la llamaría Lucy— encuentre lo ingerido. Y se prueba SIN
//! Ollama, que es como corre esto: si el camino por palabras no funcionara, aquí
//! no saldría ni una línea.

use std::io::Write;

fn arranca() {
    let p = std::env::temp_dir().join("lucy_core_docs_test.db");
    let _ = std::fs::remove_file(&p);
    lucy_core::init(&p).expect("init");
    // LA TABLA DE LA V2 SE CREA PRIMERO, tal cual la declara la app Tauri. Es
    // la forma que tiene la base REAL, y es exactamente lo que el test no
    // cubría: sobre base vacía el esquema propio se creaba limpio y todo
    // pasaba; contra la tabla de la app, el esquema viejo moría con «no such
    // column: sha» y la pestaña entera de Documentos con él.
    lucy_core::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS pdf_documents (
                 id           TEXT    PRIMARY KEY,
                 filename     TEXT    NOT NULL,
                 path         TEXT    NOT NULL,
                 page_count   INTEGER NOT NULL DEFAULT 0,
                 chunk_count  INTEGER NOT NULL DEFAULT 0,
                 ingested_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                 status       TEXT    NOT NULL DEFAULT 'ingesting',
                 content_hash TEXT    NOT NULL DEFAULT '',
                 synth_status TEXT    NOT NULL DEFAULT ''
             );",
        )
        .map_err(|e| e.to_string())
    })
    .expect("tabla de la app");
    lucy_core::docs::ensure_schema().expect("esquema");
}

/// Un documento de texto con contenido reconocible.
///
/// `.txt` y no `.pdf` a propósito: lo que se prueba aquí es la cadena de
/// troceado, filas, búsqueda y herramienta. Fabricar un PDF válido probaría el
/// extractor, que es otra cosa y ya tiene sus tests.
fn escribe_doc() -> std::path::PathBuf {
    let p = std::env::temp_dir().join("manual_goanywhere.txt");
    let mut f = std::fs::File::create(&p).expect("crear");
    let mut texto = String::new();
    texto.push_str(
        "Instalación del agente de GoAnywhere MFT\n\n\
         El agente se instala en el servidor destino y se registra contra el core \
         indicando la dirección del servidor y el puerto 8500.\n\n",
    );
    // Relleno para que dé más de un trozo y la búsqueda tenga que elegir.
    for i in 0..90 {
        texto.push_str(&format!(
            "Sección de relleno número {i}. Describe parámetros de configuración que no \
             tienen que ver con la instalación del agente ni con la rotación de claves.\n\n"
        ));
    }
    texto.push_str(
        "Rotación de claves PGP\n\n\
         Las claves PGP se rotan desde la consola de administración, en Seguridad, \
         generando un par nuevo y publicando la pública en los socios comerciales.\n",
    );
    f.write_all(texto.as_bytes()).expect("escribir");
    p
}

fn ingiere(ruta: &std::path::Path) -> Vec<lucy_core::docs::Paso> {
    let (tx, rx) = std::sync::mpsc::channel();
    let stop = std::sync::atomic::AtomicBool::new(false);
    lucy_core::docs::ingest(ruta, &tx, &stop);
    drop(tx);
    rx.iter().collect()
}

#[test]
fn la_cadena_entera_de_un_documento() {
    arranca();
    let ruta = escribe_doc();

    // ── 1. Ingerir ──
    let pasos = ingiere(&ruta);
    let doc = pasos
        .iter()
        .find_map(|p| match p {
            lucy_core::docs::Paso::Listo(d) => Some(d.clone()),
            // Sin Ollama, el final legítimo es «sin vectores»: el documento
            // existe y se puede buscar por palabras.
            lucy_core::docs::Paso::SinVectores(d, _) => Some(d.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("la ingesta no terminó: {pasos:?}"));

    assert!(doc.trozos > 1, "no troceó: {} trozos", doc.trozos);
    assert_eq!(doc.nombre, "manual_goanywhere.txt");
    // Sin embebedor delante, el documento NO es utilizable por significado — y
    // lo dice en vez de aparecer en la lista como si contestara a todo.
    assert!(!doc.utilizable() || doc.vectorizados == doc.trozos);

    // ── 2. Sale en la lista ──
    let lista = lucy_core::docs::list().expect("lista");
    assert_eq!(lista.len(), 1);
    assert_eq!(lista[0].id, doc.id);

    // ── 3. LA HERRAMIENTA, por su nombre ──
    //
    // ES EL ESLABÓN QUE DECIDE SI LA INGESTA SIRVE. Un manual ingerido y una
    // herramienta que Lucy no sabe llamar son lo mismo que no haberlo ingerido, y
    // no da error: da silencio. Se llama por `tools::run("pdf_search", …)`
    // exactamente como lo haría el modelo.
    let r = lucy_core::tools::run("pdf_search", "rotación de claves PGP")
        .expect("pdf_search no está en el despacho de herramientas");
    assert!(r.ok, "{}", r.body);
    assert!(
        r.body.contains("consola de administración"),
        "no encontró lo que había: {}",
        r.body
    );
    // Con su procedencia: un párrafo de manual sin decir de cuál sale se cita con
    // la misma autoridad que un hecho medido en la máquina, y no son lo mismo.
    assert!(r.body.contains("manual_goanywhere"), "sin procedencia: {}", r.body);

    // Y encuentra la OTRA sección, no siempre la misma.
    let r2 = lucy_core::tools::run("pdf_search", "instalar el agente en el servidor").unwrap();
    assert!(r2.body.contains("puerto 8500"), "{}", r2.body);

    // ── 4. Lo que no está, se dice — y con la lista de lo que sí ──
    let r3 = lucy_core::tools::run("pdf_search", "receta de tortilla de patatas").unwrap();
    assert!(!r3.ok);
    assert!(
        r3.body.contains("manual_goanywhere"),
        "no dice qué documentos hay, así que el modelo probará otras palabras: {}",
        r3.body
    );

    // ── 5. La misma ingesta dos veces NO duplica ──
    //
    // Sin esto, arrastrar dos veces el mismo manual mete cuatrocientos trozos que
    // después compiten entre sí en cada búsqueda y se llevan el sitio de todo lo
    // demás.
    let otra_ruta = std::env::temp_dir().join("copia_del_manual.txt");
    std::fs::copy(&ruta, &otra_ruta).expect("copiar");
    let pasos2 = ingiere(&otra_ruta);
    assert!(
        pasos2.iter().any(|p| matches!(p, lucy_core::docs::Paso::Listo(_))),
        "{pasos2:?}"
    );
    assert_eq!(
        lucy_core::docs::list().unwrap().len(),
        1,
        "reingirió el mismo contenido con otro nombre"
    );

    // ── 6. Borrar se lleva las tres cosas ──
    lucy_core::docs::delete(&doc.id).expect("borrar");
    assert!(lucy_core::docs::list().unwrap().is_empty());
    let huerfanos: i64 = lucy_core::with_db(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM agent_memories WHERE session_id LIKE 'pdf%'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(huerfanos, 0, "quedaron trozos sin documento, y siguen saliendo en las búsquedas");

    let _ = std::fs::remove_file(&ruta);
    let _ = std::fs::remove_file(&otra_ruta);
}
