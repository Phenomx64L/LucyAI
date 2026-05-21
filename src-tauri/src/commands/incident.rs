// ── Incident Response / SRE Mode (Nivel 4) ─────────────────────────────────
//
// Inspired by OpenSRE (github.com/Tracer-Cloud/opensre). An "incident" is a
// structured troubleshooting session with:
//
//   phases:  extract → plan → investigate → diagnose → report → done
//   evidence: typed, addressable outputs captured along the way
//   hypotheses: claims the LLM makes, each linked to the evidence that
//               supports (or contradicts) it
//   validity_score: 0..1, the fraction of hypotheses backed by real evidence
//
// Why this architecture: sysadmin troubleshooting currently relies on the
// LLM remembering what it saw. When context compresses, conclusions drift
// from evidence. By materializing evidence + claims into SQLite we get:
//   - auditable investigation trail (compliance value)
//   - reroute loop: if score is low, Lucy keeps investigating instead of
//     delivering a plausible-but-unsupported diagnosis
//   - post-mortem reusability: the incident record IS the runbook seed
//
// DB connection is shared with the metrics module — a single rusqlite
// Connection in a Mutex; all commands serialize writes through it.

use crate::commands::metrics::shared_db;
use crate::utils::db::{
    generate_id, Incident, IncidentAction, IncidentEvidence, IncidentHypothesis,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── Create ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartIncidentArgs {
    pub shell_id: String,
    pub host_name: String,
    pub title: String,
    pub description: String,
    pub max_loops: Option<u32>,
}

/// Open a new incident in the 'extract' phase. Returns the full record.
#[tauri::command]
pub async fn incident_start(args: StartIncidentArgs) -> Result<Incident, String> {
    let id = generate_id();
    let ts = now_ts();
    let max_loops = args.max_loops.unwrap_or(5);

    let incident = Incident {
        id: id.clone(),
        shell_id: args.shell_id.clone(),
        host_name: args.host_name.clone(),
        title: args.title.clone(),
        description: args.description.clone(),
        phase: "extract".to_string(),
        status: "open".to_string(),
        validity_score: 0.0,
        loop_count: 0,
        max_loops,
        created_at: ts,
        updated_at: ts,
        resolved_at: None,
        summary: None,
        root_cause: None,
    };

    shared_db(|conn| {
        conn.execute(
            "INSERT INTO incidents (id, shell_id, host_name, title, description,
                 phase, status, validity_score, loop_count, max_loops,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                &incident.id, &incident.shell_id, &incident.host_name,
                &incident.title, &incident.description,
                &incident.phase, &incident.status,
                incident.validity_score, incident.loop_count, incident.max_loops,
                incident.created_at, incident.updated_at,
            ],
        ).map_err(|e| format!("incident_start insert: {}", e))?;
        Ok(())
    })?;

    Ok(incident)
}

// ── Phase transitions ─────────────────────────────────────────────────────

/// Allowed transitions enforce the state machine. Skipping phases or going
/// backwards is rejected — keeps the investigation trail coherent.
fn valid_transition(from: &str, to: &str) -> bool {
    match (from, to) {
        ("extract", "plan") => true,
        ("plan", "investigate") => true,
        ("investigate", "diagnose") => true,
        ("investigate", "plan") => true,      // reroute: need more data
        ("diagnose", "report") => true,
        ("diagnose", "plan") => true,         // reroute: low validity score
        ("report", "done") => true,
        // Allow aborting from any phase → done (user bail-out)
        (_, "done") => true,
        _ => false,
    }
}

#[tauri::command]
pub async fn incident_advance_phase(
    incident_id: String,
    to_phase: String,
) -> Result<Incident, String> {
    shared_db(|conn| {
        let current: String = conn
            .query_row(
                "SELECT phase FROM incidents WHERE id = ?1",
                params![&incident_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("incident not found: {}", e))?;

        if !valid_transition(&current, &to_phase) {
            return Err(format!(
                "Illegal phase transition: {} → {}",
                current, to_phase
            ));
        }

        // Reroutes increment the loop counter — so we can cap investigation depth.
        let loop_increment = if to_phase == "plan" && current != "extract" { 1 } else { 0 };

        conn.execute(
            "UPDATE incidents
             SET phase = ?1, updated_at = ?2, loop_count = loop_count + ?3
             WHERE id = ?4",
            params![&to_phase, now_ts(), loop_increment, &incident_id],
        )
        .map_err(|e| format!("advance update: {}", e))?;

        fetch_incident_row(conn, &incident_id)
    })
}

// ── Evidence ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddEvidenceArgs {
    pub incident_id: String,
    pub kind: String,       // 'command_output'|'log'|'metric'|'user_input'|'observation'|'hypothesis_test'
    pub source: String,
    pub content: String,
    pub tags: Option<Vec<String>>,
}

/// Record a piece of evidence. Content is truncated to 64KB defensively —
/// a single command can return megabytes, and we don't want to choke the
/// DB on routine captures.
#[tauri::command]
pub async fn incident_add_evidence(args: AddEvidenceArgs) -> Result<IncidentEvidence, String> {
    const MAX_EVIDENCE_BYTES: usize = 64 * 1024;

    let content = if args.content.len() > MAX_EVIDENCE_BYTES {
        let mut t = crate::utils::safe_truncate(&args.content, MAX_EVIDENCE_BYTES).to_string();
        t.push_str("\n\n[...truncated for storage]");
        t
    } else {
        args.content.clone()
    };

    let tags_json = serde_json::to_string(&args.tags.clone().unwrap_or_default())
        .unwrap_or_else(|_| "[]".to_string());

    let id = generate_id();
    let ts = now_ts();

    let phase: String = shared_db(|conn| {
        let phase: String = conn
            .query_row(
                "SELECT phase FROM incidents WHERE id = ?1",
                params![&args.incident_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("incident not found: {}", e))?;

        conn.execute(
            "INSERT INTO incident_evidence
                (id, incident_id, kind, source, content, tags, timestamp, phase)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &id, &args.incident_id, &args.kind, &args.source,
                &content, &tags_json, ts, &phase,
            ],
        ).map_err(|e| format!("add_evidence insert: {}", e))?;

        conn.execute(
            "UPDATE incidents SET updated_at = ?1 WHERE id = ?2",
            params![ts, &args.incident_id],
        ).ok();

        Ok(phase)
    })?;

    Ok(IncidentEvidence {
        id,
        incident_id: args.incident_id,
        kind: args.kind,
        source: args.source,
        content,
        tags: tags_json,
        timestamp: ts,
        phase,
    })
}

#[tauri::command]
pub async fn incident_list_evidence(
    incident_id: String,
) -> Result<Vec<IncidentEvidence>, String> {
    shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, incident_id, kind, source, content, tags, timestamp, phase
             FROM incident_evidence WHERE incident_id = ?1 ORDER BY timestamp ASC"
        ).map_err(|e| format!("prepare: {}", e))?;

        let rows = stmt.query_map(params![&incident_id], |row| {
            Ok(IncidentEvidence {
                id: row.get(0)?,
                incident_id: row.get(1)?,
                kind: row.get(2)?,
                source: row.get(3)?,
                content: row.get(4)?,
                tags: row.get(5)?,
                timestamp: row.get(6)?,
                phase: row.get(7)?,
            })
        }).map_err(|e| format!("query: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect: {}", e))?;

        Ok(rows)
    })
}

// ── Hypotheses ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposeHypothesisArgs {
    pub incident_id: String,
    pub claim: String,
    pub supporting_evidence_ids: Vec<String>,
    pub confidence: Option<f64>,
}

#[tauri::command]
pub async fn incident_propose_hypothesis(
    args: ProposeHypothesisArgs,
) -> Result<IncidentHypothesis, String> {
    let id = generate_id();
    let supporting = serde_json::to_string(&args.supporting_evidence_ids)
        .unwrap_or_else(|_| "[]".to_string());
    let confidence = args.confidence.unwrap_or(0.5).clamp(0.0, 1.0);
    let ts = now_ts();

    shared_db(|conn| {
        conn.execute(
            "INSERT INTO incident_hypothesis
                (id, incident_id, claim, supporting_evidence_ids,
                 contradicting_evidence_ids, status, confidence, created_at)
             VALUES (?1, ?2, ?3, ?4, '[]', 'proposed', ?5, ?6)",
            params![&id, &args.incident_id, &args.claim, &supporting, confidence, ts],
        ).map_err(|e| format!("propose_hypothesis insert: {}", e))?;
        Ok(())
    })?;

    Ok(IncidentHypothesis {
        id,
        incident_id: args.incident_id,
        claim: args.claim,
        supporting_evidence_ids: supporting,
        contradicting_evidence_ids: "[]".to_string(),
        status: "proposed".to_string(),
        confidence,
        created_at: ts,
    })
}

#[tauri::command]
pub async fn incident_list_hypotheses(
    incident_id: String,
) -> Result<Vec<IncidentHypothesis>, String> {
    shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, incident_id, claim, supporting_evidence_ids,
                    contradicting_evidence_ids, status, confidence, created_at
             FROM incident_hypothesis WHERE incident_id = ?1 ORDER BY created_at ASC"
        ).map_err(|e| format!("prepare: {}", e))?;

        let rows = stmt.query_map(params![&incident_id], |row| {
            Ok(IncidentHypothesis {
                id: row.get(0)?,
                incident_id: row.get(1)?,
                claim: row.get(2)?,
                supporting_evidence_ids: row.get(3)?,
                contradicting_evidence_ids: row.get(4)?,
                status: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
            })
        }).map_err(|e| format!("query: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect: {}", e))?;

        Ok(rows)
    })
}

// ── Validity scoring ──────────────────────────────────────────────────────

/// Score is computed as:
///
///   score = mean(hypothesis_i.coverage)
///
///   where coverage_i = min(1.0, supporting_evidence_count / 2)
///                      * (1.0 if status == 'validated' else 0.5 if 'proposed' else 0.0)
///
/// Two pieces of supporting evidence is considered "well-supported". Validated
/// hypotheses count fully; merely proposed claims count for half credit;
/// refuted ones score zero. If there are no hypotheses, score is 0.0 — the
/// agent has not committed to any conclusion.
///
/// This is deliberately simple and transparent. The point is not statistical
/// rigor but to give the agent a stop signal: low score → keep investigating.
#[tauri::command]
pub async fn incident_calculate_score(incident_id: String) -> Result<f64, String> {
    shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT supporting_evidence_ids, status
             FROM incident_hypothesis WHERE incident_id = ?1"
        ).map_err(|e| format!("prepare: {}", e))?;

        let hypotheses: Vec<(String, String)> = stmt
            .query_map(params![&incident_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("query: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect: {}", e))?;

        if hypotheses.is_empty() {
            conn.execute(
                "UPDATE incidents SET validity_score = 0.0, updated_at = ?1 WHERE id = ?2",
                params![now_ts(), &incident_id],
            ).ok();
            return Ok(0.0);
        }

        let mut total = 0.0_f64;
        for (supporting_json, status) in &hypotheses {
            let supporting: Vec<String> = serde_json::from_str(supporting_json).unwrap_or_default();
            let ev_coverage = (supporting.len() as f64 / 2.0).min(1.0);
            let status_weight = match status.as_str() {
                "validated" => 1.0,
                "proposed" => 0.5,
                "inconclusive" => 0.25,
                "refuted" => 0.0,
                _ => 0.25,
            };
            total += ev_coverage * status_weight;
        }
        let score = total / hypotheses.len() as f64;

        conn.execute(
            "UPDATE incidents SET validity_score = ?1, updated_at = ?2 WHERE id = ?3",
            params![score, now_ts(), &incident_id],
        ).ok();

        Ok(score)
    })
}

// ── Actions (audit trail) ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogActionArgs {
    pub incident_id: String,
    pub rationale: String,
    pub command: String,
    pub output_evidence_id: Option<String>,
}

#[tauri::command]
pub async fn incident_log_action(args: LogActionArgs) -> Result<IncidentAction, String> {
    let id = generate_id();
    let ts = now_ts();

    let (phase, chain_hash) = shared_db(|conn| {
        let phase: String = conn.query_row(
            "SELECT phase FROM incidents WHERE id = ?1",
            params![&args.incident_id],
            |row| row.get(0),
        ).map_err(|e| format!("incident not found: {}", e))?;

        // ── Hash-chain: SHA-256( prev_hash || id || command || ts ) ──────
        let prev_hash: String = conn.query_row(
            "SELECT chain_hash FROM incident_action
             WHERE incident_id = ?1 ORDER BY executed_at DESC LIMIT 1",
            params![&args.incident_id],
            |row| row.get(0),
        ).unwrap_or_else(|_| "GENESIS".to_string());

        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_bytes());
        hasher.update(id.as_bytes());
        hasher.update(args.command.as_bytes());
        hasher.update(ts.to_string().as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        conn.execute(
            "INSERT INTO incident_action
                (id, incident_id, phase, rationale, command, output_evidence_id, executed_at, chain_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &id, &args.incident_id, &phase,
                &args.rationale, &args.command, &args.output_evidence_id, ts, &hash,
            ],
        ).map_err(|e| format!("log_action insert: {}", e))?;
        Ok((phase, hash))
    })?;

    Ok(IncidentAction {
        id,
        incident_id: args.incident_id,
        phase,
        rationale: args.rationale,
        command: args.command,
        output_evidence_id: args.output_evidence_id,
        executed_at: ts,
        chain_hash: Some(chain_hash),
    })
}

// ── Finalize / report ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizeArgs {
    pub incident_id: String,
    pub summary: String,
    pub root_cause: String,
    pub status: Option<String>, // 'resolved' | 'abandoned', defaults to 'resolved'
}

#[tauri::command]
pub async fn incident_finalize(args: FinalizeArgs) -> Result<Incident, String> {
    let status = args.status.unwrap_or_else(|| "resolved".to_string());
    if !matches!(status.as_str(), "resolved" | "abandoned") {
        return Err(format!("invalid status {:?}", status));
    }
    let ts = now_ts();

    let result = shared_db(|conn| {
        conn.execute(
            "UPDATE incidents
             SET phase = 'done', status = ?1, summary = ?2, root_cause = ?3,
                 resolved_at = ?4, updated_at = ?4
             WHERE id = ?5",
            params![&status, &args.summary, &args.root_cause, ts, &args.incident_id],
        ).map_err(|e| format!("finalize update: {}", e))?;

        fetch_incident_row(conn, &args.incident_id)
    })?;

    // F4 completion — auto-crystallize resolved incidents into healing patterns.
    // Only resolved (not abandoned) incidents become reusable patterns. We
    // extract: title (incident.title), symptom (description), fix (root_cause+summary).
    // Errors are swallowed: the incident finalize must succeed even if the
    // memory write fails.
    if status == "resolved" {
        let _ = auto_crystallize_pattern(&result, &args.summary, &args.root_cause);
    }

    Ok(result)
}

/// Persist a healing pattern derived from a resolved incident. Internal helper —
/// not exposed as a Tauri command (the entry point is incident_finalize).
fn auto_crystallize_pattern(incident: &Incident, summary: &str, root_cause: &str) -> Result<(), String> {
    // Skip if symptom is too short to be useful
    if incident.description.trim().len() < 8 || root_cause.trim().len() < 4 {
        return Ok(());
    }

    let payload = serde_json::json!({
        "kind": "healing-pattern",
        "symptom": incident.description.clone(),
        "fix": format!("{}\n\nSummary: {}", root_cause, summary),
        "host_name": incident.host_name.clone(),
        "success_count": 1,
        "last_used": now_ts(),
        "created_at": now_ts(),
        "source_incident_id": incident.id.clone(),
    });
    let content = payload.to_string();
    let title = if incident.title.len() > 200 {
        incident.title.chars().take(200).collect::<String>()
    } else {
        incident.title.clone()
    };
    let tags_json = serde_json::to_string(&vec![
        "healing-pattern".to_string(),
        "auto-crystallized".to_string(),
        "from-incident".to_string(),
    ]).unwrap_or_else(|_| "[]".to_string());

    shared_db(|conn| {
        conn.execute(
            "INSERT INTO agent_memories (session_id, title, content, tags, files, importance, created_at)
             VALUES ('', ?1, ?2, ?3, '[]', 5, ?4)",
            params![title, content, tags_json, now_ts()],
        ).map_err(|e| format!("crystallize insert: {}", e))?;
        Ok(())
    })
}

// ── Listing / detail ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn incident_list(shell_id: Option<String>) -> Result<Vec<Incident>, String> {
    shared_db(|conn| {
        // Auto-close stale open incidents (>4h old) to prevent persistent
        // false-positive banners from auto-triggered anomalies. Marks them
        // as 'abandoned' so they're distinguishable from user-resolved ones.
        let stale_cutoff = now_ts() - 4 * 3600;
        conn.execute(
            "UPDATE incidents SET status = 'abandoned', phase = 'done',
                    resolved_at = ?1, updated_at = ?1,
                    summary = 'Auto-closed: exceeded 4h without resolution'
             WHERE status = 'open' AND created_at < ?2",
            params![now_ts(), stale_cutoff],
        ).ok();

        let sql = if shell_id.is_some() {
            "SELECT id, shell_id, host_name, title, description, phase, status,
                    validity_score, loop_count, max_loops, created_at, updated_at,
                    resolved_at, summary, root_cause
             FROM incidents WHERE shell_id = ?1 ORDER BY updated_at DESC"
        } else {
            "SELECT id, shell_id, host_name, title, description, phase, status,
                    validity_score, loop_count, max_loops, created_at, updated_at,
                    resolved_at, summary, root_cause
             FROM incidents ORDER BY updated_at DESC LIMIT 100"
        };
        let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare: {}", e))?;

        let mapper = |row: &rusqlite::Row| {
            Ok(Incident {
                id: row.get(0)?,
                shell_id: row.get(1)?,
                host_name: row.get(2)?,
                title: row.get(3)?,
                description: row.get(4)?,
                phase: row.get(5)?,
                status: row.get(6)?,
                validity_score: row.get(7)?,
                loop_count: row.get(8)?,
                max_loops: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
                resolved_at: row.get(12)?,
                summary: row.get(13)?,
                root_cause: row.get(14)?,
            })
        };

        let rows: Vec<Incident> = match &shell_id {
            Some(sid) => stmt.query_map(params![sid], mapper),
            None      => stmt.query_map([], mapper),
        }
            .map_err(|e| format!("query: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect: {}", e))?;
        Ok(rows)
    })
}

#[tauri::command]
pub async fn incident_get(incident_id: String) -> Result<Incident, String> {
    shared_db(|conn| fetch_incident_row(conn, &incident_id))
}

fn fetch_incident_row(conn: &rusqlite::Connection, id: &str) -> Result<Incident, String> {
    conn.query_row(
        "SELECT id, shell_id, host_name, title, description, phase, status,
                validity_score, loop_count, max_loops, created_at, updated_at,
                resolved_at, summary, root_cause
         FROM incidents WHERE id = ?1",
        params![id],
        |row| {
            Ok(Incident {
                id: row.get(0)?,
                shell_id: row.get(1)?,
                host_name: row.get(2)?,
                title: row.get(3)?,
                description: row.get(4)?,
                phase: row.get(5)?,
                status: row.get(6)?,
                validity_score: row.get(7)?,
                loop_count: row.get(8)?,
                max_loops: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
                resolved_at: row.get(12)?,
                summary: row.get(13)?,
                root_cause: row.get(14)?,
            })
        },
    )
    .map_err(|e| format!("fetch_incident_row: {}", e))
}

// ── Hash-chain verification ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainVerifyResult {
    pub valid: bool,
    pub total_actions: usize,
    pub verified_count: usize,
    /// First broken link (if any): the action id where the chain breaks
    pub broken_at: Option<String>,
    pub error: Option<String>,
}

/// Verify the SHA-256 hash-chain integrity of all actions for an incident.
/// Recomputes each chain_hash from scratch and compares to the stored value.
/// Returns a detailed result showing whether the chain is tamper-free.
#[tauri::command]
pub async fn incident_verify_chain(incident_id: String) -> Result<ChainVerifyResult, String> {
    shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, command, executed_at, chain_hash
             FROM incident_action
             WHERE incident_id = ?1
             ORDER BY executed_at ASC"
        ).map_err(|e| format!("prepare: {}", e))?;

        let rows: Vec<(String, String, i64, String)> = stmt.query_map(
            params![&incident_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        if rows.is_empty() {
            return Ok(ChainVerifyResult {
                valid: true, total_actions: 0, verified_count: 0,
                broken_at: None, error: None,
            });
        }

        let mut prev_hash = "GENESIS".to_string();
        let mut verified = 0;

        for (id, command, ts, stored_hash) in &rows {
            let mut hasher = Sha256::new();
            hasher.update(prev_hash.as_bytes());
            hasher.update(id.as_bytes());
            hasher.update(command.as_bytes());
            hasher.update(ts.to_string().as_bytes());
            let expected = format!("{:x}", hasher.finalize());

            if &expected != stored_hash {
                return Ok(ChainVerifyResult {
                    valid: false,
                    total_actions: rows.len(),
                    verified_count: verified,
                    broken_at: Some(id.clone()),
                    error: Some(format!(
                        "Chain broken at action {}: expected {} but stored {}",
                        id, &expected[..16], &stored_hash[..stored_hash.len().min(16)]
                    )),
                });
            }
            prev_hash = expected;
            verified += 1;
        }

        Ok(ChainVerifyResult {
            valid: true,
            total_actions: rows.len(),
            verified_count: verified,
            broken_at: None,
            error: None,
        })
    })
}

// ── System prompts (phase-aware) ──────────────────────────────────────────
//
// Exposed to the frontend so the Svelte side can decide when to prepend
// the right instructions to the conversation. Keeping these in Rust avoids
// duplicating the state machine in two places.

#[tauri::command]
pub async fn incident_phase_prompt(phase: String) -> Result<String, String> {
    let prompt = match phase.as_str() {
        "extract" => EXTRACT_PROMPT,
        "plan" => PLAN_PROMPT,
        "investigate" => INVESTIGATE_PROMPT,
        "diagnose" => DIAGNOSE_PROMPT,
        "report" => REPORT_PROMPT,
        _ => DEFAULT_PROMPT,
    };
    Ok(prompt.to_string())
}

const EXTRACT_PROMPT: &str = r#"You are in INCIDENT MODE — EXTRACT phase.

Goal: understand the alert/issue the user reported. Do NOT start executing
commands yet. Extract structured fields from their description:
  - affected service or component
  - observed symptom (what is wrong)
  - when it started / trigger if known
  - severity (informational / degraded / outage)
  - any preliminary hypotheses the user already hinted at

Respond with a brief confirmation summarizing what you understood in 3-5
bullet points. End with: "Ready to plan investigation? (yes/no)"
"#;

const PLAN_PROMPT: &str = r#"You are in INCIDENT MODE — PLAN phase.

Goal: propose the next 2-4 diagnostic commands/queries to gather evidence.
Each step must declare:
  - RATIONALE: why this specific command, what signal it produces
  - COMMAND: the exact command, wrapped in <EXECUTE></EXECUTE>
  - EXPECTED: what outputs would be informative vs. noise

Order by information-gain-per-cost: cheap signals first, expensive ones
(full log dumps, long traces) last. Stop at 4 steps — more can be planned
in the next loop if current evidence is inconclusive.
"#;

const INVESTIGATE_PROMPT: &str = r#"You are in INCIDENT MODE — INVESTIGATE phase.

You are executing the plan and observing outputs. Each command result is
captured as EVIDENCE and you can reference it by id. For every observation,
TAG it with the hypothesis it supports or contradicts.

Guidelines:
  - Do NOT draw conclusions yet — you are collecting, not concluding.
  - If an unexpected signal appears, note it; may warrant a reroute to PLAN.
  - Stop and request diagnose when you have enough evidence for at least
    one testable hypothesis backed by ≥2 independent pieces of evidence.
"#;

const DIAGNOSE_PROMPT: &str = r#"You are in INCIDENT MODE — DIAGNOSE phase.

Formulate hypotheses about the root cause. For EACH hypothesis:
  - Write the claim in one sentence.
  - List the evidence IDs that SUPPORT it.
  - List evidence IDs that CONTRADICT it (if any).
  - Assign confidence 0.0–1.0.

Propose via structured output:
  <HYPOTHESIS>
    claim: "..."
    supporting: [ev-id-1, ev-id-2]
    contradicting: [ev-id-3]
    confidence: 0.8
  </HYPOTHESIS>

If no hypothesis reaches confidence > 0.6 with ≥2 supporting evidences,
REQUEST REROUTE TO PLAN. Do not fabricate support — intellectual honesty
beats a plausible-sounding wrong answer.
"#;

const REPORT_PROMPT: &str = r#"You are in INCIDENT MODE — REPORT phase.

Produce the post-mortem. Structure:

  ## Root Cause
  One-sentence root cause, stated plainly.

  ## Evidence Chain
  Numbered list: evidence-id → observation → implication.

  ## Remediation
  What was done, what the user should do next, and any monitoring to set up
  to catch the same issue earlier next time.

  ## Runbook seed
  If this class of incident is likely to recur, propose a compact runbook:
  triggers, evidence to gather, decision tree, actions. This seed can be
  promoted to a reusable skill.

No speculation beyond what evidence supports. If confidence is low, say so.
"#;

const DEFAULT_PROMPT: &str = "Incident mode — phase unknown. Await next instruction.";
