//! Lo que el equipo ES, y no lo que está haciendo ahora mismo.
//!
//! `system` mide lo que cambia —CPU, memoria, discos— y lo relee cada segundo.
//! Esto es lo otro: fabricante, modelo, gráficas, número de serie y zócalos. No
//! cambia mientras el equipo esté encendido, y por eso se pregunta UNA vez.
//!
//! ── DE DÓNDE VIENE ───────────────────────────────────────────────────────────
//!
//! De una rama de la V1 que nunca se fusionó, `feature/PO-LUCY-ui-adjust` —
//! etiquetada como `v1-feature-hardware-specs` para que no se perdiera al
//! limpiar el repositorio. Aquella versión pedía todo esto a WMI en una sola
//! llamada a PowerShell, y esa parte se conserva tal cual: es lo correcto.
//!
//! Lo que NO se conserva son tres decisiones suyas, y las tres las tumbó medir
//! contra la máquina del operador y no razonar sobre ellas:
//!
//!   1. `Select-Object -First 1` EN LAS GRÁFICAS. Un portátil de administrador
//!      lleva dos —una integrada y una discreta— y WMI no promete un orden. Ahí
//!      salieron una RTX 5070 y una Radeon 610M; enseñar solo la integrada, que
//!      es lo que podía pasar, es peor que no enseñar ninguna.
//!
//!   2. `AdapterRAM` COMO MEMORIA DE VÍDEO. El campo es un `uint32`, así que se
//!      queda a las puertas de 4 GiB pase lo que pase. Medido: devolvió
//!      4.293.918.720 —4,0 GB— para una RTX 5070 Laptop que tiene ocho. No es
//!      que sea impreciso: es que MIENTE en cuanto la tarjeta pasa de 4 GB, que
//!      hoy son todas. No se pide.
//!
//!   3. EL MODELO Y LA FRECUENCIA DE LA CPU, que `sysinfo` ya da. Preguntar dos
//!      veces por el mismo hecho es tener dos fuentes que un día discrepan, y
//!      este proyecto ya pagó eso una vez con los umbrales del consolidador.
//!
//! Tampoco se porta la URL de drivers. Lo que hacía era abrir la página de
//! inicio del fabricante —`https://support.hp.com`— o, si no lo reconocía, una
//! búsqueda de Google con el modelo. Eso es un marcador, no una función. Si
//! algún día se hace, se hace con el número de serie en la URL, que es lo que
//! Dell y Lenovo aceptan y lo que de verdad ahorra el viaje.

/// Lo que el equipo es. Todo puede venir vacío: WMI no está en todas partes y
/// una máquina virtual rellena la mitad de estos campos con «System
/// Manufacturer».
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hardware {
    /// Ya limpio: «Micro-Star International Co., Ltd.» sale como «MSI».
    pub fabricante: String,
    pub modelo: String,
    /// ENTERO. Quien lo pinta decide cuánto enseña — ver [`enmascara`].
    pub serie: String,
    /// TODAS, en el orden que las da el sistema. Ver la nota del módulo.
    pub gpus: Vec<String>,
    /// Zócalos ocupados. `NumberOfProcessors` de `Win32_ComputerSystem`, que es
    /// el número de PAQUETES y no de núcleos.
    pub sockets: usize,
}

impl Hardware {
    /// ¿Hay algo que enseñar? Una máquina donde WMI no contesta devuelve todo
    /// vacío, y una sección vacía es peor que ninguna sección.
    pub fn hay_algo(&self) -> bool {
        !self.fabricante.is_empty()
            || !self.modelo.is_empty()
            || !self.serie.is_empty()
            || !self.gpus.is_empty()
    }

    /// «MSI Crosshair A16 HX D8WGKG», sin dejar un espacio colgando si falta uno.
    pub fn equipo(&self) -> String {
        [self.fabricante.trim(), self.modelo.trim()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// El fabricante, sin la razón social.
///
/// `Win32_ComputerSystem.Manufacturer` devuelve el nombre legal completo:
/// «Micro-Star International Co., Ltd.», «Hewlett-Packard», «LENOVO». En una
/// línea que ya lleva el modelo detrás, la mitad de los caracteres se los come
/// el «Co., Ltd.» — y lo que se quiere leer es de quién es la máquina.
///
/// SE TRADUCE LO CONOCIDO Y SE RECORTA EL RESTO, en ese orden. Un fabricante que
/// no esté en la lista sale con su nombre, sin la coletilla societaria: se
/// prefiere un nombre largo pero cierto a uno corto e inventado.
pub fn marca_equipo(bruto: &str) -> String {
    let b = bruto.trim();
    let bajo = b.to_lowercase();
    for (aguja, nombre) in [
        ("micro-star", "MSI"),
        ("hewlett", "HP"),
        ("lenovo", "Lenovo"),
        ("dell", "Dell"),
        ("asustek", "ASUS"),
        ("acer", "Acer"),
        ("gigabyte", "Gigabyte"),
        ("supermicro", "Supermicro"),
        ("microsoft", "Microsoft"),
        ("apple", "Apple"),
        ("samsung", "Samsung"),
        ("toshiba", "Toshiba"),
        ("fujitsu", "Fujitsu"),
        ("vmware", "VMware"),
        ("qemu", "QEMU"),
        ("innotek", "VirtualBox"),
        ("parallels", "Parallels"),
    ] {
        if bajo.contains(aguja) {
            return nombre.to_string();
        }
    }
    // Lo que queda: fuera la forma societaria y la coma que la precede.
    let mut s = b.to_string();
    for corte in [", Ltd", ", Inc", ", LLC", ", Co", " Co., Ltd", " Corporation", " Computer"] {
        if let Some(i) = s.to_lowercase().find(&corte.to_lowercase()) {
            s.truncate(i);
        }
    }
    s.trim_end_matches([',', '.', ' ']).to_string()
}

/// Cuántos caracteres del número de serie se dejan a la vista por cada punta.
///
/// Cuatro y cuatro. Con eso, quien tiene el equipo delante reconoce el suyo y
/// puede cotejarlo con la pegatina; quien mire una captura de pantalla no se
/// lleva el número con el que se abre un caso de garantía a nombre de otro.
pub const PUNTA_SERIE: usize = 4;

/// El número de serie para ENSEÑAR, con el centro tapado.
///
/// SE ENMASCARA POR DEFECTO, y el operador lo copia entero si lo necesita. El
/// pantallazo de un dashboard acaba en un chat, en un ticket o en una
/// presentación; el número de serie identifica la máquina ante el fabricante.
///
/// Uno CORTO no se toca: por debajo de dos puntas más algo en medio, enmascarar
/// deja algo como «9S71…» que no identifica nada y encima parece un fallo.
pub fn enmascara(serie: &str) -> String {
    let s = serie.trim();
    let n = s.chars().count();
    if n <= PUNTA_SERIE * 2 + 1 {
        return s.to_string();
    }
    let ini: String = s.chars().take(PUNTA_SERIE).collect();
    let fin: String = s.chars().skip(n - PUNTA_SERIE).collect();
    format!("{ini}····{fin}")
}

/// Lee lo que WMI sabe del equipo. BLOQUEANTE: quien llama ya está en un hilo.
///
/// UNA SOLA LLAMADA A POWERSHELL para las cuatro clases. Arrancar PowerShell
/// cuesta del orden de doscientos milisegundos y las consultas en sí casi nada,
/// así que hacer cuatro llamadas costaría cuatro arranques para el mismo dato.
///
/// Y NO SE LLAMA DESDE EL HILO QUE PINTA. Este módulo lo dice y `system` ya lo
/// tenía escrito para el sondeo de procesos: un frame son 16,7 ms, y esto son
/// cientos. Se pide una vez, en un hilo, y se enseña cuando llega.
#[cfg(windows)]
pub fn sonda() -> Result<Hardware, String> {
    // `-First 1` SOLO donde hay uno por definición. En las gráficas no: ver la
    // nota del módulo. `@()` fuerza el array aunque haya una sola, porque
    // `ConvertTo-Json` de un elemento suelto devuelve un objeto y no una lista,
    // y entonces el `serde_json` de este lado no encuentra el array.
    const GUION: &str = r#"
$cs   = Get-CimInstance Win32_ComputerSystem | Select-Object -First 1
$bios = Get-CimInstance Win32_BIOS | Select-Object -First 1
$csp  = Get-CimInstance Win32_ComputerSystemProduct | Select-Object -First 1
$serie = $bios.SerialNumber
if (-not $serie) { $serie = $csp.IdentifyingNumber }
[PSCustomObject]@{
  fabricante = [string]$cs.Manufacturer
  modelo     = [string]$cs.Model
  serie      = [string]$serie
  sockets    = [int]$cs.NumberOfProcessors
  gpus       = @(Get-CimInstance Win32_VideoController | ForEach-Object { [string]$_.Name })
} | ConvertTo-Json -Compress
"#;
    let salida = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", GUION])
        .output()
        .map_err(|e| format!("no se pudo lanzar PowerShell: {e}"))?;
    if !salida.status.success() {
        return Err(format!(
            "WMI no contestó: {}",
            String::from_utf8_lossy(&salida.stderr).trim()
        ));
    }
    let v: serde_json::Value = serde_json::from_slice(&salida.stdout)
        .map_err(|e| format!("WMI devolvió algo que no es JSON: {e}"))?;
    Ok(de_json(&v))
}

#[cfg(not(windows))]
pub fn sonda() -> Result<Hardware, String> {
    Err("solo en Windows".into())
}

/// La parte pura, para poder discutirla sin WMI ni PowerShell.
///
/// APARTE A PROPÓSITO. Lo que llega de `ConvertTo-Json` tiene formas que no se
/// adivinan desde el guion —una máquina virtual sin gráficas devuelve `null` en
/// vez de una lista vacía, y `sockets` puede venir como cadena— y son
/// exactamente los casos que hacen falta fijar en un test.
pub fn de_json(v: &serde_json::Value) -> Hardware {
    let texto = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    // «System manufacturer» y «To Be Filled By O.E.M.» son los rellenos que
    // pone una placa sin personalizar. Enseñarlos es peor que no enseñar nada:
    // parecen un dato.
    let vacio = |s: &str| {
        let b = s.to_lowercase();
        s.is_empty()
            || b.starts_with("system ")
            || b.contains("to be filled")
            || b.contains("o.e.m.")
            || b == "default string"
            || b == "none"
    };
    let fab = texto("fabricante");
    let mod_ = texto("modelo");
    let ser = texto("serie");
    Hardware {
        fabricante: if vacio(&fab) { String::new() } else { marca_equipo(&fab) },
        modelo: if vacio(&mod_) { String::new() } else { mod_ },
        serie: if vacio(&ser) { String::new() } else { ser },
        gpus: v
            .get("gpus")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        // `as_u64` no vale solo: `ConvertTo-Json` a veces manda el número entre
        // comillas, y ahí un `unwrap_or(0)` dejaría los zócalos en cero sin que
        // nada fallara.
        sockets: v
            .get("sockets")
            .and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0) as usize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn el_fabricante_pierde_la_razon_social() {
        // Los cuatro que se han visto de verdad en máquinas de administrador.
        assert_eq!(marca_equipo("Micro-Star International Co., Ltd."), "MSI");
        assert_eq!(marca_equipo("Hewlett-Packard"), "HP");
        assert_eq!(marca_equipo("LENOVO"), "Lenovo");
        assert_eq!(marca_equipo("Dell Inc."), "Dell");
    }

    #[test]
    fn un_fabricante_desconocido_sale_con_su_nombre() {
        // Se prefiere un nombre largo pero cierto a uno corto e inventado. Lo
        // único que se le quita es la forma societaria.
        assert_eq!(marca_equipo("Contoso Manufacturing, Ltd."), "Contoso Manufacturing");
        assert_eq!(marca_equipo("Tyan"), "Tyan");
        assert_eq!(marca_equipo(""), "");
    }

    #[test]
    fn una_maquina_virtual_se_reconoce() {
        // Saber que se está mirando una VM cambia la lectura de todo lo demás.
        assert_eq!(marca_equipo("VMware, Inc."), "VMware");
        assert_eq!(marca_equipo("innotek GmbH"), "VirtualBox");
        assert_eq!(marca_equipo("QEMU"), "QEMU");
    }

    #[test]
    fn el_numero_de_serie_se_ensena_por_las_puntas() {
        // El de la máquina donde se midió esto.
        assert_eq!(enmascara("9S715PL21071ZS8000058"), "9S71····0058");
    }

    #[test]
    fn un_serie_corto_no_se_enmascara() {
        // Enmascarar «ABC123» dejaría algo que no identifica nada y encima
        // parece un fallo de la aplicación.
        assert_eq!(enmascara("ABC123"), "ABC123");
        assert_eq!(enmascara("123456789"), "123456789");
        assert_eq!(enmascara(""), "");
        // Justo por encima del umbral sí se tapa.
        assert_eq!(enmascara("1234567890"), "1234····7890");
    }

    #[test]
    fn se_leen_todas_las_graficas_y_no_la_primera() {
        // ESTE TEST ES LA RAZÓN DE SER DEL PORTE. La V1 hacía
        // `Select-Object -First 1`, y en la máquina medida hay dos: si WMI
        // hubiera devuelto la integrada primero, el dashboard habría dicho
        // «Radeon 610M» en un equipo con una RTX 5070.
        let h = de_json(&json!({
            "fabricante": "Micro-Star International Co., Ltd.",
            "modelo": "Crosshair A16 HX D8WGKG",
            "serie": "9S715PL21071ZS8000058",
            "sockets": 1,
            "gpus": ["NVIDIA GeForce RTX 5070 Laptop GPU", "AMD Radeon(TM) 610M"]
        }));
        assert_eq!(h.gpus.len(), 2);
        assert_eq!(h.fabricante, "MSI");
        assert_eq!(h.equipo(), "MSI Crosshair A16 HX D8WGKG");
        assert!(h.hay_algo());
    }

    #[test]
    fn una_placa_sin_personalizar_no_finge_tener_datos() {
        // Lo que pone una placa genérica. Enseñarlo es peor que no enseñar nada:
        // parece un dato y no lo es.
        let h = de_json(&json!({
            "fabricante": "System manufacturer",
            "modelo": "To Be Filled By O.E.M.",
            "serie": "Default string",
            "sockets": 2,
            "gpus": []
        }));
        assert!(h.fabricante.is_empty());
        assert!(h.modelo.is_empty());
        assert!(h.serie.is_empty());
        assert!(!h.hay_algo(), "sin nada que decir, no hay sección");
        // Pero los zócalos SÍ se leyeron: son un dato aunque el resto sea relleno.
        assert_eq!(h.sockets, 2);
    }

    #[test]
    fn lo_que_falta_no_deja_huecos_ni_ceros_falsos() {
        // Una VM sin gráficas devuelve `null` donde debería ir la lista, y
        // `ConvertTo-Json` a veces manda los números entre comillas.
        let h = de_json(&json!({ "fabricante": "QEMU", "sockets": "4" }));
        assert_eq!(h.fabricante, "QEMU");
        assert_eq!(h.equipo(), "QEMU", "sin modelo, sin espacio colgando");
        assert!(h.gpus.is_empty());
        assert_eq!(h.sockets, 4, "el número entre comillas se leyó igual");
        // Y un objeto vacío del todo no revienta.
        let nada = de_json(&json!({}));
        assert!(!nada.hay_algo());
        assert_eq!(nada.sockets, 0);
    }

    /// Lo que WMI dice de ESTA máquina. Solo lectura.
    ///
    /// `cargo test -p lucy-core --lib hardware::tests::que_dice_wmi_de_aqui -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn que_dice_wmi_de_aqui() {
        match sonda() {
            Ok(h) => {
                println!("\n  equipo   {}", h.equipo());
                println!("  serie    {} (entero: {})", enmascara(&h.serie), h.serie);
                println!("  zócalos  {}", h.sockets);
                for g in &h.gpus {
                    println!("  gráfica  {g}");
                }
                println!();
            }
            Err(e) => println!("\n  falló: {e}\n"),
        }
    }
}
