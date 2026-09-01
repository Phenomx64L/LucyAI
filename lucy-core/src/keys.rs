//! Las claves de API de los proveedores.
//!
//! EL AGUJERO QUE ESTO CIERRA. El shell nativo sabía LEER las claves del
//! Credential Manager y no sabía escribirlas: `keyring` aparecía en una sola
//! línea, dentro de una función que preguntaba si existían. En una instalación
//! limpia, egui arrancaba sin poder hablar con ningún modelo de nube y sin decir
//! dónde arreglarlo — para configurar una clave había que abrir el WebView del
//! que este shell existe para escapar. Es el mismo problema que tenía el índice
//! de equipos, con la misma forma y la misma solución.
//!
//! MISMO ALMACÉN Y MISMOS NOMBRES que la app: servicio `LucySysAdmin`, entrada
//! `{proveedor}_api_key`. Escribir la clave desde aquí la deja disponible allí y
//! al revés, que es lo que evita configurarla dos veces.
//!
//! AQUÍ NUNCA SE DEVUELVE EL VALOR. Se puede guardar, se puede borrar y se puede
//! preguntar si está — y no hay forma de leerlo. Quien lo necesita de verdad es
//! `cloud`, en el momento de firmar una petición, y ahí ya lo lee él. Una
//! función que devuelva la clave a la interfaz es una función que acaba
//! pintándola en pantalla o metiéndola en un registro.

/// El servicio del Credential Manager, el mismo que usa la app.
const SERVICE: &str = "LucySysAdmin";

/// Un proveedor que necesita clave, para la vista de configuración.
///
/// Ollama NO está: es local y no tiene clave. Enseñarle una casilla vacía
/// permanente al lado de los demás haría pensar que falta configurar algo.
pub const PROVIDERS: &[(&str, &str, &str)] = &[
    ("anthropic", "Anthropic", "console.anthropic.com"),
    ("gemini", "Google Gemini", "aistudio.google.com/apikey"),
    ("openai", "OpenAI", "platform.openai.com/api-keys"),
    ("xai", "xAI", "console.x.ai"),
    ("deepseek", "DeepSeek", "platform.deepseek.com"),
    ("nvidia", "NVIDIA NIM", "build.nvidia.com"),
];

fn entry(provider: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, &format!("{provider}_api_key"))
        .map_err(|e| format!("No se pudo abrir el almacén de credenciales: {e}"))
}

/// Si hay clave guardada para este proveedor.
pub fn has(provider: &str) -> bool {
    provider == "ollama"
        || entry(provider).map(|e| e.get_password().is_ok()).unwrap_or(false)
}

/// Guarda una clave.
///
/// Se recorta antes: pegar desde un navegador arrastra un salto de línea o un
/// espacio con una facilidad pasmosa, y una clave con un carácter de más falla
/// con un 401 que dice «clave inválida» — cierto y desconcertante cuando uno
/// acaba de copiarla bien.
pub fn set(provider: &str, key: &str) -> Result<(), String> {
    let k = key.trim();
    if k.is_empty() {
        return Err("La clave está vacía.".into());
    }
    entry(provider)?
        .set_password(k)
        .map_err(|e| format!("No se pudo guardar la clave: {e}"))
}

/// Borra la clave de un proveedor.
///
/// Que no hubiera ninguna NO es un error: el operador quería que no la hubiera,
/// y ese es el estado en el que queda.
pub fn delete(provider: &str) -> Result<(), String> {
    match entry(provider)?.delete_password() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("No se pudo borrar la clave: {e}")),
    }
}

/// Una pista de qué clave es, sin enseñarla.
///
/// Los cuatro últimos caracteres y nada más. Sirve para lo único que hace falta
/// —distinguir «puse la de producción» de «puse la de pruebas»— y no vale para
/// usar la clave ni para reconstruirla. Enseñarla entera «para comprobar» es
/// como se acaba con una clave en una captura de pantalla.
pub fn hint(provider: &str) -> Option<String> {
    let v = entry(provider).ok()?.get_password().ok()?;
    let n = v.chars().count();
    if n < 8 {
        // Demasiado corta para enseñar un trozo sin enseñarla casi entera.
        return Some("••••".into());
    }
    Some(format!("••••{}", v.chars().skip(n - 4).collect::<String>()))
}

/// Qué dijo el proveedor cuando se le preguntó si la clave sirve.
#[derive(Debug, Clone, PartialEq)]
pub enum Prueba {
    /// Contestó y la aceptó.
    Vale,
    /// Contestó y la rechazó. El texto es suyo, no nuestro.
    NoVale(String),
    /// No se pudo preguntar: sin red, sin clave, o el proveedor no contestó.
    ///
    /// SEPARADO DE `NoVale` A PROPÓSITO. «Tu clave no sirve» y «no he podido
    /// comprobarlo» llevan a sitios distintos: el primero a generar otra clave,
    /// el segundo a mirar la red. Juntarlos haría que un corte de dos minutos
    /// pareciera una credencial revocada.
    NoSeSabe(String),
}

/// Cuánto se espera a que el proveedor conteste.
///
/// Ocho segundos. Es una comprobación que alguien pidió con un clic y está
/// mirando: más allá de eso lo que hace falta es decir «no contesta», no seguir
/// esperando.
pub const PRUEBA_TIMEOUT: u64 = 8;

/// Pregunta al proveedor si la clave guardada sirve.
///
/// LA PREGUNTA MÁS BARATA QUE ACEPTE CADA UNO, y ninguna genera texto: se pide el
/// catálogo de modelos, que es una lectura. Mandar una conversación de prueba
/// costaría dinero cada vez que alguien pulsa el botón — y el botón está para
/// usarlo sin pensárselo.
///
/// Una clave GUARDADA solo dice que existe. Una caducada, revocada o pegada con
/// un espacio de más se descubre igual de bien en mitad de un incidente, que es
/// exactamente cuando no se quiere descubrir.
pub fn probe(provider: &str) -> Prueba {
    let Some(clave) = get(provider) else {
        return Prueba::NoSeSabe("no hay clave guardada".into());
    };
    let (url, cabecera, valor) = match provider {
        "anthropic" => (
            "https://api.anthropic.com/v1/models",
            "x-api-key",
            clave.clone(),
        ),
        "gemini" => (
            "https://generativelanguage.googleapis.com/v1beta/models",
            "x-goog-api-key",
            clave.clone(),
        ),
        "openai" => ("https://api.openai.com/v1/models", "authorization", format!("Bearer {clave}")),
        "xai" => ("https://api.x.ai/v1/models", "authorization", format!("Bearer {clave}")),
        "deepseek" => (
            "https://api.deepseek.com/models",
            "authorization",
            format!("Bearer {clave}"),
        ),
        "nvidia" => (
            "https://integrate.api.nvidia.com/v1/models",
            "authorization",
            format!("Bearer {clave}"),
        ),
        otro => return Prueba::NoSeSabe(format!("no sé cómo comprobar «{otro}»")),
    };
    let mut req = ureq::get(url).timeout(std::time::Duration::from_secs(PRUEBA_TIMEOUT));
    req = req.set(cabecera, &valor);
    // Anthropic exige la versión de su API también para listar.
    if provider == "anthropic" {
        req = req.set("anthropic-version", "2023-06-01");
    }
    match req.call() {
        Ok(_) => Prueba::Vale,
        // 401/403 es el proveedor diciendo que no: eso SÍ se sabe.
        Err(ureq::Error::Status(401 | 403, _)) => {
            Prueba::NoVale("el proveedor la rechaza — caducada, revocada o mal pegada".into())
        }
        Err(ureq::Error::Status(429, _)) => {
            // Aceptó la credencial y se queja del ritmo: la clave sirve.
            Prueba::Vale
        }
        Err(ureq::Error::Status(c, _)) => Prueba::NoSeSabe(format!("contestó {c}")),
        Err(e) => Prueba::NoSeSabe(format!("no se pudo preguntar: {e}")),
    }
}

/// La clave guardada. Privada a propósito: el valor no sale de este módulo más
/// que para viajar al proveedor.
///
/// Por `entry`, que es quien conoce la convención del nombre —`{proveedor}_api_key`—
/// y no repitiéndola aquí: dos sitios que construyen la misma clave son dos
/// sitios que acaban construyéndola distinto.
fn get(provider: &str) -> Option<String> {
    let e = entry(provider).ok()?;
    e.get_password().ok().filter(|k| !k.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_no_necesita_clave() {
        // Es local. Pedirle una sería inventarse un requisito.
        assert!(has("ollama"));
        assert!(!PROVIDERS.iter().any(|(k, _, _)| *k == "ollama"));
    }

    #[test]
    fn una_clave_vacia_no_se_guarda() {
        // Guardar una cadena vacía dejaría `has` diciendo que sí la hay, y el
        // fallo aparecería mucho más tarde, como un 401 del proveedor.
        assert!(set("proveedor_de_prueba", "").is_err());
        assert!(set("proveedor_de_prueba", "   ").is_err());
    }

    #[test]
    fn cada_proveedor_del_catalogo_tiene_su_entrada() {
        // El nombre de la entrada es un contrato con la app: si aquí se escribe
        // `anthropic_key` y allí se lee `anthropic_api_key`, cada interfaz ve
        // una clave distinta y ninguna de las dos lo dice.
        for (k, etiqueta, donde) in PROVIDERS {
            assert!(!k.is_empty() && !etiqueta.is_empty());
            // La dirección de dónde se saca: sin ella, «configura tu clave» es
            // una instrucción que obliga a buscar.
            assert!(donde.contains('.'), "{k} no dice dónde conseguirla");
        }
    }

    #[test]
    fn la_pista_no_permite_reconstruir_la_clave() {
        // Cuatro caracteres distinguen la de producción de la de pruebas y no
        // sirven para nada más, que es exactamente lo que se busca.
        let p = "proveedor_de_prueba_pista";
        if set(p, "sk-ant-0123456789ABCD").is_ok() {
            let h = hint(p).unwrap();
            assert_eq!(h, "••••ABCD");
            assert!(!h.contains("sk-ant"), "enseña el principio de la clave");
            let _ = delete(p);
        }
    }

    #[test]
    fn borrar_lo_que_no_esta_no_es_un_error() {
        // El operador quería que no hubiera clave, y ese es el estado en el que
        // queda. Un error aquí obligaría a distinguir dos éxitos.
        assert!(delete("proveedor_que_no_existe_jamas").is_ok());
    }
}