//! Las etiquetas de acción que Lucy emite en su respuesta.
//!
//! Port de `extractTags` y `parseTool` (`src/lib/llm-stream.ts`). Es la pieza
//! que convierte un texto en una lista de cosas que Lucy quiere hacer, y por eso
//! es la primera del bucle del agente que se puede migrar sola: no ejecuta nada,
//! solo lee.
//!
//! AQUÍ NO SE EJECUTA NADA, Y ES A PROPÓSITO. Detectar que el modelo pide correr
//! un comando y correrlo son dos cosas distintas, y la segunda vive en
//! `src-tauri` junto a los guardrails que revisan qué se va a ejecutar. Separar
//! la detección deja que el shell nativo ENSEÑE lo que Lucy propone sin poder
//! dispararlo — que es exactamente lo que hace falta mientras el resto del bucle
//! sigue al otro lado.
//!
//! UNA DIVERGENCIA QUE SE CONSERVA: el bucle del agente tiene su PROPIA
//! detección de etiquetas de ejecución, distinta de la que corre tras el stream
//! —tolera cierres truncados y no cae al respaldo de bloques de código—. No se
//! unifican. Este módulo porta la de después del stream; si algún día se trae la
//! otra, va aparte y con su nombre.
//!
//! SIN MOTOR DE EXPRESIONES, y no por gusto. La primera versión usaba `regex`, y
//! añadir esa caja reenlazó el binario de tests con un hash nuevo que Smart App
//! Control bloqueó en la máquina de desarrollo: los tests dejaron de poder
//! ejecutarse. Un escáner escrito a mano para seis etiquetas es menos código del
//! que parece, y a cambio ninguna dependencia nueva vuelve a poder opinar sobre
//! si el proyecto se puede verificar.

use std::collections::HashMap;

/// Qué clase de etiqueta es.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagKind {
    Execute,
    ExecuteCmd,
    ExecuteWmic,
    ExecuteNetsh,
    ExecuteReg,
    ExecuteCscript,
    ExecuteRemote,
    Tool,
    Thought,
    Learn,
    Remember,
    FileContent,
}

impl TagKind {
    /// El nombre de la etiqueta tal cual aparece en el texto.
    pub fn name(self) -> &'static str {
        match self {
            Self::Execute => "EXECUTE",
            Self::ExecuteCmd => "EXECUTE_CMD",
            Self::ExecuteWmic => "EXECUTE_WMIC",
            Self::ExecuteNetsh => "EXECUTE_NETSH",
            Self::ExecuteReg => "EXECUTE_REG",
            Self::ExecuteCscript => "EXECUTE_CSCRIPT",
            Self::ExecuteRemote => "EXECUTE_REMOTE",
            Self::Tool => "TOOL",
            Self::Thought => "THOUGHT",
            Self::Learn => "LEARN",
            Self::Remember => "REMEMBER",
            Self::FileContent => "FILECONTENT",
        }
    }

    /// ¿Es una petición de ejecutar algo? Lo que separa "Lucy está pensando" de
    /// "Lucy quiere tocar la máquina".
    pub fn is_execute(self) -> bool {
        matches!(
            self,
            Self::Execute
                | Self::ExecuteCmd
                | Self::ExecuteWmic
                | Self::ExecuteNetsh
                | Self::ExecuteReg
                | Self::ExecuteCscript
                | Self::ExecuteRemote
        )
    }
}

#[derive(Debug, Clone)]
pub struct Tag {
    pub kind: TagKind,
    pub content: String,
    pub attrs: HashMap<String, String>,
}

/// Las seis variantes de `<EXECUTE…>`, en el mismo orden que la alternancia del
/// original.
const EXEC_KINDS: [TagKind; 6] = [
    TagKind::Execute,
    TagKind::ExecuteCmd,
    TagKind::ExecuteWmic,
    TagKind::ExecuteNetsh,
    TagKind::ExecuteReg,
    TagKind::ExecuteCscript,
];

/// Un par `<NAME …>cuerpo</NAME>` encontrado en el texto.
struct Found<'a> {
    /// Dónde empieza `<`. Sirve para reordenar por posición.
    start: usize,
    /// Lo que había entre el nombre y el `>`.
    attrs: &'a str,
    body: &'a str,
}

/// Busca todos los pares de una etiqueta, sin distinguir mayúsculas.
///
/// `low` es una copia del texto en minúsculas ASCII, y tiene que ser ASCII: una
/// minusculización Unicode puede CAMBIAR la longitud en bytes de algunas letras,
/// y entonces las posiciones que se encuentran en la copia ya no valen para
/// cortar el original. `to_ascii_lowercase` solo toca la A-Z, así que los dos
/// textos miden lo mismo byte a byte y los índices son intercambiables.
fn scan<'a>(hay: &'a str, low: &str, name_lower: &str) -> Vec<Found<'a>> {
    let open = format!("<{name_lower}");
    let close = format!("</{name_lower}>");
    let mut out = Vec::new();
    let mut i = 0;

    while let Some(rel) = low[i..].find(&open) {
        let start = i + rel;
        let after = start + open.len();
        // El carácter que sigue al nombre decide si es ESTA etiqueta u otra que
        // empieza igual: sin esta comprobación, `<EXECUTE` encontraría también
        // `<EXECUTE_CMD` y se quedaría esperando un `</EXECUTE>` que no existe,
        // tragándose el resto del texto.
        let next = low.as_bytes().get(after).copied();
        let ok = matches!(next, Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r'));
        if !ok {
            i = start + 1;
            continue;
        }
        let Some(gt_rel) = low[after..].find('>') else { break };
        let gt = after + gt_rel;
        let body_start = gt + 1;
        let Some(close_rel) = low[body_start..].find(&close) else {
            // Cierre ausente: es una etiqueta a medio llegar. Aquí NO se
            // detecta, y eso es correcto — la tolerancia a cierres truncados es
            // de la otra detección, la del bucle, que es otra cosa a propósito.
            break;
        };
        let body_end = body_start + close_rel;
        out.push(Found {
            start,
            attrs: &hay[after..gt],
            body: &hay[body_start..body_end],
        });
        i = body_end + close.len();
    }
    out
}

/// Saca `clave="valor"` (o con comillas simples) de la parte de atributos.
fn attr<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let low = attrs.to_ascii_lowercase();
    let at = low.find(&format!("{key}="))? + key.len() + 1;
    let rest = &attrs[at..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let val = &rest[1..];
    let end = val.find(quote)?;
    Some(&val[..end])
}

/// Extrae todas las etiquetas de acción de un texto.
///
/// EL ORDEN NO ES EL DEL DOCUMENTO, y eso viene del original: primero todas las
/// de ejecución, luego las remotas, luego TOOL, THOUGHT, LEARN, FILECONTENT y
/// REMEMBER. Dentro del bloque de ejecución sí van en el orden en que aparecen,
/// que en el original salía gratis de una sola pasada con retro-referencia; aquí
/// se recorre variante por variante y se reordena por posición. El resultado es
/// el mismo, y el matiz queda escrito para que nadie lo "arregle".
pub fn extract_tags(text: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    if text.is_empty() {
        return tags;
    }
    let low = text.to_ascii_lowercase();

    // ── EXECUTE y sus variantes ──────────────────────────────────────────────
    let mut found: Vec<(usize, Tag)> = Vec::new();
    for kind in EXEC_KINDS {
        for f in scan(text, &low, &kind.name().to_ascii_lowercase()) {
            // El original exige `<EXECUTE>` exacto: con atributos no casa.
            if !f.attrs.is_empty() {
                continue;
            }
            found.push((
                f.start,
                Tag {
                    kind,
                    content: f.body.trim().to_string(),
                    attrs: HashMap::new(),
                },
            ));
        }
    }
    found.sort_by_key(|(pos, _)| *pos);
    tags.extend(found.into_iter().map(|(_, t)| t));

    // ── EXECUTE_REMOTE, que lleva el equipo destino en un atributo ───────────
    for f in scan(text, &low, "execute_remote") {
        // Sin `target` no se acepta: el original lo exige en el patrón, y una
        // ejecución remota sin destino no se sabe contra qué equipo iría.
        let Some(t) = attr(f.attrs, "target") else { continue };
        let mut attrs = HashMap::new();
        attrs.insert("target".to_string(), t.to_string());
        tags.push(Tag {
            kind: TagKind::ExecuteRemote,
            content: f.body.trim().to_string(),
            attrs,
        });
    }

    // ── El resto, sin atributos ──────────────────────────────────────────────
    for (kind, trim) in [
        (TagKind::Tool, true),
        (TagKind::Thought, true),
        (TagKind::Learn, true),
        // FILECONTENT NO se recorta: es el contenido literal de un fichero que
        // se va a escribir, y quitarle los espacios de los extremos cambiaría
        // el fichero. El original tampoco lo recorta.
        (TagKind::FileContent, false),
    ] {
        for f in scan(text, &low, &kind.name().to_ascii_lowercase()) {
            if !f.attrs.is_empty() {
                continue;
            }
            tags.push(Tag {
                kind,
                content: if trim { f.body.trim().to_string() } else { f.body.to_string() },
                attrs: HashMap::new(),
            });
        }
    }

    // ── REMEMBER, con su categoría opcional ──────────────────────────────────
    for f in scan(text, &low, "remember") {
        let mut attrs = HashMap::new();
        if let Some(c) = attr(f.attrs, "category") {
            attrs.insert("category".to_string(), c.to_string());
        }
        tags.push(Tag {
            kind: TagKind::Remember,
            content: f.body.trim().to_string(),
            attrs,
        });
    }

    tags
}

/// Lo que se enseña de una respuesta, y lo que se saca aparte.
#[derive(Debug, Default, PartialEq)]
pub struct Cleaned {
    /// El texto sin ninguna etiqueta de acción, listo para el hilo.
    pub text: String,
    /// El razonamiento de los `<THOUGHT>`, en orden.
    pub thoughts: Vec<String>,
    /// Cuántos comandos se quitaron del texto.
    ///
    /// Existe porque quitarlos EN SILENCIO deja la prosa colgando: el modelo
    /// escribe "usa esta sintaxis:" y debajo no hay nada, y la respuesta parece
    /// cortada. Con la cuenta, quien dibuja puede decir a dónde fueron.
    pub commands: usize,
}

/// ¿El operador está pidiendo CÓDIGO en vez de una acción?
///
/// Port de `detectCodeGenIntent`. La diferencia importa: "reinicia el spooler"
/// quiere que algo pase, y el comando es fontanería; "dame un script para
/// reiniciar el spooler" quiere el comando EN LA RESPUESTA, y esconderlo
/// convierte la respuesta en un texto que señala a un hueco.
pub fn detect_code_gen_intent(text: &str) -> bool {
    let t = text.to_lowercase();
    // Las mismas familias que la expresión del original, en forma de pares
    // "verbo + objeto": pedir un script, o pedir código/powershell.
    const VERBS: [&str; 12] = [
        "dame", "escribe", "escriba", "crea", "genera", "hazme", "haz", "necesito", "quiero",
        "give me", "write", "create",
    ];
    for v in VERBS {
        let Some(i) = t.find(v) else { continue };
        let rest = &t[i + v.len()..];
        if rest.contains("script") || rest.contains("código") || rest.contains("codigo")
            || rest.contains("powershell") || rest.contains("comando")
        {
            return true;
        }
    }
    false
}

/// Todas las etiquetas que hay que quitar de lo que ve el operador.
const HIDDEN: [&str; 12] = [
    "execute_remote", // ANTES que `execute`: ver la nota de abajo.
    "execute_cmd",
    "execute_wmic",
    "execute_netsh",
    "execute_reg",
    "execute_cscript",
    "execute",
    "tool",
    "learn",
    "remember",
    "filecontent",
    "thought",
];

/// Deja la respuesta como se enseña en el hilo.
///
/// Port de `cleanStreamDisplay`. Sin esto, el operador ve el marcado crudo
/// —`<TOOL>readfile:…</TOOL>`— en mitad de la respuesta mientras Lucy trabaja,
/// que es exactamente lo que las etiquetas están para evitar.
///
/// TOLERA ETIQUETAS SIN CERRAR, y aquí sí es lo correcto: esto corre MIENTRAS
/// llega el stream, así que a mitad de una etiqueta el cierre todavía no ha
/// llegado. Sin esa tolerancia, el marcado a medio escribir aparece en pantalla
/// y desaparece al cerrarse — un parpadeo en cada herramienta que Lucy usa.
/// Nótese que `extract_tags` hace lo contrario a propósito: allí una etiqueta
/// sin cerrar no es una acción, es un trozo de texto.
///
/// El `<THOUGHT>` no se tira: se devuelve aparte. El original lo convierte en un
/// `<details>` de HTML, que un frontend nativo no puede renderizar — pero la
/// decisión de fondo es la misma y es la que importa: el razonamiento se
/// GUARDA y se enseña plegado, no se borra.
pub fn clean_display(text: &str) -> Cleaned {
    clean_display_with(text, false)
}

/// Igual, pero con la decisión de qué hacer con los comandos.
///
/// Con `code_gen`, un `<EXECUTE>` se convierte en un bloque de código en vez de
/// desaparecer — es lo que hace el original cuando el operador ha PEDIDO el
/// comando. Sin ese modo, una respuesta a "dame un script" sale sin el script.
pub fn clean_display_with(text: &str, code_gen: bool) -> Cleaned {
    let low = text.to_ascii_lowercase();
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    let mut thoughts: Vec<(usize, String)> = Vec::new();
    // `(inicio, reemplazo)` — los trozos que no se borran sino que se cambian.
    let mut swaps: Vec<(usize, String)> = Vec::new();
    let mut commands = 0usize;

    for name in HIDDEN {
        let open = format!("<{name}");
        let close = format!("</{name}>");
        let mut i = 0;
        while let Some(rel) = low[i..].find(&open) {
            let start = i + rel;
            let after = start + open.len();
            let next = low.as_bytes().get(after).copied();
            if !matches!(next, Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')) {
                i = start + 1;
                continue;
            }
            // Ya cubierto por otra etiqueta: `execute_remote` va primero en la
            // lista justo para esto. Sin la comprobación, `execute` volvería a
            // encontrar el `<execute_remote…` que ya se recortó y cortaría
            // desde ahí hasta el final del texto — el fallo que el comentario
            // del original avisa de no reintroducir.
            if cuts.iter().any(|(a, b)| start >= *a && start < *b) {
                i = start + 1;
                continue;
            }
            let body_start = low[after..].find('>').map_or(after, |g| after + g + 1);
            let (end, inner_end) = match low[body_start..].find(&close) {
                Some(c) => (body_start + c + close.len(), body_start + c),
                // Sin cierre: se recorta hasta el final. Es el caso de estar a
                // mitad del stream.
                None => (text.len(), text.len()),
            };
            if name == "thought" {
                thoughts.push((start, text[body_start..inner_end].trim().to_string()));
            }
            if name.starts_with("execute") {
                commands += 1;
                if code_gen {
                    let lang = if name == "execute_cmd" { "cmd" } else { "powershell" };
                    swaps.push((
                        start,
                        format!("\n```{lang}\n{}\n```\n", text[body_start..inner_end].trim()),
                    ));
                }
            }
            cuts.push((start, end));
            i = end;
        }
    }

    cuts.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for (a, b) in cuts {
        if a > pos {
            out.push_str(&text[pos..a]);
        }
        if let Some((_, rep)) = swaps.iter().find(|(s, _)| *s == a) {
            out.push_str(rep);
        }
        pos = pos.max(b);
    }
    if pos < text.len() {
        out.push_str(&text[pos..]);
    }

    thoughts.sort_by_key(|(p, _)| *p);
    Cleaned {
        text: out.trim().to_string(),
        thoughts: thoughts.into_iter().map(|(_, t)| t).filter(|t| !t.is_empty()).collect(),
        commands,
    }
}

/// Parte el contenido de un `<TOOL>` en nombre y argumentos.
///
/// Se corta por el PRIMER `:` y nada más. Los argumentos pueden llevar más —una
/// ruta de Windows empieza por `C:`— así que partir por todos rompería
/// `readfile:C:\logs\lucy.log` en el sitio equivocado.
pub fn parse_tool(content: &str) -> (String, String) {
    match content.find(':') {
        None => (content.trim().to_string(), String::new()),
        Some(i) => (content[..i].trim().to_string(), content[i + 1..].to_string()),
    }
}

/// ¿Hay alguna etiqueta accionable? La comprobación barata de la V2.
pub fn has_tool_response(resp: &str) -> bool {
    resp.contains("<TOOL>")
        || resp.contains("<EXECUTE")
        || resp.to_ascii_lowercase().contains("<thought>")
}

/// ¿Combina razonamiento y acción? Es lo que decide si un turno necesita varias
/// vueltas del bucle.
pub fn is_multi_step(resp: &str) -> bool {
    resp.to_ascii_lowercase().contains("<thought>")
        || (resp.contains("<TOOL>") && resp.contains("<EXECUTE"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn una_lista_en_markdown_sale_intacta() {
        // LA SOSPECHA. En una captura, la respuesta salia con el «2.» en una
        // linea y su texto en la siguiente.  recorta etiquetas
        // por RANGOS DE BYTES, asi que un recorte a mitad de una linea de lista
        // dejaria el marcador colgando — y eso se ve exactamente asi.
        //
        // Este test dice si la culpa es nuestra o del modelo.
        let md = "1. Primero, lo que pasa.
2. Segundo, lo que hay que mirar.
   - anidado
";
        // Se compara recortado:  quita el espacio del final, que
        // es correcto y no tiene nada que ver con la estructura de la lista.
        assert_eq!(clean_display(md).text, md.trim_end(), "la lista limpia se ha tocado");
    }

    #[test]
    fn una_etiqueta_dentro_de_una_lista_no_parte_el_marcador() {
        // El caso que si podria ser culpa nuestra: una herramienta invocada
        // dentro de un punto de la lista.
        let md = "1. Miro la RAM.
2. <TOOL>readfile:x</TOOL>Miro el disco.
";
        let out = clean_display(md).text;
        assert!(out.contains("2. Miro el disco."), "el marcador quedo colgando: {out:?}");
    }

    use super::*;

    #[test]
    fn saca_cada_familia_de_etiqueta() {
        let t = "<THOUGHT>miro el disco</THOUGHT>\
                 <EXECUTE>Get-PSDrive</EXECUTE>\
                 <TOOL>readfile:C:\\logs\\lucy.log</TOOL>";
        let tags = extract_tags(t);
        assert_eq!(tags.len(), 3);
        // El orden es el del ORIGINAL, no el del documento: ejecución primero.
        assert_eq!(tags[0].kind, TagKind::Execute);
        assert_eq!(tags[0].content, "Get-PSDrive");
        assert_eq!(tags[1].kind, TagKind::Tool);
        assert_eq!(tags[2].kind, TagKind::Thought);
    }

    #[test]
    fn las_variantes_de_ejecucion_salen_en_orden_de_aparicion() {
        let t = "<EXECUTE_CMD>dir</EXECUTE_CMD> texto <EXECUTE>ls</EXECUTE> \
                 <EXECUTE_REG>reg query HKLM</EXECUTE_REG>";
        let k: Vec<TagKind> = extract_tags(t).iter().map(|x| x.kind).collect();
        assert_eq!(
            k,
            vec![TagKind::ExecuteCmd, TagKind::Execute, TagKind::ExecuteReg]
        );
    }

    #[test]
    fn una_etiqueta_no_se_come_a_la_que_empieza_igual() {
        // EL fallo del escáner escrito a mano: buscar `<EXECUTE` encuentra
        // también `<EXECUTE_CMD`, y sin comprobar el carácter siguiente se
        // quedaría esperando un `</EXECUTE>` que no llega, tragándose el resto.
        let t = "<EXECUTE_CMD>dir</EXECUTE_CMD> y luego <EXECUTE>ls</EXECUTE>";
        let tags = extract_tags(t);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].content, "dir");
        assert_eq!(tags[1].content, "ls");
    }

    #[test]
    fn las_mayusculas_dan_igual_y_los_acentos_no_descolocan() {
        // El texto se busca sobre una copia en minúsculas: si esa copia midiera
        // distinto en bytes que el original, los cortes saldrían movidos. Por
        // eso la minusculización es ASCII — y esto lo comprueba con acentos
        // antes y dentro de la etiqueta.
        let t = "Revisión previa <execute>Get-Proceso -Ñame señal</Execute> fin";
        let tags = extract_tags(t);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].content, "Get-Proceso -Ñame señal");
    }

    #[test]
    fn el_destino_de_una_ejecucion_remota_viaja_con_ella() {
        let t = r#"<EXECUTE_REMOTE target="SRV-DC01">Get-Service</EXECUTE_REMOTE>"#;
        let tags = extract_tags(t);
        assert_eq!(tags[0].kind, TagKind::ExecuteRemote);
        assert_eq!(tags[0].attrs.get("target").unwrap(), "SRV-DC01");
        assert_eq!(tags[0].content, "Get-Service");

        // Sin destino NO se acepta: no se sabría contra qué equipo iría.
        assert!(extract_tags("<EXECUTE_REMOTE>Get-Service</EXECUTE_REMOTE>").is_empty());
        // Y con comillas simples sí, como el original.
        let t2 = "<EXECUTE_REMOTE target='nas'>uptime</EXECUTE_REMOTE>";
        assert_eq!(extract_tags(t2)[0].attrs.get("target").unwrap(), "nas");
    }

    #[test]
    fn remember_lleva_su_categoria_cuando_la_trae() {
        let t = r#"<REMEMBER category="red">el proxy corta en el 8080</REMEMBER>"#;
        let tags = extract_tags(t);
        assert_eq!(tags[0].kind, TagKind::Remember);
        assert_eq!(tags[0].attrs.get("category").unwrap(), "red");
        // Y sin atributos también vale — el original los hace opcionales.
        let t2 = "<REMEMBER>algo suelto</REMEMBER>";
        assert_eq!(extract_tags(t2)[0].content, "algo suelto");
        assert!(extract_tags(t2)[0].attrs.is_empty());
    }

    #[test]
    fn el_contenido_de_un_fichero_no_se_recorta() {
        // Quitarle los espacios de los extremos CAMBIA el fichero que se va a
        // escribir. Es la única etiqueta que no se recorta, y no por descuido.
        let tags = extract_tags("<FILECONTENT>\n  sangrado\n</FILECONTENT>");
        assert_eq!(tags[0].content, "\n  sangrado\n");
    }

    #[test]
    fn una_ruta_de_windows_no_parte_el_nombre_de_la_herramienta() {
        // `C:` lleva dos puntos. Partir por todos daría `readfile` + `C` + el
        // resto, y la herramienta recibiría una ruta rota.
        let (name, args) = parse_tool("readfile:C:\\logs\\lucy_app.log");
        assert_eq!(name, "readfile");
        assert_eq!(args, "C:\\logs\\lucy_app.log");

        // Y una herramienta sin argumentos no inventa ninguno.
        let (name, args) = parse_tool("sysinfo");
        assert_eq!(name, "sysinfo");
        assert!(args.is_empty());
    }

    #[test]
    fn las_comprobaciones_baratas_coinciden_con_las_de_la_v2() {
        assert!(has_tool_response("bla <TOOL>sysinfo</TOOL>"));
        assert!(has_tool_response("<EXECUTE_CMD>dir</EXECUTE_CMD>"));
        assert!(has_tool_response("<thought>en minúsculas</thought>"));
        assert!(!has_tool_response("una respuesta normal y corriente"));

        assert!(is_multi_step("<THOUGHT>x</THOUGHT>"));
        assert!(is_multi_step("<TOOL>a</TOOL><EXECUTE>b</EXECUTE>"));
        assert!(!is_multi_step("<TOOL>a</TOOL>"), "una herramienta sola es un paso");
    }

    #[test]
    fn un_texto_sin_etiquetas_no_devuelve_nada() {
        assert!(extract_tags("").is_empty());
        assert!(extract_tags("Todo bien, el disco está al 29 %.").is_empty());
        // Una etiqueta a medio llegar —el stream aún no trajo el cierre— NO se
        // detecta aquí. Es correcto: esta es la detección de DESPUÉS del
        // stream. La del bucle sí tolera cierres truncados, y son dos cosas
        // distintas a propósito.
        assert!(extract_tags("<EXECUTE>Get-Proc").is_empty());
        // Un `<` suelto tampoco puede colgar el escáner.
        assert!(extract_tags("a < b y c > d").is_empty());
    }

    #[test]
    fn varias_del_mismo_tipo_se_recogen_todas() {
        let t = "<TOOL>sysinfo</TOOL> texto <TOOL>netconn</TOOL>";
        let tags = extract_tags(t);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].content, "sysinfo");
        assert_eq!(tags[1].content, "netconn");
    }
}

#[cfg(test)]
mod display {
    use super::*;

    #[test]
    fn el_marcado_no_llega_al_hilo() {
        let r = clean_display(
            "Voy a mirarlo.\n<TOOL>readfile:C:/log.txt</TOOL>\nYa está.",
        );
        assert_eq!(r.text, "Voy a mirarlo.\n\nYa está.");
        assert!(!r.text.contains("<TOOL>"));
    }

    #[test]
    fn una_etiqueta_a_medias_no_parpadea_en_pantalla() {
        // El caso de estar A MITAD del stream: el cierre aún no ha llegado. Sin
        // tolerarlo, el marcado a medio escribir sale en pantalla y desaparece
        // al cerrarse — un parpadeo por cada herramienta que Lucy usa.
        let r = clean_display("Reviso el disco.\n<EXECUTE>Get-PSDri");
        assert_eq!(r.text, "Reviso el disco.");
        // Y esto es LO CONTRARIO de lo que hace `extract_tags`, a propósito:
        // allí una etiqueta sin cerrar todavía no es una acción.
        assert!(extract_tags("<EXECUTE>Get-PSDri").is_empty());
    }

    #[test]
    fn execute_remote_no_se_come_el_resto_del_texto() {
        // El fallo del que avisa el comentario del original: si `execute`
        // encontrara el `<execute_remote target=…` y cortara desde ahí sin
        // cierre propio, se llevaría por delante todo lo que viene después.
        let r = clean_display(
            "Antes.\n<EXECUTE_REMOTE target=\"SRV\">Get-Service</EXECUTE_REMOTE>\nDespués.",
        );
        assert_eq!(r.text, "Antes.\n\nDespués.");
    }

    #[test]
    fn el_razonamiento_se_guarda_en_vez_de_borrarse() {
        // La decisión del original: `<THOUGHT>` NO se tira, se enseña plegado.
        // Borrarlo pierde la única explicación de por qué Lucy hizo lo que hizo.
        let r = clean_display("<THOUGHT>Primero el disco, luego los servicios.</THOUGHT>Listo.");
        assert_eq!(r.text, "Listo.");
        assert_eq!(r.thoughts, vec!["Primero el disco, luego los servicios."]);
    }

    #[test]
    fn un_texto_normal_sale_intacto() {
        let t = "El disco C: está al 29 % y no hay servicios caídos.";
        assert_eq!(clean_display(t).text, t);
        assert!(clean_display(t).thoughts.is_empty());
    }
}

#[cfg(test)]
mod codegen {
    use super::*;

    #[test]
    fn pedir_un_script_no_es_lo_mismo_que_pedir_una_accion() {
        // "reinicia el spooler" quiere que algo pase y el comando es
        // fontanería; "dame un script para reiniciarlo" quiere el comando EN la
        // respuesta, y esconderlo deja un texto señalando a un hueco.
        assert!(detect_code_gen_intent("dame un script para reiniciar el spooler"));
        assert!(detect_code_gen_intent("Escribe un script de limpieza"));
        assert!(detect_code_gen_intent("dame el comando de powershell"));
        assert!(detect_code_gen_intent("necesito código para esto"));
        assert!(detect_code_gen_intent("write a script that lists services"));

        assert!(!detect_code_gen_intent("reinicia el spooler"));
        assert!(!detect_code_gen_intent("¿qué servicios están detenidos?"));
        // La palabra suelta no basta: hace falta que la PIDA.
        assert!(!detect_code_gen_intent("el script que corrió ayer falló"));
    }

    #[test]
    fn en_modo_codigo_el_comando_se_ve_en_vez_de_desaparecer() {
        let t = "Aquí tienes:\n<EXECUTE>Get-Service</EXECUTE>\nEso lista todo.";
        let oculto = clean_display_with(t, false);
        assert!(!oculto.text.contains("Get-Service"));
        assert_eq!(oculto.commands, 1, "se quitó uno y hay que poder decirlo");

        let visible = clean_display_with(t, true);
        assert!(visible.text.contains("```powershell"));
        assert!(visible.text.contains("Get-Service"));
        // Y `EXECUTE_CMD` va con su propio lenguaje, no con el de PowerShell.
        let c = clean_display_with("<EXECUTE_CMD>dir</EXECUTE_CMD>", true);
        assert!(c.text.contains("```cmd"), "salió: {}", c.text);
    }

    #[test]
    fn la_cuenta_de_comandos_permite_no_dejar_la_prosa_colgando() {
        // El síntoma real: el modelo escribe "usa esta sintaxis:" y debajo no
        // hay nada, porque el comando se fue al panel. Con la cuenta, quien
        // dibuja puede decir a dónde fue.
        let c = clean_display("<EXECUTE>uno</EXECUTE><EXECUTE_REG>dos</EXECUTE_REG>");
        assert_eq!(c.commands, 2);
        assert_eq!(clean_display("sin comandos").commands, 0);
    }
}
