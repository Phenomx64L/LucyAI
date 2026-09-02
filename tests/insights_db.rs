//! Insights y el calendario del mantenimiento, contra una base de datos de
//! verdad.
//!
//! SIN OLLAMA. La destilación del patrón es lo único que necesita el modelo y su
//! lectura está probada aparte, con tests puros. Aquí se prueba lo que solo se ve
//! con disco: a qué memorias se les permite entrar, que reencontrar un patrón lo
//! REFUERCE en vez de duplicarlo, y que el reloj del mantenimiento sobreviva a
//! cerrar el programa — que es la razón entera por la que existe.
//!
//! DOS TESTS QUE NO SE PISAN. El pool es un `OnceCell`, así que `arranca` se
//! protege con un `Once`: el segundo `init` reventaría. Se pueden separar porque
//! tocan tablas distintas —uno el corpus de memorias e insights, otro solo el
//! calendario, y con un nombre de trabajo propio— pero cualquier test nuevo que
//! toque agent_memories tiene que ir dentro del primero, en su propia sección.

use lucy_core::{insights, maintenance};

/// UNA SOLA VEZ AUNQUE LO LLAMEN VARIOS TESTS. El pool es un `OnceCell`: el
/// segundo `init` no es que sea redundante, es que revienta. Y borrar el fichero
/// dos veces borraría la base por debajo de un test que ya está corriendo.
fn arranca() {
    static UNA_VEZ: std::sync::Once = std::sync::Once::new();
    UNA_VEZ.call_once(|| {
        let p = std::env::temp_dir().join("lucy_core_insights_test.db");
        let _ = std::fs::remove_file(&p);
        lucy_core::init(&p).expect("init");
    });
}

fn ahora() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Una memoria con la fecha y la sesión que digamos, que es lo que las consultas
/// de elegibilidad miran.
fn mete(titulo: &str, contenido: &str, tags: &str, sesion: &str, edad_dias: i64) {
    lucy_core::memories::ensure_schema().expect("esquema");
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![sesion, titulo, contenido, tags, ahora() - edad_dias * 86_400],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("insert");
}

/// Lee la fila EN CRUDO, saltándose `list`.
///
/// Hace falta justamente porque `list` esconde las descartadas: para comprobar
/// que a una lápida no le sube la confianza por debajo hay que mirarla por
/// dentro.
fn confianza_y_refuerzos(id: i64) -> (f64, i64) {
    lucy_core::with_db(|c| {
        c.query_row(
            "SELECT confidence, reinforcements FROM agent_insights WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())
    })
    .expect("leer la fila descartada")
}

fn supersede(titulo: &str) {
    lucy_core::with_db(|c| {
        c.execute(
            "UPDATE agent_memories SET superseded_by = '999' WHERE title = ?1",
            rusqlite::params![titulo],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("supersede");
}

#[test]
fn insights_y_el_reloj_del_mantenimiento() {
    arranca();

    // ── 1. Quién puede entrar en una reflexión ──────────────────────────────
    mete("WinRM SRV-01", "El certificado de WinRM había caducado", "[\"auto\"]", "s1", 30);
    mete("WinRM SRV-04", "El certificado LocalMachine estaba caducado", "[\"auto\"]", "s1", 20);
    // Recién escrita: un patrón sacado de esta mañana es la misma frase dicha dos
    // veces.
    mete("De hoy", "Algo que acaba de pasar", "[\"auto\"]", "s1", 0);
    // Trozo de manual: no es una observación de esta instalación, y después de
    // una ingesta sería el 99 % del corpus.
    mete("Manual pág. 40", "El protocolo WinRM usa el puerto 5985", "[]", "pdf:7", 30);
    mete("Resumen del manual", "Manual de WinRM", "[]", "pdf-doc:7", 30);
    // Supersedida por la consolidación: ya no es verdad.
    mete("Vieja", "Lo que decíamos antes del certificado", "[\"auto\"]", "s1", 30);
    supersede("Vieja");

    let e = insights::elegibles().expect("elegibles");
    let titulos: Vec<&str> = e.iter().map(|f| f.titulo.as_str()).collect();
    assert!(titulos.contains(&"WinRM SRV-01"));
    assert!(titulos.contains(&"WinRM SRV-04"));
    assert!(!titulos.contains(&"De hoy"), "las recientes no entran: {titulos:?}");
    assert!(!titulos.contains(&"Manual pág. 40"), "los trozos de PDF no entran");
    assert!(!titulos.contains(&"Resumen del manual"), "ni el resumen del documento");
    assert!(!titulos.contains(&"Vieja"), "lo supersedido ya no es verdad");
    assert_eq!(e.len(), 2);

    // Las etiquetas vuelven parseadas, que es de lo que depende el agrupado.
    assert_eq!(e[0].tags, vec!["auto".to_string()]);

    // ── 2. Sin material, se dice por qué y no se toca Ollama ────────────────
    // Dos elegibles y hacen falta cuatro. El motivo tiene que llegar: un cero
    // pelado es indistinguible de una avería.
    let r = insights::run(&std::sync::atomic::AtomicBool::new(false));
    assert_eq!(r.elegibles, 2);
    assert_eq!(r.creados + r.reforzados, 0);
    assert!(r.motivo.contains("hacen falta"), "motivo: {}", r.motivo);

    // ── 3. Reencontrar un patrón lo refuerza, no lo duplica ─────────────────
    let c = insights::Crudo {
        contenido: "Los errores de WinRM suelen venir de un certificado caducado.".into(),
        conceptos: vec!["winrm".into(), "certificados".into()],
    };
    assert!(insights::guarda(&c, 4).expect("guarda"), "la primera vez es nueva");

    // La misma frase con otro formato cae en la misma huella.
    let otra_forma = insights::Crudo {
        contenido: "  los ERRORES de WinRM   suelen venir de un certificado caducado.  ".into(),
        conceptos: vec!["winrm".into()],
    };
    assert!(!insights::guarda(&otra_forma, 3).expect("guarda"), "la segunda refuerza");
    assert!(!insights::guarda(&otra_forma, 3).expect("guarda"));

    let l = insights::list(10).expect("listar");
    assert_eq!(l.len(), 1, "tres pasadas, una sola fila");
    assert_eq!(l[0].refuerzos, 3);
    assert_eq!(l[0].fuentes, 4 + 3 + 3, "las fuentes se acumulan");
    // 0.5 → 0.55 → 0.595. Asintótica hacia 1 y sin llegar: un patrón visto tres
    // veces es probable, no seguro.
    assert!(l[0].confianza > 0.59 && l[0].confianza < 0.60, "confianza: {}", l[0].confianza);
    assert_eq!(l[0].conceptos, vec!["winrm".to_string(), "certificados".to_string()]);

    // Un patrón distinto sí es otra fila, y el más confiable manda en la lista.
    let otro = insights::Crudo {
        contenido: "Las colas de impresión largas tiran el spooler solo.".into(),
        conceptos: vec!["impresión".into()],
    };
    assert!(insights::guarda(&otro, 4).expect("guarda"));
    let l = insights::list(10).expect("listar");
    assert_eq!(l.len(), 2);
    assert!(l[0].contenido.contains("WinRM"), "el más reforzado va primero");

    // ── 4. Un secreto no se queda escrito en un patrón ──────────────────────
    let sucio = insights::Crudo {
        contenido: "Para entrar en el recurso se usa password=Tr0ub4dor&3 siempre.".into(),
        conceptos: vec![],
    };
    insights::guarda(&sucio, 4).expect("guarda");
    let l = insights::list(10).expect("listar");
    assert!(
        l.iter().all(|i| !i.contenido.contains("Tr0ub4dor")),
        "un patrón con una contraseña dentro: {l:?}"
    );

    // ── 5. Descartar uno lo retira y deja los demás ─────────────────────────
    let primero = insights::list(10).expect("listar")[0].clone();
    let id = primero.id;
    let contenido_descartado = primero.contenido.clone();
    let antes = insights::list(50).expect("listar").len();
    let (conf_antes, refuerzos_antes) = confianza_y_refuerzos(id);
    insights::descarta(id).expect("descartar");
    assert_eq!(insights::list(50).expect("listar").len(), antes - 1);
    assert_eq!(insights::descartados(), 1, "no se contó el patrón desmentido");

    // ── 5b. Y NO PUEDE VOLVER ───────────────────────────────────────────────
    //
    // Es el fallo entero que la lápida arregla. `guarda` da de alta por INSERT
    // y resuelve el choque de huella reforzando; con la fila BORRADA no había
    // choque, así que la siguiente pasada destilaba el mismo grupo, escribía la
    // misma frase, y el patrón que el operador acababa de rechazar volvía a
    // entrar a 0,50 como si fuera nuevo. Una vez por noche, indefinidamente.
    assert!(
        insights::list(50).expect("listar").iter().all(|i| i.id != id),
        "el descartado sigue en la lista"
    );
    // Se vuelve a destilar exactamente lo mismo, que es lo que hace el modelo
    // local cuando el corpus no ha cambiado.
    let repetido = insights::Crudo {
        contenido: contenido_descartado.clone(),
        conceptos: vec![],
    };
    insights::guarda(&repetido, 4).expect("guarda");
    assert!(
        insights::list(50).expect("listar").iter().all(|i| i.contenido != contenido_descartado),
        "un patrón que el operador desmintió volvió a entrar en la lista"
    );
    // Y tampoco se ha reforzado por debajo: sin el `rejected_at = 0` del WHERE
    // subiría de confianza cada noche, callado, hasta cruzar el listón del
    // prompt y volver a dirigir cada turno.
    // SE COMPARA CONTRA LO QUE TENÍA, no contra un número escrito a mano: esta
    // fila ya llevaba refuerzos de las secciones de arriba, y afirmar «tiene que
    // valer 1» sería probar el estado del test en vez del comportamiento.
    let (conf, rej) = confianza_y_refuerzos(id);
    assert_eq!(
        rej, refuerzos_antes,
        "se reforzó un patrón desmentido: la repetición ganó a la persona"
    );
    assert!(
        (conf - conf_antes).abs() < 1e-9,
        "le subió la confianza a un patrón desmentido: {conf_antes} → {conf}. Sin el \
         `rejected_at = 0` del WHERE subiría cada noche, callado, hasta cruzar el listón del \
         prompt y volver a dirigir cada turno."
    );

    // ── 6. El reloj sobrevive a cerrar el programa ──────────────────────────
    // ES LA PIEZA ENTERA. La V2 dormía cuarenta y ocho horas en un hilo: en un
    // portátil que se cierra cada tarde, ese hilo no despierta nunca.
    assert!(maintenance::toca("prueba", 100), "lo que no se ha hecho nunca está vencido");
    maintenance::marca("prueba", "salió bien").expect("marcar");
    assert!(!maintenance::toca("prueba", 100), "recién hecho no toca");
    let (cuando, nota) = maintenance::ultima("prueba").expect("hay fila");
    assert!((ahora() - cuando).abs() <= 5);
    assert_eq!(nota, "salió bien");
    assert!(maintenance::faltan("prueba", 100) > 90);

    // Con el plazo cumplido vuelve a tocar. Se envejece la fila a mano porque
    // esperar cuarenta y ocho horas no es un test.
    lucy_core::with_db(|c| {
        c.execute(
            "UPDATE maintenance_runs SET last_run = last_run - 200 WHERE job = 'prueba'",
            [],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("envejecer");
    assert!(maintenance::toca("prueba", 100));
    assert!(maintenance::faltan("prueba", 100) < 0);

    // ── 7. Una tanda corre lo vencido y lo anota aunque no cambie nada ──────
    // Anotar el intento es lo que impide que un Ollama caído deje el trabajo
    // vencido para siempre y se reintente en cada comprobación.
    let t = maintenance::tanda(&std::sync::atomic::AtomicBool::new(false));
    assert!(t.hubo_algo(), "en una base nueva los dos trabajos están vencidos");
    assert!(t.consolidado.is_some());
    assert!(t.reflexionado.is_some());
    assert!(maintenance::ultima(maintenance::CONSOLIDAR).is_some());
    assert!(maintenance::ultima(maintenance::INSIGHTS).is_some());
    // Y la nota de la reflexión dice por qué no salió nada, no solo que no salió.
    //
    // SE COMPRUEBA EL SIGNIFICADO Y NO UNA PALABRA. Esto miraba que la nota
    // contuviera «elegibles», y era cierto mientras la nota fuera una frase en
    // español escrita en el núcleo. Al pasar a `Cifras` la columna guarda
    // `s|1|motivo` y la palabra se mudó a la tabla de idiomas del shell — que
    // era justo el objetivo del cambio, porque una frase en español no se puede
    // traducir a los otros cuatro idiomas ni leer para nada más.
    //
    // Buscar la palabra ahora sería atarse otra vez a la redacción. Lo que hay
    // que exigir es lo que el operador necesita: que la pasada en blanco diga
    // POR QUÉ, porque un cero pelado es indistinguible de una avería.
    let (_, nota) = maintenance::ultima(maintenance::INSIGHTS).expect("fila");
    match maintenance::Cifras::de_nota(&nota) {
        maintenance::Cifras::SinPatrones { motivo, .. } => {
            assert!(!motivo.trim().is_empty(), "no dice por qué no salió nada: {nota}");
        }
        otra => panic!("se esperaba una pasada sin patrones con su motivo: {otra:?} — {nota}"),
    }

    // La segunda tanda seguida no repite nada.
    let t = maintenance::tanda(&std::sync::atomic::AtomicBool::new(false));
    assert!(!t.hubo_algo(), "acabado de hacer, no toca: {t:?}");

    // ── 8. «Ponte al día ahora» corre aunque no toque ───────────────────────
    // Es el botón de la vista de Memoria: esperar dos días para ver si una
    // corrección funcionó no es una forma de verificar nada.
    let (antes, _) = maintenance::ultima(maintenance::CONSOLIDAR).expect("fila");
    let nota = maintenance::corre(
        maintenance::CONSOLIDAR,
        &std::sync::atomic::AtomicBool::new(false),
    );
    // Por las CIFRAS y no por la palabra, por lo mismo que arriba: la columna
    // guarda `c|miradas|grupos|fundidas` y «miradas» vive en la tabla del shell.
    match maintenance::Cifras::de_nota(&nota) {
        maintenance::Cifras::Consolidacion { miradas, .. } => {
            // Las dos memorias que este test acaba de escribir. Que la pasada
            // MIRE algo es lo que distingue «corrió» de «no llegó a la base».
            assert!(miradas > 0, "la pasada no miró ninguna memoria: {nota}");
        }
        otra => panic!("se esperaban cifras de consolidación: {otra:?} — {nota}"),
    }
    let (despues, guardada) = maintenance::ultima(maintenance::CONSOLIDAR).expect("fila");
    assert!(despues >= antes);
    assert_eq!(guardada, nota, "la nota devuelta y la de disco tienen que ser la misma");

    // ── 9. Borrar una memoria se lleva también su vector ────────────────────
    // La mitad que se olvidaba en todas partes: una fila borrada cuyo vector
    // queda sigue saliendo en la búsqueda por significado.
    lucy_core::memories::ensure_schema().expect("esquema");
    lucy_core::vectors::ensure_schema().expect("esquema vectores");
    let id = lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO agent_memories (title, content) VALUES ('borrable', 'contenido')",
            [],
        )
        .map_err(|e| e.to_string())?;
        Ok(c.last_insert_rowid())
    })
    .expect("insert");
    lucy_core::vectors::upsert(
        "memory",
        &[(id.to_string(), "borrable — contenido".into(), vec![1.0_f32, 0.0])],
        "nomic-embed-text",
    )
    .expect("vector");
    lucy_core::memories::delete(id).expect("borrar");
    let quedan: i64 = lucy_core::with_db(|c| {
        c.query_row(
            "SELECT (SELECT COUNT(*) FROM agent_memories WHERE id = ?1)
                  + (SELECT COUNT(*) FROM embeddings
                     WHERE entity_type = 'memory' AND entity_id = ?2)",
            rusqlite::params![id, id.to_string()],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .expect("count");
    assert_eq!(quedan, 0, "borrar dejó la fila o el vector");
}

/// El histórico del mantenimiento: la serie, que la nota suelta no puede decir.
///
/// Va en su propio `#[test]` porque no comparte el corpus de memorias con el de
/// arriba — solo escribe filas de mantenimiento— pero SÍ comparte el pool, así
/// que usa un nombre de trabajo propio para no pisarse con nadie.
#[test]
fn el_historial_distingue_una_pasada_en_blanco_de_un_mes_en_blanco() {
    arranca();
    const JOB: &str = "prueba-historial";

    // Sin pasadas, no hay racha que contar. Es el estado de una instalación
    // nueva y no puede sacar un aviso.
    assert_eq!(maintenance::racha_en_blanco(JOB), (0, 0));

    // Tres en blanco seguidas.
    for i in 0..3 {
        maintenance::marca_con(JOB, &format!("0 elegibles · corpus pequeño ({i})"), false)
            .expect("marcar");
    }
    let (veces, desde) = maintenance::racha_en_blanco(JOB);
    assert_eq!(veces, 3, "no se cuentan las pasadas seguidas sin resultado");
    assert!(desde > 0, "la racha no dice desde cuándo, y sin eso el número engaña");

    // UNA QUE RINDE LA CORTA. Es lo que separa «esto viene fallando» de «esto
    // falló»: si la racha no se reiniciara, un problema resuelto seguiría
    // avisando para siempre y el aviso dejaría de leerse.
    maintenance::marca_con(JOB, "4 elegibles, 1 grupo, 1 patrón nuevo", true).expect("marcar");
    assert_eq!(
        maintenance::racha_en_blanco(JOB),
        (0, 0),
        "una pasada que rindió no cortó la racha"
    );

    // Y vuelve a contar desde cero.
    maintenance::marca_con(JOB, "0 elegibles", false).expect("marcar");
    assert_eq!(maintenance::racha_en_blanco(JOB).0, 1);

    // EL RESUMEN SIGUE SIENDO EL RESUMEN. Las dos tablas contestan preguntas
    // distintas —«cuándo tocó» decide si vence, «qué viene saliendo» es la
    // serie— y escribir en el histórico no puede haber roto el reloj.
    let (_, nota) = maintenance::ultima(JOB).expect("el resumen se perdió");
    assert_eq!(nota, "0 elegibles", "el resumen no lleva la última nota");

    // El histórico entero, la más reciente primero.
    let h = maintenance::historial(JOB, 10);
    assert_eq!(h.len(), 5);
    assert_eq!(h[0].nota, "0 elegibles");
    assert!(h[1].rindio, "el orden no es de la más reciente a la más vieja");

    // LA PODA. Sin techo, un año de pasadas diarias son trescientas sesenta y
    // cinco filas por trabajo que nadie va a leer.
    for i in 0..maintenance::HISTORIAL_MAX + 10 {
        maintenance::marca_con(JOB, &format!("relleno {i}"), false).expect("marcar");
    }
    let n: i64 = lucy_core::with_db(|c| {
        c.query_row("SELECT COUNT(*) FROM maintenance_log WHERE job = ?1", [JOB], |r| r.get(0))
            .map_err(|e| e.to_string())
    })
    .expect("contar");
    assert_eq!(
        n as usize,
        maintenance::HISTORIAL_MAX,
        "el histórico crece sin techo: {n} filas"
    );
}
