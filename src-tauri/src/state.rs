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
        // 1. String/regex deny-list (literal internal IPs, metadata hostnames).
        let scan = crate::guardrails::scan_url(url);
        if !matches!(scan.decision, crate::guardrails::ScanDecision::Allow) {
            return attempt.error(format!(
                "Redirect to internal/sensitive target blocked: {}",
                scan.reason
            ));
        }
        // 2. SECURITY v1.7.232 (Phase-2 SSRF-REDIRECT-DNS-01): also RESOLVE the
        //    redirect host and reject if it maps to an internal/loopback/metadata
        //    IP. The string check above is trivially bypassed on the redirect path
        //    by DNS-rebinding (a public-looking hostname whose A record is
        //    169.254.169.254 / 127.0.0.1) or an obfuscated IP literal
        //    (http://2130706433/) — `host_resolves_to_internal` normalizes both via
        //    the OS resolver, the same guard the INITIAL fetch already runs (ai.rs)
        //    but that the redirect chain was missing. Fail-closed (Err on
        //    unresolvable). NOTE: a BLOCKING resolve on the (rare) redirect hop
        //    inside this sync callback; a pinning connector that dials the validated
        //    IP is the ideal future fix (also closes the resolve→connect TOCTOU).
        if crate::guardrails::host_resolves_to_internal(url).is_err() {
            return attempt.error(
                "Redirect blocked: host resolves to an internal/loopback/metadata address".to_string(),
            );
        }
        attempt.follow()
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
        // v1.7.107 perf: TCP keepalive so pooled connections survive
        // NAT / corporate firewall idle timeouts (default ~5 min on most
        // boxes). Without this, the second LLM call ~3 min into a long
        // research session forces a fresh TLS handshake (~80-150ms) even
        // though the pool intended to reuse the socket.
        .tcp_keepalive(std::time::Duration::from_secs(45))
        // v1.7.107 perf: cap idle per host. Default is unlimited, which
        // builds up garbage sockets across multi-provider sessions
        // (Gemini + Claude + Ollama all in one chat). 8 covers the
        // largest realistic concurrent fan-out (parallel read-only tools)
        // without holding sockets that will never be reused.
        .pool_max_idle_per_host(8)
        .user_agent(concat!("Lucy/", env!("CARGO_PKG_VERSION")))
        .redirect(ssrf_safe_redirect_policy())
        .build()
        .expect("Error creando cliente HTTP global")
});

// ── SSRF-hardened URL-fetch client (Phase-2 SSRF-REDIRECT-DNS-01 follow-up) ──
//
// The shared HTTP_CLIENT above MUST reach loopback (local Ollama at
// 127.0.0.1:11434) and provider APIs, so it cannot reject internal IPs wholesale.
// But `fetch_url_content` takes an LLM/agent-influenced URL and must never be an
// SSRF gadget. A pre-request DNS check still leaves a resolve→connect TOCTOU (DNS
// can flip between the check and the connect), so this dedicated client plugs a
// custom resolver: it performs the ONE resolution reqwest then dials and drops
// every internal/loopback/metadata address, so the IP reqwest connects to is the
// SAME one we validated — closing the rebinding TOCTOU on the initial hop AND every
// redirect, uniformly. Kept separate from HTTP_CLIENT precisely because the shared
// client legitimately talks to loopback.

/// Pure SSRF address filter: keep only routable public addresses. Extracted so it
/// is unit-testable without a live resolver. Empty result ⇒ caller fails closed.
fn ssrf_filter_public_addrs(addrs: Vec<std::net::SocketAddr>) -> Vec<std::net::SocketAddr> {
    addrs
        .into_iter()
        .filter(|sa| !crate::guardrails::ip_is_internal(sa.ip()))
        .collect()
}

struct SsrfGuardResolver;

impl reqwest::dns::Resolve for SsrfGuardResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            // The std resolver is blocking — run it off the async worker.
            let resolved: Vec<std::net::SocketAddr> = tokio::task::spawn_blocking(move || {
                use std::net::ToSocketAddrs;
                (host.as_str(), 0u16).to_socket_addrs().map(|it| it.collect::<Vec<_>>())
            })
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;

            let public = ssrf_filter_public_addrs(resolved);
            if public.is_empty() {
                return Err::<reqwest::dns::Addrs, Box<dyn std::error::Error + Send + Sync>>(
                    "SSRF guard: host resolves only to internal/loopback/metadata addresses".into(),
                );
            }
            let addrs: reqwest::dns::Addrs = Box::new(public.into_iter());
            Ok(addrs)
        })
    }
}

/// URL-fetch HTTP client used ONLY by `fetch_url_content`. Same short timeouts as
/// HTTP_CLIENT_FAST, plus the redirect policy (string/regex + per-hop DNS check)
/// AND the SsrfGuardResolver (resolved-IP pin) — belt-and-suspenders SSRF.
pub static FETCH_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(8))
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("Lucy/", env!("CARGO_PKG_VERSION")))
        .redirect(ssrf_safe_redirect_policy())
        .dns_resolver(std::sync::Arc::new(SsrfGuardResolver))
        .build()
        .expect("Error creando FETCH_CLIENT")
});

#[cfg(test)]
mod ssrf_resolver_tests {
    use super::ssrf_filter_public_addrs;
    use std::net::SocketAddr;

    fn sa(s: &str) -> SocketAddr { s.parse().unwrap() }

    #[test]
    fn keeps_public_drops_internal() {
        let out = ssrf_filter_public_addrs(vec![
            sa("8.8.8.8:80"), sa("127.0.0.1:80"), sa("169.254.169.254:80"),
            sa("10.0.0.5:80"), sa("1.1.1.1:80"), sa("192.168.1.9:80"),
        ]);
        assert_eq!(out, vec![sa("8.8.8.8:80"), sa("1.1.1.1:80")],
            "only routable public IPs must survive the SSRF filter (order preserved)");
    }

    #[test]
    fn all_internal_yields_empty_fail_closed() {
        // Empty result → SsrfGuardResolver returns an error → connection refused.
        let out = ssrf_filter_public_addrs(vec![
            sa("127.0.0.1:80"), sa("10.1.2.3:80"), sa("[::1]:80"),
        ]);
        assert!(out.is_empty(), "a rebinding host resolving only to internal IPs must be blocked");
    }
}

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
///
/// AÑADIR UN MODELO AL DESPLEGABLE SIN AÑADIRLO AQUÍ LO DEJA MUERTO. `ask_lucy`
/// y `ask_lucy_stream` comparan la cadena COMPLETA — el sufijo `::effort` forma
/// parte de la clave, así que cada nivel necesita su propia fila — y devuelven
/// "Modelo no permitido. Selecciona un modelo válido desde el selector" a quien
/// acaba de seleccionarlo desde el selector.
///
/// Es lo que pasó con las tres altas de modelos de julio de 2026: 19 entradas
/// del catálogo (toda la gama Claude 5, GPT-5.6, Grok y DeepSeek) llegaron al
/// desplegable sin llegar a esta lista, y ninguna funcionaba. El aviso existía
/// desde v1.7.0, en la cabecera de `src/lib/llm-models.ts` — pero un aviso solo
/// lo lee quien abre ESE fichero, y quien añade un modelo abre `models.js`.
///
/// Ahora lo sujeta un test: `catalog_contract` en utils/db.rs lee el catálogo
/// del frontend en tiempo de compilación y falla nombrando la entrada que falta.
/// Convención de la lista: el id base MÁS todos los niveles de esfuerzo que
/// acepte `resolve_anthropic_model`, no solo los que el desplegable ofrezca hoy
/// — un chat guardado o un runbook puede traer cualquiera de ellos.
pub const ALLOWED_MODELS: &[&str] = &[
    // ── Gemini 3.x lineup (refreshed May 2026 from ai.google.dev/gemini-api/docs) ──
    // Pro accepts an effort hint via "::high" / "::medium" suffix that maps to
    // thinkingConfig.thinkingLevel in resolve_gemini_model(). The base ID
    // (without suffix) is what actually hits the API.
    "gemini-3.1-pro-preview",
    "gemini-3.1-pro-preview::high",
    "gemini-3.1-pro-preview::medium",
    "gemini-3.6-flash",                 // GA — current default Flash
    "gemini-3.5-flash",                 // GA — 1M ctx, frontier-class at lower cost
    "gemini-3.5-flash-lite",            // GA — cheapest 3.5 tier
    "gemini-3.1-flash-lite",            // GA — high-volume workhorse
    "gemini-3.1-flash-lite-preview",    // preview
    // Legacy alias kept for old saved chats (resolved to gemini-3.5-flash server-side)
    "gemini-3-flash-preview",
    // ── Gemini 2.5 (legacy — kept for compat with old chats) ──
    "gemini-2.5-flash",
    "gemini-2.5-pro",
    "gemini-2.5-flash-lite-preview",
    // ── Anthropic Claude (June 2026 lineup) ──
    // Effort suffix "::low|::medium|::high|::xhigh|::max" is supported by
    // resolve_anthropic_model() in ai.rs and stripped before the API call.
    //
    // ── Claude 5 (current flagship generation) ──
    // All three accept the full effort range. Fable 5 thinks unconditionally
    // and needs 30-day data retention enabled on the org; see the notes in
    // src/lib/models.js.
    "claude-opus-5",                    // flagship — $5/$25 per 1M
    "claude-opus-5::low",
    "claude-opus-5::medium",
    "claude-opus-5::high",
    "claude-opus-5::xhigh",
    "claude-opus-5::max",
    "claude-sonnet-5",                  // balanced — $3/$15 per 1M
    "claude-sonnet-5::low",
    "claude-sonnet-5::medium",
    "claude-sonnet-5::high",
    "claude-sonnet-5::xhigh",
    "claude-sonnet-5::max",
    "claude-fable-5",                   // top tier — $10/$50 per 1M, 2× Opus 5
    "claude-fable-5::low",
    "claude-fable-5::medium",
    "claude-fable-5::high",
    "claude-fable-5::xhigh",
    "claude-fable-5::max",
    // ── Previous generation ──
    "claude-opus-4-8",                  // flagship (June 2026) — 1M ctx
    "claude-opus-4-8::low",
    "claude-opus-4-8::medium",
    "claude-opus-4-8::high",
    "claude-opus-4-8::xhigh",
    "claude-opus-4-8::max",
    "claude-opus-4-7",                  // previous gen — 1M ctx
    "claude-opus-4-7::low",
    "claude-opus-4-7::medium",
    "claude-opus-4-7::high",
    "claude-opus-4-7::xhigh",
    "claude-opus-4-7::max",
    "claude-sonnet-4-6",                // balanced — 1M ctx
    "claude-sonnet-4-6::low",
    "claude-sonnet-4-6::medium",
    "claude-sonnet-4-6::high",
    "claude-sonnet-4-6::max",
    "claude-haiku-4-5",                 // fast tier (no effort param)
    // Legacy Claude
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-3-7-sonnet-20250219",
    "claude-3-5-sonnet-latest",
    "claude-3-5-haiku-latest",
    // ── OpenAI GPT-5 family (April-May 2026) ──
    "gpt-5.6-sol",                      // flagship reasoning
    "gpt-5.6-terra",                    // balanced
    "gpt-5.6-luna",                     // fast tier
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
    // ── xAI Grok ── OpenAI-dialect, via openai_compatible_endpoint() in ai.rs
    "grok-4.5",
    "grok-4.3",
    // ── DeepSeek ── same dialect, api.deepseek.com
    "deepseek-v4-flash",
    "deepseek-v4-pro",
];
