//! Dónde vive el modelo de Whisper y en qué estado está.
//!
//! EMPAQUETADO CON EL INSTALADOR, que es la decisión del operador y la correcta:
//! instalador autocontenido, sin red en el primer uso, y sin un descargador que
//! falle en una máquina detrás de un proxy corporativo — que es justo donde vive
//! una herramienta de administración.
//!
//! PERO NO EN GIT. Medio giga en el repositorio haría el clon inviable para
//! siempre, y un binario grande no se borra del historial después sin
//! reescribirlo entero. El modelo se trae en el paso de EMPAQUETADO, no se
//! guarda en el árbol. Por eso este módulo busca en dos sitios y no en uno.
//!
//! Formato safetensors + tokenizer de Hugging Face, que es lo que consume
//! `candle`. No es el `.bin` de ggml de whisper.cpp: son incompatibles, y bajar
//! el que no es produce un error de carga que no se parece a "modelo
//! equivocado".

use std::path::PathBuf;

/// Los tres ficheros que hace falta tener. Faltando uno, no hay transcripción.
///
/// Se comprueban los tres por separado a propósito: un directorio a medias —una
/// descarga interrumpida, un antivirus que se llevó uno— produce un error de
/// carga ilegible, y decir CUÁL falta es la diferencia entre arreglarlo en un
/// minuto y abrir un ticket.
pub const FILES: [&str; 3] = ["model.safetensors", "tokenizer.json", "config.json"];

/// El modelo elegido: `small`, multilingüe.
///
/// `base` transcribe español aceptablemente y se equivoca en nombres propios y
/// términos técnicos — que en una herramienta de administración son justo las
/// palabras que importan: hostnames, servicios, rutas. `medium` triplica el peso
/// del instalador para una mejora que no se nota dictando una orden corta.
pub const MODEL: &str = "whisper-small";

/// Dónde se busca, en orden.
///
/// Primero junto al ejecutable —ahí lo deja el instalador— y después en
/// `%APPDATA%`, que es donde puede dejarlo alguien que lo trae a mano o una
/// futura descarga opcional. El orden importa: el que viene con la instalación
/// manda sobre una copia suelta que nadie sabe de dónde salió.
pub fn search_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            v.push(dir.join("models").join(MODEL));
        }
    }
    if let Some(data) = dirs::data_dir() {
        v.push(data.join("Lucy").join("models").join(MODEL));
    }
    v
}

/// En qué estado está el modelo en esta máquina.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Los tres ficheros están. Se puede transcribir.
    Ready(PathBuf),
    /// El directorio existe pero le faltan ficheros — y se dicen cuáles.
    Incomplete { dir: PathBuf, missing: Vec<String> },
    /// No está en ningún sitio conocido.
    Missing,
}

impl Status {
    /// El mensaje que ve el operador. Cada estado dice qué hacer, no solo qué
    /// pasa: "falta el modelo" sin más deja a alguien mirando un botón muerto.
    pub fn message(&self) -> String {
        match self {
            Self::Ready(_) => "Modelo de voz listo".into(),
            Self::Incomplete { dir, missing } => format!(
                "El modelo de voz está incompleto en {}: falta {}. \
                 Suele ser una copia interrumpida — bórralo y reinstala.",
                dir.display(),
                missing.join(", ")
            ),
            Self::Missing => format!(
                "El modelo de voz ({MODEL}) no está instalado. Viene con el instalador \
                 de Lucy; en una compilación de desarrollo hay que ponerlo a mano en \
                 `models/{MODEL}` junto al ejecutable."
            ),
        }
    }

    pub fn ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }
}

/// Mira los sitios conocidos y dice qué hay.
pub fn status() -> Status {
    let mut incompleto: Option<Status> = None;
    for dir in search_paths() {
        if !dir.is_dir() {
            continue;
        }
        let missing = missing_files(&dir);
        if missing.is_empty() {
            return Status::Ready(dir);
        }
        // Se recuerda el primero incompleto pero se sigue buscando: puede haber
        // una copia buena en el segundo sitio, y rendirse en el primero sería
        // decir "roto" teniendo uno sano al lado.
        if incompleto.is_none() {
            incompleto = Some(Status::Incomplete { dir, missing });
        }
    }
    incompleto.unwrap_or(Status::Missing)
}

/// Cuáles de los tres ficheros faltan en un directorio.
pub fn missing_files(dir: &std::path::Path) -> Vec<String> {
    FILES
        .iter()
        .filter(|f| !dir.join(f).is_file())
        .map(|f| f.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(nombre: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lucy-whisper-test-{nombre}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn se_dice_que_fichero_falta_no_solo_que_algo_falta() {
        // Un directorio a medias —descarga interrumpida, antivirus que se llevó
        // uno— da un error de carga ilegible. Nombrar el que falta es la
        // diferencia entre arreglarlo en un minuto y abrir un ticket.
        let d = tmp("incompleto");
        std::fs::write(d.join("config.json"), "{}").unwrap();
        let m = missing_files(&d);
        assert_eq!(m.len(), 2);
        assert!(m.contains(&"model.safetensors".to_string()));
        assert!(m.contains(&"tokenizer.json".to_string()));
        assert!(!m.contains(&"config.json".to_string()));

        let s = Status::Incomplete { dir: d.clone(), missing: m };
        assert!(s.message().contains("model.safetensors"));
        assert!(!s.ready());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn con_los_tres_ficheros_esta_listo() {
        let d = tmp("completo");
        for f in FILES {
            std::fs::write(d.join(f), "x").unwrap();
        }
        assert!(missing_files(&d).is_empty());
        assert!(Status::Ready(d.clone()).ready());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn el_mensaje_de_ausente_dice_donde_ponerlo() {
        // "Falta el modelo" a secas deja a alguien mirando un botón muerto.
        let m = Status::Missing.message();
        assert!(m.contains(MODEL));
        assert!(m.contains("models/"), "no dice dónde: {m}");
    }

    #[test]
    fn se_busca_junto_al_ejecutable_antes_que_en_appdata() {
        // El que viene con la instalación manda sobre una copia suelta que nadie
        // sabe de dónde salió.
        let p = search_paths();
        assert!(!p.is_empty());
        if p.len() > 1 {
            let primero = p[0].to_string_lossy().to_lowercase();
            assert!(
                !primero.contains("appdata"),
                "appdata no puede ir primero: {primero}"
            );
        }
    }
}
