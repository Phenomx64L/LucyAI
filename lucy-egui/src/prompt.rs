//! Quién es Lucy y en qué equipo está.
//!
//! EL FALLO QUE ESTO ARREGLA. El shell nativo mandaba al modelo el texto del
//! operador y nada más. Sin contexto, un modelo de nube contesta lo único que
//! puede contestar: "como soy una inteligencia artificial no tengo acceso a tu
//! computadora" — y tiene razón, porque nadie le había dicho lo contrario.
//! Lucy no había perdido ninguna capacidad: nunca se le había contado que las
//! tenía.
//!
//! LO QUE VA EN EL PROMPT SON HECHOS, NO PERMISOS. Aquí no se le concede al
//! modelo ningún poder nuevo: se le dan las lecturas que este shell YA hace para
//! el Dashboard —el equipo, el sistema, la carga, los discos, los servicios
//! caídos, los errores recientes del log— para que pueda responder sobre esta
//! máquina en lugar de pedirle al operador que copie y pegue. "Resume los
//! errores recientes" pasa de una explicación de cómo hacerlo a mano a una
//! respuesta.
//!
//! Y SE LE DICE LA VERDAD SOBRE LO QUE NO PUEDE HACER. Un `<EXECUTE>` en este
//! shell no se ejecuta: se enseña en el panel de Plan como propuesta. Callarlo
//! haría que el modelo escribiera "ya lo he ejecutado" sobre una máquina que
//! nadie tocó, que es peor que no poder ejecutar.
//!
//! Esto NO es el prompt componible de la V2 (`prompt_sections.rs`): allí hay
//! memorias, skills, enrutado por host y una docena de secciones más. Es el
//! suelo mínimo para que Lucy sea Lucy, y queda dicho para que nadie lo
//! confunda con el port completo.

use std::fmt::Write;

/// Cuántas líneas de log se le pasan al modelo.
///
/// Veinte cabe de sobra en cualquier ventana de contexto y sigue siendo
/// suficiente para ver un patrón. Pasarle el fichero entero gasta tokens en
/// líneas idénticas repetidas mil veces.
const LOG_LINES: usize = 20;

/// Construye el prompt de sistema con el estado real del equipo.
pub fn system_prompt(
    s: &lucy_core::system::SysSnapshot,
    services: &[lucy_core::system::DownService],
    log: &[String],
) -> String {
    let mut p = String::new();
    p.push_str(
        "Eres Lucy, una asistente de administración de sistemas Windows. Hablas en \
         español, vas al grano y no adornas.\n\n\
         NO eres un modelo genérico sin acceso: estás integrada en una aplicación de \
         escritorio que corre EN el equipo del operador y que te entrega sus lecturas \
         reales. Los datos de abajo vienen de esta máquina, medidos hace segundos. \
         Úsalos. No pidas al operador que ejecute comandos para averiguar algo que ya \
         tienes delante, y no digas que no tienes acceso a su equipo.\n\n",
    );

    let _ = writeln!(p, "--- EQUIPO ---");
    let _ = writeln!(p, "Nombre: {}", s.host);
    let _ = writeln!(p, "Sistema: {} · kernel {}", s.os, s.kernel);
    let _ = writeln!(p, "CPU: {} · {} núcleos · {:.0}% de uso", s.cpu_brand, s.cores, s.cpu_pct);
    let _ = writeln!(
        p,
        "RAM: {:.1} GB usados de {:.1} GB",
        s.mem_used as f64 / 1e9,
        s.mem_total as f64 / 1e9
    );
    for d in &s.disks {
        let pct = if d.total > 0 {
            d.total.saturating_sub(d.avail) as f64 / d.total as f64 * 100.0
        } else {
            0.0
        };
        let _ = writeln!(
            p,
            "Disco {}: {:.0}% ocupado, {:.1} GB libres de {:.1} GB",
            d.mount,
            pct,
            d.avail as f64 / 1e9,
            d.total as f64 / 1e9
        );
    }
    let _ = writeln!(p, "Uptime: {} segundos", s.uptime_secs);

    // Los servicios y el log solo aparecen si hay algo que contar. Una sección
    // que dice "ninguno" cada turno gasta tokens en decir que no pasa nada, y
    // enseña al modelo a saltarse ese bloque el día que sí traiga algo.
    if !services.is_empty() {
        let _ = writeln!(p, "\n--- SERVICIOS AUTOMÁTICOS DETENIDOS ---");
        for sv in services {
            let _ = writeln!(
                p,
                "{} — {}",
                sv.name,
                if sv.crashed() {
                    format!("FALLÓ (código {})", sv.exit_code)
                } else {
                    "detenido limpiamente".to_string()
                }
            );
        }
    }

    if !log.is_empty() {
        let _ = writeln!(
            p,
            "\n--- ÚLTIMAS LÍNEAS DEL LOG DE LUCY (lucy_app.log) ---"
        );
        for l in log.iter().rev().take(LOG_LINES).rev() {
            let _ = writeln!(p, "{l}");
        }
        // Que el modelo sepa QUÉ log es. Sin esta línea confunde el log de la
        // aplicación con el registro de eventos de Windows y responde sobre el
        // que no es.
        let _ = writeln!(
            p,
            "(Es el log de la propia aplicación Lucy, NO el registro de eventos de Windows.)"
        );
    }

    p.push_str(
        "\n--- CÓMO PEDIR ACCIONES ---\n\
         Para proponer un comando de PowerShell, enciérralo en <EXECUTE>…</EXECUTE>.\n\
         IMPORTANTE: en esta versión del shell los comandos NO se ejecutan solos. \
         Aparecen en el panel de Plan como pasos PENDIENTES para que el operador los \
         revise. Así que propón el comando y explica qué hace, pero NUNCA digas que ya \
         lo ejecutaste ni des por hecho su salida.\n\n\
         Si necesitas razonar antes de responder, hazlo dentro de <THOUGHT>…</THOUGHT>: \
         se guarda aparte y no ensucia la respuesta.\n",
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucy_core::system::{DiskInfo, DownService, SysSnapshot};

    fn snap() -> SysSnapshot {
        SysSnapshot {
            host: "WORKSTATION-16".into(),
            os: "Windows 11 Pro".into(),
            kernel: "26200".into(),
            cpu_brand: "Intel i9".into(),
            cpu_pct: 7.0,
            per_core: vec![],
            mem_used: 10_000_000_000,
            mem_total: 31_200_000_000,
            swap_used: 0,
            swap_total: 0,
            uptime_secs: 3600,
            cores: 32,
            disks: vec![DiskInfo {
                name: "Local Disk".into(),
                mount: "C:\\".into(),
                total: 1_000_000_000_000,
                avail: 710_000_000_000,
            }],
        }
    }

    #[test]
    fn el_prompt_lleva_los_datos_reales_del_equipo() {
        // Sin esto, el modelo contesta "no tengo acceso a tu computadora" — que
        // es exactamente lo que pasaba, y no era culpa suya.
        let p = system_prompt(&snap(), &[], &[]);
        assert!(p.contains("WORKSTATION-16"));
        assert!(p.contains("Windows 11 Pro"));
        assert!(p.contains("32 núcleos"));
        assert!(p.contains("C:\\"));
    }

    #[test]
    fn le_dice_explicitamente_que_no_niegue_el_acceso() {
        let p = system_prompt(&snap(), &[], &[]);
        assert!(p.contains("no digas que no tienes acceso"));
    }

    #[test]
    fn avisa_de_que_los_comandos_no_se_ejecutan() {
        // Callarlo haría que el modelo escribiera "ya lo he ejecutado" sobre una
        // máquina que nadie tocó — peor que no poder ejecutar.
        let p = system_prompt(&snap(), &[], &[]);
        assert!(p.contains("NO se ejecutan"));
        assert!(p.contains("NUNCA digas que ya"));
    }

    #[test]
    fn las_secciones_vacias_no_aparecen() {
        // Una sección que dice "ninguno" en cada turno gasta tokens en decir que
        // no pasa nada, y enseña al modelo a saltarse ese bloque.
        let p = system_prompt(&snap(), &[], &[]);
        assert!(!p.contains("SERVICIOS AUTOMÁTICOS DETENIDOS"));
        assert!(!p.contains("LOG DE LUCY"));

        let svc = [DownService { name: "gpsvc".into(), exit_code: 1 }];
        let p2 = system_prompt(&snap(), &svc, &["ERROR algo".into()]);
        assert!(p2.contains("SERVICIOS AUTOMÁTICOS DETENIDOS"));
        assert!(p2.contains("gpsvc — FALLÓ (código 1)"));
        assert!(p2.contains("ERROR algo"));
    }

    #[test]
    fn del_log_van_las_ultimas_lineas_y_en_orden() {
        // Las ÚLTIMAS, porque un incidente se lee por el final; y en su orden
        // original, porque un log al revés se interpreta al revés.
        let log: Vec<String> = (0..50).map(|i| format!("linea {i}")).collect();
        let p = system_prompt(&snap(), &[], &log);
        assert!(p.contains("linea 49"));
        assert!(!p.contains("linea 29"), "solo las últimas {LOG_LINES}");
        let a = p.find("linea 30").unwrap();
        let b = p.find("linea 49").unwrap();
        assert!(a < b, "el log va en su orden, no del revés");
    }
}
