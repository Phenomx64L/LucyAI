// ── notify.ts — OS-level toast notification wrapper (P0 Feature 4) ───────────
//
// Sends native Windows toast notifications via tauri-plugin-notification.
// Used by anomaly-bridge, scheduled tasks, long-running agent loops, etc.
//
// Usage:
//   import { sendNotification, notifyTaskComplete, notifyAlert } from '$lib/notify';
//   await sendNotification('Title', 'Body text');
//   await notifyTaskComplete('Compliance scan finished', '3 warnings found');
//   await notifyAlert('cpu', 95.2, 'DESKTOP-01');

import { invoke } from '@tauri-apps/api/core';

export type NotificationUrgency = 'low' | 'normal' | 'critical';

export interface NotifyOptions {
    urgency?: NotificationUrgency;
}

/**
 * Send an OS-level toast notification (Windows Action Center).
 * Safe to call even if permissions are not granted — will silently fail.
 */
export async function sendNotification(
    title: string,
    body: string,
    options?: NotifyOptions
): Promise<boolean> {
    try {
        const result = await invoke<{ sent: boolean; message: string }>('send_notification', {
            title,
            body,
            urgency: options?.urgency || 'normal',
        });
        return result.sent;
    } catch (e) {
        console.warn('[notify] Failed to send notification:', e);
        return false;
    }
}

/**
 * Check if notification permission is granted.
 */
export async function checkNotificationPermission(): Promise<'granted' | 'denied' | 'unknown'> {
    try {
        const status = await invoke<string>('check_notification_permission');
        return status as 'granted' | 'denied' | 'unknown';
    } catch {
        return 'unknown';
    }
}

/**
 * Request notification permission from the OS.
 */
export async function requestNotificationPermission(): Promise<'granted' | 'denied' | 'unknown'> {
    try {
        const status = await invoke<string>('request_notification_permission');
        return status as 'granted' | 'denied' | 'unknown';
    } catch {
        return 'unknown';
    }
}

// ── Convenience helpers ─────────────────────────────────────────────────────

/** Notify when a long-running task completes. */
export async function notifyTaskComplete(taskName: string, summary?: string): Promise<void> {
    const body = summary ? `${taskName}\n${summary}` : taskName;
    await sendNotification('Lucy — Task Complete', body, { urgency: 'normal' });
}

/** Notify when a metric breaches an alert threshold. */
export async function notifyAlert(
    metric: 'cpu' | 'ram' | 'disk',
    value: number,
    hostName?: string
): Promise<void> {
    const host = hostName || 'local';
    const labels: Record<string, string> = {
        cpu: 'CPU Usage',
        ram: 'RAM Usage',
        disk: 'Disk Usage',
    };
    await sendNotification(
        `Lucy — ${labels[metric]} Alert`,
        `${host}: ${labels[metric]} at ${value.toFixed(1)}%`,
        { urgency: 'critical' }
    );
}

/** Notify when an agent loop needs attention (stuck, error, or user input required). */
export async function notifyAgentAttention(reason: string): Promise<void> {
    await sendNotification('Lucy — Attention Required', reason, { urgency: 'critical' });
}

/** Notify when a scheduled task fires. */
export async function notifyScheduledTask(taskName: string, status: 'ok' | 'error'): Promise<void> {
    const emoji = status === 'ok' ? 'Completed' : 'Failed';
    await sendNotification(
        `Lucy — Scheduled Task ${emoji}`,
        taskName,
        { urgency: status === 'error' ? 'critical' : 'low' }
    );
}
