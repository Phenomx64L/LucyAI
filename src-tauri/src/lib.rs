// ── Lucy — Tauri entry point ───────────────────────────────────────────────────

// v1.4.10 — mimalloc as the global allocator. 10-30% perf win on hot paths
// (JSON parse, SQLite reads, Markdown render) at zero behavioral risk:
// mimalloc is API-compatible with the system allocator. The static is
// referenced via #[global_allocator] so it's swapped at link time and
// every box / vec / string allocation routes through mimalloc for the
// entire process.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// ── v1.7.42 — GPU vendor hints for hybrid-graphics laptops ──────────────────
//
// On laptops with both an integrated GPU (Intel UHD / AMD Radeon Graphics)
// AND a discrete GPU (NVIDIA RTX / AMD Radeon RX), the OS decides which
// adapter to assign to each process. Without an explicit hint, Windows
// often routes Tauri/WebView2 apps to the iGPU to save battery — even
// when the laptop is plugged in. That causes visible UI lag and very
// high GPU% as the iGPU strains to composite Mica + backdrop-filter.
//
// These two exported symbols are *hints* read at startup by the vendor
// drivers:
//
//   • NvOptimusEnablement = 0x00000001
//       Tells NVIDIA Optimus to bind this process to the discrete GPU.
//       Documented at https://docs.nvidia.com/gameworks/content/technologies/desktop/optimus.htm
//
//   • AmdPowerXpressRequestHighPerformance = 1
//       Same for AMD PowerXpress / Enduro hybrid setups.
//
// IMPORTANT — these are 100% safe on machines WITHOUT a discrete GPU:
// the symbols are simply ignored if the vendor driver isn't installed,
// so single-GPU and pure-iGPU users see no change. They are also
// inert in debug builds because LTO is off and the linker may strip
// them; that's fine because dev builds use the dev profile anyway.
//
// `#[used]` prevents the linker from garbage-collecting the symbols
// despite them being unreferenced from Rust code (the drivers read
// them from the PE export table, not from any function call).
#[cfg(all(windows, not(debug_assertions)))]
#[used]
#[no_mangle]
pub static NvOptimusEnablement: u32 = 0x0000_0001;

#[cfg(all(windows, not(debug_assertions)))]
#[used]
#[no_mangle]
pub static AmdPowerXpressRequestHighPerformance: i32 = 1;

mod state;
mod utils;
mod commands;
mod guardrails;

use commands::{ai, compliance, config, hosts, inventory, indexer, incident, local, logs, metrics, providers, rdp_agent, local_screen, reflection, shell, system, ui, embeddings, memory, pdf, audit, capacity, diagnostics, notify, log_analysis, state_snapshot, process_lineage, self_healing, causal, threat_scan, object_bridge, runbook_gen, daily_patterns, sandbox_preview, knowledge_graph, incident_detective, frontier_telemetry, activity_feed, replay, shell_recording, cve_match, db_backup, support_bundle, inventory_drift, dashboard_integrations, hash_chain, pty, web_ingest};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ── v1.7.42 — WebView2 GPU acceleration hints ──────────────────────────
    // WebView2 (Chromium under the hood) reads this env var at process
    // launch to pass extra Chromium command-line flags. We use it to:
    //
    //   --enable-gpu-rasterization       Force GPU path for 2D content
    //                                    (text, rounded corners, shadows).
    //   --enable-zero-copy               Skip the GPU→CPU readback when
    //                                    uploading textures; significant
    //                                    win for backdrop-filter passes.
    //   --ignore-gpu-blocklist           Don't fall back to software for
    //                                    cards Chromium has historically
    //                                    flagged (mostly older Intel HD
    //                                    drivers from ~2017). On modern
    //                                    hardware this just removes a
    //                                    needless software-render fallback.
    //
    // SAFETY ON OLD HARDWARE: if any of these flags fail to take effect
    // (driver too old, GPU truly unsupported), Chromium's renderer
    // automatically falls back to software compositing — Lucy still
    // renders correctly, just without the GPU acceleration.
    //
    // We only set the var when it isn't already defined so power users
    // can override from the shell for debugging.
    #[cfg(windows)]
    {
        // ── v1.7.208 — RENDERING CONTINUITY (fixes "info disappears until I
        // move the mouse") ────────────────────────────────────────────────
        // The flag set lives at module scope (WEBVIEW2_BROWSER_ARGS) so the
        // test below can hold it against tauri.conf.json.
        // WebView2/Chromium pauses or throttles compositing + timers when it
        // believes the window is occluded or backgrounded. The symptom: the
        // DOM updates (streaming tokens land) but the screen is NOT repainted
        // until an input event (mouse move) forces a composite — so the user
        // sees a frozen bubble that "jumps" up to date on hover. The
        // streaming render's own setInterval fallback ALSO gets throttled to
        // ~1/min under background-timer-throttling, so it can't save us.
        //
        //   --disable-features=CalculateNativeWinOcclusion
        //       Stop Chromium from marking the window "occluded" and halting
        //       paints. This is the single most important flag for the bug.
        //   --disable-background-timer-throttling
        //       Keep setInterval/rAF fallbacks ticking at full rate even when
        //       the window loses focus — the streaming drain depends on it.
        //   --disable-backgrounding-occluded-windows
        //   --disable-renderer-backgrounding
        //       Don't deprioritize / freeze the renderer process when the
        //       window is behind another or unfocused.
        // ── v1.8 — ONE source for these flags ────────────────────────────
        //
        // This block and `tauri.conf.json`'s `additionalBrowserArgs` both set
        // WebView2 browser arguments, and they DID NOT AGREE: the config
        // disables six features, this set disabled one — but added three GPU
        // flags the config never had. Only one of the two reaches WebView2
        // (`AdditionalBrowserArguments` and the environment variable are
        // alternatives, not a merge), so whichever loses is dead text that
        // still reads like it is doing something.
        //
        // That matters for the freeze this block exists to fix. If the config
        // wins, `--enable-gpu-rasterization` and `--ignore-gpu-blocklist` have
        // never been applied — and a renderer that fell back to software
        // compositing is a plausible cause of "paints only on input" all by
        // itself, independent of the throttling flags.
        //
        // The full set now lives in tauri.conf.json (the source Tauri passes
        // explicitly). This keeps the env var as a byte-identical fallback so
        // the two can no longer drift, and so a WebView2 build that ignores
        // the API option still gets the same arguments.
        if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
            std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", WEBVIEW2_BROWSER_ARGS);
        }
    }

    // v1.7.83 — Custom Tokio runtime with a sane worker cap.
    //
    // Tauri's default async runtime spawns one worker per LOGICAL core. On
    // modern desktops (12-32 logical cores), that's an order of magnitude
    // more workers than Lucy's mixed I/O + occasional SIMD workload can
    // actually use. Two real consequences observed:
    //   • Scheduler thrash — short-lived tasks (a 200-token Ollama embed)
    //     get bounced across cores, killing L1/L2 cache locality.
    //   • Wakeup overhead on hybrid CPUs (12th-gen Intel+, AMD with chiplets)
    //     where cross-die wakeups cost 200-500 ns each.
    //
    // Cap at min(8, logical_cores). 8 covers any realistic concurrent
    // workload Lucy generates (a few open streams + the proactive detector
    // tick + the audit batch flusher + tier health probes). MUST be set
    // BEFORE tauri::Builder::default() because Tauri reads the global
    // async-runtime handle on first .plugin() call.
    {
        use std::thread::available_parallelism;
        let workers = available_parallelism()
            .map(|n| n.get().min(8).max(2))
            .unwrap_or(4);
        if let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .thread_name("lucy-tokio")
            .enable_all()
            .build()
        {
            tauri::async_runtime::set(rt.handle().clone());
            // Leak the runtime so it lives the lifetime of the process.
            // Without this, dropping `rt` at end of block would tear down
            // the runtime while Tauri still holds tasks on it.
            Box::leak(Box::new(rt));
            eprintln!("[tokio] lucy runtime: {} worker threads", workers);
        }
    }

    // v1.7.93 — Register sqlite-vec auto-extension BEFORE any connection
    // is opened. The auto-extension hook fires at each `sqlite3_open` so
    // every pooled connection inherits the vec0 virtual-table type.
    // Failure here is non-fatal — vec_search degrades to no-op and the
    // app continues with the legacy linear cosine scan path.
    match commands::vec_search::init_extension() {
        Ok(_)  => eprintln!("[vec_search] sqlite-vec auto-extension registered"),
        Err(e) => eprintln!("[vec_search] init failed (degrading to legacy cosine): {}", e),
    }

    tauri::Builder::default()
        // v1.4.10 — Single-instance: if a second Lucy is launched, focus
        // the existing window instead of spawning a duplicate process.
        // CRITICAL: prevents two processes racing for the SQLite write
        // lock (which would also explain part of the WAL bloat the audit
        // flagged). Must be the FIRST plugin so it runs before other
        // setup that would otherwise initialize twice.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        // v1.4.10 — Window-state: persist size, position, and maximized
        // state across launches. Default storage under
        // %APPDATA%\<bundle-id>\window-state.json. Silently loads on
        // first webview window creation; no further wiring needed.
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // v1.7.19 — log SIMD backend chosen for the cosine-similarity
            // hot path (skills auto-routing Tier 2, memory grounding).
            // Resolving here primes the OnceLock so the first real call
            // doesn't pay the CPUID branch.
            {
                let b = crate::utils::simd_cosine::backend();
                eprintln!("[simd_cosine] backend selected at boot: {}", b.name());
            }

            // v1.4.10 — DB background maintenance task. Spawns a single
            // hourly tokio task that prunes stale rows from chip_click_log
            // / conversation_turns / task_events, runs `wal_checkpoint
            // (TRUNCATE)` to reclaim WAL file space, and runs `PRAGMA
            // optimize`. Idempotent: re-calling spawn_background_maintenance
            // is a no-op thanks to the internal AtomicBool guard.
            // Disable via LUCY_DB_MAINT_DISABLE=1 (CI / tests).
            commands::db_maintenance::spawn_background_maintenance();

            // v1.7.80 — Proactive Operations Assistant background loop.
            // Watches memory pipeline, stream sessions, app log, DB size,
            // integrity, and audit-trail failure rates. Surfaces insights
            // through the proactive_insights table; the frontend polls
            // `proactive_insights_recent` every ~2 minutes and toasts
            // any new ones. Cooldown of 4 h prevents nagging.
            commands::proactive_detector::start_background_loop();

            // v1.7.232 — Background vulnerability watch. Every 6 h, scans the
            // local host's installed software against the curated CVE DB and
            // toasts when the CRITICAL/HIGH set changes — so the operator is
            // alerted even with the cockpit closed. Windows-only (no-op else).
            commands::cve_match::start_vuln_watch_loop(handle.clone());
            // v1.7.233 M2 — síntesis jerárquica de documentos ingeridos
            // (resúmenes densos por lote de secciones vía LLM local).
            commands::pdf::start_pdf_synth_loop();

            // v1.7.89 — Fast no-LLM dedup loop. Every 30 minutes, scans
            // memories saved in the last hour for near-duplicates
            // (tag-Jaccard ≥ 0.90, title 3-gram cosine ≥ 0.92, or
            // verbatim content prefix collision) and supersedes the
            // older twin. Complements the 24 h LLM consolidation by
            // catching same-session noise before it accumulates.
            commands::auto_dedup::start_background_loop();

            // v1.7.95 — Tier-A self-care schedulers. Five loops that keep
            // Lucy fit without operator intervention:
            //   • embed_warmup        — one-shot, populate v1.7.83 LRU.
            //   • audit_verify        — 12 h hash-chain re-verification.
            //   • mcp_health          —  5 min MCP server liveness probe.
            //   • crystal_promo       —  6 h promote hot memories.
            //   • snapshot_retention  —  6 h prune old state_snapshots.
            // Each can be individually disabled via env (LUCY_HK_NO_*).
            commands::housekeeping::start_all(&handle);

            // v1.7.93 — One-shot sqlite-vec backfill. Runs once on app
            // start (in a blocking background task) to copy any
            // pre-existing rows from the legacy `embeddings` table into
            // the new vec0 HNSW index. Idempotent — subsequent boots
            // skip entries already present.
            tauri::async_runtime::spawn(async {
                // Wait a bit so the DB pool is settled.
                tokio::time::sleep(std::time::Duration::from_secs(45)).await;
                let _ = tauri::async_runtime::spawn_blocking(|| {
                    let _ = crate::commands::metrics::shared_db(|conn| {
                        match crate::commands::vec_search::backfill_from_embeddings(conn) {
                            Ok((ins, _skip, err)) => {
                                if ins > 0 || err > 0 {
                                    crate::utils::logging::write_app_log(
                                        "INFO",
                                        &format!("vec_search backfill: inserted={} errored={}", ins, err),
                                    );
                                }
                            }
                            Err(e) => {
                                crate::utils::logging::write_app_log(
                                    "WARN",
                                    &format!("vec_search backfill failed: {}", e),
                                );
                            }
                        }
                        Ok::<(), String>(())
                    });
                }).await;
            });

            // ── OpenClaw Gateway — token-protected localhost webhook receiver ──
            // Opt-out via `LUCY_DISABLE_OPENCLAW=1`. Auth required: clients must
            // send `Authorization: Bearer <token>` header. Token is written to
            // `%APPDATA%\Lucy\openclaw_token` (Windows-ACL restricted to current
            // user) — trusted automations read it from there. Body must be valid
            // JSON, ≤64KB. Rate-limited to 30 req/min per peer.
            let openclaw_disabled = std::env::var("LUCY_DISABLE_OPENCLAW")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

            if !openclaw_disabled {
                let openclaw_token = crate::state::generate_secure_token();
                let token_file_path = {
                    let logs_dir = crate::utils::logging::get_logs_dir();
                    logs_dir.parent()
                        .map(|p| p.join("openclaw_token"))
                        .unwrap_or_else(|| logs_dir.join("openclaw_token"))
                };
                match std::fs::write(&token_file_path, &openclaw_token) {
                    Ok(_) => {
                        crate::utils::logging::write_app_log(
                            "INFO",
                            &format!("OpenClaw token written to: {}", token_file_path.display()),
                        );
                    }
                    Err(e) => {
                        crate::utils::logging::write_app_log(
                            "WARNING",
                            &format!("Failed to write openclaw_token file: {} — gateway will reject all requests", e),
                        );
                    }
                }
                // Windows: restrict file ACL to current user only (no inherit, no group)
                // MED-2 FIX: expand %USERNAME% via std::env — Command::arg passes
                // strings verbatim to CreateProcess which does NOT expand env vars.
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    if let Some(path_str) = token_file_path.to_str() {
                        let username = std::env::var("USERNAME").unwrap_or_default();
                        if !username.is_empty() {
                            let grant_arg = format!("{}:F", username);
                            let _ = std::process::Command::new("icacls")
                                .args([path_str, "/inheritance:r", "/grant:r", &grant_arg])
                                .creation_flags(crate::state::CREATE_NO_WINDOW)
                                .output();
                        }
                    }
                }

                let token_for_gateway = openclaw_token;
                let handle_for_gateway = handle.clone();
                tauri::async_runtime::spawn(async move {
                    use tauri::Emitter;
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    use std::collections::HashMap;
                    use std::sync::Arc;
                    use std::sync::Mutex as StdMutex;
                    use std::time::{Instant, Duration};

                    // Rate limiter: peer IP → (window_start, request_count)
                    // 30 req / 60 s. Map auto-prunes entries older than the window.
                    //
                    // KEY MUST BE THE IP, NOT THE SocketAddr. `SocketAddr`'s Display
                    // includes the EPHEMERAL PORT (`127.0.0.1:54321`), which is
                    // different on every TCP connection — keying on it gave each
                    // request a fresh bucket with count 1, so the limit never fired
                    // and the whole limiter was dead code that read as working.
                    let rate_state: Arc<StdMutex<HashMap<String, (Instant, u32)>>> =
                        Arc::new(StdMutex::new(HashMap::new()));

                    if let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:31337").await {
                        eprintln!("[lucy] OpenClaw Gateway running on 127.0.0.1:31337 (token-protected)");
                        while let Ok((mut socket, addr)) = listener.accept().await {
                            let h = handle_for_gateway.clone();
                            let token = token_for_gateway.clone();
                            let rl = rate_state.clone();
                            tauri::async_runtime::spawn(async move {
                                // ── Rate limit ─────────────────────────────────
                                let peer_key = addr.ip().to_string();
                                let allowed = match rl.lock() {
                                    Ok(mut map) => {
                                        let now = Instant::now();
                                        // Prune stale entries opportunistically (cap map at 256)
                                        if map.len() > 256 {
                                            map.retain(|_, (ts, _)| now.duration_since(*ts) < Duration::from_secs(60));
                                        }
                                        let entry = map.entry(peer_key.clone()).or_insert((now, 0));
                                        if now.duration_since(entry.0) > Duration::from_secs(60) {
                                            *entry = (now, 1);
                                            true
                                        } else {
                                            entry.1 += 1;
                                            entry.1 <= 30
                                        }
                                    }
                                    Err(_) => false, // fail-closed on poisoned mutex
                                };
                                if !allowed {
                                    let _ = socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                                    let _ = socket.shutdown().await;
                                    return;
                                }

                                // ── Read with hard cap ────────────────────────
                                //
                                // MUST LOOP. A single `read()` returns whatever one
                                // TCP segment happened to carry — it is NOT the whole
                                // request. With one read, a payload past the MSS or a
                                // client that flushes headers separately got silently
                                // truncated: a cut body produced a bogus 400, and a cut
                                // `Authorization` header produced a bogus 401. Both
                                // fail closed (no security hole) but they made the
                                // gateway look intermittently broken for valid clients.
                                //
                                // Read until the header terminator, then honour
                                // Content-Length for the body. 64 KiB hard cap and a
                                // 5 s total deadline still bound the work.
                                const MAX_REQ: usize = 65_536;
                                let mut buf: Vec<u8> = Vec::with_capacity(4096);
                                let mut chunk = [0u8; 4096];
                                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

                                // Phase 1 — headers.
                                let head_end = loop {
                                    if let Some(idx) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                        break idx + 4;
                                    }
                                    if buf.len() >= MAX_REQ {
                                        let _ = socket.write_all(b"HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                                        let _ = socket.shutdown().await;
                                        return;
                                    }
                                    match tokio::time::timeout_at(deadline, socket.read(&mut chunk)).await {
                                        Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                                        // EOF before the terminator, read error, or deadline.
                                        _ => {
                                            let _ = socket.shutdown().await;
                                            return;
                                        }
                                    }
                                };

                                // Phase 2 — body, sized by Content-Length when present.
                                let content_len: usize = {
                                    let head = String::from_utf8_lossy(&buf[..head_end]);
                                    head.lines()
                                        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                                        .and_then(|l| l.split(':').nth(1))
                                        .and_then(|v| v.trim().parse::<usize>().ok())
                                        .unwrap_or(0)
                                };
                                if content_len > MAX_REQ {
                                    let _ = socket.write_all(b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                                    let _ = socket.shutdown().await;
                                    return;
                                }
                                while buf.len() < head_end + content_len {
                                    if buf.len() >= MAX_REQ { break; }
                                    match tokio::time::timeout_at(deadline, socket.read(&mut chunk)).await {
                                        Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                                        _ => break, // short body: let the JSON check below reject it
                                    }
                                }

                                let req = match std::str::from_utf8(&buf) {
                                    Ok(s) => s,
                                    Err(_) => {
                                        let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                                        let _ = socket.shutdown().await;
                                        return;
                                    }
                                };

                                // ── Auth: require Authorization: Bearer <token> ──
                                //
                                // NOT a constant-time comparison — `==` on &str short-
                                // circuits on the first differing byte. That is accepted
                                // here, not overlooked: the token is 256 bits of CSPRNG
                                // output, the listener is loopback-only, and the (now
                                // functional) per-IP limiter caps attempts at 30/min.
                                // Remote timing extraction of a 64-hex secret under those
                                // constraints is not a practical attack. Do NOT copy this
                                // pattern to a secret that is short, low-entropy, or
                                // reachable off-host.
                                //
                                // Scan the HEADER SECTION only — `req` now carries the
                                // body too, and a token echoed inside a JSON payload must
                                // never satisfy auth.
                                let auth_ok = req[..head_end]
                                    .lines()
                                    .any(|line| {
                                        let trimmed = line.trim();
                                        let lower = trimmed.to_ascii_lowercase();
                                        lower.starts_with("authorization:")
                                            && trimmed
                                                .split_ascii_whitespace()
                                                .any(|w| w == token.as_str())
                                    });

                                if !auth_ok {
                                    let _ = socket.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                                    let _ = socket.shutdown().await;
                                    crate::utils::logging::write_app_log(
                                        "WARNING",
                                        &format!("OpenClaw: rejected unauthorized request from {}", peer_key),
                                    );
                                    return;
                                }

                                // ── Extract body ──────────────────────────────
                                // `head_end` is already the byte offset past the
                                // terminator, so no second scan is needed.
                                let body = req[head_end..].trim().to_string();

                                if body.is_empty() || body.len() > 65_536 {
                                    let status = if body.is_empty() { "400 Bad Request" } else { "413 Payload Too Large" };
                                    let resp = format!("HTTP/1.1 {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", status);
                                    let _ = socket.write_all(resp.as_bytes()).await;
                                    let _ = socket.shutdown().await;
                                    return;
                                }

                                // ── Strict JSON validation ────────────────────
                                if serde_json::from_str::<serde_json::Value>(&body).is_err() {
                                    let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 14\r\nConnection: close\r\n\r\nExpected JSON\n").await;
                                    let _ = socket.shutdown().await;
                                    return;
                                }

                                // ── Guardrail: the payload is UNTRUSTED DATA ──
                                // The frontend drops this straight into Lucy's
                                // conversation context, so it reaches the model. Every
                                // other path that feeds the model — tool output, remote
                                // shell, assistant scripts — is scanned; this one was
                                // not, which made the gateway the one unguarded way to
                                // put attacker-chosen text in front of the agent. A
                                // holder of the token is not necessarily the author of
                                // the payload: an integration relaying an issue body or
                                // an email carries a third party's words.
                                let gscan = crate::guardrails::scan(&body, crate::guardrails::Role::Tool);
                                if !matches!(gscan.decision, crate::guardrails::ScanDecision::Allow) {
                                    crate::utils::logging::write_app_log(
                                        "WARNING",
                                        &format!(
                                            "OpenClaw: payload from {} blocked by guardrail [{}]",
                                            peer_key, gscan.reason
                                        ),
                                    );
                                    let _ = socket.write_all(b"HTTP/1.1 422 Unprocessable Entity\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                                    let _ = socket.shutdown().await;
                                    return;
                                }

                                let _ = h.emit("openclaw_webhook", body);
                                let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nOK\n").await;
                                let _ = socket.shutdown().await;
                            });
                        }
                    } else {
                        crate::utils::logging::write_app_log(
                            "WARNING",
                            "OpenClaw Gateway: could not bind to 127.0.0.1:31337 (port in use?)",
                        );
                    }
                });
            } else {
                crate::utils::logging::write_app_log(
                    "INFO",
                    "OpenClaw Gateway disabled via LUCY_DISABLE_OPENCLAW env var",
                );
            }

            // ── BOOT-TIME INTEGRITY CHECK ─────────────────────────────────
            // Logged-only by design: a Mismatch could legitimately mean the
            // user just installed a new release. Hard-failing would lock
            // them out. Code signing + a signed updater that rewrites the
            // anchor are the right answer for stronger enforcement.
            match crate::utils::integrity::check_self_integrity() {
                crate::utils::integrity::IntegrityVerdict::Mismatch { .. } => {
                    crate::utils::logging::write_app_log(
                        "WARNING",
                        "Integrity anchor mismatch — binary may have been patched, or a fresh release was installed."
                    );
                }
                crate::utils::integrity::IntegrityVerdict::FirstBoot => {
                    crate::utils::logging::write_app_log(
                        "INFO",
                        "Integrity anchor written (first boot of this binary)."
                    );
                }
                _ => {}
            }
            if crate::utils::integrity::debugger_present() {
                crate::utils::logging::write_app_log(
                    "WARNING",
                    "Debugger detected attached at startup."
                );
            }

            // Initialize the shared metrics/indexer DB once at startup.
            // v1.7.238 — arranque RESILIENTE para operación desatendida: reintenta
            // locks transitorios y recupera de corrupción (cuarentena + recreación).
            // Si aun así falla, se registra FATAL en lucy_app.log (archivo, no la DB)
            // porque `eprintln!` es invisible en un binario GUI de Windows release —
            // antes esto dejaba a Lucy zombie TODA la noche sin rastro.
            if let Err(e) = metrics::init_resilient(app.handle()) {
                crate::utils::logging::write_app_log(
                    "FATAL",
                    &format!("metrics DB no disponible tras init resiliente: {} — memoria, permisos y tareas programadas quedan DESHABILITADAS esta sesión.", e),
                );
            }

            // Warm up the tiktoken BPE table on a background thread so the
            // first read_file_content() call doesn't pay the ~200ms init cost.
            std::thread::spawn(|| {
                crate::commands::local::warmup_tokenizer();
            });

            // ── Memory auto-forget sweep (Tier 1 #1) ────────────────────
            // Runs once after startup (not blocking launch), then every
            // 12h. Cleans TTL-expired memories + low-value old rows. Errors
            // are logged but never crash the app — the memory store works
            // fine without this, it just grows slowly.
            //
            // v1.7.104 Sprint-4 perf: warmup bumped 60s → 5min. At 60s
            // the sweep raced the LLM warmup + sqlite-vec backfill +
            // process_lineage first tick, adding 200-600 ms to "first
            // interaction" responsiveness. 5 min puts it well clear of
            // any boot-time contention window.
            tauri::async_runtime::spawn(async {
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                loop {
                    match crate::commands::metrics::auto_forget_run(Some(false)) {
                        Ok(r) if r.total_deleted > 0 => {
                            crate::utils::logging::write_app_log(
                                "INFO",
                                &format!(
                                    "auto-forget: ttl={} low_value={} total={}",
                                    r.ttl_expired, r.low_value, r.total_deleted
                                ),
                            );
                        }
                        Ok(_) => {}  // Nothing to clean — silent
                        Err(e) => {
                            crate::utils::logging::write_app_log(
                                "WARN",
                                &format!("auto-forget run failed: {}", e),
                            );
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(43_200)).await;  // 12h
                }
            });

            // ── Memory auto-consolidation (Tier 2 #5) ──────────────────
            // Runs once 5 min after startup (after auto-forget has had a
            // chance to prune), then every 24h. Clusters related memories
            // by shared tags and asks the LLM to fuse each cluster into
            // one durable memory; originals are marked superseded (audit
            // trail preserved). Network failures or Ollama-offline degrade
            // gracefully — the run logs and exits, retries next cycle.
            tauri::async_runtime::spawn(async {
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                loop {
                    match crate::commands::metrics::auto_consolidate_run(Some(false)).await {
                        Ok(r) if r.new_memories > 0 => {
                            crate::utils::logging::write_app_log(
                                "INFO",
                                &format!(
                                    "auto-consolidate: eligible={} clusters_found={} processed={} new={} superseded={}",
                                    r.eligible_memories, r.clusters_found,
                                    r.clusters_processed, r.new_memories, r.memories_superseded
                                ),
                            );
                        }
                        Ok(_) => {}  // No clusters worth fusing yet
                        Err(e) => {
                            crate::utils::logging::write_app_log(
                                "WARN",
                                &format!("auto-consolidate failed: {}", e),
                            );
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(86_400)).await;  // 24h
                }
            });

            // ── Reflection — meta-insight pass (Tier 3 #8) ─────────────
            // Runs once 15 min after startup (well after auto-forget +
            // auto-consolidate have shaped the memory store), then every
            // 48h. Each run clusters memories by tag overlap and asks
            // the LLM to derive ONE generalisable meta-insight per cluster
            // — those either create a new agent_insights row or reinforce
            // an existing one (confidence asymptotic toward 1.0).
            tauri::async_runtime::spawn(async {
                tokio::time::sleep(std::time::Duration::from_secs(900)).await;
                loop {
                    match crate::commands::metrics::reflect_run(Some(false)).await {
                        Ok(r) if r.insights_created > 0 || r.insights_reinforced > 0 => {
                            crate::utils::logging::write_app_log(
                                "INFO",
                                &format!(
                                    "reflect: eligible={} processed={} created={} reinforced={}",
                                    r.eligible_memories, r.clusters_processed,
                                    r.insights_created, r.insights_reinforced
                                ),
                            );
                        }
                        Ok(_) => {}
                        Err(e) => {
                            crate::utils::logging::write_app_log(
                                "WARN",
                                &format!("reflect failed: {}", e),
                            );
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(172_800)).await;  // 48h
                }
            });

            // ── Memory graph rebuild (Tier 3 #9) ───────────────────────
            // Refreshes agent_memory_edges nightly so the graph reflects
            // recent saves + consolidate's supersede sweeps. Runs 30 min
            // after launch (well after auto-consolidate at +5 min so the
            // graph sees the consolidation outputs), then every 24h.
            tauri::async_runtime::spawn(async {
                tokio::time::sleep(std::time::Duration::from_secs(1800)).await;
                loop {
                    match crate::commands::metrics::graph_rebuild_edges_run() {
                        Ok(r) if r.total_directed_edges > 0 => {
                            crate::utils::logging::write_app_log(
                                "INFO",
                                &format!(
                                    "graph rebuild: nodes={} concept={} file={} session={} kept={}",
                                    r.eligible_memories, r.concept_edges,
                                    r.file_edges, r.session_edges, r.total_directed_edges
                                ),
                            );
                        }
                        Ok(_) => {}
                        Err(e) => {
                            crate::utils::logging::write_app_log(
                                "WARN",
                                &format!("graph rebuild failed: {}", e),
                            );
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(86_400)).await;  // 24h
                }
            });

            // ── Capacity metrics sampling (every 5 min) ─────────────────
            // Automatically records CPU/RAM/disk to metrics_samples table.
            // Runs in background — never blocks the UI. Also runs hourly
            // downsampling to keep the table size bounded.
            tauri::async_runtime::spawn(async {
                // v1.7.104 Sprint-4 perf: 167s warmup (was 120s). The
                // extra 47s phase-shifts the 5-min capacity sample off
                // the top-of-hour grid where db_maintenance (3600s) and
                // ollama_model_health (3600s) collide. With this shift,
                // no three writers compete for the r2d2 pool at the
                // same epoch-aligned tick.
                tokio::time::sleep(std::time::Duration::from_secs(167)).await;
                let mut sample_interval = tokio::time::interval(std::time::Duration::from_secs(300)); // 5 min
                let mut downsample_counter: u32 = 0;
                loop {
                    sample_interval.tick().await;
                    // Save a local metrics sample
                    match crate::commands::capacity::save_metrics_sample(
                        None, None, None, None, None, None, None, None,
                    ).await {
                        Ok(_) => {}
                        Err(e) => {
                            crate::utils::logging::write_app_log(
                                "WARN",
                                &format!("Capacity auto-sample failed: {}", e),
                            );
                        }
                    }
                    // Downsample every 12 ticks (1 hour)
                    downsample_counter += 1;
                    if downsample_counter >= 12 {
                        downsample_counter = 0;
                        match crate::commands::capacity::downsample_metrics().await {
                            Ok(n) if n > 0 => {
                                crate::utils::logging::write_app_log(
                                    "INFO",
                                    &format!("Capacity downsample: {} hourly buckets created", n),
                                );
                            }
                            Ok(_) => {}
                            Err(e) => {
                                crate::utils::logging::write_app_log(
                                    "WARN",
                                    &format!("Capacity downsample failed: {}", e),
                                );
                            }
                        }
                    }
                }
            });

            // ── Periodic janitor (every 5 min) ──────────────────────────
            // Keeps in-memory state from leaking when sessions/tokens die
            // through abnormal paths (crash, abrupt disconnect, app sleep).
            tauri::async_runtime::spawn(async {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                interval.tick().await; // skip the immediate first tick (state is fresh)
                loop {
                    interval.tick().await;
                    crate::state::purge_expired_bypass_tokens();
                    let killed = crate::state::purge_dead_stream_sessions();
                    if killed > 0 {
                        crate::utils::logging::write_app_log(
                            "INFO",
                            &format!("Janitor: cleaned up {} dead stream session(s)", killed),
                        );
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Guardrails (audit S1/S2/S5/S10) + PromptGuard 2 ML status
            guardrails::guardrail_scan,
            guardrails::guardrail_scan_url,
            guardrails::prompt_guard_status,
            guardrails::download_prompt_guard_model,
            // AI
            ai::ask_lucy,
            ai::get_cache_stats,
            // v1.7.235 — JSON tool-output tabular compression (lossless, reversible)
            utils::json_tabular::compress_tool_output,
            utils::json_tabular::expand_tool_output,
            commands::mcp::call_mcp_tool,
            commands::mcp::discover_mcp_tools,
            commands::mcp::mcp_server_list,
            commands::mcp::mcp_server_upsert,
            commands::mcp::mcp_server_delete,
            commands::mcp::mcp_server_discover,
            commands::mcp::mcp_server_test,
            commands::mcp::mcp_server_call,
            commands::mcp::mcp_pool_stats,
            commands::mcp::mcp_pool_clear,
            commands::smart_chips::generate_smart_chips,
            commands::smart_chips::generate_tab_title,
            commands::chip_memory::log_chip_event,
            commands::chip_memory::suggest_memory_chips,
            commands::chip_memory::chip_stats_summary,
            commands::db_maintenance::db_maintenance_run_now,
            // v1.6.0 — Kappa Graph ADR-044 grounding scores + provenance.
            commands::grounding::memory_grounding,
            commands::grounding::memory_evidence_log,
            commands::grounding::memory_instance_save,
            commands::grounding::memory_instances_for,
            commands::grounding::memory_instances_search,
            // v1.6.5 — Kappa Graph ADR-058 polarity axis triangulation.
            commands::polarity::memory_polarity,
            commands::polarity::memory_polarity_rebuild,
            commands::polarity::memory_polarity_axis,
            // v1.6.6 — Kappa Graph ADR-200 annealing ontologies (read-only MVP).
            commands::annealing::memory_annealing_report,
            commands::annealing::memory_annealing_cluster,
            // v1.6.8 — annealing Phase 4 execution (demote with affinity routing).
            commands::annealing::memory_annealing_demote,
            // v1.7.4 — Anthropic Cybersecurity Skills library.
            commands::security_skills::security_skills_list,
            // v1.7.34 — self-introspection for /capabilities surface.
            commands::security_skills::lucy_capabilities_skills,
            commands::security_skills::security_skills_search,
            commands::security_skills::security_skills_get,
            commands::security_skills::security_skills_categories,
            // v1.7.5 — hybrid auto-routing (keyword + embedding + LLM).
            commands::security_skills::security_skills_auto_route,
            commands::security_skills::security_skills_rebuild_embeddings,
            commands::security_skills::security_skills_embed_status,
            // v1.7.15 — user skills directory + drag-drop install.
            commands::security_skills::security_skills_user_dir,
            commands::security_skills::security_skills_reload,
            commands::security_skills::security_skills_template,
            commands::security_skills::security_skills_install,
            commands::security_skills::security_skills_delete,
            // v1.7.16 — pre-delivery script syntax verification.
            commands::script_verify::verify_script,
            // v1.7.19 — SIMD backend introspection (cosine hot path).
            utils::simd_cosine::simd_info,
            // v1.7.21 — Cross-backend cosine benchmark for /bench-simd.
            utils::simd_cosine::bench_cosine,
            ai::ask_lucy_stream,
            ai::generate_skill_template,
            ai::list_local_models,
            ai::list_nvidia_models,
            ai::fetch_url_content,
            ai::search_runbooks,
            ai::change_agent_dir,
            ai::log_agent_loop,
            // Config / credenciales
            config::save_llm_key,
            config::get_configured_providers,
            config::test_api_key,
            config::save_host_credential,
            config::get_host_credential,
            config::delete_host_credential,
            config::save_mcp_secret,
            config::get_mcp_secret,
            config::delete_mcp_secret,
            config::list_mcp_secrets,
            config::set_mcp_secret_index,
            // UI / ventana / archivos
            ui::copy_to_clipboard,
            ui::minimize_window,
            ui::maximize_window,
            ui::close_window,
            ui::open_shell_window,
            ui::pick_and_read_file,
            ui::pick_multiple_files,
            ui::pick_file_path,
            ui::pick_save_path,
            ui::pick_folder_path,
            ui::pick_file_with_filter,
            ui::pick_pdf_path,
            ui::save_temp_pdf,
            ui::pick_directory,
            ui::save_file_dialog,
            // Sistema local
            system::get_system_health,
            system::get_system_health_json,
            system::kill_process,
            ui::reveal_in_explorer,
            system::get_tavily_api_key_status,
            system::set_tavily_api_key,
            // Hosts remotos
            hosts::execute_remote_windows,
            hosts::get_remote_health_windows,
            hosts::execute_remote_linux,
            hosts::get_remote_health_linux,
            hosts::execute_shell_cmd,
            hosts::nexshell_bootstrap,
            hosts::read_remote_file,
            hosts::write_remote_file,
            // Ejecución local alternativa (CMD, WMIC, netsh, reg, cscript, nativa)
            local::execute_cmd,
            local::execute_wmic,
            local::execute_netsh,
            local::execute_reg,
            local::execute_cscript,
            local::get_network_connections,
            local::get_event_log,
            local::get_tasklist,
            local::read_registry_value,
            local::list_registry_key,
            local::read_file_content,
            local::read_file_lines,
            local::write_file_content,
            local::list_directory,
            local::path_exists,
            local::search_files,
            local::edit_file,
            local::analyze_code,
            local::system_diff,
            local::search_web,
            local::set_tab_cwd,
            local::drop_tab_cwd,
            local::get_tab_cwd,
            local::read_design_md,
            local::open_vscode,
            local::panic_kill_all,
            local::launch_rdp,
            // RDP Computer Use Agent
            rdp_agent::find_rdp_windows,
            rdp_agent::capture_rdp_screenshot,
            rdp_agent::run_rdp_agent,
            // v1.7.126 — Local desktop computer-use, Phase A (Lucy SEES the screen)
            local_screen::capture_local_screen,
            // v1.7.127 — Local desktop computer-use, Phase B (Lucy CONTROLS, gated)
            local_screen::run_local_agent,
            local_screen::cancel_local_agent,
            local_screen::local_agent_running,
            indexer::locate_file,
            indexer::start_indexer,
            // Shell local + streaming interactivo
            shell::execute_powershell,
            shell::stream_shell_cmd,
            shell::send_shell_input,
            shell::kill_shell_session,
            // Log viewer
            logs::log_frontend_event,
            logs::read_log_tail,
            logs::read_remote_log_windows,
            logs::read_remote_log_linux,
            // Inventario de infraestructura
            inventory::discover_inventory_linux,
            inventory::discover_inventory_windows,
            inventory::discover_inventory_local,
            // Compliance / CIS Benchmark
            compliance::run_compliance_linux,
            compliance::run_compliance_windows,
            compliance::run_compliance_local,
            // Metrics / Cost Tracking / Permissions / Skills (Nivel 2)
            metrics::init_metrics_db,
            metrics::log_token_usage,
            metrics::get_cost_summary,
            // v1.7.31 — cost-by-day for the StatusBar sparkline.
            metrics::get_cost_by_day,
            metrics::get_token_history,
            metrics::reset_cost_history,
            metrics::check_permission,
            metrics::save_permission_rule,
            metrics::list_permission_rules,
            metrics::delete_permission_rule,
            metrics::save_skill,
            metrics::list_skills,
            metrics::delete_skill,
            metrics::increment_skill_usage,
            metrics::save_agent_memory,
            metrics::update_agent_memory_tags,
            metrics::update_agent_memory_importance,
            metrics::delete_agent_memory,
            metrics::auto_forget_run,
            metrics::crystallize_session,
            metrics::maybe_auto_crystallize_session,
            metrics::list_crystals,
            metrics::get_crystal,
            metrics::delete_crystal,
            metrics::auto_consolidate_run,
            metrics::reflect_run,
            metrics::list_insights,
            metrics::delete_insight,
            metrics::graph_rebuild_edges_run,
            metrics::graph_neighbors,
            metrics::save_session_summary,
            metrics::get_session_summary,
            metrics::delete_session_summary,
            commands::dedup::dedup_acquire,
            commands::dedup::dedup_release,
            commands::dedup::dedup_stats,
            commands::reranker::reranker_status,
            commands::reranker::download_reranker_model,
            metrics::consolidate_agent_memories,
            metrics::supersede_memory,
            metrics::search_agent_memories,
            // v1.7.236 — Lote 1 memoria: refuerzo por citas (R2) + pines (R5)
            metrics::touch_memories_by_ids,
            metrics::set_memory_pinned,
            metrics::get_pinned_memories,
            metrics::search_agent_memories_expanded,
            metrics::get_recent_memories,
            metrics::get_recent_memories_filtered,  // v1.7.233 — importance filter in SQL
            metrics::memory_browser_stats,          // v1.7.233 — real corpus totals for the browser cards
            // User Profile (Hermes-inspired persistent memory)
            metrics::set_user_profile,
            metrics::get_user_profile,
            metrics::delete_user_profile,
            metrics::build_profile_context,
            // Conversation history / recall (Hermes-inspired)
            metrics::save_conversation_turn,
            metrics::recall_conversations,
            // Quality Telemetry (opus-4-7 Tier 2.A)
            metrics::log_task_event,
            metrics::get_task_telemetry,
            metrics::loop_block_stats,
            // v1.7.99 — D3: per-model latency sparkline data source.
            metrics::recent_model_latencies,
            // v1.7.100 — D1: interactive PTY for the in-app xterm pane.
            pty::pty_open,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_close,
            pty::pty_status,
            metrics::get_confidence_distribution,
            // Provider Management (Multi-LLM Support)
            providers::save_credential,
            providers::has_credential,
            providers::check_provider_health,
            // Incident Response / SRE Mode (Nivel 4)
            incident::incident_start,
            incident::incident_advance_phase,
            incident::incident_add_evidence,
            incident::incident_list_evidence,
            incident::incident_propose_hypothesis,
            incident::incident_list_hypotheses,
            incident::incident_calculate_score,
            incident::incident_log_action,
            incident::incident_finalize,
            incident::incident_list,
            incident::incident_get,
            incident::incident_phase_prompt,
            incident::incident_verify_chain,
            // F2 Frontier — State snapshots (system state capture + temporal diff)
            state_snapshot::state_snapshot_capture,
            state_snapshot::state_snapshot_latest,
            state_snapshot::state_snapshot_list,
            state_snapshot::state_snapshot_diff,
            // F1 Frontier — Process lineage tracker (parent chain + audit hash)
            process_lineage::process_lineage_poll,
            process_lineage::process_lineage_list,
            process_lineage::process_lineage_for_pid,
            process_lineage::process_lineage_search,
            process_lineage::process_lineage_verify_chain,
            // F4 Frontier — Self-healing pattern engine
            self_healing::healing_save_pattern,
            self_healing::healing_mark_success,
            self_healing::healing_find_similar,
            self_healing::healing_list_all,
            self_healing::healing_delete_pattern,
            // F3 Frontier — Causal inference engine
            causal::diagnose_spike,
            // F8 Frontier — Mini-EDR behavioral threat scanner
            threat_scan::threat_scan,
            // F6 Frontier — Cross-app object bridge (PowerShell objects pipeable across turns)
            object_bridge::obj_bridge_store,
            object_bridge::obj_bridge_list,
            object_bridge::obj_bridge_clear,
            object_bridge::obj_bridge_query,
            // F7 Frontier — Runbook generator (sequence mining over user history)
            runbook_gen::runbook_scan,
            // F10 Frontier — Daily routine learning
            daily_patterns::daily_patterns_scan,
            // F5 Frontier — Sandbox-first preview of destructive commands
            sandbox_preview::sandbox_preview_command,
            // F9 Frontier — Personal Knowledge Graph
            knowledge_graph::kg_add_root,
            knowledge_graph::kg_remove_root,
            knowledge_graph::kg_list_roots,
            knowledge_graph::kg_index_now,
            knowledge_graph::kg_recent_files,
            knowledge_graph::kg_neighbors,
            knowledge_graph::kg_ext_summary,
            // F7 Sprint 7 — Promote a detected workflow into a saved skill
            runbook_gen::runbook_promote,
            // Cross-feature — Incident Detective (F3 + F8 + F9 synthesis)
            incident_detective::incident_detective,
            // Sprint 8 — Frontier telemetry (which Frontier tools the user actually uses)
            frontier_telemetry::frontier_telemetry_record,
            frontier_telemetry::frontier_telemetry_summary,
            frontier_telemetry::frontier_telemetry_clear,
            // Sprint 1, UI-1 — Activity Feed sidebar widget
            activity_feed::activity_feed,
            // Semantic embeddings (Sprint 2 — vector search on skills, memories, runbooks)
            embeddings::embed_text,
            embeddings::embeddings_available,
            embeddings::upsert_embedding,
            embeddings::delete_embedding,
            embeddings::semantic_search,
            embeddings::backfill_embeddings,
            // Tiered memory — MemGPT-style (Sprint 3)
            memory::memory_core_set,
            memory::memory_core_list,
            memory::memory_core_delete,
            memory::memory_core_render,
            memory::memory_core_reinforce,
            memory::memory_core_decay_stats,
            memory::memory_consolidate,
            memory::memory_graph,
            replay::replay_save,
            replay::replay_list,
            replay::replay_get,
            replay::replay_bump_count,
            replay::replay_relabel,
            replay::replay_delete,
            replay::replay_clear_old,
            replay::replay_drift,
            shell_recording::shell_recording_start,
            shell_recording::shell_recording_append,
            shell_recording::shell_recording_finish,
            shell_recording::shell_recording_list,
            shell_recording::shell_recording_events,
            shell_recording::shell_recording_delete,
            shell_recording::shell_recording_rename,
            cve_match::cve_scan,
            db_backup::db_info,
            db_backup::db_backup_create,
            db_backup::db_backup_restore,
            support_bundle::export_support_bundle,
            inventory_drift::inventory_set_baseline,
            inventory_drift::inventory_get_baseline,
            inventory_drift::inventory_delete_baseline,
            inventory_drift::inventory_compute_drift,
            dashboard_integrations::dashboard_open_incidents,
            dashboard_integrations::dashboard_process_lineage_brief,
            dashboard_integrations::dashboard_failed_logins_24h,
            dashboard_integrations::dashboard_failed_logins_detail,
            hash_chain::verify_incident_chain,
            memory::memory_working_append,
            memory::memory_working_list,
            memory::memory_working_clear,
            memory::memory_stats,
            memory::memory_health,
            // Fork persistence — Parallel Agents (Sprint 4 Pillar 1)
            metrics::fork_save,
            metrics::fork_update,
            metrics::fork_get,
            metrics::fork_list,
            metrics::fork_clear,
            // PDF Intelligence — Sprint 4 Pillar 4
            pdf::pdf_ingest,
            pdf::extract_pdf_text_from_bytes,
            pdf::pdf_list_docs,
            pdf::pdf_delete_doc,
            pdf::pdf_search,
            web_ingest::web_ingest,
            // Principles (Maestro-inspired) — behavioral rules in system prompt
            commands::principles::save_principle,
            commands::principles::update_principle,
            commands::principles::delete_principle,
            commands::principles::list_principles,
            // Scheduled tasks (Hermes-inspired natural-language cron)
            commands::scheduled::save_scheduled_task,
            commands::scheduled::list_scheduled_tasks,
            commands::scheduled::due_scheduled_tasks,
            commands::scheduled::mark_scheduled_run,
            commands::scheduled::toggle_scheduled_task,
            commands::scheduled::delete_scheduled_task,
            // Reflection safety gate (pre-emission analysis)
            reflection::reflect_on_response,
            // Prompt section runtime toggles (Phase 5)
            commands::prompt_sections::toggle_prompt_section,
            commands::prompt_sections::list_prompt_sections,
            // ── P0 Features ──────────────────────────────────────────────
            // Audit Trail (P0 Feature 1)
            audit::save_audit_entry,
            audit::query_audit_trail,
            audit::prune_audit_trail,
            audit::audit_stats,
            // Log Pattern Analysis (P0 Feature 2)
            log_analysis::analyze_log_patterns,
            // Capacity Planning (P0 Feature 3)
            capacity::save_metrics_sample,
            capacity::get_capacity_trends,
            capacity::capacity_projection,
            capacity::downsample_metrics,
            // OS Notifications (P0 Feature 4)
            notify::send_notification,
            notify::check_notification_permission,
            commands::notify_bridge::notify_bridge_status,
            commands::notify_bridge::notify_bridge_save,
            commands::notify_bridge::notify_bridge_clear,
            commands::notify_bridge::notify_bridge_send,
            commands::notify_bridge::notify_bridge_test,
            notify::request_notification_permission,
            // Self-Diagnostics (P0 Feature 5)
            diagnostics::run_self_diagnostics,
            // v1.7.64 — Repair commands surfaced as buttons inside the panel
            diagnostics::repair_agent_memories_confidence,
            // v1.7.70 — Additional repair handlers for the remaining warning
            // triggers (DB size, expired memories, leaked stream sessions,
            // oversized app log).
            diagnostics::repair_database_vacuum,
            diagnostics::repair_memory_purge_expired,
            diagnostics::repair_pdf_embeddings,
            diagnostics::repair_memory_consolidate,
            diagnostics::repair_clear_leaked_stream_sessions,
            diagnostics::repair_rotate_app_log,
            // v1.7.73 — Auto-fork advisor. Scores the user prompt for
            // parallel-branch potential so the composer can show a chip
            // and the prompt builder can inject a strong directive.
            commands::fork_advisor::fork_advice,
            // v1.7.80 — Proactive Operations Assistant. Frontend polls
            // these; backend background loop populates the insights table.
            commands::proactive_detector::proactive_insights_recent,
            commands::proactive_detector::proactive_insight_dismiss,
            commands::proactive_detector::proactive_run_once,
            // v1.7.85 — Memory Graph layout cache.
            commands::graph_layout_cache::graph_layout_load,
            commands::graph_layout_cache::graph_layout_save_bulk,
            commands::graph_layout_cache::graph_layout_clear,
            // v1.7.87 — Typed semantic links between memories.
            commands::semantic_links::memory_link_add,
            commands::semantic_links::memory_link_list,
            commands::semantic_links::memory_link_remove,
            commands::semantic_links::memory_link_kinds,
            // v1.7.89 — Manual trigger for the fast dedup pass.
            commands::auto_dedup::auto_dedup_run,
            // v1.7.94 — Hybrid SQL+vector recall exposed to the frontend.
            commands::vec_search::vec_search_query,
        ])
        .run(lucy_context())
        .expect("Error al iniciar Lucy");
}

/// Chromium command-line flags for the embedded WebView2.
///
/// MUST stay byte-identical to `app.windows[].additionalBrowserArgs` in
/// tauri.conf.json — pinned by the test at the bottom of this file. The two are
/// alternative delivery routes for the same arguments, not a merge: whichever
/// one WebView2 does not take is dead text that still reads like configuration.
/// They had already drifted, and the half that lost was the half carrying the
/// GPU flags.
#[cfg(windows)]
const WEBVIEW2_BROWSER_ARGS: &str = concat!(
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,",
    "CalculateNativeWinOcclusion,IntensiveWakeUpThrottling,BatterySaverModeAlignWakeUps ",
    "--disable-background-timer-throttling ",
    "--disable-renderer-backgrounding ",
    "--disable-backgrounding-occluded-windows ",
    "--enable-gpu-rasterization --enable-zero-copy --ignore-gpu-blocklist"
);

/// The app context, with an opt-in diagnostic for the repaint freeze.
///
/// `LUCY_OPAQUE=1` starts Lucy as an ordinary opaque window: no `transparent`,
/// no Mica. Everything else is untouched, and without the variable nothing
/// changes — this is a switch for one measurement, not a setting.
///
/// WHY THIS IS WORTH MEASURING. The freeze ("streaming tokens land in the DOM
/// but the screen does not repaint until you move the mouse") has so far been
/// treated purely as Chromium throttling, and every flag for that is already
/// set. But this window is `transparent: true` with a Mica backdrop, which puts
/// it on a different DWM composition path — and "presents only when something
/// else forces a present" is as characteristic of that path as it is of
/// throttling. Nothing in the codebase had tested the two apart.
///
/// The answer decides something expensive: a native (non-WebView) frontend for
/// an app this size is a very large rewrite, and it should not start on an
/// untested assumption about the cause. If the freeze survives with
/// `LUCY_OPAQUE=1`, transparency is exonerated and that rewrite has its
/// justification closed. If it does not, the bug was never the WebView.
///
///     $env:LUCY_OPAQUE = "1"; npm run tauri dev
fn lucy_context() -> tauri::Context {
    let mut ctx = tauri::generate_context!();
    if std::env::var_os("LUCY_OPAQUE").is_some() {
        for w in ctx.config_mut().app.windows.iter_mut() {
            w.transparent = false;
            w.window_effects = None;
        }
        eprintln!(
            "[lucy] LUCY_OPAQUE=1 — ventana opaca, sin Mica. \
             Diagnóstico del congelamiento de repintado; quita la variable para volver al aspecto normal."
        );
    }
    ctx
}

#[cfg(all(test, windows))]
mod webview_args_tests {
    use super::WEBVIEW2_BROWSER_ARGS;

    const CONF: &str = include_str!("../tauri.conf.json");

    /// The environment variable and `additionalBrowserArgs` must carry the same
    /// flags.
    ///
    /// They are alternative routes to WebView2, not a merge — only one is
    /// applied. So a difference between them is not a style question: it is a
    /// set of flags that silently never reach the browser, while both sources
    /// still read like they are configuring it. That is exactly how
    /// `--enable-gpu-rasterization` and `--ignore-gpu-blocklist` came to be
    /// written down in one place and absent from the one that wins — for a
    /// renderer whose "paints only on input" symptom a missing GPU raster path
    /// would explain on its own.
    #[test]
    fn the_browser_args_in_code_and_config_are_identical() {
        let from_conf = CONF
            .split_once("\"additionalBrowserArgs\"")
            .expect("additionalBrowserArgs disappeared from tauri.conf.json")
            .1
            .split_once(':')
            .expect("malformed additionalBrowserArgs entry")
            .1
            .trim()
            .trim_start_matches('"')
            .split('"')
            .next()
            .unwrap_or_default();

        assert!(!from_conf.is_empty(), "read an empty arg string from the config");
        assert_eq!(
            from_conf, WEBVIEW2_BROWSER_ARGS,
            "tauri.conf.json and WEBVIEW2_BROWSER_ARGS disagree; whichever WebView2 \
             ignores is dead configuration"
        );
    }
}
