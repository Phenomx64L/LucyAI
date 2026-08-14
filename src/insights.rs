//! Insights: lo que se repite entre memorias que nadie escribió juntas.
//!
//! Es la tercera cosa que hace la memoria, y las tres son distintas:
//!
//!   • Consolidar FUNDE dos filas que dicen lo mismo. Queda una.
//!   • Un cristal RESUME una sesión. Queda una fila nueva por sesión.
//!   • Un insight OBSERVA UN PATRÓN entre filas que siguen ahí. No borra ninguna,
//!     y sobre todo VUELVE: cada pasada que reencuentra el mismo patrón lo
//!     refuerza en vez de duplicarlo, y la confianza sube.
//!
//! «Los errores de WinRM casi siempre son desfase de reloj» no está escrito en
//! ninguna memoria: está repartido entre seis, y solo se ve mirándolas juntas.
//!
//! POR QUÉ NO SE AGRUPA POR ETIQUETAS, que es como lo hacía la V2. Allí dos
//! memorias entraban en el mismo grupo si compartían DOS etiquetas o más. Con el
//! corpus que produce esta aplicación eso significa exactamente dos cosas:
//!
//!   • Las memorias automáticas de turno llevan UNA etiqueta (`auto`). Una sola
//!     etiqueta no puede compartir dos con nadie. No se agrupan NUNCA — y son la
//!     fuente principal de memorias del shell.
//!   • Las lecciones de un cristal llevan siempre las mismas dos (`crystal`,
//!     `leccion`). Comparten dos con TODAS las demás, así que caen todas en un
//!     único grupo gigante sin que importe de qué hablan; y en cuanto pasan de
//!     veinte, el filtro de tamaño lo tira entero.
//!
//! O sea: cero insights, para siempre, sin un error en ninguna parte. Es el mismo
//! fallo que la V2 ya había parcheado para los trozos de PDF —excluyéndolos a
//! mano— sin ver que el problema no eran los PDF. Aquí se agrupa por CONTENIDO,
//! con la misma medida que usa el consolidador, y las etiquetas solo suman.

use std::collections::HashSet;

/// Cuántas memorias se miran por pasada.
///
/// El bucle es O(n²), igual que el del consolidador y con el mismo límite.
pub const MAX_INPUT: usize = 600;

/// Edad mínima de una memoria para entrar.
///
/// Cinco días. Un patrón sacado de lo aprendido esta mañana no es un patrón, es
/// la misma frase dicha dos veces: hace falta que las cosas hayan pasado en
/// momentos distintos para que lo común entre ellas signifique algo.
pub const MIN_EDAD_DIAS: i64 = 5;

/// Parecido de contenido para que dos memorias sean del mismo asunto.
///
/// MÁS BAJO QUE EL DE FUNDIR (0.35) A PROPÓSITO, y la diferencia es la razón de
/// ser del módulo: a 0.35 dos memorias dicen LO MISMO y el consolidador ya las
/// habría fundido; aquí se buscan las que hablan del mismo asunto SIN decir lo
/// mismo, que son las únicas entre las que puede haber un patrón que descubrir.
///
/// MEDIDO sobre diez memorias escritas como las escribe esta aplicación —cuatro
/// sobre certificados de WinRM, una de impresión, una de cocina y cuatro
/// lecciones de cristales de asuntos distintos—:
///
/// ```text
/// mismo asunto (las cuatro de WinRM entre sí)   0.208  0.217  0.261  0.318  0.318  0.429
/// mismo asunto, redactado corto (lección vs 3)  0.235
/// nada que ver (WinRM vs gazpacho)              0.050
/// nada que ver (WinRM vs discos, correo)        0.044  0.044  0.046  0.095
/// parecidas de refilón (impresora vs spooler)   0.071
/// ```
///
/// El suelo del ruido está en 0,10 y el asunto compartido empieza en 0,21. 0,15
/// cae en el hueco, y es un hueco ancho — no un número elegido para que salgan
/// los tests.
pub const MIN_PARECIDO: f32 = 0.15;

/// Tamaño de grupo con el que se puede generalizar.
///
/// Cuatro. Con tres, lo que sale es «estas dos veces pasó X», que no generaliza:
/// es una coincidencia con aspecto de regla.
pub const MIN_GRUPO: usize = 4;
/// Y el techo: un grupo de cuarenta memorias no comparte un patrón, comparte el
/// idioma. Lo que salga de ahí es una perogrullada.
pub const MAX_GRUPO: usize = 20;
/// Grupos por pasada. Cada uno cuesta una llamada al modelo local.
pub const MAX_GRUPOS: usize = 4;

/// Cuánto sube la confianza en cada refuerzo: `c += 0.10 * (1 - c)`.
///
/// Asintótica hacia 1 y nunca la alcanza, que es lo honesto: un patrón visto diez
/// veces es muy probable, y sigue sin ser una certeza.
pub const REFUERZO: f64 = 0.10;

#[derive(Debug, Clone, PartialEq)]
pub struct Insight {
    pub id: i64,
    pub contenido: String,
    pub huella: String,
    pub confianza: f64,
    pub refuerzos: i64,
    pub conceptos: Vec<String>,
    pub fuentes: i64,
    pub creado: i64,
    pub actualizado: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reporte {
    pub elegibles: usize,
    pub grupos: usize,
    pub creados: usize,
    pub reforzados: usize,
    /// Por qué no salió nada, cuando no salió nada. Un cero sin explicación es
    /// indistinguible de una avería.
    pub motivo: String,
}

// ── El formato ──────────────────────────────────────────────────────────────

/// Plano, y por lo mismo que el de los cristales: con etiquetas contenedoras un
/// modelo pequeño abre el plural y cierra el singular, y eso parsea a nada.
const SISTEMA: &str = "Eres el motor de reflexión de una asistente de administración de sistemas. \
Te dan varias observaciones relacionadas que la asistente ha ido acumulando, y buscas UN patrón \
general que se cumpla EN TODAS — una regla de andar por casa, una preferencia, una causa que se repite.

Devuelve EXACTAMENTE este XML, sin markdown y sin preámbulo:
<insight>
<content>Una sola frase, menos de 200 caracteres, con el patrón.</content>
<concept>palabra-clave</concept>
<concept>otra</concept>
</insight>

Reglas:
- REPITE <concept> por cada uno. NO uses <concepts>.
- Tiene que GENERALIZAR. Si las observaciones no comparten nada general, devuelve <content></content> vacío.
- De 1 a 4 conceptos, en minúsculas y abstractos: 'autenticación', no 'fallo-login-srv04'.
- Estilo afirmativo y en presente: \"Los errores de WinRM suelen venir de desfase de reloj.\"
- Escribe en el idioma de las observaciones.";

/// Largo mínimo del contenido para que cuente.
///
/// Por debajo de esto el modelo no ha generalizado: ha devuelto un trozo. La V2
/// exigía 20 BYTES con `len()`, que en español cuenta las tildes dobles — una
/// frase de dieciocho letras con dos acentos pasaba y una de veinte sin acentos
/// no. Aquí se cuenta en caracteres.
pub const MIN_CONTENIDO: usize = 25;
const MAX_CONTENIDO: usize = 300;
const MAX_CONCEPTOS: usize = 4;

// ── Agrupar ─────────────────────────────────────────────────────────────────

/// Una memoria preparada para comparar.
#[derive(Debug, Clone)]
pub struct Fila {
    pub id: i64,
    pub titulo: String,
    pub contenido: String,
    pub tags: Vec<String>,
}

struct Prep {
    palabras: HashSet<String>,
    tags: HashSet<String>,
}

/// Cuánto aportan las etiquetas como mucho: la mitad de lo que ya diga el
/// contenido.
const BONO_TAGS: f32 = 0.5;

/// Cuánto se parecen dos memorias, de 0 a 1.
///
/// EL CONTENIDO DECIDE Y LAS ETIQUETAS MULTIPLICAN — no suman. La diferencia
/// parece de estilo y no lo es: con un término sumado, dos memorias que comparten
/// todas sus etiquetas y NADA de contenido puntuaban `0 + 0.25` y entraban en el
/// mismo grupo por el bono solo. Escrito así se agrupaban la impresora atascada y
/// los certificados de WinRM, porque las dos llevan la etiqueta `auto` que les
/// pone la escritura automática; y las cuatro lecciones de un cristal volvían a
/// caer todas juntas, que es exactamente el fallo de la V2 que este módulo venía
/// a arreglar, reintroducido por la puerta de al lado.
///
/// Multiplicando, la mitad de nada sigue siendo nada.
fn parecido(a: &Prep, b: &Prep) -> f32 {
    let c = crate::consolidate::jaccard(&a.palabras, &b.palabras);
    if c <= 0.0 {
        return 0.0;
    }
    let t = crate::consolidate::jaccard(&a.tags, &b.tags);
    (c * (1.0 + t * BONO_TAGS)).min(1.0)
}

/// Agrupa por contigüidad: si A se parece a B y B a C, los tres van juntos aunque
/// A y C no se parezcan. Es lo correcto para un patrón —una cadena de casos
/// relacionados es exactamente donde vive— y por eso hay techo de tamaño: sin él,
/// la cadena acabaría tragándose el corpus entero.
pub fn agrupa(filas: &[Fila]) -> Vec<Vec<usize>> {
    let prep: Vec<Prep> = filas
        .iter()
        .map(|f| Prep {
            palabras: crate::consolidate::tokens(&format!("{} {}", f.titulo, f.contenido)),
            tags: f.tags.iter().map(|t| t.to_lowercase()).collect(),
        })
        .collect();
    let n = filas.len();
    let mut padre: Vec<usize> = (0..n).collect();
    fn raiz(p: &mut Vec<usize>, mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if parecido(&prep[i], &prep[j]) >= MIN_PARECIDO {
                let (ri, rj) = (raiz(&mut padre, i), raiz(&mut padre, j));
                if ri != rj {
                    padre[ri] = rj;
                }
            }
        }
    }
    let mut grupos: std::collections::HashMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        let r = raiz(&mut padre, i);
        grupos.entry(r).or_default().push(i);
    }
    let mut v: Vec<Vec<usize>> = grupos
        .into_values()
        .filter(|g| g.len() >= MIN_GRUPO && g.len() <= MAX_GRUPO)
        .collect();
    // Los más grandes primero: donde hay más casos, el patrón es más creíble.
    // Y con el id más bajo de desempate, para que dos pasadas seguidas sobre el
    // mismo corpus procesen los mismos grupos y refuercen en vez de barajar.
    v.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
    v.truncate(MAX_GRUPOS);
    v
}

// ── La huella ───────────────────────────────────────────────────────────────

/// Huella estable del contenido: minúsculas, espacios colapsados, FNV-1a.
///
/// La misma redacción en dos pasadas cae en la misma huella y REFUERZA en vez de
/// duplicar. Dos redacciones distintas del mismo patrón NO se encuentran y crean
/// dos filas — ruido aceptable: el índice ordena por confianza, así que el que se
/// repite sube y el que salió una vez se queda abajo.
///
/// Sin SHA. No es un uso criptográfico —nadie va a fabricar una colisión para
/// reforzar un insight ajeno— y meter `sha2` en el núcleo por esto sería pagar
/// una dependencia por una costumbre.
pub fn huella(contenido: &str) -> String {
    let norm = contenido
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in norm.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}")
}

// ── La base ─────────────────────────────────────────────────────────────────

pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_insights (
                 id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                 content             TEXT    NOT NULL,
                 fingerprint         TEXT    NOT NULL UNIQUE,
                 confidence          REAL    NOT NULL DEFAULT 0.5,
                 reinforcements      INTEGER NOT NULL DEFAULT 1,
                 concepts            TEXT    NOT NULL DEFAULT '[]',
                 source_count        INTEGER NOT NULL DEFAULT 0,
                 last_reinforced_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                 created_at          INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                 updated_at          INTEGER NOT NULL DEFAULT (strftime('%s','now'))
             );
             CREATE INDEX IF NOT EXISTS idx_agent_insights_confidence
                 ON agent_insights(confidence DESC, reinforcements DESC);
             CREATE INDEX IF NOT EXISTS idx_agent_insights_updated
                 ON agent_insights(updated_at DESC);",
        )
        .map_err(|e| format!("insights: esquema: {e}"))
    })
}

/// Las memorias que pueden entrar en una reflexión.
///
/// Fuera las supersedidas —ya no son verdad—, fuera los trozos de PDF —el texto
/// de un manual no es una observación de esta instalación, y después de una
/// ingesta serían el 99 % del corpus— y fuera las recientes.
pub fn elegibles() -> Result<Vec<Fila>, String> {
    crate::memories::ensure_schema()?;
    let corte = ahora() - MIN_EDAD_DIAS * 86_400;
    crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT id, title, content, tags FROM agent_memories
                 WHERE (superseded_by IS NULL OR superseded_by = '')
                   AND created_at < ?1
                   AND session_id NOT LIKE 'pdf:%'
                   AND session_id NOT LIKE 'pdf-doc:%'
                 ORDER BY created_at DESC LIMIT ?2",
            )
            .map_err(|e| format!("insights: elegibles: {e}"))?;
        let v = st
            .query_map(rusqlite::params![corte, MAX_INPUT as i64], |r| {
                Ok(Fila {
                    id: r.get(0)?,
                    titulo: r.get(1)?,
                    contenido: r.get(2)?,
                    tags: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
                })
            })
            .map_err(|e| format!("insights: elegibles: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })
}

fn ahora() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Lo que devolvió el modelo para un grupo.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Crudo {
    pub contenido: String,
    pub conceptos: Vec<String>,
}

/// Lee la respuesta. `None` = el modelo dijo que no había patrón, que es una
/// respuesta válida y no un fallo: no todo montón de notas esconde una regla.
pub fn parse(salida: &str) -> Option<Crudo> {
    let xml = crate::crystals::sin_pensamiento(salida);
    let xml = match xml.rfind("<insight>") {
        Some(i) => &xml[i..],
        None => xml,
    };
    let contenido = crate::crystals::recoge(xml, "content").into_iter().next()?;
    if contenido.chars().count() < MIN_CONTENIDO {
        return None;
    }
    let mut conceptos: Vec<String> = crate::crystals::recoge(xml, "concept")
        .into_iter()
        .map(|c| c.to_lowercase())
        .collect();
    conceptos.truncate(MAX_CONCEPTOS);
    Some(Crudo {
        contenido: crate::crystals::corta(&contenido, MAX_CONTENIDO),
        conceptos,
    })
}

/// El texto que se le enseña al modelo para un grupo.
pub fn prompt(filas: &[Fila], grupo: &[usize]) -> String {
    const TROZO: usize = 280;
    let mut s = String::from("Observaciones relacionadas:\n\n");
    for (n, &i) in grupo.iter().enumerate() {
        let f = &filas[i];
        s.push_str(&format!(
            "{}. {} — {}\n",
            n + 1,
            crate::crystals::corta(&f.titulo, 80),
            crate::crystals::corta(f.contenido.trim(), TROZO)
        ));
    }
    s
}

/// Un grupo, del prompt al patrón. `Ok(None)` = el modelo dijo que no hay
/// ninguno, que es una respuesta válida.
///
/// Aparte de `run` porque es la unidad que se puede pedir suelta: la vista de
/// Memoria querrá un «reflexiona sobre estas ahora» sin esperar al vencimiento, y
/// porque es lo único de aquí que se puede medir contra un Ollama de verdad.
pub fn destila_grupo(
    filas: &[Fila],
    grupo: &[usize],
    modelo: &str,
) -> Result<Option<Crudo>, String> {
    Ok(parse(&pregunta(&prompt(filas, grupo), modelo)?))
}

fn pregunta(texto: &str, modelo: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "model": modelo,
        "messages": [
            { "role": "system", "content": SISTEMA },
            { "role": "user", "content": texto },
        ],
        "stream": false,
        "options": { "temperature": 0.4, "num_predict": 800 },
    });
    let resp = ureq::post("http://localhost:11434/api/chat")
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(60))
        .send_string(&body.to_string())
        .map_err(|e| format!("Ollama no respondió: {e}"))?;
    let cuerpo = resp
        .into_string()
        .map_err(|e| format!("respuesta de Ollama ilegible: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&cuerpo).map_err(|e| format!("JSON inválido de Ollama: {e}"))?;
    if let Some(e) = json.get("error").and_then(|e| e.as_str()) {
        return Err(format!("Ollama: {e}"));
    }
    Ok(json
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string())
}

/// Guarda un patrón, o refuerza el que ya decía lo mismo.
///
/// Devuelve `true` si la fila es nueva.
pub fn guarda(c: &Crudo, fuentes: usize) -> Result<bool, String> {
    ensure_schema()?;
    // El contenido pasa por el filtro de secretos: sale de un modelo que acaba de
    // leer memorias, y aunque esas ya estén limpias, esta fila viaja después a
    // sitios distintos que ellas.
    let contenido = crate::memories::scrub(&c.contenido);
    let fp = huella(&contenido);
    let conceptos = serde_json::to_string(&c.conceptos).unwrap_or_else(|_| "[]".into());
    let now = ahora();
    crate::with_db(|conn| {
        let r = conn.execute(
            "INSERT INTO agent_insights
                 (content, fingerprint, confidence, reinforcements, concepts,
                  source_count, last_reinforced_at, created_at, updated_at)
             VALUES (?1, ?2, 0.5, 1, ?3, ?4, ?5, ?5, ?5)",
            rusqlite::params![contenido, fp, conceptos, fuentes as i64, now],
        );
        match r {
            Ok(_) => Ok(true),
            Err(rusqlite::Error::SqliteFailure(_, ref m))
                if m.as_deref().unwrap_or("").contains("UNIQUE") =>
            {
                conn.execute(
                    "UPDATE agent_insights SET
                         confidence = confidence + ?4 * (1.0 - confidence),
                         reinforcements = reinforcements + 1,
                         source_count = source_count + ?2,
                         last_reinforced_at = ?3,
                         updated_at = ?3
                     WHERE fingerprint = ?1",
                    rusqlite::params![fp, fuentes as i64, now, REFUERZO],
                )
                .map_err(|e| format!("insights: refuerzo: {e}"))?;
                Ok(false)
            }
            Err(e) => Err(format!("insights: alta: {e}")),
        }
    })
}

/// La pasada entera. Nunca es un error: esto corre solo, en segundo plano.
pub fn run(stop: &std::sync::atomic::AtomicBool) -> Reporte {
    use std::sync::atomic::Ordering;
    let filas = match elegibles() {
        Ok(f) => f,
        Err(e) => return Reporte { motivo: e, ..Default::default() },
    };
    let mut rep = Reporte { elegibles: filas.len(), ..Default::default() };
    if filas.len() < MIN_GRUPO {
        rep.motivo = format!(
            "{} memorias con más de {MIN_EDAD_DIAS} días (hacen falta {MIN_GRUPO})",
            filas.len()
        );
        return rep;
    }
    let grupos = agrupa(&filas);
    rep.grupos = grupos.len();
    if grupos.is_empty() {
        rep.motivo = format!(
            "ningún grupo de entre {MIN_GRUPO} y {MAX_GRUPO} memorias parecidas entre {} \
             elegibles",
            filas.len()
        );
        return rep;
    }
    let Some(modelo) = crate::crystals::modelo() else {
        rep.motivo = "no hay ningún modelo de texto en Ollama".into();
        return rep;
    };
    for g in &grupos {
        if stop.load(Ordering::Relaxed) {
            rep.motivo = "cancelado".into();
            return rep;
        }
        // Sin patrón es una respuesta válida: no todo montón de notas esconde una
        // regla, y forzar una sería inventarla. Un error de red sí para la pasada:
        // si Ollama no está, los grupos siguientes tampoco van a contestar.
        let c = match destila_grupo(&filas, g, &modelo) {
            Ok(Some(c)) => c,
            Ok(None) => continue,
            Err(e) => {
                rep.motivo = e;
                return rep;
            }
        };
        match guarda(&c, g.len()) {
            Ok(true) => rep.creados += 1,
            Ok(false) => rep.reforzados += 1,
            Err(e) => rep.motivo = e,
        }
    }
    if rep.motivo.is_empty() && rep.creados == 0 && rep.reforzados == 0 {
        rep.motivo = format!("{} grupos mirados, ninguno con un patrón general", rep.grupos);
    }
    rep
}

/// Los insights, el más confiable y más repetido primero.
pub fn list(limite: usize) -> Result<Vec<Insight>, String> {
    ensure_schema()?;
    let lim = limite.clamp(1, 200) as i64;
    crate::with_db(|c| {
        let mut st = c
            .prepare(
                "SELECT id, content, fingerprint, confidence, reinforcements, concepts,
                        source_count, created_at, updated_at
                 FROM agent_insights
                 ORDER BY confidence DESC, reinforcements DESC LIMIT ?1",
            )
            .map_err(|e| format!("insights: listar: {e}"))?;
        let v = st
            .query_map([lim], |r| {
                Ok(Insight {
                    id: r.get(0)?,
                    contenido: r.get(1)?,
                    huella: r.get(2)?,
                    confianza: r.get(3)?,
                    refuerzos: r.get(4)?,
                    conceptos: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                    fuentes: r.get(6)?,
                    creado: r.get(7)?,
                    actualizado: r.get(8)?,
                })
            })
            .map_err(|e| format!("insights: listar: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })
}

pub fn delete(id: i64) -> Result<(), String> {
    crate::with_db(|c| {
        c.execute("DELETE FROM agent_insights WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("insights: borrar: {e}"))?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fila(id: i64, titulo: &str, contenido: &str, tags: &[&str]) -> Fila {
        Fila {
            id,
            titulo: titulo.into(),
            contenido: contenido.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    /// Seis memorias: cuatro sobre certificados y WinRM, dos de otra cosa.
    fn corpus() -> Vec<Fila> {
        vec![
            fila(
                1,
                "WinRM no conecta con SRV-01",
                "El certificado de WinRM del servidor SRV-01 había caducado y el cliente \
                 rechazaba la conexión hasta renovarlo en LocalMachine",
                &["auto"],
            ),
            fila(
                2,
                "Certificado caducado en SRV-04",
                "WinRM dejó de aceptar conexiones porque el certificado LocalMachine del \
                 servidor SRV-04 estaba caducado desde marzo",
                &["auto"],
            ),
            fila(
                3,
                "Renovar certificado WinRM",
                "Para que WinRM vuelva a conectar hay que renovar el certificado en el almacén \
                 LocalMachine del servidor y reiniciar el servicio",
                &["auto"],
            ),
            fila(
                4,
                "SRV-07 rechaza WinRM",
                "El servidor SRV-07 rechazaba WinRM; el certificado del almacén LocalMachine \
                 llevaba caducado dos semanas y renovarlo lo arregló",
                &["auto"],
            ),
            fila(5, "Impresora atascada", "La cola de impresión tenía cuatrocientos trabajos", &["auto"]),
            fila(6, "Gazpacho", "Receta de gazpacho andaluz con tomate y pepino", &["cocina"]),
        ]
    }

    #[test]
    fn agrupa_lo_que_habla_de_lo_mismo_y_deja_fuera_lo_demas() {
        let c = corpus();
        let g = agrupa(&c);
        assert_eq!(g.len(), 1, "debería salir un grupo: {g:?}");
        assert_eq!(g[0].len(), 4, "las cuatro de WinRM y ninguna más");
        let ids: HashSet<i64> = g[0].iter().map(|&i| c[i].id).collect();
        assert_eq!(ids, [1, 2, 3, 4].into_iter().collect());
    }

    #[test]
    fn una_sola_etiqueta_ya_no_impide_agrupar() {
        // EL FALLO DE LA V2. Allí hacían falta DOS etiquetas compartidas, y las
        // memorias automáticas de esta aplicación llevan una: `auto`. Nunca se
        // agrupaban, y son la fuente principal del corpus.
        let c = corpus();
        assert!(c[0].tags.len() == 1 && c[0].tags[0] == "auto");
        assert!(!agrupa(&c).is_empty(), "con una etiqueta también hay que poder agrupar");
    }

    #[test]
    fn compartir_etiqueta_no_basta_para_agrupar() {
        // El otro lado del mismo fallo: las lecciones de un cristal llevan todas
        // `crystal`+`leccion`, así que con la regla de la V2 caían TODAS en el
        // mismo grupo hablaran de lo que hablaran.
        let v = vec![
            fila(1, "Certificados", "Renovar el certificado de WinRM antes de que caduque", &["crystal", "leccion"]),
            fila(2, "Impresión", "Vaciar la carpeta PRINTERS exige parar el spooler antes", &["crystal", "leccion"]),
            fila(3, "Discos", "Un disco por encima del noventa por ciento tira el servicio", &["crystal", "leccion"]),
            fila(4, "Correo", "El relay SMTP rechaza los mensajes sin registro SPF", &["crystal", "leccion"]),
        ];
        assert!(
            agrupa(&v).is_empty(),
            "cuatro lecciones de asuntos distintos no son un grupo"
        );
    }

    #[test]
    fn las_etiquetas_no_pueden_agrupar_por_si_solas() {
        // ESTO ME FALLÓ AL ESCRIBIRLO. Con el bono de etiquetas SUMADO, dos
        // memorias que comparten todas sus etiquetas y nada de contenido
        // puntuaban 0 + 0.25 y entraban juntas: la impresora atascada acababa en
        // el grupo de los certificados de WinRM porque las dos llevan `auto`.
        // Multiplicando, la mitad de nada sigue siendo nada.
        let v = vec![
            fila(1, "Impresora", "La cola de impresión tenía cuatrocientos trabajos", &["auto"]),
            fila(2, "Gazpacho", "Tomate pepino pimiento ajo y aceite", &["auto"]),
            fila(3, "Disco", "El volumen D llegó al noventa por ciento", &["auto"]),
            fila(4, "Correo", "El relay rechaza mensajes sin registro SPF", &["auto"]),
        ];
        assert!(
            agrupa(&v).is_empty(),
            "compartir la etiqueta `auto` no es hablar del mismo asunto"
        );
    }

    #[test]
    fn un_grupo_de_tres_no_generaliza() {
        // Con tres, lo que sale es «estas veces pasó X»: una coincidencia con
        // aspecto de regla.
        let mut c = corpus();
        c.truncate(3);
        assert!(agrupa(&c).is_empty());
    }

    #[test]
    fn el_umbral_esta_por_debajo_del_de_fundir() {
        // La diferencia es la razón de ser del módulo: a 0.35 dos memorias dicen
        // lo mismo y el consolidador ya las habría fundido. Aquí se buscan las que
        // hablan del mismo asunto sin decir lo mismo.
        assert!(MIN_PARECIDO < crate::consolidate::MIN_CONTENT_JACCARD);
    }

    #[test]
    fn la_misma_frase_da_la_misma_huella_aunque_cambie_el_formato() {
        assert_eq!(
            huella("Los errores de WinRM suelen ser el reloj."),
            huella("  los ERRORES  de winrm\n suelen ser el reloj.  ")
        );
        assert_ne!(huella("una cosa"), huella("otra cosa"));
    }

    #[test]
    fn un_patron_demasiado_corto_no_es_un_patron() {
        // El modelo devolvió un trozo, no una generalización.
        assert!(parse("<insight><content>Es el reloj</content></insight>").is_none());
        // Y el vacío es una respuesta válida: no todo montón esconde una regla.
        assert!(parse("<insight><content></content></insight>").is_none());
    }

    #[test]
    fn el_minimo_se_cuenta_en_caracteres_y_no_en_bytes() {
        // La V2 usaba `len()`, que en español cuenta las tildes dobles: una frase
        // de dieciocho letras con dos acentos pasaba y una de veinte sin acentos
        // no. Veinticinco caracteres justos con acentos tienen que contar como 25.
        let frase = "áéíóú áéíóú áéíóú áéíóúab"; // 25 caracteres, 35 bytes
        assert_eq!(frase.chars().count(), MIN_CONTENIDO);
        let xml = format!("<insight><content>{frase}</content></insight>");
        assert!(parse(&xml).is_some());
    }

    #[test]
    fn el_ensayo_de_un_modelo_pensante_tampoco_cuela_aqui() {
        let salida = "<think>pondría <content>Un borrador larguísimo que no vale nada</content>\
                      </think><insight><content>Los errores de WinRM suelen venir del reloj.\
                      </content><concept>WinRM</concept></insight>";
        let c = parse(salida).expect("debe parsear");
        assert!(c.contenido.starts_with("Los errores"));
        // Y los conceptos se guardan en minúsculas, que es como se comparan.
        assert_eq!(c.conceptos, vec!["winrm".to_string()]);
    }

    #[test]
    fn el_prompt_lleva_las_memorias_numeradas_y_recortadas() {
        let c = corpus();
        let p = prompt(&c, &[0, 1]);
        assert!(p.contains("1. WinRM no conecta con SRV-01"));
        assert!(p.contains("2. Certificado caducado en SRV-04"));
        assert!(!p.contains("3."));
    }
}
