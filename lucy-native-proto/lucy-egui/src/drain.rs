//! El ritmo con el que aparece el texto.
//!
//! EL PROBLEMA. Un stream de nube no llega carácter a carácter: llega a
//! ráfagas. Gemini manda a veces una frase entera en una sola trama, así que
//! pintar cada token en cuanto llega hace que el texto salte en bloques — se
//! queda quieto medio segundo y de golpe aparece un párrafo. Eso es lo que se
//! siente "agresivo": no es que vaya rápido, es que va a tirones.
//!
//! LA SOLUCIÓN ES DESACOPLAR RECIBIR DE ENSEÑAR. Lo que llega entra en una cola
//! y se revela a ritmo constante. La V2 hace lo mismo (`createTokenDrain`,
//! `DRAIN_MS = 30`), aunque suelta trozos enteros; aquí se sueltan CARACTERES,
//! que es lo que convierte el efecto en escritura en vez de en parpadeo.
//!
//! Y EL RITMO SE ADAPTA, que es la parte que se olvida. Con velocidad fija, una
//! respuesta larga termina de recibirse y sigue escribiéndose veinte segundos
//! después: el operador espera delante de un texto que ya está entero en
//! memoria. La velocidad sube con lo que queda en cola, de modo que el retraso
//! nunca pasa de una fracción de segundo por mucho que llegue de golpe.

use std::time::Instant;

/// Caracteres por segundo con la cola casi vacía.
///
/// Se ve como escritura y se puede leer al mismo tiempo. Más rápido y vuelve el
/// salto; más lento y se hace esperar sin motivo.
const BASE_CPS: f32 = 220.0;

/// En cuánto tiempo se quiere vaciar lo que haya en cola.
///
/// Es lo que fija el retraso MÁXIMO entre lo que ya llegó y lo que se ve: con
/// 0.5 s, una ráfaga de dos mil caracteres se enseña acelerando en vez de
/// tardar nueve segundos a velocidad base.
const CATCH_UP_S: f32 = 0.5;

/// La cola de texto recibido y todavía no enseñado.
#[derive(Default)]
pub struct Drain {
    pending: String,
    last: Option<Instant>,
}

impl Drain {
    /// Encola lo que acaba de llegar del stream.
    pub fn push(&mut self, s: &str) {
        self.pending.push_str(s);
    }

    /// ¿Queda algo por enseñar? Mientras sea `true` hay que seguir repintando.
    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Lo que queda en cola, sin sacarlo. Para cerrar el turno con el texto
    /// completo aunque todavía se esté escribiendo en pantalla.
    pub fn peek(&self) -> &str {
        &self.pending
    }

    /// Suelta lo que toque según el tiempo transcurrido y lo devuelve.
    ///
    /// El reloj se guarda dentro: quien llama solo tiene que hacerlo una vez por
    /// frame. La primera llamada no suelta nada —no hay intervalo del que
    /// hablar— y arranca el cronómetro.
    pub fn tick(&mut self, now: Instant) -> String {
        let Some(last) = self.last.replace(now) else {
            return String::new();
        };
        if self.pending.is_empty() {
            return String::new();
        }
        let dt = now.duration_since(last).as_secs_f32();
        let cps = BASE_CPS + self.pending.chars().count() as f32 / CATCH_UP_S;
        let n = (cps * dt).floor() as usize;
        if n == 0 {
            // Con un frame muy corto no llega ni a un carácter. Se devuelve el
            // reloj hacia atrás para que ese tiempo no se pierda: si no, a 240
            // fps no se revelaría nunca nada.
            self.last = Some(last);
            return String::new();
        }
        self.take(n)
    }

    /// Vacía la cola de golpe. Para cuando el turno termina y ya no hay nada que
    /// esperar — o cuando alguien cambia de vista y vuelve.
    pub fn flush(&mut self) -> String {
        self.last = None;
        std::mem::take(&mut self.pending)
    }

    /// Corta los primeros `n` caracteres. POR CARACTERES: cortar por índice de
    /// byte en medio de uno multibyte es un pánico, y una respuesta en español
    /// lleva acentos en cada línea.
    fn take(&mut self, n: usize) -> String {
        let cut = self
            .pending
            .char_indices()
            .nth(n)
            .map_or(self.pending.len(), |(i, _)| i);
        let rest = self.pending.split_off(cut);
        std::mem::replace(&mut self.pending, rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn la_primera_llamada_solo_arranca_el_reloj() {
        // Sin intervalo anterior no hay nada que calcular, y soltar todo aquí
        // haría que el primer frame escupiera la ráfaga entera — justo lo que
        // se quiere evitar.
        let mut d = Drain::default();
        d.push("hola mundo");
        assert_eq!(d.tick(t0()), "");
        assert!(d.busy());
    }

    #[test]
    fn suelta_a_ritmo_y_no_de_golpe() {
        let mut d = Drain::default();
        d.push(&"x".repeat(100));
        let a = t0();
        d.tick(a);
        // 100 ms a ~420 c/s (base + 100/0.5) son unos 42 caracteres.
        let out = d.tick(a + Duration::from_millis(100));
        assert!(!out.is_empty());
        assert!(out.chars().count() < 100, "no puede soltarlo todo: {}", out.chars().count());
        assert!(d.busy());
    }

    #[test]
    fn una_rafaga_grande_se_acelera_en_vez_de_hacer_esperar() {
        // Con velocidad fija, dos mil caracteres tardarían nueve segundos en
        // escribirse y el operador esperaría delante de un texto que ya está
        // entero en memoria.
        let mut d = Drain::default();
        d.push(&"x".repeat(2000));
        let a = t0();
        d.tick(a);
        let out = d.tick(a + Duration::from_millis(500));
        assert!(
            out.chars().count() >= 2000,
            "medio segundo debería bastar para 2000; soltó {}",
            out.chars().count()
        );
    }

    #[test]
    fn nunca_parte_un_caracter_por_la_mitad() {
        // El corte por índice de byte en medio de un multibyte es un pánico, y
        // una respuesta en español lleva acentos en casi cada línea.
        let mut d = Drain::default();
        d.push(&"áé—ñ".repeat(200));
        let a = t0();
        d.tick(a);
        let mut todo = String::new();
        let mut i = 1;
        while d.busy() && i < 200 {
            todo.push_str(&d.tick(a + Duration::from_millis(16 * i as u64)));
            i += 1;
        }
        todo.push_str(&d.flush());
        assert_eq!(todo, "áé—ñ".repeat(200));
    }

    #[test]
    fn un_frame_muy_corto_no_pierde_el_tiempo_acumulado() {
        // A 240 fps un frame son 4 ms: menos de un carácter a velocidad base.
        // Si ese tiempo se descartara, no se revelaría nunca nada.
        let mut d = Drain::default();
        d.push(&"x".repeat(50));
        let a = t0();
        d.tick(a);
        for k in 1..=3 {
            assert_eq!(d.tick(a + Duration::from_micros(500 * k)), "");
        }
        // Tras 2 ms acumulados sigue sin llegar a un carácter completo, pero el
        // reloj NO se ha movido: a los 20 ms sí sale texto.
        assert!(!d.tick(a + Duration::from_millis(20)).is_empty());
    }

    #[test]
    fn flush_lo_devuelve_todo_y_deja_la_cola_vacia() {
        let mut d = Drain::default();
        d.push("resto");
        assert_eq!(d.flush(), "resto");
        assert!(!d.busy());
        assert_eq!(d.flush(), "");
    }
}
