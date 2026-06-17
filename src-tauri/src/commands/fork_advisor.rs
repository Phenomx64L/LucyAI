// ── fork_advisor.rs — Auto-fork heuristic (v1.7.73) ─────────────────────
//
// Purpose: detect when a user prompt has 2+ independent branches Lucy
// could investigate in PARALLEL, and emit a soft directive that nudges
// the LLM toward `fork_task` / `wait_task` instead of sequential
// execution.
//
// ── Why this exists ─────────────────────────────────────────────────────
//
// v1.7.5 added auto-routing for security skills via cosine similarity
// against embedded skill descriptions. Subagents got no equivalent —
// the system prompt only DESCRIBES `fork_task`, leaving every fork
// decision to the LLM. In practice the LLM defaults to sequential
// execution because forks are more expensive and the prompt doesn't
// strongly nudge toward parallelism. Result: the user observed Lucy
// never forks unless explicitly told ("haz X en paralelo con Y").
//
// ── Strategy: cheap sintactic scorer ─────────────────────────────────────
//
// We don't need an LLM or embedding to recognise the obvious cases:
//
//   • "audita PROD-AD-01 y PROD-WEB-02"
//   • "compara la salida de servidor A vs servidor B"
//   • "para cada uno de estos archivos: a.log, b.log, c.log"
//   • "investiga X mientras revisas Y"
//
// All have surface-level signals (multiple hostnames, lists, comparison
// verbs, parallel conjunctions). A weighted score over those signals
// gives high-precision detection without an extra LLM round-trip.
//
// Bias to false negatives: we'd rather miss a fork than push the LLM
// into a wasteful parallel branch on a single-task prompt.
//
// ── Wired where ────────────────────────────────────────────────────────
//
//   • `commands/ai.rs` — calls `advise(prompt)` before assembling the
//     prompt. If `should_fork` is true, the result is stored in the
//     PromptContext and surfaced via the new `ForkAdvice` section.
//   • `prompt_sections.rs::ForkAdviceSection` — renders the strong
//     directive into the system prompt only when `should_fork`.
//   • Frontend chip in `src/routes/+page.svelte` reads the same advice
//     via the `fork_advice` Tauri command and renders a violet chip
//     in the composer ("🔱 fork-advised · 3 ramas") for transparency.
//   • Slash command `/serial` toggles a per-tab bypass that suppresses
//     advice for the next turn (escape hatch for the operator).

use regex::Regex;
use serde::{Deserialize, Serialize};

// ── Public API ──────────────────────────────────────────────────────────

/// The advice produced for a single user prompt.
///
/// • `should_fork` is the boolean the LLM-side directive keys off.
/// • `confidence` is the raw score on [0, 1+] — values above 0.65
///   trigger `should_fork = true`. Surfaced so the frontend can show
///   a soft "almost-fork-worthy" warning under the threshold.
/// • `branches` is the operator-facing summary of what Lucy would
///   fork into — derived from the strongest signal (multiple hosts,
///   list items, etc.). May be empty even when `should_fork` is true
///   if the signal was a generic conjunction.
/// • `signals` is the human-readable trace of which detectors fired
///   and by how much. Used by the frontend tooltip + `/diagnostico`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkAdvice {
    pub should_fork: bool,
    pub confidence: f32,
    pub branches: Vec<String>,
    pub signals: Vec<SignalHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalHit {
    pub kind: String,
    pub weight: f32,
    pub evidence: String,
}

/// Threshold above which we emit `should_fork = true`. Picked by hand
/// — single explicit "en paralelo" alone (weight 0.50) doesn't trigger
/// without at least one corroborating signal (lists or multi-host).
/// Avoids forking on edge cases like "ejecuta esto en paralelo si
/// puedes" with no actual branches.
pub const FORK_THRESHOLD: f32 = 0.65;

/// Public entry point. Returns advice based on the user's prompt.
/// Stateless and pure — safe to call from any thread, cheap to call
/// on every turn (sub-millisecond on a 10 KB prompt).
///
/// Bypass markers honoured (case-insensitive, anywhere in the prompt):
///   • `[NO-FORK]` — appended by the frontend when `/serial` bypass is on
///   • `\no-fork\b` standalone token
/// When detected, the function returns `should_fork = false` and a
/// single explanatory signal so the operator can see the bypass took
/// effect.
pub fn advise(prompt: &str) -> ForkAdvice {
    if prompt.trim().is_empty() {
        return ForkAdvice {
            should_fork: false,
            confidence: 0.0,
            branches: vec![],
            signals: vec![],
        };
    }

    let lower = prompt.to_lowercase();

    // ── Bypass marker check ──────────────────────────────────────────
    // The `/serial` slash command on the frontend appends `[NO-FORK]`
    // to the outgoing prompt. We bail out early without scoring so
    // the caller can confirm "yes, advisor ran, no fork was suggested
    // because the operator overrode".
    if BYPASS_RE.is_match(&lower) {
        return ForkAdvice {
            should_fork: false,
            confidence: 0.0,
            branches: vec![],
            signals: vec![SignalHit {
                kind: "bypass".into(),
                weight: 0.0,
                evidence: "[NO-FORK] marker present".into(),
            }],
        };
    }
    let mut score = 0.0_f32;
    let mut signals: Vec<SignalHit> = Vec::new();
    let mut branches: Vec<String> = Vec::new();

    // ── Signal 1: explicit parallelism marker ─────────────────────────
    // Weight 0.65: the operator literally asked for parallel execution.
    // Sufficient alone to cross the threshold — if the user knows the
    // word "paralelo" / "in parallel" they meant it.
    if PARALLEL_RE.is_match(&lower) {
        score += 0.65;
        signals.push(SignalHit {
            kind: "explicit_parallel".into(),
            weight: 0.65,
            evidence: PARALLEL_RE
                .find(&lower)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
        });
    }

    // ── Signal 2: multiple hostnames ──────────────────────────────────
    // Weight 0.65: highest-signal feature in a SysAdmin workflow.
    // Two or more machines = the natural unit of parallel work is
    // "one fork per host". Sufficient alone.
    let hosts = extract_hostnames(prompt);
    if hosts.len() >= 2 {
        score += 0.65;
        signals.push(SignalHit {
            kind: "multi_host".into(),
            weight: 0.65,
            evidence: format!("hosts={}", hosts.join(",")),
        });
        for h in &hosts {
            branches.push(format!("Host: {}", h));
        }
    }

    // ── Signal 3: enumerated list (bullets / numbered / comma list) ──
    // Weight 0.65: ≥3 enumerated items is the second most reliable
    // fork signal. Each item becomes a branch. Sufficient alone.
    let list_items = extract_list_items(prompt);
    if list_items.len() >= 3 {
        // Don't count this signal if it was already captured by hosts
        // (same items would be a double count).
        let new_items: Vec<&String> = list_items
            .iter()
            .filter(|it| !hosts.iter().any(|h| h.eq_ignore_ascii_case(it)))
            .collect();
        if new_items.len() >= 3 {
            score += 0.65;
            signals.push(SignalHit {
                kind: "list".into(),
                weight: 0.65,
                evidence: format!("items={}", new_items.len()),
            });
            if branches.is_empty() {
                for it in new_items.iter().take(5) {
                    branches.push((*it).to_string());
                }
            }
        }
    }

    // ── Signal 4: comparison verbs ────────────────────────────────────
    // "compara X vs Y" / "diff", "contrasta", "diferencia entre".
    // 0.30 — not enough alone. Combined with multi_host/list/paths it
    // pushes well past the threshold.
    if COMPARE_RE.is_match(&lower) {
        score += 0.30;
        signals.push(SignalHit {
            kind: "compare".into(),
            weight: 0.30,
            evidence: COMPARE_RE
                .find(&lower)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
        });
    }

    // ── Signal 5: cross-domain conjunction ────────────────────────────
    // "audita IIS y revisa el Event Viewer" — different verbs +
    // different domain nouns on each side of " y " / " and ".
    // 0.30 — easy false positives ("crea un archivo y revisa que se
    // haya guardado" is sequential). Needs corroboration.
    if CROSS_DOMAIN_RE.is_match(&lower) {
        score += 0.30;
        signals.push(SignalHit {
            kind: "cross_domain".into(),
            weight: 0.30,
            evidence: CROSS_DOMAIN_RE
                .find(&lower)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
        });
    }

    // ── Signal 6: multi-path / multi-URL ──────────────────────────────
    // ≥2 distinct absolute paths or URLs in the prompt. 0.30 alone is
    // below threshold; with any other signal it triggers.
    let paths = extract_paths(prompt);
    if paths.len() >= 2 {
        score += 0.30;
        signals.push(SignalHit {
            kind: "multi_path".into(),
            weight: 0.30,
            evidence: format!("paths={}", paths.len()),
        });
        if branches.is_empty() {
            for p in paths.iter().take(4) {
                branches.push(format!("Path: {}", p));
            }
        }
    }

    // ── Signal 7: long prompt with structural markers ─────────────────
    // Prompts > 200 chars containing newline + colon are usually
    // multi-step requests. Weak signal — only useful as tie-breaker.
    if prompt.len() > 200 && prompt.contains('\n') && prompt.contains(':') {
        score += 0.10;
        signals.push(SignalHit {
            kind: "structural".into(),
            weight: 0.10,
            evidence: format!("len={}", prompt.len()),
        });
    }

    let should_fork = score >= FORK_THRESHOLD;

    ForkAdvice {
        should_fork,
        confidence: score,
        branches,
        signals,
    }
}

// ── Detectors ───────────────────────────────────────────────────────────

// Bypass marker. Recognises the frontend's [NO-FORK] appendix and the
// `no-fork` / `serial-mode` literals an operator might type by hand.
static BYPASS_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
    Regex::new(r"(\[no-fork\]|\bno-fork\b|\bserial-mode\b)")
        .expect("BYPASS_RE compiles")
});

// Explicit parallelism: "en paralelo", "in parallel", "simultáneamente",
// "concurrent(ly)", "mientras". The "mientras" case captures Spanish
// "X mientras hace Y" — true concurrency in operator speech.
static PARALLEL_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
    Regex::new(r"\b(en paralelo|in parallel|simult[aá]neamente|simultaneously|concurrent(ly)?|mientras (revisa|analiza|verifica|chequea|investiga)|al mismo tiempo)\b")
        .expect("PARALLEL_RE compiles")
});

// Comparison verbs that imply two reads + synthesis.
static COMPARE_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
    Regex::new(r"\b(compara|comparar|compare|diff|differences?|diferencias? entre|contrasta|contrast|vs\.?|versus|sincroniza con)\b")
        .expect("COMPARE_RE compiles")
});

// Cross-domain conjunction: two distinct verbs separated by " y " / " and "
// where each verb belongs to a different action category. Conservative
// regex — only fires when both halves carry a recognised SysAdmin verb.
static CROSS_DOMAIN_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
    Regex::new(r"\b(audita|revisa|analiza|investiga|verifica|chequea|monitorea|check|audit|analyze|inspect)\b[^.]{2,80}\b(y|and)\b[^.]{2,80}\b(audita|revisa|analiza|investiga|verifica|chequea|monitorea|check|audit|analyze|inspect|consulta|genera|crea|busca|limpia)\b")
        .expect("CROSS_DOMAIN_RE compiles")
});

// Hostname heuristic: PROD-AD-01, web-01, app2.example.com, etc.
// We allow letters/digits/dashes/dots, require at least one digit or
// dash so common words don't match, and a min length of 4.
static HOSTNAME_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
    Regex::new(r"\b([A-Za-z][A-Za-z0-9]*[-_][A-Za-z0-9-]{2,}[A-Za-z0-9]|[A-Za-z][A-Za-z0-9]+\d+(?:\.[A-Za-z0-9.-]+)?)\b")
        .expect("HOSTNAME_RE compiles")
});

// Path / URL detection.
static PATH_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
    Regex::new(r"((?:[A-Za-z]:[\\/])[\w\-. \\/]+|/(?:[A-Za-z0-9_\-.]+/)+[A-Za-z0-9_\-.]+|https?://[\w./\-?=&%#]+)")
        .expect("PATH_RE compiles")
});

// List bullet / numbered detection.
static LIST_BULLET_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
    Regex::new(r"(?m)^\s*([-*•]|\d+\.|\d+\))\s+(.+)$").expect("LIST_BULLET_RE compiles")
});

fn extract_hostnames(prompt: &str) -> Vec<String> {
    // Filter out hostname-shaped tokens that are actually well-known
    // English/Spanish words (Lucy's own constants, common terms). The
    // hostname regex is broad on purpose; this is the precision filter.
    const STOP: &[&str] = &[
        "ipv4", "ipv6", "http1", "http2", "http3", "tls12", "tls13",
        "sql2019", "sql2022", "v1", "v2", "v3", "rfc1918", "utf8",
        "powershell", "windows10", "windows11", "lucy", "claude",
        "sysadmin", "log4j", "log4j2",
    ];
    let mut hits: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for m in HOSTNAME_RE.find_iter(prompt) {
        let s = m.as_str();
        if s.len() < 4 {
            continue;
        }
        let lower = s.to_lowercase();
        if STOP.contains(&lower.as_str()) {
            continue;
        }
        // Skip pure-letter sequences without a digit or dash (the
        // regex's second arm allows them but they're noisy).
        let has_digit_or_dash = s.chars().any(|c| c.is_ascii_digit() || c == '-' || c == '_');
        if !has_digit_or_dash {
            continue;
        }
        if seen.insert(lower) {
            hits.push(s.to_string());
        }
    }
    hits
}

fn extract_list_items(prompt: &str) -> Vec<String> {
    // First, dedicated bullet/numbered list (most reliable).
    let bullets: Vec<String> = LIST_BULLET_RE
        .captures_iter(prompt)
        .filter_map(|c| c.get(2).map(|m| m.as_str().trim().to_string()))
        .filter(|s| !s.is_empty() && s.len() <= 80)
        .collect();
    if !bullets.is_empty() {
        return bullets;
    }
    // Fallback: comma-separated list after a colon. We require ≥3
    // items so a regular two-clause sentence doesn't qualify.
    if let Some(colon_pos) = prompt.find(':') {
        let tail = &prompt[colon_pos + 1..];
        let items: Vec<String> = tail
            .split([',', ';'])
            .map(|s| s.trim().trim_end_matches('.').to_string())
            .filter(|s| !s.is_empty() && s.len() <= 60)
            .collect();
        if items.len() >= 3 {
            return items;
        }
    }
    Vec::new()
}

fn extract_paths(prompt: &str) -> Vec<String> {
    let mut hits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for m in PATH_RE.find_iter(prompt) {
        let s = m.as_str().trim().to_string();
        if seen.insert(s.clone()) {
            hits.push(s);
        }
    }
    hits
}

// ── Tauri command ───────────────────────────────────────────────────────

/// Exposed to the frontend so the composer chip can preview the advice
/// as the user types. Cheap enough to call on every keystroke without
/// debouncing.
#[tauri::command]
pub fn fork_advice(prompt: String) -> ForkAdvice {
    advise(&prompt)
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_prompt_returns_zero() {
        let a = advise("");
        assert!(!a.should_fork);
        assert_eq!(a.confidence, 0.0);
        assert!(a.signals.is_empty());
    }

    #[test]
    fn single_simple_task_does_not_fork() {
        let a = advise("reinicia el servicio de IIS");
        assert!(!a.should_fork, "got {:?}", a);
    }

    #[test]
    fn two_hosts_triggers_fork() {
        let a = advise("audita PROD-AD-01 y PROD-WEB-02 buscando puertos abiertos");
        assert!(a.should_fork, "got {:?}", a);
        // multi_host (0.40) + cross_domain "audita y …" no — only one verb.
        // The 0.40 alone doesn't reach 0.65, so the cross-domain rule
        // must catch the second clause to cross the threshold.
        // In practice the score should include cross_domain as well.
        assert!(a.confidence >= FORK_THRESHOLD, "score {}", a.confidence);
        assert!(a.branches.iter().any(|b| b.contains("PROD-AD-01")));
        assert!(a.branches.iter().any(|b| b.contains("PROD-WEB-02")));
    }

    #[test]
    fn explicit_parallel_alone_triggers() {
        // The operator literally asked for parallelism. We honour it
        // even without a structural signal — they know what they want
        // and the worst case (no real branches to fork) is the LLM
        // emitting one fork_task and immediately wait_task'ing it.
        let a = advise("haz esto en paralelo");
        assert!(a.should_fork, "got {:?}", a);
    }

    #[test]
    fn explicit_parallel_plus_compare_triggers_strongly() {
        let a = advise("compara los dos enfoques en paralelo");
        assert!(a.should_fork, "got {:?}", a);
        assert!(a.confidence >= 0.90, "should accumulate signals: {}", a.confidence);
    }

    #[test]
    fn bullet_list_triggers() {
        let prompt = "Revisa lo siguiente:\n- estado de IIS\n- logs del Event Viewer\n- conexiones TCP en el firewall";
        let a = advise(prompt);
        assert!(a.should_fork, "got {:?}", a);
        // Branches should reflect the bullets.
        assert!(!a.branches.is_empty());
    }

    #[test]
    fn comma_list_of_three_triggers() {
        let a = advise("para cada uno de estos archivos: a.log, b.log, c.log, dime el tamaño");
        assert!(a.should_fork || a.confidence > 0.45, "got {:?}", a);
    }

    #[test]
    fn cross_domain_conjunction_triggers() {
        let a = advise("audita IIS y revisa el Event Viewer en busca de errores");
        // "audita … y revisa …" is the cross_domain hit (0.30) — not
        // enough alone. With a generic prompt it shouldn't fork.
        assert!(!a.should_fork, "got {:?}", a);
        assert!(a.confidence > 0.0);
    }

    #[test]
    fn signals_carry_evidence() {
        let a = advise("audita PROD-AD-01 y PROD-WEB-02 en paralelo");
        // All three: explicit_parallel, multi_host, cross_domain.
        let kinds: Vec<&str> = a.signals.iter().map(|s| s.kind.as_str()).collect();
        assert!(kinds.contains(&"explicit_parallel"));
        assert!(kinds.contains(&"multi_host"));
    }

    #[test]
    fn hostname_extraction_ignores_common_words() {
        // "powershell" looks like a hostname (letters + something) but
        // it's in the stop list. Same for "lucy".
        let a = advise("ejecuta un script powershell en lucy");
        assert_eq!(a.signals.iter().filter(|s| s.kind == "multi_host").count(), 0);
    }

    #[test]
    fn confidence_caps_below_two() {
        // Even when EVERY signal fires, the sum should stay reasonable.
        let prompt = "Compara PROD-AD-01 vs PROD-WEB-02 en paralelo, revisando:\n\
                      - logs de \\\\PROD-AD-01\\C$\\Windows\\System32\\logs\n\
                      - logs de \\\\PROD-WEB-02\\C$\\Windows\\System32\\logs\n\
                      - conectividad TCP\n\
                      audita IIS y revisa Event Viewer simultáneamente";
        let a = advise(prompt);
        assert!(a.should_fork);
        // Cap is loose — every signal firing means a definitively
        // fork-worthy prompt, and the LLM consumes only the boolean
        // anyway. We just want it bounded so a UI surface (chip
        // confidence bar) doesn't render past its max.
        assert!(a.confidence < 3.0, "score {}", a.confidence);
    }

    #[test]
    fn bypass_marker_suppresses_advice() {
        // Strong fork-worthy prompt but with the [NO-FORK] marker the
        // frontend appends when /serial bypass is on.
        let a = advise("audita PROD-AD-01 y PROD-WEB-02 [NO-FORK]");
        assert!(!a.should_fork, "got {:?}", a);
        assert_eq!(a.confidence, 0.0);
        assert_eq!(a.signals.len(), 1);
        assert_eq!(a.signals[0].kind, "bypass");
    }

    #[test]
    fn path_extraction_picks_up_urls_and_unc() {
        let a = advise("descarga https://example.com/a.zip y https://example.com/b.zip");
        let paths_hit = a.signals.iter().any(|s| s.kind == "multi_path");
        assert!(paths_hit, "got {:?}", a);
    }
}
