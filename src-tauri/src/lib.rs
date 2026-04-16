// ── Lucy — Tauri entry point ───────────────────────────────────────────────────

mod state;
mod utils;
mod commands;

use commands::{ai, compliance, config, hosts, inventory, indexer, local, logs, metrics, shell, system, ui};

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
            ai::ask_lucy_stream,
            ai::list_local_models,
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
            // UI / ventana / archivos
            ui::copy_to_clipboard,
            ui::minimize_window,
            ui::maximize_window,
            ui::close_window,
            ui::open_shell_window,
            ui::pick_and_read_file,
            ui::pick_multiple_files,
            ui::pick_file_path,
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
            local::open_vscode,
            local::panic_kill_all,
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
        ])
        .run(tauri::generate_context!())
        .expect("Error al iniciar Lucy");
}
