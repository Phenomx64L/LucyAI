// ── UI — Ventana, portapapeles y selector de archivos ─────────────────────────

use std::process::Command;
use std::os::windows::process::CommandExt;
use tauri::Manager;
use crate::state::CREATE_NO_WINDOW;
use crate::utils::logging::write_app_log;
use crate::utils::shell::urlencoding_simple;

// ── PORTAPAPELES ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn copy_to_clipboard(text: String) -> Result<(), String> {
    let escaped = text.replace('\'', "''");
    let script = format!("Set-Clipboard -Value '{}'", escaped);
    let output = Command::new("powershell")
        .arg("-NoProfile").arg("-ExecutionPolicy").arg("Bypass")
        .arg("-Command").arg(&script)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Error portapapeles: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("Set-Clipboard error: {}", String::from_utf8_lossy(&output.stderr)))
    }
}

// ── VENTANA ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn minimize_window(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn maximize_window(window: tauri::Window) {
    if let Ok(maximized) = window.is_maximized() {
        if maximized {
            let _ = window.unmaximize();
        } else {
            let _ = window.maximize();
        }
    }
}

#[tauri::command]
pub fn close_window(window: tauri::Window) {
    let _ = window.close();
}

/// Abre la shell remota de un host en una ventana nativa independiente.
/// Si ya existe una ventana para ese host la enfoca en lugar de crear una nueva.
#[tauri::command]
pub fn open_shell_window(
    app: tauri::AppHandle,
    host_id: String,
    host_name: String,
) -> Result<(), String> {
    let label = format!(
        "shell_{}",
        host_id
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    // Si ya existe una ventana para este host, enfocarla
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.set_focus().ok();
        return Ok(());
    }
    let name_enc = urlencoding_simple(&host_name);
    let path = format!("/#shell={}&name={}", host_id, name_enc);
    tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App(path.into()))
        .title(format!("Lucy Shell — {}", host_name))
        .inner_size(900.0, 700.0)
        .min_inner_size(600.0, 400.0)
        .resizable(true)
        .decorations(true)
        .build()
        .map_err(|e| format!("Error creando ventana: {}", e))?;
    Ok(())
}

// ── SELECTOR DE ARCHIVOS ──────────────────────────────────────────────────────

/// Abre un diálogo de selección de un archivo y devuelve (nombre, contenido/base64, mime).
/// Para imágenes devuelve base64; para texto devuelve el contenido directamente.
#[tauri::command]
pub fn pick_and_read_file() -> Result<Option<(String, String, String)>, String> {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter(
            "Archivos Soportados",
            &["xml","csv","txt","json","log","md","trace","png","jpg","jpeg","webp"],
        )
        .pick_file()
    {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let ext = path.extension().unwrap_or_default().to_string_lossy().to_lowercase();
        if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
            use base64::{Engine as _, engine::general_purpose};
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("Error leyendo imagen: {}", e))?;
            let b64 = general_purpose::STANDARD.encode(&bytes);
            let mime = format!("image/{}", if ext == "jpg" { "jpeg" } else { &ext });
            Ok(Some((file_name, b64, mime)))
        } else {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("Error leyendo texto: {}", e))?;
            Ok(Some((file_name, content, "text/plain".to_string())))
        }
    } else {
        Ok(None)
    }
}

/// Abre un diálogo de selección múltiple y devuelve Vec de (nombre, contenido/base64, mime).
#[tauri::command]
pub fn pick_multiple_files() -> Result<Vec<(String, String, String)>, String> {
    let paths = rfd::FileDialog::new()
        .add_filter(
            "Archivos Soportados",
            &["xml","csv","txt","json","log","md","trace","png","jpg","jpeg","webp"],
        )
        .pick_files();
    let paths = match paths {
        Some(p) if !p.is_empty() => p,
        _ => return Ok(vec![]),
    };
    let mut results = Vec::new();
    for path in paths {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let ext = path.extension().unwrap_or_default().to_string_lossy().to_lowercase();
        if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
            use base64::{Engine as _, engine::general_purpose};
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let b64 = general_purpose::STANDARD.encode(&bytes);
                    let mime = format!("image/{}", if ext == "jpg" { "jpeg" } else { &ext });
                    results.push((file_name, b64, mime));
                }
                Err(e) => write_app_log(
                    "WARNING",
                    &format!("No se pudo leer imagen {}: {}", file_name, e),
                ),
            }
        } else {
            match std::fs::read_to_string(&path) {
                Ok(content) => results.push((file_name, content, "text/plain".to_string())),
                Err(e) => write_app_log(
                    "WARNING",
                    &format!("No se pudo leer archivo {}: {}", file_name, e),
                ),
            }
        }
    }
    Ok(results)
}

/// Abre un diálogo de selección de archivo y devuelve solo la ruta (para transferencias SSH).
#[tauri::command]
pub fn pick_file_path() -> Result<String, String> {
    match rfd::FileDialog::new()
        .set_title("Seleccionar archivo para transferir")
        .pick_file()
    {
        Some(p) => Ok(p.to_string_lossy().to_string()),
        None => Ok(String::new()),
    }
}

/// Diálogo especializado para seleccionar archivos PDF.
/// Filtra por extensión .pdf y muestra un título contextual.
#[tauri::command]
pub fn pick_pdf_path() -> Result<String, String> {
    match rfd::FileDialog::new()
        .set_title("Seleccionar documento PDF")
        .add_filter("Documentos PDF", &["pdf"])
        .pick_file()
    {
        Some(p) => Ok(p.to_string_lossy().to_string()),
        None    => Ok(String::new()),  // empty = user cancelled
    }
}

/// Abre un diálogo "Guardar como…" y escribe bytes (base64) en la ruta elegida.
#[tauri::command]
pub fn save_file_dialog(default_name: String, data_b64: String, filter_name: String, extensions: Vec<String>) -> Result<String, String> {
    use base64::{Engine as _, engine::general_purpose};
    let ext_refs: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
    let path = rfd::FileDialog::new()
        .set_title("Guardar archivo")
        .set_file_name(&default_name)
        .add_filter(&filter_name, &ext_refs)
        .save_file()
        .ok_or_else(|| "Cancelled".to_string())?;
    let bytes = general_purpose::STANDARD.decode(&data_b64)
        .map_err(|e| format!("Base64 decode error: {}", e))?;
    std::fs::write(&path, &bytes)
        .map_err(|e| format!("Error escribiendo archivo: {}", e))?;
    write_app_log("INFO", &format!("Archivo guardado: {} ({} bytes)", path.display(), bytes.len()));
    Ok(path.to_string_lossy().to_string())
}

/// Abre un diálogo para seleccionar un directorio de Runbooks (o cualquier otra carpeta local).
#[tauri::command]
pub fn pick_directory() -> Result<String, String> {
    match rfd::FileDialog::new()
        .set_title("Seleccionar directorio de Runbooks")
        .pick_folder()
    {
        Some(p) => Ok(p.to_string_lossy().to_string()),
        None => Ok(String::new()),
    }
}
