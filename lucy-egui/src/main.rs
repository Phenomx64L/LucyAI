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

mod theme;

use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use lucy_core::AgentMemory;
use proto_core::Pty;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 760.0]),
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
            Ok(Box::new(App::new()))
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
    /// (glifo, etiqueta) — el sistema de iconos geométricos de Lucy.
    fn label(self) -> (&'static str, &'static str) {
        match self {
            View::Dashboard => ("◱", "Dashboard"),
            View::TerminalIa => ("✦", "Terminal IA"),
            View::NexShell => ("▸", "NexShell"),
            View::LogViewer => ("▤", "Log Viewer"),
            View::Inventario => ("▦", "Inventario"),
            View::Compliance => ("◈", "Compliance"),
            View::Memoria => ("◉", "Memoria"),
            View::Configuracion => ("⚙", "Configuración"),
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
struct ChatMsg {
    user: bool,
    text: String,
}

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

struct App {
    view: View,
    // chat — Ollama local REAL (streaming vía lucy_core::chat)
    md_cache: CommonMarkCache,
    chat_log: Vec<ChatMsg>,
    chat_input: String,
    chat_model: String,
    chat_rx: Option<std::sync::mpsc::Receiver<lucy_core::chat::ChatEvent>>,
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

impl App {
    fn new() -> Self {
        let models = lucy_core::chat::list_models();
        let chat_model = models
            .first()
            .cloned()
            .unwrap_or_else(|| "qwen3:4b".to_string());
        Self {
            view: View::TerminalIa,
            md_cache: CommonMarkCache::default(),
            chat_log: Vec::new(),
            chat_input: String::new(),
            chat_model,
            chat_rx: None,
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
            sys_stamp: String::from("—"),
            last: Instant::now(),
            fps: 0.0,
            last_activity: Instant::now(),
        }
    }

    /// Drena los tokens que va emitiendo el hilo del chat de Ollama.
    fn pump_chat(&mut self) {
        if self.chat_rx.is_none() {
            return;
        }
        let mut done = false;
        if let Some(rx) = &self.chat_rx {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    lucy_core::chat::ChatEvent::Token(t) => {
                        if let Some(last) = self.chat_log.last_mut() {
                            last.text.push_str(&t);
                        }
                    }
                    lucy_core::chat::ChatEvent::Done => {
                        done = true;
                        break;
                    }
                    lucy_core::chat::ChatEvent::Error(e) => {
                        if let Some(last) = self.chat_log.last_mut() {
                            last.text.push_str(&format!("\n\n⚠ {e}"));
                        }
                        done = true;
                        break;
                    }
                }
            }
        }
        if done {
            self.chat_rx = None;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        if dt > 0.0 {
            self.fps = 0.9 * self.fps + 0.1 * (1.0 / dt);
        }

        self.pump_chat();
        // `chat_rx` está en Some mientras corre un stream: eso ES actividad,
        // aunque este frame concreto no traiga token.
        let mut live = self.chat_rx.is_some();
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
            self.refresh_system(false);
        }

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
        egui::TopBottomPanel::top("header")
            .exact_height(44.0)
            .frame(egui::Frame::none().fill(theme::BG2).inner_margin(egui::Margin::symmetric(14.0, 0.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(egui::RichText::new("✦ Lucy").color(theme::ACC).strong().size(15.0));
                    ui.add_space(14.0);
                    let (_, title) = self.view.label();
                    ui.label(egui::RichText::new(title).color(theme::TXT).size(13.5));
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
                    let (glyph, label) = v.label();
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
                    ui.painter().text(
                        egui::pos2(c.x, c.y - 7.0),
                        egui::Align2::CENTER_CENTER,
                        glyph,
                        egui::FontId::proportional(15.0),
                        fg,
                    );
                    ui.painter().text(
                        egui::pos2(c.x, c.y + 11.0),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(9.5),
                        fg,
                    );
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::TerminalIa => self.chat(ui),
            View::NexShell => self.terminal(ui),
            View::Memoria => self.memoria(ui),
            View::Dashboard => self.sistema(ui),
            View::LogViewer => self.log_viewer(ui),
            other => self.pendiente(ui, other),
        });
    }
}

impl App {
    fn chat(&mut self, ui: &mut egui::Ui) {
        let accent = theme::ACC;
        // ── header: título + selector de modelo Ollama ───────────────────────
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("CHAT · Lucy (Ollama local)").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("↻").on_hover_text("Redetectar modelos").clicked() {
                    self.models = lucy_core::chat::list_models();
                }
                egui::ComboBox::from_id_salt("modelo")
                    .selected_text(self.chat_model.clone())
                    .show_ui(ui, |ui| {
                        if self.models.is_empty() {
                            ui.label(egui::RichText::new("(Ollama no responde)").weak());
                        }
                        for m in &self.models {
                            ui.selectable_value(&mut self.chat_model, m.clone(), m);
                        }
                    });
            });
        });
        ui.separator();

        // ── input (arriba, para que el foco quede fijo) ──────────────────────
        let busy = self.chat_rx.is_some();
        let mut do_send = false;
        ui.horizontal(|ui| {
            let resp = ui.add_enabled(
                !busy,
                egui::TextEdit::singleline(&mut self.chat_input)
                    .hint_text("pregúntale a Lucy…  (Enter para enviar)")
                    .desired_width(f32::INFINITY),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                do_send = true;
            }
            if ui.add_enabled(!busy, egui::Button::new("➤")).clicked() {
                do_send = true;
            }
        });
        if do_send && !self.chat_input.trim().is_empty() && !busy {
            let prompt = std::mem::take(&mut self.chat_input);
            self.chat_log.push(ChatMsg { user: true, text: prompt.clone() });
            self.chat_log.push(ChatMsg { user: false, text: String::new() });
            self.chat_rx = Some(lucy_core::chat::start_ollama(self.chat_model.clone(), prompt));
        }
        ui.separator();

        // ── conversación ─────────────────────────────────────────────────────
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.chat_log.is_empty() {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Lucy nativa · sin WebView").heading());
                        ui.label(
                            egui::RichText::new("Escribe arriba y Lucy responde vía Ollama local.")
                                .weak(),
                        );
                    });
                }
                let n = self.chat_log.len();
                for i in 0..n {
                    let is_user = self.chat_log[i].user;
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        if is_user {
                            ui.label(
                                egui::RichText::new("Tú")
                                    .strong()
                                    .color(theme::BLUE),
                            );
                            ui.label(self.chat_log[i].text.clone());
                        } else {
                            ui.label(egui::RichText::new("Lucy").strong().color(accent));
                            let body = self.chat_log[i].text.clone();
                            CommonMarkViewer::new().show(ui, &mut self.md_cache, &body);
                            // cursor de escritura en el último mensaje mientras llega
                            if busy && i == n - 1 {
                                ui.label(egui::RichText::new("▋").color(accent));
                            }
                        }
                    });
                }
            });
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
        }
        if force || self.procs_last.elapsed() >= Duration::from_secs(3) {
            self.procs = self.sys.top_processes(8, self.proc_by_cpu);
            self.procs_last = Instant::now();
        }
        #[cfg(windows)]
        if force || self.svc_last.elapsed() >= Duration::from_secs(30) {
            // Un fallo aquí NO se propaga: lanzar PowerShell puede fallar por
            // política del equipo, y eso no debe vaciar un dashboard cuyo resto
            // de datos es correcto. Se conserva la última lista buena.
            if let Ok(v) = lucy_core::system::down_services(12) {
                self.services = v;
            }
            self.svc_last = Instant::now();
        }
    }

    /// Tarjeta KPI: número grande, unidad pequeña, subtítulo.
    ///
    /// La jerarquía tipográfica ES el diseño de la V2: la cifra domina, el `%`
    /// va a media altura, y el detalle queda debajo en secundario. Un panel de
    /// monitorización se lee de un vistazo o no se lee — todo al mismo tamaño
    /// obliga a buscar el número, que es exactamente lo que no debe pasar.
    fn kpi_card(
        ui: &mut egui::Ui,
        icon: &str,
        title: &str,
        value: &str,
        unit: &str,
        color: egui::Color32,
        sub: &str,
        bar: Option<f32>,
    ) {
        egui::Frame::none()
            .fill(theme::BG2)
            .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
            .rounding(egui::Rounding::same(8.0))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.set_min_width(190.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon).size(12.0).color(theme::TXT3));
                    ui.label(
                        egui::RichText::new(title)
                            .size(9.5)
                            .color(theme::TXT3)
                            .strong(),
                    );
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.label(egui::RichText::new(value).size(30.0).color(color).strong());
                    if !unit.is_empty() {
                        ui.label(egui::RichText::new(unit).size(13.0).color(color));
                    }
                });
                if let Some(frac) = bar {
                    ui.add_space(6.0);
                    ui.add(
                        egui::ProgressBar::new(frac.clamp(0.0, 1.0))
                            .desired_height(4.0)
                            .fill(theme::usage_color(frac * 100.0)),
                    );
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new(sub).size(10.5).color(theme::TXT3));
            });
    }

    fn pendiente(&mut self, ui: &mut egui::Ui, v: View) {
        let (glyph, label) = v.label();
        ui.add_space(48.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(glyph)
                    .size(38.0)
                    .color(theme::TXT3.linear_multiply(0.5)),
            );
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


    /// Dashboard de sistema — el diseño de la V2, no uno inventado.
    fn sistema(&mut self, ui: &mut egui::Ui) {
        let s = self.sys.snapshot();
        let net = self.net;

        // ── cabecera ─────────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Dashboard de sistema").size(17.0).color(theme::TXT));
            ui.add_space(10.0);

            // Salud: derivada de UN criterio, el mismo del helper de umbrales.
            // Y solo los servicios CAÍDOS (código != 0) la degradan — un
            // servicio parado limpio se informa en su tarjeta y no tiñe nada,
            // que es la lección que costó que el equipo pasara a "Atención" en
            // cada arranque.
            let crashed = self.services.iter().filter(|s| s.crashed()).count();
            let worst = s.cpu_pct.max(mem_pct(&s));
            let (sal_txt, sal_col) = if crashed > 0 || worst >= 85.0 {
                ("Atención", theme::AMBER)
            } else {
                ("Saludable", theme::ACC)
            };
            egui::Frame::none()
                .fill(sal_col.linear_multiply(0.12))
                .rounding(egui::Rounding::same(10.0))
                .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("●").size(8.0).color(sal_col));
                    ui.label(egui::RichText::new(sal_txt).size(11.0).color(sal_col));
                });
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("act. {}", self.sys_stamp))
                    .size(10.5)
                    .monospace()
                    .color(theme::TXT3),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("↻").on_hover_text("Actualizar ahora").clicked() {
                    self.refresh_system(true);
                }
            });
        });
        ui.add_space(10.0);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // ── cuatro KPI ───────────────────────────────────────────────────
            ui.horizontal_wrapped(|ui| {
                Self::kpi_card(
                    ui, "▣", "CPU",
                    &format!("{:.0}", s.cpu_pct), "%",
                    theme::usage_color(s.cpu_pct),
                    &format!("{} núcleos", s.cores),
                    None,
                );
                let mp = mem_pct(&s);
                Self::kpi_card(
                    ui, "◈", "RAM",
                    &format!("{mp:.0}"), "%",
                    theme::usage_color(mp),
                    &format!("{} / {}", fmt_gb(s.mem_used), fmt_gb(s.mem_total)),
                    None,
                );
                if let Some(d) = s.disks.first() {
                    let used = d.total.saturating_sub(d.avail);
                    let pct = if d.total > 0 { used as f32 / d.total as f32 * 100.0 } else { 0.0 };
                    Self::kpi_card(
                        ui, "▤", "DISCO SISTEMA",
                        &format!("{pct:.0}"), "%",
                        theme::usage_color(pct),
                        &format!("{} libres de {}", fmt_gb(d.avail), fmt_gb(d.total)),
                        Some(pct / 100.0),
                    );
                }
                Self::kpi_card(
                    ui, "◱", "SISTEMA",
                    &s.host, "",
                    theme::TXT,
                    &format!("{}\nUptime {}", s.os, fmt_uptime(s.uptime_secs)),
                    None,
                );
            });
            ui.add_space(10.0);

            // ── red + servicios ──────────────────────────────────────────────
            ui.horizontal_wrapped(|ui| {
                egui::Frame::none()
                    .fill(theme::BG2)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.set_min_width(260.0);
                        ui.label(egui::RichText::new("◈ RED").size(9.5).color(theme::TXT3).strong());
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("↓").color(theme::ACC).size(15.0));
                            ui.label(
                                egui::RichText::new(fmt_rate(net.rx_bps))
                                    .size(17.0)
                                    .color(theme::TXT)
                                    .strong(),
                            );
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("↑").color(theme::BLUE).size(15.0));
                            ui.label(
                                egui::RichText::new(fmt_rate(net.tx_bps))
                                    .size(17.0)
                                    .color(theme::TXT)
                                    .strong(),
                            );
                        });
                    });

                egui::Frame::none()
                    .fill(theme::BG2)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.set_min_width(320.0);
                        ui.label(
                            egui::RichText::new("◉ SERVICIOS DETENIDOS")
                                .size(9.5)
                                .color(theme::TXT3)
                                .strong(),
                        );
                        ui.add_space(6.0);
                        if self.services.is_empty() {
                            ui.label(
                                egui::RichText::new("✓ Todos los servicios automáticos en ejecución")
                                    .size(11.5)
                                    .color(theme::ACC),
                            );
                        } else {
                            for sv in &self.services {
                                // Caído en ámbar, parado limpio en secundario:
                                // la tarjeta distingue las dos cosas en vez de
                                // teñirlo todo de alarma.
                                let (c, suf) = if sv.crashed() {
                                    (theme::AMBER, format!("  (código {})", sv.exit_code))
                                } else {
                                    (theme::TXT2, String::new())
                                };
                                ui.label(
                                    egui::RichText::new(format!("• {}{}", sv.name, suf))
                                        .size(11.5)
                                        .color(c),
                                );
                            }
                        }
                    });
            });
            ui.add_space(10.0);

            // ── núcleos: tarjetas C0…Cn, como la V2 ──────────────────────────
            ui.label(
                egui::RichText::new(format!("NÚCLEOS {}", s.per_core.len()))
                    .size(9.5)
                    .color(theme::TXT3)
                    .strong(),
            );
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for (i, pct) in s.per_core.iter().enumerate() {
                    egui::Frame::none()
                        .fill(theme::BG2)
                        .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
                        .rounding(egui::Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                        .show(ui, |ui| {
                            ui.set_width(76.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("C{i}"))
                                        .size(10.0)
                                        .monospace()
                                        .color(theme::TXT3),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(format!("{pct:.0}%"))
                                                .size(11.5)
                                                .color(theme::TXT)
                                                .strong(),
                                        );
                                    },
                                );
                            });
                            ui.add_space(3.0);
                            ui.add(
                                egui::ProgressBar::new((pct / 100.0).clamp(0.0, 1.0))
                                    .desired_height(3.0)
                                    .fill(theme::usage_color(*pct)),
                            );
                        });
                }
            });
            ui.add_space(12.0);

            // ── discos ───────────────────────────────────────────────────────
            ui.label(
                egui::RichText::new(format!("DISCOS {} VOLÚMENES", s.disks.len()))
                    .size(9.5)
                    .color(theme::TXT3)
                    .strong(),
            );
            ui.add_space(6.0);
            for d in &s.disks {
                let used = d.total.saturating_sub(d.avail);
                let frac = if d.total > 0 { used as f32 / d.total as f32 } else { 0.0 };
                egui::Frame::none()
                    .fill(theme::BG2)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BDR))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&d.mount)
                                    .monospace()
                                    .size(12.0)
                                    .color(theme::TXT),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:.0}%", frac * 100.0))
                                            .size(12.0)
                                            .color(theme::usage_color(frac * 100.0))
                                            .strong(),
                                    );
                                },
                            );
                        });
                        ui.add_space(4.0);
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .desired_height(5.0)
                                .fill(theme::usage_color(frac * 100.0)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "{} libres · {} / {}",
                                fmt_gb(d.avail),
                                fmt_gb(used),
                                fmt_gb(d.total)
                            ))
                            .size(10.5)
                            .monospace()
                            .color(theme::TXT3),
                        );
                    });
                ui.add_space(6.0);
            }
            ui.add_space(6.0);

            // ── top procesos ─────────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("TOP PROCESOS").size(9.5).color(theme::TXT3).strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Cambiar de criterio recarga la lista al instante: ordenar
                    // la que ya está en pantalla mostraría el top-8 por RAM
                    // reordenado por CPU, que no es el top-8 por CPU.
                    if ui.selectable_label(self.proc_by_cpu, "CPU").clicked() && !self.proc_by_cpu {
                        self.proc_by_cpu = true;
                        self.procs = self.sys.top_processes(8, true);
                    }
                    if ui.selectable_label(!self.proc_by_cpu, "RAM").clicked() && self.proc_by_cpu {
                        self.proc_by_cpu = false;
                        self.procs = self.sys.top_processes(8, false);
                    }
                });
            });
            ui.add_space(4.0);
            egui::Grid::new("procs")
                .num_columns(4)
                .spacing([18.0, 5.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["PROCESO", "CPU", "RAM", "PID"] {
                        ui.label(egui::RichText::new(h).size(9.5).color(theme::TXT3).strong());
                    }
                    ui.end_row();
                    for p in &self.procs {
                        ui.label(egui::RichText::new(&p.name).size(11.5).monospace().color(theme::TXT2));
                        ui.label(
                            egui::RichText::new(format!("{:.0}%", p.cpu_pct))
                                .size(11.5)
                                .color(theme::usage_color(p.cpu_pct)),
                        );
                        ui.label(
                            egui::RichText::new(fmt_gb(p.mem_bytes))
                                .size(11.5)
                                .color(theme::ACC),
                        );
                        ui.label(
                            egui::RichText::new(p.pid.to_string())
                                .size(11.5)
                                .monospace()
                                .color(theme::TXT3),
                        );
                        ui.end_row();
                    }
                });
            ui.add_space(10.0);
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
                                                .color(theme::usage_color(h.score * 100.0)),
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
