//! Paleta de ordenes: filtro, teclado y ejecucion de barras.
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
    /// La paleta de comandos: aparece al escribir `/` y filtra según se teclea.
    ///
    /// Va ANCLADA sobre el compositor y no en un desplegable del sistema porque
    /// tiene que moverse con él: el compositor está pegado abajo, y una lista
    /// que apareciera en otro sitio obligaría a mirar a dos lados a la vez.
    pub(crate) fn slash_palette(&mut self, ui: &mut egui::Ui) {
        let draft = self.tabs[self.tab].input.clone();
        if !draft.starts_with('/') {
            // Con la paleta cerrada el resaltado vuelve arriba. Sin esto, abrirla
            // otra vez la dejaría señalando la fila donde se quedó la vez
            // anterior, sobre una lista que ya no es la misma.
            self.slash_sel = 0;
            return;
        }
        let hits = slash_hits(&draft);
        if hits.is_empty() {
            return;
        }

        let composer = ui.min_rect();
        let w = composer.width().min(620.0);
        let row_h = 26.0;
        let shown = hits.len().min(9);
        let h = shown as f32 * row_h + 16.0;

        // ── Teclado, ANTES de dibujar ────────────────────────────────────────
        //
        // Las flechas mueven, Enter elige y Tab completa. Antes solo había Tab y
        // siempre sobre el primero: con nueve resultados en pantalla eso
        // significa que ocho no se podían elegir sin ratón.
        //
        // Enter se atrapa aquí porque con la paleta abierta es lo que espera
        // cualquiera: elegir de la lista, no mandar `/kg` como si fuera una
        // pregunta. El compositor lo mira DESPUÉS y ya no lo encuentra.
        let sel = &mut self.slash_sel;
        ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                *sel = (*sel + 1) % hits.len();
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                *sel = (*sel + hits.len() - 1) % hits.len();
            }
        });
        // La lista cambia mientras se escribe, así que el índice de hace dos
        // letras puede señalar fuera. Se recorta en vez de entrar en pánico.
        let sel = recorta_sel(*sel, hits.len());
        self.slash_sel = sel;
        let mut elegido: Option<&str> = None;
        if ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
        }) {
            elegido = Some(hits[sel].0);
        }

        // ── La lista ─────────────────────────────────────────────────────────
        //
        // En un `Area` y no con un painter suelto sobre una capa de primer
        // plano, que es lo que había y por lo que no se podía pulsar nada. El
        // dibujo salía bien —está por encima de todo— pero el clic se probaba
        // con `ui.rect_contains_pointer`, que intersecta contra el `clip_rect`
        // del `ui` que la llama: la paleta se pinta ENCIMA del compositor, o
        // sea fuera de él, así que esa intersección era vacía y la fila nunca
        // se daba por señalada. Un `Area` es la capa de verdad de egui, con su
        // orden y su reparto de entrada, y las filas vuelven a ser widgets con
        // su `Response`.
        let resp = egui::Area::new(egui::Id::new("slash"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(composer.left(), composer.top() - h - 8.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::none()
                    .fill(theme::bg3())
                    .stroke(egui::Stroke::new(1.0_f32, theme::bdr2()))
                    .rounding(egui::Rounding::same(theme::R_LG))
                    .inner_margin(egui::Margin::symmetric(6.0, 8.0))
                    .show(ui, |ui| {
                        ui.set_width(w - 12.0);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let mut pulsado: Option<&str> = None;
                        for (i, (cmd, desc, ready)) in hits.iter().take(shown).enumerate() {
                            let (r, resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), row_h),
                                egui::Sense::click(),
                            );
                            // El resaltado sale del teclado O del ratón: son la
                            // misma cosa vista de dos maneras, y tener dos
                            // marcas a la vez confunde sobre cuál se elegiría.
                            if i == sel || resp.hovered() {
                                ui.painter().rect_filled(
                                    r,
                                    egui::Rounding::same(theme::R_SM),
                                    theme::bg4(),
                                );
                            }
                            if resp.clicked() {
                                pulsado = Some(cmd);
                            }
                            // RECORTADAS A LA FILA. La orden cabe siempre —son
                            // cortas y suyo es el hueco de 120 px—, pero la
                            // descripción es una frase y se traduce: en alemán
                            // pasa de largo, y con la ventana estrecha se salía
                            // por el borde del popup pintando sobre lo que
                            // hubiera detrás.
                            let p = ui.painter().with_clip_rect(r);
                            p.text(
                                egui::pos2(r.left() + 10.0, r.center().y),
                                egui::Align2::LEFT_CENTER,
                                cmd,
                                egui::FontId::monospace(theme::FS_FOOTNOTE),
                                theme::acc(),
                            );
                            p.text(
                                egui::pos2(r.left() + 130.0, r.center().y),
                                egui::Align2::LEFT_CENTER,
                                i18n::tr(desc),
                                egui::FontId::proportional(theme::FS_CAPTION),
                                if *ready { theme::txt2() } else { theme::faint() },
                            );
                            // Los que todavía no hacen nada se marcan aquí, no
                            // al pulsarlos: enterarse después de elegir es
                            // perder el movimiento.
                            if !*ready {
                                p.text(
                                    egui::pos2(r.right() - 10.0, r.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    i18n::tr("sin migrar"),
                                    egui::FontId::proportional(theme::FS_MICRO),
                                    theme::faint(),
                                );
                            }
                        }
                        if hits.len() > shown {
                            ui.painter().text(
                                egui::pos2(ui.min_rect().center().x, ui.min_rect().bottom() + 6.0),
                                egui::Align2::CENTER_CENTER,
                                i18n::trf(
                                    "+{n} más — sigue escribiendo para acotar",
                                    &[("n", &(hits.len() - shown).to_string())],
                                ),
                                egui::FontId::proportional(theme::FS_MICRO),
                                theme::faint(),
                            );
                        }
                        pulsado
                    })
                    .inner
            });
        if let Some(c) = resp.inner {
            elegido = Some(c);
        }
        if let Some(c) = elegido {
            self.tabs[self.tab].input.clear();
            // `/model` se queda aquí porque necesita el `ui` para abrir el
            // desplegable donde ya está, en vez de duplicar el selector.
            if c == "/model" {
                let id = ui.make_persistent_id("model-menu");
                ui.memory_mut(|m| m.open_popup(id));
            } else {
                self.slash_exec(c, "");
            }
        }
    }

    /// Cumple un comando de barra. Sin `ui`: lo llaman la paleta y el envío.
    ///
    /// LOS DOS CAMINOS PASAN POR AQUÍ. Elegir `/clear` de la lista y escribir
    /// `/clear` y pulsar Enter tienen que hacer lo mismo, y con la ejecución
    /// metida dentro de la paleta no lo hacían: lo segundo mandaba «/clear» al
    /// modelo como si fuera una pregunta.
    pub(crate) fn slash_exec(&mut self, cmd: &str, args: &str) {
        match cmd {
            "/clear" => {
                let uid = self.tabs[self.tab].uid;
                let t = &mut self.tabs[self.tab];
                t.log.clear();
                t.ws.reset();
                t.drain.flush();
                // El destino apuntaba a un mensaje que ya no existe.
                t.drain_dest = None;
                // Los sub-agentes de la conversación que se acaba de borrar no
                // tienen dónde volver, así que se paran de verdad: el
                // interruptor corta sus peticiones, y soltar los canales quita la
                // espera —sin eso la pestaña se quedaría ocupada para siempre
                // esperando un lote cuyo turno ya no existe.
                t.fork_stop.store(true, std::sync::atomic::Ordering::Relaxed);
                t.fork_rx.clear();
                t.espera = None;
                // CONVERSACIÓN NUEVA, SESIÓN NUEVA. El cristal es de la sesión y
                // no hay más de uno: conservando el identificador, la segunda
                // conversación de la pestaña —y todas las siguientes— quedaba
                // vetada de destilarse para siempre, sin que nada lo dijera.
                t.sesion = format!("egui-{}-{uid}", ahora_epoch());
                t.turno_ms = 0;
            }
            "/memory" => self.view = View::Memoria,
            // Rota entre los tres en vez de abrir un menú: un comando de barra
            // se escribe para no levantar las manos del teclado, y desembocar
            // en un desplegable que hay que apuntar deshace justo eso.
            // `/model <id>` ESCRITO. Elegido de la paleta no llega aquí: se queda
            // arriba, en el sitio que tiene `ui`, y abre el desplegable.
            //
            // NO TENÍA BRAZO, y estaba marcado como listo. Escribir
            // `/model claude-opus-5` y pulsar Enter pasaba el filtro de `listo`,
            // vaciaba la caja y caía en el `otro =>` de abajo: el argumento se
            // tiraba, el modelo no cambiaba, y la caja volvía a decir `/model `.
            // Sin error. Desde la paleta funcionaba, y por eso nadie lo había
            // visto — pero era el único comando anunciado como listo que no
            // hacía nada con lo que se le escribía.
            "/model" => {
                let id = args.trim();
                if id.is_empty() {
                    // Sin `ui` no se puede abrir el desplegable desde aquí: su id
                    // de memoria sale de la ruta del `ui` que lo dibuja. Se dice
                    // cuál hay y cómo se cambia.
                    let m = i18n::trf(
                        "El modelo activo es **{modelo}**. Para cambiarlo, escribe \
                         `/model` seguido del id, o elígelo en el selector de modelos.",
                        &[("modelo", lucy_core::models::describe(&self.chat_model))],
                    );
                    self.di(&m);
                } else if lucy_core::models::find(id).is_none()
                    && !self.models.iter().any(|m| m == id)
                {
                    // UNA ERRATA NO SE CONVIERTE EN EL MODELO ACTIVO. Aceptarla
                    // la guardaría como id, y el error saldría en el siguiente
                    // envío con un mensaje del proveedor que no dice que la causa
                    // fue esto. Los ids de escribir a mano —`nvidia-custom`,
                    // `local-custom`— siguen yendo por el selector.
                    let m = i18n::trf(
                        "No conozco el modelo `{id}`. El selector de modelos enseña los \
                         que hay, incluidos los de Ollama instalados en este equipo.",
                        &[("id", id)],
                    );
                    self.di(&m);
                } else if let Err(e) = lucy_core::cloud::allowed(id, self.privacy) {
                    // El mismo filtro que aplica `/privacy` al modelo que ya hay.
                    // Sin esto, escribir un modelo de nube con el modo privacidad
                    // puesto lo activaba, y el siguiente mensaje salía del equipo.
                    let m = i18n::trf("No lo cambio: {e}", &[("e", &e)]);
                    self.di(&m);
                } else {
                    self.chat_model = id.to_string();
                    let m = i18n::trf(
                        "Modelo: **{modelo}**.",
                        &[("modelo", lucy_core::models::describe(id))],
                    );
                    self.di(&m);
                }
            }
            "/theme" => {
                let siguiente = match theme::mode() {
                    theme::Mode::Dark => theme::Mode::Light,
                    theme::Mode::Light => theme::Mode::Auto,
                    theme::Mode::Auto => theme::Mode::Dark,
                };
                self.tema_pendiente = Some(siguiente);
                self.di(&format!("Tema: **{}**.", siguiente.label()));
            }
            "/privacy" => {
                self.privacy = !self.privacy;
                let m = if self.privacy {
                    match lucy_core::cloud::allowed(&self.chat_model, true) {
                        Ok(()) => i18n::trf(
                            "Modo privacidad **activado**. Nada sale de este equipo. El \
                             modelo actual (`{modelo}`) es local, así que puedes seguir.",
                            &[("modelo", &self.chat_model)],
                        ),
                        Err(e) => i18n::trf(
                            "Modo privacidad **activado**. Nada sale de este equipo.\n\n⚠ {e}",
                            &[("e", &e)],
                        ),
                    }
                } else {
                    i18n::tr(
                        "Modo privacidad **apagado**. Vuelven a estar disponibles los modelos \
                         de nube.",
                    )
                    .to_string()
                };
                self.di(&m);
            }
            "/pantalla" => match lucy_core::screen::capture_image(lucy_core::screen::MAX_WIDTH) {
                Ok(img) => {
                    let t = &mut self.tabs[self.tab];
                    let mut a = Attachment::pending("pantalla.png", AttachKind::Image);
                    a.pending = false;
                    a.image = Some(img);
                    t.attachments.push(a);
                    t.input = if args.is_empty() {
                        i18n::tr("¿Qué ves en mi pantalla? ").into()
                    } else {
                        format!("{args} ")
                    };
                }
                Err(e) => self.di(&i18n::trf("No pude capturar tu pantalla: {e}", &[("e", &e)])),
            },
            "/recall" => self.slash_recall(args),
            "/principio" => self.slash_principio(args),
            "/consolidate" => self.slash_consolidate(),
            "/snapshot" => self.slash_snapshot(),
            "/capabilities" => self.slash_capabilities(),
            "/skills" | "/skills-manager" => self.slash_skills(args),
            "/preset" => self.slash_preset(args),
            "/help" => {
                let mut s = format!("{}\n\n", i18n::tr("Comandos disponibles:"));
                for (c, desc, listo) in SLASH {
                    s.push_str(&format!(
                        "- `{c}` — {}{}\n",
                        i18n::tr(desc),
                        if listo { "" } else { i18n::tr("  _(sin migrar)_") }
                    ));
                }
                self.di(&s);
            }
            // Lo que todavía no existe rellena el campo, que es lo único
            // honesto que se puede hacer con un comando que no está.
            otro => self.tabs[self.tab].input = format!("{otro} "),
        }
    }

    /// `/preset <nombre>` fija un procedimiento; `/preset clear` lo quita.
    ///
    /// UN PRESET Y UN SKILL SON EL MISMO FICHERO, y lo que cambia es cuándo
    /// aplica: uno se pide para una tarea y se acaba con ella, el otro se fija y
    /// enmarca todo hasta que alguien lo quita. Tener dos sistemas para eso
    /// obligaría a escribir cada procedimiento dos veces, y la copia menos usada
    /// sería la que se quedara vieja.
    pub(crate) fn slash_preset(&mut self, args: &str) {
        let a = args.trim();
        if a.eq_ignore_ascii_case("clear") || a == "-" {
            match self.preset.take() {
                Some(p) => self.di(&i18n::trf(
                    "Modo **{p}** quitado. Vuelvo a contestar libremente.",
                    &[("p", &p)],
                )),
                None => self.di("No había ningún modo puesto."),
            }
            return;
        }
        if a.is_empty() {
            let m = match &self.preset {
                Some(p) => i18n::trf(
                    "Modo activo: **{p}**.\n\nQuítalo con `/preset clear`.",
                    &[("p", p)],
                ),
                None => {
                    if self.skills.is_empty() {
                        i18n::tr("No hay ningún modo puesto, y tampoco hay skills instalados.")
                            .to_string()
                    } else {
                        i18n::trf(
                            "No hay ningún modo puesto.\n\nFija uno con `/preset <nombre>`: {hay}",
                            &[(
                                "hay",
                                &self
                                    .skills
                                    .iter()
                                    .map(|k| k.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            )],
                        )
                    }
                }
            };
            self.di(&m);
            return;
        }
        match lucy_core::skills::find(&self.skills, a) {
            Some(k) => {
                let (n, d) = (k.name.clone(), k.description.clone());
                self.preset = Some(n.clone());
                self.di(&i18n::trf(
                    "Modo **{n}** puesto — {d}\n\nA partir de ahora enmarco todo en él. \
                     Se quita con `/preset clear`.",
                    &[("n", &n), ("d", &d)],
                ));
            }
            // Los que SÍ hay. «No existe» a secas deja probando nombres.
            None => {
                let hay = self
                    .skills
                    .iter()
                    .map(|k| k.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.di(&i18n::trf(
                    "No hay ningún skill llamado «{a}». Los que hay: {hay}.",
                    &[("a", a), ("hay", &hay)],
                ));
            }
        }
    }

    /// `/skills` — qué procedimientos hay instalados y de dónde salen.
    ///
    /// RELEE EL DISCO al invocarlo. El catálogo se carga al arrancar, así que
    /// sin releer aquí, añadir un skill obligaría a reiniciar Lucy para verlo —
    /// y el sentido de que sean ficheros es justamente que no haga falta.
    pub(crate) fn slash_skills(&mut self, args: &str) {
        // `install <ruta>` desde el chat: quien está escribiendo no tiene por qué
        // irse a Configuración a hacer una cosa que ya sabe nombrar.
        if let Some(ruta) = args.trim().strip_prefix("install") {
            let ruta = ruta.trim();
            if ruta.is_empty() {
                self.di(i18n::tr(
                    r"Dime de dónde: `/skills install C:\ruta\al\skill`. Vale la carpeta de un skill, o una que contenga varios — un repositorio descargado sirve tal cual.",
                ));
                return;
            }
            let m = match lucy_core::skills::user_dir() {
                Some(d) => match lucy_core::skills::install(std::path::Path::new(ruta), &d) {
                    Ok(v) => {
                        self.skills = cargar_skills();
                        i18n::trf("Instalados: {lista}.", &[("lista", &v.join(", "))])
                    }
                    Err(e) => e,
                },
                None => i18n::tr("No se pudo resolver tu perfil de usuario.").into(),
            };
            self.di(&m);
            return;
        }
        self.skills = cargar_skills();
        if self.skills.is_empty() {
            self.di(
                "No hay skills instalados.

Un skill es una carpeta con un `SKILL.md` \n                 dentro. Se buscan junto al ejecutable, en tu perfil y en el directorio \n                 desde el que lanzas Lucy.",
            );
            return;
        }
        let mut m = format!(
            "{}\n\n",
            i18n::trf(
                "**{n} skills instalados**",
                &[("n", &self.skills.len().to_string())],
            )
        );
        for k in &self.skills {
            m.push_str(&format!("- `{}` — {}
", k.name, k.description));
        }
        m.push_str(&format!(
            "\n{}",
            i18n::tr("Lucy los pide sola cuando encajan. Para forzar uno, díselo por su nombre."),
        ));
        self.di(&m);
    }

    /// `/recall <consulta>` — qué recordaría Lucy si le preguntaras eso.
    ///
    /// ENSEÑA LO QUE EL PROMPT INYECTA. La recuperación semántica corre en cada
    /// turno y es invisible: cuando Lucy contesta algo raro, saber si fue por
    /// una memoria mal recordada es imposible sin ver lo que se le metió. Esto
    /// es esa ventana, y usa la MISMA función que el prompt — no una parecida,
    /// que enseñaría un resultado que no es el que se está usando.
    pub(crate) fn slash_recall(&mut self, consulta: &str) {
        if consulta.trim().is_empty() {
            self.di("Escribe qué buscar: `/recall disco lleno`.");
            return;
        }
        // EN UN HILO, aunque sea «solo una búsqueda». Recordar embebe la
        // consulta, y eso es una petición HTTP con treinta segundos de plazo:
        // con Ollama cargando un modelo en frío, hacerla aquí congelaba la
        // ventana entera — el fallo por el que existe esta migración, metido de
        // contrabando en un comando de barra.
        let uid = self.tabs[self.tab].uid;
        let q = consulta.to_string();
        let debil = lucy_core::prompt::model_is_weak(&self.chat_model);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send((q.clone(), prompt::recall(&q, debil)));
        });
        self.recall_rx.push((uid, rx));
        self.di("Buscando en la memoria…");
    }

    /// `/principio` — dicta una regla que Lucy aplicará siempre.
    ///
    /// UN COMANDO Y NO UNA ETIQUETA que el modelo escriba. Un principio manda
    /// sobre el comportamiento por defecto en todos los turnos siguientes, así
    /// que quien lo dicta tiene que ser el operador a propósito — no algo que
    /// Lucy decida guardarse porque le pareció importante. Es la única pieza de
    /// la memoria que a propósito NO es automática.
    pub(crate) fn slash_principio(&mut self, regla: &str) {
        if regla.trim().is_empty() {
            let lista = match lucy_core::principles::list() {
                Ok(v) if v.is_empty() => {
                    i18n::tr("Todavía no hay ninguno.").to_string()
                }
                Ok(v) => v
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        format!(
                            "[P{}] {}{}",
                            i + 1,
                            p.regla,
                            if p.activo { "" } else { i18n::tr("  (desactivado)") }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                Err(e) => i18n::trf("No se pudieron leer: {e}", &[("e", &e)]),
            };
            self.di(&i18n::trf(
                "Reglas que aplico siempre:\n\n{lista}\n\nPara añadir una: \
                 `/principio en producción avisa antes de reiniciar un servicio`.",
                &[("lista", &lista)],
            ));
            return;
        }
        match lucy_core::principles::add("", regla.trim(), None) {
            Ok(_) => self.di(&format!(
                "Anotado. A partir de ahora lo aplico en todos los turnos, sin repetirlo:\n\n\
                 «{}»",
                regla.trim()
            )),
            Err(e) => self.di(&e),
        }
    }

    /// `/consolidate` — qué memorias se fundirían, sin fundirlas.
    ///
    /// EN SECO, como el botón de la vista de Memoria. Un comando de barra que
    /// modificara la base de datos al escribirlo sería la peor forma de ofrecer
    /// una función destructiva: sin ver antes qué toca.
    pub(crate) fn slash_consolidate(&mut self) {
        // En seco y EN UN HILO. La pasada es puro CPU y disco —no llama a
        // ningún modelo— pero son medio millón de comparaciones sobre una
        // conexión del pool, y el pool lo comparten los hilos de fondo: si el
        // mantenimiento está consolidando en ese momento, esperar la conexión
        // aquí congela la ventana el rato que él tarde.
        self.lanza_dedup(true);
        self.di("Revisando duplicados…");
    }

    /// `/snapshot` — el estado del equipo, ahora, en el hilo.
    ///
    /// EN LA CONVERSACIÓN y no en el Dashboard, y esa es la diferencia: queda
    /// FECHADO dentro del hilo. «Mira, a las once y cuarto la RAM estaba al 40 %»
    /// es una frase que se puede escribir porque el número quedó escrito ahí, y
    /// un panel que siempre enseña el valor de ahora no la permite.
    pub(crate) fn slash_snapshot(&mut self) {
        let s = self.sys.snapshot();
        let mut m = i18n::trf(
            "**{host}** · {os}\n\nCPU {cpu} % · RAM {usada} de {total} GB\n",
            &[
                ("host", &s.host),
                ("os", &s.os),
                ("cpu", &format!("{:.0}", s.cpu_pct)),
                ("usada", &format!("{:.1}", s.mem_used as f64 / 1e9)),
                ("total", &format!("{:.1}", s.mem_total as f64 / 1e9)),
            ],
        );
        for d in &s.disks {
            let usado = d.total.saturating_sub(d.avail);
            let pct = if d.total > 0 { usado as f64 / d.total as f64 * 100.0 } else { 0.0 };
            m.push_str(&format!(
                "{} {:.0} % usado · {:.1} GB libres de {:.1}\n",
                d.mount,
                pct,
                d.avail as f64 / 1e9,
                d.total as f64 / 1e9
            ));
        }
        if !self.services.is_empty() {
            m.push_str(&format!("\n{} servicios automáticos detenidos:\n", self.services.len()));
            for sv in self.services.iter().take(10) {
                m.push_str(&format!("- {}\n", sv.name));
            }
        }
        self.di(&m);
    }

    /// `/capabilities` — qué puede hacer ESTE shell, medido y no declarado.
    ///
    /// TODO SALE DE PREGUNTARLE AL ESTADO, no de una lista escrita a mano. Una
    /// lista escrita miente en cuanto algo cambia, y es justo lo que ha pasado
    /// cuatro veces hoy con comentarios que afirmaban carencias ya resueltas.
    /// Aquí, si mañana se migra un comando, esta respuesta lo dice sola.
    pub(crate) fn slash_capabilities(&mut self) {
        let listos: Vec<&str> =
            SLASH.iter().filter(|(_, _, l)| *l).map(|(c, _, _)| *c).collect();
        let con_clave: Vec<&str> = lucy_core::keys::PROVIDERS
            .iter()
            .filter(|(k, _, _)| lucy_core::keys::has(k))
            .map(|(_, etiqueta, _)| *etiqueta)
            .collect();
        let herramientas: Vec<&str> =
            lucy_core::tools::AVAILABLE.iter().map(|(n, _)| *n).collect();
        let remotos = self.remote_hosts.len();

        let mut m = format!("{}\n\n", i18n::tr("**Lo que puedo hacer en este equipo**"));
        m.push_str(&format!("- Herramientas: {}\n", herramientas.join(", ")));
        m.push_str(&format!(
            "{}\n",
            i18n::tr("- Ejecutar PowerShell, cmd, wmic, netsh, reg y cscript, con tu aprobación"),
        ));
        m.push_str(&format!(
            "- Equipos remotos dados de alta: {}{}\n",
            remotos,
            if remotos > 0 { " (puedo ejecutar en ellos)" } else { "" }
        ));
        m.push_str(&format!(
            "- Proveedores con clave: {}\n",
            if con_clave.is_empty() {
                "ninguno — configúralas en Configuración".to_string()
            } else {
                con_clave.join(", ")
            }
        ));
        m.push_str(&format!("- Comandos de barra activos: {}\n", listos.join(" ")));
        m.push_str(&format!(
            "- Modo automático: {} · Privacidad: {}\n",
            if self.tabs[self.tab].auto { "encendido" } else { "apagado" },
            if self.privacy { "encendida" } else { "apagada" }
        ));
        // Y lo que NO puedo, que es la mitad que suele faltar en una
        // introspección: sin ella, lo que no aparece se lee como un olvido.
        let pendientes = SLASH.len() - listos.len();
        m.push_str(&format!(
            "\n**Lo que todavía no**: {pendientes} comandos de barra sin migrar, \
             sub-agentes, y escribir ficheros sin que apruebes el diff."
        ));
        self.di(&m);
    }
}
