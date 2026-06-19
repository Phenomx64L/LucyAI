// ── artifacts.ts — pure artifact-candidate heuristic ─────────────────────────
//
// Extracted from +page.svelte (refactor, v1.7.196). The stateful
// `_promoteToArtifact` (which mutates the component's artifact list) stays in
// the component; only this pure detector moves out so it can be tested.

export interface ArtifactCandidate {
    kind: 'code' | 'markdown';
    language: string;
    content: string;
    title: string;
}

/**
 * Light heuristic — does this chat message look worth promoting to the artifact
 * panel? (was `_artifactCandidateOf` in +page.svelte). Returns the candidate
 * descriptor, or null when nothing substantial is present.
 *
 * Triggers:
 *   • a fenced code block of ≥ 30 lines, or
 *   • a markdown body ≥ 1500 chars (after stripping <TAG>…</TAG> scaffolding)
 *     that contains headings or list structure.
 */
export function artifactCandidateOf(rawContent: string | null | undefined): ArtifactCandidate | null {
    if (!rawContent) return null;
    const s = String(rawContent);
    // Fenced code block ≥ 30 lines is the primary trigger.
    const codeFence = s.match(/```([a-zA-Z0-9_-]*)\n([\s\S]+?)```/);
    if (codeFence) {
        const lang = codeFence[1];
        const body = codeFence[2];
        if (body.split('\n').length >= 30) {
            return { kind: 'code', language: lang || '', content: body.trim(), title: (lang || 'code') + ' block' };
        }
    }
    // Markdown body > 1500 chars (after stripping common tags) qualifies.
    const stripped = s.replace(/<[A-Z_]+>[\s\S]*?<\/[A-Z_]+>/g, '').trim();
    if (stripped.length >= 1500 && /^#{1,3}\s|\n#{1,3}\s|\n\s*[-*]\s/.test(stripped)) {
        const firstH = stripped.match(/^#\s+(.+)/m);
        return { kind: 'markdown', language: '', content: stripped, title: firstH ? firstH[1] : 'Document' };
    }
    return null;
}
