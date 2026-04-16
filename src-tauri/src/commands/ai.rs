// ── AI — Integración con Gemini (ask_lucy + ask_lucy_stream) ────────────────────

use keyring::Entry;
use serde_json::json;
use tauri::Emitter;
use futures_util::StreamExt;
use crate::state::{HTTP_CLIENT, ALLOWED_MODELS};
use crate::commands::metrics::log_usage_internal;

// ── LIST LOCAL MODELS (Ollama /api/tags) ─────────────────────────────────────
/// Pregunta a Ollama (o endpoint compatible) qué modelos hay instalados.
/// Lee la URL del chat endpoint guardada en keyring (`local_api_key`),
/// deriva la base y consulta `{base}/api/tags`.
#[tauri::command]
pub async fn list_local_models() -> Result<Vec<String>, String> {
    let entry = Entry::new("LucySysAdmin", "local_api_key").map_err(|e| e.to_string())?;
    let stored = entry.get_password().map_err(|_| "Endpoint local no configurado".to_string())?;

    // Derivar base URL: quitar /v1/chat/completions, /v1, /api/chat, etc.
    let base = stored
        .trim_end_matches('/')
        .trim_end_matches("/v1/chat/completions")
        .trim_end_matches("/api/chat")
        .trim_end_matches("/v1")
        .trim_end_matches("/api")
        .trim_end_matches('/')
        .to_string();

    let tags_url = format!("{}/api/tags", base);
    let resp = HTTP_CLIENT
        .get(&tags_url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("No se pudo conectar a Ollama en {}: {}", tags_url, e))?;

    if !resp.status().is_success() {
        return Err(format!("Ollama respondió {}: verifica que esté corriendo", resp.status()));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| format!("JSON inválido: {}", e))?;
    let models = json["models"].as_array().ok_or("Respuesta sin campo 'models'")?;

    let names: Vec<String> = models
        .iter()
        .filter_map(|m| m["name"].as_str().map(String::from))
        .collect();

    Ok(names)
}

// ── TOKEN EXTRACTION from API responses ──────────────────────────────────────

/// Extract input and output tokens from Anthropic API response
fn extract_tokens_anthropic(json: &serde_json::Value) -> Option<(u32, u32)> {
    let usage = &json["usage"];
    let input = usage["input_tokens"].as_u64()? as u32;
    let output = usage["output_tokens"].as_u64()? as u32;
    Some((input, output))
}

/// Extract input and output tokens from OpenAI API response
fn extract_tokens_openai(json: &serde_json::Value) -> Option<(u32, u32)> {
    let usage = &json["usage"];
    let input = usage["prompt_tokens"].as_u64()? as u32;
    let output = usage["completion_tokens"].as_u64()? as u32;
    Some((input, output))
}

/// Extract input and output tokens from Google Gemini API response
fn extract_tokens_gemini(json: &serde_json::Value) -> Option<(u32, u32)> {
    let usage = &json["usageMetadata"];
    let input = usage["promptTokenCount"].as_u64()? as u32;
    let output = usage["candidatesTokenCount"].as_u64()? as u32;
    Some((input, output))
}

// ── MAX TOKENS por modelo ────────────────────────────────────────────────────
/// Devuelve el max_tokens óptimo para cada modelo de Anthropic.
/// Si se pasa un override > 0, lo usa directamente (escalación por truncamiento).
fn get_max_tokens(model: &str, override_val: Option<u32>) -> u32 {
    if let Some(v) = override_val {
        if v > 0 { return v; }
    }
    if model.contains("sonnet-4") || model.contains("opus-4") {
        16384
    } else if model.contains("3-7") || model.contains("3.7") {
        16384
    } else if model.contains("3-5") || model.contains("3.5") {
        8192
    } else {
        8192 // default seguro
    }
}

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
#[tauri::command]
pub async fn fetch_url_content(url: String) -> Result<String, String> {
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

#[tauri::command]
pub async fn search_runbooks(dir_path: Option<String>, query: String) -> Result<String, String> {
    use simsearch::SimSearch;

    let Some(path) = dir_path else { return Err("No runbooks directory configured.".to_string()) };
    let path_obj = std::path::Path::new(&path);
    if !path_obj.exists() || !path_obj.is_dir() {
        return Err("Runbooks directory not found.".to_string());
    }

    let mut engine: SimSearch<String> = SimSearch::new();
    let mut files_metadata = std::collections::HashMap::new();

    if let Ok(entries) = std::fs::read_dir(path_obj) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(ext) = p.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if ext_str == "md" || ext_str == "txt" {
                        if let Ok(text) = std::fs::read_to_string(&p) {
                            let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                            engine.insert(name.clone(), &text);
                            files_metadata.insert(name, text);
                        }
                    }
                }
            }
        }
    }

    let results = engine.search(&query);
    if results.is_empty() {
        return Ok(format!("[Sin resultados SEMÁNTICOS estrictos para '{}'", query));
    }

    let mut out = String::new();
    for r in results.into_iter().take(2) { // Top 2 más relevantes
        if let Some(content) = files_metadata.get(&r) {
            let trunc = if content.len() > 12000 { &content[..12000] } else { content };
            out.push_str(&format!("--- RUNBOOK FILE: {} ---\n{}\n\n", r, trunc));
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn change_agent_dir(path: String) -> Result<String, String> {
    let p = std::path::Path::new(&path);
    if p.exists() && p.is_dir() {
        if let Ok(mut cwd) = crate::state::GLOBAL_CWD.write() {
            *cwd = path.clone();
            std::env::set_current_dir(&path).ok();
            Ok(format!("Directorio de trabajo cambiado a: {}", path))
        } else {
            Err("Fallo al bloquear GLOBAL_CWD".into())
        }
    } else {
        Err(format!("Directorio no encontrado o no válido: {}", path))
    }
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
    let runbooks_info = if let Some(rf) = runbooks_dir {
        format!("Runbooks Directory Configured: {rf}\nUse <TOOL>search_runbooks:YOUR_QUERY</TOOL> to fetch specific runbook files using Semantic Search TF-IDF. Strongly consider doing this BEFORE executing commands if the user is asking context-heavy infrastructure questions.")
    } else { "".to_string() };

    let user_profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".to_string());
    format!(
        "You are Lucy, an expert Windows SysAdmin AI assistant with autonomous code analysis and modification capabilities.\n\
        {lang}\n\
        CURRENT USER: {user_name} (Profile: {user_profile})\n\
        WORKING DIRECTORY: {cwd}\n\
        {rb}\n\
        When the user references project files without a full path, resolve them relative to this directory.\n\
        RULE 0 — INTENT DETECTION (apply BEFORE anything else):\n\
        STEP 1: Classify the message into one of these categories:\n\
          A) CONVERSATIONAL — general questions -> respond with normal text.\n\
          B) FILE OPERATION — user asks to create, edit, or read a file -> You MUST use <TOOL> operations like <TOOL>writefile:/path</TOOL> or <EXECUTE> directly. DO NOT just show the code to the user, ACTUALLY create the file for them autonomously.\n\
          C) SYSTEM ACTION — user asks to execute on the system -> Use <EXECUTE> tags autonomously.\n\
          D) CODE GENERATION — user EXPLICITLY asks to just SEE code without running it -> Provide standard markdown code blocks without executing.\n\
          RULE 1: For trivial tasks (like simple file creation, basic commands), COMPLETELY BYPASS <THOUGHT> tags and output the <TOOL>...</TOOL> or <EXECUTE>...</EXECUTE> tags NATIVELY to save tokens and answer instantaneously. MAKE SURE TO ACTUALLY USE THE XML TAGS (do not write raw commands without wrappers). For complex architecture tasks, you may use <THOUGHT> tags first.\n\
        RULE 2: If a command requires admin elevation, DO NOT auto-generate Start-Process RunAs. Instead: explain what requires elevation, show the command the user should run, and ask 'Do you want me to execute this with admin privileges?'. Only generate the RunAs <EXECUTE> after user explicitly confirms.\n\
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
        RULE 13: Each user message is INDEPENDENT unless explicitly referencing a previous result. Do NOT mix outputs or reports from previous tasks into new responses.\n\
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
        ⚠️ CRITICAL SYNTAX RULE: You MUST ALWAYS wrap tool invocations inside <TOOL>...</TOOL> tags VERBATIM. NEVER write a tool name as plain text (e.g. NEVER write 'system_diff:tasks' alone — it MUST be '<TOOL>system_diff:tasks</TOOL>'). The system parser only recognizes tools wrapped in literal <TOOL> tags. Plain text tool names will be IGNORED and the user will see nothing happen.\n\
        Available tools:\n\
        - READ FILE: <TOOL>readfile:/path/to/file</TOOL> — reads file content (max 512KB). For large files use readlines.\n\
        - READ LINES: <TOOL>readlines:/path/to/file:START:COUNT</TOOL> — reads specific lines (1-based). Example: <TOOL>readlines:C:\\config.txt:1:50</TOOL>\n\
        - WRITE FILE: <TOOL>writefile:/path/to/file</TOOL> followed by <FILECONTENT>full content</FILECONTENT> — overwrites entire file. ⚠️ TEXT ONLY: writefile writes UTF-8 text. It CANNOT create binary files (.ico, .png, .jpg, .exe, .dll, .wasm, .onnx, etc.). For binary assets use <EXECUTE> with PowerShell: `Invoke-WebRequest -Uri URL -OutFile path` to download, or `cargo tauri icon input.png` to generate app icons, or `[System.IO.File]::WriteAllBytes(path, bytes)` for raw bytes.\n\
        - EDIT FILE: <TOOL>editfile:/path/to/file|||exact text to find|||replacement text</TOOL> — surgical find-and-replace WITHOUT rewriting the whole file. PREFERRED for modifications.\n\
        - LIST DIR: <TOOL>listdir:/path/to/dir</TOOL> — lists directory contents with sizes and dates.\n\
        - LOCATE FILE: <TOOL>locate_file:name</TOOL> — Searches the entire local drive for a filename instantaneously (O(log n)) using the SQLite indexer.\n\
        - START INDEXER: <TOOL>start_indexer:C:\\</TOOL> — Rebuilds the global SQLite file index for a given path. Use this if locate_file cannot find something you suspect exists.\n\
        - CHANGE DIR: <TOOL>cd:/nueva/ruta</TOOL> — Changes your logical working directory. Use this when the user asks you to switch paths or create a project in a specific directory. ⚠️ CRITICAL: NEVER use `<EXECUTE>cd path</EXECUTE>` — that spawns a subprocess that exits immediately and changes NOTHING. ALWAYS use `<TOOL>cd:path</TOOL>` which persists the change for all future commands.\n\
        - SYSTEM DIFF: <TOOL>system_diff:tasks</TOOL> or <TOOL>system_diff:network</TOOL> — Takes a snapshot of system processes or ports. Call it again later to get a DIFFERENCE (who died/closed, who was born/opened). Perfect for verifying if your commands worked.\n\
        - SEARCH RUNBOOKS: <TOOL>search_runbooks:query</TOOL> — uses TF-IDF Semantic similarity to fetch the top 2 company runbooks that match your technical issue query.\n\
        - SEARCH FILES: <TOOL>searchfiles:/directory|||pattern</TOOL> — searches text across all files. For multi-pattern search, separate words with '|' (e.g. ERROR|CRITICAL|PANIC), this uses Aho-Corasick for blazing speed.\n\
        - ANALYZE CODE: <TOOL>analyze_code:/path</TOOL> — uses Tree-Sitter to extract the Abstract Syntax Tree (AST) summary of Rust or JavaScript. Use this BEFORE reading the whole file if you only want to explore existing functions/classes.\n\
        - SUB-AGENTS (Parallel Forking):
        - <TOOL>fork_task:TaskID|||Instruction</TOOL> - Forks a fast background agent to investigate something while you continue. Does not return result immediately.
        - <TOOL>wait_task:TaskID</TOOL> - Waits for a forked task to finish and returns its findings.
        - MCP: <TOOL>mcp_query:server|||tool|||json_args</TOOL> - Spawns a local MCP server, asks for context/tool, and returns the result json natively.
        TOOL CHAINING: You are AUTHORIZED to use MULTIPLE tools in a single response to speed up your work. Simply output consecutive <TOOL>...</TOOL> tags. Use this to: search → read → analyze → edit → verify in parallel.\n\
        EDITING FILES: For modifications, ALWAYS prefer <TOOL>editfile</TOOL> over <TOOL>writefile</TOOL>. editfile does surgical find-and-replace — you only need to specify the exact block to change. Use writefile ONLY for creating new files or complete rewrites.\n\
        UX RULE (FILES MODIFIED): Never manually format a list of files you modified. The system interface will automatically group and display 'Files Modified' badges for the user when you use writefile or editfile.\n\
        CRITICAL: NEVER use PowerShell for file I/O. NEVER use Get-Content/Set-Content/Out-File. ALWAYS use these native tools.\n\
        RULE 18 — CODE ANALYSIS & MODIFICATION WORKFLOW:\n\
        When asked to analyze, review, fix, or modify code:\n\
        Step 1: <TOOL>listdir:/path</TOOL> to understand the project.\n\
        Step 2: <TOOL>searchfiles:/path|||keyword</TOOL> to find relevant code.\n\
        Step 3: <TOOL>readfile:/path</TOOL> or <TOOL>readlines:/path:START:COUNT</TOOL> to read the specific file.\n\
        Step 4: Analyze and explain findings wrapped in <THOUGHT>...</THOUGHT>. (Skip THOUGHT entirely if the logic is trivial to save latency).\n\
        Step 5: If asked to fix, use <TOOL>editfile:/path|||OLD_TEXT|||NEW_TEXT</TOOL> to patch the code.\n\
        Step 6: Optionally read back the modified file to verify the change.\n\
        NEVER respond with <TOOL>sysinfo</TOOL> when asked about code. NEVER use <EXECUTE> to read/write files.\n\
        When the user asks 'ves algún problema', 'revisa el código', 'analiza este archivo' → this is CODE ANALYSIS, not system health.\n\
        RULE 18.5 — AUTONOMOUS CODING AGENT CAPABILITIES:\n\
        You are an advanced agentic programmer. You can autonomously write, test, and debug code.\n\
        When tasked with software development (e.g., \"build this feature\", \"fix tests\", \"create a project\"):\n\
        1. Explore the codebase first using your file tracking tools (searchfiles, readfile).\n\
        2. Implement the requested code correctly using editfile or writefile.\n\
        3. ALWAYS verify your changes by executing the relevant build/test commands (e.g., 'cargo check', 'npm test') via <EXECUTE> or <EXECUTE_CMD>. Commands automatically run in the GLOBAL WORKING DIRECTORY (set with <TOOL>cd:path</TOOL>).\n\
        ⚠️ POWERSHELL SYNTAX: NEVER chain commands with `&&` — it fails on PowerShell 5.x. Use `;` to chain (e.g., `Set-Location dir; cargo check`) or better yet, change directory first with <TOOL>cd:path</TOOL> then run <EXECUTE>cargo check</EXECUTE> separately.\n\
        ⚠️ BUILD COMMANDS: Use `--manifest-path` for Cargo instead of cd: `cargo check --manifest-path X:\\path\\Cargo.toml`. For npm: `npm run build --prefix X:\\path`.\n\
        4. If a command fails, autonomously read the error, reason about the fix in <THOUGHT>, edit the file, and run the command again. Repeat this verify-fix loop until successful.\n\
        DO NOT ask the user for permission to fix your own compilation errors. Work autonomously as a senior developer.\n\
        RULE 19 — SELF-AWARENESS & ANTI-HALLUCINATION:\n\
        - Your rules and configuration are embedded in this system prompt. You do NOT have a config file on disk. If asked about your rules, logic, or how to improve your behavior, answer from what you know here — do NOT try to read files.\n\
        - NEVER invent or guess file paths. Use the WORKING DIRECTORY above as your base. When a user mentions a filename without full path, use <TOOL>searchfiles:{cwd}|||filename</TOOL> to locate it FIRST.\n\
        - If a TOOL returns an error (e.g. 'os error 3' = file not found), do NOT retry with a different guessed path. Instead, tell the user the file was not found and ask for the correct path.\n\
        - When asked about yourself, your logic, or how to improve: explain based on your rules above. Suggest improvements as text — do NOT try to modify your own code.\n\
        RULE 20 — LARGE FILE STRATEGY:\n\
        - You possess a massive context window. You are AUTHORIZED to use <TOOL>readfile:/path</TOOL> for any file up to 500KB (including massive files like +page.svelte) to gain full structural understanding.\n\
        - Only for files EXCEEDING 512KB, use <TOOL>searchfiles:/path|keyword</TOOL> followed by <TOOL>readlines:/path:START:COUNT</TOOL>.\n\
        RULE 21: When using the file editing tool, NEVER attempt to replace a single line of code, as duplicate lines may exist and the system will block the operation. Always include at least 2 preceding lines and 2 succeeding lines in your search string context to ensure the match is 100% unique across the entire file.
        {ctx}
        {hosts}
        The user's name is {uname}. Always address them by name.\nINSTRUCTION: {prompt}",
        lang = lang,
        ctx = context,
        hosts = hosts_context,
        uname = user_name,
        rb = runbooks_info,
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
    max_tokens_override: Option<u32>,
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

    let cwd = crate::state::GLOBAL_CWD.read().map(|c| c.clone()).unwrap_or_else(|_| "C:\\".to_string());
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
            // Le quitamos el prefijo "local-" y aplicamos parámetros de coherencia para Qwen
            let actual_model = model.replace("local-", "");
            let payload = json!({ 
                "model": actual_model, 
                "messages": [{"role": "user", "content": final_prompt}],
                "options": {
                    "temperature": 0.2,
                    "num_ctx": 8192,
                    "top_p": 0.9
                }
            });
            HTTP_CLIENT.post(&api_key).json(&payload)
        },
        "anthropic" => {
            let max_tok = get_max_tokens(&model, max_tokens_override);
            let payload = json!({ "model": model, "max_tokens": max_tok, "messages": [{"role": "user", "content": final_prompt}] });
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

    if !res.status().is_success() {
        let status = res.status();
        let err_text = res.text().await.unwrap_or_default();
        return Err(format!("Error API HTTP {}: {}", status, err_text));
    }

    let body_text = res.text().await.map_err(|e| format!("Error al leer body: {}", e))?;
    let v: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| format!("Error parseando JSON: {}", e))?;

    if provider == "anthropic" {
        if let Some(reason) = v["stop_reason"].as_str() {
            if reason == "max_tokens" {
                let text = v["content"].get(0).and_then(|c| c["text"].as_str()).unwrap_or("");
                return Ok(format!("{}\n__TRUNCATED__", text));
            }
        }
    }

    let text_result = match provider {
        "openai" | "local" => v["choices"].get(0).and_then(|c| c["message"]["content"].as_str()),
        "anthropic" => v["content"].get(0).and_then(|c| c["text"].as_str()),
        _ => v["candidates"].get(0).and_then(|c| c["content"]["parts"][0]["text"].as_str())
    };

    if let Some(t) = text_result {
        if let Some((input_tokens, output_tokens)) = match provider {
            "openai" | "local" => extract_tokens_openai(&v),
            "anthropic" => extract_tokens_anthropic(&v),
            _ => extract_tokens_gemini(&v),
        } {
            let _ = log_usage_internal(&model, input_tokens, output_tokens, "ask_lucy", &user_name).await;
        }

        Ok(t.to_string())
    } else {
        Err(format!("Respuesta API ({}): {}", provider, body_text))
    }
}

// ── ASK LUCY STREAMING (SSE) ──────────────────────────────────────────────────

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
    max_tokens_override: Option<u32>,
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

    let cwd = crate::state::GLOBAL_CWD.read().map(|c| c.clone()).unwrap_or_else(|_| "C:\\".to_string());
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
            // Le quitamos el prefijo "local-" y aplicamos parámetros de coherencia para Qwen
            let actual_model = model.replace("local-", "");
            let payload = json!({ 
                "model": actual_model, 
                "messages": [{"role": "user", "content": final_prompt}], 
                "stream": true,
                "options": {
                    "temperature": 0.2,
                    "num_ctx": 8192,
                    "top_p": 0.9
                }
            });
            HTTP_CLIENT.post(&api_key).json(&payload)
        },
        "anthropic" => {
            let max_tok = get_max_tokens(&model, max_tokens_override);
            let payload = json!({ "model": model, "max_tokens": max_tok, "messages": [{"role": "user", "content": final_prompt}], "stream": true });
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
    let mut was_truncated = false;
    let mut input_tokens: u32 = 0;
    let mut output_tokens: u32 = 0;

    while let Some(chunk) = byte_stream.next().await {
        let bytes = chunk.map_err(|e| format!("Error de stream: {}", e))?;
        line_buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(newline_pos) = line_buffer.find('\n') {
            let line = line_buffer[..newline_pos].trim().to_string();
            line_buffer = line_buffer[newline_pos + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" { continue; }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(reason) = v["delta"]["stop_reason"].as_str().or_else(|| {
                        v["choices"].get(0).and_then(|c| c["finish_reason"].as_str())
                    }) {
                        if reason == "max_tokens" || reason == "length" {
                            was_truncated = true;
                        }
                    }

                    if input_tokens == 0 && output_tokens == 0 {
                        if let Some((in_t, out_t)) = match provider {
                            "openai" | "local" => extract_tokens_openai(&v),
                            "anthropic" => extract_tokens_anthropic(&v),
                            _ => extract_tokens_gemini(&v),
                        } {
                            input_tokens = in_t;
                            output_tokens = out_t;
                        }
                    }

                    let text_chunk = match provider {
                        "openai" | "local" => v["choices"].get(0).and_then(|c| c["delta"]["content"].as_str()),
                        "anthropic" => v["delta"]["text"].as_str(),
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

    if was_truncated {
        full_text.push_str("\n__TRUNCATED__");
    }

    if input_tokens > 0 || output_tokens > 0 {
        let _ = log_usage_internal(&model, input_tokens, output_tokens, "ask_lucy_stream", &user_name).await;
    }

    Ok(full_text)
}

#[tauri::command]
pub fn log_agent_loop(message: String) {
    use std::io::Write;
    let path = crate::utils::logging::get_logs_dir().join("lucy_agent_loop.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), message);
    }
}
