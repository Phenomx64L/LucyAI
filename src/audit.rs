//! El registro de auditoría: qué comando corrió, dónde, cómo acabó.
//!
//! LA TABLA EXISTÍA Y EL SHELL NATIVO NO ESCRIBÍA EN ELLA. `audit_trail` la
//! creó la app Tauri y la llena `NexShellView`; el shell egui ejecuta comandos
//! por tres caminos —el paso de un plan del agente, el NexShell local y el
//! remoto— y no dejaba constancia de ninguno. El visor de logs de la V2 lee esa
//! tabla, así que migrarlo tal cual habría dado un panel que enseña lo que hizo
//! la aplicación VIEJA y nada de lo que hace la nueva: un registro de auditoría
//! con un agujero exactamente del tamaño del programa que lo enseña. Y sin dar
//! error — dando vacío, que es la peor forma de fallar.
//!
//! LA MISMA BASE Y LA MISMA TABLA, no una propia. Partir el historial en dos
//! ficheros obligaría a fusionarlos en la vista para contestar «qué se hizo en
//! este equipo», que es la única pregunta que un registro de auditoría existe
//! para contestar. Los dos programas hablan por el mismo pool de `lucy-core`,
//! con WAL y `busy_timeout`, que es justo la configuración que hace que dos
//! escritores sobre un SQLite sea una cosa aburrida y no un problema.
//!
//! AQUÍ ESTÁ EL MECANISMO Y NO LA POLÍTICA, como en `logs.rs`: este módulo sabe
//! escribir y leer filas. Qué merece una fila —y qué recorte lleva -- lo decide
//! quien ejecuta.

/// Cuánto se guarda de la salida de un comando.
///
/// Lo mismo que escribe la app Tauri, y por su mismo motivo: esto es una pista
/// para reconocer la fila, no un archivo de salidas. Un listado recursivo son
/// megabytes y hay una fila por comando de toda la vida de la instalación.
pub const MAX_PREVIEW: usize = 500;

/// Cuántas filas devuelve una consulta como mucho.
///
/// El visor pinta las que quepan y filtra en memoria; traer cien mil para
/// enseñar doscientas es trabajo de disco que nadie mira.
pub const MAX_LIMIT: i64 = 2_000;

/// Una fila del registro.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: i64,
    /// ISO 8601. Es lo que escribe la app y lo que se enseña si no hay
    /// `created_at` — se conserva por compatibilidad con las filas que ya hay.
    pub timestamp: String,
    /// Vacío = este equipo.
    pub host_id: String,
    pub host_name: String,
    pub command: String,
    /// QUIÉN DECIDIÓ ESTO, que es media razón de ser de la tabla:
    ///
    /// ```text
    ///   manual      lo escribió el operador en la terminal
    ///   ai          lo propuso Lucy y una persona le dio al botón
    ///   auto        lo lanzó el bucle automático sin que nadie lo mirara
    ///   descartado  Lucy lo propuso y NUNCA llegó a ejecutarse
    ///   runbook · compliance · broadcast   lo disparó una rutina
    /// ```
    ///
    /// `auto` y `descartado` son nuevos, y los dos existen por lo mismo: la
    /// diferencia entre lo que una persona sancionó y lo que no. Antes `ai`
    /// cubría el aprobado y el automático a la vez, y lo descartado no se
    /// escribía en ninguna parte — o sea que el caso más interesante, el
    /// operador que lee un comando y decide que no, desaparecía al cerrar la
    /// pestaña.
    pub source: String,
    /// `None` = no se sabe. NO se pone 0, que significa «fue bien».
    pub exit_code: Option<i32>,
    pub duration_ms: Option<i64>,
    pub output_preview: String,
    pub user: String,
    /// Epoch en SEGUNDOS. Lo pone SQLite por defecto.
    pub created_at: i64,
}

impl Entry {
    /// Una fila nueva con lo mínimo, lista para `record`.
    ///
    /// `created_at` va a cero y lo rellena la base: dos programas escribiendo en
    /// la misma tabla con dos relojes distintos harían que el orden de la vista
    /// dependiera de quién escribió, y `ORDER BY created_at DESC` dejaría de
    /// significar «lo último que pasó».
    pub fn nueva(command: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            id: 0,
            timestamp: ahora_iso(),
            host_id: String::new(),
            host_name: "local".into(),
            command: command.into(),
            source: source.into(),
            exit_code: None,
            duration_ms: None,
            output_preview: String::new(),
            user: String::new(),
            created_at: 0,
        }
    }

    pub fn en_equipo(mut self, id: &str, nombre: &str) -> Self {
        self.host_id = id.to_string();
        self.host_name = nombre.to_string();
        self
    }

    /// Cómo acabó. `ok` se traduce a 0 o 1 porque es lo que la vista sabe leer:
    /// el código de salida real no siempre se puede recuperar de un
    /// `Invoke-Command`, y guardar `None` cuando SÍ se sabe si fue bien perdería
    /// el dato que de verdad se usa para colorear la fila.
    pub fn resultado(mut self, ok: bool, ms: u64, salida: &str) -> Self {
        self.exit_code = Some(if ok { 0 } else { 1 });
        self.duration_ms = Some(ms as i64);
        self.output_preview = recorta(salida, MAX_PREVIEW);
        self
    }

    /// POR QUÉ QUEDÓ ASÍ, sin afirmar cómo acabó.
    ///
    /// `resultado` es para lo que corrió: escribe `exit_code` y `duration_ms`.
    /// Esto es para lo que NO corrió, donde el motivo es el único dato que hay y
    /// el código de salida tiene que quedarse en `None` — ver la nota del campo:
    /// `None` es «no se sabe», y un cero aquí sería afirmar que un comando que
    /// nadie ejecutó terminó bien.
    pub fn nota(mut self, texto: &str) -> Self {
        self.output_preview = recorta(texto, MAX_PREVIEW);
        self
    }
}

/// Recorta por CARACTERES y no por bytes.
///
/// Cortar un `String` por un índice de byte que cae en medio de un carácter
/// multibyte es un pánico, y la salida de un comando en español lleva acentos en
/// casi cada línea. Es el mismo cuidado que ya se toma `agent::clip`.
fn recorta(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

pub(crate) fn ahora_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Sin dependencia de fechas: el crate no tiene ninguna y meterla para
    // formatear una cadena que la base ya deriva de `created_at` sería pagar un
    // árbol de dependencias por comodidad. El formato es el que espera la vista.
    let dias = secs / 86_400;
    let resto = secs % 86_400;
    let (a, m, d) = civil_de_dias(dias as i64);
    format!(
        "{a:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        resto / 3600,
        (resto % 3600) / 60,
        resto % 60
    )
}

/// Días desde el epoch a (año, mes, día). Algoritmo de Howard Hinnant.
pub(crate) fn civil_de_dias(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Qué filas se piden.
#[derive(Debug, Clone)]
pub struct Filter {
    /// `None` = de todos los equipos. `Some("")` = solo las de este equipo.
    pub host_id: Option<String>,
    pub source: Option<String>,
    pub limit: i64,
}

impl Default for Filter {
    fn default() -> Self {
        Self { host_id: None, source: None, limit: 200 }
    }
}

/// Crea la tabla si no está.
///
/// HACE FALTA, aunque la app Tauri ya la cree. `lucy_core::init` lo dice en su
/// propio comentario: abre el pool sobre una `lucy.db` EXISTENTE, y el esquema
/// lo pone la app. En una máquina donde la Tauri nunca haya arrancado —que es
/// hacia donde va esta migración— el visor daría «no such table: audit_trail» en
/// vez de un registro vacío.
///
/// El DDL es COPIA EXACTA del de la app, columnas, defectos e índices. Que dos
/// programas creen la misma tabla con dos formas distintas es cómo se llega a
/// que uno inserte una columna que el otro no sabe leer.
pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_trail (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 timestamp   TEXT    NOT NULL,
                 host_id     TEXT    NOT NULL DEFAULT '',
                 host_name   TEXT    NOT NULL DEFAULT 'local',
                 command     TEXT    NOT NULL,
                 source      TEXT    NOT NULL DEFAULT 'manual',
                 exit_code   INTEGER,
                 duration_ms INTEGER,
                 output_preview TEXT NOT NULL DEFAULT '',
                 user        TEXT    NOT NULL DEFAULT '',
                 created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
             );
             CREATE INDEX IF NOT EXISTS idx_audit_trail_created ON audit_trail(created_at DESC);
             CREATE INDEX IF NOT EXISTS idx_audit_trail_host    ON audit_trail(host_id, created_at DESC);
             CREATE INDEX IF NOT EXISTS idx_audit_trail_source  ON audit_trail(source, created_at DESC);",
        )
        .map_err(|e| format!("audit: no se pudo crear el esquema: {e}"))
    })
}

/// Escribe una fila. Devuelve su id.
///
/// UN SOLO `INSERT`, sin transacción explícita. SQLite envuelve cada sentencia
/// suelta en una transacción que nace ya como escritora, así que no hay
/// promoción de lectora a escritora — que es el caso en el que `busy_timeout` NO
/// se respeta y el otro proceso se lleva un `SQLITE_BUSY` inmediato. Con dos
/// programas escribiendo en la misma base, esa diferencia es la que decide si
/// una fila se pierde en silencio.
pub fn record(e: &Entry) -> Result<i64, String> {
    crate::with_db(|c| {
        c.execute(
            "INSERT INTO audit_trail
               (timestamp, host_id, host_name, command, source, exit_code, duration_ms,
                output_preview, user)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                e.timestamp,
                e.host_id,
                e.host_name,
                e.command,
                e.source,
                e.exit_code,
                e.duration_ms,
                e.output_preview,
                e.user,
            ],
        )
        .map_err(|err| format!("audit: no se pudo registrar: {err}"))?;
        Ok(c.last_insert_rowid())
    })
}

/// Las filas más recientes primero.
pub fn query(f: &Filter) -> Result<Vec<Entry>, String> {
    let limit = f.limit.clamp(1, MAX_LIMIT);
    // El filtrado por texto NO se hace aquí a propósito: la vista filtra en
    // memoria sobre lo ya traído, y así el contador de niveles y la caja de
    // búsqueda responden sin ir al disco en cada tecla.
    let mut sql = String::from(
        "SELECT id, timestamp, host_id, host_name, command, source, exit_code, \
         duration_ms, output_preview, user, created_at FROM audit_trail",
    );
    let mut donde: Vec<&str> = Vec::new();
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(h) = &f.host_id {
        donde.push("host_id = ?");
        args.push(Box::new(h.clone()));
    }
    if let Some(s) = &f.source {
        donde.push("source = ?");
        args.push(Box::new(s.clone()));
    }
    if !donde.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&donde.join(" AND "));
    }
    sql.push_str(" ORDER BY created_at DESC, id DESC LIMIT ?");
    args.push(Box::new(limit));

    crate::with_db(|c| {
        let mut st = c.prepare(&sql).map_err(|e| format!("audit: {e}"))?;
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let filas = st
            .query_map(refs.as_slice(), |r| {
                Ok(Entry {
                    id: r.get(0)?,
                    timestamp: r.get(1)?,
                    host_id: r.get(2)?,
                    host_name: r.get(3)?,
                    command: r.get(4)?,
                    source: r.get(5)?,
                    exit_code: r.get(6)?,
                    duration_ms: r.get(7)?,
                    output_preview: r.get(8)?,
                    user: r.get(9)?,
                    created_at: r.get(10)?,
                })
            })
            .map_err(|e| format!("audit: {e}"))?;
        filas.collect::<Result<Vec<_>, _>>().map_err(|e| format!("audit: {e}"))
    })
}

/// Cuántas veces falló ESTE MISMO comando en ESTE MISMO equipo últimamente.
///
/// LA SEÑAL YA ESTABA EN DISCO. `exit_code` y `duration_ms` se escriben en cada
/// fila desde hace versiones, con índice por fecha y por equipo, y no los
/// agregaba nadie: el visor enseña una lista cronológica, que contesta «qué pasó
/// el martes» y no «esto viene fallando». Así que Lucy podía proponer un comando
/// que ya había fallado tres veces en esta máquina esta semana sin saberlo, y el
/// operador tampoco tenía cómo enterarse salvo recordándolo.
///
/// COINCIDENCIA EXACTA, y es deliberado. Lo tentador es cortar por el cmdlet
/// —contar todos los `Get-Service` que fallaron— pero eso mezcla `-Name spooler`
/// con `-Name w3svc`, que son dos preguntas distintas sobre dos servicios
/// distintos: el primero puede llevar semanas roto sin que eso diga nada del
/// segundo. Un aviso que se dispara por parecido es un aviso que se aprende a
/// ignorar. Esto acierta menos veces y no se equivoca ninguna.
///
/// El equipo entra en la clave por la misma razón: un comando que falla en un
/// servidor y funciona en otro es información sobre el servidor.
///
/// `exit_code IS NOT NULL` deja fuera lo que no se sabe cómo acabó —la terminal
/// local, los pasos descartados— que no es lo mismo que haber ido bien.
pub fn fallos_recientes(command: &str, host_id: &str, days: i64) -> Result<usize, String> {
    if command.trim().is_empty() || days <= 0 {
        return Ok(0);
    }
    crate::with_db(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM audit_trail
             WHERE command = ?1 AND host_id = ?2 AND exit_code IS NOT NULL AND exit_code != 0
               AND created_at > (strftime('%s','now') - ?3 * 86400)",
            rusqlite::params![command, host_id, days],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as usize)
        .map_err(|e| format!("audit: fallos: {e}"))
    })
}

/// La ventana por defecto de `fallos_recientes`.
///
/// Dos semanas: lo bastante largo para que un problema que se arrastra se note,
/// lo bastante corto para que lo que ya se arregló deje de avisar. No está
/// medido — cuando haya meses de uso se decide con datos.
pub const DIAS_FALLOS: i64 = 14;

/// Borra lo anterior a `days` días. Devuelve cuántas filas se fueron.
///
/// Una fila por comando durante toda la vida de la instalación crece sin techo,
/// y a diferencia de los carriles del workspace esto vive en disco y sobrevive a
/// los reinicios. `days <= 0` no borra nada: sería una forma demasiado fácil de
/// vaciar el registro entero con un cero de más en la configuración.
pub fn prune(days: i64) -> Result<usize, String> {
    if days <= 0 {
        return Ok(0);
    }
    crate::with_db(|c| {
        c.execute(
            "DELETE FROM audit_trail WHERE created_at < (strftime('%s','now') - ?1 * 86400)",
            rusqlite::params![days],
        )
        .map_err(|e| format!("audit: no se pudo podar: {e}"))
    })
}

/// El nivel con el que se pinta una fila.
///
/// EL CÓDIGO DE SALIDA MANDA, y solo si no dice nada se mira el texto. Es lo que
/// hace la V2 y es lo correcto: un comando que terminó mal es un error aunque su
/// salida sea amable, y uno que terminó bien no deja de estarlo por mencionar la
/// palabra «error» en un listado de ficheros.
pub fn level_of(e: &Entry) -> crate::logs::Level {
    if let Some(c) = e.exit_code {
        if c != 0 {
            return crate::logs::Level::Error;
        }
        // Salió bien: solo puede ser aviso, nunca error. Sin este corte, un
        // `Get-ChildItem` que lista un fichero llamado `error.log` sale en rojo.
        return match crate::logs::Level::sniff(&e.output_preview) {
            crate::logs::Level::Warn => crate::logs::Level::Warn,
            _ => crate::logs::Level::Info,
        };
    }
    crate::logs::Level::sniff(&format!("{} {}", e.command, e.output_preview))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_salida_larga_se_recorta_sin_partir_un_acento() {
        // Cortar por índice de byte en medio de un carácter multibyte es un
        // pánico, y la salida de un comando en español lleva acentos en casi
        // cada línea.
        let e = Entry::nueva("Get-Service", "ai").resultado(true, 12, &"ñ".repeat(MAX_PREVIEW + 50));
        assert_eq!(e.output_preview.chars().count(), MAX_PREVIEW + 1);
        assert!(e.output_preview.ends_with('…'));
        assert!(e.output_preview.starts_with('ñ'));
    }

    #[test]
    fn lo_que_cabe_no_se_toca_ni_se_marca() {
        let e = Entry::nueva("Get-Date", "manual").resultado(true, 3, "martes");
        assert_eq!(e.output_preview, "martes");
    }

    #[test]
    fn no_saberlo_no_es_lo_mismo_que_haber_ido_bien() {
        // `exit_code: None` significa que no se sabe. Poner 0 diría que fue
        // bien, y una fila que miente sobre eso es peor que una fila incompleta.
        let sin = Entry::nueva("algo", "ai");
        assert_eq!(sin.exit_code, None);
        assert_eq!(sin.duration_ms, None);
        let con = sin.clone().resultado(false, 900, "falló");
        assert_eq!(con.exit_code, Some(1));
        assert_eq!(con.duration_ms, Some(900));
    }

    #[test]
    fn el_reloj_lo_pone_la_base_y_no_el_programa() {
        // Dos programas escribiendo en la misma tabla con dos relojes distintos
        // harían que el orden dependiera de quién escribió, y
        // `ORDER BY created_at DESC` dejaría de significar «lo último que pasó».
        assert_eq!(Entry::nueva("x", "ai").created_at, 0);
    }

    #[test]
    fn el_codigo_de_salida_manda_sobre_el_texto() {
        // Un comando que terminó mal es un error aunque su salida sea amable.
        let malo = Entry::nueva("Get-Service", "ai").resultado(false, 10, "todo correcto");
        assert_eq!(level_of(&malo), crate::logs::Level::Error);

        // Y uno que terminó bien no deja de estarlo por listar un fichero que se
        // llama error.log — es el falso positivo que tiñe de rojo media pantalla.
        let bueno = Entry::nueva("Get-ChildItem", "ai").resultado(true, 10, "error.log  4 KB");
        assert_eq!(level_of(&bueno), crate::logs::Level::Info);

        // Pero un aviso en una salida correcta sí se ve: es el caso para el que
        // existe el nivel de en medio.
        let aviso = Entry::nueva("Get-Disk", "ai").resultado(true, 10, "Advertencia: 2 GB libres");
        assert_eq!(level_of(&aviso), crate::logs::Level::Warn);
    }

    #[test]
    fn sin_codigo_de_salida_se_mira_el_comando_tambien() {
        // Las filas que ya hay en la base pueden no traerlo; la fila sigue
        // teniendo que colorearse con algo mejor que «info» por defecto.
        let mut e = Entry::nueva("systemctl status nginx", "manual");
        e.output_preview = "failed to start".into();
        assert_eq!(level_of(&e), crate::logs::Level::Error);
    }

    #[test]
    fn la_fecha_iso_es_la_que_espera_la_vista() {
        // El formato lo lee la V2 con `.slice(11, 19)` para sacar la hora, así
        // que las posiciones importan: cambiarlo rompe una vista que no está en
        // este repositorio.
        let s = ahora_iso();
        assert_eq!(s.len(), 20, "{s}");
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], "T");
        assert_eq!(&s[13..14], ":");
        assert!(s.ends_with('Z'), "{s}");
        // Y la parte que la vista recorta es la hora, no otra cosa.
        let hora = &s[11..19];
        assert_eq!(hora.matches(':').count(), 2, "{hora}");
    }

    #[test]
    fn el_calendario_no_se_inventa_los_dias() {
        // Un desfase de un día en el registro de auditoría es la diferencia
        // entre «esto pasó el viernes» y «esto pasó el jueves», que es justo lo
        // que alguien va a discutir.
        assert_eq!(civil_de_dias(0), (1970, 1, 1));
        assert_eq!(civil_de_dias(19_723), (2024, 1, 1));
        // Un 29 de febrero, que es donde fallan los calendarios caseros.
        assert_eq!(civil_de_dias(19_782), (2024, 2, 29));
    }

    #[test]
    fn podar_con_cero_dias_no_vacia_el_registro() {
        // Sería una forma demasiado fácil de borrarlo entero con un cero de más
        // en la configuración. Y no toca la base: se corta antes.
        assert_eq!(prune(0), Ok(0));
        assert_eq!(prune(-5), Ok(0));
    }

    #[test]
    fn el_limite_de_una_consulta_esta_acotado_por_arriba_y_por_abajo() {
        assert_eq!(Filter::default().limit, 200);
        // No se prueba contra la base —no hay uno en un test unitario— pero sí
        // que el recorte es el que se documenta.
        assert_eq!(MAX_LIMIT.clamp(1, MAX_LIMIT), MAX_LIMIT);
        assert_eq!(0_i64.clamp(1, MAX_LIMIT), 1);
        assert_eq!((MAX_LIMIT + 5_000).clamp(1, MAX_LIMIT), MAX_LIMIT);
    }
}
