//! Los proveedores de nube — para que elegir un modelo del catálogo signifique
//! algo.
//!
//! POR QUÉ EXISTE: el shell nativo ofrecía los 51 modelos del catálogo y solo
//! sabía hablar con Ollama. Elegir "Claude Opus 5" mandaba la cadena
//! `claude-opus-5::high` a Ollama como si fuera el nombre de un modelo local, y
//! el error que salía —"model not found"— no se parecía en nada a la causa. Un
//! selector que ofrece cincuenta cosas y cumple con siete es peor que uno que
//! ofrece siete.
//!
//! El transporte es deliberadamente fino: una petición, un flujo SSE, y un
//! extractor de texto por proveedor. Lo que NO se trae de `commands/ai.rs` es
//! todo lo demás —caché de prompt, reintentos con backoff, conteo de tokens,
//! herramientas, imágenes— porque cada una de esas cosas es una decisión de
//! producto con su propia migración. Aquí solo se cierra el agujero entre el
//! selector y la realidad.

use crate::chat::ChatEvent;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::{channel, Receiver};

/// A quién se le pregunta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    Gemini,
    OpenAi,
    Xai,
    DeepSeek,
    Nvidia,
    /// Local. También es el destino de cualquier id que no esté en el catálogo:
    /// los modelos de Ollama se descubren en la máquina, así que un id
    /// desconocido es casi siempre uno de ellos.
    Ollama,
}

impl Provider {
    /// La clave del secreto en el Credential Manager, con el mismo nombre que
    /// escribe la app real: `{proveedor}_api_key` bajo `LucySysAdmin`.
    fn credential(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic_api_key",
            Self::Gemini => "gemini_api_key",
            Self::OpenAi => "openai_api_key",
            Self::Xai => "xai_api_key",
            Self::DeepSeek => "deepseek_api_key",
            Self::Nvidia => "nvidia_api_key",
            Self::Ollama => "",
        }
    }

    /// Nombre para el operador, para que un error diga a quién no se pudo
    /// llamar.
    pub fn label(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Google Gemini",
            Self::OpenAi => "OpenAI",
            Self::Xai => "xAI",
            Self::DeepSeek => "DeepSeek",
            Self::Nvidia => "NVIDIA NIM",
            Self::Ollama => "Ollama",
        }
    }

    /// El endpoint compatible con OpenAI de cada casa. Tres proveedores hablan
    /// el mismo protocolo y solo cambian de URL — por eso comparten camino.
    fn openai_endpoint(self) -> &'static str {
        match self {
            Self::DeepSeek => "https://api.deepseek.com/chat/completions",
            Self::Xai => "https://api.x.ai/v1/chat/completions",
            Self::Nvidia => "https://nim.api.nvidia.com/v1/chat/completions",
            _ => "https://api.openai.com/v1/chat/completions",
        }
    }
}

/// De qué proveedor es un modelo.
///
/// Se pregunta al CATÁLOGO en vez de deducirlo del prefijo del id. Un
/// `starts_with("claude-")` funcionaría hoy y se rompería el día que alguien
/// añada un modelo de otra casa con un nombre parecido — y el catálogo ya sabe
/// la respuesta.
pub fn provider_of(model_id: &str) -> Provider {
    for g in crate::models::GROUPS {
        if g.options.iter().any(|o| o.id == model_id) {
            return match g.provider {
                "anthropic" => Provider::Anthropic,
                "gemini" => Provider::Gemini,
                "openai" => Provider::OpenAi,
                "xai" => Provider::Xai,
                "deepseek" => Provider::DeepSeek,
                "nvidia" => Provider::Nvidia,
                _ => Provider::Ollama,
            };
        }
    }
    Provider::Ollama
}

/// Separa un id de Anthropic en el modelo real y su nivel de esfuerzo.
///
/// Port de `resolve_anthropic_model` en `commands/ai.rs`. La lista blanca por
/// modelo es lo importante y no un detalle: mandar `xhigh` a un Sonnet 4.6 —que
/// no lo acepta— devuelve un 400 con un cuerpo que no dice cuál de los campos
/// sobra. Ante una combinación no soportada se quita el sufijo y se manda el
/// modelo a secas, que es una respuesta peor pero real, en vez de un error.
pub fn resolve_anthropic(raw: &str) -> (String, Option<&'static str>) {
    let Some((base, effort_raw)) = raw.split_once("::") else {
        return (raw.to_string(), None);
    };
    let effort = match effort_raw.trim().to_lowercase().as_str() {
        "low" | "bajo" => Some("low"),
        "medium" | "med" | "medio" | "balanced" => Some("medium"),
        "high" | "alto" => Some("high"),
        "xhigh" | "x-high" | "extra-alto" | "extra-high" | "extra" => Some("xhigh"),
        "max" | "maximo" | "máximo" => Some("max"),
        _ => None,
    };
    let supported: &[&str] = match base {
        "claude-opus-5" | "claude-sonnet-5" | "claude-fable-5" | "claude-opus-4-8"
        | "claude-opus-4-7" => &["low", "medium", "high", "xhigh", "max"],
        "claude-sonnet-4-6" => &["low", "medium", "high", "max"],
        "claude-opus-4-5" => &["low", "medium", "high"],
        // Haiku y los antiguos: no aceptan el parámetro en absoluto.
        _ => &[],
    };
    let final_effort = match effort {
        Some(e) if supported.contains(&e) => Some(e),
        _ => None,
    };
    (base.to_string(), final_effort)
}

/// Quita el sufijo `::nivel` de un id de Gemini. Aquí solo se limpia: el mapeo
/// a `thinkingConfig` es de la migración del constructor de payloads.
fn clean_gemini(raw: &str) -> String {
    raw.split_once("::").map_or(raw, |(b, _)| b).to_string()
}

/// Lee la clave del Credential Manager.
///
/// El valor NUNCA se registra ni se devuelve en un mensaje de error: lo único
/// que sale de aquí hacia la interfaz es si estaba o no.
fn api_key(p: Provider) -> Result<String, String> {
    let entry = keyring::Entry::new("LucySysAdmin", p.credential())
        .map_err(|_| format!("No se pudo abrir el almacén de credenciales para {}", p.label()))?;
    entry.get_password().map_err(|_| {
        format!(
            "No hay clave de API guardada para {}. Se configura en la vista de \
             Configuración de Lucy.",
            p.label()
        )
    })
}

/// Arranca una respuesta en streaming del proveedor que corresponda.
///
/// Devuelve el mismo `Receiver<ChatEvent>` que `chat::start_ollama`, así que
/// quien lo consume no tiene que saber con quién está hablando.
pub fn start(model: String, prompt: String) -> Receiver<ChatEvent> {
    let p = provider_of(&model);
    if p == Provider::Ollama {
        return crate::chat::start_ollama(model, prompt);
    }
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        if let Err(e) = stream(p, &model, &prompt, &tx) {
            let _ = tx.send(ChatEvent::Error(e));
        }
        let _ = tx.send(ChatEvent::Done);
    });
    rx
}

fn stream(
    p: Provider,
    model: &str,
    prompt: &str,
    tx: &std::sync::mpsc::Sender<ChatEvent>,
) -> Result<(), String> {
    let key = api_key(p)?;
    let (req, body) = match p {
        Provider::Anthropic => {
            let (id, effort) = resolve_anthropic(model);
            let mut body = serde_json::json!({
                "model": id,
                "max_tokens": 4096,
                "stream": true,
                "messages": [{ "role": "user", "content": prompt }],
            });
            if let Some(e) = effort {
                body["output_config"] = serde_json::json!({ "effort": e });
            }
            (
                ureq::post("https://api.anthropic.com/v1/messages")
                    .set("x-api-key", &key)
                    .set("anthropic-version", "2023-06-01"),
                body,
            )
        }
        Provider::Gemini => {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
                clean_gemini(model)
            );
            (
                // La clave va en cabecera y no en la query: una URL con el
                // secreto dentro acaba en cualquier log de proxy.
                ureq::post(&url).set("x-goog-api-key", &key),
                serde_json::json!({ "contents": [{ "parts": [{ "text": prompt }] }] }),
            )
        }
        _ => (
            ureq::post(p.openai_endpoint()).set("Authorization", &format!("Bearer {key}")),
            serde_json::json!({
                "model": model,
                "stream": true,
                "messages": [{ "role": "user", "content": prompt }],
            }),
        ),
    };

    let resp = req
        .set("content-type", "application/json")
        // `send_string` y no `send_json`: ureq entra aquí sin su característica
        // `json`, igual que en el resto de la caja. Serializar a mano cuesta una
        // línea; añadir la característica arrastra otra copia de serde al
        // binario para lo mismo.
        .send_string(&body.to_string())
        .map_err(|e| match e {
            // El cuerpo del error del proveedor es lo único que dice qué campo
            // sobra o falta; tragárselo deja al operador con un número.
            ureq::Error::Status(code, r) => {
                let detail = r.into_string().unwrap_or_default();
                format!("{} respondió HTTP {code}: {}", p.label(), truncate(&detail, 400))
            }
            other => format!("No se pudo llamar a {}: {other}", p.label()),
        })?;

    let reader = BufReader::new(resp.into_reader());
    for line in reader.lines() {
        let line = line.map_err(|e| format!("Se cortó el flujo de {}: {e}", p.label()))?;
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if let Some(t) = extract_delta(p, &v) {
            if !t.is_empty() && tx.send(ChatEvent::Token(t)).is_err() {
                // El receptor se fue —se cerró la pestaña— y no tiene sentido
                // seguir consumiendo el flujo.
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Saca el trozo de texto de un evento SSE. Cada casa lo pone en otro sitio.
pub fn extract_delta(p: Provider, v: &serde_json::Value) -> Option<String> {
    let s = match p {
        // Anthropic manda varios tipos de evento y solo uno lleva texto; los
        // demás (`message_start`, `ping`, los bloques de pensamiento) se
        // ignoran sin ruido.
        Provider::Anthropic => v
            .get("delta")
            .filter(|_| v.get("type").and_then(|t| t.as_str()) == Some("content_block_delta"))
            .and_then(|d| d.get("text"))
            .and_then(|t| t.as_str()),
        Provider::Gemini => v
            .get("candidates")?
            .get(0)?
            .get("content")?
            .get("parts")?
            .get(0)?
            .get("text")
            .and_then(|t| t.as_str()),
        _ => v
            .get("choices")?
            .get(0)?
            .get("delta")?
            .get("content")
            .and_then(|t| t.as_str()),
    }?;
    Some(s.to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn el_proveedor_sale_del_catalogo() {
        assert_eq!(provider_of("claude-opus-5::high"), Provider::Anthropic);
        assert_eq!(provider_of("gemini-3.5-flash"), Provider::Gemini);
        assert_eq!(provider_of("gpt-5.6-sol"), Provider::OpenAi);
        assert_eq!(provider_of("grok-4.5"), Provider::Xai);
        assert_eq!(provider_of("deepseek-v4-pro"), Provider::DeepSeek);
        assert_eq!(provider_of("meta/llama-3.3-70b-instruct"), Provider::Nvidia);
        // Un id que no está en el catálogo es un modelo descubierto en la
        // máquina: los de Ollama dependen de qué haya instalado.
        assert_eq!(provider_of("qwen3:4b"), Provider::Ollama);
        assert_eq!(provider_of("local-custom"), Provider::Ollama);
    }

    #[test]
    fn la_lista_blanca_de_esfuerzo_protege_de_un_400() {
        // Lo que acepta cada modelo NO es igual, y mandar un nivel que no
        // acepta devuelve un 400 cuyo cuerpo no dice cuál era el campo.
        assert_eq!(
            resolve_anthropic("claude-opus-5::xhigh"),
            ("claude-opus-5".to_string(), Some("xhigh"))
        );
        assert_eq!(
            resolve_anthropic("claude-sonnet-4-6::xhigh"),
            ("claude-sonnet-4-6".to_string(), None),
            "Sonnet 4.6 no acepta xhigh — se quita el sufijo, no se manda"
        );
        assert_eq!(
            resolve_anthropic("claude-haiku-4-5"),
            ("claude-haiku-4-5".to_string(), None),
            "Haiku no acepta el parámetro en absoluto"
        );
        // Y los sinónimos en español, que son los que están en el catálogo.
        assert_eq!(resolve_anthropic("claude-opus-5::alto").1, Some("high"));
        assert_eq!(resolve_anthropic("claude-opus-5::máximo").1, Some("max"));
        assert_eq!(resolve_anthropic("claude-opus-5::inventado").1, None);
    }

    #[test]
    fn cada_casa_pone_el_texto_en_otro_sitio() {
        assert_eq!(
            extract_delta(
                Provider::Anthropic,
                &json!({"type":"content_block_delta","delta":{"text":"hola"}})
            ),
            Some("hola".into())
        );
        // Un evento de Anthropic que NO es texto se ignora sin ruido: en un
        // turno normal llegan varios por cada uno que sí lo lleva.
        assert_eq!(
            extract_delta(Provider::Anthropic, &json!({"type":"message_start"})),
            None
        );
        assert_eq!(
            extract_delta(
                Provider::Gemini,
                &json!({"candidates":[{"content":{"parts":[{"text":"hola"}]}}]})
            ),
            Some("hola".into())
        );
        assert_eq!(
            extract_delta(
                Provider::OpenAi,
                &json!({"choices":[{"delta":{"content":"hola"}}]})
            ),
            Some("hola".into())
        );
        // El último frame de OpenAI trae `delta` vacío: no es un error.
        assert_eq!(
            extract_delta(Provider::OpenAi, &json!({"choices":[{"delta":{}}]})),
            None
        );
    }

    #[test]
    fn los_endpoints_son_los_de_cada_casa() {
        assert!(Provider::Xai.openai_endpoint().contains("api.x.ai"));
        assert!(Provider::DeepSeek.openai_endpoint().contains("api.deepseek.com"));
        assert!(Provider::Nvidia.openai_endpoint().contains("nvidia.com"));
        assert!(Provider::OpenAi.openai_endpoint().contains("api.openai.com"));
    }

    #[test]
    fn el_sufijo_de_gemini_se_limpia() {
        assert_eq!(clean_gemini("gemini-3.1-pro-preview::high"), "gemini-3.1-pro-preview");
        assert_eq!(clean_gemini("gemini-3.5-flash"), "gemini-3.5-flash");
    }
}
