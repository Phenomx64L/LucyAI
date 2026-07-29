# Colaboración entre dos instancias · Lucy

> Una máquina, dos sistemas operativos en SSD distintos, uno de ellos leyendo el
> SSD donde vive Lucy. Turno de mañana (trabajo) y turno de tarde/noche
> (personal). Escrito v1.8.0 · 2026-07-28.
>
> Documento operativo, no aspiracional: cada regla está aquí porque su ausencia
> produce un problema concreto que se nombra.

---

## 1. La buena noticia: la concurrencia es imposible

Solo se arranca un sistema a la vez. Eso **elimina de raíz** el problema caro de
dos agentes sobre el mismo código: no hay conflictos simultáneos, no hay dos
ramas activas, no hay que reconciliar nada.

El modelo de turnos aquí no es una convención que alguien pueda saltarse — lo
impone el hardware. Lo que queda por resolver no es la coordinación, sino **qué
se rompe al compartir un mismo árbol de trabajo entre dos sistemas**.

Y se rompen cosas concretas.

---

## 2. Lo que se rompe, y ya hay una rota

### Worktrees de git — rutas absolutas

```
C:/X/Rust_Projects/lucy-svelte                                           ← el árbol vivo (2026-07-28, tarde)
C:/X/Rust_Projects/lucy-svelte/.claude/worktrees/admiring-banach-c25e04  ← vivo
C:/Rust_Projects/lucy-svelte/.claude/worktrees/hopeful-gauss-b9b1d6      ← podado
```

Ya ocurrió **dos veces en dos turnos**: el repositorio pasó de `C:` a `D:`, y
al turno siguiente volvió a `C:`. Cada salto invalidó los worktrees que
apuntaban a la letra anterior. Git guarda la ruta **absoluta** de cada worktree,
así que cuando el SSD se monta en otro sitio, **todos los worktrees existentes
dejan de resolver**. `git worktree prune` los limpia.

La lección no es qué letra es la buena: es que **la letra no es estable**, así
que ningún turno debe dar por buena la ruta que lea aquí — la comprueba (§3).

Efecto colateral del mismo cambio: git empezó a rechazar cada comando por
*dubious ownership* (el árbol conserva el SID del sistema que lo creó). Se
arregla con `git config --global --add safe.directory <ruta>`, y hay que
repetirlo en cada sistema que lo monte en una ruta distinta.

### El directorio de memoria del agente — derivado de la ruta

```
%USERPROFILE%\.claude\projects\D--X-Rust-Projects-lucy-svelte\memory\
                              └── esto ES D:\X\Rust_Projects\lucy-svelte
```

El identificador del proyecto se deriva de la ruta. Montar en otra letra produce
**otro directorio de memoria**, y el agente del otro turno arranca sin nada de lo
aprendido. **Esto ya pasó** al mover el repo de `C:` a `D:`: el turno siguiente
abrió con la memoria vacía, y todo lo que sabía tuvo que reconstruirse desde
`HANDOFF.md`. Es la mejor demostración de por qué ese fichero importa. Además `%USERPROFILE%` ya es distinto por sistema, así que la memoria
no se comparte de todas formas — ver §4.

### `node_modules` y `src-tauri/target/`

Ambos contienen **binarios compilados para una plataforma concreta**:
`@esbuild/win32-x64`, `@rollup/rollup-win32-x64-msvc`, y el caché de `cargo`.

- Dos Windows x64 → compatibles, se comparten sin problema.
- Uno Linux → incompatibles. Compartir el mismo directorio hace que cada
  arranque invalide el del otro, con reinstalaciones y recompilaciones completas
  cada vez.

Los dos están en `.gitignore`, así que git no los toca — pero **el sistema de
ficheros sí los comparte**, y eso es lo que importa aquí.

---

## 3. La medida que resuelve casi todo: montar en la misma ruta

**Que ambos sistemas vean el repositorio en la misma ruta absoluta.**

Esa única decisión arregla de golpe los worktrees, el identificador de memoria
del agente y cualquier configuración de editor con rutas dentro. Si un sistema
es Windows y el otro Linux no hay ruta idéntica posible, y entonces conviene
aceptar que **cada sistema tenga su propio clon** y sincronizar por git — que es
el modelo del §5.

Comprobación al abrir turno:

```bash
git rev-parse --show-toplevel     # ¿la ruta esperada?
git worktree list                 # ¿alguno 'prunable'?
git worktree prune                # limpiar los que quedaron de otra ruta
```

---

## 4. Qué se comparte y qué no

| Capa | ¿Se comparte? | Detalle |
|---|---|---|
| Código y git | **Sí** | Es el mismo SSD. No hace falta empujar para traspasar |
| `docs/HANDOFF.md` | **Sí** | Viaja con el código, por eso vive ahí |
| `node_modules`, `target/` | Sí, si ambos son Windows x64 | Si no, un clon por sistema |
| Memoria del agente | **No** | `%USERPROFILE%` distinto por sistema |
| Datos de Lucy (`lucy.db`) | **No** | `%APPDATA%\com.lucy.dev` por sistema |
| Claves API | **No** | Credential Manager por sistema |
| Toolchain (Node, Rust, MSVC) | **No** | Se instala en cada sistema |

Las cuatro últimas filas tienen una consecuencia que conviene entender bien:
**Lucy-la-aplicación tendrá dos cerebros separados.** Memorias distintas,
historial distinto, claves distintas según desde qué sistema se arranque. Se
comparte el código, no lo que Lucy ha aprendido usándolo.

Si eso importa, `lucy.db` se puede copiar entre sistemas a mano — pero nunca con
la app abierta en el otro, porque SQLite en modo WAL no perdona dos escritores.

---

## 5. Si los sistemas no pueden compartir ruta

Entonces cada uno tiene su clon y `origin` vuelve a ser la vía de traspaso:

```
main                        estable, siempre verde
claude/<turno>-<fecha>      una rama por turno, se fusiona al cerrarlo
```

Hoy eso **todavía no funciona**: `main` está **279 commits por delante de
`origin/main`** (`github.com/Phenomx64L/LucyAI`). Han hecho falta dos puertas
distintas, y conviene no confundirlas:

1. **La decisión** — empujar publica trabajo, así que requiere el visto bueno
   del operador. **Concedido el 2026-07-28.**
2. **La autenticación** — el remoto es HTTPS sin credencial guardada, y el
   entorno de agente corre con `GIT_TERMINAL_PROMPT=0`. Ninguna instancia
   puede resolver esto: hace falta una persona que autentique una vez.

Que la primera esté concedida no desbloquea la segunda. Si heredas esto, no
vuelvas a pedir permiso — pide autenticación.

Aun compartiendo SSD, empujar sigue valiendo la pena como copia de seguridad: un
SSD es un único punto de fallo para todo el historial.

---

## 6. El traspaso

Un único fichero vivo: **`docs/HANDOFF.md`** — no uno por fecha. El historial de
git ya guarda las versiones anteriores; una carpeta de `HANDOFF_2026-05-17.md`
obliga a adivinar cuál es el vigente.

Se reescribe **al final de cada turno**, antes del último commit:

```markdown
# Traspaso · <turno> · <fecha y hora>

## Estado
Rama fusionada: <sha>. Árbol limpio. check 0 · vitest N · cargo N · build ok.

## Hecho este turno
- <qué, y sobre todo POR QUÉ — el commit ya dice el qué>

## Lo siguiente, en orden
1. <tarea concreta, con el fichero y la razón>

## No tocar, y por qué
- <ficheros con trabajo a medias o decisiones pendientes del operador>

## Lo que descubrí y no es obvio
- <trampas, mediciones, cosas que costaría media jornada redescubrir>
```

La última sección es la que más vale, y es la que sustituye a la memoria del
agente — que **no cruza entre sistemas**. Lo que no se escriba aquí, el turno
siguiente no lo sabe.

Este proyecto ya tiene el contraejemplo: `ARCHITECTURE.md` afirmaba durante
versiones que los inputs de NexShell seguían con el bug de repintado **después
de estar arreglados**, y eso puso trabajo inexistente en una hoja de ruta.

---

## 7. Reparto del trabajo

Aunque no haya concurrencia, sigue importando **qué** toca cada turno: dejar un
fichero grande a medias obliga al turno siguiente a entender un refactor
incompleto antes de poder hacer nada.

### Zona caliente — terminar dentro del turno o no empezar

| Fichero | Por qué |
|---|---|
| `src/routes/+page.svelte` | 14.000 líneas, `runAI()` son 4.753. Además tiene un **byte nulo** cerca del offset 264909: git y ripgrep lo tratan como binario, así que un diff parcial no se lee bien |
| `src-tauri/src/commands/metrics.rs` | 4.832 líneas |
| `src/lib/page/slash-commands.ts` | 3.027 líneas |

### Zona fría — se puede dejar a medias sin coste

Módulos del cockpit (`src/lib/cockpit/*.svelte`), comandos distintos entre sí de
`src-tauri/src/commands/`, y cualquier módulo nuevo bajo `src/lib/`.

---

## 8. Cerrar un turno

```bash
npm run check          # 0 errores
npm test               # verde
cd src-tauri && cargo test --lib
npm run build
```

Los cuatro, no una selección. Después: actualizar `HANDOFF.md`, commitear y
fusionar a `main`.

Sobre el hook de pre-commit: valida el árbol desde el que se commitea —se
arregló en v1.8.0, antes validaba siempre el checkout principal y daba verde
probando código distinto del que se subía—. Aun así, correr las suites a mano
antes de cerrar sigue siendo lo correcto: el hook comprueba, no sustituye al
juicio.

---

## 9. Abrir un turno

1. Verificar la ruta y limpiar worktrees huérfanos (§3)
2. Leer `docs/HANDOFF.md` **entero**, incluida la sección de hallazgos
3. Verificar que el árbol arranca verde — si no, esa es la primera tarea y el
   traspaso mintió
4. Trabajar solo lo listado en «Lo siguiente», salvo indicación del operador

El paso 3 no es burocracia: heredar un árbol roto y no notarlo hasta tres
commits después convierte una sesión en una investigación.

---

## 10. Lo que este modelo no resuelve

- **La memoria del agente no cruza.** Es la limitación de fondo: cada turno
  empieza sin lo aprendido en el anterior, salvo lo escrito en el traspaso.
- **Lucy tendrá dos cerebros.** Memorias, historial y claves separados por
  sistema. Compartes el código, no la experiencia acumulada de usarlo.
- **El traspaso lo escribe quien cierra.** Si lo escribe mal, el turno siguiente
  empieza peor que desde cero — con información falsa en vez de sin información.
