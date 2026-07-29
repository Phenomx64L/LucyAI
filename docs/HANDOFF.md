# Traspaso · instancia Windows (C:) · 2026-07-28 · v1.8.1

Lee `docs/COLLABORATION.md` antes que esto.

## Lo primero: la ruta volvió a cambiar

```
turno anterior    D:/X/Rust_Projects/lucy-svelte
este turno        C:/X/Rust_Projects/lucy-svelte
```

Ha ido `C:` → `D:` → `C:` en dos turnos. No es una anécdota: es la prueba de
que **la letra de montaje no es estable**, así que da por hecho que la tuya
será otra y comprueba antes de nada:

```bash
git rev-parse --show-toplevel
git worktree list          # ¿alguno 'prunable'?
git worktree prune
```

Este turno abrió con un worktree fantasma (`C:/Rust_Projects/…`, sin la `X`),
podado. Queda uno vivo y válido: `admiring-banach-c25e04`. **No hizo falta
`safe.directory`** — al volver a `C:` el SID coincidía otra vez.

Y otra vez: **la memoria del agente no cruzó**. El identificador del directorio
de memoria se deriva de la ruta, así que este turno arrancó de cero. Este
fichero es lo único que sobrevive.

## Estado

`main` en el commit de docs de esta serie. **Árbol limpio.**
`check` 0 · `vitest` 533 (37 ficheros) · `cargo` 418 · `build` ok · clippy
limpio con los grupos que deniega el CI.

`origin/main` **al día** — el push heredado de dos turnos se hizo con la
aprobación del operador. Ya no es un punto pendiente.

Toolchain de ESTE sistema: Node 24.18, Rust stable MSVC, WebView2.
`markitdown` **NO** está instalado, lo cual importa más de lo que parece —
ver hallazgos.

## Hecho este turno

Cinco commits, uno por área:

- `cef4f63` **seguridad** — heredado del turno anterior, sin commitear.
- `6a4c0fd` **adjuntos** — heredado, más dos cosas mías: la verificación real
  del extractor y un fallo de limpieza que encontré al escribirla.
- `7ee508e` **bucle del agente** — heredado, más los tres `ReferenceError`
  latentes que faltaban por triar.
- `4a4c9b5` **CI y cadena de suministro** — heredado.
- docs — este fichero y `ARCHITECTURE.md`.

Lo que aporté yo, sobre el trabajo heredado:

- **Verifiqué que `pdf-extract` 0.12 funciona.** `pdf.rs::tests` no ejercitaba
  el extractor **ni una vez** — solo funciones puras. El bump de seguridad son
  cinco versiones mayores sobre la ruta exacta de la que dependen los adjuntos,
  y nadie lo había probado contra un PDF real. Ahora hay 8 tests que sí.
- **Un fallo real en el fix de adjuntos**: `extract_pdf_text_from_bytes`
  prometía limpiar el temporal «en todas las rutas de salida» pero el `?` sobre
  el `JoinError` salía antes del `remove_file`. `pdf-extract` entra en pánico
  con algunos PDFs malformados, y eso dejaba los bytes del documento del
  usuario en `%TEMP%` indefinidamente.
- **12 tests de drag & drop**, que no tenía ninguno pese a ser la ruta que
  llevaba rota desde siempre fallando en silencio.
- **Triage cerrada** de los 20 `Cannot find name`: 3 reales arreglados, 2
  benignos documentados. Escaneo 236 → 228, cero introducidos.

## Lo siguiente, en orden

1. **Verificar en la app real adjuntos y bucle del agente.** Lo único sin
   comprobar end-to-end, y **exige gestos físicos**: adjuntar un PDF con el
   clip *y* arrastrándolo desde el Explorador, y pedir exportar un informe en
   el turno siguiente al que lo generó. El backend y la lógica ya están
   cubiertos por tests; lo que falta es la ventana real. Ninguna instancia
   puede hacerlo sola — necesita a una persona en el teclado.
2. **Módulo de incidentes en el cockpit** — heredado de dos traspasos. Sigue
   siendo lo único que separa de poder retirar V1.
3. **`loop_i` y `aiParams`** (opcional). Los dos benignos de la triage. Coste
   real: el mensaje de «límite de iteraciones con errores» no puede elegirse
   nunca, y el aviso de fallback puede nombrar el modelo equivocado. Arreglar
   `loop_i` obliga a hilarlo por el bucle del agente — zona caliente, decisión
   deliberada, no lo hagas de paso.

## No tocar, y por qué

- **El job `backend` del CI en `windows-latest`.** El crate es Windows-only y
  los contract tests lanzan `powershell.exe` real. Pasarlo a ubuntu lo pondría
  verde sin probar nada.
- **`src-tauri/target` sin cachear en CI.** 14,5 GB contra 10 GB de
  presupuesto por repo.
- **`npm audit fix --force`** — propone bajar SvelteKit a 0.0.30.
- **`capacity.rs`, `anomaly`, `inventory_drift`** — >1.000 líneas sin
  consumidor.
- **`audit.toml` en Linux.** Su razonamiento sobre gtk3/atk/gdk se sostiene
  *porque el único job que compila es el de Windows* — `cargo audit` solo lee
  el lockfile. Si algún día se compila en Linux, ese razonamiento hay que
  rehacerlo, no heredarlo.

## Lo que descubrí y no es obvio

- **`typeof x === 'undefined'` es seguro sobre un identificador no declarado;
  una referencia desnuda no lo es.** Y una desnuda como **primera sentencia de
  un `try`** mata todas las que vengan detrás. Así es como el bloque anti-leak
  de cerrar pestaña *tenía* un leak: `_runToken` lanzaba al entrar y los tres
  `.delete()` de debajo no se ejecutaron jamás. Es el patrón que explica los
  cinco identificadores de la triage — búscalo antes que nada.
- **El hook de pre-commit valida el ÁRBOL DE TRABAJO, no lo que está staged.**
  Consecuencia práctica al hacer commits por área: todos pasan el hook si el
  árbol está verde, aunque un commit intermedio no compilaría por separado. El
  hook no es una garantía de que cada commit sea verde en aislamiento.
- **`markitdown` no está instalado aquí, y eso cambia qué se prueba.**
  `extract_pdf_text` intenta markitdown primero. En una máquina que lo tenga,
  un test del wrapper pasaría por markitdown y **enmascararía** un
  `pdf-extract` roto. Por eso el canario del bump llama al crate
  directamente, no al wrapper.
- **Una red de regresión que nunca has visto fallar no está probada.** Los 12
  tests de drag & drop los verifiqué por mutación: un solo
  `await Promise.resolve()` de más hace fallar exactamente 3. Antes de
  existir, esa misma línea pasaba los cuatro gates en verde.
- **`cargo test` reescribe los bindings de `ts_rs`.** Cinco ficheros
  (`src-tauri/src/lib/types/*.ts`, `src/lib/types/CostDayPoint.ts`) aparecen
  como modificados en `git status` con contenido **byte-idéntico a HEAD** —
  `git diff --numstat` sale vacío. Es ruido de mtime, no trabajo. No lo cuentes
  como cambios pendientes ni lo commitees pensando que es algo.
- **El byte nulo de `+page.svelte` sobrevive a la herramienta de edición**,
  pero verifícalo: `[System.IO.File]::ReadAllBytes(...)` y cuenta nulos y
  bytes >127 antes y después. En mis ediciones los bytes altos subieron
  exactamente en las rayas y puntos suspensivos que escribí — así se distingue
  una edición limpia de una corrupción de codificación.
- **La receta de la gotcha 14 ya está escrita** en `ARCHITECTURE.md` con el
  `jsconfig.scan.json` listo para copiar. Bórralo después de usarlo: activar
  `checkJs` en el committeado pondría el CI en rojo con 228 errores.
