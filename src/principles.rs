//! Principios: las reglas que Lucy aplica SIEMPRE.
//!
//! No son memorias. Una memoria es un hecho —«el servidor de impresión es
//! SRV-04»— y entra en el prompt cuando viene al caso. Un principio es una
//! instrucción —«en producción nunca reinicies un servicio sin avisarme antes»—
//! y entra SIEMPRE, venga o no al caso, porque su valor está justamente en los
//! turnos donde a nadie se le habría ocurrido recordarla.
//!
//! POR ESO NO PASAN POR LA BÚSQUEDA SEMÁNTICA. Meterlas por el mismo camino que
//! las memorias significaría que la regla sobre producción solo aparece cuando la
//! pregunta ya menciona producción — o sea, cuando ya no hacía falta.
//!
//! SON POCOS A PROPÓSITO. Un prompt con cuarenta reglas no tiene ninguna: el
//! modelo las promedia y acaba siguiendo las que suenan más fuerte. El tope está
//! puesto y se dice cuando se alcanza, en vez de aceptar la número cuarenta y uno
//! y diluir las cuarenta anteriores en silencio.

/// Cuántos principios entran en el prompt.
///
/// Doce. Por encima de eso el bloque compite con las instrucciones de Lucy en vez
/// de matizarlas, y el modelo empieza a elegir cuáles seguir.
pub const MAX_ACTIVOS: usize = 12;

/// Tope de longitud de una regla.
///
/// Un principio es una instrucción, no un procedimiento. Lo que no cabe en dos
/// líneas es un skill —que se carga cuando viene al caso— y no una regla que va
/// a viajar en todos los turnos del resto de la vida de la instalación.
pub const MAX_REGLA: usize = 400;

#[derive(Debug, Clone, PartialEq)]
pub struct Principio {
    pub id: i64,
    pub nombre: String,
    pub regla: String,
    /// `None` = para todo. Si no, el id de un equipo.
    pub ambito: Option<String>,
    /// Menor = se aplica antes y se enseña antes.
    pub prioridad: i64,
    pub activo: bool,
}

pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS principles (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 name        TEXT NOT NULL,
                 rule        TEXT NOT NULL,
                 scope       TEXT,
                 priority    INTEGER NOT NULL DEFAULT 100,
                 enabled     INTEGER NOT NULL DEFAULT 1,
                 created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                 updated_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
             );
             CREATE INDEX IF NOT EXISTS idx_principles_enabled ON principles(enabled, priority);
             CREATE INDEX IF NOT EXISTS idx_principles_scope ON principles(scope);",
        )
        .map_err(|e| format!("principles: esquema: {e}"))
    })
}

/// Guarda una regla. Devuelve su id.
///
/// La regla pasa por el mismo filtro de secretos que una memoria: es texto que el
/// operador dicta y el modelo transcribe, y va a viajar en TODOS los prompts
/// siguientes — que es el peor sitio donde puede acabar un token.
pub fn add(nombre: &str, regla: &str, ambito: Option<&str>) -> Result<i64, String> {
    let nombre = nombre.trim();
    let regla = crate::memories::scrub(regla.trim());
    if regla.is_empty() {
        return Err("Un principio necesita una regla.".into());
    }
    if regla.chars().count() > MAX_REGLA {
        return Err(format!(
            "Esa regla ocupa {} caracteres y el tope son {MAX_REGLA}. Lo que no cabe en dos \
             líneas es un procedimiento —guárdalo como skill— y no una regla que va a viajar \
             en todos los turnos.",
            regla.chars().count()
        ));
    }
    ensure_schema()?;
    let nombre = if nombre.is_empty() {
        // Sin nombre, las primeras palabras de la regla. Un principio sin
        // etiqueta es imposible de discutir después: «quita el tercero» no
        // identifica nada.
        regla.split_whitespace().take(5).collect::<Vec<_>>().join(" ")
    } else {
        nombre.to_string()
    };
    crate::with_db(|c| {
        c.execute(
            "INSERT INTO principles (name, rule, scope) VALUES (?1, ?2, ?3)",
            rusqlite::params![nombre, regla, ambito],
        )
        .map_err(|e| format!("principles: alta: {e}"))?;
        Ok(c.last_insert_rowid())
    })
}

/// Los principios, en el orden en que se aplican.
pub fn list() -> Result<Vec<Principio>, String> {
    ensure_schema()?;
    crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT id, name, rule, scope, priority, enabled
                 FROM principles ORDER BY priority ASC, id ASC",
            )
            .map_err(|e| format!("principles: listar: {e}"))?;
        let v = st
            .query_map([], |r| {
                Ok(Principio {
                    id: r.get(0)?,
                    nombre: r.get(1)?,
                    regla: r.get(2)?,
                    ambito: r.get(3)?,
                    prioridad: r.get(4)?,
                    activo: r.get::<_, i64>(5)? != 0,
                })
            })
            .map_err(|e| format!("principles: listar: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })
}

/// Enciende o apaga uno. No se borra: un principio desactivado sigue siendo la
/// prueba de que alguien lo pensó, y volver a activarlo es un clic.
pub fn set_enabled(id: i64, activo: bool) -> Result<(), String> {
    crate::with_db(|c| {
        c.execute(
            "UPDATE principles SET enabled = ?1, updated_at = strftime('%s','now') WHERE id = ?2",
            rusqlite::params![i64::from(activo), id],
        )
        .map_err(|e| format!("principles: cambiar: {e}"))?;
        Ok(())
    })
}

pub fn delete(id: i64) -> Result<(), String> {
    crate::with_db(|c| {
        c.execute("DELETE FROM principles WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("principles: borrar: {e}"))?;
        Ok(())
    })
}

/// El bloque para el prompt. Vacío = no hay ninguno activo.
///
/// `ambito` filtra los que son de un equipo concreto: un principio sobre el
/// controlador de dominio no tiene por qué gobernar una conversación sobre la
/// impresora. Los globales entran siempre.
pub fn render(ambito: Option<&str>) -> String {
    let Ok(todos) = list() else { return String::new() };
    let activos: Vec<&Principio> = todos
        .iter()
        .filter(|p| p.activo)
        .filter(|p| match (&p.ambito, ambito) {
            (None, _) => true,
            (Some(s), _) if s.is_empty() => true,
            (Some(s), Some(a)) => s == a,
            (Some(_), None) => false,
        })
        .take(MAX_ACTIVOS)
        .collect();
    if activos.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    for (i, p) in activos.iter().enumerate() {
        let tag = match &p.ambito {
            Some(a) if !a.is_empty() => format!(" (solo en {a})"),
            _ => String::new(),
        };
        s.push_str(&format!("[P{}]{tag} {}\n", i + 1, p.regla.trim()));
    }
    // EN SILENCIO. Sin esta línea, el modelo recita las reglas al principio de
    // cada respuesta para demostrar que las ha leído, y una conversación entera
    // se convierte en un acuse de recibo por turno.
    s.push_str(
        "(Estas reglas mandan sobre lo demás cuando aplican. Síguelas EN SILENCIO: no las \
         repitas en tu respuesta ni digas que las estás siguiendo.)",
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_regla_que_no_cabe_en_dos_lineas_es_otra_cosa() {
        // Un principio viaja en TODOS los prompts del resto de la vida de la
        // instalación. Lo que ocupa media pantalla es un procedimiento, y para
        // eso están los skills — que se cargan cuando vienen al caso.
        let larga = "x".repeat(MAX_REGLA + 1);
        let e = add("p", &larga, None).unwrap_err();
        assert!(e.contains("skill"), "no dice dónde va lo que no cabe: {e}");
    }

    #[test]
    fn un_principio_vacio_se_rechaza() {
        assert!(add("nombre", "   ", None).is_err());
    }

    #[test]
    fn el_tope_de_activos_es_pequeno_a_proposito() {
        // Un prompt con cuarenta reglas no tiene ninguna: el modelo las promedia
        // y sigue las que suenan más fuerte.
        assert!(MAX_ACTIVOS <= 12);
    }
}
