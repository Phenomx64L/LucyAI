// ── error.rs — Typed error model for Tauri commands ───────────────────────
//
// Background
// -----------
// Lucy's commands all return `Result<T, String>` today. Strings work fine for
// developer logs but they suck for the frontend:
//   • No machine-parseable code → impossible to handle errors generically
//     (e.g. "show retry dialog on Timeout, redirect to settings on
//      PermissionDenied").
//   • No i18n — error strings are baked in Spanish/English in the backend.
//   • Lost structure — e.g. a "not found" reason gets mixed with the path
//     that wasn't found, parsing it back is fragile.
//
// `LucyError` is the typed replacement. Each variant carries:
//   • A stable `code` (string, serialized for the frontend)
//   • A human `message` (developer-facing, shown if no i18n key matches)
//   • Optional `detail` fields specific to the variant
//
// Migration strategy
// ------------------
// We don't rewrite every command at once. Instead:
//   1. New commands use `Result<T, LucyError>`.
//   2. Old commands stay on `Result<T, String>` and migrate one at a time.
//   3. `From<String>` is impl'd so a `Result<T, LucyError>` can absorb a
//      `String` error via `?` operator with `.map_err(LucyError::from)`.
//   4. Frontend's `invokeTyped<R>` can detect the LucyError shape (object with
//      `code` field) vs the legacy plain string and adapt.
//
// Wire format (serialized to frontend)
// -------------------------------------
// Tagged JSON via serde's `#[serde(tag = "code", content = "data")]`:
//
//   { "code": "PermissionDenied", "data": { "rule": "block_destructive", "message": "..." } }
//   { "code": "NotFound",         "data": { "what": "file", "path": "/x/y" } }
//   { "code": "Timeout",          "data": { "operation": "execute_powershell", "secs": 60 } }
//   { "code": "Internal",         "data": { "message": "<freeform>" } }
//
// The frontend can switch on `err.code`, retry on Timeout, prompt on
// PermissionDenied, etc. Backward-compatible with current string errors
// because old commands still emit plain strings.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Typed error for Tauri commands. Serializes to a tagged JSON object the
/// frontend can switch on.
///
/// IMPORTANT: when adding a new variant, also:
///   1) Pick a stable `code` name that won't change (used by the frontend
///      to dispatch behavior). Snake_case avoided in favor of PascalCase
///      to match TypeScript discriminated-union conventions.
///   2) Document the variant's intended usage so consumers don't pick
///      `Internal` as a catch-all when a more specific code exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "code", content = "data")]
pub enum LucyError {
    /// A user-defined or built-in permission rule blocked the operation.
    /// `rule` is the matched rule id/name; `message` is what to show the user.
    PermissionDenied { rule: String, message: String },

    /// Resource lookup failed. `what` is "file"|"host"|"key"|...; `path` is
    /// the identifier the caller tried to look up.
    NotFound { what: String, path: String },

    /// Input failed validation BEFORE any work was attempted. `field` names
    /// the offending parameter; `reason` is a short explanation.
    InvalidInput { field: String, reason: String },

    /// Operation exceeded its deadline. `operation` is the command name;
    /// `secs` is the timeout that fired.
    Timeout { operation: String, secs: u64 },

    /// Underlying I/O / system call failed (file ops, network, child process).
    /// `source` is the OS-level message; `context` is what we were doing.
    Io { source: String, context: String },

    /// Path traversal or other security-policy violation (separate from
    /// PermissionDenied which is user-rule-driven; this one is hardcoded).
    SecurityBlock { reason: String, hint: Option<String> },

    /// LLM provider returned an error or refused. Wraps provider name + their
    /// message so the frontend can re-route to a different model if desired.
    ProviderError { provider: String, message: String },

    /// Catch-all for genuinely unexpected internal errors. Use a more
    /// specific variant when possible — `Internal` should be rare.
    Internal { message: String },
}

impl LucyError {
    /// Stable error code (matches the serde tag, no allocation needed).
    #[allow(dead_code)]  // ready for frontend error-code routing
    pub fn code(&self) -> &'static str {
        match self {
            LucyError::PermissionDenied { .. } => "PermissionDenied",
            LucyError::NotFound          { .. } => "NotFound",
            LucyError::InvalidInput      { .. } => "InvalidInput",
            LucyError::Timeout           { .. } => "Timeout",
            LucyError::Io                { .. } => "Io",
            LucyError::SecurityBlock     { .. } => "SecurityBlock",
            LucyError::ProviderError     { .. } => "ProviderError",
            LucyError::Internal          { .. } => "Internal",
        }
    }

    /// Construct an Internal error from any displayable value.
    #[allow(dead_code)]  // convenience constructor for future command modules
    pub fn internal<E: fmt::Display>(e: E) -> Self {
        LucyError::Internal { message: e.to_string() }
    }

    /// Wrap an I/O failure with the operation context. Most callers should
    /// prefer this over `Internal` — it gives the frontend enough structure
    /// to show a meaningful retry button.
    pub fn io<E: fmt::Display>(context: impl Into<String>, e: E) -> Self {
        LucyError::Io { source: e.to_string(), context: context.into() }
    }
}

impl fmt::Display for LucyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LucyError::PermissionDenied { rule, message } =>
                write!(f, "Permission denied (rule: {}): {}", rule, message),
            LucyError::NotFound { what, path } =>
                write!(f, "{} not found: {}", what, path),
            LucyError::InvalidInput { field, reason } =>
                write!(f, "Invalid input on '{}': {}", field, reason),
            LucyError::Timeout { operation, secs } =>
                write!(f, "Timeout: '{}' exceeded {}s", operation, secs),
            LucyError::Io { source, context } =>
                write!(f, "I/O error during {}: {}", context, source),
            LucyError::SecurityBlock { reason, hint } => {
                write!(f, "Security block: {}", reason)?;
                if let Some(h) = hint { write!(f, " (hint: {})", h)?; }
                Ok(())
            }
            LucyError::ProviderError { provider, message } =>
                write!(f, "Provider {} error: {}", provider, message),
            LucyError::Internal { message } =>
                write!(f, "Internal error: {}", message),
        }
    }
}

// ── Backward-compat: absorb String/&str errors via `?` ──────────────────────
// This is what lets old `Result<T, String>` migrate gradually: a function
// returning `Result<T, LucyError>` can still call legacy helpers that return
// `Result<T, String>` and use `?` with `.map_err(Into::into)`.

impl From<String> for LucyError {
    fn from(s: String) -> Self { LucyError::Internal { message: s } }
}

impl From<&str> for LucyError {
    fn from(s: &str) -> Self { LucyError::Internal { message: s.to_string() } }
}

impl From<std::io::Error> for LucyError {
    fn from(e: std::io::Error) -> Self {
        LucyError::Io { source: e.to_string(), context: "io".to_string() }
    }
}

impl std::error::Error for LucyError {}
