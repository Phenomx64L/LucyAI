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
