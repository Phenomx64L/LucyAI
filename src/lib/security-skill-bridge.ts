// ── security-skill-bridge.ts (v1.7.4) ─────────────────────────────────────
//
// Lets a security skill activated via `/sec-skill use <id>` ride into the
// next prompt through the SAME injection point that the v1.6.1 preset
// system already uses. Without this bridge a security-skill activation
// would just sit in the store and never make it to the LLM.
//
// Architecture: we keep ONE active "skill thing" at a time. It's either:
//   - a normal SkillPreset (built-in from `$lib/skill-presets`), or
//   - a security skill loaded from `docs/security-skills/` (this module)
//
// The injection point in `+page.svelte` checks security-skill first; if
// nothing's there, it falls back to the regular preset. Clearing the
// preset id from the picker clears either kind.
//
// The active security skill is persisted to localStorage so it survives
// reloads, matching the preset system's behaviour.

import { writable, get, type Writable } from 'svelte/store';
import { safeGetLS, safeSetLS } from '$lib/safe-ls';

export interface SecuritySkillFull {
    meta: {
        id: string;
        name: string;
        description: string;
        domain: string;
        subdomain: string;
        tags: string[];
        version: string;
        author: string;
        nist_csf: string[];
        mitre_attck: string[];
        mitre_atlas: string[];
        mitre_d3fend: string[];
        ai_rmf: string[];
    };
    body: string;
}

const LS_KEY = 'lucy_active_security_skill_v1';

function loadCached(): SecuritySkillFull | null {
    const raw = safeGetLS(LS_KEY, '');
    if (!raw) return null;
    try { return JSON.parse(raw) as SecuritySkillFull; }
    catch { return null; }
}

/** Single-slot store. `null` = no security skill active. */
export const activeSecuritySkill: Writable<SecuritySkillFull | null> =
    writable(loadCached());

activeSecuritySkill.subscribe(s => {
    if (s) {
        try { safeSetLS(LS_KEY, JSON.stringify(s)); } catch { /* quota */ }
    } else {
        try { safeSetLS(LS_KEY, ''); } catch { /* quota */ }
    }
});

/** Imperative setter — used by `/sec-skill use <id>`. Clears any
 *  previously-active security skill atomically. */
export function setSecuritySkillAsPreset(full: SecuritySkillFull): void {
    activeSecuritySkill.set(full);
}

/** Clear the active security skill. Called by `/preset clear` and the
 *  picker's "no preset" option. */
export function clearActiveSecuritySkill(): void {
    activeSecuritySkill.set(null);
}

/** Synchronous peek for prompt builders. */
export function peekActiveSecuritySkill(): SecuritySkillFull | null {
    const s = get(activeSecuritySkill);
    if (!s) return null;
    // v1.7.153 — defensive self-heal. A stale/corrupt entry whose meta has
    // neither `id` nor `name` is a "zombie": it renders as "(active framing)"
    // in the route chip and silently forces info-only mode (every <EXECUTE>
    // downgraded to a non-running code fence + an "EXPLAIN, NOT EXECUTE"
    // prompt framing) on EVERY turn — and it survived `/preset clear`. Treat
    // it as inactive and purge it so command execution is restored.
    const m = s.meta;
    if (!m || (!m.id && !m.name)) {
        clearActiveSecuritySkill();
        return null;
    }
    return s;
}

/** Render the active security skill as a prompt prefix matching the
 *  shape `renderPresetForPrompt` produces for normal presets.
 *
 *  v1.7.6: hardened framing after a real-world bug — Lucy ran
 *  `New-ComplianceSearch` from a phishing skill against the user's
 *  local PowerShell (no Exchange Online module installed, no IPPS
 *  session) because the LLM interpreted the workflow code blocks as
 *  "instructions to execute now" instead of "documented patterns".
 *  The new framing makes the distinction explicit and instructs the
 *  agent loop to confirm BEFORE running anything from the skill body. */
export function renderSecuritySkillForPrompt(s: SecuritySkillFull): string {
    // v1.7.6 defensive: meta + every list could be undefined if a stale
    // localStorage entry or a partial backend payload survives a schema
    // change. Fall back to empty strings/arrays so we never crash inside
    // the prompt builder — better to render a slightly less-rich header
    // than to throw "Cannot read properties of undefined" and break the
    // user's whole turn.
    const meta = (s && s.meta) || ({} as SecuritySkillFull['meta']);
    const safeArr = (a?: string[]): string[] => Array.isArray(a) ? a : [];
    const attck   = safeArr(meta.mitre_attck);
    const csf     = safeArr(meta.nist_csf);
    const d3fend  = safeArr(meta.mitre_d3fend);
    const codes = [
        attck.length  ? `MITRE ATT&CK: ${attck.join(', ')}`  : null,
        csf.length    ? `NIST CSF: ${csf.join(', ')}`        : null,
        d3fend.length ? `MITRE D3FEND: ${d3fend.join(', ')}` : null,
    ].filter(Boolean).join(' · ');
    const name        = meta.name        || 'unnamed-skill';
    const description = meta.description || '';
    const domain      = meta.domain      || '';
    const subdomain   = meta.subdomain   || '';
    const header = `[ACTIVE CYBERSECURITY SKILL — ${name}]
Description: ${description}
Domain: ${domain}${subdomain ? ` / ${subdomain}` : ''}
${codes ? `Frameworks: ${codes}\n` : ''}
═══ HOW TO USE THIS SKILL — READ THIS BEFORE EMITTING ANY <EXECUTE> ═══

This skill is a DOCUMENTED REFERENCE PROCEDURE from an analyst
runbook library. The code blocks below contain ILLUSTRATIVE
EXAMPLES with PLACEHOLDER VALUES — they are NOT runnable
commands until the user provides real values.

═══ CRITICAL — PLACEHOLDER DETECTION ═══

Skill code blocks routinely contain placeholder strings such as:
  • C:\\Ruta\\Al\\Correo\\sospechoso.eml
  • C:\\Path\\To\\Evidence
  • /path/to/file
  • tu-usuario@dominio.com / user@example.com / admin@tenant.onmicrosoft.com
  • <TENANT_ID> / [INSERT_DOMAIN] / YOUR-API-KEY
  • $emlPath, $caseId (PowerShell variables with no assignment)
  • Purga_Phishing_Incident_01 (example campaign names)

NEVER emit a <EXECUTE> block that contains any of these values.
NEVER substitute them with plausible-sounding guesses. NEVER use
"sample" or "example" data and pretend it's real.

If a workflow step contains placeholders, you must:
  1. EXPLAIN the step in prose, showing the example as
     illustration only.
  2. ASK the user for the real value before any execution:
     "Para el paso 2, necesito la ruta real del .eml. ¿Lo
     tienes guardado en algún lado?"
  3. Wait for the user's reply BEFORE emitting <EXECUTE>.

═══ HARD RULES FOR THIS TURN ═══

1. DEFAULT MODE = EXPLAIN, NOT EXECUTE. Walk through the
   workflow, cite specific commands, explain what each does.
   Only emit <EXECUTE> if the user has explicitly said "run
   this", "ejecuta esto", "do step N", or has supplied concrete
   values that match the step's parameters.

2. CHECK PREREQUISITES before proposing execution:
   - Modules installed? (ExchangeOnlineManagement, Az.Accounts,
     ActiveDirectory). If unsure, suggest \`Get-Module -ListAvailable\`.
   - Remote session connected? (Connect-IPPSSession,
     Connect-AzAccount, kubectl context, ssh tunnel)
   - Role / permission? (Global Admin, Domain Admin, audit role)
   If a prerequisite is missing, SAY SO. Do not try to install
   modules silently. Do not run substitute commands hoping they
   exist.

3. IF A COMMAND FROM THIS SKILL FAILS, the failure is EXPECTED
   evidence that prerequisites are missing or paths are
   placeholders. Do NOT enter auto-correction mode. Do NOT try
   variations. STOP and report the failure to the user so they
   can decide.

4. ADAPT TO THE USER'S STACK. The skill is generic. If the user
   is on Windows but the example uses bash, translate (don't
   blindly copy). If the user hasn't mentioned a SIEM (Splunk,
   Sentinel, CrowdStrike), ASK before assuming.

5. CITE framework codes (MITRE ATT&CK / NIST CSF) when they
   clarify intent, not as filler.

6. SCOPE DISCIPLINE. The user asked a specific question. Stay
   on it. Do NOT pivot to unrelated tasks (e.g. "while we're
   here, let me also run a full security audit"). If the
   workflow exhausts what the user asked, summarize and STOP.

The full skill body follows below. Treat it as documentation
you're explaining, not a script to perform.

════════════════════════════════════════════════════════════════════════
`;
    // Cap body to ~8 KB so it doesn't blow the context budget. ADR-200
    // skills are typically 6-10 KB; longer ones get tail-truncated with
    // a marker so the LLM knows there's more on disk if it asks.
    const MAX = 8000;
    const rawBody = (s && typeof s.body === 'string') ? s.body : '';
    const body = rawBody.length > MAX
        ? rawBody.slice(0, MAX) + `\n\n[…skill body truncated at ${MAX} chars — full text in docs/security-skills/${meta.id || name}/SKILL.md…]`
        : rawBody;
    return header + body;
}
