//! Que «lo gastado hoy» sea el hoy del operador y no el de Greenwich.
//!
//! EL FALLO. La consulta cortaba por `datetime('now', 'start of day')`, y el
//! `now` de SQLite es UTC. En un operador a UTC−6 eso significa que el día
//! empieza a las seis de la tarde de AYER: a las cinco la cifra arrastra la noche
//! anterior, y a las seis y un minuto se pone a cero a media tarde sin que haya
//! cambiado el día para nadie que esté mirando la pantalla.
//!
//! Y de esa cifra cuelga la idea de un tope diario, que es justamente lo que la
//! cabecera del módulo dice que hace falta porque el tope por sesión se reinicia
//! en cada arranque.
//!
//! ── POR QUÉ ESTA PRUEBA MIRA EL CORTE Y NO LA SUMA ───────────────────────────
//!
//! Porque en una máquina en UTC —la mayoría de los servidores de integración— la
//! medianoche local Y la de Greenwich son la misma, así que una prueba que solo
//! sumara filas pasaría en verde con el fallo puesto. Estaría comprobando el
//! único caso en el que no hay nada que comprobar.
//!
//! Así que se comprueban las dos cosas: que el corte sea de verdad la medianoche
//! LOCAL —lo que falla en cualquier máquina que no esté en UTC— y que la suma
//! respete ese corte, que es lo que falla en todas.
//!
//! `usage.rs` era el único módulo del núcleo sin una sola prueba.

use std::path::PathBuf;

static TURNO: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turno() -> std::sync::MutexGuard<'static, ()> {
    TURNO.lock().unwrap_or_else(|e| e.into_inner())
}

fn con_base() {
    static UNA_VEZ: std::sync::Once = std::sync::Once::new();
    UNA_VEZ.call_once(|| {
        let d: PathBuf = std::env::temp_dir().join(format!(
            "lucy-gasto-{}-{}",
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

/// Una fila de gasto con la fecha UTC que se le diga.
fn gasto_en(cuando_utc: &str, coste: f64) {
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO token_usage
                 (id, task_id, timestamp, model, input_tokens, output_tokens,
                  total_cost, user, request_type, created_at)
             VALUES (?1, '', ?1, 'claude-opus-5', 1, 1, ?2, '', 'chat', ?3)",
            rusqlite::params![format!("t{cuando_utc}{coste}"), coste, cuando_utc],
        )
        .map_err(|e| e.to_string())
    })
    .expect("insertar gasto");
}

/// El mismo instante, corrido `segundos`, en el formato de la columna.
fn corrido(base: &str, segundos: i64) -> String {
    lucy_core::with_db(|c| {
        c.query_row(
            "SELECT datetime(?1, ?2)",
            rusqlite::params![base, format!("{segundos} seconds")],
            |f| f.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())
    })
    .expect("correr el reloj")
}

#[test]
fn el_corte_del_dia_es_la_medianoche_del_operador() {
    let _t = turno();
    con_base();

    let corte = lucy_core::usage::inicio_del_dia().expect("el corte");

    // LA COMPROBACIÓN QUE DISTINGUE EL ARREGLO DEL FALLO. Se pasa el corte a la
    // hora local: tiene que caer exactamente en una medianoche. Con el corte
    // viejo —medianoche UTC— en una máquina a UTC−6 daría las 18:00, y aquí es
    // donde salta.
    let local = lucy_core::with_db(|c| {
        c.query_row("SELECT datetime(?1, 'localtime')", rusqlite::params![&corte], |f| {
            f.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())
    })
    .expect("pasar el corte a hora local");

    assert!(
        local.ends_with(" 00:00:00"),
        "el día del operador no empieza a medianoche: el corte cae a las {local}"
    );
}

#[test]
fn la_suma_respeta_el_corte_por_los_dos_lados() {
    let _t = turno();
    con_base();

    let corte = lucy_core::usage::inicio_del_dia().expect("el corte");
    // Un segundo antes es de ayer; un segundo después, de hoy. Los dos se
    // escriben en UTC, que es como está la columna.
    gasto_en(&corrido(&corte, -1), 7.0);
    let antes = lucy_core::usage::gasto_de_hoy();
    gasto_en(&corrido(&corte, 1), 3.0);
    let despues = lucy_core::usage::gasto_de_hoy();

    assert!(
        (despues - antes - 3.0).abs() < 1e-9,
        "lo de hoy no suma bien: antes {antes}, después {despues}"
    );
    // Y lo de ayer no entra por ninguna parte: si entrara, la diferencia
    // seguiría siendo 3 pero el total arrastraría los 7 de la noche anterior.
    assert!(
        antes < 7.0,
        "el gasto de ayer cuenta como de hoy: {antes} incluye los 7 de antes del corte"
    );
}
