// ── SYSTEM — Métricas de salud del sistema local ────────────────────────────────

use sysinfo::System;
use serde_json::json;
use chrono::Local;
use std::process::Command;

fn driver_url_from_manufacturer(manufacturer: &str, model: &str) -> String {
    let m = manufacturer.to_lowercase();
    let query = format!("{} {} drivers", manufacturer, model).replace(' ', "+");
    if m.contains("dell") {
        "https://www.dell.com/support/home".to_string()
    } else if m.contains("hp") || m.contains("hewlett") {
        "https://support.hp.com".to_string()
    } else if m.contains("lenovo") {
        "https://pcsupport.lenovo.com".to_string()
    } else if m.contains("asus") {
        "https://www.asus.com/support".to_string()
    } else if m.contains("acer") {
        "https://www.acer.com/support".to_string()
    } else if m.contains("msi") {
        "https://www.msi.com/support".to_string()
    } else {
        format!("https://www.google.com/search?q={}", query)
    }
}

fn get_local_hardware_specs() -> serde_json::Value {
    let cpu_model_fallback = "N/A".to_string();
    let gpu_model_fallback = "N/A".to_string();
    let manufacturer_fallback = "Unknown".to_string();
    let model_fallback = "Unknown".to_string();

    if !cfg!(target_os = "windows") {
        return json!({
            "cpu_model": cpu_model_fallback,
            "cpu_current_ghz": serde_json::Value::Null,
            "cpu_max_ghz": serde_json::Value::Null,
            "gpu_model": gpu_model_fallback,
            "gpu_vram_mb": serde_json::Value::Null,
            "machine_manufacturer": manufacturer_fallback,
            "machine_model": model_fallback,
            "driver_url": "https://www.google.com/search?q=drivers"
        });
    }

    let ps_script = r#"
        $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1 Name, CurrentClockSpeed, MaxClockSpeed
        $gpu = Get-CimInstance Win32_VideoController | Select-Object -First 1 Name, AdapterRAM
        $cs  = Get-CimInstance Win32_ComputerSystem | Select-Object -First 1 Manufacturer, Model
        $bios = Get-CimInstance Win32_BIOS | Select-Object -First 1 SerialNumber
        $csp  = Get-CimInstance Win32_ComputerSystemProduct | Select-Object -First 1 IdentifyingNumber
        $serial = $bios.SerialNumber
        if (-not $serial) { $serial = $csp.IdentifyingNumber }
        [PSCustomObject]@{
            cpu_model = $cpu.Name
            cpu_current_mhz = [double]($cpu.CurrentClockSpeed)
            cpu_max_mhz = [double]($cpu.MaxClockSpeed)
            gpu_model = $gpu.Name
            gpu_vram_mb = if ($gpu.AdapterRAM) { [math]::Round($gpu.AdapterRAM / 1MB, 0) } else { $null }
            machine_manufacturer = $cs.Manufacturer
            machine_model = $cs.Model
            serial_number = $serial
        } | ConvertTo-Json -Compress
    "#;

    let output = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(ps_script)
        .output();

    let parsed = output
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok());

    let cpu_model = parsed
        .as_ref()
        .and_then(|v| v.get("cpu_model"))
        .and_then(|v| v.as_str())
        .unwrap_or(cpu_model_fallback.as_str())
        .to_string();
    let gpu_model = parsed
        .as_ref()
        .and_then(|v| v.get("gpu_model"))
        .and_then(|v| v.as_str())
        .unwrap_or(gpu_model_fallback.as_str())
        .to_string();
    let machine_manufacturer = parsed
        .as_ref()
        .and_then(|v| v.get("machine_manufacturer"))
        .and_then(|v| v.as_str())
        .unwrap_or(manufacturer_fallback.as_str())
        .to_string();
    let machine_model = parsed
        .as_ref()
        .and_then(|v| v.get("machine_model"))
        .and_then(|v| v.as_str())
        .unwrap_or(model_fallback.as_str())
        .to_string();
    let serial_number = parsed
        .as_ref()
        .and_then(|v| v.get("serial_number"))
        .and_then(|v| v.as_str())
        .unwrap_or("N/A")
        .to_string();

    let cpu_current_ghz = parsed
        .as_ref()
        .and_then(|v| v.get("cpu_current_mhz"))
        .and_then(|v| v.as_f64())
        .map(|mhz| (mhz / 1000.0 * 100.0).round() / 100.0);
    let cpu_max_ghz = parsed
        .as_ref()
        .and_then(|v| v.get("cpu_max_mhz"))
        .and_then(|v| v.as_f64())
        .map(|mhz| (mhz / 1000.0 * 100.0).round() / 100.0);
    let gpu_vram_mb = parsed
        .as_ref()
        .and_then(|v| v.get("gpu_vram_mb"))
        .and_then(|v| v.as_f64())
        .map(|n| n.round() as u64);

    json!({
        "cpu_model": cpu_model,
        "cpu_current_ghz": cpu_current_ghz,
        "cpu_max_ghz": cpu_max_ghz,
        "gpu_model": gpu_model,
        "gpu_vram_mb": gpu_vram_mb,
        "machine_manufacturer": machine_manufacturer,
        "machine_model": machine_model,
        "serial_number": serial_number,
        "driver_url": driver_url_from_manufacturer(&machine_manufacturer, &machine_model)
    })
}

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
        let hardware = get_local_hardware_specs();

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
            "top_processes": top_procs,
            "hardware":      hardware
        }))
    }).await.map_err(|e| format!("Error interno sysinfo: {}", e))?
}
