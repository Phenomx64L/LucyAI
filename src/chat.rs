//! Chat con Ollama local — el primer "Lucy responde de verdad" del shell nativo.
//!
//! Sin secretos (Ollama local, no requiere API key), sin tokio: HTTP bloqueante
//! en un hilo de fondo que empuja tokens por un canal `mpsc` (el mismo patrón que
//! el PTY). Tauri-free: forma parte del corazón compartido `lucy-core`.

use std::io::{BufRead, BufReader};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

const OLLAMA: &str = "http://localhost:11434";

/// Los tokens que Ollama dice haber gastado, sacados de una respuesta suya sin
/// streaming: `(entrada, salida)`.
///
/// LO QUE TIRABAN TRES MÓDULOS. `titles`, `suggest` e `insights` hacen el mismo
/// `POST /api/chat` con `stream: false`, leen `message.content` y descartan el
/// resto — y en ese resto vienen los dos recuentos. Los tres devolvían
/// `(texto, 0, 0)` a pelo, así que el contador de gasto no veía pasar ni un token
/// del modelo local: en la pantalla de coste, «no cuesta dinero» y «no se está
/// midiendo» se leían igual.
///
/// En dinero da lo mismo —Ollama es gratis— pero en trabajo no: cuántos tokens
/// mastica el modelo local para poner títulos y destilar patrones es la única
/// forma de decidir si compensa tenerlo encendido.
///
/// CERO SI NO VIENEN, y eso es correcto aquí y no una pérdida disfrazada:
/// `usage::apunta` descarta la fila que no gastó nada, así que una versión de
/// Ollama que no mande los recuentos deja de escribir filas en vez de escribir
/// ceros que ensuciarían la media.
pub(crate) fn tokens_ollama(json: &serde_json::Value) -> (u32, u32) {
    let n = |k: &str| json.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    (n("prompt_eval_count"), n("eval_count"))
}

#[derive(Debug)]
pub enum ChatEvent {
    Token(String),
    /// Tokens de entrada y salida que el proveedor dice haber cobrado.
    ///
    /// Llega al final y solo si el proveedor los manda. Es un evento y no un
    /// campo de `Done` porque no todos los mandan, y un `Done` que llevara
    /// ceros obligatorios haría indistinguible "no lo dijo" de "salió gratis".
    Usage(u32, u32),
    Done,
    Error(String),
}

/// Lanza un turno de chat en STREAMING. Devuelve un `Receiver` que el GUI drena
/// por frame. El hilo hace `POST /api/chat` (stream) y parsea el NDJSON de Ollama
/// token a token.
pub fn start_ollama(
    model: String,
    turns: Vec<crate::turns::Turn>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Receiver<ChatEvent> {
    let (tx, rx) = channel();
    thread::spawn(move || {
        // Ollama habla el mismo dialecto que OpenAI: sistema, usuario y
        // asistente dentro de la misma lista.
        let msgs: Vec<serde_json::Value> = turns
            .iter()
            .map(|t| {
                let mut m = serde_json::json!({
                    "role": match t.who {
                        crate::turns::Who::System => "system",
                        crate::turns::Who::Assistant => "assistant",
                        crate::turns::Who::User => "user",
                    },
                    "content": t.text,
                });
                // Ollama sí las quiere en un campo aparte y sin tipo MIME: una
                // lista de base64 pelados. Un modelo local sin visión ignora el
                // campo, que no es lo ideal pero es mejor que el silencio de no
                // mandarlo — con `llava` o `gemma3` funciona tal cual.
                if !t.images.is_empty() {
                    m["images"] = serde_json::Value::Array(
                        t.images.iter().map(|i| serde_json::json!(i.b64)).collect(),
                    );
                }
                m
            })
            .collect();
        let body = serde_json::json!({
            "model": model,
            "messages": msgs,
            "stream": true,
        });
        let resp = match ureq::post(&format!("{OLLAMA}/api/chat"))
            .set("content-type", "application/json")
            .send_string(&body.to_string())
        {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(ChatEvent::Error(format!(
                    "Ollama inalcanzable en {OLLAMA} — ¿está corriendo? · {e}"
                )));
                return;
            }
        };
        let reader = BufReader::new(resp.into_reader());
        for line in reader.lines() {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(tok) = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                {
                    if !tok.is_empty() {
                        let _ = tx.send(ChatEvent::Token(tok.to_string()));
                    }
                }
                if v.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
                    let _ = tx.send(ChatEvent::Done);
                    return;
                }
            }
        }
        let _ = tx.send(ChatEvent::Done);
    });
    rx
}

/// Modelos instalados en Ollama (`GET /api/tags`). Bloqueante y rápido; devuelve
/// vacío si Ollama no responde.
pub fn list_models() -> Vec<String> {
    // CON PLAZO, porque sin él ureq solo limita el CONNECT. Un Ollama que acepta
    // la conexión y se queda callado —cargando un modelo de 7B en una máquina
    // corta de RAM— dejaba esta llamada esperando indefinidamente, y quien la
    // hace es el botón de redetectar, que corre en el hilo que pinta.
    let resp = match ureq::get(&format!("{OLLAMA}/api/tags"))
        .timeout(std::time::Duration::from_secs(4))
        .call()
    {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let text = match resp.into_string() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    v.get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Los recuentos que Ollama manda y tres módulos tiraban.
    ///
    /// `titles`, `suggest` e `insights` hacen el mismo `POST /api/chat` sin
    /// streaming, leen `message.content` y descartaban el resto — y en ese resto
    /// vienen los dos contadores. Los tres devolvían `(texto, 0, 0)` a pelo, así
    /// que los cubos `titulo`, `chips` y `reflexion` de la pantalla de coste no
    /// se llenaban NUNCA mientras hubiera un modelo local instalado, que es el
    /// caso por defecto porque `elige` lo prefiere. Y ahí «no cuesta dinero» y
    /// «no se está midiendo» se leían exactamente igual.
    #[test]
    fn se_leen_los_tokens_de_una_respuesta_de_ollama() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"model":"qwen3:0.6b",
                "message":{"role":"assistant","content":"Servicios detenidos"},
                "done":true,
                "prompt_eval_count":412,
                "eval_count":37}"#,
        )
        .expect("json");
        assert_eq!(tokens_ollama(&json), (412, 37));
    }

    #[test]
    fn una_respuesta_sin_recuentos_da_cero_y_no_revienta() {
        // Es correcto y no una pérdida disfrazada: `usage::apunta` descarta la
        // fila que no gastó nada, así que una versión de Ollama que no los mande
        // deja de escribir filas en vez de escribir ceros que hundirían la
        // media de tokens por llamada.
        let mudo: serde_json::Value =
            serde_json::from_str(r#"{"message":{"content":"algo"},"done":true}"#).expect("json");
        assert_eq!(tokens_ollama(&mudo), (0, 0));

        // Y un valor que no es un número tampoco: el JSON viene de fuera.
        let raro: serde_json::Value =
            serde_json::from_str(r#"{"prompt_eval_count":"muchos","eval_count":null}"#)
                .expect("json");
        assert_eq!(tokens_ollama(&raro), (0, 0));
    }
}
