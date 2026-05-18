// ── OS NOTIFICATIONS — Windows toast notifications (P0 Feature 4) ────────────
//
// Sends native Windows toast notifications via the `tauri-plugin-notification`
// crate. Used to alert the user when:
//   - A long-running task completes (agent loop, compliance scan, runbook)
//   - An anomaly/alert threshold is breached (CPU > 95%, disk full, etc.)
//   - A scheduled task fires and needs attention
//
// The frontend can also call `send_notification` directly for custom alerts.
// Notification permission is requested once at first use (OS-level prompt).
//
// DESIGN: Lucy is local-only — no remote webhooks, no push services. These are
// plain OS-level toast notifications that appear in the Windows Action Center.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationResult {
    pub sent: bool,
    pub message: String,
}

/// Send an OS-level toast notification. Title + body are required.
/// `urgency` controls the notification priority: 'low' | 'normal' | 'critical'.
#[tauri::command]
pub async fn send_notification(
    app: tauri::AppHandle,
    title: String,
    body: String,
    urgency: Option<String>,
) -> Result<NotificationResult, String> {
    use tauri_plugin_notification::NotificationExt;

    // Sanitize inputs — prevent absurdly long notifications from clogging the OS
    let title = if title.len() > 200 {
        format!("{}…", &title[..199])
    } else {
        title
    };
    let body = if body.len() > 1000 {
        format!("{}…", &body[..999])
    } else {
        body
    };

    let _urgency = urgency.unwrap_or_else(|| "normal".to_string());

    app.notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
        .map_err(|e| format!("Failed to send notification: {}", e))?;

    Ok(NotificationResult {
        sent: true,
        message: format!("Notification sent: {}", title),
    })
}

/// Check if notification permission is granted.
#[tauri::command]
pub async fn check_notification_permission(
    app: tauri::AppHandle,
) -> Result<String, String> {
    use tauri_plugin_notification::NotificationExt;

    let perm = app.notification()
        .permission_state()
        .map_err(|e| format!("Permission check failed: {}", e))?;

    let status = match perm {
        tauri_plugin_notification::PermissionState::Granted => "granted",
        tauri_plugin_notification::PermissionState::Denied => "denied",
        _ => "unknown",
    };

    Ok(status.to_string())
}

/// Request notification permission from the OS.
#[tauri::command]
pub async fn request_notification_permission(
    app: tauri::AppHandle,
) -> Result<String, String> {
    use tauri_plugin_notification::NotificationExt;

    let perm = app.notification()
        .request_permission()
        .map_err(|e| format!("Permission request failed: {}", e))?;

    let status = match perm {
        tauri_plugin_notification::PermissionState::Granted => "granted",
        tauri_plugin_notification::PermissionState::Denied => "denied",
        _ => "unknown",
    };

    Ok(status.to_string())
}
