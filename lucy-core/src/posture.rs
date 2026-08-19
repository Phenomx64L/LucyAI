//! La historia de los escaneos de compliance: qué cumplía ayer y qué cumple hoy.
//!
//! EL ESCANEO CONTESTA «QUÉ CUMPLE». Esto contesta la pregunta que de verdad
//! importa delante de un servidor: «¿QUÉ SE HA ROTO?». Un control que ya fallaba
//! la semana pasada es deuda conocida; uno que pasaba y hoy falla es una noticia,
//! y probablemente algo que alguien tocó el martes. Sin historia las dos cosas se
//! ven igual: una fila roja entre veinte.
//!
//! ES LO MISMO QUE `drift` HACE CON EL INVENTARIO, y a propósito: allí una foto
//! declarada buena y las diferencias contra ella; aquí la pasada anterior y las
//! diferencias contra ella. La forma de mirar un equipo es la misma, y merece
//! leerse igual en las dos pantallas.
//!
//! PERO SIN LÍNEA BASE QUE HAYA QUE DECLARAR. En el inventario tiene sentido
//! decir «así está bien» porque «qué hay instalado» no tiene un valor correcto;
//! en compliance sí lo tiene —el control pasa o no pasa— así que la referencia
//! es simplemente la vez anterior. Pedirle al operador que fije una línea base de
//! algo que ya trae su propio criterio sería un paso para nada.
//!
//! NO SE GUARDA LA EVIDENCIA. Es la salida entera de cada comando: kilobytes por
//! control, veinte controles por pasada, y una pasada por día. En un mes eso es
//! una base de datos que crece por algo que solo se mira en la última. Se guarda
//! el ESTADO, que es lo que se compara; la evidencia vive en la pasada de ahora,
//! que es la que se está viendo.

use crate::compliance::{Estado, Resultado, Severidad};

/// Cuántas pasadas se conservan por equipo.
///
/// Treinta es un mes de escaneos diarios: bastante para ver una tendencia y poco
/// para que la tabla importe. Lo que se compara es siempre contra la ANTERIOR,
/// así que las viejas no cambian nada de lo que se enseña — están para poder
/// mirar hacia atrás si algún día hay una vista que lo pida.
pub const MAX_PASADAS: usize = 30;

/// Una pasada guardada, sin la evidencia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pasada {
    pub ts: i64,
    /// `(id del control, estado)`, en el orden del catálogo.
    pub estados: Vec<(String, Estado)>,
}

impl Pasada {
    pub fn estado_de(&self, id: &str) -> Option<Estado> {
        self.estados.iter().find(|(i, _)| i == id).map(|(_, e)| *e)
    }
}

/// Qué le ha pasado a un control entre dos pasadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cambio {
    /// Pasaba y ya no. LA NOTICIA.
    Rompe,
    /// No pasaba y ahora sí.
    Arregla,
    /// Se medía y ahora no se puede, o al revés. No es lo mismo que romperse:
    /// un control que dejó de medirse no dice nada del equipo, dice que faltan
    /// permisos o que el comando ya no corre — y confundirlo con un fallo manda
    /// a arreglar lo que no está roto.
    SeDejaDeMedir,
    /// Ahora se mide y antes no.
    VuelveAMedirse,
    /// Nuevo en el catálogo: no había con qué compararlo.
    Nuevo,
}

/// Un control que cambió, con lo que hace falta para leerlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fila {
    pub id: String,
    pub titulo: String,
    pub severidad: Severidad,
    pub cambio: Cambio,
}

/// Compara la pasada de ahora con la anterior.
///
/// ORDENADO POR LO QUE DUELE: primero lo que se ha roto, y dentro de eso por
/// severidad. Un informe que lista los arreglos antes que las roturas obliga a
/// buscar la mala noticia entre las buenas.
///
/// LOS QUE SIGUEN IGUAL NO SALEN, ni los que fallaban y siguen fallando. Eso es
/// el escaneo, y ya está en la pantalla de al lado; aquí solo va la diferencia.
pub fn compara(antes: &Pasada, ahora: &[Resultado]) -> Vec<Fila> {
    let mut v: Vec<Fila> = Vec::new();
    for r in ahora {
        let previo = antes.estado_de(&r.check.id);
        let cambio = match (previo, r.estado) {
            // Sin pasado: es nuevo en el catálogo. Se dice, en vez de contarlo
            // como una rotura — un control añadido este mes que falla no lo ha
            // roto nadie.
            (None, _) => Cambio::Nuevo,
            (Some(a), b) if a == b => continue,
            (Some(Estado::Error), _) => Cambio::VuelveAMedirse,
            (Some(_), Estado::Error) => Cambio::SeDejaDeMedir,
            // POR GRAVEDAD Y NO POR «PASA O NO PASA». `Aviso` es un fallo de
            // gravedad media o baja, así que hay tres escalones y no dos: pasar
            // de aviso a fallo ES empeorar, y contarlo como «sigue mal» perdería
            // justo el movimiento que hay que mirar.
            (Some(a), b) if peor(b) > peor(a) => Cambio::Rompe,
            (Some(_), _) => Cambio::Arregla,
        };
        // Un control nuevo que pasa no es noticia: entra en el catálogo y ya.
        if cambio == Cambio::Nuevo && r.estado == Estado::Pasa {
            continue;
        }
        v.push(Fila {
            id: r.check.id.clone(),
            titulo: r.check.title.clone(),
            severidad: r.check.severity,
            cambio,
        });
    }
    // `Severidad` ya deriva `Ord` con lo crítico primero, así que se usa tal
    // cual: una segunda escala de gravedad aquí sería una que mantener y que
    // podría discrepar de la que ordena la tabla del escaneo.
    v.sort_by(|a, b| {
        orden(a.cambio)
            .cmp(&orden(b.cambio))
            .then(a.severidad.cmp(&b.severidad))
            .then(a.id.cmp(&b.id))
    });
    v
}

/// Cuánto de mal está un estado. `Error` queda fuera: no dice nada del equipo.
fn peor(e: Estado) -> u8 {
    match e {
        Estado::Pasa => 0,
        Estado::Aviso => 1,
        Estado::Falla => 2,
        Estado::Error => 0,
    }
}

fn orden(c: Cambio) -> u8 {
    match c {
        Cambio::Rompe => 0,
        Cambio::SeDejaDeMedir => 1,
        Cambio::Nuevo => 2,
        Cambio::VuelveAMedirse => 3,
        Cambio::Arregla => 4,
    }
}


// ── Persistencia ────────────────────────────────────────────────────────────

/// La tabla. `IF NOT EXISTS` sobre la conexión que ya está abierta.
///
/// TABLA PROPIA Y NO UNA COLUMNA EN OTRA. `inventory_baselines` guarda UNA foto
/// por equipo porque una línea base es una; aquí son muchas por equipo y con
/// fecha, que es otra forma. Meterlas en la misma tabla obligaría a que una de
/// las dos mintiera sobre lo que significa su fila.
pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS compliance_runs (
                 id       INTEGER PRIMARY KEY AUTOINCREMENT,
                 host_id  TEXT NOT NULL,
                 ts       INTEGER NOT NULL,
                 estados  TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_compliance_runs_host
                 ON compliance_runs(host_id, ts DESC);",
        )
        .map_err(|e| e.to_string())
    })
}

/// Cómo se guarda un estado. Una letra, estable.
///
/// UNA LETRA Y NO EL `Debug` DEL ENUM. `Debug` es una representación para
/// programadores y cambia si alguien renombra una variante; entonces las filas
/// viejas dejan de leerse y el informe dice que todo es nuevo. Una letra elegida
/// a mano es un formato, y los formatos se mantienen a propósito.
fn letra(e: Estado) -> &'static str {
    match e {
        Estado::Pasa => "p",
        Estado::Aviso => "a",
        Estado::Falla => "f",
        Estado::Error => "e",
    }
}

fn de_letra(s: &str) -> Option<Estado> {
    match s {
        "p" => Some(Estado::Pasa),
        "a" => Some(Estado::Aviso),
        "f" => Some(Estado::Falla),
        "e" => Some(Estado::Error),
        _ => None,
    }
}

/// Guarda una pasada y poda las viejas.
pub fn guarda(host_id: &str, ts: i64, rs: &[Resultado]) -> Result<(), String> {
    ensure_schema()?;
    let estados: Vec<serde_json::Value> = rs
        .iter()
        .map(|r| serde_json::json!([r.check.id, letra(r.estado)]))
        .collect();
    let json = serde_json::Value::Array(estados).to_string();
    crate::with_db(|c| {
        c.execute(
            "INSERT INTO compliance_runs (host_id, ts, estados) VALUES (?1, ?2, ?3)",
            rusqlite::params![host_id, ts, json],
        )
        .map_err(|e| e.to_string())?;
        // La poda va en la MISMA transacción que la inserción: si se hiciera
        // aparte y fallara, la tabla crecería sin que nada lo dijera.
        c.execute(
            "DELETE FROM compliance_runs
             WHERE host_id = ?1
               AND id NOT IN (
                   SELECT id FROM compliance_runs
                   WHERE host_id = ?1 ORDER BY ts DESC LIMIT ?2
               )",
            rusqlite::params![host_id, MAX_PASADAS as i64],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

/// La última pasada ANTERIOR a `ts`.
///
/// Anterior y no «la última»: quien llama acaba de guardar la de ahora, y
/// compararla consigo misma daría un informe siempre vacío. Pasar el corte por
/// parámetro evita depender del orden en que se llame a las dos funciones, que
/// es la clase de dependencia que se rompe al mover una línea.
pub fn anterior(host_id: &str, ts: i64) -> Result<Option<Pasada>, String> {
    ensure_schema()?;
    crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT ts, estados FROM compliance_runs
                 WHERE host_id = ?1 AND ts < ?2 ORDER BY ts DESC LIMIT 1",
            )
            .map_err(|e| e.to_string())?;
        let fila = st
            .query_row(rusqlite::params![host_id, ts], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .ok();
        Ok(fila.map(|(ts, json)| Pasada { ts, estados: lee_estados(&json) }))
    })
}

/// Cuántas pasadas hay guardadas de un equipo.
pub fn cuantas(host_id: &str) -> Result<usize, String> {
    ensure_schema()?;
    crate::with_db(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM compliance_runs WHERE host_id = ?1",
            rusqlite::params![host_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as usize)
        .map_err(|e| e.to_string())
    })
}

/// UNA FILA ILEGIBLE NO SE LLEVA LA PASADA ENTERA. Si un par viene mal escrito
/// se salta ese control y se conservan los demás: perder veinte comparaciones
/// por una es peor que comparar diecinueve.
fn lee_estados(json: &str) -> Vec<(String, Estado)> {
    let Ok(v) = serde_json::from_str::<Vec<Vec<String>>>(json) else {
        return Vec::new();
    };
    v.into_iter()
        .filter_map(|par| {
            let id = par.first()?.clone();
            let e = de_letra(par.get(1)?)?;
            Some((id, e))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compliance::{Check, Espera};

    fn res(id: &str, estado: Estado, sev: Severidad) -> Resultado {
        Resultado {
            check: Check {
                id: id.into(),
                title: format!("Control {id}"),
                category: "cis".into(),
                severity: sev,
                command: String::new(),
                espera: Espera::Salida(0),
                remediation: String::new(),
            },
            estado,
            evidencia: String::new(),
            exit_code: 0,
        }
    }

    fn pasada(pares: &[(&str, Estado)]) -> Pasada {
        Pasada {
            ts: 100,
            estados: pares.iter().map(|(i, e)| ((*i).to_string(), *e)).collect(),
        }
    }

    #[test]
    fn lo_que_se_rompio_va_primero() {
        // Un informe que lista los arreglos antes que las roturas obliga a
        // buscar la mala noticia entre las buenas.
        let antes = pasada(&[
            ("a", Estado::Pasa),
            ("b", Estado::Falla),
            ("c", Estado::Pasa),
        ]);
        let ahora = vec![
            res("a", Estado::Falla, Severidad::Low),
            res("b", Estado::Pasa, Severidad::High),
            res("c", Estado::Falla, Severidad::High),
        ];
        let v = compara(&antes, &ahora);
        assert_eq!(v.len(), 3);
        // Las dos roturas primero, y la de severidad alta antes.
        assert_eq!(v[0].id, "c");
        assert_eq!(v[0].cambio, Cambio::Rompe);
        assert_eq!(v[1].id, "a");
        assert_eq!(v[1].cambio, Cambio::Rompe);
        assert_eq!(v[2].cambio, Cambio::Arregla);
    }

    #[test]
    fn lo_que_no_cambia_no_sale() {
        // Ni lo que pasaba y sigue pasando, ni lo que fallaba y sigue fallando.
        // Eso es el escaneo, y está en la pantalla de al lado; aquí va la
        // diferencia.
        let antes = pasada(&[("a", Estado::Pasa), ("b", Estado::Falla)]);
        let ahora = vec![
            res("a", Estado::Pasa, Severidad::High),
            res("b", Estado::Falla, Severidad::High),
        ];
        assert!(compara(&antes, &ahora).is_empty());
    }

    #[test]
    fn dejar_de_medirse_no_es_romperse() {
        // Un control que dejó de medirse no dice nada del equipo: dice que
        // faltan permisos o que el comando ya no corre. Confundirlo con un fallo
        // manda a arreglar lo que no está roto.
        let antes = pasada(&[("a", Estado::Pasa)]);
        let ahora = vec![res("a", Estado::Error, Severidad::High)];
        let v = compara(&antes, &ahora);
        assert_eq!(v[0].cambio, Cambio::SeDejaDeMedir);
    }

    #[test]
    fn un_control_nuevo_que_falla_se_dice_pero_no_como_rotura() {
        // Nadie lo ha roto: acaba de entrar en el catálogo.
        let antes = pasada(&[("a", Estado::Pasa)]);
        let ahora = vec![
            res("a", Estado::Pasa, Severidad::High),
            res("nuevo", Estado::Falla, Severidad::High),
        ];
        let v = compara(&antes, &ahora);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].cambio, Cambio::Nuevo);
    }

    #[test]
    fn un_control_nuevo_que_pasa_no_es_noticia() {
        let antes = pasada(&[]);
        let ahora = vec![res("x", Estado::Pasa, Severidad::High)];
        assert!(compara(&antes, &ahora).is_empty());
    }

    #[test]
    fn una_fila_ilegible_no_se_lleva_la_pasada_entera() {
        // Perder veinte comparaciones por un par mal escrito es peor que
        // comparar diecinueve.
        let v = lee_estados(r#"[["a","p"],["b","ZZ"],["c","f"]]"#);
        assert_eq!(v.len(), 2, "{v:?}");
        assert_eq!(v[0], ("a".to_string(), Estado::Pasa));
        assert_eq!(v[1], ("c".to_string(), Estado::Falla));
        // Y un JSON roto entero devuelve nada, no un pánico.
        assert!(lee_estados("no soy json").is_empty());
    }

    #[test]
    fn las_letras_del_formato_van_y_vuelven() {
        // Se guardan a mano y no con `Debug` porque `Debug` cambia si alguien
        // renombra una variante, y entonces las filas viejas dejan de leerse y
        // el informe dice que todo es nuevo.
        for e in [Estado::Pasa, Estado::Aviso, Estado::Falla, Estado::Error] {
            assert_eq!(de_letra(letra(e)), Some(e), "{e:?} no vuelve");
        }
    }
}
