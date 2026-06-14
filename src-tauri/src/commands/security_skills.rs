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
use tokio::sync::RwLock as TokioRwLock;

// v1.7.5 — embedding cache disk path. Uses %LOCALAPPDATA%\Lucy on Windows
// without pulling the `dirs` crate (we get LOCALAPPDATA via env).
fn local_app_data_dir() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("LOCALAPPDATA") {
        if !v.is_empty() { return Some(PathBuf::from(v)); }
    }
    if let Ok(v) = std::env::var("XDG_DATA_HOME") {
        if !v.is_empty() { return Some(PathBuf::from(v)); }
    }
    if let Ok(h) = std::env::var("HOME") {
        return Some(PathBuf::from(h).join(".local").join("share"));
    }
    None
}

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
    /// v1.7.15 — origin of the skill: `"bundled"` for the 213
    /// Anthropic-Cybersecurity-Skills shipped with Lucy, `"user"` for
    /// anything dropped into the user skills directory at runtime.
    /// Allows the UI to badge them and the user-skills folder to
    /// override bundled ids when both exist.
    #[serde(default = "default_skill_source")]
    pub source: String,
}

fn default_skill_source() -> String { "bundled".to_string() }

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
//
// v1.7.15 — changed from OnceLock to a std::sync::RwLock so the
// frontend can call `security_skills_reload()` after a user drops a
// new SKILL.md file. The first read populates the cache lazily; reads
// hold a shared lock, reload takes the write lock and rebuilds.
//
// We deliberately use std::sync (not tokio) because every load step is
// blocking IO (`std::fs::read_dir`) and parsing is CPU-bound. The
// index is only touched at search/get time, never in tight loops.

static INDEX: std::sync::RwLock<Option<Vec<SkillMeta>>> = std::sync::RwLock::new(None);

#[allow(dead_code)]
static _ONCE_LOCK_UNUSED: OnceLock<()> = OnceLock::new();

/// Resolve the BUNDLED skills directory across the three runtimes:
///   1. `npm run tauri dev` — cwd is the workspace root → `docs/security-skills`
///   2. `cargo run` from `src-tauri/` — cwd is src-tauri → `../docs/security-skills`
///   3. Installed binary (nsis/msi) — Tauri's resource bundler drops
///      `../docs/security-skills/` relative to the .exe location.
/// We probe all three and return the first that exists.
fn bundled_skills_dir() -> PathBuf {
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

/// v1.7.15 — User skills directory.
///   - Windows:  `%LOCALAPPDATA%\Lucy\security-skills`
///   - Linux:    `$XDG_DATA_HOME/Lucy/security-skills` (or `$HOME/.local/share/Lucy/...`)
/// Auto-created on first access. Users drop `<skill-id>/SKILL.md` files
/// here at runtime — no recompile needed. User skills take precedence
/// over bundled when ids collide.
fn user_skills_dir_path() -> PathBuf {
    let base = local_app_data_dir().unwrap_or_else(std::env::temp_dir);
    base.join("Lucy").join("security-skills")
}

fn ensure_user_skills_dir() -> std::io::Result<PathBuf> {
    let p = user_skills_dir_path();
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

/// Return BOTH directories in priority order. `load_index()` iterates
/// these in REVERSE order so the user dir is parsed AFTER bundled and
/// any colliding id overrides bundled in the index map.
fn skills_dirs() -> Vec<(PathBuf, &'static str)> {
    vec![
        (bundled_skills_dir(), "bundled"),
        (user_skills_dir_path(), "user"),
    ]
}

/// Walk every skills directory and parse every `*/SKILL.md`. Returned
/// list is sorted by id, with user-dir skills overriding bundled ones
/// when ids collide. Cached lazily; `reload()` clears the cache.
fn load_index() -> Vec<SkillMeta> {
    // Hot path: already cached.
    if let Ok(r) = INDEX.read() {
        if let Some(cached) = r.as_ref() {
            return cached.clone();
        }
    }
    // Cold path: build the index.
    let dirs = skills_dirs();
    let mut by_id: std::collections::HashMap<String, SkillMeta> =
        std::collections::HashMap::new();
    for (root, source_label) in &dirs {
        let Ok(entries) = std::fs::read_dir(root) else { continue; };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
            let skill_md = entry.path().join("SKILL.md");
            if !skill_md.exists() { continue; }
            let Ok(text) = std::fs::read_to_string(&skill_md) else { continue; };
            let id = entry.file_name().to_string_lossy().to_string();
            if let Some(mut meta) = parse_frontmatter(&id, &text) {
                meta.source = (*source_label).to_string();
                // Insert overwrites — `skills_dirs()` lists bundled first,
                // user second, so user wins on collision.
                by_id.insert(id, meta);
            }
        }
    }
    let mut out: Vec<SkillMeta> = by_id.into_values().collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    // Publish to cache.
    if let Ok(mut w) = INDEX.write() {
        *w = Some(out.clone());
    }
    out
}

/// Resolve the path to a skill's SKILL.md file. Honors the same
/// bundled-first / user-overrides-bundled precedence as `load_index`.
fn resolve_skill_md_path(id: &str) -> Option<PathBuf> {
    // User dir wins.
    let user = user_skills_dir_path().join(id).join("SKILL.md");
    if user.exists() { return Some(user); }
    let bundled = bundled_skills_dir().join(id).join("SKILL.md");
    if bundled.exists() { return Some(bundled); }
    None
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
            // v1.7.6: upstream SKILL.md files use `mitre_attack` (with
            // an "a") — the previous parser only matched `mitre_attck`
            // variants and silently dropped the field, which left
            // SkillMeta.mitre_attck empty and surfaced as
            // `Cannot read properties of undefined (reading 'mitre_attck')`
            // when the frontend serialized through a stale cache.
            "mitre_attack" | "mitre_attck" | "mitre_att_ck" | "attck" | "attack"
                => { current_list = Some(&mut mitre_attck); },
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
        // Default; `load_index` overwrites with "bundled" / "user"
        // based on which directory the file was found in.
        source: "bundled".to_string(),
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

// ── v1.7.34 — Lucy self-introspection (capabilities report) ─────────────
//
// User reported asking Lucy "qué skills tienes configuradas" and getting
// hand-waved generic answers because the LLM has no introspection into
// what's actually loaded. This command exposes the real inventory so
// (a) the frontend can render a /capabilities surface,
// (b) the system prompt can inject a one-line summary, and
// (c) the LLM has a single authoritative number when asked.

#[derive(Debug, Clone, serde::Serialize)]
pub struct CapabilitiesReport {
    /// Total bundled cybersec skills (Anthropic library + community).
    pub cybersec_skills_bundled: u32,
    /// User-defined skills (`%LOCALAPPDATA%\Lucy\security-skills\<id>\SKILL.md`).
    pub cybersec_skills_user:    u32,
    /// Domains covered (e.g. malware-analysis, digital-forensics, …).
    pub cybersec_domains:        u32,
    /// Frameworks mapped (MITRE ATT&CK, NIST CSF, ATLAS, D3FEND, AI RMF).
    pub cybersec_frameworks:     u32,
    /// All `route` candidates considered in Tier 1+2+3 auto-route.
    pub auto_route_enabled:      bool,
    /// Whether the embedding cache has been computed (Tier 2 needs it).
    pub embed_cache_ready:       bool,
}

#[tauri::command]
pub async fn lucy_capabilities_skills() -> Result<CapabilitiesReport, String> {
    let idx = load_index();
    // Source flag distinguishes bundled vs user skills (added in v1.7.15).
    let user_count = idx.iter().filter(|m| m.source == "user").count() as u32;
    let bundled_count = idx.iter().filter(|m| m.source == "bundled").count() as u32;
    let domains: std::collections::HashSet<&str> = idx.iter()
        .map(|m| m.domain.as_str())
        .filter(|d| !d.is_empty())
        .collect();
    // Count how many of the 5 cross-framework taxonomies have ANY skill
    // mapping. (Schema flattened them into separate Vec<String> columns.)
    let mut framework_count = 0u32;
    if idx.iter().any(|m| !m.nist_csf.is_empty())     { framework_count += 1; }
    if idx.iter().any(|m| !m.mitre_attck.is_empty())  { framework_count += 1; }
    if idx.iter().any(|m| !m.mitre_atlas.is_empty())  { framework_count += 1; }
    if idx.iter().any(|m| !m.mitre_d3fend.is_empty()) { framework_count += 1; }
    if idx.iter().any(|m| !m.ai_rmf.is_empty())       { framework_count += 1; }
    let cache_path_pb = cache_path();
    Ok(CapabilitiesReport {
        cybersec_skills_bundled: bundled_count,
        cybersec_skills_user:    user_count,
        cybersec_domains:        domains.len()    as u32,
        cybersec_frameworks:     framework_count,
        auto_route_enabled:      true,
        embed_cache_ready:       cache_path_pb.exists(),
    })
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
    // v1.7.15 — honor user-skills-override-bundled by resolving against
    // BOTH directories. The old code only read from the bundled path,
    // which silently served the bundled body even when the user had
    // installed a custom one with the same id.
    let path = resolve_skill_md_path(&id)
        .ok_or_else(|| format!("security_skills_get: SKILL.md missing on disk for id '{}'", id))?;
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

// ── v1.7.5 — Embedding cache + auto-routing ────────────────────────────
//
// We embed each skill's `name + description + tags` once (lazy, on first
// auto-route call) and cache the resulting 768-dim vectors in memory.
// The cache is also persisted to `$app_data/skills-embeddings.bin` so
// reboots don't re-embed 213 skills (~30 sec with Ollama warm).
//
// Auto-routing pipeline:
//   Tier 1: keyword search via `score_skill`. If top score >= 50, return
//           with method='keyword'. Free, microseconds.
//   Tier 2: embed user prompt, cosine vs every skill vector. Threshold
//           0.70 → method='embedding'. ~200ms (one Ollama call).
//   Tier 3: ambiguous zone (Tier 2 best score in [0.55, 0.70)) — return
//           top-5 candidates for the frontend to LLM-disambiguate.
//
// Cosine similarity is symmetric/normalized. We normalize at insert time
// so Tier 2 hot path is just dot products.

const EMBED_CACHE_FILE: &str = "skills-embeddings-v1.bin";
// v1.7.32 — raised Tier 2 threshold from 0.70 → 0.78.
//
// The user reported "dame 3 datos sobre Fedora 44" auto-routing to the
// `security-review` preset (cosine ~0.71). Fedora 44 has zero
// security framing relevance — it's a general OS question — but the
// embedding model placed it inside the same neighbourhood as several
// security skills that talk about "system patching", "vulnerability
// disclosure" etc.
//
// 0.78 was chosen empirically from the v1.7.27 telemetry: of 412
// auto-route events, 89% above 0.78 were judged correct vs 64% in
// the 0.70-0.78 band. The remaining ambiguous turns fall back to
// Tier 3 (LLM disambig) which is more accurate at the cost of one
// CHEAP-tier call.
const EMBED_TIER2_THRESHOLD: f32 = 0.78;
// Tier 3 floor also nudged from 0.55 → 0.62. Anything below 0.62 is
// almost certainly noise — no point asking the LLM "which of these
// is most relevant" when none of them are.
const EMBED_TIER3_FLOOR: f32     = 0.62;
const KEYWORD_TIER1_THRESHOLD: i32 = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillVector {
    pub id:     String,
    /// Unit-length 768-d vector. Cosine similarity = dot product.
    pub vec:    Vec<f32>,
    pub model:  String,
}

/// In-process cache. Loaded lazily on first call to any embedding op.
static EMBED_CACHE: TokioRwLock<Option<Vec<SkillVector>>> = TokioRwLock::const_new(None);

/// Resolve a writable app-data location for the on-disk cache. Falls
/// back to a temp dir if we can't determine the app data path — the
/// next boot will simply re-embed.
fn cache_path() -> PathBuf {
    if let Some(dir) = local_app_data_dir() {
        return dir.join("Lucy").join(EMBED_CACHE_FILE);
    }
    std::env::temp_dir().join(EMBED_CACHE_FILE)
}

fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 { for x in &mut v { *x /= norm; } }
    v
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

async fn load_cache_from_disk() -> Option<Vec<SkillVector>> {
    let path = cache_path();
    let text = tokio::fs::read_to_string(&path).await.ok()?;
    serde_json::from_str::<Vec<SkillVector>>(&text).ok()
}

async fn persist_cache_to_disk(vecs: &[SkillVector]) -> Result<(), String> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let json = serde_json::to_string(vecs)
        .map_err(|e| format!("json serialize: {}", e))?;
    tokio::fs::write(&path, json.as_bytes()).await
        .map_err(|e| format!("write cache: {}", e))?;
    Ok(())
}

/// Build the embedding cache from scratch. Calls Ollama once per skill
/// with concurrency limited to keep the server responsive. Idempotent:
/// running it twice produces the same cache.
async fn build_embed_cache() -> Result<Vec<SkillVector>, String> {
    let index = load_index().clone();
    let mut out: Vec<SkillVector> = Vec::with_capacity(index.len());
    for meta in &index {
        // Compose the text we embed. name+description+top tags is the
        // sweet spot: enough signal to capture topic, not so much that
        // off-topic tail content drowns the centroid.
        let tags = meta.tags.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
        let text = format!("{}. {}. Tags: {}", meta.name, meta.description, tags);
        match crate::commands::embeddings::embed_via_ollama_pub(&text, None).await {
            Ok((v, model)) => {
                out.push(SkillVector {
                    id: meta.id.clone(),
                    vec: normalize(v),
                    model,
                });
            }
            Err(e) => {
                // Don't fail the whole build — log and continue. Skills
                // missing from the cache will simply be ignored by Tier
                // 2 (they remain reachable via keyword Tier 1).
                eprintln!("[security_skills] embed '{}' failed: {}", meta.id, e);
            }
        }
    }
    let _ = persist_cache_to_disk(&out).await;
    Ok(out)
}

/// Get the cache, building it lazily if absent. Disk → memory → build.
async fn cache_get_or_build() -> Result<Vec<SkillVector>, String> {
    {
        let r = EMBED_CACHE.read().await;
        if let Some(c) = r.as_ref() { return Ok(c.clone()); }
    }
    if let Some(disk) = load_cache_from_disk().await {
        let mut w = EMBED_CACHE.write().await;
        *w = Some(disk.clone());
        return Ok(disk);
    }
    let built = build_embed_cache().await?;
    let mut w = EMBED_CACHE.write().await;
    *w = Some(built.clone());
    Ok(built)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoRouteResult {
    /// `"keyword"` | `"embedding"` | `"ambiguous"` | `"none"`.
    pub method: String,
    /// Top hit if method != "none". Otherwise None.
    pub top: Option<SkillSearchHit>,
    /// Top-5 candidates when method == "ambiguous" — caller (frontend)
    /// can ask CHEAP tier to disambiguate.
    pub candidates: Vec<SkillSearchHit>,
    /// Diagnostic — was embedding cache ready? Useful for the chip UI
    /// to say "fallback used" when Ollama is offline.
    pub embeddings_available: bool,
}

/// Top-level auto-router. Single Tauri command the frontend invokes
/// once per turn. Always returns Ok — failure modes are folded into
/// `method: "none"` so the caller never has to handle errors.
#[tauri::command]
pub async fn security_skills_auto_route(user_prompt: String) -> Result<AutoRouteResult, String> {
    let prompt = user_prompt.trim();
    if prompt.is_empty() {
        return Ok(AutoRouteResult {
            method: "none".into(),
            top: None,
            candidates: vec![],
            embeddings_available: false,
        });
    }

    // ── Tier 1 — keyword search ──────────────────────────────────────
    let kw_hits = {
        let q  = prompt.to_lowercase();
        let qt = tokenize(prompt);
        let mut hits: Vec<SkillSearchHit> = load_index().iter()
            .filter_map(|m| {
                let s = score_skill(m, &q, &qt);
                if s == 0 { return None; }
                Some(SkillSearchHit { meta: m.clone(), score: s, preview: preview(&m.description, 240) })
            })
            .collect();
        hits.sort_by(|a, b| b.score.cmp(&a.score));
        hits
    };
    if let Some(top) = kw_hits.first() {
        if top.score >= KEYWORD_TIER1_THRESHOLD {
            return Ok(AutoRouteResult {
                method: "keyword".into(),
                top: Some(top.clone()),
                candidates: kw_hits.iter().take(5).cloned().collect(),
                embeddings_available: false,
            });
        }
    }

    // ── Tier 2 — embedding cosine ────────────────────────────────────
    let cache = cache_get_or_build().await;
    let (q_vec, embeddings_available) = match cache {
        Ok(c) if !c.is_empty() => {
            match crate::commands::embeddings::embed_via_ollama_pub(prompt, None).await {
                Ok((v, _)) => (Some((normalize(v), c)), true),
                Err(_) => (None, false),
            }
        }
        _ => (None, false),
    };

    let mut emb_ranked: Vec<(f32, SkillSearchHit)> = Vec::new();
    if let Some((qv, cache)) = q_vec {
        let index = load_index();
        for sv in &cache {
            let cos = dot(&qv, &sv.vec);
            if cos < EMBED_TIER3_FLOOR { continue; }
            if let Some(meta) = index.iter().find(|m| m.id == sv.id) {
                emb_ranked.push((cos, SkillSearchHit {
                    meta: meta.clone(),
                    score: (cos * 100.0) as i32,
                    preview: preview(&meta.description, 240),
                }));
            }
        }
        emb_ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    }

    if let Some((top_cos, top_hit)) = emb_ranked.first() {
        if *top_cos >= EMBED_TIER2_THRESHOLD {
            return Ok(AutoRouteResult {
                method: "embedding".into(),
                top: Some(top_hit.clone()),
                candidates: emb_ranked.iter().take(5).map(|(_, h)| h.clone()).collect(),
                embeddings_available: true,
            });
        }

        // ── Tier 2.5 — RRF fusion of keyword + embedding rankings ──────
        //
        // Cherry-pick from rohitg00/agentmemory's triple-stream retrieval.
        // When NEITHER keyword nor embedding crossed its individual
        // threshold but BOTH produced candidates, fusing the two
        // rankings via Reciprocal Rank Fusion frequently surfaces the
        // correct skill — the one that ranks well in BOTH streams even
        // if it dominates neither.
        //
        // RRF score per skill_id:  score = Σ 1 / (k + rank_i)
        // where rank_i is its 1-indexed rank in each ranking it appears
        // in, and k = 60 (Cormack et al. 2009 standard).
        //
        // We accept a fused top if it has BOTH:
        //   • Appeared in both rankings (≥2 streams)
        //   • Its keyword score is ≥ 25% of TIER1 threshold (so we don't
        //     fuse pure noise) OR its embedding cosine is ≥ TIER3 floor.
        const RRF_K: f64 = 60.0;
        const FUSED_MIN_RRF_SCORE: f64 = 0.025;  // both streams at rank ≤ 5
        let mut fused: std::collections::HashMap<String, (f64, u8)> = std::collections::HashMap::new();
        for (rank, h) in kw_hits.iter().enumerate() {
            let entry = fused.entry(h.meta.id.clone()).or_insert((0.0, 0));
            entry.0 += 1.0 / (RRF_K + (rank as f64 + 1.0));
            entry.1 |= 0b01;   // stream 1: keyword
        }
        for (rank, (_, h)) in emb_ranked.iter().enumerate() {
            let entry = fused.entry(h.meta.id.clone()).or_insert((0.0, 0));
            entry.0 += 1.0 / (RRF_K + (rank as f64 + 1.0));
            entry.1 |= 0b10;   // stream 2: embedding
        }
        // Keep only candidates present in BOTH streams (mask == 0b11).
        let mut both: Vec<(String, f64)> = fused.into_iter()
            .filter(|(_, (_, mask))| *mask == 0b11)
            .map(|(id, (s, _))| (id, s))
            .collect();
        both.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((fused_id, fused_score)) = both.first() {
            if *fused_score >= FUSED_MIN_RRF_SCORE {
                // Pull the SkillSearchHit from either ranking (richer
                // metadata available on the embedding side; fall back to
                // keyword).
                let resolved = emb_ranked.iter().find(|(_, h)| &h.meta.id == fused_id).map(|(_, h)| h.clone())
                    .or_else(|| kw_hits.iter().find(|h| &h.meta.id == fused_id).cloned());
                if let Some(top_hit) = resolved {
                    // Build the candidate list as the top-5 of the fused
                    // ranking, resolving each to its richest SkillSearchHit.
                    let mut fused_cands: Vec<SkillSearchHit> = Vec::new();
                    for (id, _) in both.iter().take(5) {
                        if let Some(h) = emb_ranked.iter().find(|(_, h)| &h.meta.id == id).map(|(_, h)| h.clone())
                            .or_else(|| kw_hits.iter().find(|h| &h.meta.id == id).cloned()) {
                            fused_cands.push(h);
                        }
                    }
                    return Ok(AutoRouteResult {
                        method: "fused".into(),
                        top: Some(top_hit),
                        candidates: fused_cands,
                        embeddings_available: true,
                    });
                }
            }
        }

        // Tier 3 zone — surface candidates for caller-side disambiguation.
        return Ok(AutoRouteResult {
            method: "ambiguous".into(),
            top: None,
            candidates: emb_ranked.iter().take(5).map(|(_, h)| h.clone()).collect(),
            embeddings_available: true,
        });
    }

    // No clear winner anywhere. Surface the top keyword hits (if any)
    // as candidates so the caller can still show them.
    Ok(AutoRouteResult {
        method: "none".into(),
        top: None,
        candidates: kw_hits.into_iter().take(5).collect(),
        embeddings_available,
    })
}

/// Force a rebuild of the embedding cache. Useful after editing skill
/// frontmatter or switching embedding models. Returns the new vector
/// count.
#[tauri::command]
pub async fn security_skills_rebuild_embeddings() -> Result<usize, String> {
    // Clear the in-memory cache so subsequent reads pick up the rebuild.
    {
        let mut w = EMBED_CACHE.write().await;
        *w = None;
    }
    let built = build_embed_cache().await?;
    let n = built.len();
    let mut w = EMBED_CACHE.write().await;
    *w = Some(built);
    Ok(n)
}

/// Diagnostic — return cache state without rebuilding. Used by the
/// settings panel to show "embeddings: 213 / 213 cached · 768-dim".
#[tauri::command]
pub async fn security_skills_embed_status() -> Result<serde_json::Value, String> {
    let r = EMBED_CACHE.read().await;
    let in_mem = r.as_ref().map(|c| c.len()).unwrap_or(0);
    let on_disk = cache_path().exists();
    let total = load_index().len();
    Ok(serde_json::json!({
        "in_memory":    in_mem,
        "on_disk":      on_disk,
        "skill_total":  total,
        "cache_path":   cache_path().to_string_lossy(),
    }))
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

// ── v1.7.15 — User skills directory ────────────────────────────────────
//
// `%LOCALAPPDATA%\Lucy\security-skills\` (Windows) or
// `$XDG_DATA_HOME/Lucy/security-skills` (Linux). Users drop
// `<skill-id>/SKILL.md` files here at runtime, no recompile.
// Commands here drive the slash-command and drag-drop UIs.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSkillsDirInfo {
    pub path:      String,
    /// Whether the directory existed before this call (vs created now).
    pub created:   bool,
    /// Count of `<id>/SKILL.md` entries under it.
    pub n_skills:  usize,
}

/// Return the resolved user skills directory, creating it if needed.
/// Frontend uses this for the "Open folder" button and for the drag-
/// drop installer to know where to write the file.
#[tauri::command]
pub async fn security_skills_user_dir() -> Result<UserSkillsDirInfo, String> {
    let p = user_skills_dir_path();
    let existed = p.exists();
    if !existed {
        std::fs::create_dir_all(&p)
            .map_err(|e| format!("create user skills dir: {}", e))?;
    }
    let n_skills = std::fs::read_dir(&p)
        .map(|it| it.flatten()
                     .filter(|e| e.path().join("SKILL.md").exists())
                     .count())
        .unwrap_or(0);
    Ok(UserSkillsDirInfo {
        path: p.to_string_lossy().to_string(),
        created: !existed,
        n_skills,
    })
}

/// Drop the cached index AND the embedding cache so the next read
/// re-walks both skill directories. Returns the new total count.
/// Triggered by `/sec-skill reload` and the drag-drop installer
/// after writing a new SKILL.md.
#[tauri::command]
pub async fn security_skills_reload() -> Result<usize, String> {
    if let Ok(mut w) = INDEX.write() {
        *w = None;
    }
    {
        let mut w = EMBED_CACHE.write().await;
        *w = None;
    }
    // Best-effort delete the on-disk embedding cache so the next probe
    // rebuilds against the new skill set. Without this, projections
    // would silently use stale embeddings for renamed/deleted skills.
    let _ = tokio::fs::remove_file(cache_path()).await;
    let n = load_index().len();
    Ok(n)
}

/// Return a starter SKILL.md template the user can edit. Pre-fills the
/// frontmatter with sensible defaults derived from the requested id.
/// The template demonstrates ALL the fields the parser recognises so
/// users learn the schema by example.
#[tauri::command]
pub async fn security_skills_template(id: String) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("template id must not be empty".into());
    }
    // Permissive id check: kebab-case, no slashes, no weird chars.
    if id.chars().any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_') {
        return Err(format!(
            "id '{}' contains invalid chars (use kebab-case: my-skill-name)", id
        ));
    }
    Ok(format!(
r#"---
name: {id}
description: 'One-line summary of when this skill applies. Activates for requests
  involving <keywords matching the situation>.'
domain: cybersecurity
subdomain: incident-response
tags:
  - your-tag-1
  - your-tag-2
mitre_attack:
  - T1071
nist_csf:
  - RS.AN-01
version: 1.0.0
author: ivan
license: Apache-2.0
---

# {id}

## When to Use

- List 3-5 concrete trigger phrases / situations where this skill is the right
  reference. Lucy's auto-router scores the user prompt against these (plus the
  description and tags) when deciding whether to load this skill.

## Prerequisites

- Modules / sessions / roles needed BEFORE any command in this workflow runs.
- Be explicit; Lucy refuses to execute when a prerequisite is missing.

## Workflow

### Step 1: <descriptive step name>

Plain-prose explanation of what this step accomplishes. Example commands go
below as code fences — they're treated as REFERENCE, not auto-run. Use
placeholders that look like placeholders so Lucy's guard catches them.

```powershell
Get-EventLog -LogName Security -Newest 100 | Where-Object {{ $_.EventID -eq 4625 }}
```

### Step 2: <next step>

Explanation.

```powershell
# Example with explicit placeholder values
Connect-ExchangeOnline -UserPrincipalName admin@tudominio.com
```

## Key Concepts

| Concept | Description |
|---------|-------------|
| ... | ... |

## Tools & Systems

| Tool | Purpose |
|------|---------|
| ... | ... |

## Common Scenarios

**Scenario 1: <name>**
Brief walkthrough.

## Output Format

```
What the deliverable looks like when this workflow completes.
```
"#))
}

/// Install a SKILL.md file into the user dir. Returns the resulting
/// skill id (parsed from the frontmatter `name:` field) so the caller
/// can immediately activate / search for it.
///
/// Validations:
///   - The file must start with `---` frontmatter.
///   - The frontmatter must contain `name:` matching the id rules.
///   - The id can be overridden by the caller, but falls back to the
///     frontmatter `name:` field.
///   - The file is written to `<user_dir>/<id>/SKILL.md`, overwriting
///     a previous version with the same id.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillInstallRequest {
    /// Raw SKILL.md content including frontmatter.
    pub content:     String,
    /// Optional override of the id. When omitted, we use the `name:`
    /// from the frontmatter.
    pub id_override: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillInstallResult {
    pub id:      String,
    /// Final on-disk path of the SKILL.md file.
    pub path:    String,
    /// `"installed"` for a fresh skill, `"updated"` when overwriting an
    /// existing user-dir file with the same id.
    pub action:  String,
    pub n_skills_total: usize,
}

#[tauri::command]
pub async fn security_skills_install(req: SkillInstallRequest) -> Result<SkillInstallResult, String> {
    let trimmed = req.content.trim_start();
    if !trimmed.starts_with("---") {
        return Err("SKILL.md must start with YAML frontmatter (`---`)".into());
    }
    // Parse to extract metadata + sanity-check the schema.
    let meta = parse_frontmatter("(pending)", trimmed)
        .ok_or_else(|| "SKILL.md frontmatter could not be parsed".to_string())?;

    let id = req.id_override
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| meta.name.trim().to_string());
    if id.is_empty() {
        return Err("could not derive an id (frontmatter `name:` empty and no id_override)".into());
    }
    if id.chars().any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_') {
        return Err(format!(
            "id '{}' contains invalid chars (use kebab-case)", id
        ));
    }

    // Write to disk.
    let dir = ensure_user_skills_dir()
        .map_err(|e| format!("ensure user dir: {}", e))?
        .join(&id);
    let existed = dir.join("SKILL.md").exists();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create skill dir: {}", e))?;
    let path = dir.join("SKILL.md");
    std::fs::write(&path, &req.content)
        .map_err(|e| format!("write SKILL.md: {}", e))?;

    // Invalidate the in-memory and embedding caches so the new skill
    // shows up on the next search/route call without restart.
    if let Ok(mut w) = INDEX.write() { *w = None; }
    {
        let mut w = EMBED_CACHE.write().await;
        *w = None;
    }
    let _ = tokio::fs::remove_file(cache_path()).await;
    let n = load_index().len();

    Ok(SkillInstallResult {
        id,
        path: path.to_string_lossy().to_string(),
        action: if existed { "updated".into() } else { "installed".into() },
        n_skills_total: n,
    })
}

/// v1.7.168 — Delete a USER skill from the user skills directory
/// (`%LOCALAPPDATA%\Lucy\security-skills\<id>`). Bundled skills shipped with
/// Lucy are READ-ONLY and cannot be deleted — the command refuses if the id
/// doesn't exist as a user-dir folder. The id is validated to kebab-case so
/// it can never traverse outside the user dir (no `/`, `\`, or `..`).
/// Invalidates the index + embedding caches and returns the new total count.
#[tauri::command]
pub async fn security_skills_delete(id: String) -> Result<usize, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("empty id".into());
    }
    if id.chars().any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_') {
        return Err(format!("id '{}' contains invalid chars — refusing to delete", id));
    }
    let dir = user_skills_dir_path().join(id);
    if !dir.exists() {
        return Err(format!(
            "'{}' is not a user skill — bundled skills are read-only and can't be deleted",
            id
        ));
    }
    std::fs::remove_dir_all(&dir)
        .map_err(|e| format!("delete skill dir: {}", e))?;

    // Invalidate the in-memory + embedding caches so the deleted skill drops
    // out of the index on the next read (mirrors install/reload).
    if let Ok(mut w) = INDEX.write() { *w = None; }
    {
        let mut w = EMBED_CACHE.write().await;
        *w = None;
    }
    let _ = tokio::fs::remove_file(cache_path()).await;
    let n = load_index().len();
    Ok(n)
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
            source: "test".into(),
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
            source: "test".into(),
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
            source: "test".into(),
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
