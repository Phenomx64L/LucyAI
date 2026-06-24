use rusqlite::Connection;
use walkdir::WalkDir;
use std::path::PathBuf;
use std::time::Instant;
use tauri::{command, AppHandle};

fn get_db_path(app: &AppHandle) -> Result<PathBuf, String> {
    // Single shared DB file across metrics + indexer (FTS5 table coexists with metrics tables).
    crate::commands::metrics::db_path(app)
}

fn open_tuned(db_path: &PathBuf) -> Result<Connection, String> {
    let conn = Connection::open(db_path).map_err(|e| format!("open db: {}", e))?;
    // Performance pragmas: WAL + relaxed sync are safe for an index we can rebuild.
    let _ = conn.execute_batch(
        "PRAGMA journal_mode=WAL;\
         PRAGMA synchronous=NORMAL;\
         PRAGMA temp_store=MEMORY;\
         PRAGMA cache_size=-20000;",
    );
    Ok(conn)
}

fn ensure_schema(conn: &Connection) -> Result<(), String> {
    // FTS5 virtual table — substring search via MATCH is index-backed,
    // unlike LIKE '%foo%' which forces a full scan.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS file_index USING fts5(\
             name, path, tokenize='trigram'\
         );",
    ).map_err(|e| format!("create fts5: {}", e))
}

/// Directory component names pruned from the file-index walk. These are
/// system / build / dependency / cache trees that bloat the FTS5 index with
/// paths nobody searches for. (A v1.7.x DB audit found a 448 MB lucy.db whose
/// `file_index` had hit its 500k cap with 312k `C:\Program Files` binaries +
/// 40k `node_modules` + 40k `AppData` entries — ~80% of the DB was noise, and
/// the cap was crowding out the user's actual files.) Matched case-insensitively
/// against each directory's OWN component name, so a tree is pruned wherever it
/// appears. Exact-match (not substring) so `windows-tools` ≠ `windows`. (v1.7.224)
fn is_excluded_index_dir(dir_name: &str) -> bool {
    const EXCLUDED: &[&str] = &[
        // Windows system / recovery
        "$recycle.bin", "$windows.~bt", "$windows.~ws", "$winreagent",
        "$sysreset", "system volume information", "windows", "windows.old",
        "program files", "program files (x86)", "programdata", "perflogs",
        "onedrivetemp",
        // Per-user caches / toolchains / package stores. (.rustup alone held
        // 52k generated Rust-std HTML doc files in the audited DB.)
        "appdata", ".cache", ".npm", ".nuget", ".gradle", ".cargo",
        ".rustup", ".dotnet", ".conda", ".m2", ".vscode",
        // VCS internals + build / dependency output
        "node_modules", ".git", ".svn", ".hg", "target", "__pycache__",
        ".venv", "venv", "dist", ".next", ".turbo",
    ];
    let lower = dir_name.to_ascii_lowercase();
    EXCLUDED.iter().any(|&e| e == lower)
}

/// WalkDir `filter_entry` predicate: keep the entry?  Excluded DIRECTORIES are
/// pruned (their whole subtree is skipped — the point of doing this at the walk
/// layer rather than per-file). The chosen root is never pruned, even if its
/// own name is on the list (e.g. the user deliberately indexes a build dir).
/// Files always pass; the caller still filters to `is_file()`.
fn keep_index_entry(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() == 0 {
        return true; // never prune the explicitly chosen root
    }
    if entry.file_type().is_dir() {
        return !is_excluded_index_dir(&entry.file_name().to_string_lossy());
    }
    true
}

pub fn init_indexer(app: AppHandle, root_path: String) {
    tokio::task::spawn_blocking(move || {
        let db_path = match get_db_path(&app) {
            Ok(p) => p,
            Err(e) => { eprintln!("[indexer] {}", e); return; }
        };
        let mut conn = match open_tuned(&db_path) {
            Ok(c) => c,
            Err(e) => { eprintln!("[indexer] {}", e); return; }
        };
        if let Err(e) = ensure_schema(&conn) {
            eprintln!("[indexer] {}", e);
            return;
        }

        // Rebuild from scratch — FTS5 has no UNIQUE constraint, so deduping
        // would require a separate scan. A clean wipe per indexing run is fine
        // for the current single-root usage and avoids stale entries.
        if let Err(e) = conn.execute("DELETE FROM file_index", []) {
            eprintln!("[indexer] wipe: {}", e);
            return;
        }

        let tx = match conn.transaction() {
            Ok(t) => t,
            Err(e) => { eprintln!("[indexer] tx: {}", e); return; }
        };
        let mut count: u64 = 0;
        {
            let mut stmt = match tx.prepare("INSERT INTO file_index (name, path) VALUES (?1, ?2)") {
                Ok(s) => s,
                Err(e) => { eprintln!("[indexer] prepare: {}", e); return; }
            };

            for entry in WalkDir::new(&root_path)
                .into_iter()
                .filter_entry(keep_index_entry)   // prune system/build/dep trees
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file() {
                    let path_str = entry.path().to_string_lossy().to_string();
                    let name = entry.file_name().to_string_lossy().to_string();
                    if let Err(e) = stmt.execute([&name, &path_str]) {
                        eprintln!("[indexer] insert {}: {}", path_str, e);
                        continue;
                    }
                    count += 1;
                    if count >= 500_000 { break; }
                }
            }
        }
        if let Err(e) = tx.commit() {
            eprintln!("[indexer] commit: {}", e);
            return;
        }
        eprintln!("[indexer] indexed {} files from {}", count, root_path);
    });
}

#[command]
pub async fn locate_file(app: AppHandle, name: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let db_path = get_db_path(&app)?;
        let conn = open_tuned(&db_path)?;

        // Sanitize: FTS5 MATCH treats certain chars as operators. Quote the term.
        let term = name.replace('"', "");
        let query = format!("\"{}\"", term);

        let mut stmt = conn
            .prepare("SELECT path FROM file_index WHERE name MATCH ?1 LIMIT 100")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([&query], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;

        let results: Vec<String> = rows.flatten().collect();
        let elapsed_ms = started.elapsed().as_millis();

        if results.is_empty() {
            Ok(format!(
                "[No se encontró ningún archivo conteniendo '{}' en el índice de disco ({} ms)]",
                name, elapsed_ms
            ))
        } else {
            Ok(format!(
                "--- RESULTADO DE LOCATE ({} ms, {} archivos) ---\n{}",
                elapsed_ms,
                results.len(),
                results.join("\n")
            ))
        }
    })
    .await
    .map_err(|e| format!("Error del hilo: {}", e))?
}

#[command]
pub async fn start_indexer(app: AppHandle, path: String) -> Result<String, String> {
    init_indexer(app, path.clone());
    Ok(format!(
        "Recorriendo recursivamente e indexando archivos en segundo plano (SQLite FTS5) para: {}. Usa <TOOL>locate_file:name</TOOL> para probar.",
        path
    ))
}

#[cfg(test)]
mod tests {
    use super::is_excluded_index_dir;

    #[test]
    fn excludes_the_noise_trees_we_actually_found() {
        // The buckets the audit found filling the 500k cap.
        for d in ["Program Files", "Program Files (x86)", "Windows", "ProgramData",
                  "$Recycle.Bin", "node_modules", "AppData", ".git", "target",
                  "OneDriveTemp", "System Volume Information",
                  ".rustup", ".cargo", ".dotnet", ".m2", ".vscode"] {
            assert!(is_excluded_index_dir(d), "should exclude {d}");
        }
    }

    #[test]
    fn exclusion_is_case_insensitive() {
        assert!(is_excluded_index_dir("PROGRAM FILES"));
        assert!(is_excluded_index_dir("Node_Modules"));
        assert!(is_excluded_index_dir(".GIT"));
    }

    #[test]
    fn keeps_real_user_directories() {
        // Exact-match, so look-alikes and the user's own folders survive.
        for d in ["Documents", "Rust_Projects", "src", "lucy-svelte",
                  "windows-tools", "my-target-app", "git-repos", "appdata-viewer"] {
            assert!(!is_excluded_index_dir(d), "should NOT exclude {d}");
        }
    }
}
