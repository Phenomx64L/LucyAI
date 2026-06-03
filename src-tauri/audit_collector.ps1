$Report = [PSCustomObject]@{
    Services = @()
    Tasks = @()
    Ports = @()
    BitLocker = @()
    Updates = @()
    Logins = @()
    Elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# 1. Servicios LocalSystem no firmados
try {
    $services = Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | Where-Object {$_.StartName -eq 'LocalSystem' -and $_.State -eq 'Running'}
    foreach ($svc in $services) {
        $rawPath = $svc.PathName
        if ($rawPath) {
            if ($rawPath -match '^"([^"]+)"') {
                $binPath = $Matches[1]
            } else {
                $binPath = $rawPath.Split(' ')[0]
            }
            if (Test-Path $binPath) {
                $sig = Get-AuthenticodeSignature -FilePath $binPath -ErrorAction SilentlyContinue
                if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
                    $Report.Services += [PSCustomObject]@{
                        Name = $svc.Name
                        DisplayName = $svc.DisplayName
                        Path = $binPath
                        Publisher = if ($sig.SignerCertificate.Subject) { $sig.SignerCertificate.Subject } else { "Unsigned" }
                        Status = $sig.Status
                    }
                }
            }
        }
    }
} catch {}

# 2. Tareas programadas elevadas (limitar para no exceder tamaño)
try {
    $tasks = Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object {$_.Principal.RunLevel -eq 'Highest'} | Select-Object -First 20
    foreach ($task in $tasks) {
        $actions = $task.Actions | ForEach-Object { "$($_.Execute) $($_.Arguments)".Trim() }
        $Report.Tasks += [PSCustomObject]@{
            TaskName = $task.TaskName
            TaskPath = $task.TaskPath
            RunAs = $task.Principal.UserId
            Command = $actions -join ' | '
        }
    }
} catch {}

# 3. Intentos de login fallidos (Event ID 4625)
if ($Report.Elevated) {
    try {
        $events = Get-WinEvent -FilterHashtable @{LogName='Security';Id=4625} -MaxEvents 20 -ErrorAction SilentlyContinue
        foreach ($ev in $events) {
            $xml = [xml]$ev.ToXml()
            $eventData = $xml.Event.EventData.Data
            $targetUser = ($eventData | Where-Object {$_.Name -eq 'TargetUserName'}).'#text'
            $ipAddress = ($eventData | Where-Object {$_.Name -eq 'IpAddress'}).'#text'
            $Report.Logins += [PSCustomObject]@{
                Time = $ev.TimeCreated
                User = $targetUser
                Source = $ipAddress
            }
        }
    } catch {}
} else {
    $Report.Logins = "Requiere elevacion de Administrador"
}

# 4. Puertos TCP escuchando
try {
    $conns = Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue
    foreach ($conn in $conns) {
        $proc = Get-Process -Id $conn.OwningProcess -ErrorAction SilentlyContinue
        $Report.Ports += [PSCustomObject]@{
            LocalAddress = $conn.LocalAddress
            LocalPort = $conn.LocalPort
            ProcessId = $conn.OwningProcess
            ProcessName = $proc.ProcessName
        }
    }
} catch {}

# 5. BitLocker
try {
    if ($Report.Elevated) {
        $vols = Get-BitLockerVolume -ErrorAction SilentlyContinue
        foreach ($v in $vols) {
            $Report.BitLocker += [PSCustomObject]@{
                MountPoint = $v.MountPoint
                VolumeStatus = $v.VolumeStatus
                ProtectionStatus = $v.ProtectionStatus
                EncryptionMethod = $v.EncryptionMethod
            }
        }
    } else {
        $vols = Get-CimInstance -Namespace root\cimv2\Security\MicrosoftVolumeEncryption -ClassName Win32_EncryptableVolume -ErrorAction SilentlyContinue
        if ($vols) {
            foreach ($v in $vols) {
                $statusNum = $v.GetProtectionStatus().ProtectionStatus
                $status = switch($statusNum) { 0 {"Off"} 1 {"On"} default {"Unknown"} }
                $Report.BitLocker += [PSCustomObject]@{
                    MountPoint = $v.DriveLetter
                    ProtectionStatus = $status
                    EncryptionMethod = "Requiere elevacion para detalles"
                }
            }
        } else {
            $Report.BitLocker = "Requiere elevacion de Administrador"
        }
    }
} catch {}

# 6. Parches criticos pendientes
try {
    $updateSession = New-Object -ComObject Microsoft.Update.Session
    $updateSearcher = $updateSession.CreateUpdateSearcher()
    $searchResult = $updateSearcher.Search("IsInstalled=0 and Type='Software' and IsHidden=0")
    foreach ($upd in $searchResult.Updates) {
        $Report.Updates += [PSCustomObject]@{
            Title = $upd.Title
            KB = if ($upd.Title -match 'KB\d+') { $Matches[0] } else { "N/A" }
        }
    }
} catch {}

$Report | ConvertTo-Json -Depth 4
