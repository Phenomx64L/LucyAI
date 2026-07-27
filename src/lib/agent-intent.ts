// ── agent-intent.ts — intent gates + exec-tag routing for runAI() ───────────
//
// Phase 0 of the runAI() de-monolithing effort, continuing the work started in
// $lib/agent-tools (v1.7.212). Where agent-tools answers "WHICH tool did the
// model ask for?", this module answers the two questions that gate whether we
// are allowed to RUN anything at all:
//
//   1. What did the USER actually want?   (code-gen / no-exec / run-order / info)
//   2. What did the MODEL emit, and which engine should run it?
//
// Every predicate here was lifted VERBATIM out of runAI() in +page.svelte.
// The regex sources are byte-identical to the originals — see
// agent-intent.test.ts, which asserts the exact source text of each pattern so
// a future edit cannot silently drift from the behaviour these tests pin.
//
// These are the highest-stakes booleans in the product: a false negative on
// `noExecIntent` runs a command the user explicitly forbade. They had zero test
// coverage before this module existed.
//
// Pure + framework-free (no Svelte, no Tauri) so it unit-tests trivially.

/**
 * `raw` reaches runAI() as `string | undefined` (auto-retry passes an empty
 * prompt). The inline originals passed that straight to `RegExp.test()`, which
 * coerces via `String()` — so `undefined` was tested as the literal `"undefined"`.
 * Normalising to `''` is equivalent for every pattern in this module (none of
 * them can match `"undefined"` or `"null"`); agent-intent.test.ts pins that.
 */
type Prompt = string | null | undefined;

const s = (raw: Prompt): string => String(raw ?? '');

// ── User-intent gates ───────────────────────────────────────────────────────

/**
 * The user wants an ARTIFACT (a script / snippet), not an execution.
 *
 * v1.7.234 — broadened. The old regex required a space right after the verb
 * ("genera␣script"), so "generaME el script" / "entrégame el script" /
 * "genérame nuevamente el script" all slipped through and were treated as
 * run-intent. Now: any generation verb (with optional enclitic -me and Spanish
 * accents) within ~40 chars of an artifact noun (script / código / powershell /
 * comando / función / .ps1).
 */
export function detectCodeGenIntent(raw: Prompt): boolean {
    const txt = s(raw);
    return /\b(dame|d[eé]me|gen[eé]r[aá](me)?|cr[eé]a(me)?|escrib[eaí](me)?|hazme|haz|necesito|quiero|p[aá]same|entr[eé]g[aá](me)?|mu[eé]stra(me)?|prepara(me)?|arma(me)?|red[aá]cta(me)?)\b.{0,40}\b(script|c[oó]digo|powershell|bash|python|\.ps1|comando|funci[oó]n)\b/i.test(txt)
        || /\b(give|write|create|generate|make|show|draft)\b.{0,30}\b(a\s+)?(script|code|powershell|command|function|\.ps1)\b/i.test(txt);
}

/**
 * The user EXPLICITLY forbids running it: "no lo ejecutes", "sin ejecutar",
 * "sólo genérame", "don't execute", "just generate".
 *
 * This is the strongest possible signal and must HARD-SUPPRESS every <EXECUTE*>
 * the model emits THIS turn — a weak local model (e.g. qwen coder) frequently
 * emits an execute block anyway, ignoring the instruction. Kept independent of
 * codeGenIntent so it fires even when the user also said "genérame el script",
 * and folded into `detectInfoIntent` below so EVERY execution gate honours it —
 * including the two (post-stream local + agent-loop) that don't check codeGen.
 */
export function detectNoExecIntent(raw: Prompt): boolean {
    const txt = s(raw);
    // v1.7.236 (audit): the reflexive-passive clitic `se` was missing, so the
    // very common Spanish phrasing "no se ejecute(n)" / "que no se ejecute" — a
    // natural way to say "don't run it" — was not recognised as no-exec intent.
    // Added `se\s+lo\s+` and `se\s+`.
    return /\bno\s+(?:se\s+lo\s+|se\s+|lo\s+|me\s+lo\s+|la\s+|las\s+|los\s+|nada\s+)?(?:ejecut\w+|corr\w+|apliqu\w+|aplic\w+|lanc\w+)/i.test(txt) ||
        /\bsin\s+(?:ejecut\w+|correr\w*|aplicar\w*|lanzar\w*)/i.test(txt) ||
        /\bno\s+quiero\s+que\s+(?:se\s+lo\s+|se\s+|lo\s+|me\s+lo\s+)?(?:ejecut\w+|corr\w+|apliqu\w+)/i.test(txt) ||
        /\b(?:s[oó]lo|solamente|[uú]nicamente|nada\s+m[aá]s|only|just|simply)\s+(?:gener\w+|escrib\w+|cr[eé]a\w*|red[aá]ct\w*|prepar\w*|write|create|generate|draft)\b/i.test(txt) ||
        /\b(?:do\s?n['’]?t|do\s+not)\s+(?:execute|run|apply|launch)\b/i.test(txt) ||
        /\bwithout\s+(?:execut\w+|running|applying|launching)/i.test(txt);
}

/**
 * The "orden previa" that RE-ENABLES execution for a generation request:
 * "ejecútalo", "córrelo", "y ejecuta", "run it", "go ahead".
 *
 * Kept precise (explicit run verbs only) — a false positive here would wrongly
 * auto-run a script the user just wanted to see. Used ONLY to let an explicit
 * order override the "generation defaults to show, not run" rule at the two
 * local-exec gates; it can never override an explicit noExecIntent (that wins
 * via infoIntent, checked first).
 */
export function detectRunRequestIntent(raw: Prompt): boolean {
    const txt = s(raw);
    return /\b(?:ejec[uú]t(?:a|alo|ala|alos|alas|enlo|enla)|c[oó]rre(?:lo|la|los|las)?|l[aá]nza(?:lo|la)?|apl[ií]ca(?:lo|la)?|hazlo|realiz(?:a|alo))\b/i.test(txt) ||
        /\by\s+(?:ejec[uú]t\w+|c[oó]rre\w+|l[aá]nza\w+|apl[ií]ca\w+)/i.test(txt) ||
        /\b(?:run|execute|launch)\s+(?:it|this|that|the)\b/i.test(txt) ||
        /\b(?:go\s+ahead|do\s+it)\b/i.test(txt);
}

/**
 * The user asks for a command to USE THEMSELVES, not to auto-execute.
 * Signals: "dame el comando", "cómo se hace", "qué comando", "muéstrame cómo".
 *
 * This guard runs REGARDLESS of what the LLM emits — it enforces show-not-run at
 * frontend level. `noExecIntent` is OR'd in so all downstream gates (which test
 * infoIntent) suppress on it too.
 *
 * NOTE the asymmetry, preserved verbatim: noExec short-circuits FIRST, so an
 * explicit "no lo ejecutes" wins even when codeGenIntent is also true; but the
 * phrase-based half is suppressed entirely when codeGenIntent is true.
 */
export function detectInfoIntent(raw: Prompt, opts?: { codeGenIntent?: boolean; noExecIntent?: boolean }): boolean {
    const txt = s(raw);
    const noExec = opts?.noExecIntent ?? detectNoExecIntent(raw);
    const codeGen = opts?.codeGenIntent ?? detectCodeGenIntent(raw);
    return noExec || (!codeGen && (
        /dame\s+(el\s+|un\s+)?comando|d[ií]me\s+(el\s+)?comando|cu[aá]l\s+es\s+el\s+comando/i.test(txt) ||
        /qu[eé]\s+comando|c[oó]mo\s+(se\s+)?hac[eo]|c[oó]mo\s+(se\s+)?ejecuta|c[oó]mo\s+puedo/i.test(txt) ||
        /para\s+(ejecutarlo|correrlo|hacerlo)\s+(yo|manual|mismo|solo|a\s+mano)/i.test(txt) ||
        /give\s+me\s+(the\s+|a\s+)?command|show\s+me\s+(how|the\s+command)|what\s+command/i.test(txt) ||
        /how\s+(do\s+I|to)\s+\w|mu[eé]strame\s+(c[oó]mo|el)/i.test(txt) ||
        /solo\s+(qu[eé]|dame|dime|mu[eé]strame)\s/i.test(txt)
    ));
}

/**
 * v1.7.49 — Belt-and-braces guard for the quick-native-tool short-circuit: even
 * when `isMultiIntentPrompt` misses a pattern (it's heuristic, not exhaustive),
 * if the user's prompt clearly asks Lucy to WRITE the result to disk, the
 * short-circuit cannot satisfy the request because none of those branches invoke
 * writefile. Force the agent loop instead.
 */
export function wantsFileOutput(raw: Prompt): boolean {
    return /\b(escritorio|desktop|guarda\s+(?:en|el|la|lo)|guardar\s+(?:en|el|la|lo|como)|deposita(?:lo)?|dep[oó]sit[aá](?:lo|melo|me)?|exporta(?:lo)?|exp[oó]rtalo|save\s+(?:as|to|it)|export(?:\s+it|\s+to)?|en\s+(?:un\s+)?archivo|to\s+(?:a\s+)?file|en\s+(?:mi\s+)?(?:carpeta|escritorio|documentos|downloads|descargas)|\.(?:md|txt|pdf|json|csv|html|docx?|xlsx?)\b)/i.test(s(raw));
}

/**
 * The four user-intent booleans a turn is gated on, resolved together so callers
 * can't accidentally compute them in the wrong order (infoIntent depends on the
 * other two).
 *
 * `skillActive` is passed in rather than read from the security-skill store:
 * v1.7.10 — when a security skill is active, every <EXECUTE> the LLM emits is
 * treated as INFORMATIONAL (rendered as a code block, not run), regardless of
 * what the user actually typed. The skill body is reference documentation; we
 * never want it executed without explicit user values.
 */
export interface TurnIntent {
    codeGenIntent: boolean;
    noExecIntent: boolean;
    runRequestIntent: boolean;
    infoIntent: boolean;
    skillInfoIntent: boolean;
}

export function classifyTurnIntent(raw: Prompt, skillActive: unknown = null): TurnIntent {
    const codeGenIntent = detectCodeGenIntent(raw);
    const noExecIntent = detectNoExecIntent(raw);
    const runRequestIntent = detectRunRequestIntent(raw);
    const infoIntent = detectInfoIntent(raw, { codeGenIntent, noExecIntent });
    return { codeGenIntent, noExecIntent, runRequestIntent, infoIntent, skillInfoIntent: !!skillActive };
}

// ── Command-shape guards ────────────────────────────────────────────────────

/**
 * LINUX-ON-WINDOWS GUARD: detect Linux-specific syntax in <EXECUTE> on Windows.
 * Applied post-response to catch cases where the LLM ignores the OS Guard prompt
 * rule.
 */
export function isLinuxCmd(cmd: string): boolean {
    return /^\s*(sudo\s|apt(-get)?\s|yum\s|dnf\s|pacman\s|brew\s|systemctl\s|journalctl|service\s+\w+\s+(start|stop|restart|status)|chmod\s|chown\s|chgrp\s|useradd\s|userdel\s|groupadd\s|passwd\s|crontab\s|bash\s+-[ce]|sh\s+-[ce]|mount\s|umount\s|fdisk\s|lsblk|mkfs\.|iptables\s|ip\s+(route|addr|link)|ufw\s|sestatus|getenforce|setenforce)/i.test(cmd.trim());
}

/**
 * Is this command read-only, i.e. safe to auto-run in PARALLEL as part of the
 * batch-execute path?
 *
 * SECURITY (phase-1 review) — removed curl|wget|find from the allowlist.
 * curl/wget are NOT read-only (they fetch attacker-controlled content and can
 * -o/-OutFile to disk → RCE staging), and POSIX `find … -delete` /
 * `find … -exec rm` is destructive on remote Linux hosts. They were being
 * auto-run in parallel with no confirm modal. (Find- = the PowerShell discovery
 * verb, kept; locate/file/wc/od are read-only.)
 */
export function isReadOnlyCmd(cmd: string): boolean {
    const ro = /^(Get-|Select-|Where-|Format-|Out-|Measure-|Test-|Find-|grep|ls|cat|head|tail|ps|top|du|df|netstat|ss|lsof|locate|file|wc|od)/i;
    return ro.test(cmd.trim());
}

// ── Response scaffolding ────────────────────────────────────────────────────

/**
 * Strip the agent scaffolding tags to decide whether the model said anything
 * user-visible at all.
 *
 * `keepExecInner` (= infoIntent || codeGenIntent || skillInfoIntent at the call
 * site) keeps the INNER content of <EXECUTE…> blocks: in those modes the block
 * was deliberately rendered as a code fence for the user, so counting it as
 * "empty" would show a confusing "empty response" warning right below a
 * perfectly visible code block.
 */
export function stripScaffolding(resp: string | null | undefined, keepExecInner: boolean): string {
    return (resp || '')
        .replace(/<THOUGHT>[\s\S]*?<\/THOUGHT>/gi, '')
        .replace(/<TOOL>[\s\S]*?<\/TOOL>/gi, '')
        .replace(/<EXECUTE[^>]*>([\s\S]*?)<\/EXECUTE[^>]*>/gi,
            (_m, inner) => keepExecInner ? String(inner || '') : '')
        .replace(/<PLAN>[\s\S]*?<\/PLAN>/gi, '')
        .replace(/<REMEMBER[^>]*>[\s\S]*?<\/REMEMBER>/gi, '')
        .replace(/<LEARN>[\s\S]*?<\/LEARN>/gi, '')
        .trim();
}

/**
 * BUG FIX (v1.4.4): suppress the empty-response warning when the raw response
 * contained ANY actionable block. Tool cards / chapter view produce visible
 * output downstream, so a missing narrative is NOT a real "empty response".
 *
 * v1.7.154 — `<EXECUTE\b` does NOT match `<EXECUTE_REMOTE>` (the `_` is a word
 * char, so there's no \b boundary after "EXECUTE"), and only `_CMD` was
 * enumerated. A reply that was ONLY a remote command (no prose) therefore looked
 * "empty" and we bailed into the fallback BEFORE the EXECUTE_REMOTE executor
 * ran. Match ANY <EXECUTE… variant.
 */
export function hadActionableBlock(resp: string | null | undefined): boolean {
    return /<TOOL>|<EXECUTE|<PLAN>|<REMEMBER\b|<LEARN>/i.test(resp || '');
}

// ── Exec-tag routing ────────────────────────────────────────────────────────

export type ExecEngine = 'cmd' | 'powershell' | 'wmic' | 'netsh' | 'reg' | 'cscript';

export interface ExecTag {
    /** Engine that will run the command. */
    type: ExecEngine;
    /** Command body, trimmed — matches the `execM[1].trim()` the executor used. */
    cmd: string;
}

/**
 * Post-stream <EXECUTE…> detection: which tag did the model emit, and which
 * engine runs it?
 *
 * Precedence is load-bearing and preserved verbatim:
 *   CMD → WMIC → NETSH → REG → CSCRIPT → PowerShell
 *
 * Two quirks worth naming, both intentional:
 *  - When the tab's engine is `cmd`, a bare <EXECUTE> is claimed by the CMD
 *    branch. When it's `powershell`, an explicit <EXECUTE_CMD> still runs
 *    through PowerShell (PS executes native commands fine).
 *  - The PowerShell branch is the only one that falls back to a fenced code
 *    block, and it will NOT do so under infoIntent — those fences are
 *    display-only output produced by the code-generation guard, and matching
 *    them would re-execute what was deliberately shown, not run.
 */
export function detectExecTag(
    safeResp: string,
    execEngine: string | null | undefined,
    infoIntent: boolean,
): ExecTag | null {
    const execCmdM = safeResp.match(/<EXECUTE_CMD>([\s\S]*?)<\/EXECUTE_CMD>/i) || (execEngine === 'cmd' ? safeResp.match(/<EXECUTE>([\s\S]*?)<\/EXECUTE>/i) : null);
    const execWmicM = safeResp.match(/<EXECUTE_WMIC>([\s\S]*?)<\/EXECUTE_WMIC>/i);
    const execNetshM = safeResp.match(/<EXECUTE_NETSH>([\s\S]*?)<\/EXECUTE_NETSH>/i);
    const execRegM = safeResp.match(/<EXECUTE_REG>([\s\S]*?)<\/EXECUTE_REG>/i);
    const execVbsM = safeResp.match(/<EXECUTE_CSCRIPT>([\s\S]*?)<\/EXECUTE_CSCRIPT>/i) || safeResp.match(/```vbs?\n([\s\S]*?)\n```/i);
    const execPsM = (!execCmdM && !execWmicM && !execNetshM && !execRegM && !execVbsM)
        ? (safeResp.match(/<EXECUTE>([\s\S]*?)<\/EXECUTE>/i) || (!infoIntent ? safeResp.match(/\`\`\`(?:powershell|ps1|bash|cmd)\n?([\s\S]*?)\n?\`\`\`/i) : null))
        : null;

    const execM = execCmdM || execWmicM || execNetshM || execRegM || execVbsM || execPsM;
    if (!execM) return null;

    const type: ExecEngine = (execCmdM && execEngine !== 'powershell') ? 'cmd'
        : execWmicM ? 'wmic'
        : execNetshM ? 'netsh'
        : execRegM ? 'reg'
        : execVbsM ? 'cscript'
        : 'powershell';

    return { type, cmd: (execM[1] ? execM[1] : '').trim() };
}

/**
 * The post-stream execution gate, verbatim:
 *
 *   execM && !infoIntent && !skillInfoIntent
 *        && !(codeGenIntent && !runRequestIntent) && !_isLinuxCmd(_postCmd)
 *
 * v1.7.234 — a pure "generate a script" request (codeGenIntent with no explicit
 * run order) defaults to show-not-run.
 */
export function shouldExecutePostStream(tag: ExecTag | null, intent: TurnIntent): boolean {
    if (!tag) return false;
    return !intent.infoIntent
        && !intent.skillInfoIntent
        && !(intent.codeGenIntent && !intent.runRequestIntent)
        && !isLinuxCmd(tag.cmd);
}
