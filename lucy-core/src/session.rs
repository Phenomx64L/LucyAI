//! Las conversaciones, para que sobrevivan al cierre.
//!
//! EL FALLO QUE ESTO CIERRA. El shell nativo guardaba tres cosas al salir: el
//! modelo, el interruptor de movimiento y el nombre del operador. Las terminales
//! abiertas no. Se cerraba Lucy después de una investigación de veinte minutos y
//! al volver había una pestaña vacía llamada "Nueva Terminal" — el trabajo no se
//! había perdido en el sentido de que nunca estuvo guardado. La V2 sí lo hace,
//! en `lucy_sessions_svelte`, y con su número de versión.
//!
//! QUÉ SE GUARDA Y QUÉ NO. La CONVERSACIÓN, que es lo que el operador reconoce
//! como "lo que estaba haciendo". No el workspace —plan, ejecución, trace,
//! artefactos—: describe UN turno, y un plan a medio aprobar restaurado tres
//! días después es un botón de Ejecutar sobre un comando que ya no viene a
//! cuento. La salida de los comandos sí viaja, porque va dentro del hilo.
//!
//! Y NO SE GUARDA EL MODO AUTOMÁTICO. Nunca. Un modo que ejecuta comandos sin
//! que nadie los apruebe no puede volver encendido solo, y menos después de un
//! cierre que a lo mejor fue un cuelgue. Que cueste un clic es exactamente lo
//! que tiene que costar.

use serde::{Deserialize, Serialize};

/// Versión del formato.
///
/// Existe desde el primer día por lo que enseña la V2, que la añadió después y
/// tuvo que escribir una migración para envolver los datos viejos. Un fichero
/// sin versión no se puede leer mal: se puede leer COMO SI fuera el formato de
/// hoy, que es peor.
pub const VERSION: u32 = 1;

/// Cuántos mensajes se guardan por pestaña.
///
/// Una conversación larga cabe de sobra; una que lleva mil vueltas de comando y
/// salida, no. El corte es por el FINAL —lo último es lo que da contexto a
/// dónde se quedó uno— igual que en `turns::fit`, y por el mismo motivo.
pub const MAX_MSGS: usize = 120;

/// Tope de caracteres de UN mensaje guardado.
///
/// La salida de un `Get-EventLog` sin filtrar son megabytes. Guardarla entera
/// convierte el fichero de sesión en algo que tarda en escribirse al salir —y
/// salir es justo cuando nadie quiere esperar.
pub const MAX_MSG_CHARS: usize = 20_000;

/// Quién dijo un mensaje guardado.
///
/// Es un espejo del `Role` de la interfaz y no el mismo tipo a propósito: aquel
/// es de la vista y puede cambiar con ella; éste es un formato en disco, y
/// cambiarlo rompe los ficheros de todo el mundo. Que estén separados es lo que
/// permite tocar uno sin tocar el otro.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum SavedRole {
    User,
    Lucy,
    /// Un comando corrido. Con campos con nombre y no posicionales: esto es un
    /// fichero que alguien puede acabar abriendo para entender por qué su sesión
    /// no volvió, y `["Get-Service", true, "..."]` no se explica solo.
    Exec { cmd: String, ok: bool, out: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedMsg {
    pub role: SavedRole,
    pub text: String,
    pub stamp: String,
    /// Cuántas imágenes llevaba. El DATO no se guarda.
    ///
    /// Una captura en base64 son dos megabytes de texto, y una conversación con
    /// cinco convertiría el fichero de sesión en diez. Al restaurar se ve que
    /// hubo una imagen —que es lo que hace que el hilo se entienda— pero el
    /// modelo ya no la recibe: preguntar otra vez por ella pide adjuntarla otra
    /// vez, y decirlo es más honesto que enseñar una miniatura que ya no viaja.
    #[serde(default)]
    pub images: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub title: String,
    pub msgs: Vec<SavedMsg>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub tabs: Vec<SavedTab>,
    /// Cuál estaba delante. Se recorta al cargar si el número no cuadra.
    #[serde(default)]
    pub active: usize,
}

impl Session {
    /// Prepara una sesión para guardarla: recorta y numera la versión.
    ///
    /// Los topes se aplican AQUÍ y no al escribir, para que lo que se prueba sea
    /// lo que se guarda.
    pub fn new(tabs: Vec<SavedTab>, active: usize) -> Self {
        let tabs: Vec<SavedTab> = tabs
            .into_iter()
            .map(|mut t| {
                if t.msgs.len() > MAX_MSGS {
                    t.msgs.drain(..t.msgs.len() - MAX_MSGS);
                }
                for m in t.msgs.iter_mut() {
                    m.text = clip(&m.text, MAX_MSG_CHARS);
                    if let SavedRole::Exec { cmd, out, .. } = &mut m.role {
                        *cmd = clip(cmd, MAX_MSG_CHARS);
                        *out = clip(out, MAX_MSG_CHARS);
                    }
                }
                t
            })
            .collect();
        let active = if active < tabs.len() { active } else { 0 };
        Self { version: VERSION, tabs, active }
    }

    /// A JSON, para meterlo en el almacén de eframe.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// De JSON. `None` si no se entiende.
    ///
    /// UN FICHERO QUE NO SE ENTIENDE NO ES UN ERROR QUE MERECE UN DIÁLOGO: es
    /// una sesión que se pierde, y lo que el operador necesita entonces es una
    /// Lucy que arranque, no una que se niegue. Se devuelve `None` y se empieza
    /// de cero.
    pub fn from_json(s: &str) -> Option<Self> {
        let v: Self = serde_json::from_str(s).ok()?;
        // Una versión de más adelante se descarta en vez de leerse a medias. El
        // formato de mañana puede tener campos que hoy se ignorarían en
        // silencio, y una conversación restaurada a medias es peor que ninguna.
        if v.version > VERSION {
            return None;
        }
        let active = if v.active < v.tabs.len() { v.active } else { 0 };
        Some(Self { active, ..v })
    }
}

/// Recorta por caracteres conservando el final, con una marca de que se recortó.
///
/// Por el final porque en la salida de un comando lo interesante suele estar
/// abajo —el error, el total, la última línea— y en un mensaje largo, lo último
/// que se dijo. La marca importa: un texto cortado sin avisar se lee como un
/// comando que terminó donde no terminó.
fn clip(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let cortados = n - max;
    let cola: String = s.chars().skip(cortados).collect();
    format!("[…{cortados} caracteres recortados al guardar la sesión…]\n{cola}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(text: &str) -> SavedMsg {
        SavedMsg { role: SavedRole::User, text: text.into(), stamp: "10:30".into(), images: 0 }
    }

    fn tab(n: usize) -> SavedTab {
        SavedTab {
            title: "Servicios caídos".into(),
            msgs: (0..n).map(|i| msg(&format!("mensaje {i}"))).collect(),
        }
    }

    #[test]
    fn una_sesion_va_y_vuelve_igual() {
        let s = Session::new(vec![tab(3)], 0);
        let v = Session::from_json(&s.to_json()).expect("debería leerse");
        assert_eq!(v, s);
    }

    #[test]
    fn un_comando_y_su_salida_sobreviven_al_viaje() {
        // Es la mitad del valor de guardar la conversación: sin la salida, el
        // hilo restaurado dice que se ejecutó algo y no qué contestó.
        let t = SavedTab {
            title: "t".into(),
            msgs: vec![SavedMsg {
                role: SavedRole::Exec {
                    cmd: "Get-Service".into(),
                    ok: true,
                    out: "Spooler Running".into(),
                },
                text: String::new(),
                stamp: "10:31".into(),
                images: 0,
            }],
        };
        let v = Session::from_json(&Session::new(vec![t], 0).to_json()).unwrap();
        match &v.tabs[0].msgs[0].role {
            SavedRole::Exec { cmd, ok, out } => {
                assert_eq!(cmd, "Get-Service");
                assert!(*ok);
                assert_eq!(out, "Spooler Running");
            }
            otro => panic!("se perdió el comando: {otro:?}"),
        }
    }

    #[test]
    fn una_conversacion_larga_se_guarda_por_el_final() {
        // Lo último es lo que dice dónde se quedó uno. Guardar el principio y
        // tirar el final guarda justo lo que ya no importa.
        let s = Session::new(vec![tab(MAX_MSGS + 40)], 0);
        assert_eq!(s.tabs[0].msgs.len(), MAX_MSGS);
        assert_eq!(s.tabs[0].msgs.last().unwrap().text, format!("mensaje {}", MAX_MSGS + 39));
    }

    #[test]
    fn un_volcado_enorme_se_recorta_y_lo_dice() {
        // Sin la marca, la salida restaurada se leería como un comando que
        // terminó donde no terminó.
        let largo = "x".repeat(MAX_MSG_CHARS + 5_000);
        let s = Session::new(vec![SavedTab { title: "t".into(), msgs: vec![msg(&largo)] }], 0);
        let t = &s.tabs[0].msgs[0].text;
        assert!(t.contains("recortados"), "no avisa del recorte");
        assert!(t.chars().count() < MAX_MSG_CHARS + 200);
    }

    #[test]
    fn un_fichero_corrupto_no_impide_arrancar() {
        // Lo que hace falta cuando la sesión no se entiende es una Lucy que
        // arranque, no uno que se niegue.
        assert!(Session::from_json("{esto no es json").is_none());
        assert!(Session::from_json("").is_none());
        assert!(Session::from_json("null").is_none());
    }

    #[test]
    fn una_version_del_futuro_se_descarta_entera() {
        // Leerla a medias daría una conversación con huecos silenciosos, que es
        // peor que no restaurar nada.
        let futuro = r#"{"version":99,"tabs":[],"active":0}"#;
        assert!(Session::from_json(futuro).is_none());
    }

    #[test]
    fn una_pestana_activa_imposible_no_deja_la_sesion_inservible() {
        // Pasa de verdad: se guarda con la tercera pestaña delante y al arrancar
        // se restauran dos. Sin esto, el índice apunta fuera y la primera
        // lectura entra en pánico.
        let s = Session { version: VERSION, tabs: vec![tab(1)], active: 7 };
        assert_eq!(Session::from_json(&s.to_json()).unwrap().active, 0);
        assert_eq!(Session::new(vec![tab(1)], 7).active, 0);
    }

    #[test]
    fn las_imagenes_viajan_como_cuenta_y_no_como_datos() {
        // Una captura en base64 son dos megabytes de texto. El hilo restaurado
        // tiene que poder decir que hubo una imagen sin arrastrarla.
        let m = SavedMsg { images: 2, ..msg("mira esto") };
        let v = Session::from_json(&Session::new(
            vec![SavedTab { title: "t".into(), msgs: vec![m] }],
            0,
        )
        .to_json())
        .unwrap();
        assert_eq!(v.tabs[0].msgs[0].images, 2);
        assert!(v.to_json().len() < 500, "el JSON no debería llevar píxeles");
    }
}
