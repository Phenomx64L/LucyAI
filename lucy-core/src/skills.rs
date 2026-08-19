//! Los skills: instrucciones que Lucy carga cuando vienen al caso.
//!
//! POR QUÉ NO SE PORTA EL DE LA V2 TAL CUAL. Allí un skill es un objeto de
//! TypeScript compilado dentro del binario, con un array de fases fijas que se
//! recorren en orden y un veredicto entre cada una. Eso es un autómata, y como
//! autómata funciona; lo que no es, es un skill. Añadir uno exige tocar el
//! código y recompilar la aplicación, así que en la práctica hay los cinco que
//! venían y ninguno más — que es exactamente la sensación de «está conectado
//! pero no del todo».
//!
//! AQUÍ UN SKILL ES UNA CARPETA CON UN `SKILL.md`. Se descubre en disco, lleva
//! su nombre y una descripción de cuándo aplica, y su cuerpo son instrucciones
//! que el modelo sigue con su criterio. Se añade uno dejando un fichero. La
//! diferencia de fondo con las fases: un autómata decide POR el modelo y falla
//! entero cuando la máquina no encaja con lo que su autor supuso; unas
//! instrucciones lo guían y le dejan la parte que solo se puede resolver
//! mirando.
//!
//! EL CATÁLOGO VIAJA EN EL PROMPT; EL CUERPO NO. Solo el nombre y la descripción
//! —dos líneas por skill— para que Lucy sepa cuáles hay. El cuerpo entero de
//! cinco skills serían miles de tokens en cada turno de cada conversación, casi
//! siempre para no usar ninguno. Se carga el que pide, cuando lo pide.

/// Un skill leído del disco.
#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    /// El identificador, que es el nombre de su carpeta. Es lo que Lucy escribe
    /// para pedirlo, así que va en minúsculas y sin espacios.
    pub name: String,
    /// Una línea: qué hace y cuándo aplica. Es lo ÚNICO que el modelo ve del
    /// skill hasta que lo pide, así que es lo que decide si lo pide.
    pub description: String,
    /// Las instrucciones. Lo que se le entrega cuando lo invoca.
    pub body: String,
    /// Si entra en el catálogo que ve Lucy.
    ///
    /// APAGAR NO ES BORRAR, y la diferencia importa: un skill desactivado sigue
    /// en disco con su procedimiento dentro, listo para volver con un clic. Lo
    /// que se quiere casi siempre es «ahora no» —el de migración estorba mientras
    /// se atiende una incidencia— y para eso desinstalar es demasiado: hay que
    /// volver a encontrar la carpeta y reinstalarla.
    pub activo: bool,
    /// Dónde vive su carpeta.
    ///
    /// HACE FALTA PARA PODER APAGARLO. `set_enabled` resolvía la ruta por su
    /// cuenta contra el perfil del usuario, y los skills NO SOLO VIVEN AHÍ: el
    /// shell los busca además junto al ejecutable y en `./skills` del proyecto.
    /// Los cinco que trae Lucy salen de esa última, así que apagarlos fallaba
    /// siempre con «no está instalado en tu perfil» — un mensaje que además
    /// mandaba a buscar el problema donde no estaba.
    ///
    /// Vacío cuando el skill no viene de un directorio, que hoy solo pasa en los
    /// tests de `parse`.
    pub dir: std::path::PathBuf,
}

/// El fichero que marca un skill como apagado.
///
/// UN FICHERO Y NO UNA FILA EN LA BASE, para que valga la misma regla que rige
/// todo este módulo: lo que Lucy aprende acaba siendo un fichero que se puede
/// leer, copiar a otra máquina y borrar a mano. Un skill apagado desde una base
/// de datos volvería encendido al copiar la carpeta a otro equipo, y nadie
/// sabría por qué.
pub const MARCA_APAGADO: &str = ".disabled";

/// Enciende o apaga un skill. `Ok(())` también si ya estaba como se pide.
/// SE LE PASA EL SKILL, no su nombre. Antes resolvía la ruta por su cuenta
/// contra el perfil del usuario y daba por hecho que todos viven ahí; no es
/// verdad —el shell los busca también junto al ejecutable y en `./skills`— así
/// que apagar cualquiera de los que trae Lucy fallaba con «no está instalado en
/// tu perfil», que además manda a buscar el problema donde no está.
///
/// La marca sigue yendo DENTRO de la carpeta del skill, que es lo que hace que
/// el estado viaje con ella al copiarla a otra máquina. Si esa carpeta no se
/// puede escribir —una instalación en Archivos de programa— el error lo dice
/// con la ruta, en vez de callarse o mentir sobre el perfil.
pub fn set_enabled(k: &Skill, activo: bool) -> Result<(), String> {
    let nombre = &k.name;
    if !k.dir.is_dir() {
        return Err(format!(
            "no se encuentra la carpeta de «{nombre}»: {}",
            k.dir.display()
        ));
    }
    let marca = k.dir.join(MARCA_APAGADO);
    if activo {
        // Que no exista no es un error: apagar dos veces y encender dos veces
        // tienen que acabar en el mismo sitio.
        if marca.exists() {
            std::fs::remove_file(&marca)
                .map_err(|e| format!("no se pudo activar «{nombre}»: {e}"))?;
        }
    } else {
        std::fs::write(&marca, b"")
            .map_err(|e| format!("no se pudo desactivar «{nombre}»: {e}"))?;
    }
    Ok(())
}

/// Tope del cuerpo de un skill.
///
/// Se carga entero en un turno, así que un fichero desmandado se lleva por
/// delante el presupuesto de la conversación. Veinte mil caracteres son unas
/// cinco mil palabras: de sobra para un procedimiento, y un aviso claro para
/// quien esté escribiendo un manual.
pub const MAX_BODY: usize = 20_000;

/// Lee los skills de un directorio.
///
/// Cada subcarpeta con un `SKILL.md` es un skill; el nombre de la carpeta es su
/// identificador. Una carpeta sin `SKILL.md` se ignora en silencio: puede ser
/// cualquier cosa que alguien dejó ahí, y quejarse de ella convertiría el
/// directorio en algo que hay que mantener limpio.
pub fn discover(dir: &std::path::Path) -> Vec<Skill> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<Skill> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if !p.is_dir() {
                return None;
            }
            let md = std::fs::read_to_string(p.join("SKILL.md")).ok()?;
            let name = p.file_name()?.to_str()?.to_lowercase();
            // Un skill apagado SE DESCUBRE IGUAL: tiene que salir en la lista
            // para poder volver a encenderlo. Lo que cambia es que no entra en
            // el catálogo que ve Lucy.
            let activo = !p.join(MARCA_APAGADO).exists();
            let mut k = parse(&name, &md, activo);
            k.dir = p;
            Some(k)
        })
        .collect();
    // Por nombre, para que el catálogo del prompt no cambie de orden entre
    // arranques: un prompt que cambia sin motivo tira la caché del proveedor.
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// Separa la cabecera del cuerpo de un `SKILL.md`.
///
/// El formato es el mismo que usan los skills de Claude Code, y a propósito: un
/// bloque `---` con `name:` y `description:`, y debajo las instrucciones en
/// Markdown. Quien ya tenga uno escrito no tiene que aprender otro formato, y
/// quien escriba uno aquí puede llevárselo.
///
/// SIN CABECERA TAMBIÉN VALE. Un fichero suelto con instrucciones es un skill
/// perfectamente útil; exigirle una cabecera sería rechazar lo que alguien
/// acaba de escribir por una formalidad. Sin descripción, se usa la primera
/// línea con texto — que es lo que su autor puso primero por algo.
pub fn parse(name: &str, md: &str, activo: bool) -> Skill {
    let (cabecera, cuerpo) = match md.strip_prefix("---") {
        Some(resto) => match resto.split_once("\n---") {
            Some((c, b)) => (c, b.trim_start_matches(['\r', '\n'])),
            None => ("", md),
        },
        None => ("", md),
    };
    let campo = |clave: &str| -> Option<String> {
        cabecera.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim() == clave).then(|| v.trim().trim_matches(['"', '\'']).to_string())
        })
    };
    let description = campo("description")
        .filter(|d| !d.is_empty())
        .or_else(|| {
            cuerpo
                .lines()
                .map(|l| l.trim_start_matches('#').trim())
                .find(|l| !l.is_empty())
                .map(|l| l.to_string())
        })
        .unwrap_or_default();
    Skill {
        // El `name:` de la cabecera se lee pero NO manda sobre la carpeta: el
        // identificador tiene que ser el que se ve al mirar el disco, o pedir un
        // skill por su nombre y no encontrarlo se vuelve un misterio.
        name: name.to_string(),
        description: crate::skills::una_linea(&description, 200),
        body: cuerpo.chars().take(MAX_BODY).collect(),
        // `parse` recibe un texto, no un directorio: quien lo tenga lo pone
        // después. Es lo que hace `discover`.
        dir: std::path::PathBuf::new(),
        activo,
    }
}

/// Aplana un texto a una línea, recortado. Una descripción con saltos rompería
/// la lista del prompt en entradas falsas.
fn una_linea(s: &str, max: usize) -> String {
    let plano = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if plano.chars().count() <= max {
        return plano;
    }
    plano.chars().take(max).collect::<String>() + "…"
}

/// El catálogo, tal como viaja en el prompt. Vacío = no hay skills.
pub fn catalog(skills: &[Skill]) -> String {
    // SOLO LOS ACTIVOS, que es todo el efecto de apagar uno: deja de estar en la
    // lista que el modelo lee, así que deja de pedirlo. El fichero sigue en
    // disco y volver a encenderlo es un clic.
    let skills: Vec<&Skill> = skills.iter().filter(|k| k.activo).collect();
    if skills.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "SKILLS DISPONIBLES\n\
         Son procedimientos escritos para esta instalación. Si uno encaja con lo que \
         te piden, PÍDELO con <TOOL>skill:nombre</TOOL> y sigue lo que diga; si ninguno \
         encaja, trabaja normalmente y no lo fuerces.\n",
    );
    for k in skills {
        s.push_str(&format!("· {} — {}\n", k.name, k.description));
    }
    s.trim_end().to_string()
}

/// Convierte un `<LEARN>` en el contenido de un `SKILL.md`.
///
/// EL FORMATO DE LA V2 ES `claves|script|respuesta`: cuándo aplica, qué ejecutar
/// y qué contestar. Es exactamente un skill escrito en una línea, y por eso se
/// traduce en vez de construirle un almacén propio.
///
/// LO QUE LUCY APRENDE ACABA SIENDO UN FICHERO, y eso es la diferencia de fondo
/// con un almacén interno: se puede leer, corregir a mano, copiar a otra máquina
/// y borrar. Un procedimiento aprendido que solo vive dentro de una base de datos
/// es un procedimiento que nadie revisa hasta que falla.
pub fn from_learn(payload: &str) -> Option<(String, String)> {
    let mut p = payload.splitn(3, '|');
    let claves = p.next()?.trim();
    let script = p.next().unwrap_or("").trim();
    let respuesta = p.next().unwrap_or("").trim();
    if claves.is_empty() || (script.is_empty() && respuesta.is_empty()) {
        return None;
    }
    // El nombre sale de la primera clave: es la que el operador dijo primero, y
    // suele ser la que usaría para buscarlo.
    // Se quitan los acentos ANTES de filtrar. `is_alphanumeric` los acepta
    // —en Unicode una `ó` es una letra— y `is_ascii_alphanumeric` no, así que
    // sin este paso «producción» acaba en `producci-n`. Quitar la tilde es lo
    // que uno quiere de un nombre de carpeta escrito en español.
    let sin_tildes: String = claves
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            otro => otro,
        })
        .collect();
    let mut nombre = String::new();
    for c in sin_tildes.chars() {
        if c.is_ascii_alphanumeric() {
            nombre.push(c);
        } else if !nombre.ends_with('-') {
            nombre.push('-');
        }
    }
    let nombre = nombre.trim_matches('-').to_string();
    if nombre.is_empty() {
        return None;
    }
    let mut md = format!(
        "---\nname: {nombre}\ndescription: {}\n---\n\n# {nombre}\n\n",
        una_linea(claves, 200)
    );
    if !script.is_empty() {
        md.push_str(&format!("## Qué ejecutar\n\n```\n{script}\n```\n\n"));
    }
    if !respuesta.is_empty() {
        md.push_str(&format!("## Qué responder\n\n{respuesta}\n\n"));
    }
    // La fecha NO se pone aquí: `SystemTime` daría una hora distinta en cada
    // ejecución y el test dejaría de poder afirmar nada. Quien lo escriba en
    // disco puede añadirla si hace falta.
    md.push_str(
        "---\n\nAprendido de una conversación. Si al aplicarlo la máquina no responde como \
         dice aquí, manda lo que ves — este procedimiento se escribió en un momento y la \
         máquina es de ahora.\n",
    );
    Some((nombre, md))
}

/// Dónde se instalan los skills del operador.
///
/// EN SU PERFIL Y NO JUNTO AL EJECUTABLE. Lo segundo parece más ordenado y se
/// borra con la próxima actualización: `Program Files` es de la aplicación, no
/// del operador. Aquí sobreviven a reinstalar Lucy, que es lo mínimo que se le
/// pide a algo que alguien se ha molestado en escribir.
pub fn user_dir() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("lucy-egui").join("skills"))
}

/// Instala uno o varios skills desde una carpeta.
///
/// ACEPTA LAS DOS FORMAS que la gente tiene de verdad: una carpeta que ES un
/// skill —lleva su `SKILL.md` dentro— o una carpeta que CONTIENE skills, que es
/// como llega un repositorio descargado y descomprimido. Obligar a señalar cada
/// subcarpeta de un repositorio de treinta skills sería treinta viajes al
/// diálogo de ficheros.
///
/// Devuelve los nombres instalados. Una carpeta sin ningún `SKILL.md` es un
/// error CON su explicación: es la equivocación más probable —señalar el
/// directorio de al lado— y «no se instaló nada» no ayuda a corregirla.
pub fn install(origen: &std::path::Path, destino: &std::path::Path) -> Result<Vec<String>, String> {
    let mut candidatos: Vec<std::path::PathBuf> = Vec::new();
    if origen.join("SKILL.md").is_file() {
        candidatos.push(origen.to_path_buf());
    } else {
        let rd = std::fs::read_dir(origen)
            .map_err(|e| format!("No se pudo leer «{}»: {e}", origen.display()))?;
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && p.join("SKILL.md").is_file() {
                candidatos.push(p);
            }
        }
    }
    if candidatos.is_empty() {
        return Err(format!(
            "En «{}» no hay ningún SKILL.md. Un skill es una carpeta con ese fichero \
             dentro; señala la carpeta del skill, o una que las contenga.",
            origen.display()
        ));
    }
    std::fs::create_dir_all(destino)
        .map_err(|e| format!("No se pudo crear «{}»: {e}", destino.display()))?;

    let mut puestos = Vec::new();
    for c in candidatos {
        let Some(nombre) = c.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let nombre = nombre.to_lowercase();
        let dst = destino.join(&nombre);
        // Se copia SOLO el primer nivel de ficheros. Un skill puede traer
        // material de apoyo, y también un repositorio entero con su `.git`: dos
        // mil ficheros de historia que no hacen falta para leer un
        // procedimiento.
        if let Err(e) = copiar_plano(&c, &dst) {
            return Err(format!("Instalando «{nombre}»: {e}"));
        }
        puestos.push(nombre);
    }
    puestos.sort();
    Ok(puestos)
}

/// Copia los ficheros de un directorio, sin bajar a sus subcarpetas.
fn copiar_plano(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        if let Some(n) = p.file_name() {
            std::fs::copy(&p, dst.join(n)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Desinstala un skill del directorio del operador.
///
/// SOLO DE AHÍ. Los que vienen con Lucy están junto al ejecutable y borrarlos
/// desde la interfaz dejaría la instalación distinta de la que se instaló, sin
/// forma de volver. Si molesta uno de esos, se tapa poniendo otro con el mismo
/// nombre en el perfil — que es para lo que el perfil gana.
pub fn uninstall(nombre: &str) -> Result<(), String> {
    let dir = user_dir().ok_or("No se pudo resolver el perfil del usuario")?;
    let p = dir.join(nombre.to_lowercase());
    if !p.is_dir() {
        return Err(format!(
            "«{nombre}» no está instalado en tu perfil. Los que vienen con Lucy no se \
             borran desde aquí: pon uno con el mismo nombre para taparlo."
        ));
    }
    std::fs::remove_dir_all(&p).map_err(|e| format!("No se pudo borrar «{nombre}»: {e}"))
}

/// Un skill FIJADO, tal como viaja en el prompt de cada turno.
///
/// LA DIFERENCIA CON PEDIRLO es cuándo se aplica, no qué dice. Un skill se pide
/// para una tarea y se acaba con ella; un preset se fija y enmarca todo lo que
/// venga después hasta que alguien lo quite. La V2 lo llama «modo skill» y tiene
/// razón: no es otro tipo de contenido, es el mismo contenido con otra duración.
///
/// Por eso comparten fichero. Dos sistemas paralelos para lo mismo obligarían a
/// escribir cada procedimiento dos veces, y la copia que menos se use será la
/// que se quede vieja.
pub fn preset_block(k: &Skill) -> String {
    format!(
        "MODO ACTIVO: «{}»\n\
         El operador ha fijado este procedimiento. Enmárcalo TODO en él mientras siga \
         puesto: si lo que te piden encaja, aplícalo; si no encaja, dilo y contesta igual \
         — un modo fijado no es una orden de forzarlo todo por ese camino.\n\n{}",
        k.name, k.body
    )
}

/// Busca un skill por nombre, tolerando cómo lo escriba el modelo.
///
/// Se acepta con o sin espacios, mayúsculas o guiones bajos: pedir
/// `DNS Troubleshoot` cuando la carpeta es `dns-troubleshoot` es lo que va a
/// pasar, y responder «no existe» a eso sería tener razón y no servir de nada.
pub fn find<'a>(skills: &'a [Skill], pedido: &str) -> Option<&'a Skill> {
    let norm = |s: &str| {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let q = norm(pedido);
    skills.iter().find(|k| norm(&k.name) == q)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CON_CABECERA: &str = "---\nname: dns\ndescription: Diagnostica resolución DNS.\n---\n\n\
                                # Pasos\n\n1. Mira `/etc/resolv.conf`.\n";

    #[test]
    fn se_lee_la_cabecera_y_el_cuerpo_por_separado() {
        let k = parse("dns-troubleshoot", CON_CABECERA, true);
        assert_eq!(k.description, "Diagnostica resolución DNS.");
        assert!(k.body.starts_with("# Pasos"), "{:?}", k.body);
        assert!(!k.body.contains("description:"), "la cabecera se coló en el cuerpo");
    }

    #[test]
    fn el_identificador_es_la_carpeta_y_no_el_name_de_la_cabecera() {
        // Si mandara la cabecera, pedir un skill por el nombre que se ve al
        // mirar el disco no lo encontraría, y eso es un misterio de los que
        // cuestan una tarde.
        let k = parse("dns-troubleshoot", CON_CABECERA, true);
        assert_eq!(k.name, "dns-troubleshoot", "el `name:` de la cabecera mandó");
    }

    #[test]
    fn un_fichero_sin_cabecera_tambien_es_un_skill() {
        // Exigir una cabecera sería rechazar lo que alguien acaba de escribir
        // por una formalidad. La descripción sale de la primera línea con texto,
        // que es lo que su autor puso primero por algo.
        let k = parse("limpieza", "# Limpiar discos\n\nBorra temporales viejos.\n", true);
        assert_eq!(k.description, "Limpiar discos");
        assert!(k.body.contains("Borra temporales"));
    }

    #[test]
    fn una_descripcion_con_saltos_no_rompe_la_lista() {
        // El catálogo es una línea por skill. Una descripción multilínea
        // produciría entradas falsas que el modelo leería como skills que no
        // existen.
        let k = parse("x", "---\ndescription: uno\n---\ncuerpo", true);
        assert!(!k.description.contains('\n'));
        let k2 = parse("y", "primera\nsegunda\ntercera", true);
        assert_eq!(k2.description, "primera");
    }

    #[test]
    fn el_catalogo_lleva_nombre_descripcion_y_como_pedirlo() {
        // Sin el cómo, el modelo sabe que existen y no sabe invocarlos — que es
        // peor que no saber que existen.
        let ks = vec![parse("dns", CON_CABECERA, true)];
        let c = catalog(&ks);
        assert!(c.contains("dns"));
        assert!(c.contains("Diagnostica"));
        assert!(c.contains("<TOOL>skill:nombre</TOOL>"), "no dice cómo pedirlo");
        // Y se le dice que no lo fuerce: un skill aplicado a algo que no era
        // hace peor trabajo que ninguno.
        assert!(c.contains("no lo fuerces"));
    }

    #[test]
    fn un_skill_apagado_desaparece_del_catalogo_pero_no_del_disco() {
        // ESE ES TODO EL EFECTO de apagar uno: deja de estar en la lista que el
        // modelo lee, así que deja de pedirlo. Apagar no es borrar — el fichero
        // sigue ahí con su procedimiento dentro, y volver a encenderlo es un
        // clic en vez de buscar la carpeta y reinstalarla.
        let apagado = parse("migracion", CON_CABECERA, false);
        let encendido = parse("dns", CON_CABECERA, true);
        let c = catalog(&[apagado.clone(), encendido]);
        assert!(c.contains("dns"), "el activo tiene que salir: {c}");
        assert!(!c.contains("migracion"), "el apagado no puede salir: {c}");
        // Y conserva su cuerpo: no es una fila vaciada, es una fila marcada.
        assert!(!apagado.body.is_empty());
        assert!(!apagado.activo);
    }

    #[test]
    fn con_todos_apagados_el_catalogo_es_vacio_y_no_una_cabecera_huerfana() {
        // Un bloque que dice «SKILLS DISPONIBLES» y no lista ninguno enseña al
        // modelo a saltarse ese encabezado, así que el día que sí traiga algo
        // tampoco lo mirará.
        assert!(catalog(&[parse("a", CON_CABECERA, false)]).is_empty());
    }

    #[test]
    fn un_preset_lleva_el_cuerpo_y_permiso_para_no_aplicarlo() {
        // Sin ese permiso, un modo fijado convierte cada pregunta en un clavo
        // para su martillo: se pregunta la hora y contesta con un diagnóstico
        // de DNS porque es lo que dice el modo.
        let k = parse("dns", CON_CABECERA, true);
        let b = preset_block(&k);
        assert!(b.contains("# Pasos"), "no lleva el cuerpo");
        assert!(b.contains("si no encaja, dilo"), "no le deja salirse: {b}");
        assert!(b.contains("dns"), "no dice cuál está puesto");
    }

    #[test]
    fn sin_skills_el_catalogo_esta_vacio() {
        // Para que la sección del prompt no aparezca diciendo que no hay nada:
        // un bloque que no dice nada enseña al modelo a saltarse ese encabezado.
        assert!(catalog(&[]).is_empty());
    }

    #[test]
    fn se_encuentra_aunque_lo_escriba_distinto() {
        // Va a pedir `DNS Troubleshoot` cuando la carpeta es
        // `dns-troubleshoot`. Contestar «no existe» a eso es tener razón y no
        // servir de nada.
        let ks = vec![parse("dns-troubleshoot", CON_CABECERA, true)];
        for pedido in ["dns-troubleshoot", "DNS Troubleshoot", "dns_troubleshoot", "DNSTroubleshoot"] {
            assert!(find(&ks, pedido).is_some(), "no encontró «{pedido}»");
        }
        assert!(find(&ks, "otra-cosa").is_none());
    }

    #[test]
    fn un_cuerpo_desmandado_se_recorta() {
        // Se carga entero en un turno: un manual de doscientas páginas se
        // llevaría por delante el presupuesto de la conversación.
        let largo = format!("---\ndescription: x\n---\n{}", "y".repeat(MAX_BODY + 5_000));
        assert_eq!(parse("k", &largo, true).body.chars().count(), MAX_BODY);
    }

    /// Un directorio temporal propio de este test, y su limpieza.
    fn tmp(nombre: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("lucy_skills_{nombre}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn poner(dir: &std::path::Path, nombre: &str) {
        let d = dir.join(nombre);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("SKILL.md"), "---\ndescription: d\n---\ncuerpo").unwrap();
    }

    #[test]
    fn se_puede_apagar_un_skill_que_no_vive_en_el_perfil() {
        // EL FALLO DE LA CAPTURA. `set_enabled` resolvía la ruta contra el perfil
        // del usuario y daba por hecho que todos viven ahí. No es verdad: el
        // shell los busca también junto al ejecutable y en `./skills`, y los
        // cinco que trae Lucy salen de esa última. Apagar cualquiera contestaba
        // «no está instalado en tu perfil» y no apagaba nada — con un mensaje
        // que encima manda a buscar el problema donde no está.
        let dir = tmp("fuera_del_perfil");
        poner(&dir, "dns-troubleshoot");
        let k = discover(&dir).into_iter().next().expect("tiene que descubrirse");
        assert!(k.activo, "recién puesto tiene que estar activo");
        assert_eq!(k.dir, dir.join("dns-troubleshoot"), "no sabe dónde vive");

        set_enabled(&k, false).expect("apagar tiene que funcionar viva donde viva");
        let apagado = discover(&dir).into_iter().next().unwrap();
        assert!(!apagado.activo, "sigue activo después de apagarlo");
        // Y deja de estar en lo que Lucy ve, que es todo el efecto de apagarlo.
        assert!(
            catalog(&[apagado.clone()]).is_empty(),
            "un skill apagado no puede seguir en el catálogo"
        );

        set_enabled(&apagado, true).expect("volver a encenderlo tiene que funcionar");
        assert!(discover(&dir).into_iter().next().unwrap().activo);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apagar_dos_veces_acaba_donde_apagar_una() {
        // Encender lo que ya está encendido no puede ser un error: la interfaz
        // manda el estado que quiere, no la diferencia.
        let dir = tmp("idempotente");
        poner(&dir, "ssl-check");
        let k = discover(&dir).into_iter().next().unwrap();
        set_enabled(&k, false).unwrap();
        let k = discover(&dir).into_iter().next().unwrap();
        set_enabled(&k, false).unwrap();
        assert!(!discover(&dir).into_iter().next().unwrap().activo);
        let k = discover(&dir).into_iter().next().unwrap();
        set_enabled(&k, true).unwrap();
        let k = discover(&dir).into_iter().next().unwrap();
        set_enabled(&k, true).unwrap();
        assert!(discover(&dir).into_iter().next().unwrap().activo);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lo_aprendido_se_convierte_en_un_skill_legible() {
        // El formato de la V2 —`claves|script|respuesta`— es un skill escrito en
        // una línea. Traducirlo evita construirle un almacén propio, y hace que
        // lo aprendido acabe siendo un fichero que se puede leer y corregir.
        let (n, md) = from_learn(
            "reiniciar spooler, cola de impresión|Restart-Service Spooler|Se reinicia el servicio de impresión.",
        )
        .unwrap();
        assert_eq!(n, "reiniciar-spooler");
        let k = parse(&n, &md, true);
        assert!(k.description.contains("cola de impresión"), "{}", k.description);
        assert!(k.body.contains("Restart-Service Spooler"));
        assert!(k.body.contains("Se reinicia el servicio"));
        // Y el recordatorio: se escribió en un momento, la máquina es de ahora.
        assert!(k.body.contains("manda lo que ves"), "{}", k.body);
    }

    #[test]
    fn un_learn_a_medias_no_produce_un_skill_vacio() {
        // Un skill sin nada que hacer ni que decir es una entrada en el catálogo
        // que gasta tokens en cada turno para no servir de nada.
        assert!(from_learn("").is_none());
        assert!(from_learn("solo-claves").is_none());
        assert!(from_learn("|script|respuesta").is_none(), "sin claves no hay nombre");
        // Con solo respuesta SÍ vale: enseñarle qué contestar es aprender.
        assert!(from_learn("dns lento||Revisa el reenviador").is_some());
    }

    #[test]
    fn el_nombre_aprendido_es_un_nombre_de_carpeta_valido() {
        // Sale de texto que escribió un modelo: acentos, comas, barras y todo lo
        // que a un sistema de ficheros no le gusta.
        let (n, _) = from_learn("Reiniciar IIS/W3SVC en producción|iisreset|listo").unwrap();
        assert!(n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'), "salió «{n}»");
        assert!(!n.starts_with('-') && !n.ends_with('-'), "salió «{n}»");
    }

    #[test]
    fn se_instala_una_carpeta_que_es_un_skill() {
        let (o, d) = (tmp("o1"), tmp("d1"));
        poner(&o, "dns");
        let r = install(&o.join("dns"), &d).unwrap();
        assert_eq!(r, vec!["dns"]);
        assert!(d.join("dns").join("SKILL.md").is_file());
        let _ = std::fs::remove_dir_all(&o);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn se_instala_un_repositorio_entero_de_una_vez() {
        // Es como llega uno descargado y descomprimido. Obligar a señalar cada
        // subcarpeta de treinta skills serían treinta viajes al diálogo.
        let (o, d) = (tmp("o2"), tmp("d2"));
        poner(&o, "dns");
        poner(&o, "ssl");
        std::fs::create_dir_all(o.join("no-es-un-skill")).unwrap();
        let r = install(&o, &d).unwrap();
        assert_eq!(r, vec!["dns", "ssl"], "no cogió los dos, o cogió el que no era");
        assert!(!d.join("no-es-un-skill").exists(), "instaló una carpeta sin SKILL.md");
        let _ = std::fs::remove_dir_all(&o);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn señalar_la_carpeta_equivocada_se_explica() {
        // Es la equivocación más probable, y «no se instaló nada» no ayuda a
        // corregirla.
        let (o, d) = (tmp("o3"), tmp("d3"));
        let e = install(&o, &d).unwrap_err();
        assert!(e.contains("SKILL.md"), "{e}");
        assert!(e.contains("carpeta"), "no dice qué señalar: {e}");
        let _ = std::fs::remove_dir_all(&o);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn no_se_arrastra_el_historial_del_repositorio() {
        // Un repositorio descargado trae su `.git`: miles de ficheros que no
        // hacen falta para leer un procedimiento.
        let (o, d) = (tmp("o4"), tmp("d4"));
        poner(&o, "dns");
        std::fs::create_dir_all(o.join("dns").join(".git")).unwrap();
        std::fs::write(o.join("dns").join(".git").join("HEAD"), "ref: x").unwrap();
        install(&o.join("dns"), &d).unwrap();
        assert!(d.join("dns").join("SKILL.md").is_file());
        assert!(!d.join("dns").join(".git").exists(), "se trajo el .git");
        let _ = std::fs::remove_dir_all(&o);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn desinstalar_lo_que_no_es_tuyo_se_explica_en_vez_de_fallar_raro() {
        let e = uninstall("un-skill-que-nadie-instalo").unwrap_err();
        assert!(e.contains("no está instalado"), "{e}");
        assert!(e.contains("taparlo"), "no dice la alternativa: {e}");
    }

    #[test]
    fn un_directorio_que_no_existe_no_es_un_error() {
        // Una instalación sin skills es normal. Que la lectura falle no puede
        // ser motivo de nada visible.
        assert!(discover(std::path::Path::new("no-existe-este-directorio")).is_empty());
    }
}
