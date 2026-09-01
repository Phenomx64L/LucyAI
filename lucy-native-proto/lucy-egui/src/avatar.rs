//! El retrato de Lucy.
//!
//! Es la única imagen de toda la interfaz, y hace más por que la aplicación
//! parezca viva que cualquier otra cosa de este tamaño: el estado vacío de
//! Terminal IA pasa de un asterisco a una cara que te saluda por tu nombre.
//!
//! VA EMPOTRADO EN EL EJECUTABLE, no leído del disco. El PNG son 28 KB y es
//! parte del producto, no un recurso configurable: si se leyera de una ruta, un
//! usuario que mueve la carpeta se queda sin cara y con un hueco que parece un
//! error. Empotrado, la aplicación no puede arrancar sin ella.
//!
//! EL RECORTE CIRCULAR SE HACE EN LOS PÍXELES, una vez, al cargar. egui no
//! recorta una imagen a una forma —dibuja rectángulos con textura—, así que la
//! alternativa sería una malla con vértices en círculo y coordenadas UV
//! calculadas a mano. Poner el alfa a cero fuera del círculo es el mismo
//! resultado por una fracción del código, y solo cuesta una pasada al arrancar.

use eframe::egui::{self, Color32, ColorImage, TextureHandle};

/// El PNG de `static/lucy-avatar.png` de la app real, copiado aquí.
const PNG: &[u8] = include_bytes!("../assets/lucy-avatar.png");

/// Carga el retrato y lo sube como textura. `None` si el PNG no se puede
/// descodificar — la interfaz cae a su glifo y sigue funcionando.
pub fn load(ctx: &egui::Context) -> Option<TextureHandle> {
    let img = image::load_from_memory(PNG).ok()?.to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut px: Vec<Color32> = Vec::with_capacity(w * h);

    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let r = cx.min(cy);
    for y in 0..h {
        for x in 0..w {
            let p = img.get_pixel(x as u32, y as u32).0;
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            // Un píxel de transición en el borde. Sin él, el círculo sale con
            // escalones — y un borde dentado en la única imagen de la interfaz
            // se ve más que el resto del recorte junto.
            let a = ((r - d).clamp(0.0, 1.0) * p[3] as f32) as u8;
            px.push(Color32::from_rgba_unmultiplied(p[0], p[1], p[2], a));
        }
    }

    Some(ctx.load_texture(
        "lucy-avatar",
        ColorImage { size: [w, h], pixels: px },
        egui::TextureOptions::LINEAR,
    ))
}

/// Dibuja el retrato con su aro de acento, como el CSS.
///
/// El aro no es decoración: separa la imagen del fondo casi negro, que si no la
/// deja flotando sin bordes.
pub fn show(ui: &mut egui::Ui, tex: &TextureHandle, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let inner = rect.shrink(3.0);
    ui.painter().image(
        tex.id(),
        inner,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    ui.painter().circle_stroke(
        rect.center(),
        size / 2.0 - 1.5,
        egui::Stroke::new(1.5_f32, crate::theme::acc().linear_multiply(0.55)),
    );
}
