//! Sub-agentes: tareas que Lucy lanza en paralelo y recoge después.
//!
//! EL MODELO YA ESTABA ESCRITO Y NADIE LO CREABA. `Workspace` tiene `AgentFork`
//! con su estado de cuatro valores, la interfaz cuenta los que están corriendo,
//! y no había forma de lanzar ninguno. Es el mismo patrón que el de `<REMEMBER>`
//! y el de la consolidación de memorias: una pieza terminada sin la línea que la
//! enciende.
//!
//! PARA QUÉ SIRVE. Revisar cinco servidores, mirar cuatro logs, comprobar tres
//! configuraciones: tareas que no dependen entre sí y que, en serie, son cinco
//! esperas de red seguidas. En paralelo es una.
//!
//! UN SUB-AGENTE NO EJECUTA NADA, y esa es la decisión de fondo de este módulo.
//! Puede leer —ficheros, carpetas, tramos— y tiene que contar lo que encontró.
//! No propone comandos, no escribe ficheros y no toca equipos remotos. La razón
//! es aritmética: el bucle automático ya corre comandos sin que nadie los lea, y
//! permitir que cada sub-agente haga lo mismo multiplica por cinco la superficie
//! de algo que apenas se ha probado con uno. Si de lo que encuentra sale algo que
//! hay que ejecutar, lo propone la conversación principal, donde hay una persona
//! mirando el panel de Plan.

use crate::chat::ChatEvent;
use crate::turns::Turn;

/// Cuántos sub-agentes pueden estar en vuelo a la vez.
///
/// Cinco. Cada uno es una petición de red pagada y una conversación entera con
/// su prompt de sistema, así que diez lanzados a la vez es una factura
/// sorprendente y un contexto que ya nadie sigue. El tope existe para que un
/// modelo que se emociona no convierta una pregunta en veinte.
pub const MAX_PARALELOS: usize = 5;

/// Tope de lo que un sub-agente devuelve a la conversación principal.
///
/// Lo que vuelve se mete ENTERO en el prompt del turno siguiente. Cinco
/// sub-agentes con respuestas de dos mil palabras llenarían la ventana de
/// contexto con material que la conversación principal solo necesita resumido.
pub const MAX_RESULTADO: usize = 4_000;

/// El prompt de sistema de un sub-agente.
///
/// CORTO Y EXPLÍCITO SOBRE LO QUE NO PUEDE. Un sub-agente que hereda el prompt
/// entero hereda también las etiquetas de ejecución, y entonces propone comandos
/// que nadie va a ejecutar: gasta su turno y devuelve una intención en vez de un
/// hallazgo. Decírselo de entrada convierte eso en lo que sí sirve — que mire y
/// cuente.
pub fn system_prompt(equipo: &str) -> String {
    format!(
        "Eres una tarea auxiliar de Lucy, la asistente de administración de sistemas de \
         este equipo ({equipo}). Te han encargado UNA cosa concreta; hazla y cuenta lo \
         que encuentres.\n\n\
         SOLO PUEDES LEER. Tienes estas herramientas y ninguna más:\n\
         · <TOOL>readfile:C:\\ruta</TOOL>\n\
         · <TOOL>listdir:C:\\ruta</TOOL>\n\
         · <TOOL>readlines:C:\\ruta|desde|cuántas</TOOL>\n\
         NO ejecutas comandos, NO escribes ficheros y NO tocas equipos remotos. Si para \
         responder hiciera falta algo de eso, dilo y explica qué haría falta: quien \
         decide es la conversación principal, donde hay una persona mirando.\n\n\
         Responde en español, breve y con lo que hayas MEDIDO. Si no encontraste nada, \
         dilo — «no hay errores en ese log» es un resultado útil, y una suposición \
         adornada no lo es."
    )
}

/// El resultado de un sub-agente, listo para volver a la conversación.
#[derive(Debug, Clone, PartialEq)]
pub struct ForkResult {
    pub id: String,
    pub text: String,
    pub ok: bool,
    pub ms: u64,
}

/// Separa un `fork_task:id|instrucción`.
///
/// El id lo pone el modelo y se respeta tal cual: es el nombre por el que va a
/// pedirlo después con `wait_task`, y renombrárselo aquí haría que no
/// encontrara lo que él mismo lanzó.
pub fn parse_fork(args: &str) -> Option<(String, String)> {
    let (id, inst) = args.split_once('|')?;
    let (id, inst) = (id.trim(), inst.trim());
    if id.is_empty() || inst.is_empty() {
        return None;
    }
    Some((id.to_string(), inst.to_string()))
}

/// Qué tareas nombra un `wait_task`.
///
/// `*`, `todas` o nada significan TODAS las que quedan por recoger. Es la forma
/// que se usa cuando se lanzaron cuatro de golpe, y sin ella Lucy escribiría
/// cuatro etiquetas para lo que es una sola espera. Por comas también, porque
/// esperar tres de las cinco sigue siendo UNA espera.
///
/// Las ya recogidas no entran en el «todas»: pedirlas otra vez sin nombrarlas
/// devolvería en cada turno el volcado entero de todo lo que se hizo antes.
pub fn pedidos(forks: &[crate::agent::AgentFork], arg: &str) -> Vec<String> {
    let arg = arg.trim();
    if arg.is_empty() || arg == "*" || arg.eq_ignore_ascii_case("todas") {
        return forks
            .iter()
            .filter(|f| f.status != crate::agent::ForkStatus::Collected)
            .map(|f| f.id.clone())
            .collect();
    }
    let mut out: Vec<String> = Vec::new();
    for p in arg.split(',') {
        let p = p.trim();
        // Repetir un nombre en la misma espera es un descuido, y contestarlo dos
        // veces mandaría el mismo volcado dos veces en el mismo turno.
        if !p.is_empty() && !out.iter().any(|x| x == p) {
            out.push(p.to_string());
        }
    }
    out
}

/// Lanza un sub-agente. Devuelve el canal por el que llegará su resultado.
///
/// Se acumula el flujo entero en vez de irlo enseñando: lo que produce un
/// sub-agente no es una conversación que alguien esté leyendo, es un dato que la
/// principal va a usar. Enseñarlo token a token en un carril aparte sería
/// movimiento sin información.
pub fn spawn(
    id: String,
    instruccion: String,
    modelo: String,
    equipo: String,
    privacy: bool,
) -> std::sync::mpsc::Receiver<ForkResult> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let t0 = std::time::Instant::now();
        let r = corre(&instruccion, &modelo, &equipo, privacy);
        let ms = t0.elapsed().as_millis() as u64;
        let _ = tx.send(match r {
            Ok(text) => ForkResult { id, text, ok: true, ms },
            Err(e) => ForkResult { id, text: e, ok: false, ms },
        });
    });
    rx
}

/// Cuántas vueltas de lectura puede dar un sub-agente antes de tener que
/// contestar con lo que tenga.
///
/// Tres. Cada vuelta es otra petición de red pagada, y un sub-agente no tiene a
/// nadie mirando que note que lleva veinte minutos listando carpetas. El tope no
/// es un límite de calidad sino de tiempo y dinero: si en tres lecturas no ha
/// encontrado lo que buscaba, lo que hay que devolver a la conversación
/// principal es eso —lo que miró y lo que no había— y no una cuarta corazonada.
pub const MAX_VUELTAS: usize = 3;

fn corre(instruccion: &str, modelo: &str, equipo: &str, privacy: bool) -> Result<String, String> {
    crate::cloud::allowed(modelo, privacy)?;
    let mut turns = vec![
        Turn::system(system_prompt(equipo)),
        Turn::user(instruccion.to_string()),
    ];

    for vuelta in 0..=MAX_VUELTAS {
        let out = un_turno(modelo, turns.clone())?;
        // LAS HERRAMIENTAS DE UN SUB-AGENTE SE CUMPLEN AQUÍ, en su propio hilo,
        // y no vuelven a la conversación principal. Sin este bucle el prompt de
        // arriba sería una mentira de tres líneas: le ofrece `readfile`, la
        // pide, y nadie se la cumple — exactamente el fallo del que salió
        // `tools.rs`, repetido un nivel más abajo.
        let peticiones: Vec<(String, String)> = crate::tags::extract_tags(&out)
            .into_iter()
            .filter(|t| t.kind == crate::tags::TagKind::Tool)
            .map(|t| crate::tags::parse_tool(&t.content))
            .collect();
        if peticiones.is_empty() {
            return Ok(recorta(limpia(out)?));
        }
        if vuelta == MAX_VUELTAS {
            // Se acabaron las vueltas CON una petición sin cumplir. Lo honesto
            // es devolver lo que haya escrito diciendo que se quedó a medias:
            // callarlo haría que la conversación principal tomara por conclusión
            // lo que era el principio de una lectura.
            let dicho = limpia(out).unwrap_or_default();
            return Ok(recorta(format!(
                "{dicho}\n\n[la tarea agotó sus {MAX_VUELTAS} lecturas y no llegó a \
                 concluir — lo de arriba es lo que alcanzó a ver]"
            )));
        }

        let mut resultados = Vec::new();
        for (name, args) in peticiones {
            // `run` y NO `run_with_skills`: las tres de lectura y nada más. Que
            // el catálogo corto salga de la función que se llama, y no de una
            // lista de prohibidas que hay que acordarse de ampliar, es lo que
            // hace que añadir mañana una herramienta que escriba no se la regale
            // a los sub-agentes por olvido.
            let cuerpo = match crate::tools::run(&name, &args) {
                Some(r) => r.body,
                None => format!(
                    "«{name}» no existe para una tarea auxiliar. Solo tienes readfile, \
                     listdir y readlines. Si hace falta ejecutar algo, dilo en tu \
                     respuesta y lo decide la conversación principal."
                ),
            };
            resultados.push(format!(
                "<TOOL_RESULT tool=\"{name}\" arg=\"{args}\">\n{cuerpo}\n</TOOL_RESULT>"
            ));
        }
        turns.push(Turn::assistant(out));
        turns.push(Turn::user(format!(
            "Esto devolvieron. Te quedan {} lecturas.\n\n{}",
            MAX_VUELTAS - vuelta,
            resultados.join("\n\n")
        )));
    }
    unreachable!("el bucle sale por la última vuelta")
}

fn un_turno(modelo: &str, turns: Vec<Turn>) -> Result<String, String> {
    let mut out = String::new();
    let mut err = None;
    for ev in crate::cloud::start(modelo.to_string(), turns) {
        match ev {
            ChatEvent::Token(t) => out.push_str(&t),
            ChatEvent::Error(e) => err = Some(e),
            _ => {}
        }
    }
    match err {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

/// Quita las etiquetas de acción de lo que vuelve.
///
/// Un sub-agente que igualmente escribió un `<EXECUTE>` no puede colarlo en la
/// conversación principal: allí SÍ hay quien lo ejecute, y ese comando no lo ha
/// leído nadie ni ha pasado por el guardrail de un turno de verdad.
fn limpia(out: String) -> Result<String, String> {
    let limpio = crate::tags::clean_display(&out).text;
    if limpio.trim().is_empty() {
        return Err("La tarea no devolvió nada.".into());
    }
    Ok(limpio)
}

fn recorta(s: String) -> String {
    if s.chars().count() <= MAX_RESULTADO {
        return s;
    }
    let cut: String = s.chars().take(MAX_RESULTADO).collect();
    format!("{cut}\n… (truncado)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_respeta_el_id_que_puso_el_modelo() {
        // Es el nombre por el que va a pedirlo con `wait_task`. Renombrarlo aquí
        // haría que no encontrara lo que él mismo lanzó.
        let (id, inst) = parse_fork("disco-c | mira cuánto queda libre en C:").unwrap();
        assert_eq!(id, "disco-c");
        assert_eq!(inst, "mira cuánto queda libre en C:");
    }

    #[test]
    fn una_llamada_a_medias_no_lanza_nada() {
        // Lanzar una tarea sin instrucción gasta una petición de red para
        // preguntarle a un modelo qué quería el otro modelo.
        assert!(parse_fork("solo-id").is_none());
        assert!(parse_fork("|sin id").is_none());
        assert!(parse_fork("id|").is_none());
        assert!(parse_fork("").is_none());
    }

    #[test]
    fn al_sub_agente_se_le_dice_lo_que_no_puede_hacer() {
        // Sin esto hereda la idea de que ejecuta, propone un comando que nadie
        // va a correr, y devuelve una intención en vez de un hallazgo.
        let p = system_prompt("WORKSTATION-16");
        assert!(p.contains("SOLO PUEDES LEER"));
        assert!(p.contains("NO ejecutas comandos"));
        assert!(p.contains("WORKSTATION-16"), "no sabe en qué equipo está");
        // Y las tres que sí tiene, con su forma.
        for t in ["readfile", "listdir", "readlines"] {
            assert!(p.contains(t), "no se le ofrece {t}");
        }
        // Y permiso para no encontrar nada, que es un resultado.
        assert!(p.contains("es un resultado"));
    }

    fn ws_con(estados: &[(&str, crate::agent::ForkStatus)]) -> Vec<crate::agent::AgentFork> {
        estados
            .iter()
            .map(|(id, s)| crate::agent::AgentFork {
                id: (*id).to_string(),
                instruction: String::new(),
                model: String::new(),
                status: *s,
                result: String::new(),
                ms: None,
                ts: 0,
            })
            .collect()
    }

    #[test]
    fn esperar_todas_no_repite_las_ya_recogidas() {
        use crate::agent::ForkStatus::*;
        // Sin este filtro, cada `wait_task:*` de una conversación larga
        // devolvería otra vez el volcado entero de todo lo anterior.
        let fs = ws_con(&[("a", Collected), ("b", Running), ("c", Done)]);
        assert_eq!(pedidos(&fs, "*"), vec!["b", "c"]);
        assert_eq!(pedidos(&fs, ""), vec!["b", "c"], "sin argumento es lo mismo que *");
        assert_eq!(pedidos(&fs, "  todas "), vec!["b", "c"]);
    }

    #[test]
    fn esperar_tres_de_cinco_sigue_siendo_una_espera() {
        let fs = ws_con(&[("a", crate::agent::ForkStatus::Running)]);
        assert_eq!(pedidos(&fs, "a, b ,c"), vec!["a", "b", "c"]);
        // Un nombre repetido no se contesta dos veces en el mismo turno.
        assert_eq!(pedidos(&fs, "a,a"), vec!["a"]);
    }

    #[test]
    fn un_sub_agente_no_alcanza_las_herramientas_que_escriben() {
        // El catálogo corto sale de qué función se llama —`run` y no
        // `run_with_skills`—, no de una lista de prohibidas. Si mañana alguien
        // mete una herramienta que escriba en `run`, este test cae y hay que
        // decidirlo a propósito en vez de regalársela a los sub-agentes.
        for prohibida in ["writefile", "editfile", "skill"] {
            assert!(
                crate::tools::run(prohibida, "lo que sea").is_none(),
                "un sub-agente alcanza {prohibida}"
            );
        }
        assert!(crate::tools::run("readfile", "C:\\no-existe").is_some());
    }

    #[test]
    fn lo_que_vuelve_no_lleva_comandos() {
        // En la conversación principal SÍ hay quien ejecute, y ese comando no lo
        // ha leído nadie ni ha pasado por el guardrail de un turno de verdad.
        let r = limpia("Hay poco espacio.\n<EXECUTE>Remove-Item C:\\* -Recurse</EXECUTE>".into())
            .unwrap();
        assert!(r.contains("Hay poco espacio"));
        assert!(!r.contains("Remove-Item"), "coló un comando: {r}");
    }

    #[test]
    fn un_resultado_enorme_se_recorta_y_lo_dice() {
        // Lo que vuelve entra ENTERO en el prompt del turno siguiente. Cinco
        // tareas sin tope llenan la ventana de contexto con material que la
        // conversación principal solo necesitaba resumido.
        let largo = recorta("á".repeat(MAX_RESULTADO + 200));
        assert!(largo.ends_with("… (truncado)"));
        // Por caracteres y no por bytes: cortar por índice de byte en medio de
        // un carácter multibyte es un pánico, y esto viene de un log en español.
        assert!(largo.starts_with('á'));
        assert_eq!(recorta("corto".into()), "corto", "lo que cabe no se toca");
    }

    #[test]
    fn el_tope_de_paralelos_es_pequeno_a_proposito() {
        // Cada uno es una petición pagada y una conversación entera. Diez a la
        // vez es una factura sorprendente y un contexto que ya nadie sigue.
        assert!(MAX_PARALELOS <= 5);
        assert!(MAX_PARALELOS >= 2, "menos de dos no es paralelo");
    }
}
