//! Cumplimiento: comprobaciones, postura, exportacion y remediacion.
//!
//! ── POR QUE ESTA AQUI Y NO EN `main.rs` ──────────────────────────────────────
//!
//! `main.rs` llego a 24.912 lineas con un solo `impl App` de 14.240. El nucleo
//! esta repartido en 55 ficheros y el mayor son 1.609, asi que no es el estilo de
//! la casa: es deuda del shell. Y no es estetica — a ese tamaño una busqueda
//! devuelve el mismo fragmento en tres sitios y una edicion no sabe a cual iba.
//!
//! ESTE MODULO ES UN CORTE Y PEGA. Los metodos se movieron ENTEROS, con sus
//! comentarios y sin tocar una linea de logica; la suite verifica que nada
//! cambio. Lo que lo hace posible sin abrir visibilidades es que `App` se declara
//! en la raiz del crate: en Rust un campo privado se ve desde el modulo que lo
//! declara Y DESDE SUS DESCENDIENTES.

use super::*;

impl App {
    /// Qué ha cambiado desde el escaneo anterior.
    ///
    /// ENCIMA DE LA LISTA Y NO EN UNA PESTAÑA. Es la única parte del informe que
    /// no estaba ayer, y por eso es lo primero que hay que leer: el resto de la
    /// pantalla dice el estado, y el estado casi siempre es el mismo que la
    /// última vez. Escondida detrás de una pestaña, nadie la abriría — y lo que
    /// no se abre es como si no estuviera.
    ///
    /// NO SALE NADA cuando no hay con qué comparar o cuando no cambió nada. Una
    /// franja que dice «sin cambios» es una fila de interfaz cuyo único mensaje
    /// es que no tiene mensaje, y con el tiempo se deja de mirar — junto con las
    /// veces que sí trae algo.
    pub(crate) fn cmp_desde_la_ultima(&mut self, ui: &mut egui::Ui) {
        use lucy_core::posture::Cambio;
        let Some((ts, filas)) = &self.cmp_cambios else { return };
        ui.add_space(8.0);
        let cuando = hace_cuanto(ahora_epoch() - *ts);
        section(ui, "Desde el escaneo anterior", Some(cuando));
        for f in filas {
            row_align(ui, 22.0, egui::Align::Center, |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // EL COLOR LO PONE LA DIRECCIÓN DEL CAMBIO, no la severidad del
                // control. Aquí lo que se lee es «esto ha empeorado» o «esto se
                // ha arreglado»; la severidad ya ordena la lista y sale en la
                // tabla de abajo.
                let (glifo, color, que) = match f.cambio {
                    Cambio::Rompe => ("▼", theme::red(), "ha dejado de cumplir"),
                    Cambio::Arregla => ("▲", theme::acc(), "ya cumple"),
                    Cambio::SeDejaDeMedir => ("?", theme::amber(), "ya no se puede medir"),
                    Cambio::VuelveAMedirse => ("?", theme::txt3(), "vuelve a medirse"),
                    Cambio::Nuevo => ("+", theme::txt3(), "control nuevo"),
                };
                ui.label(egui::RichText::new(glifo).color(color).size(theme::FS_CAPTION));
                ui.label(
                    egui::RichText::new(&f.titulo)
                        .size(theme::FS_FOOTNOTE)
                        .color(theme::txt2()),
                );
                ui.label(
                    egui::RichText::new(i18n::tr(que))
                        .size(theme::FS_CAPTION)
                        .color(theme::faint()),
                );
            });
        }
        ui.add_space(4.0);
    }

    pub(crate) fn cmp_catalogo(&self) -> Vec<lucy_core::compliance::Check> {
        match self.remote_hosts.iter().find(|h| h.id == self.cmp_host) {
            Some(h) => lucy_core::compliance::catalogo_de(h).unwrap_or_default(),
            None => lucy_core::compliance::catalogo(true),
        }
    }

    pub(crate) fn cmp_nombre(&self) -> String {
        if self.cmp_host.is_empty() {
            lucy_core::system::hostname()
        } else {
            self.remote_hosts
                .iter()
                .find(|h| h.id == self.cmp_host)
                .map(|h| h.name.clone())
                .unwrap_or_else(|| i18n::tr("Equipo").into())
        }
    }

    pub(crate) fn cmp_cabecera(&mut self, ui: &mut egui::Ui) {
        let mut escanear = false;
        let mut parar = false;
        let corriendo = self.cmp_rx.is_some();
        row_align(ui, 30.0, egui::Align::Center, |ui| {
            titulo_modulo(ui, View::Compliance);
            ui.add_space(10.0);
            // La pastilla dice NORMA, EQUIPO Y CUÁNTOS controles. Los tres hacen
            // falta: sin la norma no se sabe contra qué se mide, y sin el número
            // no se sabe si la lista de abajo está completa.
            let n = self.cmp_catalogo().len();
            let etiqueta = format!("CIS · {} · {n} checks", self.cmp_nombre());
            let boton = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("⛉ {etiqueta}"))
                        .size(theme::FS_FOOTNOTE)
                        .color(theme::txt3()),
                )
                .fill(theme::bg3())
                .stroke(egui::Stroke::new(1.0_f32, theme::bdr()))
                .rounding(egui::Rounding::same(theme::R_SM)),
            );
            if boton.clicked() {
                self.cmp_host_menu = !self.cmp_host_menu;
            }
            if self.cmp_host_menu {
                let mut elegido: Option<String> = None;
                egui::Area::new(egui::Id::new("cmp-host-menu"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(boton.rect.left_bottom() + egui::vec2(0.0, 6.0))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::none()
                            .fill(theme::bg3())
                            .stroke(egui::Stroke::new(1.0_f32, theme::bdr2()))
                            .rounding(egui::Rounding::same(theme::R_LG))
                            .inner_margin(egui::Margin::same(6.0))
                            .show(ui, |ui| {
                                ui.set_min_width(220.0);
                                if lv_opcion(ui, i18n::tr("Este equipo"), "local", self.cmp_host.is_empty()) {
                                    elegido = Some(String::new());
                                }
                                for h in
                                    self.remote_hosts.iter().filter(|h| h.protocol.can_shell())
                                {
                                    let tipo = if h.protocol
                                        == lucy_core::hosts::Protocol::Winrm
                                    {
                                        "WinRM"
                                    } else {
                                        "SSH"
                                    };
                                    if lv_opcion(ui, &h.name, tipo, self.cmp_host == h.id) {
                                        elegido = Some(h.id.clone());
                                    }
                                }
                            });
                    });
                if let Some(id) = elegido {
                    self.cmp_host_menu = false;
                    if id != self.cmp_host {
                        self.cmp_host = id;
                        // Los resultados eran de OTRO equipo. Una lista de checks
                        // bajo el nombre de una máquina que no se midió es la
                        // peor mentira que puede contar un panel de cumplimiento.
                        self.cmp_rs.clear();
                        self.cmp_error.clear();
                        self.cmp_last.clear();
                        self.cmp_abierto.clear();
                        self.cmp_filtro = None;
                    }
                } else if ui.input(|i| i.pointer.any_click()) && !boton.clicked() {
                    let dentro = ui.ctx().pointer_latest_pos().is_some_and(|p| {
                        ui.ctx()
                            .memory(|m| m.area_rect(egui::Id::new("cmp-host-menu")))
                            .is_some_and(|r| r.contains(p))
                    });
                    if !dentro {
                        self.cmp_host_menu = false;
                    }
                }
            }
            ui.add_space(10.0);
            if let Some(t0) = self.cmp_desde {
                ui.label(
                    egui::RichText::new(i18n::trf(
                        "escaneando… {s}s",
                        &[("s", &t0.elapsed().as_secs().to_string())],
                    ))
                        .size(theme::FS_CAPTION)
                        .color(theme::amber()),
                );
            } else if !self.cmp_last.is_empty() {
                ui.label(
                    egui::RichText::new(i18n::trf(
                        "● ESCANEADO {hora}",
                        &[("hora", &self.cmp_last)],
                    ))
                        .size(theme::FS_CAPTION)
                        .monospace()
                        .color(theme::acc()),
                );
            }
            right(ui, 30.0, |ui| {
                let b = egui::Button::new(
                    egui::RichText::new(i18n::tr(if corriendo { "■  Parar" } else { "⛨  Escanear" }))
                        .size(theme::FS_CAPTION)
                        .color(if corriendo { theme::txt() } else { theme::acc_ink() }),
                )
                .fill(if corriendo { theme::bg4() } else { theme::acc() })
                .stroke(egui::Stroke::NONE)
                .rounding(egui::Rounding::same(theme::R_SM))
                .min_size(egui::vec2(108.0, 26.0));
                if ui.add(b).clicked() {
                    if corriendo {
                        parar = true;
                    } else {
                        escanear = true;
                    }
                }
                ui.add_space(6.0);
                let hay = !self.cmp_rs.is_empty();
                if ghost_icon(ui, icons::Icon::Copy)
                    .on_hover_text(i18n::tr(if hay {
                        "Copiar el informe en CSV"
                    } else {
                        "Nada que copiar todavía"
                    }))
                    .clicked()
                    && hay
                {
                    let t = self.cmp_csv();
                    ui.ctx().copy_text(t);
                }
            });
        });
        if parar {
            self.cmp_stop.store(true, std::sync::atomic::Ordering::Relaxed);
            self.cmp_rx = None;
            self.cmp_desde = None;
            self.cmp_error = i18n::tr("Revisión detenida.").into();
        }
        if escanear {
            self.cmp_escanear();
        }
    }

    pub(crate) fn cmp_resumen(&mut self, ui: &mut egui::Ui) {
        use lucy_core::compliance::Estado;
        let (pct, sin_medir) = lucy_core::compliance::porcentaje(&self.cmp_rs);
        let cuenta = |e: Estado| self.cmp_rs.iter().filter(|r| r.estado == e).count();
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            // El anillo. La cifra grande dentro y el arco alrededor dicen lo
            // mismo, y eso es a propósito: el arco se ve de reojo desde lejos y el
            // número aguanta una captura de pantalla.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(150.0, 110.0), egui::Sense::hover());
            let c = rect.center();
            let r = 42.0_f32;
            let p = ui.painter();
            p.circle_stroke(c, r, egui::Stroke::new(9.0_f32, theme::bg4()));
            // El arco, a trocitos: egui no dibuja arcos, y una polilínea de
            // sesenta puntos es indistinguible de uno a este tamaño.
            let frac = (pct as f32 / 100.0).clamp(0.0, 1.0);
            if frac > 0.0 {
                let pasos = (60.0 * frac).ceil() as usize;
                let pts: Vec<egui::Pos2> = (0..=pasos)
                    .map(|i| {
                        let t = -std::f32::consts::FRAC_PI_2
                            + (i as f32 / 60.0) * std::f32::consts::TAU;
                        egui::pos2(c.x + r * t.cos(), c.y + r * t.sin())
                    })
                    .collect();
                p.add(egui::Shape::line(pts, egui::Stroke::new(9.0_f32, theme::acc())));
            }
            p.text(
                egui::pos2(c.x, c.y - 6.0),
                egui::Align2::CENTER_CENTER,
                format!("{pct}%"),
                egui::FontId::proportional(26.0),
                theme::txt(),
            );
            p.text(
                egui::pos2(c.x, c.y + 16.0),
                egui::Align2::CENTER_CENTER,
                "CONFORME",
                egui::FontId::proportional(theme::FS_CAPTION),
                theme::txt3(),
            );

            let ancho = ((ui.available_width() - 24.0) / 3.0).max(120.0);
            // UN CERO NO SE PINTA DE ALARMA. La tarjeta de fallas llevaba franja
            // roja con un `0` dentro: cero fallas es la mejor noticia del panel
            // y salía con el color de la peor. Con la cuenta a cero la franja se
            // apaga, y el rojo queda para cuando hay algo rojo que contar.
            let tono = |n: usize, c: egui::Color32| if n == 0 { theme::bdr2() } else { c };
            cmp_tarjeta(ui, ancho, cuenta(Estado::Pasa), "CONFORMES", theme::acc());
            ui.add_space(8.0);
            let n_av = cuenta(Estado::Aviso);
            cmp_tarjeta(ui, ancho, n_av, "AVISOS", tono(n_av, theme::amber()));
            ui.add_space(8.0);
            let n_fa = cuenta(Estado::Falla);
            cmp_tarjeta(ui, ancho, n_fa, "FALLAS", tono(n_fa, theme::red()));
        });
        // Los que no se pudieron medir van FUERA del porcentaje y se dicen aquí.
        // Meterlos en el denominador hundiría la nota por un permiso; dejarlos
        // callados haría que un 100 % sobre cinco checks pareciera un 100 % sobre
        // veinte.
        if sin_medir > 0 {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(i18n::trf(
                    "⚠ {sin} de {total} no se pudieron medir y quedan fuera del \
                     porcentaje — el motivo está en cada fila.",
                    &[
                        ("sin", &sin_medir.to_string()),
                        ("total", &self.cmp_rs.len().to_string()),
                    ],
                ))
                .size(theme::FS_CAPTION)
                .color(theme::amber()),
            );
        }
    }

    pub(crate) fn cmp_chips(&mut self, ui: &mut egui::Ui) {
        use lucy_core::compliance::Estado;
        let cuenta = |e: Estado| self.cmp_rs.iter().filter(|r| r.estado == e).count();
        ui.add_space(10.0);
        row_align(ui, 28.0, egui::Align::Center, |ui| {
            let opciones: [(&str, usize, Option<Estado>); 5] = [
                ("Todos", self.cmp_rs.len(), None),
                ("Conformes", cuenta(Estado::Pasa), Some(Estado::Pasa)),
                ("Avisos", cuenta(Estado::Aviso), Some(Estado::Aviso)),
                ("Fallas", cuenta(Estado::Falla), Some(Estado::Falla)),
                ("Sin medir", cuenta(Estado::Error), Some(Estado::Error)),
            ];
            for (label, n, e) in opciones {
                if lv_chip(ui, i18n::tr(label), n, self.cmp_filtro == e) {
                    self.cmp_filtro = e;
                }
                ui.add_space(6.0);
            }
        });
    }

    pub(crate) fn cmp_lista(&mut self, ui: &mut egui::Ui) {
        use lucy_core::compliance::Estado;
        let visibles: Vec<usize> = self
            .cmp_rs
            .iter()
            .enumerate()
            .filter(|(_, r)| self.cmp_filtro.is_none_or(|e| r.estado == e))
            .map(|(i, _)| i)
            .collect();
        if visibles.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(i18n::tr("Ninguno en este estado."))
                        .size(theme::FS_FOOTNOTE)
                        .color(theme::faint()),
                );
            });
            return;
        }
        let mut alternar: Option<String> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("cmp-lista")
            .show(ui, |ui| {
                for i in visibles {
                    let r = &self.cmp_rs[i];
                    let (marca, col) = match r.estado {
                        Estado::Pasa => ("✓", theme::acc()),
                        Estado::Aviso => ("!", theme::amber()),
                        Estado::Falla => ("✕", theme::red()),
                        Estado::Error => ("?", theme::txt3()),
                    };
                    let abierto = self.cmp_abierto.contains(&r.check.id);
                    ui.add_space(6.0);
                    egui::Frame::none()
                        .fill(theme::bg3())
                        .rounding(egui::Rounding::same(theme::R_SM))
                        .inner_margin(egui::Margin::symmetric(12.0, 9.0))
                        .show(ui, |ui| {
                            // La barra de color a la izquierda: es lo que permite
                            // recorrer treinta filas de un vistazo sin leer una
                            // sola palabra.
                            let borde = ui.max_rect();
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    borde.left_top() - egui::vec2(12.0, 9.0),
                                    egui::vec2(3.0, borde.height() + 18.0),
                                ),
                                egui::Rounding::same(2.0),
                                col,
                            );
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(marca).size(14.0).color(col));
                                ui.add_space(6.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(&r.check.title)
                                            .size(theme::FS_FOOTNOTE)
                                            .color(theme::txt()),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(&r.check.id)
                                                .monospace()
                                                .size(theme::FS_CAPTION)
                                                .color(theme::faint()),
                                        );
                                        ui.label(
                                            egui::RichText::new(&r.check.category)
                                                .size(theme::FS_CAPTION)
                                                .color(theme::txt3()),
                                        );
                                        // LA REMEDIACIÓN, EN LA FILA. Es lo que
                                        // se hace después de leer que algo falla,
                                        // y esconderla tras un clic convierte
                                        // «arreglar siete cosas» en catorce.
                                        if r.estado != Estado::Pasa
                                            && !r.check.remediation.is_empty()
                                        {
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "↳ {}",
                                                    r.check.remediation
                                                ))
                                                .monospace()
                                                .size(theme::FS_CAPTION)
                                                .color(theme::txt3()),
                                            );
                                        }
                                    });
                                });
                                right(ui, 20.0, |ui| {
                                    if ui
                                        .add(egui::Label::new(
                                            egui::RichText::new(if abierto { "▴" } else { "▾" })
                                                .size(theme::FS_CAPTION)
                                                .color(theme::txt3()),
                                        )
                                        .sense(egui::Sense::click()))
                                        .on_hover_text(i18n::tr("Ver la evidencia"))
                                        .clicked()
                                    {
                                        alternar = Some(r.check.id.clone());
                                    }
                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new(i18n::tr(r.estado.label()))
                                            .size(theme::FS_CAPTION)
                                            .color(col),
                                    );
                                    ui.add_space(10.0);
                                    // LA SEVERIDAD SOLO SE TIÑE SI EL CONTROL NO
                                    // PASA. Se teñía siempre, así que un control
                                    // CONFORME enseñaba «crítica» en rojo al
                                    // lado de su propio visto verde: seis filas
                                    // donde no pasa nada, gritando. Y cada rojo
                                    // de más resta al rojo que sí importa —
                                    // exactamente lo que dice la tarjeta del
                                    // Dashboard sobre no teñir la cifra.
                                    //
                                    // La severidad de un control que pasa no es
                                    // una advertencia: es una etiqueta que dice
                                    // cuánto habría dolido. Sigue estando, en
                                    // gris, porque ordena la lista y explica por
                                    // qué un fallo es «aviso» y no «falla».
                                    ui.label(
                                        egui::RichText::new(i18n::tr(r.check.severity.label()))
                                            .size(theme::FS_CAPTION)
                                            .color(
                                                if r.estado == lucy_core::compliance::Estado::Pasa {
                                                    theme::txt3()
                                                } else {
                                                    match r.check.severity {
                                                        lucy_core::compliance::Severidad::Critical => {
                                                            theme::red()
                                                        }
                                                        lucy_core::compliance::Severidad::High => {
                                                            theme::amber()
                                                        }
                                                        _ => theme::txt3(),
                                                    }
                                                },
                                            ),
                                    );
                                });
                            });
                            if abierto {
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(format!("$ {}", r.check.command))
                                        .monospace()
                                        .size(theme::FS_CAPTION)
                                        .color(theme::faint()),
                                );
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(if r.evidencia.trim().is_empty() {
                                        i18n::tr("(el comando no devolvió nada)").to_string()
                                    } else {
                                        r.evidencia.clone()
                                    })
                                    .monospace()
                                    .size(theme::FS_CAPTION)
                                    .color(theme::txt2()),
                                );
                            }
                        });
                }
            });
        if let Some(id) = alternar {
            if !self.cmp_abierto.remove(&id) {
                self.cmp_abierto.insert(id);
            }
        }
    }

    /// El informe en CSV. La evidencia va entera: es la prueba.
    pub(crate) fn cmp_csv(&self) -> String {
        let mut s = String::from("id,titulo,categoria,gravedad,estado,evidencia,remediacion\n");
        for r in &self.cmp_rs {
            let q = |x: &str| {
                if x.contains([',', '"', '\n']) {
                    format!("\"{}\"", x.replace('"', "\"\""))
                } else {
                    x.to_string()
                }
            };
            s.push_str(&format!(
                "{},{},{},{},{},{},{}\n",
                q(&r.check.id),
                q(&r.check.title),
                q(&r.check.category),
                r.check.severity.label(),
                r.estado.label(),
                q(r.evidencia.trim()),
                q(&r.check.remediation),
            ));
        }
        s
    }

    pub(crate) fn cmp_escanear(&mut self) {
        if self.cmp_rx.is_some() {
            return;
        }
        self.cmp_error.clear();
        let host = if self.cmp_host.is_empty() {
            None
        } else {
            match self.remote_hosts.iter().find(|h| h.id == self.cmp_host) {
                Some(h) => Some(h.clone()),
                None => {
                    self.cmp_error = i18n::tr("Ese equipo ya no está dado de alta.").into();
                    return;
                }
            }
        };
        self.cmp_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = self.cmp_stop.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let r = match host {
                Some(h) => match lucy_core::compliance::catalogo_de(&h) {
                    Ok(cs) => {
                        let pw = lucy_core::hosts::password(&h.id).unwrap_or_default();
                        lucy_core::compliance::run_remote(&h, &pw, &cs, &stop)
                    }
                    Err(e) => Err(e),
                },
                None => lucy_core::compliance::run_local(&lucy_core::compliance::catalogo(true)),
            };
            let _ = tx.send(r);
        });
        self.cmp_rx = Some((self.cmp_host.clone(), rx));
        self.cmp_desde = Some(Instant::now());
    }
}
