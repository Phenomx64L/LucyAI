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
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(App::new()))
        }),
    )
}

#[derive(PartialEq, Clone, Copy)]
enum View {
    Chat,
    Terminal,
    Memoria,
    Sistema,
}

/// Un mensaje del chat real (Ollama).
struct ChatMsg {
    user: bool,
    text: String,
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
    // sistema (métricas live vía lucy_core::system)
    sys: lucy_core::system::SysMonitor,
    sys_last: Instant,
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
            view: View::Chat,
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
            sys: lucy_core::system::SysMonitor::new(),
            sys_last: Instant::now(),
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
        if self.sys_last.elapsed() >= Duration::from_millis(1000) {
            self.sys.refresh(); // el % de CPU es el delta desde el último refresco
            self.sys_last = Instant::now();
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

        // ── left rail ────────────────────────────────────────────────────────
        egui::SidePanel::left("rail")
            .exact_width(150.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.heading("Lucy");
                ui.label(egui::RichText::new("egui · nativo").small().weak());
                ui.separator();
                ui.selectable_value(&mut self.view, View::Chat, "  Chat");
                ui.selectable_value(&mut self.view, View::Terminal, "  Terminal");
                ui.selectable_value(&mut self.view, View::Memoria, "  Memoria");
                ui.selectable_value(&mut self.view, View::Sistema, "  Sistema");
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(format!("{:.0} FPS", self.fps)).small().weak());
                    ui.label(
                        egui::RichText::new(if self.pty.is_some() { "PTY ●" } else { "PTY ✕" })
                            .small()
                            .weak(),
                    );
                    ui.separator();
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Chat => self.chat(ui),
            View::Terminal => self.terminal(ui),
            View::Memoria => self.memoria(ui),
            View::Sistema => self.sistema(ui),
        });
    }
}

impl App {
    fn chat(&mut self, ui: &mut egui::Ui) {
        let accent = egui::Color32::from_rgb(61, 214, 164);
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
                                    .color(egui::Color32::from_rgb(120, 150, 220)),
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

    fn sistema(&mut self, ui: &mut egui::Ui) {
        let s = self.sys.snapshot();
        let accent = egui::Color32::from_rgb(61, 214, 164);
        ui.label(egui::RichText::new("SISTEMA (live · sysinfo)").strong());
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // ── host ──────────────────────────────────────────────────────
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(&s.host).strong().size(16.0).color(accent));
                        ui.label(egui::RichText::new(format!("· {} {}", s.os, s.kernel)).weak());
                    });
                    if !s.cpu_brand.is_empty() {
                        ui.label(egui::RichText::new(&s.cpu_brand).small().weak());
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{} núcleos · uptime {}",
                            s.cores,
                            fmt_uptime(s.uptime_secs)
                        ))
                        .small()
                        .weak(),
                    );
                });
                ui.add_space(6.0);

                // ── CPU ───────────────────────────────────────────────────────
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new("CPU").strong());
                    ui.add(
                        egui::ProgressBar::new((s.cpu_pct / 100.0).clamp(0.0, 1.0))
                            .text(format!("{:.0}%", s.cpu_pct)),
                    );
                    ui.add_space(4.0);
                    let cols = 4usize;
                    egui::Grid::new("cores").num_columns(cols).spacing([8.0, 4.0]).show(ui, |ui| {
                        for (i, c) in s.per_core.iter().enumerate() {
                            ui.add(
                                egui::ProgressBar::new((c / 100.0).clamp(0.0, 1.0))
                                    .desired_width(90.0)
                                    .text(format!("{c:.0}%")),
                            );
                            if (i + 1) % cols == 0 {
                                ui.end_row();
                            }
                        }
                    });
                });
                ui.add_space(6.0);

                // ── memoria ───────────────────────────────────────────────────
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new("Memoria").strong());
                    let frac = if s.mem_total > 0 {
                        s.mem_used as f32 / s.mem_total as f32
                    } else {
                        0.0
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .text(format!("{} / {}", fmt_gb(s.mem_used), fmt_gb(s.mem_total))),
                    );
                    if s.swap_total > 0 {
                        let sf = s.swap_used as f32 / s.swap_total as f32;
                        ui.label(egui::RichText::new("Swap").small().weak());
                        ui.add(
                            egui::ProgressBar::new(sf)
                                .text(format!("{} / {}", fmt_gb(s.swap_used), fmt_gb(s.swap_total))),
                        );
                    }
                });
                ui.add_space(6.0);

                // ── discos ────────────────────────────────────────────────────
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new("Discos").strong());
                    for d in &s.disks {
                        let used = d.total.saturating_sub(d.avail);
                        let frac = if d.total > 0 {
                            used as f32 / d.total as f32
                        } else {
                            0.0
                        };
                        let label = if d.name.trim().is_empty() {
                            d.mount.clone()
                        } else {
                            format!("{} ({})", d.mount, d.name.trim())
                        };
                        ui.label(egui::RichText::new(label).small().weak());
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .text(format!("{} / {}", fmt_gb(used), fmt_gb(d.total))),
                        );
                    }
                });
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
        match &self.mems {
            Err(e) => {
                ui.colored_label(egui::Color32::from_rgb(240, 110, 110), format!("⚠ {e}"));
                ui.label(
                    egui::RichText::new(
                        "Abre Lucy al menos una vez para crear la DB, o corre desde el mismo usuario.",
                    )
                    .weak(),
                );
            }
            Ok(mems) => {
                let q = self.mem_search.to_lowercase();
                ui.add(
                    egui::TextEdit::singleline(&mut self.mem_search)
                        .hint_text("filtrar memorias…")
                        .desired_width(f32::INFINITY),
                );
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
                                    let dots = "●".repeat(m.importance.clamp(1, 3) as usize);
                                    ui.label(
                                        egui::RichText::new(dots)
                                            .color(egui::Color32::from_rgb(61, 214, 164))
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
                                            .color(egui::Color32::from_rgb(120, 140, 200)),
                                    );
                                }
                                ui.label(egui::RichText::new(format!("#{}", m.id)).small().weak());
                            });
                        }
                    });
            }
        }
    }
}
