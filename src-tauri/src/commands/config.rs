// ── CONFIG — Gestión de credenciales y configuración segura ───────────────────
// Windows Credential Manager (keyring) para API key y contraseñas de hosts.

use keyring::Entry;
use crate::state::HTTP_CLIENT;
use crate::utils::logging::write_app_log;
use crate::utils::shell::validate_host_id;

// ── API KEY ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn save_llm_key(provider: String, api_key: String) -> Result<(), String> {
    if !["gemini", "anthropic", "openai", "local"].contains(&provider.as_str()) {
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
    for provider in ["gemini", "anthropic", "openai", "local"] {
        let key_name = format!("{}_api_key", provider);
        if let Ok(entry) = Entry::new("LucySysAdmin", &key_name) {
            if entry.get_password().is_ok() {
                configured.push(provider.to_string());
            }
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
        "local" => {
            // Para modelos locales, aceptaremos la URL proporcionada asumiendo que el usuario sabe lo que hace.
            // Ping a la IP puede fallar si la herramienta aun no es lanzada (ej: Ollama).
            return Ok(());
        },
        _ => return Err("Proveedor desconocido.".to_string())
    };

    let res = req.send().await.map_err(|e| format!("Error de red al verificar clave: {}", e))?;
    
    // Anthropic returns 404 (Not Found) or 400 for structural errors if the Key is VALID.
    // If Key is INVALID it strictly returns 401 Unauthorized.
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
