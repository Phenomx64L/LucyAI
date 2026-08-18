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
    /// Una polibézier cúbica: `[p0, c1, c2, p1, c1, c2, p1, …]`.
    ///
    /// HACE FALTA DE VERDAD y no es refinamiento. Tabler dibuja el engranaje, las
    /// chispas y las flechas circulares con curvas; aproximarlas con rectas —que
    /// es lo que había— convierte un engranaje en un polígono dentado y una
    /// chispa de lados cóncavos en una estrella de picos. A 15 px la diferencia
    /// no es sutil: es la mitad de la sensación de «tosco».
    Curve(&'static [(f32, f32)]),
    /// Arco de circunferencia: `(centro, radio, grados_inicio, grados_fin)`.
    ///
    /// Los grados van como en SVG: 0 a la derecha y creciendo en el sentido de
    /// las agujas del reloj, porque en pantalla la Y crece hacia abajo.
    Arc((f32, f32), f32, f32, f32),
}

/// En cuántos tramos se parte una curva.
///
/// Doce. Un icono ocupa entre catorce y veinticuatro píxeles: por encima de esto
/// los tramos caen por debajo del píxel y solo se paga el coste.
const TRAMOS: usize = 12;

/// Muestrea una bézier cúbica en `TRAMOS` puntos.
fn cubica(p0: (f32, f32), c1: (f32, f32), c2: (f32, f32), p1: (f32, f32)) -> Vec<(f32, f32)> {
    (0..=TRAMOS)
        .map(|i| {
            let t = i as f32 / TRAMOS as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            (
                a * p0.0 + b * c1.0 + c * c2.0 + d * p1.0,
                a * p0.1 + b * c1.1 + c * c2.1 + d * p1.1,
            )
        })
        .collect()
}

/// Los puntos de un arco, en la rejilla de 24.
fn arco(c: (f32, f32), r: f32, desde: f32, hasta: f32) -> Vec<(f32, f32)> {
    let n = TRAMOS.max(((hasta - desde).abs() / 15.0) as usize);
    (0..=n)
        .map(|i| {
            let g = (desde + (hasta - desde) * i as f32 / n as f32).to_radians();
            (c.0 + r * g.cos(), c.1 + r * g.sin())
        })
        .collect()
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
    /// `bolt` — el modo automático: Lucy encadena pasos sola.
    Bolt,
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
    /// `pencil` — editar y reenviar una orden.
    Pencil,
    /// `minus` — minimizar la ventana.
    Minimize,
    /// `square` — maximizar.
    Maximize,
    /// `copy` desplazado — restaurar desde maximizada, dos rectángulos como en
    /// cualquier ventana de Windows.
    Restore,
    /// `player-pause` — parar el refresco automático del visor de logs.
    Pause,
    /// `player-play` — reanudarlo.
    Play,
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
            // `sparkles`: LOS LADOS SON CÓNCAVOS, y ahí estaba la diferencia. En
            // Tabler cada punta se traza con un arco que se hunde hacia el
            // centro; con rectas salía una estrella de picos, que es otra figura
            // — y encima una que compite con el icono de alerta.
            //
            // Cada aspa es una bézier cuya tangente sale del centro, así que la
            // curva «tira» hacia dentro. `f` es cuánto: 0.55 es el valor que
            // aproxima un cuarto de círculo, y es exactamente la concavidad que
            // usa Tabler.
            Self::Sparkles => &[
                // Estrella grande, centro (9,10), radio 6.
                Seg::Curve(&[
                    (9.0, 4.0),
                    (9.0, 7.3), (5.7, 10.0), (3.0, 10.0),
                    (5.7, 10.0), (9.0, 12.7), (9.0, 16.0),
                    (9.0, 12.7), (12.3, 10.0), (15.0, 10.0),
                    (12.3, 10.0), (9.0, 7.3), (9.0, 4.0),
                ]),
                // Estrella pequeña, centro (17,16), radio 3.
                Seg::Curve(&[
                    (17.0, 13.0),
                    (17.0, 14.65), (15.35, 16.0), (14.0, 16.0),
                    (15.35, 16.0), (17.0, 17.35), (17.0, 19.0),
                    (17.0, 17.35), (18.65, 16.0), (20.0, 16.0),
                    (18.65, 16.0), (17.0, 14.65), (17.0, 13.0),
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
            // `settings`: EL ENGRANAJE DE TABLER TIENE LOS LÓBULOS REDONDEADOS.
            // Antes era un polígono de veinticuatro vértices rectos, y el
            // resultado a quince píxeles era una rueda dentada de picos — más
            // parecida a un aviso que a un ajuste. Cada esquina del contorno
            // lleva ahora su propia curva corta, que es lo que le da el aspecto
            // de pieza torneada en vez de sierra.
            Self::Settings => &[
                Seg::Curve(&[
                    (10.3, 3.6),
                    (10.5, 2.8), (13.5, 2.8), (13.7, 3.6),
                    (13.9, 4.4), (14.0, 5.5), (14.5, 6.1),
                    (15.1, 6.6), (16.0, 6.9), (16.7, 6.8),
                    (17.5, 6.7), (18.3, 5.5), (19.0, 5.9),
                    (19.8, 6.3), (21.0, 8.0), (20.6, 8.6),
                    (20.2, 9.3), (19.1, 9.8), (18.9, 10.5),
                    (18.7, 11.2), (18.7, 12.8), (18.9, 13.5),
                    (19.1, 14.2), (20.2, 14.7), (20.6, 15.4),
                    (21.0, 16.0), (19.8, 17.7), (19.0, 18.1),
                    (18.3, 18.5), (17.5, 17.3), (16.7, 17.2),
                    (16.0, 17.1), (15.1, 17.4), (14.5, 17.9),
                    (14.0, 18.5), (13.9, 19.6), (13.7, 20.4),
                    (13.5, 21.2), (10.5, 21.2), (10.3, 20.4),
                    (10.1, 19.6), (10.0, 18.5), (9.5, 17.9),
                    (8.9, 17.4), (8.0, 17.1), (7.3, 17.2),
                    (6.5, 17.3), (5.7, 18.5), (5.0, 18.1),
                    (4.2, 17.7), (3.0, 16.0), (3.4, 15.4),
                    (3.8, 14.7), (4.9, 14.2), (5.1, 13.5),
                    (5.3, 12.8), (5.3, 11.2), (5.1, 10.5),
                    (4.9, 9.8), (3.8, 9.3), (3.4, 8.6),
                    (3.0, 8.0), (4.2, 6.3), (5.0, 5.9),
                    (5.7, 5.5), (6.5, 6.7), (7.3, 6.8),
                    (8.0, 6.9), (8.9, 6.6), (9.5, 6.1),
                    (10.0, 5.5), (10.1, 4.4), (10.3, 3.6),
                ]),
                Seg::Circle((12.0, 12.0), 3.0),
            ],
            // `refresh`: DOS ARCOS DE VERDAD. Antes era una polilínea de doce
            // puntos aproximando el círculo, y a este tamaño los tramos rectos
            // se ven: el aro salía poligonal justo en un icono que gira, donde
            // cualquier faceta se convierte en un parpadeo al animarlo.
            Self::Refresh => &[
                // Arco superior, de las 10 a las 2 pasando por arriba.
                Seg::Arc((12.0, 12.0), 7.0, 160.0, 340.0),
                Seg::Path(&[(19.6, 5.5), (19.0, 9.4), (15.2, 8.8)]),
                // Y el inferior, simétrico.
                Seg::Arc((12.0, 12.0), 7.0, -20.0, 160.0),
                Seg::Path(&[(4.4, 18.5), (5.0, 14.6), (8.8, 15.2)]),
            ],
            // `bolt`: el rayo de Tabler. Un solo trazo cerrado — la figura es
            // el contorno, no un relleno, que es lo que la deja legible a 17px
            // igual que a 40.
            Self::Bolt => &[Seg::Path(&[
                (13.0, 3.0), (5.0, 13.5), (11.0, 13.5), (11.0, 21.0),
                (19.0, 10.5), (13.0, 10.5), (13.0, 3.0),
            ])],
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
            Self::Pencil => &[
                Seg::Path(&[(4.0, 20.0), (8.0, 19.0), (19.0, 8.0), (16.0, 5.0), (5.0, 16.0), (4.0, 20.0)]),
                Seg::Path(&[(13.5, 7.5), (16.5, 10.5)]),
            ],
            Self::Minimize => &[Seg::Path(&[(6.0, 12.0), (18.0, 12.0)])],
            Self::Maximize => &[Seg::Rect((6.0, 6.0), (18.0, 18.0), 1.5)],
            Self::Restore => &[
                Seg::Rect((5.0, 8.0), (16.0, 19.0), 1.5),
                Seg::Path(&[(8.0, 5.0), (19.0, 5.0), (19.0, 16.0)]),
            ],
            // Las dos barras de `player-pause`, con la misma caja de 24 que el
            // resto: estrechas y altas, o a este tamaño se leen como un igual.
            Self::Pause => &[
                Seg::Rect((7.0, 5.0), (10.0, 19.0), 1.0),
                Seg::Rect((14.0, 5.0), (17.0, 19.0), 1.0),
            ],
            // El triángulo de `player-play`, CERRADO: el último punto repite el
            // primero porque `Seg::Path` traza segmentos sueltos y sin él queda
            // una uve abierta en vez de un triángulo.
            Self::Play => &[Seg::Path(&[(7.0, 4.0), (20.0, 12.0), (7.0, 20.0), (7.0, 4.0)])],
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
    // LOS EXTREMOS REDONDEADOS SON LA MITAD DEL PARECIDO, y faltaban en todos.
    // Tabler dibuja con `stroke-linecap="round"` y `stroke-linejoin="round"`;
    // egui corta los trazos en plano y une las esquinas en pico. A quince
    // píxeles eso no se lee como «otro estilo», se lee como tosco: cada final de
    // línea es un tajo y cada codo un pincho.
    //
    // Un círculo relleno del radio del trazo en cada vértice ES un extremo
    // redondeado —y de paso redondea las uniones—, que es la forma barata de
    // conseguirlo sin tocar el teselador.
    let redondear = |pts: &[Pos2]| {
        let r = stroke.width * 0.5;
        for q in pts {
            painter.circle_filled(*q, r, color);
        }
    };
    let mut trazo = |pts: Vec<Pos2>| {
        painter.add(egui::Shape::line(pts.clone(), stroke));
        redondear(&pts);
    };
    for seg in icon.segs() {
        match seg {
            Seg::Path(pts) => trazo(pts.iter().map(|&t| p(t)).collect()),
            Seg::Curve(pts) => {
                // [p0, c1, c2, p1, c1, c2, p1, …]: cada tramo reusa el final del
                // anterior como principio.
                let mut salida: Vec<Pos2> = Vec::new();
                let mut i = 0;
                while i + 3 < pts.len() {
                    let muestras = cubica(pts[i], pts[i + 1], pts[i + 2], pts[i + 3]);
                    // El primer punto de cada tramo repite el último del previo.
                    let desde = usize::from(!salida.is_empty());
                    salida.extend(muestras[desde..].iter().map(|&t| p(t)));
                    i += 3;
                }
                if !salida.is_empty() {
                    painter.add(egui::Shape::line(salida.clone(), stroke));
                    // Solo los EXTREMOS: redondear los doce puntos de muestreo
                    // de cada tramo engordaría la curva entera.
                    redondear(&[salida[0], salida[salida.len() - 1]]);
                }
            }
            Seg::Arc(c, r, a, b) => {
                let pts: Vec<Pos2> = arco(*c, *r, *a, *b).iter().map(|&t| p(t)).collect();
                painter.add(egui::Shape::line(pts.clone(), stroke));
                if pts.len() >= 2 {
                    redondear(&[pts[0], pts[pts.len() - 1]]);
                }
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

    /// Los 22 iconos, uno por uno.
    const ALL: [Icon; 23] = [
        Icon::Grid, Icon::Sparkles, Icon::Terminal, Icon::FileText, Icon::Database,
        Icon::Shield, Icon::Memory, Icon::Settings, Icon::Refresh, Icon::Bolt,
        Icon::ChevronDown,
        Icon::Clip, Icon::Mic, Icon::ArrowUp, Icon::Plus, Icon::Close, Icon::Copy,
        Icon::Desktop, Icon::Server, Icon::Pencil, Icon::Minimize, Icon::Maximize,
        Icon::Restore,
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
                    // Los PUNTOS MUESTREADOS y no los de control: una bézier
                    // puede tener un control fuera de la rejilla y quedar dentro
                    // —es lo normal en una curva pronunciada— y al revés, todos
                    // los controles dentro no garantiza que la curva no se
                    // asome. Lo que se dibuja es la muestra, así que es lo que
                    // hay que medir.
                    Seg::Curve(p) => {
                        let mut v = Vec::new();
                        let mut i = 0;
                        while i + 3 < p.len() {
                            v.extend(super::cubica(p[i], p[i + 1], p[i + 2], p[i + 3]));
                            i += 3;
                        }
                        v
                    }
                    Seg::Arc(c, r, a, b) => super::arco(*c, *r, *a, *b),
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
    fn una_curva_empieza_y_acaba_donde_dice() {
        // Si el muestreo no clavara los extremos, dos tramos consecutivos de una
        // polibézier no se tocarían: el contorno del engranaje saldría con
        // grietas de una décima de píxel, que a este tamaño se ven como puntos.
        let v = super::cubica((0.0, 0.0), (1.0, 5.0), (9.0, 5.0), (10.0, 0.0));
        assert_eq!(v.first().copied(), Some((0.0, 0.0)));
        assert_eq!(v.last().copied(), Some((10.0, 0.0)));
        // Y en medio se separa de la recta: si no, no es una curva.
        let medio = v[v.len() / 2];
        assert!(medio.1 > 1.0, "la curva no se curva: {medio:?}");
    }

    #[test]
    fn un_arco_recorre_los_grados_que_se_le_piden() {
        // Cero a la derecha y creciendo hacia abajo, como en SVG — la Y de la
        // pantalla crece al revés que la de la clase de matemáticas, y confundir
        // eso pone la flecha de recargar girando al contrario.
        let v = super::arco((12.0, 12.0), 6.0, 0.0, 90.0);
        let (x0, y0) = v[0];
        assert!((x0 - 18.0).abs() < 0.01 && (y0 - 12.0).abs() < 0.01, "empieza en {x0},{y0}");
        let (x1, y1) = *v.last().unwrap();
        assert!((x1 - 12.0).abs() < 0.01 && (y1 - 18.0).abs() < 0.01, "acaba en {x1},{y1}");
        // Todos los puntos a la misma distancia del centro: es un arco, no una
        // espiral.
        for (x, y) in &v {
            let d = ((x - 12.0).powi(2) + (y - 12.0).powi(2)).sqrt();
            assert!((d - 6.0).abs() < 0.01, "radio {d} en ({x},{y})");
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
