<p align="center">
  <img src="icon.png" alt="Lucy" width="120" />
</p>

<h1 align="center">Lucy</h1>

<p align="center">
  <strong>Asistente de SysAdmin para Windows</strong><br>
  Interfaz nativa. Sin navegador embebido.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/versión-2.1.0-7dd3fc" alt="v2.1.0" />
  <img src="https://img.shields.io/badge/egui-0.29-blue" alt="egui 0.29" />
  <img src="https://img.shields.io/badge/Rust-2021-brown?logo=rust" alt="Rust 2021" />
  <img src="https://img.shields.io/badge/licencia-GPLv3-green" alt="GPLv3" />
</p>

---

## Qué es

Lucy administra equipos Windows: mira cómo están, ejecuta lo que le pidas en
español, audita el cumplimiento CIS, lee registros de eventos, hace inventario y
recuerda lo que ha ido aprendiendo de cada máquina. Habla con modelos locales
(Ollama) o de nube, y en modo privado no sale nada del equipo.

## Por qué esta versión existe

**19,6 MB contra 213 MB.** Ésa es toda la migración en una línea.

La V1 era Tauri 2 + SvelteKit sobre WebView2. Funcionaba, y arrastraba un motor
de navegador entero para pintar una rejilla de tarjetas y una tabla de procesos.
Esta versión pinta lo mismo con [`egui`](https://github.com/emilk/egui) en un
único ejecutable que no enlaza ningún navegador.

El instalador anterior pesaba 213 MB. Éste pesa 19,6 MB, y la diferencia es
exactamente el motor que ya no está.

## Cómo está repartido

```
lucy-core/            El corazón compartido. Sin interfaz y sin Tauri: memoria,
                      consolidación, patrones, cumplimiento, inventario,
                      vigilancia, notificaciones, gasto de tokens.

lucy-native-proto/    La cara nativa.
  lucy-egui/            El shell de egui: los ocho módulos y la tabla de idiomas.
  packaging/            El instalador NSIS y el MSI.

docs/security-skills/ Catálogo de skills de seguridad y forense, con su propia
                      licencia y atribución. Lucy los lee de la carpeta de
                      usuario, no de aquí — esto es de dónde copiarlos.

docs/research/        Las notas de diseño de la memoria y el grafo.
```

Los dos proyectos entraron por `git subtree`, así que su historia completa está
en el registro: `git log -- lucy-core` cuenta cómo se llegó a cada pieza.

Cada uno sigue teniendo su repositorio de trabajo, y su rama aquí:

| directorio          | rama     |
| ------------------- | -------- |
| `lucy-core`         | `nucleo` |
| `lucy-native-proto` | `egui`   |

Para traer a `main` lo que se haya empujado a una de ellas:

```bash
git subtree pull --prefix=lucy-core origin nucleo
```

## Los ocho módulos

| Módulo            | Qué hace                                                        |
| ----------------- | --------------------------------------------------------------- |
| **Dashboard**     | CPU, RAM, discos, red, servicios caídos y tendencia del historial |
| **Terminal IA**   | Le pides las cosas en español; propone el comando y lo ejecuta si lo apruebas |
| **NexShell**      | Una PowerShell de verdad, local o remota por WinRM                |
| **Log Viewer**    | Qué se ha ejecutado, con qué resultado y cuánto tardó             |
| **Inventario**    | Puertos, servicios, software instalado, certificados              |
| **Compliance**    | Controles CIS, con la evidencia de cada veredicto                 |
| **Memoria**       | Hechos, sesiones destiladas, manuales ingeridos y principios      |
| **Configuración** | Claves, modelos, umbrales, idioma y aspecto                       |

## Idiomas

Español, inglés, portugués, francés y alemán. El texto en español **es la
clave** de la tabla de traducción, que se busca por búsqueda binaria — hay un
test que impide que la tabla se desordene, porque una tabla desordenada no
falla: simplemente deja de encontrar la mitad de las frases.

## Compilar

Hace falta Rust estable. Windows 10/11.

```bash
cargo run --release --manifest-path lucy-native-proto/lucy-egui/Cargo.toml
```

Las pruebas de cada mitad:

```bash
cargo test --manifest-path lucy-core/Cargo.toml
```

574 aserciones en el núcleo y 201 en el shell, todas verdes.

## Dónde está la V1

Entera, en este mismo repositorio. No se ha borrado nada de la historia:

- La etiqueta **`v1-svelte-final`** apunta a su último árbol completo.
- Las 48 etiquetas `v1.x` marcan cada versión publicada.
- `git show v1-svelte-final:src-tauri/src/main.rs` sigue funcionando.

Lo que se retiró de `main` fue el código y su andamiaje de compilación — no su
registro.

---

## Autor

**Iván Eduardo Luna** ([@Phenomx64L](https://github.com/Phenomx64L))
· [LinkedIn](https://linkedin.com/in/phenomx64l)
· SysAdmin y desarrollador

Lucy nació de problemas reales de administración de infraestructura. Cada
decisión de arquitectura viene de haberlos tenido delante.

## Licencia

GPLv3. Ver [LICENSE](LICENSE).

El catálogo de `docs/security-skills/` tiene su propia licencia y atribución;
ver [`docs/security-skills/ATTRIBUTION.md`](docs/security-skills/ATTRIBUTION.md).
