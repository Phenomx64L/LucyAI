# Traspaso · instancia B (post-horario) · 2026-07-28 · v1.8.0

Primer traspaso bajo el modelo de `docs/COLLABORATION.md`. Léelo antes que esto.

## Estado

`main` en `74863ca`, árbol limpio.
`check` 0 errores · `vitest` 502 · `cargo` 408 · `build` ok.

**Bloqueante para colaborar: `main` está 272 commits por delante de
`origin/main`.** Hasta que se empuje, la otra máquina no ve nada de esto. Es el
primer paso de quien abra el siguiente turno, y requiere el visto bueno del
operador porque publica trabajo.

## Hecho en esta sesión

Contexto: se partió de un árbol con 445 ficheros borrados (restaurados desde
`c75168c`) y un toolchain inexistente en la máquina.

- **Seguridad**: limitador del gateway OpenClaw que nunca disparaba (clave por
  ip:puerto), payload de webhook entrando como rol `Sistema` sin pasar por
  guardrails, lectura de socket que truncaba peticiones.
- **Migración de `runAI()` por fases 3-5**: `AgentContext` (lecturas) y cierre
  del `AgentHost` (escrituras + la tercera parada HITL). `runAI` sigue en 4.753
  líneas — ninguna fase movió lógica, se nombraron las fronteras.
- **Sub-agentes con herramientas**: `fork_task` conducía un `ask_lucy` de un
  disparo; ahora conduce `runHeadlessAgent` con lista de permitidos de solo
  lectura. Corregido después de la primera prueba real, donde los cuatro forks
  fallaron: ahora sintetizan con lo recogido en vez de abandonarlo.
- **Cableado V1→V2**: MCP, insights proactivos, briefs de seguridad, avisos
  externos, tareas programadas, reglas de permisos. Inventario, Compliance y
  Log Viewer ahora siguen al host seleccionado (antes solo local).
- **Puente de notificaciones**: Telegram / Slack / webhook, solo salida,
  depurado de secretos y guardia SSRF. Conectado a CVE críticos, paradas HITL e
  insights proactivos.
- **Bugs de fondo**: hook de pre-commit validando el árbol equivocado bajo
  worktrees, inventario ciego a la rama de 32 bits del registro, falso positivo
  de EternalBlue en cualquier software con «SMBus», hostnames de maqueta.

## Lo siguiente, en orden

1. **`git push`** — bloqueante, ver arriba. Requiere aprobación del operador.
2. **Módulo de incidentes en el cockpit.** Es lo ÚNICO que queda entre el estado
   actual y poder retirar V1. 11 ficheros de backend (`incident.rs` 803 líneas,
   `incident_detective`, más evidencias/hipótesis). No es una lista con
   interruptores: es flujo con fases, así que necesita módulo propio en el rail,
   no un panel de Configuración.
3. **Retirar V1** — solo después del 2. Hoy retirarla quitaría funciones.

## No tocar, y por qué

- `src/routes/+page.svelte` — zona caliente. Si el turno siguiente hace el
  módulo de incidentes, tocará el rail de `CockpitShell.svelte`, no este.
- `capacity.rs`, `anomaly`, `inventory_drift` — más de 1.000 líneas de Rust
  **sin ningún consumidor, nunca ejecutadas**. Tentadoras para tendencias del
  Dashboard, pero antes hay que verificar que funcionan; es trabajo distinto del
  que parece.
- Las 6 vulnerabilidades `low` de npm restantes: **no ejecutar `npm audit fix
  --force`**. Propone bajar SvelteKit a 0.0.30 y bits-ui a 2.11.7 — destrozaría
  el build.

## Lo que descubrí y no es obvio

- **El hook de pre-commit validaba el árbol principal, no el worktree** (v1.8.0
  lo arregla). Cualquier commit hecho desde un worktree antes de ese arreglo
  pasó por una puerta que probaba otro código.
- **`ARCHITECTURE.md` puede mentir.** Afirmaba que los 5 inputs de NexShell
  seguían con el bug de repintado; se arreglaron en v1.7.221. Esa nota obsoleta
  puso trabajo inexistente en una hoja de ruta. Corregida, pero **verifica
  contra el código antes de creer a un doc**.
- **La UI puede mentir sobre el backend.** Los forks marcaban «recogido» en el
  cockpit mientras `fork_results` en SQLite mostraba que los cuatro habían
  fallado. Para verdad de fondo, consultar la DB:
  `node -e "const {DatabaseSync}=require('node:sqlite');..."`.
- **El escapado de shell mordió tres veces** (sed y perl con comillas dobles).
  Una vez corrompió literales de regex produciendo JavaScript **sintácticamente
  válido** que `svelte-check` no marca. Para sustituciones literales, usar la
  herramienta de edición, no el shell.
- **`document.hasFocus()`** es la señal de «el operador no está» para los avisos
  externos. `idle-detector.ts` parece el candidato y no lo es: solo aquieta el
  repintado.
- El `git stash` `pre-consolidacion` sigue ahí. Su contenido está íntegro en
  `main`; se puede descartar cuando el operador lo confirme.
