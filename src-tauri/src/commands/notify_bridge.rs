// ── notify_bridge.rs — Lucy reaches you off the machine ─────────────────────
//
// Until now Lucy could only speak inside her own window and to the Windows
// notification tray. For a SysAdmin assistant that is the wrong shape: the
// moments that matter — a critical CVE, a service that will not come back, an
// agent that needs a decision — happen while nobody is looking at the screen.
//
// This is OUTBOUND ONLY, and deliberately so. Inbound ("reply from your phone
// to drive Lucy") needs either a public endpoint or a long-poll loop, and it
// would put a remote channel on the path that executes commands. That is a
// much larger security conversation than a notification, so it is not bundled
// with one.
//
// THE THREE INVARIANTS
// --------------------
// 1. SCRUBBED. Everything is passed through `secret_scrubber` before it leaves
//    the machine. Lucy's messages routinely carry command output, and command
//    output routinely carries tokens, connection strings and passwords. A
//    notification bridge without this is an exfiltration channel with a
//    friendly name — this is the single most important line in the file.
// 2. SSRF-GUARDED. Delivery goes through `FETCH_CLIENT`, whose resolver refuses
//    loopback, RFC1918 and cloud-metadata addresses. The URL is operator-
//    supplied rather than model-chosen, but "the operator typed it" is not a
//    reason to let a webhook target 169.254.169.254.
// 3. OPT-IN. No channel configured means no egress. There is no default
//    endpoint and nothing is sent until someone deliberately sets one up.
//
// The bot token lives in the OS keyring next to the API keys, never in the DB
// and never in localStorage, and never crosses IPC back to the frontend — the
// status command answers "configured or not", the same contract as providers.

use serde::{Deserialize, Serialize};

/// Keyring entry holding the channel config JSON (it contains a bot token, so
/// it is a secret in its own right).
const KEYRING_SERVICE: &str = "LucySysAdmin";
const KEYRING_KEY: &str = "notify_bridge_config";

/// Hard cap on what leaves the machine. Telegram rejects >4096 chars anyway,
/// and a giant body is a sign something went wrong upstream — truncate rather
/// than blast a whole command dump to a phone.
const MAX_BODY: usize = 1500;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelKind {
    /// Telegram Bot API. `target` is the chat id.
    Telegram,
    /// Slack incoming webhook. `url` is the full hook, `target` unused.
    Slack,
    /// Any endpoint accepting `{"title":…,"body":…,"severity":…}`.
    Webhook,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    pub kind: ChannelKind,
    /// Telegram: the bot token. Slack/Webhook: the full URL.
    pub secret: String,
    /// Telegram: chat id. Others: unused.
    #[serde(default)]
    pub target: String,
    /// Minimum severity that is worth someone's phone. 'warning' by default —
    /// an assistant that forwards every 'info' teaches you to mute it.
    #[serde(default = "default_min_severity")]
    pub min_severity: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_min_severity() -> String { "warning".into() }
fn default_true() -> bool { true }

/// What the frontend is allowed to know. Never the token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeStatus {
    pub configured: bool,
    pub enabled: bool,
    pub kind: Option<String>,
    pub min_severity: String,
}

fn severity_rank(s: &str) -> u8 {
    match s.to_ascii_lowercase().as_str() {
        "critical" => 3,
        "warning" | "warn" => 2,
        _ => 1, // info and anything unrecognised
    }
}

fn load_config() -> Option<BridgeConfig> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_KEY).ok()?;
    let raw = entry.get_password().ok()?;
    serde_json::from_str::<BridgeConfig>(&raw).ok()
}

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn notify_bridge_status() -> BridgeStatus {
    match load_config() {
        Some(c) => BridgeStatus {
            configured: true,
            enabled: c.enabled,
            kind: Some(match c.kind {
                ChannelKind::Telegram => "telegram",
                ChannelKind::Slack => "slack",
                ChannelKind::Webhook => "webhook",
            }.into()),
            min_severity: c.min_severity,
        },
        None => BridgeStatus { configured: false, enabled: false, kind: None, min_severity: default_min_severity() },
    }
}

#[tauri::command]
pub fn notify_bridge_save(config: BridgeConfig) -> Result<(), String> {
    if config.secret.trim().is_empty() {
        return Err("Falta el token o la URL del canal.".into());
    }
    if matches!(config.kind, ChannelKind::Telegram) && config.target.trim().is_empty() {
        return Err("Telegram requiere un chat id.".into());
    }
    let json = serde_json::to_string(&config).map_err(|e| e.to_string())?;
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_KEY)
        .map_err(|e| e.to_string())?
        .set_password(&json)
        .map_err(|e| format!("No se pudo guardar en el almacén de credenciales: {}", e))
}

#[tauri::command]
pub fn notify_bridge_clear() -> Result<(), String> {
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_KEY) {
        Ok(e) => { let _ = e.delete_password(); Ok(()) }
        Err(e) => Err(e.to_string()),
    }
}

/// Sends a message, honouring the severity floor. Returns `false` when nothing
/// was sent because no channel is configured or the severity was below it —
/// that is a normal outcome, not an error.
#[tauri::command]
pub async fn notify_bridge_send(title: String, body: String, severity: String) -> Result<bool, String> {
    deliver(&title, &body, &severity).await
}

/// Ignores the severity floor so a test always sends. Everything else — the
/// scrubbing, the SSRF-guarded client — is the identical path, so a passing
/// test means the real thing works.
#[tauri::command]
pub async fn notify_bridge_test() -> Result<bool, String> {
    let Some(cfg) = load_config() else { return Err("No hay canal configurado.".into()) };
    send_via(&cfg, "Lucy — prueba de canal", "Si lees esto, el puente funciona.").await?;
    Ok(true)
}

// ── Delivery ────────────────────────────────────────────────────────────────

/// Callable from Rust so background watches (CVE, proactive detector) can reach
/// the operator without a round-trip through the frontend — which matters
/// precisely because the frontend may not be on screen.
pub async fn deliver(title: &str, body: &str, severity: &str) -> Result<bool, String> {
    let Some(cfg) = load_config() else { return Ok(false) };
    if !cfg.enabled { return Ok(false); }
    if severity_rank(severity) < severity_rank(&cfg.min_severity) { return Ok(false); }
    send_via(&cfg, title, body).await?;
    Ok(true)
}

async fn send_via(cfg: &BridgeConfig, title: &str, body: &str) -> Result<(), String> {
    // INVARIANT 1 — scrub before anything else touches the payload, so no later
    // edit can accidentally build a request from the raw text.
    let safe_title = crate::utils::secret_scrubber::scrub_for_audit(title);
    let safe_body = {
        let s = crate::utils::secret_scrubber::scrub_for_audit(body);
        if s.chars().count() > MAX_BODY {
            let cut: String = s.chars().take(MAX_BODY).collect();
            format!("{}…\n[truncado]", cut)
        } else { s }
    };

    // INVARIANT 2 — FETCH_CLIENT, never the shared HTTP_CLIENT. The shared one
    // must reach loopback for local Ollama; this must not.
    let client = &*crate::state::FETCH_CLIENT;

    let res = match cfg.kind {
        ChannelKind::Telegram => {
            let url = format!("https://api.telegram.org/bot{}/sendMessage", cfg.secret.trim());
            client.post(url)
                .json(&serde_json::json!({
                    "chat_id": cfg.target.trim(),
                    "text": format!("*{}*\n{}", safe_title, safe_body),
                    "parse_mode": "Markdown",
                }))
                .send().await
        }
        ChannelKind::Slack => {
            client.post(cfg.secret.trim())
                .json(&serde_json::json!({ "text": format!("*{}*\n{}", safe_title, safe_body) }))
                .send().await
        }
        ChannelKind::Webhook => {
            client.post(cfg.secret.trim())
                .json(&serde_json::json!({ "title": safe_title, "body": safe_body }))
                .send().await
        }
    };

    let res = res.map_err(|e| format!("Error de red enviando la notificación: {}", e))?;
    let status = res.status();
    if !status.is_success() {
        // The body can echo the token back on a Telegram auth error — scrub the
        // error too. An error path is still an egress path.
        let detail = res.text().await.unwrap_or_default();
        let detail = crate::utils::secret_scrubber::scrub_for_audit(&detail);
        return Err(format!("El canal respondió HTTP {} — {}", status.as_u16(), detail.chars().take(200).collect::<String>()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_floor_orders_correctly() {
        assert!(severity_rank("critical") > severity_rank("warning"));
        assert!(severity_rank("warning") > severity_rank("info"));
        // Unknown severities must not sneak past a floor by ranking high.
        assert_eq!(severity_rank("cualquier-cosa"), severity_rank("info"));
        assert_eq!(severity_rank("WARN"), severity_rank("warning"));
    }

    #[test]
    fn default_floor_is_warning_not_info() {
        // A bridge that forwards every 'info' gets muted, and then the
        // 'critical' does not arrive either.
        assert_eq!(default_min_severity(), "warning");
    }

    #[test]
    fn status_never_exposes_the_secret() {
        // The status struct is the ONLY thing crossing IPC about the channel.
        // If a token field is ever added here it would leak to the frontend.
        let s = BridgeStatus { configured: true, enabled: true, kind: Some("telegram".into()), min_severity: "warning".into() };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("target"));
    }

    #[test]
    fn config_rejects_empty_secret_and_missing_chat_id() {
        let bad = BridgeConfig { kind: ChannelKind::Telegram, secret: "  ".into(), target: "1".into(), min_severity: "warning".into(), enabled: true };
        assert!(notify_bridge_save(bad).is_err());

        let no_chat = BridgeConfig { kind: ChannelKind::Telegram, secret: "tok".into(), target: "".into(), min_severity: "warning".into(), enabled: true };
        assert!(notify_bridge_save(no_chat).is_err());
    }

    #[test]
    fn scrubbing_happens_before_egress() {
        // Pins invariant 1 against the scrubber itself regressing: whatever
        // send_via would put on the wire must not carry the raw secret.
        let raw = "conectado con password=SuperSecreto123 al host";
        let scrubbed = crate::utils::secret_scrubber::scrub_for_audit(raw);
        assert!(!scrubbed.contains("SuperSecreto123"), "el scrubber dejó pasar la contraseña: {}", scrubbed);
    }
}
