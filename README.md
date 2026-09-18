# Lucy — la cara nativa

Ésta es la aplicación. `lucy-egui` es Lucy: una ventana propia, sin motor de
navegador, que habla con `lucy-core` por llamadas de función y no por IPC.

El nombre del repositorio —`lucy-native-proto`— es de cuando esto era un
prototipo. Dejó de serlo en agosto de 2026 y el nombre se quedó, que es más
barato que romper los remotos de todo el mundo por una palabra.

## Correr

```powershell
cargo run -p lucy-egui --release
```

En depuración compila mucho más rápido y pinta a una fracción de los fotogramas:
hay vistas que dibujan miles de filas. Se desarrolla en depuración y se juzga el
rendimiento en release.

Lee tu base real —`%APPDATA%\com.lucy.dev\lucy.db`— y la crea si no está.

## Probar

```powershell
cargo test                      # 234, más 2 mediciones ignoradas
cargo clippy --all-targets      # 12 avisos de línea base, sin contar los dos resúmenes
cargo test cuanto_cuesta -- --ignored --nocapture
```

Las dos `#[ignore]` no son pruebas apagadas: imprimen microsegundos en vez de
afirmar nada, porque un umbral de tiempo en una batería es una prueba que falla
el día que la máquina está ocupada. Son el instrumento con el que se midieron
las dos optimizaciones de maquetado.

## Qué hay dentro

```
lucy-egui/        La aplicación.
  src/main.rs       El App, el bucle de fotograma y el estado.
  src/vista_*.rs    Una pantalla cada uno.
  src/i18n.rs       La tabla de frases en cinco idiomas.
  src/theme.rs      Los tokens de color, tipografía y movimiento.
  src/bombas.rs     Los recolectores: lo que llega de los hilos, cada fotograma.
  assets/           Icono y avatar.

proto-core/       El PTY, aparte de la interfaz. Salió de `commands/pty.rs` de
                  la V1 sin el pegamento de eventos de Tauri.

packaging/        El instalador NSIS y los cinco MSI, uno por idioma.

skills/           Cinco skills de ejemplo. Lucy las lee del perfil del usuario,
                  no de aquí — esto es de dónde copiarlas.
```

`egui-proto/` e `iced-proto/` siguen en el disco y **fuera del workspace**. Son
las dos maquetas de 190 líneas con las que se decidió el toolkit; no compilan con
el resto a propósito, porque `iced-proto` arrastra 253 crates para una decisión
que ya está tomada. Se quedan como registro.

## La propiedad que decidió todo esto

El punto de salir del WebView no era el peso, aunque el instalador pasara de
213 MB a 6,6. Era **correr donde no hay ni Edge ni GPU**: una sesión RDP contra
una máquina bloqueada, que es donde vive media administración de sistemas. Ahí
GPUI se descalificó por exigir DX11 y rechazar adaptadores por software.

No hay nada que forzar. Lucy pinta por **OpenGL y solamente por OpenGL**: enlaza
`glow` a través de eframe y no arrastra `wgpu` por ninguna rama del árbol —
comprobado con `cargo tree -p lucy-egui -i wgpu`, que no imprime nada. No hay un
camino de DX12 ni de Vulkan del que caerse, así que tampoco hay una caída que
salga mal en el sitio equivocado.

Aquí ponía `$env:WGPU_BACKEND="gl"` como forma de forzar el camino por software.
Era una orden de `egui-proto`, la maqueta del bake-off, y sobre Lucy no hace
nada: sin wgpu en el árbol, esa variable no la lee nadie.

**Y EL FPS EN REPOSO MARCA ~1, QUE ES LO CORRECTO.** El repintado va a velocidad
completa mientras hay algo que animar —tokens llegando, salida del PTY— y baja a
1 Hz cuando no lo hay. Un `request_repaint()` incondicional demostraba la
propiedad anti-congelamiento del bake-off pero fijaba un núcleo al máximo con la
ventana quieta. Lucy vive abierta todo el día, y «nativa» no puede significar
«gasta más parada que el WebView trabajando».

## Empaquetar

```powershell
cd packaging
./build-all.ps1
```

**Hoy no funciona en una máquina limpia, y es un hueco conocido.** Los scripts
buscan `makensis.exe` en `%LOCALAPPDATA%\tauri\NSIS` y WiX en
`%LOCALAPPDATA%\tauri\WixTools314` — donde los dejaba el CLI de Tauri cuando la
V1 estaba en el árbol. Sin V1 no hay quien los descargue, y `build-all.ps1`
todavía te manda conseguirlos con una orden que ya no existe.

Mientras eso siga así: o se instalan NSIS y WiX 3.14 a mano en esas rutas, o se
compila el ejecutable y se salta el instalador. Correr la aplicación no necesita
nada de esto.

## Versiones

egui/eframe **0.29** con `persistence`, y egui_commonmark **0.18**. El par se
eligió por compilar limpio, no por ser el último; las propiedades que importan
—sin WebView, sin congelarse en reposo— son idénticas en las versiones nuevas.
