// v1.7.108 (Sprint #1 follow-up audit C2) — Secret scrubber for audit logs.
//
// Before this module, every audit-log writeln! in shell.rs / local.rs / cscript
// dumped the full user script verbatim. If the LLM ran a command like
// `curl -H "Authorization: Bearer sk-real-key-12345" https://api.x.com`,
// that Bearer token landed in plaintext inside %APPDATA%\Lucy\logs\lucy_audit.log
// — a file with default-user read perms. Anyone with local access (other
// processes, malicious VS Code extensions, backup software) could harvest it.
//
// Design choices:
//   * Conservative patterns ONLY. We accept false negatives over false
//     positives — an over-eager scrubber that masks legitimate audit content
//     defeats the point of the audit trail.
//   * Patterns compiled once via Lazy — called per audit-log line, must be
//     branch-predictable and zero-alloc on the no-secret path.
//   * Always returns String (not Cow) so call sites stay uniform. The
//     no-secret path is one short_circuit() check + one .to_string() on a
//     short string, which is cheap compared to the syscall to write the line.
//   * NEVER scrubs the ALLOWED command (the one we're about to execute) —
//     only the AUDIT TRAIL. The command itself goes to the OS shell as-is so
//     the user's intent is preserved.

use once_cell::sync::Lazy;
use regex::Regex;

// (label, pattern, replacement_template).
// Replacement uses $0 for the matched key name so logs still tell us WHICH
// kind of secret was present (forensic value) without exposing the value.
struct ScrubPattern {
    re: Regex,
    replacement: &'static str,
}

static PATTERNS: Lazy<Vec<ScrubPattern>> = Lazy::new(|| {
    vec![
        // 1. URL-embedded credentials:  scheme://user:password@host
        //    Catches postgres://admin:Passw0rd@db, https://user:tok@…
        //    Whitelist of common safe schemes so we don't mangle `file://`.
        ScrubPattern {
            re: Regex::new(
                r"(?i)\b((?:https?|postgres|postgresql|mysql|mongodb|redis|amqp|smb|ftp|ssh)://)([^:@/\s]+):([^@/\s]{1,200})@"
            ).expect("scrub regex 1"),
            replacement: "$1[REDACTED]:[REDACTED]@",
        },
        // 2. Authorization: Bearer <tok>  — HTTP header form
        ScrubPattern {
            re: Regex::new(
                r"(?i)(Authorization\s*[:=]\s*Bearer\s+)\S{8,}"
            ).expect("scrub regex 2"),
            replacement: "${1}[REDACTED]",
        },
        // 3. Bare key=value secrets in flags/env:
        //    --password=Foo, -p Foo (skipped: too noisy), token=Foo, api_key=Foo,
        //    secret=Foo, apikey=Foo. Accepts both = and : separators.
        //    Requires non-space value of >= 4 chars to skip empty/placeholder.
        // v1.7.232 (Phase-2 SCRUB-1): the boundary is `(^|[^A-Za-z0-9])`, NOT `\b`.
        // `\b` treats `_` as a word char, so it FAILS to match the dominant
        // SCREAMING_SNAKE env-var form where the keyword is an underscore-joined
        // suffix (DB_PASSWORD=, AWS_SECRET_ACCESS_KEY=, NVIDIA_API_KEY=) — those
        // leaked verbatim. `[^A-Za-z0-9]` treats `_` as a boundary. The boundary
        // char is captured ($1) and preserved in the replacement so the log line
        // isn't mangled; $2 = keyword, $3 = value (dropped).
        ScrubPattern {
            re: Regex::new(
                r"(?i)(^|[^A-Za-z0-9])(password|passwd|pwd|api[-_]?key|apikey|access[-_]?key|secret[-_]?key|secret|token|auth[-_]?token|client[-_]?secret)\s*[:=]\s*[\x22\x27]?([^\s\x22\x27]{4,})[\x22\x27]?"
            ).expect("scrub regex 3"),
            replacement: "${1}${2}=[REDACTED]",
        },
        // 4. AWS access key ID — fixed prefix + 16 alnum
        ScrubPattern {
            re: Regex::new(r"\bAKIA[0-9A-Z]{16}\b").expect("scrub regex 4"),
            replacement: "[REDACTED_AWS_KEY]",
        },
        // 5. Anthropic API keys (sk-ant-…) and OpenAI keys (sk-…). The sk-
        //    prefix family is very specific to LLM provider tokens.
        ScrubPattern {
            re: Regex::new(r"\bsk-(?:ant-)?[A-Za-z0-9_\-]{20,}\b").expect("scrub regex 5"),
            replacement: "[REDACTED_LLM_KEY]",
        },
        // 6. GitHub PATs — fixed prefixes ghp_, gho_, ghs_, ghr_, github_pat_
        ScrubPattern {
            re: Regex::new(r"\b(?:ghp|gho|ghs|ghr|github_pat)_[A-Za-z0-9_]{20,}\b").expect("scrub regex 6"),
            replacement: "[REDACTED_GH_TOKEN]",
        },
        // 7. Private key headers — if a script somehow embeds a PEM block,
        //    redact the whole inline run. We do NOT try to span newlines
        //    (logs are line-based) but a single-line "-----BEGIN…END-----"
        //    is still a leak shape worth catching.
        ScrubPattern {
            re: Regex::new(
                r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]{0,4000}?-----END [A-Z ]*PRIVATE KEY-----"
            ).expect("scrub regex 7"),
            replacement: "[REDACTED_PRIVATE_KEY]",
        },
        // 8. Provider keys Lucy actually consumes: NVIDIA NIM (`nvapi-`) and
        //    Tavily (`tvly-`). v1.7.232 (Phase-2 SCRUB-3) — pattern #5 only covered
        //    the sk-/sk-ant- family, so bare/assignment forms of these leaked.
        ScrubPattern {
            re: Regex::new(
                r"\b(?:nvapi-|tvly-)[A-Za-z0-9_\-]{16,}\b"
            ).expect("scrub regex 8"),
            replacement: "[REDACTED_PROVIDER_KEY]",
        },
    ]
});

// Quick short-circuit: if none of the secret marker substrings are present,
// skip the regex pass entirely. Most audit lines are mundane PowerShell.
fn has_potential_secret(s: &str) -> bool {
    // Case-insensitive ASCII check; cheaper than running all 7 regexes.
    let bytes = s.as_bytes();
    // We're matching against the lowercase form of any of these markers.
    static MARKERS: &[&[u8]] = &[
        // v1.7.232 (Phase-2 SCRUB-2): `pwd` was missing, so `Pwd=`/`pwd=`
        // connection-string passwords short-circuited past the regex (which DOES
        // list pwd). v1.7.232 (Phase-2 SCRUB-3): `nvapi-`/`tvly-` added so provider
        // keys reach the new pattern #8.
        b"password", b"passwd", b"pwd", b"api_key", b"apikey", b"api-key",
        b"secret", b"token", b"bearer", b"authorization",
        b"akia", b"sk-", b"nvapi-", b"tvly-", b"ghp_", b"gho_", b"ghs_", b"ghr_",
        b"github_pat_", b"-----begin",
        b"://", // URL with possible embedded creds
    ];
    let lower: Vec<u8> = bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
    MARKERS.iter().any(|m| memmem(&lower, m))
}

// Tiny memmem — avoid pulling memchr just for one call site.
fn memmem(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > hay.len() { return false; }
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Scrub potential secrets from an audit-log message.
///
/// Returns a freshly-allocated String. The no-secret path takes the early
/// return after the marker scan, so cost on typical lines is O(n) over the
/// line length with no regex work.
pub fn scrub_for_audit(input: &str) -> String {
    if !has_potential_secret(input) {
        return input.to_string();
    }
    let mut out = input.to_string();
    for p in PATTERNS.iter() {
        // replace_all returns Cow — we keep it as owned to chain.
        out = p.re.replace_all(&out, p.replacement).into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_plain_lines() {
        let s = "Get-Process | Where-Object {$_.CPU -gt 100}";
        assert_eq!(scrub_for_audit(s), s);
    }

    #[test]
    fn redacts_bearer_token() {
        let s = "curl -H \"Authorization: Bearer sk-ant-api03-realkey-12345-abcde-fghij\" https://api.anthropic.com";
        let out = scrub_for_audit(s);
        assert!(!out.contains("sk-ant-api03-realkey"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_url_embedded_creds() {
        let s = "psql postgres://admin:Sup3rSecret!@db.internal:5432/prod -c 'select 1'";
        let out = scrub_for_audit(s);
        assert!(!out.contains("Sup3rSecret"));
        assert!(out.contains("[REDACTED]:[REDACTED]"));
        // Scheme and host preserved
        assert!(out.contains("postgres://"));
        assert!(out.contains("db.internal"));
    }

    #[test]
    fn redacts_password_flag() {
        let s = "mysql --user=root --password=hunter2 -e 'select 1'";
        let out = scrub_for_audit(s);
        assert!(!out.contains("hunter2"));
    }

    #[test]
    fn redacts_aws_access_key() {
        let s = "export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
        let out = scrub_for_audit(s);
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(out.contains("[REDACTED_AWS_KEY]"));
    }

    #[test]
    fn redacts_openai_style_key() {
        let s = "OPENAI_API_KEY=sk-proj-abcdef0123456789abcdef0123456789";
        let out = scrub_for_audit(s);
        assert!(!out.contains("sk-proj-abcdef"));
    }

    #[test]
    fn redacts_github_pat() {
        let s = "git push https://ghp_aaaaBBBBccccDDDD1111222233334444@github.com/x/y";
        let out = scrub_for_audit(s);
        assert!(!out.contains("ghp_aaaaBBBB"));
    }

    #[test]
    fn short_circuit_skips_regex_for_plain_text() {
        // Just ensure the function doesn't error on plain text and returns same bytes.
        let s = "ls -la /etc/hosts";
        let out = scrub_for_audit(s);
        assert_eq!(out, s);
    }

    #[test]
    fn handles_multiple_secrets_in_one_line() {
        let s = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE && password=hunter2";
        let out = scrub_for_audit(s);
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!out.contains("hunter2"));
    }

    #[test]
    fn preserves_short_value_after_keyword() {
        // 'token=' followed by < 4 chars should not match — too likely a
        // false positive (e.g. echo token=$x in scripts).
        let s = "echo token=$x";
        let out = scrub_for_audit(s);
        assert_eq!(out, s);
    }

    #[test]
    fn does_not_redact_file_url_with_username_only() {
        // file://user@host has no password — should not mangle.
        let s = "smbclient //fileserver/share -U domain\\\\user";
        let out = scrub_for_audit(s);
        assert_eq!(out, s);
    }

    // ── v1.7.232 (Phase-2) regression tests ──────────────────────────────────

    #[test]
    fn scrub1_redacts_screaming_snake_env_secrets() {
        // The dominant real-world form: keyword is an underscore-joined suffix.
        // `\b` failed here because `_` is a word char; `(^|[^A-Za-z0-9])` fixes it.
        for (line, secret) in [
            ("export DB_PASSWORD=hunter2value", "hunter2value"),
            ("AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMIK7MDENGbPxRfiCYEXAMPLEKEY", "wJalrXUtnFEMIK7MDENGbPxRfiCYEXAMPLEKEY"),
            ("$env:MY_SECRET_KEY='supersecretvalue123'", "supersecretvalue123"),
            ("FOO_TOKEN=abcd1234efgh5678", "abcd1234efgh5678"),
        ] {
            let out = scrub_for_audit(line);
            assert!(!out.contains(secret), "SCRUB-1 leaked {:?} in {:?} -> {:?}", secret, line, out);
            assert!(out.contains("[REDACTED]"), "expected redaction marker in {:?}", out);
        }
        // The underscore boundary char must be preserved (log line not mangled).
        assert!(
            scrub_for_audit("export DB_PASSWORD=hunter2value").contains("DB_PASSWORD"),
            "boundary char dropped — key name mangled"
        );
    }

    #[test]
    fn scrub2_redacts_connection_string_pwd() {
        // `Pwd=` (ADO.NET/ODBC) and bare `pwd=` were short-circuited because the
        // MARKERS gate omitted `pwd` (defeating pattern #3, which already lists it).
        for (line, secret) in [
            ("Server=db;Uid=admin;Pwd=Passw0rd;", "Passw0rd"),
            ("pwd=Passw0rdXYZ", "Passw0rdXYZ"),
            ("PWD=Passw0rdXYZ", "Passw0rdXYZ"),
        ] {
            let out = scrub_for_audit(line);
            assert!(!out.contains(secret), "SCRUB-2 leaked {:?} in {:?} -> {:?}", secret, line, out);
        }
    }

    #[test]
    fn scrub3_redacts_nvidia_and_tavily_keys() {
        for (line, secret) in [
            ("$env:NVIDIA_NIM_KEY = \"nvapi-abcdef0123456789ABCDEF0123\"", "nvapi-abcdef0123456789ABCDEF0123"),
            ("using tvly-abcdef0123456789ABCDEF for search", "tvly-abcdef0123456789ABCDEF"),
        ] {
            let out = scrub_for_audit(line);
            assert!(!out.contains(secret), "SCRUB-3 leaked {:?} in {:?} -> {:?}", secret, line, out);
            assert!(out.contains("[REDACTED_PROVIDER_KEY]"), "expected provider-key marker in {:?}", out);
        }
    }
}
