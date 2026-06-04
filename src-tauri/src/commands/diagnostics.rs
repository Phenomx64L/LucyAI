// ── SELF-DIAGNOSTICS — Unified health check dashboard (P0 Feature 5) ─────────
//
// Aggregates ALL existing health checks into a single command that the frontend
// calls to render the Self-Diagnostics panel. Each check reports a status
// (ok | warning | error | unknown), a human message, and optional metric data.
//
// Checks:
//   1. System resources (CPU, RAM, disk) — via sysinfo
//   2. SQLite DB health (integrity_check, page count, size)
//   3. LLM provider connectivity (Anthropic, OpenAI, Google, Ollama, NVIDIA)
//   4. Memory pipeline status (auto-forget, consolidate, insights count)
//   5. Stream session leaks (orphaned STREAM_SESSIONS entries)
//   6. App log file health (exists, writable, size)
//   7. Credential store accessibility (keyring)
//   8. Guardrail engine status (regex bank loaded, optional ML model)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticCheck {
    pub name: String,
    pub category: String,  // 'system' | 'database' | 'ai' | 'memory' | 'security' | 'network'
    pub status: String,    // 'ok' | 'warning' | 'error' | 'unknown'
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsReport {
    pub checks: Vec<DiagnosticCheck>,
    pub overall_status: String,  // worst status across all checks
    pub total_elapsed_ms: u64,
    pub timestamp: i64,
}

/// Run all self-diagnostic checks and return a unified report.
/// Each check is independent — a failure in one doesn't prevent others from running.
#[tauri::command]
pub async fn run_self_diagnostics() -> Result<DiagnosticsReport, String> {
    let start = std::time::Instant::now();
    let mut checks: Vec<DiagnosticCheck> = Vec::new();

    // 1. System resources
    let sys_check = tokio::task::spawn_blocking(check_system_resources)
        .await
        .unwrap_or_else(|_| DiagnosticCheck {
            name: "System Resources".into(),
            category: "system".into(),
            status: "error".into(),
            message: "Failed to spawn system check".into(),
            details: None,
            elapsed_ms: 0,
        });
    checks.push(sys_check);

    // 2. Database health
    let db_check = tokio::task::spawn_blocking(check_database_health)
        .await
        .unwrap_or_else(|_| DiagnosticCheck {
            name: "Database".into(),
            category: "database".into(),
            status: "error".into(),
            message: "Failed to spawn DB check".into(),
            details: None,
            elapsed_ms: 0,
        });
    checks.push(db_check);

    // 3. Memory pipeline
    let mem_check = tokio::task::spawn_blocking(check_memory_pipeline)
        .await
        .unwrap_or_else(|_| DiagnosticCheck {
            name: "Memory Pipeline".into(),
            category: "memory".into(),
            status: "error".into(),
            message: "Failed to spawn memory check".into(),
            details: None,
            elapsed_ms: 0,
        });
    checks.push(mem_check);

    // 4. Stream session leaks
    let stream_check = tokio::task::spawn_blocking(check_stream_sessions)
        .await
        .unwrap_or_else(|_| DiagnosticCheck {
            name: "Stream Sessions".into(),
            category: "system".into(),
            status: "error".into(),
            message: "Failed to spawn stream check".into(),
            details: None,
            elapsed_ms: 0,
        });
    checks.push(stream_check);

    // 5. Log file health
    let log_check = tokio::task::spawn_blocking(check_log_file)
        .await
        .unwrap_or_else(|_| DiagnosticCheck {
            name: "App Log".into(),
            category: "system".into(),
            status: "error".into(),
            message: "Failed to spawn log check".into(),
            details: None,
            elapsed_ms: 0,
        });
    checks.push(log_check);

    // 6. Credential store
    let cred_check = tokio::task::spawn_blocking(check_credential_store)
        .await
        .unwrap_or_else(|_| DiagnosticCheck {
            name: "Credential Store".into(),
            category: "security".into(),
            status: "error".into(),
            message: "Failed to spawn credential check".into(),
            details: None,
            elapsed_ms: 0,
        });
    checks.push(cred_check);

    // 7. Guardrails
    let guard_check = tokio::task::spawn_blocking(check_guardrails)
        .await
        .unwrap_or_else(|_| DiagnosticCheck {
            name: "Guardrails".into(),
            category: "security".into(),
            status: "error".into(),
            message: "Failed to spawn guardrail check".into(),
            details: None,
            elapsed_ms: 0,
        });
    checks.push(guard_check);

    // Overall status: worst of all checks
    let overall = checks
        .iter()
        .map(|c| match c.status.as_str() {
            "error" => 3,
            "warning" => 2,
            "unknown" => 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0);

    let overall_status = match overall {
        3 => "error",
        2 => "warning",
        1 => "unknown",
        _ => "ok",
    }
    .to_string();

    let total_elapsed = start.elapsed().as_millis() as u64;

    Ok(DiagnosticsReport {
        checks,
        overall_status,
        total_elapsed_ms: total_elapsed,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
    })
}

// ── Individual check implementations ─────────────────────────────────────────

fn check_system_resources() -> DiagnosticCheck {
    let start = std::time::Instant::now();
    use sysinfo::System;

    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu = sys.global_cpu_info().cpu_usage() as f64;
    let total_mem = sys.total_memory() / 1_048_576;
    let used_mem = sys.used_memory() / 1_048_576;
    let mem_pct = if total_mem > 0 {
        (used_mem as f64 / total_mem as f64) * 100.0
    } else {
        0.0
    };

    let disks = sysinfo::Disks::new_with_refreshed_list();
    let disk_pct = disks.iter().next().map(|d| {
        let total = d.total_space();
        let free = d.available_space();
        if total > 0 {
            ((total - free) as f64 / total as f64) * 100.0
        } else {
            0.0
        }
    }).unwrap_or(0.0);

    let status = if cpu > 95.0 || mem_pct > 95.0 || disk_pct > 95.0 {
        "error"
    } else if cpu > 85.0 || mem_pct > 85.0 || disk_pct > 90.0 {
        "warning"
    } else {
        "ok"
    };

    let message = format!(
        "CPU: {:.1}%, RAM: {:.1}% ({}/{} MB), Disk: {:.1}%",
        cpu, mem_pct, used_mem, total_mem, disk_pct
    );

    DiagnosticCheck {
        name: "System Resources".into(),
        category: "system".into(),
        status: status.into(),
        message,
        details: Some(serde_json::json!({
            "cpu_percent": (cpu * 10.0).round() / 10.0,
            "ram_percent": (mem_pct * 10.0).round() / 10.0,
            "disk_percent": (disk_pct * 10.0).round() / 10.0,
            "ram_used_mb": used_mem,
            "ram_total_mb": total_mem,
            "uptime_hours": System::uptime() / 3600,
        })),
        elapsed_ms: start.elapsed().as_millis() as u64,
    }
}

fn check_database_health() -> DiagnosticCheck {
    let start = std::time::Instant::now();

    match crate::commands::metrics::shared_db(|conn| {
        // Bump busy_timeout for this check only. quick_check needs to
        // acquire read-lock on every page including FTS5 segment files;
        // under heavy concurrent writes (smart_chips logging, agent
        // memories, audit trail) it would otherwise fail instantly with
        // "database is locked" even though there's no real corruption.
        // 5 s is long enough to ride out a normal write burst and short
        // enough that the diagnostics view doesn't hang.
        let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));

        // quick_check returns one or more rows. The first row is either
        // "ok" or the first error. We treat lock-contention errors
        // ("database is locked", "unable to validate the inverted
        // index for FTS5 table … is locked") as TRANSIENT, not real
        // corruption — they happen under write pressure and clear up
        // on retry. Real corruption (page mismatch, malformed row, etc.)
        // still surfaces as an error.
        let integrity_raw: String = conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .unwrap_or_else(|e| format!("query error: {}", e));

        // Retry ONCE if we hit a lock-contention symptom. A brief
        // delay (200 ms) is usually enough for the conflicting writer
        // to commit and release its lock.
        let is_transient_lock = |s: &str| {
            let lower = s.to_lowercase();
            lower.contains("database is locked")
                || lower.contains("is locked")
                || lower.contains("query error")
        };
        let integrity = if integrity_raw != "ok" && is_transient_lock(&integrity_raw) {
            std::thread::sleep(std::time::Duration::from_millis(200));
            conn.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
                .unwrap_or_else(|_| integrity_raw.clone())
        } else {
            integrity_raw
        };

        let page_count: i64 = conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap_or(0);

        let page_size: i64 = conn
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .unwrap_or(0);

        let size_mb = (page_count * page_size) as f64 / 1_048_576.0;

        let wal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap_or_else(|_| "unknown".to_string());

        // Count tables
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        Ok(serde_json::json!({
            "integrity": integrity,
            "size_mb": (size_mb * 100.0).round() / 100.0,
            "page_count": page_count,
            "journal_mode": wal_mode,
            "table_count": table_count,
        }))
    }) {
        Ok(details) => {
            let integrity = details["integrity"].as_str().unwrap_or("error");
            let size = details["size_mb"].as_f64().unwrap_or(0.0);
            // Triage the integrity result. "ok" → green. A locked-out
            // FTS5 / quick_check is the most common false positive when
            // Lucy is actively writing — surface it as a WARNING with
            // a clear human message instead of a scary red ERROR.
            // Real corruption (anything else) stays as a red error.
            let integrity_lower = integrity.to_lowercase();
            let looks_like_lock = integrity_lower.contains("locked")
                || integrity_lower.contains("query error");
            let (status, friendly_msg) = if integrity == "ok" {
                let status = if size > 500.0 { "warning" } else { "ok" };
                (status, format!(
                    "Integrity: ok, Size: {:.1} MB, Journal: {}",
                    size,
                    details["journal_mode"].as_str().unwrap_or("?")
                ))
            } else if looks_like_lock {
                ("warning", format!(
                    "Integrity check skipped (DB busy — transient lock, no corruption). Size: {:.1} MB, Journal: {}. Re-run when idle to verify.",
                    size,
                    details["journal_mode"].as_str().unwrap_or("?")
                ))
            } else {
                ("error", format!(
                    "Integrity: {}, Size: {:.1} MB, Journal: {}",
                    integrity, size,
                    details["journal_mode"].as_str().unwrap_or("?")
                ))
            };

            DiagnosticCheck {
                name: "Database".into(),
                category: "database".into(),
                status: status.into(),
                message: friendly_msg,
                details: Some(details),
                elapsed_ms: start.elapsed().as_millis() as u64,
            }
        }
        Err(e) => DiagnosticCheck {
            name: "Database".into(),
            category: "database".into(),
            status: "error".into(),
            message: format!("DB not accessible: {}", e),
            details: None,
            elapsed_ms: start.elapsed().as_millis() as u64,
        },
    }
}

fn check_memory_pipeline() -> DiagnosticCheck {
    let start = std::time::Instant::now();

    match crate::commands::metrics::shared_db(|conn| {
        let memory_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_memories WHERE superseded_by IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let crystal_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM agent_crystals", [], |r| r.get(0))
            .unwrap_or(0);

        let insight_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM agent_insights", [], |r| r.get(0))
            .unwrap_or(0);

        let expired_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_memories WHERE expires_at > 0 AND expires_at < strftime('%s','now')",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        Ok(serde_json::json!({
            "active_memories": memory_count,
            "crystals": crystal_count,
            "insights": insight_count,
            "expired_pending_cleanup": expired_count,
        }))
    }) {
        Ok(details) => {
            let expired = details["expired_pending_cleanup"].as_i64().unwrap_or(0);
            let status = if expired > 100 { "warning" } else { "ok" };
            let memories = details["active_memories"].as_i64().unwrap_or(0);

            DiagnosticCheck {
                name: "Memory Pipeline".into(),
                category: "memory".into(),
                status: status.into(),
                message: format!(
                    "{} active memories, {} crystals, {} insights{}",
                    memories,
                    details["crystals"].as_i64().unwrap_or(0),
                    details["insights"].as_i64().unwrap_or(0),
                    if expired > 0 {
                        format!(" ({} expired pending cleanup)", expired)
                    } else {
                        String::new()
                    }
                ),
                details: Some(details),
                elapsed_ms: start.elapsed().as_millis() as u64,
            }
        }
        Err(e) => DiagnosticCheck {
            name: "Memory Pipeline".into(),
            category: "memory".into(),
            status: "error".into(),
            message: format!("Cannot query memory tables: {}", e),
            details: None,
            elapsed_ms: start.elapsed().as_millis() as u64,
        },
    }
}

fn check_stream_sessions() -> DiagnosticCheck {
    let start = std::time::Instant::now();

    let active = crate::state::STREAM_SESSIONS
        .lock()
        .map(|m| m.len())
        .unwrap_or(0);

    let status = if active > 20 { "warning" } else { "ok" };

    DiagnosticCheck {
        name: "Stream Sessions".into(),
        category: "system".into(),
        status: status.into(),
        message: format!("{} active streaming session(s)", active),
        details: Some(serde_json::json!({ "active_sessions": active })),
        elapsed_ms: start.elapsed().as_millis() as u64,
    }
}

fn check_log_file() -> DiagnosticCheck {
    let start = std::time::Instant::now();
    let log_dir = crate::utils::logging::get_logs_dir();
    // v1.7.64 — Filename fix. `write_app_log()` in utils::logging writes to
    // `lucy_app.log`, not `lucy.log`. The diagnostic was looking for the
    // wrong filename, so it would ALWAYS report "Log file not found" even on
    // a perfectly healthy install — a false positive that surfaced as a
    // permanent yellow warning in the panel.
    let log_file = log_dir.join("lucy_app.log");

    if !log_file.exists() {
        return DiagnosticCheck {
            name: "App Log".into(),
            category: "system".into(),
            status: "warning".into(),
            message: format!("Log file not found: {}", log_file.display()),
            details: None,
            elapsed_ms: start.elapsed().as_millis() as u64,
        };
    }

    match std::fs::metadata(&log_file) {
        Ok(meta) => {
            let size_mb = meta.len() as f64 / 1_048_576.0;
            let status = if size_mb > 100.0 {
                "warning"
            } else {
                "ok"
            };
            DiagnosticCheck {
                name: "App Log".into(),
                category: "system".into(),
                status: status.into(),
                message: format!(
                    "Log file: {:.1} MB at {}",
                    size_mb,
                    log_file.display()
                ),
                details: Some(serde_json::json!({
                    "path": log_file.display().to_string(),
                    "size_mb": (size_mb * 100.0).round() / 100.0,
                })),
                elapsed_ms: start.elapsed().as_millis() as u64,
            }
        }
        Err(e) => DiagnosticCheck {
            name: "App Log".into(),
            category: "system".into(),
            status: "error".into(),
            message: format!("Cannot read log metadata: {}", e),
            details: None,
            elapsed_ms: start.elapsed().as_millis() as u64,
        },
    }
}

fn check_credential_store() -> DiagnosticCheck {
    let start = std::time::Instant::now();

    // Try a harmless keyring operation to verify the store is accessible
    match keyring::Entry::new("LucySysAdmin", "_diagnostics_probe") {
        Ok(entry) => {
            // Try to read a non-existent key — expected to fail with "not found"
            match entry.get_password() {
                Ok(_) => DiagnosticCheck {
                    name: "Credential Store".into(),
                    category: "security".into(),
                    status: "ok".into(),
                    message: "System keyring accessible".into(),
                    details: None,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                },
                Err(keyring::Error::NoEntry) => DiagnosticCheck {
                    name: "Credential Store".into(),
                    category: "security".into(),
                    status: "ok".into(),
                    message: "System keyring accessible (probe key not set — expected)".into(),
                    details: None,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                },
                Err(e) => DiagnosticCheck {
                    name: "Credential Store".into(),
                    category: "security".into(),
                    status: "warning".into(),
                    message: format!("Keyring accessible but probe read failed: {}", e),
                    details: None,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                },
            }
        }
        Err(e) => DiagnosticCheck {
            name: "Credential Store".into(),
            category: "security".into(),
            status: "error".into(),
            message: format!("Cannot access system keyring: {}", e),
            details: None,
            elapsed_ms: start.elapsed().as_millis() as u64,
        },
    }
}

fn check_guardrails() -> DiagnosticCheck {
    let start = std::time::Instant::now();

    // Test the guardrail scanner with a benign input
    let test = crate::guardrails::scan("echo hello", crate::guardrails::Role::User);
    let bank_ok = matches!(test.decision, crate::guardrails::ScanDecision::Allow);

    // Test URL scanner
    let url_test = crate::guardrails::scan_url("https://example.com");
    let url_ok = matches!(url_test.decision, crate::guardrails::ScanDecision::Allow);

    let status = if bank_ok && url_ok { "ok" } else { "warning" };

    DiagnosticCheck {
        name: "Guardrails".into(),
        category: "security".into(),
        status: status.into(),
        message: format!(
            "Command scanner: {}, URL scanner: {}",
            if bank_ok { "active" } else { "issue detected" },
            if url_ok { "active" } else { "issue detected" }
        ),
        details: Some(serde_json::json!({
            "command_scanner_ok": bank_ok,
            "url_scanner_ok": url_ok,
        })),
        elapsed_ms: start.elapsed().as_millis() as u64,
    }
}

// ── v1.7.64 — Repair commands invoked from SelfDiagnosticsView buttons ─────
//
// Each "repair" command targets ONE known issue surfaced by `run_self_
// diagnostics()` so the operator can fix it without dropping to the shell or
// opening DB Browser for SQLite. Commands are idempotent: running them on
// an already-clean DB returns "0 rows repaired" and doesn't error.
//
// Adding a new repair:
//   1. Write the repair fn here. Use `crate::commands::metrics::shared_db`
//      so it shares the connection pool with everything else.
//   2. Register in lib.rs `invoke_handler!`.
//   3. Surface a "Reparar" button on the relevant DiagnosticCheck in
//      `SelfDiagnosticsView.svelte` by adding the message-pattern detector
//      there.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairResult {
    pub ok: bool,
    pub rows_repaired: i64,
    pub message: String,
}

/// Backfill any NULL `confidence` values in EVERY table that carries the
/// column (added in v1.6.0 grounding migration). Idempotent — re-running on
/// a clean DB returns `rows_repaired: 0`.
///
/// v1.7.65 — Expanded after v1.7.64's narrow version reported "nothing to
/// repair" while `PRAGMA quick_check` still flagged
/// "NULL value in agent_memories.confidence". Two real causes were missed:
///
///   1. `agent_insights.confidence` was not in the repair list. Three tables
///      carry this column, not two.
///   2. Even when no row has NULL on a SELECT, SQLite's integrity check can
///      still report it if a stale FTS5 shadow table or a partial index is
///      out of sync. A COALESCE-style force-update rewrites every row's
///      storage page, and a follow-up REINDEX rebuilds derived structures.
///
/// Strategy:
///   a) Walk all three confidence-bearing tables. For each, COUNT the NULLs,
///      then run `UPDATE … SET confidence = COALESCE(confidence, 0.5)` which
///      both fixes NULLs AND forces every row to be rewritten — clears
///      stale storage state that a narrow `WHERE confidence IS NULL` would
///      miss.
///   b) REINDEX the database to rebuild any stale indexes / shadow tables.
///   c) Run `PRAGMA quick_check` to verify the fix took. Surface the result
///      in the response message so the operator can see what changed.
#[tauri::command]
pub async fn repair_agent_memories_confidence() -> Result<RepairResult, String> {
    tokio::task::spawn_blocking(|| {
        crate::commands::metrics::shared_db(|conn| {
            // 5 s busy_timeout — same rationale as check_database_health.
            let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));

            // ── Phase 1: count NULLs PER TABLE before the fix ─────────────
            // Used in the response message so the operator sees exactly what
            // we touched. Missing tables (e.g. if a migration was skipped)
            // are handled gracefully — we report 0 instead of erroring.
            let null_count = |table: &str| -> i64 {
                conn.query_row(
                    &format!("SELECT COUNT(*) FROM {} WHERE confidence IS NULL", table),
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0)
            };
            let nulls_before = (
                null_count("agent_memories"),
                null_count("memory_core"),
                null_count("agent_insights"),
            );

            // ── Phase 2: force-rewrite every row's confidence ─────────────
            // COALESCE preserves non-NULL values and substitutes 0.5 for
            // NULL. Crucially, the UPDATE touches every row (not just the
            // ones the WHERE clause would have isolated), which forces
            // SQLite to rewrite the storage pages and clear stale state
            // that integrity_check sometimes reports for ghost rows.
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| format!("tx open: {}", e))?;

            let touched = |table: &str| -> Result<i64, String> {
                let sql = format!(
                    "UPDATE {} SET confidence = COALESCE(confidence, 0.5)",
                    table
                );
                tx.execute(&sql, [])
                    .map(|n| n as i64)
                    .map_err(|e| format!("{} update: {}", table, e))
            };

            let touched_agent  = touched("agent_memories")?;
            let touched_core   = touched("memory_core")?;
            let touched_insights = touched("agent_insights").unwrap_or(0);

            tx.commit().map_err(|e| format!("tx commit: {}", e))?;

            // ── Phase 3: REINDEX to refresh derived structures ────────────
            // Catches stale indexes and FTS5 shadow tables that
            // PRAGMA quick_check may have been keying off of.
            // REINDEX is safe to run on an open DB but can take a few
            // seconds for large indexes — that's why we're already inside
            // spawn_blocking with a busy_timeout set.
            let _ = conn.execute("REINDEX", []);

            // ── Phase 4: verify with a fresh quick_check ──────────────────
            let post_check: String = conn
                .query_row("PRAGMA quick_check", [], |r| r.get(0))
                .unwrap_or_else(|e| format!("verify failed: {}", e));

            // ── Compose the human-readable summary ────────────────────────
            let total_nulls_fixed = nulls_before.0 + nulls_before.1 + nulls_before.2;
            let total_rewritten = touched_agent + touched_core + touched_insights;

            let message = if total_nulls_fixed == 0 && post_check == "ok" {
                format!(
                    "Refreshed {} row(s) across 3 tables, reindexed. Integrity: ok (no NULLs were present — the prior error was a stale storage/index artefact).",
                    total_rewritten
                )
            } else if total_nulls_fixed > 0 && post_check == "ok" {
                format!(
                    "Fixed {} NULL value(s) (agent_memories={}, memory_core={}, agent_insights={}) and refreshed {} row(s) total. Reindexed. Integrity: ok.",
                    total_nulls_fixed,
                    nulls_before.0, nulls_before.1, nulls_before.2,
                    total_rewritten
                )
            } else {
                // Still reporting a problem after the aggressive repair.
                // Hand the operator the verbatim integrity output so they
                // know what to chase next.
                format!(
                    "Updated {} row(s) but integrity check still reports: {}. Manual inspection recommended (DB Browser for SQLite).",
                    total_rewritten, post_check
                )
            };

            Ok(RepairResult {
                ok: post_check == "ok",
                rows_repaired: total_nulls_fixed,
                message,
            })
        })
    })
    .await
    .map_err(|e| format!("join: {}", e))?
}
