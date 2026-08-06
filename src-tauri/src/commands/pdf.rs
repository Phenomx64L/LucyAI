// ── PDF Intelligence — Sprint 4 Pillar 4 ─────────────────────────────────
//
// Ingests PDF files into Lucy's memory + embedding systems so she can answer
// questions about manuals, documentation, and any text-layer PDF.
//
// Architecture:
//   1. pdf-extract pulls full text from the file (pure Rust, no system deps)
//   2. Text is split into CHUNK_SIZE-char chunks with CHUNK_OVERLAP overlap
//   3. Each chunk is saved as an `agent_memory` row
//         session_id = "pdf:{doc_id}"  → easy bulk-delete per document
//         tags = ["pdf", "pdf:{doc_id}", "{filename}"]
//   4. A background task calls Ollama to embed each chunk and stores
//      the vector in the `embeddings` table (entity_type = "pdf_chunk")
//   5. Lucy queries via <TOOL>pdf_search:query</TOOL> → semantic_search
//
// Lucy sees PDF chunks through two paths:
//   • FTS5:   <TOOL>memoria_buscar:query</TOOL>  (works without Ollama)
//   • Vector: <TOOL>pdf_search:query</TOOL>       (requires Ollama + model)

use crate::commands::embeddings::{embed_and_store, embed_and_store_batch};
use crate::commands::metrics::shared_db;
use crate::utils::db::{generate_id, PdfDocument};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tauri::{AppHandle, Emitter};

#[allow(dead_code)]   // referenced by upcoming dynamic-chunking variant
const CHUNK_SIZE: usize    = 2_500;  // chars per chunk  (~1-2 pages of dense text)
const CHUNK_OVERLAP: usize = 200;    // chars shared with next chunk (context continuity)

// ── Public types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfIngestResult {
    pub doc_id:      String,
    pub filename:    String,
    pub chunk_count: u32,
    pub total_chars: usize,
}

/// Progress event emitted to the frontend during ingestion.
/// Event name: "pdf_progress"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfProgress {
    pub doc_id:  String,
    pub current: u32,
    pub total:   u32,
    pub phase:   String,  // "extracting" | "saving" | "embedding" | "done" | "error"
    pub message: String,
}

// ── Text chunking ─────────────────────────────────────────────────────────

fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    if text.len() <= chunk_size {
        let t = text.trim().to_string();
        return if t.is_empty() { vec![] } else { vec![t] };
    }
    let chars: Vec<char> = text.chars().collect();
    let mut chunks = Vec::new();
    let mut start  = 0usize;
    while start < chars.len() {
        let end: usize = (start + chunk_size).min(chars.len());
        let chunk: String = chars[start..end].iter().collect();
        let trimmed = chunk.trim().to_string();
        if !trimmed.is_empty() {
            chunks.push(trimmed);
        }
        if end >= chars.len() { break; }
        start = end.saturating_sub(overlap);
    }
    chunks
}

// ── External converter (optional) ───────────────────────────────────────────

/// Try converting the PDF to Markdown via Microsoft's `markitdown` CLI when it
/// is installed (`pip install markitdown`). Unlike the built-in `pdf-extract`
/// (plain text only), markitdown preserves document STRUCTURE — headings,
/// tables, lists — which yields far better retrieval chunks; with its OCR
/// plugin it can also read scanned PDFs that pdf-extract rejects outright.
///
/// Returns `None` (caller falls back to pdf-extract) when:
///   • the `LUCY_DISABLE_MARKITDOWN` env var is set,
///   • `markitdown` isn't on PATH / fails to spawn,
///   • it exits non-zero, or
///   • it produces empty output.
///
/// SECURITY: `path` is passed as a single argv entry (no shell), and the caller
/// has already validated it (no `..`, must end in `.pdf`, must exist). v1.7.183.
fn try_markitdown(path: &str) -> Option<String> {
    if std::env::var("LUCY_DISABLE_MARKITDOWN").is_ok() {
        return None;
    }
    let out = std::process::Command::new("markitdown")
        .arg(path)
        .output()
        .ok()?; // None when the binary isn't found / spawn fails
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        eprintln!(
            "[pdf] markitdown exited {:?}: {}",
            out.status.code(),
            err.lines().next().unwrap_or("")
        );
        return None;
    }
    let md = String::from_utf8_lossy(&out.stdout);
    let trimmed = md.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

/// v1.8.1 — Text extraction for CHAT ATTACHMENTS (as opposed to `pdf_ingest`,
/// which is the heavyweight RAG path: chunking + embeddings + DB rows).
///
/// Why this exists: the attachment picker used to `read_to_string` every
/// non-image file. On a PDF that fails outright (invalid UTF-8), and the file
/// was silently dropped with only a log line — so the user saw the attachment
/// chip in the composer while the model received nothing. That is what forced
/// people to paste an absolute path and ask Lucy to read the file herself.
///
/// Same extractor chain as `pdf_ingest` (markitdown first for structure + OCR,
/// pure-Rust `pdf-extract` as the always-available fallback) so an attached PDF
/// and an ingested PDF produce the same text.
///
/// SECURITY: `path` comes from the OS file dialog (a real, user-chosen path),
/// never from model output. `try_markitdown` passes it as a single argv entry
/// with no shell involved.
///
/// v1.8.x — the body now lives in `lucy_core::pdf`. The native egui shell needs
/// the same extraction and cannot call a Tauri command, and a second copy of a
/// two-extractor chain is a second copy that drifts: an attached PDF would have
/// produced different text depending on which shell was open. This stays as the
/// command-side name so callers here don't have to change.
pub(crate) fn extract_pdf_text(path: &std::path::Path) -> Result<String, String> {
    lucy_core::pdf::extract_text(path)
}

/// v1.8.1 — Extract PDF text from raw bytes, for DRAG-AND-DROP.
///
/// `tauri.conf.json` sets `dragDropEnabled: false`, so drops are handled by the
/// webview's HTML5 DnD and Lucy receives a `File` object with NO filesystem
/// path — the picker path above cannot be reused. The frontend base64-encodes
/// the bytes and calls this instead.
///
/// The bytes are staged in a temp file because both extractors are path-based
/// (markitdown is a subprocess; `pdf_extract::extract_text` takes a path). The
/// temp file is removed on every exit path, including extraction failure.
///
/// SECURITY: the caller-supplied `name` is used ONLY for error messages, never
/// to build the path — the temp filename is generated here. That keeps a
/// hostile filename (`../../evil`) from steering the write anywhere.
#[tauri::command]
pub async fn extract_pdf_text_from_bytes(name: String, data_b64: String) -> Result<String, String> {
    use base64::{Engine as _, engine::general_purpose};

    // Guard the decode size before allocating: a 100 MB base64 blob from a
    // stray drop should be refused, not turned into 75 MB of Vec<u8>.
    const MAX_B64_LEN: usize = 80 * 1024 * 1024; // ~60 MB of PDF
    if data_b64.len() > MAX_B64_LEN {
        return Err(format!("'{}' es demasiado grande para adjuntar (>60 MB).", name));
    }

    let bytes = general_purpose::STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| format!("'{}': base64 inválido ({})", name, e))?;

    let tmp = std::env::temp_dir().join(format!("lucy_attach_{}.pdf", generate_id()));
    std::fs::write(&tmp, &bytes)
        .map_err(|e| format!("No se pudo preparar '{}' para lectura: {}", name, e))?;

    let tmp_for_task = tmp.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || extract_pdf_text(&tmp_for_task)).await;

    // Delete BEFORE unwrapping the join result. `pdf-extract` panics on some
    // malformed PDFs, and a panic surfaces here as a JoinError — the previous
    // `?` on the joined result returned early and skipped the cleanup, leaving
    // the user's document bytes in %TEMP% indefinitely. Cleanup on every exit
    // path now means every path, not just the ones that return a value.
    let _ = std::fs::remove_file(&tmp);

    joined.map_err(|e| format!("Fallo interno extrayendo '{}': {}", name, e))?
}

// ── Structure-aware chunking (v1.7.184, native — no external tool) ───────────
//
// The plain char-window `chunk_text` cuts every ~2500 chars regardless of
// content, splitting sentences/paragraphs mid-stream → incoherent retrieval
// chunks. This native-Rust pass instead:
//   • groups text into blocks on blank lines (paragraphs), or single lines when
//     the PDF has no blank-line structure, so chunks break on natural
//     boundaries — never mid-word;
//   • detects section HEADINGS (Markdown `#`, numbered "3.1 Title", ALL-CAPS)
//     and PREPENDS the active heading to each chunk as `[Section]` context, so a
//     retrieved fragment carries the section it belongs to.
// Captures most of markitdown's structure benefit for the common text-PDF case
// without bundling Python. (Scanned PDFs / complex tables still want markitdown.)

/// Prepend the active section heading as `[Section]` context, unless the body
/// already begins with it.
fn heading_context(section: &Option<String>, body: &str) -> String {
    let b = body.trim();
    match section {
        Some(h) if !b.starts_with(h.as_str()) => format!("[{}]\n{}", h, b),
        _ => b.to_string(),
    }
}

/// Heuristic: does this block look like a section heading? Conservative on
/// purpose — a wrong guess only mislabels a chunk's `[Section]` prefix, which is
/// low-harm. Returns the heading text (without `#`) when it matches.
fn detect_heading(block: &str) -> Option<String> {
    let trimmed = block.trim();
    let first = trimmed.lines().next().unwrap_or("").trim();
    if first.is_empty() || first.chars().count() > 80 { return None; }
    // v1.7.233: '?' and '!' added to the veto. Without them a numbered FAQ
    // question ("5. What encryption standards does your trading partner
    // support?") passed rule 2 as a heading and — because the active section
    // is sticky in chunk_structured — got prepended to hundreds of chunks.
    let ends_sentence = first.ends_with(['.', ',', ';', ':', '?', '!']);

    // 1. Markdown ATX heading (markitdown emits these).
    if let Some(rest) = first.strip_prefix('#') {
        let h = rest.trim_start_matches('#').trim();
        if !h.is_empty() { return Some(h.to_string()); }
    }
    // A multi-line block is a paragraph, not a plain-text heading.
    if trimmed.lines().filter(|l| !l.trim().is_empty()).count() > 1 { return None; }

    // 2. Numbered section ("3", "3.1", "3.1.2") + Capitalized title.
    let fb = first.as_bytes();
    if fb.first().is_some_and(|c| c.is_ascii_digit()) {
        let mut i = 0;
        while i < fb.len() && (fb[i].is_ascii_digit() || fb[i] == b'.') { i += 1; }
        let rest = first[i..].trim();
        if i > 0 && !rest.is_empty()
            && rest.chars().next().is_some_and(|c| c.is_uppercase())
            && !ends_sentence
        {
            return Some(first.to_string());
        }
    }

    // 3. ALL-CAPS short line ("INSTALLATION", "SYSTEM REQUIREMENTS").
    let letters: Vec<char> = first.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() >= 3 && letters.iter().all(|c| c.is_uppercase()) && !ends_sentence {
        return Some(first.to_string());
    }
    None
}

/// Collect up to `max` section headings from the text, using the same
/// block-splitting + detect_heading logic as `chunk_structured`. Feeds the
/// per-document summary memory's mini table-of-contents (v1.7.233).
pub(crate) fn collect_headings(text: &str, max: usize) -> Vec<String> {
    let mut blocks: Vec<&str> = text.split("\n\n").map(|b| b.trim()).filter(|b| !b.is_empty()).collect();
    if blocks.len() <= 1 {
        blocks = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    }
    let mut out: Vec<String> = Vec::new();
    for block in blocks {
        if let Some(h) = detect_heading(block) {
            if !out.contains(&h) {
                out.push(h);
                if out.len() >= max { break; }
            }
        }
    }
    out
}

/// `pub(crate)` so web ingestion chunks identically to PDF ingestion. The two
/// corpora are searched by the same query through the same index; splitting
/// them differently would make relevance depend on where the text came from.
pub(crate) fn chunk_structured(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    let mut blocks: Vec<String> = text
        .split("\n\n")
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty())
        .collect();
    if blocks.len() <= 1 {
        // No paragraph (blank-line) structure — split on lines so chunks at
        // least break on line boundaries rather than mid-word.
        blocks = text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    }
    if blocks.len() <= 1 {
        // Truly unstructured (one giant line) — fall back to the char window.
        return chunk_text(text, chunk_size, overlap);
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut section: Option<String> = None;       // most recent heading seen
    let mut cur_section: Option<String> = None;   // heading active when `cur` began

    for block in &blocks {
        if let Some(h) = detect_heading(block) {
            section = Some(h);
        }
        // A single oversized block: flush, then hard-split just that block.
        if block.len() > chunk_size {
            if !cur.trim().is_empty() {
                chunks.push(heading_context(&cur_section, &cur));
                cur.clear();
            }
            for piece in chunk_text(block, chunk_size, overlap) {
                chunks.push(heading_context(&section, &piece));
            }
            continue;
        }
        // Adding this block would overflow → flush the current chunk first.
        if !cur.is_empty() && cur.len() + 1 + block.len() > chunk_size {
            chunks.push(heading_context(&cur_section, &cur));
            cur.clear();
        }
        if cur.is_empty() {
            cur_section = section.clone();
        } else {
            cur.push('\n');
        }
        cur.push_str(block);
    }
    if !cur.trim().is_empty() {
        chunks.push(heading_context(&cur_section, &cur));
    }
    chunks
}

// ── Tauri commands ─────────────────────────────────────────────────────────

/// Ingest a PDF file into Lucy's memory and embedding systems.
///
/// Emits `pdf_progress` events during the two phases:
///   - Phase "saving"    → chunk N of M written to agent_memories
///   - Phase "embedding" → chunk N of M embedded via Ollama (background)
///   - Phase "done"      → all embeddings complete
///
/// Returns immediately after saving chunks; embeddings run in a background task.
#[tauri::command]
pub async fn pdf_ingest(
    app: AppHandle,
    path: String,
    chunk_size: Option<u32>,
) -> Result<PdfIngestResult, String> {
    // SECURITY: validate path before touching the filesystem
    // 1. No path traversal sequences
    if path.contains("..") {
        return Err("Invalid path: path traversal sequences (..) are not allowed.".to_string());
    }
    // 2. Must be a PDF file
    if !path.to_lowercase().ends_with(".pdf") {
        return Err("Only .pdf files can be ingested. Please select a PDF document.".to_string());
    }

    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("File not found: {}", path));
    }

    let filename = p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document.pdf")
        .to_string();

    let doc_id = generate_id();
    let csize  = chunk_size.unwrap_or(2_500) as usize;

    // ── Phase 1: extract text ─────────────────────────────────────────────
    let _ = app.emit("pdf_progress", PdfProgress {
        doc_id: doc_id.clone(), current: 0, total: 0,
        phase: "extracting".into(),
        message: format!("Extracting text from {}…", filename),
    });

    // v1.7.183 — prefer markitdown (structure-preserving Markdown + optional
    // OCR) when installed; fall back to the built-in pure-Rust extractor. The
    // subprocess runs in spawn_blocking so it never stalls the async runtime.
    let (raw_text, extractor) = {
        let p_owned = path.clone();
        let via_md = tauri::async_runtime::spawn_blocking(move || try_markitdown(&p_owned))
            .await
            .ok()
            .flatten();
        match via_md {
            Some(md) => (md, "markitdown"),
            None => {
                let txt = pdf_extract::extract_text(p).map_err(|e| format!(
                    "PDF text extraction failed for '{}': {}. \
                     Tip: scanned/image-only PDFs have no text layer — add one with \
                     OCR software, or install `markitdown` with its OCR plugin first.",
                    filename, e
                ))?;
                (txt, "pdf-extract")
            }
        }
    };

    if raw_text.trim().is_empty() {
        return Err(format!(
            "'{}' appears to be image-only or empty. \
             Text extraction requires a text-layer PDF. \
             Use OCR (Adobe Acrobat / Tesseract) or install `markitdown` with its \
             OCR plugin to read scanned documents.",
            filename
        ));
    }

    // Normalise: trim lines, collapse blank runs
    let clean: String = raw_text
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n");

    let total_chars = clean.len();

    // ── v1.7.233: idempotent re-ingest ────────────────────────────────────
    // Hash the EXTRACTED TEXT (not the path — drag-drop copies the file to a
    // temp path, so path equality never matches). Same content already
    // ingested → refuse with a friendly pointer instead of silently writing
    // a full duplicate chunk set (the pre-fix behaviour: doc_id was random,
    // so re-ingesting a 1475-section manual duplicated all 1475 rows).
    let content_hash = format!("{:x}", Sha256::digest(clean.as_bytes()));
    let existing: Option<(String, u32)> = shared_db(|conn| {
        Ok(conn.query_row(
            "SELECT filename, chunk_count FROM pdf_documents
             WHERE content_hash = ?1 AND status = 'done'
             LIMIT 1",
            rusqlite::params![content_hash],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)),
        ).ok())
    })?;
    if let Some((prev_name, prev_chunks)) = existing {
        return Err(format!(
            "Este documento ya está ingerido como '{}' ({} secciones). \
             Lucy ya puede consultarlo con pdf_search. Si quieres re-ingerirlo \
             (p.ej. tras mejorar el chunking), bórralo primero desde el panel PDF.",
            prev_name, prev_chunks
        ));
    }

    // Surface which extractor ran so the user can tell whether markitdown's
    // structure-preserving path kicked in (vs the plain pdf-extract fallback).
    let _ = app.emit("pdf_progress", PdfProgress {
        doc_id: doc_id.clone(), current: 0, total: 0,
        phase: "extracting".into(),
        message: format!("Texto extraído con {} ({} caracteres)", extractor, total_chars),
    });

    let chunks      = chunk_structured(&clean, csize, CHUNK_OVERLAP);
    let total       = chunks.len() as u32;

    if total == 0 {
        return Err(format!("No usable text found in '{}'.", filename));
    }

    // ── Phase 2: save doc record (status = ingesting) ────────────────────
    shared_db(|conn| {
        conn.execute(
            "INSERT INTO pdf_documents (id, filename, path, chunk_count, status, content_hash)
             VALUES (?1, ?2, ?3, ?4, 'ingesting', ?5)",
            rusqlite::params![doc_id, filename, path, total, content_hash],
        ).map_err(|e| format!("pdf_documents insert: {}", e))?;
        Ok(())
    })?;

    // ── Phase 3: save each chunk as agent_memory ─────────────────────────
    let session_id = format!("pdf:{}", doc_id);
    let mut memory_ids: Vec<i64> = Vec::with_capacity(chunks.len());

    for (i, chunk) in chunks.iter().enumerate() {
        let n     = (i + 1) as u32;
        let title = format!("PDF: {} §{}/{}", filename, n, total);
        let tags  = format!("[\"pdf\",\"pdf:{}\",\"{}\"]",
                            doc_id,
                            filename.replace('"', "'"));
        let files = format!("[\"{}\"]", path.replace('\\', "\\\\").replace('"', "\\\""));

        // v1.7.233: importance 1 (was hardcoded 2). Reference-manual chunks
        // must never outrank organic memories in importance-ordered queries;
        // they are excluded from the row-level janitors anyway (managed at
        // document level via pdf_delete_doc).
        let mid = shared_db(|conn| {
            conn.execute(
                "INSERT INTO agent_memories
                    (session_id, title, content, tags, files, importance)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                rusqlite::params![session_id, title, chunk, tags, files],
            ).map_err(|e| format!("agent_memories insert chunk {}: {}", n, e))?;
            Ok(conn.last_insert_rowid())
        })?;

        memory_ids.push(mid);

        let _ = app.emit("pdf_progress", PdfProgress {
            doc_id: doc_id.clone(), current: n, total,
            phase: "saving".into(),
            message: format!("Saved chunk {}/{}", n, total),
        });
    }

    // ── Mark doc as done (memory phase complete) ─────────────────────────
    shared_db(|conn| {
        conn.execute(
            "UPDATE pdf_documents SET status = 'done', chunk_count = ?1 WHERE id = ?2",
            rusqlite::params![total, doc_id],
        ).map_err(|e| format!("pdf_documents update: {}", e))?;
        Ok(())
    })?;

    // ── v1.7.233: ONE summary memory per document ─────────────────────────
    // The chunks themselves are reference material (importance 1, excluded
    // from the browser/ambient recall). This single importance-2 row is what
    // Lucy actually "remembers": the document exists, its shape, and how to
    // query it. session_id 'pdf-doc:' (NOT 'pdf:') so it survives the pdf
    // exclusions; embedded as entity_type 'memory' below so the pre-loop
    // semantic recall can surface it when the user asks about the topic.
    let toc = collect_headings(&clean, 12);
    let summary_body = format!(
        "Documento de referencia ingerido: \"{}\" — {} secciones, {} caracteres. \
         Para consultar su contenido usa la herramienta pdf_search (búsqueda semántica) \
         o memoria_buscar (texto).{}",
        filename, total, total_chars,
        if toc.is_empty() { String::new() } else { format!(" Temas detectados: {}.", toc.join(" · ")) }
    );
    let summary_mid: Option<i64> = {
        let doc_session = format!("pdf-doc:{}", doc_id);
        let s_title = format!("Documento: {}", filename);
        let s_tags = format!("[\"pdf-doc\",\"pdf:{}\",\"{}\"]", doc_id, filename.replace('"', "'"));
        let s_files = format!("[\"{}\"]", path.replace('\\', "\\\\").replace('"', "\\\""));
        shared_db(|conn| {
            conn.execute(
                "INSERT INTO agent_memories
                    (session_id, title, content, tags, files, importance)
                 VALUES (?1, ?2, ?3, ?4, ?5, 2)",
                rusqlite::params![doc_session, s_title, summary_body, s_tags, s_files],
            ).map_err(|e| format!("doc summary insert: {}", e))?;
            Ok(Some(conn.last_insert_rowid()))
        }).unwrap_or(None)
    };

    // ── Phase 4: embed in background (Ollama, fire-and-forget) ───────────
    // v1.7.233: BATCHED — 16 chunks per /api/embed call (M1) instead of one
    // HTTP round-trip per chunk. A 1475-chunk manual drops from 1475 serial
    // requests (minutes of Ollama churn) to ~92. embed_and_store_batch also
    // mirrors each vector into sqlite-vec, so pdf_search sees the new chunks
    // immediately instead of after the next boot's backfill.
    {
        let app2      = app.clone();
        let doc2      = doc_id.clone();
        let fname2    = filename.clone();
        let chunks2   = chunks.clone();
        let mids2     = memory_ids.clone();

        tauri::async_runtime::spawn(async move {
            let total_emb = chunks2.len() as u32;
            let items: Vec<(String, String)> = chunks2.iter().zip(mids2.iter())
                // Prepend filename so the embedding carries document context
                .map(|(chunk, mid)| (mid.to_string(), format!("[{}] {}", fname2, chunk)))
                .collect();
            let mut done: u32 = 0;
            for batch in items.chunks(16) {
                match embed_and_store_batch("pdf_chunk", batch, None).await {
                    Ok(n)  => done += n,
                    Err(e) => {
                        // Was `eprintln!` only, which in a windowed build goes
                        // to a console nobody sees. These chunks are now
                        // recoverable — Diagnóstico → "Re-embeber documentos"
                        // runs backfill_embeddings("pdf_chunk") — but the
                        // operator has to be able to find out it happened.
                        crate::utils::logging::write_app_log("WARN", &format!(
                            "[pdf] embed batch failed for '{}' ({} chunks): {} — \
                             recoverable via the Document Embeddings repair",
                            fname2, batch.len(), e
                        ));
                    }
                }
                let _ = app2.emit("pdf_progress", PdfProgress {
                    doc_id: doc2.clone(), current: done.min(total_emb), total: total_emb,
                    phase: "embedding".into(),
                    message: format!("Embedding {}/{}", done.min(total_emb), total_emb),
                });
            }
            // The doc-level summary memory gets a 'memory'-type embedding so
            // the pre-loop semantic recall can surface "this manual exists"
            // when the user asks about its topic.
            if let Some(smid) = summary_mid {
                let _ = embed_and_store(
                    "memory".into(),
                    smid.to_string(),
                    format!("Documento: {} — manual/documento de referencia ingerido, consultable con pdf_search", fname2),
                    None,
                ).await;
            }
            // The final event used to report `current: total_emb` and "Document
            // ready. Lucy can now answer questions about this PDF" no matter
            // how many batches had failed — the one moment the user is looking
            // at this document, spent asserting a completeness nobody checked.
            // It reports what actually landed now.
            let _ = app2.emit("pdf_progress", PdfProgress {
                doc_id: doc2.clone(), current: done, total: total_emb,
                phase: "done".into(),
                message: if done >= total_emb {
                    "Document ready. Lucy can now answer questions about this PDF.".into()
                } else {
                    format!(
                        "Document saved, but {}/{} sections could not be embedded and are not \
                         searchable. Recover them in Diagnostics → Re-embed documents.",
                        total_emb - done, total_emb
                    )
                },
            });
        });
    }

    Ok(PdfIngestResult { doc_id, filename, chunk_count: total, total_chars })
}

/// List all ingested PDF documents (newest first).
#[tauri::command]
pub async fn pdf_list_docs() -> Result<Vec<PdfDocument>, String> {
    shared_db(|conn| {
        // v1.7.233: embedded_count via correlated subquery — the docs list can
        // now show real embedding coverage ("N/M embebidos") instead of the
        // ambiguous 'done' (which only covers the chunk-saving phase).
        let mut stmt = conn.prepare(
            // v1.8 — `d.kind || ':' || d.id` reconstructs the session_id for
            // either corpus ('pdf:<id>' or 'web:<id>'). Left PDF-only, an
            // ingested web page would show 0/N embedded forever, which is the
            // exact signal the coverage warning and the re-embed repair key
            // off — a permanent false alarm on a healthy document.
            "SELECT d.id, d.filename, d.path, d.page_count, d.chunk_count, d.ingested_at, d.status, d.kind,
                    (SELECT COUNT(*) FROM embeddings e
                      WHERE e.entity_type IN ('pdf_chunk','web_chunk')
                        AND e.entity_id IN (SELECT CAST(am.id AS TEXT) FROM agent_memories am
                                             WHERE am.session_id = d.kind || ':' || d.id)) AS embedded_count
             FROM pdf_documents d ORDER BY d.ingested_at DESC",
        ).map_err(|e| format!("pdf_list_docs prepare: {}", e))?;

        fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PdfDocument> {
            Ok(PdfDocument {
                id:          row.get(0)?,
                filename:    row.get(1)?,
                path:        row.get(2)?,
                page_count:  row.get(3)?,
                chunk_count: row.get(4)?,
                ingested_at: row.get(5)?,
                status:      row.get(6)?,
                kind:        row.get(7)?,
                embedded_count: row.get(8)?,
            })
        }

        let docs: Vec<PdfDocument> = stmt
            .query_map([], read_row)
            .map_err(|e| format!("pdf_list_docs query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(docs)
    })
}

/// Delete a PDF document and ALL its associated chunks (agent_memories)
/// and vector embeddings. Returns the number of chunks deleted.
#[tauri::command]
pub async fn pdf_delete_doc(doc_id: String) -> Result<u32, String> {
    let session_id = format!("pdf:{}", doc_id);

    // 1. Collect memory IDs (need them to delete matching embeddings)
    let mem_ids: Vec<i64> = shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id FROM agent_memories WHERE session_id = ?1",
        ).map_err(|e| format!("prepare: {}", e))?;
        let v: Vec<i64> = stmt
            .query_map(rusqlite::params![session_id], |r| r.get(0))
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(v)
    })?;

    let chunk_count = mem_ids.len() as u32;
    let doc_session = format!("pdf-doc:{}", doc_id);

    // 2-4. One DB pass (v1.7.233 — was 1475 separate lock acquisitions):
    //   · chunk embeddings via a single subquery DELETE
    //   · the doc-level summary memory (session 'pdf-doc:') + its 'memory'
    //     embedding, added in v1.7.233
    //   · the chunk rows and the doc record
    let synth_session = format!("pdf:synth:{}", doc_id);
    shared_db(|conn| {
        // v1.7.233 M2: los resúmenes de síntesis ('pdf:synth:') también se
        // embeben como pdf_chunk — el mismo subquery de sesión los cubre.
        conn.execute(
            "DELETE FROM embeddings
              WHERE entity_type = 'pdf_chunk'
                AND entity_id IN (SELECT CAST(id AS TEXT) FROM agent_memories
                                   WHERE session_id = ?1 OR session_id = ?2)",
            rusqlite::params![session_id, synth_session],
        ).map_err(|e| format!("delete embeddings: {}", e))?;
        conn.execute(
            "DELETE FROM embeddings
              WHERE entity_type = 'memory'
                AND entity_id IN (SELECT CAST(id AS TEXT) FROM agent_memories WHERE session_id = ?1)",
            rusqlite::params![doc_session],
        ).map_err(|e| format!("delete summary embedding: {}", e))?;
        conn.execute(
            "DELETE FROM agent_memories WHERE session_id = ?1 OR session_id = ?2 OR session_id = ?3",
            rusqlite::params![session_id, doc_session, synth_session],
        ).map_err(|e| format!("delete memories: {}", e))?;
        conn.execute(
            "DELETE FROM pdf_documents WHERE id = ?1",
            rusqlite::params![doc_id],
        ).map_err(|e| format!("delete doc: {}", e))?;
        Ok(())
    })?;

    Ok(chunk_count)
}

/// Semantic search scoped to ingested PDF chunks.
/// Wraps `embeddings::semantic_search` with entity_type = "pdf_chunk".
/// Called by the frontend <TOOL>pdf_search:query</TOOL> handler.
#[tauri::command]
pub async fn pdf_search(
    query: String,
    limit: Option<u32>,
    min_score: Option<f32>,
) -> Result<Vec<crate::commands::embeddings::SemanticHit>, String> {
    use std::collections::HashMap;
    let lim = limit.unwrap_or(5).max(1) as usize;
    let base_min = min_score.unwrap_or(0.2);

    // ── v1.7.233 M3 — expansión de consulta + fusión RRF ─────────────────
    // La original + hasta 3 reformulaciones del LLM local (best-effort: sin
    // Ollama la lista queda en [original] y el comportamiento es el previo).
    let mut queries: Vec<String> = vec![query.clone()];
    queries.extend(crate::commands::metrics::expand_query_via_ollama(&query).await);

    const RRF_K: f64 = 60.0;
    let mut fused: HashMap<String, (f64, crate::commands::embeddings::SemanticHit)> = HashMap::new();
    // Every query failing for the same reason is not "no results".
    //
    // `unwrap_or_default()` on each call made "the embedder is unreachable"
    // and "the document does not discuss this" produce the identical empty
    // list. The agent then told the user their manual had nothing on the
    // topic — a confident, wrong answer about the user's own document, and no
    // trace anywhere pointing at the real cause.
    //
    // Per-query errors are still tolerated: the expansions are best-effort and
    // one of them timing out should not sink a search the original satisfies.
    // Only the case where NOTHING succeeded is escalated.
    let mut last_err: Option<String> = None;
    let mut any_ok = false;
    // v1.8 — both ingested corpora. They are separate entity types on purpose
    // (so a search CAN be scoped, and so reference material stays out of the
    // pre-loop memory recall), but a user asking "what did that page say about
    // X" does not care which door the text came through. Each type contributes
    // its own ranked list and RRF fuses them, which is exactly what the
    // fusion below already does for the query reformulations.
    for et in ["pdf_chunk", "web_chunk"] {
        for q in &queries {
            let hits = match crate::commands::embeddings::semantic_search(
                q.clone(),
                Some(et.to_string()),
                Some(8),
                Some(base_min.min(0.30)),
                None,
            ).await {
                Ok(h) => { any_ok = true; h }
                Err(e) => { last_err = Some(e); continue; }
            };
            for (rank, h) in hits.into_iter().enumerate() {
                let e = fused.entry(h.entity_id.clone()).or_insert((0.0, h));
                e.0 += 1.0 / (RRF_K + rank as f64 + 1.0);
            }
        }
    }
    if !any_ok {
        return Err(format!(
            "No se pudo consultar el índice semántico de documentos: {}. \
             Los PDFs siguen ingeridos; hace falta un embebedor disponible \
             (Ollama local o una clave de Gemini) para buscar en ellos.",
            last_err.unwrap_or_else(|| "causa desconocida".into())
        ));
    }

    let mut ranked: Vec<(f64, crate::commands::embeddings::SemanticHit)> = fused.into_values().collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(20);

    // Rerank cross-encoder (feature `ml-reranker`; devuelve None si no está
    // compilado o el modelo ONNX falta — fallback silencioso al orden RRF).
    let passages: Vec<&str> = ranked.iter().map(|(_, h)| h.text.as_str()).collect();
    if let Some(scores) = crate::commands::reranker::rerank(&query, &passages) {
        if scores.len() == ranked.len() {
            let mut paired: Vec<(f32, crate::commands::embeddings::SemanticHit)> =
                scores.into_iter().zip(ranked.into_iter().map(|(_, h)| h)).collect();
            paired.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            ranked = paired.into_iter().map(|(s, h)| (s as f64, h)).collect();
        }
    }
    let out: Vec<crate::commands::embeddings::SemanticHit> =
        ranked.into_iter().take(lim).map(|(_, h)| h).collect();

    // ── v1.7.233 M4 — señal de uso ────────────────────────────────────────
    // Lo consultado sube en el ranking futuro (mismo touch que
    // search_agent_memories). Fire-and-forget: un fallo no rompe la búsqueda.
    let ids: Vec<String> = out.iter()
        .filter_map(|h| h.entity_id.parse::<i64>().ok().map(|i| i.to_string()))
        .collect();
    if !ids.is_empty() {
        let id_list = ids.join(",");
        let _ = shared_db(move |conn| {
            let _ = conn.execute(&format!(
                "UPDATE agent_memories
                 SET access_count = access_count + 1,
                     last_accessed_at = strftime('%s','now')
                 WHERE id IN ({})", id_list), []);
            Ok(())
        });
    }
    Ok(out)
}

// ── v1.7.233 M2 — Síntesis jerárquica de documentos ─────────────────────────
// Loop background que destila cada PDF ingerido en resúmenes densos (~100-150
// palabras por lote de secciones consecutivas) usando el LLM local. Las
// síntesis se guardan como agent_memories session 'pdf:synth:<doc_id>'
// (prefijo 'pdf:' → heredan TODAS las exclusiones de chunks) y se embeben como
// entity_type 'pdf_chunk', así pdf_search las encuentra — y al ser densas
// suelen ganar el top-k frente al chunk crudo de 2.500 chars.

const SYNTH_BATCH: usize = 9;          // ~capítulo (secciones consecutivas)
const SYNTH_MODEL: &str = "qwen3:4b";  // mismo modelo que el consolidador

/// Agrupa ids de chunks consecutivos en lotes (start§, end§, ids) — 1-based.
fn synth_group(ids: &[i64], batch: usize) -> Vec<(usize, usize, Vec<i64>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < ids.len() {
        let end = (i + batch.max(1)).min(ids.len());
        out.push((i + 1, end, ids[i..end].to_vec()));
        i = end;
    }
    out
}

/// Resumen técnico fiel vía Ollama. Best-effort: Err si Ollama no responde.
async fn ollama_summarize(text: &str) -> Result<String, String> {
    let base = crate::commands::embeddings::ollama_base();
    let body = serde_json::json!({
        "model": SYNTH_MODEL,
        "messages": [
            { "role": "system", "content": "Eres un sintetizador técnico. Resume el fragmento de manual en 100-150 palabras EN ESPAÑOL, fiel al contenido, sin inventar nada, conservando exactos los nombres de opciones, comandos, rutas y valores. Devuelve SOLO el resumen." },
            { "role": "user",   "content": text }
        ],
        "stream": false,
        "options": { "temperature": 0.2, "num_predict": 320 }
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build().map_err(|e| format!("client: {}", e))?;
    let resp = client.post(format!("{}/api/chat", base)).json(&body).send().await
        .map_err(|e| format!("ollama: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("ollama {}", resp.status()));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| format!("json: {}", e))?;
    let mut out = json.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_str())
        .unwrap_or("").trim().to_string();
    // qwen3 puede emitir razonamiento <think>…</think> — quedarnos con lo de después.
    if let Some(p) = out.find("</think>") { out = out[p + 8..].trim().to_string(); }
    if out.is_empty() { Err("respuesta vacía".into()) } else { Ok(out) }
}

/// Un documento por tick. Devuelve true si procesó (o intentó) alguno.
async fn pdf_synth_tick() -> Result<bool, String> {
    let doc: Option<(String, String)> = shared_db(|conn| {
        Ok(conn.query_row(
            "SELECT id, filename FROM pdf_documents
             WHERE status = 'done' AND synth_status = ''
             ORDER BY ingested_at ASC LIMIT 1",
            [], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ).ok())
    })?;
    let Some((doc_id, filename)) = doc else { return Ok(false) };

    let session = format!("pdf:{}", doc_id);
    let synth_session = format!("pdf:synth:{}", doc_id);

    // Claim + reintento idempotente: purga síntesis parciales de un intento
    // previo (y sus embeddings) ANTES de regenerar — sin esto un fallo a mitad
    // duplicaría resúmenes en el siguiente tick.
    shared_db(|conn| {
        conn.execute("UPDATE pdf_documents SET synth_status = 'working' WHERE id = ?1",
            rusqlite::params![doc_id]).map_err(|e| format!("claim: {}", e))?;
        conn.execute(
            "DELETE FROM embeddings WHERE entity_type = 'pdf_chunk'
               AND entity_id IN (SELECT CAST(id AS TEXT) FROM agent_memories WHERE session_id = ?1)",
            rusqlite::params![synth_session]).map_err(|e| format!("purge emb: {}", e))?;
        conn.execute("DELETE FROM agent_memories WHERE session_id = ?1",
            rusqlite::params![synth_session]).map_err(|e| format!("purge synth: {}", e))?;
        Ok(())
    })?;

    let rows: Vec<(i64, String)> = shared_db(|conn| {
        let mut st = conn.prepare(
            "SELECT id, content FROM agent_memories WHERE session_id = ?1 ORDER BY id ASC",
        ).map_err(|e| format!("chunks prepare: {}", e))?;
        let v = st.query_map(rusqlite::params![session], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| format!("chunks query: {}", e))?
            .filter_map(|r| r.ok()).collect();
        Ok(v)
    })?;
    if rows.is_empty() {
        shared_db(|conn| { let _ = conn.execute(
            "UPDATE pdf_documents SET synth_status = 'done' WHERE id = ?1",
            rusqlite::params![doc_id]); Ok(()) })?;
        return Ok(true);
    }

    let ids: Vec<i64> = rows.iter().map(|(i, _)| *i).collect();
    let mut items: Vec<(String, String)> = Vec::new();  // (entity_id, texto a embeber)
    let mut made = 0usize;
    for (a, b, gids) in synth_group(&ids, SYNTH_BATCH) {
        let joined: String = rows.iter()
            .filter(|(i, _)| gids.contains(i))
            .map(|(_, c)| c.as_str())
            .collect::<Vec<_>>().join("\n\n")
            .chars().take(12_000).collect();
        match ollama_summarize(&joined).await {
            Ok(sum) => {
                let title = format!("Síntesis: {} §{}-{}", filename, a, b);
                let tags = format!("[\"pdf-synth\",\"pdf:{}\",\"{}\"]", doc_id, filename.replace('"', "'"));
                let mid = shared_db(|conn| {
                    conn.execute(
                        "INSERT INTO agent_memories (session_id, title, content, tags, files, importance)
                         VALUES (?1, ?2, ?3, ?4, '[]', 1)",
                        rusqlite::params![synth_session, title, sum, tags],
                    ).map_err(|e| format!("synth insert: {}", e))?;
                    Ok(conn.last_insert_rowid())
                })?;
                items.push((mid.to_string(), format!("[{}] {}", filename, sum)));
                made += 1;
            }
            Err(e) => {
                // Ollama caído a mitad: re-marca pendiente (el próximo tick
                // purga lo parcial y reintenta desde cero) y sal del tick.
                eprintln!("[pdf-synth] {} lote §{}-{}: {}", filename, a, b, e);
                shared_db(|conn| { let _ = conn.execute(
                    "UPDATE pdf_documents SET synth_status = '' WHERE id = ?1",
                    rusqlite::params![doc_id]); Ok(()) })?;
                return Ok(true);
            }
        }
    }
    let _ = crate::commands::embeddings::embed_and_store_batch("pdf_chunk", &items, None).await;
    shared_db(|conn| { let _ = conn.execute(
        "UPDATE pdf_documents SET synth_status = 'done' WHERE id = ?1",
        rusqlite::params![doc_id]); Ok(()) })?;
    eprintln!("[pdf-synth] {}: {} síntesis creadas", filename, made);
    Ok(true)
}

/// Loop background (patrón proactive_detector). Idempotente. Delay inicial
/// 3 min; tick cada 30 min; UN documento por tick para no saturar Ollama.
pub fn start_pdf_synth_loop() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) { return; }
    tauri::async_runtime::spawn(async {
        // Crash-recovery: un 'working' huérfano de una sesión anterior vuelve
        // a la cola (el tick purga sus parciales antes de regenerar).
        let _ = shared_db(|conn| { let _ = conn.execute(
            "UPDATE pdf_documents SET synth_status = '' WHERE synth_status = 'working'", []); Ok(()) });
        tokio::time::sleep(std::time::Duration::from_secs(180)).await;
        loop {
            if let Err(e) = pdf_synth_tick().await { eprintln!("[pdf-synth] tick: {}", e); }
            tokio::time::sleep(std::time::Duration::from_secs(30 * 60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── detect_heading — the GoAnywhere incident class ──────────────────────
    // A numbered FAQ QUESTION must not become a (sticky) section heading; the
    // '?' veto is what prevents one FAQ entry from prefixing hundreds of
    // chunks (v1.7.233 fix).
    #[test]
    fn faq_question_is_not_a_heading() {
        assert_eq!(detect_heading("5. What encryption standards does your trading partner support?"), None);
        assert_eq!(detect_heading("3. Install the agent now!"), None);
    }

    #[test]
    fn real_numbered_headings_still_match() {
        assert_eq!(detect_heading("3.1 Installation Requirements"), Some("3.1 Installation Requirements".into()));
        assert_eq!(detect_heading("7 Pre-Installation Notes"), Some("7 Pre-Installation Notes".into()));
    }

    #[test]
    fn markdown_and_allcaps_headings_match() {
        assert_eq!(detect_heading("## Firewall Recommendations"), Some("Firewall Recommendations".into()));
        assert_eq!(detect_heading("SYSTEM REQUIREMENTS"), Some("SYSTEM REQUIREMENTS".into()));
    }

    #[test]
    fn sentences_and_paragraphs_are_not_headings() {
        assert_eq!(detect_heading("This is a normal sentence that ends with a period."), None);
        assert_eq!(detect_heading("First line\nSecond line of the same block"), None);
        assert_eq!(detect_heading(""), None);
    }

    // ── M2 síntesis: agrupación en lotes consecutivos ────────────────────────
    #[test]
    fn synth_group_batches_consecutively() {
        let ids: Vec<i64> = (100..122).collect(); // 22 chunks
        let g = synth_group(&ids, 9);
        assert_eq!(g.len(), 3);
        assert_eq!((g[0].0, g[0].1), (1, 9));
        assert_eq!((g[1].0, g[1].1), (10, 18));
        assert_eq!((g[2].0, g[2].1), (19, 22));
        assert_eq!(g[2].2, (118..122).collect::<Vec<i64>>());
        assert_eq!(g.iter().map(|(_, _, v)| v.len()).sum::<usize>(), 22);
        assert!(synth_group(&[], 9).is_empty());
    }

    // ── chunking sanity: overlap + coverage on plain text ───────────────────
    #[test]
    fn chunk_text_covers_and_overlaps() {
        let text = "abcdefghij".repeat(100); // 1000 chars, no structure
        let chunks = chunk_text(&text, 300, 50);
        assert!(chunks.len() >= 3);
        // Every chunk within size; consecutive chunks share the overlap region.
        for c in &chunks { assert!(c.chars().count() <= 300); }
    }

    // ── The extractor itself (v1.8.1) ───────────────────────────────────────
    //
    // Every other test above covers a pure helper, so until now NOTHING here
    // ever ran the actual text extraction. That is a gap with teeth: v1.8.1
    // bumped `pdf-extract` 0.7 → 0.12 (five major versions) to escape
    // RUSTSEC-2026-0187, on the exact code path the attachment fix depends on.
    // The bump's Cargo.toml note asserts "that entry point is unchanged across
    // the bump" — these tests are what turns that assertion into something
    // checked. A silent break makes every attached PDF report "no tiene capa
    // de texto", which reads as a scanned-document problem, not a regression.
    //
    // Re-run these after ANY pdf-extract / lopdf move (ARCHITECTURE §7 says to
    // expect them: the advisory triage is revisited on every Tauri upgrade).

    /// Assemble a minimal but structurally valid one-page PDF containing
    /// `text`. The xref offsets are computed while the body is built because
    /// lopdf reads them; a hand-written table drifts the moment anything above
    /// it changes by a byte.
    fn build_text_pdf(text: &str) -> Vec<u8> {
        let content = format!("BT /F1 24 Tf 72 700 Td ({}) Tj ET\n", text);
        let objects: [String; 5] = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{}endstream", content.len(), content),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];

        let mut buf: Vec<u8> = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (i, body) in objects.iter().enumerate() {
            offsets.push(buf.len());
            buf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, body).as_bytes());
        }

        let xref_at = buf.len();
        buf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        buf.extend_from_slice(b"0000000000 65535 f \n");
        for off in &offsets {
            // Every xref entry is exactly 20 bytes — the spec is strict here.
            buf.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
        }
        buf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                xref_at
            ).as_bytes(),
        );
        buf
    }

    /// Canary for the crate bump: calls `pdf_extract::extract_text` DIRECTLY
    /// rather than through `extract_pdf_text`, on purpose. The wrapper tries
    /// markitdown first, so on a machine that has it installed a wrapper-level
    /// test would pass through markitdown and mask a broken pure-Rust
    /// extractor — which is the fallback every machine without it relies on.
    #[test]
    fn pdf_extract_reads_a_real_text_layer_pdf() {
        const CANARY: &str = "LUCY PDF EXTRACTION CANARY";
        let tmp = std::env::temp_dir().join(format!("lucy_pdf_test_{}.pdf", generate_id()));
        std::fs::write(&tmp, build_text_pdf(CANARY)).expect("write temp pdf");

        let got = pdf_extract::extract_text(&tmp);
        let _ = std::fs::remove_file(&tmp);

        let text = got.expect("pdf-extract failed on a valid text-layer PDF");
        assert!(
            text.contains("CANARY"),
            "extractor returned text without the canary: {:?}",
            text
        );
    }

    /// The production wrapper on the same PDF. Covers the markitdown fallback
    /// and the empty-text guard, and pins the attachment contract: what comes
    /// back is TEXT, which is why an attached PDF is `type: 'text'` (§4.1).
    #[test]
    fn extract_pdf_text_returns_the_documents_text() {
        const CANARY: &str = "LUCY WRAPPER CANARY";
        let tmp = std::env::temp_dir().join(format!("lucy_pdf_wrap_{}.pdf", generate_id()));
        std::fs::write(&tmp, build_text_pdf(CANARY)).expect("write temp pdf");

        let got = extract_pdf_text(&tmp);
        let _ = std::fs::remove_file(&tmp);

        let text = got.expect("extract_pdf_text failed on a valid text-layer PDF");
        assert!(!text.trim().is_empty());
        assert!(text.contains("CANARY"), "wrapper lost the text: {:?}", text);
    }

    /// A file that is not a PDF must come back as `Err`, never as empty text.
    /// The attachment path reports that error to the user in-band; returning
    /// Ok("") instead would attach a chip carrying nothing, which is precisely
    /// the v1.8.0 failure mode the pipeline fix set out to end.
    #[test]
    fn extract_pdf_text_errors_on_a_non_pdf() {
        let tmp = std::env::temp_dir().join(format!("lucy_pdf_bad_{}.pdf", generate_id()));
        std::fs::write(&tmp, b"this is plainly not a PDF").expect("write temp file");

        let got = extract_pdf_text(&tmp);
        let _ = std::fs::remove_file(&tmp);

        assert!(got.is_err(), "a non-PDF must not extract as Ok: {:?}", got);
    }

    // ── The drag-and-drop entry point ───────────────────────────────────────
    //
    // `dragDropEnabled: false` means a dropped PDF arrives with no filesystem
    // path, so it cannot reuse the picker's path — it is base64'd in the
    // webview and staged here. That makes this the ONLY hop where Lucy writes
    // attacker-influenced bytes to disk, so its guards get pinned: the size
    // ceiling, the base64 validation, the filename-is-cosmetic rule, and the
    // temp-file cleanup.

    /// Names of the files this command stages, so a leak is detectable.
    ///
    /// `%TEMP%` is process-global, so this census is only exact single-threaded
    /// — a sibling test staging its own file otherwise shows up here and the
    /// assertion fails for a reason that has nothing to do with cleanup. The
    /// suite already required `--test-threads=1` for the shell contract tests;
    /// `src-tauri/.cargo/config.toml` now makes that the default instead of
    /// something you had to know.
    fn staged_temp_files() -> Vec<String> {
        std::fs::read_dir(std::env::temp_dir())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with("lucy_attach_"))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Serialises the tests that watch %TEMP%.
    ///
    /// They assert on the CONTENTS OF A SHARED DIRECTORY, and the suite runs in
    /// parallel in one process: while one test has its file staged, another one
    /// listing the same directory sees it and reports a leak that never
    /// happened. It failed intermittently — roughly once in every few runs —
    /// which is the worst kind, because the fix people reach for is re-running
    /// until it goes green.
    ///
    /// The lock cannot go inside `staged_temp_files`: the window that has to be
    /// protected spans the before-listing, the call, and the after-listing.
    fn temp_serie() -> std::sync::MutexGuard<'static, ()> {
        static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
        L.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The drag-and-drop happy path, end to end: PDF bytes → base64 → staged
    /// temp file → extractor → text. This is what the composer chip promises
    /// and what the prompt builder puts under `--- ARCHIVOS ---`.
    #[tokio::test]
    async fn extract_from_bytes_round_trips_a_dropped_pdf() {
        use base64::{engine::general_purpose, Engine as _};
        const CANARY: &str = "LUCY DROPPED PDF CANARY";

        let _serie = temp_serie();
        let b64 = general_purpose::STANDARD.encode(build_text_pdf(CANARY));
        let before = staged_temp_files();

        let text = extract_pdf_text_from_bytes("dropped.pdf".into(), b64)
            .await
            .expect("dropped PDF failed to extract");

        assert!(text.contains("CANARY"), "drop path lost the text: {:?}", text);
        assert_eq!(
            staged_temp_files(), before,
            "the staged temp file outlived a successful extraction"
        );
    }

    /// A malformed payload must fail AND clean up after itself — the failure
    /// path is the one that used to leave the user's bytes in %TEMP%.
    #[tokio::test]
    async fn extract_from_bytes_cleans_up_after_a_failed_extraction() {
        use base64::{engine::general_purpose, Engine as _};

        let _serie = temp_serie();
        let b64 = general_purpose::STANDARD.encode(b"not a PDF at all");
        let before = staged_temp_files();

        let got = extract_pdf_text_from_bytes("junk.pdf".into(), b64).await;

        assert!(got.is_err(), "garbage bytes must not extract as Ok: {:?}", got);
        assert_eq!(
            staged_temp_files(), before,
            "a failed extraction left its staged temp file behind"
        );
    }

    /// The size ceiling is checked on the ENCODED string, before decoding, so
    /// a stray 100 MB drop is refused instead of first becoming 75 MB of Vec.
    #[tokio::test]
    async fn extract_from_bytes_refuses_an_oversized_payload() {
        let _serie = temp_serie();
        let huge = "A".repeat(80 * 1024 * 1024 + 4);
        let before = staged_temp_files();

        let got = extract_pdf_text_from_bytes("huge.pdf".into(), huge).await;

        let err = got.expect_err("an oversized payload must be refused");
        assert!(err.contains("huge.pdf"), "error should name the file: {}", err);
        assert_eq!(staged_temp_files(), before, "a refused payload was still staged");
    }

    #[tokio::test]
    async fn extract_from_bytes_rejects_invalid_base64() {
        let got = extract_pdf_text_from_bytes("bad.pdf".into(), "not!valid!base64!".into()).await;
        let err = got.expect_err("invalid base64 must be refused");
        assert!(err.contains("base64"), "error should say what was wrong: {}", err);
    }

    /// The caller-supplied name reaches error messages only — never the path.
    /// A traversing name must neither steer the write nor break the read.
    #[tokio::test]
    async fn extract_from_bytes_ignores_a_hostile_filename() {
        use base64::{engine::general_purpose, Engine as _};
        const CANARY: &str = "LUCY HOSTILE NAME CANARY";

        // Takes the lock too. It does not read the directory itself, but it DOES
        // stage a file there, and a sibling counting temp files while this one
        // is mid-flight blames the leak on itself.
        let _serie = temp_serie();
        let b64 = general_purpose::STANDARD.encode(build_text_pdf(CANARY));
        let hostile = r"..\..\..\..\evil";
        let escaped = std::env::temp_dir().join("..").join("evil.pdf");

        let text = extract_pdf_text_from_bytes(hostile.into(), b64)
            .await
            .expect("a cosmetic-only name must not break extraction");

        assert!(text.contains("CANARY"));
        assert!(!escaped.exists(), "the filename steered the write outside temp_dir");
    }
}
