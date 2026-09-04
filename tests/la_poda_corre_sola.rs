//! Que la poda corra de verdad, y que se lleve solo lo que le toca.
//!
//! LAS DOS FUNCIONES ESTABAN ESCRITAS, PROBADAS Y DOCUMENTADAS, y no las llamaba
//! nadie. `audit::prune` lleva escrito en su propio comentario el motivo por el
//! que existe —«una fila por comando durante toda la vida de la instalación crece
//! sin techo»— y `notify::prune` la delicadeza que la hace segura: solo borra los
//! avisos VISTOS, porque uno sin ver es algo que el operador todavía no sabe.
//!
//! Es el patrón de la casa otra vez: la pieza entera menos la línea que la
//! enciende. Ahora esa línea existe y es un trabajo de `maintenance`, con su
//! vencimiento y su historial, como consolidar y reflexionar.
//!
//! Se prueba POR COMPORTAMIENTO y a través de `corre`, no llamando a las dos
//! podas a mano: lo que hay que fijar es que el trabajo esté CONECTADO. Llamar a
//! `prune` desde el test dejaría pasar exactamente el fallo que esto cierra.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

static TURNO: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turno() -> std::sync::MutexGuard<'static, ()> {
    TURNO.lock().unwrap_or_else(|e| e.into_inner())
}

fn con_base() {
    static UNA_VEZ: std::sync::Once = std::sync::Once::new();
    UNA_VEZ.call_once(|| {
        let d: PathBuf = std::env::temp_dir().join(format!(
            "lucy-poda-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|x| x.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).unwrap();
        let _ = lucy_core::schema::init_or_create(&d.join("lucy.db"));
        lucy_core::audit::ensure_schema().expect("esquema de auditoría");
        lucy_core::maintenance::ensure_schema().expect("esquema de mantenimiento");
        lucy_core::notify::ensure_schema().expect("esquema de avisos");
    });
}

fn ahora() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Una fila de auditoría con la fecha que se le diga.
///
/// Se escribe con SQL y no con `audit::record` porque `created_at` lo pone la
/// base por defecto: para probar un plazo hace falta poder fechar hacia atrás.
fn comando_hace(dias: i64, cmd: &str) {
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO audit_trail (timestamp, command, source, created_at)
             VALUES ('t', ?1, 'manual', ?2)",
            rusqlite::params![cmd, ahora() - dias * 86_400],
        )
        .map_err(|e| e.to_string())
    })
    .expect("insertar en la auditoría");
}

fn aviso_hace(dias: i64, titulo: &str, visto: bool) {
    lucy_core::with_db(|c| {
        c.execute(
            "INSERT INTO avisos (ts, titulo, visto) VALUES (?1, ?2, ?3)",
            rusqlite::params![ahora() - dias * 86_400, titulo, i64::from(visto)],
        )
        .map_err(|e| e.to_string())
    })
    .expect("insertar un aviso");
}

fn hay_comando(cmd: &str) -> bool {
    cuenta("SELECT COUNT(*) FROM audit_trail WHERE command = ?1", cmd) > 0
}

fn hay_aviso(titulo: &str) -> bool {
    cuenta("SELECT COUNT(*) FROM avisos WHERE titulo = ?1", titulo) > 0
}

fn cuenta(sql: &str, arg: &str) -> i64 {
    lucy_core::with_db(|c| {
        c.query_row(sql, rusqlite::params![arg], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())
    })
    .unwrap_or(0)
}

#[test]
fn la_poda_se_lleva_lo_vencido_y_respeta_lo_que_nadie_ha_visto() {
    let _t = turno();
    con_base();

    let d_aud = lucy_core::maintenance::DIAS_AUDITORIA;
    let d_avi = lucy_core::maintenance::DIAS_AVISOS;

    comando_hace(d_aud + 30, "Get-VencidoDeVerdad");
    comando_hace(d_aud - 30, "Get-TodaviaDentroDePlazo");
    aviso_hace(d_avi + 30, "visto y viejo", true);
    aviso_hace(d_avi + 30, "SIN VER y viejo", false);
    aviso_hace(d_avi - 30, "visto y reciente", true);

    let nota = lucy_core::maintenance::corre(
        lucy_core::maintenance::PODA,
        &AtomicBool::new(false),
    );

    // La nota se guarda en cifras, no en prosa: es lo que el shell traduce.
    match lucy_core::maintenance::Cifras::de_nota(&nota) {
        lucy_core::maintenance::Cifras::Poda { auditoria, avisos } => {
            assert!(auditoria >= 1, "no retiró la fila vencida de auditoría: {nota}");
            assert!(avisos >= 1, "no retiró el aviso visto y vencido: {nota}");
        }
        otra => panic!("la poda escribió unas cifras que no son suyas: {otra:?}"),
    }

    assert!(!hay_comando("Get-VencidoDeVerdad"), "se quedó una fila pasada de plazo");
    assert!(hay_comando("Get-TodaviaDentroDePlazo"), "se llevó una fila que está en plazo");

    assert!(!hay_aviso("visto y viejo"), "se quedó un aviso visto y vencido");
    assert!(hay_aviso("visto y reciente"), "se llevó un aviso visto que está en plazo");
    // LA LÍNEA QUE MÁS IMPORTA DE ESTE FICHERO. Un aviso sin ver es algo que el
    // operador todavía no sabe, y borrarlo por antiguo sería decidir por él que
    // ya no importa. Lo garantiza la consulta de `notify::prune`, y esto lo fija.
    assert!(hay_aviso("SIN VER y viejo"), "se llevó un aviso que nadie ha leído");
}

#[test]
fn una_poda_en_blanco_no_cuenta_como_que_rindio() {
    // Igual que una consolidación que no funde nada: acabó bien y no cambió nada.
    // La diferencia importa porque `racha_en_blanco` cuenta las seguidas, y una
    // racha larga de podas vacías dice que el plazo sobra — no que esté rota.
    use lucy_core::maintenance::Cifras;
    assert!(!Cifras::Poda { auditoria: 0, avisos: 0 }.rindio());
    assert!(Cifras::Poda { auditoria: 1, avisos: 0 }.rindio());
    assert!(Cifras::Poda { auditoria: 0, avisos: 1 }.rindio());
}

#[test]
fn las_cifras_de_la_poda_van_y_vuelven_de_la_columna() {
    // La nota viaja por disco como texto. Si `a_nota` y `de_nota` se separan, la
    // pestaña de Mantenimiento enseña la fila como prosa cruda —«d|3|7»— sin que
    // nada falle.
    use lucy_core::maintenance::Cifras;
    let ida = Cifras::Poda { auditoria: 3, avisos: 7 };
    assert_eq!(Cifras::de_nota(&ida.a_nota()), ida);
}
