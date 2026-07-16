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
///
/// v1.7.101 Sprint-1 B6 fix: the `writer` was previously a field of this
/// struct and `pty_write` held the global STATE mutex across the
/// blocking `write_all + flush`. A stuck shell (full pipe, paused
/// stdout reader) would deadlock every other command — including
/// pty_close and pty_status — behind the writer.
///
/// We now keep the writer in a SEPARATE mutex (`writer()` below) so
/// long-running writes don't block close/resize/status. We also wrap
/// the actual write/resize syscalls in `spawn_blocking` so they don't
/// stall the tokio runtime.
struct PtyState {
    master: Box<dyn portable_pty::MasterPty + Send>,
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

/// Separate writer mutex (v1.7.101 B6). Decoupled from `state()` so the
/// blocking `write_all` path can't starve `pty_close` / `pty_status` /
/// `pty_resize`. Held only briefly to grab a `&mut` to the writer, then
/// the actual I/O runs inside `spawn_blocking` while still holding the
/// guard — but only that thread is blocked, never the tokio executor.
fn writer() -> &'static Mutex<Option<Box<dyn Write + Send>>> {
    static WRITER: OnceLock<Mutex<Option<Box<dyn Write + Send>>>> = OnceLock::new();
    WRITER.get_or_init(|| Mutex::new(None))
}

/// Per-line audit buffer (v1.7.103 Sprint-3 H1 follow-up). Sprint #2
/// added a `[PTY_OPEN]` log at shell launch; this hook records what
/// the operator actually typed once a newline lands. We buffer raw
/// bytes (xterm sends partial multibyte runes on chunk boundaries,
/// arrow keys are 3-byte CSI sequences, etc.) and only emit complete
/// lines.
///
/// Cap at 8 KiB so a runaway paste of binary data can't grow the
/// buffer unboundedly. When the cap trips we log a single `truncated`
/// marker and reset.
const AUDIT_BUF_MAX: usize = 8 * 1024;
fn audit_buf() -> &'static Mutex<Vec<u8>> {
    static BUF: OnceLock<Mutex<Vec<u8>>> = OnceLock::new();
    BUF.get_or_init(|| Mutex::new(Vec::with_capacity(256)))
}

/// Drain accumulated bytes up to (and including) every newline, log
/// each complete line, and keep the tail. Called from `pty_write` AFTER
/// the bytes have been pushed onto the PTY successfully so a logging
/// failure can't block real shell input.
fn flush_audit_lines(buf: &mut Vec<u8>) {
    loop {
        let Some(nl) = buf.iter().position(|&b| b == b'\n' || b == b'\r') else {
            // No newline yet — keep buffering. Trim if we've blown the cap
            // (rare; protects against long runaway pastes).
            if buf.len() > AUDIT_BUF_MAX {
                crate::utils::logging::write_app_log(
                    "WARN",
                    &format!("[PTY_INPUT_TRUNCATED] dropped {} unflushed bytes", buf.len()),
                );
                buf.clear();
            }
            return;
        };
        let line_bytes: Vec<u8> = buf.drain(..=nl).collect();
        // Skip the newline itself + strip any solitary \r that prefixed
        // a \n (windows-style line ending typed in the embedded shell).
        let line_str = String::from_utf8_lossy(
            line_bytes.iter().filter(|&&b| b != b'\n' && b != b'\r').copied().collect::<Vec<u8>>().as_slice()
        ).to_string();
        let trimmed = line_str.trim();
        if trimmed.is_empty() { continue; }   // bare Enter / blank line
        // Cap individual line length — paranoid against pathological
        // single-line pastes that bypassed the buffer cap.
        let logged = if trimmed.len() > 1024 {
            format!("{}…", crate::utils::safe_truncate(trimmed, 1021))
        } else {
            trimmed.to_string()
        };
        crate::utils::logging::write_app_log(
            "INFO",
            &format!("[PTY_INPUT] {}", logged),
        );
    }
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
    // v1.7.102 Sprint-2 H1: audit every PTY shell spawn. Without this
    // hook the interactive xterm pane bypasses Lucy's command-execution
    // audit chain entirely — operator-curated permission rules and
    // bypass-token UI don't apply to keystrokes typed into the panel.
    // This is the first checkpoint: we record WHICH shell got launched,
    // when, and with which environment knob. Per-keystroke / per-line
    // audit is bigger architectural work (deferred to a future sprint).
    crate::utils::logging::write_app_log(
        "INFO",
        &format!(
            "[PTY_OPEN] shell={} cols={} rows={} env_override={}",
            shell, cols, rows,
            std::env::var("LUCY_PTY_SHELL").is_ok()
        ),
    );
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
    // Renamed v1.7.101 B6 to avoid shadowing the new `writer()` static accessor.
    let writer_box: Box<dyn Write + Send> = pair
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

    // v1.7.101 B6 — install state and writer separately. Order matters:
    // writer first, so any concurrent pty_write call that sneaks in
    // between these two assignments sees "writer set, state set" or
    // "writer unset" — never "state set but writer absent".
    {
        let mut wguard = writer().lock().map_err(|e| format!("pty writer lock poisoned: {}", e))?;
        *wguard = Some(writer_box);
    }
    let mut guard = state().lock().map_err(|e| format!("pty state lock poisoned: {}", e))?;
    *guard = Some(PtyState {
        master: pair.master,
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
///
/// v1.7.101 B6: the actual write runs on `spawn_blocking` so we never
/// stall the tokio runtime, and we lock the SEPARATE writer mutex
/// (`writer()`) so a stuck shell can't block pty_close / pty_resize.
#[tauri::command]
pub async fn pty_write(data: String) -> Result<(), String> {
    let bytes = data.into_bytes();
    tauri::async_runtime::spawn_blocking(move || {
        // 1. Push bytes to the PTY first — if the shell hangs we still
        //    want the write attempt to be the visible error, not an
        //    audit-buffer hiccup.
        {
            let mut guard = writer().lock().map_err(|e| format!("pty writer lock poisoned: {}", e))?;
            let w = guard.as_mut().ok_or_else(|| "pty not open".to_string())?;
            w.write_all(&bytes).map_err(|e| format!("pty write: {}", e))?;
            w.flush().map_err(|e| format!("pty flush: {}", e))?;
        }
        // 2. v1.7.103 H1 follow-up: per-line audit. We only log lines
        //    (post-newline) so arrow keys + Ctrl-C + partial typing
        //    stay out of the log. The audit buffer mutex is distinct
        //    from the writer mutex so this can never block real I/O.
        if let Ok(mut buf) = audit_buf().lock() {
            buf.extend_from_slice(&bytes);
            flush_audit_lines(&mut buf);
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("pty_write join: {}", e))?
}

/// Update PTY window size. Called by the frontend's FitAddon whenever
/// the host element resizes.
///
/// v1.7.101 B6: `master.resize` issues a synchronous IOCTL on Windows
/// ConPTY — moved to spawn_blocking so a slow resize never stalls the
/// async executor. We still take the state mutex briefly to grab a
/// clone-able size, then release before doing the syscall.
#[tauri::command]
pub async fn pty_resize(cols: u16, rows: u16) -> Result<(), String> {
    let size = PtySize {
        rows: rows.max(4),
        cols: cols.max(20),
        pixel_width:  0,
        pixel_height: 0,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let guard = state().lock().map_err(|e| format!("pty state lock poisoned: {}", e))?;
        let st = guard.as_ref().ok_or_else(|| "pty not open".to_string())?;
        st.master.resize(size).map_err(|e| format!("pty resize: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("pty_resize join: {}", e))?
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
    // v1.7.101 B6: also drop the writer so pty_write returns "not open"
    // promptly after close. Acquired in a separate scope so it doesn't
    // serialize with the state lock or with the join below.
    {
        let mut wguard = writer().lock().map_err(|e| format!("pty writer lock poisoned: {}", e))?;
        *wguard = None;
    }
    // v1.7.103 H1 follow-up: flush whatever was in the audit buffer
    // (anything typed without a final Enter) and reset for the next
    // session.
    if let Ok(mut buf) = audit_buf().lock() {
        if !buf.is_empty() {
            // Force a synthetic newline so flush_audit_lines emits the tail.
            buf.push(b'\n');
            flush_audit_lines(&mut buf);
        }
        crate::utils::logging::write_app_log("INFO", "[PTY_CLOSE]");
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

    #[test]
    fn audit_buffer_flushes_complete_lines_only() {
        // v1.7.103 H1 follow-up regression test. Partial input must NOT
        // emit a log line; only completed lines do.
        let mut buf: Vec<u8> = b"echo h".to_vec();
        flush_audit_lines(&mut buf);
        // No newline yet — buffer untouched.
        assert_eq!(buf, b"echo h", "partial line should not flush");

        // Append the rest + newline. We can't observe the log line
        // directly, but we can verify the buffer is drained.
        buf.extend_from_slice(b"ello\n");
        flush_audit_lines(&mut buf);
        assert!(buf.is_empty(), "complete line should drain the buffer");

        // Mid-line again — leftover stays buffered for next flush.
        buf.extend_from_slice(b"echo world\nfoo");
        flush_audit_lines(&mut buf);
        assert_eq!(buf, b"foo", "tail after final newline stays buffered");
    }

    #[test]
    fn audit_buffer_caps_runaway_paste() {
        // Bigger than AUDIT_BUF_MAX, no newline → buffer must be cleared.
        let huge: Vec<u8> = vec![b'x'; AUDIT_BUF_MAX + 100];
        let mut buf = huge;
        flush_audit_lines(&mut buf);
        assert!(buf.is_empty(), "runaway paste over cap must be dropped");
    }

    #[tokio::test]
    async fn write_when_closed_returns_not_open() {
        // v1.7.101 Sprint-1 B6 regression test: with the writer in its
        // own mutex, the not-open path should still surface a clean
        // "pty not open" instead of any deadlock / wrong-error symptom.
        let _ = pty_close().await;
        let r = pty_write("hello\n".into()).await;
        assert!(r.is_err(), "writing to a closed PTY must error");
        let msg = r.unwrap_err();
        assert!(msg.contains("not open"), "got unexpected error: {}", msg);
    }
}
