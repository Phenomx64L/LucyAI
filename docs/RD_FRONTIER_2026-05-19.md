# Lucy — I+D Frontier Roadmap (2026-05-19)

> Documento de investigación y desarrollo: capacidades que NINGUNA herramienta de IA
> tiene hoy, explotando la ventaja única de Lucy como *agente local con control real
> de objetos del sistema* en Windows. Más: re-imaginación UI/UX.
>
> Audiencia: Iván (decisor) + Lucy v2 development.
> Estado: propuesta — ninguna feature implementada todavía.

---

## Por qué Lucy puede hacer lo que nadie más puede

| Competidor | Limitación fundamental |
|-----------|-----------------------|
| ChatGPT / Claude.ai | Sandboxed. No ven tu disco, no tocan procesos, no persisten estado entre sesiones. |
| Cursor / Copilot | Atados al IDE. No saben qué hay fuera del repo abierto. |
| Claude Code | Sin memoria persistente cross-session. Sin telemetría del sistema. |
| Copilot for Windows | UX limitada a sugerencias. No autonomía, no introspección estructural. |
| Zapier / IFTTT / n8n | Reactivos a eventos predefinidos. No infieren causas. |
| EDR (CrowdStrike etc.) | Detección sí, pero sin LLM en el loop ni explicabilidad. |

**Ventaja de Lucy = intersección rara**: LLM + acceso nativo profundo a Windows + memoria
persistente + audit trail + reflection gate. Esto desbloquea capacidades *cualitativamente*
distintas, no solo "ChatGPT pero local".

---

# Parte 1 — Capacidades Frontera

Ordenadas por **moat strategic value** (cuán difícil sería para otro replicarlo).

---

## F1. Process Lineage Recorder con Audit Chain

**Idea**: Cada proceso que arranca en tu equipo queda registrado con su árbol
genealógico completo: padre, abuelo, comando, hash SHA-256 del ejecutable,
working directory, variables de entorno relevantes, timestamp. Encadenado con
SHA-256 como ya tienes en `incident_action`.

Pregunta que Lucy puede contestar y NADIE más:
- "¿Qué arrancó este `python.exe` que está al 100% CPU?"
- "¿Cuándo apareció por primera vez `svchost.exe` con este parent atípico?"
- "Reproduce el árbol de procesos de ayer a las 14:32"
- "Lista todos los hijos transitivos de mi Discord desde que arrancó"

**Por qué es frontera**: Procmon de Sysinternals hace algo similar pero sin
LLM en el loop. ChatGPT no tiene acceso. EDR ve esto pero no te deja preguntar
en lenguaje natural ni reconstruir narrativas.

**Implementación** (~2 semanas):
- Rust: ETW (Event Tracing for Windows) subscription a `Microsoft-Windows-Kernel-Process`
- Persistencia: `process_lineage` (id, pid, ppid, exe_path, exe_hash, cmdline, cwd, started_at, parent_chain_hash)
- Retención: anillo de 7 días, rotación automática
- Frontend: vista "Process Forest" con filtros temporales
- LLM: nuevo tool `<TOOL>process_history:filter</TOOL>` que devuelve árbol contextual

**Riesgos**: ETW requiere SeSystemProfilePrivilege en algunos casos → fallback a polling de `Get-Process`.

---

## F2. Ambient State Snapshots + Temporal Diff

**Idea**: Cada N minutos (ajustable, default 15min), Lucy toma un snapshot
estructurado de tu equipo:
- Servicios corriendo + su estado
- Conexiones de red activas
- Procesos top por CPU/RAM
- Variables de entorno
- Software instalado (vía registry)
- Scheduled tasks habilitadas
- Mounted drives + espacio
- Configuración de firewall

Almacena snapshots como JSON comprimido. **El verbo nuevo**: `diff_snapshots(t1, t2)`.

Pregunta que Lucy puede contestar:
- "¿Qué cambió desde ayer en la mañana?"
- "¿Cuándo empezó a aparecer este servicio en mi máquina?"
- "Compara mi máquina ahora vs el viernes antes de que dejara de funcionar X"
- "Cuántas veces ha aparecido el proceso Y esta semana"

**Por qué es frontera**: Time Machine de macOS hace algo similar para archivos.
Nadie lo hace para *estado del sistema* + LLM. Es esencialmente "git para tu OS".

**Implementación** (~1 semana):
- Rust: `system_snapshot.rs` con función `capture()` que devuelve `SystemSnapshot` serializable
- Storage: `state_snapshots` (id, captured_at, payload BLOB zstd)
- Diff: librería `similar` de Rust + comparador semántico (saber que un PID nuevo del mismo binario no es "cambio")
- Tool nuevo: `<TOOL>state_diff:from_iso:to_iso</TOOL>`

**Moat**: Cuanto más tiempo lo uses, más valioso. Lock-in natural.

---

## F3. Causal Inference Engine para Performance

**Idea**: Cuando hay un spike de CPU/RAM/disk/network, Lucy correlaciona automáticamente:
- Eventos de proceso (start/stop) en los últimos N segundos
- Cambios de Registry recientes
- Conexiones de red abiertas/cerradas
- Scheduled tasks que dispararon
- I/O de archivos pesados

Construye una **cadena causal probabilística**: "El spike de CPU a las 14:23 fue
causado *probablemente* por `chrome.exe` (peso 0.7), disparado por `Outlook.exe`
que abrió un link, debido a que `scheduler.exe` ejecutó la tarea Y a las 14:22:50."

Pregunta que Lucy puede contestar:
- "¿Por qué se puso lenta mi máquina hace 5 minutos?"
- "Cada vez que pongo Zoom, mi CPU explota — encuentra por qué"

**Por qué es frontera**: Performance Monitor de Windows muestra correlación
temporal pero deja al humano inferir causalidad. Datadog/NewRelic hacen esto
para servidores pero no integran LLM ni viven en tu desktop. Lucy puede explicar.

**Implementación** (~3 semanas):
- Recolector: aprovecha F1 + F2 + el ya-existente `metrics`
- Motor de inferencia: ventana sliding de 60s alrededor del anomaly trigger,
  ranking por correlación temporal + magnitud + plausibilidad heurística
- Output: árbol de causalidad rankeado con confidence scores
- Tool: `<TOOL>diagnose_spike:metric:timestamp</TOOL>`
- Hook al sistema de incidentes que ya tienes

**Sinergia**: Combina F1, F2 y el reflection gate.

---

## F4. Self-Healing con Memoria Acumulativa

**Idea**: Cuando Lucy detecta una anomalía (vía `anomaly-bridge` que ya tienes),
busca en su `agent_memories` si ya ha visto algo parecido antes y qué se hizo.
Si encuentra un fix con alto éxito histórico → propone aplicarlo (HITL: human
in the loop, opcionalmente automático para low-risk). Si no, investiga, y el
fix exitoso se guarda como skill.

Pregunta que Lucy resuelve:
- "Mi disco está al 95% otra vez" → Lucy recuerda: "La última vez ejecutaste
  `cleanmgr /sageset:1` y liberaste 30GB. ¿Lo aplico?"

**Por qué es frontera**: SCCM/Intune hacen self-healing con scripts predefinidos
pero no aprenden. ChatGPT no recuerda. Lucy combina memoria longitudinal +
detección + ejecución verificada.

**Implementación** (~2 semanas — gran parte ya existe):
- Hook nuevo: cuando `incident_finalize` se llama con `status='resolved'`,
  extraer la(s) acción(es) que resolvieron + síntomas iniciales → guardar como
  `healing_pattern` con embeddings
- Búsqueda al detectar anomalía: vector search en patterns + matching de síntomas
- Confidence scoring antes de proponer (cuántas veces funcionó, hace cuánto, etc.)
- UI: badge "💊 Lucy recuerda haber resuelto algo similar — ver fix"

**Aprovecha**: tu sistema de memoria + skills + reflection gate + incidents — todos
ya existen.

---

## F5. Sandbox-First Execution

**Idea**: Antes de ejecutar comandos potencialmente destructivos (delete, format,
modify registry root keys, install software, etc.), Lucy los corre primero en
**Windows Sandbox** o un contenedor liviano, captura *qué tocó*, y muestra al
usuario el diff antes de aplicar en host.

Output al usuario:
- Archivos creados/modificados/borrados
- Servicios start/stop
- Registry keys tocadas
- Network connections abiertas

**Por qué es frontera**: NADIE hace esto con LLM. Es esencialmente un "dry-run real"
en vez del dry-run actual que solo muestra el comando.

**Implementación** (~2-3 semanas):
- Detección: Windows Sandbox (`WindowsSandbox.exe`) viene con Win10 Pro+
- Lucy genera `.wsb` config con el comando, monta el path necesario read-only
- Captura efectos: snapshot pre + post del sandbox vía `Procmon` o equivalente
- Fallback: si no hay Sandbox disponible, usar `RunAs` con cuenta limitada o
  filtro manual sin sandbox
- UI nueva: "Sandbox Preview" panel mostrando el diff

**Constraint**: Windows Sandbox no está en Home edition. Detectar y degradar UX.

---

## F6. Cross-App Object Bridge (PS Objects → AI Context)

**Idea**: PowerShell devuelve objetos estructurados (`PSCustomObject`), no solo
texto. Hoy Lucy los serializa y los muestra. **Nuevo**: los objetos quedan
"vivos" en un namespace de sesión, puedes pipearlos a comandos sucesivos
ESPECIFICANDO POR LENGUAJE NATURAL.

Ejemplo:
```
> Lista los servicios no-default
[Lucy guarda el array de 47 services en $L.lastResult]
> De esos, los que llevan más de 30 días sin reiniciarse
[Lucy filtra usando la metadata, no re-ejecuta]
> Pásamelos como gráfica de barras
[Lucy renderiza widget]
> Reinicia los que tienen "Adobe" en el nombre
[Lucy filtra + previa confirmación + ejecuta]
```

**Por qué es frontera**: Ningún chatbot mantiene objetos estructurados con
identidad. ChatGPT te da el JSON pero pierde el "objeto vivo". Lucy + PS = pipa
infinita con AI mediando.

**Implementación** (~2 semanas):
- Backend: `object_session.rs` con map `session_id → Vec<TypedObject>`
- TypedObject contiene: schema inferido, raw, índices secundarios para queries rápidos
- Tool nuevo: `<TOOL>obj_query:filter</TOOL>` que opera sobre `$L.lastResult`
- Frontend: panel lateral "Objetos en contexto" con count + tipo
- TTL automático: 30min sin uso → garbage collect

**Bonus**: combinable con MCP — los objetos pueden venir de cualquier MCP server.

---

## F7. Generative Runbook from Observation

**Idea**: Lucy *observa silenciosamente* tus sesiones (con consent). Cuando
detecta que has hecho la misma secuencia 3+ veces, propone convertirla en runbook
nombrado.

Ejemplo:
> Lucy: "He notado que cada lunes ejecutas: `git pull` en 5 repos + `npm install`
> en 3 + abres VSCode con un workspace. ¿Quiero crear el comando `/monday-setup`?"

Pregunta que Lucy resuelve:
- "¿Qué patrones repetitivos hago que podría automatizar?"

**Por qué es frontera**: AutoHotkey requiere programación explícita. AI tools no
observan tu workflow. Esto es *learn-by-watching*.

**Implementación** (~3-4 semanas):
- Aprovecha tu `conversation_turns` + `incident_action` + audit log
- Algoritmo: sequence mining sobre commands ejecutados (FP-Growth o PrefixSpan adaptado)
- Threshold: secuencia de ≥3 pasos repetida ≥3 veces en ventana ≥14 días
- UI: notificación pasiva, nunca interrumpe
- Storage: usa tu `skills` table existente

**Privacy**: opt-in explícito. Local 100%. Nunca sale del equipo.

---

## F8. Behavioral Fingerprinting de Anomalías (mini-EDR con AI)

**Idea**: Lucy aprende el "perfil normal" de tu equipo:
- Qué procesos arrancan típicamente
- Sus parents habituales
- Sus directorios de origen
- Sus patrones de network IO
- Sus relaciones temporales

Cuando aparece algo que rompe el perfil (proceso con nombre alto-entropía, parent
inusual, conexión a IP exótica, escribiendo a `Temp/`...), Lucy alerta + ofrece
quarantine.

**Por qué es frontera**: EDRs comerciales hacen esto pero son cajas negras. Aquí
es local, explicable, y vive junto a tu asistente.

**Implementación** (~3 semanas):
- Recolector: F1 + F2
- Modelo: simple Bayesian + decision tree (no DL, ridículo en local). Features:
  ratio de chars únicos en nombre, profundidad en árbol, edad del binario,
  presencia en `%AppData%` vs `%ProgramFiles%`, etc.
- Output: severity score 0-1 + razones humanas
- Tool: `<TOOL>scan_anomalous_processes</TOOL>`
- UI: Posture Strip nuevo widget "Threat radar"

**Marketing angle**: "Lucy es tu asistente Y también te protege".

---

## F9. Filesystem-Wide Personal Knowledge Graph

**Idea**: Lucy indexa tus directorios prioritarios (configurable). No solo
contenido (ya hace eso con el vec_index) — también **relaciones**:
- Archivo A modificado, después archivo B (temporal correlation)
- Archivo A importa archivo B (parsing)
- Carpeta C tiene N archivos del tipo X
- Repo D tiene mismas dependencies que repo E

Resultado: grafo navegable + queryable.

Pregunta que Lucy resuelve:
- "¿Qué archivos tocan la config de mi proyecto Y?"
- "¿En cuántos proyectos uso rusqlite?"
- "Dame los archivos que más toco esta semana"

**Por qué es frontera**: Tu hard drive tiene un grafo implícito masivo. Nadie lo
hace explícito + queryable + AI-mediated.

**Implementación** (~4 semanas — grande):
- Indexer en Rust con `notify` crate para watch
- Storage: `petgraph` o SQLite con tablas de aristas
- Tool: `<TOOL>kg_query:cypher-like-syntax</TOOL>` (o sintaxis simple)
- UI: vista grafo opcional (D3.js o Cytoscape.js)

**Constraint**: privacy obvious. Solo carpetas opt-in.

---

## F10. Predictive Pre-warming (Daily Pattern Learning)

**Idea**: Lucy aprende rutinas temporales:
- Lunes 9am → abres VSCode + Spotify + 3 tabs específicos de Chrome
- Días de cobro → abres Excel + tu app de finanzas
- Después de Zoom → siempre buscas un archivo de notas

Lucy entonces **prepara** las cosas:
- Pre-warm de cachés
- Pre-arranque de servicios pesados
- Pre-abrir tabs (opcional, con confirmación)
- Notificación discreta: "Tus apps del lunes están listas"

**Por qué es frontera**: Smart home tiene esto. Tu OS no lo tiene. Y con AI mediando
las recomendaciones son explicables.

**Implementación** (~2 semanas):
- Storage: `daily_patterns` (weekday, hour, action, frequency, confidence)
- Algoritmo: ventana de 4 semanas, threshold 3/4 ocurrencias para considerar patrón
- Trigger: scheduled task interno cada 15min comparando hora actual vs patrones
- UI: card de "Lucy preparando..." con cancel inmediato

---

## Resumen de Capacidades — Matriz Prioridad/Esfuerzo

| Feature | Moat | Esfuerzo | Reuso código | Recomendación |
|---------|------|----------|--------------|---------------|
| F1 Process Lineage | Alto | 2 sem | Audit chain | **Quick win — empezar aquí** |
| F2 State Snapshots | Alto | 1 sem | metrics | **Quick win** |
| F3 Causal Engine | Muy alto | 3 sem | F1+F2 | Después de F1+F2 |
| F4 Self-Healing | Alto | 2 sem | memorias, skills | **Recomendado siguiente** |
| F5 Sandbox-First | Alto | 3 sem | reflection-gate | Diferenciador fuerte |
| F6 Object Bridge | Medio-Alto | 2 sem | shell.rs | Cool, pero menos urgente |
| F7 Runbook Gen | Alto | 4 sem | skills, audit | Largo plazo |
| F8 Mini-EDR | Muy alto | 3 sem | F1+F2 | Después de F1+F2+F3 |
| F9 Knowledge Graph | Alto | 4 sem | vec_index | Largo plazo |
| F10 Predictive Pre-warm | Medio | 2 sem | - | Nice-to-have |

**Secuencia sugerida**: F2 → F1 → F4 → F3 → F8 (forma una pila coherente de
observabilidad → causalidad → defensa, cada capa apoya la siguiente).

---

# Parte 2 — UI/UX Frontier

## Diagnóstico del estado actual

Strengths:
- Paleta clara, identidad propia ("Op Center" feel)
- Densidad bien calibrada para sysadmin
- PostureStrip + Dashboard ya muestran "ambient state"

Weaknesses observadas:
- Chat lineal está al centro como si fuera un IM, pero Lucy hace MUCHO más
- Falta jerarquía visual entre "lo que estoy preguntando" y "lo que Lucy
  observa de fondo"
- Tabs tradicionales no representan que cada tab es un *contexto operativo* distinto
- Sin storytelling cuando Lucy hace algo multi-paso (sale como spam de tool cards)
- Falta sensación de "asistente vivo" — Lucy no respira, no insinúa, no anticipa visualmente

---

## U1. Spatial Workspaces (no tabs, sino "salas")

**Cambio**: cada tab actual → una "sala" con:
- Cámara (vista principal: chat / dashboard / log viewer)
- Memoria espacial: dónde dejaste los paneles, qué hosts seleccionaste
- Ambient indicators propios de esa sala

Visualmente: la barra superior de tabs se vuelve un **carrusel de cards** que
muestran preview de lo que hay en cada sala (no solo título), con respiración
suave según actividad.

**Inspiración**: Notion workspaces, Arc browser spaces.

**Implementación**: ~1 semana
- Refactor de tabs a `workspaces` con campo `layout_state` JSON
- Vista preview hover

---

## U2. Lucy Living Avatar — Estados Visuales

**Cambio**: el avatar de Lucy en la izquierda no es estático. Tiene estados:
- **Idle**: leve breath, pupilas siguiendo cursor sutilmente
- **Thinking**: pulso interno cyan
- **Executing**: shimmer dorado en el contorno
- **Concerned** (anomalía detectada): borde amber pulsante
- **Confident** (verificación passed): brief flash verde

Cada estado tiene microsonido opcional (off by default).

**Por qué importa**: el "asistente vivo" se vuelve presencia, no widget. Estudios
muestran que ambient agents aumentan engagement.

**Implementación**: ~3-4 días
- SVG avatar con classes CSS para cada estado
- Store nuevo `lucyMood: 'idle' | 'thinking' | ...` driven por events
- CSS animations con `prefers-reduced-motion` respect

---

## U3. Chapter View para Multi-Step Tasks

**Cambio**: cuando Lucy hace una tarea multi-paso (agent loop), en vez de
spam de tool-cards, se renderiza como **un "chapter book"**:
- Cover: título + objetivo
- Páginas: cada paso con su rationale + resultado colapsable
- Index lateral para saltar
- "Final" con conclusión

Visualmente: como un "Reading mode" estilo Apple News dentro del chat.

**Por qué importa**: el spam de tool cards rompe la narrativa. Esto la restaura.

**Implementación**: ~1 semana
- Nuevo componente `AgentTaskChapterView.svelte`
- Triggered cuando `_isMultiStep` y `> 3` pasos
- Toggle entre "linear view" (actual) y "chapter view"

---

## U4. Heat Layers en File Tree y Process Tree

**Cambio**: cuando Lucy muestra un árbol (archivos, procesos), se sobrepone un
**heat layer** que codifica:
- Color: severity actual (CPU/IO/error rate)
- Opacidad: recencia de actividad
- Glow: anomalía detectada

Hover: tooltip con timestamp + acciones rápidas.

**Inspiración**: Datadog flame graphs, htop colors.

**Implementación**: ~1 semana

---

## U5. Predictive Chip Strip

**Cambio**: una *thin strip* sobre el input que muestra 2-3 chips con
acciones anticipadas, basadas en el contexto reciente. No interrumpe, está siempre.

Ejemplo:
- Después de un `git status` con cambios: chip "commit + push" + chip "diff completo"
- Después de un error de servicio: chip "restart service" + chip "ver event log"

**Diferencia con suggestions actuales**: estos son *enacted previews* — al hover
te muestra qué pasaría, click los ejecuta (con HITL si destructivo).

**Implementación**: ~1.5 semanas — necesita signal extraction del último turno

---

## U6. Density Modes Adaptativos

**Cambio**: tres modos automáticos:
- **Focus**: solo chat, todo lo demás colapsado, blur sutil de fondo
- **Explore**: actual, todos los paneles
- **War room**: dashboard al frente, chat compacto en sidebar derecho, anomalías en grande

Switch por:
- Manual (Ctrl+1/2/3)
- Auto: detección de anomalía → "war room" se sugiere

**Implementación**: ~3 días

---

## U7. Drag-to-Lucy Universal Drop

**Cambio**: arrastrar CUALQUIER cosa a Lucy:
- Archivo del Explorer → "analiza/ejecuta/abre"
- URL del browser → "fetch/ingest/summarize"
- Texto seleccionado → "explica/traduce/refactor"
- Screenshot → "describe/extract text"
- Otra ventana (con Win11 snap) → "monitorea esta app"

**Por qué**: tu cerebro ya sabe arrastrar. Lucy se vuelve omnívora.

**Implementación**: ~2 semanas — ya tienes parte del file drop, extender a otros tipos.

---

## U8. Confidence-Tagged Output

**Cambio**: en las respuestas de Lucy, las partes con baja confidence se
**stylan distinto**:
- Alta confidence: texto normal
- Media: subrayado punteado discreto
- Baja: italic + tooltip "no estoy segura, basé esto en X"
- Cita: chip enlazado a la fuente (memoria, archivo, web)

**Por qué importa**: combate la sensación de "AI segura de todo". Mejora trust calibration.

**Implementación**: ~1 semana — extender tu `renderConfidenceTags` con más granularidad

---

## U9. Time-of-Day Theming Sutil

**Cambio**: los acentos cromáticos shift suavemente con la hora:
- Mañana: acentos cyan-verdes brillantes
- Tarde: acentos warmer, más naranja
- Noche: paleta más fría, contraste reducido para fatiga visual

Diferencia con "dark mode auto": esto es *microajuste continuo*, no on/off.

**Implementación**: ~2 días — variables CSS con interp linear según hora local

---

## U10. Command Palette → Compositor

**Cambio**: el Cmd+K actual es lookup. **Nuevo**: es un *compositor*:
- Empiezas a escribir, ves matches
- Al elegir, no se ejecuta inmediatamente — entra a un "preview lane"
- Puedes componer múltiples acciones encadenadas
- Lucy te muestra el plan integrado antes de submit
- Submit ejecuta todo con HITL en cada step destructivo

**Inspiración**: Raycast extensions, Linear command bar.

**Implementación**: ~2 semanas — refactor de CommandPalette

---

## Resumen UI/UX — Matriz Impact/Effort

| Idea | Impact | Effort | Prioridad |
|------|--------|--------|-----------|
| U2 Living Avatar | Alto (identidad) | 3-4d | **P0 — quick win** |
| U6 Density Modes | Alto (workflow) | 3d | **P0** |
| U9 Time-of-Day | Medio (delight) | 2d | **P0 — fácil** |
| U3 Chapter View | Muy alto (legibilidad) | 1 sem | **P1** |
| U5 Predictive Chips | Alto (productivity) | 1.5 sem | P1 |
| U7 Drag-to-Lucy | Alto (UX universal) | 2 sem | P1 |
| U8 Confidence Tags | Alto (trust) | 1 sem | P1 |
| U1 Spatial Workspaces | Alto (mental model) | 1 sem | P2 |
| U4 Heat Layers | Medio | 1 sem | P2 |
| U10 Compositor Palette | Alto (power user) | 2 sem | P2 |

**Primer sprint UX recomendado** (5-6 días): U2 + U6 + U9 — todos son delight,
bajo riesgo, y juntos ya transforman la "sensación" de Lucy.

---

# Parte 3 — Sinergias Capacidades × UI

Algunas features se potencian al combinarse:

- **F2 State Snapshots + U6 Density Modes**: el modo "War room" puede mostrar
  el diff vs snapshot anterior en grande.

- **F3 Causal Engine + U3 Chapter View**: la cadena causal es naturalmente un
  "chapter" con secciones (síntoma → evidencia → hipótesis → conclusión).

- **F4 Self-Healing + U8 Confidence Tags**: cuando Lucy propone un fix
  recordado, su confidence visual = success rate histórico del patrón.

- **F6 Object Bridge + U7 Drag-to-Lucy**: arrastrar un objeto persistido al chat
  para operarlo.

- **F8 Mini-EDR + U2 Living Avatar**: el avatar muta a "concerned" cuando
  detecta algo extraño.

---

# Parte 4 — Riesgos y mitigaciones

| Riesgo | Mitigación |
|--------|-----------|
| Privacy: F2/F9 acumulan mucha data | Storage 100% local, opt-in granular, retention policies |
| Performance: F1 puede ser intensivo | ETW filtering agresivo, rate limiting, ring buffer |
| False positives: F8 mini-EDR | Threshold conservador, siempre HITL, "learn from corrections" |
| Complejidad creciente: UI puede saturarse | U6 density modes mitigan, focus mode siempre disponible |
| Lock-in: data acumulada | Export tools desde día 1 (JSON + SQL dumps) |

---

# Parte 5 — Próximos pasos concretos

**Si decides arrancar mañana, propongo este flow** (8 semanas):

| Semana | Capability | UI/UX | Resultado |
|--------|-----------|-------|-----------|
| 1 | F2 State Snapshots backend | U9 Time-of-day | Snapshots persistiendo, theming sutil |
| 2 | F2 frontend + tool integration | U6 Density modes | Pregunta "qué cambió" funcional |
| 3 | F1 Process Lineage backend | U2 Living Avatar | Procesos trazados, Lucy "respira" |
| 4 | F1 frontend integration | U3 Chapter view skeleton | Process forest visible |
| 5 | F4 Self-Healing engine | U3 Chapter view aplicado | Lucy recuerda fixes |
| 6 | F4 + F2/F1 integration | U8 Confidence tags | Self-healing con explainability |
| 7 | F3 Causal Engine MVP | U5 Predictive chips | "¿Por qué se puso lenta?" answerable |
| 8 | F3 polish + tests + docs | U7 Drag-to-Lucy | Frontier complete, demoable |

**Metric de éxito**: al final, ningún competidor (ChatGPT desktop, Copilot,
Cursor, MS Copilot) puede responder ninguna de estas:
1. "¿Qué cambió en mi máquina desde el lunes?"
2. "¿Por qué se puso lenta hace 5 minutos?"
3. "Recuerda la última vez que arreglé esto"
4. "Muestra el árbol de procesos de ayer a las 14:30"
5. "Qué patrones extraños hay en mis procesos ahora"

Si Lucy puede contestar las 5 con confidence y audit trail, has cruzado la
frontera.

---

*Documento vivo. Cuando se aplique una feature, mover a un changelog y marcar
estado aquí.*
