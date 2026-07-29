// ── CONFIG — Gestión de credenciales y configuración segura ───────────────────
// Windows Credential Manager (keyring) para API key y contraseñas de hosts.

use keyring::Entry;
// SECURITY: config.rs solo hace test_api_key (healthcheck rápido).
// Usa HTTP_CLIENT_FAST (15s) en lugar del HTTP_CLIENT de streaming.
use crate::state::HTTP_CLIENT_FAST as HTTP_CLIENT;
use crate::utils::logging::write_app_log;
use crate::utils::shell::validate_host_id;
use serde_json;

// ── API KEY ───────────────────────────────────────────────────────────────────

/// Providers whose API key lives in the Credential Manager as
/// `<provider>_api_key` — the same name `ai.rs` derives when it needs one.
///
/// Single source for BOTH the save whitelist and the configured-status scan.
/// They used to be two separate literals, and the failure mode of adding a
/// provider to one but not the other is quiet either way: listed in the save
/// list only, and the key saves but the settings row still reads "sin
/// configurar"; listed in the scan only, and saving is rejected as an invalid
/// provider with a working key in hand.
///
/// `tavily` is here despite not being an LLM provider — it is the web-search
/// backend and shares the keyring scheme for UI uniformity.
pub(crate) const KEYED_PROVIDERS: &[&str] = &[
    "gemini", "anthropic", "openai", "xai", "deepseek", "local", "nvidia", "tavily",
];

#[tauri::command]
pub fn save_llm_key(provider: String, api_key: String) -> Result<(), String> {
    if !KEYED_PROVIDERS.contains(&provider.as_str()) {
        return Err("Proveedor no válido.".to_string());
    }
    let key_name = format!("{}_api_key", provider);
    let entry = Entry::new("LucySysAdmin", &key_name).map_err(|e| {
        write_app_log("ERROR", &format!("Fallo al acceder a Keyring: {}", e));
        e.to_string()
    })?;
    entry.set_password(&api_key).map_err(|e| {
        write_app_log("ERROR", &format!("Fallo al guardar clave en Keyring: {}", e));
        e.to_string()
    })?;
    write_app_log("INFO", &format!("Clave de {} guardada en el Credential Manager.", provider));
    Ok(())
}

#[tauri::command]
pub fn get_configured_providers() -> Result<Vec<String>, String> {
    let mut configured = Vec::new();
    // v1.7.238 — distinguir "no configurado" (NoEntry) de un error REAL/transitorio
    // del keyring. Antes cualquier fallo (`.is_ok()`) se leía como "no configurado":
    // con auto-arranque, una race del Credential Manager justo tras el logon
    // devolvía lista vacía → el frontend mandaba a Lucy a la pantalla de setup como
    // instalación virgen TODA la noche. Ahora, si el keyring falla con algo que NO
    // es NoEntry y NO se pudo leer ninguna clave, devolvemos error para que el
    // frontend reintente antes de asumir "sin configurar".
    // `tavily` va incluido para que la UI renderice su fila de estado (es una
    // herramienta auxiliar de búsqueda web, no un proveedor LLM).
    let mut transient_err: Option<String> = None;
    for provider in KEYED_PROVIDERS.iter().copied() {
        let key_name = format!("{}_api_key", provider);
        match Entry::new("LucySysAdmin", &key_name) {
            Ok(entry) => match entry.get_password() {
                Ok(_) => configured.push(provider.to_string()),
                Err(keyring::Error::NoEntry) => { /* genuinamente sin configurar */ }
                Err(e) => { transient_err.get_or_insert(format!("{}: {}", provider, e)); }
            },
            Err(e) => { transient_err.get_or_insert(format!("{} entry: {}", provider, e)); }
        }
    }
    if configured.is_empty() {
        if let Some(e) = transient_err {
            write_app_log(
                "WARNING",
                &format!("get_configured_providers: keyring inaccesible ({}) — el frontend debe reintentar antes de asumir 'sin configurar'.", e),
            );
            return Err(format!("keyring transiently unavailable: {}", e));
        }
    }
    Ok(configured)
}

/// Verifica que una API Key sea válida
#[tauri::command]
pub async fn test_api_key(provider: String, api_key: String) -> Result<(), String> {
    let key = api_key.trim().to_string();
    if key.is_empty() { return Err("La clave no puede estar vacía.".to_string()); }

    let req = match provider.as_str() {
        "gemini" => {
            let url = format!("https://generativelanguage.googleapis.com/v1beta/models?key={}&pageSize=1", key);
            HTTP_CLIENT.get(&url)
        },
        "openai" => {
            HTTP_CLIENT.get("https://api.openai.com/v1/models")
                .header("Authorization", format!("Bearer {}", key))
        },
        "anthropic" => {
            // Anthropic no tiene /models abierto que sea fácil de testear sin payload full, pero
            // pedir models sin version da un error si la key sirve o AuthError si no.
            HTTP_CLIENT.get("https://api.anthropic.com/v1/models")
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
        },
        // Both speak the OpenAI dialect, so GET /models validates the key the
        // same way it does for OpenAI — a real round trip, not a format check.
        "xai" => {
            HTTP_CLIENT.get("https://api.x.ai/v1/models")
                .header("Authorization", format!("Bearer {}", key))
        },
        "deepseek" => {
            HTTP_CLIENT.get("https://api.deepseek.com/models")
                .header("Authorization", format!("Bearer {}", key))
        },
        "nvidia" => {
            // NVIDIA NIM free tier does not expose GET /v1/models on nim.api.nvidia.com.
            // Validate by format only: all NIM keys start with "nvapi-" and are ~80 chars.
            // Real validation happens on the first actual inference call.
            if !key.starts_with("nvapi-") {
                return Err(
                    "La clave NVIDIA NIM debe comenzar con 'nvapi-'. \
                     Consíguela en build.nvidia.com → selecciona un modelo → 'Get API Key'.".to_string()
                );
            }
            if key.len() < 30 {
                return Err("La clave NVIDIA NIM parece incompleta (muy corta).".to_string());
            }
            write_app_log("INFO", "Clave NVIDIA NIM validada por formato (nvapi-...).");
            return Ok(());
        },
        "local" => {
            // Para modelos locales, aceptaremos la URL proporcionada asumiendo que el usuario sabe lo que hace.
            return Ok(());
        },
        "tavily" => {
            // Tavily keys are prefixed "tvly-" and ~40 chars. Validate by
            // calling the search endpoint with a trivial query — single
            // credit cost (~1¢/1000 searches on free tier).
            if !key.starts_with("tvly-") {
                return Err(
                    "La clave Tavily debe comenzar con 'tvly-'. \
                     Consíguela en app.tavily.com → API Keys (free tier: 1000 searches/mes).".to_string()
                );
            }
            let body = serde_json::json!({
                "api_key": key, "query": "ping", "max_results": 1, "include_answer": false
            });
            HTTP_CLIENT.post("https://api.tavily.com/search")
                .header("Content-Type", "application/json")
                .json(&body)
        },
        _ => return Err("Proveedor desconocido.".to_string())
    };

    let res = req.send().await.map_err(|e| format!("Error de red al verificar clave: {}", e))?;

    // Anthropic returns 404/400 for structural errors when the Key IS VALID; only 401 = invalid.
    if res.status().is_success() || (provider == "anthropic" && res.status().as_u16() != 401) {
        write_app_log("INFO", &format!("API key de {} verificada correctamente.", provider));
        Ok(())
    } else {
        Err(format!("API Key de {} inválida o sin permisos.", provider))
    }
}

// ── HOST CREDENTIALS ──────────────────────────────────────────────────────────

/// Guarda la contraseña de un host remoto en el Credential Manager de Windows.
/// La clave se almacena como "LucyHost_{host_id}" bajo el servicio "LucySysAdmin".
#[tauri::command]
pub fn save_host_credential(host_id: String, password: String) -> Result<(), String> {
    validate_host_id(&host_id)?;
    let key = format!("LucyHost_{}", host_id);
    let entry = Entry::new("LucySysAdmin", &key).map_err(|e| e.to_string())?;
    entry.set_password(&password).map_err(|e| e.to_string())?;
    write_app_log("INFO", &format!("Credencial guardada para host: {}", host_id));
    Ok(())
}

/// Recupera la contraseña de un host desde el Credential Manager.
#[tauri::command]
pub fn get_host_credential(host_id: String) -> Result<String, String> {
    validate_host_id(&host_id)?;
    let key = format!("LucyHost_{}", host_id);
    let entry = Entry::new("LucySysAdmin", &key).map_err(|e| e.to_string())?;
    entry.get_password().map_err(|_| "Credencial no encontrada".to_string())
}

/// Elimina la contraseña de un host del Credential Manager.
#[tauri::command]
pub fn delete_host_credential(host_id: String) -> Result<(), String> {
    validate_host_id(&host_id)?;
    let key = format!("LucyHost_{}", host_id);
    let entry = Entry::new("LucySysAdmin", &key).map_err(|e| e.to_string())?;
    entry.delete_password().map_err(|e| e.to_string())?;
    write_app_log("INFO", &format!("Credencial eliminada para host: {}", host_id));
    Ok(())
}

// ── MCP SECRETS ───────────────────────────────────────────────────────────────
// Los secretos MCP (tokens, API keys de herramientas externas) se guardan en el
// Credential Manager del SO — nunca en localStorage (texto plano).

/// Guarda un secreto MCP en el Credential Manager.
/// La clave se almacena como "LucyMCP_{name}" bajo el servicio "LucySysAdmin".
#[tauri::command]
pub fn save_mcp_secret(name: String, value: String) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 || name.contains('\n') {
        return Err("Nombre de secreto MCP inválido.".to_string());
    }
    let key = format!("LucyMCP_{}", name);
    let entry = Entry::new("LucySysAdmin", &key).map_err(|e| e.to_string())?;
    entry.set_password(&value).map_err(|e| e.to_string())?;
    write_app_log("INFO", &format!("Secreto MCP '{}' guardado en Credential Manager.", name));
    Ok(())
}

/// Recupera un secreto MCP del Credential Manager.
#[tauri::command]
pub fn get_mcp_secret(name: String) -> Result<String, String> {
    let key = format!("LucyMCP_{}", name);
    let entry = Entry::new("LucySysAdmin", &key).map_err(|e| e.to_string())?;
    entry.get_password().map_err(|_| "Secreto no encontrado".to_string())
}

/// Elimina un secreto MCP del Credential Manager.
#[tauri::command]
pub fn delete_mcp_secret(name: String) -> Result<(), String> {
    let key = format!("LucyMCP_{}", name);
    let entry = Entry::new("LucySysAdmin", &key).map_err(|e| e.to_string())?;
    entry.delete_password().map_err(|e| e.to_string())?;
    write_app_log("INFO", &format!("Secreto MCP '{}' eliminado.", name));
    Ok(())
}

/// Devuelve la lista de nombres de secretos MCP almacenados.
/// Utiliza un índice serializado en el Credential Manager.
#[tauri::command]
pub fn list_mcp_secrets() -> Result<Vec<String>, String> {
    // Guardamos el índice de nombres como JSON bajo "LucyMCP__index"
    let idx_entry = Entry::new("LucySysAdmin", "LucyMCP__index").map_err(|e| e.to_string())?;
    match idx_entry.get_password() {
        Ok(raw) => serde_json::from_str::<Vec<String>>(&raw).map_err(|e| e.to_string()),
        Err(_) => Ok(vec![]),
    }
}

/// Actualiza el índice de nombres de secretos MCP.
#[tauri::command]
pub fn set_mcp_secret_index(names: Vec<String>) -> Result<(), String> {
    let idx_entry = Entry::new("LucySysAdmin", "LucyMCP__index").map_err(|e| e.to_string())?;
    let json = serde_json::to_string(&names).map_err(|e| e.to_string())?;
    idx_entry.set_password(&json).map_err(|e| e.to_string())
}
