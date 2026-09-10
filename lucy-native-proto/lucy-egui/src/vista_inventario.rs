//! Inventario de equipos y su detalle.
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
    /// Fijar la línea base y comparar contra ella.
    pub(crate) fn inv_barra_drift(&mut self, ui: &mut egui::Ui) {
        // Se consulta una vez por equipo y se recuerda: es una fila de SQLite,
        // pero pedirla en cada frame es una consulta a sesenta por segundo para
        // pintar un texto que no cambia.
        if self.inv_base.is_none() {
            self.inv_base = Some(
                lucy_core::drift::get_baseline(&self.inv_host)
                    .ok()
                    .flatten()
                    .map(|b| (b.label, b.updated_at)),
            );
        }
        let base = self.inv_base.clone().flatten();
        ui.add_space(8.0);
        row_align(ui, 26.0, egui::Align::Center, |ui| {
            match &base {
                Some((label, ts)) => {
                    let etiqueta = if label.trim().is_empty() { i18n::tr("sin etiqueta") } else { label };
                    ui.label(
                        egui::RichText::new(i18n::trf(
                            "Línea base: {etiqueta} · {cuando}",
                            &[
                                ("etiqueta", etiqueta),
                                ("cuando", &hace_cuanto(ahora_epoch() - *ts)),
                            ],
                        ))
                        .size(theme::FS_CAPTION)
                        .color(theme::txt3()),
                    );
                    ui.add_space(8.0);
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new(i18n::tr("Ver cambios"))
                                .size(theme::FS_CAPTION)
                                .color(theme::acc()),
                        ))
                        .on_hover_text(i18n::tr("Comparar esta foto con la línea base"))
                        .clicked()
                    {
                        self.inv_comparar();
                    }
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new(i18n::tr("Rehacer")).size(theme::FS_CAPTION),
                        ))
                        .on_hover_text(i18n::tr("Esta foto pasa a ser la nueva línea base"))
                        .clicked()
                    {
                        self.inv_fijar_base();
                    }
                }
                None => {
                    ui.label(
                        egui::RichText::new(i18n::tr("Sin línea base para este equipo."))
                            .size(theme::FS_CAPTION)
                            .color(theme::faint()),
                    );
                    ui.add_space(8.0);
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new(i18n::tr("Fijar línea base"))
                                .size(theme::FS_CAPTION)
                                .color(theme::acc()),
                        ))
                        .on_hover_text(
                            "Declara que este equipo está como debe. A partir de aquí se \
                             puede ver qué cambia.",
                        )
                        .clicked()
                    {
                        self.inv_fijar_base();
                    }
                }
            }
            if self.inv_drift.is_some() {
                right(ui, 26.0, |ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new(i18n::tr("Volver al inventario"))
                                .size(theme::FS_CAPTION),
                        ))
                        .clicked()
                    {
                        self.inv_drift = None;
                    }
                });
            }
        });
    }

    pub(crate) fn inv_fijar_base(&mut self) {
        let etiqueta = lv_hora();
        match lucy_core::drift::set_baseline(&self.inv_host, &etiqueta, &self.inv_data) {
            Ok(()) => {
                self.inv_base = None; // se relee
                // El informe anterior deja de valer: comparaba contra otra cosa,
                // y dejarlo en pantalla diría que esos cambios siguen ahí.
                self.inv_drift = None;
                self.inv_error.clear();
            }
            Err(e) => self.inv_error = e,
        }
    }

    pub(crate) fn inv_comparar(&mut self) {
        match lucy_core::drift::get_baseline(&self.inv_host) {
            Ok(Some(b)) => {
                let mut r = lucy_core::drift::compare(&b.inv, &self.inv_data);
                r.edad_secs = ahora_epoch() - b.updated_at;
                r.label = b.label;
                self.inv_drift = Some(r);
            }
            Ok(None) => self.inv_error = i18n::tr("Este equipo no tiene línea base todavía.").into(),
            Err(e) => self.inv_error = e,
        }
    }

    pub(crate) fn inv_tabla_drift(&mut self, ui: &mut egui::Ui, r: &lucy_core::drift::Report) {
        use lucy_core::drift::Cambio;
        ui.add_space(8.0);
        if r.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(if r.efimeros_ignorados == 0 {
                        i18n::tr("Nada ha cambiado desde la línea base.").to_string()
                    } else {
                        // El «sin cambios» se explica: descartar cuarenta filas
                        // por su cuenta y no decirlo hace que la próxima vez que
                        // alguien eche en falta un puerto sospeche del programa.
                        i18n::trf(
                            "Nada ha cambiado desde la línea base.\n\
                             ({n} puertos dinámicos ignorados — el sistema los reparte en \
                             cada arranque.)",
                            &[("n", &r.efimeros_ignorados.to_string())],
                        )
                    })
                    .size(theme::FS_FOOTNOTE)
                    .color(theme::faint()),
                );
            });
            return;
        }

        row_align(ui, 24.0, egui::Align::Center, |ui| {
            for c in lucy_core::inventory::Categoria::ALL {
                let n = r.cuenta(c);
                if n > 0 {
                    ui.label(
                        egui::RichText::new(i18n::trf(
                            "{cat} {n}",
                            &[("cat", i18n::tr(c.label())), ("n", &n.to_string())],
                        ))
                            .size(theme::FS_CAPTION)
                            .color(theme::txt3()),
                    );
                    ui.add_space(10.0);
                }
            }
            if r.efimeros_ignorados > 0 {
                ui.label(
                    egui::RichText::new(i18n::trf(
                        "· {n} dinámicos ignorados",
                        &[("n", &r.efimeros_ignorados.to_string())],
                    ))
                    .size(theme::FS_CAPTION)
                    .color(theme::faint()),
                );
            }
        });
        ui.add_space(4.0);

        let alto = 20.0_f32;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("inv-drift")
            .show_rows(ui, alto, r.filas.len(), |ui, rango| {
                for k in rango {
                    let f = &r.filas[k];
                    // APARECER Y DESAPARECER NO SON IGUAL DE GRAVES, y el color lo
                    // dice antes que el texto: algo nuevo escuchando en un puerto
                    // es lo que se busca en este panel; algo que ya no está suele
                    // ser una limpieza.
                    let (marca, col) = match &f.cambio {
                        Cambio::Apareció => ("+", theme::acc()),
                        Cambio::Desapareció => ("−", theme::amber()),
                        Cambio::Cambió { .. } => ("~", theme::red()),
                    };
                    ui.horizontal(|ui| {
                        ui.set_height(alto);
                        celda(ui, marca, 18.0, col, true);
                        celda(ui, i18n::tr(f.cat.label()), 96.0, theme::txt3(), false);
                        celda(ui, &f.id, 240.0, theme::txt(), true);
                        let texto = match &f.cambio {
                            Cambio::Cambió { campo, de, a } => {
                                format!("{campo}: {de} → {a}")
                            }
                            _ => f.detalle.clone(),
                        };
                        celda(ui, &texto, 0.0, theme::txt2(), false);
                    });
                }
            });
    }

    pub(crate) fn inv_cabecera(&mut self, ui: &mut egui::Ui) {
        let mut escanear = false;
        let mut parar = false;
        row_align(ui, 30.0, egui::Align::Center, |ui| {
            let corriendo = self.inv_rx.is_some();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
            ui.painter().circle_filled(
                rect.center(),
                3.5,
                if corriendo {
                    theme::amber()
                } else if self.inv_last.is_empty() {
                    theme::faint()
                } else {
                    theme::acc()
                },
            );
            ui.add_space(4.0);
            titulo_modulo(ui, View::Inventario);
            ui.add_space(8.0);
            if self.inv_host_picker(ui) {
                escanear = true;
            }
            ui.add_space(8.0);
            if let Some(t0) = self.inv_desde {
                ui.add_space(8.0);
                // El tiempo transcurrido, en segundos. Un escaneo tarda entre dos
                // y quince, y un botón que no hace nada visible durante quince
                // segundos se pulsa otra vez.
                ui.label(
                    egui::RichText::new(format!("{}s", t0.elapsed().as_secs()))
                        .size(theme::FS_CAPTION)
                        .color(theme::txt3()),
                );
            } else if !self.inv_last.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(i18n::trf(
                        "escaneado {hora}",
                        &[("hora", &self.inv_last)],
                    ))
                        .size(theme::FS_CAPTION)
                        .color(theme::acc()),
                );
            }

            right(ui, 30.0, |ui| {
                // EL BOTÓN A LA DERECHA DEL TODO, como en la vista que se migra:
                // es la acción principal de la pantalla y ahí es donde se busca.
                //
                // Y MIENTRAS CORRE, PARA — no se apaga. Deshabilitado dejaba la
                // vista sin nada que pulsar durante los minutos que WinRM tarda
                // en rendirse contra un equipo apagado, que es justo cuando el
                // operador más quiere salir de ahí.
                let b = egui::Button::new(
                    egui::RichText::new(i18n::tr(if corriendo { "■  Parar" } else { "⟳  Escanear" }))
                        .size(theme::FS_CAPTION)
                        .color(if corriendo { theme::txt() } else { theme::acc_ink() }),
                )
                .fill(if corriendo { theme::bg4() } else { theme::acc() })
                .stroke(if corriendo {
                    egui::Stroke::new(1.0_f32, theme::amber())
                } else {
                    egui::Stroke::NONE
                })
                .rounding(egui::Rounding::same(theme::R_SM))
                .min_size(egui::vec2(104.0, 26.0));
                if ui.add(b).clicked() {
                    if corriendo {
                        parar = true;
                    } else {
                        escanear = true;
                    }
                }
                ui.add_space(6.0);
                let hay = !self.inv_data.is_empty();
                if ghost_icon(ui, icons::Icon::Copy)
                    .on_hover_text(i18n::tr(if hay {
                        "Copiar el inventario en CSV"
                    } else {
                        "Nada que copiar todavía"
                    }))
                    .clicked()
                    && hay
                {
                    let nombre = self.inv_nombre_equipo();
                    let csv = lucy_core::inventory::to_csv(&self.inv_data, &nombre);
                    ui.ctx().copy_text(csv);
                }
            });
        });
        if parar {
            // La bandera mata el proceso remoto; soltar el canal devuelve la
            // vista al operador ya, sin esperar a que el hilo se entere.
            self.inv_stop.store(true, std::sync::atomic::Ordering::Relaxed);
            self.inv_rx = None;
            self.inv_desde = None;
            self.inv_error = "Escaneo detenido.".into();
        }
        if escanear {
            self.inv_escanear();
        }
    }

    /// Cómo se llama el equipo que se está mirando.
    pub(crate) fn inv_nombre_equipo(&self) -> String {
        if self.inv_host.is_empty() {
            lucy_core::system::hostname()
        } else {
            self.remote_hosts
                .iter()
                .find(|h| h.id == self.inv_host)
                .map(|h| h.name.clone())
                .unwrap_or_else(|| i18n::tr("Equipo").into())
        }
    }

    /// El desplegable de equipos. Devuelve si hay que escanear.
    pub(crate) fn inv_host_picker(&mut self, ui: &mut egui::Ui) -> bool {
        // EL NOMBRE DE LA MÁQUINA Y CÓMO SE LLEGA A ELLA, como en la vista que se
        // migra. «Este equipo» no dice cuál es, y con una captura de pantalla en
        // un ticket eso es justo lo que hace falta saber.
        let etiqueta = if self.inv_host.is_empty() {
            format!("{} · local", lucy_core::system::hostname())
        } else {
            let via = self
                .remote_hosts
                .iter()
                .find(|h| h.id == self.inv_host)
                .map(|h| {
                    if h.protocol == lucy_core::hosts::Protocol::Winrm {
                        "WinRM"
                    } else {
                        "SSH"
                    }
                })
                .unwrap_or("?");
            format!("{} · {via}", self.inv_nombre_equipo())
        };
        let boton = ui.add(
            egui::Button::new(
                egui::RichText::new(format!("▤ {etiqueta}"))
                    .monospace()
                    .size(theme::FS_FOOTNOTE)
                    .color(theme::txt3()),
            )
            .fill(theme::bg3())
            .stroke(egui::Stroke::new(1.0_f32, theme::bdr()))
            .rounding(egui::Rounding::same(theme::R_SM)),
        );
        if boton.clicked() {
            self.inv_host_menu = !self.inv_host_menu;
        }
        if !self.inv_host_menu {
            return false;
        }
        let mut elegido: Option<String> = None;
        egui::Area::new(egui::Id::new("inv-host-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(boton.rect.left_bottom() + egui::vec2(0.0, 6.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::none()
                    .fill(theme::bg3())
                    .stroke(egui::Stroke::new(1.0_f32, theme::bdr2()))
                    .rounding(egui::Rounding::same(theme::R_LG))
                    .inner_margin(egui::Margin::same(6.0))
                    // La sombra de la casa, no una copia con la mitad de
                    // profundidad. Ver `theme::sombra_flotante`.
                    .shadow(theme::sombra_flotante())
                    .show(ui, |ui| {
                        ui.set_min_width(210.0);
                        if lv_opcion(ui, i18n::tr("Este equipo"), "local", self.inv_host.is_empty()) {
                            elegido = Some(String::new());
                        }
                        // Solo los que tienen shell: un equipo dado de alta como
                        // Postgres no se puede inventariar, y ofrecerlo promete
                        // un escaneo que falla sin explicar por qué estaba ahí.
                        for h in self.remote_hosts.iter().filter(|h| h.protocol.can_shell()) {
                            let tipo = if h.protocol == lucy_core::hosts::Protocol::Winrm {
                                "WinRM"
                            } else {
                                "SSH"
                            };
                            if lv_opcion(ui, &h.name, tipo, self.inv_host == h.id) {
                                elegido = Some(h.id.clone());
                            }
                        }
                    });
            });
        if ui.input(|i| i.pointer.any_click()) && !boton.clicked() && elegido.is_none() {
            let dentro = ui.ctx().pointer_latest_pos().is_some_and(|p| {
                ui.ctx()
                    .memory(|m| m.area_rect(egui::Id::new("inv-host-menu")))
                    .is_some_and(|r| r.contains(p))
            });
            if !dentro {
                self.inv_host_menu = false;
            }
        }
        match elegido {
            Some(id) => {
                self.inv_host_menu = false;
                let cambio = id != self.inv_host;
                self.inv_host = id;
                if cambio {
                    // La foto era de OTRA máquina. Dejarla mientras se escanea la
                    // nueva enseñaría los servicios de un servidor bajo el nombre
                    // de otro — que sobre inventario es la peor mentira posible,
                    // porque es exactamente el dato que se viene a comprobar.
                    self.inv_data = lucy_core::inventory::Inventory::default();
                    self.inv_error.clear();
                    self.inv_last.clear();
                    self.inv_sort = [None; lucy_core::inventory::Categoria::ALL.len()];
                    // La línea base es POR EQUIPO. Sin esto, el panel seguiría
                    // enseñando «Línea base: 14:22 · hace 3 días» sobre la
                    // máquina recién elegida, que puede no tener ninguna — y el
                    // informe compararía la foto de una contra la foto de otra.
                    self.inv_base = None;
                    self.inv_drift = None;
                }
                // Cambiar de equipo NO escanea solo: un escaneo son segundos y
                // una sesión autenticada contra el servidor, y abrir un
                // desplegable no es pedir eso. Se pulsa Escanear.
                false
            }
            None => false,
        }
    }

    pub(crate) fn inv_pestanas(&mut self, ui: &mut egui::Ui) {
        use lucy_core::inventory::Categoria as C;
        ui.add_space(10.0);
        // TARJETAS Y NO PESTAÑAS. La cifra es el dato: «157 software» se lee de
        // un vistazo desde el otro lado de la mesa, y a la vez es el botón que
        // abre esa tabla. Con chips había que leer el número pequeño entre
        // paréntesis para enterarse de lo mismo.
        // NUNCA ESCANEADO ≠ CERO. La misma condición que usa el texto de la
        // tabla de abajo, para que las tarjetas y ese texto no se contradigan.
        let virgen = self.inv_data.is_empty() && self.inv_last.is_empty();
        ui.horizontal(|ui| {
            for c in C::ALL {
                let n = (!virgen).then(|| self.inv_data.len_de(c));
                // Una categoría que falló NO enseña un cero. Un cero dice «no hay
                // ninguno», que es un hecho sobre el equipo; lo que pasó es que
                // no se pudo mirar, y son cosas distintas.
                let fallo = self.inv_data.fallo_de(c).is_some();
                if inv_tarjeta(ui, c.label(), n, fallo, self.inv_cat == c) {
                    self.inv_cat = c;
                }
                ui.add_space(8.0);
            }
        });
        ui.add_space(10.0);
        // El texto de ayuda dice QUÉ se está filtrando. Con cinco tablas detrás
        // de cinco tarjetas, un «Filtrar…» a secas no dice sobre cuál actúa.
        ui.add_sized(
            [ui.available_width(), 30.0],
            egui::TextEdit::singleline(&mut self.inv_query)
                .hint_text(i18n::trf(
                    "⌕   Filtrar {cat}…",
                    &[("cat", &i18n::tr(self.inv_cat.label()).to_lowercase())],
                )),
        );
    }

    pub(crate) fn inv_tabla(&mut self, ui: &mut egui::Ui) {
        use lucy_core::inventory::Categoria as C;
        let cat = self.inv_cat;
        let ci = C::ALL.iter().position(|c| *c == cat).unwrap_or(0);
        let filas = inv_filas(&self.inv_data, cat, &self.inv_query, self.inv_sort[ci]);

        if filas.is_empty() {
            // Envuelto donde está el literal, por lo mismo que en el visor.
            let msg = if self.inv_data.is_empty() && self.inv_last.is_empty() {
                i18n::tr("Pulsa Escanear para hacerle una foto a este equipo.")
            } else if self.inv_data.len_de(cat) == 0 {
                match self.inv_data.fallo_de(cat) {
                    // Distinguir «no hay» de «no se pudo mirar» es la mitad del
                    // valor de un inventario: lo primero es un hecho del equipo y
                    // lo segundo un problema de permisos.
                    Some(_) => {
                        i18n::tr("No se pudo consultar esta categoría — el motivo está arriba.")
                    }
                    None => i18n::tr("Esta categoría no tiene nada en este equipo."),
                }
            } else {
                i18n::tr("Sin coincidencias.")
            };
            ui.centered_and_justified(|ui| {
                ui.label(
                    // Igual que en el visor: el `tr` va donde se pinta, porque
                    // `msg` sale de una cadena de `if` con cuatro literales.
                    egui::RichText::new(msg)
                        .monospace()
                        .size(theme::FS_FOOTNOTE)
                        .color(theme::faint()),
                );
            });
            return;
        }

        // ── cabecera de columnas, que además ordena ──
        //
        // EN UN `horizontal` NORMAL Y NO EN `row_align`. `row_align` pone
        // `item_spacing.x = GAP` (10 px) y las filas de dentro del `ScrollArea`
        // heredan el del tema (8 px): con eso, «Estado» quedaba 2 px a la
        // derecha de sus celdas y «Descripción» 4 px, y la flecha de ordenar
        // acababa señalando el hueco entre dos columnas.
        let cols = inv_columnas(cat);
        // El espaciado del tema, el mismo que usarán las filas — es lo que hace
        // que cabecera y contenido caigan en la misma rejilla.
        let gap = ui.spacing().item_spacing.x;
        let anchos = inv_anchos(cols, ui.available_width(), gap);
        let mut nuevo_orden = self.inv_sort[ci];
        ui.horizontal(|ui| {
            ui.set_height(24.0);
            for (n, (titulo, _)) in cols.iter().enumerate() {
                let ancho = &anchos[n];
                let activa = self.inv_sort[ci].map(|(c, _)| c) == Some(n);
                let flecha = match self.inv_sort[ci] {
                    Some((c, asc)) if c == n => {
                        if asc {
                            " ▲"
                        } else {
                            " ▼"
                        }
                    }
                    _ => "",
                };
                let w = *ancho;
                let r = ui.add_sized(
                    [w, 22.0],
                    egui::Label::new(
                        egui::RichText::new(format!("{}{flecha}", i18n::tr(titulo)))
                            .size(theme::FS_CAPTION)
                            .color(if activa { theme::acc() } else { theme::txt3() }),
                    )
                    .sense(egui::Sense::click()),
                );
                if r.on_hover_text(i18n::tr("Ordenar por esta columna")).clicked() {
                    // Tres estados y no dos: ascendente, descendente y NINGUNO.
                    // El orden en que llega el software es el que da el sistema y
                    // a veces es el útil; sin forma de volver a él, ordenar una
                    // vez sería irreversible sin reescanear.
                    nuevo_orden = match self.inv_sort[ci] {
                        Some((c, true)) if c == n => Some((n, false)),
                        Some((c, false)) if c == n => None,
                        _ => Some((n, true)),
                    };
                }
            }
        });
        self.inv_sort[ci] = nuevo_orden;
        ui.add_space(2.0);

        let alto = 20.0_f32;
        let ahora = ahora_epoch();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("inv-tabla")
            .show_rows(ui, alto, filas.len(), |ui, rango| {
                for k in rango {
                    let i = filas[k];
                    ui.horizontal(|ui| {
                        ui.set_height(alto);
                        match cat {
                            C::Puertos => {
                                let p = &self.inv_data.ports[i];
                                celda(ui, &p.port.to_string(), anchos[0], theme::txt(), true);
                                celda(ui, &p.process, anchos[1], theme::txt2(), true);
                                celda(ui, "LISTEN", anchos[2], theme::acc(), true);
                            }
                            C::Servicios => {
                                let s = &self.inv_data.services[i];
                                celda(ui, &s.name, anchos[0], theme::txt(), true);
                                let col = if s.status.starts_with("run") {
                                    theme::acc()
                                } else {
                                    theme::txt3()
                                };
                                celda(ui, &s.status, anchos[1], col, true);
                                celda(ui, &s.description, anchos[2], theme::txt2(), false);
                            }
                            C::Software => {
                                let s = &self.inv_data.software[i];
                                celda(ui, &s.name, anchos[0], theme::txt(), false);
                                celda(ui, &s.version, anchos[1], theme::txt2(), true);
                            }
                            C::Certificados => {
                                let c = &self.inv_data.certs[i];
                                // EL COLOR SALE DE LOS DÍAS QUE QUEDAN, que es
                                // para lo que se abre esta pestaña. Un
                                // certificado caducado en el mismo gris que uno
                                // de dos años no se distingue leyendo una lista
                                // de cuarenta.
                                //
                                // Y «no se sabe» tiene su propio aspecto. En
                                // Alpine y en BSD el equipo no sabe convertir la
                                // fecha de `openssl`, y pintar eso como «caducó
                                // hace 20672d» en rojo manda a renovar un
                                // certificado que puede estar impecable.
                                let (txt, col) = match c.days_left(ahora) {
                                    None => {
                                        (i18n::tr("fecha ilegible").to_string(), theme::txt3())
                                    }
                                    Some(d) if d < 0 => (
                                        i18n::trf("caducó hace {d}d", &[("d", &(-d).to_string())]),
                                        theme::red(),
                                    ),
                                    Some(d) if d <= 30 => (format!("{d}d"), theme::amber()),
                                    Some(d) => (format!("{d}d"), theme::txt3()),
                                };
                                celda(ui, &txt, anchos[0], col, true);
                                celda(ui, &c.subject, anchos[1], theme::txt(), false);
                                celda(ui, &c.path, anchos[2], theme::txt3(), false);
                            }
                            C::Tareas => {
                                let t = &self.inv_data.tasks[i];
                                let col = match t.state.as_str() {
                                    "Running" => theme::acc(),
                                    "Disabled" => theme::faint(),
                                    _ => theme::txt3(),
                                };
                                celda(
                                    ui,
                                    if t.state.is_empty() { "cron" } else { &t.state },
                                    anchos[0],
                                    col,
                                    true,
                                );
                                celda(ui, &t.entry, anchos[1], theme::txt2(), false);
                            }
                        }
                    });
                }
            });
    }

    /// Lanza el escaneo en un hilo.
    pub(crate) fn inv_escanear(&mut self) {
        if self.inv_rx.is_some() {
            return;
        }
        self.inv_error.clear();
        // «ESTE EQUIPO» Y «UN EQUIPO QUE YA NO ESTÁ» NO SON EL MISMO CASO, y se
        // derrumbaban en uno. Un `find()` que no encuentra devuelve `None`, y el
        // `filter` posterior no lo arregla porque ya era `None`: el hilo se iba
        // por la rama local y escaneaba la máquina del operador para
        // presentársela bajo el nombre del servidor que acababa de borrar.
        let host = if self.inv_host.is_empty() {
            None
        } else {
            match self.remote_hosts.iter().find(|h| h.id == self.inv_host) {
                Some(h) => Some(h.clone()),
                None => {
                    self.inv_error =
                        i18n::tr("Ese equipo ya no está dado de alta. Elige otro en el desplegable.")
                            .into();
                    return;
                }
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        // Uno nuevo por escaneo, no bajar el de antes: si quedara un hilo del
        // anterior mirando el mismo booleano, bajarlo lo resucitaría.
        self.inv_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = self.inv_stop.clone();
        std::thread::spawn(move || {
            let r = match host {
                // La contraseña se saca DENTRO del hilo: leer el almacén de
                // credenciales abre un diálogo del sistema la primera vez, y en
                // el hilo de la interfaz eso congela la ventana.
                Some(h) => {
                    let pw = lucy_core::hosts::password(&h.id).unwrap_or_default();
                    lucy_core::inventory::discover_remote(&h, &pw, &stop)
                }
                None => lucy_core::inventory::discover_local(),
            };
            let _ = tx.send(r);
        });
        self.inv_rx = Some((self.inv_host.clone(), rx));
        self.inv_desde = Some(Instant::now());
    }
}
