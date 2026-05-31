// ── security_skills.rs — Anthropic Cybersecurity Skills loader (v1.7.4) ──
//
// Loads the 213 SKILL.md files bundled in `docs/security-skills/` into
// an in-memory index. Each skill's YAML frontmatter is parsed for
// metadata (name, description, domain, subdomain, tags, framework
// mappings); the body is read on demand.
//
// Source attribution: Anthropic-Cybersecurity-Skills by mukul975
// (Mahipal Singh), Apache 2.0. See `docs/security-skills/ATTRIBUTION.md`
// and `docs/security-skills/LICENSE` for the full notice.
//
// ── Search strategy ─────────────────────────────────────────────────────
//
// For a list of ~213 items search complexity is irrelevant — we score
// every skill against the query and return the top-N. Per-skill score:
//
//     name match (exact substr) ........ +10
//     name match (token)              ... +6
//     description match (token)        .. +3
//     tag match                        .. +5 per tag
//     domain / subdomain match         .. +4
//     framework code match (T1071, …)  .. +8  (high value — exact intent)
//
// Tokenisation: lowercase, split on non-alphanumeric, drop tokens < 3 chars.
//
// ── Caching ────────────────────────────────────────────────────────────
//
// The index is built lazily on first call via `OnceCell`. Subsequent
// calls reuse it without touching disk. The skill BODY is NOT cached
// — read on demand from disk. This keeps memory bounded (~50 KB for
// 213 metadata entries) and survives skill edits without a restart.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;

// ── Types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    /// Slugified id matching the directory name.
    pub id:          String,
    /// Human-readable name from the YAML frontmatter.
    pub name:        String,
    pub description: String,
    pub domain:      String,
    pub subdomain:   String,
    pub tags:        Vec<String>,
    pub version:     String,
    pub author:      String,
    /// Cross-framework mappings — flattened so the frontend can render
    /// them uniformly without knowing the YAML schema.
    pub nist_csf:    Vec<String>,
    pub mitre_attck: Vec<String>,
    pub mitre_atlas: Vec<String>,
    pub mitre_d3fend:Vec<String>,
    pub ai_rmf:      Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSearchHit {
    pub meta:  SkillMeta,
    pub score: i32,
    /// First 240 chars of the description for the result list. Lets the
    /// UI render without fetching the full body.
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFull {
    pub meta: SkillMeta,
    /// Raw markdown body, frontmatter stripped.
    pub body: String,
}

// ── Index ──────────────────────────────────────────────────────────────

static INDEX: OnceLock<Vec<SkillMeta>> = OnceLock::new();

/// Resolve the bundled skills directory across the three runtimes:
///   1. `npm run tauri dev` — cwd is the workspace root → `docs/security-skills`
///   2. `cargo run` from `src-tauri/` — cwd is src-tauri → `../docs/security-skills`
///   3. Installed binary (nsis/msi) — Tauri's resource bundler drops
///      `../docs/security-skills/` relative to the .exe location.
/// We probe all three and return the first that exists.
fn skills_dir() -> PathBuf {
    let candidates: Vec<PathBuf> = {
        let mut v = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            v.push(cwd.join("docs").join("security-skills"));
            v.push(cwd.join("..").join("docs").join("security-skills"));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                // Tauri 2 nsis layout: $EXE_DIR/resources/_up_/docs/...
                v.push(parent.join("resources").join("_up_").join("docs").join("security-skills"));
                // Tauri 2 generic resource subdir
                v.push(parent.join("resources").join("docs").join("security-skills"));
                v.push(parent.join("..").join("docs").join("security-skills"));
            }
        }
        v
    };
    for p in &candidates {
        if p.exists() { return p.clone(); }
    }
    // Last resort: first candidate, even if missing. The first use will
    // return an empty index and the UI message points the dev at the
    // expected location.
    candidates.into_iter().next().unwrap_or_else(|| PathBuf::from("docs/security-skills"))
}

/// Walk the skills directory and parse every `*/SKILL.md`. Cached in
/// `INDEX` after first call.
fn load_index() -> &'static Vec<SkillMeta> {
    INDEX.get_or_init(|| {
        let root = skills_dir();
        let mut out: Vec<SkillMeta> = Vec::new();
        let Ok(entries) = std::fs::read_dir(&root) else {
            // No directory — return empty index. Front-end shows
            // "no skills loaded" rather than crashing.
            return out;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
            let skill_md = entry.path().join("SKILL.md");
            if !skill_md.exists() { continue; }
            let Ok(text) = std::fs::read_to_string(&skill_md) else { continue; };
            let id = entry.file_name().to_string_lossy().to_string();
            if let Some(meta) = parse_frontmatter(&id, &text) {
                out.push(meta);
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    })
}

/// Tiny YAML frontmatter parser scoped to the SKILL.md schema. We don't
/// pull serde_yaml just for this — every field is either a scalar
/// `key: value` or a list `key:\n  - item` with predictable formatting.
fn parse_frontmatter(id: &str, text: &str) -> Option<SkillMeta> {
    let trimmed = text.trim_start();
    if !trimmed.starts_with("---") { return None; }
    let after = &trimmed[3..];
    let end_idx = after.find("\n---")?;
    let yaml_block = &after[..end_idx];

    let mut name        = String::new();
    let mut description = String::new();
    let mut domain      = String::new();
    let mut subdomain   = String::new();
    let mut tags        = Vec::new();
    let mut version     = String::new();
    let mut author      = String::new();
    let mut nist_csf    = Vec::new();
    let mut mitre_attck = Vec::new();
    let mut mitre_atlas = Vec::new();
    let mut mitre_d3fend= Vec::new();
    let mut ai_rmf      = Vec::new();

    let mut current_list: Option<&mut Vec<String>> = None;
    let mut pending_desc = false;

    for raw_line in yaml_block.lines() {
        let line = raw_line.trim_end();
        if line.is_empty() { continue; }
        if let Some(stripped) = line.strip_prefix("- ") {
            // List item — append to current list if one's active.
            if let Some(list) = current_list.as_deref_mut() {
                let val = stripped.trim().trim_matches('\'').trim_matches('"').to_string();
                if !val.is_empty() { list.push(val); }
            } else if pending_desc {
                // Multi-line description rolled into a single string.
                description.push(' ');
                description.push_str(stripped.trim());
            }
            continue;
        }
        // Continuation of a scalar (indented value on next line):
        if line.starts_with("  ") && pending_desc {
            description.push(' ');
            description.push_str(line.trim());
            continue;
        }
        // New key.
        current_list = None;
        pending_desc = false;
        let Some(colon) = line.find(':') else { continue; };
        let key = line[..colon].trim();
        let raw_val = line[colon+1..].trim().trim_matches('\'').trim_matches('"');
        match key {
            "name"        => name        = raw_val.to_string(),
            "description" => {
                description = raw_val.to_string();
                pending_desc = true;
            },
            "domain"      => domain      = raw_val.to_string(),
            "subdomain"   => subdomain   = raw_val.to_string(),
            "version"     => version     = raw_val.to_string(),
            "author"      => author      = raw_val.to_string(),
            "tags"        => { current_list = Some(&mut tags); },
            "nist_csf"    => { current_list = Some(&mut nist_csf); },
            "mitre_attck" | "mitre_att_ck" | "attck" => { current_list = Some(&mut mitre_attck); },
            "mitre_atlas" => { current_list = Some(&mut mitre_atlas); },
            "mitre_d3fend"=> { current_list = Some(&mut mitre_d3fend); },
            "ai_rmf" | "nist_ai_rmf" => { current_list = Some(&mut ai_rmf); },
            _ => {}
        }
    }

    Some(SkillMeta {
        id: id.to_string(),
        name: if name.is_empty() { id.to_string() } else { name },
        description,
        domain,
        subdomain,
        tags,
        version,
        author,
        nist_csf,
        mitre_attck,
        mitre_atlas,
        mitre_d3fend,
        ai_rmf,
    })
}

// ── Search ─────────────────────────────────────────────────────────────

fn tokenize(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(String::from)
        .collect()
}

fn score_skill(meta: &SkillMeta, query_lc: &str, query_tokens: &HashSet<String>) -> i32 {
    let mut score: i32 = 0;
    let name_lc = meta.name.to_lowercase();
    let desc_lc = meta.description.to_lowercase();

    if name_lc.contains(query_lc) { score += 10; }
    if desc_lc.contains(query_lc) { score +=  4; }

    let name_tok = tokenize(&meta.name);
    let desc_tok = tokenize(&meta.description);
    for t in query_tokens {
        if name_tok.contains(t) { score += 6; }
        if desc_tok.contains(t) { score += 3; }
        if meta.tags.iter().any(|tag| tag.to_lowercase() == *t) { score += 5; }
        if meta.subdomain.to_lowercase() == *t { score += 4; }
        if meta.domain.to_lowercase() == *t { score += 4; }
        // Framework codes — high value because they encode exact intent
        // (T1071 = network C2 exfil, RS.AN-01 = incident response analysis).
        for code in meta.mitre_attck.iter()
            .chain(meta.nist_csf.iter())
            .chain(meta.mitre_atlas.iter())
            .chain(meta.mitre_d3fend.iter())
            .chain(meta.ai_rmf.iter())
        {
            if code.to_lowercase() == *t { score += 8; }
        }
    }
    score
}

fn preview(text: &str, max: usize) -> String {
    let trimmed: String = text.chars().take(max).collect();
    if text.len() > max { format!("{}…", trimmed) } else { trimmed }
}

// ── Tauri commands ─────────────────────────────────────────────────────

/// Return the full index. Cheap — metadata only (~50 KB total).
#[tauri::command]
pub async fn security_skills_list() -> Result<Vec<SkillMeta>, String> {
    Ok(load_index().clone())
}

/// Keyword + framework-code search. `limit` defaults to 10, max 30.
#[tauri::command]
pub async fn security_skills_search(
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SkillSearchHit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.unwrap_or(10).clamp(1, 30);
    let query_lc = q.to_lowercase();
    let query_tokens = tokenize(q);
    let mut hits: Vec<SkillSearchHit> = load_index().iter()
        .filter_map(|m| {
            let s = score_skill(m, &query_lc, &query_tokens);
            if s == 0 { return None; }
            Some(SkillSearchHit {
                meta: m.clone(),
                score: s,
                preview: preview(&m.description, 240),
            })
        })
        .collect();
    hits.sort_by(|a, b| b.score.cmp(&a.score));
    hits.truncate(limit);
    Ok(hits)
}

/// Return one skill including the full markdown body (frontmatter
/// stripped). Reads from disk every call so edits to the underlying
/// file are visible without a restart.
#[tauri::command]
pub async fn security_skills_get(id: String) -> Result<SkillFull, String> {
    let meta = load_index().iter().find(|m| m.id == id).cloned()
        .ok_or_else(|| format!("security_skills_get: unknown id '{}'", id))?;
    let path = skills_dir().join(&id).join("SKILL.md");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read SKILL.md: {}", e))?;
    // Strip frontmatter so the body is ready to inject as a prompt prefix.
    let body = if let Some(rest) = raw.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            rest[end+4..].trim_start().to_string()
        } else { raw }
    } else { raw };
    Ok(SkillFull { meta, body })
}

/// Distinct subdomain list with counts. Used by the UI's category picker.
#[tauri::command]
pub async fn security_skills_categories() -> Result<Vec<(String, usize)>, String> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for m in load_index().iter() {
        let key = if m.subdomain.is_empty() { m.domain.clone() } else { m.subdomain.clone() };
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut out: Vec<(String, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(out)
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_frontmatter() {
        let txt = "---\nname: foo-bar\ndescription: A test skill.\ndomain: cybersecurity\nsubdomain: forensics\ntags:\n- forensics\n- triage\nversion: '1.0'\nauthor: tester\n---\n# Body";
        let m = parse_frontmatter("foo-bar", txt).expect("parse should succeed");
        assert_eq!(m.id, "foo-bar");
        assert_eq!(m.name, "foo-bar");
        assert_eq!(m.subdomain, "forensics");
        assert_eq!(m.tags.len(), 2);
        assert!(m.tags.contains(&"forensics".to_string()));
    }

    #[test]
    fn parses_framework_lists() {
        let txt = "---\nname: x\ndescription: y\ntags:\n- a\nnist_csf:\n- RS.AN-01\n- DE.AE-02\nmitre_attck:\n- T1071\n---\n# body";
        let m = parse_frontmatter("x", txt).unwrap();
        assert_eq!(m.nist_csf, vec!["RS.AN-01", "DE.AE-02"]);
        assert_eq!(m.mitre_attck, vec!["T1071"]);
    }

    #[test]
    fn missing_frontmatter_returns_none() {
        assert!(parse_frontmatter("nope", "# Just a heading").is_none());
    }

    #[test]
    fn score_prefers_name_match_over_description() {
        let meta = SkillMeta {
            id: "x".into(), name: "Volatility Memory Analysis".into(),
            description: "Long description without that exact word".into(),
            domain: "cybersecurity".into(), subdomain: "forensics".into(),
            tags: vec![], version: "1.0".into(), author: "".into(),
            nist_csf: vec![], mitre_attck: vec![], mitre_atlas: vec![],
            mitre_d3fend: vec![], ai_rmf: vec![],
        };
        let q  = "volatility";
        let qt = tokenize(q);
        let s  = score_skill(&meta, q, &qt);
        assert!(s >= 16, "name substr (10) + name token (6) = at least 16, got {}", s);
    }

    #[test]
    fn score_matches_framework_codes() {
        let meta = SkillMeta {
            id: "x".into(), name: "anything".into(), description: "anything".into(),
            domain: "".into(), subdomain: "".into(), tags: vec![],
            version: "1.0".into(), author: "".into(),
            nist_csf: vec![],
            mitre_attck: vec!["T1071".into(), "T1059".into()],
            mitre_atlas: vec![], mitre_d3fend: vec![], ai_rmf: vec![],
        };
        let q  = "T1071";
        let qt = tokenize(q);
        let s  = score_skill(&meta, q, &qt);
        // Framework token match = +8 for the T1071 code.
        assert!(s >= 8, "expected ≥ 8 for framework code match, got {}", s);
    }

    #[test]
    fn score_zero_for_no_match() {
        let meta = SkillMeta {
            id: "x".into(), name: "kubernetes".into(), description: "k8s stuff".into(),
            domain: "".into(), subdomain: "".into(), tags: vec![],
            version: "1.0".into(), author: "".into(),
            nist_csf: vec![], mitre_attck: vec![], mitre_atlas: vec![],
            mitre_d3fend: vec![], ai_rmf: vec![],
        };
        let q  = "phishing";
        let qt = tokenize(q);
        assert_eq!(score_skill(&meta, q, &qt), 0);
    }

    #[test]
    fn tokenize_drops_short_tokens() {
        let t = tokenize("a, bb, k8s, malware");
        assert!(!t.contains("a"));
        assert!(!t.contains("bb"));
        assert!(t.contains("k8s"));
        assert!(t.contains("malware"));
    }
}
