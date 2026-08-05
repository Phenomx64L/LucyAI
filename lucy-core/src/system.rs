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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_nonzero_exit_code_counts_as_crashed() {
        // La distinción que hace legible el indicador: parado limpio se informa,
        // fallado alarma. Confundirlos es lo que hacía que la máquina pasara a
        // Atención en cada arranque.
        assert!(!DownService { name: "sppsvc".into(), exit_code: 0 }.crashed());
        assert!(DownService { name: "algo".into(), exit_code: 1 }.crashed());
        assert!(DownService { name: "otro".into(), exit_code: 1067 }.crashed());
    }

    #[test]
    fn a_first_reading_reports_no_traffic_rather_than_a_spike() {
        // `total_received()` es acumulado desde el arranque del equipo. Sin
        // lectura previa no hay tasa, y devolver el acumulado como si fuera
        // velocidad pintaría cientos de megabytes por segundo en el primer
        // frame.
        let mut m = SysMonitor::new();
        let first = m.net_rate();
        assert_eq!(first.rx_bps, 0, "la primera lectura no puede inventar caudal");
        assert_eq!(first.tx_bps, 0);
    }

    #[test]
    fn a_second_reading_produces_a_rate_without_overflowing() {
        // No se afirma un valor —depende del tráfico real— sino que la resta se
        // comporta: nada de números gigantes por envolvimiento.
        let mut m = SysMonitor::new();
        let _ = m.net_rate();
        std::thread::sleep(std::time::Duration::from_millis(60));
        let r = m.net_rate();
        // 100 Gbps es imposible en una prueba: ese valor solo aparece si un
        // contador se reinició y la resta envolvió.
        assert!(r.rx_bps < 12_500_000_000, "caudal absurdo: {}", r.rx_bps);
        assert!(r.tx_bps < 12_500_000_000, "caudal absurdo: {}", r.tx_bps);
    }

    #[test]
    fn top_processes_respects_the_limit_and_sorts_by_the_asked_key() {
        let mut m = SysMonitor::new();
        let by_mem = m.top_processes(5, false);
        assert!(by_mem.len() <= 5);
        for w in by_mem.windows(2) {
            assert!(w[0].mem_bytes >= w[1].mem_bytes, "no está ordenado por RAM");
        }
        let by_cpu = m.top_processes(5, true);
        for w in by_cpu.windows(2) {
            assert!(w[0].cpu_pct >= w[1].cpu_pct, "no está ordenado por CPU");
        }
    }
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

/// Un servicio automático que no está corriendo.
#[derive(Debug, Clone)]
pub struct DownService {
    pub name: String,
    /// Código de salida. `0` = parado limpiamente; distinto de `0` = FALLÓ.
    pub exit_code: i64,
}

impl DownService {
    /// Solo esto merece teñir el indicador del equipo.
    ///
    /// Un servicio parado y limpio es información real y va en su panel, pero no
    /// es una alarma. La distinción es la diferencia entre un indicador que se
    /// lee y uno que se ignora.
    pub fn crashed(&self) -> bool {
        self.exit_code != 0
    }
}

/// Servicios automáticos detenidos, vía CIM.
///
/// **CIM y no `Get-Service`, y esto costó un bug.** El `StartType` de
/// `Get-Service` reporta "Automatic (Delayed Start)" como `Automatic` a secas —
/// la marca de retardo sencillamente no está en ese objeto—, así que no puede
/// distinguir un servicio que FALLÓ al arrancar de uno que Windows arranca tarde
/// a propósito. En una máquina recién encendida eso reportaba asus, edgeupdate,
/// MapsBroker y sppsvc como caídos mientras se comportaban exactamente como
/// están diseñados, y como cualquier lista no vacía levanta un aviso, el equipo
/// pasaba de Saludable a Atención un minuto después de cada arranque. Un
/// indicador que da la voz de alarma en cada encendido enseña al operador a
/// dejar de leerlo.
///
/// `ExitCode -ne 0` conserva la mitad útil: un servicio de arranque retardado
/// que se estrelló de verdad sigue apareciendo. Solo se filtra el caso "tarde o
/// inactivo, salió limpio".
///
/// SIN RESOLVER: los servicios de arranque por disparador (AppXSvc) se apagan
/// solos y reaparecen. Ni WMI ni CIM exponen los disparadores, y `sc.exe
/// qtriggerinfo` no ayuda — su código de salida es 0 en cualquier caso y su
/// salida está LOCALIZADA, que es la misma trampa que hizo ilegible el registro
/// de seguridad en un Windows en español.
#[cfg(windows)]
pub fn down_services(limit: usize) -> Result<Vec<DownService>, String> {
    let script = format!(
        "Get-CimInstance Win32_Service -Filter \"StartMode='Auto' AND State='Stopped'\" | \
         Where-Object {{ -not $_.DelayedAutoStart -or $_.ExitCode -ne 0 }} | \
         Select-Object -First {limit} | ForEach-Object {{ \"$($_.Name)|$($_.ExitCode)\" }}"
    );
    let (stdout, _stderr, _ok) = crate::shell::run_powershell_utf8(&script)?;
    Ok(stdout
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() {
                return None;
            }
            // `nombre|código`. Una línea sin `|` (un host más antiguo, una forma
            // inesperada) se toma como parada-limpia en vez de descartarse: el
            // servicio existe y merece verse, solo que sin la certeza de que
            // falló.
            match l.split_once('|') {
                Some((name, code)) => Some(DownService {
                    name: name.trim().to_string(),
                    exit_code: code.trim().parse().unwrap_or(0),
                }),
                None => Some(DownService { name: l.to_string(), exit_code: 0 }),
            }
        })
        .collect())
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

/// La hora LOCAL del equipo: `(hora, minuto, segundo)`.
///
/// POR QUÉ NO SE CALCULA DESDE EL EPOCH. `SystemTime` da UTC, y hasta ahora todo
/// el shell nativo hacía `segundos % 86_400` con eso — lo que en México son seis
/// horas de desfase. El síntoma visible fue un "Buenos días" a las diez de la
/// noche, pero la marca de hora de cada mensaje estaba igual de mal y eso no se
/// nota hasta que alguien intenta cruzarla con un log.
///
/// Convertir UTC a local a mano exige la zona horaria, el horario de verano y
/// sus reglas históricas — que es exactamente lo que `chrono` trae y por lo que
/// pesa. Windows ya lo sabe: `GetLocalTime` devuelve la hora de pared ya
/// resuelta, sin dependencias y sin reglas que mantener.
#[cfg(windows)]
pub fn local_time() -> (u32, u32, u32) {
    use winapi::um::sysinfoapi::GetLocalTime;
    let mut st = unsafe { std::mem::zeroed() };
    // SAFETY: `GetLocalTime` solo escribe la estructura que se le pasa, y se le
    // pasa una válida y del tamaño correcto.
    unsafe { GetLocalTime(&mut st) };
    (st.wHour as u32, st.wMinute as u32, st.wSecond as u32)
}

#[cfg(not(windows))]
pub fn local_time() -> (u32, u32, u32) {
    (0, 0, 0)
}

#[cfg(all(test, windows))]
mod hora {
    use super::*;

    #[test]
    fn la_hora_local_esta_dentro_de_un_reloj() {
        let (h, m, s) = local_time();
        assert!(h < 24, "hora fuera de rango: {h}");
        assert!(m < 60 && s < 60);
    }

    #[test]
    fn no_es_la_hora_utc_salvo_en_utc() {
        // No se puede afirmar que difieran —hay máquinas en UTC— pero sí que
        // `local_time` no está devolviendo el epoch crudo: el fallo original era
        // usar `SystemTime` directamente, y eso da la hora UTC exacta.
        let utc = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| (d.as_secs() % 86_400) / 3600)
            .unwrap_or(99) as u32;
        let (h, _, _) = local_time();
        // Si coinciden, la máquina está en UTC y no hay nada que comprobar; si
        // no, el desfase tiene que ser uno de los de una zona horaria real.
        if h != utc {
            let d = (h as i32 - utc as i32).rem_euclid(24);
            assert!((1..=23).contains(&d), "desfase imposible: {d}");
        }
    }
}
