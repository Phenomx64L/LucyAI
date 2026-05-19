// ── turn-loop.ts — Formalized troubleshooting workflow engine ────────────────
// Orchestrates: DIAGNOSE → ANALYZE → PROPOSE → APPLY → VERIFY → DONE/LOOP
// Each phase is driven by AI + command execution on the remote host.

export type TurnPhase = 'idle' | 'diagnose' | 'analyze' | 'propose' | 'apply' | 'verify' | 'done' | 'failed';

export interface TurnStep {
    phase: TurnPhase;
    timestamp: number;
    command?: string;
    output?: string;
    aiResponse?: string;
    exitCode?: number | null;
    durationMs?: number | null;
}

export interface TurnLoopState {
    id: string;
    active: boolean;
    problem: string;
    phase: TurnPhase;
    iteration: number;
    maxIterations: number;
    steps: TurnStep[];
    resolved: boolean;
    summary: string;
    startTime: number;
    hostName: string;
    hostType: string;
}

export function createTurnLoop(problem: string, hostName: string, hostType: string, maxIter = 3): TurnLoopState {
    return {
        id: `tl_${Date.now()}`,
        active: true,
        problem,
        phase: 'diagnose',
        iteration: 1,
        maxIterations: maxIter,
        steps: [],
        resolved: false,
        summary: '',
        startTime: Date.now(),
        hostName,
        hostType,
    };
}

// ── PHASE PROMPTS ───────────────────────────────────────────────────────────

export function getDiagnosePrompt(state: TurnLoopState, bootCtx: string, isEN: boolean): string {
    const lang = isEN ? 'en' : 'es';
    const prevContext = state.steps.length > 0
        ? '\n\nPREVIOUS STEPS:\n' + state.steps.map(s =>
            `[${s.phase}] ${s.command ? 'CMD: ' + s.command.substring(0, 200) : ''} ${s.output ? 'OUT: ' + s.output.substring(0, 500) : ''} ${s.aiResponse ? 'AI: ' + s.aiResponse.substring(0, 300) : ''}`
        ).join('\n')
        : '';

    return `[TURN-LOOP PHASE: DIAGNOSE — iteration ${state.iteration}/${state.maxIterations}]
Host: ${state.hostName} (${state.hostType})
${bootCtx ? 'Context: ' + bootCtx : ''}

PROBLEM REPORTED: "${state.problem}"
${prevContext}

You are an Autonomous AI System Administrator inside a DIAGNOSE phase via strict Shell. Generate ONE diagnostic command to gather information about this problem.
Rules:
- Wrap the command in <EXECUTE></EXECUTE>
- Before the command, briefly explain (1-2 lines) what you're checking and why
- To read files, use standard tools directly: 'cat /path/to/file', 'grep', 'Get-Content'. You have complete file access.
- Generate ONLY raw commands, you are already inside the remote SSH/WinRM session.
- Do NOT use ssh wrappers, do NOT use Invoke-Command.
- Language for explanations: ${lang}`;
}

export function getAnalyzePrompt(state: TurnLoopState, diagOutput: string, isEN: boolean): string {
    return `[TURN-LOOP PHASE: ANALYZE — iteration ${state.iteration}/${state.maxIterations}]
Host: ${state.hostName} (${state.hostType})

PROBLEM: "${state.problem}"

DIAGNOSTIC OUTPUT:
\`\`\`
${diagOutput.substring(0, 6000)}
\`\`\`

Analyze the diagnostic output:
1. What is the ROOT CAUSE of the problem?
2. What is the SEVERITY? (critical / high / medium / low)
3. Is additional diagnosis needed, or can you propose a fix?

Respond with a structured analysis in ${isEN ? 'English' : 'Spanish'}.
At the end, include exactly ONE of these tags:
- <VERDICT>NEEDS_MORE_DIAG</VERDICT> — if you need to run another diagnostic command
- <VERDICT>CAN_FIX</VERDICT> — if you have enough info to propose a fix
- <VERDICT>NO_ISSUE</VERDICT> — if there's actually no problem
Do NOT use <EXECUTE> in this phase.`;
}

export function getProposePrompt(state: TurnLoopState, isEN: boolean): string {
    const analysisSteps = state.steps.filter(s => s.phase === 'analyze' || s.phase === 'diagnose');
    const context = analysisSteps.map(s =>
        `[${s.phase}] ${s.command ? 'CMD: ' + s.command.substring(0, 300) : ''}\n${s.output ? 'OUT: ' + s.output.substring(0, 800) : ''}\n${s.aiResponse ? 'AI: ' + s.aiResponse.substring(0, 500) : ''}`
    ).join('\n---\n');

    return `[TURN-LOOP PHASE: PROPOSE — iteration ${state.iteration}/${state.maxIterations}]
Host: ${state.hostName} (${state.hostType})

PROBLEM: "${state.problem}"

DIAGNOSIS CONTEXT:
${context}

Propose a ZERO-TOUCH FIX for this problem. Act as an Autonomous Coding Agent. 
Rules:
1. Explain what the fix does and potential risks (2-3 lines max).
2. Wrap the fix command in <EXECUTE></EXECUTE>.
3. To rewrite/edit files, use commands like 'echo "content" > file' or 'sed -i "s/old/new/g" file' or 'Set-Content'. Provide the complete command that resolves the issue directly.
4. Generate ONLY raw commands — no ssh wrappers.
5. If the fix involves multiple steps, chain them with && or ;.
6. Automatically apply destructive fixes (restart service, modify config) if they are strictly required to resolve the problem.
7. Language: ${isEN ? 'English' : 'Spanish'}`;
}

export function getVerifyPrompt(state: TurnLoopState, fixOutput: string, isEN: boolean): string {
    const fixStep = [...state.steps].reverse().find(s => s.phase === 'apply');
    return `[TURN-LOOP PHASE: VERIFY — iteration ${state.iteration}/${state.maxIterations}]
Host: ${state.hostName} (${state.hostType})

PROBLEM: "${state.problem}"
FIX APPLIED: \`${fixStep?.command?.substring(0, 300) || 'unknown'}\`
FIX OUTPUT:
\`\`\`
${fixOutput.substring(0, 4000)}
\`\`\`

Generate ONE verification command to confirm the fix worked.
- Wrap it in <EXECUTE></EXECUTE>
- Before the command, explain what you're verifying (1 line)
- Language: ${isEN ? 'English' : 'Spanish'}`;
}

export function getResultPrompt(state: TurnLoopState, verifyOutput: string, isEN: boolean): string {
    return `[TURN-LOOP PHASE: RESULT — iteration ${state.iteration}/${state.maxIterations}]
Host: ${state.hostName} (${state.hostType})

PROBLEM: "${state.problem}"

VERIFICATION OUTPUT:
\`\`\`
${verifyOutput.substring(0, 4000)}
\`\`\`

Based on the verification output, determine:
1. Is the problem RESOLVED?
2. If not, what's still wrong?

Respond with a brief summary in ${isEN ? 'English' : 'Spanish'}.
At the end, include exactly ONE of these tags:
- <VERDICT>RESOLVED</VERDICT> — problem is fixed
- <VERDICT>NOT_RESOLVED</VERDICT> — problem persists, need another iteration
- <VERDICT>PARTIAL</VERDICT> — partially fixed, may need manual intervention`;
}

// ── HELPERS ─────────────────────────────────────────────────────────────────

export function extractCommand(response: string): string | null {
    const m = response.match(/<EXECUTE>([\s\S]*?)<\/EXECUTE>/i)
           || response.match(/```(?:powershell|ps1|batch|cmd|bash|sh)?\s*\n([\s\S]*?)\n```/i);
    return m ? m[1].trim() : null;
}

export function extractVerdict(response: string): string | null {
    const m = response.match(/<VERDICT>([\s\S]*?)<\/VERDICT>/i);
    return m ? m[1].trim().toUpperCase() : null;
}

export function cleanAiResponse(response: string): string {
    return response
        .replace(/<EXECUTE>[\s\S]*?<\/EXECUTE>/gi, '')
        .replace(/<VERDICT>[\s\S]*?<\/VERDICT>/gi, '')
        .replace(/<[^>]*>/g, '')
        .trim();
}

export function phaseLabel(phase: TurnPhase, isEN: boolean): string {
    const labels: Record<TurnPhase, [string, string]> = {
        idle:      ['Inactivo',      'Idle'],
        diagnose:  ['Diagnosticando', 'Diagnosing'],
        analyze:   ['Analizando',     'Analyzing'],
        propose:   ['Proponiendo fix','Proposing fix'],
        apply:     ['Aplicando fix',  'Applying fix'],
        verify:    ['Verificando',    'Verifying'],
        done:      ['Completado',     'Completed'],
        failed:    ['Fallido',        'Failed'],
    };
    return labels[phase]?.[isEN ? 1 : 0] || phase;
}

export function phaseIcon(phase: TurnPhase): string {
    const icons: Record<TurnPhase, string> = {
        idle: '·',
        diagnose: '◎',
        analyze: '◑',
        propose: '→',
        apply: '▶',
        verify: '✓',
        done: '✓✓',
        failed: '✗',
    };
    return icons[phase] || '↻';
}

export const PHASE_ORDER: TurnPhase[] = ['diagnose', 'analyze', 'propose', 'apply', 'verify', 'done'];

// ── STUCK DETECTION ──────────────────────────────────────────────────────
// Detects when the agent loop is repeating the same commands or making no
// progress. The detector is lightweight and stateless — it analyses the
// TurnLoopState.steps array that already exists.

export interface StuckSignal {
    isStuck: boolean;
    reason: string;       // human-readable explanation
    severity: 'none' | 'warning' | 'critical';
    repeatedCmd?: string; // the command that keeps repeating
}

/**
 * Analyse a turn-loop's step history for signs of stagnation.
 *
 * Heuristics:
 *  1. Same command executed 3+ times across iterations → stuck
 *  2. Last N commands produced identical output → no progress
 *  3. Phase regression: went backwards (e.g. verify→diagnose→verify→diagnose)
 *     more than twice → oscillating
 */
export function detectStuck(state: TurnLoopState): StuckSignal {
    const none: StuckSignal = { isStuck: false, reason: '', severity: 'none' };
    const steps = state.steps;
    if (steps.length < 3) return none;

    // 1. Repeated commands (normalised: trim + lowercase)
    const cmdCounts = new Map<string, number>();
    for (const s of steps) {
        if (!s.command) continue;
        const key = s.command.trim().toLowerCase();
        cmdCounts.set(key, (cmdCounts.get(key) || 0) + 1);
    }
    for (const [cmd, count] of cmdCounts) {
        if (count >= 3) {
            return {
                isStuck: true,
                reason: `Same command executed ${count} times without resolution`,
                severity: 'critical',
                repeatedCmd: cmd.slice(0, 120),
            };
        }
    }

    // 2. Identical consecutive outputs (last 3 command outputs)
    const outputs = steps.filter(s => s.output).map(s => s.output!.trim().slice(0, 2000));
    if (outputs.length >= 3) {
        const last3 = outputs.slice(-3);
        if (last3[0] === last3[1] && last3[1] === last3[2] && last3[0].length > 0) {
            return {
                isStuck: true,
                reason: 'Last 3 command outputs are identical — no progress',
                severity: 'warning',
            };
        }
    }

    // 3. Phase oscillation: diagnose→...→diagnose→...→diagnose (3+ diag phases)
    const diagCount = steps.filter(s => s.phase === 'diagnose').length;
    if (diagCount >= 4 && state.iteration >= 2) {
        return {
            isStuck: true,
            reason: `Excessive diagnosis cycles (${diagCount} diagnose phases) — possible oscillation`,
            severity: 'warning',
        };
    }

    return none;
}

// ── PERIODIC CHECKPOINTING ───────────────────────────────────────────────
// Saves intermediate state during the turn-loop so crashes don't lose
// progress.  Called from NexShellView after each phase completes.

export interface TurnLoopCheckpoint {
    ts: number;
    loopId: string;
    iteration: number;
    phase: TurnPhase;
    problem: string;
    hostName: string;
    stepsCount: number;
    lastCommand?: string;
    lastOutput?: string;
    stuckSignal: StuckSignal;
}

const TL_CKPT_PREFIX = 'lucy_tl_ckpt_';

export function saveTurnLoopCheckpoint(shellId: string, state: TurnLoopState): void {
    try {
        const stuck = detectStuck(state);
        const lastStep = state.steps[state.steps.length - 1];
        const ckpt: TurnLoopCheckpoint = {
            ts: Date.now(),
            loopId: state.id,
            iteration: state.iteration,
            phase: state.phase,
            problem: state.problem.slice(0, 500),
            hostName: state.hostName,
            stepsCount: state.steps.length,
            lastCommand: lastStep?.command?.slice(0, 300),
            lastOutput: lastStep?.output?.slice(0, 1000),
            stuckSignal: stuck,
        };
        localStorage.setItem(TL_CKPT_PREFIX + shellId, JSON.stringify(ckpt));
    } catch {
        // localStorage full or unavailable — non-fatal
    }
}

export function clearTurnLoopCheckpoint(shellId: string): void {
    try { localStorage.removeItem(TL_CKPT_PREFIX + shellId); } catch {}
}

export function getTurnLoopCheckpoint(shellId: string): TurnLoopCheckpoint | null {
    try {
        const raw = localStorage.getItem(TL_CKPT_PREFIX + shellId);
        return raw ? JSON.parse(raw) : null;
    } catch { return null; }
}
