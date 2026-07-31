// ── llm-models.ts — Single source of truth for LLM model ids (v1.7.0) ────
//
// Before this module the frontend hardcoded model strings in ad-hoc
// `invoke('ask_lucy', { ..., model: 'gemini-3.5-flash-lite', ... })`
// callsites. Across 5 different sites we had:
//
//   - 'gemini-3.5-flash-lite'   ← NOT a real Gemini model. Silently failed
//                                 in MemoryBrowserView (v1.6.10 fix) and
//                                 LogViewerView (v1.6.16 fix).
//   - 'gemini-3-flash-preview'  ← legacy alias the backend resolves to 3.5
//   - 'gemini-2.5-flash'        ← still valid but old gen
//
// This module is the canonical list. The backend whitelist lives in
// `src-tauri/src/state.rs::ALLOWED_MODELS`; keep these two files in
// sync — adding a new id here without registering it there will
// produce 'unauthorized model' errors at runtime.
//
// ── How to choose a tier ────────────────────────────────────────────────
//
//   FAST       — default chat answers, smart-filter regex, autotag.
//                Gemini 3.5 Flash — frontier-class at lower cost.
//                Use this 95% of the time.
//
//   CHEAP      — 1-line classification, tag suggestion, intent extraction.
//                Gemini 3.1 Flash Lite preview — cheapest tier. ~3x
//                cheaper than FAST, slightly weaker for nuanced output
//                but more than enough for "produce a comma list".
//
//   REASONING  — complex multi-step reasoning, agent planning, code
//                review, root-cause analysis. Gemini 3.1 Pro preview
//                with high effort budget.
//
//   VISION     — image-aware queries (screenshot analysis, document
//                OCR-into-structured-data). Same FAST model; Gemini
//                multimodal is unified.
//
// ── Migration playbook ──────────────────────────────────────────────────
//
//   ❌ Before:
//        invoke('ask_lucy', { ..., model: 'gemini-3.5-flash-lite' })
//
//   ✅ After:
//        import { LLM } from '$lib/llm-models';
//        invoke('ask_lucy', { ..., model: LLM.CHEAP })
//
// Any string literal `model: 'gemini-...'` in the frontend should be
// considered a code smell going forward.

/**
 * Canonical model ids per tier. These MUST match a row in
 * `src-tauri/src/state.rs::ALLOWED_MODELS` or the request will be
 * rejected by the backend's whitelist check.
 */
export const LLM = Object.freeze({
    /** Default chat tier — Gemini 3.5 Flash. Frontier-class, 1M context, low cost. */
    FAST:      'gemini-3.5-flash',
    /** Cheapest tier for 1-line tasks — Gemini 3.1 Flash Lite preview. */
    CHEAP:     'gemini-3.1-flash-lite-preview',
    /** Heavy reasoning tier — Gemini 3.1 Pro with high effort budget. */
    REASONING: 'gemini-3.1-pro-preview::high',
    /** Vision-capable model — Gemini multimodal is unified with FAST. */
    VISION:    'gemini-3.5-flash',
    /** Legacy alias kept for old saved chats. Backend resolves it to FAST. */
    LEGACY:    'gemini-3-flash-preview',
} as const);

export type LlmTier = keyof typeof LLM;
export type LlmModelId = typeof LLM[LlmTier];

/**
 * All ids that the backend currently accepts. Kept here as a sanity
 * check the dev can import to validate a runtime-supplied string
 * before sending it through `ask_lucy`.
 *
 * Source of truth is `src-tauri/src/state.rs::ALLOWED_MODELS`. We
 * mirror only the Gemini portion here — Claude/Ollama ids live
 * elsewhere because they have separate effort-suffix grammars.
 */
export const KNOWN_GEMINI_IDS: ReadonlySet<string> = new Set([
    'gemini-3.1-pro-preview',
    'gemini-3.1-pro-preview::high',
    'gemini-3.1-pro-preview::medium',
    'gemini-3.6-flash',
    'gemini-3.5-flash',
    'gemini-3.5-flash-lite',
    'gemini-3.1-flash-lite',
    'gemini-3.1-flash-lite-preview',
    'gemini-3-flash-preview',
    'gemini-2.5-flash',
    'gemini-2.5-pro',
    'gemini-2.5-flash-lite-preview',
]);

// ── v1.7.30 — Per-model context window (input tokens) ───────────────────
//
// The Context Strip token chip needs a per-model "max tokens" denominator
// to render `used/max` and band by % consumed (green<65%, amber<85%,
// red>85%). Lucy was previously hardcoded to 0 (idle grey) because no
// catalog field existed. This map fills that gap.
//
// Numbers from each vendor's published context-window docs as of June 2026.
// When in doubt, take the LOWER advertised number (input window, not
// output) and round down — over-promising the user's budget is the bug
// to avoid.
//
// Unknown / unrecognised model → falls back to 128_000 (a safe modern
// default that covers Claude 3, GPT-4-class, and most Ollama models).

const CONTEXT_WINDOWS: Record<string, number> = {
    // Gemini 3.x family (1M tokens input across the line)
    'gemini-3.6-flash':                1_000_000,
    'gemini-3.5-flash':                1_000_000,
    'gemini-3.5-flash-lite':           1_000_000,
    'gemini-3.1-flash-lite-preview':   1_000_000,
    'gemini-3.1-flash-lite':           1_000_000,
    'gemini-3.1-pro-preview':          1_000_000,
    'gemini-3.1-pro-preview::high':    1_000_000,
    'gemini-3.1-pro-preview::medium':  1_000_000,
    'gemini-3-flash-preview':          1_000_000,
    // OpenAI GPT-5.6 — 1.05M input, 128K output (developers.openai.com)
    'gpt-5.6-sol':                     1_050_000,
    'gpt-5.6-terra':                   1_050_000,
    'gpt-5.6-luna':                    1_050_000,
    // xAI Grok (docs.x.ai) — note 4.5 is 500k, NOT 1M like the rest of the line
    'grok-4.5':                          500_000,
    'grok-4.3':                        1_000_000,
    // DeepSeek V4 (api-docs.deepseek.com) — 1M input, 384K max output
    'deepseek-v4-flash':               1_000_000,
    'deepseek-v4-pro':                 1_000_000,
    // Gemini 2.5 family (1M for pro, 1M for flash-lite preview)
    'gemini-2.5-flash':                1_000_000,
    'gemini-2.5-pro':                  2_000_000,
    'gemini-2.5-flash-lite-preview':   1_000_000,
    // Claude 5 & 4.x — every current model is 1M input; Haiku and the 4.5
    // generation are 200k. An id missing from this map falls back to 128k,
    // which makes a 1M-context model look nearly full on a normal session —
    // so add new ids here, not just to the picker.
    'claude-fable-5':                  1_000_000,
    'claude-opus-5':                   1_000_000,
    'claude-sonnet-5':                 1_000_000,
    'claude-opus-4-8':                 1_000_000,
    'claude-opus-4-7':                 1_000_000,
    'claude-sonnet-4-6':               1_000_000,
    'claude-haiku-4-5':                  200_000,
    'claude-opus-4-5':                   200_000,
    'claude-sonnet-4-5':                 200_000,
    // ── Legacy OpenAI, still selectable so pinned chats and runbooks resolve ──
    // These were all missing, so every one of them was denominated by the 128k
    // fallback: the chip read ~50% full on a session using a fifth of the real
    // budget, and the fix a user reaches for when the bar goes red is to compact
    // a conversation that did not need it.
    //
    // 272k is the last input window OpenAI published for the GPT-5 series (of a
    // 400k total, 128k of it output). They stopped publishing specs for these
    // when 5.6 replaced the line, so it cannot be re-verified — which is exactly
    // the case this file's own rule covers: take the lower advertised number,
    // never the total. Under-promising costs a user nothing; over-promising
    // truncates their prompt at the API with no warning from us.
    'gpt-5.5':                           272_000,
    'gpt-5.5-instant':                   272_000,
    'gpt-5.4-mini':                      272_000,
    'gpt-5.4-nano':                      272_000,
    'gpt-5.3-codex':                     272_000,
    'gpt-4o':                            128_000,
    'gpt-4o-mini':                       128_000,
};

/** Resolve the context-window (input tokens) for a model id. Strips
 *  effort suffixes like `::high` before lookup; falls back to 128k for
 *  unknown ids so the chip still renders SOMETHING instead of 0/idle.  */
export function contextWindowFor(modelId: string | null | undefined): number {
    if (!modelId) return 128_000;
    if (CONTEXT_WINDOWS[modelId]) return CONTEXT_WINDOWS[modelId];
    // Try without effort suffix (`gemini-3.1-pro-preview::high` → base id)
    const base = modelId.split('::')[0];
    if (CONTEXT_WINDOWS[base]) return CONTEXT_WINDOWS[base];
    // Provider-prefix heuristics for ids we don't fully enumerate
    if (modelId.startsWith('claude-'))  return 200_000;
    if (modelId.startsWith('gemini-'))  return 1_000_000;
    if (modelId.startsWith('ollama'))   return  32_768;   // common default
    if (modelId.includes('/'))           return 128_000;   // NIM owner/model
    return 128_000;
}

/**
 * Validate a runtime model string. Returns the input unchanged when
 * recognised; falls back to the FAST tier (with a console warning)
 * otherwise. Use this at the boundary where user-controlled or
 * config-loaded model ids enter the system — never silently send an
 * unverified string to `ask_lucy`, since the only failure mode is
 * the 401-like backend rejection followed by a silent catch.
 *
 * NB: non-Gemini ids are passed through untouched — they have separate
 * validation paths in the backend, and the real gate is ALLOWED_MODELS in
 * `src-tauri/src/state.rs`. Only Gemini is name-checked here, because the
 * fallback below is itself a Gemini model: sending an unknown Gemini id is a
 * plain rejection, whereas quietly rewriting someone's Grok id to Gemini is a
 * different model answering under the name they chose. That is what this did
 * to every OpenAI-dialect provider until the list of passthroughs caught up
 * with the catalog.
 */
export function resolveModelOrFallback(raw: string | null | undefined): string {
    if (!raw) return LLM.FAST;
    if (raw.includes('/'))     return raw;   // NVIDIA NIM owner/model format
    if (raw.startsWith('claude-')) return raw;
    if (raw.startsWith('ollama')) return raw;
    if (raw.startsWith('local-')) return raw;
    if (raw.startsWith('gpt-') || raw.startsWith('o1') || raw.startsWith('o3') || raw.startsWith('o4')) return raw;
    if (raw.startsWith('grok-')) return raw;
    if (raw.startsWith('deepseek-')) return raw;
    if (KNOWN_GEMINI_IDS.has(raw)) return raw;
    // Unknown id — log and fall back so the call doesn't silently
    // fail at the backend boundary.
    // eslint-disable-next-line no-console
    console.warn(`[llm-models] Unknown model id "${raw}", falling back to ${LLM.FAST}`);
    return LLM.FAST;
}
