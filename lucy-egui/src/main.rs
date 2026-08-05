//! Lucy — Fase 1 · shell nativo egui (paso 1).
//!
//! Una app egui REAL: rail izquierdo + 3 vistas. Prueba de extremo a extremo del
//! camino de migración — nativo, sin WebView, sin Tauri:
//!   • Chat     — markdown en streaming (egui_commonmark).
//!   • Terminal — PTY viva (portable-pty, el de tu app).
//!   • Memoria  — lee tu DB REAL de Lucy (%APPDATA%\com.lucy.dev\lucy.db) en
//!                SOLO-LECTURA con rusqlite y renderiza tus memorias de verdad.
//!
//! NO modifica lucy-svelte. El acceso a la DB es read-only (WAL permite lectores
//! concurrentes con la app corriendo). Para render por software / RDP:
//!   set WGPU_BACKEND=gl && cargo run -p lucy-egui --release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod avatar;
mod drain;
mod hosts;
mod icons;
mod prompt;
mod theme;

use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use lucy_core::AgentMemory;
use proto_core::Pty;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            // Por debajo de esto la rejilla del dashboard empieza a apilar
            // columnas de 48 px y el compositor se queda sin sitio para el
            // campo de texto.
            .with_min_inner_size([900.0, 560.0])
            // Sin barra de título del sistema: la cabecera de la aplicación ES
            // la barra de título. Con las dos, Lucy tendría dos cabeceras de
            // distinto color y distinta altura, una encima de la otra.
            //
            // Lo que hay que reponer a mano está en `window_buttons` y en el
            // manejador de arrastre de la cabecera. El redimensionado NO: winit
            // sigue dando los bordes mientras la ventana sea `resizable`.
            .with_decorations(false)
            .with_resizable(true),
        ..Default::default()
    };
    eframe::run_native(
        "Lucy · egui (Fase 1)",
        opts,
        Box::new(|cc| {
            // El tema de Lucy, no el oscuro genérico de egui. Una vez, al
            // arrancar: en modo inmediato el estilo se consulta en cada frame,
            // así que fijarlo aquí lo aplica a todo lo que se dibuje después.
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new(cc.storage)))
        }),
    )
}

/// Las ocho entradas del rail de Lucy, en su orden real.
///
/// Están TODAS, no solo las migradas. Un rail con cuatro entradas daría la
/// impresión de que la app nativa está casi lista; con las ocho y las pendientes
/// marcadas, el propio prototipo dice en qué punto va la migración cada vez que
/// se abre. Eso es más útil que un documento de estado, porque no puede quedarse
/// obsoleto sin que se note.
#[derive(PartialEq, Clone, Copy)]
enum View {
    Dashboard,
    TerminalIa,
    NexShell,
    LogViewer,
    Inventario,
    Compliance,
    Memoria,
    Configuracion,
}

impl View {
    /// El nombre que ve el operador.
    fn label(self) -> &'static str {
        match self {
            View::Dashboard => "Dashboard",
            View::TerminalIa => "Terminal IA",
            View::NexShell => "NexShell",
            View::LogViewer => "Log Viewer",
            View::Inventario => "Inventario",
            View::Compliance => "Compliance",
            View::Memoria => "Memoria",
            View::Configuracion => "Configuración",
        }
    }

    /// Su icono, el mismo que usa la V2.
    fn icon(self) -> icons::Icon {
        match self {
            View::Dashboard => icons::Icon::Grid,
            View::TerminalIa => icons::Icon::Sparkles,
            View::NexShell => icons::Icon::Terminal,
            View::LogViewer => icons::Icon::FileText,
            View::Inventario => icons::Icon::Database,
            View::Compliance => icons::Icon::Shield,
            View::Memoria => icons::Icon::Memory,
            View::Configuracion => icons::Icon::Settings,
        }
    }

    /// Qué necesita del backend la vista que aún no está migrada. Se enseña en
    /// su panel: convierte un "pendiente" vago en el trabajo concreto que falta.
    fn pending_needs(self) -> Option<&'static str> {
        match self {
            View::Dashboard
            | View::TerminalIa
            | View::NexShell
            | View::Memoria
            | View::LogViewer => None,
            View::Inventario => Some(
                "commands/inventory.rs — puro, sin AppHandle. \
                 Necesita además la tabla ordenable y el export a PDF.",
            ),
            View::Compliance => Some(
                "commands/compliance.rs — puro. La vista es la tabla de checks \
                 por host más el porcentaje de aprobados.",
            ),
            View::Configuracion => Some(
                "Claves de API (keyring), catálogo de modelos, tema y umbrales. \
                 Depende de que el catálogo se mueva a lucy-core para no duplicarlo.",
            ),
        }
    }

    const ALL: [View; 8] = [
        View::Dashboard,
        View::TerminalIa,
        View::NexShell,
        View::LogViewer,
        View::Inventario,
        View::Compliance,
        View::Memoria,
        View::Configuracion,
    ];
}

/// Un mensaje del chat real (Ollama).
/// Qué clase de línea es en el hilo.
///
/// Un comando ejecutado NO es un mensaje del operador, y meterlo como tal era
/// lo que dejaba burbujas de "Resultado devuelto a Lucy" por toda la
/// conversación: con su avatar, su hora y su ancho, ocupando lo mismo que algo
/// que alguien escribió. Es un EVENTO, y se dibuja como una línea plegada.
#[derive(PartialEq)]
enum Role {
    User,
    Lucy,
    /// Un comando aprobado y corrido: `(comando, ok, salida)`.
    Exec(String, bool, String),
}

struct ChatMsg {
    role: Role,
    text: String,
    /// Hora del mensaje. La V2 la pone junto al nombre de Lucy en mono tabular —
    /// es lo que convierte una lista de burbujas en un hilo con historia.
    stamp: String,
}

impl ChatMsg {
    fn new(user: bool, text: String) -> Self {
        Self {
            role: if user { Role::User } else { Role::Lucy },
            text,
            stamp: hhmm(),
        }
    }

    fn exec(cmd: String, ok: bool, output: String) -> Self {
        Self { role: Role::Exec(cmd, ok, output), text: String::new(), stamp: hhmm() }
    }
}

/// `HH:MM` local. Formato corto a propósito: en un hilo interesa el orden y el
/// hueco entre mensajes, no el segundo exacto.
fn hhmm() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let t = secs % 86_400;
    format!("{:02}:{:02}", t / 3600, (t % 3600) / 60)
}

/// Iniciales del operador para su avatar.
///
/// Estaban escritas a mano como "IV" en la V2 —correcto para Iván por
/// casualidad, falso para cualquier otro—. Aquí salen del nombre real.
fn initials(name: &str) -> String {
    let parts: Vec<&str> = name.split_whitespace().collect();
    match parts.len() {
        0 => "U".to_string(),
        1 => parts[0].chars().take(2).collect::<String>().to_uppercase(),
        _ => {
            let a = parts[0].chars().next().unwrap_or('U');
            let b = parts[1].chars().next().unwrap_or(' ');
            format!("{a}{b}").to_uppercase()
        }
    }
}

/// Tope del contenido de un adjunto de texto.
///
/// El mismo número que `file-inputs.ts`, y por la misma razón dicha de otra
/// forma: un log de 400 MB arrastrado a la ventana no puede tumbar el proceso.
/// Cambia la memoria que se protege, no el problema.
const ATTACH_MAX_CHARS: usize = 200_000;

/// Qué clase de fichero es. La V2 distingue `image | text` en el compositor —
/// dos valores, no tres — porque el constructor del prompt filtra por
/// `type === 'text'`. El PDF es un texto que todavía hay que extraer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttachKind {
    Text,
    Image,
    Pdf,
}

impl AttachKind {
    fn of(path: &std::path::Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "tif" | "tiff" => Self::Image,
            "pdf" => Self::Pdf,
            _ => Self::Text,
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Self::Text => "▤",
            Self::Image => "▣",
            Self::Pdf => "▥",
        }
    }
}

/// Un fichero adjunto a la orden.
struct Attachment {
    name: String,
    kind: AttachKind,
    /// Contenido, ya recortado. Vacío para imagen y PDF — ver `attach`.
    content: String,
    /// Por qué no se puede mandar, cuando no se puede. Vacío = se manda.
    blocked: String,
}

impl Attachment {
    /// Lee un fichero del disco y decide qué se puede hacer con él.
    ///
    /// Un adjunto que no se va a poder mandar SE ACEPTA IGUAL y dice por qué.
    /// Rechazarlo en silencio al soltarlo deja al operador pensando que el
    /// arrastre no funciona; aceptarlo y mandarlo vacío es peor todavía.
    fn read(path: &std::path::Path) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("(sin nombre)")
            .to_string();
        let kind = AttachKind::of(path);
        match kind {
            AttachKind::Image => Self {
                name,
                kind,
                content: String::new(),
                blocked: "las imágenes necesitan la ruta de visión del backend".into(),
            },
            AttachKind::Pdf => Self {
                name,
                kind,
                content: String::new(),
                blocked: "el PDF se extrae en el backend (`extract_pdf_text`)".into(),
            },
            AttachKind::Text => match std::fs::read_to_string(path) {
                Ok(s) => {
                    let content: String = s.chars().take(ATTACH_MAX_CHARS).collect();
                    Self { name, kind, content, blocked: String::new() }
                }
                // Un binario cualquiera cae aquí: no es UTF-8 y no hay nada
                // sensato que mandarle al modelo.
                Err(e) => Self {
                    name,
                    kind,
                    content: String::new(),
                    blocked: format!("no se pudo leer como texto: {e}"),
                },
            },
        }
    }
}

/// Una terminal abierta. Cada pestaña es una conversación INDEPENDIENTE.
///
/// El receptor del stream vive aquí y no en la aplicación a propósito: una
/// respuesta pertenece a la conversación que la pidió. Con un único receptor
/// global, cambiar de pestaña mientras Lucy escribe mandaría los tokens a la
/// conversación equivocada — y en la V2 las pestañas de fondo siguen corriendo.
struct ChatTab {
    /// Identidad estable de la pestaña. NO su índice: cerrar una pestaña
    /// desplaza los índices de las de después, y un resultado que llegara
    /// después de ese cierre acabaría en la conversación equivocada.
    uid: usize,
    title: String,
    log: Vec<ChatMsg>,
    input: String,
    /// El workspace de ESTA pestaña.
    ///
    /// Por pestaña y no global, que es como estaba y era un fallo: con dos
    /// órdenes en marcha, el plan de una salía en el panel de la otra y el
    /// resultado de un comando se imprimía en la conversación equivocada. El
    /// workspace describe UN turno, y los turnos son de su pestaña.
    ws: lucy_core::agent::Workspace,
    /// Cuándo empezó el turno en curso de esta pestaña.
    turn_start: Option<Instant>,
    /// El texto recibido y aún no enseñado del turno en curso. Ver `drain`.
    drain: drain::Drain,
    /// Adjuntos de ESTA pestaña. Por pestaña y no globales: los ficheros
    /// pertenecen a la orden que se está escribiendo, y en la V2 cada terminal
    /// tiene los suyos.
    attachments: Vec<Attachment>,
    rx: Option<std::sync::mpsc::Receiver<lucy_core::chat::ChatEvent>>,
}

impl ChatTab {
    fn new(n: usize) -> Self {
        Self {
            uid: n,
            ws: lucy_core::agent::Workspace::default(),
            turn_start: None,
            drain: drain::Drain::default(),
            title: if n == 0 {
                "Nueva Terminal".to_string()
            } else {
                format!("Terminal {}", n + 1)
            },
            log: Vec::new(),
            input: String::new(),
            attachments: Vec::new(),
            rx: None,
        }
    }

    /// ¿Está Lucy escribiendo en ESTA pestaña?
    ///
    /// La cola cuenta: el stream puede haber terminado y quedar texto por
    /// revelar, y durante ese rato Lucy SIGUE escribiendo en pantalla. Sin
    /// esto, el cursor desaparecería a mitad de la última frase.
    fn busy(&self) -> bool {
        self.rx.is_some() || self.drain.busy()
    }
}

/// Con quién habla Lucy.
///
/// La V2 usa `lucyConfig.name`, que el operador escribe en Configuración… y que
/// vive en el `localStorage` del WebView. Un shell nativo no puede leerlo, así
/// que aquí se usa el usuario de Windows. Es el mismo dato el 99 % de las veces
/// en una estación de trabajo, pero NO es lo mismo, y queda anotado: la
/// configuración del usuario es lo siguiente que tiene que salir del navegador
/// —igual que el índice de equipos ya salió al Credential Manager— o el shell
/// nativo llegará a producción sin la mitad de los ajustes.
fn user_name() -> String {
    std::env::var("USERNAME").unwrap_or_default()
}

/// Saludo por franja horaria, como el `empty-state` de la V2.
fn greeting(name: &str) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let h = (secs % 86_400) / 3600;
    let word = if h < 12 {
        "Buenos días"
    } else if h < 19 {
        "Buenas tardes"
    } else {
        "Buenas noches"
    };
    let first = name.split_whitespace().next().unwrap_or("");
    if first.is_empty() {
        word.to_string()
    } else {
        format!("{word}, {first}")
    }
}

/// La paleta de comandos: los mismos 29 que la V2, con su descripción.
///
/// Está entera aunque el shell nativo todavía no ejecute casi ninguno, y es
/// deliberado: la paleta es una herramienta de DESCUBRIMIENTO —así se entera el
/// operador de que `/crystallize` existe— y una lista recortada a lo ya migrado
/// enseñaría una versión de Lucy más pequeña de la que hay. Los que aún no
/// funcionan lo dicen al elegirlos, en vez de no aparecer.
const SLASH: [(&str, &str, bool); 29] = [
    ("/model", "Cambiar el modelo activo", true),
    ("/clear", "Limpiar el chat actual", true),
    ("/memory", "Explorador de memoria (V1)", true),
    ("/kg", "Grafo de conocimiento (V1)", false),
    ("/link", "Relaciones tipadas entre memorias", false),
    ("/recall", "Recuperar memorias por consulta", true),
    ("/crystals", "Ver crystals de memoria", false),
    ("/crystallize", "Destilar la sesión en un crystal", false),
    ("/insights", "Insights consolidados", false),
    ("/consolidate", "Ejecutar consolidación ahora", false),
    ("/playbooks", "Playbooks multi-fase curados", false),
    ("/skills", "Picker de skills ejecutables", false),
    ("/preset", "Presets de framing (AD, Hyper-V, SQL…)", false),
    ("/sec-skill", "Catálogo security/forensics (200+)", false),
    ("/skills-manager", "Gestionar skills cargadas", false),
    ("/capabilities", "Auto-introspección: skills, MCPs, frameworks", false),
    ("/route", "Ver la última decisión de routing", false),
    ("/serial", "Bypass del fork advisor (esta pestaña)", false),
    ("/smart-router", "Smart-router on/off", false),
    ("/proactive", "Listar insights proactivos", false),
    ("/snapshot", "Capturar snapshot del sistema", false),
    ("/diff", "Comparar dos snapshots", false),
    ("/detective", "Síntesis forense de incidente", false),
    ("/runbooks", "Lista de runbooks (V1)", false),
    ("/pantalla", "Lucy ve tu pantalla (captura + pregunta)", false),
    ("/polarity", "Proyección de polaridad de un texto", false),
    ("/privacy", "Modo privacidad (sólo LLM local)", false),
    ("/theme", "Cambiar el tema visual", false),
    ("/help", "Referencia completa de comandos", true),
];

/// Las cuatro sugerencias del empty-state: `(icono, etiqueta, la orden real)`.
///
/// La etiqueta es corta y la orden es larga porque no son lo mismo: el chip dice
/// de qué va, y lo que se envía es una instrucción completa. Un chip que enviara
/// su propio texto —"Salud del sistema"— le daría a Lucy tres palabras sueltas
/// en lugar de una tarea.
const SUGGESTIONS: [(&str, &str, &str); 4] = [
    (
        "◈",
        "Salud del sistema",
        "Revisa la salud del sistema (CPU, RAM, disco, servicios) y dame un resumen del estado.",
    ),
    (
        "◉",
        "Vulnerabilidades",
        "Escanea el software instalado en busca de vulnerabilidades conocidas y dime cómo parcharlas.",
    ),
    (
        "▤",
        "Servicios detenidos",
        "¿Qué servicios de inicio automático están detenidos ahora mismo? Muéstramelos.",
    ),
    (
        "▥",
        "Errores recientes",
        "Resume los errores más recientes del registro de eventos del sistema (últimas 24 h).",
    ),
];

/// `%APPDATA%\Lucy\logs\lucy_app.log` — el MISMO fichero que escribe
/// `write_app_log` en el backend. Ojo: no cuelga de `com.lucy.dev` como la DB,
/// sino de `Lucy\logs` — `get_logs_dir()` lo tiene fijo así.
fn log_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Lucy").join("logs").join("lucy_app.log"))
}

/// `%APPDATA%\com.lucy.dev\lucy.db` — la MISMA DB que usa la app Tauri.
fn db_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("com.lucy.dev").join("lucy.db"))
}

/// Inicializa el pool del CORAZÓN sin-Tauri (`lucy_core::init`) sobre tu DB real y
/// llama `lucy_core::get_recent_memories` — la MISMA lógica que el backend Tauri,
/// pero en un crate compartido SIN motor de navegador. Sin IPC, sin duplicar.
fn load_memories() -> Result<Vec<AgentMemory>, String> {
    let path = db_path().ok_or("no se pudo resolver %APPDATA%")?;
    if !path.exists() {
        return Err(format!("DB no encontrada en {}", path.display()));
    }
    lucy_core::init(&path)?;
    lucy_core::get_recent_memories(Some(300))
}

fn rel_time(created_at: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let d = (now - created_at).max(0);
    if d < 60 {
        "ahora".into()
    } else if d < 3600 {
        format!("hace {} min", d / 60)
    } else if d < 86_400 {
        format!("hace {} h", d / 3600)
    } else {
        format!("hace {} d", d / 86_400)
    }
}

fn fmt_gb(bytes: u64) -> String {
    let gb = bytes as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}

/// Porcentaje de RAM usada. Vive aquí y no en cada tarjeta porque la V2 lo
/// muestra en dos sitios y dos divisiones distintas acaban discrepando en el
/// redondeo.
fn mem_pct(s: &lucy_core::system::SysSnapshot) -> f32 {
    if s.mem_total == 0 {
        0.0
    } else {
        s.mem_used as f32 / s.mem_total as f32 * 100.0
    }
}

/// Caudal en las unidades de la V2: kbps por debajo de 1 Mbps, Mbps por encima.
fn fmt_rate(bps: u64) -> String {
    let kbps = bps as f64 * 8.0 / 1000.0;
    if kbps >= 1000.0 {
        format!("{:.1} Mbps", kbps / 1000.0)
    } else {
        format!("{kbps:.0} kbps")
    }
}

/// `HH:MM:SS` local, sin arrastrar `chrono` al prototipo por una etiqueta.
fn stamp_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // UTC: el desfase local requeriría chrono o la API de zonas de Windows, y
    // esta etiqueta responde a "¿está vivo?", no a qué hora es exactamente.
    let t = secs % 86_400;
    format!("{:02}:{:02}:{:02}", t / 3600, (t % 3600) / 60, t % 60)
}

fn fmt_uptime(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

// ── primitivas de rejilla ────────────────────────────────────────────────────
//
// POR QUÉ EXISTEN — y es concreto, no estilo. `horizontal_wrapped` monta la
// fila con alineación CENTRADA, y una fila centrada ancla su origen en el
// centro del hueco disponible. La fila nace con la altura de un widget de
// texto, así que cualquier tarjeta más alta que una línea se desborda; al
// desbordarse, el hueco de la fila crece; al crecer, su centro baja; y la
// siguiente tarjeta empieza más abajo y se desborda más. Es un bucle
// realimentado: ocho tarjetas de 44 px en fila medían 1312 px de alto, cada una
// más grande y más caída que la anterior. Eso es la diagonal que se veía.
//
// La regla que lo cierra, y que este módulo no debe romper: toda celda se
// asigna con un tamaño EXPLÍCITO, las filas se alinean ARRIBA, y ningún
// `right_to_left` vive sin altura propia. El efecto secundario es el que se
// buscaba de todos modos — todas las tarjetas iguales, que es lo que hace que
// una rejilla se lea como una rejilla.

/// Separación única entre tarjetas. Una sola, para que columnas y filas casen.
const GAP: f32 = 10.0;

// Alturas de tarjeta del Dashboard. Se calculan, no se tantean: cada una es la
// suma de sus filas contando la altura real de línea de la fuente (≈ tamaño ×
// 1.45) más los márgenes. Sobrar unos píxeles no se ve; faltar uno recorta el
// texto de abajo — y los tests de `layout` miden que ninguna se pase.
// 18 + 8 + 41 + (10 + 26 + 6) + 16 + 28 = 153.6 medidos; 156 deja holgura.
const KPI_H: f32 = 156.0;
const NET_H: f32 = 106.0; // 16 + 10 + 3×18 + 28
const CORE_H: f32 = 44.0; // 16 + 6 + 4 + 18
const DISK_H: f32 = 78.0; // 18 + 8 + 5 + 8 + 16 + 24
const PROC_ROW: f32 = 22.0;

/// Muestras del historial de las líneas de tendencia — las mismas 44 de la V2.
/// A una por segundo son los últimos 44 s, que es el horizonte en el que una
/// subida todavía significa algo.
const SPARK_SAMPLES: usize = 44;

/// Ancho exacto de celda para repartir `total` en `cols` columnas.
///
/// El suelo de 48 px es una red para una ventana absurdamente estrecha: cuando
/// entra, la fila se desborda a lo ancho — algo que se ve y se arregla
/// ensanchando la ventana — en vez de dejar tarjetas de ancho cero, que no se
/// ven y se leen como datos que faltan.
fn cell_w(total: f32, cols: usize) -> f32 {
    let cols = cols.max(1);
    ((total - GAP * (cols - 1) as f32) / cols as f32).max(48.0)
}

/// Cuántas columnas de al menos `min_w` caben en `total`.
fn fit_cols(total: f32, min_w: f32) -> usize {
    (((total + GAP) / (min_w + GAP)).floor() as usize).max(1)
}

/// Fila de altura explícita, con las celdas alineadas ARRIBA.
///
/// Arriba y no centradas: si el contenido se pasa de `h` por unos píxeles, el
/// `max_rect` de la fila crece, y con alineación centrada lo que venga después
/// se centraría en la fila ya crecida. Alineado arriba, un desbordamiento sale
/// por abajo y no arrastra a nadie.
fn row(ui: &mut egui::Ui, h: f32, add: impl FnOnce(&mut egui::Ui)) {
    row_align(ui, h, egui::Align::Min, add);
}

/// Fila de altura explícita con alineación vertical elegida.
///
/// Solo para filas de TEXTO, donde tamaños distintos en la misma línea piden
/// una referencia común: el rótulo junto al badge en la cabecera, o el `%`
/// junto a la cifra. Es seguro porque `h` es explícita — lo que no lo es, y es
/// lo que descuadraba la rejilla, es centrar dentro de un hueco que crece.
fn row_align(ui: &mut egui::Ui, h: f32, align: egui::Align, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), h),
        egui::Layout::left_to_right(align),
        |ui| {
            ui.set_min_height(h);
            ui.spacing_mut().item_spacing.x = GAP;
            add(ui);
        },
    );
}

/// Alinea a la derecha DENTRO de una altura explícita.
fn right(ui: &mut egui::Ui, h: f32, add: impl FnOnce(&mut egui::Ui)) {
    let w = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(w, h),
        egui::Layout::right_to_left(egui::Align::Center),
        add,
    );
}

/// Tarjeta de tamaño exacto sobre `--surface-2`, el escalón de las tarjetas.
fn card(ui: &mut egui::Ui, size: egui::Vec2, pad: f32, add: impl FnOnce(&mut egui::Ui)) {
    card_on(ui, size, pad, theme::BG3, add);
}

/// Tarjeta de tamaño exacto: el contenido no decide cuánto mide, lo decide la
/// rejilla.
///
/// El relleno es un parámetro porque el CSS distingue dos escalones: las
/// tarjetas van sobre `--surface-2` y los paneles grandes que las contienen
/// sobre `--surface-1`. Apilar dos superficies iguales borra la separación.
fn card_on(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    pad: f32,
    fill: egui::Color32,
    add: impl FnOnce(&mut egui::Ui),
) {
    ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_min_size(size);
        egui::Frame::none()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
            .rounding(egui::Rounding::same(theme::R_LG))
            .inner_margin(egui::Margin::same(pad))
            .show(ui, |ui| {
                let inner = size - egui::Vec2::splat(pad * 2.0);
                ui.set_min_size(inner);
                ui.set_max_width(inner.x);
                // Sin separación implícita entre elementos: dentro de una
                // tarjeta de altura fija, los 3 px que egui añade por su cuenta
                // entre cada par de widgets son 15 px al final de la cuenta, y
                // el contenido se sale por abajo. El ritmo vertical lo ponen
                // los `add_space` de cada tarjeta, que sí se pueden sumar.
                ui.spacing_mut().item_spacing.y = 0.0;
                add(ui);
            });
    });
}

/// Celda de tabla de ancho exacto: así las columnas casan fila a fila en vez de
/// bailar con el contenido más largo de cada una.
fn cell(ui: &mut egui::Ui, w: f32, h: f32, align_right: bool, txt: egui::RichText) {
    let layout = if align_right {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.allocate_ui_with_layout(egui::vec2(w, h), layout, |ui| {
        ui.set_min_size(egui::vec2(w, h));
        ui.add(egui::Label::new(txt).truncate());
    });
}

/// Rótulo de sección: el escalón tipográfico que separa bloques en la V2.
fn section(ui: &mut egui::Ui, title: &str, sub: Option<String>) {
    ui.add_space(GAP + 4.0);
    row_align(ui, 18.0, egui::Align::Center, |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add(egui::Label::new(theme::instrument_label(title, theme::FAINT)));
        if let Some(s) = sub {
            ui.label(
                egui::RichText::new(s)
                    .size(theme::FS_CAPTION)
                    .color(theme::FAINT),
            );
        }
    });
    ui.add_space(8.0);
}

/// Título DENTRO de una tarjeta: icono en acento + rótulo de instrumento.
///
/// El icono en acento y la etiqueta en `faint` es el patrón del CSS, y es lo
/// que hace que el ojo encuentre la sección antes de leerla.
fn panel_title(ui: &mut egui::Ui, icon: &str, title: &str) {
    row_align(ui, 16.0, egui::Align::Center, |ui| {
        ui.spacing_mut().item_spacing.x = 7.0;
        ui.label(egui::RichText::new(icon).size(13.0).color(theme::ACC));
        ui.add(egui::Label::new(theme::instrument_label(title, theme::FAINT)));
    });
}

/// ¿Se anima? La V2 respeta `prefers-reduced-motion`; egui no expone esa
/// preferencia del sistema, así que aquí la puerta es `LUCY_NO_MOTION=1`.
///
/// Se lee UNA vez: consultar el entorno en cada frame de cada barra sería una
/// llamada al sistema por cada núcleo, sesenta veces por segundo.
fn motion() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("LUCY_NO_MOTION").unwrap_or_default() != "1")
}

/// `--ease-out`, la curva de entrada del CSS: rápida al principio y asentándose
/// al final. Un movimiento lineal se nota mecánico justo porque nada en el
/// mundo físico arranca y para de golpe.
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Envuelve un bloque en su opacidad de entrada.
fn block(ui: &mut egui::Ui, t: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.scope(|ui| {
        ui.multiply_opacity(t);
        add(ui);
    });
}

/// Añade una muestra al historial de una línea de tendencia.
fn push_hist(h: &mut Vec<f32>, v: f32) {
    h.push(v);
    if h.len() > SPARK_SAMPLES {
        h.remove(0);
    }
}

/// Porcentaje de ocupación de un volumen. Una función y no la división suelta
/// porque aparece en tres sitios y dos redondeos distintos no cuadran.
fn disk_pct(d: &lucy_core::system::DiskInfo) -> f32 {
    if d.total == 0 {
        return 0.0;
    }
    d.total.saturating_sub(d.avail) as f32 / d.total as f32 * 100.0
}

/// Línea de tendencia. Se dibuja con el painter porque no hay widget para
/// esto: son N puntos, un trazo, y un recorte.
///
/// El recorte es la animación `spark-reveal` del CSS: la línea se descubre de
/// izquierda a derecha la primera vez que hay datos, que es lo que la convierte
/// en "esto acaba de medirse" en vez de en un adorno.
fn sparkline(ui: &mut egui::Ui, w: f32, h: f32, data: &[f32], color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    if data.len() < 2 {
        return;
    }
    let reveal = if motion() {
        ease_out(
            ui.ctx()
                .animate_bool_with_time(ui.id().with("spark"), true, theme::DUR_SLOW),
        )
    } else {
        1.0
    };
    let step = rect.width() / (data.len() - 1) as f32;
    let pts: Vec<egui::Pos2> = data
        .iter()
        .enumerate()
        .map(|(i, v)| {
            egui::pos2(
                rect.left() + i as f32 * step,
                rect.bottom() - v.clamp(0.0, 100.0) / 100.0 * rect.height(),
            )
        })
        .collect();
    ui.painter_at(egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width() * reveal, rect.height()),
    ))
    .add(egui::Shape::line(pts, egui::Stroke::new(1.5_f32, color)));
}

/// Barra de medida con el relleno animado, como la transición de `width` del
/// CSS: al entrar crece desde cero y en cada sondeo se desliza al nuevo valor.
///
/// La duración es un parámetro porque el CSS usa dos: `--dur-slow` para las
/// métricas grandes y los discos, y `--dur-base` para los núcleos. No es
/// capricho — treinta y dos barras moviéndose a la velocidad de una sola tarjeta
/// grande se ven pesadas, y son las que más a menudo cambian.
fn meter(ui: &mut egui::Ui, w: f32, h: f32, frac: f32, color: egui::Color32, key: &str, dur: f32) {
    let frac = frac.clamp(0.0, 1.0);
    let shown = if motion() {
        ui.ctx()
            .animate_value_with_time(egui::Id::new(("meter", key)), frac, dur)
    } else {
        frac
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let r = egui::Rounding::same(h / 2.0);
    ui.painter().rect_filled(rect, r, theme::BG4);
    if shown > 0.0 {
        let fill = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * shown, rect.height()));
        ui.painter().rect_filled(fill, r, color);
    }
}

/// Fila de un servicio detenido: LED + nombre en mono.
///
/// El LED es neutro por defecto. Estuvo en ámbar para todas las filas, y una
/// máquina normal parecía un muro de avisos donde las filas que importaban no
/// tenían contra qué destacar.
fn svc_row(ui: &mut egui::Ui, w: f32, name: &str, crashed: bool) {
    let h = 18.0;
    ui.allocate_ui_with_layout(
        egui::vec2(w, h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(w, h));
            ui.spacing_mut().item_spacing.x = 8.0;
            let (dot, txt) = if crashed {
                (theme::AMBER, theme::TXT)
            } else {
                (theme::FAINT, theme::TXT2)
            };
            let (r, _) = ui.allocate_exact_size(egui::vec2(5.0, 5.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 2.5, dot);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(name)
                        .size(theme::FS_CAPTION)
                        .monospace()
                        .color(txt),
                )
                .truncate(),
            )
            .on_hover_text(if crashed {
                "Salió con código de error"
            } else {
                "Detenido, sin error de arranque"
            });
        },
    );
}

/// Tarjeta de un volumen: punto de montaje, porcentaje, barra y espacio libre.
fn disk_card(ui: &mut egui::Ui, w: f32, d: &lucy_core::system::DiskInfo) {
    let pct = disk_pct(d);
    card(ui, egui::vec2(w, DISK_H), 12.0, |ui| {
        row_align(ui, 18.0, egui::Align::Center, |ui| {
            ui.label(
                egui::RichText::new(&d.mount)
                    .monospace()
                    .size(theme::FS_FOOTNOTE)
                    .color(theme::TXT),
            );
            right(ui, 18.0, |ui| {
                ui.label(
                    egui::RichText::new(format!("{pct:.0}%"))
                        .size(theme::FS_FOOTNOTE)
                        .monospace()
                        .color(theme::disk_color(pct)),
                );
            });
        });
        ui.add_space(8.0);
        meter(ui, w - 24.0, 5.0, pct / 100.0, theme::disk_color(pct), &d.mount, theme::DUR_SLOW);
        ui.add_space(8.0);
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!(
                    "{} libres · {} / {}",
                    fmt_gb(d.avail),
                    fmt_gb(d.total.saturating_sub(d.avail)),
                    fmt_gb(d.total)
                ))
                .size(theme::FS_CAPTION)
                .monospace()
                .color(theme::FAINT),
            )
            .truncate(),
        );
    });
}

/// La píldora del selector de equipo: icono en acento, nombre, y el chevron.
///
/// Se mide y se pinta a mano en vez de usar un `Button` porque el icono va en
/// acento y el nombre en secundario — dos colores en un control, que es lo que
/// hace que se lea como un selector y no como un botón cualquiera.
fn host_pill(ui: &mut egui::Ui, icon: icons::Icon, name: &str) -> egui::Response {
    let font = egui::FontId::proportional(theme::FS_FOOTNOTE);
    let nw = ui.fonts(|f| f.layout_no_wrap(name.to_string(), font.clone(), theme::TXT3).size().x);
    let (iw, chev) = (15.0, 13.0);
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(10.0 + iw + 6.0 + nw + 6.0 + chev + 10.0, 24.0),
        egui::Sense::click(),
    );
    ui.painter().rect(
        rect,
        egui::Rounding::same(theme::R_SM),
        if resp.hovered() { theme::BG4 } else { theme::BG3 },
        egui::Stroke::new(1.0_f32, theme::BDR),
    );
    let cy = rect.center().y;
    icons::draw(
        ui.painter(),
        icon,
        egui::pos2(rect.left() + 10.0 + iw / 2.0, cy),
        15.0,
        theme::ACC,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 10.0 + iw + 6.0, cy),
        egui::Align2::LEFT_CENTER,
        name,
        font,
        theme::TXT3,
    );
    icons::draw(
        ui.painter(),
        icons::Icon::ChevronDown,
        egui::pos2(rect.right() - 11.0, cy),
        13.0,
        theme::TXT3,
    );
    resp
}

/// Una entrada del menú de equipos: icono, nombre, y la etiqueta del transporte.
///
/// El nombre se recorta contra la etiqueta en vez de empujarla fuera: un equipo
/// con nombre largo no debe poder esconder CÓMO se llega a él, que es el dato
/// que dice si va por WinRM o por SSH.
fn host_option(
    ui: &mut egui::Ui,
    w: f32,
    icon: icons::Icon,
    name: &str,
    kind: &str,
    sel: bool,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 30.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::Rounding::same(theme::R_SM), theme::BG4);
    }
    let font = egui::FontId::proportional(theme::FS_FOOTNOTE);
    let small = egui::FontId::proportional(theme::FS_CAPTION);
    let cy = rect.center().y;
    icons::draw(
        ui.painter(),
        icon,
        egui::pos2(rect.left() + 17.0, cy),
        16.0,
        if sel { theme::ACC } else { theme::TXT3 },
    );
    let chip_w = ui.fonts(|f| {
        f.layout_no_wrap(kind.to_string(), small.clone(), theme::FAINT)
            .size()
            .x
    }) + 12.0;
    let chip = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 10.0 - chip_w, cy - 8.0),
        egui::vec2(chip_w, 16.0),
    );
    ui.painter()
        .rect_filled(chip, egui::Rounding::same(5.0), theme::BG4);
    ui.painter().text(
        chip.center(),
        egui::Align2::CENTER_CENTER,
        kind,
        small,
        theme::FAINT,
    );
    let left = rect.left() + 30.0;
    ui.painter()
        .with_clip_rect(egui::Rect::from_min_max(
            egui::pos2(left, rect.top()),
            egui::pos2(chip.left() - 8.0, rect.bottom()),
        ))
        .text(
            egui::pos2(left, cy),
            egui::Align2::LEFT_CENTER,
            name,
            font,
            if sel { theme::ACC } else { theme::TXT2 },
        );
    resp.clicked()
}

/// La píldora del selector de modelo. Como la del host, pero el nombre se
/// recorta: "Gemini 3.5 Flash — Rendimiento de frontera sostenido" no cabe, y
/// truncarlo es mejor que dejar que empuje la cabecera entera.
fn model_pill(ui: &mut egui::Ui, icon: &str, name: &str, max_w: f32) -> egui::Response {
    let font = egui::FontId::proportional(theme::FS_FOOTNOTE);
    let tw = |ui: &egui::Ui, s: &str| {
        ui.fonts(|f| f.layout_no_wrap(s.to_string(), font.clone(), theme::TXT3).size().x)
    };
    let (iw, chev) = (tw(ui, icon), 12.0);
    let fixed = 10.0 + iw + 6.0 + 6.0 + chev + 10.0;
    let name_w = tw(ui, name).min((max_w - fixed).max(60.0));
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(fixed + name_w, 26.0), egui::Sense::click());
    ui.painter().rect(
        rect,
        egui::Rounding::same(theme::R_SM),
        if resp.hovered() { theme::BG4 } else { theme::BG3 },
        egui::Stroke::new(1.0_f32, theme::BDR),
    );
    let cy = rect.center().y;
    ui.painter().text(
        egui::pos2(rect.left() + 10.0, cy),
        egui::Align2::LEFT_CENTER,
        icon,
        font.clone(),
        theme::ACC,
    );
    let nx = rect.left() + 10.0 + iw + 6.0;
    ui.painter()
        .with_clip_rect(egui::Rect::from_min_max(
            egui::pos2(nx, rect.top()),
            egui::pos2(nx + name_w, rect.bottom()),
        ))
        .text(
            egui::pos2(nx, cy),
            egui::Align2::LEFT_CENTER,
            name,
            font.clone(),
            theme::TXT3,
        );
    icons::draw(
        ui.painter(),
        icons::Icon::ChevronDown,
        egui::pos2(rect.right() - 11.0, cy),
        13.0,
        theme::TXT3,
    );
    resp
}

/// Una entrada del desplegable de modelos.
fn model_option(ui: &mut egui::Ui, w: f32, icon: &str, name: &str, sel: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 26.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::Rounding::same(theme::R_SM), theme::BG4);
    }
    let font = egui::FontId::proportional(theme::FS_FOOTNOTE);
    let cy = rect.center().y;
    let col = if sel { theme::ACC } else { theme::TXT2 };
    ui.painter().text(
        egui::pos2(rect.left() + 10.0, cy),
        egui::Align2::LEFT_CENTER,
        icon,
        font.clone(),
        col,
    );
    let nx = rect.left() + 28.0;
    ui.painter()
        .with_clip_rect(egui::Rect::from_min_max(
            egui::pos2(nx, rect.top()),
            egui::pos2(rect.right() - 8.0, rect.bottom()),
        ))
        .text(egui::pos2(nx, cy), egui::Align2::LEFT_CENTER, name, font, col);
    resp.clicked()
}

/// Ancho que ocupará un chip de sugerencia. Se necesita ANTES de dibujarlo para
/// poder centrar la fila: egui no sabe cuánto mide una fila hasta que la coloca.
fn chip_w(ui: &egui::Ui, label: &str) -> f32 {
    let font = egui::FontId::proportional(theme::FS_FOOTNOTE);
    let w = ui.fonts(|f| f.layout_no_wrap(label.to_string(), font, theme::TXT2).size().x);
    w + 46.0
}

/// Chip de sugerencia del estado vacío.
fn chip(ui: &mut egui::Ui, icon: &str, label: &str) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(chip_w(ui, label), 30.0), egui::Sense::click());
    ui.painter().rect(
        rect,
        egui::Rounding::same(999.0),
        if resp.hovered() { theme::BG4 } else { theme::BG3 },
        egui::Stroke::new(
            1.0_f32,
            if resp.hovered() { theme::ACC_LINE } else { theme::BDR },
        ),
    );
    let font = egui::FontId::proportional(theme::FS_FOOTNOTE);
    let cy = rect.center().y;
    ui.painter().text(
        egui::pos2(rect.left() + 14.0, cy),
        egui::Align2::LEFT_CENTER,
        icon,
        font.clone(),
        theme::ACC,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 32.0, cy),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        theme::TXT2,
    );
    resp.clicked()
}

/// El estado vacío de un carril del workspace: el glifo en su baldosa, el
/// título, y para qué sirve el panel.
fn ws_empty(ui: &mut egui::Ui, tab: WsTab) {
    let (glyph, title, hint) = tab.empty();
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        egui::Frame::none()
            .fill(theme::BG3)
            .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
            .rounding(egui::Rounding::same(theme::R_MD))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(glyph).size(24.0).color(theme::FAINT));
            });
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(title)
                .size(theme::FS_FOOTNOTE)
                .color(theme::TXT2),
        );
        ui.add_space(6.0);
        ui.set_max_width(230.0);
        ui.label(
            egui::RichText::new(hint)
                .size(theme::FS_CAPTION)
                .color(theme::TXT3),
        );
    });
}

/// Duración legible. Por debajo de un segundo, en milisegundos: un turno de
/// 800 ms redondeado a "1 s" oculta justo la diferencia que se quería medir.
fn fmt_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{} m {} s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

/// Chip de un fichero adjunto. Devuelve `true` si se pulsó la ✕.
///
/// Un adjunto que no se va a mandar se dibuja en ámbar y explica por qué al
/// pasar el cursor. Ir tachado y en silencio sería lo mismo que no estar.
fn attach_chip(ui: &mut egui::Ui, a: &Attachment) -> bool {
    let ok = a.blocked.is_empty();
    let col = if ok { theme::TXT2 } else { theme::AMBER };
    let mut quitar = false;
    let r = egui::Frame::none()
        .fill(theme::BG4)
        .stroke(egui::Stroke::new(
            1.0_f32,
            if ok { theme::BDR } else { theme::AMBER.linear_multiply(0.4) },
        ))
        .rounding(egui::Rounding::same(999.0))
        .inner_margin(egui::Margin::symmetric(9.0, 3.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(egui::RichText::new(a.kind.glyph()).size(11.0).color(col));
            ui.label(
                egui::RichText::new(&a.name)
                    .size(theme::FS_CAPTION)
                    .color(col),
            );
            // El tamaño en CARACTERES y no en bytes: es lo que le va a costar al
            // modelo, que es la única unidad que importa aquí.
            if ok {
                ui.label(
                    egui::RichText::new(fmt_chars(a.content.chars().count()))
                        .size(theme::FS_CAPTION)
                        .color(theme::FAINT),
                );
            }
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("✕").size(10.0).color(theme::FAINT))
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE)
                        .min_size(egui::vec2(14.0, 14.0)),
                )
                .clicked()
            {
                quitar = true;
            }
        })
        .response;
    if !ok {
        r.on_hover_text(format!("No se enviará: {}", a.blocked));
    }
    quitar
}

/// Cuenta de caracteres abreviada.
fn fmt_chars(n: usize) -> String {
    if n >= 1000 {
        format!("{:.0}k", n as f64 / 1000.0)
    } else {
        format!("{n}")
    }
}

/// ¿Hay clave guardada para este proveedor?
///
/// Se pregunta al almacén del sistema una sola vez por proveedor y por sesión.
/// El desplegable redibuja sesenta veces por segundo mientras está abierto, y
/// consultar el Credential Manager en cada frame son siete llamadas al sistema
/// por fotograma. El valor de la clave NUNCA sale de aquí — solo si existe.
fn with_key(provider: &str) -> bool {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut c) = cache.lock() else { return true };
    if let Some(v) = c.get(provider) {
        return *v;
    }
    // Ollama no lleva clave: es local.
    let v = provider == "ollama"
        || keyring::Entry::new("LucySysAdmin", &format!("{provider}_api_key"))
            .and_then(|e| e.get_password())
            .is_ok();
    c.insert(provider.to_string(), v);
    v
}

/// Un comando ejecutado, en UNA línea plegable.
///
/// Antes esto era una burbuja de operador con avatar, hora y ancho completo que
/// decía "Resultado devuelto a Lucy" — y no decía nada más. Ocupaba lo mismo que
/// algo que una persona había escrito, se acumulaba una por comando, y para ver
/// la salida había que irse al panel de Ejecución.
///
/// Ahora es lo que es: una línea con el comando, si fue bien, y la salida
/// dentro. Plegada por defecto porque en el 90 % de los casos basta con saber
/// que corrió; abierta cuando hace falta mirar.
fn exec_row(ui: &mut egui::Ui, i: usize, cmd: &str, ok: bool, out: &str) {
    ui.add_space(6.0);
    let (col, glyph) = if ok {
        (theme::ACC, "✓")
    } else {
        (theme::RED, "✕")
    };
    egui::CollapsingHeader::new(
        egui::RichText::new(format!("{glyph}  {cmd}"))
            .size(theme::FS_CAPTION)
            .monospace()
            .color(col),
    )
    .id_salt(("exec", i))
    .show(ui, |ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(out)
                    .size(theme::FS_CAPTION)
                    .monospace()
                    .color(theme::TXT3),
            )
            .wrap(),
        );
    });
}

/// Avatar cuadrado de 28 px con esquinas redondeadas, como el del CSS.
fn avatar(ui: &mut egui::Ui, txt: &str, fg: egui::Color32, bg: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::Rounding::same(theme::R_SM), bg);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        txt,
        egui::FontId::proportional(theme::FS_CAPTION),
        fg,
    );
}

/// La fila de acciones de un mensaje: la hora y el botón de copiar.
///
/// En el CSS aparecen al pasar el cursor. Aquí están siempre pero en `faint`:
/// egui no sabe si el cursor está sobre un bloque hasta después de dibujarlo,
/// así que ocultarlas costaría un frame de retraso y un parpadeo — peor negocio
/// que un icono discreto que no se mueve.
fn msg_actions(ui: &mut egui::Ui, stamp: &str, right_aligned: bool) -> bool {
    let mut copiar = false;
    let layout = if right_aligned {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 20.0), layout, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if ui
            .add(
                egui::Button::new(egui::RichText::new("⎘").size(11.0).color(theme::FAINT))
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE)
                    .min_size(egui::vec2(20.0, 18.0)),
            )
            .on_hover_text("Copiar")
            .clicked()
        {
            copiar = true;
        }
        if !stamp.is_empty() {
            ui.label(
                egui::RichText::new(stamp)
                    .size(theme::FS_MICRO)
                    .monospace()
                    .color(theme::FAINT),
            );
        }
    });
    copiar
}

/// Botón de icono sin relleno ni borde — los del compositor.
///
/// Se aclara al pasar el cursor. Es la única respuesta que tiene un botón sin
/// fondo, y sin ella no hay forma de saber que es pulsable hasta pulsarlo.
fn ghost_icon(ui: &mut egui::Ui, icon: icons::Icon) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
    let c = if resp.hovered() { theme::TXT } else { theme::TXT3 };
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::Rounding::same(6.0), theme::BG4);
    }
    icons::draw(ui.painter(), icon, rect.center(), 17.0, c);
    resp
}

/// Un lado de un control segmentado. Activo = relleno de acento con tinta
/// oscura encima, que es el único sitio donde el CSS pone el acento sólido.
fn seg(ui: &mut egui::Ui, label: &str, on: bool) -> bool {
    let b = egui::Button::new(
        egui::RichText::new(label)
            .size(theme::FS_CAPTION)
            .color(if on { theme::ACC_INK } else { theme::TXT3 }),
    )
    .fill(if on {
        theme::ACC
    } else {
        egui::Color32::TRANSPARENT
    })
    .stroke(egui::Stroke::NONE)
    .rounding(egui::Rounding::same(6.0))
    .min_size(egui::vec2(40.0, 18.0));
    ui.add(b).clicked()
}

/// Los datos de una tarjeta KPI.
///
/// Struct y no nueve argumentos posicionales: con nueve, `color` y `sub` se
/// intercambian sin que el compilador diga nada.
struct Kpi<'a> {
    icon: &'a str,
    title: &'a str,
    /// Cifra: se anima desde el valor anterior. Ignorada si `text` no está vacío.
    value: f32,
    unit: &'a str,
    /// Texto en lugar de cifra — el hostname. No se anima ni va a 28 pt: un
    /// nombre de equipo no es una lectura, es una etiqueta.
    text: String,
    /// Historial de la línea de tendencia. La V2 la pone en CPU y RAM, donde el
    /// valor se mueve; en disco no, porque un disco no cambia en 44 segundos y
    /// la línea sería una recta. Ahí va una barra de ocupación, que sí dice algo.
    spark: &'a [f32],
    bar: Option<f32>,
    sub: String,
    /// Segunda línea de detalle. Existe porque la tarjeta de SISTEMA necesita
    /// dos y un `\n` no serviría: el truncado que impide que un nombre largo
    /// desborde la columna corta el texto en la primera línea.
    sub2: String,
}

impl Default for Kpi<'_> {
    fn default() -> Self {
        Self {
            icon: "",
            title: "",
            value: 0.0,
            unit: "",
            text: String::new(),
            spark: &[],
            bar: None,
            sub: String::new(),
            sub2: String::new(),
        }
    }
}

/// Los cuatro carriles del workspace del agente.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WsTab {
    Plan,
    Exec,
    Trace,
    Artifacts,
}

impl WsTab {
    const ALL: [WsTab; 4] = [WsTab::Plan, WsTab::Exec, WsTab::Trace, WsTab::Artifacts];

    fn label(self) -> &'static str {
        match self {
            Self::Plan => "Plan",
            Self::Exec => "Ejecución",
            Self::Trace => "Trace",
            Self::Artifacts => "Artefactos",
        }
    }

    /// El estado vacío de cada carril: glifo, título y qué llenará el panel.
    ///
    /// La pista importa más que el título: un panel vacío que solo dice "sin
    /// nada" no enseña para qué sirve, y estos cuatro solo se llenan cuando el
    /// agente trabaja — es decir, casi nunca la primera vez que se miran.
    fn empty(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Plan => (
                "▤",
                "Sin plan todavía",
                "Lucy desglosa la tarea en pasos y los va marcando conforme avanza.",
            ),
            Self::Exec => (
                "▸",
                "Nada ejecutado aún",
                "La salida de cada comando aparece aquí en vivo mientras el agente trabaja.",
            ),
            Self::Trace => (
                "◈",
                "Trace vacío",
                "El razonamiento del agente — pensar · actuar · observar — se registra aquí.",
            ),
            Self::Artifacts => (
                "▥",
                "Sin artefactos",
                "Los archivos que Lucy edita o escribe aparecen aquí con su diff.",
            ),
        }
    }
}

/// Gravedad de una alerta derivada. Dos niveles, como la V2: uno colorea el
/// chip del equipo de ámbar y el otro de rojo.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sev {
    Warn,
    Bad,
}

struct App {
    view: View,
    // chat — Ollama local REAL (streaming vía lucy_core::chat)
    md_cache: CommonMarkCache,
    /// Las terminales abiertas. Siempre hay al menos una.
    tabs: Vec<ChatTab>,
    tab: usize,
    /// Cuántas se han abierto en total — numera las nuevas sin reutilizar el
    /// nombre de una que se cerró.
    tabs_opened: usize,
    chat_model: String,
    /// Texto del buscador del desplegable de modelos.
    model_query: String,
    /// El retrato de Lucy. Se sube una vez, en el primer frame que lo necesita:
    /// subir una textura exige el contexto, y `new` todavía no lo tiene.
    face: Option<egui::TextureHandle>,
    /// Qué carril del workspace se está mirando. Global a propósito: es una
    /// preferencia de quien mira, no un dato de la conversación.
    ws_tab: WsTab,
    models: Vec<String>,
    // terminal — VT real: el PTY emite bytes crudos, el parser los interpreta en
    // una pantalla de terminal (sin escapes visibles).
    pty: Option<Pty>,
    vt: vt100::Parser,
    term_input: String,
    // memoria
    mems: Result<Vec<AgentMemory>, String>,
    mem_search: String,
    /// Último resultado semántico: `None` = no se ha buscado todavía.
    /// Los avisos viajan CON los aciertos porque describen ese resultado
    /// concreto — separarlos es cómo acaban desincronizados.
    #[allow(clippy::type_complexity)]
    sem_result: Option<Result<(Vec<lucy_core::vectors::SemanticHit>, Vec<String>), String>>,
    // log viewer
    log_lines: Result<Vec<String>, String>,
    log_error: bool,
    log_warn: bool,
    log_info: bool,
    // sistema (métricas live vía lucy_core::system)
    sys: lucy_core::system::SysMonitor,
    sys_last: Instant,
    /// Caudal de red de la última medición. Se guarda porque `net_rate()` es
    /// destructivo: calcula contra la lectura anterior y la reemplaza. Llamarlo
    /// en cada frame daría deltas de 16 ms — ruido, no caudal.
    net: lucy_core::system::NetRate,
    procs: Vec<lucy_core::system::ProcInfo>,
    proc_by_cpu: bool,
    services: Vec<lucy_core::system::DownService>,
    /// Cadencias separadas por COSTE, no por gusto: los medidores van a 1 s,
    /// los procesos a 3 s (`refresh_processes` recorre la tabla entera) y los
    /// servicios a 30 s (lanza un PowerShell). Una sola cadencia obligaría a
    /// elegir entre medidores lentos o un PowerShell por segundo.
    procs_last: Instant,
    svc_last: Instant,
    /// Un comando aprobado que se está ejecutando: el id de su paso del plan y
    /// el canal por el que llegará `(salida, error, ok, ms)`.
    ///
    /// Uno cada vez. Dos comandos a la vez sobre la misma máquina es lo que
    /// convierte un diagnóstico en una carrera, y no hay ninguna prisa que lo
    /// justifique cuando cada uno lo aprueba una persona.
    exec_rx: Option<(usize, String, std::sync::mpsc::Receiver<(String, String, bool, u64)>)>,
    /// Sonda de servicios en vuelo. `Some` = hay un hilo trabajando, que es lo
    /// que anima el botón de refresco y lo que impide lanzar una segunda.
    svc_rx: Option<std::sync::mpsc::Receiver<Option<Vec<lucy_core::system::DownService>>>>,
    /// Historial de las líneas de tendencia de CPU y RAM.
    cpu_hist: Vec<f32>,
    ram_hist: Vec<f32>,
    /// Los equipos dados de alta, leídos del Credential Manager. Ver `hosts`.
    remote_hosts: Vec<hosts::Host>,
    /// Equipo seleccionado: `"local"` o el `id` de uno remoto.
    selected_host: String,
    /// Cuándo entró el Dashboard en pantalla. Gobierna la animación de entrada;
    /// `None` mientras la vista no está visible, y así al volver vuelve a
    /// montarse como en la V2 en vez de aparecer de golpe.
    dash_shown: Option<Instant>,
    /// Hora de la última actualización, para la cabecera.
    sys_stamp: String,
    // telemetry
    last: Instant,
    fps: f32,
    /// Última vez que hubo ALGO que animar: tokens llegando, salida del PTY.
    ///
    /// Gobierna la política de repintado. Ver `update()`: repintar sin
    /// condición es lo que demuestra la propiedad anti-congelamiento, y también
    /// lo que fija un núcleo al máximo con la ventana quieta enseñando una
    /// lista estática. Lucy vive abierta todo el día en la estación de trabajo
    /// de alguien; "nativa" no puede significar "gasta más parada que el
    /// WebView trabajando".
    last_activity: Instant,
}

/// Clave del modelo elegido en el almacén de eframe.
const K_MODEL: &str = "lucy.chat_model";

/// El modelo con el que arranca una instalación nueva.
///
/// Antes era "el primero que devolviera Ollama, y si no `qwen3:4b`", que es una
/// forma de decir "el que sea": en una máquina sin Ollama arrancaba apuntando a
/// un modelo local que no existe, y la primera orden fallaba antes de llegar a
/// ninguna parte. Gemini 3.5 Flash es la elección de la V2 y aguanta el trabajo
/// de agente sin ser el más caro del catálogo.
const DEFAULT_MODEL: &str = "gemini-3.5-flash";

impl App {
    fn new(storage: Option<&dyn eframe::Storage>) -> Self {
        let models = lucy_core::chat::list_models();
        // Lo que el operador eligió la última vez manda sobre el valor por
        // defecto: cambiar de modelo en cada arranque es la clase de fricción
        // que hace que se deje de cambiar.
        let chat_model = storage
            .and_then(|s| s.get_string(K_MODEL))
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        Self {
            view: View::TerminalIa,
            md_cache: CommonMarkCache::default(),
            tabs: vec![ChatTab::new(0)],
            tab: 0,
            tabs_opened: 1,
            chat_model,
            model_query: String::new(),
            face: None,
            ws_tab: WsTab::Plan,
            models,
            pty: Pty::spawn(140, 44).ok(),
            vt: vt100::Parser::new(44, 140, 4000),
            term_input: String::new(),
            mems: load_memories(),
            mem_search: String::new(),
            sem_result: None,
            log_lines: log_path()
                .ok_or_else(|| "no se pudo resolver %APPDATA%".to_string())
                .and_then(|p| lucy_core::logs::tail(&p, 2_000)),
            // Error y Warn encendidos, Info apagado: quien abre un visor de logs
            // suele venir buscando qué falló, no la narración completa. Se
            // enciende con un clic.
            log_error: true,
            log_warn: true,
            log_info: false,
            sys: lucy_core::system::SysMonitor::new(),
            sys_last: Instant::now(),
            net: lucy_core::system::NetRate::default(),
            procs: Vec::new(),
            proc_by_cpu: false,
            services: Vec::new(),
            // Instantes en el pasado para que las tres cadencias disparen en el
            // primer frame: el dashboard tiene que abrirse con datos, no vacío
            // esperando 30 segundos a que aparezcan los servicios.
            procs_last: Instant::now() - Duration::from_secs(60),
            svc_last: Instant::now() - Duration::from_secs(60),
            exec_rx: None,
            svc_rx: None,
            cpu_hist: Vec::new(),
            ram_hist: Vec::new(),
            remote_hosts: hosts::load(),
            selected_host: "local".to_string(),
            dash_shown: None,
            sys_stamp: String::from("—"),
            last: Instant::now(),
            fps: 0.0,
            last_activity: Instant::now(),
        }
    }

    /// Drena los tokens que va emitiendo el hilo del chat de Ollama.
    /// Vacía los canales de TODAS las pestañas, no solo la visible.
    ///
    /// Una pestaña de fondo tiene que seguir recibiendo: en la V2 se lanza una
    /// tarea larga, se cambia de terminal a trabajar en otra cosa y se vuelve
    /// con la respuesta ya escrita. Bombear solo la activa dejaría el canal
    /// llenándose y la respuesta llegaría de golpe al volver.
    fn pump_chat(&mut self) {
        // Los turnos que se cierran en esta pasada, con lo que llegó a escribir
        // cada uno. Se anotan y se procesan DESPUÉS del bucle: dentro, `self`
        // está prestado por las pestañas y el workspace no se puede tocar.
        // Con el UID de su pestaña: el turno que se cierra puede no ser el de la
        // pestaña que se está mirando.
        let mut cerrados: Vec<(usize, String)> = Vec::new();
        for t in &mut self.tabs {
            if t.rx.is_none() {
                continue;
            }
            let mut done = false;
            if let Some(rx) = &t.rx {
                while let Ok(ev) = rx.try_recv() {
                    match ev {
                        // NO va directo al mensaje: entra en la cola y se
                        // revela a ritmo. Pintarlo en cuanto llega es lo que
                        // hace que el texto salte en bloques.
                        lucy_core::chat::ChatEvent::Token(tok) => t.drain.push(&tok),
                        lucy_core::chat::ChatEvent::Done => {
                            done = true;
                            break;
                        }
                        lucy_core::chat::ChatEvent::Error(e) => {
                            if let Some(last) = t.log.last_mut() {
                                last.text.push_str(&format!("\n\n⚠ {e}"));
                            }
                            done = true;
                            break;
                        }
                    }
                }
            }
            if done {
                t.rx = None;
                // El turno se cierra con el texto COMPLETO, no con lo que se
                // haya alcanzado a enseñar: los carriles del workspace y el
                // recuento son del contenido, no del ritmo al que se pinta. Lo
                // que quede en cola sigue escribiéndose después.
                let reply = format!(
                    "{}{}",
                    t.log.last().map(|m| m.text.clone()).unwrap_or_default(),
                    t.drain.peek()
                );
                cerrados.push((t.uid, reply));
            }
        }
        for (uid, reply) in cerrados {
            self.absorb_tags(uid, &reply);
            self.turn_finished(uid, reply.chars().count());
        }
    }

    /// Mueve al mensaje visible lo que la cola deje salir en este frame.
    ///
    /// Sobre TODAS las pestañas: una de fondo que sigue escribiendo tiene que
    /// terminar de hacerlo, o al volver aparecería el resto de golpe.
    fn pump_drain(&mut self, now: Instant) -> bool {
        let mut vivo = false;
        for t in &mut self.tabs {
            let out = t.drain.tick(now);
            if !out.is_empty() {
                if let Some(last) = t.log.last_mut() {
                    last.text.push_str(&out);
                }
            }
            vivo |= t.drain.busy();
        }
        vivo
    }
}

impl eframe::App for App {
    /// eframe lo llama al cerrar y cada pocos minutos. Solo se guarda la
    /// elección de modelo: las conversaciones son de la sesión, y persistirlas
    /// sin que nadie lo haya pedido es guardar en disco lo que el operador
    /// escribió sin decírselo.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(K_MODEL, self.chat_model.clone());
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        if dt > 0.0 {
            self.fps = 0.9 * self.fps + 0.1 * (1.0 / dt);
        }

        self.pump_chat();
        // Cualquier pestaña con stream abierto cuenta, no solo la visible: la de
        // fondo también está escribiendo y su texto tiene que llegar entero.
        // `chat_rx` está en Some mientras corre un stream: eso ES actividad,
        // aunque este frame concreto no traiga token.
        // La cola cuenta como actividad: mientras quede texto por revelar hay
        // que repintar, aunque el stream ya haya terminado.
        let mut live = self.tabs.iter().any(ChatTab::busy);
        live |= self.pump_drain(now);
        if let Some(pty) = &self.pty {
            let bytes = pty.drain_bytes();
            if !bytes.is_empty() {
                self.vt.process(&bytes); // interpreta los escapes VT → pantalla limpia
                live = true;
            }
        }
        // Solo se mide lo que se está mirando. Refrescar métricas mientras el
        // usuario lee el chat gasta CPU para pintar algo que no está en pantalla
        // — y en una app que vive abierta todo el día eso se nota en la batería.
        if self.view == View::Dashboard {
            // El reloj de la entrada arranca al ENTRAR en la vista, no al
            // arrancar la app: la V2 monta el componente cada vez, y volver al
            // dashboard vuelve a armarlo delante de ti.
            if self.dash_shown.is_none() {
                self.dash_shown = Some(now);
            }
            self.refresh_system(false);
        } else {
            self.dash_shown = None;
        }
        // Fuera del `if`: una sonda lanzada justo antes de cambiar de vista
        // tiene que poder cerrarse igual, o el botón se quedaría girando para
        // siempre al volver.
        self.pump_services();
        // Fuera de la vista de Terminal IA también: un comando aprobado tiene
        // que poder terminar aunque el operador se vaya al Dashboard a mirar
        // otra cosa mientras corre.
        self.pump_exec();

        // ── Política de repintado ────────────────────────────────────────────
        //
        // Aquí había un `ctx.request_repaint()` incondicional. Demuestra lo que
        // tiene que demostrar —no hay compositor de WebView que pueda pararse—
        // pero repinta a la velocidad del monitor PARA SIEMPRE, también con la
        // ventana quieta enseñando una lista de memorias que no cambia.
        //
        // La propiedad que buscamos no es "repintar siempre", es "repintar
        // cuando hay algo que mostrar, sin depender de que el usuario mueva el
        // ratón". Eso se conserva entero: mientras llegan tokens o el PTY
        // escribe, va a fondo; en reposo baja a 1 Hz — que es lo que la vista
        // de Sistema necesita de todos modos para su refresco de métricas — y
        // cualquier entrada lo despierta al instante, porque winit entrega los
        // eventos sin pedir permiso a nadie.
        //
        // Los 300 ms de cola evitan un tirón al final de un stream: los últimos
        // tokens y el cursor del terminal se asientan a velocidad completa en
        // vez de caer en seco a 1 Hz.
        if live {
            self.last_activity = now;
        }
        if now.duration_since(self.last_activity) < Duration::from_millis(300) {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(1000));
        }

        // ── cabecera ─────────────────────────────────────────────────────────
        //
        // ES la barra de título de la ventana. La del sistema está apagada
        // (`with_decorations(false)`), y esta ocupa su sitio: la V2 no enseña
        // una barra de Windows encima de su propio cromo, y con las dos la
        // aplicación tiene dos cabeceras de distinto color y dos alturas.
        //
        // Lo que había que reponer a mano al quitarla: arrastrar la ventana,
        // maximizar con doble clic, y los tres botones. El redimensionado lo
        // sigue dando winit por los bordes mientras la ventana sea `resizable`.
        egui::TopBottomPanel::top("header")
            .exact_height(44.0)
            .frame(egui::Frame::none().fill(theme::BG2).inner_margin(egui::Margin::symmetric(14.0, 0.0)))
            .show(ctx, |ui| {
                // EL ORDEN ES AL REVÉS DE LO QUE PARECE, y equivocarse deja la
                // ventana muerta. egui resuelve un solapamiento a favor del
                // widget registrado MÁS TARDE, así que la franja de arrastre va
                // PRIMERO y los botones después: registrada al final se quedaba
                // con todos los clics de la barra y ni cerrar funcionaba.
                let bar = ui.max_rect();
                let drag = ui.interact(
                    bar,
                    egui::Id::new("titlebar-drag"),
                    egui::Sense::click_and_drag(),
                );
                if drag.double_clicked() {
                    let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                } else if drag.is_pointer_button_down_on() {
                    // `is_pointer_button_down_on` y no `drag_started`: winit se
                    // queda con el ratón en cuanto empieza el arrastre nativo,
                    // así que egui nunca llega a ver el movimiento que
                    // convertiría la pulsación en arrastre.
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                ui.horizontal_centered(|ui| {
                    ui.label(egui::RichText::new("✦ Lucy").color(theme::ACC).strong().size(15.0));
                    ui.add_space(14.0);
                    ui.label(egui::RichText::new(self.view.label()).color(theme::TXT).size(13.5));
                    if self.view == View::TerminalIa {
                        ui.add_space(6.0);
                        // El badge COCKPIT de la app: fondo tenue del acento,
                        // versalitas, sin borde.
                        egui::Frame::none()
                            .fill(theme::ACC.linear_multiply(0.14))
                            .rounding(egui::Rounding::same(4.0))
                            .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("COCKPIT")
                                        .color(theme::ACC)
                                        .size(9.5)
                                        .strong(),
                                );
                            });
                    }
                    right(ui, 30.0, |ui| self.window_buttons(ui));
                });
            });

        // ── barra de estado ──────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("status")
            .exact_height(26.0)
            .frame(egui::Frame::none().fill(theme::BG2).inner_margin(egui::Margin::symmetric(14.0, 0.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let host = lucy_core::system::hostname();
                    ui.label(egui::RichText::new("●").color(theme::ACC).size(9.0));
                    ui.label(egui::RichText::new(host.to_uppercase()).color(theme::TXT3).size(10.5));
                    ui.add_space(10.0);
                    let (pty_glyph, pty_color) = if self.pty.is_some() {
                        ("▸ PTY", theme::TXT3)
                    } else {
                        ("✕ PTY", theme::AMBER)
                    };
                    ui.label(egui::RichText::new(pty_glyph).color(pty_color).size(10.5));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // El FPS marca ~1 en reposo A PROPÓSITO: se repinta a
                        // fondo solo cuando hay algo que animar. Se etiqueta
                        // para que nadie lo lea como un problema.
                        let idle = self.fps < 5.0;
                        ui.label(
                            egui::RichText::new(if idle {
                                "reposo".to_string()
                            } else {
                                format!("{:.0} FPS", self.fps)
                            })
                            .color(theme::TXT3)
                            .size(10.5),
                        );
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(&self.chat_model).color(theme::TXT3).size(10.5),
                        );
                    });
                });
            });

        // ── rail izquierdo ───────────────────────────────────────────────────
        egui::SidePanel::left("rail")
            .exact_width(96.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(theme::BG2).inner_margin(egui::Margin::symmetric(0.0, 10.0)))
            .show(ctx, |ui| {
                for v in View::ALL {
                    let label = v.label();
                    let active = self.view == v;
                    let pending = v.pending_needs().is_some();

                    // Tres estados, no dos: activa, disponible, y pendiente de
                    // migrar. La tercera se atenúa pero SIGUE siendo pulsable —
                    // su panel explica qué le falta, que es información útil.
                    let fg = if active {
                        theme::ACC
                    } else if pending {
                        theme::TXT3.linear_multiply(0.55)
                    } else {
                        theme::TXT2
                    };

                    let resp = ui.allocate_response(
                        egui::vec2(ui.available_width(), 46.0),
                        egui::Sense::click(),
                    );
                    if resp.clicked() {
                        self.view = v;
                    }
                    if active {
                        // Barra de acento a la izquierda, como el CSS.
                        let r = resp.rect;
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(r.min, egui::vec2(2.5, r.height())),
                            0.0,
                            theme::ACC,
                        );
                        ui.painter().rect_filled(
                            r.shrink2(egui::vec2(3.0, 2.0)),
                            4.0,
                            theme::ACC.linear_multiply(0.10),
                        );
                    } else if resp.hovered() {
                        ui.painter().rect_filled(
                            resp.rect.shrink2(egui::vec2(3.0, 2.0)),
                            4.0,
                            theme::BG3,
                        );
                    }
                    let c = resp.rect.center();
                    icons::draw(
                        ui.painter(),
                        v.icon(),
                        egui::pos2(c.x, c.y - 8.0),
                        20.0,
                        fg,
                    );
                    ui.painter().text(
                        egui::pos2(c.x, c.y + 12.0),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(9.5),
                        fg,
                    );
                }
            });

        // ── ficheros soltados en la ventana ──────────────────────────────────
        //
        // eframe los entrega sin que haya que registrar nada: winit ya escucha
        // el arrastre del sistema. Solo se recogen en Terminal IA — soltar un
        // log sobre el dashboard no significa nada, y aceptarlo ahí lo metería
        // en una conversación que no se está viendo.
        if self.view == View::TerminalIa {
            let soltados: Vec<std::path::PathBuf> = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect()
            });
            if !soltados.is_empty() {
                self.attach(&soltados);
            }
            // Mientras el fichero está en el aire, la ventana lo dice. Sin esta
            // señal, arrastrar sobre una aplicación es adivinar si va a aceptar.
            let encima = ctx.input(|i| i.raw.hovered_files.len());
            if encima > 0 {
                let r = ctx.screen_rect();
                let p = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("drop"),
                ));
                p.rect_filled(r, 0.0, theme::BG.linear_multiply(0.75));
                p.rect_stroke(
                    r.shrink(10.0),
                    egui::Rounding::same(theme::R_LG),
                    egui::Stroke::new(2.0_f32, theme::ACC),
                );
                p.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    if encima == 1 {
                        "Soltar para adjuntar".to_string()
                    } else {
                        format!("Soltar para adjuntar {encima} ficheros")
                    },
                    egui::FontId::proportional(18.0),
                    theme::ACC,
                );
            }
        }

        // ── carril derecho: el workspace del agente ──────────────────────────
        //
        // Solo en Terminal IA. En las demás vistas no hay turno del que enseñar
        // el plan, y un panel vacío permanente enseña a no mirarlo.
        if self.view == View::TerminalIa {
            egui::SidePanel::right("workspace")
                .exact_width(340.0)
                .resizable(false)
                .frame(
                    egui::Frame::none()
                        .fill(theme::BG2)
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
                )
                .show(ctx, |ui| self.workspace(ui));
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::TerminalIa => self.terminal_ia(ui),
            View::NexShell => self.terminal(ui),
            View::Memoria => self.memoria(ui),
            View::Dashboard => self.sistema(ui),
            View::LogViewer => self.log_viewer(ui),
            other => self.pendiente(ui, other),
        });
    }
}

impl App {
    /// Terminal IA — la vista principal del Cockpit.
    ///
    /// Orden de arriba abajo, como la V2: barra de pestañas, rótulo de la
    /// conversación con el selector de modelo a la derecha, la conversación, y
    /// el compositor abajo. El compositor VA ABAJO y no arriba: es donde está la
    /// mano cuando se acaba de leer una respuesta.
    fn terminal_ia(&mut self, ui: &mut egui::Ui) {
        self.tab_bar(ui);
        ui.add_space(6.0);

        row_align(ui, 20.0, egui::Align::Center, |ui| {
            ui.spacing_mut().item_spacing.x = 7.0;
            ui.label(egui::RichText::new("●").size(7.0).color(theme::ACC));
            ui.add(egui::Label::new(theme::instrument_label(
                "Conversación",
                theme::FAINT,
            )));
            let mut w = ui.available_width();
            // El selector se ancla a la derecha con su ancho real, no con un
            // hueco a ojo: el nombre del modelo cambia de largo al elegir otro.
            w = w.min(420.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 26.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.set_max_width(ui.available_width());
                    self.model_picker(ui, w);
                },
            );
        });
        ui.add_space(8.0);

        // El compositor se reserva ABAJO antes de dibujar la conversación: con
        // el orden natural, una conversación larga lo empujaría fuera de la
        // ventana justo cuando hace falta escribir.
        egui::TopBottomPanel::bottom("composer")
            .frame(egui::Frame::none().inner_margin(egui::Margin {
                top: 10.0,
                bottom: 4.0,
                ..Default::default()
            }))
            .show_separator_line(false)
            .show_inside(ui, |ui| self.composer(ui));

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.tabs[self.tab].log.is_empty() {
                    self.empty_state(ui);
                    return;
                }
                self.transcript(ui);
            });
    }

    /// La barra de pestañas: una por conversación, más el botón de abrir otra.
    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let mut activar: Option<usize> = None;
        let mut cerrar: Option<usize> = None;
        let mut abrir = false;

        row_align(ui, 30.0, egui::Align::Center, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (i, t) in self.tabs.iter().enumerate() {
                let on = i == self.tab;
                // Un punto de acento marca la pestaña donde Lucy está
                // escribiendo. Sin él, lanzar una tarea y cambiar de pestaña
                // deja al operador sin saber si sigue viva.
                let label = if t.busy() {
                    format!("● {}", t.title)
                } else {
                    format!("▭ {}", t.title)
                };
                let b = egui::Button::new(
                    egui::RichText::new(label)
                        .size(theme::FS_FOOTNOTE)
                        .color(if on { theme::ACC } else { theme::TXT3 }),
                )
                .fill(if on { theme::ACC_BG } else { theme::BG3 })
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    if on { theme::ACC_LINE } else { theme::BDR },
                ))
                .rounding(egui::Rounding::same(theme::R_SM))
                .min_size(egui::vec2(0.0, 26.0));
                let r = ui.add(b);
                if r.clicked() {
                    activar = Some(i);
                }
                // La ✕ es VISIBLE, no solo clic central. El atajo estaba y no
                // lo sabía nadie: una acción que solo existe en un gesto que no
                // se ve es una acción que no existe.
                //
                // Solo con más de una abierta: quedarse sin ninguna dejaría la
                // vista sin nada donde escribir.
                if self.tabs.len() > 1 {
                    ui.add_space(-4.0);
                    let (xr, xresp) =
                        ui.allocate_exact_size(egui::vec2(18.0, 26.0), egui::Sense::click());
                    let c = if xresp.hovered() { theme::RED } else { theme::FAINT };
                    icons::draw(ui.painter(), icons::Icon::Close, xr.center(), 11.0, c);
                    if xresp.on_hover_text("Cerrar terminal").clicked() || r.middle_clicked() {
                        cerrar = Some(i);
                    }
                }
            }
            let (pr, presp) =
                ui.allocate_exact_size(egui::vec2(28.0, 26.0), egui::Sense::click());
            ui.painter().rect(
                pr,
                egui::Rounding::same(theme::R_SM),
                if presp.hovered() { theme::BG4 } else { egui::Color32::TRANSPARENT },
                egui::Stroke::new(1.0_f32, theme::BDR),
            );
            icons::draw(
                ui.painter(),
                icons::Icon::Plus,
                pr.center(),
                15.0,
                if presp.hovered() { theme::TXT } else { theme::TXT3 },
            );
            if presp.on_hover_text("Nueva terminal").clicked() {
                abrir = true;
            }
        });

        if let Some(i) = activar {
            self.tab = i;
        }
        if let Some(i) = cerrar {
            self.tabs.remove(i);
            self.tab = self.tab.min(self.tabs.len() - 1);
        }
        if abrir {
            self.tabs.push(ChatTab::new(self.tabs_opened));
            self.tabs_opened += 1;
            self.tab = self.tabs.len() - 1;
        }
    }

    /// El selector de modelo: píldora con el icono del proveedor + desplegable
    /// con buscador.
    ///
    /// El buscador no es adorno: son 51 modelos en siete grupos, y sin él elegir
    /// uno concreto es recorrer una lista con la rueda del ratón.
    fn model_picker(&mut self, ui: &mut egui::Ui, max_w: f32) {
        let icon = lucy_core::models::icon(&self.chat_model);
        let label = lucy_core::models::describe(&self.chat_model);
        let pill = model_pill(ui, icon, label, max_w);
        let popup_id = ui.make_persistent_id("model-menu");
        if pill.clicked() {
            self.model_query.clear();
            ui.memory_mut(|m| m.toggle_popup(popup_id));
        }

        let mut elegido: Option<String> = None;
        egui::popup::popup_below_widget(
            ui,
            popup_id,
            &pill,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                let w = 330.0_f32;
                ui.set_min_width(w);
                ui.add(
                    egui::TextEdit::singleline(&mut self.model_query)
                        .hint_text("Buscar modelo…")
                        .desired_width(w),
                );
                ui.add_space(6.0);

                let grupos = lucy_core::models::filter(&self.model_query);
                egui::ScrollArea::vertical()
                    .max_height(330.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for (g, opts) in &grupos {
                            ui.add_space(4.0);
                            row_align(ui, 16.0, egui::Align::Center, |ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.add(egui::Label::new(theme::instrument_label(
                                    g.label,
                                    theme::FAINT,
                                )));
                                // Sin clave guardada, el grupo entero lo dice
                                // aquí. Descubrirlo al enviar la primera orden
                                // significa perder el turno para averiguar algo
                                // que se sabía antes de escribirlo.
                                if !with_key(g.provider) {
                                    ui.label(
                                        egui::RichText::new("sin clave")
                                            .size(theme::FS_CAPTION)
                                            .color(theme::AMBER),
                                    );
                                }
                            });
                            ui.add_space(2.0);
                            for o in opts {
                                if model_option(ui, w, o.icon, o.name, o.id == self.chat_model) {
                                    elegido = Some(o.id.to_string());
                                }
                            }
                        }
                        // Los modelos de Ollama se DESCUBREN, no están en el
                        // catálogo: lo que hay instalado depende de la máquina.
                        for m in &self.models {
                            let q = self.model_query.trim().to_lowercase();
                            if !q.is_empty() && !m.to_lowercase().contains(&q) {
                                continue;
                            }
                            if model_option(ui, w, "⌂", m, m == &self.chat_model) {
                                elegido = Some(m.clone());
                            }
                        }
                        if grupos.is_empty() && self.models.is_empty() {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("Ningún modelo coincide")
                                    .size(theme::FS_CAPTION)
                                    .color(theme::FAINT),
                            );
                        }
                    });

                // Pie: estado de Ollama y redetección, igual que la V2.
                ui.add_space(6.0);
                ui.separator();
                row_align(ui, 20.0, egui::Align::Center, |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let online = !self.models.is_empty();
                    ui.label(
                        egui::RichText::new("●")
                            .size(8.0)
                            .color(if online { theme::ACC } else { theme::FAINT }),
                    );
                    ui.label(
                        egui::RichText::new(if online {
                            format!("Ollama · {} modelos", self.models.len())
                        } else {
                            "Ollama offline".to_string()
                        })
                        .size(theme::FS_CAPTION)
                        .color(theme::TXT3),
                    );
                    right(ui, 20.0, |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("↻ redetectar")
                                        .size(theme::FS_CAPTION)
                                        .color(theme::ACC),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            self.models = lucy_core::chat::list_models();
                        }
                    });
                });
            },
        );

        if let Some(id) = elegido {
            self.chat_model = id;
            ui.memory_mut(|m| m.close_popup());
        }
    }

    /// El estado vacío: el saludo, qué hace Lucy, y cuatro tareas de un clic.
    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let mut enviar: Option<String> = None;
        ui.add_space(60.0);
        // La textura se sube la primera vez que hace falta: `new` no tiene
        // contexto todavía, y cargarla al arrancar retrasaría la ventana por
        // una imagen que quizá nadie mire si abre en el Dashboard.
        if self.face.is_none() {
            self.face = avatar::load(ui.ctx());
        }
        let face = self.face.clone();
        ui.vertical_centered(|ui| {
            match &face {
                Some(t) => avatar::show(ui, t, 84.0),
                // Sin retrato la vista sigue en pie: el glifo de siempre.
                None => {
                    ui.label(egui::RichText::new("✦").size(40.0).color(theme::ACC));
                }
            }
            ui.add_space(14.0);
            ui.label(
                egui::RichText::new(greeting(&user_name()))
                    .size(22.0)
                    .color(theme::TXT),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "Escribe una orden y Lucy la ejecuta — el plan, la salida y el trace\n\
                     se llenan en el workspace →",
                )
                .size(theme::FS_BODY)
                .color(theme::TXT3),
            );
            ui.add_space(22.0);

            // Dos filas de dos, como la V2. En una sola fila los cuatro chips
            // se estiran a lo ancho de la ventana y dejan de leerse como
            // botones.
            for par in SUGGESTIONS.chunks(2) {
                ui.horizontal(|ui| {
                    // `vertical_centered` deja el cursor a la izquierda; hay que
                    // centrar la fila a mano contra el ancho que ocupa.
                    let w: f32 = par.iter().map(|(_, l, _)| chip_w(ui, l)).sum::<f32>() + 8.0;
                    ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                    for (icon, label, order) in par {
                        if chip(ui, icon, label) {
                            enviar = Some(order.to_string());
                        }
                    }
                });
                ui.add_space(8.0);
            }
        });
        if let Some(o) = enviar {
            self.send(o);
        }
    }

    /// La conversación, con el formato del hilo de la V2.
    ///
    /// Dos formas distintas y no una con el color cambiado: la orden del
    /// operador es una BURBUJA alineada a la derecha, con su avatar y un ancho
    /// máximo del 76 % para que una orden larga no ocupe la línea entera; la
    /// respuesta de Lucy va PLANA sobre el lienzo, con su nombre y la hora
    /// encima, porque lleva markdown y una burbuja lo estrangula.
    ///
    /// El color del operador vive en su AVATAR, no en la burbuja. Teñir la
    /// burbuja —que es lo que había aquí— compite con el acento y hace que dos
    /// órdenes seguidas parezcan un bloque de color.
    fn transcript(&mut self, ui: &mut egui::Ui) {
        let busy = self.tabs[self.tab].busy();
        let n = self.tabs[self.tab].log.len();
        let full = ui.available_width();
        let me = initials(&user_name());
        let mut copiar: Option<String> = None;
        // La intención la marca la ÚLTIMA orden del operador, no la respuesta:
        // es él quien decide si quería un comando o una acción.
        let code_gen = self.tabs[self.tab]
            .log
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| lucy_core::tags::detect_code_gen_intent(&m.text))
            .unwrap_or(false);

        for i in 0..n {
            let m = &self.tabs[self.tab].log[i];
            let (text, stamp) = (m.text.clone(), m.stamp.clone());
            // Un comando ejecutado se dibuja plegado y en una línea: es un
            // evento del flujo, no algo que alguien dijo.
            if let Role::Exec(cmd, ok, out) = &m.role {
                let (cmd, ok, out) = (cmd.clone(), *ok, out.clone());
                exec_row(ui, i, &cmd, ok, &out);
                continue;
            }
            let user = m.role == Role::User;
            ui.add_space(10.0);

            // Cada mensaje entra con un fundido, como el `msg-in` del CSS. Un
            // bloque que aparece de golpe en mitad de un hilo se lee como un
            // salto; con 200 ms se lee como algo que llega.
            let t_in = if motion() {
                ease_out(ui.ctx().animate_bool_with_time(
                    egui::Id::new(("msg", self.tabs[self.tab].uid, i)),
                    true,
                    theme::DUR_BASE,
                ))
            } else {
                1.0
            };
            ui.scope(|ui| {
            ui.multiply_opacity(t_in);

            if user {
                // Fila invertida: burbuja a la izquierda del avatar, las dos
                // pegadas al borde derecho.
                let bubble_w = (full * 0.76).min(full - 44.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    right(ui, 0.0, |ui| {
                        avatar(ui, &me, theme::BLUE, theme::BLUE.linear_multiply(0.14));
                        ui.allocate_ui_with_layout(
                            egui::vec2(bubble_w, 0.0),
                            egui::Layout::top_down(egui::Align::Max),
                            |ui| {
                                ui.set_max_width(bubble_w);
                                egui::Frame::none()
                                    .fill(theme::BG3)
                                    .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
                                    .rounding(egui::Rounding::same(theme::R_LG))
                                    .inner_margin(egui::Margin::symmetric(12.0, 9.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(&text)
                                                .size(13.5)
                                                .color(theme::TXT),
                                        );
                                    });
                                if msg_actions(ui, &stamp, true) {
                                    copiar = Some(text.clone());
                                }
                            },
                        );
                    });
                });
            } else {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    // Mientras escribe, el avatar de Lucy late. Es la señal de
                    // que sigue viva cuando la respuesta tarda en arrancar.
                    let pulse = busy && i == n - 1;
                    let bg = if pulse {
                        let t = ui.ctx().animate_bool_with_time(
                            egui::Id::new(("pulse", i)),
                            (ui.input(|x| x.time) * 1.6) as i64 % 2 == 0,
                            0.5,
                        );
                        theme::ACC.linear_multiply(0.10 + 0.14 * t)
                    } else {
                        theme::ACC_BG
                    };
                    avatar(ui, "✦", theme::ACC, bg);
                    ui.vertical(|ui| {
                        row_align(ui, 18.0, egui::Align::Max, |ui| {
                            ui.spacing_mut().item_spacing.x = 7.0;
                            ui.label(
                                egui::RichText::new("Lucy")
                                    .size(theme::FS_FOOTNOTE)
                                    .color(theme::TXT),
                            );
                            ui.label(
                                egui::RichText::new(&stamp)
                                    .size(theme::FS_MICRO)
                                    .monospace()
                                    .color(theme::FAINT),
                            );
                        });
                        ui.add_space(5.0);
                        // El marcado de acción NO llega al hilo: sin esto, el
                        // operador ve `<TOOL>readfile:…</TOOL>` crudo en mitad
                        // de la respuesta mientras Lucy trabaja.
                        // Si el operador PIDIÓ el comando, se enseña como
                        // bloque de código; si pidió una acción, se esconde y
                        // va al panel. La intención sale de su último mensaje.
                        let shown = lucy_core::tags::clean_display_with(&text, code_gen);
                        if shown.text.is_empty() && pulse {
                            ui.label(
                                egui::RichText::new("Pensando…")
                                    .size(theme::FS_FOOTNOTE)
                                    .color(theme::FAINT),
                            );
                        } else {
                            CommonMarkViewer::new().show(ui, &mut self.md_cache, &shown.text);
                        }
                        // A DÓNDE FUE EL COMANDO. Quitarlo en silencio deja la
                        // prosa colgando: Lucy escribe "usa esta sintaxis:" y
                        // debajo no hay nada, y la respuesta parece cortada a
                        // media frase. Es lo que se veía. Con esta línea, el
                        // texto apunta a un sitio que existe.
                        if !code_gen && shown.commands > 0 {
                            ui.add_space(6.0);
                            egui::Frame::none()
                                .fill(theme::ACC_BG)
                                .rounding(egui::Rounding::same(theme::R_SM))
                                .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.x = 7.0;
                                    icons::show(ui, icons::Icon::Terminal, 13.0, theme::ACC);
                                    ui.label(
                                        egui::RichText::new(if shown.commands == 1 {
                                            "1 comando propuesto — apruébalo en el panel de Plan"
                                                .to_string()
                                        } else {
                                            format!(
                                                "{} comandos propuestos — apruébalos en el panel \
                                                 de Plan",
                                                shown.commands
                                            )
                                        })
                                        .size(theme::FS_CAPTION)
                                        .color(theme::ACC),
                                    );
                                });
                        }
                        // El razonamiento se GUARDA y se enseña plegado, nunca
                        // se borra: es la única explicación de por qué Lucy hizo
                        // lo que hizo. La V2 usa un `<details>` de HTML; aquí,
                        // el desplegable nativo.
                        for (k, th) in shown.thoughts.iter().enumerate() {
                            egui::CollapsingHeader::new(
                                egui::RichText::new("Razonando…")
                                    .size(theme::FS_CAPTION)
                                    .color(theme::FAINT),
                            )
                            .id_salt(("thought", i, k))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(th)
                                        .size(theme::FS_CAPTION)
                                        .color(theme::TXT3),
                                );
                            });
                        }
                        if pulse && !text.is_empty() {
                            ui.label(egui::RichText::new("▋").color(theme::ACC));
                        }
                        if !pulse && msg_actions(ui, "", false) {
                            copiar = Some(text.clone());
                        }
                    });
                });
            }
            });
        }
        if let Some(t) = copiar {
            ui.ctx().copy_text(t);
        }
    }

    /// El compositor: adjuntar, dictar, escribir, enviar.
    fn composer(&mut self, ui: &mut egui::Ui) {
        let busy = self.tabs[self.tab].busy();
        let mut enviar = false;
        let mut abrir_dialogo = false;
        let mut quitar: Option<usize> = None;

        egui::Frame::none()
            .fill(theme::BG3)
            .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
            .rounding(egui::Rounding::same(theme::R_LG))
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                // Los adjuntos van ENCIMA del campo, como en el CSS: son
                // contexto de lo que se está a punto de escribir, no un
                // resultado de haberlo escrito.
                if !self.tabs[self.tab].attachments.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                        for (i, a) in self.tabs[self.tab].attachments.iter().enumerate() {
                            if attach_chip(ui, a) {
                                quitar = Some(i);
                            }
                        }
                    });
                    ui.add_space(8.0);
                }
                row_align(ui, 26.0, egui::Align::Center, |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    if ghost_icon(ui, icons::Icon::Clip)
                        .on_hover_text("Adjuntar fichero — o arrastra uno a la ventana")
                        .clicked()
                    {
                        abrir_dialogo = true;
                    }
                    // El dictado sigue sin hacer nada, y lo dice. Un botón que
                    // ignora los clics en silencio es peor que uno ausente, y
                    // este además nombra el obstáculo real.
                    if ghost_icon(ui, icons::Icon::Mic)
                        .on_hover_text(
                            "Dictado por voz — pendiente: la V2 usa la API del navegador y \
                             un shell nativo necesita otro motor",
                        )
                        .clicked()
                    {}

                    let mut send_w = 34.0;
                    send_w += 8.0;
                    let field_w = (ui.available_width() - send_w).max(80.0);
                    let resp = ui.add_enabled(
                        !busy,
                        egui::TextEdit::singleline(&mut self.tabs[self.tab].input)
                            .hint_text("Escribe una orden…   ·   Shift+Enter = salto de línea")
                            .desired_width(field_w)
                            .frame(false)
                            .font(egui::FontId::proportional(theme::FS_BODY)),
                    );
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        enviar = true;
                    }

                    right(ui, 26.0, |ui| {
                        // Redondo y relleno de acento: es la ÚNICA acción primaria
                        // de la vista, y el CSS le da el único acento sólido.
                        let (sr, sresp) = ui.allocate_exact_size(
                            egui::vec2(30.0, 30.0),
                            if busy { egui::Sense::hover() } else { egui::Sense::click() },
                        );
                        let fill = if busy {
                            theme::ACC.linear_multiply(0.4)
                        } else if sresp.hovered() {
                            theme::ACC_HOVER
                        } else {
                            theme::ACC
                        };
                        ui.painter().circle_filled(sr.center(), 15.0, fill);
                        icons::draw(
                            ui.painter(),
                            icons::Icon::ArrowUp,
                            sr.center(),
                            17.0,
                            theme::ACC_INK,
                        );
                        if sresp.clicked() {
                            enviar = true;
                        }
                    });
                });
            });

        // La paleta se dibuja DESPUÉS del compositor y encima: tiene que quedar
        // por delante del hilo, y en modo inmediato lo último que se pinta es lo
        // que está arriba.
        self.slash_palette(ui);

        if let Some(i) = quitar {
            self.tabs[self.tab].attachments.remove(i);
        }
        if abrir_dialogo {
            // Bloqueante a propósito: es el diálogo modal del sistema, el mismo
            // que abre cualquier aplicación de Windows, y mientras está abierto
            // no hay nada que animar detrás.
            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                self.attach(&paths);
            }
        }
        if enviar && !busy {
            let text = std::mem::take(&mut self.tabs[self.tab].input);
            // Se permite enviar SOLO con adjuntos: arrastrar un log y pulsar
            // enviar es una petición perfectamente clara.
            if !text.trim().is_empty() || !self.tabs[self.tab].attachments.is_empty() {
                self.send(text);
            }
        }
    }

    /// Minimizar, maximizar y cerrar, a la derecha de la barra de título.
    ///
    /// En el orden de Windows —cerrar el ÚLTIMO, pegado a la esquina— porque el
    /// músculo del operador ya está entrenado ahí, y el botón que no se puede
    /// deshacer es el peor sitio para innovar. Como la fila se dibuja de derecha
    /// a izquierda, se piden en ese mismo orden.
    fn window_buttons(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));

        for (icon, tip, danger) in [
            (icons::Icon::Close, "Cerrar", true),
            (
                if maximized { icons::Icon::Restore } else { icons::Icon::Maximize },
                if maximized { "Restaurar" } else { "Maximizar" },
                false,
            ),
            (icons::Icon::Minimize, "Minimizar", false),
        ] {
            let (r, resp) =
                ui.allocate_exact_size(egui::vec2(34.0, 30.0), egui::Sense::click());
            if resp.hovered() {
                // El de cerrar se tiñe de rojo al pasar por encima, como toda
                // ventana de Windows. Es la única señal antes de un clic que no
                // se puede deshacer.
                ui.painter().rect_filled(
                    r,
                    egui::Rounding::same(6.0),
                    if danger { theme::RED.linear_multiply(0.85) } else { theme::BG4 },
                );
            }
            let fg = if resp.hovered() && danger {
                theme::TXT
            } else if resp.hovered() {
                theme::TXT
            } else {
                theme::TXT3
            };
            icons::draw(ui.painter(), icon, r.center(), 15.0, fg);
            if resp.on_hover_text(tip).clicked() {
                ctx.send_viewport_cmd(match tip {
                    "Cerrar" => egui::ViewportCommand::Close,
                    "Minimizar" => egui::ViewportCommand::Minimized(true),
                    _ => egui::ViewportCommand::Maximized(!maximized),
                });
            }
        }
    }

    /// La paleta de comandos: aparece al escribir `/` y filtra según se teclea.
    ///
    /// Va ANCLADA sobre el compositor y no en un desplegable del sistema porque
    /// tiene que moverse con él: el compositor está pegado abajo, y una lista
    /// que apareciera en otro sitio obligaría a mirar a dos lados a la vez.
    fn slash_palette(&mut self, ui: &mut egui::Ui) {
        let draft = self.tabs[self.tab].input.clone();
        if !draft.starts_with('/') {
            return;
        }
        let q = draft.to_lowercase();
        let hits: Vec<&(&str, &str, bool)> = SLASH
            .iter()
            .filter(|(c, d, _)| c.starts_with(&q) || d.to_lowercase().contains(q.trim_start_matches('/')))
            .collect();
        if hits.is_empty() {
            return;
        }

        let composer = ui.min_rect();
        let w = composer.width().min(620.0);
        let row_h = 26.0;
        let shown = hits.len().min(9);
        let h = shown as f32 * row_h + 16.0;
        let rect = egui::Rect::from_min_size(
            egui::pos2(composer.left(), composer.top() - h - 8.0),
            egui::vec2(w, h),
        );

        let mut elegido: Option<&str> = None;
        // Capa propia: si se dibujara en el flujo, el hilo de la conversación
        // —que se pinta antes— se quedaría por encima.
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("slash"),
        ));
        painter.rect(
            rect,
            egui::Rounding::same(theme::R_LG),
            theme::BG3,
            egui::Stroke::new(1.0_f32, theme::BDR2),
        );

        let mut y = rect.top() + 8.0;
        for (cmd, desc, ready) in hits.iter().take(shown) {
            let r = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 6.0, y),
                egui::vec2(w - 12.0, row_h),
            );
            let hover = ui.rect_contains_pointer(r);
            if hover {
                painter.rect_filled(r, egui::Rounding::same(theme::R_SM), theme::BG4);
                if ui.input(|i| i.pointer.primary_clicked()) {
                    elegido = Some(cmd);
                }
            }
            painter.text(
                egui::pos2(r.left() + 10.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                cmd,
                egui::FontId::monospace(theme::FS_FOOTNOTE),
                theme::ACC,
            );
            painter.text(
                egui::pos2(r.left() + 130.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                desc,
                egui::FontId::proportional(theme::FS_CAPTION),
                if *ready { theme::TXT2 } else { theme::FAINT },
            );
            // Los que todavía no hacen nada se marcan aquí, no al pulsarlos:
            // enterarse después de elegir es perder el movimiento.
            if !*ready {
                painter.text(
                    egui::pos2(r.right() - 10.0, r.center().y),
                    egui::Align2::RIGHT_CENTER,
                    "sin migrar",
                    egui::FontId::proportional(theme::FS_MICRO),
                    theme::FAINT,
                );
            }
            y += row_h;
        }
        if hits.len() > shown {
            painter.text(
                egui::pos2(rect.center().x, rect.bottom() - 4.0),
                egui::Align2::CENTER_CENTER,
                format!("+{} más", hits.len() - shown),
                egui::FontId::proportional(theme::FS_MICRO),
                theme::FAINT,
            );
        }

        // Tab completa con el primero, como en cualquier terminal.
        if ui.input(|i| i.key_pressed(egui::Key::Tab)) {
            elegido = Some(hits[0].0);
        }
        if let Some(c) = elegido {
            self.tabs[self.tab].input = format!("{c} ");
        }
    }

    /// Añade ficheros a la pestaña activa, sin repetir los que ya están.
    fn attach(&mut self, paths: &[std::path::PathBuf]) {
        for p in paths {
            let a = Attachment::read(p);
            let t = &mut self.tabs[self.tab];
            if t.attachments.iter().any(|x| x.name == a.name) {
                continue;
            }
            t.attachments.push(a);
        }
    }

    /// Manda una orden por la pestaña activa.
    fn send(&mut self, text: String) {
        if self.tabs[self.tab].busy() {
            return;
        }
        // Los adjuntos de TEXTO se anteponen a la orden, que es lo que hace el
        // constructor de prompts de la V2 con `type === 'text'`. Imágenes y PDF
        // no: la primera necesita la ruta de visión y el segundo el extractor
        // del backend, y meterlos vacíos le daría al modelo un fichero que
        // parece estar y no está.
        let mut prompt = String::new();
        let mut adjuntos = Vec::new();
        for a in &self.tabs[self.tab].attachments {
            adjuntos.push((a.name.clone(), a.blocked.clone()));
            if a.blocked.is_empty() {
                prompt.push_str(&format!(
                    "--- fichero adjunto: {} ---\n{}\n\n",
                    a.name, a.content
                ));
            }
        }
        // El prompt de sistema va DELANTE en cada turno: quién es Lucy y en qué
        // equipo está. Sin él, un modelo de nube contesta lo único que puede —
        // "no tengo acceso a tu computadora"— y tiene razón.
        let sys_prompt = prompt::system_prompt(
            &self.sys.snapshot(),
            &self.services,
            self.log_lines.as_deref().unwrap_or(&[]),
        );
        prompt.push_str(&text);
        let prompt = format!("{sys_prompt}
--- ORDEN DEL OPERADOR ---
{prompt}");

        {
            let t = &mut self.tabs[self.tab];
            // Lo que quede por revelar se vuelca YA en el mensaje al que
            // pertenece. La cola escribe siempre en el último mensaje del hilo,
            // así que empezar un turno nuevo con texto pendiente lo pegaría a
            // la respuesta siguiente — mezclando dos respuestas en una.
            let resto = t.drain.flush();
            if !resto.is_empty() {
                if let Some(last) = t.log.last_mut() {
                    last.text.push_str(&resto);
                }
            }
            // El título de la pestaña pasa a ser la primera orden: con tres
            // terminales abiertas, "Terminal 2" no dice cuál era cuál.
            if t.log.is_empty() {
                let base = if text.trim().is_empty() {
                    adjuntos.first().map(|(n, _)| n.clone()).unwrap_or_default()
                } else {
                    text.clone()
                };
                t.title = base.chars().take(28).collect::<String>().trim().to_string();
            }
            // En la conversación se ve la orden del operador, no el prompt con
            // los ficheros pegados: eso es fontanería, no lo que él escribió.
            let mut shown = text.clone();
            for (n, _) in &adjuntos {
                shown.push_str(&format!("\n⎘ {n}"));
            }
            t.log.push(ChatMsg::new(true, shown));
            t.log.push(ChatMsg::new(false, String::new()));
            t.attachments.clear();
            t.rx = Some(lucy_core::cloud::start(self.chat_model.clone(), prompt));
        }

        for (n, blocked) in &adjuntos {
            self.tabs[self.tab].ws.trace_push(lucy_core::agent::TraceEntry {
                phase: "info".into(),
                label: format!("Adjunto · {n}"),
                detail: if blocked.is_empty() {
                    "incluido en el prompt".into()
                } else {
                    blocked.clone()
                },
                ..Default::default()
            });
        }

        // El workspace registra lo que DE VERDAD pasa en este shell. Sin bucle
        // de agente no hay plan que desglosar ni comandos que ejecutar, así que
        // esos carriles se quedan vacíos y lo dicen; el trace sí tiene algo
        // cierto que contar —el ciclo de vida del turno— y va como `info`, que
        // es una de las fases del propio modelo. Inventar pasos de un plan que
        // nadie planificó sería teatro.
        self.tabs[self.tab].ws.status.running = true;
        self.tabs[self.tab].ws.status.model = self.chat_model.clone();
        self.tabs[self.tab].turn_start = Some(Instant::now());
        self.tabs[self.tab].ws.trace_push(lucy_core::agent::TraceEntry {
            phase: "info".into(),
            label: "Orden enviada".into(),
            detail: format!(
                "{} · {} · {} caracteres",
                lucy_core::cloud::provider_of(&self.chat_model).label(),
                lucy_core::models::describe(&self.chat_model),
                text.chars().count()
            ),
            ..Default::default()
        });
    }

    /// Vuelca lo que Lucy PIDIÓ en los carriles del workspace.
    ///
    /// Cada etiqueta va al carril que le corresponde: el razonamiento y las
    /// herramientas al Trace, las ejecuciones al Plan como pasos PENDIENTES, y
    /// los ficheros a Artefactos. Es la primera vez que esos tres paneles se
    /// llenan con algo real.
    ///
    /// PENDIENTES, no hechos, y ahí está todo: este shell detecta lo que Lucy
    /// quiere hacer y no lo hace. Marcarlos de otra forma diría que la máquina
    /// se tocó, que es la peor mentira que puede contar un panel de auditoría.
    fn absorb_tags(&mut self, uid: usize, reply: &str) {
        let Some(ti) = self.tabs.iter().position(|t| t.uid == uid) else { return };
        use lucy_core::agent::{PlanStep, StepStatus, TraceEntry};
        use lucy_core::tags::{self, TagKind};

        for t in tags::extract_tags(reply) {
            match t.kind {
                TagKind::Thought => self.tabs[ti].ws.trace_push(TraceEntry {
                    phase: "think".into(),
                    label: "Razonamiento".into(),
                    detail: t.content,
                    ..Default::default()
                }),
                TagKind::Tool => {
                    let (name, args) = tags::parse_tool(&t.content);
                    self.tabs[ti].ws.trace_push(TraceEntry {
                        phase: "act".into(),
                        label: name.clone(),
                        detail: args.clone(),
                        ..Default::default()
                    });
                    // `writefile` y `editfile` tocan ficheros: eso es un
                    // artefacto, aunque todavía no se haya escrito ninguno.
                    if matches!(name.as_str(), "writefile" | "editfile") {
                        self.tabs[ti].ws.artifact_push(lucy_core::agent::Artifact {
                            id: String::new(),
                            kind: if name == "editfile" {
                                lucy_core::agent::ArtifactKind::Edit
                            } else {
                                lucy_core::agent::ArtifactKind::Write
                            },
                            path: args.split("|||").next().unwrap_or(&args).to_string(),
                            summary: "propuesto — sin escribir".into(),
                            before: String::new(),
                            after: String::new(),
                            ts: 0,
                        });
                    }
                }
                k if k.is_execute() => {
                    let host = t.attrs.get("target").cloned().unwrap_or_default();
                    self.tabs[ti].ws.plan_append(PlanStep {
                        label: format!("Ejecutar ({})", k.name()),
                        status: StepStatus::Pending,
                        detail: t.content,
                        host,
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
    }

    /// Manda un turno cuyo texto NO lo escribió el operador.
    ///
    /// En el hilo se ve una línea corta —"resultado devuelto"— y no el volcado
    /// entero: la salida ya está en el panel de Ejecución, y repetirla en la
    /// conversación empujaría fuera de pantalla la pregunta original. Al modelo
    /// sí le va completa.
    fn send_raw(&mut self, ti: usize, prompt: String) {
        if self.tabs[ti].busy() {
            return;
        }
        let sys = prompt::system_prompt(
            &self.sys.snapshot(),
            &self.services,
            self.log_lines.as_deref().unwrap_or(&[]),
        );
        let t = &mut self.tabs[ti];
        let resto = t.drain.flush();
        if !resto.is_empty() {
            if let Some(last) = t.log.last_mut() {
                last.text.push_str(&resto);
            }
        }
        // La línea del comando ya se añadió en `pump_exec`: aquí solo se abre el
        // hueco de la respuesta.
        t.log.push(ChatMsg::new(false, String::new()));
        t.rx = Some(lucy_core::cloud::start(
            self.chat_model.clone(),
            format!("{sys}\n--- ORDEN DEL OPERADOR ---\n{prompt}"),
        ));
        self.tabs[ti].ws.status.running = true;
        self.tabs[ti].turn_start = Some(Instant::now());
    }

    /// Corre un paso que el operador acaba de aprobar.
    ///
    /// EN OTRO HILO, siempre. PowerShell tarda cientos de milisegundos como
    /// mínimo y un `Get-Service` sobre una máquina cargada puede irse a varios
    /// segundos: hacerlo en el hilo de interfaz congelaría la ventana justo
    /// mientras el operador mira si su comando funcionó. Ya cometí ese error una
    /// vez con la sonda de servicios.
    #[cfg(windows)]
    fn run_step(&mut self, id: String, cmd: String, elevated: bool) {
        use lucy_core::agent::StepStatus;
        self.tabs[self.tab].ws.plan_update(&id, StepStatus::Running, None);
        self.ws_tab = WsTab::Exec; // se mira la salida, no el plan
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let t0 = Instant::now();
            // Elevado va por otro camino entero: proceso nuevo, UAC, y la
            // salida por fichero. Ver `lucy_core::elevate`.
            let r = if elevated {
                lucy_core::elevate::run_elevated(&cmd).map(|(o, ok)| (o, String::new(), ok))
            } else {
                lucy_core::shell::run_powershell_utf8(&cmd)
            };
            let ms = t0.elapsed().as_millis() as u64;
            let _ = tx.send(match r {
                Ok((out, err, ok)) => (out, err, ok, ms),
                // Que PowerShell no arranque también es un resultado, y el
                // operador tiene que verlo: si no, el paso se queda "en curso"
                // para siempre sin explicación.
                Err(e) => (String::new(), e, false, ms),
            });
        });
        self.exec_rx = Some((self.tabs[self.tab].uid, id, rx));
    }

    #[cfg(not(windows))]
    fn run_step(&mut self, _id: String, _cmd: String, _elevated: bool) {}

    /// Recoge el resultado de un comando aprobado.
    fn pump_exec(&mut self) {
        use lucy_core::agent::{ExecEntry, StepStatus};
        // El resultado vuelve a LA PESTAÑA que lo pidió, no a la que esté
        // delante. Con dos órdenes en marcha, mandarlo a la activa imprimía la
        // salida de una en la conversación de la otra — que es lo que pasaba.
        let Some((uid, id, rx)) = &self.exec_rx else { return };
        let (uid, id) = (*uid, id.clone());
        let (out, err, ok, ms) = match rx.try_recv() {
            Ok(v) => v,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if let Some(t) = self.tabs.iter_mut().find(|t| t.uid == uid) {
                    t.ws.plan_update(&id, StepStatus::Error, None);
                }
                self.exec_rx = None;
                return;
            }
        };
        // La pestaña puede haberse cerrado mientras el comando corría. No es un
        // error: se ejecutó igual, y no hay a quién contárselo.
        let Some(ti) = self.tabs.iter().position(|t| t.uid == uid) else {
            self.exec_rx = None;
            return;
        };
        let cmd = self.tabs[ti]
            .ws
            .plan
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.detail.clone())
            .unwrap_or_default();

        // La salida y el error van JUNTOS. PowerShell escribe avisos por stderr
        // en comandos que funcionan, y separarlos hace que un comando correcto
        // parezca fallido y uno fallido parezca vacío.
        let mut body = out.trim().to_string();
        if !err.trim().is_empty() {
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            body.push_str(&format!("[stderr]\n{}", err.trim()));
        }
        if body.is_empty() {
            body = "(sin salida)".into();
        }

        self.tabs[ti].ws.exec_push(ExecEntry {
            id: String::new(),
            cmd: cmd.clone(),
            output: body.clone(),
            ok,
            ms: Some(ms),
            engine: "PS".into(),
            code: None,
            ts: 0,
        });
        self.tabs[ti].ws.plan_update(
            &id,
            if ok { StepStatus::Done } else { StepStatus::Error },
            Some(ms),
        );
        self.exec_rx = None;

        // La línea del comando entra en el hilo como EVENTO, plegada: el
        // comando, si fue bien, y su salida dentro.
        self.tabs[ti].log.push(ChatMsg::exec(cmd.clone(), ok, body.clone()));

        // Y VUELVE A LUCY. Sin esto, el operador tiene la salida cruda en un
        // panel y sigue sin la respuesta que pidió — que es exactamente lo que
        // pasaba: el comando se proponía, se quedaba ahí, y nadie cerraba el
        // círculo. La aprobación fue el clic; devolver el resultado es la otra
        // mitad de ese mismo gesto.
        self.send_raw(ti, format!(
            "He ejecutado el comando que propusiste y esta es su salida literal. \
             Resúmela y dime qué significa. No propongas ejecutarlo otra vez.\n\n\
             $ {cmd}\n\n{body}"
        ));
    }

    /// Cierra el turno en el workspace cuando el stream termina.
    fn turn_finished(&mut self, uid: usize, chars: usize) {
        let Some(ti) = self.tabs.iter().position(|t| t.uid == uid) else { return };
        let ms = self.tabs[ti].turn_start.take().map(|t| t.elapsed().as_millis() as u64);
        self.tabs[ti].ws.status.running = false;
        self.tabs[ti].ws.trace_push(lucy_core::agent::TraceEntry {
            phase: "info".into(),
            label: "Respuesta completa".into(),
            detail: match ms {
                Some(ms) => format!("{chars} caracteres en {}", fmt_ms(ms)),
                None => format!("{chars} caracteres"),
            },
            ..Default::default()
        });
    }

    /// El panel derecho: los cuatro carriles del agente.
    fn workspace(&mut self, ui: &mut egui::Ui) {
        let counts = [
            self.tabs[self.tab].ws.plan.len(),
            self.tabs[self.tab].ws.exec.len(),
            self.tabs[self.tab].ws.trace.len(),
            self.tabs[self.tab].ws.artifacts.len(),
        ];
        let forks = self.tabs[self.tab].ws.forks_running();

        // ── pestañas ─────────────────────────────────────────────────────────
        row_align(ui, 30.0, egui::Align::Center, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, t) in WsTab::ALL.iter().enumerate() {
                let on = self.ws_tab == *t;
                let mut label = t.label().to_string();
                if counts[i] > 0 {
                    label.push_str(&format!("  {}", counts[i]));
                }
                // Los sub-agentes viven en el panel de Plan, así que su pestaña
                // lleva la señal: un fork corriendo se ve desde cualquier otra.
                if *t == WsTab::Plan && forks > 0 {
                    label.push_str(&format!("  ⇉{forks}"));
                }
                let b = egui::Button::new(
                    egui::RichText::new(label)
                        .size(theme::FS_FOOTNOTE)
                        .color(if on { theme::ACC } else { theme::TXT3 }),
                )
                .fill(egui::Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE)
                .min_size(egui::vec2(0.0, 26.0));
                let r = ui.add(b);
                if on {
                    // Subrayado de acento, como el CSS: la pestaña activa se
                    // marca por debajo y no con un relleno que compita con el
                    // contenido del panel.
                    let x = r.rect;
                    ui.painter().hline(
                        x.left()..=x.right(),
                        x.bottom() - 1.0,
                        egui::Stroke::new(2.0_f32, theme::ACC),
                    );
                }
                if r.clicked() {
                    self.ws_tab = *t;
                }
            }
        });
        ui.add_space(4.0);
        ui.separator();

        // ── herramientas: solo cuando hay algo que exportar o limpiar ─────────
        if !self.tabs[self.tab].ws.is_empty() {
            row_align(ui, 22.0, egui::Align::Center, |ui| {
                right(ui, 22.0, |ui| {
                    if ghost_icon(ui, icons::Icon::Close).on_hover_text("Limpiar el workspace").clicked() {
                        self.tabs[self.tab].ws.reset();
                    }
                    if ghost_icon(ui, icons::Icon::Copy)
                        .on_hover_text("Exportar el run (copia al portapapeles)")
                        .clicked()
                    {
                        let r = self.export_run();
                        ui.ctx().copy_text(r);
                    }
                });
            });
        }
        ui.add_space(4.0);

        let tab = self.ws_tab;
        let empty = match tab {
            WsTab::Plan => self.tabs[self.tab].ws.plan.is_empty() && self.tabs[self.tab].ws.forks.is_empty(),
            WsTab::Exec => self.tabs[self.tab].ws.exec.is_empty(),
            WsTab::Trace => self.tabs[self.tab].ws.trace.is_empty(),
            WsTab::Artifacts => self.tabs[self.tab].ws.artifacts.is_empty(),
        };

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if empty {
                    ws_empty(ui, tab);
                    return;
                }
                match tab {
                    WsTab::Plan => self.ws_plan(ui),
                    WsTab::Exec => self.ws_exec(ui),
                    WsTab::Trace => self.ws_trace(ui),
                    WsTab::Artifacts => self.ws_artifacts(ui),
                }
            });
    }

    fn ws_plan(&mut self, ui: &mut egui::Ui) {
        use lucy_core::agent::{ForkStatus, StepStatus};
        use lucy_core::elevate::Elevation;
        // Se consulta una vez por sesión y se cachea dentro.
        let elev = lucy_core::elevate::state();
        let busy = self.exec_rx.is_some();
        // Los comandos cuya última ejecución murió por permisos. Se mira la
        // SALIDA real y no se adivina por el texto del comando: `Start-Service`
        // funciona sin elevar en muchos servicios y falla en otros.
        let denegado: Vec<String> = self.tabs[self.tab]
            .ws
            .exec
            .iter()
            .filter(|e| !e.ok && lucy_core::elevate::looks_like_access_denied(&e.output))
            .map(|e| e.cmd.clone())
            .collect();
        // `(id, comando, elevado)`.
        let mut aprobado: Option<(String, String, bool)> = None;

        for s in &self.tabs[self.tab].ws.plan {
            let (glyph, col) = match s.status {
                StepStatus::Done => ("✓", theme::ACC),
                StepStatus::Running => ("▸", theme::ACC),
                StepStatus::Error => ("✕", theme::RED),
                StepStatus::Pending => ("○", theme::DISABLED),
            };
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(egui::RichText::new(glyph).size(12.0).color(col));
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(&s.label)
                            .size(theme::FS_FOOTNOTE)
                            .color(if s.status == StepStatus::Pending {
                                theme::TXT3
                            } else {
                                theme::TXT
                            }),
                    );
                    if !s.detail.is_empty() {
                        // El comando ENTERO y en monoespaciada. Es lo que se va
                        // a correr en la máquina, y aprobar algo recortado con
                        // puntos suspensivos no es aprobar nada.
                        ui.label(
                            egui::RichText::new(&s.detail)
                                .size(theme::FS_CAPTION)
                                .monospace()
                                .color(theme::TXT2),
                        );
                    }
                    // El botón SOLO existe en los pasos pendientes, y nada corre
                    // sin que alguien lo pulse: en este shell no hay ejecución
                    // automática. El operador lee el comando y decide — que es
                    // el único guardrail que no hace falta portar.
                    // Tras un fallo por permisos se OFRECE la elevación, no antes.
                    // Un UAC que salta sin saber si hace falta enseña a
                    // aceptarlo sin leerlo, y ese hábito es peor que el comando
                    // que se quería correr.
                    if s.status == StepStatus::Error && denegado.contains(&s.detail) {
                        ui.add_space(4.0);
                        match elev {
                            // El único caso en que el botón puede cumplir.
                            Elevation::CanPrompt => {
                                let b = egui::Button::new(
                                    egui::RichText::new("⇈ Reintentar como administrador")
                                        .size(theme::FS_CAPTION)
                                        .color(theme::AMBER),
                                )
                                .fill(theme::AMBER_BG)
                                .stroke(egui::Stroke::new(
                                    1.0_f32,
                                    theme::AMBER.linear_multiply(0.4),
                                ))
                                .rounding(egui::Rounding::same(6.0))
                                .min_size(egui::vec2(0.0, 22.0));
                                if ui
                                    .add_enabled(!busy, b)
                                    .on_hover_text("Windows pedirá confirmación (UAC)")
                                    .clicked()
                                {
                                    aprobado = Some((s.id.clone(), s.detail.clone(), true));
                                }
                            }
                            // Lucy YA manda en esta máquina. Ofrecer elevación
                            // sería una promesa falsa: el reintento fallaría
                            // igual, porque lo que falló no fueron los
                            // permisos. `gpsvc` es el ejemplo — Windows
                            // restringe su control manual hasta al
                            // administrador.
                            Elevation::Already => {
                                ui.label(
                                    egui::RichText::new(
                                        "Lucy ya corre como administrador: esto no es \n                                         un problema de privilegios.",
                                    )
                                    .size(theme::FS_CAPTION)
                                    .color(theme::FAINT),
                                );
                            }
                            // Cuenta estándar y consentimiento apagado: no hay
                            // mecanismo que pedir. Decirlo es más útil que un
                            // botón que no puede funcionar.
                            Elevation::Unavailable => {
                                ui.label(
                                    egui::RichText::new(
                                        "Sin privilegios y con UAC desactivado: hay que \n                                         abrir Lucy con una cuenta de administrador.",
                                    )
                                    .size(theme::FS_CAPTION)
                                    .color(theme::AMBER),
                                );
                            }
                        }
                    }
                    if s.status == StepStatus::Pending && !s.detail.is_empty() {
                        ui.add_space(4.0);
                        let b = egui::Button::new(
                            egui::RichText::new("▸ Ejecutar")
                                .size(theme::FS_CAPTION)
                                .color(theme::ACC_INK),
                        )
                        .fill(theme::ACC)
                        .stroke(egui::Stroke::NONE)
                        .rounding(egui::Rounding::same(6.0))
                        .min_size(egui::vec2(0.0, 22.0));
                        if ui
                            .add_enabled(!busy, b)
                            .on_hover_text("Correr este comando en este equipo")
                            .clicked()
                        {
                            aprobado = Some((s.id.clone(), s.detail.clone(), false));
                        }
                    }
                });
            });
        }
        if let Some((id, cmd, elev)) = aprobado {
            self.run_step(id, cmd, elev);
        }
        // Los forks van DESPUÉS del plan y fuera de su estado vacío: con un
        // sub-agente corriendo el panel no está vacío, solo no tiene plan.
        if !self.tabs[self.tab].ws.forks.is_empty() {
            ui.add_space(10.0);
            ui.add(egui::Label::new(theme::instrument_label(
                "Sub-agentes",
                theme::FAINT,
            )));
            for f in &self.tabs[self.tab].ws.forks {
                let (txt, col) = match f.status {
                    ForkStatus::Running => ("en curso", theme::ACC),
                    ForkStatus::Done => ("terminado", theme::TXT3),
                    ForkStatus::Error => ("error", theme::RED),
                    ForkStatus::Collected => ("recogido", theme::FAINT),
                };
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("⇉").size(11.0).color(col));
                    ui.label(
                        egui::RichText::new(&f.id)
                            .size(theme::FS_CAPTION)
                            .monospace()
                            .color(theme::TXT2),
                    );
                    ui.label(egui::RichText::new(txt).size(theme::FS_CAPTION).color(col));
                });
            }
        }
    }

    fn ws_exec(&mut self, ui: &mut egui::Ui) {
        for e in &self.tabs[self.tab].ws.exec {
            ui.add_space(6.0);
            egui::Frame::none()
                .fill(theme::BG3)
                .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
                .rounding(egui::Rounding::same(theme::R_SM))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(if e.ok { "✓" } else { "✕" })
                                .size(11.0)
                                .color(if e.ok { theme::ACC } else { theme::RED }),
                        );
                        ui.label(
                            egui::RichText::new(&e.cmd)
                                .size(theme::FS_CAPTION)
                                .monospace()
                                .color(theme::TXT),
                        );
                    });
                    if !e.output.is_empty() {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(&e.output)
                                .size(theme::FS_CAPTION)
                                .monospace()
                                .color(theme::TXT3),
                        );
                    }
                });
        }
    }

    fn ws_trace(&mut self, ui: &mut egui::Ui) {
        for t in &self.tabs[self.tab].ws.trace {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // La fase va en su propio chip: en una lista larga es por lo
                // que se busca, no por la etiqueta.
                egui::Frame::none()
                    .fill(theme::BG4)
                    .rounding(egui::Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(6.0, 1.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(&t.phase)
                                .size(theme::FS_CAPTION)
                                .monospace()
                                .color(theme::TXT3),
                        );
                    });
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(&t.label)
                            .size(theme::FS_CAPTION)
                            .color(theme::TXT2),
                    );
                    if !t.detail.is_empty() {
                        ui.label(
                            egui::RichText::new(&t.detail)
                                .size(theme::FS_CAPTION)
                                .color(theme::FAINT),
                        );
                    }
                });
            });
        }
    }

    fn ws_artifacts(&mut self, ui: &mut egui::Ui) {
        for a in &self.tabs[self.tab].ws.artifacts {
            ui.add_space(6.0);
            egui::Frame::none()
                .fill(theme::BG3)
                .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
                .rounding(egui::Rounding::same(theme::R_SM))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(a.kind.label())
                                .size(theme::FS_CAPTION)
                                .color(theme::ACC),
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&a.path)
                                    .size(theme::FS_CAPTION)
                                    .monospace()
                                    .color(theme::TXT),
                            )
                            .truncate(),
                        );
                    });
                    if !a.summary.is_empty() {
                        ui.label(
                            egui::RichText::new(&a.summary)
                                .size(theme::FS_CAPTION)
                                .color(theme::FAINT),
                        );
                    }
                });
        }
    }

    /// El run en texto plano, para el portapapeles.
    ///
    /// Texto y no JSON porque el destino es un ticket o un mensaje a un
    /// compañero, no otro programa.
    fn export_run(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("# Run de Lucy · {}\n", self.chat_model));
        if !self.tabs[self.tab].ws.plan.is_empty() {
            s.push_str("\n## Plan\n");
            for p in &self.tabs[self.tab].ws.plan {
                s.push_str(&format!("- [{:?}] {}\n", p.status, p.label));
            }
        }
        if !self.tabs[self.tab].ws.exec.is_empty() {
            s.push_str("\n## Ejecución\n");
            for e in &self.tabs[self.tab].ws.exec {
                s.push_str(&format!(
                    "\n$ {}\n{}\n",
                    e.cmd,
                    if e.output.is_empty() { "(sin salida)" } else { &e.output }
                ));
            }
        }
        if !self.tabs[self.tab].ws.trace.is_empty() {
            s.push_str("\n## Trace\n");
            for t in &self.tabs[self.tab].ws.trace {
                s.push_str(&format!("- {} · {} — {}\n", t.phase, t.label, t.detail));
            }
        }
        if !self.tabs[self.tab].ws.artifacts.is_empty() {
            s.push_str("\n## Artefactos\n");
            for a in &self.tabs[self.tab].ws.artifacts {
                s.push_str(&format!("- {} {}\n", a.kind.label(), a.path));
            }
        }
        s
    }

    /// Log Viewer — la cola de `lucy_app.log`, con filtro por nivel.
    ///
    /// La ruta se construye aquí, fija: `%APPDATA%\Lucy\logs\lucy_app.log`, la
    /// misma que escribe `write_app_log`. Por eso esta vista NO necesita la
    /// guarda de rutas sensibles que sí lleva el comando Tauri — allí la ruta la
    /// puede pedir un modelo, aquí la pone el programa.
    fn log_viewer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("LOG DE LA APP").strong());
            if ui.button("↻ Recargar").clicked() {
                self.reload_log();
            }
            ui.separator();
            // El filtro es acumulativo, no excluyente: querer ver errores Y
            // avisos a la vez es lo normal cuando se investiga algo.
            ui.checkbox(&mut self.log_error, "Error");
            ui.checkbox(&mut self.log_warn, "Warn");
            ui.checkbox(&mut self.log_info, "Info");
        });

        match &self.log_lines {
            Err(e) => {
                ui.add_space(6.0);
                ui.colored_label(theme::AMBER, format!("⚠ {e}"));
                ui.label(
                    egui::RichText::new(
                        "El log aparece en cuanto Lucy arranca al menos una vez.",
                    )
                    .small()
                    .color(theme::TXT3),
                );
            }
            Ok(lines) => {
                let visible: Vec<&String> = lines
                    .iter()
                    .filter(|l| match lucy_core::logs::Level::of(l) {
                        lucy_core::logs::Level::Error => self.log_error,
                        lucy_core::logs::Level::Warn => self.log_warn,
                        lucy_core::logs::Level::Info => self.log_info,
                    })
                    .collect();
                ui.label(
                    egui::RichText::new(format!("{} de {} líneas", visible.len(), lines.len()))
                        .small()
                        .color(theme::TXT3),
                );
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for l in visible {
                            let color = match lucy_core::logs::Level::of(l) {
                                lucy_core::logs::Level::Error => theme::RED,
                                lucy_core::logs::Level::Warn => theme::AMBER,
                                lucy_core::logs::Level::Info => theme::TXT2,
                            };
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(l).monospace().size(11.5).color(color),
                                )
                                .wrap(),
                            );
                        }
                    });
            }
        }
    }

    /// Relee la cola del log. 2000 líneas: suficiente para una sesión larga y
    /// lejos del tope de 50 000 del core.
    fn reload_log(&mut self) {
        self.log_lines = log_path()
            .ok_or_else(|| "no se pudo resolver %APPDATA%".to_string())
            .and_then(|p| lucy_core::logs::tail(&p, 2_000));
    }

    /// Lanza la búsqueda semántica sobre `lucy-core::vectors`.
    ///
    /// Bloquea el frame: es una petición a Ollama en localhost, decenas de
    /// milisegundos, y ocurre solo al pulsar. Mover esto a un hilo es correcto
    /// cuando se note — pero un spinner sobre una espera de 30 ms es peor
    /// experiencia que la espera, y complica el estado a cambio de nada. Si el
    /// corpus crece hasta que se sienta, el sitio para arreglarlo es éste.
    fn run_semantic_search(&mut self) {
        let q = self.mem_search.trim();
        if q.is_empty() {
            self.sem_result = None;
            return;
        }
        // 'memory' es el entity_type que escriben los dos frontends — la app
        // Tauri en upsert_embedding y el backfill.
        self.sem_result = Some(lucy_core::vectors::search(q, "memory", 8, 0.25));
    }
    /// Refresca las métricas respetando cada cadencia. `force` las salta todas
    /// — es lo que hace el botón ↻, que debe dar datos frescos ahora y no
    /// "dentro de 27 segundos".
    fn refresh_system(&mut self, force: bool) {
        if force || self.sys_last.elapsed() >= Duration::from_millis(1000) {
            self.sys.refresh(); // el % de CPU es el delta desde el último refresco
            self.net = self.sys.net_rate();
            self.sys_last = Instant::now();
            self.sys_stamp = stamp_now();

            // Historial de las líneas de tendencia. 44 muestras es lo que guarda
            // la V2; a un segundo por muestra son los últimos 44 segundos, que
            // es el horizonte en el que una subida todavía significa algo.
            let s = self.sys.snapshot();
            push_hist(&mut self.cpu_hist, s.cpu_pct);
            push_hist(&mut self.ram_hist, mem_pct(&s));
        }
        if force || self.procs_last.elapsed() >= Duration::from_secs(3) {
            self.procs = self.sys.top_processes(8, self.proc_by_cpu);
            self.procs_last = Instant::now();
        }

        // ── servicios: EN OTRO HILO ──────────────────────────────────────────
        //
        // Lanzar PowerShell tarda cientos de milisegundos y esto corría en el
        // hilo de interfaz: cada 30 segundos la ventana se quedaba clavada. En
        // una aplicación cuya razón de ser es que el WebView se congelaba, meter
        // una congelación propia es el peor error posible.
        //
        // El hilo manda el resultado por un canal y `pump_services` lo recoge
        // cuando llega. De paso, `svc_rx.is_some()` es "hay una sonda en curso",
        // que es lo que anima el botón de refresco.
        #[cfg(windows)]
        if self.svc_rx.is_none() && (force || self.svc_last.elapsed() >= Duration::from_secs(30)) {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                // Un fallo NO se propaga: lanzar PowerShell puede fallar por
                // política del equipo, y eso no debe vaciar un dashboard cuyo
                // resto de datos es correcto. Se manda `None` y quien recibe
                // conserva la última lista buena.
                let _ = tx.send(lucy_core::system::down_services(12).ok());
            });
            self.svc_rx = Some(rx);
            self.svc_last = Instant::now();
        }
    }

    /// Recoge el resultado de la sonda de servicios si ya llegó.
    fn pump_services(&mut self) {
        let Some(rx) = &self.svc_rx else { return };
        match rx.try_recv() {
            Ok(Some(v)) => {
                self.services = v;
                self.svc_rx = None;
            }
            // Sonda fallida o hilo caído: se cierra el turno y se deja intacta
            // la última lista conocida.
            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => self.svc_rx = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Progreso 0→1 de la animación de entrada del bloque `idx`.
    ///
    /// La V2 escalona los hijos del dashboard 50 ms cada uno mientras aparecen y
    /// los desliza 10 px hacia arriba. Aquí se conserva el escalonado y la
    /// opacidad; el desplazamiento no, porque en modo inmediato la posición de
    /// un bloque la fija el flujo, y moverlo empujaría a todos los siguientes.
    fn entrance(&self, idx: usize) -> f32 {
        if !motion() {
            return 1.0;
        }
        let Some(t0) = self.dash_shown else { return 1.0 };
        let delay = idx.min(6) as f32 * 0.05;
        let t = (t0.elapsed().as_secs_f32() - delay) / theme::DUR_SLOW;
        ease_out(t.clamp(0.0, 1.0))
    }

    /// Tarjeta KPI: rótulo de instrumento, cifra héroe, tendencia o barra, y el
    /// detalle debajo.
    ///
    /// La jerarquía tipográfica ES el diseño del Cockpit: la cifra manda en
    /// mono a 28 —monoespaciada para que las lecturas no "bailen" al refrescar,
    /// que es lo que hace que el panel se lea como instrumental y no como una
    /// plantilla—, la unidad se apoya en su base, y el detalle cae a `faint`.
    ///
    /// La cifra NO se tiñe por umbral. En la V2 siempre es `--text-primary`: el
    /// color vive en la barra y en la tira de alertas. Un número que cambia de
    /// color compite con ellas y dice lo mismo dos veces.
    fn kpi_card(ui: &mut egui::Ui, size: egui::Vec2, k: Kpi<'_>) {
        let inner_w = size.x - 28.0;
        card(ui, size, 14.0, |ui| {
            row_align(ui, 18.0, egui::Align::Center, |ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                ui.label(egui::RichText::new(k.icon).size(14.0).color(theme::ACC));
                ui.add(egui::Label::new(theme::instrument_label(k.title, theme::FAINT)));
            });
            ui.add_space(8.0);

            // La unidad se alinea por ABAJO con la cifra: centrada, el `%` flota
            // a media altura del número y parece un exponente.
            let is_text = !k.text.is_empty();
            let vsize = if is_text { theme::FS_HEADING } else { 28.0 };
            row_align(ui, vsize * 1.45, egui::Align::Max, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if is_text {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&k.text).size(vsize).color(theme::TXT),
                        )
                        .truncate(),
                    );
                    return;
                }
                // Cuenta desde cero al entrar y se desliza al nuevo valor en
                // cada sondeo, como el `tweened` de la V2: una cifra que salta
                // de golpe se lee como texto estático que alguien reemplazó.
                let shown = ui.ctx().animate_value_with_time(
                    egui::Id::new(("kpi", k.title)),
                    k.value,
                    if motion() { 0.65 } else { 0.0 },
                );
                ui.label(
                    egui::RichText::new(format!("{shown:.0}"))
                        .size(vsize)
                        .monospace()
                        .color(theme::TXT),
                );
                if !k.unit.is_empty() {
                    ui.label(
                        egui::RichText::new(k.unit)
                            .size(14.0)
                            .color(theme::TXT3),
                    );
                }
            });

            if !k.spark.is_empty() {
                ui.add_space(10.0);
                sparkline(ui, inner_w, 26.0, k.spark, theme::ACC);
                ui.add_space(6.0);
            }
            if let Some(frac) = k.bar {
                ui.add_space(12.0);
                meter(ui, inner_w, 5.0, frac, theme::meter_color(frac * 100.0), k.title, theme::DUR_SLOW);
                ui.add_space(8.0);
            }

            // Truncado, no ajustado: un texto que se parte en dos líneas dentro
            // de una tarjeta de altura fija se sale por abajo.
            for line in [&k.sub, &k.sub2] {
                if line.is_empty() {
                    continue;
                }
                row(ui, 16.0, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(line)
                                .size(theme::FS_CAPTION)
                                .color(theme::FAINT),
                        )
                        .truncate(),
                    );
                });
            }
        });
    }

    /// Tarjeta de un núcleo: `C7`, su porcentaje, y una barra fina.
    ///
    /// Función aparte y no un bloque dentro del bucle porque es la pieza que se
    /// repite 32 veces: si una sola se sale de su caja, la rejilla entera se
    /// descuadra. Suelta, un test la puede medir.
    fn core_card(ui: &mut egui::Ui, w: f32, i: usize, pct: f32, host_cpu: f32) {
        card(ui, egui::vec2(w, CORE_H), 9.0, |ui| {
            row_align(ui, 16.0, egui::Align::Center, |ui| {
                ui.label(
                    egui::RichText::new(format!("C{i}"))
                        .size(theme::FS_CAPTION)
                        .monospace()
                        .color(theme::FAINT),
                );
                right(ui, 16.0, |ui| {
                    ui.label(
                        egui::RichText::new(format!("{pct:.0}%"))
                            .size(theme::FS_FOOTNOTE)
                            .monospace()
                            .color(theme::TXT2),
                    );
                });
            });
            ui.add_space(6.0);
            meter(
                ui,
                w - 18.0,
                4.0,
                pct / 100.0,
                theme::core_color(pct, host_cpu),
                // Cada núcleo necesita su propia animación: con una sola clave
                // los 32 compartirían valor y la rejilla parpadearía a la vez.
                &format!("core-{i}"),
                theme::DUR_BASE,
            );
        });
    }

    fn pendiente(&mut self, ui: &mut egui::Ui, v: View) {
        let label = v.label();
        ui.add_space(48.0);
        ui.vertical_centered(|ui| {
            // El icono de la vista, atenuado: dice CUÁL falta sin gritarlo.
            icons::show(ui, v.icon(), 34.0, theme::TXT3.linear_multiply(0.5));
            ui.add_space(10.0);
            ui.label(egui::RichText::new(label).size(17.0).color(theme::TXT));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Todavía no migrada al shell nativo")
                    .size(11.5)
                    .color(theme::TXT3),
            );
            ui.add_space(18.0);

            if let Some(needs) = v.pending_needs() {
                egui::Frame::none()
                    .fill(theme::BG2)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
                    .rounding(egui::Rounding::same(6.0))
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.set_max_width(430.0);
                        ui.label(
                            egui::RichText::new("QUÉ FALTA")
                                .size(9.5)
                                .strong()
                                .color(theme::ACC),
                        );
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(needs).size(11.5).color(theme::TXT2));
                    });
            }
        });
    }


    /// El selector de equipo: píldora + menú, con los equipos REALES del
    /// operador.
    ///
    /// El menú se cierra al pulsar fuera, que es lo que hace el `.host-backdrop`
    /// del CSS; aquí lo da egui con `CloseOnClickOutside` en vez de un elemento
    /// invisible a pantalla completa.
    fn host_picker(&mut self, ui: &mut egui::Ui) {
        let is_local = self.selected_host == "local";
        let name = if is_local {
            "Este equipo".to_string()
        } else {
            self.remote_hosts
                .iter()
                .find(|h| h.id == self.selected_host)
                // Si el equipo seleccionado ya no está en el índice —lo borraron
                // desde la app web mientras esto estaba abierto— se dice, en vez
                // de enseñar un dashboard sin dueño.
                .map_or_else(|| "(equipo no encontrado)".to_string(), |h| h.name.clone())
        };
        let pill = host_pill(
            ui,
            if is_local { icons::Icon::Desktop } else { icons::Icon::Server },
            &name,
        );
        let popup_id = ui.make_persistent_id("host-menu");
        if pill.clicked() {
            // Se relee al abrir: el operador puede haber dado de alta un equipo
            // en la app web hace un minuto, y una lista cacheada al arrancar no
            // lo tendría.
            self.remote_hosts = hosts::load();
            ui.memory_mut(|m| m.toggle_popup(popup_id));
        }

        let mut elegido: Option<String> = None;
        egui::popup::popup_below_widget(
            ui,
            popup_id,
            &pill,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                let w = 236.0_f32;
                ui.set_min_width(w);
                ui.spacing_mut().item_spacing.y = 1.0;
                if host_option(ui, w, icons::Icon::Desktop, "Este equipo", "local", is_local) {
                    elegido = Some("local".to_string());
                }
                for h in &self.remote_hosts {
                    if host_option(
                        ui,
                        w,
                        icons::Icon::Server,
                        &h.name,
                        h.transport(),
                        h.id == self.selected_host,
                    ) {
                        elegido = Some(h.id.clone());
                    }
                }
                if self.remote_hosts.is_empty() {
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("Sin equipos remotos dados de alta")
                            .size(theme::FS_CAPTION)
                            .color(theme::FAINT),
                    );
                }
            },
        );

        if let Some(id) = elegido {
            ui.memory_mut(|m| m.close_popup());
            if id != self.selected_host {
                self.selected_host = id;
                // El historial es de ESTE equipo. Arrastrarlo al cambiar de host
                // dibujaría la tendencia de una máquina bajo el nombre de otra.
                self.cpu_hist.clear();
                self.ram_hist.clear();
                self.services.clear();
                self.dash_shown = Some(Instant::now());
            }
        }
    }

    /// Dashboard de un equipo remoto — la parte que todavía no está migrada.
    ///
    /// AQUÍ NO SE ENSEÑAN LAS MÉTRICAS LOCALES. Sería trivial y sería mentir:
    /// un panel que pone "SRV-DC01" encima de la CPU de esta máquina es peor que
    /// uno que no está, porque el operador no tiene forma de notarlo.
    ///
    /// Lo que falta es concreto: el sondeo remoto va por WinRM/SSH y ese
    /// transporte vive en `src-tauri` junto a los guardrails que revisan la
    /// credencial antes de usarla. Se migra entero o no se migra — llevarse el
    /// transporte y dejar atrás el control que lo protege es exactamente la
    /// clase de atajo que no se toma con contraseñas.
    fn remoto(&mut self, ui: &mut egui::Ui) {
        let h = self.remote_hosts.iter().find(|h| h.id == self.selected_host);
        let (name, dest, via) = match h {
            Some(h) => (
                h.name.clone(),
                format!("{}@{}", h.username, h.host),
                h.transport(),
            ),
            None => (
                "(equipo no encontrado)".to_string(),
                "—".to_string(),
                "—",
            ),
        };
        ui.add_space(28.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("▤").size(34.0).color(theme::FAINT));
            ui.add_space(10.0);
            ui.label(egui::RichText::new(&name).size(theme::FS_TITLE).color(theme::TXT));
            ui.add_space(3.0);
            ui.label(
                egui::RichText::new(format!("{dest} · {via}"))
                    .size(theme::FS_CAPTION)
                    .monospace()
                    .color(theme::FAINT),
            );
            ui.add_space(20.0);
            card_on(ui, egui::vec2(460.0, 132.0), 16.0, theme::BG2, |ui| {
                panel_title(ui, "◉", "Qué falta");
                ui.add_space(10.0);
                for line in [
                    "El sondeo remoto (`get_remote_health_windows` / `_linux`)",
                    "todavía vive en src-tauri, junto al transporte WinRM y a",
                    "los guardrails que revisan la credencial antes de usarla.",
                    "Se migra el bloque entero o no se migra.",
                ] {
                    row(ui, 16.0, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(line)
                                    .size(theme::FS_CAPTION)
                                    .color(theme::TXT2),
                            )
                            .truncate(),
                        );
                    });
                }
            });
        });
    }

    /// Las alertas derivadas de la V2 — y de ellas sale el estado de salud.
    ///
    /// Los umbrales son distintos por métrica y eso es deliberado: una CPU al
    /// 80 % es una máquina trabajando, un disco al 80 % es una máquina a la que
    /// le queda poco. Un único umbral para todo es cómo un panel acaba avisando
    /// tarde de lo que importa y pronto de lo que no.
    fn alerts(&self, s: &lucy_core::system::SysSnapshot) -> Vec<(Sev, String)> {
        let mut a = Vec::new();
        let cpu = s.cpu_pct;
        if cpu >= 90.0 {
            a.push((Sev::Bad, format!("CPU al {cpu:.0}%")));
        } else if cpu >= 78.0 {
            a.push((Sev::Warn, format!("CPU alta ({cpu:.0}%)")));
        }
        let mp = mem_pct(s);
        if mp >= 92.0 {
            a.push((Sev::Bad, format!("RAM al {mp:.0}%")));
        } else if mp >= 82.0 {
            a.push((Sev::Warn, format!("RAM alta ({mp:.0}%)")));
        }
        for d in &s.disks {
            let pct = disk_pct(d);
            if pct >= 93.0 {
                a.push((Sev::Bad, format!("Disco {} al {pct:.0}%", d.mount)));
            } else if pct >= 86.0 {
                a.push((Sev::Warn, format!("Disco {} al {pct:.0}%", d.mount)));
            }
        }
        // Solo los CAÍDOS. Un servicio parado limpio se informa en su tarjeta y
        // no levanta alerta: contarlos hacía que el equipo pasara a "Atención"
        // un minuto después de cada arranque, y un indicador que avisa en cada
        // arranque deja de leerse.
        let crashed = self.services.iter().filter(|s| s.crashed()).count();
        if crashed > 0 {
            a.push((Sev::Warn, format!("{crashed} servicio(s) con fallo de arranque")));
        }
        a
    }

    /// Dashboard de sistema — el diseño del Cockpit V2, sobre una rejilla
    /// explícita.
    ///
    /// Todo el panel cuelga de un ancho: `full`. Las KPI reparten ese ancho en
    /// columnas iguales, la fila de red/servicios usa las mismas columnas, y
    /// núcleos y discos calculan las suyas con el mismo hueco. Por eso los
    /// bordes verticales caen unos sobre otros en vez de aparecer donde el
    /// contenido de cada tarjeta decida.
    fn sistema(&mut self, ui: &mut egui::Ui) {
        let s = self.sys.snapshot();
        let net = self.net;
        let alerts = self.alerts(&s);
        let ctx = ui.ctx().clone();

        // La entrada escalonada sigue corriendo aunque no llegue ningún dato:
        // hay que pedir repintado mientras dure, o se vería a saltos de 1 Hz.
        if self.entrance(5) < 1.0 {
            ctx.request_repaint();
        }
        let ent: [f32; 6] = std::array::from_fn(|i| self.entrance(i));

        // ── cabecera ─────────────────────────────────────────────────────────
        row_align(ui, 30.0, egui::Align::Center, |ui| {
            ui.label(
                egui::RichText::new("Dashboard de sistema")
                    .size(theme::FS_TITLE)
                    .color(theme::TXT),
            );
            self.host_picker(ui);

            let (sal_txt, sal_col, sal_bg) = if alerts.iter().any(|(v, _)| *v == Sev::Bad) {
                ("Crítico", theme::RED, theme::RED_BG)
            } else if alerts.is_empty() {
                ("Saludable", theme::ACC, theme::ACC_BG)
            } else {
                ("Atención", theme::AMBER, theme::AMBER_BG)
            };
            egui::Frame::none()
                .fill(sal_bg)
                .rounding(egui::Rounding::same(999.0))
                .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(egui::RichText::new("●").size(7.0).color(sal_col));
                    ui.label(
                        egui::RichText::new(sal_txt)
                            .size(theme::FS_CAPTION)
                            .color(sal_col),
                    );
                });
            ui.label(
                egui::RichText::new(format!("act. {}", self.sys_stamp))
                    .size(theme::FS_CAPTION)
                    .monospace()
                    .color(theme::FAINT),
            );

            let mut pedir = false;
            right(ui, 26.0, |ui| {
                // Mientras la sonda de servicios trabaja en su hilo, el botón se
                // convierte en el indicador de que algo está en marcha. Es la
                // versión nativa del `.spin` del CSS, y solo es posible porque
                // la sonda dejó de bloquear el hilo de interfaz.
                if self.svc_rx.is_some() {
                    ui.add(egui::Spinner::new().size(15.0).color(theme::ACC));
                } else {
                    let (rr, rresp) =
                        ui.allocate_exact_size(egui::vec2(30.0, 26.0), egui::Sense::click());
                    ui.painter().rect(
                        rr,
                        egui::Rounding::same(theme::R_MD),
                        if rresp.hovered() { theme::BG3 } else { egui::Color32::TRANSPARENT },
                        egui::Stroke::new(1.0_f32, theme::BDR),
                    );
                    icons::draw(
                        ui.painter(),
                        icons::Icon::Refresh,
                        rr.center(),
                        16.0,
                        if rresp.hovered() { theme::TXT } else { theme::TXT3 },
                    );
                    pedir = rresp.on_hover_text("Actualizar ahora").clicked();
                }
            });
            if pedir {
                self.refresh_system(true);
            }
        });
        ui.add_space(10.0);

        // El cuerpo pertenece al equipo seleccionado. Con uno remoto no se
        // dibuja lo local: ver `remoto`.
        if self.selected_host != "local" {
            self.remoto(ui);
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Se descuenta el canal de la barra de desplazamiento: si no,
                // la última columna queda debajo de ella cuando aparece.
                let full = (ui.available_width() - 8.0).max(240.0);

                // ── tira de alertas ──────────────────────────────────────────
                if !alerts.is_empty() {
                    block(ui, ent[0], |ui| {
                        let h = 22.0 + 16.0 * (1 + alerts.len() / 3) as f32;
                        card_on(ui, egui::vec2(full, h), 12.0, theme::BG2, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "⚠ {} alerta{}",
                                        alerts.len(),
                                        if alerts.len() > 1 { "s" } else { "" }
                                    ))
                                    .size(theme::FS_FOOTNOTE)
                                    .color(theme::AMBER)
                                    .strong(),
                                );
                                for (sev, txt) in &alerts {
                                    let (c, bg) = match sev {
                                        Sev::Bad => (theme::RED, theme::RED_BG),
                                        Sev::Warn => (theme::AMBER, theme::AMBER_BG),
                                    };
                                    egui::Frame::none()
                                        .fill(bg)
                                        .rounding(egui::Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(9.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(
                                                egui::RichText::new(txt)
                                                    .size(theme::FS_CAPTION)
                                                    .color(c),
                                            );
                                        });
                                }
                            });
                        });
                    });
                    ui.add_space(GAP);
                }

                // ── KPI ──────────────────────────────────────────────────────
                //
                // El número de columnas sale de cuántas tarjetas hay, no al
                // revés: sin disco montado son tres y ocupan el ancho entero
                // igual, sin dejar un hueco donde estaba la cuarta.
                let disk0 = s.disks.first();
                let n_kpi = 3 + usize::from(disk0.is_some());
                let kw = cell_w(full, n_kpi);
                let mp = mem_pct(&s);
                let cpu_hist = &self.cpu_hist;
                let ram_hist = &self.ram_hist;
                block(ui, ent[1], |ui| {
                    row(ui, KPI_H, |ui| {
                        Self::kpi_card(
                            ui,
                            egui::vec2(kw, KPI_H),
                            Kpi {
                                icon: "▣",
                                title: "CPU",
                                value: s.cpu_pct,
                                unit: "%",
                                spark: &cpu_hist,
                                sub: format!("{} núcleos", s.cores),
                                ..Default::default()
                            },
                        );
                        Self::kpi_card(
                            ui,
                            egui::vec2(kw, KPI_H),
                            Kpi {
                                icon: "◈",
                                title: "RAM",
                                value: mp,
                                unit: "%",
                                spark: &ram_hist,
                                sub: format!("{} / {}", fmt_gb(s.mem_used), fmt_gb(s.mem_total)),
                                ..Default::default()
                            },
                        );
                        if let Some(d) = disk0 {
                            let pct = disk_pct(d);
                            Self::kpi_card(
                                ui,
                                egui::vec2(kw, KPI_H),
                                Kpi {
                                    icon: "▤",
                                    title: "Disco sistema",
                                    value: pct,
                                    unit: "%",
                                    // Barra y no tendencia: un disco no se mueve
                                    // en 44 segundos, así que la línea sería una
                                    // recta. Lo que se quiere saber es cuánto
                                    // queda, y eso lo dice la ocupación.
                                    bar: Some(pct / 100.0),
                                    sub: format!(
                                        "{} libres de {}",
                                        fmt_gb(d.avail),
                                        fmt_gb(d.total)
                                    ),
                                    ..Default::default()
                                },
                            );
                        }
                        Self::kpi_card(
                            ui,
                            egui::vec2(kw, KPI_H),
                            Kpi {
                                icon: "◱",
                                title: "Sistema",
                                text: s.host.clone(),
                                sub: s.os.clone(),
                                sub2: format!("Uptime {}", fmt_uptime(s.uptime_secs)),
                                ..Default::default()
                            },
                        );
                    });
                });
                ui.add_space(GAP);

                // ── red + servicios ──────────────────────────────────────────
                let netw = cell_w(full, 4);
                let svw = full - netw - GAP;
                let services = &self.services;
                block(ui, ent[2], |ui| {
                    row(ui, NET_H, |ui| {
                        card(ui, egui::vec2(netw, NET_H), 14.0, |ui| {
                            panel_title(ui, "◈", "Red");
                            ui.add_space(10.0);
                            row_align(ui, 22.0, egui::Align::Max, |ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for (glyph, rate, col) in [
                                    ("↓", net.rx_bps, theme::ACC),
                                    ("↑", net.tx_bps, theme::BLUE),
                                ] {
                                    ui.label(
                                        egui::RichText::new(glyph)
                                            .size(theme::FS_HEADING)
                                            .color(col),
                                    );
                                    ui.label(
                                        egui::RichText::new(fmt_rate(rate))
                                            .size(theme::FS_HEADING)
                                            .monospace()
                                            .color(col),
                                    );
                                    ui.add_space(10.0);
                                }
                            });
                        });

                        card(ui, egui::vec2(svw, NET_H), 14.0, |ui| {
                            panel_title(ui, "◉", "Servicios detenidos");
                            ui.add_space(10.0);
                            if services.is_empty() {
                                ui.label(
                                    egui::RichText::new(
                                        "✓ Todos los servicios automáticos en ejecución",
                                    )
                                    .size(theme::FS_CAPTION)
                                    .color(theme::ACC),
                                );
                                return;
                            }
                            let inner = svw - 28.0;
                            let scols = fit_cols(inner, 230.0);
                            let cap = scols * 3;
                            let hidden = services.len().saturating_sub(cap);
                            let shown = if hidden > 0 { cap - 1 } else { services.len() };
                            let cw = cell_w(inner, scols);
                            for line in services[..shown].chunks(scols) {
                                row(ui, 18.0, |ui| {
                                    for sv in line {
                                        svc_row(ui, cw, &sv.name, sv.crashed());
                                    }
                                });
                            }
                            if hidden > 0 {
                                // Se sacrifica un hueco para decir cuántos
                                // quedan: una lista recortada en silencio miente
                                // sobre el estado del equipo.
                                ui.label(
                                    egui::RichText::new(format!("+{} más", hidden + 1))
                                        .size(theme::FS_CAPTION)
                                        .color(theme::FAINT),
                                );
                            }
                        });
                    });
                });

                // ── núcleos ──────────────────────────────────────────────────
                if !s.per_core.is_empty() {
                    let cores = &s.per_core;
                    let host_cpu = s.cpu_pct;
                    block(ui, ent[3], |ui| {
                        section(ui, "Núcleos", Some(cores.len().to_string()));
                        let ccols = fit_cols(full, 100.0);
                        let ccw = cell_w(full, ccols);
                        for (r, chunk) in cores.chunks(ccols).enumerate() {
                            row(ui, CORE_H, |ui| {
                                for (c, pct) in chunk.iter().enumerate() {
                                    Self::core_card(ui, ccw, r * ccols + c, *pct, host_cpu);
                                }
                            });
                            ui.add_space(GAP);
                        }
                    });
                }

                // ── discos ───────────────────────────────────────────────────
                if !s.disks.is_empty() {
                    let disks = &s.disks;
                    block(ui, ent[4], |ui| {
                        section(
                            ui,
                            "Discos",
                            Some(if disks.len() == 1 {
                                "1 volumen".to_string()
                            } else {
                                format!("{} volúmenes", disks.len())
                            }),
                        );
                        // Tres columnas como máximo: una barra de uso estirada a
                        // lo ancho de una pantalla de 27" no se lee mejor.
                        let dcols = fit_cols(full, 420.0).min(3);
                        let dcw = cell_w(full, dcols);
                        for chunk in disks.chunks(dcols) {
                            row(ui, DISK_H, |ui| {
                                for d in chunk {
                                    disk_card(ui, dcw, d);
                                }
                            });
                            ui.add_space(GAP);
                        }
                    });
                }

                // ── top procesos ─────────────────────────────────────────────
                let t_proc = ent[5];
                let procs = &self.procs;
                let mut cambiar: Option<bool> = None;
                let by_cpu = self.proc_by_cpu;
                block(ui, t_proc, |ui| {
                    let w_cpu = 60.0;
                    let w_ram = 76.0;
                    let w_pid = 60.0;
                    let w_name = (full - 28.0 - w_cpu - w_ram - w_pid - GAP * 3.0).max(140.0);
                    let table_h = 28.0 + 20.0 + 8.0 + procs.len() as f32 * PROC_ROW;
                    card_on(ui, egui::vec2(full, table_h), 14.0, theme::BG2, |ui| {
                        row_align(ui, 20.0, egui::Align::Center, |ui| {
                            ui.add(egui::Label::new(theme::instrument_label(
                                "Top procesos",
                                theme::FAINT,
                            )));
                            // El selector va en la cabecera de la tabla, que es
                            // donde el operador ya está mirando cuando decide
                            // por qué columna ordenar.
                            right(ui, 22.0, |ui| {
                                egui::Frame::none()
                                    .fill(theme::BG3)
                                    .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
                                    .rounding(egui::Rounding::same(theme::R_SM))
                                    .inner_margin(egui::Margin::same(2.0))
                                    .show(ui, |ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        // Cambiar de criterio RECARGA la lista:
                                        // reordenar la que ya está en pantalla
                                        // mostraría el top-8 por RAM reordenado
                                        // por CPU, que no es el top-8 por CPU.
                                        if seg(ui, "CPU", by_cpu) {
                                            cambiar = Some(true);
                                        }
                                        if seg(ui, "RAM", !by_cpu) {
                                            cambiar = Some(false);
                                        }
                                    });
                            });
                        });
                        ui.add_space(8.0);
                        let head = |t: &str| {
                            egui::RichText::new(t.to_string())
                                .size(theme::FS_CAPTION)
                                .color(theme::FAINT)
                        };
                        row(ui, 18.0, |ui| {
                            cell(ui, w_name, 18.0, false, head("PROCESO"));
                            cell(ui, w_cpu, 18.0, true, head("CPU"));
                            cell(ui, w_ram, 18.0, true, head("RAM"));
                            cell(ui, w_pid, 18.0, true, head("PID"));
                        });
                        for (i, p) in procs.iter().enumerate() {
                            row(ui, PROC_ROW, |ui| {
                                if i > 0 {
                                    // Filete fino entre filas, como el CSS. Se
                                    // puede pintar en el sitio exacto porque la
                                    // fila tiene altura conocida.
                                    let r = ui.max_rect();
                                    ui.painter().hline(
                                        r.left()..=r.right(),
                                        r.top(),
                                        egui::Stroke::new(1.0_f32, theme::BDR),
                                    );
                                }
                                cell(
                                    ui,
                                    w_name,
                                    PROC_ROW,
                                    false,
                                    egui::RichText::new(&p.name)
                                        .size(theme::FS_FOOTNOTE)
                                        .monospace()
                                        .color(theme::TXT2),
                                );
                                // La columna por la que se ordena va en acento:
                                // dice cuál manda sin repetirlo en un rótulo.
                                cell(
                                    ui,
                                    w_cpu,
                                    PROC_ROW,
                                    true,
                                    egui::RichText::new(format!("{:.0}%", p.cpu_pct))
                                        .size(theme::FS_FOOTNOTE)
                                        .monospace()
                                        .color(if by_cpu { theme::ACC } else { theme::TXT3 }),
                                );
                                cell(
                                    ui,
                                    w_ram,
                                    PROC_ROW,
                                    true,
                                    egui::RichText::new(fmt_gb(p.mem_bytes))
                                        .size(theme::FS_FOOTNOTE)
                                        .monospace()
                                        .color(if by_cpu { theme::TXT3 } else { theme::ACC }),
                                );
                                cell(
                                    ui,
                                    w_pid,
                                    PROC_ROW,
                                    true,
                                    egui::RichText::new(p.pid.to_string())
                                        .size(theme::FS_CAPTION)
                                        .monospace()
                                        .color(theme::FAINT),
                                );
                            });
                        }
                    });
                });
                if let Some(by_cpu) = cambiar {
                    if by_cpu != self.proc_by_cpu {
                        self.proc_by_cpu = by_cpu;
                        self.procs = self.sys.top_processes(8, by_cpu);
                    }
                }
                ui.add_space(GAP);
            });
    }

    fn terminal(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("TERMINAL (PowerShell · portable-pty)").strong());
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.term_input)
                .hint_text("comando + Enter…")
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Monospace),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(pty) = &mut self.pty {
                let line = std::mem::take(&mut self.term_input);
                pty.send(&format!("{line}\r"));
            }
            resp.request_focus();
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                // La pantalla VT ya viene limpia (sin secuencias de escape).
                let contents = self.vt.screen().contents();
                ui.add(
                    egui::Label::new(egui::RichText::new(contents).monospace().size(12.0)).wrap(),
                );
            });
    }

    fn memoria(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("MEMORIA (DB real · solo-lectura)").strong());
            if ui.button("↻ Recargar").clicked() {
                self.mems = load_memories();
            }
        });
        // La búsqueda se PIDE dentro del match (que tiene prestado `self.mems`)
        // y se EJECUTA al salir. `run_semantic_search` necesita `&mut self`, así
        // que llamarla ahí dentro no compila — y forzarlo con un clon de las
        // memorias sería copiar un vector entero por frame para evitar un
        // booleano.
        let mut pedir_semantica = false;

        match &self.mems {
            Err(e) => {
                ui.colored_label(theme::RED, format!("⚠ {e}"));
                ui.label(
                    egui::RichText::new(
                        "Abre Lucy al menos una vez para crear la DB, o corre desde el mismo usuario.",
                    )
                    .weak(),
                );
            }
            Ok(mems) => {
                let q = self.mem_search.to_lowercase();
                ui.horizontal(|ui| {
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut self.mem_search)
                            .hint_text("filtrar por texto — Intro para búsqueda semántica")
                            .desired_width(ui.available_width() - 108.0),
                    );
                    let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("◈ Semántica").clicked() || enter {
                        pedir_semantica = true;
                    }
                });

                // ── resultado semántico ───────────────────────────────────────
                // Se enseña ENCIMA del filtro de texto, no en su lugar: son dos
                // preguntas distintas. "Filtrar" busca una palabra que recuerdas;
                // "semántica" busca un tema que no sabes cómo escribiste.
                if let Some(res) = &self.sem_result {
                    match res {
                        Err(e) => {
                            ui.add_space(4.0);
                            ui.colored_label(theme::AMBER, format!("⚠ {e}"));
                            ui.label(
                                egui::RichText::new(
                                    "La búsqueda semántica necesita Ollama con un modelo de \
                                     embeddings (ollama pull nomic-embed-text).",
                                )
                                .small()
                                .color(theme::TXT3),
                            );
                        }
                        Ok((hits, notes)) => {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!("{} por similitud", hits.len()))
                                    .small()
                                    .color(theme::ACC),
                            );
                            // Las filas descartadas se DICEN. Enseñar menos
                            // resultados sin explicar por qué es el fallo que
                            // este proyecto lleva persiguiendo toda la semana.
                            for n in notes {
                                ui.label(
                                    egui::RichText::new(format!("⚠ {n}"))
                                        .small()
                                        .color(theme::AMBER),
                                );
                            }
                            for h in hits {
                                egui::Frame::group(ui.style()).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("{:.0}%", h.score * 100.0))
                                                .small()
                                                // Un parecido NO es un uso: en
                                                // la paleta de uso, más es peor,
                                                // y aquí más es mejor. Pintaba
                                                // de rojo justo los aciertos
                                                // buenos.
                                                .color(theme::match_color(h.score)),
                                        );
                                        ui.label(
                                            egui::RichText::new(
                                                h.text.chars().take(140).collect::<String>(),
                                            )
                                            .color(theme::TXT2),
                                        );
                                    });
                                });
                            }
                            ui.separator();
                        }
                    }
                }
                let filtered: Vec<&AgentMemory> = mems
                    .iter()
                    .filter(|m| {
                        q.is_empty()
                            || m.title.to_lowercase().contains(&q)
                            || m.content.to_lowercase().contains(&q)
                            || m.tags.to_lowercase().contains(&q)
                    })
                    .collect();
                ui.label(
                    egui::RichText::new(format!(
                        "{} de {} memorias vivas",
                        filtered.len(),
                        mems.len()
                    ))
                    .small()
                    .weak(),
                );
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for m in filtered {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Los puntos toman el color del NIVEL, no
                                    // el acento fijo: así una memoria de
                                    // importancia 3 se distingue de un vistazo
                                    // sin tener que contar puntos.
                                    let dots = "●".repeat(m.importance.clamp(1, 3) as usize);
                                    ui.label(
                                        egui::RichText::new(dots)
                                            .color(theme::importance_color(m.importance))
                                            .small(),
                                    );
                                    let title = if m.title.trim().is_empty() {
                                        m.content.chars().take(64).collect::<String>()
                                    } else {
                                        m.title.clone()
                                    };
                                    ui.label(egui::RichText::new(title).strong());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(rel_time(m.created_at))
                                                    .small()
                                                    .weak(),
                                            );
                                        },
                                    );
                                });
                                if !m.content.trim().is_empty() {
                                    ui.label(
                                        egui::RichText::new(
                                            m.content.chars().take(220).collect::<String>(),
                                        )
                                        .weak(),
                                    );
                                }
                                let tags = m.tags.trim_matches(|c| c == '[' || c == ']');
                                if !tags.is_empty() {
                                    ui.label(
                                        egui::RichText::new(tags.replace('"', ""))
                                            .small()
                                            .color(theme::BLUE),
                                    );
                                }
                                ui.label(egui::RichText::new(format!("#{}", m.id)).small().weak());
                            });
                        }
                    });
            }
        }

        // Fuera del préstamo de `self.mems`.
        if pedir_semantica {
            self.run_semantic_search();
        }
    }
}

#[cfg(test)]
mod layout {
    use super::*;

    /// Mide lo que un trozo de interfaz ocupa DE VERDAD.
    ///
    /// egui resuelve la geometría sin ventana ni GPU: se le entrega un
    /// `RawInput` y devuelve las posiciones ya calculadas con las métricas
    /// reales de la fuente. Eso convierte "¿cabe el texto dentro de la
    /// tarjeta?" en algo que un test puede contestar — que es justo la clase de
    /// fallo que ni el compilador ve ni una captura de pantalla pilla a tiempo.
    fn measure(width: f32, add: impl Fn(&mut egui::Ui)) -> egui::Rect {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width + 32.0, 4000.0),
            )),
            ..Default::default()
        };
        let mut out = egui::Rect::NOTHING;
        // Dos pasadas: en la primera egui todavía está montando el atlas de
        // fuentes y algunas medidas salen del tamaño por defecto.
        for _ in 0..2 {
            let _ = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::none())
                    .show(ctx, |ui| {
                        ui.set_max_width(width);
                        out = ui.scope(|ui| add(ui)).response.rect;
                    });
            });
        }
        out
    }

    #[test]
    fn una_fila_de_tarjetas_no_crece_en_diagonal() {
        // LA REGRESIÓN. Dentro de una fila sin altura fijada, cada tarjeta
        // agrandaba el hueco disponible de la siguiente, que se centraba en un
        // hueco mayor y se desbordaba más: la rejilla de núcleos salía
        // escalonada en diagonal y creciendo. Ocho tarjetas en fila tienen que
        // medir exactamente lo que mide una.
        let w = 900.0;
        let cw = cell_w(w, 8);
        let r = measure(w, |ui| {
            row(ui, CORE_H, |ui| {
                for i in 0..8 {
                    App::core_card(ui, cw, i, 50.0, 20.0);
                }
            });
        });
        assert!(
            r.height() <= CORE_H + 0.5,
            "ocho tarjetas de núcleo en fila miden {} de alto; una sola mide {CORE_H}",
            r.height()
        );
        assert!(
            r.width() <= w + 0.5,
            "la fila mide {} de ancho y el hueco era {w}",
            r.width()
        );
    }

    #[test]
    fn la_tarjeta_de_nucleo_cabe_en_su_caja() {
        let r = measure(120.0, |ui| App::core_card(ui, 120.0, 31, 100.0, 90.0));
        assert!(r.height() <= CORE_H + 0.5, "mide {}", r.height());
    }

    #[test]
    fn las_kpi_caben_en_su_caja_con_el_contenido_mas_alto() {
        let size = egui::vec2(300.0, KPI_H);

        // Con línea de tendencia: el caso de CPU y RAM, y el más alto de todos
        // — la sparkline ocupa 42 px que la tarjeta de disco no gasta.
        let hist: Vec<f32> = (0..SPARK_SAMPLES).map(|i| i as f32 * 2.0 % 100.0).collect();
        let cpu = measure(300.0, |ui| {
            App::kpi_card(
                ui,
                size,
                Kpi {
                    icon: "▣",
                    title: "CPU",
                    value: 100.0,
                    unit: "%",
                    spark: &hist,
                    sub: "32 núcleos".into(),
                    ..Default::default()
                },
            )
        });
        assert!(
            cpu.height() <= KPI_H + 0.5,
            "la KPI con tendencia mide {} y su caja son {KPI_H}",
            cpu.height()
        );

        // Con barra y una línea de detalle: el caso del disco.
        let disco = measure(300.0, |ui| {
            App::kpi_card(
                ui,
                size,
                Kpi {
                    icon: "▤",
                    title: "Disco sistema",
                    value: 100.0,
                    unit: "%",
                    bar: Some(1.0),
                    sub: "662.6 GB libres de 931.5 GB".into(),
                    ..Default::default()
                },
            )
        });
        assert!(disco.height() <= KPI_H + 0.5, "mide {}", disco.height());

        // Texto y DOS líneas de detalle: el caso de SISTEMA.
        let sistema = measure(300.0, |ui| {
            App::kpi_card(
                ui,
                size,
                Kpi {
                    icon: "◱",
                    title: "Sistema",
                    text: "WORKSTATION-16".into(),
                    sub: "Windows 11 Pro 26200".into(),
                    sub2: "Uptime 12d 7h".into(),
                    ..Default::default()
                },
            )
        });
        assert!(sistema.height() <= KPI_H + 0.5, "mide {}", sistema.height());
    }

    #[test]
    fn un_valor_larguisimo_no_ensancha_la_tarjeta() {
        // El nombre de equipo lo pone el dominio, no nosotros. Se trunca.
        let size = egui::vec2(240.0, KPI_H);
        let r = measure(240.0, |ui| {
            App::kpi_card(
                ui,
                size,
                Kpi {
                    title: "Sistema",
                    text: "SRV-CONTABILIDAD-MEXICO-NORTE-0042".into(),
                    sub: "Microsoft Windows Server 2022 Datacenter Edition".into(),
                    ..Default::default()
                },
            )
        });
        assert!(r.width() <= 240.5, "mide {} de ancho", r.width());
        assert!(r.height() <= KPI_H + 0.5, "mide {} de alto", r.height());
    }

    #[test]
    fn las_columnas_reparten_el_ancho_sin_pasarse() {
        for total in [240.0_f32, 640.0, 1280.0, 1920.0, 3440.0] {
            for cols in 1..=8_usize {
                let w = cell_w(total, cols);
                assert!(w >= 48.0, "{cols} columnas en {total} dan celdas de {w}");
                // El suelo de 48 px es deliberado y rompe el reparto a
                // propósito; fuera de él, las columnas nunca se salen.
                if w > 48.0 {
                    let usado = w * cols as f32 + GAP * (cols - 1) as f32;
                    assert!(
                        usado <= total + 0.01,
                        "{cols} columnas de {w} ocupan {usado} en un hueco de {total}"
                    );
                }
            }
        }
    }

    #[test]
    fn fit_cols_devuelve_columnas_que_de_verdad_caben() {
        for total in [240.0_f32, 640.0, 1280.0, 1920.0, 3440.0] {
            let n = fit_cols(total, 106.0);
            assert!(n >= 1, "nunca cero columnas");
            let w = cell_w(total, n);
            assert!(
                w >= 106.0 - 0.01,
                "en {total} px caben {n} columnas de {w}, menos del mínimo pedido"
            );
            // Y ni una más: si cupiera otra, `fit_cols` se estaría quedando corto.
            let siguiente = cell_w(total, n + 1);
            assert!(
                siguiente < 106.0,
                "en {total} px cabían {} columnas y solo se pidieron {n}",
                n + 1
            );
        }
    }
}

#[cfg(test)]
mod adjuntos {
    use super::*;
    use std::path::Path;

    #[test]
    fn la_clase_sale_de_la_extension() {
        assert_eq!(AttachKind::of(Path::new("captura.PNG")), AttachKind::Image);
        assert_eq!(AttachKind::of(Path::new("informe.pdf")), AttachKind::Pdf);
        assert_eq!(AttachKind::of(Path::new("lucy_app.log")), AttachKind::Text);
        // Sin extensión se asume texto: un `Dockerfile` o un `.env` son texto,
        // y equivocarse hacia texto solo cuesta un aviso de lectura fallida.
        assert_eq!(AttachKind::of(Path::new("Dockerfile")), AttachKind::Text);
    }

    #[test]
    fn un_adjunto_que_no_se_puede_mandar_dice_por_que() {
        // Se ACEPTA igual. Rechazarlo al soltarlo deja al operador pensando que
        // el arrastre no funciona, y mandarlo vacío es peor: el modelo cree que
        // tiene el fichero.
        let img = Attachment::read(Path::new("foto.jpg"));
        assert_eq!(img.kind, AttachKind::Image);
        assert!(!img.blocked.is_empty(), "una imagen sin ruta de visión avisa");
        assert!(img.content.is_empty());

        let pdf = Attachment::read(Path::new("manual.pdf"));
        assert!(pdf.blocked.contains("extract_pdf_text"), "nombra lo que falta");
    }

    #[test]
    fn un_fichero_que_no_existe_no_rompe_nada() {
        let a = Attachment::read(Path::new("no-existe-este-fichero.txt"));
        assert!(!a.blocked.is_empty());
        assert_eq!(a.name, "no-existe-este-fichero.txt");
    }

    #[test]
    fn el_texto_se_recorta_al_tope() {
        let dir = std::env::temp_dir().join("lucy-egui-test-adjunto.txt");
        std::fs::write(&dir, "á".repeat(ATTACH_MAX_CHARS + 500)).unwrap();
        let a = Attachment::read(&dir);
        assert!(a.blocked.is_empty());
        // Por CARACTERES, no por bytes: con acentos, `á` ocupa dos bytes y
        // cortar por byte partiría uno por la mitad.
        assert_eq!(a.content.chars().count(), ATTACH_MAX_CHARS);
        let _ = std::fs::remove_file(&dir);
    }
}

#[cfg(test)]
mod hilo {
    use super::*;

    #[test]
    fn las_iniciales_salen_del_nombre_real() {
        // En la V2 estaban escritas a mano como "IV": correcto para Iván por
        // casualidad y falso para cualquier otro operador.
        assert_eq!(initials("Iván Eduardo Luna"), "IE");
        assert_eq!(initials("Ada"), "AD");
        assert_eq!(initials("ada lovelace"), "AL");
        // Sin nombre no se inventa uno.
        assert_eq!(initials(""), "U");
        assert_eq!(initials("   "), "U");
    }

    #[test]
    fn el_saludo_lleva_solo_el_nombre_de_pila() {
        // "Buenas tardes, Iván Eduardo Luna" no es un saludo, es un registro.
        let g = greeting("Iván Eduardo Luna");
        assert!(g.ends_with(", Iván"), "salió: {g}");
        // Y sin nombre, saluda igual en vez de dejar una coma colgando.
        assert!(!greeting("").contains(','));
    }
}

#[cfg(test)]
mod paleta {
    use super::*;

    #[test]
    fn el_catalogo_de_comandos_es_el_de_la_v2() {
        // 29, los mismos que `SLASH` en CockpitShell. Recortarlo a lo ya
        // migrado enseñaría una Lucy más pequeña de la que hay: la paleta es
        // una herramienta de descubrimiento antes que un menú.
        assert_eq!(SLASH.len(), 29);
        for (cmd, desc, _) in SLASH {
            assert!(cmd.starts_with('/'), "{cmd} no empieza por barra");
            assert!(!desc.is_empty(), "{cmd} sin descripción no se descubre");
        }
    }

    #[test]
    fn no_hay_comandos_repetidos() {
        // Dos filas iguales en la paleta son dos formas de elegir lo mismo, y
        // la segunda nunca se puede pulsar.
        let mut v: Vec<&str> = SLASH.iter().map(|(c, _, _)| *c).collect();
        v.sort_unstable();
        let n = v.len();
        v.dedup();
        assert_eq!(v.len(), n, "hay un comando duplicado");
    }
}
