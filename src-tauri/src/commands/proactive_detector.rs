// ── proactive_detector.rs — Proactive Operations Assistant (v1.7.80) ─────
//
// Background detectors that watch Lucy's existing telemetry surfaces and
// surface OPERATOR-ACTIONABLE insights without the user having to ask.
// The classic ChatGPT/Gemini/Claude experience is purely reactive: the
// operator types, the assistant responds. Lucy already collects:
//
//   • Self-diagnostic reports (commands/diagnostics.rs)
//   • App log (utils/logging.rs · lucy_app.log)
//   • Memory pipeline state (memory tables)
//   • Stream session map (state::STREAM_SESSIONS)
//
// This module is the eyes-on-the-data layer that turns "data Lucy
// already has" into "insights she surfaces unprompted". When a
// detector fires, the result is:
//
//   1. Persisted to the new `proactive_insights` table (timestamp,
//      kind, severity, message, dedupe_key).
//   2. Exposed via the Tauri command `proactive_insights_recent`
//      which the frontend polls every 2 minutes.
//   3. A new insight triggers an OS notification when severity > info
//      via the existing `tauri-plugin-notification`.
//
// Detectors run on a tokio interval (3-minute tick). Each detector is
// a pure function over a snapshot of the relevant state; no mutable
// shared state outside the SQLite write.
//
// Dedupe strategy: each detection produces a stable `dedupe_key`. We
// only insert an insight if the same key hasn't fired in the last
// COOLDOWN window (default 4 hours). This prevents the same "IIS pool
// restarted 5×" alert from spamming every 3 minutes for hours.

use serde::{Deserialize, Serialize};
use std::time::Duration;

// ── Public types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProactiveInsight {
    pub id: i64,
    pub created_at: i64,
    pub kind: String,             // detector name (snake_case)
    pub severity: String,         // 'info' | 'warning' | 'critical'
    pub title: String,            // short summary, ≤80 chars
    pub detail: String,           // longer explanation, ≤500 chars
    pub dedupe_key: String,       // stable id for cooldown
    pub dismissed: i64,           // 0 = open, 1 = user-dismissed
    pub action_hint: Option<String>,  // suggested next step (slash command etc.)
}

// ── Schema ──────────────────────────────────────────────────────────────

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS proactive_insights (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at  INTEGER NOT NULL,
    kind        TEXT    NOT NULL,
    severity    TEXT    NOT NULL,
    title       TEXT    NOT NULL,
    detail      TEXT    NOT NULL,
    dedupe_key  TEXT    NOT NULL,
    dismissed   INTEGER NOT NULL DEFAULT 0,
    action_hint TEXT
);
CREATE INDEX IF NOT EXISTS idx_proactive_insights_kind_time
    ON proactive_insights(dedupe_key, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_proactive_insights_open
    ON proactive_insights(dismissed, created_at DESC);
"#;

/// Cooldown between identical dedupe_key insights. 4 h is the default —
/// long enough not to nag, short enough to re-warn on still-present
/// issues across an operator's working day.
const COOLDOWN_SECS: i64 = 4 * 60 * 60;

/// How long to keep insights around. Older rows get cleaned on each tick.
const RETENTION_SECS: i64 = 14 * 24 * 60 * 60;   // 14 days

// ── Public API: Tauri commands ──────────────────────────────────────────

/// Returns the most recent N open (non-dismissed) insights, newest
/// first. Frontend polls this every ~120 s.
#[tauri::command]
pub fn proactive_insights_recent(limit: Option<i64>) -> Result<Vec<ProactiveInsight>, String> {
    let limit = limit.unwrap_or(20).clamp(1, 100);
    crate::commands::metrics::shared_db(|conn| {
        ensure_schema(conn)?;
        let mut stmt = conn.prepare(
            "SELECT id, created_at, kind, severity, title, detail, dedupe_key, dismissed, action_hint \
             FROM proactive_insights \
             WHERE dismissed = 0 \
             ORDER BY created_at DESC \
             LIMIT ?1"
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt.query_map([limit], |r| {
            Ok(ProactiveInsight {
                id:          r.get(0)?,
                created_at:  r.get(1)?,
                kind:        r.get(2)?,
                severity:    r.get(3)?,
                title:       r.get(4)?,
                detail:      r.get(5)?,
                dedupe_key:  r.get(6)?,
                dismissed:   r.get(7)?,
                action_hint: r.get(8)?,
            })
        }).map_err(|e| format!("query: {}", e))?;
        let mut out = Vec::new();
        for row in rows { out.push(row.map_err(|e| format!("row: {}", e))?); }
        Ok(out)
    })
}

/// Mark an insight as dismissed by the operator.
#[tauri::command]
pub fn proactive_insight_dismiss(id: i64) -> Result<(), String> {
    crate::commands::metrics::shared_db(|conn| {
        conn.execute("UPDATE proactive_insights SET dismissed = 1 WHERE id = ?1", [id])
            .map(|_| ())
            .map_err(|e| format!("dismiss: {}", e))
    })
}

/// Force a detector tick — useful for /proactive scan command or tests.
#[tauri::command]
pub async fn proactive_run_once() -> Result<i64, String> {
    // v1.7.81 — Use tauri::async_runtime instead of tokio::task directly.
    // Tauri 2 runs the global runtime via its own wrapper; calling the
    // raw tokio::task::spawn_blocking before the runtime is fully set
    // up panics with "no reactor running". The wrapper plays nice with
    // both the main thread setup() context and worker threads.
    tauri::async_runtime::spawn_blocking(run_all_detectors)
        .await
        .map_err(|e| format!("join: {}", e))?
}

// ── Background scheduler ────────────────────────────────────────────────

/// Spawn the periodic detector loop. Called from lib.rs at app startup.
/// Returns immediately; the loop runs forever on a tokio task.
///
/// Interval: 3 minutes. Picked because:
///   • Long enough that detectors don't dominate background CPU.
///   • Short enough that an operator sees a fresh insight on a
///     normal coffee-break interval.
///   • Multiplies with the 4-hour cooldown — same dedupe_key fires at
///     most ~6 times per day even at the most aggressive detection rate.
pub fn start_background_loop() {
    // v1.7.81 — Use Tauri's async runtime wrapper instead of `tokio::spawn`
    // directly. setup() runs in a context where the raw tokio runtime
    // isn't reachable via `tokio::runtime::Handle::current()`, so calling
    // tokio::spawn from here panics with "no reactor running". The Tauri
    // wrapper resolves to the global runtime regardless of caller context.
    // Same pattern as db_maintenance::spawn_background_maintenance.
    tauri::async_runtime::spawn(async {
        // Wait a bit on boot so we don't race the DB-open + migrations.
        // Tauri's async runtime wraps tokio so tokio::time::sleep works
        // here without dragging in its own runtime handle.
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            // Run detectors on a blocking thread so the rest of the
            // app doesn't block on SQL queries.
            let _ = tauri::async_runtime::spawn_blocking(run_all_detectors).await;
            tokio::time::sleep(Duration::from_secs(180)).await;
        }
    });
}

// ── Detector orchestration ──────────────────────────────────────────────

/// Runs every detector, persists new insights (subject to cooldown),
/// cleans up retention-aged rows. Returns the count of NEW insights
/// inserted this tick.
fn run_all_detectors() -> Result<i64, String> {
    crate::commands::metrics::shared_db(|conn| {
        ensure_schema(conn)?;
        // Retention cleanup first — keeps the table small.
        let cutoff = now_epoch() - RETENTION_SECS;
        let _ = conn.execute(
            "DELETE FROM proactive_insights WHERE created_at < ?1",
            [cutoff],
        );

        let mut new_count: i64 = 0;

        // Each detector is a Result of an Option of an Insight to emit.
        let detectors: Vec<fn(&rusqlite::Connection) -> Option<DetectionHit>> = vec![
            detect_expired_memory_buildup,
            detect_stream_session_leak,
            detect_oversized_log,
            detect_db_size_creeping,
            detect_db_integrity_alarm,
            detect_repeated_command_failure,
        ];

        for d in detectors {
            if let Some(hit) = d(conn) {
                if let Ok(true) = try_insert(conn, &hit) {
                    new_count += 1;
                }
            }
        }
        Ok(new_count)
    })
}

/// A single detector's emission. Stored separately from the row so
/// dedupe + cooldown happen in one transaction.
struct DetectionHit {
    kind: &'static str,
    severity: &'static str,
    title: String,
    detail: String,
    dedupe_key: String,
    action_hint: Option<&'static str>,
}

fn try_insert(conn: &rusqlite::Connection, hit: &DetectionHit) -> Result<bool, String> {
    // Cooldown: skip if same dedupe_key fired in COOLDOWN_SECS.
    let cutoff = now_epoch() - COOLDOWN_SECS;
    let recent: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proactive_insights WHERE dedupe_key = ?1 AND created_at > ?2",
        rusqlite::params![&hit.dedupe_key, cutoff],
        |r| r.get(0),
    ).unwrap_or(0);
    if recent > 0 {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO proactive_insights (created_at, kind, severity, title, detail, dedupe_key, dismissed, action_hint) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
        rusqlite::params![
            now_epoch(),
            hit.kind,
            hit.severity,
            &hit.title,
            &hit.detail,
            &hit.dedupe_key,
            hit.action_hint,
        ],
    ).map_err(|e| format!("insert: {}", e))?;

    // Fuera de la máquina, si hay canal configurado.
    //
    // Este punto y no `run_all_detectors`: aquí ya pasó el dedupe y la ventana
    // de enfriamiento, así que `Ok(true)` significa "esto es NUEVO". Avisar
    // desde el bucle reenviaría el mismo hallazgo cada pasada hasta que alguien
    // lo descartara, y un canal que repite acaba silenciado.
    //
    // El umbral de severidad lo aplica el puente: el detector emite
    // info/warning/critical, el mismo vocabulario, así que un 'info' no cruza
    // salvo que el operador lo haya pedido explícitamente.
    // `severity` is a &'static str, so cloning it copies the reference and does
    // nothing — title and detail are Strings and genuinely need theirs.
    let (sev, title, detail) = (hit.severity, hit.title.clone(), hit.detail.clone());
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::commands::notify_bridge::deliver(
            &format!("Lucy — {}", title), &detail, sev,
        ).await {
            crate::utils::logging::write_app_log(
                "WARNING",
                &format!("notify_bridge: no se pudo enviar el insight proactivo: {}", e),
            );
        }
    });

    Ok(true)
}

// ── Individual detectors ────────────────────────────────────────────────

/// > 100 expired memories pending cleanup → memory pipeline is falling
/// > behind. Suggests `/diagnostico` to use the v1.7.70 repair button.
fn detect_expired_memory_buildup(conn: &rusqlite::Connection) -> Option<DetectionHit> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM agent_memories WHERE expires_at > 0 AND expires_at < strftime('%s','now')",
        [],
        |r| r.get(0),
    ).ok()?;
    if n < 100 { return None; }
    Some(DetectionHit {
        kind: "memory_expired_buildup",
        severity: if n > 500 { "warning" } else { "info" },
        title: format!("{} expired memories pending cleanup", n),
        detail: format!(
            "The memory pipeline has {} rows past their expiry date that haven't been physically removed yet. The Diagnostics panel can clean them with one click via 'Reparar' (Purge expired).",
            n
        ),
        dedupe_key: "memory_expired_buildup".to_string(),
        action_hint: Some("Open /diagnostico → Memory Pipeline → Purge expired"),
    })
}

/// > 20 entries in STREAM_SESSIONS = likely leak (real usage stays well
/// > under 5 even with many tabs).
fn detect_stream_session_leak(_conn: &rusqlite::Connection) -> Option<DetectionHit> {
    let n = crate::state::STREAM_SESSIONS.lock().map(|m| m.len()).unwrap_or(0);
    if n < 20 { return None; }
    Some(DetectionHit {
        kind: "stream_session_leak",
        severity: if n > 50 { "warning" } else { "info" },
        title: format!("{} stream sessions accumulated", n),
        detail: format!(
            "Healthy usage keeps this under 5. {} entries suggests a leak (orphan bookkeeping from previously-completed streams). The Diagnostics panel can drain the map with 'Clear leaked sessions'.",
            n
        ),
        dedupe_key: "stream_session_leak".to_string(),
        action_hint: Some("/diagnostico → Stream Sessions → Clear leaked"),
    })
}

/// App log > 80 MB. Threshold is BELOW the diagnostic warning (100 MB)
/// so we nudge BEFORE the diagnostic goes amber.
fn detect_oversized_log(_conn: &rusqlite::Connection) -> Option<DetectionHit> {
    let dir = crate::utils::logging::get_logs_dir();
    let log = dir.join("lucy_app.log");
    let meta = std::fs::metadata(&log).ok()?;
    let size_mb = meta.len() as f64 / 1_048_576.0;
    if size_mb < 80.0 { return None; }
    Some(DetectionHit {
        kind: "log_oversized",
        severity: if size_mb > 200.0 { "warning" } else { "info" },
        title: format!("App log is {:.0} MB", size_mb),
        detail: format!(
            "lucy_app.log has grown to {:.1} MB. Rotation keeps the previous run as lucy_app.log.1 and starts a fresh file. Diagnostics → App Log → 'Rotar log'.",
            size_mb
        ),
        dedupe_key: "log_oversized".to_string(),
        action_hint: Some("/diagnostico → App Log → Rotar log"),
    })
}

/// SQLite DB > 400 MB. Threshold below the diagnostic warning (500 MB).
fn detect_db_size_creeping(conn: &rusqlite::Connection) -> Option<DetectionHit> {
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0)).ok()?;
    let page_size:  i64 = conn.query_row("PRAGMA page_size",  [], |r| r.get(0)).ok()?;
    let size_mb = (page_count * page_size) as f64 / 1_048_576.0;
    if size_mb < 400.0 { return None; }
    Some(DetectionHit {
        kind: "db_size_creeping",
        severity: if size_mb > 700.0 { "warning" } else { "info" },
        title: format!("Database is {:.0} MB", size_mb),
        detail: format!(
            "lucy.db has grown to {:.1} MB. Consolidation and VACUUM typically reclaim 30-60% of that. Diagnostics → Database → 'VACUUM database'.",
            size_mb
        ),
        dedupe_key: "db_size_creeping".to_string(),
        action_hint: Some("/diagnostico → Database → VACUUM"),
    })
}

/// PRAGMA quick_check returns anything other than "ok" → real integrity
/// concern. Highest severity detector.
fn detect_db_integrity_alarm(conn: &rusqlite::Connection) -> Option<DetectionHit> {
    let res: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0)).ok()?;
    if res == "ok" { return None; }
    let r_lower = res.to_lowercase();
    // Lock-contention artefacts are NOT real corruption; skip them.
    if r_lower.contains("locked") || r_lower.contains("query error") {
        return None;
    }
    let preview = if res.len() > 200 { format!("{}…", crate::utils::safe_truncate(&res, 200)) } else { res.clone() };
    Some(DetectionHit {
        kind: "db_integrity_alarm",
        severity: "critical",
        title: "Database integrity warning".to_string(),
        detail: format!(
            "PRAGMA quick_check reports: {}. The v1.7.65 repair handler covers NULL-confidence artefacts; for other patterns open the Diagnostics panel for details.",
            preview
        ),
        // Include a prefix hash so different concrete errors emit separate
        // insights. Same generic "locked" message would otherwise dedupe.
        dedupe_key: format!("db_integrity_alarm:{}", preview.chars().take(40).collect::<String>()),
        action_hint: Some("/diagnostico → Database"),
    })
}

/// > 20 audit_trail entries with severity = 'critical' or 'error' in
/// > the last 24 h → an operational pattern that warrants attention.
fn detect_repeated_command_failure(conn: &rusqlite::Connection) -> Option<DetectionHit> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM audit_trail \
         WHERE severity IN ('critical','error') \
           AND timestamp > strftime('%s','now','-1 day')",
        [],
        |r| r.get(0),
    ).unwrap_or(0);
    if n < 20 { return None; }
    Some(DetectionHit {
        kind: "command_failure_spike",
        severity: if n > 50 { "warning" } else { "info" },
        title: format!("{} command failures in 24 h", n),
        detail: format!(
            "The audit trail shows {} commands with error/critical severity in the last day. This is well above the typical baseline (< 5/day). The Audit Trail view filters by severity.",
            n
        ),
        dedupe_key: "command_failure_spike".to_string(),
        action_hint: Some("Sidebar → Audit Trail"),
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn ensure_schema(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute_batch(SCHEMA).map_err(|e| format!("schema: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_key_format_includes_kind() {
        let h = DetectionHit {
            kind: "memory_expired_buildup",
            severity: "info",
            title: "x".into(),
            detail: "y".into(),
            dedupe_key: "memory_expired_buildup".into(),
            action_hint: None,
        };
        assert!(h.dedupe_key.contains("memory_expired_buildup"));
    }

    #[test]
    fn cooldown_constant_is_reasonable() {
        // Don't spam: ≥ 1 hour cooldown.
        assert!(COOLDOWN_SECS >= 3600);
        // Don't permanently hide: ≤ 1 day.
        assert!(COOLDOWN_SECS <= 86400);
    }
}
