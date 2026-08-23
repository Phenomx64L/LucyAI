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

# `cargo` no está en el PATH de una sesión recién abierta en esta máquina.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
}

# ── 1. la aplicación ────────────────────────────────────────────────────────
if (-not $SinCompilar) {
    Write-Host '── compilando lucy-egui ──' -ForegroundColor Cyan
    # Un binario en marcha no se puede sobrescribir: el enlazador falla con un
    # «failed to remove file» que no dice que la aplicación esté abierta.
    Get-Process lucy-egui -ErrorAction SilentlyContinue | Stop-Process -Force
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
New-Item -ItemType Directory -Force -Path $dist | Out-Null
Get-ChildItem $dist -File -ErrorAction SilentlyContinue | Remove-Item -Force

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
& $nsis /V2 $nsi
if ($LASTEXITCODE -ne 0) { throw 'makensis falló' }

# ── 3. los cinco MSI ────────────────────────────────────────────────────────
if (-not $SoloExe) {
    Write-Host '── MSI (uno por idioma) ──' -ForegroundColor Cyan
    & (Join-Path $aqui 'build-msi.ps1')
}

Write-Host "`n── listo ──" -ForegroundColor Green
Get-ChildItem $dist -File |
    Select-Object @{n = 'fichero'; e = { $_.Name } },
                  @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } } |
    Format-Table -AutoSize
Write-Host "en $dist"
