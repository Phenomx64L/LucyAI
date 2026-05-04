// ── Lucy — Tauri entry point ───────────────────────────────────────────────────

mod state;
mod utils;
mod commands;

use commands::{ai, compliance, config, hosts, inventory, indexer, incident, local, logs, metrics, providers, rdp_agent, shell, system, ui, embeddings, memory, pdf};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::default().build())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use tauri::Emitter;
                use tokio::io::AsyncReadExt;
                if let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:31337").await {
                    eprintln!("[lucy] OpenClaw Gateway runnning on port 31337");
                    while let Ok((mut socket, _)) = listener.accept().await {
                        let h = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let mut buf = vec![0; 4096];
                            if let Ok(n) = socket.read(&mut buf).await {
                                if n > 0 {
                                    if let Ok(req) = String::from_utf8(buf[0..n].to_vec()) {
                                        let body = if let Some(idx) = req.find("\r\n\r\n") {
                                            req[idx+4..].trim().to_string()
                                        } else {
                                            req.trim().to_string()
                                        };
                                        if !body.is_empty() {
                                            let _ = h.emit("openclaw_webhook", body);
                                        }
                                    }
                                }
                            }
                        });
                    }
                }
            });

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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            metrics::consolidate_agent_memories,
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
            // Quality Telemetry (opus-4-7 Tier 2.A) — raw logger only
            metrics::log_task_event,
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
        ])
        .run(tauri::generate_context!())
        .expect("Error al iniciar Lucy");
}
