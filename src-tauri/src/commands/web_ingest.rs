// ── WEB INGESTION — read a URL into Lucy's searchable memory ─────────────────
//
// Lucy could already SEARCH the web (`search_web`: Tavily, DuckDuckGo as
// fallback) but not READ it. That tool returns snippets, uses them for one
// turn, and forgets them: the same page gets re-fetched, re-summarised and
// re-paid for in every session that needs it, and nothing it contained is ever
// recallable afterwards.
//
// PDF ingestion already had the whole pipeline — extract, chunk, embed, search
// with RRF fusion and a reranker. The only piece missing for the web was the
// front of it: fetch a URL and turn HTML into text worth embedding. That is
// what this module is. Everything downstream is shared, deliberately: the two
// corpora answer the same queries through the same index, so a difference in
// how they are chunked would make relevance depend on where the text came from.
//
// Storage reuses `pdf_documents` with `kind = 'web'`. The table name is
// historical — it is the ingested-documents table — and reusing it means web
// pages inherit the documents list, the embedded/total counter, deletion, the
// coverage diagnostic and the re-embed repair without a second copy of any of
// them. Chunks are their own `entity_type` (`web_chunk`) because that IS a real
// distinction: it lets a search scope to one or the other, and keeps reference
// material out of the pre-loop memory recall.

use crate::commands::embeddings::{embed_and_store, embed_and_store_batch};
use crate::commands::metrics::shared_db;
use crate::commands::pdf::{chunk_structured, collect_headings, PdfProgress};
use crate::utils::db::generate_id;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

const CHUNK_SIZE: usize = 2_500;
const CHUNK_OVERLAP: usize = 200;

/// Ceiling on the HTML we will pull down, before any parsing.
///
/// Checked against the bytes actually received rather than `Content-Length`,
/// which a server is free to lie about or omit. 8 MB is far past any real
/// article and far short of anything that threatens memory.
const MAX_HTML_BYTES: usize = 8 * 1024 * 1024;

/// Below this, extraction is treated as having failed rather than succeeded
/// with little to say. A JS-rendered page returns a shell of navigation and
/// nothing else, and silently ingesting 200 characters of menu labels is worse
/// than refusing: it produces a document that exists, looks ingested, and
/// answers every question about the page with noise.
const MIN_USEFUL_CHARS: usize = 250;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebIngestResult {
    pub doc_id: String,
    pub title: String,
    pub url: String,
    pub chunk_count: u32,
    pub total_chars: usize,
}

// ── The reading engine ───────────────────────────────────────────────────────
//
// A tag-stripper, not a browser. It does not run JavaScript, so a page that
// renders its content client-side yields the shell — which is why extraction
// has a floor and reports failure instead of ingesting a menu.
//
// No HTML parser crate is used. This is deliberate: the output is text destined
// for an embedding model, not a DOM anyone will query, and the failure mode of
// a hand-rolled stripper on weird markup is "some stray text survives", not a
// crash or a wrong answer. A real parser would be the right call the moment
// anything needs structure back.

/// Decode the HTML entities that actually appear in prose. Not the full set —
/// the numeric forms plus the handful that carry meaning in text.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let decoded = match ent {
            "amp" => Some("&".to_string()),
            "lt" => Some("<".to_string()),
            "gt" => Some(">".to_string()),
            "quot" => Some("\"".to_string()),
            "apos" | "#39" => Some("'".to_string()),
            "nbsp" | "#160" => Some(" ".to_string()),
            "mdash" => Some("—".to_string()),
            "ndash" => Some("–".to_string()),
            "hellip" => Some("…".to_string()),
            _ => ent
                .strip_prefix('#')
                .and_then(|n| {
                    let cp = if let Some(hex) = n.strip_prefix('x').or_else(|| n.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        n.parse::<u32>().ok()
                    }?;
                    char::from_u32(cp).map(|c| c.to_string())
                }),
        };
        match decoded {
            Some(d) => {
                out.push_str(&d);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Remove `<tag>…</tag>` and everything between, for tags whose CONTENT is not
/// prose. Case-insensitive, tolerant of attributes and of a missing close tag
/// (in which case the rest of the document is dropped — the same thing a
/// browser does with an unclosed `<script>`).
fn strip_element(html: &str, tag: &str) -> String {
    let lower = html.to_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    let mut out = String::with_capacity(html.len());
    let mut i = 0usize;
    while let Some(rel) = lower[i..].find(&open) {
        let start = i + rel;
        // `<script` must not match `<scriptish`; the next char has to end the name.
        let after = lower[start + open.len()..].chars().next();
        if !matches!(after, Some(c) if c == '>' || c.is_whitespace() || c == '/') {
            out.push_str(&html[i..start + open.len()]);
            i = start + open.len();
            continue;
        }
        out.push_str(&html[i..start]);
        match lower[start..].find(&close) {
            Some(rel_end) => {
                let after_close = start + rel_end;
                i = lower[after_close..]
                    .find('>')
                    .map(|g| after_close + g + 1)
                    .unwrap_or(html.len());
            }
            None => return out, // unclosed — drop the remainder
        }
    }
    out.push_str(&html[i..]);
    out
}

/// The narrowest element that plausibly holds the article, or the whole
/// document when there is no such wrapper.
fn main_content(html: &str) -> &str {
    let lower = html.to_lowercase();
    for tag in ["<article", "<main", "<body"] {
        if let Some(start) = lower.find(tag) {
            let close = format!("</{}", &tag[1..]);
            if let Some(end) = lower.rfind(&close) {
                if end > start {
                    return &html[start..end];
                }
            }
        }
    }
    html
}

/// Read a fetched HTML document into `(title, text)`.
///
/// Headings come out as markdown (`## …`) rather than bare lines, because
/// `chunk_structured` splits on them — so the chunk boundaries follow the
/// page's own sections instead of falling wherever 2500 characters happen to
/// land. That is the single biggest lever on retrieval quality here, and it
/// costs one substitution.
pub(crate) fn html_to_text(html: &str) -> (String, String) {
    let title = {
        let lower = html.to_lowercase();
        lower
            .find("<title")
            .and_then(|s| html[s..].find('>').map(|g| s + g + 1))
            .and_then(|start| lower[start..].find("</title").map(|e| (start, start + e)))
            .map(|(a, b)| decode_entities(html[a..b].trim()).trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_default()
    };

    let body = main_content(html);
    // Order matters: kill non-prose CONTENT before the generic tag strip, or
    // the script bodies survive as text.
    let mut s = body.to_string();
    for tag in [
        "script", "style", "noscript", "svg", "template", "iframe", "canvas", "form", "select",
        "nav", "header", "footer", "aside",
    ] {
        s = strip_element(&s, tag);
    }
    // HTML comments — including conditional ones, which is why this is not a
    // tag strip.
    while let Some(a) = s.find("<!--") {
        match s[a..].find("-->") {
            Some(b) => s.replace_range(a..a + b + 3, ""),
            None => {
                s.truncate(a);
                break;
            }
        }
    }

    // Block-level tags become newlines so sentences do not weld together;
    // headings become markdown so the chunker can see the structure.
    let lower = s.to_lowercase();
    let mut marked = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        let Some(rel) = s[i..].find('<') else {
            marked.push_str(&s[i..]);
            break;
        };
        let start = i + rel;
        marked.push_str(&s[i..start]);
        let Some(rel_end) = s[start..].find('>') else {
            break; // truncated tag at EOF
        };
        let end = start + rel_end + 1;
        let name: String = lower[start + 1..end - 1]
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        let closing = lower[start..].starts_with("</");
        match name.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if closing {
                    marked.push('\n');
                } else {
                    let level = name[1..].parse::<usize>().unwrap_or(2).clamp(1, 6);
                    marked.push_str(&format!("\n\n{} ", "#".repeat(level)));
                }
            }
            "li" if !closing => marked.push_str("\n- "),
            "br" => marked.push('\n'),
            "p" | "div" | "tr" | "section" | "article" | "blockquote" | "pre" | "ul" | "ol"
            | "table" | "figcaption" => marked.push('\n'),
            "td" | "th" => marked.push('\t'),
            _ => {}
        }
        i = end;
    }

    let decoded = decode_entities(&marked);

    // Collapse runs of blank lines and trailing spaces, but keep single
    // newlines: they are the paragraph structure the chunker reads.
    let mut text = String::with_capacity(decoded.len());
    let mut blank_run = 0usize;
    for line in decoded.lines() {
        let t = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                text.push('\n');
            }
        } else {
            blank_run = 0;
            text.push_str(&t);
            text.push('\n');
        }
    }

    (title, text.trim().to_string())
}

// ── Ingestion ────────────────────────────────────────────────────────────────

/// Fetch a URL and ingest its readable text into Lucy's document memory.
#[tauri::command]
pub async fn web_ingest(app: AppHandle, url: String) -> Result<WebIngestResult, String> {
    let url = url.trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("Solo se admiten URLs http:// y https://".into());
    }

    // ── SSRF, on the INITIAL url ─────────────────────────────────────────────
    // HTTP_CLIENT already walks the redirect chain with these same two checks
    // (state.rs::ssrf_safe_redirect_policy). The first hop is not covered by
    // that policy, and it is the one the caller controls directly — and here
    // the caller may be the LLM, acting on a URL it read in a web-search
    // result. The body comes back into the model's context either way, so an
    // unguarded fetch of `169.254.169.254` or `127.0.0.1:11434` hands whatever
    // is there to the next outbound message.
    let scan = crate::guardrails::scan_url(&url);
    if !matches!(scan.decision, crate::guardrails::ScanDecision::Allow) {
        return Err(format!("URL bloqueada por guardrails: {}", scan.reason));
    }
    crate::guardrails::host_resolves_to_internal(&url)
        .map_err(|e| format!("URL bloqueada: el host resuelve a una dirección interna ({})", e))?;

    let doc_id = generate_id();
    let emit = |phase: &str, cur: u32, total: u32, msg: String| {
        let _ = app.emit(
            "pdf_progress",
            PdfProgress { doc_id: doc_id.clone(), current: cur, total, phase: phase.into(), message: msg },
        );
    };
    emit("extracting", 0, 1, format!("Descargando {}…", url));

    // ── Fetch ────────────────────────────────────────────────────────────────
    let resp = crate::state::HTTP_CLIENT
        .get(&url)
        .header("Accept", "text/html,application/xhtml+xml")
        .send()
        .await
        .map_err(|e| format!("No se pudo descargar '{}': {}", url, e))?;

    if !resp.status().is_success() {
        return Err(format!("'{}' respondió {}", url, resp.status()));
    }
    // Refuse non-HTML by content type. A PDF served over http belongs in the
    // PDF path, which has a real extractor; feeding its bytes to a tag stripper
    // produces plausible-looking garbage rather than an error.
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    if !ctype.is_empty() && !(ctype.contains("html") || ctype.contains("xml") || ctype.contains("text/plain")) {
        return Err(format!(
            "'{}' no es una página web (Content-Type: {}). Para un PDF usa la ingesta de PDF.",
            url, ctype
        ));
    }

    let bytes = resp.bytes().await.map_err(|e| format!("Error leyendo '{}': {}", url, e))?;
    if bytes.len() > MAX_HTML_BYTES {
        return Err(format!(
            "'{}' devolvió {} MB, por encima del límite de {} MB",
            url,
            bytes.len() / 1024 / 1024,
            MAX_HTML_BYTES / 1024 / 1024
        ));
    }
    let html = String::from_utf8_lossy(&bytes).to_string();

    // ── Read ─────────────────────────────────────────────────────────────────
    let (title, text) = html_to_text(&html);
    let title = if title.is_empty() { url.clone() } else { title };
    if text.chars().count() < MIN_USEFUL_CHARS {
        return Err(format!(
            "'{}' no expuso texto legible ({} caracteres). Suele significar que la página \
             renderiza su contenido con JavaScript, que esta ingesta no ejecuta.",
            url,
            text.chars().count()
        ));
    }
    let total_chars = text.chars().count();

    // Idempotence: same URL, same content → reuse the existing document rather
    // than growing a second copy that competes with the first in every search.
    let content_hash = format!("{:x}", Sha256::digest(text.as_bytes()));
    let existing: Option<(String, i64)> = shared_db(|conn| {
        Ok(conn
            .query_row(
                "SELECT id, chunk_count FROM pdf_documents
                 WHERE path = ?1 AND content_hash = ?2 AND status = 'done'",
                rusqlite::params![&url, &content_hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok())
    })
    .unwrap_or(None);
    if let Some((id, chunks)) = existing {
        emit("done", 1, 1, "Ya ingerida y sin cambios.".into());
        return Ok(WebIngestResult {
            doc_id: id,
            title,
            url,
            chunk_count: chunks as u32,
            total_chars,
        });
    }

    let chunks = chunk_structured(&text, CHUNK_SIZE, CHUNK_OVERLAP);
    let total = chunks.len() as u32;
    if total == 0 {
        return Err("La página no produjo ninguna sección indexable.".into());
    }

    shared_db(|conn| {
        conn.execute(
            "INSERT INTO pdf_documents (id, filename, path, chunk_count, status, content_hash, kind)
             VALUES (?1, ?2, ?3, ?4, 'ingesting', ?5, 'web')",
            rusqlite::params![doc_id, title, url, total, content_hash],
        )
        .map_err(|e| format!("insert web document: {}", e))?;
        Ok(())
    })?;

    // ── Save chunks ──────────────────────────────────────────────────────────
    let session_id = format!("web:{}", doc_id);
    let mut memory_ids: Vec<i64> = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        let n = (i + 1) as u32;
        let c_title = format!("WEB: {} §{}/{}", title, n, total);
        let tags = format!(
            "[\"web\",\"web:{}\",\"{}\"]",
            doc_id,
            title.replace('"', "'")
        );
        let files = format!("[\"{}\"]", url.replace('\\', "\\\\").replace('"', "\\\""));
        // importance 1, like PDF chunks: reference material must never outrank
        // an organic memory in importance-ordered queries.
        let mid = shared_db(|conn| {
            conn.execute(
                "INSERT INTO agent_memories (session_id, title, content, tags, files, importance)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                rusqlite::params![session_id, c_title, chunk, tags, files],
            )
            .map_err(|e| format!("insert web chunk {}: {}", n, e))?;
            Ok(conn.last_insert_rowid())
        })?;
        memory_ids.push(mid);
        emit("saving", n, total, format!("Guardando sección {}/{}", n, total));
    }

    shared_db(|conn| {
        conn.execute(
            "UPDATE pdf_documents SET status = 'done' WHERE id = ?1",
            rusqlite::params![doc_id],
        )
        .map_err(|e| format!("update web document: {}", e))?;
        Ok(())
    })?;

    // One document-level memory, so the pre-loop recall can surface "this page
    // was read" when the user asks about its topic. session_id 'web-doc:' so it
    // survives the reference-material exclusions the chunks are subject to.
    let toc = collect_headings(&text, 12);
    let summary_mid: Option<i64> = {
        let s_body = format!(
            "Página web ingerida: \"{}\" ({}) — {} secciones, {} caracteres. \
             Para consultar su contenido usa pdf_search (búsqueda semántica) o memoria_buscar (texto).{}",
            title,
            url,
            total,
            total_chars,
            if toc.is_empty() { String::new() } else { format!(" Temas: {}.", toc.join(" · ")) }
        );
        let s_tags = format!("[\"web-doc\",\"web:{}\",\"{}\"]", doc_id, title.replace('"', "'"));
        let s_files = format!("[\"{}\"]", url.replace('\\', "\\\\").replace('"', "\\\""));
        shared_db(|conn| {
            conn.execute(
                "INSERT INTO agent_memories (session_id, title, content, tags, files, importance)
                 VALUES (?1, ?2, ?3, ?4, ?5, 2)",
                rusqlite::params![
                    format!("web-doc:{}", doc_id),
                    format!("Web: {}", title),
                    s_body,
                    s_tags,
                    s_files
                ],
            )
            .map_err(|e| format!("web summary insert: {}", e))?;
            Ok(Some(conn.last_insert_rowid()))
        })
        .unwrap_or(None)
    };

    // ── Embed ────────────────────────────────────────────────────────────────
    // Reports what actually landed, and logs a failed batch where an operator
    // can find it. The PDF path shipped for months claiming "ready" regardless
    // (fixed in a0bb315); there is no reason to rebuild that here.
    {
        let app2 = app.clone();
        let doc2 = doc_id.clone();
        let title2 = title.clone();
        let chunks2 = chunks.clone();
        let mids2 = memory_ids.clone();
        tauri::async_runtime::spawn(async move {
            let items: Vec<(String, String)> = chunks2
                .iter()
                .zip(mids2.iter())
                .map(|(c, mid)| (mid.to_string(), format!("[{}] {}", title2, c)))
                .collect();
            let mut done: u32 = 0;
            for batch in items.chunks(16) {
                match embed_and_store_batch("web_chunk", batch, None).await {
                    Ok(n) => done += n,
                    Err(e) => crate::utils::logging::write_app_log(
                        "WARN",
                        &format!(
                            "[web] embed batch failed for '{}' ({} chunks): {} — \
                             recoverable via the Document Embeddings repair",
                            title2,
                            batch.len(),
                            e
                        ),
                    ),
                }
                let _ = app2.emit(
                    "pdf_progress",
                    PdfProgress {
                        doc_id: doc2.clone(),
                        current: done.min(total),
                        total,
                        phase: "embedding".into(),
                        message: format!("Embedding {}/{}", done.min(total), total),
                    },
                );
            }
            if let Some(smid) = summary_mid {
                let _ = embed_and_store(
                    "memory".into(),
                    smid.to_string(),
                    format!("Web: {} — página consultable con pdf_search", title2),
                    None,
                )
                .await;
            }
            let _ = app2.emit(
                "pdf_progress",
                PdfProgress {
                    doc_id: doc2,
                    current: done,
                    total,
                    phase: "done".into(),
                    message: if done >= total {
                        "Página lista. Lucy ya puede responder sobre ella.".into()
                    } else {
                        format!(
                            "Página guardada, pero {}/{} secciones no se pudieron embeber y no son \
                             buscables. Recupéralas en Diagnóstico → Re-embeber documentos.",
                            total - done,
                            total
                        )
                    },
                },
            );
        });
    }

    Ok(WebIngestResult { doc_id, title, url, chunk_count: total, total_chars })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<!doctype html>
<html><head>
  <title>Configurar WinRM &amp; PowerShell Remoting</title>
  <style>body { color: red; }</style>
  <script>var tracker = "no debe aparecer"; if (a < b) { hide(); }</script>
</head>
<body>
  <nav><a href="/">Inicio</a><a href="/docs">Documentaci&oacute;n</a></nav>
  <header>Men&uacute; superior que no es contenido</header>
  <article>
    <h1>Configurar WinRM</h1>
    <p>Habilita el servicio con <code>Enable-PSRemoting</code>.</p>
    <h2>Requisitos previos</h2>
    <ul><li>Windows&nbsp;10 o superior</li><li>Perfil de red privado</li></ul>
    <!-- comentario interno -->
    <p>El puerto por defecto es 5985.</p>
  </article>
  <footer>Copyright 2026</footer>
</body></html>"#;

    // Verified once against a real page rather than only this fixture:
    // en.wikipedia.org/wiki/Windows_Remote_Management, 135 KB of production
    // markup → 7648 characters of prose, no angle brackets and no script
    // fragments surviving, and the article's five sections coming out as `##`
    // headings. Not kept as a test: it would need the network, and pinning a
    // 135 KB fixture to assert what the cases below already assert is cost
    // without coverage.

    #[test]
    fn the_title_comes_out_decoded() {
        let (title, _) = html_to_text(PAGE);
        assert_eq!(title, "Configurar WinRM & PowerShell Remoting");
    }

    #[test]
    fn chrome_and_code_do_not_reach_the_index() {
        // Everything here would otherwise be embedded as if it were prose. The
        // script body is the sharp one: it contains `<` and `>`, so a naive
        // tag-strip leaves fragments of JavaScript in the text, and those
        // fragments then match queries about the page's actual subject.
        let (_, text) = html_to_text(PAGE);
        assert!(!text.contains("no debe aparecer"), "script body leaked: {text}");
        assert!(!text.contains("color: red"), "style leaked: {text}");
        assert!(!text.contains("Inicio"), "nav leaked: {text}");
        assert!(!text.contains("Men"), "header leaked: {text}");
        assert!(!text.contains("Copyright"), "footer leaked: {text}");
        assert!(!text.contains("comentario interno"), "comment leaked: {text}");
    }

    #[test]
    fn prose_survives_with_its_entities_decoded() {
        let (_, text) = html_to_text(PAGE);
        assert!(text.contains("Enable-PSRemoting"), "{text}");
        assert!(text.contains("El puerto por defecto es 5985."), "{text}");
        assert!(text.contains("Windows 10 o superior"), "nbsp not decoded: {text}");
    }

    #[test]
    fn headings_become_markdown_so_the_chunker_can_see_sections() {
        // This is the lever on retrieval quality: `chunk_structured` splits on
        // markdown headings, so with them the chunk boundaries follow the
        // page's own sections instead of landing wherever 2500 characters run
        // out — mid-sentence, mid-topic, across two unrelated subjects.
        let (_, text) = html_to_text(PAGE);
        assert!(text.contains("# Configurar WinRM"), "{text}");
        assert!(text.contains("## Requisitos previos"), "{text}");
    }

    #[test]
    fn adjacent_blocks_do_not_weld_into_one_word() {
        let (_, text) = html_to_text("<body><p>uno</p><p>dos</p><div>tres</div></body>");
        assert!(!text.contains("unodos"), "{text}");
        assert!(!text.contains("dostres"), "{text}");
    }

    #[test]
    fn list_items_keep_their_boundaries() {
        let (_, text) = html_to_text("<body><ul><li>alfa</li><li>beta</li></ul></body>");
        assert!(text.contains("- alfa"), "{text}");
        assert!(text.contains("- beta"), "{text}");
    }

    #[test]
    fn an_unclosed_script_drops_the_rest_instead_of_leaking_it() {
        // What a browser does, and the safe direction: losing text is a visible
        // "not much here" that the MIN_USEFUL_CHARS floor catches, whereas
        // leaking a script body is invisible and pollutes the index.
        let (_, text) = html_to_text("<body><p>antes</p><script>var x = 1; leak_me()");
        assert!(text.contains("antes"), "{text}");
        assert!(!text.contains("leak_me"), "{text}");
    }

    #[test]
    fn a_tag_that_merely_starts_like_script_is_not_eaten() {
        let (_, text) = html_to_text("<body><scriptural>texto válido</scriptural></body>");
        assert!(text.contains("texto válido"), "{text}");
    }

    #[test]
    fn numeric_and_hex_entities_decode() {
        let (_, text) = html_to_text("<body><p>&#67;af&#xE9; &amp; t&eacute;</p></body>");
        assert!(text.contains("Café"), "{text}");
        assert!(text.contains("&"), "{text}");
    }

    #[test]
    fn a_lone_ampersand_is_left_alone() {
        // Prose is full of bare `&` and of `&` followed by words that are not
        // entities. Consuming those would silently delete characters.
        let (_, text) = html_to_text("<body><p>Marks &amp; Spencer, A & B, R&D</p></body>");
        assert!(text.contains("Marks & Spencer"), "{text}");
        assert!(text.contains("A & B"), "{text}");
        assert!(text.contains("R&D"), "{text}");
    }

    #[test]
    fn a_page_without_article_or_main_still_reads() {
        let (_, text) = html_to_text("<html><body><p>solo body</p></body></html>");
        assert_eq!(text, "solo body");
    }

    #[test]
    fn a_javascript_shell_produces_too_little_to_pass_the_floor() {
        // The case the floor exists for: a page whose content is rendered
        // client-side gives back navigation and nothing else. Ingesting it
        // creates a document that looks present and answers with noise.
        let (_, text) = html_to_text(
            "<html><head><title>App</title></head><body><div id=\"root\"></div>\
             <script>ReactDOM.render(<App/>, root)</script></body></html>",
        );
        assert!(
            text.chars().count() < MIN_USEFUL_CHARS,
            "expected a shell below the floor, got {} chars: {text}",
            text.chars().count()
        );
    }
}
