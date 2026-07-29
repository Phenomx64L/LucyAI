# Traspaso · instancia Windows (C:) · 2026-07-28 · v1.8.1

Lee `docs/COLLABORATION.md` antes que esto.

## Lo primero: la ruta del repositorio no es estable

```
turno anterior    D:/X/Rust_Projects/lucy-svelte
este turno        C:/X/Rust_Projects/lucy-svelte
```

`C:` → `D:` → `C:` en dos turnos. La lección no es cuál es la letra buena: es
que **no hay letra buena**. Comprueba antes de nada:

```bash
git rev-parse --show-toplevel
git worktree list          # ¿alguno 'prunable'?
git worktree prune
```

Este turno abrió con un worktree fantasma (`C:/Rust_Projects/…`, sin la `X`),
podado. Queda uno vivo: `admiring-banach-c25e04`. **No hizo falta
`safe.directory`** — al volver a `C:` el SID coincidía otra vez.

Y otra vez: **la memoria del agente no cruzó**. El directorio de memoria se
deriva de la ruta, así que este turno arrancó de cero. Este fichero es lo único
que sobrevive.

## Estado

**Árbol limpio.** `check` 0 · `vitest` 544 (38 ficheros) · `cargo` 429 ·
`build` ok · clippy limpio con los grupos que deniega el CI.

`main` va **~287 commits por delante de `origin/main`**, en fast-forward limpio
(0 por detrás). **El push está APROBADO por el operador** desde este turno, pero
falla por autenticación:

```
fatal: could not read Username for 'https://github.com'
```

No hay credencial de GitHub en el Credential Manager y el entorno de agente
corre con `GIT_TERMINAL_PROMPT=0`. **Ninguna instancia puede resolverlo sola.**
Si heredas esto, **no vuelvas a pedir permiso — pide autenticación**
(`gh auth login`, o un push manual que guarde la credencial).

`markitdown` **NO** está instalado en este sistema, y eso cambia qué se prueba
— ver hallazgos. Toolchain: Node 24.18, Rust stable MSVC, WebView2.

## Hecho este turno

**Primera mitad — cerrar el turno anterior.** Commiteados por área los 20
ficheros heredados (`cef4f63` seguridad · `6a4c0fd` adjuntos · `7ee508e` bucle
del agente · `4a4c9b5` CI · `570ef75` docs), más de mi parte:

- **Verificado que `pdf-extract` 0.12 funciona.** `pdf.rs::tests` no ejercitaba
  el extractor **ni una vez** — solo funciones puras — pese a que el bump de
  seguridad son cinco versiones mayores sobre la ruta de la que dependen los
  adjuntos. Ahora hay 8 tests que sí.
- **Un fallo real en el fix de adjuntos**: `extract_pdf_text_from_bytes` decía
  limpiar el temporal «en todas las rutas de salida», pero el `?` sobre el
  `JoinError` salía antes del `remove_file`. `pdf-extract` entra en pánico con
  algunos PDFs malformados → los bytes del documento del usuario se quedaban en
  `%TEMP%`.
- **12 tests de drag & drop**, que no tenía ninguno.
- **Triage cerrada** de los 20 `Cannot find name`: 3 reales, 2 benignos.

**Segunda mitad — validación del Dashboard**, a petición del operador:

- `09a6358` **El log de seguridad mentía.** Decía «Registro de seguridad no
  legible» cuando el log se leía perfectamente y había **cero** logins fallidos
  — el mejor resultado posible, pintado como avería.
- `ddee9db` + `2f5ef20` **Servicios detenidos.** 6 falsos positivos que además
  degradaban el equipo entero a «Atención» un minuto después de cada arranque.
- `a5ece27` **Red, uptime y núcleos.** Tres valores medidos bien y destruidos al
  mostrarlos.
- `7cd5635` **Auditoría de codificación** en los 64 `from_utf8_lossy`.
- `0481bdc` La trampa, documentada como **gotcha 15**.

## Lo siguiente, en orden

1. **Autenticar y empujar.** Aprobación ya dada; falta la credencial.
2. **Decodificar las herramientas nativas de `local.rs`.** La mitad pendiente de
   la auditoría: `tasklist` y `netstat` **medidos** produciendo `U+FFFD`, más
   `wevtutil`, `wmic`, `reg`, `cmd`, `netsh`, `cscript`. Necesita
   `MultiByteToWideChar` + `CP_OEMCP` vía `winapi` (ya es dependencia).
   **`encoding_rs` está en el lockfile pero NO sirve** — implementa el conjunto
   WHATWG y no cubre CP-850. No pierdas media hora ahí como estuve a punto.
3. **El camino WinRM de `utils/shell.rs`**, misma exposición. Lo dejé sin tocar
   a propósito: pasa una credencial por stdin bajo un fix estructural explícito
   y aquí no hay host WinRM contra el que verificar.
4. **Verificar adjuntos y bucle del agente en la app real.** Exige gestos
   físicos: adjuntar un PDF con el clip *y* arrastrándolo, y pedir exportar un
   informe en el turno siguiente al que lo generó. El backend y la lógica ya
   tienen red; falta la ventana. Ninguna instancia puede hacerlo sola.
5. **Módulo de incidentes en el cockpit** — heredado de tres traspasos. Sigue
   siendo lo único que separa de poder retirar V1.
6. **`loop_i` y `aiParams`** (opcional). Los dos benignos de la triage. Coste
   real: el mensaje de «límite de iteraciones con errores» no puede elegirse
   nunca, y el aviso de fallback puede nombrar el modelo equivocado. Arreglar
   `loop_i` obliga a hilarlo por el bucle del agente — zona caliente.

## No tocar, y por qué

- **El job `backend` del CI en `windows-latest`.** El crate es Windows-only y
  los contract tests lanzan `powershell.exe` real. Pasarlo a ubuntu lo pondría
  verde sin probar nada.
- **`src-tauri/target` sin cachear en CI.** 14,5 GB contra 10 GB de presupuesto.
- **`npm audit fix --force`** — propone bajar SvelteKit a 0.0.30.
- **`capacity.rs`, `anomaly`, `inventory_drift`** — >1.000 líneas sin consumidor.
- **`audit.toml` en Linux.** Su razonamiento sobre gtk3/atk/gdk se sostiene
  *porque el único job que compila es el de Windows*; `cargo audit` solo lee el
  lockfile. Si algún día se compila en Linux, rehaz ese razonamiento.

## Lo que descubrí y no es obvio

- **PowerShell miente en dos idiomas a la vez.** Nunca ramifiques sobre
  `$_.Exception.Message`: Windows lo traduce. `Get-WinEvent` **lanza excepción**
  cuando no encuentra nada en vez de devolver 0, así que el caso «todo bien»
  llega como error y había que distinguirlo… comparando texto en inglés. Usa
  `$_.FullyQualifiedErrorId`. Y la salida de un PowerShell lanzado desde un
  proceso GUI viene en **CP-850**, no UTF-8. Gotcha 15 tiene la receta completa.
- **Todos los spawns artesanales de PowerShell estaban mal.** No la mayoría:
  todos. Solo `shell.rs` estaba bien, y por casualidad. El fallo es silencioso —
  el texto corrupto sigue siendo JSON válido y parsea.
- **`typeof x === 'undefined'` es seguro sobre un identificador no declarado;
  una referencia desnuda no lo es.** Y una desnuda como **primera sentencia de
  un `try`** mata todas las de detrás. Así el bloque anti-leak de cerrar pestaña
  *tenía* un leak.
- **Una red de regresión que nunca has visto fallar no está probada.** Muté las
  dos que escribí (drag & drop y el guard de spawns) para verlas fallar. La de
  drag & drop cae con un solo `await Promise.resolve()` de más — que antes
  pasaba los cuatro gates en verde.
- **El patrón de fondo del Dashboard: trataba «no lo entiendo» como «hay un
  problema».** Log vacío → avería. Servicio que debe estar parado → alerta.
  Valor medido bien → redondeado a cero. Un panel que grita en cada arranque
  enseña al operador a no leerlo, que es lo contrario de para lo que existe.
- **Mido antes de afirmar, y me corrigió.** Sospeché que
  `Networks::new_with_refreshed_list()` rompía el cálculo de red por crear
  instancia nueva. Falso: en Windows `total_received()` devuelve `InOctets`, el
  contador del SO desde el arranque. El bug estaba en el `.toFixed(1)` final.
- **`markitdown` no está instalado aquí, y eso cambia qué se prueba.**
  `extract_pdf_text` lo intenta primero. En una máquina que lo tenga, un test
  del wrapper pasaría por markitdown y **enmascararía** un `pdf-extract` roto.
  Por eso el canario del bump llama al crate directamente.
- **El hook de pre-commit valida el ÁRBOL DE TRABAJO, no lo staged.** Al hacer
  commits por área todos pasan si el árbol está verde, aunque un commit
  intermedio no compilara por separado.
- **`cargo test` reescribe los bindings de `ts_rs`.** Cinco ficheros aparecen
  como modificados en `git status` con contenido **byte-idéntico a HEAD**
  (`git diff --numstat` vacío). Es ruido de mtime, no trabajo pendiente.
- **El byte nulo de `+page.svelte` sobrevive a la herramienta de edición**, pero
  verifícalo: cuenta nulos y bytes >127 antes y después con
  `[System.IO.File]::ReadAllBytes`. En mis ediciones los bytes altos subieron
  exactamente en las rayas que escribí — así distingues edición limpia de
  corrupción.
- **Al reproducir un problema de codificación, compara BYTES, no texto
  renderizado.** Tu terminal miente en ambas direcciones: me mostró `gr�ficos`
  para una «á» ya correcta. `chcp 850` + volcado de bytes es la única medida
  fiable (`0xA2` roto vs `0xC3 0xB3` bien).
