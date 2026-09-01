//! Que la auditoría conteste «esto viene fallando» y no solo «qué pasó el martes».
//!
//! LA SEÑAL LLEVABA VERSIONES EN DISCO. `exit_code` y `duration_ms` se escriben
//! en cada fila del registro, con índice por fecha, por equipo y por origen, y no
//! los agregaba nadie: el visor pinta una lista cronológica. Así que Lucy podía
//! proponer por tercera vez un comando que ya había fallado dos veces en esta
//! misma máquina, y el operador aprobarlo sin más pista que su memoria.
//!
//! Es el patrón de la casa otra vez —la pieza entera menos la línea que la
//! enciende— y aquí se prueban las tres decisiones que hacen que el aviso sirva:
//! que la coincidencia sea exacta, que el equipo cuente, y que «no se sabe cómo
//! acabó» no se confunda con «fue bien».

use std::path::PathBuf;

static TURNO: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn turno() -> std::sync::MutexGuard<'static, ()> {
    TURNO.lock().unwrap_or_else(|e| e.into_inner())
}

fn con_base() {
    static UNA_VEZ: std::sync::Once = std::sync::Once::new();
    UNA_VEZ.call_once(|| {
        let d: PathBuf = std::env::temp_dir().join(format!(
            "lucy-fallos-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|x| x.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).unwrap();
        let _ = lucy_core::schema::init_or_create(&d.join("lucy.db"));
        lucy_core::audit::ensure_schema().expect("esquema de auditoría");
    });
}

/// Deja una fila como la que escribe el shell al terminar un comando.
fn corrio(cmd: &str, host: &str, ok: bool) {
    let e = lucy_core::audit::Entry::nueva(cmd, "ai")
        .en_equipo(host, if host.is_empty() { "local" } else { host })
        .resultado(ok, 120, if ok { "vale" } else { "no se pudo" });
    lucy_core::audit::record(&e).expect("registrar");
}

fn fallos(cmd: &str, host: &str) -> usize {
    lucy_core::audit::fallos_recientes(cmd, host, lucy_core::audit::DIAS_FALLOS).expect("contar")
}

#[test]
fn se_cuentan_los_fallos_de_este_comando_y_no_los_aciertos() {
    let _t = turno();
    con_base();

    let cmd = "Restart-Service -Name 'Spooler' -Force";
    assert_eq!(fallos(cmd, ""), 0, "sin historial no puede haber aviso");

    corrio(cmd, "", false);
    corrio(cmd, "", false);
    corrio(cmd, "", true); // el día que sí funcionó

    assert_eq!(
        fallos(cmd, ""),
        2,
        "o no cuenta los fallos o está contando también las veces que fue bien"
    );
}

#[test]
fn un_comando_parecido_no_dispara_el_aviso_de_otro() {
    let _t = turno();
    con_base();

    // COINCIDENCIA EXACTA, Y ES LO QUE HACE QUE EL AVISO SIRVA. Lo tentador es
    // cortar por el cmdlet y contar todos los `Get-Service` que fallaron, pero
    // eso mezcla dos preguntas sobre dos servicios distintos: uno puede llevar
    // semanas roto sin que eso diga nada del otro. Un aviso que salta por
    // parecido es un aviso que se aprende a ignorar, y entonces no avisa de
    // nada.
    corrio("Get-Service -Name 'w3svc'", "", false);
    corrio("Get-Service -Name 'w3svc'", "", false);
    corrio("Get-Service -Name 'w3svc'", "", false);

    assert_eq!(fallos("Get-Service -Name 'w3svc'", ""), 3);
    assert_eq!(
        fallos("Get-Service -Name 'MSSQLSERVER'", ""),
        0,
        "el aviso saltó por parecerse al comando de al lado"
    );
}

#[test]
fn lo_que_falla_en_un_equipo_no_avisa_en_otro() {
    let _t = turno();
    con_base();

    // Un comando que falla en un servidor y funciona en otro es información
    // sobre el servidor, no sobre el comando. Por eso el equipo entra en la
    // clave.
    let cmd = "Get-WinEvent -LogName System -MaxEvents 50";
    corrio(cmd, "srv-fs01", false);
    corrio(cmd, "srv-fs01", false);

    assert_eq!(fallos(cmd, "srv-fs01"), 2);
    assert_eq!(fallos(cmd, "srv-dc01"), 0, "el historial de un equipo se coló en otro");
    assert_eq!(fallos(cmd, ""), 0, "el historial de un remoto se coló en el equipo local");
}

#[test]
fn lo_que_no_se_sabe_como_acabo_no_cuenta_como_fallo() {
    let _t = turno();
    con_base();

    // `exit_code = None` significa «no se sabe»: la terminal local es un PTY sin
    // evento de fin, y un paso descartado no llegó a correr. Ninguna de las dos
    // cosas es un fallo, y contarlas sacaría el aviso en pasos que nunca han
    // ido mal.
    let cmd = "ipconfig /all";
    lucy_core::audit::record(&lucy_core::audit::Entry::nueva(cmd, "manual"))
        .expect("registrar un enviado sin resultado");
    lucy_core::audit::record(
        &lucy_core::audit::Entry::nueva(cmd, "descartado").nota("Caducado — llegó una orden nueva"),
    )
    .expect("registrar un descartado");

    assert_eq!(
        fallos(cmd, ""),
        0,
        "se está contando como fallo algo de lo que no se sabe cómo acabó"
    );
}

#[test]
fn una_ventana_de_cero_dias_o_un_comando_vacio_no_consultan_nada() {
    let _t = turno();
    con_base();

    // Las dos salidas tempranas. La del comando vacío importa porque el panel
    // pregunta por cada paso pendiente y un paso sin detalle es posible.
    assert_eq!(lucy_core::audit::fallos_recientes("", "", 14), Ok(0));
    assert_eq!(lucy_core::audit::fallos_recientes("   ", "", 14), Ok(0));
    assert_eq!(lucy_core::audit::fallos_recientes("cualquier cosa", "", 0), Ok(0));
}

#[test]
fn una_fila_descartada_guarda_el_motivo_y_no_finge_un_codigo_de_salida() {
    let _t = turno();
    con_base();

    // La propuesta que el operador NO ejecuta. Era el caso más interesante del
    // registro —alguien leyó el comando y decidió que no— y desaparecía al
    // cerrar la pestaña, así que «de lo que Lucy propone, cuánto se ejecuta» no
    // se podía calcular ni a posteriori.
    let motivo = "Cancelado — el operador detuvo la respuesta";
    lucy_core::audit::record(
        &lucy_core::audit::Entry::nueva("Stop-Computer -Force", "descartado").nota(motivo),
    )
    .expect("registrar");

    let filas = lucy_core::audit::query(&lucy_core::audit::Filter {
        source: Some("descartado".into()),
        ..Default::default()
    })
    .expect("consultar");

    let f = filas
        .iter()
        .find(|f| f.command == "Stop-Computer -Force")
        .expect("la fila del descartado no se escribió");
    assert_eq!(f.output_preview, motivo, "se perdió por qué no se ejecutó");
    assert_eq!(
        f.exit_code, None,
        "un comando que nadie ejecutó no puede tener código de salida: un 0 aquí diría que \
         terminó bien"
    );
}

#[test]
fn el_origen_distingue_lo_que_aprobo_una_persona_de_lo_que_corrio_solo() {
    let _t = turno();
    con_base();

    // «Un humano sancionó esto» es la señal de supervisión más valiosa que hay
    // en la aplicación, se conoce en el instante en que se lanza y es gratis.
    // Las dos ramas escribían `ai`, así que el registro no podía contestar
    // cuánto de lo que corre en esta máquina lo miró alguien.
    corrio("Get-Process -Name lsass", "", true);
    lucy_core::audit::record(
        &lucy_core::audit::Entry::nueva("Get-Process -Name lsass", "auto").resultado(true, 30, ""),
    )
    .expect("registrar");

    let aprobados = lucy_core::audit::query(&lucy_core::audit::Filter {
        source: Some("ai".into()),
        ..Default::default()
    })
    .expect("consultar");
    let solos = lucy_core::audit::query(&lucy_core::audit::Filter {
        source: Some("auto".into()),
        ..Default::default()
    })
    .expect("consultar");

    assert!(
        aprobados.iter().any(|f| f.command == "Get-Process -Name lsass"),
        "lo que aprobó una persona no se puede recuperar por origen"
    );
    assert!(
        solos.iter().any(|f| f.command == "Get-Process -Name lsass"),
        "lo que corrió el automático no se puede separar de lo aprobado"
    );
}

#[test]
fn cargar_un_skill_deja_constancia_de_que_se_uso() {
    let _t = turno();
    con_base();

    // EL OPERADOR INSTALA SKILLS Y NADIE SABIA CUALES SE USABAN. Lucy los
    // anuncia en el catálogo de cada turno, así que uno que no pide nadie sigue
    // costando su línea en cada petición, para siempre, y no había dato con el
    // que decidir apagarlo. Es la misma pregunta que la de las memorias:
    // ¿sirvió lo que cargué?
    let skills = vec![lucy_core::skills::Skill {
        name: "inventario-red".into(),
        description: "Levanta el inventario de la red".into(),
        body: "Corre Get-NetAdapter y resume.".into(),
        activo: true,
        dir: std::path::PathBuf::from("inventario-red"),
    }];

    let r = lucy_core::tools::run_with_skills("skill", "inventario-red", &skills)
        .expect("el skill no se resolvió");
    assert!(r.ok);

    let filas = lucy_core::audit::query(&lucy_core::audit::Filter {
        source: Some("skill".into()),
        ..Default::default()
    })
    .expect("consultar");

    let f = filas
        .iter()
        .find(|f| f.command == "inventario-red")
        .expect("cargar un skill no dejó rastro en ninguna parte");
    assert_eq!(
        f.exit_code, None,
        "cargar un skill no puede fallar ni tarda: un código de salida aquí afirmaría algo que \
         nadie ha comprobado"
    );

    // Y el que NO existe no deja fila: apuntar un nombre que el modelo se
    // inventó ensuciaría el recuento de uso con skills que no están.
    let antes = filas.len();
    let _ = lucy_core::tools::run_with_skills("skill", "no-existe", &skills);
    let despues = lucy_core::audit::query(&lucy_core::audit::Filter {
        source: Some("skill".into()),
        ..Default::default()
    })
    .expect("consultar")
    .len();
    assert_eq!(despues, antes, "se apuntó como usado un skill que no existe");
}

#[test]
fn el_resumen_contesta_supervision_y_aceptacion() {
    let _t = turno();
    con_base();

    // LAS DOS PREGUNTAS QUE LA LISTA CRONOLOGICA NO PUEDE CONTESTAR, y que
    // existen porque ayer se separaron los origenes. Escribir la señal y no
    // leerla es el mismo fallo que se ha ido cerrando por toda la casa — y este
    // me lo hice yo al añadir las fuentes sin agregarlas en ninguna parte.
    //
    // La base ya trae filas de los tests de arriba, asi que se mide el
    // MOVIMIENTO y no el valor absoluto: afirmar «supervision = 80 %» seria
    // probar el estado del fichero de test en vez del comportamiento.
    let antes = lucy_core::audit::resumen(30).expect("resumen");

    for _ in 0..6 {
        corrio("Get-Service -Name Spooler", "", true);
    }
    for _ in 0..2 {
        lucy_core::audit::record(
            &lucy_core::audit::Entry::nueva("Restart-Computer", "auto").resultado(true, 10, ""),
        )
        .expect("registrar");
    }
    for _ in 0..2 {
        lucy_core::audit::record(
            &lucy_core::audit::Entry::nueva("Format-Volume", "descartado").nota("cancelado"),
        )
        .expect("registrar");
    }

    let r = lucy_core::audit::resumen(30).expect("resumen");
    assert_eq!(r.aprobados, antes.aprobados + 6, "no cuenta lo que aprobo una persona");
    assert_eq!(r.solos, antes.solos + 2, "no separa lo que corrio solo");
    assert_eq!(r.descartados, antes.descartados + 2, "no cuenta lo descartado");

    // Las dos fracciones existen y estan en su rango.
    let sup = r.supervision().expect("con actividad tiene que haber supervision");
    let acp = r.aceptacion().expect("con propuestas tiene que haber aceptacion");
    assert!((0.0..=1.0).contains(&sup), "supervision fuera de rango: {sup}");
    assert!((0.0..=1.0).contains(&acp), "aceptacion fuera de rango: {acp}");

    // Y SE MUEVEN EN LA DIRECCION CORRECTA, que es lo que de verdad se prueba.
    // Dos automaticos mas y ningun aprobado extra tienen que BAJAR la
    // supervision; dos descartes mas, la aceptacion.
    let base = lucy_core::audit::resumen(30).expect("resumen");
    for _ in 0..20 {
        lucy_core::audit::record(
            &lucy_core::audit::Entry::nueva("Get-Date", "auto").resultado(true, 5, ""),
        )
        .expect("registrar");
    }
    let luego = lucy_core::audit::resumen(30).expect("resumen");
    assert!(
        luego.supervision().unwrap() < base.supervision().unwrap(),
        "veinte comandos que nadie miro no bajaron la supervision"
    );

    // El desglose por origen distingue lo que fue bien de lo que fallo, y no
    // cuenta como bueno lo que no se sabe.
    lucy_core::audit::record(&lucy_core::audit::Entry::nueva("cmd-sin-final", "manual"))
        .expect("registrar");
    let r = lucy_core::audit::resumen(30).expect("resumen");
    let (_, n, ok, mal) = r
        .por_origen
        .iter()
        .find(|(o, ..)| o == "manual")
        .expect("falta el origen manual")
        .clone();
    assert!(n > ok + mal, "un comando sin codigo de salida se conto como resuelto");
}

#[test]
fn sin_actividad_las_fracciones_no_se_inventan() {
    let _t = turno();
    con_base();

    // Cero de cero no es cero por ciento. Un `0 %` en la cabecera con la base
    // vacia se lee como «nada de lo que corre esta supervisado», que es una
    // acusacion falsa contra una instalacion recien puesta.
    let vacio = lucy_core::audit::Resumen::default();
    assert_eq!(vacio.supervision(), None);
    assert_eq!(vacio.aceptacion(), None);
}
