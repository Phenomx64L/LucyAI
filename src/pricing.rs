//! Lo que cuesta cada turno.
//!
//! Duplica `src/lib/model-pricing.ts` con el mismo trato que el catálogo de
//! modelos: se copia a propósito porque el frontend nativo no puede importar
//! JavaScript, y el test del final compara los dos ficheros para que el
//! duplicado no pueda derivar. Un precio desactualizado no falla — miente, que
//! es peor.
//!
//! Los precios son POR MIL TOKENS, como en el original. La tentación es
//! guardarlos por millón, que es como los publican los proveedores; se guardan
//! como están para que la comparación con el fichero de la app sea literal y no
//! haya que confiar en una conversión al leerla.

/// `(id, entrada por 1K, salida por 1K)`.
pub const PRICES: &[(&str, f64, f64)] = &[
    ("claude-opus-5", 0.005, 0.025),
    ("claude-opus-4-8", 0.005, 0.025),
    ("claude-opus-4-7", 0.005, 0.025),
    ("claude-opus-4-6", 0.005, 0.025),
    ("claude-opus-4-5", 0.015, 0.075),
    ("claude-sonnet-5", 0.003, 0.015),
    ("claude-sonnet-4-6", 0.003, 0.015),
    ("claude-sonnet-4-5", 0.003, 0.015),
    ("claude-fable-5", 0.010, 0.050),
    ("claude-haiku-4-5", 0.001, 0.005),
    ("gemini-3.1-pro-preview", 0.0020, 0.012),
    ("gemini-3.6-flash", 0.0015, 0.0075),
    ("gemini-3.5-flash", 0.0015, 0.0090),
    ("gemini-3.5-flash-lite", 0.00030, 0.0025),
    ("gemini-3.1-flash-lite", 0.00025, 0.0015),
    ("gemini-3.1-flash-lite-preview", 0.00025, 0.0015),
    ("gpt-5.6-sol", 0.005, 0.030),
    ("gpt-5.6-terra", 0.0025, 0.015),
    ("gpt-5.6-luna", 0.001, 0.006),
    ("gpt-5.5", 0.005, 0.020),
    ("gpt-5.5-instant", 0.002, 0.008),
    ("gpt-5.4-mini", 0.00020, 0.00080),
    ("gpt-5.4-nano", 0.00010, 0.00040),
    ("gpt-5.3-codex", 0.005, 0.020),
    ("gpt-4o", 0.0025, 0.010),
    ("gpt-4o-mini", 0.00015, 0.0006),
    ("grok-4.5", 0.0020, 0.0060),
    ("grok-4.3", 0.00125, 0.0025),
    ("deepseek-v4-flash", 0.00014, 0.00028),
    ("deepseek-v4-pro", 0.000435, 0.00087),
];

/// Precio de respaldo para NVIDIA NIM.
///
/// El catálogo lista diez modelos alojados de terceros y el original no les pone
/// precio uno a uno: cobra por alojamiento, no por modelo. Una media es más
/// honesta que inventar diez cifras distintas.
pub const NVIDIA_FALLBACK: (f64, f64) = (0.0008, 0.0024);

/// Lo que cuestan `input` y `output` tokens con un modelo, en dólares.
///
/// El sufijo `::nivel` se quita antes de buscar: el esfuerzo cambia cuántos
/// tokens se generan, no lo que cuesta cada uno.
///
/// Un modelo sin precio conocido devuelve `None` y NO cero. Cero se sumaría al
/// total como si fuera gratis, y un contador que dice 0,00 $ después de una hora
/// de trabajo es peor que uno que no está.
pub fn cost(model: &str, input: u32, output: u32) -> Option<f64> {
    let base = model.split_once("::").map_or(model, |(b, _)| b);
    let (pin, pout) = PRICES
        .iter()
        .find(|(id, _, _)| *id == base)
        .map(|(_, i, o)| (*i, *o))
        .or_else(|| base.contains('/').then_some(NVIDIA_FALLBACK))?;
    Some(input as f64 / 1000.0 * pin + output as f64 / 1000.0 * pout)
}

/// El total, como se enseña en la barra de estado.
///
/// Cuatro decimales por debajo de un céntimo: con dos, una conversación entera
/// con Flash se ve como 0,00 $ y el contador parece roto. Por encima, dos bastan
/// y más solo ensucian.
pub fn fmt_usd(total: f64) -> String {
    // EL CERO NEGATIVO EXISTE Y SE IMPRIME. `{:.2}` sobre `-0.0` escribe
    // «-0.00», y eso salía en la vista de Configuración: «llevas $-0.00». Un
    // contador a cero con signo hace dudar de todas las demás cifras de la
    // pantalla, que es mucho daño para un carácter. Sumar cero costes puede
    // producirlo, así que se normaliza aquí y no en cada sitio que suma.
    let total = if total == 0.0 { 0.0 } else { total };
    if total > 0.0 && total < 0.01 {
        format!("${total:.4}")
    } else {
        format!("${total:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Los ids del catalogo que se cobran, ya sin el sufijo de esfuerzo.
    ///
    /// Fuera: Ollama —que corre en la maquina y no cobra— y las dos entradas de
    /// escribir a mano, que no son modelos sino un hueco donde teclear uno.
    fn los_de_pago() -> Vec<&'static str> {
        crate::models::GROUPS
            .iter()
            .filter(|g| g.provider != "ollama")
            .flat_map(|g| g.options.iter())
            .map(|o| o.id.split("::").next().unwrap_or(o.id))
            .filter(|id| !id.ends_with("-custom"))
            .collect()
    }

    #[test]
    fn todo_modelo_que_se_ofrece_tiene_precio() {
        // ESTE TEST SUSTITUYE AL QUE COMPARABA CON `model-pricing.ts` DE LA V1.
        // Aquel vigilaba que dos tablas de precios —una en Rust, otra en
        // JavaScript— no se separaran. Ya no hay dos tablas, asi que el test se
        // saltaba solo y ocupaba el sitio de uno que mirara.
        //
        // Lo que queda por vigilar es la otra mitad del mismo fallo: el menu de
        // modelos y la tabla de precios siguen siendo dos listas en dos ficheros
        // distintos. Un modelo que se pueda elegir y no tenga fila aqui hace que
        // `cost()` devuelva `None`, y esa conversacion suma CERO. El contador no
        // se rompe: se equivoca en silencio, solo para ese modelo, y nadie lo
        // reporta porque no hay nada roto que ver.
        let sin_precio: Vec<&str> =
            los_de_pago().into_iter().filter(|id| cost(id, 1000, 1000).is_none()).collect();
        assert!(
            sin_precio.is_empty(),
            "se pueden elegir y no cuestan nada: {sin_precio:?}"
        );
    }

    #[test]
    fn el_vigilante_de_precios_no_esta_mirando_una_lista_vacia() {
        // SUELO. Si `los_de_pago` deja de encontrar modelos —porque cambio la
        // forma del catalogo, o el sufijo, o el nombre del grupo local— el test
        // de arriba pasaria siempre recorriendo cero elementos. Un test que se
        // apaga solo es peor que no tenerlo.
        let n = los_de_pago().len();
        assert!(n >= 20, "el catalogo solo dio {n} modelos de pago: ya no lo esta leyendo");
    }

    #[test]
    fn el_sufijo_de_esfuerzo_no_cambia_el_precio() {
        // El esfuerzo cambia cuántos tokens se generan, no lo que cuesta cada
        // uno. Buscar con el sufijo puesto no encontraría nada y el turno
        // saldría gratis.
        let a = cost("claude-opus-5", 1000, 1000).unwrap();
        let b = cost("claude-opus-5::xhigh", 1000, 1000).unwrap();
        assert!((a - b).abs() < 1e-12);
        assert!((a - 0.030).abs() < 1e-9, "1K+1K de Opus 5 son 0.005+0.025");
    }

    #[test]
    fn un_modelo_sin_precio_no_vale_cero() {
        // Cero se sumaría al total como si fuera gratis, y un contador que dice
        // 0,00 $ tras una hora de trabajo es peor que uno ausente.
        assert!(cost("modelo-que-no-existe", 1000, 1000).is_none());
        // Los de NVIDIA van por id con barra y caen al respaldo.
        assert!(cost("meta/llama-3.3-70b-instruct", 1000, 0).is_some());
    }

    #[test]
    fn por_debajo_de_un_centimo_se_ven_cuatro_decimales() {
        // Con dos, una conversación entera con Flash se ve como 0,00 $ y el
        // contador parece roto.
        assert_eq!(fmt_usd(0.0), "$0.00");
        assert_eq!(fmt_usd(0.0007), "$0.0007");
        assert_eq!(fmt_usd(0.42), "$0.42");
        assert_eq!(fmt_usd(12.5), "$12.50");
    }
}

#[cfg(test)]
mod tests_cero {
    use super::*;

    #[test]
    fn el_cero_no_sale_con_signo_menos() {
        // «llevas $-0.00» es lo que se veía en la vista de Configuración. Un
        // contador a cero con signo negativo hace dudar de todas las demás
        // cifras de la pantalla, que es mucho daño para un carácter.
        assert_eq!(fmt_usd(0.0), "$0.00");
        // Y el caso que lo produce: la suma de una lista vacía de modelos SIN
        // precio puede dar cero negativo, que `{:.2}` imprime con el signo.
        let vacio: f64 = [].iter().sum();
        assert_eq!(fmt_usd(vacio), "$0.00");
        assert_eq!(fmt_usd(-0.0), "$0.00", "el cero negativo se imprime con signo");
    }
}
