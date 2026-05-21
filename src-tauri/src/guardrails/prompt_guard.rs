// ── guardrails/prompt_guard.rs ────────────────────────────────────────────
//
// PromptGuard 2 ONNX inference (LlamaFirewall Phase 2, May 2026).
//
// Catches the ~30-40% of prompt-injection attempts that the regex bank in
// `patterns.rs` misses — paraphrased jailbreaks, multilingual variants,
// social-engineering patterns. The model is Meta's PromptGuard 2 (86M
// params BERT-style classifier), exported to ONNX format and run locally
// via the `ort` crate.
//
// Build modes
// -----------
//   • DEFAULT (no `ml-guard` feature)  — the entire module is a stub.
//     `score(text)` always returns `None`, `status()` returns
//     `Status::FeatureDisabled`. The regex bank in patterns.rs continues
//     to do its job; ML inference is just absent.
//
//   • `cargo build --features ml-guard` — pulls in `ort` + `tokenizers` +
//     `ndarray`. ONNX Runtime DLLs are loaded at RUNTIME via the
//     `load-dynamic` ort feature, so the build never bundles binaries.
//     User installs them once (see PromptGuard install docs).
//
// Runtime contract
// ----------------
// The scanner calls `score(text)` for inputs that the regex bank flagged
// as "ambiguous" (HumanInTheLoop). If `score >= 0.85` we promote to Block.
// If score returns None (model not loaded, ORT missing, etc.) we fall
// through to the regex decision unchanged. Net effect: enabling ML only
// makes the guardrail STRICTER, never lets bad inputs through that the
// regex would have blocked.

use std::path::{Path, PathBuf};
use serde::Serialize;
use tauri::Emitter;
#[cfg(feature = "ml-guard")]
use std::sync::OnceLock;

/// Where Lucy expects to find the model files. Lazy-resolved on first
/// access so we don't crash startup when the path is missing.
fn model_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("Lucy").join("guardrails").join("prompt_guard_2")
}

/// Public status — exposed to the frontend via the `prompt_guard_status`
/// Tauri command. Lets the UI show "✓ ML guard active" or "✗ Model not
/// installed (click to download)".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]  // variants used conditionally behind ml-guard feature gate
pub enum Status {
    /// Built without the `ml-guard` feature. Inference is impossible.
    FeatureDisabled,
    /// Feature enabled but the model files aren't on disk yet.
    ModelMissing,
    /// Feature enabled, files present, but ORT runtime DLLs not loaded.
    /// User needs to install ONNX Runtime separately (one-time).
    RuntimeMissing,
    /// Everything is loaded and inference is active.
    Active,
    /// Last load attempt failed — see the `note` for the error reason.
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusInfo {
    pub status: Status,
    /// Expected path of the ONNX file (whether or not it exists).
    pub model_path: String,
    pub note: Option<String>,
}

pub fn status_info() -> StatusInfo {
    let mp = model_dir().join("model.onnx");
    let path_str = mp.to_string_lossy().to_string();

    #[cfg(not(feature = "ml-guard"))]
    {
        StatusInfo {
            status: Status::FeatureDisabled,
            model_path: path_str,
            note: Some("Build without `ml-guard` feature — regex-only guardrail active.".to_string()),
        }
    }

    #[cfg(feature = "ml-guard")]
    {
        if !mp.exists() {
            return StatusInfo {
                status: Status::ModelMissing,
                model_path: path_str,
                note: Some("Download PromptGuard 2 ONNX from HuggingFace and place at the above path.".to_string()),
            };
        }
        let st = ml::SESSION.get_or_init(ml::try_load);
        match st {
            Ok(_) => StatusInfo { status: Status::Active, model_path: path_str, note: None },
            Err(e) => {
                // If the error message mentions "load" or "dll" assume the runtime is the problem
                let lower = e.to_ascii_lowercase();
                let s = if lower.contains("dll") || lower.contains("load library") || lower.contains("dylib") {
                    Status::RuntimeMissing
                } else {
                    Status::Failed
                };
                StatusInfo { status: s, model_path: path_str, note: Some(e.clone()) }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// FEATURE-GATED IMPLEMENTATION
// ─────────────────────────────────────────────────────────────────────────

#[cfg(not(feature = "ml-guard"))]
mod stub {
    /// Stub for default builds. Always returns None so the regex bank's
    /// decision passes through unmodified.
    pub fn score(_text: &str) -> Option<f32> { None }
}
#[cfg(not(feature = "ml-guard"))]
pub use stub::score;

#[cfg(feature = "ml-guard")]
mod ml {
    use super::*;
    use ort::session::{Session, builder::GraphOptimizationLevel};
    use ort::value::Value;
    use tokenizers::Tokenizer;
    use ndarray::{Array1, Array2, s};
    use std::sync::Mutex;

    /// Singleton session — initialized on first call to `score()`. Stored
    /// as Result<Loaded, ErrMsg> so we can show the error in the UI.
    pub(super) static SESSION: OnceLock<Result<Mutex<Loaded>, String>> = OnceLock::new();

    pub(super) struct Loaded {
        session: Session,
        tokenizer: Tokenizer,
    }

    pub(super) fn try_load() -> Result<Mutex<Loaded>, String> {
        let dir = model_dir();
        let onnx = dir.join("model.onnx");
        let tok  = dir.join("tokenizer.json");

        let tokenizer = Tokenizer::from_file(&tok)
            .map_err(|e| format!("Tokenizer load failed: {}", e))?;

        let session = Session::builder()
            .map_err(|e| format!("ORT session builder failed: {}", e))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| format!("ORT opt level: {}", e))?
            .commit_from_file(&onnx)
            .map_err(|e| format!("ORT model load failed (DLL or file): {}", e))?;

        Ok(Mutex::new(Loaded { session, tokenizer }))
    }

    /// Run PromptGuard inference on `text`. Returns the predicted
    /// jailbreak probability in [0.0, 1.0], or None on any failure.
    ///
    /// The model output shape is [1, 3] (3-class softmax over BENIGN /
    /// INJECTION / JAILBREAK). We take max(INJECTION_prob, JAILBREAK_prob)
    /// to get a single "bad" score.
    pub fn score(text: &str) -> Option<f32> {
        let cell = SESSION.get_or_init(super::ml::try_load);
        let mtx = cell.as_ref().ok()?;
        let mut loaded = mtx.lock().ok()?;

        // Tokenize. PromptGuard 2 uses sequence length 512; we truncate
        // longer inputs to the first 512 tokens (the head of an attack
        // is almost always where the injection signature lives).
        let enc = loaded.tokenizer.encode(text, true).ok()?;
        let ids:   Vec<i64> = enc.get_ids().iter().map(|x| *x as i64).take(512).collect();
        let mask:  Vec<i64> = enc.get_attention_mask().iter().map(|x| *x as i64).take(512).collect();

        let n = ids.len();
        let ids_arr  = Array2::from_shape_vec((1, n), ids).ok()?;
        let mask_arr = Array2::from_shape_vec((1, n), mask).ok()?;

        let inputs = ort::inputs![
            "input_ids" => Value::from_array(ids_arr).ok()?,
            "attention_mask" => Value::from_array(mask_arr).ok()?,
        ].ok()?;

        let outputs = loaded.session.run(inputs).ok()?;
        let logits = outputs[0].try_extract_tensor::<f32>().ok()?;
        let (_, view) = logits;
        // Softmax over the 3 classes (batch=1, so first row only).
        let row: Vec<f32> = view.iter().copied().take(3).collect();
        if row.len() < 3 { return None; }
        let max = row.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = row.iter().map(|x| (x - max).exp()).collect();
        let sum: f32 = exps.iter().sum();
        if sum <= 0.0 { return None; }
        let probs: Vec<f32> = exps.iter().map(|x| x / sum).collect();
        // class 0 = benign, 1 = injection, 2 = jailbreak. Bad = 1+2.
        let bad = probs.get(1).copied().unwrap_or(0.0) + probs.get(2).copied().unwrap_or(0.0);
        Some(bad.clamp(0.0, 1.0))
    }
}
#[cfg(feature = "ml-guard")]
pub use ml::score;

// ─────────────────────────────────────────────────────────────────────────
// MODEL DOWNLOAD (from HuggingFace)
// ─────────────────────────────────────────────────────────────────────────
//
// Downloads `model.onnx` + `tokenizer.json` into `%APPDATA%\Lucy\guardrails
// \prompt_guard_2\`. The user must supply a HuggingFace token with read
// access AND have accepted the Meta license on the gated model page
// (otherwise HF returns 401/403).
//
// We emit `prompt_guard:download` events at each phase so the UI can
// render a progress bar. Event payload:
//   { file: string, phase: "start" | "progress" | "done" | "error",
//     bytes_received?: u64, bytes_total?: u64, error?: string }

#[derive(Debug, Clone, Serialize)]
struct DownloadEvent<'a> {
    file: &'a str,
    phase: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes_received: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// HuggingFace model repository for PromptGuard 2.
/// Meta's `meta-llama/Llama-Prompt-Guard-2-86M` is gated — user needs
/// a token with read access + accepted license.
const HF_REPO: &str = "meta-llama/Llama-Prompt-Guard-2-86M";

pub async fn download_model(hf_token: &str, app: &tauri::AppHandle) -> Result<String, String> {
    if hf_token.trim().is_empty() {
        return Err("HuggingFace token requerido. Genera uno en huggingface.co/settings/tokens".to_string());
    }
    let dir = model_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("No se pudo crear {}: {}", dir.display(), e))?;

    // Two files we need from the repo root
    for filename in &["model.onnx", "tokenizer.json"] {
        download_one_file(hf_token, filename, &dir, app).await?;
    }

    Ok(format!("Modelo descargado en {}", dir.display()))
}

async fn download_one_file(
    token: &str,
    filename: &str,
    dest_dir: &Path,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    use futures_util::StreamExt;

    let url = format!("https://huggingface.co/{}/resolve/main/{}", HF_REPO, filename);
    let dest = dest_dir.join(filename);

    let _ = app.emit("prompt_guard:download", DownloadEvent {
        file: filename, phase: "start",
        bytes_received: None, bytes_total: None, error: None,
    });

    let res = crate::state::HTTP_CLIENT
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", concat!("Lucy/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|e| {
            let msg = format!("HF request failed for {}: {}", filename, e);
            let _ = app.emit("prompt_guard:download", DownloadEvent {
                file: filename, phase: "error",
                bytes_received: None, bytes_total: None, error: Some(msg.clone()),
            });
            msg
        })?;

    if !res.status().is_success() {
        let code = res.status().as_u16();
        let body = res.text().await.unwrap_or_default();
        let snippet = &body[..body.len().min(200)];
        let msg = match code {
            401 => "HF 401 — token inválido o sin acceso al modelo gated.".to_string(),
            403 => format!("HF 403 — debes aceptar la licencia en huggingface.co/{} antes de descargar.", HF_REPO),
            404 => format!("HF 404 — archivo {} no encontrado en el repo.", filename),
            _   => format!("HF HTTP {}: {}", code, snippet),
        };
        let _ = app.emit("prompt_guard:download", DownloadEvent {
            file: filename, phase: "error",
            bytes_received: None, bytes_total: None, error: Some(msg.clone()),
        });
        return Err(msg);
    }

    let total = res.content_length();
    let mut received: u64 = 0;
    let mut stream = res.bytes_stream();

    // Write to a `.partial` file then rename atomically at the end so
    // an interrupted download never leaves a corrupted onnx that would
    // crash the loader on next startup.
    let tmp = dest.with_extension("partial");
    let mut file = std::fs::File::create(&tmp)
        .map_err(|e| format!("create {} failed: {}", tmp.display(), e))?;
    use std::io::Write;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| {
            let msg = format!("stream error on {}: {}", filename, e);
            let _ = app.emit("prompt_guard:download", DownloadEvent {
                file: filename, phase: "error",
                bytes_received: Some(received), bytes_total: total, error: Some(msg.clone()),
            });
            msg
        })?;
        file.write_all(&chunk).map_err(|e| format!("write failed: {}", e))?;
        received += chunk.len() as u64;
        // Throttle progress events: emit roughly every 256 KB to avoid
        // flooding the bridge during a 280 MB download.
        if received % (256 * 1024) < chunk.len() as u64 {
            let _ = app.emit("prompt_guard:download", DownloadEvent {
                file: filename, phase: "progress",
                bytes_received: Some(received), bytes_total: total, error: None,
            });
        }
    }
    drop(file);

    // Atomic rename — Windows requires the destination to NOT exist
    let _ = std::fs::remove_file(&dest);
    std::fs::rename(&tmp, &dest)
        .map_err(|e| format!("rename {} → {} failed: {}", tmp.display(), dest.display(), e))?;

    let _ = app.emit("prompt_guard:download", DownloadEvent {
        file: filename, phase: "done",
        bytes_received: Some(received), bytes_total: total, error: None,
    });
    Ok(())
}
