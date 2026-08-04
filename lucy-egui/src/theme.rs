//! El sistema de diseño de Lucy, portado del CSS a egui.
//!
//! Va PRIMERO, antes de migrar vistas. Construir veinte vistas con el estilo por
//! defecto de egui y re-tematizarlas después es tirar ese trabajo: en modo
//! inmediato el estilo no es una hoja aparte que se cambia al final, está en
//! cada llamada de dibujo. Fijarlo ahora hace que cada vista que venga salga ya
//! con el aspecto correcto.
//!
//! Los valores NO son inventados: salen de `src/routes/page.css:68` de la app
//! real, que es la única fuente de verdad del tema oscuro. Si allí cambian,
//! aquí hay que cambiarlos — y por eso cada constante lleva su nombre CSS al
//! lado, para que la correspondencia se pueda comprobar de un vistazo en vez de
//! adivinarla.

use eframe::egui::{self, Color32, Rounding, Stroke};

// ── Paleta ───────────────────────────────────────────────────────────────────
// Nombres exactos del CSS para que grep encuentre los dos lados.

/// `--bg` — fondo de la aplicación.
pub const BG: Color32 = Color32::from_rgb(0x0f, 0x11, 0x1a);
/// `--bg2` — paneles y barras.
pub const BG2: Color32 = Color32::from_rgb(0x16, 0x18, 0x22);
/// `--bg3` — controles en reposo, burbujas.
pub const BG3: Color32 = Color32::from_rgb(0x1e, 0x20, 0x30);
/// `--bg4` — control con el cursor encima.
pub const BG4: Color32 = Color32::from_rgb(0x26, 0x28, 0x38);

/// `--acc` — el verde de Lucy. Acción primaria, estado activo, foco.
pub const ACC: Color32 = Color32::from_rgb(0x10, 0xb9, 0x81);

/// `--txt` — texto principal.
pub const TXT: Color32 = Color32::from_rgb(0xe2, 0xe8, 0xf0);
/// `--txt2` — secundario: metadatos, etiquetas.
pub const TXT2: Color32 = Color32::from_rgb(0x94, 0xa3, 0xb8);
/// `--txt3` — terciario: pistas, marcas de tiempo.
pub const TXT3: Color32 = Color32::from_rgb(0x7c, 0x8a, 0xa3);

/// `--bdr` — borde normal.
pub const BDR: Color32 = Color32::from_rgb(0x1e, 0x29, 0x3b);
/// `--bdr2` — borde destacado.
pub const BDR2: Color32 = Color32::from_rgb(0x33, 0x41, 0x55);

/// `--blue` / `--purple` / `--amber` / `--red` — semánticos.
pub const BLUE: Color32 = Color32::from_rgb(0x60, 0xa5, 0xfa);
/// `--purple` — en la app marca la actividad del agente (`--sem-agent`).
/// Todavía sin uso aquí: la vista del cockpit con el plan y la traza es de las
/// que faltan. Se define ahora para que llegue con el color correcto y no con
/// uno inventado sobre la marcha, que es lo que ya había pasado con el verde.
#[allow(dead_code)]
pub const PURPLE: Color32 = Color32::from_rgb(0xa7, 0x8b, 0xfa);
pub const AMBER: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);
pub const RED: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);

/// Radio de esquina. El CSS usa 6-8 px casi en todo.
const ROUND: f32 = 6.0;

/// Carga las fuentes del sistema que el tema necesita.
///
/// egui empaqueta un subconjunto pensado para latín y emoji. Las formas
/// geométricas del lenguaje visual de Lucy —`◱ ✦ ▸ ▤ ▦ ◈ ◉`— no están, así que
/// el rail se dibujaba con un `□` de tofu en cada entrada. Y el terminal
/// heredaba una proporcional donde el CSS pide monoespaciada.
///
/// Se leen de `C:\Windows\Fonts` en vez de empaquetarlas:
///   • Sin ficheros binarios en el repo ni peso en el ejecutable.
///   • Sin problema de licencia: son las fuentes del sistema del propio usuario,
///     no redistribuidas.
///   • Lucy es una app de Windows; están garantizadas.
///
/// Es un añadido, no un reemplazo: se registran DETRÁS de las de egui, así que
/// si alguna falta el texto sigue saliendo y solo se pierde el glifo. Un tema no
/// debe poder dejar la app sin letras.
fn load_system_fonts(ctx: &egui::Context) {
    use eframe::egui::{FontData, FontFamily};

    let mut fonts = egui::FontDefinitions::default();
    let mut loaded_symbols = false;

    // Símbolos: cubre las formas geométricas del rail.
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
        fonts.font_data.insert("segoe_symbol".into(), FontData::from_owned(bytes));
        for fam in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(fam).or_default().push("segoe_symbol".into());
        }
        loaded_symbols = true;
    }

    // Monoespaciada real para el terminal y los bloques de código. `--mono` del
    // CSS pide JetBrains Mono / Cascadia / Consolas; Consolas es la que siempre
    // está.
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\consola.ttf") {
        fonts.font_data.insert("consolas".into(), FontData::from_owned(bytes));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "consolas".into());
    }

    if !loaded_symbols {
        // Sin símbolos el rail volvería al tofu. Mejor decirlo que dejar al
        // usuario mirando cuadrados sin saber por qué.
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

    visuals.override_text_color = Some(TXT);
    visuals.panel_fill = BG;
    visuals.window_fill = BG2;
    // `extreme_bg_color` es el fondo de campos de texto Y el CARRIL de las
    // barras de progreso. Puesto a BG —el mismo color del panel— la parte vacía
    // de la barra desaparecía: un 1 % de CPU se veía como un puntito verde con
    // el texto desbordado por fuera, en vez de como una barra casi vacía.
    // BG3 es el mismo escalón que usan los controles en reposo.
    visuals.extreme_bg_color = BG3;
    visuals.faint_bg_color = BG2;
    visuals.code_bg_color = BG3;
    visuals.hyperlink_color = ACC;
    visuals.window_stroke = Stroke::new(1.0_f32, BDR);
    visuals.window_rounding = Rounding::same(ROUND);

    // Los cinco estados de widget. egui los usa TODOS: dejarlos por defecto es
    // lo que hace que una app se vea "de egui" en vez de tuya.
    let w = &mut visuals.widgets;

    // Inactivo — la mayoría de lo que se ve en pantalla.
    w.inactive.bg_fill = BG3;
    w.inactive.weak_bg_fill = BG3;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, BDR);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, TXT2);
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

    // Espaciado. Los valores por defecto de egui son más apretados que el CSS
    // de Lucy, que respira bastante más.
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.menu_margin = egui::Margin::same(8.0);
    style.spacing.indent = 16.0;
    style.spacing.scroll.bar_width = 8.0;
    ctx.set_style(style);
}

/// Color del texto según el nivel de importancia de una memoria (1-3), igual
/// que el navegador de memoria de la app.
pub fn importance_color(importance: i64) -> Color32 {
    match importance {
        i if i >= 3 => AMBER,
        2 => ACC,
        _ => TXT3,
    }
}

/// Color de un porcentaje de uso, con los mismos cortes que el dashboard:
/// verde por debajo de 65, ámbar por debajo de 85, rojo por encima.
///
/// Los umbrales están aquí y no repartidos por las vistas porque son una
/// decisión de producto ("cuándo preocuparse"), no de dibujo. Repetirlos en
/// cada vista es cómo la CPU acaba avisando al 85 % y el disco al 90.
pub fn usage_color(pct: f32) -> Color32 {
    if pct >= 85.0 {
        RED
    } else if pct >= 65.0 {
        AMBER
    } else {
        ACC
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_matches_lucys_css_tokens() {
        // Pinta los valores contra `src/routes/page.css:68` de la app real. Si
        // alguien retoca el CSS y no esto, las dos interfaces divergen en
        // silencio — que es exactamente el tipo de deriva que este proyecto
        // lleva toda la sesión persiguiendo. No puede leerse el CSS desde aquí
        // (otro repo), así que se pinan los literales.
        assert_eq!(BG, Color32::from_rgb(0x0f, 0x11, 0x1a), "--bg");
        assert_eq!(ACC, Color32::from_rgb(0x10, 0xb9, 0x81), "--acc");
        assert_eq!(TXT, Color32::from_rgb(0xe2, 0xe8, 0xf0), "--txt");
        assert_eq!(RED, Color32::from_rgb(0xef, 0x44, 0x44), "--red");
    }

    #[test]
    fn usage_thresholds_match_the_dashboard_bands() {
        assert_eq!(usage_color(0.0), ACC);
        assert_eq!(usage_color(64.9), ACC);
        assert_eq!(usage_color(65.0), AMBER, "65 ya es ámbar, no verde");
        assert_eq!(usage_color(84.9), AMBER);
        assert_eq!(usage_color(85.0), RED, "85 ya es rojo, no ámbar");
        assert_eq!(usage_color(100.0), RED);
    }

    #[test]
    fn importance_maps_the_three_levels_and_clamps_outside_them() {
        assert_eq!(importance_color(3), AMBER);
        assert_eq!(importance_color(9), AMBER, "por encima de 3 sigue siendo alta");
        assert_eq!(importance_color(2), ACC);
        assert_eq!(importance_color(1), TXT3);
        assert_eq!(importance_color(0), TXT3, "un 0 inesperado no debe teñirse de alta");
    }
}
