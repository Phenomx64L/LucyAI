//! Memoria del agente: notas, principios, consolidacion y sugerencias.
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
    /// Carga la lista de la pestaña si aún no se ha mirado. Al entrar y al
    /// recargar — nunca por frame.
    pub(crate) fn mem_carga(&mut self, t: MemTab) {
        match t {
            MemTab::Memorias => {}
            MemTab::Cristales => {
                if self.cristales.is_none() {
                    self.cristales = Some(lucy_core::crystals::list(100));
                }
            }
            MemTab::Insights => {
                if self.insights_l.is_none() {
                    self.recarga_insights();
                }
            }
            MemTab::Documentos => {
                if self.docs_l.is_none() {
                    self.docs_l = Some(lucy_core::docs::list());
                }
            }
            MemTab::Principios => {
                if self.principios_l.is_none() {
                    self.principios_l = Some(lucy_core::principles::list());
                }
            }
            MemTab::Mantenimiento => {
                if self.mant_info.is_none() {
                    self.mant_info = Some(
                        [
                            lucy_core::maintenance::CONSOLIDAR,
                            lucy_core::maintenance::INSIGHTS,
                            lucy_core::maintenance::PODA,
                        ]
                        .into_iter()
                        .map(|j| {
                            (
                                j,
                                lucy_core::maintenance::ultima(j),
                                lucy_core::maintenance::racha_en_blanco(j),
                            )
                        })
                        .collect(),
                    );
                }
            }
        }
    }

    pub(crate) fn mem_tab_memorias(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("↻ Recargar")).clicked() {
                self.mems = load_memories();
            }
            // ── Duplicados ───────────────────────────────────────────────────
            //
            // EN SECO PRIMERO, SIEMPRE. La pasada existía desde hace tiempo en la
            // app y nunca la llamaba nadie, así que sobre esta base de datos no
            // ha corrido jamás: lo primero que haga tiene que ser enseñar qué
            // fundiría, no fundirlo. El botón de aplicar solo aparece después,
            // y solo si encontró algo.
            // En un hilo, por el pool: si el mantenimiento está consolidando en
            // ese momento, esperar aquí su conexión congela la ventana.
            let girando = self.dedup_rx.is_some();
            if ui.add_enabled(!girando, egui::Button::new(i18n::tr(if girando { "Buscando…" } else { "Buscar duplicados" }))).clicked() {
                self.lanza_dedup(true);
            }
            if let Some(Ok(r)) = &self.dedup {
                if r.clusters_found > 0 && r.dry_run {
                    let puede = !girando;
                    if ui.add_enabled(puede, egui::Button::new(i18n::tr("Fundir"))).clicked() {
                        self.lanza_dedup(false);
                    }
                }
            }
        });
        // El informe, junto al botón que lo pidió.
        match &self.dedup {
            Some(Err(e)) => {
                ui.colored_label(theme::red(), format!("⚠ {e}"));
            }
            Some(Ok(r)) if r.clusters_found == 0 => {
                ui.label(
                    egui::RichText::new(i18n::trf(
                        "Ninguna repetida entre las {n} más recientes.",
                        &[("n", &r.scanned.to_string())],
                    ))
                    .size(theme::FS_CAPTION)
                    .color(theme::faint()),
                );
            }
            Some(Ok(r)) => {
                ui.label(
                    egui::RichText::new(if r.dry_run {
                        i18n::trf(
                            "{grupos} grupos · {memorias} memorias se fundirían en otra, de \
                             {miradas} miradas. No se ha tocado nada todavía.",
                            &[
                                ("grupos", &r.clusters_found.to_string()),
                                ("memorias", &r.memories_merged.to_string()),
                                ("miradas", &r.scanned.to_string()),
                            ],
                        )
                    } else {
                        i18n::trf(
                            "{grupos} grupos fundidos · {memorias} memorias marcadas. No se \
                             borró ninguna: quedan etiquetadas y fuera de las consultas vivas.",
                            &[
                                ("grupos", &r.clusters_found.to_string()),
                                ("memorias", &r.memories_merged.to_string()),
                            ],
                        )
                    })
                    .size(theme::FS_CAPTION)
                    .color(if r.dry_run { theme::amber() } else { theme::acc() }),
                );
                // Cuáles, con nombre. Un contador sin la lista pide fiarse de un
                // número, y de lo que hay que fiarse es del criterio.
                for c in r.clusters.iter().take(8) {
                    ui.label(
                        egui::RichText::new(format!(
                            "· «{}» absorbe {} — parecido {:.0} %",
                            c.canonical_title,
                            if c.merged_ids.len() == 1 {
                                "1 memoria".to_string()
                            } else {
                                format!("{} memorias", c.merged_ids.len())
                            },
                            c.overlap_score * 100.0
                        ))
                        .size(theme::FS_MICRO)
                        .color(theme::txt3()),
                    );
                }
            }
            None => {}
        }
        // La búsqueda se PIDE dentro del match (que tiene prestado `self.mems`)
        // y se EJECUTA al salir. `run_semantic_search` necesita `&mut self`, así
        // que llamarla ahí dentro no compila — y forzarlo con un clon de las
        // memorias sería copiar un vector entero por frame para evitar un
        // booleano.
        let mut pedir_semantica = false;
        // El borrado, por lo mismo: dentro del préstamo solo se ANOTA.
        let confirmado = self.mem_confirm;
        let mut armar: Option<i64> = None;
        let mut borrar_id: Option<i64> = None;
        let mut fijar: Option<(i64, bool)> = None;
        // La etiqueta que se acaba de pulsar, para filtrar por ella. Fuera del
        // bucle porque cambiar el filtro dentro sería reordenar la lista que se
        // está recorriendo.
        let mut nuevo_filtro: Option<String> = None;

        match &self.mems {
            Err(e) => {
                ui.colored_label(theme::red(), format!("⚠ {e}"));
                ui.label(
                    egui::RichText::new(i18n::tr(
                        "Abre Lucy al menos una vez para crear la DB, o corre desde el mismo usuario.",
                    ))
                    .weak(),
                );
            }
            Ok(mems) => {
                let q = self.mem_search.to_lowercase();
                ui.horizontal(|ui| {
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut self.mem_search)
                            .hint_text(i18n::tr("filtrar por texto — Intro para búsqueda semántica"))
                            .desired_width(ui.available_width() - 108.0),
                    );
                    let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button(i18n::tr("◈ Semántica")).clicked() || enter {
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
                            ui.colored_label(theme::amber(), format!("⚠ {e}"));
                            ui.label(
                                egui::RichText::new(i18n::tr(
                                    "La búsqueda semántica necesita Ollama con un modelo de \
                                     embeddings (ollama pull nomic-embed-text).",
                                ))
                                .small()
                                .color(theme::txt3()),
                            );
                        }
                        Ok((hits, notes)) => {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(i18n::trf("{n} por similitud", &[("n", &hits.len().to_string())]))
                                    .small()
                                    .color(theme::acc()),
                            );
                            // Las filas descartadas se DICEN. Enseñar menos
                            // resultados sin explicar por qué es el fallo que
                            // este proyecto lleva persiguiendo toda la semana.
                            for n in notes {
                                ui.label(
                                    egui::RichText::new(format!("⚠ {n}"))
                                        .small()
                                        .color(theme::amber()),
                                );
                            }
                            for h in hits {
                                egui::Frame::group(ui.style()).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("{:.0}%", h.score * 100.0))
                                                .small()
                                                // Un parecido NO es un uso: en
                                                // la paleta de uso, más es peor,
                                                // y aquí más es mejor. Pintaba
                                                // de rojo justo los aciertos
                                                // buenos.
                                                .color(theme::match_color(h.score)),
                                        );
                                        ui.label(
                                            egui::RichText::new(
                                                h.text.chars().take(140).collect::<String>(),
                                            )
                                            .color(theme::txt2()),
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
                            // POR EL NÚMERO TAMBIÉN. Cada tarjeta pinta su id
                            // abajo —«#468»— y el carril de Trace dice «memoria
                            // 468» al guardarla. Con las dos cosas a la vista, lo
                            // natural es escribir ese número aquí; y como el
                            // filtro solo miraba texto, la respuesta era una
                            // lista vacía. Que es indistinguible de «esa memoria
                            // no existe», y llevó a pensar que Lucy mentía al
                            // decir que la había guardado.
                            //
                            // Se acepta con y sin almohadilla: se copia de los
                            // dos sitios y en uno la lleva.
                            || {
                                let n = q.trim_start_matches('#');
                                !n.is_empty()
                                    && n.chars().all(|c| c.is_ascii_digit())
                                    && m.id.to_string() == n
                            }
                    })
                    .collect();
                ui.label(
                    egui::RichText::new(i18n::trf(
                        "{vivas} de {total} memorias vivas",
                        &[
                            ("vivas", &filtered.len().to_string()),
                            ("total", &mems.len().to_string()),
                        ],
                    ))
                    .small()
                    .weak(),
                );
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Escalonadas: la lista se posa en vez de aparecer de
                        // golpe. Solo las seis primeras llevan retraso — ver
                        // `entrada_lista`.
                        for (n, m) in filtered.iter().enumerate() {
                            let t = entrada_lista(ui.ctx(), egui::Id::new(("mem-fila", m.id)), n);
                            ui.scope(|ui| {
                            ui.multiply_opacity(t);
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Los puntos toman el color del NIVEL, no
                                    // el acento fijo: así una memoria de
                                    // importancia 3 se distingue de un vistazo
                                    // sin tener que contar puntos.
                                    // LA CHINCHETA SE DEDUCE DE LA IMPORTANCIA,
                                    // y no es un atajo: `memories::set_pinned`
                                    // escribe las dos columnas a la vez y es el
                                    // único escritor de `pinned` en las dos
                                    // aplicaciones. Leerla haría falta añadir el
                                    // campo a `AgentMemory`, que es el tipo que
                                    // cruza el puente IPC de la app en
                                    // producción. Hay un test de integración que
                                    // fija el invariante.
                                    let fija = m.importance >= lucy_core::memories::FIJADA;
                                    let pin = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new(if fija { "📌" } else { "○" })
                                                .size(11.0)
                                                .color(if fija {
                                                    theme::amber()
                                                } else {
                                                    theme::faint()
                                                }),
                                        )
                                        .frame(false),
                                    );
                                    if pin
                                        .on_hover_text(i18n::tr(if fija {
                                            "Fijada: entra en TODOS los prompts. Pulsa para soltarla."
                                        } else {
                                            "Fijar: que Lucy la tenga presente siempre, venga o no al caso"
                                        }))
                                        .clicked()
                                    {
                                        fijar = Some((m.id, !fija));
                                    }
                                    // LOS PUNTOS DICEN QUÉ SON AL SEÑALARLOS.
                                    // Tres puntos de colores sin leyenda son un
                                    // adorno: se ven en cada fila y no se sabe
                                    // si cuentan algo, miden algo o marcan algo.
                                    // Y aquí importa saberlo, porque la
                                    // importancia decide qué recuerda Lucy
                                    // cuando no cabe todo.
                                    let dots = "●".repeat(m.importance.clamp(1, 3) as usize);
                                    ui.label(
                                        egui::RichText::new(dots)
                                            .color(theme::importance_color(m.importance))
                                            .small(),
                                    )
                                    .on_hover_text(i18n::tr(match m.importance {
                                        i if i >= lucy_core::memories::FIJADA => {
                                            "Fijada · entra en todos los prompts"
                                        }
                                        3 => "Importancia alta · se recuerda antes que las demás",
                                        2 => "Importancia normal",
                                        _ => "Importancia baja · la última en entrar si no cabe todo",
                                    }));
                                    let title = if m.title.trim().is_empty() {
                                        m.content.chars().take(64).collect::<String>()
                                    } else {
                                        m.title.clone()
                                    };
                                    ui.label(egui::RichText::new(title).strong());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let armado =
                                                confirmado == Some((MemTab::Memorias, m.id));
                                            let b = if armado {
                                                egui::Button::new(
                                                    egui::RichText::new(i18n::tr("¿borrar?"))
                                                        .color(theme::red())
                                                        .small(),
                                                )
                                            } else {
                                                egui::Button::new(
                                                    egui::RichText::new("🗑").small(),
                                                )
                                            };
                                            if ui.add(b).clicked() {
                                                if armado {
                                                    borrar_id = Some(m.id);
                                                } else {
                                                    armar = Some(m.id);
                                                }
                                            }
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
                                // ETIQUETAS COMO CHIPS Y NO COMO UNA CADENA.
                                // Salían tal cual venían de la base —
                                // `crystal,leccion`, en AZUL— y eso tenía dos
                                // problemas. El azul es el color del OPERADOR en
                                // esta aplicación: usarlo para datos hace que una
                                // etiqueta parezca algo que escribiste tú. Y una
                                // lista separada por comas no se puede pulsar,
                                // así que la pregunta obvia al ver una etiqueta
                                // —«enséñame las demás de esto»— había que
                                // teclearla en el filtro.
                                for t in mem_tags(&m.tags) {
                                    if tag_chip(ui, &t) {
                                        nuevo_filtro = Some(t.clone());
                                    }
                                }
                                ui.label(egui::RichText::new(format!("#{}", m.id)).small().weak());
                            });
                            });
                        }
                    });
            }
        }

        // Fuera del préstamo de `self.mems`.
        if let Some(t) = nuevo_filtro {
            self.mem_search = t;
            // El filtro por texto ya casa contra las etiquetas, así que no hace
            // falta un modo aparte: pulsar una etiqueta es escribirla.
        }
        if let Some((id, on)) = fijar {
            match lucy_core::memories::set_pinned(id, on) {
                Ok(()) => self.mems = load_memories(),
                Err(e) => self.mems = Err(e),
            }
        }
        if let Some(id) = armar {
            self.mem_confirm = Some((MemTab::Memorias, id));
        }
        if let Some(id) = borrar_id {
            self.mem_confirm = None;
            match lucy_core::memories::delete(id) {
                Ok(()) => self.mems = load_memories(),
                Err(e) => self.mems = Err(e),
            }
        }
        if pedir_semantica {
            self.run_semantic_search();
        }
    }

    pub(crate) fn mem_tab_cristales(&mut self, ui: &mut egui::Ui) {
        // MEDIDO: esta cabecera pedía 790 px pasara lo que pasara. Con la
        // ventana a 520 se salía por 270, y con ella se iba el ancho del
        // contenido del `ScrollArea` de más abajo — el mismo mecanismo que
        // rompía los títulos de los cristales.
        //
        // La frase va DEBAJO del botón y no a su lado. En un `horizontal` un
        // `Label` no parte, así que ponerla ahí obliga a elegir entre truncar una
        // explicación —que entonces no explica— o dejar que empuje. Debajo
        // envuelve, que es lo que hace la prosa.
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("↻ Recargar")).clicked() {
                self.cristales = Some(lucy_core::crystals::list(100));
            }
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(i18n::tr(
                "Cada cristal es una sesión destilada. Se escriben solos al cerrar turnos; \
                 sus lecciones ya son memorias y sobreviven aunque borres el cristal.",
            ))
            .size(theme::FS_CAPTION)
            .color(theme::faint()),
        );
        ui.add_space(4.0);
        let mut borrar: Option<i64> = None;
        let mut armar: Option<i64> = None;
        let confirmado = self.mem_confirm;
        match &self.cristales {
            None => {}
            Some(Err(e)) => {
                ui.colored_label(theme::red(), format!("⚠ {e}"));
            }
            Some(Ok(v)) if v.is_empty() => {
                ui.label(
                    egui::RichText::new(i18n::tr(
                        "Todavía no hay ninguno. Salen solos: una conversación con al menos \
                         cuatro turnos y tres comandos o lecturas se destila al cerrar el turno.",
                    ))
                    .color(theme::txt3()),
                );
            }
            Some(Ok(v)) => {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for c in v {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            // EL ANCHO SE REPARTE ANTES DE DIBUJAR NADA, que es
                            // el patrón que ya usa `fila` y por el mismo motivo.
                            //
                            // Aquí había un `ui.label` con la narrativa entera
                            // dentro de un `ui.horizontal`, y un `Label` en un
                            // horizontal HEREDA «no partas»: crecía hasta donde
                            // hiciera falta. Con una narrativa de dos líneas, el
                            // título se salía por el borde derecho de la ventana
                            // y empujaba fuera el botón de borrar y la hora.
                            //
                            // Y NO SE QUEDABA AHÍ. Ese desbordamiento infla el
                            // ancho del CONTENIDO del `ScrollArea`, y ese ancho
                            // persiste al fotograma siguiente: a partir de
                            // entonces `available_width()` viene inflado para
                            // todo lo de dentro, así que los hitos y las
                            // lecciones —que sí envuelven— envolvían a un ancho
                            // mayor que la ventana y también se salían. Un solo
                            // desborde envenenaba la lista entera, que es lo que
                            // se veía en el pantallazo: los cuatro cristales
                            // cortados por la misma vertical.
                            let total = ui.available_width();
                            let w_der = 96.0;
                            let w_tit = (total - w_der - GAP).max(160.0);
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(w_tit, 0.0),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&c.narrativa).strong(),
                                            )
                                            .wrap(),
                                        );
                                    },
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let armado =
                                            confirmado == Some((MemTab::Cristales, c.id));
                                        let b = if armado {
                                            egui::Button::new(
                                                egui::RichText::new(i18n::tr("¿borrar?"))
                                                    .color(theme::red())
                                                    .small(),
                                            )
                                        } else {
                                            egui::Button::new(egui::RichText::new("🗑").small())
                                        };
                                        if ui.add(b).clicked() {
                                            if armado {
                                                borrar = Some(c.id);
                                            } else {
                                                armar = Some(c.id);
                                            }
                                        }
                                        ui.label(
                                            egui::RichText::new(rel_time(c.creado)).small().weak(),
                                        );
                                    },
                                );
                            });
                            for h in &c.hitos {
                                ui.label(
                                    egui::RichText::new(format!("· {h}"))
                                        .small()
                                        .color(theme::txt2()),
                                );
                            }
                            for l in &c.lecciones {
                                ui.label(
                                    egui::RichText::new(format!("→ {l}"))
                                        .small()
                                        .color(theme::acc()),
                                );
                            }
                            if !c.archivos.is_empty() {
                                ui.label(
                                    egui::RichText::new(c.archivos.join(" · "))
                                        .size(theme::FS_MICRO)
                                        .color(theme::txt3()),
                                );
                            }
                            ui.label(
                                egui::RichText::new(i18n::trf(
                                    "#{id} · sesión {sesion} · {chars} caracteres leídos",
                                    &[
                                        ("id", &c.id.to_string()),
                                        ("sesion", &c.session_id),
                                        ("chars", &c.caracteres.to_string()),
                                    ],
                                ))
                                .size(theme::FS_MICRO)
                                .weak(),
                            );
                        });
                    }
                });
            }
        }
        if let Some(id) = armar {
            self.mem_confirm = Some((MemTab::Cristales, id));
        }
        if let Some(id) = borrar {
            self.mem_confirm = None;
            match lucy_core::crystals::delete(id) {
                Ok(()) => self.cristales = Some(lucy_core::crystals::list(100)),
                Err(e) => self.cristales = Some(Err(e)),
            }
        }
    }

    pub(crate) fn mem_tab_insights(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("↻ Recargar")).clicked() {
                self.recarga_insights();
            }
            ui.label(
                egui::RichText::new(i18n::tr(
                    "Un patrón es lo que se repite entre memorias que nadie escribió juntas. \
                     Reencontrarlo lo refuerza: la confianza sube con cada vez.",
                ))
                .size(theme::FS_CAPTION)
                .color(theme::faint()),
            );
        });
        // LOS QUE EL OPERADOR HA DESMENTIDO. Es la única señal de que la
        // destilación está produciendo basura: las filas descartadas
        // desaparecen de la lista por definición, así que sin este contador un
        // corpus que genera diez perogrulladas por semana se ve exactamente
        // igual que uno que genera un patrón bueno al mes. Si sube deprisa, lo
        // que hay que tocar es el agrupado (`MIN_PARECIDO`, `MIN_GRUPO`), no
        // seguir descartando a mano.
        let n_desc = self.insights_desc;
        if n_desc > 0 {
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(if n_desc == 1 {
                    i18n::tr("1 patrón descartado — no volverá").to_string()
                } else {
                    i18n::trf(
                        "{n} patrones descartados — no volverán",
                        &[("n", &n_desc.to_string())],
                    )
                })
                .size(theme::FS_MICRO)
                .color(theme::faint()),
            )
            .on_hover_text(i18n::tr(
                "Un patrón descartado deja su huella puesta, así que la reflexión de cada noche \
                 no puede volver a darlo de alta. Si este número sube deprisa, lo que falla es el \
                 agrupado, no los patrones.",
            ));
        }
        ui.add_space(4.0);
        let mut borrar: Option<i64> = None;
        let mut armar: Option<i64> = None;
        let confirmado = self.mem_confirm;
        match &self.insights_l {
            None => {}
            Some(Err(e)) => {
                ui.colored_label(theme::red(), format!("⚠ {e}"));
            }
            Some(Ok(v)) if v.is_empty() => {
                ui.label(
                    egui::RichText::new(i18n::tr(
                        "Todavía no hay ninguno. Hacen falta al menos cuatro memorias del mismo \
                         asunto con más de cinco días — la reflexión corre sola cada día, o \
                         desde Mantenimiento → Reflexionar ahora.",
                    ))
                    .color(theme::txt3()),
                );
            }
            Some(Ok(v)) => {
                // CUÁLES ESTÁN DIRIGIENDO A LUCY AHORA MISMO. De los cien que
                // lista el panel, solo los primeros que pasan el listón entran
                // en el prompt de cada turno; el resto está guardado y callado.
                // Sin la marca, el operador que ve un patrón equivocado no sabe
                // si está corrigiendo algo que importa o borrando una fila que
                // no se manda — y desde que los insights llegan al prompt, esa
                // diferencia es la que hace útil el botón de borrar.
                //
                // Sale de `seleccion`, la misma función que monta el bloque, para
                // que el panel no pueda señalar unos y el prompt llevar otros.
                let en_prompt: std::collections::HashSet<i64> =
                    lucy_core::insights::seleccion().into_iter().map(|(id, _)| id).collect();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for i in v {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("{:.0}%", i.confianza * 100.0))
                                        .strong()
                                        .color(theme::match_color(i.confianza as f32)),
                                );
                                if en_prompt.contains(&i.id) {
                                    ui.label(
                                        egui::RichText::new(i18n::tr("· en uso"))
                                            .small()
                                            .color(theme::acc()),
                                    )
                                    .on_hover_text(i18n::tr(
                                        "Este patrón viaja en el prompt de cada turno. Bórralo si \
                                         está equivocado.",
                                    ));
                                }
                                ui.label(egui::RichText::new(&i.contenido).color(theme::txt2()));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let armado =
                                            confirmado == Some((MemTab::Insights, i.id));
                                        let b = if armado {
                                            egui::Button::new(
                                                egui::RichText::new(i18n::tr("¿borrar?"))
                                                    .color(theme::red())
                                                    .small(),
                                            )
                                        } else {
                                            egui::Button::new(egui::RichText::new("🗑").small())
                                        };
                                        if ui.add(b).clicked() {
                                            if armado {
                                                borrar = Some(i.id);
                                            } else {
                                                armar = Some(i.id);
                                            }
                                        }
                                    },
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(if i.refuerzos == 1 {
                                        i18n::tr("visto 1 vez").to_string()
                                    } else {
                                        i18n::trf("visto {n} veces", &[("n", &i.refuerzos.to_string())])
                                    })
                                    .small()
                                    .weak(),
                                );
                                ui.label(
                                    egui::RichText::new(i18n::trf("{n} memorias detrás", &[("n", &i.fuentes.to_string())]))
                                        .small()
                                        .weak(),
                                );
                                if !i.conceptos.is_empty() {
                                    ui.label(
                                        egui::RichText::new(i.conceptos.join(" · "))
                                            .small()
                                            .color(theme::blue()),
                                    );
                                }
                                ui.label(
                                    egui::RichText::new(rel_time(i.actualizado)).small().weak(),
                                );
                            });
                        });
                    }
                });
            }
        }
        if let Some(id) = armar {
            self.mem_confirm = Some((MemTab::Insights, id));
        }
        if let Some(id) = borrar {
            self.mem_confirm = None;
            // DESCARTAR Y NO BORRAR. Lo que el operador quiere decir con este
            // botón es «este patrón es falso», y borrar la fila no lo conseguía:
            // sin ella no había choque de huella, así que la pasada de
            // mantenimiento de esa misma noche destilaba el mismo grupo de
            // memorias y lo volvía a meter a 0,50 como si fuera nuevo. Desde
            // que los insights llegan al prompt, eso es un patrón desmentido
            // dirigiendo cada turno otra vez.
            match lucy_core::insights::descarta(id) {
                Ok(()) => self.recarga_insights(),
                Err(e) => self.insights_l = Some(Err(e)),
            }
        }
    }

    pub(crate) fn mem_tab_documentos(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let ingiriendo = self.doc_rx.is_some();
            if ui
                .add_enabled(
                    !ingiriendo,
                    egui::Button::new(i18n::tr(if ingiriendo { "Ingiriendo…" } else { "＋ Ingerir documento" })),
                )
                .clicked()
            {
                // El diálogo es MODAL del sistema: bloquea este hilo mientras
                // está abierto, igual que el de adjuntos. Aceptable porque lo
                // abre un clic — no pasa solo.
                if let Some(ruta) = rfd::FileDialog::new()
                    .add_filter("Documentos", &{
                        let mut exts: Vec<&str> = vec!["pdf"];
                        exts.extend_from_slice(lucy_core::docs::TEXTO_PLANO);
                        exts
                    })
                    .pick_file()
                {
                    self.doc_stop =
                        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let stop = self.doc_stop.clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        lucy_core::docs::ingest(&ruta, &tx, &stop);
                    });
                    self.doc_rx = Some(rx);
                    self.doc_estado = Some((i18n::tr("Extrayendo texto…").into(), false));
                }
            }
            if ingiriendo && ui.button(i18n::tr("Cancelar")).clicked() {
                self.doc_stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if ui.button(i18n::tr("↻ Recargar")).clicked() {
                self.docs_l = Some(lucy_core::docs::list());
            }
            ui.label(
                egui::RichText::new(i18n::tr(
                    "Lo ingerido alimenta el recuerdo y a pdf_search. Los secretos se \
                     redactan al entrar.",
                ))
                .size(theme::FS_CAPTION)
                .color(theme::faint()),
            );
        });
        if let Some((linea, es_error)) = &self.doc_estado {
            ui.label(
                egui::RichText::new(linea)
                    .size(theme::FS_CAPTION)
                    .color(if *es_error { theme::red() } else { theme::amber() }),
            );
        }
        ui.add_space(4.0);
        let mut borrar: Option<String> = None;
        let mut armar: Option<String> = None;
        let confirmado = self.doc_confirm.clone();
        match &self.docs_l {
            None => {}
            Some(Err(e)) => {
                ui.colored_label(theme::red(), format!("⚠ {e}"));
            }
            Some(Ok(v)) if v.is_empty() => {
                ui.label(
                    egui::RichText::new(i18n::tr(
                        "Ningún documento todavía. Un manual ingerido contesta preguntas sin \
                         que nadie lo mencione — es la fuente principal de la memoria.",
                    ))
                    .color(theme::txt3()),
                );
            }
            Some(Ok(v)) => {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for d in v {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(&d.nombre).strong());
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let armado =
                                            confirmado.as_deref() == Some(d.id.as_str());
                                        let b = if armado {
                                            egui::Button::new(
                                                egui::RichText::new(i18n::tr("¿borrar?"))
                                                    .color(theme::red())
                                                    .small(),
                                            )
                                        } else {
                                            egui::Button::new(egui::RichText::new("🗑").small())
                                        };
                                        if ui.add(b).clicked() {
                                            if armado {
                                                borrar = Some(d.id.clone());
                                            } else {
                                                armar = Some(d.id.clone());
                                            }
                                        }
                                        ui.label(
                                            egui::RichText::new(rel_time(d.creado)).small().weak(),
                                        );
                                    },
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(i18n::trf(
                                        "{n} trozos",
                                        &[("n", &d.trozos.to_string())],
                                    ))
                                        .small()
                                        .weak(),
                                );
                                // MENOS VECTORES QUE TROZOS SE DICE EN ÁMBAR: ese
                                // documento solo se encuentra por palabras, y si
                                // nadie lo ve, «Lucy no encuentra el manual» no
                                // tiene diagnóstico.
                                if d.vectorizados < d.trozos {
                                    ui.label(
                                        egui::RichText::new(i18n::trf(
                                            "{con} de {total} con vector — el resto solo \
                                             se encuentra por palabras",
                                            &[
                                                ("con", &d.vectorizados.to_string()),
                                                ("total", &d.trozos.to_string()),
                                            ],
                                        ))
                                        .small()
                                        .color(theme::amber()),
                                    );
                                } else {
                                    ui.label(
                                        egui::RichText::new(i18n::tr("buscable por significado"))
                                            .small()
                                            .color(theme::acc()),
                                    );
                                }
                                ui.label(
                                    egui::RichText::new(&d.ruta)
                                        .size(theme::FS_MICRO)
                                        .color(theme::txt3()),
                                );
                            });
                        });
                    }
                });
            }
        }
        if let Some(id) = armar {
            self.doc_confirm = Some(id);
        }
        if let Some(id) = borrar {
            self.doc_confirm = None;
            match lucy_core::docs::delete(&id) {
                Ok(()) => self.docs_l = Some(lucy_core::docs::list()),
                Err(e) => self.docs_l = Some(Err(e)),
            }
        }
    }

    pub(crate) fn mem_tab_principios(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(i18n::tr(
                "Un principio entra en TODOS los turnos, venga o no al caso — su valor está \
                 justo en los turnos donde a nadie se le habría ocurrido recordarlo. Por eso \
                 son pocos.",
            ))
            .size(theme::FS_CAPTION)
            .color(theme::faint()),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let te = ui.add(
                egui::TextEdit::singleline(&mut self.princ_nueva)
                    .hint_text(i18n::tr("en producción avisa antes de reiniciar un servicio"))
                    .desired_width(ui.available_width() - 96.0),
            );
            let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button(i18n::tr("＋ Añadir")).clicked() || enter) && !self.princ_nueva.trim().is_empty() {
                match lucy_core::principles::add("", self.princ_nueva.trim(), None) {
                    Ok(_) => {
                        self.princ_nueva.clear();
                        self.principios_l = Some(lucy_core::principles::list());
                    }
                    Err(e) => self.principios_l = Some(Err(e)),
                }
            }
        });
        ui.add_space(4.0);
        let mut borrar: Option<i64> = None;
        let mut armar: Option<i64> = None;
        let mut cambiar: Option<(i64, bool)> = None;
        let confirmado = self.mem_confirm;
        match &self.principios_l {
            None => {}
            Some(Err(e)) => {
                ui.colored_label(theme::red(), format!("⚠ {e}"));
                if ui.button(i18n::tr("↻ Reintentar")).clicked() {
                    self.principios_l = Some(lucy_core::principles::list());
                }
            }
            Some(Ok(v)) if v.is_empty() => {
                ui.label(
                    egui::RichText::new(i18n::tr("Todavía no hay ninguno. También se dictan con /principio."))
                        .color(theme::txt3()),
                );
            }
            Some(Ok(v)) => {
                let activos = v.iter().filter(|p| p.activo).count();
                if activos >= lucy_core::principles::MAX_ACTIVOS {
                    ui.label(
                        egui::RichText::new(i18n::trf(
                            "Hay {activos} activos y en el prompt entran {caben}: los que \
                             sobren no se aplican. Apaga los que ya no manden.",
                            &[
                                ("activos", &activos.to_string()),
                                ("caben", &lucy_core::principles::MAX_ACTIVOS.to_string()),
                            ],
                        ))
                        .small()
                        .color(theme::amber()),
                    );
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        for (n, p) in v.iter().enumerate() {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let mut activo = p.activo;
                                    if ui.checkbox(&mut activo, "").changed() {
                                        cambiar = Some((p.id, activo));
                                    }
                                    ui.label(
                                        egui::RichText::new(format!("[P{}]", n + 1))
                                            .small()
                                            .color(theme::blue()),
                                    );
                                    let texto = egui::RichText::new(&p.regla);
                                    ui.label(if p.activo {
                                        texto.color(theme::txt2())
                                    } else {
                                        texto.weak().strikethrough()
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let armado =
                                                confirmado == Some((MemTab::Principios, p.id));
                                            let b = if armado {
                                                egui::Button::new(
                                                    egui::RichText::new(i18n::tr("¿borrar?"))
                                                        .color(theme::red())
                                                        .small(),
                                                )
                                            } else {
                                                egui::Button::new(egui::RichText::new("🗑").small())
                                            };
                                            if ui.add(b).clicked() {
                                                if armado {
                                                    borrar = Some(p.id);
                                                } else {
                                                    armar = Some(p.id);
                                                }
                                            }
                                        },
                                    );
                                });
                            });
                        }
                    });
                }
            }
            if let Some((id, activo)) = cambiar {
                let _ = lucy_core::principles::set_enabled(id, activo);
                self.principios_l = Some(lucy_core::principles::list());
            }
            if let Some(id) = armar {
                self.mem_confirm = Some((MemTab::Principios, id));
            }
            if let Some(id) = borrar {
                self.mem_confirm = None;
                match lucy_core::principles::delete(id) {
                    Ok(()) => self.principios_l = Some(lucy_core::principles::list()),
                    Err(e) => self.principios_l = Some(Err(e)),
                }
            }
        }

        pub(crate) fn mem_tab_mantenimiento(&mut self, ui: &mut egui::Ui) {
            ui.label(
                // Traducida a los cinco idiomas desde que se escribió la pantalla, y
                // sin usar: faltaba el `tr`.
                egui::RichText::new(i18n::tr(
                    "Los dos trabajos corren solos por vencimiento — también si el programa \
                     estuvo cerrado cuando tocaba. Esto es para no esperar al plazo.",
                ))
                .size(theme::FS_CAPTION)
                .color(theme::faint()),
            );
            ui.add_space(6.0);
            let corriendo = self.mant_rx.is_some();
            let mut forzar: Option<&'static str> = None;
            // CON SCROLL, COMO LAS OTRAS CINCO. Ésta era la única de las seis
            // pestañas de Memoria sin él, y no se notaba porque en español, con la
            // ventana ancha y las dos pasadas saliendo bien, cabe justo.
            //
            // Deja de caber en cuanto pasa cualquiera de estas: una nota de
            // `Cifras::Fallo`, que guarda el mensaje de error ENTERO y envuelve a
            // varias líneas; una nota vieja por `Cifras::Prosa`, que es texto libre;
            // el aviso de racha en blanco en los dos trabajos; o el alemán, que
            // alarga cada línea. Sin scroll, lo que se sale del panel no está
            // recortado: está fuera del alcance, y no hay forma de llegar a ello.
            //
            // `auto_shrink` a falso en los dos ejes, igual que las hermanas: sin eso
            // el área encoge al contenido y el ancho que calcula la fila de arriba
            // baila entre pintadas.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if let Some(info) = &self.mant_info {
                for (job, ultima, racha) in info {
                    let (titulo, cada, boton, explica) = match *job {
                        lucy_core::maintenance::CONSOLIDAR => (
                            "Consolidación",
                            lucy_core::maintenance::CADA_CONSOLIDAR,
                            "Consolidar ahora",
                            "funde memorias que dicen lo mismo; nada se borra",
                        ),
                        // EXPLÍCITO Y NO POR DESCARTE. Esta rama era el `_`, y con
                        // dos trabajos daba igual; con tres, la poda habría salido
                        // rotulada «Reflexión» y con el plazo de los insights, sin
                        // que nada fallara. Un `match` por descarte sobre una lista
                        // que crece es una etiqueta equivocada esperando su turno.
                        lucy_core::maintenance::PODA => (
                            "Poda",
                            lucy_core::maintenance::CADA_PODA,
                            "Podar ahora",
                            "quita lo vencido: un año de auditoría, tres meses de avisos ya vistos",
                        ),
                        _ => (
                            "Reflexión",
                            lucy_core::maintenance::CADA_INSIGHTS,
                            "Reflexionar ahora",
                            "busca patrones entre memorias con más de cinco días",
                        ),
                    };
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        // ── LOS DOS ANCHOS, ANTES DE DIBUJAR NADA ───────────────
                        //
                        // Esto era un `ui.horizontal` con las dos etiquetas y luego
                        // el botón por `right_to_left`, que es EXACTAMENTE el reparto
                        // que `fila` documenta como roto: un `Label` dentro de un
                        // `horizontal` hereda «no envolver», así que se extiende por
                        // encima del ancho disponible y el botón se queda sin sitio.
                        // Allí se midió: la fila empezaba setenta y seis píxeles
                        // fuera del panel y las etiquetas salían cortadas por delante.
                        //
                        // Aquí no se había arreglado porque `fila` no pasa por esta
                        // pantalla — tiene su propio marco y su propia altura.
                        //
                        // Y AGUANTA LOS CINCO IDIOMAS. «funde memorias que dicen lo
                        // mismo; nada se borra» son 46 caracteres en español y 68 en
                        // alemán; «Consolidar ahora» pasa a «Jetzt konsolidieren».
                        // Con el reparto de antes, el idioma decidía si el botón
                        // cabía.
                        //
                        // Quien cede es el texto, que envuelve sin perder nada. Un
                        // botón encogido deja de poder pulsarse.
                        let total = ui.available_width();
                        let w_texto = ancho_texto_mant(total);
                        let alto = theme::H_MD;
                        ui.allocate_ui_with_layout(
                            egui::vec2(total, alto),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_height(alto);
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.allocate_ui_with_layout(
                                    egui::vec2(w_texto, alto),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| {
                                        ui.set_max_width(w_texto);
                                        ui.spacing_mut().item_spacing.y = 1.0;
                                        ui.label(
                                            egui::RichText::new(i18n::tr(titulo)).strong(),
                                        );
                                        ui.label(
                                            egui::RichText::new(i18n::tr(explica))
                                                .size(theme::FS_CAPTION)
                                                .color(theme::faint()),
                                        );
                                    },
                                );
                                ui.add_space(GAP);
                                // POR `right`, no por un reparto propio: la nota de
                                // `fila` dice que un `set_max_width` dentro del
                                // reparto de derecha a izquierda mueve el borde desde
                                // el que ese reparto cuenta, y los controles dejan de
                                // llegar a su lado.
                                right(ui, alto, |ui| {
                                    if ui
                                        .add_enabled(
                                            !corriendo,
                                            egui::Button::new(i18n::tr(if corriendo {
                                                "Corriendo…"
                                            } else {
                                                boton
                                            })),
                                        )
                                        .clicked()
                                    {
                                        forzar = Some(job);
                                    }
                                });
                            },
                        );
                        match ultima {
                            None => {
                                ui.label(
                                    egui::RichText::new(i18n::tr(
                                        "Nunca ha corrido en esta base — correrá en la próxima \
                                         comprobación.",
                                    ))
                                    .small()
                                    .color(theme::amber()),
                                );
                            }
                            Some((cuando, nota)) => {
                                let faltan = (cuando + cada) - ahora_epoch();
                                ui.label(
                                    egui::RichText::new(i18n::trf(
                                        "Última vez {cuando} · {plazo}",
                                        &[
                                            ("cuando", &rel_time(*cuando)),
                                            (
                                                "plazo",
                                                &if faltan <= 0 {
                                                    i18n::tr(
                                                        "vencido: correrá en la próxima comprobación",
                                                    )
                                                    .to_string()
                                                } else {
                                                    i18n::trf(
                                                        "próxima en {plazo}",
                                                        &[("plazo", &dentro_de(faltan))],
                                                    )
                                                },
                                            ),
                                        ],
                                    ))
                                    .small()
                                    .weak(),
                                );
                                if !nota.is_empty() {
                                    ui.label(
                                        egui::RichText::new(nota_en_palabras(nota))
                                            .small()
                                            .color(theme::txt3()),
                                    );
                                }
                            }
                        }
                        // LA SERIE, QUE ES LO QUE LA NOTA SUELTA NO PUEDE DECIR.
                        // «0 elegibles · corpus demasiado pequeño» se lee igual si
                        // pasó ayer que si lleva pasando un mes, y son dos
                        // diagnósticos opuestos: el primero no es nada, el segundo
                        // dice que los umbrales del agrupado están mal calibrados
                        // para este corpus y que la reflexión lleva semanas
                        // gastando llamadas a Ollama para no producir nada.
                        //
                        // Desde TRES: una o dos pasadas en blanco son ruido normal
                        // —hay días sin memorias nuevas— y avisar de eso enseñaría a
                        // ignorar el aviso.
                        let (veces, desde) = *racha;
                        if veces >= 3 {
                            ui.label(
                                egui::RichText::new(i18n::trf(
                                    "Lleva {n} pasadas sin sacar nada, desde {cuando}",
                                    &[("n", &veces.to_string()), ("cuando", &rel_time(desde))],
                                ))
                                .small()
                                .color(theme::amber()),
                            )
                            .on_hover_text(i18n::tr(
                                "Una pasada en blanco no dice nada; muchas seguidas sí. Suele \
                                 significar que el corpus no da para agrupar todavía, o que los \
                                 umbrales de parecido están puestos para otro tamaño de corpus.",
                            ));
                        }
                    });
                    ui.add_space(4.0);
                }
            }
        });
        if let Some(job) = forzar {
            let stop = self.mant_stop.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let nota = lucy_core::maintenance::corre(job, &stop);
                let mut t = lucy_core::maintenance::Tanda::default();
                // Por NOMBRE y no por descarte, por lo mismo que el rótulo de
                // arriba: con dos trabajos un `else` acertaba siempre, y con tres
                // la poda forzada a mano se reportaba como una reflexión.
                match job {
                    lucy_core::maintenance::CONSOLIDAR => t.consolidado = Some(nota),
                    lucy_core::maintenance::PODA => t.podado = Some(nota),
                    _ => t.reflexionado = Some(nota),
                }
                let _ = tx.send(t);
            });
            // El mismo canal que la tanda automática: la nota llega por
            // `pump_mantenimiento`, que la anota en el Trace y refresca esta
            // pestaña.
            self.mant_rx = Some(rx);
        }
    }
}
