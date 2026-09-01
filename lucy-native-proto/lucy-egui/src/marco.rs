//! Lo que el marco del sistema daba y `with_decorations(false)` se llevó.
//!
//! ── LO QUE ESTO ARREGLA ──────────────────────────────────────────────────────
//!
//! Contado por quien lo sufrió: «el marco de la ventana es muy cuadrada y rompe
//! mucho la estética». Y no era una impresión: una ventana sin decoraciones en
//! Windows es un RECTÁNGULO EXACTO, con las cuatro esquinas a noventa grados y
//! sin una sombra que la despegue del escritorio.
//!
//! Windows 11 redondea las esquinas de las ventanas por su cuenta, sí — pero lo
//! hace sobre el marco que dibuja el sistema, y aquí ese marco no existe. La
//! cabecera de Lucy ES la barra de título, que es lo correcto y lo que hace que
//! no haya dos cabeceras de distinto color una encima de otra. El precio, que
//! nadie había pagado hasta ahora, es que todo lo que el marco traía hay que
//! reponerlo: los botones, el arrastre, el redimensionado —eso ya estaba— y
//! esto, que es la FORMA de la ventana.
//!
//! ── POR QUÉ A MANO Y NO CON EL CRATE `windows` ───────────────────────────────
//!
//! Son dos funciones de `dwmapi.dll`. El crate `windows` son cientos de megas de
//! metadatos y varios minutos de compilación para poder escribir esas dos
//! declaraciones, que aquí ocupan seis líneas. Es el mismo razonamiento que ya
//! hay escrito en `theme::os_prefers_light`, que lee el tema del sistema con
//! `reg.exe` en vez de arrastrar `winreg` por una consulta.
//!
//! ── LO QUE PASA EN UN WINDOWS QUE NO SEA EL 11 ───────────────────────────────
//!
//! Nada. `DwmSetWindowAttribute` devuelve un error para un atributo que no
//! conoce, y ese error se descarta a propósito: en Windows 10 la ventana se
//! queda cuadrada, que es exactamente como está hoy. Una función decorativa no
//! puede impedir que la aplicación arranque, y menos por una versión del sistema
//! que quien la ejecuta no eligió.

#![cfg(windows)]

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// `DWMWA_WINDOW_CORNER_PREFERENCE`. Windows 11 (compilación 22000) en adelante.
const ESQUINAS: u32 = 33;
/// `DWMWCP_ROUND` — el redondeo grande, el de una ventana de aplicación.
///
/// Y NO `DWMWCP_ROUNDSMALL`, que es el de un menú contextual o un desplegable.
/// Una ventana principal con el radio de un menú se lee como un menú enorme.
const REDONDEO_GRANDE: u32 = 2;

/// `DWMWA_SYSTEMBACKDROP_TYPE`. Windows 11 22H2 (compilación 22621) en adelante.
const FONDO: u32 = 38;
/// `DWMSBT_NONE` — sin material translúcido detrás.
const FONDO_NINGUNO: u32 = 1;

#[repr(C)]
struct Margenes {
    izquierda: i32,
    derecha: i32,
    arriba: i32,
    abajo: i32,
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: isize,
        atributo: u32,
        valor: *const std::ffi::c_void,
        tamano: u32,
    ) -> i32;

    fn DwmExtendFrameIntoClientArea(hwnd: isize, margenes: *const Margenes) -> i32;
}

/// El `HWND` de la ventana, si es que hay uno.
///
/// `Result` y no `unwrap`: en una compilación cruzada o bajo un backend que no
/// sea Win32 esto no existe, y quedarse sin arrancar por no poder redondear una
/// esquina sería un mal negocio.
fn hwnd_de(w: &impl HasWindowHandle) -> Option<isize> {
    match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

/// Le pide a Windows que redondee las esquinas y devuelva la sombra.
///
/// Se llama UNA VEZ, al crear la ventana. Los dos atributos son propiedades de
/// la ventana y el gestor de escritorio los conserva: repetirlo en cada
/// fotograma serían dos llamadas al sistema sesenta veces por segundo para
/// contestar lo mismo.
pub fn redondea(w: &impl HasWindowHandle) {
    let Some(hwnd) = hwnd_de(w) else { return };
    unsafe {
        // ── Las esquinas ─────────────────────────────────────────────────────
        let v = REDONDEO_GRANDE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            ESQUINAS,
            std::ptr::addr_of!(v).cast(),
            std::mem::size_of::<u32>() as u32,
        );

        // ── La sombra ────────────────────────────────────────────────────────
        //
        // UNA VENTANA SIN MARCO NO PROYECTA SOMBRA, y sin ella Lucy no se
        // despega del escritorio: sobre un fondo oscuro, la ventana en tema
        // oscuro se funde con lo que tenga detrás y el borde deja de saberse
        // dónde está. La sombra no es adorno; es lo que dice dónde acaba la
        // ventana.
        //
        // UN PÍXEL Y NO CERO. `DwmExtendFrameIntoClientArea` con todo a cero
        // significa «no extiendas nada» y no hace nada; con un margen, el
        // gestor de escritorio vuelve a tratar la ventana como una ventana con
        // marco a efectos de composición —sombra incluida— aunque el marco no se
        // dibuje. Un píxel es lo mínimo que lo consigue y lo que menos se ve.
        //
        // Y NO -1 EN LOS CUATRO, que es la receta que circula para el efecto de
        // cristal: eso extiende el marco a TODA el área de cliente, y sobre una
        // ventana que ya pinta su propio fondo opaco lo que sale es un borde
        // claro alrededor del contenido.
        let m = Margenes { izquierda: 1, derecha: 1, arriba: 1, abajo: 1 };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &m);

        // ── Y NADA DETRÁS ────────────────────────────────────────────────────
        //
        // Mica y Acrílico son lo más parecido que tiene Windows a la
        // translucidez de macOS, y se quedan fuera A PROPÓSITO: los dos exigen
        // que la ventana sea transparente para que se vea el material, y Lucy
        // pinta un lienzo opaco. Encendiéndolos sin más, lo que se consigue es
        // pagar el trabajo de composición para que no se vea nada.
        //
        // Se pide `NONE` explícitamente en vez de dejar el `AUTO` de fábrica
        // porque `AUTO` deja que el sistema decida, y esa decisión ha cambiado
        // entre versiones de Windows 11. Un aspecto que depende de la
        // compilación del sistema es un aspecto que no se puede probar.
        let f = FONDO_NINGUNO;
        let _ = DwmSetWindowAttribute(
            hwnd,
            FONDO,
            std::ptr::addr_of!(f).cast(),
            std::mem::size_of::<u32>() as u32,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_atributos_son_los_que_documenta_windows() {
        // Los números no se pueden deducir leyendo el código: son constantes de
        // `dwmapi.h` y un dígito cambiado no da error —`DwmSetWindowAttribute`
        // devuelve un fallo que aquí se descarta— sino que no hace nada. Esto no
        // los comprueba contra Windows; los deja escritos al lado de su nombre
        // para que la próxima persona no tenga que ir a buscarlos.
        assert_eq!(ESQUINAS, 33, "DWMWA_WINDOW_CORNER_PREFERENCE");
        assert_eq!(REDONDEO_GRANDE, 2, "DWMWCP_ROUND");
        assert_eq!(FONDO, 38, "DWMWA_SYSTEMBACKDROP_TYPE");
        assert_eq!(FONDO_NINGUNO, 1, "DWMSBT_NONE");
    }

    #[test]
    fn los_margenes_van_en_el_orden_de_win32() {
        // `MARGINS` es izquierda, DERECHA, arriba, abajo — y no el orden de las
        // agujas del reloj que uno escribiría de memoria. Con los campos
        // cruzados la estructura sigue midiendo dieciséis bytes y la llamada
        // sigue devolviendo éxito: el fallo no se ve hasta que alguien mira la
        // sombra de cerca.
        let m = Margenes { izquierda: 1, derecha: 2, arriba: 3, abajo: 4 };
        let bytes: [i32; 4] =
            unsafe { std::mem::transmute_copy::<Margenes, [i32; 4]>(&m) };
        assert_eq!(bytes, [1, 2, 3, 4], "el orden de MARGINS no es el de Win32");
        assert_eq!(std::mem::size_of::<Margenes>(), 16);
    }
}
