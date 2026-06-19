// ── provider-fallback.ts — pure provider/model fallback helpers ──────────────
//
// Extracted from +page.svelte (refactor, v1.7.197). The async picker
// `_findFallbackModel` stays in the component (it calls invoke); these three
// PURE building blocks move out so the routing logic is unit-tested.

export type Provider = 'local' | 'gemini' | 'anthropic' | 'openai' | 'nvidia' | 'unknown';

/** Classify a model id into the provider whose API serves it. */
export function getProviderForModel(model: string | null | undefined): Provider {
    if (!model) return 'unknown';
    const m = String(model).toLowerCase();
    if (m.startsWith('local-'))       return 'local';
    if (m.startsWith('gemini'))       return 'gemini';
    if (m.startsWith('claude'))       return 'anthropic';
    if (m.startsWith('gpt') || m.startsWith('o1') || m.startsWith('o3') || m.startsWith('o4')) return 'openai';
    if (m.includes('/') && !m.startsWith('local-')) return 'nvidia'; // NIM model ids contain '/'
    return 'unknown';
}

/**
 * Fast, cheap default model per provider — used when falling back from a
 * failing primary. `local` returns null (needs the user's installed model).
 */
export function getDefaultModelForProvider(provider: string): string | null {
    switch (provider) {
        case 'gemini':    return 'gemini-2.5-flash';
        case 'anthropic': return 'claude-haiku-4-5';
        case 'openai':    return 'gpt-4o-mini';
        case 'nvidia':    return 'meta/llama-3.3-70b-instruct';
        case 'local':     return null;
        default:          return null;
    }
}

/**
 * Does this error mean the primary provider is unrecoverable for this request
 * (quota, persistent 5xx, network) and a fallback is warranted? Auth/payload/
 * cancel errors return false — those should fail loud, not silently degrade.
 */
export function isRetryableProviderError(err: unknown): boolean {
    const s = String(err ?? '').toLowerCase();
    return /tras\s+\d+\s+intentos|http\s+(429|5\d\d)|rate.?limit|quota|overload|tiempo de espera|timeout|econnrefused|enotfound|network|fetch failed/i.test(s);
}
