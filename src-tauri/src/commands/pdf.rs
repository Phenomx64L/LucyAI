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

use crate::commands::embeddings::embed_and_store;
use crate::commands::metrics::shared_db;
use crate::utils::db::{generate_id, PdfDocument};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{AppHandle, Emitter};

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

    let raw_text = pdf_extract::extract_text(p)
        .map_err(|e| format!(
            "PDF text extraction failed for '{}': {}. \
             Tip: scanned/image-only PDFs have no text layer — \
             use OCR software first.",
            filename, e
        ))?;

    if raw_text.trim().is_empty() {
        return Err(format!(
            "'{}' appears to be image-only or empty. \
             Text extraction requires a text-layer PDF. \
             Use OCR software (e.g. Adobe Acrobat, Tesseract) to add a text layer first.",
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
    let chunks      = chunk_text(&clean, csize, CHUNK_OVERLAP);
    let total       = chunks.len() as u32;

    if total == 0 {
        return Err(format!("No usable text found in '{}'.", filename));
    }

    // ── Phase 2: save doc record (status = ingesting) ────────────────────
    shared_db(|conn| {
        conn.execute(
            "INSERT INTO pdf_documents (id, filename, path, chunk_count, status)
             VALUES (?1, ?2, ?3, ?4, 'ingesting')",
            rusqlite::params![doc_id, filename, path, total],
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

        let mid = shared_db(|conn| {
            conn.execute(
                "INSERT INTO agent_memories
                    (session_id, title, content, tags, files, importance)
                 VALUES (?1, ?2, ?3, ?4, ?5, 2)",
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

    // ── Phase 4: embed in background (Ollama, fire-and-forget) ───────────
    {
        let app2      = app.clone();
        let doc2      = doc_id.clone();
        let fname2    = filename.clone();
        let chunks2   = chunks.clone();
        let mids2     = memory_ids.clone();

        tauri::async_runtime::spawn(async move {
            let total_emb = chunks2.len() as u32;
            for (i, (chunk, mid)) in chunks2.iter().zip(mids2.iter()).enumerate() {
                let n   = (i + 1) as u32;
                // Prepend filename so the embedding carries document context
                let txt = format!("[{}] {}", fname2, chunk);
                match embed_and_store(
                    "pdf_chunk".into(),
                    mid.to_string(),
                    txt,
                    None,
                ).await {
                    Ok(_)  => {},
                    Err(e) => {
                        eprintln!("[pdf] embed skip chunk {}/{}: {}", n, total_emb, e);
                    }
                }
                let _ = app2.emit("pdf_progress", PdfProgress {
                    doc_id: doc2.clone(), current: n, total: total_emb,
                    phase: "embedding".into(),
                    message: format!("Embedding chunk {}/{}", n, total_emb),
                });
            }
            let _ = app2.emit("pdf_progress", PdfProgress {
                doc_id: doc2.clone(), current: total_emb, total: total_emb,
                phase: "done".into(),
                message: "Document ready. Lucy can now answer questions about this PDF.".into(),
            });
        });
    }

    Ok(PdfIngestResult { doc_id, filename, chunk_count: total, total_chars })
}

/// List all ingested PDF documents (newest first).
#[tauri::command]
pub async fn pdf_list_docs() -> Result<Vec<PdfDocument>, String> {
    shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, filename, path, page_count, chunk_count, ingested_at, status
             FROM pdf_documents ORDER BY ingested_at DESC",
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

    // 2. Delete embeddings (entity_id = mem_id as string)
    for mid in &mem_ids {
        let eid = mid.to_string();
        let _ = shared_db(|conn| {
            conn.execute(
                "DELETE FROM embeddings
                  WHERE entity_type = 'pdf_chunk' AND entity_id = ?1",
                rusqlite::params![eid],
            ).map_err(|e| format!("delete embedding: {}", e))?;
            Ok(())
        });
    }

    // 3. Delete all memory chunks for this document
    shared_db(|conn| {
        conn.execute(
            "DELETE FROM agent_memories WHERE session_id = ?1",
            rusqlite::params![session_id],
        ).map_err(|e| format!("delete memories: {}", e))?;
        Ok(())
    })?;

    // 4. Delete doc record
    shared_db(|conn| {
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
    crate::commands::embeddings::semantic_search(
        query,
        Some("pdf_chunk".to_string()),
        limit.or(Some(5)),
        min_score.or(Some(0.2)),
        None,
    ).await
}
