//! Los iconos de Lucy, dibujados con trazos en vez de glifos.
//!
//! POR QUÉ NO SON TEXTO. Hasta ahora el prototipo usaba figuras geométricas
//! Unicode —`▤ ◈ ▣ ✦ ◱`— sacadas de Segoe UI Symbol. Funcionan como marcador de
//! posición y se notan: son sólidas donde la V2 usa trazo, tienen el peso y la
//! altura que decidió el diseñador de la fuente, no encajan en una rejilla
//! común, y varias son cuadrados casi idénticos entre sí. Es la mitad de la
//! sensación de "tosco".
//!
//! La V2 usa Tabler: rejilla de 24, trazo de 1.75, extremos redondeados. Eso no
//! es un formato de fichero, es una FORMA de dibujar — y se reproduce con el
//! painter de egui sin dependencias, sin SVG que descodificar y sin un atlas que
//! se vea borroso al escalar. Además el color lo pone quien dibuja, así que un
//! icono activo, uno atenuado y uno de alerta son la misma figura.
//!
//! Las coordenadas van en la rejilla de 24 de Tabler para poder compararlas con
//! el original de un vistazo; el renderizador las lleva al tamaño pedido.

use eframe::egui::{self, Color32, Pos2, Stroke};

/// Un trazo del dibujo, en coordenadas de la rejilla de 24.
enum Seg {
    /// Polilínea. Dos puntos son una recta.
    Path(&'static [(f32, f32)]),
    Circle((f32, f32), f32),
    /// Rectángulo con esquinas redondeadas: `(min, max, radio)`.
    Rect((f32, f32), (f32, f32), f32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// `layout-grid` — Dashboard.
    Grid,
    /// `sparkles` — Terminal IA.
    Sparkles,
    /// `terminal-2` — NexShell.
    Terminal,
    /// `file-text` — Log Viewer.
    FileText,
    /// `database` — Inventario.
    Database,
    /// `shield-check` — Compliance.
    Shield,
    /// `topology-star` — Memoria.
    Memory,
    /// `settings` — Configuración.
    Settings,
    /// `refresh`.
    Refresh,
    /// `chevron-down`.
    ChevronDown,
    /// `paperclip`.
    Clip,
    /// `microphone`.
    Mic,
    /// `arrow-up` — enviar.
    ArrowUp,
    /// `plus` — nueva pestaña.
    Plus,
    /// `x` — cerrar, quitar.
    Close,
    /// `copy`.
    Copy,
    /// `device-desktop` — este equipo.
    Desktop,
    /// `server` — un equipo remoto.
    Server,
}

impl Icon {
    fn segs(self) -> &'static [Seg] {
        match self {
            Self::Grid => &[
                Seg::Rect((4.0, 4.0), (10.0, 10.0), 1.5),
                Seg::Rect((14.0, 4.0), (20.0, 10.0), 1.5),
                Seg::Rect((4.0, 14.0), (10.0, 20.0), 1.5),
                Seg::Rect((14.0, 14.0), (20.0, 20.0), 1.5),
            ],
            // Dos destellos de cuatro puntas, uno grande y uno pequeño.
            Self::Sparkles => &[
                Seg::Path(&[
                    (9.0, 4.0),
                    (10.7, 8.3),
                    (15.0, 10.0),
                    (10.7, 11.7),
                    (9.0, 16.0),
                    (7.3, 11.7),
                    (3.0, 10.0),
                    (7.3, 8.3),
                    (9.0, 4.0),
                ]),
                Seg::Path(&[
                    (17.0, 13.0),
                    (17.9, 15.1),
                    (20.0, 16.0),
                    (17.9, 16.9),
                    (17.0, 19.0),
                    (16.1, 16.9),
                    (14.0, 16.0),
                    (16.1, 15.1),
                    (17.0, 13.0),
                ]),
            ],
            Self::Terminal => &[
                Seg::Path(&[(5.0, 7.0), (10.0, 12.0), (5.0, 17.0)]),
                Seg::Path(&[(13.0, 17.0), (19.0, 17.0)]),
            ],
            Self::FileText => &[
                Seg::Path(&[
                    (14.0, 3.0),
                    (6.0, 3.0),
                    (6.0, 21.0),
                    (18.0, 21.0),
                    (18.0, 7.0),
                    (14.0, 3.0),
                    (14.0, 7.0),
                    (18.0, 7.0),
                ]),
                Seg::Path(&[(9.0, 13.0), (15.0, 13.0)]),
                Seg::Path(&[(9.0, 17.0), (15.0, 17.0)]),
            ],
            // Cilindro: una elipse arriba aproximada con la parte alta de un
            // círculo achatado, más los dos costados y las dos curvas.
            Self::Database => &[
                Seg::Path(&[
                    (4.0, 6.0),
                    (6.0, 4.6),
                    (12.0, 4.0),
                    (18.0, 4.6),
                    (20.0, 6.0),
                    (18.0, 7.4),
                    (12.0, 8.0),
                    (6.0, 7.4),
                    (4.0, 6.0),
                ]),
                Seg::Path(&[(4.0, 6.0), (4.0, 18.0), (6.0, 19.4), (12.0, 20.0), (18.0, 19.4), (20.0, 18.0), (20.0, 6.0)]),
                Seg::Path(&[(4.0, 12.0), (6.0, 13.4), (12.0, 14.0), (18.0, 13.4), (20.0, 12.0)]),
            ],
            Self::Shield => &[
                Seg::Path(&[
                    (12.0, 3.0),
                    (19.0, 6.0),
                    (19.0, 11.0),
                    (15.5, 19.0),
                    (12.0, 21.0),
                    (8.5, 19.0),
                    (5.0, 11.0),
                    (5.0, 6.0),
                    (12.0, 3.0),
                ]),
                Seg::Path(&[(9.0, 12.0), (11.0, 14.0), (15.0, 10.0)]),
            ],
            // `topology-star`: un nodo central y cuatro satélites.
            Self::Memory => &[
                Seg::Circle((12.0, 12.0), 2.6),
                Seg::Circle((5.0, 5.0), 2.0),
                Seg::Circle((19.0, 5.0), 2.0),
                Seg::Circle((5.0, 19.0), 2.0),
                Seg::Circle((19.0, 19.0), 2.0),
                Seg::Path(&[(6.6, 6.6), (10.1, 10.1)]),
                Seg::Path(&[(17.4, 6.6), (13.9, 10.1)]),
                Seg::Path(&[(6.6, 17.4), (10.1, 13.9)]),
                Seg::Path(&[(17.4, 17.4), (13.9, 13.9)]),
            ],
            // Engranaje aproximado con un octógono dentado y el eje.
            Self::Settings => &[
                Seg::Path(&[
                    (10.3, 3.5), (13.7, 3.5), (14.4, 6.0), (16.6, 6.9), (18.9, 5.6),
                    (20.6, 8.4), (18.9, 10.2), (18.9, 13.8), (20.6, 15.6), (18.9, 18.4),
                    (16.6, 17.1), (14.4, 18.0), (13.7, 20.5), (10.3, 20.5), (9.6, 18.0),
                    (7.4, 17.1), (5.1, 18.4), (3.4, 15.6), (5.1, 13.8), (5.1, 10.2),
                    (3.4, 8.4), (5.1, 5.6), (7.4, 6.9), (9.6, 6.0), (10.3, 3.5),
                ]),
                Seg::Circle((12.0, 12.0), 3.0),
            ],
            Self::Refresh => &[
                Seg::Path(&[(20.0, 11.0), (20.0, 6.0), (15.5, 6.0)]),
                Seg::Path(&[
                    (19.5, 9.0), (18.0, 6.5), (15.0, 4.8), (11.5, 4.6), (8.2, 6.0),
                    (5.8, 8.6), (4.9, 12.0), (5.6, 15.5), (7.8, 18.2), (11.0, 19.6),
                    (14.5, 19.5), (17.4, 18.0),
                ]),
            ],
            Self::ChevronDown => &[Seg::Path(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)])],
            Self::Clip => &[Seg::Path(&[
                (15.0, 7.0), (8.5, 13.5), (8.5, 16.0), (10.5, 18.0), (13.0, 18.0),
                (19.5, 11.5), (19.5, 7.5), (16.5, 4.5), (12.5, 4.5), (5.0, 12.0),
                (5.0, 17.0), (8.0, 20.0),
            ])],
            Self::Mic => &[
                Seg::Rect((9.0, 3.0), (15.0, 14.0), 3.0),
                Seg::Path(&[(5.5, 11.0), (5.5, 12.5), (8.0, 16.5), (12.0, 17.5), (16.0, 16.5), (18.5, 12.5), (18.5, 11.0)]),
                Seg::Path(&[(12.0, 17.5), (12.0, 21.0)]),
            ],
            Self::ArrowUp => &[
                Seg::Path(&[(12.0, 19.0), (12.0, 5.0)]),
                Seg::Path(&[(6.0, 11.0), (12.0, 5.0), (18.0, 11.0)]),
            ],
            Self::Plus => &[
                Seg::Path(&[(12.0, 5.0), (12.0, 19.0)]),
                Seg::Path(&[(5.0, 12.0), (19.0, 12.0)]),
            ],
            Self::Close => &[
                Seg::Path(&[(6.0, 6.0), (18.0, 18.0)]),
                Seg::Path(&[(18.0, 6.0), (6.0, 18.0)]),
            ],
            Self::Copy => &[
                Seg::Rect((8.0, 8.0), (20.0, 20.0), 2.0),
                Seg::Path(&[(16.0, 8.0), (16.0, 6.0), (4.0, 6.0), (4.0, 16.0), (6.0, 16.0)]),
            ],
            Self::Desktop => &[
                Seg::Rect((3.0, 4.0), (21.0, 16.0), 2.0),
                Seg::Path(&[(7.0, 20.0), (17.0, 20.0)]),
                Seg::Path(&[(9.0, 16.0), (8.0, 20.0)]),
                Seg::Path(&[(15.0, 16.0), (16.0, 20.0)]),
            ],
            Self::Server => &[
                Seg::Rect((3.0, 4.0), (21.0, 10.0), 2.0),
                Seg::Rect((3.0, 14.0), (21.0, 20.0), 2.0),
                Seg::Path(&[(7.0, 7.0), (7.01, 7.0)]),
                Seg::Path(&[(7.0, 17.0), (7.01, 17.0)]),
            ],
        }
    }
}

/// Dibuja un icono centrado en `center`, ocupando `size` píxeles de lado.
///
/// El grosor del trazo escala con el tamaño —el 1.75 de Tabler sobre 24— para
/// que un icono de 14 px y otro de 24 se vean de la misma familia y no como dos
/// pesos distintos.
pub fn draw(painter: &egui::Painter, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let k = size / 24.0;
    let stroke = Stroke::new((1.75 * k).max(1.0), color);
    let p = |(x, y): (f32, f32)| -> Pos2 {
        egui::pos2(center.x + (x - 12.0) * k, center.y + (y - 12.0) * k)
    };
    for seg in icon.segs() {
        match seg {
            Seg::Path(pts) => {
                painter.add(egui::Shape::line(
                    pts.iter().map(|&t| p(t)).collect(),
                    stroke,
                ));
            }
            Seg::Circle(c, r) => {
                painter.circle_stroke(p(*c), r * k, stroke);
            }
            Seg::Rect(min, max, r) => {
                painter.rect_stroke(
                    egui::Rect::from_min_max(p(*min), p(*max)),
                    egui::Rounding::same(r * k),
                    stroke,
                );
            }
        }
    }
}

/// Asigna espacio y dibuja un icono ahí. Para usarlo dentro de una fila.
pub fn show(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    draw(ui.painter(), icon, rect.center(), size, color);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Los 18 iconos, uno por uno.
    const ALL: [Icon; 18] = [
        Icon::Grid, Icon::Sparkles, Icon::Terminal, Icon::FileText, Icon::Database,
        Icon::Shield, Icon::Memory, Icon::Settings, Icon::Refresh, Icon::ChevronDown,
        Icon::Clip, Icon::Mic, Icon::ArrowUp, Icon::Plus, Icon::Close, Icon::Copy,
        Icon::Desktop, Icon::Server,
    ];

    #[test]
    fn ningun_icono_esta_vacio() {
        // Un icono sin trazos no falla: deja un hueco. Y un hueco donde debería
        // haber un icono se lee como un fallo de la fuente, que es justo lo que
        // este módulo vino a quitar.
        for i in ALL {
            assert!(!i.segs().is_empty(), "un icono sin trazos deja un hueco mudo");
        }
    }

    #[test]
    fn todo_cae_dentro_de_la_rejilla_de_24() {
        // Salirse de la rejilla no rompe nada visible al dibujar uno solo, pero
        // hace que ese icono se vea MÁS GRANDE que sus vecinos con el mismo
        // tamaño pedido — y una fila de iconos que no miden lo mismo es
        // exactamente la sensación de "hecho a mano".
        for i in ALL {
            for seg in i.segs() {
                let pts: Vec<(f32, f32)> = match seg {
                    Seg::Path(p) => p.to_vec(),
                    Seg::Circle((x, y), r) => {
                        vec![(x - r, y - r), (x + r, y + r)]
                    }
                    Seg::Rect(a, b, _) => vec![*a, *b],
                };
                for (x, y) in pts {
                    assert!(
                        (0.0..=24.0).contains(&x) && (0.0..=24.0).contains(&y),
                        "un trazo se sale de la rejilla: ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn una_polilinea_necesita_al_menos_dos_puntos() {
        for i in ALL {
            for seg in i.segs() {
                if let Seg::Path(p) = seg {
                    assert!(p.len() >= 2, "una polilínea de un punto no dibuja nada");
                }
            }
        }
    }
}
