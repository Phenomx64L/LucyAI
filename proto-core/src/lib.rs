//! Shared bits for the two prototypes: a live PTY (same `portable-pty` crate the
//! real Lucy app uses) and a realistic streaming-markdown sample. Toolkit-agnostic
//! on purpose — this is exactly the kind of code that survives the migration.

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

/// A running pseudo-terminal. A background thread pumps the shell's output into a
/// channel; the GUI drains it each frame (non-blocking). Mirrors the real app's
/// `commands/pty.rs` plumbing, minus the Tauri event glue.
pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    _child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
}

impl Pty {
    /// Spawn the default OS shell (PowerShell on Windows, bash elsewhere).
    pub fn spawn(cols: u16, rows: u16) -> Result<Self, String> {
        let sys = native_pty_system();
        let pair = sys
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("openpty: {e}"))?;

        let shell = if cfg!(windows) { "powershell.exe" } else { "bash" };
        let cmd = CommandBuilder::new(shell);
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn {shell}: {e}"))?;
        // Drop the slave handle so the PTY closes cleanly when the shell exits.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("clone reader: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("take writer: {e}"))?;

        let (tx, rx) = channel::<Vec<u8>>();
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,               // EOF — shell exited
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;                // GUI gone
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self { master: pair.master, _child: child, writer, rx })
    }

    /// Non-blocking: pull whatever the shell has emitted since the last call.
    /// Lossy UTF-8 (a real terminal would parse VT escapes — see `drain_bytes`).
    pub fn drain(&self) -> String {
        let mut out = String::new();
        while let Ok(chunk) = self.rx.try_recv() {
            out.push_str(&String::from_utf8_lossy(&chunk));
        }
        out
    }

    /// Non-blocking: pull the RAW bytes emitted since the last call — feed these
    /// to a VT emulator (e.g. `vt100`) so escape sequences are interpreted into a
    /// real terminal screen instead of shown as garbage.
    pub fn drain_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        while let Ok(chunk) = self.rx.try_recv() {
            out.extend_from_slice(&chunk);
        }
        out
    }

    /// Send input (append "\r" yourself to submit a line).
    pub fn send(&mut self, data: &str) {
        let _ = self.writer.write_all(data.as_bytes());
        let _ = self.writer.flush();
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self
            .master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }
}

/// A realistic Lucy answer that exercises the markdown features that matter:
/// headings, bold, inline code, a bullet list, a fenced code block, a table and
/// a blockquote. Streaming this char-by-char is the core fidelity/smoothness test.
pub fn sample_markdown() -> &'static str {
    "## Diagnóstico de red — PROD-LINUX\n\
\n\
Revisé el enlace **Ethernet** y encontré el problema de raíz:\n\
\n\
- El driver `e2xw` está colgado (no responde a `ethtool`)\n\
- La interfaz `eth0` quedó en estado `DOWN`\n\
- 3 reintentos de DHCP fallaron seguidos\n\
\n\
### Comando de reparación\n\
\n\
```bash\n\
sudo ip link set eth0 down \\\n\
  && sudo modprobe -r e2xw \\\n\
  && sudo modprobe e2xw \\\n\
  && sudo ip link set eth0 up\n\
```\n\
\n\
| Métrica  | Antes | Después |\n\
|----------|-------|---------|\n\
| Estado   | DOWN  | UP      |\n\
| Latencia | —     | 12 ms   |\n\
| Pérdida  | 100%  | 0%      |\n\
\n\
> ✅ Conectividad restaurada en el primer intento. ¿Quieres que lo deje\n\
> monitoreado y guarde el patrón como runbook para la próxima?\n"
}
