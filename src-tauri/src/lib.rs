// ── Lucy — Tauri entry point ───────────────────────────────────────────────────

mod state;
mod utils;
mod commands;

use commands::{ai, compliance, config, hosts, inventory, local, logs, shell, system, ui};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            // AI
            ai::ask_lucy,
            ai::ask_lucy_stream,
            ai::fetch_url_content,
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
            local::open_vscode,
            local::panic_kill_all,
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
        ])
        .run(tauri::generate_context!())
        .expect("Error al iniciar Lucy");
}
