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

    /// El fichero de precios de la app real. `None` si no está al lado.
    ///
    /// EN EJECUCIÓN Y NO CON `include_str!`. Aunque esté dentro de
    /// `#[cfg(test)]`, la macro se resuelve al compilar, así que la batería del
    /// núcleo no compilaba sin el frontend de la V1 delante. Ver `models.rs`.
    fn js_fuente() -> Option<String> {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/model-pricing.ts"),
        )
        .ok()
    }

    /// Extrae `(id, in, out)` del fichero de la app.
    fn js_prices(js: &str) -> Vec<(String, f64, f64)> {
        let mut out = Vec::new();
        for line in js.lines() {
            let l = line.trim();
            if !l.starts_with('\'') || !l.contains("inputPer1K") {
                continue;
            }
            let id = l[1..].split('\'').next().unwrap_or("").to_string();
            let num = |campo: &str| -> Option<f64> {
                let at = l.find(campo)? + campo.len();
                l[at..]
                    .trim_start_matches([':', ' '])
                    .split(',')
                    .next()?
                    .trim()
                    .parse()
                    .ok()
            };
            if let (Some(i), Some(o)) = (num("inputPer1K"), num("outputPer1K")) {
                out.push((id, i, o));
            }
        }
        out
    }

    #[test]
    fn los_precios_no_se_han_desviado_de_los_de_la_app() {
        // Igual que el catálogo de modelos: el duplicado solo es legítimo
        // mientras esto lo vigile. Un precio viejo no rompe nada — informa mal,
        // que es la clase de fallo que nadie reporta porque nadie lo ve.
        // Sin la V1 al lado no hay con qué comparar. Se salta, no se falla.
        let Some(fuente) = js_fuente() else { return };
        let js = js_prices(&fuente);
        assert!(js.len() >= 30, "el parseo del .ts falló: {} entradas", js.len());
        for (id, i, o) in &PRICES.iter().map(|(a, b, c)| (a.to_string(), *b, *c)).collect::<Vec<_>>()
        {
            let Some((_, ji, jo)) = js.iter().find(|(jid, _, _)| jid == id) else {
                panic!("{id} está aquí y no en model-pricing.ts");
            };
            assert!((ji - i).abs() < 1e-9, "{id}: entrada {i} aquí, {ji} allí");
            assert!((jo - o).abs() < 1e-9, "{id}: salida {o} aquí, {jo} allí");
        }
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
