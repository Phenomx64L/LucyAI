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

use std::path::PathBuf;
use serde::Serialize;
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
        return StatusInfo {
            status: Status::FeatureDisabled,
            model_path: path_str,
            note: Some("Build without `ml-guard` feature — regex-only guardrail active.".to_string()),
        };
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
