//! Los cuatro atajos de la pantalla vacía, escritos para ESTA máquina.
//!
//! LOS DE FÁBRICA SON GENÉRICOS Y POR ESO ENVEJECEN MAL. «Salud del sistema»,
//! «Vulnerabilidades», «Servicios detenidos», «Errores recientes»: valen el
//! primer día y a la semana ya no dicen nada, porque no miran lo que le pasa al
//! equipo que tienes delante. Si hay dos servicios caídos y el disco al 93 %, un
//! atajo que dice «Salud del sistema» está tapando la respuesta con la pregunta.
//!
//! LOS ESCRIBE UN MODELO LOCAL Y PEQUEÑO, a propósito. Esto se recalcula al
//! abrir la aplicación y cada pocos minutos; con un modelo de nube sería un
//! goteo de céntimos para siempre por algo que nadie ha pedido, y encima
//! mandaría fuera el estado del equipo —servicios caídos, errores del log— en
//! cada refresco. Se elige EL MÁS PEQUEÑO que sepa contestar, no el mejor: la
//! tarea es «mira estos datos y escribe cuatro frases», y para eso un modelo de
//! seiscientos millones de parámetros en CPU vale y no calienta la máquina.
//!
//! FORMATO PLANO, NUNCA ANIDADO. Es la lección que costó un día en `crystals`:
//! `mistral` devolvía `<outcomes>texto</outcome>` cerrando la etiqueta que no
//! era. Un modelo de 0.6B es mucho peor que mistral, así que aquí se pide una
//! línea por atajo con una barra en medio y se descarta sin drama lo que no
//! encaje. Un atajo malo se tira; cuatro atajos malos dejan la pantalla como
//! estaba, que es un sitio perfectamente bueno donde quedarse.

/// Un atajo: lo que se lee y lo que se manda.
///
/// SON DOS TEXTOS DISTINTOS y no uno. El chip dice de qué va en dos palabras;
/// lo que viaja es una instrucción entera. Un chip que enviara su propia
/// etiqueta le daría a Lucy tres palabras sueltas en lugar de una tarea.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub etiqueta: String,
    pub orden: String,
}

/// Cuántos caben en la pantalla vacía.
pub const MAX: usize = 4;
/// Lo que como mucho ocupa una etiqueta antes de romper la rejilla de chips.
pub const MAX_ETIQUETA: usize = 26;
/// Y lo mínimo que tiene que medir una orden para ser una tarea y no un ruido.
pub const MIN_ORDEN: usize = 20;
pub const MAX_ORDEN: usize = 240;

/// Plazo. Corto: esto adorna una pantalla vacía, no responde a nadie.
pub const TIMEOUT_SECS: u64 = 45;

/// Cada cuánto se vuelven a pedir.
///
/// DOCE HORAS, Y ESO ES DE PROPÓSITO LARGO. Lo que proponen estos atajos cambia
/// con el equipo, y un equipo no cambia en veinte minutos: si el disco se llena
/// hoy, mañana sigue lleno. Refrescarlos a menudo no daría atajos mejores —
/// daría los mismos escritos de otra manera — y con el respaldo de nube sería un
/// goteo de céntimos por reescribir lo que ya estaba bien.
///
/// Y hay una razón de uso además de la del gasto: unos atajos que cambian cada
/// vez que abres la pantalla no se aprenden nunca. Que estén quietos medio día
/// es lo que permite reconocerlos de un vistazo.
pub const CADA_SECS: i64 = 12 * 3_600;

/// ¿Toca volver a pedirlos?
///
/// La misma regla que el mantenimiento: no haberlos pedido nunca CUENTA como
/// vencido, y un reloj que va hacia atrás también. Sin lo segundo, cambiar la
/// hora del equipo dejaría los atajos congelados hasta que el reloj volviera a
/// alcanzar la marca.
pub fn vencido(ultima: Option<i64>, ahora: i64) -> bool {
    match ultima {
        None => true,
        Some(t) if ahora < t => true,
        Some(t) => ahora - t >= CADA_SECS,
    }
}

/// Quién escribe los atajos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fuente {
    /// Un modelo de Ollama. No cuesta y no sale del equipo.
    Local(String),
    /// El de nube más barato con clave. Ver [`elige_fuente`].
    Nube(String),
}

impl Fuente {
    pub fn modelo(&self) -> &str {
        match self {
            Fuente::Local(m) | Fuente::Nube(m) => m,
        }
    }
}

/// Local primero; si no hay, el de nube más barato; con privacidad, nadie.
///
/// EL RESPALDO DE NUBE ES DEFENDIBLE AQUÍ Y NO EN CUALQUIER SITIO, y la razón es
/// el plazo de arriba: dos llamadas al día a un modelo ligero no se notan en la
/// factura. Con un refresco de minutos sería otra conversación.
///
/// CON PRIVACIDAD, NADIE, aunque haya clave: el contexto que viaja son los
/// servicios caídos y los últimos errores del log de ESTE equipo. Es
/// exactamente lo que ese modo promete que no sale de aquí, y que sea para
/// adornar una pantalla no lo hace menos cierto.
pub fn elige_fuente(instalados: &[String], privacidad: bool) -> Option<Fuente> {
    if let Some(m) = elige(instalados) {
        return Some(Fuente::Local(m));
    }
    if privacidad {
        return None;
    }
    crate::titles::baratos_con_clave().into_iter().next().map(Fuente::Nube)
}

const OLLAMA: &str = "http://127.0.0.1:11434";

const SISTEMA: &str = "Eres el ayudante que propone atajos en la pantalla de inicio de una \
herramienta de administración de Windows.\n\n\
Te dan el estado real de un equipo. Devuelves como mucho CUATRO atajos: cosas \
concretas que al administrador le convenga mirar HOY en ESTE equipo.\n\n\
FORMATO — una línea por atajo, exactamente así:\n\
ATAJO: etiqueta corta | la orden completa que se le enviará a Lucy\n\n\
REGLAS:\n\
- La etiqueta, como mucho tres palabras. Nombra el ASUNTO concreto.\n\
- La orden, una instrucción entera en español, como se la dirías a una persona.\n\
- NO escribas comandos de PowerShell. Lucy los decide ella.\n\
- Prioriza lo que esté MAL en los datos: un servicio caído, un disco lleno, \
errores repetidos. Si no hay nada mal, propón revisiones útiles y normales.\n\
- Nada de preámbulos ni de explicaciones. Solo las líneas ATAJO:.";

/// Elige quién escribe los atajos: el modelo local MÁS PEQUEÑO que sepa hablar.
///
/// EL MÁS PEQUEÑO Y NO EL MEJOR, que es al revés que en todo lo demás. Los
/// cristales destilan una sesión entera y ahí el tamaño se nota; esto es leer
/// ocho líneas de estado y escribir cuatro frases. Con el modelo grande, cada
/// refresco ocupa la GPU y la CPU durante segundos para adornar una pantalla que
/// el operador va a abandonar en cuanto escriba — y si está en mitad de otra
/// cosa, se lo nota.
///
/// Los que no dicen su tamaño (`mistral:latest`) van al final: pueden ser
/// enormes, y en la duda primero los que sabemos que son pequeños.
pub fn elige(instalados: &[String]) -> Option<String> {
    let mut texto: Vec<&String> = instalados
        .iter()
        .filter(|m| {
            let b = m.to_ascii_lowercase();
            // Los de embeddings no tienen `/api/chat`: pedírselo devuelve un
            // error que parece de red y manda a mirar el sitio equivocado.
            !b.contains("embed") && !b.contains("bge-") && !b.contains("minilm")
        })
        .collect();
    texto.sort_by(|a, b| {
        let ta = crate::prompt::tam_b(a).unwrap_or(f32::INFINITY);
        let tb = crate::prompt::tam_b(b).unwrap_or(f32::INFINITY);
        ta.total_cmp(&tb).then(a.cmp(b))
    });
    texto.first().map(|m| (*m).clone())
}

/// El estado del equipo, en las pocas líneas que un modelo pequeño puede leer.
///
/// CORTO A PROPÓSITO. A un 0.6B se le puede dar un párrafo, no un informe: con
/// más contexto del que aguanta, deja de mirar los datos y empieza a repetir el
/// formato del ejemplo. Van los cinco números que deciden si algo va mal y los
/// nombres de lo que esté roto, que es de donde salen los atajos buenos.
pub fn contexto(
    s: &crate::system::SysSnapshot,
    caidos: &[crate::system::DownService],
    errores: &[String],
) -> String {
    use std::fmt::Write;
    let mut t = String::new();
    let _ = writeln!(t, "Equipo: {} · {}", s.host, s.os);
    let _ = writeln!(t, "CPU: {:.0}% de uso, {} núcleos", s.cpu_pct, s.cores);
    let mem = if s.mem_total > 0 {
        s.mem_used as f64 / s.mem_total as f64 * 100.0
    } else {
        0.0
    };
    let _ = writeln!(
        t,
        "RAM: {:.0}% ocupada ({:.1} de {:.1} GB)",
        mem,
        s.mem_used as f64 / 1e9,
        s.mem_total as f64 / 1e9
    );
    for d in &s.disks {
        if d.total == 0 {
            continue;
        }
        let pct = d.total.saturating_sub(d.avail) as f64 / d.total as f64 * 100.0;
        let _ = writeln!(
            t,
            "Disco {}: {:.0}% ocupado, quedan {:.0} GB",
            d.mount,
            pct,
            d.avail as f64 / 1e9
        );
    }
    if caidos.is_empty() {
        let _ = writeln!(t, "Servicios automáticos caídos: ninguno");
    } else {
        let nombres: Vec<&str> = caidos.iter().take(6).map(|c| c.name.as_str()).collect();
        let _ = writeln!(
            t,
            "Servicios automáticos CAÍDOS ({}): {}",
            caidos.len(),
            nombres.join(", ")
        );
    }
    // Solo las últimas y recortadas: una traza de Windows son doscientos
    // caracteres y cuatro llenarían el contexto entero del modelo.
    let recientes: Vec<String> = errores
        .iter()
        .rev()
        .take(3)
        .map(|e| e.chars().take(120).collect::<String>())
        .collect();
    if !recientes.is_empty() {
        let _ = writeln!(t, "Últimos errores del log:");
        for e in recientes {
            let _ = writeln!(t, "- {}", e.trim());
        }
    }
    t
}

/// Saca los atajos de lo que devuelva el modelo.
///
/// TIRA LO QUE NO ENCAJE, una línea a una. Con un modelo de este tamaño, que dos
/// de cuatro salgan bien es un buen día — y dos atajos buenos más dos de fábrica
/// es mejor pantalla que cuatro de fábrica. Lo que no se hace nunca es enseñar
/// una línea a medias por no perderla.
pub fn parse(salida: &str) -> Vec<Chip> {
    // Los razonadores abren `<think>`: dentro hay frases con dos puntos y barras
    // que parecen atajos y no lo son.
    let limpio = match salida.rfind("</think>") {
        Some(i) => &salida[i + "</think>".len()..],
        None => salida,
    };
    let mut out: Vec<Chip> = Vec::new();
    for linea in limpio.lines() {
        let l = linea.trim();
        // La marca, tolerando que el modelo la adorne con viñetas o asteriscos.
        let l = l.trim_start_matches(['-', '*', '·', '•', ' ', '\t']);
        let bajo = l.to_ascii_lowercase();
        let Some(p) = bajo.find("atajo:") else { continue };
        let cuerpo = l[p + "atajo:".len()..].trim();
        let Some((et, orden)) = cuerpo.split_once('|') else { continue };
        // EL ESPACIO VA EN EL MISMO CONJUNTO que los adornos, no en un `trim`
        // aparte. Con `**ATAJO:** «Servicios caídos»` el sobrante es `** «…»`, y
        // quitando primero los asteriscos el recorte se para en el espacio que
        // viene detrás: quedaba `«Servicios caídos`, con la comilla de apertura
        // pegada. Un solo recorte que se coma las dos cosas lo resuelve.
        let et = et
            .trim_matches(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '«' | '»' | '*' | '#' | ':')
            })
            .to_string();
        let orden = orden.trim().to_string();
        if et.is_empty() || et.chars().count() > MAX_ETIQUETA {
            continue;
        }
        let n = orden.chars().count();
        if !(MIN_ORDEN..=MAX_ORDEN).contains(&n) {
            continue;
        }
        // UN COMANDO NO ES UNA ORDEN. Si el modelo ignora la regla y devuelve
        // PowerShell, ese atajo mandaría a Lucy un comando que nadie ha
        // revisado, saltándose el paso en el que el operador lo aprueba.
        if parece_comando(&orden) {
            continue;
        }
        // Sin repetir etiqueta: los modelos pequeños se enrocan y devuelven la
        // misma cuatro veces con distinta redacción.
        if out.iter().any(|c| c.etiqueta.eq_ignore_ascii_case(&et)) {
            continue;
        }
        out.push(Chip { etiqueta: et, orden });
        if out.len() == MAX {
            break;
        }
    }
    out
}

/// ¿Esto es un comando en vez de una instrucción?
fn parece_comando(s: &str) -> bool {
    let b = s.to_ascii_lowercase();
    b.starts_with("get-")
        || b.starts_with("set-")
        || b.starts_with("start-")
        || b.starts_with("stop-")
        || b.starts_with("restart-")
        || b.contains("| select-object")
        || b.contains("powershell -")
        || b.contains("cmd /c")
        || s.contains("```")
}

/// Pide los atajos a quien toque. BLOQUEANTE: quien llama ya está en un hilo.
///
/// Devuelve además los tokens que dijo el proveedor, para que el gasto se apunte
/// igual que el de un turno. Un coste que no se apunta es un coste que aparece
/// en la factura sin haber salido nunca en la barra de estado.
pub fn pide_a(ctx: &str, f: &Fuente) -> Result<(Vec<Chip>, u32, u32), String> {
    match f {
        Fuente::Local(m) => pide(ctx, m),
        Fuente::Nube(m) => nube(ctx, m),
    }
}

/// La nube, drenando el stream que ya usa el resto de la aplicación.
fn nube(ctx: &str, modelo: &str) -> Result<(Vec<Chip>, u32, u32), String> {
    use crate::chat::ChatEvent;
    use crate::turns::Turn;
    let turnos = vec![
        Turn::system(SISTEMA),
        Turn::user(format!("Estado del equipo:\n\n{ctx}")),
    ];
    let rx = crate::cloud::start(modelo.to_string(), turnos);
    let limite = std::time::Duration::from_secs(TIMEOUT_SECS);
    let (mut texto, mut ent, mut sal) = (String::new(), 0, 0);
    loop {
        match rx.recv_timeout(limite) {
            Ok(ChatEvent::Token(t)) => texto.push_str(&t),
            Ok(ChatEvent::Usage(i, o)) => (ent, sal) = (i, o),
            Ok(ChatEvent::Done) => break,
            Ok(ChatEvent::Error(e)) => return Err(e),
            // Se acabó el plazo o se cayó el hilo. Con lo que haya llegado
            // basta: `parse` se queda con las líneas enteras y tira la última si
            // vino cortada.
            Err(_) => break,
        }
    }
    Ok((parse(&texto), ent, sal))
}

/// Pide los atajos a Ollama. BLOQUEANTE.
///
/// DEVUELVE SUS RECUENTOS, por lo mismo que `titles::local`: Ollama los manda en
/// la misma respuesta y descartarlos dejaba el cubo «chips» del gasto vacío para
/// siempre, sin forma de distinguir «no cuesta» de «no se mide».
pub fn pide(ctx: &str, modelo: &str) -> Result<(Vec<Chip>, u32, u32), String> {
    let cuerpo = serde_json::json!({
        "model": modelo,
        "messages": [
            { "role": "system", "content": SISTEMA },
            { "role": "user", "content": format!("Estado del equipo:\n\n{ctx}") },
        ],
        "stream": false,
        // Sitio para cuatro líneas y para que un razonador piense antes: por
        // debajo, `qwen3` gasta el presupuesto entero en `<think>` y cierra sin
        // haber escrito ningún atajo.
        "options": { "temperature": 0.4, "num_predict": 700 },
    });
    let resp = ureq::post(&format!("{OLLAMA}/api/chat"))
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .send_string(&cuerpo.to_string())
        .map_err(|e| format!("Ollama no respondió: {e}"))?;
    let texto = resp.into_string().map_err(|e| format!("respuesta ilegible: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&texto).map_err(|e| format!("JSON inválido: {e}"))?;
    if let Some(e) = json.get("error").and_then(|e| e.as_str()) {
        return Err(format!("Ollama: {e}"));
    }
    let salida = json
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    let (ent, sal) = crate::chat::tokens_ollama(&json);
    Ok((parse(salida), ent, sal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elige_el_mas_pequeno_no_el_mejor() {
        // Al revés que en los cristales. Esto es leer ocho líneas y escribir
        // cuatro frases: con el modelo grande cada refresco ocupa la máquina
        // segundos para adornar una pantalla que se abandona al escribir.
        let m = elige(&[
            "llama3.1:8b".into(),
            "qwen3:0.6b".into(),
            "deepseek-r1:7b".into(),
        ]);
        assert_eq!(m.as_deref(), Some("qwen3:0.6b"));
    }

    #[test]
    fn los_que_no_dicen_su_tamano_van_al_final() {
        // `mistral:latest` puede ser enorme. En la duda, primero los que sabemos
        // que son pequeños.
        let m = elige(&["mistral:latest".into(), "qwen3:1.7b".into()]);
        assert_eq!(m.as_deref(), Some("qwen3:1.7b"));
        // Pero si es lo único que hay, sirve.
        assert_eq!(elige(&["mistral:latest".into()]).as_deref(), Some("mistral:latest"));
    }

    #[test]
    fn el_embebedor_no_escribe_atajos() {
        // `nomic-embed-text` no tiene `/api/chat`, y es de los más pequeños que
        // suele haber instalados: sin excluirlo saldría elegido casi siempre.
        assert_eq!(elige(&["nomic-embed-text".into()]), None);
        assert_eq!(
            elige(&["nomic-embed-text".into(), "qwen3:4b".into()]).as_deref(),
            Some("qwen3:4b")
        );
    }

    #[test]
    fn saca_los_atajos_de_una_salida_normal() {
        let s = "ATAJO: Spooler caído | Mira por qué está parado el servicio de impresión \
                 y dime si se puede arrancar.\n\
                 ATAJO: Disco C al 93% | Averigua qué está ocupando el disco C y propón qué \
                 se puede liberar sin riesgo.";
        let v = parse(s);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].etiqueta, "Spooler caído");
        assert!(v[0].orden.starts_with("Mira por qué"));
    }

    #[test]
    fn aguanta_que_el_modelo_lo_adorne() {
        // Viñetas, asteriscos y comillas: lo que hace cualquier modelo pequeño
        // al que se le pide un formato.
        let s = "Aquí tienes los atajos:\n\
                 - **ATAJO:** «Servicios caídos» | Revisa los servicios automáticos que \
                 están parados y dime cuáles importan.\n\
                 Espero que te sirvan.";
        let v = parse(s);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].etiqueta, "Servicios caídos");
    }

    #[test]
    fn el_bloque_de_pensamiento_no_se_cuela() {
        // `qwen3` abre `<think>` siempre, y dentro hay frases con dos puntos y
        // barras que parecen atajos.
        let s = "<think>Vale, el usuario quiere atajos. ATAJO: podría ser | algo así\
                 </think>\nATAJO: RAM al 90% | Dime qué proceso se está comiendo la memoria.";
        let v = parse(s);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].etiqueta, "RAM al 90%");
    }

    #[test]
    fn un_comando_no_pasa_por_orden() {
        // Si el modelo ignora la regla, ese atajo mandaría a Lucy un comando sin
        // revisar, saltándose el paso en el que el operador lo aprueba.
        let s = "ATAJO: Servicios | Get-Service | Where-Object {$_.Status -eq 'Stopped'}\n\
                 ATAJO: Ver logs | ```powershell\nGet-EventLog System\n```";
        assert!(parse(s).is_empty(), "un comando se ha colado como orden");
    }

    #[test]
    fn lo_que_no_encaja_se_tira_sin_llevarse_lo_demas() {
        let s = "ATAJO: sin barra y sin nada\n\
                 ATAJO: Corta | corta\n\
                 ATAJO: Una etiqueta francamente demasiado larga para un chip | Esta orden \
                 sí tiene longitud suficiente para valer.\n\
                 ATAJO: Buena | Revisa el estado del cortafuegos y dime si hay alguna regla \
                 que sobre.";
        let v = parse(s);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].etiqueta, "Buena");
    }

    #[test]
    fn no_repite_la_misma_etiqueta() {
        // Los modelos pequeños se enrocan y devuelven la misma cuatro veces con
        // distinta redacción.
        let s = "ATAJO: Disco | Averigua qué ocupa el disco y propón qué liberar.\n\
                 ATAJO: disco | Mira el espacio libre del disco y dime si preocupa.";
        assert_eq!(parse(s).len(), 1);
    }

    #[test]
    fn nunca_devuelve_mas_de_los_que_caben() {
        let mut s = String::new();
        for i in 0..9 {
            s.push_str(&format!(
                "ATAJO: Cosa {i} | Revisa la cosa número {i} de este equipo y cuéntame.\n"
            ));
        }
        assert_eq!(parse(&s).len(), MAX);
    }

    #[test]
    fn con_privacidad_los_atajos_no_salen_del_equipo() {
        // El contexto que viaja son los servicios caídos y los últimos errores
        // del log. Es exactamente lo que ese modo promete que no sale de aquí, y
        // que sea para adornar una pantalla no lo hace menos cierto.
        assert_eq!(elige_fuente(&[], true), None);
    }

    #[test]
    fn manda_el_local_aunque_haya_nube() {
        match elige_fuente(&["qwen3:0.6b".into()], false) {
            Some(Fuente::Local(m)) => assert_eq!(m, "qwen3:0.6b"),
            otro => panic!("tenía que elegir el local, salió {otro:?}"),
        }
    }

    #[test]
    fn no_haberlos_pedido_nunca_cuenta_como_vencido() {
        // Si `None` no contara, los atajos no se escribirían jamás en una
        // instalación nueva: se quedarían esperando a que venciera algo que no
        // ha empezado.
        assert!(vencido(None, 1_000));
        assert!(!vencido(Some(1_000), 1_000));
        assert!(!vencido(Some(1_000), 1_000 + CADA_SECS - 1));
        assert!(vencido(Some(1_000), 1_000 + CADA_SECS));
        // Y un reloj que va hacia atrás: sin esto, cambiar la hora del equipo
        // congelaría los atajos hasta que el reloj alcanzara la marca otra vez.
        assert!(vencido(Some(9_000), 1_000));
    }

    #[test]
    fn el_plazo_es_de_medio_dia_y_no_de_minutos() {
        // Un equipo no cambia en veinte minutos: si el disco se llena hoy,
        // mañana sigue lleno. Refrescar a menudo daría los mismos atajos
        // escritos de otra manera, y con el respaldo de nube sería un goteo de
        // céntimos por reescribir lo que ya estaba bien.
        assert!(CADA_SECS >= 6 * 3_600, "un plazo corto convierte esto en un gasto");
    }

    #[test]
    fn el_contexto_dice_lo_que_esta_mal_y_cabe_en_un_parrafo() {
        let s = crate::system::SysSnapshot {
            host: "SRV-04".into(),
            os: "Windows".into(),
            kernel: String::new(),
            cpu_brand: String::new(),
            cpu_pct: 12.0,
            per_core: Vec::new(),
            mem_used: 6_000_000_000,
            mem_total: 8_000_000_000,
            swap_used: 0,
            swap_total: 0,
            uptime_secs: 0,
            cores: 8,
            disks: Vec::new(),
        };
        let caidos = vec![crate::system::DownService { name: "Spooler".into(), exit_code: 0 }];
        let t = contexto(&s, &caidos, &["error de red".into()]);
        assert!(t.contains("SRV-04"));
        assert!(t.contains("CAÍDOS (1): Spooler"), "{t}");
        assert!(t.contains("75% ocupada"), "{t}");
        // Que quepa: un 0.6B con un informe delante deja de mirar los datos y se
        // pone a repetir el formato del ejemplo.
        assert!(t.chars().count() < 900, "el contexto son {} caracteres", t.chars().count());
    }
}
