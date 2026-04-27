// ── LOGGING — Escritura de logs de aplicación y auditoría ─────────────────────

use std::fs::OpenOptions;
use std::io::Write;
use chrono::Local;

/// Devuelve la ruta al directorio de logs: %APPDATA%\Lucy\logs\
/// Lo crea si no existe.
pub fn get_logs_dir() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(
        std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string())
    );
    path.push("Lucy");
    path.push("logs");
    let _ = std::fs::create_dir_all(&path);
    path
}

/// Escribe una línea en lucy_app.log con timestamp y nivel.
/// Rota automáticamente si supera 5 MB.
pub fn write_app_log(level: &str, message: &str) {
    let log_path = get_logs_dir().join("lucy_app.log");
    // Rotación simple: si supera 5 MB renombra a .1.log
    if let Ok(meta) = std::fs::metadata(&log_path) {
        if meta.len() > 5 * 1024 * 1024 {
            let _ = std::fs::rename(&log_path, get_logs_dir().join("lucy_app.1.log"));
        }
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{}] [{}] {}", ts, level, message);
    }
}

/// Rota el audit log si supera 10 MB — conserva hasta 3 archivos históricos.
pub fn rotate_audit_log() {
    let audit_path = get_logs_dir().join("lucy_audit.log");
    if let Ok(meta) = std::fs::metadata(&audit_path) {
        if meta.len() > 10 * 1024 * 1024 {
            let _ = std::fs::rename(
                get_logs_dir().join("lucy_audit.2.log"),
                get_logs_dir().join("lucy_audit.3.log"),
            );
            let _ = std::fs::rename(
                get_logs_dir().join("lucy_audit.1.log"),
                get_logs_dir().join("lucy_audit.2.log"),
            );
            let _ = std::fs::rename(&audit_path, get_logs_dir().join("lucy_audit.1.log"));
        }
    }
}

/// Rota cualquier log con tamaño máximo configurable y N archivos históricos.
/// Útil para `lucy_agent_loop.log` y futuros logs verbose.
pub fn rotate_log(file_name: &str, max_size_bytes: u64, keep: usize) {
    let dir = get_logs_dir();
    let main = dir.join(file_name);
    let Ok(meta) = std::fs::metadata(&main) else { return; };
    if meta.len() <= max_size_bytes { return; }

    // Shift histories backwards: .{keep-1}.log → discard, ..., .1.log → .2.log
    let stem = file_name.trim_end_matches(".log");
    for i in (1..keep).rev() {
        let from = dir.join(format!("{}.{}.log", stem, i));
        let to   = dir.join(format!("{}.{}.log", stem, i + 1));
        let _ = std::fs::rename(&from, &to);
    }
    let _ = std::fs::rename(&main, dir.join(format!("{}.1.log", stem)));
}
