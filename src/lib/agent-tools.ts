// ── agent-tools.ts — canonical tool taxonomy for the agent loop ─────────────
//
// SINGLE SOURCE OF TRUTH for "which <TOOL> / tag does the model want, and does
// this response need the multi-step agent loop?". Extracted v1.7.212 as the
// foundation of the runAI() de-monolithing effort.
//
// Why this module exists
// ----------------------
// The same regexes + predicates lived in BOTH `llm-stream.ts` AND inline inside
// `+page.svelte`'s runAI(). They had already DIVERGED: llm-stream's
// NATIVE_TOOL_RE listed only 11 native tools while the inline copy (the one the
// chat path actually used) listed ~25. Any consumer of llm-stream's
// needsAgentLoop() silently failed to recognise ~14 native tools. Centralising
// here kills that whole class of drift — there is now exactly one list, tested.
//
// Pure + framework-free (no Svelte, no Tauri) so it unit-tests trivially.

/**
 * File / agent tools — the ones that drive the multi-step agent loop (read,
 * write, search, memory ops, scheduling…). Capture group 1 is the tool name.
 */
export const FILE_TOOL_RE = /<TOOL>(readfile|readlines|writefile|listdir|searchfiles|editfile|locate_file|start_indexer|analyze_code|mcp_query|graphify|memoria_guardar|memoria_buscar|memoria_eliminar|memoria_consolidar|memory_core_set|memory_core_delete|fork_task|wait_task|cd|pdf_search|principle_set|principle_delete|schedule_create|schedule_list):/i;

/**
 * Native tools — system/diagnostic/knowledge-graph tools. This is the COMPLETE
 * list (the value that was live in +page.svelte; llm-stream's old copy was
 * missing state_diff, process_lineage, process_ancestry, diagnose_spike,
 * healing_find, threat_scan, obj_query, runbook_scan, daily_patterns,
 * sandbox_preview, kg_*, incident_detective). Capture group 1 is the tool name
 * for the ones whose match begins with a word (the `:`-suffixed entries match
 * the prefix only).
 */
export const NATIVE_TOOL_RE = /<TOOL>(sysinfo|netconn|tasklist|eventlog:|registry:|system_diff:|state_diff:|process_lineage:|process_ancestry:|diagnose_spike|healing_find:|threat_scan|obj_query:|runbook_scan|daily_patterns|sandbox_preview:|kg_neighbors:|kg_recent|kg_ext_summary|incident_detective|search_runbooks:|search_web:|semantic:|fetch:|mcp_discover:)/i;

/** Quick check: does the response contain any actionable tag? */
export function hasToolResponse(resp: string): boolean {
    return resp.includes('<TOOL>') || resp.includes('<EXECUTE') || /<THOUGHT>/i.test(resp);
}

/** Whether the response requires multi-step tool chaining (file/native/THOUGHT). */
export function needsAgentLoop(resp: string): boolean {
    return FILE_TOOL_RE.test(resp) || NATIVE_TOOL_RE.test(resp) || /<THOUGHT>/i.test(resp);
}

/** Detects responses that combine reasoning + action (THOUGHT, or TOOL+EXECUTE). */
export function isMultiStepResponse(resp: string): boolean {
    return /<THOUGHT>/i.test(resp) || (resp.includes('<TOOL>') && resp.includes('<EXECUTE'));
}

/**
 * Foundation primitive for the upcoming table-driven dispatch: returns the
 * matched tool name (file OR native) for the FIRST tool tag in `resp`, or null.
 * File tools win ties (they're checked first), matching the loop's own order.
 * The returned name is lowercased and stripped of a trailing ':' so callers can
 * key a handler registry on it directly.
 */
export function detectToolKind(resp: string): string | null {
    const f = FILE_TOOL_RE.exec(resp);
    if (f && f[1]) return f[1].toLowerCase().replace(/:$/, '');
    const n = NATIVE_TOOL_RE.exec(resp);
    if (n && n[1]) return n[1].toLowerCase().replace(/:$/, '');
    return null;
}
