//! Capturar el micrófono.
//!
//! Es la mitad del dictado que NO depende del motor. Whisper local, el
//! reconocedor de Windows o un servicio de nube piden todos lo mismo: audio
//! mono, y en la práctica a 16 kHz. Por eso se escribe una vez y no ata a
//! ninguna decisión — la elección de motor cambia lo que se hace DESPUÉS con
//! estas muestras, no cómo se consiguen.
//!
//! EL DISPOSITIVO NO DA LO QUE UNO PIDE. Un micrófono entrega lo que él tiene:
//! 44 100 o 48 000 Hz, uno o dos canales, y a veces más. Pedirle 16 kHz mono y
//! confiar en que obedezca es cómo se acaba transcribiendo ruido — el audio
//! llega bien y se interpreta a la velocidad equivocada, que suena a voz
//! acelerada. Aquí se acepta lo que dé y se convierte.

use std::sync::{Arc, Mutex};

/// Lo que quiere Whisper, y lo que aceptan los demás motores.
pub const TARGET_HZ: u32 = 16_000;

/// Mezcla a mono promediando los canales.
///
/// Promediar y no quedarse con el primer canal: en un portátil con dos
/// micrófonos, quedarse con uno tira la mitad de la señal y en algunos equipos
/// ese uno es el que mira al ventilador.
pub fn to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks(channels)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect()
}

/// Remuestrea a `TARGET_HZ` por interpolación lineal.
///
/// Lineal y no un filtro de verdad: para VOZ a 16 kHz la diferencia es
/// inaudible para un reconocedor, y un remuestreador decente es una dependencia
/// entera. Si algún día se nota, se cambia esta función y nada más.
pub fn resample(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from_hz as f64 / to_hz as f64;
    let n = ((input.len() as f64) / ratio).floor() as usize;
    (0..n)
        .map(|i| {
            let pos = i as f64 * ratio;
            let a = pos.floor() as usize;
            let b = (a + 1).min(input.len() - 1);
            let t = (pos - a as f64) as f32;
            input[a] * (1.0 - t) + input[b] * t
        })
        .collect()
}

/// Cuántos segundos de audio hay.
pub fn duration_s(samples: &[f32]) -> f32 {
    samples.len() as f32 / TARGET_HZ as f32
}

/// El nivel de la señal, para pintar que el micro está oyendo algo.
///
/// Sin esto, "grabando" es una palabra en pantalla y no hay forma de saber si el
/// micrófono elegido por Windows es el que tienes delante o uno que no existe.
pub fn level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let n = samples.len().min(2048);
    let rms = samples[samples.len() - n..]
        .iter()
        .map(|s| s * s)
        .sum::<f32>()
        / n as f32;
    (rms.sqrt() * 4.0).min(1.0)
}

/// Una grabación en curso.
///
/// El stream de `cpal` vive dentro y se cierra al soltar la estructura, que es
/// lo que hace que parar sea simplemente dejar de tenerla.
pub struct Recording {
    _stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    hz: u32,
    channels: usize,
}

impl Recording {
    /// Abre el micrófono por defecto y empieza a grabar.
    pub fn start() -> Result<Self, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        let dev = host
            .default_input_device()
            .ok_or("No hay micrófono disponible")?;
        let cfg = dev
            .default_input_config()
            .map_err(|e| format!("El micrófono no dice su formato: {e}"))?;
        let hz = cfg.sample_rate().0;
        let channels = cfg.channels() as usize;

        let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
        let sink = buf.clone();
        let stream = dev
            .build_input_stream(
                &cfg.config(),
                move |data: &[f32], _: &_| {
                    // El callback corre en el hilo de audio: aquí no se hace
                    // NADA que pueda tardar. Copiar y salir. Convertir a mono y
                    // remuestrear dentro de él es cómo se produce un corte en la
                    // grabación que luego se oye como un chasquido.
                    if let Ok(mut b) = sink.lock() {
                        b.extend_from_slice(data);
                    }
                },
                |e| eprintln!("[lucy] error del micrófono: {e}"),
                None,
            )
            .map_err(|e| format!("No se pudo abrir el micrófono: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("No se pudo arrancar la grabación: {e}"))?;

        Ok(Self { _stream: stream, buf, hz, channels })
    }

    /// El nivel actual, para el indicador.
    pub fn level(&self) -> f32 {
        self.buf.lock().map(|b| level(&b)).unwrap_or(0.0)
    }

    /// Cierra la grabación y devuelve el audio ya en mono a 16 kHz.
    pub fn finish(self) -> Vec<f32> {
        let raw = self.buf.lock().map(|b| b.clone()).unwrap_or_default();
        resample(&to_mono(&raw, self.channels), self.hz, TARGET_HZ)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_mezcla_a_mono_promedia_los_canales() {
        // Promediar y no quedarse con el primero: en un portátil con dos
        // micrófonos, uno de ellos puede estar mirando al ventilador.
        let estereo = [1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
        assert_eq!(to_mono(&estereo, 2), vec![0.5, 0.5, 0.0]);
        // Mono se queda como está, sin copiar de más ni perder muestras.
        assert_eq!(to_mono(&[0.1, 0.2], 1), vec![0.1, 0.2]);
    }

    #[test]
    fn remuestrear_ajusta_la_duracion_no_solo_el_numero_de_muestras() {
        // El fallo que esto evita: dejar el audio a 48 kHz y decirle al
        // reconocedor que son 16 000. Llega entero y se interpreta tres veces
        // más rápido — suena a voz acelerada y no transcribe nada.
        let un_segundo_a_48k = vec![0.0f32; 48_000];
        let out = resample(&un_segundo_a_48k, 48_000, TARGET_HZ);
        assert_eq!(out.len(), 16_000);
        assert!((duration_s(&out) - 1.0).abs() < 0.01);
    }

    #[test]
    fn remuestrear_a_la_misma_frecuencia_no_toca_nada() {
        let v = vec![0.1, 0.2, 0.3];
        assert_eq!(resample(&v, 16_000, 16_000), v);
        assert!(resample(&[], 44_100, 16_000).is_empty());
    }

    #[test]
    fn la_interpolacion_no_se_sale_del_ultimo_indice() {
        // El punto siguiente al último no existe, y leerlo sería un pánico en la
        // última muestra de cada grabación — es decir, siempre.
        let v: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let out = resample(&v, 44_100, 16_000);
        assert!(!out.is_empty());
        assert!(out.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn el_nivel_va_de_cero_a_uno_y_no_se_sale() {
        assert_eq!(level(&[]), 0.0);
        assert_eq!(level(&[0.0; 100]), 0.0);
        // Una señal saturada no puede pasar de 1, o la barra se saldría de su
        // hueco.
        assert!(level(&[1.0; 4000]) <= 1.0);
        assert!(level(&[0.2; 4000]) > 0.0);
    }
}
