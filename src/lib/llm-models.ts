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
    'gemini-3.5-flash',
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
    'gemini-3.5-flash':                1_000_000,
    'gemini-3.1-flash-lite-preview':   1_000_000,
    'gemini-3.1-flash-lite':           1_000_000,
    'gemini-3.1-pro-preview':          1_000_000,
    'gemini-3.1-pro-preview::high':    1_000_000,
    'gemini-3.1-pro-preview::medium':  1_000_000,
    'gemini-3-flash-preview':          1_000_000,
    // Gemini 2.5 family (1M for pro, 1M for flash-lite preview)
    'gemini-2.5-flash':                1_000_000,
    'gemini-2.5-pro':                  2_000_000,
    'gemini-2.5-flash-lite-preview':   1_000_000,
    // Claude 4.x — Opus 4.8/4.7 & Sonnet 4.6 are 1M input; Haiku/older are 200k.
    'claude-opus-4-8':                 1_000_000,
    'claude-opus-4-7':                 1_000_000,
    'claude-sonnet-4-6':               1_000_000,
    'claude-haiku-4-5':                  200_000,
    'claude-opus-4-5':                   200_000,
    'claude-sonnet-4-5':                 200_000,
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
 * NB: Claude/Ollama ids are passed through untouched — they have
 * separate validation paths in the backend.
 */
export function resolveModelOrFallback(raw: string | null | undefined): string {
    if (!raw) return LLM.FAST;
    if (raw.includes('/'))     return raw;   // NVIDIA NIM owner/model format
    if (raw.startsWith('claude-')) return raw;
    if (raw.startsWith('ollama')) return raw;
    if (KNOWN_GEMINI_IDS.has(raw)) return raw;
    // Unknown id — log and fall back so the call doesn't silently
    // fail at the backend boundary.
    // eslint-disable-next-line no-console
    console.warn(`[llm-models] Unknown model id "${raw}", falling back to ${LLM.FAST}`);
    return LLM.FAST;
}
