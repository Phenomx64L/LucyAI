# Script para generar el reporte HTML y convertirlo a PDF usando Edge Headless
$jsonPath = "C:\Users\eleue\AppData\Local\Temp\security_audit_report.json"
$htmlPath = "C:\Users\eleue\AppData\Local\Temp\security_report.html"
$pdfPath = "C:\Users\eleue\Desktop\Informe_Seguridad_Compliance.pdf"

if (-not (Test-Path $jsonPath)) {
    Write-Error "No se encontró el archivo JSON de auditoría."
    exit 1
}

$audit = Get-Content -Raw -Path $jsonPath -Encoding utf8 | ConvertFrom-Json

# Estilos CSS basados en los Design Tokens de Lucy SOC (Oscuro, denso, profesional)
$css = @"
<style>
    @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;600;700;800&family=JetBrains+Mono:wght@400;700&display=swap');
    
    @page {
        size: A4;
        margin: 15mm;
    }
    
    body {
        font-family: 'Inter', -apple-system, sans-serif;
        background-color: #0d1117;
        color: #e2e8f0;
        margin: 0;
        padding: 0;
        font-size: 12px;
        line-height: 1.5;
    }
    
    .header {
        border-bottom: 2px solid #10b981;
        padding-bottom: 15px;
        margin-bottom: 25px;
        display: flex;
        justify-content: space-between;
        align-items: flex-end;
    }
    
    .header-title h1 {
        font-size: 22px;
        font-weight: 800;
        color: #f1f5f9;
        margin: 0;
        letter-spacing: -0.5px;
    }
    
    .header-title p {
        color: #64748b;
        margin: 5px 0 0 0;
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 1px;
    }
    
    .header-meta {
        text-align: right;
        font-family: 'JetBrains Mono', monospace;
        font-size: 11px;
        color: #64748b;
    }
    
    .header-meta span {
        color: #10b981;
        font-weight: bold;
    }
    
    .grid {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 15px;
        margin-bottom: 20px;
    }
    
    .card {
        background-color: #161b22;
        border: 1px solid #1e293b;
        border-radius: 8px;
        padding: 15px;
    }
    
    .card-full {
        grid-column: span 2;
    }
    
    .card h2 {
        font-size: 13px;
        color: #3b9eff;
        margin-top: 0;
        margin-bottom: 12px;
        border-bottom: 1px solid #1e293b;
        padding-bottom: 6px;
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }
    
    table {
        width: 100%;
        border-collapse: collapse;
        margin-top: 5px;
    }
    
    th {
        text-align: left;
        color: #64748b;
        font-size: 10px;
        text-transform: uppercase;
        padding: 6px 8px;
        border-bottom: 1px solid #1e293b;
    }
    
    td {
        padding: 6px 8px;
        border-bottom: 1px solid #161b22;
        font-size: 11px;
    }
    
    .mono {
        font-family: 'JetBrains Mono', monospace;
        font-size: 10.5px;
    }
    
    .badge {
        display: inline-block;
        padding: 2px 6px;
        border-radius: 4px;
        font-size: 9px;
        font-weight: bold;
        text-transform: uppercase;
    }
    
    .badge-pass {
        background-color: rgba(16, 185, 129, 0.15);
        color: #10b981;
        border: 1px solid rgba(16, 185, 129, 0.3);
    }
    
    .badge-fail {
        background-color: rgba(239, 68, 68, 0.15);
        color: #ef4444;
        border: 1px solid rgba(239, 68, 68, 0.3);
    }
    
    .badge-warn {
        background-color: rgba(245, 158, 11, 0.15);
        color: #f59e0b;
        border: 1px solid rgba(245, 158, 11, 0.3);
    }
    
    .summary-box {
        display: flex;
        justify-content: space-around;
        background-color: #161b22;
        border: 1px solid #1e293b;
        border-radius: 8px;
        padding: 15px;
        margin-bottom: 20px;
        text-align: center;
    }
    
    .summary-item h3 {
        margin: 0;
        font-size: 24px;
        font-weight: 800;
    }
    
    .summary-item p {
        margin: 5px 0 0 0;
        font-size: 10px;
        color: #64748b;
        text-transform: uppercase;
    }
    
    .text-success { color: #10b981; }
    .text-danger { color: #ef4444; }
    .text-warn { color: #f59e0b; }
    
    .page-break {
        page-break-before: always;
    }
</style>
"@

# Construcción del HTML dinámico
$html = @"
<!DOCTYPE html>
<html lang="es">
<head>
    <meta charset="UTF-8">
    <title>Informe de Seguridad y Compliance - $（$audit.Hostname）</title>
    $css
</head>
<body>

    <!-- PORTADA / CABECERA -->
    <div class="header">
        <div class="header-title">
            <h1>INFORME DE SEGURIDAD Y COMPLIANCE</h1>
            <p>Auditoría Interna de Endpoint | Nivel CISO</p>
        </div>
        <div class="header-meta">
            Host: <span>$($audit.Hostname)</span><br>
            SO: $($audit.OSVersion)<br>
            Fecha: $($audit.Timestamp)
        </div>
    </div>

    <!-- SUMMARY METRICS -->
    <div class="summary-box">
        <div class="summary-item">
            <h3 class="text-success">
                $( ($audit.CISBenchmark | Where-Object { $_.Status -eq "PASS" }).Count )/10
            </h3>
            <p>Controles CIS Aprobados</p>
        </div>
        <div class="summary-item">
            <h3 class="$(if ($audit.SuspiciousServices.Count -gt 0) { 'text-danger' } else { 'text-success' })">
                $($audit.SuspiciousServices.Count)
            </h3>
            <p>Servicios Sospechosos</p>
        </div>
        <div class="summary-item">
            <h3 class="text-warn">
                $($audit.InstalledSoftware.Count)
            </h3>
            <p>Soft. Instalado (7d)</p>
        </div>
        <div class="summary-item">
            <h3 class="$(if ($audit.FailedLogins24h -match 'Requiere') { 'text-warn' } elseif ($audit.FailedLogins24h -gt 5) { 'text-danger' } else { 'text-success' })">
                $($audit.FailedLogins24h)
            </h3>
            <p>Logins Fallidos (24h)</p>
        </div>
    </div>

    <div class="grid">
        <!-- SECCIÓN 1: CIS BENCHMARK -->
        <div class="card card-full">
            <h2>Veredicto CIS Benchmark (Top 10 Controles)</h2>
            <table>
                <thead>
                    <tr>
                        <th style="width: 50%;">Control de Seguridad</th>
                        <th style="width: 15%;">Estado</th>
                        <th style="width: 35%;">Detalle Técnico</th>
                    </tr>
                </thead>
                <tbody>
"@

foreach ($ctrl in $audit.CISBenchmark) {
    $statusBadge = if ($ctrl.Status -eq "PASS") { "<span class='badge badge-pass'>PASS</span>" } else { "<span class='badge badge-fail'>FAIL</span>" }
    $html += @"
                    <tr>
                        <td><strong>$($ctrl.Control)</strong></td>
                        <td>$statusBadge</td>
                        <td class="mono">$($ctrl.Details)</td>
                    </tr>
"@
}

$html += @"
                </tbody>
            </table>
        </div>

        <!-- SECCIÓN 2: SERVICIOS SOSPECHOSOS -->
        <div class="card card-full">
            <h2>Servicios Anómalos o de Terceros en Rutas Críticas</h2>
"@

if ($audit.SuspiciousServices.Count -eq 0) {
    $html += "<p style='color: #10b981; font-weight: bold; margin-top: 10px;'>No se encontraron servicios anómalos o en rutas sospechosas ejecutándose.</p>"
} else {
    $html += @"
            <table>
                <thead>
                    <tr>
                        <th>Servicio</th>
                        <th>Ruta del Ejecutable</th>
                        <th>Usuario</th>
                        <th>Criterio</th>
                    </tr>
                </thead>
                <tbody>
"@
    foreach ($svc in $audit.SuspiciousServices) {
        $html += @"
                    <tr>
                        <td><strong>$($svc.DisplayName)</strong> ($($svc.Name))</td>
                        <td class="mono" style="word-break: break-all;">$($svc.Path)</td>
                        <td class="mono">$($svc.StartName)</td>
                        <td><span class="badge badge-warn">$($svc.Reason)</span></td>
                    </tr>
"@
    }
    $html += "</tbody></table>"
}

$html += @"
        </div>
    </div> <!-- Cierre de grid de la primera página -->

    <!-- SALTO DE PÁGINA PARA LA SEGUNDA PARTE DEL REPORTE -->
    <div class="page-break"></div>

    <div class="header">
        <div class="header-title">
            <h1>INFORME DE SEGURIDAD Y COMPLIANCE</h1>
            <p>Auditoría Interna de Endpoint | Detalle Adicional</p>
        </div>
        <div class="header-meta">
            Host: <span>$($audit.Hostname)</span>
        </div>
    </div>

    <div class="grid">
        <!-- SECCIÓN 3: PUERTOS ABIERTOS -->
        <div class="card">
            <h2>Puertos Abiertos en Escucha (LISTEN)</h2>
            <table>
                <thead>
                    <tr>
                        <th>Puerto Local</th>
                        <th>Proceso</th>
                        <th>PID</th>
                    </tr>
                </thead>
                <tbody>
"@

$shownPorts = 0
foreach ($conn in $audit.OpenPorts) {
    if ($shownPorts -lt 12) { # Limitar para el reporte físico elegante
        $html += @"
                    <tr>
                        <td class="mono"><strong>$($conn.LocalPort)</strong> ($($conn.LocalAddress))</td>
                        <td>$($conn.ProcessName)</td>
                        <td class="mono">$($conn.PID)</td>
                    </tr>
"@
        $shownPorts++
    }
}
if ($audit.OpenPorts.Count -gt 12) {
    $html += "<tr><td colspan='3' style='text-align:center; color:#64748b;'>... y $($audit.OpenPorts.Count - 12) puertos más activos ...</td></tr>"
}

$html += @"
                </tbody>
            </table>
        </div>

        <!-- SECCIÓN 4: USUARIOS PRIVILEGIADOS -->
        <div class="card">
            <h2>Miembros del Grupo Administradores</h2>
            <table>
                <thead>
                    <tr>
                        <th>Usuario / Grupo</th>
                        <th>Clase</th>
                        <th>Origen</th>
                    </tr>
                </thead>
                <tbody>
"@

foreach ($usr in $audit.PrivilegedUsers) {
    $html += @"
                    <tr>
                        <td><strong>$($usr.Name)</strong></td>
                        <td>$($usr.ObjectClass)</td>
                        <td>$($usr.PrincipalSource)</td>
                    </tr>
"@
}

$html += @"
                </tbody>
            </table>
            
            <h2 style="margin-top: 20px;">Últimos Parches Aplicados (Top 5)</h2>
            <table>
                <thead>
                    <tr>
                        <th>KB ID</th>
                        <th>Descripción</th>
                        <th>Instalado el</th>
                    </tr>
                </thead>
                <tbody>
"@

foreach ($patch in $audit.Patches) {
    $html += @"
                    <tr>
                        <td class="mono" style="color: #3b9eff;"><strong>$($patch.HotFixID)</strong></td>
                        <td>$($patch.Description)</td>
                        <td class="mono">$($patch.InstalledOn)</td>
                    </tr>
"@
}

$html += @"
                </tbody>
            </table>
        </div>

        <!-- SECCIÓN 5: SOFTWARE INSTALADO RECIENTEMENTE (ÚLTIMOS 7 DÍAS) -->
        <div class="card card-full">
            <h2>Software Instalado Recientemente (Últimos 7 Días)</h2>
            <table>
                <thead>
                    <tr>
                        <th>Nombre de la Aplicación</th>
                        <th>Editor</th>
                        <th>Versión</th>
                        <th>Fecha de Inst.</th>
                    </tr>
                </thead>
                <tbody>
"@

if ($audit.InstalledSoftware.Count -eq 0) {
    $html += "<tr><td colspan='4' style='color:#64748b; text-align:center;'>No se ha detectado software nuevo instalado en los últimos 7 días.</td></tr>"
} else {
    foreach ($app in $audit.InstalledSoftware) {
        $html += @"
                    <tr>
                        <td><strong>$($app.Name)</strong></td>
                        <td>$($app.Publisher)</td>
                        <td class="mono">$($app.Version)</td>
                        <td class="mono">$($app.InstallDate)</td>
                    </tr>
"@
    }
}

$html += @"
                </tbody>
            </table>
        </div>
    </div>

</body>
</html>
"@

$html | Out-File -FilePath $htmlPath -Encoding utf8

# Imprimir HTML a PDF usando Edge Headless
$edgePath = "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
if (-not (Test-Path $edgePath)) {
    $edgePath = "C:\Program Files\Microsoft\Edge\Application\msedge.exe"
}

if (Test-Path $edgePath) {
    $args = @(
        "--headless",
        "--disable-gpu",
        "--print-to-pdf=`"$pdfPath`"",
        "--no-margins",
        "`"file:///$htmlPath`""
    )
    
    # Ejecutamos Edge Headless para la conversión
    Start-Process -FilePath $edgePath -ArgumentList $args -WindowStyle Hidden -Wait
    if (Test-Path $pdfPath) {
        Write-Output "PDF Generado con éxito en $pdfPath"
    } else {
        Write-Error "Fallo al generar el archivo PDF."
    }
} else {
    Write-Error "Microsoft Edge no se encuentra en las rutas estándar para conversión PDF."
}
