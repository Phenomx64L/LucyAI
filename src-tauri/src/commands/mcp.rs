use tauri::command;
use serde_json::{json, Value};
use tokio::process::Command;
use tokio::io::{AsyncWriteExt, AsyncBufReadExt, AsyncReadExt, BufReader};
use std::process::Stdio;
use std::collections::HashMap;

#[tauri::command]
pub async fn call_mcp_tool(server_name: String, query: String, env: Option<HashMap<String, String>>) -> Result<String, String> {
    let parts: Vec<&str> = query.split("|||").collect();
    let tool_name = parts[0].trim();
    let args_str = parts.get(1).unwrap_or(&"{}").trim();
    
    let args_json: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

    let mut cmd_parts = server_name.split_whitespace();
    let mut base_cmd = cmd_parts.next().unwrap_or("npx").to_string();
    if cfg!(windows) && base_cmd == "npx" {
        base_cmd = "npx.cmd".to_string();
    }
    let mut args_vec: Vec<String> = cmd_parts.map(|s| s.to_string()).collect();

    if (base_cmd == "npx" || base_cmd == "npx.cmd") && !args_vec.contains(&"-y".to_string()) {
        args_vec.insert(0, "-y".to_string());
    }

    let mut cmd = Command::new(&base_cmd);
    cmd.args(&args_vec)
       .stdin(Stdio::piped())
       .stdout(Stdio::piped())
       .stderr(Stdio::piped());
       
    if let Some(e_map) = env {
        cmd.envs(e_map);
    }

    let mut child = cmd.spawn().map_err(|e| format!("Fallo al iniciar el servidor MCP: {}", e))?;

    let mut stdin = child.stdin.take().ok_or("No se pudo capturar stdin del MCP")?;
    let stdout = child.stdout.take().ok_or("No se pudo capturar stdout del MCP")?;
    let mut stderr = child.stderr.take().ok_or("No se pudo capturar stderr del MCP")?;
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
    
    let mut init_str = serde_json::to_string(&init_req).unwrap();
    init_str.push('\n');
    stdin.write_all(init_str.as_bytes()).await.map_err(|e: std::io::Error| e.to_string())?;

    let mut buf = String::new();
    loop {
        buf.clear();
        let read_result = tokio::time::timeout(std::time::Duration::from_secs(15), reader.read_line(&mut buf)).await;
        match read_result {
            Ok(Ok(0)) | Err(_) => {
                let mut err_str = String::new();
                let _ = stderr.read_to_string(&mut err_str).await;
                let _ = child.kill().await;
                if err_str.trim().is_empty() {
                    return Err("Timeout o finalizacion inesperada del servidor MCP.".into());
                } else {
                    return Err(format!("Error en el servidor MCP: {}", err_str));
                }
            },
            Ok(Ok(_)) => {
                if let Ok(v) = serde_json::from_str::<Value>(&buf) {
                    if v["id"] == 1 { break; }
                }
            },
            _ => { return Err("Error al leer stdout".into()); }
        }
    }

    let init_notif = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    let mut notif_str = serde_json::to_string(&init_notif).unwrap();
    notif_str.push('\n');
    let _ = stdin.write_all(notif_str.as_bytes()).await;

    let tool_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": tool_name, "arguments": args_json }
    });

    let mut tool_str = serde_json::to_string(&tool_req).unwrap();
    tool_str.push('\n');
    stdin.write_all(tool_str.as_bytes()).await.map_err(|e: std::io::Error| e.to_string())?;

    let mut result_output = String::new();
    loop {
        buf.clear();
        if let Ok(0) = tokio::time::timeout(std::time::Duration::from_secs(45), reader.read_line(&mut buf)).await.unwrap_or(Ok(0)) {
            break; 
        }
        if let Ok(v) = serde_json::from_str::<Value>(&buf) {
            if v["id"] == 2 {
                if let Some(err) = v.get("error") {
                    let _ = child.kill().await;
                    return Err(format!("Error desde plugin MCP: {}", err));
                }
                if let Some(res) = v.get("result") {
                    if let Some(content_arr) = res.get("content").and_then(|c| c.as_array()) {
                        for c in content_arr {
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

    let _ = child.kill().await;
    if result_output.is_empty() { Ok("Sin retorno o resultado vacio del conector MCP.".into()) } else { Ok(result_output) }
}

#[tauri::command]
pub async fn discover_mcp_tools(server_name: String, env: Option<HashMap<String, String>>) -> Result<String, String> {
    let mut cmd_parts = server_name.split_whitespace();
    let mut base_cmd = cmd_parts.next().unwrap_or("npx").to_string();
    if cfg!(windows) && base_cmd == "npx" {
        base_cmd = "npx.cmd".to_string();
    }
    let mut args_vec: Vec<String> = cmd_parts.map(|s| s.to_string()).collect();

    if (base_cmd == "npx" || base_cmd == "npx.cmd") && !args_vec.contains(&"-y".to_string()) {
        args_vec.insert(0, "-y".to_string());
    }

    let mut cmd = Command::new(&base_cmd);
    cmd.args(&args_vec)
       .stdin(Stdio::piped())
       .stdout(Stdio::piped())
       .stderr(Stdio::piped());
       
    if let Some(e_map) = env {
        cmd.envs(e_map);
    }

    let mut child = cmd.spawn().map_err(|e| format!("Fallo al iniciar el servidor MCP: {}", e))?;

    let mut stdin = child.stdin.take().ok_or("No se pudo capturar stdin del MCP")?;
    let stdout = child.stdout.take().ok_or("No se pudo capturar stdout del MCP")?;
    let mut stderr = child.stderr.take().ok_or("No se pudo capturar stderr del MCP")?;
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

    let mut init_str = serde_json::to_string(&init_req).unwrap();
    init_str.push('\n');
    stdin.write_all(init_str.as_bytes()).await.map_err(|e: std::io::Error| e.to_string())?;

    let mut buf = String::new();
    loop {
        buf.clear();
        let read_result = tokio::time::timeout(std::time::Duration::from_secs(15), reader.read_line(&mut buf)).await;
        match read_result {
            Ok(Ok(0)) | Err(_) => {
                let mut err_str = String::new();
                let _ = stderr.read_to_string(&mut err_str).await;
                let _ = child.kill().await;
                if err_str.trim().is_empty() {
                    return Err("Timeout o finalizacion inesperada del servidor MCP.".into());
                } else {
                    return Err(format!("Error fatal en servidor MCP: {}", err_str));
                }
            },
            Ok(Ok(_)) => {
                if let Ok(v) = serde_json::from_str::<Value>(&buf) {
                    if v["id"] == 1 { break; }
                }
            },
            _ => { return Err("Error al leer stdout mcp".into()); }
        }
    }

    let init_notif = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    let mut notif_str = serde_json::to_string(&init_notif).unwrap();
    notif_str.push('\n');
    let _ = stdin.write_all(notif_str.as_bytes()).await;

    let list_req = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
    let mut list_str = serde_json::to_string(&list_req).unwrap();
    list_str.push('\n');
    stdin.write_all(list_str.as_bytes()).await.map_err(|e: std::io::Error| e.to_string())?;

    let mut tools_schema = String::new();
    loop {
        buf.clear();
        if let Ok(0) = tokio::time::timeout(std::time::Duration::from_secs(15), reader.read_line(&mut buf)).await.unwrap_or(Ok(0)) {
            break;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&buf) {
            if v["id"] == 2 {
                if let Some(res) = v.get("result") {
                    if let Some(tools) = res.get("tools") {
                        tools_schema = serde_json::to_string_pretty(tools).unwrap_or_default();
                    }
                }
                break;
            }
        }
    }
    let _ = child.kill().await;
    if tools_schema.is_empty() { Ok("No se encontraron herramientas o formato desconocido.".into()) } else { Ok(tools_schema) }
}
