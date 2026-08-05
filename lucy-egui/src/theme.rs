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

// ── Superficies · escalera de elevación ──────────────────────────────────────
// La profundidad del Cockpit sale de superficies planas apiladas más bordes
// finos, nunca de sombras ni de brillos. Solo los menús flotantes llevan sombra.

/// `--surface-0` — lienzo de la página.
pub const BG: Color32 = Color32::from_rgb(0x0A, 0x0E, 0x14);
/// `--surface-1` — cromo: rail, cabecera, pie, y los paneles grandes.
pub const BG2: Color32 = Color32::from_rgb(0x0E, 0x13, 0x19);
/// `--surface-2` — tarjetas y controles en reposo.
pub const BG3: Color32 = Color32::from_rgb(0x13, 0x1A, 0x22);
/// `--surface-3` — hover, seleccionado, flotante, y el carril de las barras.
pub const BG4: Color32 = Color32::from_rgb(0x18, 0x20, 0x2A);

// ── Bordes ───────────────────────────────────────────────────────────────────
// Blanco translúcido, no un color sólido: así el mismo borde funciona sobre las
// cuatro superficies sin recalcularlo, que es justo lo que hace el CSS.

/// `--border` — `rgba(255,255,255,0.06)`. Premultiplicado: blanco con alfa `a`
/// es `(a,a,a,a)`, y el constructor premultiplicado es el único que es `const`.
pub const BDR: Color32 = Color32::from_rgba_premultiplied(15, 15, 15, 15);
/// `--border-strong` — `rgba(255,255,255,0.09)`.
pub const BDR2: Color32 = Color32::from_rgba_premultiplied(23, 23, 23, 23);

// ── Texto · escalera de contraste ────────────────────────────────────────────

/// `--text-primary` — títulos, cifras héroe, etiquetas activas.
pub const TXT: Color32 = Color32::from_rgb(0xE8, 0xED, 0xF2);
/// `--text-secondary` — cuerpo, narrativa.
pub const TXT2: Color32 = Color32::from_rgb(0xC7, 0xD0, 0xD9);
/// `--text-muted` — etiquetas, chips secundarios, marcas de tiempo.
pub const TXT3: Color32 = Color32::from_rgb(0x98, 0xA2, 0xAE);
/// `--text-faint` — rótulos de instrumento, subtítulos, pistas.
pub const FAINT: Color32 = Color32::from_rgb(0x5C, 0x66, 0x72);
/// `--text-disabled` — inactivo.
#[allow(dead_code)]
pub const DISABLED: Color32 = Color32::from_rgb(0x3C, 0x45, 0x50);

// ── Acento · la esmeralda de Lucy ────────────────────────────────────────────
// UN acento por vista. Nav activo, actividad del agente, progreso, "hecho".

/// `--accent`.
pub const ACC: Color32 = Color32::from_rgb(0x3D, 0xD6, 0xA4);
/// `--accent-hover`.
pub const ACC_HOVER: Color32 = Color32::from_rgb(0x34, 0xC2, 0x96);
/// `--accent-ink` — texto SOBRE un relleno de acento sólido.
pub const ACC_INK: Color32 = Color32::from_rgb(0x07, 0x13, 0x0E);
/// `--accent-bg` — `rgba(61,214,164,0.12)`: chip teñido, píldora activa.
pub const ACC_BG: Color32 = Color32::from_rgba_premultiplied(7, 25, 19, 31);
/// `--accent-line` — `rgba(61,214,164,0.28)`.
#[allow(dead_code)]
pub const ACC_LINE: Color32 = Color32::from_rgba_premultiplied(17, 59, 45, 71);

// ── Semánticos ───────────────────────────────────────────────────────────────
// Contenidos, no de neón: el Cockpit se apoya en el contraste, no en el color.

/// `--user` — el operador. También la subida de red.
pub const BLUE: Color32 = Color32::from_rgb(0x80, 0x98, 0xFF);
/// `--warning`.
pub const AMBER: Color32 = Color32::from_rgb(0xE5, 0xB5, 0x67);
/// `--warning-bg` — `rgba(229,181,103,0.12)`.
pub const AMBER_BG: Color32 = Color32::from_rgba_premultiplied(27, 21, 12, 31);
/// `--danger`.
pub const RED: Color32 = Color32::from_rgb(0xF0, 0x6E, 0x6E);
/// `--danger-bg` — `rgba(240,110,110,0.12)`.
pub const RED_BG: Color32 = Color32::from_rgba_premultiplied(28, 13, 13, 31);

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

/// Aplica el tema completo al contexto. Se llama una vez al arrancar.
pub fn apply(ctx: &egui::Context) {
    load_system_fonts(ctx);

    let mut visuals = egui::Visuals::dark();

    visuals.override_text_color = Some(TXT2);
    visuals.panel_fill = BG;
    visuals.window_fill = BG3;
    // `extreme_bg_color` es el fondo de los campos de texto Y el CARRIL de las
    // barras de progreso. Con el color del panel, la parte vacía de la barra
    // desaparecía: un 1 % de CPU se veía como un puntito verde suelto en vez de
    // como una barra casi vacía. `--surface-3` es lo que usa el CSS.
    visuals.extreme_bg_color = BG4;
    visuals.faint_bg_color = BG2;
    visuals.code_bg_color = BG3;
    visuals.hyperlink_color = ACC;
    visuals.window_stroke = Stroke::new(1.0_f32, BDR2);
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
    w.inactive.bg_fill = BG3;
    w.inactive.weak_bg_fill = BG3;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, BDR);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, TXT3);
    w.inactive.rounding = Rounding::same(ROUND);

    // Con el cursor encima — sube un escalón de fondo, el borde se aclara.
    w.hovered.bg_fill = BG4;
    w.hovered.weak_bg_fill = BG4;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, BDR2);
    w.hovered.fg_stroke = Stroke::new(1.0_f32, TXT);
    w.hovered.rounding = Rounding::same(ROUND);

    // Pulsado / activo — aquí entra el acento, y solo aquí.
    w.active.bg_fill = BG4;
    w.active.weak_bg_fill = BG4;
    w.active.bg_stroke = Stroke::new(1.0_f32, ACC);
    w.active.fg_stroke = Stroke::new(1.0_f32, TXT);
    w.active.rounding = Rounding::same(ROUND);

    // No interactivo — texto plano, etiquetas.
    w.noninteractive.bg_fill = BG;
    w.noninteractive.weak_bg_fill = BG;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, BDR);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TXT2);
    w.noninteractive.rounding = Rounding::same(ROUND);

    // Abierto (desplegables desplegados).
    w.open.bg_fill = BG3;
    w.open.weak_bg_fill = BG3;
    w.open.bg_stroke = Stroke::new(1.0_f32, BDR2);
    w.open.fg_stroke = Stroke::new(1.0_f32, TXT);
    w.open.rounding = Rounding::same(ROUND);

    visuals.selection.bg_fill = ACC.linear_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACC);

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
        i if i >= 3 => AMBER,
        2 => ACC,
        _ => FAINT,
    }
}

/// Color de la barra de una métrica general (CPU, RAM, disco de sistema).
///
/// Acento hasta el 90 %, y solo entonces peligro. Los umbrales no son uno solo
/// para todo el panel a propósito — ver `core_color`.
pub fn meter_color(pct: f32) -> Color32 {
    if pct >= 90.0 {
        RED
    } else {
        ACC
    }
}

/// Color de un acierto de búsqueda semántica, por su parecido (0-1).
///
/// Escala PROPIA y no la de uso: en aquella más es peor, y aquí más es mejor —
/// usarla pintaba de rojo justo los resultados buenos. Los cortes son los de
/// `vectors::search`: por debajo de 0.25 ni se devuelve.
pub fn match_color(score: f32) -> Color32 {
    if score >= 0.6 {
        ACC
    } else if score >= 0.4 {
        TXT2
    } else {
        FAINT
    }
}

/// Color de un volumen: rojo desde 90, ámbar desde 80.
///
/// Un disco lleno no se arregla solo, así que avisa antes que la CPU: el aviso
/// llega cuando todavía queda margen para actuar.
pub fn disk_color(pct: f32) -> Color32 {
    if pct >= 90.0 {
        RED
    } else if pct >= 80.0 {
        AMBER
    } else {
        ACC
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
        RED
    } else if core_pct >= 95.0 {
        AMBER
    } else {
        ACC
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
        assert_eq!(BG, Color32::from_rgb(0x0A, 0x0E, 0x14), "--surface-0");
        assert_eq!(BG3, Color32::from_rgb(0x13, 0x1A, 0x22), "--surface-2");
        assert_eq!(ACC, Color32::from_rgb(0x3D, 0xD6, 0xA4), "--accent");
        assert_eq!(TXT, Color32::from_rgb(0xE8, 0xED, 0xF2), "--text-primary");
        assert_eq!(FAINT, Color32::from_rgb(0x5C, 0x66, 0x72), "--text-faint");
        assert_eq!(AMBER, Color32::from_rgb(0xE5, 0xB5, 0x67), "--warning");
        assert_eq!(RED, Color32::from_rgb(0xF0, 0x6E, 0x6E), "--danger");
        assert_eq!(BLUE, Color32::from_rgb(0x80, 0x98, 0xFF), "--user");
    }

    #[test]
    fn un_solo_nucleo_al_maximo_no_es_una_emergencia() {
        // La lección que el CSS lleva escrita: en una máquina de 32 núcleos, uno
        // clavado al 100 % es una tarea de un hilo, no un incidente.
        assert_eq!(core_color(100.0, 12.0), AMBER, "el equipo está tranquilo");
        assert_eq!(core_color(100.0, 80.0), RED, "el equipo TAMBIÉN está cargado");
        assert_eq!(core_color(94.9, 90.0), ACC, "por debajo de 95 no se tiñe");
        assert_eq!(core_color(60.0, 12.0), ACC);
    }

    #[test]
    fn los_umbrales_de_disco_avisan_antes_que_los_de_cpu() {
        assert_eq!(disk_color(79.9), ACC);
        assert_eq!(disk_color(80.0), AMBER);
        assert_eq!(disk_color(89.9), AMBER);
        assert_eq!(disk_color(90.0), RED);
        // Y la barra general no se tiñe en ese mismo punto: un 80 % de RAM no es
        // lo mismo que un 80 % de disco.
        assert_eq!(meter_color(80.0), ACC);
        assert_eq!(meter_color(90.0), RED);
    }

    #[test]
    fn importance_maps_the_three_levels_and_clamps_outside_them() {
        assert_eq!(importance_color(3), AMBER);
        assert_eq!(importance_color(9), AMBER, "por encima de 3 sigue siendo alta");
        assert_eq!(importance_color(2), ACC);
        assert_eq!(importance_color(1), FAINT);
        assert_eq!(importance_color(0), FAINT, "un 0 inesperado no debe teñirse de alta");
    }

    #[test]
    fn el_rotulo_de_instrumento_va_en_versalitas_y_con_tracking() {
        let job = instrument_label("Disco sistema", FAINT);
        assert_eq!(job.text, "DISCO SISTEMA");
        let f = &job.sections[0].format;
        assert!(f.extra_letter_spacing > 0.0, "sin tracking no es un instrumento");
        assert_eq!(f.font_id.family, egui::FontFamily::Monospace);
    }
}
