//! Lo que cuesta Lucy, apuntado en disco.
//!
//! LA PREGUNTA QUE NO SE PODÍA CONTESTAR. «¿Cuánto me costó Lucy este mes?» es
//! probablemente la primera que hace quien la pone en producción, y hasta ahora
//! no había forma de responderla ni aproximadamente:
//!
//! ```text
//!   los datos viejos   la tabla `token_usage` tiene 896 filas que escribió la
//!                      app Tauri entre el 27 de julio y el 21 de agosto, y ahí
//!                      se paró: la cara nueva no la toca. Están congeladas.
//!   los datos nuevos   `tokens_in` y `tokens_out` viven en el struct de cada
//!                      pestaña, en memoria. Al cerrar el programa desaparecen.
//! ```
//!
//! Y sin embargo TODO estaba construido: `pricing::cost` sabe tarifar, los
//! tokens se cuentan turno a turno, el tope de gasto funciona y apaga el
//! automático. Es la pieza entera menos la línea que la guarda — el mismo patrón
//! que ya apareció en `expires_at`, en `access_count` y en `exit_code`.
//!
//! El tope, además, es POR SESIÓN: se reinicia en cada arranque. Con esto en
//! disco, un tope diario o mensual pasa a ser una consulta.
//!
//! LA MISMA TABLA QUE LA V1, y a propósito. El esquema ya está creado en la base
//! del operador con casi mil filas dentro; escribir en otra tabla haría que su
//! historial de julio y agosto quedara huérfano justo cuando por fin hay algo
//! que lo lea. Las columnas que la V1 usaba para conceptos suyos —`task_id`,
//! `user`— se rellenan con lo que significan aquí o se dejan vacías, y queda
//! dicho cuál es cuál.
//!
//! `created_at` lo pone SQLite con `CURRENT_TIMESTAMP`, que es exactamente lo
//! que hizo la V1: dos programas con dos relojes escribiendo en la misma tabla
//! harían que el orden dependiera de quién escribió.

/// Para qué se gastó.
///
/// LO QUE LA V1 NO SEPARABA. Allí todas las filas ponían `ask_lucy_stream`, así
/// que el historial contesta cuánto se gastó pero no en qué — y esa es la mitad
/// interesante. Un mes en el que el 40 % del gasto se lo llevan los títulos de
/// las conversaciones no se arregla hablando menos con Lucy: se arregla
/// titulando con el modelo local.
/// SOLO LO QUE SE PUEDE LLENAR, y `Reflexion` volvió por eso. Estuvo fuera un
/// rato: `insights::destila_grupo` hablaba con Ollama y devolvía el texto
/// parseado tirando los recuentos, así que la variante no se podía escribir — y
/// una variante que nadie escribe es exactamente la pieza muerta que este módulo
/// existe para quitar. Ahora esa cadena devuelve tokens y la variante tiene
/// quien la llene.
///
/// TRES DE LOS CUATRO CUBOS SON DEL MODELO LOCAL y por tanto valen cero dólares.
/// Se guardan igual: «no cuesta dinero» y «no se está midiendo» no pueden
/// leerse iguales en una pantalla de coste, y cuántos tokens mastica Ollama para
/// poner títulos y destilar patrones es la única forma de decidir si compensa
/// tenerlo encendido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Para {
    /// Un turno de conversación: lo que el operador pidió.
    Chat,
    /// Ponerle nombre a una conversación.
    Titulo,
    /// Las sugerencias de la pantalla de inicio.
    Chips,
    /// Un sub-agente.
    Fork,
    /// La destilación de patrones del mantenimiento.
    Reflexion,
}

impl Para {
    /// La clave que se guarda. Estable: es lo que agrupa el resumen, así que
    /// cambiarla parte el historial en dos.
    pub fn clave(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Titulo => "titulo",
            Self::Chips => "chips",
            Self::Fork => "fork",
            Self::Reflexion => "reflexion",
        }
    }
}

pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        // MISMA DECLARACIÓN QUE LA DE LA V1, campo por campo. Si esta difiriera
        // —un INTEGER donde allí hay TEXT— qué tipo tienen las columnas
        // dependería de qué programa creó la base primero, y eso convierte
        // cualquier lectura en una ruleta. Ya pasó con `superseded_by`.
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS token_usage (
                 id            TEXT PRIMARY KEY,
                 task_id       TEXT NOT NULL,
                 timestamp     TEXT NOT NULL,
                 model         TEXT NOT NULL,
                 input_tokens  INTEGER NOT NULL DEFAULT 0,
                 output_tokens INTEGER NOT NULL DEFAULT 0,
                 total_cost    REAL NOT NULL DEFAULT 0.0,
                 user          TEXT NOT NULL,
                 request_type  TEXT NOT NULL,
                 created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE INDEX IF NOT EXISTS idx_token_usage_created
                 ON token_usage(created_at DESC);",
        )
        .map_err(|e| format!("usage: esquema: {e}"))
    })
}

/// Apunta lo que costó una llamada al modelo.
///
/// EN SILENCIO Y SIN PODER FALLAR HACIA ARRIBA: esto se llama al terminar cada
/// turno, y que la contabilidad reviente no puede impedir que el operador vea su
/// respuesta. Devuelve `Result` para que un test pueda mirarlo, pero quien llama
/// desde la interfaz lo ignora a propósito.
///
/// `conversacion` es lo que la V1 llamaba `task_id`. Aquí es la pestaña, que es
/// el equivalente honesto: permite preguntar «cuánto costó esta conversación»
/// sin inventarse un concepto que el shell nuevo no tiene.
pub fn apunta(
    modelo: &str,
    entrada: u32,
    salida: u32,
    para: Para,
    conversacion: &str,
) -> Result<(), String> {
    // NI UNA FILA POR NADA. Un turno que no gastó tokens —un reintento que falló
    // antes de salir, una respuesta servida de caché— no es gasto, y llenar la
    // tabla de ceros haría que «cuántas llamadas hice» dejara de significar algo.
    if entrada == 0 && salida == 0 {
        return Ok(());
    }
    ensure_schema()?;
    // SIN PRECIO TAMBIÉN SE APUNTA. Un modelo que no está en el catálogo
    // —Ollama, uno nuevo— gasta cero dinero pero sí tokens, y borrar esas filas
    // dejaría el recuento de uso mintiendo. El coste va a cero, que es cierto.
    let coste = crate::pricing::cost(modelo, entrada, salida).unwrap_or(0.0);
    let id = identificador();
    crate::with_db(|c| {
        c.execute(
            "INSERT INTO token_usage
                 (id, task_id, timestamp, model, input_tokens, output_tokens,
                  total_cost, user, request_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '', ?8)",
            rusqlite::params![
                id,
                conversacion,
                crate::audit::ahora_iso(),
                modelo,
                entrada as i64,
                salida as i64,
                coste,
                para.clave(),
            ],
        )
        .map_err(|e| format!("usage: apuntar: {e}"))?;
        Ok(())
    })
}

/// Un identificador único para la fila.
///
/// La columna es `TEXT PRIMARY KEY` porque así la dejó la V1 y no se toca. Los
/// nanosegundos solos no bastan: dos llamadas que terminan en el mismo tic del
/// reloj —un fork y su padre— chocarían y la segunda se perdería en silencio. El
/// contador lo cierra.
fn identificador() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{t:x}-{:x}", N.fetch_add(1, Ordering::Relaxed))
}

/// Lo que se gastó en un tramo, desglosado.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resumen {
    pub total: f64,
    pub llamadas: usize,
    pub entrada: u64,
    pub salida: u64,
    /// `(modelo, coste, llamadas)`, el más caro primero.
    pub por_modelo: Vec<(String, f64, usize)>,
    /// `(para qué, coste, llamadas)`, el más caro primero.
    pub por_para: Vec<(String, f64, usize)>,
}

/// El gasto de los últimos `dias`.
///
/// El corte lo hace SQLite con `datetime('now', '-N days')` y no un epoch
/// calculado aquí: `created_at` es TEXT en UTC porque así lo escribió la V1, y
/// comparar su formato con un número sería exactamente el desajuste de tipos que
/// este módulo evita en el esquema.
pub fn resumen(dias: i64) -> Result<Resumen, String> {
    ensure_schema()?;
    let dias = dias.clamp(1, 3_650);
    let corte = format!("-{dias} days");
    crate::with_db(|c| {
        // EL CORTE SE RESUELVE UNA VEZ y viaja como texto a las tres consultas.
        // Recalcular `datetime('now', …)` en cada una las dejaría midiendo
        // tramos ligeramente distintos —el reloj corre entre ellas— y el total
        // podría no cuadrar con la suma del desglose por unos céntimos, que es
        // la clase de descuadre que hace desconfiar de toda la pantalla.
        let desde: String = c
            .query_row("SELECT datetime('now', ?1)", [&corte], |f| f.get(0))
            .map_err(|e| format!("usage: corte: {e}"))?;

        let mut r = Resumen::default();
        c.query_row(
            "SELECT COALESCE(SUM(total_cost), 0.0), COUNT(*),
                    COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
             FROM token_usage WHERE created_at > ?1",
            [&desde],
            |f| {
                r.total = f.get(0)?;
                r.llamadas = f.get::<_, i64>(1)? as usize;
                r.entrada = f.get::<_, i64>(2)? as u64;
                r.salida = f.get::<_, i64>(3)? as u64;
                Ok(())
            },
        )
        .map_err(|e| format!("usage: resumen: {e}"))?;

        r.por_modelo = agrupa(c, "model", &desde)?;
        r.por_para = agrupa(c, "request_type", &desde)?;
        Ok(r)
    })
}

/// El desglose por una columna. `campo` NO viene de fuera: lo elige `resumen`
/// entre dos literales, así que interpolarlo no abre una inyección.
fn agrupa(
    c: &rusqlite::Connection,
    campo: &str,
    desde: &str,
) -> Result<Vec<(String, f64, usize)>, String> {
    let mut st = c
        .prepare(&format!(
            "SELECT {campo}, COALESCE(SUM(total_cost), 0.0), COUNT(*) FROM token_usage
             WHERE created_at > ?1 GROUP BY {campo} ORDER BY SUM(total_cost) DESC"
        ))
        .map_err(|e| format!("usage: desglose: {e}"))?;
    let v = st
        .query_map([desde], |f| {
            Ok((f.get(0)?, f.get::<_, f64>(1)?, f.get::<_, i64>(2)? as usize))
        })
        .map_err(|e| format!("usage: desglose: {e}"))?
        .filter_map(|x| x.ok())
        .collect();
    Ok(v)
}

/// Cuánto se lleva gastado HOY, en dinero.
///
/// Es la que hace falta para un tope diario: `spend_limit` es por sesión y se
/// reinicia en cada arranque, así que hoy no impide gastar diez veces el tope
/// abriendo Lucy diez veces.
pub fn gasto_de_hoy() -> f64 {
    if ensure_schema().is_err() {
        return 0.0;
    }
    crate::with_db(|c| {
        c.query_row(
            "SELECT COALESCE(SUM(total_cost), 0.0) FROM token_usage
             WHERE created_at >= datetime('now', 'start of day')",
            [],
            |f| f.get::<_, f64>(0),
        )
        .map_err(|e| e.to_string())
    })
    .unwrap_or(0.0)
}
