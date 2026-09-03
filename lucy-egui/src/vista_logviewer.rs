//! Visor de registros: filtrado, resaltado y navegacion.
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
    /// Qué logs tiene este equipo. Carpetas sugeridas y lo que hay dentro.
    pub(crate) fn lv_explorador(&mut self, ui: &mut egui::Ui) {
        let Some(h) = self.remote_hosts.iter().find(|x| x.id == self.lv_host).cloned() else {
            return;
        };
        let mut explorar: Option<String> = None;
        let mut abrir: Option<String> = None;

        ui.add_space(8.0);
        ui.add(egui::Label::new(theme::instrument_label(
            &i18n::trf("Dónde mirar en {equipo}", &[("equipo", &h.name)]),
            theme::faint(),
        )));
        ui.add_space(6.0);
        // Las carpetas dependen del sistema: ofrecerle `/var/log` a un Windows
        // sería prometer un listado que vuelve vacío, y el operador no sabría si
        // es que no hay logs o que la ruta no aplica a esa máquina.
        ui.horizontal_wrapped(|ui| {
            for (nombre, ruta) in lucy_core::logs::common_dirs(&h) {
                if lv_chip(ui, nombre, 0, self.lv_dir == *ruta) {
                    explorar = Some((*ruta).to_string());
                }
                ui.add_space(6.0);
            }
        });

        if self.lv_files_rx.is_some() {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(i18n::trf(
                    "buscando en {ruta}…",
                    &[("ruta", &self.lv_dir)],
                ))
                    .size(theme::FS_CAPTION)
                    .color(theme::txt3()),
            );
        } else if !self.lv_files.is_empty() {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(i18n::trf(
                    "{n} ficheros en {dir} — el más reciente primero",
                    &[
                        ("n", &self.lv_files.len().to_string()),
                        ("dir", &self.lv_dir),
                    ],
                ))
                .size(theme::FS_CAPTION)
                .color(theme::txt3()),
            );
            ui.add_space(4.0);
            // Altura acotada: con doscientos ficheros esto se comería la pantalla
            // entera y el flujo de abajo dejaría de verse.
            egui::ScrollArea::vertical()
                .max_height(190.0)
                .auto_shrink([false, false])
                .id_salt("lv-files")
                .show(ui, |ui| {
                    for f in &self.lv_files {
                        let r = ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!(
                                    "{}   {}   {}",
                                    f.modified, f.size, f.path
                                ))
                                .monospace()
                                .size(theme::FS_CAPTION)
                                .color(theme::txt2()),
                            )
                            .truncate()
                            .sense(egui::Sense::click()),
                        );
                        if r.hovered() {
                            ui.painter().rect_filled(
                                r.rect.expand2(egui::vec2(4.0, 1.0)),
                                egui::Rounding::same(4.0),
                                theme::bg3(),
                            );
                        }
                        if r.on_hover_text(i18n::tr("Leer la cola de este fichero")).clicked() {
                            abrir = Some(f.path.clone());
                        }
                    }
                });
        } else if !self.lv_dir.is_empty() {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(i18n::trf("No hay ficheros de log en {dir}.", &[("dir", &self.lv_dir)]))
                    .size(theme::FS_CAPTION)
                    .color(theme::faint()),
            );
        }

        if let Some(d) = explorar {
            self.lv_explorar(&h, &d);
        }
        if let Some(p) = abrir {
            self.lv_path = p;
            self.lv_cargar();
        }
    }

    pub(crate) fn lv_explorar(&mut self, h: &lucy_core::hosts::Host, dir: &str) {
        if self.lv_files_rx.is_some() {
            return;
        }
        self.lv_dir = dir.to_string();
        self.lv_files.clear();
        self.lv_error.clear();
        let (host, carpeta) = (h.clone(), dir.to_string());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let pw = lucy_core::hosts::password(&host.id).unwrap_or_default();
            let _ = tx.send(lucy_core::logs::list_remote(&host, &pw, &carpeta));
        });
        self.lv_files_rx = Some(rx);
    }

    pub(crate) fn lv_cabecera(&mut self, ui: &mut egui::Ui) {
        let mut recargar = false;
        row_align(ui, 30.0, egui::Align::Center, |ui| {
            // El punto de estado: verde si se está releyendo solo, ámbar si está
            // en pausa. Sin él, «en vivo» y «pausado» se distinguen leyendo, y
            // esto se mira de reojo.
            let vivo = !self.lv_paused;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
            ui.painter().circle_filled(
                rect.center(),
                3.5,
                if vivo { theme::acc() } else { theme::amber() },
            );
            ui.add_space(4.0);
            titulo_modulo(ui, View::LogViewer);
            ui.add_space(6.0);

            // ── modo ──
            //
            // POR `segmentado`, QUE ES EL CONTROL DE LA CASA. Esto tenía su
            // propio segmentado hecho a mano —`fn seg`, un `Button` de 18 px con
            // radio 6— dentro de un `Frame` con otro radio y sin tocar el
            // `item_spacing`, o sea con ocho píxeles entre las dos opciones. Con
            // esa separación no se leía como un grupo: se leía como dos botones
            // sueltos metidos en una caja, que es el aspecto exacto de un
            // GroupBox de Windows. Y no deslizaba: el relleno saltaba de una a
            // otra, lo que se notaba más porque el segmentado de Configuración
            // sí desliza.
            let mut nuevo = self.lv_mode;
            let modos = [i18n::tr("Auditoría"), i18n::tr("Archivo")];
            let actual = if self.lv_mode == LvMode::Auditoria { 0 } else { 1 };
            if let Some(i) = segmentado(ui, "lv-modo", 200.0, &modos, actual) {
                nuevo = if i == 0 { LvMode::Auditoria } else { LvMode::Archivo };
            }
            if nuevo != self.lv_mode {
                self.lv_mode = nuevo;
                // Las filas del modo anterior NO se quedan. Contestan a otra
                // pregunta, y dejarlas mientras carga lo nuevo haría que los
                // contadores de la barra describieran algo que ya no se enseña.
                self.lv_rows.clear();
                self.lv_error.clear();
                self.lv_last.clear();
                recargar = true;
            }
            ui.add_space(6.0);

            match self.lv_mode {
                LvMode::Auditoria => {
                    // LAS DOS PREGUNTAS QUE LA LISTA NO CONTESTA. Aquí ponía
                    // «audit trail» —el nombre de la tabla, que el operador ya
                    // sabe— en el único sitio de la aplicación donde caben las
                    // dos cifras que hacen falta:
                    //
                    //   supervisión  de lo que CORRIÓ, qué fracción miró alguien
                    //   aceptación   de lo que Lucy PROPUSO, cuánto se ejecutó
                    //
                    // La primera es la pregunta de auditoría de verdad y con el
                    // automático encendido es la única forma de notar que está
                    // haciendo cosas que nadie habría dejado pasar. La segunda
                    // es la medida más barata de si lo que Lucy sugiere sirve.
                    //
                    // Los orígenes se separaron ayer y hasta ahora no los
                    // agregaba nadie: escribir la señal y no leerla es el mismo
                    // fallo que se ha ido cerrando por toda la casa, y este me
                    // lo hice yo al añadir las fuentes.
                    if let Some(r) = &self.lv_resumen {
                        let mut partes: Vec<String> = Vec::new();
                        if let Some(s) = r.supervision() {
                            partes.push(i18n::trf(
                                "{pct}% supervisado",
                                &[("pct", &format!("{:.0}", s * 100.0))],
                            ));
                        }
                        if let Some(a) = r.aceptacion() {
                            partes.push(i18n::trf(
                                "{pct}% de lo propuesto se ejecutó",
                                &[("pct", &format!("{:.0}", a * 100.0))],
                            ));
                        }
                        let texto = if partes.is_empty() {
                            i18n::tr("sin actividad en 30 días").to_string()
                        } else {
                            partes.join(" · ")
                        };
                        ui.add(egui::Label::new(
                            egui::RichText::new(texto)
                                .size(theme::FS_FOOTNOTE)
                                .color(theme::txt3()),
                        ))
                        .on_hover_text(i18n::trf(
                            "Últimos 30 días: {apr} comandos los aprobó una persona, {solos} \
                             los lanzó el automático, {desc} se propusieron y no se ejecutaron.",
                            &[
                                ("apr", &r.aprobados.to_string()),
                                ("solos", &r.solos.to_string()),
                                ("desc", &r.descartados.to_string()),
                            ],
                        ));
                    }
                }
                LvMode::Archivo => {
                    if self.lv_host_picker(ui) {
                        recargar = true;
                    }
                    ui.add_space(6.0);
                    let ph = if self.lv_host.is_empty() {
                        i18n::tr("C:\\ruta\\al\\archivo.log")
                    } else {
                        "/var/log/syslog"
                    };
                    let campo = ui.add_sized(
                        [280.0, 24.0],
                        egui::TextEdit::singleline(&mut self.lv_path)
                            .font(egui::TextStyle::Monospace)
                            .hint_text(ph),
                    );
                    // Enter lee. `lost_focus` + la tecla, que es la forma que
                    // funciona en un `singleline`: mirar solo la tecla dispara
                    // también cuando el foco está en otro sitio de la vista.
                    if campo.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        recargar = true;
                    }
                    ui.add_space(4.0);
                    if ghost_icon(ui, icons::Icon::Refresh)
                        .on_hover_text(i18n::tr("Leer la cola del fichero"))
                        .clicked()
                    {
                        recargar = true;
                    }
                }
            }

            if !self.lv_last.is_empty() {
                ui.add_space(8.0);
                let (txt, col) = if self.lv_paused {
                    (
                        i18n::trf("⏸ pausado · {hora}", &[("hora", &self.lv_last)]),
                        theme::amber(),
                    )
                } else {
                    (
                        i18n::trf("en vivo · {hora}", &[("hora", &self.lv_last)]),
                        theme::acc(),
                    )
                };
                ui.label(egui::RichText::new(txt).size(theme::FS_CAPTION).color(col));
            }
            // Y si hay una lectura remota en vuelo se dice, con lo que lleva
            // esperando: un botón que no hace nada visible durante ocho segundos
            // se pulsa otra vez, y entonces son dos sesiones contra el servidor.
            if let Some(t0) = self.lv_desde {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(i18n::trf("leyendo… {s}s", &[("s", &t0.elapsed().as_secs().to_string())]))
                        .size(theme::FS_CAPTION)
                        .color(theme::txt3()),
                );
            }

            right(ui, 30.0, |ui| {
                let n = lv_filtrar(&self.lv_rows, self.lv_filter, &self.lv_query).len();
                if ghost_icon(ui, icons::Icon::Copy)
                    .on_hover_text(if n == 0 {
                        i18n::tr("No hay nada visible que copiar").to_string()
                    } else {
                        i18n::trf("Copiar las {n} líneas visibles", &[("n", &n.to_string())])
                    })
                    .clicked()
                    && n > 0
                {
                    let txt = self.lv_texto_visible();
                    ui.ctx().copy_text(txt);
                }
                let icono = if self.lv_paused { icons::Icon::Play } else { icons::Icon::Pause };
                if ghost_icon(ui, icono)
                    .on_hover_text(i18n::tr(if self.lv_paused {
                        "Reanudar la actualización"
                    } else {
                        "Pausar la actualización"
                    }))
                    .clicked()
                {
                    self.lv_paused = !self.lv_paused;
                    // Al reanudar se relee YA. Esperar al siguiente tic dejaría
                    // hasta cinco segundos de pantalla vieja justo después de
                    // pedir explícitamente que vuelva a moverse.
                    if !self.lv_paused {
                        recargar = true;
                    }
                }
            });
        });
        if recargar {
            self.lv_cargar();
        }
    }

    /// El desplegable de equipos. Devuelve si hay que releer.
    pub(crate) fn lv_host_picker(&mut self, ui: &mut egui::Ui) -> bool {
        let etiqueta = if self.lv_host.is_empty() {
            i18n::tr("Este equipo").to_string()
        } else {
            self.remote_hosts
                .iter()
                .find(|h| h.id == self.lv_host)
                .map(|h| h.name.clone())
                .unwrap_or_else(|| i18n::tr("Equipo").into())
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
            self.lv_host_menu = !self.lv_host_menu;
        }
        if !self.lv_host_menu {
            return false;
        }

        let mut elegido: Option<String> = None;
        // En un `Area` y no pintando a pelo sobre una capa: el menú se dibuja
        // encima de lo que ya está colocado, y `rect_contains_pointer` mira el
        // rectángulo de recorte de quien llama — que aquí es una fila de 30 px.
        // Es el mismo fallo que dejó la paleta de comandos sin poder pulsarse.
        egui::Area::new(egui::Id::new("lv-host-menu"))
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
                        if lv_opcion(ui, i18n::tr("Este equipo"), "local", self.lv_host.is_empty()) {
                            elegido = Some(String::new());
                        }
                        // SOLO LOS QUE SABEN LEER UN FICHERO. Un equipo dado de
                        // alta como Redis o Postgres no tiene shell, y ofrecerlo
                        // aquí es prometer una lectura que va a fallar con un
                        // mensaje que no explica por qué estaba el botón.
                        for h in self.remote_hosts.iter().filter(|h| h.protocol.can_shell()) {
                            let tipo = if h.protocol == lucy_core::hosts::Protocol::Winrm {
                                "WinRM"
                            } else {
                                "SSH"
                            };
                            if lv_opcion(ui, &h.name, tipo, self.lv_host == h.id) {
                                elegido = Some(h.id.clone());
                            }
                        }
                    });
            });

        // Fuera del menú se cierra. Sin esto hay que volver a pulsar el botón,
        // que es justo donde no está el ratón cuando se decide no cambiar nada.
        if ui.input(|i| i.pointer.any_click()) && !boton.clicked() && elegido.is_none() {
            let dentro = ui.ctx().pointer_latest_pos().is_some_and(|p| {
                ui.ctx()
                    .memory(|m| m.area_rect(egui::Id::new("lv-host-menu")))
                    .is_some_and(|r| r.contains(p))
            });
            if !dentro {
                self.lv_host_menu = false;
            }
        }
        match elegido {
            Some(id) => {
                self.lv_host_menu = false;
                let cambio = id != self.lv_host;
                self.lv_host = id;
                if !cambio {
                    return false;
                }
                // Las filas eran del equipo anterior. Dejarlas mientras se lee el
                // nuevo enseñaría el log de una máquina bajo el nombre de otra,
                // que es peor que no enseñar nada.
                self.lv_rows.clear();
                self.lv_files.clear();
                self.lv_error.clear();
                if !self.lv_path.trim().is_empty() {
                    return true;
                }
                // Sin ruta escrita, elegir equipo EXPLORA. Quien abre este
                // desplegable ha venido a mirar los logs de esa máquina, y
                // dejarle un campo en blanco delante es devolverle la pregunta
                // que traía — cuál era la ruta.
                let h = self.remote_hosts.iter().find(|x| x.id == self.lv_host).cloned();
                if let Some(h) = h {
                    if let Some((_, dir)) = lucy_core::logs::common_dirs(&h).first() {
                        self.lv_explorar(&h, dir);
                    }
                }
                false
            }
            None => false,
        }
    }

    pub(crate) fn lv_barra(&mut self, ui: &mut egui::Ui) {
        let (e, w, i) = lv_cuenta(&self.lv_rows);
        let total = self.lv_rows.len();
        ui.add_space(8.0);
        row_align(ui, 28.0, egui::Align::Center, |ui| {
            let chips: [(&str, usize, Option<lucy_core::logs::Level>); 4] = [
                ("Todos", total, None),
                ("Error", e, Some(lucy_core::logs::Level::Error)),
                ("Warn", w, Some(lucy_core::logs::Level::Warn)),
                ("Info", i, Some(lucy_core::logs::Level::Info)),
            ];
            for (label, n, nivel) in chips {
                if lv_chip(ui, i18n::tr(label), n, self.lv_filter == nivel) {
                    self.lv_filter = nivel;
                }
                ui.add_space(6.0);
            }
            ui.add_space(6.0);
            ui.add_sized(
                [ui.available_width().clamp(160.0, 520.0), 26.0],
                egui::TextEdit::singleline(&mut self.lv_query).hint_text(i18n::tr("⌕  Filtrar mensajes…")),
            );
        });
    }

    pub(crate) fn lv_stream(&mut self, ui: &mut egui::Ui) {
        let visibles = lv_filtrar(&self.lv_rows, self.lv_filter, &self.lv_query);
        if visibles.is_empty() {
            // ENVUELTO DONDE ESTÁ EL LITERAL, no donde se pinta. Las dos
            // formas funcionan igual en ejecución, pero solo esta la ve el test
            // que comprueba que toda frase envuelta tenga traducción: con
            // `tr(msg)` sobre una variable no hay forma de saber desde el
            // fuente qué cadena acaba pasando por ahí. Y estos vacíos se colaron
            // en español precisamente por eso.
            let msg = if self.lv_rows.is_empty() {
                match self.lv_mode {
                    LvMode::Auditoria => i18n::tr("Sin actividad registrada."),
                    LvMode::Archivo if self.lv_path.trim().is_empty() => {
                        i18n::tr("Escribe la ruta de un fichero y pulsa Enter.")
                    }
                    LvMode::Archivo => i18n::tr("El fichero no tiene líneas."),
                }
            } else {
                i18n::tr("Sin coincidencias.")
            };
            ui.centered_and_justified(|ui| {
                ui.label(
                    // EL `tr` VA AQUI Y NO EN EL `let`. `msg` sale de una
                    // cadena de `if` con cuatro literales, y envolver cada uno
                    // seria envolver cuatro veces lo que se pinta una. Ademas
                    // este es el patron que ningun escaner de lineas ve: el
                    // literal se asigna arriba y se pinta quince lineas abajo.
                    egui::RichText::new(msg)
                        .monospace()
                        .size(theme::FS_FOOTNOTE)
                        .color(theme::faint()),
                );
            });
            return;
        }

        // POR FILAS Y NO PINTÁNDOLAS TODAS. Son hasta dos mil líneas y el visor
        // se relee cada cinco segundos: dibujar las dos mil en cada frame es lo
        // que convierte una vista de texto en una que tira la tasa de refresco.
        // `show_rows` pide altura fija, así que las líneas no se parten — se
        // recortan a lo ancho y la entera se lee en el globo al pasar por encima.
        // EL EQUIPO SE COLAPSA CUANDO ES EL MISMO EN TODAS. Con una sola máquina
        // —el caso normal— la columna repetía «WORSKTATION-1…» en cada fila:
        // noventa y seis píxeles gastados en decir lo mismo veinte veces,
        // mientras el mensaje se cortaba por la derecha. Si hay varios equipos la
        // columna vuelve, porque entonces sí distingue.
        let mut hosts = visibles
            .iter()
            .map(|i| self.lv_rows[*i].host.as_str())
            .filter(|h| !h.is_empty());
        let primero = hosts.next();
        let unico = primero.filter(|p| hosts.all(|h| h == *p)).map(str::to_string);
        if let Some(h) = &unico {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                insignia(ui, h, true);
            });
            ui.add_space(4.0);
        }

        let alto = 20.0_f32;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
            ui,
            alto,
            visibles.len(),
            |ui, rango| {
                // El día de la fila ANTERIOR a la primera visible, para que el
                // separador no salga otra vez al desplazarse dentro de un mismo
                // día — y para que sí salga si el corte del desplazamiento cae
                // justo entre dos.
                let mut dia_previo = rango
                    .start
                    .checked_sub(1)
                    .and_then(|p| visibles.get(p))
                    .map(|i| self.lv_rows[*i].dia.clone())
                    .unwrap_or_default();
                for k in rango {
                    let r = &self.lv_rows[visibles[k]];
                    // LA LÍNEA ENTRE DÍAS. La lista salta de 03:31 a 17:07 sin
                    // avisar de que son días distintos, y eso se lee como que
                    // pasaron catorce horas esta madrugada.
                    if !r.dia.is_empty() && r.dia != dia_previo {
                        if !dia_previo.is_empty() {
                            ui.horizontal(|ui| {
                                ui.set_height(alto);
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(&r.dia)
                                        .monospace()
                                        .size(theme::FS_MICRO)
                                        .color(theme::faint()),
                                );
                                let y = ui.min_rect().center().y;
                                let x = ui.min_rect().right() + 8.0;
                                ui.painter().hline(
                                    x..=ui.max_rect().right(),
                                    y,
                                    egui::Stroke::new(1.0_f32, theme::bdr()),
                                );
                            });
                        }
                        dia_previo = r.dia.clone();
                    }
                    let col = match r.lv {
                        lucy_core::logs::Level::Error => theme::red(),
                        lucy_core::logs::Level::Warn => theme::amber(),
                        lucy_core::logs::Level::Info => theme::txt3(),
                    };
                    ui.horizontal(|ui| {
                        ui.set_height(alto);
                        if !r.t.is_empty() {
                            ui.add_sized(
                                [58.0, alto],
                                egui::Label::new(
                                    egui::RichText::new(&r.t)
                                        .monospace()
                                        .size(theme::FS_CAPTION)
                                        .color(theme::faint()),
                                )
                                .truncate(),
                            );
                        }
                        ui.add_sized(
                            [46.0, alto],
                            // ERROR, WARN e INFO NO se traducen: son los nombres
                            // del nivel tal y como los escribe el propio log, y
                            // una fila que dijera FEHLER junto a una linea del
                            // fichero que dice ERROR se lee peor que las dos en
                            // ingles.
                            egui::Label::new(
                                egui::RichText::new(match r.lv {
                                    lucy_core::logs::Level::Error => "ERROR",
                                    lucy_core::logs::Level::Warn => "WARN",
                                    lucy_core::logs::Level::Info => "INFO",
                                })
                                .monospace()
                                .size(theme::FS_CAPTION)
                                .color(col),
                            )
                            .truncate(),
                        );
                        // El equipo solo si hay más de uno: si es siempre el
                        // mismo ya está dicho arriba, en la insignia.
                        if unico.is_none() && !r.host.is_empty() {
                            ui.add_sized(
                                [96.0, alto],
                                egui::Label::new(
                                    egui::RichText::new(&r.host)
                                        .monospace()
                                        .size(theme::FS_CAPTION)
                                        .color(theme::txt3()),
                                )
                                .truncate(),
                            );
                        }
                        if !r.src.is_empty() {
                            ui.add_sized(
                                [72.0, alto],
                                egui::Label::new(
                                    egui::RichText::new(&r.src)
                                        .monospace()
                                        .size(theme::FS_CAPTION)
                                        .color(theme::txt3()),
                                )
                                .truncate(),
                            );
                        }
                        let resp = ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new(&r.m)
                                        .monospace()
                                        .size(theme::FS_CAPTION)
                                        .color(theme::txt2()),
                                )
                                .truncate()
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_text(&r.m);
                        // Un clic copia la línea entera. Es la operación que se
                        // hace de verdad con una línea de log —pegarla en un
                        // ticket o en un buscador— y con el texto recortado a lo
                        // ancho seleccionarlo a mano no la daría completa.
                        if resp.clicked() {
                            ui.ctx().copy_text(format!(
                                "{}{}{}",
                                if r.t.is_empty() { String::new() } else { format!("{}  ", r.t) },
                                if r.src.is_empty() {
                                    String::new()
                                } else {
                                    format!("{}  ", r.src)
                                },
                                r.m
                            ));
                        }
                    });
                }
            },
        );
    }

    /// Lo visible, en texto plano, para el portapapeles.
    pub(crate) fn lv_texto_visible(&self) -> String {
        lv_filtrar(&self.lv_rows, self.lv_filter, &self.lv_query)
            .into_iter()
            .map(|i| {
                let r = &self.lv_rows[i];
                let nivel = match r.lv {
                    lucy_core::logs::Level::Error => "ERROR",
                    lucy_core::logs::Level::Warn => "WARN ",
                    lucy_core::logs::Level::Info => "INFO ",
                };
                format!("{:>8}  {nivel}  {:<14}  {}", r.t, r.src, r.m)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Abre el visor de logs sobre un equipo concreto, ya explorando.
    ///
    /// Es lo que se pide desde NexShell. Deja la vista lista para elegir fichero
    /// —modo Archivo, ese equipo, y la primera carpeta sugerida ya buscándose—
    /// en vez de dejar la ruta en blanco: llegar aquí con un campo vacío es el
    /// mismo callejón del que venimos, solo que dos clics más allá.
    pub(crate) fn lv_ir_a_equipo(&mut self, id: &str) {
        let Some(h) = self.remote_hosts.iter().find(|x| x.id == id).cloned() else { return };
        self.view = View::LogViewer;
        self.lv_mode = LvMode::Archivo;
        self.lv_host = id.to_string();
        self.lv_host_menu = false;
        // Lo que hubiera era de otro equipo o de la auditoría. Dejarlo mientras
        // se explora el nuevo enseñaría el log de una máquina bajo el nombre de
        // otra, que es peor que no enseñar nada.
        self.lv_rows.clear();
        self.lv_files.clear();
        self.lv_path.clear();
        self.lv_error.clear();
        self.lv_last.clear();
        if let Some((_, dir)) = lucy_core::logs::common_dirs(&h).first() {
            self.lv_explorar(&h, dir);
        }
    }

    /// Trae lo que toque según el modo. Es el único sitio que escribe `lv_rows`.
    pub(crate) fn lv_cargar(&mut self) {
        self.lv_error.clear();
        match self.lv_mode {
            LvMode::Auditoria => self.lv_cargar_auditoria(),
            LvMode::Archivo if self.lv_host.is_empty() => self.lv_cargar_local(),
            LvMode::Archivo => self.lv_cargar_remoto(),
        }
        self.lv_next = Instant::now() + LV_POLL;
    }

    pub(crate) fn lv_cargar_auditoria(&mut self) {
        // El esquema se asegura en cada carga y no una vez al arrancar: es un
        // `CREATE TABLE IF NOT EXISTS` sobre una base ya abierta —microsegundos—
        // y cubre el caso de que la base se cree después de arrancar Lucy, que
        // es lo que pasa en una instalación nueva.
        if let Err(e) = lucy_core::audit::ensure_schema() {
            self.lv_error = e;
            return;
        }
        // El resumen viaja con las filas: son la misma carga y separarlos dejaría
        // una ventana en la que la cabecera habla de una lista que ya cambió.
        self.lv_resumen = lucy_core::audit::resumen(30).ok();
        match lucy_core::audit::query(&lucy_core::audit::Filter::default()) {
            Ok(filas) => {
                self.lv_rows = filas
                    .iter()
                    .map(|e| LvRow {
                        t: lv_hora_de(e.created_at, &e.timestamp),
                        dia: lv_dia_de(e.created_at),
                        lv: lucy_core::audit::level_of(e),
                        host: e.host_name.clone(),
                        src: e.source.clone(),
                        // El comando es la fila; la salida solo si no hay
                        // comando. Enseñar los dos juntos llenaría la línea de
                        // volcado y taparía justo lo que se busca.
                        m: if e.command.is_empty() {
                            e.output_preview.clone()
                        } else {
                            e.command.clone()
                        },
                    })
                    .collect();
                self.lv_last = lv_hora();
            }
            Err(e) => {
                self.lv_rows.clear();
                self.lv_error = e;
            }
        }
    }

    pub(crate) fn lv_cargar_local(&mut self) {
        let ruta = self.lv_path.trim().to_string();
        if ruta.is_empty() {
            self.lv_rows.clear();
            self.lv_last.clear();
            return;
        }
        // En el hilo de la interfaz: es un fichero de disco local con tope de
        // líneas, milisegundos. Lo que justificó un hilo —una sesión remota— es
        // el otro camino, y va por hilo.
        match lucy_core::logs::tail(std::path::Path::new(&ruta), LV_LINES) {
            Ok(l) => self.lv_absorber(l, i18n::tr("este equipo")),
            Err(e) => {
                self.lv_rows.clear();
                self.lv_error = format!("No se pudo leer «{ruta}»: {e}");
                self.lv_last.clear();
            }
        }
    }

    pub(crate) fn lv_cargar_remoto(&mut self) {
        // Una lectura en vuelo cada vez. Dos sesiones simultáneas contra el
        // mismo servidor no traen la respuesta antes: la traen dos veces.
        if self.lv_rx.is_some() {
            return;
        }
        let ruta = self.lv_path.trim().to_string();
        if ruta.is_empty() {
            self.lv_rows.clear();
            self.lv_last.clear();
            return;
        }
        let Some(h) = self.remote_hosts.iter().find(|h| h.id == self.lv_host).cloned() else {
            self.lv_error = i18n::tr("Ese equipo ya no está dado de alta.").into();
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // La contraseña se saca DENTRO del hilo: leer el almacén de
            // credenciales de Windows abre un diálogo del sistema la primera
            // vez, y eso en el hilo de la interfaz congela la ventana.
            let pw = lucy_core::hosts::password(&h.id).unwrap_or_default();
            let _ = tx.send(lucy_core::logs::tail_remote(&h, &pw, &ruta, LV_LINES));
        });
        self.lv_rx = Some(rx);
        self.lv_desde = Some(Instant::now());
    }

    /// Convierte líneas crudas en filas.
    pub(crate) fn lv_absorber(&mut self, lineas: Vec<String>, origen: &str) {
        // AL REVÉS: lo más reciente arriba, como la auditoría. `tail` devuelve
        // en orden de lectura —lo último al final— y mezclar los dos criterios
        // en la misma lista haría que cambiar de modo diera la vuelta a la
        // pantalla sin avisar.
        self.lv_rows = lineas
            .into_iter()
            .rev()
            .map(|l| LvRow {
                t: String::new(),
                // Un fichero de log no trae fecha en columna aparte —viene
                // dentro de la linea— ni equipo: lo que se lee es UN fichero.
                dia: String::new(),
                lv: lucy_core::logs::Level::sniff(&l),
                host: String::new(),
                src: origen.to_string(),
                m: l,
            })
            .collect();
        self.lv_last = lv_hora();
    }
}
