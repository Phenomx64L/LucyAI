//! Que el directorio de trabajo que elige el operador rija de verdad.
//!
//! EL FALLO, contado por quien lo sufrió: «tiene tendencia a usar su ruta de
//! deploy y eso provoca que ciertos archivos se escriban en el proyecto». Y no
//! era una tendencia del modelo. El prompt de sistema le ORDENABA en cada turno
//! resolver los nombres sin ruta contra `std::env::current_dir()` — la carpeta
//! desde la que se lanzó el ejecutable.
//!
//! LO QUE ESTE TEST FIJA no es que la ruta se guarde, que es la parte fácil, sino
//! que ELEGIRLA CAMBIE DÓNDE ACABAN LOS FICHEROS. Guardar el ajuste y que
//! `resuelve` siguiera mirando a otro sitio es exactamente la clase de arreglo
//! que parece hecho: el desplegable enseña la carpeta correcta y los ficheros
//! siguen apareciendo donde no toca.
//!
//! Va en `tests/` y no en el módulo por lo mismo que `arranque_limpio`: el pool
//! de `lucy-core` es un `OnceLock` global y necesita un proceso para él solo.

use std::path::{Path, PathBuf};

/// DE UNO EN UNO. Los cinco tests de este fichero mueven el MISMO valor global
/// —el directorio de trabajo del proceso—, así que en paralelo cada uno lee el
/// que puso otro. `--test-threads=1` lo arregla desde fuera, pero un test que
/// solo pasa si se le invoca de cierta manera es un test que va a fallar el día
/// que alguien escriba `cargo test` a secas, y el fallo parecerá del código.
static DE_UNO_EN_UNO: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// El turno. Se queda con el candado hasta el final del test.
///
/// `unwrap_or_else` y no `unwrap`: si un test falla con el candado tomado, el
/// mutex queda envenenado y los otros cuatro morirían por eso en vez de por lo
/// suyo — cuatro fallos donde hay uno, y ninguno señalando la causa.
fn turno() -> std::sync::MutexGuard<'static, ()> {
    DE_UNO_EN_UNO.lock().unwrap_or_else(|e| e.into_inner())
}

fn carpeta_nueva(que: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "lucy-{que}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Abre una base propia. Idempotente: el pool es global y solo se abre una vez.
///
/// UNA CARPETA Y NO SEIS. Esto creaba una nueva en cada llamada, y como el pool
/// es un `OnceLock` solo servía la primera: las otras cinco quedaban en la
/// temporal del sistema para siempre. Con seis tests y unas cuantas ejecuciones
/// al día eso son cientos de carpetas huérfanas, que es como se llega a una
/// temporal donde ya no se encuentra nada.
fn con_base() {
    static UNA_VEZ: std::sync::Once = std::sync::Once::new();
    UNA_VEZ.call_once(|| {
        let db = carpeta_nueva("wd-base").join("lucy.db");
        let _ = lucy_core::schema::init_or_create(&db);
    });
}

/// Se lleva por delante lo que creó el test.
///
/// No es limpieza por gusto: cada test crea su carpeta de trabajo y `resuelve`
/// mira si las cosas EXISTEN. Dejarlas puestas hace que una ejecución vea los
/// restos de la anterior, que es la clase de test que pasa hasta el día que
/// alguien limpia la temporal.
fn borra(d: &std::path::Path) {
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn elegir_una_carpeta_cambia_donde_acaban_los_ficheros() {
    let _turno = turno();
    con_base();
    let trabajo = carpeta_nueva("wd-trabajo");

    let puesta = lucy_core::workdir::pon(&trabajo).expect("una carpeta que existe se acepta");
    assert_eq!(lucy_core::workdir::actual(), puesta);
    assert_eq!(lucy_core::workdir::configurado(), Some(puesta.clone()));

    // 1. LA RESOLUCIÓN DE UN FICHERO NUEVO. Es la mitad que hace que esto sirva:
    //    un fichero que todavía no existe no está en ninguna parte, así que sin
    //    respaldo la elección solo contaría para los que ya están — al revés de
    //    lo que se pidió, que era no tener que sondear la ruta cada vez.
    let r = lucy_core::tools::resuelve("informe-nuevo.txt");
    assert_eq!(
        r,
        trabajo.join("informe-nuevo.txt"),
        "un fichero nuevo no cae en el directorio de trabajo"
    );

    // 2. LO QUE SE PREPARA PARA ESCRIBIR. `apply` escribe en `a.path` tal cual,
    //    así que si aquí sale otra carpeta, el diff que aprueba el operador y el
    //    fichero que se escribe no son el mismo.
    let a = lucy_core::tools::prepare_write("notas.txt|||hola");
    assert!(a.blocked.is_empty(), "se bloqueó sin motivo: {}", a.blocked);
    assert_eq!(Path::new(&a.path), trabajo.join("notas.txt").as_path());

    // 3. Y UNO QUE SÍ EXISTE, en el directorio de trabajo, gana igual.
    let ya = trabajo.join("ya-estaba.log");
    std::fs::write(&ya, "x").unwrap();
    assert_eq!(lucy_core::tools::resuelve("ya-estaba.log"), ya);

    borra(&trabajo);
}

#[test]
fn una_ruta_completa_sigue_mandando() {
    let _turno = turno();
    con_base();
    let trabajo = carpeta_nueva("wd-absolutas");
    lucy_core::workdir::pon(&trabajo).unwrap();

    // El directorio de trabajo resuelve lo que NO lleva ruta. Reescribir una
    // absoluta sería quitarle al operador la única forma de decir «esta vez no».
    for p in [r"C:\Windows\System32\drivers\etc\hosts", r"\\servidor\recurso\log.txt"] {
        assert_eq!(lucy_core::tools::resuelve(p).display().to_string(), p);
    }

    borra(&trabajo);
}

#[test]
fn el_prompt_dice_la_carpeta_elegida_y_le_pide_que_no_pregunte() {
    let _turno = turno();
    con_base();
    let trabajo = carpeta_nueva("wd-prompt");
    let puesta = lucy_core::workdir::pon(&trabajo).unwrap();

    let ruta = puesta.display().to_string();
    let ctx = lucy_core::prompt::Ctx { working_dir: &ruta, ..Default::default() };
    let s = lucy_core::prompt::build(&ctx);

    assert!(s.contains(&ruta), "el prompt no lleva el directorio de trabajo");
    // NO PREGUNTES, que es literalmente lo que se pidió: «que no se tenga que
    // sondear a cada rato la ruta donde deberá trabajar». Sin esa frase, un
    // modelo prudente gasta un turno preguntando dónde dejar cada cosa.
    assert!(
        s.contains("No preguntes"),
        "el prompt no le dice que deje de preguntar dónde dejar los ficheros"
    );
    // Y NO PUEDE SEGUIR HABLANDO DEL DIRECTORIO DEL PROCESO. Ésa era la orden
    // que producía el fallo.
    if let Ok(proceso) = std::env::current_dir() {
        let p = proceso.display().to_string();
        if p != ruta {
            assert!(!s.contains(&p), "el prompt sigue nombrando el directorio del proceso");
        }
    }

    borra(&trabajo);
}

#[test]
fn olvidarlo_vuelve_a_la_carpeta_personal_y_nunca_a_la_del_proceso() {
    let _turno = turno();
    con_base();
    let trabajo = carpeta_nueva("wd-olvido");
    lucy_core::workdir::pon(&trabajo).unwrap();
    assert!(lucy_core::workdir::configurado().is_some());

    lucy_core::workdir::olvida().unwrap();
    assert_eq!(lucy_core::workdir::configurado(), None, "siguió configurado tras olvidarlo");

    let d = lucy_core::workdir::actual();
    assert!(d.is_absolute());
    assert_ne!(d, trabajo, "olvidarlo no cambió nada");
    // LA PROPIEDAD DE FONDO, y la única que no se puede perder: bajo ningún
    // camino se vuelve al directorio del proceso.
    if let Ok(proceso) = std::env::current_dir() {
        let casa = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
        if casa.map(PathBuf::from) != Some(proceso.clone()) {
            assert_ne!(d, proceso, "se volvió al directorio del proceso");
        }
    }

    borra(&trabajo);
}

#[test]
fn una_carpeta_que_ya_no_esta_no_deja_a_lucy_trabajando_en_el_vacio() {
    let _turno = turno();
    con_base();
    // El operador elige una carpeta de un USB, o de una unidad de red, y mañana
    // no está. Guardarla y devolverla igual convierte cada escritura en un error
    // raro; volver al de por defecto es peor de lo que eligió y mejor que nada.
    let temporal = carpeta_nueva("wd-desaparece");
    lucy_core::workdir::pon(&temporal).unwrap();
    std::fs::remove_dir_all(&temporal).unwrap();

    // Se fuerza una relectura desde la base, que es lo que pasa al arrancar.
    let leido = lucy_core::workdir::carga();
    assert_eq!(leido, None, "devolvió una carpeta que ya no existe");
    assert!(lucy_core::workdir::actual().is_dir(), "el de por defecto tampoco existe");
}

#[test]
fn preguntar_antes_de_que_la_base_este_abierta_no_deja_el_ajuste_muerto() {
    // EL CAMPO DE MINAS QUE ESTO DESACTIVA. «No hay nada guardado» y «no pude
    // preguntar» valen lo mismo al contestar —el de por defecto— pero NO se
    // pueden apuntar igual: cachear la segunda como si fuera la primera deja la
    // carpeta del operador sin efecto para el resto de la sesión, sin error y
    // sin aviso.
    //
    // Y pasa de verdad: en el shell nativo dos campos de un literal de struct
    // preguntaban antes de que un tercero abriera la base, y en la app Tauri
    // `GLOBAL_CWD` es un `Lazy` cuyo momento de lectura no controla nadie.
    let _turno = turno();
    con_base();
    let trabajo = carpeta_nueva("wd-orden");
    lucy_core::workdir::pon(&trabajo).unwrap();

    // Se simula el arranque: alguien pregunta y luego se vuelve a cargar. Si la
    // primera respuesta se hubiera apuntado como definitiva, la segunda no
    // podría corregirla.
    let _ = lucy_core::workdir::actual();
    assert_eq!(
        lucy_core::workdir::carga(),
        Some(trabajo.clone()),
        "una relectura no recupera lo que el operador eligió"
    );
    assert_eq!(lucy_core::workdir::actual(), trabajo);

    borra(&trabajo);
}
