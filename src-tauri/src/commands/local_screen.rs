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

// ═══════════════════════════════════════════════════════════════════════════
//  PHASE B — Local desktop CONTROL (Lucy moves the mouse / types)
//
//  Reuses the existing Computer-Use provider abstraction + the rdp_agent loop
//  shape, but captures the LOCAL screen and sends input at ABSOLUTE screen
//  coordinates (scaled back up from the downscaled screenshot the model saw).
//
//  SAFETY (this controls the user's real mouse + keyboard):
//    • `confirm` MUST be true — the frontend only passes it after an explicit
//      per-invocation user confirmation. Defends against accidental / model-
//      triggered runs.
//    • LOCAL_AGENT_RUNNING blocks concurrent agents.
//    • LOCAL_AGENT_CANCEL is the kill-switch: checked before every step AND
//      before every action; cancel_local_agent() flips it from the UI.
//    • Hard step cap.
//  The Rust execute_powershell blocklist does NOT gate GUI input, so these
//  software guards + the explicit confirm are the safety boundary here.
// ═══════════════════════════════════════════════════════════════════════════

use std::sync::atomic::{AtomicBool, Ordering};

static LOCAL_AGENT_RUNNING: AtomicBool = AtomicBool::new(false);
static LOCAL_AGENT_CANCEL:  AtomicBool = AtomicBool::new(false);

/// Kill-switch — the UI calls this to abort an in-flight local agent.
#[tauri::command]
pub fn cancel_local_agent() {
    LOCAL_AGENT_CANCEL.store(true, Ordering::SeqCst);
}

/// True while a local agent loop is executing (for the UI to show state).
#[tauri::command]
pub fn local_agent_running() -> bool {
    LOCAL_AGENT_RUNNING.load(Ordering::SeqCst)
}

// ── Win32 input layer — ABSOLUTE screen coordinates ───────────────────────
#[cfg(windows)]
mod input {
    use winapi::um::winuser::{
        SetCursorPos, mouse_event, keybd_event, SendInput, INPUT, INPUT_KEYBOARD,
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_WHEEL,
    };
    use std::{thread, time::Duration, mem};

    const KEYEVENTF_UNICODE: u32 = 0x0004;
    const KEYEVENTF_KEYUP:   u32 = 0x0002;

    pub fn mouse_click(x: i32, y: i32, button: &str, double: bool) {
        unsafe {
            SetCursorPos(x, y);
            thread::sleep(Duration::from_millis(70));
            let (down, up) = match button {
                "right"  => (MOUSEEVENTF_RIGHTDOWN,  MOUSEEVENTF_RIGHTUP),
                "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                _        => (MOUSEEVENTF_LEFTDOWN,   MOUSEEVENTF_LEFTUP),
            };
            mouse_event(down, 0, 0, 0, 0);
            thread::sleep(Duration::from_millis(50));
            mouse_event(up, 0, 0, 0, 0);
            if double {
                thread::sleep(Duration::from_millis(90));
                mouse_event(down, 0, 0, 0, 0);
                thread::sleep(Duration::from_millis(50));
                mouse_event(up, 0, 0, 0, 0);
            }
        }
    }

    pub fn mouse_move(x: i32, y: i32) { unsafe { SetCursorPos(x, y); } }

    pub fn drag(x0: i32, y0: i32, x1: i32, y1: i32) {
        unsafe {
            SetCursorPos(x0, y0);
            thread::sleep(Duration::from_millis(80));
            mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
            thread::sleep(Duration::from_millis(70));
            SetCursorPos(x1, y1);
            thread::sleep(Duration::from_millis(120));
            mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
        }
    }

    // Local computer-use input primitive — kept ready for Phase B (scroll
    // control); not wired into a command yet, so silence the dead-code warn.
    #[allow(dead_code)]
    pub fn scroll(x: i32, y: i32, amount: i32) {
        unsafe {
            SetCursorPos(x, y);
            thread::sleep(Duration::from_millis(50));
            // 120 = one wheel notch; positive = up.
            mouse_event(MOUSEEVENTF_WHEEL, 0, 0, (amount * 120) as u32, 0);
        }
    }

    pub fn type_text(text: &str) {
        unsafe {
            for ch in text.encode_utf16() {
                let mut dn: INPUT = mem::zeroed();
                dn.type_ = INPUT_KEYBOARD;
                { let k = dn.u.ki_mut(); k.wScan = ch; k.dwFlags = KEYEVENTF_UNICODE; }
                let mut up: INPUT = mem::zeroed();
                up.type_ = INPUT_KEYBOARD;
                { let k = up.u.ki_mut(); k.wScan = ch; k.dwFlags = KEYEVENTF_UNICODE | KEYEVENTF_KEYUP; }
                let inputs = [dn, up];
                SendInput(2, inputs.as_ptr() as *mut _, mem::size_of::<INPUT>() as i32);
                thread::sleep(Duration::from_millis(16));
            }
        }
    }

    pub fn send_key(key: &str) {
        unsafe {
            let parts: Vec<&str> = key.split('+').collect();
            let (mods, main) = if parts.len() > 1 {
                (&parts[..parts.len()-1], parts[parts.len()-1])
            } else { (&[][..], parts[0]) };
            for m in mods { let vk = modifier_vk(m); if vk != 0 { keybd_event(vk, 0, 0, 0); } }
            thread::sleep(Duration::from_millis(40));
            let vk = named_vk(main);
            if vk != 0 {
                keybd_event(vk, 0, 0, 0);
                thread::sleep(Duration::from_millis(50));
                keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
            }
            for m in mods.iter().rev() { let vk = modifier_vk(m); if vk != 0 { keybd_event(vk, 0, KEYEVENTF_KEYUP, 0); } }
        }
    }

    fn modifier_vk(name: &str) -> u8 {
        match name.to_lowercase().as_str() {
            "ctrl" | "control" => 0x11, "alt" => 0x12, "shift" => 0x10,
            "win" | "super" => 0x5B, _ => 0,
        }
    }
    fn named_vk(name: &str) -> u8 {
        match name.to_lowercase().as_str() {
            "return" | "enter" | "\n" => 0x0D, "escape" | "esc" => 0x1B, "tab" => 0x09,
            "backspace" => 0x08, "delete" | "del" => 0x2E, "space" => 0x20,
            "home" => 0x24, "end" => 0x23, "pageup" => 0x21, "pagedown" => 0x22,
            "left" => 0x25, "up" => 0x26, "right" => 0x27, "down" => 0x28,
            "f1" => 0x70, "f2" => 0x71, "f3" => 0x72, "f4" => 0x73, "f5" => 0x74, "f6" => 0x75,
            "f7" => 0x76, "f8" => 0x77, "f9" => 0x78, "f10" => 0x79, "f11" => 0x7A, "f12" => 0x7B,
            s if s.len() == 1 => s.chars().next().map(|c| c.to_ascii_uppercase() as u8).unwrap_or(0),
            _ => 0,
        }
    }
}

/// Capture for the agent loop: returns (base64 PNG, scale_x, scale_y) where
/// scale maps a coordinate in the (downscaled) screenshot back to the real
/// screen. Windows-only.
#[cfg(windows)]
fn capture_for_agent(max_width: u32) -> Result<(String, f64, f64), String> {
    use winapi::um::winuser::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let (nw, nh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let png = capture_primary_screen_png(max_width)?;
    let shot_w = if max_width > 0 && (nw as u32) > max_width { max_width } else { nw as u32 };
    let shot_h = if shot_w == nw as u32 { nh as u32 } else { ((nh as u64 * shot_w as u64) / nw as u64).max(1) as u32 };
    let sx = nw as f64 / shot_w.max(1) as f64;
    let sy = nh as f64 / shot_h.max(1) as f64;
    Ok((base64::engine::general_purpose::STANDARD.encode(&png), sx, sy))
}

#[cfg(windows)]
fn emit_local(app: &tauri::AppHandle, kind: &str, data: &str, detail: &str) {
    use tauri::Emitter;
    let _ = app.emit("local_agent_step", serde_json::json!({
        "kind": kind, "data": data, "detail": detail,
    }));
}

/// Phase B — agentic local-desktop control loop. GATED by `confirm`.
#[tauri::command]
pub async fn run_local_agent(
    task:      String,
    model:     Option<String>,
    max_steps: Option<u32>,
    confirm:   bool,
    app:       tauri::AppHandle,
) -> Result<String, String> {
    if !confirm {
        return Err("Control local no confirmado por el usuario (gate de seguridad).".into());
    }
    #[cfg(not(windows))]
    { let _ = (task, model, max_steps, app); return Err("Control local solo en Windows por ahora.".into()); }

    #[cfg(windows)]
    {
        use crate::commands::computer_use::{create_provider, types::{ComputeConfig, ComputerAction}};
        use serde_json::{json, Value};
        use std::{thread, time::Duration};

        // One agent at a time.
        if LOCAL_AGENT_RUNNING.swap(true, Ordering::SeqCst) {
            return Err("Ya hay un agente local en ejecución.".into());
        }
        LOCAL_AGENT_CANCEL.store(false, Ordering::SeqCst);
        // RAII-ish guard so the flag clears on every exit path.
        struct Guard;
        impl Drop for Guard { fn drop(&mut self) { LOCAL_AGENT_RUNNING.store(false, Ordering::SeqCst); } }
        let _guard = Guard;

        let model     = model.unwrap_or_else(|| "claude-opus-4-8".into());
        let max_steps = max_steps.unwrap_or(15);
        let max_width = 1280u32;

        // Wrap in an Arc so the credential check can run on a BLOCKING thread
        // (the keyring read is synchronous; if it ever stalls it must not freeze
        // the async executor — and a JoinHandle is a real future, so the timeout
        // around it can actually fire).
        let provider: std::sync::Arc<dyn crate::commands::computer_use::ComputerUseProvider> =
            std::sync::Arc::from(create_provider(&model));
        emit_local(&app, "action", "", &format!("[Agente local] {} — armado, controlando tu equipo", provider.name()));

        // ── Stage 1: credentials ──────────────────────────────────────────────
        emit_local(&app, "action", "", "1/3 Verificando credenciales del proveedor…");
        let prov_cc = provider.clone();
        let cred = tokio::time::timeout(
            Duration::from_secs(10),
            tauri::async_runtime::spawn(async move { prov_cc.check_credentials().await }),
        ).await;
        match cred {
            Err(_)         => { emit_local(&app, "error", "", "La verificación de credenciales no respondió en 10s. ¿API key del proveedor configurada en Ajustes?"); return Err("Timeout verificando credenciales (10s).".into()); }
            Ok(Err(e))     => { emit_local(&app, "error", "", &format!("Fallo interno verificando credenciales: {}", e)); return Err(format!("Credenciales (hilo): {}", e)); }
            Ok(Ok(Err(e))) => { emit_local(&app, "error", "", &format!("Credenciales: {}", e)); return Err(format!("Credenciales: {}", e)); }
            Ok(Ok(Ok(()))) => {}
        }

        // ── Stage 2: initial screenshot (timeout-wrapped this time) ────────────
        emit_local(&app, "action", "", "2/3 Capturando la pantalla…");
        let init_cap = tokio::time::timeout(
            Duration::from_secs(20),
            tauri::async_runtime::spawn_blocking(move || capture_for_agent(max_width)),
        ).await;
        let (init_b64, mut sx, mut sy) = match init_cap {
            Err(_)             => { emit_local(&app, "error", "", "La captura de pantalla no terminó en 20s (timeout)."); return Err("Timeout en la captura inicial (20s).".into()); }
            Ok(Err(e))         => { emit_local(&app, "error", "", &format!("Captura inicial (hilo): {}", e)); return Err(format!("Captura inicial: {}", e)); }
            Ok(Ok(Err(e)))     => { emit_local(&app, "error", "", &format!("Captura inicial: {}", e)); return Err(format!("Captura inicial: {}", e)); }
            Ok(Ok(Ok(v)))      => v,
        };
        emit_local(&app, "action", "", "3/3 Pantalla capturada. Consultando al modelo…");
        emit_local(&app, "screenshot", &init_b64, "Pantalla inicial");

        // Screen dims (in screenshot space) for the prompt.
        let shot_w = (1920.0 / sx).round() as i32; // informational only
        let _ = shot_w;

        let mut messages: Vec<Value> = vec![json!({
            "role": "user",
            "content": [
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": init_b64 } },
                { "type": "text", "text": format!(
                    "You are controlling the user's LOCAL Windows desktop. The screenshot is the whole primary screen.\n\
                     Coordinates you give must be in SCREENSHOT pixels (the image you see).\n\n\
                     Task: {}\n\n\
                     Work step by step. When done, say so and STOP.", task) }
            ]
        })];

        let config = ComputeConfig {
            model: model.clone(), api_key: String::new(), task: task.clone(),
            max_steps, window_width: 1280, window_height: 800,
        };

        let mut step = 0u32;
        let mut summary = String::new();

        loop {
            if LOCAL_AGENT_CANCEL.load(Ordering::SeqCst) {
                emit_local(&app, "done", "", "Detenido por el usuario.");
                return Ok("Detenido por el usuario.".into());
            }
            if step >= max_steps {
                emit_local(&app, "done", "", &format!("Límite de {} pasos.", max_steps));
                break;
            }
            step += 1;
            emit_local(&app, "action", "", &format!("[Agente local] paso {}/{}…", step, max_steps));

            let (shot, nsx, nsy) = match tauri::async_runtime::spawn_blocking(move || capture_for_agent(max_width)).await {
                Ok(Ok(v)) => v,
                _ => (String::new(), sx, sy),
            };
            sx = nsx; sy = nsy;

            // Per-step timeout: a slow or hung provider call must surface as a
            // clear error instead of pinning the UI on "Procesando…" forever.
            let outcome = match tokio::time::timeout(
                Duration::from_secs(90),
                provider.query_llm(&config, &shot, &messages),
            ).await {
                Ok(inner) => inner,
                Err(_) => {
                    emit_local(&app, "error", "", "El modelo no respondió en 90s (timeout). Revisa la API key del proveedor y la conexión.");
                    return Err("Timeout del modelo (90s).".into());
                }
            };
            match outcome {
                Ok((actions, text, should_stop)) => {
                    if !text.is_empty() {
                        emit_local(&app, "text", "", &text);
                        summary = text.clone();
                        messages.push(json!({ "role": "assistant", "content": [{ "type": "text", "text": text }] }));
                    }
                    if actions.is_empty() || should_stop {
                        emit_local(&app, "done", "", &summary);
                        break;
                    }
                    for action in actions {
                        if LOCAL_AGENT_CANCEL.load(Ordering::SeqCst) {
                            emit_local(&app, "done", "", "Detenido por el usuario.");
                            return Ok("Detenido por el usuario.".into());
                        }
                        // Map screenshot coords → real screen coords.
                        let map = |c: [i32; 2]| [ (c[0] as f64 * sx).round() as i32, (c[1] as f64 * sy).round() as i32 ];
                        // Win32 input is blocking (SetCursorPos + SendInput +
                        // sleeps); run each action on the blocking pool so the
                        // async loop keeps draining cancel checks + events.
                        match action {
                            ComputerAction::LeftClick { coordinate } => { let p = map(coordinate); emit_local(&app, "action", "", &format!("Click ({},{})", p[0], p[1])); let _ = tauri::async_runtime::spawn_blocking(move || input::mouse_click(p[0], p[1], "left", false)).await; }
                            ComputerAction::RightClick { coordinate } => { let p = map(coordinate); emit_local(&app, "action", "", &format!("Click derecho ({},{})", p[0], p[1])); let _ = tauri::async_runtime::spawn_blocking(move || input::mouse_click(p[0], p[1], "right", false)).await; }
                            ComputerAction::DoubleClick { coordinate } => { let p = map(coordinate); emit_local(&app, "action", "", &format!("Doble click ({},{})", p[0], p[1])); let _ = tauri::async_runtime::spawn_blocking(move || input::mouse_click(p[0], p[1], "left", true)).await; }
                            ComputerAction::MiddleClick { coordinate } => { let p = map(coordinate); let _ = tauri::async_runtime::spawn_blocking(move || input::mouse_click(p[0], p[1], "middle", false)).await; }
                            ComputerAction::MouseMove { coordinate } => { let p = map(coordinate); let _ = tauri::async_runtime::spawn_blocking(move || input::mouse_move(p[0], p[1])).await; }
                            ComputerAction::LeftClickDrag { start_coordinate, end_coordinate } => { let a = map(start_coordinate); let b = map(end_coordinate); emit_local(&app, "action", "", &format!("Arrastrar ({},{})→({},{})", a[0], a[1], b[0], b[1])); let _ = tauri::async_runtime::spawn_blocking(move || input::drag(a[0], a[1], b[0], b[1])).await; }
                            ComputerAction::Type { text } => { emit_local(&app, "action", "", &format!("Escribir: {:.40}", text)); let _ = tauri::async_runtime::spawn_blocking(move || input::type_text(&text)).await; }
                            ComputerAction::Key { text } => { emit_local(&app, "action", "", &format!("Tecla: {}", text)); let _ = tauri::async_runtime::spawn_blocking(move || { for k in text.split_whitespace() { input::send_key(k); thread::sleep(Duration::from_millis(90)); } }).await; }
                            ComputerAction::Screenshot | ComputerAction::CursorPosition => {}
                        }
                        tokio::time::sleep(Duration::from_millis(650)).await;
                        let (after, _, _) = match tauri::async_runtime::spawn_blocking(move || capture_for_agent(max_width)).await {
                            Ok(Ok(v)) => v,
                            _ => (String::new(), sx, sy),
                        };
                        messages.push(json!({ "role": "user", "content": [{ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": after } }] }));
                        if messages.len() > 8 {
                            let first = messages[0].clone();
                            let tail = messages[messages.len()-6..].to_vec();
                            messages = std::iter::once(first).chain(tail).collect();
                        }
                    }
                }
                Err(e) => { emit_local(&app, "error", "", &format!("Error: {}", e)); return Err(format!("Agente local: {}", e)); }
            }
        }
        Ok(summary)
    }
}
