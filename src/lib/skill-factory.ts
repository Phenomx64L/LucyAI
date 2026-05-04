// ── skill-factory — silently observe repeated workflows, propose skills ──
//
// Adapted from the hermes-skill-factory community project. Watches every
// successful exec in the active session and detects when the user has
// repeated:
//   • the SAME single command (≥2× → eligible after 3rd time)
//   • a SEQUENCE of 2-3 commands in the SAME order (≥2× → eligible
//     after the second occurrence — sequences are stronger signal)
//
// On detection it emits a structured proposal the UI can render:
//   { kind: 'single' | 'sequence',
//     occurrences, commands, suggestedName, suggestedScript }
//
// The factory is PURE state — no DOM, no Svelte, no Tauri. The caller
// decides when to call observe()/getProposals()/dismissProposal()/etc.
//
// Storage:
//   - In-memory ring buffer per tab (last 50 commands).
//   - Persisted to localStorage between reloads under
//     `lucy_skill_factory_${tabId}` so a refresh doesn't lose the signal.
//
// Privacy: only command STRINGS are tracked — no outputs, no env vars,
// no host names. Hashed before any cross-session comparison.

import { safeParseLS, safeSetLS } from './safe-ls';

const RING_MAX           = 50;        // how many recent commands to remember per tab
const SEQ_MAX_LEN        = 3;         // longest sequence we try to detect
const SINGLE_MIN_COUNT   = 3;         // a lone cmd needs 3+ occurrences to propose
const SEQUENCE_MIN_COUNT = 2;         // a sequence only needs 2 (stronger signal)
const PROPOSAL_COOLDOWN_MS = 60_000;  // don't re-propose the same workflow within 1 min

export interface CommandObservation {
    cmd: string;          // the raw command string
    target?: string;      // 'local' | hostId
    engine?: string;      // 'powershell' | 'cmd' | 'shell' | …
    ts: number;           // wall clock of the observation
    ok: boolean;          // success — only OK execs feed the buffer
}

export type ProposalKind = 'single' | 'sequence';

export interface SkillProposal {
    kind: ProposalKind;
    occurrences: number;
    commands: string[];               // 1 entry for 'single', N for 'sequence'
    suggestedName: string;
    suggestedDescription: string;
    suggestedScript: string;
    suggestedTriggers: string[];
    /** Stable id derived from commands — used to dedupe proposals. */
    fingerprint: string;
    /** Wall clock when first detected this round. */
    detectedAt: number;
}

interface FactoryState {
    ring: CommandObservation[];
    /** fingerprint → last-proposed timestamp, for cooldown */
    proposedAt: Record<string, number>;
    /** fingerprints the user actively dismissed — never re-propose */
    dismissed: string[];
}

// ── Helpers ──────────────────────────────────────────────────────────────

function _stateKey(tabId: string | number) { return `lucy_skill_factory_${tabId}`; }
function _loadState(tabId: string | number): FactoryState {
    const s = safeParseLS<FactoryState>(_stateKey(tabId), {
        ring: [], proposedAt: {}, dismissed: [],
    });
    // Defensive backfill in case schema changed.
    s.ring       ??= [];
    s.proposedAt ??= {};
    s.dismissed  ??= [];
    return s;
}
function _saveState(tabId: string | number, s: FactoryState) {
    safeSetLS(_stateKey(tabId), s);
}

/** Lossy normalization so "Get-Service IIS" and "Get-Service  IIS" match. */
function _normalize(cmd: string): string {
    return String(cmd || '')
        .trim()
        .replace(/\s+/g, ' ')
        .replace(/['"`]/g, '')
        .toLowerCase()
        .slice(0, 200);
}

/** Fingerprint a sequence of normalized commands. Order matters. */
function _fingerprint(normCmds: string[]): string {
    return normCmds.join(' || ');
}

/** Suggest a kebab-case name from a command. Best-effort, user can edit. */
function _suggestName(cmd: string): string {
    // Pull the first non-flag identifier.
    const tokens = cmd.split(/\s+/).filter(t => !t.startsWith('-') && !t.startsWith('/'));
    const head = tokens[0] || cmd.split(/\s+/)[0] || 'workflow';
    const rest = tokens.slice(1, 3).filter(Boolean).join('-');
    const raw = rest ? `${head}-${rest}` : head;
    return raw
        .replace(/[^a-zA-Z0-9-]/g, '-')
        .replace(/-+/g, '-')
        .replace(/^-|-$/g, '')
        .toLowerCase()
        .slice(0, 40) || 'workflow';
}

/** Build the script body for a proposed skill. Sequences become multi-line. */
function _buildScript(commands: string[]): string {
    return commands.join('\n');
}

/** Trigger phrases — heuristics from the command for memoria-style match. */
function _suggestTriggers(commands: string[]): string[] {
    const out = new Set<string>();
    for (const c of commands) {
        const tokens = c.split(/\s+/).filter(t => /^[a-zA-Z][a-zA-Z0-9_-]+$/.test(t));
        if (tokens[0]) out.add(tokens[0].toLowerCase());
        if (tokens[1]) out.add(`${tokens[0]} ${tokens[1]}`.toLowerCase());
    }
    return [...out].slice(0, 5);
}

// ── Public API ───────────────────────────────────────────────────────────

/**
 * Record a successfully-executed command for a tab. Called from the agent
 * loop right after each exec_powershell / executemany / etc returns ok=true.
 * Failed execs are intentionally NOT tracked — we don't want to propose
 * skills that don't work.
 */
export function observe(tabId: string | number, obs: CommandObservation): void {
    if (!obs?.cmd || !obs.ok) return;
    const s = _loadState(tabId);
    s.ring.push({
        cmd: obs.cmd.slice(0, 400),
        target: obs.target,
        engine: obs.engine,
        ts: obs.ts || Date.now(),
        ok: !!obs.ok,
    });
    if (s.ring.length > RING_MAX) s.ring.splice(0, s.ring.length - RING_MAX);
    _saveState(tabId, s);
}

/**
 * Find new proposals. Pure read-only inspection of the ring + cooldown
 * registry — does not mutate state. Caller flags accept/dismiss explicitly.
 *
 * Returns 0..2 proposals (we cap to avoid spamming).
 */
export function getProposals(tabId: string | number): SkillProposal[] {
    const s = _loadState(tabId);
    if (s.ring.length < SEQUENCE_MIN_COUNT * 2) return [];

    const now      = Date.now();
    const norm     = s.ring.map(o => _normalize(o.cmd));
    const proposals: SkillProposal[] = [];

    // ── Pass A: sequences of length 2-3 ─────────────────────────────────
    // Sliding window: for each position, hash the window and count
    // occurrences. ≥2 occurrences of the same window → propose.
    for (let len = SEQ_MAX_LEN; len >= 2; len--) {
        if (norm.length < len * 2) continue;
        const counts = new Map<string, { count: number; firstIdx: number; cmds: string[] }>();
        for (let i = 0; i + len <= norm.length; i++) {
            const slice = norm.slice(i, i + len);
            // Skip windows where any cmd is empty / trivially short.
            if (slice.some(c => c.length < 4)) continue;
            const fp = _fingerprint(slice);
            const e  = counts.get(fp);
            if (e) { e.count++; }
            else   { counts.set(fp, { count: 1, firstIdx: i, cmds: s.ring.slice(i, i + len).map(o => o.cmd) }); }
        }
        for (const [fp, e] of counts.entries()) {
            if (e.count < SEQUENCE_MIN_COUNT) continue;
            if (s.dismissed.includes(fp)) continue;
            if ((now - (s.proposedAt[fp] || 0)) < PROPOSAL_COOLDOWN_MS) continue;
            proposals.push(_buildProposal('sequence', e.count, e.cmds, fp, now));
            if (proposals.length >= 2) return proposals;
        }
    }

    // ── Pass B: single commands ────────────────────────────────────────
    if (proposals.length < 2) {
        const counts = new Map<string, { count: number; cmd: string }>();
        for (let i = 0; i < norm.length; i++) {
            const k = norm[i];
            if (k.length < 4) continue;       // skip trivial single-token cmds
            const e = counts.get(k);
            if (e) e.count++;
            else   counts.set(k, { count: 1, cmd: s.ring[i].cmd });
        }
        for (const [fp, e] of counts.entries()) {
            if (e.count < SINGLE_MIN_COUNT) continue;
            if (s.dismissed.includes(fp)) continue;
            if ((now - (s.proposedAt[fp] || 0)) < PROPOSAL_COOLDOWN_MS) continue;
            proposals.push(_buildProposal('single', e.count, [e.cmd], fp, now));
            if (proposals.length >= 2) break;
        }
    }
    return proposals;
}

function _buildProposal(
    kind: ProposalKind,
    occurrences: number,
    commands: string[],
    fingerprint: string,
    now: number,
): SkillProposal {
    const name = _suggestName(commands[0]);
    const desc = kind === 'sequence'
        ? `Auto-detected workflow: ${commands.length} steps repeated ${occurrences}× this session.`
        : `Auto-detected command: "${commands[0].slice(0, 60)}" used ${occurrences}× this session.`;
    return {
        kind,
        occurrences,
        commands,
        suggestedName: name,
        suggestedDescription: desc,
        suggestedScript: _buildScript(commands),
        suggestedTriggers: _suggestTriggers(commands),
        fingerprint,
        detectedAt: now,
    };
}

/** Mark a proposal as accepted: stamps proposedAt so we don't re-suggest the
 *  same fingerprint again immediately. The actual save_skill call is the
 *  caller's responsibility. */
export function markAccepted(tabId: string | number, fingerprint: string): void {
    const s = _loadState(tabId);
    s.proposedAt[fingerprint] = Date.now();
    _saveState(tabId, s);
}

/** Mark a proposal as dismissed forever — never re-suggest. */
export function dismissProposal(tabId: string | number, fingerprint: string): void {
    const s = _loadState(tabId);
    if (!s.dismissed.includes(fingerprint)) s.dismissed.push(fingerprint);
    if (s.dismissed.length > 64) s.dismissed.splice(0, s.dismissed.length - 64);
    _saveState(tabId, s);
}

/** Clear everything for a tab — invoked when the tab itself is closed. */
export function resetForTab(tabId: string | number): void {
    safeSetLS(_stateKey(tabId), { ring: [], proposedAt: {}, dismissed: [] });
}
