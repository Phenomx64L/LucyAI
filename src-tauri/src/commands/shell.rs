// ── SHELL — PowerShell local + streaming SSH/WinRM interactivo ─────────────────

use std::process::{Command, Stdio};
use std::os::windows::process::CommandExt;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::Emitter;
use sysinfo::System;
use chrono::Local;
use crate::state::{CREATE_NO_WINDOW, STREAM_SESSIONS, StreamSession};
use crate::utils::logging::{write_app_log, rotate_audit_log, get_logs_dir};
use crate::utils::shell::{strip_ansi, ensure_trusted_host, spawn_winrm_streaming};
use crate::commands::hosts::{validate_host, validate_username, validate_password};

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

    // SECURITY: blocklist strings are XOR-obfuscated at compile time so they
    // don't appear verbatim in `strings lucy.exe`. An attacker dumping
    // strings gets garbage; the blocklist is decoded only inside this
    // function. `to_string()` lifts the obfstr-temporaries onto the heap so
    // they live long enough to be borrowed by the iteration loop below.
    use obfstr::obfstr as s;
    // S10 audit (May 2026): explicit UAC-elevation patterns. The guardrail
    // layer also flags these but having them in the obfstr blocklist means
    // they trigger the same bypass-token flow as other destructive verbs,
    // surfacing a clear "Lucy quiere elevar a admin — autorízalo" UI prompt.
    let blocklist: [String; 23] = [
        s!("remove-item -recurse").into(), s!("rm -rf").into(), s!("format-volume").into(),
        s!("clear-disk").into(), s!("net user").into(), s!("disable-netadapter").into(),
        s!("stop-process -name lsass").into(), s!("-encodedcommand").into(),
        s!("invoke-expression").into(), s!("iex ").into(), s!("iex(").into(),
        s!("&{").into(), s!("& {").into(),
        s!("downloadstring").into(), s!("downloadfile").into(), s!("webclient").into(),
        // UAC elevation — audit S10
        s!("-verb runas").into(), s!("-verb 'runas'").into(), s!("-verb \"runas\"").into(),
        s!(".shellexecute(").into(),
        s!("shell.application").into(),
        s!("runas /user:administrator").into(), s!("runas /user:system").into(),
    ];
    let mut was_blocked_but_bypassed = false;

    // Limpiar tokens expirados antes de cualquier validación (TTL 5 min)
    crate::state::purge_expired_bypass_tokens();

    if let Some(ref token) = bypass_token {
        // Mutex poisoning fail-safe: si el lock está poisoned, fallar con error claro
        // en lugar de panic-ear el proceso entero (crash global en producción).
        match crate::state::BYPASS_TOKENS.lock() {
            Ok(mut tokens_map) => {
                if let Some((authorized_script, _expiry)) = tokens_map.get(token) {
                    if authorized_script == &script {
                        was_blocked_but_bypassed = true;
                        let _ = writeln!(log_file, "[{}] [HOST: {}] [AUTHORIZED_BYPASS] Token consumido para: {}", timestamp, user, script);
                        write_app_log("WARNING", "Usuario autorizó comando destructivo vía token");
                        tokens_map.remove(token);
                    }
                }
            }
            Err(e) => {
                write_app_log("ERROR", &format!("BYPASS_TOKENS mutex poisoned: {}", e));
                return Err("Error interno: estado de tokens corrupto. Reinicia Lucy.".to_string());
            }
        }
    }

    // ── Guardrail layer (audit S10): UAC elevation injection ──
    // The substring blocklist below doesn't cover Start-Process -Verb RunAs,
    // .ShellExecute('runas'), or COM-based elevation. A malicious file Lucy
    // reads can plant such a command in its output, and unwary echo-and-exec
    // turns it into local admin RCE. We route these through the same
    // bypass-token flow so the user has to explicitly authorize elevation.
    if !was_blocked_but_bypassed {
        let scan = crate::guardrails::scan(&script, crate::guardrails::Role::Assistant);
        if matches!(scan.decision, crate::guardrails::ScanDecision::HumanInTheLoop
                                  | crate::guardrails::ScanDecision::Block) {
            let new_token = crate::state::generate_secure_token();
            let expiry = std::time::Instant::now()
                + std::time::Duration::from_secs(crate::state::BYPASS_TOKEN_TTL_SECS);
            match crate::state::BYPASS_TOKENS.lock() {
                Ok(mut t) => { t.insert(new_token.clone(), (script.clone(), expiry)); }
                Err(e) => {
                    write_app_log("ERROR", &format!("BYPASS_TOKENS mutex poisoned during insert: {}", e));
                    return Err("Error interno: no se pudo registrar token de seguridad. Reinicia Lucy.".to_string());
                }
            }
            let _ = writeln!(log_file, "[{}] [HOST: {}] [GUARDRAIL_S10_PENDING_AUTH] {} :: {}",
                timestamp, user, scan.reason, script);
            write_app_log("WARNING", &format!("Guardrail intercepted: {}", scan.reason));
            return Err(format!("SECURITY_BLOCK:{}:{}", new_token, scan.reason));
        }
    }

    if !was_blocked_but_bypassed {
        for blocked in blocklist.iter() {
            if script_lower.contains(blocked) {
                // Token criptográficamente seguro (256 bits OsRng) con TTL 5 min
                let new_token = crate::state::generate_secure_token();
                let expiry = std::time::Instant::now()
                    + std::time::Duration::from_secs(crate::state::BYPASS_TOKEN_TTL_SECS);
                match crate::state::BYPASS_TOKENS.lock() {
                    Ok(mut t) => { t.insert(new_token.clone(), (script.clone(), expiry)); }
                    Err(e) => {
                        write_app_log("ERROR", &format!("BYPASS_TOKENS mutex poisoned during insert: {}", e));
                        return Err("Error interno: no se pudo registrar token de seguridad. Reinicia Lucy.".to_string());
                    }
                }

                let _ = writeln!(log_file, "[{}] [HOST: {}] [BLOCKED_PENDING_AUTH] Script: {}", timestamp, user, script);
                write_app_log("WARNING", &format!("Bloqueado comando prohibido: {}", blocked));
                return Err(format!("SECURITY_BLOCK:{}:{}", new_token, blocked));
            }
        }
    }

    // Check permission rules ALWAYS — even when a bypass token authorized the
    // hardcoded blocklist. User-defined "block" rules must override token bypass.
    // Fail-closed: a DB error blocks execution, matching the other exec paths.
    let perm = crate::commands::metrics::check_permission(script.clone(), "command".to_string())
        .await
        .map_err(|e| {
            let _ = writeln!(log_file, "[{}] [HOST: {}] [PERMISSION_CHECK_ERROR] {} - Script: {}", timestamp, user, e, script);
            write_app_log("ERROR", &format!("check_permission falló: {}", e));
            format!("Error verificando permisos (fail-closed): {}", e)
        })?;
    match perm.action.as_str() {
        "block" => {
            let _ = writeln!(log_file, "[{}] [HOST: {}] [BLOCKED_BY_RULE] Rule: {} - Script: {}", timestamp, user, perm.reason, script);
            return Err(format!("Permiso denegado: {}", perm.reason));
        }
        "ask" => {
            let _ = writeln!(log_file, "[{}] [HOST: {}] [PERMISSION_REQUIRED] Rule: {} - Script: {}", timestamp, user, perm.reason, script);
            return Err(format!("Comando requiere aprobación: {}. Crea una regla 'allow' en Permisos para ejecutar.", perm.reason));
        }
        "allow" => {}, // Continue with execution
        _ => return Err(format!("Acción de permiso inválida: {}", perm.action)),
    }

    if was_blocked_but_bypassed {
        let _ = writeln!(log_file, "[{}] [HOST: {}] [EXECUTED_AFTER_BYPASS] Script: {}", timestamp, user, script);
    } else {
        let _ = writeln!(log_file, "[{}] [HOST: {}] [EXECUTED] Script: {}", timestamp, user, script);
    }

    let script_clone = script.clone();
    let timeout_val = timeout_secs.unwrap_or(60);

    // Spawn child process so we can kill it on timeout (prevents zombie processes).
    //
    // CRITICAL FIX (May 2026 — "(sin salida)" bug on Get-Service / Get-MpComputerStatus):
    // PowerShell auto-formats object output to text via the host's display
    // pipeline (Out-Default). When stdout is redirected to a pipe (as we do
    // for capture), that pipeline can silently drop objects whose default
    // formatter expects a terminal width — Get-Service, Get-Process,
    // Get-MpComputerStatus all hit this with -Command on a redirected stdout.
    //
    // Fix: wrap the user's script in a scriptblock and explicitly pipe it
    // through Out-String -Width 4096, which forces materialisation as plain
    // text regardless of whether stdout is a TTY. Also set Console.Output
    // Encoding to UTF-8 so accented chars / box-drawing don't corrupt into
    // garbage when from_utf8_lossy decodes them.
    let wrapped_script = format!(
        "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); \
         $OutputEncoding = [System.Text.UTF8Encoding]::new(); \
         & {{ {} }} | Out-String -Width 4096",
        script_clone
    );

    let child = tokio::task::spawn_blocking(move || {
        let cwd = crate::state::GLOBAL_CWD.read().map(|c| c.clone()).unwrap_or_else(|_| "C:\\".to_string());
        Command::new("powershell")
            .current_dir(cwd)
            .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
            .arg("-OutputFormat").arg("Text")
            .arg("-Command").arg(&wrapped_script)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
    }).await
        .map_err(|e| format!("Error interno spawn: {}", e))?
        .map_err(|e| { write_app_log("ERROR", &format!("Fallo PowerShell spawn: {}", e)); format!("Fallo crítico: {}", e) })?;

    let child_pid = child.id();
    let child = Arc::new(StdMutex::new(Some(child)));
    let child_for_kill = Arc::clone(&child);

    let output_result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_val),
        tokio::task::spawn_blocking(move || {
            let taken = child.lock().map_err(|e| format!("lock: {}", e))?
                .take().ok_or_else(|| "child already consumed".to_string())?;
            taken.wait_with_output().map_err(|e| format!("wait: {}", e))
        })
    ).await;

    let output = match output_result {
        Err(_) => {
            // Timeout: kill the zombie process. Race-safe approach:
            //   1) Try Child::kill() via the Mutex if the Child is still there.
            //   2) ALSO fire `taskkill /F /T /PID <pid>` which kills the whole
            //      process tree (PowerShell can spawn children that the parent
            //      handle alone can't reach).
            // The Child may have already been moved out by `.take()` inside the
            // wait spawn_blocking — in that case the spawn_blocking thread is
            // still running and would otherwise leak. taskkill by PID handles
            // both situations.
            write_app_log("WARNING", &format!("PowerShell timeout (PID {}): comando tardó más de {} segundos — matando proceso (taskkill /T)", child_pid, timeout_val));
            if let Ok(mut guard) = child_for_kill.lock() {
                if let Some(ref mut c) = *guard {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            }
            // Belt-and-suspenders: kill the entire process tree by PID
            let _ = Command::new("taskkill")
                .arg("/F").arg("/T").arg("/PID").arg(child_pid.to_string())
                .creation_flags(CREATE_NO_WINDOW)
                .output();
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
        // stderr is owned + not used after this point — move it directly
        // instead of cloning. Saves an allocation in the error path.
        let err_msg = if stderr.trim().is_empty() {
            "(no output)".to_string()
        } else {
            stderr
        };
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
    // SECURITY: validate host & username BEFORE permission check so malformed
    // inputs are rejected even if no permission rule exists yet.
    validate_host(&host)?;
    validate_username(&username)?;
    if let Some(ref p) = password { validate_password(p)?; }

    // Check permission rules before executing
    let perm = crate::commands::metrics::check_permission(command.clone(), "command".to_string()).await?;
    match perm.action.as_str() {
        "block" => return Err(format!("Permiso denegado: {}", perm.reason)),
        "ask" => return Err(format!("Comando requiere aprobación: {}. Crea una regla 'allow' en Permisos para ejecutar.", perm.reason)),
        "allow" => {}, // Continue with execution
        _ => return Err(format!("Acción de permiso inválida: {}", perm.action)),
    }

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

        // Unified session record: PID + stdin pipe together, single lock acquisition
        STREAM_SESSIONS
            .lock()
            .map_err(|e| format!("session lock: {}", e))?
            .insert(session_id.clone(), StreamSession {
                pid,
                stdin: Some(Arc::new(StdMutex::new(stdin))),
            });

        // Capturar tiempo de inicio para calcular duración del comando
        let start_time = std::time::Instant::now();

        // Hilo lector de stdout — emite chunks; al EOF captura exit code y duración
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
            // Limpiar sesión — single map now, single lock
            if let Ok(mut m) = STREAM_SESSIONS.lock() { m.remove(&sid_out); }
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
        // Inyectar marcador __LUCY_EXIT al final del ScriptBlock para capturar
        // el exit code del último comando nativo ejecutado en el host remoto.
        let exit_suffix = "$__ec=if($LASTEXITCODE){$LASTEXITCODE}else{if($?){0}else{1}}; \
                           Write-Host ('__LUCY_EXIT:'+$__ec) -NoNewline";
        let session_id_clone = session_id.clone();

        let mut child = tokio::task::spawn_blocking(move || {
            spawn_winrm_streaming(&host, &username, &pwd, &command, exit_suffix)
        }).await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("Error al iniciar WinRM streaming: {}", e))?;

        let pid_win = child.id();
        // stdin was already consumed by spawn_winrm_streaming to pipe the script.
        // WinRM Invoke-Command doesn't support interactive stdin after launch.
        let stdout  = child.stdout.take().ok_or("stdout no disponible")?;
        let stderr  = child.stderr.take().ok_or("stderr no disponible")?;

        // WinRM has no usable stdin pipe (Invoke-Command consumed it at launch),
        // but we still register the session so cancellation/cleanup paths work.
        STREAM_SESSIONS
            .lock()
            .map_err(|e| format!("session lock: {}", e))?
            .insert(session_id.clone(), StreamSession {
                pid: pid_win,
                stdin: None,
            });

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
            if let Ok(mut m) = STREAM_SESSIONS.lock() { m.remove(&sid_out); }
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
    // Grab the Arc to stdin and release the map lock immediately so we don't
    // hold the global map lock during a potentially-blocking write. WinRM
    // sessions have stdin=None and are explicitly rejected with a clear error.
    let stdin_arc = {
        let map = STREAM_SESSIONS.lock()
            .map_err(|e| format!("session lock poisoned: {}", e))?;
        let session = map.get(&session_id)
            .ok_or_else(|| format!("Sesión {} no encontrada o ya terminó", session_id))?;
        match &session.stdin {
            Some(s) => s.clone(),
            None => return Err(format!(
                "Sesión {} no soporta input interactivo (WinRM no expone stdin tras lanzar)",
                session_id
            )),
        }
    };
    let mut stdin = stdin_arc.lock()
        .map_err(|e| format!("stdin lock poisoned: {}", e))?;
    writeln!(*stdin, "{}", input)
        .map_err(|e| format!("Error al enviar input: {}", e))?;
    Ok(())
}

/// Cancela la sesión de streaming: cierra stdin y mata el árbol de procesos con taskkill /F /T.
#[tauri::command]
pub fn kill_shell_session(session_id: String) {
    // Atomic remove from the unified map: grab the PID and drop the entry in
    // one lock acquisition. The Drop on StreamSession.stdin (if any) closes
    // the pipe, which alone may not kill the child — taskkill /T finishes the
    // job by reaping the whole process tree.
    let pid = match STREAM_SESSIONS.lock() {
        Ok(mut m) => m.remove(&session_id).map(|s| s.pid),
        Err(_) => None,
    };
    if let Some(pid) = pid {
        let _ = Command::new("taskkill")
            .arg("/F").arg("/T").arg("/PID").arg(pid.to_string())
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
}
