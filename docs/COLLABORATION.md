# Colaboración entre dos instancias · Lucy

> Dos agentes trabajando sobre el mismo repositorio desde máquinas distintas y
> en franjas horarias distintas. Escrito v1.8.0 · 2026-07-28.
>
> Documento operativo, no aspiracional: cada regla está aquí porque su ausencia
> produce un problema concreto que se nombra.

---

## 0. La precondición que hoy no se cumple

```
main ... [origin/main: ahead 272]
```

`origin` existe (`github.com/Phenomx64L/LucyAI`) y **lleva 272 commits sin
recibir nada**. Mientras eso siga así, la segunda máquina no puede ver ni un
solo cambio: no hay colaboración posible, solo dos historias divergentes que
después habrá que reconciliar a mano.

**Nada de lo que sigue funciona sin `git push` primero.** Es el único paso
bloqueante de todo el documento.

---

## 1. El modelo: turnos, no concurrencia

Dos instancias sobre el mismo código **al mismo tiempo** es el escenario caro:
conflictos en ficheros grandes, dos refactors incompatibles, y un árbol que nadie
sabe si está verde. El modelo aquí lo evita por construcción.

| Franja | Quién | Qué hace |
|---|---|---|
| Horario laboral del operador | Instancia **A** | Trabajo acordado en el turno anterior |
| Fuera de horario | Instancia **B** | Trabajo acordado en el turno anterior |

**Una sola instancia escribe a la vez.** La otra, si corre, es de solo lectura
(análisis, revisión, informes) y **no commitea**.

No es una limitación técnica: es que el coste de reconciliar dos ramas activas
sobre un fichero de 14.000 líneas supera con creces lo que se gana paralelizando.

---

## 2. Ramas

```
main                        estable, siempre verde
claude/<turno>-<fecha>      una rama por turno, se fusiona al cerrarlo
```

Regla dura: **el turno se cierra fusionando a `main` y empujando**. Una rama que
sobrevive a su turno es una rama que la otra instancia no ve, y el turno
siguiente empieza sobre una base falsa.

Si el trabajo no cabe en un turno, se fusiona **lo que esté terminado y verde** y
el resto se describe en el traspaso. Media función fusionada es peor que ninguna;
una función completa aunque el conjunto no lo esté, no lo es.

---

## 3. El traspaso

Un único fichero vivo: **`docs/HANDOFF.md`** — no uno por fecha. El historial de
git ya guarda las versiones anteriores; una carpeta de `HANDOFF_2026-05-17.md`
obliga a adivinar cuál es el vigente.

Se reescribe **al final de cada turno**, antes del último commit:

```markdown
# Traspaso · <instancia> · <fecha y hora> · <zona horaria>

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

La última sección es la que más vale. Este proyecto ya tiene precedente de lo
contrario: `ARCHITECTURE.md` afirmaba durante versiones que los inputs de
NexShell seguían con el bug de repintado **después de estar arreglados**, y eso
puso trabajo inexistente en una hoja de ruta.

---

## 4. Reparto: qué se puede tocar en paralelo y qué no

El riesgo real no es el número de ficheros, es **cuáles**.

### Zona caliente — una sola instancia por turno

| Fichero | Por qué |
|---|---|
| `src/routes/+page.svelte` | 14.000 líneas, `runAI()` son 4.753. Dos ediciones simultáneas aquí producen conflictos irresolubles por revisión. Además tiene un **byte nulo** cerca del offset 264909: git lo trata como binario en algunos diffs, así que un conflicto no se lee bien |
| `src-tauri/src/commands/metrics.rs` | 4.832 líneas |
| `src/lib/page/slash-commands.ts` | 3.027 líneas |

### Zona fría — repartible sin coordinación

Módulos del cockpit (`src/lib/cockpit/*.svelte`), comandos de `src-tauri/src/commands/`
distintos entre sí, y cualquier módulo nuevo bajo `src/lib/`.

**Regla práctica:** si dos tareas tocan ficheros distintos de la zona fría, van en
paralelo. Si alguna toca la caliente, va sola.

---

## 5. Qué viaja entre máquinas y qué no

| Capa | Viaja | Cómo |
|---|---|---|
| Código | **Sí** | `git push` / `pull` — la única fuente de verdad |
| Traspaso | **Sí** | `docs/HANDOFF.md`, versionado con el código |
| Memoria del agente | **No** | `%USERPROFILE%\.claude\projects\<slug>\memory\` es local. Lo que deba sobrevivir al turno va al traspaso, no a la memoria |
| Sesión `.jsonl` | **No** | Historial de conversación de una máquina |
| `lucy.db` | **No** | Estado de ejecución de Lucy, no del desarrollo |
| Claves API | **No** | Credential Manager, por máquina. Cada una configura las suyas |

La segunda fila es la que se olvida: **la memoria del agente no cruza**. Si una
instancia aprende algo relevante y solo lo escribe en su memoria, la otra jamás
lo sabrá.

---

## 6. Cerrar un turno

```bash
npm run check          # 0 errores
npm test               # verde
cd src-tauri && cargo test --lib
npm run build
```

Los cuatro, no una selección. Después: actualizar `HANDOFF.md`, commitear,
fusionar a `main`, **empujar**.

Sobre el hook de pre-commit: valida el árbol desde el que se commitea —se
arregló en v1.8.0, antes validaba siempre el checkout principal y daba verde
probando código distinto del que se estaba subiendo—. Aun así, correr las
suites a mano antes de cerrar sigue siendo lo correcto: el hook comprueba, no
sustituye al juicio.

---

## 7. Abrir un turno

1. `git pull`
2. Leer `docs/HANDOFF.md` **entero**, incluida la sección de hallazgos
3. Verificar que el árbol arranca verde — si no, eso es la primera tarea y el
   traspaso mintió
4. Crear la rama del turno
5. Trabajar solo lo listado en «Lo siguiente», salvo que el operador diga otra cosa

El paso 3 no es burocracia: heredar un árbol roto y no notarlo hasta tres
commits después convierte una sesión en una investigación.

---

## 8. Lo que este modelo no resuelve

Conviene decirlo para que nadie cuente con ello:

- **No hay bloqueo real.** Dos instancias que ignoren el modelo de turnos van a
  chocar; esto es una convención, no un mecanismo.
- **No hay traspaso automático.** El `HANDOFF.md` lo escribe quien cierra. Si lo
  escribe mal, el turno siguiente empieza peor que desde cero — con información
  falsa en vez de sin información.
- **No hay memoria compartida.** Las dos instancias comparten el repositorio,
  no lo aprendido. Todo lo que importe se escribe en el repositorio.
