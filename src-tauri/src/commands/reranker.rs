// ── commands/reranker.rs ──────────────────────────────────────────────────
//
// Cross-encoder reranker (Tier 3 #7, agentmemory-inspired). Re-scores the
// top-N candidates from search_agent_memories_expanded by feeding each
// (query, passage) pair through ms-marco-MiniLM-L-6-v2 — a 22 MB BERT-style
// model trained specifically on MS-MARCO passage relevance.
//
// Why a cross-encoder vs. our existing cosine?
//   • Cosine on Ollama embeddings is a BI-encoder: query and passage get
//     encoded INDEPENDENTLY, then dot-producted. Fast, but it can't see
//     token-level interactions ("PowerShell 7 on Windows Server 2022"
//     vs. "Windows Server 2022 PowerShell 7" look identical to cosine).
//   • Cross-encoder concatenates [CLS] query [SEP] passage [SEP] and runs
//     a full transformer pass, producing a single relevance logit. Slower
//     per pair, but ~20-30% better nDCG on top-K ranking.
//   • Standard pattern: use RRF (fast) to get top-20, then cross-encoder
//     (slow) to reorder those 20. Final top-K is way better than RRF alone.
//
// Build modes
// -----------
//   DEFAULT (no `ml-reranker` feature)   — module is a stub. Returns None.
//   `--features ml-reranker`             — full ORT + tokenizers path.
//   `--features ml-reranker,ml-guard`    — both share the ORT cone, free.
//
// Model is PUBLIC (not gated like PromptGuard): no HF token required.

use serde::Serialize;
use std::path::PathBuf;
#[cfg(feature = "ml-reranker")]
use std::sync::OnceLock;

/// Where Lucy expects the reranker files. Sibling layout to prompt_guard.
fn model_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("Lucy").join("rerankers").join("ms_marco_minilm_l6")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]  // variants used conditionally behind feature gates
pub enum Status {
    FeatureDisabled,
    ModelMissing,
    RuntimeMissing,
    Active,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusInfo {
    pub status: Status,
    pub model_path: String,
    pub note: Option<String>,
}

#[tauri::command]
pub fn reranker_status() -> StatusInfo {
    let mp = model_dir().join("model.onnx");
    let path_str = mp.to_string_lossy().to_string();

    #[cfg(not(feature = "ml-reranker"))]
    {
        StatusInfo {
            status: Status::FeatureDisabled,
            model_path: path_str,
            note: Some("Build without `ml-reranker` feature — RRF-only ranking active. Rebuild with `cargo build --features ml-reranker` to enable.".to_string()),
        }
    }

    #[cfg(feature = "ml-reranker")]
    {
        if !mp.exists() {
            return StatusInfo {
                status: Status::ModelMissing,
                model_path: path_str,
                note: Some("Call `download_reranker_model` to fetch the 22 MB ms-marco-MiniLM-L-6-v2 ONNX from HuggingFace (no token required).".to_string()),
            };
        }
        let st = ml::SESSION.get_or_init(ml::try_load);
        match st {
            Ok(_) => StatusInfo { status: Status::Active, model_path: path_str, note: None },
            Err(e) => {
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

#[cfg(not(feature = "ml-reranker"))]
mod stub {
    /// Stub: caller falls back to RRF ranking unchanged.
    pub fn rerank(_query: &str, _passages: &[&str]) -> Option<Vec<f32>> { None }
}
#[cfg(not(feature = "ml-reranker"))]
pub use stub::rerank;

#[cfg(feature = "ml-reranker")]
mod ml {
    use super::*;
    use ort::session::{Session, builder::GraphOptimizationLevel};
    use ort::value::Value;
    use tokenizers::Tokenizer;
    use ndarray::Array2;
    use std::sync::Mutex;

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
            .map_err(|e| format!("Reranker tokenizer load failed: {}", e))?;

        let session = Session::builder()
            .map_err(|e| format!("ORT session builder failed: {}", e))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| format!("ORT opt level: {}", e))?
            .commit_from_file(&onnx)
            .map_err(|e| format!("Reranker ONNX load failed (DLL or file): {}", e))?;

        Ok(Mutex::new(Loaded { session, tokenizer }))
    }

    /// Rerank N passages against one query. Returns a Vec<f32> of length
    /// N — the i-th element is the relevance score for `passages[i]`.
    /// Higher = more relevant. Returns None on any failure.
    ///
    /// Uses pair encoding: [CLS] query [SEP] passage [SEP]. The model
    /// outputs a single logit; we return it directly (no sigmoid needed —
    /// callers just sort by score, monotonic transforms don't matter).
    pub fn rerank(query: &str, passages: &[&str]) -> Option<Vec<f32>> {
        if passages.is_empty() { return Some(Vec::new()); }
        let cell = SESSION.get_or_init(super::ml::try_load);
        let mtx = cell.as_ref().ok()?;
        let mut loaded = mtx.lock().ok()?;

        // Cap sequence length at 256 — ms-marco was trained on short
        // passages, longer inputs hurt quality AND cost more compute.
        const MAX_LEN: usize = 256;
        let mut scores: Vec<f32> = Vec::with_capacity(passages.len());

        for passage in passages {
            // Encode as pair: tokenizer handles [CLS] / [SEP] / type ids
            let enc = loaded.tokenizer
                .encode((query, *passage), true)
                .ok()?;

            let ids:    Vec<i64> = enc.get_ids().iter().map(|x| *x as i64).take(MAX_LEN).collect();
            let mask:   Vec<i64> = enc.get_attention_mask().iter().map(|x| *x as i64).take(MAX_LEN).collect();
            let types:  Vec<i64> = enc.get_type_ids().iter().map(|x| *x as i64).take(MAX_LEN).collect();

            let n = ids.len();
            let ids_arr   = Array2::from_shape_vec((1, n), ids).ok()?;
            let mask_arr  = Array2::from_shape_vec((1, n), mask).ok()?;
            let types_arr = Array2::from_shape_vec((1, n), types).ok()?;

            // ort::inputs! returns a Vec<(name, Value)> in rc.10 (not Result),
            // so no trailing `.ok()?` like prompt_guard.rs has (its ml-guard
            // path is actually broken on rc.10 — needs a follow-up fix when
            // someone enables ml-guard for real).
            let inputs = ort::inputs![
                "input_ids" => Value::from_array(ids_arr).ok()?,
                "attention_mask" => Value::from_array(mask_arr).ok()?,
                "token_type_ids" => Value::from_array(types_arr).ok()?,
            ];

            let outputs = loaded.session.run(inputs).ok()?;
            let logits = outputs[0].try_extract_tensor::<f32>().ok()?;
            let (_, view) = logits;
            // ms-marco output: [batch=1, classes=1] → single logit
            let s = view.iter().next().copied().unwrap_or(0.0);
            scores.push(s);
        }
        Some(scores)
    }
}
#[cfg(feature = "ml-reranker")]
pub use ml::rerank;

// ─────────────────────────────────────────────────────────────────────────
// MODEL DOWNLOAD
// ─────────────────────────────────────────────────────────────────────────
// ms-marco-MiniLM-L-6-v2 ONNX (~22 MB). Hosted publicly — no HF auth
// required. We fetch `model.onnx` + `tokenizer.json` into the model_dir.

/// Mirror of HF using the resolve/main URL pattern. Public files.
const ONNX_URL: &str =
    "https://huggingface.co/cross-encoder/ms-marco-MiniLM-L-6-v2/resolve/main/onnx/model.onnx";
const TOKENIZER_URL: &str =
    "https://huggingface.co/cross-encoder/ms-marco-MiniLM-L-6-v2/resolve/main/tokenizer.json";

#[tauri::command]
pub async fn download_reranker_model() -> Result<String, String> {
    let dir = model_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create dir {}: {}", dir.display(), e))?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("http client: {}", e))?;

    async fn fetch_to(client: &reqwest::Client, url: &str, dest: &PathBuf) -> Result<u64, String> {
        let resp = client.get(url).send().await
            .map_err(|e| format!("GET {}: {}", url, e))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {} for {}", resp.status(), url));
        }
        let bytes = resp.bytes().await
            .map_err(|e| format!("download body: {}", e))?;
        let n = bytes.len() as u64;
        std::fs::write(dest, &bytes)
            .map_err(|e| format!("write {}: {}", dest.display(), e))?;
        Ok(n)
    }

    let onnx_path = dir.join("model.onnx");
    let tok_path  = dir.join("tokenizer.json");

    let onnx_bytes = fetch_to(&client, ONNX_URL, &onnx_path).await?;
    let tok_bytes  = fetch_to(&client, TOKENIZER_URL, &tok_path).await?;

    Ok(format!(
        "Reranker model downloaded: {} ({:.1} MB) + {} ({:.1} KB)",
        onnx_path.display(),
        onnx_bytes as f64 / 1_048_576.0,
        tok_path.display(),
        tok_bytes as f64 / 1024.0,
    ))
}
