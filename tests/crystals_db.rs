//! Cristales, contra una base de datos de verdad.
//!
//! SIN OLLAMA. La destilación es lo único que necesita el modelo y ya está
//! probada aparte —`parse` es pura y los tests unitarios fijan lo que devolvió
//! `mistral` en esta máquina—. Lo que se prueba aquí es lo de después: que la
//! fila llegue al disco, que las lecciones salgan del visor y entren en la
//! memoria, que un secreto no se quede visible en la narración, y que llamar a
//! esto en cada cierre de turno no deje dos cristales de la misma sesión.
//!
//! Un solo `#[test]` con secciones, por lo mismo que en `memories_db`: las
//! funciones de un fichero corren en paralelo y se pelean por el `OnceCell` del
//! pool.

use lucy_core::crystals::{self, Crudo, Sesion};

fn arranca() {
    let p = std::env::temp_dir().join("lucy_core_crystals_test.db");
    let _ = std::fs::remove_file(&p);
    lucy_core::init(&p).expect("init");
}

fn crudo() -> Crudo {
    Crudo {
        narrativa: "Se vació la cola de impresión de SRV-04 y el spooler volvió a aceptar trabajos."
            .into(),
        hitos: vec!["El spooler lleva cinco minutos arriba".into()],
        archivos: vec!["C:\\Windows\\System32\\spool\\PRINTERS".into()],
        lecciones: vec![
            "Si PRINTERS pasa de 100 ficheros el spooler empieza a caerse solo".into(),
            "Vaciar PRINTERS exige parar el servicio antes".into(),
        ],
    }
}

fn memorias_con(tag: &str) -> Vec<(String, String, i64)> {
    lucy_core::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT title, content, importance FROM agent_memories
                 WHERE tags LIKE ?1 ORDER BY id ASC",
            )
            .map_err(|e| e.to_string())?;
        let v = st
            .query_map([format!("%{tag}%")], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })
    .expect("memorias")
}

#[test]
fn el_camino_entero_de_un_cristal() {
    arranca();

    // ── 1. Guardar crea el esquema y la fila vuelve entera ──────────────────
    let (id, nuevas) = crystals::guardar("s-1", &crudo(), 5_000).expect("guardar");
    assert!(id > 0);
    assert_eq!(nuevas, 2, "las dos lecciones son nuevas la primera vez");

    let cs = crystals::list(10).expect("listar");
    assert_eq!(cs.len(), 1);
    let c = &cs[0];
    assert_eq!(c.session_id, "s-1");
    assert!(c.narrativa.contains("spooler"));
    assert_eq!(c.hitos.len(), 1, "las listas vuelven parseadas, no como JSON crudo");
    assert_eq!(c.archivos.len(), 1);
    assert_eq!(c.lecciones.len(), 2);
    assert_eq!(c.caracteres, 5_000);

    // ── 2. Las lecciones son memorias de verdad ─────────────────────────────
    // Es la razón de ser de la promoción: un cristal que solo vive en su pestaña
    // es documentación, y la documentación que hay que ir a buscar no se busca.
    let lecciones = memorias_con("leccion");
    assert_eq!(lecciones.len(), 2);
    assert!(lecciones[0].0.starts_with("Lección:"));
    assert!(lecciones[0].1.contains("100 ficheros"));
    assert_eq!(lecciones[0].2, 2, "una lección destilada vale como un desenlace medido");

    // Y la narración, que es la que contesta «¿ya me pasó esto?».
    let sesion = memorias_con("sesion");
    assert_eq!(sesion.len(), 1);
    assert!(sesion[0].0.starts_with("Sesión:"));
    assert_eq!(sesion[0].2, 1, "contexto de sesión, no un hecho duro");
    // Las rutas viajan DENTRO del cuerpo para que las encuentre la búsqueda por
    // texto, que es como se pregunta por ellas.
    assert!(
        sesion[0].1.contains("PRINTERS"),
        "los ficheros tienen que ser buscables: {}",
        sesion[0].1
    );

    // ── 3. Una sesión no se cristaliza dos veces ────────────────────────────
    // La sonda barata que hace que se pueda invocar esto al cerrar CADA turno.
    assert!(crystals::existe("s-1"));
    assert!(!crystals::existe("s-2"));
    let r = crystals::cristaliza(
        "s-1",
        "da igual lo que ponga aquí",
        &Sesion { turnos: 20, herramientas: 20, caracteres: 20_000 },
        &std::sync::atomic::AtomicBool::new(false),
    );
    assert!(r.id.is_none());
    assert!(r.motivo.contains("ya tiene cristal"), "motivo: {}", r.motivo);
    assert_eq!(crystals::list(10).expect("listar").len(), 1);

    // ── 4. Una puerta cerrada no despierta a Ollama ni escribe nada ─────────
    // Con dos turnos no hay sesión, hay una pregunta. Y el motivo llega arriba:
    // «¿por qué no cristalizó?» tiene que tener respuesta.
    let r = crystals::cristaliza(
        "s-corta",
        "hola",
        &Sesion { turnos: 2, herramientas: 9, caracteres: 9_000 },
        &std::sync::atomic::AtomicBool::new(false),
    );
    assert!(r.id.is_none());
    assert!(r.motivo.contains("turnos"), "motivo: {}", r.motivo);
    assert_eq!(crystals::list(10).expect("listar").len(), 1);

    // ── 5. Un secreto no se queda visible en la fila del cristal ────────────
    // LA V2 SÍ LO DEJABA. Limpiaba las lecciones al promoverlas —porque las
    // limpia el guardado de memorias— pero insertaba la fila del cristal tal
    // cual, y esa fila es justo la que se enseña en el visor.
    let sucio = Crudo {
        narrativa: "Se conectó con password=Tr0ub4dor&3 al recurso".into(),
        hitos: vec![],
        archivos: vec![],
        lecciones: vec!["El token es ghp_AbCdEfGhIjKlMnOpQrStUvWxYz012345".into()],
    };
    crystals::guardar("s-3", &sucio, 100).expect("guardar");
    let c = crystals::list(10)
        .expect("listar")
        .into_iter()
        .find(|c| c.session_id == "s-3")
        .expect("el cristal de s-3");
    assert!(!c.narrativa.contains("Tr0ub4dor"), "la narración sigue sucia: {}", c.narrativa);
    assert!(c.narrativa.contains("REDACTADO"));
    assert!(!c.lecciones[0].contains("ghp_AbCdEf"), "la lección sigue sucia: {}", c.lecciones[0]);

    // ── 6. Borrar el cristal no borra lo que enseñó ─────────────────────────
    // Las lecciones son suyas desde que se guardaron: borrarlas con el cristal
    // quitaría cosas que ya se habían confirmado en otras sesiones.
    let antes = memorias_con("leccion").len();
    crystals::delete(id).expect("borrar");
    assert!(crystals::list(10).expect("listar").iter().all(|c| c.id != id));
    assert_eq!(memorias_con("leccion").len(), antes, "las lecciones sobreviven al cristal");
}
