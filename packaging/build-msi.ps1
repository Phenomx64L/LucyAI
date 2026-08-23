# ── build-msi.ps1 — los cinco MSI del shell nativo de Lucy ───────────────────
#
# UNO POR IDIOMA. Un MSI lleva su idioma horneado en la cabecera del paquete;
# un desplegable dentro exige transformaciones .mst y un arrancador que las
# aplique. El selector de idioma vive en el instalador NSIS — ver `lucy.nsi`.
#
# Usa el WiX que ya descargó Tauri en `%LOCALAPPDATA%\tauri\WixTools314`, para
# no pedir una instalación aparte de una herramienta que la máquina ya tiene.

$ErrorActionPreference = 'Stop'
$aqui    = Split-Path -Parent $MyInvocation.MyCommand.Path
$raiz    = Split-Path -Parent $aqui
$wix     = "$env:LOCALAPPDATA\tauri\WixTools314"
$version = '2.0.1'

$exe     = Join-Path $raiz 'target\release\lucy-egui.exe'
$icono   = Join-Path (Split-Path -Parent $raiz) 'lucy-svelte\src-tauri\icons\icon.ico'
$licencia= Join-Path (Split-Path -Parent $raiz) 'lucy-svelte\LICENSE'
$rtf     = Join-Path $aqui 'license.rtf'
$dist    = Join-Path $raiz 'dist'
$obj     = Join-Path $aqui 'obj'

foreach ($f in @($exe, $icono, $licencia, $rtf)) {
    if (-not (Test-Path $f)) { throw "falta: $f" }
}
New-Item -ItemType Directory -Force -Path $dist, $obj | Out-Null

# (cultura, LCID, código que entiende Lucy)
$idiomas = @(
    @('en-US', 1033, 'en'),
    @('es-ES', 3082, 'es'),
    @('pt-PT', 2070, 'pt'),
    @('fr-FR', 1036, 'fr'),
    @('de-DE', 1031, 'de')
)

foreach ($i in $idiomas) {
    $cultura, $lcid, $codigo = $i
    $wixobj = Join-Path $obj "lucy.$cultura.wixobj"
    $msi    = Join-Path $dist "Lucy_${version}_x64_$cultura.msi"

    & "$wix\candle.exe" -nologo -arch x64 `
        -dVersion="$version" -dLang="$lcid" -dCodigoIdioma="$codigo" `
        -dExeRuta="$exe" -dIconoRuta="$icono" `
        -dLicenciaRuta="$licencia" -dRtfRuta="$rtf" `
        -out $wixobj (Join-Path $aqui 'lucy.wxs')
    if ($LASTEXITCODE -ne 0) { throw "candle fallo en $cultura" }

    # `-spdb`: sin el .wixpdb. Son símbolos para depurar el paquete, no algo
    # que se entregue, y dejarlos en `dist` junto a los MSI invita a subirlos.
    & "$wix\light.exe" -nologo -sval -spdb `
        -ext "$wix\WixUIExtension.dll" `
        -cultures:$cultura -loc (Join-Path $aqui "lucy.$cultura.wxl") `
        -out $msi $wixobj
    if ($LASTEXITCODE -ne 0) { throw "light fallo en $cultura" }

    $mb = [math]::Round((Get-Item $msi).Length / 1MB, 1)
    Write-Host "  $cultura -> $($msi | Split-Path -Leaf)  ($mb MB)"
}
Write-Host "`n$($idiomas.Count) MSI en $dist"
