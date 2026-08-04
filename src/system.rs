//! Métricas del sistema en vivo (CPU/RAM/swap/discos/host) vía `sysinfo`.
//!
//! Tauri-free: parte del corazón compartido `lucy-core`. El GUI mantiene un
//! `SysMonitor`, lo refresca cada ~1 s y pinta el `SysSnapshot`. El % de CPU es
//! un delta entre dos refrescos, por eso el monitor guarda estado.

use sysinfo::{Disks, Networks, System};

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

/// Caudal de red, en bytes por segundo.
///
/// Es un DELTA, no un total: `total_received()` de sysinfo devuelve lo
/// acumulado desde el arranque del equipo, así que un dashboard que lo pintara
/// tal cual mostraría un número que solo sube y no significa nada. El monitor
/// guarda la lectura anterior y su instante para poder restar.
///
/// La primera llamada devuelve 0. Es lo correcto: sin lectura previa no hay
/// tasa que calcular, y un pico inventado en el primer frame es peor que un
/// cero honesto.
#[derive(Debug, Clone, Copy, Default)]
pub struct NetRate {
    pub rx_bps: u64,
    pub tx_bps: u64,
}

/// Un proceso, para la tabla de los que más consumen.
#[derive(Debug, Clone)]
pub struct ProcInfo {
    pub name: String,
    pub pid: u32,
    pub cpu_pct: f32,
    pub mem_bytes: u64,
}

pub struct SysMonitor {
    sys: System,
    disks: Disks,
    networks: Networks,
    /// (recibido, enviado) acumulados en la última lectura, y cuándo fue.
    /// `None` hasta la primera: ver `NetRate`.
    last_net: Option<(u64, u64, std::time::Instant)>,
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
            networks: Networks::new_with_refreshed_list(),
            last_net: None,
        }
    }

    /// Refresca CPU + memoria + discos. Llamar cada ~1 s (el % de CPU es el delta
    /// desde el último refresco).
    pub fn refresh(&mut self) {
        self.sys.refresh_cpu();
        self.sys.refresh_memory();
        self.disks.refresh();
    }

    /// Caudal de red desde la última llamada A ESTA función.
    ///
    /// Se refresca y se calcula el delta aquí mismo, en vez de en `refresh()`,
    /// porque la tasa depende del intervalo entre lecturas: mezclarla con el
    /// refresco general ataría el cálculo a la cadencia de todo lo demás.
    ///
    /// Se suma sobre TODAS las interfaces. Un desglose por adaptador es útil en
    /// otra vista; lo que la tarjeta del dashboard responde es "cuánto está
    /// entrando y saliendo del equipo", y ahí un loopback a cero no estorba.
    pub fn net_rate(&mut self) -> NetRate {
        self.networks.refresh();
        let (mut recv, mut send) = (0u64, 0u64);
        for (_iface, data) in self.networks.iter() {
            // `total_*` es lo acumulado DESDE EL ARRANQUE del equipo, no desde
            // el último refresco. De ahí que haga falta guardar la lectura
            // anterior: usar esto directamente pinta un contador creciente
            // disfrazado de velocidad.
            recv += data.total_received();
            send += data.total_transmitted();
        }
        let now = std::time::Instant::now();
        let rate = match self.last_net {
            None => NetRate::default(),
            Some((prev_r, prev_s, prev_t)) => {
                let secs = now.duration_since(prev_t).as_secs_f64();
                if secs <= 0.0 {
                    NetRate::default()
                } else {
                    NetRate {
                        // `saturating_sub`: un contador puede reiniciarse si la
                        // interfaz se cae y vuelve. Sin esto, el resultado
                        // envolvería a un número gigante y la tarjeta mostraría
                        // gigabytes por segundo.
                        rx_bps: ((recv.saturating_sub(prev_r)) as f64 / secs) as u64,
                        tx_bps: ((send.saturating_sub(prev_s)) as f64 / secs) as u64,
                    }
                }
            }
        };
        self.last_net = Some((recv, send, now));
        rate
    }

    /// Los `n` procesos que más consumen, ordenados por RAM o por CPU.
    ///
    /// `refresh_processes` es CARO comparado con CPU o memoria: recorre la tabla
    /// de procesos entera. Por eso está aparte del `refresh()` de 1 s — la tabla
    /// del dashboard puede actualizarse más despacio que los medidores sin que
    /// se note, y lo contrario cuesta batería todo el día.
    pub fn top_processes(&mut self, n: usize, by_cpu: bool) -> Vec<ProcInfo> {
        self.sys.refresh_processes();
        let mut procs: Vec<ProcInfo> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, p)| ProcInfo {
                name: p.name().to_string(),
                pid: pid.as_u32(),
                cpu_pct: p.cpu_usage(),
                mem_bytes: p.memory(),
            })
            .collect();
        if by_cpu {
            procs.sort_by(|a, b| {
                b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap_or(std::cmp::Ordering::Equal)
            });
        } else {
            procs.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
        }
        procs.truncate(n);
        procs
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
