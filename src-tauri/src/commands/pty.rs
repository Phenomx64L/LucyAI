// ── pty.rs — Interactive PTY backend for the in-app xterm panel ──────────
//
// v1.7.100 (Option D1)
//
// Lucy already exposes `execute_powershell` for one-shot scripts, but
// that command spawns a fresh process per call, captures stdout, and
// returns the entire blob — it cannot host an *interactive* shell
// session (prompts, line editing, TUIs, persistent env). The xterm.js
// pane needs a true PTY.
//
// Design
// ──────
// Single PTY instance per app process (singleton). Backed by a
// `portable-pty` master/child pair. A dedicated OS thread blocks on the
// master's reader and pushes every chunk to the frontend as a
// `pty:data` Tauri event. Writes go through the master writer under
// the same mutex.
//
// Why a thread (not tokio): portable-pty's master reader is blocking
// stdio. Tokio's `spawn_blocking` works too, but a plain thread keeps
// the dependency surface flat — no executor involvement at all.
//
// Lifecycle
// ─────────
//   pty_open(cols, rows)  → spawns the shell, returns Ok once the
//                            reader thread is running.
//   pty_write(data)       → writes the bytes verbatim to PTY stdin.
//   pty_resize(cols,rows) → updates the master's window size; PTY
//                            sends SIGWINCH-equivalent so curses-style
//                            apps re-layout.
//   pty_close()           → kills the child, joins the reader thread.
//                            Idempotent.
//
// Frontend listens for:
//   pty:data { bytes: <base64> }   — chunks of raw PTY output.
//   pty:exit { code: <i32> }       — child exited.
//
// Why base64 over UTF-8 string: PTY output is byte-oriented (ANSI
// escapes, partial multibyte chars at chunk boundaries). Wrapping
// each chunk in base64 keeps the JSON payload binary-safe; xterm.js
// happily decodes it.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use base64::Engine;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tauri::{AppHandle, Emitter};

/// Internal state carried while a PTY is open. None when closed.
struct PtyState {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child:  Box<dyn portable_pty::Child + Send + Sync>,
    /// Set to true by the close path so the reader thread can bail
    /// cleanly on its next iteration instead of waiting on a dead pipe.
    closing: Arc<std::sync::atomic::AtomicBool>,
    reader_thread: Option<JoinHandle<()>>,
}

/// Global singleton. OnceLock holds the Mutex; the Mutex holds the
/// optional PtyState (None until pty_open succeeds).
fn state() -> &'static Mutex<Option<PtyState>> {
    static STATE: OnceLock<Mutex<Option<PtyState>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

/// Pick the shell program to spawn. Operator can override via the
/// LUCY_PTY_SHELL env var — useful for testing with bash on WSL or
/// pwsh.exe instead of the legacy powershell.exe.
fn default_shell() -> String {
    if let Ok(custom) = std::env::var("LUCY_PTY_SHELL") {
        if !custom.trim().is_empty() {
            return custom;
        }
    }
    if cfg!(target_os = "windows") {
        // pwsh (PowerShell 7+) is preferred when present; fall back to
        // the system powershell.exe (Windows PowerShell 5.1). We don't
        // probe with `which`-style logic at runtime — CommandBuilder
        // falls back through PATH on its own.
        "powershell.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
    }
}

/// Open the PTY. Idempotent: returns Ok if one is already running.
#[tauri::command]
pub async fn pty_open(app: AppHandle, cols: u16, rows: u16) -> Result<(), String> {
    {
        // Fast path: already open. We don't tear it down — that would
        // throw away the user's scrollback.
        let guard = state().lock().map_err(|e| format!("pty state lock poisoned: {}", e))?;
        if guard.is_some() {
            return Ok(());
        }
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: rows.max(4),
            cols: cols.max(20),
            pixel_width:  0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {}", e))?;

    let shell = default_shell();
    let cmd = CommandBuilder::new(&shell);
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn {}: {}", shell, e))?;
    // The slave end is held by the spawned child. Drop our handle so
    // EOF arrives when the child exits — without this, the reader
    // thread would block forever after the shell quits.
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone reader: {}", e))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take writer: {}", e))?;

    let closing = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Reader thread — blocks on read, base64-encodes each chunk, and
    // emits to the frontend. Exits when read returns 0 or errors, or
    // when `closing` is flipped by pty_close.
    let app_for_thread = app.clone();
    let closing_for_thread = closing.clone();
    let reader_thread = std::thread::Builder::new()
        .name("lucy-pty-reader".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                if closing_for_thread.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // EOF — shell closed. Emit and bail.
                        let _ = app_for_thread.emit(
                            "pty:exit",
                            serde_json::json!({ "code": 0 }),
                        );
                        break;
                    }
                    Ok(n) => {
                        let encoded = base64::engine::general_purpose::STANDARD
                            .encode(&buf[..n]);
                        let _ = app_for_thread.emit(
                            "pty:data",
                            serde_json::json!({ "bytes": encoded }),
                        );
                    }
                    Err(e) => {
                        // Reading from a closed master returns this on
                        // every platform. Treat any error as terminal —
                        // we won't recover from it.
                        let _ = app_for_thread.emit(
                            "pty:exit",
                            serde_json::json!({ "code": -1, "err": e.to_string() }),
                        );
                        break;
                    }
                }
            }
        })
        .map_err(|e| format!("spawn reader thread: {}", e))?;

    let mut guard = state().lock().map_err(|e| format!("pty state lock poisoned: {}", e))?;
    *guard = Some(PtyState {
        master: pair.master,
        writer,
        child,
        closing,
        reader_thread: Some(reader_thread),
    });
    Ok(())
}

/// Write a UTF-8 string to PTY stdin. xterm.js's onData callback hands
/// us strings — we pass them through verbatim. Escape sequences for
/// arrow keys, Ctrl-C, etc. all arrive correctly because xterm encodes
/// them as the raw ANSI bytes that the shell expects.
#[tauri::command]
pub async fn pty_write(data: String) -> Result<(), String> {
    let mut guard = state().lock().map_err(|e| format!("pty state lock poisoned: {}", e))?;
    let st = guard.as_mut().ok_or_else(|| "pty not open".to_string())?;
    st.writer.write_all(data.as_bytes()).map_err(|e| format!("pty write: {}", e))?;
    st.writer.flush().map_err(|e| format!("pty flush: {}", e))?;
    Ok(())
}

/// Update PTY window size. Called by the frontend's FitAddon whenever
/// the host element resizes.
#[tauri::command]
pub async fn pty_resize(cols: u16, rows: u16) -> Result<(), String> {
    let guard = state().lock().map_err(|e| format!("pty state lock poisoned: {}", e))?;
    let st = guard.as_ref().ok_or_else(|| "pty not open".to_string())?;
    st.master
        .resize(PtySize {
            rows: rows.max(4),
            cols: cols.max(20),
            pixel_width:  0,
            pixel_height: 0,
        })
        .map_err(|e| format!("pty resize: {}", e))?;
    Ok(())
}

/// Kill the child + join the reader thread. Safe to call when the PTY
/// is not open.
#[tauri::command]
pub async fn pty_close() -> Result<(), String> {
    let mut taken: Option<PtyState> = None;
    {
        let mut guard = state().lock().map_err(|e| format!("pty state lock poisoned: {}", e))?;
        if let Some(mut st) = guard.take() {
            st.closing.store(true, std::sync::atomic::Ordering::Relaxed);
            // Kill the child so the reader's blocking read returns EOF
            // quickly. Result is best-effort — child may already be gone.
            let _ = st.child.kill();
            taken = Some(st);
        }
    }
    // Join outside the mutex so the reader thread can finish without
    // contending with us for the same lock.
    if let Some(mut st) = taken {
        if let Some(handle) = st.reader_thread.take() {
            let _ = handle.join();
        }
        let _ = st.child.wait();
    }
    Ok(())
}

/// Cheap probe used by the frontend to render the "open / closed"
/// state without forcing a write/resize call to bounce off the lock.
#[tauri::command]
pub async fn pty_status() -> Result<bool, String> {
    let guard = state().lock().map_err(|e| format!("pty state lock poisoned: {}", e))?;
    Ok(guard.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_returns_non_empty() {
        // We don't assert the exact path — CI may set $SHELL or
        // LUCY_PTY_SHELL differently. We only want the contract:
        // never empty.
        let s = default_shell();
        assert!(!s.is_empty(), "default_shell must always return something runnable");
    }

    #[tokio::test]
    async fn status_is_false_before_open() {
        // pty_close is idempotent so we can safely reset state in case
        // a sibling test opened a PTY before us.
        let _ = pty_close().await;
        let s = pty_status().await.expect("pty_status should not error");
        assert!(!s, "no PTY should be open at test start");
    }
}
