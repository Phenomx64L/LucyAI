// ── llm-stream.ts — LLM streaming, tool detection, and tag parsing ──────────
// Pure/near-pure utilities extracted from +page.svelte's runAI() function.
// No Svelte imports. Tauri invoke/listen are injected via opts for testability.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// ── Constants ────────────────────────────────────────────────────────────────

/** Maximum agent loop iterations before forced stop */
export const MAX_AGENT_LOOPS = 25;

/** Maximum identical tool calls before abort (anti-loop) */
export const MAX_IDENTICAL_TOOL_CALLS = 3;

/** Token drain interval for progressive reveal (ms) */
export const DRAIN_MS = 30;

/** Tool regex for file/agent tools that enter the agent loop */
export const FILE_TOOL_RE = /<TOOL>(readfile|readlines|writefile|listdir|searchfiles|editfile|locate_file|start_indexer|analyze_code|mcp_query|graphify|memoria_guardar|memoria_buscar|memoria_eliminar|memoria_consolidar|memory_core_set|memory_core_delete|fork_task|wait_task|cd|pdf_search|principle_set|principle_delete|schedule_create|schedule_list):/i;

/** Tool regex for native tools that may short-circuit */
export const NATIVE_TOOL_RE = /<TOOL>(sysinfo|netconn|tasklist|eventlog:|registry:|system_diff:|search_runbooks:|search_web:|semantic:|fetch:|mcp_discover:)/i;

// ── Tag types emitted by the LLM ────────────────────────────────────────────

export type LucyTagType =
    | 'EXECUTE' | 'EXECUTE_CMD' | 'EXECUTE_WMIC' | 'EXECUTE_NETSH'
    | 'EXECUTE_REG' | 'EXECUTE_CSCRIPT' | 'EXECUTE_REMOTE'
    | 'TOOL' | 'THOUGHT' | 'LEARN' | 'REMEMBER' | 'PLAN'
    | 'FILECONTENT';

// ── cleanStreamDisplay ───────────────────────────────────────────────────────
// Strips all action tags from LLM output for visual display during streaming.
// When codeGenIntent=true, EXECUTE tags are shown as code blocks instead of hidden.

export function cleanStreamDisplay(text: string, codeGenIntent: boolean): string {
    let cleaned = codeGenIntent
        ? text
            .replace(/<EXECUTE>([\s\S]*?)(?:<\/EXECUTE>|$)/gi, (_, c) => '\n```powershell\n' + c.trim() + '\n```\n')
            .replace(/<EXECUTE_CMD>([\s\S]*?)(?:<\/EXECUTE_CMD>|$)/gi, (_, c) => '\n```cmd\n' + c.trim() + '\n```\n')
        : text
            .replace(/<EXECUTE>[\s\S]*?(?:<\/EXECUTE>|$)/gi, '')
            .replace(/<EXECUTE_CMD>[\s\S]*?(?:<\/EXECUTE_CMD>|$)/gi, '');

    cleaned = cleaned
        .replace(/<EXECUTE_WMIC>[\s\S]*?(?:<\/EXECUTE_WMIC>|$)/gi, '')
        .replace(/<EXECUTE_NETSH>[\s\S]*?(?:<\/EXECUTE_NETSH>|$)/gi, '')
        .replace(/<EXECUTE_REG>[\s\S]*?(?:<\/EXECUTE_REG>|$)/gi, '')
        .replace(/<EXECUTE_CSCRIPT>[\s\S]*?(?:<\/EXECUTE_CSCRIPT>|$)/gi, '')
        .replace(/<LEARN>[\s\S]*?(?:<\/LEARN>|$)/gi, '')
        // EXECUTE_REMOTE carries a `target="<host-id>"` attribute (see RULE 14
        // in prompt_sections.rs HostRoutingSection), so the opening tag is
        // NOT a bare `<EXECUTE_REMOTE>`. The `[\s\S]*?` after the tag name
        // (NOT after `>`) lets the matcher accept anything until the next
        // `<` — which is the start of the close tag — or end-of-string in
        // the streaming-mid-tag case. Do NOT "fix" this to require a `>`
        // right after EXECUTE_REMOTE; that breaks attribute-bearing remotes
        // and leaks `<EXECUTE_REMOTE target="...">...` into the chat UI.
        .replace(/<EXECUTE_REMOTE[\s\S]*?(?:<\/EXECUTE_REMOTE>|$)/gi, '')
        .replace(/<REMEMBER[^>]*>[\s\S]*?(?:<\/REMEMBER>|$)/gi, '')
        .replace(/<TOOL>[\s\S]*?(?:<\/TOOL>|$)/gi, '')
        // v1.7.78 — Extended Thinking visible.
        // Instead of deleting <THOUGHT>...</THOUGHT> blocks (the model's
        // internal reasoning), convert them to a collapsed <details>
        // block so the operator can OPT-IN to see Lucy's reasoning trace
        // — matching the Claude/Gemini/o3 "extended thinking" pattern.
        //
        // Why <details>: native HTML disclosure widget, zero JS, zero
        // reactive cost, works with markdown render and DOMPurify pass-
        // through (HTML5 details + summary are in the safe default list).
        //
        // Mid-stream case: the trailing `(?:<\/THOUGHT>|$)` form matches
        // both closed and unclosed tags. While streaming, the inner text
        // appears progressively inside the collapsed widget so the user
        // can pop it open to watch reasoning unfold token-by-token —
        // the exact UX Anthropic ships in Claude.ai.
        //
        // Blank line padding around `$1` so marked() treats the inner
        // text as markdown (bullets, fences, links all work).
        .replace(/<THOUGHT>([\s\S]*?)(?:<\/THOUGHT>|$)/gi,
                 (_, inner) => `\n<details class="lucy-thought">\n<summary>💭 Razonando…</summary>\n\n${inner.trim()}\n\n</details>\n`)
        .replace(/<FILECONTENT>[\s\S]*?(?:<\/FILECONTENT>|$)/gi, '')
        .replace('__TRUNCATED__', '')
        .trim();

    return cleaned;
}

// ── detectCodeGenIntent ──────────────────────────────────────────────────────
// Returns true if the user's message is asking for code/scripts without execution.

export function detectCodeGenIntent(text: string): boolean {
    if (!text) return false;
    return /dame\s+(un\s+)?script|escrib[ea]\s+(un\s+)?script|crea\s+(un\s+)?script|genera\s+(un\s+)?script|give\s+me\s+(a\s+)?script|write\s+(a\s+)?script|create\s+(a\s+)?script|generate\s+(a\s+)?script|hazme\s+(un\s+)?script|necesito\s+(un\s+)?script|quiero\s+(un\s+)?script|dame\s+.*c[oó]digo|dame\s+.*powershell|haz\s+.*script/i.test(text);
}

// ── hasToolResponse ──────────────────────────────────────────────────────────
// Quick check if LLM response contains actionable tags.

export function hasToolResponse(resp: string): boolean {
    return resp.includes('<TOOL>') || resp.includes('<EXECUTE') || /<THOUGHT>/i.test(resp);
}

// ── needsAgentLoop ───────────────────────────────────────────────────────────
// Whether the response requires multi-step tool chaining.

export function needsAgentLoop(resp: string): boolean {
    return FILE_TOOL_RE.test(resp) || NATIVE_TOOL_RE.test(resp) || /<THOUGHT>/i.test(resp);
}

// ── isMultiStepResponse ──────────────────────────────────────────────────────
// Detects responses that have both THOUGHT + TOOL/EXECUTE (complex reasoning).

export function isMultiStepResponse(resp: string): boolean {
    return /<THOUGHT>/i.test(resp) || (resp.includes('<TOOL>') && resp.includes('<EXECUTE'));
}

// ── extractTags ──────────────────────────────────────────────────────────────
// Parse all structured tags from an LLM response. Returns matched tags with
// their type, content, and attributes (for EXECUTE_REMOTE, PLAN, etc.)

export interface ParsedTag {
    type:    LucyTagType;
    content: string;
    attrs:   Record<string, string>;
    raw:     string;
}

export function extractTags(text: string): ParsedTag[] {
    const tags: ParsedTag[] = [];
    if (!text) return tags;

    // EXECUTE variants
    const execRe = /<(EXECUTE|EXECUTE_CMD|EXECUTE_WMIC|EXECUTE_NETSH|EXECUTE_REG|EXECUTE_CSCRIPT)>([\s\S]*?)<\/\1>/gi;
    let m: RegExpExecArray | null;
    while ((m = execRe.exec(text)) !== null) {
        tags.push({ type: m[1].toUpperCase() as LucyTagType, content: m[2].trim(), attrs: {}, raw: m[0] });
    }

    // EXECUTE_REMOTE with target attribute
    const remoteRe = /<EXECUTE_REMOTE\s+target=["']([^"']+)["']>([\s\S]*?)<\/EXECUTE_REMOTE>/gi;
    while ((m = remoteRe.exec(text)) !== null) {
        tags.push({ type: 'EXECUTE_REMOTE', content: m[2].trim(), attrs: { target: m[1] }, raw: m[0] });
    }

    // TOOL tags
    const toolRe = /<TOOL>([\s\S]*?)<\/TOOL>/gi;
    while ((m = toolRe.exec(text)) !== null) {
        tags.push({ type: 'TOOL', content: m[1].trim(), attrs: {}, raw: m[0] });
    }

    // THOUGHT
    const thoughtRe = /<THOUGHT>([\s\S]*?)<\/THOUGHT>/gi;
    while ((m = thoughtRe.exec(text)) !== null) {
        tags.push({ type: 'THOUGHT', content: m[1].trim(), attrs: {}, raw: m[0] });
    }

    // LEARN
    const learnRe = /<LEARN>([\s\S]*?)<\/LEARN>/gi;
    while ((m = learnRe.exec(text)) !== null) {
        tags.push({ type: 'LEARN', content: m[1].trim(), attrs: {}, raw: m[0] });
    }

    // REMEMBER
    const rememberRe = /<REMEMBER\s*([^>]*)>([\s\S]*?)<\/REMEMBER>/gi;
    while ((m = rememberRe.exec(text)) !== null) {
        const attrStr = m[1] || '';
        const catMatch = attrStr.match(/category=["']([^"']+)["']/i);
        const attrs: Record<string, string> = {};
        if (catMatch) attrs.category = catMatch[1];
        tags.push({ type: 'REMEMBER', content: m[2].trim(), attrs, raw: m[0] });
    }

    // FILECONTENT (always paired with writefile TOOL)
    const fcRe = /<FILECONTENT>([\s\S]*?)<\/FILECONTENT>/gi;
    while ((m = fcRe.exec(text)) !== null) {
        tags.push({ type: 'FILECONTENT', content: m[1], attrs: {}, raw: m[0] });
    }

    return tags;
}

// ── parseTool ────────────────────────────────────────────────────────────────
// Parse a TOOL tag content into name + arguments.
// e.g. "readfile:/path/to/file" → { name: "readfile", args: "/path/to/file" }
// e.g. "editfile:/path|||old|||new" → { name: "editfile", args: "/path|||old|||new" }

export interface ParsedTool {
    name: string;
    args: string;
}

export function parseTool(content: string): ParsedTool {
    const colonIdx = content.indexOf(':');
    if (colonIdx === -1) return { name: content.trim(), args: '' };
    return {
        name: content.substring(0, colonIdx).trim(),
        args: content.substring(colonIdx + 1),
    };
}

// ── Tool deduplication hash ──────────────────────────────────────────────────
// Detects when the agent is stuck calling the same tool repeatedly.

export function toolHash(toolContent: string): string {
    // Simple hash: first 200 chars of the tool call normalized
    return toolContent.trim().toLowerCase().slice(0, 200);
}

export function isToolLooping(
    hash: string,
    hashCounts: Map<string, number>,
    maxIdentical: number = MAX_IDENTICAL_TOOL_CALLS,
): boolean {
    const count = (hashCounts.get(hash) || 0) + 1;
    hashCounts.set(hash, count);
    return count > maxIdentical;
}

// ── StreamOpts ───────────────────────────────────────────────────────────────
// Configuration for the streaming function. Allows injection of state callbacks.

export interface StreamOpts {
    getTab:     (id: string) => any;
    refresh:    () => void;
}

// ── askLucyStream ────────────────────────────────────────────────────────────
// Invokes ask_lucy_stream and calls onChunk with accumulated text in real-time.
// Uses rAF-throttled chunk dispatch for smooth rendering at 60fps.
// Returns the complete response text.

const _activeStreams = new Map<string, { cancelled: boolean; unlisten: (() => void) | null }>();

export function cancelStream(tabId: string): void {
    const stream = _activeStreams.get(tabId);
    if (stream) {
        stream.cancelled = true;
        if (stream.unlisten) stream.unlisten();
        _activeStreams.delete(tabId);
    }
}

export function isStreaming(tabId: string): boolean {
    return _activeStreams.has(tabId);
}

export async function askLucyStream(
    params: Record<string, any>,
    onChunk: (accumulated: string) => void,
    tabId: string | null,
    opts?: StreamOpts,
): Promise<string> {
    const requestId = `req_${Date.now()}_${Math.random().toString(36).slice(2)}`;
    let accumulated = '';
    const streamState = { cancelled: false, unlisten: null as (() => void) | null };
    const t0 = performance.now();
    let ttft = 0;

    // rAF-throttled chunk dispatch: coalesces ~100 chunks/sec into 60fps updates
    let _rafScheduled = false;
    let _pendingChunk = false;
    // v1.7.190 — fallback timer handle (see the chunk listener below). rAF is
    // paused by the webview when the window is unfocused/occluded; the timer is
    // the safety net that keeps streaming text rendering in the background.
    let _fallbackTimer: ReturnType<typeof setTimeout> | null = null;
    let _lastTpsAt = 0;
    let _cachedTps = 0;
    // v1.7.175 — decouple the expensive markdown re-render from the rAF rate.
    // onChunk() re-parses the FULL accumulated text (marked + DOMPurify) on
    // every call; at 60 fps that grows ~O(N²) over a long response and is the
    // main source of streaming jank. Cap the render cadence to ~12 fps — still
    // smooth to the eye, ~5× fewer parses. The cheap telemetry (TTFT/TPS +
    // refresh) stays at the full rAF rate so the t/s chip keeps its live feel.
    let _lastRenderAt = 0;
    const RENDER_THROTTLE_MS = 80;

    const flushChunk = (force = false) => {
        _rafScheduled = false;
        if (!_pendingChunk && !force) return;
        _pendingChunk = false;
        const nowPerf = performance.now();
        if (nowPerf - _lastTpsAt > 500) {
            const elapsed = (nowPerf - t0) / 1000;
            _cachedTps = elapsed > 0 ? Math.round((accumulated.length / 4) / elapsed) : 0;
            _lastTpsAt = nowPerf;
        }
        if (tabId && opts) {
            const tt = opts.getTab(tabId);
            if (tt) {
                tt._streamTTFT = Math.round(ttft);
                tt._streamTPS = _cachedTps;
                opts.refresh();
            }
        }
        // Expensive path (full markdown re-parse) — throttled. The final flush
        // passes force=true so the complete message always renders even if the
        // last chunk landed inside the throttle window.
        if (force || nowPerf - _lastRenderAt >= RENDER_THROTTLE_MS) {
            _lastRenderAt = nowPerf;
            onChunk(accumulated);
        }
    };

    // Register listener BEFORE invoke to not lose initial chunks
    const unlisten = await listen(`lucy-chunk-${requestId}`, (event: any) => {
        if (streamState.cancelled) return;
        if (!ttft) ttft = performance.now() - t0;
        accumulated += event.payload;
        _pendingChunk = true;
        if (!_rafScheduled) {
            _rafScheduled = true;
            // v1.7.190 — schedule the flush via rAF AND a setTimeout fallback.
            // When Lucy's window is unfocused/occluded (the user clicks the
            // Windows taskbar), the WebView2/Chromium compositor throttles or
            // fully PAUSES requestAnimationFrame, so an rAF-only flush never
            // runs and the streaming text freezes mid-write until a mouse event
            // forces a repaint ("la información desaparece hasta pasar el
            // mouse"). A timer keeps firing in the background (Chromium clamps
            // it to ~1s when the page is hidden, full rate when merely
            // unfocused), so the DOM keeps updating. flushChunk is idempotent
            // (it clears _pendingChunk and no-ops without one), so whichever
            // path fires first wins and the other is a cheap no-op.
            // Wrap so rAF's timestamp arg isn't passed as `force` (truthy →
            // would defeat the render throttle).
            const _run = () => {
                if (_fallbackTimer !== null) { clearTimeout(_fallbackTimer); _fallbackTimer = null; }
                flushChunk();
            };
            // Skip the rAF entirely when the page is hidden — it would never
            // fire and the queued callbacks would pile up until refocus.
            if (typeof document === 'undefined' || document.visibilityState !== 'hidden') {
                requestAnimationFrame(_run);
            }
            _fallbackTimer = setTimeout(_run, 200);
        }
    });
    streamState.unlisten = unlisten;
    if (tabId) _activeStreams.set(tabId, streamState);

    try {
        const result = await invoke('ask_lucy_stream', { ...params, requestId });
        // Final flush (force) so the complete message renders even if the last
        // rAF was inside the render-throttle window.
        flushChunk(true);
        if (streamState.cancelled) return accumulated || '';
        return result as string;
    } catch (e) {
        if (streamState.cancelled) return accumulated || '';
        throw e;
    } finally {
        unlisten();
        if (_fallbackTimer !== null) { clearTimeout(_fallbackTimer); _fallbackTimer = null; }
        if (tabId) _activeStreams.delete(tabId);
    }
}

// ── isSensitiveRegistry ──────────────────────────────────────────────────────
// Security check: prevents reading sensitive registry paths.

const SENSITIVE_REG_PATHS = [
    /\\SAM\\/i,
    /\\SECURITY\\/i,
    /\\Credentials/i,
    /\\Secrets/i,
    /\\PuTTY\\Sessions/i,
    /WinLogon.*DefaultPassword/i,
];

export function isSensitiveRegistry(keyPath: string): boolean {
    return SENSITIVE_REG_PATHS.some(re => re.test(keyPath));
}

// ── Context building helpers ─────────────────────────────────────────────────

/** Builds the code generation safety protocol injected into context */
export function buildCodeProtocol(): string {
    return `
[CODE GENERATION PROTOCOL]:
- If the user asks for code, scripts, or commands (PowerShell, SQL, Python, bash, etc):
  1. GENERATE the code with full, untruncated content
  2. DO NOT wrap code in <EXECUTE> tags unless user explicitly asks to "run", "execute", or "test"
  3. DO NOT attempt to automatically execute, install dependencies, or elevate privileges
  4. DO NOT try "auto-fix" - ask the user first if they want help fixing errors
  5. Provide the code clearly formatted and ready for user to copy/paste

- If user asks for installation (Install-Module, apt-get, pip, etc):
  DO NOT execute these automatically. Explain what to run and ask for permission first.

- If you need to use <EXECUTE>: ONLY if user explicitly says "run", "execute", "test it", or "check if..."
- Always ask before attempting privilege elevation (RunAs, sudo, etc.)

[SYNTHESIS PROTOCOL — MANDATORY when the user asks for analysis]:
If the user's request contains ANY of: "correlaciona", "correlate", "diagnostica", "diagnose",
"dime si", "tell me if", "explain why", "analiza", "analyze", "compara", "compare",
"resumen", "summary", "reporte", "report", "qué pasó", "what happened", "is the bottleneck",
"cuál es la causa", "root cause", "consolidad", "consolidat", "dame los que estén" —
you MUST end your final turn with a NARRATIVE response in Markdown that:
  1. Synthesizes the findings from ALL gathered tool outputs (do not just list raw data).
  2. Answers the user's specific question DIRECTLY (e.g. "the bottleneck is X because Y").
  3. Provides a concrete recommendation or next-step suggestion when applicable.
NEVER end such a task with only "Operations completed" or "Pide explícame los cambios".

[PDF GENERATION — full path mandatory]:
When generating PDFs via Edge Headless, NEVER call \`msedge\` as a bare command (it is not in PATH).
Use ONE of these patterns instead:
  • Full path: & 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe' --headless --disable-gpu --print-to-pdf="OUT.pdf" "file:///INPUT.html"
  • Discovery first: Use <TOOL>locate_file:msedge.exe</TOOL> to find the executable, then quote the full path with the call operator (&).
  • Fallback: If Edge is missing, try Chrome's full path: 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe'.
`;
}

// ── Token queue for progressive reveal ───────────────────────────────────────

export interface TokenDrainState {
    queue:      string[];
    revealed:   string;
    prevAccLen: number;
    timer:      ReturnType<typeof setInterval> | null;
}

export function createTokenDrain(): TokenDrainState {
    return { queue: [], revealed: '', prevAccLen: 0, timer: null };
}

export function enqueueChunk(state: TokenDrainState, accumulated: string): void {
    const newText = accumulated.substring(state.prevAccLen);
    state.prevAccLen = accumulated.length;
    if (newText) state.queue.push(newText);
}

/** Drain one batch from queue into revealed. Returns true if text was revealed. */
export function drainBatch(state: TokenDrainState): boolean {
    if (state.queue.length === 0) return false;
    // Adaptive: drain faster when queue builds up
    const batch = state.queue.length > 40 ? Math.ceil(state.queue.length / 3)
                : state.queue.length > 15 ? 4 : 1;
    for (let i = 0; i < batch && state.queue.length > 0; i++) {
        state.revealed += state.queue.shift()!;
    }
    return true;
}

/** Flush all remaining tokens immediately */
export function flushDrain(state: TokenDrainState): void {
    if (state.queue.length > 0) {
        state.revealed += state.queue.join('');
        state.queue = [];
    }
    if (state.timer) {
        clearInterval(state.timer);
        state.timer = null;
    }
}
