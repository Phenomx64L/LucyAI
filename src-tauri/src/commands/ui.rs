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

/// Open Windows Explorer with the given file highlighted — Dashboard → process
/// table → "open file location". GUI app, so no console window appears.
#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("Ruta vacía.".into());
    }
    if !std::path::Path::new(&path).exists() {
        return Err("La ruta ya no existe en disco.".into());
    }
    Command::new("explorer")
        .arg(format!("/select,{}", path))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("No se pudo abrir el explorador: {}", e))?;
    Ok(())
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

// v1.8.1 — Extensions offered by the attachment dialog.
//
// PDF was missing, which is why attaching one was impossible through the clip
// button; users worked around it by pasting an absolute path and asking Lucy
// to read the file herself. The text list also only covered a handful of
// formats, so ordinary SysAdmin material (.ps1, .yaml, .conf, .sql, .reg) was
// unattachable for no good reason.
//
// Keep IMAGE_EXTS in sync with the `image/*` mime built below — the frontend
// decides "render as picture vs. feed as text" purely from that mime prefix.
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];
const DOC_EXTS: &[&str] = &["pdf"];
const TEXT_EXTS: &[&str] = &[
    "txt", "md", "log", "trace", "csv", "tsv", "json", "xml", "yaml", "yml",
    "toml", "ini", "conf", "cfg", "properties", "env", "sql", "reg",
    "ps1", "psm1", "psd1", "sh", "bash", "bat", "cmd",
    "html", "htm", "css", "js", "ts", "py", "rs", "go", "java", "c", "h", "cpp",
];

fn all_supported_exts() -> Vec<&'static str> {
    let mut v = Vec::with_capacity(IMAGE_EXTS.len() + DOC_EXTS.len() + TEXT_EXTS.len());
    v.extend_from_slice(TEXT_EXTS);
    v.extend_from_slice(DOC_EXTS);
    v.extend_from_slice(IMAGE_EXTS);
    v
}

/// v1.8.1 — Read one user-picked file into the `(name, content, mime)` shape the
/// frontend attachment pipeline consumes.
///
/// The mime is the CONTRACT that tells the frontend how to treat `content`:
///   • `image/*`         → `content` is base64, render a thumbnail, send as vision input
///   • `application/pdf` → `content` is ALREADY-EXTRACTED TEXT, show a PDF chip
///   • `text/plain`      → `content` is the file text
///
/// That distinction matters: the old code classified anything that was not
/// `text/plain` as an image, so a PDF became a fake "image" whose base64 was
/// never sent anywhere useful.
///
/// Errors are returned rather than swallowed. Previously an unreadable file was
/// written to the log and dropped, so the composer showed a chip for a file the
/// model would never see — a silent failure the user could only detect by
/// noticing Lucy answering about nothing.
fn read_one_for_attach(path: &std::path::Path) -> Result<(String, String, String), String> {
    let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let ext = path.extension().unwrap_or_default().to_string_lossy().to_lowercase();

    if IMAGE_EXTS.contains(&ext.as_str()) {
        use base64::{Engine as _, engine::general_purpose};
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {}", file_name, e))?;
        let b64 = general_purpose::STANDARD.encode(&bytes);
        let mime = format!("image/{}", if ext == "jpg" { "jpeg".to_string() } else { ext.clone() });
        return Ok((file_name, b64, mime));
    }

    if DOC_EXTS.contains(&ext.as_str()) {
        // Extract here, in the backend, so the model receives real text instead
        // of the mojibake `readAsText` produced from PDF bytes.
        let text = crate::commands::pdf::extract_pdf_text(path)?;
        return Ok((file_name, text, "application/pdf".to_string()));
    }

    // Everything else: treat as text, but fail loudly on non-UTF-8 rather than
    // handing the model a corrupted blob.
    match std::fs::read_to_string(path) {
        Ok(content) => Ok((file_name, content, "text/plain".to_string())),
        Err(e) => Err(format!(
            "'{}' no es texto legible ({}). Si es un documento binario, conviértelo \
             o adjunta un PDF —  Lucy extrae su texto automáticamente.",
            file_name, e
        )),
    }
}

/// Abre un diálogo de selección de un archivo y devuelve (nombre, contenido/base64, mime).
/// Para imágenes devuelve base64; para PDF el texto extraído; para texto el contenido.
#[tauri::command]
pub fn pick_and_read_file() -> Result<Option<(String, String, String)>, String> {
    match rfd::FileDialog::new()
        .add_filter("Archivos soportados", &all_supported_exts())
        .add_filter("Todos los archivos", &["*"])
        .pick_file()
    {
        Some(path) => read_one_for_attach(&path).map(Some),
        None => Ok(None),
    }
}

/// Abre un diálogo de selección múltiple y devuelve Vec de (nombre, contenido/base64, mime).
///
/// Partial success is intentional: with several files selected, one bad file
/// must not lose the others. Unreadable ones are reported back through the
/// `__error__` mime so the frontend can toast them by name — the previous
/// behaviour (log line, silent drop) left the user with no signal at all.
#[tauri::command]
pub fn pick_multiple_files() -> Result<Vec<(String, String, String)>, String> {
    let paths = rfd::FileDialog::new()
        .add_filter("Archivos soportados", &all_supported_exts())
        .add_filter("Todos los archivos", &["*"])
        .pick_files();
    let paths = match paths {
        Some(p) if !p.is_empty() => p,
        _ => return Ok(vec![]),
    };
    let mut results = Vec::new();
    for path in paths {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        match read_one_for_attach(&path) {
            Ok(triple) => results.push(triple),
            Err(e) => {
                write_app_log("WARNING", &format!("Adjunto descartado {}: {}", file_name, e));
                results.push((file_name, e, "__error__".to_string()));
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

/// Sprint A — Native "save as" dialog for the DB backup feature.
///
/// Returns the absolute path the user picked, or empty string on cancel.
/// Filters limit the chosen extension to SQLite-friendly suffixes.
#[tauri::command]
pub fn pick_save_path(default_name: String, extensions: Vec<String>) -> Result<String, String> {
    let exts_borrow: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
    let mut dlg = rfd::FileDialog::new()
        .set_title("Save as")
        .set_file_name(&default_name);
    if !exts_borrow.is_empty() {
        dlg = dlg.add_filter("SQLite DB", &exts_borrow);
    }
    match dlg.save_file() {
        Some(p) => Ok(p.to_string_lossy().to_string()),
        None => Ok(String::new()),
    }
}

/// Sprint A — Native folder picker for the support bundle feature.
#[tauri::command]
pub fn pick_folder_path(title: Option<String>) -> Result<String, String> {
    let mut dlg = rfd::FileDialog::new();
    if let Some(t) = title { dlg = dlg.set_title(t); }
    match dlg.pick_folder() {
        Some(p) => Ok(p.to_string_lossy().to_string()),
        None => Ok(String::new()),
    }
}

/// Sprint A — Pick an existing file with extension filter (for DB restore).
#[tauri::command]
pub fn pick_file_with_filter(extensions: Vec<String>) -> Result<String, String> {
    let exts_borrow: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
    let mut dlg = rfd::FileDialog::new();
    if !exts_borrow.is_empty() {
        dlg = dlg.add_filter("Supported", &exts_borrow);
    }
    match dlg.pick_file() {
        Some(p) => Ok(p.to_string_lossy().to_string()),
        None => Ok(String::new()),
    }
}

/// Escribe bytes (base64) en un archivo PDF temporal y devuelve la ruta absoluta.
/// Lo usa PdfIngestPanel cuando se arrastra un PDF desde el explorador, ya que
/// Tauri no expone `File.path` en el webview.
#[tauri::command]
pub fn save_temp_pdf(filename: String, data_b64: String) -> Result<String, String> {
    use base64::{Engine as _, engine::general_purpose};
    // Sanitiza el nombre: solo conserva alfanuméricos, guiones, puntos y _ ; obliga .pdf
    let safe: String = filename.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' { c } else { '_' })
        .collect();
    let safe = if safe.to_lowercase().ends_with(".pdf") { safe } else { format!("{}.pdf", safe) };
    let bytes = general_purpose::STANDARD.decode(&data_b64)
        .map_err(|e| format!("Base64 inválido: {}", e))?;
    // Validación mínima: cabecera %PDF
    if bytes.len() < 5 || &bytes[..5] != b"%PDF-" {
        return Err("El archivo no parece ser un PDF válido (falta cabecera %PDF).".to_string());
    }
    let mut path = std::env::temp_dir();
    path.push(format!("lucy_pdf_{}_{}", std::process::id(), safe));
    std::fs::write(&path, &bytes)
        .map_err(|e| format!("Error escribiendo PDF temporal: {}", e))?;
    Ok(path.to_string_lossy().to_string())
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
