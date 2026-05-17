// ── Lucy — Tauri entry point ───────────────────────────────────────────────────

mod state;
mod utils;
mod commands;
mod guardrails;

use commands::{ai, compliance, config, hosts, inventory, indexer, incident, local, logs, metrics, providers, rdp_agent, reflection, shell, system, ui, embeddings, memory, pdf};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::default().build())
        .setup(|app| {
            let handle = app.handle().clone();

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
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    if let Some(path_str) = token_file_path.to_str() {
                        let _ = std::process::Command::new("icacls")
                            .args([path_str, "/inheritance:r", "/grant:r", "%USERNAME%:F"])
                            .creation_flags(crate::state::CREATE_NO_WINDOW)
                            .output();
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

                    // Rate limiter: peer_addr → (window_start, request_count)
                    // 30 req / 60 s. Map auto-prunes entries older than the window.
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
                                let peer_key = addr.to_string();
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
                                let mut buf = vec![0u8; 65536];
                                let n = match tokio::time::timeout(
                                    Duration::from_secs(5),
                                    socket.read(&mut buf)
                                ).await {
                                    Ok(Ok(n)) if n > 0 => n,
                                    _ => {
                                        let _ = socket.shutdown().await;
                                        return;
                                    }
                                };

                                let req = match std::str::from_utf8(&buf[..n]) {
                                    Ok(s) => s,
                                    Err(_) => {
                                        let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                                        let _ = socket.shutdown().await;
                                        return;
                                    }
                                };

                                // ── Auth: require Authorization: Bearer <token> ──
                                // Constant-time-ish comparison via exact-substring match on word boundary.
                                // Token is 64 hex chars; collisions with random text are astronomically unlikely.
                                let auth_ok = req
                                    .lines()
                                    .take(40)  // headers section only
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
                                let body = match req.find("\r\n\r\n") {
                                    Some(idx) => req[idx + 4..].trim().to_string(),
                                    None => {
                                        let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                                        let _ = socket.shutdown().await;
                                        return;
                                    }
                                };

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
            // Failing here would leave commands unable to read/write, so log and continue.
            if let Err(e) = metrics::init(&app.handle()) {
                eprintln!("[lucy] metrics::init failed: {}", e);
            }

            // Warm up the tiktoken BPE table on a background thread so the
            // first read_file_content() call doesn't pay the ~200ms init cost.
            std::thread::spawn(|| {
                crate::commands::local::warmup_tokenizer();
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
            commands::mcp::call_mcp_tool,
            commands::mcp::discover_mcp_tools,
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
            ui::pick_pdf_path,
            ui::save_temp_pdf,
            ui::pick_directory,
            ui::save_file_dialog,
            // Sistema local
            system::get_system_health,
            system::get_system_health_json,
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
            indexer::locate_file,
            indexer::start_indexer,
            // Shell local + streaming interactivo
            shell::execute_powershell,
            shell::stream_shell_cmd,
            shell::send_shell_input,
            shell::kill_shell_session,
            // Log viewer
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
            metrics::get_token_history,
            metrics::check_permission,
            metrics::save_permission_rule,
            metrics::list_permission_rules,
            metrics::delete_permission_rule,
            metrics::save_skill,
            metrics::list_skills,
            metrics::delete_skill,
            metrics::increment_skill_usage,
            metrics::save_agent_memory,
            metrics::delete_agent_memory,
            metrics::auto_forget_run,
            metrics::consolidate_agent_memories,
            metrics::supersede_memory,
            metrics::search_agent_memories,
            metrics::get_recent_memories,
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
            metrics::get_confidence_distribution,
            // Provider Management (Multi-LLM Support)
            providers::save_credential,
            providers::get_credential,
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
            memory::memory_working_append,
            memory::memory_working_list,
            memory::memory_working_clear,
            memory::memory_stats,
            // Fork persistence — Parallel Agents (Sprint 4 Pillar 1)
            metrics::fork_save,
            metrics::fork_update,
            metrics::fork_get,
            metrics::fork_list,
            metrics::fork_clear,
            // PDF Intelligence — Sprint 4 Pillar 4
            pdf::pdf_ingest,
            pdf::pdf_list_docs,
            pdf::pdf_delete_doc,
            pdf::pdf_search,
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
        ])
        .run(tauri::generate_context!())
        .expect("Error al iniciar Lucy");
}
