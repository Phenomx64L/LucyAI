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
pub fn faint() -> Color32 {
    if light() { Color32::from_rgb(0x87, 0x92, 0xA0) } else { Color32::from_rgb(0x5C, 0x66, 0x72) }
}
/// `--text-disabled` — inactivo.
#[allow(dead_code)]
pub fn disabled() -> Color32 {
    if light() { Color32::from_rgb(0xB4, 0xBD, 0xC7) } else { Color32::from_rgb(0x3C, 0x45, 0x50) }
}

// ── Acento · la esmeralda de Lucy ────────────────────────────────────────────
// UN acento por vista. Nav activo, actividad del agente, progreso, "hecho".

/// `--accent`.
pub fn acc() -> Color32 {
    if light() { Color32::from_rgb(0x12, 0xA3, 0x79) } else { Color32::from_rgb(0x3D, 0xD6, 0xA4) }
}
/// `--accent-hover`.
pub fn acc_hover() -> Color32 {
    if light() { Color32::from_rgb(0x0E, 0x8A, 0x66) } else { Color32::from_rgb(0x34, 0xC2, 0x96) }
}
/// `--accent-ink` — texto SOBRE un relleno de acento sólido.
pub fn acc_ink() -> Color32 {
    if light() { Color32::from_rgb(0xFF, 0xFF, 0xFF) } else { Color32::from_rgb(0x07, 0x13, 0x0E) }
}
/// `--accent-bg` — `rgba(61,214,164,0.12)`: chip teñido, píldora activa.
pub fn acc_bg() -> Color32 {
    if light() { Color32::from_rgba_premultiplied(2, 20, 15, 31) } else { Color32::from_rgba_premultiplied(7, 25, 19, 31) }
}
/// `--accent-line` — `rgba(61,214,164,0.28)`.
#[allow(dead_code)]
pub fn acc_line() -> Color32 {
    if light() { Color32::from_rgba_premultiplied(6, 55, 41, 87) } else { Color32::from_rgba_premultiplied(17, 59, 45, 71) }
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
/// `--ls-label` — `0.09em`, el tracking de los rótulos de instrumento.
pub const LS_LABEL: f32 = 0.09;

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
    // plano — `--shadow-pop`.
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 12.0),
        blur: 32.0,
        spread: 0.0,
        color: Color32::from_black_alpha(115),
    };

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

    // Las transiciones que egui hace por su cuenta —el fundido de un control al
    // pasar el cursor— pasan a durar lo que dura una transición de hover en el
    // CSS. Es el mismo `--dur-fast` de `.host-pill:hover` y `.ghost-btn:hover`,
    // y con el valor por defecto de egui los controles respondían a otra
    // velocidad que todo lo demás.
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
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &text.to_uppercase(),
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

/// Color del texto según el nivel de importancia de una memoria (1-3), igual
/// que el navegador de memoria de la app.
pub fn importance_color(importance: i64) -> Color32 {
    match importance {
        i if i >= 3 => amber(),
        2 => acc(),
        _ => faint(),
    }
}

/// Color de la barra de una métrica general (CPU, RAM, disco de sistema).
///
/// Acento hasta el 90 %, y solo entonces peligro. Los umbrales no son uno solo
/// para todo el panel a propósito — ver `core_color`.
pub fn meter_color(pct: f32) -> Color32 {
    if pct >= 90.0 {
        red()
    } else {
        acc()
    }
}

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

/// Color de un volumen: rojo desde 90, ámbar desde 80.
///
/// Un disco lleno no se arregla solo, así que avisa antes que la CPU: el aviso
/// llega cuando todavía queda margen para actuar.
pub fn disk_color(pct: f32) -> Color32 {
    if pct >= 90.0 {
        red()
    } else if pct >= 80.0 {
        amber()
    } else {
        acc()
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
        assert_eq!(faint(), Color32::from_rgb(0x5C, 0x66, 0x72), "--text-faint");
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
        // que en claro se oscurece a #12A379 — mismo carácter, legible.
        let _t = serie();
        set_mode(Mode::Light);
        assert!(light());
        assert_eq!(bg(), Color32::from_rgb(0xF4, 0xF6, 0xFA), "--surface-0 claro");
        assert_eq!(acc(), Color32::from_rgb(0x12, 0xA3, 0x79), "--accent claro");
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

    #[test]
    fn los_umbrales_de_disco_avisan_antes_que_los_de_cpu() {
        assert_eq!(disk_color(79.9), acc());
        assert_eq!(disk_color(80.0), amber());
        assert_eq!(disk_color(89.9), amber());
        assert_eq!(disk_color(90.0), red());
        // Y la barra general no se tiñe en ese mismo punto: un 80 % de RAM no es
        // lo mismo que un 80 % de disco.
        assert_eq!(meter_color(80.0), acc());
        assert_eq!(meter_color(90.0), red());
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
