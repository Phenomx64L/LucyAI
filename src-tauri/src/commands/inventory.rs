// ── INVENTORY — Descubrimiento de servicios, puertos, software, certificados ─────

use std::process::Command;
use std::os::windows::process::CommandExt;
use crate::state::CREATE_NO_WINDOW;
use crate::utils::shell::{ensure_trusted_host, run_winrm};

/// Descubre servicios, puertos, software y certificados en un host Linux via SSH.
#[tauri::command]
pub async fn discover_inventory_linux(
    host: String,
    username: String,
    port: Option<u16>,
    key_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let script = r#"
echo "{"

# ── Puertos abiertos ──────────────────────────────────
echo '"ports":['
if command -v ss >/dev/null 2>&1; then
    ss -tlnp 2>/dev/null | awk 'NR>1{
        gsub(/.*:/,"",$4); port=$4;
        proc=$7; gsub(/.*\"/,"",proc); gsub(/\".*/,"",proc);
        if(proc=="") proc=$7;
        gsub(/users:\(\("/,"",proc); gsub(/\".*/,"",proc);
        printf "{\"port\":%s,\"process\":\"%s\",\"state\":\"LISTEN\"},", port, proc
    }' | sed 's/,$//'
elif command -v netstat >/dev/null 2>&1; then
    netstat -tlnp 2>/dev/null | awk '/LISTEN/{
        gsub(/.*:/,"",$4); port=$4;
        proc=$7; gsub(/\/.*/,"",proc);
        printf "{\"port\":%s,\"process\":\"%s\",\"state\":\"LISTEN\"},", port, proc
    }' | sed 's/,$//'
fi
echo '],'

# ── Servicios activos ─────────────────────────────────
echo '"services":['
if command -v systemctl >/dev/null 2>&1; then
    systemctl list-units --type=service --state=running --no-pager --no-legend 2>/dev/null | head -50 | awk '{
        name=$1; gsub(/\.service$/,"",name);
        desc=""; for(i=5;i<=NF;i++) desc=desc" "$i; gsub(/^ /,"",desc); gsub(/"/,"\\\"",desc);
        printf "{\"name\":\"%s\",\"status\":\"running\",\"description\":\"%s\"},", name, desc
    }' | sed 's/,$//'
fi
echo '],'

# ── Software instalado (top 60) ──────────────────────
echo '"software":['
if command -v dpkg >/dev/null 2>&1; then
    dpkg -l 2>/dev/null | awk '/^ii/{gsub(/"/,"\\\"",$2); gsub(/"/,"\\\"",$3); printf "{\"name\":\"%s\",\"version\":\"%s\"},", $2, $3}' | head -c 8000 | sed 's/,$//'
elif command -v rpm >/dev/null 2>&1; then
    rpm -qa --queryformat '{"name":"%{NAME}","version":"%{VERSION}"},' 2>/dev/null | head -c 8000 | sed 's/,$//'
elif command -v apk >/dev/null 2>&1; then
    apk list --installed 2>/dev/null | head -60 | awk '{split($1,a,"-"); name=a[1]; ver=a[2]; printf "{\"name\":\"%s\",\"version\":\"%s\"},", name, ver}' | sed 's/,$//'
fi
echo '],'

# ── Certificados SSL ─────────────────────────────────
echo '"certs":['
find /etc/ssl/certs /etc/letsencrypt/live /etc/pki/tls/certs -name '*.pem' -o -name '*.crt' 2>/dev/null | head -20 | while read f; do
    exp=$(openssl x509 -enddate -noout -in "$f" 2>/dev/null | cut -d= -f2)
    subj=$(openssl x509 -subject -noout -in "$f" 2>/dev/null | sed 's/subject=//' | tr -d '"\\')
    if [ -n "$exp" ]; then
        exp_epoch=$(date -d "$exp" +%s 2>/dev/null || echo 0)
        now_epoch=$(date +%s)
        days_left=$(( (exp_epoch - now_epoch) / 86400 ))
        printf '{"path":"%s","subject":"%s","expires":"%s","days_left":%d},' "$f" "$subj" "$exp" "$days_left"
    fi
done | sed 's/,$//'
echo '],'

# ── Tareas programadas (cron) ─────────────────────────
echo '"scheduled":['
(crontab -l 2>/dev/null; cat /etc/cron.d/* 2>/dev/null) | grep -v '^#' | grep -v '^$' | head -20 | while IFS= read -r line; do
    esc=$(echo "$line" | tr -d '"\\' | tr '\t' ' ')
    printf '{"entry":"%s"},' "$esc"
done | sed 's/,$//'
echo ']'

echo "}"
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

    serde_json::from_str(&raw)
        .map_err(|e| format!("JSON inválido del inventario: {}. Raw: {}", e, &raw[..raw.len().min(400)]))
}

/// Descubre servicios, puertos, software y certificados en un host Windows via WinRM.
#[tauri::command]
pub async fn discover_inventory_windows(
    host: String,
    username: String,
    password: String,
) -> Result<serde_json::Value, String> {
    ensure_trusted_host(&host);
    let script = r#"
$ports = Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
    Group-Object LocalPort | ForEach-Object {
        $p = $_.Name; $proc = ($_.Group | Select-Object -First 1).OwningProcess;
        $pname = (Get-Process -Id $proc -ErrorAction SilentlyContinue).Name
        [PSCustomObject]@{port=[int]$p; process=$pname; state='LISTEN'}
    } | Select-Object -First 50

$services = Get-Service | Where-Object {$_.Status -eq 'Running'} | Select-Object -First 50 |
    ForEach-Object { [PSCustomObject]@{name=$_.Name; status='running'; description=$_.DisplayName} }

$software = Get-ItemProperty HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\* -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName } | Select-Object -First 60 |
    ForEach-Object { [PSCustomObject]@{name=$_.DisplayName; version=$_.DisplayVersion} }

$certs = Get-ChildItem Cert:\LocalMachine\My -ErrorAction SilentlyContinue |
    ForEach-Object {
        $days = ($_.NotAfter - (Get-Date)).Days
        [PSCustomObject]@{path='Cert:\LocalMachine\My'; subject=$_.Subject; expires=$_.NotAfter.ToString('yyyy-MM-dd'); days_left=$days}
    }

$tasks = Get-ScheduledTask -ErrorAction SilentlyContinue |
    Where-Object {$_.State -eq 'Ready'} | Select-Object -First 20 |
    ForEach-Object { [PSCustomObject]@{entry=$_.TaskName} }

[PSCustomObject]@{
    ports     = @($ports)
    services  = @($services)
    software  = @($software)
    certs     = @($certs)
    scheduled = @($tasks)
} | ConvertTo-Json -Depth 4
    "#;

    let output = run_winrm(host, username, password, script.to_string()).await?;

    if !output.status.success() {
        return Err(format!("WinRM Error: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    serde_json::from_str(&raw)
        .map_err(|e| format!("JSON inválido: {}. Raw: {}", e, &raw[..raw.len().min(400)]))
}

/// Descubre servicios locales (máquina donde corre Lucy).
#[tauri::command]
pub async fn discover_inventory_local() -> Result<serde_json::Value, String> {
    let script = r#"
$ports = Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
    Group-Object LocalPort | ForEach-Object {
        $p = $_.Name; $proc = ($_.Group | Select-Object -First 1).OwningProcess;
        $pname = (Get-Process -Id $proc -ErrorAction SilentlyContinue).Name
        [PSCustomObject]@{port=[int]$p; process=$pname; state='LISTEN'}
    } | Select-Object -First 50

$services = Get-Service | Where-Object {$_.Status -eq 'Running'} | Select-Object -First 50 |
    ForEach-Object { [PSCustomObject]@{name=$_.Name; status='running'; description=$_.DisplayName} }

$software = Get-ItemProperty HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\* -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName } | Select-Object -First 60 |
    ForEach-Object { [PSCustomObject]@{name=$_.DisplayName; version=$_.DisplayVersion} }

$certs = Get-ChildItem Cert:\LocalMachine\My -ErrorAction SilentlyContinue |
    ForEach-Object {
        $days = ($_.NotAfter - (Get-Date)).Days
        [PSCustomObject]@{path='Cert:\LocalMachine\My'; subject=$_.Subject; expires=$_.NotAfter.ToString('yyyy-MM-dd'); days_left=$days}
    }

$tasks = Get-ScheduledTask -ErrorAction SilentlyContinue |
    Where-Object {$_.State -eq 'Ready'} | Select-Object -First 20 |
    ForEach-Object { [PSCustomObject]@{entry=$_.TaskName} }

[PSCustomObject]@{
    ports     = @($ports)
    services  = @($services)
    software  = @($software)
    certs     = @($certs)
    scheduled = @($tasks)
} | ConvertTo-Json -Depth 4
    "#;

    let output = tokio::task::spawn_blocking(move || {
        Command::new("powershell")
            .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
            .arg("-Command").arg(script)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    }).await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Error PowerShell: {}", e))?;

    if !output.status.success() {
        return Err(format!("Error: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    serde_json::from_str(&raw)
        .map_err(|e| format!("JSON inválido: {}. Raw: {}", e, &raw[..raw.len().min(400)]))
}
