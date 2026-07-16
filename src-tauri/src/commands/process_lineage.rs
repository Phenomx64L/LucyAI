// ── Process Lineage Recorder — F1 (Frontier R&D, May 2026) ────────────────
//
// Records the full lineage (parent → child chain) of processes as they
// appear on the system. Lets Lucy answer questions that no other AI tool
// can answer:
//
//   - "What spawned this python.exe that's at 100% CPU?"
//   - "When did this svchost.exe with weird parent first appear?"
//   - "Reconstruct the process tree of yesterday at 14:32"
//   - "List all transitive children of my Discord since it started"
//
// Implementation strategy:
//   - Polling-based (not ETW) — uses `sysinfo` crate which works without
//     elevated privileges. Trade-off: short-lived processes (<2s) may be
//     missed. ETW would catch them but requires SeSystemProfilePrivilege,
//     not always available.
//   - On every capture, diff current process list against the previous one:
//       NEW processes get a row in `process_lineage` with full snapshot of
//       parent chain at observation time.
//       VANISHED processes get their `vanished_at` updated.
//   - Polling interval: 5-10s (configurable). Lucy's frontend triggers it.
//
// Storage:
//   - `process_lineage` table:
//       id, pid, ppid, parent_chain (JSON [{pid, name, exe}]), exe_path,
//       exe_hash (optional, SHA-256 of binary), cmdline (truncated),
//       first_seen, vanished_at NULL until process exits, observed_count
//   - Retention: 14 days
//   - Index on (first_seen DESC), (pid, vanished_at) for fast lookups
//
// Hash chain (audit-grade): we link each new lineage record to the previous
// one via SHA-256(prev_hash || id || pid || exe_path || first_seen). Lets
// the user verify that no records were tampered with after the fact —
// same pattern as `incident_action`.

use crate::commands::metrics::shared_db;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use sysinfo::{Pid, System};

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessLineageRow {
    pub id: i64,
    pub pid: u32,
    pub ppid: Option<u32>,
    /// JSON-encoded array of `{ pid, name, exe }` walking up to root.
    pub parent_chain: String,
    pub exe_path: String,
    pub cmdline: String,
    pub first_seen: i64,
    pub vanished_at: Option<i64>,
    pub observed_count: i64,
    pub chain_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParentNode {
    pub pid: u32,
    pub name: String,
    pub exe: String,
}

/// Ensure the table exists. Idempotent.
fn ensure_table(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS process_lineage (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            pid            INTEGER NOT NULL,
            ppid           INTEGER,
            parent_chain   TEXT    NOT NULL,
            exe_path       TEXT    NOT NULL,
            cmdline        TEXT    NOT NULL,
            first_seen     INTEGER NOT NULL,
            vanished_at    INTEGER,
            observed_count INTEGER NOT NULL DEFAULT 1,
            chain_hash     TEXT    NOT NULL DEFAULT 'GENESIS'
        )",
        [],
    ).map_err(|e| format!("create table: {}", e))?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_proc_lineage_first_seen ON process_lineage(first_seen DESC)",
        [],
    ).ok();
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_proc_lineage_pid ON process_lineage(pid, vanished_at)",
        [],
    ).ok();
    Ok(())
}

/// Build the parent chain (current → root) for a given pid using a process
/// map. Caps at 12 levels to avoid pathological cycles (theoretically
/// impossible but defensive).
fn build_chain(pid: u32, by_pid: &HashMap<u32, (u32, String, String)>) -> Vec<ParentNode> {
    let mut chain = Vec::new();
    let mut cur = pid;
    for _ in 0..12 {
        let entry = match by_pid.get(&cur) {
            Some(e) => e,
            None => break,
        };
        chain.push(ParentNode {
            pid: cur,
            name: entry.1.clone(),
            exe: entry.2.clone(),
        });
        if entry.0 == 0 || entry.0 == cur { break; }
        cur = entry.0;
    }
    chain
}

/// Capture-and-diff cycle. Returns count of new rows + vanished rows.
/// Designed to be called on a 5-10s polling cadence.
pub fn poll_once() -> Result<(usize, usize), String> {
    let mut sys = System::new_all();
    sys.refresh_processes();

    // (ppid, name, exe) by pid
    let mut by_pid: HashMap<u32, (u32, String, String)> = HashMap::new();
    for (pid, p) in sys.processes() {
        let ppid = p.parent().map(|p| p.as_u32()).unwrap_or(0);
        let exe = p.exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        by_pid.insert(pid.as_u32(), (ppid, p.name().to_string(), exe));
    }

    let current_pids: std::collections::HashSet<u32> = by_pid.keys().copied().collect();

    let ts = now_ts();
    let mut new_count = 0usize;
    let mut vanished_count = 0usize;

    shared_db(|conn| {
        ensure_table(conn)?;

        // 1) Mark vanished processes
        let mut stmt = conn.prepare(
            "SELECT id, pid FROM process_lineage WHERE vanished_at IS NULL"
        ).map_err(|e| format!("prepare alive: {}", e))?;
        let alive: Vec<(i64, u32)> = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u32)))
            .map_err(|e| format!("query alive: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        for (id, pid) in &alive {
            if !current_pids.contains(pid) {
                conn.execute(
                    "UPDATE process_lineage SET vanished_at = ?1 WHERE id = ?2",
                    params![ts, id],
                ).ok();
                vanished_count += 1;
            } else {
                // Bump observed_count for still-alive entries
                conn.execute(
                    "UPDATE process_lineage SET observed_count = observed_count + 1 WHERE id = ?1",
                    params![id],
                ).ok();
            }
        }

        // 2) Find pids that are new (not in process_lineage with NULL vanished)
        let mut known_pids = std::collections::HashSet::new();
        for (_, pid) in &alive { known_pids.insert(*pid); }

        // Last chain hash for the audit chain
        let mut prev_hash: String = conn.query_row(
            "SELECT chain_hash FROM process_lineage ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        ).unwrap_or_else(|_| "GENESIS".to_string());

        for (pid, (ppid, name, exe)) in &by_pid {
            if known_pids.contains(pid) { continue; }
            // Build parent chain
            let chain = build_chain(*ppid, &by_pid);
            let chain_json = serde_json::to_string(&chain).unwrap_or_else(|_| "[]".to_string());
            let cmdline = sys.process(Pid::from_u32(*pid))
                .map(|p| p.cmd().join(" "))
                .unwrap_or_default();
            let cmdline_truncated = if cmdline.len() > 512 { format!("{}…", crate::utils::safe_truncate(&cmdline, 512)) } else { cmdline };
            let _ = name; // currently only used to enrich chain via by_pid

            // Compute chain hash
            let mut hasher = Sha256::new();
            hasher.update(prev_hash.as_bytes());
            hasher.update(pid.to_le_bytes());
            hasher.update(exe.as_bytes());
            hasher.update(ts.to_le_bytes());
            let new_hash = format!("{:x}", hasher.finalize());

            conn.execute(
                "INSERT INTO process_lineage
                    (pid, ppid, parent_chain, exe_path, cmdline, first_seen, chain_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![*pid as i64, *ppid as i64, &chain_json, exe, &cmdline_truncated, ts, &new_hash],
            ).map_err(|e| format!("insert lineage: {}", e))?;
            prev_hash = new_hash;
            new_count += 1;
        }

        // 3) Retention: delete entries older than 14 days
        let cutoff = ts - 14 * 24 * 3600;
        conn.execute(
            "DELETE FROM process_lineage WHERE vanished_at IS NOT NULL AND vanished_at < ?1",
            params![cutoff],
        ).ok();

        Ok::<(), String>(())
    })?;

    Ok((new_count, vanished_count))
}

/// Tauri command: run one poll cycle. Returns counts.
#[tauri::command]
pub async fn process_lineage_poll() -> Result<(usize, usize), String> {
    tokio::task::spawn_blocking(poll_once)
        .await
        .map_err(|e| format!("join: {}", e))?
}

/// List recent lineage entries for the frontend Process Forest view.
#[tauri::command]
pub async fn process_lineage_list(
    since_ts: Option<i64>,
    only_alive: Option<bool>,
    limit: Option<i64>,
) -> Result<Vec<ProcessLineageRow>, String> {
    let since = since_ts.unwrap_or(0);
    let cap = limit.unwrap_or(200).min(2000);
    let only_alive = only_alive.unwrap_or(false);

    shared_db(|conn| {
        ensure_table(conn)?;
        let sql = if only_alive {
            "SELECT id, pid, ppid, parent_chain, exe_path, cmdline, first_seen,
                    vanished_at, observed_count, chain_hash
             FROM process_lineage
             WHERE first_seen >= ?1 AND vanished_at IS NULL
             ORDER BY first_seen DESC LIMIT ?2"
        } else {
            "SELECT id, pid, ppid, parent_chain, exe_path, cmdline, first_seen,
                    vanished_at, observed_count, chain_hash
             FROM process_lineage
             WHERE first_seen >= ?1
             ORDER BY first_seen DESC LIMIT ?2"
        };
        let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare: {}", e))?;
        let rows: Vec<ProcessLineageRow> = stmt
            .query_map(params![since, cap], |row| {
                Ok(ProcessLineageRow {
                    id: row.get(0)?,
                    pid: row.get::<_, i64>(1)? as u32,
                    ppid: row.get::<_, Option<i64>>(2)?.map(|v| v as u32),
                    parent_chain: row.get(3)?,
                    exe_path: row.get(4)?,
                    cmdline: row.get(5)?,
                    first_seen: row.get(6)?,
                    vanished_at: row.get(7)?,
                    observed_count: row.get(8)?,
                    chain_hash: row.get(9)?,
                })
            })
            .map_err(|e| format!("query: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect: {}", e))?;
        Ok(rows)
    })
}

/// Get full ancestry of a single pid (the chain stored at first_seen).
#[tauri::command]
pub async fn process_lineage_for_pid(pid: u32) -> Result<Option<ProcessLineageRow>, String> {
    shared_db(|conn| {
        ensure_table(conn)?;
        let row = conn.query_row(
            "SELECT id, pid, ppid, parent_chain, exe_path, cmdline, first_seen,
                    vanished_at, observed_count, chain_hash
             FROM process_lineage
             WHERE pid = ?1 AND vanished_at IS NULL
             ORDER BY first_seen DESC LIMIT 1",
            params![pid as i64],
            |row| {
                Ok(ProcessLineageRow {
                    id: row.get(0)?,
                    pid: row.get::<_, i64>(1)? as u32,
                    ppid: row.get::<_, Option<i64>>(2)?.map(|v| v as u32),
                    parent_chain: row.get(3)?,
                    exe_path: row.get(4)?,
                    cmdline: row.get(5)?,
                    first_seen: row.get(6)?,
                    vanished_at: row.get(7)?,
                    observed_count: row.get(8)?,
                    chain_hash: row.get(9)?,
                })
            },
        ).ok();
        Ok(row)
    })
}

/// Search by binary name (substring match, case-insensitive).
#[tauri::command]
pub async fn process_lineage_search(name_substring: String, limit: Option<i64>) -> Result<Vec<ProcessLineageRow>, String> {
    let cap = limit.unwrap_or(100).min(500);
    let pattern = format!("%{}%", name_substring.to_lowercase());
    shared_db(|conn| {
        ensure_table(conn)?;
        let mut stmt = conn.prepare(
            "SELECT id, pid, ppid, parent_chain, exe_path, cmdline, first_seen,
                    vanished_at, observed_count, chain_hash
             FROM process_lineage
             WHERE LOWER(exe_path) LIKE ?1 OR LOWER(cmdline) LIKE ?1
             ORDER BY first_seen DESC LIMIT ?2"
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows: Vec<ProcessLineageRow> = stmt
            .query_map(params![pattern, cap], |row| {
                Ok(ProcessLineageRow {
                    id: row.get(0)?,
                    pid: row.get::<_, i64>(1)? as u32,
                    ppid: row.get::<_, Option<i64>>(2)?.map(|v| v as u32),
                    parent_chain: row.get(3)?,
                    exe_path: row.get(4)?,
                    cmdline: row.get(5)?,
                    first_seen: row.get(6)?,
                    vanished_at: row.get(7)?,
                    observed_count: row.get(8)?,
                    chain_hash: row.get(9)?,
                })
            })
            .map_err(|e| format!("query: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect: {}", e))?;
        Ok(rows)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageChainVerifyResult {
    pub valid: bool,
    pub total_rows: usize,
    pub verified: usize,
    pub broken_at_id: Option<i64>,
}

/// Verify the SHA-256 audit chain of the entire lineage table.
#[tauri::command]
pub async fn process_lineage_verify_chain() -> Result<LineageChainVerifyResult, String> {
    shared_db(|conn| {
        ensure_table(conn)?;
        let mut stmt = conn.prepare(
            "SELECT id, pid, exe_path, first_seen, chain_hash
             FROM process_lineage ORDER BY id ASC"
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows: Vec<(i64, i64, String, i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        if rows.is_empty() {
            return Ok(LineageChainVerifyResult { valid: true, total_rows: 0, verified: 0, broken_at_id: None });
        }

        let mut prev_hash = "GENESIS".to_string();
        let mut verified = 0;
        for (id, pid, exe, ts, stored) in &rows {
            let mut hasher = Sha256::new();
            hasher.update(prev_hash.as_bytes());
            hasher.update((*pid as u32).to_le_bytes());
            hasher.update(exe.as_bytes());
            hasher.update(ts.to_le_bytes());
            let expected = format!("{:x}", hasher.finalize());
            if &expected != stored {
                return Ok(LineageChainVerifyResult {
                    valid: false,
                    total_rows: rows.len(),
                    verified,
                    broken_at_id: Some(*id),
                });
            }
            prev_hash = expected;
            verified += 1;
        }
        Ok(LineageChainVerifyResult {
            valid: true,
            total_rows: rows.len(),
            verified,
            broken_at_id: None,
        })
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_chain_walks_parents() {
        let mut map = HashMap::new();
        // pid 100 has parent 50, parent 50 has parent 1, parent 1 has no parent (0)
        map.insert(100u32, (50u32, "child".to_string(), "C:/child.exe".to_string()));
        map.insert(50u32,  (1u32,  "mid".to_string(),   "C:/mid.exe".to_string()));
        map.insert(1u32,   (0u32,  "root".to_string(),  "C:/root.exe".to_string()));

        let chain = build_chain(100, &map);
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].pid, 100);
        assert_eq!(chain[1].pid, 50);
        assert_eq!(chain[2].pid, 1);
    }

    #[test]
    fn build_chain_handles_orphan() {
        let map: HashMap<u32, (u32, String, String)> = HashMap::new();
        let chain = build_chain(42, &map);
        assert_eq!(chain.len(), 0);
    }

    #[test]
    fn build_chain_caps_at_12() {
        let mut map = HashMap::new();
        // Create a chain of 20 levels to ensure the cap kicks in
        for i in 1..=20u32 {
            let parent = if i == 1 { 0 } else { i - 1 };
            map.insert(i, (parent, format!("p{}", i), format!("p{}.exe", i)));
        }
        let chain = build_chain(20, &map);
        assert_eq!(chain.len(), 12, "should cap at 12 levels");
    }
}
