// ── input-classifier.ts — Intent detection for Lucy's chat input ────────────
//
// What it does
// ------------
// Lucy's input bar receives many kinds of payloads. Treating them all as
// "send this to an LLM" wastes tokens, slows down obvious cases, and hides
// what the user really wants. This module looks at the current input value
// (and attached files, if any) and classifies the user's INTENT into one of
// a few well-defined buckets:
//
//   • 'shell-command'   → e.g. "ls /tmp", "Get-Process | Where ...". User wants execution.
//   • 'question'        → e.g. "por qué iis está lento". User wants Lucy to think.
//   • 'log-paste'       → multi-line text matching common log formats. User wants analysis.
//   • 'file-attached'   → has files in the attached list. User wants Lucy to read them.
//   • 'short-greeting'  → "hola", "hi", "thanks". Cheap chat, never agent.
//   • 'multi-intent'    → "checa X y luego haz Y". Requires agent loop.
//   • 'unknown'         → fallback when nothing matches; treat as question.
//
// Why dedicated module
// ---------------------
// The Smart Router's shell heuristic is duplicated logic. Centralizing it
// here lets:
//   1) Multiple UI parts react to intent (placeholder text, send-button
//      label, model routing, agent loop entry decision).
//   2) Tests cover all heuristics in one place.
//   3) Future ML upgrade (e.g. tiny on-device classifier) swaps the
//      implementation without rippling through callers.
//
// All checks are pure, synchronous, < 1 ms even on long inputs. No I/O.

export type Intent =
    | 'shell-command'
    | 'question'
    | 'log-paste'
    | 'file-attached'
    | 'short-greeting'
    | 'multi-intent'
    | 'code-gen'
    | 'unknown';

export interface ClassificationResult {
    intent: Intent;
    /** Short human-readable explanation (for debug/UI tooltip) */
    reason: string;
    /** Confidence in 0..1. Heuristic, not probabilistic. */
    confidence: number;
}

export interface ClassifierInput {
    /** Raw text in the input field */
    text: string;
    /** Number of files currently attached (drag-drop or paste) */
    attachedFileCount: number;
    /** Locale hint to refine keyword sets (defaults to 'es') */
    lang?: 'es' | 'en';
}

// ── Helper regexes (compiled once at module load) ────────────────────────
// Shell verbs that almost always start a real shell command. Conservative —
// false positives here would send actual questions to the shell path.
const SHELL_HEADS = new Set([
    // POSIX
    'ls', 'cd', 'pwd', 'cat', 'tail', 'head', 'grep', 'awk', 'sed', 'cp', 'mv', 'rm',
    'mkdir', 'rmdir', 'chmod', 'chown', 'find', 'ps', 'top', 'htop', 'kill', 'killall',
    'systemctl', 'service', 'journalctl', 'dmesg', 'iptables', 'firewall-cmd',
    'docker', 'docker-compose', 'kubectl', 'helm', 'minikube', 'podman',
    'git', 'svn', 'hg',
    'curl', 'wget', 'http', 'rsync', 'ssh', 'scp', 'sftp', 'nc', 'netcat',
    'tar', 'zip', 'unzip', 'gzip', 'gunzip',
    'echo', 'printf', 'export', 'env', 'set', 'unset', 'source', 'alias',
    'whoami', 'who', 'w', 'last', 'id', 'groups', 'hostname', 'uname', 'uptime',
    'df', 'du', 'free', 'lscpu', 'lsblk', 'lspci', 'lsusb',
    'netstat', 'ss', 'ip', 'ifconfig', 'ping', 'ping6', 'traceroute', 'mtr', 'dig', 'nslookup', 'host',
    // PowerShell verb-Noun (lowercased)
    'get-process', 'get-service', 'get-childitem', 'get-content', 'get-eventlog',
    'get-wmiobject', 'get-ciminstance', 'get-acl', 'get-netipaddress',
    'set-location', 'set-content', 'set-itemproperty', 'set-service',
    'stop-process', 'stop-service', 'start-service', 'restart-service', 'restart-computer',
    'invoke-command', 'invoke-restmethod', 'invoke-webrequest', 'test-connection',
    'new-item', 'remove-item', 'copy-item', 'move-item',
    'select-string', 'where-object', 'foreach-object', 'sort-object', 'group-object',
    'export-csv', 'import-csv', 'convertto-json', 'convertfrom-json',
    // Windows CMD
    'dir', 'cls', 'cd', 'copy', 'del', 'move', 'rename', 'attrib', 'tasklist', 'taskkill',
    'netsh', 'ipconfig', 'systeminfo', 'sfc', 'dism', 'chkdsk', 'reg', 'wmic',
]);

// Pipes, redirects, command substitution, backgrounding → strongly shell
const SHELL_OPS_RE = /(?:\|{1,2}|&{1,2}|>{1,2}|<{1,2}|`[^`]*`|\$\([^)]*\))/;

// Short greetings — never go to the agent loop
const GREETINGS_RE = /^\s*(hola|hi|hello|hey|buenos\s+días|buenas\s+tardes|buenas\s+noches|good\s+(morning|afternoon|evening)|thanks?|gracias|ok|okay|listo|bye|adios)[\s.!?]*$/i;

// Common log line signatures: ISO timestamp + level, syslog-style, Windows EventLog
const LOG_TIMESTAMP_RE = /(?:\d{4}-\d{2}-\d{2}[T\s]\d{2}:\d{2}:\d{2}|\w{3}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}|\[\d{2}:\d{2}:\d{2}\])/;
const LOG_LEVEL_RE = /\b(?:ERROR|ERR|WARN(?:ING)?|INFO|DEBUG|TRACE|FATAL|CRITICAL|SEVERE)\b/;
const LOG_PREFIX_RE = /^\s*(?:\[|\<)?\d/;  // many logs start with timestamp / numbers

// Multi-intent indicators: connectives that suggest sequential operations
const MULTI_INTENT_SEQ_RE = /\b(?:y\s+(?:luego|despu[eé]s|tambi[eé]n|haz|busca|verifica|comprueba|checa|investiga|consulta|compara)|luego|despu[eé]s|tras\s+eso|antes\s+(?:de\s+|checa|verifica|haz)|una\s+vez|con\s+eso|entonces|adem[aá]s|posteriormente|then|after\s+that|once\s+you|next)\b/i;

// Question indicators (ES + EN). Used as a tiebreaker when shell heuristics
// don't fire but the text has interrogative tone.
const QUESTION_WORDS_RE = /\b(?:qu[eé]|c[oó]mo|cu[aá]ndo|cu[aá]nto|d[oó]nde|por\s+qu[eé]|para\s+qu[eé]|cu[aá]l|qui[eé]n|qu[eé]\s+es|what|why|how|when|where|which|who)\b/i;

// Code-generation indicators. The user wants Lucy to WRITE code (script,
// function, program, snippet), not execute a shell command. Must route to
// the code-specialist model (e.g. qwen2.5-coder) for quality output —
// general-purpose 7-8B models hallucinate syntax and produce broken code.
//
// Pattern: imperative-verb + (script|programa|código|función|class|game|app)
// in any language tag (python, js, bash, ps1, etc.).
const CODE_GEN_VERB_RE = /\b(?:genera|escrib[ae]|crea|haz(?:me)?|dame|desarroll[ae]|impleme(?:n|nt)ta|programa|construye|c[oó]dea|generate|write|create|build|develop|implement|code|make\s+me)\b/i;
const CODE_GEN_NOUN_RE = /\b(?:script|programa|c[oó]digo|funci[oó]n|method[ao]|clase|class|app|aplicaci[oó]n|game|juego|snippet|module|m[oó]dulo|playbook|api|endpoint|crud|component|componente)\b/i;
const CODE_LANG_RE = /\b(?:python|javascript|typescript|node(?:js)?|bash|powershell|ps1|sh|c\+\+|c#|csharp|java|kotlin|swift|rust|golang|go|ruby|php|perl|sql|html|css|react|svelte|vue|django|flask|fastapi|express|next(?:js)?|pygame|tkinter|en\s+python|en\s+javascript|en\s+powershell|en\s+bash)\b/i;

// ── Internal helpers ─────────────────────────────────────────────────────
function isLikelyShell(text: string): { yes: boolean; reason?: string } {
    const trimmed = text.trim();
    if (trimmed.length === 0) return { yes: false };
    if (trimmed.length > 200) return { yes: false }; // too long to be a one-shot command

    // Strong signal: shell metacharacters
    if (SHELL_OPS_RE.test(trimmed)) {
        return { yes: true, reason: 'shell operator (|, >, &&, $(...))' };
    }
    // Strong signal: starts with known shell verb
    const firstWord = trimmed.split(/\s+/)[0]?.toLowerCase().replace(/[;,]$/, '') ?? '';
    if (SHELL_HEADS.has(firstWord)) {
        return { yes: true, reason: `starts with '${firstWord}'` };
    }
    return { yes: false };
}

function isLikelyLog(text: string): { yes: boolean; reason?: string } {
    // Must be multi-line to be a log paste (single line = comando o pregunta)
    const lines = text.split('\n').filter(l => l.trim().length > 0);
    if (lines.length < 3) return { yes: false };

    // Count how many lines match log heuristics
    let hits = 0;
    const sample = lines.slice(0, 20); // only look at first 20 lines for speed
    for (const ln of sample) {
        if (LOG_TIMESTAMP_RE.test(ln) || LOG_LEVEL_RE.test(ln) || LOG_PREFIX_RE.test(ln)) {
            hits++;
        }
    }
    // ≥60% of sampled lines look log-shaped
    const ratio = hits / sample.length;
    if (ratio >= 0.6) {
        return { yes: true, reason: `${Math.round(ratio * 100)}% lines match log patterns` };
    }
    return { yes: false };
}

function isMultiIntent(text: string): boolean {
    if (text.length < 20) return false;
    if (MULTI_INTENT_SEQ_RE.test(text)) return true;
    // Multiple imperative verbs in different clauses
    const verbCount = (text.match(/\b(?:verifica|busca|investiga|checa|chequea|consulta|compara|analiza|haz|hazlo|dame|mu[eé]strame|lista|ejecuta|corre|instala|actualiza|descarga|guarda|crea|edita|abre|env[ií]a|prueba|valida|revisa|inspecciona|detecta|search|check|verify|investigate|analyze|compare|list|run|create|edit|fetch|download|install|update)\b/gi) || []).length;
    return verbCount >= 2;
}

// ── Public API ───────────────────────────────────────────────────────────
/**
 * Classify the user's current input. Returns the best-guess intent plus a
 * reason string suitable for UI tooltips / debug logs.
 *
 * Designed to be called on every keystroke (cheap) so the UI can react in
 * real time (e.g. swap the send button label between "Run" and "Ask Lucy").
 */
export function classifyInput(input: ClassifierInput): ClassificationResult {
    const text = input.text ?? '';
    const trimmed = text.trim();

    // 1. File attached takes precedence — user explicitly added context
    if (input.attachedFileCount > 0) {
        return {
            intent: 'file-attached',
            reason: `${input.attachedFileCount} file(s) attached`,
            confidence: 0.95,
        };
    }

    // 2. Empty input
    if (trimmed.length === 0) {
        return { intent: 'unknown', reason: 'empty', confidence: 1.0 };
    }

    // 3. Greeting — cheapest path, never wakes the agent
    if (GREETINGS_RE.test(trimmed)) {
        return { intent: 'short-greeting', reason: 'matches greeting pattern', confidence: 0.95 };
    }

    // 4. Log paste — multi-line with log-shape ratio ≥60%
    const logCheck = isLikelyLog(trimmed);
    if (logCheck.yes) {
        return { intent: 'log-paste', reason: logCheck.reason ?? 'log-shaped lines', confidence: 0.85 };
    }

    // 4.5. Code generation — "genera un script en python", "escribe función JS"
    // Must come BEFORE shell detection: "crea" is sometimes a shell verb, but
    // "crea un script en python" clearly wants generated code, not execution.
    // Signal strength = verb + (noun OR language). Either combination wins.
    const hasGenVerb = CODE_GEN_VERB_RE.test(trimmed);
    const hasCodeNoun = CODE_GEN_NOUN_RE.test(trimmed);
    const hasLang     = CODE_LANG_RE.test(trimmed);
    if (hasGenVerb && (hasCodeNoun || hasLang)) {
        return {
            intent: 'code-gen',
            reason: hasLang ? 'code-gen verb + language' : 'code-gen verb + noun',
            confidence: 0.88,
        };
    }
    // Strong standalone signal: explicit language mention with imperative tone
    // (e.g. "snake en python") even when verb is implicit.
    if (hasLang && hasCodeNoun) {
        return { intent: 'code-gen', reason: 'language + code noun', confidence: 0.80 };
    }

    // 5. Shell command — verb or metachar
    const shellCheck = isLikelyShell(trimmed);
    if (shellCheck.yes) {
        return { intent: 'shell-command', reason: shellCheck.reason ?? 'shell pattern', confidence: 0.85 };
    }

    // 6. Multi-intent — sequential connectives or multiple verbs
    if (isMultiIntent(trimmed)) {
        return { intent: 'multi-intent', reason: 'sequential connectives detected', confidence: 0.75 };
    }

    // 7. Question — interrogative words
    if (QUESTION_WORDS_RE.test(trimmed) || trimmed.endsWith('?') || trimmed.endsWith('¿')) {
        return { intent: 'question', reason: 'interrogative pattern', confidence: 0.80 };
    }

    // 8. Fallback: short → greeting-ish, long → question
    if (trimmed.length <= 30) {
        return { intent: 'short-greeting', reason: 'short input default', confidence: 0.4 };
    }
    return { intent: 'unknown', reason: 'no strong signals', confidence: 0.3 };
}

/**
 * Human label for each intent, suitable for the send-button or status hint.
 * Returns Spanish/English variant based on lang.
 */
export function intentLabel(intent: Intent, lang: 'es' | 'en' = 'es'): string {
    if (lang === 'en') {
        switch (intent) {
            case 'shell-command':  return 'Run command';
            case 'question':       return 'Ask Lucy';
            case 'log-paste':      return 'Analyze log';
            case 'file-attached':  return 'Read files';
            case 'short-greeting': return 'Send';
            case 'multi-intent':   return 'Plan & execute';
            case 'code-gen':       return 'Generate code';
            default:               return 'Send';
        }
    }
    switch (intent) {
        case 'shell-command':  return 'Ejecutar';
        case 'question':       return 'Preguntar a Lucy';
        case 'log-paste':      return 'Analizar log';
        case 'file-attached':  return 'Leer archivos';
        case 'short-greeting': return 'Enviar';
        case 'multi-intent':   return 'Planear & ejecutar';
        case 'code-gen':       return 'Generar código';
        default:               return 'Enviar';
    }
}

/**
 * Dynamic placeholder text for the input bar. Reacts to recent input or
 * the active context (e.g. focused on a remote host).
 */
export function placeholderForContext(opts: {
    isRemoteHost?: boolean;
    hostName?: string;
    lang?: 'es' | 'en';
    lastIntent?: Intent;
}): string {
    const en = opts.lang === 'en';
    if (opts.isRemoteHost && opts.hostName) {
        return en
            ? `Command on ${opts.hostName} or question for Lucy...`
            : `Comando en ${opts.hostName} o pregunta para Lucy...`;
    }
    if (opts.lastIntent === 'log-paste') {
        return en
            ? 'Drop more logs or ask about the previous one...'
            : 'Pega más logs o pregunta sobre el anterior...';
    }
    return en
        ? 'Type a command, paste a log or drag a file...'
        : 'Escribe un comando, pega un log o arrastra un archivo...';
}
