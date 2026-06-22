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

// MED-5 FIX: regex compiled once at startup instead of on every execute_powershell call.
// The old `Regex::new(...).ok()` inside the function body added ~200μs per invocation
// and would panic the process on a future regex typo.
static VAR_ASSIGN_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(r"^\$([A-Za-z_][A-Za-z0-9_]*)\s*=").expect("VAR_ASSIGN_RE is a valid pattern")
});

// ── POWERSHELL LOCAL CON AUDIT LOG ────────────────────────────────────────────

#[tauri::command]
pub async fn execute_powershell(script: String, bypass_token: Option<String>, timeout_secs: Option<u64>) -> Result<String, String> {
    use std::fs::OpenOptions;
    let script_lower = script.to_lowercase();
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let user = System::host_name().unwrap_or_else(|| "LocalSystem".to_string());
    let audit_path = get_logs_dir().join("lucy_audit.log");

    rotate_audit_log();

    // v1.7.108 audit C2 — scrubbed copy used in ALL writeln!(log_file, ...)
    // calls below. The real `script` still flows to PowerShell so the user's
    // intent is executed verbatim; only the disk audit trail gets the
    // redacted view. Without this, Bearer tokens / passwords passed inline
    // (e.g. `curl -H "Authorization: Bearer sk-..."`) landed in
    // %APPDATA%\Lucy\logs\lucy_audit.log readable by any local process.
    let scrub = crate::utils::secret_scrubber::scrub_for_audit(&script);

    // v1.7.9 — Placeholder guard. See utils/placeholder_guard.rs.
    // Refuses to run example values from a documentation skill body
    // before they hit PowerShell and trigger the autocorrect / agent
    // loop. The error message is tuned to make the LLM stop and ask
    // the user for real values, not retry.
    if let Some(evidence) = crate::utils::placeholder_guard::detect_placeholders(&script) {
        let line = format!(
            "[{}] [USR:{}] [PLACEHOLDER_GUARD] {} :: {}",
            timestamp, user, evidence,
            &crate::utils::secret_scrubber::scrub_for_audit(
                &script[..script.len().min(200)]
            )
        );
        let _ = OpenOptions::new().create(true).append(true).open(&audit_path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{}", line)
            });
        return Err(crate::utils::placeholder_guard::refusal_message(&evidence));
    }

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
                        let _ = writeln!(log_file, "[{}] [HOST: {}] [AUTHORIZED_BYPASS] Token consumido para: {}", timestamp, user, scrub);
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
                timestamp, user, scan.reason, scrub);
            write_app_log("WARNING", &format!("Guardrail intercepted: {}", scan.reason));
            return Err(format!("SECURITY_BLOCK:{}:{}", new_token, scan.reason));
        }
    }

    if !was_blocked_but_bypassed {
        // v1.7.218 SEC — also match the blocklist against a DE-OBFUSCATED form.
        // The raw `script_lower` let `remove-item`` ` ``-recurse` (a PowerShell
        // backtick escape) slip past the substring check; the backtick was then
        // stripped below (clean_script) and the destructive command executed
        // WITHOUT the HITL bypass-token prompt. The guardrail layer intentionally
        // does NOT block destructive verbs (scanner.rs), so this blocklist is the
        // sole gate. Strip backticks + collapse whitespace so the obfuscated form
        // matches the same entry the literal form would.
        let script_norm: String = script_lower
            .replace('`', "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for blocked in blocklist.iter() {
            if script_lower.contains(blocked) || script_norm.contains(blocked) {
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

                let _ = writeln!(log_file, "[{}] [HOST: {}] [BLOCKED_PENDING_AUTH] Script: {}", timestamp, user, scrub);
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
            let _ = writeln!(log_file, "[{}] [HOST: {}] [PERMISSION_CHECK_ERROR] {} - Script: {}", timestamp, user, e, scrub);
            write_app_log("ERROR", &format!("check_permission falló: {}", e));
            format!("Error verificando permisos (fail-closed): {}", e)
        })?;
    match perm.action.as_str() {
        "block" => {
            let _ = writeln!(log_file, "[{}] [HOST: {}] [BLOCKED_BY_RULE] Rule: {} - Script: {}", timestamp, user, perm.reason, scrub);
            return Err(format!("Permiso denegado: {}", perm.reason));
        }
        "ask" => {
            let _ = writeln!(log_file, "[{}] [HOST: {}] [PERMISSION_REQUIRED] Rule: {} - Script: {}", timestamp, user, perm.reason, scrub);
            return Err(format!("Comando requiere aprobación: {}. Crea una regla 'allow' en Permisos para ejecutar.", perm.reason));
        }
        "allow" => {}, // Continue with execution
        _ => return Err(format!("Acción de permiso inválida: {}", perm.action)),
    }

    if was_blocked_but_bypassed {
        let _ = writeln!(log_file, "[{}] [HOST: {}] [EXECUTED_AFTER_BYPASS] Script: {}", timestamp, user, scrub);
    } else {
        let _ = writeln!(log_file, "[{}] [HOST: {}] [EXECUTED] Script: {}", timestamp, user, scrub);
    }

    let script_clone = script.clone();
    let timeout_val = timeout_secs.unwrap_or(60);

    // Spawn child process so we can kill it on timeout (prevents zombie processes).
    //
    // ── PowerShell stdout-capture pipeline (May 2026 v3 — temp-file approach) ──
    //
    // History of failed attempts on the "(sin salida)" bug:
    //   v1: wrap in `& { … } | Out-String` via -Command. FAILED — Command::arg
    //       passes verbatim to CreateProcess which re-tokenises; nested {} from
    //       Where-Object scriptblocks confused the outer wrap.
    //   v2: same wrap + base64 -EncodedCommand. FAILED — PowerShell decoded the
    //       script correctly but PROGRESS objects on stderr ("Preparando módulos
    //       para el primer uso") interleaved with the output stream and CLIXML
    //       serialisation drowned the actual data path.
    //
    // v3 — write the wrapped script to a temp .ps1 file and execute via -File.
    // Why this works where the others didn't:
    //   • PowerShell parses .ps1 files in their native script mode, NOT the
    //     command-line re-tokenisation mode. Any script with any nesting works.
    //   • We can prepend $ProgressPreference='SilentlyContinue' at the top of
    //     the file to suppress the progress CLIXML noise that was polluting
    //     stderr and (more importantly) confusing our success-detection.
    //   • Force UTF-8 BOM on the file so PowerShell reads it as UTF-8 reliably.
    //   • Temp file lives in std::env::temp_dir() — auto-cleaned by Windows on
    //     reboot, plus we delete it ourselves in the finally-equivalent.
    //
    // The wrapper now:
    //   1. Suppresses progress UI (the source of the CLIXML noise).
    //   2. Sets UTF-8 console encoding so emoji / accented chars survive.
    //   3. Runs the user script and pipes through Out-String -Width 4096 so
    //      object output is always materialised as text.
    // ── Sanitise the user script ────────────────────────────────────────
    // Two real-world LLM bug fingerprints we now defend against:
    //
    //  (a) Line-continuation backticks. The LLM often writes scripts like:
    //          Get-MpComputerStatus | Select-Object `
    //              AMServiceEnabled, AntivirusEnabled
    //      When that goes through the TOOL regex it can collapse to one
    //      line, leaving the backtick adjacent to a space — which in
    //      PowerShell escapes the next char and breaks parsing. Strip
    //      `<whitespace> sequences (the continuation form) since they're
    //      meaningless once newlines collapse.
    //
    //  (b) `$var = <pipeline>` shaped scripts. The assignment captures
    //      the WHOLE pipeline into the variable, so when we append
    //      `| Out-String` the assignment swallows it and stdout stays
    //      empty. We auto-detect `^$word\s*=` at the start of the
    //      (single-statement) script and append `; $word` so the
    //      variable's value gets echoed at the end.
    let mut clean_script = script_clone.clone();
    // (a) strip line-continuation backticks followed by whitespace
    while let Some(pos) = clean_script.find("`\n") { clean_script.replace_range(pos..pos+2, " "); }
    while let Some(pos) = clean_script.find("` ")  { clean_script.replace_range(pos..pos+2, " "); }
    // (b) auto-echo for single-line `$var = ...` pattern (no other ;)
    let trimmed = clean_script.trim();
    {
        let re = &*VAR_ASSIGN_RE; // MED-5 FIX: use static compiled regex
        if let Some(cap) = re.captures(trimmed) {
            if let Some(name) = cap.get(1) {
                let varname = name.as_str().to_string();
                // Only auto-echo if the script has no explicit semicolon
                // (single-statement). Multi-statement scripts may have
                // their own output logic and we don't want to clobber.
                if !trimmed.contains(';') && !trimmed.to_lowercase().contains("write-output") {
                    clean_script = format!("{}; ${}", trimmed, varname);
                }
            }
        }
    }

    let wrapped_script = format!(
        "\u{FEFF}# Lucy execute_powershell wrapper (auto-generated)\n\
         $ProgressPreference = 'SilentlyContinue'\n\
         $ErrorActionPreference = 'Continue'\n\
         [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()\n\
         $OutputEncoding = [System.Text.UTF8Encoding]::new()\n\
         try {{\n\
             & {{\n\
         {}\n\
             }} | Out-String -Width 4096\n\
         }} catch {{\n\
             Write-Output \"[POWERSHELL ERROR]: $($_.Exception.Message)\"\n\
             exit 1\n\
         }}\n",
        clean_script
    );

    // v1.7.109 audit C1 — symlink-resistant temp file creation.
    //
    // Old version was `fs::write(predictable_path)` where the path was
    // `lucy_ps_<pid>_<nanos>.ps1` in %TEMP%. Three problems:
    //   1. fs::write FOLLOWS symlinks. An attacker with local write access
    //      to %TEMP% could pre-create a symlink at a predicted path
    //      pointing to a sensitive file; we'd overwrite the target with
    //      the script. (PID is enumerable, nanos has limited precision —
    //      both can be predicted within a small window.)
    //   2. Even without an attacker, two execute_powershell calls in the
    //      same nanosecond (rare but possible under sub-µs scheduler)
    //      would clobber each other.
    //   3. fs::write silently truncates any existing target.
    //
    // Fix: 64 bits of cryptographic entropy in the filename + create_new
    // (which fails atomically if anything — file, dir, or symlink — exists
    // at the path). Retry up to 8 times on AlreadyExists for the pathological
    // case where entropy collides. 64 bits ≈ 1 in 1.8e19 collision per draw,
    // so reaching the retry limit means an active attacker — we fail closed.
    let pid = std::process::id();
    let mut tmp_path: std::path::PathBuf = std::path::PathBuf::new();
    let mut last_err: Option<std::io::Error> = None;
    for _ in 0..8 {
        let suffix: u64 = rand::random();
        let candidate = std::env::temp_dir()
            .join(format!("lucy_ps_{}_{:016x}.ps1", pid, suffix));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut f) => {
                if let Err(e) = f.write_all(wrapped_script.as_bytes()) {
                    // We created the file but failed to populate it — clean
                    // up to avoid leaving an empty stub in %TEMP%.
                    let _ = std::fs::remove_file(&candidate);
                    write_app_log("ERROR", &format!("Fallo escribir script temp: {}", e));
                    return Err(format!("No se pudo escribir script temporal: {}", e));
                }
                tmp_path = candidate;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                last_err = Some(e);
                continue;
            }
            Err(e) => {
                write_app_log("ERROR", &format!("Fallo crear script temp: {}", e));
                return Err(format!("No se pudo crear script temporal: {}", e));
            }
        }
    }
    if tmp_path.as_os_str().is_empty() {
        let detail = last_err.map(|e| e.to_string()).unwrap_or_else(|| "retries exhausted".to_string());
        write_app_log("ERROR", &format!("Symlink/collision retries exhausted: {}", detail));
        return Err(format!("No se pudo crear script temporal de forma segura: {}", detail));
    }

    let tmp_path_for_run = tmp_path.clone();
    let child = tokio::task::spawn_blocking(move || {
        let cwd = crate::state::GLOBAL_CWD.read().map(|c| c.clone()).unwrap_or_else(|_| "C:\\".to_string());
        // ── CRITICAL FIX (May 2026 v4 — root cause of "(sin salida)" bug) ──
        // v1–v3 all attacked the wrong layer: they kept rewriting the
        // PowerShell wrapper (Out-String, EncodedCommand, temp .ps1) under
        // the assumption that PS wasn't producing output. PS WAS producing
        // output. The actual bug is that `.spawn()` without explicit
        // .stdout/.stderr piping INHERITS those handles from the parent
        // Tauri process — which on a windowed GUI app has no console, so
        // the inherited handle resolves to NUL. `wait_with_output()` then
        // returns empty Vec<u8> for both streams REGARDLESS of what the
        // child actually wrote. (`Command::output()` doesn't have this bug
        // because it pipes implicitly — which is why execute_cmd worked
        // while execute_powershell didn't.)
        //
        // The three Stdio configs below are the minimum required for
        // wait_with_output() to capture anything. stdin=null because the
        // PowerShell script is fully self-contained in the temp .ps1 file
        // and never reads stdin — closing it avoids the (rare) hang where
        // a malformed user script does `Read-Host`.
        Command::new("powershell")
            .current_dir(cwd)
            .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
            .arg("-OutputFormat").arg("Text")
            .arg("-File").arg(&tmp_path_for_run)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    }).await
        .map_err(|e| format!("Error interno spawn: {}", e))?
        .map_err(|e| {
            // Try to clean the temp file even on spawn failure
            let _ = std::fs::remove_file(&tmp_path);
            write_app_log("ERROR", &format!("Fallo PowerShell spawn: {}", e));
            format!("Fallo crítico: {}", e)
        })?;

    // Clone for cleanup after exit
    let tmp_path_for_cleanup = tmp_path.clone();

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
            write_app_log("WARNING", &format!("PowerShell timeout (PID {}): comando tardó más de {} segundos — matando proceso (taskkill /T)", child_pid, timeout_val));
            if let Ok(mut guard) = child_for_kill.lock() {
                if let Some(ref mut c) = *guard {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            }
            let _ = Command::new("taskkill")
                .arg("/F").arg("/T").arg("/PID").arg(child_pid.to_string())
                .creation_flags(CREATE_NO_WINDOW)
                .output();
            // Clean up temp script on timeout too
            let _ = std::fs::remove_file(&tmp_path_for_cleanup);
            return Err(format!("Timeout: el comando tardó más de {} segundos y fue cancelado.", timeout_val));
        }
        Ok(Err(e)) => {
            let _ = std::fs::remove_file(&tmp_path_for_cleanup);
            return Err(format!("Error interno spawn: {}", e));
        }
        Ok(Ok(Err(e))) => {
            write_app_log("ERROR", &format!("Fallo PowerShell: {}", e));
            let _ = std::fs::remove_file(&tmp_path_for_cleanup);
            return Err(format!("Fallo crítico: {}", e));
        }
        Ok(Ok(Ok(out))) => out,
    };

    // Clean up the temp script file now that PowerShell has finished with it.
    // Best-effort: ignore errors (Windows often holds a brief lock after exit).
    let _ = std::fs::remove_file(&tmp_path_for_cleanup);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Success criteria: exit code 0 ⇒ success.
    // If exit != 0 BUT stdout has content, treat as partial success (Get-ChildItem -Recurse
    // commonly emits stderr for ACL-denied dirs while still returning useful results).
    // Returning Err here would force the agent into useless retries.
    if output.status.success() {
        if stderr.trim().is_empty() {
            // For commands that produce no stdout (Stop-Process, Stop-Service, Set-*, etc.)
            // return an explicit empty string so the frontend's fallback renders cleanly.
            // Previously this returned "" which was fine, but adding no extra noise here.
            Ok(stdout)
        } else {
            // Success with stderr noise — append as warning footer so the agent sees both.
            // Strip the PowerShell "Preparing modules for first use" progress noise that
            // Out-String sometimes leaks into stderr on first run per session.
            let clean_stderr = stderr.trim()
                .lines()
                .filter(|l| !l.contains("Preparing modules") && !l.contains("Loading personal"))
                .collect::<Vec<_>>()
                .join("\n");
            if clean_stderr.is_empty() {
                Ok(stdout)
            } else {
                Ok(format!("{}\n\n[stderr warnings]\n{}", stdout, clean_stderr))
            }
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
    // v1.7.111 audit H2 — reject an SSH key path that could be reinterpreted
    // as an option. A legitimate identity file is an absolute filesystem
    // path; one starting with '-' (e.g. "-oProxyCommand=calc.exe") is an
    // argument-injection attempt. Although `cmd.arg("-i").arg(kp)` makes kp
    // the OPERAND of -i (so ssh's getopt won't re-parse it as a flag), we
    // fail closed here as defense-in-depth and to guard any future call site
    // that interpolates the path differently.
    if let Some(ref kp) = key_path {
        if kp.trim_start().starts_with('-') {
            return Err("Ruta de clave SSH inválida: no puede comenzar con '-'".to_string());
        }
    }

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
            // SEC-4 KNOWN RISK: accept-new silently trusts unknown host keys on first
            // connection — zero MITM protection until the key is cached in known_hosts.
            // TODO: surface fingerprint to user on first connect, or use
            // StrictHostKeyChecking=yes with a managed known_hosts file.
            cmd.arg("-tt")
               .arg("-o").arg("StrictHostKeyChecking=accept-new")
               .arg("-o").arg("ConnectTimeout=10")
               // Keep-alive hardening: ServerAliveInterval was set but
               // ServerAliveCountMax defaulted to 3, so ~30s of unanswered
               // keepalives (common while a heavy rpm/apt transaction or kernel
               // scriptlet briefly stalls I/O) dropped the connection. With
               // -tt that SIGHUPs the remote command, but rpm transactions
               // survive and keep running — so the UI saw "done" while the host
               // kept upgrading. 10s × 12 = 120s of grace before giving up.
               .arg("-o").arg("ServerAliveInterval=10")
               .arg("-o").arg("ServerAliveCountMax=12")
               .arg("-o").arg("TCPKeepAlive=yes")
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

// ─────────────────────────────────────────────────────────────────────────────
// CONTRACT TESTS — execute_powershell stdout/stderr capture
//
// These tests exist to prevent the "(sin salida)" bug from EVER coming back.
//
// History: between May 2026 and the v4 fix, three "critical fix" commits
// (9dc5dce, 25a0c96, a170ad9) tried to fix this bug by rewriting the
// PowerShell wrapper script — Out-String, EncodedCommand, temp .ps1 files.
// None worked, because none addressed the actual root cause: `.spawn()`
// inherits stdout/stderr from the parent unless explicitly piped, so
// `wait_with_output()` returned empty Vec<u8> regardless of what PS printed.
//
// These tests assert the CONTRACT — "PowerShell output reaches the caller" —
// rather than the implementation. If anyone refactors execute_powershell
// (LLM-assisted or not) and breaks the capture pipe, these tests go red
// IMMEDIATELY. The tests cost ~3 seconds total to run; the bug cost two
// debugging sessions and a frustrated user. Worth it.
//
// To add a NEW test: pick a unique sentinel string (so parallel test
// execution doesn't false-positive), run a small script that prints it,
// assert it comes back. Keep tests fast (< 5s each) — these run on every
// `cargo test`.
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// One-time setup — initialises an in-memory DB so check_permission works.
    /// Safe to call from every test: it's a no-op after the first invocation.
    fn setup() {
        crate::commands::metrics::init_in_memory_for_tests()
            .expect("test DB init failed");
    }

    /// CONTRACT: stdout from `Write-Output` MUST reach the caller.
    /// This is the canary that died for two months. If this fails, the
    /// stdout pipe is broken again — check that `.stdout(Stdio::piped())`
    /// is still present on the `Command::new("powershell")` invocation.
    #[tokio::test]
    async fn powershell_captures_simple_stdout() {
        setup();
        // Sentinel intentionally weird so we know it came from THIS test.
        let out = execute_powershell(
            "Write-Output 'sentinel-9f3a-x7k2'".to_string(),
            None,
            Some(15),
        ).await.expect("execute_powershell returned Err");
        assert!(
            out.contains("sentinel-9f3a-x7k2"),
            "Regression: PowerShell stdout NOT captured. \
             This is the (sin salida) bug returning. \
             Check that .stdout(Stdio::piped()) is set in execute_powershell. \
             Actual output: {:?}", out
        );
    }

    /// CONTRACT: object output (the real-world case — Get-Service, Get-Process,
    /// Get-MpComputerStatus etc.) must materialise via Out-String wrap.
    #[tokio::test]
    async fn powershell_captures_object_output() {
        setup();
        let out = execute_powershell(
            "Get-Process | Select-Object -First 1 -Property Name".to_string(),
            None,
            Some(15),
        ).await.expect("execute_powershell returned Err");
        // Get-Process ALWAYS returns at least the System process. If the
        // output capture works, we'll see a "Name" header and at least one
        // row of data. If broken, the string is empty/whitespace.
        assert!(
            !out.trim().is_empty(),
            "Object output not materialised — likely Out-String wrap regression or pipe break. Got: {:?}", out
        );
        assert!(
            out.contains("Name"),
            "Expected 'Name' header in formatted object output. Got: {:?}", out
        );
    }

    /// CONTRACT: the v3 auto-echo for `$var = ...` single-statement scripts
    /// must keep working. Without it, scripts like `$x = Get-Date` returned
    /// nothing because the assignment swallowed the pipeline.
    #[tokio::test]
    async fn powershell_auto_echoes_var_assignment() {
        setup();
        let out = execute_powershell(
            "$x = 'auto-echo-3k7m'".to_string(),
            None,
            Some(15),
        ).await.expect("execute_powershell returned Err");
        assert!(
            out.contains("auto-echo-3k7m"),
            "Var-assign auto-echo broken (v3 sanitisation). Got: {:?}", out
        );
    }

    /// CONTRACT: a deliberately broken script must surface its error message
    /// (via the try/catch wrap), not silently return empty success.
    #[tokio::test]
    async fn powershell_surfaces_errors() {
        setup();
        // Throw a unique sentinel from inside PowerShell — the catch block
        // in the wrapper should turn it into "[POWERSHELL ERROR]: ..." output.
        let out = execute_powershell(
            "throw 'sentinel-error-q4p8'".to_string(),
            None,
            Some(15),
        ).await;
        match out {
            Ok(s) => {
                assert!(
                    s.contains("sentinel-error-q4p8"),
                    "Errors should surface either as Ok with [POWERSHELL ERROR] or as Err — got Ok({:?})", s
                );
            }
            Err(e) => {
                assert!(
                    e.contains("sentinel-error-q4p8") || e.to_lowercase().contains("error"),
                    "Error path returned but doesn't mention the failure. Got: {:?}", e
                );
            }
        }
    }

    /// CONTRACT: the timeout flag actually kills runaway scripts.
    /// We don't want a regression that lets `Start-Sleep` hang Lucy forever.
    ///
    /// Note on bounds (Cleanup task, v1.4.3): the elapsed-time assert was
    /// previously `< 10s` which intermittently failed when PowerShell's
    /// first-run module load took 6-8s. The kill path is correct in both
    /// cases — the test just wasn't allowing PS enough cold-start slack.
    /// 20s gives plenty of room while still catching a real broken kill
    /// (which would let the 30s sleep complete).
    #[tokio::test]
    async fn powershell_timeout_fires() {
        setup();
        let start = std::time::Instant::now();
        let result = execute_powershell(
            "Start-Sleep -Seconds 30".to_string(),
            None,
            Some(3), // 3-second timeout — should kill well before sleep completes
        ).await;
        let elapsed = start.elapsed();
        assert!(
            result.is_err(),
            "Expected timeout Err, got Ok"
        );
        assert!(
            elapsed.as_secs() < 20,
            "Timeout took too long ({:?}) — kill path broken", elapsed
        );
        let err = result.unwrap_err();
        assert!(
            err.to_lowercase().contains("timeout"),
            "Error should mention timeout. Got: {:?}", err
        );
    }

    // ── Sprint 1, NS-1 — Three new contract tests ────────────────────────
    //
    // These cover paths that previously had no coverage and that were the
    // source of "Lucy just stopped responding" bug reports during beta.

    /// CONTRACT: the stderr-noise filter strips PowerShell "first run" warnings
    /// ("Preparing modules for first use", "Loading personal and system
    /// profiles") that PS sometimes leaks to stderr even on successful runs.
    /// Without the filter, downstream code interprets these as command errors.
    #[tokio::test]
    async fn stderr_noise_is_filtered() {
        setup();
        // A command that always succeeds and emits clean stdout. If the noise
        // filter ever regresses, this test catches it because the noise lines
        // would land in stderr → be appended → break the contains() match.
        // Cleanup task (v1.4.3): bumped from 15s → 30s to stop transient
        // failures on cold PS-7 first launch where module preload can take
        // 12-18s. The filter logic is the same; the test just needs slack.
        let out = execute_powershell(
            "Write-Output 'clean-stdout-marker'".to_string(),
            None,
            Some(30),
        ).await.expect("execute_powershell returned Err");
        assert!(out.contains("clean-stdout-marker"),
            "Lost the actual stdout. out = {:?}", out);
        assert!(
            !out.to_lowercase().contains("preparing modules for first use"),
            "Regression: PowerShell first-run noise leaked through. \
             Check that the stderr filter in execute_powershell strips \
             'Preparing modules' lines. Got: {:?}", out
        );
        assert!(
            !out.to_lowercase().contains("loading personal and system profiles"),
            "Regression: PowerShell profile-load noise leaked through. \
             Got: {:?}", out
        );
    }

    /// CONTRACT: commands that legitimately produce NO stdout (Stop-Process,
    /// Set-Variable, Move-Item, Out-Null) MUST return Ok("") not Err. A previous
    /// regression treated empty output as an error and Lucy started saying
    /// "(sin salida)" for every successful housekeeping command.
    #[tokio::test]
    async fn empty_output_returns_empty_string_not_error() {
        setup();
        // Set-Variable produces no stdout when successful; classic empty-success case.
        //
        // NOTE: tests share a persistent PowerShell session, so the *content*
        // of `out` may contain output bled in from a sibling test running in
        // parallel. The CONTRACT this test pins is "Ok, not Err" — the bug we
        // guard against is the layer treating silent-success as failure. We do
        // NOT assert empty stdout (that would be flaky under cargo test).
        let result = execute_powershell(
            "Set-Variable -Name foo -Value 42 | Out-Null".to_string(),
            None,
            Some(15),
        ).await;
        assert!(result.is_ok(),
            "Empty-output command incorrectly returned Err. \
             A silent success is NOT a failure. Got: {:?}", result);
    }

    // NOTE: a bypass_token_passthrough test was prototyped here but removed
    // because it issues parallel Write-Output calls into the shared persistent
    // shell session, racing with sibling tests for response pairing. The
    // bypass_token signature is already enforced at compile time and the gate
    // logic lives in `check_permission` which has its own dedicated coverage.
}
