# Pedirle a Gemini la imagen de Lucy

Lo que sale de aquí entra por `nueva-imagen.ps1`, que lo reparte a los cuatro
sitios. Lee eso primero si no lo has hecho.

## Antes de copiar nada: las tres cosas que deciden si sirve

**Tiene que leerse a 16 píxeles.** Es el tamaño de la barra de tareas y del
Explorador en vista de lista, y es donde más veces al día se va a ver. A 16 px no
hay cara: hay tres manchas de color. Por eso lo que se pide es una **silueta**
—una forma reconocible de un vistazo— y no un retrato. El icono actual acierta en
esto: pelo turquesa, chaqueta oscura, marco. A 16 px sigue siendo «la de pelo
turquesa».

**Sin texto dentro.** El icono de hoy lleva la palabra «LUCY» abajo. Por debajo
de 48 px eso es una línea gris, y por debajo de 32 no está. Ocupa sitio que
podría usar la cara.

**Nada de fotos de personas reales.** Una foto trae derechos de imagen encima de
los de autor, y es un frente legal nuevo justo cuando el del nombre está abierto.
Una ilustración generada no tiene ese problema. Además una foto no sobrevive al
recorte cuadrado ni a los 16 px.

---

## El prompt

Pégalo tal cual. Está en inglés a propósito: los generadores de imagen entienden
notablemente mejor los términos de estilo en inglés, y «flat vector» o «rim
light» no tienen un equivalente que funcione igual de bien en español.

```text
App icon for a Windows system-administration assistant called Lucy.

SUBJECT: stylized illustrated portrait of a young woman, three-quarter view,
confident and calm expression, looking slightly off-camera. Short-to-medium
teal/cyan hair with a clean silhouette. Dark technical jacket with a high collar.
She reads as competent and technical, not cute and not sexualized.

STYLE: flat vector illustration with clean bold shapes and a limited palette.
Crisp edges, no gradients except one subtle rim light. Modern software-product
icon, in the spirit of a well-made desktop app icon. NOT photorealistic, NOT
anime, NOT 3D render, NOT painterly.

PALETTE: deep near-black background (#0A0E14). Emerald green accent (#3DD6A4)
used sparingly — a rim light on one side of the hair and shoulder, and nothing
else. Teal/cyan hair (#4CD2E8 range). Muted cool greys for the jacket. At most
four colours plus the background.

COMPOSITION: perfect square, 1024x1024. Head and shoulders fill roughly 70% of
the frame, centred, with even margin on all sides. Strong readable silhouette:
the shape of the hair and shoulders alone should be identifiable when shrunk to a
16x16 thumbnail. High contrast between the subject and the background.

DO NOT INCLUDE: any text, letters, words or numbers. No watermark, no signature.
No frame, border, HUD overlay, brackets, reticle or UI chrome around the subject.
No busy background, no particles, no circuitry, no lens flare. No hands. No
photorealism.
```

## Si lo que sale no te convence

Cambia UNA cosa cada vez y vuelve a pedirlo; cambiando tres a la vez no se sabe
cuál ayudó.

| Qué falla                              | Qué añadir al final del prompt                                  |
|----------------------------------------|-----------------------------------------------------------------|
| Se pierde a tamaño pequeño             | `Simplify further: fewer shapes, larger features, higher contrast.` |
| Demasiado infantil                     | `More serious and professional. Older, around 30. Less stylized eyes.` |
| Demasiado verde                        | `Reduce the emerald to a single thin rim light on the left edge only.` |
| Muy plana, sin profundidad             | `Add one soft shadow plane separating the hair from the jacket.` |
| No se parece a la Lucy de ahora        | Adjunta el `lucy-icon.png` actual y añade `Keep the character identity from the reference image, but restyle it as described.` |
| Sale rectangular o descentrada         | `The subject must be centred with equal margins. Square canvas.`  |

## Cuando la tengas

1. Guárdala como PNG. Cuadrada, **1024×1024 o más** — el script avisa si es
   menor, y la capa de 256 px del `.ico` se nota borrosa si la agrandas.
2. Fondo: sólido oscuro está bien. Si el generador te lo da transparente, mejor
   todavía.
3. Y pásala por el script:

```powershell
.\packaging\nueva-imagen.ps1 -Origen C:\ruta\a\tu\lucy-nueva.png
```

4. Recompila, que `build.rs` hornea el `.ico` dentro del `.exe`:

```powershell
cargo build --release -p lucy-egui
```

5. Míralo a 16 px de verdad antes de darlo por bueno: deja el `.exe` en una
   carpeta y pon el Explorador en vista de detalles. Si a ese tamaño no se
   distingue del icono genérico de Windows, no vale por bonito que sea grande.

## Lo que este script NO toca

Las dos imágenes del instalador NSIS, que son otro formato y otra proporción:

- `packaging/assets/installer-header.bmp` — la banda de arriba, 150×57 px
- `packaging/assets/installer-sidebar.bmp` — el lateral de bienvenida, 164×314 px

Son BMP y apaisada la primera, vertical la segunda: un icono cuadrado no encaja
en ninguna de las dos, así que se dejan como están hasta que haya un diseño
pensado para ese sitio.
