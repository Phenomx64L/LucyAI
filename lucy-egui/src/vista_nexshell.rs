//! NexShell: conexion, bloques, ejecucion y analisis.
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
    /// La sesión contra un equipo remoto.
    ///
    /// SIN PTY. Un WinRM no es una sesión viva: cada `Invoke-Command` va y
    /// vuelve. Así que aquí las líneas se acumulan en una lista propia en vez de
    /// pasar por el emulador VT — fingir un terminal sobre algo que no lo es
    /// daría un cursor que no significa nada y un Ctrl+C que no llega.
    pub(crate) fn nx_remote(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.nx_host.clone() else { return };
        let Some(h) = self.remote_hosts.iter().find(|x| x.id == id).cloned() else {
            self.nx_host = None;
            return;
        };
        // Se llama a la puerta SOLO la primera vez que se abre el equipo. Ese
        // viaje contesta las tres preguntas de una: si se llega, si valen las
        // credenciales, y qué sistema corre — y cada conexión de WinRM paga su
        // autenticación, que es la parte lenta.
        if h.protocol.can_shell() && !self.nx_estado.contains_key(&h.id) {
            self.nx_conectar(&h);
        }
        row_align(ui, 28.0, egui::Align::Center, |ui| {
            ui.spacing_mut().item_spacing.x = 7.0;
            icons::show(ui, icons::Icon::Server, 15.0, color_hex(&h.color).unwrap_or(theme::acc()));
            ui.label(egui::RichText::new(&h.name).size(theme::FS_FOOTNOTE).color(theme::txt()));
            ui.label(theme::instrument_label(h.protocol.label(), theme::faint()));
            // El sistema MEDIDO, no el declarado. Es lo que decide si la
            // traducción propone `dnf` o `apt`, y verlo aquí es lo que permite
            // notar que el equipo no es lo que uno creía.
            match self.nx_estado.get(&h.id) {
                Some(Conexion::Ok { os, ms }) => {
                    ui.label(
                        egui::RichText::new(format!("{os} · {ms} ms"))
                            .size(theme::FS_MICRO)
                            .color(theme::faint()),
                    );
                }
                Some(Conexion::Desconectado) => {
                    ui.label(
                        egui::RichText::new(i18n::tr("desconectado"))
                            .size(theme::FS_MICRO)
                            .color(theme::faint()),
                    );
                }
                Some(Conexion::Fallo(e)) => {
                    // En la barra, corto; entero al pasar el ratón y entero en
                    // la sesión. Un mensaje de WinRM son varias líneas y aquí
                    // solo cabe una — dejarlo suelto lo cortaba por donde
                    // tocara, que fue justo lo que escondió el motivo real.
                    ui.label(
                        egui::RichText::new(recorta_visual(e, 60))
                            .size(theme::FS_MICRO)
                            .color(theme::red()),
                    )
                    .on_hover_text(e);
                }
                Some(Conexion::Probando) => {
                    ui.label(
                        egui::RichText::new(i18n::tr("conectando…"))
                            .size(theme::FS_MICRO)
                            .color(theme::amber()),
                    );
                    ui.ctx().request_repaint();
                }
                None => {}
            }
            // EL BOTÓN QUE FALTABA. En mi diseño cada comando abre su propia
            // conexión —WinRM es así— y por eso no había nada que «conectar».
            // Pero eso deja al operador a ciegas: lo primero que hace uno con un
            // equipo recién dado de alta es comprobar que responde, y sin botón
            // la única forma era mandarle un comando a ver qué pasaba.
            //
            // No abre una sesión persistente, y la V2 tampoco: llama a la puerta
            // y enciende la luz.
            // ── EL BOTÓN DICE EN QUÉ ESTADO ESTÁ, y no siempre lo mismo ──────
            //
            // Ponía «Conectar» pase lo que pase, también con el equipo ya
            // respondiendo — y entonces el único control visible de la cabecera
            // ofrece hacer algo que ya está hecho. Reportado: «al momento de
            // darle en conectar, el botón se queda así con la misma acción».
            //
            // «DESCONECTAR» Y NO «CERRAR SESIÓN», Y LA PALABRA IMPORTA. Aquí no
            // hay sesión que cerrar: WinRM no la abre —cada `Invoke-Command` va
            // y vuelve— y la V2 tampoco. Lo único que existe es lo que Lucy
            // RECUERDA de haber llamado a la puerta: el sistema que contestó, lo
            // que tardó y el punto encendido del carril. Eso es lo que se
            // suelta. Llamarlo «cerrar sesión» prometería que al otro lado se
            // termina algo, y no se termina nada.
            let estado = self.nx_estado.get(&h.id);
            let (rotulo, ayuda, conectado) = match estado {
                Some(Conexion::Probando) => ("Conectando…", "", false),
                Some(Conexion::Ok { .. }) => (
                    "Desconectar",
                    "Olvida lo que se sabe de este equipo. WinRM no mantiene una \
                     sesión abierta, así que no hay nada que cerrar al otro lado.",
                    true,
                ),
                Some(Conexion::Desconectado) => (
                    "Conectar",
                    "Comprobar que responde y con qué sistema",
                    false,
                ),
                Some(Conexion::Fallo(_)) => (
                    "Reintentar",
                    "Volver a llamar a la puerta",
                    false,
                ),
                None => ("Conectar", "Comprobar que responde y con qué sistema", false),
            };
            let probando = matches!(estado, Some(Conexion::Probando));
            let pulsado = ui
                .add_enabled(!probando, egui::Button::new(i18n::tr(rotulo)).small())
                .on_hover_text(i18n::tr(ayuda))
                .clicked();
            if pulsado {
                if conectado {
                    // SE ANOTA, NO SE BORRA. Ver `Conexion::Desconectado`:
                    // quitar la entrada devolvia el equipo a «no se nada de el»
                    // y la vista lo reconectaba sola al fotograma siguiente.
                    self.nx_estado.insert(h.id.clone(), Conexion::Desconectado);
                    // Y SE CORTA LO QUE HUBIERA EN VUELO. Desconectar de una
                    // maquina tiene que parar lo que corre contra ella: es lo
                    // que significa. Sin esto, la cabecera decia «desconectado»
                    // y «traduciendo… 64s» a la vez.
                    let motivo = i18n::tr(
                        "Desconectado, y no se vuelve a conectar solo. Lo de arriba se queda.",
                    )
                    .to_string();
                    let hid = h.id.clone();
                    self.nx_suelta(&hid, &motivo);
                } else {
                    self.nx_conectar(&h);
                }
            }
            // ── QUÉ ESTÁ HACIENDO, Y NO SOLO QUE ESTÁ OCUPADA ────────────────
            //
            // Decía «ejecutando…» para cuatro cosas distintas —traducir una
            // frase, ejecutar el comando, leer un error, leer una salida larga—
            // y las dos últimas ni siquiera encendían el indicador: ocurrían en
            // un hilo y la ventana se quedaba quieta.
            //
            // La condición es `nx_fase` y ya no `nx_busy`, porque `nx_busy` solo
            // cubre las dos primeras. Es lo que hace visibles las otras dos.
            if let Some((fase, desde)) = self.nx_fase {
                ui.add_space(8.0);
                // El girador: cuatro palos que dan la vuelta cada segundo. Es lo
                // único de la cabecera que se mueve, y por eso es lo que
                // distingue «está pensando» de «se colgó» sin leer nada.
                const PALOS: [&str; 4] = ["|", "/", "—", "\\"];
                let paso = (desde.elapsed().as_millis() / 250) as usize % PALOS.len();
                let s = desde.elapsed().as_secs();
                ui.label(
                    egui::RichText::new(i18n::trf(
                        "{palo} {fase}… {s}s",
                        &[("palo", PALOS[paso]), ("fase", i18n::tr(fase)), ("s", &s.to_string())],
                    ))
                    .size(theme::FS_CAPTION)
                    .color(theme::acc()),
                );
                // Sin esto el girador solo gira cuando algo más pide repintado, y
                // un indicador de actividad que se congela dice lo contrario de
                // lo que viene a decir.
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
            }
            if self.nx_busy {
                if ui.add(egui::Button::new(i18n::tr("■ Detener")).small()).clicked() {
                    // Mata el proceso, no solo deja de mirarlo: al otro lado hay
                    // un comando corriendo en una máquina de verdad, y dejar de
                    // leer no lo para.
                    self.nx_stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    // Y SI LO QUE ESTÁ EN VUELO ES UNA TRADUCCIÓN, se suelta la
                    // pantalla aquí mismo. Ahí no hay proceso que matar: hay una
                    // petición a un modelo que puede tardar en contestar o no
                    // contestar nunca. Levantar la bandera y esperar dejaba el
                    // botón sin efecto visible, que es como no tener botón.
                    //
                    // Soltar el receptor descarta la respuesta si llega tarde,
                    // que es lo correcto: el operador ya dijo que no la quería.
                    // POR `nx_suelta`, EL MISMO SITIO QUE DESCONECTAR. Aquí
                    // había una lista propia de campos a limpiar y allí no
                    // había ninguna: dos listas son dos sitios donde olvidar
                    // uno, y el que se olvidó fue el de desconectar entero.
                    if self.nx_rx.is_some() || self.nx_diag_rx.is_some() {
                        let hid = h.id.clone();
                        let motivo = i18n::tr("(detenido)").to_string();
                        self.nx_suelta(&hid, &motivo);
                    }
                }
                ui.ctx().request_repaint();
            }
            // ── LA VÍA QUE PROPONE LUCY TRAS UN FALLO ────────────────────────
            //
            // UN BOTÓN Y NO UNA EJECUCIÓN. La V1 a veces lo lanzaba sola; aquí
            // no. Al otro lado hay un servidor y una credencial guardada, y el
            // comando que propone un modelo sobre un error que acaba de leer no
            // tiene por qué ser inocuo. Pulsarlo pasa por `nx_gate_remote`, con
            // su guardrail y su confirmación de lo destructivo — la misma puerta
            // que si lo hubiera escrito el operador.
            //
            // Solo mientras no haya nada corriendo: ofrecer «probar esto» con un
            // comando en vuelo invita a lanzar dos cosas a la vez.
            let sugerido = self
                .nx_sugerido
                .as_ref()
                .filter(|(hid, _)| *hid == h.id && !self.nx_busy)
                .map(|(_, c)| c.clone());
            if let Some(cmd) = sugerido {
                ui.add_space(8.0);
                // El comando ENTERO en el globo, y recortado en el botón: la
                // decisión de lanzarlo se toma leyéndolo, no adivinándolo.
                if ui
                    .add(egui::Button::new(i18n::trf(
                        "▷ Probar: {cmd}",
                        &[("cmd", &recorta_visual(&cmd, 34))],
                    )))
                    .on_hover_text(&cmd)
                    .clicked()
                {
                    self.nx_sugerido = None;
                    let host = h.clone();
                    // Lo escribio Lucy y lo acepta una persona: el caso central
                    // de `aceptacion()`.
                    self.nx_gate_remote(&host, cmd, "ai");
                }
                if ui
                    .add(egui::Button::new("✕").small())
                    .on_hover_text(i18n::tr("Descartar la sugerencia"))
                    .clicked()
                {
                    self.nx_sugerido = None;
                }
            }
            right(ui, 24.0, |ui| {
                if ui.add(egui::Button::new("⌫").small()).on_hover_text(i18n::tr("Limpiar")).clicked() {
                    self.nx_lines.remove(&h.id);
                }
                if ui
                    .add(egui::Button::new("⧉").small())
                    .on_hover_text(i18n::tr("Copiar la salida"))
                    .clicked()
                {
                    let t = self
                        .nx_lines
                        .get(&h.id)
                        .map(|v| {
                            v.iter()
                                .map(|(c, t)| if *c == 'c' { format!("❯ {t}") } else { t.clone() })
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    ui.output_mut(|o| o.copied_text = t);
                }
            });
        });
        ui.add_space(6.0);

        let mut enviar = false;
        egui::TopBottomPanel::bottom("nx-remote-input")
            .frame(egui::Frame::none().inner_margin(egui::Margin {
                top: 8.0,
                bottom: 4.0,
                ..Default::default()
            }))
            .show_separator_line(false)
            .show_inside(ui, |ui| {
                if let Some(p) = confirm_strip(ui, &mut self.nx_confirm, Some(&h.id)) {
                    self.nx_run_remote(&h, &p.cmd, p.origen);
                }
                egui::Frame::none()
                    .fill(theme::bg3())
                    .stroke(egui::Stroke::new(1.0_f32, theme::bdr()))
                    .rounding(egui::Rounding::same(theme::R_MD))
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        row_align(ui, 26.0, egui::Align::Center, |ui| {
                            ui.spacing_mut().item_spacing.x = 9.0;
                            icons::show(ui, icons::Icon::Terminal, 15.0, theme::txt3());
                            // Las flechas, ANTES de dibujar el campo, o el
                            // propio `TextEdit` las usa para mover el cursor. Lo
                            // tenía en el equipo local y no lo llevé aquí, así
                            // que en un remoto —donde los comandos son más
                            // largos y se repiten más— no había historial.
                            let id = ui.make_persistent_id("nx-remote-field");
                            if ui.memory(|m| m.has_focus(id)) {
                                let (arriba, abajo) = ui.input_mut(|i| {
                                    (
                                        i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                                        i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                                    )
                                });
                                if arriba || abajo {
                                    self.nx_recall(if arriba { -1 } else { 1 });
                                }
                            }
                            let te = ui.add(
                                egui::TextEdit::singleline(&mut self.term_input)
                                    .id(id)
                                    // La pista cambia con el estado, porque el
                                    // significado de la tecla cambia con él: con
                                    // algo corriendo, lo que escribes es una
                                    // respuesta PARA ese comando.
                                    .hint_text(if self.nx_busy {
                                        i18n::tr("Respuesta para el comando en curso (p. ej. y) …")
                                            .to_string()
                                    } else {
                                        i18n::trf("Comando o petición para {equipo}…", &[("equipo", &h.name)])
                                    })
                                    .desired_width(ui.available_width() - 34.0)
                                    .frame(false)
                                    .font(egui::FontId::monospace(theme::FS_FOOTNOTE)),
                            );
                            if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                enviar = true;
                                te.request_focus();
                            }
                        });
                    });
            });

        // FUERA DEL PINTADO, porque dentro se esta leyendo `nx_lines` prestado y
        // relanzar escribe en el. Se anota que hay que hacer y se hace despues,
        // que es el mismo patron que usa `forzar` en la vista de Mantenimiento.
        let mut repetir: Option<String> = None;
        egui::Frame::none()
            .fill(theme::bg())
            .stroke(egui::Stroke::new(1.0_f32, theme::bdr()))
            .rounding(egui::Rounding::same(theme::R_MD))
            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        // Las de ESTE equipo. Sin la clave, la lista era una sola
                        // y la salida del servidor anterior aparecía aquí como
                        // si fuera de éste.
                        let vacio = Vec::new();
                        let lineas = self.nx_lines.get(&h.id).unwrap_or(&vacio);
                        if lineas.is_empty() {
                            ui.add_space(30.0);
                            ui.vertical_centered(|ui| {
                                icons::show(ui, icons::Icon::Server, 26.0, theme::faint());
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new(i18n::trf("Listo para operar en {equipo}", &[("equipo", &h.name)]))
                                        .size(theme::FS_HEADING)
                                        .color(theme::txt2()),
                                );
                            });
                        }
                        // ── EN BLOQUES, NO EN UN CHORRO DE RENGLONES ────────
                        //
                        // Cada petición con su salida dentro de un marco, con una
                        // franja a la izquierda que dice cómo acabó. Ver
                        // `bloques`: lo que se arregla es que todo pesara lo
                        // mismo — la frase, el comando, doscientas líneas de
                        // volcado y la conclusión de Lucy salían como renglones
                        // iguales, y encontrar dónde empezaba una cosa obligaba a
                        // leerlo todo.
                        let grupos = bloques(lineas);
                        let ultimo = grupos.len().saturating_sub(1);
                        for (n, b) in grupos.iter().enumerate() {
                            let trozo = &lineas[b.desde..b.hasta];
                            // El estado, que es lo que colorea la franja:
                            //   rojo   si hay alguna línea de error dentro
                            //   acento si es el bloque de abajo y algo corre
                            //   apagado en lo demás
                            let hay_error = trozo.iter().any(|(m, _)| *m == 'e');
                            let corriendo = n == ultimo && self.nx_fase.is_some();
                            // El preámbulo —conectar, conectado— no es una
                            // petición y no lleva marco: enmarcarlo le daría el
                            // mismo peso que a un comando.
                            let es_peticion = matches!(trozo.first(), Some(('p', _)) | Some(('c', _)));
                            let franja = if corriendo {
                                theme::acc()
                            } else if hay_error {
                                theme::red()
                            } else {
                                theme::bdr()
                            };
                            ui.add_space(if es_peticion { 8.0 } else { 2.0 });
                            // ── LA ENTRADA DEL BLOQUE ───────────────────────
                            //
                            // Aparece subiendo y aclarándose. Es la única
                            // animación de esta pantalla y va donde de verdad
                            // dice algo: que ha llegado ALGO NUEVO. Con la
                            // transcripción pegada abajo, un bloque que aparece
                            // de golpe es indistinguible de un salto del scroll.
                            //
                            // POR LA PUERTA DE LA CASA. `motion()` es el ajuste
                            // de Configuración y `LUCY_NO_MOTION=1`; una
                            // animación que no se puede apagar es una animación
                            // que alguien va a tener que soportar.
                            //
                            // La identidad lleva el equipo Y el índice: sin el
                            // equipo, cambiar de máquina reutilizaría la
                            // animación del bloque que ocupaba ese sitio y los
                            // de la nueva entrarían ya hechos.
                            let id_bloque = ui.id().with((&h.id, b.desde));
                            let entrada = if motion() && es_peticion {
                                theme::ease_out(ui.ctx().animate_bool_with_time(
                                    id_bloque,
                                    true,
                                    theme::DUR_SLOW,
                                ))
                            } else {
                                1.0
                            };
                            let dibuja = |ui: &mut egui::Ui| {
                                if entrada < 1.0 {
                                    ui.multiply_opacity(entrada);
                                    // Sube ocho píxeles. Más se lee como un
                                    // sobresalto; menos no se percibe.
                                    ui.add_space((1.0 - entrada) * 8.0);
                                }
                        for (clase, texto) in trozo {
                            let (prefijo, color) = match *clase {
                                'c' => ("❯ ", theme::acc()),
                                // LO QUE PIDIO EL OPERADOR, en su propia marca.
                                // Lleva el mismo simbolo que un comando porque
                                // las dos cosas son «esto lo escribi yo», pero
                                // en un color mas apagado: lo que se ejecuto de
                                // verdad es la linea `c` que viene despues, y
                                // pintarlas iguales seria decir que la frase y
                                // el comando son lo mismo.
                                'p' => ("❯ ", theme::txt3()),
                                'e' => ("", theme::red()),
                                // Lo que dice LUCY, no el equipo: conectando,
                                // conectado, detenido. En otro color porque no
                                // es salida del comando, y leerlo como si lo
                                // fuera confunde sobre qué contestó el servidor.
                                'i' => ("", theme::txt3()),
                                _ => ("", theme::txt2()),
                            };
                            // Con salto de línea EXPLÍCITO. Un error de WinRM no
                            // cabe a lo ancho, y sin esto se sale del panel en
                            // vez de partirse — que es la otra mitad de por qué
                            // el motivo no se leía entero.
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("{prefijo}{texto}"))
                                        .monospace()
                                        .size(theme::FS_FOOTNOTE)
                                        .color(color),
                                )
                                .wrap(),
                            );
                        }
                            };
                            if es_peticion {
                                // LA FRANJA SE PINTA A MANO Y NO CON UN BORDE
                                // ENTERO. Un marco de cuatro lados sobre cada
                                // bloque llena la pantalla de cajas y compite con
                                // el del propio panel; una barra en el canto
                                // izquierdo dice dónde empieza cada cosa sin
                                // añadir ni una línea horizontal.
                                let r = egui::Frame::none()
                                    .inner_margin(egui::Margin {
                                        left: 10.0,
                                        right: 0.0,
                                        top: 4.0,
                                        bottom: 6.0,
                                    })
                                    .show(ui, dibuja)
                                    .response
                                    .rect;
                                // LA FRANJA LATE MIENTRAS CORRE. Un color fijo
                                // dice «este bloque es el último», no «esto está
                                // pasando ahora». El latido es lo que distingue
                                // lo uno de lo otro sin leer nada, y es la razón
                                // de que la animación esté aquí y no en un
                                // adorno: el canto izquierdo del bloque que
                                // trabaja es donde ya está mirando el ojo.
                                let viva = if corriendo && motion() {
                                    let t = ui.input(|i| i.time) as f32;
                                    // Entre 0,45 y 1: no se apaga del todo, para
                                    // que la franja no parpadee sino que respire.
                                    franja.gamma_multiply(0.45 + 0.55 * (0.5 + 0.5 * (t * 3.2).sin()))
                                } else {
                                    franja
                                };
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_max(
                                        egui::pos2(r.left(), r.top()),
                                        egui::pos2(r.left() + 2.0, r.bottom()),
                                    ),
                                    theme::capsule(2.0),
                                    viva,
                                );
                                if corriendo || entrada < 1.0 {
                                    ui.ctx().request_repaint_after(
                                        std::time::Duration::from_millis(16),
                                    );
                                }
                                // ── LAS ACCIONES DEL BLOQUE ─────────────────
                                //
                                // Solo al pasar el ratón por encima. Dos botones
                                // fijos por bloque en una transcripción de
                                // treinta serían sesenta controles compitiendo
                                // con el texto; al pasar por encima son dos, y
                                // están donde se está mirando.
                                //
                                // Se colocan con `put` sobre el rectángulo que
                                // el bloque ACABA de ocupar: el alto de un
                                // bloque no se sabe hasta haberlo pintado.
                                if ui.rect_contains_pointer(r) {
                                    let bw = 24.0;
                                    let caja = egui::Rect::from_min_size(
                                        egui::pos2(r.right() - bw * 2.0 - 6.0, r.top()),
                                        egui::vec2(bw * 2.0 + 2.0, 20.0),
                                    );
                                    let mut copiar = false;
                                    let mut relanzar = false;
                                    // `allocate_new_ui` y no `allocate_ui_at_rect`,
                                    // que egui 0.29 marca obsoleto.
                                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(caja), |ui| {
                                        ui.horizontal(|ui| {
                                            copiar = ui
                                                .add(egui::Button::new("⧉").small().frame(false))
                                                .on_hover_text(i18n::tr("Copiar este bloque"))
                                                .clicked();
                                            // RELANZAR SOLO SI HAY UN COMANDO Y
                                            // NADA EN VUELO. Ofrecerlo con algo
                                            // corriendo invita a lanzar dos cosas
                                            // a la vez sobre el mismo servidor.
                                            if trozo.iter().any(|(m, _)| *m == 'c')
                                                && self.nx_fase.is_none()
                                            {
                                                relanzar = ui
                                                    .add(
                                                        egui::Button::new("↻")
                                                            .small()
                                                            .frame(false),
                                                    )
                                                    .on_hover_text(i18n::tr("Volver a lanzarlo"))
                                                    .clicked();
                                            }
                                        });
                                    });
                                    if copiar {
                                        let t = trozo
                                            .iter()
                                            .map(|(m, x)| {
                                                if matches!(m, 'c' | 'p') {
                                                    format!("❯ {x}")
                                                } else {
                                                    x.clone()
                                                }
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n");
                                        ui.output_mut(|o| o.copied_text = t);
                                    }
                                    if relanzar {
                                        // El comando, no la frase: lo que se
                                        // relanza es lo que se ejecutó.
                                        if let Some((_, cmd)) =
                                            trozo.iter().find(|(m, _)| *m == 'c')
                                        {
                                            repetir = Some(cmd.clone());
                                        }
                                    }
                                }
                            } else {
                                dibuja(ui);
                            }
                        }
                    });
            });

        // EL RELANZAR, YA FUERA DEL PINTADO. Dentro se estaba leyendo `nx_lines`
        // prestado y esto escribe en él. Va por la misma puerta que un comando
        // escrito a mano —guardrail y confirmación de lo destructivo— porque es
        // exactamente eso: un comando que se lanza contra un servidor.
        if let Some(cmd) = repetir {
            let host = h.clone();
            // «Repetir» no es una propuesta nueva de Lucy: es una persona
            // relanzando algo. Contarlo como propuesta inflaria `aceptacion()`.
            self.nx_gate_remote(&host, cmd, "manual");
        }
        if enviar {
            let texto = std::mem::take(&mut self.term_input).trim().to_string();
            // CON UN COMANDO EN VUELO, lo que se escribe es una RESPUESTA para
            // él, no un comando nuevo. Es lo que hace falta para un `sudo` o un
            // «¿seguro? [y/N]», y sin ello el comando se queda esperando algo
            // que nadie le va a dar hasta que alguien lo mate.
            if self.nx_busy {
                if !texto.is_empty() {
                    self.nx_send_input(&h.id, &texto);
                }
            } else if !texto.is_empty() {
                self.nx_history.push(texto.clone());
                self.nx_hist_idx = None;
                // EL LENGUAJE NATURAL TAMBIÉN AQUÍ. Estaba solo en la ruta
                // local, así que la función que da nombre al módulo funcionaba
                // en la mitad de los sitios: en un remoto, escribir una frase
                // intentaba ejecutarla como comando.
                if lucy_core::nexshell::looks_like_command(&texto) {
                    self.nx_gate_remote(&h, texto, "manual");
                } else {
                    self.nx_translate(Some(h.clone()), texto);
                }
            }
        }
        // El bombeo ya NO se hace aquí: subió a `update`, para que un comando
        // remoto pueda terminar con el operador mirando otra pantalla.
    }

    /// Llama a la puerta de un equipo y deja el resultado a la vista.
    ///
    /// En otro hilo: un WinRM que no contesta tarda lo que tarde su tiempo de
    /// espera, y una ventana congelada mientras tanto es lo que esta migración
    /// existe para no tener.
    pub(crate) fn nx_conectar(&mut self, h: &lucy_core::hosts::Host) {
        self.nx_estado.insert(h.id.clone(), Conexion::Probando);
        let id = h.id.clone();
        self.nx_lines_mut(&id).push((
            'i',
            // POR LA TABLA. Era un `format!` suelto, asi que la primera
            // linea que ve el operador de un equipo remoto salia en español en
            // los cinco idiomas.
            i18n::trf(
                "Conectando a {equipo} ({host}:{puerto})…",
                &[
                    ("equipo", &h.name),
                    ("host", &h.host),
                    ("puerto", &h.port.to_string()),
                ],
            ),
        ));
        let (host, tx) = (h.clone(), self.nx_conn_tx.clone());
        std::thread::spawn(move || {
            let pw = lucy_core::hosts::password(&host.id).unwrap_or_default();
            let _ = tx.send((host.id.clone(), lucy_core::hosts::probe(&host, &pw)));
        });
    }

    /// Contesta a un comando remoto que está esperando algo.
    ///
    /// La respuesta se ECHA EN LA SESIÓN antes de mandarla, con su propia marca.
    /// Sin verla escrita, el operador no tiene forma de saber si la escribió
    /// bien — y en un `sudo` eso significa reintentar a ciegas.
    pub(crate) fn nx_send_input(&mut self, id: &str, texto: &str) {
        use std::io::Write;
        // El extremo llega por el canal en cuanto el proceso arranca. Se recoge
        // aquí y no en el pump para no tener que mirarlo en cada frame.
        if self.nx_stdin.is_none() {
            if let Some(rx) = &self.nx_stdin_rx {
                if let Ok(s) = rx.try_recv() {
                    self.nx_stdin = s;
                }
            }
        }
        let Some(s) = self.nx_stdin.as_mut() else {
            self.nx_lines_mut(id).push((
                'e',
                // POR LA TABLA. Salía en español con la interfaz en inglés, y es
                // de los que más se ven: cualquier frase escrita con un comando
                // en vuelo cae aquí.
                i18n::tr(
                    "Este comando no admite respuestas: WinRM no deja escribirle una vez \
                     lanzado. Detenlo y vuelve a lanzarlo sin la parte interactiva.",
                )
                .to_string(),
            ));
            return;
        };
        let r = writeln!(s, "{texto}").and_then(|()| s.flush());
        match r {
            Ok(()) => self.nx_lines_mut(id).push(('c', format!("↳ {texto}"))),
            Err(e) => self
                .nx_lines_mut(id)
                .push((
                    'e',
                    i18n::trf("No se pudo enviar la respuesta: {e}", &[("e", &e.to_string())]),
                )),
        }
    }

    /// Las líneas de ESTE equipo. Se crean al primer uso.
    ///
    /// POR EQUIPO Y NO UNA SOLA LISTA, que es como estaba y era un fallo:
    /// cambiabas del servidor A al B y seguías viendo la salida de A como si
    /// fuera suya. Es la misma clase de error que el workspace global de las
    /// pestañas —el que imprimía el resultado de una conversación en otra— y lo
    /// volví a cometer aquí.
    pub(crate) fn nx_lines_mut(&mut self, id: &str) -> &mut Vec<(char, String)> {
        self.nx_lines.entry(id.to_string()).or_default()
    }

    /// Escribe un aviso donde el operador lo esté mirando: la pantalla del
    /// remoto, o la sesión local a través del PTY.
    pub(crate) fn nx_aviso(&mut self, destino: Option<&lucy_core::hosts::Host>, texto: &str) {
        match destino {
            Some(h) => {
                let id = h.id.clone();
                self.nx_lines_mut(&id).push(('e', texto.to_string()));
            }
            None => self.nx_say(texto),
        }
    }

    /// La misma puerta que la local, para el camino remoto.
    ///
    /// LAS DOS COMPROBACIONES, Y AQUÍ IMPORTAN MÁS. En local, un comando que se
    /// escapa corre en la máquina que el operador tiene delante y cuya pantalla
    /// está mirando. En remoto corre en un servidor, con una credencial
    /// guardada, y lo único que vuelve son unas líneas de salida. Tener MENOS
    /// control en el camino que llega más lejos es exactamente al revés.
    pub(crate) fn nx_gate_remote(
        &mut self,
        h: &lucy_core::hosts::Host,
        cmd: String,
        origen: &'static str,
    ) {
        let g = lucy_core::guard::scan(&cmd, lucy_core::guard::Role::Assistant);
        if g.decision == lucy_core::guard::Decision::Block {
            self.nx_aviso(
                Some(h),
                &i18n::trf(
                    "Bloqueado por el guardrail: {motivo}",
                    &[("motivo", &g.reason)],
                ),
            );
            return;
        }
        if lucy_core::destructive::is_destructive(&cmd) {
            self.nx_confirm = Some(Pendiente { host: Some(h.id.clone()), cmd, origen });
        } else {
            self.nx_run_remote(h, &cmd, origen);
        }
    }

    /// Lanza un comando contra el equipo remoto, entregando la salida según
    /// llega.
    pub(crate) fn nx_run_remote(
        &mut self,
        h: &lucy_core::hosts::Host,
        cmd: &str,
        origen: &'static str,
    ) {
        let id = h.id.clone();
        self.nx_lines_mut(&id).push(('c', cmd.to_string()));
        let pw = lucy_core::hosts::password(&h.id).unwrap_or_default();
        let (host, script) = (h.clone(), cmd.to_string());
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.nx_busy = true;
        self.nx_stop = stop.clone();
        self.nx_exec_id = id;
        self.nx_origen = origen;
        self.nx_exec_rx = Some(rx);
        self.nx_started = Some(Instant::now());
        self.nx_fase = Some(("Ejecutando en el equipo", Instant::now()));
        let (in_tx, in_rx) = std::sync::mpsc::channel();
        self.nx_stdin = None;
        self.nx_stdin_rx = Some(in_rx);
        std::thread::spawn(move || {
            if let Err(e) = lucy_core::hosts::run_remote_streaming(
                // Sin plazo: es una terminal interactiva y un comando legítimo
                // puede tardar lo que quiera con el operador delante. El plazo
                // es para lo que corre solo, como el inventario.
                &host, &pw, &script, &tx, &stop, Some(&in_tx), None,
            ) {
                let _ = tx.send(lucy_core::hosts::Line::Err(e));
                let _ = tx.send(lucy_core::hosts::Line::Done(false));
            }
        });
    }

    /// Pregunta por qué falló un comando y qué probar en su lugar.
    ///
    /// ── POR SU PROPIO CANAL Y NO POR EL DE LA TRADUCCIÓN ─────────────────────
    ///
    /// `nx_rx` está para la traducción, y lo que vuelve por ahí SE EJECUTA: mira
    /// `pump_nx`, que manda el comando a `nx_gate_remote` en cuanto llega. Un
    /// diagnóstico que volviera por ese canal se ejecutaría solo, en un servidor
    /// remoto, sin que nadie lo haya leído. Eso es exactamente lo que no puede
    /// pasar aquí — así que canal aparte, y lo que llega SE ENSEÑA.
    ///
    /// El comando propuesto queda en `nx_sugerido`: un botón. Pulsarlo pasa por
    /// `nx_gate_remote`, con su guardrail y su confirmación de lo destructivo,
    /// igual que si lo hubiera escrito el operador. La V1 a veces lo lanzaba
    /// sola; aquí no, y la diferencia importa porque al otro lado hay un
    /// servidor con una credencial guardada.
    ///
    /// NO SE ENCADENA. Un solo intento por fallo: si el comando propuesto falla
    /// a su vez, se explica ese fallo pero no se pide otro sobre el anterior. Un
    /// bucle de diagnósticos que se diagnostican entre sí gasta dinero y no
    /// converge.
    pub(crate) fn nx_diagnostica(&mut self, id: &str, cmd: &str, salida: &str) {
        if self.nx_diag_rx.is_some() {
            return;
        }
        let (shell, so) = match self.nx_estado.get(id) {
            Some(Conexion::Ok { os, .. }) if !os.is_empty() => {
                (if os.contains("Windows") { "PowerShell" } else { "bash" }, os.clone())
            }
            _ => ("PowerShell", "Windows".to_string()),
        };
        self.nx_fase = Some(("Leyendo el error", Instant::now()));
        let prompt = lucy_core::nexshell::diagnose_prompt(cmd, salida, shell, &so);
        let modelo = self.chat_model.clone();
        let privado = self.privacy;
        let (tx, rx) = std::sync::mpsc::channel();
        self.nx_diag_rx = Some((id.to_string(), rx));
        std::thread::spawn(move || {
            let r = match lucy_core::cloud::allowed(&modelo, privado) {
                Ok(()) => {
                    let mut out = String::new();
                    for ev in lucy_core::cloud::start(
                        modelo,
                        vec![lucy_core::turns::Turn::user(prompt)],
                    ) {
                        if let lucy_core::chat::ChatEvent::Token(t) = ev {
                            out.push_str(&t);
                        }
                    }
                    lucy_core::nexshell::parse_diagnostico(&out)
                }
                // UN FALLO SE TRAGA. Esto es una ayuda sobre un error que el
                // operador ya está viendo; que además no se pueda explicar no
                // merece una segunda línea roja debajo de la primera.
                Err(_) => None,
            };
            let _ = tx.send(r);
        });
    }

    /// Suelta la pantalla y corta lo que hubiera en vuelo contra este equipo.
    ///
    /// ── UNO SOLO, PORQUE ANTES HABÍA MEDIO ───────────────────────────────────
    ///
    /// El botón de detener soltaba la traducción. Desconectar no soltaba nada:
    /// se anotaba el estado y el hilo que estaba hablando con el modelo seguía,
    /// con la cabecera diciendo «desconectado» y «traduciendo… 64s» a la vez.
    /// Reportado tal cual: «si lo desconecto intencionalmente se queda trabajando
    /// algo en segundo plano».
    ///
    /// Desconectar de una máquina TIENE que parar lo que corre contra ella: es
    /// lo que significa. Y como son las mismas cinco cosas que suelta el botón
    /// de detener, se hacen en un solo sitio — dos listas de campos a limpiar
    /// son dos sitios donde olvidar uno.
    ///
    /// SOLTAR EL RECEPTOR DESCARTA LA RESPUESTA si llega tarde, que es lo
    /// correcto: quien desconecta ya dijo que no la quería.
    pub(crate) fn nx_suelta(&mut self, id: &str, motivo: &str) {
        self.nx_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.nx_rx = None;
        self.nx_diag_rx = None;
        self.nx_exec_rx = None;
        self.nx_stdin = None;
        self.nx_stdin_rx = None;
        self.nx_busy = false;
        self.nx_started = None;
        self.nx_fase = None;
        self.nx_destino = None;
        self.nx_sugerido = None;
        if !motivo.is_empty() {
            self.nx_lines_mut(id).push(('i', motivo.to_string()));
        }
    }

    /// Lee lo que devolvió un comando y dice qué hay dentro.
    ///
    /// EL MISMO CANAL QUE EL DIAGNÓSTICO, y a propósito: los dos son «lo que
    /// Lucy tiene que decir sobre el último comando», nunca coinciden —o falló o
    /// salió bien— y compartir el canal es lo que garantiza que no se pisen. Dos
    /// canales serían dos formas de que llegaran dos comentarios a la vez.
    ///
    /// Se distingue por lo que trae: `causa` es el resumen, y `prueba` viene
    /// vacía porque de un comando que funcionó no hay nada que proponer.
    pub(crate) fn nx_resume(&mut self, id: &str, cmd: &str, salida: &str) {
        if self.nx_diag_rx.is_some() {
            return;
        }
        let so = match self.nx_estado.get(id) {
            Some(Conexion::Ok { os, .. }) if !os.is_empty() => os.clone(),
            _ => "Windows".to_string(),
        };
        self.nx_fase = Some(("Leyendo la salida", Instant::now()));
        let prompt = lucy_core::nexshell::summarize_prompt(cmd, salida, &so);
        let modelo = self.chat_model.clone();
        let privado = self.privacy;
        let (tx, rx) = std::sync::mpsc::channel();
        self.nx_diag_rx = Some((id.to_string(), rx));
        std::thread::spawn(move || {
            let r = match lucy_core::cloud::allowed(&modelo, privado) {
                Ok(()) => {
                    let mut out = String::new();
                    for ev in lucy_core::cloud::start(
                        modelo,
                        vec![lucy_core::turns::Turn::user(prompt)],
                    ) {
                        if let lucy_core::chat::ChatEvent::Token(t) = ev {
                            out.push_str(&t);
                        }
                    }
                    lucy_core::nexshell::parse_resumen(&out).map(|causa| {
                        lucy_core::nexshell::Diagnostico { causa, prueba: String::new() }
                    })
                }
                // Se traga: la salida en crudo sigue arriba y se puede leer. Que
                // además no se haya podido resumir no merece una línea roja.
                Err(_) => None,
            };
            let _ = tx.send(r);
        });
    }

    /// El último comando de un carril remoto y lo que devolvió.
    ///
    /// Se reconstruye del propio carril en vez de guardarlo aparte: es la misma
    /// fuente que el operador está viendo, así que no pueden discrepar. Las
    /// líneas van marcadas con su tipo —`c` comando, `o` salida, `e` error— y el
    /// último `c` abre el bloque que interesa.
    pub(crate) fn nx_ultimo_comando(&self, id: &str) -> (String, String) {
        let Some(lineas) = self.nx_lines.get(id) else { return (String::new(), String::new()) };
        let Some(i) = lineas.iter().rposition(|(k, _)| *k == 'c') else {
            return (String::new(), String::new());
        };
        let salida = lineas[i + 1..]
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        (lineas[i].1.clone(), salida)
    }

    /// Prueba la conexión desde el modal, sin bloquear la ventana.
    pub(crate) fn nx_probar(&mut self, h: &lucy_core::hosts::Host) {
        let (host, pw) = (h.clone(), self.nx_edit_pw.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        self.nx_testing = true;
        self.nx_test = None;
        self.nx_test_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(lucy_core::hosts::test_connection(&host, &pw));
        });
    }

    /// La lista de equipos: el local arriba, los remotos debajo.
    pub(crate) fn nx_host_rail(&mut self, ui: &mut egui::Ui) {
        row_align(ui, 24.0, egui::Align::Center, |ui| {
            ui.add(egui::Label::new(theme::instrument_label("Equipos", theme::faint())));
            ui.label(
                egui::RichText::new(format!("{}", self.remote_hosts.len() + 1))
                    .size(theme::FS_MICRO)
                    .monospace()
                    .color(theme::txt3()),
            );
            right(ui, 22.0, |ui| {
                if ui.small_button("+").on_hover_text(i18n::tr("Añadir equipo")).clicked() {
                    self.nx_edit = Some(lucy_core::hosts::Host::nuevo(
                        lucy_core::hosts::Protocol::Winrm,
                        millis_ahora(),
                    ));
                    self.nx_edit_pw.clear();
                    self.nx_edit_nuevo = true;
                    self.nx_test = None;
                }
            });
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // El local, primero y sin adornos.
            let local_sel = self.nx_host.is_none();
            if self
                .nx_host_row(
                    ui,
                    i18n::tr("Este equipo"),
                    "PowerShell · PTY",
                    theme::acc(),
                    local_sel,
                )
                .clicked()
            {
                self.nx_host = None;
            }
            let mut editar = None;
            let mut borrar = None;
            let mut elegir = None;
            let mut ver_logs = None;
            for h in &self.remote_hosts {
                let sel = self.nx_host.as_deref() == Some(h.id.as_str());
                let color = color_hex(&h.color).unwrap_or(theme::txt3());
                let sub = format!("{}:{} · {}", h.host, h.port, h.protocol.label());
                let r = self.nx_host_row(ui, &h.name, &sub, color, sel);
                // El punto de estado, a la derecha de la fila. Con ocho equipos
                // es lo que se mira antes que el nombre: cuál responde.
                if let Some(c) = self.nx_estado.get(&h.id) {
                    let rect = r.rect;
                    ui.painter().circle_filled(
                        egui::pos2(rect.right() - 12.0, rect.center().y),
                        3.5,
                        c.color(),
                    );
                }
                if r.clicked() {
                    elegir = Some(h.id.clone());
                }
                // Editar y borrar en el menú del clic derecho, no como dos
                // iconos permanentes: con ocho equipos son dieciséis botones
                // pidiendo atención para algo que se hace una vez al mes.
                r.context_menu(|ui| {
                    // VER SUS LOGS, desde donde ya se está mirando ese equipo.
                    // Sin esto había que irse al visor, cambiar a modo Archivo,
                    // abrir el desplegable y volver a encontrar la misma máquina
                    // — cuatro pasos para algo que se pide estando ya encima de
                    // la fila que la nombra.
                    if h.protocol.can_shell() && ui.button(i18n::tr("Ver sus logs")).clicked() {
                        ver_logs = Some(h.id.clone());
                        ui.close_menu();
                    }
                    if ui.button(i18n::tr("Editar")).clicked() {
                        editar = Some(h.clone());
                        ui.close_menu();
                    }
                    if ui.button(i18n::tr("Eliminar")).clicked() {
                        borrar = Some(h.id.clone());
                        ui.close_menu();
                    }
                });
            }
            if let Some(id) = elegir {
                self.nx_host = Some(id);
            }
            if let Some(id) = ver_logs {
                self.lv_ir_a_equipo(&id);
            }
            if let Some(h) = editar {
                self.nx_edit_pw = lucy_core::hosts::password(&h.id).unwrap_or_default();
                self.nx_edit = Some(h);
                self.nx_edit_nuevo = false;
                self.nx_test = None;
            }
            if let Some(id) = borrar {
                let _ = lucy_core::hosts::delete(&mut self.remote_hosts, &id);
                if self.nx_host.as_deref() == Some(id.as_str()) {
                    self.nx_host = None;
                }
            }
            if self.remote_hosts.is_empty() {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    icons::show(ui, icons::Icon::Server, 22.0, theme::faint());
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(i18n::tr("Sin equipos remotos."))
                            .size(theme::FS_CAPTION)
                            .color(theme::faint()),
                    );
                });
            }
        });
    }

    /// Una fila del carril. Devuelve su respuesta para el clic y el menú.
    pub(crate) fn nx_host_row(
        &self,
        ui: &mut egui::Ui,
        nombre: &str,
        sub: &str,
        color: egui::Color32,
        sel: bool,
    ) -> egui::Response {
        let (r, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 40.0),
            egui::Sense::click(),
        );
        if sel {
            ui.painter().rect_filled(r, egui::Rounding::same(theme::R_SM), theme::acc_bg());
        } else {
            // `bg4` Y NO `bg3`: este carril se rellena con `bg2()`, y en el tema
            // claro `bg2` y `bg3` son los dos `#FFFFFF` exacto. El hover de la
            // lista de equipos era invisible, el mismo fallo que tenía el rail
            // de módulos. Ver la nota de `theme::bg3`.
            superficie(ui, &resp, r, egui::Rounding::same(theme::R_SM), theme::bg4());
        }
        anillo(ui, &resp, r, egui::Rounding::same(theme::R_SM));
        // La pastilla de color a la izquierda: es lo que el operador asoció a
        // ese equipo al darlo de alta, y con ocho filas es lo que se busca antes
        // de leer el nombre.
        ui.painter().rect_filled(
            egui::Rect::from_min_size(r.left_top() + egui::vec2(6.0, 8.0), egui::vec2(3.0, 24.0)),
            egui::Rounding::same(2.0),
            color,
        );
        // RECORTADOS A LA FILA. El nombre lo pone el operador al dar de alta el
        // equipo y el subtítulo lleva la dirección y el puerto —
        // «192.168.1.100:5985 · Windows (WinRM)»—, que en un carril de 232 px no
        // cabe. Sin recorte se salía por la derecha del panel y se pintaba
        // encima de la terminal que tiene al lado.
        let p = ui.painter().with_clip_rect(r.shrink2(egui::vec2(6.0, 0.0)));
        p.text(
            egui::pos2(r.left() + 16.0, r.top() + 12.0),
            egui::Align2::LEFT_CENTER,
            nombre,
            egui::FontId::proportional(theme::FS_FOOTNOTE),
            if sel { theme::txt() } else { theme::txt2() },
        );
        p.text(
            egui::pos2(r.left() + 16.0, r.top() + 27.0),
            egui::Align2::LEFT_CENTER,
            sub,
            egui::FontId::proportional(theme::FS_MICRO),
            theme::faint(),
        );
        ui.add_space(2.0);
        resp
    }

    /// El alta y la edición de un equipo.
    pub(crate) fn nx_modal(&mut self, ctx: &egui::Context) {
        let Some(mut h) = self.nx_edit.clone() else { return };
        let mut cerrar = false;
        let mut guardar = false;
        let mut probar = false;
        egui::Window::new(i18n::tr(if self.nx_edit_nuevo {
            "Nuevo equipo remoto"
        } else {
            "Editar equipo"
        }))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(520.0)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(theme::bg2())
                    .stroke(egui::Stroke::new(1.0_f32, theme::acc_line())),
            )
            .show(ctx, |ui| {
                ui.set_width(500.0);
                // El subtítulo dice a qué se está dando de alta ANTES de
                // rellenar nada, y cambia con el protocolo. La V2 lo tiene y es
                // lo que evita guardar un WinRM creyendo que era un SSH.
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {}",
                        h.protocol.label(),
                        if h.host.is_empty() { i18n::tr("sin dirección aún") } else { &h.host }
                    ))
                    .size(theme::FS_CAPTION)
                    .color(theme::faint()),
                );
                ui.add_space(10.0);

                campo(ui, "Nombre", &mut h.name, "Ej. Prod-Web-01");

                // ── protocolo, agrupado ─────────────────────────────────────
                row_align(ui, 26.0, egui::Align::Center, |ui| {
                    cell(ui, 110.0, 26.0, false, etiqueta_campo("Protocolo"));
                    egui::ComboBox::from_id_salt("nx-proto")
                        .selected_text(h.protocol.label())
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            let mut grupo = "";
                            for p in lucy_core::hosts::Protocol::ALL {
                                if p.group() != grupo {
                                    grupo = p.group();
                                    ui.add_space(4.0);
                                    ui.label(theme::instrument_label(grupo, theme::faint()));
                                }
                                if ui
                                    .selectable_label(h.protocol == p, p.label())
                                    .clicked()
                                {
                                    // Arrastra puerto, sistema y categoría. Es
                                    // el núcleo quien decide, no esta vista.
                                    h.set_protocol(p);
                                }
                            }
                        });
                });
                ui.add_space(6.0);

                campo(ui, "Dirección", &mut h.host, "192.168.1.10 ó servidor.empresa.local");

                row_align(ui, 26.0, egui::Align::Center, |ui| {
                    cell(ui, 110.0, 26.0, false, etiqueta_campo("Puerto"));
                    let mut p = h.port.to_string();
                    if ui
                        .add(egui::TextEdit::singleline(&mut p).desired_width(80.0))
                        .changed()
                    {
                        // Un puerto vacío mientras se escribe no es un error: se
                        // deja en cero y `missing` no lo exige, porque el
                        // protocolo ya trae el suyo.
                        h.port = p.parse().unwrap_or(0);
                    }
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(i18n::trf(
                            "por defecto {puerto}",
                            &[("puerto", &h.protocol.default_port().to_string())],
                        ))
                            .size(theme::FS_MICRO)
                            .color(theme::faint()),
                    );
                });
                ui.add_space(6.0);

                campo(ui, "Usuario", &mut h.username, "DOMINIO/usuario");

                // LO QUE EL TRANSPORTE USA DE VERDAD, y no los dos campos
                // siempre. WinRM autentica con contraseña; SSH va por clave,
                // porque la confianza se establece antes. Enseñar una casilla de
                // contraseña a un host SSH invita a rellenarla, y ese dato no
                // se usa para nada — antes incluso impedía ejecutar.
                if h.protocol == lucy_core::hosts::Protocol::Ssh {
                    row_align(ui, 26.0, egui::Align::Center, |ui| {
                        cell(ui, 110.0, 26.0, false, etiqueta_campo("Clave privada"));
                        ui.add(
                            egui::TextEdit::singleline(&mut h.ssh_key_path)
                                .desired_width(300.0)
                                // Vacío es lo NORMAL: con `ssh-agent` o con la
                                // clave en su sitio de siempre, `ssh` la
                                // encuentra sola y no hay nada que escribir.
                                .hint_text(i18n::tr("vacío = ssh-agent o ~/.ssh/id_ed25519")),
                        );
                    });
                } else {
                    row_align(ui, 26.0, egui::Align::Center, |ui| {
                        cell(ui, 110.0, 26.0, false, etiqueta_campo("Contraseña"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.nx_edit_pw)
                                .password(true)
                                .desired_width(220.0)
                                .hint_text(i18n::tr(if self.nx_edit_nuevo { "" } else { "(sin cambios)" })),
                        );
                    });
                }
                ui.add_space(6.0);

                let mut tags = h.tags.join(", ");
                row_align(ui, 26.0, egui::Align::Center, |ui| {
                    cell(ui, 110.0, 26.0, false, etiqueta_campo("Etiquetas"));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut tags)
                                .desired_width(300.0)
                                .hint_text(i18n::tr("prod, web, db")),
                        )
                        .changed()
                    {
                        h.tags = tags
                            .split(',')
                            .map(|t| t.trim().to_string())
                            .filter(|t| !t.is_empty())
                            .collect();
                    }
                });
                ui.add_space(8.0);

                row_align(ui, 26.0, egui::Align::Center, |ui| {
                    cell(ui, 110.0, 26.0, false, etiqueta_campo("Color"));
                    for c in lucy_core::hosts::COLORS {
                        let col = color_hex(c).unwrap_or(theme::acc());
                        let (r, resp) =
                            ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                        ui.painter().circle_filled(r.center(), 10.0, col);
                        if h.color == c {
                            ui.painter().circle_stroke(
                                r.center(),
                                12.0,
                                egui::Stroke::new(2.0_f32, theme::txt()),
                            );
                        }
                        if resp.clicked() {
                            h.color = c.to_string();
                        }
                    }
                });

                // ── el requisito del protocolo ──────────────────────────────
                //
                // Se dice AQUÍ y no cuando la conexión falla. «WinRM necesita
                // Enable-PSRemoting» leído tras un error de red es un rato
                // perdido buscando en el sitio equivocado.
                let req = h.protocol.requirement();
                if !req.is_empty() {
                    ui.add_space(8.0);
                    egui::Frame::none()
                        .fill(theme::acc_bg())
                        .rounding(egui::Rounding::same(theme::R_SM))
                        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                        .show(ui, |ui| {
                            ui.label(
                                // Viene de `hosts::Protocol::requirement()`, en
                                // `lucy-core`, que no sabe de idiomas: se
                                // traduce aqui, en el punto de uso.
                                egui::RichText::new(i18n::tr(req))
                                    .size(theme::FS_CAPTION)
                                    .color(theme::txt2()),
                            );
                        });
                }
                if let Some(r) = &self.nx_test {
                    ui.add_space(6.0);
                    let (col, txt) = match r {
                        Ok(ms) => (
                            theme::acc(),
                            i18n::trf("Conectado en {ms} ms", &[("ms", &ms.to_string())]),
                        ),
                        Err(e) => (theme::red(), e.clone()),
                    };
                    ui.label(egui::RichText::new(txt).size(theme::FS_CAPTION).color(col));
                }

                ui.add_space(12.0);
                let falta = h.missing();
                row_align(ui, 28.0, egui::Align::Center, |ui| {
                    // Probar ANTES de guardar. La V2 lo añadió porque «guardé una
                    // errata y falló al primer uso» era la queja número uno.
                    if ui
                        .add_enabled(
                            falta.is_empty() && h.protocol.can_shell() && !self.nx_testing,
                            egui::Button::new(i18n::tr("Probar conexión")),
                        )
                        .clicked()
                    {
                        probar = true;
                    }
                    if ui.button(i18n::tr("Cancelar")).clicked() {
                        cerrar = true;
                    }
                    right(ui, 28.0, |ui| {
                        if ui
                            .add_enabled(falta.is_empty(), egui::Button::new(i18n::tr("Guardar")))
                            .clicked()
                        {
                            guardar = true;
                        }
                    });
                });
                // Lo que falta, dicho entero y a la vista, no de uno en uno al
                // pulsar.
                if !falta.is_empty() {
                    ui.label(
                        egui::RichText::new(i18n::trf(
                                "Falta: {campos}",
                                // CADA NOMBRE DE CAMPO POR SEPARADO. `missing()`
                                // devuelve «nombre», «dirección» y «usuario»
                                // desde `lucy-core`, y traducir la lista ya
                                // unida dejaría una frase mitad en un idioma y
                                // mitad en otro.
                                &[(
                                    "campos",
                                    &falta
                                        .iter()
                                        .map(|c| i18n::tr(c))
                                        .collect::<Vec<_>>()
                                        .join(", "),
                                )],
                            ))
                            .size(theme::FS_MICRO)
                            .color(theme::amber()),
                    );
                }
            });

        if probar {
            self.nx_probar(&h);
        }
        if guardar {
            match self.remote_hosts.iter().position(|x| x.id == h.id) {
                Some(i) => self.remote_hosts[i] = h.clone(),
                None => self.remote_hosts.push(h.clone()),
            }
            let _ = lucy_core::hosts::save(&self.remote_hosts);
            // La contraseña solo si se escribió algo: al editar, un campo en
            // blanco significa «déjala como está», no «bórrala».
            //
            // Y NUNCA para un SSH. Ese transporte va por clave, así que una
            // contraseña ahí es un secreto que se guarda, se respalda y se
            // filtra sin que nada la use jamás. La mejor forma de proteger un
            // dato es no tenerlo.
            if h.protocol != lucy_core::hosts::Protocol::Ssh
                && !self.nx_edit_pw.trim().is_empty()
            {
                let _ = lucy_core::hosts::set_password(&h.id, self.nx_edit_pw.trim());
            }
            cerrar = true;
        }
        if cerrar {
            self.nx_edit = None;
            self.nx_edit_pw.clear();
            self.nx_test = None;
        } else {
            self.nx_edit = Some(h);
        }
    }

    /// Decide qué hacer con lo escrito: ejecutarlo o mandarlo a traducir.
    pub(crate) fn nx_submit(&mut self) {
        let texto = std::mem::take(&mut self.term_input).trim().to_string();
        if texto.is_empty() || self.nx_busy {
            return;
        }
        // Al historial va lo que ESCRIBIÓ el operador, no lo que se ejecutó. Si
        // guardara el comando traducido, la flecha arriba devolvería algo que él
        // nunca tecleó y no reconocería.
        self.nx_history.push(texto.clone());
        self.nx_hist_idx = None;

        if lucy_core::nexshell::looks_like_command(&texto) {
            // Ya era un comando: lo escribio el operador tal cual, sin que el
            // modelo tocara nada.
            self.nx_maybe_run(texto, "manual");
            return;
        }
        self.nx_translate(None, texto);
    }

    /// Manda una frase a traducir. `destino` = el equipo remoto, o `None` para
    /// el local.
    ///
    /// UNA SOLA FUNCIÓN PARA LOS DOS. Estaba escrita dentro de la ruta local, y
    /// por eso el remoto no traducía: la frase se intentaba ejecutar tal cual.
    /// Sacarla aquí es lo que hace que el módulo se comporte igual en los dos
    /// sitios, que es lo único que se le pide a un módulo.
    pub(crate) fn nx_translate(&mut self, destino: Option<lucy_core::hosts::Host>, texto: String) {
        // EL SISTEMA DEL EQUIPO AL QUE VA, no el de esta máquina. Pedir un
        // comando «para Windows 11» cuando al otro lado hay una Debian devuelve
        // algo que allí no existe.
        let (shell, so) = match &destino {
            Some(h) => (
                if h.protocol.os() == "windows" { "PowerShell" } else { "bash" },
                self.nx_estado
                    .get(&h.id)
                    .and_then(|c| match c {
                        Conexion::Ok { os, .. } if !os.is_empty() => Some(os.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| {
                    // Sin haberlo medido aún, el tipo declarado del equipo es
                    // peor que la medición y mucho mejor que el de aquí.
                    if h.protocol.os() == "windows" { "Windows" } else { "Linux" }.to_string()
                }),
            ),
            None => ("PowerShell", self.sys.snapshot().os),
        };
        // LO QUE PIDIÓ EL OPERADOR, EN LA TRANSCRIPCIÓN. No se escribía en
        // ninguna parte: se pedía una frase, la caja se vaciaba, y en la pantalla
        // no quedaba rastro ni de la pregunta ni de que se estuviera haciendo
        // algo con ella. Al volver el comando traducido aparecía un `❯ Get-…`
        // salido de la nada, sin la frase que lo produjo — que es justo lo que
        // hace falta para saber si Lucy entendió lo que se le pidió.
        //
        // Con marca propia (`p`) y no con la del comando: son dos cosas
        // distintas y confundirlas es no poder distinguir lo que uno escribió de
        // lo que se ejecutó.
        if let Some(h) = &destino {
            let id = h.id.clone();
            self.nx_lines_mut(&id).push(('p', texto.clone()));
        } else {
            self.nx_say(&format!("❯ {texto}"));
        }
        self.nx_destino = destino;
        let prompt = lucy_core::nexshell::translate_prompt(&texto, shell, &so);
        let modelo = self.chat_model.clone();
        let privado = self.privacy;
        let (tx, rx) = std::sync::mpsc::channel();
        self.nx_busy = true;
        self.nx_rx = Some(rx);
        // EL RELOJ, QUE NO SE PONÍA. La cabecera enseña `nx_started.elapsed()`, y
        // sin esto se quedaba clavado en «ejecutando… 0s» — que es exactamente el
        // texto fijo que el comentario de la cabecera dice que se quiso evitar,
        // porque no distingue «avanza» de «se colgó».
        self.nx_started = Some(Instant::now());
        self.nx_fase = Some(("Traduciendo a un comando", Instant::now()));
        // Y UNA BANDERA NUEVA PARA ESTA TRADUCCIÓN. `nx_stop` es el mismo campo
        // que usa la ejecución remota; reutilizar el de la vez anterior —que
        // puede estar ya en alto— haría que la traducción se cortara sola nada
        // más empezar.
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.nx_stop = stop.clone();
        std::thread::spawn(move || {
            let r = match lucy_core::cloud::allowed(&modelo, privado) {
                Ok(()) => {
                    // Se acumula el flujo entero: aquí no hay nada que enseñar
                    // token a token, es una línea de comando.
                    let mut out = String::new();
                    let mut err = None;
                    for ev in lucy_core::cloud::start(
                        modelo,
                        vec![lucy_core::turns::Turn::user(prompt)],
                    ) {
                        // SE MIRA ENTRE TOKEN Y TOKEN. No corta una petición ya
                        // lanzada —para eso haría falta que el cliente HTTP lo
                        // soportara— pero deja de consumirla y de pagarla en
                        // cuanto el operador dice basta. La pantalla se libera
                        // igual desde el otro lado: ver el botón de detener.
                        if stop.load(std::sync::atomic::Ordering::Relaxed) {
                            return;
                        }
                        match ev {
                            lucy_core::chat::ChatEvent::Token(t) => out.push_str(&t),
                            lucy_core::chat::ChatEvent::Error(e) => err = Some(e),
                            _ => {}
                        }
                    }
                    match err {
                        Some(e) => Err(e),
                        None => Ok(lucy_core::nexshell::clean_command(&out)),
                    }
                }
                Err(e) => Err(e),
            };
            let _ = tx.send(r);
        });
    }

    /// Corre el comando, o pide confirmación si no se deshace.
    ///
    /// LAS DOS COMPROBACIONES, COMO EL BUCLE AUTOMÁTICO. Y hasta ahora era solo
    /// una: este comentario decía «la comprobación es la MISMA que usa el bucle
    /// automático» y el cuerpo llamaba únicamente a `destructive::is_destructive`.
    /// El bucle hace DOS cosas y en este orden — primero `guard::scan`, que
    /// decide si el texto tiene forma de ataque, y solo después `is_destructive`,
    /// que decide si hace falta que alguien lo lea.
    ///
    /// Un comentario que afirma una equivalencia que no existe es peor que no
    /// tenerlo: es lo que hace que nadie vuelva a mirar. Lo comprobé contando las
    /// llamadas al guardrail en este fichero — cuatro, y las cuatro en el bucle
    /// del agente.
    ///
    /// Y EL CAMINO NO ES EL DEL OPERADOR ESCRIBIENDO. Aquí llega lo que devolvió
    /// el MODELO al traducir una frase (`nx_translate` → `pump_nx`), así que el
    /// rol es `Assistant`, el mismo con el que se escanea un `<EXECUTE>`. Que el
    /// operador escribiera la frase no hace suyo el comando: lo redactó el
    /// modelo, con lo que hubiera en su contexto.
    ///
    /// Bloqueado NO ofrece botón, igual que en el bucle: ofrecer uno para
    /// ejecutar una firma de ataque convierte el guardrail en un trámite.
    pub(crate) fn nx_maybe_run(&mut self, cmd: String, origen: &'static str) {
        let g = lucy_core::guard::scan(&cmd, lucy_core::guard::Role::Assistant);
        if g.decision == lucy_core::guard::Decision::Block {
            self.nx_aviso(None, &i18n::trf("Bloqueado por el guardrail: {motivo}",
                &[("motivo", &g.reason)]));
            return;
        }
        if lucy_core::destructive::is_destructive(&cmd) {
            self.nx_confirm = Some(Pendiente { host: None, cmd, origen });
        } else {
            // SE REGISTRA QUE SE MANDÓ, SIN CÓMO ACABÓ. La terminal local es un
            // PTY de verdad: no hay un evento de «este comando terminó y devolvió
            // esto», solo una pantalla que va cambiando. Deducir los límites de
            // cada comando del emulador VT sería adivinar, y una fila de
            // auditoría adivinada es peor que ninguna.
            //
            // `exit_code: None` dice exactamente eso — «no se sabe» — que es para
            // lo que existe. Lo que sí es cierto y merece constancia es que Lucy
            // convirtió una frase en un comando y lo mandó a la máquina.
            self.auditar_enviado(&cmd, "", origen);
            self.nx_run(&cmd);
        }
    }

    pub(crate) fn nx_run(&mut self, cmd: &str) {
        if let Some(pty) = &mut self.pty {
            pty.send(&format!("{cmd}\r"));
        }
    }

    /// Escribe una línea de Lucy en la pantalla del terminal.
    ///
    /// Por el PTY con `Write-Host` y no pintándola aparte: así queda EN la
    /// sesión, en su sitio y en su orden, y el «copiar salida» se la lleva
    /// también. Una línea flotante fuera del emulador se saldría del orden en
    /// cuanto el comando anterior siguiera escribiendo.
    pub(crate) fn nx_say(&mut self, texto: &str) {
        // Las comillas simples del propio texto se doblan: es la única forma de
        // escaparlas dentro de un literal de PowerShell, y sin ello un mensaje
        // con un apóstrofo rompe la línea.
        let seguro = texto.replace('\'', "''");
        self.nx_run(&format!("Write-Host '{seguro}' -ForegroundColor DarkYellow"));
    }

    /// Recorre el historial. `-1` hacia atrás, `1` hacia delante.
    pub(crate) fn nx_recall(&mut self, dir: i32) {
        if self.nx_history.is_empty() {
            return;
        }
        let n = self.nx_history.len();
        self.nx_hist_idx = match (self.nx_hist_idx, dir) {
            // Desde una línea nueva, arriba lleva a lo último escrito.
            (None, -1) => Some(n - 1),
            (None, _) => None,
            (Some(0), -1) => Some(0),
            (Some(i), -1) => Some(i - 1),
            // Y bajar desde lo último devuelve la línea EN BLANCO, no se queda
            // clavado en el último comando: es lo que hace cualquier shell y lo
            // que uno espera para escribir algo nuevo.
            (Some(i), _) if i + 1 >= n => None,
            (Some(i), _) => Some(i + 1),
        };
        self.term_input = match self.nx_hist_idx {
            Some(i) => self.nx_history[i].clone(),
            None => String::new(),
        };
    }
}
