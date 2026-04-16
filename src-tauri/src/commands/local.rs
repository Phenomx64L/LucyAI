// ── LOCAL — Ejecución alternativa a PowerShell: CMD, WMIC, Netsh, Reg, Cscript ──
// Capacidades que funcionan incluso cuando PowerShell está restringido por política.

use std::process::Command;
use std::os::windows::process::CommandExt;
use std::io::Write as IoWrite;
use sysinfo::System;
use chrono::Local;
use serde::Serialize;
use crate::state::CREATE_NO_WINDOW;
use crate::utils::logging::{write_app_log, rotate_audit_log, get_logs_dir};

// ── HELPER: Resolve relative paths against GLOBAL_CWD ───────────────────────
// The agent often emits relative paths like "src-tauri/src/upscaler.rs".
// Without this, they resolve against the *process* CWD (Lucy's install dir),
// not the user's project directory.
fn resolve_path(raw: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        let cwd = crate::state::GLOBAL_CWD.read()
            .map(|c| c.clone())
            .unwrap_or_else(|_| "C:\\".to_string());
        std::path::Path::new(&cwd).join(p)
    }
}

// ── HELPER DE PARSEO DE ARGUMENTOS (Para soportar comillas) ──────────────────

fn parse_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in input.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                current.push(c); // Mantenemos la comilla para el sistema
            }
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

// ── AUDIT HELPER ─────────────────────────────────────────────────────────────

fn audit(entry: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true)
        .open(get_logs_dir().join("lucy_audit.log"))
    {
        let _ = writeln!(f, "{}", entry);
    }
}

fn ts() -> String { Local::now().format("%Y-%m-%d %H:%M:%S").to_string() }
fn host() -> String { System::host_name().unwrap_or_else(|| "Local".to_string()) }

// ── CMD.EXE — alternativa cuando PS está bloqueado por política ───────────────

#[tauri::command]
pub async fn execute_cmd(script: String, force_execute: bool) -> Result<String, String> {
    rotate_audit_log();
    let lower = script.to_lowercase();

    let blocklist = [
        "format ", "del /s", "rd /s", "rmdir /s",
        "net user /add", "net localgroup administrators",
        "schtasks /create", "schtasks /delete",
        "takeown /f c:\\windows", "bcdedit", "diskpart",
    ];

    let mut bypassed = false;
    for blocked in &blocklist {
        if lower.contains(blocked) {
            if force_execute {
                audit(&format!("[{}] [HOST:{}] [CMD_BYPASS] Restringido: {}", ts(), host(), blocked));
                write_app_log("WARNING", &format!("CMD bypass autorizado: {}", blocked));
                bypassed = true;
                break;
            } else {
                audit(&format!("[{}] [HOST:{}] [CMD_BLOCKED] {}", ts(), host(), &script[..script.len().min(200)]));
                return Err(format!("SECURITY_BLOCK:{}", blocked));
            }
        }
    }

    let op = if bypassed { "CMD_EXEC_BYPASS" } else { "CMD_EXECUTED" };
    audit(&format!("[{}] [HOST:{}] [{}] {}", ts(), host(), op, &script[..script.len().min(200)]));

    let script_clone = script.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300), // AUMENTADO A 5 MINUTOS (300s)
        tokio::task::spawn_blocking(move || {
            let cwd = crate::state::GLOBAL_CWD.read().map(|c| c.clone()).unwrap_or_else(|_| "C:\\".to_string());
            Command::new("cmd")
                .current_dir(cwd)
                .arg("/C")
                .arg(&script_clone)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        }),
    ).await;

    match result {
        Err(_) => Err("Timeout: el comando tardó más de 5 minutos (300s) y fue cancelado.".to_string()),
        Ok(Err(e)) => Err(format!("Error spawn CMD: {}", e)),
        Ok(Ok(Err(e))) => { write_app_log("ERROR", &format!("CMD fallo: {}", e)); Err(format!("Fallo crítico CMD: {}", e)) }
        Ok(Ok(Ok(out))) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() { Ok(stdout) }
            else {
                write_app_log("WARNING", &format!("CMD error: {}", stderr));
                Err(format!("CMD Error:\n{}{}", stderr, stdout))
            }
        }
    }
}

// ── WMIC — información de hardware/OS sin PowerShell ──────────────────────────

#[tauri::command]
pub async fn execute_wmic(query: String) -> Result<String, String> {
    let lower = query.trim().to_lowercase();

    let allowed_prefixes = [
        "cpu ", "memorychip ", "diskdrive ", "logicaldisk ", "os ",
        "computersystem ", "nic ", "nicconfig ", "process ", "service ",
        "startup ", "bios ", "baseboard ", "csproduct ", "useraccount ",
        "path win32_", "qfe ",
    ];
    if !allowed_prefixes.iter().any(|p| lower.starts_with(p)) {
        return Err(format!(
            "Query WMIC no permitida. Aliases seguros: cpu, os, diskdrive, memorychip, \
             nic, process, service, bios, csproduct, qfe, path Win32_*."
        ));
    }
    if lower.contains("/node:") || lower.contains(" delete ") || lower.contains(" call ") {
        return Err("WMIC: /node, delete y call no están permitidos en este modo.".to_string());
    }

    let args = parse_args(&query);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(60), // Aumentado a 60s
        tokio::task::spawn_blocking(move || {
            Command::new("wmic")
                .args(args)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        }),
    ).await;

    match result {
        Err(_) => Err("Timeout WMIC (60s).".to_string()),
        Ok(Err(e)) => Err(format!("Error spawn WMIC: {}", e)),
        Ok(Ok(Err(e))) => Err(format!("Error WMIC: {}", e)),
        Ok(Ok(Ok(out))) => Ok(String::from_utf8_lossy(&out.stdout).trim().to_string()),
    }
}

// ── NETSH — configuración de red y firewall ───────────────────────────────────

#[tauri::command]
pub async fn execute_netsh(args: String) -> Result<String, String> {
    let lower = args.to_lowercase();
    let blocklist = [
        "advfirewall set allprofiles state off",
        "set machine",
        "reset",
    ];
    for blocked in &blocklist {
        if lower.contains(blocked) {
            return Err(format!("SECURITY_BLOCK: netsh '{}' no permitido.", blocked));
        }
    }

    audit(&format!("[{}] [HOST:{}] [NETSH] {}", ts(), host(), &args[..args.len().min(200)]));

    let parsed_args = parse_args(&args);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(60), // Aumentado a 60s
        tokio::task::spawn_blocking(move || {
            Command::new("netsh")
                .args(parsed_args)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        }),
    ).await;

    match result {
        Err(_) => Err("Timeout netsh (60s).".to_string()),
        Ok(Err(e)) => Err(format!("Error spawn netsh: {}", e)),
        Ok(Ok(Err(e))) => Err(format!("Error netsh: {}", e)),
        Ok(Ok(Ok(out))) => Ok(String::from_utf8_lossy(&out.stdout).trim().to_string()),
    }
}

// ── REG.EXE — consultas de registro sin PowerShell ───────────────────────────

#[tauri::command]
pub async fn execute_reg(args: String, force_write: bool) -> Result<String, String> {
    let lower = args.trim().to_lowercase();
    let is_write = lower.starts_with("add ")
        || lower.starts_with("delete ")
        || lower.starts_with("import ")
        || lower.starts_with("restore ");

    if is_write && !force_write {
        return Err("SECURITY_BLOCK:reg write — usa force_write=true para operaciones de escritura en el registro.".to_string());
    }

    let op_type = if is_write { "REG_WRITE" } else { "REG_READ" };
    audit(&format!("[{}] [HOST:{}] [{}] {}", ts(), host(), op_type, &args[..args.len().min(300)]));

    let parsed_args = parse_args(&args);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30), // Aumentado a 30s
        tokio::task::spawn_blocking(move || {
            Command::new("reg")
                .args(parsed_args)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        }),
    ).await;

    match result {
        Err(_) => Err("Timeout reg.exe (30s).".to_string()),
        Ok(Err(e)) => Err(format!("Error spawn reg: {}", e)),
        Ok(Ok(Err(e))) => Err(format!("Error reg: {}", e)),
        Ok(Ok(Ok(out))) => {
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                Err(format!("Reg Error: {}", String::from_utf8_lossy(&out.stderr).trim()))
            }
        }
    }
}

// ── CSCRIPT — Windows Script Host para automatización COM / AD ────────────────

#[tauri::command]
pub async fn execute_cscript(script_content: String, force_execute: bool) -> Result<String, String> {
    let lower = script_content.to_lowercase();
    let blocklist = [
        "wscript.shell",
        "shell.application",
        "winhttp.winhttprequest",
        "msxml2.xmlhttp",
    ];
    let mut bypassed = false;
    for blocked in &blocklist {
        if lower.contains(blocked) {
            if force_execute {
                write_app_log("WARNING", &format!("CScript bypass autorizado: {}", blocked));
                bypassed = true;
                break;
            } else {
                return Err(format!("SECURITY_BLOCK:{}", blocked));
            }
        }
    }

    let op = if bypassed { "CSCRIPT_BYPASS" } else { "CSCRIPT_EXEC" };
    audit(&format!("[{}] [HOST:{}] [{}]", ts(), host(), op));

    let tmp_path = std::env::temp_dir()
        .join(format!("lucy_{}.vbs", Local::now().format("%Y%m%d%H%M%S")));
    std::fs::write(&tmp_path, &script_content)
        .map_err(|e| format!("Error escribiendo VBS temporal: {}", e))?;

    let tmp_str = tmp_path.to_string_lossy().to_string();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(120), // Aumentado a 120s
        tokio::task::spawn_blocking(move || {
            Command::new("cscript")
                .args(["//NoLogo", "//T:120", &tmp_str]) // Actualizado T:120
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        }),
    ).await;

    let _ = std::fs::remove_file(&tmp_path);

    match result {
        Err(_) => Err("Timeout cscript (120s).".to_string()),
        Ok(Err(e)) => Err(format!("Error spawn cscript: {}", e)),
        Ok(Ok(Err(e))) => Err(format!("Error cscript: {}", e)),
        Ok(Ok(Ok(out))) => {
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                Err(format!("CScript Error:\n{}", String::from_utf8_lossy(&out.stderr).trim()))
            }
        }
    }
}

// ── CONEXIONES DE RED ACTIVAS — netstat -ano ─────────────────────────────────

#[derive(Serialize, Clone, PartialEq, Eq, Hash)]
pub struct NetConnection {
    pub protocol:    String,
    pub local_addr:  String,
    pub local_port:  u16,
    pub remote_addr: String,
    pub remote_port: u16,
    pub state:       String,
    pub pid:         Option<u32>,
}

#[tauri::command]
pub async fn get_network_connections() -> Result<Vec<NetConnection>, String> {
    tokio::task::spawn_blocking(|| {
        let out = Command::new("netstat")
            .args(["-ano"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Error netstat: {}", e))?;

        let text = String::from_utf8_lossy(&out.stdout);
        let mut conns = Vec::new();

        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 { continue; }
            let proto = parts[0].to_uppercase();
            if proto != "TCP" && proto != "UDP" { continue; }

            let parse_addr = |s: &str| -> (String, u16) {
                if let Some(pos) = s.rfind(':') {
                    let addr = s[..pos].trim_matches(|c| c == '[' || c == ']').to_string();
                    let port = s[pos+1..].parse::<u16>().unwrap_or(0);
                    (addr, port)
                } else {
                    (s.to_string(), 0)
                }
            };

            let (local_addr, local_port) = parse_addr(parts[1]);

            if proto == "UDP" {
                let pid = parts.get(2).and_then(|p| p.parse::<u32>().ok());
                conns.push(NetConnection {
                    protocol: proto, local_addr, local_port,
                    remote_addr: String::new(), remote_port: 0,
                    state: "STATELESS".to_string(), pid,
                });
            } else if parts.len() >= 5 {
                let (remote_addr, remote_port) = parse_addr(parts[2]);
                let state = parts[3].to_string();
                let pid   = parts[4].parse::<u32>().ok();
                conns.push(NetConnection {
                    protocol: proto, local_addr, local_port,
                    remote_addr, remote_port, state, pid,
                });
            }
        }
        Ok(conns)
    }).await.map_err(|e| format!("Error interno netstat: {}", e))?
}

// ── WINDOWS EVENT LOG — wevtutil ─────────────────────────────────────────────

#[derive(Serialize)]
pub struct EventEntry {
    pub time:     String,
    pub level:    String,
    pub source:   String,
    pub event_id: String,
    pub message:  String,
}

#[tauri::command]
pub async fn get_event_log(
    log_name: String,
    count: u32,
    level: Option<String>,
) -> Result<Vec<EventEntry>, String> {
    let count = count.min(500);
    let log_clone = log_name.clone();

    let query = match level.as_deref() {
        Some("critical") => "*[System[Level=1]]".to_string(),
        Some("error")    => "*[System[Level=2]]".to_string(),
        Some("warn")     => "*[System[Level=3]]".to_string(),
        Some("info")     => "*[System[Level=4]]".to_string(),
        _                => String::new(),
    };

    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("wevtutil");
        cmd.arg("qe")
           .arg(&log_clone)
           .arg(format!("/c:{}", count))
           .arg("/rd:true")
           .arg("/f:text")
           .creation_flags(CREATE_NO_WINDOW);
        if !query.is_empty() {
            cmd.arg(format!("/q:{}", query));
        }

        let out = cmd.output().map_err(|e| format!("Error wevtutil: {}", e))?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        parse_wevtutil_text(&text)
    }).await.map_err(|e| format!("Error interno wevtutil: {}", e))?
}

fn parse_wevtutil_text(text: &str) -> Result<Vec<EventEntry>, String> {
    let mut entries: Vec<EventEntry> = Vec::new();
    let mut cur = EventEntry {
        time: String::new(), level: String::new(), source: String::new(),
        event_id: String::new(), message: String::new(),
    };
    let mut msg_buf: Vec<String> = Vec::new();
    let mut in_msg = false;

    let flush = |cur: &mut EventEntry, buf: &mut Vec<String>, entries: &mut Vec<EventEntry>| {
        if !cur.time.is_empty() {
            cur.message = buf.join(" ").chars().take(250).collect();
            entries.push(std::mem::replace(cur, EventEntry {
                time: String::new(), level: String::new(), source: String::new(),
                event_id: String::new(), message: String::new(),
            }));
            buf.clear();
        }
    };

    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("Event[") {
            flush(&mut cur, &mut msg_buf, &mut entries);
            in_msg = false;
        } else if let Some(v) = t.strip_prefix("Date:") {
            cur.time = v.trim().to_string(); in_msg = false;
        } else if let Some(v) = t.strip_prefix("Source:") {
            cur.source = v.trim().to_string(); in_msg = false;
        } else if let Some(v) = t.strip_prefix("Event ID:") {
            cur.event_id = v.trim().to_string(); in_msg = false;
        } else if let Some(v) = t.strip_prefix("Level:") {
            cur.level = v.trim().to_string(); in_msg = false;
        } else if let Some(v) = t.strip_prefix("Message:") {
            in_msg = true;
            let rest = v.trim();
            if !rest.is_empty() { msg_buf.push(rest.to_string()); }
        } else if in_msg && !t.is_empty() {
            msg_buf.push(t.to_string());
            if msg_buf.len() >= 4 { in_msg = false; }
        }
    }
    flush(&mut cur, &mut msg_buf, &mut entries);
    Ok(entries)
}

// ── TASKLIST — procesos activos con detalle ───────────────────────────────────

#[derive(Serialize, Clone, PartialEq, Eq, Hash)]
pub struct TaskEntry {
    pub name:    String,
    pub pid:     u32,
    pub session: String,
    pub mem_kb:  u64,
}

#[tauri::command]
pub async fn get_tasklist() -> Result<Vec<TaskEntry>, String> {
    tokio::task::spawn_blocking(|| {
        let out = Command::new("tasklist")
            .args(["/fo", "csv", "/nh"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Error tasklist: {}", e))?;

        let text = String::from_utf8_lossy(&out.stdout);
        let mut tasks = Vec::new();

        for line in text.lines() {
            let fields: Vec<&str> = line.split(',')
                .map(|f| f.trim().trim_matches('"'))
                .collect();
            if fields.len() < 5 { continue; }
            let pid    = fields[1].parse::<u32>().unwrap_or(0);
            let mem_kb = fields[4].replace([' ', 'K', ',', '.'], "")
                .trim().parse::<u64>().unwrap_or(0);
            tasks.push(TaskEntry {
                name:    fields[0].to_string(),
                pid,
                session: fields[2].to_string(),
                mem_kb,
            });
        }
        tasks.sort_by(|a, b| b.mem_kb.cmp(&a.mem_kb));
        Ok(tasks)
    }).await.map_err(|e| format!("Error interno tasklist: {}", e))?
}

// ── REGISTRO WINDOWS NATIVO — winreg ─────────────────────────────────────────

#[tauri::command]
pub fn read_registry_value(
    hive: String,
    key_path: String,
    value_name: String,
) -> Result<String, String> {
    use winreg::{RegKey, enums::*};

    let root = match hive.to_uppercase().as_str() {
        "HKLM" | "HKEY_LOCAL_MACHINE"  => RegKey::predef(HKEY_LOCAL_MACHINE),
        "HKCU" | "HKEY_CURRENT_USER"   => RegKey::predef(HKEY_CURRENT_USER),
        "HKCR" | "HKEY_CLASSES_ROOT"   => RegKey::predef(HKEY_CLASSES_ROOT),
        "HKU"  | "HKEY_USERS"          => RegKey::predef(HKEY_USERS),
        "HKCC" | "HKEY_CURRENT_CONFIG" => RegKey::predef(HKEY_CURRENT_CONFIG),
        _ => return Err(format!("Hive desconocido: {}. Usa HKLM, HKCU, HKCR, HKU o HKCC.", hive)),
    };

    let subkey = root.open_subkey(&key_path)
        .map_err(|e| format!("Clave '{key_path}' no encontrada: {e}"))?;

    let result: String = if value_name.is_empty() {
        subkey.get_value("").map_err(|e| format!("Valor por defecto no encontrado: {e}"))?
    } else {
        subkey.get_value(&value_name)
            .or_else(|_| subkey.get_value::<u32, _>(&value_name).map(|v| v.to_string()))
            .map_err(|e| format!("Valor '{value_name}' no encontrado: {e}"))?
    };

    Ok(result)
}

#[tauri::command]
pub fn list_registry_key(hive: String, key_path: String) -> Result<serde_json::Value, String> {
    use winreg::{RegKey, enums::*};
    use serde_json::json;

    let root = match hive.to_uppercase().as_str() {
        "HKLM" | "HKEY_LOCAL_MACHINE"  => RegKey::predef(HKEY_LOCAL_MACHINE),
        "HKCU" | "HKEY_CURRENT_USER"   => RegKey::predef(HKEY_CURRENT_USER),
        "HKCR" | "HKEY_CLASSES_ROOT"   => RegKey::predef(HKEY_CLASSES_ROOT),
        "HKU"  | "HKEY_USERS"          => RegKey::predef(HKEY_USERS),
        "HKCC" | "HKEY_CURRENT_CONFIG" => RegKey::predef(HKEY_CURRENT_CONFIG),
        _ => return Err(format!("Hive desconocido: {}", hive)),
    };

    let subkey = root.open_subkey(&key_path)
        .map_err(|e| format!("Clave '{key_path}' no encontrada: {e}"))?;

    let subkeys: Vec<String> = subkey.enum_keys()
        .filter_map(|k| k.ok())
        .collect();

    let values: Vec<serde_json::Value> = subkey.enum_values()
        .filter_map(|v| v.ok())
        .map(|(name, data)| json!({ "name": name, "value": format!("{:?}", data) }))
        .collect();

    Ok(json!({ "subkeys": subkeys, "values": values }))
}

// ── FILE OPERATIONS — lectura/escritura nativa ───────────────────────────────

#[tauri::command]
pub async fn read_file_content(path: String) -> Result<String, String> {
    use once_cell::sync::Lazy;
    static BPE: Lazy<tiktoken_rs::CoreBPE> = Lazy::new(|| tiktoken_rs::cl100k_base().unwrap());

    let resolved = resolve_path(&path);
    let p = resolved.as_path();
    if !p.exists() {
        return Err(format!("Archivo no encontrado: {}", resolved.display()));
    }
    let meta = std::fs::metadata(p).map_err(|e| format!("Error al leer metadata: {}", e))?;
    if meta.len() > 1024 * 1024 { // Bypass general para archivos bestialmente gigantes
        return Err(format!(
            "Archivo demasiado grande ({:.1} MB). Máximo permitido 1MB físico. Usa <TOOL>readlines</TOOL>.",
            meta.len() as f64 / 1024.0 / 1024.0
        ));
    }
    
    let content = std::fs::read_to_string(p).map_err(|e| format!("Error al leer archivo: {}", e))?;

    let tokens = BPE.encode_with_special_tokens(&content).len();
    if tokens > 20000 {
        return Err(format!("El archivo es demasiado extenso ({} tokens). Excede el límite de lectura monolítica de 20,000. Por favor, usa la herramienta <TOOL>readlines:{}:1:200</TOOL> en su lugar.", tokens, resolved.display()));
    }

    audit(&format!("[{}] [HOST:{}] [FILE_READ] {} ({} tokens)", ts(), host(), resolved.display(), tokens));
    Ok(content)
}

#[tauri::command]
pub async fn read_file_lines(path: String, start: usize, count: usize) -> Result<String, String> {
    use ropey::Rope;
    use std::fs::File;
    use std::io::BufReader;
    let resolved = resolve_path(&path);
    let p = resolved.as_path();
    if !p.exists() {
        return Err(format!("Archivo no encontrado: {}", resolved.display()));
    }
    let file = File::open(p).map_err(|e| format!("Error abriendo archivo: {}", e))?;
    let rope = Rope::from_reader(BufReader::new(file)).map_err(|e| format!("Error Ropey: {}", e))?;
    
    let total = rope.len_lines();
    let s = if start == 0 { 0 } else { start - 1 };
    let e = (s + count).min(total);
    if s >= total {
        return Ok(format!("[Archivo tiene {} líneas, rango solicitado {} fuera de alcance]", total, start));
    }
    audit(&format!("[{}] [HOST:{}] [FILE_READ_LINES] {} ({}..{})", ts(), host(), resolved.display(), s+1, e));
    let mut result = Vec::new();
    for (i, line) in rope.lines_at(s).take(e - s).enumerate() {
        let line_str = line.to_string();
        result.push(format!("{:>4}│ {}", s + i + 1, line_str.trim_end_matches(&['\n', '\r'][..])));
    }
    Ok(format!("[Líneas {}-{} de {}]\n{}", s+1, e, total, result.join("\n")))
}

#[tauri::command]
pub async fn write_file_content(path: String, content: String, force: bool) -> Result<String, String> {
    let resolved = resolve_path(&path);
    let p = resolved.as_path();

    // DEFENSA: Resolver la ruta para evitar Path Traversal (ej. \..\..\Windows)
    let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let lower = canonical.to_string_lossy().to_lowercase().replace('/', "\\");

    let blocked_paths = ["c:\\windows\\", "c:\\program files\\", "c:\\program files (x86)\\"];
    for bp in &blocked_paths {
        if lower.starts_with(bp) {
            return Err(format!("Escritura bloqueada en ruta del sistema: {}", bp));
        }
    }

    if p.exists() && !force {
        return Err(format!(
            "El archivo ya existe: {}. Usa force=true para sobrescribir.",
            resolved.display()
        ));
    }

    if let Some(parent) = p.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Error al crear directorio: {}", e))?;
        }
    }

    audit(&format!("[{}] [HOST:{}] [FILE_WRITE] {} ({} bytes, force={})",
        ts(), host(), resolved.display(), content.len(), force));

    std::fs::write(p, &content)
        .map_err(|e| format!("Error al escribir archivo: {}", e))?;

    Ok(format!("✓ Archivo escrito: {} ({} bytes)", resolved.display(), content.len()))
}

#[tauri::command]
pub async fn list_directory(path: String) -> Result<Vec<serde_json::Value>, String> {
    use serde_json::json;
    use radix_trie::Trie;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use once_cell::sync::Lazy;
    use std::time::{Instant, Duration};

    struct CacheEntry {
        entries: Vec<serde_json::Value>,
        timestamp: Instant,
    }

    static DIR_CACHE: Lazy<Arc<Mutex<Trie<String, CacheEntry>>>> = Lazy::new(|| Arc::new(Mutex::new(Trie::new())));

    let resolved = resolve_path(&path);
    let p = resolved.as_path();
    let norm_path = p.to_string_lossy().to_string();

    {
        let cache = DIR_CACHE.lock().await;
        if let Some(entry) = cache.get(&norm_path) {
            if entry.timestamp.elapsed() < Duration::from_secs(60) {
                return Ok(entry.entries.clone());
            }
        }
    }

    if !p.exists() {
        return Err(format!("Directorio no encontrado: {}", path));
    }
    if !p.is_dir() {
        return Err(format!("La ruta no es un directorio: {}", path));
    }

    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(&path)
        .map_err(|e| format!("Error al listar directorio: {}", e))?;

    for entry in read_dir.take(500) {
        if let Ok(e) = entry {
            let meta = e.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta.as_ref()
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    let dt: chrono::DateTime<chrono::Local> = t.into();
                    dt.format("%Y-%m-%d %H:%M:%S").to_string()
                })
                .unwrap_or_default();

            entries.push(json!({
                "name": e.file_name().to_string_lossy(),
                "is_dir": is_dir,
                "size": size,
                "modified": modified
            }));
        }
    }

    entries.sort_by(|a, b| {
        let a_dir = a["is_dir"].as_bool().unwrap_or(false);
        let b_dir = b["is_dir"].as_bool().unwrap_or(false);
        b_dir.cmp(&a_dir)
            .then(a["name"].as_str().unwrap_or("").to_lowercase().cmp(
                &b["name"].as_str().unwrap_or("").to_lowercase()
            ))
    });

    {
        let mut cache = DIR_CACHE.lock().await;
        cache.insert(norm_path, CacheEntry { entries: entries.clone(), timestamp: Instant::now() });
    }

    Ok(entries)
}

// ── SEARCH FILES — búsqueda de texto en archivos (grep equivalente) ──────────

#[tauri::command]
pub async fn search_files(
    directory: String,
    pattern: String,
    file_glob: Option<String>,
    max_results: Option<usize>,
) -> Result<String, String> {
    use std::path::Path;
    use aho_corasick::AhoCorasick;

    let dir = Path::new(&directory);
    if !dir.exists() || !dir.is_dir() {
        return Err(format!("Directorio no encontrado: {}", directory));
    }

    let patterns: Vec<String> = pattern.split('|').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if patterns.is_empty() {
        return Err("No se proporcionó un patrón válido.".to_string());
    }

    let ac = AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build(&patterns)
        .map_err(|e| format!("Error compilando motor Aho-Corasick: {}", e))?;

    let max = max_results.unwrap_or(100).min(200);
    let glob_ext: Option<Vec<String>> = file_glob.map(|g| {
        g.split(',')
            .map(|s| s.trim().trim_start_matches('*').trim_start_matches('.').to_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    });

    let mut results = Vec::new();

    fn walk(
        dir: &Path, ac: &AhoCorasick, glob_ext: &Option<Vec<String>>,
        results: &mut Vec<String>, max: usize, depth: usize,
    ) {
        if depth > 6 || results.len() >= max { return; }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries {
            if results.len() >= max { break; }
            let Ok(e) = entry else { continue };
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();

            if path.is_dir() {
                if name.starts_with('.') || name == "node_modules" || name == "target"
                    || name == "build" || name == "dist" || name == ".svelte-kit" {
                    continue;
                }
                walk(&path, ac, glob_ext, results, max, depth + 1);
                continue;
            }

            if let Some(exts) = glob_ext {
                let ext = path.extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if !exts.iter().any(|g| g == &ext) { continue; }
            }

            let Ok(meta) = std::fs::metadata(&path) else { continue };
            if meta.len() > 1_048_576 { continue; } // 1 MB max

            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            for (i, line) in content.lines().enumerate() {
                if results.len() >= max { break; }
                if ac.is_match(line) {
                    let rel = path.strip_prefix(dir).unwrap_or(&path);
                    results.push(format!("{}:{}| {}",
                        rel.display(), i + 1,
                        if line.len() > 200 { &line[..200] } else { line }
                    ));
                }
            }
        }
    }

    audit(&format!("[{}] [HOST:{}] [SEARCH_FILES] dir={} pattern=\"{:?}\" glob={:?}",
        ts(), host(), &directory, &patterns, &glob_ext));

    walk(dir, &ac, &glob_ext, &mut results, max, 0);

    if results.is_empty() {
        Ok(format!("[Sin coincidencias para \"{}\" en {}]", pattern, directory))
    } else {
        Ok(format!("[{} coincidencias para \"{}\"]\n{}", results.len(), pattern, results.join("\n")))
    }
}

// ── EDIT FILE — edición quirúrgica (find-and-replace) ────────────────────────

#[tauri::command]
pub async fn edit_file(
    path: String,
    old_string: String,
    new_string: String,
    replace_all: Option<bool>,
) -> Result<String, String> {
    use similar::{ChangeTag, TextDiff};

    let resolved = resolve_path(&path);
    let p = resolved.as_path();
    if !p.exists() {
        return Err(format!("Archivo no encontrado: {}", resolved.display()));
    }

    // DEFENSA: Resolver la ruta para evitar Path Traversal (ej. \..\..\Windows)
    let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let lower = canonical.to_string_lossy().to_lowercase().replace('/', "\\");

    let blocked = ["c:\\windows\\", "c:\\program files\\", "c:\\program files (x86)\\"];
    for bp in &blocked {
        if lower.starts_with(bp) {
            return Err(format!("Edición bloqueada en ruta del sistema: {}", bp));
        }
    }

    let content = std::fs::read_to_string(p)
        .map_err(|e| format!("Error al leer archivo: {}", e))?;

    if !content.contains(&old_string) {
        let lines: Vec<&str> = content.lines().collect();
        return Err(format!(
            "No se encontró el texto a reemplazar en {}. El archivo tiene {} líneas.\n\
            Primeras 3 líneas:\n{}\n\
            Sugerencia: usa readlines para ver el contenido actual.",
            resolved.display(), lines.len(),
            lines.iter().take(3).cloned().collect::<Vec<&str>>().join("\n")
        ));
    }

    let count = content.matches(&old_string).count();
    let do_all = replace_all.unwrap_or(false);

    if count > 1 && !do_all {
        return Err(format!(
            "Se encontraron {} coincidencias del texto en {}. Usa replace_all=true para reemplazar todas, o proporciona más contexto para hacer la búsqueda única.",
            count, resolved.display()
        ));
    }

    let new_content = if do_all {
        content.replace(&old_string, &new_string)
    } else {
        content.replacen(&old_string, &new_string, 1)
    };

    let diff = TextDiff::from_lines(&content, &new_content);
    let mut diff_str = String::new();
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        diff_str.push_str(&format!("{}{}", sign, change));
    }

    audit(&format!("[{}] [HOST:{}] [FILE_EDIT] {} (replaced {} occurrence(s), {} -> {} bytes)",
        ts(), host(), resolved.display(), if do_all { count } else { 1 },
        old_string.len(), new_string.len()));

    std::fs::write(p, &new_content)
        .map_err(|e| format!("Error al escribir archivo: {}", e))?;

    let limited_diff = if diff_str.len() > 3000 {
        format!("{}... [Diff Truncado]", &diff_str[..3000])
    } else {
        diff_str
    };

    Ok(format!("ok: editado {} ({} reemplazo(s))\n## Unified Diff:\n```diff\n{}\n```",
        path, if do_all { count } else { 1 }, limited_diff))
}

// ── OPEN VS CODE — herramienta UI para Agentes ────────────────────────

#[tauri::command]
pub fn open_vscode(path: String) -> Result<(), String> {
    std::process::Command::new("code")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("No se pudo abrir VS Code. Asegúrate de tener 'code' en el PATH. Error: {}", e))?;
    Ok(())
}

// ── TREE-SITTER — abstracción sintáctica AST ─────────────────────────────────

#[tauri::command]
pub async fn analyze_code(path: String) -> Result<String, String> {
    use tree_sitter::Parser;
    
    let path = std::path::Path::new(&path);
    if !path.exists() {
        return Err(format!("Archivo no encontrado: {:?}", path));
    }
    
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let is_rust = ext == "rs";
    let is_js = ext == "js" || ext == "ts" || ext == "svelte";
    
    if !is_rust && !is_js {
        return Err("Solo se soporta AST para Rust (.rs) o JavaScript (.js, .ts, .svelte)".to_string());
    }
    
    let source = std::fs::read_to_string(&path).map_err(|e| format!("Error abriendo archivo: {}", e))?;
        
    let mut parser = Parser::new();
    let language = if is_rust {
        tree_sitter_rust::LANGUAGE.into()
    } else {
        tree_sitter_javascript::LANGUAGE.into()
    };
    
    parser.set_language(&language).map_err(|_| "Error configurando parser".to_string())?;
        
    let tree = parser.parse(&source, None).ok_or("Error generando AST")?;
    let root = tree.root_node();
    
    let mut summary = String::new();
    summary.push_str(&format!("### Abstract Syntax Tree Summary for {:?}\n\n", path));
    
    fn format_node(node: tree_sitter::Node, source_code: &[u8], depth: usize, summary: &mut String) {
        if depth > 4 { return; } // Limitar anidamiento profundo
        let kind = node.kind();
        let is_interesting = kind == "function_item" || kind == "impl_item" || kind == "struct_item" || 
                             kind == "function_declaration" || kind == "class_declaration" || kind == "method_definition";
                             
        if is_interesting {
            if let Ok(text) = node.utf8_text(source_code) {
                let first_line = text.lines().next().unwrap_or("").trim_end_matches('{').trim();
                summary.push_str(&format!("{}• {} (lines {}..{})\n", "  ".repeat(depth), first_line, node.start_position().row + 1, node.end_position().row + 1));
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            format_node(child, source_code, depth + 1, summary);
        }
    }
    
    format_node(root, source.as_bytes(), 0, &mut summary);
    if !summary.contains("•") { summary.push_str("> No se detectaron funciones/clases principales en AST root.\n"); }
    
    Ok(summary)
}

// ── SYSTEM DIFFING — Analizador temporal abstracto ──────────────────────────

use std::collections::HashSet;
use tokio::sync::Mutex;
use std::sync::Arc;
use once_cell::sync::Lazy;

static TASKS_CACHE: Lazy<Arc<Mutex<Option<Vec<TaskEntry>>>>> = Lazy::new(|| Arc::new(Mutex::new(None)));
static NET_CACHE: Lazy<Arc<Mutex<Option<Vec<NetConnection>>>>> = Lazy::new(|| Arc::new(Mutex::new(None)));

#[tauri::command]
pub async fn system_diff(category: String) -> Result<String, String> {
    if category == "tasks" || category == "processes" {
        let current = get_tasklist().await?;
        let mut cache = TASKS_CACHE.lock().await;

        if let Some(prev) = cache.take() {
            cache.replace(current.clone()); // Update cache to current

            let prev_pids: HashSet<u32> = prev.iter().map(|p| p.pid).collect();
            let curr_pids: HashSet<u32> = current.iter().map(|p| p.pid).collect();

            let died: Vec<_> = prev.into_iter().filter(|p| !curr_pids.contains(&p.pid)).collect();
            let born: Vec<_> = current.into_iter().filter(|p| !prev_pids.contains(&p.pid)).collect();

            let mut out = format!("--- DIFF DE PROCESOS ({} murieron, {} nacieron) ---\n", died.len(), born.len());
            for d in died.iter().take(50) { out.push_str(&format!("[-] MURIÓ: {} (PID: {})\n", d.name, d.pid)); }
            for b in born.iter().take(50) { out.push_str(&format!("[+] NACIÓ: {} (PID: {})\n", b.name, b.pid)); }
            return Ok(out);
        } else {
            cache.replace(current);
            return Ok("SNAPSHOT INICIAL DE PROCESOS ESTABLECIDA. Realiza tus comandos y vuelve a llamar a system_diff.".to_string());
        }
    } else if category == "network" || category == "ports" {
        let current = get_network_connections().await?;
        let mut cache = NET_CACHE.lock().await;

        if let Some(prev) = cache.take() {
            cache.replace(current.clone()); // Update cache

            let prev_set: HashSet<NetConnection> = prev.into_iter().collect();
            let curr_set: HashSet<NetConnection> = current.into_iter().collect();

            let died: Vec<_> = prev_set.difference(&curr_set).collect();
            let born: Vec<_> = curr_set.difference(&prev_set).collect();

            let mut out = format!("--- DIFF DE RED ({} cerradas, {} abiertas) ---\n", died.len(), born.len());
            for d in died.iter().take(50) { out.push_str(&format!("[-] CERRÓ: {} {}:{} -> {}:{} (PID: {:?})\n", d.protocol, d.local_addr, d.local_port, d.remote_addr, d.remote_port, d.pid)); }
            for b in born.iter().take(50) { out.push_str(&format!("[+] ABRIÓ: {} {}:{} -> {}:{} (PID: {:?})\n", b.protocol, b.local_addr, b.local_port, b.remote_addr, b.remote_port, b.pid)); }
            return Ok(out);
        } else {
            cache.replace(current);
            return Ok("SNAPSHOT INICIAL DE RED ESTABLECIDA. Realiza tus comandos y vuelve a llamar a system_diff.".to_string());
        }
    }

    Err("Categoría no soportada. Usa 'tasks' o 'network'.".to_string())
}

// ── PANIC BUTTON ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn panic_kill_all() -> Result<(), String> {
    audit(&format!("[{}] [HOST:local] [PANIC] Deteniendo todos los procesos de fondo.", ts()));
    // Detener la ejecución del intérprete IA es manejado en Svelte.
    // Si hubiesen procesos PTY en NexShell u otros locales de larga duración,
    // se detendrían aquí. Como mitigación base, imprimimos la acción.
    Ok(())
}


#[tauri::command]
pub async fn search_web(query: String) -> Result<String, String> {
    let url = format!("https://html.duckduckgo.com/html/?q={}", urlencoding::encode(&query));
    let res = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Error fetching web: {}", e))?;

    if res.status().as_u16() >= 400 {
        return Err(format!("Web search returned HTTP {}", res.status()));
    }

    let html = res.text().await.unwrap_or_default();
    let re = regex::Regex::new(r#"(?s)<a class="result__snippet[^>]*>(.*?)</a>"#).unwrap();
    let re_url = regex::Regex::new(r#"(?s)<a class="result__url[^>]*>(.*?)</a>"#).unwrap();
    
    let mut results = Vec::new();
    let snippets: Vec<_> = re.captures_iter(&html).collect();
    let urls: Vec<_> = re_url.captures_iter(&html).collect();
    
    for i in 0..snippets.len().min(5).min(urls.len()) {
        let mut snip = snippets[i][1].to_string();
        snip = snip.replace("<b>", "").replace("</b>", "").replace("&#x27;", "'").replace("&quot;", "\"").replace("&amp;", "&");
        let url = urls[i][1].trim().to_string();
        results.push(format!("* {}: {}", url, snip));
    }
    
    if results.is_empty() {
        return Ok("No results found or HTML format changed.".to_string());
    }
    
    Ok(results.join("\n\n"))
}
