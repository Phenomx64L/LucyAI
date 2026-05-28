// ── hash_chain.rs — SHA-256 chain verifier for incident actions ─────────
//
// Lucy stores incident actions in a SHA-256-linked chain:
//
//   chain_hash[N] = sha256(chain_hash[N-1] || id || command || executed_at)
//   chain_hash[0] = sha256("GENESIS" || id || command || executed_at)
//
// This module recomputes the chain from scratch and compares each row's
// stored hash against the computed expected hash. Any mismatch means the
// row was tampered with (or the algorithm changed silently — both are
// forensically interesting).
//
// Surfaced via the Audit / Incident view as "Verify chain" button.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::commands::metrics::shared_db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainVerifyReport {
    pub incident_id: String,
    pub total_actions: i64,
    pub verified_ok: i64,
    pub mismatches: Vec<ChainMismatch>,
    /// True when every row passed. Renders the green "verified" badge.
    pub fully_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainMismatch {
    pub action_id: String,
    pub position: i64,  // 1-based index in chronological order
    pub stored_hash: String,
    pub expected_hash: String,
    /// Short reason for the operator: 'missing_hash' | 'algorithm_mismatch'.
    pub reason: String,
}

/// Recompute and verify the chain for `incident_id`. O(N) over the actions
/// of that incident — typically <50 rows, so sub-millisecond.
#[tauri::command]
pub async fn verify_incident_chain(incident_id: String) -> Result<ChainVerifyReport, String> {
    shared_db(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, command, executed_at, chain_hash
             FROM incident_action
             WHERE incident_id = ?1
             ORDER BY executed_at ASC, id ASC",
        ).map_err(|e| format!("prepare: {}", e))?;

        let rows: Vec<(String, String, i64, String)> = stmt
            .query_map(params![incident_id], |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            )))
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        let total = rows.len() as i64;
        let mut report = ChainVerifyReport {
            incident_id: incident_id.clone(),
            total_actions: total,
            verified_ok: 0,
            mismatches: Vec::new(),
            fully_valid: true,
        };

        // Walk the chain. prev_hash starts with the literal "GENESIS" token
        // to match the insertion logic. If anyone ever changes that token,
        // this verifier breaks — that's intentional (we WANT to flag a
        // protocol change as a chain break).
        let mut prev_hash = String::from("GENESIS");
        for (i, (id, command, executed_at, stored_hash)) in rows.iter().enumerate() {
            let expected = compute_chain_hash(&prev_hash, id, command, *executed_at);
            if stored_hash.is_empty() {
                report.mismatches.push(ChainMismatch {
                    action_id: id.clone(),
                    position: (i + 1) as i64,
                    stored_hash: String::new(),
                    expected_hash: expected.clone(),
                    reason: "missing_hash".to_string(),
                });
                report.fully_valid = false;
            } else if stored_hash != &expected {
                report.mismatches.push(ChainMismatch {
                    action_id: id.clone(),
                    position: (i + 1) as i64,
                    stored_hash: stored_hash.clone(),
                    expected_hash: expected.clone(),
                    reason: "algorithm_mismatch".to_string(),
                });
                report.fully_valid = false;
            } else {
                report.verified_ok += 1;
            }
            // Move forward — even on mismatch, we continue with the STORED
            // hash so we don't cascade mismatches downstream. The operator
            // sees exactly which rows broke, not "everything after row 3".
            prev_hash = stored_hash.clone();
        }
        Ok(report)
    })
}

fn compute_chain_hash(prev: &str, id: &str, command: &str, executed_at: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev.as_bytes());
    hasher.update(id.as_bytes());
    hasher.update(command.as_bytes());
    hasher.update(executed_at.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_chain_produces_deterministic_hash() {
        // Same inputs → same hash, always. If this ever changes between
        // releases, the entire stored history would re-verify as broken.
        let a = compute_chain_hash("GENESIS", "abc-123", "ls /tmp", 1730000000);
        let b = compute_chain_hash("GENESIS", "abc-123", "ls /tmp", 1730000000);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64, "SHA-256 hex must be 64 chars");
    }

    #[test]
    fn different_command_produces_different_hash() {
        // Trivial tamper-detection check.
        let a = compute_chain_hash("GENESIS", "id1", "rm /etc/passwd", 1);
        let b = compute_chain_hash("GENESIS", "id1", "ls /etc/passwd", 1);
        assert_ne!(a, b, "different commands must hash differently");
    }

    #[test]
    fn chain_propagates_prev_hash() {
        // Same id+command but different prev → different hash. This is the
        // anti-replay property that prevents an attacker from re-inserting
        // a row from one chain into another.
        let h1 = compute_chain_hash("GENESIS",  "id1", "cmd", 1);
        let h2 = compute_chain_hash("DIFFERENT", "id1", "cmd", 1);
        assert_ne!(h1, h2);
    }
}
