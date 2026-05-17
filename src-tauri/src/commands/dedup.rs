// ── Tool-run dedup (Tier 2 #6, agentmemory-inspired) ────────────────────
//
// Defense against tool-call loops in the agent. When Lucy's reasoning
// stage produces the SAME tool name with the SAME (or near-same) input
// twice within 5 minutes, that's almost always:
//   • A streaming hiccup that made the LLM re-emit the same <TOOL>...
//   • A failed parse loop where the agent keeps proposing the same fix
//   • A genuine duplicate that wastes tokens + (worse) re-runs a
//     potentially destructive command
//
// agentmemory dedup.ts uses sha256 over (session, tool, input[:500]) with
// a 5-min TTL. We mirror that exactly — in-memory only (5 min doesn't
// merit DB writes), thread-safe Mutex, lazy cleanup on every call so the
// map never grows unbounded.
//
// Single-call atomic API: dedup_acquire returns whether the slot was
// newly acquired (callers proceed) or already held (callers should skip
// the tool execution and surface a warning to the user).

use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a tool-run signature blocks re-execution. Matches agentmemory.
const TTL: Duration = Duration::from_secs(5 * 60);

/// Max input chars folded into the hash. Long stdin / file contents would
/// over-pin the hash; the first 500 chars are enough to detect "same call".
const INPUT_PREFIX: usize = 500;

struct Entry {
    inserted: Instant,
}

static STORE: Lazy<Mutex<HashMap<String, Entry>>> =
    Lazy::new(|| Mutex::new(HashMap::with_capacity(128)));

fn hash_key(session_id: &str, tool_name: &str, tool_input: &str) -> String {
    let trimmed_input: String = tool_input.chars().take(INPUT_PREFIX).collect();
    let mut h = Sha256::new();
    h.update(session_id.as_bytes());
    h.update(b":");
    h.update(tool_name.as_bytes());
    h.update(b":");
    h.update(trimmed_input.as_bytes());
    let bytes = h.finalize();
    // Hex-encode without pulling in another crate
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes { s.push_str(&format!("{:02x}", b)); }
    s
}

#[derive(serde::Serialize)]
pub struct DedupResult {
    /// True iff this is a fresh slot. False means the same (session, tool,
    /// input) was seen within the last 5 minutes and the caller should
    /// skip the tool execution.
    pub acquired: bool,
    /// When acquired=false: seconds elapsed since the previous record.
    /// When acquired=true: 0.
    pub prev_age_seconds: u64,
}

/// Atomic check-and-record. Always cleans expired entries first so the
/// map stays small (no separate janitor task needed).
#[tauri::command]
pub fn dedup_acquire(
    session_id: String,
    tool_name:  String,
    tool_input: String,
) -> Result<DedupResult, String> {
    let mut store = STORE.lock().map_err(|e| format!("dedup mutex: {}", e))?;
    let now = Instant::now();

    // Lazy cleanup — drop everything older than TTL. Cheap: bounded
    // map, single pass, no allocations.
    store.retain(|_, e| now.duration_since(e.inserted) < TTL);

    let key = hash_key(&session_id, &tool_name, &tool_input);
    if let Some(prev) = store.get(&key) {
        let age = now.duration_since(prev.inserted);
        // Within TTL → duplicate.
        return Ok(DedupResult {
            acquired: false,
            prev_age_seconds: age.as_secs(),
        });
    }

    store.insert(key, Entry { inserted: now });
    Ok(DedupResult { acquired: true, prev_age_seconds: 0 })
}

/// Explicit release — usually unnecessary (TTL eviction handles cleanup)
/// but useful for tests or when a tool call definitively failed and the
/// caller wants to allow an immediate retry. Returns true if the key was
/// found and removed.
#[tauri::command]
pub fn dedup_release(
    session_id: String,
    tool_name:  String,
    tool_input: String,
) -> Result<bool, String> {
    let mut store = STORE.lock().map_err(|e| format!("dedup mutex: {}", e))?;
    let key = hash_key(&session_id, &tool_name, &tool_input);
    Ok(store.remove(&key).is_some())
}

/// For diagnostics — current count of held slots.
#[tauri::command]
pub fn dedup_stats() -> Result<i64, String> {
    let store = STORE.lock().map_err(|e| format!("dedup mutex: {}", e))?;
    Ok(store.len() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_call_acquires() {
        let _ = dedup_release("t1".into(), "ps".into(), "Get-Process".into());
        let r = dedup_acquire("t1".into(), "ps".into(), "Get-Process".into()).unwrap();
        assert!(r.acquired);
        assert_eq!(r.prev_age_seconds, 0);
    }

    #[test]
    fn second_call_blocked() {
        let _ = dedup_release("t2".into(), "ps".into(), "Get-Service".into());
        let _ = dedup_acquire("t2".into(), "ps".into(), "Get-Service".into()).unwrap();
        let r = dedup_acquire("t2".into(), "ps".into(), "Get-Service".into()).unwrap();
        assert!(!r.acquired);
    }

    #[test]
    fn different_session_not_blocked() {
        let _ = dedup_release("t3a".into(), "ps".into(), "ls".into());
        let _ = dedup_release("t3b".into(), "ps".into(), "ls".into());
        let _ = dedup_acquire("t3a".into(), "ps".into(), "ls".into()).unwrap();
        let r = dedup_acquire("t3b".into(), "ps".into(), "ls".into()).unwrap();
        assert!(r.acquired);
    }

    #[test]
    fn different_input_not_blocked() {
        let _ = dedup_release("t4".into(), "ps".into(), "Get-Process explorer".into());
        let _ = dedup_release("t4".into(), "ps".into(), "Get-Process chrome".into());
        let _ = dedup_acquire("t4".into(), "ps".into(), "Get-Process explorer".into()).unwrap();
        let r = dedup_acquire("t4".into(), "ps".into(), "Get-Process chrome".into()).unwrap();
        assert!(r.acquired);
    }

    #[test]
    fn long_input_truncated_consistently() {
        let _ = dedup_release("t5".into(), "ps".into(), "a".repeat(2000)).ok();
        let a = "a".repeat(600);
        let b = "a".repeat(550);
        // Both truncate to "aaaa…" at 500 chars → same hash → block
        let _ = dedup_acquire("t5".into(), "ps".into(), a).unwrap();
        let r = dedup_acquire("t5".into(), "ps".into(), b).unwrap();
        assert!(!r.acquired);
    }

    #[test]
    fn release_allows_retry() {
        let _ = dedup_release("t6".into(), "ps".into(), "kill -9 1".into());
        let _ = dedup_acquire("t6".into(), "ps".into(), "kill -9 1".into()).unwrap();
        let _ = dedup_release("t6".into(), "ps".into(), "kill -9 1".into());
        let r = dedup_acquire("t6".into(), "ps".into(), "kill -9 1".into()).unwrap();
        assert!(r.acquired);
    }
}
