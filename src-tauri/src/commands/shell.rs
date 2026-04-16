// ── SHELL — PowerShell local + streaming SSH/WinRM interactivo ─────────────────

use std::process::{Command, Stdio};
use std::os::windows::process::CommandExt;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::Emitter;
use sysinfo::System;
use chrono::Local;
use crate::state::{CREATE_NO_WINDOW, STREAM_SESSIONS, STREAM_PIDS};
use crate::utils::logging::{write_app_log, rotate_audit_log, get_logs_dir};
use crate::utils::shell::{strip_ansi, ensure_trusted_host};

// ── POWERSHELL LOCAL CON AUDIT LOG ────────────────────────────────────────────

#[tauri::command]
pub async fn execute_powershell(script: String, bypass_token: Option<String>, timeout_secs: Option<u64>) -> Result<String, String> {
    use std::fs::OpenOptions;
    let script_lower = script.to_lowercase();
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let user = System::host_name().unwrap_or_else(|| "LocalSystem".to_string());
    let audit_path = get_logs_dir().join("lucy_audit.log");

    rotate_audit_log();

    let mut log_file = OpenOptions::new().create(true).append(true).open(&audit_path)
        .map_err(|e| { write_app_log("ERROR", &format!("No se pudo abrir audit log: {}", e)); format!("Error audit log: {}", e) })?;

    let blocklist = [
        "remove-item -recurse", "rm -rf", "format-volume", "clear-disk",
        "net user", "disable-netadapter", "stop-process -name lsass",
        "-encodedcommand", "invoke-expression", "iex ", "iex(", "&{", "& {",
        "downloadstring", "downloadfile", "webclient",
    ];
    let mut was_blocked_but_bypassed = false;

    if let Some(ref token) = bypass_token {
        let mut tokens_map = crate::state::BYPASS_TOKENS.lock().unwrap();
        if let Some(authorized_script) = tokens_map.get(token) {
            if authorized_script == &script {
                was_blocked_but_bypassed = true;
                let _ = writeln!(log_file, "[{}] [HOST: {}] [AUTHORIZED_BYPASS] Token consumido para: {}", timestamp, user, script);
                write_app_log("WARNING", "Usuario autorizó comando destructivo vía token");
                tokens_map.remove(token);
            }
        }
    }

    if !was_blocked_but_bypassed {
        for blocked in blocklist.iter() {
            if script_lower.contains(blocked) {
                let new_token = format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
                crate::state::BYPASS_TOKENS.lock().unwrap().insert(new_token.clone(), script.clone());

                let _ = writeln!(log_file, "[{}] [HOST: {}] [BLOCKED_PENDING_AUTH] Script: {}", timestamp, user, script);
                write_app_log("WARNING", &format!("Bloqueado comando prohibido: {}", blocked));
                return Err(format!("SECURITY_BLOCK:{}:{}", new_token, blocked));
            }
        }
    }

    if was_blocked_but_bypassed {
        let _ = writeln!(log_file, "[{}] [HOST: {}] [EXECUTED_AFTER_BYPASS] Script: {}", timestamp, user, script);
    } else {
        let _ = writeln!(log_file, "[{}] [HOST: {}] [EXECUTED] Script: {}", timestamp, user, script);
    }

    let script_clone = script.clone();
    let timeout_val = timeout_secs.unwrap_or(60);
    let output_result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_val),
        tokio::task::spawn_blocking(move || {
            let cwd = crate::state::GLOBAL_CWD.read().map(|c| c.clone()).unwrap_or_else(|_| "C:\\".to_string());
            Command::new("powershell")
                .current_dir(cwd)
                .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
                .arg("-Command").arg(&script_clone)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        })
    ).await;

    let output = match output_result {
        Err(_) => {
            write_app_log("WARNING", &format!("PowerShell timeout: comando tardó más de {} segundos", timeout_val));
            return Err(format!("Timeout: el comando tardó más de {} segundos y fue cancelado.", timeout_val));
        }
        Ok(Err(e)) => { return Err(format!("Error interno spawn: {}", e)); }
        Ok(Ok(Err(e))) => {
            write_app_log("ERROR", &format!("Fallo PowerShell: {}", e));
            return Err(format!("Fallo crítico: {}", e));
        }
        Ok(Ok(Ok(out))) => out,
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Success criteria: exit code 0 ⇒ success.
    // If exit != 0 BUT stdout has content, treat as partial success (Get-ChildItem -Recurse
    // commonly emits stderr for ACL-denied dirs while still returning useful results).
    // Returning Err here would force the agent into useless retries.
    if output.status.success() {
        if stderr.trim().is_empty() {
            Ok(stdout)
        } else {
            // Success with stderr noise — append as warning footer so the agent sees both.
            Ok(format!("{}\n\n[stderr warnings]\n{}", stdout, stderr.trim()))
        }
    } else if !stdout.trim().is_empty() {
        // Non-zero exit but we got output — return it with a warning footer instead of erroring.
        write_app_log("WARNING", &format!("PowerShell non-zero exit with output. stderr: {}",
            stderr.trim()));
        Ok(format!("{}\n\n[stderr warnings — partial results, exit non-zero]\n{}",
            stdout, stderr.trim()))
    } else {
        let err_msg = if stderr.trim().is_empty() { String::from("(no output)") } else { stderr.clone() };
        write_app_log("WARNING", &format!("PowerShell error: {}", err_msg));
        Err(format!("PowerShell Error:\n{}", err_msg))
    }
}

// ── STREAMING SSH/WinRM CON INPUT INTERACTIVO ─────────────────────────────────

/// Inicia un proceso SSH (Linux) o WinRM (Windows) en background y emite chunks
/// vía eventos Tauri. El frontend escucha "ssh-out-{session_id}", "ssh-err-{session_id}",
/// "ssh-done-{session_id}". Permite input interactivo (sudo, y/n) vía send_shell_input.
#[tauri::command]
pub async fn stream_shell_cmd(
    window: tauri::Window,
    session_id: String,
    host: String,
    username: String,
    command: String,
    host_type: String,
    port: Option<u16>,
    password: Option<String>,
    key_path: Option<String>,
) -> Result<(), String> {
    if host_type == "linux" {
        let port_str = port.unwrap_or(22).to_string();
        let key_path_clone = key_path.clone();
        let host_clone = host.clone();
        let username_clone = username.clone();
        let command_clone = command.clone();
        let session_id_clone = session_id.clone();

        let mut child = tokio::task::spawn_blocking(move || {
            let mut cmd = Command::new("ssh");
            // -tt fuerza PTY para comandos interactivos (sudo, python, etc.)
            cmd.arg("-tt")
               .arg("-o").arg("StrictHostKeyChecking=accept-new")
               .arg("-o").arg("ConnectTimeout=10")
               .arg("-o").arg("ServerAliveInterval=10")
               .arg("-o").arg("LogLevel=ERROR")
               .arg("-p").arg(&port_str);
            if let Some(ref kp) = key_path_clone { if !kp.is_empty() { cmd.arg("-i").arg(kp); } }
            cmd.arg(format!("{}@{}", username_clone, host_clone))
               .arg(&command_clone)
               .stdin(Stdio::piped())
               .stdout(Stdio::piped())
               .stderr(Stdio::piped())
               .creation_flags(CREATE_NO_WINDOW)
               .spawn()
        }).await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("Error al iniciar SSH streaming: {}", e))?;

        let pid    = child.id();
        let stdin  = child.stdin.take().ok_or("stdin no disponible")?;
        let stdout = child.stdout.take().ok_or("stdout no disponible")?;
        let stderr = child.stderr.take().ok_or("stderr no disponible")?;

        STREAM_SESSIONS.lock().unwrap().insert(session_id.clone(), Arc::new(StdMutex::new(stdin)));
        STREAM_PIDS.lock().unwrap().insert(session_id.clone(), pid);

        // Capturar tiempo de inicio para calcular duración del comando
        let start_time = std::time::Instant::now();

        // Hilo lector de stdout — emite chunks; al EOF captura exit code y duración
        // (SSH con -tt propaga el exit code del comando remoto al proceso local)
        let win_out = window.clone();
        let sid_out = session_id.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut buf = [0u8; 2048];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let raw   = String::from_utf8_lossy(&buf[..n]).to_string();
                        let clean = strip_ansi(&raw);
                        if !clean.is_empty() {
                            let _ = win_out.emit(&format!("ssh-out-{}", sid_out), &clean);
                        }
                    }
                }
            }
            // Limpiar sesión, luego esperar exit code real del proceso SSH
            STREAM_SESSIONS.lock().unwrap().remove(&sid_out);
            STREAM_PIDS.lock().unwrap().remove(&sid_out);
            let exit_code   = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            let duration_ms = start_time.elapsed().as_millis() as u64;
            let _ = win_out.emit(
                &format!("ssh-done-{}", sid_out),
                format!(r#"{{"exit_code":{},"duration_ms":{}}}"#, exit_code, duration_ms),
            );
        });

        // Hilo lector de stderr — prompts de sudo/password llegan aquí en muchos sistemas
        let win_err = window.clone();
        let sid_err = session_id_clone;
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut buf = [0u8; 512];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let raw   = String::from_utf8_lossy(&buf[..n]).to_string();
                        let clean = strip_ansi(&raw);
                        if !clean.is_empty() {
                            let _ = win_err.emit(&format!("ssh-err-{}", sid_err), &clean);
                        }
                    }
                }
            }
        });

        Ok(())
    } else {
        // Windows WinRM — streaming por líneas
        ensure_trusted_host(&host);
        let pwd = password.unwrap_or_default();
        let pw_esc = pwd.replace('\'', "''");
        // Inyectar marcador __LUCY_EXIT al final del ScriptBlock para capturar
        // el exit code del último comando nativo ejecutado en el host remoto.
        let ps = format!(
            "$pass = ConvertTo-SecureString '{}' -AsPlainText -Force; \
             $cred = New-Object System.Management.Automation.PSCredential('{}', $pass); \
             Invoke-Command -ComputerName '{}' -Credential $cred \
               -ScriptBlock {{ $ErrorActionPreference='Continue'; {}; \
               $__ec=if($LASTEXITCODE){{$LASTEXITCODE}}else{{if($?){{0}}else{{1}}}}; \
               Write-Host ('__LUCY_EXIT:'+$__ec) -NoNewline }} -ErrorAction Stop",
            pw_esc, username, host, command
        );
        let session_id_clone = session_id.clone();

        let mut child = tokio::task::spawn_blocking(move || {
            Command::new("powershell")
                .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
                .arg("-NonInteractive").arg("-Command").arg(&ps)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
        }).await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("Error al iniciar WinRM streaming: {}", e))?;

        let pid_win = child.id();
        let stdin   = child.stdin.take().ok_or("stdin no disponible")?;
        let stdout  = child.stdout.take().ok_or("stdout no disponible")?;
        let stderr  = child.stderr.take().ok_or("stderr no disponible")?;

        STREAM_SESSIONS.lock().unwrap().insert(session_id.clone(), Arc::new(StdMutex::new(stdin)));
        STREAM_PIDS.lock().unwrap().insert(session_id.clone(), pid_win);

        let start_time_win = std::time::Instant::now();

        let win_out = window.clone();
        let sid_out = session_id.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            let mut exit_code: i32 = 0;
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        // Detectar marcador de exit code; no reenviarlo al frontend
                        if let Some(code_str) = l.trim().strip_prefix("__LUCY_EXIT:") {
                            exit_code = code_str.trim().parse().unwrap_or(0);
                        } else {
                            let _ = win_out.emit(&format!("ssh-out-{}", sid_out), l + "\n");
                        }
                    }
                    Err(_) => break,
                }
            }
            STREAM_SESSIONS.lock().unwrap().remove(&sid_out);
            STREAM_PIDS.lock().unwrap().remove(&sid_out);
            let duration_ms = start_time_win.elapsed().as_millis() as u64;
            let _ = win_out.emit(
                &format!("ssh-done-{}", sid_out),
                format!(r#"{{"exit_code":{},"duration_ms":{}}}"#, exit_code, duration_ms),
            );
        });

        let win_err = window.clone();
        let sid_err = session_id_clone;
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(l)  => { let _ = win_err.emit(&format!("ssh-err-{}", sid_err), l + "\n"); }
                    Err(_) => break,
                }
            }
        });

        Ok(())
    }
}

/// Envía texto a stdin del proceso de streaming activo (respuesta a prompts interactivos).
#[tauri::command]
pub fn send_shell_input(session_id: String, input: String) -> Result<(), String> {
    let map = STREAM_SESSIONS.lock().unwrap();
    if let Some(stdin_arc) = map.get(&session_id) {
        let mut stdin = stdin_arc.lock().unwrap();
        writeln!(*stdin, "{}", input)
            .map_err(|e| format!("Error al enviar input: {}", e))?;
        Ok(())
    } else {
        Err(format!("Sesión {} no encontrada o ya terminó", session_id))
    }
}

/// Cancela la sesión de streaming: cierra stdin y mata el árbol de procesos con taskkill /F /T.
#[tauri::command]
pub fn kill_shell_session(session_id: String) {
    STREAM_SESSIONS.lock().unwrap().remove(&session_id);
    if let Some(pid) = STREAM_PIDS.lock().unwrap().remove(&session_id) {
        let _ = Command::new("taskkill")
            .arg("/F").arg("/T").arg("/PID").arg(pid.to_string())
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
}
