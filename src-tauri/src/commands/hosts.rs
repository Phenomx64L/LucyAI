// ── HOSTS — Ejecución y métricas remotas (Windows WinRM + Linux SSH) ────────────

use std::process::Command;
use std::os::windows::process::CommandExt;
use serde_json::json;
use crate::state::CREATE_NO_WINDOW;
use crate::utils::shell::{ensure_trusted_host, run_winrm};

// ── SECURITY helpers ─────────────────────────────────────────────────────────

/// Validate that a host string is a safe IP or hostname (no shell metacharacters).
/// Allows: alphanumeric, dots, hyphens, underscores, IPv6 brackets and colons.
/// Public so other modules (shell.rs, etc.) can use the same validator.
pub fn validate_host(host: &str) -> Result<(), String> {
    if host.is_empty() || host.len() > 253 {
        return Err("Host inválido: vacío o demasiado largo.".to_string());
    }
    // SECURITY v1.7.101 Sprint-1 H10: reject leading `-` to prevent
    // argv-injection via `user@-oProxyCommand=evil`. OpenSSH (and other
    // tools that consume the host part) interpret a leading hyphen as
    // an option. DNS names + IPs never start with `-`, so this is a
    // pure tightening with no legitimate-input cost.
    if host.starts_with('-') {
        return Err(format!(
            "Host inválido '{}': no puede comenzar con '-' (riesgo de argv-injection).", host
        ));
    }
    // SECURITY: ASCII-only — rechaza caracteres Unicode look-alike
    // (ej. U+FF07 fullwidth apostrophe, U+02BB modifier letter) que `is_alphanumeric`
    // (Unicode-aware) aceptaría pero PowerShell/SSH podrían interpretar como meta-chars.
    if !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '[' | ']' | ':')) {
        return Err(format!(
            "Host inválido '{}': solo se permiten IPs y nombres DNS ASCII (sin caracteres especiales o Unicode).", host
        ));
    }
    Ok(())
}

/// Validate that a password is sane-sized. We don't restrict character set
/// (passwords contain arbitrary symbols) but we cap length to:
///   - prevent DoS via 100 MB password fields exhausting memory
///   - keep stack traces / panic messages bounded (in case some path leaks it)
///   - reject obvious garbage (no real password is >256 chars)
/// Empty passwords are allowed (some flows use key-based auth or pre-shared creds).
pub fn validate_password(pwd: &str) -> Result<(), String> {
    if pwd.len() > 256 {
        return Err("Password inválido: excede 256 caracteres (límite de seguridad).".to_string());
    }
    Ok(())
}

/// Validate that a username contains only safe characters.
/// Allows: alphanumeric, dots, hyphens, underscores, backslash (domain\user), @
/// Public so other modules (shell.rs, etc.) can use the same validator.
pub fn validate_username(user: &str) -> Result<(), String> {
    if user.is_empty() || user.len() > 128 {
        return Err("Username inválido: vacío o demasiado largo.".to_string());
    }
    // SECURITY v1.7.101 Sprint-1 H10: same leading-`-` defense as
    // validate_host. A username like `-oProxyCommand=evil` would
    // otherwise concatenate cleanly into `format!("{}@{}", user, host)`
    // and present as an SSH option in argv parsing edge cases.
    if user.starts_with('-') {
        return Err(format!(
            "Username inválido '{}': no puede comenzar con '-' (riesgo de argv-injection).", user
        ));
    }
    // SECURITY: ASCII-only para evitar Unicode trickery (look-alike chars que pasen
    // el filtro pero confundan a PowerShell/SSH). is_alphanumeric() acepta Unicode.
    if !user.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '\\' | '@')) {
        return Err(format!(
            "Username inválido '{}': solo se permiten caracteres ASCII alfanuméricos, '.', '-', '_', '\\\\', '@'.", user
        ));
    }
    Ok(())
}

// ── WINDOWS REMOTO (WinRM via PowerShell) ─────────────────────────────────────

#[tauri::command]
pub async fn execute_remote_windows(
    host: String,
    username: String,
    password: String,
    command: String,
) -> Result<String, String> {
    validate_host(&host)?;
    validate_username(&username)?;
    validate_password(&password)?;

    // SECURITY v1.7.232 (Phase-2 C2): guardrail scan on the remote command (S2
    // injection/obfuscation shapes only — see scan_remote_shell). Closes the
    // local-vs-remote gate asymmetry without false-blocking legit admin.
    {
        let rscan = crate::guardrails::scan_remote_shell(&command);
        if matches!(rscan.decision, crate::guardrails::ScanDecision::Block) {
            return Err(format!("Comando remoto bloqueado por guardrail: {}", rscan.reason));
        }
    }

    // Check permission rules before executing
    let perm = super::metrics::check_permission(command.clone(), "command".to_string()).await?;
    match perm.action.as_str() {
        "block" => return Err(format!("Permiso denegado: {}", perm.reason)),
        "ask" => return Err(format!("Comando requiere aprobación: {}. Crea una regla 'allow' en Permisos para ejecutar.", perm.reason)),
        "allow" => {}, // Continue with execution
        _ => return Err(format!("Acción de permiso inválida: {}", perm.action)),
    }

    let output = run_winrm(host, username, password, command).await?;
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
    validate_host(&host)?;
    validate_username(&username)?;
    validate_password(&password)?;
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
        let out = run_winrm(host.clone(), username.clone(), password.clone(), script.to_string()).await?;
        if out.status.success() {
            String::from_utf8_lossy(&out.stdout).to_string()
        } else {
            return Err(format!("WinRM Error: {}", String::from_utf8_lossy(&out.stderr)));
        }
    };
    let v: serde_json::Value = serde_json::from_str(raw.trim())
        .map_err(|e| format!("Error parseando métricas: {}. Raw: {}", e, crate::utils::safe_truncate(&raw, 200)))?;

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
    // Validate inputs to prevent injection via host/username
    validate_host(&host)?;
    validate_username(&username)?;

    // SECURITY v1.7.232 (Phase-2 C2): guardrail scan on the remote command (S2
    // injection/obfuscation shapes only — see scan_remote_shell). Closes the
    // local-vs-remote gate asymmetry without false-blocking legit admin.
    {
        let rscan = crate::guardrails::scan_remote_shell(&command);
        if matches!(rscan.decision, crate::guardrails::ScanDecision::Block) {
            return Err(format!("Comando remoto bloqueado por guardrail: {}", rscan.reason));
        }
    }

    // Check permission rules before executing
    let perm = super::metrics::check_permission(command.clone(), "command".to_string()).await?;
    match perm.action.as_str() {
        "block" => return Err(format!("Permiso denegado: {}", perm.reason)),
        "ask" => return Err(format!("Comando requiere aprobación: {}. Crea una regla 'allow' en Permisos para ejecutar.", perm.reason)),
        "allow" => {}, // Continue with execution
        _ => return Err(format!("Acción de permiso inválida: {}", perm.action)),
    }

    let port_str = port.unwrap_or(22).to_string();
    let mut cmd = Command::new("ssh");
    cmd.arg("-o").arg("StrictHostKeyChecking=accept-new")
       .arg("-o").arg("BatchMode=yes")
       .arg("-o").arg("ConnectTimeout=10")
       .arg("-p").arg(&port_str);
    if let Some(ref kp) = key_path { if !kp.is_empty() { cmd.arg("-i").arg(kp); } }
    cmd.arg(format!("{}@{}", username, host))
       .arg(&command)
       .creation_flags(CREATE_NO_WINDOW);
    let output = tokio::task::spawn_blocking(move || cmd.output())
        .await
        .map_err(|e| e.to_string())?
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
    // SECURITY: validate host & username to prevent shell injection
    // via the `ssh user@host` argument construction.
    validate_host(&host)?;
    validate_username(&username)?;

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
    let raw = tokio::task::spawn_blocking(move || -> Result<String, String> {
        use std::io::Write;
        let mut ssh_cmd = Command::new("ssh");
        ssh_cmd.arg("-o").arg("StrictHostKeyChecking=accept-new")
               .arg("-o").arg("BatchMode=yes")
               .arg("-o").arg("ConnectTimeout=15")
               .arg("-p").arg(&port_str);
        if let Some(ref kp) = key_path { if !kp.is_empty() { ssh_cmd.arg("-i").arg(kp); } }
        ssh_cmd.arg(format!("{}@{}", username, host))
               .arg("bash -s");
        let mut child = ssh_cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("SSH no disponible (verifica OpenSSH en Windows): {}", e))?;
        if let Some(mut s) = child.stdin.take() {
            s.write_all(script.as_bytes())
                .map_err(|e| format!("Error enviando script SSH: {}", e))?;
        }
        let output = child.wait_with_output()
            .map_err(|e| format!("Error esperando SSH: {}", e))?;
        if !output.status.success() {
            return Err(format!("SSH Error: {}", String::from_utf8_lossy(&output.stderr)));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }).await
        .map_err(|e| e.to_string())??;

    if raw.is_empty() {
        return Err("El host no devolvió datos. Verifica que bash esté disponible y la conexión SSH.".to_string());
    }

    serde_json::from_str(&raw)
        .map_err(|e| format!("JSON inválido del host: {}.\nRaw (primeros 400 chars): {}", e, crate::utils::safe_truncate(&raw, 400)))
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
    // SECURITY v1.7.232 (Phase-2 C11): validate host + username for BOTH the Linux
    // (SSH) and Windows (WinRM) paths, BEFORE the host_type branch. The Linux branch
    // builds `format!("{}@{}", username, host)` and passes it to `ssh` argv — a
    // username/host beginning with '-' would otherwise be re-parsed by SSH's getopt as
    // an option (H10 argv-injection → -oProxyCommand local exec). Previously ONLY the
    // Windows else-branch validated, leaving the SSH path exposed; read_remote_file /
    // write_remote_file funnel through here so they inherited the gap too.
    validate_host(&host)?;
    validate_username(&username)?;
    // SECURITY: cap password length early (validates Some only — None is fine
    // for key-based auth). Prevents 100MB password payloads from consuming
    // memory or appearing in panic traces.
    if let Some(ref p) = password { validate_password(p)?; }

    // SECURITY v1.7.232 (Phase-2 C2): remote exec had NO guardrail scan (only
    // default-allow check_permission) — an asymmetry vs. the local exec paths.
    // Block genuine injection/obfuscation shapes; see scan_remote_shell for why
    // SSRF / plain destructive verbs are deliberately NOT gated here (would break
    // legitimate interactive remote admin + internal-IP/metadata access).
    {
        let rscan = crate::guardrails::scan_remote_shell(&command);
        if matches!(rscan.decision, crate::guardrails::ScanDecision::Block) {
            return Err(format!("Comando remoto bloqueado por guardrail: {}", rscan.reason));
        }
    }

    // Check permission rules before executing
    let perm = super::metrics::check_permission(command.clone(), "command".to_string()).await?;
    match perm.action.as_str() {
        "block" => return Err(format!("Permiso denegado: {}", perm.reason)),
        "ask" => return Err(format!("Comando requiere aprobación: {}. Crea una regla 'allow' en Permisos para ejecutar.", perm.reason)),
        "allow" => {}, // Continue with execution
        _ => return Err(format!("Acción de permiso inválida: {}", perm.action)),
    }

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
        // (host/username already validated at the top of the function — C11)
        ensure_trusted_host(&host);
        let pwd = password.unwrap_or_default();
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            run_winrm(host, username, pwd, command)
        ).await
            .map_err(|_| "Timeout: el comando tardó más de 60 segundos.".to_string())??;

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
    // SECURITY: validate inputs for both Linux (SSH) and Windows (WinRM) paths.
    // Both inject `host`/`username` into shell-level argument construction.
    validate_host(&host)?;
    validate_username(&username)?;
    if let Some(ref p) = password { validate_password(p)?; }

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
        let raw = tokio::task::spawn_blocking(move || -> Result<String, String> {
            use std::io::Write;
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
            if let Some(mut s) = child.stdin.take() {
                s.write_all(script.as_bytes())
                    .map_err(|e| format!("Error enviando bootstrap: {}", e))?;
            }
            let output = child.wait_with_output()
                .map_err(|e| format!("Error en bootstrap SSH: {}", e))?;
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }).await
            .map_err(|e| e.to_string())??;

        if raw.is_empty() { return Err("Bootstrap: sin datos del host".to_string()); }
        serde_json::from_str(&raw)
            .map_err(|e| format!("Bootstrap JSON inválido: {}. Raw: {}", e, crate::utils::safe_truncate(&raw, 300)))

    } else {
        // Windows WinRM — script PowerShell inyectado en ScriptBlock
        ensure_trusted_host(&host);
        let pwd = password.unwrap_or_default();

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

        let output = run_winrm(host, username, pwd, win_script.to_string()).await
            .map_err(|e| format!("WinRM Bootstrap error: {}", e))?;

        if !output.status.success() {
            return Err(format!("WinRM Bootstrap error: {}", String::from_utf8_lossy(&output.stderr)));
        }
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if raw.is_empty() { return Err("Bootstrap Windows: sin datos".to_string()); }
        serde_json::from_str(&raw)
            .map_err(|e| format!("Bootstrap JSON inválido: {}. Raw: {}", e, crate::utils::safe_truncate(&raw, 300)))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// REMOTE FILE OPS — read/write archivos remotos vía base64 para diff preview
// ─────────────────────────────────────────────────────────────────────────────
//
// Flujo de trabajo:
//   1. Frontend llama read_remote_file(host, ...) → recibe contenido UTF-8
//   2. Usuario edita en un componente local (modal con diff visual)
//   3. Frontend llama write_remote_file con el nuevo contenido
//   4. El backend hace backup automático antes de sobrescribir (.lucy.bak)
//
// SECURITY:
//   - Las rutas se validan: deben ser absolutas, no contener `..`
//   - Tamaño máximo 1 MB para evitar OOM
//   - El contenido se transporta como base64 para evitar problemas de escaping
//   - Se respetan las permission rules igual que execute_remote_*

use base64::Engine as _;

const MAX_REMOTE_FILE_BYTES: usize = 1_048_576; // 1 MB

fn validate_remote_path(p: &str) -> Result<(), String> {
    if p.is_empty() { return Err("Ruta vacía".to_string()); }
    if p.len() > 4096 { return Err("Ruta demasiado larga (>4096)".to_string()); }
    if p.contains("..") { return Err("Ruta inválida: '..' no permitido (path traversal)".to_string()); }
    if p.contains('\0') { return Err("Ruta inválida: contiene byte nulo".to_string()); }

    // Strategy: instead of an ASCII-only whitelist (which rejected legitimate
    // international filenames like "año.txt", "東京.log", etc), we BLACKLIST
    // shell metacharacters and control bytes. The path is single-quoted
    // before injection into bash/PowerShell, so any non-quote char is safe.
    //
    // Rejected: ; | & $ ! < > * ? [ ] { } # ` " ' \n \r \t
    //   (single-quote `'` is handled by the caller's escaping but is still
    //    rejected pre-emptively to keep the contract simple)
    // Also rejected: any ASCII control byte (0x00-0x1F, 0x7F).
    const FORBIDDEN: &[char] = &[
        ';', '|', '&', '$', '!', '<', '>', '*', '?',
        '[', ']', '{', '}', '#', '`', '"', '\'',
        '\n', '\r', '\t',
    ];
    if let Some(bad) = p.chars().find(|c| FORBIDDEN.contains(c) || c.is_control()) {
        return Err(format!(
            "Ruta inválida: contiene carácter prohibido '{}' (shell metacharacter o control byte)",
            bad.escape_default()
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn read_remote_file(
    host: String,
    username: String,
    path: String,
    host_type: String,
    port: Option<u16>,
    password: Option<String>,
    key_path: Option<String>,
) -> Result<String, String> {
    validate_remote_path(&path)?;

    // Build a remote command that base64-encodes the file content. Both bash and PowerShell
    // can do this without external dependencies.
    //
    // Stderr handling: previously this mixed stderr into stdout via `2>&1`, which meant
    // a missing-file error message got piped into the base64 decoder → confusing
    // "Error decodificando base64" instead of "File not found". Now we surface
    // categorised errors with a `__LUCY_ERROR__:` prefix and discard real stderr.
    let remote_cmd = if host_type == "linux" {
        // Quote-safe filename (single quotes inside POSIX single-quoted strings)
        let path_esc = path.replace('\'', "'\\''");
        // Sequence:
        //   1) explicit existence + readable checks → categorical error message
        //   2) base64 only on success, stderr silenced (real errors are rare past step 1)
        //   3) cap at 4 MB to avoid mailing the entire disk back
        format!(
            "if [ ! -e '{p}' ]; then echo '__LUCY_ERROR__:not_found'; exit 0; fi; \
             if [ ! -f '{p}' ]; then echo '__LUCY_ERROR__:not_a_file'; exit 0; fi; \
             if [ ! -r '{p}' ]; then echo '__LUCY_ERROR__:permission_denied'; exit 0; fi; \
             base64 -w 0 '{p}' 2>/dev/null | head -c 4194304",
            p = path_esc
        )
    } else {
        // PowerShell on Windows — explicit Test-Path then ReadAllBytes. On error we
        // emit the same `__LUCY_ERROR__:` prefix for symmetry with the Linux branch.
        let path_esc = path.replace('\'', "''");
        format!(
            "if (-not (Test-Path -LiteralPath '{p}')) {{ Write-Output '__LUCY_ERROR__:not_found'; }} \
             elseif ((Get-Item -LiteralPath '{p}').PSIsContainer) {{ Write-Output '__LUCY_ERROR__:not_a_file'; }} \
             else {{ \
                try {{ [Convert]::ToBase64String([System.IO.File]::ReadAllBytes('{p}')) }} \
                catch {{ Write-Output ('__LUCY_ERROR__:' + ($_.Exception.GetType().Name)) }} \
             }}",
            p = path_esc
        )
    };

    // Reuse execute_shell_cmd for transport — it already enforces permission rules + audit
    let b64 = execute_shell_cmd(host, username, remote_cmd, host_type, port, password, key_path).await?;
    let b64_clean = b64.trim().replace(['\n', '\r', ' '], "");

    // Detect and surface categorical errors from the remote helper script
    if let Some(reason) = b64_clean.strip_prefix("__LUCY_ERROR__:") {
        let human = match reason {
            "not_found" => format!("Archivo no encontrado: {}", path),
            "not_a_file" => format!("La ruta existe pero no es un archivo regular: {}", path),
            "permission_denied" => format!("Permiso denegado al leer: {}", path),
            other => format!("Error remoto al leer '{}': {}", path, other),
        };
        return Err(human);
    }

    let bytes = base64::engine::general_purpose::STANDARD.decode(&b64_clean)
        .map_err(|e| format!("Error decodificando base64 desde host: {} (¿el archivo es binario o el remoto truncó la salida?)", e))?;

    if bytes.len() > MAX_REMOTE_FILE_BYTES {
        return Err(format!("Archivo demasiado grande ({} bytes, máx 1 MB para edición remota)", bytes.len()));
    }

    String::from_utf8(bytes)
        .map_err(|_| "El archivo contiene bytes no-UTF8 — solo se admiten archivos de texto para edición".to_string())
}

/// Firma fijada por el contrato IPC con el frontend: cada parametro es un campo
/// que el lado Svelte envia por nombre en su `invoke()`. Agruparlos en una
/// struct para complacer al lint obligaria a cambiar todas las llamadas del
/// frontend, y el lint mide legibilidad, no protocolos.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn write_remote_file(
    host: String,
    username: String,
    path: String,
    content: String,
    host_type: String,
    port: Option<u16>,
    password: Option<String>,
    key_path: Option<String>,
    create_backup: Option<bool>,
) -> Result<String, String> {
    validate_remote_path(&path)?;
    if content.len() > MAX_REMOTE_FILE_BYTES {
        return Err(format!("Contenido demasiado grande ({} bytes, máx 1 MB)", content.len()));
    }

    let backup = create_backup.unwrap_or(true);
    let b64 = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());

    let remote_cmd = if host_type == "linux" {
        // 1) backup (if requested), 2) decode b64 to file via printf+base64
        let path_esc = path.replace('\'', "'\\''");
        let backup_cmd = if backup {
            format!("[ -f '{p}' ] && cp -p '{p}' '{p}.lucy.bak' 2>/dev/null; ", p = path_esc)
        } else {
            String::new()
        };
        format!(
            "{backup}printf '%s' '{b64}' | base64 -d > '{path}' && echo 'OK:{bytes}'",
            backup = backup_cmd,
            b64 = b64,
            path = path_esc,
            bytes = content.len()
        )
    } else {
        // PowerShell
        let path_esc = path.replace('\'', "''");
        let backup_cmd = if backup {
            format!("if (Test-Path '{p}') {{ Copy-Item '{p}' '{p}.lucy.bak' -Force }}; ", p = path_esc)
        } else {
            String::new()
        };
        format!(
            "{backup}$bytes = [Convert]::FromBase64String('{b64}'); [System.IO.File]::WriteAllBytes('{path}', $bytes); Write-Output 'OK:{bytes_len}'",
            backup = backup_cmd,
            b64 = b64,
            path = path_esc,
            bytes_len = content.len()
        )
    };

    let result = execute_shell_cmd(host, username, remote_cmd, host_type, port, password, key_path).await?;

    if !result.contains("OK:") {
        return Err(format!("Error escribiendo archivo remoto: {}", result.trim()));
    }

    Ok(if backup {
        format!("{} bytes escritos. Backup: {}.lucy.bak", content.len(), path)
    } else {
        format!("{} bytes escritos.", content.len())
    })
}
