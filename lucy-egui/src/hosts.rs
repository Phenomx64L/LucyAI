//! Los equipos remotos que el operador ya tiene dados de alta.
//!
//! NO SE INVENTAN NI SE VUELVEN A PEDIR. La app real guarda el índice completo
//! —nombre, dirección, usuario, tipo— en el Credential Manager de Windows, bajo
//! el servicio `LucySysAdmin` y la clave `LucyHost_lucy_hosts_index`, y es de
//! ahí de donde sale esta lista. Es la misma entrada que lee
//! `initHostsFromKeyring` en el frontend web, así que las dos interfaces ven
//! exactamente los mismos equipos sin sincronizar nada.
//!
//! Ese detalle es justo lo que hace posible este prototipo: el índice no está en
//! `localStorage` del WebView —ahí solo hay una copia parcial heredada— sino en
//! un almacén del sistema operativo. Si la lista viviera dentro del navegador,
//! un shell nativo no podría verla y habría que volver a dar de alta cada equipo
//! a mano.
//!
//! Solo LECTURA. Aquí no se escribe el índice ni se tocan las contraseñas: dar
//! de alta un equipo sigue siendo cosa de la vista de Configuración, que además
//! valida lo que se guarda.

use serde::Deserialize;

/// El servicio bajo el que la app real guarda todo en el Credential Manager.
const SERVICE: &str = "LucySysAdmin";
/// La entrada concreta con el índice de equipos.
const INDEX_KEY: &str = "LucyHost_lucy_hosts_index";

/// Un equipo del índice.
///
/// Los nombres de campo son los del JSON que escribe el frontend (`camelCase`);
/// solo se declara lo que esta vista usa — serde ignora el resto, que es lo que
/// permite que la app añada campos sin romper esto.
#[derive(Debug, Clone, Deserialize)]
pub struct Host {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub username: String,
    /// `windows` (WinRM) o `linux` (SSH).
    #[serde(rename = "type", default)]
    pub kind: String,
}

impl Host {
    /// Cómo se llega al equipo, para la etiqueta del menú.
    pub fn transport(&self) -> &'static str {
        if self.kind == "windows" {
            "WinRM"
        } else {
            "SSH"
        }
    }
}

/// Lee el índice de equipos. Una lista vacía significa "no hay ninguno dado de
/// alta", que es un estado normal y no un error.
///
/// Un fallo del almacén tampoco se propaga: sin equipos remotos el dashboard del
/// equipo local funciona igual, y romper la vista entera porque el Credential
/// Manager no contesta sería cambiar una función que falta por una pantalla que
/// no arranca.
pub fn load() -> Vec<Host> {
    let Ok(entry) = keyring::Entry::new(SERVICE, INDEX_KEY) else {
        return Vec::new();
    };
    let Ok(raw) = entry.get_password() else {
        return Vec::new();
    };
    parse(&raw)
}

/// Separado de `load` para poder probarlo: el que lee del sistema no se puede
/// ejecutar en un test, pero el que interpreta el JSON es donde están los fallos
/// de verdad.
pub fn parse(raw: &str) -> Vec<Host> {
    serde_json::from_str::<Vec<Host>>(raw).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_el_formato_que_escribe_la_app() {
        // Recorte real del índice: `type` es palabra reservada en Rust y el
        // frontend usa camelCase — si el renombrado se cae, esto deja de
        // encontrar equipos sin que nada falle a la vista.
        let raw = r#"[
            {"id":"h1","name":"SRV-DC01","host":"10.0.0.5","type":"windows",
             "username":"admin","port":5985,"tags":["prod"]},
            {"id":"h2","name":"nas","host":"10.0.0.9","type":"linux",
             "username":"root","sshKeyPath":"C:/k/id_rsa"}
        ]"#;
        let hs = parse(raw);
        assert_eq!(hs.len(), 2);
        assert_eq!(hs[0].name, "SRV-DC01");
        assert_eq!(hs[0].kind, "windows");
        assert_eq!(hs[0].transport(), "WinRM");
        assert_eq!(hs[1].transport(), "SSH");
    }

    #[test]
    fn un_indice_ilegible_no_tumba_la_vista() {
        // Sin equipos remotos el dashboard local sigue siendo útil. Devolver
        // vacío es la respuesta correcta a "no se pudo leer".
        assert!(parse("").is_empty());
        assert!(parse("{}").is_empty());
        assert!(parse(r#"[{"sin":"campos"}]"#).is_empty(), "faltan id y name");
    }

    #[test]
    fn los_campos_opcionales_pueden_faltar() {
        // Un equipo dado de alta por una versión anterior puede no traerlos
        // todos; eso no debe hacer desaparecer al resto de la lista.
        let hs = parse(r#"[{"id":"h1","name":"solo-nombre"}]"#);
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].transport(), "SSH", "sin tipo se asume el genérico");
    }
}
