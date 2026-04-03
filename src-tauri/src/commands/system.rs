// ── SYSTEM — Métricas de salud del sistema local ────────────────────────────────

use sysinfo::System;
use serde_json::json;
use chrono::Local;

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

/// Devuelve métricas del sistema local en JSON — usado por el Dashboard.
/// spawn_blocking porque sys.refresh_all() es bloqueante (50-200ms).
#[tauri::command]
pub async fn get_system_health_json() -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(|| {
        let mut sys = System::new_all();
        sys.refresh_all();

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
            "pid":    p.pid().as_u32()
        })).collect();

        let per_core: Vec<f64> = sys.cpus().iter().take(12)
            .map(|c| (c.cpu_usage() as f64 * 10.0).round() / 10.0)
            .collect();

        let total_mem = sys.total_memory() / 1_048_576;
        let used_mem  = sys.used_memory()  / 1_048_576;
        let mem_pct   = if total_mem > 0 { (used_mem as f64 / total_mem as f64) * 100.0 } else { 0.0 };

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
            "disks":         disks_data,
            "top_processes": top_procs
        }))
    }).await.map_err(|e| format!("Error interno sysinfo: {}", e))?
}
