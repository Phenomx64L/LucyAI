//! Enrutado: avisar cuando el modelo elegido se queda corto para lo que se pide.
//!
//! NO CAMBIA EL MODELO POR SU CUENTA, y esa es la decisión de diseño entera. Un
//! enrutador que elige en silencio significa que la respuesta que lees no viene
//! del modelo que seleccionaste, y cuando algo sale raro no hay forma de saber
//! si fue el modelo, la pregunta o el enrutador. Peor: en esta aplicación cambiar
//! de modelo cambia lo que se GASTA, y decidir gasto por el operador sin decirlo
//! no se puede defender.
//!
//! Así que esto avisa. «Vas a mandar una tarea de varios pasos con un modelo
//! económico» es lo que hace falta saber, y la decisión sigue siendo de quien
//! está delante. Se puede apagar entero.
//!
//! LA EXIGENCIA SE MIDE ANTES DE MANDAR, con lo que hay: el texto de la orden,
//! si lleva imágenes, y si la cadena va a correr sola. No hay nada más — pedirle
//! a un modelo que clasifique la dificultad costaría una llamada por turno para
//! decidir con qué modelo hacer la llamada siguiente.

/// Cuánto pide una orden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exigencia {
    /// Una pregunta suelta. Cualquier modelo vale.
    Simple,
    /// Lo normal: mirar algo del equipo y contestar.
    Normal,
    /// Varios pasos, o material que hay que interpretar.
    Alta,
}

/// Lo que se sabe de una orden antes de mandarla.
#[derive(Debug, Clone, Copy, Default)]
pub struct Tarea<'a> {
    pub texto: &'a str,
    /// Imágenes adjuntas. Una sola cambia el requisito de forma dura: sin visión
    /// el modelo no las ve, y no es cuestión de calidad.
    pub imagenes: usize,
    /// La cadena va a encadenar pasos sin que nadie los apruebe.
    pub auto: bool,
}

/// A partir de cuántos caracteres una orden deja de ser una pregunta suelta.
///
/// No es una medida de dificultad —hay preguntas cortas dificilísimas— sino de
/// CUÁNTO CONTEXTO trae. Y para eso sí sirve: lo que distingue «¿cuánta RAM
/// tengo?» de un párrafo describiendo un incidente es la longitud.
pub const TEXTO_LARGO: usize = 220;

/// Palabras que anuncian trabajo de varios pasos.
///
/// EN ESPAÑOL Y EN INGLÉS porque el operador escribe en los dos, a veces en la
/// misma frase. Son verbos de PLAN, no de consulta: «mira el disco» es una
/// lectura, «audita el disco» es un procedimiento.
const VERBOS: &[&str] = &[
    "audita", "auditar", "auditoría", "audit", "diagnostica", "diagnosticar",
    "diagnóstico", "investiga", "investigar", "analiza", "analizar", "compara",
    "comparar", "migra", "migrar", "automatiza", "automatizar", "script",
    "planifica", "planificar", "revisa todo", "paso a paso", "troubleshoot",
    "root cause", "por qué", "porqué", "por que",
];

/// Cuánto pide esta orden.
///
/// PURA, para que la regla se pueda discutir sin ventana ni red.
pub fn exigencia(t: &Tarea) -> Exigencia {
    // Una imagen es material que hay que interpretar: eso no es una pregunta
    // suelta por corta que sea la frase que la acompaña.
    if t.imagenes > 0 {
        return Exigencia::Alta;
    }
    // El automático encadena comandos sin que nadie los mire. Eso pide criterio
    // sostenido —decidir el paso siguiente a la vista del anterior— que es justo
    // lo que peor hacen los modelos pequeños.
    if t.auto {
        return Exigencia::Alta;
    }
    let bajo = t.texto.to_lowercase();
    if VERBOS.iter().any(|v| bajo.contains(v)) {
        return Exigencia::Alta;
    }
    // CRUZAR SUBSISTEMAS Y PEDIR SÍNTESIS ES TRABAJO DE VARIOS PASOS, aunque la
    // frase sea corta y no lleve ningún verbo de plan.
    //
    // ESTO SE MIDIÓ CONTRA UN CASO REAL. «Revisa la salud del sistema (CPU, RAM,
    // disco, servicios) y dame un resumen del estado» salía SIMPLE: ochenta y
    // ocho caracteres —lejos de los doscientos veinte—, sin imágenes, sin
    // automático, y «revisa» solo contaba como verbo de plan si era «revisa
    // todo». El aviso no habría saltado ni con el interruptor encendido.
    //
    // Y saltó justo lo que el aviso existe para prevenir: el modelo dijo «8
    // cores» con treinta y dos en pantalla, «Free Space: Not specified» con el
    // dato dos líneas más arriba en su propio contexto, y contestó medio en
    // portugués. Cuatro subsistemas y una síntesis es exactamente lo que un
    // modelo pequeño hace mal.
    let subsistemas = cuenta_subsistemas(&bajo);
    let sintetiza = SINTESIS.iter().any(|v| bajo.contains(v));
    if (subsistemas >= 2 && sintetiza) || subsistemas >= 3 {
        return Exigencia::Alta;
    }
    if t.texto.chars().count() >= TEXTO_LARGO {
        return Exigencia::Normal;
    }
    Exigencia::Simple
}

/// Palabras que piden JUNTAR, no consultar.
///
/// «Cuánta RAM tengo» es una lectura; «cómo está el equipo» es un juicio sobre
/// varias lecturas, y eso es lo que se le da mal a un modelo pequeño: no le
/// falta el dato, le falta sostener el criterio mientras cruza cuatro.
const SINTESIS: &[&str] = &[
    "resumen", "resume", "resumir", "estado general", "salud", "informe", "panorama",
    "cómo está", "como está", "cómo van", "como van", "qué tal está", "que tal está",
    "overview", "summary", "health",
];

/// Los subsistemas que nombra una orden, contados UNA VEZ CADA UNO.
///
/// Cada familia va con sus sinónimos porque el operador escribe como habla —
/// «procesador» un día y «CPU» al siguiente— y contar «CPU» y «procesador» como
/// dos subsistemas haría que «mira el uso de CPU del procesador» pareciera una
/// tarea de dos frentes.
fn cuenta_subsistemas(bajo: &str) -> usize {
    const FAMILIAS: &[&[&str]] = &[
        &["cpu", "procesador", "núcleo", "nucleo"],
        &["ram", "memoria"],
        &["disco", "almacenamiento", "espacio libre"],
        &["red", "network", "dns", "conectividad"],
        &["servicio", "service"],
        &["log", "evento", "registro de eventos"],
        &["certificad"],
        &["firewall", "cortafuegos"],
        &["usuario", "cuenta"],
        &["actualiza", "update", "parche"],
        &["puerto", "port"],
        &["tarea programada", "scheduled"],
        &["proceso"],
    ];
    FAMILIAS.iter().filter(|f| f.iter().any(|p| bajo.contains(p))).count()
}

/// Qué hay que decirle al operador antes de mandar.
#[derive(Debug, Clone, PartialEq)]
pub enum Aviso {
    /// El modelo no ve imágenes y hay imágenes. Es un impedimento, no una
    /// preferencia: la orden va a salir sin lo que se adjuntó.
    SinVision { modelo: String },
    /// El modelo es de los económicos y la tarea pide criterio.
    Debil { modelo: String, sugerido: Option<String> },
}

impl Aviso {
    /// La línea que se enseña. Dice el problema, la consecuencia y la salida —
    /// un aviso que solo dice «cuidado» se aprende a ignorar en dos días.
    pub fn texto(&self) -> String {
        match self {
            Aviso::SinVision { modelo } => format!(
                "«{modelo}» no interpreta imágenes: lo adjuntado no le va a llegar. Elige un \
                 modelo con visión o describe lo que se ve."
            ),
            Aviso::Debil { modelo, sugerido: Some(s) } => format!(
                "«{modelo}» es de los económicos y esto pide varios pasos. Con «{s}» acertaría \
                 más a la primera; si prefieres el barato, sigue sin tocar nada."
            ),
            Aviso::Debil { modelo, sugerido: None } => format!(
                "«{modelo}» es de los económicos y esto pide varios pasos. Puede que se quede \
                 corto; no hay ningún modelo más capaz configurado para sugerirte."
            ),
        }
    }
}

/// ¿Ve imágenes este modelo?
///
/// POR EXCLUSIÓN Y NO POR LISTA BLANCA. Los de nube que quedan hoy son todos
/// multimodales, y una lista blanca de ids envejece mal: el modelo nuevo que no
/// esté en ella daría un aviso falso, y un aviso falso enseña a ignorarlos.
/// Lo que sí se sabe con certeza es lo contrario — un modelo de Ollama solo ve
/// imágenes si es de los que las ven, y esos se cuentan con los dedos.
pub fn ve_imagenes(modelo: &str) -> bool {
    let m = modelo.to_ascii_lowercase();
    if crate::cloud::provider_of(modelo) != crate::cloud::Provider::Ollama {
        return true;
    }
    ["llava", "bakllava", "moondream", "llama3.2-vision", "gemma3", "minicpm-v", "qwen2.5vl"]
        .iter()
        .any(|v| m.contains(v))
}

/// El mejor modelo que el operador PUEDE usar hoy, para sugerirlo.
///
/// «Puede usar» es literal: se comprueba que haya clave guardada. Sugerir un
/// modelo de un proveedor sin configurar es mandar a alguien a una puerta
/// cerrada, y convierte el aviso en una molestia.
///
/// `tiene_clave` se inyecta para no atar este módulo al almacén de credenciales
/// —y para poder probarlo sin tocarlo.
pub fn sugerido(actual: &str, tiene_clave: impl Fn(&str) -> bool) -> Option<String> {
    let actual_base = actual.split_once("::").map_or(actual, |(b, _)| b);
    for g in crate::models::GROUPS {
        // Ollama queda fuera de las sugerencias: es local y no tiene clave, pero
        // sugerir «usa otro modelo local» cuando el problema es que el local se
        // queda corto no ayuda a nadie.
        if g.provider == "ollama" || !tiene_clave(g.provider) {
            continue;
        }
        for o in g.options {
            // ◆ insignia y ◇ intermedio son los que valen para trabajo de varios
            // pasos. El resto —rápidos, económicos, legado— no arreglarían nada.
            if (o.icon == "◆" || o.icon == "◇") && o.id != actual_base {
                return Some(o.id.to_string());
            }
        }
    }
    None
}

/// El aviso que corresponde, si hay alguno.
///
/// `None` = el modelo elegido vale para lo que se pide, que es el caso normal y
/// con diferencia.
pub fn revisa(modelo: &str, t: &Tarea, tiene_clave: impl Fn(&str) -> bool) -> Option<Aviso> {
    // LA VISIÓN PRIMERO: es lo único que impide de verdad, y avisar de que el
    // modelo es flojo cuando además ni siquiera va a ver la captura sería
    // contestar a la pregunta menos importante.
    if t.imagenes > 0 && !ve_imagenes(modelo) {
        return Some(Aviso::SinVision { modelo: modelo.to_string() });
    }
    if exigencia(t) == Exigencia::Alta && crate::prompt::model_is_weak(modelo) {
        return Some(Aviso::Debil {
            modelo: modelo.to_string(),
            sugerido: sugerido(modelo, tiene_clave),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    /// La orden REAL que dejó al descubierto el fallo. Sale de una captura del
    /// operador: la mandó con un modelo de 7B, y la respuesta dijo «8 cores»
    /// con treinta y dos en pantalla, «Free Space: Not specified» con el dato en
    /// su propio contexto, y vino medio en portugués. El aviso existe para eso y
    /// no habría saltado ni encendido.
    const LA_QUE_FALLABA: &str =
        "Revisa la salud del sistema (CPU, RAM, disco, servicios) y dame un resumen del estado.";

    #[test]
    fn cruzar_subsistemas_y_pedir_sintesis_es_exigente() {
        let t = Tarea { texto: LA_QUE_FALLABA, ..Default::default() };
        assert_eq!(
            exigencia(&t),
            Exigencia::Alta,
            "la orden que hizo falta el aviso sigue sin pedirlo"
        );
        // Ochenta y ocho caracteres: muy por debajo del umbral de longitud, así
        // que lo que la clasifica es el cruce y no el tamaño.
        assert!(LA_QUE_FALLABA.chars().count() < TEXTO_LARGO);
    }

    #[test]
    fn una_lectura_suelta_sigue_siendo_simple() {
        // LO QUE NO PUEDE ROMPERSE. Un aviso que salta en «¿cuánta RAM tengo?»
        // se aprende a ignorar en dos días, y entonces tampoco se lee el que sí
        // importaba.
        for texto in [
            "¿cuánta RAM tengo?",
            "mira el disco",
            "hostname",
            "¿qué procesos consumen más CPU?",
            "reinicia el spooler",
        ] {
            let t = Tarea { texto, ..Default::default() };
            assert_ne!(exigencia(&t), Exigencia::Alta, "«{texto}» no pide un modelo grande");
        }
    }

    #[test]
    fn tres_subsistemas_bastan_aunque_no_pida_resumen() {
        let t = Tarea {
            texto: "mira la red, los servicios y el disco",
            ..Default::default()
        };
        assert_eq!(exigencia(&t), Exigencia::Alta);
    }

    #[test]
    fn los_sinonimos_de_un_subsistema_no_cuentan_dos_veces() {
        // «Mira el uso de CPU del procesador» es UN frente, no dos. Sin esto, la
        // forma natural de hablar dispararía el aviso sola.
        let t = Tarea {
            texto: "mira el uso de CPU del procesador y sus núcleos",
            ..Default::default()
        };
        assert_ne!(exigencia(&t), Exigencia::Alta);
    }

    use super::*;

    fn t(texto: &str) -> Tarea<'_> {
        Tarea { texto, ..Default::default() }
    }

    #[test]
    fn una_pregunta_suelta_no_exige_nada() {
        assert_eq!(exigencia(&t("¿cuánta RAM tengo?")), Exigencia::Simple);
        assert_eq!(exigencia(&t("lista los servicios parados")), Exigencia::Simple);
    }

    #[test]
    fn el_trabajo_de_varios_pasos_se_reconoce_por_el_verbo() {
        // «mira el disco» es una lectura; «audita el disco» es un procedimiento.
        assert_eq!(exigencia(&t("mira el disco C")), Exigencia::Simple);
        assert_eq!(exigencia(&t("audita el disco C")), Exigencia::Alta);
        assert_eq!(exigencia(&t("diagnostica por qué va lento")), Exigencia::Alta);
        // Y en inglés, que es como se escribe media terminología del oficio.
        assert_eq!(exigencia(&t("haz un troubleshoot del DNS")), Exigencia::Alta);
    }

    #[test]
    fn el_automatico_es_exigente_por_si_solo() {
        // Encadenar comandos sin que nadie los mire pide criterio sostenido:
        // decidir el paso siguiente a la vista del anterior es justo lo que peor
        // hacen los modelos pequeños, y ahí no hay nadie para corregirlos.
        let a = Tarea { texto: "sigue", auto: true, ..Default::default() };
        assert_eq!(exigencia(&a), Exigencia::Alta);
    }

    #[test]
    fn una_imagen_no_es_una_pregunta_suelta_por_corta_que_sea() {
        let i = Tarea { texto: "¿qué es esto?", imagenes: 1, ..Default::default() };
        assert_eq!(exigencia(&i), Exigencia::Alta);
    }

    #[test]
    fn sin_vision_se_avisa_aunque_el_modelo_sea_potente() {
        // Es un impedimento y no una preferencia: la orden sale sin lo que se
        // adjuntó, y el modelo contesta sobre algo que no ha visto.
        let i = Tarea { texto: "¿qué ves?", imagenes: 1, ..Default::default() };
        let a = revisa("mistral:latest", &i, |_| true).expect("debe avisar");
        assert!(matches!(a, Aviso::SinVision { .. }));
        assert!(a.texto().contains("no le va a llegar"), "{}", a.texto());
        // Y un modelo local QUE SÍ VE no dispara el aviso de visión.
        assert!(!matches!(
            revisa("llava:13b", &i, |_| true),
            Some(Aviso::SinVision { .. })
        ));
    }

    #[test]
    fn la_vision_se_mira_antes_que_la_capacidad() {
        // Con los dos problemas a la vez, avisar de que el modelo es flojo
        // cuando además ni va a ver la captura sería contestar a la pregunta
        // menos importante.
        let i = Tarea { texto: "audita esto", imagenes: 1, ..Default::default() };
        assert!(matches!(
            revisa("qwen3:0.6b", &i, |_| true),
            Some(Aviso::SinVision { .. })
        ));
    }

    #[test]
    fn no_se_sugiere_un_proveedor_sin_clave() {
        // Mandar a alguien a una puerta cerrada convierte el aviso en una
        // molestia, y un aviso molesto se aprende a ignorar.
        let a = Tarea { texto: "audita el dominio", ..Default::default() };
        match revisa("qwen3:0.6b", &a, |_| false) {
            Some(Aviso::Debil { sugerido, .. }) => {
                assert!(sugerido.is_none(), "sugirió sin clave: {sugerido:?}")
            }
            otro => panic!("debería avisar de modelo débil, salió {otro:?}"),
        }
        // Con clave, sí sugiere — y algo distinto de lo que ya hay puesto.
        match revisa("qwen3:0.6b", &a, |p| p == "anthropic") {
            Some(Aviso::Debil { sugerido: Some(s), .. }) => {
                assert_ne!(s, "qwen3:0.6b");
                assert!(a.texto.is_empty() || !s.is_empty());
            }
            otro => panic!("debería sugerir, salió {otro:?}"),
        }
    }

    #[test]
    fn un_modelo_capaz_no_dispara_ningun_aviso() {
        // El caso normal, y con diferencia. Un enrutador que avisa siempre es
        // ruido, y el ruido se filtra ignorándolo entero.
        let a = Tarea { texto: "audita el controlador de dominio", ..Default::default() };
        assert_eq!(revisa("claude-sonnet-4-6", &a, |_| true), None);
        // Y tampoco para una pregunta suelta con un modelo pequeño: ahí el
        // modelo pequeño es exactamente la elección correcta.
        assert_eq!(revisa("qwen3:0.6b", &t("¿cuánta RAM tengo?"), |_| true), None);
    }
}
