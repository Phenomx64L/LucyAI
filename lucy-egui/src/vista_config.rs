//! Configuracion: modelos, idioma, tema, hosts y credenciales.
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
    /// Modelo, privacidad, Ollama e interfaz: lo que gobierna cómo se comporta.
    /// `s` es la foto del equipo: la usa el panel «Este equipo», que vive al
    /// final de esta columna para que las dos queden a la misma altura.
    pub(crate) fn cfg_columna_izquierda(
        &mut self,
        ui: &mut egui::Ui,
        col: f32,
        s: &lucy_core::system::SysSnapshot,
    ) {
        // ── modelo y comportamiento ──────────────────────────────────────────
        let aviso = lucy_core::cloud::allowed(&self.chat_model, self.privacy).err();
        let mut privado = self.privacy;
        let mut tope = self.max_loops;
        let mut tope_gasto = self.spend_limit;
        let mut tono = self.tono;
        let mut enrutado = self.enrutado;
        let gastado = self.gasto_sesion();
        let modelo = self.chat_model.clone();
        let desc = i18n::modelo(lucy_core::models::describe(&modelo));
        panel(
            ui,
            col,
            icons::Icon::Sparkles,
            "Modelo y comportamiento",
            |_| {},
            |ui| {
                // La descripción SOLO si dice algo distinto del id. Para un
                // modelo de Ollama, `describe` devuelve el id tal cual, y la
                // fila quedaba con la misma cadena arriba y abajo — una línea
                // que no informa y que hace dudar de si son dos cosas distintas.
                let sub = (desc != modelo).then_some(desc.as_str());
                fila(ui, "Modelo activo", sub, false, |ui| {
                    // TRUNCADO: un id como `gemini-3.1-pro-preview::high` son
                    // veintiocho caracteres en monoespaciada, y en una ventana
                    // estrecha no cabe en su mitad. Sin esto pediría el ancho
                    // entero y volvería a desbordar la fila.
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&modelo)
                                .size(theme::FS_FOOTNOTE)
                                .monospace()
                                .color(theme::txt()),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&modelo);
                });
                // EL SEGMENTADO Y NO UNA CASILLA, porque los dos estados tienen
                // nombre y consecuencia. Una casilla marcada obliga a deducir
                // qué significa estar marcada, y para el ajuste que decide si
                // tus datos salen del equipo esa deducción no se puede pedir.
                fila(
                    ui,
                    "Modo privacidad",
                    Some("todo el tráfico a Ollama local"),
                    false,
                    |ui| {
                        if let Some(i) =
                            segmentado(
                                ui,
                                "privacidad",
                                180.0,
                                &["Activado", "Apagado"],
                                usize::from(!privado),
                            )
                        {
                            privado = i == 0;
                        }
                    },
                );
                fila(
                    ui,
                    "Tope de pasos seguidos",
                    Some("comandos encadenados sin aprobar, por orden"),
                    false,
                    |ui| {
                        // De uno en uno: cada paso es un comando que se ejecuta
                        // solo en la máquina de alguien, así que la unidad en la
                        // que se piensa este número es el paso.
                        numero(ui, &mut tope, MAX_LOOPS_MIN, MAX_LOOPS_MAX, 1.0, |d| {
                            d.speed(0.25)
                        });
                    },
                );
                // El gasto se enseña JUNTO al tope, no solo el tope: «llevas
                // 0,42 de 1,00» es lo que permite decidir si el número está
                // bien puesto. Un límite sin el consumo al lado es un número
                // que se pone a ojo una vez y no se vuelve a mirar.
                fila(
                    ui,
                    "Tope de gasto de la sesión",
                    Some(&if tope_gasto > 0.0 {
                        i18n::trf(
                            "llevas {g} · al cruzarlo se apaga el automático",
                            &[("g", &lucy_core::pricing::fmt_usd(gastado))],
                        )
                    } else {
                        i18n::trf(
                            "llevas {g} · 0 = sin límite",
                            &[("g", &lucy_core::pricing::fmt_usd(gastado))],
                        )
                    }),
                    false,
                    |ui| {
                        // De cuarto en cuarto: es dinero, y en dinero nadie
                        // piensa de céntimo en céntimo ni de dólar en dólar.
                        numero(ui, &mut tope_gasto, 0.0, 500.0, 0.25, |d| {
                            d.speed(0.05).prefix("$").max_decimals(2)
                        });
                    },
                );
                // ── EL VIGILANTE ────────────────────────────────────────
                //
                // Los avisos sin leer van AQUÍ y no en un icono aparte, porque
                // aquí es donde el operador viene a decidir cuánto habla Lucy.
                // Ver `lucy_core::notify`: el globo de Windows se pierde en
                // cuanto se va de pantalla, así que este recuento es la única
                // memoria fiable de lo que se dijo.
                let sin_ver = lucy_core::notify::cuantos_sin_ver();
                fila(
                    ui,
                    "Avisos sin leer",
                    Some(if sin_ver == 0 {
                        i18n::tr("nada pendiente").to_string()
                    } else {
                        i18n::tr("el globo de Windows se va; esto no").to_string()
                    })
                    .as_deref(),
                    false,
                    |ui| {
                        ui.label(
                            egui::RichText::new(sin_ver.to_string())
                                .strong()
                                .color(if sin_ver > 0 { theme::amber() } else { theme::txt3() }),
                        );
                        if ui
                            .add_enabled(sin_ver > 0, egui::Button::new(i18n::tr("Marcar leídos")))
                            .clicked()
                        {
                            let _ = lucy_core::notify::marca_visto(None);
                            self.avisos_sin_ver = 0;
                        }
                    },
                );

                // EL INTERRUPTOR DE LA REDACCIÓN, con lo medido al lado y no una
                // etiqueta neutra. Un ajuste que no dice qué hace se deja como
                // viene o se enciende a ciegas; éste tiene una respuesta medida
                // —hoy la plantilla gana— y esa respuesta cambia el día que el
                // vigilante tenga cosas más difíciles que decir.
                let mut redactar = lucy_core::redacta::activa();
                fila(
                    ui,
                    "Que un modelo local reescriba los avisos",
                    // El hueco de veintiséis espacios que había en medio de esta
                    // frase no era sangría: estaba DENTRO de la cadena, así que
                    // se pintaba. En la pantalla se leía «hoy con un modelo» y
                    // luego un vacío antes de «pequeño».
                    Some(
                        "las cifras se comprueban una a una contra la medición; hoy con un \
                         modelo pequeño la plantilla suele salir mejor",
                    ),
                    false,
                    |ui| {
                        if ui.checkbox(&mut redactar, "").changed() {
                            lucy_core::redacta::pon_activa(redactar);
                        }
                    },
                );

                // LO QUE COSTÓ DE VERDAD, Y NO SOLO ESTA SESIÓN.
                //
                // El tope de arriba es POR SESIÓN y se reinicia en cada
                // arranque, así que hasta ahora abrir Lucy cinco veces en un día
                // permitía gastar cinco topes sin que nada lo dijera. Y el
                // recuento de tokens vivía en el struct de cada pestaña: al
                // cerrar el programa desaparecía, y con él la única respuesta
                // posible a «¿cuánto me costó Lucy este mes?».
                //
                // La tarifa, los recuentos y hasta la tabla estaban desde
                // siempre —`token_usage` tiene casi mil filas que escribió la
                // app vieja hasta agosto— y lo que faltaba era la línea que las
                // guarda y ésta que las lee.
                if self.gasto_hist.is_none() {
                    self.gasto_hist = lucy_core::usage::resumen(30).ok();
                }
                if let Some(h) = &self.gasto_hist {
                    // EL DESGLOSE POR PARA QUÉ es la mitad útil: saber que se
                    // gastaron doce dólares no sugiere nada, y saber que cuatro
                    // se fueron en poner títulos sugiere titular con el modelo
                    // local.
                    let desglose = h
                        .por_para
                        .iter()
                        .filter(|(_, c, _)| *c > 0.0)
                        .map(|(k, c, _)| format!("{k} {}", lucy_core::pricing::fmt_usd(*c)))
                        .collect::<Vec<_>>()
                        .join(" · ");
                    fila(
                        ui,
                        "Gastado de verdad",
                        Some(&if desglose.is_empty() {
                            i18n::tr("todavía no hay nada apuntado en esta base").to_string()
                        } else {
                            desglose
                        }),
                        false,
                        |ui| {
                            ui.label(
                                egui::RichText::new(i18n::trf(
                                    "hoy {hoy} · 30 días {mes}",
                                    &[
                                        (
                                            "hoy",
                                            &lucy_core::pricing::fmt_usd(
                                                lucy_core::usage::gasto_de_hoy(),
                                            ),
                                        ),
                                        ("mes", &lucy_core::pricing::fmt_usd(h.total)),
                                    ],
                                ))
                                .strong(),
                            )
                            .on_hover_text(i18n::trf(
                                "{n} llamadas al modelo en 30 días · {ent} tokens de entrada, \
                                 {sal} de salida",
                                &[
                                    ("n", &h.llamadas.to_string()),
                                    ("ent", &h.entrada.to_string()),
                                    ("sal", &h.salida.to_string()),
                                ],
                            ));
                        },
                    );
                }
                // El enrutado AVISA, no cambia el modelo. Ver la cabecera de
                // `lucy_core::routing`: un enrutador que elige en silencio hace
                // que la respuesta que lees no venga del modelo que
                // seleccionaste — y aquí cambiar de modelo cambia lo que se
                // gasta.
                fila(
                    ui,
                    "Avisar si el modelo se queda corto",
                    Some("antes de mandar una tarea exigente \u{b7} no cambia el modelo por ti"),
                    false,
                    |ui| {
                        if let Some(i) = segmentado(
                            ui,
                            "enrutado",
                            180.0,
                            &["Activado", "Apagado"],
                            usize::from(!enrutado),
                        ) {
                            enrutado = i == 0;
                        }
                    },
                );
                // EL TONO NO TOCA LO QUE SE EJECUTA, solo cómo se redacta. En
                // mitad de un incidente, tres párrafos antes del comando son
                // tres párrafos que saltarse con el servicio caído; aprendiendo
                // el sistema, un comando a secas no enseña nada.
                fila(
                    ui,
                    "Personalidad de Lucy",
                    Some("cuánto se extiende al contestar · no cambia qué ejecuta ni qué avisa"),
                    aviso.is_none(),
                    |ui| {
                        let i = lucy_core::prompt::Tono::ALL
                            .iter()
                            .position(|t| *t == tono)
                            .unwrap_or(1);
                        let etiquetas: Vec<&str> = lucy_core::prompt::Tono::ALL
                            .iter()
                            .map(|t| i18n::tr(t.label()))
                            .collect();
                        if let Some(k) = segmentado(ui, "tono", 270.0, &etiquetas, i) {
                            tono = lucy_core::prompt::Tono::ALL[k];
                        }
                    },
                );
                if let Some(e) = &aviso {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("⚠ {e}"))
                            .size(theme::FS_CAPTION)
                            .color(theme::amber()),
                    );
                }
            },
        );
        self.privacy = privado;
        self.max_loops = tope;
        self.spend_limit = tope_gasto.max(0.0);
        self.tono = tono;
        self.enrutado = enrutado;

        // ── ollama ───────────────────────────────────────────────────────────
        //
        // DE ÉL DEPENDE LA MITAD DE LA MEMORIA y no lo decía ninguna pantalla.
        // Sin embebedor no hay recuerdo por significado —solo por palabras, que
        // encuentra bastante menos— y sin modelo de texto no hay cristales ni
        // patrones. Cuando algo de eso no aparece, la pregunta es siempre «¿está
        // Ollama?», y la respuesta había que buscarla en una terminal.
        //
        // Todo sale de la lista YA CACHEADA y de una función pura: esto no
        // cuesta una petición por frame, que fue el fallo de `list_models` sin
        // plazo y no se repite.
        ui.add_space(GAP_SECCION);
        let vivo = !self.models.is_empty();
        let n_modelos = self.models.len();
        let embebedor = self
            .models
            .iter()
            .any(|m| m.starts_with(lucy_core::vectors::DEFAULT_EMBED_MODEL));
        let destilador = lucy_core::crystals::elige(&self.models);
        let mut redetectar = false;
        panel(
            ui,
            col,
            icons::Icon::Database,
            "Ollama · modelos locales",
            |ui| {
                let t = if vivo {
                    format!("{n_modelos} detectados")
                } else {
                    "no responde".to_string()
                };
                insignia(ui, &t, vivo);
            },
            |ui| {
                // Las dos cosas que la memoria le pide, cada una con lo que se
                // PIERDE si falta — no un ✓/✗ que no dice qué se pierde.
                fila(
                    ui,
                    "Recuerdo por significado",
                    Some(if embebedor {
                        "busca por lo que quieres decir, no por las palabras exactas"
                    } else {
                        "sin él, Lucy recuerda solo por palabras y encuentra bastante menos"
                    }),
                    false,
                    |ui| {
                        insignia(
                            ui,
                            if embebedor {
                                lucy_core::vectors::DEFAULT_EMBED_MODEL
                            } else {
                                "falta"
                            },
                            embebedor,
                        );
                    },
                );
                fila(
                    ui,
                    "Cristales y patrones",
                    Some(match &destilador {
                        Some(_) => "destila las sesiones y busca lo que se repite",
                        None => "sin modelo de texto no se destila ninguna sesión",
                    }),
                    true,
                    |ui| {
                        insignia(
                            ui,
                            destilador.as_deref().unwrap_or("falta"),
                            destilador.is_some(),
                        );
                    },
                );
                if !embebedor {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "ollama pull {}",
                            lucy_core::vectors::DEFAULT_EMBED_MODEL
                        ))
                        .size(theme::FS_CAPTION)
                        .monospace()
                        .color(theme::amber()),
                    );
                }
                ui.add_space(8.0);
                row_align(ui, 24.0, egui::Align::Center, |ui| {
                    right(ui, 24.0, |ui| {
                        redetectar = ui.small_button(i18n::tr("↻ Redetectar")).clicked();
                    });
                });
            },
        );
        if redetectar {
            self.models = lucy_core::chat::list_models();
        }

        // ── interfaz ─────────────────────────────────────────────────────────
        ui.add_space(GAP_SECCION);
        let mut nuevo_tema: Option<theme::Mode> = None;
        let mut nuevo_motion: Option<bool> = None;
        let mut nueva_paleta: Option<usize> = None;
        panel(
            ui,
            col,
            icons::Icon::Settings,
            "Interfaz",
            |_| {},
            |ui| {
                let actual = theme::mode();
                let i = theme::Mode::ALL.iter().position(|m| *m == actual).unwrap_or(0);
                // LA EXPLICACIÓN DEPENDE DE LO ELEGIDO, y antes era siempre la
                // misma advertencia sobre «del sistema» aunque estuvieras en
                // oscuro fijo — un texto que no aplica a lo que tienes puesto se
                // lee una vez, se descarta, y con él se descarta el sitio donde
                // vive. En «del sistema» sí aparece, porque ahí es donde importa:
                // Windows tiene DOS ajustes de tema y mucha gente los tiene
                // cruzados, así que Lucy puede salir clara con la barra oscura y
                // parecer un fallo.
                let explica = match actual {
                    theme::Mode::Auto => {
                        "sigue a Windows — mira el ajuste de las APLICACIONES, no el de la \
                         barra de tareas: mucha gente los tiene cruzados"
                    }
                    theme::Mode::Dark => "fijo, sin seguir a Windows",
                    theme::Mode::Light => {
                        "fijo. Pensado para pantallas con reflejos; el oscuro es el tema de casa"
                    }
                };
                // EL IDIOMA, EL PRIMERO DE LA SECCIÓN. Es lo único de esta
                // pantalla que alguien puede necesitar sin entender el resto de
                // la pantalla, así que enterrarlo debajo del tema y del acento
                // sería pedirle que lea en un idioma que no tiene para llegar a
                // ponerlo en el suyo. Los idiomas se nombran EN SU IDIOMA por lo
                // mismo: «Deutsch» se reconoce, «Alemán» no si no sabes español.
                // UN DESPLEGABLE Y NO UN SEGMENTADO, que es lo que había. Cinco
                // nombres largos —«Português», «Français»— en la mitad de una
                // fila salen a cuarenta píxeles por opción: con la ventana ancha
                // se leen apretados y con la ventana estrecha no se leen. Un
                // segmentado es para tres opciones cortas; con cinco largas el
                // control correcto es otro.
                let idioma = i18n::lang();
                let mut nuevo_idioma = None;
                fila(
                    ui,
                    "Idioma",
                    // LA COBERTURA SE DICE EN LA PROPIA PANTALLA mientras sea
                    // parcial. Una aplicación a medio traducir se lee como una
                    // traducción rota; dicha de antemano, se lee como lo que es.
                    // Esta frase se borra cuando no quede pantalla sin pasar.
                    Some(
                        "de la interfaz y de lo que Lucy responde · traducidas esta pantalla, \
                         la navegación y la ayuda; las demás van en camino",
                    ),
                    false,
                    |ui| {
                        egui::ComboBox::from_id_salt("cfg-idioma")
                            .selected_text(idioma.nombre())
                            .width(180.0)
                            .show_ui(ui, |ui| {
                                for l in i18n::Lang::ALL {
                                    // El nombre EN SU IDIOMA: quien busca el
                                    // suyo lo busca como lo llama él, y si la
                                    // pantalla está ahora en uno que no entiende,
                                    // «Alemán» no le sirve para encontrar el
                                    // alemán.
                                    if ui
                                        .selectable_label(l == idioma, l.nombre())
                                        .clicked()
                                    {
                                        nuevo_idioma = Some(l);
                                    }
                                }
                            });
                    },
                );
                if let Some(l) = nuevo_idioma {
                    i18n::set(l);
                }
                fila(ui, "Tema", Some(explica), false, |ui| {
                    let etiquetas: Vec<&str> =
                        theme::Mode::ALL.iter().map(|m| i18n::tr(m.label())).collect();
                    if let Some(k) = segmentado(ui, "tema", 240.0, &etiquetas, i) {
                        if k != i {
                            nuevo_tema = Some(theme::Mode::ALL[k]);
                        }
                    }
                });
                // Y una muestra de lo que se está eligiendo. Cambiar de tema
                // repinta la ventana entera, así que una tira de color es
                // redundante ahí — pero no en «del sistema», donde lo que se ve
                // depende de un ajuste de Windows que puede cambiar solo al
                // anochecer, y saber en qué está resolviendo AHORA es la única
                // forma de entender por qué la ventana se ve como se ve.
                if actual == theme::Mode::Auto {
                    fila(
                        ui,
                        "Ahora mismo resuelve a",
                        None,
                        false,
                        |ui| {
                            insignia(
                                ui,
                                if theme::light() { "Claro" } else { "Oscuro" },
                                true,
                            );
                        },
                    );
                }
                // UN SOLO COLOR GOBIERNA TODO EL ACENTO —navegación activa,
                // actividad del agente, progreso, hecho— así que cambiarlo aquí
                // cambia la aplicación entera. Ni rojo ni ámbar en la lista: en
                // Lucy significan «ha fallado» y «cuidado», y un acento de ese
                // color haría que media pantalla pareciera una advertencia y que
                // una advertencia de verdad se leyera como decoración.
                let pal = theme::PALETAS
                    .iter()
                    .position(|p| p.clave == theme::paleta().clave)
                    .unwrap_or(0);
                fila(
                    ui,
                    "Color de acento",
                    Some("lo que se ilumina: navegación, progreso, hecho"),
                    false,
                    |ui| {
                        let etiquetas: Vec<&str> =
                            theme::PALETAS.iter().map(|p| p.nombre).collect();
                        if let Some(k) = segmentado(ui, "paleta", 300.0, &etiquetas, pal) {
                            if k != pal {
                                nueva_paleta = Some(k);
                            }
                        }
                    },
                );
                let mov = motion();
                fila(
                    ui,
                    "Animaciones",
                    Some(
                        "escritura progresiva y transiciones · LUCY_NO_MOTION=1 las apaga al \
                         arrancar",
                    ),
                    true,
                    |ui| {
                        if let Some(k) =
                            segmentado(
                                ui,
                                "animaciones",
                                180.0,
                                &["Activadas", "Apagadas"],
                                usize::from(!mov),
                            )
                        {
                            nuevo_motion = Some(k == 0);
                        }
                    },
                );
            },
        );
        if let Some(m) = nuevo_tema {
            self.tema_pendiente = Some(m);
        }
        if let Some(v) = nuevo_motion {
            set_motion(v);
        }
        if let Some(k) = nueva_paleta {
            // `cambia_paleta` y no `set_paleta`: hay cuatro colores que viven
            // dentro de los `Visuals` de egui y no se enteran de un cambio de
            // acento hasta que alguien los vuelve a poner. Ver la función.
            theme::cambia_paleta(ui.ctx(), k);
        }
        // ── este equipo ──────────────────────────────────────────────────────
        ui.add_space(GAP_SECCION);
        let elev = match lucy_core::elevate::state() {
            lucy_core::elevate::Elevation::Already => ("Administrador", true),
            lucy_core::elevate::Elevation::CanPrompt => ("Sin privilegios · UAC disponible", false),
            lucy_core::elevate::Elevation::Unavailable => {
                ("Sin privilegios · UAC desactivado", false)
            }
        };
        let db = db_path().map(|p| p.display().to_string()).unwrap_or_default();
        let lg = log_path().map(|p| p.display().to_string()).unwrap_or_default();
        panel(
            ui,
            col,
            icons::Icon::Server,
            "Este equipo",
            |_| {},
            |ui| {
                fila(ui, "Equipo", None, false, |ui| {
                    ui.label(
                        egui::RichText::new(&s.host)
                            .size(theme::FS_CAPTION)
                            .color(theme::txt2()),
                    );
                });
                fila(ui, "Sistema", None, false, |ui| {
                    ui.label(
                        egui::RichText::new(&s.os)
                            .size(theme::FS_CAPTION)
                            .color(theme::txt2()),
                    );
                });
                fila(ui, "Privilegios", None, false, |ui| {
                    insignia(ui, elev.0, elev.1);
                });
                for (i, (k, v)) in [("Base de datos", db), ("Log", lg)].into_iter().enumerate() {
                    let mut copiar = false;
                    fila(ui, k, None, i == 1, |ui| {
                        copiar = ghost_icon(ui, icons::Icon::Copy)
                            .on_hover_text(i18n::tr("Copiar la ruta"))
                            .clicked();
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&v)
                                    .size(theme::FS_MICRO)
                                    .monospace()
                                    .color(theme::txt3()),
                            )
                            .truncate(),
                        );
                    });
                    if copiar {
                        ui.ctx().copy_text(v.clone());
                    }
                }
            },
        );
        ui.add_space(GAP_SECCION);
        self.cfg_umbrales(ui, col);
    }

    /// A partir de qué número avisa el Dashboard, para ESTE equipo.
    ///
    /// POR EQUIPO Y NO GLOBAL. Un servidor de compilación al 90 % de CPU está
    /// haciendo su trabajo, y un panel que lo pinta en rojo todas las tardes
    /// enseña a no mirarlo — que es exactamente lo contrario de para lo que
    /// está. Los de fábrica son los que había siempre; esto solo permite
    /// moverlos donde estorban.
    pub(crate) fn cfg_umbrales(&mut self, ui: &mut egui::Ui, col: f32) {
        let mut u = self.umbrales;
        let mut tocado = false;
        let mut restaurar = false;
        let de_fabrica = u == lucy_core::thresholds::Umbrales::default();
        panel(
            ui,
            col,
            icons::Icon::Bolt,
            "Umbrales de este equipo",
            |ui| {
                // El botón SOLO si hay algo que devolver. Un «volver a los de
                // fábrica» siempre visible en un panel que ya está de fábrica es
                // un control que no hace nada, y esos enseñan a no leer los
                // demás.
                if !de_fabrica
                    && ui.small_button(i18n::tr("Volver a los de fábrica")).clicked()
                {
                    restaurar = true;
                }
            },
            |ui| {
                ui.add(egui::Label::new(
                    egui::RichText::new(i18n::tr(
                        "Un servidor de compilación al 90 % está trabajando. Lo que aquí se \
                         ajusta cambia el color, las alertas y el indicador de salud — solo \
                         en este equipo.",
                    ))
                    .size(theme::FS_CAPTION)
                    .color(theme::faint()),
                ));
                ui.add_space(8.0);
                // Los seis en pares: el aviso y el crítico de cada métrica
                // juntos, que es como se leen — uno dice cuándo mirar y el otro
                // cuándo actuar, y separarlos obliga a recomponerlos mentalmente.
                let campos: [(&str, &mut f32, f32, f32); 6] = [
                    ("CPU · aviso", &mut u.cpu_aviso, 1.0, 99.0),
                    ("CPU · crítico", &mut u.cpu_critico, 1.0, 100.0),
                    ("RAM · aviso", &mut u.mem_aviso, 1.0, 99.0),
                    ("RAM · crítico", &mut u.mem_critico, 1.0, 100.0),
                    ("Disco · aviso", &mut u.disco_aviso, 1.0, 99.0),
                    ("Disco · crítico", &mut u.disco_critico, 1.0, 100.0),
                ];
                let ultimo = campos.len() - 1;
                for (i, (etiqueta, valor, min, max)) in campos.into_iter().enumerate() {
                    fila(ui, etiqueta, None, i == ultimo, |ui| {
                        // De punto en punto: en un umbral, de 80 a 81 significa
                        // algo, y las décimas que permite el arrastre son para
                        // afinar, no para recorrer.
                        if numero(ui, valor, min, max, 1.0, |d| d.speed(0.5).suffix(" %")) {
                            tocado = true;
                        }
                    });
                }
            },
        );
        if restaurar {
            let _ = lucy_core::thresholds::olvida("local");
            self.umbrales = lucy_core::thresholds::Umbrales::default();
        } else if tocado {
            // SE GUARDA YA, sin botón de aplicar. Todo lo demás en esta pantalla
            // funciona así, y un panel que sí lo pidiera dejaría al operador sin
            // saber cuáles de sus cambios están puestos.
            //
            // `sane` corrige un aviso por encima del crítico en vez de
            // rechazarlo: se escribe así arrastrando el control, y negarse a
            // guardar deja un formulario que no explica qué le pasa.
            self.umbrales = u.sane();
            let _ = lucy_core::thresholds::guarda("local", &self.umbrales);
        }
    }

    /// Claves, operador, skills y la memoria: lo que se da de alta una vez.
    ///
    /// YA NO LLEVA LA FOTO DEL EQUIPO. «Este equipo» se fue a la columna
    /// izquierda: aquí había cinco paneles contra tres y quedaba medio panel
    /// negro debajo de «Animaciones». Y de paso encaja mejor allí, porque este
    /// lado es lo que se DA DE ALTA —claves, nombre, skills— y aquello era
    /// información de solo lectura sobre la máquina.
    pub(crate) fn cfg_columna_derecha(&mut self, ui: &mut egui::Ui, col: f32) {
        // ── claves de API ────────────────────────────────────────────────────
        //
        // UN SOLO PANEL, y antes eran dos: «Proveedores» listaba los mismos
        // nombres con un punto de color y mandaba a la app de escritorio a
        // ponerlas —cosa que dejó de ser verdad el día que se añadió la otra
        // sección—, y «Claves de API» los volvía a listar para escribirlas. El
        // operador leía dos veces la misma lista y una de las dos le mentía.
        let mut guardar: Option<(String, String)> = None;
        let mut borrar: Option<String> = None;
        let mut probar: Option<String> = None;
        let n_claves = lucy_core::keys::PROVIDERS
            .iter()
            .filter(|(k, _, _)| lucy_core::keys::hint(k).is_some())
            .count();
        let total = lucy_core::keys::PROVIDERS.len();
        panel(
            ui,
            col,
            icons::Icon::Shield,
            "Claves API",
            |ui| {
                let t = i18n::trf(
                    "{n_claves} de {total}",
                    &[
                        ("n_claves", &n_claves.to_string()),
                        ("total", &total.to_string()),
                    ],
                );
                insignia(ui, &t, n_claves > 0);
            },
            |ui| {
                let ultimo = total.saturating_sub(1);
                for (i, (clave, etiqueta, donde)) in lucy_core::keys::PROVIDERS.iter().enumerate() {
                    match lucy_core::keys::hint(clave) {
                        // GUARDADA se enseña con una pista de cuatro caracteres,
                        // nunca entera. Distingue la de producción de la de
                        // pruebas, que es lo único que hace falta, y no sirve
                        // para reconstruirla ni acaba en una captura.
                        Some(pista) => {
                            // UNA CLAVE GUARDADA SOLO DICE QUE EXISTE. Caducada,
                            // revocada o pegada con un espacio de más se
                            // descubre igual de bien en mitad de un incidente,
                            // que es cuando no se quiere descubrir.
                            let estado = self.claves_probadas.get(*clave).cloned();
                            let probando = self.prueba_rx.iter().any(|(p, _)| p == clave);
                            let sub = match &estado {
                                Some(lucy_core::keys::Prueba::Vale) => {
                                    format!("{pista} · el proveedor la acepta")
                                }
                                Some(lucy_core::keys::Prueba::NoVale(m)) => {
                                    format!("{pista} · {m}")
                                }
                                Some(lucy_core::keys::Prueba::NoSeSabe(m)) => i18n::trf(
                                    "{pista} · sin comprobar: {m}",
                                    &[("pista", &pista), ("m", &m)],
                                ),
                                None => pista.clone(),
                            };
                            fila(ui, etiqueta, Some(&sub), i == ultimo, |ui| {
                                if ui.small_button(i18n::tr("Quitar")).clicked() {
                                    borrar = Some(clave.to_string());
                                }
                                ui.add_space(4.0);
                                if ui
                                    .add_enabled(
                                        !probando,
                                        egui::Button::new(i18n::tr(if probando {
                                            "Probando…"
                                        } else {
                                            "Probar"
                                        }))
                                        .small(),
                                    )
                                    .on_hover_text(i18n::tr("Pide el catálogo de modelos — no gasta"))
                                    .clicked()
                                {
                                    probar = Some(clave.to_string());
                                }
                                ui.add_space(4.0);
                                // TRES ESTADOS Y NO DOS: «la rechaza» y «no he
                                // podido comprobarlo» llevan a sitios distintos.
                                match &estado {
                                    Some(lucy_core::keys::Prueba::Vale) => {
                                        insignia(ui, "válida", true)
                                    }
                                    Some(lucy_core::keys::Prueba::NoVale(_)) => {
                                        insignia(ui, "no vale", false)
                                    }
                                    Some(lucy_core::keys::Prueba::NoSeSabe(_)) => {
                                        insignia(ui, "sin saber", false)
                                    }
                                    None => insignia(ui, "configurada", true),
                                }
                            });
                        }
                        None => {
                            let buf = self.api_keys.entry(clave.to_string()).or_default();
                            let mut pedir = false;
                            fila(ui, etiqueta, Some(donde), i == ultimo, |ui| {
                                let te = ui.add(
                                    egui::TextEdit::singleline(buf)
                                        .password(true)
                                        .desired_width(150.0)
                                        .hint_text(i18n::tr("pegar clave")),
                                );
                                let intro = te.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                let pulsado = ui
                                    .add_enabled(
                                        !buf.trim().is_empty(),
                                        egui::Button::new(i18n::tr("Guardar")).small(),
                                    )
                                    .clicked();
                                pedir = (intro || pulsado) && !buf.trim().is_empty();
                            });
                            if pedir {
                                guardar = Some((clave.to_string(), buf.trim().to_string()));
                            }
                        }
                    }
                }
                ui.add_space(8.0);
                ui.label(
                    // La traducción de esta frase ya estaba escrita en los cinco
                    // idiomas; lo que faltaba era el `tr` de aquí.
                    egui::RichText::new(i18n::tr(
                        "Se guardan en el Credential Manager de Windows, en el mismo sitio del \
                         que las lee la app de escritorio. Ollama no necesita clave: es local.",
                    ))
                    .size(theme::FS_CAPTION)
                    .color(theme::faint()),
                );
            },
        );
        if let Some((p, k)) = guardar {
            self.api_key_msg = match lucy_core::keys::set(&p, &k) {
                // El campo se vacía al guardar: dejar la clave escrita en un
                // cuadro de texto es dejarla en memoria y en pantalla para nada.
                Ok(()) => {
                    self.api_keys.remove(&p);
                    olvidar_claves();
                    String::new()
                }
                Err(e) => e,
            };
        }
        if let Some(p) = borrar {
            self.api_key_msg = lucy_core::keys::delete(&p).err().unwrap_or_default();
            self.claves_probadas.remove(&p);
            olvidar_claves();
        }
        if let Some(p) = probar {
            self.prueba_clave(&p);
        }
        if !self.api_key_msg.is_empty() {
            ui.label(
                egui::RichText::new(&self.api_key_msg)
                    .size(theme::FS_CAPTION)
                    .color(theme::red()),
            );
        }

        // ── operador y lo que Lucy sabe de él ────────────────────────────────
        //
        // JUNTOS PORQUE SON LO MISMO: quién eres, y lo que Lucy ha ido apuntando
        // sobre ti. Y esa lista es la mitad de confianza de la función: lo que
        // hay ahí lo escribió un modelo, sin que nadie lo aprobara, y viaja en
        // todos los prompts a partir de entonces. Un almacén así sin forma de
        // verlo ni de vaciarlo no es una memoria: es algo que se te queda
        // pegado.
        ui.add_space(GAP_SECCION);
        let perfil = lucy_core::profile::all().unwrap_or_default();
        let mut olvidar: Option<String> = None;
        let n_perfil = perfil.len();
        panel(
            ui,
            col,
            icons::Icon::Desktop,
            "Operador",
            |ui| {
                if n_perfil > 0 {
                    let t = format!("{n_perfil} datos");
                    insignia(ui, &t, true);
                }
            },
            |ui| {
                let mut n = user_name();
                fila(
                    ui,
                    "Tu nombre",
                    Some(
                        "si se deja vacío usa el usuario de Windows, que es una cuenta y no un \
                         nombre",
                    ),
                    perfil.is_empty(),
                    |ui| {
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut n)
                                    .hint_text(i18n::tr("Tu nombre"))
                                    .desired_width(160.0),
                            )
                            .changed()
                        {
                            set_user_name(&n);
                        }
                    },
                );
                if perfil.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(
                            "Lucy todavía no ha apuntado nada sobre ti. Lo hace sola cuando le \
                             cuentas algo que le servirá otro día.",
                        )
                        .size(theme::FS_CAPTION)
                        .color(theme::faint()),
                    );
                }
                let ultimo = n_perfil.saturating_sub(1);
                for (i, e) in perfil.iter().enumerate() {
                    let etiqueta = e.key.replace('_', " ");
                    fila(ui, &etiqueta, Some(&e.value), i == ultimo, |ui| {
                        if ui
                            .small_button("×")
                            .on_hover_text(i18n::tr("Que Lucy lo olvide"))
                            .clicked()
                        {
                            olvidar = Some(e.key.clone());
                        }
                    });
                }
            },
        );
        if let Some(k) = olvidar {
            let _ = lucy_core::profile::forget(&k);
        }

        // ── skills ───────────────────────────────────────────────────────────
        ui.add_space(GAP_SECCION);
        let mut instalar = false;
        let mut quitar: Option<String> = None;
        // APAGAR NO ES BORRAR. Lo que se quiere casi siempre es «ahora no» —el
        // de migración estorba mientras se atiende una incidencia— y para eso
        // desinstalar es demasiado: hay que volver a encontrar la carpeta.
        // NUNCA SE ASIGNABA. Estaba declarado a `None` fijo, así que el `if let`
        // de más abajo —el que llama a `set_enabled`— era código muerto y la
        // única acción de la fila era la «×» de desinstalar. El motor estaba
        // entero: `Skill.activo`, `skills::set_enabled`, el recuento de activos y
        // hasta el texto que explica qué pasa con los apagados. Lo que faltaba
        // era el interruptor.
        let mut alternar: Option<(lucy_core::skills::Skill, bool)> = None;
        let n_skills = self.skills.len();
        let activos = self.skills.iter().filter(|k| k.activo).count();
        panel(
            ui,
            col,
            icons::Icon::Bolt,
            "Skills",
            |ui| {
                if ui
                    .small_button(i18n::tr("Instalar…"))
                    .on_hover_text(
                        "Elige la carpeta de un skill, o una que contenga varios — un \
                         repositorio descargado sirve tal cual",
                    )
                    .clicked()
                {
                    instalar = true;
                }
            },
            |ui| {
                if self.skills.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "Ninguno. Un skill es una carpeta con un SKILL.md dentro; Lucy los \
                             ve y pide el que encaje.",
                        )
                        .size(theme::FS_CAPTION)
                        .color(theme::faint()),
                    );
                }
                let ultimo = n_skills.saturating_sub(1);
                for (i, k) in self.skills.iter().enumerate() {
                    fila(ui, &k.name, Some(&k.description), i == ultimo, |ui| {
                        // DESINSTALAR PRIMERO PORQUE LA FILA REPARTE DE DERECHA A
                        // IZQUIERDA: lo que se añade antes queda más a la
                        // derecha. La «×» va al borde y el interruptor a su
                        // izquierda, que es el orden de las filas de claves.
                        if ui
                            .small_button("×")
                            .on_hover_text(i18n::tr("Desinstalar: borra la carpeta del skill"))
                            .clicked()
                        {
                            quitar = Some(k.name.clone());
                        }
                        ui.add_space(6.0);
                        // APAGAR NO ES BORRAR, y por eso son dos controles y no
                        // uno. Casi siempre lo que se quiere es «ahora no» —el de
                        // migración estorba mientras atiendes una incidencia— y
                        // desinstalar para eso obliga a volver a encontrar la
                        // carpeta dentro de una semana.
                        if let Some(j) = segmentado(
                            ui,
                            // El nombre del skill EN LA CLAVE: si no, los cinco
                            // interruptores comparten estado de animación y las
                            // píldoras se persiguen entre sí.
                            &format!("skill-{}", k.name),
                            150.0,
                            &["Activo", "Apagado"],
                            usize::from(!k.activo),
                        ) {
                            let on = j == 0;
                            if on != k.activo {
                                alternar = Some((k.clone(), on));
                            }
                        }
                    });
                }
                if !self.skills.is_empty() {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{activos} de {n_skills} activos. Los apagados siguen en disco y no \
                             entran en lo que Lucy ve, así que deja de pedirlos. Se instalan en \
                             tu perfil y sobreviven a reinstalar Lucy."
                        ))
                        .size(theme::FS_CAPTION)
                        .color(theme::faint()),
                    );
                }
                if !self.skills_msg.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(&self.skills_msg)
                            .size(theme::FS_CAPTION)
                            .color(theme::txt3()),
                    );
                }
            },
        );
        if instalar {
            // Bloqueante a propósito: es el diálogo del sistema, y mientras está
            // abierto no hay nada que animar detrás.
            if let Some(dir) = rfd::FileDialog::new()
                .set_title(i18n::tr("Carpeta del skill (o una que contenga varios)"))
                .pick_folder()
            {
                let destino = lucy_core::skills::user_dir();
                self.skills_msg = match destino {
                    Some(d) => match lucy_core::skills::install(&dir, &d) {
                        Ok(v) => {
                            self.skills = cargar_skills();
                            format!("Instalados: {}", v.join(", "))
                        }
                        Err(e) => e,
                    },
                    None => i18n::tr("No se pudo resolver tu perfil de usuario.").into(),
                };
            }
        }
        if let Some((k, on)) = alternar {
            self.skills_msg = match lucy_core::skills::set_enabled(&k, on) {
                Ok(()) => {
                    self.skills = cargar_skills();
                    // Un modo fijado que acaba de apagarse dejaría el prompt
                    // pidiendo un procedimiento que Lucy ya no ve.
                    if !on && self.preset.as_deref() == Some(k.name.as_str()) {
                        self.preset = None;
                    }
                    String::new()
                }
                Err(e) => e,
            };
        }
        if let Some(n) = quitar {
            self.skills_msg = match lucy_core::skills::uninstall(&n) {
                Ok(()) => {
                    self.skills = cargar_skills();
                    // Un modo fijado que ya no existe dejaría el prompt pidiendo
                    // un procedimiento ausente en cada turno.
                    if self.preset.as_deref() == Some(n.as_str()) {
                        self.preset = None;
                    }
                    format!("«{n}» quitado.")
                }
                Err(e) => e,
            };
        }

        // ── la base ──────────────────────────────────────────────────────────
        //
        // TODA LA MEMORIA DE LUCY VIVE EN UN FICHERO, y hasta ahora esta vista
        // enseñaba su ruta y nada más. Enseñar dónde está algo irreemplazable
        // sin ofrecer copiarlo es dar media instrucción.
        ui.add_space(GAP);
        if self.recuento.is_none() {
            if let Some(p) = db_path() {
                self.recuento = Some(lucy_core::upkeep::recuento(&p));
                self.sin_vector = lucy_core::upkeep::sin_vector();
            }
        }
        let r = self.recuento.clone().unwrap_or_default();
        let armada = self.purga_armada;
        let reembebiendo = self.reembeber_rx.is_some();
        let sin_vec = self.sin_vector;
        let mut copiar = false;
        let mut recargar = false;
        let mut reembeber = false;
        let mut purgar: Option<lucy_core::upkeep::Purga> = None;
        panel(
            ui,
            col,
            icons::Icon::Database,
            "La memoria en disco",
            |ui| {
                let mb = r.bytes as f64 / 1_048_576.0;
                insignia(ui, &format!("{mb:.1} MB"), true);
            },
            |ui| {
                // El recuento por tipo es lo que convierte «ocupa 7 MB» en
                // «casi todo es un PDF que ingeriste en abril» — que es una
                // frase sobre la que se puede decidir algo.
                for (etiqueta, n, explica) in [
                    ("Memorias", r.memorias, "hechos que Lucy recuerda"),
                    ("· de ellas, automáticas", r.automaticas, "escritas al cerrar un turno"),
                    ("· fijadas", r.fijadas, "entran en todos los prompts"),
                    ("Cristales", r.cristales, "sesiones destiladas"),
                    ("Patrones", r.patrones, "lo que se repite entre memorias"),
                    ("Trozos de documento", r.trozos, "de los manuales ingeridos"),
                    ("Retiradas", r.retiradas, "fundidas por la consolidación; ya no se leen"),
                ] {
                    fila(ui, etiqueta, Some(explica), false, |ui| {
                        ui.label(
                            egui::RichText::new(n.to_string())
                                .size(theme::FS_FOOTNOTE)
                                .monospace()
                                .color(if n == 0 { theme::faint() } else { theme::txt2() }),
                        );
                    });
                }
                // Trozos sin vector: la vista de Documentos ya lo decía y no
                // ofrecía arreglarlo. Solo aparece cuando los hay.
                if sin_vec > 0 {
                    fila(
                        ui,
                        "Trozos sin vector",
                        Some("solo se encuentran por palabras — pasó si Ollama estaba caído al ingerir"),
                        false,
                        |ui| {
                            if ui
                                .add_enabled(
                                    !reembebiendo,
                                    egui::Button::new(i18n::tr(if reembebiendo {
                                        "Rehaciendo…"
                                    } else {
                                        "Rehacer"
                                    }))
                                    .small(),
                                )
                                .clicked()
                            {
                                reembeber = true;
                            }
                            ui.add_space(6.0);
                            insignia(ui, &sin_vec.to_string(), false);
                        },
                    );
                }
                fila(ui, "Copia de seguridad", Some("consistente, aunque Lucy esté escribiendo"), false, |ui| {
                    copiar = ui.small_button(i18n::tr("Guardar copia…")).clicked();
                });
                fila(ui, "", Some("vuelve a contar lo de arriba"), true, |ui| {
                    recargar = ui.small_button(i18n::tr("↻ Recontar")).clicked();
                });
                // ── purgas ───────────────────────────────────────────────────
                //
                // EN DOS TIEMPOS Y DICIENDO QUÉ SE PIERDE. «Se borrarán 14
                // filas» no permite decidir; lo que hace falta saber es qué deja
                // de poder hacerse después.
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(i18n::tr("Quitar en lote"))
                        .size(theme::FS_CAPTION)
                        .color(theme::faint()),
                );
                ui.add_space(4.0);
                for (p, nombre, n) in [
                    (lucy_core::upkeep::Purga::Retiradas, "Retiradas", r.retiradas),
                    (lucy_core::upkeep::Purga::Automaticas, "Memorias automáticas", r.automaticas),
                    (lucy_core::upkeep::Purga::Documentos, "Documentos ingeridos", r.documentos),
                ] {
                    let lista = armada == Some(p);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                n > 0,
                                egui::Button::new(
                                    egui::RichText::new(if lista {
                                        i18n::trf(
                                            "¿Borrar {nombre}?",
                                            &[("nombre", i18n::tr(nombre))],
                                        )
                                    } else {
                                        i18n::tr(nombre).to_string()
                                    })
                                    .size(theme::FS_CAPTION)
                                    .color(if lista { theme::red() } else { theme::txt3() }),
                                )
                                .small(),
                            )
                            .on_hover_text(p.describe(&r))
                            .clicked()
                        {
                            purgar = Some(p);
                        }
                        ui.label(
                            egui::RichText::new(format!("{n}"))
                                .size(theme::FS_MICRO)
                                .color(theme::faint()),
                        );
                    });
                    if lista {
                        ui.label(
                            egui::RichText::new(p.describe(&r))
                                .size(theme::FS_MICRO)
                                .color(theme::amber()),
                        );
                    }
                }
                if !self.upkeep_msg.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(&self.upkeep_msg)
                            .size(theme::FS_CAPTION)
                            .color(theme::txt3()),
                    );
                }
            },
        );
        if recargar {
            self.recuento = None;
        }
        if reembeber {
            let (tx, rx) = std::sync::mpsc::channel();
            let stop = self.mant_stop.clone();
            std::thread::spawn(move || {
                let _ = tx.send(lucy_core::upkeep::reembeber(&stop));
            });
            self.reembeber_rx = Some(rx);
            self.upkeep_msg = i18n::tr("Rehaciendo los vectores que faltaban…").into();
        }
        if copiar {
            // El diálogo del sistema es modal: bloquea mientras está abierto, y
            // lo abre un clic — no pasa solo.
            if let Some(d) = rfd::FileDialog::new()
                .set_title(i18n::tr("Dónde guardar la copia"))
                .set_file_name(format!("lucy-{}.db", ahora_epoch()))
                .add_filter(i18n::tr("Base de datos"), &["db"])
                .save_file()
            {
                self.upkeep_msg = match lucy_core::upkeep::backup(&d) {
                    Ok(b) => format!(
                        "Copia guardada: {:.1} MB en {}",
                        b as f64 / 1_048_576.0,
                        d.display()
                    ),
                    Err(e) => e,
                };
            }
        }
        if let Some(p) = purgar {
            if armada == Some(p) {
                self.purga_armada = None;
                self.upkeep_msg = match lucy_core::upkeep::purga(p) {
                    Ok(n) => format!("{n} filas quitadas."),
                    Err(e) => e,
                };
                self.recuento = None;
                self.sin_vector = lucy_core::upkeep::sin_vector();
                self.mems = load_memories();
            } else {
                self.purga_armada = Some(p);
            }
        }

    }
}
