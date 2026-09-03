//! Espacio de trabajo: pestanas, forks y su estado.
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
    /// El panel derecho: los cuatro carriles del agente.
    /// El botón de plegar el carril. Devuelve `true` si se ha pulsado.
    ///
    /// A LA IZQUIERDA DEL TODO Y ARRIBA, pegado al borde que separa el carril de
    /// la conversación: es donde está la mano después de leer el plan, y es el
    /// lado por el que el panel se va a ir.
    pub(crate) fn ws_cabecera_plegar(&mut self, ui: &mut egui::Ui) -> bool {
        let mut pulsado = false;
        let ancho = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(ancho, 0.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let (r, resp) =
                    ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                let c = if resp.hovered() { theme::txt() } else { theme::faint() };
                // APUNTA A DONDE SE VA EL CARRIL. Llevaba el chevrón de ABAJO
                // mientras el panel se pliega hacia la DERECHA: una flecha que
                // señala un sitio y hace otra cosa promete un desplegable, y lo
                // que hace es llevarse medio panel.
                icons::draw(ui.painter(), icons::Icon::ChevronRight, r.center(), 13.0, c);
                if resp
                    .on_hover_text(i18n::tr("Plegar el carril — vuelve con el botón de la cabecera"))
                    .clicked()
                {
                    pulsado = true;
                }
            },
        );
        pulsado
    }

    pub(crate) fn ws_plan(&mut self, ui: &mut egui::Ui) {
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
        // LO QUE YA FALLÓ AQUÍ ANTES. Se resuelve fuera del bucle porque dentro
        // `self` está prestado, y se cachea porque esto se repinta sesenta veces
        // por segundo. Solo los pendientes: en uno que ya corrió, el resultado
        // de verdad está a la vista y un historial al lado sobra.
        let pendientes: Vec<(String, String)> = self.tabs[self.tab]
            .ws
            .plan
            .iter()
            .filter(|s| s.status == StepStatus::Pending)
            .map(|s| (s.detail.clone(), s.host.clone()))
            .collect();
        for (cmd, host) in pendientes {
            let clave = format!("{host}\u{1}{cmd}");
            // `entry` y no `contains_key` + `insert`: son dos búsquedas en el
            // mapa por cada paso pendiente y en cada fotograma. La consulta solo
            // corre cuando la clave falta, igual que antes.
            self.fallos.entry(clave).or_insert_with(|| {
                lucy_core::audit::fallos_recientes(&cmd, &host, lucy_core::audit::DIAS_FALLOS)
                    .unwrap_or(0)
            });
        }
        // `(id, comando, elevado)`.
        let mut aprobado: Option<(String, String, bool)> = None;

        for s in &self.tabs[self.tab].ws.plan {
            let (glyph, col) = match s.status {
                StepStatus::Done => ("✓", theme::acc()),
                StepStatus::Running => ("▸", theme::acc()),
                StepStatus::Error => ("✕", theme::red()),
                StepStatus::Pending => ("○", theme::disabled()),
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
                                theme::txt3()
                            } else {
                                theme::txt()
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
                                .color(theme::txt2()),
                        );
                    }
                    // Por qué ESTE paso no lo va a dar Lucy sola.
                    //
                    // Sin esta línea, el automático encendido y parado se ve
                    // igual que el automático que todavía no ha empezado: el
                    // operador espera a que siga, y no va a seguir. Decirlo aquí
                    // —pegado al comando, no en el trace— es lo que convierte
                    // una pausa en una decisión que se puede tomar.
                    if let Some(motivo) = &s.needs_human {
                        ui.add_space(3.0);
                        row(ui, 15.0, |ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            icons::show(ui, icons::Icon::Shield, 12.0, theme::amber());
                            ui.label(
                                egui::RichText::new(motivo)
                                    .size(theme::FS_MICRO)
                                    .color(theme::amber()),
                            );
                        });
                    }
                    // ESTO YA FALLÓ AQUÍ. La señal llevaba versiones en disco
                    // —`exit_code` se escribe en cada fila, con índice por fecha
                    // y por equipo— y no la agregaba nadie: el visor enseña una
                    // lista cronológica, que contesta «qué pasó el martes» y no
                    // «esto viene fallando». Así que Lucy podía proponer por
                    // tercera vez un comando que ya había fallado dos, y el
                    // operador aprobarlo sin más pista que su memoria.
                    //
                    // Va DELANTE del botón a propósito: un aviso que aparece
                    // debajo de lo que hay que pulsar se lee después de pulsar.
                    if s.status == StepStatus::Pending {
                        let clave = format!("{}\u{1}{}", s.host, s.detail);
                        if let Some(&n) = self.fallos.get(&clave).filter(|&&n| n > 0) {
                            ui.add_space(3.0);
                            row(ui, 15.0, |ui| {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                // El mismo glifo con el que este panel marca un
                                // paso fallido: es literalmente lo que dice.
                                ui.label(
                                    egui::RichText::new("✕")
                                        .size(theme::FS_MICRO)
                                        .color(theme::red()),
                                );
                                ui.label(
                                    egui::RichText::new(if n == 1 {
                                        i18n::tr("Este mismo comando ya falló aquí una vez").to_string()
                                    } else {
                                        i18n::trf(
                                            "Este mismo comando ya falló aquí {n} veces",
                                            &[("n", &n.to_string())],
                                        )
                                    })
                                    .size(theme::FS_MICRO)
                                    .color(theme::red()),
                                );
                            });
                        }
                    }
                    // El botón SOLO existe en los pasos pendientes. Con el
                    // automático apagado —que es como viene— nada corre sin que
                    // alguien lo pulse, y esa persona leyendo el comando ERA el
                    // guardrail. Encendido, el guardrail es `lucy_core::guard` y
                    // este botón queda para lo que él manda mirar.
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
                                    egui::RichText::new(i18n::tr("⇈ Reintentar como administrador"))
                                        .size(theme::FS_CAPTION)
                                        .color(theme::amber()),
                                )
                                .fill(theme::amber_bg())
                                .stroke(egui::Stroke::new(
                                    1.0_f32,
                                    theme::amber().linear_multiply(0.4),
                                ))
                                .rounding(egui::Rounding::same(theme::R_XS))
                                .min_size(egui::vec2(0.0, 22.0));
                                if ui
                                    .add_enabled(!busy, b)
                                    .on_hover_text(i18n::tr("Windows pedirá confirmación (UAC)"))
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
                                        // SIN EL SALTO NI LA SANGRÍA que llevaba
                                        // dentro. El literal estaba partido en
                                        // dos líneas SIN la barra de
                                        // continuación, así que la cadena
                                        // contenía un salto y cuarenta y un
                                        // espacios de verdad: se pintaba con un
                                        // escalón enorme en medio de la frase.
                                        "Lucy ya corre como administrador: esto no es un \
                                         problema de privilegios.",
                                    )
                                    .size(theme::FS_CAPTION)
                                    .color(theme::faint()),
                                );
                            }
                            // Cuenta estándar y consentimiento apagado: no hay
                            // mecanismo que pedir. Decirlo es más útil que un
                            // botón que no puede funcionar.
                            Elevation::Unavailable => {
                                ui.label(
                                    egui::RichText::new(
                                        "Sin privilegios y con UAC desactivado: hay que \
                                         abrir Lucy con una cuenta de administrador.",
                                    )
                                    .size(theme::FS_CAPTION)
                                    .color(theme::amber()),
                                );
                            }
                        }
                    }
                    if s.status == StepStatus::Pending && !s.detail.is_empty() {
                        ui.add_space(4.0);
                        let b = egui::Button::new(
                            egui::RichText::new(i18n::tr("▸ Ejecutar"))
                                .size(theme::FS_CAPTION)
                                .color(theme::acc_ink()),
                        )
                        .fill(theme::acc())
                        .stroke(egui::Stroke::NONE)
                        .rounding(egui::Rounding::same(theme::R_XS))
                        .min_size(egui::vec2(0.0, 22.0));
                        if ui
                            .add_enabled(!busy, b)
                            .on_hover_text(i18n::tr("Correr este comando en este equipo"))
                            .clicked()
                        {
                            aprobado = Some((s.id.clone(), s.detail.clone(), false));
                        }
                    }
                });
            });
        }
        if let Some((id, cmd, elev)) = aprobado {
            // Lo propuso Lucy y lo sancionó una persona con el botón.
            self.run_step(self.tab, id, cmd, elev, "ai");
        }
        // Los forks van DESPUÉS del plan y fuera de su estado vacío: con un
        // sub-agente corriendo el panel no está vacío, solo no tiene plan.
        if !self.tabs[self.tab].ws.forks.is_empty() {
            ui.add_space(10.0);
            ui.add(egui::Label::new(theme::instrument_label(
                "Sub-agentes",
                theme::faint(),
            )));
            for f in &self.tabs[self.tab].ws.forks {
                let (txt, col) = match f.status {
                    ForkStatus::Running => ("en curso", theme::acc()),
                    ForkStatus::Done => ("terminado", theme::txt3()),
                    ForkStatus::Error => ("error", theme::red()),
                    ForkStatus::Collected => ("recogido", theme::faint()),
                };
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("⇉").size(11.0).color(col));
                    ui.label(
                        egui::RichText::new(&f.id)
                            .size(theme::FS_CAPTION)
                            .monospace()
                            .color(theme::txt2()),
                    );
                    ui.label(egui::RichText::new(txt).size(theme::FS_CAPTION).color(col));
                });
            }
        }
    }

    /// El carril de Ejecución: qué se corrió y qué contestó.
    ///
    /// EL COMANDO SE PARTE Y LA SALIDA NO, y son dos decisiones distintas sobre
    /// el mismo problema. Los dos se salían del panel y se recortaban contra su
    /// borde —el operador lo reportó como «se pierde texto»— pero no valen el
    /// mismo arreglo:
    ///
    ///   · UN COMANDO se lee igual partido en tres líneas, y así se ve entero.
    ///     Es lo que ya hace el carril de Plan.
    ///
    ///   · UNA SALIDA suele ser una TABLA. `Get-WinEvent` devuelve columnas
    ///     alineadas con espacios, y partirlas por el ancho del panel destruye
    ///     justo lo que las hace legibles: las columnas dejan de estar debajo de
    ///     su cabecera. Se desplaza en horizontal, que conserva la forma.
    pub(crate) fn ws_exec(&mut self, ui: &mut egui::Ui) {
        let ancho = ui.available_width();
        for (i, e) in self.tabs[self.tab].ws.exec.iter().enumerate() {
            ui.add_space(6.0);
            egui::Frame::none()
                .fill(theme::bg3())
                .stroke(egui::Stroke::new(1.0_f32, theme::bdr()))
                .rounding(egui::Rounding::same(theme::R_SM))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.set_max_width((ancho - 20.0).max(160.0));
                    ui.horizontal_top(|ui| {
                        ui.label(
                            egui::RichText::new(if e.ok { "✓" } else { "✕" })
                                .size(11.0)
                                .color(if e.ok { theme::acc() } else { theme::red() }),
                        );
                        // `wrap()` explícito: en un `horizontal`, un `Label`
                        // hereda «no partas» y crece hasta donde haga falta.
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&e.cmd)
                                    .size(theme::FS_CAPTION)
                                    .monospace()
                                    .color(theme::txt()),
                            )
                            .wrap(),
                        );
                    });
                    if !e.output.is_empty() {
                        ui.add_space(4.0);
                        egui::ScrollArea::horizontal()
                            .id_salt(("exec-out", i))
                            // Sin encogerse: si no, el área toma el ancho del
                            // texto y volvemos a empujar el panel.
                            .auto_shrink([false, true])
                            .max_height(260.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&e.output)
                                            .size(theme::FS_CAPTION)
                                            .monospace()
                                            .color(theme::txt3()),
                                    )
                                    // NO se parte: es lo que conserva las
                                    // columnas alineadas.
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            });
                    }
                });
        }
    }

    pub(crate) fn ws_trace(&mut self, ui: &mut egui::Ui) {
        // EL ANCHO, PORQUE AHORA ENTRAN COMANDOS. El detalle de una entrada era
        // media línea —«578 caracteres en 9.1 s»— y con eso nada se salía. Desde
        // que el carril anota «Comando lanzado» con el comando dentro, el detalle
        // puede medir doscientos caracteres, y un `vertical` dentro de un
        // `horizontal` no tiene tope: se recortaría contra el borde del panel,
        // que es el mismo fallo que ya se arregló en la conversación.
        //
        // 46 es el chip de la fase más las dos separaciones.
        let ancho = (ui.available_width() - 46.0).max(140.0);
        for t in &self.tabs[self.tab].ws.trace {
            ui.add_space(5.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // La fase va en su propio chip: en una lista larga es por lo
                // que se busca, no por la etiqueta.
                egui::Frame::none()
                    .fill(theme::bg4())
                    .rounding(egui::Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(6.0, 1.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(&t.phase)
                                .size(theme::FS_CAPTION)
                                .monospace()
                                .color(theme::txt3()),
                        );
                    });
                ui.allocate_ui_with_layout(
                    egui::vec2(ancho, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(ancho);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&t.label)
                                    .size(theme::FS_CAPTION)
                                    .color(theme::txt2()),
                            )
                            .wrap(),
                        );
                        if !t.detail.is_empty() {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&t.detail)
                                        .size(theme::FS_CAPTION)
                                        .color(theme::faint()),
                                )
                                .wrap(),
                            );
                        }
                    },
                );
            });
        }
    }

    pub(crate) fn ws_artifacts(&mut self, ui: &mut egui::Ui) {
        let mut escribir: Option<String> = None;
        for a in &self.tabs[self.tab].ws.artifacts {
            ui.add_space(6.0);
            egui::Frame::none()
                .fill(theme::bg3())
                .stroke(egui::Stroke::new(1.0_f32, theme::bdr()))
                .rounding(egui::Rounding::same(theme::R_SM))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(i18n::tr(a.kind.label()))
                                .size(theme::FS_CAPTION)
                                .color(theme::acc()),
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&a.path)
                                    .size(theme::FS_CAPTION)
                                    .monospace()
                                    .color(theme::txt()),
                            )
                            .truncate(),
                        );
                    });
                    if !a.summary.is_empty() {
                        ui.label(
                            egui::RichText::new(&a.summary)
                                .size(theme::FS_CAPTION)
                                .color(theme::faint()),
                        );
                    }
                    // Por qué no se puede aplicar, si no se puede. En rojo y
                    // sin botón: un botón que va a fallar es peor que ninguno.
                    if !a.blocked.is_empty() {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(&a.blocked)
                                .size(theme::FS_MICRO)
                                .color(theme::red()),
                        );
                        return;
                    }
                    // EL DIFF. Es lo que hace que el botón signifique algo:
                    // aprobar una ruta y la palabra de Lucy sobre lo que le va
                    // a hacer no es aprobar nada. Las líneas que cambian, con
                    // su signo, hasta un tope — un fichero entero en el carril
                    // no se lee, y para eso está abrirlo.
                    let d = diff_lineas(&a.before, &a.after, DIFF_MAX);
                    if !d.is_empty() {
                        ui.add_space(5.0);
                        egui::Frame::none()
                            .fill(theme::bg())
                            .rounding(egui::Rounding::same(theme::R_SM))
                            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                            .show(ui, |ui| {
                                for (signo, linea) in &d {
                                    ui.label(
                                        egui::RichText::new(format!("{signo} {linea}"))
                                            .size(theme::FS_MICRO)
                                            .monospace()
                                            .color(match signo {
                                                '+' => theme::acc(),
                                                '-' => theme::red(),
                                                _ => theme::faint(),
                                            }),
                                    );
                                }
                            });
                    }
                    if !a.applied {
                        ui.add_space(6.0);
                        if ui
                            .button(i18n::tr("Escribir"))
                            .on_hover_text(i18n::trf("Aplicar el cambio en {ruta}", &[("ruta", &a.path)]))
                            .clicked()
                        {
                            escribir = Some(a.id.clone());
                        }
                    } else {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("escrito")
                                .size(theme::FS_MICRO)
                                .color(theme::acc()),
                        );
                    }
                });
        }
        // Fuera del bucle: aplicar toca el mismo vector que se está recorriendo.
        if let Some(id) = escribir {
            self.aplicar_artefacto(&id);
        }
    }
}
