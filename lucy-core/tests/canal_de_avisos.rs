//! Que un aviso no se pierda aunque la pantalla no lo enseñe.
//!
//! ES LA MITAD DEL CANAL QUE SÍ SE PUEDE GARANTIZAR. Windows no dice si un toast
//! se entregó —está medido: `CreateToastNotifier` con un AUMID inexistente no
//! lanza, su `.Setting` dice «Enabled» y `.Show()` vuelve limpio, y el aviso no
//! llega a ninguna parte— así que lo único que puede prometer este módulo es que
//! el operador PUEDA enterarse. Eso es el registro, y eso es lo que se prueba
//! aquí.
//!
//! Un solo `#[test]` con secciones, por el `OnceCell` del pool.

use lucy_core::notify::{self, Aviso};
use lucy_core::thresholds::Nivel;

fn arranca() {
    static UNA_VEZ: std::sync::Once = std::sync::Once::new();
    UNA_VEZ.call_once(|| {
        let p = std::env::temp_dir().join(format!(
            "lucy-avisos-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|x| x.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_file(&p);
        lucy_core::init(&p).expect("init");
        notify::ensure_schema().expect("esquema");
    });
}

#[test]
fn el_registro_de_avisos() {
    arranca();

    // ── SIN AVISOS, CERO Y SIN REVENTAR ─────────────────────────────────────
    assert_eq!(notify::cuantos_sin_ver(), 0);
    assert!(notify::sin_ver(10).is_empty());
    assert_eq!(notify::ultimo_de("disco:C:"), None);

    // ── SE ANOTA AUNQUE LA PANTALLA NO LO ENSEÑE ────────────────────────────
    //
    // Es la decisión que ordena el módulo entero. Un toast se pierde por seis
    // motivos que no son culpa de nadie —concentración activada, sesión
    // bloqueada, AUMID sin registrar—, y si el toast fuera el único registro,
    // «Lucy no avisó» y «Lucy avisó y Windows se lo tragó» serían
    // indistinguibles. La primera vez que pasara lo segundo, el operador
    // dejaría de fiarse del vigilante entero.
    //
    // En los tests no hay escritorio interactivo, así que esto es justamente el
    // caso malo: se llama a `envia` y el aviso tiene que estar igual.
    let e = notify::envia(
        &Aviso::nuevo("Disco casi lleno", "C: al 94 %, subiendo desde el martes")
            .con_nivel(Nivel::Critico)
            .con_clave("disco:C:"),
    );
    let _ = e; // lo que diga la entrega da igual: el aviso tiene que estar

    assert_eq!(notify::cuantos_sin_ver(), 1, "el aviso no se anotó");
    let v = notify::sin_ver(10);
    assert_eq!(v[0].titulo, "Disco casi lleno");
    assert_eq!(v[0].nivel, Nivel::Critico, "el nivel no sobrevivió a la base");
    assert_eq!(v[0].clave, "disco:C:");

    // ── LA MEMORIA QUE EL CANAL LE PRESTA A QUIEN DECIDE ────────────────────
    //
    // El canal NO decide si algo merece decirse: no hay silencio nocturno ni
    // antirrepetición, porque un canal que se calla cosas por su cuenta es un
    // canal en el que no se confía. Lo único que aporta es saber cuándo se dijo
    // por última vez algo con esta clave — con eso, quien decide puede decidir.
    let (t, n) = notify::ultimo_de("disco:C:").expect("no recuerda cuándo lo dijo");
    assert!(t > 0);
    // EL NIVEL VIAJA CON LA FECHA. Sin él, «esto ha empeorado» y «esto se ha
    // arreglado» no se pueden preguntar, y esas dos son media política del
    // vigilante.
    assert_eq!(n, Nivel::Critico, "no recuerda en qué estado lo dijo");
    assert_eq!(notify::ultimo_de("cpu"), None, "una clave que no se ha usado no tiene fecha");
    assert_eq!(notify::ultimo_de(""), None, "la clave vacía no puede casar con todo");

    // ── VISTO ES DEL OPERADOR, NO DEL SISTEMA ───────────────────────────────
    let id = v[0].id;
    notify::marca_visto(Some(id)).expect("marcar");
    assert_eq!(notify::cuantos_sin_ver(), 0);
    // Y sigue estando: marcarlo visto no lo borra.
    assert!(notify::ultimo_de("disco:C:").is_some());

    // ── LA PODA NO SE LLEVA LO QUE NADIE HA VISTO ───────────────────────────
    //
    // Un aviso sin ver es algo que el operador todavía no sabe, y borrarlo por
    // antiguo sería decidir por él que ya no importa.
    notify::envia(&Aviso::nuevo("Sin ver", "esto no lo ha leído nadie").con_clave("x"));
    lucy_core::with_db(|c| {
        // Los dos al pasado, lejos del corte.
        c.execute("UPDATE avisos SET ts = strftime('%s','now') - 99*86400", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .expect("envejecer");

    let fuera = notify::prune(30).expect("podar");
    assert_eq!(fuera, 1, "la poda se llevó algo que no era el visto");
    assert_eq!(notify::cuantos_sin_ver(), 1, "la poda borró un aviso que nadie había visto");

    // Un plazo de cero o negativo no vacía la tabla: sería demasiado fácil
    // perderlo todo con un cero de más en la configuración.
    assert_eq!(notify::prune(0).expect("podar"), 0);
    assert_eq!(notify::prune(-5).expect("podar"), 0);

    // ── MARCAR TODOS ────────────────────────────────────────────────────────
    notify::marca_visto(None).expect("marcar todos");
    assert_eq!(notify::cuantos_sin_ver(), 0);
}
