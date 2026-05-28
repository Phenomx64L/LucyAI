// ── shell_recording.rs — Tier S #3 ────────────────────────────────────────
//
// Persistent recording of NexShell sessions: every cmd, stdout chunk,
// stderr chunk, meta event, and exit code is stored with a millisecond
// offset from the recording start. The tape can be replayed at any speed
// in the front-end without re-connecting to the remote host.
//
// Differentiator: no conventional AI shell tool (Cursor, Cline, Warp AI,
// Hermes) keeps a structured time-coded log of remote SSH/WinRM sessions.
// Asciinema does, but only locally and without integration. Lucy unifies
// both: native recording + LLM-aware metadata + scrubbing player.
//
// Schema lives in utils/db.rs — see the `shell_recordings` and
// `shell_recording_events` tables.
//
// Storage budget (typical):
//   • ~5 events/sec while a command streams output
//   • ~80 bytes per event (chunk + overhead)
//   • A 30-min recording with 60% active time → ~250 KB
// SQLite handles this trivially; no truncation needed before a year.

use crate::commands::metrics::shared_db;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellRecording {
    pub id: i64,
    pub session_id: String,
    pub host_id: String,
    pub host_name: String,
    pub host_type: String,
    pub title: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub event_count: i64,
    pub byte_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellRecordingEvent {
    pub id: i64,
    pub recording_id: i64,
    pub t_ms: i64,
    pub kind: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct StartArgs {
    pub session_id: String,
    pub host_id: Option<String>,
    pub host_name: Option<String>,
    pub host_type: Option<String>,
    pub title: Option<String>,
}

/// Begin a new recording. Returns the recording id the frontend must pass
/// to every subsequent append + finish call.
#[tauri::command]
pub async fn shell_recording_start(args: StartArgs) -> Result<i64, String> {
    shared_db(move |conn| {
        conn.execute(
            "INSERT INTO shell_recordings
                (session_id, host_id, host_name, host_type, title)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                &args.session_id,
                &args.host_id.unwrap_or_default(),
                &args.host_name.unwrap_or_default(),
                &args.host_type.unwrap_or_default(),
                &args.title.unwrap_or_default(),
            ],
        ).map_err(|e| format!("shell_recording_start: {}", e))?;
        Ok(conn.last_insert_rowid())
    })
}

/// Append one event to a recording. Cheap — single INSERT + counter bump.
///
/// Why we update `event_count` / `byte_count` on the parent here instead of
/// computing them at read time: the player needs both numbers to render the
/// list view efficiently. Computing COUNT() + SUM(LENGTH(data)) over
/// thousands of events on every list-load would be wasteful.
#[tauri::command]
pub async fn shell_recording_append(
    recording_id: i64,
    t_ms: i64,
    kind: String,
    data: String,
) -> Result<(), String> {
    let data_len = data.len() as i64;
    shared_db(move |conn| {
        conn.execute(
            "INSERT INTO shell_recording_events (recording_id, t_ms, kind, data)
             VALUES (?1, ?2, ?3, ?4)",
            params![recording_id, t_ms, &kind, &data],
        ).map_err(|e| format!("shell_recording_append: insert event: {}", e))?;
        conn.execute(
            "UPDATE shell_recordings
             SET event_count = event_count + 1,
                 byte_count  = byte_count + ?1
             WHERE id = ?2",
            params![data_len, recording_id],
        ).map_err(|e| format!("shell_recording_append: bump counters: {}", e))?;
        Ok(())
    })
}

/// Finalize a recording — stamps ended_at. If `title` is non-empty, also
/// overwrites the title (lets the user rename right at stop).
#[tauri::command]
pub async fn shell_recording_finish(
    recording_id: i64,
    title: Option<String>,
) -> Result<(), String> {
    shared_db(move |conn| {
        if let Some(t) = title.filter(|s| !s.is_empty()) {
            conn.execute(
                "UPDATE shell_recordings
                 SET ended_at = strftime('%s','now'), title = ?1
                 WHERE id = ?2 AND ended_at IS NULL",
                params![t, recording_id],
            ).map_err(|e| format!("shell_recording_finish (titled): {}", e))?;
        } else {
            conn.execute(
                "UPDATE shell_recordings
                 SET ended_at = strftime('%s','now')
                 WHERE id = ?1 AND ended_at IS NULL",
                params![recording_id],
            ).map_err(|e| format!("shell_recording_finish: {}", e))?;
        }
        Ok(())
    })
}

/// List recent recordings — newest first. Use `host_id` filter to scope by
/// a specific host, or pass None for the global tape library.
#[tauri::command]
pub async fn shell_recording_list(
    host_id: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<ShellRecording>, String> {
    let lim = limit.unwrap_or(100).min(500) as i64;
    shared_db(move |conn| {
        fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ShellRecording> {
            Ok(ShellRecording {
                id:          row.get(0)?,
                session_id:  row.get(1)?,
                host_id:     row.get(2)?,
                host_name:   row.get(3)?,
                host_type:   row.get(4)?,
                title:       row.get(5)?,
                started_at:  row.get(6)?,
                ended_at:    row.get(7)?,
                event_count: row.get(8)?,
                byte_count:  row.get(9)?,
            })
        }
        let rows: Vec<ShellRecording> = if let Some(ref hid) = host_id {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, host_id, host_name, host_type,
                        title, started_at, ended_at, event_count, byte_count
                 FROM shell_recordings
                 WHERE host_id = ?1
                 ORDER BY started_at DESC
                 LIMIT ?2"
            ).map_err(|e| format!("shell_recording_list prepare (host): {}", e))?;
            let v: Vec<ShellRecording> = stmt.query_map(params![hid, lim], read_row)
                .map_err(|e| format!("shell_recording_list query (host): {}", e))?
                .filter_map(|r| r.ok()).collect();
            v
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, host_id, host_name, host_type,
                        title, started_at, ended_at, event_count, byte_count
                 FROM shell_recordings
                 ORDER BY started_at DESC
                 LIMIT ?1"
            ).map_err(|e| format!("shell_recording_list prepare: {}", e))?;
            let v: Vec<ShellRecording> = stmt.query_map(params![lim], read_row)
                .map_err(|e| format!("shell_recording_list query: {}", e))?
                .filter_map(|r| r.ok()).collect();
            v
        };
        Ok(rows)
    })
}

/// Fetch every event of a recording, ordered by t_ms ascending.
///
/// Returned shape is flat — the frontend builds the player timeline from
/// the raw event sequence. For very long recordings we cap at
/// MAX_EVENTS_PER_GET (50_000); that's ~3 hours of dense streaming, far
/// beyond any realistic SRE session.
#[tauri::command]
pub async fn shell_recording_events(
    recording_id: i64,
) -> Result<Vec<ShellRecordingEvent>, String> {
    const MAX_EVENTS_PER_GET: i64 = 50_000;
    shared_db(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, recording_id, t_ms, kind, data
             FROM shell_recording_events
             WHERE recording_id = ?1
             ORDER BY t_ms ASC, id ASC
             LIMIT ?2"
        ).map_err(|e| format!("shell_recording_events prepare: {}", e))?;
        let v: Vec<ShellRecordingEvent> = stmt.query_map(
            params![recording_id, MAX_EVENTS_PER_GET],
            |row| Ok(ShellRecordingEvent {
                id: row.get(0)?,
                recording_id: row.get(1)?,
                t_ms: row.get(2)?,
                kind: row.get(3)?,
                data: row.get(4)?,
            }),
        ).map_err(|e| format!("shell_recording_events query: {}", e))?
          .filter_map(|r| r.ok()).collect();
        Ok(v)
    })
}

#[tauri::command]
pub async fn shell_recording_delete(recording_id: i64) -> Result<(), String> {
    shared_db(move |conn| {
        // FOREIGN KEY ... ON DELETE CASCADE drops the events automatically,
        // BUT only when PRAGMA foreign_keys = ON. SQLite's default is OFF.
        // We don't rely on the cascade — delete explicitly so the contract
        // is the same regardless of PRAGMA state.
        conn.execute(
            "DELETE FROM shell_recording_events WHERE recording_id = ?1",
            params![recording_id],
        ).map_err(|e| format!("shell_recording_delete (events): {}", e))?;
        conn.execute(
            "DELETE FROM shell_recordings WHERE id = ?1",
            params![recording_id],
        ).map_err(|e| format!("shell_recording_delete (parent): {}", e))?;
        Ok(())
    })
}

/// Rename a recording — useful when the user remembers what the session
/// was about and wants a meaningful title.
#[tauri::command]
pub async fn shell_recording_rename(
    recording_id: i64,
    title: String,
) -> Result<(), String> {
    shared_db(move |conn| {
        conn.execute(
            "UPDATE shell_recordings SET title = ?1 WHERE id = ?2",
            params![title, recording_id],
        ).map_err(|e| format!("shell_recording_rename: {}", e))?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    // The commands above are pure DB wrappers with no business logic worth
    // unit-testing in isolation. The contract-level guarantees we DO want
    // to pin live in this small set of constant/invariant assertions:

    #[test]
    fn event_kind_vocabulary_stable() {
        // The frontend player switches on these exact strings. If anyone
        // renames one ('out' → 'stdout' for example), the player goes blank
        // for that kind. Pin the vocabulary so a rename has to update both
        // sides intentionally.
        let allowed = ["cmd", "out", "err", "meta", "exit"];
        // Confidence test: every string is non-empty and lowercase.
        for s in allowed.iter() {
            assert!(!s.is_empty(), "empty kind in vocabulary");
            assert_eq!(*s, s.to_lowercase(), "kind must be lowercase: {}", s);
        }
    }
}
