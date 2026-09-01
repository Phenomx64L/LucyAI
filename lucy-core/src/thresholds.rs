//! A partir de qué número se avisa, y por equipo.
//!
//! DOS PROBLEMAS DISTINTOS, y este módulo existe por los dos.
//!
//! El primero es que había TRES ESCALAS para el mismo dato. Un disco al 85 %
//! salía verde en su tarjeta KPI —el medidor solo tenía rojo a partir de 90—,
//! ámbar en la tarjeta del volumen —esa avisaba desde 80— y sin alerta ninguna
//! en la tira de arriba —esa empezaba en 86—. El mismo número, tres colores, en
//! la misma pantalla. Nada de eso estaba decidido: eran tres funciones escritas
//! en momentos distintos, cada una con su corte.
//!
//! El segundo es que los cortes buenos no son los mismos en todas las máquinas.
//! Un servidor de compilación al 90 % de CPU está haciendo su trabajo, y un
//! panel que lo pinta en rojo todas las tardes enseña a no mirarlo. Los de
//! fábrica son los que había; lo que cambia es que ahora se pueden mover para
//! el equipo donde estorban, en vez de para todos.

/// En qué banda cae una medida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Nivel {
    Ok,
    Aviso,
    Critico,
}

/// Los cortes de un equipo, en porcentaje.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Umbrales {
    pub cpu_aviso: f32,
    pub cpu_critico: f32,
    pub mem_aviso: f32,
    pub mem_critico: f32,
    pub disco_aviso: f32,
    pub disco_critico: f32,
}

impl Default for Umbrales {
    /// LOS QUE YA HABÍA, uno por uno.
    ///
    /// No se aprovecha para «mejorarlos»: son distintos por métrica a propósito
    /// —una CPU al 80 % es una máquina trabajando, un disco al 80 % es una
    /// máquina a la que le queda poco— y ese razonamiento sigue siendo bueno.
    /// Lo que estaba mal era tener tres copias de él y que no coincidieran.
    fn default() -> Self {
        Self {
            cpu_aviso: 78.0,
            cpu_critico: 90.0,
            mem_aviso: 82.0,
            mem_critico: 92.0,
            disco_aviso: 86.0,
            disco_critico: 93.0,
        }
    }
}

impl Umbrales {
    pub fn cpu(&self, pct: f32) -> Nivel {
        nivel(pct, self.cpu_aviso, self.cpu_critico)
    }
    pub fn mem(&self, pct: f32) -> Nivel {
        nivel(pct, self.mem_aviso, self.mem_critico)
    }
    pub fn disco(&self, pct: f32) -> Nivel {
        nivel(pct, self.disco_aviso, self.disco_critico)
    }

    /// Corrige lo que no tiene sentido en vez de rechazarlo.
    ///
    /// Un aviso por encima del crítico deja la banda de aviso vacía y el equipo
    /// salta de verde a rojo sin pasar por ámbar — que es justo el aviso
    /// temprano que se venía a dar. Se puede escribir así desde la interfaz sin
    /// querer, arrastrando un control, y negarse a guardar deja al operador con
    /// un formulario que no explica qué le pasa.
    pub fn sane(mut self) -> Self {
        for (a, c) in [
            (&mut self.cpu_aviso, &mut self.cpu_critico),
            (&mut self.mem_aviso, &mut self.mem_critico),
            (&mut self.disco_aviso, &mut self.disco_critico),
        ] {
            *a = a.clamp(1.0, 99.0);
            *c = c.clamp(1.0, 100.0);
            if *a >= *c {
                *a = (*c - 5.0).max(1.0);
            }
        }
        self
    }
}

pub fn nivel(v: f32, aviso: f32, critico: f32) -> Nivel {
    if v >= critico {
        Nivel::Critico
    } else if v >= aviso {
        Nivel::Aviso
    } else {
        Nivel::Ok
    }
}

pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS host_thresholds (
                 host_id      TEXT PRIMARY KEY,
                 cpu_aviso    REAL NOT NULL,
                 cpu_critico  REAL NOT NULL,
                 mem_aviso    REAL NOT NULL,
                 mem_critico  REAL NOT NULL,
                 disco_aviso  REAL NOT NULL,
                 disco_critico REAL NOT NULL
             );",
        )
        .map_err(|e| e.to_string())
    })
}

/// Los de un equipo, o los de fábrica si no tiene propios.
///
/// NO DEVUELVE `Result`. Quien llama a esto está pintando una pantalla, y un
/// fallo de base de datos no puede dejar el Dashboard sin colores: los de
/// fábrica son una respuesta correcta, y son exactamente lo que había antes de
/// que este módulo existiera.
pub fn de(host_id: &str) -> Umbrales {
    lee(host_id).unwrap_or_default()
}

fn lee(host_id: &str) -> Option<Umbrales> {
    ensure_schema().ok()?;
    crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT cpu_aviso, cpu_critico, mem_aviso, mem_critico,
                        disco_aviso, disco_critico
                 FROM host_thresholds WHERE host_id = ?1",
            )
            .map_err(|e| e.to_string())?;
        Ok(st
            .query_row(rusqlite::params![host_id], |r| {
                Ok(Umbrales {
                    cpu_aviso: r.get::<_, f64>(0)? as f32,
                    cpu_critico: r.get::<_, f64>(1)? as f32,
                    mem_aviso: r.get::<_, f64>(2)? as f32,
                    mem_critico: r.get::<_, f64>(3)? as f32,
                    disco_aviso: r.get::<_, f64>(4)? as f32,
                    disco_critico: r.get::<_, f64>(5)? as f32,
                })
            })
            .ok())
    })
    .ok()
    .flatten()
    .map(Umbrales::sane)
}

pub fn guarda(host_id: &str, u: &Umbrales) -> Result<(), String> {
    ensure_schema()?;
    let u = u.sane();
    crate::with_db(|c| {
        c.execute(
            "INSERT INTO host_thresholds
                 (host_id, cpu_aviso, cpu_critico, mem_aviso, mem_critico,
                  disco_aviso, disco_critico)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(host_id) DO UPDATE SET
                 cpu_aviso = ?2, cpu_critico = ?3, mem_aviso = ?4,
                 mem_critico = ?5, disco_aviso = ?6, disco_critico = ?7",
            rusqlite::params![
                host_id,
                u.cpu_aviso as f64,
                u.cpu_critico as f64,
                u.mem_aviso as f64,
                u.mem_critico as f64,
                u.disco_aviso as f64,
                u.disco_critico as f64
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

/// Devuelve un equipo a los de fábrica.
pub fn olvida(host_id: &str) -> Result<(), String> {
    ensure_schema()?;
    crate::with_db(|c| {
        c.execute("DELETE FROM host_thresholds WHERE host_id = ?1", rusqlite::params![host_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_de_fabrica_son_exactamente_los_que_ya_habia() {
        // Si esto cambia, cambia el comportamiento de TODOS los equipos que no
        // han tocado nada — que son todos, el día que esto se estrena. Un
        // módulo de umbrales configurables no es sitio para aprovechar y
        // reajustar de paso lo que ya funcionaba.
        let u = Umbrales::default();
        assert_eq!((u.cpu_aviso, u.cpu_critico), (78.0, 90.0));
        assert_eq!((u.mem_aviso, u.mem_critico), (82.0, 92.0));
        assert_eq!((u.disco_aviso, u.disco_critico), (86.0, 93.0));
    }

    #[test]
    fn un_mismo_numero_da_un_solo_nivel() {
        // LO QUE ESTE MÓDULO VIENE A ARREGLAR. Antes un disco al 85 % era verde
        // en la KPI, ámbar en su tarjeta y sin alerta en la tira: tres cortes
        // distintos escritos en tres sitios. Ahora hay uno.
        let u = Umbrales::default();
        assert_eq!(u.disco(85.0), Nivel::Ok);
        assert_eq!(u.disco(86.0), Nivel::Aviso);
        assert_eq!(u.disco(92.9), Nivel::Aviso);
        assert_eq!(u.disco(93.0), Nivel::Critico);
    }

    #[test]
    fn un_aviso_por_encima_del_critico_se_corrige_en_vez_de_rechazarse() {
        // Se escribe así arrastrando un control sin querer. Dejarlo pasa el
        // equipo de verde a rojo sin ámbar: desaparece justo el aviso temprano
        // que es la razón de tener dos cortes.
        let mala = Umbrales { disco_aviso: 97.0, disco_critico: 90.0, ..Default::default() };
        let s = mala.sane();
        assert!(s.disco_aviso < s.disco_critico, "{s:?}");
        // El crítico manda —es el que el operador escribió a conciencia— y el
        // aviso se coloca por debajo. Así que se comprueba DENTRO de la banda
        // nueva, no en el 92, que con el crítico en 90 es crítico y hace bien.
        assert_eq!(s.disco(87.0), Nivel::Aviso, "la banda de aviso tiene que existir");
        assert_eq!(s.disco(92.0), Nivel::Critico);
    }

    #[test]
    fn los_niveles_se_ordenan_de_menos_a_mas_grave() {
        // El Dashboard necesita quedarse con el PEOR de varios discos, y eso se
        // escribe con un `max` si el orden es el correcto. Al revés, el panel
        // enseñaría el disco más sano de la máquina como resumen.
        assert!(Nivel::Critico > Nivel::Aviso);
        assert!(Nivel::Aviso > Nivel::Ok);
        let peor = [Nivel::Ok, Nivel::Critico, Nivel::Aviso].into_iter().max().unwrap();
        assert_eq!(peor, Nivel::Critico);
    }
}
