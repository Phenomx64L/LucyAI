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
    return get(activeSecuritySkill);
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
    const codes = [
        s.meta.mitre_attck.length ? `MITRE ATT&CK: ${s.meta.mitre_attck.join(', ')}` : null,
        s.meta.nist_csf.length    ? `NIST CSF: ${s.meta.nist_csf.join(', ')}`         : null,
        s.meta.mitre_d3fend.length? `MITRE D3FEND: ${s.meta.mitre_d3fend.join(', ')}` : null,
    ].filter(Boolean).join(' · ');
    const header = `[ACTIVE CYBERSECURITY SKILL — ${s.meta.name}]
Description: ${s.meta.description}
Domain: ${s.meta.domain}${s.meta.subdomain ? ` / ${s.meta.subdomain}` : ''}
${codes ? `Frameworks: ${codes}\n` : ''}
═══ HOW TO USE THIS SKILL — READ CAREFULLY ═══

This skill is a DOCUMENTED REFERENCE PROCEDURE. The code blocks
below are EXAMPLE COMMANDS that illustrate the workflow — they
are NOT instructions to execute immediately.

Hard rules for this turn:

1. PRESENT the workflow as guidance. Explain each phase, cite the
   relevant commands, but DO NOT auto-run any of them unless the
   user explicitly asks "run this" / "ejecuta esto" / "do step N".

2. CHECK PREREQUISITES before proposing any command:
   - Required modules installed? (ExchangeOnlineManagement,
     ActiveDirectory, Az.Accounts, etc.) Test with
     \`Get-Module -ListAvailable\` if you're unsure.
   - Required remote session connected? (Connect-IPPSSession,
     Connect-AzAccount, kubectl context, ssh tunnel, …)
   - Required role / permission? (Global Admin, Domain Admin,
     audit role …)

3. If a prerequisite is MISSING, mention it instead of running
   the command. Example: "This workflow uses Exchange Online's
   New-ComplianceSearch. You don't have ExchangeOnlineManagement
   loaded in this session — connect with Connect-IPPSSession
   first, then I can run the search."

4. If the workflow targets a system the user hasn't mentioned
   (Splunk, Sentinel, CrowdStrike Falcon, …), ASK whether they
   have access, don't assume.

5. Cite framework codes (MITRE ATT&CK / NIST CSF) when they
   clarify intent, not as filler.

6. The skill describes a GENERAL procedure. Adapt the steps to
   the user's actual stack — don't copy-paste a SIEM query into
   a PowerShell prompt.

The full skill body follows below. Treat it as a senior analyst's
runbook you're consulting, not a script to execute.

════════════════════════════════════════════════════════════════════════
`;
    // Cap body to ~8 KB so it doesn't blow the context budget. ADR-200
    // skills are typically 6-10 KB; longer ones get tail-truncated with
    // a marker so the LLM knows there's more on disk if it asks.
    const MAX = 8000;
    const body = s.body.length > MAX
        ? s.body.slice(0, MAX) + `\n\n[…skill body truncated at ${MAX} chars — full text in docs/security-skills/${s.meta.id}/SKILL.md…]`
        : s.body;
    return header + body;
}
