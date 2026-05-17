pub mod logging;
pub mod shell;
pub mod db;
pub mod integrity;
pub mod error;

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
