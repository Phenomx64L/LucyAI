//! El vigilante: qué se mira, y sobre todo qué se calla.
//!
//! TRES CAPAS, Y EL MODELO NO TOCA NINGUNA DE LAS DOS PRIMERAS:
//!
//! ```text
//!   1. observar   de una medición sale un síntoma      Rust puro
//!   2. decidir    de un síntoma sale (o no) un aviso   Rust puro
//!   3. redactar   de un aviso sale una frase           ahí sí el modelo
//! ```
//!
//! Este fichero es 1 y 2. Que estén en Rust y no en un modelo no es una
//! preferencia de estilo: es lo que hace que «sin alucinaciones» sea una
//! GARANTÍA ESTRUCTURAL y no una esperanza. Una cifra que sale de
//! `SysSnapshot` no puede estar inventada, y una decisión tomada por un `match`
//! no puede tener un mal día.
//!
//! ── DÓNDE ESTÁ EL RIESGO DE VERDAD ──────────────────────────────────────────
//!
//! No en detectar. Detectar un disco lleno es una resta. El riesgo está en la
//! capa 2, y es este: UN VIGILANTE QUE AVISA DE MÁS SE SILENCIA, y entonces la
//! función está muerta — peor que no haberla hecho, porque el operador cree que
//! le avisarían y ya no le avisa nadie.
//!
//! Es el mismo argumento que `thresholds` escribe sobre el servidor de
//! compilación al 90 % pintado en rojo todas las tardes: «un panel que lo pinta
//! en rojo todas las tardes enseña a no mirarlo». Aquí es más grave, porque un
//! panel se ignora mirándolo y un aviso se ignora apagándolo.
//!
//! Por eso las reglas de abajo son casi todas reglas para CALLARSE, y por eso
//! `decide` es una función pura: la política es lo que hay que poder discutir y
//! probar entera, sin reloj y sin base de datos.

use crate::system::{DownService, SysSnapshot};
use crate::thresholds::{Nivel, Umbrales};

/// Algo que se ha observado y que PODRÍA merecer un aviso.
///
/// No es un aviso todavía. La capa 1 produce síntomas de todo lo que mira,
/// incluidos los que están en `Ok` — que hacen falta para saber que algo ha
/// dejado de estar mal, y sin ellos la recuperación sería invisible.
#[derive(Debug, Clone, PartialEq)]
pub struct Sintoma {
    /// Qué cosa es, de forma estable. `disco:C:` y no «disco al 94 %»: la
    /// segunda cambia con cada medida y entonces todo síntoma sería nuevo, que
    /// es la manera más rápida de convertir esto en un grifo abierto.
    pub clave: String,
    pub nivel: Nivel,
    /// De QUÉ habla, independiente de cómo esté. «Disco D:\», «Servicios
    /// automáticos», «CPU».
    ///
    /// EXISTE PORQUE LA RECUPERACIÓN NO SE PUEDE REDACTAR CON EL TÍTULO MALO.
    /// Antes se componía bajando el título a minúsculas, y salían cosas así:
    ///
    /// ```text
    ///   Resuelto: 0 servicios automáticos han fallado
    ///   Resuelto: disco d:\ casi lleno
    /// ```
    ///
    /// La primera porque el título se arma con `rotos.len()`, que en la
    /// recuperación vale cero; la segunda porque `to_lowercase` se lleva por
    /// delante la letra de unidad, que es justo lo que el operador usa para
    /// saber de qué disco le hablan.
    ///
    /// Y no era un caso raro: `servicios` es la ÚNICA clave que llega de
    /// Critico a Ok en una sola pasada —cpu, memoria y disco tienen que
    /// atravesar la banda de aviso entera—, así que la forma DOMINANTE del
    /// aviso de recuperación era la rota.
    pub asunto: String,
    pub titulo: String,
    pub cuerpo: String,
    /// Cómo se dice que esto ya está bien. Se usa solo al recuperarse.
    pub cuerpo_ok: String,
    /// Vacío = este equipo.
    pub equipo: String,
}

/// Lo que se sabe de una clave: cuándo se avisó por última vez, con qué nivel, y
/// cuántas veces se ha avisado de ella en las últimas horas.
///
/// El tercero es lo que permite que el freno se espacie solo. Ver `freno`.
pub type Antes = std::collections::HashMap<String, (i64, Nivel, usize)>;

/// Nunca dos avisos de la MISMA clave más cerca que esto, pase lo que pase.
///
/// EL FRENO CONTRA EL PARPADEO. Una CPU que cruza el 78 % arriba y abajo cada
/// dos minutos generaría un aviso por cruce, y las reglas de «empeoró» no lo
/// impedirían porque cada subida es de verdad un empeoramiento. Quince minutos
/// es más que el ciclo de cualquier pico normal y menos que el tiempo en que
/// algo se pone serio de verdad.
pub const MIN_ENTRE_AVISOS: i64 = 15 * 60;

/// Cuántos avisos seguidos de la misma clave se toleran antes de empezar a
/// espaciarlos.
///
/// EL FRENO FIJO NO BASTA CONTRA ALGO QUE PARPADEA, y esto se midió. Una CPU que
/// alterna crítico y tranquilo cada cinco minutos produce, con solo el freno de
/// quince minutos, una pareja «alarma + resuelto» cada cuarto de hora: unos
/// noventa y seis globos al día, todos legítimos según las reglas. Cada uno es
/// de verdad un cambio de estado; el problema es que el estado cambia sin parar.
///
/// Así que el freno se DUPLICA con cada aviso a partir del tercero. Un problema
/// de verdad —que se dice una vez y se arregla— no llega a notarlo; uno que
/// parpadea se va espaciando solo hasta callarse. Es lo que hace cualquier
/// sistema de monitorización serio, y por lo mismo.
pub const ANTES_DE_ESPACIAR: usize = 2;

/// Cuántas veces se puede duplicar el freno. `15 min · 2^5` son ocho horas.
pub const TOPE_ESPACIADO: u32 = 5;

/// En cuánto tiempo se cuentan los avisos recientes de una clave.
///
/// Seis horas. Más corto dejaría que el parpadeo se reiniciara en cuanto el
/// operador se fuera a comer; más largo castigaría un problema de la mañana con
/// el silencio de la tarde.
pub const VENTANA_RECIENTES: i64 = 6 * 3_600;

/// El freno que le toca a una clave según cuántas veces se ha avisado de ella.
///
/// Pura y separada para poder discutir la curva sin base de datos delante.
pub fn freno(recientes: usize) -> i64 {
    let n = recientes.saturating_sub(ANTES_DE_ESPACIAR).min(TOPE_ESPACIADO as usize) as u32;
    MIN_ENTRE_AVISOS.saturating_mul(1_i64 << n)
}

/// Cada cuánto se repite algo que sigue crítico.
///
/// Cuatro horas. Lo bastante para que no se olvide, lo bastante poco para que no
/// se convierta en el ruido de fondo que se aprende a cerrar sin leer.
pub const REPITE_CRITICO: i64 = 4 * 3_600;

/// Cada cuánto se repite algo que sigue en aviso.
///
/// Un día. Un disco al 87 % no es una urgencia: es un recordatorio, y un
/// recordatorio diario ya es más de lo que la mayoría de la gente quiere.
pub const REPITE_AVISO: i64 = 24 * 3_600;

/// Cuántos avisos como mucho salen de una sola pasada.
///
/// EL CASO QUE ARRUINA UN VIGILANTE EL PRIMER DÍA. Lucy estuvo cerrada una
/// semana, se abre, y cuarenta cosas han cambiado: cuarenta globos seguidos. El
/// operador no lee ninguno y apaga las notificaciones para siempre.
///
/// Tres, y los más graves primero. Los demás NO se pierden — quedan en el
/// registro de `notify`, que es justo para lo que está.
pub const MAX_POR_PASADA: usize = 3;

/// Qué hacer con un síntoma.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Avisar, y por qué se ha decidido avisar.
    Avisa(Sintoma, Motivo),
    /// Callarse, y por qué.
    Calla(Motivo),
}

/// Por qué se dijo o no se dijo. Se guarda para poder discutir la política con
/// datos en vez de con impresiones: «me avisa demasiado» es una queja que sin
/// esto no se puede investigar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motivo {
    /// Primera vez que esta clave cruza un umbral.
    Nuevo,
    /// Estaba mal y está peor.
    Empeora,
    /// Sigue igual, pero ha pasado el plazo de repetición.
    Recordatorio,
    /// Estaba crítico y ha vuelto a la normalidad.
    Recuperado,
    /// Está bien y no había nada que decir.
    NadaQueDecir,
    /// Sigue igual y no toca repetirlo.
    YaLoDije,
    /// Demasiado pronto desde el anterior, sea cual sea el motivo.
    DemasiadoPronto,
    /// Cabía, pero ya salían tres más graves en esta pasada.
    NoCabeEnLaPasada,
}

impl Motivo {
    pub fn avisa(self) -> bool {
        matches!(self, Self::Nuevo | Self::Empeora | Self::Recordatorio | Self::Recuperado)
    }
}

/// La política, entera y sin efectos.
///
/// PURA A PROPÓSITO: sin reloj, sin base de datos, sin red. Es lo único de esta
/// función que garantiza que se pueda probar de verdad — y de las ocho ramas que
/// tiene, seis son para callarse. Eso no es casualidad: es el diseño.
///
/// `antes` es lo último que se avisó de cada clave. `ahora` es epoch en
/// segundos.
pub fn decide(sintomas: Vec<Sintoma>, antes: &Antes, ahora: i64) -> Vec<Decision> {
    // LOS MÁS GRAVES PRIMERO, porque el tope de la pasada corta por abajo. Si el
    // orden fuera el de medición, un disco crítico podría quedarse fuera por
    // tres avisos de CPU que llegaron antes.
    let mut orden: Vec<Sintoma> = sintomas;
    orden.sort_by_key(|s| std::cmp::Reverse(s.nivel));

    let mut fuera = Vec::new();
    let mut dichos = 0;
    for s in orden {
        let previo = antes.get(&s.clave).copied();
        let motivo = juzga(&s, previo, ahora);
        if !motivo.avisa() {
            fuera.push(Decision::Calla(motivo));
            continue;
        }
        if dichos >= MAX_POR_PASADA {
            fuera.push(Decision::Calla(Motivo::NoCabeEnLaPasada));
            continue;
        }
        dichos += 1;
        fuera.push(Decision::Avisa(s, motivo));
    }
    fuera
}

/// El juicio de UN síntoma. Separado para que las ramas se lean de una vez.
fn juzga(s: &Sintoma, previo: Option<(i64, Nivel, usize)>, ahora: i64) -> Motivo {
    let Some((ts, nivel_antes, recientes)) = previo else {
        // Nunca se dijo nada de esto. Solo es noticia si está mal.
        return if s.nivel == Nivel::Ok { Motivo::NadaQueDecir } else { Motivo::Nuevo };
    };
    let desde = ahora.saturating_sub(ts);

    // EL FRENO DEL PARPADEO VA ANTES QUE TODO LO DEMÁS, y esto incluye la
    // recuperación. Aquí la recuperación iba PRIMERO, con el argumento de que
    // callar el «ya está» deja al operador con la alarma en la cabeza. El
    // argumento vale para una recuperación; para una serie no.
    //
    // LO QUE PRODUCÍA, sobre un servidor de compilación con los umbrales de
    // fábrica y muestreo cada cinco minutos:
    //
    // ```text
    //   10:00  CPU 95 %  Nuevo        toast
    //   10:05  CPU 40 %  Recuperado   toast   <- se saltaba el freno
    //   10:10  CPU 96 %  DemasiadoPronto
    //   10:20  CPU 93 %  Empeora      toast
    //   10:25  CPU 50 %  Recuperado   toast
    // ```
    //
    // Dos globos cada veinte minutos indefinidamente: unos ciento cuarenta al
    // día alternando «CPU alta» y «Resuelto: cpu alta» sin que nadie tenga nada
    // que hacer. Y el mismo patrón lo produce cualquier servicio en bucle de
    // caída —el SCM lo reinicia, la pasada siguiente lo ve sano, la otra
    // caído—, o sea que el caso donde más falta hace UN aviso era el que
    // generaba diez.
    //
    // Con el freno delante, un problema que se resuelve en menos de quince
    // minutos no genera el «ya está», y eso resulta ser lo correcto: un pico que
    // se arregla solo en cinco minutos no merece dos notificaciones, merece
    // cero.
    // EL FRENO SE ESPACIA SOLO. Con uno fijo, algo que parpadea produce una
    // pareja «alarma + resuelto» cada cuarto de hora indefinidamente. Ver
    // `ANTES_DE_ESPACIAR`.
    if desde < freno(recientes) {
        return Motivo::DemasiadoPronto;
    }

    // LA RECUPERACIÓN, Y SOLO DE LO QUE FUE CRÍTICO. Si algo estuvo grave y ya
    // no lo está, hay que decirlo: sin esto el operador se queda sin saber si
    // aquello sigue pasando, y acaba yendo a mirar — que es exactamente el
    // trabajo que el vigilante venía a ahorrar.
    //
    // De un aviso que se resuelve NO se avisa: «el disco ya no está al 87 %» no
    // es una noticia, es ruido con buenas intenciones.
    if s.nivel == Nivel::Ok {
        return if nivel_antes == Nivel::Critico { Motivo::Recuperado } else { Motivo::NadaQueDecir };
    }

    if s.nivel > nivel_antes {
        return Motivo::Empeora;
    }

    // Sigue igual (o ha mejorado sin llegar a Ok). El plazo lo pone el nivel en
    // el que está AHORA.
    let plazo = if s.nivel == Nivel::Critico { REPITE_CRITICO } else { REPITE_AVISO };
    if desde >= plazo {
        Motivo::Recordatorio
    } else {
        Motivo::YaLoDije
    }
}

// ── Capa 1: mirar ───────────────────────────────────────────────────────────

/// Los síntomas de este equipo, de una medición.
///
/// DEVUELVE TAMBIÉN LO QUE ESTÁ BIEN, y hace falta: sin un síntoma en `Ok` para
/// el disco C:, `decide` no puede saber que aquel disco crítico de ayer ya no lo
/// está, y la recuperación no se avisaría nunca.
pub fn observa_local(s: &SysSnapshot, servicios: &[DownService], u: &Umbrales) -> Vec<Sintoma> {
    let mut v = Vec::new();

    v.push(Sintoma {
        clave: "cpu".into(),
        nivel: u.cpu(s.cpu_pct),
        asunto: "CPU".into(),
        titulo: "CPU alta".into(),
        cuerpo: format!("La CPU va al {:.0} %.", s.cpu_pct),
        cuerpo_ok: format!("La CPU ha bajado al {:.0} %.", s.cpu_pct),
        equipo: String::new(),
    });

    if s.mem_total > 0 {
        let pct = s.mem_used as f32 / s.mem_total as f32 * 100.0;
        v.push(Sintoma {
            clave: "memoria".into(),
            nivel: u.mem(pct),
            asunto: "Memoria".into(),
            titulo: "Memoria alta".into(),
            cuerpo: format!(
                "La memoria va al {pct:.0} % — {:.1} de {:.1} GB.",
                s.mem_used as f64 / 1e9,
                s.mem_total as f64 / 1e9
            ),
            cuerpo_ok: format!("La memoria ha bajado al {pct:.0} %."),
            equipo: String::new(),
        });
    }

    for d in &s.disks {
        if d.total == 0 {
            continue;
        }
        let usado = d.total.saturating_sub(d.avail);
        let pct = usado as f32 / d.total as f32 * 100.0;
        v.push(Sintoma {
            // El punto de montaje y no el nombre: el nombre de un volumen se
            // puede cambiar desde el explorador, y entonces la clave cambiaría y
            // el disco volvería a ser «nuevo» para el vigilante.
            clave: format!("disco:{}", d.mount),
            nivel: u.disco(pct),
            asunto: format!("Disco {}", d.mount),
            titulo: format!("Disco {} casi lleno", d.mount),
            cuerpo: format!(
                "{} al {pct:.0} % — quedan {:.1} GB de {:.1}.",
                d.mount,
                d.avail as f64 / 1e9,
                d.total as f64 / 1e9
            ),
            cuerpo_ok: format!(
                "{} al {pct:.0} %, con {:.1} GB libres.",
                d.mount,
                d.avail as f64 / 1e9
            ),
            equipo: String::new(),
        });
    }

    // SOLO LOS QUE FALLARON, no los que están parados. Un servicio automático
    // detenido limpiamente es de lo más normal —`sppsvc` se para solo para
    // ahorrar recursos— y avisar de eso es la primera forma de que alguien
    // apague los avisos. La distinción ya la hace `DownService::crashed`.
    let rotos: Vec<&str> = servicios.iter().filter(|x| x.crashed()).map(|x| x.name.as_str()).collect();
    v.push(Sintoma {
        clave: "servicios".into(),
        nivel: if rotos.is_empty() { Nivel::Ok } else { Nivel::Critico },
        asunto: "Servicios automáticos".into(),
        // El plural se decide con `rotos.len() == 1`, y con la lista vacía cae
        // en la rama del plural: por eso el título del estado malo NUNCA sirve
        // para redactar la recuperación. Ver `Sintoma::asunto`.
        titulo: if rotos.len() == 1 {
            "Un servicio automático ha fallado".into()
        } else {
            format!("{} servicios automáticos han fallado", rotos.len())
        },
        cuerpo: if rotos.is_empty() { String::new() } else { rotos.join(", ") },
        cuerpo_ok: "Ya no hay ningún servicio automático fallado.".into(),
        equipo: String::new(),
    });

    v
}

// ── El puente con el canal ──────────────────────────────────────────────────

/// Lo que este proceso ha dicho desde que arrancó.
///
/// EL FRENO NO PUEDE VIVIR SOLO EN LA BASE, y el caso que lo demuestra es el
/// peor posible: el disco se llena. SQLite en WAL necesita crecer su fichero
/// para insertar, devuelve `SQLITE_FULL`, y `notify::anota` falla. La fila del
/// aviso no se escribe.
///
/// A partir de ahí `ultimo_de` sigue devolviendo la fila VIEJA —la del lunes,
/// con nivel Aviso— así que cada pasada compara «ahora Critico» contra «entonces
/// Aviso», concluye `Empeora`, y manda otro globo. El plazo no frena porque
/// `desde` son días. Con muestreo de un minuto son mil cuatrocientos avisos al
/// día, todos legítimos según la política, y ninguno de los tres frenos actúa
/// porque los tres leen la fila que no se pudo escribir.
///
/// O sea: en la emergencia para la que existe el vigilante, el vigilante se
/// convierte en el problema. Y el carril de trace mostraría mil cuatrocientos
/// `Empeora` impecables sin una pista de que la causa es un INSERT fallido.
///
/// Con esta memoria de proceso, el freno sigue funcionando aunque el disco no
/// admita una letra más. Se pierde al cerrar Lucy, que es aceptable: al abrirla
/// otra vez, un aviso es razonable.
fn dicho_en_esta_sesion() -> &'static std::sync::Mutex<Antes> {
    static M: std::sync::OnceLock<std::sync::Mutex<Antes>> = std::sync::OnceLock::new();
    M.get_or_init(|| std::sync::Mutex::new(Antes::new()))
}

/// Lee del registro de avisos qué se dijo por última vez de cada clave.
///
/// Manda LA MÁS RECIENTE de las dos fuentes: la fila en disco y lo que este
/// proceso recuerda haber dicho. Ver `dicho_en_esta_sesion` para por qué hacen
/// falta las dos.
pub fn lo_ya_dicho(claves: &[String]) -> Antes {
    let sesion = dicho_en_esta_sesion().lock().ok().map(|g| g.clone()).unwrap_or_default();
    let ahora = crate::notify::ahora_epoch();
    let mut m = Antes::new();
    for c in claves {
        let en_disco = crate::notify::ultimo_de(c);
        let en_sesion = sesion.get(c).copied();
        // El recuento sale de la base porque tiene que sobrevivir a cerrar
        // Lucy: un parpadeo que llevaba toda la mañana no debe reiniciar su
        // espaciado solo porque alguien reinició el programa.
        let recientes = crate::notify::cuantos_de(c, ahora - VENTANA_RECIENTES);
        let elegido = match (en_disco, en_sesion) {
            (Some((td, nd)), Some((ts, ns, rs))) => {
                if ts >= td { Some((ts, ns, rs.max(recientes))) } else { Some((td, nd, recientes)) }
            }
            (Some((td, nd)), None) => Some((td, nd, recientes)),
            (None, y) => y,
        };
        if let Some(x) = elegido {
            m.insert(c.clone(), x);
        }
    }
    m
}

/// Apunta en la memoria de proceso que se acaba de decir esto.
fn recuerda_que_lo_dije(clave: &str, ts: i64, nivel: Nivel) {
    if let Ok(mut g) = dicho_en_esta_sesion().lock() {
        let antes = g.get(clave).map(|x| x.2).unwrap_or(0);
        g.insert(clave.to_string(), (ts, nivel, antes + 1));
    }
}

/// Olvida lo dicho en esta sesión. Para los tests, que comparten proceso.
#[doc(hidden)]
pub fn olvida_la_sesion() {
    if let Ok(mut g) = dicho_en_esta_sesion().lock() {
        g.clear();
    }
}

/// Una pasada entera: mira, decide y manda lo que toque.
///
/// Devuelve TODAS las decisiones, incluidas las de callarse: quien la llama las
/// escribe en el carril de trace, que es donde se investiga un «me avisa
/// demasiado» sin tener que adivinar.
pub fn pasada(s: &SysSnapshot, servicios: &[DownService], ahora: i64) -> Vec<Decision> {
    let u = crate::thresholds::de("");
    let sintomas = observa_local(s, servicios, &u);
    let claves: Vec<String> = sintomas.iter().map(|x| x.clave.clone()).collect();
    let decisiones = decide(sintomas, &lo_ya_dicho(&claves), ahora);
    for d in &decisiones {
        if let Decision::Avisa(s, m) = d {
            let a = aviso_de(s, *m);
            crate::notify::envia(&a);
            // SE APUNTA AQUÍ Y NO SOLO EN LA BASE. Si el INSERT falló —disco
            // lleno, que es justo cuando esto importa— la fila no existe y el
            // freno de la pasada siguiente leería la de hace días. Ver
            // `dicho_en_esta_sesion`.
            recuerda_que_lo_dije(&s.clave, ahora, a.nivel);
        }
    }
    decisiones
}

/// El aviso que sale de un síntoma.
///
/// LA REDACCIÓN DE AHORA ES UNA PLANTILLA, y es donde entrará el modelo en el
/// paso siguiente. Lo que no va a cambiar es de dónde salen las cifras: de la
/// medición, no de lo que el modelo recuerde. El modelo redactará ESTO, no lo
/// averiguará.
pub fn aviso_de(s: &Sintoma, m: Motivo) -> crate::notify::Aviso {
    let (titulo, cuerpo) = match m {
        // La recuperación se redacta aparte: con el título del síntoma diría
        // «Disco C: casi lleno» justo cuando ha dejado de estarlo.
        // La recuperación se redacta desde el ASUNTO y con su propio cuerpo, no
        // rebajando a minúsculas un título escrito para el estado malo. Ver
        // `Sintoma::asunto` para lo que salía cuando se hacía así.
        Motivo::Recuperado => (
            format!("Resuelto: {}", s.asunto),
            if s.cuerpo_ok.trim().is_empty() {
                "Ha vuelto a la normalidad.".to_string()
            } else {
                s.cuerpo_ok.clone()
            },
        ),
        _ => (s.titulo.clone(), s.cuerpo.clone()),
    };
    crate::notify::Aviso::nuevo(titulo, cuerpo)
        .con_nivel(if m == Motivo::Recuperado { Nivel::Ok } else { s.nivel })
        .con_clave(&s.clave)
        .en_equipo(&s.equipo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sint(clave: &str, n: Nivel) -> Sintoma {
        Sintoma {
            clave: clave.into(),
            nivel: n,
            asunto: format!("asunto de {clave}"),
            titulo: format!("algo con {clave}"),
            cuerpo: "detalle".into(),
            cuerpo_ok: "ya está bien".into(),
            equipo: String::new(),
        }
    }

    const AHORA: i64 = 1_800_000_000;

    /// El previo de un test, con cero avisos recientes: el caso normal, donde
    /// el freno todavía no se ha espaciado.
    fn juicio(s: &Sintoma, previo: Option<(i64, Nivel)>) -> Motivo {
        juzga(s, previo.map(|(t, n)| (t, n, 0)), AHORA)
    }

    #[test]
    fn lo_que_esta_bien_y_nunca_estuvo_mal_no_se_dice() {
        // La rama que más veces corre de todas: casi siempre está todo bien.
        assert_eq!(juicio(&sint("cpu", Nivel::Ok), None), Motivo::NadaQueDecir);
    }

    #[test]
    fn la_primera_vez_que_algo_cruza_se_dice() {
        assert_eq!(juicio(&sint("cpu", Nivel::Aviso), None), Motivo::Nuevo);
        assert_eq!(juicio(&sint("cpu", Nivel::Critico), None), Motivo::Nuevo);
    }

    #[test]
    fn lo_mismo_otra_vez_se_calla() {
        // El corazón de la política. Un disco al 87 % medido cada cinco minutos
        // son doscientos ochenta y ocho avisos al día si nadie lo impide.
        let previo = Some((AHORA - 3_600, Nivel::Aviso));
        assert_eq!(juicio(&sint("disco:C:", Nivel::Aviso), previo), Motivo::YaLoDije);
    }

    #[test]
    fn empeorar_si_es_noticia() {
        let previo = Some((AHORA - 3_600, Nivel::Aviso));
        assert_eq!(juicio(&sint("disco:C:", Nivel::Critico), previo), Motivo::Empeora);
    }

    #[test]
    fn el_parpadeo_no_pasa_ni_empeorando() {
        // Un valor que baila alrededor del umbral produce empeoramientos DE
        // VERDAD cada pocos minutos. Sin este corte, cada uno sería un globo — y
        // «empeora» es precisamente la regla que no lo impediría por su cuenta.
        let previo = Some((AHORA - 60, Nivel::Aviso));
        assert_eq!(juicio(&sint("cpu", Nivel::Critico), previo), Motivo::DemasiadoPronto);
    }

    #[test]
    fn a_las_horas_se_recuerda_lo_critico_y_al_dia_lo_demas() {
        let s_crit = sint("disco:C:", Nivel::Critico);
        let s_avi = sint("disco:C:", Nivel::Aviso);
        // Crítico: a las cuatro horas.
        assert_eq!(
            juicio(&s_crit, Some((AHORA - REPITE_CRITICO + 1, Nivel::Critico))),
            Motivo::YaLoDije
        );
        assert_eq!(juicio(&s_crit, Some((AHORA - REPITE_CRITICO, Nivel::Critico))), Motivo::Recordatorio);
        // Aviso: al día. Con el plazo de lo crítico ya cumplido, todavía calla.
        assert_eq!(juicio(&s_avi, Some((AHORA - REPITE_CRITICO, Nivel::Aviso))), Motivo::YaLoDije);
        assert_eq!(juicio(&s_avi, Some((AHORA - REPITE_AVISO, Nivel::Aviso))), Motivo::Recordatorio);
    }

    #[test]
    fn de_lo_critico_se_avisa_cuando_se_arregla_y_de_lo_demas_no() {
        // Sin esto, el operador se queda sin saber si aquello sigue pasando y
        // acaba yendo a mirar — el trabajo que el vigilante venía a ahorrar.
        let ok = sint("servicios", Nivel::Ok);
        assert_eq!(juicio(&ok, Some((AHORA - 3_600, Nivel::Critico))), Motivo::Recuperado);
        // Pero «el disco ya no está al 87 %» no es una noticia.
        assert_eq!(juicio(&ok, Some((AHORA - 3_600, Nivel::Aviso))), Motivo::NadaQueDecir);
    }

    #[test]
    fn la_recuperacion_tambien_pasa_por_el_freno() {
        // ESTE TEST AFIRMABA LO CONTRARIO Y ESTABA MAL. Decía que la
        // recuperación debía saltarse el freno «para no dejar al operador con la
        // alarma en la cabeza y sin el ya está», y ese argumento vale para UNA
        // recuperación, no para una serie: con la salida por delante del corte,
        // una CPU que baila alrededor del umbral producía dos globos cada veinte
        // minutos indefinidamente, alternando la alarma y su resolución.
        //
        // Un problema que se arregla solo en menos de quince minutos no merece
        // dos notificaciones: merece cero.
        let ok = sint("servicios", Nivel::Ok);
        assert_eq!(juicio(&ok, Some((AHORA - 30, Nivel::Critico))), Motivo::DemasiadoPronto);
        // Pasado el freno, sí se dice.
        assert_eq!(
            juicio(&ok, Some((AHORA - MIN_ENTRE_AVISOS, Nivel::Critico))),
            Motivo::Recuperado
        );
    }

    #[test]
    fn una_metrica_que_parpadea_se_va_callando_sola() {
        // LA SECUENCIA QUE ROMPIÓ LA POLÍTICA, medida sobre un servidor de
        // compilación: la CPU alterna crítico y tranquilo cada cinco minutos.
        // Cada cambio de estado es DE VERDAD una noticia según las reglas; el
        // problema es que el estado cambia sin parar.
        //
        //   con la recuperación por delante del freno   14 globos en 2 h
        //   con el freno fijo por delante                8 globos en 2 h
        //   con el freno que se espacia                  4 globos en 2 h, y bajando
        let mut antes: Antes = Antes::new();
        let mut globos = 0;
        let mut en_dos_horas = 0;
        // Un día entero, que es donde se ve si converge o si solo se ralentiza.
        for paso in 0..288 {
            let t = AHORA + paso * 300;
            let n = if paso % 2 == 0 { Nivel::Critico } else { Nivel::Ok };
            let s = sint("cpu", n);
            let m = juzga(&s, antes.get("cpu").copied(), t);
            if m.avisa() {
                globos += 1;
                if t - AHORA <= 7_200 {
                    en_dos_horas += 1;
                }
                let ya = antes.get("cpu").map(|x| x.2).unwrap_or(0);
                antes.insert("cpu".into(), (t, n, ya + 1));
            }
        }
        assert!(en_dos_horas <= 4, "las dos primeras horas dieron {en_dos_horas} globos");
        // Y EL DÍA ENTERO ES LO QUE DECIDE SI LA FUNCIÓN SOBREVIVE. Antes eran
        // noventa y seis: el operador apaga las notificaciones y el vigilante
        // deja de existir.
        assert!(globos <= 12, "un día de parpadeo dio {globos} globos");
    }

    #[test]
    fn el_freno_se_duplica_y_tiene_techo() {
        // La curva, escrita para poder discutirla sin base de datos delante.
        assert_eq!(freno(0), MIN_ENTRE_AVISOS, "el primer aviso no se hace esperar");
        assert_eq!(freno(ANTES_DE_ESPACIAR), MIN_ENTRE_AVISOS, "empieza a espaciar demasiado pronto");
        assert_eq!(freno(ANTES_DE_ESPACIAR + 1), MIN_ENTRE_AVISOS * 2);
        assert_eq!(freno(ANTES_DE_ESPACIAR + 2), MIN_ENTRE_AVISOS * 4);
        // El techo existe: sin él, veinte avisos serían un freno de años y la
        // clave se quedaría muda para siempre — que es el fallo contrario y
        // igual de malo.
        let techo = MIN_ENTRE_AVISOS * (1 << TOPE_ESPACIADO);
        assert_eq!(freno(999), techo);
        assert!(techo <= 12 * 3_600, "el techo deja una clave muda medio día o más");
    }

    #[test]
    fn una_pasada_no_puede_soltar_cuarenta_globos() {
        // Lucy cerrada una semana, se abre, y todo ha cambiado. Cuarenta globos
        // seguidos: el operador no lee ninguno y apaga los avisos para siempre.
        let muchos: Vec<Sintoma> = (0..12)
            .map(|i| sint(&format!("cosa{i}"), Nivel::Aviso))
            .collect();
        let d = decide(muchos, &Antes::new(), AHORA);
        let avisados = d.iter().filter(|x| matches!(x, Decision::Avisa(..))).count();
        assert_eq!(avisados, MAX_POR_PASADA);
        // Y los que no caben lo dicen, no desaparecen sin explicación.
        assert!(d.iter().any(|x| matches!(x, Decision::Calla(Motivo::NoCabeEnLaPasada))));
    }

    #[test]
    fn el_tope_de_la_pasada_corta_por_abajo_y_no_por_orden_de_medicion() {
        // Si cortara por orden de llegada, un disco crítico se quedaría fuera
        // por tres avisos de CPU que se midieron antes.
        let mut v: Vec<Sintoma> = (0..5).map(|i| sint(&format!("leve{i}"), Nivel::Aviso)).collect();
        v.push(sint("disco:C:", Nivel::Critico));
        let d = decide(v, &Antes::new(), AHORA);
        let dichos: Vec<&str> = d
            .iter()
            .filter_map(|x| match x {
                Decision::Avisa(s, _) => Some(s.clave.as_str()),
                _ => None,
            })
            .collect();
        assert!(dichos.contains(&"disco:C:"), "lo crítico se quedó fuera: {dichos:?}");
    }

    #[test]
    fn un_reloj_hacia_atras_no_desata_una_tormenta() {
        // Cambio de hora o sincronización NTP: `ahora - ts` sale negativo. Con
        // una resta a secas eso es un número enorme por debajo, y de golpe todo
        // «toca repetirlo». El `saturating_sub` lo deja en cero, que cae en
        // DemasiadoPronto — callarse.
        let previo = Some((AHORA + 86_400, Nivel::Critico));
        assert_eq!(juicio(&sint("cpu", Nivel::Critico), previo), Motivo::DemasiadoPronto);
    }

    #[test]
    fn un_servicio_parado_limpiamente_no_es_una_averia() {
        // `sppsvc` se para solo para ahorrar recursos y `MapsBroker` arranca
        // bajo demanda. Avisar de eso es la primera forma de que alguien apague
        // los avisos para siempre.
        let s = SysSnapshot {
            host: "x".into(),
            os: "y".into(),
            kernel: String::new(),
            cpu_brand: String::new(),
            cpu_pct: 5.0,
            per_core: vec![],
            mem_used: 1,
            mem_total: 100,
            swap_used: 0,
            swap_total: 0,
            uptime_secs: 0,
            cores: 4,
            disks: vec![],
        };
        let limpios = vec![
            DownService { name: "sppsvc".into(), exit_code: 0 },
            DownService { name: "MapsBroker".into(), exit_code: 0 },
        ];
        let v = observa_local(&s, &limpios, &Umbrales::default());
        let serv = v.iter().find(|x| x.clave == "servicios").expect("falta el síntoma");
        assert_eq!(serv.nivel, Nivel::Ok, "un servicio parado limpiamente salió como avería");

        let roto = vec![DownService { name: "w3svc".into(), exit_code: 1067 }];
        let v = observa_local(&s, &roto, &Umbrales::default());
        let serv = v.iter().find(|x| x.clave == "servicios").expect("falta el síntoma");
        assert_eq!(serv.nivel, Nivel::Critico);
        assert!(serv.cuerpo.contains("w3svc"));
    }

    #[test]
    fn la_recuperacion_no_dice_que_cero_servicios_han_fallado() {
        // LO QUE SALÍA ANTES, y no era un caso raro: `servicios` es la única
        // clave que llega de Critico a Ok en una sola pasada, así que ésta era
        // la forma DOMINANTE del aviso de recuperación.
        //
        //   Resuelto: 0 servicios automáticos han fallado
        //   Ha vuelto a la normalidad.            <- y el cuerpo, vacío
        let s = SysSnapshot {
            host: String::new(),
            os: String::new(),
            kernel: String::new(),
            cpu_brand: String::new(),
            cpu_pct: 1.0,
            per_core: vec![],
            mem_used: 0,
            mem_total: 0,
            swap_used: 0,
            swap_total: 0,
            uptime_secs: 0,
            cores: 1,
            disks: vec![],
        };
        let v = observa_local(&s, &[], &Umbrales::default());
        let serv = v.iter().find(|x| x.clave == "servicios").expect("falta el síntoma");
        let a = aviso_de(serv, Motivo::Recuperado);
        assert!(!a.titulo.contains('0'), "el título dice que cero han fallado: «{}»", a.titulo);
        assert!(!a.cuerpo.trim().is_empty(), "el cuerpo de la recuperación va vacío");
        assert!(a.titulo.starts_with("Resuelto:"));

        // Y una unidad no pierde su letra por el camino: `to_lowercase` dejaba
        // «Resuelto: disco d:\ casi lleno», que es justo lo que el operador usa
        // para saber de qué disco le hablan.
        let con_disco = SysSnapshot {
            disks: vec![crate::system::DiskInfo {
                name: "Datos".into(),
                mount: "D:\\".into(),
                total: 1_000,
                avail: 900,
            }],
            ..s
        };
        let v = observa_local(&con_disco, &[], &Umbrales::default());
        let d = v.iter().find(|x| x.clave.starts_with("disco:")).expect("falta el disco");
        let a = aviso_de(d, Motivo::Recuperado);
        assert!(a.titulo.contains("D:"), "la unidad se perdió: «{}»", a.titulo);
    }

    #[test]
    fn la_clave_de_un_disco_es_el_punto_de_montaje_y_no_su_nombre() {
        // El nombre de un volumen se cambia desde el explorador. Si la clave
        // fuera el nombre, renombrarlo haría que el disco volviera a ser «nuevo»
        // y el vigilante avisaría otra vez de lo mismo.
        let s = SysSnapshot {
            host: String::new(),
            os: String::new(),
            kernel: String::new(),
            cpu_brand: String::new(),
            cpu_pct: 1.0,
            per_core: vec![],
            mem_used: 0,
            mem_total: 0,
            swap_used: 0,
            swap_total: 0,
            uptime_secs: 0,
            cores: 1,
            disks: vec![crate::system::DiskInfo {
                name: "Datos de Iván".into(),
                mount: "D:\\".into(),
                total: 1_000,
                avail: 10,
            }],
        };
        let v = observa_local(&s, &[], &Umbrales::default());
        assert!(v.iter().any(|x| x.clave == "disco:D:\\"), "claves: {:?}",
                v.iter().map(|x| &x.clave).collect::<Vec<_>>());
        assert!(!v.iter().any(|x| x.clave.contains("Iván")));
    }

    #[test]
    fn un_disco_sin_tamano_no_produce_una_division_por_cero() {
        // Los hay: unidades de red desconectadas, lectores vacíos.
        let s = SysSnapshot {
            host: String::new(),
            os: String::new(),
            kernel: String::new(),
            cpu_brand: String::new(),
            cpu_pct: 1.0,
            per_core: vec![],
            mem_used: 0,
            mem_total: 0,
            swap_used: 0,
            swap_total: 0,
            uptime_secs: 0,
            cores: 1,
            disks: vec![crate::system::DiskInfo {
                name: "vacío".into(),
                mount: "E:\\".into(),
                total: 0,
                avail: 0,
            }],
        };
        let v = observa_local(&s, &[], &Umbrales::default());
        assert!(!v.iter().any(|x| x.clave.starts_with("disco:")));
    }

    #[test]
    fn seis_de_las_ocho_ramas_son_para_callarse() {
        // No es una prueba de comportamiento: es una guarda de diseño. Si
        // alguien añade motivos de avisar sin añadir motivos de callarse, esta
        // proporción se rompe y conviene que alguien lo mire — un vigilante que
        // avisa de más se silencia, y entonces la función está muerta.
        let todos = [
            Motivo::Nuevo,
            Motivo::Empeora,
            Motivo::Recordatorio,
            Motivo::Recuperado,
            Motivo::NadaQueDecir,
            Motivo::YaLoDije,
            Motivo::DemasiadoPronto,
            Motivo::NoCabeEnLaPasada,
        ];
        let callan = todos.iter().filter(|m| !m.avisa()).count();
        assert!(callan >= todos.len() / 2, "más motivos de hablar que de callar");
    }
}
