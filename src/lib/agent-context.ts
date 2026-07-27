// ── agent-context.ts — the values runAI() READS from its environment ────────
//
// Phase 3 of the runAI() de-monolithing effort (v1.7.239), after the AgentHost
// port in $lib/agent-host.
//
// Why a second port
// -----------------
// Phase 1 named runAI()'s WRITES: every addMsg / invoke / toast / fin now goes
// through `host.*`. That covered the effects, but a function is not portable
// until BOTH directions are named, and runAI() still reaches directly into the
// component for its INPUTS — `lucyConfig`, `userLang`, `activeTabId`,
// `mcpServers`, the cockpit flag. 71 such reads remain.
//
// Those reads are exactly why runAI() cannot be called from anywhere else yet:
// a headless caller (the task scheduler, the OpenClaw gateway) has no component
// to read them from. It has a language and a config of its own, no active tab,
// and no cockpit at all. Naming the reads lets it supply them.
//
// GETTERS, NOT A SNAPSHOT — this is the whole correctness argument
// ----------------------------------------------------------------
// Every member below is a getter, and the production binding in +page.svelte
// implements them as `get x() { return theComponentVar; }`. That is not a
// stylistic choice.
//
// A turn is long: the agent loop can run for minutes across many model calls.
// During it, `_sessionSpendUsd` climbs with every cloud call, `mcpServers` can
// be reloaded, `hostName` changes when the operator switches host. If this port
// were a plain object built at call time, runAI() would read the values frozen
// at turn START — and the spend cap, which compares the LIVE total against the
// limit, would never fire because it would keep re-reading the total from
// before the turn spent anything.
//
// Getters preserve the existing semantics exactly: each access re-reads the
// live variable, which is what `lucyConfig.name` did before. Same as Phase 1's
// thin arrows — the indirection is named, the timing is untouched.
//
// What is deliberately NOT here
// -----------------------------
// Only values runAI() reads and never writes. Verified per symbol; three
// candidates were excluded because runAI() mutates them, and a read port is the
// wrong home for state the function owns:
//
//   tabs           reassigned (the Svelte reactivity trigger)
//   forkedTasks    mutated by index — `forkedTasks[id].status = 'done'`
//   auditAlerts    incremented — `auditAlerts++`
//   contextUsed    assigned
//   _pendingPlans  assigned
//
// Svelte STORE reads ($hosts, $runbooks, $ollamaOnline — 14 sites) are also
// left for a later phase. Store access is compiled auto-subscription, so moving
// it needs more care than a variable rename and does not belong in a phase
// whose whole claim is that it is mechanical.

/**
 * Lucy's user-level configuration. The real object carries more; only what
 * runAI() reads is named, with an index signature keeping the rest addressable
 * (same approach as AgentTab in $lib/agent-host).
 */
export interface AgentConfig {
    /** Operator's display name — becomes `userName` in every model call. */
    name: string;
    /** Directory of company runbooks, or null/'' when unset. */
    runbooksDir?: string | null;
    /** Terse-answer mode: prefixes the prompt with a brevity instruction. */
    briefMode?: boolean;
    /** Router picks the model per turn instead of honouring the dropdown. */
    smartRouting?: boolean;
    /** Hard-lock every LLM call to local Ollama. */
    privacyMode?: boolean;
    /** Demote borderline prompts to the fast tier. */
    economyMode?: boolean;
    [key: string]: any;
}

/**
 * Everything runAI() reads from its environment.
 *
 * Every member is a GETTER — see the note at the top of this file. A phase being
 * migrated can declare the narrow slice it needs (`Pick<AgentContext, 'lang'>`)
 * rather than taking the whole port, same as AgentHost.
 */
export interface AgentContext {
    // ── Operator configuration ──────────────────────────────────────────────
    /** User config: name, runbooks dir, brief/privacy/economy/smart-routing. */
    readonly config: AgentConfig;
    /** UI language tag ('es' / 'en'), passed to every model call as `lang`. */
    readonly lang: string;
    /** Answer verbosity: 'concise' | 'balanced' | 'detailed'. */
    readonly personality: string;

    // ── Model selection ─────────────────────────────────────────────────────
    /** Model id for spawned sub-agents (fork_task), '' = cheapest available. */
    readonly subAgentModel: string;
    /** Verifier pass setting: 'off' when no second model reviews the answer. */
    readonly verifierMode: string;

    // ── Session / environment ───────────────────────────────────────────────
    /** Active remote host label, '---' when none is selected. */
    readonly hostName: string;
    /** Id of the tab the operator is looking at — NOT necessarily the tab the
     *  turn runs in, which is why runAI() takes `tabId` separately. */
    readonly activeTabId: string | number | null;
    /** Cloud spend so far this session, in USD. Climbs DURING a turn — the
     *  spend cap compares against this, so it must be read live. */
    readonly sessionSpendUsd: number;

    // ── Integrations ────────────────────────────────────────────────────────
    /** Configured MCP servers. */
    readonly mcpServers: any[];
    /** MCP secret values, keyed by name. */
    readonly mcpSecrets: Record<string, any>;

    // ── Feature flags ───────────────────────────────────────────────────────
    /** Whether the 2.0 cockpit shell is the active UI. A headless caller sets
     *  this false: there is no cockpit to mirror anything into. */
    readonly cockpitUi: boolean;
}

// ── Test doubles ────────────────────────────────────────────────────────────

/**
 * A context with neutral defaults, for testing migrated phases without a
 * component. Override only what the test cares about:
 *
 *   const ctx = createTestContext({ lang: 'en', config: { name: 'Ada' } });
 *
 * Defaults mirror the component's own initial values (see +page.svelte): smart
 * routing OFF, no host selected, cockpit ON.
 */
export function createTestContext(overrides: Partial<AgentContext> = {}): AgentContext {
    const base: AgentContext = {
        config: { name: '', runbooksDir: null, briefMode: false, smartRouting: false, privacyMode: false, economyMode: false },
        lang: 'es',
        personality: 'balanced',
        subAgentModel: '',
        verifierMode: 'off',
        hostName: '---',
        activeTabId: null,
        sessionSpendUsd: 0,
        mcpServers: [],
        mcpSecrets: {},
        cockpitUi: true,
    };
    return { ...base, ...overrides };
}

/**
 * A context whose values can be changed after creation, for testing the
 * live-read semantics that plain overrides cannot express.
 *
 * Returns the context plus a `set` function. Use it when a test needs a value to
 * CHANGE mid-turn — the spend cap is the motivating case: it only fires because
 * the total it reads keeps climbing while the loop runs.
 *
 *   const { ctx, set } = createMutableTestContext({ sessionSpendUsd: 0 });
 *   set({ sessionSpendUsd: 9.5 });   // ctx.sessionSpendUsd is now 9.5
 */
export function createMutableTestContext(
    initial: Partial<AgentContext> = {},
): { ctx: AgentContext; set: (patch: Partial<AgentContext>) => void } {
    const state = createTestContext(initial) as Record<string, any>;
    const ctx = {} as Record<string, any>;
    for (const key of Object.keys(state)) {
        Object.defineProperty(ctx, key, {
            get: () => state[key],
            enumerable: true,
        });
    }
    return {
        ctx: ctx as AgentContext,
        set: (patch) => { Object.assign(state, patch); },
    };
}
