//! Que el vigilante avise con los umbrales del operador, y no con los de fábrica.
//!
//! EL FALLO QUE ESTA PRUEBA CIERRA no se veía por ninguna parte. La tabla de
//! umbrales se lee con `WHERE host_id = ?1` y `thresholds::de` cae a los valores
//! de fábrica cuando no encuentra fila, así que escribir con una clave y leer con
//! otra NO FALLA: devuelve unos umbrales perfectamente válidos que no son los que
//! el operador puso.
//!
//! Y eso es lo que pasaba. Tres literales sueltos en tres ficheros:
//!
//! ```text
//!   vista_config.rs   guarda("local", …)     ← el panel, el único que escribe
//!   main.rs           de("local")            ← el Dashboard: obedecía
//!   watch.rs          de("")                 ← el vigilante: no obedecía
//! ```
//!
//! El operador bajaba el corte del disco al 70 %, veía la tarjeta ponerse ámbar
//! —esa lee «local»— y la notificación seguía saltando en el 85 de fábrica. La
//! pantalla decía una cosa y el aviso otra.
//!
//! Lo irónico es que `thresholds` existe precisamente para eso: su cabecera
//! cuenta que había TRES ESCALAS para el mismo dato en la misma pantalla. Con
//! este desajuste volvían a ser dos.
//!
//! Se prueba por COMPORTAMIENTO y no comparando constantes: que dos literales
//! sean iguales no dice nada el día que alguien añada un tercer sitio. Lo que
//! esto fija es que guardar un umbral cambia lo que el vigilante decide.

use std::path::PathBuf;

static TURNO: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turno() -> std::sync::MutexGuard<'static, ()> {
    TURNO.lock().unwrap_or_else(|e| e.into_inner())
}

fn con_base() {
    static UNA_VEZ: std::sync::Once = std::sync::Once::new();
    UNA_VEZ.call_once(|| {
        let d: PathBuf = std::env::temp_dir().join(format!(
            "lucy-umbrales-{}-{}",
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

/// Una foto del sistema con el disco al `pct` por ciento y todo lo demás en calma.
///
/// CPU y memoria bajas a propósito: lo que se mide aquí es el disco, y un síntoma
/// de CPU colándose en la lista haría pasar la prueba por el motivo equivocado.
fn con_disco_al(pct: f32) -> lucy_core::system::SysSnapshot {
    const TOTAL: u64 = 1_000_000_000_000;
    lucy_core::system::SysSnapshot {
        host: "PRUEBA".into(),
        os: "Windows".into(),
        kernel: String::new(),
        cpu_brand: String::new(),
        cpu_pct: 5.0,
        per_core: vec![5.0],
        mem_used: 2_000_000_000,
        mem_total: 16_000_000_000,
        swap_used: 0,
        swap_total: 0,
        uptime_secs: 3_600,
        cores: 1,
        nucleos_fisicos: 1,
        cpu_mhz: 0,
        disks: vec![lucy_core::system::DiskInfo {
            name: "Sistema".into(),
            mount: "C:\\".into(),
            total: TOTAL,
            avail: TOTAL - (TOTAL as f64 * (pct as f64 / 100.0)) as u64,
            soporte: lucy_core::system::Soporte::Ssd,
            fs: "NTFS".into(),
            extraible: false,
        }],
    }
}

fn habla_del_disco(ds: &[lucy_core::watch::Decision]) -> bool {
    ds.iter().any(|d| match d {
        lucy_core::watch::Decision::Avisa(s, _) => s.clave.contains("disco"),
        lucy_core::watch::Decision::Calla(_) => false,
    })
}

#[test]
fn bajar_el_umbral_en_el_panel_cambia_lo_que_avisa_el_vigilante() {
    let _t = turno();
    con_base();

    // Un disco al 75 %. Con los cortes de fábrica —aviso en 80— esto se calla.
    let foto = con_disco_al(75.0);

    let de_fabrica = lucy_core::thresholds::Umbrales::default();
    lucy_core::thresholds::guarda(lucy_core::thresholds::LOCAL, &de_fabrica)
        .expect("guardar los de fábrica");
    lucy_core::watch::olvida_la_sesion();
    assert!(
        !habla_del_disco(&lucy_core::watch::pasada(&foto, None, 1_000)),
        "con el corte en {} un disco al 75 % no es noticia",
        de_fabrica.disco_aviso
    );

    // El operador baja el corte a 70, que es lo que hace el panel de ajustes.
    let suyos = lucy_core::thresholds::Umbrales { disco_aviso: 70.0, ..de_fabrica };
    lucy_core::thresholds::guarda(lucy_core::thresholds::LOCAL, &suyos)
        .expect("guardar los del operador");
    lucy_core::watch::olvida_la_sesion();

    // AQUÍ ES DONDE SE CAÍA. Antes, `pasada` leía la cadena vacía, no encontraba
    // fila, y seguía usando el 80 de fábrica: el mismo silencio que arriba.
    assert!(
        habla_del_disco(&lucy_core::watch::pasada(&foto, None, 2_000)),
        "el vigilante ignoró el umbral que el operador acababa de guardar"
    );
}
