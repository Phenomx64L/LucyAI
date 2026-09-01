//! Qué ha cambiado en un equipo desde que dijimos que estaba bien.
//!
//! El inventario contesta «qué hay». Esto contesta la pregunta que de verdad se
//! hace un administrador delante de un servidor que se porta raro: «¿qué es
//! distinto de ayer?». Se fija una línea base —una foto que se declara buena— y
//! a partir de ahí cada escaneo se compara con ella.
//!
//! LA MISMA TABLA Y LA MISMA FORMA QUE LA APP TAURI, `inventory_baselines`. No
//! por nostalgia: si el shell nativo escribiera ahí una forma que la app no sabe
//! leer, la app no daría error — sus `as_array(v,"software")` devolverían listas
//! vacías y su informe diría que se ha desinstalado TODO. Un formato incompatible
//! en una tabla compartida no rompe, miente. Por eso el envoltorio de este módulo
//! escribe las claves de la app (`scheduled` y no `tasks`, `expires` además de
//! `expires_epoch`) y añade las suyas al lado, que la otra mitad ignora sin
//! enterarse.
//!
//! LOS PUERTOS EFÍMEROS NO CUENTAN, y esa es la decisión que separa un informe de
//! un montón de ruido. Un escaneo real de esta máquina devolvió 49670, 49668 y
//! 49667: puertos dinámicos que Windows reparte en cada arranque y que son
//! distintos mañana sin que nadie haya tocado nada. Compararlos llenaría la lista
//! de «puerto nuevo» y «puerto que ya no está» en cada vuelta, y lo que de verdad
//! importa —que el 3389 se ha abierto— se perdería entre treinta filas de humo.

use crate::inventory::{Categoria, Cert, Inventory, Port, Service, Software, Task};

/// Desde dónde empieza el rango dinámico.
///
/// 49152 es el que usan Windows y el estándar (IANA) para los puertos efímeros.
/// Linux reparte desde 32768 por defecto, pero bajar el corte hasta ahí escondería
/// cosas que sí importan —un PostgreSQL en el 32800 es un servicio de verdad—, así
/// que se coge el mayor de los dos y se acepta perder algún falso positivo por
/// debajo. Esconder de más es peor que enseñar de más.
pub const EFIMERO_DESDE: u32 = 49152;

/// Si un puerto lo reparte el sistema en vez de abrirlo un servicio.
pub fn es_efimero(p: u32) -> bool {
    p >= EFIMERO_DESDE
}

/// Qué le pasó a una cosa entre la línea base y ahora.
#[derive(Debug, Clone, PartialEq)]
pub enum Cambio {
    Apareció,
    Desapareció,
    /// Sigue estando, pero algo suyo es distinto.
    Cambió {
        campo: &'static str,
        de: String,
        a: String,
    },
}

impl Cambio {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Apareció => "nuevo",
            Self::Desapareció => "ya no está",
            Self::Cambió { .. } => "cambió",
        }
    }
}

/// Una diferencia concreta.
#[derive(Debug, Clone, PartialEq)]
pub struct Fila {
    pub cat: Categoria,
    /// Cómo se llama la cosa: el nombre del servicio, el número del puerto, el
    /// asunto del certificado. Es la columna por la que el operador la busca.
    pub id: String,
    /// Lo que la acompaña, para no tener que ir al inventario a mirarlo: la
    /// versión del paquete, el proceso que abrió el puerto.
    pub detalle: String,
    pub cambio: Cambio,
}

/// El informe completo.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub filas: Vec<Fila>,
    /// Cuántos segundos hace que se fijó la línea base.
    pub edad_secs: i64,
    pub label: String,
    /// Puertos efímeros que se han ignorado a propósito, para poder decirlo.
    ///
    /// Se cuenta en vez de callarlo: un informe que dice «sin cambios» habiendo
    /// descartado cuarenta filas por su cuenta tiene que poder explicarse, o la
    /// próxima vez que alguien eche en falta un puerto sospechará del programa.
    pub efimeros_ignorados: usize,
}

impl Report {
    pub fn is_empty(&self) -> bool {
        self.filas.is_empty()
    }

    pub fn cuenta(&self, cat: Categoria) -> usize {
        self.filas.iter().filter(|f| f.cat == cat).count()
    }
}

/// Compara dos fotos. Es el corazón del módulo y no toca nada de fuera.
///
/// Puro a propósito: comparar inventarios es donde están las decisiones
/// discutibles —qué cuenta como la misma cosa, qué campo importa— y eso tiene que
/// poder probarse sin una base de datos ni un servidor delante.
pub fn compare(base: &Inventory, ahora: &Inventory) -> Report {
    let mut r = Report::default();

    // ── Puertos, por número ──
    //
    // Los efímeros se descartan de los DOS lados antes de comparar. Descartarlos
    // solo de uno haría que todos parecieran desaparecidos.
    let b_ports: Vec<&Port> = base.ports.iter().filter(|p| !es_efimero(p.port)).collect();
    let a_ports: Vec<&Port> = ahora.ports.iter().filter(|p| !es_efimero(p.port)).collect();
    r.efimeros_ignorados = (base.ports.len() - b_ports.len()) + (ahora.ports.len() - a_ports.len());
    for p in &a_ports {
        match b_ports.iter().find(|x| x.port == p.port) {
            None => r.filas.push(Fila {
                cat: Categoria::Puertos,
                id: p.port.to_string(),
                detalle: p.process.clone(),
                cambio: Cambio::Apareció,
            }),
            Some(b) if b.process != p.process => r.filas.push(Fila {
                cat: Categoria::Puertos,
                id: p.port.to_string(),
                detalle: p.process.clone(),
                // Un puerto que sigue abierto pero lo tiene OTRO proceso es la
                // señal más fea de todo el informe: el mismo número, otra cosa
                // detrás.
                cambio: Cambio::Cambió {
                    campo: "proceso",
                    de: b.process.clone(),
                    a: p.process.clone(),
                },
            }),
            Some(_) => {}
        }
    }
    for b in &b_ports {
        if !a_ports.iter().any(|x| x.port == b.port) {
            r.filas.push(Fila {
                cat: Categoria::Puertos,
                id: b.port.to_string(),
                detalle: b.process.clone(),
                cambio: Cambio::Desapareció,
            });
        }
    }

    // ── Servicios, por nombre ──
    diff(
        &base.services,
        &ahora.services,
        Categoria::Servicios,
        |s: &Service| s.name.to_lowercase(),
        |s: &Service| s.name.clone(),
        |s: &Service| s.description.clone(),
        // EL ESTADO ES LO QUE SE MIRA. Un servicio que estaba corriendo y ahora
        // no es la fila por la que existe todo este módulo.
        |b: &Service, a: &Service| {
            (b.status != a.status).then(|| ("estado", b.status.clone(), a.status.clone()))
        },
        &mut r.filas,
    );

    // ── Software, por nombre ──
    diff(
        &base.software,
        &ahora.software,
        Categoria::Software,
        |s: &Software| s.name.to_lowercase(),
        |s: &Software| s.name.clone(),
        |s: &Software| s.version.clone(),
        |b: &Software, a: &Software| {
            (b.version != a.version).then(|| ("versión", b.version.clone(), a.version.clone()))
        },
        &mut r.filas,
    );

    // ── Certificados, por asunto ──
    diff(
        &base.certs,
        &ahora.certs,
        Categoria::Certificados,
        |c: &Cert| c.subject.to_lowercase(),
        |c: &Cert| c.subject.clone(),
        |c: &Cert| c.path.clone(),
        // Que cambie la fecha de caducidad casi siempre es una RENOVACIÓN, que es
        // justo lo que se quiere ver confirmado. Se compara el epoch y no el
        // texto: dos formatos distintos de la misma fecha no son un cambio.
        |b: &Cert, a: &Cert| {
            (b.expires_epoch != a.expires_epoch).then(|| {
                ("caducidad", fecha(b.expires_epoch), fecha(a.expires_epoch))
            })
        },
        &mut r.filas,
    );

    // ── Tareas, por su texto ──
    diff(
        &base.tasks,
        &ahora.tasks,
        Categoria::Tareas,
        |t: &Task| t.entry.to_lowercase(),
        |t: &Task| t.entry.clone(),
        |t: &Task| t.state.clone(),
        |b: &Task, a: &Task| {
            (b.state != a.state).then(|| ("estado", b.state.clone(), a.state.clone()))
        },
        &mut r.filas,
    );

    r
}

/// La comparación genérica: qué apareció, qué desapareció, qué cambió.
///
/// La CLAVE y la ETIQUETA son dos funciones distintas a propósito. La clave va en
/// minúsculas para que un fabricante que cambie `NVIDIA` por `Nvidia` en su
/// instalador no aparezca como un paquete desinstalado y otro instalado; la
/// etiqueta conserva las mayúsculas porque es lo que se enseña.
#[allow(clippy::too_many_arguments)]
fn diff<T>(
    base: &[T],
    ahora: &[T],
    cat: Categoria,
    clave: impl Fn(&T) -> String,
    etiqueta: impl Fn(&T) -> String,
    detalle: impl Fn(&T) -> String,
    cambio: impl Fn(&T, &T) -> Option<(&'static str, String, String)>,
    out: &mut Vec<Fila>,
) {
    use std::collections::HashMap;
    // El PRIMERO de cada clave, no el último: una lista de software puede traer
    // el mismo nombre dos veces (una instalación de 32 y otra de 64 bits), y
    // quedarse con uno cualquiera al menos es estable entre escaneos.
    let mut b_por: HashMap<String, &T> = HashMap::new();
    for x in base {
        let k = clave(x);
        if !k.is_empty() {
            b_por.entry(k).or_insert(x);
        }
    }
    let mut a_por: HashMap<String, &T> = HashMap::new();
    for x in ahora {
        let k = clave(x);
        if !k.is_empty() {
            a_por.entry(k).or_insert(x);
        }
    }
    for (k, a) in &a_por {
        match b_por.get(k) {
            None => out.push(Fila {
                cat,
                id: etiqueta(a),
                detalle: detalle(a),
                cambio: Cambio::Apareció,
            }),
            Some(b) => {
                if let Some((campo, de, hasta)) = cambio(b, a) {
                    out.push(Fila {
                        cat,
                        id: etiqueta(a),
                        detalle: detalle(a),
                        cambio: Cambio::Cambió { campo, de, a: hasta },
                    });
                }
            }
        }
    }
    for (k, b) in &b_por {
        if !a_por.contains_key(k) {
            out.push(Fila {
                cat,
                id: etiqueta(b),
                detalle: detalle(b),
                cambio: Cambio::Desapareció,
            });
        }
    }
}

fn fecha(ep: Option<i64>) -> String {
    match ep {
        None => "desconocida".into(),
        Some(e) => {
            let (a, m, d) = crate::audit::civil_de_dias(e.div_euclid(86_400));
            format!("{a:04}-{m:02}-{d:02}")
        }
    }
}

// ── Almacén ─────────────────────────────────────────────────────────────────

/// La foto que se declaró buena, con cuándo se declaró.
#[derive(Debug, Clone, PartialEq)]
pub struct Baseline {
    pub host_id: String,
    pub label: String,
    pub inv: Inventory,
    pub updated_at: i64,
}

/// Crea la tabla si no está. Mismo DDL que la app, por lo mismo que en `audit`.
pub fn ensure_schema() -> Result<(), String> {
    crate::with_db(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS inventory_baselines (
                 host_id        TEXT    PRIMARY KEY,
                 label          TEXT    NOT NULL DEFAULT '',
                 snapshot_json  TEXT    NOT NULL,
                 created_at     INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                 updated_at     INTEGER NOT NULL DEFAULT (strftime('%s','now'))
             );
             CREATE INDEX IF NOT EXISTS idx_inv_baseline_updated
                 ON inventory_baselines(updated_at DESC);",
        )
        .map_err(|e| format!("drift: no se pudo crear el esquema: {e}"))
    })
}

/// Fija —o reemplaza— la línea base de un equipo.
pub fn set_baseline(host_id: &str, label: &str, inv: &Inventory) -> Result<(), String> {
    ensure_schema()?;
    let json = a_json(inv);
    crate::with_db(|c| {
        c.execute(
            "INSERT INTO inventory_baselines (host_id, label, snapshot_json, updated_at)
             VALUES (?1, ?2, ?3, strftime('%s','now'))
             ON CONFLICT(host_id) DO UPDATE SET
                 label = excluded.label,
                 snapshot_json = excluded.snapshot_json,
                 updated_at = excluded.updated_at",
            rusqlite::params![host_id, label, json],
        )
        .map_err(|e| format!("drift: no se pudo guardar la línea base: {e}"))?;
        Ok(())
    })
}

pub fn get_baseline(host_id: &str) -> Result<Option<Baseline>, String> {
    ensure_schema()?;
    crate::with_db(|c| {
        let fila = c
            .query_row(
                "SELECT label, snapshot_json, updated_at FROM inventory_baselines
                 WHERE host_id = ?1",
                rusqlite::params![host_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .ok();
        Ok(fila.map(|(label, json, updated_at)| Baseline {
            host_id: host_id.to_string(),
            label,
            inv: de_json(&json),
            updated_at,
        }))
    })
}

pub fn delete_baseline(host_id: &str) -> Result<(), String> {
    ensure_schema()?;
    crate::with_db(|c| {
        c.execute(
            "DELETE FROM inventory_baselines WHERE host_id = ?1",
            rusqlite::params![host_id],
        )
        .map_err(|e| format!("drift: no se pudo borrar la línea base: {e}"))?;
        Ok(())
    })
}

// ── El formato en disco ─────────────────────────────────────────────────────
//
// A MANO Y NO CON `derive`, y merece explicarse. Este JSON lo lee también la app
// Tauri, así que las claves son las SUYAS: `scheduled` donde aquí decimos
// `tasks`, y `expires` en texto además del epoch. Un `derive` sobre las structs
// de `inventory` produciría los nombres de aquí, la app leería listas vacías, y
// su informe diría que se ha desinstalado todo el software del servidor. Un
// formato incompatible en una tabla compartida no rompe: miente.
//
// Los campos que la app no conoce —el `state` de una tarea— van igualmente: JSON
// ignora lo que no espera, así que añadir no cuesta nada y quitar sí.

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

fn a_json(inv: &Inventory) -> String {
    let mut s = String::from("{");
    s.push_str("\"ports\":[");
    for (i, p) in inv.ports.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"port\":{},\"process\":\"{}\",\"state\":\"LISTEN\"}}",
            p.port,
            esc(&p.process)
        ));
    }
    s.push_str("],\"services\":[");
    for (i, x) in inv.services.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"name\":\"{}\",\"status\":\"{}\",\"description\":\"{}\"}}",
            esc(&x.name),
            esc(&x.status),
            esc(&x.description)
        ));
    }
    s.push_str("],\"software\":[");
    for (i, x) in inv.software.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"name\":\"{}\",\"version\":\"{}\"}}",
            esc(&x.name),
            esc(&x.version)
        ));
    }
    s.push_str("],\"certs\":[");
    for (i, x) in inv.certs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"path\":\"{}\",\"subject\":\"{}\",\"expires\":\"{}\",\"expires_epoch\":{}}}",
            esc(&x.path),
            esc(&x.subject),
            esc(&fecha(x.expires_epoch)),
            x.expires_epoch.map(|e| e.to_string()).unwrap_or_else(|| "null".into())
        ));
    }
    // `scheduled` es la clave de la app. Aquí la lista se llama `tasks`, y
    // escribir ese nombre haría que la app viera cero tareas.
    s.push_str("],\"scheduled\":[");
    for (i, x) in inv.tasks.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"entry\":\"{}\",\"state\":\"{}\"}}",
            esc(&x.entry),
            esc(&x.state)
        ));
    }
    s.push_str("]}");
    s
}

fn de_json(json: &str) -> Inventory {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::Value::Null);
    let arr = |k: &str| -> Vec<serde_json::Value> {
        v.get(k).and_then(|x| x.as_array()).cloned().unwrap_or_default()
    };
    let txt = |o: &serde_json::Value, k: &str| -> String {
        o.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
    };
    Inventory {
        ports: arr("ports")
            .iter()
            .filter_map(|o| {
                Some(Port {
                    port: o.get("port")?.as_u64()? as u32,
                    process: txt(o, "process"),
                })
            })
            .collect(),
        services: arr("services")
            .iter()
            .map(|o| Service {
                name: txt(o, "name"),
                status: txt(o, "status"),
                description: txt(o, "description"),
            })
            .collect(),
        software: arr("software")
            .iter()
            .map(|o| Software { name: txt(o, "name"), version: txt(o, "version") })
            .collect(),
        certs: arr("certs")
            .iter()
            .map(|o| Cert {
                path: txt(o, "path"),
                subject: txt(o, "subject"),
                expires_epoch: o.get("expires_epoch").and_then(|x| x.as_i64()),
            })
            .collect(),
        tasks: arr("scheduled")
            .iter()
            .map(|o| Task { entry: txt(o, "entry"), state: txt(o, "state") })
            .collect(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inv(
        ports: &[(u32, &str)],
        svcs: &[(&str, &str)],
        sw: &[(&str, &str)],
        tasks: &[(&str, &str)],
    ) -> Inventory {
        Inventory {
            ports: ports
                .iter()
                .map(|(p, pr)| Port { port: *p, process: (*pr).into() })
                .collect(),
            services: svcs
                .iter()
                .map(|(n, s)| Service {
                    name: (*n).into(),
                    status: (*s).into(),
                    description: String::new(),
                })
                .collect(),
            software: sw
                .iter()
                .map(|(n, v)| Software { name: (*n).into(), version: (*v).into() })
                .collect(),
            tasks: tasks
                .iter()
                .map(|(e, s)| Task { entry: (*e).into(), state: (*s).into() })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn los_puertos_efimeros_no_ensucian_el_informe() {
        // LA DECISIÓN QUE HACE QUE ESTO SIRVA. Un escaneo real de esta máquina
        // devolvió 49670, 49668 y 49667: puertos que Windows reparte en cada
        // arranque. Compararlos llenaría la lista de «nuevo» y «ya no está» cada
        // vuelta, y que se haya abierto el 3389 se perdería entre el humo.
        let base = inv(&[(443, "nginx"), (49670, "svchost")], &[], &[], &[]);
        let ahora = inv(&[(443, "nginx"), (49999, "svchost"), (3389, "TermService")], &[], &[], &[]);
        let r = compare(&base, &ahora);
        assert_eq!(r.filas.len(), 1, "{:?}", r.filas);
        assert_eq!(r.filas[0].id, "3389");
        assert_eq!(r.filas[0].cambio, Cambio::Apareció);
        // Y se dice cuántos se ignoraron: un «sin cambios» que descartó cuarenta
        // filas por su cuenta tiene que poder explicarse.
        assert_eq!(r.efimeros_ignorados, 2);
    }

    #[test]
    fn un_servicio_que_se_cayo_es_la_fila_que_importa() {
        let base = inv(&[], &[("nginx", "running"), ("sshd", "running")], &[], &[]);
        let ahora = inv(&[], &[("nginx", "stopped"), ("sshd", "running")], &[], &[]);
        let r = compare(&base, &ahora);
        assert_eq!(r.filas.len(), 1);
        assert_eq!(r.filas[0].id, "nginx");
        assert_eq!(
            r.filas[0].cambio,
            Cambio::Cambió { campo: "estado", de: "running".into(), a: "stopped".into() }
        );
    }

    #[test]
    fn un_puerto_que_cambia_de_dueno_se_ve() {
        // El mismo número con otra cosa detrás es la señal más fea del informe, y
        // sin comparar el proceso pasaría por «sin cambios».
        let base = inv(&[(8080, "llama-server")], &[], &[], &[]);
        let ahora = inv(&[(8080, "python")], &[], &[], &[]);
        let r = compare(&base, &ahora);
        assert_eq!(r.filas.len(), 1);
        assert!(matches!(r.filas[0].cambio, Cambio::Cambió { campo: "proceso", .. }));
    }

    #[test]
    fn cambiar_las_mayusculas_de_un_paquete_no_es_instalar_y_desinstalar() {
        // Un fabricante que pase de `NVIDIA` a `Nvidia` en su instalador
        // produciría dos filas —una desinstalación y una instalación— sobre un
        // equipo donde no ha pasado nada.
        let base = inv(&[], &[], &[("NVIDIA Driver", "1.0")], &[]);
        let ahora = inv(&[], &[], &[("Nvidia Driver", "1.0")], &[]);
        assert!(compare(&base, &ahora).is_empty());

        // Pero la versión sí se mira, y la etiqueta que se enseña conserva las
        // mayúsculas de AHORA, no las de la línea base.
        let nuevo = inv(&[], &[], &[("Nvidia Driver", "2.0")], &[]);
        let r = compare(&base, &nuevo);
        assert_eq!(r.filas[0].id, "Nvidia Driver");
        assert!(matches!(r.filas[0].cambio, Cambio::Cambió { campo: "versión", .. }));
    }

    #[test]
    fn dos_fotos_iguales_no_dan_ninguna_fila() {
        // Lo primero que tiene que hacer: no inventarse cambios. Un informe con
        // ruido de fondo no se lee nunca.
        let a = inv(
            &[(443, "nginx")],
            &[("sshd", "running")],
            &[("git", "2.45")],
            &[("\\Backup", "Ready")],
        );
        assert!(compare(&a, &a.clone()).is_empty());
    }

    #[test]
    fn una_tarea_deshabilitada_se_distingue_de_una_borrada() {
        // Son dos cosas muy distintas y sin el estado se veían igual: la V2 solo
        // listaba las `Ready`, así que deshabilitar una se leía como borrarla.
        let base = inv(&[], &[], &[], &[("\\Backup", "Ready")]);
        let apagada = inv(&[], &[], &[], &[("\\Backup", "Disabled")]);
        let borrada = inv(&[], &[], &[], &[]);
        assert!(matches!(
            compare(&base, &apagada).filas[0].cambio,
            Cambio::Cambió { campo: "estado", .. }
        ));
        assert_eq!(compare(&base, &borrada).filas[0].cambio, Cambio::Desapareció);
    }

    #[test]
    fn un_certificado_renovado_se_ve_con_las_dos_fechas() {
        let base = Inventory {
            certs: vec![Cert {
                path: "/a.pem".into(),
                subject: "CN=api".into(),
                expires_epoch: Some(1_786_060_800),
            }],
            ..Default::default()
        };
        let ahora = Inventory {
            certs: vec![Cert {
                path: "/a.pem".into(),
                subject: "CN=api".into(),
                expires_epoch: Some(1_817_596_800),
            }],
            ..Default::default()
        };
        let r = compare(&base, &ahora);
        match &r.filas[0].cambio {
            Cambio::Cambió { campo, de, a } => {
                assert_eq!(*campo, "caducidad");
                // 20 672 y 21 037 días desde el epoch. Un año justo de
                // diferencia, que es lo que dura una renovación normal.
                assert_eq!(de, "2026-08-07");
                assert_eq!(a, "2027-08-07");
            }
            otro => panic!("{otro:?}"),
        }
    }

    #[test]
    fn el_json_lleva_las_claves_que_la_app_sabe_leer() {
        // Si el shell escribiera aquí una forma que la app no entiende, la app no
        // daría error: leería listas vacías y su informe diría que se ha
        // desinstalado TODO. Un formato incompatible en una tabla compartida no
        // rompe, miente.
        let i = inv(&[(443, "nginx")], &[("sshd", "running")], &[("git", "2.45")], &[("\\B", "Ready")]);
        let j = a_json(&i);
        for clave in ["\"ports\"", "\"services\"", "\"software\"", "\"certs\"", "\"scheduled\""] {
            assert!(j.contains(clave), "falta {clave} en {j}");
        }
        assert!(!j.contains("\"tasks\""), "usa el nombre de aquí y no el de la app: {j}");
        // Y da la vuelta entero.
        let vuelta = de_json(&j);
        assert_eq!(vuelta.ports, i.ports);
        assert_eq!(vuelta.services, i.services);
        assert_eq!(vuelta.software, i.software);
        assert_eq!(vuelta.tasks, i.tasks);
    }

    #[test]
    fn un_nombre_con_comillas_no_rompe_el_json() {
        // El asunto de un certificado lleva comillas más veces de las que parece,
        // y una ruta de Windows lleva barras invertidas en todas.
        let i = Inventory {
            software: vec![Software {
                name: "Acme \"IT\" Suite\\Core".into(),
                version: "1.0\n2".into(),
            }],
            ..Default::default()
        };
        let j = a_json(&i);
        let vuelta = de_json(&j);
        assert_eq!(vuelta.software, i.software, "no sobrevivió a la ida y vuelta: {j}");
    }

    #[test]
    fn una_linea_base_de_la_app_se_lee_sin_su_epoch() {
        // La app escribe `expires` en texto y no `expires_epoch`. Se acepta la
        // fila —el certificado existe y su asunto sirve de clave— con la fecha
        // desconocida, en vez de descartarla.
        let de_la_app = r#"{"ports":[],"services":[],"software":[],
            "certs":[{"path":"/a.pem","subject":"CN=api","expires":"2027-01-01","days_left":300}],
            "scheduled":[{"entry":"tarea"}]}"#;
        let i = de_json(de_la_app);
        assert_eq!(i.certs.len(), 1);
        assert_eq!(i.certs[0].subject, "CN=api");
        assert_eq!(i.certs[0].expires_epoch, None);
        assert_eq!(i.tasks[0].entry, "tarea");
        assert_eq!(i.tasks[0].state, "", "una tarea de la app no trae estado");
    }
}
