"""Los cinco .wxl: las cadenas del MSI, una por idioma.

Son POCAS a propósito. Todo lo demás del diálogo —«Siguiente», «Cancelar», el
acuerdo de licencia— lo trae WixUIExtension ya traducido; aquí solo van las
cadenas del producto, que son las que WiX no puede saber.
"""
import io
import os

DEST = r'C:\X\Rust_Projects\lucy-native-proto\packaging'
os.makedirs(DEST, exist_ok=True)

# (cultura, LCID, código de Lucy, nombre, descripción, aviso de downgrade)
IDIOMAS = [
    ('en-US', 1033, 'en', 'Lucy',
     'Lucy — Windows system administration assistant',
     'A newer version of Lucy is already installed.'),
    ('es-ES', 3082, 'es', 'Lucy',
     'Lucy — asistente de administración de sistemas Windows',
     'Ya hay instalada una versión más reciente de Lucy.'),
    ('pt-PT', 2070, 'pt', 'Lucy',
     'Lucy — assistente de administração de sistemas Windows',
     'Já está instalada uma versão mais recente da Lucy.'),
    ('fr-FR', 1036, 'fr', 'Lucy',
     "Lucy — assistant d'administration système Windows",
     'Une version plus récente de Lucy est déjà installée.'),
    ('de-DE', 1031, 'de', 'Lucy',
     'Lucy — Assistent für die Windows-Systemadministration',
     'Es ist bereits eine neuere Version von Lucy installiert.'),
]


def esc(s: str) -> str:
    return s.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;')


for cultura, lcid, _codigo, nombre, desc, downgrade in IDIOMAS:
    xml = f'''<?xml version="1.0" encoding="utf-8"?>
<WixLocalization Culture="{cultura}" Codepage="1252"
                 xmlns="http://schemas.microsoft.com/wix/2006/localization">
  <String Id="NombreProducto">{esc(nombre)}</String>
  <String Id="DescripcionPaquete">{esc(desc)}</String>
  <String Id="YaHayVersionNueva">{esc(downgrade)}</String>
</WixLocalization>
'''
    p = f'{DEST}\\lucy.{cultura}.wxl'
    io.open(p, 'w', encoding='utf-8').write(xml)
    print(f'  {cultura}  (LCID {lcid})')
print(f'\n{len(IDIOMAS)} ficheros de localización en packaging\\')
