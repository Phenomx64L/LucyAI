// ── SysAdmin domain synonyms (Tier 1 #3, agentmemory-inspired) ──────────
//
// FTS5 tokenizes by whitespace + punctuation but has no notion that "ps"
// and "process" mean the same thing, or that "gpo" and "group policy"
// describe the same Windows feature. This module ships a curated set of
// equivalence groups specific to Lucy's SysAdmin domain and exposes
// `expand_query_terms` which rewrites user tokens into an FTS5-friendly
// OR group: e.g. `ps` → `("ps" OR "process" OR "processes")`.
//
// Coverage philosophy:
//   • Windows-first (Lucy's primary OS): PowerShell, registry, WMI/CIM,
//     services, scheduled tasks, GPO, AD, WinRM, RDP, event log, Defender
//   • Cross-platform shells + tools: bash, zsh, ssh, scp, sed, awk
//   • Common abbreviations + plurals/verbings of the same root
//
// Bidirectional: every term in a group resolves to the whole group. We
// store groups as a Vec<&'static [&'static str]> and build the lookup
// map lazily on first use — no allocations on cold paths.
//
// All terms are lowercased; the lookup also lowercases the input. Keep
// the group definitions short (≤ 5 members each) — wider groups dilute
// BM25 ranking by matching too broadly.

use once_cell::sync::Lazy;
use std::collections::HashMap;

// ── Domain groups ───────────────────────────────────────────────────────
// Curated, not exhaustive. Add a group when you notice a recurring miss
// in real Lucy sessions. Order within a group doesn't matter.
static GROUPS: &[&[&str]] = &[
    // ── Process management ──
    &["ps", "process", "processes", "proc"],
    &["kill", "terminate", "stop", "end-process"],
    &["pid", "process-id"],

    // ── Windows shells ──
    &["pwsh", "powershell", "ps1"],
    &["cmd", "command-prompt", "console"],
    &["wsl", "linux-subsystem"],

    // ── Linux shells ──
    &["bash", "sh", "shell"],

    // ── Registry ──
    &["reg", "registry", "regedit"],
    &["hkcu", "current-user"],
    &["hklm", "local-machine"],

    // ── WMI / CIM ──
    &["wmi", "cim", "wbem"],
    &["wmic"],

    // ── Services + scheduled tasks ──
    &["service", "services", "svc", "daemon"],
    &["schtasks", "scheduled-task", "task-scheduler", "cron"],

    // ── Group Policy + AD ──
    &["gpo", "group-policy", "group-policies"],
    &["ad", "active-directory"],
    &["ou", "organizational-unit"],
    &["dc", "domain-controller"],
    &["dn", "distinguished-name"],

    // ── Remoting ──
    &["winrm", "ps-remoting"],
    &["rdp", "remote-desktop", "mstsc"],
    &["ssh", "secure-shell"],
    &["scp", "secure-copy", "sftp"],

    // ── Logs + events ──
    &["log", "logs", "logging"],
    &["event", "events", "evtx", "event-log"],
    &["audit", "auditing", "audited"],

    // ── Networking ──
    &["dns", "name-resolution", "name-lookup", "resolver"],
    &["dhcp"],
    &["ip", "ipv4", "ipv6", "ip-address"],
    &["nic", "network-adapter", "interface"],
    &["fw", "firewall"],
    &["vpn"],
    &["proxy"],

    // ── Storage + filesystem ──
    &["fs", "filesystem", "file-system"],
    &["ntfs"],
    &["acl", "permissions", "permission", "perms"],
    &["share", "smb", "cifs", "fileshare"],

    // ── Security ──
    &["defender", "antivirus", "av"],
    &["cred", "credential", "credentials"],
    &["cert", "certificate", "certs", "x509"],
    &["sid", "security-identifier"],
    &["mfa", "2fa", "multi-factor"],

    // ── Updates + packages ──
    &["wu", "windows-update", "wsus"],
    &["winget"],
    &["choco", "chocolatey"],
    &["msi", "installer", "install-package"],

    // ── Common verbs ──
    &["install", "installation", "deploy", "deployment"],
    &["uninstall", "remove", "removal"],
    &["error", "failure", "failed", "exception"],
    &["fix", "resolve", "resolved", "fixed"],
    &["config", "configuration", "settings"],
    &["restart", "reboot", "reset"],
    &["backup", "snapshot"],
    &["restore", "recover", "recovery"],
];

// Flat lookup: term → all members of its group (including the term itself).
static LOOKUP: Lazy<HashMap<&'static str, &'static [&'static str]>> = Lazy::new(|| {
    let mut m = HashMap::with_capacity(GROUPS.iter().map(|g| g.len()).sum());
    for group in GROUPS {
        for term in *group {
            m.insert(*term, *group);
        }
    }
    m
});

/// Expand a single user token into an FTS5 OR group string. If the token
/// is unknown, returns just the token (no wrapping). Output is already
/// FTS5-quoted and ready to embed in a MATCH expression.
///
/// Examples:
///   `"ps"`       → `("ps"* OR "process"* OR "processes"* OR "proc"*)`
///   `"foobar"`   → `"foobar"*`            (unknown → passthrough)
///   `"DNS"`      → `("dns"* OR "name-resolution"* OR ...)`   (case-insensitive)
pub fn expand_token(token: &str) -> String {
    let lc = token.to_lowercase();
    // Strip any FTS5-hostile chars; keep alphanumerics, hyphen, underscore
    let clean: String = lc.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if clean.is_empty() {
        return String::new();
    }

    match LOOKUP.get(clean.as_str()) {
        Some(group) => {
            let parts: Vec<String> = group.iter()
                .map(|s| format!("\"{}\"*", s))
                .collect();
            format!("({})", parts.join(" OR "))
        }
        None => format!("\"{}\"*", clean),
    }
}

/// Expand a full query string into an FTS5 MATCH expression where each
/// token has been replaced with its synonym OR-group. Empty tokens are
/// dropped. Tokens are joined with `OR` (broad recall, RRF re-tightens
/// at the fusion step).
pub fn expand_query(query: &str) -> String {
    let parts: Vec<String> = query
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(expand_token)
        .filter(|s| !s.is_empty())
        .collect();
    parts.join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_token_passes_through() {
        let r = expand_token("foobarbaz");
        assert_eq!(r, "\"foobarbaz\"*");
    }

    #[test]
    fn known_token_expands_to_group() {
        let r = expand_token("ps");
        assert!(r.starts_with('(') && r.ends_with(')'));
        assert!(r.contains("\"ps\"*"));
        assert!(r.contains("\"process\"*"));
    }

    #[test]
    fn case_insensitive() {
        let upper = expand_token("DNS");
        let lower = expand_token("dns");
        assert_eq!(upper, lower);
    }

    #[test]
    fn full_query_joins_groups() {
        let r = expand_query("ps memory leak");
        assert!(r.contains("\"ps\"*"));
        assert!(r.contains("\"memory\"*") || r.contains("\"memory-leak\"*"));
        assert!(r.contains(" OR "));
    }

    #[test]
    fn empty_query_is_empty() {
        assert_eq!(expand_query(""), "");
        assert_eq!(expand_query("   "), "");
    }
}
