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
fn el_liston_rebajado_esta_por_debajo_del_normal_y_por_encima_del_ruido() {
    let _t = turno();
    // LOS TRES NÚMEROS TIENEN QUE ESTAR EN ESTE ORDEN o la fase 2 no hace nada
    // —o hace demasiado— y ninguna de las dos cosas daría error en ninguna
    // parte: simplemente entraría lo que no debe, o no entraría nada.
    //
    // El suelo de ruido son las mediciones que documenta `MIN_DOCUMENTO`: lo que
    // no tenía NADA que ver puntuaba entre 0,53 y 0,56.
    const RUIDO: f32 = 0.56;
    assert!(
        lucy_core::memories::MIN_CONFIRMADA < lucy_core::memories::MIN_MEMORIA,
        "el listón rebajado no rebaja nada"
    );
    assert!(
        lucy_core::memories::MIN_CONFIRMADA > RUIDO,
        "el listón rebajado cae en el suelo de ruido: entraría cualquier cosa"
    );
}

#[test]
fn el_umbral_de_confianza_es_alcanzable_desde_el_valor_por_defecto() {
    let _t = turno();
    con_base();
    let id = siembra("el servidor de ficheros es FS-01");

    // QUE LAS DOS CONSTANTES NO SE SEPAREN. `UMBRAL_CONFIRMADA` decide quién
    // entra con el listón bajo y `PASO_CONFIRMACION` decide cuánto sube cada
    // confirmación. Si alguien baja el paso o sube el umbral sin mirar al otro,
    // el listón rebajado deja de alcanzarse y la fase 2 se apaga sola, en
    // silencio, sin que ningún test hable — que es exactamente como estaba el
    // sistema antes de todo esto.
    //
    // Ocho confirmaciones es holgado a propósito: lo que se fija es que sea
    // ALCANZABLE, no cuántas hacen falta.
    for _ in 0..8 {
        lucy_core::memories::confirma(id).expect("confirmar");
    }
    let (_, conf) = lee(id);
    assert!(
        conf >= lucy_core::memories::UMBRAL_CONFIRMADA,
        "ocho confirmaciones dejan la confianza en {conf} y el umbral pide {}: \
         las dos constantes se han separado",
        lucy_core::memories::UMBRAL_CONFIRMADA
    );
}

fn caducidad(id: i64) -> i64 {
    lucy_core::with_db(|c| {
        c.query_row("SELECT expires_at FROM agent_memories WHERE id = ?1", [id], |r| r.get(0))
            .map_err(|e| e.to_string())
    })
    .expect("leer caducidad")
}

#[test]
fn lo_automatico_nace_con_plazo_y_lo_demas_no() {
    let _t = turno();
    con_base();

    // SOLO LO AUTOMÁTICO CADUCA. Lo que guarda la escritura de turno es la
    // pregunta y la respuesta entera, con sus cifras dentro: «CPU al 22 %» es
    // cierto la noche que se mide y deja de serlo al día siguiente. Lo que dictó
    // el operador no es una foto de la máquina y no lleva plazo.
    let auto = lucy_core::memories::save(&
        lucy_core::memories::New::nueva(
            "una consulta automática",
            "Se ejecutó:\n- Get-Service\n\nEl equipo está al 22 % de CPU y con 10 GB libres.",
        )
        .con_tags(&["auto"]),
    )
    .expect("guardar");

    let dictada = lucy_core::memories::save(&lucy_core::memories::New::nueva(
        "el dominio de la casa",
        "El dominio de Active Directory de esta oficina es corp.local y el controlador es DC-01.",
    ))
    .expect("guardar");

    assert!(caducidad(auto.id) > 0, "una memoria automática nació sin plazo");
    assert_eq!(caducidad(dictada.id), 0, "se le puso plazo a algo que no es automático");
}

#[test]
fn confirmar_renueva_el_plazo_y_al_final_lo_quita() {
    let _t = turno();
    con_base();
    let g = lucy_core::memories::save(&
        lucy_core::memories::New::nueva(
            "el servicio de impresión se reinicia solo",
            "Se ejecutó:\n- Get-Service Spooler\n\nEl Spooler estaba detenido y se reinició sin \
             incidencias tras aplicar la acción de recuperación configurada.",
        )
        .con_tags(&["auto"]),
    )
    .expect("guardar");
    let id = g.id;
    let inicial = caducidad(id);
    assert!(inicial > 0);

    // LA PODA VA POR EL CRITERIO CORRECTO —lo que no vuelve, se va— y no por
    // antigüedad a secas, que borraría igual lo que se escribió hace tiempo y
    // sigue siendo cierto. Cada confirmación renueva el plazo desde hoy.
    for _ in 0..3 {
        lucy_core::memories::confirma(id).expect("confirmar");
        assert!(caducidad(id) > 0, "el plazo se quitó antes de tiempo");
    }

    // Y EN CUANTO CRUZA EL UMBRAL, se le quita la fecha: ya no es la foto de una
    // noche, es algo que ha resistido cuatro conversaciones distintas.
    lucy_core::memories::confirma(id).expect("confirmar");
    let (_, conf) = lee(id);
    assert!(conf >= lucy_core::memories::UMBRAL_CONFIRMADA, "no cruzó el umbral: {conf}");
    assert_eq!(
        caducidad(id),
        0,
        "cruzó el umbral de confirmada y sigue con fecha de caducidad"
    );
}

#[test]
fn confirmar_no_le_pone_plazo_a_lo_que_no_lo_tenia() {
    let _t = turno();
    con_base();
    // Una memoria dictada por el operador no caduca. Confirmarla no puede
    // ponerle una fecha que nunca tuvo — seria el olvido entrando por la puerta
    // de atras, y justo sobre lo unico que no se puede volver a medir.
    let g = lucy_core::memories::save(&lucy_core::memories::New::nueva(
        "en producción se avisa antes de reiniciar nada",
        "Regla de la casa: cualquier reinicio en producción se anuncia antes en el canal del \
         equipo, aunque sea fuera de horario.",
    ))
    .expect("guardar");
    assert_eq!(caducidad(g.id), 0);
    lucy_core::memories::confirma(g.id).expect("confirmar");
    assert_eq!(caducidad(g.id), 0, "confirmar le puso plazo a una memoria que no caducaba");
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

// ─────────────────────────────────────────────────────────────────────────────
// USAR NO ES CONFIRMAR
//
// La fase del olvido tenía un agujero por el medio, y este bloque es el que lo
// cierra. Una memoria automática nacía con sesenta días y solo los renovaba al
// CONFIRMARSE, o sea cuando Lucy volvía a deducirla por su cuenta. Una que
// entrase en el prompt todos los días sin volver a deducirse caducaba
// exactamente igual que una que no hubiese mirado nadie: el plazo estaba
// midiendo si Lucy REDESCUBRE, no si USA.
//
// Pero las dos cosas no pueden valer lo mismo. Si recuperar subiera la
// confianza, bastaría con preguntar cuatro veces lo mismo para que cualquier
// cosa cruzase el umbral y se volviera permanente por repetición del operador.
// Por eso son dos funciones y por eso hay tests: `usadas` mantiene viva,
// `confirma` además da crédito.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn recuperar_una_memoria_la_mantiene_viva_pero_no_la_hace_mas_creible() {
    let _t = turno();
    con_base();

    let m = lucy_core::memories::save(
        &lucy_core::memories::New::nueva(
            "cuántos discos tiene el servidor de ficheros",
            "Se ejecutó:\n- Get-Disk\n\nFS-01 tiene tres discos, dos de ellos en espejo.",
        )
        .con_tags(&["auto"]),
    )
    .expect("guardar");

    let (accesos_antes, confianza_antes) = lee(m.id);

    // Entra en el prompt cuatro veces: son cuatro turnos en los que la pregunta
    // se parecía a esta memoria. Con `confirma` eso cruzaría el listón de
    // promoción — lo prueba `cuatro_confirmaciones_cruzan_el_listón_de_la_promocion`.
    for _ in 0..4 {
        lucy_core::memories::usadas(&[m.id]);
    }

    let (accesos, confianza) = lee(m.id);
    assert_eq!(accesos, accesos_antes + 4, "recuperarla cuatro veces no contó como cuatro usos");
    assert_eq!(
        confianza, confianza_antes,
        "recuperar una memoria le subió la confianza: preguntar cuatro veces lo mismo no es \
         corroborar nada, y si contase, «confirmada» dejaría de significar algo"
    );
}

#[test]
fn usar_una_memoria_le_corre_el_plazo_desde_hoy() {
    let _t = turno();
    con_base();

    let m = lucy_core::memories::save(
        &lucy_core::memories::New::nueva(
            "qué versión de Windows corre el controlador",
            "Se ejecutó:\n- Get-ComputerInfo\n\nDC-01 va con Windows Server 2019, 17763.5576.",
        )
        .con_tags(&["auto"]),
    )
    .expect("guardar");

    // Se le envejece el plazo a mano hasta dejarlo a dos días de vencer: es una
    // memoria escrita hace casi dos meses y no vuelta a deducir nunca.
    let casi = lucy_core::memories::ahora() + 2 * 86_400;
    lucy_core::with_db(|c| {
        c.execute("UPDATE agent_memories SET expires_at = ?1 WHERE id = ?2", rusqlite::params![casi, m.id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("envejecer");

    lucy_core::memories::usadas(&[m.id]);

    assert!(
        caducidad(m.id) > casi,
        "una memoria que acaba de entrar en el prompt seguía a dos días de caducar: el olvido \
         estaría borrando justo lo que se está usando"
    );
    assert!(lucy_core::memories::viva(m.id), "la dio por muerta después de usarla");
}

#[test]
fn usar_no_le_pone_plazo_a_lo_que_no_caducaba() {
    let _t = turno();
    con_base();

    // La misma puerta de atrás que ya se cerró en `confirma`. Lo que dictó el
    // operador no caduca, y recuperarlo no puede ser la forma de que empiece a
    // hacerlo.
    let g = lucy_core::memories::save(&lucy_core::memories::New::nueva(
        "la ventana de mantenimiento",
        "Los parches se aplican los domingos de 02:00 a 05:00, nunca en horario de oficina.",
    ))
    .expect("guardar");

    assert_eq!(caducidad(g.id), 0);
    lucy_core::memories::usadas(&[g.id]);
    assert_eq!(caducidad(g.id), 0, "usar una memoria dictada le puso fecha de caducidad");
}

#[test]
fn no_recuperar_nada_no_toca_la_base() {
    let _t = turno();
    con_base();

    // El caso de todos los turnos en los que la pregunta no se parece a nada.
    // Sin esta salida temprana la lista vacía arma un `IN ()`, que es un error
    // de sintaxis de SQLite.
    lucy_core::memories::usadas(&[]);
}
