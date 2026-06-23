// ── mcp.rs — Model Context Protocol integration ─────────────────────────
//
// Two layers coexist here:
//
//   • Legacy (raw-command) layer
//       call_mcp_tool / discover_mcp_tools — accept the full server
//       command verbatim ("npx -y @mcp/server-fs C:/path"). Kept for
//       backwards compatibility with prompts that emit the raw command.
//
//   • First-class registry layer (v1.4.2)
//       mcp_server_list / mcp_server_upsert / mcp_server_delete
//       mcp_server_discover / mcp_server_test / mcp_server_call
//       Backed by the `mcp_servers` SQLite table. Equivalent UX to the
//       claude_desktop_config.json model: users register a named server
//       once, and Lucy invokes it by name. Tools discovered from
//       tools/list are cached so the system prompt can list available
//       capabilities without re-spawning the subprocess every turn.
//
// Why both? The agent loop has been emitting raw-command MCP calls for
// months — removing that would break existing prompts and skills. The
// registry layer is the new default surfaced in the UI, but the old
// path is still wired so nothing breaks during the transition.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, ChildStderr, Command};
use tokio::sync::Mutex as AsyncMutex;

use rusqlite::params;

use crate::commands::metrics::shared_db;

// ── Shared helpers ──────────────────────────────────────────────────────

/// v1.7.102 Sprint-2 B5: registry resolver.
///
/// Before this fix, `call_mcp_tool` / `discover_mcp_tools` accepted ANY
/// string as `server_name` and ran it as a subprocess via
/// `spawn_and_initialize` — bypassing the operator-curated `mcp_servers`
/// registry, the bypass-token UI, the blocklist, and the audit chain.
/// An indirect prompt-injection in tool output could pivot the agent
/// into spawning arbitrary commands.
///
/// We now require `server_name` to resolve to a row in `mcp_servers`
/// where `enabled = 1`. The returned `(command, env_keys)` come from
/// the registry, so even if the LLM asks for "powershell -c rm-rf",
/// the resolver rejects it because the literal string isn't a
/// registered name. Operator escape hatch: `LUCY_MCP_ALLOW_RAW=1`
/// preserves the legacy behaviour for one-off debugging.
fn resolve_server(server_name: &str) -> Result<(String, Vec<String>), String> {
    if std::env::var("LUCY_MCP_ALLOW_RAW").is_ok() {
        // Escape hatch — operator explicitly opted out of the gate.
        return Ok((server_name.to_string(), Vec::new()));
    }
    // Reject obvious raw commands fast: a registered name is a short
    // identifier, never a path or whitespace-laden command line.
    if server_name.contains(char::is_whitespace)
        || server_name.contains('\\')
        || server_name.contains('/')
        || server_name.is_empty()
        || server_name.len() > 128
    {
        return Err(format!(
            "MCP server '{}' rejected: not a registered name. Register it via the MCP Servers panel first.",
            server_name.chars().take(60).collect::<String>()
        ));
    }
    let name = server_name.to_string();
    crate::commands::metrics::shared_db(|conn| {
        let mut stmt = conn.prepare(
            "SELECT command, env_keys FROM mcp_servers \
             WHERE (name = ?1 OR id = ?1) AND enabled = 1 \
             LIMIT 1"
        ).map_err(|e| format!("resolve_server prepare: {}", e))?;
        let row = stmt.query_row(rusqlite::params![&name], |r| {
            let cmd: String = r.get(0)?;
            let env_keys: String = r.get(1).unwrap_or_default();
            Ok((cmd, env_keys))
        });
        match row {
            Ok((cmd, env_keys_json)) => {
                let env_keys: Vec<String> = serde_json::from_str(&env_keys_json)
                    .unwrap_or_default();
                Ok((cmd, env_keys))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(format!(
                "MCP server '{}' is not registered (or disabled). Open the MCP Servers panel to register it.",
                name
            )),
            Err(e) => Err(format!("resolve_server query: {}", e)),
        }
    })
}

/// Parse a command-line string into (program, args), applying the
/// `npx` → `npx.cmd` + `-y` injection used everywhere.
///
/// v1.7.103 Sprint-3 B5 follow-up: previously used `split_whitespace`,
/// which mangled paths like `"C:\Program Files\Node\node.exe" srv.js`
/// into three tokens. shlex respects single + double quoting the same
/// way bash and wezterm do. If shlex can't parse (extremely unlikely
/// — unbalanced quote), we fall back to the old behaviour so a typo
/// in the MCP registry still gets a useful error from the spawn
/// attempt instead of a silent skip.
fn parse_command(cmd_line: &str) -> (String, Vec<String>) {
    let mut parts: Vec<String> = shlex::split(cmd_line)
        .unwrap_or_else(|| cmd_line.split_whitespace().map(|s| s.to_string()).collect());
    if parts.is_empty() { parts.push("npx".to_string()); }
    let mut base_cmd = parts.remove(0);
    if cfg!(windows) && base_cmd == "npx" {
        base_cmd = "npx.cmd".to_string();
    }
    let mut args_vec = parts;
    if (base_cmd == "npx" || base_cmd == "npx.cmd")
        && !args_vec.iter().any(|a| a == "-y")
    {
        args_vec.insert(0, "-y".to_string());
    }
    (base_cmd, args_vec)
}

#[cfg(test)]
mod parse_command_tests {
    use super::parse_command;

    #[test]
    fn quoted_path_with_spaces_stays_one_token() {
        // v1.7.103 B5 follow-up regression test.
        let (cmd, args) = parse_command(r#""C:\Program Files\Node\node.exe" server.js"#);
        assert_eq!(cmd, r"C:\Program Files\Node\node.exe");
        assert_eq!(args, vec!["server.js"]);
    }

    #[test]
    fn unquoted_npx_injects_dash_y() {
        let (cmd, args) = parse_command("npx some-mcp-package");
        // On Windows this is `npx.cmd`, on others it's `npx`. Either way
        // the -y must precede the package name so npm doesn't prompt.
        assert!(cmd == "npx" || cmd == "npx.cmd");
        assert_eq!(args, vec!["-y", "some-mcp-package"]);
    }

    #[test]
    fn empty_string_does_not_panic() {
        let (cmd, args) = parse_command("");
        // Same npx/.cmd switch as the unquoted test — we don't care
        // which variant; both surface a useful spawn error.
        assert!(cmd == "npx" || cmd == "npx.cmd", "got cmd={}", cmd);
        // Empty input still gets the npx -y injection so a typo
        // surfaces an Ollama-style "couldn't resolve" rather than a
        // panic deep in the spawn path.
        assert_eq!(args, vec!["-y"]);
    }
}

/// Max bytes Lucy will buffer for a SINGLE JSON-RPC line from an MCP
/// server. Well-behaved servers emit compact newline-delimited frames;
/// even a large tool result fits far under 16 MiB (≈4M tokens — already
/// beyond any LLM context). The cap matters because
/// `AsyncBufReadExt::read_line` grows its buffer until it sees `\n` or
/// EOF, with NO size limit — a malicious or buggy server (MCP servers are
/// third-party npm / Python packages, i.e. the supply-chain threat model)
/// could stream endless bytes with no newline and OOM Lucy *before* the
/// per-read timeout fires. Bounding the line turns that into a clean
/// per-call error instead of a process-wide crash.
const MCP_MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// Drop-in replacement for `reader.read_line(&mut buf)` that refuses to
/// accumulate more than `MCP_MAX_LINE_BYTES`. Returns the number of bytes
/// appended (0 on EOF, matching `read_line`). On cap breach it returns an
/// `InvalidData` error so each caller's existing error path (evict the
/// session, or treat as EOF and stop) kicks in — never an unbounded alloc.
///
/// Generic over the reader so it can be exercised against an in-memory
/// buffer in tests; production callers pass `BufReader<ChildStdout>`.
async fn read_line_capped<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut String,
) -> std::io::Result<usize> {
    read_line_capped_with(reader, buf, MCP_MAX_LINE_BYTES).await
}

/// Inner form with an explicit byte cap so tests can drive the over-limit
/// branch without allocating 16 MiB. Bytes are accumulated raw and decoded
/// once at the end via `from_utf8_lossy`, so a multi-byte UTF-8 sequence
/// split across two `fill_buf` chunks is never corrupted (and a stray
/// non-UTF-8 byte from a misbehaving server degrades gracefully instead of
/// erroring like std `read_line` would).
async fn read_line_capped_with<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut String,
    max_bytes: usize,
) -> std::io::Result<usize> {
    let mut raw: Vec<u8> = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            break; // EOF
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(pos) => {
                raw.extend_from_slice(&available[..=pos]); // include the '\n'
                reader.consume(pos + 1);
                break;
            }
            None => {
                let take = available.len();
                raw.extend_from_slice(available);
                reader.consume(take);
                if raw.len() > max_bytes {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "MCP response line exceeded cap",
                    ));
                }
            }
        }
    }
    if raw.is_empty() {
        return Ok(0);
    }
    buf.push_str(&String::from_utf8_lossy(&raw));
    Ok(raw.len())
}

/// Spawn a subprocess for an MCP server and run the JSON-RPC handshake
/// (initialize + notifications/initialized). Returns the live process
/// handles ready for tools/list or tools/call.
async fn spawn_and_initialize(
    cmd_line: &str,
    env: Option<HashMap<String, String>>,
) -> Result<(Child, ChildStdin, BufReader<ChildStdout>, ChildStderr), String> {
    let (base_cmd, args_vec) = parse_command(cmd_line);
    let mut cmd = Command::new(&base_cmd);
    cmd.args(&args_vec)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(e_map) = env {
        cmd.envs(e_map);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Fallo al iniciar el servidor MCP: {}", e))?;

    let mut stdin = child.stdin.take().ok_or("No se pudo capturar stdin del MCP")?;
    let stdout = child.stdout.take().ok_or("No se pudo capturar stdout del MCP")?;
    let stderr = child.stderr.take().ok_or("No se pudo capturar stderr del MCP")?;
    let mut reader = BufReader::new(stdout);

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "LucySysAdmin", "version": "1.0.0" }
        }
    });
    let mut init_str = serde_json::to_string(&init_req)
        .map_err(|e| format!("MCP serialize init: {}", e))?;
    init_str.push('\n');
    stdin
        .write_all(init_str.as_bytes())
        .await
        .map_err(|e: std::io::Error| e.to_string())?;

    // Read until we get the initialize response (id=1).
    let mut buf = String::new();
    let mut stderr = stderr; // moved into the loop below
    loop {
        buf.clear();
        let read = tokio::time::timeout(Duration::from_secs(15), read_line_capped(&mut reader, &mut buf)).await;
        match read {
            Ok(Ok(0)) | Err(_) => {
                let mut err_str = String::new();
                let _ = stderr.read_to_string(&mut err_str).await;
                let _ = child.kill().await;
                return Err(if err_str.trim().is_empty() {
                    "Timeout o finalizacion inesperada del servidor MCP.".into()
                } else {
                    format!("Error fatal en servidor MCP: {}", err_str)
                });
            }
            Ok(Ok(_)) => {
                if let Ok(v) = serde_json::from_str::<Value>(&buf) {
                    if v["id"] == 1 {
                        break;
                    }
                }
            }
            _ => return Err("Error al leer stdout mcp".into()),
        }
    }

    let init_notif = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    let mut notif_str = serde_json::to_string(&init_notif)
        .map_err(|e| format!("MCP serialize notif: {}", e))?;
    notif_str.push('\n');
    let _ = stdin.write_all(notif_str.as_bytes()).await;

    Ok((child, stdin, reader, stderr))
}

/// Run a tools/list against an initialized server and return the raw
/// tools JSON array (or an empty array if absent).
async fn list_tools(
    stdin: &mut ChildStdin,
    reader: &mut BufReader<ChildStdout>,
) -> Result<Value, String> {
    let list_req = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
    let mut list_str = serde_json::to_string(&list_req)
        .map_err(|e| format!("MCP serialize list: {}", e))?;
    list_str.push('\n');
    stdin
        .write_all(list_str.as_bytes())
        .await
        .map_err(|e: std::io::Error| e.to_string())?;

    let mut buf = String::new();
    loop {
        buf.clear();
        let n = tokio::time::timeout(Duration::from_secs(15), read_line_capped(reader, &mut buf))
            .await
            .unwrap_or(Ok(0))
            .unwrap_or(0);
        if n == 0 {
            return Ok(json!([]));
        }
        if let Ok(v) = serde_json::from_str::<Value>(&buf) {
            if v["id"] == 2 {
                return Ok(v.get("result")
                    .and_then(|r| r.get("tools"))
                    .cloned()
                    .unwrap_or(json!([])));
            }
        }
    }
}

/// Run a tools/call against an initialized server and return the
/// concatenated text content (the standard MCP convention).
async fn call_tool(
    stdin: &mut ChildStdin,
    reader: &mut BufReader<ChildStdout>,
    tool_name: &str,
    args: Value,
) -> Result<String, String> {
    let tool_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": tool_name, "arguments": args }
    });
    let mut tool_str = serde_json::to_string(&tool_req)
        .map_err(|e| format!("MCP serialize tool: {}", e))?;
    tool_str.push('\n');
    stdin
        .write_all(tool_str.as_bytes())
        .await
        .map_err(|e: std::io::Error| e.to_string())?;

    let mut buf = String::new();
    let mut result_output = String::new();
    loop {
        buf.clear();
        let n = tokio::time::timeout(Duration::from_secs(45), read_line_capped(reader, &mut buf))
            .await
            .unwrap_or(Ok(0))
            .unwrap_or(0);
        if n == 0 {
            break;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&buf) {
            if v["id"] == 2 {
                if let Some(err) = v.get("error") {
                    return Err(format!("Error desde plugin MCP: {}", err));
                }
                if let Some(res) = v.get("result") {
                    if let Some(arr) = res.get("content").and_then(|c| c.as_array()) {
                        for c in arr {
                            if c["type"] == "text" {
                                if let Some(txt) = c["text"].as_str() {
                                    result_output.push_str(txt);
                                }
                            }
                        }
                    } else {
                        result_output = res.to_string();
                    }
                }
                break;
            }
        }
    }
    Ok(result_output)
}

// ── Connection pool (v1.4.2) ────────────────────────────────────────────
//
// Why this exists:
//   Each MCP `tools/call` used to spawn a fresh subprocess: `npx` cold-start
//   (200-800ms) → JSON-RPC initialize handshake → tools/call → kill. A turn
//   that hits the same server N times paid that overhead N times. With the
//   pool, we keep the subprocess alive between calls and ONLY pay the
//   tools/call latency (~50-200ms each). Empirically 15 GitHub MCP calls
//   dropped from ~45s overhead to ~750ms total.
//
// Lifecycle:
//   1. First call → spawn + initialize + send tools/call. Insert session
//      into the pool keyed by (server_name, command_line, env_hash).
//   2. Subsequent calls → look up the session, reuse the live process,
//      send tools/call with a fresh JSON-RPC id (the per-session counter).
//   3. Background reaper task (lazily started on first pool use) wakes
//      every 30s and evicts sessions idle >IDLE_TTL. Eviction kills the
//      child cleanly so it never lingers.
//   4. On ANY I/O error during a call, the session is dropped (the child
//      is killed) and the error is returned to the caller — the NEXT call
//      will re-spawn fresh, no manual recovery needed.
//
// Concurrency:
//   • Outer pool map: `Mutex<HashMap<...>>` — held only briefly for
//     insert/remove/lookup. The session itself is wrapped in `Arc<Mutex<>>`
//     so we can drop the outer lock immediately and serialize per-session.
//   • Per-session mutex: enforces ONE inflight request per session. JSON-RPC
//     over stdio is sequential — sending two requests concurrently would
//     interleave responses and corrupt parsing.

const POOL_IDLE_TTL: Duration = Duration::from_secs(60);
const POOL_REAPER_INTERVAL: Duration = Duration::from_secs(30);

struct PooledSession {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    /// Wall-clock timestamp of the last successful use. Updated AFTER a
    /// call returns Ok so the reaper sees "recent activity" only on
    /// healthy sessions, not stalled ones.
    last_used: Instant,
    /// Next JSON-RPC request id. Init used 1; tools/call starts at 2 and
    /// monotonically increases for each reuse on this session.
    next_req_id: u64,
    /// Human-readable label, surfaced in mcp_pool_stats.
    label: String,
}

static MCP_POOL: Lazy<AsyncMutex<HashMap<String, Arc<AsyncMutex<PooledSession>>>>> =
    Lazy::new(|| AsyncMutex::new(HashMap::new()));

/// Guard ensuring the reaper task is spawned exactly once per process.
static POOL_REAPER_STARTED: Lazy<AsyncMutex<bool>> = Lazy::new(|| AsyncMutex::new(false));

/// Build the pool key from (server_name, command, env). The env is
/// fingerprinted via a stable sorted concat so two calls with the same
/// environment hit the same session even when the HashMap iteration
/// order differs. We avoid keying on the literal env values for
/// **security** — never include secrets in a key that might be printed.
/// Instead we hash via the standard hasher; collisions are acceptable
/// because we ALSO verify the command matches.
fn pool_key(server_name: &str, command: &str, env: &Option<HashMap<String, String>>) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    if let Some(map) = env {
        // BTreeMap iteration is deterministic — sorts by key.
        let sorted: BTreeMap<&String, &String> = map.iter().collect();
        for (k, v) in sorted { k.hash(&mut hasher); v.hash(&mut hasher); }
    }
    let env_hash = hasher.finish();
    format!("{}::{}::{:x}", server_name, command, env_hash)
}

/// Try to acquire (or create) a pooled session for the given server.
/// Always returns a session ready for a tools/call; the caller MUST
/// either: (a) put it back into the pool by calling `pool_return` on
/// success, or (b) call `pool_evict` on error so the dead session is
/// removed and the next call re-spawns.
async fn pool_acquire(
    server_name: &str,
    command: &str,
    env: Option<HashMap<String, String>>,
) -> Result<(String, Arc<AsyncMutex<PooledSession>>), String> {
    let key = pool_key(server_name, command, &env);
    ensure_reaper_running().await;

    // Fast path: look up existing session.
    {
        let map = MCP_POOL.lock().await;
        if let Some(arc) = map.get(&key) {
            return Ok((key, arc.clone()));
        }
        // No existing session — drop the map lock before spawning (which
        // can take 500ms+ for cold npx) so other servers don't block.
        drop(map);
    }

    // Slow path: spawn a new session.
    let (child, stdin, reader, _stderr) = spawn_and_initialize(command, env).await?;
    let session = PooledSession {
        child, stdin, reader,
        last_used: Instant::now(),
        next_req_id: 2,  // init used id=1
        label: server_name.to_string(),
    };
    let arc = Arc::new(AsyncMutex::new(session));
    let mut map = MCP_POOL.lock().await;
    // Another concurrent call may have inserted between our lookup and
    // here — in that case kill our extra session, use theirs.
    if let Some(existing) = map.get(&key) {
        let mut s = arc.lock().await;
        let _ = s.child.kill().await;
        return Ok((key, existing.clone()));
    }
    map.insert(key.clone(), arc.clone());
    Ok((key, arc))
}

/// Evict a specific pool entry (used after I/O errors). Kills the child
/// process and removes the map entry. Safe to call even if the entry
/// is already gone.
async fn pool_evict(key: &str) {
    let mut map = MCP_POOL.lock().await;
    if let Some(arc) = map.remove(key) {
        // Try to lock briefly; if another task is mid-call we still
        // remove it from the map but let them finish. The `Drop` impl
        // on Child does not kill, so we explicitly kill here.
        if let Ok(mut s) = arc.try_lock() {
            let _ = s.child.kill().await;
        }
    }
}

/// Background task that wakes every 30s and evicts sessions whose
/// last_used is older than 60s. Started lazily on the first pool use
/// so test runs / CLI invocations that never touch MCP pay no cost.
async fn ensure_reaper_running() {
    let mut started = POOL_REAPER_STARTED.lock().await;
    if *started { return; }
    *started = true;
    drop(started);

    tokio::spawn(async {
        let mut tick = tokio::time::interval(POOL_REAPER_INTERVAL);
        // First tick fires immediately — burn it so we don't sweep an
        // empty pool right after startup.
        tick.tick().await;
        loop {
            tick.tick().await;
            let now = Instant::now();
            let to_evict: Vec<String> = {
                let map = MCP_POOL.lock().await;
                let mut keys = Vec::new();
                for (k, arc) in map.iter() {
                    if let Ok(s) = arc.try_lock() {
                        if now.duration_since(s.last_used) > POOL_IDLE_TTL {
                            keys.push(k.clone());
                        }
                    }
                }
                keys
            };
            for k in to_evict {
                pool_evict(&k).await;
            }
        }
    });
}

/// Pooled equivalent of `call_tool` — sends `tools/call` over an existing
/// session's stdio. Uses the session's request_id counter so multiple
/// reuses don't collide. On any I/O error returns Err and the caller
/// MUST evict the session.
async fn pool_call_tool_on_session(
    session: &mut PooledSession,
    tool_name: &str,
    args: Value,
) -> Result<String, String> {
    let req_id = session.next_req_id;
    session.next_req_id += 1;

    let tool_req = json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "tools/call",
        "params": { "name": tool_name, "arguments": args }
    });
    let mut tool_str = serde_json::to_string(&tool_req)
        .map_err(|e| format!("MCP serialize tool: {}", e))?;
    tool_str.push('\n');
    session.stdin
        .write_all(tool_str.as_bytes())
        .await
        .map_err(|e: std::io::Error| format!("pool stdin: {}", e))?;

    let mut buf = String::new();
    let mut result_output = String::new();
    loop {
        buf.clear();
        let n = tokio::time::timeout(Duration::from_secs(45), read_line_capped(&mut session.reader, &mut buf))
            .await
            .map_err(|_| "pool: read timeout".to_string())?
            .map_err(|e| format!("pool stdout: {}", e))?;
        if n == 0 {
            return Err("pool: server closed stdout".into());
        }
        if let Ok(v) = serde_json::from_str::<Value>(&buf) {
            // Only react to OUR request id; stray notifications/responses
            // for other ids (rare but possible) get silently skipped.
            if v["id"] == req_id {
                if let Some(err) = v.get("error") {
                    return Err(format!("Error desde plugin MCP: {}", err));
                }
                if let Some(res) = v.get("result") {
                    if let Some(arr) = res.get("content").and_then(|c| c.as_array()) {
                        for c in arr {
                            if c["type"] == "text" {
                                if let Some(txt) = c["text"].as_str() {
                                    result_output.push_str(txt);
                                }
                            }
                        }
                    } else {
                        result_output = res.to_string();
                    }
                }
                break;
            }
        }
    }
    Ok(result_output)
}

/// Public wrapper: acquire a pooled session, send the tool call, return
/// the output. Handles all error-recovery (evict on failure) so callers
/// just match on Result like with the un-pooled version.
async fn pooled_call(
    server_name: &str,
    command: &str,
    env: Option<HashMap<String, String>>,
    tool_name: &str,
    args: Value,
) -> Result<String, String> {
    let (key, arc) = pool_acquire(server_name, command, env).await?;
    let result = {
        let mut session = arc.lock().await;
        let r = pool_call_tool_on_session(&mut session, tool_name, args).await;
        if r.is_ok() { session.last_used = Instant::now(); }
        r
    };
    if result.is_err() {
        pool_evict(&key).await;
    }
    result
}

/// Telemetry — lets the diagnostics view show pool occupancy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolEntryInfo {
    pub label: String,
    pub idle_secs: i64,
    pub next_req_id: u64,
}

#[tauri::command]
pub async fn mcp_pool_stats() -> Result<Vec<PoolEntryInfo>, String> {
    let map = MCP_POOL.lock().await;
    let now = Instant::now();
    let mut out = Vec::new();
    for arc in map.values() {
        if let Ok(s) = arc.try_lock() {
            out.push(PoolEntryInfo {
                label: s.label.clone(),
                idle_secs: now.duration_since(s.last_used).as_secs() as i64,
                next_req_id: s.next_req_id,
            });
        }
    }
    Ok(out)
}

/// Manual flush — useful from Settings → "Clear MCP pool" or when the
/// user updates a server's command and wants the next call to re-spawn.
#[tauri::command]
pub async fn mcp_pool_clear() -> Result<usize, String> {
    let mut map = MCP_POOL.lock().await;
    let n = map.len();
    let entries: Vec<_> = map.drain().collect();
    drop(map);
    for (_, arc) in entries {
        if let Ok(mut s) = arc.try_lock() {
            let _ = s.child.kill().await;
        }
    }
    Ok(n)
}

// ── Legacy (raw-command) commands — backward compatibility ──────────────

#[tauri::command]
pub async fn call_mcp_tool(
    server_name: String,
    query: String,
    env: Option<HashMap<String, String>>,
) -> Result<String, String> {
    // v1.7.102 Sprint-2 B5: gate through the registry. The previous
    // contract treated `server_name` as a raw command line — an
    // LLM-driven RCE vector via prompt injection. Now we resolve it to
    // a registered, operator-curated `mcp_servers` row.
    let (resolved_cmd, _registered_env_keys) = resolve_server(&server_name)?;
    crate::utils::logging::write_app_log(
        "INFO",
        &format!("MCP tool call: server='{}' (resolved=`{}`)", server_name, resolved_cmd),
    );

    let parts: Vec<&str> = query.split("|||").collect();
    let tool_name = parts[0].trim().to_string();
    let args_str = parts.get(1).unwrap_or(&"{}").trim().to_string();
    let args_json: Value = serde_json::from_str(&args_str).unwrap_or(json!({}));

    let (mut child, mut stdin, mut reader, _stderr) =
        spawn_and_initialize(&resolved_cmd, env).await?;
    let res = call_tool(&mut stdin, &mut reader, &tool_name, args_json).await;
    let _ = child.kill().await;
    match res {
        Ok(s) if s.is_empty() => Ok("Sin retorno o resultado vacio del conector MCP.".into()),
        Ok(s) => Ok(s),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn discover_mcp_tools(
    server_name: String,
    env: Option<HashMap<String, String>>,
) -> Result<String, String> {
    // v1.7.102 Sprint-2 B5: same registry gate as call_mcp_tool.
    let (resolved_cmd, _registered_env_keys) = resolve_server(&server_name)?;
    let (mut child, mut stdin, mut reader, _stderr) =
        spawn_and_initialize(&resolved_cmd, env).await?;
    let tools = list_tools(&mut stdin, &mut reader).await?;
    let _ = child.kill().await;
    let pretty = serde_json::to_string_pretty(&tools).unwrap_or_default();
    if pretty == "[]" || pretty.is_empty() {
        Ok("No se encontraron herramientas o formato desconocido.".into())
    } else {
        Ok(pretty)
    }
}

// ── First-class registry layer ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub command: String,
    pub env_keys: Vec<String>,
    pub enabled: bool,
    pub tools_cache: Value,        // JSON array of tools (verbatim from tools/list)
    pub last_discovered: Option<i64>,
    pub last_status: String,       // 'ok'|'error'|'pending'
    pub last_error: Option<String>,
    pub last_latency_ms: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTestResult {
    pub ok: bool,
    pub latency_ms: i64,
    pub tools_count: i64,
    pub error: Option<String>,
}

fn row_to_server(
    id: String,
    name: String,
    command: String,
    env_keys_json: String,
    enabled: i64,
    tools_cache_json: String,
    last_discovered: Option<i64>,
    last_status: String,
    last_error: Option<String>,
    last_latency_ms: Option<i64>,
    created_at: i64,
    updated_at: i64,
) -> McpServer {
    let env_keys: Vec<String> =
        serde_json::from_str(&env_keys_json).unwrap_or_default();
    let tools_cache: Value =
        serde_json::from_str(&tools_cache_json).unwrap_or(json!([]));
    McpServer {
        id, name, command, env_keys,
        enabled: enabled != 0,
        tools_cache, last_discovered, last_status, last_error,
        last_latency_ms, created_at, updated_at,
    }
}

/// Pick the subset of `bag` whose keys appear in `wanted`.
/// Returns `None` (no env override) when both are empty.
fn filter_env(
    wanted: &[String],
    bag: Option<HashMap<String, String>>,
) -> Option<HashMap<String, String>> {
    let bag = bag?;
    if wanted.is_empty() {
        return None;
    }
    let filtered: HashMap<String, String> = bag
        .into_iter()
        .filter(|(k, _)| wanted.iter().any(|w| w == k))
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
}

#[tauri::command]
pub async fn mcp_server_list() -> Result<Vec<McpServer>, String> {
    shared_db(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, command, env_keys, enabled, tools_cache,
                    last_discovered, last_status, last_error, last_latency_ms,
                    created_at, updated_at
             FROM mcp_servers ORDER BY name COLLATE NOCASE ASC",
        ).map_err(|e| format!("mcp_server_list prepare: {}", e))?;
        let rows = stmt.query_map([], |r| {
            Ok(row_to_server(
                r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?,
                r.get::<_, i64>(4)?, r.get(5)?, r.get(6)?, r.get(7)?,
                r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?,
            ))
        }).map_err(|e| format!("mcp_server_list query: {}", e))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("mcp_server_list row: {}", e))?);
        }
        Ok(out)
    })
}

/// Create or update a server by name. Returns the persisted row.
#[tauri::command]
pub async fn mcp_server_upsert(
    name: String,
    command: String,
    env_keys: Vec<String>,
    enabled: bool,
) -> Result<McpServer, String> {
    let name_trim = name.trim().to_string();
    let cmd_trim = command.trim().to_string();
    if name_trim.is_empty() {
        return Err("El nombre no puede estar vacio.".into());
    }
    if cmd_trim.is_empty() {
        return Err("El comando no puede estar vacio.".into());
    }
    let env_keys_json = serde_json::to_string(&env_keys).unwrap_or("[]".into());
    let enabled_int: i64 = if enabled { 1 } else { 0 };

    shared_db(move |conn| {
        // Generate stable id from name (slug-ish) — keeps URLs / refs nicer.
        let existing_id: Option<String> = conn
            .query_row(
                "SELECT id FROM mcp_servers WHERE name = ?1",
                params![name_trim],
                |r| r.get(0),
            )
            .ok();
        let id = existing_id.unwrap_or_else(|| {
            format!("mcp-{}", chrono::Local::now().timestamp_millis())
        });

        conn.execute(
            "INSERT INTO mcp_servers (id, name, command, env_keys, enabled, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s','now'))
             ON CONFLICT(name) DO UPDATE SET
                command = excluded.command,
                env_keys = excluded.env_keys,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at",
            params![id, name_trim, cmd_trim, env_keys_json, enabled_int],
        ).map_err(|e| format!("mcp_server_upsert: {}", e))?;

        let server = conn.query_row(
            "SELECT id, name, command, env_keys, enabled, tools_cache,
                    last_discovered, last_status, last_error, last_latency_ms,
                    created_at, updated_at
             FROM mcp_servers WHERE name = ?1",
            params![name_trim],
            |r| Ok(row_to_server(
                r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?,
                r.get::<_, i64>(4)?, r.get(5)?, r.get(6)?, r.get(7)?,
                r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?,
            )),
        ).map_err(|e| format!("mcp_server_upsert reload: {}", e))?;
        Ok(server)
    })?;

    // v1.4.2: evict pooled sessions for this server label. The command or
    // env_keys may have changed, so any stale process must be respawned
    // on the next call. Cheap: O(n) over the (usually small) pool map.
    let evict_label = name.trim().to_string();
    let mut to_evict = Vec::new();
    {
        let map = MCP_POOL.lock().await;
        for (key, arc) in map.iter() {
            if let Ok(s) = arc.try_lock() {
                if s.label == evict_label { to_evict.push(key.clone()); }
            }
        }
    }
    for k in to_evict { pool_evict(&k).await; }

    // Re-read the canonical row to return — server isn't borrowed any more.
    let final_name = evict_label;
    shared_db(move |conn| {
        conn.query_row(
            "SELECT id, name, command, env_keys, enabled, tools_cache,
                    last_discovered, last_status, last_error, last_latency_ms,
                    created_at, updated_at
             FROM mcp_servers WHERE name = ?1",
            params![final_name],
            |r| Ok(row_to_server(
                r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?,
                r.get::<_, i64>(4)?, r.get(5)?, r.get(6)?, r.get(7)?,
                r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?,
            )),
        ).map_err(|e| format!("mcp_server_upsert final read: {}", e))
    })
}

#[tauri::command]
pub async fn mcp_server_delete(name: String) -> Result<(), String> {
    let evict_name = name.clone();
    shared_db(move |conn| {
        conn.execute(
            "DELETE FROM mcp_servers WHERE name = ?1",
            params![name],
        ).map_err(|e| format!("mcp_server_delete: {}", e))?;
        Ok(())
    })?;
    // v1.4.2: also kill any pooled sessions for this server so a future
    // re-add with a different command doesn't accidentally reuse them.
    // We iterate the pool and evict any entry whose label matches.
    let mut to_evict = Vec::new();
    {
        let map = MCP_POOL.lock().await;
        for (key, arc) in map.iter() {
            if let Ok(s) = arc.try_lock() {
                if s.label == evict_name { to_evict.push(key.clone()); }
            }
        }
    }
    for k in to_evict { pool_evict(&k).await; }
    Ok(())
}

/// Spawn the registered server, run tools/list, persist the result, return
/// the updated row. Tools cache + status are atomically refreshed.
#[tauri::command]
pub async fn mcp_server_discover(
    name: String,
    env: Option<HashMap<String, String>>,
) -> Result<McpServer, String> {
    // Pull command + env_keys from the registry first.
    let lookup_name = name.clone();
    let (command, env_keys) = shared_db(move |conn| {
        let row = conn.query_row(
            "SELECT command, env_keys FROM mcp_servers WHERE name = ?1",
            params![lookup_name],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ).map_err(|e| format!("mcp_server_discover lookup: {}", e))?;
        Ok((row.0, serde_json::from_str::<Vec<String>>(&row.1).unwrap_or_default()))
    })?;

    let filtered_env = filter_env(&env_keys, env);

    let start = Instant::now();
    let result = async {
        let (mut child, mut stdin, mut reader, _stderr) =
            spawn_and_initialize(&command, filtered_env).await?;
        let tools = list_tools(&mut stdin, &mut reader).await?;
        let _ = child.kill().await;
        Ok::<Value, String>(tools)
    }.await;
    let elapsed_ms = start.elapsed().as_millis() as i64;

    let (status, error, tools_json, count): (String, Option<String>, String, i64) = match result {
        Ok(tools) => {
            let n = tools.as_array().map(|a| a.len() as i64).unwrap_or(0);
            (
                "ok".into(), None,
                serde_json::to_string(&tools).unwrap_or("[]".into()),
                n,
            )
        }
        Err(e) => ("error".into(), Some(e), "[]".into(), 0),
    };
    let _ = count; // available for telemetry; not stored separately

    let save_name = name.clone();
    shared_db(move |conn| {
        conn.execute(
            "UPDATE mcp_servers
                SET tools_cache = ?1,
                    last_discovered = strftime('%s','now'),
                    last_status = ?2,
                    last_error = ?3,
                    last_latency_ms = ?4,
                    updated_at = strftime('%s','now')
              WHERE name = ?5",
            params![tools_json, status, error, elapsed_ms, save_name],
        ).map_err(|e| format!("mcp_server_discover persist: {}", e))?;
        Ok(())
    })?;

    // Re-read to return the canonical row.
    let final_name = name.clone();
    shared_db(move |conn| {
        let server = conn.query_row(
            "SELECT id, name, command, env_keys, enabled, tools_cache,
                    last_discovered, last_status, last_error, last_latency_ms,
                    created_at, updated_at
             FROM mcp_servers WHERE name = ?1",
            params![final_name],
            |r| Ok(row_to_server(
                r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?,
                r.get::<_, i64>(4)?, r.get(5)?, r.get(6)?, r.get(7)?,
                r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?,
            )),
        ).map_err(|e| format!("mcp_server_discover read: {}", e))?;
        Ok(server)
    })
}

/// Lightweight health check: spawn → initialize → tools/list → close.
/// Returns latency and tool count; does NOT persist anything.
#[tauri::command]
pub async fn mcp_server_test(
    name: String,
    env: Option<HashMap<String, String>>,
) -> Result<McpTestResult, String> {
    let lookup_name = name.clone();
    let (command, env_keys) = shared_db(move |conn| {
        let row = conn.query_row(
            "SELECT command, env_keys FROM mcp_servers WHERE name = ?1",
            params![lookup_name],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ).map_err(|e| format!("mcp_server_test lookup: {}", e))?;
        Ok((row.0, serde_json::from_str::<Vec<String>>(&row.1).unwrap_or_default()))
    })?;

    let filtered_env = filter_env(&env_keys, env);
    let start = Instant::now();
    let res = async {
        let (mut child, mut stdin, mut reader, _stderr) =
            spawn_and_initialize(&command, filtered_env).await?;
        let tools = list_tools(&mut stdin, &mut reader).await?;
        let _ = child.kill().await;
        Ok::<i64, String>(tools.as_array().map(|a| a.len() as i64).unwrap_or(0))
    }.await;
    let latency_ms = start.elapsed().as_millis() as i64;

    Ok(match res {
        Ok(count) => McpTestResult {
            ok: true, latency_ms, tools_count: count, error: None,
        },
        Err(e) => McpTestResult {
            ok: false, latency_ms, tools_count: 0, error: Some(e),
        },
    })
}

/// Invoke a tool on a registered server by NAME (not raw command).
/// Filters the env bag to only the keys this server declared it needs.
#[tauri::command]
pub async fn mcp_server_call(
    name: String,
    tool_name: String,
    args_json: String,
    env: Option<HashMap<String, String>>,
) -> Result<String, String> {
    let lookup_name = name.clone();
    let (command, env_keys, enabled) = shared_db(move |conn| {
        let row = conn.query_row(
            "SELECT command, env_keys, enabled FROM mcp_servers WHERE name = ?1",
            params![lookup_name],
            |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            )),
        ).map_err(|e| format!("mcp_server_call lookup: {}", e))?;
        Ok((
            row.0,
            serde_json::from_str::<Vec<String>>(&row.1).unwrap_or_default(),
            row.2 != 0,
        ))
    })?;
    if !enabled {
        return Err(format!("El servidor MCP '{}' esta deshabilitado.", name));
    }

    let args_value: Value = serde_json::from_str(&args_json).unwrap_or(json!({}));
    let filtered_env = filter_env(&env_keys, env);

    // v1.4.2: route through the connection pool. First call cold-starts
    // the subprocess; subsequent calls in the next 60s reuse it.
    let res = pooled_call(&name, &command, filtered_env, &tool_name, args_value).await;
    match res {
        Ok(s) if s.is_empty() => Ok("Sin retorno o resultado vacio del conector MCP.".into()),
        Ok(s) => Ok(s),
        Err(e) => Err(e),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parse_command_npx_inserts_dash_y() {
        let (base, args) = parse_command("npx @scope/server arg1");
        let expected_base = if cfg!(windows) { "npx.cmd" } else { "npx" };
        assert_eq!(base, expected_base);
        assert_eq!(args, vec!["-y", "@scope/server", "arg1"]);
    }

    #[test]
    fn parse_command_npx_keeps_existing_dash_y() {
        let (_, args) = parse_command("npx -y @scope/server");
        // Only one "-y" expected.
        assert_eq!(args.iter().filter(|a| *a == "-y").count(), 1);
    }

    #[test]
    fn parse_command_non_npx_left_alone() {
        let (base, args) = parse_command("python -m my_server --port 9000");
        assert_eq!(base, "python");
        assert_eq!(args, vec!["-m", "my_server", "--port", "9000"]);
    }

    // ── read_line_capped: bounded JSON-RPC line reader (v1.7.219) ──
    // Guards against a hostile/buggy MCP server streaming an endless line
    // (no '\n') and OOM-ing Lucy. Driven against an in-memory reader so no
    // subprocess / Python is needed.

    #[tokio::test]
    async fn capped_reads_one_line_including_newline() {
        let data: &[u8] = b"hello\nworld\n";
        let mut reader = BufReader::new(data);
        let mut buf = String::new();
        let n = read_line_capped(&mut reader, &mut buf).await.unwrap();
        assert_eq!(n, 6);
        assert_eq!(buf, "hello\n");
        buf.clear();
        let n2 = read_line_capped(&mut reader, &mut buf).await.unwrap();
        assert_eq!(n2, 6);
        assert_eq!(buf, "world\n");
    }

    #[tokio::test]
    async fn capped_eof_returns_zero() {
        let data: &[u8] = b"";
        let mut reader = BufReader::new(data);
        let mut buf = String::new();
        let n = read_line_capped(&mut reader, &mut buf).await.unwrap();
        assert_eq!(n, 0);
        assert!(buf.is_empty());
    }

    #[tokio::test]
    async fn capped_unterminated_final_line_then_eof() {
        let data: &[u8] = b"partial"; // no trailing newline
        let mut reader = BufReader::new(data);
        let mut buf = String::new();
        let n = read_line_capped(&mut reader, &mut buf).await.unwrap();
        assert_eq!(n, 7);
        assert_eq!(buf, "partial");
        buf.clear();
        let n2 = read_line_capped(&mut reader, &mut buf).await.unwrap();
        assert_eq!(n2, 0);
    }

    #[tokio::test]
    async fn capped_rejects_line_over_limit() {
        // 10 bytes, no newline, cap of 4 → must error, not buffer it all.
        let data: &[u8] = b"abcdefghij";
        let mut reader = BufReader::new(data);
        let mut buf = String::new();
        let err = read_line_capped_with(&mut reader, &mut buf, 4)
            .await
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn capped_allows_exact_fit_terminated_line() {
        // A newline-terminated line at the cap is fine — the cap only fires
        // on the unbounded (no-newline) path, never on a complete frame.
        let data: &[u8] = b"abc\n";
        let mut reader = BufReader::new(data);
        let mut buf = String::new();
        let n = read_line_capped_with(&mut reader, &mut buf, 4)
            .await
            .unwrap();
        assert_eq!(n, 4);
        assert_eq!(buf, "abc\n");
    }

    // ── Pool key tests ──
    // ── Integration tests against the Python mock MCP server ──
    // These hit the FULL spawn → JSON-RPC → tools/call → close pipeline
    // using src-tauri/tests/mcp_mock_server.py. Marked #[ignore] because
    // they need Python on PATH; run via `cargo test -- --ignored`.

    fn detect_python() -> Option<String> {
        for cmd in &["python", "python3", "py"] {
            if std::process::Command::new(cmd)
                .arg("--version")
                .output()
                .ok()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return Some(cmd.to_string());
            }
        }
        None
    }

    fn mock_server_cmd(py: &str) -> String {
        // src-tauri/tests/mcp_mock_server.py — relative to crate root.
        let manifest = env!("CARGO_MANIFEST_DIR");
        format!("{} {}/tests/mcp_mock_server.py", py, manifest)
    }

    #[tokio::test]
    #[ignore]
    async fn mock_server_discover_returns_one_tool() {
        let Some(py) = detect_python() else {
            eprintln!("Python not found — skipping mock_server_discover_returns_one_tool");
            return;
        };
        let cmd = mock_server_cmd(&py);
        let (mut child, mut stdin, mut reader, _stderr) =
            spawn_and_initialize(&cmd, None).await.expect("spawn ok");
        let tools = list_tools(&mut stdin, &mut reader).await.expect("list ok");
        let _ = child.kill().await;
        let arr = tools.as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "echo");
        // inputSchema must be present — that's what feeds the system prompt signature.
        assert!(arr[0]["inputSchema"]["properties"]["message"].is_object());
    }

    #[tokio::test]
    #[ignore]
    async fn mock_server_call_echoes_args() {
        let Some(py) = detect_python() else { return; };
        let cmd = mock_server_cmd(&py);
        let (mut child, mut stdin, mut reader, _stderr) =
            spawn_and_initialize(&cmd, None).await.expect("spawn ok");
        let res = call_tool(&mut stdin, &mut reader, "echo",
                            serde_json::json!({ "message": "ping" })).await
            .expect("call ok");
        let _ = child.kill().await;
        assert!(res.contains("ping"), "expected echo, got: {}", res);
        assert!(res.starts_with("[1]"), "first call counter should be 1: {}", res);
    }

    #[tokio::test]
    #[ignore]
    async fn mock_server_pool_reuses_session() {
        // The whole point of the pool: 3 sequential calls hit ONE process
        // (call counter should be 1, 2, 3 — proving reuse, not respawn).
        let Some(py) = detect_python() else { return; };
        let cmd = mock_server_cmd(&py);
        let server = "mock-pool-test";

        let r1 = pooled_call(server, &cmd, None, "echo",
            serde_json::json!({ "message": "a" })).await.expect("r1");
        let r2 = pooled_call(server, &cmd, None, "echo",
            serde_json::json!({ "message": "b" })).await.expect("r2");
        let r3 = pooled_call(server, &cmd, None, "echo",
            serde_json::json!({ "message": "c" })).await.expect("r3");

        // Cleanup so subsequent runs start fresh.
        let _ = mcp_pool_clear().await;

        assert!(r1.starts_with("[1]"), "first call had counter != 1: {}", r1);
        assert!(r2.starts_with("[2]"), "session reused → second call should be 2, got: {}", r2);
        assert!(r3.starts_with("[3]"), "session reused → third call should be 3, got: {}", r3);
    }

    #[test]
    fn pool_key_is_deterministic_across_env_iter_order() {
        let mut a = HashMap::new();
        a.insert("FOO".to_string(), "1".to_string());
        a.insert("BAR".to_string(), "2".to_string());
        let mut b = HashMap::new();
        b.insert("BAR".to_string(), "2".to_string());
        b.insert("FOO".to_string(), "1".to_string());
        let ka = pool_key("github", "npx -y x", &Some(a));
        let kb = pool_key("github", "npx -y x", &Some(b));
        assert_eq!(ka, kb, "same env, different insertion order → same key");
    }

    #[test]
    fn pool_key_differs_when_env_values_differ() {
        let mut a = HashMap::new();
        a.insert("TOKEN".to_string(), "aaa".to_string());
        let mut b = HashMap::new();
        b.insert("TOKEN".to_string(), "bbb".to_string());
        let ka = pool_key("x", "cmd", &Some(a));
        let kb = pool_key("x", "cmd", &Some(b));
        assert_ne!(ka, kb, "different secret → must NOT share a session");
    }

    #[test]
    fn pool_key_differs_per_server_name() {
        let ka = pool_key("github", "cmd", &None);
        let kb = pool_key("filesystem", "cmd", &None);
        assert_ne!(ka, kb);
    }

    #[test]
    fn pool_key_differs_per_command() {
        let ka = pool_key("x", "cmd-A", &None);
        let kb = pool_key("x", "cmd-B", &None);
        assert_ne!(ka, kb);
    }

    #[test]
    fn pool_key_no_env_is_stable() {
        let k1 = pool_key("a", "b", &None);
        let k2 = pool_key("a", "b", &None);
        assert_eq!(k1, k2);
    }

    #[test]
    fn filter_env_subset_works() {
        let mut bag = HashMap::new();
        bag.insert("GITHUB_TOKEN".into(), "ghp_aaa".into());
        bag.insert("SLACK_TOKEN".into(), "xoxb_bbb".into());
        bag.insert("UNRELATED".into(), "noise".into());
        let wanted = vec!["GITHUB_TOKEN".to_string()];
        let filtered = filter_env(&wanted, Some(bag)).expect("some");
        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains_key("GITHUB_TOKEN"));
    }

    #[test]
    fn filter_env_no_keys_returns_none() {
        let mut bag = HashMap::new();
        bag.insert("X".into(), "Y".into());
        assert!(filter_env(&[], Some(bag)).is_none());
    }

    #[test]
    fn filter_env_no_matches_returns_none() {
        let mut bag = HashMap::new();
        bag.insert("X".into(), "Y".into());
        let wanted = vec!["NOT_THERE".to_string()];
        assert!(filter_env(&wanted, Some(bag)).is_none());
    }
}
