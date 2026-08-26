//! La foto de salud de un equipo remoto: CPU, memoria, discos y qué lo carga.
//!
//! ERA LO ÚLTIMO QUE ATABA UNA PANTALLA DEL SHELL NATIVO A LA V1. El Dashboard
//! de un equipo remoto enseñaba un cartel de «Qué falta» diciendo que el sondeo
//! vivía en `src-tauri`, y el cartel exageraba: el transporte —WinRM envuelto en
//! base64, SSH por clave, el escapado y la decodificación de consola— ya estaba
//! en `hosts.rs` desde hace versiones. Lo único que faltaba era ESTO: el script
//! que se manda y cómo se lee lo que vuelve.
//!
//! NO ES UN PORTE LITERAL, y las diferencias no son de gusto. El original tenía
//! dos fallos que se ven solo en según qué máquina:
//!
//!   · MEZCLABA `Get-WmiObject` Y `Get-CimInstance` EN EL MISMO SCRIPT, líneas
//!     seguidas. El primero está desaconsejado desde PowerShell 3.0 y NO EXISTE
//!     en PowerShell 7: en un servidor donde el shell por defecto sea `pwsh`, la
//!     sonda entera falla con «no se reconoce el término». Aquí todo es CIM.
//!
//!   · PEDÍA EL CONTADOR `\Processor(_Total)\% Processor Time`, Y ESE NOMBRE
//!     ESTÁ TRADUCIDO. En un Windows en español el contador se llama
//!     `\Procesador(_Total)\% de tiempo de procesador`, así que en los servidores
//!     de media Europa la CPU volvía vacía —no con error: vacía— y el panel
//!     enseñaba 0 %. Los nombres de CLASE y PROPIEDAD de CIM no se traducen
//!     nunca, así que la cuenta sale de ahí.
//!
//! Y UNA FORMA SOLA PARA LOS DOS SISTEMAS. El original devolvía `serde_json::Value`
//! con una forma para Windows —que Rust volvía a amasar después— y otra distinta
//! para Linux, armada a mano con `printf` dentro del script. Dos formas para el
//! mismo concepto significa que cada consumidor tiene que saber de qué sistema
//! vino lo que está pintando.

use crate::hosts::Host;

/// Un volumen del equipo.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Disco {
    /// `C` en Windows, `/dev/sda1` en Linux.
    pub nombre: String,
    /// Dónde está montado. En Windows coincide con el nombre.
    #[serde(default)]
    pub montaje: String,
    pub total_gb: f32,
    pub usado_gb: f32,
}

impl Disco {
    /// Cuánto lleva ocupado. DERIVADO Y NO GUARDADO: un porcentaje almacenado
    /// junto a los dos números de los que sale es un tercer sitio que puede
    /// discrepar, y el que discrepe será el que se pinte.
    pub fn pct(&self) -> f32 {
        if self.total_gb > 0.0 {
            (self.usado_gb / self.total_gb) * 100.0
        } else {
            0.0
        }
    }
}

/// Uno de los procesos que más ocupan.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Proceso {
    pub nombre: String,
    #[serde(default)]
    pub cpu: f32,
    pub mem_mb: u64,
}

/// La foto entera.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Salud {
    pub hostname: String,
    pub os: String,
    pub uptime_h: u64,
    pub cpu_pct: f32,
    pub cpu_cores: u32,
    pub mem_total_mb: u64,
    pub mem_used_mb: u64,
    #[serde(default)]
    pub discos: Vec<Disco>,
    #[serde(default)]
    pub procesos: Vec<Proceso>,
}

impl Salud {
    pub fn mem_pct(&self) -> f32 {
        if self.mem_total_mb > 0 {
            (self.mem_used_mb as f32 / self.mem_total_mb as f32) * 100.0
        } else {
            0.0
        }
    }

    /// El disco que antes se va a llenar.
    ///
    /// El MÁS LLENO y no el del sistema: la pregunta que contesta un panel de
    /// salud es cuál va a dar problemas, y un disco de datos al 97 % los da
    /// antes que un `C:` al 60 %.
    pub fn peor_disco(&self) -> Option<&Disco> {
        self.discos
            .iter()
            .max_by(|a, b| a.pct().partial_cmp(&b.pct()).unwrap_or(std::cmp::Ordering::Equal))
    }
}

/// Cuántos procesos se piden. Cinco: es lo que cabe en una tarjeta y lo que se
/// mira de verdad. Veinte es un listado, y para eso está el módulo de procesos.
const TOP: usize = 5;

/// El script de Windows. TODO CIM, ningún `Get-WmiObject`.
///
/// La CPU sale de `Win32_PerfFormattedData_PerfOS_Processor`, cuyos nombres de
/// clase y propiedad son invariantes. El contador de rendimiento por nombre
/// —`\Processor(_Total)\% Processor Time`— está traducido en cada idioma de
/// Windows, y pedirlo por su nombre en inglés en un servidor en español no da
/// error: da nada.
fn script_windows() -> String {
    format!(
        r#"
$ErrorActionPreference = 'SilentlyContinue'
$os  = Get-CimInstance Win32_OperatingSystem
$cs  = Get-CimInstance Win32_ComputerSystem
$cpu = (Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor |
        Where-Object {{ $_.Name -eq '_Total' }}).PercentProcessorTime
if ($null -eq $cpu) {{
    # Respaldo por si la clase de rendimiento no responde. Es mas basta —una
    # media del ultimo intervalo del propio procesador— pero es mejor que cero.
    $cpu = (Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average
}}
$discos = Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | ForEach-Object {{
    [PSCustomObject]@{{
        nombre   = $_.DeviceID.TrimEnd(':')
        montaje  = $_.DeviceID
        total_gb = [Math]::Round($_.Size / 1GB, 1)
        usado_gb = [Math]::Round(($_.Size - $_.FreeSpace) / 1GB, 1)
    }}
}}
$procesos = Get-Process | Sort-Object WorkingSet64 -Descending | Select-Object -First {TOP} |
    ForEach-Object {{
        [PSCustomObject]@{{
            nombre = $_.Name
            cpu    = [Math]::Round(([double]$_.CPU), 1)
            mem_mb = [int][Math]::Round($_.WorkingSet64 / 1MB, 0)
        }}
    }}
$total = [int][Math]::Round($os.TotalVisibleMemorySize / 1KB, 0)
$libre = [int][Math]::Round($os.FreePhysicalMemory / 1KB, 0)
[PSCustomObject]@{{
    hostname      = $env:COMPUTERNAME
    os            = $os.Caption
    uptime_h      = [int][Math]::Round((New-TimeSpan -Start $os.LastBootUpTime).TotalHours, 0)
    cpu_pct       = [Math]::Round(([double]$cpu), 1)
    cpu_cores     = [int]$cs.NumberOfLogicalProcessors
    mem_total_mb  = $total
    mem_used_mb   = [Math]::Max(0, $total - $libre)
    discos        = @($discos)
    procesos      = @($procesos)
}} | ConvertTo-Json -Depth 4 -Compress
"#
    )
}

/// El script de Linux. Sin dependencias fuera de coreutils y `/proc`.
///
/// La CPU se mide con DOS lecturas de `/proc/stat` separadas por un segundo,
/// porque la primera columna es un acumulado desde el arranque: leerla una vez
/// da la media de toda la vida de la máquina, que no es lo que nadie pregunta.
fn script_linux() -> String {
    format!(
        r#"
set -u
HOSTNAME=$(hostname)
UPTIME_H=$(awk '{{print int($1/3600)}}' /proc/uptime)
CORES=$(nproc 2>/dev/null || echo 1)
A=$(awk 'NR==1{{u=$2+$3+$4; t=u+$5+$6+$7+$8; print u" "t}}' /proc/stat)
sleep 1
B=$(awk 'NR==1{{u=$2+$3+$4; t=u+$5+$6+$7+$8; print u" "t}}' /proc/stat)
CPU=$(echo "$A $B" | awk '{{du=$3-$1; dt=$4-$2; if(dt>0) printf "%.1f", du/dt*100; else print "0"}}')
MEM_TOTAL=$(awk '/^MemTotal/{{print int($2/1024)}}' /proc/meminfo)
MEM_AVAIL=$(awk '/^MemAvailable/{{print int($2/1024)}}' /proc/meminfo)
MEM_USED=$((MEM_TOTAL - MEM_AVAIL))
OS=$(. /etc/os-release 2>/dev/null && echo "$PRETTY_NAME")
[ -z "${{OS:-}}" ] && OS="Linux"
DISCOS=$(df -B1 2>/dev/null | awk 'NR>1 && /^\/dev\// {{
    printf "{{\"nombre\":\"%s\",\"montaje\":\"%s\",\"total_gb\":%.1f,\"usado_gb\":%.1f}},", $1, $6, $2/1073741824, $3/1073741824
}}' | sed 's/,$//')
PROCS=$(ps -eo comm=,pcpu=,rss= --sort=-rss 2>/dev/null | head -{TOP} | awk '{{
    printf "{{\"nombre\":\"%s\",\"cpu\":%s,\"mem_mb\":%d}},", $1, $2, $3/1024
}}' | sed 's/,$//')
printf '{{"hostname":"%s","os":"%s","uptime_h":%d,"cpu_pct":%s,"cpu_cores":%d,"mem_total_mb":%d,"mem_used_mb":%d,"discos":[%s],"procesos":[%s]}}' \
    "$HOSTNAME" "$OS" "$UPTIME_H" "$CPU" "$CORES" "$MEM_TOTAL" "$MEM_USED" "$DISCOS" "$PROCS"
"#
    )
}

/// Pide la foto a un equipo. Bloquea: llámalo desde un hilo.
///
/// EL TRANSPORTE NO SE REIMPLEMENTA. `hosts::run_remote` ya decide protocolo,
/// envuelve el script en base64 UTF-16LE para que su contenido no pueda cerrar
/// el bloque que lo contiene, y decodifica la consola. La versión de la V1 tenía
/// su propio lanzador de `ssh` y su propio `run_winrm` dentro de cada función —
/// tres copias del mismo transporte, y la que se arreglara sola sería la que
/// difiriera.
pub fn sonda(h: &Host, password: &str) -> Result<Salud, String> {
    let script = if h.protocol.os() == "linux" { script_linux() } else { script_windows() };
    let (salida, error, ok) = crate::hosts::run_remote(h, password, &script)?;
    if !ok {
        return Err(crate::hosts::motivo_de(&error, !salida.trim().is_empty()));
    }
    parsea(&salida)
}

/// Separa el JSON de lo que venga pegado.
///
/// SE BUSCA LA PRIMERA LLAVE en vez de confiar en que la salida empiece por
/// ella. Un perfil de PowerShell que escribe un saludo, un aviso de política, un
/// «Warning:» de `df` — cualquiera de esos antepone líneas, y `from_str` sobre
/// todo el bloque falla con «expected value at line 1» sin decir que el JSON sí
/// estaba, tres renglones más abajo.
fn parsea(salida: &str) -> Result<Salud, String> {
    let t = salida.trim();
    let ini = t.find('{').ok_or_else(|| {
        format!("El equipo no devolvió datos. Contestó: {}", recorta(t, 200))
    })?;
    let fin = t.rfind('}').map(|i| i + 1).unwrap_or(t.len());
    serde_json::from_str(&t[ini..fin]).map_err(|e| {
        format!("No se entiende lo que devolvió el equipo: {e}. Empezaba por: {}", recorta(t, 200))
    })
}

fn recorta(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    format!("{}…", s.chars().take(n).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"hostname":"SRV-04","os":"Windows Server 2022","uptime_h":73,
        "cpu_pct":12.5,"cpu_cores":8,"mem_total_mb":16384,"mem_used_mb":9012,
        "discos":[{"nombre":"C","montaje":"C:","total_gb":476.0,"usado_gb":300.0},
                  {"nombre":"D","montaje":"D:","total_gb":100.0,"usado_gb":97.0}],
        "procesos":[{"nombre":"sqlservr","cpu":812.4,"mem_mb":4096}]}"#;

    #[test]
    fn se_lee_la_foto_entera() {
        let s = parsea(JSON).expect("JSON válido");
        assert_eq!(s.hostname, "SRV-04");
        assert_eq!(s.cpu_cores, 8);
        assert_eq!(s.discos.len(), 2);
        assert_eq!(s.procesos[0].nombre, "sqlservr");
        assert!((s.mem_pct() - 55.0).abs() < 0.5, "memoria: {}", s.mem_pct());
    }

    #[test]
    fn el_peor_disco_es_el_mas_lleno_y_no_el_del_sistema() {
        // La pregunta que contesta un panel de salud es cuál va a dar problemas.
        // Un disco de datos al 97 % los da antes que un `C:` al 63 %.
        let s = parsea(JSON).unwrap();
        let peor = s.peor_disco().expect("hay discos");
        assert_eq!(peor.nombre, "D", "eligió el del sistema en vez del más lleno");
        assert!(peor.pct() > 96.0);
    }

    #[test]
    fn un_saludo_del_perfil_delante_no_rompe_la_lectura() {
        // Un perfil de PowerShell que escribe algo, un aviso de política, un
        // «Warning» de `df`: cualquiera antepone líneas. `from_str` sobre todo
        // el bloque falla con «expected value at line 1» y no dice que el JSON
        // estaba tres renglones más abajo.
        let sucio = format!("Bienvenido a SRV-04\nWARNING: perfil no cargado\n{JSON}\n");
        let s = parsea(&sucio).expect("tenía que encontrar el JSON");
        assert_eq!(s.hostname, "SRV-04");
    }

    #[test]
    fn si_no_vuelve_nada_el_error_dice_que_contesto() {
        // «El equipo no devolvió datos» a secas deja al operador sin saber si
        // falló la conexión, la credencial o el script. Lo que sí escribió es la
        // única pista que hay.
        let e = parsea("bash: nproc: command not found").unwrap_err();
        assert!(e.contains("nproc"), "el error se comió la pista: {e}");
    }

    #[test]
    fn el_script_de_windows_no_usa_el_cmdlet_retirado_ni_el_contador_traducido() {
        // LOS DOS FALLOS DEL ORIGINAL, fijados. `Get-WmiObject` no existe en
        // PowerShell 7 —en un servidor con `pwsh` por defecto la sonda entera
        // falla— y el contador `\Processor(_Total)\% Processor Time` tiene el
        // nombre traducido en cada idioma de Windows, así que en un servidor en
        // español devolvía vacío y el panel enseñaba 0 %.
        let s = script_windows();
        assert!(!s.contains("Get-WmiObject"), "volvió el cmdlet retirado");
        assert!(!s.contains("Get-Counter"), "volvió el contador con nombre traducido");
        assert!(s.contains("Get-CimInstance"), "el script ya no consulta CIM");
    }

    #[test]
    fn los_dos_scripts_piden_los_mismos_campos() {
        // UNA FORMA SOLA PARA LOS DOS SISTEMAS. El original devolvía una forma
        // para Windows —que Rust volvía a amasar— y otra para Linux, armada a
        // mano en el script. Con dos formas, cada consumidor tiene que saber de
        // qué sistema vino lo que pinta.
        let (w, l) = (script_windows(), script_linux());
        for campo in [
            "hostname", "os", "uptime_h", "cpu_pct", "cpu_cores", "mem_total_mb",
            "mem_used_mb", "discos", "procesos",
        ] {
            assert!(w.contains(campo), "el script de Windows no emite «{campo}»");
            assert!(l.contains(campo), "el script de Linux no emite «{campo}»");
        }
    }

    #[test]
    fn un_equipo_sin_discos_no_revienta() {
        // Un contenedor, una máquina donde el filtro no casó nada. Los campos
        // que faltan son `default` a propósito: media foto sirve, y un panel en
        // blanco por un array vacío no.
        let s = parsea(
            r#"{"hostname":"c1","os":"Alpine","uptime_h":1,"cpu_pct":0.0,"cpu_cores":1,
                "mem_total_mb":512,"mem_used_mb":100}"#,
        )
        .expect("los arrays son opcionales");
        assert!(s.discos.is_empty());
        assert_eq!(s.peor_disco(), None);
    }
}
