//! Lo que Lucy sabe del operador, entre sesiones.
//!
//! LA ASIMETRÍA QUE ESTO CIERRA. El prompt de sistema tiene desde el primer día
//! un hueco `user_profile`, y la sección que lo pinta está escrita. En el shell
//! nativo ese hueco viajaba SIEMPRE vacío, y la etiqueta con la que Lucy escribe
//! —`<REMEMBER>`— caía en un `_ => {}` sin hacer nada. Con la recuperación
//! semántica ya conectada, el resultado era una Lucy que recuerda cosas que ella
//! no escribió y que no puede guardar nada de lo que aprende: pregunta el nombre
//! del dominio cada mañana y no hay forma de que deje de preguntarlo.
//!
//! El formato es el mismo que escribe la app —tabla `user_profile`, clave
//! primaria— porque es la MISMA base de datos. Dos formas de guardar el mismo
//! hecho darían dos perfiles según por qué ventana entraste.
//!
//! CLAVE PRIMARIA, no un registro con historia. Un perfil es lo que es cierto
//! AHORA: si el operador cambia de turno, "turno: noche" tiene que sustituir a
//! "turno: mañana" y no acompañarlo. Para lo que sí tiene historia están las
//! memorias.

/// Un hecho del perfil.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub value: String,
    pub category: String,
}

/// Tope de una clave y de un valor.
///
/// Los mismos que aplica el frontend de la app antes de llamar. Están aquí y no
/// solo allí porque quien escribe ahora es un modelo de lenguaje: una respuesta
/// que se desmadra no puede meter cuatro mil caracteres en un campo que se
/// inyecta en CADA prompt a partir de entonces.
pub const MAX_KEY: usize = 80;
pub const MAX_VALUE: usize = 500;

/// Cuántos hechos entran en el prompt.
///
/// El perfil viaja ENTERO en cada turno, así que crece el coste de todas las
/// conversaciones futuras. Cuarenta hechos son de sobra para describir a una
/// persona y su parque; más allá es que algo está escribiendo de más.
pub const MAX_IN_PROMPT: usize = 40;

/// Normaliza una clave: minúsculas, sin espacios, recortada.
///
/// El mismo aseo que hace la app antes de guardar. Sin él, "Dominio AD" y
/// "dominio_ad" son dos filas distintas del mismo hecho, y como la clave es
/// primaria, la segunda no sustituye a la primera: se acumulan las dos y el
/// prompt acaba diciendo dos cosas sobre lo mismo.
pub fn normalize_key(raw: &str) -> String {
    raw.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
        .chars()
        .take(MAX_KEY)
        .collect()
}

/// Separa `clave: valor` como lo hace la app.
///
/// Por el PRIMER dos puntos, no por el último ni por todos: el valor puede
/// llevarlos —una ruta con unidad, una hora, una URL— y partir por el último
/// convertiría `servidor: sql01:1433` en la clave `servidor: sql01`.
pub fn parse_fact(body: &str) -> Option<(String, String)> {
    let body = body.trim();
    let i = body.find(':')?;
    // Ni al principio ni al final: `:algo` no tiene clave y `algo:` no tiene
    // valor, y guardar cualquiera de los dos es guardar medio hecho.
    if i == 0 || i >= body.len() - 1 {
        return None;
    }
    let key = normalize_key(&body[..i]);
    let value: String = body[i + 1..].trim().chars().take(MAX_VALUE).collect();
    if key.is_empty() || value.is_empty() {
        return None;
    }
    Some((key, value))
}

/// Guarda o actualiza un hecho.
///
/// SE LIMPIA DE SECRETOS ANTES DEL DISCO, y no lo hacía. Este era el único
/// almacén que acaba en el prompt de sistema sin pasar por `scrub`.
///
/// El razonamiento está escrito en `memories`, que sí lo hace: «una memoria es
/// lo ÚNICO que sobrevive a la conversación, así que un token que se cuele en
/// una se queda para siempre y encima vuelve al prompt de todos los turnos
/// siguientes». Aquí es MÁS cierto, no menos: una memoria vuelve cuando se
/// parece a lo que se pregunta, y el perfil entra SIEMPRE — `block()` lo pega
/// entero en cada turno de cada conversación.
///
/// Y LO ESCRIBE EL MODELO. Estas filas las pone la etiqueta `<REMEMBER>`, que
/// Lucy emite por su cuenta con lo que tenga delante. Basta con que un volcado
/// de comando traiga un `Authorization: Bearer …` y que Lucy decida que ese
/// servidor es un dato del operador: la clave queda guardada, sin que nadie la
/// apruebe, y vuelve en todos los prompts posteriores.
///
/// La clave se limpia también: el modelo la redacta igual que el valor, y un
/// `api_key_de_produccion` como nombre de fila filtra tanto como el valor.
pub fn set(key: &str, value: &str, category: &str) -> Result<(), String> {
    let cat = if category.trim().is_empty() { "general" } else { category.trim() };
    let key = crate::memories::scrub(key);
    let value = crate::memories::scrub(value);
    crate::with_db(|conn| {
        conn.execute(
            "INSERT INTO user_profile (key, value, category, updated_at) \
             VALUES (?1, ?2, ?3, strftime('%s','now')) \
             ON CONFLICT(key) DO UPDATE SET \
               value = excluded.value, category = excluded.category, \
               updated_at = excluded.updated_at",
            rusqlite::params![key, value, cat],
        )
        .map_err(|e| format!("profile set: {e}"))?;
        Ok(())
    })
}

/// Olvida un hecho.
///
/// EXISTE POR LO MISMO QUE LA LISTA. Lo que aquí se guarda lo escribe un modelo,
/// sin que nadie lo apruebe, y viaja en todos los prompts a partir de entonces.
/// Un almacén así sin forma de borrar no es una memoria: es algo que se te queda
/// pegado.
pub fn forget(key: &str) -> Result<(), String> {
    crate::with_db(|conn| {
        conn.execute("DELETE FROM user_profile WHERE key = ?1", rusqlite::params![key])
            .map_err(|e| format!("profile forget: {e}"))?;
        Ok(())
    })
}

/// Todo el perfil, ordenado por categoría.
pub fn all() -> Result<Vec<Entry>, String> {
    crate::with_db(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT key, value, category FROM user_profile \
                 ORDER BY category, key LIMIT ?1",
            )
            .map_err(|e| format!("profile prepare: {e}"))?;
        let rows = stmt
            .query_map([MAX_IN_PROMPT as i64], |r| {
                Ok(Entry { key: r.get(0)?, value: r.get(1)?, category: r.get(2)? })
            })
            .map_err(|e| format!("profile query: {e}"))?;
        Ok(rows.flatten().collect())
    })
}

/// El perfil como bloque de prompt. Vacío = no hay nada que contar.
///
/// Agrupado por categoría y sin encabezados vacíos: un bloque que dice
/// "PREFERENCIAS: (ninguna)" enseña al modelo a saltarse ese encabezado, y el
/// día que sí traiga algo tampoco lo mirará.
pub fn block() -> String {
    let Ok(entries) = all() else {
        // Falla en silencio, como el recuerdo semántico: sin perfil se contesta
        // igual, solo que sin saber quién pregunta. Un error visible aquí
        // castigaría al operador por una función que ni pidió.
        return String::new();
    };
    if entries.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    let mut cat_actual = "";
    for e in &entries {
        if e.category != cat_actual {
            cat_actual = &e.category;
            s.push_str(&format!("\n[{}]\n", cat_actual.to_uppercase()));
        }
        s.push_str(&format!("- {}: {}\n", e.key.replace('_', " "), e.value));
    }
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_secreto_no_llega_al_prompt_de_todos_los_turnos() {
        // ERA EL ÚNICO ALMACÉN que acaba en el prompt de sistema sin limpiar.
        // Y aquí importa MÁS que en las memorias: una memoria vuelve cuando se
        // parece a lo que se pregunta; el perfil entra SIEMPRE.
        //
        // No toca base de datos: se comprueba el filtro, que es lo que faltaba.
        // Lo que `set` hace con el resultado ya lo prueba el resto del módulo.
        let sucio = "Authorization: Bearer sk-abcdefghijklmnop1234";
        let limpio = crate::memories::scrub(sucio);
        assert!(!limpio.contains("sk-abcdefghijklmnop1234"), "pasó el token: {limpio}");

        // Y la clave también, que la redacta el modelo igual que el valor.
        let k = crate::memories::scrub("api_key=AKIAIOSFODNN7EXAMPLE");
        assert!(!k.contains("AKIAIOSFODNN7EXAMPLE"), "pasó por la clave: {k}");

        // Lo que NO es un secreto se queda intacto: un perfil que se come el
        // nombre del dominio no sirve para nada.
        assert_eq!(crate::memories::scrub("dominio ad: corp.local"), "dominio ad: corp.local");
    }

    #[test]
    fn se_parte_por_el_primer_dos_puntos() {
        // El valor puede llevarlos: una ruta con unidad, un puerto, una URL.
        // Partir por el último convertiría el puerto en parte de la clave.
        let (k, v) = parse_fact("servidor principal: sql01:1433").unwrap();
        assert_eq!(k, "servidor_principal");
        assert_eq!(v, "sql01:1433");
        let (k, v) = parse_fact("carpeta de scripts: C:\\ops\\scripts").unwrap();
        assert_eq!(k, "carpeta_de_scripts");
        assert_eq!(v, "C:\\ops\\scripts");
    }

    #[test]
    fn medio_hecho_no_se_guarda() {
        // Guardar una clave sin valor o un valor sin clave llena el perfil de
        // filas que no dicen nada y que viajan en cada prompt a partir de ahí.
        assert!(parse_fact(": sin clave").is_none());
        assert!(parse_fact("sin valor:").is_none());
        assert!(parse_fact("sin nada").is_none());
        assert!(parse_fact("").is_none());
        assert!(parse_fact("   ").is_none());
    }

    #[test]
    fn la_misma_cosa_escrita_de_dos_formas_es_una_sola_clave() {
        // La clave es PRIMARIA. Sin normalizar, "Dominio AD" y "dominio_ad"
        // serían dos filas del mismo hecho y el prompt diría las dos.
        assert_eq!(normalize_key("Dominio AD"), "dominio_ad");
        assert_eq!(normalize_key("  DOMINIO   ad  "), "dominio_ad");
        assert_eq!(parse_fact("Dominio AD: corp.local").unwrap().0, "dominio_ad");
    }

    #[test]
    fn una_respuesta_desmadrada_no_puede_inflar_el_prompt() {
        // Quien escribe aquí es un modelo de lenguaje, y lo que escriba se
        // inyecta en CADA turno a partir de entonces. El tope no es cortesía.
        let largo = format!("clave: {}", "x".repeat(MAX_VALUE + 1_000));
        let (_, v) = parse_fact(&largo).unwrap();
        assert_eq!(v.chars().count(), MAX_VALUE);
        let clave_larga = format!("{}: v", "k".repeat(MAX_KEY + 50));
        assert_eq!(parse_fact(&clave_larga).unwrap().0.chars().count(), MAX_KEY);
    }

    #[test]
    fn sin_base_de_datos_el_bloque_sale_vacio_y_no_revienta() {
        // Igual que el recuerdo semántico: sin perfil se contesta, solo que sin
        // saber quién pregunta. Este test corre sin pool inicializado.
        assert!(block().is_empty());
    }
}
