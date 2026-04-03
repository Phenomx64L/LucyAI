// ── AI — Integración con Gemini (ask_lucy + ask_lucy_stream) ────────────────────

use keyring::Entry;
use serde_json::json;
use tauri::Emitter;
use futures_util::StreamExt;
use crate::state::{HTTP_CLIENT, ALLOWED_MODELS};

// ── URL CONTENT FETCHER ───────────────────────────────────────────────────────

/// Strips HTML tags from a string, returning readable plain text.
fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag   = false;
    let mut in_script = false;
    let mut in_style  = false;

    for (i, c) in html.char_indices() {
        if c == '<' {
            in_tag = true;
            let remain = &html[i..];
            if remain.get(..7).map_or(false, |s| s.eq_ignore_ascii_case("<script")) {
                in_script = true;
            } else if remain.get(..8).map_or(false, |s| s.eq_ignore_ascii_case("</script")) {
                in_script = false;
            } else if remain.get(..6).map_or(false, |s| s.eq_ignore_ascii_case("<style")) {
                in_style = true;
            } else if remain.get(..7).map_or(false, |s| s.eq_ignore_ascii_case("</style")) {
                in_style = false;
            }
        } else if c == '>' {
            in_tag = false;
            if !in_script && !in_style { out.push(' '); }
        } else if !in_tag && !in_script && !in_style {
            out.push(c);
        }
    }

    // Decode common HTML entities & collapse whitespace
    out.replace("&amp;",  "&")
       .replace("&lt;",   "<")
       .replace("&gt;",   ">")
       .replace("&quot;", "\"")
       .replace("&#39;",  "'")
       .replace("&nbsp;", " ")
}

/// Fetches a URL and returns up to 12 000 chars of readable plain text.
/// Used by the frontend to attach web documentation to the AI context.
#[tauri::command]
pub async fn fetch_url_content(url: String) -> Result<String, String> {
    // Basic URL validation
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("URL debe comenzar con http:// o https://".to_string());
    }
    let res = HTTP_CLIENT
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Lucy/1.0")
        .header("Accept", "text/html,application/xhtml+xml,text/plain;q=0.9")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Error de red al obtener URL: {}", e))?;

    let status = res.status().as_u16();
    if status >= 400 {
        return Err(format!("La URL devolvió HTTP {}", status));
    }

    let body = res.text().await
        .map_err(|e| format!("Error al leer cuerpo: {}", e))?;

    let plain = strip_html_tags(&body);
    // Collapse whitespace
    let clean: String = plain.split_whitespace().collect::<Vec<&str>>().join(" ");
    let truncated = if clean.len() > 6_000 { &clean[..6_000] } else { &clean };

    Ok(truncated.to_string())
}

// ── PROMPT HELPERS ────────────────────────────────────────────────────────────

fn lang_instruction(lang: &str) -> &'static str {
    match lang {
        l if l.starts_with("es") => "LANGUAGE RULE: Always respond in Spanish.",
        l if l.starts_with("en") => "LANGUAGE RULE: Always respond in English.",
        l if l.starts_with("pt") => "LANGUAGE RULE: Always respond in Portuguese.",
        l if l.starts_with("fr") => "LANGUAGE RULE: Always respond in French.",
        l if l.starts_with("de") => "LANGUAGE RULE: Always respond in German.",
        l if l.starts_with("it") => "LANGUAGE RULE: Always respond in Italian.",
        l if l.starts_with("ja") => "LANGUAGE RULE: Always respond in Japanese.",
        l if l.starts_with("zh") => "LANGUAGE RULE: Always respond in Chinese.",
        _                        => "LANGUAGE RULE: Detect the user language and respond in the same language.",
    }
}

fn build_hosts_context(hosts_json: Option<&str>) -> String {
    let Some(hj) = hosts_json else { return String::new(); };
    let Ok(hosts) = serde_json::from_str::<serde_json::Value>(hj) else { return String::new(); };
    let Some(arr) = hosts.as_array() else { return String::new(); };
    if arr.is_empty() { return String::new(); }

    let mut lines = String::from("\n--- CONFIGURED REMOTE HOSTS (use these when user mentions a host by name) ---\n");
    for h in arr {
        let name    = h["name"].as_str().unwrap_or("?");
        let htype   = h["type"].as_str().unwrap_or("windows");
        let host_ip = h["host"].as_str().unwrap_or("?");
        let uname   = h["username"].as_str().unwrap_or("?");
        let port    = h["port"].as_u64().unwrap_or(if htype == "linux" { 22 } else { 5985 });
        let proto   = if htype == "linux" { "SSH" } else { "WinRM" };
        lines.push_str(&format!("- \"{name}\": type={htype} ({proto}), ip={host_ip}, user={uname}, port={port}\n"));
    }
    lines.push_str("For Windows remote: use Invoke-Command -ComputerName <ip> with PSCredential.\n");
    lines.push_str("For Linux remote: use ssh <user>@<ip> -p <port> '<command>'.\n");
    lines.push_str("--- END HOSTS ---\n");
    lines
}

fn load_local_runbooks(dir_path: Option<&str>) -> String {
    let Some(path) = dir_path else { return String::new(); };
    if path.trim().is_empty() { return String::new(); }
    
    let path_obj = std::path::Path::new(path);
    if !path_obj.exists() || !path_obj.is_dir() {
        return String::new();
    }

    let mut content = String::from("\n<COMPANY_RUNBOOKS>\n");
    let mut found = false;

    if let Ok(entries) = std::fs::read_dir(path_obj) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(ext) = p.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if ext_str == "md" || ext_str == "txt" {
                        if let Ok(text) = std::fs::read_to_string(&p) {
                            found = true;
                            content.push_str(&format!("--- FILE: {} ---\n", p.file_name().unwrap_or_default().to_string_lossy()));
                            let trunc = if text.len() > 100_000 { &text[..100_000] } else { &text };
                            content.push_str(trunc);
                            content.push_str("\n\n");
                        }
                    }
                }
            }
        }
    }
    
    if !found { return String::new(); }
    
    content.push_str("</COMPANY_RUNBOOKS>\n");
    content.push_str("COMPANY KNOWLEDGE RULE: Always consult <COMPANY_RUNBOOKS> first when facing an error or when the user asks how to do something related to the company infrastructure. If a runbook matches the scenario, EXECUTE EXACTLY the steps outlined in the file.\n");
    content
}

fn build_system_prompt(
    lang: &str,
    context: &str,
    hosts_context: &str,
    user_name: &str,
    prompt: &str,
    working_dir: &str,
    runbooks_dir: Option<&str>,
) -> String {
    let cwd = working_dir;
    let local_runbooks = load_local_runbooks(runbooks_dir);
    format!(
        "You are Lucy, an expert Windows SysAdmin AI assistant with autonomous code analysis and modification capabilities.\n\
        {lang}\n\
        WORKING DIRECTORY: {cwd}\n\
        When the user references project files without a full path, resolve them relative to this directory.\n\
        RULE 0 — INTENT DETECTION (apply BEFORE anything else):\n\
        STEP 1: Classify the message into one of these categories:\n\
        A) CONVERSATIONAL — questions about you, your logic, opinions, advice, explanations, 'how', 'why', 'what is', 'can you', 'cómo', 'qué', 'por qué', 'puedes', 'explícame', 'ves algún problema', 'qué opinas', 'cómo podríamos' → respond with MARKDOWN TEXT ONLY. NO <EXECUTE>, NO <TOOL>.\n\
        B) FILE OPERATION — the user mentions a file name, path, or asks to analyze/review/fix code → use RULE 17/18 <TOOL> file operations. If only a filename is given (e.g. 'local.rs', '+page.svelte'), resolve it within the WORKING DIRECTORY using <TOOL>searchfiles</TOOL> or <TOOL>listdir</TOOL> first.\n\
        C) SYSTEM ACTION — the user asks to DO something on the system: install, restart, check status, clean, generate, create, execute, list processes/services, verify connectivity → use <EXECUTE>, <EXECUTE_REMOTE> or appropriate <TOOL>. NEVER print commands in plain text if they belong to this category.\n\
        STEP 2: If the message does NOT contain a specific file path or system target, AND is NOT about code, it is category A (CONVERSATIONAL). Respond in Markdown only.\n\
        STEP 3: 'verifica' + file path = category B. 'verifica' without file path = category A. 'ves algún problema' about YOUR logic = category A. 'busca todas las funciones' = category B.\n\
        RULE 1: To execute a local action or tool, YOU MUST ALWAYS reason first. Provide your reasoning wrapped in <THOUGHT>...</THOUGHT> tags. AFTER your reasoning, YOU ABSOLUTELY MUST provide the command wrapped in <EXECUTE>...</EXECUTE>, <EXECUTE_REMOTE target=\"...\">...</EXECUTE_REMOTE> or <TOOL>...</TOOL> tags. NEVER print bare commands in plain text. NEVER output <EXECUTE> or <TOOL> without a preceding <THOUGHT> block analyzing the risks.\n\
        RULE 2: Commands requiring admin rights must use EXACTLY: <EXECUTE>Start-Process powershell -Verb RunAs -ArgumentList '-NoProfile -ExecutionPolicy Bypass -Command \"COMMAND\"'</EXECUTE>\n\
        RULE 3: NEVER print raw HTML. Use Markdown for formatting responses.\n\
        RULE 4: ONLY if a command you already executed in THIS conversation returned an error, analyze the error and ask how to proceed WITHOUT generating <EXECUTE>. Do NOT apply this rule to new independent instructions.\n\
        RULE 5: Silently correct phonetically mistranscribed words.\n\
        RULE 6: If the user teaches you a command, respond ONLY with <LEARN>key1,key2|powershell_command|response</LEARN>.\n\
        RULE 7: You can create PDFs using Edge Headless.\n\
        RULE 8: For Linux use native ssh. For Windows Server use Invoke-Command -ComputerName. EXCEPTION: if the context says \"ACTIVE REMOTE SHELL\", the session is already established — generate RAW commands only, NO Invoke-Command, NO -ComputerName, NO -Credential wrappers.\n\
        RULE 9: <TOOL>sysinfo</TOOL> is ONLY for LOCAL machine hardware queries: CPU usage, RAM, disk, system health, uptime. NEVER use sysinfo for: code analysis, file review, bug detection, logic verification, architecture analysis, or ANY question about code/files/projects. For REMOTE hosts, use Invoke-Command or SSH with the host details. EXCEPTION: if context says \"ACTIVE REMOTE SHELL\", generate raw commands — the WinRM/SSH tunnel is already open.\n\
        RULE 10: To keep the machine awake use PowerToys Awake.\n\
        RULE 11: For cleaning system logs, ALWAYS use RULE 2 elevation.\n\
        RULE 12: If asked about quick actions or the sidebar, tell them to use the + button in the side panel.\n\
        RULE 13: Each user message is an INDEPENDENT instruction unless explicitly referencing a previous result. Do NOT mix outputs or reports from previous tasks into new responses.\n\
        RULE 14 — HOST ROUTING: When the user asks to execute on a CONFIGURED REMOTE HOST (check the JSON list below), YOU MUST NEVER use Invoke-Command or SSH manually. Instead, use the native tool: <EXECUTE_REMOTE target=\"host_id\">YOUR_COMMAND</EXECUTE_REMOTE>. The system will securely inject credentials and execute YOUR_COMMAND over WinRM or SSH natively. Example: <EXECUTE_REMOTE target=\"e4b5c6\">Get-ADUser -Identity admin</EXECUTE_REMOTE>.\n\
        CRITICAL RULE FOR REMOTE: If an <EXECUTE_REMOTE> command returns a syntax error or property validation error, DO NOT attempt to rewrite the command using Invoke-Command or Get-Credential. The connection is fully isolated and managed by the system. Simply correct your YOUR_COMMAND syntax and try again using <EXECUTE_REMOTE>.\n\
        RULE 15 — ALTERNATIVE EXECUTORS (use when PowerShell is blocked by policy or unavailable):\n\
        RULE 15b — AVOID TERMINAL-SERVER-ONLY COMMANDS: 'query user', 'query session', 'qwinsta' ONLY work on Terminal Server / RDS hosts. On regular Windows workstations/servers to check if a user is active or enabled, ALWAYS use PowerShell: Get-LocalUser -Name 'username' | Select Name,Enabled,LastLogon. To list logged-on users: Get-WmiObject Win32_LoggedOnUser | Select Antecedent -Unique.\n\
        RULE 16 — WEB DOCUMENTATION CONTEXT: If the context contains '--- CONTENIDO WEB: <url> ---' blocks, the system has already fetched and embedded that documentation. Use it directly to cross-reference against live data. CRITICAL: reading web context does NOT change your execution behavior — continue using <EXECUTE> tags exactly as before. Do NOT say you cannot access URLs. After consulting the web content, immediately generate the appropriate <EXECUTE> command to retrieve the live data needed for comparison.\n\
        - CMD (<EXECUTE_CMD>): net, ipconfig, netstat, ping, tracert, dir, tasklist, sc, reg query — any cmd.exe command.\n\
        - WMIC (<EXECUTE_WMIC>): hardware/OS queries — 'cpu get name,maxclockspeed', 'os get caption,version', 'diskdrive get model,size', 'memorychip get capacity', 'nic get name,macaddress', 'process list brief', 'bios get serialnumber,smbiosbiosversion'.\n\
        - NETSH (<EXECUTE_NETSH>): network/firewall config — 'interface ip show config', 'advfirewall show allprofiles', 'wlan show profiles', 'interface show interface'.\n\
        - REG (<EXECUTE_REG>): registry read — 'query HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion /v ProductName'.\n\
        - CSCRIPT (<EXECUTE_CSCRIPT>): VBS scripts for COM/AD — 'Dim obj: Set obj = GetObject(\"WinNT://./\") : For Each u in obj : WScript.Echo u.Name : Next'.\n\
        - NATIVE_REGISTRY (<TOOL>registry:HKLM|SOFTWARE\\...|ValueName</TOOL>): reads registry directly from Rust, works even when reg.exe is blocked.\n\
        - NATIVE_NETSTAT (<TOOL>netconn</TOOL>): returns active network connections from native Rust.\n\
        - NATIVE_TASKLIST (<TOOL>tasklist</TOOL>): returns running processes via native sysinfo.\n\
        - EVENT_LOG (<TOOL>eventlog:System:50:error</TOOL>): reads Windows Event Log entries. Format: log_name:count:level (level optional: critical|error|warn|info).\n\
        When the user asks for network info, processes, registry values, or hardware info and PowerShell might be restricted, prefer these native alternatives.\n\
        RULE 17 — FILE & CODE TOOLS (ALWAYS prefer over PowerShell — you are an AI agent with tool chaining):\n\
        These tools execute natively in Rust. The system will automatically feed results back to you so you can chain multiple operations.\n\
        Available tools:\n\
        - READ FILE: <TOOL>readfile:/path/to/file</TOOL> — reads file content (max 512KB). For large files use readlines.\n\
        - READ LINES: <TOOL>readlines:/path/to/file:START:COUNT</TOOL> — reads specific lines (1-based). Example: <TOOL>readlines:C:\\config.txt:1:50</TOOL>\n\
        - WRITE FILE: <TOOL>writefile:/path/to/file</TOOL> followed by <FILECONTENT>full content</FILECONTENT> — overwrites entire file.\n\
        - EDIT FILE: <TOOL>editfile:/path/to/file</TOOL> followed by <OLDSTRING>exact text to find</OLDSTRING><NEWSTRING>replacement text</NEWSTRING> — surgical find-and-replace WITHOUT rewriting the whole file. PREFERRED for modifications.\n\
        - LIST DIR: <TOOL>listdir:/path/to/dir</TOOL> — lists directory contents with sizes and dates.\n\
        - SEARCH FILES: <TOOL>searchfiles:/directory|search pattern</TOOL> — searches text across all files in a directory (like grep). Returns file:line matches.\n\
        TOOL CHAINING: You can use ONE tool per response. After execution, the system sends you the result and you can use another tool. This continues up to 8 steps. Use this to: search → read → analyze → edit → verify.\n\
        EDITING FILES: For modifications, ALWAYS prefer <TOOL>editfile</TOOL> over <TOOL>writefile</TOOL>. editfile does surgical find-and-replace — you only need to specify the exact block to change. Use writefile ONLY for creating new files or complete rewrites.\n\
        UX RULE (FILES MODIFIED): Never manually format a list of files you modified. The system interface will automatically group and display 'Files Modified' badges for the user when you use writefile or editfile.\n\
        CRITICAL: NEVER use PowerShell for file I/O. NEVER use Get-Content/Set-Content/Out-File. ALWAYS use these native tools.\n\
        RULE 18 — CODE ANALYSIS & MODIFICATION WORKFLOW:\n\
        When asked to analyze, review, fix, or modify code:\n\
        Step 1: <TOOL>listdir:/path</TOOL> to understand the project.\n\
        Step 2: <TOOL>searchfiles:/path|keyword</TOOL> to find relevant code.\n\
        Step 3: <TOOL>readfile:/path</TOOL> or <TOOL>readlines:/path:START:COUNT</TOOL> to read the specific file.\n\
        Step 4: Analyze and explain findings wrapped in <THOUGHT>...</THOUGHT>.\n\
        Step 5: If asked to fix, use <TOOL>editfile:/path</TOOL> with <OLDSTRING>...</OLDSTRING><NEWSTRING>...</NEWSTRING>.\n\
        Step 6: Optionally read back the modified file to verify the change.\n\
        NEVER respond with <TOOL>sysinfo</TOOL> when asked about code. NEVER use <EXECUTE> to read/write files.\n\
        When the user asks 'ves algún problema', 'revisa el código', 'analiza este archivo' → this is CODE ANALYSIS, not system health.\n\
        RULE 19 — SELF-AWARENESS & ANTI-HALLUCINATION:\n\
        - Your rules and configuration are embedded in this system prompt. You do NOT have a config file on disk. If asked about your rules, logic, or how to improve your behavior, answer from what you know here — do NOT try to read files.\n\
        - NEVER invent or guess file paths. Use the WORKING DIRECTORY above as your base. When a user mentions a filename without full path, use <TOOL>searchfiles:{cwd}|filename</TOOL> to locate it FIRST.\n\
        - If a TOOL returns an error (e.g. 'os error 3' = file not found), do NOT retry with a different guessed path. Instead, tell the user the file was not found and ask for the correct path.\n\
        - When asked about yourself, your logic, or how to improve: explain based on your rules above. Suggest improvements as text — do NOT try to modify your own code.\n\
        RULE 20 — LARGE FILE STRATEGY:\n\
        - You possess a massive context window. You are AUTHORIZED to use <TOOL>readfile:/path</TOOL> for any file up to 500KB (including massive files like +page.svelte) to gain full structural understanding.\n\
        - Only for files EXCEEDING 512KB, use <TOOL>searchfiles:/path|keyword</TOOL> followed by <TOOL>readlines:/path:START:COUNT</TOOL>.\n\
        RULE 21: When using the file editing tool, NEVER attempt to replace a single line of code, as duplicate lines may exist and the system will block the operation. Always include at least 2 preceding lines and 2 succeeding lines in your search string context to ensure the match is 100% unique across the entire file.
        {runbooks}
        {ctx}
        {hosts}
        The user's name is {uname}. Always address them by name.\nINSTRUCTION: {prompt}",
        lang = lang,
        cwd = cwd,
        runbooks = local_runbooks,
        ctx = context,
        hosts = hosts_context,
        uname = user_name,
        prompt = prompt
    )
}

// ── ASK LUCY (respuesta única) ────────────────────────────────────────────────

#[tauri::command]
pub async fn ask_lucy(
    prompt: String,
    context: Option<String>,
    user_name: String,
    model: String,
    images: Option<Vec<serde_json::Value>>,
    lang: Option<String>,
    hosts_json: Option<String>,
    runbooks_dir: Option<String>,
) -> Result<String, String> {
    let is_allowed = ALLOWED_MODELS.contains(&model.as_str()) || model.starts_with("local-");
    if !is_allowed {
        return Err(format!("Modelo '{}' no permitido. Selecciona un modelo válido desde el selector.", model));
    }

    let provider = if model.starts_with("gpt-") { "openai" } 
                   else if model.starts_with("claude-") { "anthropic" } 
                   else if model.starts_with("local-") { "local" }
                   else { "gemini" };

    let entry = Entry::new("LucySysAdmin", &format!("{}_api_key", provider)).map_err(|e| e.to_string())?;
    let api_key = entry.get_password().map_err(|_| format!("API Key para {} no configurada.", provider))?;

    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "C:\\".to_string());
    let user_lang = lang.as_deref().unwrap_or("es-MX");
    let hosts_context = build_hosts_context(hosts_json.as_deref());
    let final_prompt = build_system_prompt(
        lang_instruction(user_lang),
        context.as_deref().unwrap_or_default(),
        &hosts_context,
        &user_name,
        &prompt,
        &cwd,
        runbooks_dir.as_deref(),
    );

    let req = match provider {
        "openai" => {
            let payload = json!({ "model": model, "messages": [{"role": "user", "content": final_prompt}] });
            HTTP_CLIENT.post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&payload)
        },
        "local" => {
            // api_key contiene la URL del endpoint
            let payload = json!({ "model": model, "messages": [{"role": "user", "content": final_prompt}] });
            HTTP_CLIENT.post(&api_key).json(&payload)
        },
        "anthropic" => {
            let payload = json!({ "model": model, "max_tokens": 4096, "messages": [{"role": "user", "content": final_prompt}] });
            HTTP_CLIENT.post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&payload)
        },
        _ => {
            let mut parts = vec![json!({"text": final_prompt})];
            if let Some(imgs) = images {
                for img in imgs { parts.push(json!({ "inlineData": { "mimeType": img["mimeType"], "data": img["data"] } })); }
            }
            let payload = json!({ "contents": [{ "parts": parts }] });
            let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}", model, api_key);
            HTTP_CLIENT.post(&url).json(&payload)
        }
    };

    let res = req.send().await.map_err(|e| format!("Error de red: {}", e))?;
    
    // Check for HTTP errors before parsing
    if !res.status().is_success() {
        let status = res.status();
        let err_text = res.text().await.unwrap_or_default();
        return Err(format!("Error API HTTP {}: {}", status, err_text));
    }
    
    let body_text = res.text().await.map_err(|e| format!("Error al leer body: {}", e))?;
    let v: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| format!("Error parseando JSON: {}", e))?;

    let text_result = match provider {
        "openai" | "local" => v["choices"].get(0).and_then(|c| c["message"]["content"].as_str()),
        "anthropic" => v["content"].get(0).and_then(|c| c["text"].as_str()),
        _ => v["candidates"].get(0).and_then(|c| c["content"]["parts"][0]["text"].as_str())
    };

    if let Some(t) = text_result {
        Ok(t.to_string())
    } else {
        Err(format!("Respuesta API ({}): {}", provider, body_text))
    }
}

// ── ASK LUCY STREAMING (SSE) ──────────────────────────────────────────────────

/// Igual que ask_lucy pero emite chunks vía eventos Tauri para respuesta progresiva.
/// El frontend escucha "lucy-chunk-{request_id}".
/// Retorna el texto completo como resultado del invoke para mayor fiabilidad.
#[tauri::command]
pub async fn ask_lucy_stream(
    window: tauri::Window,
    request_id: String,
    prompt: String,
    context: Option<String>,
    user_name: String,
    model: String,
    images: Option<Vec<serde_json::Value>>,
    lang: Option<String>,
    hosts_json: Option<String>,
    runbooks_dir: Option<String>,
) -> Result<String, String> {
    let is_allowed = ALLOWED_MODELS.contains(&model.as_str()) || model.starts_with("local-");
    if !is_allowed {
        return Err(format!("Modelo '{}' no permitido.", model));
    }

    let provider = if model.starts_with("gpt-") { "openai" } 
                   else if model.starts_with("claude-") { "anthropic" } 
                   else if model.starts_with("local-") { "local" }
                   else { "gemini" };

    let entry = Entry::new("LucySysAdmin", &format!("{}_api_key", provider)).map_err(|e| e.to_string())?;
    let api_key = entry.get_password().map_err(|_| format!("API Key para {} no configurada.", provider))?;

    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "C:\\".to_string());
    let user_lang = lang.as_deref().unwrap_or("es-MX");
    let hosts_context = build_hosts_context(hosts_json.as_deref());
    let final_prompt = build_system_prompt(
        lang_instruction(user_lang),
        context.as_deref().unwrap_or_default(),
        &hosts_context,
        &user_name,
        &prompt,
        &cwd,
        runbooks_dir.as_deref(),
    );

    let req = match provider {
        "openai" => {
            let payload = json!({ "model": model, "messages": [{"role": "user", "content": final_prompt}], "stream": true });
            HTTP_CLIENT.post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&payload)
        },
        "local" => {
            let payload = json!({ "model": model, "messages": [{"role": "user", "content": final_prompt}], "stream": true });
            HTTP_CLIENT.post(&api_key).json(&payload)
        },
        "anthropic" => {
            let payload = json!({ "model": model, "max_tokens": 4096, "messages": [{"role": "user", "content": final_prompt}], "stream": true });
            HTTP_CLIENT.post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&payload)
        },
        _ => {
            let mut parts = vec![json!({"text": final_prompt})];
            if let Some(imgs) = images {
                for img in imgs { parts.push(json!({ "inlineData": { "mimeType": img["mimeType"], "data": img["data"] } })); }
            }
            let payload = json!({ "contents": [{ "parts": parts }] });
            let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}", model, api_key);
            HTTP_CLIENT.post(&url).json(&payload)
        }
    };

    let res = req.send().await.map_err(|e| format!("Error de red: {}", e))?;

    if !res.status().is_success() {
        let status = res.status();
        let err_text = res.text().await.unwrap_or_default();
        return Err(format!("Error API HTTP {}: {}", status, err_text));
    }

    let mut byte_stream = res.bytes_stream();
    let mut full_text = String::new();
    let mut line_buffer = String::new();
    let chunk_event = format!("lucy-chunk-{}", request_id);

    while let Some(chunk) = byte_stream.next().await {
        let bytes = chunk.map_err(|e| format!("Error de stream: {}", e))?;
        line_buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(newline_pos) = line_buffer.find('\n') {
            let line = line_buffer[..newline_pos].trim().to_string();
            line_buffer = line_buffer[newline_pos + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" { continue; }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                    let text_chunk = match provider {
                        "openai" | "local" => v["choices"].get(0).and_then(|c| c["delta"]["content"].as_str()),
                        "anthropic" => v["delta"]["text"].as_str(), // handles type: text_delta
                        _ => v["candidates"].get(0).and_then(|c| c["content"]["parts"][0]["text"].as_str())
                    };
                    
                    if let Some(t) = text_chunk {
                        full_text.push_str(t);
                        let _ = window.emit(&chunk_event, t);
                    }
                }
            }
        }
    }

    Ok(full_text)
}
