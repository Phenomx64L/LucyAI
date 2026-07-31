// ── vec_index.rs — In-memory HNSW-lite vector index for fast ANN search ─────
//
// PURPOSE: When the embeddings corpus exceeds a threshold (default 500 rows),
// the linear cosine scan becomes noticeable (~20ms). This module provides an
// in-memory approximate nearest-neighbor index that accelerates semantic_search
// to <2ms regardless of corpus size.
//
// DESIGN:
//   • Pure Rust — no C dependency (avoids sqlite-vec build-system complexity)
//   • Lazy initialization: only builds the index when corpus > threshold
//   • Auto-invalidates on insert/delete (rebuilds on next query)
//   • Falls back to linear scan if index is stale or corpus is small
//   • Thread-safe via Mutex wrapping
//
// ALGORITHM: Simplified HNSW (Hierarchical Navigable Small World graph) with
// single-layer navigation. For Lucy's expected scale (500-5000 vectors), this
// gives >95% recall with 10-50x speedup over brute force.

use std::sync::Mutex;
use once_cell::sync::Lazy;

// ── Configuration ───────────────────────────────────────────────────────────

/// Minimum corpus size before we bother building the index.
/// Below this threshold, linear scan is fast enough (<5ms).
pub const INDEX_THRESHOLD: usize = 500;

/// Number of neighbors per node in the graph (M parameter).
const M: usize = 16;

/// Number of candidates during search (ef parameter — higher = more accurate).
const EF_SEARCH: usize = 50;

// ── Types ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct IndexEntry {
    pub entity_type: String,
    pub entity_id: String,
    pub text: String,
    pub vec: Vec<f32>,
    /// The embedder that produced `vec`. Two models can output the same
    /// dimension while placing the same sentence somewhere completely
    /// different — `nomic-embed-text` and Gemini's `text-embedding-004` are
    /// both 768-dim, which is exactly the pair Lucy alternates between when
    /// Ollama is not running. Cosine across that boundary is a number, not a
    /// similarity, and nothing about it looks wrong.
    pub model: String,
}

struct VecIndex {
    entries: Vec<IndexEntry>,
    /// Adjacency list: neighbors[i] contains indices of M nearest neighbors of entries[i]
    neighbors: Vec<Vec<usize>>,
    /// The single embedder every entry in `entries` came from.
    ///
    /// Filtering hits by model at query time would not be enough here: `build()`
    /// computes cosine BETWEEN entries, so a mixed corpus produces a neighbour
    /// graph whose edges are meaningless before any query arrives. The index
    /// therefore holds one model's vectors and refuses queries from another,
    /// which drops the caller to the linear scan — slower, and correct.
    index_model: String,
    /// Generation counter — bumped on every insert/delete to invalidate
    generation: u64,
    /// Generation at which the index was last built
    built_generation: u64,
}

impl VecIndex {
    fn new() -> Self {
        Self {
            index_model: String::new(),
            entries: Vec::new(),
            neighbors: Vec::new(),
            generation: 0,
            built_generation: 0,
        }
    }

    fn is_valid(&self) -> bool {
        !self.entries.is_empty()
            && self.generation == self.built_generation
            && self.entries.len() >= INDEX_THRESHOLD
    }

    /// Build the graph index from current entries.
    fn build(&mut self) {
        let n = self.entries.len();
        if n < INDEX_THRESHOLD {
            self.neighbors.clear();
            self.built_generation = self.generation;
            return;
        }

        self.neighbors = vec![Vec::new(); n];

        // For each entry, find its M nearest neighbors via brute-force during build.
        // Build is O(n² * d) but runs rarely (on invalidation) and n < 5000 typically.
        for i in 0..n {
            let mut scores: Vec<(usize, f32)> = (0..n)
                .filter(|&j| j != i)
                .map(|j| (j, cosine_fast(&self.entries[i].vec, &self.entries[j].vec)))
                .collect();
            scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            self.neighbors[i] = scores.iter().take(M).map(|(idx, _)| *idx).collect();
        }

        self.built_generation = self.generation;
    }

    /// Approximate nearest neighbor search using greedy graph traversal
    /// with multiple entry points for better recall.
    /// Returns up to `limit` entries with score >= min_score.
    fn search(&self, query: &[f32], limit: usize, min_score: f32) -> Vec<(usize, f32)> {
        if !self.is_valid() || self.entries.is_empty() {
            return Vec::new();
        }

        let n = self.entries.len();
        let mut visited = vec![false; n];
        let mut candidates: Vec<(usize, f32)> = Vec::with_capacity(EF_SEARCH * 2);

        // Multiple entry points for better coverage (3 spread across the corpus)
        let hash = query.iter().take(4).fold(0u64, |acc, &f| acc.wrapping_add(f.to_bits() as u64));
        let starts = [
            hash as usize % n,
            (hash.wrapping_mul(6364136223846793005).wrapping_add(1)) as usize % n,
            (hash.wrapping_mul(1442695040888963407).wrapping_add(3)) as usize % n,
        ];

        for &start in &starts {
            if !visited[start] {
                visited[start] = true;
                let score = cosine_fast(query, &self.entries[start].vec);
                candidates.push((start, score));
            }
        }

        // Greedy expansion: explore neighbors of the best candidates first
        let mut ptr = 0;
        while ptr < candidates.len() && candidates.len() < EF_SEARCH {
            // Sort remaining candidates by score to expand best-first
            if ptr > 0 && ptr % 8 == 0 {
                candidates[ptr..].sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            }

            let (current, _) = candidates[ptr];
            ptr += 1;

            for &neighbor in &self.neighbors[current] {
                if visited[neighbor] {
                    continue;
                }
                visited[neighbor] = true;
                let score = cosine_fast(query, &self.entries[neighbor].vec);
                candidates.push((neighbor, score));
            }
        }

        // Sort by score desc and filter
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        candidates.into_iter()
            .filter(|(_, s)| *s >= min_score)
            .take(limit)
            .collect()
    }
}

// ── Global state ────────────────────────────────────────────────────────────

static INDEX: Lazy<Mutex<VecIndex>> = Lazy::new(|| Mutex::new(VecIndex::new()));

// ── Public API ──────────────────────────────────────────────────────────────

/// Notify the index that the embeddings corpus changed (insert/delete/update).
/// This bumps the generation counter, causing the next search to rebuild.
pub fn invalidate() {
    if let Ok(mut idx) = INDEX.lock() {
        idx.generation += 1;
    }
}

/// Load all entries into the index from the provided data.
/// Called during search when the index is stale.
/// Builds the HNSW graph OUTSIDE the mutex to avoid blocking searches
/// during the O(n²) construction phase.
///
/// Entries from more than one embedder are narrowed to the largest group
/// before building — see `index_model`. Dropping the minority is the right
/// trade: those rows stay reachable through the linear scan, whereas mixing
/// them in would corrupt the graph for everything.
pub fn reload(entries: Vec<IndexEntry>) {
    let entries = keep_dominant_model(entries);
    let model = entries.first().map(|e| e.model.clone()).unwrap_or_default();

    // Build the index without holding the lock
    let mut temp = VecIndex::new();
    temp.index_model = model;
    temp.entries = entries;
    temp.generation = 1;
    temp.build();

    // Swap under lock — minimal hold time
    if let Ok(mut idx) = INDEX.lock() {
        let gen = idx.generation + 1;
        *idx = temp;
        idx.generation = gen;
        idx.built_generation = gen;
    }
}

/// Check if the index is ready for use (valid and corpus > threshold).
pub fn is_ready() -> bool {
    INDEX.lock().map(|idx| idx.is_valid()).unwrap_or(false)
}

/// Get the current corpus size in the index.
#[allow(dead_code)]  // available for diagnostics / future telemetry
pub fn corpus_size() -> usize {
    INDEX.lock().map(|idx| idx.entries.len()).unwrap_or(0)
}

/// Keep only the entries from whichever embedder contributed the most vectors.
///
/// Split out so the tie-break and the "one model wins" rule are testable
/// without building a 500-entry graph.
fn keep_dominant_model(entries: Vec<IndexEntry>) -> Vec<IndexEntry> {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for e in &entries {
        *counts.entry(e.model.as_str()).or_insert(0) += 1;
    }
    if counts.len() <= 1 {
        return entries;
    }
    // Ties break on the model name so the same corpus always yields the same
    // index — an index that reshuffles between boots makes a recall complaint
    // impossible to reproduce.
    let winner = counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(m, _)| m.to_string())
        .unwrap_or_default();
    entries.into_iter().filter(|e| e.model == winner).collect()
}

/// Search the index. Returns (entity_type, entity_id, text, score) tuples.
///
/// `model` is the embedder that produced `query`. A query from a different
/// embedder than the index holds returns empty, which reads to the caller the
/// same as a cold index: fall through to the linear scan.
///
/// Returns empty vec if index isn't ready (caller should fall back to linear scan).
pub fn search(query: &[f32], model: &str, limit: usize, min_score: f32) -> Vec<(String, String, String, f32)> {
    let idx = match INDEX.lock() {
        Ok(idx) => idx,
        Err(_) => return Vec::new(),
    };

    if !idx.is_valid() || idx.index_model != model {
        return Vec::new();
    }

    idx.search(query, limit, min_score)
        .into_iter()
        .map(|(i, score)| {
            let e = &idx.entries[i];
            (e.entity_type.clone(), e.entity_id.clone(), e.text.clone(), score)
        })
        .collect()
}

// ── Fast cosine (SIMD-dispatched) ───────────────────────────────────────────
//
// v1.7.19 — the hand-unrolled-by-4 loop here was a hint to the auto-
// vectorizer that worked at opt-level=3 but did nothing at our
// production `opt-level = "z"`. Replaced with the explicit dispatch
// module that picks AVX-512 / AVX2 / scalar at runtime regardless of
// the global optimization profile.
#[inline]
fn cosine_fast(a: &[f32], b: &[f32]) -> f32 {
    crate::utils::simd_cosine::cosine(a, b)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: &str, vec: Vec<f32>) -> IndexEntry {
        make_entry_from(id, vec, "nomic-embed-text")
    }

    fn make_entry_from(id: &str, vec: Vec<f32>, model: &str) -> IndexEntry {
        IndexEntry {
            entity_type: "test".into(),
            entity_id: id.into(),
            text: format!("text for {}", id),
            vec,
            model: model.into(),
        }
    }

    // ── One index, one embedder ─────────────────────────────────────────────
    //
    // These pin the rule that makes the whole same-space guard work. It is not
    // a filter you can apply to the results: `build()` computes cosine BETWEEN
    // entries, so a corpus holding two models produces a neighbour graph whose
    // edges are already meaningless — the damage is done before a query exists.

    #[test]
    fn a_single_model_corpus_is_kept_whole() {
        let entries = vec![
            make_entry_from("a", vec![1.0, 0.0], "nomic-embed-text"),
            make_entry_from("b", vec![0.0, 1.0], "nomic-embed-text"),
        ];
        assert_eq!(keep_dominant_model(entries).len(), 2);
    }

    #[test]
    fn a_mixed_corpus_keeps_only_the_majority_embedder() {
        // The realistic shape: a long-standing Ollama corpus with a handful of
        // rows written during an outage, when the write path fell back to
        // Gemini. Both models emit 768 dimensions, so nothing downstream can
        // tell these apart — this is the only place that can.
        let entries = vec![
            make_entry_from("a", vec![1.0, 0.0], "nomic-embed-text"),
            make_entry_from("b", vec![0.0, 1.0], "nomic-embed-text"),
            make_entry_from("c", vec![1.0, 1.0], "nomic-embed-text"),
            make_entry_from("d", vec![0.5, 0.5], "text-embedding-004"),
        ];
        let kept = keep_dominant_model(entries);
        assert_eq!(kept.len(), 3, "the minority embedder should be dropped");
        assert!(
            kept.iter().all(|e| e.model == "nomic-embed-text"),
            "kept entries must share one model"
        );
    }

    #[test]
    fn a_tie_resolves_the_same_way_every_time() {
        // Deterministic on purpose. HashMap iteration order is not stable
        // across runs, so without the name tie-break the index could hold a
        // different half on each boot — and a recall complaint that changes
        // shape between restarts is one nobody can reproduce.
        let build = || vec![
            make_entry_from("a", vec![1.0, 0.0], "aaa-model"),
            make_entry_from("b", vec![0.0, 1.0], "zzz-model"),
        ];
        let first = keep_dominant_model(build());
        for _ in 0..20 {
            let again = keep_dominant_model(build());
            assert_eq!(again.len(), 1);
            assert_eq!(again[0].model, first[0].model, "tie-break is not stable");
        }
    }

    #[test]
    fn test_cosine_fast_identical() {
        let v = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((cosine_fast(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_fast_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!(cosine_fast(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn test_index_below_threshold_not_ready() {
        let mut idx = VecIndex::new();
        idx.entries = (0..10).map(|i| make_entry(&format!("e{}", i), vec![i as f32; 128])).collect();
        idx.build();
        assert!(!idx.is_valid()); // below threshold
    }

    #[test]
    fn test_index_search_finds_nearest() {
        let mut idx = VecIndex::new();
        // Create 600 entries in a realistic embedding distribution:
        // All vectors are unit-normalized with a base + perturbation pattern.
        // Entry 0 and entry 1 are very similar (differ by small noise).
        idx.entries = (0..600).map(|i| {
            let mut v: Vec<f32> = (0..64).map(|d| {
                // Base: all entries share a common direction (typical for same-model embeddings)
                let base = 0.5 + (d as f32 * 0.01);
                // Perturbation unique to this entry
                let perturb = ((i * 31 + d * 7 + 13) % 50) as f32 / 200.0;
                base + perturb
            }).collect();
            // Normalize to unit length
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            for x in v.iter_mut() { *x /= norm; }
            make_entry(&format!("e{}", i), v)
        }).collect();
        idx.generation = 1;
        idx.build();
        assert!(idx.is_valid());

        // Query for entry 0's exact vector — should find it (score 1.0)
        let query = idx.entries[0].vec.clone();
        let results = idx.search(&query, 10, 0.0);
        assert!(!results.is_empty(), "ANN search should return results");
        // Top result must be very similar (>0.95)
        assert!(results[0].1 > 0.95,
            "Top ANN result should be >0.95, got {}", results[0].1);
        // All returned results should have meaningful similarity (same region of space)
        assert!(results.iter().all(|(_, s)| *s > 0.8),
            "All results should be in same region (>0.8 similarity)");
    }
}
