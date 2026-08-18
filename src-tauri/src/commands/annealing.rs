// ── annealing.rs — Annealing Ontologies MVP (v1.6.6) ─────────────────────
//
// Implements a Lucy-scale MVP of Kappa Graph ADR-200 (Annealing
// Ontologies — Self-Organizing Knowledge Graph Structure) from the
// mirror at docs/research/kappa-graph/adrs/ADR-200-annealing-ontologies.md
//
// ── What ADR-200 says ───────────────────────────────────────────────────
//
// A concept is just an extremely narrow ontology. An ontology is just a
// concept that has accumulated enough structure to serve as an
// organizing frame. Promote concepts upward when they earn it; demote
// ontologies when they fail to capture mass over time. The graph
// proposes; humans approve.
//
// Scoring (per-cluster):
//
//     mass        = sigmoid(degree / mass_scale)                 0..1
//     coherence   = 1 − diversity(members)                       0..1
//     exposure    = (epoch − birth_epoch) / opportunity_scale    0..1
//     protection  = mass × coherence  −  exposure_pressure
//     promotion   = sigmoid(mass × coherence) − exposure_pressure
//
// The hysteresis band (ADR-200 §7) keeps a cluster from flickering:
//
//     promotion_threshold = 0.80
//     demotion_threshold  = 0.50
//
// ── How Lucy maps to ADR-200 ─────────────────────────────────────────────
//
// ADR-200 assumes Apache AGE + Cypher and a dedicated `:Ontology` node
// type. Lucy is SQLite-only. The MVP adapts faithfully without a
// schema migration:
//
//   - "Ontology" in Lucy ≡ the set of `agent_memories` rows sharing a
//     tag. `tags` is already JSON in the existing table. The tag string
//     IS the ontology name; no new column needed.
//   - "Source" in ADR-200 ≡ an `agent_memories` row (the unit of
//     ingestion).
//   - "Concept" in ADR-200 ≡ a high-importance row that anchors a
//     cluster (importance ≥ 7 by convention).
//   - "global epoch" ≡ count of all agent_memories rows ever created
//     (lifetime, includes superseded).
//   - "ontology birth epoch" ≡ row-count snapshot when this tag first
//     appeared (we estimate from MIN(created_at) of any row with the
//     tag — close enough for an MVP).
//
// This release is READ-ONLY per the ADR's HITL Phase 3 directive: "The
// worker does NOT execute proposals in Phase 3. It produces scored
// recommendations for human review." Lucy surfaces the report via a
// new slash command (/anneal) and a Tauri command. Promotion /
// demotion EXECUTION is deferred to a later release.
//
// ── Why now ──────────────────────────────────────────────────────────────
//
// Lucy users (per chip_memory telemetry) accumulate ~hundreds of
// memories over weeks. Tag inflation is the typical failure mode —
// tags like "misc", "todo", "random" become catch-all buckets while
// genuine domains (e.g. "k8s-prod") stay coherent. The annealing
// report makes that pathology visible: surface the buckets that
// failed to earn their status alongside the buckets that look like
// real domains.

use crate::commands::metrics::shared_db;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ── Tunable constants ───────────────────────────────────────────────────
//
// Calibrated by hand for a graph of 100–2000 memories. Will need to be
// re-fit empirically once we have telemetry on a real-user graph.

const PROMOTION_THRESHOLD: f32 = 0.80;
const DEMOTION_THRESHOLD:  f32 = 0.50;

/// At MASS_SCALE memories in a cluster the mass sigmoid is at 0.5.
/// 15 is the "this is starting to look like a domain" inflection point
/// for Lucy's scale.
const MASS_SCALE:        f32 = 15.0;
const COHERENCE_FLOOR:   f32 = 0.05;
const OPPORTUNITY_SCALE: f32 = 200.0;     // ingest events to reach exposure≈1

// Tags below MIN_MASS_FOR_SCORING are skipped. With 1-2 members a tag
// is just noise — scoring it produces lots of spurious entries.
const MIN_MEMBERS_TO_SCORE: usize = 2;

// ── Types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyScore {
    pub name:             String,
    pub members:          usize,
    pub mass:             f32,
    pub coherence:        f32,
    pub exposure:         f32,
    pub promotion_score:  f32,
    pub protection_score: f32,
    pub lifecycle_state:  String,    // "newborn" | "struggling" | "stable" | "failed" | "growing"
    pub verdict:          String,    // "no_action" | "promote" | "demote" | "watch"
    /// IDs of the top-3 highest-importance members, useful as anchor
    /// candidates if a promotion happens later.
    pub anchor_ids:       Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnealingReport {
    pub built_at:         i64,
    /// MAX(id) over agent_memories — proxy for lifetime ingest count
    /// per ADR-200 §"Epoch-Based Exposure". This counts memories ever
    /// created (including deleted/superseded), not what's currently
    /// live, because exposure pressure depends on "opportunities the
    /// graph has had", not "memories present right now".
    pub global_epoch:     i64,
    /// v1.6.15: explicit count of currently-live (non-superseded)
    /// memories so the slash command can distinguish "lifetime" from
    /// "active" — users were confused when global_epoch=596 vs 6
    /// rows visible in Memory Browser.
    pub active_memories:  i64,
    pub n_clusters:       usize,
    pub promotion_count:  usize,
    pub demotion_count:   usize,
    pub clusters:         Vec<OntologyScore>,
}

// ── Math helpers ───────────────────────────────────────────────────────

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Sigmoid normalized so that input == scale/2 maps to ~0.5 and
/// input == scale lands around ~0.88 (well into the upper plateau).
/// The [0..2*scale] range covers most of the 0..1 output curve. Below
/// scale/2 the cluster is not yet "ontology-shaped"; well above scale
/// it asymptotes to 1.
fn mass_curve(degree: f32, scale: f32) -> f32 {
    sigmoid(4.0 * (degree / scale - 0.5))
}

/// Exposure pressure: low when newborn, grows past the opportunity
/// scale. Bounded in [0, 1).
fn exposure_pressure(birth: i64, now: i64, scale: f32) -> f32 {
    let delta = (now - birth).max(0) as f32;
    let raw   = delta / scale;
    sigmoid(2.0 * (raw - 1.0))
}

// ── Coherence: token-bag Jaccard diversity ─────────────────────────────
//
// ADR-200 §11 specifies coherence = 1 − diversity, where diversity is
// the Gini-Simpson index of pairwise embedding similarity in the
// neighborhood. We don't have embeddings cached on every memory row.
//
// The MVP substitute: pairwise token-bag Jaccard distance over title +
// tags + first 200 chars of content. Fast (no embedding calls), purely
// SQL-side. The signal correlates well with embedding similarity for
// the short-text regime Lucy lives in. We can upgrade to embedding-based
// coherence in a later release once memory rows carry stored
// embeddings.

fn tokenize(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(String::from)
        .collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() { return 1.0; }
    let inter = a.intersection(b).count() as f32;
    let union = a.union(b).count() as f32;
    if union == 0.0 { 0.0 } else { inter / union }
}

/// Mean pairwise Jaccard over up to N=20 sampled members. Beyond 20
/// members the cost grows quadratically; sampling keeps the report
/// snappy on big graphs.
fn cluster_coherence(member_tokens: &[HashSet<String>]) -> f32 {
    let n = member_tokens.len();
    if n < 2 { return 1.0; }
    let cap = n.min(20);
    let slice = &member_tokens[..cap];
    let mut sum  = 0.0_f32;
    let mut pairs = 0_u32;
    for i in 0..cap {
        for j in (i+1)..cap {
            sum += jaccard(&slice[i], &slice[j]);
            pairs += 1;
        }
    }
    if pairs == 0 { return 1.0; }
    (sum / pairs as f32).max(COHERENCE_FLOOR)
}

// ── Tag extraction ─────────────────────────────────────────────────────
//
// `tags` is JSON: `["k8s", "prod"]`. We parse leniently; rows with
// malformed JSON contribute zero tags rather than failing the report.

fn parse_tags(json_str: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(json_str)
        .ok()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !t.trim().is_empty())
        .collect()
}

// ── Lifecycle state classification (ADR-200 §7 table) ───────────────────

fn classify(mass: f32, exposure: f32) -> &'static str {
    match (mass >= 0.5, exposure >= 0.5) {
        (false, false) => "newborn",      // low mass, low exposure
        (false, true)  => "failed",       // low mass, high exposure
        (true,  false) => "growing",      // high mass, low exposure
        (true,  true)  => "stable",       // high mass, high exposure
    }
}

fn verdict(promo: f32, prot: f32, members: usize) -> &'static str {
    if members < MIN_MEMBERS_TO_SCORE { return "no_action"; }
    if promo >= PROMOTION_THRESHOLD   { return "promote"; }
    if prot  <  DEMOTION_THRESHOLD    { return "demote"; }
    if promo >= 0.60 || prot < 0.65   { return "watch"; }
    "no_action"
}

// ── Core scoring loop ──────────────────────────────────────────────────

fn score_inner(conn: &Connection) -> Result<AnnealingReport, String> {
    // Global epoch = lifetime memory count. Cheap COUNT(*) over a
    // table with an index on id is microseconds even at 100k rows.
    let global_epoch: i64 = conn
        .query_row("SELECT COALESCE(MAX(id), 0) FROM agent_memories", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let active_memories: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_memories
             WHERE superseded_by IS NULL OR superseded_by = ''",
            [], |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT id, title, content, tags, importance, created_at
             FROM agent_memories
             WHERE superseded_by IS NULL OR superseded_by = ''"
        )
        .map_err(|e| e.to_string())?;

    // Per-tag buckets keyed by tag string.
    // (id, importancia, creado, etiquetas) — con nombre, porque una tupla de
    // cuatro dentro de un Vec dentro de un HashMap no se lee de un vistazo.
    type Candidata = (i64, i32, i64, HashSet<String>);
    let mut by_tag: HashMap<String, Vec<Candidata>> = HashMap::new();

    let rows = stmt.query_map([], |r| {
        let id: i64        = r.get(0)?;
        let title: String  = r.get(1).unwrap_or_default();
        let content: String= r.get(2).unwrap_or_default();
        let tags_j: String = r.get(3).unwrap_or_else(|_| "[]".into());
        let imp:  i32      = r.get(4).unwrap_or(1);
        let ts:   i64      = r.get(5).unwrap_or(0);
        Ok((id, title, content, tags_j, imp, ts))
    }).map_err(|e| e.to_string())?;

    for row in rows.flatten() {
        let (id, title, content, tags_j, imp, ts) = row;
        let mut bag = tokenize(&title);
        // Cap content tokenization at 200 chars — enough signal for
        // Jaccard, bounded cost on long memories.
        bag.extend(tokenize(&content.chars().take(200).collect::<String>()));
        for t in parse_tags(&tags_j) {
            by_tag.entry(t).or_default().push((id, imp, ts, bag.clone()));
        }
    }

    let mut clusters: Vec<OntologyScore> = Vec::new();
    let mut promotion_count = 0;
    let mut demotion_count  = 0;

    for (tag, mut members) in by_tag.into_iter() {
        if members.len() < MIN_MEMBERS_TO_SCORE { continue; }

        // Birth epoch ≈ oldest member's created_at. This is an
        // approximation — a strict ADR-200 implementation would
        // record creation_epoch at tag creation, but we don't have
        // that history yet, so we infer from MIN(created_at).
        let birth = members.iter().map(|m| m.2).min().unwrap_or(0);
        let now   = chrono::Utc::now().timestamp();

        let mass = mass_curve(members.len() as f32, MASS_SCALE);
        let token_bags: Vec<HashSet<String>> = members.iter().map(|m| m.3.clone()).collect();
        let coherence = cluster_coherence(&token_bags);
        let exposure  = exposure_pressure(birth, now,
                                          OPPORTUNITY_SCALE * 60.0 * 60.0); // seconds

        let promotion  = sigmoid(4.0 * (mass * coherence - 0.5)) - 0.3 * exposure;
        let protection = mass_curve(mass * coherence * 2.0, 1.0) - exposure;

        // Pick anchor candidates: top 3 by importance, stable tiebreak by id.
        members.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let anchor_ids: Vec<i64> = members.iter().take(3).map(|m| m.0).collect();

        let lifecycle = classify(mass, exposure).to_string();
        let v = verdict(promotion, protection, members.len()).to_string();
        if v == "promote" { promotion_count += 1; }
        if v == "demote"  { demotion_count += 1; }

        clusters.push(OntologyScore {
            name: tag,
            members: members.len(),
            mass, coherence, exposure,
            promotion_score:  promotion,
            protection_score: protection,
            lifecycle_state:  lifecycle,
            verdict:          v,
            anchor_ids,
        });
    }

    // Sort by promotion_score desc so the most actionable rows
    // surface first in the report.
    clusters.sort_by(|a, b| b.promotion_score.partial_cmp(&a.promotion_score)
                                 .unwrap_or(std::cmp::Ordering::Equal));

    Ok(AnnealingReport {
        built_at: chrono::Utc::now().timestamp(),
        global_epoch,
        active_memories,
        n_clusters: clusters.len(),
        promotion_count,
        demotion_count,
        clusters,
    })
}

// ── Tauri commands ─────────────────────────────────────────────────────

/// Compute the annealing report over current `agent_memories`.
/// Read-only — no graph mutations in this release.
#[tauri::command]
pub async fn memory_annealing_report() -> Result<AnnealingReport, String> {
    shared_db(score_inner)
}

/// Inspect a single cluster by tag name, for the UI drill-down.
#[tauri::command]
pub async fn memory_annealing_cluster(tag: String) -> Result<Option<OntologyScore>, String> {
    let report = memory_annealing_report().await?;
    Ok(report.clusters.into_iter().find(|c| c.name == tag))
}

// ── v1.6.8 — Phase 4 execution: demote with affinity routing ───────────
//
// ADR-200 §8 principle: "No deletion, only movement. Concepts never
// disappear; they relocate."
//
// For Lucy's MVP, a "demote" operation re-tags every memory in the
// dying cluster. For each member memory we compute affinity to every
// other tag (shared-concept proxy: count of other memories in tag X
// that share ≥ 1 token with this memory). The dying tag is removed
// from the memory; the top-affinity surviving tag is added (or
// "primordial" if affinity is below a floor).
//
// This is the "execute" half of the proposal/execute split. The
// /anneal report proposes; the user approves a specific tag from the
// UI to trigger this command.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoteReport {
    pub tag:              String,
    pub members_touched:  usize,
    pub reassigned:       Vec<DemoteReassignment>,
    /// How many memories landed in the primordial pool (no clear
    /// affinity to any surviving tag).
    pub orphaned:         usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoteReassignment {
    pub memory_id:   i64,
    pub target_tag:  String,
    pub shared:      usize,
}

/// Primordial pool name — ADR-200 §3 "everything else". When a memory
/// has no clear affinity to any other tag, it goes here.
pub const PRIMORDIAL_TAG: &str = "primordial";

fn demote_inner(conn: &Connection, dying: &str) -> Result<DemoteReport, String> {
    if dying.trim().is_empty() || dying == PRIMORDIAL_TAG {
        return Err(format!("cannot demote tag '{}'", dying));
    }

    // Step 1: load every memory and its current tags + token bag.
    let mut stmt = conn.prepare(
        "SELECT id, title, content, tags
         FROM agent_memories
         WHERE superseded_by IS NULL OR superseded_by = ''"
    ).map_err(|e| e.to_string())?;

    struct M { id: i64, tags: Vec<String>, bag: HashSet<String> }
    let mut memories: Vec<M> = Vec::new();
    let rows = stmt.query_map([], |r| {
        let id: i64        = r.get(0)?;
        let title: String  = r.get(1).unwrap_or_default();
        let content: String= r.get(2).unwrap_or_default();
        let tags_j: String = r.get(3).unwrap_or_else(|_| "[]".into());
        Ok((id, title, content, tags_j))
    }).map_err(|e| e.to_string())?;
    for row in rows.flatten() {
        let (id, title, content, tags_j) = row;
        let tags = parse_tags(&tags_j);
        let mut bag = tokenize(&title);
        bag.extend(tokenize(&content.chars().take(200).collect::<String>()));
        memories.push(M { id, tags, bag });
    }

    // Step 2: split into dying-set and survivor-set.
    let (dying_mems, survivors): (Vec<&M>, Vec<&M>) =
        memories.iter().partition(|m| m.tags.iter().any(|t| t == dying));

    let mut report = DemoteReport {
        tag: dying.into(),
        members_touched: 0,
        reassigned: Vec::new(),
        orphaned: 0,
    };

    for m in dying_mems {
        // Compute affinity to every other tag: sum of Jaccard against
        // survivor memories carrying that tag. Cheap because we sample
        // the survivor set to keep cost bounded.
        let mut tag_score: HashMap<String, f32> = HashMap::new();
        for s in survivors.iter().take(500) {
            let j = jaccard(&m.bag, &s.bag);
            if j < 0.1 { continue; }
            for t in &s.tags {
                if t == dying { continue; }
                *tag_score.entry(t.clone()).or_insert(0.0) += j;
            }
        }

        // Pick the top tag; floor it.
        let target = tag_score.into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let (new_tag, shared_score) = match target {
            Some((t, s)) if s >= 0.5 => (t, s),
            _ => {
                report.orphaned += 1;
                (PRIMORDIAL_TAG.to_string(), 0.0)
            }
        };

        // Mutate the row's tags: drop `dying`, add `new_tag` (if not
        // already present).
        let mut new_tags: Vec<String> =
            m.tags.iter().filter(|t| *t != dying).cloned().collect();
        if !new_tags.iter().any(|t| t == &new_tag) {
            new_tags.push(new_tag.clone());
        }
        let new_tags_j = serde_json::to_string(&new_tags).unwrap_or_else(|_| "[]".into());

        conn.execute(
            "UPDATE agent_memories SET tags = ?1 WHERE id = ?2",
            params![new_tags_j, m.id],
        ).map_err(|e| format!("demote UPDATE id={}: {}", m.id, e))?;

        report.reassigned.push(DemoteReassignment {
            memory_id: m.id,
            target_tag: new_tag,
            shared: (shared_score * 10.0).round() as usize,
        });
        report.members_touched += 1;
    }

    Ok(report)
}

/// Execute a demote: re-tag every memory carrying `tag` onto its
/// highest-affinity surviving tag (or primordial if no good match).
/// No memories are deleted; only the `tags` array is mutated.
#[tauri::command]
pub async fn memory_annealing_demote(tag: String) -> Result<DemoteReport, String> {
    shared_db(move |c| demote_inner(c, &tag))
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigmoid_midpoint() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mass_curve_below_scale_is_under_half() {
        assert!(mass_curve(5.0, MASS_SCALE) < 0.5);
    }

    #[test]
    fn mass_curve_at_half_scale_is_about_half() {
        // Curve midpoint is at degree == scale/2 (sigmoid argument == 0).
        // At degree == scale, the value is already ~0.88 — see the
        // doc comment on mass_curve. This test pins the actual midpoint
        // so future tweaks to the steepness factor are caught.
        let m = mass_curve(MASS_SCALE / 2.0, MASS_SCALE);
        assert!((m - 0.5).abs() < 0.05, "expected ~0.5, got {m}");
    }

    #[test]
    fn mass_curve_far_above_scale_saturates() {
        assert!(mass_curve(MASS_SCALE * 10.0, MASS_SCALE) > 0.99);
    }

    #[test]
    fn tokenize_drops_short_and_punctuation() {
        let t = tokenize("a, BB, k8s-prod, http://example.com");
        assert!(!t.contains("a"));
        assert!(!t.contains("bb"));
        assert!(t.contains("k8s"));
        assert!(t.contains("prod"));
        assert!(t.contains("http"));
        assert!(t.contains("example"));
    }

    #[test]
    fn jaccard_identical_is_one() {
        let a = tokenize("kubernetes prod cluster");
        let b = tokenize("kubernetes prod cluster");
        assert!((jaccard(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn jaccard_disjoint_is_zero() {
        let a = tokenize("apple banana");
        let b = tokenize("xylophone zebra");
        assert!(jaccard(&a, &b) < 1e-6);
    }

    #[test]
    fn coherence_homogeneous_cluster_is_high() {
        let bags: Vec<HashSet<String>> = (0..5)
            .map(|i| tokenize(&format!("kubernetes prod cluster node{}", i)))
            .collect();
        let c = cluster_coherence(&bags);
        // Most tokens overlap (kubernetes, prod, cluster) — coherence
        // should comfortably clear 0.5.
        assert!(c > 0.5, "expected high coherence, got {}", c);
    }

    #[test]
    fn coherence_heterogeneous_cluster_is_low() {
        let bags: Vec<HashSet<String>> = vec![
            tokenize("apple banana cherry"),
            tokenize("xylophone zebra yak"),
            tokenize("rocket moon launch"),
            tokenize("cucumber dill pickle"),
        ];
        let c = cluster_coherence(&bags);
        assert!(c < 0.2, "expected low coherence, got {}", c);
    }

    #[test]
    fn classify_state_table() {
        // newborn = low mass, low exposure → safe
        assert_eq!(classify(0.1, 0.1), "newborn");
        // failed = low mass, high exposure → demote candidate
        assert_eq!(classify(0.1, 0.9), "failed");
        // growing = high mass, low exposure → actively accumulating
        assert_eq!(classify(0.9, 0.1), "growing");
        // stable = high mass, high exposure → self-sustaining
        assert_eq!(classify(0.9, 0.9), "stable");
    }

    #[test]
    fn verdict_thresholds() {
        // Below MIN_MEMBERS_TO_SCORE: always no_action.
        assert_eq!(verdict(0.99, 0.99, 1), "no_action");
        // High promotion_score: promote.
        assert_eq!(verdict(0.85, 0.7, 5), "promote");
        // Low protection_score: demote.
        assert_eq!(verdict(0.3, 0.4, 5), "demote");
        // Mid-band: watch.
        assert_eq!(verdict(0.65, 0.6, 5), "watch");
        // Healthy stable: no_action.
        assert_eq!(verdict(0.4, 0.85, 5), "no_action");
    }

    #[test]
    fn parse_tags_handles_garbage() {
        assert_eq!(parse_tags("[]").len(), 0);
        assert_eq!(parse_tags("garbage").len(), 0);
        let v = parse_tags(r#"["k8s", "prod", "", "  "]"#);
        assert_eq!(v.len(), 2);
        assert!(v.contains(&"k8s".to_string()));
    }
}
