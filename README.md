# Lucy — Fase 0: bake-off de front nativo (egui vs iced)

Prototipo **aislado** para decidir **con evidencia** si Lucy puede dejar el WebView.
No toca `lucy-svelte` ni su git. Reescribe SOLO las **2 superficies más duras**
contra el mismo `portable-pty` que ya usa la app real:

1. **Chat con markdown en streaming** (la superficie #1 de Lucy).
2. **Terminal PTY viva** (el componente de mayor riesgo en cualquier toolkit).

## Correr

```powershell
# ── Fase 0 · bake-off (comparar toolkits) ─────────────────────────────────────
cargo run -p egui-proto --release    # egui (immediate-mode, wgpu/OpenGL nativo)
cargo run -p iced-proto --release    # iced (Elm-like, wgpu + tiny-skia CPU)

# ── Fase 1 · shell nativo egui con DATOS REALES ───────────────────────────────
# Rail + Chat + Terminal + Memoria. La vista Memoria lee tu DB REAL de Lucy
# (%APPDATA%\com.lucy.dev\lucy.db) en solo-lectura y muestra memorias reales.
cargo run -p lucy-egui --release
```

> **Fase 1 (elegido: egui).** `lucy-egui` es el primer paso de la migración real:
> una app egui nativa que ya renderiza TUS datos (memorias) sin WebView ni Tauri.
> El siguiente paso — llamar las 478 funciones del backend directo (sin IPC) —
> necesita una costura mínima en `src-tauri` (`init_with_path` + `pub mod commands`);
> se hace con tu OK porque toca la app real.

## Probar el caso LOCKED-DOWN / sin GPU / RDP (lo que de verdad importa)

El punto de salir del WebView es correr donde no hay Edge/Chrome **ni GPU**.
Fuerza el renderizado por **software** y prueba por RDP:

```powershell
# egui → backend OpenGL (o software si no hay GPU)
$env:WGPU_BACKEND="gl"; cargo run -p egui-proto --release

# iced → rasterizador CPU puro (tiny-skia) — su mayor ventaja
$env:ICED_BACKEND="tiny-skia"; cargo run -p iced-proto --release
```

Ejecuta ambos **dentro de una sesión RDP** a una máquina bloqueada representativa.
Ahí es donde GPUI (Zed) se descalificó: exige GPU DX11 y rechaza adaptadores software.

## Checklist de evaluación (puntúa 1-5 cada uno, en AMBOS)

| # | Qué mirar | egui | iced |
|---|-----------|------|------|
| 1 | **Sin congelamiento en reposo** — deja la ventana quieta; ¿sigue animando el cursor/streaming SIN mover el mouse? (el bug #1 del WebView) | | |
| 2 | **Fidelidad del markdown** — ¿se ven bien tabla, bloque de código con resaltado, lista, cita, negritas? | | |
| 3 | **Fluidez del streaming** — pulsa «▶ Streaming»; ¿el revelado token-a-token es suave? ¿FPS estable? (egui muestra FPS arriba) | | |

> **Leer el FPS en `lucy-egui`.** En reposo marca ~1, y eso es lo correcto, no un
> fallo. El repintado va a velocidad completa mientras hay algo que animar
> (tokens llegando, salida del PTY) y baja a 1 Hz cuando no lo hay — que es lo
> que la vista de Sistema necesita igualmente. Un `request_repaint()`
> incondicional demostraba la propiedad anti-congelamiento pero fijaba un núcleo
> al máximo con la ventana quieta; Lucy vive abierta todo el día, y "nativa" no
> puede significar "gasta más parada que el WebView trabajando".
>
> El punto 1 de esta tabla sigue midiendo lo mismo: durante un stream, deja la
> ventana quieta y sin tocar el ratón — debe seguir animando. Ahí `live` es
> cierto y no baja el ritmo. `egui-proto` conserva el repintado incondicional a
> propósito, como referencia del bake-off.
| 4 | **Selección / copiar en el chat** — arrastra para seleccionar texto a través de párrafos y bloques de código. (hueco CONOCIDO de egui: no selecciona entre-widgets) | | |
| 5 | **Terminal viva** — ¿aparece el prompt de PowerShell? escribe `dir`/`Get-Process` + Enter; ¿la salida llega en vivo y fluida? | | |
| 6 | **Render por software (RDP)** — repite 1-5 con el backend software de arriba. ¿Se mantiene usable? | | |
| 7 | **Sensación general** — ¿se siente como una app nativa de producto o como un demo? | | |

## Cómo leer el resultado

- Ambos **eliminan el WebView de raíz** (ventana nativa winit + GPU/CPU). El bug de
  animaciones-congeladas **no puede ocurrir** por construcción — el punto #1 es la prueba.
- El **markdown** (punto 2) y la **terminal** (punto 5) son los que deciden: si egui
  gana en fidelidad de markdown pero pierde en selección de texto (punto 4), o iced
  gana en render-CPU (punto 6) pero su terminal community-widget flaquea, eso es la
  evidencia para elegir.
- Lo que NO está aquí (a propósito, es Fase 3): emulación VT completa de la terminal
  (colores/cursor addressing), grafo de fuerza, y las 89 vistas restantes. Fase 0
  solo de-riesga las 2 superficies que definen la decisión.

## Qué se reutiliza de la app real

`proto-core/src/lib.rs` usa **el mismo `portable-pty` 0.8** que `src-tauri/src/commands/pty.rs`.
Todo tu backend Rust (47k LOC, 478 comandos) es reutilizable igual: en la migración
real dejan de ser comandos IPC y pasan a llamadas de función directas.

## Notas de versiones

- egui/eframe **0.29** + egui_commonmark **0.18** (par estable; el prototipo prioriza
  COMPILAR sobre bleeding-edge — las propiedades no-WebView/no-freeze son idénticas en 0.35).
- iced **0.13** con features `markdown` + `highlighter` (syntect) + `tokio`.

Si algún crate no resuelve versión en tu red, dime y ajusto los pines.
