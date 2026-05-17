// ── RDP Agent — Multi-Provider Computer Use (GUI Automation for Remote Desktop) ──
//
// Architecture:
//   find_rdp_windows()      → enumerate open mstsc.exe windows (HWND + size)
//   capture_rdp_screenshot() → PrintWindow → PNG → base64  (callable from JS)
//   run_rdp_agent()          → Multi-provider Computer Use agentic loop:
//       1. Screenshot RDP window
//       2. Send to selected LLM provider (Anthropic/Gemini/OpenAI/Ollama)
//       3. Parse actions → execute (click / type / key / screenshot)
//       4. Send screenshot → repeat until task complete or max_steps
//
// Tauri events emitted to frontend:
//   "rdp_agent_step"  →  { hwnd, kind, data, detail }
//     kind = "screenshot" | "action" | "text" | "done" | "error"
//     data = base64 PNG for "screenshot", empty otherwise
//
// Supported providers:
//   - claude-* (Anthropic)
//   - gemini-* (Google)
//   - gpt-4* (OpenAI)
//   - local-* or default (Ollama)
//
// ─────────────────────────────────────────────────────────────────────────────

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use crate::commands::computer_use::{create_provider, types::{ComputeConfig, ComputerAction}};

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpWindowInfo {
    pub hwnd:   u64,
    pub title:  String,
    pub width:  i32,
    pub height: i32,
}

// ─── Win32 layer (compiled only on Windows) ───────────────────────────────────

#[cfg(windows)]
mod win32 {
    use winapi::shared::windef::{HWND, POINT, RECT};
    use winapi::shared::minwindef::{BOOL, DWORD, LPARAM};
    use winapi::um::winuser::*;
    use winapi::um::wingdi::*;
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::winnt::{PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
    use winapi::um::psapi::GetModuleFileNameExW;
    use winapi::um::handleapi::CloseHandle;
    use std::mem;

    // ── Window enumeration ────────────────────────────────────────────────────

    struct EnumCtx {
        results: Vec<super::RdpWindowInfo>,
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 { return 1; }

        let mut pid: DWORD = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);

        let hproc = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if hproc.is_null() { return 1; }

        let mut name_buf = [0u16; 260];
        let len = GetModuleFileNameExW(hproc, std::ptr::null_mut(), name_buf.as_mut_ptr(), 260);
        CloseHandle(hproc);

        if len == 0 { return 1; }
        let proc_name = String::from_utf16_lossy(&name_buf[..len as usize]);
        if !proc_name.to_lowercase().ends_with("mstsc.exe") { return 1; }

        let mut title_buf = [0u16; 512];
        let tlen = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), 512);
        if tlen == 0 { return 1; }
        let title = String::from_utf16_lossy(&title_buf[..tlen as usize]).to_string();

        let mut rect: RECT = mem::zeroed();
        GetClientRect(hwnd, &mut rect);

        let ctx = &mut *(lp as *mut EnumCtx);
        ctx.results.push(super::RdpWindowInfo {
            hwnd:   hwnd as u64,
            title,
            width:  rect.right  - rect.left,
            height: rect.bottom - rect.top,
        });
        1
    }

    pub fn find_rdp_windows() -> Vec<super::RdpWindowInfo> {
        let mut ctx = EnumCtx { results: Vec::new() };
        unsafe { EnumWindows(Some(enum_cb), &mut ctx as *mut EnumCtx as LPARAM); }
        ctx.results
    }

    // ── Screenshot via PrintWindow + GDI ─────────────────────────────────────
    //
    // PrintWindow works even when the mstsc window is behind other windows
    // or partially off-screen. It renders the window's own content.

    pub fn capture_screenshot(hwnd_val: u64) -> Result<Vec<u8>, String> {
        use image::{ImageBuffer, Rgba};
        use std::io::Cursor;

        unsafe {
            let hwnd = hwnd_val as isize as HWND;

            let mut rect: RECT = mem::zeroed();
            if GetClientRect(hwnd, &mut rect) == 0 {
                return Err("GetClientRect failed — window may have closed".into());
            }
            let w = rect.right  - rect.left;
            let h = rect.bottom - rect.top;
            if w <= 0 || h <= 0 {
                return Err(format!("Ventana con tamaño inválido: {}×{}", w, h));
            }

            // Create in-memory DC + bitmap
            let hdc     = GetDC(hwnd);
            let hdc_mem = CreateCompatibleDC(hdc);
            let hbmp    = CreateCompatibleBitmap(hdc, w, h);
            let old_bmp = SelectObject(hdc_mem, hbmp as *mut _);

            // PW_CLIENTONLY = 0x00000001
            PrintWindow(hwnd, hdc_mem, 1u32);

            // Read pixel data as 32-bpp top-down DIB
            let mut bmi: BITMAPINFO = mem::zeroed();
            bmi.bmiHeader.biSize        = mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth       = w;
            bmi.bmiHeader.biHeight      = -h;   // negative = top-down scan order
            bmi.bmiHeader.biPlanes      = 1;
            bmi.bmiHeader.biBitCount    = 32;
            bmi.bmiHeader.biCompression = BI_RGB;

            let n = (w * h * 4) as usize;
            let mut pixels = vec![0u8; n];
            GetDIBits(
                hdc_mem, hbmp, 0, h as u32,
                pixels.as_mut_ptr() as *mut _,
                &mut bmi,
                DIB_RGB_COLORS,
            );

            // Clean up GDI objects
            SelectObject(hdc_mem, old_bmp);
            DeleteObject(hbmp as *mut _);
            DeleteDC(hdc_mem);
            ReleaseDC(hwnd, hdc);

            // Win32 returns BGRA → convert to RGBA
            for px in pixels.chunks_exact_mut(4) { px.swap(0, 2); }

            // Encode as PNG
            let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
                ImageBuffer::from_raw(w as u32, h as u32, pixels)
                    .ok_or("ImageBuffer::from_raw failed")?;
            let mut out = Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png)
                .map_err(|e| format!("PNG encode: {}", e))?;
            Ok(out.into_inner())
        }
    }

    // ── Mouse input ───────────────────────────────────────────────────────────
    //
    // Coordinates from Claude are relative to the RDP window client area.
    // ClientToScreen converts them to absolute screen coordinates for SetCursorPos.

    pub fn mouse_click(hwnd_val: u64, x: i32, y: i32, button: &str, double_click: bool) {
        unsafe {
            let hwnd = hwnd_val as isize as HWND;

            // Restore if minimised and bring to foreground
            if IsIconic(hwnd) != 0 { ShowWindow(hwnd, SW_RESTORE); }
            SetForegroundWindow(hwnd);
            std::thread::sleep(std::time::Duration::from_millis(200));

            // Convert client → screen
            let mut pt = POINT { x, y };
            ClientToScreen(hwnd, &mut pt);
            SetCursorPos(pt.x, pt.y);
            std::thread::sleep(std::time::Duration::from_millis(80));

            let (down, up) = match button {
                "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                _       => (MOUSEEVENTF_LEFTDOWN,  MOUSEEVENTF_LEFTUP),
            };

            mouse_event(down, 0, 0, 0, 0);
            std::thread::sleep(std::time::Duration::from_millis(60));
            mouse_event(up, 0, 0, 0, 0);

            if double_click {
                std::thread::sleep(std::time::Duration::from_millis(100));
                mouse_event(down, 0, 0, 0, 0);
                std::thread::sleep(std::time::Duration::from_millis(60));
                mouse_event(up, 0, 0, 0, 0);
            }
        }
    }

    pub fn mouse_move(hwnd_val: u64, x: i32, y: i32) {
        unsafe {
            let hwnd = hwnd_val as isize as HWND;
            let mut pt = POINT { x, y };
            ClientToScreen(hwnd, &mut pt);
            SetCursorPos(pt.x, pt.y);
        }
    }

    // ── Keyboard — type arbitrary text ────────────────────────────────────────
    //
    // Uses SendInput with KEYEVENTF_UNICODE so any Unicode character works,
    // including accented letters and special symbols.

    pub fn type_text(hwnd_val: u64, text: &str) {
        use winapi::um::winuser::{SendInput, INPUT, INPUT_KEYBOARD};
        const KEYEVENTF_UNICODE: u32 = 0x0004;
        const KEYEVENTF_KEYUP:   u32 = 0x0002;

        unsafe {
            let hwnd = hwnd_val as isize as HWND;
            SetForegroundWindow(hwnd);
            std::thread::sleep(std::time::Duration::from_millis(120));

            for ch in text.encode_utf16() {
                // key-down
                let mut inp_dn: INPUT = std::mem::zeroed();
                inp_dn.type_ = INPUT_KEYBOARD;
                let ki_dn = inp_dn.u.ki_mut();
                ki_dn.wScan   = ch;
                ki_dn.dwFlags = KEYEVENTF_UNICODE;

                // key-up
                let mut inp_up: INPUT = std::mem::zeroed();
                inp_up.type_ = INPUT_KEYBOARD;
                let ki_up = inp_up.u.ki_mut();
                ki_up.wScan   = ch;
                ki_up.dwFlags = KEYEVENTF_UNICODE | KEYEVENTF_KEYUP;

                let inputs = [inp_dn, inp_up];
                SendInput(
                    inputs.len() as u32,
                    inputs.as_ptr() as *mut _,
                    std::mem::size_of::<INPUT>() as i32,
                );
                std::thread::sleep(std::time::Duration::from_millis(18));
            }
        }
    }

    // ── Keyboard — press named keys ───────────────────────────────────────────

    pub fn send_key(hwnd_val: u64, key: &str) {
        const KEYEVENTF_KEYUP: u32 = 0x0002;

        unsafe {
            let hwnd = hwnd_val as isize as HWND;
            SetForegroundWindow(hwnd);
            std::thread::sleep(std::time::Duration::from_millis(80));

            // Handle modifier+key combos (e.g. "ctrl+r", "win+r")
            let parts: Vec<&str> = key.split('+').collect();
            let (modifiers, main) = if parts.len() > 1 {
                (&parts[..parts.len()-1], parts[parts.len()-1])
            } else {
                (&[][..], parts[0])
            };

            // Press modifiers
            for m in modifiers {
                let vk = modifier_vk(m);
                if vk != 0 { keybd_event(vk, 0, 0, 0); }
            }
            std::thread::sleep(std::time::Duration::from_millis(40));

            // Press main key
            let vk = named_vk(main);
            if vk != 0 {
                keybd_event(vk, 0, 0, 0);
                std::thread::sleep(std::time::Duration::from_millis(50));
                keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
            }

            // Release modifiers (reverse order)
            for m in modifiers.iter().rev() {
                let vk = modifier_vk(m);
                if vk != 0 { keybd_event(vk, 0, KEYEVENTF_KEYUP, 0); }
            }
        }
    }

    fn modifier_vk(name: &str) -> u8 {
        match name.to_lowercase().as_str() {
            "ctrl" | "control" => 0x11,
            "alt"              => 0x12,
            "shift"            => 0x10,
            "win" | "super"    => 0x5B, // VK_LWIN
            _ => 0,
        }
    }

    fn named_vk(name: &str) -> u8 {
        match name.to_lowercase().as_str() {
            "return" | "enter" | "\n" => 0x0D,
            "escape" | "esc"          => 0x1B,
            "tab"                     => 0x09,
            "backspace"               => 0x08,
            "delete" | "del"          => 0x2E,
            "space"                   => 0x20,
            "home"                    => 0x24,
            "end"                     => 0x23,
            "pageup"                  => 0x21,
            "pagedown"                => 0x22,
            "left"                    => 0x25,
            "up"                      => 0x26,
            "right"                   => 0x27,
            "down"                    => 0x28,
            "f1"  => 0x70, "f2"  => 0x71, "f3"  => 0x72, "f4"  => 0x73,
            "f5"  => 0x74, "f6"  => 0x75, "f7"  => 0x76, "f8"  => 0x77,
            "f9"  => 0x78, "f10" => 0x79, "f11" => 0x7A, "f12" => 0x7B,
            // Single character keys (a-z, 0-9)
            s if s.len() == 1 => s.chars().next().map(|c| c.to_ascii_uppercase() as u8).unwrap_or(0),
            _ => 0,
        }
    }
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

/// Returns a list of all currently open mstsc.exe windows.
#[tauri::command]
pub fn find_rdp_windows() -> Vec<RdpWindowInfo> {
    #[cfg(windows)]
    return win32::find_rdp_windows();
    #[cfg(not(windows))]
    return vec![];
}

/// Captures a screenshot of the given window (by HWND) and returns base64 PNG.
#[tauri::command]
pub async fn capture_rdp_screenshot(hwnd: u64) -> Result<String, String> {
    #[cfg(windows)]
    {
        let png = win32::capture_screenshot(hwnd)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(&png))
    }
    #[cfg(not(windows))]
    Err("Solo disponible en Windows".into())
}

// ─── Computer Use agentic loop ────────────────────────────────────────────────
//
// Implements the full Anthropic Computer Use loop:
//   screenshot → ask Claude → execute tool_use actions → screenshot → …
//
// Events fired during the loop:
//   "rdp_agent_step" { hwnd, kind, data, detail }

// ── Removed: computer_use_config (now handled by provider abstraction) ──────────

fn emit(app: &AppHandle, hwnd: u64, kind: &str, data: &str, detail: &str) {
    let _ = app.emit("rdp_agent_step", json!({
        "hwnd":   hwnd,
        "kind":   kind,
        "data":   data,
        "detail": detail,
    }));
}

#[tauri::command]
pub async fn run_rdp_agent(
    hwnd:      u64,
    task:      String,
    model:     Option<String>,
    max_steps: Option<u32>,
    app:       AppHandle,
) -> Result<String, String> {

    let model     = model.unwrap_or_else(|| "claude-opus-4-7".into());
    let max_steps = max_steps.unwrap_or(20);

    // Get window dimensions
    #[cfg(windows)]
    let (win_w, win_h) = {
        let wins = win32::find_rdp_windows();
        wins.iter().find(|w| w.hwnd == hwnd)
            .map(|w| (w.width, w.height))
            .unwrap_or((1920, 1080))
    };
    #[cfg(not(windows))]
    let (win_w, win_h) = (1920i32, 1080i32);

    // Create the appropriate provider based on model name
    let provider = create_provider(&model);
    emit(&app, hwnd, "action", "", &format!("[Agent] {} — iniciando", provider.name()));

    // Check credentials for the provider
    provider.check_credentials().await
        .map_err(|e| format!("[Agent] Credenciales: {}", e))?;

    emit(&app, hwnd, "action", "", "[Desktop] Capturando pantalla inicial…");

    // Take initial screenshot
    #[cfg(windows)]
    let initial_png = win32::capture_screenshot(hwnd)
        .map_err(|e| format!("[Desktop] Screenshot error: {}", e))?;
    #[cfg(not(windows))]
    return Err("[Desktop] Solo disponible en Windows".into());

    let initial_b64 = base64::engine::general_purpose::STANDARD.encode(&initial_png);
    emit(&app, hwnd, "screenshot", &initial_b64, "Pantalla inicial");

    // Build initial message for conversation
    let mut messages: Vec<Value> = vec![json!({
        "role": "user",
        "content": [
            {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": initial_b64
                }
            },
            {
                "type": "text",
                "text": format!(
                    "You are controlling a Windows Remote Desktop (mstsc) session.\n\
                     Display size: {}×{} pixels.\n\n\
                     Task: {}\n\n\
                     Analyze the screenshot and execute the task step by step.\n\
                     When the task is complete, describe what you accomplished.",
                    win_w, win_h, task
                )
            }
        ]
    })];

    let config = ComputeConfig {
        model: model.clone(),
        api_key: String::new(), // Handled by provider
        task: task.clone(),
        max_steps,
        window_width: win_w,
        window_height: win_h,
    };

    let mut step    = 0u32;
    let mut summary = String::new();

    loop {
        if step >= max_steps {
            emit(&app, hwnd, "done", "", &format!("Límite de {} pasos alcanzado.", max_steps));
            break;
        }
        step += 1;

        let provider_name = provider.name();
        emit(&app, hwnd, "action", "", &format!("[Agent] {} — paso {}/{}…", provider_name, step, max_steps));

        // Query the LLM provider
        let current_screenshot = if step == 1 {
            initial_b64.clone()
        } else {
            take_screenshot_b64(hwnd, &app)
        };

        match provider.query_llm(&config, &current_screenshot, &messages).await {
            Ok((actions, text_response, should_stop)) => {
                // Add assistant response to conversation
                if !text_response.is_empty() {
                    emit(&app, hwnd, "text", "", &text_response);
                    summary = text_response.clone();

                    // Add assistant message to history for next turn
                    messages.push(json!({
                        "role": "assistant",
                        "content": [{
                            "type": "text",
                            "text": text_response
                        }]
                    }));
                }

                // Execute each action
                if actions.is_empty() || should_stop {
                    emit(&app, hwnd, "done", "", &summary);
                    break;
                }

                for action in actions {
                    let screenshot_after = execute_action_impl(hwnd, action, &app).await;

                    // Build message turn with screenshot result
                    messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/png",
                                "data": screenshot_after
                            }
                        }]
                    }));

                    // Trim conversation to prevent token explosion
                    if messages.len() > 8 {
                        let first = messages[0].clone();
                        let tail = messages[messages.len()-6..].to_vec();
                        messages = std::iter::once(first).chain(tail).collect();
                    }
                }

            }
            Err(e) => {
                emit(&app, hwnd, "error", "", &format!("[Agent] Error: {}", e));
                return Err(format!("[Agent] {}", e));
            }
        }
    }

    Ok(summary)
}

/// Execute a single action and return the screenshot after execution
async fn execute_action_impl(hwnd: u64, action: ComputerAction, app: &AppHandle) -> String {
    use std::thread;

        match action {
            ComputerAction::Screenshot => {
                emit(app, hwnd, "action", "", "[Desktop] Screenshot…");
                thread::sleep(std::time::Duration::from_millis(400));
                take_screenshot_b64(hwnd, app)
            }

            ComputerAction::LeftClick { coordinate } => {
                emit(app, hwnd, "action", "", &format!("[Desktop] Click en ({}, {})", coordinate[0], coordinate[1]));
                #[cfg(windows)]
                win32::mouse_click(hwnd, coordinate[0], coordinate[1], "left", false);
                thread::sleep(std::time::Duration::from_millis(700));
                take_screenshot_b64(hwnd, app)
            }

            ComputerAction::RightClick { coordinate } => {
                emit(app, hwnd, "action", "", &format!("[Desktop] Right-click en ({}, {})", coordinate[0], coordinate[1]));
                #[cfg(windows)]
                win32::mouse_click(hwnd, coordinate[0], coordinate[1], "right", false);
                thread::sleep(std::time::Duration::from_millis(700));
                take_screenshot_b64(hwnd, app)
            }

            ComputerAction::DoubleClick { coordinate } => {
                emit(app, hwnd, "action", "", &format!("[Desktop] Double-click en ({}, {})", coordinate[0], coordinate[1]));
                #[cfg(windows)]
                win32::mouse_click(hwnd, coordinate[0], coordinate[1], "left", true);
                thread::sleep(std::time::Duration::from_millis(700));
                take_screenshot_b64(hwnd, app)
            }

            ComputerAction::LeftClickDrag { start_coordinate, end_coordinate } => {
                emit(app, hwnd, "action", "", &format!("[Desktop] Drag ({},{}) → ({},{})", start_coordinate[0], start_coordinate[1], end_coordinate[0], end_coordinate[1]));
                #[cfg(windows)]
                {
                    use winapi::um::winuser::{mouse_event, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP};
                    unsafe {
                        win32::mouse_move(hwnd, start_coordinate[0], start_coordinate[1]);
                        thread::sleep(std::time::Duration::from_millis(80));
                        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
                        thread::sleep(std::time::Duration::from_millis(60));
                        win32::mouse_move(hwnd, end_coordinate[0], end_coordinate[1]);
                        thread::sleep(std::time::Duration::from_millis(120));
                        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
                    }
                }
                thread::sleep(std::time::Duration::from_millis(700));
                take_screenshot_b64(hwnd, app)
            }

            ComputerAction::Type { text } => {
                let preview = if text.len() > 40 { &text[..40] } else { &text };
                emit(app, hwnd, "action", "", &format!("[Desktop] Escribiendo: \"{}\"{}",
                    preview, if text.len() > 40 { "…" } else { "" }));
                #[cfg(windows)]
                win32::type_text(hwnd, &text);
                thread::sleep(std::time::Duration::from_millis(600));
                take_screenshot_b64(hwnd, app)
            }

            ComputerAction::Key { text } => {
                emit(app, hwnd, "action", "", &format!("[Desktop] Tecla: {}", text));
                #[cfg(windows)]
                for k in text.split_whitespace() {
                    win32::send_key(hwnd, k);
                    thread::sleep(std::time::Duration::from_millis(100));
                }
                thread::sleep(std::time::Duration::from_millis(700));
                take_screenshot_b64(hwnd, app)
            }

            ComputerAction::MouseMove { coordinate } => {
                #[cfg(windows)]
                win32::mouse_move(hwnd, coordinate[0], coordinate[1]);
                take_screenshot_b64(hwnd, app)
            }

            ComputerAction::CursorPosition => {
                take_screenshot_b64(hwnd, app)
            }

            ComputerAction::MiddleClick { coordinate } => {
                emit(app, hwnd, "action", "", &format!("[Desktop] Middle-click en ({}, {})", coordinate[0], coordinate[1]));
                // Middle click not yet implemented in win32 module
                take_screenshot_b64(hwnd, app)
            }
        }
}

// ─── Helper: take screenshot, emit it, return base64 ─────────────────────────

fn take_screenshot_b64(hwnd: u64, app: &AppHandle) -> String {
    #[cfg(windows)]
    match win32::capture_screenshot(hwnd) {
        Ok(png) => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
            emit(app, hwnd, "screenshot", &b64, "");
            b64
        }
        Err(e) => {
            emit(app, hwnd, "error", "", &format!("Screenshot error: {}", e));
            String::new()
        }
    }
    #[cfg(not(windows))]
    String::new()
}
