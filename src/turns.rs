//! La conversación que se le manda al modelo.
//!
//! EL FALLO QUE ESTO CIERRA. El shell nativo mandaba, en cada turno, el prompt
//! de sistema y el mensaje del operador — y nada más. Lucy no recordaba lo que
//! se había dicho dos mensajes antes en la misma pestaña: cada pregunta llegaba
//! a un modelo que no había visto ninguna. No se notaba en los casos de prueba
//! porque se sostenían solos —la salida de un comando se pegaba al prompt, y el
//! estado del equipo va en cada turno—, pero "¿y eso por qué?" era una pregunta
//! sin sujeto.
//!
//! Aquí vive la forma neutra —quién dijo qué— y el recorte. Cómo se traduce eso
//! al cuerpo de cada proveedor es cosa de `cloud`: los tres lo llaman distinto y
//! Gemini además llama "model" a lo que los otros llaman "assistant".

/// Quién habla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    /// Las instrucciones. Van aparte en las tres APIs, no como un mensaje más.
    System,
    User,
    Assistant,
}

/// Una imagen que viaja con un turno, ya en base64.
///
/// Se guarda codificada y no como bytes porque las tres APIs la quieren así, y
/// tener los bytes crudos solo serviría para volver a codificarlos en cada
/// reintento. El `media_type` va aparte del dato: Anthropic lo exige como campo
/// propio y adivinarlo del contenido no es asunto de esta capa.
#[derive(Debug, Clone)]
pub struct Image {
    pub media_type: String,
    pub b64: String,
}

#[derive(Debug, Clone)]
pub struct Turn {
    pub who: Who,
    pub text: String,
    /// Imágenes adjuntas. Vacío en la inmensa mayoría de los turnos.
    ///
    /// Van en el TURNO y no en una lista aparte de la petición porque una
    /// imagen pertenece al mensaje en el que se adjuntó: si el operador manda
    /// una captura, pregunta otra cosa, y luego dice "¿y en la de antes?", el
    /// modelo tiene que poder atribuir cada imagen a su momento. Colgarlas
    /// todas del último turno perdería eso.
    pub images: Vec<Image>,
}

impl Turn {
    pub fn user(text: impl Into<String>) -> Self {
        Self { who: Who::User, text: text.into(), images: Vec::new() }
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self { who: Who::Assistant, text: text.into(), images: Vec::new() }
    }
    pub fn system(text: impl Into<String>) -> Self {
        Self { who: Who::System, text: text.into(), images: Vec::new() }
    }
    /// El mismo turno con imágenes colgadas.
    pub fn with_images(mut self, images: Vec<Image>) -> Self {
        self.images = images;
        self
    }
}

/// Cuántos caracteres de historia se mandan como mucho.
///
/// No es el límite del modelo —todos aguantan mucho más— sino el del BOLSILLO:
/// la historia viaja ENTERA en cada turno, así que una conversación de cien
/// mensajes se paga cien veces. 40 000 caracteres son unos 10 000 tokens, que en
/// una charla normal cubre de sobra el contexto que hace falta para entender un
/// "¿y eso por qué?".
pub const MAX_HISTORY_CHARS: usize = 40_000;

/// Lo que "ocupa" una imagen en el presupuesto de la historia.
///
/// Una imagen no tiene caracteres, así que sin esto no pesaría nada y una
/// conversación con diez capturas viajaría entera en cada turno costando lo que
/// cuestan diez imágenes — que es mucho más de lo que costaría el texto que sí
/// se estaría recortando. No es una medida: es una estimación deliberadamente
/// alta. Una captura de pantalla ronda los 1500 tokens, y aquí se cuenta en
/// caracteres a cuatro por token.
pub const IMAGE_COST_CHARS: usize = 6_000;

/// Lo que pesa un turno en el presupuesto: su texto más sus imágenes.
fn weight(t: &Turn) -> usize {
    t.text.chars().count() + t.images.len() * IMAGE_COST_CHARS
}

/// Recorta la conversación para que quepa, conservando lo ÚLTIMO.
///
/// Por el final y no por el principio: en una conversación, lo que se acaba de
/// decir es lo que da sentido a la pregunta actual. Y siempre en pares
/// completos — cortar entre una pregunta y su respuesta deja al modelo con una
/// respuesta huérfana, que es peor que no darle nada.
///
/// El mensaje de sistema NUNCA se recorta: es quién es Lucy y en qué equipo
/// está, y sin él vuelve a ser un modelo genérico.
pub fn fit(system: &str, history: &[Turn], max_chars: usize) -> Vec<Turn> {
    let mut out: Vec<Turn> = Vec::new();
    let mut used = 0usize;

    // De atrás hacia delante, en pares.
    let mut i = history.len();
    while i > 0 {
        let start = if i >= 2 && history[i - 2].who == Who::User && history[i - 1].who != Who::User
        {
            i - 2
        } else {
            i - 1
        };
        let bloque: usize = history[start..i].iter().map(weight).sum();
        if used + bloque > max_chars && !out.is_empty() {
            break;
        }
        for t in history[start..i].iter().rev() {
            out.push(t.clone());
        }
        used += bloque;
        i = start;
    }

    out.reverse();
    let mut v = Vec::with_capacity(out.len() + 1);
    if !system.is_empty() {
        v.push(Turn::system(system));
    }
    v.extend(out);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(n: usize) -> Vec<Turn> {
        (0..n)
            .flat_map(|i| {
                [
                    Turn::user(format!("pregunta {i}")),
                    Turn::assistant(format!("respuesta {i}")),
                ]
            })
            .collect()
    }

    #[test]
    fn el_sistema_va_siempre_y_primero() {
        // Sin él Lucy vuelve a ser un modelo genérico que no sabe en qué equipo
        // está — que es exactamente el fallo del que venimos.
        let v = fit("eres lucy", &conv(2), 10_000);
        assert_eq!(v[0].who, Who::System);
        assert_eq!(v[0].text, "eres lucy");
        assert_eq!(v.len(), 5);
    }

    #[test]
    fn se_conserva_lo_ultimo_no_lo_primero() {
        // En una conversación, lo que se acaba de decir es lo que da sentido a
        // la pregunta actual. Guardar el principio y tirar el final es guardar
        // justo lo que ya no importa.
        let v = fit("s", &conv(50), 200);
        let textos: Vec<&str> = v.iter().map(|t| t.text.as_str()).collect();
        assert!(textos.contains(&"respuesta 49"), "falta lo último: {textos:?}");
        assert!(!textos.contains(&"pregunta 0"), "no debería caber lo primero");
    }

    #[test]
    fn no_se_corta_entre_una_pregunta_y_su_respuesta() {
        // Una respuesta huérfana es peor que no mandar nada: el modelo ve algo
        // que él dijo sin saber a qué.
        let v = fit("s", &conv(50), 200);
        let h: Vec<&Turn> = v.iter().filter(|t| t.who != Who::System).collect();
        assert_eq!(h.first().unwrap().who, Who::User, "empieza por una respuesta suelta");
        assert_eq!(h.len() % 2, 0, "quedó un turno descabalado");
    }

    #[test]
    fn una_conversacion_corta_va_entera() {
        let c = conv(3);
        let v = fit("s", &c, MAX_HISTORY_CHARS);
        assert_eq!(v.len(), c.len() + 1);
    }

    #[test]
    fn un_solo_turno_gigante_se_manda_igual() {
        // Aunque no quepa: recortarlo a cero dejaría al modelo sin la pregunta.
        // Vale más pagar un turno caro que contestar a nada.
        let c = vec![Turn::user("x".repeat(100_000))];
        let v = fit("s", &c, 100);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].text.chars().count(), 100_000);
    }

    #[test]
    fn sin_historia_solo_va_el_sistema() {
        assert_eq!(fit("s", &[], 100).len(), 1);
        assert!(fit("", &[], 100).is_empty());
    }
}
