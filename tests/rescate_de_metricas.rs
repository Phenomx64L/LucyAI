//! Que un renombrado no se lleve por delante un día de historial.
//!
//! EL RESTO DE UNA CORRECCIÓN QUE SE QUEDÓ A MEDIAS. `history` creó
//! `metric_samples` —singular— sin mirar que la app Tauri ya escribía
//! `metrics_samples` —plural— en el MISMO fichero, con un nombre que se
//! diferencia en una letra. Cambiar a qué tabla se escribe arregló el futuro y
//! dejó el pasado tirado: en la instalación real quedaron 43 muestras dentro de
//! la tabla vieja que ninguna consulta vuelve a mirar.
//!
//! Un módulo cuya razón de ser es contestar «¿esto es nuevo?» no puede permitirse
//! tirar un día de mediciones por un renombrado suyo.

use std::path::PathBuf;

fn arranca() {
    static UNA_VEZ: std::sync::Once = std::sync::Once::new();
    UNA_VEZ.call_once(|| {
        let p: PathBuf = std::env::temp_dir().join(format!(
            "lucy-rescate-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|x| x.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_file(&p);
        lucy_core::init(&p).expect("init");
    });
}

/// Deja la tabla vieja tal y como la creó `history` antes de la corrección.
fn tabla_vieja_con(filas: &[(&str, i64, f64, f64, &str)]) {
    lucy_core::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS metric_samples (
                 id      INTEGER PRIMARY KEY AUTOINCREMENT,
                 host_id TEXT NOT NULL,
                 ts      INTEGER NOT NULL,
                 cpu     REAL NOT NULL,
                 mem     REAL NOT NULL,
                 discos  TEXT NOT NULL
             )",
        )
        .map_err(|e| e.to_string())?;
        for (host, ts, cpu, mem, discos) in filas {
            c.execute(
                "INSERT INTO metric_samples (host_id, ts, cpu, mem, discos)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![host, ts, cpu, mem, discos],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .expect("sembrar la tabla vieja");
}

fn hay_tabla(nombre: &str) -> bool {
    lucy_core::with_db(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [nombre],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())
    })
    .map(|n| n > 0)
    .unwrap_or(false)
}

fn muestras() -> Vec<(i64, f64, f64, f64, String)> {
    lucy_core::with_db(|c| {
        let mut st = c
            .prepare("SELECT ts, cpu, ram, disk, discos FROM metrics_samples ORDER BY ts")
            .map_err(|e| e.to_string())?;
        let v = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|x| x.ok())
            .collect();
        Ok(v)
    })
    .expect("leer")
}

#[test]
fn las_muestras_de_la_tabla_vieja_se_rescatan_una_sola_vez() {
    arranca();

    tabla_vieja_con(&[
        ("local", 1_787_718_010, 0.71, 28.47, "C:\\=43.0"),
        ("local", 1_787_718_070, 1.47, 29.25, "C:\\=43.0,D:\\=12.5"),
        ("srv-01", 1_787_718_130, 5.00, 60.00, ""),
    ]);
    assert!(hay_tabla("metric_samples"), "la siembra no dejó la tabla vieja");

    // `ensure_schema` es quien rescata: corre al arrancar cualquier cosa que
    // toque el historial, así que no hace falta acordarse de llamar a nada.
    lucy_core::history::ensure_schema().expect("esquema");

    let m = muestras();
    assert_eq!(m.len(), 3, "no se rescataron las tres muestras: {m:?}");

    // EL MAPEO. `mem` de la vieja es `ram` de la nueva, y el detalle por punto de
    // montaje se conserva entero.
    assert_eq!(m[0].0, 1_787_718_010);
    assert!((m[0].2 - 28.47).abs() < 1e-6, "`mem` no acabó en `ram`");
    assert_eq!(m[0].4, "C:\\=43.0");

    // EL PORCENTAJE DE DISCO SE SACA DEL DETALLE. La tabla vieja no guardaba un
    // porcentaje suelto, y ponerlo a cero habría metido en la serie muestras de
    // «disco al 0 %» que son mentira y que hunden cualquier media.
    assert!((m[0].3 - 43.0).abs() < 1e-6, "el disco no se sacó del detalle: {}", m[0].3);
    // Con varios puntos de montaje se coge el primero y no se traga la coma.
    assert!((m[1].3 - 43.0).abs() < 1e-6, "con dos discos salió {}", m[1].3);
    // Sin detalle, cero — que es lo que había y no es peor.
    assert!((m[2].3 - 0.0).abs() < 1e-6);

    // LA TABLA VIEJA DESAPARECE, y ése es el registro de que esto ya corrió: la
    // segunda vez no hay nada que encontrar, así que no hace ni una consulta.
    // Sin el borrado haría falta una marca aparte, y la marca sería otra cosa
    // que mantener.
    assert!(!hay_tabla("metric_samples"), "la tabla vieja sigue ahí");

    // Y correr otra vez no duplica ni revienta: es lo que va a pasar en cada
    // arranque a partir de ahora.
    lucy_core::history::ensure_schema().expect("esquema");
    lucy_core::history::ensure_schema().expect("esquema");
    assert_eq!(muestras().len(), 3, "el rescate se repitió");

    // ── UNA MUESTRA QUE YA ESTABA NO SE DUPLICA ─────────────────────────────
    //
    // Va en el mismo `#[test]` y no en uno aparte porque necesita volver a crear
    // la tabla vieja, y la sección de arriba la borra: en dos tests paralelos
    // sobre la misma base eso es una carrera, y con un mutex seguirían viéndose
    // las filas del otro en los recuentos.

    // Una muestra que la V1 ya había escrito en la tabla buena.
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO metrics_samples (host_id, ts, cpu, ram, disk) VALUES ('local', ?1, 9.0, 9.0, 9.0)",
            [1_600_000_000_i64],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("sembrar la nueva");

    // La misma marca de tiempo en la vieja: es lo que pasa con el minuto en que
    // las dos tablas estuvieron escribiendo a la vez.
    tabla_vieja_con(&[
        ("local", 1_600_000_000, 1.0, 1.0, "C:\\=1.0"),
        ("local", 1_600_000_060, 2.0, 2.0, "C:\\=2.0"),
    ]);
    lucy_core::history::ensure_schema().expect("esquema");

    let repes: i64 = lucy_core::with_db(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM metrics_samples WHERE ts = ?1",
            [1_600_000_000_i64],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .expect("contar");
    assert_eq!(repes, 1, "se duplicó una muestra que ya estaba");

    // Y la que sí era nueva entró.
    let nuevas: i64 = lucy_core::with_db(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM metrics_samples WHERE ts = ?1",
            [1_600_000_060_i64],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .expect("contar");
    assert_eq!(nuevas, 1, "la muestra que no estaba se perdió por precaución de más");
}
