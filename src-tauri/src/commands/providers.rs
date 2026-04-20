// ── Provider Management Commands ───────────────────────────────────────────────
//
// Handles:
//   - Credential storage/retrieval via system keyring
//   - Provider health checks (can we connect?)
//   - Listing available models per provider
//

use serde::{Deserialize, Serialize};
use keyring::Entry;
use crate::state::HTTP_CLIENT;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProviderHealth {
    pub status: String,    // "ok" | "error" | "unconfigured"
    pub message: String,
    pub models_available: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CredentialSaveResult {
    pub success: bool,
    pub message: String,
}

/// Save an API key or credential to the system keyring
#[tauri::command]
pub async fn save_credential(key: String, value: String) -> Result<CredentialSaveResult, String> {
    if value.is_empty() {
        return Err("Credential cannot be empty".into());
    }

    // Parse key format: "provider_api_key" or "provider_endpoint"
    let service = "LucySysAdmin";
    let credential_key = if key.ends_with("_api_key") || key.ends_with("_endpoint") {
        key
    } else {
        format!("{}_api_key", key)
    };

    match Entry::new(service, &credential_key) {
        Ok(entry) => {
            match entry.set_password(&value) {
                Ok(_) => Ok(CredentialSaveResult {
                    success: true,
                    message: format!("Credential '{}' saved successfully", credential_key),
                }),
                Err(e) => Err(format!("Failed to save credential: {}", e)),
            }
        }
        Err(e) => Err(format!("Keyring error: {}", e)),
    }
}

/// Retrieve a credential from the system keyring
#[tauri::command]
pub async fn get_credential(key: String) -> Result<String, String> {
    let service = "LucySysAdmin";
    let credential_key = if key.ends_with("_api_key") || key.ends_with("_endpoint") {
        key
    } else {
        format!("{}_api_key", key)
    };

    match Entry::new(service, &credential_key) {
        Ok(entry) => match entry.get_password() {
            Ok(pass) => Ok(pass),
            Err(_) => Err("Credential not found".into()),
        },
        Err(e) => Err(format!("Keyring error: {}", e)),
    }
}

/// Check provider health/connectivity
#[tauri::command]
pub async fn check_provider_health(provider: String) -> Result<ProviderHealth, String> {
    match provider.as_str() {
        "anthropic" => check_anthropic_health().await,
        "gemini" => check_gemini_health().await,
        "openai" => check_openai_health().await,
        "ollama" => check_ollama_health().await,
        _ => Err(format!("Unknown provider: {}", provider)),
    }
}

async fn check_anthropic_health() -> Result<ProviderHealth, String> {
    match Entry::new("LucySysAdmin", "anthropic_api_key")
        .and_then(|e| e.get_password())
    {
        Ok(api_key) => {
            let resp = HTTP_CLIENT
                .get("https://api.anthropic.com/v1/models")
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    Ok(ProviderHealth {
                        status: "ok".to_string(),
                        message: "Anthropic API: Connection successful".to_string(),
                        models_available: Some(4),
                    })
                }
                Ok(r) => {
                    Err(format!(
                        "Anthropic API error: {}",
                        r.status().canonical_reason().unwrap_or("Unknown")
                    ))
                }
                Err(e) => Err(format!("Anthropic connection error: {}", e)),
            }
        }
        Err(_) => Err("Anthropic API key not configured".into()),
    }
}

async fn check_gemini_health() -> Result<ProviderHealth, String> {
    match Entry::new("LucySysAdmin", "gemini_api_key")
        .and_then(|e| e.get_password())
    {
        Ok(api_key) => {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models?key={}",
                api_key
            );

            let resp = HTTP_CLIENT.get(&url).send().await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    Ok(ProviderHealth {
                        status: "ok".to_string(),
                        message: "Google Gemini API: Connection successful".to_string(),
                        models_available: Some(4),
                    })
                }
                Ok(r) => {
                    Err(format!(
                        "Gemini API error: {}",
                        r.status().canonical_reason().unwrap_or("Unknown")
                    ))
                }
                Err(e) => Err(format!("Gemini connection error: {}", e)),
            }
        }
        Err(_) => Err("Gemini API key not configured".into()),
    }
}

async fn check_openai_health() -> Result<ProviderHealth, String> {
    match Entry::new("LucySysAdmin", "openai_api_key")
        .and_then(|e| e.get_password())
    {
        Ok(api_key) => {
            let resp = HTTP_CLIENT
                .get("https://api.openai.com/v1/models")
                .header("Authorization", format!("Bearer {}", api_key))
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    Ok(ProviderHealth {
                        status: "ok".to_string(),
                        message: "OpenAI API: Connection successful".to_string(),
                        models_available: Some(3),
                    })
                }
                Ok(r) => {
                    Err(format!(
                        "OpenAI API error: {}",
                        r.status().canonical_reason().unwrap_or("Unknown")
                    ))
                }
                Err(e) => Err(format!("OpenAI connection error: {}", e)),
            }
        }
        Err(_) => Err("OpenAI API key not configured".into()),
    }
}

async fn check_ollama_health() -> Result<ProviderHealth, String> {
    let endpoint = std::env::var("OLLAMA_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());

    let resp = HTTP_CLIENT
        .get(&format!("{}/api/tags", endpoint))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            Ok(ProviderHealth {
                status: "ok".to_string(),
                message: format!("Ollama: Connected to {}", endpoint),
                models_available: Some(1),
            })
        }
        Ok(r) => {
            Err(format!(
                "Ollama error: {} (make sure Ollama is running)",
                r.status().canonical_reason().unwrap_or("Unknown")
            ))
        }
        Err(e) => Err(format!(
            "Ollama connection error at {}: {}",
            endpoint, e
        )),
    }
}
