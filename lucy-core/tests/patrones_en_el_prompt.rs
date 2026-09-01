//! Que lo que Lucy generaliza vuelva a la conversación.
//!
//! EL ÚNICO SITIO DONDE LUCY GENERALIZA. La pasada de mantenimiento destila
//! insights cada 24 horas gastando hasta cuatro llamadas al modelo local, con
//! toda la maquinaria de huella, refuerzo y confianza asintótica funcionando —
//! para producir filas que solo se veían si el operador abría una pestaña
//! concreta. Lo generalizado no volvía nunca al prompt, así que Lucy podía
//! haber deducido un patrón sobre esta instalación en marzo y no usarlo ni una
//! vez en abril. Se quedaba en un «ya lo sabía» que no servía para el trabajo
//! siguiente.
//!
//! Aquí se prueban los tres frenos que hacen que la sección sirva en vez de
//! estorbar: pocos, buenos, y cortos.
//!
//! UN SOLO `#[test]` PARA TODO LO QUE TOCA DISCO, por el `OnceCell` del pool: dos
//! tests con base correrían en paralelo sobre la misma tabla y se pisarían el
//! corpus. El segundo test del fichero es aritmética pura y no abre la base, por
//! eso puede ir suelto.

use lucy_core::insights;

fn arranca() {
    let p = std::env::temp_dir().join("lucy_core_patrones_prompt_test.db");
    let _ = std::fs::remove_file(&p);
    lucy_core::init(&p).expect("init");
    insights::ensure_schema().expect("esquema");
}

/// Un patrón ya destilado, con la confianza que digamos.
fn mete(contenido: &str, confianza: f64) {
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO agent_insights (content, fingerprint, confidence, reinforcements)
             VALUES (?1, ?2, ?3, 1)",
            rusqlite::params![contenido, insights::huella(contenido), confianza],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("insert");
}

fn vacia() {
    lucy_core::with_db(|c| {
        c.execute("DELETE FROM agent_insights", []).map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("vaciar");
}

#[test]
fn los_patrones_que_llegan_al_prompt() {
    arranca();

    // ── SIN NADA, NADA ──────────────────────────────────────────────────────
    //
    // La regla del fichero de prompt: un bloque vacío enseña al modelo a
    // saltarse ese encabezado, así que el día que sí trae algo tampoco lo mira.
    // En una instalación nueva no hay ni un insight, que es el caso normal
    // durante las primeras semanas.
    assert_eq!(insights::render(), "", "se montó una sección sin nada dentro");

    // ── POCOS: EL TOPE ──────────────────────────────────────────────────────
    //
    // Son frases largas —un patrón observado, no una etiqueta— y meterle veinte
    // al modelo en cada turno es enseñarle a saltarse el encabezado.
    for i in 0..8 {
        mete(&format!("Patrón número {i}: algo que se repite en este equipo."), 0.9);
    }
    let r = insights::render();
    let cuantos = r.lines().filter(|l| l.starts_with("- ")).count();
    assert_eq!(
        cuantos,
        insights::MAX_EN_PROMPT,
        "el tope de patrones no se respeta: entraron {cuantos}"
    );

    // ── BUENOS: EL LISTÓN DE CONFIANZA ──────────────────────────────────────
    //
    // Un insight nace con la confianza que le puso el modelo y solo sube con los
    // refuerzos. Por debajo del listón es una corazonada de una sola
    // observación, y mandarle corazonadas al modelo como si fueran conocimiento
    // es la forma más rápida de que generalice de una casualidad.
    vacia();
    mete("Los reinicios de este equipo se concentran los lunes.", 0.85);
    mete("Puede que el disco D esté relacionado con algo.", 0.30);
    let r = insights::render();
    assert!(r.contains("los lunes"), "no entró un patrón que se lo había ganado");
    assert!(
        !r.contains("Puede que"),
        "entró una corazonada de una sola observación como si fuera conocimiento"
    );

    // El listón se comprueba contra la constante y no contra un número escrito
    // a mano: si alguien la baja, este test tiene que seguir midiendo lo mismo.
    vacia();
    mete("Justo por debajo del listón.", insights::MIN_CONFIANZA_PROMPT - 0.01);
    assert_eq!(insights::render(), "", "el listón deja pasar lo que está justo debajo");
    vacia();
    mete("Justo en el listón.", insights::MIN_CONFIANZA_PROMPT);
    assert!(!insights::render().is_empty(), "el listón rechaza lo que está justo encima");

    // ── CORTOS: EL TOPE DE CARACTERES ───────────────────────────────────────
    //
    // El modelo local escribe la frase y no hay nada que le impida devolver un
    // párrafo. Sin tope duro, una mala destilación de hace tres semanas se come
    // el prompt de todos los turnos desde entonces.
    vacia();
    for i in 0..3 {
        mete(&format!("{i} {}", "palabra ".repeat(120)), 0.9);
    }
    let r = insights::render();
    let cuerpo: String = r.lines().filter(|l| l.starts_with("- ")).collect::<Vec<_>>().join("\n");
    assert!(
        cuerpo.chars().count() <= insights::MAX_CHARS_PROMPT,
        "el tope de caracteres no frena una destilación larga: {} caracteres",
        cuerpo.chars().count()
    );
    // Y no corta por la mitad: entran enteros los que quepan y se para. Media
    // conclusión es peor que ninguna.
    for l in cuerpo.lines() {
        assert!(l.ends_with("palabra"), "una frase salió cortada: «{}»", &l[l.len() - 20..]);
    }

    // ── EL AVISO NO ES OPCIONAL ─────────────────────────────────────────────
    //
    // Un insight es una observación de Lucy sobre su propio historial, no un
    // hecho comprobado. Sin esta línea el modelo los cita con la misma autoridad
    // que una medición, y un patrón sacado de cuatro conversaciones puede ser
    // perfectamente una casualidad.
    vacia();
    mete("Los reinicios de este equipo se concentran los lunes.", 0.9);
    let r = insights::render();
    assert!(r.contains("no hechos comprobados"), "falta el aviso de que pueden estar mal");
    assert!(r.contains("gana lo que ves"), "no se dice qué hacer cuando el patrón se contradice");

    // ── LOS SALTOS DE LÍNEA NO ROMPEN LA LISTA ──────────────────────────────
    vacia();
    mete("Una frase\ncon un salto\n  y sangría.", 0.9);
    let r = insights::render();
    let cuerpo: Vec<&str> = r.lines().filter(|l| l.starts_with("- ")).collect();
    assert_eq!(cuerpo.len(), 1, "un patrón con saltos se partió en varias viñetas");
    assert_eq!(cuerpo[0], "- Una frase con un salto y sangría.");

    // ── EL PANEL Y EL PROMPT NO PUEDEN SEÑALAR COSAS DISTINTAS ──────────────
    vacia();

    // El panel marca «en uso» los patrones que de verdad viajan en el prompt, y
    // saca esa marca de la misma función que monta el bloque. Si cada uno
    // aplicara los topes por su cuenta, el día que alguien cambiara el tope el
    // panel estaría señalando filas distintas de las que se mandan — y el
    // operador borraría la que no era.
    for i in 0..6 {
        mete(&format!("Patrón {i}: algo que se repite."), 0.9 - i as f64 * 0.01);
    }
    mete("Una corazonada suelta.", 0.20);

    let elegidos = insights::seleccion();
    let bloque = insights::render();

    assert_eq!(elegidos.len(), insights::MAX_EN_PROMPT);
    for (_, linea) in &elegidos {
        assert!(
            bloque.contains(linea.as_str()),
            "el panel marcaría «{linea}» como en uso y el prompt no la lleva"
        );
    }
    let en_bloque = bloque.lines().filter(|l| l.starts_with("- ")).count();
    assert_eq!(
        en_bloque,
        elegidos.len(),
        "el prompt lleva patrones que el panel no marca: el operador no puede saber cuál corregir"
    );
    assert!(!bloque.contains("corazonada"), "una corazonada llegó al prompt");
}

/// Que el listón sea alcanzable, y en cuántos refuerzos.
///
/// LA MISMA TRAMPA QUE YA COSTÓ UN NÚMERO EN ESTE PROYECTO: un umbral que suena
/// razonable y que en la práctica no cruza nadie. Aquí se comprueba la
/// aritmética que la constante documenta, para que bajar `REFUERZO` o subir el
/// listón rompa el test en vez de dejar la sección callada para siempre.
#[test]
fn el_liston_se_cruza_al_tercer_refuerzo_y_no_antes() {
    let mut c: f64 = 0.5; // con la que nace un insight
    let mut n = 0;
    while c < insights::MIN_CONFIANZA_PROMPT && n < 50 {
        c += insights::REFUERZO * (1.0 - c);
        n += 1;
    }
    assert_eq!(
        n, 3,
        "hacen falta {n} refuerzos para que un patrón llegue al prompt, no 3: o el listón se ha \
         movido o lo ha hecho el paso, y la constante dice otra cosa"
    );
    assert!(c < 0.70, "cruzar el listón no puede dejarlo casi confirmado de golpe");
}
