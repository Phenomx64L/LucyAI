// ── command-guard.ts — Pre-execution hook: detects dangerous commands ────────
// Analyzes commands for risk level before execution.
// Does NOT copy any external code — inspired by generic SysAdmin best-practices.

export type RiskLevel = 'critical' | 'high' | 'medium' | 'low' | 'safe';

export interface RiskAssessment {
    level: RiskLevel;
    score: number;            // 0–100
    command: string;
    matches: RiskMatch[];
    summary: string;
    suggestion?: string;
}

export interface RiskMatch {
    pattern: string;
    category: string;
    description: string;
    severity: RiskLevel;
    points: number;
}

// ── PATTERN DEFINITIONS ─────────────────────────────────────────────────────

interface DangerPattern {
    regex: RegExp;
    category: string;
    description: string;
    descriptionEN: string;
    severity: RiskLevel;
    points: number;
}

const LINUX_PATTERNS: DangerPattern[] = [
    // ── CRITICAL: Destructive filesystem ops ────
    { regex: /\brm\s+(-[a-zA-Z]*r[a-zA-Z]*f|--recursive\b|--force\b|-[a-zA-Z]*f[a-zA-Z]*r)\b/i,
      category: 'filesystem', description: 'Eliminacion recursiva forzada', descriptionEN: 'Forced recursive deletion',
      severity: 'critical', points: 40 },
    { regex: /\brm\s+(-[a-zA-Z]*r[a-zA-Z]*)\s+(\/|~\/|\$HOME|\/etc|\/var|\/usr|\/boot|\/root)/i,
      category: 'filesystem', description: 'Eliminacion recursiva en ruta critica', descriptionEN: 'Recursive delete on critical path',
      severity: 'critical', points: 50 },
    { regex: /\bmkfs\b/i,
      category: 'filesystem', description: 'Formateo de disco', descriptionEN: 'Disk formatting',
      severity: 'critical', points: 50 },
    { regex: /\bdd\s+.*\bof\s*=\s*\/dev\//i,
      category: 'filesystem', description: 'Escritura directa a dispositivo de bloque', descriptionEN: 'Direct write to block device',
      severity: 'critical', points: 50 },
    { regex: /:\s*>\s*\/[a-z]/i,
      category: 'filesystem', description: 'Truncamiento de archivo del sistema', descriptionEN: 'System file truncation',
      severity: 'high', points: 30 },

    // ── HIGH: Service/system manipulation ────
    { regex: /\bsystemctl\s+(stop|disable|mask)\s+(sshd|ssh|firewalld|iptables|ufw|systemd-journald|NetworkManager|docker|kubelet)/i,
      category: 'service', description: 'Detencion de servicio critico', descriptionEN: 'Critical service stop',
      severity: 'high', points: 30 },
    { regex: /\bshutdown\b|\breboot\b|\binit\s+[06]\b|\bpoweroff\b|\bhalt\b/i,
      category: 'system', description: 'Apagado o reinicio del sistema', descriptionEN: 'System shutdown or reboot',
      severity: 'high', points: 35 },
    { regex: /\bkill\s+-9\s+(-1|1)\b|\bkillall\b/i,
      category: 'process', description: 'Terminacion masiva de procesos', descriptionEN: 'Mass process kill',
      severity: 'high', points: 30 },

    // ── HIGH: Security-sensitive ops ────
    { regex: /\bchmod\s+777\b/i,
      category: 'security', description: 'Permisos abiertos a todos', descriptionEN: 'World-open permissions',
      severity: 'high', points: 25 },
    { regex: /\bchmod\s+[0-7]*s[0-7]*/i,
      category: 'security', description: 'Configuracion de SUID/SGID', descriptionEN: 'SUID/SGID bit change',
      severity: 'high', points: 25 },
    { regex: /\bpasswd\s+(root|--stdin)/i,
      category: 'security', description: 'Cambio de contrasena de root', descriptionEN: 'Root password change',
      severity: 'high', points: 30 },
    { regex: /\bufw\s+disable\b|\biptables\s+-F\b|\biptables\s+--flush\b/i,
      category: 'security', description: 'Desactivacion de firewall', descriptionEN: 'Firewall disable',
      severity: 'high', points: 30 },
    { regex: /\bvisudo\b|\bsudoers\b/i,
      category: 'security', description: 'Edicion de configuracion sudo', descriptionEN: 'Sudoers file edit',
      severity: 'high', points: 20 },

    // ── MEDIUM: Network and package ops ────
    { regex: /\bcurl\b.*\|\s*(bash|sh|sudo)/i,
      category: 'security', description: 'Ejecucion de script remoto via pipe', descriptionEN: 'Remote script piped to shell',
      severity: 'high', points: 30 },
    { regex: /\bwget\b.*-O\s*-\s*\|\s*(bash|sh)/i,
      category: 'security', description: 'Descarga y ejecucion de script', descriptionEN: 'Download and execute script',
      severity: 'high', points: 30 },
    { regex: /\bapt\s+(remove|purge|autoremove)\b|\byum\s+(remove|erase)\b|\bdnf\s+remove\b/i,
      category: 'package', description: 'Desinstalacion de paquetes', descriptionEN: 'Package uninstall',
      severity: 'medium', points: 15 },

    // ── MEDIUM: Data and disk ────
    { regex: /\b>\s*\/dev\/sd[a-z]/i,
      category: 'filesystem', description: 'Redireccion a dispositivo de disco', descriptionEN: 'Redirect to disk device',
      severity: 'critical', points: 50 },
    { regex: /\bfdisk\b|\bparted\b|\blvremove\b|\bpvremove\b|\bvgremove\b/i,
      category: 'filesystem', description: 'Manipulacion de particiones/LVM', descriptionEN: 'Partition/LVM manipulation',
      severity: 'high', points: 30 },

    // ── LOW: Informational but noteworthy ────
    { regex: /\bsudo\b/i,
      category: 'privilege', description: 'Ejecucion con privilegios elevados', descriptionEN: 'Elevated privilege execution',
      severity: 'low', points: 5 },
];

const WINDOWS_PATTERNS: DangerPattern[] = [
    // ── CRITICAL ────
    { regex: /\bRemove-Item\s+.*-Recurse\b.*-Force\b/i,
      category: 'filesystem', description: 'Eliminacion recursiva forzada', descriptionEN: 'Forced recursive deletion',
      severity: 'critical', points: 40 },
    { regex: /\bFormat-Volume\b|\bClear-Disk\b|\bInitialize-Disk\b/i,
      category: 'filesystem', description: 'Formateo de disco', descriptionEN: 'Disk formatting',
      severity: 'critical', points: 50 },
    { regex: /\bRemove-Item\s+.*(?:C:\\Windows|C:\\Program\s*Files|System32)/i,
      category: 'filesystem', description: 'Eliminacion en ruta de sistema', descriptionEN: 'System path deletion',
      severity: 'critical', points: 50 },

    // ── HIGH ────
    { regex: /\bStop-Service\s+.*(?:WinRM|sshd|Dnscache|wuauserv|MpsSvc|EventLog|W3SVC|MSSQLSERVER)/i,
      category: 'service', description: 'Detencion de servicio critico', descriptionEN: 'Critical service stop',
      severity: 'high', points: 30 },
    { regex: /\bRestart-Computer\b|\bStop-Computer\b|\bshutdown\s+\/[srt]/i,
      category: 'system', description: 'Apagado o reinicio del sistema', descriptionEN: 'System shutdown or reboot',
      severity: 'high', points: 35 },
    { regex: /\bStop-Process\s+.*-Force\b|\btaskkill\s+\/F/i,
      category: 'process', description: 'Terminacion forzada de procesos', descriptionEN: 'Force process kill',
      severity: 'high', points: 25 },
    { regex: /\bSet-NetFirewallProfile\s+.*-Enabled\s+False/i,
      category: 'security', description: 'Desactivacion de firewall', descriptionEN: 'Firewall disable',
      severity: 'high', points: 30 },
    { regex: /\bDisable-NetAdapter\b/i,
      category: 'security', description: 'Desactivacion de adaptador de red', descriptionEN: 'Network adapter disable',
      severity: 'high', points: 30 },
    { regex: /\bNew-LocalUser\b|\bAdd-LocalGroupMember\b.*Administrators/i,
      category: 'security', description: 'Creacion/elevacion de usuario local', descriptionEN: 'Local user creation/elevation',
      severity: 'high', points: 25 },

    // ── MEDIUM ────
    { regex: /\bUninstall-Package\b|\bRemove-WindowsFeature\b/i,
      category: 'package', description: 'Desinstalacion de paquetes o features', descriptionEN: 'Package/feature uninstall',
      severity: 'medium', points: 15 },
    { regex: /\bSet-ExecutionPolicy\s+Unrestricted\b/i,
      category: 'security', description: 'Politica de ejecucion sin restricciones', descriptionEN: 'Unrestricted execution policy',
      severity: 'medium', points: 20 },
    { regex: /\breg\s+(delete|add)\b.*\\HKLM\\/i,
      category: 'registry', description: 'Modificacion de registro del sistema', descriptionEN: 'System registry modification',
      severity: 'medium', points: 20 },
    { regex: /\bRemove-Item\b.*-Recurse\b/i,
      category: 'filesystem', description: 'Eliminacion recursiva', descriptionEN: 'Recursive deletion',
      severity: 'medium', points: 15 },

    // ── LOW ────
    { regex: /\bRunAs\b|\b-Credential\b/i,
      category: 'privilege', description: 'Ejecucion con credenciales alternativas', descriptionEN: 'Alternate credential execution',
      severity: 'low', points: 5 },
];

// ── SQL / DB patterns (cross-platform) ────
const DB_PATTERNS: DangerPattern[] = [
    { regex: /\bDROP\s+(DATABASE|TABLE|SCHEMA|INDEX)\b/i,
      category: 'database', description: 'Eliminacion de estructura de base de datos', descriptionEN: 'Database structure drop',
      severity: 'critical', points: 50 },
    { regex: /\bTRUNCATE\s+TABLE\b/i,
      category: 'database', description: 'Truncamiento de tabla', descriptionEN: 'Table truncation',
      severity: 'critical', points: 40 },
    { regex: /\bDELETE\s+FROM\b(?!\s+.*\bWHERE\b)/i,
      category: 'database', description: 'DELETE sin clausula WHERE', descriptionEN: 'DELETE without WHERE clause',
      severity: 'critical', points: 45 },
    { regex: /\bUPDATE\b.*\bSET\b(?![\s\S]*\bWHERE\b)/i,
      category: 'database', description: 'UPDATE sin clausula WHERE', descriptionEN: 'UPDATE without WHERE clause',
      severity: 'high', points: 35 },
    { regex: /\bALTER\s+TABLE\b.*\bDROP\s+COLUMN\b/i,
      category: 'database', description: 'Eliminacion de columna', descriptionEN: 'Column drop',
      severity: 'high', points: 25 },
    { regex: /\bGRANT\s+ALL\b/i,
      category: 'database', description: 'Asignacion de todos los privilegios', descriptionEN: 'Grant all privileges',
      severity: 'medium', points: 20 },
];

// ── MAIN ANALYSIS FUNCTION ──────────────────────────────────────────────────

export function analyzeCommand(
    command: string,
    hostType: 'linux' | 'windows' = 'linux',
    isEN = false,
): RiskAssessment {
    const matches: RiskMatch[] = [];
    const patterns = [
        ...(hostType === 'linux' ? LINUX_PATTERNS : WINDOWS_PATTERNS),
        ...DB_PATTERNS,
    ];

    for (const p of patterns) {
        if (p.regex.test(command)) {
            matches.push({
                pattern: p.regex.source,
                category: p.category,
                description: isEN ? p.descriptionEN : p.description,
                severity: p.severity,
                points: p.points,
            });
        }
    }

    const score = Math.min(100, matches.reduce((sum, m) => sum + m.points, 0));
    const level: RiskLevel = score >= 40 ? 'critical'
                           : score >= 25 ? 'high'
                           : score >= 10 ? 'medium'
                           : score > 0   ? 'low'
                           : 'safe';

    const topMatch = matches.sort((a, b) => b.points - a.points)[0];

    let summary = '';
    let suggestion: string | undefined;

    if (level === 'critical') {
        summary = isEN
            ? `CRITICAL RISK: This command could cause irreversible damage. ${topMatch?.description || ''}`
            : `RIESGO CRITICO: Este comando podria causar dano irreversible. ${topMatch?.description || ''}`;
        suggestion = isEN
            ? 'Consider creating a backup or snapshot before proceeding.'
            : 'Considera crear un respaldo o snapshot antes de continuar.';
    } else if (level === 'high') {
        summary = isEN
            ? `HIGH RISK: This command modifies critical system configuration. ${topMatch?.description || ''}`
            : `RIESGO ALTO: Este comando modifica configuracion critica del sistema. ${topMatch?.description || ''}`;
        suggestion = isEN
            ? 'Verify the target and parameters are correct.'
            : 'Verifica que el objetivo y los parametros sean correctos.';
    } else if (level === 'medium') {
        summary = isEN
            ? `MODERATE RISK: ${topMatch?.description || 'This command makes notable changes.'}`
            : `RIESGO MODERADO: ${topMatch?.description || 'Este comando hace cambios notables.'}`;
    } else if (level === 'low') {
        summary = isEN
            ? `LOW RISK: ${topMatch?.description || 'Minor privilege or config change.'}`
            : `RIESGO BAJO: ${topMatch?.description || 'Cambio menor de privilegios o configuracion.'}`;
    } else {
        summary = isEN ? 'No risk detected.' : 'Sin riesgo detectado.';
    }

    return { level, score, command, matches, summary, suggestion };
}

// ── HELPERS ──────────────────────────────────────────────────────────────────

export function shouldBlock(assessment: RiskAssessment, threshold: RiskLevel = 'high'): boolean {
    const order: RiskLevel[] = ['safe', 'low', 'medium', 'high', 'critical'];
    return order.indexOf(assessment.level) >= order.indexOf(threshold);
}

export function riskColor(level: RiskLevel): string {
    switch (level) {
        case 'critical': return '#ff2020';
        case 'high':     return '#ff6644';
        case 'medium':   return '#ffaa33';
        case 'low':      return '#88aacc';
        default:         return '#10b981';
    }
}

export function riskIcon(level: RiskLevel): string {
    switch (level) {
        case 'critical': return '🚨';
        case 'high':     return '⚠️';
        case 'medium':   return '🔶';
        case 'low':      return '🔵';
        default:         return '✅';
    }
}
