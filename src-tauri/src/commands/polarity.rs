// ── polarity.rs — Polarity Axis Triangulation (v1.6.5) ───────────────────
//
// Implements Kappa Graph ADR-058 (Polarity Axis Triangulation) from the
// mirror in docs/research/kappa-graph/adrs/ADR-058-polarity-axis-triangulation.md
//
// ── The math ────────────────────────────────────────────────────────────
//
// For n opposing pairs (p₁⁺, p₁⁻) … (pₙ⁺, pₙ⁻):
//
//     Δᵢ = E(pᵢ⁺) − E(pᵢ⁻)             // difference vector per pair
//
//          Σ Δᵢ
//     a = ──────                       // normalized polarity axis
//         ‖Σ Δᵢ‖
//
// For any text t, its polarity score is the dot product of its embedding
// projected onto the axis:
//
//     π(t) = E(t) · a                   // scalar in roughly [-1, +1]
//
// Why "triangulate" with multiple pairs instead of a single one: the ADR
// notes individual pair similarity creates weak signal (~81%) but
// averaged difference vectors amplify the true semantic direction.
//
// ── What Lucy uses this for ─────────────────────────────────────────────
//
// Today (chip_memory.rs) we classify chip events with a hard-coded
// string match: 'thumbs_up' → 'click' (positive), 'thumbs_down' →
// 'dismiss' (negative). That breaks the moment an LLM proposes a novel
// event_kind we haven't seen.
//
// With this module, ANY string of arbitrary new vocabulary can be
// scored on a continuous [-1, +1] scale by computing its embedding and
// projecting onto the cached axis. No more hard-coded mapping.
//
// ── Caching policy ──────────────────────────────────────────────────────
//
// The axis is expensive to compute (2 × n_pairs embedding calls). We
// cache it in a process-lifetime OnceCell. Invalidation triggers:
//   - Process restart (cold start always rebuilds)
//   - Explicit memory_polarity_rebuild Tauri command
//   - First-time fetch when not present
//
// We do NOT auto-invalidate on every embedding-model change because
// callers might want to compare scores across model swaps. Manual
// rebuild is the right discipline per ADR-058 §"Caching".

use crate::commands::embeddings::embed_via_ollama_pub;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

// ── Default anchor pairs (5 SP/EN pairs) ────────────────────────────────
//
// These define the SUPPORTS ↔ CONTRADICTS axis for Lucy's memory layer.
// SP/EN coverage avoids language drift when the user switches locale.
// Each pair is in the SAME language so the difference captures the
// semantic axis, not a cross-language artifact.
pub const DEFAULT_ANCHOR_PAIRS: &[(&str, &str)] = &[
    // English
    ("supports",                      "contradicts"),
    ("confirms the claim",            "refutes the claim"),
    ("agrees with the statement",     "disagrees with the statement"),
    // Spanish
    ("soporta la afirmación",         "refuta la afirmación"),
    ("confirma este hecho",           "contradice este hecho"),
];

// ── Cache ──────────────────────────────────────────────────────────────
//
// Wrapped in RwLock so concurrent readers don't block each other after
// the axis is built. Tokio's RwLock is async-aware so we can `.await`
// inside the read guard without poisoning.

static AXIS_CACHE: tokio::sync::OnceCell<RwLock<Option<PolarityAxis>>> =
    tokio::sync::OnceCell::const_new();

async fn cache_handle() -> &'static RwLock<Option<PolarityAxis>> {
    AXIS_CACHE.get_or_init(|| async { RwLock::new(None) }).await
}

// ── Types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolarityAxis {
    /// Normalized axis vector — same dimensionality as the embedder.
    pub axis:        Vec<f32>,
    /// Embedder model that produced the anchor embeddings (for diagnostics).
    pub model:       String,
    /// Number of anchor pairs that contributed to the axis.
    pub n_pairs:     usize,
    /// Unix epoch seconds when the axis was built.
    pub built_at:    i64,
    /// Magnitude of the un-normalized sum, for diagnostics. A high value
    /// means the anchor pairs agree on the axis direction; near-zero
    /// means they cancel out and the axis is unreliable.
    pub raw_norm:    f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolarityProjection {
    /// Projected score in roughly [-1, +1]. Positive = aligned with the
    /// "positive pole" (supports), negative = aligned with the
    /// "negative pole" (contradicts).
    pub score:   f32,
    /// Axis snapshot used for the projection (so the caller can verify
    /// freshness and reproducibility).
    pub axis_built_at: i64,
    pub axis_model:    String,
}

// ── Vector math helpers ────────────────────────────────────────────────

fn vec_sub(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b.iter()).map(|(x, y)| x - y).collect()
}

fn vec_add_in_place(acc: &mut [f32], b: &[f32]) {
    for (x, y) in acc.iter_mut().zip(b.iter()) { *x += *y; }
}

fn vec_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

fn vec_normalize(v: &[f32]) -> Vec<f32> {
    let n = vec_norm(v);
    if n == 0.0 { v.to_vec() } else { v.iter().map(|x| x / n).collect() }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

// ── Public surface ─────────────────────────────────────────────────────

/// Build the axis from `DEFAULT_ANCHOR_PAIRS` and cache it. Returns
/// the freshly-built axis. Idempotent: a concurrent caller will see
/// the same axis once this returns.
pub async fn rebuild_axis(model: Option<String>) -> Result<PolarityAxis, String> {
    let pairs = DEFAULT_ANCHOR_PAIRS;
    if pairs.is_empty() {
        return Err("no anchor pairs configured".into());
    }

    let mut sum: Option<Vec<f32>> = None;
    let mut model_used = String::from("(unknown)");

    for (pos, neg) in pairs.iter() {
        let (e_pos, m_pos) = embed_via_ollama_pub(pos, model.clone()).await
            .map_err(|e| format!("embed '{}' failed: {}", pos, e))?;
        let (e_neg, _)     = embed_via_ollama_pub(neg, model.clone()).await
            .map_err(|e| format!("embed '{}' failed: {}", neg, e))?;
        if e_pos.len() != e_neg.len() {
            return Err(format!(
                "anchor dim mismatch: '{}' = {} vs '{}' = {}",
                pos, e_pos.len(), neg, e_neg.len()
            ));
        }
        let diff = vec_sub(&e_pos, &e_neg);
        match sum.as_mut() {
            None      => sum = Some(diff),
            Some(acc) => vec_add_in_place(acc, &diff),
        }
        model_used = m_pos;
    }

    let sum = sum.expect("at least one pair was processed");
    let raw_norm = vec_norm(&sum);
    let axis = vec_normalize(&sum);

    let built = PolarityAxis {
        axis,
        model: model_used,
        n_pairs: pairs.len(),
        built_at: chrono::Utc::now().timestamp(),
        raw_norm,
    };

    let cell = cache_handle().await;
    let mut w = cell.write().await;
    *w = Some(built.clone());
    Ok(built)
}

/// Return the cached axis, building it on first access.
pub async fn current_axis(model: Option<String>) -> Result<PolarityAxis, String> {
    {
        let cell = cache_handle().await;
        let r = cell.read().await;
        if let Some(a) = r.as_ref() { return Ok(a.clone()); }
    }
    rebuild_axis(model).await
}

/// Project `text` onto the cached axis. Cheap once the axis is warm:
/// 1 embed + 1 dot product. The score is roughly bounded in [-1, +1];
/// values outside that range can happen because the input embedding
/// is not constrained to be unit-length the way the axis is.
pub async fn project_text(
    text: &str, model: Option<String>,
) -> Result<PolarityProjection, String> {
    let axis = current_axis(model.clone()).await?;
    let (e, _) = embed_via_ollama_pub(text, model).await
        .map_err(|e| format!("embed input failed: {}", e))?;
    if e.len() != axis.axis.len() {
        return Err(format!(
            "input dim {} != axis dim {} (model drift — rebuild axis?)",
            e.len(), axis.axis.len()
        ));
    }
    let score = dot(&e, &axis.axis);
    Ok(PolarityProjection {
        score,
        axis_built_at: axis.built_at,
        axis_model:    axis.model.clone(),
    })
}

// ── Tauri commands ─────────────────────────────────────────────────────

/// Compute polarity for a text. Mostly for the frontend to dogfood —
/// the long-term integration path is to have chip_memory.rs call
/// project_text() internally when logging new events with novel kinds.
#[tauri::command]
pub async fn memory_polarity(
    text: String, model: Option<String>,
) -> Result<PolarityProjection, String> {
    project_text(&text, model).await
}

/// Force a rebuild. Use after changing the embedding model or anchor
/// pairs. Returns the new axis snapshot.
#[tauri::command]
pub async fn memory_polarity_rebuild(
    model: Option<String>,
) -> Result<PolarityAxis, String> {
    rebuild_axis(model).await
}

/// Read-only snapshot of the cached axis for diagnostics. Does NOT
/// trigger a rebuild if missing — returns Option.
#[tauri::command]
pub async fn memory_polarity_axis() -> Result<Option<PolarityAxis>, String> {
    let cell = cache_handle().await;
    let r = cell.read().await;
    Ok(r.as_ref().cloned())
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_of_unit_vector_is_one() {
        let v = vec![0.0, 1.0, 0.0];
        assert!((vec_norm(&v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_zero_returns_zero() {
        let v = vec![0.0, 0.0, 0.0];
        let n = vec_normalize(&v);
        assert!(n.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn dot_orthogonal_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((dot(&a, &b)).abs() < 1e-6);
    }

    #[test]
    fn dot_aligned_is_one() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 0.0];
        assert!((dot(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn dot_opposite_is_minus_one() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((dot(&a, &b) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn axis_construction_from_synthetic_pairs() {
        // Manually build the axis the way rebuild_axis does, but with
        // synthetic embeddings so the test doesn't depend on Ollama.
        // Each pair contributes Δ = pos − neg pointing toward +x.
        let pairs: Vec<(Vec<f32>, Vec<f32>)> = vec![
            (vec![1.0, 0.0], vec![-1.0, 0.0]),
            (vec![0.9, 0.1], vec![-0.9, -0.1]),
            (vec![1.0, 0.0], vec![-1.0, 0.0]),
        ];
        let mut sum = vec![0.0, 0.0];
        for (p, n) in &pairs {
            let d = vec_sub(p, n);
            vec_add_in_place(&mut sum, &d);
        }
        let axis = vec_normalize(&sum);
        // The axis should point strongly along +x.
        assert!(axis[0] > 0.99);
        // A new positive-pole input projects to a high positive score.
        let pos_input = vec![1.0, 0.0];
        let neg_input = vec![-1.0, 0.0];
        assert!(dot(&pos_input, &axis) > 0.99);
        assert!(dot(&neg_input, &axis) < -0.99);
    }

    #[test]
    fn cancelling_pairs_yield_small_raw_norm() {
        // If anchor pairs disagree on direction, their sum cancels and
        // raw_norm is small — this is the diagnostic signal flagged in
        // PolarityAxis.raw_norm.
        let pairs: Vec<(Vec<f32>, Vec<f32>)> = vec![
            (vec![1.0, 0.0], vec![-1.0, 0.0]),    // → +x
            (vec![-1.0, 0.0], vec![1.0, 0.0]),    // → -x (intentional disagreement)
        ];
        let mut sum = vec![0.0, 0.0];
        for (p, n) in &pairs {
            let d = vec_sub(p, n);
            vec_add_in_place(&mut sum, &d);
        }
        assert!(vec_norm(&sum) < 1e-6);
    }
}
