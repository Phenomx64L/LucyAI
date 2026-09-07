# lucy-core

El corazón de Lucy sin Tauri y sin navegador. Es la mitad que ejecuta, mide,
recuerda y decide; la que dibuja va aparte.

Hoy lo consume una sola cara:

| Consumidor | Qué es | Ruta |
| --- | --- | --- |
| `lucy-egui` | El shell nativo. La cara de Lucy. | `../lucy-native-proto/lucy-egui` |

Hubo una segunda —`src-tauri`, la V1 en SvelteKit sobre WebView2— y ya no está.
El crate sigue sin saber dibujar y sin depender de ningún motor de navegador, que
era el punto: se puede cambiar la cara sin tocar el motor. Lo demostró irse una.

## Por qué vive solo

Vivía dentro de `lucy-svelte/`, que entonces era el repositorio de la V1. Eso hacía que
el crate que se describe como «el corazón SIN Tauri» necesitara la mitad Tauri
para existir: cuatro `include_str!` subían dos niveles y se metían en el otro
proyecto, así que no compilaba sin él. No era una dependencia que se echara de
menos en ejecución — era un error del compilador.

Y mientras el núcleo colgara de la V1, retirar la V1 significaba retirar el
núcleo. Está fuera para que se pueda apagar una cara sin apagar el motor.

## Los guardianes que miraban a la V1, y por qué ya no están

Hubo cuatro tests que leían ficheros de `../lucy-svelte` para vigilar que las dos
mitades no derivaran: el esquema de la base, el catálogo de modelos, la tabla de
precios y la delegación del deduplicador. Eran buenos mientras hubo dos copias de
cada cosa.

Todos empezaban igual:

```rust
let Ok(app) = std::fs::read_to_string(ruta_en_lucy_svelte) else { return };
```

Y aquí decía que ese `return` era «deliberado, porque el crate tiene que pasar
sus pruebas por su cuenta». Cierto — y con la V1 fuera del árbol pasó a ser la
primera línea que se ejecuta. Cuatro tests declarando verde sin comprobar nada,
ocupando el sitio de los que sí mirarían, que es la única forma en que un test
hace daño. Están fuera.

**Uno se sustituyó en vez de borrarse.** El de precios vigilaba que la tabla de
Rust no se separara de la de JavaScript; ya no hay dos tablas, pero la otra mitad
del mismo fallo sigue en pie: el menú de modelos y la tabla de precios son dos
listas en dos ficheros. Un modelo que se pueda elegir y no tenga fila de precio
hace que `cost()` devuelva `None` y esa conversación sume cero — se equivoca en
silencio, para un solo modelo. Eso lo vigila ahora
`pricing::tests::todo_modelo_que_se_ofrece_tiene_precio`, sin salir del crate.

## Probarlo

```
cargo test --all-features -- --test-threads=1
```

`--test-threads=1` no es opcional para la batería entera: el pool de conexiones
es un `OnceLock` global y hay tests que mueven estado del proceso —el directorio
de trabajo, el directorio actual—. Los ficheros que lo necesitan toman su propio
turno con un mutex, pero los que abren base de datos comparten pool.

## Mirar la memoria de una instalación de verdad

```
python tools/memoria.py
```

Los tests dicen que las piezas hacen lo que prometen. Lo que no pueden decir es
si en la máquina de alguien la memoria está *sirviendo*: si lo que se escribió
el martes vuelve el jueves, si algo ha llegado a confirmarse alguna vez, si los
patrones destilados cruzan su listón o llevan meses parados justo debajo.

Casi todo eso es invisible en la pantalla —la confianza de una fila, sus
accesos, su plazo— así que después de una tarde de pruebas lo único que se puede
decir es «parece que se acuerda», y esa frase no distingue una memoria que
funciona de una que acertó por casualidad.

La herramienta saca una foto y la compara con la anterior:

```
+ MEMORIA NUEVA  id=880  conf 0.500  caduca en 60 d  tags=["auto"]
~ id=877  Revisa la salud del sistema…
    confianza 0.613 → 0.710
    accesos 4 → 5
    plazo corrido +12.0 días
~ AUDITORÍA «auto»  0 → 3 filas
```

Abre la base en **solo lectura** y guarda la foto en el directorio temporal, no
aquí: una herramienta de diagnóstico que ensucia el árbol en cada ejecución se
deja de usar a la tercera vez.

**Los umbrales los lee de `src/`.** Es lo que hace que siga sirviendo dentro de
seis meses: un medidor con los números pegados a mano empieza a mentir en cuanto
alguien mueve una constante, y miente enseñando en verde lo que ya no lo está.
La primera versión traía `MIN_MEMORIA = 0.62` copiado de memoria cuando el valor
real era 0,65 — y así habría dibujado un listón que no existe.

Con `--base RUTA` o `LUCY_DB` se apunta a otra base; con `--limpia` se olvida la
foto y se empieza de cero.
