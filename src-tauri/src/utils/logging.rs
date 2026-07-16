// ── LOGGING — Escritura de logs de aplicación y auditoría ─────────────────────

use std::fs::{OpenOptions, File};
use std::io::{Write, BufReader, BufWriter, copy as io_copy};
use chrono::Local;
use flate2::Compression;
use flate2::write::GzEncoder;

/// Compress a rotated log file in-place. After success the original `.log`
/// is replaced by `.log.gz` (best-effort: failure is logged silently so
/// log rotation never blocks the app).
///
/// Background-friendly: callers may wrap this in `std::thread::spawn` for
/// large files — gzip on a 10 MB log takes ~50–100 ms.
fn gzip_rotated_log(path: &std::path::Path) {
    let Ok(input) = File::open(path) else { return; };
    let out_path = {
        let mut p = path.to_path_buf();
        let new_name = format!(
            "{}.gz",
            p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        );
        p.set_file_name(new_name);
        p
    };
    let Ok(output) = File::create(&out_path) else { return; };
    let mut reader = BufReader::new(input);
    let mut encoder = GzEncoder::new(BufWriter::new(output), Compression::default());
    if io_copy(&mut reader, &mut encoder).is_err() { return; }
    if encoder.finish().is_err() { return; }
    // Original .log only gets removed once .gz exists successfully
    let _ = std::fs::remove_file(path);
}

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
/// Rota automáticamente si supera 5 MB y comprime el archivo histórico a .gz.
pub fn write_app_log(level: &str, message: &str) {
    let log_path = get_logs_dir().join("lucy_app.log");
    // v1.7.238 — rotación con 5 generaciones comprimidas (antes: 1). En operación
    // desatendida, una noche con un loop de errores ruidoso puede churnear varias
    // generaciones; conservar solo 1 borraba el INICIO del incidente — justo el
    // dato clave para el post-mortem. Reusa el helper genérico rotate_log (mismo
    // criterio que el audit log, que ya guarda 3).
    rotate_log("lucy_app.log", 5 * 1024 * 1024, 5);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{}] [{}] {}", ts, level, message);
    }
}

/// Rota el audit log si supera 10 MB — conserva hasta 3 archivos históricos
/// comprimidos a .gz para reducir uso de disco (~10× compresión típica).
pub fn rotate_audit_log() {
    let audit_path = get_logs_dir().join("lucy_audit.log");
    if let Ok(meta) = std::fs::metadata(&audit_path) {
        if meta.len() > 10 * 1024 * 1024 {
            let dir = get_logs_dir();
            // Shift compressed history backwards
            let _ = std::fs::remove_file(dir.join("lucy_audit.3.log.gz"));
            let _ = std::fs::rename(
                dir.join("lucy_audit.2.log.gz"),
                dir.join("lucy_audit.3.log.gz"),
            );
            let _ = std::fs::rename(
                dir.join("lucy_audit.1.log.gz"),
                dir.join("lucy_audit.2.log.gz"),
            );
            let rotated = dir.join("lucy_audit.1.log");
            if std::fs::rename(&audit_path, &rotated).is_ok() {
                std::thread::spawn(move || gzip_rotated_log(&rotated));
            }
        }
    }
}

/// Rota cualquier log con tamaño máximo configurable y N archivos históricos
/// (todos comprimidos a .gz tras la rotación, salvo el .log activo).
/// Útil para `lucy_agent_loop.log` y futuros logs verbose.
pub fn rotate_log(file_name: &str, max_size_bytes: u64, keep: usize) {
    let dir = get_logs_dir();
    let main = dir.join(file_name);
    let Ok(meta) = std::fs::metadata(&main) else { return; };
    if meta.len() <= max_size_bytes { return; }

    let stem = file_name.trim_end_matches(".log");
    // Discard the oldest beyond `keep`
    let _ = std::fs::remove_file(dir.join(format!("{}.{}.log.gz", stem, keep)));
    // Shift histories backwards: .{keep-1}.log.gz → .{keep}.log.gz, ...
    for i in (1..keep).rev() {
        let from = dir.join(format!("{}.{}.log.gz", stem, i));
        let to   = dir.join(format!("{}.{}.log.gz", stem, i + 1));
        let _ = std::fs::rename(&from, &to);
    }
    let rotated = dir.join(format!("{}.1.log", stem));
    if std::fs::rename(&main, &rotated).is_ok() {
        std::thread::spawn(move || gzip_rotated_log(&rotated));
    }
}
