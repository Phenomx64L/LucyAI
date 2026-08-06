//! Capturar la pantalla, para que Lucy pueda verla.
//!
//! Port de `capture_primary_screen_png` en `commands/local_screen.rs`. Lo que se
//! trae es la MITAD DE VER: la otra mitad de ese módulo —mover el ratón, teclear,
//! el bucle que conduce el escritorio— no viene, y no por falta de tiempo. Ver la
//! pantalla es algo que el operador pide una vez y mira; conducirla es una
//! capacidad que necesita su propia puerta de permiso, y meterla de rebote con
//! la captura sería colar la segunda dentro del sí que se dio a la primera.
//!
//! BitBlt del DC de pantalla, BGRA→RGB, y un reescalado opcional. Lo último no
//! es cosmético: una pantalla 4K son ocho millones de píxeles que acaban en
//! base64 dentro de una petición HTTP, y los proveedores cobran la imagen por
//! área. A 1280 de ancho se lee un mensaje de error perfectamente y cuesta una
//! fracción.

/// Ancho al que se reduce por defecto.
///
/// El mismo que usa la app. Es el punto donde un cuadro de diálogo de Windows
/// sigue siendo legible y la imagen todavía cabe en el presupuesto de una
/// pregunta normal.
pub const MAX_WIDTH: u32 = 1280;

/// La pantalla principal, en PNG.
///
/// Reduce a `max_width` conservando la proporción cuando la pantalla es más
/// ancha. `0` no reduce.
#[cfg(windows)]
pub fn capture_png(max_width: u32) -> Result<Vec<u8>, String> {
    use image::{ImageBuffer, Rgba};
    use std::io::Cursor;
    use std::mem;
    use std::ptr::null_mut;
    use winapi::um::wingdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };
    use winapi::um::winuser::{GetDC, GetSystemMetrics, ReleaseDC, SM_CXSCREEN, SM_CYSCREEN};

    unsafe {
        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        if w <= 0 || h <= 0 {
            return Err(format!("pantalla con tamaño inválido: {w}×{h}"));
        }

        let hdc_screen = GetDC(null_mut());
        if hdc_screen.is_null() {
            return Err("GetDC(NULL) falló — no se pudo obtener el DC de pantalla".into());
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbmp = CreateCompatibleBitmap(hdc_screen, w, h);
        let old_bmp = SelectObject(hdc_mem, hbmp as *mut _);

        BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, 0, 0, SRCCOPY);

        // Los píxeles, como DIB de 32 bits y de arriba abajo.
        let mut bmi: BITMAPINFO = mem::zeroed();
        bmi.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        // Negativo = de arriba abajo. Con el signo cambiado la captura sale del
        // revés, que es un fallo que no se ve en el código y se ve muchísimo en
        // la imagen.
        bmi.bmiHeader.biHeight = -h;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        let mut pixels = vec![0u8; (w * h * 4) as usize];
        GetDIBits(
            hdc_mem,
            hbmp,
            0,
            h as u32,
            pixels.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(hdc_mem, old_bmp);
        DeleteObject(hbmp as *mut _);
        DeleteDC(hdc_mem);
        ReleaseDC(null_mut(), hdc_screen);

        // Win32 entrega BGRA; el codificador espera RGBA.
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(w as u32, h as u32, pixels)
                .ok_or("ImageBuffer::from_raw falló")?;

        let dynimg = if max_width > 0 && (w as u32) > max_width {
            let new_h = ((h as u64 * max_width as u64) / w as u64).max(1) as u32;
            let resized =
                image::imageops::resize(&img, max_width, new_h, image::imageops::FilterType::Triangle);
            image::DynamicImage::ImageRgba8(resized)
        } else {
            image::DynamicImage::ImageRgba8(img)
        };

        // Sin canal alfa: una captura no tiene transparencia, y quitarlo hace el
        // PNG bastante más pequeño por nada a cambio.
        let mut out = Cursor::new(Vec::new());
        dynimg
            .to_rgb8()
            .write_to(&mut out, image::ImageFormat::Png)
            .map_err(|e| format!("codificando el PNG: {e}"))?;
        Ok(out.into_inner())
    }
}

#[cfg(not(windows))]
pub fn capture_png(_max_width: u32) -> Result<Vec<u8>, String> {
    Err("la captura de pantalla solo está implementada en Windows".into())
}

/// La pantalla, ya lista para colgarla de un turno.
pub fn capture_image(max_width: u32) -> Result<crate::turns::Image, String> {
    let png = capture_png(max_width)?;
    Ok(crate::turns::Image {
        media_type: "image/png".into(),
        b64: crate::attach::b64_encode(&png),
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn la_captura_sale_y_es_un_png_de_verdad() {
        // En un runner sin escritorio esto no puede correr, y decirlo es mejor
        // que fallar: `GetDC(NULL)` en una sesión sin ventana no devuelve una
        // pantalla, devuelve otra cosa.
        let Ok(png) = capture_png(320) else {
            eprintln!("sin escritorio disponible; nada que comprobar");
            return;
        };
        // La firma del formato. Sin esto, un vector de ceros pasaría el test.
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "no empieza por la firma PNG");
        assert!(png.len() > 1000, "un PNG de una pantalla no ocupa {} bytes", png.len());
    }

    #[test]
    fn el_reescalado_baja_el_tamano_de_verdad() {
        // El sentido del tope es el presupuesto de la petición, así que lo que
        // hay que comprobar es que pesa menos, no que la llamada devolvió algo.
        let (Ok(chica), Ok(grande)) = (capture_png(320), capture_png(0)) else {
            eprintln!("sin escritorio disponible; nada que comprobar");
            return;
        };
        assert!(
            chica.len() < grande.len(),
            "reducida {} bytes, entera {} — el reescalado no hizo nada",
            chica.len(),
            grande.len()
        );
    }
}
