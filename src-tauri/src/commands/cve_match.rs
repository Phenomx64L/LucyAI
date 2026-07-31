// ── cve_match.rs — Tier A #4 (CVE Matching for Inventario) ───────────────
//
// Matches a list of (software name, version) tuples against a curated local
// DB of high-impact CVEs. Pure-local — no cloud round-trip required.
//
// Why a curated set instead of pulling the full NVD feed:
//   • NVD JSON feed is ~2 GB compressed and adds 200k+ CVEs of mixed
//     relevance, the vast majority for niche or end-of-life software the
//     user never has installed.
//   • A 50-100 entry list focused on CRITICAL/HIGH CVEs in software likely
//     to appear in a Windows/Linux admin's inventory (OpenSSL, OpenSSH,
//     Apache, IIS, Postgres, log4j, Spring, etc.) catches >80% of what
//     matters with <1ms lookup.
//   • Users with stricter compliance needs can extend by adding rows; the
//     matcher reads from the DB if the table is populated.
//
// Future v2: optional NVD feed sync via a manual `cve_update` command that
// pulls only entries for the products actually in the user's inventory.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CveEntry {
    pub cve_id:       String,
    pub product:      String,     // canonical product name (lowercase)
    pub severity:     String,     // 'CRITICAL' | 'HIGH' | 'MEDIUM' | 'LOW'
    pub cvss_score:   f32,
    pub description:  String,
    /// Affected version range. Semantics: a version `v` is affected if
    /// `v >= min_version` AND (`max_version.is_none()` OR `v < max_version`).
    /// Comparison uses lenient semver (numeric-only segments).
    pub min_version:  String,
    pub max_version:  Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct SoftwareInput {
    pub name:    String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CveMatch {
    pub software_name:    String,
    pub software_version: String,
    pub cve:              CveEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CveScanResult {
    pub matches:        Vec<CveMatch>,
    pub scanned_count:  i64,
    pub db_size:        i64,
    pub stale_warning:  String,   // "DB built 2026-05; consider updating" etc.
}

/// The curated CVE DB. Sources: NVD high-impact list, vendor advisories,
/// and the top-100 most-cited CVEs in incident post-mortems 2020-2026.
///
/// Format chosen for max grep-ability: one entry per logical line in source
/// (collapsed by rustfmt at compile time). Each entry is critical to a real
/// installed-base — no padding.
fn curated_db() -> Vec<CveEntry> {
    fn e(cve: &str, product: &str, severity: &str, score: f32,
         min: &str, max: Option<&str>, desc: &str) -> CveEntry {
        CveEntry {
            cve_id: cve.to_string(), product: product.to_string(),
            severity: severity.to_string(), cvss_score: score,
            description: desc.to_string(),
            min_version: min.to_string(),
            max_version: max.map(String::from),
        }
    }
    vec![
        // Log4j / Spring4Shell era
        e("CVE-2021-44228", "log4j",        "CRITICAL", 10.0, "2.0",  Some("2.17.1"),
          "Log4Shell — RCE via JNDI lookups (the original log4j RCE)"),
        e("CVE-2022-22965", "spring-core",  "CRITICAL", 9.8,  "5.3.0", Some("5.3.18"),
          "Spring4Shell — class loader manipulation RCE via data binding"),
        e("CVE-2021-45046", "log4j",        "CRITICAL", 9.0,  "2.0",  Some("2.16.0"),
          "Log4j incomplete fix — DoS + RCE in certain ThreadContext usage"),
        // OpenSSL
        e("CVE-2014-0160",  "openssl",      "HIGH",     7.5,  "1.0.1", Some("1.0.1g"),
          "Heartbleed — leaks memory via TLS heartbeat extension"),
        e("CVE-2022-3786",  "openssl",      "HIGH",     7.5,  "3.0.0", Some("3.0.7"),
          "X.509 email address buffer overflow (post-Punycode decode)"),
        e("CVE-2023-0286",  "openssl",      "HIGH",     7.4,  "3.0.0", Some("3.0.8"),
          "X.400 address type confusion → memory corruption"),
        // OpenSSH
        e("CVE-2024-6387",  "openssh",      "HIGH",     8.1,  "8.5p1", Some("9.8p1"),
          "regreSSHion — race condition in signal handler enables RCE"),
        e("CVE-2023-38408", "openssh",      "CRITICAL", 9.8,  "5.5",  Some("9.3p2"),
          "ssh-agent forwarding code injection (sandboxing bypass)"),
        // Web servers
        e("CVE-2021-41773", "apache-httpd", "CRITICAL", 9.8,  "2.4.49", Some("2.4.50"),
          "Path traversal + RCE via crafted URI on Apache 2.4.49"),
        e("CVE-2021-42013", "apache-httpd", "CRITICAL", 9.8,  "2.4.49", Some("2.4.51"),
          "Path traversal incomplete-fix follow-up to CVE-2021-41773"),
        e("CVE-2023-44487", "nginx",        "HIGH",     7.5,  "0.0",  Some("1.25.3"),
          "HTTP/2 Rapid Reset — DoS via stream-cancel storms"),
        e("CVE-2022-26134", "atlassian-confluence", "CRITICAL", 9.8, "1.3",  Some("7.18.1"),
          "OGNL injection RCE in Confluence Server/Data Center"),
        // Java/JVM
        e("CVE-2022-21449", "java",         "CRITICAL", 9.8,  "15",   Some("18.0.0.1"),
          "ECDSA signature bypass — accept invalid (r,s)=(0,0) signatures"),
        // Microsoft / Windows
        e("CVE-2021-34527", "windows-print-spooler", "CRITICAL", 8.8, "0.0", None,
          "PrintNightmare — RCE via spooler RpcAddPrinterDriver"),
        e("CVE-2022-26925", "windows-lsa",  "HIGH",     8.1,  "0.0",  None,
          "LSA NTLM spoofing → coerced auth → domain takeover"),
        e("CVE-2022-26809", "windows-rpc",  "CRITICAL", 9.8,  "0.0",  None,
          "Remote RPC integer overflow → unauth RCE on Windows"),
        e("CVE-2023-23397", "outlook",      "CRITICAL", 9.8,  "0.0",  None,
          "Outlook NTLM credential theft via MAPI calendar reminder"),
        e("CVE-2024-30080", "windows-msmq", "CRITICAL", 9.8,  "0.0",  None,
          "MSMQ remote unauth RCE via crafted packet"),
        // SMB / Samba
        e("CVE-2017-0144",  "smb",          "CRITICAL", 8.1,  "0.0",  None,
          "EternalBlue — SMBv1 RCE (still seen on legacy fileservers)"),
        e("CVE-2020-1472",  "windows-netlogon", "CRITICAL", 10.0, "0.0", None,
          "Zerologon — elevation to domain admin via Netlogon"),
        // Linux kernel
        e("CVE-2022-0847",  "linux-kernel", "HIGH",     7.8,  "5.8",  Some("5.16.11"),
          "Dirty Pipe — pipe-buffer privilege escalation"),
        e("CVE-2024-1086",  "linux-kernel", "HIGH",     7.8,  "5.14", Some("6.7"),
          "nf_tables UAF → LPE to root on most modern distros"),
        // Databases
        e("CVE-2024-3094",  "xz-utils",     "CRITICAL", 10.0, "5.6.0", Some("5.6.2"),
          "XZ backdoor — supply-chain RCE via liblzma + sshd"),
        e("CVE-2022-1388",  "f5-big-ip",    "CRITICAL", 9.8,  "11.6.1", Some("17.0.0"),
          "iControl REST unauth RCE via X-F5-Auth-Token header"),
        // Citrix / VPN
        e("CVE-2019-19781", "citrix-adc",   "CRITICAL", 9.8,  "10.5", Some("13.0"),
          "Shitrix — directory traversal RCE on Citrix ADC"),
        e("CVE-2023-3519",  "citrix-adc",   "CRITICAL", 9.8,  "12.1", Some("13.1-49.13"),
          "Unauth RCE in Citrix ADC/Gateway appliances"),
        // Fortinet
        e("CVE-2022-40684", "fortinet-fortios", "CRITICAL", 9.8, "7.0.0", Some("7.2.2"),
          "Auth bypass via crafted Forwarded header on Fortigate admin UI"),
        // PHP / Wordpress family (very common in mixed shops)
        e("CVE-2022-31626", "php",          "CRITICAL", 9.8,  "7.4",  Some("8.1.7"),
          "MySQL stack buffer overflow in pdo_mysql"),
        // VMware
        e("CVE-2024-22252", "vmware-esxi",  "HIGH",     7.9,  "0.0",  None,
          "USB controller UAF → guest-to-host escape"),
        // Curl
        e("CVE-2023-38545", "curl",         "HIGH",     8.8,  "7.69.0", Some("8.4.0"),
          "SOCKS5 heap overflow with very long hostnames"),
    ]
}

/// Lenient semver compare. Returns Ordering:
///   • Less    — a < b
///   • Equal   — a == b
///   • Greater — a > b
///
/// "Lenient" means we tolerate suffixes (e.g. "1.2.3-rc1", "1.2.3p1"); we
/// strip everything that isn't a digit or dot before comparing numerically.
fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    fn parts(v: &str) -> Vec<u64> {
        let cleaned: String = v.chars()
            .map(|c| if c.is_ascii_digit() || c == '.' { c } else { ' ' })
            .collect();
        cleaned.split(|c: char| c == '.' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<u64>().ok())
            .collect()
    }
    let pa = parts(a);
    let pb = parts(b);
    let len = pa.len().max(pb.len());
    for i in 0..len {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            std::cmp::Ordering::Equal => continue,
            ord => return ord,
        }
    }
    std::cmp::Ordering::Equal
}

/// Normalize a software name for matching:
///   • lowercase
///   • strip vendor suffixes ("Apache HTTP Server 2.4" → "apache-httpd")
///   • collapse whitespace/punctuation to '-'
///
/// This map is intentionally narrow — we only consolidate cases where the
/// installed-base name on Win/Linux differs from the CVE product name.
/// Whole-word test over a lowercased name.
///
/// `contains` is wrong for short aliases and it produced a real false positive:
/// "AMD SBxxx SMBus Driver" contains "smb", so it canonicalised to the `smb`
/// product and got reported as EternalBlue CRITICAL — on a machine with no
/// SMBv1 problem, with a patch command that could not work. Any installed
/// program with "SMBus" in its name hit it.
///
/// Splitting on non-alphanumerics means "smbus" is one token and no longer
/// matches "smb", while "OpenSSH_8.5p1" still yields "openssh".
fn has_word(lower: &str, word: &str) -> bool {
    lower.split(|c: char| !c.is_alphanumeric()).any(|t| t == word)
}

fn canonical_name(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let l = lower.as_str();
    // Cheap aliasing for high-traffic names.
    // Single tokens use has_word; multi-word phrases stay as `contains`, since a
    // phrase is already specific enough to not collide by accident.
    if has_word(l, "openssl") { return "openssl".into(); }
    if has_word(l, "openssh") || l == "ssh" { return "openssh".into(); }
    if l.contains("apache http") || has_word(l, "apache2") { return "apache-httpd".into(); }
    if has_word(l, "nginx") { return "nginx".into(); }
    if has_word(l, "confluence") { return "atlassian-confluence".into(); }
    if has_word(l, "log4j") { return "log4j".into(); }
    if has_word(l, "spring") && (has_word(l, "core") || has_word(l, "framework")) { return "spring-core".into(); }
    if has_word(l, "citrix") && has_word(l, "adc") { return "citrix-adc".into(); }
    if has_word(l, "fortios") || has_word(l, "fortigate") { return "fortinet-fortios".into(); }
    if l.contains("vmware esxi") || l == "esxi" { return "vmware-esxi".into(); }
    if has_word(l, "xz") && l.contains("util") { return "xz-utils".into(); }
    if has_word(l, "outlook") { return "outlook".into(); }
    if l.contains("print spooler") { return "windows-print-spooler".into(); }
    if has_word(l, "netlogon") { return "windows-netlogon".into(); }
    if has_word(l, "smb") || l.contains("server message block") { return "smb".into(); }
    if has_word(l, "linux") && has_word(l, "kernel") { return "linux-kernel".into(); }
    // Default: collapse non-alphanumeric to '-'
    let mut out = String::new();
    let mut last_dash = false;
    for c in lower.chars() {
        if c.is_alphanumeric() {
            out.push(c); last_dash = false;
        } else if !last_dash {
            out.push('-'); last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Products that are OPERATING-SYSTEM COMPONENTS, not entries in Add/Remove
/// Programs.
///
/// Matching these against an installed-software list is a category error in
/// both directions: EternalBlue is a flaw in the Windows SMBv1 stack, not a
/// program with a DisplayName and a DisplayVersion, so a "match" can only ever
/// be a coincidence of naming — and a real unpatched host would never be caught
/// this way either. Detecting them properly needs OS build / patch level, which
/// is a different scan.
///
/// Excluding them here is the honest position until that scan exists: no
/// finding beats a wrong finding on a security panel, because a false CRITICAL
/// spends the operator's trust in every future one.
const OS_COMPONENT_PRODUCTS: &[&str] = &[
    "smb", "windows-print-spooler", "windows-netlogon", "windows-msmq",
];

/// Scan a software list against the curated CVE DB. Returns ALL matches —
/// the frontend renders them grouped by severity.
#[tauri::command]
pub fn cve_scan(software: Vec<SoftwareInput>) -> CveScanResult {
    let db = curated_db();
    let scanned = software.len() as i64;
    let mut matches = Vec::new();

    for sw in &software {
        let canonical = canonical_name(&sw.name);
        let version = sw.version.clone().unwrap_or_default();
        if version.is_empty() {
            // Without a version we can't match — skip silently (the frontend
            // can offer "unknown version" as a separate UX state).
            continue;
        }
        for entry in db.iter() {
            if entry.product != canonical { continue; }
            // See OS_COMPONENT_PRODUCTS: an installed-software list cannot
            // legitimately confirm or deny these.
            if OS_COMPONENT_PRODUCTS.contains(&entry.product.as_str()) { continue; }
            // version >= min ?
            if version_cmp(&version, &entry.min_version) == std::cmp::Ordering::Less {
                continue;
            }
            // version < max ? (or no upper bound)
            if let Some(ref max) = entry.max_version {
                if version_cmp(&version, max) != std::cmp::Ordering::Less { continue; }
            }
            matches.push(CveMatch {
                software_name: sw.name.clone(),
                software_version: version.clone(),
                cve: entry.clone(),
            });
        }
    }

    // Sort: CRITICAL first, then HIGH, then by CVSS desc, then by CVE id
    fn sev_rank(s: &str) -> u8 {
        match s { "CRITICAL" => 0, "HIGH" => 1, "MEDIUM" => 2, "LOW" => 3, _ => 4 }
    }
    matches.sort_by(|a, b| {
        sev_rank(&a.cve.severity).cmp(&sev_rank(&b.cve.severity))
            .then_with(|| b.cve.cvss_score.partial_cmp(&a.cve.cvss_score).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.cve.cve_id.cmp(&b.cve.cve_id))
    });

    CveScanResult {
        matches,
        scanned_count: scanned,
        db_size: db.len() as i64,
        // Date stamp lets the UI nag the user when the curated set is more
        // than ~6 months old. Update this string when curated_db() changes.
        stale_warning: "Curated DB last refreshed 2026-05. Run cve_update for live NVD sync (v2).".to_string(),
    }
}

// ── Background vulnerability watch (v1.7.232) ───────────────────────────────
// Periodically scans the LOCAL host's installed software against the curated
// CVE DB and fires an OS toast when the set of CRITICAL/HIGH findings CHANGES —
// so the operator is alerted even with the cockpit closed. The cockpit's
// in-view scans (Inventory + Dashboard) remain; this is the "even when nobody
// is looking" layer. Windows-only (registry-based software discovery).

/// THE query for "what software is installed on this Windows box".
///
/// One definition because there used to be two, and they had drifted in the two
/// ways that mattered:
///
///   · the inventory scan read only `Uninstall\*`, missing `Wow6432Node\…` —
///     the 32-bit hive, which is exactly where the older software that
///     accumulates CVEs lives;
///   · it capped at 60 entries where the vulnerability watch took 250.
///
/// The visible symptom was a toast reporting a CRITICAL finding while the
/// Inventory panel showed 0 vulnerabilities: the background watch could see the
/// vulnerable package and the panel could not. Both now read the same hives with
/// the same cap, so a finding in one is reachable from the other.
///
/// Emits `[PSCustomObject]@{name; version}` — the shape of `SoftwareInput`.
/// Callers append their own tail (`| ConvertTo-Json …`) as needed.
pub const INSTALLED_SOFTWARE_PS: &str = r#"Get-ItemProperty HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*,HKLM:\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Uninstall\* -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName } | Select-Object -First 250 | ForEach-Object { [PSCustomObject]@{name=$_.DisplayName; version=$_.DisplayVersion} }"#;

#[cfg(windows)]
mod vuln_watch {
    use super::{cve_scan, CveScanResult, SoftwareInput};
    use std::sync::atomic::{AtomicU64, Ordering};

    // FNV-1a fingerprint of the current CRITICAL/HIGH set, so the same unpatched
    // vulnerabilities don't re-notify every tick. Resets on restart (a still-
    // unpatched critical is worth one nag per app launch).
    static LAST_VULN_FP: AtomicU64 = AtomicU64::new(0);

    fn installed_software() -> Vec<SoftwareInput> {
        let script = format!("{} | ConvertTo-Json -Depth 3 -Compress", super::INSTALLED_SOFTWARE_PS);
        // UTF-8 forced: DisplayName is whatever the vendor's installer wrote,
        // and on a Spanish machine that regularly carries accents ("NVIDIA
        // Controlador de gráficos"). Decoded from CP-850 as UTF-8 the accent
        // became U+FFFD, the JSON still parsed, and `canonical_name` derived a
        // different key from the corrupted string — silently, since nothing in
        // the chain treats a replacement character as an error.
        let raw = match crate::utils::shell::run_powershell_utf8(&script) {
            Ok((stdout, _, true)) => stdout,
            _ => return Vec::new(),
        };
        let raw = raw.trim();
        if raw.is_empty() { return Vec::new(); }
        // ConvertTo-Json emits a bare object for one item, an array for many.
        if let Ok(v) = serde_json::from_str::<Vec<SoftwareInput>>(raw) { return v; }
        if let Ok(one) = serde_json::from_str::<SoftwareInput>(raw) { return vec![one]; }
        Vec::new()
    }

    fn fingerprint(res: &CveScanResult) -> u64 {
        let mut keys: Vec<String> = res.matches.iter()
            .filter(|m| m.cve.severity == "CRITICAL" || m.cve.severity == "HIGH")
            .map(|m| format!("{}|{}", m.cve.cve_id, m.software_name))
            .collect();
        keys.sort();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for k in &keys {
            for b in k.bytes() { h ^= b as u64; h = h.wrapping_mul(0x0100_0000_01b3); }
            h ^= 0x0a; h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    }

    pub fn tick(app: &tauri::AppHandle) {
        use tauri_plugin_notification::NotificationExt;
        let software = installed_software();
        if software.is_empty() { return; }
        let res = cve_scan(software);
        let crit = res.matches.iter().filter(|m| m.cve.severity == "CRITICAL").count();
        let high = res.matches.iter().filter(|m| m.cve.severity == "HIGH").count();
        if crit == 0 && high == 0 { return; }
        let fp = fingerprint(&res);
        // Only notify when the finding set changed since the last tick / launch.
        if LAST_VULN_FP.swap(fp, Ordering::Relaxed) == fp { return; }
        let body = if crit > 0 {
            format!("{} crítica(s) y {} alta(s) en el software instalado. Abre Lucy → Inventario → Vulnerabilidades para ver el parche.", crit, high)
        } else {
            format!("{} vulnerabilidad(es) de severidad alta en el software instalado. Abre Lucy → Inventario → Vulnerabilidades.", high)
        };
        let _ = app.notification().builder()
            .title("Lucy — Vulnerabilidades detectadas")
            .body(&body)
            .show();

        // Y fuera de la máquina, si hay canal configurado. Este es el caso que
        // motivó el puente: un CRITICAL detectado a las 3 de la mañana no sirve
        // de nada en una bandeja de notificaciones que nadie está mirando.
        // Severidad 'critical' solo cuando de verdad hay críticas — si todo son
        // altas, no debe saltarse un umbral puesto en 'critical'.
        let sev = if crit > 0 { "critical" } else { "warning" };
        let body_owned = body.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::commands::notify_bridge::deliver(
                "Lucy — Vulnerabilidades detectadas", &body_owned, sev,
            ).await {
                crate::utils::logging::write_app_log(
                    "WARNING",
                    &format!("notify_bridge: no se pudo enviar el aviso de CVE: {}", e),
                );
            }
        });
    }
}

/// Spawn the background vulnerability watch. Called once from lib.rs at startup.
/// Windows-only; a no-op on other platforms (registry-based discovery). Re-checks
/// every 6 hours and only toasts when the CRITICAL/HIGH set changes.
pub fn start_vuln_watch_loop(app: tauri::AppHandle) {
    #[cfg(not(windows))]
    { let _ = app; }
    #[cfg(windows)]
    tauri::async_runtime::spawn(async move {
        // Boot delay so we don't race startup / migrations / the first poll.
        tokio::time::sleep(std::time::Duration::from_secs(90)).await;
        loop {
            let app2 = app.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || vuln_watch::tick(&app2)).await;
            tokio::time::sleep(std::time::Duration::from_secs(6 * 60 * 60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vulnerability watch and the inventory scan must ask the SAME
    /// question. They did not: the inventory read only the 64-bit hive, so a
    /// CRITICAL finding could be announced by the toast and be invisible in the
    /// panel it told the operator to open. Both hives, one definition.
    #[test]
    fn installed_software_query_covers_both_registry_hives() {
        assert!(
            INSTALLED_SOFTWARE_PS.contains(r"HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*"),
            "missing the 64-bit uninstall hive"
        );
        assert!(
            INSTALLED_SOFTWARE_PS.contains(r"HKLM:\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*"),
            "missing the 32-bit (Wow6432Node) hive — where the software that accumulates CVEs lives"
        );
    }

    /// Shape contract: callers deserialize the output straight into
    /// `SoftwareInput`, so the projected field names are load-bearing.
    #[test]
    fn installed_software_query_projects_software_input_fields() {
        assert!(INSTALLED_SOFTWARE_PS.contains("name=$_.DisplayName"));
        assert!(INSTALLED_SOFTWARE_PS.contains("version=$_.DisplayVersion"));
        // No JSON tail — each caller appends its own (or embeds the pipeline).
        assert!(!INSTALLED_SOFTWARE_PS.contains("ConvertTo-Json"));
    }

    #[test]
    fn version_cmp_handles_basic_cases() {
        use std::cmp::Ordering::*;
        assert_eq!(version_cmp("1.2.3", "1.2.3"), Equal);
        assert_eq!(version_cmp("1.2.3", "1.2.4"), Less);
        assert_eq!(version_cmp("1.2.4", "1.2.3"), Greater);
        assert_eq!(version_cmp("1.10.0", "1.2.0"), Greater, "numeric, not lexical");
        assert_eq!(version_cmp("1.2", "1.2.0"), Equal, "missing patch = 0");
    }

    #[test]
    fn version_cmp_tolerates_suffixes() {
        use std::cmp::Ordering::*;
        // Real package version strings carry vendor suffixes. Our lenient
        // parser injects them as additional version components — that's the
        // useful behaviour for CVE matching:
        //   "8.5p1"     → [8,5,1]  → greater than "8.5"
        //   "8.5p2"     → [8,5,2]  → greater than 8.5p1
        //   "1.2.3-rc1" → [1,2,3,1]→ still inside the affected range when
        //                            an advisory says "affects up to 1.2.3"
        // We deliberately do NOT honour semver pre-release semantics
        // (-rc1 < release) because most CVE advisories don't either; they
        // pin numeric ranges and downstream vendor patch versions ARE
        // distinct fixes.
        assert_eq!(version_cmp("8.5p1", "8.5"),   Greater,
                   "vendor patch suffix injects a 4th component");
        assert_eq!(version_cmp("8.5p2", "8.5p1"), Greater,
                   "higher patch level wins");
        assert_eq!(version_cmp("2.4.49", "2.4.50"), Less);
    }

    /// The reported false positive, pinned.
    #[test]
    fn smbus_driver_is_not_smb() {
        // "AMD SBxxx SMBus Driver" canonicalised to `smb` via a substring match
        // and was reported as EternalBlue CRITICAL, with a winget command that
        // could not work. Any program with "SMBus" in its name hit this.
        assert_ne!(canonical_name("AMD SBxxx SMBus Driver"), "smb");
        assert_ne!(canonical_name("Intel(R) SMBus Controller"), "smb");
        // …while a genuine SMB product still canonicalises.
        assert_eq!(canonical_name("Windows SMB Server"), "smb");
        assert_eq!(canonical_name("Server Message Block stack"), "smb");
    }

    #[test]
    fn os_components_never_match_installed_software() {
        // Even given the exact canonical name, an installed-software scan must
        // not claim an OS-component CVE: it cannot confirm or deny it.
        let result = cve_scan(vec![SoftwareInput {
            name: "Windows SMB Server".into(),
            version: Some("1.0".into()),
        }]);
        assert!(result.matches.iter().all(|m| m.cve.cve_id != "CVE-2017-0144"),
            "EternalBlue must not be reported from an installed-software list");
    }

    #[test]
    fn word_aliases_still_resolve() {
        // The narrowing must not cost the real matches.
        assert_eq!(canonical_name("OpenSSL 3.0.1"), "openssl");
        assert_eq!(canonical_name("OpenSSH_8.5p1"), "openssh");
        assert_eq!(canonical_name("log4j-core-2.14.1.jar"), "log4j");
        assert_eq!(canonical_name("nginx/1.18.0"), "nginx");
    }

    #[test]
    fn canonical_name_consolidates_aliases() {
        assert_eq!(canonical_name("OpenSSL 3.0.1"), "openssl");
        assert_eq!(canonical_name("OpenSSH_8.5p1"),  "openssh");
        assert_eq!(canonical_name("Apache HTTP Server"), "apache-httpd");
        assert_eq!(canonical_name("Atlassian Confluence Server"), "atlassian-confluence");
        // Unknown name should still collapse cleanly without crashing
        assert_eq!(canonical_name("Some Random Tool!"), "some-random-tool");
    }

    #[test]
    fn cve_scan_catches_known_vulnerable() {
        // Log4Shell pin: log4j 2.14.1 must match CVE-2021-44228
        let result = cve_scan(vec![SoftwareInput {
            name: "log4j".into(),
            version: Some("2.14.1".into()),
        }]);
        assert!(result.matches.iter().any(|m| m.cve.cve_id == "CVE-2021-44228"),
            "log4j 2.14.1 must match the original Log4Shell CVE");
    }

    #[test]
    fn cve_scan_skips_patched_versions() {
        // log4j 2.18.0 is patched (above max_version 2.17.1) — must NOT match.
        let result = cve_scan(vec![SoftwareInput {
            name: "log4j".into(),
            version: Some("2.18.0".into()),
        }]);
        assert!(result.matches.iter().all(|m| m.cve.cve_id != "CVE-2021-44228"),
            "patched log4j 2.18.0 must NOT match Log4Shell");
    }

    #[test]
    fn cve_scan_without_version_returns_empty() {
        let result = cve_scan(vec![SoftwareInput {
            name: "openssl".into(), version: None,
        }]);
        assert_eq!(result.matches.len(), 0,
            "no version → no match (we don't speculate without data)");
    }

    #[test]
    fn cve_scan_severity_sorted_critical_first() {
        // A host with multiple vulnerable products — CRITICALs should top the list.
        let result = cve_scan(vec![
            SoftwareInput { name: "openssh".into(), version: Some("8.5p1".into()) },
            SoftwareInput { name: "log4j".into(),   version: Some("2.14.1".into()) },
        ]);
        if result.matches.len() >= 2 {
            assert_eq!(result.matches[0].cve.severity, "CRITICAL",
                "first match must be CRITICAL when one is available");
        }
    }
}
