// ── agent-host.ts — the port runAI() talks to the outside world through ─────
//
// Phase 1 of the runAI() de-monolithing effort (v1.7.239), after the
// characterization net in $lib/agent-intent.
//
// The problem this solves
// -----------------------
// runAI() is ~4.8k lines and its UI mutation is INTERLEAVED with its logic, not
// wrapped around it: 72 `invoke`, 35 `addMsg`, 19 `refresh`, 28 `fin` calls
// spread through every phase. There is no pure core to lift out — which is why
// "extract the logic, leave the UI" cannot work as a single move.
//
// So we invert it. Instead of pulling logic out of the component, we give the
// component's capabilities a NAME and a TYPE. Once every side effect in runAI()
// goes through `host.*`, a phase can move into a headless module unchanged: it
// keeps calling `host.addMsg(...)`, and only the object behind `host` differs
// between the chat UI and a headless caller.
//
// This module is deliberately TYPES + TEST DOUBLES ONLY. It contains no
// behaviour, so wiring it up cannot change how the app runs — the default host
// in +page.svelte binds the component's existing functions unchanged.
//
// Relationship to $lib/headless-agent
// -----------------------------------
// headless-agent runs a bounded, read-only loop for the task scheduler and does
// not need a host at all (it has no UI to drive). This port is the wider
// contract the FULL agent needs; the two converge once enough phases have moved.

/**
 * A chat tab, as far as the host contract is concerned. The real object carries
 * far more (attachments, model selection, exec engine, streaming bookkeeping…);
 * only what crosses the port is named here, and the index signature keeps the
 * rest addressable without forcing a premature full typing of the tab.
 */
export interface AgentTab {
    id: string | number;
    messages: any[];
    isProcessing?: boolean;
    execEngine?: string;
    selectedModel?: string;
    [key: string]: any;
}

/** A message as pushed by `addMsg`. `html` is sanitized by the host, not here. */
export interface AgentMessage {
    role: string;
    html?: string;
    style?: string;
    rawRole?: string;
    rawContent?: string;
    id?: string | number;
    [key: string]: any;
}

/** Minimal toast surface — the subset of svelte-sonner the agent path uses. */
export interface AgentToast {
    success(msg: string): void;
    error(msg: string): void;
    info(msg: string): void;
    warning(msg: string): void;
}

/** A command awaiting elevated / destructive confirmation (the RunAs modal). */
export interface RunAsRequest {
    cmd: string;
    ctx: string;
    doSpeak: boolean;
    tabId: string | number;
    isDestructive?: boolean;
}

/** A command stopped by the security blocklist, awaiting explicit authorization. */
export interface SecurityBlockRequest {
    tabId: string | number;
    cmd: string;
    ctx: string;
    doSpeak: boolean;
    blockWord: string;
    displayCmd: string;
    execType: string;
    token: string | null;
}

/**
 * A learned command/answer pair awaiting the operator's confirmation before it
 * is written to memory (the "aprende esto" flow).
 *
 * The third HITL halt, added in Phase 4. Phase 1 caught confirmRunAs and
 * confirmSecurityBlock but missed this one — it still assigned the pending
 * state and raised the modal inline, which meant a headless caller reaching
 * this branch would have set component variables that, for it, do not exist.
 */
export interface LearnRequest {
    /** Trigger phrases the operator wants Lucy to recognise later. */
    claves: string[];
    /** The command/script to associate with them. */
    script: string;
    /** The canned answer to give. */
    respuesta: string;
    tabId: string | number;
    doSpeak: boolean;
}

/**
 * Everything runAI() needs from its environment.
 *
 * Grouped by concern so a phase being migrated can declare the narrow slice it
 * actually depends on (e.g. `Pick<AgentHost, 'addMsg' | 'refresh'>`) rather than
 * taking the whole port.
 */
export interface AgentHost {
    // ── Chat surface ────────────────────────────────────────────────────────
    /** Append a message to a tab. Sanitizes `html` and mirrors to the cockpit. */
    addMsg(tabId: string | number, msg: AgentMessage): void;
    /** Show the "thinking" placeholder bubble for a tab. */
    addThinking(tabId: string | number): void;
    /** Scroll the transcript to the newest message. */
    scrollChat(): Promise<void> | void;
    /** Finalize a turn: sweep placeholders, clear processing state, mirror the reply. */
    fin(tabId: string | number): Promise<void> | void;

    // ── Reactivity / tab bookkeeping ───────────────────────────────────────
    /** Resolve a tab by id. Returns undefined once a tab has been closed. */
    getTab(tabId: string | number): AgentTab | undefined;
    /** Global re-render — reassigns the tabs array to trigger Svelte reactivity. */
    refresh(): void;
    /**
     * GRANULAR re-render for a single tab. Much cheaper than `refresh()` during
     * streaming, where a global invalidation re-renders every tab per token.
     * Prefer this inside hot loops.
     */
    bumpTab(tabId: string | number): void;

    // ── Notifications ───────────────────────────────────────────────────────
    toast: AgentToast;
    /** Text-to-speech for the reply, when the turn was voice-initiated. */
    speak(text: string): void;

    // ── Human-in-the-loop confirmation ─────────────────────────────────────
    /**
     * Hand a destructive / elevation-requiring command to the RunAs modal and
     * STOP. The turn does not continue — the user's answer resumes it.
     */
    confirmRunAs(req: RunAsRequest): void;
    /**
     * Hand a blocklisted command to the authorization panel and STOP, same
     * contract as confirmRunAs.
     */
    confirmSecurityBlock(req: SecurityBlockRequest): void;
    /**
     * Hand a learned command/answer pair to the confirmation modal and STOP,
     * same contract as the two above: the caller `fin()`s and returns, and the
     * operator's answer is what commits it to memory.
     */
    confirmLearn(req: LearnRequest): void;

    // ── Telemetry ───────────────────────────────────────────────────────────
    logTaskEvent(
        eventType: string,
        subtype: string,
        elapsedMs: number | null,
        metadata: Record<string, unknown> | null,
        tabId: string | number,
    ): void;

    // ── Backend ─────────────────────────────────────────────────────────────
    /** Tauri IPC. The single widest dependency in runAI() — 72 call sites. */
    invoke<T = any>(cmd: string, args?: Record<string, unknown>): Promise<T>;
}

// ── Test doubles ────────────────────────────────────────────────────────────

/** One recorded host interaction, in call order. */
export interface HostCall {
    method: string;
    args: any[];
}

export interface RecordingHost extends AgentHost {
    /** Every host call in order — the assertion surface for migrated phases. */
    calls: HostCall[];
    /** Calls to one method, in order. */
    callsTo(method: string): HostCall[];
    /** Messages passed to addMsg, in order. */
    messages(): AgentMessage[];
    /** Tabs the double knows about, keyed by id. */
    tabs: Map<string | number, AgentTab>;
}

/**
 * Overrides accepted by `createRecordingHost`.
 *
 * `invoke` is widened to a concrete `Promise<any>` return: the real port declares
 * it generic (`invoke<T>`) so production call sites can pin a result type, but a
 * test script like `async (cmd) => cmd === 'x' ? 'RAM: 32GB' : null` cannot
 * satisfy "returns whatever T the caller asks for". Widening here keeps the
 * production contract strict while letting tests write the obvious thing.
 */
export type AgentHostOverrides =
    Partial<Omit<AgentHost, 'invoke'>>
    & { invoke?: (cmd: string, args?: Record<string, unknown>) => Promise<any> };

/**
 * A host that records everything and does nothing, for testing migrated phases
 * without a DOM or a Tauri backend.
 *
 * `invoke` resolves to `undefined` unless an override is supplied — override it
 * per test to script backend responses:
 *
 *   const host = createRecordingHost({
 *       invoke: async (cmd) => cmd === 'get_system_health' ? 'RAM: 32GB' : null,
 *   });
 */
export function createRecordingHost(overrides: AgentHostOverrides = {}): RecordingHost {
    const calls: HostCall[] = [];
    const tabs = new Map<string | number, AgentTab>();
    const rec = (method: string) => (...args: any[]) => { calls.push({ method, args }); };

    const base: AgentHost = {
        addMsg: (tabId, msg) => {
            calls.push({ method: 'addMsg', args: [tabId, msg] });
            const t = tabs.get(tabId);
            if (t) t.messages.push(msg);
        },
        addThinking: rec('addThinking'),
        scrollChat: rec('scrollChat'),
        fin: rec('fin'),
        getTab: (tabId) => {
            calls.push({ method: 'getTab', args: [tabId] });
            return tabs.get(tabId);
        },
        refresh: rec('refresh'),
        bumpTab: rec('bumpTab'),
        toast: {
            success: (m) => { calls.push({ method: 'toast.success', args: [m] }); },
            error: (m) => { calls.push({ method: 'toast.error', args: [m] }); },
            info: (m) => { calls.push({ method: 'toast.info', args: [m] }); },
            warning: (m) => { calls.push({ method: 'toast.warning', args: [m] }); },
        },
        speak: rec('speak'),
        confirmRunAs: rec('confirmRunAs'),
        confirmSecurityBlock: rec('confirmSecurityBlock'),
        confirmLearn: rec('confirmLearn'),
        logTaskEvent: rec('logTaskEvent'),
        invoke: async (cmd, args) => {
            calls.push({ method: 'invoke', args: [cmd, args] });
            return undefined as any;
        },
    };

    const host = { ...base, ...overrides } as RecordingHost;

    // Wrap any override so it is recorded too — otherwise a test that scripts
    // `invoke` loses the call log for exactly the interaction it cares about.
    for (const key of Object.keys(overrides) as (keyof AgentHostOverrides)[]) {
        const fn = (overrides as any)[key];
        if (typeof fn !== 'function') continue;
        (host as any)[key] = (...args: any[]) => {
            calls.push({ method: key as string, args });
            return fn(...args);
        };
    }

    host.calls = calls;
    host.tabs = tabs;
    host.callsTo = (method: string) => calls.filter((c) => c.method === method);
    host.messages = () => calls.filter((c) => c.method === 'addMsg').map((c) => c.args[1]);
    return host;
}

/** Convenience for tests: register a tab the double will return from getTab. */
export function seedTab(host: RecordingHost, tab: Partial<AgentTab> & { id: string | number }): AgentTab {
    const full: AgentTab = { messages: [], isProcessing: false, ...tab };
    host.tabs.set(full.id, full);
    return full;
}
