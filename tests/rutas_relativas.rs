//! Una ruta relativa no se resuelve contra el directorio del proceso.
//!
//! EL DIRECTORIO DE TRABAJO DE LUCY INSTALADA ES `C:\Program Files\Lucy`: de ahí
//! arranca el acceso directo del menú Inicio. Nadie guarda nada ahí. Así que
//! `std::fs`, que resuelve las relativas contra él, buscaba y escribía en el
//! único sitio del disco donde seguro que no está lo que se pide.
//!
//! Las dos formas de fallar no se parecen:
//!
//!   · LEER fallaba RUIDOSAMENTE y sin decir dónde. «No se pudo abrir
//!     'informe.txt'» no le dice al modelo que se miró en Archivos de programa,
//!     así que lo reintenta igual y gasta las vueltas que tiene.
//!
//!   · ESCRIBIR fallaba EN SILENCIO. El artefacto guardaba la relativa, y
//!     `apply` escribe en `a.path` sin volver a resolverla. O muere con acceso
//!     denegado —confuso pero visible— o, con Lucy elevada, escribe DENTRO de
//!     Archivos de programa: sale bien, el operador ve «aplicado», y el fichero
//!     no está donde lo buscó.
//!
//! Este test corre en `tests/` a propósito: necesita cambiar el directorio de
//! trabajo del proceso para reproducirlo, y eso es global — dentro de la batería
//! del `lib` se lo haría a los demás.

use std::path::Path;

/// Dónde cae una relativa, sin tocar el disco.
fn resuelta(p: &str) -> String {
    lucy_core::tools::resuelve(p).display().to_string()
}

#[test]
fn una_relativa_no_cae_en_el_directorio_del_proceso() {
    // Se pone el proceso en una carpeta que hace de `C:\Program Files\Lucy`.
    let ajeno = std::env::temp_dir().join("lucy-hace-de-archivos-de-programa");
    std::fs::create_dir_all(&ajeno).unwrap();
    let antes = std::env::current_dir().ok();
    std::env::set_current_dir(&ajeno).unwrap();

    let r = resuelta("informe.txt");

    // Se devuelve el proceso a donde estaba: `set_current_dir` es global y los
    // demás tests de este binario corren después.
    if let Some(a) = antes {
        let _ = std::env::set_current_dir(a);
    }
    assert!(
        !Path::new(&r).starts_with(&ajeno),
        "una relativa acabó en el directorio del proceso: {r}"
    );
    assert!(Path::new(&r).is_absolute(), "no salió una ruta absoluta: {r}");
}

#[test]
fn lo_que_se_prepara_para_escribir_ya_lleva_la_ruta_completa() {
    // `apply` escribe en `a.path` tal cual. Si aquí sale una relativa, lo que se
    // escribe depende de dónde esté el proceso, y eso no lo ve nadie en el diff.
    let a = lucy_core::tools::prepare_write("notas.txt|||hola");
    assert!(
        Path::new(&a.path).is_absolute(),
        "el artefacto guarda una ruta relativa: {}",
        a.path
    );
    assert!(a.blocked.is_empty(), "se bloqueó sin motivo: {}", a.blocked);
}

#[test]
fn una_virgulilla_pegada_a_otra_cosa_no_es_la_carpeta_personal() {
    // `~$informe.docx` es el fichero de bloqueo que Word deja al lado de un
    // documento abierto, y en la máquina de un administrador hay varios a la
    // vista. Tratar la virgulilla como «carpeta personal» lo manda a otro sitio,
    // y el error dice que no existe un fichero que sí está.
    let Some(casa) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) else {
        return;
    };
    let r = resuelta("~$informe.docx");
    assert!(
        !r.ends_with("$informe.docx") || r.ends_with("~$informe.docx"),
        "se comió la virgulilla del nombre: {r}"
    );
    // Y `~` de verdad sí es la carpeta personal.
    assert_eq!(resuelta("~"), Path::new(&casa).display().to_string());
    assert_eq!(
        resuelta("~\\notas.txt"),
        Path::new(&casa).join("notas.txt").display().to_string()
    );
}

#[test]
fn una_absoluta_no_se_toca() {
    // Lo normal es que el modelo escriba la ruta entera, y ahí no hay nada que
    // decidir. Un resolutor que «mejore» una absoluta es un resolutor roto.
    for p in [r"C:\Windows\System32\drivers\etc\hosts", r"\\servidor\recurso\log.txt"] {
        assert_eq!(resuelta(p), p, "se cambió una ruta absoluta");
    }
    // Y `\Windows\System32` tampoco: no lleva unidad, pero tampoco es relativa a
    // una carpeta personal. Ésa la resuelve el sistema contra la unidad actual.
    assert_eq!(resuelta(r"\Windows\System32"), r"\Windows\System32");
}

#[test]
fn se_prefiere_el_fichero_que_existe_de_verdad() {
    // De las cuatro carpetas que se miran gana la primera donde HAY algo. Sin
    // esto, «lee presupuesto.txt» crea uno vacío en la carpeta personal en vez
    // de leer el que el operador tiene en el escritorio.
    let Some(casa) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) else {
        return; // Sin perfil no hay nada que preferir.
    };
    let escritorio = Path::new(&casa).join("Desktop");
    if !escritorio.is_dir() {
        return;
    }
    let nombre = "lucy-prueba-de-rutas-relativas.txt";
    let puesto = escritorio.join(nombre);
    std::fs::write(&puesto, "contenido").unwrap();

    let r = resuelta(nombre);
    let _ = std::fs::remove_file(&puesto);

    assert_eq!(
        r,
        puesto.display().to_string(),
        "estaba en el escritorio y se resolvió a otro sitio"
    );
}

#[test]
fn al_modelo_se_le_dice_dónde_se_miró() {
    // El aviso va en el CUERPO, que es lo único que lee el modelo. Sin él, pide
    // la relativa, se le contesta con el contenido de otra carpeta, y en el
    // turno siguiente vuelve a escribir la relativa.
    let r = lucy_core::tools::run("readfile", "no-existe-este-fichero-de-lucy.txt").unwrap();
    assert!(!r.ok);
    assert!(
        r.body.contains("relativa"),
        "el error no explica que la ruta era relativa: {}",
        r.body
    );
    // Y lleva la ruta donde se miró de verdad, no la que se pidió a secas.
    assert!(
        r.body.contains(std::path::MAIN_SEPARATOR),
        "el error no dice dónde se miró: {}",
        r.body
    );
}
