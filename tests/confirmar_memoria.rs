//! Que volver a deducir un hecho signifique algo.
//!
//! EL CALLEJÓN QUE ESTO ABRE. Cuando Lucy vuelve a concluir algo que ya sabía,
//! el módulo descarta el duplicado y sube `access_count`. Ese contador NO LO LEE
//! NADIE en todo el núcleo — cero apariciones en un `WHERE` o un `ORDER BY`. Y
//! `confidence`, que sí tiene un consumidor en la V1 (la promoción a cristal,
//! con listón en 0,80), nacía en su valor por defecto y no se movía nunca porque
//! la escribía el modelo de una vez.
//!
//! O sea que la señal más valiosa del sistema de memoria —que Lucy llegue DOS
//! VECES por su cuenta al mismo sitio— se tiraba entera.
//!
//! Va en `tests/` porque necesita base de datos y el pool es un `OnceLock`
//! global: un proceso para él solo.

use std::path::PathBuf;

/// De uno en uno: los tests de este fichero comparten la tabla.
static TURNO: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turno() -> std::sync::MutexGuard<'static, ()> {
    TURNO.lock().unwrap_or_else(|e| e.into_inner())
}

fn con_base() {
    static UNA_VEZ: std::sync::Once = std::sync::Once::new();
    UNA_VEZ.call_once(|| {
        let d: PathBuf = std::env::temp_dir().join(format!(
            "lucy-confirma-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|x| x.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).unwrap();
        let _ = lucy_core::schema::init_or_create(&d.join("lucy.db"));
    });
}

/// Mete una fila directa, sin pasar por `save`: aquí se prueba la confirmación,
/// no la deduplicación.
fn siembra(titulo: &str) -> i64 {
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, files, importance)
             VALUES ('prueba', ?1, 'contenido de prueba', '[]', '[]', 2)",
            rusqlite::params![titulo],
        )
        .map_err(|e| e.to_string())?;
        Ok(c.last_insert_rowid())
    })
    .expect("insertar")
}

fn lee(id: i64) -> (i64, f64) {
    lucy_core::with_db(|c| {
        c.query_row(
            "SELECT access_count, confidence FROM agent_memories WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())
    })
    .expect("leer")
}

#[test]
fn cada_confirmacion_sube_la_confianza_y_el_contador() {
    let _t = turno();
    con_base();
    let id = siembra("el servidor de impresión es SRV-04");

    let (n0, c0) = lee(id);
    let (antes, despues) = lucy_core::memories::confirma(id).expect("confirmar");

    assert_eq!(antes, c0, "el «antes» que devuelve no es el que había");
    let (n1, c1) = lee(id);
    assert_eq!(n1, n0 + 1, "el contador de accesos no subió");
    assert!(c1 > c0, "la confianza no se movió: {c0} → {c1}");
    assert_eq!(c1, despues, "el «después» que devuelve no es el que quedó");
}

#[test]
fn el_rendimiento_es_decreciente_y_no_llega_al_uno() {
    let _t = turno();
    con_base();
    let id = siembra("la copia de seguridad corre a las 02:00");

    // LAS DOS PROPIEDADES QUE HACEN QUE ESTO SIRVA.
    //
    // Decreciente: la primera confirmación dice mucho —Lucy llegó dos veces por
    // su cuenta al mismo sitio— y la décima no dice casi nada, porque ya lo
    // sabíamos. Con un incremento fijo, veinte repeticiones de una anécdota
    // pesarían lo mismo que veinte de un hecho de verdad.
    let mut saltos = Vec::new();
    let mut previo = lee(id).1;
    for _ in 0..6 {
        lucy_core::memories::confirma(id).expect("confirmar");
        let ahora = lee(id).1;
        saltos.push(ahora - previo);
        previo = ahora;
    }
    for par in saltos.windows(2) {
        assert!(
            par[1] < par[0] + 1e-9,
            "un salto fue mayor que el anterior: {:?}",
            saltos
        );
    }

    // Y NO LLEGA AL 1,0 NUNCA. El uno queda para que lo diga una persona, igual
    // que la importancia 10. Lucy confirmándose seis veces es una buena señal,
    // no una certeza: puede haber leído el mismo dato equivocado seis veces.
    assert!(
        previo < 1.0 && previo <= lucy_core::memories::TECHO_CONFIRMACION,
        "la confianza automática pasó de su techo: {previo}"
    );
}

#[test]
fn cuatro_confirmaciones_cruzan_el_listón_de_la_promocion() {
    let _t = turno();
    con_base();
    let id = siembra("el dominio es corp.local");

    // EL NÚMERO NO ES ARBITRARIO. La promoción a cristal de la V1 exige
    // `confidence >= 0.80`, y el paso de 0,25 está elegido para que cuatro
    // confirmaciones lo crucen desde el valor por defecto. Si alguien mueve la
    // constante sin mirar, este test lo dice.
    for _ in 0..4 {
        lucy_core::memories::confirma(id).expect("confirmar");
    }
    let (accesos, conf) = lee(id);
    assert!(conf >= 0.80, "cuatro confirmaciones dejan la confianza en {conf}, y hace falta 0.80");
    assert_eq!(accesos, 4);
}

#[test]
fn lo_que_una_persona_marcó_no_se_baja_al_confirmarlo() {
    let _t = turno();
    con_base();
    let id = siembra("en producción se avisa antes de tocar nada");
    // Por encima del techo automático: eso solo lo pone un operador.
    lucy_core::with_db(|c| {
        c.execute("UPDATE agent_memories SET confidence = 1.0 WHERE id = ?1", [id])
            .map_err(|e| e.to_string())
    })
    .unwrap();

    lucy_core::memories::confirma(id).expect("confirmar");
    let (accesos, conf) = lee(id);
    assert_eq!(conf, 1.0, "confirmar BAJÓ una confianza puesta a mano: quedó en {conf}");
    // El acceso sí se cuenta: se ha usado, aunque no cambie la confianza.
    assert_eq!(accesos, 1);
}

#[test]
fn confirmar_una_memoria_que_no_existe_no_revienta() {
    let _t = turno();
    con_base();
    assert_eq!(
        lucy_core::memories::confirma(999_999),
        None,
        "una fila que no está tiene que devolver None, no fingir un movimiento"
    );
}
