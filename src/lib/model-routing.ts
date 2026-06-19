// ── model-routing.ts — pure model-id helpers ─────────────────────────────────
//
// Extracted from +page.svelte (refactor, v1.7.196). Pure string logic only —
// the state-dependent pickers (pickSubAgentModel / pickCrossVerifierModel) will
// move here in a later batch once their dependencies are parameterised.

export type ProviderFamily =
    | 'ollama' | 'gemini' | 'anthropic' | 'openai' | 'nvidia' | 'unknown';

/**
 * Classify a model id into its provider family (was `_providerFamily` in
 * +page.svelte). Used by the cross-model verifier so a review is routed to a
 * DIFFERENT family than the main agent (uncorrelated failure modes).
 */
export function providerFamily(modelId: string | null | undefined): ProviderFamily {
    const m = (modelId || '').toLowerCase();
    if (m.startsWith('local-') || m.startsWith('ollama')) return 'ollama';
    if (m.startsWith('gemini') || m.startsWith('models/')) return 'gemini';
    if (m.startsWith('claude') || m.startsWith('anthropic')) return 'anthropic';
    if (m.startsWith('gpt') || m.startsWith('o1') || m.startsWith('o3') || m.startsWith('openai')) return 'openai';
    if (m.startsWith('meta/') || m.startsWith('nvidia')) return 'nvidia';
    return 'unknown';
}
