// ── HOSTS — Ejecución y métricas remotas (Windows WinRM + Linux SSH) ────────────

use std::process::Command;
use std::os::windows::process::CommandExt;
use serde_json::json;
use crate::state::CREATE_NO_WINDOW;
use crate::utils::shell::ensure_trusted_host;

// ── WINDOWS REMOTO (WinRM via PowerShell) ─────────────────────────────────────

#[tauri::command]
pub async fn execute_remote_windows(
    host: String,
    username: String,
    password: String,
    command: String,
) -> Result<String, String> {
    let pw_esc = password.replace('\'', "''");
    let ps = format!(
        "$pass = ConvertTo-SecureString '{}' -AsPlainText -Force; \
         $cred = New-Object System.Management.Automation.PSCredential('{}', $pass); \
         Invoke-Command -ComputerName '{}' -Credential $cred -ScriptBlock {{ {} }} -ErrorAction Stop",
        pw_esc, username, host, command
    );
    let output = Command::new("powershell")
        .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
        .arg("-Command").arg(&ps)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Error WinRM: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(format!("WinRM Error: {}", String::from_utf8_lossy(&output.stderr)))
    }
}

#[tauri::command]
pub async fn get_remote_health_windows(
    host: String,
    username: String,
    password: String,
) -> Result<serde_json::Value, String> {
    let script = r#"
        $os    = Get-WmiObject Win32_OperatingSystem
        $cpu   = (Get-Counter '\Processor(_Total)\% Processor Time' -SampleInterval 1 -MaxSamples 1).CounterSamples[0].CookedValue
        $cores = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
        $disk  = Get-PSDrive -PSProvider FileSystem | Where-Object { $_.Used -ne $null } |
                Select-Object @{N='name';E={$_.Name}}, @{N='used_gb';E={[Math]::Round($_.Used/1GB,1)}}, @{N='free_gb';E={[Math]::Round($_.Free/1GB,1)}}, @{N='total_gb';E={[Math]::Round(($_.Used+$_.Free)/1GB,1)}}
        $procs = Get-Process | Sort-Object WorkingSet64 -Descending | Select-Object -First 5 |
                 Select-Object @{N='name';E={$_.Name}}, @{N='cpu';E={[Math]::Round($_.CPU,1)}}, @{N='mem_mb';E={[Math]::Round($_.WorkingSet64/1MB,0)}}
        [PSCustomObject]@{
            hostname      = $env:COMPUTERNAME
            os            = $os.Caption
            uptime_h      = [Math]::Round((New-TimeSpan -Start $os.ConvertToDateTime($os.LastBootUpTime)).TotalHours, 0)
            timestamp     = (Get-Date -Format 'HH:mm:ss')
            cpu_global    = [Math]::Round($cpu, 1)
            cpu_cores     = [int]$cores
            mem_total_mb  = [Math]::Round($os.TotalVisibleMemorySize / 1KB, 0)
            mem_free_mb   = [Math]::Round($os.FreePhysicalMemory / 1KB, 0)
            disks         = $disk
            top_processes = $procs
        } | ConvertTo-Json -Depth 5
    "#;
    let raw = {
        let pw_esc = password.replace('\'', "''");
        let ps = format!(
            "$pass = ConvertTo-SecureString '{}' -AsPlainText -Force; \
             $cred = New-Object System.Management.Automation.PSCredential('{}', $pass); \
             Invoke-Command -ComputerName '{}' -Credential $cred -ScriptBlock {{ {} }} -ErrorAction Stop",
            pw_esc, username, host, script
        );
        let output = Command::new("powershell")
            .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
            .arg("-Command").arg(&ps)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Error WinRM: {}", e))?;
        if output.status.success() {
            String::from_utf8_lossy(&output.stdout).to_string()
        } else {
            return Err(format!("WinRM Error: {}", String::from_utf8_lossy(&output.stderr)));
        }
    };
    let v: serde_json::Value = serde_json::from_str(raw.trim())
        .map_err(|e| format!("Error parseando métricas: {}. Raw: {}", e, &raw[..raw.len().min(200)]))?;

    let mem_total = v["mem_total_mb"].as_f64().unwrap_or(0.0) as u64;
    let mem_free  = v["mem_free_mb"].as_f64().unwrap_or(0.0) as u64;
    let mem_used  = mem_total.saturating_sub(mem_free);
    let mem_pct   = if mem_total > 0 { (mem_used as f64 / mem_total as f64) * 100.0 } else { 0.0 };

    Ok(json!({
        "hostname":      v["hostname"],
        "os":            v["os"],
        "uptime_h":      v["uptime_h"],
        "timestamp":     v["timestamp"],
        "cpu": {
            "cores":    v["cpu_cores"].as_u64().unwrap_or(0),
            "global":   v["cpu_global"],
            "per_core": []
        },
        "memory": {
            "total_mb": mem_total,
            "used_mb":  mem_used,
            "percent":  (mem_pct * 10.0).round() / 10.0
        },
        "disks":         v["disks"],
        "top_processes": v["top_processes"]
    }))
}

// ── LINUX REMOTO (OpenSSH nativo de Windows 10/11) ───────────────────────────

#[tauri::command]
pub async fn execute_remote_linux(
    host: String,
    username: String,
    command: String,
    port: Option<u16>,
    key_path: Option<String>,
) -> Result<String, String> {
    let port_str = port.unwrap_or(22).to_string();
    let mut cmd = Command::new("ssh");
    cmd.arg("-o").arg("StrictHostKeyChecking=accept-new")
       .arg("-o").arg("BatchMode=yes")
       .arg("-o").arg("ConnectTimeout=10")
       .arg("-p").arg(&port_str);
    if let Some(ref kp) = key_path { if !kp.is_empty() { cmd.arg("-i").arg(kp); } }
    cmd.arg(&format!("{}@{}", username, host))
       .arg(&command)
       .creation_flags(CREATE_NO_WINDOW);
    let output = cmd.output()
        .map_err(|e| format!("SSH no disponible. Verifica OpenSSH en Windows: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(format!("SSH Error: {}", String::from_utf8_lossy(&output.stderr)))
    }
}

#[tauri::command]
pub async fn get_remote_health_linux(
    host: String,
    username: String,
    port: Option<u16>,
    key_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let script = r#"
HOSTNAME=$(hostname)
UPTIME_H=$(awk '{print int($1/3600)}' /proc/uptime)
TIMESTAMP=$(date '+%H:%M:%S')
CORES=$(nproc)

CPU1=$(awk 'NR==1{u=$2+$3+$4; i=$5; t=u+i+$6+$7+$8; print u" "t}' /proc/stat)
sleep 1
CPU2=$(awk 'NR==1{u=$2+$3+$4; i=$5; t=u+i+$6+$7+$8; print u" "t}' /proc/stat)
CPU_USED=$(echo "$CPU1 $CPU2" | awk '{du=$3-$1; dt=$4-$2; if(dt>0) printf "%.1f", du/dt*100; else print "0"}')

MEM_TOTAL=$(awk '/MemTotal/{print int($2/1024)}' /proc/meminfo)
MEM_AVAIL=$(awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo)
MEM_USED=$((MEM_TOTAL - MEM_AVAIL))
MEM_PCT=$(awk "BEGIN{if($MEM_TOTAL>0) printf \"%.1f\", $MEM_USED/$MEM_TOTAL*100; else print 0}")

OS_NAME=$(grep '^PRETTY_NAME=' /etc/os-release 2>/dev/null | cut -d= -f2- | tr -d '"\\')
[ -z "$OS_NAME" ] && OS_NAME=$(grep '^NAME=' /etc/os-release 2>/dev/null | cut -d= -f2- | tr -d '"\\')
[ -z "$OS_NAME" ] && OS_NAME="Linux"

DISKS_JSON=$(df -BG 2>/dev/null | awk 'NR>1 && /^\/dev\// {
    gsub("G","",$2); gsub("G","",$3); gsub("G","",$4);
    tot=$2; used=$3; free=$4;
    pct=(tot>0) ? used/tot*100 : 0;
    printf "{\"name\":\"%s\",\"mount\":\"%s\",\"total_gb\":%d,\"used_gb\":%d,\"free_gb\":%d,\"percent\":%.1f},", $1, $6, tot, used, free, pct
}' | sed 's/,$//')

PROCS_JSON=$(ps aux --no-headers --sort=-%mem 2>/dev/null | head -5 | awk '{
    name=$11; gsub(".*/","",name);
    printf "{\"name\":\"%s\",\"cpu\":\"%s\",\"mem_mb\":%d,\"pid\":%s},", name, $3, $6/1024, $2
}' | sed 's/,$//')

printf '{"hostname":"%s","os":"%s","uptime_h":%d,"timestamp":"%s","cpu":{"global":%s,"cores":%d,"per_core":[]},"memory":{"total_mb":%d,"used_mb":%d,"percent":%s},"disks":[%s],"top_processes":[%s]}' \
    "$HOSTNAME" "$OS_NAME" "$UPTIME_H" "$TIMESTAMP" \
    "$CPU_USED" "$CORES" \
    "$MEM_TOTAL" "$MEM_USED" "$MEM_PCT" \
    "$DISKS_JSON" "$PROCS_JSON"
"#;

    let port_str = port.unwrap_or(22).to_string();
    let mut ssh_cmd = Command::new("ssh");
    ssh_cmd.arg("-o").arg("StrictHostKeyChecking=accept-new")
           .arg("-o").arg("BatchMode=yes")
           .arg("-o").arg("ConnectTimeout=15")
           .arg("-p").arg(&port_str);
    if let Some(ref kp) = key_path { if !kp.is_empty() { ssh_cmd.arg("-i").arg(kp); } }
    ssh_cmd.arg(&format!("{}@{}", username, host))
           .arg("bash -s");
    let mut child = ssh_cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("SSH no disponible (verifica OpenSSH en Windows): {}", e))?;

    if let Some(stdin) = child.stdin.take() {
        use std::io::Write;
        let mut stdin = stdin;
        stdin.write_all(script.as_bytes())
            .map_err(|e| format!("Error enviando script SSH: {}", e))?;
    }

    let output = child.wait_with_output()
        .map_err(|e| format!("Error esperando SSH: {}", e))?;

    if !output.status.success() {
        return Err(format!("SSH Error: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return Err("El host no devolvió datos. Verifica que bash esté disponible y la conexión SSH.".to_string());
    }

    serde_json::from_str(&raw)
        .map_err(|e| format!("JSON inválido del host: {}.\nRaw (primeros 400 chars): {}", e, &raw[..raw.len().min(400)]))
}

// ── SHELL REMOTA DIRECTA (sin streaming) ──────────────────────────────────────

#[tauri::command]
pub async fn execute_shell_cmd(
    host: String,
    username: String,
    command: String,
    host_type: String,
    port: Option<u16>,
    password: Option<String>,
    key_path: Option<String>,
) -> Result<String, String> {
    if host_type == "linux" {
        let port_str = port.unwrap_or(22).to_string();
        let full_cmd = format!("{}@{}", username, host);
        let key_path_clone = key_path.clone();
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            tokio::task::spawn_blocking(move || {
                let mut cmd = Command::new("ssh");
                cmd.arg("-o").arg("StrictHostKeyChecking=accept-new")
                   .arg("-o").arg("BatchMode=yes")
                   .arg("-o").arg("ConnectTimeout=10")
                   .arg("-o").arg("ServerAliveInterval=5")
                   .arg("-o").arg("ServerAliveCountMax=2")
                   .arg("-p").arg(&port_str);
                if let Some(ref kp) = key_path_clone { if !kp.is_empty() { cmd.arg("-i").arg(kp); } }
                cmd.arg(&full_cmd)
                   .arg(&command)
                   .creation_flags(CREATE_NO_WINDOW)
                   .output()
            })
        ).await
            .map_err(|_| "Timeout: el comando SSH tardó más de 120 segundos.".to_string())?
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("SSH no disponible. Verifica OpenSSH en Windows: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if output.status.success() {
            Ok(if stdout.trim().is_empty() { "(sin salida)".to_string() } else { stdout })
        } else {
            Err(if stderr.trim().is_empty() { stdout } else { stderr })
        }
    } else {
        ensure_trusted_host(&host);
        let pwd = password.unwrap_or_default();
        let pw_esc = pwd.replace('\'', "''");
        let ps = format!(
            "$pass = ConvertTo-SecureString '{}' -AsPlainText -Force; \
             $cred = New-Object System.Management.Automation.PSCredential('{}', $pass); \
             Invoke-Command -ComputerName '{}' -Credential $cred \
               -ScriptBlock {{ {} }} -ErrorAction Stop",
            pw_esc, username, host, command
        );
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            tokio::task::spawn_blocking(move || {
                Command::new("powershell")
                    .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
                    .arg("-Command").arg(&ps)
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
            })
        ).await
            .map_err(|_| "Timeout: el comando tardó más de 60 segundos.".to_string())?
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("WinRM Error: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if output.status.success() {
            Ok(if stdout.trim().is_empty() { "(sin salida)".to_string() } else { stdout })
        } else {
            Err(format!("WinRM Error: {}", stderr))
        }
    }
}

// ── NEXSHELL BOOTSTRAP — descubre contexto del host al conectar ───────────────
// Devuelve JSON con: hostname, OS, shell, user, CWD, rama Git, K8s ctx,
// Python venv, Node version, Docker disponible, herramientas instaladas.

#[tauri::command]
pub async fn nexshell_bootstrap(
    host: String,
    username: String,
    host_type: String,
    port: Option<u16>,
    password: Option<String>,
    key_path: Option<String>,
) -> Result<serde_json::Value, String> {
    if host_type == "linux" {
        // Enviado por stdin (bash -s) para evitar problemas de escapado de comillas
        let script = r#"
__H=$(hostname 2>/dev/null | tr -d '"\\')
__OS=$(grep '^PRETTY_NAME=' /etc/os-release 2>/dev/null | cut -d= -f2- | tr -d '"\\')
[ -z "$__OS" ] && __OS=$(grep '^NAME=' /etc/os-release 2>/dev/null | cut -d= -f2- | tr -d '"\\')
[ -z "$__OS" ] && __OS=$(uname -s)
__SHELL=$(basename "${SHELL:-bash}")
__KERNEL=$(uname -r 2>/dev/null | cut -d- -f1 | tr -d '"\\')
__USER=$(whoami 2>/dev/null | tr -d '"\\')
__CWD=$(pwd 2>/dev/null | tr -d '"\\')

# Git — solo si estamos dentro de un repositorio
__GB=""; __GD="false"
if command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    __GB=$(git rev-parse --abbrev-ref HEAD 2>/dev/null | tr -d '"\\')
    [ -n "$(git status --porcelain 2>/dev/null)" ] && __GD="true"
fi

# Kubernetes
__K8S=""
command -v kubectl >/dev/null 2>&1 && __K8S=$(kubectl config current-context 2>/dev/null | tr -d '"\\')

# Python venv activo
__VENV=""
[ -n "$VIRTUAL_ENV" ] && __VENV=$(basename "$VIRTUAL_ENV" | tr -d '"\\')

# Node.js
__NODE=""
command -v node >/dev/null 2>&1 && __NODE=$(node --version 2>/dev/null | tr -d 'v"\\')

# Docker disponible
__DOCK="false"
command -v docker >/dev/null 2>&1 && __DOCK="true"

# Herramientas relevantes instaladas en este servidor
__TOOLS=""
for t in nginx apache2 httpd pm2 redis-server psql mysql mongod java python3 php go rustc terraform ansible kubectl helm podman certbot; do
    command -v $t >/dev/null 2>&1 && __TOOLS="${__TOOLS}${t},"
done
__TOOLS=$(printf '%s' "$__TOOLS" | sed 's/,$//')

printf '{"hostname":"%s","os":"%s","shell":"%s","kernel":"%s","user":"%s","cwd":"%s","git_branch":"%s","git_dirty":%s,"k8s_ctx":"%s","python_venv":"%s","node_ver":"%s","docker":%s,"tools":"%s"}' \
    "$__H" "$__OS" "$__SHELL" "$__KERNEL" "$__USER" "$__CWD" \
    "$__GB" "$__GD" "$__K8S" "$__VENV" "$__NODE" "$__DOCK" "$__TOOLS"
"#;
        let port_str = port.unwrap_or(22).to_string();
        let mut ssh_cmd = Command::new("ssh");
        ssh_cmd.arg("-o").arg("StrictHostKeyChecking=accept-new")
               .arg("-o").arg("BatchMode=yes")
               .arg("-o").arg("ConnectTimeout=10")
               .arg("-p").arg(&port_str);
        if let Some(ref kp) = key_path { if !kp.is_empty() { ssh_cmd.arg("-i").arg(kp); } }
        ssh_cmd.arg(format!("{}@{}", username, host)).arg("bash -s");

        let mut child = ssh_cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("SSH no disponible: {}", e))?;

        if let Some(stdin) = child.stdin.take() {
            use std::io::Write;
            let mut s = stdin;
            s.write_all(script.as_bytes())
                .map_err(|e| format!("Error enviando bootstrap: {}", e))?;
        }

        let output = child.wait_with_output()
            .map_err(|e| format!("Error en bootstrap SSH: {}", e))?;

        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if raw.is_empty() { return Err("Bootstrap: sin datos del host".to_string()); }
        serde_json::from_str(&raw)
            .map_err(|e| format!("Bootstrap JSON inválido: {}. Raw: {}", e, &raw[..raw.len().min(300)]))

    } else {
        // Windows WinRM — script PowerShell inyectado en ScriptBlock
        ensure_trusted_host(&host);
        let pwd = password.unwrap_or_default();
        let pw_esc = pwd.replace('\'', "''");

        // Script embebido como argumento (no en el format! string) para evitar conflictos de llaves
        let win_script = r#"
            $h  = $env:COMPUTERNAME
            $os = (Get-WmiObject Win32_OperatingSystem -EA SilentlyContinue).Caption
            if (-not $os) { $os = 'Windows' }
            $user   = $env:USERNAME
            $cwd    = (Get-Location).Path -replace '\\', '/'
            $psver  = $PSVersionTable.PSVersion.Major.ToString()
            $gitB   = ''; $gitD = $false
            if (Get-Command git -EA SilentlyContinue) {
                $gitB = (git rev-parse --abbrev-ref HEAD 2>$null)
                if ($gitB) { $gitD = [bool](git status --porcelain 2>$null) } else { $gitB = '' }
            }
            $docker = [bool](Get-Command docker -EA SilentlyContinue)
            $nodeV  = ''; if (Get-Command node -EA SilentlyContinue) { $nodeV = (node --version 2>$null) -replace 'v','' }
            $k8s    = ''; if (Get-Command kubectl -EA SilentlyContinue) { $k8s = (kubectl config current-context 2>$null) }
            $tools  = @()
            foreach ($t in @('nginx','redis-server','psql','mysql','mongod','java','python','node','dotnet','docker','kubectl','terraform','ansible','az','aws','helm','certbot')) {
                if (Get-Command $t -EA SilentlyContinue) { $tools += $t }
            }
            [PSCustomObject]@{
                hostname=$h; os=$os; shell='powershell'; kernel=''; ps_ver=$psver
                user=$user; cwd=$cwd; git_branch=$gitB; git_dirty=$gitD
                k8s_ctx=$k8s; docker=$docker; node_ver=$nodeV; python_venv=''
                tools=($tools -join ',')
            } | ConvertTo-Json -Compress
        "#;

        let ps = format!(
            "$pass = ConvertTo-SecureString '{}' -AsPlainText -Force; \
             $cred = New-Object System.Management.Automation.PSCredential('{}', $pass); \
             Invoke-Command -ComputerName '{}' -Credential $cred \
               -ScriptBlock {{ {} }} -ErrorAction Stop",
            pw_esc, username, host, win_script
        );

        let output = tokio::task::spawn_blocking(move || {
            Command::new("powershell")
                .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
                .arg("-Command").arg(&ps)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        }).await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("WinRM Bootstrap error: {}", e))?;

        if !output.status.success() {
            return Err(format!("WinRM Bootstrap error: {}", String::from_utf8_lossy(&output.stderr)));
        }
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if raw.is_empty() { return Err("Bootstrap Windows: sin datos".to_string()); }
        serde_json::from_str(&raw)
            .map_err(|e| format!("Bootstrap JSON inválido: {}. Raw: {}", e, &raw[..raw.len().min(300)]))
    }
}
