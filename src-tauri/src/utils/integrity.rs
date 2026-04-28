// ── integrity — boot-time self-tampering detection ─────────────────────────
//
// Computes the SHA-256 of the running executable at startup and compares it
// against an expected value embedded at compile time (or pinned the first
// time we see a "trusted" hash).
//
// Threat model:
//   • CASUAL TAMPERING (patcher tools editing bytes in the binary to bypass
//     a license check, blocklist, etc.): DETECTED — hash will mismatch.
//   • ADVANCED ATTACKER (rebuilds from source, modifies, re-signs): NOT
//     detected by this layer. That's what code signing + cert pinning are
//     for, not us.
//
// IMPORTANT NUANCES
//   • In `cargo run` / dev builds, the embedded hash is empty so the check
//     becomes a no-op (otherwise no developer iteration would ever boot).
//   • For release: a trust-on-first-use ("TOFU") strategy stores the first
//     observed hash to %APPDATA%\Lucy\.integrity. After that, mismatches
//     trigger an error. This protects post-install tampering, NOT the
//     download channel itself (use code signing for that).
//
// This is NOT a strong defense. It's one extra layer of cost.

use std::path::PathBuf;
use sha2::{Digest, Sha256};

/// Read the running executable's bytes and return its SHA-256 (hex).
/// Returns `None` if the path can't be resolved or the file can't be read.
pub fn current_exe_sha256() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let bytes = std::fs::read(&exe).ok()?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Some(hex_encode(&h.finalize()))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Path to the trust-on-first-use anchor file (%APPDATA%\Lucy\.integrity).
fn anchor_path() -> Option<PathBuf> {
    let mut p = PathBuf::from(std::env::var("APPDATA").ok()?);
    p.push("Lucy");
    let _ = std::fs::create_dir_all(&p);
    p.push(".integrity");
    Some(p)
}

/// Verdict for the boot-time check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityVerdict {
    /// First boot with this binary — anchor written, app may proceed.
    /// In TOFU model this is the "trust" event.
    FirstBoot,
    /// Hash matches the stored anchor. All good.
    Verified,
    /// Hash differs from the anchor. Either the binary was patched OR a
    /// legitimate update overwrote it. Caller must decide what to do.
    Mismatch { expected: String, actual: String },
    /// Couldn't compute or read the hash (read-only filesystem, AV lock,
    /// portable launch from CD, etc.). Not necessarily an attack — fail open.
    Unknown,
}

/// Run the trust-on-first-use integrity check.
///
/// Behaviour:
///   1. Compute SHA-256 of the running .exe.
///   2. Compare to `%APPDATA%\Lucy\.integrity` (created on first boot).
///   3. Return one of FirstBoot / Verified / Mismatch / Unknown.
///
/// Caller should LOG the verdict but probably NOT abort on Mismatch unless
/// you also ship a signed updater that updates `.integrity` after legit
/// updates. Otherwise every release would lock users out.
pub fn check_self_integrity() -> IntegrityVerdict {
    // In debug builds, skip — every cargo run produces a different hash.
    if cfg!(debug_assertions) {
        return IntegrityVerdict::Unknown;
    }
    let actual = match current_exe_sha256() {
        Some(h) => h,
        None    => return IntegrityVerdict::Unknown,
    };
    let anchor = match anchor_path() {
        Some(p) => p,
        None    => return IntegrityVerdict::Unknown,
    };
    match std::fs::read_to_string(&anchor) {
        Ok(stored) => {
            let stored = stored.trim().to_string();
            if stored.is_empty() {
                let _ = std::fs::write(&anchor, &actual);
                IntegrityVerdict::FirstBoot
            } else if stored == actual {
                IntegrityVerdict::Verified
            } else {
                IntegrityVerdict::Mismatch { expected: stored, actual }
            }
        }
        Err(_) => {
            // First boot OR anchor was deleted (user cleaned %APPDATA%).
            // Treat as TOFU: write the new anchor.
            let _ = std::fs::write(&anchor, &actual);
            IntegrityVerdict::FirstBoot
        }
    }
}

/// Detect whether a Windows debugger is attached to the current process.
/// Returns false on non-Windows (we don't ship for other platforms yet).
///
/// Bypassable in seconds with `ScyllaHide`, `TitanHide`, etc., but raises
/// the bar against scriptkiddies poking with x64dbg's defaults.
#[cfg(windows)]
pub fn debugger_present() -> bool {
    // SAFETY: IsDebuggerPresent is a stable Win32 API with no preconditions.
    unsafe { winapi::um::debugapi::IsDebuggerPresent() != 0 }
}

#[cfg(not(windows))]
pub fn debugger_present() -> bool {
    false
}
