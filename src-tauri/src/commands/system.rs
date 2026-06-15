// ── SYSTEM — Métricas de salud del sistema local ────────────────────────────────

use sysinfo::{System, Components, Networks};
use serde_json::json;
use chrono::Local;
use std::sync::Mutex;
use once_cell::sync::Lazy;

/// Network throughput delta tracker — sysinfo's `Networks` reports CUMULATIVE
/// bytes sent/received since boot. The Dashboard wants instantaneous MB/s,
/// which means we have to remember the last reading and compute a delta.
///
/// Persisted globally because get_system_health_json may be called every 10s
/// from the Dashboard and we want the delta to span the inter-call window.
struct NetSnapshot {
    received_total: u64,
    transmitted_total: u64,
    at: std::time::Instant,
}
static LAST_NET: Lazy<Mutex<Option<NetSnapshot>>> = Lazy::new(|| Mutex::new(None));

/// Devuelve un resumen de salud del sistema como texto plano.
/// Usa spawn_blocking porque sys.refresh_all() es CPU-bound (~50-200ms)
/// y bloquearía el hilo async de Tauri si se llama directamente.
#[tauri::command]
pub async fn get_system_health() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        let mut sys = System::new_all();
        sys.refresh_all();
        let total_mem = sys.total_memory() / 1_048_576;
        let used_mem  = sys.used_memory()  / 1_048_576;
        let mem_pct   = if total_mem > 0 { (used_mem as f64 / total_mem as f64) * 100.0 } else { 0.0 };
        Ok(format!(
            "--- REPORTE DE SALUD DEL SISTEMA ---\n\
            Hostname: {}\nOS: {}\nTiempo activo: {} horas\n\n\
            [CPU]\nNúcleos lógicos: {}\nUso global: {:.2}%\n\n\
            [MEMORIA RAM]\nTotal: {} MB\nEn uso: {} MB ({:.2}%)\n",
            System::host_name().unwrap_or_else(|| "---".into()),
            System::long_os_version().unwrap_or_else(|| "---".into()),
            System::uptime() / 3600,
            sys.cpus().len(),
            sys.global_cpu_info().cpu_usage(),
            total_mem, used_mem, mem_pct
        ))
    })
    .await
    .map_err(|e| format!("Error interno sysinfo: {}", e))?
}

// ── Tavily API key management ─────────────────────────────────────────────
//
// Stored in Windows Credential Manager (or equivalent on macOS Keychain /
// Linux secret service) under target `LucySysAdmin` username `tavily_api_key`.
// The `search_web` command reads it; these commands let the UI set/check it
// from Settings without the user touching the keyring CLI.
//
// We deliberately NEVER return the key value from Rust — `get_tavily_api_key`
// returns ONLY a boolean (is_set). Surfacing the key into JS would expose
// it to the WebView devtools, eroding the keyring's security guarantee.

#[tauri::command]
pub async fn get_tavily_api_key_status() -> Result<bool, String> {
    tokio::task::spawn_blocking(|| {
        match keyring::Entry::new("LucySysAdmin", "tavily_api_key") {
            Ok(entry) => match entry.get_password() {
                Ok(s) => Ok(!s.trim().is_empty()),
                Err(_) => Ok(false),  // entry absent = not configured
            },
            Err(e) => Err(format!("Keyring access error: {}", e)),
        }
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn set_tavily_api_key(api_key: String) -> Result<(), String> {
    let trimmed = api_key.trim().to_string();
    // Tavily API keys start with "tvly-" and are 39-50 chars long. Light
    // validation: prefix check + reasonable length. Skipped if the user
    // explicitly passes empty string (treated as "delete the key").
    if !trimmed.is_empty() {
        if !trimmed.starts_with("tvly-") {
            return Err(
                "Tavily API key must start with 'tvly-' (visit tavily.com to get one)".to_string()
            );
        }
        if trimmed.len() < 30 || trimmed.len() > 80 {
            return Err(
                "Tavily API key length looks wrong (expected ~40 characters)".to_string()
            );
        }
    }
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new("LucySysAdmin", "tavily_api_key")
            .map_err(|e| format!("Keyring access error: {}", e))?;
        if trimmed.is_empty() {
            // Empty means "remove the entry"
            let _ = entry.delete_password();   // ignore "not found"
            Ok(())
        } else {
            entry.set_password(&trimmed)
                .map_err(|e| format!("Failed to write to keyring: {}", e))
        }
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Devuelve métricas del sistema local en JSON — usado por el Dashboard.
/// spawn_blocking porque sys.refresh_all() es bloqueante (50-200ms).
#[tauri::command]
pub async fn get_system_health_json() -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(|| {
        let mut sys = System::new_all();
        sys.refresh_all();
        // ── CPU sampling accuracy (v1.7.180) ─────────────────────────────
        // sysinfo derives CPU usage from the DELTA between two refreshes. The
        // refresh above lands microseconds after `new_all()`'s internal sample,
        // so per-core (and per-process) % is computed over a ~0 ms window —
        // noise that doesn't track Task Manager. Refresh CPU + processes again
        // after the documented minimum interval so the deltas reflect a real
        // ~250 ms window and line up with what Task Manager reports. (Runs in
        // spawn_blocking, so the extra sleep never touches the UI thread.)
        std::thread::sleep(
            sysinfo::MINIMUM_CPU_UPDATE_INTERVAL
                .saturating_add(std::time::Duration::from_millis(50)),
        );
        sys.refresh_cpu();
        sys.refresh_processes();

        let disks_data: Vec<serde_json::Value> = sysinfo::Disks::new_with_refreshed_list()
            .iter()
            .map(|d| {
                let total_gb = d.total_space()     / 1_073_741_824;
                let free_gb  = d.available_space() / 1_073_741_824;
                let used_gb  = total_gb.saturating_sub(free_gb);
                let pct = if total_gb > 0 { (used_gb as f64 / total_gb as f64) * 100.0 } else { 0.0 };
                json!({
                    "name":     d.name().to_string_lossy().to_string(),
                    "mount":    d.mount_point().display().to_string(),
                    "total_gb": total_gb,
                    "used_gb":  used_gb,
                    "free_gb":  free_gb,
                    "percent":  (pct * 10.0).round() / 10.0
                })
            })
            .collect();

        let mut procs: Vec<_> = sys.processes().values().collect();
        procs.sort_by(|a, b| b.memory().cmp(&a.memory()));
        let top_procs: Vec<serde_json::Value> = procs.iter().take(5).map(|p| json!({
            "name":   p.name().to_string(),
            "cpu":    (p.cpu_usage() as f64 * 10.0).round() / 10.0,
            "mem_mb": p.memory() / 1_048_576,
            "pid":    p.pid().as_u32(),
            // D-Proc: exe path so the Dashboard can "open file location".
            "path":   p.exe().map(|x| x.display().to_string()),
        })).collect();

        // v1.7.180 — was `.take(12)`, which truncated to 12 logical cores and
        // hid the rest on higher-thread CPUs (e.g. a 16-thread i9 showed only
        // C0–C11). Cap raised to 64 purely as a UI-flood guard for big servers.
        let per_core: Vec<f64> = sys.cpus().iter().take(64)
            .map(|c| (c.cpu_usage() as f64 * 10.0).round() / 10.0)
            .collect();

        let total_mem = sys.total_memory() / 1_048_576;
        let used_mem  = sys.used_memory()  / 1_048_576;
        let mem_pct   = if total_mem > 0 { (used_mem as f64 / total_mem as f64) * 100.0 } else { 0.0 };

        // ── D2 — Page file / swap ────────────────────────────────────────
        // Windows: this is the paging file. Linux: this is swap. macOS:
        // compressed memory. Same semantic for the user: "memory pressure
        // safety net". If total_swap == 0, the host has no paging
        // configured — emit `enabled: false` so the UI hides the card.
        let total_swap = sys.total_swap() / 1_048_576;  // MB
        let used_swap  = sys.used_swap()  / 1_048_576;
        let swap_pct   = if total_swap > 0 { (used_swap as f64 / total_swap as f64) * 100.0 } else { 0.0 };
        let swap_block = json!({
            "enabled":  total_swap > 0,
            "total_mb": total_swap,
            "used_mb":  used_swap,
            "percent":  (swap_pct * 10.0).round() / 10.0,
        });

        // ── D4 — Temperatures (CPU/GPU/disk sensors) ─────────────────────
        // sysinfo 0.30 exposes a separate Components API; some platforms
        // and some firmwares don't expose any sensors (Linux without
        // lm-sensors loaded, locked-down Windows with disabled WMI Temp).
        // We emit `available: false` so the UI hides the card gracefully.
        let mut comps = Components::new_with_refreshed_list();
        comps.refresh();
        let temps: Vec<serde_json::Value> = comps.iter()
            .filter(|c| c.temperature().is_finite() && c.temperature() > 0.0)
            .take(8)
            .map(|c| {
                let temp = c.temperature();
                let critical = c.critical().filter(|t| t.is_finite());
                let max = c.max();
                json!({
                    "label":      c.label().to_string(),
                    "celsius":    (temp * 10.0).round() / 10.0,
                    "max":        if max.is_finite() && max > 0.0 { Some((max * 10.0).round() / 10.0) } else { None },
                    "critical":   critical.map(|t| (t * 10.0).round() / 10.0),
                })
            })
            .collect();
        let temps_block = json!({
            "available": !temps.is_empty(),
            "sensors":   temps,
        });

        // ── D1 — Network throughput (delta over last call) ───────────────
        // Networks::new_with_refreshed_list collects current totals;
        // compare against LAST_NET to compute per-second rates. First
        // call after startup returns rate=0 — that's the right behaviour
        // (we have no prior reference point).
        let networks = Networks::new_with_refreshed_list();
        let mut cur_recv: u64 = 0;
        let mut cur_send: u64 = 0;
        let mut per_iface: Vec<serde_json::Value> = Vec::new();
        for (iface, data) in networks.iter() {
            // Skip loopback / virtual interfaces that always report 0 traffic
            // (they dominate the list and crowd the UI). Real NICs almost
            // always have non-zero total_received over the host's uptime.
            let total_recv = data.total_received();
            let total_send = data.total_transmitted();
            cur_recv += total_recv;
            cur_send += total_send;
            // Per-iface ONLY include those with traffic — keeps the UI
            // small. The user wants to see "wifi + ethernet" not 18 entries.
            if total_recv > 0 || total_send > 0 {
                per_iface.push(json!({
                    "name":       iface,
                    "total_recv": total_recv,
                    "total_send": total_send,
                }));
            }
        }
        // Compute delta rates from previous snapshot
        let now = std::time::Instant::now();
        let mut last = LAST_NET.lock().ok();
        let (recv_mbps, send_mbps) = if let Some(g) = last.as_mut() {
            if let Some(snap) = g.as_ref() {
                let elapsed = now.duration_since(snap.at).as_secs_f64();
                if elapsed > 0.05 {
                    let dr = cur_recv.saturating_sub(snap.received_total) as f64;
                    let ds = cur_send.saturating_sub(snap.transmitted_total) as f64;
                    // v1.7.2 — previously `.round() / 1.0` truncated
                    // anything below 0.5 Mbps to 0 and showed bands like
                    // "↓ 0.0 ↑ 1.0" all day for users on idle browsing
                    // (typical ~0.1-0.4 Mbps download, ~0.5-1.4 upload).
                    // Now we keep 2 decimals so the chip actually moves:
                    // 0.34 Mbps reads as 0.3, not 0.0. The frontend
                    // already does `.toFixed(1)` for display.
                    //
                    // Bytes/sec → Mbits/sec  (×8 / 1e6), then 2 decimals.
                    let to_mbps = |bytes: f64| -> f64 {
                        let raw = bytes * 8.0 / 1_000_000.0 / elapsed;
                        (raw * 100.0).round() / 100.0
                    };
                    (to_mbps(dr), to_mbps(ds))
                } else { (0.0, 0.0) }
            } else { (0.0, 0.0) }
        } else { (0.0, 0.0) };
        // Update the snapshot for the next call
        if let Some(g) = last.as_mut() {
            **g = Some(NetSnapshot {
                received_total: cur_recv,
                transmitted_total: cur_send,
                at: now,
            });
        }
        // Sort per_iface by total bytes desc, cap 6 — keeps the UI tidy.
        per_iface.sort_by(|a, b| {
            let aw = a["total_recv"].as_u64().unwrap_or(0) + a["total_send"].as_u64().unwrap_or(0);
            let bw = b["total_recv"].as_u64().unwrap_or(0) + b["total_send"].as_u64().unwrap_or(0);
            bw.cmp(&aw)
        });
        per_iface.truncate(6);
        let net_block = json!({
            "recv_mbps":  recv_mbps,
            "send_mbps":  send_mbps,
            "interfaces": per_iface,
        });

        Ok(json!({
            "hostname":      System::host_name().unwrap_or_else(|| "---".into()),
            "os":            System::long_os_version().unwrap_or_else(|| "---".into()),
            "uptime_h":      System::uptime() / 3600,
            "timestamp":     Local::now().format("%H:%M:%S").to_string(),
            "cpu": {
                "cores":    sys.cpus().len(),
                "global":   (sys.global_cpu_info().cpu_usage() as f64 * 10.0).round() / 10.0,
                "per_core": per_core
            },
            "memory": {
                "total_mb": total_mem,
                "used_mb":  used_mem,
                "percent":  (mem_pct * 10.0).round() / 10.0
            },
            "swap":          swap_block,
            "temperatures":  temps_block,
            "network":       net_block,
            "disks":         disks_data,
            "top_processes": top_procs
        }))
    }).await.map_err(|e| format!("Error interno sysinfo: {}", e))?
}

/// Terminate a process by PID — local-machine SysAdmin action invoked from the
/// Dashboard's process table (right-click → end task). Refuses the low system
/// PIDs (0 = System Idle, 4 = System) so a misclick can't try to kill the
/// kernel. Returns a clear error if the OS denies it (needs admin) or the
/// process already exited.
#[tauri::command]
pub async fn kill_process(pid: u32) -> Result<(), String> {
    if pid <= 4 {
        return Err("PID protegido del sistema — no se puede terminar.".into());
    }
    tokio::task::spawn_blocking(move || {
        let mut sys = System::new();
        sys.refresh_processes();
        match sys.process(sysinfo::Pid::from_u32(pid)) {
            Some(p) => {
                if p.kill() { Ok(()) }
                else { Err("No se pudo terminar el proceso (¿requiere ejecutar Lucy como administrador?).".to_string()) }
            }
            None => Err("Proceso no encontrado (quizá ya terminó).".to_string()),
        }
    })
    .await
    .map_err(|e| format!("Error interno: {}", e))?
}
