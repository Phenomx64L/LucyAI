//! Que se pueda contestar «¿cuánto me costó Lucy este mes?».
//!
//! ERA UNA PIEZA ENTERA MENOS LA LÍNEA QUE LA GUARDA. `pricing::cost` sabe
//! tarifar desde siempre, los tokens se cuentan turno a turno, el tope de gasto
//! funciona y apaga el automático — y todo eso vivía en el struct de cada
//! pestaña, en memoria. Al cerrar el programa desaparecía. Los datos viejos
//! tampoco servían: la tabla `token_usage` tiene casi mil filas que escribió la
//! app Tauri hasta el 21 de agosto y ahí se paró, porque la cara nueva no la
//! tocaba.
//!
//! Un solo `#[test]` con secciones, por el `OnceCell` del pool.

use lucy_core::usage::{self, Para};

fn arranca() {
    static UNA_VEZ: std::sync::Once = std::sync::Once::new();
    UNA_VEZ.call_once(|| {
        let p = std::env::temp_dir().join(format!(
            "lucy-usage-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|x| x.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_file(&p);
        lucy_core::init(&p).expect("init");
        usage::ensure_schema().expect("esquema");
    });
}

/// Un modelo que SÍ está en el catálogo de tarifas, para que el coste no salga
/// cero y las sumas signifiquen algo.
const TARIFADO: &str = "claude-sonnet-5";

#[test]
fn el_gasto_sobrevive_y_se_puede_desglosar() {
    arranca();

    // ── NADA APUNTADO, CERO Y SIN REVENTAR ──────────────────────────────────
    //
    // Es el estado de una instalación nueva, y la pantalla de Configuración lo
    // pide en cada visita.
    let r = usage::resumen(30).expect("resumen");
    assert_eq!(r.llamadas, 0);
    assert_eq!(r.total, 0.0);
    assert!(r.por_para.is_empty(), "un desglose de la nada tiene que venir vacío");

    // ── UNA LLAMADA SE APUNTA CON SU PRECIO ─────────────────────────────────
    usage::apunta(TARIFADO, 10_000, 500, Para::Chat, "7").expect("apuntar");
    let r = usage::resumen(30).expect("resumen");
    assert_eq!(r.llamadas, 1);
    assert_eq!(r.entrada, 10_000);
    assert_eq!(r.salida, 500);
    let esperado = lucy_core::pricing::cost(TARIFADO, 10_000, 500).expect("tarifa");
    assert!(
        (r.total - esperado).abs() < 1e-9,
        "el coste guardado no es el que dice la tarifa: {} vs {esperado}",
        r.total
    );

    // ── NI UNA FILA POR NADA ────────────────────────────────────────────────
    //
    // Un reintento que falló antes de salir, o una respuesta servida de caché,
    // no es gasto. Llenar la tabla de ceros haría que «cuántas llamadas hice»
    // dejara de significar algo.
    usage::apunta(TARIFADO, 0, 0, Para::Chat, "7").expect("apuntar");
    assert_eq!(usage::resumen(30).expect("resumen").llamadas, 1, "se apuntó una llamada vacía");

    // ── SIN PRECIO TAMBIÉN SE APUNTA ────────────────────────────────────────
    //
    // Un modelo local gasta cero dinero pero sí tokens, y borrar esas filas
    // dejaría el recuento de uso mintiendo. El coste va a cero, que es cierto.
    usage::apunta("llama3.2:3b", 4_000, 900, Para::Chat, "7").expect("apuntar");
    let r = usage::resumen(30).expect("resumen");
    assert_eq!(r.llamadas, 2, "una llamada sin tarifa no se apuntó");
    assert_eq!(r.entrada, 14_000, "sus tokens no se sumaron");
    assert!(
        (r.total - esperado).abs() < 1e-9,
        "un modelo sin tarifa añadió dinero de la nada"
    );

    // ── EL DESGLOSE POR PARA QUÉ, QUE ES LA MITAD ÚTIL ──────────────────────
    //
    // Saber que se gastaron doce dólares no sugiere nada. Saber que cuatro se
    // fueron en poner títulos sugiere titular con el modelo local — y la V1 no
    // lo podía decir porque escribía `ask_lucy_stream` en todas las filas.
    usage::apunta(TARIFADO, 2_000, 60, Para::Titulo, "7").expect("apuntar");
    usage::apunta(TARIFADO, 2_000, 60, Para::Titulo, "8").expect("apuntar");
    usage::apunta(TARIFADO, 30_000, 1_200, Para::Fork, "7").expect("apuntar");

    let r = usage::resumen(30).expect("resumen");
    let cubos: std::collections::HashMap<&str, (f64, usize)> =
        r.por_para.iter().map(|(k, c, n)| (k.as_str(), (*c, *n))).collect();
    assert_eq!(cubos.get("titulo").map(|x| x.1), Some(2), "los títulos no se cuentan aparte");
    assert_eq!(cubos.get("fork").map(|x| x.1), Some(1), "los sub-agentes no se cuentan aparte");
    assert_eq!(cubos.get("chat").map(|x| x.1), Some(2));

    // EL TOTAL CUADRA CON LA SUMA DEL DESGLOSE. Si no cuadrara —porque cada
    // consulta resolviera su propio corte de fecha con el reloj corriendo entre
    // ellas— la pantalla enseñaría dos cifras que no encajan por unos céntimos,
    // que es la clase de descuadre que hace desconfiar de todo lo demás.
    let suma: f64 = r.por_para.iter().map(|(_, c, _)| c).sum();
    assert!((suma - r.total).abs() < 1e-9, "el total {} no cuadra con el desglose {suma}", r.total);
    let suma_modelos: f64 = r.por_modelo.iter().map(|(_, c, _)| c).sum();
    assert!((suma_modelos - r.total).abs() < 1e-9, "el desglose por modelo no cuadra");

    // El más caro manda en la lista: es lo que se enseña primero.
    assert_eq!(r.por_para[0].0, "fork", "el desglose no viene por coste descendente");

    // ── DOS LLAMADAS DEL MISMO INSTANTE NO SE PISAN ─────────────────────────
    //
    // La clave primaria es TEXT porque así la dejó la V1. Con los nanosegundos
    // solos, un fork y su padre terminando en el mismo tic chocarían y la
    // segunda fila se perdería en silencio — que es la peor forma de perder
    // dinero de la factura.
    let antes = usage::resumen(30).expect("resumen").llamadas;
    for _ in 0..50 {
        usage::apunta(TARIFADO, 1, 1, Para::Chat, "9").expect("apuntar");
    }
    assert_eq!(
        usage::resumen(30).expect("resumen").llamadas,
        antes + 50,
        "se perdieron filas por choque de identificador"
    );

    // ── HOY ES UN SUBCONJUNTO DEL MES ───────────────────────────────────────
    //
    // `gasto_de_hoy` es lo que hace posible un tope diario: `spend_limit` es por
    // sesión y se reinicia en cada arranque, así que hoy no impide gastar diez
    // veces el tope abriendo Lucy diez veces.
    let hoy = usage::gasto_de_hoy();
    let mes = usage::resumen(30).expect("resumen").total;
    assert!(hoy > 0.0, "todo lo apuntado es de hoy y sale cero");
    assert!(hoy <= mes + 1e-9, "lo de hoy no puede pasar de lo del mes: {hoy} > {mes}");
}
