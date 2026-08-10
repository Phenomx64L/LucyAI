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
    /// Lo que cobró el proveedor por ESTA tarea, sumando sus vueltas.
    ///
    /// Vuelve con el resultado y no por un canal aparte para que no se pueda
    /// recoger lo uno sin lo otro: el coste de un sub-agente es la parte de la
    /// factura que nadie ve pasar.
    pub tokens_in: u32,
    pub tokens_out: u32,
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
/// El interruptor es OBLIGATORIO en la firma, no un `Option`.
///
/// Nació sin él: `corre` llamaba a `cloud::start`, que es el atajo que se fabrica
/// un interruptor apagado y lo tira. Resultado: el botón de Detener limpiaba
/// `fork_rx` —el operador recuperaba su pestaña, que es lo que ve— y por debajo
/// seguían hasta cuatro peticiones de red por tarea, pagándose, contra un canal
/// que ya no escuchaba nadie. Pedirlo en la firma es lo que impide que el
/// siguiente sitio que lance una tarea lo vuelva a olvidar.
pub fn spawn(
    id: String,
    instruccion: String,
    modelo: String,
    equipo: String,
    privacy: bool,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::sync::mpsc::Receiver<ForkResult> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let t0 = std::time::Instant::now();
        let mut gasto = Gasto::default();
        let r = corre(&instruccion, &modelo, &equipo, privacy, &stop, &mut gasto);
        let ms = t0.elapsed().as_millis() as u64;
        let (text, ok) = match r {
            Ok(text) => (text, true),
            Err(e) => (e, false),
        };
        let _ = tx.send(ForkResult {
            id,
            text,
            ok,
            ms,
            tokens_in: gasto.tin,
            tokens_out: gasto.tout,
        });
    });
    rx
}

/// Lo que un sub-agente lleva cobrado. Se acumula ENTRE vueltas, no por vuelta:
/// lo que el operador paga es la tarea entera.
#[derive(Default)]
struct Gasto {
    tin: u32,
    tout: u32,
}

/// Cuántas vueltas de lectura puede dar un sub-agente antes de tener que
/// contestar con lo que tenga.
///
/// Tres. Cada vuelta es otra petición de red pagada, y un sub-agente no tiene a
/// nadie mirando que note que lleva veinte minutos listando carpetas. El tope no
/// es un límite de calidad sino de tiempo y dinero: si en tres lecturas no ha
/// encontrado lo que buscaba, lo que hay que devolver a la conversación
/// principal es eso —lo que miró y lo que no había— y no una cuarta corazonada.
///
/// SON TRES LECTURAS Y HASTA CUATRO PETICIONES, y conviene decirlo porque el
/// bucle lo disimula: la última vuelta corre igual pero sus herramientas ya no se
/// cumplen — es la que sirve para concluir con lo que se tenga. El mensaje que se
/// le manda al sub-agente contaba mal por eso y le prometía una lectura más de
/// las que iba a tener.
pub const MAX_VUELTAS: usize = 3;

fn corre(
    instruccion: &str,
    modelo: &str,
    equipo: &str,
    privacy: bool,
    stop: &std::sync::atomic::AtomicBool,
    gasto: &mut Gasto,
) -> Result<String, String> {
    crate::cloud::allowed(modelo, privacy)?;
    let mut turns = vec![
        Turn::system(system_prompt(equipo)),
        Turn::user(instruccion.to_string()),
    ];

    for vuelta in 0..=MAX_VUELTAS {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Cancelada por el operador.".into());
        }
        let out = un_turno(modelo, turns.clone(), stop, gasto)?;
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
            //
            // Y si no escribió NADA fuera de la etiqueta —que es lo normal en
            // esta vuelta: pide y calla— el aviso va solo. Antes salía un texto
            // que empezaba por dos saltos de línea y un corchete, con las cuatro
            // peticiones pagadas y ni una palabra de por qué.
            let dicho = limpia(out).unwrap_or_default();
            let nota = format!(
                "[la tarea agotó sus {MAX_VUELTAS} lecturas sin llegar a concluir; seguía \
                 pidiendo ficheros. Si necesitas eso, pídelo tú.]"
            );
            return Ok(recorta(if dicho.trim().is_empty() {
                nota
            } else {
                format!("{dicho}\n\n{nota}")
            }));
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
            // POR LA MISMA PUERTA QUE TODO LO DEMÁS. Aquí se armaba el sobre a
            // mano y sin revisar, que es el sitio MÁS goloso de los cuatro: un
            // sub-agente lee un log envenenado, se lo cree, y devuelve la
            // instrucción convertida en prosa propia. A partir de ahí ningún
            // patrón la reconoce — el lavado ya está hecho.
            resultados.push(crate::guard::tool_result(&name, &args, &cuerpo).block);
        }
        turns.push(Turn::assistant(out));
        // LAS QUE LE QUEDAN DE VERDAD. Decía `MAX_VUELTAS - vuelta`, que cuenta
        // también la última —la que corre pero ya no cumple herramientas—, así
        // que en la penúltima le prometía una lectura que no iba a tener. Un
        // presupuesto que miente es peor que no darlo: se lo gasta creyendo que
        // le sobra.
        turns.push(Turn::user(format!(
            "Esto devolvieron. {}\n\n{}",
            aviso_de_presupuesto(vuelta),
            resultados.join("\n\n")
        )));
    }
    unreachable!("el bucle sale por la última vuelta")
}

/// Lo que se le dice al sub-agente sobre lo que le queda, tras cumplirle la
/// vuelta `vuelta` (contando desde 0).
///
/// Está aparte para poder probar el conteo. Decía `MAX_VUELTAS - vuelta`, que
/// incluye la última vuelta —la que corre pero cuyas herramientas ya NO se
/// cumplen—, así que en la penúltima prometía una lectura que no existía. Un
/// presupuesto que miente es peor que no dar ninguno: se lo gasta creyendo que
/// le sobra, y las cuatro peticiones se pagan igual.
fn aviso_de_presupuesto(vuelta: usize) -> String {
    match MAX_VUELTAS.saturating_sub(vuelta + 1) {
        0 => "Es la ÚLTIMA: no vas a poder pedir más ficheros, así que responde con esto."
            .to_string(),
        1 => "Te queda 1 lectura.".to_string(),
        n => format!("Te quedan {n} lecturas."),
    }
}

fn un_turno(
    modelo: &str,
    turns: Vec<Turn>,
    stop: &std::sync::atomic::AtomicBool,
    gasto: &mut Gasto,
) -> Result<String, String> {
    let mut out = String::new();
    let mut err = None;
    // El MISMO interruptor que el turno principal. El hilo del proveedor lo mira
    // entre trama y trama, así que parar deja de pagar tokens de verdad, no solo
    // de mirarlos.
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
        stop.load(std::sync::atomic::Ordering::Relaxed),
    ));
    for ev in crate::cloud::start_cancellable(modelo.to_string(), turns, flag.clone()) {
        // Se propaga en cada evento porque el `Arc` que ve el proveedor es otro:
        // el de la pestaña vive en la interfaz y este hilo solo tiene una
        // referencia prestada.
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        match ev {
            ChatEvent::Token(t) => out.push_str(&t),
            // LO QUE COBRA UN SUB-AGENTE SE CUENTA. Este brazo era `_ => {}`, y
            // con hasta cinco tareas de hasta cuatro peticiones cada una, el
            // contador de coste de la pestaña podía ir veinte llamadas por
            // detrás de la factura.
            ChatEvent::Usage(i, o) => {
                gasto.tin += i;
                gasto.tout += o;
            }
            ChatEvent::Error(e) => err = Some(e),
            _ => {}
        }
    }
    if stop.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Cancelada por el operador.".into());
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
    fn el_presupuesto_que_se_le_promete_es_el_que_va_a_tener() {
        // EL DESFASE DE UNO. `MAX_VUELTAS - vuelta` cuenta también la vuelta que
        // corre sin cumplir herramientas, así que en la penúltima le prometía una
        // lectura que no iba a existir. Se lo gasta creyendo que le sobra.
        //
        // Con MAX_VUELTAS = 3 se cumplen las vueltas 0, 1 y 2. Tras la 0 quedan
        // dos; tras la 1, una; tras la 2, ninguna.
        assert_eq!(aviso_de_presupuesto(0), "Te quedan 2 lecturas.");
        assert_eq!(aviso_de_presupuesto(1), "Te queda 1 lectura.");
        assert!(aviso_de_presupuesto(2).contains("ÚLTIMA"), "{}", aviso_de_presupuesto(2));
        // Y en la última no se le pide que pida: se le pide que conteste.
        assert!(aviso_de_presupuesto(2).contains("responde"));
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
