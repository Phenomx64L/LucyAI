//! Ponerle nombre a una pestaña a partir de la primera orden.
//!
//! EL NOMBRE LO PONÍA UN `take(28)` y se notaba: las pestañas se llamaban
//! «Resume los errores más recie» y «Revisa la salud del sistem», cortadas a
//! mitad de palabra. Con tres conversaciones abiertas eso obliga a leer las tres
//! enteras para saber cuál era cuál, que es justo lo que un título evita.
//!
//! LO PIDE UN MODELO LOCAL SI LO HAY, y esa preferencia no es de estilo: nombrar
//! una pestaña es una tarea de dos líneas que no vale ni medio céntimo, pero
//! son medio céntimo POR PESTAÑA y por sesión, y además manda a un tercero la
//! primera frase de cada cosa que se hace. Un modelo de Ollama la resuelve
//! gratis y sin que salga del equipo.
//!
//! Y SI NO HAY NINGUNO, EL MÁS BARATO CON CLAVE. Lo que no hace nunca es usar el
//! modelo de trabajo por defecto: el operador puede tener puesto un Opus para
//! razonar, y gastar eso en un título de cinco palabras es tirar el dinero sin
//! que nadie lo haya pedido.
//!
//! CUANDO NO HAY NADIE, EL RESPALDO NO FALLA: recorta la orden por la última
//! palabra entera. Peor que un título escrito, mejor que uno partido.

use crate::turns::Turn;

/// Lo que como mucho ocupa el nombre de una pestaña.
///
/// Sale del sitio que hay en la barra, no de un gusto: por encima de esto el
/// título se corta al pintarlo y volvemos a la palabra partida por otra vía.
pub const MAX_CHARS: usize = 32;

/// Plazo para la petición del nombre.
///
/// Corto A PROPÓSITO. Esto corre mientras el operador ya está leyendo la
/// respuesta a su orden; si el modelo se atasca, el nombre no llega y la pestaña
/// se queda con el recorte, que es un final perfectamente bueno. Esperar medio
/// minuto por un título sería esperar por nada.
pub const TIMEOUT_SECS: u64 = 20;

/// Cuántos tokens se le dejan generar.
///
/// Treinta y dos y no doscientos: un título son cinco palabras. Lo que esto
/// impide es que un modelo hablador se ponga a explicar por qué ha elegido ese
/// nombre y nos cobre el ensayo.
pub const NUM_PREDICT: u32 = 32;

/// Ollama, en este equipo.
const OLLAMA: &str = "http://127.0.0.1:11434";

/// La instrucción. En español porque las órdenes vienen en español y un modelo
/// pequeño al que se le habla en inglés contesta en inglés.
const SISTEMA: &str = "Eres un rotulador de pestañas. Te dan la primera orden de una \
conversación y devuelves un título corto que la identifique.\n\n\
REGLAS:\n\
- Como mucho cinco palabras.\n\
- En el idioma de la orden.\n\
- Sin comillas, sin punto final, sin prefijos como «Título:».\n\
- Nombra el ASUNTO, no la acción genérica: «Servicios de Google parados», no \
«Consulta del sistema».\n\
- Responde SOLO con el título, nada más.";

/// Quién pone el nombre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fuente {
    /// Un modelo de Ollama. No cuesta y no sale del equipo.
    Local(String),
    /// El modelo de nube más barato de los que tienen clave guardada.
    Nube(String),
}

impl Fuente {
    pub fn modelo(&self) -> &str {
        match self {
            Fuente::Local(m) | Fuente::Nube(m) => m,
        }
    }
}

/// El respaldo: la orden recortada POR PALABRAS ENTERAS.
///
/// Es lo que se ve cuando no hay ningún modelo, y también lo que se ve durante
/// el segundo o dos que tarda el que sí hay. Que corte por espacio en vez de por
/// carácter es la mitad de la mejora y no cuesta nada.
pub fn recorta(orden: &str) -> String {
    let limpio = orden.split_whitespace().collect::<Vec<_>>().join(" ");
    if limpio.chars().count() <= MAX_CHARS {
        return limpio;
    }
    let cortado: String = limpio.chars().take(MAX_CHARS).collect();
    // Se retrocede hasta el último espacio para no partir la palabra. Si no hay
    // ninguno —una ruta larguísima sin espacios— se deja el corte duro: mejor
    // eso que una pestaña vacía.
    match cortado.rfind(' ') {
        Some(i) if i >= MAX_CHARS / 3 => format!("{}…", cortado[..i].trim_end()),
        _ => format!("{}…", cortado.trim_end()),
    }
}

/// Deja en un título lo que devuelve un modelo.
///
/// LOS MODELOS NO OBEDECEN DEL TODO, y aquí eso importa más que en otros sitios
/// porque la salida se pinta tal cual en un sitio de treinta caracteres. Los
/// razonadores abren `<think>`, los pequeños contestan «Título: X», casi todos
/// envuelven en comillas y alguno añade un punto. Se limpia todo eso en vez de
/// pedirlo más fuerte en el prompt, que es lo que no funciona.
pub fn limpia(bruto: &str) -> String {
    // El bloque de pensamiento primero: dentro puede haber comillas y dos
    // puntos, y quitarlos después dejaría medio razonamiento como título.
    let sin_pensar = match bruto.rfind("</think>") {
        Some(i) => &bruto[i + "</think>".len()..],
        None => bruto,
    };
    // La primera línea con algo: si se enrolla, lo que sirve está arriba.
    let mut t = sin_pensar
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string();
    // «Título: X», «Title: X», «Nombre: X» — solo si lo que queda detrás tiene
    // sustancia. Sin esa condición, «Reinicio: el spooler» se quedaría en «el
    // spooler», que dice menos.
    for p in ["título:", "titulo:", "title:", "nombre:", "pestaña:"] {
        let bajo = t.to_lowercase();
        if let Some(resto) = bajo.strip_prefix(p) {
            if resto.trim().chars().count() >= 3 {
                t = t[t.len() - resto.len()..].trim().to_string();
            }
        }
    }
    t = t
        .trim_matches(|c: char| {
            c.is_whitespace() || matches!(c, '"' | '\'' | '«' | '»' | '`' | '*' | '#' | '.')
        })
        .to_string();
    // Un modelo que ignora el plazo devuelve un párrafo. Recortarlo aquí y no
    // rechazarlo: las primeras palabras de un párrafo sobre el asunto siguen
    // siendo mejores que las primeras palabras de la orden.
    recorta(&t)
}

/// Los modelos de nube con clave guardada, del más barato al más caro.
///
/// SE ORDENA POR EL PRECIO DE SALIDA. Un título son treinta tokens de salida y
/// unos cien de entrada, así que ninguna de las dos domina de largo; se elige la
/// salida porque es la que más varía entre modelos —hasta cien veces— y la que
/// castiga si un modelo se enrolla.
pub fn baratos_con_clave() -> Vec<String> {
    let mut v: Vec<(f64, &str)> = crate::pricing::PRICES
        .iter()
        .filter(|(id, _, _)| {
            // Que exista en el catálogo, para no proponer un id que el selector
            // no sabría ni dibujar.
            crate::models::find(id).is_some() && tiene_clave(id)
        })
        .map(|(id, _, salida)| (*salida, *id))
        .collect();
    // Por precio y, a igualdad, por nombre: sin el desempate el orden depende de
    // cómo estuviera la tabla ese día y el test de abajo sería una lotería.
    v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(b.1)));
    v.into_iter().map(|(_, id)| id.to_string()).collect()
}

/// ¿Hay clave guardada para el proveedor de este modelo?
///
/// SIN RAMA COMODÍN a propósito: si mañana entra un proveedor nuevo en
/// `cloud::Provider`, esto deja de compilar y alguien tiene que decidir si sus
/// modelos pueden titular. Con un `_ => false` se colaría callando y el
/// proveedor nuevo sería el único que nunca sale elegido, sin que nada lo diga.
fn tiene_clave(modelo: &str) -> bool {
    use crate::cloud::Provider;
    // Los ids son los de `keys::PROVIDERS`, y el test de abajo comprueba que
    // ninguno se ha quedado huérfano.
    let p = match crate::cloud::provider_of(modelo) {
        Provider::Anthropic => "anthropic",
        Provider::Gemini => "gemini",
        Provider::OpenAi => "openai",
        Provider::Xai => "xai",
        Provider::DeepSeek => "deepseek",
        Provider::Nvidia => "nvidia",
        // Ollama no lleva clave, y aquí solo se buscan modelos de nube.
        Provider::Ollama => return false,
    };
    crate::keys::has(p)
}

/// Quién le pone el nombre a la pestaña, o nadie.
///
/// EL ORDEN NO ES NEGOCIABLE: local, luego nube barata, luego nadie. Y con el
/// modo privacidad puesto la nube no se toca ni aunque haya clave — un título es
/// una tarea menor, pero manda fuera la primera frase de la orden, que es
/// exactamente lo que ese modo promete que no pasa.
pub fn elige(instalados: &[String], privacidad: bool) -> Option<Fuente> {
    if let Some(m) = crate::crystals::elige(instalados) {
        return Some(Fuente::Local(m));
    }
    if privacidad {
        return None;
    }
    baratos_con_clave().into_iter().next().map(Fuente::Nube)
}

/// Lo que se le manda al modelo.
fn turnos(orden: &str) -> Vec<Turn> {
    // La orden recortada: para titular no hace falta el párrafo entero, y
    // mandarlo entero multiplica por diez los tokens de entrada de algo que
    // tiene que ser barato.
    let corta: String = orden.trim().chars().take(600).collect();
    vec![
        Turn::system(SISTEMA),
        Turn::user(format!("Primera orden de la conversación:\n\n{corta}")),
    ]
}

/// Pide el nombre. BLOQUEANTE: quien llama ya está en un hilo.
///
/// Devuelve además los tokens que dijo el proveedor, para que el gasto de la
/// sesión lo cuente igual que cuenta el de un turno normal. Un coste que no se
/// apunta es un coste que aparece en la factura sin haber salido nunca en la
/// barra de estado.
pub fn nombra(orden: &str, fuente: &Fuente) -> Result<(String, u32, u32), String> {
    match fuente {
        Fuente::Local(m) => local(orden, m).map(|t| (t, 0, 0)),
        Fuente::Nube(m) => nube(orden, m),
    }
}

/// Ollama, sin streaming: son treinta tokens y no hay nada que enseñar mientras.
fn local(orden: &str, modelo: &str) -> Result<String, String> {
    let cuerpo = serde_json::json!({
        "model": modelo,
        "messages": turnos(orden)
            .iter()
            .map(|t| serde_json::json!({
                "role": match t.who {
                    crate::turns::Who::System => "system",
                    crate::turns::Who::Assistant => "assistant",
                    crate::turns::Who::User => "user",
                },
                "content": t.text,
            }))
            .collect::<Vec<_>>(),
        "stream": false,
        "options": { "temperature": 0.2, "num_predict": NUM_PREDICT },
    });
    let resp = ureq::post(&format!("{OLLAMA}/api/chat"))
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .send_string(&cuerpo.to_string())
        .map_err(|e| format!("Ollama no respondió: {e}"))?;
    let texto = resp.into_string().map_err(|e| format!("respuesta ilegible: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&texto).map_err(|e| format!("JSON inválido: {e}"))?;
    // Ollama devuelve 200 con un `error` dentro cuando el modelo no está.
    if let Some(e) = json.get("error").and_then(|e| e.as_str()) {
        return Err(format!("Ollama: {e}"));
    }
    let salida = json
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    let t = limpia(salida);
    if t.is_empty() {
        return Err("el modelo devolvió un título vacío".into());
    }
    Ok(t)
}

/// La nube, drenando el stream que ya usa el resto de la aplicación.
///
/// Se reutiliza `cloud::start` en vez de escribir una petición aparte: es el
/// mismo camino que ya sabe de claves, de cabeceras y de las cinco formas
/// distintas que tienen los proveedores de contar tokens. Una segunda
/// implementación de eso sería una segunda que mantener.
fn nube(orden: &str, modelo: &str) -> Result<(String, u32, u32), String> {
    use crate::chat::ChatEvent;
    let rx = crate::cloud::start(modelo.to_string(), turnos(orden));
    let limite = std::time::Duration::from_secs(TIMEOUT_SECS);
    let (mut texto, mut ent, mut sal) = (String::new(), 0, 0);
    loop {
        match rx.recv_timeout(limite) {
            Ok(ChatEvent::Token(t)) => texto.push_str(&t),
            Ok(ChatEvent::Usage(i, o)) => (ent, sal) = (i, o),
            Ok(ChatEvent::Done) => break,
            Ok(ChatEvent::Error(e)) => return Err(e),
            // Se acabó el plazo o se cayó el hilo. Con lo que haya llegado basta
            // si llegó algo: un título a medias sigue diciendo de qué va.
            Err(_) => break,
        }
    }
    let t = limpia(&texto);
    if t.is_empty() {
        return Err("el modelo devolvió un título vacío".into());
    }
    Ok((t, ent, sal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_respaldo_no_parte_palabras() {
        // EL FALLO QUE SE VE EN LAS CAPTURAS. `take(28)` dejaba «Resume los
        // errores más recie», y una pestaña que acaba en «recie» obliga a abrirla
        // para saber qué era.
        let t = recorta("Resume los errores más recientes del registro de eventos");
        assert!(t.chars().count() <= MAX_CHARS + 1, "«{t}» no cabe");
        assert!(!t.contains("recie…"), "ha partido una palabra: «{t}»");
        assert!(t.ends_with('…'), "no avisa de que hay más: «{t}»");
        // Lo que queda tienen que ser palabras enteras.
        let cuerpo = t.trim_end_matches('…');
        assert!(
            "Resume los errores más recientes del registro de eventos"
                .starts_with(cuerpo),
            "«{cuerpo}» no es un prefijo por palabras"
        );
    }

    #[test]
    fn una_orden_corta_se_queda_como_esta() {
        assert_eq!(recorta("hostname"), "hostname");
        assert_eq!(recorta("  reinicia   el spooler  "), "reinicia el spooler");
    }

    #[test]
    fn una_palabra_larguisima_se_corta_aunque_duela() {
        // Una ruta sin espacios. Cortar duro es feo; dejar la pestaña en blanco
        // o desbordar la barra es peor.
        let t = recorta("C:\\Windows\\System32\\drivers\\etc\\hosts\\muy\\larga\\sin\\espacios");
        assert!(t.chars().count() <= MAX_CHARS + 1, "«{t}» no cabe");
        assert!(!t.is_empty());
    }

    #[test]
    fn se_le_quita_al_modelo_todo_lo_que_le_sobra() {
        // Cada uno de estos salió de verdad de un modelo pequeño.
        for (bruto, espera) in [
            ("\"Servicios de Google parados\"", "Servicios de Google parados"),
            ("Título: Reinicio del spooler", "Reinicio del spooler"),
            ("**Salud del sistema**", "Salud del sistema"),
            ("Salud del sistema.", "Salud del sistema"),
            ("«Certificado caducado»", "Certificado caducado"),
            ("  \n\nAuditoría de firewall\n\nEspero que te sirva.", "Auditoría de firewall"),
        ] {
            assert_eq!(limpia(bruto), espera, "no ha limpiado «{bruto}»");
        }
    }

    #[test]
    fn el_bloque_de_pensamiento_no_acaba_de_titulo() {
        // `qwen3` y `deepseek-r1` abren `<think>` siempre. Sin cortarlo, la
        // pestaña se llamaba «Vale, el usuario quiere que».
        let bruto = "<think>Vale, el usuario quiere que resuma los servicios \
                     parados. Un buen título sería...</think>\nServicios parados";
        assert_eq!(limpia(bruto), "Servicios parados");
    }

    #[test]
    fn un_prefijo_sin_nada_detras_no_se_come_el_titulo() {
        // «Reinicio: el spooler» es un título legítimo con dos puntos. Quitarle
        // el prefijo lo dejaría en «el spooler», que dice menos que el original.
        assert_eq!(limpia("Reinicio: el spooler"), "Reinicio: el spooler");
    }

    #[test]
    fn el_modo_privacidad_no_deja_salir_el_titulo_del_equipo() {
        // Sin modelos locales y con privacidad puesta, NADIE. Aunque haya clave.
        // Un título es una tarea menor y aun así manda fuera la primera frase de
        // la orden, que es lo que ese modo promete que no pasa.
        assert_eq!(elige(&[], true), None);
    }

    #[test]
    fn manda_el_local_aunque_haya_nube() {
        let instalados = vec!["mistral:latest".to_string(), "nomic-embed-text".to_string()];
        match elige(&instalados, false) {
            Some(Fuente::Local(m)) => assert_eq!(m, "mistral:latest"),
            otro => panic!("tenía que elegir el local, salió {otro:?}"),
        }
    }

    #[test]
    fn el_embebedor_no_vale_para_titular() {
        // `nomic-embed-text` no tiene `/api/chat`. Si se colara, cada pestaña
        // nueva pediría un título a un modelo que solo sabe devolver vectores.
        //
        // NO SE AFIRMA `None`: sin local usable, lo correcto es caer a la nube si
        // hay clave. Así estaba escrito este test y fallaba en cualquier equipo
        // con una clave guardada —o sea, en el del operador— acusando al código
        // de un fallo que era del test.
        let embebedor = vec!["nomic-embed-text".to_string()];
        if let Some(Fuente::Local(m)) = elige(&embebedor, false) {
            panic!("ha elegido el embebedor «{m}», que no sabe contestar");
        }
        // Y con privacidad no hay salida por la nube: nadie, y el recorte manda.
        assert_eq!(elige(&embebedor, true), None);
    }

    #[test]
    fn los_baratos_salen_ordenados_y_de_verdad_baratos() {
        // No se puede afirmar cuáles hay —depende de qué claves tenga guardadas
        // el equipo donde corra el test— pero sí que la lista está ordenada y
        // que cada uno existe en el catálogo.
        let v = baratos_con_clave();
        for par in v.windows(2) {
            let precio = |id: &str| {
                crate::pricing::PRICES
                    .iter()
                    .find(|(i, _, _)| *i == id)
                    .map(|(_, _, o)| *o)
                    .unwrap()
            };
            assert!(
                precio(&par[0]) <= precio(&par[1]),
                "«{}» va antes que «{}» y cuesta más",
                par[0],
                par[1]
            );
        }
        for id in &v {
            assert!(crate::models::find(id).is_some(), "«{id}» no está en el catálogo");
        }
    }

    #[test]
    fn cada_proveedor_de_nube_apunta_a_una_clave_que_existe() {
        // `tiene_clave` traduce el enum de `cloud` al id de `keys`, y son dos
        // listas distintas en dos ficheros distintos. Un id mal escrito —«google»
        // por «gemini»— no falla: hace que ese proveedor NUNCA salga elegido, y
        // el síntoma es que las pestañas dejan de tener nombre sin decir por qué.
        for id in crate::pricing::PRICES.iter().map(|(id, _, _)| *id) {
            let Some(_) = crate::models::find(id) else { continue };
            use crate::cloud::Provider;
            let p = match crate::cloud::provider_of(id) {
                Provider::Anthropic => "anthropic",
                Provider::Gemini => "gemini",
                Provider::OpenAi => "openai",
                Provider::Xai => "xai",
                Provider::DeepSeek => "deepseek",
                Provider::Nvidia => "nvidia",
                Provider::Ollama => continue,
            };
            assert!(
                crate::keys::PROVIDERS.iter().any(|(k, _, _)| *k == p),
                "«{id}» apunta al proveedor «{p}», que no está en keys::PROVIDERS"
            );
        }
    }

    #[test]
    fn el_titulo_que_sale_del_modelo_cabe_en_la_pestana() {
        // Un modelo que ignora «cinco palabras» y suelta un párrafo no puede
        // dejar la barra de pestañas desbordada.
        let parrafo = "Este título describe una auditoría completa del cortafuegos \
                       del equipo con todas sus reglas y excepciones documentadas";
        assert!(limpia(parrafo).chars().count() <= MAX_CHARS + 1);
    }
}
