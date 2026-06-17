// ── scheduled.rs — natural-language cron tasks (Hermes-inspired)
//
// Each Scheduled Task is a piece of natural-language work the user wants to
// repeat: "every morning at 9:00, generate a system-health report",
// "every Monday at 8:00, audit user logons from the past week", etc.
//
// MVP scope (this module):
//   • CRUD: create / list / update / delete / toggle.
//   • Helper to find which tasks are "due" right now so the UI or a future
//     background ticker can dispatch them.
//   • Tiny cron-like expression parser supporting the common subset:
//        "*"          every (minute | hour | day | dow | month)
//        "N"          exact value
//        "N-M"        range
//        "*/N"        every-N step (only on the field's range)
//        "N,M,O"      comma list
//
// What's NOT in this module yet (deliberate, future PR):
//   • The background ticker that actually fires due tasks. That requires a
//     tokio actor or app-startup hook beyond a single-file scope. For now
//     the frontend can call `due_scheduled_tasks` periodically and dispatch
//     each result through the existing askLucyStream pipeline.
//   • Skipped/missed handling. If the app was offline when a job was due,
//     the simple policy is "run once on next wake" — implementable in the
//     ticker.

use serde::{Deserialize, Serialize};
use crate::commands::metrics::shared_db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id:           i64,
    pub name:         String,
    pub prompt:       String,
    pub cron_expr:    Option<String>,
    pub next_run:     i64,
    pub last_run:     Option<i64>,
    pub last_status:  Option<String>,
    pub last_output:  Option<String>,
    pub enabled:      bool,
    pub created_at:   i64,
    pub updated_at:   i64,
}

/// Save a new scheduled task. `cron_expr` is optional — if omitted, the task
/// is one-shot (after running once, it auto-disables).
#[tauri::command]
pub fn save_scheduled_task(
    name:      String,
    prompt:    String,
    cron_expr: Option<String>,
    next_run:  i64,
) -> Result<i64, String> {
    if name.trim().is_empty() || prompt.trim().is_empty() {
        return Err("save_scheduled_task: name and prompt required".to_string());
    }
    if name.len() > 120 || prompt.len() > 4000 {
        return Err("save_scheduled_task: name <= 120, prompt <= 4000".to_string());
    }
    if let Some(ref e) = cron_expr {
        // Validate so we fail fast.
        validate_cron(e).map_err(|err| format!("invalid cron_expr: {}", err))?;
    }
    if next_run < 0 {
        return Err("save_scheduled_task: next_run must be unix epoch >= 0".to_string());
    }
    shared_db(|conn| {
        conn.execute(
            "INSERT INTO scheduled_tasks (name, prompt, cron_expr, next_run)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![name, prompt, cron_expr, next_run],
        ).map_err(|e| format!("save_scheduled_task: {}", e))?;
        Ok(conn.last_insert_rowid())
    })
}

/// Return all scheduled tasks ordered by next_run.
#[tauri::command]
pub fn list_scheduled_tasks() -> Result<Vec<ScheduledTask>, String> {
    shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, prompt, cron_expr, next_run, last_run, last_status,
                    last_output, enabled, created_at, updated_at
             FROM scheduled_tasks
             ORDER BY enabled DESC, next_run ASC, id ASC"
        ).map_err(|e| format!("list_scheduled_tasks prepare: {}", e))?;
        let rows = stmt.query_map([], |row| {
            Ok(ScheduledTask {
                id:           row.get(0)?,
                name:         row.get(1)?,
                prompt:       row.get(2)?,
                cron_expr:    row.get(3)?,
                next_run:     row.get(4)?,
                last_run:     row.get(5)?,
                last_status:  row.get(6)?,
                last_output:  row.get(7)?,
                enabled:      row.get::<_, i64>(8)? != 0,
                created_at:   row.get(9)?,
                updated_at:   row.get(10)?,
            })
        }).map_err(|e| format!("list_scheduled_tasks query: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("list_scheduled_tasks row: {}", e))?);
        }
        Ok(out)
    })
}

/// Return scheduled tasks whose next_run is <= now AND enabled = 1.
/// Frontend polls this every minute or two and dispatches each.
#[tauri::command]
pub fn due_scheduled_tasks() -> Result<Vec<ScheduledTask>, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, prompt, cron_expr, next_run, last_run, last_status,
                    last_output, enabled, created_at, updated_at
             FROM scheduled_tasks
             WHERE enabled = 1 AND next_run <= ?1
             ORDER BY next_run ASC"
        ).map_err(|e| format!("due_scheduled_tasks prepare: {}", e))?;
        let rows = stmt.query_map(rusqlite::params![now], |row| {
            Ok(ScheduledTask {
                id:           row.get(0)?,
                name:         row.get(1)?,
                prompt:       row.get(2)?,
                cron_expr:    row.get(3)?,
                next_run:     row.get(4)?,
                last_run:     row.get(5)?,
                last_status:  row.get(6)?,
                last_output:  row.get(7)?,
                enabled:      row.get::<_, i64>(8)? != 0,
                created_at:   row.get(9)?,
                updated_at:   row.get(10)?,
            })
        }).map_err(|e| format!("due_scheduled_tasks query: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("due_scheduled_tasks row: {}", e))?);
        }
        Ok(out)
    })
}

/// Mark a task as just-run, recording status/output and computing next_run.
/// If cron_expr is set, advance next_run; otherwise (one-shot) disable.
#[tauri::command]
pub fn mark_scheduled_run(
    id:     i64,
    status: String,        // 'ok' | 'error' | 'skipped'
    output: Option<String>,
) -> Result<(), String> {
    if id <= 0 { return Err("mark_scheduled_run: invalid id".to_string()); }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let truncated = output.as_ref().map(|s| s.chars().take(2000).collect::<String>());
    shared_db(|conn| {
        // Pull cron + decide what next_run should become.
        let row: rusqlite::Result<(Option<String>,)> = conn.query_row(
            "SELECT cron_expr FROM scheduled_tasks WHERE id = ?1",
            rusqlite::params![id],
            |r| Ok((r.get(0)?,)),
        );
        let (cron_expr,) = row.map_err(|e| format!("mark_scheduled_run lookup: {}", e))?;
        let (next_run, enabled) = match cron_expr {
            Some(ref e) if !e.is_empty() => match next_after(e, now + 60) {
                Ok(n)  => (n, 1_i64),
                Err(_) => (now + 3600, 1_i64),  // bad cron → retry in 1h, don't disable
            },
            _ => (0, 0_i64),                    // one-shot → disable, next_run irrelevant
        };
        conn.execute(
            "UPDATE scheduled_tasks SET
                 last_run    = ?2,
                 last_status = ?3,
                 last_output = ?4,
                 next_run    = ?5,
                 enabled     = ?6,
                 updated_at  = ?2
             WHERE id = ?1",
            rusqlite::params![id, now, status, truncated, next_run, enabled],
        ).map_err(|e| format!("mark_scheduled_run update: {}", e))?;
        Ok(())
    })
}

/// Toggle enabled state.
#[tauri::command]
pub fn toggle_scheduled_task(id: i64, enabled: bool) -> Result<usize, String> {
    if id <= 0 { return Err("toggle_scheduled_task: invalid id".to_string()); }
    shared_db(|conn| {
        let n = conn.execute(
            "UPDATE scheduled_tasks SET enabled = ?2, updated_at = strftime('%s','now') WHERE id = ?1",
            rusqlite::params![id, if enabled { 1 } else { 0 }],
        ).map_err(|e| format!("toggle_scheduled_task: {}", e))?;
        Ok(n)
    })
}

/// Delete a scheduled task.
#[tauri::command]
pub fn delete_scheduled_task(id: i64) -> Result<usize, String> {
    if id <= 0 { return Err("delete_scheduled_task: invalid id".to_string()); }
    shared_db(|conn| {
        let n = conn.execute(
            "DELETE FROM scheduled_tasks WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| format!("delete_scheduled_task: {}", e))?;
        Ok(n)
    })
}

// ── Cron expression helpers ─────────────────────────────────────────────

/// Validate a cron expression. Returns Ok on valid, Err otherwise.
/// Supports the common 5-field POSIX-ish subset: minute hour day month dow.
pub fn validate_cron(expr: &str) -> Result<(), String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!("expected 5 fields (minute hour day month dow), got {}", fields.len()));
    }
    let ranges: [(i32, i32); 5] = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)];
    for (i, f) in fields.iter().enumerate() {
        validate_field(f, ranges[i].0, ranges[i].1)
            .map_err(|e| format!("field {} ({:?}): {}", i + 1, f, e))?;
    }
    Ok(())
}

fn validate_field(f: &str, lo: i32, hi: i32) -> Result<(), String> {
    if f == "*" { return Ok(()); }
    for chunk in f.split(',') {
        // step?
        let (range, step_str) = match chunk.split_once('/') {
            Some((r, s)) => (r, Some(s)),
            None         => (chunk, None),
        };
        if let Some(s) = step_str {
            let _: i32 = s.parse().map_err(|_| format!("bad step {:?}", s))?;
        }
        if range == "*" {
            // Fine.
        } else if let Some((a, b)) = range.split_once('-') {
            let an: i32 = a.parse().map_err(|_| format!("bad range start {:?}", a))?;
            let bn: i32 = b.parse().map_err(|_| format!("bad range end {:?}", b))?;
            if an < lo || bn > hi || an > bn {
                return Err(format!("range {}-{} out of {}-{}", an, bn, lo, hi));
            }
        } else {
            let n: i32 = range.parse().map_err(|_| format!("bad value {:?}", range))?;
            if n < lo || n > hi {
                return Err(format!("value {} out of {}-{}", n, lo, hi));
            }
        }
    }
    Ok(())
}

/// Compute the next unix-epoch second after `from` that satisfies the cron
/// expression. Naive minute-by-minute search capped at 366 days lookahead —
/// good enough for the common cases and immune to timezone bugs.
pub fn next_after(expr: &str, from: i64) -> Result<i64, String> {
    validate_cron(expr)?;
    let fields: Vec<&str> = expr.split_whitespace().collect();
    // We work in UTC seconds. Round `from` UP to the next minute boundary.
    let mut t = ((from + 59) / 60) * 60;
    let cap = from + 366 * 24 * 60 * 60;
    while t < cap {
        let (mn, hr, dy, mo, dow) = decompose_utc(t);
        if matches_field(fields[0], mn, 0, 59)
            && matches_field(fields[1], hr, 0, 23)
            && matches_field(fields[2], dy, 1, 31)
            && matches_field(fields[3], mo, 1, 12)
            && matches_field(fields[4], dow, 0, 6) {
            return Ok(t);
        }
        t += 60;
    }
    Err("no match within 366 days".to_string())
}

fn matches_field(f: &str, val: i32, lo: i32, hi: i32) -> bool {
    if f == "*" { return true; }
    for chunk in f.split(',') {
        let (range, step) = match chunk.split_once('/') {
            Some((r, s)) => (r, s.parse::<i32>().unwrap_or(1)),
            None         => (chunk, 1),
        };
        let (a, b) = if range == "*" {
            (lo, hi)
        } else if let Some((aa, bb)) = range.split_once('-') {
            (aa.parse().unwrap_or(lo), bb.parse().unwrap_or(hi))
        } else {
            let n: i32 = range.parse().unwrap_or(-1);
            (n, n)
        };
        if val < a || val > b { continue; }
        if step <= 1 { return true; }
        if (val - a) % step == 0 { return true; }
    }
    false
}

/// Decompose a unix-epoch second into (minute, hour, day-of-month, month,
/// day-of-week). UTC; pure-Rust without chrono dep on this hot path —
/// chrono is already in the workspace but we avoid pulling its locale data
/// cost on every minute tick once the scheduler is wired up.
fn decompose_utc(t: i64) -> (i32, i32, i32, i32, i32) {
    let total_min = t / 60;
    let total_hr  = total_min / 60;
    let total_dy  = total_hr / 24;
    let mn = (total_min % 60) as i32;
    let hr = (total_hr  % 24) as i32;
    let dow = ((total_dy + 4) % 7) as i32;     // 1970-01-01 was a Thursday
    // Convert total_dy → year/month/day.
    let (_y, m, d) = ymd_from_days(total_dy);
    (mn, hr, d, m, dow)
}

/// Days since 1970-01-01 → (year, month [1-12], day [1-31]).
fn ymd_from_days(days: i64) -> (i32, i32, i32) {
    // Algorithm 199 from Howard Hinnant. Robust for any positive day count
    // without leap-year edge-case bugs.
    let z = days + 719_468;
    let era = if z >= 0 { z / 146_097 } else { (z - 146_096) / 146_097 };
    let doe = z - era * 146_097;
    let yoe = (doe - doe/1460 + doe/36_524 - doe/146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365*yoe + yoe/4 - yoe/100);
    let mp = (5*doy + 2) / 153;
    let d  = (doy - (153*mp + 2)/5 + 1) as i32;
    let m  = (if mp < 10 { mp + 3 } else { mp - 9 }) as i32;
    let y  = (y + (if m <= 2 { 1 } else { 0 })) as i32;
    (y, m, d)
}

// ── Tests (cargo test) ────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_cron_accepts_common_patterns() {
        assert!(validate_cron("* * * * *").is_ok());
        assert!(validate_cron("0 9 * * 1-5").is_ok());
        assert!(validate_cron("*/15 * * * *").is_ok());
        assert!(validate_cron("0 0,12 * * *").is_ok());
        assert!(validate_cron("30 8 1 1 *").is_ok());
    }

    #[test]
    fn validate_cron_rejects_malformed() {
        assert!(validate_cron("").is_err());
        assert!(validate_cron("0 9 * *").is_err());        // 4 fields
        assert!(validate_cron("0 9 * * * *").is_err());    // 6 fields
        assert!(validate_cron("60 9 * * *").is_err());     // minute > 59
        assert!(validate_cron("0 24 * * *").is_err());     // hour > 23
        assert!(validate_cron("0 9 32 * *").is_err());     // day > 31
        assert!(validate_cron("0 9 * 13 *").is_err());     // month > 12
        assert!(validate_cron("0 9 * * 7").is_err());      // dow > 6
        assert!(validate_cron("0 9 * * abc").is_err());    // non-numeric
    }

    #[test]
    fn next_after_for_every_minute_is_t_plus_60_at_minute_boundary() {
        // t is unix epoch seconds. Pick a clean minute.
        let t0 = 1_700_000_000_i64;       // arbitrary, lands on a minute
        let next = next_after("* * * * *", t0).unwrap();
        // For "every minute", next match should be the very next minute boundary.
        assert!(next > t0);
        assert!(next - t0 <= 60);
        // And it must itself be aligned to minute zero.
        assert_eq!(next % 60, 0);
    }

    #[test]
    fn next_after_for_daily_9am_is_within_24h() {
        let t0 = 1_700_000_000_i64;
        let next = next_after("0 9 * * *", t0).unwrap();
        assert!(next > t0);
        assert!(next - t0 < 25 * 3600);   // less than 25 hours away
        // The hour field of next must be 9 in UTC.
        let (mn, hr, _, _, _) = decompose_utc(next);
        assert_eq!(mn, 0);
        assert_eq!(hr, 9);
    }

    #[test]
    fn matches_field_handles_step_lists_and_ranges() {
        assert!(matches_field("*",     5, 0, 59));
        assert!(matches_field("5",     5, 0, 59));
        assert!(matches_field("0,5,10", 5, 0, 59));
        assert!(matches_field("0-10",   5, 0, 59));
        assert!(matches_field("*/5",    5, 0, 59));
        assert!(matches_field("*/5",    0, 0, 59));
        assert!(matches_field("*/5",   55, 0, 59));
        assert!(!matches_field("*/5",   3, 0, 59));
        assert!(!matches_field("0-3",   5, 0, 59));
    }

    #[test]
    fn ymd_round_trips_known_dates() {
        // 0 days since 1970-01-01 → 1970-01-01.
        assert_eq!(ymd_from_days(0), (1970, 1, 1));
        // 31 days later → 1970-02-01.
        assert_eq!(ymd_from_days(31), (1970, 2, 1));
        // Common reference: 2000-01-01.
        let days_to_2000 = 30 * 365 + 7;     // 30 years + leap days, approx
        let (y, _, _) = ymd_from_days(days_to_2000 as i64);
        assert!(y >= 1999 && y <= 2001);
    }
}
