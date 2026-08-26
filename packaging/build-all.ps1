# ── build-all.ps1 — de cero a instaladores, en un comando ───────────────────
#
# `cargo build` produce la APLICACIÓN (`target\release\lucy-egui.exe`), no los
# instaladores. Son tres pasos y es fácil pararse en el primero, sobre todo
# porque el binario y el instalador son los dos un `.exe` y se parecen en el
# nombre. Este guion hace los tres y dice qué salió.
#
#   .\packaging\build-all.ps1              todo
#   .\packaging\build-all.ps1 -SoloExe     solo el instalador NSIS (el rápido)
#   .\packaging\build-all.ps1 -SinCompilar reempaqueta sin volver a compilar

param(
    [switch]$SoloExe,
    [switch]$SinCompilar
)

$ErrorActionPreference = 'Stop'
$aqui = Split-Path -Parent $MyInvocation.MyCommand.Path
$raiz = Split-Path -Parent $aqui
$dist = Join-Path $raiz 'dist'

# UNA SOLA VERSIÓN, y sale de `Cargo.toml` — la misma que compila dentro del
# binario. Ver `version.ps1`: estaba escrita a mano en tres sitios.
$version = & (Join-Path $aqui 'version.ps1')
Write-Host "── Lucy $version ──" -ForegroundColor Cyan

# El binario que ESTE guion produce. Se necesita antes de compilar para saber
# qué proceso estorba de verdad y cuál solo comparte nombre.
$exeEsperado = Join-Path $raiz 'target\release\lucy-egui.exe'

# `cargo` no está en el PATH de una sesión recién abierta en esta máquina.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
}

# ── 1. la aplicación ────────────────────────────────────────────────────────
if (-not $SinCompilar) {
    Write-Host '── compilando lucy-egui ──' -ForegroundColor Cyan
    # Un binario en marcha no se puede sobrescribir: el enlazador falla con un
    # «failed to remove file» que no dice que la aplicación esté abierta.
    #
    # SOLO EL DEL TARGET, Y SIN MORIR SI NO SE PUEDE. Esto cerraba CUALQUIER
    # `lucy-egui`, incluida la Lucy INSTALADA que el operador tenga abierta
    # trabajando — que no bloquea nada, porque el enlazador escribe en
    # `target\release` y ésa vive en Archivos de programa. Y si estaba elevada,
    # `Stop-Process` fallaba con «Acceso denegado» y, con
    # `ErrorActionPreference = Stop`, se llevaba por delante la compilación
    # entera. Se perdía la build por cerrar un proceso que no estorbaba.
    $enMarcha = Get-Process lucy-egui -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $exeEsperado }
    foreach ($p in $enMarcha) {
        try {
            Stop-Process -Id $p.Id -Force -ErrorAction Stop
            Write-Host "   cerrado lucy-egui ($($p.Id)) para poder enlazar" -ForegroundColor DarkGray
        } catch {
            throw "hay un lucy-egui ($($p.Id)) corriendo desde el target y no se pudo cerrar. Ciérralo a mano."
        }
    }
    Push-Location $raiz
    try {
        cargo build -p lucy-egui --release
        if ($LASTEXITCODE -ne 0) { throw 'cargo build falló' }
    } finally { Pop-Location }
}

$exe = Join-Path $raiz 'target\release\lucy-egui.exe'
if (-not (Test-Path $exe)) { throw "no está el binario: $exe" }
$mbExe = [math]::Round((Get-Item $exe).Length / 1MB, 1)
Write-Host "   aplicación: $mbExe MB" -ForegroundColor DarkGray

# Se limpia `dist` para que no queden instaladores de una versión anterior al
# lado de los nuevos. Dos versiones en la misma carpeta es como se acaba
# instalando la vieja sin darse cuenta.
#
# SE AVISA Y SE SIGUE SI NO SE PUEDE BORRAR. El antivirus abre los .msi recién
# escritos para escanearlos, y mientras los tiene, `Remove-Item` falla con
# «Acceso denegado». Con `ErrorActionPreference = Stop` eso mataba la
# compilación entera DESPUÉS de haber compilado — se perdía el trabajo por no
# poder borrar un fichero que `light.exe` iba a sobrescribir de todos modos.
#
# Lo que de verdad importa —que no quede un instalador de OTRA versión al
# lado— se comprueba al final, sobre lo que haya quedado.
New-Item -ItemType Directory -Force -Path $dist | Out-Null
foreach ($f in Get-ChildItem $dist -File -ErrorAction SilentlyContinue) {
    try {
        Remove-Item $f.FullName -Force -ErrorAction Stop
    } catch {
        Write-Host "   ! no se pudo borrar $($f.Name) (¿antivirus?); se sobrescribe" -ForegroundColor DarkYellow
    }
}

# ── 2. el instalador con selector de idioma ─────────────────────────────────
Write-Host '── NSIS (con selector de idioma) ──' -ForegroundColor Cyan

# EL .nsi TIENE QUE LLEVAR BOM. Con `Unicode true`, NSIS lee un fichero sin
# marca de orden de bytes como ANSI, y «Iván Eduardo Luna» se registra en
# Windows como «IvÃ¡n Eduardo Luna». No falla nada al compilar: el instalador
# sale, se instala, y el nombre roto solo se ve en «Aplicaciones instaladas»,
# que es donde nadie vuelve a mirar. Por eso se comprueba aquí y no se confía.
$nsi = Join-Path $aqui 'lucy.nsi'
$primeros = [System.IO.File]::ReadAllBytes($nsi)[0..2]
if ($primeros[0] -ne 0xEF -or $primeros[1] -ne 0xBB -or $primeros[2] -ne 0xBF) {
    throw "lucy.nsi no tiene BOM: los acentos se registrarian mal. Guardalo como UTF-8 con BOM."
}

$nsis = "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe"
if (-not (Test-Path $nsis)) {
    # Sin comilla invertida en el mensaje: en PowerShell es el carácter de
    # escape y parte la cadena por la mitad.
    throw "falta makensis en $nsis - lo descarga 'npm run tauri build' en lucy-svelte"
}
& $nsis /V2 "/DVERSION=$version" $nsi
if ($LASTEXITCODE -ne 0) { throw 'makensis falló' }

# ── 3. los cinco MSI ────────────────────────────────────────────────────────
if (-not $SoloExe) {
    Write-Host '── MSI (uno por idioma) ──' -ForegroundColor Cyan
    & (Join-Path $aqui 'build-msi.ps1') -Version $version
}

# NINGÚN INSTALADOR DE OTRA VERSIÓN AL LADO. Es lo que la limpieza de arriba
# venía a garantizar, y como ahora puede fallar sin parar la compilación, hay
# que comprobarlo de verdad: dos versiones en la misma carpeta es como se acaba
# instalando la vieja sin darse cuenta.
$intrusos = Get-ChildItem $dist -File | Where-Object { $_.Name -notmatch [regex]::Escape($version) }
if ($intrusos) {
    Write-Host "`n! quedan instaladores de otra versión en dist:" -ForegroundColor Red
    $intrusos | ForEach-Object { Write-Host "    $($_.Name)" -ForegroundColor Red }
    Write-Host "  Bórralos a mano antes de repartir nada." -ForegroundColor Red
}

# Y NINGUNO MÁS VIEJO QUE EL BINARIO QUE DEBERÍA LLEVAR DENTRO.
#
# Un instalador con el número de versión correcto y los bits de hace media hora
# es peor que uno con el número mal: el número cuadra, así que nadie lo mira. Se
# reparte, se instala, y el fallo que se acababa de arreglar sigue ahí.
#
# Pasó de verdad: los .msi quedaron de una ejecución ELEVADA y su dueño era
# BUILTIN\Administradores, así que ni `Remove-Item` ni `light.exe` podían
# sobrescribirlos desde una sesión normal. El .exe se regeneró y los .msi no, y
# lo único que lo delataba era la hora.
$viejos = Get-ChildItem $dist -File |
    Where-Object { $_.LastWriteTime -lt (Get-Item $exe).LastWriteTime }
if ($viejos) {
    Write-Host "`n! estos instaladores son ANTERIORES al binario y no llevan los cambios:" -ForegroundColor Red
    $viejos | ForEach-Object {
        $duenio = (Get-Acl $_.FullName).Owner
        Write-Host "    $($_.Name)  ($($_.LastWriteTime.ToString('HH:mm:ss')) · dueño: $duenio)" -ForegroundColor Red
    }
    Write-Host "  Si el dueño no eres tú, los escribió una sesión elevada:" -ForegroundColor Red
    Write-Host "  borra la carpeta dist desde una consola de administrador y repite." -ForegroundColor Red
}

Write-Host "`n── listo ──" -ForegroundColor Green
Get-ChildItem $dist -File |
    Select-Object @{n = 'fichero'; e = { $_.Name } },
                  @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } } |
    Format-Table -AutoSize
Write-Host "en $dist"
