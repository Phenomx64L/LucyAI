//! La capa 3: convertir un aviso medido en una frase que se lea bien.
//!
//! ES LA ÚNICA CAPA DONDE ENTRA EL MODELO, y entra con las manos atadas. No
//! averigua nada: recibe una estructura donde las cifras ya están medidas y la
//! decisión ya está tomada, y su trabajo entero es decirlo mejor.
//!
//! ```text
//!   entra   «Disco C: casi lleno» · «C:\ al 94 % — quedan 12.3 GB de 500.0»
//!   sale    «Al disco C: le quedan 12.3 GB de 500: está al 94 % y subiendo.»
//! ```
//!
//! ── POR QUÉ ESTO NO PUEDE ALUCINAR UNA CIFRA ────────────────────────────────
//!
//! No por el prompt —a un modelo de 600 millones de parámetros se le pide
//! cualquier cosa y hace lo que puede— sino porque la salida SE COMPRUEBA. Todo
//! número que aparezca en la frase tiene que salir de la entrada: si aparece uno
//! que no estaba, la redacción entera se tira y sale la plantilla.
//!
//! Eso convierte la promesa de «sin alucinaciones» en una propiedad verificable
//! en vez de una esperanza depositada en un modelo pequeño. Se pierde fluidez en
//! el caso raro; no se pierde corrección en ninguno.
//!
//! Y SE ADMITE EL REDONDEO, porque rechazarlo sería tirar la mitad de las
//! redacciones buenas: «12 GB» a partir de «12.3 GB» es exactamente lo que uno
//! quiere que escriba. Lo que no se admite es un número que no se parezca a
//! ninguno de los de entrada.
//!
//! ── Y SI NO HAY MODELO, NO PASA NADA ────────────────────────────────────────
//!
//! Ollama apagado, sin modelo instalado, o tardando más de la cuenta: sale la
//! plantilla. Un vigilante que deja de avisar porque el redactor no contesta
//! sería un vigilante roto por su parte decorativa.

/// Lo que se le da al modelo. Todo medido, nada por averiguar.
#[derive(Debug, Clone, Default)]
pub struct Material {
    pub titulo: String,
    pub cuerpo: String,
    /// Por qué se está avisando: «es nuevo», «ha empeorado», «se ha arreglado».
    /// Le da al modelo el tiempo verbal correcto sin tener que deducirlo.
    pub motivo: String,
    /// Vacío = este equipo.
    pub equipo: String,
}

/// Cuánto se le deja escribir. Dos frases con holgura.
///
/// Una notificación de escritorio se corta en pantalla a las pocas líneas, así
/// que pedir más es pagar tiempo de CPU por texto que nadie va a ver. Ciento
/// cincuenta es lo medido: con ciento veinte, un modelo que se enrolla un poco
/// se queda a mitad de la segunda etiqueta y la respuesta entera se descarta.
const NUM_PREDICT: i32 = 150;

/// Cuánto se espera al modelo antes de rendirse y usar la plantilla.
///
/// Ocho segundos. Un aviso que llega ocho segundos tarde sigue sirviendo; uno
/// que llega un minuto tarde ya no habla del presente. Y con un modelo cargando
/// en frío se pasa de aquí sin problema, que es justo cuando conviene rendirse.
const TIMEOUT_SECS: u64 = 8;

const OLLAMA: &str = "http://127.0.0.1:11434";

/// EL EJEMPLO VA RELLENO, Y ES LO QUE HACE QUE ESTA CAPA FUNCIONE.
///
/// Aquí el formato se enseñaba con huecos —`<T>título corto</T>`— y un modelo de
/// seiscientos millones de parámetros hace lo más literal que puede: devolvía
///
/// ```text
///   <T>una o dos frases</T>
///   C: casi lleno, con 12.3 GB de espacio restante.
/// ```
///
/// copiando el hueco DENTRO de la etiqueta y dejando el texto de verdad fuera.
/// Con el ejemplo relleno, ese mismo modelo acierta en cien milisegundos.
///
/// Y SUS CIFRAS SON TODAS DADAS, a propósito. La primera versión del ejemplo
/// decía «quedan 2.9 GB libres de 32» a partir de «29.1 de 32.0», o sea le
/// enseñaba a DERIVAR un número — que es exactamente lo que
/// `solo_usa_cifras_dadas` rechaza. Un ejemplo que enseña lo que el verificador
/// tira es un ejemplo que garantiza el rechazo.
///
/// TRES EJEMPLOS Y NO UNO, UNO POR CADA FORMA DE MENSAJE. Con un solo ejemplo,
/// el modelo lo CALCA sobre datos donde no aplica, y sale algo cuyas cifras
/// pasan la comprobación y cuyo significado está invertido:
///
/// ```text
///   medido     C:\ al 94 % — quedan 12.3 GB de 500.0     (12.3 es lo LIBRE)
///   redactado  «Usa 12.3 GB de 500.0 GB»                 (dice que es lo USADO)
/// ```
///
/// Los números eran correctos y el verificador lo dejó pasar: comprueba cifras,
/// no semántica. La única defensa contra eso resultó ser enseñarle las tres
/// formas —usado-de-total, libre-de-total, y la recuperación— y prohibirle
/// explícitamente afirmar duraciones y usar la situación como título. Con eso
/// acierta las tres en unos ciento cincuenta milisegundos.
const SISTEMA: &str = "Reescribes avisos de un sistema Windows para que se lean de un \
     vistazo en una notificación de escritorio.\n\
     REGLAS: no inventes números — usa solo los que te doy, y puedes redondearlos. No \
     añadas causas ni consejos: no sabes qué lo ha provocado. No afirmes cuánto tiempo \
     lleva pasando algo si no te lo he dicho. El título describe el PROBLEMA, nunca la \
     situación que te doy. Dos frases como mucho, en español llano y sin exclamaciones.\n\n\
     EJEMPLO 1\n\
     Entrada: Título: Memoria alta / Detalle: La memoria va al 91 % — 29.1 de 32.0 GB.\n\
     Salida:\n\
     <T>La memoria está al 91 %</T>\n\
     <C>Se usan 29.1 de 32.0 GB.</C>\n\n\
     EJEMPLO 2\n\
     Entrada: Título: Disco D: casi lleno / Detalle: D: al 88 % — quedan 60.0 GB de 500.0.\n\
     Salida:\n\
     <T>Al disco D: le quedan 60 GB</T>\n\
     <C>Está al 88 % de 500.0 GB.</C>\n\n\
     EJEMPLO 3\n\
     Entrada: Título: Resuelto: Servicios automáticos / Detalle: Ya no hay ningún servicio \
     automático fallado.\n\
     Salida:\n\
     <T>Servicios automáticos, resuelto</T>\n\
     <C>Ya no queda ninguno fallado.</C>";

/// Si la redacción está encendida. APAGADA de fábrica.
///
/// Y APAGADA POR LO MEDIDO, no por prudencia genérica. Con `qwen3:0.6b`, que es
/// lo que elige el vigilante en esta máquina, la redacción sale correcta —las
/// tres formas bien, unos dos segundos— y aun así NO MEJORA LA PLANTILLA:
///
/// ```text
///   plantilla  C:\ al 94 % — quedan 12.3 GB de 500.0
///   modelo     Está al 94 % de 500.0 GB
/// ```
///
/// El modelo se dejó por el camino los 12,3 GB libres, que son EL número
/// accionable: es la diferencia entre «tengo que hacer algo hoy» y «ya veré».
/// Y en otro caso escribió «Se han fallado MSSQLSERVER y SQLSERVERAGENT», que no
/// es español.
///
/// La maquinaria está entera y verificada: lo que falta para que encenderla
/// merezca la pena no es código, es que el vigilante tenga cosas MÁS DIFÍCILES
/// que decir —tendencias, varios equipos, dos síntomas a la vez— donde una
/// plantilla no llega. Con material de una línea, una plantilla escrita por una
/// persona gana a un modelo de seiscientos millones de parámetros, y decirlo es
/// más útil que fingir lo contrario.
static ACTIVA: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn activa() -> bool {
    ACTIVA.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn pon_activa(v: bool) {
    ACTIVA.store(v, std::sync::atomic::Ordering::Relaxed);
}

/// La frase redactada, o `None` si no se puede confiar en ella.
///
/// BLOQUEANTE: quien llama ya está en un hilo — el vigilante lo está.
pub fn redacta(m: &Material) -> Option<(String, String)> {
    if !activa() {
        return None;
    }
    let modelo = crate::suggest::elige(&crate::chat::list_models())?;
    let salida = pregunta(m, &modelo).ok()?;
    let (t, c) = parse(&salida)?;
    // LA COMPROBACIÓN. Todo lo demás de este módulo es fontanería; esto es lo
    // que hace que se pueda encender sin miedo.
    let dado = format!("{} {}", m.titulo, m.cuerpo);
    if !solo_usa_cifras_dadas(&format!("{t} {c}"), &dado) {
        return None;
    }
    Some((t, c))
}

/// Los números de un texto, tal como aparecen.
///
/// Se queda con la parte numérica y normaliza la coma decimal: un modelo que
/// escribe en español puede devolver «12,3» donde la entrada decía «12.3», y
/// rechazar la redacción por eso sería castigar al modelo por escribir bien.
pub fn cifras(s: &str) -> Vec<f64> {
    let mut v = Vec::new();
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let ini = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        // Un separador decimal solo cuenta si le sigue un dígito: «12.» al final
        // de una frase es un punto, no un decimal.
        if i + 1 < b.len() && (b[i] == '.' || b[i] == ',') && b[i + 1].is_ascii_digit() {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        let txt: String = b[ini..i].iter().map(|c| if *c == ',' { '.' } else { *c }).collect();
        if let Ok(n) = txt.parse::<f64>() {
            v.push(n);
        }
    }
    v
}

/// Si toda cifra de `salida` sale de `entrada`.
///
/// EL REDONDEO SE ADMITE y el invento no. «12» a partir de «12.3» pasa —es lo
/// que uno quiere que escriba— y «7» sin ningún 7 detrás, no. Un número que no
/// se parece a ninguno de los dados es exactamente la clase de cosa que un
/// modelo pequeño se saca de la manga: «quedan 7 días», «hay 3 procesos».
pub fn solo_usa_cifras_dadas(salida: &str, entrada: &str) -> bool {
    let dadas = cifras(entrada);
    cifras(salida).iter().all(|n| {
        dadas.iter().any(|d| {
            (n - d).abs() < 1e-9
                || (*n - d.round()).abs() < 1e-9
                || (*n - d.trunc()).abs() < 1e-9
                || (*n - d.ceil()).abs() < 1e-9
        })
    })
}

/// Saca el título y el cuerpo de la respuesta.
///
/// XML Y NO JSON, por lo mismo que los cristales: un modelo pequeño se deja el
/// paréntesis o mete una coma de más y el JSON entero se pierde, mientras que
/// aquí una etiqueta mal cerrada solo pierde esa.
pub fn parse(s: &str) -> Option<(String, String)> {
    let t = entre(s, "<T>", "</T>")?;
    let c = entre(s, "<C>", "</C>")?;
    let (t, c) = (limpia(&t), limpia(&c));
    // Una redacción vacía o de una palabra no mejora la plantilla: mejor la
    // plantilla, que al menos lleva las cifras.
    if t.chars().count() < 3 || c.chars().count() < 10 {
        return None;
    }
    // Y una que se desmadra tampoco: la notificación la corta igual, y un texto
    // largo suele significar que el modelo se ha puesto a explicar causas que no
    // sabe.
    if t.chars().count() > 80 || c.chars().count() > 240 {
        return None;
    }
    Some((t, c))
}

fn entre(s: &str, a: &str, b: &str) -> Option<String> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(s[i..j].to_string())
}

fn limpia(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn pregunta(m: &Material, modelo: &str) -> Result<String, String> {
    let donde = if m.equipo.is_empty() { "este equipo".to_string() } else { m.equipo.clone() };
    let usuario = format!(
        "Equipo: {donde}\nSituación: {}\nTítulo medido: {}\nDetalle medido: {}",
        m.motivo, m.titulo, m.cuerpo
    );
    let cuerpo = serde_json::json!({
        "model": modelo,
        "messages": [
            { "role": "system", "content": SISTEMA },
            { "role": "user", "content": usuario },
        ],
        "stream": false,
        // SIN RAZONAR, y sin esto la función no existe. Los modelos pequeños de
        // hoy —`qwen3:0.6b`, `deepseek-r1:1.5b`— son de razonamiento: se gastan
        // el presupuesto entero en su bloque de pensamiento y devuelven el
        // contenido VACÍO, con `done_reason: length`. Medido: 120 tokens
        // consumidos, cero caracteres de salida, cuatro segundos tirados.
        //
        // Con `think` en falso, ese mismo modelo contesta bien en CIEN
        // MILISEGUNDOS. Es la diferencia entre que esta capa se pueda encender
        // en un portátil y que no.
        //
        // Un Ollama viejo que no conozca el campo lo ignora, y entonces se cae a
        // la plantilla como con cualquier otro fallo.
        "think": false,
        // Frío a propósito: aquí no se quiere creatividad, se quiere que repita
        // lo que se le ha dado con mejores palabras.
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
    if let Some(e) = json.get("error").and_then(|e| e.as_str()) {
        return Err(format!("Ollama: {e}"));
    }
    Ok(json
        .get("message")
        .and_then(|x| x.get("content"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_leen_las_cifras_de_un_aviso_de_verdad() {
        let v = cifras("C:\\ al 94 % — quedan 12.3 GB de 500.0.");
        assert_eq!(v, vec![94.0, 12.3, 500.0]);
    }

    #[test]
    fn la_coma_decimal_no_rompe_la_comprobacion() {
        // Un modelo que escribe en español devuelve «12,3» donde la entrada
        // decía «12.3». Rechazar la redacción por eso sería castigarle por
        // escribir bien.
        assert!(solo_usa_cifras_dadas("quedan 12,3 GB", "quedan 12.3 GB de 500.0"));
    }

    #[test]
    fn un_punto_final_no_es_un_decimal() {
        // «al 94.» al final de una frase son noventa y cuatro y un punto, no
        // noventa y cuatro coma nada.
        assert_eq!(cifras("va al 94. Y sigue"), vec![94.0]);
    }

    #[test]
    fn se_admite_redondear_hacia_donde_sea() {
        // Es lo que uno QUIERE que escriba: «12 GB» a partir de «12.3 GB».
        let dado = "C:\\ al 94.2 % — quedan 12.7 GB de 500.0";
        assert!(solo_usa_cifras_dadas("al 94 %, quedan 12 GB", dado));
        assert!(solo_usa_cifras_dadas("al 94 %, quedan 13 GB", dado));
    }

    #[test]
    fn una_cifra_que_no_estaba_tira_la_redaccion_entera() {
        // ES LA REGLA QUE HACE QUE ESTO SE PUEDA ENCENDER SIN MIEDO. «Quedan 7
        // días» es exactamente lo que un modelo pequeño se saca de la manga
        // cuando le pides que escriba bonito sobre un disco lleno.
        let dado = "C:\\ al 94 % — quedan 12.3 GB de 500.0";
        assert!(!solo_usa_cifras_dadas("al 94 %: se llena en 7 días", dado));
        assert!(!solo_usa_cifras_dadas("hay 3 discos afectados", dado));
    }

    #[test]
    fn un_aviso_sin_cifras_no_bloquea_la_redaccion() {
        // «Ya no hay ningún servicio automático fallado» no lleva números, y una
        // redacción suya tampoco debería llevarlos — pero si no los lleva
        // ninguno, la comprobación tiene que pasar en vez de rechazar por
        // vacío.
        assert!(solo_usa_cifras_dadas("Todo en orden.", "Ya no hay ningún servicio fallado."));
        // Y si el modelo se inventa uno, se rechaza igual.
        assert!(!solo_usa_cifras_dadas("Hay 2 pendientes.", "Ya no hay ningún servicio fallado."));
    }

    #[test]
    fn se_leen_las_dos_etiquetas_y_se_limpian() {
        let s = "<T>  Disco C: casi lleno </T>\n<C>Quedan 12 GB\n  de 500.</C>";
        let (t, c) = parse(s).expect("no parseó");
        assert_eq!(t, "Disco C: casi lleno");
        assert_eq!(c, "Quedan 12 GB de 500.");
    }

    #[test]
    fn una_etiqueta_perdida_no_se_lleva_la_otra_por_delante() {
        // La razón de usar XML y no JSON con un modelo pequeño: aquí se pierde
        // lo que falta, no la respuesta entera. Sin las dos no hay redacción,
        // pero el fallo es local y legible.
        assert!(parse("<T>solo el título</T>").is_none());
        assert!(parse("nada de nada").is_none());
    }

    #[test]
    fn una_redaccion_vacia_o_desmadrada_no_mejora_la_plantilla() {
        assert!(parse("<T>ok</T><C>corto</C>").is_none(), "aceptó una redacción de una palabra");
        let largo = "x".repeat(300);
        assert!(
            parse(&format!("<T>bien</T><C>{largo}</C>")).is_none(),
            "aceptó un párrafo que la notificación va a cortar igual"
        );
    }

    #[test]
    fn viene_apagada_de_fabrica() {
        // No por prudencia genérica: por lo medido. Con el modelo que elige el
        // vigilante, la redacción sale correcta y aun así pierde el número
        // accionable —los GB libres— que la plantilla sí lleva. Encenderla es
        // una decisión que toma el operador cuando el vigilante tenga cosas más
        // difíciles que decir.
        //
        // Este test corre en el mismo proceso que los demás, así que se
        // restaura el estado: otro test que dependa del valor por defecto no
        // puede quedar a merced del orden.
        let antes = activa();
        pon_activa(false);
        assert!(!activa());
        assert!(
            redacta(&Material::default()).is_none(),
            "apagada, no debería ni preguntar al modelo"
        );
        pon_activa(antes);
    }

    #[test]
    fn el_prompt_le_prohibe_lo_que_no_puede_saber() {
        // Las tres cosas que un modelo pequeño añade por su cuenta si no se le
        // dice que no: números, causas y consejos.
        assert!(SISTEMA.contains("no inventes números"));
        assert!(SISTEMA.contains("No añadas"));
        assert!(SISTEMA.contains("<T>"), "el formato de salida no está en el prompt");
    }

    #[test]
    fn el_ejemplo_del_prompt_va_relleno_y_no_con_huecos() {
        // AQUÍ HABÍA UN FALLO QUE ANULABA LA FUNCIÓN ENTERA. El formato se
        // enseñaba como `<T>título corto</T>`, y un modelo de seiscientos
        // millones de parámetros hace lo más literal posible: devolvía
        //
        //     <T>una o dos frases</T>
        //     C: casi lleno, con 12.3 GB de espacio restante.
        //
        // …copiando el hueco dentro de la etiqueta y dejando el texto de verdad
        // fuera. Con un ejemplo RELLENO, el mismo modelo acierta en cien
        // milisegundos.
        assert!(!SISTEMA.contains("título corto"), "el ejemplo volvió a ser un hueco");
        assert!(!SISTEMA.contains("una o dos frases"), "el ejemplo volvió a ser un hueco");
        assert!(SISTEMA.contains("EJEMPLO"));
    }

    #[test]
    fn el_ejemplo_del_prompt_pasaria_su_propia_comprobacion() {
        // LA PRIMERA VERSIÓN NO LA PASABA, y eso garantizaba el rechazo. Decía
        // «quedan 2.9 GB libres de 32» a partir de «29.1 de 32.0», o sea le
        // enseñaba al modelo a DERIVAR un número — justo lo que
        // `solo_usa_cifras_dadas` tira. Un ejemplo que enseña lo que el
        // verificador rechaza es un ejemplo que asegura que nunca se acepte
        // nada.
        let ej = SISTEMA.split("EJEMPLO").nth(1).expect("el prompt perdió su ejemplo");
        let entrada = ej.split("Salida:").next().unwrap_or("");
        let salida = ej.split("Salida:").nth(1).unwrap_or("");
        assert!(
            solo_usa_cifras_dadas(salida, entrada),
            "el ejemplo del prompt le enseña al modelo a escribir cifras que luego se rechazan"
        );
    }
}
