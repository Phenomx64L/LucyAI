# ── version.ps1 — la versión de Lucy, de donde sale de verdad ────────────────
#
# `Cargo.toml` es la fuente. No por elegancia: es la que compila dentro del
# binario —`CARGO_PKG_VERSION`, que es lo que enseña la insignia de la interfaz—
# y la que `build.rs` mete en los datos de versión del fichero .exe. Cualquier
# otra copia es una que puede discrepar de lo que la aplicación dice de sí misma.
#
# ANTES ESTABA ESCRITA TRES VECES: aquí no, sino en `lucy.nsi`, en
# `build-msi.ps1` y en `Cargo.toml`. Olvidarse de una no da error. Da un
# instalador que anuncia una versión y registra otra, o —peor— un MSI con el
# mismo número que el que ya está instalado, y entonces Windows no lo sustituye:
# el `MajorUpgrade` de `lucy.wxs` compara números, y dos iguales no son una
# actualización. El operador instala, no falla nada, y sigue con la versión
# vieja.

$ErrorActionPreference = 'Stop'
$aqui  = Split-Path -Parent $MyInvocation.MyCommand.Path
$cargo = Join-Path (Split-Path -Parent $aqui) 'lucy-egui\Cargo.toml'

if (-not (Test-Path $cargo)) { throw "no está $cargo" }

# Solo en la sección [package] y solo hasta la primera línea de otra sección:
# más abajo hay `eframe = { version = "0.29" }` y compañía, y un patrón suelto
# se traería la versión de una dependencia sin decir nada.
$enPaquete = $false
foreach ($linea in Get-Content $cargo -Encoding UTF8) {
    $t = $linea.Trim()
    if ($t -match '^\[(.+)\]$') {
        $enPaquete = ($Matches[1] -eq 'package')
        continue
    }
    if ($enPaquete -and $t -match '^version\s*=\s*"([^"]+)"') {
        $v = $Matches[1]
        # Tres campos numéricos: es lo que exige `VIProductVersion` de NSIS al
        # añadirle el cuarto, y lo que compara el `MajorUpgrade` del MSI.
        if ($v -notmatch '^\d+\.\d+\.\d+$') {
            throw "la versión de Cargo.toml no es X.Y.Z: '$v'"
        }
        return $v
    }
}
throw "no se encontró la versión en la sección [package] de $cargo"
