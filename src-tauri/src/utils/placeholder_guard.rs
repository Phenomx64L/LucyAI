// ── placeholder_guard.rs — Pre-execution placeholder scanner (v1.7.9) ────
//
// Backend defense-in-depth on top of the v1.7.6/7/8 framing rules. The
// problem we're solving: when a cybersecurity skill is active, Gemini
// (especially the Flash tier) sometimes ignores the prompt-side
// instruction "do not emit <EXECUTE> with placeholder values" and
// emits one anyway — using literal example strings from the skill
// body like `C:\Ruta\Al\Adjunto\sospechoso.zip` or
// `admin@tudominio.com`. The command then fails on the user's machine
// because those paths don't exist, the autocorrect / agent loop kicks
// in, and the conversation drifts into unrelated tasks.
//
// This module scans every command BEFORE it hits PowerShell / cmd /
// reg / wmic and refuses to execute if it spots a placeholder
// pattern. The error string is crafted so the LLM understands what
// to do next (explain the workflow to the user and ASK for real
// values) — not as a hostile reject.
//
// Patterns are intentionally generous. False positives are tolerable
// (one extra exchange where the LLM has to be more explicit); false
// negatives wreck the user's confidence in the skill system.
//
// ── Skipping the guard ──────────────────────────────────────────────────
//
// Plenty of legit commands contain "example.com" or "/path/to/" as
// real arguments — think DNS queries, GitHub clones. The guard only
// fires when one or more of the high-confidence patterns appear (see
// `is_clearly_placeholder`); single weak signals like "example.com"
// alone do NOT trigger.

use regex::Regex;
use once_cell::sync::Lazy;

/// High-confidence placeholder patterns. If any match, we refuse to
/// execute. These are tuned against the Anthropic-Cybersecurity-Skills
/// repo's actual example strings so the most common false negatives
/// (real skill content) are blocked.
static STRONG_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| vec![
    // ── Spanish-style placeholder paths from the skill library ──
    // C:\Ruta\Al\..., C:\Ruta\Del\..., etc.
    Regex::new(r"(?i)C:\\Ruta\\(Al|Del|Para|A)\\").unwrap(),
    // English equivalents
    Regex::new(r"(?i)C:\\Path\\To\\").unwrap(),
    Regex::new(r"(?i)/path/to/[A-Za-z]").unwrap(),

    // ── Placeholder usernames / emails ──
    Regex::new(r"(?i)\btu[-_]?usuario@").unwrap(),
    Regex::new(r"(?i)\byour[-_]?(user|email|admin)@").unwrap(),
    Regex::new(r"(?i)\b(usuario|admin)@(tudominio|tuempresa|tuorganizacion)\.com").unwrap(),
    Regex::new(r"(?i)\b(admin|user)@(your-tenant|yourtenant|tenant-id)\.onmicrosoft\.com").unwrap(),
    Regex::new(r"(?i)\b[a-z]+@(empresa|company|tenant|dominio|domain)\.(com|local|net)\b").unwrap(),

    // ── Bracketed placeholder tokens ──
    // <TENANT_ID>, [INSERT_DOMAIN], <YOUR-KEY>, {NAME}, [BRACKET], etc.
    Regex::new(r"<(YOUR[_-]|INSERT[_-]|TENANT[_-]|CLIENT[_-]|DOMAIN[_-]|API[_-]|REPLACE)").unwrap(),
    Regex::new(r"\[(YOUR[_-]|INSERT[_-]|TENANT[_-]|CLIENT[_-]|DOMAIN[_-]|API[_-]|REPLACE|TODO|FILL)").unwrap(),
    Regex::new(r"<[a-z]+>\.com").unwrap(),                 // <foo>.com
    Regex::new(r"YOUR[-_](API|TENANT|CLIENT|SUBSCRIPTION|KEY)").unwrap(),

    // ── Common skill-body example IDs ──
    Regex::new(r"(?i)\bPurga[_-]?Phishing[_-]?Incident").unwrap(),
    Regex::new(r"(?i)\bsospechoso\.(eml|msg|zip|exe|pdf|html)\b").unwrap(),
    Regex::new(r"(?i)\bsuspicious\.(eml|msg|zip|exe|pdf|html)\b").unwrap(),
    Regex::new(r"(?i)\bevidence_\d{4}_[a-z]+").unwrap(),  // evidence_2024_case names
    Regex::new(r"(?i)\bcase[-_]20\d{2}[-_]\d+").unwrap(),  // case-2024-001 etc.
]);

/// Lightweight signals — match these only as POSSIBLE placeholders;
/// they need a second corroborating signal before we refuse. Reserved
/// for v1.7.x+ if false negatives keep slipping through.
#[allow(dead_code)]
static WEAK_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| vec![
    Regex::new(r"(?i)\bexample\.com\b").unwrap(),
    Regex::new(r"(?i)\bcontoso\.com\b").unwrap(),
]);

/// Whitelist a command from the guard. Used by sibling modules that
/// run benign synthetic scripts (e.g. our own contract tests).
pub fn looks_like_internal_test(script: &str) -> bool {
    script.contains("cmd-sentinel-")
        || script.contains("Lucy execute_powershell wrapper (auto-generated)")
}

/// Scan a script for placeholder patterns. Returns the first matching
/// pattern's text as evidence so the caller can show the user exactly
/// what tripped the guard.
pub fn detect_placeholders(script: &str) -> Option<String> {
    if script.is_empty() { return None; }
    if looks_like_internal_test(script) { return None; }
    for re in STRONG_PATTERNS.iter() {
        if let Some(m) = re.find(script) {
            return Some(m.as_str().to_string());
        }
    }
    None
}

/// Build a refusal message designed to make the LLM ask the user for
/// real values instead of retrying. The text is intentionally
/// non-adversarial — the LLM is not being attacked, it just over-eagerly
/// substituted documentation examples.
pub fn refusal_message(evidence: &str) -> String {
    format!(
        "[PLACEHOLDER_GUARD] Refusing to execute: the command contains the placeholder pattern '{}', \
         which looks like a documentation example, not a real value. \
         DO NOT retry with a guess. DO NOT enter auto-correction mode. \
         Explain the workflow step to the user and ASK for the real value \
         (real file path, real tenant id, real username, etc.) before emitting any <EXECUTE> block.",
        evidence
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_spanish_route_placeholder() {
        let scripts = [
            r"Get-Content -Path 'C:\Ruta\Al\Correo\sospechoso.eml' -Raw",
            "Get-FileHash -Path \"C:\\Ruta\\Al\\Adjunto\\sospechoso.zip\"",
            r"copy C:\Ruta\Del\Origen\file.txt C:\dest\",
        ];
        for s in scripts {
            let hit = detect_placeholders(s);
            assert!(hit.is_some(), "missed placeholder in: {}", s);
        }
    }

    #[test]
    fn catches_english_path_placeholder() {
        assert!(detect_placeholders(r"copy C:\Path\To\Evidence file.txt").is_some());
        assert!(detect_placeholders(r"cat /path/to/some-log.txt").is_some());
    }

    #[test]
    fn catches_placeholder_usernames() {
        let scripts = [
            r"Connect-IPPSSession -UserPrincipalName tu-usuario@dominio.com",
            r"net user your-user@example.com",
            r"Connect-AzAccount admin@tudominio.com",
            r"Connect-ExchangeOnline admin@tudominio.com",
            r"foo bar usuario@empresa.com baz",
        ];
        for s in scripts {
            assert!(detect_placeholders(s).is_some(), "missed: {}", s);
        }
    }

    #[test]
    fn catches_angle_bracket_placeholders() {
        let scripts = [
            r"az account set --subscription <YOUR-SUBSCRIPTION>",
            r"curl -H 'X-API-Key: YOUR-API-KEY' http://example",
            r"Connect-AzAccount -TenantId <TENANT_ID>",
            r"$key = '[INSERT_API_KEY]'",
        ];
        for s in scripts {
            assert!(detect_placeholders(s).is_some(), "missed: {}", s);
        }
    }

    #[test]
    fn catches_skill_body_example_names() {
        let scripts = [
            "New-ComplianceSearch -Name \"Purga_Phishing_Incident_01\"",
            r"Get-FileHash -Path .\sospechoso.zip",
            r"Get-FileHash -Path 'suspicious.exe'",
            r"$caseId = 'case-2024-001'",
        ];
        for s in scripts {
            assert!(detect_placeholders(s).is_some(), "missed: {}", s);
        }
    }

    #[test]
    fn allows_real_commands() {
        let scripts = [
            "Get-Process | Select-Object -First 10",
            r"Get-ChildItem C:\Users\eleue\Documents",
            "ipconfig /all",
            "netstat -ano",
            "Get-EventLog -LogName Security -Newest 100",
            "ssh user@10.0.0.5 'uname -a'",
            "git clone https://github.com/foo/bar.git",
            r"dir C:\Windows\System32",
        ];
        for s in scripts {
            let hit = detect_placeholders(s);
            assert!(hit.is_none(), "false positive on: {} (matched: {:?})", s, hit);
        }
    }

    #[test]
    fn allows_empty_and_internal_test_scripts() {
        assert!(detect_placeholders("").is_none());
        assert!(detect_placeholders("echo cmd-sentinel-abc").is_none());
    }

    #[test]
    fn refusal_message_mentions_evidence() {
        let m = refusal_message(r"C:\Ruta\Al\");
        assert!(m.contains(r"C:\Ruta\Al\"));
        assert!(m.contains("PLACEHOLDER_GUARD"));
        assert!(m.contains("ASK for the real value"));
    }
}
