// ── local_screen.rs — Capture the LOCAL desktop (Phase A of local computer-use) ──
//
// Lucy already ships a complete Computer-Use agent (see commands/computer_use/
// + rdp_agent.rs) — screenshot + click/type/key/drag, four LLM providers, an
// agentic loop — but it is locked to RDP windows (mstsc.exe). Phase A opens
// the first half of that capability on the LOCAL desktop: Lucy can SEE the
// screen. No mouse/keyboard control yet — that is Phase B, which will reuse
// the ComputerAction vocabulary behind an explicit per-session permission
// gate.
//
// This module grabs the PRIMARY monitor via BitBlt from the screen DC,
// converts BGRA→RGB, optionally downscales (so a 4K screen doesn't blow up the
// base64 / vision-token budget), and returns a base64 PNG suitable for the
// chat's existing image-attachment path.

use base64::Engine as _;

/// Capture the primary monitor as a PNG byte vector. Downscales to `max_width`
/// (keeping aspect ratio) when the screen is wider. Windows-only.
#[cfg(windows)]
pub fn capture_primary_screen_png(max_width: u32) -> Result<Vec<u8>, String> {
    use image::{ImageBuffer, Rgba};
    use std::io::Cursor;
    use std::mem;
    use std::ptr::null_mut;
    use winapi::um::winuser::{GetDC, ReleaseDC, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    use winapi::um::wingdi::{
        CreateCompatibleDC, CreateCompatibleBitmap, SelectObject, BitBlt, GetDIBits,
        DeleteObject, DeleteDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };

    unsafe {
        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        if w <= 0 || h <= 0 {
            return Err(format!("pantalla con tamaño inválido: {}×{}", w, h));
        }

        let hdc_screen = GetDC(null_mut());
        if hdc_screen.is_null() {
            return Err("GetDC(NULL) falló — no se pudo obtener el DC de pantalla".into());
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbmp    = CreateCompatibleBitmap(hdc_screen, w, h);
        let old_bmp = SelectObject(hdc_mem, hbmp as *mut _);

        // Blit the whole primary screen into our memory bitmap.
        BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, 0, 0, SRCCOPY);

        // Pull the pixels as a 32-bpp top-down DIB.
        let mut bmi: BITMAPINFO = mem::zeroed();
        bmi.bmiHeader.biSize        = mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth       = w;
        bmi.bmiHeader.biHeight      = -h;   // negative = top-down
        bmi.bmiHeader.biPlanes      = 1;
        bmi.bmiHeader.biBitCount    = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        let mut pixels = vec![0u8; (w * h * 4) as usize];
        GetDIBits(
            hdc_mem, hbmp, 0, h as u32,
            pixels.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        );

        // GDI cleanup.
        SelectObject(hdc_mem, old_bmp);
        DeleteObject(hbmp as *mut _);
        DeleteDC(hdc_mem);
        ReleaseDC(null_mut(), hdc_screen);

        // Win32 gives BGRA → swap to RGBA for the image crate.
        for px in pixels.chunks_exact_mut(4) { px.swap(0, 2); }

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(w as u32, h as u32, pixels)
                .ok_or("ImageBuffer::from_raw falló")?;

        // Downscale wide screens so the base64 + vision tokens stay reasonable.
        let dynimg = if max_width > 0 && (w as u32) > max_width {
            let new_h = ((h as u64 * max_width as u64) / w as u64).max(1) as u32;
            let resized = image::imageops::resize(
                &img, max_width, new_h, image::imageops::FilterType::Triangle,
            );
            image::DynamicImage::ImageRgba8(resized)
        } else {
            image::DynamicImage::ImageRgba8(img)
        };

        // Drop alpha (a screenshot has none) → smaller PNG.
        let mut out = Cursor::new(Vec::new());
        dynimg.to_rgb8()
            .write_to(&mut out, image::ImageFormat::Png)
            .map_err(|e| format!("PNG encode: {}", e))?;
        Ok(out.into_inner())
    }
}

/// Tauri command — returns a base64 PNG of the primary monitor.
/// `max_width` defaults to 1280px (downscaled, aspect-preserving).
#[tauri::command]
pub async fn capture_local_screen(max_width: Option<u32>) -> Result<String, String> {
    let mw = max_width.unwrap_or(1280);
    #[cfg(windows)]
    {
        let png = tauri::async_runtime::spawn_blocking(move || capture_primary_screen_png(mw))
            .await
            .map_err(|e| format!("error interno de captura: {}", e))??;
        Ok(base64::engine::general_purpose::STANDARD.encode(&png))
    }
    #[cfg(not(windows))]
    {
        let _ = mw;
        Err("La captura de pantalla local solo está soportada en Windows por ahora.".into())
    }
}
