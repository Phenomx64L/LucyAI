// ── agent-loop-util.ts — small pure primitives for the agent loop ────────────
//
// Extracted from +page.svelte `runAI` (Phase-3 refactor, v1.7.199). The agent
// loop is the hot path where recent intermittent bugs lived, so Phase 3 moves
// out ONLY provably-pure leaf helpers (no closure/DOM/Svelte deps) — never the
// control flow — and locks each with tests.

/**
 * djb2 string hash → unsigned 32-bit int. Used by the skip-stuck detector to
 * cheaply tell whether two consecutive agent-turn responses are byte-identical
 * (the model grinding out the same output). Trims first so trailing-whitespace
 * churn between turns doesn't change the hash.
 *
 * Must stay bit-for-bit identical to the original inline version — the streak
 * counter compares hashes across turns, so any drift would silently change when
 * the loop bails.
 */
export function hashResp(s: string | null | undefined): number {
    let h = 5381;
    const str = String(s || '').trim();
    for (let i = 0; i < str.length; i++) h = ((h << 5) + h + str.charCodeAt(i)) | 0;
    return h >>> 0;
}

/**
 * Normalize an agent-turn response for NEAR-identical grind detection.
 *
 * The byte-identical skip-stuck (`hashResp`) only catches an EXACT repeat. A
 * model that re-emits the SAME plan/tool with cosmetic churn — a reworded
 * `<THOUGHT>`, different casing, changed whitespace/line-wrapping — evades it and
 * rides the loop toward MAX_LOOPS (the "molió con turnos casi iguales" symptom).
 * Hashing the NORMALIZED form (`hashResp(normalizeAgentResp(x))`) collapses that
 * churn so two cosmetically-different re-emissions of the same turn compare equal.
 *
 * Deliberately CONSERVATIVE so it never cuts genuine progress:
 *   - Drops `<THOUGHT>…</THOUGHT>` (and an unclosed trailing THOUGHT): reasoning
 *     is where wording varies most and is the least load-bearing part of a turn.
 *   - Lowercases and collapses all whitespace runs to one space.
 *   - KEEPS tool/execute tags and their args verbatim (only lowercased). Two
 *     turns acting on DIFFERENT targets (`readfile:A` vs `readfile:B`, or
 *     `readlines` at different offsets) therefore normalize DIFFERENTLY and are
 *     NOT flagged — only a real re-emission of the same action collapses. This
 *     is why digits are intentionally NOT stripped: paging a file in chunks is
 *     legitimate progress, not a grind.
 */
export function normalizeAgentResp(s: string | null | undefined): string {
    return String(s || '')
        .replace(/<THOUGHT>[\s\S]*?<\/THOUGHT>/gi, ' ')  // drop closed reasoning blocks
        .replace(/<THOUGHT>[\s\S]*$/i, ' ')              // drop an unclosed trailing THOUGHT
        .toLowerCase()
        .replace(/\s+/g, ' ')
        .trim();
}

interface ModelOption { id: string; icon?: string }
interface ModelGroup { provider?: string; options?: ModelOption[] }

/**
 * Given a model id and the LLM catalog (LLM_GROUPS), return the id of a STRONGER
 * model in the SAME provider family — or null if there's nothing to escalate to.
 * "Same family" guarantees the same credential/API key, so escalating never hits
 * an unconfigured provider. Used by the agent loop to self-heal when a weak model
 * keeps failing (e.g. Gemini Flash emitting malformed PowerShell twice in a row):
 * escalate once to the family flagship instead of grinding on the weak model.
 *
 * Rules (conservative, cost-aware):
 *   - Local models (`local-…`) never escalate: no cloud key implied, and the user
 *     chose local for cost/privacy.
 *   - If the current model IS the family flagship (◆), return null — a flagship
 *     failing isn't a weak-model problem, so a bigger model won't help.
 *   - Escalate to a flagship (◆) of the SAME group, PREFERRING a balanced-effort
 *     variant over the priciest `::high`/`::max` effort tier.
 *   - Unknown/uncatalogued model → null (don't guess).
 */
export function pickStrongerInFamily(
    modelId: string | null | undefined,
    groups: ModelGroup[] | null | undefined,
): string | null {
    const id = String(modelId || '');
    if (!id || id.startsWith('local-')) return null;
    const base = (s: string) => s.split('::')[0];
    const curBase = base(id);
    const group = (groups || []).find(g => (g.options || []).some(o => base(o.id) === curBase));
    if (!group || !group.options) return null;
    const cur = group.options.find(o => base(o.id) === curBase);
    if (cur && cur.icon === '◆') return null;                 // already the flagship
    const flagships = group.options.filter(o => o.icon === '◆');
    if (!flagships.length) return null;
    // Prefer a balanced flagship — avoid the slowest/priciest effort tier.
    const pick = flagships.find(o => !/::(high|max)\b/.test(o.id)) || flagships[0];
    return base(pick.id) === curBase ? null : pick.id;
}
