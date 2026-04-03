// ── STATE — Statics compartidos entre todos los módulos ───────────────────────
// Centralizar aquí evita importaciones circulares y facilita testing.

use reqwest::Client;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use once_cell::sync::Lazy;

/// Flag de creación sin ventana de consola (Windows-only).
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Sesiones de streaming activas: session_id → stdin handle del proceso hijo.
/// Permite enviar input interactivo (sudo, contraseñas, y/n) a comandos en ejecución.
pub static STREAM_SESSIONS: Lazy<StdMutex<HashMap<String, Arc<StdMutex<std::process::ChildStdin>>>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));

pub static STREAM_PIDS: Lazy<StdMutex<HashMap<String, u32>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));

/// Tokens de seguridad efímeros para bypass en bloqueos de PowerShell.
/// Mapea Un Token Aleatorio -> Comando Original
pub static BYPASS_TOKENS: Lazy<StdMutex<HashMap<String, String>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));

/// Cliente HTTP global con pool de conexiones — un único cliente para toda la app
/// evita crear sockets nuevos en cada llamada y amortiza TLS handshake.
pub static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("Error creando cliente HTTP global")
});

/// Whitelist explícita de modelos Gemini permitidos.
/// Previene llamadas a endpoints arbitrarios si el frontend envía un modelo inválido.
pub const ALLOWED_MODELS: &[&str] = &[
    "gemini-2.5-flash",
    "gemini-2.5-pro",
    "gemini-2.5-flash-lite",
    "gemini-3-flash-preview",
    "gemini-3.1-pro-preview",
    "gemini-3.1-flash-lite-preview",
    "claude-3-7-sonnet-20250219",
    "claude-3-5-sonnet-latest",
    "claude-3-5-haiku-latest",
    "claude-sonnet-4-5",
    "claude-sonnet-4-6",
    "gpt-4.5-preview",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.4-nano",
    "gpt-4o",
    "gpt-4o-mini",
    "o1",
    "o3-mini"
];
