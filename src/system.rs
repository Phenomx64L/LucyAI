//! Métricas del sistema en vivo (CPU/RAM/swap/discos/host) vía `sysinfo`.
//!
//! Tauri-free: parte del corazón compartido `lucy-core`. El GUI mantiene un
//! `SysMonitor`, lo refresca cada ~1 s y pinta el `SysSnapshot`. El % de CPU es
//! un delta entre dos refrescos, por eso el monitor guarda estado.

use sysinfo::{Disks, System};

pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub total: u64,
    pub avail: u64,
}

pub struct SysSnapshot {
    pub host: String,
    pub os: String,
    pub kernel: String,
    pub cpu_brand: String,
    pub cpu_pct: f32,
    pub per_core: Vec<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub uptime_secs: u64,
    pub cores: usize,
    pub disks: Vec<DiskInfo>,
}

/// Nombre del host, sin necesitar un `SysMonitor` vivo.
///
/// La barra de estado lo pinta en cada frame y no quiere arrastrar un refresco
/// completo de métricas para leer una cadena que no cambia.
pub fn hostname() -> String {
    System::host_name().unwrap_or_else(|| "desconocido".into())
}

pub struct SysMonitor {
    sys: System,
    disks: Disks,
}

impl Default for SysMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl SysMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self {
            sys,
            disks: Disks::new_with_refreshed_list(),
        }
    }

    /// Refresca CPU + memoria + discos. Llamar cada ~1 s (el % de CPU es el delta
    /// desde el último refresco).
    pub fn refresh(&mut self) {
        self.sys.refresh_cpu();
        self.sys.refresh_memory();
        self.disks.refresh();
    }

    pub fn snapshot(&self) -> SysSnapshot {
        let per_core = self.sys.cpus().iter().map(|c| c.cpu_usage()).collect();
        let cpu_brand = self
            .sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .unwrap_or_default();
        let disks = self
            .disks
            .iter()
            .map(|d| DiskInfo {
                name: d.name().to_string_lossy().to_string(),
                mount: d.mount_point().to_string_lossy().to_string(),
                total: d.total_space(),
                avail: d.available_space(),
            })
            .collect();
        SysSnapshot {
            host: System::host_name().unwrap_or_default(),
            os: System::name().unwrap_or_default(),
            kernel: System::kernel_version().unwrap_or_default(),
            cpu_brand,
            cpu_pct: self.sys.global_cpu_info().cpu_usage(),
            per_core,
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            uptime_secs: System::uptime(),
            cores: self.sys.cpus().len(),
            disks,
        }
    }
}
