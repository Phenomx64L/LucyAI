// ── F9 — Personal Knowledge Graph (MVP) ───────────────────────────────────
//
// Tracks the user's filesystem as a temporal graph:
//   • Nodes  = files (with metadata: extension, parent dir, last_mtime)
//   • Edges  = "co-touched": two files modified within ±N minutes of each
//              other on the same scan pass. Weighted by frequency.
//
// Capabilities this unlocks (no other AI tool has these):
//   • "What files are touched together when I'm working on X?"
//   • "Which files have I modified most recently in this project?"
//   • "Which file is most connected to <path>?"
//   • "What extensions dominate this directory tree?"
//
// Storage: SQLite tables (additive — created lazily, idempotent).
//   kg_files  (path PK, extension, parent_dir, last_mtime, first_seen, touch_count)
//   kg_edges  (path_a, path_b, weight, last_seen)  unique(path_a, path_b) where a<b
//
// Indexer:
//   • Polls explicitly-configured "watched" roots
//   • Caps total nodes per root (default 5000) to bound work
//   • Skips noise extensions (.tmp, .log, .pyc, .lock, dot-files, node_modules)
//   • Co-touch window: 5 minutes (configurable)
//
// Privacy: 100% local. The user adds roots explicitly via the UI/slash
// command — Lucy never auto-indexes the home directory.

use crate::commands::metrics::shared_db;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ts() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgFile {
    pub path: String,
    pub extension: String,
    pub parent_dir: String,
    pub last_mtime: i64,
    pub first_seen: i64,
    pub touch_count: i64,
}

// Reserved for future use — once we expose a `kg_edges_list` command for
// graph visualization, the struct will materialize as a returned type.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgEdge {
    pub path_a: String,
    pub path_b: String,
    pub weight: i64,
    pub last_seen: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgNeighbor {
    pub path: String,
    pub weight: i64,
    pub extension: String,
    pub last_mtime: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgIndexResult {
    pub root: String,
    pub files_scanned: usize,
    pub files_recorded: usize,
    pub edges_added: usize,
    pub elapsed_ms: u64,
}

const CO_TOUCH_WINDOW_SEC: i64 = 5 * 60;   // 5 min window for co-modification
const MAX_FILES_PER_SCAN: usize = 5000;
const MAX_DEPTH: usize = 12;

const SKIP_DIRS: &[&str] = &[
    "node_modules", ".git", "target", "build", "dist", ".next",
    ".svelte-kit", "__pycache__", ".pytest_cache", "venv", ".venv",
    ".cargo", ".vscode", ".idea", "bin", "obj", "Debug", "Release",
];

const SKIP_EXTS: &[&str] = &[
    "tmp", "log", "lock", "pyc", "pyo", "o", "obj", "exe", "dll",
    "so", "dylib", "class", "swp", "swo", "DS_Store",
];

fn ensure_tables(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS kg_files (
            path        TEXT PRIMARY KEY,
            extension   TEXT NOT NULL DEFAULT '',
            parent_dir  TEXT NOT NULL DEFAULT '',
            last_mtime  INTEGER NOT NULL,
            first_seen  INTEGER NOT NULL,
            touch_count INTEGER NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_kg_files_mtime  ON kg_files(last_mtime DESC);
        CREATE INDEX IF NOT EXISTS idx_kg_files_pdir   ON kg_files(parent_dir);
        CREATE INDEX IF NOT EXISTS idx_kg_files_ext    ON kg_files(extension);

        CREATE TABLE IF NOT EXISTS kg_edges (
            path_a    TEXT NOT NULL,
            path_b    TEXT NOT NULL,
            weight    INTEGER NOT NULL DEFAULT 1,
            last_seen INTEGER NOT NULL,
            PRIMARY KEY (path_a, path_b)
        );
        CREATE INDEX IF NOT EXISTS idx_kg_edges_a ON kg_edges(path_a, weight DESC);
        CREATE INDEX IF NOT EXISTS idx_kg_edges_b ON kg_edges(path_b, weight DESC);

        CREATE TABLE IF NOT EXISTS kg_roots (
            root        TEXT PRIMARY KEY,
            added_at    INTEGER NOT NULL,
            last_scan   INTEGER
        );"
    ).map_err(|e| format!("kg create tables: {}", e))
}

fn should_skip_dir(name: &str) -> bool {
    if name.starts_with('.') { return true; }
    SKIP_DIRS.iter().any(|d| name.eq_ignore_ascii_case(d))
}

fn should_skip_ext(ext: &str) -> bool {
    SKIP_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e))
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Scan a single root, recording file metadata and co-touched edges.
fn scan_root_into(conn: &Connection, root_path: &Path, since: i64) -> Result<KgIndexResult, String> {
    let start = std::time::Instant::now();
    let mut files_scanned = 0usize;
    let mut files_recorded = 0usize;

    // Files touched in this scan pass, with their mtime — used to compute
    // co-touched edges after the walk.
    let mut touched: Vec<(String, i64)> = Vec::new();

    let mut stack: Vec<(PathBuf, usize)> = vec![(root_path.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_DEPTH { continue; }
        if files_scanned >= MAX_FILES_PER_SCAN { break; }

        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry_res in entries {
            if files_scanned >= MAX_FILES_PER_SCAN { break; }
            let entry = match entry_res { Ok(e) => e, Err(_) => continue };
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy().into_owned();
            let ft = match entry.file_type() { Ok(t) => t, Err(_) => continue };

            if ft.is_dir() {
                if should_skip_dir(&name_str) { continue; }
                stack.push((path, depth + 1));
                continue;
            }
            if !ft.is_file() { continue; }

            let extension = ext_of(&path);
            if should_skip_ext(&extension) { continue; }
            files_scanned += 1;

            let meta = match entry.metadata() { Ok(m) => m, Err(_) => continue };
            let mtime = meta.modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            // Skip files older than `since` — they didn't change in this window
            if mtime < since { continue; }

            let path_str = path.to_string_lossy().into_owned();
            let parent_dir = path.parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();

            // Upsert into kg_files
            conn.execute(
                "INSERT INTO kg_files (path, extension, parent_dir, last_mtime, first_seen, touch_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1)
                 ON CONFLICT(path) DO UPDATE SET
                    last_mtime  = excluded.last_mtime,
                    touch_count = kg_files.touch_count + 1",
                params![&path_str, &extension, &parent_dir, mtime, now_ts()],
            ).ok();

            touched.push((path_str, mtime));
            files_recorded += 1;
        }
    }

    // ── Co-touched edges ─────────────────────────────────────────────────
    // O(n²) over touched — but bounded by MAX_FILES_PER_SCAN. In practice
    // most scans touch <50 files (post-since filter).
    let mut edges_added = 0usize;
    let now = now_ts();
    for i in 0..touched.len() {
        for j in (i + 1)..touched.len() {
            if (touched[i].1 - touched[j].1).abs() > CO_TOUCH_WINDOW_SEC { continue; }
            // Order pair lexicographically so the PK is canonical
            let (a, b) = if touched[i].0 <= touched[j].0 {
                (&touched[i].0, &touched[j].0)
            } else {
                (&touched[j].0, &touched[i].0)
            };
            if a == b { continue; }
            conn.execute(
                "INSERT INTO kg_edges (path_a, path_b, weight, last_seen)
                 VALUES (?1, ?2, 1, ?3)
                 ON CONFLICT(path_a, path_b) DO UPDATE SET
                    weight    = kg_edges.weight + 1,
                    last_seen = excluded.last_seen",
                params![a, b, now],
            ).ok();
            edges_added += 1;
        }
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;
    Ok(KgIndexResult {
        root: root_path.to_string_lossy().into_owned(),
        files_scanned,
        files_recorded,
        edges_added,
        elapsed_ms,
    })
}

// ── Public commands ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn kg_add_root(root: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(&root);
    if !p.exists() { return Err(format!("path does not exist: {}", root)); }
    if !p.is_dir() { return Err(format!("not a directory: {}", root)); }
    shared_db(|conn| {
        ensure_tables(conn)?;
        conn.execute(
            "INSERT INTO kg_roots (root, added_at) VALUES (?1, ?2)
             ON CONFLICT(root) DO NOTHING",
            params![&root, now_ts()],
        ).map_err(|e| format!("insert root: {}", e))?;
        Ok(())
    })
}

#[tauri::command]
pub async fn kg_remove_root(root: String) -> Result<(), String> {
    shared_db(|conn| {
        ensure_tables(conn)?;
        conn.execute("DELETE FROM kg_roots WHERE root = ?1", params![&root])
            .map_err(|e| format!("delete root: {}", e))?;
        Ok(())
    })
}

#[tauri::command]
pub async fn kg_list_roots() -> Result<Vec<String>, String> {
    shared_db(|conn| {
        ensure_tables(conn)?;
        let mut stmt = conn.prepare("SELECT root FROM kg_roots ORDER BY added_at ASC")
            .map_err(|e| format!("prepare: {}", e))?;
        let rows: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgIndexBatchResult {
    pub roots: Vec<KgIndexResult>,
    pub total_files_scanned: usize,
    pub total_files_recorded: usize,
    pub total_edges: usize,
}

#[tauri::command]
pub async fn kg_index_now(since_min: Option<i64>) -> Result<KgIndexBatchResult, String> {
    let since_window = since_min.unwrap_or(60 * 24).clamp(1, 60 * 24 * 30);
    let since = now_ts() - since_window * 60;

    let roots = tokio::task::spawn_blocking(|| -> Result<Vec<String>, String> {
        shared_db(|conn| {
            ensure_tables(conn)?;
            let mut stmt = conn.prepare("SELECT root FROM kg_roots").map_err(|e| format!("prepare: {}", e))?;
            let rows: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| format!("query: {}", e))?
                .filter_map(|r| r.ok()).collect();
            Ok(rows)
        })
    }).await.map_err(|e| format!("join: {}", e))??;

    if roots.is_empty() {
        return Ok(KgIndexBatchResult {
            roots: vec![],
            total_files_scanned: 0,
            total_files_recorded: 0,
            total_edges: 0,
        });
    }

    tokio::task::spawn_blocking(move || {
        shared_db(|conn| {
            ensure_tables(conn)?;
            let mut results = Vec::new();
            let mut tot_scanned = 0;
            let mut tot_recorded = 0;
            let mut tot_edges = 0;
            for root in &roots {
                let p = std::path::Path::new(root);
                if !p.exists() { continue; }
                let r = scan_root_into(conn, p, since)?;
                tot_scanned += r.files_scanned;
                tot_recorded += r.files_recorded;
                tot_edges += r.edges_added;
                conn.execute(
                    "UPDATE kg_roots SET last_scan = ?1 WHERE root = ?2",
                    params![now_ts(), root],
                ).ok();
                results.push(r);
            }

            // Retention pass — runs once per batch (cheap because the indices
            // make these range deletes O(log n)). Without this, a power user
            // who edits hundreds of files per day accumulates GB of kg_edges
            // in a week. Caps:
            //   • kg_files: drop entries whose last_mtime is older than 90 days
            //     AND whose touch_count == 1 (one-touch files = noise).
            //   • kg_edges: drop edges whose last_seen is older than 60 days.
            //     Edges accumulate combinatorially; 60d is a sweet spot for
            //     "still relevant to current workflows".
            //   • Hard cap: if kg_files > 50k or kg_edges > 200k, prune the
            //     oldest 10% to prevent runaway disk growth.
            let retention_now = now_ts();
            let stale_file_cutoff = retention_now - 90 * 24 * 3600;
            let stale_edge_cutoff = retention_now - 60 * 24 * 3600;
            conn.execute(
                "DELETE FROM kg_files WHERE last_mtime < ?1 AND touch_count <= 1",
                params![stale_file_cutoff],
            ).ok();
            conn.execute(
                "DELETE FROM kg_edges WHERE last_seen < ?1",
                params![stale_edge_cutoff],
            ).ok();

            let file_count: i64 = conn.query_row("SELECT COUNT(*) FROM kg_files", [], |r| r.get(0)).unwrap_or(0);
            if file_count > 50_000 {
                // Drop the oldest-mtime 10% to bring it back to ~45k
                let to_drop = file_count / 10;
                conn.execute(
                    "DELETE FROM kg_files WHERE path IN
                     (SELECT path FROM kg_files ORDER BY last_mtime ASC LIMIT ?1)",
                    params![to_drop],
                ).ok();
            }
            let edge_count: i64 = conn.query_row("SELECT COUNT(*) FROM kg_edges", [], |r| r.get(0)).unwrap_or(0);
            if edge_count > 200_000 {
                let to_drop = edge_count / 10;
                conn.execute(
                    "DELETE FROM kg_edges WHERE rowid IN
                     (SELECT rowid FROM kg_edges ORDER BY weight ASC, last_seen ASC LIMIT ?1)",
                    params![to_drop],
                ).ok();
            }
            Ok(KgIndexBatchResult {
                roots: results,
                total_files_scanned: tot_scanned,
                total_files_recorded: tot_recorded,
                total_edges: tot_edges,
            })
        })
    }).await.map_err(|e| format!("join: {}", e))?
}

/// Sync version of kg_recent_files — callable from non-async contexts like
/// the PromptSection trait (which composes the system prompt synchronously).
/// shared_db() is already sync internally, so this is a thin wrapper that
/// skips the #[tauri::command] async ceremony.
///
/// Returns an empty Vec on any DB error (the prompt builder should NOT fail
/// just because the knowledge graph is unavailable — that's a non-essential
/// enhancement).
pub fn kg_recent_files_sync(dir_prefix: Option<String>, limit: Option<i64>) -> Vec<KgFile> {
    let cap = limit.unwrap_or(50).clamp(1, 500);
    shared_db(move |conn| {
        ensure_tables(conn)?;
        let (sql, like_arg) = match dir_prefix {
            Some(prefix) => (
                "SELECT path, extension, parent_dir, last_mtime, first_seen, touch_count
                 FROM kg_files WHERE path LIKE ?1
                 ORDER BY last_mtime DESC LIMIT ?2",
                Some(format!("{}%", prefix)),
            ),
            None => (
                "SELECT path, extension, parent_dir, last_mtime, first_seen, touch_count
                 FROM kg_files ORDER BY last_mtime DESC LIMIT ?1",
                None,
            ),
        };
        let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare: {}", e))?;
        let mapper = |row: &rusqlite::Row| {
            Ok(KgFile {
                path: row.get(0)?,
                extension: row.get(1)?,
                parent_dir: row.get(2)?,
                last_mtime: row.get(3)?,
                first_seen: row.get(4)?,
                touch_count: row.get(5)?,
            })
        };
        let rows: Vec<KgFile> = match &like_arg {
            Some(lk) => stmt.query_map(params![lk, cap], mapper),
            None     => stmt.query_map(params![cap], mapper),
        }.map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }).unwrap_or_default()
}

/// List files within or under a directory, ordered by last_mtime DESC.
#[tauri::command]
pub async fn kg_recent_files(dir_prefix: Option<String>, limit: Option<i64>) -> Result<Vec<KgFile>, String> {
    Ok(kg_recent_files_sync(dir_prefix, limit))
}

/// Return the strongest co-touched neighbors of a given path.
#[tauri::command]
pub async fn kg_neighbors(path: String, top_k: Option<i64>) -> Result<Vec<KgNeighbor>, String> {
    let cap = top_k.unwrap_or(10).clamp(1, 100);
    shared_db(move |conn| {
        ensure_tables(conn)?;
        // Edges may be stored with `path` as either side
        let mut stmt = conn.prepare(
            "SELECT CASE WHEN e.path_a = ?1 THEN e.path_b ELSE e.path_a END AS other,
                    e.weight,
                    COALESCE(f.extension, '') AS ext,
                    COALESCE(f.last_mtime, 0) AS mtime
             FROM kg_edges e
             LEFT JOIN kg_files f ON f.path = CASE WHEN e.path_a = ?1 THEN e.path_b ELSE e.path_a END
             WHERE e.path_a = ?1 OR e.path_b = ?1
             ORDER BY e.weight DESC, e.last_seen DESC
             LIMIT ?2"
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows: Vec<KgNeighbor> = stmt
            .query_map(params![&path, cap], |row| {
                Ok(KgNeighbor {
                    path: row.get(0)?,
                    weight: row.get(1)?,
                    extension: row.get(2)?,
                    last_mtime: row.get(3)?,
                })
            })
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionStat {
    pub extension: String,
    pub file_count: i64,
    pub total_touches: i64,
}

/// Summarize the extension distribution under a directory prefix.
#[tauri::command]
pub async fn kg_ext_summary(dir_prefix: Option<String>) -> Result<Vec<ExtensionStat>, String> {
    shared_db(move |conn| {
        ensure_tables(conn)?;
        let (sql, like_arg) = match dir_prefix {
            Some(p) => (
                "SELECT extension, COUNT(*) AS n, SUM(touch_count) AS t
                 FROM kg_files WHERE path LIKE ?1
                 GROUP BY extension ORDER BY n DESC LIMIT 30",
                Some(format!("{}%", p)),
            ),
            None => (
                "SELECT extension, COUNT(*) AS n, SUM(touch_count) AS t
                 FROM kg_files GROUP BY extension ORDER BY n DESC LIMIT 30",
                None,
            ),
        };
        let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare: {}", e))?;
        let mapper = |row: &rusqlite::Row| {
            Ok(ExtensionStat {
                extension: row.get(0)?,
                file_count: row.get(1)?,
                total_touches: row.get(2).unwrap_or(0),
            })
        };
        let rows: Vec<ExtensionStat> = match &like_arg {
            Some(lk) => stmt.query_map(params![lk], mapper),
            None     => stmt.query_map([], mapper),
        }.map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_dirs_blocks_node_modules() {
        assert!(should_skip_dir("node_modules"));
        assert!(should_skip_dir(".git"));
        assert!(should_skip_dir("target"));
        assert!(should_skip_dir(".vscode"));
    }

    #[test]
    fn skip_dirs_allows_normal_dirs() {
        assert!(!should_skip_dir("src"));
        assert!(!should_skip_dir("docs"));
        assert!(!should_skip_dir("my-project"));
    }

    #[test]
    fn skip_exts_filters_noise() {
        assert!(should_skip_ext("tmp"));
        assert!(should_skip_ext("log"));
        assert!(should_skip_ext("pyc"));
        assert!(!should_skip_ext("rs"));
        assert!(!should_skip_ext("ts"));
        assert!(!should_skip_ext("md"));
    }

    #[test]
    fn ext_of_handles_no_extension() {
        let p = std::path::PathBuf::from("/some/Makefile");
        assert_eq!(ext_of(&p), "");
    }

    #[test]
    fn ext_of_lowercases() {
        let p = std::path::PathBuf::from("/some/File.RS");
        assert_eq!(ext_of(&p), "rs");
    }
}
