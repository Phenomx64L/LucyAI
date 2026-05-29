// ── chip_memory.rs — Layer 3 of the smart-chip stack (v1.4.2) ──────────────
//
// Heuristic chips (Layer 2) fire from a fixed rule library. LLM chips
// (Layer 1) come from a fresh Gemini/Anthropic call each turn. This
// module adds the missing third leg: **chips learned from THIS user's
// own click history**.
//
// How it works:
//
//   1. Every time the user clicks (or dismisses) a chip in
//      PredictiveChipStrip, the frontend calls `log_chip_event` with the
//      chip text + a context signature {domains, tool_labels, had_error,
//      lang}. We write one row per event into chip_click_log.
//
//   2. When chips are computed for the current turn, the frontend ALSO
//      calls `suggest_memory_chips` with the CURRENT signature. We
//      retrieve past rows whose signature overlaps the current one and
//      score them by:
//          score = Σ(click) · decay(age) - Σ(dismiss) · 0.6 · decay(age)
//      where decay = exp(-age_days / 30). 30-day half-life because
//      sysadmin contexts shift fast (today's hot project ≠ last quarter's).
//
//   3. Each surviving (label, text) gets returned as a SmartChip with
//      `source: 'memory'`. The frontend's merge logic places ◊-badged
//      chips alongside ⚡ heuristic and ✦ LLM ones.
//
// Why this beats vanilla "engagement scoring" (which already exists in
// predictive-chips.ts): engagement scoring only re-RANKS chips that the
// rule library would have proposed anyway. Layer 3 can surface a chip
// the rule library does NOT propose for this turn, because it appeared
// in similar contexts before. Concrete example: user often clicks
// "snapshot before patch" in sysadmin+no-error turns. Rule library
// only fires "snapshot" if CPU/RAM keywords appear. Memory layer
// surfaces it whenever sysadmin+no-error matches — even for "I'm
// about to install something" prompts that the rule misses.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::commands::metrics::shared_db;
use crate::commands::smart_chips::SmartChip;

// ── Public command payloads ────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ChipEventInput {
    pub label:       String,
    pub text:        String,
    pub intent:      Option<String>,
    pub domains:     Vec<String>,      // e.g. ["sysadmin", "perf"]
    pub tool_labels: Vec<String>,
    pub had_error:   bool,
    pub lang:        Option<String>,
    pub event_kind:  String,            // "click" or "dismiss"
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChipSignature {
    pub domains:     Vec<String>,
    pub tool_labels: Vec<String>,
    pub had_error:   bool,
    pub lang:        Option<String>,
}

// ── Tauri commands ─────────────────────────────────────────────────────

/// Append one click/dismiss event to the log. Fire-and-forget from the
/// frontend; we never block the UI. Returns the new row's id.
#[tauri::command]
pub async fn log_chip_event(event: ChipEventInput) -> Result<String, String> {
    let label = event.label.trim().to_string();
    let text  = event.text.trim().to_string();
    if label.is_empty() || text.is_empty() {
        return Err("log_chip_event: label and text are required".into());
    }
    let intent     = event.intent.unwrap_or_else(|| "other".into());
    let lang       = event.lang.unwrap_or_else(|| "es-MX".into());
    let kind       = normalize_event_kind(&event.event_kind);
    let domains_j  = serde_json::to_string(&event.domains).unwrap_or("[]".into());
    let tools_j    = serde_json::to_string(&event.tool_labels).unwrap_or("[]".into());
    let had_err: i64 = if event.had_error { 1 } else { 0 };
    let id = format!("chip-{}-{}",
        chrono::Local::now().timestamp_millis(),
        rand_suffix(),
    );

    let row_id = id.clone();
    shared_db(move |conn| {
        conn.execute(
            "INSERT INTO chip_click_log
             (id, label, text, intent, domains, tool_labels, had_error, lang, event_kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![row_id, label, text, intent, domains_j, tools_j, had_err, lang, kind],
        ).map_err(|e| format!("log_chip_event insert: {}", e))?;
        Ok(())
    })?;
    Ok(id)
}

/// Look at the user's chip-click history and propose up to N chips that
/// were clicked in contexts similar to the current one. Returns at most
/// 2 chips so the strip has room for heuristic + LLM picks too.
#[tauri::command]
pub async fn suggest_memory_chips(
    sig: ChipSignature,
    limit: Option<i64>,
) -> Result<Vec<SmartChip>, String> {
    let limit = limit.unwrap_or(2).clamp(1, 5);
    let cur_lang = sig.lang.unwrap_or_else(|| "es-MX".into());
    let cur_had_err: i64 = if sig.had_error { 1 } else { 0 };

    // Pull recent events from the same lang and same had_error state.
    // Filtering in SQL keeps the in-memory working set small even after
    // years of logs. Domain overlap is checked in Rust because SQLite
    // can't easily do JSON intersection without json1 extension calls.
    let rows: Vec<EventRow> = shared_db(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT label, text, intent, domains, tool_labels, event_kind, occurred_at
               FROM chip_click_log
              WHERE lang = ?1
                AND had_error = ?2
                AND occurred_at >= strftime('%s','now') - 60 * 24 * 60 * 60  -- 60 days
              ORDER BY occurred_at DESC
              LIMIT 800"
        ).map_err(|e| format!("suggest_memory_chips prepare: {}", e))?;
        let iter = stmt.query_map(params![cur_lang, cur_had_err], |r| {
            Ok(EventRow {
                label:       r.get::<_, String>(0)?,
                text:        r.get::<_, String>(1)?,
                intent:      r.get::<_, String>(2)?,
                domains_json: r.get::<_, String>(3)?,
                tools_json:  r.get::<_, String>(4)?,
                event_kind:  r.get::<_, String>(5)?,
                occurred_at: r.get::<_, i64>(6)?,
            })
        }).map_err(|e| format!("suggest_memory_chips query: {}", e))?;
        let mut out = Vec::new();
        for r in iter { if let Ok(row) = r { out.push(row); } }
        Ok::<_, String>(out)
    })?;

    let cur_domains: HashSet<String> = sig.domains.into_iter().collect();
    let cur_tools:   HashSet<String> = sig.tool_labels.into_iter().collect();
    let now = chrono::Local::now().timestamp();

    let aggregates = score_candidates(&rows, &cur_domains, &cur_tools, now);
    Ok(aggregates_to_chips(aggregates, limit as usize))
}

fn aggregates_to_chips(agg: Vec<Aggregate>, limit: usize) -> Vec<SmartChip> {
    agg.into_iter().take(limit).map(|a| SmartChip {
        label:    a.label,
        text:     a.text,
        intent:   a.intent,
        rationale: format!("Sugerencia aprendida (score {:.1})", a.score),
    }).collect()
}

// ── Internals ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct EventRow {
    label:       String,
    text:        String,
    intent:      String,
    domains_json: String,
    tools_json:  String,
    event_kind:  String,
    occurred_at: i64,
}

#[derive(Debug, Clone)]
struct Aggregate {
    label:       String,
    text:        String,
    intent:      String,
    score:       f64,
}

/// Group events by (label, text) and compute a recency-decayed click-minus-
/// dismiss score. Filters out candidates whose context overlap with the
/// current signature is zero (no shared domain AND no shared tool label).
/// Returns internal Aggregates so tests can assert on raw scores; the
/// public command wraps these in SmartChip via aggregates_to_chips.
fn score_candidates(
    rows: &[EventRow],
    cur_domains: &HashSet<String>,
    cur_tools:   &HashSet<String>,
    now_ts:      i64,
) -> Vec<Aggregate> {
    let mut by_key: HashMap<String, Aggregate> = HashMap::new();

    for row in rows {
        let row_domains: HashSet<String> = parse_json_array(&row.domains_json);
        let row_tools:   HashSet<String> = parse_json_array(&row.tools_json);

        // Context overlap gate: at least one shared domain OR one shared tool.
        // Pure-token overlap means "the situation rhymed with this past one".
        let domain_overlap = row_domains.intersection(cur_domains).count();
        let tool_overlap   = row_tools.intersection(cur_tools).count();
        if domain_overlap == 0 && tool_overlap == 0 {
            continue;
        }

        // Overlap bonus: more shared features → higher confidence.
        let overlap_bonus = 1.0
            + (domain_overlap as f64) * 0.15
            + (tool_overlap as f64) * 0.10;

        let age_days = ((now_ts - row.occurred_at).max(0) as f64) / 86_400.0;
        let decay = (-age_days / 30.0).exp();   // 30-day half-life-ish
        let sign  = if row.event_kind == "dismiss" { -0.6 } else { 1.0 };
        let weight = sign * decay * overlap_bonus;

        let key = format!("{}|||{}", row.label.to_lowercase(), row.text.to_lowercase());
        let entry = by_key.entry(key).or_insert_with(|| Aggregate {
            label:  row.label.clone(),
            text:   row.text.clone(),
            intent: row.intent.clone(),
            score:  0.0,
        });
        entry.score += weight;
    }

    // Drop candidates whose net score is non-positive — dismissed more
    // than clicked, or zero signal.
    let mut sorted: Vec<Aggregate> = by_key.into_values()
        .filter(|a| a.score > 0.5)  // small floor avoids flickery one-off matches
        .collect();
    sorted.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    sorted
}

fn parse_json_array(s: &str) -> HashSet<String> {
    serde_json::from_str::<Vec<String>>(s)
        .unwrap_or_default()
        .into_iter()
        .map(|x| x.to_lowercase())
        .collect()
}

fn normalize_event_kind(s: &str) -> String {
    match s.trim().to_lowercase().as_str() {
        "dismiss" | "dismissed" | "x"  => "dismiss".into(),
        _                              => "click".into(),
    }
}

fn rand_suffix() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let n: u32 = rng.gen_range(100_000..999_999);
    n.to_string()
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_row(label: &str, kind: &str, domains: &[&str], tools: &[&str], age_secs: i64) -> EventRow {
        EventRow {
            label: label.into(),
            text:  format!("text for {}", label),
            intent: "other".into(),
            domains_json: serde_json::to_string(domains).unwrap(),
            tools_json:   serde_json::to_string(tools).unwrap(),
            event_kind:   kind.into(),
            occurred_at:  chrono::Local::now().timestamp() - age_secs,
        }
    }

    #[test]
    fn skips_rows_without_overlap() {
        let now = chrono::Local::now().timestamp();
        let rows = vec![
            mk_row("Snap",    "click", &["sysadmin"], &["readfile"], 60),
            mk_row("Render",  "click", &["image"],    &[],            60),
        ];
        let cur_domains: HashSet<_> = ["sysadmin".to_string()].into_iter().collect();
        let cur_tools:   HashSet<_> = HashSet::new();
        let out = score_candidates(&rows, &cur_domains, &cur_tools, now);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "Snap");
    }

    #[test]
    fn dismiss_subtracts_from_click_for_same_label() {
        let now = chrono::Local::now().timestamp();
        let rows = vec![
            mk_row("Snap", "click",   &["sysadmin"], &[], 60),
            mk_row("Snap", "click",   &["sysadmin"], &[], 60),
            mk_row("Snap", "dismiss", &["sysadmin"], &[], 60),
        ];
        let cur_domains: HashSet<_> = ["sysadmin".to_string()].into_iter().collect();
        let out = score_candidates(&rows, &cur_domains, &HashSet::new(), now);
        assert_eq!(out.len(), 1);
        // 2 clicks - (1 * 0.6) = 1.4, before recency/overlap factors. Both > 0.5 threshold.
        assert!(out[0].score > 1.0);
    }

    #[test]
    fn many_dismisses_filter_out_chip() {
        let now = chrono::Local::now().timestamp();
        let mut rows = vec![mk_row("X", "click", &["sysadmin"], &[], 60)];
        for _ in 0..5 { rows.push(mk_row("X", "dismiss", &["sysadmin"], &[], 60)); }
        let cur_domains: HashSet<_> = ["sysadmin".to_string()].into_iter().collect();
        let out = score_candidates(&rows, &cur_domains, &HashSet::new(), now);
        // Net score = 1 - (5 * 0.6) = -2 → filtered.
        assert_eq!(out.len(), 0);
    }

    #[test]
    fn recent_clicks_outrank_older_ones() {
        let now = chrono::Local::now().timestamp();
        let one_day = 86_400;
        // Old = 20 days back. Decay = e^(-20/30) ≈ 0.51 (just above the 0.5 floor).
        // Recent = 1 hour. Decay ≈ 1.0.
        let rows = vec![
            mk_row("Recent", "click", &["sysadmin"], &[], 3600),
            mk_row("Old",    "click", &["sysadmin"], &[], 20 * one_day),
        ];
        let cur_domains: HashSet<_> = ["sysadmin".to_string()].into_iter().collect();
        let out = score_candidates(&rows, &cur_domains, &HashSet::new(), now);
        assert_eq!(out.len(), 2, "both should survive the 0.5 floor");
        assert_eq!(out[0].label, "Recent");
        assert!(out[0].score > out[1].score);
    }

    #[test]
    fn overlap_bonus_prefers_better_match() {
        let now = chrono::Local::now().timestamp();
        // Both same number of clicks but different overlap with current context.
        let rows = vec![
            mk_row("Multi",  "click", &["sysadmin", "perf"], &["diagnose"], 60),
            mk_row("Single", "click", &["sysadmin"],          &[],           60),
        ];
        let cur_domains: HashSet<_> = ["sysadmin".to_string(), "perf".to_string()].into_iter().collect();
        let cur_tools:   HashSet<_> = ["diagnose".to_string()].into_iter().collect();
        let out = score_candidates(&rows, &cur_domains, &cur_tools, now);
        assert_eq!(out[0].label, "Multi");
        assert!(out[0].score > out[1].score);
    }

    #[test]
    fn tool_overlap_alone_is_enough_for_match() {
        let now = chrono::Local::now().timestamp();
        let rows = vec![
            mk_row("ToolPick", "click", &["other"], &["mcp_query:github"], 60),
        ];
        let cur_domains: HashSet<_> = ["sysadmin".to_string()].into_iter().collect(); // NO overlap
        let cur_tools:   HashSet<_> = ["mcp_query:github".to_string()].into_iter().collect();
        let out = score_candidates(&rows, &cur_domains, &cur_tools, now);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "ToolPick");
    }

    #[test]
    fn case_insensitive_dedup_collapses_same_label() {
        let now = chrono::Local::now().timestamp();
        let rows = vec![
            mk_row("Snap", "click", &["sysadmin"], &[], 60),
            mk_row("snap", "click", &["sysadmin"], &[], 60),
            mk_row("SNAP", "click", &["sysadmin"], &[], 60),
        ];
        let cur_domains: HashSet<_> = ["sysadmin".to_string()].into_iter().collect();
        let out = score_candidates(&rows, &cur_domains, &HashSet::new(), now);
        // All three should collapse into one aggregate.
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn normalize_event_kind_canonicalizes() {
        assert_eq!(normalize_event_kind("click"),     "click");
        assert_eq!(normalize_event_kind("CLICK"),     "click");
        assert_eq!(normalize_event_kind("  click  "), "click");
        assert_eq!(normalize_event_kind("dismiss"),   "dismiss");
        assert_eq!(normalize_event_kind("Dismissed"), "dismiss");
        assert_eq!(normalize_event_kind("x"),         "dismiss");
        assert_eq!(normalize_event_kind("random"),    "click"); // unknown → click
    }

    #[test]
    fn parse_json_array_handles_invalid() {
        assert_eq!(parse_json_array("[]").len(), 0);
        assert_eq!(parse_json_array("[\"a\", \"B\"]").len(), 2);
        assert!(parse_json_array("not json").is_empty());
        // Case-folded on parse so set ops are insensitive.
        let s = parse_json_array("[\"FOO\"]");
        assert!(s.contains("foo"));
    }
}
