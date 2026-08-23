"""La licencia en RTF, que es lo único que acepta el diálogo de WiX.

Se genera en vez de guardarse a mano: así no puede quedarse desincronizada del
LICENSE de verdad, que es el que vale.
"""
import io

FUENTE = r'C:\X\Rust_Projects\lucy-svelte\LICENSE'
DESTINO = r'C:\X\Rust_Projects\lucy-native-proto\packaging\license.rtf'

texto = io.open(FUENTE, encoding='utf-8', errors='replace').read()

# RTF: las llaves y la barra son sintaxis, y lo que no sea ASCII va escapado.
cuerpo = []
for c in texto:
    if c in '\\{}':
        cuerpo.append('\\' + c)
    elif c == '\n':
        cuerpo.append('\\par\n')
    elif c == '\r':
        continue
    elif ord(c) < 128:
        cuerpo.append(c)
    else:
        cuerpo.append(f"\\u{ord(c)}?")

rtf = (
    '{\\rtf1\\ansi\\ansicpg1252\\deff0'
    '{\\fonttbl{\\f0\\fnil\\fcharset0 Segoe UI;}}'
    '\\viewkind4\\uc1\\pard\\f0\\fs18\n'
    + ''.join(cuerpo)
    + '\n}'
)
io.open(DESTINO, 'w', encoding='ascii', errors='replace').write(rtf)
print(f'{len(rtf)} bytes de RTF desde {len(texto)} de licencia')
