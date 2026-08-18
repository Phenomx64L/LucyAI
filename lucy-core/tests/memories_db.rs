//! Guardar memorias, contra una base de datos de verdad.
//!
//! En `tests/` por lo mismo que `consolidate_db`: el pool de `lucy_core` es un
//! `OnceCell` global y cada fichero de aquí corre en su propio proceso.
//!
//! Lo que se prueba es lo que los tests unitarios NO pueden ver: que el esquema
//! se cree solo, que la fila llegue al disco, y sobre todo que el duplicado NO
//! llegue. El dique contra la basura es la escritura, y un dique solo se puede
//! probar mojándolo.

//! UN SOLO `#[test]` CON SECCIONES, y no cuatro. Las funciones de test de un
//! mismo fichero corren EN PARALELO: cuatro hilos llamando a `lucy_core::init`
//! se pelean por el `OnceCell` —tres se llevan «pool ya inicializado»— y, aunque
//! no se pelearan, compartirían la tabla y el recuento de uno dependería de si
//! el otro ya había escrito. Un escenario secuencial sobre una base limpia dice
//! lo mismo y no depende del orden.

use lucy_core::memories::{save, Accion, New};

fn arranca() {
    let p = std::env::temp_dir().join("lucy_core_memories_test.db");
    let _ = std::fs::remove_file(&p);
    lucy_core::init(&p).expect("init");
}

/// Deja la tabla vacía entre secciones — y la de vectores también: en una
/// máquina con Ollama corriendo, `save` los escribe, y un vector huérfano de una
/// sección anterior contaminaría la deduplicación de la siguiente.
fn limpia() {
    lucy_core::memories::ensure_schema().expect("esquema");
    lucy_core::vectors::ensure_schema().expect("esquema vectores");
    lucy_core::with_db(|c| {
        c.execute("DELETE FROM agent_memories", []).map_err(|e| e.to_string())?;
        c.execute("DELETE FROM embeddings", []).map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("limpiar");
}

fn cuantas() -> i64 {
    lucy_core::with_db(|c| {
        c.query_row("SELECT COUNT(*) FROM agent_memories", [], |r| r.get(0))
            .map_err(|e| e.to_string())
    })
    .expect("count")
}

fn fila(id: i64) -> (String, String, String, i64) {
    lucy_core::with_db(|c| {
        c.query_row(
            "SELECT title, content, tags, importance FROM agent_memories WHERE id = ?1",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| e.to_string())
    })
    .expect("row")
}

#[test]
fn el_camino_de_escritura_entero() {
    arranca();

    // ── 1. Guardar crea el esquema y deduplica en la puerta ──
    limpia();

    // EL ESQUEMA SE CREA SOLO. `lucy_core::init` abre una lucy.db existente y el
    // esquema lo pone la app Tauri; en una máquina donde la Tauri no haya
    // arrancado nunca, guardar daría «no such table».
    let a = save(&New::nueva(
        "Reiniciar el servicio de impresión",
        "Cuando la cola de impresión se atasca hay que reiniciar el servicio Spooler en el servidor de impresión",
    )
    .con_tags(&["impresion", "spooler"])
    .importancia(2))
    .expect("primera");
    assert!(a.es_nueva(), "{:?}", a.accion);
    assert_eq!(cuantas(), 1);

    let (t, c, tags, imp) = fila(a.id);
    assert_eq!(t, "Reiniciar el servicio de impresión");
    assert!(c.contains("Spooler"));
    assert!(tags.contains("spooler"), "las etiquetas no llegaron: {tags}");
    assert_eq!(imp, 2);

    // EL DUPLICADO NO ENTRA. Es el dique: fundir después es la limpieza, pero lo
    // que decide si la tabla se llena de veinte formas de la misma frase es qué
    // se acepta al escribir.
    //
    // Lo que la etapa 1 caza es la REFORMULACIÓN: las mismas palabras en otro
    // orden, que es lo que sale cuando el modelo vuelve a contar un hecho que ya
    // contó.
    let b = save(&New::nueva(
        "Reiniciar el servicio de impresión",
        "El servicio Spooler del servidor de impresión hay que reiniciarlo cuando la cola de impresión se atasca",
    )
    .con_tags(&["impresion", "spooler"]))
    .expect("segunda");
    match &b.accion {
        Accion::Duplicada { motivo } => {
            assert_eq!(b.id, a.id, "apunta a otra memoria");
            assert!(motivo.contains("parecido"), "{motivo}");
        }
        otra => panic!("se guardó por duplicado: {otra:?}"),
    }
    assert_eq!(cuantas(), 1, "entró la duplicada");

    // LO QUE NINGUNA PASADA DE TEXTO CAZA, Y SIN RED DEBAJO.
    //
    // Una paráfrasis con otra morfología —«atasca»/«atascada»,
    // «reiniciar»/«reinicia»— comparte menos palabras de las que parece: medido,
    // 0,333 contra el umbral de 0,35. Un conjunto de palabras no une dos formas
    // del mismo verbo, y en español eso pasa en casi cada frase.
    //
    // Y la consolidación TAMPOCO la funde, porque usa exactamente el mismo
    // criterio: es la misma función. Así que no hay red de seguridad — lo único
    // que caza esto es la etapa 2, que compara significados y necesita el
    // servicio de embeddings.
    //
    // CONSECUENCIA QUE HAY QUE SABER: con Ollama apagado, las paráfrasis se
    // acumulan PARA SIEMPRE. No es una degradación temporal que se arregle
    // cuando el servicio vuelva; esas filas ya están dentro y ninguna pasada
    // posterior las mira.
    //
    // Este test fija las dos mitades para que nadie «arregle» el umbral: es el
    // mismo que usa `consolidate`, y bajarlo la haría fundir cosas distintas.
    let parafrasis = save(&New::nueva(
        "Cola de impresión atascada",
        "Para arreglar la cola de impresión atascada se reinicia el servicio Spooler del servidor de impresión",
    )
    .con_tags(&["impresion", "spooler"]))
    .expect("paráfrasis");
    if parafrasis.es_nueva() {
        // Sin servicio de embeddings: la paráfrasis entra, y es la consecuencia
        // documentada arriba.
        assert_eq!(cuantas(), 2, "la paráfrasis entró, que es lo esperado sin etapa 2");
        let r = lucy_core::consolidate::run(false).expect("consolidar");
        assert_eq!(
            r.memories_merged, 0,
            "la consolidación la fundió — si esto cambia, el umbral se ha tocado y hay \
             que revisar qué MÁS está fundiendo ahora"
        );
    } else {
        // CON Ollama corriendo, la etapa 2 la caza — y que esta rama sea
        // alcanzable es la prueba de que `save` escribe vectores: antes no los
        // escribía nadie, así que la etapa 2 comparaba contra una tabla vacía y
        // esta rama era código muerto en CUALQUIER máquina, con el síntoma de
        // que las paráfrasis se acumulaban igual con el servicio levantado.
        match &parafrasis.accion {
            Accion::Duplicada { motivo } => {
                assert!(motivo.contains("dice lo mismo"), "motivo raro: {motivo}")
            }
            otra => panic!("no era nueva pero tampoco duplicada: {otra:?}"),
        }
        assert_eq!(cuantas(), 1, "la duplicada por coseno no debe dejar fila");
    }

    // Y algo distinto SÍ entra, corra lo que corra en esta máquina. Sin esto, un
    // deduplicador que rechace todo pasaría este test igual de bien.
    let antes = cuantas();
    let c = save(&New::nueva(
        "Rotar el certificado de la VPN",
        "El certificado de la VPN se renueva desde la consola de la autoridad certificadora interna",
    )
    .con_tags(&["vpn", "certificados"]))
    .expect("tercera");
    assert!(c.es_nueva(), "rechazó una memoria que no se parece: {:?}", c.accion);
    assert_eq!(cuantas(), antes + 1);

    // ── 2. Un secreto no llega al disco, ni por las etiquetas ──
    limpia();
    let g = save(&New::nueva(
        "Acceso al servidor de base de datos",
        "Conectar con postgres://admin:Passw0rd123@db.local/prod usando DB_PASSWORD=Passw0rd123",
    )
    .con_tags(&["db", "token=ghp_abcdefghijklmnopqrstuvwxyz01"]))
    .expect("guardar");

    let (_, contenido, tags, _) = fila(g.id);
    assert!(!contenido.contains("Passw0rd123"), "la clave llegó al disco: {contenido}");
    assert!(contenido.contains("REDACTADO"), "{contenido}");
    // LAS ETIQUETAS TAMBIÉN. En la V2 eran, con los ficheros, los dos únicos
    // campos que se guardaban sin limpiar — y el modelo las controla igual.
    assert!(!tags.contains("ghp_abcdefghij"), "el token entró por la etiqueta: {tags}");

    // ── 3. Una memoria no se deduplica contra un trozo de PDF ──
    limpia();
    // Los trozos de un documento viven en ESTA misma tabla y en su índice de
    // texto. Sin excluirlos, una memoria escrita sobre un tema del que hay un
    // manual ingerido casa con un trozo del manual, se declara duplicada y NO SE
    // GUARDA NUNCA — «te lo guardé» sobre algo que no está. Pasó en producción.
    lucy_core::memories::ensure_schema().expect("esquema");
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, files, importance)
             VALUES ('pdf:7', 'GoAnywhere MFT — Resumen Técnico', 'El agente de GoAnywhere MFT se instala en el servidor y se registra contra el core', '[]', '[]', 1)",
            [],
        )
        .map_err(|e| e.to_string())
    })
    .expect("trozo");

    let g = save(&New::nueva(
        "GoAnywhere MFT — Resumen Técnico",
        "El agente de GoAnywhere MFT se instala en el servidor y se registra contra el core",
    ))
    .expect("guardar");
    assert!(
        g.es_nueva(),
        "se dedujo contra un trozo de PDF y la memoria no existe: {:?}",
        g.accion
    );

    // ── 4. Sin cuerpo se rechaza en vez de guardar una fila vacía ──
    limpia();
    assert!(save(&New::nueva("  ", "algo")).is_err());
    assert!(save(&New::nueva("algo", "   ")).is_err());
    assert_eq!(cuantas(), 0);

    // ── 5. El recuerdo funciona SIN servicio de embeddings ──
    //
    // Es la mitad que faltaba: la versión anterior buscaba solo con vectores y
    // devolvía vacío si el embebedor no contestaba. En una máquina sin Ollama
    // —o con Ollama caído— Lucy no recordaba NADA nunca, y el síntoma era
    // simplemente que parecía tener mala memoria.
    //
    // Este test corre SIN Ollama: si el respaldo léxico no funcionara, aquí no
    // saldría ni una línea.
    limpia();
    save(&New::nueva(
        "Certificado de la VPN",
        "El certificado de la VPN se renueva desde la consola de la autoridad certificadora interna cada doce meses",
    )
    .con_tags(&["vpn"]))
    .expect("vpn");
    save(&New::nueva(
        "Servidor de impresión",
        "El servicio Spooler del servidor de impresión se reinicia cuando la cola se atasca",
    )
    .con_tags(&["impresion"]))
    .expect("impresion");
    // SE BORRAN LOS VECTORES A PROPÓSITO: esta sección prueba el RESPALDO, y en
    // una máquina con Ollama corriendo el guardado acaba de embeber las dos
    // filas — el recuerdo iría por significado y `lexico` saldría falso. El
    // respaldo tiene que probarse igual corra lo que corra en esta máquina.
    lucy_core::vectors::ensure_schema().expect("esquema vectores");
    lucy_core::with_db(|c| {
        c.execute("DELETE FROM embeddings", []).map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("sin vectores");

    let r = lucy_core::memories::recall("cómo renuevo el certificado de la VPN", 5);
    assert!(!r.is_empty(), "no recordó nada sin embeddings: {r:?}");
    assert!(r.lexico, "no marcó que iba con el respaldo léxico: {r:?}");
    assert!(
        r.bloque.contains("autoridad certificadora"),
        "recordó lo que no era: {}",
        r.bloque
    );
    // Y NO se trae la otra: encontrar poco es mejor que no encontrar nada, pero
    // traerlo todo sería peor que las dos cosas.
    assert!(!r.bloque.contains("Spooler"), "se trajo media tabla: {}", r.bloque);

    // Cada memoria en UNA línea. Con saltos dentro, la lista del prompt se parte
    // en viñetas falsas y el modelo lee media memoria como un elemento aparte.
    assert_eq!(
        r.bloque.lines().count(),
        r.memorias,
        "las líneas no cuadran con las memorias: {}",
        r.bloque
    );
    for l in r.bloque.lines() {
        assert!(l.starts_with("- "), "línea sin viñeta: {l}");
    }

    // ── 6. El respaldo léxico no confunde un manual con la memoria ──
    //
    // Corre justo cuando Ollama no está, que es cuando no hay ranking semántico
    // que mantenga a raya los trozos: tras ingerir un manual, sus cuatrocientos
    // párrafos ganan cualquier búsqueda por palabras. Un trozo que casa con la
    // consulta NO debe salir como si fuera algo que Lucy aprendió.
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, importance)
             VALUES ('pdf:9', 'manual — parte 4/40',
                     'El certificado de la VPN según el fabricante se renueva desde el panel del appliance',
                     '[\"documento\"]', 1)",
            [],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("trozo");
    let r = lucy_core::memories::recall("cómo renuevo el certificado de la VPN", 5);
    assert!(
        !r.bloque.contains("appliance"),
        "un trozo de manual salió disfrazado de memoria: {}",
        r.bloque
    );
    assert!(r.bloque.contains("autoridad certificadora"), "y la memoria de verdad sí: {}", r.bloque);

    // ── 7. Una memoria FIJADA entra aunque no se parezca a la pregunta ──
    //
    // ES LO QUE SIGNIFICA FIJARLA. Sin esto la chincheta sería decorativa:
    // entraría solo cuando se pareciera a la pregunta, o sea cuando ya no hacía
    // falta acordarse de ella. Es el mismo razonamiento que separa un principio
    // de una memoria, aplicado a lo que el operador señaló a mano.
    limpia();
    let g = save(&New::nueva(
        "Ventana de mantenimiento",
        "Los reinicios de producción solo se hacen los domingos entre las 2 y las 5 de la mañana",
    ))
    .expect("guardar");
    let otra = save(&New::nueva(
        "Impresora atascada",
        "La cola del spooler se vacía parando el servicio y borrando la carpeta PRINTERS",
    ))
    .expect("guardar");

    // Sin fijar, una pregunta que no tiene nada que ver no la trae.
    let r = lucy_core::memories::recall("cómo se despliega una aplicación web", 5);
    assert!(!r.bloque.contains("domingos"), "sin fijar no debería salir: {}", r.bloque);

    lucy_core::memories::set_pinned(g.id, true).expect("fijar");
    let r = lucy_core::memories::recall("cómo se despliega una aplicación web", 5);
    assert!(
        r.bloque.contains("domingos"),
        "una memoria fijada tiene que entrar sin parecerse: {}",
        r.bloque
    );
    assert_eq!(r.fijadas, 1, "y contarse aparte de las semánticas");
    // Marcada como lo que es: sin la marca, el modelo no distingue un hecho que
    // vino al caso de una regla que el operador dejó puesta.
    assert!(r.bloque.contains("[fijada]"), "{}", r.bloque);
    // Y la que NO está fijada sigue sin salir: fijar una no trae el resto.
    assert!(!r.bloque.contains("PRINTERS"), "arrastró una memoria ajena: {}", r.bloque);

    // Fijar sube la importancia a la de «lo decidió una persona», que es lo que
    // impide que el consolidador la funda con otra.
    let (_, _, _, imp) = fila(g.id);
    assert_eq!(imp, lucy_core::memories::FIJADA);
    let r = lucy_core::consolidate::run(true).expect("consolidar");
    assert!(
        r.clusters.iter().all(|c| c.canonical_id != g.id && !c.merged_ids.contains(&g.id)),
        "el consolidador se metió con una fijada"
    );

    // Y soltarla la devuelve al techo de lo automático: dejarla en 10 sin la
    // marca sería una memoria intocable que nada explica.
    lucy_core::memories::set_pinned(g.id, false).expect("soltar");
    let (_, _, _, imp) = fila(g.id);
    assert_eq!(imp, lucy_core::memories::MAX_AUTO_IMPORTANCE);
    let r = lucy_core::memories::recall("cómo se despliega una aplicación web", 5);
    assert_eq!(r.fijadas, 0);
    assert!(!r.bloque.contains("domingos"));
    let _ = otra;
}
