// ── STATE — Statics compartidos entre todos los módulos ───────────────────────
// Centralizar aquí evita importaciones circulares y facilita testing.

use reqwest::Client;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, RwLock};
use once_cell::sync::Lazy;

/// Flag de creación sin ventana de consola (Windows-only).
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Info per active streaming session — PID and (optional) stdin pipe travel
/// together so we can clean up atomically. Previously kept in two parallel
/// HashMaps (STREAM_PIDS + STREAM_SESSIONS) which required double-locking on
/// every cleanup and risked the two maps drifting out of sync.
///
/// `stdin` is None for transports that don't support interactive input
/// (WinRM Invoke-Command consumes stdin at launch). For SSH it holds the
/// child's stdin so `send_shell_input` can write sudo passwords, y/n, etc.
pub struct StreamSession {
    pub pid: u32,
    pub stdin: Option<Arc<StdMutex<std::process::ChildStdin>>>,
}

/// Sesiones de streaming activas: session_id → SessionInfo (PID + optional stdin).
/// Single source of truth for all in-flight remote shell streams.
pub static STREAM_SESSIONS: Lazy<StdMutex<HashMap<String, StreamSession>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));

/// Tokens de seguridad efímeros para bypass en bloqueos de PowerShell.
/// Map: token_aleatorio -> (script_autorizado, instant_expiración)
///
/// SEGURIDAD:
/// - El token se genera con OsRng (rand crate) — 256 bits, criptográficamente seguro
/// - Cada token tiene TTL de 5 minutos: si el usuario no lo consume en ese tiempo, expira
/// - shell.rs llama `purge_expired_bypass_tokens()` antes de validar para limpiar tokens viejos
pub static BYPASS_TOKENS: Lazy<StdMutex<HashMap<String, (String, std::time::Instant)>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));

/// TTL para tokens de bypass — 5 minutos es suficiente para que el usuario lea el modal y decida.
pub const BYPASS_TOKEN_TTL_SECS: u64 = 300;

/// Genera un token criptográficamente seguro (256 bits / 64 chars hex).
/// Usado solo para bypass tokens — NUNCA reutilizar para IDs visibles al usuario.
pub fn generate_secure_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Limpia tokens de bypass expirados. Llamar antes de cualquier validación
/// para evitar que tokens viejos se acumulen en memoria.
pub fn purge_expired_bypass_tokens() {
    let now = std::time::Instant::now();
    if let Ok(mut tokens) = BYPASS_TOKENS.lock() {
        tokens.retain(|_, (_, exp)| *exp > now);
    }
}

/// Cleanup stream sessions whose underlying process has died without going
/// through the normal `kill_shell_session` path (e.g. ssh client crashed,
/// remote server cut the connection abruptly, OOM killer, etc).
///
/// Without this, STREAM_SESSIONS / STREAM_PIDS leak entries every time a
/// process exits abnormally — the maps grow forever.
///
/// Returns how many sessions were cleaned up (mostly for diagnostics/logging).
pub fn purge_dead_stream_sessions() -> usize {
    use sysinfo::{System, Pid};

    let mut sys = System::new();
    sys.refresh_processes();

    // Build the set of PIDs we currently track, then drop the lock before
    // doing the (slower) process-table scan.
    let tracked: Vec<(String, u32)> = match STREAM_SESSIONS.lock() {
        Ok(map) => map.iter().map(|(k, v)| (k.clone(), v.pid)).collect(),
        Err(_) => return 0,
    };

    let mut dead_keys: Vec<String> = Vec::new();
    for (session, pid) in tracked {
        if sys.process(Pid::from(pid as usize)).is_none() {
            dead_keys.push(session);
        }
    }

    let count = dead_keys.len();
    if count > 0 {
        if let Ok(mut m) = STREAM_SESSIONS.lock() {
            for k in &dead_keys { m.remove(k); }
        }
    }
    count
}

/// Global working directory. Uses `std::sync::RwLock` (not tokio) because 30+
/// call sites read it synchronously inside `spawn_blocking` closures where
/// tokio's async lock would require `.await`.
///
/// SAFETY NOTE (MED-6): write acquisitions block the tokio executor thread
/// until all readers release. Keep ALL lock holds under 1μs — clone immediately
/// and drop the guard. Never do I/O while holding this lock. If write starvation
/// becomes an issue, migrate to `tokio::sync::RwLock` with an async refactor.
pub static GLOBAL_CWD: Lazy<RwLock<String>> = Lazy::new(|| {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "C:\\".to_string());
    RwLock::new(cwd)
});

/// Per-tab CWD overrides. Lets each chat tab maintain its own working directory
/// without bleeding into others. When a command receives `tab_id`, it should
/// prefer this map; if no entry, falls back to `GLOBAL_CWD`.
///
/// Stability fix: GLOBAL_CWD shared across all tabs caused confusing UX where
/// `cd` in tab A silently changed the CWD seen by tab B's commands.
pub static TAB_CWDS: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Get the effective CWD for a tab. If `tab_id` is None, returns GLOBAL_CWD.
/// If `tab_id` is Some but has no per-tab override, also returns GLOBAL_CWD.
pub fn get_cwd_for(tab_id: Option<&str>) -> String {
    if let Some(tid) = tab_id {
        if let Ok(map) = TAB_CWDS.read() {
            if let Some(p) = map.get(tid) { return p.clone(); }
        }
    }
    GLOBAL_CWD.read().map(|c| c.clone()).unwrap_or_else(|_| "C:\\".to_string())
}

/// Set per-tab CWD. If `tab_id` is None, updates GLOBAL_CWD.
pub fn set_cwd_for(tab_id: Option<&str>, path: String) -> Result<(), String> {
    match tab_id {
        Some(tid) => {
            let mut map = TAB_CWDS.write().map_err(|e| format!("TAB_CWDS poisoned: {}", e))?;
            map.insert(tid.to_string(), path);
            Ok(())
        }
        None => {
            let mut g = GLOBAL_CWD.write().map_err(|e| format!("GLOBAL_CWD poisoned: {}", e))?;
            *g = path;
            Ok(())
        }
    }
}

/// Cleanup: remove a tab's CWD entry when it's closed (frontend should call this
/// to avoid the map growing unbounded).
pub fn drop_tab_cwd(tab_id: &str) {
    if let Ok(mut map) = TAB_CWDS.write() { map.remove(tab_id); }
}

/// SSRF-safe redirect policy (audit S5, May 2026).
///
/// The previous policy was reqwest's default — follow up to 10 redirects to
/// ANY host. That's an SSRF amplifier: a benign-looking public URL can
/// `Location:` itself to `169.254.169.254/latest/meta-data/iam/...` (cloud
/// metadata service) or `127.0.0.1:11434` (Ollama on the user's box), and
/// the response body comes back to whoever requested the original URL — in
/// Lucy's case, the LLM, which then exfiltrates it via the next message.
///
/// This policy walks the redirect chain hop-by-hop and rejects any hop
/// pointing at an internal/loopback/cloud-metadata target. Uses the
/// guardrails::scan_url helper so the deny-list is centralized.
fn ssrf_safe_redirect_policy() -> reqwest::redirect::Policy {
    use reqwest::redirect::Policy;
    Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            return attempt.error("too many redirects (>10)");
        }
        let url = attempt.url().as_str();
        let scan = crate::guardrails::scan_url(url);
        match scan.decision {
            crate::guardrails::ScanDecision::Allow => attempt.follow(),
            _ => attempt.error(format!(
                "Redirect to internal/sensitive target blocked: {}",
                scan.reason
            )),
        }
    })
}

/// Cliente HTTP "AI" — timeout largo (300s) para streaming LLM y agent loops.
/// Un único cliente comparte pool de conexiones y amortiza TLS handshake.
///
/// SECURITY: aceptar conexiones lentas hasta 5 min está bien para AI (los modelos
/// reasoning toman tiempo) pero NO para resto de calls — usa HTTP_CLIENT_FAST allí.
/// SSRF: redirect policy validates each hop (audit S5).
pub static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(15))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .user_agent(concat!("Lucy/", env!("CARGO_PKG_VERSION")))
        .redirect(ssrf_safe_redirect_policy())
        .build()
        .expect("Error creando cliente HTTP global")
});

/// Cliente HTTP "rápido" — timeout corto (15s) para fetch de docs, providers list,
/// catálogos NIM, healthchecks, etc. Mitiga ataques tipo slowloris donde una URL
/// maliciosa mantiene conexiones abiertas indefinidamente.
/// SSRF: redirect policy validates each hop (audit S5).
pub static HTTP_CLIENT_FAST: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(8))
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("Lucy/", env!("CARGO_PKG_VERSION")))
        .redirect(ssrf_safe_redirect_policy())
        .build()
        .expect("Error creando cliente HTTP rápido")
});

/// Whitelist explícita de modelos permitidos.
/// Previene llamadas a endpoints arbitrarios si el frontend envía un modelo inválido.
/// Los modelos NVIDIA NIM tienen formato "owner/model-name" y se validan
/// por la regla `model.contains('/')` en ai.rs (no se listan aquí).
pub const ALLOWED_MODELS: &[&str] = &[
    // ── Gemini 3.1 (May 2026) ──
    "gemini-3.1-pro-preview",
    "gemini-3-flash-preview",
    "gemini-3.1-flash-lite",            // production
    "gemini-3.1-flash-lite-preview",    // preview
    // ── Gemini 2.5 (legacy — kept for compat with old chats) ──
    "gemini-2.5-flash",
    "gemini-2.5-pro",
    "gemini-2.5-flash-lite-preview",
    // ── Anthropic Claude (May 2026 lineup) ──
    "claude-opus-4-7",                  // flagship — 1M ctx
    "claude-sonnet-4-6",                // balanced — 1M ctx
    "claude-haiku-4-5",                 // fast tier
    // Legacy Claude
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-3-7-sonnet-20250219",
    "claude-3-5-sonnet-latest",
    "claude-3-5-haiku-latest",
    // ── OpenAI GPT-5 family (April-May 2026) ──
    "gpt-5.5",
    "gpt-5.5-instant",
    "gpt-5.4-mini",
    "gpt-5.4-nano",
    "gpt-5.3-codex",
    "gpt-5",
    "gpt-5-pro",
    "gpt-5-mini",
    "gpt-5-nano",
    // Legacy GPT-4
    "gpt-4o",
    "gpt-4o-mini",
    "gpt-4-turbo",
    "o1",
    "o3-mini",
    "o4-mini",
];
