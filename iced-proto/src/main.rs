//! Lucy — Fase 0 · prototipo ICED (Elm-like, wgpu + tiny-skia CPU, sin WebView).
//!
//! Mismas dos superficies duras que el proto de egui:
//!   • Izquierda — chat markdown en STREAMING (widget `markdown` nativo de iced).
//!   • Derecha   — TERMINAL PTY viva (el mismo portable-pty de la app real).
//!
//! Ventaja específica de iced para tu caso: el rasterizador `tiny-skia` (CPU puro)
//! como fallback → corre en máquinas SIN GPU / por RDP. Para forzarlo:
//! `set ICED_BACKEND=tiny-skia` antes de ejecutar.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use iced::widget::{
    button, column, container, markdown, row, scrollable, text, text_input,
};
use iced::{time, Element, Length, Subscription, Task, Theme};
use proto_core::{sample_markdown, Pty};
use std::time::Duration;

fn main() -> iced::Result {
    iced::application("Lucy · iced proto", App::update, App::view)
        .subscription(App::subscription)
        .theme(|_| Theme::Dark)
        .run_with(App::new)
}

struct App {
    full: &'static str,
    revealed: usize,
    streaming: bool,
    md_items: Vec<markdown::Item>,
    chat_input: String,
    pty: Option<Pty>,
    term: String,
    term_input: String,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    StartStream,
    ChatInput(String),
    TermInput(String),
    TermSubmit,
    LinkClicked(markdown::Url),
}

impl App {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                full: sample_markdown(),
                revealed: 0,
                streaming: false,
                md_items: Vec::new(),
                chat_input: String::new(),
                pty: Pty::spawn(100, 30).ok(),
                term: String::new(),
                term_input: String::new(),
            },
            Task::none(),
        )
    }

    fn reparse(&mut self) {
        self.md_items = markdown::parse(&self.full[..self.revealed]).collect();
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                // stream reveal (token-ish: a few chars per tick)
                if self.streaming {
                    let mut n = 0;
                    while self.revealed < self.full.len() && n < 3 {
                        self.revealed += 1;
                        while self.revealed < self.full.len()
                            && !self.full.is_char_boundary(self.revealed)
                        {
                            self.revealed += 1;
                        }
                        n += 1;
                    }
                    if self.revealed >= self.full.len() {
                        self.streaming = false;
                    }
                    self.reparse();
                }
                // drain the live PTY
                if let Some(pty) = &self.pty {
                    let chunk = pty.drain();
                    if !chunk.is_empty() {
                        self.term.push_str(&chunk);
                        if self.term.len() > 200_000 {
                            let cut = self.term.len() - 150_000;
                            let cut = (cut..self.term.len())
                                .find(|&i| self.term.is_char_boundary(i))
                                .unwrap_or(0);
                            self.term.drain(..cut);
                        }
                    }
                }
                Task::none()
            }
            Message::StartStream => {
                self.revealed = 0;
                self.streaming = true;
                self.reparse();
                Task::none()
            }
            Message::ChatInput(s) => {
                self.chat_input = s;
                Task::none()
            }
            Message::TermInput(s) => {
                self.term_input = s;
                Task::none()
            }
            Message::TermSubmit => {
                if let Some(pty) = &mut self.pty {
                    let line = std::mem::take(&mut self.term_input);
                    pty.send(&format!("{line}\r"));
                }
                Task::none()
            }
            Message::LinkClicked(_url) => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        // ── left: streaming markdown chat ────────────────────────────────────
        let chat_md = markdown::view(
            &self.md_items,
            markdown::Settings::default(),
            markdown::Style::from_palette(Theme::Dark.palette()),
        )
        .map(Message::LinkClicked);

        let chat = column![
            text("CHAT (markdown en streaming)").size(13),
            row![
                text_input("escribe algo…", &self.chat_input).on_input(Message::ChatInput),
                button("▶ Streaming").on_press(Message::StartStream),
            ]
            .spacing(8),
            scrollable(container(chat_md).padding(10))
                .height(Length::Fill)
                .width(Length::Fill),
        ]
        .spacing(8)
        .width(Length::FillPortion(1));

        // ── right: live terminal ─────────────────────────────────────────────
        let term = column![
            text("TERMINAL (PowerShell · portable-pty)").size(13),
            text_input("comando + Enter…", &self.term_input)
                .on_input(Message::TermInput)
                .on_submit(Message::TermSubmit)
                .font(iced::Font::MONOSPACE),
            scrollable(
                container(text(&self.term).font(iced::Font::MONOSPACE).size(12))
                    .padding(10)
                    .width(Length::Fill)
            )
            .height(Length::Fill),
        ]
        .spacing(8)
        .width(Length::FillPortion(1));

        container(row![chat, term].spacing(12))
            .padding(12)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        // A 16ms heartbeat drives BOTH the stream reveal and the PTY drain.
        // On a native toolkit this simply schedules the next frame — there is no
        // WebView2 compositor to freeze, so it never stalls at idle.
        time::every(Duration::from_millis(16)).map(|_| Message::Tick)
    }
}
