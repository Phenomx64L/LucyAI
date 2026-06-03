# Script de Recopilación de Seguridad y Compliance para Lucy SOC
# Genera un archivo JSON con los datos de auditoría de seguridad del sistema.

$report = [ordered]@{
    Timestamp = (Get-Date).ToString("yyyy-MM-dd HH:mm:ss")
    Hostname = $env:COMPUTERNAME
    OSVersion = (Get-WmiObject Win32_OperatingSystem).Caption
    Patches = @()
    InstalledSoftware = @()
    OpenPorts = @()
    SuspiciousServices = @()
    PrivilegedUsers = @()
    FailedLogins24h = 0
    CISBenchmark = @()
}

# 1. ESTADO DE PARCHES
try {
    $patches = Get-HotFix | Sort-Object InstalledOn -Descending -ErrorAction SilentlyContinue | Select-Object -First 10
    foreach ($p in $patches) {
        $report.Patches += @{
            HotFixID = $p.HotFixID
            Description = $p.Description
            InstalledOn = if ($p.InstalledOn) { $p.InstalledOn.ToString("yyyy-MM-dd") } else { "Desconocido" }
            InstalledBy = $p.InstalledBy
        }
    }
} catch {
    $report.Patches = @{ Error = $_.Exception.Message }
}

# 2. SOFTWARE INSTALADO EN LOS ÚLTIMOS 7 DÍAS
try {
    $sevenDaysAgo = (Get-Date).AddDays(-7)
    $regPaths = @(
        "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
        "HKLM:\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*"
    )
    
    $installed = Get-ItemProperty $regPaths -ErrorAction SilentlyContinue | 
        Where-Object { $_.DisplayName -and ($_.InstallDate -or $_.PSChildName) }
        
    foreach ($app in $installed) {
        $parsedDate = $null
        if ($app.InstallDate -match '^(\d{4})(\d{2})(\d{2})$') {
            $parsedDate = [datetime]::ParseExact($app.InstallDate, "yyyyMMdd", $null)
        }
        
        if ($parsedDate -and $parsedDate -ge $sevenDaysAgo) {
            $report.InstalledSoftware += @{
                Name = $app.DisplayName
                Version = $app.DisplayVersion
                InstallDate = $parsedDate.ToString("yyyy-MM-dd")
                Publisher = $app.Publisher
            }
        }
    }
} catch {
    $report.InstalledSoftware = @{ Error = $_.Exception.Message }
}

# 3. PUERTOS ABIERTOS (LISTEN)
try {
    $connections = Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue
    $processes = Get-Process -ErrorAction SilentlyContinue | Group-Object -Property Id -AsHashTable
    
    foreach ($conn in $connections) {
        $procName = "Desconocido"
        if ($processes.ContainsKey($conn.OwningProcess)) {
            $procName = $processes[$conn.OwningProcess].ProcessName
        }
        $report.OpenPorts += @{
            LocalAddress = $conn.LocalAddress
            LocalPort = $conn.LocalPort
            PID = $conn.OwningProcess
            ProcessName = $procName
        }
    }
} catch {
    $report.OpenPorts = @{ Error = $_.Exception.Message }
}

# 4. SERVICIOS ANÓMALOS/SOSPECHOSOS
# Identificamos servicios que no pertenecen a Microsoft, están ejecutándose o automáticos, o con rutas raras.
try {
    $services = Get-WmiObject Win32_Service -Filter "StartMode='Auto' OR State='Running'" -ErrorAction SilentlyContinue
    foreach ($svc in $services) {
        $path = $svc.PathName
        $isMicrosoft = $false
        if ($path -match "System32" -or $path -match "SysWOW64" -or $path -match "Microsoft Shared" -or $svc.Publisher -match "Microsoft") {
            $isMicrosoft = $true
        }
        
        # Criterios de sospecha: No Microsoft corriendo fuera de carpetas habituales de sistema/programas, o ejecutables en Temp/AppData.
        $isSuspiciousPath = $path -match "Temp" -or $path -match "AppData" -or $path -match "Users\\Public"
        
        if (-not $isMicrosoft -and ($isSuspiciousPath -or $svc.StartName -eq "LocalSystem" -and -not $svc.AcceptStop)) {
            $report.SuspiciousServices += @{
                Name = $svc.Name
                DisplayName = $svc.DisplayName
                Path = $path
                State = $svc.State
                StartMode = $svc.StartMode
                StartName = $svc.StartName
                Reason = if ($isSuspiciousPath) { "Ruta no estándar (Temp/AppData/Public)" } else { "Servicio de terceros de alta prioridad sin detención" }
            }
        }
    }
} catch {
    $report.SuspiciousServices = @{ Error = $_.Exception.Message }
}

# 5. USUARIOS CON PRIVILEGIOS
try {
    $adminGroups = @("Administradores", "Administrators")
    foreach ($g in $adminGroups) {
        $members = Get-LocalGroupMember -Group $g -ErrorAction SilentlyContinue
        if ($members) {
            foreach ($m in $members) {
                $report.PrivilegedUsers += @{
                    Name = $m.Name
                    ObjectClass = $m.ObjectClass
                    PrincipalSource = $m.PrincipalSource
                }
            }
            break # Si encontramos miembros en uno de los idiomas, nos quedamos con ese
        }
    }
} catch {
    $report.PrivilegedUsers = @{ Error = $_.Exception.Message }
}

# 6. INTENTOS DE LOGIN FALLIDOS EN 24H
try {
    $yesterday = (Get-Date).AddDays(-1)
    # Buscamos eventos 4625 (Logon fallido) en el Security Log
    $failedEvents = Get-WinEvent -FilterHashtable @{LogName='Security'; ID=4625; StartTime=$yesterday} -ErrorAction SilentlyContinue
    $report.FailedLogins24h = if ($failedEvents) { $failedEvents.Count } else { 0 }
} catch {
    $report.FailedLogins24h = "Requiere elevación / No disponible"
}

# 7. VEREDICTO CIS BENCHMARK (TOP 10 CONTROLES)
$cis = @()

# 1. Firewall de Windows
$fw = Get-NetFirewallProfile -ErrorAction SilentlyContinue
$fwEnabled = $fw -and ($fw | Where-Object { $_.Enabled -eq $true }).Count -eq 3
$cis += @{ Control = "1. Firewall de Windows Activo (Todos los Perfiles)"; Status = if ($fwEnabled) { "PASS" } else { "FAIL" }; Details = "Perfiles activos: $(($fw | Where-Object { $_.Enabled -eq $true }).Name -join ', ')" }

# 2. UAC (User Account Control) habilitado
$uac = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -ErrorAction SilentlyContinue).ConsentPromptBehaviorAdmin
$cis += @{ Control = "2. Control de Cuentas de Usuario (UAC) Habilitado"; Status = if ($uac -and $uac -ne 0) { "PASS" } else { "FAIL" }; Details = "ConsentPromptBehaviorAdmin: $uac" }

# 3. Windows Defender Antivirus Activo
$defender = Get-Service -Name "Windefend" -ErrorAction SilentlyContinue
$cis += @{ Control = "3. Antivirus Windows Defender Activo"; Status = if ($defender -and $defender.Status -eq "Running") { "PASS" } else { "FAIL" }; Details = "Estado: $($defender.Status)" }

# 4. Cifrado BitLocker en C:
$bitlocker = Get-BitLockerVolume -MountPoint "C:" -ErrorAction SilentlyContinue
$cis += @{ Control = "4. Cifrado de Unidad BitLocker (C:)"; Status = if ($bitlocker -and $bitlocker.VolumeStatus -eq "FullyDecrypted") { "FAIL" } elseif ($bitlocker) { "PASS" } else { "FAIL" }; Details = "Estado: $(if ($bitlocker) { $bitlocker.VolumeStatus } else { 'No configurado' })" }

# 5. Deshabilitado Inicio de Sesión Automático
$autoLogon = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon" -ErrorAction SilentlyContinue).AutoAdminLogon
$cis += @{ Control = "5. Inicio de Sesión Automático Deshabilitado"; Status = if ($autoLogon -eq "1") { "FAIL" } else { "PASS" }; Details = "AutoAdminLogon: $autoLogon" }

# 6. Deshabilitado SMB v1
$smb1 = (Get-SmbServerConfiguration -ErrorAction SilentlyContinue).EnableSMB1Protocol
$cis += @{ Control = "6. Deshabilitar Protocolo Antiguo SMBv1"; Status = if ($smb1 -eq $true) { "FAIL" } else { "PASS" }; Details = "SMBv1 Habilitado: $smb1" }

# 7. Política de Ejecución de PowerShell Restringida (No Bypass global)
$execPolicy = Get-ExecutionPolicy
$cis += @{ Control = "7. Política de Ejecución de PowerShell Segura"; Status = if ($execPolicy -eq "Bypass" -or $execPolicy -eq "Unrestricted") { "FAIL" } else { "PASS" }; Details = "Execution Policy: $execPolicy" }

# 8. Cuenta de Invitado (Guest) Deshabilitada
$guest = Get-LocalUser -Name "Invitado", "Guest" -ErrorAction SilentlyContinue | Where-Object { $_.Enabled -eq $true }
$cis += @{ Control = "8. Cuenta de Invitado (Guest) Deshabilitada"; Status = if ($guest) { "FAIL" } else { "PASS" }; Details = if ($guest) { "Invitado activo" } else { "Deshabilitado" } }

# 9. Protección de LSA Habilitada (Local Security Authority)
$lsa = (Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Control\Lsa" -ErrorAction SilentlyContinue).RunAsPPL
$cis += @{ Control = "9. Protección de Autoridad de Seguridad Local (LSA)"; Status = if ($lsa -eq 1 -or $lsa -eq 2) { "PASS" } else { "FAIL" }; Details = "RunAsPPL: $lsa" }

# 10. Deshabilitación de LLMNR (Link-Local Multicast Name Resolution)
$llmnr = (Get-ItemProperty "HKLM:\Software\Policies\Microsoft\Windows NT\DNSClient" -ErrorAction SilentlyContinue).TurnOffMulticast
$cis += @{ Control = "10. Deshabilitar Resolución LLMNR (Mitigar envenenamiento)"; Status = if ($llmnr -eq 1) { "PASS" } else { "FAIL" }; Details = "TurnOffMulticast: $llmnr" }

$report.CISBenchmark = $cis

# Exportar resultados a un archivo JSON intermedio
$report | ConvertTo-Json -Depth 5 | Out-File -FilePath "C:\Users\eleue\AppData\Local\Temp\security_audit_report.json" -Encoding utf8
Write-Output "Recopilación completada correctamente."
