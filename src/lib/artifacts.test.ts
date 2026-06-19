import { describe, it, expect } from 'vitest';
import { artifactCandidateOf } from './artifacts';

describe('artifactCandidateOf', () => {
    it('promotes a fenced code block of >= 30 lines', () => {
        const body = Array.from({ length: 32 }, (_, i) => `line ${i}`).join('\n');
        const c = artifactCandidateOf('intro\n```python\n' + body + '\n```\nrest');
        expect(c).not.toBeNull();
        expect(c!.kind).toBe('code');
        expect(c!.language).toBe('python');
        expect(c!.title).toBe('python block');
        expect(c!.content.startsWith('line 0')).toBe(true);
    });

    it('does NOT promote a short code block', () => {
        const c = artifactCandidateOf('```js\nconsole.log(1)\n```');
        expect(c).toBeNull();
    });

    it('promotes a long markdown body with headings', () => {
        const c = artifactCandidateOf('# Title\n\n' + 'x'.repeat(1600));
        expect(c).not.toBeNull();
        expect(c!.kind).toBe('markdown');
        expect(c!.title).toBe('Title');
    });

    it('ignores <TAG> scaffolding when measuring markdown length', () => {
        const tagged = '<THOUGHT>' + 'z'.repeat(3000) + '</THOUGHT>\nshort';
        expect(artifactCandidateOf(tagged)).toBeNull();
    });

    it('returns null for empty / falsy input', () => {
        expect(artifactCandidateOf('')).toBeNull();
        expect(artifactCandidateOf(null)).toBeNull();
        expect(artifactCandidateOf(undefined)).toBeNull();
    });
});
