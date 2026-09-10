//! Recogedores: lo que llega de los hilos de fondo, un canal por asunto.
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
    /// Recoge las miniaturas y las sube a la GPU.
    ///
    /// LA TEXTURA SE CREA AQUI Y NO EN EL HILO: `TextureHandle` necesita el
    /// contexto de egui, que no cruza a otro hilo. El hilo hace lo caro
    /// —decodificar y reducir— y esto hace lo que solo se puede hacer aqui.
    pub(crate) fn pump_minis(&mut self, ctx: &egui::Context) {
        let mut llegadas: Vec<(String, Option<lucy_core::attach::Miniatura>)> = Vec::new();
        self.mini_rx.retain(|(ruta, rx)| match rx.try_recv() {
            Ok(m) => {
                llegadas.push((ruta.clone(), m));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                llegadas.push((ruta.clone(), None));
                false
            }
        });
        for (ruta, m) in llegadas {
            let tex = m.map(|mini| {
                let img = egui::ColorImage::from_rgba_unmultiplied(
                    [mini.ancho as usize, mini.alto as usize],
                    &mini.rgba,
                );
                ctx.load_texture(format!("mini:{ruta}"), img, egui::TextureOptions::LINEAR)
            });
            // SE GUARDA TAMBIEN EL FALLO. Sin esto, un fichero que no decodifica
            // se reintentaria en cada fotograma: un hilo por frame para volver a
            // fallar.
            self.minis.insert(ruta, tex);
        }
    }

    /// Comprueba que el «restaurar» hizo algo, y si no, le da un tamaño.
    pub(crate) fn pump_restaurar(&mut self, ctx: &egui::Context) {
        let Some(desde) = self.restaurando else { return };
        if desde.elapsed().as_millis() < Self::ESPERA_RESTAURAR_MS {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
            return;
        }
        self.restaurando = None;
        // Si volvió a maximizarse por su cuenta no hay nada que arreglar: el
        // operador pulsó otra vez.
        if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
            return;
        }
        let (v, pantalla) = ctx.input(|i| {
            (
                i.screen_rect().size(),
                i.viewport().monitor_size.unwrap_or(egui::vec2(1920.0, 1080.0)),
            )
        });
        if v.x >= pantalla.x * Self::RESTAURAR_FALLIDO
            && v.y >= pantalla.y * Self::RESTAURAR_FALLIDO
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                VENTANA[0], VENTANA[1],
            )));
            // Y AL CENTRO. Sin esto, la ventana encoge dejando la esquina
            // superior izquierda donde estaba, que en un monitor grande la deja
            // arrinconada arriba a la izquierda — se ve como otro fallo.
            let x = ((pantalla.x - VENTANA[0]) * 0.5).max(0.0);
            let y = ((pantalla.y - VENTANA[1]) * 0.5).max(0.0);
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        }
    }

    /// Recoge las pruebas de clave y el reembebido.
    pub(crate) fn pump_upkeep(&mut self) {
        let mut llegadas: Vec<(String, lucy_core::keys::Prueba)> = Vec::new();
        self.prueba_rx.retain(|(p, rx)| match rx.try_recv() {
            Ok(r) => {
                llegadas.push((p.clone(), r));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        for (p, r) in llegadas {
            self.claves_probadas.insert(p, r);
        }
        if let Some(rx) = &self.reembeber_rx {
            match rx.try_recv() {
                Ok(r) => {
                    self.reembeber_rx = None;
                    self.upkeep_msg = match r {
                        Ok(0) => i18n::tr("No había nada sin vector.").into(),
                        Ok(n) => i18n::trf(
                            "{n} filas vuelven a ser buscables por significado.",
                            &[("n", &n.to_string())],
                        ),
                        Err(e) => e,
                    };
                    // LAS DOS CUENTAS, no la de la clase que se acaba de
                    // rehacer. Son dos consultas baratas que corren una vez al
                    // terminar un trabajo manual, y refrescar solo una deja la
                    // otra fila con una cifra vieja en pantalla — que es peor
                    // que la consulta que ahorra.
                    self.sin_vector = lucy_core::upkeep::sin_vector(upkeep::Clase::Trozo);
                    self.sin_vector_mem = lucy_core::upkeep::sin_vector(upkeep::Clase::Memoria);
                    self.recuento = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.reembeber_rx = None,
            }
        }
    }

    /// Recoge los atajos que estaban en vuelo.
    pub(crate) fn pump_chips(&mut self) {
        let Some(rx) = &self.chips_rx else { return };
        match rx.try_recv() {
            Ok(r) => {
                self.chips_rx = None;
                // Un fallo se traga: los de fábrica siguen puestos y la pantalla
                // queda como estaba. Avisar de que no se ha podido embellecer
                // una pantalla vacía sería ruido sobre algo que nadie pidió.
                //
                // EL LISTÓN DE TRES VIVE AHORA EN `suggest::MIN_UTILES`, junto a
                // quien lo usa para decidir si prueba con otro modelo. Estaba
                // aquí como un `3` suelto, y el núcleo no podía saber cuándo
                // había salido bien.
                if let Some((chips, ent, sal, modelo)) = r {
                    self.chips = chips;
                    if let Some(c) = lucy_core::pricing::cost(&modelo, ent, sal) {
                        self.gasto_titulos += c;
                    }
                    // Las sugerencias de la pantalla de inicio también cuestan, y
                    // van en su propio cubo: nadie las pide, salen solas, y por
                    // eso son de lo primero que hay que poder mirar cuando la
                    // factura no cuadra con lo que uno cree haber usado.
                    let _ = lucy_core::usage::apunta(
                        &modelo,
                        ent,
                        sal,
                        lucy_core::usage::Para::Chips,
                        "",
                    );
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.chips_rx = None,
        }
    }

    /// Recoge los nombres de pestaña que estaban en vuelo.
    pub(crate) fn pump_titulos(&mut self) {
        let mut llegados: Vec<(usize, String, Titulado)> = Vec::new();
        self.titulo_rx.retain(|(uid, modelo, rx)| match rx.try_recv() {
            Ok(r) => {
                llegados.push((*uid, modelo.clone(), r));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        for (uid, modelo, r) in llegados {
            // Un error se traga en silencio: el recorte sigue puesto y es un
            // nombre válido. Lo único que se pierde es la mejora.
            let Ok((titulo, ent, sal)) = r else { continue };
            if titulo.trim().is_empty() {
                continue;
            }
            // La pestaña pudo cerrarse mientras el título viajaba.
            let Some(t) = self.tabs.iter_mut().find(|t| t.uid == uid) else { continue };
            t.title = titulo;
            // Los tokens de un modelo local son cero y `cost` de un id que no
            // está tarifado devuelve `None`: en los dos casos no se suma nada,
            // que es lo correcto.
            if let Some(c) = lucy_core::pricing::cost(&modelo, ent, sal) {
                self.gasto_titulos += c;
            }
            // Y AL DISCO, COMO «titulo». Es el gasto que más fácil se le escapa
            // a cualquiera —nadie pide un título, salen solos— y por eso vale la
            // pena poder separarlo: un mes en el que los títulos se llevan el
            // 40 % no se arregla hablando menos con Lucy, se arregla titulando
            // con el modelo local.
            let _ = lucy_core::usage::apunta(
                &modelo,
                ent,
                sal,
                lucy_core::usage::Para::Titulo,
                &uid.to_string(),
            );
        }
    }

    /// Recoge las búsquedas de `/recall` y las escribe en su conversación.
    pub(crate) fn pump_recall(&mut self) {
        let mut llegados: Vec<(usize, (String, lucy_core::memories::Recuerdo))> = Vec::new();
        self.recall_rx.retain(|(uid, rx)| match rx.try_recv() {
            Ok(r) => {
                llegados.push((*uid, r));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        for (uid, (consulta, r)) in llegados {
            let Some(ti) = self.tabs.iter().position(|t| t.uid == uid) else { continue };
            let texto = if r.is_empty() {
                format!(
                    "Nada parecido a «{consulta}».\n\nSe ha buscado por significado y también \
                     por palabras, así que esto no es que falte Ollama: es que no hay nada \
                     guardado que se parezca."
                )
            } else {
                // POR QUÉ CAMINO LLEGÓ. Con el respaldo léxico, Lucy encuentra
                // menos y peor; quien esté mirando por qué no se acordó de algo
                // evidente necesita saber que estaba trabajando con una mano
                // atada.
                let como = i18n::tr(if r.lexico {
                    "  (por palabras — el embebedor no contestó, así que esto encuentra menos)"
                } else if r.documentos > 0 {
                    "  (por significado, memorias y documentos)"
                } else {
                    "  (por significado)"
                });
                i18n::trf(
                    "Esto es lo que recordaría con «{consulta}»:{como}\n\n{bloque}",
                    &[("consulta", &consulta), ("como", como), ("bloque", &r.bloque)],
                )
            };
            self.tabs[ti].log.push(ChatMsg::new(false, texto));
        }
    }

    /// Recoge la revisión de duplicados. El resultado va a los DOS sitios que la
    /// piden —la vista de Memoria y el hilo de chat— porque el operador puede
    /// haberla lanzado desde cualquiera y estar mirando el otro.
    pub(crate) fn pump_dedup(&mut self) {
        let Some(rx) = &self.dedup_rx else { return };
        let r = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.dedup_rx = None;
                return;
            }
        };
        self.dedup_rx = None;
        let m = match &r {
            Err(e) => i18n::trf("No se pudo revisar: {e}", &[("e", &e)]),
            Ok(rep) if !rep.dry_run => {
                i18n::trf(
                    "Fundidas {memorias} memorias en {grupos} grupos.",
                    &[
                        ("memorias", &rep.memories_merged.to_string()),
                        ("grupos", &rep.clusters_found.to_string()),
                    ],
                )
            }
            Ok(rep) if rep.clusters_found == 0 => {
                i18n::trf(
                    "Ninguna repetida entre las {n} más recientes.",
                    &[("n", &rep.scanned.to_string())],
                )
            }
            Ok(rep) => {
                let mut s = format!(
                    "**{} grupos · {} memorias** se fundirían, de {} miradas. No se ha \
                     tocado nada.\n\n",
                    rep.clusters_found, rep.memories_merged, rep.scanned
                );
                for c in rep.clusters.iter().take(10) {
                    s.push_str(&format!(
                        "- «{}» absorbe {} — parecido {:.0} %\n",
                        c.canonical_title,
                        c.merged_ids.len(),
                        c.overlap_score * 100.0
                    ));
                }
                s.push_str("\nPara aplicarlo: Memoria → Fundir.");
                s
            }
        };
        // Tras fundir de verdad, la lista de la vista se relee.
        if matches!(&r, Ok(rep) if !rep.dry_run) {
            self.mems = load_memories();
        }
        self.dedup = Some(r);
        self.di(&m);
    }

    /// Recoge los sub-agentes que hayan terminado y suelta las esperas cumplidas.
    ///
    /// En CADA frame y sobre TODAS las pestañas, como el resto de los `pump`: la
    /// pestaña que espera puede no ser la que se está mirando, y un sub-agente
    /// que termina en una terminal de fondo tiene que cerrar su turno igual.
    pub(crate) fn pump_forks(&mut self) {
        use lucy_core::agent::ForkStatus;
        use std::sync::mpsc::TryRecvError;

        for i in 0..self.tabs.len() {
            let mut llegados = Vec::new();
            self.tabs[i].fork_rx.retain(|(id, rx)| match rx.try_recv() {
                Ok(r) => {
                    llegados.push(r);
                    false
                }
                Err(TryRecvError::Empty) => true,
                // El hilo murió sin mandar nada — un pánico dentro de la tarea.
                // Se cierra como error en vez de dejar el canal puesto: si no,
                // el `wait_task` que la espera no terminaría nunca y la pestaña
                // se quedaría ocupada para siempre.
                Err(TryRecvError::Disconnected) => {
                    llegados.push(lucy_core::forks::ForkResult {
                        id: id.clone(),
                        text: "La tarea se cortó sin devolver nada.".into(),
                        ok: false,
                        ms: 0,
                        // Lo que gastara antes de morir no se sabe: el gasto
                        // viaja con el resultado, y aquí no hubo resultado.
                        tokens_in: 0,
                        tokens_out: 0,
                    });
                    false
                }
            });
            for r in llegados {
                let estado = if r.ok { ForkStatus::Done } else { ForkStatus::Error };
                self.tabs[i].ws.fork_finish(&r.id, estado, &r.text);
                // LO QUE COBRÓ LA TAREA VA AL CONTADOR DE LA PESTAÑA. El brazo
                // de `Usage` del sub-agente era un `_ => {}`, así que con cinco
                // tareas de hasta cuatro peticiones cada una el coste que se
                // enseña podía ir veinte llamadas por detrás de la factura.
                self.tabs[i].tokens_in += r.tokens_in;
                self.tabs[i].tokens_out += r.tokens_out;
                // Y SU COSTE, cobrado con el modelo que corrio la tarea. Ver
                // `Tab::coste`: sumar solo los tokens dejaba el gasto del
                // sub-agente a merced de lo que hubiera en el selector despues.
                self.tabs[i].coste = suma_coste(
                    self.tabs[i].coste,
                    lucy_core::pricing::cost(&self.chat_model, r.tokens_in, r.tokens_out),
                );
                // APUNTADO APARTE COMO «fork». El coste de un sub-agente es la
                // parte de la factura que nadie ve pasar, y sumarlo al del chat
                // en el mismo cubo dejaría sin contestar «¿me salen a cuenta los
                // sub-agentes?».
                let _ = lucy_core::usage::apunta(
                    &self.chat_model,
                    r.tokens_in,
                    r.tokens_out,
                    lucy_core::usage::Para::Fork,
                    &self.tabs[i].uid.to_string(),
                );
                self.tabs[i].ws.trace_push(lucy_core::agent::TraceEntry {
                    phase: if r.ok { "obs" } else { "error" }.into(),
                    label: format!("Sub-agente {}: {}", r.id, if r.ok { "hecho" } else { "error" }),
                    detail: r.text.clone(),
                    ..Default::default()
                });
            }
        }

        for i in 0..self.tabs.len() {
            let listo = match &self.tabs[i].espera {
                Some(e) => e.ids.iter().all(|id| {
                    self.tabs[i]
                        .ws
                        .forks
                        .iter()
                        .find(|f| f.id == *id)
                        .is_none_or(|f| f.status != ForkStatus::Running)
                }),
                None => false,
            };
            if !listo {
                continue;
            }
            // Se saca ANTES de mandar: `send_raw` mira `busy()`, y `busy()`
            // ahora incluye esta espera. Dejarla puesta encolaría el lote contra
            // sí mismo y no saldría nunca.
            let Some(e) = self.tabs[i].espera.take() else { continue };
            let mut res = e.resultados;
            for id in &e.ids {
                res.push(self.fork_cobrar(i, id));
            }
            self.mandar_resultados(i, res);
        }
    }

    /// Recoge el resultado de un comando aprobado.
    pub(crate) fn pump_exec(&mut self) {
        use lucy_core::agent::{ExecEntry, StepStatus};
        // El resultado vuelve a LA PESTAÑA que lo pidió, no a la que esté
        // delante. Con dos órdenes en marcha, mandarlo a la activa imprimía la
        // salida de una en la conversación de la otra — que es lo que pasaba.
        let Some((uid, id, rx)) = &self.exec_rx else { return };
        let (uid, id) = (*uid, id.clone());
        let (out, err, ok, ms) = match rx.try_recv() {
            Ok(v) => v,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if let Some(t) = self.tabs.iter_mut().find(|t| t.uid == uid) {
                    t.ws.plan_update(&id, StepStatus::Error, None);
                }
                self.exec_rx = None;
                // Éste y el de abajo son los dos únicos finales que liberan el
                // carril SIN abrir un turno, así que son los que de verdad
                // dejaban colgadas a las demás pestañas: por el camino normal, el
                // `send_raw` del final ya provoca un cierre que las reintenta.
                self.reintentar_auto(uid);
                return;
            }
        };
        // La pestaña puede haberse cerrado mientras el comando corría. No es un
        // error: se ejecutó igual, y no hay a quién contárselo.
        let Some(ti) = self.tabs.iter().position(|t| t.uid == uid) else {
            self.exec_rx = None;
            self.reintentar_auto(uid);
            return;
        };
        let cmd = self.tabs[ti]
            .ws
            .plan
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.detail.clone())
            .unwrap_or_default();

        // La salida y el error van JUNTOS. PowerShell escribe avisos por stderr
        // en comandos que funcionan, y separarlos hace que un comando correcto
        // parezca fallido y uno fallido parezca vacío.
        let mut body = out.trim().to_string();
        if !err.trim().is_empty() {
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            body.push_str(&format!("[stderr]\n{}", err.trim()));
        }
        if body.is_empty() {
            body = i18n::tr("(sin salida)").into();
        }

        // EL ESCANEO VA ANTES DE TOCAR EL HILO, y ahí estaba el fallo. Estaba
        // ochenta líneas más abajo, después del `log.push`: cuando decidía
        // retener, el volcado YA vivía en `t.log` como `Role::Exec`, y
        // `history()` interpola ese campo literalmente en cada petición. O sea
        // que el `send_raw` que dice "no la vas a ver" viajaba con el log
        // envenenado entero dentro del mismo cuerpo — y se reenviaba en cada
        // turno posterior de la pestaña, y en cada sesión restaurada de disco.
        //
        // El comentario de abajo afirmaba lo contrario de lo que hacía el código,
        // que es la peor clase de guardrail: uno que hace tomar decisiones —
        // "sigo usando esta pestaña"— sobre un aviso falso.
        let g = lucy_core::guard::scan(&body, lucy_core::guard::Role::Tool);
        let retenido = g.decision == lucy_core::guard::Decision::Block;

        // Y QUE VOLVIÓ, con qué y cuánto tardó. Es la línea que cierra la de
        // «Comando lanzado»: sin ella, el carril no distingue un comando que
        // sigue corriendo de uno cuyo resultado se perdió por el camino.
        //
        // El TAMAÑO de la salida y no la salida: el volcado crudo es del carril
        // de Ejecución, y meterlo aquí también sería tenerlo dos veces. Lo que
        // este carril contesta es «pasó algo y cuánto», no «qué dijo».
        self.tabs[ti].ws.trace_push(lucy_core::agent::TraceEntry {
            phase: if ok { "react".into() } else { "error".into() },
            label: if ok {
                i18n::tr("Comando terminado").into()
            } else {
                i18n::tr("Comando fallido").into()
            },
            detail: i18n::trf(
                "{ms} ms · {n} caracteres de salida",
                &[("ms", &ms.to_string()), ("n", &body.chars().count().to_string())],
            ),
            ..Default::default()
        });
        if retenido {
            // EL GUARDRAIL, DICHO. Retener la salida y no anotarlo deja al
            // operador viendo cómo Lucy contesta sobre un comando cuyo volcado
            // no le llegó, sin nada que lo explique.
            self.tabs[ti].ws.trace_push(lucy_core::agent::TraceEntry {
                phase: "error".into(),
                label: i18n::tr("Salida retenida por el guardrail").into(),
                detail: g.reason.clone(),
                ..Default::default()
            });
        }

        // DÓNDE CORRIÓ, no solo con qué. `engine` ponía "PS" a secas también para
        // los pasos remotos, así que el carril que sirve para auditar no permitía
        // saber en qué máquina pasó nada. Se resuelve antes del `exec_push`
        // porque dentro el workspace ya está prestado.
        let destino = self
            .tabs[ti]
            .ws
            .plan
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.host.clone())
            .unwrap_or_default();
        let motor =
            if destino.is_empty() { "PS".to_string() } else { format!("PS · {destino}") };
        // QUIÉN LO DECIDIÓ, no solo que lo propuso Lucy. Aquí se escribía `"ai"`
        // en las dos ramas —con un comentario que ya decía que con el automático
        // encendido nadie lo había aprobado— así que el registro no distinguía
        // un comando sancionado por una persona de uno que corrió solo. El dato
        // se conoce en `run_step` y viaja en `exec_origen`.
        self.auditar(Some(ti), &cmd, &destino, self.exec_origen, ok, ms, &body);
        // El carril de Ejecución SÍ lleva el volcado crudo: es del operador, y
        // no viaja en ningún prompt. Es el único sitio donde debe vivir.
        self.tabs[ti].ws.exec_push(ExecEntry {
            id: String::new(),
            cmd: cmd.clone(),
            output: body.clone(),
            ok,
            ms: Some(ms),
            engine: motor,
            code: None,
            ts: 0,
        });
        self.tabs[ti].ws.plan_update(
            &id,
            if ok { StepStatus::Done } else { StepStatus::Error },
            Some(ms),
        );
        self.exec_rx = None;
        // Acaba de escribirse una fila de auditoría, así que el recuento de
        // fallos cacheado puede estar viejo. Vaciarlo entero es más barato que
        // afinar la clave y es el único momento en que hace falta.
        self.fallos.clear();

        // La línea del comando entra en el hilo como EVENTO, plegada: el
        // comando, si fue bien, y su salida dentro. Con el motivo en lugar del
        // cuerpo cuando se retuvo, porque este campo es el que acaba en el prompt.
        // AL HILO VA LA VERSIÓN LIMPIA, y esta distinción es la corrección. El
        // carril de Ejecución de arriba se queda con el volcado CRUDO —es del
        // operador y no viaja a ninguna parte—, pero `t.log` sí viaja: `history()`
        // lo interpola literalmente en cada petición al proveedor. Sin limpiar,
        // un `Get-Content web.config` o un `type appsettings.json` entregaba su
        // cadena de conexión con contraseña a Anthropic o a Google, en cada turno
        // posterior de la pestaña y en cada sesión restaurada de disco.
        //
        // Lucy no pierde nada para diagnosticar: le basta saber que ahí HAY una
        // credencial. El operador tampoco: la tiene entera en su panel.
        self.tabs[ti].log.push(ChatMsg::exec(
            cmd.clone(),
            ok,
            if retenido {
                i18n::trf("[salida retenida por el guardrail: {motivo}]", &[("motivo", &g.reason)])
            } else {
                lucy_core::memories::scrub(&body)
            },
        ));

        if retenido {
            self.tabs[ti].auto = false;
            self.tabs[ti].ws.trace_push(lucy_core::agent::TraceEntry {
                phase: "info".into(),
                label: i18n::tr("Salida retenida por el guardrail").into(),
                detail: g.reason.clone(),
                ..Default::default()
            });
            // Al modelo se le cuenta QUÉ pasó, no se le enseña el contenido: si
            // se lo pasáramos "para que lo analice" habríamos entregado
            // exactamente lo que el guardrail acaba de retener. El operador sí
            // lo tiene entero, en el panel de Ejecución.
            self.send_raw(ti, format!(
                "El comando `{cmd}` se ejecutó, pero su salida quedó retenida: {}. \
                 No la vas a ver. Dile al operador que la revise él en el panel de \
                 Ejecución y no propongas más comandos sobre este contenido.",
                g.reason
            ));
            // DESPUÉS de la decisión del guardrail y no en el `exec_rx = None` de
            // arriba: reintentar allí lanzaría el siguiente paso antes de que
            // esta rama pudiera apagar el automático, que es justo lo que existe
            // para impedir. `salvo` es esta pestaña — ya tiene turno abierto y su
            // propio cierre la reintenta; las otras son las que llevaban
            // esperando el carril.
            self.reintentar_auto(uid);
            return;
        }

        // Y VUELVE A LUCY. Sin esto, el operador tiene la salida cruda en un
        // panel y sigue sin la respuesta que pidió — que es exactamente lo que
        // pasaba: el comando se proponía, se quedaba ahí, y nadie cerraba el
        // círculo. La aprobación fue el clic; devolver el resultado es la otra
        // mitad de ese mismo gesto.
        //
        // Y LA INSTRUCCIÓN CAMBIA SEGÚN EL MODO. En manual se le pide que
        // resuma y que NO proponga nada más, porque cada comando cuesta un clic
        // y encadenarlos sin que nadie los pida es pesado. En automático esa
        // misma frase mataba la cadena en el primer paso: se le pedía a Lucy
        // que no siguiera, y obedecía.
        let cola = if self.tabs[ti].auto {
            "Resume lo que dice. Si hace falta otro comando para responder a lo \
             que se te pidió, propónlo; si ya tienes la respuesta, dala y no \
             propongas nada más."
        } else {
            "Resúmela y dime qué significa. No propongas ejecutarlo otra vez."
        };
        // ── «ESTO YA HABÍA FALLADO AQUÍ», QUE ES LO QUE DISPARA EL REPLANTEO ──
        //
        // La señal existía entera y no salía de la pantalla. `audit::record`
        // escribe `exit_code` en cada fila, hay índice por comando y por equipo,
        // y `fallos_recientes` la consulta con su ventana de catorce días — pero
        // su único llamante en los dos crates era el panel del workspace.
        //
        // Con el automático encendido no hay nadie leyendo ese panel, y ése es
        // justo el modo donde importa: al modelo se le devolvía la salida con el
        // mismo remate tanto si el comando fue bien como si fue mal, así que un
        // paso fallido no disparaba un replanteo — disparaba otra vuelta
        // idéntica hasta que el tope de puntos la cortaba.
        //
        // SOLO CUANDO HA FALLADO, y solo si ya venía fallando. En un comando que
        // acaba de ir bien el dato no cambia nada de lo que Lucy debería hacer, y
        // una línea más en cada turno es contexto que se paga.
        let historial = if ok {
            String::new()
        } else {
            match lucy_core::audit::fallos_recientes(&cmd, &destino, lucy_core::audit::DIAS_FALLOS)
            {
                // Uno es el que acaba de pasar. A partir de dos hay un patrón.
                Ok(n) if n >= 2 => format!(
                    "\n\nAVISO: este comando exacto ya ha fallado {n} veces en este equipo en \
                     los últimos {} días. Repetirlo tal cual va a volver a fallar. Prueba otra \
                     cosa, o di qué falta para que funcione.",
                    lucy_core::audit::DIAS_FALLOS
                ),
                _ => String::new(),
            }
        };
        // ── LA SALIDA NO SE REPITE AQUÍ, Y ANTES SÍ ─────────────────────────
        //
        // Veinte líneas más arriba entra en `t.log` como `Role::Exec`, y
        // `history()` la interpola en CADA petición de esta pestaña — incluida
        // ésta, porque `send_raw` arma la conversación entera antes de añadir su
        // turno. Así que el volcado viajaba dos veces en el mismo cuerpo.
        //
        // Y la segunda copia no era solo desperdicio. La del log pasa por
        // `memories::scrub`; la que iba aquí iba CRUDA. O sea que una cadena de
        // conexión con contraseña llegaba al proveedor de nube igualmente, en el
        // mismo turno en que el `scrub` decía haberla quitado — el depurado solo
        // servía para los turnos siguientes.
        //
        // Quitarla arregla las dos cosas de una vez: la mitad de los tokens de
        // una cadena automática son salidas de comando, y ahora la única copia
        // que sale de la máquina es la depurada.
        self.send_raw(ti, format!(
            "He ejecutado el comando que propusiste. Su salida literal está en el \
             turno anterior, marcada como salida de `{cmd}`. {cola}{historial}"
        ));
        // Y el carril queda libre para quien lo esperaba. Esta pestaña no —
        // acaba de abrir turno y su propio cierre la reintenta—; las otras
        // llevaban paradas desde que `next_auto` les dijo `Idle` por ocupado, y
        // ese `Idle` no tenía quien lo deshiciera.
        self.reintentar_auto(uid);
    }

    pub(crate) fn pump_compliance(&mut self) {
        let Some((pedido, rx)) = &self.cmp_rx else { return };
        // El mismo cuidado que el inventario: si el operador cambió de equipo
        // mientras esto llegaba, el informe es de otra máquina y se tira.
        let de_otro = *pedido != self.cmp_host;
        match rx.try_recv() {
            Ok(_) if de_otro => {
                self.cmp_rx = None;
                self.cmp_desde = None;
            }
            Ok(r) => {
                self.cmp_rx = None;
                self.cmp_desde = None;
                match r {
                    Ok(rs) => {
                        // LA COMPARACIÓN, ANTES DE GUARDAR. Se busca la pasada
                        // anterior con el corte en «ahora», así que da igual el
                        // orden — pero guardar primero y comparar después
                        // dependería de ese detalle, y esa es la clase de
                        // dependencia que se rompe al mover una línea.
                        let ahora = ahora_epoch();
                        let host = if self.cmp_host.is_empty() {
                            "local".to_string()
                        } else {
                            self.cmp_host.clone()
                        };
                        self.cmp_cambios = lucy_core::posture::anterior(&host, ahora)
                            .ok()
                            .flatten()
                            .map(|p| (p.ts, lucy_core::posture::compara(&p, &rs)))
                            .filter(|(_, v)| !v.is_empty());
                        // Un fallo al guardar NO se lleva por delante el escaneo:
                        // lo que se acaba de medir sigue en pantalla y lo único
                        // que se pierde es poder compararlo la próxima vez.
                        let _ = lucy_core::posture::guarda(&host, ahora, &rs);
                        self.cmp_rs = rs;
                        self.cmp_last = lv_hora();
                        self.cmp_abierto.clear();
                    }
                    Err(e) => {
                        self.cmp_rs.clear();
                        self.cmp_error = e;
                        self.cmp_last.clear();
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.cmp_rx = None;
                self.cmp_desde = None;
                self.cmp_error = i18n::tr("La revisión se cortó sin devolver nada.").into();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    pub(crate) fn pump_inventario(&mut self) {
        let Some((pedido, rx)) = &self.inv_rx else { return };
        // A QUÉ EQUIPO SE LE PIDIÓ. Si el operador cambió de equipo mientras
        // esto llegaba, la foto es de otra máquina y no se enseña bajo el nombre
        // de ésta: se tira. Volver a pedirla cuesta un botón; enseñar los
        // servicios de un servidor como si fueran los de otro no se detecta
        // hasta que alguien actúa sobre ellos.
        let de_otro = *pedido != self.inv_host;
        match rx.try_recv() {
            Ok(_) if de_otro => {
                self.inv_rx = None;
                self.inv_desde = None;
            }
            Ok(r) => {
                self.inv_rx = None;
                self.inv_desde = None;
                match r {
                    Ok(inv) => {
                        self.inv_data = inv;
                        self.inv_last = lv_hora();
                    }
                    // NADA DE DATOS DE EJEMPLO. La V2 enseña un inventario
                    // inventado cuando el escaneo falla —bajo el nombre del
                    // equipo real, y encima cruza esos datos contra la base de
                    // vulnerabilidades—, y su aviso de «datos de ejemplo» está
                    // detrás de una condición que en esa rama nunca se cumple.
                    // Aquí un fallo es un fallo: el motivo y la tabla vacía.
                    Err(e) => {
                        self.inv_data = lucy_core::inventory::Inventory::default();
                        self.inv_error = e;
                        self.inv_last.clear();
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.inv_rx = None;
                self.inv_desde = None;
                self.inv_error = i18n::tr("El escaneo se cortó sin devolver nada.").into();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Recoge la lectura remota y relee sola cuando toca.
    pub(crate) fn pump_logs(&mut self) {
        if let Some(rx) = &self.lv_rx {
            match rx.try_recv() {
                Ok(r) => {
                    self.lv_rx = None;
                    self.lv_desde = None;
                    let nombre = self
                        .remote_hosts
                        .iter()
                        .find(|h| h.id == self.lv_host)
                        .map(|h| h.name.clone())
                        .unwrap_or_else(|| "remoto".into());
                    match r {
                        Ok(l) => self.lv_absorber(l, &nombre),
                        Err(e) => {
                            self.lv_rows.clear();
                            self.lv_error =
                                i18n::trf(
                                    "No se pudo leer «{ruta}» en {equipo}: {e}",
                                    &[
                                        ("ruta", self.lv_path.trim()),
                                        ("equipo", &nombre),
                                        ("e", &e),
                                    ],
                                );
                            self.lv_last.clear();
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.lv_rx = None;
                    self.lv_desde = None;
                    self.lv_error = "La lectura remota se cortó sin devolver nada.".into();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(rx) = &self.lv_files_rx {
            match rx.try_recv() {
                Ok(r) => {
                    self.lv_files_rx = None;
                    match r {
                        Ok(f) => self.lv_files = f,
                        Err(e) => {
                            self.lv_files.clear();
                            self.lv_error = i18n::trf(
                                "No se pudo listar «{ruta}»: {e}",
                                &[("ruta", &self.lv_dir), ("e", &e)],
                            );
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.lv_files_rx = None;
                    self.lv_error = "La exploración se cortó sin devolver nada.".into();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        // El refresco automático NO alcanza al modo remoto, y es a propósito:
        // cada lectura por WinRM levanta un PowerShell que abre una sesión
        // autenticada contra el servidor —segundos, y una entrada en su registro
        // de seguridad— así que repetirlo cada cinco segundos mientras la
        // pestaña está abierta es un martilleo que nadie pidió. Ahí se relee con
        // el botón.
        let auto = match self.lv_mode {
            LvMode::Auditoria => true,
            LvMode::Archivo => self.lv_host.is_empty() && !self.lv_path.trim().is_empty(),
        };
        if self.view == View::LogViewer
            && auto
            && !self.lv_paused
            && Instant::now() >= self.lv_next
        {
            self.lv_cargar();
        }
    }

    /// Recoge el texto dictado cuando el hilo de Whisper termina.
    ///
    /// El texto va al COMPOSITOR, no al hilo: dictar es escribir con la voz,
    /// y mandarlo solo quitaría la oportunidad de corregir una palabra que el
    /// reconocedor entendió mal antes de que Lucy actúe sobre ella.
    pub(crate) fn pump_voice(&mut self) {
        for t in &mut self.tabs {
            let Some(rx) = &t.tr_rx else { continue };
            match rx.try_recv() {
                Ok(Ok(texto)) => {
                    if !texto.is_empty() {
                        if !t.input.is_empty() && !t.input.ends_with(' ') {
                            t.input.push(' ');
                        }
                        t.input.push_str(&texto);
                    }
                    t.tr_rx = None;
                }
                Ok(Err(e)) => {
                    t.log.push(ChatMsg::new(false, i18n::trf("No se pudo transcribir: {e}", &[("e", &e)])));
                    t.tr_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => t.tr_rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    /// Recoge la pasada del vigilante y la deja escrita en el carril de Trace.
    ///
    /// SE ANOTA TAMBIÉN LO QUE SE CALLÓ, y es media razón de que exista este
    /// método. «Me avisa demasiado» y «no me avisó de aquello» son las dos
    /// quejas que va a recibir esta función, y las dos son imposibles de
    /// investigar si lo único que queda escrito es lo que sí salió. Con el
    /// motivo de cada silencio, la conversación deja de ser sobre impresiones.
    ///
    /// En una línea plegada y solo cuando hubo algo: seis «nada que decir» por
    /// minuto llenarían el carril y taparían lo que se está mirando.
    pub(crate) fn pump_vigilante(&mut self) {
        self.recoge_vigilante(false);
        self.recoge_vigilante(true);
    }

    /// Recoge el resultado de la sonda de servicios si ya llegó.
    pub(crate) fn pump_services(&mut self) {
        let Some(rx) = &self.svc_rx else { return };
        match rx.try_recv() {
            Ok(Some(v)) => {
                self.services = v;
                self.svc_medidos = true;
                self.svc_stamp = stamp_now();
                self.svc_rx = None;
            }
            // Sonda fallida o hilo caído: se cierra el turno y se deja intacta
            // la última lista conocida.
            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => self.svc_rx = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Recoge la sonda que haya terminado.
    pub(crate) fn pump_remoto(&mut self) {
        let Some(rx) = &self.remoto_rx else { return };
        match rx.try_recv() {
            Ok(r) => {
                self.remoto_salud = Some(r);
                self.remoto_rx = None;
                self.remoto_desde = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            // El hilo se fue sin contestar. Sin esta rama la pantalla se queda
            // «sondeando» para siempre — el mismo fallo que tenía `pump_chat`.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.remoto_salud = Some((
                    self.selected_host.clone(),
                    Err(i18n::tr("La sonda terminó sin contestar.").to_string()),
                ));
                self.remoto_rx = None;
                self.remoto_desde = None;
            }
        }
    }

    /// Recoge las sondas que hayan terminado.
    pub(crate) fn pump_nx_conn(&mut self) {
        while let Ok((id, r)) = self.nx_conn_rx.try_recv() {
            let (estado, linea) = match r {
                Ok(p) => (
                    Conexion::Ok { os: p.os.clone(), ms: p.ms },
                    (
                        'i',
                        // Lo mismo, y aqui habia ademas DOS versiones de la
                        // misma frase en el fichero: la de la prueba de conexion
                        // si pasaba por la tabla y esta no. Ahora comparten
                        // clave.
                        i18n::trf(
                            "✓ Conectado en {ms} ms{so}",
                            &[
                                ("ms", &p.ms.to_string()),
                                (
                                    "so",
                                    &if p.os.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" · {}", p.os)
                                    },
                                ),
                            ],
                        ),
                    ),
                ),
                Err(e) => (Conexion::Fallo(e.clone()), ('e', e)),
            };
            self.nx_estado.insert(id.clone(), estado);
            self.nx_lines_mut(&id).push(linea);
        }
    }

    pub(crate) fn pump_nx_remote(&mut self) {
        let Some(rx) = &self.nx_exec_rx else { return };
        // TODAS las líneas que haya en este frame, no una. A sesenta cuadros por
        // segundo, una línea por frame convierte un `Get-EventLog` de dos mil
        // líneas en medio minuto de pintado.
        let mut recibidas = Vec::new();
        let mut fin = None;
        loop {
            match rx.try_recv() {
                Ok(lucy_core::hosts::Line::Done(ok)) => {
                    fin = Some(ok);
                    break;
                }
                Ok(l) => recibidas.push(l),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    fin = Some(false);
                    break;
                }
            }
        }
        let id = self.nx_exec_id.clone();
        let lineas = self.nx_lines_mut(&id);
        for l in recibidas {
            match l {
                // La salida de error va SEPARADA y en rojo. En un remoto,
                // distinguir «el comando dijo esto» de «no se pudo llegar» es la
                // mitad del diagnóstico.
                lucy_core::hosts::Line::Err(t) => lineas.push(('e', t)),
                lucy_core::hosts::Line::Out(t) => lineas.push(('o', t)),
                lucy_core::hosts::Line::Done(_) => {}
            }
        }
        if let Some(ok) = fin {
            self.nx_exec_rx = None;
            self.nx_busy = false;
            self.nx_fase = None;
            // EL ORIGEN VIENE CON EL COMANDO, no cableado aquí. Esta línea
            // decía «manual» fija, con un comentario que afirmaba que por aquí
            // solo pasa lo que teclea el operador — y la traducción del modelo
            // pasa por aquí también, desde la línea 1115 de este mismo fichero.
            let ms = self.nx_started.map_or(0, |t| t.elapsed().as_millis() as u64);
            let (cmd, salida) = self.nx_ultimo_comando(&id);
            if !cmd.is_empty() {
                self.auditar(None, &cmd, &id, self.nx_origen, ok, ms, &salida);
            }
            self.nx_started = None;
            if !ok {
                let parado = self.nx_stop.load(std::sync::atomic::Ordering::Relaxed);
                self.nx_lines_mut(&id).push((
                    'e',
                    i18n::tr(if parado {
                        "(detenido)"
                    } else {
                        "(el comando terminó con error)"
                    })
                    .to_string(),
                ));
                // ── Y SE PREGUNTA POR QUÉ ────────────────────────────────────
                //
                // «El comando terminó con error» es el final de la
                // conversación, y no debería serlo: el operador tiene delante
                // un servidor, un error de PowerShell y ninguna pista. Es lo que
                // la V1 hacía y aquí se había quedado fuera — reportado tal cual:
                // «tenía la capacidad de responderme si un comando fallaba y me
                // recomendaba intentarlo de otra manera».
                //
                // NO SI LO PARÓ EL OPERADOR. Un comando detenido a mano no ha
                // fallado: se ha cancelado, y explicar por qué «falló» algo que
                // uno mismo acaba de cortar es ruido.
                //
                // NI SI NO HAY NADA QUE EXPLICAR. Sin comando o sin salida no
                // hay diagnóstico posible, y una llamada al modelo para que
                // adivine cuesta dinero y devuelve invención.
                if !parado && !cmd.is_empty() && !salida.trim().is_empty() {
                    self.nx_diagnostica(&id, &cmd, &salida);
                }
            } else if salida.chars().count() >= lucy_core::nexshell::MIN_SALIDA_RESUMEN {
                // ── Y SI SALIÓ BIEN PERO ES ILEGIBLE, SE LEE ─────────────────
                //
                // Reportado: «al momento de pedirle que revisara si existen
                // actualizaciones, trae la información en crudo, no procesa la
                // información ni da análisis».
                //
                // Exacto, y es lo que separa una shell con IA de una terminal.
                // `Get-WindowsUpdate` devuelve el volcado de un objeto COM:
                // cuarenta propiedades por actualización, con
                // `System.__ComObject` en la mitad de ellas. Lo que se preguntó
                // fue «¿hay actualizaciones?» y lo que vuelve son doscientas
                // líneas donde la respuesta está enterrada.
                //
                // POR UMBRAL Y NO SIEMPRE. `Get-Service Spooler` son tres líneas
                // que se leen de un vistazo: resumirlas costaría dinero para
                // devolver una frase más larga que la salida. Ver
                // `MIN_SALIDA_RESUMEN`.
                self.nx_resume(&id, &cmd, &salida);
            }
        }
    }

    /// Da por perdida una espera a un modelo que no vuelve.
    ///
    /// EL HILO NO SE MATA, SE DEJA DE ESPERAR. No hay forma de cortar una
    /// petición HTTP ya lanzada desde fuera del cliente; lo que sí se puede es
    /// no tener la pantalla secuestrada por ella. Cuando el hilo termine, su
    /// respuesta se encuentra el canal cerrado y se descarta.
    pub(crate) fn pump_nx_plazo(&mut self) {
        // SOLO LAS ESPERAS A UN MODELO. Ver `PLAZO_MODELO_SECS`: un comando
        // remoto puede tardar media hora legítimamente.
        let esperando_modelo = self.nx_rx.is_some() || self.nx_diag_rx.is_some();
        if !esperando_modelo {
            return;
        }
        let Some((_, desde)) = self.nx_fase else { return };
        if desde.elapsed().as_secs() < Self::PLAZO_MODELO_SECS {
            return;
        }
        let id = self
            .nx_destino
            .as_ref()
            .map(|h| h.id.clone())
            .or_else(|| self.nx_diag_rx.as_ref().map(|(i, _)| i.clone()))
            .unwrap_or_default();
        let aviso = i18n::trf(
            "El modelo no contestó en {s}s. Se deja de esperar; vuelve a intentarlo.",
            &[("s", &Self::PLAZO_MODELO_SECS.to_string())],
        );
        if id.is_empty() {
            self.nx_say(&aviso);
            self.nx_rx = None;
            self.nx_diag_rx = None;
            self.nx_busy = false;
            self.nx_started = None;
            self.nx_fase = None;
            self.nx_destino = None;
        } else {
            self.nx_suelta(&id, &aviso);
        }
    }

    /// Recoge el diagnóstico y lo enseña. No ejecuta nada.
    pub(crate) fn pump_nx_diag(&mut self) {
        let Some((id, rx)) = &self.nx_diag_rx else { return };
        match rx.try_recv() {
            Ok(r) => {
                let id = id.clone();
                self.nx_diag_rx = None;
                self.nx_fase = None;
                let Some(d) = r else { return };
                self.nx_lines_mut(&id).push((
                    'i',
                    i18n::trf("↳ {causa}", &[("causa", &d.causa)]),
                ));
                if !d.prueba.is_empty() {
                    self.nx_sugerido = Some((id, d.prueba));
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.nx_diag_rx = None,
        }
    }

    pub(crate) fn pump_nx_test(&mut self) {
        let Some(rx) = &self.nx_test_rx else { return };
        match rx.try_recv() {
            Ok(v) => {
                self.nx_test = Some(v);
                self.nx_test_rx = None;
                self.nx_testing = false;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.nx_test_rx = None;
                self.nx_testing = false;
            }
        }
    }

    /// Recoge la traducción cuando llega.
    pub(crate) fn pump_nx(&mut self) {
        let Some(rx) = &self.nx_rx else { return };
        match rx.try_recv() {
            Ok(Ok(cmd)) => {
                self.nx_rx = None;
                self.nx_busy = false;
                let destino = self.nx_destino.take();
                // ── LA NEGATIVA, CON SU MOTIVO ───────────────────────────────
                //
                // Decía «No supe convertir eso en un comando» y ahí terminaba la
                // conversación. El modelo SÍ sabía por qué —«instala winget» en
                // un Server 2022 no es un comando, son varios pasos— pero no
                // había dónde decirlo. Es el mismo agujero que tenía un comando
                // fallido antes del diagnóstico: la respuesta se acaba justo
                // donde empieza lo útil.
                let sin = lucy_core::nexshell::motivo_sin_comando(&cmd);
                if cmd.is_empty() || sin.is_some() {
                    let motivo = sin.unwrap_or_default();
                    let texto = if motivo.is_empty() {
                        i18n::tr("No supe convertir eso en un comando.").to_string()
                    } else {
                        i18n::trf(
                            "No se resuelve con un solo comando: {motivo}",
                            &[("motivo", &motivo)],
                        )
                    };
                    self.nx_aviso(destino.as_ref(), &texto);
                    return;
                }
                // La traducción vuelve al equipo QUE LA PIDIÓ. Sin guardar el
                // destino, un comando pensado para una Debian remota acabaría
                // ejecutándose en el PowerShell de aquí.
                // Las dos ramas son la MISMA procedencia: esto es la
                // traduccion que acaba de escribir el modelo.
                match destino {
                    Some(h) => self.nx_gate_remote(&h, cmd, "ai"),
                    None => self.nx_maybe_run(cmd, "ai"),
                }
            }
            // Se dice en la propia pantalla y no en un diálogo: el operador está
            // mirando ahí, y un error de traducción es parte de la sesión.
            Ok(Err(e)) => {
                self.nx_rx = None;
                self.nx_busy = false;
                let destino = self.nx_destino.take();
                // POR LA TABLA. Este `format!` suelto sacaba el error en
                // español en los cinco idiomas — el mismo agujero de siempre.
                self.nx_aviso(
                    destino.as_ref(),
                    &i18n::trf("No se pudo traducir: {e}", &[("e", &e)]),
                );
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.nx_rx = None;
                self.nx_busy = false;
            }
        }
    }

    /// Recoge el progreso de la ingesta. Corre en cada frame, esté la vista que
    /// esté: cambiar de pantalla no puede parar un documento a medias.
    pub(crate) fn pump_docs(&mut self) {
        use lucy_core::docs::Paso;
        let Some(rx) = &self.doc_rx else { return };
        let mut fin = false;
        while let Ok(p) = rx.try_recv() {
            self.doc_estado = Some(match p {
                Paso::Extrayendo => (i18n::tr("Extrayendo texto…").into(), false),
                Paso::Troceando(n) => (
                    i18n::trf("Troceando: {n} trozos", &[("n", &n.to_string())]),
                    false,
                ),
                Paso::Embebiendo(h, total) => (
                    i18n::trf(
                        "Embebiendo {h}/{total}…",
                        &[("h", &h.to_string()), ("total", &total.to_string())],
                    ),
                    false,
                ),
                Paso::Listo(d) => {
                    fin = true;
                    self.docs_l = Some(lucy_core::docs::list());
                    (
                        i18n::trf(
                            "«{nombre}» ingerido: {trozos} trozos, todos con vector.",
                            &[("nombre", &d.nombre), ("trozos", &d.trozos.to_string())],
                        ),
                        false,
                    )
                }
                Paso::SinVectores(d, e) => {
                    fin = true;
                    self.docs_l = Some(lucy_core::docs::list());
                    (
                        i18n::trf(
                            "«{nombre}» quedó buscable por palabras ({hechos} de {total} con \
                             vector): {e}",
                            &[
                                ("nombre", &d.nombre),
                                ("hechos", &d.vectorizados.to_string()),
                                ("total", &d.trozos.to_string()),
                                ("e", &e),
                            ],
                        ),
                        false,
                    )
                }
                // NO CIERRA LA INGESTA, y por eso `fin` se queda como está. Es
                // el guardrail diciendo que ese documento lleva dentro algo con
                // forma de instrucción; el documento entra igual —un manual de
                // este oficio cita ataques— pero el operador tiene que saberlo,
                // porque ese texto va a acabar en el prompt de sistema.
                //
                // Se pinta como aviso y no como error: un error deja el
                // documento fuera y esto no. Pintarlos igual haría creer que la
                // ingesta falló.
                Paso::Aviso(a) => (a, true),
                Paso::Error(e) => {
                    fin = true;
                    (e, true)
                }
            });
        }
        if fin
            || matches!(
                self.doc_rx.as_ref().map(|r| r.try_recv()),
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected))
            )
        {
            self.doc_rx = None;
        }
    }
}
