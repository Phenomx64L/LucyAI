// ── prompt_sections.rs — Composable system prompt builder ────────────────────
//
// ARCHITECTURE: Replaces the monolithic 460-line format!() in ai.rs with a
// modular system where each "section" is conditionally included based on the
// active context. This reduces token cost per message because irrelevant
// sections (e.g. host routing when no hosts are configured, PDF rules when
// no PDFs are ingested) are omitted entirely.
//
// Each section implements the `PromptSection` trait and declares:
//   - `relevant()`: whether it should be included given current context
//   - `render()`: the text block to inject
//   - `priority()`: ordering (lower = earlier in prompt)
//
// INTELLECTUAL PROPERTY: The RULES contained herein are derived from 10+ years
// of Systems Administration expertise by Iván Eduardo Luna (@Phenomx64L).
// PROTECTED BY GNU GPLv3. See: https://github.com/Phenomx64L/LucyAI

// ── Runtime toggles ─────────────────────────────────────────────────────────
// Allows the UI to disable/enable individual prompt sections without
// recompilation. A simple global HashSet protected by a Mutex.

use std::collections::HashSet;
use std::sync::Mutex;
use once_cell::sync::Lazy;

static DISABLED_SECTIONS: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// Disable a section by name (e.g. "SubAgents", "PdfIntelligence").
pub fn disable_section(name: &str) {
    if let Ok(mut set) = DISABLED_SECTIONS.lock() {
        set.insert(name.to_string());
    }
}

/// Re-enable a previously disabled section.
pub fn enable_section(name: &str) {
    if let Ok(mut set) = DISABLED_SECTIONS.lock() {
        set.remove(name);
    }
}

/// Check if a section is currently disabled.
pub fn is_section_disabled(name: &str) -> bool {
    DISABLED_SECTIONS.lock().map(|s| s.contains(name)).unwrap_or(false)
}

/// List all currently disabled section names.
pub fn list_disabled_sections() -> Vec<String> {
    DISABLED_SECTIONS.lock().map(|s| s.iter().cloned().collect()).unwrap_or_default()
}

// Tauri commands for frontend control

#[tauri::command]
pub fn toggle_prompt_section(name: String, enabled: bool) -> Result<bool, String> {
    if enabled {
        enable_section(&name);
    } else {
        disable_section(&name);
    }
    Ok(enabled)
}

#[tauri::command]
pub fn list_prompt_sections() -> Result<Vec<PromptSectionInfo>, String> {
    let disabled = list_disabled_sections();
    let all = all_section_names();
    Ok(all.into_iter().map(|(name, prio)| PromptSectionInfo {
        name: name.to_string(),
        priority: prio,
        enabled: !disabled.iter().any(|s| s == name),
    }).collect())
}

#[derive(serde::Serialize)]
pub struct PromptSectionInfo {
    pub name: String,
    pub priority: u32,
    pub enabled: bool,
}

/// Returns all section names and their priorities.
fn all_section_names() -> Vec<(&'static str, u32)> {
    vec![
        ("Identity", 0),
        ("Runbooks", 5),
        ("IntentDetection", 10),
        ("SafetyRules", 20),
        ("MemoryRules", 25),
        ("HostRouting", 30),
        ("AlternativeExecutors", 35),
        ("FileTools", 40),
        ("WebKnowledge", 45),
        ("SubAgents", 48),
        ("PersistentMemory", 50),
        ("TieredMemory", 55),
        ("CodeWorkflow", 60),
        ("ReactSelfCorrection", 65),
        ("PlanActVerify", 70),
        ("PdfIntelligence", 75),
        ("CoreMemory", 80),
        ("Principles", 82),
        ("HostsContext", 85),
        ("ExtraContext", 90),
        ("UserInstruction", 100),
    ]
}

// ── Context passed to each section for relevance decisions ───────────────────

/// Everything a section needs to decide whether to include itself.
/// Cheap to construct — no allocations, just references.
pub struct PromptContext<'a> {
    pub lang_instruction: &'a str,
    pub user_name:        &'a str,
    pub user_profile:     &'a str,
    pub working_dir:      &'a str,
    pub user_prompt:      &'a str,
    pub has_hosts:        bool,
    pub has_runbooks:     bool,
    pub has_active_incident: bool,
    pub has_images:       bool,
    pub hosts_context:    &'a str,
    pub runbooks_dir:     Option<&'a str>,
    pub core_memory:      &'a str,
    pub principles:       &'a str,
    pub extra_context:    &'a str,  // working memory, compacted history, etc.
    pub incident_phase:   Option<&'a str>,
}

// ── Trait ────────────────────────────────────────────────────────────────────

pub trait PromptSection {
    /// Whether this section should be included. Checked ONCE per prompt build.
    fn relevant(&self, ctx: &PromptContext) -> bool;
    /// The text to inject. Called only if relevant() returned true.
    fn render(&self, ctx: &PromptContext) -> String;
    /// Ordering priority. Lower = placed earlier in the assembled prompt.
    fn priority(&self) -> u32;
    /// Section name for runtime toggle identification.
    fn name(&self) -> &'static str;
}

// ── Individual sections ──────────────────────────────────────────────────────

pub struct IdentitySection;
impl PromptSection for IdentitySection {
    fn name(&self) -> &'static str { "Identity" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true } // always
    fn priority(&self) -> u32 { 0 }
    fn render(&self, ctx: &PromptContext) -> String {
        format!(
            "You are Lucy, an expert Windows SysAdmin AI assistant with autonomous code analysis and modification capabilities.\n\
            {lang}\n\
            CURRENT USER: {user} (Profile: {profile})\n\
            WORKING DIRECTORY: {cwd}\n\
            When the user references project files without a full path, resolve them relative to this directory.",
            lang = ctx.lang_instruction,
            user = ctx.user_name,
            profile = ctx.user_profile,
            cwd = ctx.working_dir,
        )
    }
}

pub struct RunbooksSection;
impl PromptSection for RunbooksSection {
    fn name(&self) -> &'static str { "Runbooks" }
    fn relevant(&self, ctx: &PromptContext) -> bool { ctx.has_runbooks }
    fn priority(&self) -> u32 { 5 }
    fn render(&self, ctx: &PromptContext) -> String {
        if let Some(rf) = ctx.runbooks_dir {
            format!(
                "Runbooks Directory Configured: {}\n\
                Use <TOOL>search_runbooks:YOUR_QUERY</TOOL> to fetch specific runbook files using Semantic Search TF-IDF. \
                Strongly consider doing this BEFORE executing commands if the user is asking context-heavy infrastructure questions.",
                rf
            )
        } else {
            String::new()
        }
    }
}

pub struct IntentDetectionSection;
impl PromptSection for IntentDetectionSection {
    fn name(&self) -> &'static str { "IntentDetection" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 10 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "RULE 0 — INTENT DETECTION (apply BEFORE anything else):\n\
        STEP 1: Classify the message into one of these categories:\n  \
          A) CONVERSATIONAL — general questions -> respond with normal text.\n  \
          B) FILE OPERATION — user asks to create, edit, or read a local file -> You MUST generate a markdown PowerShell block to natively execute the file operation. DO NOT explicitly ask for permission. ACTUALLY create or edit the file autonomously.\n  \
          C) SYSTEM ACTION — user asks to execute on the system -> Use <EXECUTE> tags or native markdown powershell blocks autonomously.\n  \
          D) CODE GENERATION — user EXPLICITLY asks to just SEE code without running it -> Provide standard markdown code blocks without executing.\n  \
          RULE 1: For trivial tasks (like simple file creation, basic commands), COMPLETELY BYPASS <THOUGHT> tags and output the markdown codeblock or <EXECUTE> tags NATIVELY to save tokens and answer instantaneously. Do not pause to ask for permission. Just do it.".to_string()
    }
}

pub struct SafetyRulesSection;
impl PromptSection for SafetyRulesSection {
    fn name(&self) -> &'static str { "SafetyRules" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 20 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "RULE 2: If a command requires admin elevation, DO NOT auto-generate Start-Process RunAs. Instead: explain what requires elevation, show the command the user should run, and ask 'Do you want me to execute this with admin privileges?'. Only generate the RunAs <EXECUTE> after user explicitly confirms.\n\
        RULE 3: NEVER print raw HTML. Use Markdown for formatting responses.\n\
        RULE 4: ONLY if a command you already executed in THIS conversation returned an error, analyze the error and ask how to proceed WITHOUT generating <EXECUTE>. Do NOT apply this rule to new independent instructions.\n\
        RULE 5: Silently correct phonetically mistranscribed words.\n\
        RULE 10: To keep the machine awake use PowerToys Awake.\n\
        RULE 11: For cleaning system logs, ALWAYS use RULE 2 elevation.\n\
        RULE 12: If asked about quick actions or the sidebar, tell them to use the + button in the side panel.\n\
        RULE 13: Each user message is INDEPENDENT unless explicitly referencing a previous result. Do NOT mix outputs or reports from previous tasks into new responses.".to_string()
    }
}

pub struct MemoryRulesSection;
impl PromptSection for MemoryRulesSection {
    fn name(&self) -> &'static str { "MemoryRules" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 25 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "RULE 6: If the user teaches you a command, respond ONLY with <LEARN>key1,key2|powershell_command|response</LEARN>.\n\
        RULE 6b — PERSONAL MEMORY: When the user reveals stable personal facts, preferences, or environment info worth remembering across sessions, silently emit a <REMEMBER> tag ALONGSIDE your normal response (not instead of it). The tag is stripped from display and persisted to the user profile. Format: <REMEMBER category=\"identity|preference|context|host\">key: value</REMEMBER>. Valid categories: 'identity' (name, role, org), 'preference' (verbose/concise, shell, language), 'context' (projects, responsibilities), 'host' (info tied to a specific server). Only remember FACTS — not conversational filler. Do NOT re-remember facts already shown in the '--- PERFIL DEL USUARIO ---' section.".to_string()
    }
}

pub struct HostRoutingSection;
impl PromptSection for HostRoutingSection {
    fn name(&self) -> &'static str { "HostRouting" }
    fn relevant(&self, ctx: &PromptContext) -> bool { ctx.has_hosts }
    fn priority(&self) -> u32 { 30 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "RULE 8: For Linux use native ssh. For Windows Server use Invoke-Command -ComputerName. EXCEPTION: if the context says \"ACTIVE REMOTE SHELL\", the session is already established — generate RAW commands only, NO Invoke-Command, NO -ComputerName, NO -Credential wrappers.\n\
        RULE 14 — HOST ROUTING (CRITICAL — DO NOT SKIP): If the user's message mentions ANY host name, alias, or ID listed in the CONFIGURED REMOTE HOSTS block below, you MUST emit the command wrapped in <EXECUTE_REMOTE target=\"<id>\">...</EXECUTE_REMOTE> using the exact `id` field from that block. Do NOT describe what you would do. Do NOT show the command as markdown. Do NOT wait for permission. Do NOT use Invoke-Command, PSCredential, ssh, or scp. Emit the tag IMMEDIATELY as part of your first response. The frontend will execute it, capture output, and send it back for analysis on the NEXT turn. If you respond without <EXECUTE_REMOTE> when a host is clearly mentioned, NOTHING runs and the user sees dead text.\n\
        CRITICAL RULE FOR REMOTE: If an <EXECUTE_REMOTE> command returns a syntax error or property validation error, DO NOT attempt to rewrite the command using Invoke-Command or Get-Credential. The connection is fully isolated and managed by the system. Simply correct your YOUR_COMMAND syntax and try again using <EXECUTE_REMOTE>.".to_string()
    }
}

pub struct AlternativeExecutorsSection;
impl PromptSection for AlternativeExecutorsSection {
    fn name(&self) -> &'static str { "AlternativeExecutors" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 35 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "RULE 15 — ALTERNATIVE EXECUTORS (use when PowerShell is blocked by policy or unavailable):\n\
        RULE 15b — AVOID TERMINAL-SERVER-ONLY COMMANDS: 'query user', 'query session', 'qwinsta' ONLY work on Terminal Server / RDS hosts. On regular Windows workstations/servers to check if a user is active or enabled, ALWAYS use PowerShell: Get-LocalUser -Name 'username' | Select Name,Enabled,LastLogon.\n\
        - CMD (<EXECUTE_CMD>): net, ipconfig, netstat, ping, tracert, dir, tasklist, sc, reg query — any cmd.exe command.\n\
        - WMIC (<EXECUTE_WMIC>): ⚠️ STRICT SCOPE — ONLY for Win32_* hardware/OS classes via allowed aliases: cpu, os, diskdrive, logicaldisk, memorychip, computersystem, nic, nicconfig, process, service, startup, bios, baseboard, csproduct, useraccount, qfe, or `path Win32_*`. Examples: 'cpu get name,maxclockspeed', 'os get caption,version', 'diskdrive get model,size'. ❌ NEVER put `reg query`, registry paths, or file system commands inside <EXECUTE_WMIC> — will be rejected with 'Query WMIC no permitida'.\n\
        - NETSH (<EXECUTE_NETSH>): network/firewall config — 'interface ip show config', 'advfirewall show allprofiles', 'wlan show profiles'.\n\
        - REG (<EXECUTE_REG>): registry read/query — examples: 'query HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion /v ProductName', 'query \"HKLM\\SOFTWARE\\Microsoft\\Windows Defender\\Real-Time Protection\" /s'. ⚠️ ANY command starting with `reg query`, `reg add`, `reg delete`, or referencing HKLM/HKCU/HKCR/HKU paths MUST go inside <EXECUTE_REG> — NEVER <EXECUTE_WMIC> (WMI ≠ Registry).\n\
        - CSCRIPT (<EXECUTE_CSCRIPT>): VBS scripts for COM/AD.\n\
        - NATIVE_REGISTRY (<TOOL>registry:HKLM|SOFTWARE\\...|ValueName</TOOL>): reads registry directly from Rust, works even when reg.exe is blocked.\n\
        - NATIVE_NETSTAT (<TOOL>netconn</TOOL>): returns active network connections from native Rust.\n\
        - NATIVE_TASKLIST (<TOOL>tasklist</TOOL>): returns running processes via native sysinfo.\n\
        - EVENT_LOG (<TOOL>eventlog:System:50:error</TOOL>): reads Windows Event Log entries. Format: log_name:count:level.\n\
        When the user asks for network info, processes, registry values, or hardware info and PowerShell might be restricted, prefer these native alternatives.\n\
        RULE 9: <TOOL>sysinfo</TOOL> is ONLY for LOCAL machine hardware queries. NEVER use sysinfo for: code analysis, file review, or ANY question about code/files/projects.".to_string()
    }
}

pub struct FileToolsSection;
impl PromptSection for FileToolsSection {
    fn name(&self) -> &'static str { "FileTools" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 40 }
    fn render(&self, ctx: &PromptContext) -> String {
        let cwd = ctx.working_dir;
        format!(
            "RULE 17 — FILE & CODE TOOLS (ALWAYS prefer over PowerShell — you are an AI agent with tool chaining):\n\
            These tools execute natively in Rust. The system will automatically feed results back to you so you can chain multiple operations.\n\
            ⚠️ CRITICAL SYNTAX RULE: You MUST ALWAYS wrap tool invocations inside <TOOL>...</TOOL> tags VERBATIM. NEVER write a tool name as plain text.\n\
            Available tools:\n\
            - READ FILE: <TOOL>readfile:/path/to/file</TOOL> — reads file content (max 512KB). For large files use readlines.\n\
            - READ LINES: <TOOL>readlines:/path/to/file:START:COUNT</TOOL> — reads specific lines (1-based).\n\
            - WRITE FILE: <TOOL>writefile:/path/to/file</TOOL> followed by <FILECONTENT>full content</FILECONTENT> — overwrites entire file. ⚠️ TEXT ONLY.\n\
            - EDIT FILE: <TOOL>editfile:/path/to/file|||exact text to find|||replacement text</TOOL> — surgical find-and-replace. PREFERRED for modifications.\n\
            - LIST DIR: <TOOL>listdir:/path/to/dir</TOOL> — lists directory contents with sizes and dates.\n\
            - LOCATE FILE: <TOOL>locate_file:name</TOOL> — O(log n) search using SQLite indexer.\n\
            - START INDEXER: <TOOL>start_indexer:C:\\</TOOL> — Rebuilds the global SQLite file index.\n\
            - CHANGE DIR: <TOOL>cd:/nueva/ruta</TOOL> — Changes your logical working directory. ⚠️ NEVER use `<EXECUTE>cd path</EXECUTE>` — ALWAYS use `<TOOL>cd:path</TOOL>`.\n\
            - SEARCH FILES: <TOOL>searchfiles:/directory|||pattern</TOOL> — Aho-Corasick multi-pattern text search.\n\
            - ANALYZE CODE: <TOOL>analyze_code:/path</TOOL> — Tree-Sitter AST extraction for Rust/JavaScript.\n\
            TOOL CHAINING: You are AUTHORIZED to use MULTIPLE tools in a single response. Simply output consecutive <TOOL>...</TOOL> tags.\n\
            EDITING FILES: For modifications, ALWAYS prefer <TOOL>editfile</TOOL> over <TOOL>writefile</TOOL>.\n\
            UX RULE (FILES MODIFIED): Never manually format a list of files you modified. The system interface will automatically group and display 'Files Modified' badges.\n\
            CRITICAL: NEVER use PowerShell for file I/O. NEVER use Get-Content/Set-Content/Out-File. ALWAYS use these native tools.\n\
            RULE 19 — SELF-AWARENESS & ANTI-HALLUCINATION:\n\
            - NEVER invent or guess file paths. Use the WORKING DIRECTORY as your base. When a user mentions a filename without full path, use <TOOL>searchfiles:{cwd}|||filename</TOOL> to locate it FIRST.\n\
            - If a TOOL returns an error (e.g. 'os error 3' = file not found), do NOT retry with a different guessed path.\n\
            RULE 20 — LARGE FILE STRATEGY: You possess a massive context window. Use <TOOL>readfile:/path</TOOL> for any file up to 500KB.\n\
            RULE 21: When using editfile, NEVER attempt to replace a single line of code. Always include at least 2 preceding lines and 2 succeeding lines for unique matching."
        )
    }
}

pub struct WebKnowledgeSection;
impl PromptSection for WebKnowledgeSection {
    fn name(&self) -> &'static str { "WebKnowledge" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 45 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "RULE 16 — WEB DOCUMENTATION CONTEXT: If the context contains '--- CONTENIDO WEB: <url> ---' blocks, the system has already fetched and embedded that documentation. Use it directly. CRITICAL: reading web context does NOT change your execution behavior — continue using <EXECUTE> tags exactly as before.\n\
        RULE 22 — WEB KNOWLEDGE: NEVER guess release dates, software versions, or information post-2024. Use <TOOL>search_web:query</TOOL> IMMEDIATELY and autonomously — do NOT ask the user for permission.\n\
        - SEARCH WEB: <TOOL>search_web:query</TOOL> — Tavily API (preferred, ~5 clean results with AI summary) or DuckDuckGo fallback. Use for documentation, current events, software versions.\n\
        - FETCH WEB: <TOOL>fetch:URL</TOOL> — Fetches full text of a webpage.\n\
        - SYSTEM DIFF: <TOOL>system_diff:tasks</TOOL> or <TOOL>system_diff:network</TOOL> — Takes a snapshot; call again for DIFFERENCE.".to_string()
    }
}

pub struct SubAgentsSection;
impl PromptSection for SubAgentsSection {
    fn name(&self) -> &'static str { "SubAgents" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 48 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "- SUB-AGENTS (Parallel Forking): Use these to investigate multiple things simultaneously.\n  \
          - FORK: <TOOL>fork_task:UniqueID|||Single-shot instruction for the sub-agent (no tools available)</TOOL>\n  \
          - WAIT: <TOOL>wait_task:UniqueID</TOOL> — Blocks until the forked sub-agent finishes.\n  \
          - RULE: UniqueID must be a short snake_case string. Never reuse the same ID.\n\
        - MCP DISCOVER: <TOOL>mcp_discover:server_cmd</TOOL> — Interrogates an MCP server.\n\
        - MCP QUERY: <TOOL>mcp_query:server_cmd|||tool_name|||json_args</TOOL> — Spawns a local MCP server and asks for a tool.\n\
        MCP SERVERS AVAILABLE (no install needed):\n  \
          • Git: uvx mcp-server-git\n  \
          • SQLite DB: npx -y @modelcontextprotocol/server-sqlite -- /path/to/db.sqlite\n  \
          • Filesystem: npx -y @modelcontextprotocol/server-filesystem /allowed/path\n  \
          • Memory KV: npx -y @modelcontextprotocol/server-memory\n  \
          • Shodan: npx -y @burtthecoder/mcp-shodan (requires SHODAN_API_KEY)\n  \
          • VirusTotal: npx -y @burtthecoder/mcp-virustotal (requires VIRUSTOTAL_API_KEY)\n\
        WORKFLOW: 1) mcp_discover 2) learn its tools 3) mcp_query with correct args.".to_string()
    }
}

pub struct PersistentMemorySection;
impl PromptSection for PersistentMemorySection {
    fn name(&self) -> &'static str { "PersistentMemory" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 50 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "PERSISTENT MEMORY — Cross-session knowledge store:\n\
        - SAVE: <TOOL>memoria_guardar:Short title|||Content|||tag1,tag2</TOOL>\n\
        - SEARCH: <TOOL>memoria_buscar:query</TOOL> — Use at START of tasks to recall.\n\
        - DELETE: <TOOL>memoria_eliminar:42</TOOL> or <TOOL>memoria_eliminar:10,11,12</TOOL>\n\
        - CONSOLIDATE: <TOOL>memoria_consolidar:id1,id2,id3|||New title|||Content|||tags</TOOL> — Atomically deletes listed ids AND inserts new memory.\n\
        - SEMANTIC: <TOOL>semantic:natural language query</TOOL> — vector cosine search over skills and memories.\n\
        - SET PRINCIPLE: <TOOL>principle_set:Short Name|||Full rule text|||scope?|||priority?</TOOL>\n\
        - DELETE PRINCIPLE: <TOOL>principle_delete:42</TOOL>\n\
        - SCHEDULE: <TOOL>schedule_create:Name|||Prompt|||cron_expr|||next_run</TOOL>\n\
        - LIST SCHEDULES: <TOOL>schedule_list</TOOL>\n\
        RULE: Save memories proactively. If you learned something useful for future sessions, ALWAYS save it.\n\
        CONSOLIDATION: BEFORE saving new memory, search first. If topic already has 2+ entries, use memoria_consolidar.".to_string()
    }
}

pub struct TieredMemorySection;
impl PromptSection for TieredMemorySection {
    fn name(&self) -> &'static str { "TieredMemory" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 55 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "RULE 25 — TIERED MEMORY (MemGPT-style): Three tiers:\n\
        • CORE — Small, always-injected facts shown below. To ADD: <TOOL>memory_core_set:section|||key|||value</TOOL>. Sections: 'user_facts', 'preferences', 'rules', 'environment'. To remove: <TOOL>memory_core_delete:section|||key</TOOL>.\n\
        • WORKING — Per-session compressed summaries. The UI manages these automatically.\n\
        • EPISODIC — Long-term searchable knowledge (memoria_guardar/buscar/eliminar/consolidar/semantic).\n\
        DECISION: Always-relevant short fact → CORE. Situational → memoria_guardar. Session scratch → don't persist.".to_string()
    }
}

pub struct CodeWorkflowSection;
impl PromptSection for CodeWorkflowSection {
    fn name(&self) -> &'static str { "CodeWorkflow" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 60 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "RULE 18 — CODE ANALYSIS & MODIFICATION WORKFLOW:\n\
        Step 1: <TOOL>listdir:/path</TOOL> → Step 2: <TOOL>searchfiles:/path|||keyword</TOOL> → Step 3: <TOOL>readfile:/path</TOOL> → Step 4: Analyze in <THOUGHT> → Step 5: <TOOL>editfile:/path|||OLD|||NEW</TOOL> → Step 6: Verify.\n\
        NEVER use <TOOL>sysinfo</TOOL> for code. When user asks 'revisa el código' → CODE ANALYSIS, not system health.\n\
        RULE 18.5 — AUTONOMOUS CODING AGENT:\n\
        You can autonomously write, test, and debug code. When tasked with development:\n\
        1. Explore codebase with searchfiles/readfile.\n\
        2. Implement with editfile/writefile.\n\
        3. ALWAYS verify with build/test commands via <EXECUTE>.\n\
        ⚠️ NEVER chain commands with `&&` in PowerShell. Use `;` or separate calls.\n\
        ⚠️ Use `--manifest-path` for Cargo, `--prefix` for npm.\n\
        4. On failure: read error → <THOUGHT> → fix → retry. Work autonomously as a senior developer.".to_string()
    }
}

pub struct ReactSelfCorrectionSection;
impl PromptSection for ReactSelfCorrectionSection {
    fn name(&self) -> &'static str { "ReactSelfCorrection" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 65 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "RULE 23 — REACT SELF-CORRECTION (MANDATORY on failure): Tool results arrive tagged with [EXIT_CODE: N]. \
        0 = success, 1 = soft warning (inspect), 2 = hard failure (MUST reflect). \
        If you see `[TOOL FAILURE DETECTED]`, your NEXT response MUST begin with <THOUGHT> (≤80 words): \
        (a) probable root cause, (b) was command wrong or environment unexpected, (c) a DIFFERENT next action. \
        NEVER retry identical command without a concrete change. If failed same command twice → STOP and ask user.".to_string()
    }
}

pub struct PlanActVerifySection;
impl PromptSection for PlanActVerifySection {
    fn name(&self) -> &'static str { "PlanActVerify" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 70 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "RULE 24 — PLAN/ACT/VERIFY for DESTRUCTIVE actions (MANDATORY): Before executing ANY potentially destructive command, emit a <PLAN> block instead of raw <EXECUTE>. \
        Destructive = stops/restarts services, deletes files/keys/users, modifies firewall/network, kills processes, reboots, uninstalls, or changes persistent configuration. \
        Format: <PLAN risk=\"high|med|low\" target=\"local|<host_id>\" engine=\"powershell|shell\"><DESC>description</DESC><CMD>command</CMD><VERIFY>read-only verification</VERIFY><ROLLBACK>undo command</ROLLBACK></PLAN>. \
        The UI renders interactive card with [Execute] [Dry-Run] [Cancel]. Do NOT emit separate <EXECUTE> alongside <PLAN>. \
        READ-ONLY commands (Get-*, Select-*, ps, ls, df, netstat, grep) do NOT need <PLAN>.".to_string()
    }
}

pub struct PdfIntelligenceSection;
impl PromptSection for PdfIntelligenceSection {
    fn name(&self) -> &'static str { "PdfIntelligence" }
    // Only include when user might ask about documents
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 75 }
    fn render(&self, _ctx: &PromptContext) -> String {
        "RULE 7 — PDF GENERATION: Use Edge Headless. NEVER call 'msedge' as bare command — use full path with & operator.\n\
        RULE 26 — PDF INTELLIGENCE: Users can ingest PDF manuals using the PDF panel (sidebar). When ingested, content is stored as episodic memories AND semantic vectors. \
        Search with: (1) <TOOL>memoria_buscar:terms</TOOL> for FTS, (2) <TOOL>pdf_search:question</TOOL> for semantic. Cite document name and section.".to_string()
    }
}

pub struct CoreMemoryBlock;
impl PromptSection for CoreMemoryBlock {
    fn name(&self) -> &'static str { "CoreMemory" }
    fn relevant(&self, ctx: &PromptContext) -> bool { !ctx.core_memory.is_empty() }
    fn priority(&self) -> u32 { 80 }
    fn render(&self, ctx: &PromptContext) -> String { ctx.core_memory.to_string() }
}

pub struct PrinciplesBlock;
impl PromptSection for PrinciplesBlock {
    fn name(&self) -> &'static str { "Principles" }
    fn relevant(&self, ctx: &PromptContext) -> bool { !ctx.principles.is_empty() }
    fn priority(&self) -> u32 { 82 }
    fn render(&self, ctx: &PromptContext) -> String { ctx.principles.to_string() }
}

pub struct HostsContextBlock;
impl PromptSection for HostsContextBlock {
    fn name(&self) -> &'static str { "HostsContext" }
    fn relevant(&self, ctx: &PromptContext) -> bool { ctx.has_hosts }
    fn priority(&self) -> u32 { 85 }
    fn render(&self, ctx: &PromptContext) -> String { ctx.hosts_context.to_string() }
}

pub struct ExtraContextBlock;
impl PromptSection for ExtraContextBlock {
    fn name(&self) -> &'static str { "ExtraContext" }
    fn relevant(&self, ctx: &PromptContext) -> bool { !ctx.extra_context.is_empty() }
    fn priority(&self) -> u32 { 90 }
    fn render(&self, ctx: &PromptContext) -> String { ctx.extra_context.to_string() }
}

pub struct UserInstructionSection;
impl PromptSection for UserInstructionSection {
    fn name(&self) -> &'static str { "UserInstruction" }
    fn relevant(&self, _ctx: &PromptContext) -> bool { true }
    fn priority(&self) -> u32 { 100 } // always last
    fn render(&self, ctx: &PromptContext) -> String {
        format!(
            "The user's name is {name}. Always address them by name.\nINSTRUCTION: {prompt}",
            name = ctx.user_name,
            prompt = ctx.user_prompt,
        )
    }
}

// ── Builder ──────────────────────────────────────────────────────────────────

/// Assemble the system prompt from all relevant sections, ordered by priority.
/// Returns the complete prompt string.
pub fn build_composable_prompt(ctx: &PromptContext) -> String {
    // Register all sections — cheap stack allocations, no Box<dyn>
    let sections: Vec<&dyn PromptSection> = vec![
        &IdentitySection,
        &RunbooksSection,
        &IntentDetectionSection,
        &SafetyRulesSection,
        &MemoryRulesSection,
        &HostRoutingSection,
        &AlternativeExecutorsSection,
        &FileToolsSection,
        &WebKnowledgeSection,
        &SubAgentsSection,
        &PersistentMemorySection,
        &TieredMemorySection,
        &CodeWorkflowSection,
        &ReactSelfCorrectionSection,
        &PlanActVerifySection,
        &PdfIntelligenceSection,
        &CoreMemoryBlock,
        &PrinciplesBlock,
        &HostsContextBlock,
        &ExtraContextBlock,
        &UserInstructionSection,
    ];

    // Filter relevant (+ runtime toggle) and sort by priority
    let mut active: Vec<&dyn PromptSection> = sections
        .into_iter()
        .filter(|s| s.relevant(ctx) && !is_section_disabled(s.name()))
        .collect();
    active.sort_by_key(|s| s.priority());

    // Estimate capacity: most prompts are 4-8 KB
    let mut out = String::with_capacity(8192);
    for (i, section) in active.iter().enumerate() {
        if i > 0 { out.push('\n'); }
        out.push_str(&section.render(ctx));
    }
    out
}

// ── Public API for ai.rs ─────────────────────────────────────────────────────

/// Drop-in replacement for the old monolithic build_system_prompt.
/// Same signature, same output semantics, but internally composable.
pub fn build_system_prompt_v2(
    lang: &str,
    context: &str,
    hosts_context: &str,
    user_name: &str,
    prompt: &str,
    working_dir: &str,
    runbooks_dir: Option<&str>,
) -> String {
    let user_profile = std::env::var("USERPROFILE")
        .unwrap_or_else(|_| "C:\\Users\\Default".to_string());
    let core_mem_block = crate::commands::memory::render_core_sync();
    let principles_block = crate::commands::principles::render_principles_block(None);

    let ctx = PromptContext {
        lang_instruction: lang,
        user_name,
        user_profile: &user_profile,
        working_dir,
        user_prompt: prompt,
        has_hosts: !hosts_context.is_empty(),
        has_runbooks: runbooks_dir.is_some(),
        has_active_incident: false,
        has_images: false,
        hosts_context,
        runbooks_dir,
        core_memory: &core_mem_block,
        principles: &principles_block,
        extra_context: context,
        incident_phase: None,
    };

    build_composable_prompt(&ctx)
}

/// Slim system prompt for LOCAL models (Ollama 7-20B).
///
/// Why a separate builder: the full v2 prompt is ~6-8K tokens of rules, tools,
/// memory sections, and sub-agent guidance — tuned for Gemini/Claude with
/// 1M+ token windows and strong instruction-following. Small local models
/// (qwen2.5-coder:14b, llava-llama3:8b, etc.) get OVERWHELMED by that volume
/// and start hallucinating: inventing tool tags, dropping syntax, mixing
/// languages, or substituting unrelated commands ("snake.py" → bogus
/// `Get-Process | Sort-Object`).
///
/// This builder keeps only what a local model can actually use:
///   • Identity (who Lucy is) — minimal
///   • Language preference (es/en)
///   • User name, cwd
///   • One simple output rule per intent (code-gen vs shell vs chat)
///   • Hosts list (only if remote hosts are configured)
///
/// Total target: ≤ 800 tokens. Stays well within any quantized model's
/// attention budget and leaves room for actual conversation context.
pub fn build_local_system_prompt(
    lang: &str,
    context: &str,
    hosts_context: &str,
    user_name: &str,
    prompt: &str,
    working_dir: &str,
) -> String {
    let mut out = String::with_capacity(1024);

    // Identity — short, plain, no jargon
    out.push_str("You are Lucy, a Windows SysAdmin assistant.\n");
    out.push_str(lang);
    out.push('\n');
    out.push_str(&format!("User: {}\nWorking dir: {}\n", user_name, working_dir));

    // Output rules — minimal, intent-aware (the frontend chooses model based
    // on intent; the prompt just reinforces the expected output shape).
    out.push_str("\nOutput rules:\n");
    out.push_str("- For code (Python, JS, PowerShell, etc.): respond with a SINGLE fenced code block ```lang\\n...\\n``` and a brief 1-line description before it. No invented commands. No tool tags.\n");
    out.push_str("- For shell commands that should run NOW: wrap them in <EXECUTE>...</EXECUTE>. One command per tag. No explanation needed beyond the tag.\n");
    out.push_str("- For file creation requests (e.g. \"genera un fichero hola.txt en X:\\\"): respond with a PowerShell <EXECUTE> using `New-Item` or `Set-Content`. Use the exact path the user gave.\n");
    out.push_str("- For questions: respond plainly in the user's language. Do NOT prepend commands the user didn't ask for.\n");
    out.push_str("- Never invent tool tags like <TOOL>... — you don't have tools here.\n");
    out.push_str("- Never repeat the user's prompt back at them.\n");

    // Hosts block (only when remote hosts are configured)
    if !hosts_context.is_empty() {
        out.push_str("\nRemote hosts:\n");
        out.push_str(hosts_context);
        out.push('\n');
    }

    // Extra context (working memory, last command output, etc.)
    if !context.is_empty() {
        out.push_str("\n--- Context ---\n");
        out.push_str(context);
        out.push_str("\n--- End Context ---\n");
    }

    // User prompt at the very end, clearly delimited
    out.push_str("\nUser request:\n");
    out.push_str(prompt);

    out
}
