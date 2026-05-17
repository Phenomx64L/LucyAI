// ── smart-router.ts — Heuristic model routing for Lucy Gen 2 ───────────────
//
// Reduces user friction: instead of picking a model in the dropdown every
// turn, Lucy auto-selects based on the prompt's apparent complexity. The
// user keeps a hard-override dropdown for cases where they know better.
//
// Routing tiers (priority order — first match wins):
//
//   1. FORCED MANUAL OVERRIDE
//      If the user picked something explicitly in the dropdown AND has not
//      enabled "Smart routing" in settings → respect their choice.
//
//   2. PRIVACY MODE
//      If `lucyConfig.privacyMode === true` → always Ollama local. Never
//      route to cloud, regardless of complexity. (Compliance use case:
//      government/healthcare operators who can't send data offsite.)
//
//   3. SHELL-INTENT (cheap & fast)
//      Short prompt (<60 chars) that LOOKS like a shell command (starts
//      with verb-noun pattern or contains `|`, `>`, `&&`) → fastest
//      available local Ollama model (if online) or Gemini Flash.
//
//   4. ANALYSIS-INTENT (heavyweight)
//      Prompt contains security/audit keywords OR has >800 tokens of
//      context → Claude Opus or Sonnet (best reasoning).
//      Examples: "audit", "compliance", "review", "vulnerability",
//      "root cause", "correlate", "explain why".
//
//   5. DEFAULT
//      Gemini 3 Flash — best speed/cost/quality balance for everyday
//      SysAdmin chat.
//
// All decisions are EXPLAINABLE: every call returns a `reason` string so
// the UI can show a small badge ("via Gemini Flash · routed by length").

/**
 * Metadata for a locally-available Ollama model. Lucy uses this to pick the
 * right model for each task: small/fast for greetings + shell commands, large
 * for analysis, code-tuned for programming questions, vision-capable for
 * image attachments.
 *
 * `sizeBillion` and `capabilities` are best-effort inferred from the model
 * name. Override them only if your model is misnamed (e.g. a 14B custom
 * fine-tune of an 8B base).
 */
export interface LocalModelInfo {
    /** Ollama model id WITHOUT the `local-` prefix (e.g. "qwen2.5-coder:14b") */
    id: string;
    /** Parameter size in billions (parsed from name; defaults to 7) */
    sizeBillion?: number;
    /** Task affinities inferred from name. `text-weak` flags pure vision models
     *  (e.g. moondream) that hallucinate on text-only prompts and must be
     *  filtered out unless the need is explicitly `vision`. */
    capabilities?: Array<'code' | 'vision' | 'general' | 'reasoning' | 'text-weak'>;
}

// ── Model metadata inference (pure helpers, no allocations beyond the result)
const SIZE_RE = /(\d+(?:\.\d+)?)\s*[bB]\b/;
const CODE_RE = /\b(coder?|deepseek-coder|starcoder|codellama|codestral|codeqwen|wizardcoder)\b/i;
const VISION_RE = /\b(vl|vision|llava|moondream|cogvlm|qwen.?vl|pixtral)\b/i;
const REASONING_RE = /\b(r1|reasoner|qwq|reflection|o1|nemotron|gpt-oss)\b/i;

// Vision-only or vision-broken models that produce garbage OR crash on
// text-only prompts. These MUST be excluded for text-only needs.
//
//   • moondream → returns gibberish ("!!! [Luna]!!!")
//   • plain llava → mirrors prompt back ("Quien te cree, Luna?")
//   • qwen2.5vl / qwen-vl / cogvlm / pixtral → crash with
//     `GGML_ASSERT(a->ne[2] * 4 == b->ne[0]) failed` in Ollama when no image
//     is attached (model expects vision tensors)
//
// `llava-llama3`, `llava-phi`, `llava-mistral` are NOT flagged — they wrap
// proper text LLM bases and handle text-only prompts fine.
const VISION_ONLY_RE = /^(moondream|llava(?!-llama|-phi|-mistral)|qwen[\d.]*[-]?vl|cogvlm|pixtral)/i;

// Known model size hints for names that don't expose a `:7b`-style suffix.
// Order matters: longest-prefix match wins. Sizes are in billions of params.
const MODEL_SIZE_HINTS: Array<[RegExp, number]> = [
    [/^moondream\b/i,           1.8],
    [/^llava-llama3\b/i,        8],
    [/^llava-phi\b/i,           3.8],
    [/^llava\b/i,               7],
    [/^phi3?:mini\b/i,          3.8],
    [/^tinyllama\b/i,           1.1],
    [/^gemma:?2b\b/i,           2],
    [/^gemma\b/i,               7],
    [/^mistral\b/i,             7],
    [/^qwen2\.5vl\b/i,          7],
    [/^gpt-oss\b/i,             20],
];

function parseModelSize(name: string): number {
    const m = SIZE_RE.exec(name);
    if (m) {
        const n = parseFloat(m[1]);
        if (Number.isFinite(n) && n > 0) return n;
    }
    // Fall back to known-model lookup before defaulting
    for (const [re, size] of MODEL_SIZE_HINTS) {
        if (re.test(name)) return size;
    }
    return 7; // sensible default for unknown names
}

function inferCapabilities(name: string): Array<'code' | 'vision' | 'general' | 'reasoning' | 'text-weak'> {
    const caps: Array<'code' | 'vision' | 'general' | 'reasoning' | 'text-weak'> = ['general'];
    if (CODE_RE.test(name)) caps.unshift('code');           // unshift → code-first sort
    if (VISION_RE.test(name)) caps.unshift('vision');
    if (REASONING_RE.test(name)) caps.unshift('reasoning');
    // Mark models that perform poorly on text-only prompts. Used as an
    // EXCLUSION filter in pickLocalForNeed when the task is not vision.
    if (VISION_ONLY_RE.test(name)) caps.push('text-weak');
    return caps;
}

/**
 * Enrich a bare model id with size + capability metadata. Idempotent: if the
 * input already has size/capabilities set, returns as-is.
 */
export function enrichLocalModel(raw: string | LocalModelInfo): LocalModelInfo {
    if (typeof raw === 'string') {
        return {
            id: raw,
            sizeBillion: parseModelSize(raw),
            capabilities: inferCapabilities(raw),
        };
    }
    return {
        id: raw.id,
        sizeBillion: raw.sizeBillion ?? parseModelSize(raw.id),
        capabilities: raw.capabilities ?? inferCapabilities(raw.id),
    };
}

export interface RoutingContext {
    /** Raw user prompt for this turn */
    prompt: string;
    /** Approximate token count of the FULL context being sent (history + prompt + system) */
    contextTokens: number;
    /** Whether Ollama is reachable right now */
    ollamaOnline: boolean;
    /**
     * ALL locally-available Ollama models (enriched). Optional — when omitted
     * the router falls back to `primaryLocalModel` for backward compatibility.
     * When provided, the router picks the best one for each task tier:
     *   - Tier 3 (shell, greeting) → smallest available
     *   - Tier 4 (analysis, log)   → largest available
     *   - Tier 5 (default fast)    → smallest available
     *   - Vision intent            → vision-capable model preferred
     *   - Code intent              → code-tuned model preferred
     */
    localModels?: LocalModelInfo[];
    /** Legacy single-model field. Kept for backward compat with old callers. */
    primaryLocalModel?: string;
    /** User's hard override from the dropdown (model id) */
    manualOverride?: string;
    /** If true, hard-respects manualOverride. If false, advisory only. */
    smartRoutingEnabled: boolean;
    /** Privacy mode forces local routing regardless of other factors */
    privacyMode?: boolean;
    /**
     * Optional pre-computed intent from input-classifier.ts. When provided,
     * the router uses it for tier 3/4 decisions INSTEAD of its internal
     * heuristics — the classifier handles multi-line logs, file attachments,
     * greetings, etc. that the router can't detect on its own.
     *
     * Accepted values mirror the Intent type from input-classifier.ts:
     *   'shell-command' | 'question' | 'log-paste' | 'file-attached'
     *   | 'short-greeting' | 'multi-intent' | 'unknown'
     *
     * Backward-compat: when omitted, the router falls back to its built-in
     * shell-pattern heuristic so legacy callers keep working unchanged.
     */
    detectedIntent?:
        | 'shell-command'
        | 'question'
        | 'log-paste'
        | 'file-attached'
        | 'short-greeting'
        | 'multi-intent'
        | 'code-gen'
        | 'unknown';
}

export interface RoutingDecision {
    /** Model id to use (e.g. "gemini-3.1-flash-lite", "claude-opus-4-7", "local-llama3.2") */
    modelId: string;
    /** Short human-readable reason for diagnostics + UI badge */
    reason: string;
    /** Tier number 1-5 matching the doc above */
    tier: 1 | 2 | 3 | 4 | 5;
    /** True if this is an automatic decision; false if user override */
    autoSelected: boolean;
}

// ── Tier 3: shell-intent detection ────────────────────────────────────────
// Verbs typical of shell commands at the START of an input. Conservative
// list — we only want shell-intent when very confident, since false positives
// would route a normal question to local Ollama (poor for chat).
const SHELL_VERBS = new Set([
    'ls', 'cd', 'cat', 'tail', 'head', 'grep', 'awk', 'sed', 'cp', 'mv', 'rm',
    'mkdir', 'rmdir', 'chmod', 'chown', 'find', 'ps', 'top', 'htop', 'kill',
    'systemctl', 'service', 'journalctl', 'docker', 'kubectl', 'git',
    'curl', 'wget', 'ssh', 'scp', 'rsync', 'tar', 'zip', 'unzip',
    'echo', 'export', 'env', 'whoami', 'hostname', 'uptime', 'df', 'du',
    'netstat', 'ss', 'ip', 'ifconfig', 'ping', 'traceroute', 'dig', 'nslookup',
    'get-process', 'get-service', 'stop-process', 'start-service',
    'invoke-command', 'test-connection', 'get-childitem', 'set-location',
]);

const SHELL_OPS_RE = /[|><&]{1,2}|\$\(|`/;  // shell operators: pipes, redirects, subshells

function looksLikeShellCommand(prompt: string): boolean {
    if (prompt.length === 0 || prompt.length > 60) return false;
    const first = prompt.trim().split(/\s+/)[0]?.toLowerCase() ?? '';
    if (SHELL_VERBS.has(first)) return true;
    if (SHELL_OPS_RE.test(prompt)) return true;
    return false;
}

// ── Tier 4: analysis-intent keyword detection ────────────────────────────
// These keywords STRONGLY suggest the user wants deep reasoning, not a
// quick command. We accept some false positives here — sending a simple
// chat to Opus is wasteful but not broken; downgrading complex analysis
// to Flash is broken because the reasoning quality matters.
const HEAVY_KEYWORDS = [
    // security / compliance
    'audit', 'compliance', 'vulnerability', 'cve', 'cwe', 'cis',
    'hardening', 'pentest', 'forensic', 'incident',
    // analysis / reasoning
    'analyze', 'analyse', 'analiza', 'correlate', 'correlaciona',
    'root cause', 'causa raíz', 'causa raiz', 'diagnose', 'diagnostica',
    'explain why', 'explain how', 'compare', 'compara',
    'design', 'architecture', 'arquitectura', 'review', 'revisa',
    // multi-step planning
    'plan', 'strategy', 'estrategia', 'migrate', 'migra', 'refactor',
];
const HEAVY_RE = new RegExp(
    '\\b(' + HEAVY_KEYWORDS.map(k => k.replace(/\s/g, '\\s')).join('|') + ')\\b',
    'i'
);

// ── Model selection helpers ──────────────────────────────────────────────
type LocalNeed = 'fast' | 'reasoning' | 'code' | 'vision';

/**
 * Pick the BEST local model for a given task need from the available list.
 * Strategy:
 *   • vision/code → filter by capability first, then size-rank
 *   • fast        → smallest available (under N B params if possible)
 *   • reasoning   → largest available (better quality for analysis)
 * Skips any model currently in the failure blacklist.
 *
 * Returns the model id (with `local-` prefix added) or null when no usable
 * local model exists.
 */
function pickLocalForNeed(ctx: RoutingContext, need: LocalNeed): string | null {
    if (!ctx.ollamaOnline) return null;

    // Build a working list — prefer the enriched array, fall back to the
    // legacy single-string field for backward compat.
    let available: LocalModelInfo[] = [];
    if (ctx.localModels && ctx.localModels.length > 0) {
        available = ctx.localModels.map(enrichLocalModel);
    } else if (ctx.primaryLocalModel) {
        available = [enrichLocalModel(ctx.primaryLocalModel)];
    }
    if (available.length === 0) return null;

    // Skip blacklisted models
    available = available.filter(m => !isModelBlacklisted(`local-${m.id}`));
    if (available.length === 0) return null;

    // EXCLUDE text-weak (vision-only) models from text needs. Moondream and
    // similar tiny vision models return gibberish on plain text prompts —
    // never route a greeting/question/log to them just because they're small.
    if (need !== 'vision') {
        const textCapable = available.filter(m => !m.capabilities?.includes('text-weak'));
        if (textCapable.length > 0) available = textCapable;
        // If ALL remaining models are text-weak we still try (better than null)
    }

    // Capability filter — gracefully fall back to general if no specialty match
    if (need === 'vision') {
        const visionModels = available.filter(m => m.capabilities?.includes('vision'));
        if (visionModels.length > 0) available = visionModels;
    } else if (need === 'code') {
        const codeModels = available.filter(m => m.capabilities?.includes('code'));
        if (codeModels.length > 0) available = codeModels;
    } else if (need === 'reasoning') {
        // Prefer reasoning-tuned models when explicitly asked for analysis.
        const reasoningModels = available.filter(m => m.capabilities?.includes('reasoning'));
        if (reasoningModels.length > 0) available = reasoningModels;
    }

    // Size rank: ascending for fast, descending for reasoning, balanced otherwise
    available.sort((a, b) => {
        const sa = a.sizeBillion ?? 7;
        const sb = b.sizeBillion ?? 7;
        if (need === 'reasoning') return sb - sa; // largest first
        if (need === 'fast')      return sa - sb; // smallest first
        return sa - sb; // default: smallest first (cost/latency aware)
    });
    return `local-${available[0].id}`;
}

/**
 * Convenience wrapper: pick a sane default local model when the caller
 * doesn't have a specific intent to express. Used by tiers that don't carry
 * task-specific signal (e.g. Privacy Mode lock).
 */
function bestLocalModel(ctx: RoutingContext): string | null {
    return pickLocalForNeed(ctx, 'fast');
}

/**
 * Map the classifier's intent to the local-model need vocabulary. Vision
 * intent is approximated via file-attached + image extension hint (caller
 * passes that signal — keep router pure).
 */
function needForIntent(intent: RoutingContext['detectedIntent']): LocalNeed {
    switch (intent) {
        case 'shell-command':  return 'fast';
        case 'short-greeting': return 'fast';
        case 'question':       return 'fast';
        case 'log-paste':      return 'reasoning';
        case 'multi-intent':   return 'reasoning';
        case 'file-attached':  return 'reasoning';
        case 'code-gen':       return 'code';       // route to qwen2.5-coder etc.
        default:               return 'fast';
    }
}

const HEAVY_MODEL_DEFAULT = 'claude-sonnet-4-6';  // strong reasoning, less $ than Opus
const HEAVY_MODEL_PEAK    = 'claude-opus-4-7';    // flagship — for extreme contexts (>3000 tokens)
const FAST_MODEL_DEFAULT  = 'gemini-3.1-flash-lite';  // May 2026 cheapest frontier — $0.25/$1.50 per M tok

// ── Recent-failure tracking ──────────────────────────────────────────────
// Models that fail (provider 5xx, "model runner crashed", quota errors, etc)
// get a 5-minute timeout. The router skips them and falls back to the next
// tier so the user isn't stuck looping into a broken model.
//
// Real example: Ollama returns HTTP 500 "model runner has unexpectedly
// stopped" when it OOMs. Without this map, the router keeps trying Ollama
// every turn and the user sees the same error repeatedly.
const RECENT_FAILURES = new Map<string, number>();  // modelId → expiry epoch ms
const FAILURE_TTL_MS = 5 * 60 * 1000;

/**
 * Tell the router that a model just failed. The router will avoid routing
 * to this model for FAILURE_TTL_MS milliseconds. Call from `runAI`'s catch
 * block (or from the streaming wrapper) when the provider returns an error.
 */
export function recordModelFailure(modelId: string): void {
    if (!modelId) return;
    RECENT_FAILURES.set(modelId, Date.now() + FAILURE_TTL_MS);
    // Opportunistic cleanup of expired entries to keep map small
    if (RECENT_FAILURES.size > 16) {
        const now = Date.now();
        for (const [k, exp] of RECENT_FAILURES) {
            if (exp < now) RECENT_FAILURES.delete(k);
        }
    }
}

/** True if the model has failed within the last FAILURE_TTL_MS. */
function isModelBlacklisted(modelId: string): boolean {
    const exp = RECENT_FAILURES.get(modelId);
    if (!exp) return false;
    if (exp < Date.now()) {
        RECENT_FAILURES.delete(modelId);
        return false;
    }
    return true;
}

/**
 * Public introspection — useful for debug/devtools to see which models
 * are temporarily unavailable. Returns a map of modelId → millis remaining.
 */
export function getBlacklistedModels(): Record<string, number> {
    const now = Date.now();
    const out: Record<string, number> = {};
    for (const [k, exp] of RECENT_FAILURES) {
        if (exp > now) out[k] = exp - now;
    }
    return out;
}

/** Clear all blacklisted entries — for tests or manual recovery. */
export function clearFailureBlacklist(): void {
    RECENT_FAILURES.clear();
}

/**
 * Given a failed routing decision, compute the next tier to try.
 * Falls in priority order:
 *   tier 2/3 (local/privacy) → 5 (Gemini Flash, cloud fast)
 *   tier 4 (heavy)           → 5 (downgrade to fast)
 *   tier 5 (fast)            → 1 (manual override) or null (give up)
 *
 * Returns null when there's no sane fallback left.
 */
export function nextFallback(failed: RoutingDecision, ctx: RoutingContext): RoutingDecision | null {
    // Tier 2/3 used local → fall to cloud fast (unless privacy is enforced)
    if (failed.tier === 2 && ctx.privacyMode) return null; // privacy hard-locks local
    if (failed.tier === 2 || failed.tier === 3) {
        return {
            modelId: FAST_MODEL_DEFAULT,
            reason: `fallback from tier ${failed.tier} (local failed) → cloud fast`,
            tier: 5,
            autoSelected: true,
        };
    }
    // Tier 4 (heavy) failed → downgrade to default fast
    if (failed.tier === 4) {
        return {
            modelId: FAST_MODEL_DEFAULT,
            reason: `fallback from tier 4 (heavy failed) → cloud fast`,
            tier: 5,
            autoSelected: true,
        };
    }
    // Tier 5 (default fast) failed → try manual override if any
    if (failed.tier === 5 && ctx.manualOverride && ctx.manualOverride !== failed.modelId) {
        return {
            modelId: ctx.manualOverride,
            reason: `fallback from tier 5 (cloud fast failed) → manual override`,
            tier: 1,
            autoSelected: false,
        };
    }
    return null;
}

// ── Public API ───────────────────────────────────────────────────────────
/**
 * Decide which model to use for the current turn.
 * Pure function — does not perform any I/O.
 */
export function routeModel(ctx: RoutingContext): RoutingDecision {
    // ── Tier 2 FIRST (Privacy Mode is a HARD lock) ──────────────────────
    // BUG FIX: Privacy Mode used to be checked AFTER the manual-override
    // shortcut, so users with Smart Routing OFF + Privacy ON kept hitting
    // their dropdown model (often Gemini Flash) instead of local Ollama.
    // Privacy Mode now wins over ALL other tiers — toggling it should always
    // route to local, period. If no local model is available, we DO NOT
    // silently fall back to cloud (that would violate the user's intent);
    // instead we mark the decision tier 2 and let the caller's UI surface
    // an actionable error: "Privacy mode is on but no local model is reachable".
    if (ctx.privacyMode) {
        // Pick the BEST local model for the task at hand — use the classifier's
        // intent if available (smallest for shell/greeting, largest for analysis).
        // This is how Lucy actually leverages multiple Ollama models the user
        // has cached locally instead of always defaulting to the first one.
        const need = needForIntent(ctx.detectedIntent);
        const local = pickLocalForNeed(ctx, need);
        if (local) {
            return {
                modelId: local,
                reason: `privacy mode → local (need=${need}, hard lock)`,
                tier: 2,
                autoSelected: true,
            };
        }
        // Local unavailable OR all blacklisted. We still return a tier-2
        // decision with a CLEAR reason — runAI sees tier === 2 + privacyMode
        // and refuses to swap to cloud (compliance lock).
        return {
            modelId: ctx.manualOverride ?? FAST_MODEL_DEFAULT,
            reason: 'privacy mode active but NO local model reachable — check Ollama is running or all candidates are in failure backoff',
            tier: 2,
            autoSelected: true,
        };
    }

    // ── Tier 1: manual override (when smart routing is OFF) ─────────────
    if (!ctx.smartRoutingEnabled && ctx.manualOverride) {
        return {
            modelId: ctx.manualOverride,
            reason: 'manual selection',
            tier: 1,
            autoSelected: false,
        };
    }

    // Tier 3: shell-intent → smallest local model if available, else flash.
    // Prefer the precomputed intent (richer classifier) when present;
    // otherwise fall back to the router's built-in heuristic.
    const isShellByIntent = ctx.detectedIntent === 'shell-command';
    const isShellByHeuristic = !ctx.detectedIntent && looksLikeShellCommand(ctx.prompt);
    if (isShellByIntent || isShellByHeuristic) {
        // 'fast' bias → smallest, cheapest local model. Even on machines with
        // many models cached, a `ls /tmp` shouldn't wake gpt-oss:20b.
        const local = pickLocalForNeed(ctx, 'fast');
        const reasonPrefix = isShellByIntent ? 'shell (classifier)' : 'shell command';
        if (local) {
            return { modelId: local, reason: `${reasonPrefix} → local (smallest)`, tier: 3, autoSelected: true };
        }
        if (!isModelBlacklisted(FAST_MODEL_DEFAULT)) {
            return { modelId: FAST_MODEL_DEFAULT, reason: `${reasonPrefix} → fast (no local available)`, tier: 3, autoSelected: true };
        }
    }

    // Tier 3b: short-greeting → fast tier, skip heavy/local computation entirely.
    // Cheap chat doesn't deserve Claude Sonnet OR a local model warmup.
    if (ctx.detectedIntent === 'short-greeting') {
        if (!isModelBlacklisted(FAST_MODEL_DEFAULT)) {
            return { modelId: FAST_MODEL_DEFAULT, reason: 'greeting → fast', tier: 5, autoSelected: true };
        }
    }

    // Tier 4: analysis-intent → heavyweight model.
    // Both the classifier's signals (log-paste, multi-intent, file-attached)
    // AND the router's keyword heuristics qualify for heavy reasoning.
    const intentNeedsHeavy =
        ctx.detectedIntent === 'log-paste' ||
        ctx.detectedIntent === 'multi-intent' ||
        ctx.detectedIntent === 'file-attached';
    const heavyMatch = HEAVY_RE.exec(ctx.prompt);
    const veryLargeContext = ctx.contextTokens > 3000;
    if (intentNeedsHeavy || heavyMatch || veryLargeContext || ctx.contextTokens > 800) {
        const peak = veryLargeContext;
        const heavyId = peak ? HEAVY_MODEL_PEAK : HEAVY_MODEL_DEFAULT;
        if (!isModelBlacklisted(heavyId)) {
            return {
                modelId: heavyId,
                reason: peak
                    ? `large context (${ctx.contextTokens} tokens) → deep reasoning`
                    : intentNeedsHeavy
                        ? `${ctx.detectedIntent} → analysis tier`
                        : heavyMatch
                            ? `analysis keyword '${heavyMatch[1].toLowerCase()}'`
                            : `context >800 tokens`,
                tier: 4,
                autoSelected: true,
            };
        }
        // Heavy model blacklisted — fall through to fast tier
    }

    // Tier 5: default — fast everyday model. If even this is blacklisted,
    // hand back the manual override as a last resort.
    if (!isModelBlacklisted(FAST_MODEL_DEFAULT)) {
        return {
            modelId: FAST_MODEL_DEFAULT,
            reason: 'default fast tier',
            tier: 5,
            autoSelected: true,
        };
    }
    return {
        modelId: ctx.manualOverride || FAST_MODEL_DEFAULT,
        reason: 'all auto-tiers blacklisted — using manual override',
        tier: 1,
        autoSelected: false,
    };
}

// ── Cheap utility: rough token estimator ─────────────────────────────────
/**
 * Estimate token count without loading a real tokenizer. Good enough for
 * the routing threshold (within ±15% for typical English/Spanish prose).
 * Use the backend's tiktoken for precise counts when accuracy matters.
 */
export function estimateTokens(text: string): number {
    if (!text) return 0;
    // Conservative heuristic: 1 token ≈ 4 chars in English/Spanish prose,
    // tighter for code (≈3 chars). Use the lower bound to bias toward
    // upgrading to heavier models for borderline cases.
    return Math.ceil(text.length / 3.6);
}
