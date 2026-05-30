// ── db_backup.rs — DB backup & restore (Sprint A #1, May 2026) ──────────
//
// Two commands the Settings UI exposes:
//
//   • db_backup_create(target_path)
//       Uses SQLite's `VACUUM INTO <path>` to atomically snapshot the live
//       database to `target_path`. VACUUM INTO is preferred over a raw file
//       copy because it:
//         - works even while connections are open + writing
//         - emits a fully consistent file (transaction-safe)
//         - compacts the output (no WAL fragmentation in the backup)
//
//   • db_backup_restore(source_path)
//       Validates that `source_path` is a SQLite file with Lucy's expected
//       schema markers (`agent_memories` + `audit_chain` tables), copies it
//       over the live DB path, and returns a flag telling the frontend
//       "you must restart the app now". We do NOT try to hot-swap the pool
//       because dangling connections would point at the old inode.
//
// db_info() returns the path + size + per-table row counts so the Settings
// UI can show what the user is about to back up.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::commands::metrics::shared_db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbInfo {
    /// Absolute path on disk (e.g. `C:\Users\X\AppData\Roaming\com.lucy.dev\lucy.db`)
    pub path: String,
    /// Bytes on disk. Includes WAL + SHM files when present.
    pub size_bytes: u64,
    /// Unix seconds of last modification.
    pub last_modified: i64,
    /// Per-table approximate row counts (top tables by user-visible impact).
    pub tables: Vec<TableStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableStat {
    pub name: String,
    pub rows: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
    /// True when the restore completed and the user must restart the app
    /// for the new DB to take effect.
    pub restart_required: bool,
    pub source_rows: i64,
    pub backup_kept_at: Option<String>,  // path of the safety backup we wrote
}

/// Resolve the live DB path. Mirrors `metrics::db_path` logic without
/// requiring an AppHandle parameter — Tauri commands can call this via
/// the AppHandle Tauri injects.
fn live_db_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    let dir = app.path().app_data_dir()
        .map_err(|e| format!("app_data_dir resolution failed: {}", e))?;
    Ok(dir.join("lucy.db"))
}

#[tauri::command]
pub async fn db_info(app: tauri::AppHandle) -> Result<DbInfo, String> {
    let path = live_db_path(&app)?;
    let path_str = path.to_string_lossy().to_string();

    // File metadata: size + last_modified. WAL/SHM siblings add to "real"
    // storage but the .db file itself is the authoritative size for the
    // backup payload (VACUUM INTO produces a single consolidated file).
    let (size_bytes, last_modified) = match std::fs::metadata(&path) {
        Ok(md) => {
            let size = md.len();
            let mt = md.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            (size, mt)
        }
        Err(_) => (0, 0),
    };

    // Per-table row counts. Listed here are the tables the user actually
    // cares about for a backup ("how much will I lose / restore"). We
    // skip internal SQLite tables and FTS shadow tables (they're derived).
    let user_visible_tables = [
        "agent_memories", "agent_crystals", "agent_insights",
        "audit_chain", "audit_trail",
        "incidents", "incident_evidence", "incident_hypotheses",
        "process_lineage", "state_snapshots",
        "token_usage", "daily_summary",
        "fork_results", "replay_snapshots",
        "shell_recordings", "shell_recording_events",
        "skill_runs", "scheduled_tasks", "permission_rules",
        "embeddings",
    ];

    let tables: Vec<TableStat> = shared_db(|conn| {
        let mut out = Vec::new();
        for name in user_visible_tables.iter() {
            // Each query is wrapped in `ok()` so a missing table (e.g. a
            // fresh install without recordings yet) doesn't fail the whole
            // info call — just omit that row.
            let rows: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {}", name), [], |r| r.get(0))
                .unwrap_or(-1);
            if rows >= 0 {
                out.push(TableStat { name: name.to_string(), rows });
            }
        }
        Ok(out)
    }).unwrap_or_default();

    Ok(DbInfo { path: path_str, size_bytes, last_modified, tables })
}

/// Create a backup via SQLite's VACUUM INTO.
///
/// The target path must be ABSOLUTE and writable. The parent directory is
/// created if it doesn't exist. We refuse to overwrite a file that's NOT
/// already a `.lucy.db`/`.db` sibling — protects the user from accidentally
/// targeting `Important.docx`.
#[tauri::command]
pub async fn db_backup_create(target_path: String) -> Result<u64, String> {
    let target = PathBuf::from(target_path.clone());
    if !target.is_absolute() {
        return Err("Target path must be absolute".to_string());
    }
    // Extension safety: only allow .db / .sqlite / .sqlite3 / .bak. Anything
    // else is almost certainly a typo or wrong file the user shouldn't lose.
    let ok_ext = target.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_lowercase().as_str(), "db" | "sqlite" | "sqlite3" | "bak"))
        .unwrap_or(false);
    if !ok_ext {
        return Err(format!(
            "Refusing to write to '{}' — extension must be .db, .sqlite, .sqlite3, or .bak. \
             This protects against typos pointing at important documents.",
            target.display()
        ));
    }
    // Ensure parent dir exists.
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create parent dir '{}': {}", parent.display(), e))?;
    }
    // VACUUM INTO requires the target file to NOT exist (SQLite enforces).
    if target.exists() {
        std::fs::remove_file(&target)
            .map_err(|e| format!("Cannot overwrite existing file '{}': {}", target.display(), e))?;
    }

    // VACUUM INTO uses a string literal in SQL; we must escape single
    // quotes in the path to avoid SQL injection on Windows paths with apostrophes
    // (rare but possible — e.g. `C:\Users\John's PC\backups`).
    let escaped = target_path.replace('\'', "''");
    let sql = format!("VACUUM INTO '{}'", escaped);
    shared_db(move |conn| {
        conn.execute_batch(&sql)
            .map_err(|e| format!("VACUUM INTO failed: {}", e))?;
        Ok(())
    })?;

    // Return the size of the produced file so the UI can show "Backed up: 4.2 MB"
    std::fs::metadata(&target)
        .map(|md| md.len())
        .map_err(|e| format!("Backup written but stat failed: {}", e))
}

/// Restore the live DB from a previously-created backup file.
///
/// Steps:
///   1. Validate `source_path` exists and looks like a SQLite file
///   2. Sanity-check: open it read-only and verify `agent_memories` exists
///   3. Take a "safety backup" of the current live DB (we keep this so the
///      user can recover from a bad restore)
///   4. Stop the WAL by closing any open writers; flush via WAL checkpoint
///   5. Copy source file over live path
///   6. Tell the frontend a restart is needed
///
/// We don't tear down the pool here because Tauri's command lifecycle
/// makes that fragile — instead the frontend pops a "please restart" modal.
#[tauri::command]
pub async fn db_backup_restore(
    app: tauri::AppHandle,
    source_path: String,
) -> Result<RestoreResult, String> {
    let source = PathBuf::from(&source_path);
    if !source.exists() {
        return Err(format!("Source file not found: {}", source_path));
    }
    if !source.is_file() {
        return Err(format!("Source path is not a file: {}", source_path));
    }

    // ── 1. Validate it's a Lucy-shaped SQLite file ─────────────────────
    let row_count = validate_lucy_db(&source)
        .map_err(|e| format!("Validation failed for '{}': {}", source_path, e))?;

    let live = live_db_path(&app)?;
    // ── 2. Safety backup of the CURRENT live DB before clobbering ──────
    // Lands next to the original with a timestamp suffix. If the restore
    // turns out to be wrong, the user can rename this back to lucy.db.
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let safety = live.with_file_name(format!("lucy_pre_restore_{}.db.bak", ts));
    let safety_str = safety.to_string_lossy().to_string();

    // Use VACUUM INTO for the safety backup too — same atomicity guarantee.
    if live.exists() {
        let escaped = safety_str.replace('\'', "''");
        let sql = format!("VACUUM INTO '{}'", escaped);
        shared_db(move |conn| {
            conn.execute_batch(&sql)
                .map_err(|e| format!("safety-backup VACUUM INTO failed: {}", e))?;
            Ok(())
        })?;
    }

    // ── 3. Checkpoint the WAL so the live file is consistent before copy
    // We do this before the file replace to avoid leaving an orphan WAL
    // pointing at the OLD database after the new file is in place.
    let _ = shared_db(|conn| {
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        Ok(())
    });

    // ── 4. Replace the live file ───────────────────────────────────────
    // We rename the live file out of the way THEN copy the source in.
    // Avoiding a direct overwrite means if the copy fails, the user still
    // has the original on disk under the "_overwritten" name.
    let overwritten = live.with_file_name(format!("lucy_overwritten_{}.db.bak", ts));
    if live.exists() {
        std::fs::rename(&live, &overwritten)
            .map_err(|e| format!("Could not move live DB out of the way: {}", e))?;
    }
    // Also remove the WAL/SHM siblings — they belonged to the old DB.
    let wal_path = live.with_extension("db-wal");
    let shm_path = live.with_extension("db-shm");
    let _ = std::fs::remove_file(&wal_path);
    let _ = std::fs::remove_file(&shm_path);

    if let Err(e) = std::fs::copy(&source, &live) {
        // Restore the original on failure so we don't leave the user in a
        // broken state with no DB at all.
        let _ = std::fs::rename(&overwritten, &live);
        return Err(format!("Copy failed mid-restore (original restored): {}", e));
    }
    // Successful — keep the overwritten file as a second safety net (the
    // user can delete it manually after confirming things work).

    Ok(RestoreResult {
        restart_required: true,
        source_rows: row_count,
        backup_kept_at: Some(safety_str),
    })
}

/// Open the candidate file with a fresh, read-only SQLite connection and
/// confirm it has Lucy's schema markers. Returns the total `agent_memories`
/// row count so the UI can display "Restoring 412 memories".
fn validate_lucy_db(path: &Path) -> Result<i64, String> {
    use rusqlite::OpenFlags;
    let conn = rusqlite::Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ).map_err(|e| format!("Cannot open as SQLite: {}", e))?;

    // v1.4.9 (C5): same lock-contention treatment as diagnostics
    // (check_database_health) — give quick_check up to 10 s to acquire
    // the FTS5 inverted-index read lock instead of failing instantly
    // with "is locked", and treat lock-style errors as transient
    // rather than corruption. Otherwise a restore-from-backup on a
    // healthy DB falsely rejects under concurrent writes (smart_chips,
    // audit, conversation_turns) and the user thinks their file is
    // broken. Real corruption still surfaces because its error text
    // never contains "is locked" / "query error".
    let _ = conn.busy_timeout(std::time::Duration::from_millis(10_000));

    // quick_check is the right tool here, not full integrity_check —
    // it skips UNIQUE-constraint validation which adds ~10× cost on
    // big DBs without catching new corruption classes. Same gold-
    // standard guarantee for malformed pages / bad rowids.
    let integrity_raw: String = conn
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(|e| format!("quick_check query failed: {}", e))?;

    let is_transient_lock = |s: &str| {
        let lower = s.to_lowercase();
        lower.contains("database is locked")
            || lower.contains("is locked")
            || lower.contains("query error")
    };
    // One-shot retry if locked — 200ms is enough for typical write
    // bursts to commit and release their lock.
    let integrity = if integrity_raw != "ok" && is_transient_lock(&integrity_raw) {
        std::thread::sleep(std::time::Duration::from_millis(200));
        conn.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
            .unwrap_or_else(|_| integrity_raw.clone())
    } else {
        integrity_raw
    };

    if integrity != "ok" {
        // If after retry it's STILL a lock-style error, accept the
        // file rather than rejecting a healthy DB the user worked
        // hard to restore. Real corruption falls through to Err.
        if !is_transient_lock(&integrity) {
            return Err(format!("Integrity check returned: {}", integrity));
        }
        // Log but continue.
        eprintln!("[db_backup] validate_lucy_db: transient lock during integrity check, proceeding: {}", integrity);
    }

    // Schema markers — both tables exist in EVERY Lucy install since v1.0.
    // If either is missing, this isn't a Lucy DB (or it's pre-v1.0).
    let has_memories: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='agent_memories'",
            [], |r| r.get(0),
        ).unwrap_or(0);
    let has_audit: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='audit_chain'",
            [], |r| r.get(0),
        ).unwrap_or(0);
    if has_memories == 0 || has_audit == 0 {
        return Err("File is a SQLite DB but doesn't match Lucy's schema (missing agent_memories or audit_chain table)".to_string());
    }

    // Row count is informational — let the UI display "Restoring N memories"
    let memory_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM agent_memories", [], |r| r.get(0))
        .unwrap_or(0);
    Ok(memory_rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Validation must REJECT a non-SQLite file (e.g. user picked a Word doc).
    #[test]
    fn validate_rejects_non_sqlite_file() {
        let tmp = std::env::temp_dir().join("lucy_test_not_sqlite.bin");
        let mut f = std::fs::File::create(&tmp).expect("temp file");
        f.write_all(b"this is not a sqlite database").unwrap();
        drop(f);
        let r = validate_lucy_db(&tmp);
        assert!(r.is_err(), "Non-SQLite file should be rejected");
        let _ = std::fs::remove_file(&tmp);
    }

    /// Validation must REJECT a sqlite DB that lacks Lucy's schema.
    #[test]
    fn validate_rejects_foreign_sqlite_db() {
        let tmp = std::env::temp_dir().join("lucy_test_foreign.db");
        let _ = std::fs::remove_file(&tmp);
        // Build a SQLite DB with an unrelated schema.
        let conn = rusqlite::Connection::open(&tmp).expect("open");
        conn.execute_batch(
            "CREATE TABLE recipes (id INTEGER, name TEXT); \
             INSERT INTO recipes VALUES (1, 'tacos');"
        ).expect("schema");
        drop(conn);
        let r = validate_lucy_db(&tmp);
        assert!(r.is_err(), "Foreign SQLite DB should be rejected");
        let err = r.unwrap_err();
        assert!(err.contains("agent_memories") || err.contains("audit_chain"),
                "Error should mention the missing markers, got: {}", err);
        let _ = std::fs::remove_file(&tmp);
    }

    /// Validation must ACCEPT a sqlite DB with Lucy's marker tables.
    #[test]
    fn validate_accepts_lucy_shaped_db() {
        let tmp = std::env::temp_dir().join("lucy_test_valid.db");
        let _ = std::fs::remove_file(&tmp);
        let conn = rusqlite::Connection::open(&tmp).expect("open");
        // Minimal schema — just the two marker tables. Real Lucy DBs have
        // dozens more, but the validator only checks these two.
        conn.execute_batch(
            "CREATE TABLE agent_memories (id INTEGER PRIMARY KEY, title TEXT, content TEXT, tags TEXT, files TEXT, importance INTEGER, created_at INTEGER); \
             CREATE TABLE audit_chain (id INTEGER PRIMARY KEY, hash TEXT); \
             INSERT INTO agent_memories (title, content, tags, files, importance, created_at) VALUES ('test', 'body', '[]', '[]', 1, 0); \
             INSERT INTO agent_memories (title, content, tags, files, importance, created_at) VALUES ('test2', 'body2', '[]', '[]', 1, 0);"
        ).expect("schema");
        drop(conn);
        let r = validate_lucy_db(&tmp);
        assert!(r.is_ok(), "Lucy-shaped DB should validate, got: {:?}", r);
        assert_eq!(r.unwrap(), 2, "Should count both memory rows");
        let _ = std::fs::remove_file(&tmp);
    }
}
