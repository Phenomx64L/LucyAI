pub mod logging;
pub mod shell;
pub mod db;
pub mod integrity;
pub mod error;
// v1.7.9 — placeholder pattern detector used by execute_powershell /
// execute_cmd / execute_reg to refuse skill-body example values.
pub mod placeholder_guard;
// v1.7.19 — SIMD-dispatched cosine similarity (AVX-512 / AVX2 / scalar).
// Consolidates three duplicated implementations into one runtime-
// dispatched entry point. Boot logs which backend is active.
pub mod simd_cosine;
// v1.7.108 audit C2 — secret scrubber for audit logs. All writeln! sites
// in shell.rs / local.rs that dump user scripts now go through
// scrub_for_audit so Bearer tokens, password=, URL-embedded creds, AWS
// keys, GitHub PATs and LLM provider keys never reach disk.
pub mod secret_scrubber;

/// Safely truncate a string at a char boundary, never panicking.
/// Returns a slice of at most `max_bytes` bytes, ending at a valid UTF-8 boundary.
pub fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the largest char boundary <= max_bytes
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
