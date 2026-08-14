//! Insights y el calendario del mantenimiento, contra una base de datos de
//! verdad.
//!
//! SIN OLLAMA. La destilación del patrón es lo único que necesita el modelo y su
//! lectura está probada aparte, con tests puros. Aquí se prueba lo que solo se ve
//! con disco: a qué memorias se les permite entrar, que reencontrar un patrón lo
//! REFUERCE en vez de duplicarlo, y que el reloj del mantenimiento sobreviva a
//! cerrar el programa — que es la razón entera por la que existe.
//!
//! Un solo `#[test]` con secciones, por el `OnceCell` del pool.

use lucy_core::{insights, maintenance};

fn arranca() {
    let p = std::env::temp_dir().join("lucy_core_insights_test.db");
    let _ = std::fs::remove_file(&p);
    lucy_core::init(&p).expect("init");
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

    // ── 5. Borrar uno deja los demás ────────────────────────────────────────
    let id = insights::list(10).expect("listar")[0].id;
    let antes = insights::list(50).expect("listar").len();
    insights::delete(id).expect("borrar");
    assert_eq!(insights::list(50).expect("listar").len(), antes - 1);

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
    let (_, nota) = maintenance::ultima(maintenance::INSIGHTS).expect("fila");
    assert!(nota.contains("elegibles"), "nota: {nota}");

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
    assert!(nota.contains("miradas"), "nota: {nota}");
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
