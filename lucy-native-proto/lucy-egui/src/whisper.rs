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
            Self::Ready(_) => crate::i18n::tr("Modelo de voz listo").to_string(),
            Self::Incomplete { dir, missing } => crate::i18n::trf(
                "El modelo de voz está incompleto en {dir}: falta {falta}. Suele ser una \
                 copia interrumpida — bórralo y vuelve a ponerlo.",
                &[("dir", &dir.display().to_string()), ("falta", &missing.join(", "))],
            ),
            // NO VIENE CON EL INSTALADOR, y este mensaje decía que sí.
            //
            // Reportado por el operador: instaló Lucy, el dictado le dijo que el
            // modelo «viene con el instalador de Lucy», y no estaba. Comprobado:
            // ni `lucy.nsi` ni `lucy.wxs` mencionan `models`, `whisper` ni un
            // solo `.bin` — no lo empaquetan, y probablemente nunca lo hicieron.
            //
            // Y NO DEBERÍAN. `whisper-small` son unos cientos de megas contra los
            // 19,6 MB que mide hoy el instalador entero; meterlo dentro
            // multiplicaría por quince lo que se descarga todo el mundo para una
            // función que no usa todo el mundo. Es el mismo razonamiento que
            // deja a Ollama fuera.
            //
            // Así que lo que se arregla es el MENSAJE. Un aviso que manda a
            // reinstalar para conseguir algo que la reinstalación no trae no es
            // un aviso: es una tarde perdida.
            Self::Missing => crate::i18n::trf(
                "El dictado necesita el modelo de voz {modelo}, que no viene con Lucy: \
                 son cientos de megas y el instalador entero pesa veinte. Descárgalo y \
                 deja sus tres ficheros en «{ruta}».",
                &[
                    ("modelo", MODEL),
                    (
                        "ruta",
                        &search_paths()
                            .last()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| format!("models/{MODEL}")),
                    ),
                ],
            ),
        }
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
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn con_los_tres_ficheros_esta_listo() {
        let d = tmp("completo");
        for f in FILES {
            std::fs::write(d.join(f), "x").unwrap();
        }
        assert!(missing_files(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn el_mensaje_de_ausente_dice_donde_ponerlo() {
        // "Falta el modelo" a secas deja a alguien mirando un botón muerto.
        let m = Status::Missing.message();
        assert!(m.contains(MODEL));
        // LA RUTA DE VERDAD, no un `models/` de ejemplo. El mensaje ahora dice
        // la carpeta exacta de esta máquina —con las barras que usa Windows— y
        // pedir `models/` obligaba a escribir una ruta ilustrativa que quien la
        // lee tiene que traducir a la suya. Se comprueba la carpeta, no el
        // separador.
        assert!(m.contains("models"), "no dice dónde: {m}");
        assert!(
            search_paths().iter().any(|p| m.contains(&p.display().to_string())),
            "la ruta que da no es ninguna de las que busca: {m}"
        );
    }

    #[test]
    fn el_mensaje_de_ausente_no_promete_lo_que_el_instalador_no_trae() {
        // EL FALLO QUE ESTO CIERRA, reportado por el operador: instaló Lucy, el
        // dictado le dijo que el modelo «viene con el instalador de Lucy», y no
        // estaba. Comprobado en `packaging/`: ni el NSIS ni el WiX mencionan
        // `models`, `whisper` ni un solo `.bin`.
        //
        // Un aviso que manda a reinstalar para conseguir algo que la
        // reinstalación no trae no es un aviso: es una tarde perdida.
        let m = Status::Missing.message();
        assert!(
            !m.contains("instalador de Lucy") && !m.contains("reinstala"),
            "vuelve a prometer que viene con el instalador: {m}"
        );
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

// ── Banco de filtros mel ─────────────────────────────────────────────────────
//
// `pcm_to_mel` necesita el banco, y `candle-transformers` no lo trae: su ejemplo
// lo empotra desde un binario propio. Calcularlo evita meter 64 KB opacos en el
// árbol, y sobre todo permite que un error salte en un test en vez de en una
// transcripción mediocre que nadie sabe explicar.
//
// Es la escala mel de SLANEY, no la de HTK. Son dos fórmulas distintas y las dos
// se llaman "mel": librosa usa Slaney por defecto y Whisper se entrenó con eso.
// Con la de HTK el banco sale plausible —triángulos crecientes, todo positivo— y
// las bandas caen en frecuencias equivocadas, que es justo la clase de fallo que
// no se ve mirando.

/// Frecuencia (Hz) → mel, escala Slaney.
///
/// Lineal por debajo de 1000 Hz y logarítmica por encima. El punto de corte no
/// es redondo por casualidad: 1000 Hz son exactamente 15 mel, y las dos ramas se
/// encuentran ahí sin salto.
fn hz_to_mel(f: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1000.0;
    const MIN_LOG_MEL: f64 = MIN_LOG_HZ / F_SP; // 15
    if f < MIN_LOG_HZ {
        f / F_SP
    } else {
        MIN_LOG_MEL + (f / MIN_LOG_HZ).ln() / (6.4f64.ln() / 27.0)
    }
}

/// La inversa exacta de `hz_to_mel`.
fn mel_to_hz(m: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1000.0;
    const MIN_LOG_MEL: f64 = MIN_LOG_HZ / F_SP;
    if m < MIN_LOG_MEL {
        F_SP * m
    } else {
        MIN_LOG_HZ * ((6.4f64.ln() / 27.0) * (m - MIN_LOG_MEL)).exp()
    }
}

/// El banco de `n_mels` filtros triangulares, aplanado por filas.
///
/// Cada fila mide `1 + n_fft/2` — 201 para el n_fft de 400 de Whisper — porque
/// es como lo indexa `pcm_to_mel`: `filters[fila * 201 + bin]`. Aplanarlo con
/// otra anchura produce un espectrograma que no falla y no significa nada.
pub fn mel_filters(sr: usize, n_fft: usize, n_mels: usize) -> Vec<f32> {
    let bins = 1 + n_fft / 2;
    // Los centros de las bandas van repartidos por igual EN MEL, que es toda la
    // idea: en Hz se apiñan abajo, donde el oído distingue.
    let (m0, m1) = (hz_to_mel(0.0), hz_to_mel(sr as f64 / 2.0));
    let pts: Vec<f64> = (0..n_mels + 2)
        .map(|i| mel_to_hz(m0 + (m1 - m0) * i as f64 / (n_mels + 1) as f64))
        .collect();

    let mut w = vec![0.0f32; n_mels * bins];
    for i in 0..n_mels {
        let (lo, mid, hi) = (pts[i], pts[i + 1], pts[i + 2]);
        // Normalización de Slaney: cada filtro cubre el mismo ÁREA, no la misma
        // altura. Sin ella las bandas anchas de arriba pesarían más que las
        // estrechas de abajo solo por ser anchas.
        let enorm = 2.0 / (hi - lo);
        for k in 0..bins {
            let f = k as f64 * sr as f64 / n_fft as f64;
            let up = (f - lo) / (mid - lo);
            let down = (hi - f) / (hi - mid);
            w[i * bins + k] = (up.min(down).max(0.0) * enorm) as f32;
        }
    }
    w
}

#[cfg(test)]
mod mel {
    use super::*;

    #[test]
    fn mil_hercios_son_quince_mel_exactos() {
        // El ancla de la escala de Slaney: es donde la rama lineal y la
        // logarítmica se encuentran, y sale de la definición, no de una tabla.
        // Si esto se mueve, la escala es otra — probablemente la de HTK, que da
        // un banco de aspecto correcto en frecuencias equivocadas.
        assert!((hz_to_mel(1000.0) - 15.0).abs() < 1e-9);
        assert!((mel_to_hz(15.0) - 1000.0).abs() < 1e-9);
        assert!((hz_to_mel(0.0)).abs() < 1e-12);
    }

    #[test]
    fn ir_y_volver_devuelve_la_misma_frecuencia() {
        // Una inversa mal escrita produce centros de banda desplazados, y eso no
        // rompe nada: transcribe peor y punto.
        for f in [50.0, 440.0, 999.0, 1000.0, 1001.0, 4000.0, 8000.0] {
            let ida_vuelta = mel_to_hz(hz_to_mel(f));
            assert!((ida_vuelta - f).abs() < 1e-6, "{f} → {ida_vuelta}");
        }
    }

    #[test]
    fn la_forma_es_la_que_espera_candle() {
        // 80 filas de 201: `pcm_to_mel` indexa `filters[fila * 201 + bin]`.
        // Aplanarlo con otra anchura da un espectrograma que no falla y no
        // significa nada.
        let w = mel_filters(16_000, 400, 80);
        assert_eq!(w.len(), 80 * 201);
        assert!(w.iter().all(|v| v.is_finite() && *v >= 0.0));
    }

    #[test]
    fn cada_filtro_es_un_triangulo_y_sube_de_frecuencia() {
        let (bins, n) = (201usize, 80usize);
        let w = mel_filters(16_000, 400, n);
        let pico = |i: usize| {
            (0..bins)
                .max_by(|a, b| w[i * bins + a].total_cmp(&w[i * bins + b]))
                .unwrap()
        };
        // Los centros suben: el filtro i escucha más agudo que el i-1. Un banco
        // desordenado es el síntoma de haber repartido los puntos en Hz en vez
        // de en mel.
        for i in 1..n {
            assert!(pico(i) >= pico(i - 1), "el filtro {i} no sube");
        }
        // Y cada uno tiene una sola cuesta arriba y una abajo.
        let i = 40;
        let p = pico(i);
        assert!(w[i * bins + p] > 0.0);
        for k in 1..p {
            assert!(w[i * bins + k] >= w[i * bins + k - 1] - 1e-6, "sube y baja");
        }
    }

    #[test]
    fn las_bandas_bajas_son_mas_estrechas_que_las_altas() {
        // Es LA propiedad de la escala mel — resolución fina donde el oído
        // distingue. Un banco con bandas uniformes es un banco lineal disfrazado.
        let (bins, n) = (201usize, 80usize);
        let w = mel_filters(16_000, 400, n);
        let ancho = |i: usize| (0..bins).filter(|k| w[i * bins + k] > 0.0).count();
        assert!(ancho(70) > ancho(5), "{} vs {}", ancho(70), ancho(5));
    }
}

// ── Transcripción ────────────────────────────────────────────────────────────

use candle_core::{Device, IndexOp, Tensor};
use candle_transformers::models::whisper as w;

/// Tope de tokens por dictado.
///
/// Whisper procesa ventanas de 30 s y una orden dictada no llega ni de lejos.
/// El tope existe por otra razón: un modelo puede entrar en bucle repitiendo la
/// última palabra, y sin corte se queda generando hasta que alguien mata la app.
const MAX_TOKENS: usize = 224;

/// El transcriptor cargado. Cargar cuesta segundos; transcribir, décimas.
///
/// Por eso vive y se reutiliza en vez de construirse por dictado: cargar medio
/// giga de pesos cada vez que se pulsa el micrófono convertiría una función
/// instantánea en una espera.
pub struct Transcriber {
    model: w::model::Whisper,
    tokenizer: tokenizers::Tokenizer,
    config: w::Config,
    filters: Vec<f32>,
    sot: u32,
    eot: u32,
    lang: u32,
    transcribe: u32,
    no_ts: u32,
}

impl Transcriber {
    /// Carga el modelo desde un directorio ya verificado por `status`.
    pub fn load(dir: &std::path::Path) -> Result<Self, String> {
        let cfg_txt = std::fs::read_to_string(dir.join("config.json"))
            .map_err(|e| format!("No se pudo leer config.json: {e}"))?;
        let config: w::Config = serde_json::from_str(&cfg_txt)
            .map_err(|e| format!("config.json no tiene la forma esperada: {e}"))?;

        let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| format!("No se pudo leer el tokenizador: {e}"))?;

        // CPU a propósito: `candle` con GPU exige compilar con CUDA, que es
        // volver a la dependencia nativa que se evitó al elegir Rust puro. Para
        // una orden dictada de unos segundos, la CPU basta.
        let device = Device::Cpu;
        let vb = unsafe {
            candle_nn::VarBuilder::from_mmaped_safetensors(
                &[dir.join("model.safetensors")],
                w::DTYPE,
                &device,
            )
            .map_err(|e| format!("No se pudieron mapear los pesos: {e}"))?
        };
        let model = w::model::Whisper::load(&vb, config.clone())
            .map_err(|e| format!("No se pudo construir el modelo: {e}"))?;

        // Los tokens especiales se resuelven UNA vez y se guardan. Buscarlos por
        // texto en cada dictado es trabajo repetido, y que falte uno es un fallo
        // de modelo equivocado que conviene detectar al cargar, no a mitad de
        // una transcripción.
        let tok = |s: &str| {
            tokenizer
                .token_to_id(s)
                .ok_or_else(|| format!("El tokenizador no tiene {s}: ¿es un modelo multilingüe?"))
        };
        Ok(Self {
            filters: mel_filters(w::SAMPLE_RATE, w::N_FFT, config.num_mel_bins),
            sot: tok(w::SOT_TOKEN)?,
            eot: tok(w::EOT_TOKEN)?,
            lang: tok("<|es|>")?,
            transcribe: tok(w::TRANSCRIBE_TOKEN)?,
            no_ts: tok(w::NO_TIMESTAMPS_TOKEN)?,
            model,
            tokenizer,
            config,
        })
    }

    /// Transcribe audio mono a 16 kHz y devuelve el texto.
    pub fn transcribe(&mut self, pcm: &[f32]) -> Result<String, String> {
        // Whisper SIEMPRE mira una ventana de 30 s. Un dictado corto se rellena
        // con silencio: darle menos muestras produce un espectrograma de otra
        // forma y el encoder falla con un error de dimensiones que no dice nada
        // sobre la causa.
        let mut pad = pcm.to_vec();
        pad.resize(w::N_SAMPLES, 0.0);

        let mel = w::audio::pcm_to_mel(&self.config, &pad, &self.filters);
        let n = self.config.num_mel_bins;
        let mel = Tensor::from_vec(mel, (1, n, w::N_FRAMES), &Device::Cpu)
            .map_err(|e| format!("El espectrograma no cuadra: {e}"))?;

        let audio = self
            .model
            .encoder
            .forward(&mel, true)
            .map_err(|e| format!("Falló el encoder: {e}"))?;

        // Los cuatro tokens de arranque fijan la tarea: transcribir, en español,
        // sin marcas de tiempo. Fijar el idioma en vez de detectarlo se salta una
        // pasada entera del modelo, y esta herramienta se usa en español.
        let mut tokens = vec![self.sot, self.lang, self.transcribe, self.no_ts];
        for i in 0..MAX_TOKENS {
            let t = Tensor::new(tokens.as_slice(), &Device::Cpu)
                .and_then(|t| t.unsqueeze(0))
                .map_err(|e| format!("No se pudo formar la entrada: {e}"))?;
            // `flush_kv_cache` solo en la primera pasada: después la caché es lo
            // que evita recalcular todo el prefijo en cada token.
            let logits = self
                .model
                .decoder
                .forward(&t, &audio, i == 0)
                .map_err(|e| format!("Falló el decoder: {e}"))?;
            let last = logits
                .dim(1)
                .and_then(|d| logits.i((0, d - 1)))
                .map_err(|e| format!("No se pudo leer la salida: {e}"))?;
            let next = last
                .argmax(0)
                .and_then(|t| t.to_scalar::<u32>())
                .map_err(|e| format!("No se pudo elegir el token: {e}"))?;
            if next == self.eot {
                break;
            }
            tokens.push(next);
        }

        // Se descartan los cuatro de control: son instrucciones, no texto.
        self.tokenizer
            .decode(&tokens[4..], true)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("No se pudo descodificar: {e}"))
    }
}
