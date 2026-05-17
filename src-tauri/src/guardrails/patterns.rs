// ── guardrails/patterns.rs — Compiled regex bank ──────────────────────────
//
// Each pattern documents:
//   • Audit ID (S1/S2/S5/S10) it mitigates
//   • Which Role(s) the pattern applies to
//   • Severity if matched (Block vs HumanInTheLoop)
//
// Patterns are intentionally narrow to minimize false positives. Lucy is a
// SysAdmin tool — it MUST be allowed to discuss `Remove-Item`, `format`, etc.
// in chat context. We only flag DANGEROUS COMBINATIONS or known bypass
// shapes, not the existence of the verb.

use once_cell::sync::Lazy;
use regex::Regex;

/// One scanner pattern.
pub struct Pattern {
    pub id:        &'static str,
    pub audit_ref: &'static str,
    pub re:        Regex,
    /// `true` → BLOCK on match (clear attack signature).
    /// `false` → HUMAN_IN_THE_LOOP (requires explicit operator confirm).
    pub block:     bool,
}

// ── S1 — PowerShell injection inside SecureString literal ──────────────────
// build_winrm_script does:
//     $pwd = ConvertTo-SecureString '<password>' -AsPlainText -Force
// where '<password>' is single-quote-escaped (`'` → `''`). A password
// containing `';Invoke-Expression(...)#` breaks out:
//     ConvertTo-SecureString ''; Invoke-Expression(...); #' -AsPlainText...
// → arbitrary PowerShell executes locally.
//
// We apply this scanner to Role::SecretMaterial (host passwords loaded from
// keyring, before they're interpolated into any script).
pub static S1_PS_INJECTION_IN_SECRET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?ix)
        (?:                                # any of the breakout shapes:
            ' \s* ;                        #   '; — closes literal, starts new cmd
          | ' \s* \$ \(                    #   '$( — closes + opens subexpression
          | ' \s* \&                       #   '& — closes + invocation
          | ' \s* `                        #   '` — closes + backtick (PS escape)
        )
        |
        (?:                                # bare patterns even without escape:
            \b Invoke-Expression \b
          | \b iex \b
          | \b Add-Type \b \s* -TypeDefinition
          | \b Start-Process \b .{0,40} -Verb \s+ RunAs
        )
        "
    ).expect("S1 regex compile")
});

// ── S2 — cmd /C blocklist bypass shapes ────────────────────────────────────
// Current blocklist in local.rs::execute_cmd checks for literal substrings
// like "format " and "del /s" — trivially evaded. We additionally flag:
//   • `for %i in (...) do <prog>` — common loop-to-exec trick
//   • `%COMSPEC% /c` — env var pointing at cmd.exe
//   • Fullwidth Unicode that normalizes to dangerous tokens
//   • Tab/CR characters embedded in command (used to break naive parsing)
//   • Pipe-chained format/del
pub static S2_CMD_BYPASS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?ix)
        (?:
            \b for \s+ /? \w* \s+ %\w+ \s+ in \s* \(  # for-in subshell exec
          | %COMSPEC% \s+ /[cCkK]                     # env-var cmd.exe redirect
          | \\system32\\cmd\.exe                       # absolute cmd path
          | \\system32\\format\.com                    # bypasses substring blocklist
          | [\u{ff01}-\u{ff5e}]                        # fullwidth ASCII homoglyph
          | [\t\r] .* (?: format | del | rmdir )       # tab/CR before destructive verb
          | & \s* (?: del | format | rmdir | rd ) \s+ /[sSqQ]
        )
        "#
    ).expect("S2 regex compile")
});

// ── S5 — SSRF via fetch_url / search_web targeting internal services ───────
// LLM can emit <TOOL>fetch_url:http://...</TOOL> targeting:
//   • Cloud metadata: 169.254.169.254 (AWS/Azure/GCP IMDS)
//   • Loopback: 127.0.0.1, localhost (Ollama, OpenClaw gateway)
//   • Private RFC1918: 10.*, 192.168.*, 172.16-31.*
//   • Link-local: 169.254.*
//
// We scan LLM output for these URLs in tool tags, AND we scan the raw URL
// argument inside fetch_url_content as a backstop.
pub static S5_SSRF_INTERNAL_TARGETS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?ix)
        (?: https? :// | // )
        (?:
            169\.254\.\d{1,3}\.\d{1,3}                 # link-local + IMDS
          | 127\. \d{1,3} \. \d{1,3} \. \d{1,3}        # 127.0.0.0/8 loopback
          | localhost
          | 0\.0\.0\.0
          | \[ :: 1? \]                                 # ipv6 loopback
          | 10 \. \d{1,3} \. \d{1,3} \. \d{1,3}        # 10/8
          | 192 \. 168 \. \d{1,3} \. \d{1,3}           # 192.168/16
          | 172 \. (?: 1[6-9] | 2\d | 3[01] ) \. \d{1,3} \. \d{1,3}   # 172.16/12
          | metadata \. (?: google | aws | azure ) \. internal
        )
        "
    ).expect("S5 regex compile")
});

// ── S10 — UAC elevation patterns in file content / LLM output ──────────────
// Lucy reads files via <TOOL>readfile:...</TOOL>. A malicious file can
// contain "to fix this, please run: Start-Process powershell -Verb RunAs ..."
// → if Lucy echoes that and the user clicks, attacker gets admin.
//
// We BLOCK if a Role::Tool message (file content / command output) contains
// an explicit elevation request. We HITL if Role::Assistant emits one
// (Lucy's own decision — operator may legitimately need elevation).
pub static S10_UAC_ELEVATION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?ix)
        (?:
            Start-Process \b .{0,80} -Verb \s+ RunAs
          | \. ShellExecute \s* \( .* runas
          | New-Object \s+ -ComObject \s+ Shell\.Application
          | sudo \s+ (?: rm | dd | mkfs | shred )      # *nix counterpart
          | runas \s+ /user: (?: administrator | root | system )
        )
        "
    ).expect("S10 regex compile")
});

// ── Prompt injection — classic jailbreak shapes ────────────────────────────
// Applied to Role::User and Role::Tool (file content, command output). These
// are intentionally narrow to avoid false positives on legitimate SysAdmin
// chatter.
pub static PROMPT_INJECTION_CLASSIC: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?ix)
        (?:
            ignore \s+ (?: all \s+ )? (?: previous | prior | above ) \s+ instructions
          | disregard \s+ (?: all \s+ )? previous
          | forget \s+ (?: your \s+ )? (?: system \s+ prompt | instructions )
          | new \s+ instructions: \s* \n
          | <\| \s* (?: system | im_start | endoftext ) \s* \|?>   # chat-template injection
          | \[ \s* INST \s* \]                                       # llama template
          | system \s* \( \s* ["']
        )
        "#
    ).expect("Prompt injection regex compile")
});

// ── Hidden Unicode tags (U+E0000..U+E007F) ─────────────────────────────────
// These are invisible to humans but tokenized as text by LLMs. Used to
// smuggle instructions inside what looks like an innocent paste.
// This is a fast char-range check, not a regex.
#[inline]
pub fn has_hidden_unicode_tags(text: &str) -> bool {
    text.chars().any(|c| {
        let cp = c as u32;
        (0xE0000..=0xE007F).contains(&cp)
    })
}

/// All patterns that fire on Role::User input.
pub static USER_INPUT_PATTERNS: Lazy<Vec<Pattern>> = Lazy::new(|| vec![
    Pattern { id: "prompt_injection_classic", audit_ref: "PI",  re: PROMPT_INJECTION_CLASSIC.clone(), block: true },
]);

/// All patterns that fire on Role::Tool output (file content, cmd stdout).
pub static TOOL_OUTPUT_PATTERNS: Lazy<Vec<Pattern>> = Lazy::new(|| vec![
    Pattern { id: "prompt_injection_classic", audit_ref: "PI",  re: PROMPT_INJECTION_CLASSIC.clone(), block: true },
    Pattern { id: "uac_elevation_in_tool",    audit_ref: "S10", re: S10_UAC_ELEVATION.clone(),        block: true },
]);

/// All patterns that fire on Role::Assistant output (LLM-generated text).
pub static ASSISTANT_OUTPUT_PATTERNS: Lazy<Vec<Pattern>> = Lazy::new(|| vec![
    Pattern { id: "ssrf_internal_target",   audit_ref: "S5",  re: S5_SSRF_INTERNAL_TARGETS.clone(), block: true  },
    Pattern { id: "uac_elevation_proposed", audit_ref: "S10", re: S10_UAC_ELEVATION.clone(),        block: false /* HITL */ },
    Pattern { id: "cmd_bypass_shape",       audit_ref: "S2",  re: S2_CMD_BYPASS.clone(),            block: true  },
]);

/// All patterns that fire on Role::SecretMaterial (host password / API key).
pub static SECRET_PATTERNS: Lazy<Vec<Pattern>> = Lazy::new(|| vec![
    Pattern { id: "ps_injection_in_secret", audit_ref: "S1", re: S1_PS_INJECTION_IN_SECRET.clone(), block: true },
]);
