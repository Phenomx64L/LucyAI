//! El calendario del mantenimiento: qué toca hacer y cuándo tocó por última vez.
//!
//! POR VENCIMIENTO PERSISTIDO Y NO POR TEMPORIZADOR, y esa es toda la pieza. La
//! V2 lanzaba un hilo con `sleep(172800)` — dormir cuarenta y ocho horas y
//! consolidar. Eso funciona en un servidor que no se apaga; Lucy corre en el
//! portátil de un administrador, que se cierra cada tarde. El hilo nunca llegaba
//! a despertar, así que la consolidación automática no es que fuera rara: no
//! ocurría. Y como nadie la echaba de menos —no deja rastro cuando no corre—,
//! llevaba así desde que se escribió.
//!
//! Aquí se anota en disco CUÁNDO se hizo cada cosa, y al arrancar se mira si ha
//! pasado el plazo. Un programa que estuvo cerrado tres días hace el trabajo al
//! abrirse, que es justo lo que un temporizador no puede hacer.
//!
//! Lo que NO hace: no lanza hilos ni sabe qué es consolidar. Eso es de quien
//! llama, y a propósito: la ventana quiere hacerlo en segundo plano y contarlo, y
//! un test quiere hacerlo en el sitio. Aquí solo vive el reloj.

/// Consolidar memorias duplicadas.
pub const CONSOLIDAR: &str = "consolidate";
/// Buscar patrones entre memorias.
pub const INSIGHTS: &str = "insights";

/// Cada cuánto se consolida.
///
/// Cuarenta y ocho horas, el plazo de la V2. No es un número medido —es una
/// convención— pero el criterio sí: consolidar es barato y no llama a ningún
/// modelo, así que el plazo lo pone el ruido que genera en la vista de memorias,
/// no el coste.
pub const CADA_CONSOLIDAR: i64 = 48 * 3_600;

/// Cada cuánto se reflexiona.
///
/// Un día. Más a menudo que consolidar aunque cueste más: los insights se
/// REFUERZAN al reencontrarse, así que la frecuencia es lo que convierte «una vez
/// me pasó» en «esto pasa siempre». Con un plazo largo, la confianza de un patrón
/// real tardaría meses en subir.
pub const CADA_INSIGHTS: i64 = 24 * 3_600;

pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS maintenance_runs (
                 job        TEXT PRIMARY KEY,
                 last_run   INTEGER NOT NULL,
                 last_note  TEXT NOT NULL DEFAULT ''
             );",
        )
        .map_err(|e| format!("maintenance: esquema: {e}"))
    })
}

fn ahora() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Cuándo corrió por última vez, y qué dejó dicho. `None` = nunca.
pub fn ultima(job: &str) -> Option<(i64, String)> {
    ensure_schema().ok()?;
    crate::with_db(|c| {
        c.query_row(
            "SELECT last_run, last_note FROM maintenance_runs WHERE job = ?1",
            rusqlite::params![job],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())
    })
    .ok()
}

/// La parte pura de la decisión, para poder discutirla sin base de datos ni
/// reloj.
///
/// «Nunca» es VENCIDO y no «empieza a contar ahora». La alternativa —anotar la
/// fecha en la primera comprobación sin hacer nada— retrasaría la primera pasada
/// de verdad un plazo entero en cada instalación nueva, en silencio. Que un
/// trabajo recién instalado corra enseguida es correcto: los dos que hay saben
/// mirar un corpus vacío y no hacer nada.
pub fn vencido(ultima: Option<i64>, cada: i64, ahora: i64) -> bool {
    match ultima {
        None => true,
        // Un reloj que va hacia atrás —cambio de hora, sincronización NTP— dejaba
        // `ahora - ultima` en negativo, y el trabajo no volvía a vencer hasta que
        // el reloj recuperase el terreno perdido. Se trata como vencido: repetir
        // una consolidación no rompe nada, y no volver a consolidar nunca sí.
        Some(t) if ahora < t => true,
        Some(t) => ahora - t >= cada,
    }
}

/// ¿Toca hacer este trabajo?
pub fn toca(job: &str, cada: i64) -> bool {
    vencido(ultima(job).map(|(t, _)| t), cada, ahora())
}

/// Anota que se hizo, con una línea de lo que salió.
///
/// SE ANOTA AUNQUE HAYA FALLADO, y es deliberado: si solo se anotaran los éxitos,
/// un Ollama caído dejaría el trabajo vencido para siempre y la ventana lo
/// reintentaría en cada comprobación, cada pocos minutos, mientras dure la avería.
/// La nota dice qué pasó, que es lo que hace falta para diagnosticarlo.
pub fn marca(job: &str, nota: &str) -> Result<(), String> {
    ensure_schema()?;
    let nota: String = nota.chars().take(300).collect();
    crate::with_db(|c| {
        c.execute(
            "INSERT INTO maintenance_runs (job, last_run, last_note) VALUES (?1, ?2, ?3)
             ON CONFLICT(job) DO UPDATE SET last_run = excluded.last_run,
                                           last_note = excluded.last_note",
            rusqlite::params![job, ahora(), nota],
        )
        .map_err(|e| format!("maintenance: marcar: {e}"))?;
        Ok(())
    })
}

/// Cuánto falta, en segundos. Cero o menos = vencido.
pub fn faltan(job: &str, cada: i64) -> i64 {
    match ultima(job) {
        None => 0,
        Some((t, _)) => (t + cada) - ahora(),
    }
}

/// Qué hizo una pasada, en una línea que se pueda enseñar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tanda {
    pub consolidado: Option<String>,
    pub reflexionado: Option<String>,
}

impl Tanda {
    pub fn hubo_algo(&self) -> bool {
        self.consolidado.is_some() || self.reflexionado.is_some()
    }
}

/// Corre UN trabajo, sin mirar el plazo, y lo anota. Devuelve la misma nota que
/// queda en disco.
///
/// APARTE DE `tanda` porque la vista de Memoria necesita un «ponte al día
/// ahora» por trabajo: esperar hasta dos días para ver si una corrección
/// funcionó no es una forma de verificar nada.
pub fn corre(job: &str, stop: &std::sync::atomic::AtomicBool) -> String {
    let nota = match job {
        CONSOLIDAR => match crate::consolidate::run(false) {
            Ok(r) => format!(
                "{} memorias miradas, {} grupos, {} fundidas",
                r.scanned, r.clusters_found, r.memories_merged
            ),
            Err(e) => format!("falló: {e}"),
        },
        INSIGHTS => {
            let r = crate::insights::run(stop);
            if r.creados + r.reforzados > 0 {
                format!(
                    "{} elegibles, {} grupos, {} patrones nuevos, {} reforzados",
                    r.elegibles, r.grupos, r.creados, r.reforzados
                )
            } else {
                // Sin patrones, lo que interesa es POR QUÉ. Un cero pelado es
                // indistinguible de una avería.
                format!("{} elegibles · {}", r.elegibles, r.motivo)
            }
        }
        otro => format!("trabajo desconocido: {otro}"),
    };
    let _ = marca(job, &nota);
    nota
}

/// Hace lo que toque. Bloqueante: quien llama ya está en un hilo.
///
/// LA CONSOLIDACIÓN CORRE DE VERDAD, no en seco, y eso hay que decirlo en voz
/// alta porque es una escritura desatendida sobre la memoria. Se sostiene por dos
/// cosas que ya estaban en el consolidador y no en este código: fundir marca la
/// columna `superseded_by` en vez de borrar la fila —nada se pierde, deja de
/// leerse—, y lo que tiene importancia 10 no se toca, que es la convención de
/// «fijado a mano».
pub fn tanda(stop: &std::sync::atomic::AtomicBool) -> Tanda {
    let mut t = Tanda::default();
    if toca(CONSOLIDAR, CADA_CONSOLIDAR) {
        t.consolidado = Some(corre(CONSOLIDAR, stop));
    }
    if stop.load(std::sync::atomic::Ordering::Relaxed) {
        return t;
    }
    if toca(INSIGHTS, CADA_INSIGHTS) {
        t.reflexionado = Some(corre(INSIGHTS, stop));
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    const AYER: i64 = 1_700_000_000;

    #[test]
    fn nunca_hecho_es_vencido_y_no_empieza_a_contar_ahora() {
        // La alternativa —anotar la fecha en la primera comprobación sin hacer
        // nada— retrasaría la primera pasada de verdad un plazo entero en cada
        // instalación nueva, en silencio.
        assert!(vencido(None, CADA_CONSOLIDAR, AYER));
    }

    #[test]
    fn el_plazo_se_cumple_justo_al_llegar() {
        assert!(!vencido(Some(AYER), 100, AYER + 99));
        assert!(vencido(Some(AYER), 100, AYER + 100));
        assert!(vencido(Some(AYER), 100, AYER + 10_000));
    }

    #[test]
    fn un_reloj_que_va_hacia_atras_no_congela_el_mantenimiento() {
        // Cambio de hora o sincronización NTP: `ahora - ultima` sale negativo y,
        // con la resta a secas, el trabajo no vuelve a vencer hasta que el reloj
        // recupere el terreno. Repetir una consolidación no rompe nada; no volver
        // a consolidar nunca, sí.
        assert!(vencido(Some(AYER), CADA_CONSOLIDAR, AYER - 86_400));
    }

    #[test]
    fn reflexionar_es_mas_frecuente_que_consolidar() {
        // Los insights se REFUERZAN al reencontrarse: la frecuencia es lo que
        // convierte «una vez me pasó» en «esto pasa siempre».
        assert!(CADA_INSIGHTS < CADA_CONSOLIDAR);
    }
}
