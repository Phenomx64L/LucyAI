import { describe, it, expect } from 'vitest';
import { getProviderForModel, getDefaultModelForProvider, isRetryableProviderError } from './provider-fallback';

describe('getProviderForModel', () => {
    it('maps each model family to its provider', () => {
        expect(getProviderForModel('local-qwen2.5:7b')).toBe('local');
        expect(getProviderForModel('gemini-3.5-flash')).toBe('gemini');
        expect(getProviderForModel('claude-opus-4-8')).toBe('anthropic');
        expect(getProviderForModel('gpt-5.5')).toBe('openai');
        expect(getProviderForModel('o3-mini')).toBe('openai');
        expect(getProviderForModel('meta/llama-3.3-70b-instruct')).toBe('nvidia');
    });
    it('returns unknown for empty/unrecognised', () => {
        expect(getProviderForModel('')).toBe('unknown');
        expect(getProviderForModel(null)).toBe('unknown');
        expect(getProviderForModel('mistral-large')).toBe('unknown');
    });
});

describe('getDefaultModelForProvider', () => {
    it('returns a cheap default per cloud provider', () => {
        expect(getDefaultModelForProvider('gemini')).toBe('gemini-2.5-flash');
        expect(getDefaultModelForProvider('anthropic')).toBe('claude-haiku-4-5');
        expect(getDefaultModelForProvider('openai')).toBe('gpt-4o-mini');
        expect(getDefaultModelForProvider('nvidia')).toBe('meta/llama-3.3-70b-instruct');
    });
    it('returns null for local and unknown providers', () => {
        expect(getDefaultModelForProvider('local')).toBeNull();
        expect(getDefaultModelForProvider('whatever')).toBeNull();
    });
});

describe('isRetryableProviderError', () => {
    it('flags quota / 5xx / 429 / network / timeout as retryable', () => {
        expect(isRetryableProviderError('HTTP 429 rate limit')).toBe(true);
        expect(isRetryableProviderError('Error API HTTP 503')).toBe(true);
        expect(isRetryableProviderError('quota exceeded')).toBe(true);
        expect(isRetryableProviderError('ECONNREFUSED')).toBe(true);
        expect(isRetryableProviderError('fetch failed')).toBe(true);
        expect(isRetryableProviderError('Tiempo de espera agotado')).toBe(true);
    });
    it('does NOT flag auth / 401 / generic errors', () => {
        expect(isRetryableProviderError('HTTP 401 invalid x-api-key')).toBe(false);
        expect(isRetryableProviderError('malformed payload')).toBe(false);
        expect(isRetryableProviderError('')).toBe(false);
        expect(isRetryableProviderError(null)).toBe(false);
    });
});
