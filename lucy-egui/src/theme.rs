//! El sistema de diseño de Lucy, portado del CSS a egui.
//!
//! Va PRIMERO, antes de migrar vistas. Construir veinte vistas con el estilo por
//! defecto de egui y re-tematizarlas después es tirar ese trabajo: en modo
//! inmediato el estilo no es una hoja aparte que se cambia al final, está en
//! cada llamada de dibujo.
//!
//! LA FUENTE DE VERDAD ES `src/lib/styles/cockpit-tokens.css`, no `page.css`.
//! Esto estuvo mal apuntado hasta ahora y explica por qué el prototipo "se
//! parecía pero no era": `page.css` es el tema del Lucy clásico —verde `#10b981`,
//! fondos azulados— y el Cockpit, que es la interfaz de las capturas, tiene su
//! propio juego de tokens: esmeralda desaturada `#3DD6A4`, superficies casi
//! negras y bordes de un blanco translúcido en vez de líneas sólidas. Dos
//! paletas distintas conviviendo en el mismo repo, y el prototipo había copiado
//! la que no era.
//!
//! Cada constante lleva su nombre CSS al lado para que la correspondencia se
//! pueda comprobar de un vistazo en vez de adivinarla.

use eframe::egui::{self, Color32, Rounding, Stroke};

// ── Claro u oscuro ───────────────────────────────────────────────────────────
//
// POR QUÉ LOS COLORES SON FUNCIONES Y NO CONSTANTES. Eran constantes, y era lo
// correcto mientras hubo un solo tema. Con dos, el color deja de ser un dato del
// programa y pasa a ser una consulta: depende de un ajuste que el operador puede
// cambiar en caliente. Una constante no puede contestar a eso.
//
// El coste es una rama por consulta de color, unas cuantas por frame. Al lado de
// lo que cuesta teselar el texto de la ventana, no se mide.

/// Qué tema se está usando.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
    /// El del sistema. Windows lo publica en el registro.
    Auto,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Oscuro",
            Self::Light => "Claro",
            Self::Auto => "Del sistema",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Auto => "auto",
        }
    }
    pub fn from_key(k: &str) -> Self {
        match k {
            "light" => Self::Light,
            "auto" => Self::Auto,
            _ => Self::Dark,
        }
    }
    pub const ALL: [Self; 3] = [Self::Dark, Self::Light, Self::Auto];
}

/// El modo elegido, y el valor YA RESUELTO.
///
/// Se guardan los dos porque `Auto` hay que preguntárselo al sistema, y eso es
/// leer el registro: hacerlo en cada consulta de color serían miles de lecturas
/// por segundo. Se resuelve al cambiar el modo y al arrancar, y lo que se
/// consulta luego es un booleano atómico.
static MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static LIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Si el tema resuelto es el claro. Es lo que consultan los colores.
pub fn light() -> bool {
    LIGHT.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn mode() -> Mode {
    match MODE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Mode::Light,
        2 => Mode::Auto,
        _ => Mode::Dark,
    }
}

pub fn set_mode(m: Mode) {
    MODE.store(
        match m {
            Mode::Dark => 0,
            Mode::Light => 1,
            Mode::Auto => 2,
        },
        std::sync::atomic::Ordering::Relaxed,
    );
    let claro = match m {
        Mode::Dark => false,
        Mode::Light => true,
        Mode::Auto => os_prefers_light(),
    };
    LIGHT.store(claro, std::sync::atomic::Ordering::Relaxed);
}

/// Si Windows está en tema claro para las aplicaciones.
///
/// `AppsUseLightTheme` y no `SystemUsesLightTheme`: son dos ajustes distintos y
/// la gente los tiene cruzados a menudo — barra de tareas oscura con
/// aplicaciones claras es una combinación común. El que manda aquí es el de las
/// aplicaciones, porque Lucy es una.
///
/// Si no se puede leer, oscuro. Es el tema de la casa, y adivinar claro dejaría
/// una ventana blanca a quien nunca pidió una.
#[cfg(windows)]
fn os_prefers_light() -> bool {
    use std::process::Command;
    let out = Command::new("reg")
        .args([
            "query",
            r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .split_whitespace()
            .last()
            .map(|v| v == "0x1")
            .unwrap_or(false),
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn os_prefers_light() -> bool {
    false
}

// ── Superficies · escalera de elevación ──────────────────────────────────────
// La profundidad del Cockpit sale de superficies planas apiladas más bordes
// finos, nunca de sombras ni de brillos. Solo los menús flotantes llevan sombra.

/// `--surface-0` — lienzo de la página.
pub fn bg() -> Color32 {
    if light() { Color32::from_rgb(0xF4, 0xF6, 0xFA) } else { Color32::from_rgb(0x0A, 0x0E, 0x14) }
}
/// `--surface-1` — cromo: rail, cabecera, pie, y los paneles grandes.
pub fn bg2() -> Color32 {
    if light() { Color32::from_rgb(0xFF, 0xFF, 0xFF) } else { Color32::from_rgb(0x0E, 0x13, 0x19) }
}
/// `--surface-2` — tarjetas y controles en reposo.
///
/// ⚠ `bg2` Y `bg3` SON EL MISMO BLANCO EN EL TEMA CLARO, y de ahí salió una
/// familia entera de fallos. La escalera tiene cuatro peldaños y en un tema claro
/// no hay cuatro blancos distintos que sigan siendo blancos, así que los dos de
/// en medio coinciden. No es un error: es lo que hace que el tema claro se vea
/// como un tema claro y no como un gris.
///
/// LO QUE SÍ ES UN ERROR es apoyar en esa diferencia una señal de ESTADO. Un
/// hover que pinta `bg3` sobre una superficie que ya es `bg2` no se ve — y no
/// falla, no avisa, simplemente no pasa nada al pasar el ratón. Se encontró en
/// tres sitios: el rail de módulos, la lista de equipos de NexShell y el botón
/// de refresco del dashboard remoto. Los tres eran el elemento más usado de su
/// pantalla.
///
/// LA REGLA: para el estado de un control, el salto va a `bg4`, que es el único
/// peldaño distinto de los otros tres en los DOS temas. `bg3` sobre `bg()` sí
/// vale —ahí la diferencia existe en ambos— y por eso las filas del visor de
/// logs se quedan como están.
pub fn bg3() -> Color32 {
    if light() { Color32::from_rgb(0xFF, 0xFF, 0xFF) } else { Color32::from_rgb(0x13, 0x1A, 0x22) }
}
/// `--surface-3` — hover, seleccionado, flotante, y el carril de las barras.
pub fn bg4() -> Color32 {
    if light() { Color32::from_rgb(0xEB, 0xEF, 0xF5) } else { Color32::from_rgb(0x18, 0x20, 0x2A) }
}

// ── Bordes ───────────────────────────────────────────────────────────────────
// Blanco translúcido, no un color sólido: así el mismo borde funciona sobre las
// cuatro superficies sin recalcularlo, que es justo lo que hace el CSS.

/// `--border` — `rgba(255,255,255,0.06)`. Premultiplicado: blanco con alfa `a`
/// es `(a,a,a,a)`, y el constructor premultiplicado es el único que es `const`.
pub fn bdr() -> Color32 {
    if light() { Color32::from_rgba_premultiplied(2, 2, 3, 26) } else { Color32::from_rgba_premultiplied(15, 15, 15, 15) }
}
/// `--border-strong` — `rgba(255,255,255,0.09)`.
pub fn bdr2() -> Color32 {
    if light() { Color32::from_rgba_premultiplied(2, 4, 5, 41) } else { Color32::from_rgba_premultiplied(23, 23, 23, 23) }
}

// ── Texto · escalera de contraste ────────────────────────────────────────────

/// `--text-primary` — títulos, cifras héroe, etiquetas activas.
pub fn txt() -> Color32 {
    if light() { Color32::from_rgb(0x0E, 0x16, 0x21) } else { Color32::from_rgb(0xE8, 0xED, 0xF2) }
}
/// `--text-secondary` — cuerpo, narrativa.
pub fn txt2() -> Color32 {
    if light() { Color32::from_rgb(0x2B, 0x37, 0x42) } else { Color32::from_rgb(0xC7, 0xD0, 0xD9) }
}
/// `--text-muted` — etiquetas, chips secundarios, marcas de tiempo.
pub fn txt3() -> Color32 {
    if light() { Color32::from_rgb(0x57, 0x63, 0x6F) } else { Color32::from_rgb(0x98, 0xA2, 0xAE) }
}
/// `--text-faint` — rótulos de instrumento, subtítulos, pistas.
///
/// SUBIDO HASTA QUE SE LEE, y lo que había antes no se leía. Los valores viejos
/// —`#8792A0` en claro, `#5C6672` en oscuro— dan 2.74 y 2.81 de contraste contra
/// el lienzo. El mínimo de la norma para texto pequeño es 4.5, y ni siquiera
/// llegaban al 3.0 que se le pide a una LÍNEA o a un icono.
///
/// No era un color de adorno pagando el precio de serlo: de sus ciento tres
/// usos, prácticamente todos son TEXTO —los rótulos de instrumento («DISCO
/// SISTEMA») y las dos líneas de detalle de cada tarjeta KPI— y ninguno es
/// pintura. O sea que el peldaño más flojo de la escalera llevaba encima las
/// etiquetas que dicen qué es cada número, a diez y once puntos.
///
/// El precio es que se acerca a `txt3`, y se acepta a sabiendas: la escalera
/// pierde algo de separación en el último tramo (4.50 contra 5.32 en claro) y
/// gana que ese tramo exista de verdad. Un peldaño ilegible no es un peldaño.
///
/// Los dos valores son el más cercano al original que llega a 4.5 moviendo SOLO
/// la luminosidad: el tono y la saturación no se tocan, así que es el mismo gris
/// azulado de siempre.
pub fn faint() -> Color32 {
    if light() { Color32::from_rgb(0x62, 0x6E, 0x7C) } else { Color32::from_rgb(0x7C, 0x88, 0x95) }
}
/// `--text-disabled` — inactivo.
#[allow(dead_code)]
pub fn disabled() -> Color32 {
    if light() { Color32::from_rgb(0xB4, 0xBD, 0xC7) } else { Color32::from_rgb(0x3C, 0x45, 0x50) }
}

// ── La paleta de acento ──────────────────────────────────────────────────────
//
// UN SOLO COLOR GOBIERNA TODO EL ACENTO de Lucy: la navegación activa, la
// actividad del agente, el progreso, lo hecho. Por eso se puede cambiar sin
// tocar nada más — y por eso hay que elegirlo con cuidado.
//
// LO QUE NO SE OFRECE, Y POR QUÉ. Ni rojo ni ámbar. En esta aplicación el rojo
// significa «esto ha fallado» y el ámbar «cuidado con esto», y son colores que
// aparecen sobre comandos destructivos y avisos de seguridad. Un acento del
// mismo color haría que la mitad de la pantalla pareciera una advertencia — y,
// peor, que una advertencia de verdad se leyera como decoración. El azul también
// queda fuera: es el color del operador en el hilo de conversación.

/// Un acento, en sus dos modos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paleta {
    /// El nombre que ve el operador.
    pub nombre: &'static str,
    /// La clave con la que se guarda. Estable: cambiarla dejaría la elección de
    /// todo el mundo en el valor de fábrica sin avisar.
    pub clave: &'static str,
    claro: (u8, u8, u8),
    claro_hover: (u8, u8, u8),
    oscuro: (u8, u8, u8),
    oscuro_hover: (u8, u8, u8),
}

/// Las paletas disponibles. La primera es la de casa.
///
/// LOS DOS ACENTOS CLAROS SE OSCURECIERON UN PASO MÁS, y no es un cambio de
/// gusto: es terminar algo que el CSS ya había empezado. `cockpit-tokens.css`
/// dice al lado del valor «emerald oscurecido para contraste sobre blanco», así
/// que la intención estaba escrita — pero medido, `#12A379` daba 2.78 sobre el
/// lienzo claro. Ni el 4.5 de la norma para texto, ni el 3.0 que se le pide a una
/// barra o a un icono. El oscurecido se había quedado a medio camino.
///
/// Esmeralda y Cian, y solo ellas. Violeta (5.15) y Magenta (4.61) ya pasaban, y
/// los cuatro acentos oscuros pasan de sobra —el más flojo va por 6.10—, porque
/// sobre un lienzo casi negro un color saturado tiene todo el margen del mundo.
///
/// El hover baja lo mismo que bajaba antes, para que la distancia entre reposo y
/// hover siga siendo la que era.
pub const PALETAS: &[Paleta] = &[
    Paleta {
        nombre: "Esmeralda",
        clave: "esmeralda",
        claro: (0x0E, 0x7B, 0x5B),
        claro_hover: (0x0C, 0x67, 0x4C),
        oscuro: (0x3D, 0xD6, 0xA4),
        oscuro_hover: (0x34, 0xC2, 0x96),
    },
    Paleta {
        nombre: "Cian",
        clave: "cian",
        claro: (0x0C, 0x77, 0x8C),
        claro_hover: (0x0B, 0x68, 0x7B),
        oscuro: (0x4C, 0xD2, 0xE8),
        oscuro_hover: (0x3F, 0xBC, 0xD1),
    },
    Paleta {
        nombre: "Violeta",
        clave: "violeta",
        claro: (0x6D, 0x4A, 0xCF),
        claro_hover: (0x5B, 0x3C, 0xB3),
        oscuro: (0xA8, 0x8B, 0xFF),
        oscuro_hover: (0x96, 0x79, 0xF0),
    },
    Paleta {
        nombre: "Magenta",
        clave: "magenta",
        claro: (0xB8, 0x36, 0x8C),
        claro_hover: (0x9C, 0x2B, 0x76),
        oscuro: (0xF0, 0x7A, 0xC4),
        oscuro_hover: (0xDC, 0x69, 0xB1),
    },
];

/// La elegida, por índice en `PALETAS`.
///
/// En un atómico y no en la struct de la aplicación por lo mismo que el modo
/// claro/oscuro: estas funciones las llama cualquier widget en cualquier punto
/// del árbol, y pasarles el estado por parámetro obligaría a atravesar toda la
/// interfaz con un argumento que casi nadie usa.
static ACENTO: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// La paleta activa.
pub fn paleta() -> &'static Paleta {
    let i = ACENTO.load(std::sync::atomic::Ordering::Relaxed);
    PALETAS.get(i).unwrap_or(&PALETAS[0])
}

/// Cambia la paleta. Fuera de rango deja la de casa — una clave de una versión
/// futura no puede dejar la aplicación sin acento.
pub fn set_paleta(i: usize) {
    ACENTO.store(if i < PALETAS.len() { i } else { 0 }, std::sync::atomic::Ordering::Relaxed);
}

/// El índice de una clave guardada. Desconocida = la de casa.
pub fn paleta_de(clave: &str) -> usize {
    PALETAS.iter().position(|p| p.clave == clave).unwrap_or(0)
}

/// El acento base, según modo y paleta.
fn acc_rgb() -> (u8, u8, u8) {
    let p = paleta();
    if light() { p.claro } else { p.oscuro }
}

// ── Acento · la esmeralda de Lucy ────────────────────────────────────────────
// UN acento por vista. Nav activo, actividad del agente, progreso, "hecho".

/// `--accent`.
pub fn acc() -> Color32 {
    let (r, g, b) = acc_rgb();
    Color32::from_rgb(r, g, b)
}
/// `--accent-hover`.
pub fn acc_hover() -> Color32 {
    let p = paleta();
    let (r, g, b) = if light() { p.claro_hover } else { p.oscuro_hover };
    Color32::from_rgb(r, g, b)
}
/// `--accent-ink` — texto SOBRE un relleno de acento sólido.
pub fn acc_ink() -> Color32 {
    if light() { Color32::from_rgb(0xFF, 0xFF, 0xFF) } else { Color32::from_rgb(0x07, 0x13, 0x0E) }
}
/// `--accent-bg` — el acento al 12 %: chip teñido, píldora activa.
///
/// CALCULADO DEL ACENTO y no escrito a mano como antes. Con cuatro paletas, una
/// tabla de tintes premultiplicados serían veinte números que hay que recalcular
/// a mano cada vez que se toca un color — y que se desincronizan en silencio,
/// dejando un chip verde en una interfaz violeta.
pub fn acc_bg() -> Color32 {
    let (r, g, b) = acc_rgb();
    Color32::from_rgba_unmultiplied(r, g, b, 31)
}
/// `--accent-line` — el acento al 28 %.
#[allow(dead_code)]
pub fn acc_line() -> Color32 {
    let (r, g, b) = acc_rgb();
    Color32::from_rgba_unmultiplied(r, g, b, 71)
}

// ── Semánticos ───────────────────────────────────────────────────────────────
// Contenidos, no de neón: el Cockpit se apoya en el contraste, no en el color.

/// `--user` — el operador. También la subida de red.
pub fn blue() -> Color32 {
    if light() { Color32::from_rgb(0x51, 0x62, 0xDC) } else { Color32::from_rgb(0x80, 0x98, 0xFF) }
}
/// `--warning`.
pub fn amber() -> Color32 {
    if light() { Color32::from_rgb(0xB5, 0x76, 0x14) } else { Color32::from_rgb(0xE5, 0xB5, 0x67) }
}
/// `--warning-bg` — `rgba(229,181,103,0.12)`.
pub fn amber_bg() -> Color32 {
    if light() { Color32::from_rgba_premultiplied(25, 17, 3, 36) } else { Color32::from_rgba_premultiplied(27, 21, 12, 31) }
}
/// `--danger`.
pub fn red() -> Color32 {
    if light() { Color32::from_rgb(0xD6, 0x45, 0x45) } else { Color32::from_rgb(0xF0, 0x6E, 0x6E) }
}
/// `--danger-bg` — `rgba(240,110,110,0.12)`.
pub fn red_bg() -> Color32 {
    if light() { Color32::from_rgba_premultiplied(25, 8, 8, 31) } else { Color32::from_rgba_premultiplied(28, 13, 13, 31) }
}

// ── Tipografía ───────────────────────────────────────────────────────────────
// La escala del Cockpit, en puntos. No son números sueltos: cada tamaño tiene un
// papel, y usar el de al lado es lo que convierte un panel en una plantilla.

/// `--fs-micro` — rótulo de instrumento (mono, versalitas, con tracking).
pub const FS_MICRO: f32 = 10.0;
/// La barra de estado de arriba, medio punto por debajo de `FS_CAPTION`.
///
/// EXISTÍA YA, PERO COMO NUEVE `10.5` SUELTOS. Es una tira horizontal que no
/// puede envolver ni desbordar y que lleva el equipo, el modelo, el modo fijado y
/// los avisos sin leer a la vez; medio punto menos es lo que hace que quepan en
/// una ventana estrecha. Eso es un PAPEL, y un papel se nombra: escrito nueve
/// veces a mano, el décimo sitio lo pone a 11 sin querer y la tira deja de estar
/// alineada consigo misma.
///
/// Medio punto y no uno: a 10 se confunde con el rótulo de instrumento, que es
/// otra cosa —versalitas, mono, con tracking— y a 11 no cabe.
pub const FS_BAR: f32 = 10.5;
/// `--fs-caption` — etiquetas de sección, metadatos.
pub const FS_CAPTION: f32 = 11.0;
/// `--fs-footnote` — chips, secundario.
pub const FS_FOOTNOTE: f32 = 12.0;
/// `--fs-body` — la mayor parte de la interfaz.
pub const FS_BODY: f32 = 13.0;
/// `--fs-heading` — títulos de panel.
pub const FS_HEADING: f32 = 15.0;
/// `--fs-title` — título de vista.
pub const FS_TITLE: f32 = 18.0;

// ── El tramo de display ──────────────────────────────────────────────────────
//
// LA ESCALA NO LLEGABA HASTA ARRIBA, y por eso el código tenía un `22.0` en tres
// sitios y un `28.0` en el que más se mira de la aplicación. No era gente
// saltándose el sistema: era el sistema quedándose corto. Una cifra héroe no es
// «un título de vista un poco más grande»; es el elemento que ES la tarjeta, y
// no tenía nombre.
//
// Dos y no uno, porque hay dos tamaños de tarjeta: la del panel local, donde la
// cifra manda sobre todo lo demás, y la de un equipo remoto o el saludo, que
// comparten sitio con más cosas.

/// La cifra que ES la tarjeta: el número grande del KPI local.
pub const FS_HERO: f32 = 28.0;
/// La cifra o la frase que manda en una tarjeta compartida: los equipos
/// remotos, el saludo de la pantalla de bienvenida.
pub const FS_DISPLAY: f32 = 22.0;

/// `--ls-label` — `0.09em`, el tracking de los rótulos de instrumento.
pub const LS_LABEL: f32 = 0.09;

// LOS GLIFOS NO SON ESTA ESCALA, y por eso no están aquí. Un `●` de estado a 7,
// un `✦` de avatar a 40 o un `⚠` a 12 se miden contra la CAJA que ocupan y
// contra el texto que llevan al lado, no contra un papel tipográfico: son
// dibujos hechos con una fuente. Meterlos en la escala obligaría a inventar un
// nombre por cada tamaño de punto y no haría el código más claro.

// ── Movimiento ───────────────────────────────────────────────────────────────
// En segundos, que es lo que piden las funciones de animación de egui.

/// `--dur-fast` — 120 ms.
pub const DUR_FAST: f32 = 0.12;
/// `--dur-base` — 200 ms.
pub const DUR_BASE: f32 = 0.2;
/// `--dur-slow` — 320 ms.
pub const DUR_SLOW: f32 = 0.32;

// ── Radios ───────────────────────────────────────────────────────────────────

/// `--r-sm` — controles, chips.
pub const R_SM: f32 = 8.0;
/// `--r-md` — botones, campos, elementos de navegación.
pub const R_MD: f32 = 10.0;
/// `--r-lg` — tarjetas y paneles.
pub const R_LG: f32 = 12.0;

/// La cápsula de un control: el radio que hace sus extremos semicírculos.
///
/// SE PIDE POR EL ALTO Y NO POR EL RADIO porque el radio no es el dato: el dato
/// es «esto es una cápsula», y de un alto solo sale un radio que lo cumpla. Un
/// número escrito a mano se queda atrás en cuanto alguien cambia la altura del
/// control, y lo que sale entonces no es ni cápsula ni rectángulo — es la forma
/// intermedia que se lee como «rectángulo con las esquinas limadas», que es
/// justo lo que hay que evitar.
///
/// La familia de píldoras de Lucy ya era cápsula de verdad —`tag_chip` con 9
/// sobre 18, `insignia` con 11 sobre 22, `meter` con `h / 2.0`, y cinco sitios
/// más con un 999 que se pasa de largo a propósito— pero cada una a su manera.
/// Esto es esa misma cuenta, con nombre.
pub fn capsule(alto: f32) -> Rounding {
    Rounding::same(alto / 2.0)
}

/// El radio de algo insertado dentro de otra cosa redondeada.
///
/// CONCÉNTRICO, que es una regla y no una preferencia: para que el aro de aire
/// entre las dos formas tenga el MISMO grosor en todo el contorno, el radio de
/// dentro tiene que ser el de fuera menos la inserción. Con cualquier otro
/// número el aro es más grueso en los lados rectos que en las esquinas, y eso se
/// ve aunque no se sepa nombrar.
///
/// El segmentado lo tenía mal por un píxel: 8 fuera, 3 de inserción, y 6 dentro
/// donde tocaba 5. Un píxel en un control de treinta.
#[allow(dead_code)]
pub fn concentrico(fuera: f32, insercion: f32) -> Rounding {
    Rounding::same((fuera - insercion).max(0.0))
}

/// Radio por defecto de los widgets de egui.
const ROUND: f32 = R_SM;

/// Carga las fuentes del sistema que el tema necesita.
///
/// El CSS pide Inter para la interfaz y JetBrains Mono para el instrumento.
/// Ninguna de las dos se puede usar aquí: vienen de `@fontsource-variable` y solo
/// se distribuyen en `.woff2`, un formato que el motor de texto de egui no lee.
/// Así que se toman las del sistema que ocupan ese mismo papel, y que además ya
/// están en la cadena de reserva del propio CSS:
///
///   • Segoe UI por Inter — la sans de interfaz de Windows.
///   • Cascadia Mono por JetBrains Mono — `--font-mono` la lista por su nombre,
///     así que no es una sustitución inventada; es la siguiente de la lista.
///
/// Se leen de `C:\Windows\Fonts` en vez de empaquetarlas: sin binarios en el
/// repo, sin peso en el ejecutable y sin problema de licencia. Y si un fichero
/// falta, la fuente por defecto de egui sigue detrás en la lista — un tema no
/// debe poder dejar la aplicación sin letras.
fn load_system_fonts(ctx: &egui::Context) {
    use eframe::egui::{FontData, FontFamily};

    let mut fonts = egui::FontDefinitions::default();

    // Delante de la de egui: es la fuente de la interfaz, no un respaldo.
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts.font_data.insert("segoe_ui".into(), FontData::from_owned(bytes));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe_ui".into());
    }

    // Monoespaciada: Cascadia primero, Consolas si no está.
    for (name, path) in [
        ("cascadia", r"C:\Windows\Fonts\CascadiaMono.ttf"),
        ("consolas", r"C:\Windows\Fonts\consola.ttf"),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(name.into(), FontData::from_owned(bytes));
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(0, name.into());
            break;
        }
    }

    // Símbolos AL FINAL de las dos familias: solo se consulta para los glifos
    // geométricos (`◱ ✦ ▸ ▤ ◈ ◉`) que ninguna de las otras trae, y que sin ella
    // salían como cuadros de tofu en cada entrada del rail.
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
        fonts.font_data.insert("segoe_symbol".into(), FontData::from_owned(bytes));
        for fam in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(fam).or_default().push("segoe_symbol".into());
        }
    } else {
        eprintln!(
            "[lucy] no se pudo leer C:\\Windows\\Fonts\\seguisym.ttf — \
             los iconos del rail se verán como cuadros vacíos."
        );
    }

    ctx.set_fonts(fonts);
}

/// Cambia la paleta de acento Y VUELVE A APLICAR LOS VISUALES.
///
/// LO SEGUNDO ES LA MITAD QUE FALTABA, y el fallo se veía. Los colores que se
/// consultan al dibujar —todo lo que llama a `acc()` en un `painter`— siguen la
/// paleta al instante, pero hay CUATRO que se copian dentro de los `Visuals` de
/// egui y se quedan ahí hasta que alguien los vuelva a poner:
///
///   · `selection.bg_fill`  — el relleno de un `selectable_label`
///   · `selection.stroke`
///   · `widgets.active.bg_stroke` — el anillo de foco de los widgets de egui
///   · `hyperlink_color`
///
/// Con `set_paleta` a secas, cambiar de acento dejaba esos cuatro en el color
/// anterior. Se veía sin buscarlo: en la vista de Memoria, con la paleta puesta
/// en Violeta, la pestaña activa seguía saliendo VERDE — es un
/// `selectable_label`, y su relleno es `selection.bg_fill`.
///
/// Y PASABA TAMBIÉN AL ARRANCAR, que es lo que lo hacía permanente. La paleta
/// guardada se lee en `App::new`, que corre DESPUÉS de `apply`; así que quien no
/// usara la esmeralda abría Lucy con esos cuatro en esmeralda todos los días, y
/// solo se arreglaban de rebote si tocaba el modo claro/oscuro —lo único que
/// volvía a llamar a `apply_visuals`—.
///
/// Es la misma forma que `switch` para el modo, y por eso vive al lado: dos
/// funciones que hacen «cambia esto y vuelve a aplicar», para que no se pueda
/// hacer lo uno sin lo otro.
pub fn cambia_paleta(ctx: &egui::Context, i: usize) {
    set_paleta(i);
    apply_visuals(ctx);
}

/// Cambia el tema y lo vuelve a aplicar. Un solo sitio, para que no se pueda
/// hacer lo uno sin lo otro.
pub fn switch(ctx: &egui::Context, m: Mode) {
    set_mode(m);
    // SOLO los colores, no las fuentes. `apply` lee tres ficheros TTF del disco
    // y llama a `set_fonts`, que tira el atlas entero y obliga a volver a
    // teselar cada letra de la ventana. Hacerlo por cambiar de tema es un tirón
    // visible a cambio de nada: las fuentes son las mismas en claro y en oscuro.
    apply_visuals(ctx);
}

/// Aplica el tema completo al contexto: fuentes y colores. Al arrancar.
pub fn apply(ctx: &egui::Context) {
    load_system_fonts(ctx);
    apply_visuals(ctx);
}

/// Los colores, y nada más.
///
/// Al arrancar y CADA VEZ QUE CAMBIA EL MODO. Los que se consultan al dibujar se
/// resuelven solos, pero éstos no: se copian dentro de
/// los `Visuals` de egui y se quedan ahí hasta que alguien los vuelva a poner.
/// Sin la segunda llamada, pasar a claro dejaba los widgets propios de egui
/// —desplegables, barras de scroll, campos— en negro sobre blanco.
fn apply_visuals(ctx: &egui::Context) {
    // La base la elige el modo. No es cosmético: de ahí salen decenas de colores
    // que este fichero no toca uno a uno, y partir de la base oscura en tema
    // claro deja rincones negros donde nadie los ha puesto.
    let mut visuals = if light() { egui::Visuals::light() } else { egui::Visuals::dark() };

    visuals.override_text_color = Some(txt2());
    visuals.panel_fill = bg();
    visuals.window_fill = bg3();
    // `extreme_bg_color` es el fondo de los campos de texto Y el CARRIL de las
    // barras de progreso. Con el color del panel, la parte vacía de la barra
    // desaparecía: un 1 % de CPU se veía como un puntito verde suelto en vez de
    // como una barra casi vacía. `--surface-3` es lo que usa el CSS.
    visuals.extreme_bg_color = bg4();
    visuals.faint_bg_color = bg2();
    visuals.code_bg_color = bg3();
    visuals.hyperlink_color = acc();
    visuals.window_stroke = Stroke::new(1.0_f32, bdr2());
    visuals.window_rounding = Rounding::same(R_LG);
    // La ÚNICA sombra suave permitida: menús y popovers flotantes. Todo lo demás
    // plano — `--shadow-pop`. Sale de `sombra_flotante` para que los menús que
    // Lucy monta a mano sobre un `Area` —que no pasan por aquí— usen exactamente
    // la misma y no una copia con otros números.
    visuals.popup_shadow = sombra_flotante();

    // Los cinco estados de widget. egui los usa TODOS: dejarlos por defecto es
    // lo que hace que una app se vea "de egui" en vez de tuya.
    let w = &mut visuals.widgets;

    // Inactivo — la mayoría de lo que se ve en pantalla.
    w.inactive.bg_fill = bg3();
    w.inactive.weak_bg_fill = bg3();
    w.inactive.bg_stroke = Stroke::new(1.0_f32, bdr());
    w.inactive.fg_stroke = Stroke::new(1.0_f32, txt3());
    w.inactive.rounding = Rounding::same(ROUND);

    // Con el cursor encima — sube un escalón de fondo, el borde se aclara.
    w.hovered.bg_fill = bg4();
    w.hovered.weak_bg_fill = bg4();
    w.hovered.bg_stroke = Stroke::new(1.0_f32, bdr2());
    w.hovered.fg_stroke = Stroke::new(1.0_f32, txt());
    w.hovered.rounding = Rounding::same(ROUND);

    // Pulsado / activo — aquí entra el acento, y solo aquí.
    w.active.bg_fill = bg4();
    w.active.weak_bg_fill = bg4();
    w.active.bg_stroke = Stroke::new(1.0_f32, acc());
    w.active.fg_stroke = Stroke::new(1.0_f32, txt());
    w.active.rounding = Rounding::same(ROUND);

    // No interactivo — texto plano, etiquetas.
    w.noninteractive.bg_fill = bg();
    w.noninteractive.weak_bg_fill = bg();
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, bdr());
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, txt2());
    w.noninteractive.rounding = Rounding::same(ROUND);

    // Abierto (desplegables desplegados).
    w.open.bg_fill = bg3();
    w.open.weak_bg_fill = bg3();
    w.open.bg_stroke = Stroke::new(1.0_f32, bdr2());
    w.open.fg_stroke = Stroke::new(1.0_f32, txt());
    w.open.rounding = Rounding::same(ROUND);

    visuals.selection.bg_fill = acc().linear_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0_f32, acc());

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();

    // La escala tipográfica del Cockpit, no la de egui. Los valores por defecto
    // (12.5 / 18 / 9) no son ninguno de los siete tamaños del sistema.
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(FS_TITLE, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(FS_BODY, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(FS_BODY, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(FS_CAPTION, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(FS_FOOTNOTE, FontFamily::Monospace)),
    ]
    .into();

    // ESTO NO FUNDE NINGÚN HOVER, y el comentario que había aquí decía que sí.
    //
    // Decía que «las transiciones que egui hace por su cuenta —el fundido de un
    // control al pasar el cursor— pasan a durar lo que dura una transición de
    // hover en el CSS». Ese fundido NO EXISTE: `Widgets::style` (egui 0.29.1,
    // style.rs) elige `hovered`, `active` o `inactive` con un `if` seco y
    // devuelve una referencia, sin interpolar nada. Comprobado leyendo la
    // función, no supuesto.
    //
    // Lo que sí gobierna este valor es `Context::animate_bool` y compañía —los
    // desplegables que se abren, y las animaciones que Lucy pide a mano—, así
    // que la línea se queda y el número es bueno. Lo que se va es la promesa.
    //
    // Consecuencia que conviene tener escrita: los controles que Lucy pinta ella
    // misma cambian de color en UN fotograma, y ninguna constante de duración va
    // a arreglar eso. Hace falta interpolar en el sitio, como ya hace el
    // segmentado con la posición de su píldora.
    style.animation_time = DUR_FAST;

    // Espaciado. Los valores por defecto de egui son más apretados que el CSS
    // de Lucy, que respira bastante más.
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.menu_margin = egui::Margin::same(6.0);
    style.spacing.indent = 16.0;
    style.spacing.scroll.bar_width = 8.0;
    ctx.set_style(style);
}

/// Rótulo de instrumento: mono, versalitas y con tracking.
///
/// Es el detalle que más separa "tarjeta de plantilla" de "instrumento": el CSS
/// lo aplica a TODA cabecera de métrica y título de panel. egui no tiene
/// `letter-spacing` en `RichText`, así que hay que bajar a `LayoutJob` — por eso
/// vive aquí y no repetido en cada vista.
pub fn instrument_label(text: &str, color: Color32) -> egui::text::LayoutJob {
    // TRADUCE AQUÍ, como `fila` y `panel`. Por esta función pasan TODOS los
    // rótulos en versalitas de la aplicación —«DISCO SISTEMA», «SERVICIOS
    // DETENIDOS», «TOP PROCESOS», «CONVERSACIÓN», «EQUIPOS»— y estaban saliendo
    // en español en cualquier idioma. Envolverlos uno a uno eran veintitantos
    // sitios; aquí es una línea, y los que se añadan mañana entran solos.
    //
    // `theme` pasa a depender de `i18n`, que es una capa por encima de lo que
    // este módulo hacía. Se acepta a sabiendas: los dos son la interfaz del
    // mismo binario, y la alternativa —repetir el envoltorio en cada sitio— es
    // justo la que ya fallo una vez.
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &crate::i18n::tr(text).to_uppercase(),
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(FS_MICRO, egui::FontFamily::Monospace),
            color,
            extra_letter_spacing: FS_MICRO * LS_LABEL,
            ..Default::default()
        },
    );
    job
}

/// El contorno de un rectángulo de esquinas CONTINUAS.
///
/// ── QUÉ ES ESTO Y POR QUÉ NO LO DA egui ─────────────────────────────────────
///
/// `Rounding` en egui son cuatro radios, y el teselador los dibuja con
/// `add_circle_quadrant`: la esquina es un arco de circunferencia. Comprobado en
/// el código de epaint 0.29.1, y buscando `continuous`, `squircle` o
/// `superellipse` en todo el crate no hay una sola coincidencia. No es que esté
/// mal puesto: es que no existe la opción.
///
/// Un arco de circunferencia entra en la recta con un salto de curvatura — pasa
/// de cero a 1/r de golpe. El ojo lo registra aunque no sepa nombrarlo, y es
/// exactamente la diferencia entre un rectángulo redondeado y una esquina de
/// macOS: desde Big Sur, Apple usa una superelipse, donde la curvatura sube
/// progresivamente y la esquina «entra» en el lado en vez de pegarse a él.
///
/// ── LA CUENTA ────────────────────────────────────────────────────────────────
///
/// La superelipse es |x/a|^n + |y/b|^n = 1. Con n = 2 sale una elipse —el arco de
/// siempre—; según sube n, la forma se acerca al rectángulo y la transición se
/// reparte por más tramo del lado. Cinco es el valor que se usa habitualmente
/// para aproximar la esquina de Apple.
///
/// Se muestrea SOLO LA ESQUINA y no la figura entera: los lados son rectas y
/// gastar puntos en ellas es teselar de más para dibujar lo mismo.
///
/// ── DÓNDE USARLA Y DÓNDE NO ─────────────────────────────────────────────────
///
/// En las cuatro superficies GRANDES: una tarjeta, un panel, la burbuja del
/// operador y el compositor. En un chip de veinte píxeles la diferencia entre
/// una superelipse y un arco es menos de un píxel, así que se paga un polígono
/// de sesenta y cuatro vértices para dibujar lo mismo — ahí `Rounding` está bien.
///
/// El radio se acota a la mitad del lado corto: por encima de eso la figura deja
/// de tener lados rectos y las dos esquinas de un mismo lado se pisan.
pub fn superelipse(rect: egui::Rect, radio: f32) -> Vec<egui::Pos2> {
    /// Cuántos puntos por esquina. Dieciséis es donde deja de verse el polígono
    /// en una esquina de veinte píxeles; más es teselar para nadie.
    const POR_ESQUINA: usize = 16;
    /// El exponente. 2 sería una elipse —el arco de toda la vida—; 5 es la
    /// aproximación habitual de la esquina continua de Apple.
    const N: f32 = 5.0;

    let r = radio.min(rect.width() * 0.5).min(rect.height() * 0.5).max(0.0);
    if r <= 0.0 {
        let (a, b) = (rect.min, rect.max);
        return vec![a, egui::pos2(b.x, a.y), b, egui::pos2(a.x, b.y)];
    }

    // Un cuadrante en coordenadas locales, del lado hacia el vértice.
    let cuadrante: Vec<(f32, f32)> = (0..=POR_ESQUINA)
        .map(|i| {
            let t = i as f32 / POR_ESQUINA as f32 * std::f32::consts::FRAC_PI_2;
            // Forma paramétrica: x = cos(t)^(2/n), y = sin(t)^(2/n). Da un
            // reparto de puntos mucho más regular que despejar y de x, que los
            // amontona en los extremos justo donde no hacen falta.
            let e = 2.0 / N;
            (t.cos().abs().powf(e) * r, t.sin().abs().powf(e) * r)
        })
        .collect();

    let (x0, y0, x1, y1) = (rect.min.x, rect.min.y, rect.max.x, rect.max.y);
    let mut p = Vec::with_capacity(POR_ESQUINA * 4 + 4);

    // EL SENTIDO DE GIRO TIENE QUE SER EL MISMO EN LAS CUATRO ESQUINAS Y EN EL
    // RECORRIDO, y aquí no lo era: las esquinas se visitaban en horario y cada
    // una se trazaba en antihorario. El resultado es un LAZO en cada esquina —el
    // contorno se cruza consigo mismo— y lo que se veía eran muescas diagonales
    // y picos triangulares saliendo de las tarjetas del Dashboard.
    //
    // `convex_polygon` no reordena nada ni avisa: da por hecho que los puntos
    // vienen en orden. Un polígono cruzado no es un error para él, es una figura
    // rara que tesela sin quejarse.
    //
    // Horario en coordenadas de pantalla —con la Y hacia abajo— es: por el borde
    // de arriba hacia la derecha, bajar por el derecho, por el de abajo hacia la
    // izquierda, y subir por el izquierdo. Cada esquina se recorre en ese mismo
    // sentido, y por eso dos van al derecho y dos al revés: `cuadrante` va de
    // (r,0) a (0,r), y según en qué esquina se aplique eso avanza con el giro o
    // en contra.
    //
    // Se entra por el borde de arriba, justo antes de la esquina derecha.
    for (dx, dy) in cuadrante.iter().rev() {
        p.push(egui::pos2(x1 - r + dx, y0 + r - dy));
    }
    for (dx, dy) in &cuadrante {
        p.push(egui::pos2(x1 - r + dx, y1 - r + dy));
    }
    for (dx, dy) in cuadrante.iter().rev() {
        p.push(egui::pos2(x0 + r - dx, y1 - r + dy));
    }
    for (dx, dy) in &cuadrante {
        p.push(egui::pos2(x0 + r - dx, y0 + r - dy));
    }
    p
}

/// El área con signo de un polígono. Positiva si va en horario con la Y hacia
/// abajo, que es como se dibuja en pantalla.
///
/// SIRVE PARA DETECTAR UN CONTORNO CRUZADO, que es lo que ninguno de los otros
/// tests veía: un lazo cancela área contra sí mismo, así que un polígono que se
/// cruza mide mucho menos de lo que ocupa su silueta. Comprobar que el área es
/// la que debería ser es comprobar que no hay lazo.
#[cfg(test)]
fn area_con_signo(p: &[egui::Pos2]) -> f32 {
    let mut s = 0.0;
    for i in 0..p.len() {
        let a = p[i];
        let b = p[(i + 1) % p.len()];
        s += a.x * b.y - b.x * a.y;
    }
    s / 2.0
}

/// La sombra de lo que FLOTA, en una sola función.
///
/// ESTABA ESCRITA TRES VECES CON DOS VALORES. La de `popup_shadow` —que es la
/// que reciben los desplegables que construye egui— va a 0/12/32/115; los dos
/// menús de equipo que Lucy monta a mano sobre un `Area` llevaban su propia
/// copia a 0/6/18/90, la mitad de profundidad; y otros dos menús del mismo tipo
/// no llevaban ninguna. Cuatro menús que hacen lo mismo, flotando a tres alturas
/// distintas sobre la misma pantalla.
///
/// La profundidad es lo que dice QUÉ ESTÁ ENCIMA DE QUÉ. Con tres valores, esa
/// información deja de ser información: pasa a ser una decoración que varía.
///
/// Sigue siendo la ÚNICA sombra permitida —el resto de la profundidad del
/// Cockpit sale de superficies planas apiladas más bordes finos— y por eso vive
/// aquí y no en cada sitio que la quiera.
pub fn sombra_flotante() -> egui::epaint::Shadow {
    egui::epaint::Shadow {
        offset: egui::vec2(0.0, 12.0),
        blur: 32.0,
        spread: 0.0,
        color: Color32::from_black_alpha(115),
    }
}

/// El velo que se pone encima de una superficie mientras se la tiene pulsada.
///
/// UN SOLO NÚMERO PARA TODA LA APLICACIÓN, porque «pulsado» es un estado del
/// sistema visual y no de cada control: con un valor por sitio, dos botones
/// contiguos se hunden distinto y el que se hunde menos parece que no responde.
///
/// OSCURECE EN CLARO Y ACLARA EN OSCURO. Un velo negro sobre una superficie ya
/// oscura no se ve —#131A22 con un diez por ciento de negro sigue siendo casi
/// #131A22— así que en el tema de casa el hundido no existiría. Lo que dice
/// «esto se está pulsando» no es el color, es que la superficie CAMBIE; hacia
/// dónde cambia depende de dónde parta.
pub fn hundido() -> Color32 {
    if light() { Color32::from_black_alpha(20) } else { Color32::from_white_alpha(16) }
}

/// Color del texto según el nivel de importancia de una memoria (1-3), igual
/// que el navegador de memoria de la app.
pub fn importance_color(importance: i64) -> Color32 {
    match importance {
        i if i >= 3 => amber(),
        2 => acc(),
        _ => faint(),
    }
}

// AQUÍ ESTABAN `meter_color` Y `disk_color`, y se han ido.
//
// Repartían color por umbral —uno saltaba a rojo en 90 sin ámbar, el otro avisaba
// en ámbar desde 80— mientras la tira de alertas usaba 86 y la tarjeta del mismo
// volumen otro más. Tres cortes para el mismo número en la misma pantalla, y el
// mismo disco al 85 % salía verde, ámbar y sin avisar a la vez.
//
// Los sustituyó `main::color_nivel`, que solo PINTA lo que ya decidió
// `lucy_core::thresholds`. El compilador las daba por muertas hace tiempo y sus
// tests las mantenían vivas lo justo para que nadie lo notara. Se borran en vez
// de dejarlas con un `#[allow(dead_code)]`: mientras existan, son dos funciones
// con nombre razonable esperando a que alguien las llame y reponga el bug.

/// Color de un acierto de búsqueda semántica, por su parecido (0-1).
///
/// Escala PROPIA y no la de uso: en aquella más es peor, y aquí más es mejor —
/// usarla pintaba de rojo justo los resultados buenos. Los cortes son los de
/// `vectors::search`: por debajo de 0.25 ni se devuelve.
pub fn match_color(score: f32) -> Color32 {
    if score >= 0.6 {
        acc()
    } else if score >= 0.4 {
        txt2()
    } else {
        faint()
    }
}

/// Color de la barra de UN núcleo, que depende también de la carga del equipo.
///
/// UN núcleo al 100 % es normal: cualquier tarea de un solo hilo lo hace, y en
/// una máquina de 32 núcleos es el 3 % del equipo. El CSS pintaba peligro desde
/// el 85 % —el mismo rojo que un disco al 93 %— y una máquina sana enseñaba un
/// color de emergencia por trabajo rutinario. Ámbar desde 95, y rojo solo si el
/// EQUIPO también está cargado, que es el estado que de verdad significa
/// contención.
pub fn core_color(core_pct: f32, host_cpu: f32) -> Color32 {
    if core_pct >= 95.0 && host_cpu >= 78.0 {
        red()
    } else if core_pct >= 95.0 {
        amber()
    } else {
        acc()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_matches_the_cockpit_tokens() {
        // Pinta los valores contra `src/lib/styles/cockpit-tokens.css` de la app
        // real. Si allí se retoca el tema y aquí no, las dos interfaces divergen
        // en silencio — que es exactamente la deriva que ya pasó una vez, cuando
        // esto copiaba los tokens del Lucy clásico.
        let _t = serie();
        set_mode(Mode::Dark);
        assert_eq!(bg(), Color32::from_rgb(0x0A, 0x0E, 0x14), "--surface-0");
        assert_eq!(bg3(), Color32::from_rgb(0x13, 0x1A, 0x22), "--surface-2");
        assert_eq!(acc(), Color32::from_rgb(0x3D, 0xD6, 0xA4), "--accent");
        assert_eq!(txt(), Color32::from_rgb(0xE8, 0xED, 0xF2), "--text-primary");
        // `--text-faint` va con el valor SUBIDO, en los dos ficheros. El de la
        // hoja de estilos —`#5C6672`— daba 2.81 de contraste y no se leía; se
        // corrigió allí y aquí a la vez, que es de lo que va este test: que las
        // dos caras digan lo mismo. Si alguien lo baja en uno de los dos, esto
        // salta antes de que las interfaces diverjan.
        assert_eq!(faint(), Color32::from_rgb(0x7C, 0x88, 0x95), "--text-faint");
        assert_eq!(amber(), Color32::from_rgb(0xE5, 0xB5, 0x67), "--warning");
        assert_eq!(red(), Color32::from_rgb(0xF0, 0x6E, 0x6E), "--danger");
        assert_eq!(blue(), Color32::from_rgb(0x80, 0x98, 0xFF), "--user");
    }

    /// El modo es estado GLOBAL y los tests corren en paralelo en el mismo
    /// proceso. Sin esto, el que pone claro se lo cambia por debajo al que está
    /// comprobando el oscuro — un fallo intermitente que sale una vez de cada
    /// cincuenta y cuesta una tarde.
    fn serie() -> std::sync::MutexGuard<'static, ()> {
        static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let g = L.lock().unwrap_or_else(|e| e.into_inner());
        set_mode(Mode::Dark);
        g
    }

    #[test]
    fn el_tema_claro_es_el_de_la_v2_y_no_el_oscuro_invertido() {
        // Los mismos NOMBRES de token con la escala de luminosidad al revés, no
        // una inversión aritmética. Y el detalle que el CSS deja escrito: la
        // esmeralda de identidad NO pasa contraste como texto sobre blanco, así
        // que en claro se oscurece.
        //
        // HASTA `#0E7B5B`, Y NO HASTA `#12A379` COMO DECÍA ANTES. La frase del
        // CSS era correcta y el número no la cumplía: `#12A379` da 2.78 sobre el
        // lienzo claro, o sea que el oscurecido se quedó a medio camino y el
        // comentario lo daba por hecho. Quien lo comprueba es
        // `las_cuatro_paletas_se_ven_en_los_dos_modos`; aquí solo se fija el
        // valor para que las dos caras no se separen.
        let _t = serie();
        set_mode(Mode::Light);
        assert!(light());
        assert_eq!(bg(), Color32::from_rgb(0xF4, 0xF6, 0xFA), "--surface-0 claro");
        assert_eq!(acc(), Color32::from_rgb(0x0E, 0x7B, 0x5B), "--accent claro");
        assert_eq!(txt(), Color32::from_rgb(0x0E, 0x16, 0x21), "--text-primary claro");
        set_mode(Mode::Dark);
    }

    #[test]
    fn el_texto_se_ve_sobre_su_fondo_en_los_dos_temas() {
        // La prueba de que el tema claro no es un pegote: en cada modo, el texto
        // principal y el lienzo tienen que estar en extremos opuestos de la
        // luminosidad. Invertir uno y olvidar el otro deja gris sobre gris, y
        // eso no lo detecta ninguna comparación de valores sueltos.
        let _t = serie();
        let luz = |c: Color32| {
            0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32
        };
        for m in [Mode::Dark, Mode::Light] {
            set_mode(m);
            let (f, t) = (luz(bg()), luz(txt()));
            assert!(
                (f - t).abs() > 120.0,
                "{m:?}: fondo {f:.0} y texto {t:.0} se parecen demasiado"
            );
        }
        set_mode(Mode::Dark);
    }

    #[test]
    fn el_modo_va_y_vuelve_por_su_clave() {
        // Es lo que se guarda en disco. Una clave que no redondea deja al
        // operador con el tema que no eligió en cada arranque.
        for m in Mode::ALL {
            assert_eq!(Mode::from_key(m.key()), m, "{m:?}");
        }
        // Y algo que no se entiende cae en oscuro, que es el tema de la casa:
        // adivinar claro le pondría una ventana blanca a quien nunca la pidió.
        assert_eq!(Mode::from_key("azul"), Mode::Dark);
        assert_eq!(Mode::from_key(""), Mode::Dark);
    }

    #[test]
    fn un_solo_nucleo_al_maximo_no_es_una_emergencia() {
        // La lección que el CSS lleva escrita: en una máquina de 32 núcleos, uno
        // clavado al 100 % es una tarea de un hilo, no un incidente.
        assert_eq!(core_color(100.0, 12.0), amber(), "el equipo está tranquilo");
        assert_eq!(core_color(100.0, 80.0), red(), "el equipo TAMBIÉN está cargado");
        assert_eq!(core_color(94.9, 90.0), acc(), "por debajo de 95 no se tiñe");
        assert_eq!(core_color(60.0, 12.0), acc());
    }

    // ── Contraste ────────────────────────────────────────────────────────────
    //
    // ESTE ES EL TEST QUE FALTABA, y su ausencia es la razón de que hubiera que
    // arreglar nada. El que sí había —`el_texto_se_ve_sobre_su_fondo_en_los_dos_
    // temas`— compara la luminancia de `bg` con la de `txt` y pide 120 de
    // diferencia: pasaba holgado mientras `faint` iba por 2.74 y el acento claro
    // por 2.78. Comprobaba el peldaño más fácil de la escalera y ninguno de los
    // que fallaban.

    /// La fórmula de contraste de la WCAG. Es aritmética, no una opinión.
    fn contraste(a: Color32, b: Color32) -> f32 {
        let canal = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        let lum = |c: Color32| {
            0.2126 * canal(c.r()) + 0.7152 * canal(c.g()) + 0.0722 * canal(c.b())
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// Las cuatro superficies sobre las que puede caer algo.
    fn superficies() -> [Color32; 4] {
        [bg(), bg2(), bg3(), bg4()]
    }

    /// El mínimo de la norma para texto normal. Todo lo que se lee tiene que
    /// llegar aquí, contra CUALQUIERA de las superficies — no contra la mejor.
    const AA: f32 = 4.5;

    #[test]
    fn toda_la_escalera_de_texto_se_lee_en_los_dos_temas() {
        let _t = serie();
        for m in [Mode::Dark, Mode::Light] {
            set_mode(m);
            for (nombre, color) in
                [("txt", txt()), ("txt2", txt2()), ("txt3", txt3()), ("faint", faint())]
            {
                for (i, s) in superficies().iter().enumerate() {
                    let r = contraste(color, *s);
                    assert!(
                        r >= AA,
                        "{m:?}: {nombre} sobre la superficie {i} da {r:.2}, y hace falta {AA}"
                    );
                }
            }
        }
        set_mode(Mode::Dark);
    }

    #[test]
    fn la_escalera_sigue_siendo_una_escalera() {
        // Subir `faint` hasta que se lea lo acerca a `txt3`. Que llegue a
        // ADELANTARLO sería otra cosa: entonces el peldaño flojo pintaría más
        // fuerte que el de encima y la jerarquía diría lo contrario de lo que
        // significa. Con margen, para que un retoque futuro no los cruce por
        // centésimas.
        let _t = serie();
        for m in [Mode::Dark, Mode::Light] {
            set_mode(m);
            let c = |x: Color32| contraste(x, bg());
            assert!(c(txt()) > c(txt2()) + 0.5, "{m:?}: txt no manda sobre txt2");
            assert!(c(txt2()) > c(txt3()) + 0.5, "{m:?}: txt2 no manda sobre txt3");
            assert!(c(txt3()) > c(faint()) + 0.5, "{m:?}: txt3 no manda sobre faint");
        }
        set_mode(Mode::Dark);
    }

    #[test]
    fn las_cuatro_paletas_se_ven_en_los_dos_modos() {
        // EL ACENTO NO ES SOLO DECORACIÓN. De sus noventa y seis usos, seis son
        // texto —el nombre de Lucy en la cabecera, el chip «privado» a diez
        // puntos y medio— así que el listón es el de texto y no el de gráfico.
        //
        // Y se comprueban las CUATRO: el operador elige, y una paleta que solo
        // se ha mirado en oscuro es una paleta que falla el día que alguien pasa
        // a claro. Violeta y Magenta ya pasaban; Esmeralda y Cian no, y por eso
        // se tocaron.
        let _t = serie();
        for (i, p) in PALETAS.iter().enumerate() {
            for (m, base, hover) in [
                (Mode::Dark, p.oscuro, p.oscuro_hover),
                (Mode::Light, p.claro, p.claro_hover),
            ] {
                set_mode(m);
                set_paleta(i);
                for (que, (r, g, b)) in [("acento", base), ("hover", hover)] {
                    let c = Color32::from_rgb(r, g, b);
                    for (j, s) in superficies().iter().enumerate() {
                        let v = contraste(c, *s);
                        assert!(
                            v >= AA,
                            "{} en {m:?}: el {que} sobre la superficie {j} da {v:.2}, \
                             y hace falta {AA}",
                            p.nombre
                        );
                    }
                }
            }
        }
        set_paleta(0);
        set_mode(Mode::Dark);
    }

    #[test]
    fn el_hover_del_acento_se_distingue_del_reposo() {
        // Si el hover empatara con el reposo, el control no contestaría al ratón
        // — y al oscurecer los dos acentos claros para ganar contraste, el hover
        // es justo lo que se podía haber quedado plano.
        let _t = serie();
        for (i, p) in PALETAS.iter().enumerate() {
            set_paleta(i);
            for m in [Mode::Dark, Mode::Light] {
                set_mode(m);
                assert_ne!(acc(), acc_hover(), "{} en {m:?}", p.nombre);
            }
        }
        set_paleta(0);
        set_mode(Mode::Dark);
    }

    #[test]
    fn la_superelipse_se_queda_dentro_de_su_rectangulo() {
        // Un contorno que se sale pinta por encima de lo que tenga al lado, y en
        // una rejilla de tarjetas eso es un borde que invade a su vecina.
        let r = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(200.0, 120.0));
        let p = superelipse(r, 14.0);
        assert!(p.len() > 32, "muy pocos puntos para una curva: {}", p.len());
        for q in &p {
            assert!(
                r.contains(*q),
                "el punto {q:?} se sale de {r:?}"
            );
        }
    }

    #[test]
    fn el_contorno_no_se_cruza_consigo_mismo() {
        // EL TEST QUE FALTABA, y su ausencia costó una regresión que se vio en
        // pantalla: las esquinas se visitaban en horario y cada una se trazaba
        // en antihorario, así que el contorno hacía un LAZO en cada esquina. En
        // el Dashboard salían muescas diagonales y picos triangulares.
        //
        // Los otros cuatro tests pasaban tan contentos: comprobaban que los
        // puntos están dentro del rect, que tocan los cuatro lados y que la
        // esquina llega cerca del vértice. Un contorno cruzado cumple las tres.
        //
        // El área con signo sí lo ve. Un lazo cancela área contra sí mismo, así
        // que la figura mide mucho menos que su silueta; y el signo, además,
        // dice el sentido de giro.
        let lado = 200.0_f32;
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(lado, lado));
        let radio = 24.0_f32;
        let a = area_con_signo(&superelipse(r, radio));

        assert!(a > 0.0, "el contorno va en sentido antihorario: área {a}");
        // Una superelipse de exponente 5 recorta muy poco de cada esquina: menos
        // del 4 % del área total con este radio. Con un lazo se perdería mucho
        // más que eso.
        let total = lado * lado;
        assert!(
            a > total * 0.94,
            "el área es {a:.0} de {total:.0}: falta demasiada, el contorno se cruza"
        );
        assert!(a <= total, "el área es mayor que el rectángulo que la contiene");
    }

    #[test]
    fn el_sentido_de_giro_no_depende_del_radio() {
        // El orden de los cuatro tramos es fijo, pero el de dentro de cada uno
        // cambia según la esquina. Un radio distinto no debería poder invertir
        // nada — y si algún día alguien toca el bucle, esto lo dice.
        let r = egui::Rect::from_min_size(egui::pos2(5.0, 7.0), egui::vec2(160.0, 90.0));
        for radio in [1.0_f32, 6.0, 12.0, 24.0, 45.0] {
            let a = area_con_signo(&superelipse(r, radio));
            assert!(a > 0.0, "con radio {radio} el giro se invierte: área {a}");
            assert!(
                a > r.width() * r.height() * 0.85,
                "con radio {radio} el área cae a {a:.0}: hay lazo"
            );
        }
    }

    #[test]
    fn la_superelipse_toca_los_cuatro_lados() {
        // Si no llegara a los lados, la figura sería más pequeña que su rect y
        // las tarjetas encogerían visualmente sin que nadie lo hubiera pedido.
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 60.0));
        let p = superelipse(r, 12.0);
        let cerca = |a: f32, b: f32| (a - b).abs() < 0.01;
        assert!(p.iter().any(|q| cerca(q.x, r.min.x)), "no toca el lado izquierdo");
        assert!(p.iter().any(|q| cerca(q.x, r.max.x)), "no toca el derecho");
        assert!(p.iter().any(|q| cerca(q.y, r.min.y)), "no toca el de arriba");
        assert!(p.iter().any(|q| cerca(q.y, r.max.y)), "no toca el de abajo");
    }

    #[test]
    fn la_esquina_continua_es_mas_llena_que_un_arco() {
        // LO QUE DISTINGUE LA FIGURA. En el punto medio de la esquina, un arco de
        // circunferencia está a r·(1−√2/2) ≈ 0.293·r del vértice. La superelipse
        // se acerca más —la curvatura se reparte por el lado en vez de
        // concentrarse— y eso es exactamente la diferencia que se ve.
        //
        // Sin esta comprobación, un exponente puesto en 2 por error daría una
        // elipse: la misma esquina de siempre, con el coste de teselar sesenta y
        // cuatro puntos para nada, y nadie lo notaría leyendo el código.
        let lado = 100.0_f32;
        let radio = 20.0_f32;
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(lado, lado));
        let p = superelipse(r, radio);
        let vertice = egui::pos2(lado, 0.0); // la esquina superior derecha
        let mas_cerca = p
            .iter()
            .map(|q| q.distance(vertice))
            .fold(f32::INFINITY, f32::min);
        let arco = radio * (1.0 - std::f32::consts::FRAC_1_SQRT_2);
        assert!(
            mas_cerca < arco * 0.85,
            "la esquina se queda a {mas_cerca:.2} del vértice y un arco llega a \
             {arco:.2}: esto no es una superelipse, es un arco"
        );
    }

    #[test]
    fn un_radio_imposible_no_deforma_la_figura() {
        // Un radio mayor que la mitad del lado corto haría que las dos esquinas
        // de un mismo lado se pisaran. Se acota, y lo que sale sigue estando
        // dentro del rect.
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(40.0, 20.0));
        for q in superelipse(r, 999.0) {
            assert!(r.contains(q), "{q:?} fuera de {r:?}");
        }
        // Y con radio cero salen las cuatro esquinas y nada más.
        assert_eq!(superelipse(r, 0.0).len(), 4);
    }

    #[test]
    fn una_capsula_es_media_altura() {
        assert_eq!(capsule(24.0), Rounding::same(12.0));
        assert_eq!(capsule(30.0), Rounding::same(15.0));
    }

    #[test]
    fn la_capsula_sale_concentrica_sola() {
        // ESTA ES LA RAZÓN DE QUE `capsule` EXISTA, y no que ahorre una
        // división. Si el grupo es la cápsula del alto TOTAL y la píldora es la
        // cápsula del alto INTERIOR, el aro de aire sale del mismo grosor en
        // todo el contorno sin que nadie tenga que calcularlo:
        //
        //     capsule(H + 2P) - P  =  (H + 2P)/2 - P  =  H/2  =  capsule(H)
        //
        // Con los radios escritos a mano no salía: el segmentado tenía 8 fuera,
        // 3 de inserción y 6 dentro, donde tocaba 5. Un píxel en un control de
        // treinta, y se ve — el aro era más grueso en los lados rectos que en
        // las esquinas.
        for (h, p) in [(24.0_f32, 3.0_f32), (18.0, 2.0), (32.0, 4.0), (40.0, 6.0)] {
            assert_eq!(
                concentrico(capsule(h + p * 2.0).nw, p),
                capsule(h),
                "con alto {h} e inserción {p} el aro no es constante"
            );
        }
    }

    #[test]
    fn bg3_no_sirve_como_estado_de_una_superficie_bg2() {
        // LA REGLA, ESCRITA COMO ASERCIÓN. No se puede comprobar desde aquí
        // sobre qué superficie cae cada control —eso vive en `main.rs`— pero sí
        // se puede dejar clavado el HECHO del que salió la familia de fallos:
        // en el tema claro, `bg2` y `bg3` son el mismo color. Quien vaya a
        // apoyar un hover en esa diferencia se encuentra esto.
        //
        // Si algún día se separan, este test falla y hay que venir a leer por
        // qué existía — que es exactamente lo que se quiere de un test así.
        let _t = serie();
        set_mode(Mode::Light);
        assert_eq!(bg2(), bg3(), "en claro eran el mismo blanco; si ya no lo son, relee la nota de bg3");
        assert_ne!(bg2(), bg4(), "bg4 es el único peldaño que sirve de estado en los dos temas");
        set_mode(Mode::Dark);
        assert_ne!(bg2(), bg3(), "en oscuro sí se distinguen");
        assert_ne!(bg2(), bg4());
    }

    #[test]
    fn el_rail_y_su_hover_no_son_el_mismo_color_en_ningun_tema() {
        // EL FALLO QUE ESTO CIERRA. El rail se rellena con `bg2()` y su hover
        // pintaba `bg3()` — y en el tema claro los dos son `#FFFFFF` EXACTO, así
        // que pasar el ratón por los ocho módulos no hacía absolutamente nada.
        // Era el único de los once hover de la aplicación que no se veía, y
        // estaba en el elemento que más se usa.
        //
        // Que `bg2` y `bg3` coincidan en claro no es un error: la escalera de
        // superficies tiene cuatro peldaños y en un tema claro no hay cuatro
        // blancos distintos que sigan siendo blancos. Lo que era un error es
        // apoyar una señal en una diferencia que ese tema no tiene.
        let _t = serie();
        for m in [Mode::Dark, Mode::Light] {
            set_mode(m);
            assert_ne!(bg2(), bg4(), "{m:?}: el hover del rail sería invisible");
        }
        set_mode(Mode::Dark);
    }

    #[test]
    fn la_escala_tipografica_esta_ordenada_y_sin_empates() {
        // Dos papeles con el mismo número no son dos papeles: son uno con dos
        // nombres, y el segundo se usa creyendo que cambia algo.
        let escala = [
            ("MICRO", FS_MICRO),
            ("BAR", FS_BAR),
            ("CAPTION", FS_CAPTION),
            ("FOOTNOTE", FS_FOOTNOTE),
            ("BODY", FS_BODY),
            ("HEADING", FS_HEADING),
            ("TITLE", FS_TITLE),
            ("DISPLAY", FS_DISPLAY),
            ("HERO", FS_HERO),
        ];
        for par in escala.windows(2) {
            let ((na, a), (nb, b)) = (par[0], par[1]);
            assert!(a < b, "{na} ({a}) no va por debajo de {nb} ({b})");
        }
    }

    #[test]
    fn importance_maps_the_three_levels_and_clamps_outside_them() {
        assert_eq!(importance_color(3), amber());
        assert_eq!(importance_color(9), amber(), "por encima de 3 sigue siendo alta");
        assert_eq!(importance_color(2), acc());
        assert_eq!(importance_color(1), faint());
        assert_eq!(importance_color(0), faint(), "un 0 inesperado no debe teñirse de alta");
    }

    #[test]
    fn el_rotulo_de_instrumento_va_en_versalitas_y_con_tracking() {
        let job = instrument_label("Disco sistema", faint());
        assert_eq!(job.text, "DISCO SISTEMA");
        let f = &job.sections[0].format;
        assert!(f.extra_letter_spacing > 0.0, "sin tracking no es un instrumento");
        assert_eq!(f.font_id.family, egui::FontFamily::Monospace);
    }
}
