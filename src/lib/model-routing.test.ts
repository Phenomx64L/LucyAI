import { describe, it, expect } from 'vitest';
import { providerFamily } from './model-routing';

describe('providerFamily', () => {
    it('classifies each vendor prefix', () => {
        expect(providerFamily('local-qwen2.5:7b')).toBe('ollama');
        expect(providerFamily('ollama/llama3')).toBe('ollama');
        expect(providerFamily('gemini-3.5-flash')).toBe('gemini');
        expect(providerFamily('models/gemini-pro')).toBe('gemini');
        expect(providerFamily('claude-opus-4-8')).toBe('anthropic');
        expect(providerFamily('anthropic.claude')).toBe('anthropic');
        expect(providerFamily('gpt-5.5')).toBe('openai');
        expect(providerFamily('o3-mini')).toBe('openai');
        expect(providerFamily('meta/llama-3.3-70b-instruct')).toBe('nvidia');
        expect(providerFamily('nvidia/nemotron')).toBe('nvidia');
    });

    it('is case-insensitive and strips effort suffixes naturally', () => {
        expect(providerFamily('CLAUDE-OPUS-4-8::xhigh')).toBe('anthropic');
        expect(providerFamily('Gemini-3.1-Pro-Preview::high')).toBe('gemini');
    });

    it('returns "unknown" for unrecognised / empty ids', () => {
        expect(providerFamily('')).toBe('unknown');
        expect(providerFamily(null)).toBe('unknown');
        expect(providerFamily(undefined)).toBe('unknown');
        expect(providerFamily('mistral-large')).toBe('unknown');
    });
});
