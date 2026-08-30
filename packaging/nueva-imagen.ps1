<#
.SYNOPSIS
    Regenera los CUATRO assets de imagen de Lucy a partir de un solo PNG.

.DESCRIPTION
    Los cuatro ficheros de imagen de Lucy son en realidad dos pares idénticos
    byte a byte:

        lucy-egui\assets\lucy-avatar.png  ═  lucy-egui\assets\lucy-icon.png
        lucy-egui\assets\lucy.ico         ═  packaging\assets\icon.ico

    QUE SEAN IDÉNTICOS ES LA TRAMPA QUE ESTE SCRIPT EVITA. Nada en el código
    obliga a que lo sigan siendo: son cuatro rutas que se leen desde cuatro
    sitios distintos —`avatar.rs` embebe uno, `main.rs` otro, `build.rs` hornea
    el .ico en el .exe y el NSIS lee el suyo—. Cambiar la imagen a mano son
    cuatro copias y una conversión, y la que se olvide no da error: sale una Lucy
    con una cara en la ventana y otra en la barra de tareas, y eso se descubre
    semanas después, en una máquina que no es la tuya.

    NO HACE FALTA IMAGEMAGICK NI NADA QUE INSTALAR. Se usa System.Drawing, que
    viene con .NET Framework en cualquier Windows.

.PARAMETER Origen
    El PNG de partida. Cuadrado y de 1024x1024 o más: de ahí se sacan todos los
    tamaños hacia abajo, y agrandar una imagen pequeña se nota justo en el
    tamaño donde más se mira.

.PARAMETER Raiz
    La carpeta de lucy-native-proto. Por defecto, la que contiene este script.

.EXAMPLE
    .\nueva-imagen.ps1 -Origen C:\Users\tu\Desktop\lucy-nueva.png

.NOTES
    Después hay que recompilar: `build.rs` mete el .ico DENTRO del .exe, así que
    sin un `cargo build` el icono del fichero sigue siendo el viejo aunque el
    .ico del disco ya sea el nuevo.

    ESTE FICHERO NECESITA BOM, igual que `lucy.nsi` y por la misma razón. Sin la
    marca de orden de bytes, PowerShell 5.1 lee un .ps1 como ANSI: los acentos
    salen como «saldrÃ¡ borrosa» y —esto es lo que de verdad rompe— una variable
    pegada a un carácter no ASCII deja de leerse. `"«$ext» no vale"` se
    interpretaba como la variable `$extÂ`, que no existe, así que el mensaje de
    error decía «"" no vale como origen» y no nombraba el fichero. Un aviso que
    no dice qué está mal es peor que no darlo.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Origen,

    [string]$Raiz = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

if (-not (Test-Path $Origen)) { throw "No encuentro «$Origen»." }

# UN .ICO DE ENTRADA FALLA CON UN ERROR QUE NO SE PUEDE LEER, y es un error fácil
# de cometer: lo natural al querer cambiar el icono es apuntar al icono que ya
# hay. GDI+ carga el .ico pero no sabe dibujarlo en un lienzo, y lo que sale es
# «Error genérico en GDI+» en la línea del `DrawImage` — que no dice ni qué
# fichero ni por qué. Se comprueba antes y se dice.
$ext = [System.IO.Path]::GetExtension($Origen).ToLowerInvariant()
if ($ext -notin '.png', '.jpg', '.jpeg', '.bmp') {
    throw "«$ext» no vale como origen. Hace falta un PNG (o JPG/BMP). Un .ico no: " +
          "GDI+ lo abre pero no lo puede redibujar, y el error que da no lo dice."
}

# Las seis capas del .ico. NO ES DECORACIÓN TENER SEIS: Windows elige la que
# más se acerca al tamaño que necesita y, si no está, escala la que haya — y una
# imagen de 256 reducida a 16 por el sistema sale con los bordes sucios. 16 es
# la barra de tareas y el Explorador en vista de lista; 256 es la vista de
# iconos grandes y el instalador.
$CAPAS = 16, 24, 32, 48, 64, 256

# El avatar del chat y el icono de ventana, que van embebidos en el binario.
$LADO_PNG = 128

function Escala {
    param([System.Drawing.Image]$Img, [int]$Lado)
    $bmp = New-Object System.Drawing.Bitmap $Lado, $Lado
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    try {
        # Alta calidad en los cuatro ajustes. Con los de por defecto, un icono de
        # 16 px sale con los bordes dentados y se nota MÁS cuanto más pequeño,
        # que es justo al revés de lo que uno esperaría.
        $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
        $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
        $g.Clear([System.Drawing.Color]::Transparent)
        $g.DrawImage($Img, (New-Object System.Drawing.Rectangle 0, 0, $Lado, $Lado))
    } finally { $g.Dispose() }
    return $bmp
}

function BytesPng {
    param([System.Drawing.Bitmap]$Bmp)
    $ms = New-Object System.IO.MemoryStream
    try {
        $Bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        # LA COMA DE DELANTE NO SOBRA. PowerShell DESENROLLA los arrays al
        # devolverlos de una función: sin ella, esto no sale como un `byte[]`
        # sino como treinta mil bytes sueltos, y lo que queda al otro lado es un
        # `Object[]` que `BinaryWriter.Write` no reconoce como buffer.
        #
        # Y no falla: escribe UN byte y sigue. El .ico salía de 108 bytes —solo
        # la cabecera y las entradas, sin una sola imagen dentro— y con un
        # tamaño declarado en cada entrada que no correspondía a nada. Un
        # fichero así no revienta al abrirlo: Windows enseña el icono en blanco.
        return , $ms.ToArray()
    } finally { $ms.Dispose() }
}

Write-Host "Origen: $Origen"
$src = [System.Drawing.Image]::FromFile((Resolve-Path $Origen))
try {
    Write-Host ("  {0} x {1}" -f $src.Width, $src.Height)
    if ($src.Width -ne $src.Height) {
        Write-Warning "No es cuadrado: se va a deformar. Recórtalo a cuadrado antes."
    }
    if ($src.Width -lt 256) {
        Write-Warning "Menos de 256 px: la capa grande del .ico saldrá borrosa."
    }

    # ── El .ico ──────────────────────────────────────────────────────────────
    #
    # Se arma a mano porque `Icon.FromHandle` solo sabe hacer iconos de UNA capa,
    # y una capa es exactamente el problema que las seis vienen a resolver.
    #
    # Las capas van con carga PNG, no BMP. Windows lo admite desde Vista y ahorra
    # unos trescientos kilobytes solo en la de 256; el formato viejo guarda cada
    # píxel sin comprimir y además exige una máscara AND que aquí no pinta nada
    # porque el alfa ya va en el canal.
    $imagenes = @()
    foreach ($lado in $CAPAS) {
        $bmp = Escala $src $lado
        try { $imagenes += , (BytesPng $bmp) } finally { $bmp.Dispose() }
    }

    $ms = New-Object System.IO.MemoryStream
    $w = New-Object System.IO.BinaryWriter $ms
    try {
        $w.Write([uint16]0)                  # reservado
        $w.Write([uint16]1)                  # 1 = icono (2 sería cursor)
        $w.Write([uint16]$CAPAS.Count)

        # El primer byte de datos va detrás de la cabecera y de las entradas.
        $offset = 6 + (16 * $CAPAS.Count)
        for ($i = 0; $i -lt $CAPAS.Count; $i++) {
            $lado = $CAPAS[$i]
            # 256 SE ESCRIBE COMO 0, y no es un truco: el campo es de un byte, así
            # que 256 no cabe. Poner 255 daría un icono de 255 px que Windows
            # escala para todo, y la capa grande —la que más se ve— saldría
            # remuestreada.
            $b = if ($lado -ge 256) { 0 } else { $lado }
            $w.Write([byte]$b)               # ancho
            $w.Write([byte]$b)               # alto
            $w.Write([byte]0)                # colores de la paleta: 0 = sin paleta
            $w.Write([byte]0)                # reservado
            $w.Write([uint16]1)              # planos
            $w.Write([uint16]32)             # bits por píxel
            $w.Write([uint32]$imagenes[$i].Length)
            $w.Write([uint32]$offset)
            $offset += $imagenes[$i].Length
        }
        # El casteo explícito, por lo mismo que la coma de `BytesPng`: si alguna
        # vez esto vuelve a llegar como `Object[]`, aquí se convierte o se rompe
        # con un error — que es mejor que escribir un byte y callarse.
        foreach ($img in $imagenes) { $w.Write([byte[]]$img) }
        $w.Flush()
        $ico = $ms.ToArray()
    } finally { $w.Dispose(); $ms.Dispose() }

    # ── Los PNG de 128 ───────────────────────────────────────────────────────
    $bmp = Escala $src $LADO_PNG
    try { $png = BytesPng $bmp } finally { $bmp.Dispose() }
}
finally { $src.Dispose() }

# ── A los cuatro sitios ──────────────────────────────────────────────────────
$destinos = @(
    @{ Ruta = "lucy-egui\assets\lucy-icon.png";   Datos = $png; Que = "icono de ventana (main.rs)" },
    @{ Ruta = "lucy-egui\assets\lucy-avatar.png"; Datos = $png; Que = "avatar del chat (avatar.rs)" },
    @{ Ruta = "lucy-egui\assets\lucy.ico";        Datos = $ico; Que = "icono del .exe (build.rs)" },
    @{ Ruta = "packaging\assets\icon.ico";        Datos = $ico; Que = "instalador NSIS y MSI" }
)

Write-Host ""
foreach ($d in $destinos) {
    $ruta = Join-Path $Raiz $d.Ruta
    $dir = Split-Path -Parent $ruta
    if (-not (Test-Path $dir)) { throw "No existe la carpeta «$dir». ¿Es -Raiz la correcta?" }
    [System.IO.File]::WriteAllBytes($ruta, $d.Datos)
    Write-Host ("  escrito  {0,-38} {1,7:N0} bytes   {2}" -f $d.Ruta, $d.Datos.Length, $d.Que)
}

Write-Host ""
Write-Host "Falta recompilar para que el icono entre en el .exe:"
Write-Host "    cargo build --release -p lucy-egui"
