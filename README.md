# lucy-core

El corazón de Lucy sin Tauri y sin navegador. Es la mitad que ejecuta, mide,
recuerda y decide; la que dibuja va aparte.

Lo consumen los dos frentes, y ninguno es su dueño:

| Consumidor | Qué es | Ruta |
| --- | --- | --- |
| `lucy-egui` | El shell nativo. La cara actual de Lucy. | `../lucy-native-proto/lucy-egui` |
| `src-tauri` | La V1, SvelteKit + WebView2. En retirada. | `../lucy-svelte/src-tauri` |

## Por qué vive solo

Vivía dentro de `lucy-svelte/`, que es el repositorio de la V1. Eso hacía que
el crate que se describe como «el corazón SIN Tauri» necesitara la mitad Tauri
para existir: cuatro `include_str!` subían dos niveles y se metían en el otro
proyecto, así que no compilaba sin él. No era una dependencia que se echara de
menos en ejecución — era un error del compilador.

Y mientras el núcleo colgara de la V1, retirar la V1 significaba retirar el
núcleo. Está fuera para que se pueda apagar una cara sin apagar el motor.

## Los guardianes que miran a la V1

Hay cuatro tests que leen ficheros de `../lucy-svelte` para vigilar que las dos
mitades no deriven: el esquema de la base, el catálogo de modelos, la tabla de
precios y la delegación del deduplicador de memorias.

**Se saltan solos si la V1 no está al lado.** Eso es deliberado: el crate tiene
que compilar y pasar sus pruebas por su cuenta. Pero significa que en una
máquina sin la V1 esos cuatro no vigilan nada, y no lo dicen. Quien vaya a
tocar algo compartido, que los corra con los dos repositorios presentes.

## Probarlo

```
cargo test --all-features -- --test-threads=1
```

`--test-threads=1` no es opcional para la batería entera: el pool de conexiones
es un `OnceLock` global y hay tests que mueven estado del proceso —el directorio
de trabajo, el directorio actual—. Los ficheros que lo necesitan toman su propio
turno con un mutex, pero los que abren base de datos comparten pool.
