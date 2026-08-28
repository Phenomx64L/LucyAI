//! El historial de métricas de un equipo, en disco.
//!
//! LO QUE VIENE A ARREGLAR. El Dashboard enseña «CPU al 91 %» y una línea de
//! tendencia de los últimos cuarenta y cuatro segundos. Con eso no se puede
//! decidir nada: no distingue un pico de una hora punta ni de una fuga de
//! memoria que lleva subiendo desde el martes. La pregunta que un operador se
//! hace delante de ese número no es «cuánto es» —lo está viendo— sino «¿esto
//! es nuevo?», y hasta ahora la aplicación no tenía forma de contestarla.
//!
//! Se guarda una muestra por minuto y se conservan treinta días. Son 43.200
//! filas por equipo, unos dos megas: cabe al lado de las memorias sin que nadie
//! lo note, y treinta días es donde una tendencia deja de ser una anécdota.
//!
//! POR QUÉ NO SE GUARDA CADA SEGUNDO, que es a lo que se muestrea el panel. Una
//! fila por segundo son 2,6 millones al mes por equipo para responder una
//! pregunta que se hace en escala de días. El minuto es el grano más fino que
//! sigue siendo útil para lo que esto contesta.

/// Cada cuánto se guarda una muestra.
pub const CADA_SECS: i64 = 60;

/// Cuántos días se conservan.
pub const DIAS: i64 = 30;

/// Cuántas muestras hacen falta para atreverse a hablar de tendencia.
const MIN_MUESTRAS: usize = 30;

/// Y cuánto tienen que abarcar. Seis horas.
///
/// LAS DOS CONDICIONES, y no una. Treinta muestras seguidas son treinta minutos
/// y ahí no hay tendencia que valga: una compilación sube la CPU cuarenta
/// puntos en ese rato y saldría «sube 1.900 puntos al día». Y al revés, dos
/// muestras separadas por una semana abarcan de sobra y no dicen nada.
const MIN_SPAN: i64 = 6 * 3600;

/// Una lectura del equipo en un instante.
#[derive(Debug, Clone, PartialEq)]
pub struct Muestra {
    pub ts: i64,
    /// Porcentaje de CPU del equipo.
    pub cpu: f32,
    /// Porcentaje de RAM usada.
    pub mem: f32,
    /// `(punto de montaje, porcentaje de ocupación)`.
    pub discos: Vec<(String, f32)>,
}

/// Hacia dónde va una métrica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rumbo {
    Sube,
    Baja,
    Estable,
}

/// Lo que se puede decir de una serie, con la honradez incluida.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tendencia {
    pub rumbo: Rumbo,
    /// Puntos porcentuales por día. Negativo si baja.
    pub por_dia: f32,
    /// Cuánto abarca de verdad la serie, en segundos. Va en la respuesta
    /// porque «sube 2 puntos al día» significa una cosa medido sobre tres
    /// semanas y otra sobre siete horas, y quien lo lea tiene derecho a saber
    /// cuál de las dos está mirando.
    pub span_secs: i64,
    pub muestras: usize,
}

impl Tendencia {
    /// Cuántos días faltan para llegar a `objetivo` al ritmo actual.
    ///
    /// `None` si no va hacia allí. LA PREGUNTA DE VERDAD de un disco no es «al
    /// 87 %» sino «cuándo se llena», y esa se contesta con la pendiente. Un
    /// disco que sube medio punto al día se llena en veintiséis días y eso cabe
    /// en el plan de mantenimiento del mes; el mismo 87 % subiendo ocho puntos
    /// al día es una llamada esta tarde.
    pub fn dias_hasta(&self, actual: f32, objetivo: f32) -> Option<f32> {
        let falta = objetivo - actual;
        if self.por_dia.abs() < f32::EPSILON || falta.signum() != self.por_dia.signum() {
            return None;
        }
        let d = falta / self.por_dia;
        if d.is_finite() && d >= 0.0 {
            Some(d)
        } else {
            None
        }
    }
}

/// LA TABLA ES LA DE LA V1, y esto es la corrección de un error mío.
///
/// Este módulo creó `metric_samples` —singular— sin mirar que la aplicación
/// Tauri ya escribía `metrics_samples` —plural— en el MISMO fichero de base de
/// datos, cada cinco minutos, desde hace versiones. Dos tablas para el mismo
/// dato, en el mismo sitio, con un nombre que se diferencia en una letra.
///
/// LO QUE ESO PRODUCE ES PEOR QUE DUPLICAR FILAS. La V1 lleva meses de historial
/// real y la V2 empezaba de cero al lado, así que el Dashboard nativo decía «no
/// hay bastante para una tendencia» sobre un equipo del que sí había datos —los
/// tenía la otra tabla—. Y al revés: lo que mide el shell nativo no aparecía en
/// las proyecciones de capacidad de la V1. Cada mitad de Lucy sabía la mitad.
///
/// Gana la de la V1: tiene los datos. Lo único que le faltaba es el detalle POR
/// PUNTO DE MONTAJE —guarda un solo porcentaje de disco, el principal— y eso
/// entra como una columna más. Las filas que ya existen la tienen vacía, que es
/// exactamente lo que eran: muestras sin ese detalle.
pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS metrics_samples (
                 id            INTEGER PRIMARY KEY AUTOINCREMENT,
                 host_id       TEXT    NOT NULL DEFAULT '',
                 ts            INTEGER NOT NULL,
                 cpu           REAL    NOT NULL DEFAULT 0.0,
                 ram           REAL    NOT NULL DEFAULT 0.0,
                 disk          REAL    NOT NULL DEFAULT 0.0,
                 ram_mb        INTEGER NOT NULL DEFAULT 0,
                 disk_gb       INTEGER NOT NULL DEFAULT 0,
                 disk_total_gb INTEGER NOT NULL DEFAULT 0,
                 ram_total_mb  INTEGER NOT NULL DEFAULT 0,
                 is_hourly     INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_metrics_samples_ts
                 ON metrics_samples(host_id, ts DESC);
             CREATE INDEX IF NOT EXISTS idx_metrics_samples_hourly
                 ON metrics_samples(is_hourly, ts DESC);",
        )
        .map_err(|e| e.to_string())?;
        // La columna del detalle por punto de montaje. Sobre una base de la V1
        // la tabla ya existe sin ella, así que un `CREATE TABLE IF NOT EXISTS`
        // no la añade nunca: hace falta el ALTER, y hay que tragarse el error de
        // «ya existe» porque es el caso normal a partir de la segunda vez.
        let _ = c.execute(
            "ALTER TABLE metrics_samples ADD COLUMN discos TEXT NOT NULL DEFAULT ''",
            [],
        );
        rescata_las_huerfanas(c)?;
        Ok(())
    })
}

/// Se trae las muestras que quedaron en la tabla equivocada y la borra.
///
/// EL RESTO DE LA CORRECCIÓN DE ARRIBA. Cambiar a qué tabla se escribe arregló el
/// futuro y dejó el pasado tirado: en esta instalación quedaron 43 muestras
/// dentro de `metric_samples` —singular— que son mediciones de verdad, de un día
/// entero, y que ninguna consulta vuelve a mirar. Un módulo cuya razón de ser es
/// «¿esto es nuevo?» no puede permitirse tirar un día de historial por un
/// renombrado suyo.
///
/// EL MAPEO ES DIRECTO menos en una cosa: la tabla vieja guardaba `mem` y el
/// detalle por punto de montaje, pero no un porcentaje de disco suelto. Se saca
/// del propio detalle —`C:\=43.0`— porque está ahí, y ponerlo a cero habría
/// metido en la serie cuarenta y tres puntos de «disco al 0 %» que son mentira y
/// que además hundirían cualquier media.
///
/// CORRE UNA VEZ Y SE NOTA QUE CORRIÓ: al terminar, la tabla vieja se borra, así
/// que la segunda vez el `SELECT name FROM sqlite_master` no encuentra nada y
/// esto no hace ni una consulta. Sin ese borrado haría falta una marca aparte
/// para saber si ya se hizo, y la marca sería otra cosa que mantener.
///
/// LA DUPLICIDAD SE EVITA CON UN `WHERE NOT EXISTS` y no con un índice único
/// sobre `(host_id, ts)`. El índice sería más limpio, pero añadirlo ahora es
/// tocar el esquema de una tabla que lleva meses escribiendo la V1, y podría
/// fallar sobre filas suyas que no son mías. Esto hace lo mismo y no le cambia
/// la tabla a nadie.
fn rescata_las_huerfanas(c: &rusqlite::Connection) -> Result<(), String> {
    let existe: bool = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'metric_samples'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false);
    if !existe {
        return Ok(());
    }
    // El porcentaje del disco sale del texto `C:\=43.0`: se corta por el `=` y
    // se lee lo que va hasta la coma del siguiente punto de montaje, si lo hay.
    // `CAST` de un texto que no empiece por un número da 0.0 en SQLite, que es
    // lo mismo que había antes y no es peor.
    c.execute(
        "INSERT INTO metrics_samples (host_id, ts, cpu, ram, disk, discos)
         SELECT v.host_id, v.ts, v.cpu, v.mem,
                CAST(
                    CASE WHEN instr(v.discos, '=') > 0
                         THEN substr(v.discos, instr(v.discos, '=') + 1)
                         ELSE '0' END
                AS REAL),
                v.discos
         FROM metric_samples v
         WHERE NOT EXISTS (
             SELECT 1 FROM metrics_samples n
             WHERE n.ts = v.ts AND n.host_id = v.host_id
         )",
        [],
    )
    .map_err(|e| format!("history: rescatar muestras: {e}"))?;
    c.execute("DROP TABLE metric_samples", [])
        .map_err(|e| format!("history: borrar la tabla vieja: {e}"))?;
    Ok(())
}

/// Los discos, en una línea. `C:\=91.2` separados por tabulador.
///
/// TABULADOR Y NO PUNTO Y COMA. Un punto de montaje puede llevar casi cualquier
/// cosa —`/mnt/copias;viejas` es un nombre legal en Linux— y con un separador
/// que quepa dentro del dato, un volumen con el nombre equivocado parte la fila
/// en dos discos que no existen. Un tabulador no cabe en un punto de montaje.
fn escribe_discos(d: &[(String, f32)]) -> String {
    d.iter()
        .map(|(m, p)| format!("{m}={p:.1}"))
        .collect::<Vec<_>>()
        .join("\t")
}

fn lee_discos(s: &str) -> Vec<(String, f32)> {
    s.split('\t')
        .filter(|t| !t.is_empty())
        // `rsplit_once` y no `split_once`: el punto de montaje puede llevar un
        // `=` dentro, el porcentaje no.
        .filter_map(|t| t.rsplit_once('='))
        .filter_map(|(m, p)| p.parse::<f32>().ok().map(|p| (m.to_string(), p)))
        .collect()
}

/// Guarda una muestra y poda lo que se ha pasado de los treinta días.
///
/// La poda va en la MISMA llamada que la escritura. Separarlas deja la puerta
/// abierta a que alguien llame a una y no a la otra, y entonces la tabla crece
/// para siempre sin que nada avise.
pub fn guarda(host_id: &str, m: &Muestra) -> Result<(), String> {
    ensure_schema()?;
    crate::with_db(|c| {
        // `disk` SE RELLENA CON EL DISCO MÁS LLENO y no se deja en cero. Es la
        // columna que lee la V1 para sus proyecciones de capacidad, así que
        // escribir aquí sin ponerla metería filas que la otra mitad de Lucy lee
        // como «disco al 0 %» — y una serie con ceros intercalados no da una
        // tendencia mala, da una tendencia que baja.
        //
        // El más lleno y no el primero: es el que se va a llenar antes, que es
        // la pregunta que la columna contesta.
        let peor = m.discos.iter().map(|(_, p)| *p).fold(0.0_f32, f32::max);
        c.execute(
            "INSERT INTO metrics_samples (host_id, ts, cpu, ram, disk, discos)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                host_id,
                m.ts,
                m.cpu as f64,
                m.mem as f64,
                peor as f64,
                escribe_discos(&m.discos)
            ],
        )
        .map_err(|e| e.to_string())?;
        // LA PODA SOLO SE LLEVA LO SUYO. La V1 guarda medias horarias
        // (`is_hourly = 1`) durante noventa días, y este módulo conserva treinta:
        // sin el filtro, escribir una muestra desde el shell nativo borraría dos
        // meses del historial resumido de la otra mitad. Un borrado que nadie
        // pidió, disparado por una escritura de rutina.
        c.execute(
            "DELETE FROM metrics_samples
             WHERE host_id = ?1 AND ts < ?2 AND is_hourly = 0",
            rusqlite::params![host_id, m.ts - DIAS * 86_400],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

/// Las muestras de un equipo desde `desde`, de la más vieja a la más nueva.
pub fn serie(host_id: &str, desde: i64) -> Result<Vec<Muestra>, String> {
    ensure_schema()?;
    crate::with_db(|c| {
        // LAS HORARIAS FUERA. Una media de una hora y una muestra de un minuto
        // pesan lo mismo en una regresión, así que mezclarlas le da a cada hora
        // del historial largo de la V1 el peso de un minuto del reciente. La
        // pendiente que sale de ahí no es de nada.
        //
        // `disk` COMO RESPALDO de `discos`: las filas que escribió la V1 no
        // tienen el detalle por punto de montaje —esa columna es nueva— pero sí
        // el porcentaje del disco principal. Descartarlas dejaría a la V2 sin la
        // parte del historial que justamente venía a aprovechar.
        let mut st = c
            .prepare(
                "SELECT ts, cpu, ram, discos, disk FROM metrics_samples
                 WHERE host_id = ?1 AND ts >= ?2 AND is_hourly = 0
                 ORDER BY ts ASC",
            )
            .map_err(|e| e.to_string())?;
        let filas = st
            .query_map(rusqlite::params![host_id, desde], |r| {
                let detalle = lee_discos(&r.get::<_, String>(3)?);
                let discos = if detalle.is_empty() {
                    let pct = r.get::<_, f64>(4)? as f32;
                    // Sin nombre de montaje: no se sabe cuál era. Se le pone uno
                    // que lo diga en vez de inventarse `C:\`, que sería
                    // afirmar algo que la fila no contiene.
                    if pct > 0.0 { vec![("(principal)".to_string(), pct)] } else { Vec::new() }
                } else {
                    detalle
                };
                Ok(Muestra {
                    ts: r.get(0)?,
                    cpu: r.get::<_, f64>(1)? as f32,
                    mem: r.get::<_, f64>(2)? as f32,
                    discos,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();
        Ok(filas)
    })
}

/// La pendiente de una serie, o `None` si no hay con qué.
///
/// `umbral` son los puntos porcentuales al día a partir de los cuales se
/// considera que se mueve. Va por parámetro porque no significa lo mismo en
/// cada métrica: un disco que sube un punto al día se llena en tres meses y eso
/// es una noticia; una CPU que sube un punto al día es ruido de medida.
///
/// DEVUELVE `None` Y NO CERO cuando no hay datos suficientes. Un cero se lee
/// como «está estable», que es una afirmación sobre el equipo; lo que pasa de
/// verdad es que todavía no se sabe, y son cosas distintas.
pub fn tendencia(puntos: &[(i64, f32)], umbral: f32) -> Option<Tendencia> {
    if puntos.len() < MIN_MUESTRAS {
        return None;
    }
    let (t0, tn) = (puntos.first()?.0, puntos.last()?.0);
    let span = tn - t0;
    if span < MIN_SPAN {
        return None;
    }

    // Mínimos cuadrados sobre (días desde el principio, valor). En días y no en
    // segundos para que la pendiente ya salga en la unidad que se enseña.
    let n = puntos.len() as f64;
    let xs: Vec<f64> = puntos.iter().map(|(t, _)| (t - t0) as f64 / 86_400.0).collect();
    let ys: Vec<f64> = puntos.iter().map(|(_, v)| *v as f64).collect();
    let mx = xs.iter().sum::<f64>() / n;
    let my = ys.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for (x, y) in xs.iter().zip(&ys) {
        num += (x - mx) * (y - my);
        den += (x - mx) * (x - mx);
    }
    if den.abs() < f64::EPSILON {
        return None;
    }
    let por_dia = (num / den) as f32;
    let rumbo = if por_dia >= umbral {
        Rumbo::Sube
    } else if por_dia <= -umbral {
        Rumbo::Baja
    } else {
        Rumbo::Estable
    };
    Some(Tendencia { rumbo, por_dia, span_secs: span, muestras: puntos.len() })
}

/// A partir de cuántos puntos al día se considera que una métrica se mueve.
///
/// UNO POR MÉTRICA, y las diferencias son grandes a propósito:
///
/// · La CPU de un equipo de trabajo sube y baja con lo que se esté haciendo esa
///   semana. Por debajo de tres puntos al día eso es la agenda del operador, no
///   una tendencia del equipo, y decírselo sería ruido con aire de hallazgo.
/// · La RAM es más quieta, y una que sube despacio y no vuelve es el síntoma
///   clásico de una fuga. Ahí un punto al día ya merece mirarse.
/// · Un disco medio punto al día se llena en seis meses. Eso ENTRA en el plan
///   de mantenimiento, que es exactamente la decisión que esto quiere permitir.
const UMBRAL_CPU: f32 = 3.0;
const UMBRAL_MEM: f32 = 1.0;
const UMBRAL_DISCO: f32 = 0.2;

/// Lo que se sabe de un equipo mirando hacia atrás.
#[derive(Debug, Clone, Default)]
pub struct Resumen {
    pub cpu: Option<Tendencia>,
    pub mem: Option<Tendencia>,
    /// `(punto de montaje, tendencia)`. Solo los que dan para una.
    pub discos: Vec<(String, Tendencia)>,
    pub muestras: usize,
    /// Desde cuándo hay datos de verdad. `None` si no hay ninguno.
    pub desde_ts: Option<i64>,
}

/// El resumen de los últimos `dias` de un equipo.
///
/// ESTO LEE DE DISCO Y AJUSTA RECTAS. No se llama desde el pintado: son hasta
/// cuarenta y tres mil filas, y aunque salga en milisegundos, hacerlo sesenta
/// veces por segundo para enseñar una frase que cambia una vez al día es la
/// clase de gasto que no se ve hasta que se mira la batería.
pub fn resumen(host_id: &str, dias: i64, ahora: i64) -> Resumen {
    let desde = ahora - dias * 86_400;
    let Ok(s) = serie(host_id, desde) else { return Resumen::default() };
    if s.is_empty() {
        return Resumen::default();
    }
    let cpu: Vec<(i64, f32)> = s.iter().map(|m| (m.ts, m.cpu)).collect();
    let mem: Vec<(i64, f32)> = s.iter().map(|m| (m.ts, m.mem)).collect();

    // Los discos, cada uno con su serie. Un volumen que se conectó ayer tiene
    // menos muestras que el resto y `tendencia` lo descartará solo — que es lo
    // correcto: de un disco visto durante dos horas no se puede decir nada.
    let mut montajes: Vec<String> = Vec::new();
    for m in &s {
        for (mount, _) in &m.discos {
            if !montajes.iter().any(|x| x == mount) {
                montajes.push(mount.clone());
            }
        }
    }
    let mut discos = Vec::new();
    for mount in montajes {
        let serie_d: Vec<(i64, f32)> = s
            .iter()
            .filter_map(|m| {
                m.discos.iter().find(|(x, _)| *x == mount).map(|(_, p)| (m.ts, *p))
            })
            .collect();
        if let Some(t) = tendencia(&serie_d, UMBRAL_DISCO) {
            discos.push((mount, t));
        }
    }

    Resumen {
        cpu: tendencia(&cpu, UMBRAL_CPU),
        mem: tendencia(&mem, UMBRAL_MEM),
        discos,
        muestras: s.len(),
        desde_ts: s.first().map(|m| m.ts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serie_recta(n: usize, cada: i64, desde: f32, por_dia: f32) -> Vec<(i64, f32)> {
        (0..n)
            .map(|i| {
                let t = i as i64 * cada;
                (t, desde + por_dia * (t as f32 / 86_400.0))
            })
            .collect()
    }

    #[test]
    fn una_racha_corta_no_es_una_tendencia() {
        // EL FALLO QUE ESTO EVITA, y es el que convierte esta pantalla en un
        // mentiroso: media hora de compilación sube la CPU cuarenta puntos, y
        // extrapolar eso a un día da «sube 1.920 puntos al día». Un número así
        // no se lee como un error, se lee como una alarma.
        let corta = serie_recta(40, 60, 20.0, 0.0); // 40 min
        assert!(tendencia(&corta, 1.0).is_none(), "media hora no da tendencia");

        // Y pocas muestras muy separadas tampoco: abarcan de sobra y no hay
        // con qué ajustar una recta.
        let flaca = serie_recta(5, 86_400, 20.0, 1.0);
        assert!(tendencia(&flaca, 1.0).is_none(), "cinco puntos no son una serie");
    }

    #[test]
    fn la_pendiente_sale_en_puntos_por_dia() {
        // Tres días a una muestra por minuto, subiendo dos puntos al día.
        let s = serie_recta(3 * 1440, 60, 40.0, 2.0);
        let t = tendencia(&s, 1.0).expect("tres días dan de sobra");
        assert!((t.por_dia - 2.0).abs() < 0.01, "salió {}", t.por_dia);
        assert_eq!(t.rumbo, Rumbo::Sube);
        assert_eq!(t.span_secs, 3 * 86_400 - 60);
    }

    #[test]
    fn el_umbral_decide_que_es_moverse_y_cambia_por_metrica() {
        // Medio punto al día: en un disco es llenarse en seis meses —noticia—;
        // en la CPU es ruido de medida. La misma serie, dos lecturas, y por eso
        // el umbral es un parámetro y no una constante del módulo.
        let s = serie_recta(3 * 1440, 60, 40.0, 0.5);
        assert_eq!(tendencia(&s, 0.2).unwrap().rumbo, Rumbo::Sube, "para un disco, sube");
        assert_eq!(tendencia(&s, 2.0).unwrap().rumbo, Rumbo::Estable, "para la CPU, quieta");
    }

    #[test]
    fn una_serie_que_baja_no_predice_llenarse() {
        let s = serie_recta(3 * 1440, 60, 60.0, -1.0);
        let t = tendencia(&s, 0.2).unwrap();
        assert_eq!(t.rumbo, Rumbo::Baja);
        // Va en dirección contraria: no hay plazo que dar, y dar uno sería
        // inventárselo.
        assert!(t.dias_hasta(60.0, 90.0).is_none());
    }

    #[test]
    fn el_plazo_hasta_llenarse_es_lo_que_se_viene_a_preguntar() {
        // Al 87 % y subiendo medio punto al día: veintiséis días hasta el 100.
        // Cabe en el mantenimiento del mes. El mismo 87 % subiendo ocho puntos
        // al día es esta tarde, y son dos decisiones distintas.
        let lenta = tendencia(&serie_recta(3 * 1440, 60, 87.0, 0.5), 0.2).unwrap();
        let d = lenta.dias_hasta(87.0, 100.0).unwrap();
        assert!((d - 26.0).abs() < 1.0, "salió {d}");

        let rapida = tendencia(&serie_recta(3 * 1440, 60, 87.0, 8.0), 0.2).unwrap();
        let d = rapida.dias_hasta(87.0, 100.0).unwrap();
        assert!(d < 2.0, "salió {d}");
    }

    #[test]
    fn un_punto_de_montaje_con_signos_raros_sobrevive_a_la_ida_y_vuelta() {
        // El separador tiene que no caber en el dato. `/mnt/copias;viejas` es un
        // nombre legal, y con punto y coma se partiría en dos discos que no
        // existen — y el que no existe saldría en el informe.
        let d = vec![
            (r"C:\".to_string(), 91.2_f32),
            ("/mnt/copias;viejas".to_string(), 44.0),
            ("/mnt/a=b".to_string(), 7.5),
        ];
        let vuelta = lee_discos(&escribe_discos(&d));
        assert_eq!(vuelta.len(), 3, "se perdió o se partió alguno: {vuelta:?}");
        assert_eq!(vuelta[1].0, "/mnt/copias;viejas");
        assert_eq!(vuelta[2].0, "/mnt/a=b", "el `=` del nombre partió mal");
        assert!((vuelta[0].1 - 91.2).abs() < 0.05);
    }

    #[test]
    fn una_linea_rota_no_se_lleva_por_delante_a_las_demas() {
        // Una fila vieja escrita por otra versión, o media fila. Lo que se lee
        // se lee; lo que no, se salta. Devolver un error aquí dejaría el panel
        // entero sin historial por un carácter.
        let v = lee_discos("C:\\=50.0\tbasura\t\tD:\\=no\tE:\\=10.5");
        assert_eq!(v.len(), 2, "{v:?}");
        assert_eq!(v[0].0, r"C:\");
        assert_eq!(v[1].0, r"E:\");
    }
}
