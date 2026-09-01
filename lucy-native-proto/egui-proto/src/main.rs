//! Lucy — Fase 0 · prototipo EGUI (immediate-mode, wgpu/OpenGL nativo, sin WebView).
//!
//! Dos superficies duras lado a lado:
//!   • Izquierda  — chat con markdown en STREAMING (egui_commonmark), revelado
//!                  token a token para medir fidelidad + fluidez.
//!   • Derecha    — TERMINAL PTY viva (el mismo portable-pty de la app real).
//!
//! Qué observar: que las animaciones NO se congelen en reposo (no hace falta
//! mover el mouse), que el markdown se vea bien (tabla/código/lista/cita), y que
//! la terminal reciba salida en vivo. Correr en máquina bloqueada/RDP con
//! `set WGPU_BACKEND=gl` o el backend software para probar sin GPU real.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use proto_core::{sample_markdown, Pty};
use std::time::Instant;

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Lucy · egui proto",
        opts,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(App::new()))
        }),
    )
}

struct App {
    // chat
    md_cache: CommonMarkCache,
    full: &'static str,
    revealed: usize,
    streaming: bool,
    chat_input: String,
    // terminal
    pty: Option<Pty>,
    term: String,
    term_input: String,
    // telemetry
    last: Instant,
    fps: f32,
}

impl App {
    fn new() -> Self {
        Self {
            md_cache: CommonMarkCache::default(),
            full: sample_markdown(),
            revealed: 0,
            streaming: false,
            chat_input: String::new(),
            pty: Pty::spawn(100, 30).ok(),
            term: String::new(),
            term_input: String::new(),
            last: Instant::now(),
            fps: 0.0,
        }
    }

    fn start_stream(&mut self) {
        self.revealed = 0;
        self.streaming = true;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── telemetry: measure real frame cadence ────────────────────────────
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        if dt > 0.0 {
            self.fps = 0.9 * self.fps + 0.1 * (1.0 / dt);
        }

        // ── streaming reveal: a few chars per frame (token-ish) ──────────────
        if self.streaming {
            // advance to the next char boundary a handful of codepoints along
            let mut n = 0;
            while self.revealed < self.full.len() && n < 3 {
                self.revealed += 1;
                while !self.full.is_char_boundary(self.revealed) && self.revealed < self.full.len() {
                    self.revealed += 1;
                }
                n += 1;
            }
            if self.revealed >= self.full.len() {
                self.streaming = false;
            }
        }

        // ── drain the live PTY ───────────────────────────────────────────────
        if let Some(pty) = &self.pty {
            let chunk = pty.drain();
            if !chunk.is_empty() {
                self.term.push_str(&chunk);
                // keep the buffer bounded
                if self.term.len() > 200_000 {
                    let cut = self.term.len() - 150_000;
                    let cut = (cut..self.term.len())
                        .find(|&i| self.term.is_char_boundary(i))
                        .unwrap_or(0);
                    self.term.drain(..cut);
                }
            }
        }

        // Keep producing frames while the PTY is live or we're animating — on a
        // native window this is a plain rAF-style pump, NOT a WebView2 kludge.
        ctx.request_repaint();

        // ── top bar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Lucy · egui");
                ui.separator();
                if ui.button("▶ Simular respuesta (streaming)").clicked() {
                    self.start_stream();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{:.0} FPS", self.fps));
                    ui.separator();
                    ui.label(if self.pty.is_some() { "PTY ●" } else { "PTY ✕" });
                });
            });
        });

        // ── right: live terminal ─────────────────────────────────────────────
        egui::SidePanel::right("term")
            .resizable(true)
            .default_width(500.0)
            .show(ctx, |ui| {
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
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&self.term).monospace().size(12.0),
                            )
                            .wrap(),
                        );
                    });
            });

        // ── center: streaming markdown chat ──────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(egui::RichText::new("CHAT (markdown en streaming)").strong());
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.chat_input)
                    .hint_text("escribe algo y Enter para re-lanzar el streaming…")
                    .desired_width(f32::INFINITY),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.chat_input.clear();
                self.start_stream();
                resp.request_focus();
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.label(egui::RichText::new("Lucy").strong().color(egui::Color32::from_rgb(61, 214, 164)));
                        let shown = &self.full[..self.revealed];
                        CommonMarkViewer::new().show(ui, &mut self.md_cache, shown);
                        if self.streaming {
                            ui.label(egui::RichText::new("▋").color(egui::Color32::from_rgb(61, 214, 164)));
                        }
                    });
                });
        });
    }
}
