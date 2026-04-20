// ── skill-engine.ts — Skill system for Lucy ─────────────────────────────────
// Skills are structured prompts that drive multi-step sysadmin workflows.
// Each skill has phases with AI prompts + expected commands.

export interface SkillDef {
    id: string;
    name: string;
    nameEN: string;
    description: string;
    descriptionEN: string;
    icon: string;
    category: 'network' | 'security' | 'storage' | 'services' | 'monitoring' | 'custom';
    os: 'linux' | 'windows' | 'both';
    phases: SkillPhase[];
    tags: string[];
    author?: string;
    version?: string;
}

export interface SkillPhase {
    id: string;
    name: string;
    nameEN: string;
    prompt: string;         // AI prompt for this phase (includes <EXECUTE> instruction)
    expectVerdict?: boolean; // If true, expect <VERDICT>CONTINUE|DONE|ESCALATE</VERDICT>
    optional?: boolean;      // Phase can be skipped if prior phase resolves the issue
}

export type SkillRunPhase = 'idle' | 'running' | 'waiting' | 'done' | 'error';

export interface SkillRunState {
    skillId: string;
    skillName: string;
    active: boolean;
    currentPhaseIdx: number;
    phaseStatus: SkillRunPhase;
    results: SkillPhaseResult[];
    startTime: number;
    userInput?: string;     // User's context/parameters for the skill
}

export interface SkillPhaseResult {
    phaseId: string;
    phaseName: string;
    command?: string;
    output?: string;
    aiResponse?: string;
    verdict?: string;
    timestamp: number;
}

// ── SKILL REGISTRY ──────────────────────────────────────────────────────────

const _registry: Map<string, SkillDef> = new Map();

export function registerSkill(skill: SkillDef) {
    _registry.set(skill.id, skill);
}

export function getSkill(id: string): SkillDef | undefined {
    return _registry.get(id);
}

export function getAllSkills(): SkillDef[] {
    return [..._registry.values()];
}

export function getSkillsByCategory(cat: string): SkillDef[] {
    return getAllSkills().filter(s => s.category === cat);
}

export function getSkillsForOS(os: 'linux' | 'windows'): SkillDef[] {
    return getAllSkills().filter(s => s.os === 'both' || s.os === os);
}

export function searchSkills(query: string): SkillDef[] {
    const q = query.toLowerCase();
    return getAllSkills().filter(s =>
        s.name.toLowerCase().includes(q) ||
        s.nameEN.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q) ||
        s.tags.some(t => t.toLowerCase().includes(q))
    );
}

// ── SKILL RUN HELPERS ───────────────────────────────────────────────────────

export function createSkillRun(skill: SkillDef, userInput: string): SkillRunState {
    return {
        skillId: skill.id,
        skillName: skill.name,
        active: true,
        currentPhaseIdx: 0,
        phaseStatus: 'idle',
        results: [],
        startTime: Date.now(),
        userInput,
    };
}

export function buildPhasePrompt(
    skill: SkillDef,
    phase: SkillPhase,
    run: SkillRunState,
    hostName: string,
    hostType: string,
    bootCtx: string,
    isEN: boolean,
): string {
    const prevResults = run.results.map(r =>
        `[${r.phaseName}] ${r.command ? 'CMD: ' + r.command.substring(0, 200) : ''} ${r.output ? 'OUT: ' + r.output.substring(0, 600) : ''}`
    ).join('\n');

    return `[LUCY SKILL: ${skill.name} — Phase: ${phase.name}]
Host: ${hostName} (${hostType})
${bootCtx ? 'System: ' + bootCtx : ''}
${run.userInput ? 'User context: ' + run.userInput : ''}

${prevResults ? 'PREVIOUS PHASES:\n' + prevResults + '\n' : ''}
SKILL INSTRUCTIONS:
${phase.prompt}

Rules:
- Wrap commands in <EXECUTE></EXECUTE>
- Generate ONLY raw commands — no ssh wrappers, no Invoke-Command wrappers
- Language: ${isEN ? 'English' : 'Spanish'}
${phase.expectVerdict ? '- End your response with <VERDICT>CONTINUE</VERDICT>, <VERDICT>DONE</VERDICT>, or <VERDICT>ESCALATE</VERDICT>' : ''}`;
}

export function extractCommand(response: string): string | null {
    const m = response.match(/<EXECUTE>([\s\S]*?)<\/EXECUTE>/i)
           || response.match(/```(?:powershell|ps1|batch|cmd|bash|sh)?\s*\n([\s\S]*?)\n```/i);
    return m ? m[1].trim() : null;
}

export function extractVerdict(response: string): string | null {
    const m = response.match(/<VERDICT>([\s\S]*?)<\/VERDICT>/i);
    return m ? m[1].trim().toUpperCase() : null;
}

export function cleanResponse(response: string): string {
    return response
        .replace(/<EXECUTE>[\s\S]*?<\/EXECUTE>/gi, '')
        .replace(/<VERDICT>[\s\S]*?<\/VERDICT>/gi, '')
        .replace(/<[^>]*>/g, '')
        .trim();
}

// ── CATEGORY HELPERS ────────────────────────────────────────────────────────

export function categoryIcon(cat: string): string {
    const icons: Record<string, string> = {
        network: '◉', security: '⬡', storage: '⊞',
        services: '⚙', monitoring: '◑', custom: '⊕',
    };
    return icons[cat] || '⊕';
}

export function categoryLabel(cat: string, isEN: boolean): string {
    const labels: Record<string, [string, string]> = {
        network:    ['Red',          'Network'],
        security:   ['Seguridad',    'Security'],
        storage:    ['Almacenamiento','Storage'],
        services:   ['Servicios',    'Services'],
        monitoring: ['Monitoreo',    'Monitoring'],
        custom:     ['Personalizado','Custom'],
    };
    return labels[cat]?.[isEN ? 1 : 0] || cat;
}
